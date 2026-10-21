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

use std::collections::{BTreeMap, BTreeSet};

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

mod blocked;
pub use blocked::Wait;
use blocked::{Manoeuvre, ObsRef};
mod jam;
pub mod siren;
use siren::SirenRel;
#[cfg(test)]
mod signal_tests;
pub use siren::{SirenYield, YieldPhase};

/// Civilian car length used for spacing and overlap tests, m.
pub const CAR_LENGTH_M: f32 = 4.5;
/// Civilian car width, m.
pub const CAR_WIDTH_M: f32 = 1.9;

/// Look-ahead for an emergency stop on a tracked vehicle's collision course, s.
const EMERGENCY_HORIZON_S: f32 = 2.5;
/// Side clearance under which a closest approach counts as a hit for the emergency stop, m
/// (on top of both cars' half widths).
const EMERGENCY_SIDE_MARGIN_M: f32 = 0.4;
/// Traffic AI update period (60 Hz), s.
pub const TRAFFIC_DT: f32 = 1.0 / 60.0;
/// Physics ticks per traffic step (240 Hz / 60 Hz).
pub const TICKS_PER_TRAFFIC_STEP: u64 = 4;
/// A discretionary lane change is indicated at least this long before it starts, s.
/// Lane changes are re-checked every 0.5 s, so this is the next check choosing the same change.
/// Kept at 0.5 s: 1.5 s cut lane changes by a third (traffic_bench) and would move the goldens; the
/// indicator then stays on through the lateral move ([`LANE_CHANGE_SIGNAL_OFF_M`]), about
/// 3 s in all.
pub const LANE_CHANGE_SIGNAL_S: f32 = 0.5;
/// A junction turn is indicated this long before the end of the lane (at the current speed), s.
pub const TURN_SIGNAL_LEAD_S: f32 = 3.0;
/// Shortest distance before the end of the lane at which a junction turn is indicated, m.
pub const TURN_SIGNAL_MIN_M: f32 = 30.0;
/// Longest distance before the end of the lane at which a junction turn is indicated, m.
pub const TURN_SIGNAL_MAX_M: f32 = 50.0;
/// After a lane change the indicator stays on until the car is this close to its new lane
/// centre, m.
pub const LANE_CHANGE_SIGNAL_OFF_M: f32 = 0.3;

/// Distance before the end of the lane at which a car doing `speed_mps` switches on its
/// indicator for a junction turn: [`TURN_SIGNAL_LEAD_S`] of travel, clamped to
/// [`TURN_SIGNAL_MIN_M`]..[`TURN_SIGNAL_MAX_M`], m.
pub fn turn_signal_lead_m(speed_mps: f32) -> f32 {
    (speed_mps * TURN_SIGNAL_LEAD_S).clamp(TURN_SIGNAL_MIN_M, TURN_SIGNAL_MAX_M)
}

/// Indicator for a junction turn (-1 left, 0 none, 1 right; right-hand traffic: a U-turn
/// indicates left).
fn turn_indicator(turn: TurnKind) -> i8 {
    match turn {
        TurnKind::Left | TurnKind::UTurn => -1,
        TurnKind::Right => 1,
        TurnKind::Straight => 0,
    }
}

/// Sightline radius while filling the area before the first tick ([`Traffic::populate`]), m.
/// Nobody has seen the city yet, so the start state keeps cars this close (the live rule,
/// `TrafficConfig::sight_radius_m`, is wider so cars never pop in or out in plain view).
pub const POPULATE_SIGHT_RADIUS_M: f32 = 150.0;

/// A driver who has stood at its stop line this long (outside red) books its turn
/// ([`Traffic::book_for_waiting`]), s.
pub const BOOK_AFTER_S: f32 = 20.0;
/// A booking not used within this long lapses, s.
pub const BOOK_MAX_S: f32 = 10.0;
/// After a booking lapsed, no new one for this long, s.
pub const BOOK_RETRY_S: f32 = 30.0;
/// A booking may also be made while the exit has no room yet (the car starves behind a short
/// block that cars from the other approaches keep filling); it then waits for room at most
/// this long before it lapses, s.
pub const BOOK_ROOM_MAX_S: f32 = 40.0;
/// An all-way-stop driver who stopped first but has not gone after this long (outside red, no
/// permit) no longer has the right of way over the others, s.
pub const ALLWAY_STALE_S: f32 = 8.0;
/// A tracked vehicle in a junction faster than this sweeps through it (crossing movements wait);
/// slower (a police car shunting at a scene) it only blocks the paths it stands in, m/s.
pub const TV_SWEEP_MPS: f32 = 2.5;
/// Two civilians touching again within this many traffic steps (2 s) of separating are
/// still one collision in [`TrafficStats::civilian_collisions`] (cars that bump, part and
/// bump again counted twice).
pub const RETOUCH_STEPS: u64 = 120;
/// A unit beside a car that is merging back after a siren yield counts as standing (the car
/// creeps on past it along the kerb) below this speed, m/s; a moving one is waited for.
pub const BESIDE_STANDING_MPS: f32 = 1.0;
/// ... and only with at least this much room between the two bodies' sides, m.
pub const BESIDE_CLEAR_M: f32 = 0.5;
/// A driver held at its stop line by a siren unit heading for the junction goes anyway after
/// this long (the unit is milling about a scene rather than coming through), s.
pub const SIREN_JUNCTION_HOLD_MAX_S: f32 = 8.0;

/// A wreck further than this from its lane's centre line is off the road (sidewalk, lot) and
/// not an obstacle in the lane, m.
pub const OFF_LANE_WRECK_M: f32 = 3.5;

/// No spawn behind a (nearly) stopped car closer than this ahead (spawning into a queue), m.
pub const SPAWN_QUEUE_LOOK_M: f32 = 60.0;
/// Spawning stops on a lane holding one car per this many metres at the daytime peak (more
/// metres per car as the time-of-day factor drops), m.
pub const SPAWN_LANE_SPACING_M: f32 = 20.0;

/// A car stopped at its stop line without a permit for longer than this (outside red lights) is
/// not counted as arriving at the junction by the drivers who would yield to it, s.
pub const STALE_ARRIVAL_S: f32 = 4.0;

/// Upper bound on how much [`Traffic`] raises the density target of the out-of-sight part of
/// the active area, to make up for the part in a player's view that cannot be spawned into.
pub const MAX_SPAWNABLE_BOOST: f32 = 2.0;

/// At or below this time-of-day density factor ([`crate::density::f_time`]) the out-of-sight
/// boost is off, so night traffic never rises above the curve (owner playtest row 76).
pub const BOOST_OFF_AT_F_TIME: f32 = 0.2;

/// At or above this time-of-day density factor the full [`MAX_SPAWNABLE_BOOST`] applies
/// (all daytime hours); in between it is interpolated linearly.
pub const BOOST_FULL_AT_F_TIME: f32 = 0.6;

/// Largest out-of-sight boost for a zone whose time-of-day factor is `f`.
pub fn max_spawnable_boost(f: f32) -> f32 {
    let s =
        ((f - BOOST_OFF_AT_F_TIME) / (BOOST_FULL_AT_F_TIME - BOOST_OFF_AT_F_TIME)).clamp(0.0, 1.0);
    1.0 + (MAX_SPAWNABLE_BOOST - 1.0) * s
}

/// A car turning around or reversing (it cannot pull over) counts as yielding while a siren unit
/// is this close, m. Everyone else follows [`siren`].
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
    /// This is the distance at which traffic pops in, so it is kept beyond the draw distance
    /// a player needs at highway speed (default 320 m).
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
    /// Unseen agents that have stood still this long outside a red light are recycled, s
    /// (`traffic::jam`, owner row 85).
    pub jam_recycle_s: f32,
}

impl Default for TrafficConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_agents: 400,
            max_physical: 16,
            // Lead helper, playtest row 25 (LOD pop-in): cars appeared / vanished 150 m from
            // the player, in plain view on Port Vesper's highway and hill sightlines. Spawns and
            // despawns now stay >= 320 m away (tests/lod_draw.rs); the populated radius grew
            // with it so the density around the player stays about the same.
            active_radius_m: 440.0,
            sight_radius_m: 320.0,
            physical_radius_m: 80.0,
            casual_fraction: 0.25,
            aggressive_fraction: 0.05,
            density_falloff_m: 600.0,
            density_scale: 1.0,
            detect_collisions: false,
            stuck_timeout_s: 120.0,
            jam_recycle_s: 30.0,
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
    /// New civilian–civilian overlap pairs (only with `detect_collisions`); a pair touching
    /// again within [`RETOUCH_STEPS`] of separating is not counted again.
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
    /// Lane changes made to get around a stationary vehicle (blocked-vehicle logic; also
    /// counted in `lane_changes`).
    #[serde(default)]
    pub go_around_lane_changes: u64,
    /// Stationary vehicles passed in the oncoming lane (completed passes).
    #[serde(default)]
    pub oncoming_passes: u64,
    /// Oncoming-lane passes abandoned (traffic appeared): the car went back behind the blocker.
    #[serde(default)]
    pub pass_aborts: u64,
    /// U-turns / k-point turns made because the lane was blocked.
    #[serde(default)]
    pub turnarounds: u64,
    /// Cars that reversed back to the last junction because the lane was blocked.
    #[serde(default)]
    pub reverses: u64,
    /// Plans changed to avoid a lane known to be blocked.
    #[serde(default)]
    pub reroutes: u64,
    /// Turn-arounds finished (the car drove off in the opposing lane).
    #[serde(default)]
    pub turnarounds_completed: u64,
    /// Civilians that pulled over for a siren unit coming up behind them ([`siren`]).
    #[serde(default)]
    pub siren_yields: u64,
    /// Lane changes made to get out of a siren unit's lane (also counted in `lane_changes`).
    #[serde(default)]
    pub siren_lane_changes: u64,
    /// First 64 new overlap pairs: `(traffic step, agent a, agent b)`.
    pub collision_log: Vec<(u64, AgentId, AgentId)>,
    /// Longest any agent has stood still outside a red / amber light and away from a standing
    /// scene (a tracked vehicle stopped for 20 s: a capture, a parked player), s (jam
    /// diagnostics, owner row 85; see `traffic::jam`).
    #[serde(default)]
    pub max_stop_s: f32,
    /// Longest queue of stopped cars back from a stop line (followed back through junctions),
    /// sampled once a second.
    #[serde(default)]
    pub max_queue: u32,
    /// Times a car stood inside a junction box for more than 5 s.
    #[serde(default)]
    pub box_stops: u64,
    /// Unseen agents recycled because they stood still outside a red light for
    /// `TrafficConfig::jam_recycle_s` (not counted in `stuck_recycled`).
    #[serde(default)]
    pub jam_recycled: u64,
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
    /// Stopped time outside red lights (jam diagnostics, `traffic::jam`), s.
    jam_s: f32,
    /// No forced (blocked-lane) lane change before this traffic step (set when one stalled).
    lc_block_until: u64,
    /// No junction permit before this traffic step (set when one was given up for an
    /// obstruction).
    permit_block_until: u64,
    /// Time its current booking has held ([`Traffic::book_for_waiting`]), s.
    booked_s: f32,
    /// No booking before this traffic step.
    book_block_until: u64,
    /// Time its booking has waited for room on the exit, s.
    book_room_s: f32,
    /// Time held at the stop line by a siren unit heading for the junction, s
    /// ([`SIREN_JUNCTION_HOLD_MAX_S`]).
    siren_hold_s: f32,
    /// Stopped time inside a junction connector, s.
    box_s: f32,
    /// Stopped time for the jam metric: like `jam_s`, also held near a standing scene, s.
    metric_stop_s: f32,
    /// Time the current manoeuvre has not moved, s.
    man_still_s: f32,
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
    /// Why the agent is held up (blocked-vehicle logic), as of its last decision.
    pub wait: Wait,
    /// Time spent stopped behind a stationary obstruction, s.
    blocked_s: f32,
    /// How long the obstruction ahead has been stationary, s.
    obstacle_still_s: f32,
    /// What the obstruction is (valid while `wait == Wait::Obstructed`).
    obs: ObsRef,
    /// How long this driver waits behind a stationary vehicle before going around or turning
    /// (3–8 s by personality, derived from the id and the traffic seed).
    pub patience_s: f32,
    /// Time until the next go-around / turn-around attempt, s.
    retry_s: f32,
    /// Obstructed long enough to force a lane change (MOBIL with a lowered threshold).
    go_around: bool,
    /// Manoeuvre in progress (pass in the oncoming lane, turn around, reverse).
    manoeuvre: Option<Box<Manoeuvre>>,
    /// Connector the agent entered its current lane from (`NO_SEG` if spawned there).
    came_from: SegId,
    /// Yield to a siren unit coming up behind ([`siren`]).
    pub siren: SirenYield,
    /// Reaction time before pulling over for a siren, s ([`siren::reaction_s`]).
    pub siren_reaction_s: f32,
    /// Wants a lane change out of a siren unit's lane (multi-lane roads): `Some(to the right)`.
    siren_lc: Option<bool>,
    /// Connector whose turn is being indicated (`NONE` = none). Latched once the car is within
    /// [`turn_signal_lead_m`] of the lane end, so the indicator does not go off again when the
    /// car slows down in the queue. View-only: not in the state hash, never read by the AI.
    turn_latch: SegId,
    /// Lateral move of the last lane change still being indicated (-1 left, 1 right, 0 none);
    /// cleared once the car is centred in its new lane. View-only like `turn_latch`.
    lc_signal: i8,
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
    /// The tracked vehicle / hazard behind `tv_block` is moving (> 1 m/s): it will sweep
    /// through, so crossing paths are held too. A stationary one blocks only its own path.
    tv_block_moving: Vec<bool>,
    tv_block_list: Vec<SegId>,
    zone_samples: Vec<Vec<Sample>>,
    zone_bbox: Vec<(V2, V2)>,
    spawn_timer: f32,
    stats: TrafficStats,
    lod_changes: Vec<LodChange>,
    overlaps: BTreeSet<(u32, u32)>,
    /// Pairs that stopped overlapping recently, with the traffic step it ended: a re-touch
    /// within [`RETOUCH_STEPS`] is the same incident, not a new collision.
    overlap_ended: BTreeMap<(u32, u32), u64>,
    /// Car models for physical agents; agent `id` uses `cars[traffic_model_index(id, len)]`.
    cars: Vec<(VehicleDef, TireDb)>,
    /// Seed for per-driver values derived from the id (patience).
    seed: u64,
    /// Opposing lane next to the centre line, per lane segment (`NONE` if none / not the
    /// leftmost lane).
    opposite: Vec<SegId>,
    /// Drivable lanes of each road (both directions).
    road_lanes: Vec<Vec<SegId>>,
    /// Lanes known to be blocked, with the traffic step the knowledge expires at.
    blocked_lanes: std::collections::BTreeMap<SegId, u64>,
    /// How long each tracked vehicle has been stationary, s (by entity id).
    tracked_still: std::collections::BTreeMap<u32, f32>,
    /// Tracked vehicles that have stood still for [`jam::SCENE_STILL_S`] (a pursuit scene, a
    /// parked player): positions, for the jam metric.
    scenes: Vec<V2>,
    /// Inside [`Traffic::populate`]: the sightline rule uses [`POPULATE_SIGHT_RADIUS_M`].
    populating: bool,
}

/// Which of `n` traffic car models agent `id` drives: a pure function of the id (a SplitMix64
/// finalizer, modulo `n`), so the choice is the same while the agent is kinematic and after it
/// is promoted to physics, on every platform, and uses no RNG draws (single-model traffic is
/// bit-identical to before). `n <= 1` → 0.
pub fn traffic_model_index(id: AgentId, n: usize) -> usize {
    if n <= 1 {
        return 0;
    }
    let mut z = (id as u64).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z % n as u64) as usize
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

/// A stopped car within 40 m ahead of `s` in `list`.
fn list_has_stopped_ahead(list: &[Entry], s: f32) -> bool {
    list.iter()
        .any(|e| e.s > s && e.s - s < 40.0 + CAR_LENGTH_M && e.v < 0.5)
}

/// A tracked vehicle standing in the lane band ahead of `p` (facing `t`) within 40 m.
fn tracked_blocks(tracked: &[TrackedVehicle], p: V2, t: V2) -> bool {
    tracked.iter().any(|tv| {
        let r = geom::sub(geom::xy(tv.pos), p);
        let long = geom::dot(r, t);
        let lat = geom::dot(r, geom::right_of(t));
        let still = tv.vel[0] * tv.vel[0] + tv.vel[1] * tv.vel[1] < 0.25;
        let tf = if geom::dot(tv.forward, tv.forward) > 0.25 {
            geom::normalize(tv.forward)
        } else {
            t
        };
        let ext = 2.25 * geom::dot(tf, geom::right_of(t)).abs() + 0.95 * geom::dot(tf, t).abs();
        still && long > -HALF_L && long < 40.0 && lat.abs() < CAR_WIDTH_M * 0.5 + ext + 0.3
    })
}

/// The constraint that sets an agent's acceleration this step.
#[derive(Debug, Clone, Copy)]
enum Bind {
    /// Stop line, signal, siren, junction rules, start delay, oncoming car while passing.
    Line,
    /// The car ahead (agent index, if any) at this gap, m.
    Leader(Option<u32>, f32),
    /// Tracked vehicle (index in the inputs) and how it holds the agent up, at this gap.
    Tracked(usize, Wait, f32),
    /// Debris (position, radius) at this gap.
    Hazard(V2, f32, f32),
}

/// Pass state change decided this step.
#[derive(Debug, Clone, Copy, PartialEq)]
enum PassEvent {
    None,
    Return,
    Finish,
    Abort,
}

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
        let (opposite, road_lanes) = blocked::lane_topology(map);
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
            tv_block_moving: vec![false; nseg],
            tv_block_list: Vec::new(),
            zone_samples,
            zone_bbox,
            spawn_timer: 0.0,
            stats: TrafficStats::default(),
            lod_changes: Vec::new(),
            overlaps: BTreeSet::new(),
            overlap_ended: BTreeMap::new(),
            cars: Vec::new(),
            seed,
            opposite,
            road_lanes,
            blocked_lanes: Default::default(),
            tracked_still: Default::default(),
            scenes: Vec::new(),
            populating: false,
        }
    }

    /// Vehicle definition used for physical (near-LOD) agents. Without one, agents stay
    /// kinematic.
    pub fn set_traffic_car(&mut self, def: VehicleDef, tires: TireDb) {
        self.cars = vec![(def, tires)];
    }

    /// Several car models for physical agents (empty = agents stay kinematic). Each agent's
    /// model is [`traffic_model_index`] of its id; with one model this is
    /// [`Traffic::set_traffic_car`].
    pub fn set_traffic_cars(&mut self, cars: Vec<(VehicleDef, TireDb)>) {
        self.cars = cars;
    }

    /// Number of physical car models set.
    pub fn model_count(&self) -> usize {
        self.cars.len()
    }

    /// Index into the models of [`Traffic::set_traffic_cars`] that agent `id` drives (0 when
    /// fewer than two models are set).
    pub fn model_of(&self, id: AgentId) -> usize {
        traffic_model_index(id, self.cars.len())
    }

    /// The car model agent `id` drives once physical ([`Traffic::model_of`]); `None` while no
    /// physical traffic model is set.
    fn car_of(&self, id: AgentId) -> Option<&(VehicleDef, TireDb)> {
        self.cars.get(self.model_of(id))
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
            if let SegKind::Connector { turn, .. } = map.seg(a.seg).kind {
                // Keep indicating through the junction until on the exit lane.
                turn_signal = turn_indicator(turn);
            } else if a.plan[0] != NONE && a.plan[0] == a.turn_latch {
                if let SegKind::Connector { turn, .. } = map.seg(a.plan[0]).kind {
                    turn_signal = turn_indicator(turn);
                }
            }
            if a.ghost.is_some() || a.lc_signal != 0 {
                // Lane change under way: indicate until centred in the new lane.
                turn_signal = if a.lc_signal != 0 {
                    a.lc_signal
                } else if a.lat < 0.0 {
                    1
                } else {
                    -1
                };
            } else if let Some((to_right, _)) = a.pending_lc {
                turn_signal = if to_right { 1 } else { -1 };
            }
            match a.siren.phase {
                YieldPhase::PullingOver | YieldPhase::Waiting => {
                    turn_signal = if a.siren.side < 0.0 { -1 } else { 1 };
                }
                YieldPhase::MergingBack => {
                    turn_signal = if a.siren.side < 0.0 { 1 } else { -1 };
                }
                _ => {}
            }
            let mut hazard_lights = a.disabled || a.emergency;
            if a.disabled {
                // A wreck shows hazards only.
                turn_signal = 0;
            }
            if let Some(m) = &a.manoeuvre {
                match m.signal() {
                    Some(s) => turn_signal = s,
                    None => hazard_lights = true,
                }
            }
            AgentView {
                id: a.id,
                pos: p,
                yaw: geom::yaw_of(f),
                speed_mps: a.v,
                lod: a.lod,
                personality: a.personality,
                brake_lights: a.acc < -0.4 || (a.v < 0.1 && !a.disabled),
                hazard_lights,
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
        if let Some(pose) = a.manoeuvre.as_deref().and_then(|m| m.kinematic_pose()) {
            return pose;
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
            // Only agents in a manoeuvre / yielding to a siren add to the hash (unchanged hashes
            // otherwise).
            if let Some(m) = &a.manoeuvre {
                m.hash_into(h);
            }
            if a.siren != SirenYield::default() {
                a.siren.hash_into(h);
            }
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
        self.update_tracked_still(inp.tracked, dt);
        self.expire_blocked_lanes();
        self.sync_physical(map, inp);
        self.rebuild_lists(map);
        self.rebuild_reservations(map);
        self.sample_queues(map);
        self.compute_poses(map);
        self.compute_tv_blocks(map, inp.tracked, inp.hazards);
        self.decide(map, inp);
        self.resolve_blocks(map, inp);
        self.lane_changes(map, inp);
        self.drive_physical(map, inp);
        self.integrate(map, inp, dt);
        self.update_signals(map);
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

    /// Rebuild the junction reservations from what the agents hold now: a connector is reserved
    /// by the car driving through it, or (taking precedence) by the car holding a permit for it.
    /// Done every step so a reservation can never outlive its holder (owner row 85: reservations
    /// left behind by despawned, re-routed or turned-around cars held crossing traffic for
    /// minutes).
    fn rebuild_reservations(&mut self, map: &CityMap) {
        self.reservations.fill(NONE);
        for a in &self.agents {
            if a.despawn {
                continue;
            }
            if map.seg(a.seg).is_connector() {
                self.reservations[a.seg as usize] = a.id;
            }
        }
        for a in &self.agents {
            if !a.despawn && a.permit != NONE {
                self.reservations[a.permit as usize] = a.id;
            }
        }
        self.book_for_waiting(map);
    }

    /// Turn-taking for drivers who have waited [`BOOK_AFTER_S`] at their stop line (a minor
    /// road onto a busy arterial, a box that never empties): the longest-waiting driver books its
    /// connector, so nobody gets a permit into a movement crossing it until it has gone. A
    /// booking is never granted next to a crossing booking (no deadlock between bookings), and
    /// only while there is room on the exit (owner row 85).
    fn book_for_waiting(&mut self, map: &CityMap) {
        let now = self.step_count;
        let mut cands: Vec<(f32, u32, usize, bool, bool)> = Vec::new();
        for (i, a) in self.agents.iter().enumerate() {
            if a.despawn
                || a.permit != NONE
                || a.v >= 0.5
                || a.jam_s < BOOK_AFTER_S
                || now < a.book_block_until
                || now < a.permit_block_until
            {
                continue;
            }
            let sg = map.seg(a.seg);
            let c = a.plan[0];
            if sg.is_connector() || c == NONE || !map.seg(c).is_connector() {
                continue;
            }
            // Its own path must be free of tracked vehicles: a booking it cannot use would only
            // hold the crossing traffic.
            if self.tv_block[c as usize] {
                continue;
            }
            if sg.length - a.s > CAR_LENGTH_M + 4.0 {
                continue;
            }
            // Not against its own red / amber: the booking would lapse before the green (and
            // meanwhile hold the crossing traffic that has it).
            let red = sg
                .lane()
                .and_then(|l| map.lane_signal(l))
                .is_some_and(|(st, _)| matches!(st, SignalState::Red | SignalState::Yellow));
            if red {
                continue;
            }
            // No room on the exit: still book (the room that comes free is then its, not the
            // other approaches'), unless a wreck holds the exit.
            let tgt = map.seg(c).next[0];
            let room = !self.lists[tgt as usize]
                .first()
                .is_some_and(|e| e.v < 3.0 && e.s - HALF_L < CAR_LENGTH_M + 2.0);
            if !room && self.exit_wrecked(map, c) {
                continue;
            }
            // Waiting for room behind a red-light queue always ends; only other holds count
            // against `BOOK_ROOM_MAX_S`.
            let exit_red = map
                .seg(tgt)
                .lane()
                .and_then(|l| map.lane_signal(l))
                .is_some_and(|(st, _)| matches!(st, SignalState::Red | SignalState::Yellow));
            cands.push((a.jam_s, a.id, i, room, exit_red));
        }
        if cands.is_empty() {
            return;
        }
        cands.sort_by(|x, y| y.0.total_cmp(&x.0).then(x.1.cmp(&y.1)));
        let mut booked: Vec<SegId> = Vec::new();
        for (_, id, i, room, exit_red) in cands {
            let c = self.agents[i].plan[0];
            if self.reservations[c as usize] != NONE {
                continue;
            }
            let cs = map.seg(c);
            if cs.conflicts.iter().any(|x| booked.contains(x)) {
                continue;
            }
            self.reservations[c as usize] = id;
            booked.push(c);
            // A booking not turned into a permit within `BOOK_MAX_S` of room on the exit (or
            // `BOOK_ROOM_MAX_S` waiting for room) lapses (something else holds the car), with no
            // new booking for `BOOK_RETRY_S`.
            let a = &mut self.agents[i];
            if room {
                a.booked_s += TRAFFIC_DT;
            } else if !exit_red {
                a.book_room_s += TRAFFIC_DT;
            }
            if a.booked_s > BOOK_MAX_S || a.book_room_s > BOOK_ROOM_MAX_S {
                a.booked_s = 0.0;
                a.book_room_s = 0.0;
                a.book_block_until = now + (BOOK_RETRY_S / TRAFFIC_DT) as u64;
            }
        }
    }

    /// Update the view-only indicator state (`Agent::turn_latch`, `Agent::lc_signal`) after
    /// the agents moved. Never feeds back into the driving decisions.
    fn update_signals(&mut self, map: &CityMap) {
        for a in &mut self.agents {
            let seg = map.seg(a.seg);
            if seg.is_connector() {
                // Kept while in the junction (the view reads the connector's own turn).
            } else if a.plan[0] != NONE && map.seg(a.plan[0]).is_connector() {
                if a.turn_latch != a.plan[0] {
                    a.turn_latch = if seg.length - a.s < turn_signal_lead_m(a.v) {
                        a.plan[0]
                    } else {
                        NONE
                    };
                }
            } else {
                a.turn_latch = NONE;
            }
            if a.lc_signal != 0 && a.ghost.is_none() && a.lat.abs() < LANE_CHANGE_SIGNAL_OFF_M {
                a.lc_signal = 0;
            }
        }
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
        self.populating = true;
        for _ in 0..40 {
            self.rebuild_lists(map);
            self.compute_poses(map);
            let before = self.agents.len();
            self.spawn_update(map, inp, 200);
            self.remove_despawned();
            if self.agents.len() == before {
                break;
            }
        }
        self.populating = false;
    }

    fn rebuild_lists(&mut self, map: &CityMap) {
        for &s in &self.touched {
            self.lists[s as usize].clear();
        }
        self.touched.clear();
        // Manoeuvring agents also occupy the lane they intrude into (oncoming lane while
        // passing, the far side while turning around): refresh where.
        self.update_intrusions(map);
        for (i, opp, s) in self.stray_intrusions(map) {
            let l = &mut self.lists[opp as usize];
            if l.is_empty() {
                self.touched.push(opp);
            }
            // The body, and a hold point further up the oncoming lane: traffic arriving later
            // stops there, leaving the room a turn-around out of the spot needs.
            let hold = s - blocked::STRAY_HOLD_BACK_M;
            for es in [Some(s), (hold > 0.0).then_some(hold)]
                .into_iter()
                .flatten()
            {
                l.push(Entry {
                    s: es,
                    v: -self.agents[i].v,
                    agent: i as u32,
                    ghost: true,
                });
            }
        }
        for (i, a) in self.agents.iter().enumerate() {
            let l = &mut self.lists[a.seg as usize];
            if l.is_empty() {
                self.touched.push(a.seg);
            }
            l.push(Entry {
                s: a.s,
                v: if a.reversing() { -a.v } else { a.v },
                agent: i as u32,
                ghost: false,
            });
            for (seg, s, v) in a.intrusions() {
                let l = &mut self.lists[seg as usize];
                if l.is_empty() {
                    self.touched.push(seg);
                }
                l.push(Entry {
                    s,
                    v,
                    agent: i as u32,
                    ghost: true,
                });
            }
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
                vel: geom::scale(f, if a.reversing() { -a.v } else { a.v }),
            });
        }
    }

    /// Mark junction connectors occupied by tracked vehicles or debris (nobody may enter them).
    fn compute_tv_blocks(&mut self, map: &CityMap, tracked: &[TrackedVehicle], hazards: &[Hazard]) {
        for &s in &self.tv_block_list {
            self.tv_block[s as usize] = false;
            self.tv_block_moving[s as usize] = false;
        }
        self.tv_block_list.clear();
        let obstacles = tracked
            .iter()
            .map(|t| {
                let moving =
                    t.vel[0] * t.vel[0] + t.vel[1] * t.vel[1] > TV_SWEEP_MPS * TV_SWEEP_MPS;
                (geom::xy(t.pos), 3.0, moving)
            })
            .chain(
                hazards
                    .iter()
                    .map(|h| (geom::xy(h.pos), h.radius_m + CAR_WIDTH_M * 0.5 + 0.2, false)),
            );
        // A car reversing out of a blocked lane holds the connector it backs through.
        for a in &self.agents {
            if let Some(c) = a.manoeuvre.as_deref().and_then(|m| m.holds_connector()) {
                if !self.tv_block[c as usize] {
                    self.tv_block[c as usize] = true;
                    self.tv_block_list.push(c);
                }
                self.tv_block_moving[c as usize] = true;
            }
        }
        for (q, reach, moving) in obstacles {
            if map.intersection_at(q).is_none() {
                continue;
            }
            for &sid in map.segs_near(q) {
                let sg = map.seg(sid);
                if !sg.is_connector() || sg.project(q).dist >= reach {
                    continue;
                }
                if !self.tv_block[sid as usize] {
                    self.tv_block[sid as usize] = true;
                    self.tv_block_list.push(sid);
                }
                self.tv_block_moving[sid as usize] |= moving;
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
            if self.agents[i].manoeuvre.is_some() && self.sync_manoeuvre(map, i, q, st.speed_mps) {
                continue;
            }
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

    /// Nearest car ahead: `(gap, speed, agent index)`. Entries on the own segment at or before
    /// `skip_own` (a cluster being passed) are ignored.
    fn leader(&self, map: &CityMap, i: usize, skip_own: f32) -> (f32, f32, Option<u32>) {
        let a = &self.agents[i];
        let seg = map.seg(a.seg);
        let l = &self.lists[a.seg as usize];
        let mut best: Option<(f32, f32, u32)> = None;
        // A wreck shoved well off its lane (onto the sidewalk, a lot) is not in the way.
        let off_lane = |j: u32| {
            let o = &self.agents[j as usize];
            o.disabled && o.lat.abs() > OFF_LANE_WRECK_M
        };
        for e in &l[self.list_pos[i] as usize + 1..] {
            if e.agent as usize != i && e.s > skip_own && !off_lane(e.agent) {
                best = Some((e.s - a.s - CAR_LENGTH_M, e.v, e.agent));
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
                            best = Some((g, e.v, e.agent));
                        }
                    }
                }
            }
        }
        if let Some(b) = best {
            return (b.0, b.1, Some(b.2));
        }
        let mut dist = seg.length - a.s;
        for (k, &nx) in a.plan.iter().enumerate() {
            if nx == NONE || dist > 150.0 {
                break;
            }
            let ns = map.seg(nx);
            let mut cand: Option<(f32, f32, u32)> = self.lists[nx as usize]
                .iter()
                .find(|e| e.agent as usize != i && !off_lane(e.agent))
                .map(|e| (e.s, e.v, e.agent));
            if k == 0 && ns.is_connector() {
                for &(sib, div) in &ns.siblings {
                    if let Some(e) = self.lists[sib as usize].first() {
                        if e.s - HALF_L < div && cand.is_none_or(|c| e.s < c.0) {
                            cand = Some((e.s, e.v, e.agent));
                        }
                    }
                }
            }
            if let Some((s, v, j)) = cand {
                return (dist + s - CAR_LENGTH_M, v, Some(j));
            }
            dist += ns.length;
        }
        (f32::INFINITY, 0.0, None)
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
            // A car that has stood at its line without a permit for a while is held by
            // something else (a blocked path, a jam ahead): it is not about to come through,
            // and yielding to it would deadlock (owner row 85). Reservations keep the box
            // safe if it does get its permit.
            if a.permit != conn && a.v < 0.5 && a.jam_s > STALE_ARRIVAL_S {
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
            // First to stop but not going: held by something else (a blocked exit, a police
            // car in its path); waiting for it would deadlock the junction (owner row 85).
            if f.permit == NONE && f.jam_s > ALLWAY_STALE_S {
                continue;
            }
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
        if siren {
            return false;
        }
        if self.step_count < a.permit_block_until {
            return false;
        }
        if self.tv_block[conn as usize] {
            return false;
        }
        if !self.is_front(i) {
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
                    // A freeway on-ramp (minor approach at a radius-0 merge junction) merges on
                    // the move: it yields to the main line below but needs no stop first.
                    let merging = map.net.intersections[ap.intersection as usize].radius_m <= 0.0;
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
            let mut r = self.reservations[c as usize];
            // A wreck standing in a crossing path blocks only the paths it stands in (owner
            // row 85: a wrecked civilian in one arm held the whole junction).
            let wreck_clear = |e: &Entry| {
                let o = &self.agents[e.agent as usize];
                o.disabled && cs.project(geom::xy(self.poses[e.agent as usize].p)).dist > 3.0
            };
            let occupied = self.lists[c as usize].iter().any(|e| !wreck_clear(e));
            if r != NONE
                && !occupied
                && self.lists[c as usize]
                    .iter()
                    .any(|e| self.agents[e.agent as usize].id == r && wreck_clear(e))
            {
                r = NONE;
            }
            // A stationary tracked vehicle in a crossing path blocks only the cars whose own
            // path it stands in (`tv_block[conn]` above): a police car parked in one arm of the
            // junction must not hold every crossing movement (owner row 85).
            if occupied || (r != NONE && r != a.id) || self.tv_block_moving[c as usize] {
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
        // Yield to higher-ranked conflicting movements (not once it has booked its turn).
        let booked = self.reservations[conn as usize] == a.id;
        for &c in cs.conflicts.iter().filter(|_| !booked) {
            if self.rank_of(map, c) > my_rank && self.arrival_time(map, c) < p.gap_accept_s {
                return false;
            }
        }
        true
    }

    fn decide(&mut self, map: &CityMap, inp: &TrafficInputs) {
        // Siren reactions cost nothing while no siren is on.
        let any_siren = inp.tracked.iter().any(|t| t.siren);
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
                a.wait = Wait::Free;
                a.go_around = false;
                continue;
            }
            // Turning around / reversing: driven by the manoeuvre itself.
            if a.manoeuvre.is_some() && a.manoeuvre.as_deref().and_then(|m| m.pass()).is_none() {
                let siren = inp
                    .tracked
                    .iter()
                    .any(|t| t.siren && geom::dist(geom::xy(t.pos), q) < SIREN_RADIUS_M);
                let a = &mut self.agents[i];
                a.wait = Wait::Free;
                a.go_around = false;
                a.emergency = false;
                a.yielding = siren;
                continue;
            }
            let pass = a.manoeuvre.as_deref().and_then(|m| m.pass());

            // Desired speed.
            let sf = speed_factor_on(map, a.seg, &p);
            let mut v0 = (seg.speed_limit * sf).min(seg.curve_speed * sf);
            let d_end = seg.length - a.s;
            if a.plan[0] != NONE {
                let ns = map.seg(a.plan[0]);
                let vc = ns.curve_speed.min(ns.speed_limit) * speed_factor_on(map, a.plan[0], &p);
                v0 = v0.min((vc * vc + 2.0 * p.comfort_decel * (d_end - HALF_L).max(0.0)).sqrt());
            }
            if pass.is_some() {
                v0 = v0.min(blocked::PASS_MPS);
            }
            let mut lat_target = 0.0f32;
            let mut acc = f32::INFINITY;
            let mut emergency = false;
            // What caps the acceleration (for the blocked-vehicle logic).
            let mut bind = Bind::Line;
            let cap = |acc: &mut f32, bind: &mut Bind, x: f32, b: Bind| {
                if x < *acc {
                    *acc = x;
                    *bind = b;
                }
            };

            // Sirens ([`siren`]): pull over for a unit coming up behind, slow for one coming the
            // other way, keep out of a junction a unit is heading for.
            let mut sy = a.siren;
            let mut oncoming = false;
            let mut near = false;
            let mut jsiren = false;
            let mut siren_hold = a.siren_hold_s;
            // The unit this driver yields to has stopped behind it (waiting for room).
            let mut unit_waiting = false;
            let mut siren_lc = None;
            let mut yield_started = false;
            if any_siren || sy.phase != YieldPhase::None {
                // Nearest unit behind: (distance, lateral offset, speed). A driver not yet
                // yielding only reacts to a moving one.
                let mut behind: Option<(f32, f32, f32)> = None;
                for t in inp.tracked.iter().filter(|t| t.siren) {
                    match siren::classify(t, q, pose.p[2], pose.f) {
                        Some(
                            rel @ SirenRel::Behind {
                                back_m,
                                lat_m,
                                speed_mps,
                            },
                        ) => {
                            // A stopped unit level with or just behind this car is also one
                            // it is passing: slow past it (as for `Near`) while still
                            // reacting, instead of driving on at speed until the pull-over.
                            if speed_mps < siren::MOVING_MPS && back_m < siren::NEAR_M {
                                near = true;
                            }
                            let fresh = matches!(
                                sy.phase,
                                YieldPhase::None | YieldPhase::Noticing | YieldPhase::MergingBack
                            );
                            if (!fresh || siren::starts_yield(&rel))
                                && behind.is_none_or(|b| back_m < b.0)
                            {
                                behind = Some((back_m, lat_m, speed_mps));
                            }
                        }
                        Some(SirenRel::Oncoming { .. }) => oncoming = true,
                        Some(SirenRel::Near) => near = true,
                        None => {}
                    }
                }
                // Gave up on a unit standing behind: not again until it moves or goes.
                let ignored = sy.phase == YieldPhase::None
                    && sy.stall_s > siren::STALL_S
                    && behind.is_some_and(|b| b.2 < siren::MOVING_MPS);
                if ignored {
                    behind = None;
                    near = true;
                }
                let next = a.plan[0];
                // (Only cars near their stop line: the others are not about to enter.)
                if any_siren
                    && !seg.is_connector()
                    && next != NONE
                    && map.seg(next).is_connector()
                    && seg.length - a.s < siren::JUNCTION_APPROACH_M
                {
                    let (lp, _) = seg.point_at(seg.length);
                    jsiren = inp
                        .tracked
                        .iter()
                        .any(|t| siren::heads_for_junction(t, geom::xy(lp), lp[2]));
                }
                // Held at the line by a unit that never comes through: go after a while.
                if jsiren && v < 0.5 {
                    siren_hold += TRAFFIC_DT;
                    if siren_hold > SIREN_JUNCTION_HOLD_MAX_S {
                        jsiren = false;
                    }
                } else if !jsiren {
                    siren_hold = 0.0;
                }
                let lane = seg.lane().map(|l| &map.net.lanes[l as usize]);
                let half_w = lane.map_or(1.75, |l| l.width_m * 0.5);
                let can_pull = !seg.is_connector() && pass.is_none();
                let kerb = siren::kerb_offset_m(half_w * 2.0);
                unit_waiting = behind.is_some_and(|b| b.2 < siren::MOVING_MPS);
                match behind {
                    Some((back_u, lat_u, speed_u)) => {
                        // Away from the unit: to the kerb unless it comes up on the right.
                        let side = siren::side_for(back_u, lat_u);
                        match sy.phase {
                            YieldPhase::None => {
                                sy = SirenYield {
                                    phase: YieldPhase::Noticing,
                                    timer_s: 0.0,
                                    side,
                                    stall_s: 0.0,
                                };
                            }
                            YieldPhase::Noticing => {
                                sy.timer_s += TRAFFIC_DT;
                                sy.side = side;
                                if sy.timer_s >= a.siren_reaction_s {
                                    sy.phase = YieldPhase::PullingOver;
                                    sy.timer_s = 0.0;
                                    yield_started = true;
                                }
                            }
                            YieldPhase::PullingOver => {
                                sy.side = side;
                                sy.timer_s = 0.0;
                                // Stopped half-way to the kerb (something ahead holds it, and
                                // a physical car only moves sideways while rolling) with the
                                // unit standing behind: each waits for the other, so give up
                                // after a while, as a car waiting at the kerb does.
                                sy.stall_s = if speed_u < siren::MOVING_MPS && v < 0.3 {
                                    sy.stall_s + TRAFFIC_DT
                                } else {
                                    0.0
                                };
                                if sy.stall_s > siren::STALL_S {
                                    sy.phase = YieldPhase::MergingBack;
                                }
                            }
                            // The unit turned out to come up on the other side: move over.
                            YieldPhase::Waiting if side != sy.side => {
                                sy.phase = YieldPhase::PullingOver;
                                sy.side = side;
                                sy.timer_s = 0.0;
                            }
                            YieldPhase::Waiting => {
                                sy.timer_s = 0.0;
                                // A unit that just stands there behind (it stopped for something
                                // else): give up on it after a while.
                                sy.stall_s = if speed_u < siren::MOVING_MPS {
                                    sy.stall_s + TRAFFIC_DT
                                } else {
                                    0.0
                                };
                                if sy.stall_s > siren::STALL_S {
                                    sy.phase = YieldPhase::MergingBack;
                                }
                            }
                            YieldPhase::MergingBack if sy.stall_s <= siren::STALL_S => {
                                sy.phase = YieldPhase::PullingOver;
                                sy.timer_s = 0.0;
                            }
                            YieldPhase::MergingBack => {}
                        }
                        // Multi-lane road, unit in this lane: change out of it if there is room
                        // (looked for from the moment the driver hears it, while gaps still
                        // exist; the lane-change safety check is unchanged).
                        let neighbour = lane.is_some_and(|l| {
                            [l.left, l.right].into_iter().flatten().any(|n| {
                                let nl = &map.net.lanes[n as usize];
                                nl.kind.drivable()
                                    && !inp.closed.get(n as usize).copied().unwrap_or(false)
                            })
                        });
                        if sy.phase != YieldPhase::None
                            && neighbour
                            && back_u < siren::SIDE_RANGE_M * 2.0
                            && lat_u.abs() < half_w
                        {
                            siren_lc = Some(lat_u <= 0.0);
                        }
                    }
                    None => match sy.phase {
                        YieldPhase::None if !ignored => sy.stall_s = 0.0,
                        YieldPhase::None => {}
                        YieldPhase::Noticing => sy = SirenYield::default(),
                        YieldPhase::PullingOver | YieldPhase::Waiting => {
                            // The unit has gone past (or switched off): wait, then merge back.
                            sy.phase = YieldPhase::Waiting;
                            sy.timer_s += TRAFFIC_DT;
                            if sy.timer_s >= siren::CLEAR_S {
                                sy.phase = YieldPhase::MergingBack;
                                sy.timer_s = 0.0;
                            }
                        }
                        YieldPhase::MergingBack => sy.timer_s += TRAFFIC_DT,
                    },
                }
                let max_decel = p.comfort_decel.clamp(2.5, 3.5);
                match sy.phase {
                    YieldPhase::PullingOver | YieldPhase::Waiting if can_pull => {
                        lat_target = sy.side * kerb;
                        // Physical cars only move sideways while rolling: crawl until within
                        // 3 cm. (At 0.1 m a car on a 3.5 m lane could stop 6-10 cm short of the
                        // kerb offset, its body inside the corridor of a unit riding the line
                        // at -1.55 m in the same lane, which then waited STALL_S behind it.)
                        let sideways = (a.lat - lat_target).abs() > 0.03;
                        let vt = if sideways {
                            siren::PULL_OVER_CRAWL_MPS
                        } else {
                            0.0
                        };
                        v0 = v0.min(siren::PULL_OVER_CRAWL_MPS);
                        acc = acc.min(siren::approach_speed(v, vt, max_decel, p.max_accel));
                        if sy.phase == YieldPhase::PullingOver && !sideways && v < 0.3 {
                            sy.phase = YieldPhase::Waiting;
                            sy.timer_s = 0.0;
                        }
                    }
                    YieldPhase::MergingBack if can_pull => {
                        // Back into the lane once nobody is closing in behind.
                        let own = &self.lists[a.seg as usize];
                        let pos = self.list_pos[i] as usize;
                        let closing = own[..pos.min(own.len())]
                            .iter()
                            .rev()
                            .find(|e| !e.ghost)
                            .is_some_and(|e| {
                                e.v > 1.0 && a.s - e.s - CAR_LENGTH_M < 4.0 + 1.5 * e.v
                            });
                        // Not while a unit stands beside it: moving back to the lane centre
                        // would swing into it (it gave up on a unit stopped alongside).
                        // `Some(pass)` for a unit beside the car: `pass` when every such unit
                        // stands with room to creep past it.
                        let beside = inp
                            .tracked
                            .iter()
                            .filter(|t| {
                                let r = geom::sub(geom::xy(t.pos), q);
                                let long = geom::dot(r, pose.f);
                                (t.pos[2] - pose.p[2]).abs() < 4.0
                                    && long > -CAR_LENGTH_M - 1.0
                                    && long < CAR_LENGTH_M + 1.0
                                    && geom::dot(r, right).abs() < CAR_WIDTH_M + 1.2
                            })
                            .map(|t| {
                                let lat_gap = geom::dot(geom::sub(geom::xy(t.pos), q), right).abs();
                                geom::len(geom::xy(t.vel)) < BESIDE_STANDING_MPS
                                    && lat_gap >= CAR_WIDTH_M + BESIDE_CLEAR_M
                            })
                            .reduce(|x, y| x && y);
                        let at_kerb = (a.lat - sy.side * kerb).abs() < 0.2;
                        if closing || (beside.is_some() && !(beside == Some(true) && at_kerb)) {
                            lat_target = sy.side * kerb;
                            acc = acc.min(siren::approach_speed(v, 0.0, max_decel, p.max_accel));
                        } else if beside.is_some() {
                            // A unit standing alongside (a scene): creep on along the kerb and
                            // merge back past it instead of waiting for it (owner row 85).
                            lat_target = sy.side * kerb;
                            v0 = v0.min(5.0);
                        }
                        let beside = beside.is_some();
                        v0 = v0.min(8.0);
                        if !closing && !beside && a.lat.abs() < 0.2 {
                            sy = SirenYield {
                                stall_s: sy.stall_s,
                                ..SirenYield::default()
                            };
                        }
                    }
                    YieldPhase::MergingBack => {
                        sy = SirenYield {
                            stall_s: sy.stall_s,
                            ..SirenYield::default()
                        };
                    }
                    _ => {}
                }
                // Slow past a stopped unit with its lights on.
                if near && !sy.active() {
                    v0 = v0.min(siren::NEAR_MPS);
                    acc = acc.min(siren::approach_speed(
                        v,
                        siren::NEAR_MPS,
                        max_decel,
                        p.max_accel,
                    ));
                }
                if oncoming && !sy.active() {
                    if can_pull {
                        lat_target = kerb;
                    }
                    v0 = v0.min(siren::ONCOMING_MPS);
                    acc = acc.min(siren::approach_speed(
                        v,
                        siren::ONCOMING_MPS,
                        max_decel,
                        p.max_accel,
                    ));
                }
            }
            let siren = sy.active() || oncoming || near || jsiren;

            // Car ahead (a cluster being passed is not a leader).
            let skip_own = pass.map_or(f32::NEG_INFINITY, |b| b.skip_until_s);
            let (mut gap, vl, lead) = self.leader(map, i, skip_own);
            if let Some(j) = lead {
                // A wreck: stop further back, with room to pull out around it.
                if self.agents[j as usize].disabled {
                    gap -= blocked::OBSTACLE_GAP_M;
                }
            }
            cap(
                &mut acc,
                &mut bind,
                idm(v, v0, gap, vl, &p),
                Bind::Leader(lead, gap),
            );
            // Mid lane change the car still occupies its old lane: respect that lane's leader.
            if let Some((gseg, gs)) = a.ghost {
                let lead = self.lists[gseg as usize]
                    .iter()
                    .find(|e| e.s > gs && e.agent as usize != i);
                if let Some(e) = lead {
                    let g = e.s - gs - CAR_LENGTH_M;
                    cap(
                        &mut acc,
                        &mut bind,
                        idm(v, v0, g, e.v, &p),
                        Bind::Leader(Some(e.agent), g),
                    );
                }
            }

            // Passing: keep clear of the obstruction until the body is beside it, and give way
            // to anything coming the other way in the oncoming lane.
            let mut pass_event = PassEvent::None;
            let mut pass_stalled = false;
            if let Some(b) = pass {
                if !b.returning {
                    lat_target = b.offset;
                    // Body not clear of the obstruction yet: aim a little wider and creep; stop
                    // short of it if the body (0.1 m margin, 1 m ahead) would touch it.
                    let body_at = |ahead: f32, m: f32| Obb {
                        c: geom::add(q, geom::scale(pose.f, ahead)),
                        f: pose.f,
                        hl: HALF_L + m,
                        hw: CAR_WIDTH_M * 0.5 + m,
                    };
                    if geom::obb_overlap(&body_at(1.5, 0.3), &b.obs_box) {
                        lat_target = b.offset - blocked::PASS_PULL_OUT_EXTRA_M;
                        let creep = idm(v, 1.5, f32::INFINITY, 0.0, &p);
                        cap(&mut acc, &mut bind, creep, Bind::Line);
                    }
                    if geom::obb_overlap(&body_at(1.0, 0.1), &b.obs_box) {
                        let g = b.obs_rear_s - a.s - HALF_L;
                        cap(
                            &mut acc,
                            &mut bind,
                            idm(v, v0, g + p.min_gap_m - 0.8, 0.0, &p),
                            Bind::Line,
                        );
                        if v < 0.2 {
                            pass_stalled = true;
                        }
                    }
                    if a.s >= b.end_s {
                        pass_event = PassEvent::Return;
                    }
                } else if a.lat.abs() < 0.3 {
                    pass_event = PassEvent::Finish;
                }
                if a.lat.abs() > 1.0 {
                    let onc = self.lists[b.opp as usize]
                        .iter()
                        .filter(|e| e.agent as usize != i && e.s < b.opp_s)
                        .max_by(|x, y| x.s.total_cmp(&y.s));
                    if let Some(e) = onc {
                        let g = b.opp_s - e.s - CAR_LENGTH_M;
                        cap(&mut acc, &mut bind, idm(v, v0, g, -e.v, &p), Bind::Line);
                        // Not alongside yet and someone is coming: go back behind it.
                        if !b.returning
                            && a.s + HALF_L < b.obs_rear_s + 0.5
                            && e.v > 0.5
                            && g < 30.0 + 2.0 * e.v
                        {
                            pass_event = PassEvent::Abort;
                        }
                    }
                }
            }

            // Tracked vehicles in the path, and time-to-collision reactions.
            for (k, t) in inp.tracked.iter().enumerate() {
                let r = geom::sub(geom::xy(t.pos), q);
                if (t.pos[2] - pose.p[2]).abs() > 4.0 || geom::dot(r, r) > 80.0 * 80.0 {
                    continue;
                }
                let long = geom::dot(r, pose.f);
                let latd = geom::dot(r, right);
                let tv = [t.vel[0], t.vel[1]];
                // Lateral reach of its body (a car parked across the lane reaches further).
                let tf = if geom::dot(t.forward, t.forward) > 0.25 {
                    geom::normalize(t.forward)
                } else {
                    pose.f
                };
                let ext = 2.25 * geom::dot(tf, right).abs() + 0.95 * geom::dot(tf, pose.f).abs();
                let is_obs = pass.is_some_and(|b| b.obs == ObsRef::Tracked(t.entity.0));
                if long > 0.0
                    && long < 80.0
                    && latd.abs() < CAR_WIDTH_M * 0.5 + ext + 0.3
                    && !is_obs
                {
                    let still = geom::dot(tv, tv) < 0.25;
                    let cls = if still {
                        self.classify_tracked(map, i, long, self.tracked_still_s(t.entity.0))
                    } else {
                        Wait::Queue
                    };
                    let extra = if cls == Wait::Obstructed {
                        blocked::OBSTACLE_GAP_M
                    } else {
                        0.0
                    };
                    let g = long - CAR_LENGTH_M - extra;
                    cap(
                        &mut acc,
                        &mut bind,
                        idm(v, v0, g, geom::dot(tv, pose.f), &p),
                        Bind::Tracked(k, cls, g),
                    );
                }
                let w = geom::sub(tv, pose.vel);
                let ww = geom::dot(w, w);
                // A siren unit coming past a car that has pulled over / slowed for it is expected.
                // Only a real collision course counts: at the closest approach the two bodies
                // overlap side to side (a car passing in the next lane or coming the other way
                // in the opposite lane is ~3.3 m over and is ignored; owner: traffic must not
                // "freak out and slam the brakes" when the player gets near).
                if ww > 25.0 && long > -2.0 && !(t.siren && (sy.active() || oncoming)) {
                    let ts = -geom::dot(r, w) / ww;
                    if ts > 0.0 && ts < EMERGENCY_HORIZON_S {
                        let m = geom::add(r, geom::scale(w, ts));
                        let side = geom::dot(m, right).abs();
                        let along = geom::dot(m, pose.f).abs();
                        if side < CAR_WIDTH_M * 0.5 + ext + EMERGENCY_SIDE_MARGIN_M
                            && along < CAR_LENGTH_M
                        {
                            emergency = true;
                        }
                    }
                }
            }

            // Debris: swerve within the lane (planned in the lane frame so it holds the
            // offset while passing), or stop if it blocks the lane.
            let lat_ref = pass.map_or(a.lat, |b| a.lat - b.offset);
            for h in inp.hazards {
                if pass.is_some_and(|b| match b.obs {
                    ObsRef::Hazard(c, _) => geom::dist(c, geom::xy(h.pos)) < 0.5,
                    _ => false,
                }) {
                    continue;
                }
                match hazard_effect(h, q, pose.f, lat_ref, v) {
                    Some(HazardEffect::Swerve(shift)) if !seg.is_connector() && pass.is_none() => {
                        if lat_target == 0.0 || shift.abs() > lat_target.abs() {
                            lat_target = shift;
                        }
                        v0 = v0.min(seg.speed_limit * 0.6);
                    }
                    Some(HazardEffect::Swerve(_)) => {
                        v0 = v0.min(seg.speed_limit * 0.6);
                    }
                    Some(HazardEffect::Blocked(gap)) => {
                        let g = gap - blocked::OBSTACLE_GAP_M;
                        cap(
                            &mut acc,
                            &mut bind,
                            idm(v, v0, g + p.min_gap_m, 0.0, &p),
                            Bind::Hazard(geom::xy(h.pos), h.radius_m, g),
                        );
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
                            cap(
                                &mut acc,
                                &mut bind,
                                idm(v, v0, gap + p.min_gap_m, 0.0, &p),
                                Bind::Line,
                            );
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
                    // A left-turner waiting at the line with its permit (the "sneaker") goes
                    // when the oncoming traffic stops at amber / red: standing still it "can
                    // stop" at any decel, so the stopping check below revoked the permit the
                    // step after it was given and it sat out the next cycle too (owner row 85,
                    // short greens on main's grid).
                    let sneaker = matches!(
                        map.seg(next).kind,
                        SegKind::Connector {
                            turn: TurnKind::Left,
                            ..
                        }
                    ) && d_line < 3.5
                        && v < 3.0;
                    let limit = match st {
                        _ if sneaker => 0.0,
                        Some(SignalState::Yellow) if p.yellow_runner => 0.0,
                        Some(SignalState::Yellow) if reaches => p.amber_decel,
                        Some(SignalState::Yellow) => p.comfort_decel,
                        Some(SignalState::Red) => 5.0,
                        _ => 0.0,
                    };
                    // `limit > 0`: at a green (or no signal) a stopped car keeps its permit
                    // (need = 0 would otherwise revoke it every step).
                    // A yielding driver keeps out of the junction while the unit is coming,
                    // but once the unit has stopped behind it (waiting for room: a car at the
                    // stop line cannot pull over) it goes when it may, to clear the way.
                    let hold = jsiren || (sy.active() && !unit_waiting);
                    // A permit is only good while it is this car's reservation and the car is
                    // first in its lane: a car that cut in ahead takes the same connector, and
                    // leaving it clears the reservation, so the permit behind would be stale
                    // and conflicting movements would see the junction free (a left-turner
                    // and an oncoming car met in the box, grid 1+1 blocked-lane test).
                    let stale = self.reservations[next as usize] != a.id || !self.is_front(i);
                    // Stopped well short of the line (queued behind someone): a permit it cannot
                    // use only holds the crossing traffic.
                    let parked = v < 0.5 && d_line > 6.0;
                    if stale
                        || parked
                        || (limit > 0.0 && need <= limit)
                        || (hold && need <= 5.0)
                        || (self.tv_block[next as usize] && need <= 8.0)
                    {
                        revoke = true;
                    }
                } else if self.may_enter(
                    map,
                    i,
                    next,
                    d_line,
                    jsiren || (sy.active() && !unit_waiting),
                ) {
                    grant = Some(next);
                }
                let has = (a.permit == next && !revoke) || grant.is_some();
                if !has {
                    cap(
                        &mut acc,
                        &mut bind,
                        idm(v, v0, d_line.max(0.0) + p.min_gap_m, 0.0, &p),
                        Bind::Line,
                    );
                }
            } else if seg.is_connector() && next != NONE && a.plan[1] != NONE {
                // Approaching a short lane that ends at a junction: respect its stop line early.
                let ns = map.seg(next);
                if ns.length < 40.0 && map.seg(a.plan[1]).is_connector() {
                    let d_line = d_end + ns.length - HALF_L - 0.5;
                    cap(
                        &mut acc,
                        &mut bind,
                        idm(v, v0, d_line.max(0.0) + p.min_gap_m, 0.0, &p),
                        Bind::Line,
                    );
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
                        cap(&mut acc, &mut bind, -0.5, Bind::Line);
                    }
                }
            }

            if emergency {
                cap(&mut acc, &mut bind, -8.0, Bind::Line);
            }

            // Why the car is held up, and how long the obstruction has stood there.
            let (cls, still, obs, obs_gap) = match bind {
                Bind::Line => (Wait::Line, 0.0, ObsRef::None, f32::INFINITY),
                Bind::Leader(None, _) => (Wait::Free, 0.0, ObsRef::None, f32::INFINITY),
                Bind::Leader(Some(j), g) => {
                    let l = &self.agents[j as usize];
                    if l.disabled {
                        (Wait::Obstructed, l.stuck_s.max(5.0), ObsRef::Agent(j), g)
                    } else if l.manoeuvre.is_some() {
                        (Wait::Queue, 0.0, ObsRef::None, g)
                    } else if l.wait == Wait::Obstructed {
                        (Wait::Obstructed, l.stuck_s, ObsRef::Agent(j), g)
                    } else if l.v < 0.5 {
                        (Wait::Queue, 0.0, ObsRef::None, g)
                    } else {
                        (Wait::Free, 0.0, ObsRef::None, g)
                    }
                }
                Bind::Tracked(k, w, g) => {
                    let e = inp.tracked[k].entity.0;
                    (w, self.tracked_still_s(e), ObsRef::Tracked(e), g)
                }
                Bind::Hazard(c, r, g) => (Wait::Obstructed, 1e3, ObsRef::Hazard(c, r), g),
            };
            // Held at the line because a tracked vehicle stands in its path through the
            // junction (a pursuit scene in the box), outside red: after a while that is an
            // obstruction, so the queue behind turns around / backs out and the lane is marked
            // blocked for the cars upstream (owner row 85).
            // Same for a wreck standing just past the junction on its exit (the box rule
            // holds it for room that never comes).
            let box_blocked = cls == Wait::Line
                && !seg.is_connector()
                && !siren
                && a.plan[0] != NONE
                && ((self.tv_block[a.plan[0] as usize]
                    && !self.tv_block_moving[a.plan[0] as usize])
                    || self.exit_wrecked(map, a.plan[0]))
                && seg.length - a.s < CAR_LENGTH_M + 4.0
                && a.jam_s > blocked::TRACKED_AT_LINE_S;
            let (cls, still) = if box_blocked {
                (Wait::Obstructed, a.jam_s)
            } else {
                (cls, still)
            };
            let leader_manoeuvring = matches!(bind, Bind::Leader(Some(j), _)
                if self.agents[j as usize].manoeuvre.is_some());

            let id = a.id;
            let old_permit = a.permit;
            let a = &mut self.agents[i];
            if emergency && !a.emergency {
                self.stats.emergency_brakes += 1;
            }
            a.emergency = emergency;
            a.yielding = siren;
            a.siren = sy;
            a.siren_hold_s = siren_hold;
            a.siren_lc = siren_lc;
            if yield_started {
                self.stats.siren_yields += 1;
            }
            a.start_wait_s = start_wait;
            a.acc = acc.clamp(-9.0, p.max_accel);
            a.lat_target = lat_target;
            a.wait = if v < 0.5 { cls } else { Wait::Free };
            a.blocked_s = if cls == Wait::Obstructed && v < 0.5 {
                a.blocked_s + TRAFFIC_DT
            } else if cls == Wait::Obstructed || leader_manoeuvring || v < 0.5 {
                // Held, not reset, while it stands: the class flickers for a step now and then
                // (a level-of-detail switch, a siren yield, the leader's class) and a reset
                // kept the driver from ever giving up on the obstruction (owner row 85).
                a.blocked_s
            } else {
                0.0
            };
            a.obstacle_still_s = if cls == Wait::Obstructed { still } else { 0.0 };
            a.obs = if cls == Wait::Obstructed {
                obs
            } else {
                ObsRef::None
            };
            a.go_around = cls == Wait::Obstructed && still >= a.patience_s && obs_gap < 60.0;
            a.retry_s = (a.retry_s - TRAFFIC_DT).max(0.0);
            if let Some(Manoeuvre::Pass(b)) = a.manoeuvre.as_deref_mut() {
                b.stall_s = if pass_stalled {
                    b.stall_s + TRAFFIC_DT
                } else {
                    0.0
                };
                if b.stall_s > blocked::PASS_STALL_S && pass_event == PassEvent::None {
                    pass_event = PassEvent::Abort;
                }
            }
            match pass_event {
                PassEvent::None => {}
                PassEvent::Return => {
                    if let Some(Manoeuvre::Pass(b)) = a.manoeuvre.as_deref_mut() {
                        b.returning = true;
                    }
                    a.lat_target = 0.0;
                }
                PassEvent::Finish => {
                    a.manoeuvre = None;
                    a.blocked_s = 0.0;
                    a.lat_target = 0.0;
                    self.stats.oncoming_passes += 1;
                }
                PassEvent::Abort => {
                    a.manoeuvre = None;
                    a.lat_target = 0.0;
                    a.retry_s = 3.0;
                    self.stats.pass_aborts += 1;
                }
            }
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
            if a.disabled || a.permit != NONE || a.ghost.is_some() || a.manoeuvre.is_some() {
                continue;
            }
            // Physical agents change lanes only to get around a stationary obstruction or out of
            // a siren unit's lane.
            if a.physical.is_some() && !a.go_around && a.siren_lc.is_none() {
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
            let debris = inp.hazards.iter().any(|h| {
                matches!(
                    hazard_effect(h, geom::xy(pose.p), pose.f, a.lat, a.v),
                    Some(HazardEffect::Blocked(_))
                )
            });
            // A stationary obstruction ahead (wreck, parked car) forces a change too: MOBIL with
            // the incentive threshold lowered, the safety criterion unchanged.
            // So does a siren unit coming up behind in this lane ([`siren`]).
            let blocked = debris || a.go_around || a.siren_lc.is_some();
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
            // Not before the whole body has left the junction box: a change started at the lane
            // start sweeps the tail across cars still turning through the junction.
            if blocked
                && (self.step_count < a.lc_block_until
                    || a.s < CAR_LENGTH_M
                    || d_end < 15.0
                    || (a.yielding && a.siren_lc.is_none()))
            {
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
                // Out of a siren unit's lane: only away from the unit.
                if a.siren_lc.is_some_and(|r| r != to_right) {
                    continue;
                }
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
                // The target lane must not be blocked too.
                if a.go_around {
                    let stopped_ahead = list_has_stopped_ahead(&self.lists[c as usize], s2);
                    let (cp, ct) = cs.point_at(s2);
                    if stopped_ahead || tracked_blocks(inp.tracked, geom::xy(cp), ct) {
                        continue;
                    }
                }
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
                if a.siren_lc.is_some() && !debris {
                    self.stats.siren_lane_changes += 1;
                } else if a.go_around && !debris {
                    self.stats.go_around_lane_changes += 1;
                }
                a.seg = c;
                a.s = s2;
                a.lat = if to_right { -w } else { w };
                a.ghost = Some((old_seg, old_s));
                a.plan = [plan0, plan1];
                a.last_lane_change = t;
                a.lc_signal = if to_right { 1 } else { -1 };
                a.stopped_since = None;
                a.came_from = NONE;
                a.blocked_s = 0.0;
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
        self.choose_next_avoiding(map, lane, closed, false)
    }

    /// [`Traffic::choose_next`]; with `avoid_tv_block`, connectors a tracked vehicle stands in
    /// are skipped too, and `NONE` is returned when nothing else is left.
    fn choose_next_avoiding(
        &mut self,
        map: &CityMap,
        lane: SegId,
        closed: &[bool],
        avoid_tv_block: bool,
    ) -> SegId {
        let seg = map.seg(lane);
        let mut total = 0.0f32;
        let mut cands: [(SegId, f32); 16] = [(NONE, 0.0); 16];
        let mut n = 0;
        for &c in &seg.next {
            let cs = map.seg(c);
            let to = cs.next[0];
            if !cs.drivable
                || closed.get(to as usize).copied().unwrap_or(false)
                || self.lane_blocked(to)
                || (avoid_tv_block && (self.tv_block[c as usize] || self.exit_wrecked(map, c)))
            {
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
            if avoid_tv_block {
                return NONE;
            }
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
            a.came_from = if map.seg(old).is_connector() {
                old
            } else {
                NONE
            };
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
                self.track_physical_ghost(map, i);
                self.update_stop_state(map, i, dt);
                continue;
            }
            if self.agents[i].manoeuvre.is_some() && self.step_manoeuvre_kinematic(map, inp, i, dt)
            {
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

    /// Physical agent mid lane change: advance its ghost in the old lane, drop it once clear.
    fn track_physical_ghost(&mut self, map: &CityMap, i: usize) {
        let a = &self.agents[i];
        let Some((gseg, _)) = a.ghost else { return };
        let (p, _) = self.pose_of(map, a);
        let gs = map.seg(gseg).project(geom::xy(p)).s;
        let a = &mut self.agents[i];
        if a.lat.abs() < 1.4 {
            a.ghost = None;
        } else {
            a.ghost = Some((gseg, gs));
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
        self.update_jam_time(map, i, dt);
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

    fn drive_physical(&mut self, map: &CityMap, inp: &TrafficInputs) {
        for i in 0..self.agents.len() {
            if self.agents[i].physical.is_none() {
                continue;
            }
            if self.agents[i].manoeuvre.is_some() && self.drive_manoeuvre(map, inp, i) {
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
        let a = &self.agents[i];
        let Some((def, tires)) = self.cars.get(traffic_model_index(a.id, self.cars.len())) else {
            return;
        };
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
            let seg = map.seg(a.seg);
            let pr = seg.project(geom::xy(st.pos));
            a.s = pr.s.clamp(0.0, seg.length);
            // Up to a lane width off: a car demoted mid lane change (3.7 m highway lanes on
            // main's grid viaduct) keeps its place and eases back like a kinematic lane change.
            let w = seg.width.max(1.5);
            a.lat = pr.lateral.clamp(-w, w);
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
            if self.lod_locked(i) {
                if self.agents[i].physical.is_some() {
                    physical += 1;
                }
                continue;
            }
            let a = &self.agents[i];
            if a.physical.is_some() {
                if a.disabled
                    && d > r + 100.0
                    && !self.visible(geom::xy(self.poses[i].p), inp.tracked)
                {
                    self.agents[i].despawn = true;
                    self.stats.despawned += 1;
                } else if (d > r + 20.0 && !a.disabled) || self.cars.is_empty() {
                    self.demote(map, i);
                } else {
                    physical += 1;
                }
            } else if d < r && !self.cars.is_empty() {
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
        let r = if self.populating {
            self.config.sight_radius_m.min(POPULATE_SIGHT_RADIUS_M)
        } else {
            self.config.sight_radius_m
        };
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

    /// Per-zone targets for the part of each zone a player cannot see (where spawning and
    /// despawning are allowed), given the full [`Traffic::zone_targets`] and the number of
    /// agents currently inside a player's sightline.
    ///
    /// Each zone keeps the share of its target that lies outside every sightline. The share
    /// in view can only fill as cars drive into it, so whatever the cars in view fall short of
    /// it is added to the spawnable shares *around the players* (within the active radius of a
    /// player, not of a race route's focus points) in proportion, at most
    /// [`MAX_SPAWNABLE_BOOST`]× — less at night ([`max_spawnable_boost`] of the zone's
    /// time-of-day factor, none in the small hours). The total therefore still adds up to the density target: with
    /// the 320 m sightline a whole downtown can be in view, and without this the city settled
    /// about a quarter below target (`world_traffic_soak_observer`). With no player in the
    /// inputs nothing is in view and the targets are returned unchanged.
    fn spawnable_targets(
        &self,
        map: &CityMap,
        inp: &TrafficInputs,
        full: &[f32],
        seen_agents: u32,
    ) -> Vec<f32> {
        if !inp.tracked.iter().any(|t| t.player) {
            return full.to_vec();
        }
        let players: Vec<V2> = inp
            .tracked
            .iter()
            .filter(|t| t.player)
            .map(|t| geom::xy(t.pos))
            .collect();
        let mut focus = players.clone();
        focus.extend_from_slice(inp.route_focus);
        let ar = self.config.active_radius_m;
        let mut out = vec![0.0f32; full.len()];
        // Spawnable share of each zone's target that lies around a player.
        let mut near = vec![0.0f32; full.len()];
        let mut in_view_target = 0.0f32;
        for (z, samples) in self.zone_samples.iter().enumerate() {
            if full[z] <= 0.0 {
                continue;
            }
            // Same sample weighting as `zone_targets`, split by visibility.
            let (mut w_all, mut w_open, mut w_near) = (0.0f32, 0.0f32, 0.0f32);
            for smp in samples {
                if inp.closed.get(smp.seg as usize).copied().unwrap_or(false) {
                    continue;
                }
                if !focus.iter().any(|f| geom::dist(*f, smp.pos) < ar) {
                    continue;
                }
                w_all += smp.weight_km;
                if !self.visible(smp.pos, inp.tracked) {
                    w_open += smp.weight_km;
                    if players.iter().any(|f| geom::dist(*f, smp.pos) < ar) {
                        w_near += smp.weight_km;
                    }
                }
            }
            if w_all <= 0.0 {
                out[z] = full[z];
                continue;
            }
            out[z] = full[z] * w_open / w_all;
            near[z] = full[z] * w_near / w_all;
            in_view_target += full[z] - out[z];
        }
        let shortfall = (in_view_target - seen_agents as f32).max(0.0);
        let near_total: f32 = near.iter().sum();
        if shortfall > 0.0 && near_total > 0.0 {
            // At most double the density where cars can appear, so a thin spawnable strip
            // (player near the map edge, most of the area in view) does not clump.
            let k = (1.0 + shortfall / near_total).min(MAX_SPAWNABLE_BOOST);
            for (z, (t, n)) in out.iter_mut().zip(&near).enumerate() {
                let kz = k.min(max_spawnable_boost(f_time(map.time, map.net.zones[z].kind)));
                *t += n * (kz - 1.0);
            }
        }
        out
    }

    // Index loops: agents are mutated in place while parallel arrays are read.
    #[allow(clippy::needless_range_loop)]
    fn spawn_update(&mut self, map: &CityMap, inp: &TrafficInputs, budget: usize) {
        let full_targets = self.zone_targets(map, inp);
        let mut focus: Vec<V2> = inp
            .tracked
            .iter()
            .filter(|t| t.player)
            .map(|t| geom::xy(t.pos))
            .collect();
        focus.extend_from_slice(inp.route_focus);
        let global = focus.is_empty();
        let ar = self.config.active_radius_m;

        // Agents inside a player's sightline can be neither spawned nor despawned, so the
        // controller below only counts (and steers) the hidden-from-view part of each zone.
        let mut counts = vec![0u32; full_targets.len()];
        let mut agent_zone: Vec<Option<ZoneId>> = Vec::with_capacity(self.agents.len());
        let mut agent_seen: Vec<bool> = Vec::with_capacity(self.agents.len());
        let mut seen_in_zones = 0u32;
        for i in 0..self.agents.len() {
            let q = geom::xy(self.poses[i].p);
            let z = map.zone_at(q);
            let seen = self.visible(q, inp.tracked);
            if let Some(z) = z {
                if seen {
                    seen_in_zones += 1;
                } else {
                    counts[z as usize] += 1;
                }
            }
            agent_zone.push(z);
            agent_seen.push(seen);
        }
        let targets = self.spawnable_targets(map, inp, &full_targets, seen_in_zones);
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
            if a.despawn || a.physical.is_some() || agent_seen[i] {
                continue;
            }
            let in_connector = map.seg(a.seg).is_connector() || a.permit != NONE;
            let outside = !global && !focus.iter().any(|f| geom::dist(*f, q) < ar + 60.0);
            let closed = inp.closed.get(a.seg as usize).copied().unwrap_or(false);
            // Stuck: stopped for the timeout, or standing still outside a red light (a jam, a
            // wreck, a car behind a parked police car) for `jam_recycle_s` (owner row 85).
            let timed_out = a.stuck_s > self.config.stuck_timeout_s;
            let jammed = a.jam_s > self.config.jam_recycle_s;
            let stuck = timed_out || jammed;
            let surplus = agent_zone[i].is_some_and(|z| {
                let z = z as usize;
                let (t, c) = (targets[z], counts[z] as f32);
                let k = kind_of(z);
                !removed_in_zone[z]
                    && ((kind_c[k] > kind_t[k] * 1.04 + 1.0 && c > t + 0.5) || c > t * 1.5 + 3.0)
            });
            // A car stuck inside a junction box goes too (it is what holds the box).
            if (outside || closed || stuck || surplus) && (!in_connector || stuck) {
                if timed_out {
                    self.stats.stuck_recycled += 1;
                } else if jammed {
                    self.stats.jam_recycled += 1;
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
            // Never into a queue: not behind a stopped car (owner row 85).
            if ahead.is_some_and(|e| e.v < jam::STOPPED_MPS * 2.0 && gap_a < SPAWN_QUEUE_LOOK_M) {
                continue;
            }
            // Per-lane density cap, scaled with the time of day like the zone targets.
            let f = f_time(map.time, map.net.zones[z].kind).max(0.2);
            let cap = (seg.length * f / SPAWN_LANE_SPACING_M).floor().max(1.0) as usize;
            if list.iter().filter(|e| !e.ghost).count() >= cap {
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
                jam_s: 0.0,
                box_s: 0.0,
                metric_stop_s: 0.0,
                man_still_s: 0.0,
                siren_hold_s: 0.0,
                lc_block_until: 0,
                permit_block_until: 0,
                booked_s: 0.0,
                book_block_until: 0,
                book_room_s: 0.0,
                emergency: false,
                yielding: false,
                disabled: false,
                variant,
                lod: Lod::Kinematic,
                physical: None,
                despawn: false,
                wait: Wait::Free,
                blocked_s: 0.0,
                obstacle_still_s: 0.0,
                obs: ObsRef::None,
                patience_s: blocked::patience(self.seed, id, personality),
                siren: SirenYield::default(),
                siren_reaction_s: siren::reaction_s(self.seed, id, personality),
                siren_lc: None,
                turn_latch: NONE,
                lc_signal: 0,
                retry_s: 0.0,
                go_around: false,
                manoeuvre: None,
                came_from: NONE,
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
        let step = self.step_count;
        for pair in &self.overlaps {
            if !now.contains(pair) {
                self.overlap_ended.insert(*pair, step);
            }
        }
        self.overlap_ended
            .retain(|_, ended| step - *ended <= RETOUCH_STEPS);
        for pair in &now {
            if !self.overlaps.contains(pair) && !self.overlap_ended.contains_key(pair) {
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

#[cfg(test)]
mod model_tests {
    use super::traffic_model_index;

    #[test]
    fn model_index_is_stable_in_range_and_spread() {
        assert_eq!(traffic_model_index(17, 0), 0);
        assert_eq!(traffic_model_index(17, 1), 0);
        let mut counts = [0u32; 7];
        for id in 1..=7000u32 {
            let m = traffic_model_index(id, 7);
            assert_eq!(m, traffic_model_index(id, 7));
            counts[m] += 1;
        }
        // Roughly uniform (1000 each).
        assert!(
            counts.iter().all(|c| (850..=1150).contains(c)),
            "{counts:?}"
        );
        // Pinned values: renderers may cache this table, so it must never change.
        assert_eq!(
            (1..=8)
                .map(|id| traffic_model_index(id, 7))
                .collect::<Vec<_>>(),
            PINNED.to_vec()
        );
    }

    const PINNED: [usize; 8] = [2, 4, 2, 6, 3, 3, 2, 4];
}
