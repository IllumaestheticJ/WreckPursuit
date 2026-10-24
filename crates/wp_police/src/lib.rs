//! # wp_police
//!
//! Heat/bounty, dispatch, day/night presence, patrol & pursuit AI, police tools and the captain
//! interface (owned by workstream 06).
//!
//! Design pillars: chases are lethal to both sides. Police drive at the car's limit and
//! escalate by wanted level (box-ins, PIT, rams, rolling blocks, junction blocks, spikes and
//! roadblocks ahead of the predicted path, backup converging on the predicted position, a
//! helicopter keeping visual); racers win by breaking line of sight, reaching a safehouse or
//! disabling pursuers. Day vs night differences are **tables**
//! (`data/police/*.toml`, keyed by `DayPhase`), never code branches.
//!
//! Entry point: [`PoliceSystem::step`], called once per physics tick by `wp_sim`.
//!
//! Module map:
//! - [`config`]: the TOML tables.
//! - [`graph`]: road graph, line of sight, sight distance, routing, the procedural test city.
//! - [`heat`]: heat, wanted levels, bounty, escape/capture/safehouse rules.
//! - [`dispatch`]: patrol pool, incidents, reinforcements, helicopter, AI captain.
//! - [`pursuit`]: tactics (shadow, box-in, PIT, rolling block, intercept, disengage) and driving.
//! - [`unit`]: unit state and the built-in kinematic driver.
//! - [`walls`]: static feelers against building footprints and tunnel walls.
//! - [`tools`]: roadblocks, spike strips, radar detector and scanner queries.
//! - [`captain`]: captain commands, validation and the captain view.
//! - [`race`]: race-specific police profiles (`data/police/races.toml`): doctrine and
//!   pre-scripted roadblocks, spike strips and stakeouts per race.
//! - [`radio`]: radio pacing (one transmission at a time, gaps by wanted level).
//! - [`chatter`]: unit-to-unit callouts with call signs and street-name places (row 79).
//! - [`events`]: re-exports of the contract `PoliceEvent` (`SimEvent::Police`).
//! - [`scenario`]: headless scripted scenarios on the procedural city.

#![warn(missing_docs)]

pub mod avoid;
pub mod captain;
pub mod chatter;
pub mod city;
pub mod config;
pub mod dispatch;
pub mod events;
pub mod graph;
pub mod heat;
pub mod pro;
pub mod pursuit;
pub mod race;
pub mod radio;
pub mod scenario;
pub mod stuck;
pub mod time;
pub mod tools;
pub mod unit;
pub mod walls;

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use wp_contracts::{
    DamageMode, DayPhase, EntityId, HandlingEffects, PlayerId, SimEvent, VehicleInput, WorldQuery,
};
use wp_math::{Rng, StateHasher};

pub use avoid::ObstacleView;
pub use captain::{
    command_from_bytes, command_to_bytes, CaptainCommand, CaptainError, CaptainView, RoadEdgeId,
    UnitOrder,
};
pub use config::PoliceConfig;
pub use dispatch::{Incident, IncidentKind};
pub use events::{
    DisableCause, IncidentId, PoliceEvent, PoliceEventExt, PursuitOutcome, ToolId, UnitId, UnitKind,
};
pub use graph::{District, RoadGraph};
pub use heat::{BountyRecord, PursuitStatus, RacerRecord, WantedLevel};
pub use radio::{RadioConfig, RadioStats, RadioTier};
pub use time::SimTime;
pub use tools::{HeloView, RadarBlip, RadioLine, Roadblock, SpikeStrip, StaticCollider};
pub use unit::{Assignment, DriveCommand, RamKind, SteerGeometry, Tactic, Unit, UnitRole};

use graph::xy;
use tools::Puncture;
use unit::UnitBody;

/// Height of a driver's eyes / a car's roof for line-of-sight queries, m.
pub const EYE_HEIGHT_M: f32 = 1.3;

/// Eye (or sightline target) height for a vehicle whose chassis is at height `z`, m:
/// [`EYE_HEIGHT_M`] on flat ground, following the car up on terrain.
pub(crate) fn eye_height_at(z: f32) -> f32 {
    EYE_HEIGHT_M + (z - 1.0).max(0.0)
}

/// What the police system sees of a racer each tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RacerView {
    /// Racer's vehicle.
    pub entity: EntityId,
    /// World position, m.
    pub pos: [f32; 3],
    /// World velocity, m/s.
    pub vel: [f32; 3],
}

/// Instance-level options.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PoliceSetup {
    /// Seed for the police RNG stream.
    pub seed: u64,
    /// Damage mode (PIT speed limits, crash severity).
    pub mode: DamageMode,
    /// Drive AI units with the built-in kinematic driver (true until `wp_sim` gives units
    /// physical bodies and calls [`PoliceSystem::sync_unit`]).
    pub internal_driving: bool,
    /// Simulate background traffic hazards for internally driven units: a statistical
    /// stand-in (random near misses and collisions) for headless runs with **no real
    /// traffic**. Off by default; leave it off when `wp_world` traffic is simulated and
    /// reported with [`PoliceSystem::set_obstacles`], or units pay for traffic twice.
    pub hazards: bool,
}

impl Default for PoliceSetup {
    fn default() -> Self {
        Self {
            seed: 0x0090_11CE,
            mode: DamageMode::Arcade,
            internal_driving: true,
            hazards: false,
        }
    }
}

/// External body update for a unit driven by `wp_sim` (player or physical AI car).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitSync {
    /// World position, m.
    pub pos: [f32; 3],
    /// World velocity, m/s.
    pub vel: [f32; 3],
    /// Heading (XY unit vector).
    pub heading: [f32; 2],
    /// Vehicle health 0..=1.
    pub health: f32,
    /// Current handling effects (a cop with a bent wheel pulls over).
    pub effects: HandlingEffects,
}

/// One pursuit, for statistics.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PursuitRecord {
    /// Racer.
    pub racer: EntityId,
    /// Start.
    pub start: SimTime,
    /// End.
    pub end: Option<SimTime>,
    /// Outcome.
    pub outcome: Option<PursuitOutcome>,
}

/// One reckless-driving call (a violation no unit saw that opened a report), for statistics.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CallRecord {
    /// Reported racer.
    pub racer: EntityId,
    /// When the call came in.
    pub at: SimTime,
    /// Where (the violation), m.
    pub pos: [f32; 2],
    /// A responder was spawned for it (no available unit within `response.radius_m`).
    pub spawned: bool,
}

/// An active unit standing still (row 86 audit, `PoliceStats::active_stops`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ActiveStop {
    /// Unit.
    pub unit: UnitId,
    /// When it stopped.
    pub at: SimTime,
    /// How long it stood, s.
    pub stood_s: f32,
    /// Where, m.
    pub pos: [f32; 2],
    /// What held it when it stopped (diagnosis): its `drive_input` branch, and what capped its
    /// speed (0 nothing, 1 a vehicle, 2 a police unit, 3 kinematic traffic, 4 its racer).
    pub branch: u8,
    /// See `branch`.
    pub blocked_by: u8,
}

/// One ground unit appearing after the session start, for statistics: how close to (and
/// whether in sight of) a racer it popped in.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpawnRecord {
    /// Unit.
    pub unit: UnitId,
    /// Why it exists.
    pub role: UnitRole,
    /// When.
    pub at: SimTime,
    /// Where, m.
    pub pos: [f32; 2],
    /// Straight-line distance to the nearest racer, m (`f32::MAX` with no racers).
    pub nearest_racer_m: f32,
    /// Some racer had line of sight to it.
    pub in_sight: bool,
    /// Seconds after it appeared that some racer first had line of sight to the spot it
    /// appeared at, within [`dispatch::ROUTE_VIEW_S`] (row 131's pop-in audit: hidden when it
    /// appeared, in plain view a moment later). `None`: not seen in that window.
    #[serde(default)]
    pub seen_after_s: Option<f32>,
    /// It appeared on the far carriageway of a pursued racer's divided road (row 140 audit).
    #[serde(default)]
    pub far_carriageway: bool,
}

/// How a unit's trip to a reckless-driving call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResponseOutcome {
    /// Got within 100 m of the racer.
    Arrived,
    /// Fell behind the racer (it turned away); the call went to another unit.
    FellBehind,
    /// Stuck for good and despawned out of sight; the call went to another unit.
    Stuck(stuck::StuckCause),
    /// Wrecked or out of service on the way.
    Wrecked,
    /// The call ended first (a pursuit started, or it went stale).
    CallEnded,
}

/// One unit's trip to a reckless-driving call (`PoliceStats::responses`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ResponseRecord {
    /// Unit.
    pub unit: UnitId,
    /// Call.
    pub incident: IncidentId,
    /// Reported racer.
    pub racer: EntityId,
    /// Spawned for this call (else an existing unit).
    pub spawned: bool,
    /// When it took the call.
    pub at: SimTime,
    /// Straight-line distance to the racer then, m.
    pub start_m: f32,
    /// Legal driving distance to the racer then, m.
    pub route_m: f32,
    /// Stuck events on the way.
    pub stuck_events: u32,
    /// When it ended, and how (`None`: still on its way).
    pub end: Option<(SimTime, ResponseOutcome)>,
}

/// One physical unit's contact episode with static geometry (`PoliceStats::static_hits`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StaticHit {
    /// Unit.
    pub unit: UnitId,
    /// When the episode began.
    pub at: SimTime,
    /// Contact point, m.
    pub pos: [f32; 2],
    /// What it was: `Wall` (building, barrier, rail), `ParkedProp`, `Kerb` or `JunctionStep`.
    pub kind: stuck::StuckCause,
    /// Unit speed along its heading at the contact, m/s (negative: backing into it).
    pub speed_mps: f32,
    /// The unit was pursuing or intercepting a racer.
    pub pursuing: bool,
    /// The contact is on a building footprint or tunnel wall ([`walls::WallIndex`], within
    /// 1.5 m): what row 66 counts. Other `Wall` contacts are steps, kerbs and rails.
    #[serde(default)]
    pub building: bool,
    /// The unit was turning round (U-turn or reverse swing) or backing out.
    #[serde(default)]
    pub turning: bool,
}

/// Counters for tests, tuning and the README perf/behaviour tables.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PoliceStats {
    /// Every pursuit.
    pub pursuits: Vec<PursuitRecord>,
    /// Reinforcement request → unit committed, s.
    pub reinforcement_delays_s: Vec<f32>,
    /// Per pursuit: pursuit start → the first backup unit committed, s.
    #[serde(default)]
    pub first_backup_delays_s: Vec<f32>,
    /// Units that ever pursued.
    pub units_engaged: u32,
    /// Units total-lost without racer contact.
    pub self_wrecks: u32,
    /// Units total-lost after racer contact.
    pub contact_wrecks: u32,
    /// Units that pulled over (damage / bent wheel).
    pub pulled_over: u32,
    /// Approach sight distance of every tool placed, m.
    pub tool_sight_m: Vec<f32>,
    /// Roadblocks placed.
    pub roadblocks: u32,
    /// Spike strips placed.
    pub spikes: u32,
    /// Spike hits.
    pub spike_hits: u32,
    /// PIT attempts.
    pub pit_attempts: u32,
    /// Helicopter launches.
    pub helo_launches: u32,
    /// Helicopter requests that found it unavailable.
    pub helo_unavailable: u32,
    /// Racer–unit contacts.
    pub racer_contacts: u32,
    /// Racer–unit contacts hard enough to count as assaulting an officer (`heat.assault`).
    pub assaults: u32,
    /// Built-in driver: traffic near misses.
    pub near_misses: u32,
    /// Built-in driver: traffic collisions.
    pub traffic_collisions: u32,
    /// Built-in driver: ran wide and crashed.
    pub run_wide_crashes: u32,
    /// Built-in driver: near misses, traffic collisions and run-wide crashes by units while
    /// pursuing or intercepting (what the risk budget and pursuit caps govern).
    pub pursuit_harm: u32,
    /// Built-in driver: seconds pursuing / intercepting units spent cornering above
    /// `pursuit.risk.lateral_g`, summed over units.
    pub pursuit_high_g_s: f32,
    /// Seconds pursuing / intercepting units spent with their risk budget used up (held to
    /// shadowing and gentle cornering), summed over units.
    pub pursuit_no_risk_s: f32,
    /// Reckless-driving calls (reports opened).
    pub calls: Vec<CallRecord>,
    /// Calls taken off a unit that fell behind the racer and dispatched again.
    pub redispatches: u32,
    /// Blatant speeding right past a unit, recognised at once (`heat.blatant`).
    pub blatant: u32,
    /// Pursuits started by the unit that witnessed the violation making the racer wanted
    /// (row 80).
    #[serde(default)]
    pub witnessed_pursuits: u32,
    /// Speeding violations clocked by a unit's radar ([`config::RadarConfig`]).
    #[serde(default)]
    pub radar_clocks: u32,
    /// Resets in place (owner row 78): un-wrecked units that stood stuck and snapped to a clear
    /// spot in their lane ([`stuck::RESET_ACTIVE_S`]).
    #[serde(default)]
    pub resets: Vec<ResetRecord>,
    /// Turn-round reverse swings by physical units (target behind them).
    pub swings: u32,
    /// Chasing units that routed around a traffic jam they stood in (row 86).
    #[serde(default)]
    pub jam_reroutes: u32,
    /// Units that gave up turning round after [`stuck::MAX_TURNS`] tries on one spot: their
    /// call was handed back (siren off) and they despawn out of sight.
    #[serde(default)]
    pub turn_loops: u32,
    /// Units backing out of the way of a slow racer they weren't after
    /// ([`stuck::GIVE_WAY_S`]).
    #[serde(default)]
    pub give_ways: u32,
    /// Units that gave up, stood still [`stuck::DEADLOCK_VANISH_S`] next to a racer and were
    /// despawned in view to break the deadlock.
    #[serde(default)]
    pub deadlock_despawns: u32,
    /// Ground units spawned after the session start (pool refills, responders,
    /// reinforcements, tool units).
    pub spawns: Vec<SpawnRecord>,
    /// Physical units that got stuck, why, and what was done ([`stuck`]).
    pub stuck: Vec<stuck::StuckRecord>,
    /// Row 86 audit: an active unit (pursuit, intercept, response) standing still with no
    /// reason to (not arresting, not waiting in an ambush, not blocking a junction) for more
    /// than [`dispatch::ACTIVE_STOP_S`]: one entry per episode, when it started moving again
    /// or its job ended.
    #[serde(default)]
    pub active_stops: Vec<ActiveStop>,
    /// Physical units touching static geometry (walls, buildings, rails, parked props): one
    /// entry per contact episode (owner playtest row 66).
    #[serde(default)]
    pub static_hits: Vec<StaticHit>,
    /// Push-throughs: a siren unit blocked by a civilian or parked car shoving it aside at low
    /// speed (owner playtest row 61).
    #[serde(default)]
    pub pushes: u32,
    /// Idle / patrolling units that joined a pursuit as backup at once (owner playtest row 64).
    #[serde(default)]
    pub backup_joins: u32,
    /// Deliberate rams by pursuing units (rear quarter, side slam, hard; row 63).
    #[serde(default)]
    pub rams: u32,
    /// Interceptors that parked across a junction ahead of the racer (L3+, row 63).
    #[serde(default)]
    pub junction_blocks: u32,
    /// Every unit sent to a reckless-driving call: how far, how long, how it ended.
    pub responses: Vec<ResponseRecord>,
}

/// One reset in place ([`PoliceStats::resets`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ResetRecord {
    /// Unit.
    pub unit: UnitId,
    /// When.
    pub at: SimTime,
    /// Where it stood, m.
    pub from: [f32; 2],
    /// Where it was placed, m.
    pub to: [f32; 2],
    /// On a pursuit or search at the time.
    pub active: bool,
    /// A racer could see it (the reset waited [`stuck::RESET_VIEW_DELAY_S`]).
    pub in_view: bool,
}

/// A unit placed somewhere new by the police system (reset in place, row 78). `wp_sim` moves
/// the unit's car there, upright, and makes it a ghost for `ghost_s`
/// ([`PoliceSystem::take_unit_resets`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitReset {
    /// Unit.
    pub unit: UnitId,
    /// Its vehicle (`None`: driven headless, already moved).
    pub entity: Option<EntityId>,
    /// New position, m.
    pub pos: [f32; 2],
    /// New heading (XY unit vector).
    pub heading: [f32; 2],
    /// No-collision window, s.
    pub ghost_s: f32,
}

/// Captain bookkeeping (human or AI).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CaptainState {
    pub(crate) human: bool,
    pub(crate) priority: Option<EntityId>,
    pub(crate) roadblock_ready: SimTime,
    pub(crate) spikes_ready: SimTime,
    pub(crate) helo_ready: SimTime,
}

/// Per-think observation of a racer.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct RacerObs {
    /// Any unit / helicopter / camera has line of sight.
    pub(crate) seen: bool,
    /// A ground unit or camera within witness range has line of sight.
    pub(crate) witnessed: bool,
    /// Closest ground unit with line of sight within recognition range.
    pub(crate) spotter: Option<UnitId>,
    /// Its distance, m (recognition takes longer further out).
    pub(crate) spotter_m: f32,
    /// Closest ground unit with line of sight within witness range (it starts the pursuit when
    /// a violation it sees makes the racer wanted, row 80).
    pub(crate) witness: Option<UnitId>,
    /// Nearest ground unit distance, m.
    pub(crate) nearest_unit_m: f32,
    /// Nearest ground unit chasing this racer, m.
    pub(crate) nearest_chaser_m: f32,
}

/// Police state for one instance.
#[derive(Debug, Clone)]
pub struct PoliceSystem {
    pub(crate) cfg: PoliceConfig,
    /// Never changes after construction; shared so clones (crash-replay snapshots, race
    /// meet reports) stay cheap.
    pub(crate) graph: std::sync::Arc<RoadGraph>,
    pub(crate) setup: PoliceSetup,
    pub(crate) rng: Rng,
    pub(crate) tick: u64,
    pub(crate) now: SimTime,
    pub(crate) think_accum: f32,
    pub(crate) phase: DayPhase,
    pub(crate) warmed_up: bool,
    pub(crate) racers: BTreeMap<EntityId, RacerRecord>,
    pub(crate) obs: BTreeMap<EntityId, RacerObs>,
    pub(crate) recognition_s: BTreeMap<EntityId, f32>,
    /// Seconds each racer has been speeding in a unit's sight (radar hold).
    pub(crate) radar_over: BTreeMap<EntityId, f32>,
    /// When each racer last had a unit sent to wait ahead of it (see
    /// [`dispatch::AMBUSH_EVERY_S`]).
    pub(crate) last_ambush: BTreeMap<EntityId, SimTime>,
    /// The pursuit start each racer's first backup was logged for (see
    /// `PoliceStats::first_backup_delays_s`).
    pub(crate) first_backup: BTreeMap<EntityId, SimTime>,
    /// Units booking a racer they captured, until when (see `pursuit::ARREST_BUSY_S`).
    pub(crate) arresting: BTreeMap<UnitId, SimTime>,
    pub(crate) trails: BTreeMap<EntityId, unit::Track>,
    pub(crate) prev_pos: BTreeMap<EntityId, [f32; 2]>,
    /// Racer velocity reported on the previous step, m/s: how the racer came into this tick's
    /// contacts (by the time a contact is reported, the hit has already changed its velocity).
    pub(crate) prev_vel: BTreeMap<EntityId, [f32; 3]>,
    pub(crate) units: Vec<Unit>,
    pub(crate) next_unit: u32,
    pub(crate) incidents: Vec<Incident>,
    pub(crate) next_incident: u32,
    pub(crate) reinforcements: Vec<dispatch::ReinforcementRequest>,
    pub(crate) helo_requests: Vec<dispatch::HeloRequest>,
    /// Per tracking helicopter: whether it has visual on its target (radio bookkeeping).
    pub(crate) helo_sight: BTreeMap<UnitId, dispatch::HeloSight>,
    pub(crate) roadblocks: Vec<Roadblock>,
    pub(crate) spikes: Vec<SpikeStrip>,
    /// Tool cars waiting for their spot to leave every racer's view (row 131).
    pub(crate) tool_cars: Vec<dispatch::PendingToolCar>,
    /// Indices into `stats.spawns` still watched for being seen soon after (pop-in audit).
    pub(crate) spawn_watch: Vec<usize>,
    /// Elevated road decks (race stakeouts park on the deck, not under it).
    pub(crate) decks: Vec<race::DeckFoot>,
    pub(crate) next_tool: u32,
    pub(crate) punctures: BTreeMap<EntityId, Puncture>,
    pub(crate) radio: VecDeque<RadioLine>,
    pub(crate) radio_director: radio::RadioDirector,
    /// Whose radio this is ([`PoliceSystem::set_listeners`]): `None` = every racer's.
    pub(crate) radio_listeners: Option<BTreeSet<EntityId>>,
    /// Unit-to-unit callouts (row 79): observer state, never read by the simulation.
    pub(crate) chatter: chatter::Chatter,
    pub(crate) captain: CaptainState,
    pub(crate) stats: PoliceStats,
    pub(crate) contact_cooldown: BTreeMap<(UnitId, EntityId), SimTime>,
    /// Racer–unit contact episodes (assault detection).
    pub(crate) contact_episodes: BTreeMap<(UnitId, EntityId), dispatch::ContactEpisode>,
    pub(crate) pending: Vec<PoliceEvent>,
    /// Racers whose driver dropped out (disconnect grace, `wp_net`): their heat, wanted level
    /// and pursuit are frozen until released or captured ([`PoliceSystem::set_racer_held`]).
    pub(crate) held: BTreeSet<EntityId>,
    /// Vehicles reported by `wp_sim` ([`PoliceSystem::set_obstacles`]).
    pub(crate) ext_obstacles: Vec<avoid::Obstacle>,
    /// Whether `wp_sim` reports obstacles (then racers and bound units come from that list).
    pub(crate) ext_obstacles_set: bool,
    /// Everything units avoid this tick (rebuilt by `rebuild_obstacles`).
    pub(crate) obstacles: Vec<avoid::Obstacle>,
    /// The city's parked cars and cones (stuck classification).
    pub(crate) props: stuck::PropIndex,
    /// Stuck units back out, re-route and respawn (off: only logged, for measurements).
    pub(crate) unstick_recovery: bool,
    /// Building footprints and tunnel walls (static feelers, [`walls`]). Never changes after
    /// construction; shared so clones stay cheap.
    pub(crate) walls: std::sync::Arc<walls::WallIndex>,
    /// Static feelers on (default); off only for before/after measurements.
    pub(crate) feelers: bool,
    /// Covered roads (tunnels): a racer lost inside one is searched for at its portals.
    pub(crate) covers: Vec<walls::CoverSpan>,
    /// Resets in place not yet taken by `wp_sim` ([`PoliceSystem::take_unit_resets`]).
    pub(crate) resets: Vec<UnitReset>,
    /// Drivable room for the pro driver's racing line (lead P8).
    pub(crate) room: pro::RoadRoom,
    /// The race being policed ([`PoliceSystem::start_race`]), if any.
    pub(crate) race: Option<race::RacePolice>,
}

impl Default for PoliceSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl PoliceSystem {
    /// Police with the built-in tables and **no road network**: heat, wanted levels and the
    /// escape/capture rules work, but no units exist. This is what `wp_sim` uses until
    /// `wp_world` provides a road graph (see [`PoliceSystem::with_city`]).
    pub fn new() -> Self {
        Self::with_city(
            PoliceConfig::builtin(),
            RoadGraph::new(),
            PoliceSetup::default(),
        )
    }

    /// Police on `wp_world`'s city: the road graph is converted with
    /// [`RoadGraph::from_city`]. Pass the same `World` as `world` to [`PoliceSystem::step`] so
    /// line of sight uses its buildings.
    pub fn for_world(cfg: PoliceConfig, world: &wp_world::World, setup: PoliceSetup) -> Self {
        let mut p = Self::with_city(cfg, RoadGraph::from_city(world.map()), setup);
        chatter::attach_world(&mut p, world);
        p.set_decks(
            world
                .map()
                .decks()
                .iter()
                .map(|d| race::DeckFoot {
                    a: [d.a[0], d.a[1]],
                    b: [d.b[0], d.b[1]],
                    z: [d.a[2], d.b[2]],
                    left_m: d.left_m,
                    right_m: d.right_m,
                })
                .collect(),
        );
        p.set_props(
            world
                .static_props()
                .map(|s| {
                    let (sn, cs) = wp_math::sin_cos(s.yaw);
                    stuck::PropBox {
                        pos: [s.pos[0], s.pos[1]],
                        half: [s.half_extents[0], s.half_extents[1]],
                        rot: [sn, cs],
                    }
                })
                .collect(),
        );
        let mut segs = walls::WallIndex::from_outlines(
            world
                .map()
                .net
                .buildings
                .iter()
                .map(|b| b.footprint.as_slice()),
        );
        for c in &world.map().net.covered {
            let centre: Vec<[f32; 2]> = c.centre.iter().map(|q| [q[0], q[1]]).collect();
            segs.extend(walls::WallIndex::from_corridor(&centre, c.half_width_m));
            p.covers.push(walls::CoverSpan {
                centre,
                half_width_m: c.half_width_m,
            });
        }
        p.set_walls(walls::WallIndex::new(segs));
        // The real paved geometry: the road graph's polylines follow a lane.
        p.room = pro::RoadRoom::from_city(world.map(), &p.cfg.pursuit.pro);
        p
    }

    /// The city's solid outlines (building footprints, tunnel walls) that physical units feel
    /// for ([`walls`]). [`PoliceSystem::for_world`] sets them.
    pub fn set_walls(&mut self, walls: walls::WallIndex) {
        self.walls = std::sync::Arc::new(walls);
    }

    /// Covered road (tunnel) containing `p`, if any (index into `covers`).
    pub(crate) fn cover_at(&self, p: [f32; 2]) -> Option<usize> {
        self.covers.iter().position(|c| c.contains(p))
    }

    /// Switch the static feelers on (default) or off (before/after measurements).
    pub fn set_feelers(&mut self, on: bool) {
        self.feelers = on;
    }

    /// Switch stuck recovery (reverse, re-route, respawn) on (default) or off. Off, stuck units
    /// are still detected and logged in `stats().stuck`: for before/after measurements.
    pub fn set_unstick_recovery(&mut self, on: bool) {
        self.unstick_recovery = on;
    }

    /// The city's parked cars and cones (footprints), so stuck units can tell a prop from a
    /// wall ([`stuck`]). [`PoliceSystem::for_world`] sets them.
    pub fn set_props(&mut self, props: Vec<stuck::PropBox>) {
        self.props = stuck::PropIndex::new(props);
    }

    /// Police on a road network.
    pub fn with_city(cfg: PoliceConfig, graph: RoadGraph, setup: PoliceSetup) -> Self {
        let room = pro::RoadRoom::new(&graph, &cfg.pursuit.pro);
        Self {
            room,
            cfg,
            graph: std::sync::Arc::new(graph),
            setup,
            rng: Rng::new(setup.seed),
            tick: 0,
            now: SimTime::ZERO,
            think_accum: 0.0,
            phase: DayPhase::Day,
            warmed_up: false,
            racers: BTreeMap::new(),
            last_ambush: BTreeMap::new(),
            first_backup: BTreeMap::new(),
            arresting: BTreeMap::new(),
            obs: BTreeMap::new(),
            recognition_s: BTreeMap::new(),
            radar_over: BTreeMap::new(),
            trails: BTreeMap::new(),
            prev_pos: BTreeMap::new(),
            prev_vel: BTreeMap::new(),
            units: Vec::new(),
            next_unit: 1,
            incidents: Vec::new(),
            next_incident: 1,
            reinforcements: Vec::new(),
            helo_requests: Vec::new(),
            helo_sight: BTreeMap::new(),
            roadblocks: Vec::new(),
            tool_cars: Vec::new(),
            spawn_watch: Vec::new(),
            decks: Vec::new(),
            spikes: Vec::new(),
            next_tool: 1,
            punctures: BTreeMap::new(),
            radio: VecDeque::new(),
            radio_director: radio::RadioDirector::new(),
            radio_listeners: None,
            chatter: chatter::Chatter::new(setup.seed),
            captain: CaptainState {
                human: false,
                priority: None,
                roadblock_ready: SimTime::ZERO,
                spikes_ready: SimTime::ZERO,
                helo_ready: SimTime::ZERO,
            },
            stats: PoliceStats::default(),
            contact_cooldown: BTreeMap::new(),
            contact_episodes: BTreeMap::new(),
            pending: Vec::new(),
            held: BTreeSet::new(),
            ext_obstacles: Vec::new(),
            ext_obstacles_set: false,
            obstacles: Vec::new(),
            props: stuck::PropIndex::default(),
            unstick_recovery: true,
            walls: std::sync::Arc::new(walls::WallIndex::default()),
            feelers: true,
            covers: Vec::new(),
            resets: Vec::new(),
            race: None,
        }
    }

    /// Advance by `dt`: consume this tick's `events` (violations, contacts, total losses),
    /// update heat, dispatch and pursuits, push `SimEvent::PoliceEvent`s into `out`.
    ///
    /// Driving integrates every call; decisions (line of sight, rules, dispatch, tactics) run
    /// at `pursuit.think_hz`.
    pub fn step(
        &mut self,
        world: &dyn WorldQuery,
        racers: &[RacerView],
        events: &[SimEvent],
        dt: f32,
        out: &mut Vec<SimEvent>,
    ) {
        self.tick += 1;
        self.now = self.now.plus(dt);
        self.phase = world.time_of_day().phase();
        self.sync_racers(racers);
        self.process_events(world, events);
        self.rebuild_obstacles();
        self.drive_units(world, dt);
        self.think_accum += dt;
        let thought = self.think_accum >= 1.0 / self.cfg.pursuit.think_hz;
        if thought {
            let tdt = self.think_accum;
            self.think_accum = 0.0;
            self.think(world, tdt);
        }
        self.chatter_step(world, events, thought);
        self.flush_radio();
        for e in self.pending.drain(..) {
            out.push(SimEvent::Police(e));
        }
    }

    fn sync_racers(&mut self, racers: &[RacerView]) {
        let now = self.now;
        for r in racers {
            let rec = self
                .racers
                .entry(r.entity)
                .or_insert_with(|| RacerRecord::new(r.entity));
            self.prev_vel.insert(r.entity, rec.vel);
            rec.pos = r.pos;
            rec.vel = r.vel;
            let p = xy(r.pos);
            // Spike strips.
            if let Some(prev) = self.prev_pos.get(&r.entity).copied() {
                for s in &self.spikes {
                    let wheels = s.wheels_crossing(prev, p, 0.8);
                    if wheels.iter().any(|w| *w) {
                        self.punctures.entry(r.entity).or_default().hit(wheels, now);
                        self.stats.spike_hits += 1;
                        self.pending.push(PoliceEvent::SpikeHit {
                            vehicle: r.entity,
                            wheels,
                        });
                    }
                }
            }
            self.prev_pos.insert(r.entity, p);
        }
    }

    fn process_events(&mut self, world: &dyn WorldQuery, events: &[SimEvent]) {
        for ev in events {
            match ev {
                SimEvent::TrafficViolation { vehicle, kind, pos } => {
                    if self.racers.contains_key(vehicle) {
                        self.on_violation(world, *vehicle, kind, xy(*pos));
                    }
                }
                SimEvent::TotalLoss { vehicle } => {
                    if self.racers.contains_key(vehicle) {
                        self.on_racer_total_loss(*vehicle);
                    } else if let Some(i) = self.unit_index_by_entity(*vehicle) {
                        self.units[i].health = 0.0;
                    }
                }
                SimEvent::Contact {
                    a,
                    b,
                    point,
                    normal,
                    rel_speed,
                    impulse_ns,
                } => {
                    self.note_unit_contact(*a, *b, *point, *normal);
                    self.on_contact(world, *a, *b, *point, *rel_speed, *impulse_ns)
                }
                _ => {}
            }
        }
    }

    /// Line of sight between two ground points at eye height: the police graph's own occluders
    /// (procedural test city) and the world's buildings (`WorldQuery::line_of_sight`).
    pub(crate) fn sees(&self, world: &dyn WorldQuery, a: [f32; 2], b: [f32; 2]) -> bool {
        self.graph.line_of_sight(a, b)
            && world.line_of_sight([a[0], a[1], EYE_HEIGHT_M], [b[0], b[1], EYE_HEIGHT_M])
    }

    /// [`PoliceSystem::sees`] between two vehicles at their heights (`az`, `bz`: chassis
    /// z, m): eyes sit at [`EYE_HEIGHT_M`] on flat ground and follow the car up on terrain
    /// (Port Vesper v2's downtown is ~5 m above z = 0, where a fixed 1.3 m sightline runs
    /// through the ground). Identical to `sees` for cars below z = 1 m. (Fixed by 07.)
    pub(crate) fn sees_at(
        &self,
        world: &dyn WorldQuery,
        a: [f32; 2],
        az: f32,
        b: [f32; 2],
        bz: f32,
    ) -> bool {
        self.graph.line_of_sight(a, b)
            && world.line_of_sight(
                [a[0], a[1], eye_height_at(az)],
                [b[0], b[1], eye_height_at(bz)],
            )
    }

    pub(crate) fn unit_index(&self, id: UnitId) -> Option<usize> {
        self.units.binary_search_by_key(&id, |u| u.id).ok()
    }

    pub(crate) fn unit_index_by_entity(&self, e: EntityId) -> Option<usize> {
        self.units.iter().position(|u| u.entity == Some(e))
    }

    /// Raise a radio line. The radio director ([`radio`]) decides at the end of the step
    /// whether (and merged with which other lines) it is spoken.
    /// A line built without [`radio::RadioCue::about`] is attributed here
    /// ([`PoliceSystem::radio_subject`]).
    pub(crate) fn radio(&mut self, mut cue: radio::RadioCue) {
        if cue.racer.is_none() {
            cue.racer = self.radio_subject(&cue);
        }
        self.radio_director.raise(self.now, cue);
    }

    /// Whose radio this is: the racers (players) the police radio is played to. Lines about
    /// other racers are heard only when they happen near a listener, impacts only near one,
    /// the pacing follows the listeners' wanted level, and a single listener is called "the
    /// suspect" instead of "racer #N" (owner, 2026-10-23: no chatter about AI cars on the
    /// other side of the map). `None` (the default) = every racer listens: headless
    /// scenarios, tests. Presentation only: nothing here changes what the police do.
    pub fn set_listeners(&mut self, listeners: Option<&[EntityId]>) {
        self.radio_listeners = listeners.map(|l| l.iter().copied().collect());
    }

    /// Speak what the radio director lets through: event + scanner log.
    fn flush_radio(&mut self) {
        let listening = |r: &&RacerRecord| {
            self.radio_listeners
                .as_ref()
                .is_none_or(|l| l.contains(&r.entity))
        };
        let racers: Vec<[f32; 2]> = self
            .racers
            .values()
            .filter(listening)
            .map(|r| [r.pos[0], r.pos[1]])
            .collect();
        let level = self
            .racers
            .values()
            .filter(listening)
            .map(|r| r.level.number())
            .max()
            .unwrap_or(0);
        let state = self
            .racers
            .values()
            .filter(listening)
            .map(|r| match r.status {
                PursuitStatus::Pursuit => radio::ListenerState::Pursuit,
                PursuitStatus::Searching | PursuitStatus::Wanted => radio::ListenerState::Wanted,
                PursuitStatus::Clear => radio::ListenerState::Calm,
            })
            .max()
            .unwrap_or_default();
        let listeners: Option<Vec<EntityId>> = self
            .radio_listeners
            .as_ref()
            .map(|l| l.iter().copied().collect());
        let racer_records = &self.racers;
        let racer_pos = |e: EntityId| racer_records.get(&e).map(|r| [r.pos[0], r.pos[1]]);
        let units = &self.units;
        let unit_pos = |id: UnitId| {
            units
                .binary_search_by_key(&id, |u| u.id)
                .ok()
                .map(|i| units[i].body.pos)
        };
        let ctx = radio::RadioContext {
            now: self.now,
            level,
            state,
            racers: &racers,
            listeners: listeners.as_deref(),
            racer_pos: &racer_pos,
            unit_pos: &unit_pos,
        };
        for line in self.radio_director.flush(&self.cfg.radio, &ctx) {
            self.radio.push_back(RadioLine {
                time: self.now,
                unit: line.unit,
                text: line.text.clone(),
                tier: line.tier,
                duration_s: line.duration_s,
                cut_in: line.cut_in,
                racer: line.racer,
            });
            while self.radio.len() > self.cfg.tools.gadgets.radio_log_len {
                self.radio.pop_front();
            }
            self.pending.push(PoliceEvent::Radio {
                unit: line.unit,
                text: line.text,
            });
        }
    }

    /// Radio pacing counters (raised / spoken / collapsed / dropped lines).
    pub fn radio_stats(&self) -> radio::RadioStats {
        self.radio_director.stats()
    }

    /// Tier of the radio transmission on air now (`None`: the channel is free). The channel
    /// counts as busy for a line's estimated length plus the silence after it.
    pub fn radio_on_air(&self) -> Option<radio::RadioTier> {
        self.radio_director.on_air(self.now)
    }

    // -----------------------------------------------------------------------------------------
    // Public queries
    // -----------------------------------------------------------------------------------------

    /// Current wanted level of a racer.
    pub fn wanted_level(&self, racer: EntityId) -> WantedLevel {
        self.racers.get(&racer).map(|r| r.level).unwrap_or_default()
    }

    /// True when a racer is wanted (level above None) or in an active pursuit (including
    /// the search phase). Used by `wp_net` to decide whether a disconnect gets the capture
    /// grace.
    pub fn is_wanted_or_pursued(&self, racer: EntityId) -> bool {
        self.racers
            .get(&racer)
            .is_some_and(|r| r.level > WantedLevel::None || r.pursued())
    }

    /// Freeze (`held = true`) or release a racer's police state. While held, the racer's
    /// heat, wanted level, bounty and pursuit status do not change: no escape timer, no
    /// safehouse dwell, no capture rule, no heat decay, no new pursuit. Units keep driving.
    /// `wp_net` holds a wanted racer whose player disconnected, so a reconnect resumes the
    /// same pursuit and sitting out the escape timer offline is not an escape.
    pub fn set_racer_held(&mut self, racer: EntityId, held: bool) {
        if held {
            self.held.insert(racer);
        } else {
            self.held.remove(&racer);
        }
    }

    /// True while [`PoliceSystem::set_racer_held`] holds a racer.
    pub fn is_racer_held(&self, racer: EntityId) -> bool {
        self.held.contains(&racer)
    }

    /// End a wanted racer's pursuit as [`PursuitOutcome::Captured`] because its driver
    /// abandoned it (disconnect grace ran out, or the player quit). The capturer is the
    /// nearest in-service ground unit assigned to the racer, `None` if no unit was on it;
    /// the other assigned units are assists. Emits `PursuitEnded` and the level reset like a
    /// normal capture (drained on the next [`PoliceSystem::step`]) and releases the hold.
    /// Returns the wanted level at the capture and the capturer, or `None` (nothing done)
    /// when the racer is unknown or neither wanted nor pursued.
    pub fn capture_abandoned(&mut self, racer: EntityId) -> Option<(WantedLevel, Option<UnitId>)> {
        if !self.is_wanted_or_pursued(racer) {
            return None;
        }
        self.held.remove(&racer);
        let level = self.racers[&racer].level;
        let credit = self.pursuing_credit(racer);
        let capturer = credit.0;
        self.end_pursuit_credited(racer, PursuitOutcome::Captured, credit);
        Some((level, capturer))
    }

    /// Start a pursuit of a wanted racer right now by the nearest in-service ground unit
    /// (scenarios, measurement tools and debug: the normal path is recognition by a unit).
    /// Returns that unit, or `None` (nothing done) when the racer is unknown, not wanted,
    /// already pursued, or no ground unit is in service.
    pub fn begin_pursuit(&mut self, racer: EntityId) -> Option<UnitId> {
        let rec = self.racers.get(&racer)?;
        if rec.level == WantedLevel::None || rec.pursued() {
            return None;
        }
        let p = xy(rec.pos);
        let unit = self
            .units
            .iter()
            .filter(|u| {
                u.kind == UnitKind::Car
                    && !u.wrecked
                    && u.player.is_none()
                    && u.assignment != Assignment::OutOfService
            })
            .min_by(|a, b| {
                graph::dist2(a.body.pos, p)
                    .total_cmp(&graph::dist2(b.body.pos, p))
                    .then(a.id.cmp(&b.id))
            })?
            .id;
        self.start_pursuit(racer, unit);
        Some(unit)
    }

    /// Every racer's police record, ascending by entity.
    pub fn racers_iter(&self) -> impl Iterator<Item = &RacerRecord> {
        self.racers.values()
    }

    /// Full police record of a racer.
    pub fn racer(&self, racer: EntityId) -> Option<&RacerRecord> {
        self.racers.get(&racer)
    }

    /// All units, ascending by id.
    pub fn units(&self) -> &[Unit] {
        &self.units
    }

    /// Units reset in place since the last call (row 78), in reset order. `wp_sim` places each
    /// unit's car at the new pose and makes it a ghost for `ghost_s`.
    pub fn take_unit_resets(&mut self) -> Vec<UnitReset> {
        std::mem::take(&mut self.resets)
    }

    /// Open incidents.
    pub fn incidents(&self) -> &[Incident] {
        &self.incidents
    }

    /// The road graph in use.
    pub fn graph(&self) -> &RoadGraph {
        &self.graph
    }

    /// Tuning in use.
    pub fn config(&self) -> &PoliceConfig {
        &self.cfg
    }

    /// Statistics so far.
    pub fn stats(&self) -> &PoliceStats {
        &self.stats
    }

    /// Simulation time seen by the police.
    pub fn now(&self) -> SimTime {
        self.now
    }

    /// Current day phase (as of the last step).
    pub fn phase(&self) -> DayPhase {
        self.phase
    }

    /// Pool units alive per district (the patrol presence the table asks for).
    pub fn pool_count(&self, district: District) -> u32 {
        self.units
            .iter()
            .filter(|u| !u.wrecked && u.role == UnitRole::Pool(district))
            .count() as u32
    }

    /// Tire deflation for a vehicle (merge into `HandlingEffects::tire_puncture`).
    pub fn tire_puncture(&self, vehicle: EntityId) -> [f32; 4] {
        self.punctures
            .get(&vehicle)
            .map(|p| p.level(self.now, self.cfg.tools.spikes.deflate_s))
            .unwrap_or([0.0; 4])
    }

    /// Bounty of a racer.
    pub fn bounty(&self, racer: EntityId) -> f32 {
        self.racers
            .get(&racer)
            .map(|r| r.bounty.bounty(&self.cfg.heat))
            .unwrap_or(0.0)
    }

    /// Persistent bounty ledger (prompt 08 stores this).
    pub fn bounty_record(&self, racer: EntityId) -> BountyRecord {
        self.racers
            .get(&racer)
            .map(|r| r.bounty)
            .unwrap_or_default()
    }

    /// Restore a persisted bounty ledger (call before the racer's first step).
    pub fn set_bounty_record(&mut self, racer: EntityId, rec: BountyRecord) {
        self.racers
            .entry(racer)
            .or_insert_with(|| RacerRecord::new(racer))
            .bounty = rec;
    }

    /// Record a race won; counts toward bounty only while wanted.
    pub fn record_race_won(&mut self, racer: EntityId) {
        if let Some(r) = self.racers.get_mut(&racer) {
            if r.level > WantedLevel::None {
                r.bounty.races_won_wanted += 1;
            }
        }
    }

    /// Race payout / stake multiplier for the current phase (night pays more).
    pub fn payout_scale(&self) -> f32 {
        self.cfg.heat.bounty.payout_scale.get(self.phase)
    }

    /// Set a racer's heat directly (scenarios, debug, persistence).
    pub fn set_heat(&mut self, racer: EntityId, heat: f32) {
        let cfg = self.cfg.heat.clone();
        let r = self
            .racers
            .entry(racer)
            .or_insert_with(|| RacerRecord::new(racer));
        let before = r.level;
        r.heat = 0.0;
        r.add_heat(heat, &cfg);
        if r.status == PursuitStatus::Clear && r.level > WantedLevel::None {
            r.status = PursuitStatus::Wanted;
        }
        let after = r.level;
        // Listeners (economy, HUD) track the level from events, so report the change.
        if after != before {
            self.pending.push(PoliceEvent::WantedLevelChanged {
                racer,
                level: after,
            });
        }
    }

    /// Roadblocks in place.
    pub fn roadblocks(&self) -> &[Roadblock] {
        &self.roadblocks
    }

    /// Elevated road decks of the city ([`PoliceSystem::for_world`] sets them from the map):
    /// race stakeouts on a viaduct park on the deck, not on the ground under it.
    pub fn set_decks(&mut self, decks: Vec<race::DeckFoot>) {
        self.decks = decks;
    }

    /// Spots `(position, heading)` of tool `tool`'s cars not spawned yet. Roadblock cars wait
    /// while their spot is in a racer's view (row 131) and appear once it is hidden; a spike
    /// car is decided on the next think (hidden: it appears, in view: dropped, the strip stays
    /// unmanned; PR #54 port).
    pub fn tool_cars_waiting(&self, tool: ToolId) -> Vec<([f32; 2], [f32; 2])> {
        self.tool_cars
            .iter()
            .filter(|c| c.tool == tool)
            .map(|c| (c.pos, c.heading))
            .collect()
    }

    /// Spike strips in place (line colliders).
    pub fn spike_strips(&self) -> &[SpikeStrip] {
        &self.spikes
    }

    /// Static colliders for `wp_sim`: roadblock barriers plus parked roadblock units (when
    /// units are not physical bodies yet).
    pub fn static_colliders(&self) -> Vec<StaticCollider> {
        let mut v = Vec::new();
        for rb in &self.roadblocks {
            v.extend(rb.barriers.iter().copied());
            for uid in &rb.units {
                if let Some(i) = self.unit_index(*uid) {
                    let u = &self.units[i];
                    if u.entity.is_none() {
                        v.push(StaticCollider {
                            pos: [u.body.pos[0], u.body.pos[1], 0.7],
                            half_extents_m: [0.95, 2.4, 0.7],
                            forward: u.body.heading,
                            mass_kg: self.cfg.tools.roadblock.unit_mass_kg,
                        });
                    }
                }
            }
        }
        v
    }

    // -----------------------------------------------------------------------------------------
    // wp_sim integration for physical units
    // -----------------------------------------------------------------------------------------

    /// Report every vehicle units must not drive into: players, racers, traffic (physical and
    /// kinematic) and police cars, including the units' own bodies (each unit skips itself).
    /// Call once per tick before [`PoliceSystem::unit_input`]; the list stays until replaced.
    /// Without it, units avoid the racers and each other only.
    pub fn set_obstacles(&mut self, vehicles: &[ObstacleView]) {
        self.ext_obstacles.clear();
        self.ext_obstacles
            .extend(vehicles.iter().map(avoid::Obstacle::from_view));
        self.ext_obstacles_set = true;
        self.rebuild_obstacles();
    }

    /// Rebuild `obstacles`: `wp_sim`'s list plus internally driven units, or (headless) the
    /// racers and every ground unit.
    pub(crate) fn rebuild_obstacles(&mut self) {
        self.obstacles.clear();
        let ext = self.ext_obstacles_set;
        if ext {
            self.obstacles.extend_from_slice(&self.ext_obstacles);
            // Mark police cars (junction give-way breaks ties between units by id).
            for o in &mut self.obstacles {
                if let Some(u) = self
                    .units
                    .iter()
                    .find(|u| u.entity.is_some() && u.entity == o.entity)
                {
                    o.unit = Some(u.id);
                }
            }
        } else {
            for r in self.racers.values() {
                self.obstacles.push(avoid::Obstacle::car(
                    Some(r.entity),
                    None,
                    xy(r.pos),
                    [r.vel[0], r.vel[1]],
                    [r.vel[0], r.vel[1]],
                ));
            }
        }
        for u in &self.units {
            if u.kind != UnitKind::Car || (ext && u.entity.is_some()) {
                continue;
            }
            self.obstacles.push(avoid::Obstacle::car(
                u.entity,
                Some(u.id),
                u.body.pos,
                u.body.vel(),
                u.body.heading,
            ));
        }
        // Parked cars and cones around ground units (solid in `wp_sim`; police drove into
        // them): stopped obstacles to follow, wait behind or pass.
        if !self.props.is_empty() {
            let mut near: Vec<u32> = Vec::new();
            for u in &self.units {
                if u.kind == UnitKind::Car && u.player.is_none() && !u.wrecked {
                    self.props.around(u.body.pos, &mut near);
                }
            }
            near.sort_unstable();
            near.dedup();
            for k in near {
                let b = self.props.get(k);
                self.obstacles.push(avoid::Obstacle {
                    entity: None,
                    unit: None,
                    pos: b.pos,
                    vel: [0.0, 0.0],
                    // Local +Y forward, rotated by the prop's yaw.
                    fwd: [-b.rot[0], b.rot[1]],
                    half_len: b.half[1],
                    half_wid: b.half[0],
                    ghost: false,
                });
            }
        }
    }

    /// Run obstacle awareness for unit `i` for one tick; returns the lane offset to steer for.
    pub(crate) fn update_avoid(&mut self, i: usize, dt: f32) -> f32 {
        let u = &mut self.units[i];
        let ignore = match (&u.assignment, u.tactic) {
            (Assignment::Pursue { target }, Tactic::Pit | Tactic::BoxIn(_) | Tactic::Ram(_)) => {
                Some(*target)
            }
            _ => None,
        };
        let chase = matches!(
            u.assignment,
            Assignment::Pursue { .. } | Assignment::Intercept { .. }
        );
        // Kinematic traffic has no collider; near a player it turns physical. A chasing unit
        // farther than GHOST_PASS_M from every racer drives through it (out of sight).
        let pos = u.body.pos;
        let pass_ghosts = chase
            && self
                .racers
                .values()
                .all(|r| graph::dist2(xy(r.pos), pos) > GHOST_PASS_M * GHOST_PASS_M);
        // Closing on a stopped racer it can see coming (arrest).
        let arrest = match self.units[i].assignment {
            Assignment::Pursue { target } | Assignment::Intercept { target, .. } => {
                self.racers.get(&target).is_some_and(|r| {
                    r.vel[0] * r.vel[0] + r.vel[1] * r.vel[1] < 9.0
                        && graph::dist2(xy(r.pos), pos) < ARREST_CLOSE_M * ARREST_CLOSE_M
                })
            }
            _ => false,
        };
        let u = &mut self.units[i];
        let me = avoid::Me {
            unit: u.id,
            entity: u.entity,
            pos: u.body.pos,
            heading: u.body.heading,
            track_s: u.track_s,
            ignore,
            target: u.assignment.target(),
            skip_higher_units: false,
            chase,
            pass_ghosts,
            arrest,
        };
        avoid::update(
            &mut u.avoid,
            &me,
            &u.cmd.track,
            u.cmd.lane_offset_m,
            u.body.speed,
            u.cmd.siren,
            &self.obstacles,
            &self.cfg.pursuit.avoid,
            dt,
        )
    }

    /// Bind a unit to the vehicle entity `wp_sim` spawned for it, with that car's steering
    /// geometry (`VehicleDef::wheelbase_m`, front `WheelDef::max_steer_rad`). From then on the
    /// unit is driven externally: `wp_sim` applies [`PoliceSystem::unit_input`] and reports back
    /// with [`PoliceSystem::sync_unit`].
    pub fn bind_unit_entity(&mut self, unit: UnitId, entity: EntityId, geometry: SteerGeometry) {
        if let Some(i) = self.unit_index(unit) {
            let u = &mut self.units[i];
            u.entity = Some(entity);
            u.internal = false;
            u.geometry = geometry;
            u.controller = None;
            u.pro = None;
        }
    }

    /// Limits of the car a unit drives (`wp_vehicle::metrics`: skidpad, braking, 0–100, top
    /// speed; see [`pro::CarLimits::from_metrics`]). Without them the pro driver plans with the
    /// `[pro] default_limits` of `pursuit.toml` (the Sentinel Cruiser).
    pub fn set_unit_limits(&mut self, unit: UnitId, limits: pro::CarLimits) {
        if let Some(i) = self.unit_index(unit) {
            let u = &mut self.units[i];
            u.limits = Some(limits);
            if let Some(d) = u.pro.as_mut() {
                d.limits = limits;
            }
        }
    }

    /// Report what the driver of an externally driven unit feels (yaw rate, road grip), after
    /// [`PoliceSystem::sync_unit`]. Optional: without it the pro driver assumes a dry road and
    /// learns the grip from its slides.
    pub fn sync_unit_feel(&mut self, unit: UnitId, feel: pro::UnitFeel) {
        if let Some(i) = self.unit_index(unit) {
            self.units[i].feel = feel;
        }
    }

    /// Report how upright an externally driven unit is (row 75): `up_z` is the world Z of the
    /// chassis up axis (1 upright, 0 on its side, -1 on its roof), after
    /// [`PoliceSystem::sync_unit`]. A unit on its side or roof for
    /// [`stuck::FLIPPED_RESET_S`] is reset upright in its lane (once no racer sees it).
    /// Optional: without it a unit counts as upright.
    pub fn sync_unit_upright(&mut self, unit: UnitId, up_z: f32) {
        let now = self.now;
        if let Some(i) = self.unit_index(unit) {
            let u = &mut self.units[i];
            if up_z < stuck::FLIPPED_UP_Z {
                u.flipped_since.get_or_insert(now);
            } else {
                u.flipped_since = None;
            }
        }
    }

    /// Add a player-driven chase unit. It receives [`UnitOrder`] suggestions, never control.
    pub fn add_player_unit(
        &mut self,
        player: PlayerId,
        entity: EntityId,
        pos: [f32; 3],
        heading: [f32; 2],
    ) -> UnitId {
        let id = self.alloc_unit_id();
        let mut u = Unit::new(
            id,
            UnitKind::Car,
            UnitRole::Player,
            xy(pos),
            heading,
            self.cfg.pursuit.risk.budget,
        );
        u.entity = Some(entity);
        u.player = Some(player);
        u.internal = false;
        self.units.push(u);
        id
    }

    /// Remove a player-driven chase unit (the player left or changed role). Assignments,
    /// incidents and tools that referenced it are released. Returns false for unknown ids and
    /// AI units (those are retired by the dispatcher). Added by 07 (net) for player cop seats.
    pub fn remove_player_unit(&mut self, unit: UnitId) -> bool {
        let Some(i) = self.unit_index(unit) else {
            return false;
        };
        if self.units[i].player.is_none() {
            return false;
        }
        self.units.remove(i);
        for inc in &mut self.incidents {
            if inc.unit == Some(unit) {
                inc.unit = None;
            }
        }
        true
    }

    /// Report the physical state of an externally driven unit.
    pub fn sync_unit(&mut self, unit: UnitId, s: UnitSync) {
        if let Some(i) = self.unit_index(unit) {
            let u = &mut self.units[i];
            // Negative while clearly rolling backwards. With an unsigned speed a unit going
            // backwards "too fast" braked, and in reverse gear the brake pedal drives backwards
            // (`wp_vehicle` swaps the pedals): a runaway reverse into buildings (row 66).
            let heading = graph::normalize(s.heading);
            let along = s.vel[0] * heading[0] + s.vel[1] * heading[1];
            let speed = if along < -BACKWARDS_MPS {
                along
            } else {
                (s.vel[0] * s.vel[0] + s.vel[1] * s.vel[1]).sqrt()
            };
            u.body = UnitBody {
                pos: xy(s.pos),
                heading,
                speed,
                lat_g: u.body.lat_g,
                alt_m: s.pos[2],
            };
            u.vel = [s.vel[0], s.vel[1]];
            u.health = s.health;
            u.effects = s.effects;
        }
    }

    /// Whether another car is closing on unit `i` and would reach it within
    /// [`TURN_LOOK_S`] (with `ahead_only`, one in front of it; with `racers_only`, a racer: a pursuer turns round for its racer at once
    /// past other traffic, but not across a racer's path — Loop 9: a pursuer U-turned in front of
    /// the oncoming racer it had just been given, which hit it and left the corridor; DQ). A unit
    /// doesn't start turning across the road then (opponent-pace: units
    /// turning round on the Hollins hairpin, just after the race leader passed, ended up across
    /// both lanes in front of the next racer).
    fn traffic_closing(&self, i: usize, racers_only: bool, ahead_only: bool) -> bool {
        let u = &self.units[i];
        let (entity, pos, heading) = (u.entity, u.body.pos, u.body.heading);
        self.obstacles.iter().any(|o| {
            // Other police cars coordinate among themselves (converging on an arrest they
            // would hold each other up for good).
            if o.ghost || o.unit.is_some() || (o.entity.is_some() && o.entity == entity) {
                return false;
            }
            if racers_only && !o.entity.is_some_and(|e| self.racers.contains_key(&e)) {
                return false;
            }
            let r = graph::sub(o.pos, pos);
            if ahead_only && graph::dot(r, heading) <= 0.0 {
                return false;
            }
            let d2 = graph::dot(r, r);
            if !(1e-4..=TURN_LOOK_M * TURN_LOOK_M).contains(&d2) {
                return false;
            }
            // Closing speed along the line between them, times the distance: c·d.
            let cd = -graph::dot(r, o.vel);
            cd > TURN_LOOK_MIN_MPS * d2.sqrt() && d2 < cd * TURN_LOOK_S
        })
    }

    /// Driver input for an externally driven AI unit. Round 12's manoeuvres come first
    /// (turn-round U-turn / reverse swing, give-way, push-through, back-out, holds, stuck
    /// detection); the plain drive along the track is, with lights on, the unit's
    /// [`pro::ProDriver`] (lead P8: racing line inside its own-direction lanes, grip-limit
    /// speed profile with road grip from [`PoliceSystem::sync_unit_feel`], damage and the car's
    /// [`pro::CarLimits`], late braking, slide control) and, with lights off, the lane and
    /// gentle corner budget on `wp_world`'s shared pure-pursuit and PID controller. Call once
    /// per physics tick with that tick's `dt`, after [`PoliceSystem::sync_unit`]. `None` for
    /// player units and unknown ids.
    pub fn unit_input(&mut self, unit: UnitId, dt: f32) -> Option<VehicleInput> {
        let i = self.unit_index(unit)?;
        let brake = self.cfg.pursuit.unit.brake_mps2;
        let u = &mut self.units[i];
        if u.player.is_some() {
            return None;
        }
        let v = u.body.speed;
        if !u.cmd.track.is_drivable() {
            // Parked (tool units, between jobs): hold the brake. The controller alone releases
            // it below its hold speed, and the car would idle-creep away.
            let (_, b) = u.controller().pedals(v, -brake, dt, true);
            let (brake, handbrake) = standstill_hold(v, b);
            return Some(VehicleInput {
                brake,
                handbrake,
                siren: u.cmd.siren,
                headlights: true,
                ..Default::default()
            });
        }
        let (s, _) = u.cmd.track.project_ahead(u.body.pos, u.track_s, 3.0, 40.0);
        u.track_s = s;
        match u.unstick.stand_at {
            Some(p) if graph::dist(p, u.body.pos) < stuck::STAND_M => u.unstick.stand_s += dt,
            _ => {
                u.unstick.stand_at = Some(u.body.pos);
                u.unstick.stand_s = 0.0;
            }
        }
        let offset = self.update_avoid(i, dt);
        Some(self.drive_input(i, offset, dt))
    }

    /// Driver input for unit `i` (round 12 + lead P8): the round-12 manoeuvres first (U-turn,
    /// reverse swing, give-way, push-through, back-out, holds, stuck detection); then, for the
    /// plain drive along the track, with lights on the [`pro::ProDriver`] (racing line in the
    /// own-direction lanes, grip-limit speed profile, late braking, slide control) and with
    /// lights off pure pursuit on the lane at `offset` with PID pedals toward the think-rate
    /// corner plan (`planned_speed`, `patrol_cornering_g`) and comfortable braking.
    fn drive_input(&mut self, i: usize, offset: f32, dt: f32) -> VehicleInput {
        let (accel, brake) = (
            self.cfg.pursuit.unit.accel_mps2,
            self.cfg.pursuit.unit.brake_mps2,
        );
        let u = &mut self.units[i];
        let v = u.body.speed;
        let s = u.track_s;
        let look = u.controller().lookahead(v);
        // Static feelers (row 66): away from a wall at the side, slow for one ahead.
        let offset = offset + u.avoid.wall.shift_m;
        let target = unit::lane_point(&u.cmd.track, s + look, offset);
        let (pos, heading) = (u.body.pos, u.body.heading);
        let want = u
            .cmd
            .speed_mps
            .min(u.planned_speed)
            .min(u.avoid.limit_mps)
            .min(u.avoid.wall.limit_mps);
        let hold = u.avoid.limit_mps <= 0.0;
        let acc = (want - v).clamp(-brake, accel);
        let disabled = u.wrecked || u.assignment == Assignment::OutOfService;
        // Drive as with the siren; sound it unless quiet (`Unit::quiet`).
        let siren = u.cmd.siren;
        let siren_out = siren && !u.quiet;
        // Turning round toward a target behind (row 60): a reverse swing, not a wide forward
        // U-turn or a drive round the block.
        let now = self.now;
        let to_t = graph::normalize(graph::sub([target[0], target[1]], pos));
        let facing = graph::dot(heading, to_t);
        if let stuck::Phase::UTurn {
            since,
            until,
            steer,
        } = u.unstick.phase
        {
            if facing < stuck::SWING_DONE_COS && now < until && !disabled {
                let fast = v > stuck::UTURN_MPS;
                let handbrake = if fast && now.since(since) < stuck::UTURN_HANDBRAKE_S {
                    1.0
                } else {
                    0.0
                };
                // Up to the U-turn speed briskly (0.5 throttle crept round at ≈ 3 m/s: a
                // half circle took 5 s, owner "don't turn around fast enough"), then hold it.
                let throttle = if fast {
                    0.0
                } else if v < stuck::UTURN_MPS - 1.0 {
                    UTURN_THROTTLE
                } else {
                    UTURN_HOLD_THROTTLE
                };
                self.units[i].branch = 1;
                return VehicleInput {
                    steer,
                    throttle,
                    brake: if fast { 0.5 } else { 0.0 },
                    handbrake,
                    siren: siren_out,
                    headlights: true,
                    ..Default::default()
                };
            }
            u.unstick.phase = stuck::Phase::Drive;
            u.unstick.swing_ready = now.plus(stuck::SWING_COOLDOWN_S);
            u.unstick.slow_s = 0.0;
        }
        if let stuck::Phase::Swing { until, steer } = u.unstick.phase {
            if facing < stuck::SWING_DONE_COS && now < until && !disabled {
                // In reverse the pedals swap: brake drives backwards, throttle brakes.
                let (throttle, brake) = reverse_pedals(v, 1.0, SWING_REVERSE_MAX_MPS);
                self.units[i].branch = 2;
                return VehicleInput {
                    steer,
                    throttle,
                    brake,
                    siren: siren_out,
                    headlights: true,
                    ..Default::default()
                };
            }
            u.unstick.phase = stuck::Phase::Drive;
            u.unstick.swing_ready = now.plus(stuck::SWING_COOLDOWN_S);
            u.unstick.slow_s = 0.0;
        }
        let chasing = matches!(
            u.assignment,
            Assignment::Pursue { .. } | Assignment::Intercept { .. }
        );
        // Its way lies behind it and a car is closing: don't arc across the road in front of
        // it (Loop 9: a unit given a racer to chase turned across the road at walking pace into
        // the oncoming field). Hold straight in its lane until they have passed.
        // Its target coming straight at it (in front, closing head-on): wait in its lane for it
        // to go by, then turn round from a standstill, instead of driving at it and having to
        // brake from speed before turning (wp_sim police_passby).
        let target_head_on = u.assignment.target().is_some_and(|e| {
            self.racers.get(&e).is_some_and(|r| {
                let d = graph::sub([r.pos[0], r.pos[1]], pos);
                let rv = [r.vel[0], r.vel[1]];
                graph::dot(d, heading) > 0.0
                    && graph::dot(rv, heading) < -HEAD_ON_MPS
                    && graph::dot(d, d) < TURN_LOOK_M * TURN_LOOK_M
            })
        });
        // Waiting for it head-on while standing in its line (a single-carriageway hairpin,
        // Hollins): pull aside out of the line at a crawl instead of stalling in front of it,
        // so it can pass (and the field behind it is not stopped by a parked police car).
        let aside = if target_head_on {
            u.assignment
                .target()
                .and_then(|e| self.racers.get(&e))
                .and_then(|r| {
                    let rv = [r.vel[0], r.vel[1]];
                    let rd = graph::normalize(rv);
                    let off = graph::cross(rd, graph::sub(pos, [r.pos[0], r.pos[1]]));
                    (off.abs() < HEAD_ON_PATH_M).then(|| {
                        // Further out on the side it already leans to (its right when dead on).
                        let side = if off.abs() > 0.3 {
                            off.signum()
                        } else {
                            graph::cross(rd, graph::perp_left(heading)).signum()
                        };
                        let out = graph::scale(graph::perp_left(rd), side * HEAD_ON_PATH_M);
                        graph::add(graph::add(pos, graph::scale(heading, 6.0)), out)
                    })
                })
        } else {
            None
        };
        if let (Some(to), false) = (aside, disabled) {
            let left = graph::cross(heading, graph::sub(to, pos)) > 0.0;
            let (throttle, brake) = if v < HEAD_ON_ASIDE_MPS {
                (0.3, 0.0)
            } else {
                (0.0, 0.4)
            };
            self.units[i].branch = 3;
            return VehicleInput {
                steer: if left { -1.0 } else { 1.0 },
                throttle,
                brake,
                siren: siren_out,
                headlights: true,
                ..Default::default()
            };
        }
        if (target_head_on || facing < stuck::SWING_BEHIND_COS)
            && !disabled
            && matches!(u.unstick.phase, stuck::Phase::Drive)
            && (target_head_on || self.traffic_closing(i, chasing, true))
        {
            let (throttle, brake, handbrake) = hold_pedals(v);
            self.units[i].branch = 4;
            return VehicleInput {
                throttle,
                brake,
                handbrake,
                siren: siren_out,
                headlights: true,
                ..Default::default()
            };
        }
        let u = &mut self.units[i];
        // Lights on only (pursuit, intercept, response): a patrol turns round like traffic.
        let swing_wanted = matches!(u.unstick.phase, stuck::Phase::Drive)
            && siren
            && want > stuck::WANT_MPS
            && !disabled
            && facing < stuck::SWING_BEHIND_COS
            && v < stuck::SWING_MAX_MPS
            && now >= u.unstick.swing_ready
            && u.unstick.may_turn(now)
            && graph::dist(pos, [target[0], target[1]]) > 6.0
            && !self.traffic_closing(i, chasing, false);
        // Room for the turning circle (row 66: full-lock turns on narrow streets hit buildings):
        // forward round a centre on the target's side, backwards round one on the other side.
        let left_t = graph::cross(heading, to_t) > 0.0;
        let side = |left: bool| {
            let l = graph::perp_left(heading);
            if left {
                l
            } else {
                graph::scale(l, -1.0)
            }
        };
        let room = |centre: [f32; 2]| {
            self.walls
                .nearest(centre)
                .is_none_or(|d| d >= TURN_RADIUS_M + TURN_CLEAR_M)
        };
        let fwd_room = room(graph::add(pos, graph::scale(side(left_t), TURN_RADIUS_M)));
        let back_room = room(graph::add(pos, graph::scale(side(!left_t), TURN_RADIUS_M)));
        if swing_wanted && v >= stuck::UTURN_MIN_MPS && !fwd_room {
            // No room to U-turn here: stop, then three-point it (or drive on if no room for
            // that either).
            let u = &mut self.units[i];
            u.unstick.swing_ready = now.plus(if back_room {
                0.0
            } else {
                stuck::SWING_COOLDOWN_S
            });
            self.units[i].branch = 5;
            return VehicleInput {
                brake: 0.8,
                siren: siren_out,
                headlights: true,
                ..Default::default()
            };
        }
        if swing_wanted && (v >= stuck::UTURN_MIN_MPS || fwd_room) {
            // Still rolling, or standing with room for the turning circle: U-turn forward, full
            // lock toward the target's side. (From a standstill reverse takes about a second to
            // engage and a full-lock reverse arc turns slowly; with the round-12 tyre side-force
            // floor a parked cop's three-point turn missed wp_sim police_passby's 4 s bound.)
            let left = left_t;
            let steer = if left { -1.0 } else { 1.0 };
            let u = &mut self.units[i];
            u.unstick.phase = stuck::Phase::UTurn {
                since: now,
                until: now.plus(stuck::UTURN_MAX_S),
                steer,
            };
            u.unstick.note_turn(pos, now);
            self.stats.swings += 1;
            self.units[i].branch = 6;
            return VehicleInput {
                steer,
                brake: 0.5,
                handbrake: 1.0,
                siren: siren_out,
                headlights: true,
                ..Default::default()
            };
        }
        if swing_wanted && back_room && self.room_behind(i, 6.0) {
            // Reversing with right lock swings the nose left, and the other way round.
            let left = graph::cross(heading, to_t) > 0.0;
            let steer = if left { 1.0 } else { -1.0 };
            let u = &mut self.units[i];
            u.unstick.phase = stuck::Phase::Swing {
                until: now.plus(stuck::SWING_MAX_S),
                steer,
            };
            u.unstick.note_turn(pos, now);
            self.stats.swings += 1;
            self.units[i].branch = 7;
            return VehicleInput {
                steer,
                brake: 1.0,
                siren: siren_out,
                headlights: true,
                ..Default::default()
            };
        }
        let u = &mut self.units[i];
        // Un-sticking ([`stuck`]): back out first; otherwise watch for no progress.
        if let stuck::Phase::Reverse {
            from,
            dist_m,
            until,
            steer,
        } = u.unstick.phase
        {
            if graph::dist(pos, from) < dist_m && now < until && !disabled {
                // Brake held at a standstill engages reverse; in reverse the pedals swap, so
                // the brake pedal drives backwards.
                let (throttle, brake) = reverse_pedals(v, 0.6, REVERSE_MAX_MPS);
                self.units[i].branch = 8;
                return VehicleInput {
                    steer,
                    throttle,
                    brake,
                    siren: siren_out,
                    headlights: true,
                    ..Default::default()
                };
            }
            u.unstick.phase = stuck::Phase::Drive;
            u.unstick.slow_s = 0.0;
        }
        // Give way (opponent-pace): standing blocked by a slow racer it isn't after, back out
        // of its way, swinging the nose back along the lane, then wait there (avoid still
        // holds it) so the racer can get past. Counts as a turn-round try.
        // Gave way: wait for that racer to get past (it is still in front and close).
        if let Some((e, until)) = u.unstick.give_way_to {
            let waiting = now < until
                && !disabled
                && self.racers.get(&e).is_some_and(|r| {
                    let d = graph::sub([r.pos[0], r.pos[1]], pos);
                    graph::dot(d, heading) > 0.0
                        && graph::dot(d, d) < stuck::GIVE_WAY_HOLD_M * stuck::GIVE_WAY_HOLD_M
                });
            if waiting {
                let (throttle, brake, handbrake) = hold_pedals(v);
                self.units[i].branch = 9;
                return VehicleInput {
                    throttle,
                    brake,
                    handbrake,
                    siren: siren_out,
                    headlights: true,
                    ..Default::default()
                };
            }
            self.units[i].unstick.give_way_to = None;
        }
        let u = &mut self.units[i];
        // The slow racer it is in the way of: the one blocking it, or (blocked by anything)
        // one standing close by, the two of them stuck round the same car (Hollins hairpin).
        let after = |e: EntityId| {
            matches!(
                u.assignment,
                Assignment::Pursue { target, .. } | Assignment::Intercept { target, .. }
                    if target == e
            )
        };
        let slow = stuck::GIVE_WAY_RACER_MPS * stuck::GIVE_WAY_RACER_MPS;
        let near = stuck::GIVE_WAY_NEAR_M * stuck::GIVE_WAY_NEAR_M;
        let give_way_racer = match u.avoid.blocked_by {
            Some(avoid::BlockedBy::Vehicle(e))
                if self.racers.get(&e).is_some_and(|r| {
                    !after(e) && r.vel[0] * r.vel[0] + r.vel[1] * r.vel[1] < slow
                }) =>
            {
                Some(e)
            }
            Some(_) => self
                .racers
                .iter()
                .filter(|(e, r)| {
                    let d = graph::sub([r.pos[0], r.pos[1]], pos);
                    !after(**e)
                        && graph::dot(d, d) < near
                        && r.vel[0] * r.vel[0] + r.vel[1] * r.vel[1] < slow
                })
                .map(|(e, _)| *e)
                .next(),
            None => None,
        };
        let racer_block = give_way_racer.is_some();
        let u = &mut self.units[i];
        if racer_block && v.abs() < 0.5 && !disabled {
            u.unstick.give_way_s += dt;
        } else {
            u.unstick.give_way_s = 0.0;
        }
        if u.unstick.give_way_s >= stuck::GIVE_WAY_S
            && u.unstick.may_turn(now)
            && self.room_behind(i, stuck::GIVE_WAY_M)
        {
            let u = &mut self.units[i];
            u.unstick.give_way_s = 0.0;
            u.unstick.note_turn(pos, now);
            if let Some(e) = give_way_racer {
                u.unstick.give_way_to = Some((
                    e,
                    now.plus(2.0 * stuck::REVERSE_MAX_S + stuck::GIVE_WAY_HOLD_S),
                ));
            }
            // Straight back: make room in front (steering round near a hairpin swung it
            // across the road).
            let steer = 0.0;
            u.unstick.phase = stuck::Phase::Reverse {
                from: pos,
                dist_m: stuck::GIVE_WAY_M,
                until: now.plus(2.0 * stuck::REVERSE_MAX_S),
                steer,
            };
            self.stats.give_ways += 1;
            self.units[i].branch = 10;
            return VehicleInput {
                steer,
                brake: 1.0,
                siren: siren_out,
                headlights: true,
                ..Default::default()
            };
        }
        let u = &mut self.units[i];
        // Push-through (row 61): with lights on, blocked by a stopped civilian or parked car
        // it can't pass, shove it aside at low speed; if that doesn't move it, un-stick.
        let av = self.cfg.pursuit.avoid.clone();
        if let stuck::Phase::Push { from, until } = u.unstick.phase {
            let moved = graph::dist(pos, from);
            if moved < PUSH_DONE_M && now < until && !disabled {
                // Straight ahead, a little left (nudging the blocker to the kerb): a shove,
                // not a turn (steering for the track swung units across the road).
                let tgt = graph::add(
                    graph::add(pos, graph::scale(heading, PUSH_AHEAD_M)),
                    graph::scale(graph::perp_left(heading), PUSH_SIDE_M),
                );
                let ctl = u.controller();
                let steer = ctl.steer(pos, heading, [tgt[0], tgt[1], 0.0]);
                let acc = (av.push_mps - v).clamp(-brake, accel);
                let (throttle, brake) = ctl.pedals(v, acc, dt, false);
                self.units[i].branch = 11;
                return VehicleInput {
                    steer,
                    throttle,
                    brake,
                    siren: siren_out,
                    headlights: true,
                    ..Default::default()
                };
            }
            u.unstick.phase = stuck::Phase::Drive;
            u.unstick.push_ready = now.plus(av.push_cooldown_s);
            u.avoid.push_wait_s = 0.0;
            if moved < av.push_min_progress_m && !disabled {
                // Didn't budge: back out / re-route / respawn like any stuck unit.
                self.on_stuck(i, true);
            }
        }
        let u = &mut self.units[i];
        let pushable = match u.avoid.blocked_by {
            Some(avoid::BlockedBy::Traffic) => true,
            Some(avoid::BlockedBy::Vehicle(e)) => !self.racers.contains_key(&e),
            _ => false,
        };
        let u = &mut self.units[i];
        let blocked = siren
            && !disabled
            && pushable
            && u.avoid.limit_mps < 0.5
            && v.abs() < 1.0
            && u.cmd.speed_mps.min(u.planned_speed) > stuck::WANT_MPS
            && now >= u.unstick.push_ready
            && matches!(u.unstick.phase, stuck::Phase::Drive);
        u.avoid.push_wait_s = if blocked {
            u.avoid.push_wait_s + dt
        } else {
            0.0
        };
        if u.avoid.push_wait_s >= av.push_after_s {
            u.avoid.push_wait_s = 0.0;
            u.unstick.phase = stuck::Phase::Push {
                from: pos,
                until: now.plus(av.push_max_s),
            };
            self.stats.pushes += 1;
        }
        let u = &mut self.units[i];
        let chasing = matches!(
            u.assignment,
            Assignment::Pursue { .. } | Assignment::Intercept { .. }
        );
        let wants = want > stuck::WANT_MPS && !hold && !disabled;
        if wants && v.abs() < stuck::SLOW_MPS {
            u.unstick.slow_s += dt;
        } else if v.abs() > 2.0 * stuck::SLOW_MPS {
            u.unstick.slow_s = 0.0;
        }
        // Waiting behind a stopped racer or police car (a wreck, a stand-off on a narrow road),
        // not in a red-light queue. Civilian traffic jams clear by themselves: no turning
        // around across them.
        let red_ahead = u
            .avoid
            .junction
            .is_some_and(|j| j.light == Some(avoid::Light::Stop) && j.s - u.track_s < 80.0);
        let behind_racer_or_unit = match u.avoid.blocked_by {
            Some(avoid::BlockedBy::Unit(_)) => true,
            Some(avoid::BlockedBy::Vehicle(e)) => self.racers.contains_key(&e),
            _ => false,
        };
        let u = &mut self.units[i];
        if hold && !disabled && behind_racer_or_unit && !red_ahead && v.abs() < stuck::SLOW_MPS {
            u.unstick.held_s += dt;
        } else {
            u.unstick.held_s = 0.0;
        }
        u.unstick.progress(pos);
        self.units[i].branch = 0;
        let (steer, mut throttle, mut brake) = if siren {
            self.pro_drive(i, offset, disabled, dt)
        } else {
            let u = &mut self.units[i];
            // The pro driver picks up from here when the lights go on.
            u.pro = None;
            let ctl = u.controller();
            let steer = ctl.steer(pos, heading, [target[0], target[1], 0.0]);
            let (throttle, brake) = ctl.pedals(v, acc, dt, disabled);
            (steer, throttle, brake)
        };
        let u = &mut self.units[i];
        if steer.abs() > 0.1 {
            u.unstick.last_steer = steer;
        }
        if u.unstick.slow_s >= stuck::STUCK_AFTER_S {
            self.on_stuck(i, false);
        } else if u.unstick.held_s >= stuck::HELD_AFTER_S
            || (chasing && u.unstick.held_s >= stuck::HELD_CHASE_S)
        {
            self.on_stuck(i, true);
        }
        let mut handbrake = 0.0;
        if hold {
            // The shared controller releases the brake below its hold speed; right behind
            // another vehicle or at a stop line the unit must not creep forwards either.
            throttle = 0.0;
            (brake, handbrake) = standstill_hold(v, brake.max(1.0));
        }
        // Still rolling backwards after a reverse manoeuvre: the gearbox is in reverse, where
        // the brake pedal drives backwards (`body.speed` is unsigned, so the pedals above
        // would brake). Stop on the handbrake; at a standstill the throttle selects first
        // again (from claude/beautiful-hopper-sfuta0: a unit ran backwards at 22 m/s).
        let u = &self.units[i];
        if graph::dot(u.vel, heading) < -ROLLING_BACK_MPS {
            throttle = 0.0;
            brake = 0.0;
            handbrake = 1.0;
        }
        VehicleInput {
            steer,
            throttle,
            brake,
            handbrake,
            siren: siren_out,
            headlights: true,
            ..Default::default()
        }
    }

    /// The plain drive of a lights-on unit by its [`pro::ProDriver`] (lead P8): steer, throttle,
    /// brake. The speed it is asked for is the command capped by obstacle awareness and the
    /// static feelers; a pass or a wall shift is driven as an exact offset. The pro driver's
    /// own stuck reverse is off: round 12's un-sticking (`stuck`) owns that, and two recovery
    /// loops fought each other.
    fn pro_drive(&mut self, i: usize, offset: f32, disabled: bool, dt: f32) -> (f32, f32, f32) {
        // Traffic close ahead: keep to the lane (obstacle awareness looks along the lane), no
        // racing line through other cars' lanes.
        let traffic_ahead = self.traffic_near_line(i);
        let pro_cfg = &self.cfg.pursuit.pro;
        let full_g = self.cfg.pursuit.pursuit_cornering_g;
        let room = &self.room;
        let u = &mut self.units[i];
        let v = u.body.speed;
        let want = u
            .cmd
            .speed_mps
            .min(u.avoid.limit_mps)
            .min(u.avoid.wall.limit_mps);
        let shift = u.avoid.wall.shift_m;
        let off = offset + shift;
        let exact = u.avoid.pass_offset_m.is_some() || (off - u.cmd.lane_offset_m).abs() > 0.05;
        let ctx = pro::DriveCtx {
            pos: u.body.pos,
            heading: u.body.heading,
            vel: if u.vel == [0.0, 0.0] {
                u.body.vel()
            } else {
                u.vel
            },
            yaw_rate: u.feel.yaw_rate,
            road_grip: u.feel.road_grip.unwrap_or(1.0),
            damage_grip: u.grip_scale(),
            power_scale: u.effects.power_scale,
            target_mps: want.max(0.0),
            // Shadowing (risk budget spent) keeps its gentle corner budget; a pursuit or a
            // response drives at the grip limit.
            cornering_cap_g: (u.cmd.cornering_g < full_g - 1e-3).then_some(u.cmd.cornering_g),
            braking_cap_mps2: None,
            lane_offset_m: u.cmd.lane_offset_m,
            pass_offset_m: exact.then_some(off),
            racing_line: !traffic_ahead && !exact,
            disabled,
        };
        let (geometry, limits) = (u.geometry, u.limits.unwrap_or(pro_cfg.default_limits));
        let driver = u.pro.get_or_insert_with(|| {
            let mut params = pro_cfg.clone();
            params.stuck_s = f32::INFINITY;
            pro::ProDriver::new(params, limits, geometry, v)
        });
        let out = driver.drive(&ctx, &u.cmd.track, room, dt);
        u.pro_dbg = (out.planned_mps, out.reversing);
        u.turning_round = out.turning_round;
        let input = out.input;
        let steer = (input.steer - u.effects.steer_pull).clamp(-1.0, 1.0);
        (steer, input.throttle, input.brake)
    }

    /// Whether another vehicle is close ahead of unit `i` near its road (within
    /// [`pro::ProParams::traffic_clear_m`] ahead and a road width either side): the racing line
    /// then gives way to the lane. The unit's own car and the target it is allowed to touch
    /// (PIT, box-in) do not count.
    fn traffic_near_line(&self, i: usize) -> bool {
        let u = &self.units[i];
        let range = self.cfg.pursuit.pro.traffic_clear_m;
        let ignore = match (&u.assignment, u.tactic) {
            (Assignment::Pursue { target }, Tactic::Pit | Tactic::BoxIn(_)) => Some(*target),
            _ => None,
        };
        let f = u.body.heading;
        let r = [f[1], -f[0]];
        self.obstacles.iter().any(|o| {
            if (o.entity.is_some() && o.entity == u.entity)
                || o.unit == Some(u.id)
                || (ignore.is_some() && o.entity == ignore)
            {
                return false;
            }
            let d = graph::sub(o.pos, u.body.pos);
            let ahead = graph::dot(d, f);
            ahead > -3.0 && ahead < range && graph::dot(d, r).abs() < 9.0
        })
    }

    pub(crate) fn alloc_unit_id(&mut self) -> UnitId {
        let id = UnitId(self.next_unit);
        self.next_unit += 1;
        id
    }

    // -----------------------------------------------------------------------------------------
    // Racer-side gadgets
    // -----------------------------------------------------------------------------------------

    /// Radar detector: units within the tier's range that are patrolling or pursuing.
    pub fn radar(&self, racer_pos: [f32; 3], tier: usize) -> Vec<RadarBlip> {
        let ranges = &self.cfg.tools.gadgets.radar_range_m;
        let range = ranges[tier.min(ranges.len() - 1)];
        let p = xy(racer_pos);
        let mut v: Vec<RadarBlip> = self
            .units
            .iter()
            .filter(|u| u.kind == UnitKind::Car && !u.wrecked)
            .filter_map(|u| {
                let state = match u.assignment {
                    Assignment::Patrol { .. } => tools::RadarState::Patrol,
                    Assignment::Pursue { .. } | Assignment::Intercept { .. } => {
                        tools::RadarState::Pursuit
                    }
                    _ => return None,
                };
                let d = graph::dist(u.body.pos, p);
                (d <= range).then(|| RadarBlip {
                    unit: u.id,
                    distance_m: d,
                    dir: graph::normalize(graph::sub(u.body.pos, p)),
                    state,
                })
            })
            .collect();
        v.sort_by(|a, b| {
            a.distance_m
                .total_cmp(&b.distance_m)
                .then(a.unit.cmp(&b.unit))
        });
        v
    }

    /// Scanner: radio lines spoken after `since` (all if `None`), delayed by the tier's delay.
    pub fn scanner(&self, tier: usize, since: Option<SimTime>) -> Vec<RadioLine> {
        let delays = &self.cfg.tools.gadgets.scanner_delay_s;
        let delay = delays[tier.min(delays.len() - 1)];
        self.radio
            .iter()
            .filter(|l| since.is_none_or(|s| l.time > s) && self.now.since(l.time) >= delay)
            .cloned()
            .collect()
    }

    /// Full radio log (captain).
    pub fn radio_log(&self) -> impl Iterator<Item = &RadioLine> {
        self.radio.iter()
    }

    /// Priority tier of the latest logged radio line spoken by `unit` with this `text` (row
    /// 144). The contract's `PoliceEvent::Radio` carries no tier yet; bridges (audio, the
    /// browser strip) look it up here. `None` if it is no longer in the log.
    pub fn radio_tier_of(&self, unit: Option<UnitId>, text: &str) -> Option<radio::RadioTier> {
        self.radio
            .iter()
            .rev()
            .find(|l| l.unit == unit && l.text == text)
            .map(|l| l.tier)
    }

    /// Helicopters on station (tracking a racer).
    pub fn helicopters(&self) -> Vec<HeloView> {
        self.units
            .iter()
            .filter_map(|u| match (u.kind, &u.assignment) {
                (UnitKind::Helicopter, Assignment::HeloTrack { target }) => {
                    let r = self.racers.get(target);
                    let aim = r.and_then(|r| r.last_known).unwrap_or(u.body.pos);
                    let aim_z = r.map_or(0.0, |r| r.pos[2]);
                    Some(HeloView {
                        unit: u.id,
                        pos: [u.body.pos[0], u.body.pos[1], u.body.alt_m],
                        spotlight_target: [aim[0], aim[1], aim_z],
                        cone_half_angle_rad: self.cfg.tools.helicopter.spotlight_half_angle_rad,
                        target: *target,
                        has_visual: self.helo_sight.get(&u.id).is_some_and(|s| s.visual),
                    })
                }
                _ => None,
            })
            .collect()
    }

    /// Stable hash of the police state (tick, racers, units, tools) for determinism checks.
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        h.write_u64(self.tick);
        for (e, r) in &self.racers {
            h.write_u32(e.0);
            h.write_f32_q(r.heat, 1e3);
            h.write_u32(r.status as u32);
            h.write_u32(r.level as u32);
        }
        for u in &self.units {
            h.write_u32(u.id.0);
            h.write_f32_q(u.body.pos[0], 1e3);
            h.write_f32_q(u.body.pos[1], 1e3);
            h.write_f32_q(u.body.speed, 1e3);
            h.write_f32_q(u.health, 1e4);
            h.write_bytes(u.assignment.name().as_bytes());
        }
        h.write_u32(self.roadblocks.len() as u32);
        h.write_u32(self.spikes.len() as u32);
        h.write_u64(self.stats.pursuits.len() as u64);
        if let Some(r) = &self.race {
            h.write_bytes(r.race.as_bytes());
            h.write_u32(r.stakeouts.iter().filter(|s| s.sprung).count() as u32);
        }
        h.finish()
    }
}

/// A pursuer this close to a stopped racer ignores other stopped police cars, m.
const ARREST_CLOSE_M: f32 = 45.0;

/// Turning-circle radius of a police car at full lock (centre to the car's centre line), m.
const TURN_RADIUS_M: f32 = 5.0;

/// A turning circle needs walls at least this much farther than its radius from its centre, m.
const TURN_CLEAR_M: f32 = 2.0;

/// A chasing unit this far from every racer ignores kinematic (collider-less) traffic, m.
const GHOST_PASS_M: f32 = 150.0;

/// A push-through ends once the unit has moved this far, m.
const PUSH_DONE_M: f32 = 8.0;

/// A pushing unit aims at a point this far ahead of it, m.
const PUSH_AHEAD_M: f32 = 8.0;

/// A pushing unit aims this far left of its line (nudging the blocker toward the kerb), m.
const PUSH_SIDE_M: f32 = 0.8;

/// A unit moving backwards faster than this reports a negative speed (below it, the speed
/// magnitude, so settling and creeping at a standstill behave as before), m/s.
const BACKWARDS_MPS: f32 = 1.0;

/// Fastest a unit backs out of a jam (un-sticking), m/s.
const REVERSE_MAX_MPS: f32 = 3.0;

/// Fastest a unit reverses in a three-point turn (slower and the turn doesn't come round in
/// A target racer coming at a unit faster than this (along the unit's heading) is head-on, m/s.
const HEAD_ON_MPS: f32 = 5.0;
/// A car closing on a unit this soon stops it from starting a turn-round, s.
const TURN_LOOK_S: f32 = 4.0;
/// ... looking this far, m.
const TURN_LOOK_M: f32 = 80.0;
/// ... and closing at least this fast, m/s.
const TURN_LOOK_MIN_MPS: f32 = 2.0;
/// its time), m/s.
const SWING_REVERSE_MAX_MPS: f32 = 5.0;

/// Pedals for backing up with `pedal` on the brake (which drives backwards in reverse), easing
/// off and braking (throttle, in reverse) past `max_mps`. `v` is negative going backwards.
fn reverse_pedals(v: f32, pedal: f32, max_mps: f32) -> (f32, f32) {
    if v < -max_mps {
        (0.5, 0.0)
    } else if v < -0.8 * max_mps {
        (0.0, 0.0)
    } else {
        (0.0, pedal)
    }
}

/// Throttle, brake and handbrake that bring a car to a stop and keep it there, whichever way
/// it is rolling: rolling backwards it is in reverse, where the pedals swap (the brake drives
/// it backwards, the throttle brakes), so it brakes with the throttle.
fn hold_pedals(v: f32) -> (f32, f32, f32) {
    if v < -1.0 {
        (0.6, 0.0, 0.0)
    } else if v > 1.0 {
        (0.0, 1.0, 0.0)
    } else {
        (0.0, STANDSTILL_BRAKE, 1.0)
    }
}

/// A unit closer than this to the line of its target coming at it head-on pulls aside, m.
const HEAD_ON_PATH_M: f32 = 6.0;
/// ... at this crawl, m/s.
const HEAD_ON_ASIDE_MPS: f32 = 2.5;

/// Throttle at full lock in a forward U-turn below [`stuck::UTURN_MPS`] − 1 m/s.
const UTURN_THROTTLE: f32 = 0.9;
/// ... and from there up to [`stuck::UTURN_MPS`].
const UTURN_HOLD_THROTTLE: f32 = 0.35;
/// Rolling backwards faster than this outside a reverse manoeuvre, a physical unit stops on
/// the handbrake, m/s.
const ROLLING_BACK_MPS: f32 = 0.3;
/// Pedal held at a standstill (below `wp_vehicle`'s auto-reverse threshold of 0.5).
const STANDSTILL_BRAKE: f32 = 0.4;

/// Brake and handbrake that keep a car stopped: above walking pace the requested `brake`; at a
/// standstill a light pedal plus the handbrake, because `wp_vehicle`'s automatic gearbox
/// engages reverse when the pedal is held past half travel at a standstill.
fn standstill_hold(v: f32, brake: f32) -> (f32, f32) {
    if v.abs() > 1.0 {
        (brake, 0.0)
    } else {
        (STANDSTILL_BRAKE, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traffic_hazard_stand_in_is_opt_in() {
        // Real traffic is the norm (wp_sim); the statistical stand-in must be asked for.
        assert!(!PoliceSetup::default().hazards);
    }

    #[test]
    fn no_city_still_tracks_heat() {
        struct W;
        impl WorldQuery for W {
            fn surface_at(&self, _p: [f32; 3]) -> wp_contracts::SurfaceType {
                wp_contracts::SurfaceType::Asphalt
            }
            fn speed_limit_at(&self, _p: [f32; 3]) -> Option<f32> {
                None
            }
            fn signal_state_ahead(
                &self,
                _p: [f32; 3],
                _h: [f32; 3],
            ) -> Option<wp_contracts::SignalState> {
                None
            }
            fn time_of_day(&self) -> wp_contracts::TimeOfDay {
                wp_contracts::TimeOfDay::from_hours(12.0)
            }
            fn line_of_sight(&self, _a: [f32; 3], _b: [f32; 3]) -> bool {
                true
            }
        }
        let mut p = PoliceSystem::new();
        let r = RacerView {
            entity: EntityId(1),
            pos: [0.0; 3],
            vel: [0.0; 3],
        };
        let ev = vec![SimEvent::TrafficViolation {
            vehicle: EntityId(1),
            kind: wp_contracts::ViolationKind::Collision,
            pos: [0.0; 3],
        }];
        let mut out = Vec::new();
        p.step(&W, &[r], &ev, wp_contracts::PHYSICS_DT, &mut out);
        assert!(p.racer(EntityId(1)).unwrap().heat > 0.0);
        assert!(p.units().is_empty());
    }

    /// Flat, empty, always-visible world (no city).
    struct OpenWorld;
    impl WorldQuery for OpenWorld {
        fn surface_at(&self, _p: [f32; 3]) -> wp_contracts::SurfaceType {
            wp_contracts::SurfaceType::Asphalt
        }
        fn speed_limit_at(&self, _p: [f32; 3]) -> Option<f32> {
            None
        }
        fn signal_state_ahead(
            &self,
            _p: [f32; 3],
            _h: [f32; 3],
        ) -> Option<wp_contracts::SignalState> {
            None
        }
        fn time_of_day(&self) -> wp_contracts::TimeOfDay {
            wp_contracts::TimeOfDay::from_hours(12.0)
        }
        fn line_of_sight(&self, _a: [f32; 3], _b: [f32; 3]) -> bool {
            true
        }
    }

    /// B05 hooks for `wp_net`: a held racer's heat does not decay; `capture_abandoned` ends
    /// it as `Captured` with no capturer when no unit is on it, and clears the heat.
    #[test]
    fn held_racer_is_frozen_and_capture_abandoned_ends_as_captured() {
        let racer = EntityId(1);
        let view = RacerView {
            entity: racer,
            pos: [0.0; 3],
            vel: [0.0; 3],
        };
        let mut p = PoliceSystem::new();
        let mut out = Vec::new();
        p.step(&OpenWorld, &[view], &[], wp_contracts::PHYSICS_DT, &mut out);
        assert!(p.capture_abandoned(racer).is_none(), "not wanted: nothing");
        p.set_heat(racer, 55.0);
        assert!(p.is_wanted_or_pursued(racer));
        let level = p.wanted_level(racer);
        let heat = p.racer(racer).unwrap().heat;

        p.set_racer_held(racer, true);
        for _ in 0..240 * 30 {
            p.step(&OpenWorld, &[view], &[], wp_contracts::PHYSICS_DT, &mut out);
        }
        assert_eq!(p.racer(racer).unwrap().heat, heat, "no decay while held");

        // Released, unseen heat decays again.
        p.set_racer_held(racer, false);
        for _ in 0..240 * 5 {
            p.step(&OpenWorld, &[view], &[], wp_contracts::PHYSICS_DT, &mut out);
        }
        assert!(p.racer(racer).unwrap().heat < heat, "decays when released");

        p.set_racer_held(racer, true);
        let level_now = p.wanted_level(racer);
        assert!(level_now <= level);
        out.clear();
        assert_eq!(p.capture_abandoned(racer), Some((level_now, None)));
        assert!(!p.is_racer_held(racer));
        p.step(&OpenWorld, &[view], &[], wp_contracts::PHYSICS_DT, &mut out);
        assert!(out.iter().any(|e| matches!(
            e,
            SimEvent::Police(PoliceEvent::PursuitEnded {
                racer: r,
                outcome: PursuitOutcome::Captured,
                capturer: None,
                ..
            }) if *r == racer
        )));
        assert_eq!(p.wanted_level(racer), WantedLevel::None);
    }
}
