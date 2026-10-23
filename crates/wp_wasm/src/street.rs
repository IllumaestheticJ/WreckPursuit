//! Street racers (lead): NPC racers who cruise the city in free roam and race the player from
//! where they meet (A) to a finish the player picks (B), for a bet.
//!
//! - **Roster.** A live free-drive or city-chase session in a city gets the [`ROSTER`]: 13
//!   racers over the four [`LEVELS`] (2 Street … 5 Legend), each level with its own car pool,
//!   driver skill and bet range, spawned [`SPAWN_SPACING_M`] apart. By day 8 are out (two per
//!   level); at night ([`is_night`]) 13, four of them lapping the Loop 9 highway (owner row
//!   152). Off-shift racers wait in a lockup off the roads (asleep, `Sim::set_dormant`) and
//!   change shift out of the player's sight (logged moves, like a respawn). At night the loop
//!   racers end their laps in the Loop 9 rest stops and idle there; a challenge at a rest stop
//!   starts side by side on its parking road and offers a Loop 9 lap back to it (row 153).
//!   They are
//!   ordinary *scenario entities* (`street_<level>_<name>`, AI assists, no player steering
//!   aids), added to the scenario before the sim is built ([`plan_roster`]), so every
//!   recording carries them and a playback spawns them exactly as live did.
//! - **Roaming.** A roaming racer is a one-racer `wp_opponent::Field` on a route to a random
//!   destination, driven calmly: a careful profile (stops at reds, never uses the opposing
//!   lanes, no lane-splitting) and a *cruise capability* (top speed and grip capped well below
//!   the car's) so it keeps to traffic pace and the speed limit, with a stop-line guard for red
//!   and catchable amber lights. At the end of a route it plans the next one.
//! - **Challenge.** Honking (horn rising edge) within [`CHALLENGE_RADIUS_M`] of a roaming racer
//!   stops it and opens an [`Offer`]: the player picks B on the map ([`StreetRacers::preview`]),
//!   optionally waypoints the route must run through in order (up to [`MAX_WAYPOINTS`], owner
//!   row 151; gates the player passes within [`GATE_RADIUS_M`] before B counts), and a stake
//!   inside the level's range (capped by their cash), then accepts. Not while the player is
//!   wanted or chased (owner row 150): a honk does nothing and an open offer lapses.
//! - **Race.** Accepting pays the stake (a ledger line), resets both cars side by side at the
//!   start, at rest, facing along the route (logged moves, owner row 150),
//!   holds both for a 3 s countdown, then the racer drives a full-pace `Field` like a picker
//!   race's AI ([`wp_race::entrants`]): its level's [`DriverProfile`], the car's real
//!   capability (`CarCapability::for_car`), a run-off past B, and the owner's never-give-up
//!   rule (`RaceConfig::allow_abandon` off: it keeps racing through crashes, spins and the
//!   police until it finishes, is wrecked or is actually captured). First within
//!   [`FINISH_RADIUS_M`] of B wins, by any path (shortcuts are fair); a wreck or a capture
//!   decides it early; at the time limit the one further along the route wins. A win pays back
//!   twice the stake (even money).
//! - **Police.** Street racers are `VehicleKind::AiRacer` for the whole session (the kind is
//!   not a replay op, so it cannot change mid-session): the police treat them as racers, as
//!   they treat a picker race's AI. Roaming ones keep to the law, so they draw no heat; a
//!   racing one speeds and runs reds like any racer.
//!
//! **Replays.** The street AI never writes to the sim itself: [`StreetRacers::before_step`]
//! returns the racers' inputs and moves ([`StreetStep`]), and the session logs them (inputs
//! with `log_input`, moves as `OpKind::ResetAt` (+ `Ghost`) ops), as it does for a picker
//! race's AI. A playback (browser, crash replay) takes everything from the log and runs no
//! street AI. Only the session's own street state (offers, the race, the banner) is not in a
//! recording; `simcli replay` drifts on street sessions because it does not mark the racers
//! `AiRacer` (the same gap as picker races, `docs/integration/race.md` row 214).

use std::collections::BTreeMap;

use serde::Serialize;
use simcli::scenario::{AssistSpec, CitySpec, EntitySpec};
use simcli::Scenario;
use wp_contracts::{DamageSim, EntityId, LoadedCar, SimEvent, VehicleInput, VehicleSim};
use wp_math::Rng;
use wp_opponent::{
    CarCapability, CorridorOptions, DriverProfile, Field, OpposingLanes, RaceConfig, RacerSpec,
    RacerStatus, SimPolice, VehicleObs,
};
use wp_sim::{Sim, VehicleKind};
use wp_world::{geom, World};

use crate::data::mem_data;
use crate::run::PlayerRun;

/// Tests: wall-clock time spent in the street AI (`before_step` + `after_step`), for the perf
/// test. Never read by the sim.
#[cfg(test)]
pub(crate) mod perf {
    use std::cell::Cell;

    thread_local! {
        static NS: Cell<u64> = const { Cell::new(0) };
        static MAX_NS: Cell<u64> = const { Cell::new(0) };
    }

    /// Add the time since `t0` (one `before_step` or `after_step` call).
    pub(crate) fn add(t0: std::time::Instant) {
        let ns = t0.elapsed().as_nanos() as u64;
        NS.with(|c| c.set(c.get() + ns));
        MAX_NS.with(|c| c.set(c.get().max(ns)));
    }

    /// Nanoseconds so far and the slowest single call, then reset.
    pub(crate) fn take() -> (u64, u64) {
        (NS.with(|c| c.replace(0)), MAX_NS.with(|c| c.replace(0)))
    }
}

/// Horn within this distance of a roaming racer challenges it, m.
pub const CHALLENGE_RADIUS_M: f32 = 30.0;
/// An open offer lapses when the player drives this far from the racer, m.
pub const OFFER_LEAVE_M: f32 = 70.0;
/// Reaching this close to B finishes the race, m.
pub const FINISH_RADIUS_M: f32 = 18.0;
/// Shortest and longest race the player may set, m (route length).
pub const RACE_MIN_M: f32 = 250.0;
/// See [`RACE_MIN_M`]. Room for a Loop 9 lap from a rest stop back to it (~10.5 km, row 153).
pub const RACE_MAX_M: f32 = 12000.0;
/// A challenge within this distance of a Loop 9 rest stop is a rest-stop meet: the race starts
/// from the rest stop's parking road, both cars side by side, and the card offers a Loop 9 lap
/// back to it (owner row 153), m.
pub const REST_STOP_MEET_M: f32 = 80.0;
/// Lateral offset of each car from the rest stop's lane centre at a rest-stop start, m (the
/// parking road is paved 4.25 m either side of its centre).
const REST_STOP_GRID_M: f32 = 2.0;
/// A loop racer idles this long at a rest stop between laps, s (plus up to
/// [`LOOP_IDLE_EXTRA_S`]).
const LOOP_IDLE_S: u64 = 40;
/// See [`LOOP_IDLE_S`].
const LOOP_IDLE_EXTRA_S: u32 = 50;
/// Passing this close to a waypoint gate counts it, m (in order; B counts only after the
/// last one).
pub const GATE_RADIUS_M: f32 = 20.0;
/// Most extra waypoints a race route takes (owner row 151).
pub const MAX_WAYPOINTS: usize = 8;
/// Countdown before the start, ticks (3 s).
pub const COUNTDOWN_TICKS: u64 = 3 * 240;
/// When a street racer is out (owner row 152: 8 roam by day; at night 13, 4 of them lapping
/// the Loop 9 highway).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Shift {
    /// Roams the city day and night.
    AllDay,
    /// Roams the city at night only ([`is_night`]).
    Night,
    /// Laps Loop 9 at night only (roams the city instead in a city without it).
    Loop,
}

/// The racers a city session spawns, one per entry: level and [`Shift`]. Two per level all
/// day (8), one more roaming at night and four on Loop 9 at night (13 at night).
pub const ROSTER: [(u8, Shift); 13] = [
    (2, Shift::AllDay),
    (2, Shift::AllDay),
    (3, Shift::AllDay),
    (3, Shift::AllDay),
    (4, Shift::AllDay),
    (4, Shift::AllDay),
    (5, Shift::AllDay),
    (5, Shift::AllDay),
    (2, Shift::Night),
    (3, Shift::Loop),
    (4, Shift::Loop),
    (5, Shift::Loop),
    (5, Shift::Loop),
];
/// Cruising top speed of a racer lapping Loop 9, m/s (under the speeding threshold of its
/// 65 mph limit, 29.1 + 2.24 m/s).
pub const LOOP_CRUISE_MPS: f32 = 27.0;
/// On a flat city (no terrain) off-shift racers wait this far past its east edge, m.
const LOCKUP_OFFSET_M: f32 = 600.0;
/// Lockup spots on terrain keep this far from any road segment and building, m.
const LOCKUP_CLEAR_M: f32 = 40.0;
/// Street racers spawn at least this far from the player and from each other, m.
pub const SPAWN_SPACING_M: f32 = 140.0;
/// Scenario entity ids of street racers start with this (`street_<level>_<name>`).
pub const ENTITY_PREFIX: &str = "street_";
/// RNG stream of the street racers, salted from the scenario seed.
const STREET_SEED_SALT: u64 = 0x057E_E7AC;
/// A racer that lost or declined can be challenged again after this many ticks (20 s).
const RECHALLENGE_TICKS: u64 = 20 * 240;
/// A roaming racer standing still this long is stuck (45 s; a red light is far shorter).
const STUCK_TICKS: u64 = 45 * 240;
/// Wrecked or stuck racers are repaired and moved only this far from the player, m.
const RESPAWN_OUT_OF_SIGHT_M: f32 = 150.0;
/// An unanswered offer lapses after this many ticks (2 min).
const OFFER_TIMEOUT_TICKS: u64 = 120 * 240;

/// A driver level: who races at it, how well, and for how much.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Level {
    /// 2 (Street) … 5 (Legend); [`LEVELS`] is the source of truth (there is no level 1).
    pub level: u8,
    /// Display name.
    pub name: &'static str,
    /// Map icon colour (CSS hex).
    pub colour: &'static str,
    /// Car pool (analog names, `data/cars`; never a `debug_*` car).
    pub cars: &'static [&'static str],
    /// Driver skill (`DriverProfile::skill`) when racing.
    pub skill: f32,
    /// Driver aggression when racing.
    pub aggression: f32,
    /// Driver consistency when racing.
    pub consistency: f32,
    /// Appetite for risk when racing (runs red lights, accepts tight traffic gaps).
    pub risk: f32,
    /// Cruising top speed while roaming, m/s (under the speeding threshold of a 25 mph street,
    /// 11.2 + 2.24 m/s, so roaming racers do not draw the police).
    pub cruise_mps: f32,
    /// Smallest bet the racer takes, $.
    pub bet_min: i64,
    /// Largest bet the racer takes, $.
    pub bet_max: i64,
}

/// The four levels, in order (owner, 2026-10-22: the slowest level, Rookie, was dropped; the
/// others keep their numbers 2..=5, so `level` in every JSON is still the table's `level`).
pub const LEVELS: [Level; 4] = [
    Level {
        level: 2,
        name: "Street",
        colour: "#2f81f7",
        cars: &["kestrel_ts", "shiro_turbo"],
        skill: 0.45,
        aggression: 0.4,
        consistency: 0.55,
        risk: 0.35,
        cruise_mps: 12.0,
        bet_min: 250,
        bet_max: 1_200,
    },
    Level {
        level: 3,
        name: "Pro",
        colour: "#a371f7",
        cars: &["lindwurm_4t", "sperber_32", "ironside_440"],
        skill: 0.65,
        aggression: 0.55,
        consistency: 0.7,
        risk: 0.5,
        cruise_mps: 12.5,
        bet_min: 500,
        bet_max: 2_500,
    },
    Level {
        level: 4,
        name: "Elite",
        colour: "#f0883e",
        cars: &["rampart_50", "hakone_gt"],
        skill: 0.82,
        aggression: 0.7,
        consistency: 0.85,
        risk: 0.65,
        cruise_mps: 13.0,
        bet_min: 1_000,
        bet_max: 5_000,
    },
    Level {
        level: 5,
        name: "Legend",
        colour: "#f85149",
        cars: &["lucciola_350", "hakone_gt"],
        skill: 0.97,
        aggression: 0.8,
        consistency: 0.95,
        risk: 0.8,
        cruise_mps: 13.3,
        bet_min: 2_000,
        bet_max: 10_000,
    },
];

/// Street names the racers go by.
const NAMES: [&str; 16] = [
    "Dex", "Mara", "Kito", "Juno", "Rook", "Vega", "Sable", "Ozzy", "Nyx", "Brand", "Lark",
    "Tamsin", "Cruz", "Pike", "Wren", "Halo",
];

/// The level table entry of `level` (clamped to the table's range, 2..=5).
pub fn level(level: u8) -> &'static Level {
    let lo = LEVELS[0].level;
    &LEVELS[(level.clamp(lo, lo + LEVELS.len() as u8 - 1) - lo) as usize]
}

/// It is night for the street racers' shifts (`DayPhase::Night`, 20:00–05:00).
pub fn is_night(sim: &Sim) -> bool {
    sim.world().clock.time.phase() == wp_contracts::DayPhase::Night
}

/// Scenario entity id of a street racer: `street_<level>_<lowercase name>`, with `_night` /
/// `_loop` after it for the night shifts ([`Shift`]).
pub fn entity_id(level: u8, name: &str, shift: Shift) -> String {
    let tail = match shift {
        Shift::AllDay => "",
        Shift::Night => "_night",
        Shift::Loop => "_loop",
    };
    format!("{ENTITY_PREFIX}{level}_{}{tail}", name.to_lowercase())
}

/// Shift of a street racer's scenario entity id ([`entity_id`]; ids without a shift, as in
/// older recordings, are [`Shift::AllDay`]).
pub fn shift_of_entity_id(id: &str) -> Shift {
    if id.ends_with("_night") {
        Shift::Night
    } else if id.ends_with("_loop") {
        Shift::Loop
    } else {
        Shift::AllDay
    }
}

/// Level and name of a street racer's scenario entity id ([`entity_id`]); `None` for any
/// other entity.
pub fn parse_entity_id(id: &str) -> Option<(u8, &'static str)> {
    let id = id
        .strip_suffix("_night")
        .or_else(|| id.strip_suffix("_loop"))
        .unwrap_or(id);
    let (lv, name) = id.strip_prefix(ENTITY_PREFIX)?.split_once('_')?;
    let lv: u8 = lv.parse().ok()?;
    if !LEVELS.iter().any(|l| l.level == lv) {
        return None;
    }
    let name = NAMES.iter().find(|n| n.eq_ignore_ascii_case(name))?;
    Some((lv, name))
}

/// The scenario entity is a street racer.
pub fn is_street_entity(spec: &EntitySpec) -> bool {
    parse_entity_id(&spec.id).is_some()
}

/// A live session of `sc` gets street racers added: a free drive or city chase (any weather)
/// in a city, with the default systems, and no street racers in it yet (a scenario saved from
/// a street session keeps its own). Picker races, menu scenarios and simcli scenario files
/// never get them.
pub fn scenario_wants_racers(sc: &Scenario) -> bool {
    sc.city != CitySpec::Flat
        && sc.systems.is_none()
        && sc.expect.is_empty()
        && (sc.name.starts_with("free_drive") || sc.name.starts_with("city_chase"))
        && !sc.entities.iter().any(is_street_entity)
}

/// The street racers' scenario entities for a session in `world` with the player at
/// `player_xy`, starting at night or not: one per [`ROSTER`] entry, on shift on a random
/// through / highway lane (Loop 9 for a loop racer at night; at least [`SPAWN_SPACING_M`]
/// from the player and from each other; a racer with no such spot left is skipped), off shift
/// in the lockup out of the city. A car from the level's pool, AI assists, no player steering
/// aids. Deterministic in `seed`. Empty when the city has no drivable lanes.
pub fn plan_roster(world: &World, player_xy: [f32; 2], seed: u64, night: bool) -> Vec<EntitySpec> {
    let mut rng = Rng::new(seed ^ STREET_SEED_SALT);
    let mut taken: Vec<[f32; 2]> = vec![player_xy];
    let mut names: Vec<&str> = NAMES.to_vec();
    let mut out = Vec::new();
    let ring = loop9(world);
    let lockup = lockup_slots(world);
    for (k, (lv, shift)) in ROSTER.into_iter().enumerate() {
        let l = level(lv);
        let car = l.cars[rng.below(l.cars.len() as u32) as usize];
        let pose = if shift != Shift::AllDay && !night {
            Some(lockup[k])
        } else if shift == Shift::Loop && !ring.is_empty() {
            loop_lane_pose(&mut rng, &ring, &taken, SPAWN_SPACING_M, SPAWN_SPACING_M)
        } else {
            random_lane_pose(world, &mut rng, &taken, SPAWN_SPACING_M, SPAWN_SPACING_M)
        };
        let Some((pos, yaw)) = pose else {
            continue;
        };
        taken.push(geom::xy(pos));
        let name = names.remove(rng.below(names.len() as u32) as usize);
        let mut spec = crate::scenario::entity(&entity_id(lv, name, shift), car, pos, yaw);
        spec.assists = Some(AssistSpec::Ai);
        spec.player_steering = Some(false);
        out.push(spec);
    }
    out
}

/// Mark the street racers among `sc`'s entities (`ids` in scenario order) as
/// `VehicleKind::AiRacer` and allowed the sim's light model far from the player
/// (`Sim::set_light_when_far`): live, in playback and in crash replays alike (the kind decides
/// how traffic and the police treat a car, so a playback must match).
pub fn mark_racers(sim: &mut Sim, sc: &Scenario, ids: &[EntityId]) {
    for (spec, id) in sc.entities.iter().zip(ids) {
        if is_street_entity(spec) {
            let _ = sim.set_vehicle_kind(*id, VehicleKind::AiRacer);
            // Far from the player they drive the sim's light model (SR-3); a race with the
            // player turns it off for the racer (logged op).
            let _ = sim.set_light_when_far(*id, true);
        }
    }
}

/// The racer's driving profile in a race.
fn race_profile(l: &Level) -> DriverProfile {
    DriverProfile {
        skill: l.skill,
        aggression: l.aggression,
        consistency: l.consistency,
        risk: l.risk,
        awareness: 0.5 + 0.5 * l.skill,
    }
}

/// Calm driving while roaming: low risk (stops at reds), patient, generous gaps, below the
/// lane-splitting skill.
fn roam_profile() -> DriverProfile {
    DriverProfile {
        skill: 0.5,
        aggression: 0.15,
        consistency: 0.9,
        risk: 0.05,
        awareness: 0.9,
    }
}

/// The car's limits cut down to traffic pace for roaming.
fn cruise_capability(car: &LoadedCar, cruise_mps: f32) -> CarCapability {
    CarCapability {
        max_lat_g: 0.4,
        brake_g: 0.75,
        accel_mps2: 2.2,
        top_speed_mps: cruise_mps,
        ..CarCapability::geometry_only(car)
    }
}

/// What a racer is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RacerMode {
    /// Cruising the city.
    Roam,
    /// Pulled over, waiting for the player's answer to a challenge.
    Waiting,
    /// Racing the player.
    Racing,
    /// Off shift ([`Shift`]): parked in the lockup out of the city.
    OffDuty,
}

/// A street racer's car moved on purpose (lined up for a race, or repaired and moved out of
/// sight). The session logs it as `OpKind::ResetAt` (then `OpKind::Ghost` when
/// `ghost_ticks > 0`), so playback repeats it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Move {
    /// The racer's car.
    pub entity: EntityId,
    /// Ground position, world m.
    pub xy: [f32; 2],
    /// Heading, rad about +Z (0 = +Y).
    pub yaw: f32,
    /// Height hint: the road nearest this z.
    pub near_z: f32,
    /// Repair the car fully too.
    pub repair: bool,
    /// Ghost window after the move, ticks (0 = none).
    pub ghost_ticks: u16,
}

/// What the street racers do this tick ([`StreetRacers::before_step`]); the session logs it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StreetStep {
    /// Moves, applied (logged ops) before the inputs.
    pub moves: Vec<Move>,
    /// Every racer's input for this tick.
    pub inputs: Vec<(EntityId, VehicleInput)>,
    /// Off-shift racers settled in the lockup to put to sleep (`Sim::set_dormant`, logged as
    /// `OpKind::Dormant`; SR-3). The sim wakes them itself.
    pub sleep: Vec<EntityId>,
    /// Light-model permission changes (`Sim::set_light_when_far`, logged as
    /// `OpKind::LightWhenFar`): off for a racer racing the player, on again after.
    pub light: Vec<(EntityId, bool)>,
    /// This tick's honk opened a challenge (the session's race spots then ignore it).
    pub challenged: bool,
}

/// One NPC street racer.
struct Racer {
    entity: EntityId,
    level: u8,
    shift: Shift,
    name: String,
    car_id: String,
    mode: RacerMode,
    /// Roam driver (`None` while waiting or racing, or until the next route is planned).
    field: Option<Field>,
    /// Do not plan a new roam route before this tick.
    replan_at: u64,
    /// Cannot be challenged before this tick.
    cooldown_until: u64,
    /// Ticks standing still while roaming.
    idle_ticks: u64,
    /// Wrecked or stuck: repair and move it out of the player's sight at the next plan.
    respawn: bool,
    wins: u32,
    losses: u32,
}

/// An open challenge, waiting for the player's finish and stake (JSON for the testbed).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Offer {
    /// Racer entity.
    pub entity: u32,
    /// Racer name.
    pub name: String,
    /// Level, 2..=5 ([`LEVELS`]).
    pub level: u8,
    /// Level name.
    pub level_name: &'static str,
    /// Level colour.
    pub colour: &'static str,
    /// Racer's car id.
    pub car: String,
    /// Racer's record against the player.
    pub wins: u32,
    /// Racer's losses against the player.
    pub losses: u32,
    /// Smallest stake, $.
    pub bet_min: i64,
    /// Largest stake the player can place now (level maximum capped by cash), $.
    pub bet_max: i64,
    /// Player's cash, $.
    pub cash: i64,
    /// The player can afford the minimum stake.
    pub affordable: bool,
    /// Start (A): where the player is now (the rest stop's parking road at a rest-stop meet).
    pub start: [f32; 2],
    /// The Loop 9 rest stop this meet is at (owner row 153).
    pub rest_stop: Option<String>,
    /// At a rest stop: a Loop 9 lap back to it (B and the waypoints that make the route go
    /// round), for the card's one-tap choice.
    pub loop_lap: Option<LapPlan>,
}

/// A planned Loop 9 lap from a rest stop back to it (`Offer::loop_lap`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LapPlan {
    /// Finish (B): the rest stop.
    pub finish: [f32; 2],
    /// Waypoints round the loop, in order.
    pub via: Vec<[f32; 2]>,
}

/// A planned race route (`street_preview`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Preview {
    /// The route is raceable (length within limits).
    pub ok: bool,
    /// Why not, when not.
    pub reason: Option<String>,
    /// Route length, m.
    pub length_m: f32,
    /// Finish (B) snapped onto the road.
    pub finish: [f32; 2],
    /// The waypoints, in order, snapped onto the route (the race's gates before B).
    pub gates: Vec<[f32; 2]>,
    /// Route centreline (every ~10 m), `[x, y]`.
    pub polyline: Vec<[f32; 2]>,
}

/// The race in progress (JSON for the HUD).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RaceView {
    /// Racer entity.
    pub entity: u32,
    /// Racer name.
    pub name: String,
    /// Level.
    pub level: u8,
    /// Level name.
    pub level_name: &'static str,
    /// Level colour.
    pub colour: &'static str,
    /// Stake, $.
    pub stake: i64,
    /// Seconds to the green light (0 once racing).
    pub countdown_s: f32,
    /// Seconds since the green light.
    pub elapsed_s: f32,
    /// Time limit left, s.
    pub time_left_s: f32,
    /// Route length, m.
    pub length_m: f32,
    /// Player's distance left, m.
    pub player_left_m: f32,
    /// Racer's distance left, m.
    pub racer_left_m: f32,
    /// Finish (B).
    pub finish: [f32; 2],
    /// Start (A).
    pub start: [f32; 2],
    /// Waypoint gates before B, in order.
    pub gates: Vec<[f32; 2]>,
    /// Index of the player's next gate (`gates.len()` = all passed: on to B).
    pub next_gate: usize,
    /// Route centreline (every ~10 m).
    pub polyline: Vec<[f32; 2]>,
}

/// How a race ended (JSON for the result banner).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RaceResult {
    /// 1, 2, … per race this session (the UI shows each once).
    pub seq: u32,
    /// The player won.
    pub won: bool,
    /// Plain-language reason.
    pub reason: String,
    /// Opponent name.
    pub name: String,
    /// Opponent level.
    pub level: u8,
    /// Opponent level name.
    pub level_name: &'static str,
    /// Level colour.
    pub colour: &'static str,
    /// Stake, $.
    pub stake: i64,
    /// Cash change from the race (+stake on a win, -stake on a loss), $.
    pub net: i64,
    /// Cash after, $.
    pub cash_after: i64,
    /// Race time of the winner (or of the race when decided early), s.
    pub time_s: f32,
    /// Distance the loser still had to go, m.
    pub margin_m: f32,
}

/// The race in progress (session-side state; not in a recording).
struct Race {
    racer: usize,
    /// The racer's driver: a full-pace field on the route plus its run-off past B.
    field: Field,
    start: [f32; 2],
    finish: [f32; 3],
    polyline: Vec<[f32; 2]>,
    /// Waypoint gates before B, in order.
    gates: Vec<[f32; 2]>,
    /// The player's next gate.
    next_gate: usize,
    /// [`course_packed`].
    course: Vec<f32>,
    length_m: f32,
    stake: i64,
    go_tick: u64,
    limit_tick: u64,
    /// Player's progress along the route, m.
    player_s: f32,
}

/// Every street racer of a session, the open offer and the race in progress.
pub struct StreetRacers {
    racers: Vec<Racer>,
    rng: Rng,
    offer: Option<usize>,
    race: Option<Race>,
    result: Option<RaceResult>,
    seq: u32,
    /// Tick the open offer was made.
    offer_tick: u64,
    /// Last tick's sim events (contacts and police events for the fields).
    events: Vec<SimEvent>,
    /// Loaded cars per car id.
    cars: BTreeMap<String, LoadedCar>,
    /// Loop 9 (empty in a city without it); `None` until the first step looks it up.
    ring: Option<Loop9>,
    /// Lockup spots ([`lockup_slots`]), looked up at the first shift change.
    lockup: Vec<([f32; 3], f32)>,
    /// Light-model permission changes due next step ([`StreetStep::light`]).
    pending_light: Vec<(EntityId, bool)>,
}

/// The player is wanted or in a pursuit (search included): no street challenges then (owner
/// row 150).
fn pursued(sim: &Sim, player: EntityId) -> bool {
    sim.police().is_wanted_or_pursued(player)
}

/// For each waypoint, the index of the nearest point of the route centreline (searched in
/// order along it, so a route passing near a later waypoint early does not take it).
fn gate_indices(polyline: &[[f32; 3]], via: &[[f32; 2]]) -> Vec<usize> {
    let mut from = 0;
    let mut out = Vec::with_capacity(via.len());
    for w in via {
        let mut best = (f32::INFINITY, from);
        for (i, p) in polyline.iter().enumerate().skip(from) {
            let d = dist(geom::xy(*p), *w);
            if d < best.0 {
                best = (d, i);
            }
        }
        from = best.1;
        out.push(best.1);
    }
    out
}

/// The waypoints moved onto the route ([`gate_indices`]).
fn snap_gates(polyline: &[[f32; 3]], via: &[[f32; 2]]) -> Vec<[f32; 2]> {
    gate_indices(polyline, via)
        .into_iter()
        .filter_map(|i| polyline.get(i).map(|p| geom::xy(*p)))
        .collect()
}

/// Gate width drawn in the world, m (the arch spans the road; `FINISH_RADIUS_M` and
/// [`GATE_RADIUS_M`] decide what counts).
const GATE_WIDTH_M: f32 = 14.0;

/// The street race's course for the 3D gate marker, in the testbed's `race_course()` layout:
/// `[gates, (x, y, z, dx, dy, width) × gates, (x, y, z) × points]`; the gates are the
/// waypoints then B.
fn course_packed(polyline: &[[f32; 3]], via: &[[f32; 2]]) -> Vec<f32> {
    let mut idx = gate_indices(polyline, via);
    if polyline.is_empty() {
        return Vec::new();
    }
    idx.push(polyline.len() - 1);
    let mut out = vec![idx.len() as f32];
    for i in idx {
        let a = polyline[i.saturating_sub(1)];
        let b = polyline[(i + 1).min(polyline.len() - 1)];
        let d = geom::normalize(geom::sub(geom::xy(b), geom::xy(a)));
        let p = polyline[i];
        out.extend_from_slice(&[p[0], p[1], p[2], d[0], d[1], GATE_WIDTH_M]);
    }
    // Every ~10 m, as the map's centreline.
    let mut last: Option<[f32; 3]> = None;
    for p in polyline {
        if last.is_none_or(|q| dist(geom::xy(q), geom::xy(*p)) >= 10.0) {
            out.extend_from_slice(p);
            last = Some(*p);
        }
    }
    out
}

/// A field on `route`, or `None` when the route has nothing to drive. `World::plan_route` can
/// return a one-segment route whose destination projects behind its start; `Field::from_route`
/// panics on it, and in the browser that panic stopped the game for good (owner row 154: a
/// roaming racer planning its next route in Port Vesper at night, any tick, often seen at a
/// race start).
fn field_on(
    world: &World,
    route: &wp_world::Route,
    specs: &[RacerSpec],
    config: RaceConfig,
) -> Option<Field> {
    let corridor = wp_opponent::Corridor::try_from_route(world.map(), route, &config.corridor)?;
    Some(Field::new(corridor, specs, config))
}

/// Horizontal position of a vehicle.
fn xy_of(sim: &Sim, id: EntityId) -> Option<[f32; 2]> {
    sim.vehicles()
        .iter()
        .find(|v| v.id == id)
        .map(|v| geom::xy(v.vehicle.state().pos))
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    geom::dist(a, b)
}

/// Yaw (0 = +Y) of a vehicle's chassis.
fn yaw_of_rot(q: [f32; 4]) -> f32 {
    let fx = 2.0 * (q[0] * q[1] - q[3] * q[2]);
    let fy = 1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]);
    wp_math::atan2(-fx, fy)
}

/// Every ~10 m of a polyline, as `[x, y]`.
fn decimate(points: &[[f32; 3]]) -> Vec<[f32; 2]> {
    let mut out: Vec<[f32; 2]> = Vec::new();
    for p in points {
        let q = geom::xy(*p);
        if out.last().is_none_or(|l| dist(*l, q) >= 10.0) {
            out.push(q);
        }
    }
    if let (Some(l), Some(p)) = (out.last().copied(), points.last()) {
        if dist(l, geom::xy(*p)) > 0.5 {
            out.push(geom::xy(*p));
        }
    }
    out
}

fn obs(sim: &Sim) -> Vec<VehicleObs> {
    sim.vehicles()
        .iter()
        .map(|v| VehicleObs {
            entity: v.id,
            state: v.vehicle.state(),
            effects: v.damage.handling_effects(),
            size_m: [
                v.vehicle.def().body_size_m[0],
                v.vehicle.def().body_size_m[1],
            ],
        })
        .collect()
}

impl StreetRacers {
    /// The street racers of a session built from `sc` (`ids` = its entities in scenario
    /// order): every [`is_street_entity`] entity, roaming. `None` when there are none.
    pub fn new(sc: &Scenario, ids: &[EntityId]) -> Option<Self> {
        let night = wp_contracts::TimeOfDay::from_hours(sc.time_of_day_h).phase()
            == wp_contracts::DayPhase::Night;
        let mut racers = Vec::new();
        for (k, (spec, id)) in sc.entities.iter().zip(ids).enumerate() {
            let Some((lv, name)) = parse_entity_id(&spec.id) else {
                continue;
            };
            let shift = shift_of_entity_id(&spec.id);
            racers.push(Racer {
                entity: *id,
                level: lv,
                shift,
                name: name.to_string(),
                car_id: spec.car.clone(),
                mode: if shift != Shift::AllDay && !night {
                    RacerMode::OffDuty
                } else {
                    RacerMode::Roam
                },
                field: None,
                // Stagger the first route plans over the first ticks.
                replan_at: 1 + k as u64 * 8,
                cooldown_until: 0,
                idle_ticks: 0,
                respawn: false,
                wins: 0,
                losses: 0,
            });
        }
        if racers.is_empty() {
            return None;
        }
        Some(Self {
            racers,
            // A stream of its own (the roster was planned on `seed ^ salt`).
            rng: Rng::new((sc.seed ^ STREET_SEED_SALT).rotate_left(17)),
            offer: None,
            race: None,
            result: None,
            seq: 0,
            offer_tick: 0,
            events: Vec::new(),
            cars: BTreeMap::new(),
            ring: None,
            lockup: Vec::new(),
            pending_light: Vec::new(),
        })
    }

    fn car(&mut self, id: &str) -> Option<&LoadedCar> {
        if !self.cars.contains_key(id) {
            let car = simcli::data::load_car(mem_data(), id).ok()?;
            self.cars.insert(id.to_string(), car);
        }
        self.cars.get(id)
    }

    /// Racer entities (any mode).
    pub fn entities(&self) -> Vec<EntityId> {
        self.racers.iter().map(|r| r.entity).collect()
    }

    /// Hold the player on the handbrake (race countdown).
    pub fn holds_player(&self, tick: u64) -> bool {
        self.race.as_ref().is_some_and(|r| tick < r.go_tick)
    }

    /// Per-racer packed state for the map: `[entity, level, x, y, yaw, mode]` per racer
    /// (mode 0 roam, 1 waiting, 2 racing).
    pub fn packed(&self, sim: &Sim) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.racers.len() * 6);
        for r in self.racers.iter().filter(|r| r.mode != RacerMode::OffDuty) {
            let Some(v) = sim.vehicles().iter().find(|v| v.id == r.entity) else {
                continue;
            };
            let st = v.vehicle.state();
            out.extend_from_slice(&[
                r.entity.0 as f32,
                r.level as f32,
                st.pos[0],
                st.pos[1],
                yaw_of_rot(st.rot),
                match r.mode {
                    RacerMode::Roam => 0.0,
                    RacerMode::Waiting => 1.0,
                    RacerMode::Racing => 2.0,
                    RacerMode::OffDuty => 3.0,
                },
            ]);
        }
        out
    }

    /// Roster for the UI: name, level and record of every racer.
    pub fn roster(&self) -> Vec<RosterEntry> {
        self.racers
            .iter()
            .map(|r| {
                let l = level(r.level);
                RosterEntry {
                    entity: r.entity.0,
                    name: r.name.clone(),
                    level: r.level,
                    level_name: l.name,
                    colour: l.colour,
                    car: r.car_id.clone(),
                    mode: r.mode,
                    shift: r.shift,
                    wins: r.wins,
                    losses: r.losses,
                }
            })
            .collect()
    }

    /// The open offer, priced against the player's cash.
    pub fn offer(&self, sim: &Sim, run: Option<&PlayerRun>) -> Option<Offer> {
        let r = &self.racers[self.offer?];
        let l = level(r.level);
        let cash = run.map_or(0, |run| run.account().cash);
        let player = run.map(|run| run.player());
        let here = player.and_then(|p| xy_of(sim, p)).unwrap_or([0.0, 0.0]);
        let meet = rest_stop_near(sim.world(), here);
        let start = meet.map_or(here, |p| geom::xy(p.pos));
        let ring = self.ring.as_ref().filter(|r| !r.is_empty());
        let loop_lap = meet.zip(ring).map(|(p, ring)| LapPlan {
            finish: geom::xy(p.pos),
            via: lap_waypoints(
                &ring.corners,
                geom::xy(p.pos),
                geom::dir_of_yaw(p.heading_rad),
            ),
        });
        Some(Offer {
            entity: r.entity.0,
            name: r.name.clone(),
            level: r.level,
            level_name: l.name,
            colour: l.colour,
            car: r.car_id.clone(),
            wins: r.wins,
            losses: r.losses,
            bet_min: l.bet_min,
            bet_max: l.bet_max.min(cash),
            cash,
            affordable: cash >= l.bet_min,
            start,
            rest_stop: meet.map(|p| p.name.clone()),
            loop_lap,
        })
    }

    /// The race in progress.
    pub fn race(&self, sim: &Sim) -> Option<RaceView> {
        let race = self.race.as_ref()?;
        let r = &self.racers[race.racer];
        let l = level(r.level);
        let tick = sim.tick();
        let racer_s = race
            .field
            .standings()
            .first()
            .map_or(0.0, |st| st.progress_m.min(race.length_m));
        Some(RaceView {
            entity: r.entity.0,
            name: r.name.clone(),
            level: r.level,
            level_name: l.name,
            colour: l.colour,
            stake: race.stake,
            countdown_s: race.go_tick.saturating_sub(tick) as f32 / 240.0,
            elapsed_s: tick.saturating_sub(race.go_tick) as f32 / 240.0,
            time_left_s: race.limit_tick.saturating_sub(tick) as f32 / 240.0,
            length_m: race.length_m,
            player_left_m: (race.length_m - race.player_s).max(0.0),
            racer_left_m: (race.length_m - racer_s).max(0.0),
            finish: geom::xy(race.finish),
            start: race.start,
            gates: race.gates.clone(),
            next_gate: race.next_gate,
            polyline: race.polyline.clone(),
        })
    }

    /// Tests: racer `k` is roaming and standing at a red light (its driver's speed is limited
    /// by the signal).
    #[cfg(test)]
    pub(crate) fn waiting_at_red(&self, sim: &Sim, k: usize) -> bool {
        let r = &self.racers[k];
        let standing = sim
            .vehicles()
            .iter()
            .find(|v| v.id == r.entity)
            .is_some_and(|v| v.vehicle.state().speed_mps.abs() < 1.0);
        r.mode == RacerMode::Roam
            && standing
            && r.field
                .as_ref()
                .is_some_and(|f| f.debug_of(0).limit == wp_opponent::SpeedLimit::Signal)
    }

    /// The race in progress' gates (waypoints, then B) and route for the 3D gate marker (the
    /// layout of the testbed's `race_course()`); empty without a race.
    pub fn course(&self) -> &[f32] {
        self.race.as_ref().map_or(&[], |r| &r.course)
    }

    /// The latest race result.
    pub fn result(&self) -> Option<&RaceResult> {
        self.result.as_ref()
    }

    /// Nearest racer the player could challenge now (for the "honk to challenge" hint).
    pub fn challengeable(&self, sim: &Sim, player: EntityId) -> Option<RosterEntry> {
        if pursued(sim, player) {
            return None;
        }
        self.in_reach(sim, player)
    }

    /// Nearest roaming racer in honking range with no challenge or race open, police or not
    /// (the hint says to lose the police first).
    pub fn in_reach(&self, sim: &Sim, player: EntityId) -> Option<RosterEntry> {
        if self.offer.is_some() || self.race.is_some() {
            return None;
        }
        let k = self.nearest_roaming(sim, player)?;
        self.roster().into_iter().nth(k)
    }

    fn nearest_roaming(&self, sim: &Sim, player: EntityId) -> Option<usize> {
        let p = xy_of(sim, player)?;
        let tick = sim.tick();
        self.racers
            .iter()
            .enumerate()
            .filter(|(_, r)| r.mode == RacerMode::Roam && tick >= r.cooldown_until)
            .filter_map(|(k, r)| xy_of(sim, r.entity).map(|q| (k, dist(p, q))))
            .filter(|(_, d)| *d <= CHALLENGE_RADIUS_M)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k)
    }

    /// Plan the route from the player through the waypoints `via` (in order) to `finish`
    /// (the offer's B).
    pub fn preview(
        &self,
        sim: &Sim,
        player: EntityId,
        finish: [f32; 2],
        via: &[[f32; 2]],
    ) -> Result<Preview, String> {
        let (route, _) = self.plan_race(sim, player, finish, via)?;
        let len = route.length_m;
        let reason = if len < RACE_MIN_M {
            Some(format!("too close: at least {RACE_MIN_M:.0} m"))
        } else if len > RACE_MAX_M {
            Some(format!("too far: at most {:.0} km", RACE_MAX_M / 1000.0))
        } else {
            None
        };
        let end = route.polyline.last().copied().unwrap_or([0.0; 3]);
        Ok(Preview {
            ok: reason.is_none(),
            reason,
            length_m: len,
            finish: geom::xy(end),
            gates: snap_gates(&route.polyline, via),
            polyline: decimate(&route.polyline),
        })
    }

    fn plan_race(
        &self,
        sim: &Sim,
        player: EntityId,
        finish: [f32; 2],
        via: &[[f32; 2]],
    ) -> Result<(wp_world::Route, [f32; 4]), String> {
        if via.len() > MAX_WAYPOINTS {
            return Err(format!("at most {MAX_WAYPOINTS} waypoints"));
        }
        let v = sim
            .vehicles()
            .iter()
            .find(|v| v.id == player)
            .ok_or("no player car")?;
        let st = v.vehicle.state();
        // At a rest stop the race starts on its parking road, facing its way (row 153).
        let (pos, fwd) = match rest_stop_near(sim.world(), geom::xy(st.pos)) {
            Some(p) => (p.pos, geom::dir_of_yaw(p.heading_rad)),
            None => (st.pos, wp_world::drive::forward_of(st.rot)),
        };
        let z = sim.world().map().ground_height(finish, Some(pos[2]));
        let route = sim
            .world()
            .plan_route_via(pos, Some(fwd), via, [finish[0], finish[1], z])
            .ok_or(if via.is_empty() {
                "no road route to that point"
            } else {
                "no road route through those waypoints"
            })?;
        Ok((route, st.rot))
    }

    /// Accept the open offer: race through the waypoints `via` (in order) to `finish` for
    /// `stake`. Pays the stake and starts the countdown; returns the moves that put both cars
    /// side by side at the start, at rest, facing along the route (owner row 150; the session
    /// logs them). Refused while the police are after the player. Nothing changes on an error.
    pub fn accept(
        &mut self,
        sim: &Sim,
        run: &mut PlayerRun,
        finish: [f32; 2],
        via: &[[f32; 2]],
        stake: i64,
    ) -> Result<Vec<Move>, String> {
        let k = self.offer.ok_or("no open challenge")?;
        if run.is_over() {
            return Err("your run is over".into());
        }
        let player = run.player();
        if pursued(sim, player) {
            return Err("lose the police first".into());
        }
        let l = *level(self.racers[k].level);
        let cash = run.account().cash;
        if stake < l.bet_min || stake > l.bet_max {
            return Err(format!(
                "{} {} races for ${} to ${}",
                l.name, self.racers[k].name, l.bet_min, l.bet_max
            ));
        }
        if stake > cash {
            return Err(format!("not enough cash (${cash})"));
        }
        let (route, _) = self.plan_race(sim, player, finish, via)?;
        if !(RACE_MIN_M..=RACE_MAX_M).contains(&route.length_m) {
            return Err(format!(
                "race length must be {RACE_MIN_M:.0} m to {:.0} km",
                RACE_MAX_M / 1000.0
            ));
        }
        let car_id = self.racers[k].car_id.clone();
        let cap = CarCapability::for_car(
            self.car(&car_id)
                .ok_or_else(|| format!("unknown car {car_id}"))?,
        );
        let entity = self.racers[k].entity;
        // As a picker race's AI (`wp_race::entrants`): the corridor runs on past B so the
        // racer drives through the finish; past `finish_m` it is done racing.
        let ai_route =
            wp_race::entrants::run_off_route(sim.world(), &route).unwrap_or_else(|| route.clone());
        let config = RaceConfig {
            laps: 1,
            seed: self.rng.next_u64(),
            // The owner's rule (2026-10-21): a racer never gives up a race for the police.
            allow_abandon: false,
            // The session decides the race; the AI does not quit over a civilian.
            civilian_contact_dq: false,
            cooldown_speed_mps: 8.0,
            corridor: CorridorOptions {
                opposing: OpposingLanes::ForOvertaking,
                ..CorridorOptions::default()
            },
            signals: true,
            finish_m: Some(route.length_m),
            ..RaceConfig::default()
        };
        let spec = RacerSpec {
            entity,
            profile: race_profile(&l),
            capability: cap,
        };
        let field = field_on(sim.world(), &ai_route, &[spec], config)
            .ok_or("no road route to that point")?;
        // Both cars side by side at the start, at rest, facing along the route: the player on
        // the corridor where they stand (kept inside the road), the racer beside them on the
        // roomier side, else right behind (owner row 150).
        let pxy = xy_of(sim, player).ok_or("no player car")?;
        let pz = sim
            .vehicles()
            .iter()
            .find(|v| v.id == player)
            .map_or(0.0, |v| v.vehicle.state().pos[2]);
        let c = field.corridor();
        let pp = c.project(pxy, None, 0);
        let (bl, br) = c.bounds(pp.s);
        let half = 0.5 * cap.width_m;
        let gap = half + 2.4;
        let tangent = c
            .sample((pp.s / wp_opponent::CORRIDOR_STEP_M) as isize)
            .tangent;
        let yaw = geom::yaw_of(tangent);
        let (lo, hi) = (-(bl - half - 0.3), br - half - 0.3);
        let pn = if lo <= hi { pp.n.clamp(lo, hi) } else { pp.n };
        let mut start = geom::xy(c.point(pp.s, pn));
        // The racer's side: right if it fits, else left, where the opposing lanes count (a
        // street with one lane each way puts it on the other lane at the start).
        let (left_pass, _) = c.pass_bounds(pp.s);
        let pos = if pn + gap <= br - half {
            geom::xy(c.point(pp.s, pn + gap))
        } else if pn - gap >= -(bl.max(left_pass) - half) {
            geom::xy(c.point(pp.s, pn - gap))
        } else {
            let d = cap.length_m + 3.0;
            [start[0] - tangent[0] * d, start[1] - tangent[1] * d]
        };
        // A rest-stop meet starts side by side on its parking road (row 153): the player right
        // of the lane centre, the racer left, both facing its way.
        let (pos, yaw) = match rest_stop_near(sim.world(), pxy) {
            Some(p) => {
                let f = geom::dir_of_yaw(p.heading_rad);
                let r = geom::right_of(f);
                let o = geom::xy(p.pos);
                let g = REST_STOP_GRID_M;
                start = [o[0] + r[0] * g, o[1] + r[1] * g];
                ([o[0] - r[0] * g, o[1] - r[1] * g], p.heading_rad)
            }
            None => (pos, yaw),
        };
        run.wager(
            -stake,
            &format!("street race stake vs {} ({})", self.racers[k].name, l.name),
        )?;
        let tick = sim.tick();
        let go_tick = tick + COUNTDOWN_TICKS;
        // Generous: an average of 8 m/s plus a minute.
        let limit_tick = go_tick + ((route.length_m / 8.0 + 60.0) * 240.0) as u64;
        let r = &mut self.racers[k];
        r.mode = RacerMode::Racing;
        r.field = None;
        // Full physics for the whole race, however far ahead it gets.
        self.pending_light.push((r.entity, false));
        self.offer = None;
        let gates = snap_gates(&route.polyline, via);
        self.race = Some(Race {
            racer: k,
            field,
            start,
            finish: route.polyline.last().copied().unwrap_or([0.0; 3]),
            polyline: decimate(&route.polyline),
            gates,
            next_gate: 0,
            course: course_packed(&route.polyline, via),
            length_m: route.length_m,
            stake,
            go_tick,
            limit_tick,
            player_s: 0.0,
        });
        let at = |entity, xy| Move {
            entity,
            xy,
            yaw,
            near_z: rest_stop_near(sim.world(), pxy).map_or(pz, |p| p.pos[2]),
            repair: false,
            ghost_ticks: 0,
        };
        Ok(vec![at(player, start), at(entity, pos)])
    }

    /// Debug / e2e: park roaming racer `entity` idle at rest stop `name` (as at the end of a
    /// loop lap) for two minutes; returns the move that puts it there (the session logs it).
    pub fn park_at_rest_stop(
        &mut self,
        sim: &Sim,
        entity: EntityId,
        name: &str,
    ) -> Result<Move, String> {
        let poi = sim
            .world()
            .rest_stop(name)
            .ok_or_else(|| format!("no rest stop {name}"))?;
        let r = self
            .racers
            .iter_mut()
            .find(|r| r.entity == entity && r.mode == RacerMode::Roam)
            .ok_or("not a roaming street racer")?;
        r.field = None;
        r.replan_at = sim.tick() + 120 * 240;
        Ok(Move {
            entity,
            xy: geom::xy(poi.pos),
            yaw: poi.heading_rad,
            near_z: poi.pos[2],
            repair: false,
            ghost_ticks: 0,
        })
    }

    /// Decline (or withdraw) the open offer: the racer drives off.
    pub fn decline(&mut self, tick: u64) {
        if let Some(k) = self.offer.take() {
            let r = &mut self.racers[k];
            r.mode = RacerMode::Roam;
            r.field = None;
            r.replan_at = tick;
            r.cooldown_until = tick + RECHALLENGE_TICKS;
        }
    }

    /// A challenge is open or a street race is running (no other challenge then: the
    /// session's race spots ignore the horn too).
    pub fn busy(&self) -> bool {
        self.offer.is_some() || self.race.is_some()
    }

    /// Before the sim step: horn challenges, offers lapsing, roam routes, and every racer's
    /// input and move for this tick. `honk` is the horn's rising edge this tick (the session
    /// owns the one horn edge every challenge reads). Reads the sim only; the session logs
    /// and applies the result.
    pub fn before_step(
        &mut self,
        sim: &Sim,
        player: EntityId,
        honk: bool,
        run_over: bool,
    ) -> StreetStep {
        #[cfg(test)]
        let t0 = std::time::Instant::now();
        let tick = sim.tick();
        let mut out = StreetStep::default();
        let chased = pursued(sim, player);
        if honk && !run_over && !chased && self.offer.is_none() && self.race.is_none() {
            if let Some(k) = self.nearest_roaming(sim, player) {
                let r = &mut self.racers[k];
                r.mode = RacerMode::Waiting;
                r.field = None;
                self.offer = Some(k);
                self.offer_tick = tick;
                out.challenged = true;
            }
        }
        if let Some(k) = self.offer {
            let far = match (xy_of(sim, player), xy_of(sim, self.racers[k].entity)) {
                (Some(p), Some(q)) => dist(p, q) > OFFER_LEAVE_M,
                _ => true,
            };
            if far || run_over || chased || tick >= self.offer_tick + OFFER_TIMEOUT_TICKS {
                self.decline(tick);
            }
        }
        if self.ring.is_none() {
            self.ring = Some(loop9(sim.world()));
        }
        self.shift_change(sim, player, &mut out.moves);
        self.lockup_sleep(sim, player, &mut out.sleep);
        out.light = std::mem::take(&mut self.pending_light);
        // Plan roam routes that are due (at most one per tick: the line optimiser is not free).
        if let Some(k) = (0..self.racers.len()).find(|&k| {
            let r = &self.racers[k];
            r.mode == RacerMode::Roam && r.field.is_none() && tick >= r.replan_at
        }) {
            self.plan_roam(sim, player, k, &mut out.moves);
        }
        let obs = obs(sim);
        let police = SimPolice(sim.police());
        for r in &mut self.racers {
            match r.mode {
                RacerMode::Waiting | RacerMode::OffDuty => out.inputs.push((r.entity, stopped())),
                RacerMode::Roam => match &mut r.field {
                    Some(f) => {
                        let input = f
                            .tick(sim.world(), &obs, &self.events, &police)
                            .first()
                            .map_or_else(stopped, |(_, i)| *i);
                        let v = obs
                            .iter()
                            .find(|o| o.entity == r.entity)
                            .map_or(0.0, |o| o.state.speed_mps);
                        out.inputs
                            .push((r.entity, red_light_guard(sim.world(), f, v, input)));
                    }
                    None => out.inputs.push((r.entity, stopped())),
                },
                RacerMode::Racing => {}
            }
        }
        if let Some(race) = &mut self.race {
            let e = self.racers[race.racer].entity;
            if tick < race.go_tick {
                out.inputs.push((e, stopped()));
            } else {
                out.inputs.extend_from_slice(race.field.tick(
                    sim.world(),
                    &obs,
                    &self.events,
                    &police,
                ));
            }
        }
        #[cfg(test)]
        perf::add(t0);
        out
    }

    /// Shift changes (owner row 152): at most one racer a tick, and only out of the player's
    /// sight. By day a night-shift racer that is roaming goes to the lockup; at night one in the
    /// lockup comes out (repaired, ghosted a moment) on a lane, a loop racer on Loop 9. A racer
    /// waiting on a challenge or racing finishes that first.
    fn shift_change(&mut self, sim: &Sim, player: EntityId, moves: &mut Vec<Move>) {
        let night = is_night(sim);
        let p = xy_of(sim, player).unwrap_or([0.0; 2]);
        let hidden = |q: Option<[f32; 2]>| q.is_none_or(|q| dist(p, q) >= RESPAWN_OUT_OF_SIGHT_M);
        let Some(k) = self.racers.iter().position(|r| {
            r.shift != Shift::AllDay
                && ((night && r.mode == RacerMode::OffDuty)
                    || (!night && r.mode == RacerMode::Roam && hidden(xy_of(sim, r.entity))))
        }) else {
            return;
        };
        let tick = sim.tick();
        if night {
            let Some((pos, yaw)) = self.duty_pose(sim, k, p) else {
                return;
            };
            moves.push(Move {
                entity: self.racers[k].entity,
                xy: geom::xy(pos),
                yaw,
                near_z: pos[2],
                repair: true,
                ghost_ticks: crate::reset::GHOST_TICKS,
            });
            let r = &mut self.racers[k];
            r.mode = RacerMode::Roam;
            r.respawn = false;
            r.idle_ticks = 0;
            r.replan_at = tick + 1;
        } else {
            if self.lockup.is_empty() {
                self.lockup = lockup_slots(sim.world());
            }
            let (pos, yaw) = self.lockup[k];
            moves.push(Move {
                entity: self.racers[k].entity,
                xy: geom::xy(pos),
                yaw,
                near_z: pos[2],
                repair: true,
                ghost_ticks: 0,
            });
            let r = &mut self.racers[k];
            r.mode = RacerMode::OffDuty;
            r.field = None;
            r.respawn = false;
        }
    }

    /// Off-shift racers that stand still in the lockup, with the player well away, go to sleep
    /// (they cost no vehicle physics there; SR-3).
    fn lockup_sleep(&self, sim: &Sim, player: EntityId, sleep: &mut Vec<EntityId>) {
        let p = xy_of(sim, player).unwrap_or([0.0; 2]);
        for r in self.racers.iter().filter(|r| r.mode == RacerMode::OffDuty) {
            if sim.is_dormant(r.entity) != Some(false) {
                continue;
            }
            let Some(v) = sim.vehicles().iter().find(|v| v.id == r.entity) else {
                continue;
            };
            let st = v.vehicle.state();
            let far = dist(p, geom::xy(st.pos)) > wp_sim::DORMANT_WAKE_M + 60.0;
            if far && st.speed_mps.abs() < 0.05 {
                sleep.push(r.entity);
            }
        }
    }

    /// Where racer `k` comes (back) out, out of sight of the player at `p`: Loop 9 for a loop
    /// racer in a city with it, else a random lane.
    fn duty_pose(&mut self, sim: &Sim, k: usize, p: [f32; 2]) -> Option<([f32; 3], f32)> {
        let ring = self.ring.clone().unwrap_or_default();
        if self.racers[k].shift == Shift::Loop && !ring.is_empty() {
            loop_lane_pose(&mut self.rng, &ring, &[p], RESPAWN_OUT_OF_SIGHT_M, 0.0)
        } else {
            random_lane_pose(
                sim.world(),
                &mut self.rng,
                &[p],
                RESPAWN_OUT_OF_SIGHT_M,
                0.0,
            )
        }
    }

    fn plan_roam(&mut self, sim: &Sim, player: EntityId, k: usize, moves: &mut Vec<Move>) {
        let e = self.racers[k].entity;
        let tick = sim.tick();
        // A wrecked or stuck racer is repaired and moved somewhere else, out of sight. The
        // route is planned on a later tick, from where the move put it.
        if self.racers[k].respawn {
            let p = xy_of(sim, player).unwrap_or([0.0; 2]);
            let far = xy_of(sim, e).is_none_or(|q| dist(p, q) >= RESPAWN_OUT_OF_SIGHT_M);
            if far {
                if let Some((pos, yaw)) = self.duty_pose(sim, k, p) {
                    moves.push(Move {
                        entity: e,
                        xy: geom::xy(pos),
                        yaw,
                        near_z: pos[2],
                        repair: true,
                        // As a reset in place: it cannot land stuck in (or be hit by) traffic.
                        ghost_ticks: crate::reset::GHOST_TICKS,
                    });
                    let r = &mut self.racers[k];
                    r.respawn = false;
                    r.idle_ticks = 0;
                    r.replan_at = tick + 1;
                    return;
                }
            }
        }
        let Some(v) = sim.vehicles().iter().find(|v| v.id == e) else {
            self.racers[k].replan_at = tick + 240;
            return;
        };
        let st = v.vehicle.state();
        let here = geom::xy(st.pos);
        let fwd = wp_world::drive::forward_of(st.rot);
        let l = *level(self.racers[k].level);
        let car_id = self.racers[k].car_id.clone();
        let Some(car) = self.car(&car_id).cloned() else {
            self.racers[k].replan_at = tick + 3 * 240;
            return;
        };
        let ring = self.ring.clone().unwrap_or_default();
        let on_loop = self.racers[k].shift == Shift::Loop && !ring.is_empty();
        if on_loop {
            // One lap of Loop 9 from here, into one of its rest stops (row 153), where it idles a
            // while before the next.
            let stops: Vec<[f32; 3]> = sim.world().rest_stops().map(|p| p.pos).collect();
            let end =
                (!stops.is_empty()).then(|| stops[self.rng.below(stops.len() as u32) as usize]);
            let Some(route) = loop_lap(sim.world(), st.pos, fwd, &ring.corners, end) else {
                self.racers[k].replan_at = tick + 3 * 240;
                return;
            };
            let spec = RacerSpec {
                entity: e,
                profile: roam_profile(),
                capability: cruise_capability(&car, LOOP_CRUISE_MPS),
            };
            let config = roam_config(self.rng.next_u64());
            match field_on(sim.world(), &route, &[spec], config) {
                Some(f) => self.racers[k].field = Some(f),
                None => self.racers[k].replan_at = tick + 3 * 240,
            }
            return;
        }
        // A destination 300–1200 m away (cheap draws), then at most one route plan this tick:
        // a plan that fails is retried a quarter second later, not now.
        let dest = (0..6).find_map(|_| {
            random_lane_pose(sim.world(), &mut self.rng, &[], 0.0, 0.0)
                .map(|(p, _)| p)
                .filter(|p| (300.0..=1200.0).contains(&dist(here, geom::xy(*p))))
        });
        let Some(dest) = dest else {
            // Nowhere to go from here right now: try again in a few seconds.
            self.racers[k].replan_at = tick + 3 * 240;
            return;
        };
        let Some(route) = sim
            .world()
            .plan_route(st.pos, Some(fwd), dest)
            .filter(|r| r.length_m >= 150.0)
        else {
            self.racers[k].replan_at = tick + 60;
            return;
        };
        let config = roam_config(self.rng.next_u64());
        let spec = RacerSpec {
            entity: e,
            profile: roam_profile(),
            capability: cruise_capability(&car, l.cruise_mps),
        };
        match field_on(sim.world(), &route, &[spec], config) {
            Some(f) => self.racers[k].field = Some(f),
            // Nothing to drive on that route: another destination a quarter second later.
            None => self.racers[k].replan_at = tick + 60,
        }
    }

    /// After the sim step: roam routes that ended, and the race (finish, wreck, capture, time
    /// limit, payout).
    pub fn after_step(&mut self, sim: &Sim, events: &[SimEvent], run: Option<&mut PlayerRun>) {
        self.events.clear();
        self.events.extend_from_slice(events);
        #[cfg(test)]
        let t0 = std::time::Instant::now();
        let tick = sim.tick();
        for r in &mut self.racers {
            if r.mode != RacerMode::Roam {
                r.idle_ticks = 0;
                continue;
            }
            let Some(f) = &r.field else {
                continue;
            };
            let moving = sim
                .vehicles()
                .iter()
                .find(|v| v.id == r.entity)
                .is_some_and(|v| v.vehicle.state().speed_mps.abs() > 0.5);
            r.idle_ticks = if moving { 0 } else { r.idle_ticks + 1 };
            let status = f.standings().first().map(|s| (s.status, s.progress_m));
            let arrived = status.is_some_and(|(st, progress)| {
                matches!(st, RacerStatus::Finished { .. })
                    || (st == RacerStatus::Racing && f.corridor().length_m - progress < 30.0)
            });
            if arrived {
                r.field = None;
                r.replan_at = tick;
                // A loop racer idles at the rest stop its lap ended in (row 153).
                let at_stop =
                    xy_of(sim, r.entity).is_some_and(|q| rest_stop_near(sim.world(), q).is_some());
                if r.shift == Shift::Loop && at_stop {
                    r.replan_at =
                        tick + (LOOP_IDLE_S + u64::from(self.rng.below(LOOP_IDLE_EXTRA_S))) * 240;
                }
            } else if f.race_over() || r.idle_ticks > STUCK_TICKS {
                // Wrecked (retired), caught or stuck: pause, then repair and move it.
                r.field = None;
                r.respawn = true;
                r.replan_at = tick + 10 * 240;
            }
        }
        self.judge_race(sim, events, run);
        #[cfg(test)]
        perf::add(t0);
    }

    fn judge_race(&mut self, sim: &Sim, events: &[SimEvent], run: Option<&mut PlayerRun>) {
        let Some(run) = run else {
            return;
        };
        let Some(race) = &mut self.race else {
            return;
        };
        let tick = sim.tick();
        let player = run.player();
        // Progress along the route; the player may take any path, so off the route (a
        // shortcut) it is the straight-line distance still to go.
        if let Some(p) = xy_of(sim, player) {
            let pp = race.field.corridor().project(p, None, 0);
            race.player_s = if pp.n.abs() <= 25.0 {
                pp.s.min(race.length_m)
            } else {
                (race.length_m - dist(p, geom::xy(race.finish))).max(0.0)
            };
        }
        let st = race.field.standings();
        let npc = st.first();
        let racer_s = npc.map_or(0.0, |s| s.progress_m.min(race.length_m));
        let elapsed = tick.saturating_sub(race.go_tick) as f32 / 240.0;
        if let Some(p) = xy_of(sim, player) {
            while race
                .gates
                .get(race.next_gate)
                .is_some_and(|g| dist(p, *g) <= GATE_RADIUS_M)
            {
                race.next_gate += 1;
            }
        }
        // B counts only once every waypoint gate is passed, in order.
        let near_b = race.next_gate >= race.gates.len()
            && xy_of(sim, player)
                .is_some_and(|p| dist(p, geom::xy(race.finish)) <= FINISH_RADIUS_M);
        let player_wrecked = events
            .iter()
            .any(|e| matches!(e, SimEvent::TotalLoss { vehicle, .. } if *vehicle == player));
        // The session judges the racer's finish as it judges the player's: at B (or past it
        // along the route). The field only reports `Finished` at the end of its run-off, 60 m
        // further on at cool-down pace, as a picker race's AI (whose finish `wp_race` judges).
        let racer_entity = self.racers[race.racer].entity;
        let racer_at_b = npc.is_some_and(|s| {
            matches!(s.status, RacerStatus::Racing | RacerStatus::Finished { .. })
                && (s.progress_m >= race.length_m
                    || xy_of(sim, racer_entity)
                        .is_some_and(|q| dist(q, geom::xy(race.finish)) <= FINISH_RADIUS_M))
        });
        let outcome: Option<(bool, String)> = if tick <= race.go_tick {
            None
        } else if run.is_over() || player_wrecked {
            Some((false, "you were stopped (wrecked or caught)".into()))
        } else if near_b {
            Some((true, "you crossed the line first".into()))
        } else if racer_at_b {
            Some((false, "they crossed the line first".into()))
        } else {
            match npc.map(|s| s.status) {
                Some(RacerStatus::Finished { .. }) => {
                    Some((false, "they crossed the line first".into()))
                }
                Some(RacerStatus::Retired) => Some((true, "their car was wrecked".into())),
                Some(RacerStatus::Busted) => Some((true, "the police caught them".into())),
                // Not in a race (`allow_abandon` is off), kept for completeness.
                Some(RacerStatus::Fled) => Some((true, "they ran from the police".into())),
                Some(RacerStatus::Disqualified) => Some((true, "they were disqualified".into())),
                _ if tick >= race.limit_tick => {
                    let won = race.player_s >= racer_s;
                    Some((won, "time limit: whoever was further along wins".into()))
                }
                _ => None,
            }
        };
        let Some((won, reason)) = outcome else {
            return;
        };
        let stake = race.stake;
        let margin_m = if won {
            race.length_m - racer_s
        } else {
            race.length_m - race.player_s
        }
        .max(0.0);
        let k = race.racer;
        self.race = None;
        let r = &mut self.racers[k];
        self.pending_light.push((r.entity, true));
        let l = level(r.level);
        if won {
            r.losses += 1;
            // Even money: the stake back plus the racer's.
            let _ = run.wager(
                2 * stake,
                &format!("street race won vs {} ({})", r.name, l.name),
            );
        } else {
            r.wins += 1;
        }
        r.mode = RacerMode::Roam;
        r.field = None;
        r.replan_at = tick + 4 * 240;
        r.cooldown_until = tick + RECHALLENGE_TICKS;
        self.seq += 1;
        self.result = Some(RaceResult {
            seq: self.seq,
            won,
            reason,
            name: r.name.clone(),
            level: r.level,
            level_name: l.name,
            colour: l.colour,
            stake,
            net: if won { stake } else { -stake },
            cash_after: run.account().cash,
            time_s: elapsed,
            margin_m,
        });
    }
}

/// One roster line (JSON for the UI).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RosterEntry {
    /// Entity.
    pub entity: u32,
    /// Name.
    pub name: String,
    /// Level.
    pub level: u8,
    /// Level name.
    pub level_name: &'static str,
    /// Level colour.
    pub colour: &'static str,
    /// Car id.
    pub car: String,
    /// What the racer is doing.
    pub mode: RacerMode,
    /// When the racer is out.
    pub shift: Shift,
    /// Races won against the player.
    pub wins: u32,
    /// Races lost against the player.
    pub losses: u32,
}

/// The red-light guard holds a car this close to the stop line, m.
const GUARD_HOLD_M: f32 = 1.0;
/// The guard's gentle stop at the line: the speed it allows at distance `d` falls to zero at
/// the line at this deceleration, m/s².
const GUARD_DECEL_MPS2: f32 = 2.0;
/// Most throttle while rolling up to a red line.
const GUARD_CREEP_THROTTLE: f32 = 0.35;

/// Roaming racers stop for red (and catchable amber) lights. The race AI caps its speed for a
/// red too, but a cruise-paced car that lags its falling target ends up "unable to stop" and
/// commits through the junction; this brakes to the stop line of the route's next signalled
/// junction entry instead, rolls up to it when it stopped (or spawned) short of it, and holds
/// the car there on the handbrake (a held brake pedal at a standstill would engage reverse).
fn red_light_guard(
    world: &wp_world::World,
    f: &Field,
    v: f32,
    input: VehicleInput,
) -> VehicleInput {
    use wp_contracts::SignalState;
    let c = f.corridor();
    let s = f.debug_of(0).s;
    let Some(&(j, lane)) = c
        .junction_lanes
        .iter()
        .find(|(j, _)| *j as f32 * wp_opponent::CORRIDOR_STEP_M > s - 1.0)
    else {
        return input;
    };
    let d = j as f32 * wp_opponent::CORRIDOR_STEP_M - s - 3.5;
    if d > 80.0 {
        return input;
    }
    let Some((state, _)) = world.map().lane_signal(lane) else {
        return input;
    };
    // Deceleration needed to stop at the line.
    let need = if d > 0.3 {
        v * v / (2.0 * d)
    } else {
        f32::INFINITY
    };
    let stop = match state {
        // Too close to stop without slamming the brakes: carry on through (it just changed).
        // Every red holds until the green: a red's time left runs only to the end of the
        // current phase, and the next phase may be red too.
        SignalState::Red => need < 7.0,
        SignalState::Yellow => need < 3.5,
        SignalState::Green | SignalState::Off => false,
    };
    if !stop {
        return input;
    }
    if d <= GUARD_HOLD_M {
        // At the line: brake to a halt, then hold it on the handbrake.
        return if v < 0.3 {
            VehicleInput {
                throttle: 0.0,
                brake: 0.0,
                handbrake: 1.0,
                ..input
            }
        } else {
            VehicleInput {
                throttle: 0.0,
                brake: 1.0,
                ..input
            }
        };
    }
    // Short of the line: roll up to it (queueing as traffic does, so the next green is not
    // spent closing a gap), no faster than a gentle stop at the line allows. The field's own
    // input still brakes for a car queued ahead.
    let v_allow = (2.0 * GUARD_DECEL_MPS2 * (d - GUARD_HOLD_M)).sqrt();
    if v <= v_allow {
        return VehicleInput {
            throttle: input.throttle.min(GUARD_CREEP_THROTTLE),
            ..input
        };
    }
    VehicleInput {
        throttle: 0.0,
        brake: (need / 7.0 + 0.1).clamp(0.1, 1.0).max(input.brake),
        ..input
    }
}

/// Standing still: the handbrake (a held brake pedal at a standstill would engage reverse),
/// headlights on as the racing AI drives.
fn stopped() -> VehicleInput {
    VehicleInput {
        handbrake: 1.0,
        headlights: true,
        ..Default::default()
    }
}

/// A roaming racer's field settings: calm, never on the opposing lanes (no overtakes there),
/// stops for signals.
fn roam_config(seed: u64) -> RaceConfig {
    RaceConfig {
        laps: 1,
        seed,
        allow_abandon: false,
        civilian_contact_dq: false,
        cooldown_speed_mps: 6.0,
        corridor: CorridorOptions {
            opposing: OpposingLanes::Never,
            ..CorridorOptions::default()
        },
        signals: true,
        ..RaceConfig::default()
    }
}

/// Loop 9 in a city that has it: its corners in driving order and the road line round it.
#[derive(Debug, Clone, Default)]
struct Loop9 {
    /// Corners in driving order (the `loop9_circuit` picker race's polygon).
    corners: Vec<[f32; 2]>,
    /// The circuit's centreline (`World::plan_circuit` through the corners).
    line: Vec<[f32; 3]>,
}

impl Loop9 {
    fn is_empty(&self) -> bool {
        self.corners.is_empty() || self.line.len() < 2
    }
}

/// [`Loop9`] when the city has the highway (every corner within 40 m of a highway lane and
/// the circuit plans); empty otherwise.
fn loop9(world: &World) -> Loop9 {
    let corners = loop9_corners(world);
    if corners.is_empty() {
        return Loop9::default();
    }
    match world.plan_circuit(&corners) {
        Some(r) => Loop9 {
            corners,
            line: r.polyline,
        },
        None => Loop9::default(),
    }
}

/// Loop 9's corners when every one is within 40 m of a highway lane; empty otherwise.
fn loop9_corners(world: &World) -> Vec<[f32; 2]> {
    use wp_world::network::LaneKind;
    let Some(poly) = wp_race::catalog::races()
        .into_iter()
        .find_map(|r| match r.course {
            wp_race::catalog::CourseSpec::Circuit { polygon } if r.id == "loop9_circuit" => {
                Some(polygon.to_vec())
            }
            _ => None,
        })
    else {
        return Vec::new();
    };
    let lanes = &world.network().lanes;
    let on_highway = |q: [f32; 2]| {
        lanes
            .iter()
            .filter(|l| l.kind == LaneKind::Highway)
            .any(|l| l.polyline.iter().any(|p| dist(geom::xy(*p), q) < 40.0))
    };
    if poly.iter().all(|q| on_highway(*q)) {
        poly
    } else {
        Vec::new()
    }
}

/// One lap of Loop 9 (`ring`, corners in driving order) from `pos` heading `fwd`: through the
/// corners from the one ahead round to the one before it, then back to where it started.
/// One lap of Loop 9 (`ring`, corners in driving order) from `pos` heading `fwd`: through the
/// corners from the one ahead round to the one before it, then to `end` (a rest stop) or back
/// to where it started.
fn loop_lap(
    world: &World,
    pos: [f32; 3],
    fwd: [f32; 2],
    ring: &[[f32; 2]],
    end: Option<[f32; 3]>,
) -> Option<wp_world::Route> {
    let here = geom::xy(pos);
    let n = ring.len();
    // The corner ahead: nearest corner whose direction agrees with the heading.
    let ahead = (0..n)
        .filter(|&i| {
            let d = geom::sub(ring[i], here);
            d[0] * fwd[0] + d[1] * fwd[1] > 0.0
        })
        .min_by(|&a, &b| dist(ring[a], here).total_cmp(&dist(ring[b], here)))
        .unwrap_or(0);
    let via: Vec<[f32; 2]> = (0..n).map(|j| ring[(ahead + j) % n]).collect();
    let fin = end.unwrap_or_else(|| {
        let back = [here[0] - fwd[0] * 40.0, here[1] - fwd[1] * 40.0];
        [
            back[0],
            back[1],
            world.map().ground_height(back, Some(pos[2])),
        ]
    });
    world
        .plan_route_via(pos, Some(fwd), &via, fin)
        .filter(|r| r.length_m > 1000.0)
}

/// The index of the Loop 9 corner ahead of `here` facing `fwd` (nearest in front).
fn corner_ahead(ring: &[[f32; 2]], here: [f32; 2], fwd: [f32; 2]) -> usize {
    (0..ring.len())
        .filter(|&i| {
            let d = geom::sub(ring[i], here);
            d[0] * fwd[0] + d[1] * fwd[1] > 0.0
        })
        .min_by(|&a, &b| dist(ring[a], here).total_cmp(&dist(ring[b], here)))
        .unwrap_or(0)
}

/// Waypoints that take a race from `here` (facing `fwd`) once round Loop 9: three corners
/// spread round the ring, the last one just before where it started.
fn lap_waypoints(ring: &[[f32; 2]], here: [f32; 2], fwd: [f32; 2]) -> Vec<[f32; 2]> {
    let n = ring.len();
    let a = corner_ahead(ring, here, fwd);
    [n / 4, n / 2, n - 1]
        .iter()
        .map(|k| ring[(a + k) % n])
        .collect()
}

/// The Loop 9 rest stop within [`REST_STOP_MEET_M`] of `xy` (row 153).
fn rest_stop_near(world: &World, xy: [f32; 2]) -> Option<&wp_world::network::Poi> {
    world
        .rest_stops()
        .filter(|p| dist(geom::xy(p.pos), xy) <= REST_STOP_MEET_M)
        .min_by(|a, b| dist(geom::xy(a.pos), xy).total_cmp(&dist(geom::xy(b.pos), xy)))
}

/// A random pose on Loop 9's road line, as [`random_lane_pose`] (same distance rules),
/// heading the way the loop is driven.
fn loop_lane_pose(
    rng: &mut Rng,
    ring: &Loop9,
    away_from: &[[f32; 2]],
    min_from: f32,
    min_apart: f32,
) -> Option<([f32; 3], f32)> {
    let line = &ring.line;
    for _ in 0..64 {
        let i = rng.below((line.len() - 1) as u32) as usize;
        let (p, b) = (line[i], line[i + 1]);
        let q = geom::xy(p);
        let ok = away_from.iter().enumerate().all(|(j, o)| {
            let need = if j == 0 { min_from } else { min_apart };
            dist(*o, q) >= need
        });
        if !ok || dist(q, geom::xy(b)) < 0.5 {
            continue;
        }
        let yaw = geom::yaw_of(geom::normalize(geom::sub(geom::xy(b), q)));
        return Some((p, yaw));
    }
    None
}

/// One lockup spot per [`ROSTER`] entry, where off-shift racers wait. On a city with terrain:
/// flat terrain inside the heightfield, at least [`LOCKUP_CLEAR_M`] from every road and
/// building, scanned from the grid's far corner inwards (deterministic; the cars rest on drawn
/// ground, they never drop onto the safety floor below the city). On a flat city (or when the
/// terrain has too few such spots) a row past the east edge on the ground slab.
fn lockup_slots(world: &World) -> Vec<([f32; 3], f32)> {
    let map = world.map();
    let net = world.network();
    let mut out: Vec<([f32; 3], f32)> = Vec::with_capacity(ROSTER.len());
    if let Some(g) = map.ground.as_ref() {
        // Buildings as circles round their footprints.
        let blds: Vec<([f32; 2], f32)> = net
            .buildings
            .iter()
            .filter(|b| !b.footprint.is_empty())
            .map(|b| {
                let n = b.footprint.len() as f32;
                let c = b
                    .footprint
                    .iter()
                    .fold([0.0f32; 2], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n]);
                let r = b
                    .footprint
                    .iter()
                    .map(|p| dist(*p, c))
                    .fold(0.0f32, f32::max);
                (c, r)
            })
            .collect();
        let hi = g.max_corner();
        let lo = g.origin;
        let step = 24.0;
        let clear = |q: [f32; 2]| {
            let k = LOCKUP_CLEAR_M;
            [[0.0, 0.0], [k, 0.0], [-k, 0.0], [0.0, k], [0.0, -k]]
                .iter()
                .all(|d| map.segs_near([q[0] + d[0], q[1] + d[1]]).is_empty())
                && blds.iter().all(|(c, r)| dist(*c, q) > r + LOCKUP_CLEAR_M)
        };
        let mut y = hi[1] - 60.0;
        'scan: while y > lo[1] + 60.0 {
            let mut x = hi[0] - 60.0;
            while x > lo[0] + 60.0 {
                let q = [x, y];
                let flat = g.normal_at(q)[2] > 0.995;
                if flat && out.iter().all(|(p, _)| dist(geom::xy(*p), q) > 15.0) && clear(q) {
                    out.push(([q[0], q[1], g.height_on_triangles(q)], 0.0));
                    if out.len() == ROSTER.len() {
                        break 'scan;
                    }
                }
                x -= step;
            }
            y -= step;
        }
    }
    if out.len() < ROSTER.len() {
        out.clear();
        let (mut max_x, mut min_y) = (0.0f32, 0.0f32);
        for l in &net.lanes {
            for p in &l.polyline {
                max_x = max_x.max(p[0]);
                min_y = min_y.min(p[1]);
            }
        }
        let x = (max_x + LOCKUP_OFFSET_M).min(4800.0);
        let z = map
            .ground
            .as_ref()
            .map_or(0.0, |g| g.height_on_triangles([x, min_y]));
        for k in 0..ROSTER.len() {
            out.push(([x, min_y + 12.0 * k as f32, z], 0.0));
        }
    }
    out
}

/// A random pose on a drivable through / highway lane, at least `min_from` m from
/// `away_from[0]` (the player) and `min_apart` m from the other points.
fn random_lane_pose(
    world: &World,
    rng: &mut Rng,
    away_from: &[[f32; 2]],
    min_from: f32,
    min_apart: f32,
) -> Option<([f32; 3], f32)> {
    use wp_world::network::LaneKind;
    let lanes = &world.network().lanes;
    if lanes.is_empty() {
        return None;
    }
    for _ in 0..256 {
        let lane = &lanes[rng.below(lanes.len() as u32) as usize];
        if !matches!(lane.kind, LaneKind::Through | LaneKind::Highway) || lane.polyline.len() < 2 {
            continue;
        }
        // Keep clear of the stop lines at either end.
        let n = lane.polyline.len();
        let i = rng.below((n - 1) as u32) as usize;
        let (a, b) = (lane.polyline[i], lane.polyline[i + 1]);
        let seg = dist(geom::xy(a), geom::xy(b));
        let total: f32 = lane
            .polyline
            .windows(2)
            .map(|w| dist(geom::xy(w[0]), geom::xy(w[1])))
            .sum();
        if total < 60.0 || seg < 1.0 {
            continue;
        }
        let t = 0.5;
        let p = [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ];
        let q = geom::xy(p);
        let ok = away_from.iter().enumerate().all(|(j, o)| {
            let need = if j == 0 { min_from } else { min_apart };
            dist(*o, q) >= need
        });
        if !ok {
            continue;
        }
        let yaw = geom::yaw_of(geom::normalize(geom::sub(geom::xy(b), geom::xy(a))));
        return Some((p, yaw));
    }
    None
}

#[cfg(test)]
mod tests {
    use wp_contracts::{DamageMode, VehicleSim};

    use super::*;
    use crate::scenario::free_drive;
    use crate::session::Session;

    #[test]
    fn entity_ids_round_trip_and_other_ids_are_not_racers() {
        for (lv, name, shift) in [
            (2, "Dex", Shift::AllDay),
            (5, "Tamsin", Shift::Night),
            (3, "Halo", Shift::Loop),
        ] {
            let id = entity_id(lv, name, shift);
            assert_eq!(parse_entity_id(&id), Some((lv, name)), "{id}");
            assert_eq!(shift_of_entity_id(&id), shift, "{id}");
        }
        // Ids from before the shifts (16-racer recordings) are all-day racers.
        assert_eq!(parse_entity_id("street_4_nyx"), Some((4, "Nyx")));
        assert_eq!(shift_of_entity_id("street_4_nyx"), Shift::AllDay);
        for id in [
            "player",
            "car2",
            "street_9_dex",
            "street_1_dex",
            "street_2_nobody",
            "street_x",
        ] {
            assert_eq!(parse_entity_id(id), None, "{id}");
        }
    }

    /// Row 154: a route with nothing to drive (one segment, destination behind the start, as
    /// `World::plan_route` returns for a destination back down the same long segment) gives no
    /// field instead of a panic.
    #[test]
    fn a_route_with_nothing_to_drive_gives_no_field() {
        let world = World::procedural(
            &wp_world::procgen::ProcGenConfig::default(),
            wp_world::WorldConfig::default(),
        );
        let map = world.map();
        let (id, seg) = map
            .segs
            .iter()
            .enumerate()
            .find(|(_, s)| s.lane().is_some() && s.length > 50.0)
            .expect("a long lane");
        let (a, _) = seg.point_at(seg.length * 0.8);
        let (b, _) = seg.point_at(seg.length * 0.2);
        let route = wp_world::Route {
            segs: vec![id as u32],
            lanes: Vec::new(),
            polyline: vec![a, b],
            checkpoints: Vec::new(),
            length_m: 200.0,
            closed_loop: false,
        };
        let car = simcli::data::load_car(mem_data(), "kestrel_ts").unwrap();
        let spec = RacerSpec {
            entity: EntityId(2),
            profile: roam_profile(),
            capability: cruise_capability(&car, 12.0),
        };
        assert!(field_on(&world, &route, &[spec], RaceConfig::default()).is_none());
    }

    /// Night Port Vesper session with the 4th loop racer parked idle at `stop`, the player 9 m
    /// behind it; returns the session, the racer's roster index and its entity.
    fn rest_stop_meet(stop: usize) -> (Session, usize, EntityId, wp_world::network::Poi) {
        let mut sc = crate::scenario::free_drive_in(
            "debug_sedan",
            DamageMode::Arcade,
            crate::scenario::city_spec("vesper_v2"),
        );
        sc.time_of_day_h = 23.0;
        let mut s = Session::new(sc, false).unwrap();
        s.run_ticks(240);
        let poi = s
            .sim()
            .world()
            .rest_stops()
            .nth(stop)
            .cloned()
            .expect("a rest stop");
        let k = s
            .street()
            .unwrap()
            .racers
            .iter()
            .position(|r| r.shift == Shift::Loop)
            .unwrap();
        let e = s.street().unwrap().racers[k].entity;
        let f = geom::dir_of_yaw(poi.heading_rad);
        let o = geom::xy(poi.pos);
        s.place_logged(e, o, poi.heading_rad, poi.pos[2]).unwrap();
        {
            let r = &mut s.street_mut_for_tests().racers[k];
            r.field = None;
            r.replan_at = u64::MAX;
        }
        s.place_logged(
            s.entities()[0],
            [o[0] - f[0] * 9.0, o[1] - f[1] * 9.0],
            poi.heading_rad,
            poi.pos[2],
        )
        .unwrap();
        s.run_ticks(240);
        (s, k, e, poi)
    }

    /// Row 153: a loop racer idling at a rest stop takes a challenge there; the card offers a
    /// Loop 9 lap back to the rest stop; accepting puts both cars side by side on its parking
    /// road, at rest, facing its way, and the racer laps at speed.
    #[test]
    fn night_loop_race_meets_at_a_rest_stop() {
        let (mut s, k, e, poi) = rest_stop_meet(0);
        let player = s.entities()[0];
        let horn = |on: bool| VehicleInput {
            handbrake: 1.0,
            horn: on,
            headlights: true,
            ..Default::default()
        };
        s.set_input(player, horn(true)).unwrap();
        s.run_ticks(2);
        s.set_input(player, horn(false)).unwrap();
        s.run_ticks(2);
        let offer = s
            .street()
            .unwrap()
            .offer(s.sim(), s.run())
            .expect("challenge at the rest stop");
        assert_eq!(offer.entity, e.0);
        assert_eq!(offer.rest_stop.as_deref(), Some(poi.name.as_str()));
        let lap = offer.loop_lap.clone().expect("a Loop 9 lap on the card");
        let pv = s.street_preview(lap.finish, &lap.via).unwrap();
        eprintln!("{}: lap {:.0} m, ok {}", poi.name, pv.length_m, pv.ok);
        assert!(
            pv.ok && (9000.0..=RACE_MAX_M).contains(&pv.length_m),
            "{pv:?}"
        );
        let stake = level(s.street().unwrap().racers[k].level).bet_min;
        s.street_accept(lap.finish, &lap.via, stake).unwrap();
        s.run_ticks(2);
        let st = |s: &Session, id| s.vehicle(id).unwrap().vehicle.state();
        let (a, b) = (st(&s, player), st(&s, e));
        let f = geom::dir_of_yaw(poi.heading_rad);
        let r = geom::right_of(f);
        let rel = |p: [f32; 3]| {
            let d = geom::sub(geom::xy(p), geom::xy(poi.pos));
            (d[0] * f[0] + d[1] * f[1], d[0] * r[0] + d[1] * r[1])
        };
        let (pa, pb) = (rel(a.pos), rel(b.pos));
        eprintln!(
            "start: player {pa:?} racer {pb:?} speeds {:.2} {:.2}",
            a.speed_mps, b.speed_mps
        );
        assert!(
            pa.0.abs() < 1.0 && pb.0.abs() < 1.0,
            "both level on the parking road"
        );
        assert!(
            pa.1 > 1.5 && pb.1 < -1.5,
            "side by side: player right, racer left"
        );
        assert!(
            a.speed_mps.abs() < 0.3 && b.speed_mps.abs() < 0.3,
            "at rest"
        );
        // The racer gets going (the player stays on the handbrake).
        let mut top = 0.0f32;
        for _ in 0..(240 * 25) {
            s.set_input(player, horn(false)).unwrap();
            s.run_ticks(1);
            top = top.max(st(&s, e).speed_mps);
        }
        eprintln!("racer top {top:.1} m/s");
        assert!(top > 20.0, "the racer laps at speed ({top} m/s)");
    }

    /// Row 153: a loop racer's lap ends in a rest stop (one to two laps from wherever it is).
    #[test]
    fn a_loop_lap_ends_at_a_rest_stop() {
        let world = crate::scenario::city_spec("vesper_v2");
        let mut sc = crate::scenario::free_drive_in("debug_sedan", DamageMode::Arcade, world);
        sc.time_of_day_h = 23.0;
        let s = Session::new(sc, false).unwrap();
        let w = s.sim().world();
        let ring = loop9(w);
        assert!(!ring.is_empty());
        let start = ring.line[ring.line.len() / 3];
        let next = ring.line[ring.line.len() / 3 + 1];
        let fwd = geom::normalize(geom::sub(geom::xy(next), geom::xy(start)));
        for p in w.rest_stops() {
            let r = loop_lap(w, start, fwd, &ring.corners, Some(p.pos))
                .expect("a lap into the rest stop");
            let end = *r.polyline.last().unwrap();
            eprintln!(
                "{}: {:.0} m, ends {:.1} m from it",
                p.name,
                r.length_m,
                dist(geom::xy(end), geom::xy(p.pos))
            );
            assert!(dist(geom::xy(end), geom::xy(p.pos)) < 5.0);
            // A full lap from the corner ahead, then on round to the rest stop: 1–2 laps.
            assert!(r.length_m > 9000.0 && r.length_m < 22000.0);
        }
    }

    #[test]
    fn every_level_car_loads() {
        for l in &LEVELS {
            for car in l.cars {
                assert!(!car.starts_with("debug_"), "{}: debug car {car}", l.name);
                simcli::data::load_car(mem_data(), car).unwrap_or_else(|e| panic!("{car}: {e}"));
            }
        }
    }

    #[test]
    fn wrecked_or_stuck_racer_is_moved_out_of_sight() {
        let mut s =
            Session::new(free_drive("debug_sedan", DamageMode::Arcade, true), false).unwrap();
        s.run_ticks(240);
        let player = s.entities()[0];
        let e = s.street().unwrap().racers[0].entity;
        let pos = |s: &Session, id| s.vehicle(id).unwrap().vehicle.state().pos;
        let before = pos(&s, e);
        let p = pos(&s, player);
        assert!(dist(geom::xy(before), geom::xy(p)) >= RESPAWN_OUT_OF_SIGHT_M);
        {
            let street = s.street_mut_for_tests();
            let r = &mut street.racers[0];
            r.field = None;
            r.respawn = true;
            r.replan_at = 0;
        }
        s.run_ticks(3);
        assert!(!s.street().unwrap().racers[0].respawn, "respawned");
        let after = pos(&s, e);
        assert!(dist(geom::xy(before), geom::xy(after)) > 1.0, "moved");
        assert!(dist(geom::xy(after), geom::xy(p)) >= RESPAWN_OUT_OF_SIGHT_M);
        assert!(
            s.sim().ghost_ticks(e).unwrap_or(0) > 0,
            "ghosted after the move"
        );
        // Then on a new route: one plan per tick, a failed plan retried 60 ticks later.
        for _ in 0..2 * 240 {
            if s.street().unwrap().racers[0].field.is_some() {
                break;
            }
            s.run_ticks(1);
        }
        assert!(
            s.street().unwrap().racers[0].field.is_some(),
            "and on a new route"
        );
    }
}
