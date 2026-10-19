//! Low-speed manoeuvres for AI drivers stuck behind a stationary vehicle (owner playtest row 32,
//! "AI cars, when impeded, should go around the stationary vehicle or turn around").
//!
//! Shared by civilian traffic ([`crate::traffic`]) and anything else driving with the shared
//! controller ([`crate::drive`]), e.g. police patrols:
//! - [`plan_turn`]: a k-point turn (U-turn when the road is wide enough, otherwise a 3-, 5-, …
//!   point turn) inside a straight road corridor, clear of obstacle boxes. Pure geometry.
//! - [`TurnDriver`]: drives a physical `wp_vehicle` through such a turn, closed loop: it re-plans
//!   from the measured pose before every leg, engages reverse through the automatic gearbox and
//!   stops a leg early if the body would leave the corridor.
//! - [`ReverseDriver`]: reverses a physical vehicle along a path (back to the last junction).
//! - [`sight_clear`]: is a stretch of road ahead visible (no crest, no sharp bend)? Used before
//!   overtaking a blocker in the oncoming lane.
//!
//! Conventions: +Z up, XY plane, `forward` unit vectors, lateral offsets + right. The car's
//! reference point for turning is the rear axle (`wheelbase / 2` behind the body centre).
//! Deterministic: IEEE `+ - * /`, `sqrt` and `wp_math::sin_cos` only.

use wp_contracts::{VehicleDef, VehicleInput, VehicleState};
use wp_math::sin_cos;

use crate::geom::{self, Obb, V2, V3};

/// Size and steering of the car doing a manoeuvre.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarShape {
    /// Half length (bumper to centre), m.
    pub half_length_m: f32,
    /// Half width, m.
    pub half_width_m: f32,
    /// Wheelbase, m.
    pub wheelbase_m: f32,
    /// Road-wheel angle at full lock, rad.
    pub max_steer_rad: f32,
}

impl CarShape {
    /// The civilian traffic car (4.5 m × 1.9 m, 2.7 m wheelbase, 0.6 rad lock).
    pub const CIVILIAN: Self = Self {
        half_length_m: 2.25,
        half_width_m: 0.95,
        wheelbase_m: 2.7,
        max_steer_rad: 0.6,
    };

    /// Shape of a car definition: wheelbase and lock from `def`, body from the traffic car.
    pub fn from_def(def: &VehicleDef) -> Self {
        let lock = def
            .wheels
            .iter()
            .map(|w| w.max_steer_rad)
            .fold(0.0, f32::max);
        Self {
            wheelbase_m: def.wheelbase_m,
            max_steer_rad: if lock > 0.1 { lock } else { 0.6 },
            ..Self::CIVILIAN
        }
    }

    /// Curvature planned at full lock (90 % of the geometric lock, so a real car with a
    /// little understeer still follows), 1/m.
    pub fn plan_curvature(&self) -> f32 {
        let (s, c) = sin_cos(self.max_steer_rad * 0.9);
        s / c.max(1e-3) / self.wheelbase_m.max(0.5)
    }

    /// Body centre of a car whose rear axle is at `rear`, facing `f`.
    pub fn centre(&self, rear: V2, f: V2) -> V2 {
        geom::add(rear, geom::scale(f, self.wheelbase_m * 0.5))
    }

    /// Rear axle of a car whose body centre is at `centre`, facing `f`.
    pub fn rear(&self, centre: V2, f: V2) -> V2 {
        geom::sub(centre, geom::scale(f, self.wheelbase_m * 0.5))
    }

    fn body(&self, rear: V2, f: V2, margin: f32) -> Obb {
        Obb {
            c: self.centre(rear, f),
            f,
            hl: self.half_length_m + margin,
            hw: self.half_width_m + margin,
        }
    }
}

/// The road around a turn: a straight corridor along `dir` through `origin`, with the paved
/// width given as lateral bounds (+ right of `dir`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadCorridor {
    /// A point on the reference line (the turning car's lane centre), m.
    pub origin: V2,
    /// Unit direction of the reference line.
    pub dir: V2,
    /// Left edge, lateral offset (negative = left), m.
    pub lat_min: f32,
    /// Right edge, lateral offset, m.
    pub lat_max: f32,
}

impl RoadCorridor {
    /// Lateral offset of `p` (+ right).
    pub fn lateral(&self, p: V2) -> f32 {
        geom::dot(geom::sub(p, self.origin), geom::right_of(self.dir))
    }

    /// How far the box `b` sticks out of the corridor shrunk by `margin` (0 = inside), m.
    fn excess(&self, b: &Obb, margin: f32) -> f32 {
        let r = geom::right_of(b.f);
        let mut worst = 0.0f32;
        for (sl, sw) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
            let p = geom::add(
                b.c,
                geom::add(geom::scale(b.f, sl * b.hl), geom::scale(r, sw * b.hw)),
            );
            let lat = self.lateral(p);
            worst = worst
                .max(self.lat_min + margin - lat)
                .max(lat - (self.lat_max - margin));
        }
        worst
    }
}

/// One leg of a turn: drive forward or backward at constant curvature.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TurnLeg {
    /// Driving forward (else reversing).
    pub forward: bool,
    /// Path curvature of the rear axle, 1/m (+ = turning left when driving forward). Forward
    /// legs steer left, reverse legs steer right: both rotate the car counter-clockwise.
    pub curvature: f32,
    /// Distance driven, m.
    pub length_m: f32,
}

impl TurnLeg {
    /// Steering input for this leg (-1 = full left).
    pub fn steer(&self) -> f32 {
        if self.curvature > 0.0 {
            -1.0
        } else if self.curvature < 0.0 {
            1.0
        } else {
            0.0
        }
    }
}

/// A planned turn-around: legs from a start pose (rear axle `start_rear`, facing `start_f`).
#[derive(Debug, Clone, PartialEq)]
pub struct TurnPlan {
    /// Rear axle at the start, m.
    pub start_rear: V2,
    /// Facing at the start.
    pub start_f: V2,
    /// Legs in order (alternating forward / reverse).
    pub legs: Vec<TurnLeg>,
}

impl TurnPlan {
    /// Total distance driven, m.
    pub fn length_m(&self) -> f32 {
        self.legs.iter().map(|l| l.length_m).sum()
    }

    /// Rear axle and facing after `d` metres of leg `leg` (legs before it completed).
    pub fn pose(&self, leg: usize, d: f32) -> (V2, V2) {
        let (mut rear, mut f) = (self.start_rear, self.start_f);
        for (k, l) in self.legs.iter().enumerate() {
            let along = if k == leg {
                d.min(l.length_m)
            } else {
                l.length_m
            };
            let signed = if l.forward { along } else { -along };
            (rear, f) = arc_pose(rear, f, l.curvature, signed);
            if k == leg {
                break;
            }
        }
        (rear, f)
    }

    /// Rear axle and facing at the end of the turn.
    pub fn end_pose(&self) -> (V2, V2) {
        self.pose(self.legs.len().saturating_sub(1), f32::INFINITY)
    }
}

/// Pose after driving `d` metres (negative = reversing) on a circle of curvature `kappa`
/// (rear axle reference; + = counter-clockwise when driving forward).
pub fn arc_pose(rear: V2, f: V2, kappa: f32, d: f32) -> (V2, V2) {
    if kappa.abs() < 1e-6 {
        return (geom::add(rear, geom::scale(f, d)), f);
    }
    let a = kappa * d;
    let (s, c) = sin_cos(a);
    let left = [-f[1], f[0]];
    let nf = geom::normalize([f[0] * c - f[1] * s, f[0] * s + f[1] * c]);
    let off = geom::scale(
        geom::add(geom::scale(left, 1.0 - c), geom::scale(f, s)),
        1.0 / kappa,
    );
    (geom::add(rear, off), nf)
}

/// Turn is complete when the car faces within this angle of the goal direction (cos 25°).
pub const TURN_DONE_COS: f32 = 0.906;
/// Distance kept from the corridor edges while turning, m.
pub const TURN_EDGE_MARGIN_M: f32 = 0.25;
/// Clearance kept from obstacle boxes while turning, m.
pub const TURN_OBSTACLE_MARGIN_M: f32 = 0.3;
/// Most legs a turn may take.
pub const MAX_TURN_LEGS: usize = 9;

/// Plan a turn from rear axle `rear`, facing `f`, until the car faces `goal` (within 25°),
/// staying inside `road` and clear of `obstacles`. Legs alternate forward-left / reverse-right
/// at full lock, each as long as the space allows (stopping early once the goal is reached).
/// `None` if the road is too narrow or the obstacles leave no room (a leg would gain less than
/// 3°, or more than [`MAX_TURN_LEGS`] legs are needed).
pub fn plan_turn(
    shape: &CarShape,
    road: &RoadCorridor,
    obstacles: &[Obb],
    rear: V2,
    f: V2,
    goal: V2,
) -> Option<TurnPlan> {
    let kappa = shape.plan_curvature();
    const STEP: f32 = 0.1;
    let max_leg = std::f32::consts::PI / kappa + 2.0;
    let mut plan = TurnPlan {
        start_rear: rear,
        start_f: f,
        legs: Vec::new(),
    };
    let (mut r, mut h) = (rear, f);
    let blocked = |r: V2, h: V2| {
        let b = shape.body(r, h, TURN_OBSTACLE_MARGIN_M);
        obstacles.iter().any(|o| geom::obb_overlap(&b, o))
    };
    let edge = |r: V2, h: V2| road.excess(&shape.body(r, h, 0.0), TURN_EDGE_MARGIN_M);
    // A start pose already touching an obstacle or an edge may only move away from it.
    let mut forward = true;
    let mut tried_reverse_first = false;
    while plan.legs.len() < MAX_TURN_LEGS {
        if geom::dot(h, goal) >= TURN_DONE_COS {
            return Some(plan);
        }
        let curvature = if forward { kappa } else { -kappa };
        let (mut len, mut cr, mut ch) = (0.0f32, r, h);
        let mut last_excess = edge(r, h);
        let start_blocked = blocked(r, h);
        while len < max_leg {
            let d = len + STEP;
            let (nr, nh) = arc_pose(r, h, curvature, if forward { d } else { -d });
            let ex = edge(nr, nh);
            if ex > 1e-4 && ex > last_excess - 1e-4 {
                break;
            }
            if !start_blocked && blocked(nr, nh) {
                break;
            }
            (len, cr, ch, last_excess) = (d, nr, nh, ex);
            if geom::dot(ch, goal) >= TURN_DONE_COS {
                break;
            }
        }
        let gained = geom::dot(ch, h).clamp(-1.0, 1.0);
        // < 3° of rotation: no progress this way.
        if gained > 0.9986 {
            if plan.legs.is_empty() && forward && !tried_reverse_first {
                tried_reverse_first = true;
                forward = false;
                continue;
            }
            return None;
        }
        plan.legs.push(TurnLeg {
            forward,
            curvature,
            length_m: len,
        });
        (r, h) = (cr, ch);
        forward = !forward;
    }
    (geom::dot(h, goal) >= TURN_DONE_COS).then_some(plan)
}

/// Whether the road along `profile` (points every `step_m` metres from the driver, the first
/// being the driver's position) is visible from the driver: no point beyond a crest (eye and
/// object height 1.1 m) and no bend sharper than `max_bend_rad` between the first point and
/// any other. Used before overtaking in the oncoming lane.
pub fn sight_clear(profile: &[(V3, V2)], max_bend_rad: f32) -> bool {
    let Some(&(p0, t0)) = profile.first() else {
        return true;
    };
    let cos_max = sin_cos(max_bend_rad).1;
    const EYE: f32 = 1.1;
    for (k, &(pk, tk)) in profile.iter().enumerate().skip(1) {
        if geom::dot(t0, tk) < cos_max {
            return false;
        }
        // Line of sight from the eye at 0 to an object of eye height at k.
        let dk = geom::dist(geom::xy(p0), geom::xy(pk)).max(0.1);
        for &(pj, _) in &profile[1..k] {
            let dj = geom::dist(geom::xy(p0), geom::xy(pj));
            let line = p0[2] + EYE + (pk[2] - p0[2]) * (dj / dk);
            if pj[2] + 0.2 > line {
                return false;
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------------------------
// Map helpers (for drivers outside `traffic`, e.g. police patrols)
// ---------------------------------------------------------------------------------------------

/// The paved driving width around a car at `pos` facing `forward`, as a corridor along the lane
/// it is in (reference line = that lane's centre, lateral + right): every driving lane of the
/// same road in both directions (parking lanes excluded, bus lanes included). `None` off the
/// road or inside a junction. Feed it to [`plan_turn`] / [`TurnDriver`].
pub fn road_corridor(map: &crate::CityMap, pos: V3, forward: V2) -> Option<RoadCorridor> {
    let loc = map.locate(pos, Some(forward), 1.0, false, false)?;
    let lane = map.seg(loc.seg).lane()?;
    let (p, t) = map.seg(loc.seg).point_at(loc.s);
    let origin = geom::xy(p);
    let right = geom::right_of(t);
    let road = map.net.lanes[lane as usize].road;
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for (i, l) in map.net.lanes.iter().enumerate() {
        if l.road != road || l.kind == crate::network::LaneKind::Parking {
            continue;
        }
        let ls = map.seg(i as crate::SegId);
        let pr = ls.project(origin);
        if pr.dist > 16.0 {
            continue;
        }
        let (pl, _) = ls.point_at(pr.s);
        let d = geom::dot(geom::sub(geom::xy(pl), origin), right);
        lo = lo.min(d - ls.width * 0.5);
        hi = hi.max(d + ls.width * 0.5);
    }
    lo.is_finite().then_some(RoadCorridor {
        origin,
        dir: t,
        lat_min: lo,
        lat_max: hi,
    })
}

/// Points and tangents every 5 m along the lane a car at `pos` facing `forward` is in, from the
/// car up to `dist` metres or the end of the lane (the junction). Empty off the road.
pub fn lane_profile(map: &crate::CityMap, pos: V3, forward: V2, dist: f32) -> Vec<(V3, V2)> {
    let Some(loc) = map.locate(pos, Some(forward), 1.0, false, false) else {
        return Vec::new();
    };
    let seg = map.seg(loc.seg);
    let end = (loc.s + dist).min(seg.length);
    let mut out = Vec::new();
    let mut s = loc.s;
    while s <= end {
        out.push(seg.point_at(s));
        s += 5.0;
    }
    out
}

/// Whether a car at `pos` facing `forward` may overtake in the oncoming lane as far as sight
/// goes: the road ahead (up to `dist` m or the next junction) has no crest hiding oncoming cars
/// and no bend sharper than 20°. (Traffic's own rule; police patrols should use it too.)
pub fn overtake_sight_clear(map: &crate::CityMap, pos: V3, forward: V2, dist: f32) -> bool {
    let profile = lane_profile(map, pos, forward, dist);
    !profile.is_empty() && sight_clear(&profile, 0.35)
}

// ---------------------------------------------------------------------------------------------
// Physical execution
// ---------------------------------------------------------------------------------------------

/// Crawl speed for manoeuvres, m/s.
pub const CRAWL_MPS: f32 = 1.2;
/// Time spent steering at a standstill before a leg starts, s.
const DRY_STEER_S: f32 = 0.5;

/// Low-speed pedal and gear handling for a `wp_vehicle` with automatic gears (AI assists):
/// creeping forward or backward at a target speed, engaging reverse (brake held at a standstill)
/// and first gear (a touch of throttle), and holding still on the handbrake.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Crawl;

impl Crawl {
    /// Input that holds the car still. In reverse gear only the handbrake is used: the pedals
    /// are swapped there, and a brake press would drive the car backwards.
    pub fn hold(st: &VehicleState, steer: f32) -> VehicleInput {
        VehicleInput {
            steer,
            brake: if st.gear >= 1 { 0.4 } else { 0.0 },
            handbrake: 1.0,
            ..Default::default()
        }
    }

    /// Input that creeps the car `forward` (or backwards) at `speed` m/s with `steer`, and
    /// whether the right gear is engaged yet (while it is not, the car is held).
    pub fn drive(st: &VehicleState, forward: bool, speed: f32, steer: f32) -> (VehicleInput, bool) {
        let v = st.speed_mps;
        if forward {
            if st.gear < 1 {
                if v < -0.5 {
                    return (Self::hold(st, steer), false);
                }
                // A touch of throttle at a standstill selects first gear.
                let inp = VehicleInput {
                    steer,
                    throttle: 0.15,
                    handbrake: 1.0,
                    ..Default::default()
                };
                return (inp, false);
            }
            let err = speed - v;
            let mut inp = VehicleInput {
                steer,
                ..Default::default()
            };
            if err >= -0.3 {
                inp.throttle = (0.12 + 0.3 * err).clamp(0.0, 0.5);
            } else {
                inp.brake = (0.2 - 0.3 * err).clamp(0.0, 0.45);
            }
            (inp, true)
        } else {
            if st.gear >= 1 {
                if v > 0.4 {
                    return (Self::hold(st, steer), false);
                }
                // Brake held at a standstill selects reverse (wp_vehicle auto-reverse).
                let inp = VehicleInput {
                    steer,
                    brake: 0.7,
                    ..Default::default()
                };
                return (inp, false);
            }
            // Reverse: the brake pedal drives, the throttle pedal brakes.
            let err = speed - (-v);
            let mut inp = VehicleInput {
                steer,
                ..Default::default()
            };
            if err >= -0.3 {
                inp.brake = (0.12 + 0.3 * err).clamp(0.0, 0.5);
            } else if v < -0.6 {
                inp.throttle = (0.2 - 0.3 * err).clamp(0.0, 0.45);
            } else {
                inp.handbrake = 1.0;
            }
            (inp, true)
        }
    }
}

/// Outcome of a manoeuvre step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManoeuvreStatus {
    /// Still going.
    Running,
    /// Finished: the car faces the goal (turn) / reached the end of the path (reverse).
    Done,
    /// No way to continue (re-planning found no room, or it took too long).
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    /// Steering at a standstill and engaging the gear for `leg`.
    Prepare { t: f32 },
    /// Driving the leg.
    Move,
    /// Stopping before the next leg.
    Stop,
}

/// Drives a physical vehicle through a k-point turn ([`plan_turn`]), closed loop: before each
/// leg it re-plans from the measured pose and drives only that plan's first leg, stopping
/// early if the body would get within [`TURN_EDGE_MARGIN_M`]` / 2` of the corridor edge or
/// into an obstacle box. Call [`TurnDriver::step`] at a fixed rate (60 Hz in traffic).
#[derive(Debug, Clone, PartialEq)]
pub struct TurnDriver {
    /// Car being driven.
    pub shape: CarShape,
    /// Road corridor.
    pub road: RoadCorridor,
    /// Direction to face at the end.
    pub goal: V2,
    /// Obstacles to keep clear of (refresh with [`TurnDriver::set_obstacles`]).
    obstacles: Vec<Obb>,
    leg: Option<TurnLeg>,
    leg_start: V2,
    travelled: f32,
    phase: Phase,
    legs: usize,
    elapsed_s: f32,
}

/// A turn taking longer than this has failed, s.
pub const TURN_TIMEOUT_S: f32 = 60.0;

impl TurnDriver {
    /// Driver for a turn to face `goal` inside `road`, clear of `obstacles`.
    pub fn new(shape: CarShape, road: RoadCorridor, goal: V2, obstacles: Vec<Obb>) -> Self {
        Self {
            shape,
            road,
            goal,
            obstacles,
            leg: None,
            leg_start: [0.0; 2],
            travelled: 0.0,
            phase: Phase::Stop,
            legs: 0,
            elapsed_s: 0.0,
        }
    }

    /// Replace the obstacle boxes (other cars move).
    pub fn set_obstacles(&mut self, obstacles: Vec<Obb>) {
        self.obstacles = obstacles;
    }

    /// Legs started so far.
    pub fn legs_driven(&self) -> usize {
        self.legs
    }

    /// One control step for a car in `st`, `dt` seconds long.
    pub fn step(&mut self, st: &VehicleState, dt: f32) -> (VehicleInput, ManoeuvreStatus) {
        self.elapsed_s += dt;
        let (pos, f) = crate::drive::pose_2d(st);
        let rear = self.shape.rear(pos, f);
        let steer_now = self.leg.map_or(0.0, |l| l.steer());
        if self.elapsed_s > TURN_TIMEOUT_S || self.legs > MAX_TURN_LEGS + 3 {
            return (Crawl::hold(st, steer_now), ManoeuvreStatus::Failed);
        }
        match self.phase {
            Phase::Stop => {
                if st.speed_mps.abs() > 0.15 {
                    return (Crawl::hold(st, steer_now), ManoeuvreStatus::Running);
                }
                if geom::dot(f, self.goal) >= TURN_DONE_COS {
                    return (Crawl::hold(st, 0.0), ManoeuvreStatus::Done);
                }
                let Some(plan) =
                    plan_turn(&self.shape, &self.road, &self.obstacles, rear, f, self.goal)
                else {
                    return (Crawl::hold(st, steer_now), ManoeuvreStatus::Failed);
                };
                let Some(&leg) = plan.legs.first() else {
                    return (Crawl::hold(st, 0.0), ManoeuvreStatus::Done);
                };
                self.leg = Some(leg);
                self.legs += 1;
                self.phase = Phase::Prepare { t: 0.0 };
                (Crawl::hold(st, leg.steer()), ManoeuvreStatus::Running)
            }
            Phase::Prepare { t } => {
                let leg = self.leg.expect("leg planned");
                let t = t + dt;
                if t < DRY_STEER_S {
                    self.phase = Phase::Prepare { t };
                    return (Crawl::hold(st, leg.steer()), ManoeuvreStatus::Running);
                }
                let (inp, ready) = Crawl::drive(st, leg.forward, CRAWL_MPS, leg.steer());
                if ready {
                    self.phase = Phase::Move;
                    self.leg_start = rear;
                    self.travelled = 0.0;
                } else {
                    self.phase = Phase::Prepare { t };
                }
                (inp, ManoeuvreStatus::Running)
            }
            Phase::Move => {
                let leg = self.leg.expect("leg planned");
                self.travelled += st.speed_mps.abs() * dt;
                // Stop early if half a metre further would touch an edge or an obstacle.
                let ahead = if leg.forward { 0.5 } else { -0.5 };
                let (pr, pf) = arc_pose(rear, f, leg.curvature, ahead);
                let body = self.shape.body(pr, pf, 0.0);
                let edge = self.road.excess(&body, TURN_EDGE_MARGIN_M * 0.5) > 0.0
                    && self.road.excess(&body, TURN_EDGE_MARGIN_M * 0.5)
                        > self
                            .road
                            .excess(&self.shape.body(rear, f, 0.0), TURN_EDGE_MARGIN_M * 0.5);
                let hit = self.obstacles.iter().any(|o| {
                    geom::obb_overlap(&self.shape.body(pr, pf, TURN_OBSTACLE_MARGIN_M * 0.5), o)
                });
                let at_goal = geom::dot(f, self.goal) >= TURN_DONE_COS;
                // Remaining distance → slow down for the end of the leg.
                let left = (leg.length_m - self.travelled).max(0.0);
                if left < 0.05 || edge || hit || at_goal {
                    self.phase = Phase::Stop;
                    return (Crawl::hold(st, leg.steer()), ManoeuvreStatus::Running);
                }
                let speed = CRAWL_MPS.min((2.0 * 0.8 * left).sqrt() + 0.2);
                let (inp, _) = Crawl::drive(st, leg.forward, speed, leg.steer());
                (inp, ManoeuvreStatus::Running)
            }
        }
    }
}

/// Reverses a physical vehicle along a path (e.g. back down a blocked one-way street to the
/// junction it came from), steering the rear axle onto the path with pure pursuit. The path
/// points run in the direction of travel (backwards), so its end is where the car stops.
#[derive(Debug, Clone, PartialEq)]
pub struct ReverseDriver {
    /// Car being driven.
    pub shape: CarShape,
    /// Path for the rear axle.
    pub path: crate::drive::DrivePath,
    /// Target speed, m/s.
    pub speed_mps: f32,
    s_hint: Option<f32>,
    elapsed_s: f32,
}

impl ReverseDriver {
    /// Driver reversing along `points` (in travel order) at `speed_mps`.
    pub fn new(shape: CarShape, points: &[V3], speed_mps: f32) -> Self {
        Self {
            shape,
            path: crate::drive::DrivePath::new(points, false),
            speed_mps,
            s_hint: None,
            elapsed_s: 0.0,
        }
    }

    /// Distance left to the end of the path from the car in `st`, m.
    pub fn remaining(&self, st: &VehicleState) -> f32 {
        let (pos, f) = crate::drive::pose_2d(st);
        let (s, _, _) = self.path.project(self.shape.rear(pos, f), self.s_hint);
        (self.path.length() - s).max(0.0)
    }

    /// One control step.
    pub fn step(&mut self, st: &VehicleState, dt: f32) -> (VehicleInput, ManoeuvreStatus) {
        self.elapsed_s += dt;
        let (pos, f) = crate::drive::pose_2d(st);
        let rear = self.shape.rear(pos, f);
        let (s, _, dist) = self.path.project(rear, self.s_hint);
        self.s_hint = Some(s);
        let left = (self.path.length() - s).max(0.0);
        if left < 0.4 {
            let done = st.speed_mps.abs() < 0.15;
            let status = if done {
                ManoeuvreStatus::Done
            } else {
                ManoeuvreStatus::Running
            };
            return (Crawl::hold(st, 0.0), status);
        }
        if self.elapsed_s > TURN_TIMEOUT_S * 2.0 || dist > 4.0 {
            return (Crawl::hold(st, 0.0), ManoeuvreStatus::Failed);
        }
        let look = 3.5f32;
        let (tp, _) = self.path.point_at(s + look);
        // Pure pursuit seen from the rear axle looking backwards; reversing turns the other way.
        let back = [-f[0], -f[1]];
        let d = geom::sub(geom::xy(tp), rear);
        let long = geom::dot(d, back);
        let latd = geom::dot(d, geom::right_of(back));
        let l2 = (long * long + latd * latd).max(1.0);
        let delta = crate::geom::atan(self.shape.wheelbase_m * (-2.0 * latd / l2));
        let steer = (delta / self.shape.max_steer_rad).clamp(-1.0, 1.0);
        let speed = self.speed_mps.min((2.0 * 0.8 * left).sqrt() + 0.2);
        let (inp, _) = Crawl::drive(st, false, speed, steer);
        (inp, ManoeuvreStatus::Running)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn road(width_left: f32, width_right: f32) -> RoadCorridor {
        RoadCorridor {
            origin: [0.0, 0.0],
            dir: [0.0, 1.0],
            lat_min: -width_left,
            lat_max: width_right,
        }
    }

    fn check_inside(shape: &CarShape, road: &RoadCorridor, obstacles: &[Obb], plan: &TurnPlan) {
        for (k, l) in plan.legs.iter().enumerate() {
            let mut d = 0.0;
            while d <= l.length_m {
                let (r, f) = plan.pose(k, d);
                let b = shape.body(r, f, 0.0);
                assert!(
                    road.excess(&b, 0.0) < 1e-3,
                    "leg {k} at {d}: outside the road"
                );
                assert!(
                    !obstacles.iter().any(|o| geom::obb_overlap(&b, o)),
                    "leg {k} at {d}: hits an obstacle"
                );
                d += 0.05;
            }
        }
    }

    #[test]
    fn arc_pose_is_a_circle() {
        let k = 0.25;
        let (r, f) = arc_pose([0.0, 0.0], [0.0, 1.0], k, std::f32::consts::PI / k);
        // Half a circle of radius 4 to the left: ends 8 m left, facing back.
        assert!((r[0] + 8.0).abs() < 1e-3 && r[1].abs() < 1e-3, "{r:?}");
        assert!((f[1] + 1.0).abs() < 1e-4);
        // Reversing with right lock also rotates counter-clockwise.
        let (_, f) = arc_pose([0.0, 0.0], [0.0, 1.0], -k, -1.0);
        assert!(f[0] < 0.0, "{f:?}");
    }

    #[test]
    fn wide_road_gets_a_u_turn_and_narrow_road_a_k_point_turn() {
        let shape = CarShape::CIVILIAN;
        let goal = [0.0, -1.0];
        // 3+3 lanes (car in the left lane of its direction): one sweep.
        let wide = road(3.5 * 3.0 + 1.75, 1.75 + 7.0);
        let p = plan_turn(&shape, &wide, &[], [0.0, -1.35], [0.0, 1.0], goal).unwrap();
        assert_eq!(p.legs.len(), 1, "{p:?}");
        check_inside(&shape, &wide, &[], &p);
        // A 1+1 street (7 m): several points, all inside the kerbs, ends facing back.
        let narrow = road(3.5 + 1.75, 1.75);
        let p = plan_turn(&shape, &narrow, &[], [0.0, -1.35], [0.0, 1.0], goal).unwrap();
        assert!((2..=7).contains(&p.legs.len()), "{p:?}");
        check_inside(&shape, &narrow, &[], &p);
        let (_, f) = p.end_pose();
        assert!(geom::dot(f, goal) >= TURN_DONE_COS);
        // Too narrow for any turn (one 3 m alley lane).
        assert!(plan_turn(&shape, &road(1.5, 1.5), &[], [0.0, -1.35], [0.0, 1.0], goal).is_none());
    }

    #[test]
    fn turn_keeps_clear_of_the_blocker_ahead() {
        let shape = CarShape::CIVILIAN;
        let narrow = road(3.5 + 1.75, 1.75);
        // Stopped car 6 m ahead (bumper to bumper) in the same lane; follower 2 m behind.
        let blocker = Obb {
            c: [0.0, 2.25 + 6.0 + 2.25],
            f: [0.0, 1.0],
            hl: 2.25,
            hw: 0.95,
        };
        let follower = Obb {
            c: [0.0, -(2.25 + 2.0 + 2.25)],
            ..blocker
        };
        let obs = [blocker, follower];
        let p = plan_turn(&shape, &narrow, &obs, [0.0, -1.35], [0.0, 1.0], [0.0, -1.0]).unwrap();
        check_inside(&shape, &narrow, &obs, &p);
    }

    #[test]
    fn sight_is_blocked_by_a_crest_and_a_bend() {
        let flat: Vec<(V3, V2)> = (0..20)
            .map(|k| ([0.0, k as f32 * 5.0, 0.0], [0.0, 1.0]))
            .collect();
        assert!(sight_clear(&flat, 0.35));
        // Up 6 % for 40 m then down 6 %: the far side is hidden.
        let crest: Vec<(V3, V2)> = (0..20)
            .map(|k| {
                let y = k as f32 * 5.0;
                let z = if y < 40.0 {
                    0.06 * y
                } else {
                    2.4 - 0.06 * (y - 40.0)
                };
                ([0.0, y, z], [0.0, 1.0])
            })
            .collect();
        assert!(!sight_clear(&crest, 0.35));
        // A steady climb is fine.
        let climb: Vec<(V3, V2)> = (0..20)
            .map(|k| ([0.0, k as f32 * 5.0, k as f32 * 0.4], [0.0, 1.0]))
            .collect();
        assert!(sight_clear(&climb, 0.35));
        // 30° bend.
        let mut bend = flat.clone();
        bend[15].1 = geom::normalize([0.5, 0.866]);
        assert!(!sight_clear(&bend, 0.35));
    }
}
