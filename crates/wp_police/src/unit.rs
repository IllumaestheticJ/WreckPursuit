//! Police units: state, assignments, and the built-in kinematic driver.
//!
//! In the full game each ground unit is a `wp_vehicle` car in `wp_sim`, driven by the shared
//! AI controller from a [`DriveCommand`]; `wp_sim` reports the result back with
//! `PoliceSystem::sync_unit`. Until prompt 05's controller and prompt 12's spawning exist,
//! units are driven by [`step_ground`] here: a point-mass car that follows a polyline, plans
//! corner speeds from a lateral-g budget, saturates at a tire grip limit and crashes when it
//! runs wide. It is deliberately simple; it exists so that pursuit tactics, risk budgets and
//! self-wreck rates can be exercised headless.

use serde::{Deserialize, Serialize};
use wp_contracts::{DamageMode, DayPhase, EntityId, HandlingEffects, PlayerId, GRAVITY_MPS2};
use wp_math::{sin_cos, Rng};
use wp_world::drive::{DriveController, DriveGains};

use crate::avoid::AvoidState;
use crate::config::PursuitFile;
use crate::events::{IncidentId, ToolId, UnitId, UnitKind};
use crate::graph::{
    add, cross, dist, dist2, dot, len, lerp2, normalize, scale, sub, District, NodeId, SegmentId,
};
use crate::time::SimTime;

/// A polyline the unit drives along, with cumulative arc length.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// Points, m.
    pub pts: Vec<[f32; 2]>,
    /// Cumulative arc length at each point, m.
    pub cum: Vec<f32>,
}

impl Track {
    /// Build from points (consecutive duplicates removed).
    pub fn new(points: Vec<[f32; 2]>) -> Self {
        let mut pts: Vec<[f32; 2]> = Vec::with_capacity(points.len());
        for p in points {
            if pts.last().is_none_or(|l| dist2(*l, p) > 0.01) {
                pts.push(p);
            }
        }
        let mut cum = Vec::with_capacity(pts.len());
        let mut acc = 0.0;
        for i in 0..pts.len() {
            if i > 0 {
                acc += dist(pts[i - 1], pts[i]);
            }
            cum.push(acc);
        }
        Self { pts, cum }
    }

    /// Total length, m.
    pub fn length(&self) -> f32 {
        self.cum.last().copied().unwrap_or(0.0)
    }

    /// Whether the track has at least one leg.
    pub fn is_drivable(&self) -> bool {
        self.pts.len() >= 2
    }

    /// Point at arc length `s` (clamped; extrapolates past the end along the last leg).
    pub fn point_at(&self, s: f32) -> [f32; 2] {
        match self.pts.len() {
            0 => [0.0, 0.0],
            1 => self.pts[0],
            n => {
                let s = s.max(0.0);
                // Binary search the leg.
                let i = match self.cum.binary_search_by(|c| c.total_cmp(&s)) {
                    Ok(i) => i.min(n - 2),
                    Err(i) => i.saturating_sub(1).min(n - 2),
                };
                let l = self.cum[i + 1] - self.cum[i];
                let k = if l > 0.0 { (s - self.cum[i]) / l } else { 0.0 };
                lerp2(self.pts[i], self.pts[i + 1], k)
            }
        }
    }

    /// Project `p` onto the track, searching legs around arc length `hint` (±`window` m).
    /// Returns `(s, lateral distance)`.
    pub fn project(&self, p: [f32; 2], hint: f32, window: f32) -> (f32, f32) {
        self.project_ahead(p, hint, window, window)
    }

    /// Project `p` onto the track within `[hint - behind, hint + ahead]` (progress tracking:
    /// a small `behind` keeps hairpins and U-turns from snapping back onto the earlier leg).
    /// Returns `(s, lateral distance)`; searches the whole track if nothing lies in the window.
    pub fn project_ahead(&self, p: [f32; 2], hint: f32, behind: f32, ahead: f32) -> (f32, f32) {
        if self.pts.len() < 2 {
            return (0.0, self.pts.first().map(|q| dist(*q, p)).unwrap_or(0.0));
        }
        let (from, to) = (hint - behind, hint + ahead);
        let mut best = (0.0f32, f32::INFINITY);
        // Legs overlapping [from, to] (cum is ascending).
        let lo = self.cum.partition_point(|c| *c < from).saturating_sub(1);
        let hi = self
            .cum
            .partition_point(|c| *c <= to)
            .min(self.pts.len() - 1);
        for i in lo..hi.max(lo) {
            if self.cum[i + 1] < from || self.cum[i] > to {
                continue;
            }
            let (t, _) = crate::graph::closest_on_segment(self.pts[i], self.pts[i + 1], p);
            let s = (self.cum[i] + t * (self.cum[i + 1] - self.cum[i])).max(from);
            let d = dist(self.point_at(s), p);
            if d < best.1 {
                best = (s, d);
            }
        }
        if best.1.is_infinite() {
            return self.project_ahead(p, hint, f32::INFINITY, f32::INFINITY);
        }
        best
    }

    /// Radius of curvature at arc length `s` from the circumcircle of the points `h` metres
    /// before and after (`f32::INFINITY` when straight).
    pub fn radius_at(&self, s: f32, h: f32) -> f32 {
        let a = self.point_at(s - h);
        let b = self.point_at(s);
        let c = self.point_at(s + h);
        // A reversal (U-turn) is collinear but anything but straight.
        if dot(sub(b, a), sub(c, b)) < 0.0 {
            return 2.0;
        }
        let ab = dist(a, b);
        let bc = dist(b, c);
        let ca = dist(c, a);
        let area2 = cross(sub(b, a), sub(c, a)).abs();
        if area2 < 1e-4 {
            f32::INFINITY
        } else {
            ab * bc * ca / (2.0 * area2)
        }
    }

    /// Drop everything before arc length `s - keep` (keeps the track short as it is consumed).
    pub fn trim_before(&mut self, s: f32, keep: f32) {
        let cut = s - keep;
        let mut first = 0;
        while first + 2 < self.pts.len() && self.cum[first + 1] < cut {
            first += 1;
        }
        if first > 0 {
            let pts = self.pts.split_off(first);
            *self = Track::new(pts);
        }
    }
}

/// What a unit is doing. Every change emits a radio line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Assignment {
    /// Just spawned / between jobs.
    Idle,
    /// Patrolling toward a node in its district.
    Patrol {
        /// Destination node.
        dest: NodeId,
    },
    /// Responding to an incident.
    Respond {
        /// Incident.
        incident: IncidentId,
    },
    /// Pursuing a racer (tactic chosen by [`crate::pursuit`]).
    Pursue {
        /// Racer.
        target: EntityId,
    },
    /// Driving the road graph to a predicted racer position.
    Intercept {
        /// Racer.
        target: EntityId,
        /// Predicted point, m.
        point: [f32; 2],
    },
    /// Parked as part of a roadblock.
    RoadblockAt {
        /// Segment.
        segment: SegmentId,
        /// Roadblock.
        tool: ToolId,
    },
    /// Deployed a spike strip.
    SpikeAt {
        /// Segment.
        segment: SegmentId,
        /// Strip.
        tool: ToolId,
    },
    /// Helicopter tracking a racer.
    HeloTrack {
        /// Racer.
        target: EntityId,
    },
    /// Pulled over / returning; will be removed.
    OutOfService,
}

impl Assignment {
    /// Short name for logs and views.
    pub fn name(&self) -> &'static str {
        match self {
            Assignment::Idle => "idle",
            Assignment::Patrol { .. } => "patrol",
            Assignment::Respond { .. } => "respond",
            Assignment::Pursue { .. } => "pursue",
            Assignment::Intercept { .. } => "intercept",
            Assignment::RoadblockAt { .. } => "roadblock",
            Assignment::SpikeAt { .. } => "spikes",
            Assignment::HeloTrack { .. } => "helo-track",
            Assignment::OutOfService => "out-of-service",
        }
    }

    /// Racer this assignment is about, if any.
    pub fn target(&self) -> Option<EntityId> {
        match self {
            Assignment::Pursue { target }
            | Assignment::Intercept { target, .. }
            | Assignment::HeloTrack { target } => Some(*target),
            _ => None,
        }
    }
}

/// Pursuit tactic of a unit in `Pursue`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tactic {
    /// Hold 25–40 m behind, mirror the racer's line.
    Shadow,
    /// Box-in slot.
    BoxIn(BoxSlot),
    /// PIT attempt from the rear quarter.
    Pit,
    /// Slow down in front of the racer.
    RollingBlock,
    /// Deliberate contact (wanted level 2+): a rear-quarter shunt, a side slam or a hard ram.
    Ram(RamKind),
}

/// How a ramming unit hits the racer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RamKind {
    /// Nose into the racer's rear quarter (L2+).
    RearQuarter,
    /// Alongside, steer into its flank (L3+).
    Side,
    /// Straight into its tail at a closing speed (L3+).
    Hard,
}

/// Box-in slot relative to the racer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoxSlot {
    /// Behind.
    Rear,
    /// Ahead, left.
    FrontLeft,
    /// Ahead, right.
    FrontRight,
}

/// Why a unit exists (for pool accounting and clean-up).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitRole {
    /// District patrol pool unit.
    Pool(District),
    /// Spawned reinforcement.
    Reinforcement,
    /// Spawned to answer a reckless-driving call (no unit was close enough); despawns out of
    /// sight once the call is over.
    Responder,
    /// Parked roadblock / spike unit.
    Tool,
    /// Helicopter.
    Helicopter,
    /// Player-driven chase unit (never despawned by the dispatcher).
    Player,
}

/// Kinematic body (written by the built-in driver or by `sync_unit`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct UnitBody {
    /// XY position, m.
    pub pos: [f32; 2],
    /// Unit heading vector (XY).
    pub heading: [f32; 2],
    /// Forward speed, m/s.
    pub speed: f32,
    /// Current lateral acceleration, g.
    pub lat_g: f32,
    /// Altitude, m (helicopters).
    pub alt_m: f32,
}

impl UnitBody {
    /// Velocity, m/s.
    pub fn vel(&self) -> [f32; 2] {
        scale(self.heading, self.speed)
    }
}

/// What the AI wants the unit to do this think step (input to any driver).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DriveCommand {
    /// Path to follow.
    pub track: Track,
    /// Desired speed before corner planning, m/s.
    pub speed_mps: f32,
    /// Planning lateral acceleration, g.
    pub cornering_g: f32,
    /// Lights and siren.
    pub siren: bool,
    /// Drive this far right of the track centreline (keep to the lane), m. 0 for tactics that
    /// follow the racer's own line.
    pub lane_offset_m: f32,
}

/// Right-hand lane offset from the road centreline, m.
pub const LANE_OFFSET_M: f32 = 1.75;

impl DriveCommand {
    /// Stand still.
    pub fn stop() -> Self {
        Self {
            track: Track::default(),
            speed_mps: 0.0,
            cornering_g: 0.3,
            siren: false,
            lane_offset_m: 0.0,
        }
    }
}

/// One police unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Unit {
    /// Id.
    pub id: UnitId,
    /// Car or helicopter.
    pub kind: UnitKind,
    /// Why it exists.
    pub role: UnitRole,
    /// Vehicle body in `wp_sim`, once bound.
    pub entity: Option<EntityId>,
    /// Human driver, for player chase units.
    pub player: Option<PlayerId>,
    /// Driven by [`step_ground`] here (true) or by `wp_sim` + `sync_unit` (false).
    pub internal: bool,
    /// Body.
    pub body: UnitBody,
    /// Assignment.
    pub assignment: Assignment,
    /// Tactic while pursuing.
    pub tactic: Tactic,
    /// Current drive command.
    pub cmd: DriveCommand,
    /// Progress along `cmd.track`, m.
    pub track_s: f32,
    /// Speed allowed by corner planning (refreshed at think rate), m/s.
    pub planned_speed: f32,
    /// Speed limit where the unit is (refreshed at think rate), m/s.
    pub limit_mps: f32,
    /// The built-in driver has converged onto the current track (only then can it run wide).
    pub on_track: bool,
    /// Remaining risk budget.
    pub risk: f32,
    /// Health 0..=1.
    pub health: f32,
    /// Latest handling effects (from damage or the built-in driver's crashes).
    pub effects: HandlingEffects,
    /// Touched a racer since it was assigned to the current pursuit.
    pub racer_contact: bool,
    /// Total loss.
    pub wrecked: bool,
    /// Sim time when the current assignment began.
    pub assigned: SimTime,
    /// A captain order overrides the AI until this time.
    pub captain_hold_until: SimTime,
    /// Next PIT allowed at.
    pub pit_ready: SimTime,
    /// Whether this unit ever pursued (for statistics).
    pub engaged: bool,
    /// Steering geometry of the unit's car.
    pub geometry: SteerGeometry,
    /// `wp_world`'s shared driving controller (pure pursuit + PID), created on first use.
    #[serde(skip)]
    pub controller: Option<DriveController>,
    /// Obstacle awareness: speed cap behind the vehicle ahead, passing state.
    pub avoid: AvoidState,
    /// Stuck detection and recovery (physical units, [`crate::stuck`]).
    #[serde(default)]
    pub unstick: crate::stuck::UnstickState,
    /// Road route a pursuer is following to the racer (out of its sight), if any.
    #[serde(default)]
    pub chase_route: Option<ChaseRoute>,
}

/// A pursuer's road route to the racer (see `PoliceSystem::chase_route`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ChaseRoute {
    /// Junction the racer was heading for when the route was planned.
    pub node: NodeId,
    /// When it was planned.
    pub at: SimTime,
}

impl Unit {
    /// New unit at `pos` facing `heading`.
    pub fn new(
        id: UnitId,
        kind: UnitKind,
        role: UnitRole,
        pos: [f32; 2],
        heading: [f32; 2],
        risk: f32,
    ) -> Self {
        Self {
            id,
            kind,
            role,
            entity: None,
            player: None,
            internal: true,
            body: UnitBody {
                pos,
                heading: normalize(heading),
                speed: 0.0,
                lat_g: 0.0,
                alt_m: 0.0,
            },
            assignment: Assignment::Idle,
            tactic: Tactic::Shadow,
            cmd: DriveCommand::stop(),
            track_s: 0.0,
            planned_speed: 0.0,
            limit_mps: 13.9,
            on_track: false,
            risk,
            health: 1.0,
            effects: HandlingEffects::NEUTRAL,
            racer_contact: false,
            wrecked: false,
            assigned: SimTime::ZERO,
            captain_hold_until: SimTime::ZERO,
            pit_ready: SimTime::ZERO,
            engaged: false,
            geometry: SteerGeometry::default(),
            controller: None,
            avoid: AvoidState::default(),
            unstick: crate::stuck::UnstickState::default(),
            chase_route: None,
        }
    }

    /// The unit's driving controller (created with its geometry on first use).
    pub fn controller(&mut self) -> &mut DriveController {
        let (g, v) = (self.geometry, self.body.speed);
        self.controller.get_or_insert_with(|| {
            DriveController::new(DriveGains::TRAFFIC, g.wheelbase_m, g.max_steer_rad, v)
        })
    }

    /// Available for a new job (not busy with a tool, not out of service).
    pub fn available(&self) -> bool {
        !self.wrecked
            && self.kind == UnitKind::Car
            && matches!(
                self.assignment,
                Assignment::Idle | Assignment::Patrol { .. }
            )
    }

    /// Set a new drive command and reset track progress.
    pub fn set_command(&mut self, cmd: DriveCommand) {
        let (s, lat) = cmd.track.project(self.body.pos, 0.0, f32::INFINITY);
        self.track_s = s;
        self.on_track = lat < 2.0;
        self.cmd = cmd;
        // A pass is tied to the old track's arc length.
        self.avoid.pass_offset_m = None;
        self.avoid.blocked_s = 0.0;
        self.avoid.junction = None;
    }

    /// Whether handling damage forces the unit to pull over.
    pub fn must_pull_over(&self, cfg: &PursuitFile) -> bool {
        let t = &cfg.tactics;
        self.effects.wheel_lost.iter().any(|w| *w)
            || self
                .effects
                .toe_delta
                .iter()
                .any(|d| d.abs() > t.bent_wheel_toe_rad)
            || self
                .effects
                .tire_puncture
                .iter()
                .any(|p| *p > t.puncture_pull_over)
            || self.effects.engine_dead
    }

    /// Grip multiplier from handling effects (worst wheel; flats cost 60%).
    pub fn grip_scale(&self) -> f32 {
        let mut g: f32 = 1.0;
        for i in 0..4 {
            let w = self.effects.grip_scale[i] * (1.0 - 0.6 * self.effects.tire_puncture[i]);
            g = g.min(w);
        }
        g.max(0.1)
    }
}

/// Speed allowed by upcoming curvature on `track` from arc length `s0`, planning with
/// `cornering_g` and braking at `brake_mps2`.
pub fn plan_corner_speed(
    track: &Track,
    s0: f32,
    desired: f32,
    cornering_g: f32,
    brake_mps2: f32,
) -> f32 {
    if !track.is_drivable() {
        return 0.0;
    }
    let horizon = desired * desired / (2.0 * brake_mps2) + 30.0;
    let mut v2_allowed = desired * desired;
    let mut d = 0.0;
    while d <= horizon {
        let s = s0 + d;
        if s > track.length() {
            break;
        }
        let r = track.radius_at(s, 10.0);
        if r.is_finite() {
            let vc2 = cornering_g * GRAVITY_MPS2 * r;
            v2_allowed = v2_allowed.min(vc2 + 2.0 * brake_mps2 * d);
        }
        d += 5.0;
    }
    // Stop at the end of the track.
    let to_end = (track.length() - s0).max(0.0);
    v2_allowed = v2_allowed.min(2.0 * brake_mps2 * to_end);
    v2_allowed.max(0.0).sqrt()
}

/// Point at arc length `s` shifted `offset_m` to the right of the track direction.
pub fn lane_point(track: &Track, s: f32, offset_m: f32) -> [f32; 2] {
    let p = track.point_at(s);
    if offset_m == 0.0 {
        return p;
    }
    let d = normalize(sub(track.point_at(s + 1.0), track.point_at(s - 1.0)));
    add(p, scale([d[1], -d[0]], offset_m))
}

/// Planned speed for a unit: corner planning plus a crawl while the unit faces away from its
/// track (it has to turn around first).
pub fn plan_unit_speed(u: &Unit, brake_mps2: f32) -> f32 {
    let mut v = plan_corner_speed(
        &u.cmd.track,
        u.track_s,
        u.cmd.speed_mps,
        u.cmd.cornering_g,
        brake_mps2,
    );
    if u.cmd.track.is_drivable() {
        let ahead = u.cmd.track.point_at(u.track_s + 8.0);
        let to = sub(ahead, u.body.pos);
        if len(to) > 1.0 && dot(normalize(to), u.body.heading) < 0.3 {
            v = v.min(4.0);
        }
    }
    v
}

/// What happened to a ground unit during one built-in driver step.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StepReport {
    /// Ran wide and hit something.
    pub crashed: bool,
    /// Traffic near miss.
    pub near_miss: bool,
    /// Traffic collision.
    pub traffic_collision: bool,
}

/// Context for the built-in driver.
#[derive(Debug, Clone, Copy)]
pub struct DriveEnv {
    /// Speed limit where the unit is, m/s.
    pub limit_mps: f32,
    /// Day phase (traffic density).
    pub phase: DayPhase,
    /// Damage mode (crash severity).
    pub mode: DamageMode,
    /// Whether hazards are simulated.
    pub hazards: bool,
}

/// Advance a ground unit by `dt` with the built-in point-mass driver.
pub fn step_ground(
    u: &mut Unit,
    env: &DriveEnv,
    cfg: &PursuitFile,
    rng: &mut Rng,
    dt: f32,
) -> StepReport {
    let mut rep = StepReport::default();
    let dynm = &cfg.unit;
    let grip_scale = u.grip_scale();
    let geom = u.geometry;
    let speed = u.body.speed;
    let look = u.controller().lookahead(speed);
    let track = &u.cmd.track;
    let b = &mut u.body;
    if !track.is_drivable() || u.wrecked {
        b.speed = (b.speed - dynm.brake_mps2 * dt).max(0.0);
        b.pos = add(b.pos, scale(b.heading, b.speed * dt));
        b.lat_g = 0.0;
        return rep;
    }
    let (s, lateral) = track.project_ahead(b.pos, u.track_s, 3.0, 40.0);
    u.track_s = s;
    if lateral < 2.0 {
        u.on_track = true;
    }

    // Steering: wp_world's shared controller (pure pursuit) toward a lookahead point, then a
    // kinematic bicycle (curvature = tan(road-wheel angle) / wheelbase).
    let offset = u.avoid.pass_offset_m.unwrap_or(u.cmd.lane_offset_m);
    let target = lane_point(track, s + look, offset);
    let to = sub(target, b.pos);
    let mut steer = u
        .controller
        .as_ref()
        .expect("controller created above")
        .steer(b.pos, b.heading, [target[0], target[1], 0.0]);
    // Target behind: pure pursuit degenerates (lateral offset → 0), so turn at full lock.
    if dot(b.heading, to) < 0.0 {
        steer = if cross(b.heading, to) >= 0.0 {
            -1.0
        } else {
            1.0
        };
    }
    let delta = -steer * geom.max_steer_rad; // + = left
    let (sd, cd) = sin_cos(delta);
    let curvature = sd / cd.max(1e-3) / geom.wheelbase_m.max(0.5);
    let mut a_lat = b.speed * b.speed * curvature;
    let grip = dynm.grip_g * GRAVITY_MPS2 * grip_scale;
    if a_lat.abs() > grip {
        a_lat = a_lat.signum() * grip;
    }
    let yaw_rate = if b.speed > 0.5 {
        a_lat / b.speed
    } else {
        curvature // turning on the spot at ~1 m/s while nearly stopped
    };
    let (sn, cs) = sin_cos(yaw_rate * dt);
    b.heading = normalize([
        b.heading[0] * cs - b.heading[1] * sn,
        b.heading[0] * sn + b.heading[1] * cs,
    ]);
    b.lat_g = a_lat.abs() / GRAVITY_MPS2;

    // Longitudinal.
    let want = u.cmd.speed_mps.min(u.planned_speed).min(u.avoid.limit_mps);
    let accel = dynm.accel_mps2 * u.effects.power_scale.clamp(0.0, 1.0);
    let brake = dynm.brake_mps2 * 1.3;
    let dv = (want - b.speed).clamp(-brake * dt, accel * dt);
    b.speed = (b.speed + dv).max(0.0);
    let ds = b.speed * dt;
    b.pos = add(b.pos, scale(b.heading, ds));

    // Ran wide: hit the kerb / building.
    // Behind the start of the track (turning around) is not running wide.
    if u.on_track && s > 1.0 && lateral > dynm.crash_offset_m && b.speed > 5.0 {
        let impact = b.speed * 0.6;
        u.health -= dynm.crash_health_per_kmps2.get(env.mode) * impact * impact / 1000.0;
        rep.crashed = true;
        b.speed = 0.0;
        b.pos = track.point_at(s);
        if impact > 10.0 {
            // Hard hits bend a wheel.
            let w = rng.below(4) as usize;
            u.effects.toe_delta[w] += 0.03;
        }
    }

    // Background traffic hazards (stand-in for physical traffic).
    if env.hazards && b.speed > 0.5 * env.limit_mps && env.limit_mps > 0.0 {
        let h = &cfg.hazard;
        let ratio = b.speed / env.limit_mps;
        let p = h.near_miss_per_km.get(env.phase) * powu(ratio, h.speed_exponent) * ds / 1000.0;
        if rng.chance(p) {
            rep.near_miss = true;
            let pc = h.collision_chance_at_2x * (ratio - 1.0).max(0.0);
            if rng.chance(pc) {
                rep.traffic_collision = true;
                let v = b.speed
                    * rng.range_f32(
                        h.collision_speed_fraction.min,
                        h.collision_speed_fraction.max,
                    );
                u.health -= dynm.contact_health_per_kmps2.get(env.mode) * v * v / 1000.0;
                b.speed *= 0.3;
            }
        }
    }
    u.health = u.health.max(0.0);
    rep
}

/// `x^n` by repeated multiplication (bit-identical on every target, unlike `powf`).
pub fn powu(x: f32, n: u32) -> f32 {
    let mut r = 1.0;
    for _ in 0..n {
        r *= x;
    }
    r
}

/// Advance a helicopter toward `target` (XY) by `dt`.
pub fn step_helo(u: &mut Unit, target: [f32; 2], speed: f32, altitude: f32, dt: f32) {
    let b = &mut u.body;
    let to = sub(target, b.pos);
    let d = len(to);
    let v = speed.min(d / dt.max(1e-3));
    if d > 1e-3 {
        b.heading = scale(to, 1.0 / d);
    }
    b.speed = v;
    b.pos = add(b.pos, scale(b.heading, v * dt));
    b.alt_m += (altitude - b.alt_m).clamp(-8.0 * dt, 8.0 * dt);
}

/// Steering geometry of the vehicle a [`DriveCommand`] is turned into input for.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SteerGeometry {
    /// Wheelbase, m (`VehicleDef::wheelbase_m`).
    pub wheelbase_m: f32,
    /// Road-wheel angle at full lock, rad (`WheelDef::max_steer_rad` of a front wheel).
    pub max_steer_rad: f32,
}

impl Default for SteerGeometry {
    fn default() -> Self {
        Self {
            wheelbase_m: 2.7,
            max_steer_rad: 0.6,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PoliceConfig;

    fn l_track() -> Track {
        Track::new(vec![[0.0, 0.0], [0.0, 200.0], [200.0, 200.0]])
    }

    #[test]
    fn track_point_and_project() {
        let t = l_track();
        assert_eq!(t.length(), 400.0);
        assert_eq!(t.point_at(250.0), [50.0, 200.0]);
        let (s, d) = t.project([3.0, 100.0], 0.0, 1000.0);
        assert!((s - 100.0).abs() < 1e-3 && (d - 3.0).abs() < 1e-3);
    }

    #[test]
    fn corner_radius_and_planning() {
        let t = l_track();
        assert!(t.radius_at(100.0, 10.0).is_infinite());
        let r = t.radius_at(200.0, 10.0);
        assert!(r > 5.0 && r < 10.0, "{r}");
        let far = plan_corner_speed(&t, 0.0, 30.0, 0.6, 6.5);
        assert_eq!(far, 30.0);
        let near = plan_corner_speed(&t, 185.0, 30.0, 0.6, 6.5);
        assert!(near < 15.0, "{near}");
    }

    #[test]
    fn ground_unit_drives_corner_without_crashing() {
        let cfg = PoliceConfig::builtin().pursuit;
        let mut u = Unit::new(
            UnitId(1),
            UnitKind::Car,
            UnitRole::Reinforcement,
            [0.0, 0.0],
            [0.0, 1.0],
            10.0,
        );
        u.set_command(DriveCommand {
            track: l_track(),
            speed_mps: 25.0,
            cornering_g: cfg.pursuit_cornering_g,
            siren: true,
            lane_offset_m: 0.0,
        });
        let env = DriveEnv {
            limit_mps: 14.0,
            phase: DayPhase::Day,
            mode: DamageMode::Simulation,
            hazards: false,
        };
        let mut rng = Rng::new(1);
        let dt = 1.0 / 240.0;
        for i in 0..240 * 30 {
            if i % 24 == 0 {
                u.planned_speed = plan_corner_speed(
                    &u.cmd.track,
                    u.track_s,
                    25.0,
                    cfg.pursuit_cornering_g,
                    cfg.unit.brake_mps2,
                );
            }
            let r = step_ground(&mut u, &env, &cfg, &mut rng, dt);
            assert!(!r.crashed, "crashed at tick {i} pos {:?}", u.body.pos);
        }
        assert!(u.body.pos[0] > 150.0, "{:?}", u.body.pos);
    }
}
