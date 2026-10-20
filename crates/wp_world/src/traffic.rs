//! Civilian traffic: lane-following agents with IDM car-following, MOBIL lane changes,
//! intersection right-of-way, hazard reactions, density-driven spawning and two LODs.
//!
//! ## Model
//! - Agents live on [`Segment`](crate::map::Segment)s (lanes and junction connectors) at arc
//!   length `s` (car centre) with speed `v`. They plan two segments ahead (`plan`).
//! - Longitudinal: Intelligent Driver Model against the nearest of: the car ahead (own segment
//!   or the planned ones, plus diverging sibling connectors), tracked vehicles (players/police)
//!   in the path, the stop line when the agent has no permit to enter the junction, debris.
//!   Above the desired speed the free term is the IIDM braking form (never harsher than the
//!   comfortable deceleration).
//! - Junctions: an agent may enter a connector only with a **permit**. Permits follow the
//!   control rule (signal state with yellow dilemma logic, right-on-red after a full stop,
//!   all-way stop FIFO, priority/minor stop-and-yield), require every conflicting connector to
//!   be empty and unreserved, require room on the target lane (don't block the box), and make
//!   lower-ranked movements (permissive lefts, minor roads, right-on-red) accept a gap against
//!   approaching higher-ranked traffic. Granting reserves the connector until the agent leaves
//!   it.
//! - Lane changes: MOBIL (safety + politeness-weighted incentive, keep-right bias) on multi-lane
//!   roads away from junctions; the changing car leaves a ghost in its old lane until clear.
//! - Hazards: emergency braking on time-to-collision with a tracked vehicle approaching head-on
//!   or crossing; sirens within 60 m make agents pull right, slow down and not enter junctions;
//!   debris ahead makes them swerve lightly (or stop if it cannot be passed).
//! - LOD: kinematic agents integrate along their path at 60 Hz. Agents within the physical
//!   radius of a player or active race route become `wp_vehicle` instances driven by a
//!   pure-pursuit + PID controller; promotion/demotion copy pose and speed.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use wp_contracts::{
    AssistProfile, EntityId, HandlingEffects, SignalState, SimEvent, VehicleDef, VehicleInput,
    VehicleSim,
};
use wp_math::{Rng, StateHasher};
use wp_vehicle::{TireDb, Vehicle};

use crate::density::{f_distance, f_time};
use crate::drive::{DriveController, DriveGains};
use crate::geom::{self, Obb, V2, V3};
use crate::map::{CityMap, SegId, SegKind, NO_SEG};
use crate::network::{IntersectionControl, LaneKind, TurnKind, ZoneId};

/// Civilian car length used for spacing and overlap tests, m.
pub const CAR_LENGTH_M: f32 = 4.5;
/// Civilian car width, m.
pub const CAR_WIDTH_M: f32 = 1.9;
/// Traffic AI update period (60 Hz), s.
pub const TRAFFIC_DT: f32 = 1.0 / 60.0;
/// Physics ticks per traffic step (240 Hz / 60 Hz).
pub const TICKS_PER_TRAFFIC_STEP: u64 = 4;
/// A discretionary lane change is indicated at least this long before it starts, s.
pub const LANE_CHANGE_SIGNAL_S: f32 = 0.5;

/// Siren reaction radius, m.
pub const SIREN_RADIUS_M: f32 = 60.0;

const NONE: u32 = NO_SEG;
const HALF_L: f32 = CAR_LENGTH_M * 0.5;
const SPAWN_PERIOD_S: f32 = 0.25;

/// Seconds spanned by `steps` traffic steps.
pub fn steps_to_s(steps: u64) -> f32 {
    steps as f32 * TRAFFIC_DT
}

/// Stable traffic agent id (never reused within a world).
pub type AgentId = u32;

/// Driver personality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Personality {
    /// 70 %: drives at or slightly under the limit, long headways.
    LawAbiding,
    /// 25 %: limit +10 %, medium headways.
    Casual,
    /// 5 %: fast, short headways, runs yellows, small gaps.
    Aggressive,
}

/// Per-driver parameters sampled from the personality (once, at spawn, from the traffic RNG).
///
/// Every driver is different within its personality: desired speed, headway, acceleration
/// and braking comfort, how quickly it pulls away at a green, how it treats ambers and how
/// eager it is to change lanes. The ranges (owner playtest row 14, "NPC cars need variance"):
///
/// | | law-abiding (70 %) | casual (25 %) | aggressive (5 %) |
/// |---|---|---|---|
/// | speed × limit | 0.80–0.88 (12 % slow) or 0.88–1.03 | 1.03–1.13 | 1.13–1.25 |
/// | extra on highways | −4…+5 % | 0…+8 % | +3…+12 % |
/// | headway, s | 1.6–2.2 | 1.3–1.6 | 1.1–1.3 |
/// | accel / comfort brake, m/s² | 1.1–1.7 / 2.0–2.8 | 1.5–2.1 / 2.6–3.3 | 2.2–2.8 / 3.2–3.8 |
/// | start delay (queue moving, straight/right at green), s | 0.5–1.0 (10 % hesitant: 1.2–2.2) | 0.35–0.75 (5 % hesitant) | 0.15–0.4 |
/// | runs an amber when stopping needs more than | its comfort brake | 55–90 % of it, if it reaches the line before red | always, if it reaches the line |
/// | lane-change incentive threshold / keep-right bias, m/s² | 0.15–0.35 / 0.2–0.4 | 0.15–0.3 / 0.15–0.3 | 0.08–0.15 / 0.05–0.15 |
///
/// Alleys: everyone drives at most 0.9 × the limit there.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DriverParams {
    /// Desired speed as a multiple of the limit.
    pub speed_factor: f32,
    /// Extra desired-speed fraction on highway lanes (−0.04 … +0.12).
    pub highway_bias: f32,
    /// IDM time headway, s (1.1–2.2).
    pub headway_s: f32,
    /// IDM standstill gap, m.
    pub min_gap_m: f32,
    /// IDM maximum acceleration, m/s².
    pub max_accel: f32,
    /// Comfortable deceleration, m/s².
    pub comfort_decel: f32,
    /// MOBIL politeness 0..1.
    pub politeness: f32,
    /// Minimum accepted gap when yielding, s.
    pub gap_accept_s: f32,
    /// Goes through yellows whenever it can reach the line before red.
    pub yellow_runner: bool,
    /// At an amber, stops if that needs at most this deceleration (m/s²); otherwise goes when
    /// it can reach the line before red. Law-abiding = `comfort_decel`.
    pub amber_decel: f32,
    /// Reaction time before pulling away from a standstill (green light, queue moving), s.
    pub start_delay_s: f32,
    /// MOBIL incentive needed for a discretionary lane change, m/s².
    pub lane_change_threshold: f32,
    /// MOBIL keep-right bias (added for moving right, subtracted for moving left), m/s².
    pub keep_right_bias: f32,
}

impl DriverParams {
    /// Sample parameters for a personality.
    pub fn sample(p: Personality, rng: &mut Rng) -> Self {
        match p {
            Personality::LawAbiding => {
                let slow = rng.next_f32() < 0.12;
                let speed_factor = if slow {
                    rng.range_f32(0.80, 0.88)
                } else {
                    rng.range_f32(0.88, 1.03)
                };
                let comfort_decel = rng.range_f32(2.0, 2.8);
                let start_delay_s = if rng.next_f32() < 0.10 {
                    rng.range_f32(1.2, 2.2)
                } else {
                    rng.range_f32(0.5, 1.0)
                };
                Self {
                    speed_factor,
                    highway_bias: rng.range_f32(-0.04, 0.05),
                    headway_s: rng.range_f32(1.6, 2.2),
                    min_gap_m: 2.0,
                    max_accel: rng.range_f32(1.1, 1.7),
                    comfort_decel,
                    politeness: rng.range_f32(0.4, 0.7),
                    gap_accept_s: 5.0,
                    yellow_runner: false,
                    amber_decel: comfort_decel,
                    start_delay_s,
                    lane_change_threshold: rng.range_f32(0.15, 0.35),
                    keep_right_bias: rng.range_f32(0.2, 0.4),
                }
            }
            Personality::Casual => {
                let speed_factor = rng.range_f32(1.03, 1.13);
                let comfort_decel = rng.range_f32(2.6, 3.3);
                let start_delay_s = if rng.next_f32() < 0.05 {
                    rng.range_f32(1.2, 2.0)
                } else {
                    rng.range_f32(0.35, 0.75)
                };
                Self {
                    speed_factor,
                    highway_bias: rng.range_f32(0.0, 0.08),
                    headway_s: rng.range_f32(1.3, 1.6),
                    min_gap_m: 1.8,
                    max_accel: rng.range_f32(1.5, 2.1),
                    comfort_decel,
                    politeness: rng.range_f32(0.2, 0.4),
                    gap_accept_s: 4.5,
                    yellow_runner: false,
                    amber_decel: comfort_decel * rng.range_f32(0.55, 0.9),
                    start_delay_s,
                    lane_change_threshold: rng.range_f32(0.15, 0.3),
                    keep_right_bias: rng.range_f32(0.15, 0.3),
                }
            }
            Personality::Aggressive => Self {
                speed_factor: rng.range_f32(1.13, 1.25),
                highway_bias: rng.range_f32(0.03, 0.12),
                headway_s: rng.range_f32(1.1, 1.3),
                min_gap_m: 1.2,
                max_accel: rng.range_f32(2.2, 2.8),
                comfort_decel: rng.range_f32(3.2, 3.8),
                politeness: rng.range_f32(0.0, 0.1),
                gap_accept_s: 3.5,
                yellow_runner: true,
                amber_decel: 0.0,
                start_delay_s: rng.range_f32(0.15, 0.4),
                lane_change_threshold: rng.range_f32(0.08, 0.15),
                keep_right_bias: rng.range_f32(0.05, 0.15),
            },
        }
    }

    /// Desired speed as a multiple of the limit on a lane of this kind (`None` = junction
    /// connector): `speed_factor`, plus `highway_bias` on highways, at most 0.9 in alleys.
    pub fn factor_on(&self, kind: Option<LaneKind>) -> f32 {
        match kind {
            Some(LaneKind::Highway) => self.speed_factor * (1.0 + self.highway_bias),
            Some(LaneKind::Alley) => self.speed_factor.min(0.9),
            _ => self.speed_factor,
        }
    }
}

/// Level of detail of an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Lod {
    /// Pose integrated along the lane path.
    Kinematic,
    /// A `wp_vehicle` instance driven by a controller.
    Physical,
}

/// A LOD transition, for the sim loop to create/destroy rigid bodies and damage models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LodChange {
    /// The agent became physical: create its rigid body / damage model.
    Promoted(AgentId),
    /// The agent became kinematic again: remove its body.
    Demoted(AgentId),
    /// A physical agent was removed from the world: remove its body.
    Despawned(AgentId),
}

/// A vehicle the world tracks for hazards, spawning, LOD and violations (players, police,
/// AI racers). Supplied by the sim loop every tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackedVehicle {
    /// Entity id.
    pub entity: EntityId,
    /// Position, m.
    pub pos: V3,
    /// Velocity, m/s.
    pub vel: V3,
    /// Unit forward direction (XY).
    pub forward: V2,
    /// Siren on.
    pub siren: bool,
    /// Human player (drives the spawn ring, sightline rule and physical LOD).
    pub player: bool,
}

impl TrackedVehicle {
    /// Build from a vehicle state (forward taken from the orientation quaternion).
    pub fn from_state(
        entity: EntityId,
        st: &wp_contracts::VehicleState,
        siren: bool,
        player: bool,
    ) -> Self {
        Self {
            entity,
            pos: st.pos,
            vel: st.lin_vel,
            forward: forward_of(st.rot),
            siren,
            player,
        }
    }
}

/// Debris or another obstacle on the road (detached parts, wheels, knocked props). The sim
/// loop passes every loose body each tick with [`crate::World::set_hazards`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hazard {
    /// Position, m.
    pub pos: V3,
    /// Radius of the obstacle's footprint, m.
    pub radius_m: f32,
    /// Velocity (rolling wheels, sliding panels), m/s; drivers extrapolate up to 2 s.
    pub vel: V3,
}

impl Hazard {
    /// A resting obstacle.
    pub fn new(pos: V3, radius_m: f32) -> Self {
        Self {
            pos,
            radius_m,
            vel: [0.0; 3],
        }
    }

    /// Where the obstacle will be after `t` seconds (extrapolation capped at 2 s).
    pub fn pos_after(&self, t: f32) -> V2 {
        let t = t.clamp(0.0, 2.0);
        [self.pos[0] + self.vel[0] * t, self.pos[1] + self.vel[1] * t]
    }
}

/// How a hazard ahead affects a car keeping a lane-relative lateral offset.
#[derive(Debug, Clone, Copy, PartialEq)]
enum HazardEffect {
    /// Pass at this lane-relative lateral offset (+ right), m.
    Swerve(f32),
    /// Cannot pass in this lane: stop `gap` metres before it.
    Blocked(f32),
}

/// Largest sideways shift within the lane drivers make for debris, m.
const MAX_SWERVE_M: f32 = 1.5;

/// Effect of hazard `h` on a car at `q` facing `f` (lane centre at lateral offset `-lat` from
/// the car) travelling at `v`; `None` if it is not in the way within 40 m.
fn hazard_effect(h: &Hazard, q: V2, f: V2, lat: f32, v: f32) -> Option<HazardEffect> {
    let r0 = geom::sub(geom::xy(h.pos), q);
    let t = geom::dot(r0, f).max(0.0) / v.max(1.0);
    let r = geom::sub(h.pos_after(t), q);
    let long = geom::dot(r, f);
    // Keep reacting until the rear bumper is past it.
    if !(-(HALF_L + h.radius_m + 0.3)..40.0).contains(&long) {
        return None;
    }
    // Hazard offset from the lane centre, and the clearance a car body needs.
    let h_lat = geom::dot(r, geom::right_of(f)) + lat;
    let clear = h.radius_m + CAR_WIDTH_M * 0.5 + 0.3;
    if h_lat.abs() >= clear {
        return None;
    }
    // Pass on whichever side needs the smaller shift.
    let left = h_lat - clear;
    let right = h_lat + clear;
    let shift = if left.abs() <= right.abs() {
        left
    } else {
        right
    };
    if shift.abs() <= MAX_SWERVE_M {
        Some(HazardEffect::Swerve(shift))
    } else if long > 0.0 {
        Some(HazardEffect::Blocked(long - h.radius_m - HALF_L))
    } else {
        None // already alongside: braking cannot help
    }
}

/// Traffic tuning.
#[derive(Debug, Clone, PartialEq)]
pub struct TrafficConfig {
    /// Spawn and AI enabled.
    pub enabled: bool,
    /// Agent cap (default 400).
    pub max_agents: usize,
    /// Physical (near-LOD) cap.
    pub max_physical: usize,
    /// Radius around players / route focus points that is populated, m.
    pub active_radius_m: f32,
    /// Agents never spawn or despawn within this distance of a player (now or 1 s ahead), m.
    pub sight_radius_m: f32,
    /// Agents within this distance of a player or race route become physical, m.
    pub physical_radius_m: f32,
    /// Fraction of casual drivers.
    pub casual_fraction: f32,
    /// Fraction of aggressive drivers.
    pub aggressive_fraction: f32,
    /// Density falloff radius from the core, m.
    pub density_falloff_m: f32,
    /// Global density multiplier.
    pub density_scale: f32,
    /// Track civilian–civilian overlaps (diagnostics; costs ~10 % of traffic time).
    pub detect_collisions: bool,
    /// Agents stopped longer than this (and unseen) are recycled, s.
    pub stuck_timeout_s: f32,
}

impl Default for TrafficConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_agents: 400,
            max_physical: 16,
            active_radius_m: 400.0,
            sight_radius_m: 150.0,
            physical_radius_m: 80.0,
            casual_fraction: 0.25,
            aggressive_fraction: 0.05,
            density_falloff_m: 600.0,
            density_scale: 1.0,
            detect_collisions: false,
            stuck_timeout_s: 120.0,
        }
    }
}

/// Running counters.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TrafficStats {
    /// Agents spawned.
    pub spawned: u64,
    /// Agents despawned (density, area, map exit).
    pub despawned: u64,
    /// Agents recycled because they were stuck.
    pub stuck_recycled: u64,
    /// New civilian–civilian overlap pairs (only with `detect_collisions`).
    pub civilian_collisions: u64,
    /// Of those, pairs involving an aggressive driver.
    pub collisions_involving_aggressive: u64,
    /// Promotions to physical.
    pub promotions: u64,
    /// Demotions to kinematic.
    pub demotions: u64,
    /// Lane changes performed.
    pub lane_changes: u64,
    /// Of those, changes to the left (overtaking; the rest move right / keep right).
    pub lane_changes_left: u64,
    /// Emergency brake reactions.
    pub emergency_brakes: u64,
    /// First 64 new overlap pairs: `(traffic step, agent a, agent b)`.
    pub collision_log: Vec<(u64, AgentId, AgentId)>,
}

/// Near-LOD state.
#[derive(Debug, Clone)]
pub struct PhysicalAgent {
    /// The vehicle simulation (the sim loop attaches a rigid body / damage model to it).
    pub vehicle: Vehicle,
    /// Controller output applied every physics tick.
    pub input: VehicleInput,
    /// Shared pure-pursuit + PID controller ([`crate::drive`]).
    pub controller: DriveController,
}

/// One civilian car.
#[derive(Debug, Clone)]
pub struct Agent {
    /// Stable id.
    pub id: AgentId,
    /// Entity id assigned by the sim loop (for physical agents), if any.
    pub entity: Option<EntityId>,
    /// Personality.
    pub personality: Personality,
    /// Sampled parameters.
    pub params: DriverParams,
    /// Current segment.
    pub seg: SegId,
    /// Arc length of the car centre along `seg`, m.
    pub s: f32,
    /// Speed, m/s.
    pub v: f32,
    /// Last commanded acceleration, m/s².
    pub acc: f32,
    /// Lateral offset from the path (+ right), m.
    pub lat: f32,
    lat_target: f32,
    /// Next two segments.
    pub plan: [SegId; 2],
    permit: SegId,
    /// Traffic step at which the agent came to a stop.
    stopped_since: Option<u64>,
    last_lane_change: u64,
    /// Discretionary lane change being indicated: (to the right, traffic step it started).
    pending_lc: Option<(bool, u64)>,
    /// Time spent wanting to pull away from a standstill (reaction delay), s.
    start_wait_s: f32,
    ghost: Option<(SegId, f32)>,
    stuck_s: f32,
    /// Currently braking hard for a hazard.
    pub emergency: bool,
    /// Currently yielding to a siren.
    pub yielding: bool,
    /// Disabled (wrecked or lost control): stopped with hazard lights.
    pub disabled: bool,
    /// Visual variant for the renderer.
    pub variant: u8,
    /// LOD.
    pub lod: Lod,
    /// Near-LOD vehicle.
    pub physical: Option<Box<PhysicalAgent>>,
    despawn: bool,
}

/// Renderer / netcode view of an agent.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AgentView {
    /// Agent id.
    pub id: AgentId,
    /// Position, m.
    pub pos: V3,
    /// Yaw about +Z (0 = +Y), rad.
    pub yaw: f32,
    /// Speed, m/s.
    pub speed_mps: f32,
    /// LOD.
    pub lod: Lod,
    /// Personality.
    pub personality: Personality,
    /// Brake lights on.
    pub brake_lights: bool,
    /// Hazard lights on.
    pub hazard_lights: bool,
    /// Pulling over for a siren.
    pub yielding: bool,
    /// Turn signal: -1 left, 0 off, 1 right.
    pub turn_signal: i8,
    /// Visual variant.
    pub variant: u8,
}

impl From<&AgentView> for wp_contracts::TrafficAgentSnapshot {
    /// Contract snapshot of a kinematic agent (v0.3.0, world #6).
    fn from(a: &AgentView) -> Self {
        use wp_contracts::traffic_flags as f;
        let mut flags = 0u8;
        if a.brake_lights {
            flags |= f::BRAKE;
        }
        if a.hazard_lights {
            flags |= f::HAZARDS;
        }
        if a.turn_signal < 0 {
            flags |= f::LEFT_SIGNAL;
        }
        if a.turn_signal > 0 {
            flags |= f::RIGHT_SIGNAL;
        }
        if a.yielding {
            flags |= f::YIELDING;
        }
        wp_contracts::TrafficAgentSnapshot {
            id: a.id,
            pos: a.pos,
            yaw: a.yaw,
            speed_mps: a.speed_mps,
            flags,
            variant: a.variant,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    s: f32,
    v: f32,
    agent: u32,
    ghost: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct Pose {
    p: V3,
    f: V2,
    vel: V2,
}

#[derive(Debug, Clone, Copy)]
struct Sample {
    seg: SegId,
    s: f32,
    pos: V2,
    weight_km: f32,
}

/// Inputs from the sim loop for one traffic update.
#[derive(Debug, Clone, Copy)]
pub struct TrafficInputs<'a> {
    /// Tracked vehicles.
    pub tracked: &'a [TrackedVehicle],
    /// Debris.
    pub hazards: &'a [Hazard],
    /// Race-route focus points (sampled along active routes), m.
    pub route_focus: &'a [V2],
    /// Closed lanes (indexed by lane id).
    pub closed: &'a [bool],
}

/// The civilian traffic system.
#[derive(Debug, Clone)]
pub struct Traffic {
    /// Configuration.
    pub config: TrafficConfig,
    agents: Vec<Agent>,
    next_id: u32,
    rng: Rng,
    /// Traffic steps run (the traffic clock; one step = `TRAFFIC_DT`).
    step_count: u64,
    lists: Vec<Vec<Entry>>,
    touched: Vec<SegId>,
    list_pos: Vec<u32>,
    poses: Vec<Pose>,
    reservations: Vec<u32>,
    tv_block: Vec<bool>,
    tv_block_list: Vec<SegId>,
    zone_samples: Vec<Vec<Sample>>,
    zone_bbox: Vec<(V2, V2)>,
    spawn_timer: f32,
    stats: TrafficStats,
    lod_changes: Vec<LodChange>,
    overlaps: BTreeSet<(u32, u32)>,
    car: Option<(VehicleDef, TireDb)>,
}

/// Desired-speed factor of driver `p` on segment `seg` ([`DriverParams::factor_on`]).
fn speed_factor_on(map: &CityMap, seg: SegId, p: &DriverParams) -> f32 {
    p.factor_on(map.seg(seg).lane().map(|l| map.net.lanes[l as usize].kind))
}

/// IDM acceleration with an IIDM-style free term above the desired speed (the shared law in
/// [`crate::leader::CarFollowing::idm`], so police / racers follow cars the same way).
fn idm(v: f32, v0: f32, gap: f32, v_lead: f32, p: &DriverParams) -> f32 {
    crate::leader::CarFollowing {
        headway_s: p.headway_s,
        min_gap_m: p.min_gap_m,
        max_accel: p.max_accel,
        comfort_decel: p.comfort_decel,
    }
    .idm(v, v0, gap, v_lead)
}

/// Unit forward (XY) of a chassis quaternion `[x, y, z, w]` (chassis +Y rotated).
pub use crate::drive::forward_of;

impl Traffic {
    /// New traffic system for a map.
    pub fn new(map: &CityMap, config: TrafficConfig, seed: u64) -> Self {
        let nseg = map.segs.len();
        let mut zone_samples: Vec<Vec<Sample>> = vec![Vec::new(); map.net.zones.len()];
        for lane in 0..map.n_lanes() {
            let s = &map.segs[lane];
            if !s.drivable {
                continue;
            }
            let n = (s.length / 10.0).ceil().max(1.0) as usize;
            let step = s.length / n as f32;
            for k in 0..n {
                let sa = (k as f32 + 0.5) * step;
                let (p, _) = s.point_at(sa);
                let q = geom::xy(p);
                if let Some(z) = map.zone_at(q) {
                    let d = geom::dist(q, map.net.core);
                    zone_samples[z as usize].push(Sample {
                        seg: lane as SegId,
                        s: sa,
                        pos: q,
                        weight_km: step / 1000.0 * f_distance(d, config.density_falloff_m),
                    });
                }
            }
        }
        let zone_bbox = map
            .net
            .zones
            .iter()
            .map(|z| {
                let mut lo = [f32::INFINITY; 2];
                let mut hi = [f32::NEG_INFINITY; 2];
                for p in &z.polygon {
                    lo = [lo[0].min(p[0]), lo[1].min(p[1])];
                    hi = [hi[0].max(p[0]), hi[1].max(p[1])];
                }
                (lo, hi)
            })
            .collect();
        Self {
            config,
            agents: Vec::new(),
            next_id: 1,
            rng: Rng::new(seed ^ 0x7AFF_1C00_5EED),
            step_count: 0,
            lists: vec![Vec::new(); nseg],
            touched: Vec::new(),
            list_pos: Vec::new(),
            poses: Vec::new(),
            reservations: vec![NONE; nseg],
            tv_block: vec![false; nseg],
            tv_block_list: Vec::new(),
            zone_samples,
            zone_bbox,
            spawn_timer: 0.0,
            stats: TrafficStats::default(),
            lod_changes: Vec::new(),
            overlaps: BTreeSet::new(),
            car: None,
        }
    }

    /// Vehicle definition used for physical (near-LOD) agents. Without one, agents stay
    /// kinematic.
    pub fn set_traffic_car(&mut self, def: VehicleDef, tires: TireDb) {
        self.car = Some((def, tires));
    }

    /// All agents, ordered by id.
    pub fn agents(&self) -> &[Agent] {
        &self.agents
    }

    /// Mutable access to physical agents' vehicles (the sim loop attaches bodies/damage).
    pub fn physical_mut(&mut self) -> impl Iterator<Item = (AgentId, &mut PhysicalAgent)> {
        self.agents
            .iter_mut()
            .filter_map(|a| a.physical.as_deref_mut().map(|p| (a.id, p)))
    }

    /// Traffic steps run so far (one step = [`TRAFFIC_DT`]).
    pub fn step_count(&self) -> u64 {
        self.step_count
    }

    /// Counters.
    pub fn stats(&self) -> &TrafficStats {
        &self.stats
    }

    /// Take the LOD transitions since the last call.
    pub fn drain_lod_changes(&mut self) -> Vec<LodChange> {
        std::mem::take(&mut self.lod_changes)
    }

    /// Assign the sim-loop entity id of an agent.
    pub fn set_entity(&mut self, agent: AgentId, entity: Option<EntityId>) {
        if let Some(a) = self.agents.iter_mut().find(|a| a.id == agent) {
            a.entity = entity;
        }
    }

    /// Apply damage effects to a physical agent; a dead engine or lost wheel disables it.
    pub fn apply_damage(&mut self, agent: AgentId, eff: &HandlingEffects) {
        if let Some(a) = self.agents.iter_mut().find(|a| a.id == agent) {
            if let Some(pa) = a.physical.as_deref_mut() {
                pa.vehicle.apply_damage_effects(eff);
            }
            if eff.engine_dead || eff.wheel_lost.iter().any(|w| *w) {
                a.disabled = true;
            }
        }
    }

    /// Renderer view of every agent.
    pub fn views<'a>(&'a self, map: &'a CityMap) -> impl Iterator<Item = AgentView> + 'a {
        self.agents.iter().map(move |a| {
            let (p, f) = self.pose_of(map, a);
            let mut turn_signal = 0i8;
            if a.plan[0] != NONE && !map.seg(a.seg).is_connector() {
                if let SegKind::Connector { turn, .. } = map.seg(a.plan[0]).kind {
                    if map.seg(a.seg).length - a.s < 40.0 {
                        turn_signal = match turn {
                            TurnKind::Left | TurnKind::UTurn => -1,
                            TurnKind::Right => 1,
                            TurnKind::Straight => 0,
                        };
                    }
                }
            }
            if a.ghost.is_some() {
                turn_signal = if a.lat < 0.0 { 1 } else { -1 };
            } else if let Some((to_right, _)) = a.pending_lc {
                turn_signal = if to_right { 1 } else { -1 };
            }
            AgentView {
                id: a.id,
                pos: p,
                yaw: geom::yaw_of(f),
                speed_mps: a.v,
                lod: a.lod,
                personality: a.personality,
                brake_lights: a.acc < -0.4 || (a.v < 0.1 && !a.disabled),
                hazard_lights: a.disabled || a.emergency,
                yielding: a.yielding,
                turn_signal,
                variant: a.variant,
            }
        })
    }

    fn pose_of(&self, map: &CityMap, a: &Agent) -> (V3, V2) {
        if let Some(pa) = &a.physical {
            let st = pa.vehicle.state();
            return (st.pos, forward_of(st.rot));
        }
        let (p, t) = map.seg(a.seg).point_at(a.s);
        let r = geom::right_of(t);
        ([p[0] + r[0] * a.lat, p[1] + r[1] * a.lat, p[2]], t)
    }

    /// Oriented box of an agent (for overlap tests).
    pub fn agent_obb(&self, map: &CityMap, a: &Agent) -> (Obb, f32) {
        let (p, f) = self.pose_of(map, a);
        (
            Obb {
                c: geom::xy(p),
                f,
                hl: HALF_L,
                hw: CAR_WIDTH_M * 0.5,
            },
            p[2],
        )
    }

    /// Deterministic hash of the traffic state.
    pub fn hash_into(&self, h: &mut StateHasher) {
        h.write_u32(self.next_id);
        h.write_u64(self.agents.len() as u64);
        for a in &self.agents {
            h.write_u32(a.id);
            h.write_u32(a.seg);
            h.write_f32_q(a.s, 1e3);
            h.write_f32_q(a.v, 1e3);
            h.write_f32_q(a.lat, 1e3);
            h.write_u32(a.plan[0]);
            h.write_u32(a.plan[1]);
            h.write_u32(a.permit);
            h.write_u32(a.personality as u32);
            h.write_u32(a.lod as u32);
        }
        h.write_u64(self.stats.spawned);
        h.write_u64(self.stats.despawned);
    }

    // ------------------------------------------------------------------------------------
    // Main update
    // ------------------------------------------------------------------------------------

    /// Advance the traffic AI by [`TRAFFIC_DT`].
    pub fn step(&mut self, map: &CityMap, inp: &TrafficInputs, _out: &mut Vec<SimEvent>) {
        if !self.config.enabled || map.segs.is_empty() {
            return;
        }
        let dt = TRAFFIC_DT;
        self.step_count += 1;
        self.sync_physical(map, inp);
        self.rebuild_lists();
        self.compute_poses(map);
        self.compute_tv_blocks(map, inp.tracked, inp.hazards);
        self.decide(map, inp);
        self.lane_changes(map, inp);
        self.drive_physical(map);
        self.integrate(map, inp, dt);
        self.spawn_timer += dt;
        if self.spawn_timer >= SPAWN_PERIOD_S {
            self.spawn_timer -= SPAWN_PERIOD_S;
            self.compute_poses(map);
            self.update_lod(map, inp);
            self.spawn_update(map, inp, 12);
        }
        if self.config.detect_collisions {
            self.compute_poses(map);
            self.detect_collisions();
        }
        self.remove_despawned();
    }

    fn remove_despawned(&mut self) {
        for a in &self.agents {
            if !a.despawn {
                continue;
            }
            for r in [a.permit, a.seg] {
                if r != NONE && self.reservations[r as usize] == a.id {
                    self.reservations[r as usize] = NONE;
                }
            }
            if a.physical.is_some() {
                self.lod_changes.push(LodChange::Despawned(a.id));
            }
        }
        self.agents.retain(|a| !a.despawn);
    }

    /// Step physical agents' vehicles by one physics tick.
    pub fn step_physics(&mut self, map: &CityMap, dt: f32, out: &mut Vec<SimEvent>) {
        let assists = AssistProfile::ai();
        for a in &mut self.agents {
            // Agents with an entity have a rigid body in wp_sim, which steps them (lead, D #3).
            if a.entity.is_some() {
                continue;
            }
            if let Some(pa) = a.physical.as_deref_mut() {
                // Roads and terrain (city v2) for agents the sim loop has not embodied; on a
                // flat city this is exactly the old z = 0 plane.
                let ground = crate::surface::MapGround { map };
                pa.vehicle.step(&pa.input, &assists, map, &ground, dt, out);
            }
        }
    }

    /// Fill the map up to the density targets immediately (warm start, no visibility rule for
    /// agents outside the sight radius).
    pub fn populate(&mut self, map: &CityMap, inp: &TrafficInputs) {
        if !self.config.enabled {
            return;
        }
        for _ in 0..40 {
            self.rebuild_lists();
            self.compute_poses(map);
            let before = self.agents.len();
            self.spawn_update(map, inp, 200);
            self.remove_despawned();
            if self.agents.len() == before {
                break;
            }
        }
    }

    fn rebuild_lists(&mut self) {
        for &s in &self.touched {
            self.lists[s as usize].clear();
        }
        self.touched.clear();
        for (i, a) in self.agents.iter().enumerate() {
            let l = &mut self.lists[a.seg as usize];
            if l.is_empty() {
                self.touched.push(a.seg);
            }
            l.push(Entry {
                s: a.s,
                v: a.v,
                agent: i as u32,
                ghost: false,
            });
            if let Some((gs, gss)) = a.ghost {
                let l = &mut self.lists[gs as usize];
                if l.is_empty() {
                    self.touched.push(gs);
                }
                l.push(Entry {
                    s: gss,
                    v: a.v,
                    agent: i as u32,
                    ghost: true,
                });
            }
        }
        self.list_pos.clear();
        self.list_pos.resize(self.agents.len(), 0);
        self.touched.sort_unstable();
        for &s in &self.touched {
            let l = &mut self.lists[s as usize];
            l.sort_by(|x, y| x.s.total_cmp(&y.s).then(x.agent.cmp(&y.agent)));
            for (k, e) in l.iter().enumerate() {
                if !e.ghost {
                    self.list_pos[e.agent as usize] = k as u32;
                }
            }
        }
    }

    fn compute_poses(&mut self, map: &CityMap) {
        self.poses.clear();
        for a in &self.agents {
            let (p, f) = self.pose_of(map, a);
            self.poses.push(Pose {
                p,
                f,
                vel: geom::scale(f, a.v),
            });
        }
    }

    /// Mark junction connectors occupied by tracked vehicles or debris (nobody may enter them).
    fn compute_tv_blocks(&mut self, map: &CityMap, tracked: &[TrackedVehicle], hazards: &[Hazard]) {
        for &s in &self.tv_block_list {
            self.tv_block[s as usize] = false;
        }
        self.tv_block_list.clear();
        let obstacles = tracked.iter().map(|t| (geom::xy(t.pos), 3.0)).chain(
            hazards
                .iter()
                .map(|h| (geom::xy(h.pos), h.radius_m + CAR_WIDTH_M * 0.5 + 0.2)),
        );
        for (q, reach) in obstacles {
            if map.intersection_at(q).is_none() {
                continue;
            }
            for &sid in map.segs_near(q) {
                let sg = map.seg(sid);
                if !sg.is_connector() {
                    continue;
                }
                if sg.project(q).dist < reach && !self.tv_block[sid as usize] {
                    self.tv_block[sid as usize] = true;
                    self.tv_block_list.push(sid);
                }
            }
        }
    }

    fn sync_physical(&mut self, map: &CityMap, inp: &TrafficInputs) {
        for i in 0..self.agents.len() {
            let Some(pa) = &self.agents[i].physical else {
                continue;
            };
            let st = pa.vehicle.state();
            let q = geom::xy(st.pos);
            let fwd = forward_of(st.rot);
            self.agents[i].v = st.speed_mps.max(0.0);
            for _ in 0..3 {
                let a = &self.agents[i];
                let sg = map.seg(a.seg);
                let pr = sg.project(q);
                if pr.s >= sg.length && a.plan[0] != NONE {
                    let rem = pr.s - sg.length;
                    self.agents[i].s = sg.length;
                    self.enter_next(map, i, rem, inp.closed);
                    continue;
                }
                let a = &mut self.agents[i];
                a.s = pr.s.clamp(0.0, sg.length);
                a.lat = pr.lateral;
                let (_, t) = sg.point_at(a.s);
                if pr.dist > 6.0 || geom::dot(fwd, t) < 0.2 {
                    a.disabled = true;
                }
                break;
            }
        }
    }

    // ------------------------------------------------------------------------------------
    // Decisions
    // ------------------------------------------------------------------------------------

    fn is_front(&self, i: usize) -> bool {
        let a = &self.agents[i];
        let l = &self.lists[a.seg as usize];
        l[self.list_pos[i] as usize + 1..].iter().all(|e| e.ghost)
    }

    fn leader(&self, map: &CityMap, i: usize) -> (f32, f32) {
        let a = &self.agents[i];
        let seg = map.seg(a.seg);
        let l = &self.lists[a.seg as usize];
        let mut best: Option<(f32, f32)> = None;
        for e in &l[self.list_pos[i] as usize + 1..] {
            if e.agent as usize != i {
                best = Some((e.s - a.s - CAR_LENGTH_M, e.v));
                break;
            }
        }
        // Diverging connectors share their first metres: a car on a sibling whose rear is
        // still before the divergence point is in the way.
        if seg.is_connector() {
            for &(sib, div) in &seg.siblings {
                for e in &self.lists[sib as usize] {
                    if e.s > a.s && e.s - HALF_L < div {
                        let g = e.s - a.s - CAR_LENGTH_M;
                        if best.is_none_or(|b| g < b.0) {
                            best = Some((g, e.v));
                        }
                    }
                }
            }
        }
        if let Some(b) = best {
            return b;
        }
        let mut dist = seg.length - a.s;
        for (k, &nx) in a.plan.iter().enumerate() {
            if nx == NONE || dist > 150.0 {
                break;
            }
            let ns = map.seg(nx);
            let mut cand: Option<(f32, f32)> = self.lists[nx as usize]
                .iter()
                .find(|e| e.agent as usize != i)
                .map(|e| (e.s, e.v));
            if k == 0 && ns.is_connector() {
                for &(sib, div) in &ns.siblings {
                    if let Some(e) = self.lists[sib as usize].first() {
                        if e.s - HALF_L < div && cand.is_none_or(|c| e.s < c.0) {
                            cand = Some((e.s, e.v));
                        }
                    }
                }
            }
            if let Some((s, v)) = cand {
                return (dist + s - CAR_LENGTH_M, v);
            }
            dist += ns.length;
        }
        (f32::INFINITY, 0.0)
    }

    fn rank_of(&self, map: &CityMap, conn: SegId) -> u8 {
        let SegKind::Connector { from, turn, .. } = map.seg(conn).kind else {
            return 1;
        };
        match map.lane_control(from) {
            None => 1,
            Some((IntersectionControl::Signal { .. }, _, _)) => {
                match map.lane_signal(from).map(|s| s.0) {
                    Some(SignalState::Green) | Some(SignalState::Yellow) => {
                        if turn == TurnKind::Left {
                            1
                        } else {
                            2
                        }
                    }
                    Some(SignalState::Red) => 0,
                    _ => 1,
                }
            }
            Some((IntersectionControl::Priority, ap, _)) => {
                if ap.major {
                    2
                } else {
                    0
                }
            }
            Some(_) => 1,
        }
    }

    /// Earliest time an agent planning to use `conn` reaches its start (upstream search).
    fn arrival_time(&self, map: &CityMap, conn: SegId) -> f32 {
        let SegKind::Connector { from, .. } = map.seg(conn).kind else {
            return f32::INFINITY;
        };
        let mut best = f32::INFINITY;
        let fseg = map.seg(from);
        // On yellow/red, agents without a permit that can stop comfortably will stop.
        let stopping = matches!(
            map.lane_signal(from).map(|s| s.0),
            Some(SignalState::Yellow) | Some(SignalState::Red)
        );
        for e in &self.lists[from as usize] {
            let a = &self.agents[e.agent as usize];
            if e.ghost || a.plan[0] != conn {
                continue;
            }
            if stopping && a.permit != conn {
                let d = (fseg.length - a.s - HALF_L - 0.5).max(0.1);
                if a.v * a.v / (2.0 * d) <= a.params.comfort_decel {
                    continue;
                }
            }
            let d = (fseg.length - a.s - HALF_L).max(0.0);
            best = best.min(d / a.v.max(1.0));
        }
        let base = fseg.length;
        if stopping {
            return best;
        }
        for &p1 in &fseg.prev {
            let s1 = map.seg(p1);
            for e in &self.lists[p1 as usize] {
                let a = &self.agents[e.agent as usize];
                if a.plan[0] == from && a.plan[1] == conn {
                    let d = s1.length - a.s + base;
                    best = best.min(d / a.v.max(1.0));
                }
            }
            if base + s1.length < 150.0 {
                for &l0 in &s1.prev {
                    let s0 = map.seg(l0);
                    for e in &self.lists[l0 as usize] {
                        let a = &self.agents[e.agent as usize];
                        if !e.ghost && a.plan[0] == p1 && a.plan[1] == from {
                            let d = s0.length - a.s + s1.length + base;
                            best = best.min(d / a.v.max(1.0));
                        }
                    }
                }
            }
        }
        best
    }

    fn stopped_for(&self, i: usize) -> f32 {
        self.agents[i]
            .stopped_since
            .map(|t| steps_to_s(self.step_count - t))
            .unwrap_or(0.0)
    }

    fn allway_first(&self, map: &CityMap, i: usize, conn: SegId) -> bool {
        let a = &self.agents[i];
        let lane = a.seg;
        let Some(ap) = map.approach[lane as usize] else {
            return true;
        };
        let my_t = a.stopped_since.unwrap_or(self.step_count);
        let conflicts = &map.seg(conn).conflicts;
        for inc in &map.net.intersections[ap.intersection as usize].incoming {
            if inc.lane == lane {
                continue;
            }
            let Some(e) = self.lists[inc.lane as usize]
                .iter()
                .rev()
                .find(|e| !e.ghost)
            else {
                continue;
            };
            let f = &self.agents[e.agent as usize];
            let d_line = map.seg(inc.lane).length - f.s - HALF_L;
            let Some(ft) = f.stopped_since else { continue };
            if d_line < 4.0
                && (ft < my_t || (ft == my_t && f.id < a.id))
                && conflicts.contains(&f.plan[0])
            {
                return false;
            }
        }
        true
    }

    /// Whether agent `i` may enter `conn` now (control rule, occupancy, space, gaps).
    fn may_enter(&self, map: &CityMap, i: usize, conn: SegId, d_line: f32, siren: bool) -> bool {
        let a = &self.agents[i];
        let p = &a.params;
        let v = a.v;
        if siren || self.tv_block[conn as usize] || !self.is_front(i) {
            return false;
        }
        let request = (v * v / (2.0 * 2.5) + 10.0).max(v * 2.5).max(6.0);
        if d_line > request {
            return false;
        }
        let SegKind::Connector { turn, .. } = map.seg(conn).kind else {
            return true;
        };
        let need = v * v / (2.0 * d_line.max(0.1));
        let at_line = d_line < 3.5;
        let my_rank: u8 = match map.lane_control(a.seg) {
            None => 1,
            Some((IntersectionControl::Signal { .. }, _, rtor)) => {
                let (st, rem) = map.lane_signal(a.seg).unwrap_or((SignalState::Off, 0.0));
                match st {
                    SignalState::Green => {
                        if turn == TurnKind::Left {
                            1
                        } else {
                            2
                        }
                    }
                    SignalState::Yellow
                        if turn == TurnKind::Left && at_line && self.stopped_for(i) >= 1.0 =>
                    {
                        // "Sneaker": a left-turner waiting at the line goes once oncoming
                        // traffic is stopping (still yields to anyone committed).
                        1
                    }
                    SignalState::Yellow => {
                        let go = if p.yellow_runner {
                            d_line / v.max(0.1) < rem + 1.0
                        } else {
                            // Can't stop comfortably: go. Casual drivers also go when stopping
                            // would merely be firm, if they reach the line before red.
                            need > p.comfort_decel
                                || (need > p.amber_decel && d_line / v.max(0.1) < rem + 0.5)
                        };
                        if !go {
                            return false;
                        }
                        2
                    }
                    SignalState::Red => {
                        if need > 7.0 && v > 3.0 {
                            2
                        } else if turn == TurnKind::Right
                            && rtor
                            && at_line
                            && self.stopped_for(i) >= 1.0
                        {
                            0
                        } else {
                            return false;
                        }
                    }
                    SignalState::Off => {
                        if !(at_line && self.stopped_for(i) >= 0.5)
                            || !self.allway_first(map, i, conn)
                        {
                            return false;
                        }
                        1
                    }
                }
            }
            Some((IntersectionControl::AllWayStop, _, _)) => {
                if !(at_line && self.stopped_for(i) >= 0.5) || !self.allway_first(map, i, conn) {
                    return false;
                }
                1
            }
            Some((IntersectionControl::Priority, ap, _)) => {
                if ap.major {
                    2
                } else {
                    // A freeway on-ramp (a minor highway lane) merges on the move: it yields
                    // to the main line below but needs no stop first.
                    let merging = map.net.lanes[a.seg as usize].kind == LaneKind::Highway;
                    if !merging && !(at_line && self.stopped_for(i) >= 0.3) {
                        return false;
                    }
                    0
                }
            }
            Some((IntersectionControl::Uncontrolled, _, _)) => 1,
        };
        let cs = map.seg(conn);
        for &c in &cs.conflicts {
            let r = self.reservations[c as usize];
            if !self.lists[c as usize].is_empty()
                || (r != NONE && r != a.id)
                || self.tv_block[c as usize]
            {
                return false;
            }
        }
        // Don't block the box: room on the target lane.
        let target = cs.next[0];
        let on_conn = self.lists[conn as usize].len() as f32;
        if let Some(e) = self.lists[target as usize].first() {
            let space = e.s - HALF_L;
            if e.v < 3.0 && space < (on_conn + 1.0) * (CAR_LENGTH_M + 2.0) {
                return false;
            }
        }
        // Yield to higher-ranked conflicting movements.
        for &c in &cs.conflicts {
            if self.rank_of(map, c) > my_rank && self.arrival_time(map, c) < p.gap_accept_s {
                return false;
            }
        }
        true
    }

    fn decide(&mut self, map: &CityMap, inp: &TrafficInputs) {
        for i in 0..self.agents.len() {
            let pose = self.poses[i];
            let a = &self.agents[i];
            let p = a.params;
            let seg = map.seg(a.seg);
            let v = a.v;
            let right = geom::right_of(pose.f);
            let q = geom::xy(pose.p);

            if a.disabled {
                let a = &mut self.agents[i];
                a.acc = -6.0;
                a.lat_target = a.lat;
                continue;
            }

            // Desired speed.
            let sf = speed_factor_on(map, a.seg, &p);
            let mut v0 = (seg.speed_limit * sf).min(seg.curve_speed * sf);
            let d_end = seg.length - a.s;
            if a.plan[0] != NONE {
                let ns = map.seg(a.plan[0]);
                let vc = ns.curve_speed.min(ns.speed_limit) * speed_factor_on(map, a.plan[0], &p);
                v0 = v0.min((vc * vc + 2.0 * p.comfort_decel * (d_end - HALF_L).max(0.0)).sqrt());
            }
            let mut lat_target = 0.0f32;
            let mut acc = f32::INFINITY;
            let mut emergency = false;

            // Sirens.
            let mut siren = false;
            for t in inp.tracked.iter().filter(|t| t.siren) {
                let r = geom::sub(geom::xy(t.pos), q);
                let d = geom::len(r);
                if d < SIREN_RADIUS_M {
                    siren = true;
                    v0 = v0.min(3.0);
                    // Slow down promptly (≈ 4 m/s²) rather than coasting down comfortably.
                    acc = acc.min(-((v - 3.0) * 1.5).clamp(0.0, 4.0));
                    if !seg.is_connector() {
                        lat_target = 1.0;
                    }
                    if geom::dot(r, pose.f) < 0.0 && d < 25.0 {
                        acc = acc.min(idm(v, v0, 1.0 + p.min_gap_m, 0.0, &p));
                    }
                }
            }

            // Car ahead.
            let (gap, vl) = self.leader(map, i);
            acc = acc.min(idm(v, v0, gap, vl, &p));
            // Mid lane change the car still occupies its old lane: respect that lane's leader.
            if let Some((gseg, gs)) = a.ghost {
                let lead = self.lists[gseg as usize]
                    .iter()
                    .find(|e| e.s > gs && e.agent as usize != i);
                if let Some(e) = lead {
                    acc = acc.min(idm(v, v0, e.s - gs - CAR_LENGTH_M, e.v, &p));
                }
            }

            // Tracked vehicles in the path, and time-to-collision reactions.
            for t in inp.tracked {
                let r = geom::sub(geom::xy(t.pos), q);
                if (t.pos[2] - pose.p[2]).abs() > 4.0 || geom::dot(r, r) > 80.0 * 80.0 {
                    continue;
                }
                let long = geom::dot(r, pose.f);
                let latd = geom::dot(r, right);
                let tv = [t.vel[0], t.vel[1]];
                if long > 0.0 && long < 80.0 && latd.abs() < CAR_WIDTH_M + 0.4 {
                    acc = acc.min(idm(v, v0, long - CAR_LENGTH_M, geom::dot(tv, pose.f), &p));
                }
                let w = geom::sub(tv, pose.vel);
                let ww = geom::dot(w, w);
                if ww > 25.0 && long > -2.0 {
                    let ts = -geom::dot(r, w) / ww;
                    if ts > 0.0 && ts < 3.0 {
                        let miss = geom::len(geom::add(r, geom::scale(w, ts)));
                        if miss < 3.0 {
                            emergency = true;
                        }
                    }
                }
            }

            // Debris: swerve within the lane (planned in the lane frame so it holds the
            // offset while passing), or stop if it blocks the lane.
            for h in inp.hazards {
                match hazard_effect(h, q, pose.f, a.lat, v) {
                    Some(HazardEffect::Swerve(shift)) if !seg.is_connector() => {
                        if lat_target == 0.0 || shift.abs() > lat_target.abs() {
                            lat_target = shift;
                        }
                        v0 = v0.min(seg.speed_limit * 0.6);
                    }
                    Some(HazardEffect::Swerve(_)) => {
                        v0 = v0.min(seg.speed_limit * 0.6);
                    }
                    Some(HazardEffect::Blocked(gap)) => {
                        acc = acc.min(idm(v, v0, gap + p.min_gap_m, 0.0, &p));
                    }
                    None => {}
                }
                // Mid lane change the body still overlaps the old lane: do not reach debris
                // there before the sideways slide (1.3 m/s) has cleared it.
                if a.ghost.is_some() {
                    if let Some(HazardEffect::Blocked(gap)) = hazard_effect(h, q, pose.f, 0.0, v) {
                        let side = geom::dot(geom::sub(geom::xy(h.pos), q), right).abs();
                        let clear = h.radius_m + CAR_WIDTH_M * 0.5 + 0.3;
                        let t_side = (clear - side).max(0.0) / 1.3;
                        if gap / v.max(0.1) < t_side + 0.5 {
                            acc = acc.min(idm(v, v0, gap + p.min_gap_m, 0.0, &p));
                        }
                    }
                }
            }

            // Junction permit / stop line.
            let next = a.plan[0];
            let mut grant: Option<SegId> = None;
            let mut revoke = false;
            if !seg.is_connector() && next != NONE && map.seg(next).is_connector() {
                let d_line = d_end - HALF_L - 0.5;
                let need = v * v / (2.0 * d_line.max(0.1));
                if a.permit == next {
                    let sig = map.lane_signal(a.seg);
                    let st = sig.map(|x| x.0);
                    let reaches = sig.is_some_and(|x| d_line / v.max(0.1) < x.1 + 0.5);
                    let limit = match st {
                        Some(SignalState::Yellow) if p.yellow_runner => 0.0,
                        Some(SignalState::Yellow) if reaches => p.amber_decel,
                        Some(SignalState::Yellow) => p.comfort_decel,
                        Some(SignalState::Red) => 5.0,
                        _ => 0.0,
                    };
                    // `limit > 0`: at a green (or no signal) a stopped car keeps its permit
                    // (need = 0 would otherwise revoke it every step).
                    if (limit > 0.0 && need <= limit)
                        || (siren && need <= 5.0)
                        || (self.tv_block[next as usize] && need <= 8.0)
                    {
                        revoke = true;
                    }
                } else if self.may_enter(map, i, next, d_line, siren) {
                    grant = Some(next);
                }
                let has = (a.permit == next && !revoke) || grant.is_some();
                if !has {
                    acc = acc.min(idm(v, v0, d_line.max(0.0) + p.min_gap_m, 0.0, &p));
                }
            } else if seg.is_connector() && next != NONE && a.plan[1] != NONE {
                // Approaching a short lane that ends at a junction: respect its stop line early.
                let ns = map.seg(next);
                if ns.length < 40.0 && map.seg(a.plan[1]).is_connector() {
                    let d_line = d_end + ns.length - HALF_L - 0.5;
                    acc = acc.min(idm(v, v0, d_line.max(0.0) + p.min_gap_m, 0.0, &p));
                }
            }

            // Reaction delay pulling away from a standstill: in a queue that starts moving, or
            // first in line at a green (straight / right). Not for moves that wait for a gap
            // (permissive lefts, stop / yield signs): the gap would close while hesitating.
            let mut start_wait = 0.0;
            if v < 0.3 && acc > 0.0 {
                let in_queue = gap < 15.0;
                let green_start = !seg.is_connector()
                    && next != NONE
                    && map.lane_signal(a.seg).map(|x| x.0) == Some(SignalState::Green)
                    && !matches!(
                        map.seg(next).kind,
                        SegKind::Connector {
                            turn: TurnKind::Left | TurnKind::UTurn,
                            ..
                        }
                    );
                if in_queue || green_start {
                    start_wait = a.start_wait_s + TRAFFIC_DT;
                    if start_wait < p.start_delay_s {
                        acc = acc.min(-0.5);
                    }
                }
            }

            if emergency {
                acc = acc.min(-8.0);
            }
            let id = a.id;
            let old_permit = a.permit;
            let a = &mut self.agents[i];
            if emergency && !a.emergency {
                self.stats.emergency_brakes += 1;
            }
            a.emergency = emergency;
            a.yielding = siren;
            a.start_wait_s = start_wait;
            a.acc = acc.clamp(-9.0, p.max_accel);
            a.lat_target = lat_target;
            if revoke {
                a.permit = NONE;
                if self.reservations[old_permit as usize] == id {
                    self.reservations[old_permit as usize] = NONE;
                }
            }
            if let Some(c) = grant {
                a.permit = c;
                self.reservations[c as usize] = id;
            }
        }
    }

    // ------------------------------------------------------------------------------------
    // Lane changes (MOBIL)
    // ------------------------------------------------------------------------------------

    fn lane_local_acc(&self, list: &[Entry], s: f32, v: f32, p: &DriverParams, v0: f32) -> f32 {
        let lead = list.iter().find(|e| e.s > s);
        match lead {
            Some(e) => idm(v, v0, e.s - s - CAR_LENGTH_M, e.v, p),
            None => idm(v, v0, f32::INFINITY, 0.0, p),
        }
    }

    fn lane_changes(&mut self, map: &CityMap, inp: &TrafficInputs) {
        for i in 0..self.agents.len() {
            let a = &self.agents[i];
            if !(a.id as u64 + self.step_count).is_multiple_of(30) {
                continue;
            }
            // The indicator stays on only while this check keeps choosing the same change.
            let prev_signal = a.pending_lc;
            self.agents[i].pending_lc = None;
            let a = &self.agents[i];
            if a.physical.is_some() || a.disabled || a.permit != NONE || a.ghost.is_some() {
                continue;
            }
            let Some(lane) = map.seg(a.seg).lane() else {
                continue;
            };
            let l = &map.net.lanes[lane as usize];
            let seg = map.seg(a.seg);
            let d_end = seg.length - a.s;
            // Debris blocking this lane forces a change (if a neighbour is free) regardless of
            // speed, timers and the junction distance.
            let pose = self.poses[i];
            let blocked = inp.hazards.iter().any(|h| {
                matches!(
                    hazard_effect(h, geom::xy(pose.p), pose.f, a.lat, a.v),
                    Some(HazardEffect::Blocked(_))
                )
            });
            if !blocked
                && (a.s < 15.0
                    || d_end < 60.0
                    || a.v < 3.0
                    || a.lat.abs() > 0.3
                    || steps_to_s(self.step_count - a.last_lane_change) < 6.0
                    || a.yielding)
            {
                continue;
            }
            if blocked && (d_end < 15.0 || a.yielding) {
                continue;
            }
            let p = a.params;
            let v0 = seg.speed_limit * speed_factor_on(map, a.seg, &p);
            let own = &self.lists[a.seg as usize];
            let pos = self.list_pos[i] as usize;
            let a_c = self.lane_local_acc(own, a.s, a.v, &p, v0);
            // Old follower.
            let old_f = own[..pos].iter().rev().find(|e| !e.ghost);
            let (a_o, a_o2) = match old_f {
                Some(e) => {
                    let fp = self.agents[e.agent as usize].params;
                    let fv0 = seg.speed_limit * speed_factor_on(map, a.seg, &fp);
                    let a_o = idm(e.v, fv0, a.s - e.s - CAR_LENGTH_M, a.v, &fp);
                    let a_o2 = self.lane_local_acc(&own[pos + 1..], e.s, e.v, &fp, fv0);
                    (a_o, a_o2)
                }
                None => (0.0, 0.0),
            };
            let mut best: Option<(f32, SegId, f32, bool)> = None;
            for (cand, to_right) in [(l.left, false), (l.right, true)] {
                let Some(c) = cand else { continue };
                let cl = &map.net.lanes[c as usize];
                if !cl.kind.drivable() || inp.closed.get(c as usize).copied().unwrap_or(false) {
                    continue;
                }
                let cs = map.seg(c);
                if cs.next.iter().all(|n| !map.seg(*n).drivable) {
                    continue;
                }
                let s2 = a.s * cs.length / seg.length.max(1e-3);
                if cs.length - s2 < if blocked { 15.0 } else { 60.0 } {
                    continue;
                }
                // The target lane must not be blocked by debris too.
                if blocked {
                    let (cp, ct) = cs.point_at(s2);
                    let target_blocked = inp.hazards.iter().any(|h| {
                        matches!(
                            hazard_effect(h, geom::xy(cp), ct, 0.0, a.v),
                            Some(HazardEffect::Blocked(_))
                        )
                    });
                    if target_blocked {
                        continue;
                    }
                }
                let list = &self.lists[c as usize];
                let lead = list.iter().find(|e| e.s > s2);
                let fol = list.iter().rev().find(|e| e.s <= s2);
                if let Some(e) = lead {
                    if e.s - s2 - CAR_LENGTH_M < p.min_gap_m + 1.0 {
                        continue;
                    }
                }
                let (a_n, a_n2) = match fol {
                    Some(e) => {
                        if s2 - e.s - CAR_LENGTH_M < 2.0 {
                            continue;
                        }
                        let fp = self.agents[e.agent as usize].params;
                        let fv0 = cs.speed_limit * speed_factor_on(map, c, &fp);
                        let a_n2 = idm(e.v, fv0, s2 - e.s - CAR_LENGTH_M, a.v, &fp);
                        if a_n2 < -4.0 {
                            continue;
                        }
                        let a_n = match lead {
                            Some(l2) => idm(e.v, fv0, l2.s - e.s - CAR_LENGTH_M, l2.v, &fp),
                            None => idm(e.v, fv0, f32::INFINITY, 0.0, &fp),
                        };
                        (a_n, a_n2)
                    }
                    None => (0.0, 0.0),
                };
                let a_c2 = match lead {
                    Some(e) => idm(a.v, v0, e.s - s2 - CAR_LENGTH_M, e.v, &p),
                    None => idm(a.v, v0, f32::INFINITY, 0.0, &p),
                };
                let bias = if to_right {
                    p.keep_right_bias
                } else {
                    -p.keep_right_bias
                };
                let forced = if blocked { 10.0 } else { 0.0 };
                let incentive =
                    a_c2 - a_c + p.politeness * (a_n2 - a_n + a_o2 - a_o) + bias + forced;
                // Hysteresis: a change already being indicated goes ahead at half the threshold.
                let thr = if prev_signal.is_some_and(|(d, _)| d == to_right) {
                    p.lane_change_threshold * 0.5
                } else {
                    p.lane_change_threshold
                };
                if incentive > thr && best.is_none_or(|b| incentive > b.0) {
                    best = Some((incentive, c, s2, to_right));
                }
            }
            if let Some((_, c, s2, to_right)) = best {
                // Discretionary changes are indicated first: the change happens at the next
                // check (≥ 0.5 s later) if it is still the best and still safe. Debris: at once.
                if !blocked {
                    match prev_signal {
                        Some((dir, t0))
                            if dir == to_right
                                && steps_to_s(self.step_count - t0) >= LANE_CHANGE_SIGNAL_S => {}
                        Some((dir, t0)) if dir == to_right => {
                            self.agents[i].pending_lc = Some((dir, t0));
                            continue;
                        }
                        _ => {
                            self.agents[i].pending_lc = Some((to_right, self.step_count));
                            continue;
                        }
                    }
                }
                let w = map.net.lanes[c as usize].width_m;
                let old_seg = self.agents[i].seg;
                let old_s = self.agents[i].s;
                let plan0 = self.choose_next(map, c, inp.closed);
                let plan1 = if plan0 != NONE {
                    map.seg(plan0).next[0]
                } else {
                    NONE
                };
                let t = self.step_count;
                let a = &mut self.agents[i];
                a.seg = c;
                a.s = s2;
                a.lat = if to_right { -w } else { w };
                a.ghost = Some((old_seg, old_s));
                a.plan = [plan0, plan1];
                a.last_lane_change = t;
                a.stopped_since = None;
                self.stats.lane_changes += 1;
                if !to_right {
                    self.stats.lane_changes_left += 1;
                }
            }
        }
    }

    // ------------------------------------------------------------------------------------
    // Integration and segment transitions
    // ------------------------------------------------------------------------------------

    fn choose_next(&mut self, map: &CityMap, lane: SegId, closed: &[bool]) -> SegId {
        let seg = map.seg(lane);
        let mut total = 0.0f32;
        let mut cands: [(SegId, f32); 16] = [(NONE, 0.0); 16];
        let mut n = 0;
        for &c in &seg.next {
            let cs = map.seg(c);
            let to = cs.next[0];
            if !cs.drivable || closed.get(to as usize).copied().unwrap_or(false) {
                continue;
            }
            // Avoid lanes that lead nowhere drivable.
            if map.seg(to).next.iter().all(|x| !map.seg(*x).drivable) {
                continue;
            }
            let w = match cs.kind {
                SegKind::Connector { turn, .. } => match turn {
                    TurnKind::Straight => 0.6,
                    TurnKind::Left | TurnKind::Right => 0.2,
                    TurnKind::UTurn => 0.05,
                },
                SegKind::Lane { .. } => 0.0,
            };
            if n < cands.len() {
                cands[n] = (c, w);
                n += 1;
                total += w;
            }
        }
        if n == 0 {
            return seg
                .next
                .iter()
                .copied()
                .find(|c| map.seg(*c).drivable)
                .unwrap_or(NONE);
        }
        let mut r = self.rng.next_f32() * total;
        for &(c, w) in &cands[..n] {
            if r < w {
                return c;
            }
            r -= w;
        }
        cands[n - 1].0
    }

    /// Move agent `i` from the end of its segment into `plan[0]` with `rem` metres left over.
    fn enter_next(&mut self, map: &CityMap, i: usize, rem: f32, closed: &[bool]) {
        let a = &self.agents[i];
        let nx = a.plan[0];
        if nx == NONE {
            self.agents[i].despawn = true;
            return;
        }
        let old = a.seg;
        let id = a.id;
        let entering_connector = map.seg(nx).is_connector();
        let plan = if entering_connector {
            let to = map.seg(nx).next[0];
            [to, self.choose_next(map, to, closed)]
        } else {
            let p1 = a.plan[1];
            [
                p1,
                if p1 != NONE {
                    map.seg(p1).next[0]
                } else {
                    NONE
                },
            ]
        };
        if map.seg(old).is_connector() && self.reservations[old as usize] == id {
            self.reservations[old as usize] = NONE;
        }
        let a = &mut self.agents[i];
        if !entering_connector {
            a.permit = NONE;
        }
        a.seg = nx;
        a.s = rem;
        a.plan = plan;
        a.ghost = None;
        a.stopped_since = None;
    }

    fn integrate(&mut self, map: &CityMap, inp: &TrafficInputs, dt: f32) {
        for i in 0..self.agents.len() {
            if self.agents[i].physical.is_some() {
                self.update_stop_state(map, i, dt);
                continue;
            }
            {
                let a = &mut self.agents[i];
                let v1 = (a.v + a.acc * dt).max(0.0);
                let ds = 0.5 * (a.v + v1) * dt;
                a.v = v1;
                let s_prev = a.s;
                a.s += ds;
                let seg = map.seg(a.seg);
                if !seg.is_connector()
                    && a.plan[0] != NONE
                    && map.seg(a.plan[0]).is_connector()
                    && a.permit != a.plan[0]
                {
                    let lim = seg.length - HALF_L - 0.3;
                    if a.s > lim {
                        a.s = lim.max(s_prev);
                        a.v = 0.0;
                    }
                }
                // Lateral motion.
                let step = 1.3 * dt;
                a.lat += (a.lat_target - a.lat).clamp(-step, step);
                if let Some((gseg, _)) = a.ghost {
                    if a.lat.abs() < 1.4 {
                        a.ghost = None;
                    } else {
                        let gl = map.seg(gseg).length;
                        if let Some(g) = a.ghost.as_mut() {
                            g.1 = (g.1 + ds).min(gl);
                        }
                    }
                }
            }
            for _ in 0..3 {
                let a = &self.agents[i];
                let len = map.seg(a.seg).length;
                if a.s < len || a.despawn {
                    break;
                }
                let rem = a.s - len;
                self.enter_next(map, i, rem, inp.closed);
            }
            self.update_stop_state(map, i, dt);
        }
    }

    fn update_stop_state(&mut self, map: &CityMap, i: usize, dt: f32) {
        let t = self.step_count;
        let a = &mut self.agents[i];
        let seg = map.seg(a.seg);
        if a.v < 0.2 && !seg.is_connector() {
            if a.stopped_since.is_none() {
                a.stopped_since = Some(t);
            }
        } else if a.v > 1.0 {
            a.stopped_since = None;
        }
        if a.v < 0.1 {
            a.stuck_s += dt;
        } else {
            a.stuck_s = 0.0;
        }
    }

    // ------------------------------------------------------------------------------------
    // Physical LOD
    // ------------------------------------------------------------------------------------

    fn path_point(&self, map: &CityMap, a: &Agent, s_target: f32) -> V3 {
        let mut seg = a.seg;
        let mut s = s_target;
        let mut k = 0;
        loop {
            let sg = map.seg(seg);
            if s <= sg.length || k >= 2 || a.plan[k] == NONE {
                let (p, t) = sg.point_at(s.min(sg.length + 30.0));
                let r = geom::right_of(t);
                return [p[0] + r[0] * a.lat_target, p[1] + r[1] * a.lat_target, p[2]];
            }
            s -= sg.length;
            seg = a.plan[k];
            k += 1;
        }
    }

    fn drive_physical(&mut self, map: &CityMap) {
        for i in 0..self.agents.len() {
            if self.agents[i].physical.is_none() {
                continue;
            }
            let a = &self.agents[i];
            let pa = a.physical.as_deref().unwrap();
            let st = pa.vehicle.state();
            let pos = geom::xy(st.pos);
            let f = forward_of(st.rot);
            let v = st.speed_mps;
            let ld = pa.controller.lookahead(v);
            let tp = self.path_point(map, a, a.s + ld);
            let steer = pa.controller.steer(pos, f, tp);
            let acc = a.acc;
            let disabled = a.disabled;
            let pa = self.agents[i].physical.as_deref_mut().unwrap();
            let (throttle, brake) = pa.controller.pedals(v, acc, TRAFFIC_DT, disabled);
            pa.input = VehicleInput {
                steer,
                throttle,
                brake,
                headlights: map.time.night_factor() > 0.5,
                ..Default::default()
            };
            // Stopped (queue, stop line, wreck): hold on the handbrake. The PID releases the
            // foot brake near standstill (auto-reverse), which would let the car idle forward.
            if (acc < 0.0 || disabled) && v < crate::leader::HOLD_BEHIND_MPS {
                pa.input = crate::leader::held(pa.input);
            }
        }
    }

    fn promote(&mut self, map: &CityMap, i: usize) {
        let Some((def, tires)) = &self.car else {
            return;
        };
        let a = &self.agents[i];
        let (p, f) = self.pose_of(map, a);
        let yaw = geom::yaw_of(f);
        let mut vehicle = Vehicle::new(def, tires, p, yaw);
        vehicle.set_pose_and_speed(p, yaw, a.v);
        let pa = PhysicalAgent {
            vehicle,
            input: VehicleInput::default(),
            controller: DriveController::new(
                DriveGains::TRAFFIC,
                def.wheelbase_m,
                def.wheels[0].max_steer_rad,
                a.v,
            ),
        };
        let a = &mut self.agents[i];
        a.physical = Some(Box::new(pa));
        a.lod = Lod::Physical;
        a.ghost = None;
        self.stats.promotions += 1;
        self.lod_changes.push(LodChange::Promoted(a.id));
    }

    fn demote(&mut self, map: &CityMap, i: usize) {
        let a = &mut self.agents[i];
        if let Some(pa) = a.physical.take() {
            let st = pa.vehicle.state();
            let pr = map.seg(a.seg).project(geom::xy(st.pos));
            a.s = pr.s.clamp(0.0, map.seg(a.seg).length);
            a.lat = pr.lateral.clamp(-1.5, 1.5);
            a.lat_target = 0.0;
            a.v = st.speed_mps.max(0.0);
        }
        a.lod = Lod::Kinematic;
        self.stats.demotions += 1;
        self.lod_changes.push(LodChange::Demoted(a.id));
    }

    fn update_lod(&mut self, map: &CityMap, inp: &TrafficInputs) {
        let r = self.config.physical_radius_m;
        let near = |q: V2| -> f32 {
            let mut best = f32::INFINITY;
            for t in inp.tracked.iter().filter(|t| t.player) {
                best = best.min(geom::dist(q, geom::xy(t.pos)));
            }
            for f in inp.route_focus {
                best = best.min(geom::dist(q, *f));
            }
            best
        };
        let mut cands: Vec<(f32, u32)> = Vec::new();
        let mut physical = 0usize;
        for i in 0..self.agents.len() {
            let d = near(geom::xy(self.poses[i].p));
            let a = &self.agents[i];
            if a.physical.is_some() {
                if a.disabled
                    && d > r + 100.0
                    && !self.visible(geom::xy(self.poses[i].p), inp.tracked)
                {
                    self.agents[i].despawn = true;
                    self.stats.despawned += 1;
                } else if (d > r + 20.0 && !a.disabled) || self.car.is_none() {
                    self.demote(map, i);
                } else {
                    physical += 1;
                }
            } else if d < r && self.car.is_some() {
                cands.push((d, i as u32));
            }
        }
        cands.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for (_, i) in cands {
            if physical >= self.config.max_physical {
                break;
            }
            self.promote(map, i as usize);
            physical += 1;
        }
    }

    // ------------------------------------------------------------------------------------
    // Density-driven spawning
    // ------------------------------------------------------------------------------------

    fn visible(&self, q: V2, tracked: &[TrackedVehicle]) -> bool {
        let r = self.config.sight_radius_m;
        tracked.iter().filter(|t| t.player).any(|t| {
            let p = geom::xy(t.pos);
            let ahead = [p[0] + t.vel[0], p[1] + t.vel[1]];
            geom::dist(q, p) < r || geom::dist(q, ahead) < r
        })
    }

    /// Per-zone density targets (after scaling to the cap) for the current inputs.
    pub fn zone_targets(&self, map: &CityMap, inp: &TrafficInputs) -> Vec<f32> {
        let mut focus: Vec<V2> = inp
            .tracked
            .iter()
            .filter(|t| t.player)
            .map(|t| geom::xy(t.pos))
            .collect();
        focus.extend_from_slice(inp.route_focus);
        let global = focus.is_empty();
        let ar = self.config.active_radius_m;
        let mut targets = vec![0.0f32; map.net.zones.len()];
        for (z, samples) in self.zone_samples.iter().enumerate() {
            let zone = &map.net.zones[z];
            if !global {
                let (lo, hi) = self.zone_bbox[z];
                let near = focus.iter().any(|f| {
                    f[0] > lo[0] - ar && f[0] < hi[0] + ar && f[1] > lo[1] - ar && f[1] < hi[1] + ar
                });
                if !near {
                    continue;
                }
            }
            let mut w = 0.0;
            for smp in samples {
                if inp.closed.get(smp.seg as usize).copied().unwrap_or(false) {
                    continue;
                }
                if global || focus.iter().any(|f| geom::dist(*f, smp.pos) < ar) {
                    w += smp.weight_km;
                }
            }
            targets[z] =
                w * zone.density_base * f_time(map.time, zone.kind) * self.config.density_scale;
        }
        let total: f32 = targets.iter().sum();
        let cap = self.config.max_agents as f32;
        if total > cap {
            let k = cap / total;
            for t in &mut targets {
                *t *= k;
            }
        }
        targets
    }

    /// Current agent count per zone (agents outside every zone are not counted).
    pub fn zone_counts(&self, map: &CityMap) -> Vec<u32> {
        let mut counts = vec![0u32; map.net.zones.len()];
        for a in &self.agents {
            let (p, _) = self.pose_of(map, a);
            if let Some(z) = map.zone_at(geom::xy(p)) {
                counts[z as usize] += 1;
            }
        }
        counts
    }

    // Index loops: agents are mutated in place while parallel arrays are read.
    #[allow(clippy::needless_range_loop)]
    fn spawn_update(&mut self, map: &CityMap, inp: &TrafficInputs, budget: usize) {
        let targets = self.zone_targets(map, inp);
        let mut focus: Vec<V2> = inp
            .tracked
            .iter()
            .filter(|t| t.player)
            .map(|t| geom::xy(t.pos))
            .collect();
        focus.extend_from_slice(inp.route_focus);
        let global = focus.is_empty();
        let ar = self.config.active_radius_m;

        let mut counts = vec![0u32; targets.len()];
        let mut agent_zone: Vec<Option<ZoneId>> = Vec::with_capacity(self.agents.len());
        for i in 0..self.agents.len() {
            let z = map.zone_at(geom::xy(self.poses[i].p));
            if let Some(z) = z {
                counts[z as usize] += 1;
            }
            agent_zone.push(z);
        }
        // District-level totals: zones are small, so control is per district kind with a
        // looser per-zone clamp against local clumps.
        let kind_of = |z: usize| map.net.zones[z].kind as usize;
        let mut kind_t = [0.0f32; 6];
        let mut kind_c = [0.0f32; 6];
        for z in 0..targets.len() {
            kind_t[kind_of(z)] += targets[z];
            kind_c[kind_of(z)] += counts[z] as f32;
        }

        // Despawn: out of the active area, map-closed lanes, stuck, or zone surplus.
        let mut removed_in_zone = vec![false; targets.len()];
        for i in 0..self.agents.len() {
            let q = geom::xy(self.poses[i].p);
            let a = &self.agents[i];
            if a.despawn || a.physical.is_some() || self.visible(q, inp.tracked) {
                continue;
            }
            let in_connector = map.seg(a.seg).is_connector() || a.permit != NONE;
            let outside = !global && !focus.iter().any(|f| geom::dist(*f, q) < ar + 60.0);
            let closed = inp.closed.get(a.seg as usize).copied().unwrap_or(false);
            let stuck = a.stuck_s > self.config.stuck_timeout_s;
            let surplus = agent_zone[i].is_some_and(|z| {
                let z = z as usize;
                let (t, c) = (targets[z], counts[z] as f32);
                let k = kind_of(z);
                !removed_in_zone[z]
                    && ((kind_c[k] > kind_t[k] * 1.04 + 1.0 && c > t + 0.5) || c > t * 1.5 + 3.0)
            });
            if (outside || closed || stuck || surplus) && !in_connector {
                if stuck {
                    self.stats.stuck_recycled += 1;
                }
                if let Some(z) = agent_zone[i] {
                    counts[z as usize] -= 1;
                    kind_c[kind_of(z as usize)] -= 1.0;
                    removed_in_zone[z as usize] = surplus;
                }
                self.agents[i].despawn = true;
                self.stats.despawned += 1;
            }
        }

        // Spawn into the zones with the largest deficit.
        let mut order: Vec<(f32, usize)> = targets
            .iter()
            .enumerate()
            .map(|(z, t)| (t - counts[z] as f32, z))
            .filter(|(d, _)| *d >= 0.5)
            .collect();
        order.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut spawned = 0usize;
        let alive = self.agents.iter().filter(|a| !a.despawn).count();
        let mut room = self.config.max_agents.saturating_sub(alive);
        for (deficit, z) in order {
            let k = kind_of(z);
            let mut want = deficit.round().max(1.0) as usize;
            while want > 0 && spawned < budget && room > 0 && kind_c[k] < kind_t[k] - 0.5 {
                want -= 1;
                if self.try_spawn(map, inp, z, global, &focus) {
                    spawned += 1;
                    room -= 1;
                    kind_c[k] += 1.0;
                }
            }
        }
    }

    fn try_spawn(
        &mut self,
        map: &CityMap,
        inp: &TrafficInputs,
        z: usize,
        global: bool,
        focus: &[V2],
    ) -> bool {
        let n = self.zone_samples[z].len();
        if n == 0 {
            return false;
        }
        for _ in 0..6 {
            let smp = self.zone_samples[z][self.rng.below(n as u32) as usize];
            let seg = map.seg(smp.seg);
            if inp.closed.get(smp.seg as usize).copied().unwrap_or(false) {
                continue;
            }
            if !global
                && !focus
                    .iter()
                    .any(|f| geom::dist(*f, smp.pos) < self.config.active_radius_m)
            {
                continue;
            }
            if self.visible(smp.pos, inp.tracked) {
                continue;
            }
            let s = smp.s;
            if s < CAR_LENGTH_M + 3.0 || s > seg.length - 25.0 {
                continue;
            }
            let list = &self.lists[smp.seg as usize];
            let ahead = list.iter().find(|e| e.s >= s);
            let behind = list.iter().rev().find(|e| e.s < s);
            let gap_a = ahead
                .map(|e| e.s - s - CAR_LENGTH_M)
                .unwrap_or(f32::INFINITY);
            if gap_a < 12.0 {
                continue;
            }
            if let Some(e) = behind {
                if s - e.s - CAR_LENGTH_M < 12.0 + e.v * 1.6 {
                    continue;
                }
            }
            // Do not spawn on top of a tracked vehicle or next to debris.
            if inp
                .tracked
                .iter()
                .any(|t| geom::dist(geom::xy(t.pos), smp.pos) < 10.0)
                || inp
                    .hazards
                    .iter()
                    .any(|h| geom::dist(geom::xy(h.pos), smp.pos) < h.radius_m + 30.0)
            {
                continue;
            }
            let personality = {
                let r = self.rng.next_f32();
                if r < self.config.aggressive_fraction {
                    Personality::Aggressive
                } else if r < self.config.aggressive_fraction + self.config.casual_fraction {
                    Personality::Casual
                } else {
                    Personality::LawAbiding
                }
            };
            let params = DriverParams::sample(personality, &mut self.rng);
            let v0 = seg.speed_limit * speed_factor_on(map, smp.seg, &params);
            let room = (seg.length - s - 15.0).max(0.0);
            let v = (v0 * 0.8)
                .min(((gap_a - 6.0) / 1.6).max(0.0))
                .min((2.0 * 2.0 * room).sqrt());
            let plan0 = self.choose_next(map, smp.seg, inp.closed);
            let plan1 = if plan0 != NONE {
                map.seg(plan0).next[0]
            } else {
                NONE
            };
            let id = self.next_id;
            self.next_id += 1;
            let variant = self.rng.below(8) as u8;
            let agent = Agent {
                id,
                entity: None,
                personality,
                params,
                seg: smp.seg,
                s,
                v,
                acc: 0.0,
                lat: 0.0,
                lat_target: 0.0,
                plan: [plan0, plan1],
                permit: NONE,
                stopped_since: None,
                last_lane_change: self.step_count,
                pending_lc: None,
                start_wait_s: 0.0,
                ghost: None,
                stuck_s: 0.0,
                emergency: false,
                yielding: false,
                disabled: false,
                variant,
                lod: Lod::Kinematic,
                physical: None,
                despawn: false,
            };
            // Keep lists current for later spawns this update.
            let idx = self.agents.len() as u32;
            let l = &mut self.lists[smp.seg as usize];
            if l.is_empty() {
                self.touched.push(smp.seg);
            }
            let pos = l.partition_point(|e| e.s < s);
            l.insert(
                pos,
                Entry {
                    s,
                    v,
                    agent: idx,
                    ghost: false,
                },
            );
            let (p, f) = seg.point_at(s);
            self.poses.push(Pose {
                p,
                f,
                vel: geom::scale(f, v),
            });
            self.agents.push(agent);
            self.list_pos.push(pos as u32);
            self.stats.spawned += 1;
            return true;
        }
        false
    }

    // ------------------------------------------------------------------------------------
    // Diagnostics
    // ------------------------------------------------------------------------------------

    fn detect_collisions(&mut self) {
        const CELL: f32 = 8.0;
        let mut keys: Vec<((i32, i32), u32)> = Vec::with_capacity(self.agents.len());
        for (i, ps) in self.poses.iter().enumerate() {
            if self.agents[i].despawn {
                continue;
            }
            keys.push((
                (
                    (ps.p[0] / CELL).floor() as i32,
                    (ps.p[1] / CELL).floor() as i32,
                ),
                i as u32,
            ));
        }
        keys.sort_unstable();
        let mut now: BTreeSet<(u32, u32)> = BTreeSet::new();
        for &((cx, cy), i) in &keys {
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let k = (cx + dx, cy + dy);
                    let start = keys.partition_point(|e| e.0 < k);
                    for &(kk, j) in &keys[start..] {
                        if kk != k {
                            break;
                        }
                        if j <= i {
                            continue;
                        }
                        let (pa, pb) = (self.poses[i as usize], self.poses[j as usize]);
                        if (pa.p[2] - pb.p[2]).abs() > 3.0 {
                            continue;
                        }
                        let oa = Obb {
                            c: geom::xy(pa.p),
                            f: pa.f,
                            hl: HALF_L,
                            hw: CAR_WIDTH_M * 0.5,
                        };
                        let ob = Obb {
                            c: geom::xy(pb.p),
                            f: pb.f,
                            ..oa
                        };
                        if geom::obb_overlap(&oa, &ob) {
                            let (ia, ib) = (self.agents[i as usize].id, self.agents[j as usize].id);
                            now.insert((ia.min(ib), ia.max(ib)));
                        }
                    }
                }
            }
        }
        for pair in &now {
            if !self.overlaps.contains(pair) {
                self.stats.civilian_collisions += 1;
                if self.stats.collision_log.len() < 64 {
                    self.stats
                        .collision_log
                        .push((self.step_count, pair.0, pair.1));
                }
                let aggressive = self.agents.iter().any(|a| {
                    (a.id == pair.0 || a.id == pair.1) && a.personality == Personality::Aggressive
                });
                if aggressive {
                    self.stats.collisions_involving_aggressive += 1;
                }
            }
        }
        self.overlaps = now;
    }
}
