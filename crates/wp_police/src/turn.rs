//! Turning around.
//!
//! A unit whose route starts behind it (a new destination back the way it came, a racer seen in
//! the mirror) used to crawl round at full lock. On a two-lane street that runs over the far kerb
//! and into parked cars. Now it turns on the road it is on, like a driver would:
//!
//! 1. **Brake** to a standstill, straight ahead.
//! 2. **Wait** while a moving vehicle would reach the turning area within `traffic_gap_s`.
//! 3. **Forward** leg at full lock toward the turn side, until a wheel would come within
//!    `kerb_margin_m` of the edge of the road or any part of the body within `obstacle_margin_m`
//!    of a vehicle, parked car or cone (the body may overhang the kerb).
//! 4. **Reverse** leg at the opposite lock, until the same happens going backwards.
//!
//! Legs 3–4 repeat until the unit faces within `acos(done_cos)` of the goal direction; then the
//! normal controller takes over. Where the road is wide enough this is a U-turn (one forward
//! leg), otherwise a three-point turn. A leg also ends after `max_leg_s`, the whole turn after
//! `max_s`. Boxed in on lock both ways (a corner right by a parked car), it backs out (or pulls
//! forward) straight to make room; boxed in completely, or while traffic comes close, it holds
//! still.
//!
//! The turn side is the way the goal lies; for a goal straight behind it is the side with more
//! room (left on ties: units keep right, so that is across the road).
//!
//! One logic for both drivers: [`control`] returns full-lock steer and a signed speed target. The
//! built-in driver integrates it with [`step_kinematic`]; physical units turn it into pedals with
//! [`pedals`] (`wp_vehicle`'s automatic gearbox selects reverse when the brake is held at a
//! standstill, and the pedals swap while in reverse).
//!
//! The road is frozen at the start as a straight strip (centre point, direction, half width of
//! the paved road, parking lanes included where the city says so): turns take place within a
//! few metres. The car pivots about its rear axle (kinematic bicycle), so the tail barely swings.

use serde::{Deserialize, Serialize};
use wp_contracts::{EntityId, GRAVITY_MPS2};
use wp_math::sin_cos;

use crate::avoid::{Obstacle, UNIT_HALF_LENGTH_M, UNIT_HALF_WIDTH_M};
use crate::config::TurnConfig;
use crate::events::UnitId;
use crate::graph::{add, cross, dist, dot, normalize, perp_left, scale, sub};
use crate::unit::{SteerGeometry, UnitBody};

/// Below this speed the unit counts as stopped, m/s.
const STOPPED_MPS: f32 = 0.3;

/// The leading end is checked this far beyond the stopping distance, m.
const PROBE_M: f32 = 0.15;

/// Half the wheel track of a police car (wheel centre to the car's centreline), m.
const WHEEL_TRACK_HALF_M: f32 = 0.8;

/// Phase of a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnPhase {
    /// Braking to a standstill.
    Brake,
    /// Stopped, waiting for a gap in traffic.
    Wait,
    /// Forward leg at full lock toward the turn side.
    Forward,
    /// Reverse leg at the opposite lock.
    Reverse,
}

/// A turn in progress.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TurnState {
    /// Current phase.
    pub phase: TurnPhase,
    /// +1 turns left (counter-clockwise), -1 right.
    pub side: f32,
    /// Direction to face at the end (unit vector).
    pub goal: [f32; 2],
    /// A point on the road's centreline, m.
    pub road_centre: [f32; 2],
    /// Road direction (unit vector).
    pub road_dir: [f32; 2],
    /// Half width of the driving lanes, m.
    pub half_width_m: f32,
    /// Time since the turn started, s.
    pub elapsed_s: f32,
    /// Time in the current phase, s.
    pub phase_s: f32,
    /// Forward and reverse legs started so far.
    pub legs: u32,
    /// Physical units: the last pedals asked for reverse (the gearbox may still be in R).
    pub reverse_gear: bool,
}

/// What the unit should do this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TurnControl {
    /// Steering, -1 (full left) ..= 1 (full right).
    pub steer: f32,
    /// Signed speed target along the heading (negative = reverse), m/s.
    pub speed: f32,
    /// The turn is over: hand back to the normal controller.
    pub done: bool,
}

/// The unit as the turn logic sees it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TurnBody {
    pub(crate) unit: UnitId,
    pub(crate) entity: Option<EntityId>,
    pub(crate) pos: [f32; 2],
    pub(crate) heading: [f32; 2],
    /// Signed speed along the heading, m/s.
    pub(crate) v: f32,
    pub(crate) geometry: SteerGeometry,
}

impl TurnState {
    /// A turn from `pos` facing `heading` to face `goal`, on a straight road through
    /// `road_centre` along `road_dir` whose driving lanes are `half_width_m` either side.
    pub fn new(
        pos: [f32; 2],
        heading: [f32; 2],
        goal: [f32; 2],
        road_centre: [f32; 2],
        road_dir: [f32; 2],
        half_width_m: f32,
    ) -> Self {
        let goal = normalize(goal);
        let road_dir = normalize(road_dir);
        let c = cross(heading, goal);
        let side = if c.abs() > 0.3 {
            c.signum()
        } else {
            // Straight behind: the side with more room (lateral position across the road,
            // measured to the unit's left).
            let along = if dot(road_dir, heading) >= 0.0 {
                road_dir
            } else {
                scale(road_dir, -1.0)
            };
            let left_of_centre = dot(sub(pos, road_centre), perp_left(along));
            let room_left = half_width_m - left_of_centre;
            let room_right = half_width_m + left_of_centre;
            if room_left + 0.5 >= room_right {
                1.0
            } else {
                -1.0
            }
        };
        Self {
            phase: TurnPhase::Brake,
            side,
            goal,
            road_centre,
            road_dir,
            half_width_m,
            elapsed_s: 0.0,
            phase_s: 0.0,
            legs: 0,
            reverse_gear: false,
        }
    }

    /// Lateral position of `p` across the road (left of the road direction), m.
    fn lateral(&self, p: [f32; 2]) -> f32 {
        dot(sub(p, self.road_centre), perp_left(self.road_dir))
    }

    fn enter(&mut self, phase: TurnPhase) {
        if matches!(phase, TurnPhase::Forward | TurnPhase::Reverse) {
            self.legs += 1;
        }
        self.phase = phase;
        self.phase_s = 0.0;
    }
}

/// Rotate `v` counter-clockwise by `a` rad.
fn rotate(v: [f32; 2], a: f32) -> [f32; 2] {
    let (s, c) = sin_cos(a);
    [v[0] * c - v[1] * s, v[0] * s + v[1] * c]
}

/// Path curvature at steering input `steer` (+ = right), 1/m, positive to the left.
fn curvature(steer: f32, g: SteerGeometry) -> f32 {
    let (s, c) = sin_cos(-steer * g.max_steer_rad);
    s / c.max(1e-3) / g.wheelbase_m.max(0.5)
}

/// Pose after the rear axle travels `ds` (signed) along an arc of curvature `k` (+ = left):
/// a front-steered car pivots about its rear axle, half a wheelbase behind `pos` (the body
/// centre), so the tail barely swings out.
fn advance(
    pos: [f32; 2],
    heading: [f32; 2],
    g: SteerGeometry,
    k: f32,
    ds: f32,
) -> ([f32; 2], [f32; 2]) {
    let half_wb = 0.5 * g.wheelbase_m;
    let rear = sub(pos, scale(heading, half_wb));
    let turn = k * ds;
    let h2 = normalize(rotate(heading, turn));
    let rear2 = add(rear, scale(rotate(heading, 0.5 * turn), ds));
    (add(rear2, scale(h2, half_wb)), h2)
}

/// Turning radius at full lock, m.
fn turn_radius(g: SteerGeometry) -> f32 {
    1.0 / curvature(1.0, g).abs().max(1e-3)
}

/// Point of a body at `pos` facing `h`: `lon` ahead, `lat` to the right, m.
fn body_point(pos: [f32; 2], h: [f32; 2], lon: f32, lat: f32) -> [f32; 2] {
    add(add(pos, scale(h, lon)), scale([h[1], -h[0]], lat))
}

/// Distance from `p` to the rectangle centred at `c` facing `fwd` with half extents
/// `half_len` × `half_wid` (0 inside), m.
fn to_rect(c: [f32; 2], fwd: [f32; 2], half_len: f32, half_wid: f32, p: [f32; 2]) -> f32 {
    let rel = sub(p, c);
    let right = [fwd[1], -fwd[0]];
    let dl = (dot(rel, fwd).abs() - half_len).max(0.0);
    let dw = (dot(rel, right).abs() - half_wid).max(0.0);
    (dl * dl + dw * dw).sqrt()
}

/// Gap between obstacle `o` and the unit's body at `pos` facing `h` (0 = touching), m:
/// the nearer of the body outline (corners and edge midpoints) to `o`'s box and of `o`'s
/// corners and centre to the body's box.
fn gap(o: &Obstacle, pos: [f32; 2], h: [f32; 2]) -> f32 {
    let (hl, hw) = (UNIT_HALF_LENGTH_M, UNIT_HALF_WIDTH_M);
    let mut g = f32::INFINITY;
    for (lon, lat) in [
        (hl, hw),
        (hl, 0.0),
        (hl, -hw),
        (0.0, hw),
        (0.0, -hw),
        (-hl, hw),
        (-hl, 0.0),
        (-hl, -hw),
    ] {
        g = g.min(to_rect(
            o.pos,
            o.fwd,
            o.half_len,
            o.half_wid,
            body_point(pos, h, lon, lat),
        ));
    }
    let right = [o.fwd[1], -o.fwd[0]];
    for (lon, lat) in [
        (o.half_len, o.half_wid),
        (o.half_len, -o.half_wid),
        (-o.half_len, o.half_wid),
        (-o.half_len, -o.half_wid),
        (0.0, 0.0),
    ] {
        let p = add(add(o.pos, scale(o.fwd, lon)), scale(right, lat));
        g = g.min(to_rect(pos, h, hl, hw, p));
    }
    g
}

fn is_me(me: &TurnBody, o: &Obstacle) -> bool {
    o.unit == Some(me.unit) || (o.entity.is_some() && o.entity == me.entity)
}

/// Whether moving `dir` (+1 forward, -1 reverse) at steering `steer` would bring a wheel over
/// the kerb (the body may overhang it) or the body into something before it can stop. Every
/// corner and both bumper centres are checked, not just the leading end: on lock the other end
/// swings out (the nose, reversing). A wheel already over the kerb may still move back in.
#[allow(clippy::too_many_arguments)]
fn blocked(
    st: &TurnState,
    me: &TurnBody,
    dir: f32,
    steer: f32,
    obstacles: &[Obstacle],
    statics: &[Obstacle],
    cfg: &TurnConfig,
    decel: f32,
) -> bool {
    let v = me.v.abs();
    let d = v * v / (2.0 * decel.max(0.5)) + PROBE_M;
    let (p2, h2) = advance(
        me.pos,
        me.heading,
        me.geometry,
        curvature(steer, me.geometry),
        d * dir,
    );
    let limit = st.half_width_m - cfg.kerb_margin_m;
    let near = |o: &&Obstacle| {
        !is_me(me, o) && dist(o.pos, me.pos) < UNIT_HALF_LENGTH_M + o.half_len + d + 2.0
    };
    // The wheels stay on the road; the body may overhang the kerb.
    let half_wb = 0.5 * me.geometry.wheelbase_m;
    for lon in [half_wb, -half_wb] {
        for lat in [-WHEEL_TRACK_HALF_M, WHEEL_TRACK_HALF_M] {
            let now = st.lateral(body_point(me.pos, me.heading, lon, lat));
            let n = st.lateral(body_point(p2, h2, lon, lat));
            if n.abs() > limit && n.abs() > now.abs() - 0.01 {
                return true;
            }
        }
    }
    // Closer than the margin to something, and closing in (moving away is always allowed).
    obstacles.iter().chain(statics).filter(near).any(|o| {
        let next = gap(o, p2, h2);
        next < cfg.obstacle_margin_m && next < gap(o, me.pos, me.heading) - 1e-4
    })
}

/// Whether a moving vehicle comes within the turning area around the unit within `horizon_s`.
fn traffic_near(me: &TurnBody, obstacles: &[Obstacle], horizon_s: f32) -> bool {
    let radius = turn_radius(me.geometry) + UNIT_HALF_LENGTH_M + 1.0;
    obstacles.iter().any(|o| {
        if is_me(me, o) {
            return false;
        }
        let speed = dot(o.vel, o.vel).sqrt();
        if speed < 1.0 {
            return false; // parked or waiting: the leg checks keep clear of it
        }
        let dir = scale(o.vel, 1.0 / speed);
        let t = (dot(sub(me.pos, o.pos), dir) / speed).clamp(0.0, horizon_s);
        dist(add(o.pos, scale(dir, speed * t)), me.pos) < radius + o.half_len
    })
}

/// Advance the turn by `dt` and return this tick's steer and speed target. `obstacles` are
/// vehicles (moving or not), `statics` parked cars and cones near the unit, `decel` the braking
/// the unit plans with (m/s²).
#[allow(clippy::too_many_arguments)]
pub(crate) fn control(
    st: &mut TurnState,
    me: &TurnBody,
    obstacles: &[Obstacle],
    statics: &[Obstacle],
    cfg: &TurnConfig,
    decel: f32,
    dt: f32,
) -> TurnControl {
    st.elapsed_s += dt;
    st.phase_s += dt;
    let stop = TurnControl {
        steer: 0.0,
        speed: 0.0,
        done: false,
    };
    let aligned = dot(me.heading, st.goal) >= cfg.done_cos;
    if aligned || st.elapsed_s > cfg.max_s {
        // Rolling backwards: stop first (the normal controller only drives forwards).
        if me.v < -STOPPED_MPS {
            return stop;
        }
        return TurnControl { done: true, ..stop };
    }
    let lock = -st.side; // steering input toward the turn side (+ = right)
    if st.phase == TurnPhase::Brake {
        if me.v.abs() > STOPPED_MPS {
            return stop;
        }
        st.enter(TurnPhase::Wait);
    }
    if st.phase == TurnPhase::Wait {
        if traffic_near(me, obstacles, cfg.traffic_gap_s) {
            return stop;
        }
        st.enter(TurnPhase::Forward);
    }
    // Traffic arriving mid-turn: hold still rather than move into its way.
    let hold = traffic_near(me, obstacles, 0.5 * cfg.traffic_gap_s);
    let (dir, steer) = match st.phase {
        TurnPhase::Reverse => (-1.0, -lock),
        _ => (1.0, lock),
    };
    let ahead_blocked = blocked(st, me, dir, steer, obstacles, statics, cfg, decel);
    if ahead_blocked || st.phase_s > cfg.max_leg_s {
        // Switch legs unless the other way is blocked too (boxed in: wait).
        let back_blocked = blocked(st, me, -dir, -steer, obstacles, statics, cfg, decel);
        if back_blocked && ahead_blocked {
            // Boxed in on lock both ways (a corner right by a parked car): straight back, or
            // straight on, to make room; otherwise wait.
            let creep = 0.5 * cfg.forward_mps.min(cfg.reverse_mps);
            for d in [-1.0, 1.0] {
                let rolling_other_way = me.v * d < -STOPPED_MPS;
                if !rolling_other_way && !blocked(st, me, d, 0.0, obstacles, statics, cfg, decel) {
                    return TurnControl {
                        steer: 0.0,
                        speed: if hold { 0.0 } else { d * creep },
                        done: false,
                    };
                }
            }
            return TurnControl {
                steer: rolling_steer(me.v, lock, steer),
                ..stop
            };
        }
        if !back_blocked || st.phase_s > cfg.max_leg_s {
            st.enter(if dir > 0.0 {
                TurnPhase::Reverse
            } else {
                TurnPhase::Forward
            });
            return leg_control(me.v, lock, -dir, cfg, hold);
        }
    }
    leg_control(me.v, lock, dir, cfg, hold)
}

/// Steering that keeps the body rotating toward the turn side: `lock` while rolling forwards,
/// the opposite lock while rolling backwards, `leg_steer` (the coming leg's) when stopped.
/// Flipping the wheels before the car has stopped would undo part of the turn.
fn rolling_steer(v: f32, lock: f32, leg_steer: f32) -> f32 {
    if v > STOPPED_MPS {
        lock
    } else if v < -STOPPED_MPS {
        -lock
    } else {
        leg_steer
    }
}

/// Control for a leg in direction `dir` (+1 forward, -1 reverse); `hold` = stand still.
fn leg_control(v: f32, lock: f32, dir: f32, cfg: &TurnConfig, hold: bool) -> TurnControl {
    let target = if dir > 0.0 {
        cfg.forward_mps
    } else {
        -cfg.reverse_mps
    };
    TurnControl {
        steer: rolling_steer(v, lock, lock * dir),
        speed: if hold { 0.0 } else { target },
        done: false,
    }
}

/// Move a headless unit's kinematic body one tick under `c`: signed speed toward the target
/// (accelerating at `accel`, braking at `brake`, m/s²), then a kinematic bicycle about the rear
/// axle.
pub(crate) fn step_kinematic(
    b: &mut UnitBody,
    c: &TurnControl,
    g: SteerGeometry,
    accel: f32,
    brake: f32,
    dt: f32,
) {
    let v = b.signed_speed();
    let slowing = c.speed.abs() < v.abs() || c.speed * v < 0.0;
    let rate = if slowing { brake } else { accel };
    let v = v + (c.speed - v).clamp(-rate * dt, rate * dt);
    let k = curvature(c.steer, g);
    (b.pos, b.heading) = advance(b.pos, b.heading, g, k, v * dt);
    b.fwd_speed = v;
    b.speed = v.abs();
    b.lat_g = (v * v * k).abs() / GRAVITY_MPS2;
}

/// Pedals `(throttle, brake, handbrake)` for a physical unit to follow `target` (signed m/s) at
/// signed speed `v`, with `wp_vehicle`'s automatic gearbox: holding the brake at a standstill
/// selects reverse, and in reverse the brake pedal drives and the throttle brakes (pressing it
/// near a standstill selects first again).
pub(crate) fn pedals(st: &mut TurnState, v: f32, target: f32) -> (f32, f32, f32) {
    if target > 0.0 {
        if v < -STOPPED_MPS {
            return (0.6, 0.0, 0.0); // still rolling back in reverse: the throttle brakes
        }
        st.reverse_gear = false;
        if v > target + 1.0 {
            return (0.0, 0.4, 0.0);
        }
        ((0.2 + 0.25 * (target - v)).clamp(0.0, 0.5), 0.0, 0.0)
    } else if target < 0.0 {
        if v > STOPPED_MPS {
            return (0.0, 0.8, 0.0); // still rolling forwards: brake (then hold it for reverse)
        }
        st.reverse_gear = true;
        if v < target - 0.5 {
            (0.3, 0.0, 0.0) // too fast backwards: the throttle brakes
        } else if v > target {
            (0.0, (0.6 + 0.2 * (v - target)).min(0.8), 0.0)
        } else {
            (0.0, 0.0, 0.0)
        }
    } else if v > STOPPED_MPS {
        (0.0, 1.0, 0.0)
    } else if v < -STOPPED_MPS {
        (0.6, 0.0, 0.0)
    } else if st.reverse_gear {
        // Back to first (a held brake in reverse would drive), held by the handbrake.
        st.reverse_gear = false;
        (0.08, 0.0, 1.0)
    } else {
        (0.0, 0.4, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PoliceConfig;

    fn cfg() -> TurnConfig {
        PoliceConfig::builtin().pursuit.turn
    }

    /// How far a body corner may overhang the kerb (bumpers pass over it, wheels do not), m.
    const OVERHANG_M: f32 = 0.9;

    fn body(pos: [f32; 2], heading: [f32; 2]) -> UnitBody {
        UnitBody {
            pos,
            heading,
            speed: 0.0,
            fwd_speed: 0.0,
            lat_g: 0.0,
            alt_m: 0.0,
        }
    }

    /// Run a turn on a straight road along +Y (centre x = 0) with the built-in kinematics;
    /// returns the body, the turn state, the largest |x| any wheel reached and whether it
    /// finished. Body corners may overhang the kerb by at most `OVERHANG_M`.
    fn run_turn(
        start: UnitBody,
        half_width: f32,
        obstacles: &[Obstacle],
        statics: &[Obstacle],
        secs: f32,
    ) -> (UnitBody, TurnState, f32, bool) {
        let c = cfg();
        let g = SteerGeometry::default();
        let mut b = start;
        let mut st = TurnState::new(b.pos, b.heading, [0.0, -1.0], [0.0, 0.0], [0.0, 1.0], 3.5);
        st.half_width_m = half_width;
        let dt = 1.0 / 240.0;
        let mut worst: f32 = 0.0;
        let mut done = false;
        for _ in 0..(secs / dt) as usize {
            let me = TurnBody {
                unit: UnitId(1),
                entity: None,
                pos: b.pos,
                heading: b.heading,
                v: b.fwd_speed,
                geometry: g,
            };
            let ctl = control(&mut st, &me, obstacles, statics, &c, 6.5, dt);
            if ctl.done {
                done = true;
                break;
            }
            step_kinematic(&mut b, &ctl, g, 4.0, 6.5, dt);
            let half_wb = 0.5 * g.wheelbase_m;
            for lon in [-half_wb, half_wb] {
                for lat in [-WHEEL_TRACK_HALF_M, WHEEL_TRACK_HALF_M] {
                    worst = worst.max(body_point(b.pos, b.heading, lon, lat)[0].abs());
                }
            }
            for lon in [-UNIT_HALF_LENGTH_M, UNIT_HALF_LENGTH_M] {
                for lat in [-UNIT_HALF_WIDTH_M, UNIT_HALF_WIDTH_M] {
                    let x = body_point(b.pos, b.heading, lon, lat)[0].abs();
                    assert!(x < half_width + OVERHANG_M, "body {x} m out");
                }
            }
            for o in statics {
                let g = gap(o, b.pos, b.heading);
                assert!(g > 0.3, "{g} m from a parked car");
            }
        }
        (b, st, worst, done)
    }

    #[test]
    fn three_point_turn_on_a_two_lane_street_stays_on_the_road() {
        // Right-hand lane of a 7 m street, facing +Y, goal -Y.
        let (b, st, worst, done) = run_turn(body([1.75, 0.0], [0.0, 1.0]), 3.5, &[], &[], 30.0);
        assert!(done, "turn finished: {st:?}");
        assert!(
            dot(b.heading, [0.0, -1.0]) > 0.9,
            "faces back: {:?}",
            b.heading
        );
        assert!(
            (2..=4).contains(&st.legs),
            "a three-point turn on 7 m: {} legs",
            st.legs
        );
        assert_eq!(st.side, 1.0, "turns left, across the road");
        assert!(
            worst < 3.5 - 0.3 + 0.05,
            "wheels stayed on the road: max |x| {worst}"
        );
        assert!(st.elapsed_s < 8.0, "took {} s", st.elapsed_s);
    }

    #[test]
    fn wide_road_is_a_single_u_turn() {
        let (b, st, worst, done) = run_turn(body([1.75, 0.0], [0.0, 1.0]), 12.0, &[], &[], 30.0);
        assert!(done);
        assert!(dot(b.heading, [0.0, -1.0]) > 0.9);
        assert_eq!(st.legs, 1, "one forward leg");
        assert!(worst < 12.0);
    }

    #[test]
    fn brakes_to_a_stop_before_turning() {
        let mut start = body([1.75, 0.0], [0.0, 1.0]);
        start.fwd_speed = 12.0;
        start.speed = 12.0;
        let (b, st, worst, done) = run_turn(start, 3.5, &[], &[], 40.0);
        assert!(done, "{st:?}");
        assert!(dot(b.heading, [0.0, -1.0]) > 0.9);
        assert!(worst < 3.5 - 0.3 + 0.05, "max wheel |x| {worst}");
    }

    #[test]
    fn keeps_clear_of_a_parked_car() {
        // A parked car right where the nose swings on the far side.
        let parked = Obstacle {
            entity: None,
            unit: None,
            pos: [-2.6, 3.5],
            vel: [0.0, 0.0],
            fwd: [0.0, 1.0],
            half_len: 2.25,
            half_wid: 0.95,
        };
        let (b, st, _, done) = run_turn(body([1.75, 0.0], [0.0, 1.0]), 6.0, &[], &[parked], 40.0);
        assert!(done, "{st:?}");
        assert!(dot(b.heading, [0.0, -1.0]) > 0.9);
        // Never touched the parked car.
        assert!(gap(&parked, b.pos, b.heading) > 0.0);
    }

    #[test]
    fn waits_for_oncoming_traffic() {
        let c = cfg();
        let g = SteerGeometry::default();
        let mut st = TurnState::new(
            [1.75, 0.0],
            [0.0, 1.0],
            [0.0, -1.0],
            [0.0, 0.0],
            [0.0, 1.0],
            3.5,
        );
        let me = TurnBody {
            unit: UnitId(1),
            entity: None,
            pos: [1.75, 0.0],
            heading: [0.0, 1.0],
            v: 0.0,
            geometry: g,
        };
        // Oncoming car 30 m ahead in the other lane at 12 m/s: arrives in 2.5 s.
        let car = Obstacle {
            entity: Some(EntityId(9)),
            unit: None,
            pos: [-1.75, 30.0],
            vel: [0.0, -12.0],
            fwd: [0.0, -1.0],
            half_len: 2.3,
            half_wid: 0.95,
        };
        let ctl = control(&mut st, &me, &[car], &[], &c, 6.5, 0.1);
        assert_eq!(ctl.speed, 0.0);
        assert_eq!(st.phase, TurnPhase::Wait);
        // Once it has passed, go.
        let gone = Obstacle {
            pos: [-1.75, -30.0],
            ..car
        };
        let ctl = control(&mut st, &me, &[gone], &[], &c, 6.5, 0.1);
        assert_eq!(st.phase, TurnPhase::Forward);
        assert!(
            ctl.speed > 0.0 && ctl.steer < 0.0,
            "forward, left lock: {ctl:?}"
        );
    }

    #[test]
    fn pedals_engage_reverse_and_come_back() {
        let mut st = TurnState::new([0.0; 2], [0.0, 1.0], [0.0, -1.0], [0.0; 2], [0.0, 1.0], 3.5);
        // Rolling forward, reverse wanted: brake.
        assert_eq!(pedals(&mut st, 1.0, -2.0), (0.0, 0.8, 0.0));
        // Stopped: hold the brake past half travel (selects reverse, then drives back).
        let (t, b, _) = pedals(&mut st, 0.0, -2.0);
        assert!(t == 0.0 && b > 0.5);
        assert!(st.reverse_gear);
        // Reversing, forward wanted: throttle brakes until stopped.
        assert_eq!(pedals(&mut st, -2.0, 3.0), (0.6, 0.0, 0.0));
        // Hold after a reverse leg: back to first on the handbrake.
        assert_eq!(pedals(&mut st, 0.0, 0.0), (0.08, 0.0, 1.0));
        assert!(!st.reverse_gear);
        assert_eq!(pedals(&mut st, 0.0, 0.0), (0.0, 0.4, 1.0));
    }
}
