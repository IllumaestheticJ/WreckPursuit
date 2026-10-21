//! The built-in driver's turn-round manoeuvre (owner decision 2026-10-21: "model real
//! three-point turns").
//!
//! A headless unit whose track starts behind it (a new route the other way, a racer that went
//! past) used to swing round at a 4 m/s crawl on full lock wherever it was, sweeping two
//! turning radii beside its track whatever the road's width. Now [`step_turn_round`] drives a
//! deterministic state machine instead:
//!
//! 1. **Slow**: brake in a straight line to [`UTURN_MPS`].
//! 2. **Plan** on the road surface round the car ([`crate::graph::RoadSpan`]: its segment plus
//!    the side roads at either end, refreshed by the police at think rate into [`Unit::road`]):
//!    if one forward sweep on full lock fits it is a **U-turn** (after a short swing out the
//!    other way, up to [`MAX_SWING_RAD`], when that is what makes it fit: a car in the inner
//!    lane of a 2+2 road); otherwise a **three-point turn**: starting forward or in reverse,
//!    turning left or right, near a junction after moving straight up to or back into the
//!    side road, whichever finishes soonest (each is simulated with the live leg logic). A
//!    car near a side road turns into it and reverses out, or reverses into it and drives
//!    out.
//! 3. **Legs**: forward on full lock until aligned with the track (done) or until the car would
//!    come within [`KERB_MARGIN_M`] of a kerb, **pause** [`RELOCK_S`] to swing the wheel,
//!    **reverse** on the opposite lock until a forward sweep fits (or the rear reaches a
//!    kerb), pause, forward again. Legs repeat on a very narrow road (a five-point turn), up to
//!    [`MAX_LEGS`]. A leg whose lock would not move the car [`MIN_LEG_M`] (an inside kerb
//!    corner in a junction) goes straight instead if that gets farther. Nearly round (within
//!    [`TURN_HANDOVER_COS`]) with the kerb in the way and the road clear straight on, the
//!    normal driver takes over. Boxed in ([`MAX_STALLED_LEGS`] legs without progress), out of
//!    legs or out of time ([`MAX_TURN_S`]), the unit gives up and the normal driver swings
//!    round the old way until the track is no longer behind.
//!
//! The goal is the track's direction where the car's position at the plan projects onto it
//! (the car's own projection jumps about a junction bend while it manoeuvres). A new command
//! ends the manoeuvre only if its track lies ahead of the heading ([`Unit::set_command`]);
//! the police's 3-s re-plans skip a unit that is turning round and [`accepts_replan`] keeps
//! the others from undoing half a turn (both in `pursuit.rs`).
//!
//! The car is a rectangle round `UnitBody::pos` (half length `wheelbase / 2 +`
//! [`CAR_OVERHANG_M`], half width [`crate::avoid::UNIT_HALF_WIDTH_M`]) turning about its rear
//! axle on the driver's kinematic bicycle (path curvature `tan(full lock) / wheelbase`). Its
//! corners and the middles of its sides stay on the surface at every step, so the manoeuvre
//! never trips the driver's "ran wide" crash check. All arithmetic is plain `f32` with
//! `wp_math`'s trigonometry, in a fixed order: bit-identical on every target.
//!
//! One-way carriageways (the viaduct): a unit facing the legal way never turns round on the
//! deck. Its routes never ask it to (`route_from` starts at the node ahead, and a one-way
//! segment is never travelled back), so this only matters for a pursuer whose racer went
//! the wrong way past it; it keeps the old full-lock swing then (README, known gaps).

use serde::{Deserialize, Serialize};
use wp_contracts::GRAVITY_MPS2;
use wp_math::{atan2, sin_cos};

use crate::avoid::UNIT_HALF_WIDTH_M;
use crate::graph::{add, cross, dot, len, normalize, scale, sub, RoadRect, RoadSpan};
use crate::unit::{Track, Unit};

/// A unit turns round when its track 8 m ahead lies more than ~100° off its heading. Up to
/// that the normal driver steers (a junction turn is ≤ 90°).
pub const TURN_ROUND_COS: f32 = -0.17;
/// The manoeuvre is over once the heading is within ~20° of the track direction.
pub const TURN_ALIGN_COS: f32 = 0.94;
/// ...or within ~45° when the kerb stops it turning further and the road straight on is
/// clear (the normal driver straightens up).
pub const TURN_HANDOVER_COS: f32 = 0.7;
/// A new command during a manoeuvre ends it only if the new track lies within ~45° of the
/// heading ([`Unit::set_command`]).
pub const TURN_ABANDON_COS: f32 = 0.7;
/// Speed of a U-turn sweep, m/s (≈ 0.4 g on full lock).
pub const UTURN_MPS: f32 = 4.0;
/// Forward legs of a three-point turn, m/s.
pub const THREE_POINT_FWD_MPS: f32 = 2.5;
/// Reverse legs of a three-point turn, m/s.
pub const THREE_POINT_REV_MPS: f32 = 1.5;
/// Acceleration during the manoeuvre, m/s².
pub const TURN_ACCEL_MPS2: f32 = 2.5;
/// Braking during the manoeuvre (legs end exactly at the kerb margin), m/s².
pub const TURN_BRAKE_MPS2: f32 = 5.0;
/// Standstill between legs while the wheel goes from lock to lock, s.
pub const RELOCK_S: f32 = 0.6;
/// Closest any corner of the car comes to a kerb, m.
pub const KERB_MARGIN_M: f32 = 0.3;
/// Body overhang beyond each axle, m (the car's half length is `wheelbase / 2 +` this).
pub const CAR_OVERHANG_M: f32 = 1.0;
/// After this many forward/reverse legs the unit gives up and swings round the old way (the
/// normal driver's full-lock steering at a crawl).
pub const MAX_LEGS: u8 = 7;
/// A leg that moves less than this made no progress, m.
pub const MIN_PROGRESS_M: f32 = 0.2;
/// ...and after this many such legs in a row the unit gives up.
pub const MAX_STALLED_LEGS: u8 = 2;
/// ...or after this long, s.
pub const MAX_TURN_S: f32 = 30.0;
/// Largest swing out away from the turn before a U-turn sweep, rad (45°).
pub const MAX_SWING_RAD: f32 = 0.785;
/// A leg goes on its preferred lock if that lets it move at least this far (else straight,
/// if that goes farther), m.
pub const MIN_LEG_M: f32 = 0.5;

/// Swing angles tried, rad (0°, 15°, 30°, 45°).
const SWINGS_RAD: [f32; 4] = [0.0, 0.262, 0.524, MAX_SWING_RAD];
/// Kerb look-ahead along the arc of a leg, m (more than the stopping distance from
/// [`UTURN_MPS`]).
const FREE_HORIZON_M: f32 = 3.0;
/// Step of the kerb look-ahead, m.
const FREE_STEP_M: f32 = 0.1;
/// Step of the forward-sweep check, m.
const SWEEP_STEP_M: f32 = 0.1;
/// The forward-sweep check keeps this much more off the kerbs than the legs do, so that a
/// sweep judged to fit is never stopped by the legs' own kerb check (they sample other
/// points of the same arc), m.
const SWEEP_SLACK_M: f32 = 0.05;
/// A leg stops this short of where the car would reach the kerb margin, m.
const STOP_SHORT_M: f32 = 0.05;
/// Remaining rotation (in the manoeuvre's sense) beyond which the goal has moved round
/// behind the turn and the manoeuvre is planned again, rad (π + ~30°).
const REPLAN_ROTATION_RAD: f32 = std::f32::consts::PI + 0.5;
/// Time step of the planning simulation of a three-point turn, s.
const PLAN_DT_S: f32 = 1.0 / 15.0;
/// Side roads farther than this from the car are left out of the plan, m.
pub const PLAN_REACH_M: f32 = 15.0;
/// Straight approaches tried before a three-point turn near a side road, m.
pub const APPROACH_M: [f32; 4] = [0.0, 3.0, 6.0, 9.0];

/// U-turn or three-point turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnKind {
    /// One forward sweep on full lock.
    UTurn,
    /// Forward, reverse, forward (more legs if the road is very narrow).
    ThreePoint,
}

/// Where the manoeuvre is.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TurnLeg {
    /// Braking straight to [`UTURN_MPS`] before planning.
    Slow,
    /// The plan's [`Approach`] before the first full-lock leg.
    Approach {
        /// Arc length still to drive, m.
        left_m: f32,
    },
    /// Forward; `lock` is the steering as a multiple of full lock (+ = left).
    Forward {
        /// -1, 0 or +1.
        lock: f32,
    },
    /// Reversing; `lock` as for `Forward`.
    Reverse {
        /// -1, 0 or +1.
        lock: f32,
    },
    /// Stopped while the wheel goes lock to lock; then forward (`true`) or reverse.
    Pause {
        /// The next leg is forward.
        forward: bool,
        /// Time left, s.
        left_s: f32,
    },
    /// Hit [`MAX_LEGS`] / [`MAX_TURN_S`]: the normal driver swings round until the track is
    /// no longer behind.
    GaveUp,
}

/// What the manoeuvre is, decided when the unit is slow enough ([`plan_turn`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TurnPlan {
    /// U-turn or three-point turn.
    pub kind: TurnKind,
    /// Turn sense: +1 left (anticlockwise seen from above), -1 right.
    pub dir: f32,
    /// First full-lock leg forward (else reversing).
    pub start_forward: bool,
    /// Short drive before it (a U-turn's swing out, a three-point turn's move up to or back
    /// into a side road).
    pub approach: Approach,
    /// The surface, with the car's footprint at the start added if it was not all on it (a
    /// car already over a kerb may stay there but goes no further).
    pub road: RoadSpan,
    /// Where the car was when the turn was planned: the goal is the track's direction where
    /// this point projects onto it (stable while the car moves about; follows a new track).
    #[serde(default)]
    pub anchor: [f32; 2],
}

/// A short drive at a fixed lock before a manoeuvre's first full-lock leg, in that leg's
/// direction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Approach {
    /// Arc length, m (0 = none).
    pub arc_m: f32,
    /// Steering as a multiple of full lock (+ = left).
    pub lock: f32,
}

impl Approach {
    /// No approach.
    pub const NONE: Approach = Approach {
        arc_m: 0.0,
        lock: 0.0,
    };
}

/// A unit's turn-round manoeuvre in progress ([`Unit::turn`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TurnRound {
    /// Current leg.
    pub leg: TurnLeg,
    /// The plan (`None` while slowing).
    pub plan: Option<TurnPlan>,
    /// Forward and reverse legs started so far (a U-turn is 1, a three-point turn 3).
    pub legs: u8,
    /// Time in the manoeuvre, s.
    pub elapsed_s: f32,
    /// Distance driven in the current leg, m.
    #[serde(default)]
    pub leg_m: f32,
    /// Legs in a row that the kerbs stopped before they moved [`MIN_PROGRESS_M`] (boxed in:
    /// the manoeuvre gives up after [`MAX_STALLED_LEGS`]).
    #[serde(default)]
    pub stalled: u8,
}

impl TurnRound {
    /// A manoeuvre about to brake for its plan.
    pub fn slowing() -> Self {
        Self {
            leg: TurnLeg::Slow,
            plan: None,
            legs: 0,
            elapsed_s: 0.0,
            leg_m: 0.0,
            stalled: 0,
        }
    }

    /// Whether the unit is actually turning (planned, not given up).
    pub fn active(&self) -> bool {
        self.plan.is_some() && self.leg != TurnLeg::GaveUp
    }

    /// Whether it has run out of legs, time or progress.
    fn exhausted(&self, limit_s: f32) -> bool {
        self.elapsed_s > limit_s || self.legs > MAX_LEGS || self.stalled >= MAX_STALLED_LEGS
    }
}

/// Direction of `track` at arc length `s` (unit vector; `[0, 0]` for a degenerate track).
pub fn track_dir(track: &Track, s: f32) -> [f32; 2] {
    normalize(sub(track.point_at(s + 2.0), track.point_at(s)))
}

/// Cosine between `heading` and the direction from `pos` to the point 8 m along `track` from
/// arc length `s` (1 when that point is within a metre).
pub fn track_ahead_cos(track: &Track, s: f32, pos: [f32; 2], heading: [f32; 2]) -> f32 {
    let to = sub(track.point_at(s + 8.0), pos);
    if len(to) <= 1.0 {
        return 1.0;
    }
    dot(normalize(to), heading)
}

/// Whether `u` has to turn round to start on its track: the track 8 m ahead lies more than
/// ~100° off its heading ([`TURN_ROUND_COS`]), and so does the track's own direction where
/// the unit is (short of a sharp junction turn the track there still runs ahead: the normal
/// driver takes the corner).
pub fn needs_turn_round(u: &Unit) -> bool {
    let t = &u.cmd.track;
    t.is_drivable()
        && track_ahead_cos(t, u.track_s, u.body.pos, u.body.heading) < TURN_ROUND_COS
        && dot(track_dir(t, u.track_s), u.body.heading) < TURN_ROUND_COS
}

/// Path curvature on full lock, 1/m.
pub fn full_lock_curvature(u: &Unit) -> f32 {
    let (s, c) = sin_cos(u.geometry.max_steer_rad);
    s / c.max(1e-3) / u.geometry.wheelbase_m.max(0.5)
}

/// The unit's car as the manoeuvre sees it, m.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CarBox {
    /// Half length (`wheelbase / 2 +` [`CAR_OVERHANG_M`]).
    half_len: f32,
    /// Half width.
    half_w: f32,
    /// Half wheelbase: `UnitBody::pos` (the car's middle) is this far ahead of the rear axle.
    half_wb: f32,
}

/// Size of the unit's car.
fn car_box(u: &Unit) -> CarBox {
    let half_wb = u.geometry.wheelbase_m.max(0.5) * 0.5;
    CarBox {
        half_len: half_wb + CAR_OVERHANG_M,
        half_w: UNIT_HALF_WIDTH_M,
        half_wb,
    }
}

/// Pose of a point after it drives signed arc length `ds` (negative = reversing) at path
/// curvature `kappa` (1/m, + = turning left when going forward).
pub fn arc_pose(pos: [f32; 2], heading: [f32; 2], kappa: f32, ds: f32) -> ([f32; 2], [f32; 2]) {
    let phi = kappa * ds;
    let left = [-heading[1], heading[0]];
    if phi.abs() < 1e-5 {
        return (add(pos, scale(heading, ds)), heading);
    }
    let (s, c) = sin_cos(phi);
    let fwd = s / kappa;
    let side = (1.0 - c) / kappa;
    let p = add(pos, add(scale(heading, fwd), scale(left, side)));
    let h = normalize([
        heading[0] * c - heading[1] * s,
        heading[0] * s + heading[1] * c,
    ]);
    (p, h)
}

/// Pose of the car's middle after its rear axle (the point a car turns about) drives signed
/// arc length `ds` at curvature `kappa` ([`arc_pose`]).
fn car_arc(
    pos: [f32; 2],
    heading: [f32; 2],
    kappa: f32,
    ds: f32,
    car: CarBox,
) -> ([f32; 2], [f32; 2]) {
    let rear = sub(pos, scale(heading, car.half_wb));
    let (r, h) = arc_pose(rear, heading, kappa, ds);
    (add(r, scale(h, car.half_wb)), h)
}

/// The points of the car checked against the kerbs: its corners and the middles of its long
/// sides (so a kerb corner in a junction cannot cut the car between two corners).
fn outline(pos: [f32; 2], heading: [f32; 2], car: CarBox) -> [[f32; 2]; 6] {
    let left = [-heading[1], heading[0]];
    let at = |lx: f32, ly: f32| add(pos, add(scale(heading, lx), scale(left, ly)));
    [
        at(car.half_len, car.half_w),
        at(car.half_len, -car.half_w),
        at(-car.half_len, car.half_w),
        at(-car.half_len, -car.half_w),
        at(0.0, car.half_w),
        at(0.0, -car.half_w),
    ]
}

/// Whether the whole car is on `road` with `margin` to spare from the kerbs.
fn on_road(road: &RoadSpan, pos: [f32; 2], heading: [f32; 2], car: CarBox, margin: f32) -> bool {
    outline(pos, heading, car)
        .iter()
        .all(|p| road.contains(*p, margin))
}

/// `road` plus the car's footprint (with margin) when the car is not all on it already.
fn with_footprint(road: RoadSpan, pos: [f32; 2], heading: [f32; 2], car: CarBox) -> RoadSpan {
    if on_road(&road, pos, heading, car, KERB_MARGIN_M) {
        return road;
    }
    let mut r = road;
    let m = 2.0 * KERB_MARGIN_M;
    r.add_piece(RoadRect {
        origin: sub(pos, scale(heading, car.half_len - car.half_w)),
        axis: heading,
        len_m: 2.0 * (car.half_len - car.half_w),
        left_m: car.half_w + m,
        right_m: car.half_w + m,
    });
    r
}

/// How far the car can drive along its arc (`sign` +1 forward, -1 reverse) before it would
/// come within the kerb margin, m (capped at [`FREE_HORIZON_M`]).
fn free_arc(
    road: &RoadSpan,
    pos: [f32; 2],
    heading: [f32; 2],
    sign: f32,
    kappa: f32,
    car: CarBox,
) -> f32 {
    let n = (FREE_HORIZON_M / FREE_STEP_M) as u32;
    for k in 1..=n {
        let d = k as f32 * FREE_STEP_M;
        let (p, h) = car_arc(pos, heading, kappa, sign * d, car);
        if !on_road(road, p, h, car, KERB_MARGIN_M) {
            return d - FREE_STEP_M;
        }
    }
    FREE_HORIZON_M
}

/// Whether driving `arc_m` (`sign` +1 forward, -1 reverse) at curvature `kappa` keeps the
/// car on `road`.
fn arc_on_road(
    road: &RoadSpan,
    pos: [f32; 2],
    heading: [f32; 2],
    sign: f32,
    kappa: f32,
    arc_m: f32,
    car: CarBox,
) -> bool {
    let n = (arc_m / SWEEP_STEP_M) as u32 + 1;
    (1..=n).all(|k| {
        let d = (k as f32 * SWEEP_STEP_M).min(arc_m);
        let (p, h) = car_arc(pos, heading, kappa, sign * d, car);
        on_road(road, p, h, car, KERB_MARGIN_M + SWEEP_SLACK_M)
    })
}

/// Whether one sweep on full lock turning the car in sense `dir` (`sign` +1: forward on that
/// lock; -1: reversing on the opposite lock) brings the heading within [`TURN_ALIGN_COS`] of
/// `goal` with the car on `road` all the way.
#[allow(clippy::too_many_arguments)]
fn sweep_fits(
    road: &RoadSpan,
    pos: [f32; 2],
    heading: [f32; 2],
    sign: f32,
    dir: f32,
    goal: [f32; 2],
    kmax: f32,
    car: CarBox,
) -> bool {
    let kappa = sign * dir * kmax;
    // At most 225° of rotation.
    let max_len = 1.25 * std::f32::consts::PI / kmax.max(1e-3);
    let n = (max_len / SWEEP_STEP_M) as u32 + 1;
    for k in 0..=n {
        let (p, h) = car_arc(pos, heading, kappa, sign * k as f32 * SWEEP_STEP_M, car);
        if !on_road(road, p, h, car, KERB_MARGIN_M + SWEEP_SLACK_M) {
            return false;
        }
        if dot(h, goal) >= TURN_ALIGN_COS {
            return true;
        }
    }
    false
}

/// Rotation from `heading` to `goal` in sense `dir` (+1 anticlockwise), rad in `[0, 2π)`.
fn rotation_to(heading: [f32; 2], goal: [f32; 2], dir: f32) -> f32 {
    let mut r = atan2(cross(heading, goal), dot(heading, goal)) * dir;
    if r < 0.0 {
        r += 2.0 * std::f32::consts::PI;
    }
    r
}

/// Kinematic state the legs act on.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Body {
    pos: [f32; 2],
    heading: [f32; 2],
    /// Signed speed, m/s (negative reversing).
    speed: f32,
}

/// Advance the legs of a planned manoeuvre by `dt` toward `goal`. Returns `false` once the
/// heading is aligned on a forward leg (the manoeuvre is over; the body was not moved).
/// Shared by the live driver and the planner's simulation, so a plan is what gets driven.
fn advance_legs(
    b: &mut Body,
    t: &mut TurnRound,
    goal: [f32; 2],
    car: CarBox,
    kmax: f32,
    dt: f32,
) -> bool {
    let Some(mut plan) = t.plan else {
        return false;
    };
    if let TurnLeg::Forward { lock } = t.leg {
        let c = dot(b.heading, goal);
        if c >= TURN_ALIGN_COS {
            return false;
        }
        // Nearly round, with the kerb in the way of turning further but room straight on:
        // the normal driver straightens up from here.
        if c >= TURN_HANDOVER_COS
            && free_arc(&plan.road, b.pos, b.heading, 1.0, lock * kmax, car) < MIN_LEG_M
            && free_arc(&plan.road, b.pos, b.heading, 1.0, 0.0, car) >= FREE_HORIZON_M
        {
            return false;
        }
    }
    match t.leg {
        TurnLeg::Approach { left_m } => {
            let sign = if plan.start_forward { 1.0 } else { -1.0 };
            let kappa = plan.approach.lock * kmax;
            let free = free_arc(&plan.road, b.pos, b.heading, sign, kappa, car);
            let room = (free - STOP_SHORT_M).max(0.0);
            let v_allow =
                leg_speed(&plan, plan.start_forward).min((2.0 * TURN_BRAKE_MPS2 * room).sqrt());
            let cur = (b.speed * sign).max(0.0);
            let v = cur + (v_allow - cur).clamp(-TURN_BRAKE_MPS2 * dt, TURN_ACCEL_MPS2 * dt);
            let ds = (v * dt).min(left_m).min(free.max(0.0));
            (b.pos, b.heading) = car_arc(b.pos, b.heading, kappa, sign * ds, car);
            b.speed = sign * v;
            t.leg_m += ds;
            // Done, or blocked by the kerb: on to the first full-lock leg (its own kerb
            // check stops it if need be).
            t.leg = if left_m - ds <= 0.0 || free < STOP_SHORT_M + FREE_STEP_M {
                main_leg(&plan)
            } else {
                TurnLeg::Approach {
                    left_m: left_m - ds,
                }
            };
        }
        TurnLeg::Pause { forward, left_s } => {
            b.speed = 0.0;
            let left_s = left_s - dt;
            t.leg = if left_s > 0.0 {
                TurnLeg::Pause { forward, left_s }
            } else {
                t.legs = t.legs.saturating_add(1);
                let lock = choose_lock(&plan, b, forward, car, kmax);
                if forward {
                    TurnLeg::Forward { lock }
                } else {
                    // A U-turn that met the kerb finishes as a three-point turn.
                    plan.kind = TurnKind::ThreePoint;
                    t.plan = Some(plan);
                    TurnLeg::Reverse { lock }
                }
            };
        }
        TurnLeg::Forward { lock } | TurnLeg::Reverse { lock } => {
            let forward = matches!(t.leg, TurnLeg::Forward { .. });
            let sign = if forward { 1.0 } else { -1.0 };
            let kappa = lock * kmax;
            let v_leg = leg_speed(&plan, forward);
            let free = free_arc(&plan.road, b.pos, b.heading, sign, kappa, car);
            let room = (free - STOP_SHORT_M).max(0.0);
            let mut v_allow = v_leg.min((2.0 * TURN_BRAKE_MPS2 * room).sqrt());
            // Stop as soon as the other direction can finish the turn in one sweep: reversing,
            // once a forward sweep fits; forward (into a side road), once reversing out does
            // and going on forward does not. A U-turn's own sweep is never cut short.
            let finish = if forward {
                plan.kind == TurnKind::ThreePoint
                    && sweep_fits(
                        &plan.road, b.pos, b.heading, -1.0, plan.dir, goal, kmax, car,
                    )
                    && !sweep_fits(&plan.road, b.pos, b.heading, 1.0, plan.dir, goal, kmax, car)
            } else {
                sweep_fits(&plan.road, b.pos, b.heading, 1.0, plan.dir, goal, kmax, car)
            };
            if finish {
                v_allow = 0.0;
            }
            let cur = (b.speed * sign).max(0.0);
            let v = cur + (v_allow - cur).clamp(-TURN_BRAKE_MPS2 * dt, TURN_ACCEL_MPS2 * dt);
            if v_allow < 0.05 && v < 0.05 {
                b.speed = 0.0;
                // Boxed in: stopped by the kerbs (not to finish the other way) with no
                // progress.
                t.stalled = if !finish && t.leg_m < MIN_PROGRESS_M {
                    t.stalled.saturating_add(1)
                } else {
                    0
                };
                t.leg_m = 0.0;
                t.leg = TurnLeg::Pause {
                    forward: !forward,
                    left_s: RELOCK_S,
                };
            } else {
                let ds = (v * dt).min(free.max(0.0));
                (b.pos, b.heading) = car_arc(b.pos, b.heading, kappa, sign * ds, car);
                b.speed = sign * v;
                t.leg_m += ds;
            }
        }
        TurnLeg::Slow | TurnLeg::GaveUp => {}
    }
    true
}

/// Top speed of a leg of `plan`, m/s.
fn leg_speed(plan: &TurnPlan, forward: bool) -> f32 {
    match (forward, plan.kind) {
        (true, TurnKind::UTurn) => UTURN_MPS,
        (true, TurnKind::ThreePoint) => THREE_POINT_FWD_MPS,
        (false, _) => THREE_POINT_REV_MPS,
    }
}

/// The first full-lock leg of `plan`.
fn main_leg(plan: &TurnPlan) -> TurnLeg {
    if plan.start_forward {
        TurnLeg::Forward { lock: plan.dir }
    } else {
        TurnLeg::Reverse { lock: -plan.dir }
    }
}

/// The first leg of `plan`.
fn first_leg(plan: &TurnPlan) -> TurnLeg {
    if plan.approach.arc_m > 0.0 {
        TurnLeg::Approach {
            left_m: plan.approach.arc_m,
        }
    } else {
        main_leg(plan)
    }
}

/// Steering for the next leg from standstill: the lock that turns the car the plan's way
/// (forward: toward the turn; reverse: the opposite lock), unless that would not move the
/// car [`MIN_LEG_M`] and going straight would move it farther (the nose against an inside
/// kerb corner). Never the other lock: that undoes the turn.
fn choose_lock(plan: &TurnPlan, b: &Body, forward: bool, car: CarBox, kmax: f32) -> f32 {
    let sign = if forward { 1.0 } else { -1.0 };
    let preferred = sign * plan.dir;
    let free = free_arc(&plan.road, b.pos, b.heading, sign, preferred * kmax, car);
    if free >= MIN_LEG_M {
        return preferred;
    }
    let straight = free_arc(&plan.road, b.pos, b.heading, sign, 0.0, car);
    if straight > free {
        0.0
    } else {
        preferred
    }
}

/// How long `plan` takes from `b` (simulated at [`PLAN_DT_S`] with the live leg logic),
/// s; `None` if it does not finish within `limit_s` or [`MAX_LEGS`].
fn simulate(
    plan: TurnPlan,
    b: Body,
    goal: [f32; 2],
    car: CarBox,
    kmax: f32,
    limit_s: f32,
) -> Option<f32> {
    let mut b = b;
    let mut t = TurnRound {
        leg: first_leg(&plan),
        plan: Some(plan),
        legs: 1,
        ..TurnRound::slowing()
    };
    while !t.exhausted(limit_s) {
        if !advance_legs(&mut b, &mut t, goal, car, kmax, PLAN_DT_S) {
            return Some(t.elapsed_s);
        }
        t.elapsed_s += PLAN_DT_S;
    }
    None
}

/// Plan the turn-round of `u` on `road` toward the direction `goal`.
///
/// A U-turn if a forward sweep fits (after a swing out if need be; the goal's side first).
/// Otherwise the quickest of the three-point turns starting forward or in reverse, turning
/// left or right, and, with a side road within [`PLAN_REACH_M`], after moving straight up
/// to or back into it by one of [`APPROACH_M`]: each candidate is simulated with the live
/// leg logic (ties: the earlier candidate).
pub fn plan_turn(u: &Unit, road: RoadSpan, goal: [f32; 2]) -> TurnPlan {
    let car = car_box(u);
    let (pos, heading) = (u.body.pos, u.body.heading);
    let road = with_footprint(near_pieces(road, pos), pos, heading, car);
    let kmax = full_lock_curvature(u);
    let side = cross(heading, goal);
    let first = if side >= 0.0 { 1.0 } else { -1.0 };
    for swing in SWINGS_RAD {
        let swing_m = swing / kmax.max(1e-3);
        for dir in [first, -first] {
            // The swing itself has to stay on the road too.
            if swing_m > 0.0 && !arc_on_road(&road, pos, heading, 1.0, -dir * kmax, swing_m, car) {
                continue;
            }
            let (p, h) = car_arc(pos, heading, -dir * kmax, swing_m, car);
            if sweep_fits(&road, p, h, 1.0, dir, goal, kmax, car) {
                return TurnPlan {
                    kind: TurnKind::UTurn,
                    dir,
                    start_forward: true,
                    approach: Approach {
                        arc_m: swing_m,
                        lock: -dir,
                    },
                    road,
                    anchor: pos,
                };
            }
        }
    }
    let body = Body {
        pos,
        heading,
        speed: u.body.speed.max(0.0),
    };
    let junction = road.others.iter().flatten().next().is_some();
    let approaches: &[f32] = if junction {
        &APPROACH_M
    } else {
        &APPROACH_M[..1]
    };
    let mut best: Option<(f32, TurnPlan)> = None;
    for start_forward in [true, false] {
        for &arc_m in approaches {
            let sign = if start_forward { 1.0 } else { -1.0 };
            if arc_m > 0.0 && !arc_on_road(&road, pos, heading, sign, 0.0, arc_m, car) {
                continue;
            }
            for dir in [first, -first] {
                let plan = TurnPlan {
                    kind: TurnKind::ThreePoint,
                    dir,
                    start_forward,
                    approach: Approach { arc_m, lock: 0.0 },
                    road,
                    anchor: pos,
                };
                let limit = best.map_or(MAX_TURN_S, |(t, _)| t);
                if let Some(t) = simulate(plan, body, goal, car, kmax, limit) {
                    if best.is_none_or(|(bt, _)| t < bt) {
                        best = Some((t, plan));
                    }
                }
            }
        }
    }
    best.map_or(
        TurnPlan {
            kind: TurnKind::ThreePoint,
            dir: first,
            start_forward: true,
            approach: Approach::NONE,
            road,
            anchor: pos,
        },
        |(_, p)| p,
    )
}

/// `road` without the side pieces farther than [`PLAN_REACH_M`] from `pos` (a manoeuvre
/// never reaches them; fewer pieces to check).
fn near_pieces(road: RoadSpan, pos: [f32; 2]) -> RoadSpan {
    let mut r = road;
    r.others = [None; crate::graph::ROAD_SPAN_PIECES - 1];
    for piece in road.others.iter().flatten() {
        if piece.distance_to(pos) <= PLAN_REACH_M {
            r.add_piece(*piece);
        }
    }
    r
}

/// Whether a new track for `u` keeps its manoeuvre whole: no manoeuvre under way, or the new
/// track lies ahead of the heading (the manoeuvre ends, [`TURN_ABANDON_COS`]), or it is
/// reached by carrying on with the turn. A new track that would have the unit undo half a
/// turn is refused by the periodic re-plans (owner, 2026-10-21).
pub fn accepts_replan(u: &Unit, track: &Track) -> bool {
    let Some(plan) = u.turn.and_then(|t| if t.active() { t.plan } else { None }) else {
        return true;
    };
    if !track.is_drivable() {
        return true;
    }
    let (s, _) = track.project(u.body.pos, 0.0, f32::INFINITY);
    if track_ahead_cos(track, s, u.body.pos, u.body.heading) >= TURN_ABANDON_COS {
        return true;
    }
    rotation_to(u.body.heading, track_dir(track, s), plan.dir) <= REPLAN_ROTATION_RAD
}

/// Advance the turn-round manoeuvre of `u` by `dt`, starting one if its track lies behind it.
/// Returns whether it moved the unit this tick (then the normal driver must not).
/// `slow_brake_mps2` is the braking used to slow down before the turn.
pub fn step_turn_round(u: &mut Unit, slow_brake_mps2: f32, dt: f32) -> bool {
    let need = needs_turn_round(u);
    match u.turn {
        None => {
            let Some(road) = u.road else {
                return false; // scripted racers: the old swing
            };
            if !need || (road.oneway && dot(u.body.heading, road.own.axis) > 0.0) {
                return false;
            }
            u.turn = Some(TurnRound::slowing());
        }
        Some(t) if t.leg == TurnLeg::GaveUp => {
            if !need {
                u.turn = None;
            }
            return false;
        }
        Some(t) if t.leg == TurnLeg::Slow && !need => {
            u.turn = None;
            return false;
        }
        Some(_) => {}
    }
    let mut t = u.turn.expect("set above");
    t.elapsed_s += dt;
    if t.exhausted(MAX_TURN_S) {
        t.leg = TurnLeg::GaveUp;
        u.turn = Some(t);
        return false;
    }
    let goal = match t.plan {
        Some(p) => {
            let (s, _) = u.cmd.track.project(p.anchor, u.track_s, 60.0);
            track_dir(&u.cmd.track, s)
        }
        None => track_dir(&u.cmd.track, u.track_s),
    };
    if t.leg == TurnLeg::Slow {
        if u.body.speed > UTURN_MPS {
            let b = &mut u.body;
            b.speed = (b.speed - slow_brake_mps2 * dt).max(UTURN_MPS);
            b.pos = add(b.pos, scale(b.heading, b.speed * dt));
            b.lat_g = 0.0;
            u.turn = Some(t);
            return true;
        }
        let Some(road) = u.road else {
            t.leg = TurnLeg::GaveUp;
            u.turn = Some(t);
            return false;
        };
        // A unit rolling backwards (a physical sync) starts the first leg from rest.
        u.body.speed = u.body.speed.max(0.0);
        let plan = plan_turn(u, road, goal);
        t.plan = Some(plan);
        t.leg = first_leg(&plan);
        t.legs = 1;
    } else if matches!(t.leg, TurnLeg::Forward { .. }) {
        let plan = t.plan.expect("planned");
        if rotation_to(u.body.heading, goal, plan.dir) > REPLAN_ROTATION_RAD
            && dot(u.body.heading, goal) < TURN_ALIGN_COS
        {
            // The goal swung round behind the turn (a new track): plan again from here.
            let plan = plan_turn(u, plan.road, goal);
            t.plan = Some(plan);
            t.leg = first_leg(&plan);
        }
    }
    let car = car_box(u);
    let kmax = full_lock_curvature(u);
    let mut b = Body {
        pos: u.body.pos,
        heading: u.body.heading,
        speed: u.body.speed,
    };
    if !advance_legs(&mut b, &mut t, goal, car, kmax, dt) {
        u.turn = None;
        return false;
    }
    u.body.pos = b.pos;
    u.body.heading = b.heading;
    u.body.speed = b.speed;
    u.body.lat_g = b.speed * b.speed * kmax / GRAVITY_MPS2;
    u.turn = Some(t);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PoliceConfig;
    use crate::events::{UnitId, UnitKind};
    use crate::unit::{
        plan_unit_speed, step_ground, DriveCommand, DriveEnv, UnitRole, LANE_OFFSET_M,
    };
    use wp_contracts::{DamageMode, DayPhase};
    use wp_math::Rng;

    /// A straight road along +Y through the origin, `left` / `right` m each side of the
    /// centreline (seen driving +Y).
    fn road(left: f32, right: f32) -> RoadSpan {
        RoadSpan::straight([0.0, 0.0], [0.0, 1.0], left, right)
    }

    /// A patrol unit in the right-hand lane at (1.75, 0) facing +Y whose new route runs back
    /// down the road (-Y): what `route_from` gives a unit routed the other way.
    fn unit_facing_away(r: RoadSpan) -> Unit {
        let mut u = Unit::new(
            UnitId(1),
            UnitKind::Car,
            UnitRole::Reinforcement,
            [LANE_OFFSET_M, 0.0],
            [0.0, 1.0],
            10.0,
        );
        u.road = Some(r);
        u.set_command(DriveCommand {
            track: Track::new(vec![[0.0, 0.0], [0.0, -300.0]]),
            speed_mps: 12.0,
            cornering_g: 0.4,
            siren: true,
            lane_offset_m: LANE_OFFSET_M,
        });
        u
    }

    /// What one run of the manoeuvre did.
    #[derive(Debug, Default)]
    struct Outcome {
        /// Time until the manoeuvre ended, s.
        done_s: Option<f32>,
        /// Legs driven.
        legs: u8,
        /// The plan.
        kind: Option<TurnKind>,
        /// Least room between a corner and a kerb, m (negative = off the road).
        min_room_m: f32,
        /// Fastest forward / reverse speed, m/s.
        max_fwd: f32,
        max_rev: f32,
        /// Ran-wide crashes.
        crashes: u32,
        /// Heading and position at the end.
        heading: [f32; 2],
        pos: [f32; 2],
    }

    /// Least room between the car's outline and the kerbs of `r`, m (negative = off it):
    /// the most a kerb margin could grow and still have every point on some piece.
    fn room_m(r: &RoadSpan, pos: [f32; 2], heading: [f32; 2], car: CarBox) -> f32 {
        let mut worst = f32::INFINITY;
        for p in outline(pos, heading, car) {
            // Best margin over the pieces this point is on (search in 1 cm steps).
            let mut m = -1.0;
            while r.contains(p, m + 0.01) && m < 20.0 {
                m += 0.01;
            }
            worst = worst.min(m);
        }
        worst
    }

    /// Drive `u` with the built-in driver for `secs` (think-rate speed planning as in
    /// `PoliceSystem`), recording the manoeuvre.
    fn drive(mut u: Unit, secs: f32) -> (Unit, Outcome) {
        let cfg = PoliceConfig::builtin().pursuit;
        let env = DriveEnv {
            limit_mps: 13.9,
            phase: DayPhase::Day,
            mode: DamageMode::Simulation,
            hazards: false,
        };
        let r = u.road.expect("road");
        let car = car_box(&u);
        let mut rng = Rng::new(7);
        let dt = 1.0 / 240.0;
        let mut out = Outcome {
            min_room_m: f32::INFINITY,
            ..Outcome::default()
        };
        for i in 0..(secs / dt) as u32 {
            if i % 24 == 0 {
                u.planned_speed = plan_unit_speed(&u, cfg.unit.brake_mps2);
            }
            let rep = step_ground(&mut u, &env, &cfg, &mut rng, dt);
            out.crashes += rep.crashed as u32;
            if let Some(t) = u.turn {
                out.legs = out.legs.max(t.legs);
                if let Some(p) = t.plan {
                    out.kind.get_or_insert(p.kind);
                    if p.kind == TurnKind::ThreePoint {
                        out.kind = Some(TurnKind::ThreePoint);
                    }
                }
            } else if out.done_s.is_none() && out.legs > 0 {
                out.done_s = Some(i as f32 * dt);
            }
            if out.done_s.is_none() {
                out.min_room_m = out
                    .min_room_m
                    .min(room_m(&r, u.body.pos, u.body.heading, car));
            }
            out.max_fwd = out.max_fwd.max(u.body.speed);
            out.max_rev = out.max_rev.max(-u.body.speed);
        }
        out.heading = u.body.heading;
        out.pos = u.body.pos;
        (u, out)
    }

    #[test]
    fn narrow_road_takes_a_three_point_turn_on_the_surface() {
        // A 1+1 local street (3.5 m lanes): too narrow for one sweep.
        let (u, o) = drive(unit_facing_away(road(3.5, 3.5)), 20.0);
        assert_eq!(o.kind, Some(TurnKind::ThreePoint), "{o:?}");
        assert!(o.legs >= 3, "{o:?}");
        let t = o.done_s.expect("finished");
        assert!(t < 14.0, "three-point turn took {t} s");
        assert!(
            o.min_room_m >= 0.25,
            "corner {} m from the kerb",
            o.min_room_m
        );
        assert!(o.max_fwd <= UTURN_MPS + 1e-3 || o.done_s.is_some());
        assert!(
            o.max_rev > 0.5 && o.max_rev <= THREE_POINT_REV_MPS + 1e-3,
            "{o:?}"
        );
        assert_eq!(o.crashes, 0);
        // Away down the road, in the other lane.
        assert!(dot(u.body.heading, [0.0, -1.0]) > 0.95, "{o:?}");
        assert!(u.body.pos[1] < -30.0, "{o:?}");
        assert!(u.body.pos[0].abs() < 3.5, "{o:?}");
    }

    #[test]
    fn wide_road_takes_one_u_turn_sweep() {
        // A 2+2 arterial: room for the turning circle after a short swing out.
        let (_, o) = drive(unit_facing_away(road(7.0, 7.0)), 20.0);
        assert_eq!(o.kind, Some(TurnKind::UTurn), "{o:?}");
        assert_eq!(o.legs, 1, "{o:?}");
        let t = o.done_s.expect("finished");
        assert!(t < 6.0, "U-turn took {t} s");
        assert!(o.min_room_m >= 0.25, "{o:?}");
        assert!(o.max_rev == 0.0, "a U-turn never reverses: {o:?}");
        assert_eq!(o.crashes, 0);
        assert!(dot(o.heading, [0.0, -1.0]) > 0.95, "{o:?}");
    }

    #[test]
    fn u_turn_needs_the_turning_circle() {
        let u = unit_facing_away(road(7.0, 7.0));
        let kmax = full_lock_curvature(&u);
        // Default geometry: 2.7 m wheelbase, 0.6 rad lock → ~3.9 m path radius.
        assert!((1.0 / kmax - 3.95).abs() < 0.1, "{}", 1.0 / kmax);
        let plan = plan_turn(&u, road(7.0, 7.0), [0.0, -1.0]);
        assert_eq!(plan.kind, TurnKind::UTurn);
        assert_eq!(plan.dir, 1.0, "turns left, across the road");
        let narrow = plan_turn(&u, road(3.5, 3.5), [0.0, -1.0]);
        assert_eq!(narrow.kind, TurnKind::ThreePoint);
        // A tighter-locking car U-turns where the default one cannot.
        let mut tight = unit_facing_away(road(6.5, 6.5));
        assert_eq!(
            plan_turn(&tight, road(6.5, 6.5), [0.0, -1.0]).kind,
            TurnKind::ThreePoint
        );
        tight.geometry.max_steer_rad = 0.75;
        assert_eq!(
            plan_turn(&tight, road(6.5, 6.5), [0.0, -1.0]).kind,
            TurnKind::UTurn
        );
    }

    #[test]
    fn junction_side_road_makes_a_quicker_three_point_turn() {
        // The same 1+1 street, with a 1+1 side road leaving to the left 6 m ahead.
        let plain = road(3.5, 3.5);
        let mut junction = plain;
        junction.add_piece(crate::graph::RoadRect {
            origin: [0.0, 6.0],
            axis: [-1.0, 0.0],
            len_m: 100.0,
            left_m: 3.5,
            right_m: 3.5,
        });
        let (_, o_plain) = drive(unit_facing_away(plain), 20.0);
        let (u, o_junction) = drive(unit_facing_away(junction), 20.0);
        let (tp, tj) = (o_plain.done_s.unwrap(), o_junction.done_s.unwrap());
        assert!(tj < tp - 1.0, "side road {tj} s vs plain {tp} s");
        assert!(
            o_junction.legs < o_plain.legs,
            "{o_junction:?} vs {o_plain:?}"
        );
        assert!(o_junction.min_room_m >= 0.25, "{o_junction:?}");
        assert!(dot(u.body.heading, [0.0, -1.0]) > 0.95, "{o_junction:?}");
    }

    #[test]
    fn manoeuvre_is_deterministic() {
        let (a, oa) = drive(unit_facing_away(road(3.5, 3.5)), 12.0);
        let (b, ob) = drive(unit_facing_away(road(3.5, 3.5)), 12.0);
        assert_eq!(a.body, b.body);
        assert_eq!(oa.done_s, ob.done_s);
        assert_eq!(a.body.pos.map(f32::to_bits), b.body.pos.map(f32::to_bits));
    }

    #[test]
    fn fast_unit_slows_down_before_turning() {
        let mut u = unit_facing_away(road(3.5, 3.5));
        u.body.speed = 20.0;
        let (_, o) = drive(u, 25.0);
        assert_eq!(o.kind, Some(TurnKind::ThreePoint), "{o:?}");
        assert!(o.min_room_m >= 0.25, "{o:?}");
        assert!(o.done_s.expect("finished") < 18.0, "{o:?}");
        assert_eq!(o.crashes, 0);
    }

    #[test]
    fn no_turn_round_without_a_road_or_on_a_one_way_deck() {
        // Scripted racers have no road span: no manoeuvre (they keep the old swing).
        let mut u = unit_facing_away(road(3.5, 3.5));
        u.road = None;
        let (u, _) = drive_no_road(u, 1.0);
        assert!(u.turn.is_none());
        // Facing the legal way on a one-way carriageway: never turns round on the deck.
        let mut deck = road(3.7, 3.7);
        deck.oneway = true;
        let mut u = unit_facing_away(deck);
        u.road = Some(deck);
        let cfg = PoliceConfig::builtin().pursuit;
        assert!(!step_turn_round(&mut u, cfg.unit.brake_mps2, 1.0 / 240.0));
        assert!(u.turn.is_none());
    }

    fn drive_no_road(mut u: Unit, secs: f32) -> (Unit, ()) {
        let cfg = PoliceConfig::builtin().pursuit;
        let env = DriveEnv {
            limit_mps: 13.9,
            phase: DayPhase::Day,
            mode: DamageMode::Simulation,
            hazards: false,
        };
        let mut rng = Rng::new(7);
        for _ in 0..(secs * 240.0) as u32 {
            step_ground(&mut u, &env, &cfg, &mut rng, 1.0 / 240.0);
            assert!(u.turn.is_none());
        }
        (u, ())
    }

    #[test]
    fn replan_mid_turn_only_ends_it_for_a_track_ahead() {
        let (mut u, _) = drive(unit_facing_away(road(3.5, 3.5)), 3.0);
        assert!(u.turning_round(), "still turning after 3 s: {:?}", u.turn);
        // A route back the way it came (half a turn undone): refused by periodic re-plans.
        let back = Track::new(vec![u.body.pos, add(u.body.pos, [0.0, 300.0])]);
        let h = u.body.heading;
        let undo = rotation_to(h, [0.0, 1.0], u.turn.unwrap().plan.unwrap().dir);
        if undo > REPLAN_ROTATION_RAD {
            assert!(!accepts_replan(&u, &back));
        }
        // Another route down the road: accepted, the manoeuvre carries on.
        let down = Track::new(vec![[0.0, 0.0], [0.0, -200.0]]);
        assert!(accepts_replan(&u, &down));
        // A track straight ahead of the nose: accepted, and it ends the manoeuvre.
        let ahead = Track::new(vec![u.body.pos, add(u.body.pos, scale(h, 100.0))]);
        assert!(accepts_replan(&u, &ahead));
        u.set_command(DriveCommand {
            track: ahead,
            speed_mps: 10.0,
            cornering_g: 0.4,
            siren: true,
            lane_offset_m: 0.0,
        });
        assert!(u.turn.is_none());
    }
}
