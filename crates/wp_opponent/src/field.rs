//! The race field: every AI racer in one event, their perception of each other, civilians and
//! other vehicles, the tactical layer, lap timing and race control (DQ, retirement, flight).
//!
//! ## Per tick (call [`Field::tick`] once per physics tick)
//! Decisions run every [`RaceConfig::decision_ticks`] ticks (default 4 = 60 Hz, like traffic);
//! in between the last inputs are returned unchanged.
//! 1. **Observe**: project every racer onto the corridor (tracking hint), update progress,
//!    laps, stability, damage-derived capability, police heat; consume this tick's contacts.
//! 2. **Perceive**: every other vehicle and every civilian near a racer becomes an
//!    [`Obstacle`] in corridor coordinates (`ds` ahead/behind, lateral `n`, speeds, size).
//! 3. **Decide** (per racer, independent of race position — no rubber-banding):
//!    tactic state machine (follow / overtake / defend / rejoin / reverse / park / flee),
//!    lateral target with hard side constraints (never steer into an overlapping car),
//!    speed target = min(speed profile with this zone's braking-point error, obstacle caps).
//! 4. **Track**: `wp_world::drive::DriveController` (pure pursuit + PID, shared with traffic).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::{
    EntityId, HandlingEffects, SignalState, SimEvent, VehicleInput, VehicleState, PHYSICS_DT,
};
use wp_math::{Rng, StateHasher};
use wp_world::drive::{forward_of, DriveController, DriveGains};
use wp_world::geom::{self, Obb, V2, V3};
use wp_world::traffic::{CAR_LENGTH_M, CAR_WIDTH_M};
use wp_world::World;

use crate::capability::CarCapability;
use crate::corridor::{Corridor, CorridorOptions, OpposingLanes, CORRIDOR_STEP_M};
use crate::line::{LineParams, RacingLine, SpeedProfile};
use crate::profile::{DriverParams, DriverProfile};
use crate::pursuit::PoliceQuery;
use wp_police::{PoliceEvent, PursuitOutcome};

/// Impulse above which a racer–racer contact counts as a collision (not a rub), N·s.
/// 400 N·s ≈ a 1.4 t car changing speed by 0.3 m/s.
pub const AI_CONTACT_IMPULSE_NS: f32 = 400.0;

/// Race rules and AI update settings. (Race rule objects proper come from prompt 15; this is
/// the subset the AI needs.)
#[derive(Debug, Clone, PartialEq)]
pub struct RaceConfig {
    /// Laps (circuits). Ignored on point-to-point routes.
    pub laps: u32,
    /// Physics ticks between AI decisions (4 = 60 Hz).
    pub decision_ticks: u32,
    /// Racers may abandon the race and run from the police.
    pub allow_abandon: bool,
    /// Touching a civilian disqualifies the racer (always true in the game; tests may relax it).
    pub civilian_contact_dq: bool,
    /// Speed held after finishing, m/s.
    pub cooldown_speed_mps: f32,
    /// Seed for every driver's noise stream.
    pub seed: u64,
    /// Line optimiser settings.
    pub line: LineParams,
    /// Corridor derivation.
    pub corridor: CorridorOptions,
    /// Read traffic signals at junctions (open roads): a lasting green means cross traffic
    /// is held; careful drivers stop at reds. Off for closed courses.
    pub signals: bool,
    /// The sim applies slipstream ([`crate::draft`]): held-up racers close right up into the
    /// tow on straights and pull out with the run it gives. Leave off unless the sim loop
    /// really applies the drag multipliers (otherwise sitting that close only adds risk).
    pub draft: bool,
    /// Where the judged finish line lies along a point-to-point corridor, m, when the
    /// corridor runs on past it (run-off): racers beyond it are done racing (they cool down in
    /// their lane and pass nobody). `None`: the corridor's end is the finish.
    pub finish_m: Option<f32>,
}

impl Default for RaceConfig {
    fn default() -> Self {
        Self {
            laps: 3,
            decision_ticks: 4,
            allow_abandon: true,
            civilian_contact_dq: true,
            cooldown_speed_mps: 12.0,
            seed: 1,
            line: LineParams::default(),
            corridor: CorridorOptions::default(),
            signals: true,
            draft: false,
            finish_m: None,
        }
    }
}

/// One AI racer to put in the field.
#[derive(Debug, Clone, PartialEq)]
pub struct RacerSpec {
    /// Sim entity of the racer's vehicle.
    pub entity: EntityId,
    /// Personality.
    pub profile: DriverProfile,
    /// Measured car limits ([`CarCapability::measure`]).
    pub capability: CarCapability,
}

/// What the field sees of one vehicle each tick (racers and any other sim vehicle: players,
/// police). Civilians come from `World::agents()`.
#[derive(Debug, Clone, PartialEq)]
pub struct VehicleObs {
    /// Entity.
    pub entity: EntityId,
    /// Current state.
    pub state: VehicleState,
    /// Current damage effects (`DamageSim::handling_effects`).
    pub effects: HandlingEffects,
    /// Body `[width, length]`, m.
    pub size_m: [f32; 2],
}

/// Starting grid shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GridLayout {
    /// Two staggered columns (left column on pole).
    TwoWide,
    /// One car behind the other.
    SingleFile,
}

/// Race status of a racer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RacerStatus {
    /// Racing.
    Racing,
    /// Took the flag at this tick.
    Finished {
        /// Finish tick.
        tick: u64,
    },
    /// Disqualified (hit a civilian).
    Disqualified,
    /// Car too damaged to continue.
    Retired,
    /// Abandoned the race to run from the police.
    Fled,
    /// Caught by the police (stopped next to a unit while wanted).
    Busted,
}

/// What a racer is doing right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Tactic {
    /// Follow the racing line (and whoever is ahead).
    Follow,
    /// Passing someone on a committed side.
    Overtake,
    /// The single defensive move of this straight.
    Defend,
    /// Back onto the corridor after an off or a spin.
    Rejoin,
    /// Backing out of a wall / stuck spot.
    Reverse,
    /// Pulled over and stopped (DQ, retired, escaped).
    Park,
    /// Pulled over for the police (wanted below the flee level with a unit on its tail);
    /// resumes if the police lose interest.
    Yield,
}

/// Race events reported by the field (race/economy/UI consumers; not `SimEvent`s until the
/// race event contract from prompt 15 exists).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OpponentEvent {
    /// A lap was completed.
    LapCompleted {
        /// Racer.
        entity: EntityId,
        /// Lap number (1-based).
        lap: u32,
        /// Lap time, s.
        time_s: f32,
    },
    /// Took the flag.
    Finished {
        /// Racer.
        entity: EntityId,
        /// Finishing position (1-based, among finishers).
        position: u32,
        /// Race time since the field's first tick, s.
        time_s: f32,
    },
    /// Passed another car (racer or not) for position on the road.
    Overtook {
        /// Passer.
        entity: EntityId,
        /// Car passed.
        other: EntityId,
    },
    /// Hit a civilian: out of the race.
    Disqualified {
        /// Racer.
        entity: EntityId,
        /// Civilian agent id.
        civilian: u32,
        /// Race time, s.
        time_s: f32,
    },
    /// Car too damaged to continue.
    Retired {
        /// Racer.
        entity: EntityId,
    },
    /// Abandoned the race to run from the police.
    Abandoned {
        /// Racer.
        entity: EntityId,
        /// Wanted level that triggered it.
        wanted_level: u8,
    },
    /// Pulled over for a chasing unit instead of running.
    Yielded {
        /// Racer.
        entity: EntityId,
        /// Wanted level at the time.
        wanted_level: u8,
    },
    /// Caught by the police (`PursuitEnded` with a capture outcome).
    Busted {
        /// Racer.
        entity: EntityId,
        /// Race time, s.
        time_s: f32,
    },
    /// A fleeing racer lost the police (no longer wanted or chased).
    Escaped {
        /// Racer.
        entity: EntityId,
        /// Race time, s.
        time_s: f32,
    },
    /// Two AI racers touched above [`AI_CONTACT_IMPULSE_NS`].
    AiContact {
        /// First racer.
        a: EntityId,
        /// Second racer.
        b: EntityId,
        /// Normal impulse, N·s.
        impulse_ns: f32,
    },
}

/// The signal governing the next junction entry on a racer's track.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SignalAhead {
    /// Distance to the junction entry, m.
    d_entry: f32,
    /// Current state for my approach.
    state: SignalState,
    /// Seconds until that state ends.
    remaining_s: f32,
}

/// Who an obstacle is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Who {
    Racer(usize),
    Vehicle(EntityId),
    Civilian(u32),
}

/// Another car in a racer's corridor coordinates.
#[derive(Debug, Clone, Copy)]
struct Obstacle {
    who: Who,
    /// Arc-length position.
    s: f32,
    n: f32,
    v_s: f32,
    v_n: f32,
    half_len: f32,
    half_w: f32,
    unstable: bool,
    /// A racer that is itself overtaking (never pass it: no three-wide).
    passing: bool,
    /// A car pulled over for good (a retired, busted or yielding racer; a civilian stopped
    /// for a siren or with its hazards on): a fixed obstacle anyone may pass, using the
    /// opposing lanes where the corridor allows overtaking there.
    parked: bool,
    /// Its heading along the corridor (cosine to the corridor direction; −1 = facing me
    /// head on).
    facing: f32,
}

/// An obstacle relative to one racer: arc distance ahead (`ds` < 0 = behind) and the
/// bumper-to-bumper gap.
#[derive(Debug, Clone, Copy)]
struct Rel {
    o: Obstacle,
    ds: f32,
    gap: f32,
}

/// A track a racer drives: the shared race corridor or a private flight route.
#[derive(Debug, Clone)]
struct Flight {
    corridor: Corridor,
    line: RacingLine,
    profile: SpeedProfile,
}

/// One AI racer.
#[derive(Debug, Clone)]
struct Racer {
    entity: EntityId,
    profile: DriverProfile,
    params: DriverParams,
    cap: CarCapability,
    cap_now: CarCapability,
    line_idx: usize,
    speed: SpeedProfile,
    ctrl: DriveController,
    rng: Rng,
    status: RacerStatus,
    tactic: Tactic,
    // Kinematics (decision rate).
    pos: V2,
    fwd: V2,
    s: f32,
    n: f32,
    v: f32,
    v_s: f32,
    v_n: f32,
    /// World velocity (horizontal), m/s.
    vel: V2,
    /// Yaw rate, rad/s (+ = turning left).
    yaw_rate: f32,
    /// Duration of the current reverse manoeuvre, s.
    reverse_s: f32,
    hint: usize,
    progress: f32,
    started: bool,
    lap_start_tick: u64,
    laps_done: u32,
    lap_times_ticks: Vec<u64>,
    unstable: bool,
    pressure: f32,
    // Driving state.
    n_cmd: f32,
    n_goal: f32,
    pass_target: Option<Who>,
    tactic_ticks: u64,
    blocked_s: f32,
    stuck_s: f32,
    stable_s: f32,
    defended: bool,
    in_brake_zone: bool,
    brake_err_s: f32,
    line_noise: f32,
    effects: HandlingEffects,
    v_cap: f32,
    throttle: f32,
    input: VehicleInput,
    flight: Option<Box<Flight>>,
    passes: u32,
    race_speed: Option<SpeedProfile>,
    debug: Debug,
    /// Half extents along / across the corridor (yaw-aware), m.
    ext: (f32, f32),
    /// Time spent side by side in braking zones during the current pass, s.
    alongside_s: f32,
    /// Drifting outward of the commanded line in a corner (set by the last decision).
    running_wide: bool,
    /// Lateral position on the grid (held during the start).
    start_n: f32,
    /// Car passed by the last completed overtake (reported once by the field).
    last_pass: Option<Who>,
    /// Pass evaluations: [accepted, no run, too late, no room, slow line, lane ahead, lane
    /// behind, target busy passing].
    pass_checks: [u32; 8],
    /// Held up by this car and no pass possible yet: drop back through the next corner to
    /// get a run at it on the following straight.
    get_run: Option<Who>,
    /// Seconds a chasing unit has been within [`YIELD_RADIUS_M`] (wanted below flee level).
    yield_s: f32,
    /// Seconds since the flight route was last planned.
    replan_s: f32,
    /// Fleeing racer that lost the police.
    escaped: bool,
    /// Seconds held at a junction by civilians that are not moving (deadlock breaker).
    junction_wait_s: f32,
}

/// Why a racer is doing what it does (last decision), for overlays and tuning.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Debug {
    /// Speed target, m/s.
    pub v_target: f32,
    /// What set the speed target.
    pub limit: SpeedLimit,
    /// Commanded lateral offset, m.
    pub n_cmd: f32,
    /// Corridor position, m.
    pub s: f32,
    /// Lateral position, m.
    pub n: f32,
    /// Distance to the next braking point, m.
    pub brake_dist: f32,
    /// Pass evaluations so far: [accepted, no run, too late, no room, slower line, lane
    /// taken ahead, lane taken behind, target busy passing someone].
    pub pass_checks: [u32; 8],
}

/// What limits a racer's speed target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SpeedLimit {
    /// The speed profile (corner / braking zone / top speed).
    #[default]
    Profile,
    /// A car or civilian ahead in my path (entity id or civilian agent id + 1_000_000).
    Obstacle(u32),
    /// Backing out of a side-by-side.
    BackOut(u32),
    /// Waiting to rejoin.
    RejoinWait,
    /// Lost the rear / sliding.
    Unstable,
    /// Rejoin / park / cool-down caps.
    Mode,
    /// Stopping for a red light (careful drivers on open roads).
    Signal,
}

/// Standing of one racer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Standing {
    /// Racer.
    pub entity: EntityId,
    /// Status.
    pub status: RacerStatus,
    /// Completed laps.
    pub laps: u32,
    /// Distance covered along the corridor since the start line, m.
    pub progress_m: f32,
    /// Completed lap times, s.
    pub lap_times_s: Vec<f32>,
}

/// All AI racers in one event.
#[derive(Debug, Clone)]
pub struct Field {
    corridor: Corridor,
    lines: Vec<(u32, RacingLine)>,
    racers: Vec<Racer>,
    config: RaceConfig,
    tick: u64,
    hints: BTreeMap<EntityId, usize>,
    events: Vec<OpponentEvent>,
    outputs: Vec<(EntityId, VehicleInput)>,
    finishers: u32,
    contacts: BTreeMap<(EntityId, EntityId), u32>,
    /// Where each non-racer came to a stand and for how long it has stayed within
    /// [`STILL_DRIFT_M`] of that spot, s (civilian agents by agent id, other sim vehicles by
    /// entity id + [`STILL_VEHICLE_KEY`]). Kept only near racers.
    still: BTreeMap<u64, (V2, f32)>,
}

impl Racer {
    /// Still racing on the race corridor: not fleeing (own route), busted or retired.
    fn in_race(&self) -> bool {
        self.flight.is_none() && !matches!(self.status, RacerStatus::Busted | RacerStatus::Retired)
    }

    /// How far ahead this driver notices cars and civilians, m: the awareness range, but
    /// never less than 1.3× the braking distance from the present speed plus 30 m (at
    /// 60 m/s on a highway a stalled car must be seen ~250 m out).
    fn ahead_m(&self) -> f32 {
        let v = self.v.max(0.0);
        let a = self.speed.a_brake.max(3.0);
        self.params
            .perception_ahead_m
            .max(1.3 * v * v / (2.0 * a) + 30.0)
    }
}

/// Key offset of sim vehicles in [`Field`]'s standing-still bookkeeping.
const STILL_VEHICLE_KEY: u64 = 1 << 40;

/// A non-racer that has not moved for this long counts as parked: anyone may go round it,
/// on the opposing lanes where the corridor has them, s. (A red light is shorter; a car
/// stuck in a traffic deadlock stays put for minutes.)
const STILL_PARKED_S: f32 = 8.0;

/// A sim vehicle (police, another player) still counts as standing while it stays this
/// close to where it stopped, m, and moves slower than [`STILL_MAX_MPS`]: a stopped police
/// unit nudges back and forth on the spot.
const STILL_DRIFT_M: f32 = 2.5;
/// Fastest a standing sim vehicle may move while it shuffles on the spot, m/s.
const STILL_MAX_MPS: f32 = 1.0;
/// Civilians stand only below this speed (a queue at a light creeps forward and must not
/// count as parked), m/s.
const CIV_STILL_MPS: f32 = 0.3;

/// Side clearance kept to a parked sim vehicle while creeping past it, m. It grows to the
/// normal clearance by 15 m/s.
const PARKED_GAP_M: f32 = 0.2;
/// How much nearer the road edge a racer may squeeze round a parked car than it drives
/// otherwise, m (0.25 m edge reserve → 0.10 m).
const PARKED_EDGE_SLACK_M: f32 = 0.15;

/// Side clearance to a parked sim vehicle at speed `v`: `PARKED_GAP_M` at a crawl, `full`
/// from 15 m/s.
fn parked_gap(v: f32, full: f32) -> f32 {
    PARKED_GAP_M + (full - PARKED_GAP_M).max(0.0) * (v / 15.0).clamp(0.0, 1.0)
}

/// Whether `o` is a parked sim vehicle that is not a racer (a police unit standing in the
/// road): squeezed past slowly with little clearance. Racers pulled over and civilians keep
/// their normal clearance.
fn parked_non_racer(o: &Obstacle) -> bool {
    o.parked && matches!(o.who, Who::Vehicle(_))
}

/// Approximate standard normal from four uniforms (Irwin–Hall; no transcendental functions).
fn gauss(rng: &mut Rng) -> f32 {
    let s = rng.next_f32() + rng.next_f32() + rng.next_f32() + rng.next_f32();
    (s - 2.0) * 1.732_050_8
}

fn approach(x: f32, target: f32, max_step: f32) -> f32 {
    x + (target - x).clamp(-max_step, max_step)
}

const SIDE_GAP_M: f32 = 0.6;
const MIN_GAP_M: f32 = 2.5;
const LAT_MARGIN_M: f32 = 0.35;
/// Distance from the start line over which racers hold their grid lane, m.
const START_HOLD_M: f32 = 150.0;
/// Seconds of lateral closing speed added to the side clearance.
const LATERAL_LEAD_S: f32 = 0.6;
/// A racer wanted below its flee level pulls over when a unit chasing it stays this close, m.
pub const YIELD_RADIUS_M: f32 = 30.0;
/// ... for this long, s.
pub const YIELD_AFTER_S: f32 = 2.0;
/// A fleeing racer re-plans its route when a police position is this close to the next
/// stretch of it, m.
const FLIGHT_THREAT_M: f32 = 35.0;
/// Route stretch checked for police ahead, m.
const FLIGHT_LOOKAHEAD_M: f32 = 160.0;
/// Minimum time between flight re-plans, s.
const FLIGHT_REPLAN_S: f32 = 6.0;
/// Drivers below this risk stop for red lights on open roads; the others go through a red
/// only when the junction is clear.
pub const RED_LIGHT_RISK: f32 = 0.35;
/// Signals are looked up this far ahead, m.
const SIGNAL_LOOKAHEAD_M: f32 = 150.0;
/// After waiting this long at a junction for civilians that are standing still (each waiting
/// for the other), a racer stops counting them and creeps through, s.
const JUNCTION_DEADLOCK_S: f32 = 3.0;
/// A civilian agent near the racers (junction awareness).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Civ {
    id: u32,
    pos: V2,
    vel: V2,
    /// -1 left, 0 off, 1 right.
    turn_signal: i8,
}

/// Speed allowed while steering out round a parked car the racer is already stopped behind
/// (a car must roll to move sideways), m/s.
const CREEP_MPS: f32 = 2.5;
/// Gap a creeping racer always keeps to the parked car it is steering round, m.
const CREEP_STOP_GAP_M: f32 = 1.0;

impl Field {
    /// Build a field on `corridor`. Lines are optimised once per distinct car width.
    pub fn new(corridor: Corridor, specs: &[RacerSpec], config: RaceConfig) -> Self {
        let mut master = Rng::new(config.seed ^ 0x0BB0_57E5_u64);
        let mut lines: Vec<(u32, RacingLine)> = Vec::new();
        let mut racers = Vec::with_capacity(specs.len());
        for spec in specs {
            let key = (spec.capability.width_m * 100.0).round() as u32;
            let line_idx = match lines.iter().position(|(k, _)| *k == key) {
                Some(i) => i,
                None => {
                    let half = spec.capability.width_m * 0.5;
                    lines.push((key, RacingLine::optimise(&corridor, half, &config.line)));
                    lines.len() - 1
                }
            };
            let params = spec.profile.params();
            let speed = SpeedProfile::build(
                &corridor,
                &lines[line_idx].1,
                &spec.capability,
                &params.usage,
                f32::INFINITY,
            );
            let gains = DriveGains {
                lookahead_per_mps: params.lookahead_per_mps,
                lookahead_max_m: 30.0,
                ..DriveGains::TRAFFIC
            };
            racers.push(Racer {
                entity: spec.entity,
                profile: spec.profile.clamped(),
                params,
                cap: spec.capability,
                cap_now: spec.capability,
                line_idx,
                speed,
                ctrl: DriveController::new(
                    gains,
                    spec.capability.wheelbase_m,
                    spec.capability.max_steer_rad,
                    0.0,
                ),
                rng: master.fork(),
                status: RacerStatus::Racing,
                tactic: Tactic::Follow,
                pos: [0.0; 2],
                fwd: [0.0, 1.0],
                s: 0.0,
                n: 0.0,
                v: 0.0,
                v_s: 0.0,
                v_n: 0.0,
                vel: [0.0; 2],
                yaw_rate: 0.0,
                reverse_s: 1.6,
                hint: 0,
                progress: f32::NAN,
                started: false,
                lap_start_tick: 0,
                laps_done: 0,
                lap_times_ticks: Vec::new(),
                unstable: false,
                pressure: 0.0,
                n_cmd: f32::NAN,
                n_goal: 0.0,
                pass_target: None,
                tactic_ticks: 0,
                blocked_s: 0.0,
                stuck_s: 0.0,
                stable_s: 0.0,
                defended: false,
                in_brake_zone: false,
                brake_err_s: 0.0,
                line_noise: 0.0,
                effects: HandlingEffects::NEUTRAL,
                v_cap: f32::INFINITY,
                throttle: 0.0,
                input: VehicleInput::default(),
                flight: None,
                passes: 0,
                race_speed: None,
                debug: Debug::default(),
                ext: (
                    0.5 * spec.capability.length_m,
                    0.5 * spec.capability.width_m,
                ),
                alongside_s: 0.0,
                get_run: None,
                yield_s: 0.0,
                replan_s: 0.0,
                escaped: false,
                junction_wait_s: 0.0,
                running_wide: false,
                start_n: 0.0,
                last_pass: None,
                pass_checks: [0; 8],
            });
        }
        Self {
            corridor,
            lines,
            racers,
            config,
            tick: 0,
            hints: BTreeMap::new(),
            events: Vec::new(),
            outputs: Vec::new(),
            finishers: 0,
            contacts: BTreeMap::new(),
            still: BTreeMap::new(),
        }
    }

    /// Build a field on a `wp_world` route.
    pub fn from_route(
        world: &World,
        route: &wp_world::Route,
        specs: &[RacerSpec],
        config: RaceConfig,
    ) -> Self {
        let corridor = Corridor::from_route(world.map(), route, &config.corridor);
        Self::new(corridor, specs, config)
    }

    /// The race corridor.
    pub fn corridor(&self) -> &Corridor {
        &self.corridor
    }

    /// Racing line for racer `k` (spec order).
    pub fn line_of(&self, k: usize) -> &RacingLine {
        &self.lines[self.racers[k].line_idx].1
    }

    /// Speed profile racer `k` (spec order) currently drives to.
    pub fn speed_profile_of(&self, k: usize) -> &SpeedProfile {
        &self.racers[k].speed
    }

    /// Grid slots behind the start line (`s = 0`; on a point-to-point route the grid fills
    /// the first metres instead), front first: `(pos, yaw)`. [`GridLayout::TwoWide`] falls
    /// back to single file where the corridor is narrower than two cars plus a gap.
    pub fn grid_slots(
        &self,
        count: usize,
        car_len_m: f32,
        car_width_m: f32,
        layout: GridLayout,
    ) -> Vec<(V3, f32)> {
        let c = &self.corridor;
        let probe = if c.closed { c.wrap_s(-60.0) } else { 0.0 };
        let (l, r) = c.min_bounds(probe, 70.0);
        let two = layout == GridLayout::TwoWide && l + r >= 2.0 * car_width_m + 2.5;
        let off = 0.5 * car_width_m + 0.6;
        let centre = 0.5 * (r - l);
        let row_gap = |k: usize| {
            if two {
                k as f32 * (car_len_m + 3.0) * 0.5
            } else {
                k as f32 * (car_len_m + 3.5)
            }
        };
        let s_front = if c.closed {
            c.length_m - car_len_m
        } else {
            row_gap(count.saturating_sub(1)) + car_len_m + 2.0
        };
        (0..count)
            .map(|k| {
                let n = match (two, k % 2) {
                    (true, 0) => centre - off,
                    (true, _) => centre + off,
                    (false, _) => centre,
                };
                let s = c.wrap_s(s_front - row_gap(k));
                let p = c.point(s, n);
                let t = c.samples[(s / CORRIDOR_STEP_M) as usize % c.len()].tangent;
                (p, geom::yaw_of(t))
            })
            .collect()
    }

    /// Ticks seen.
    pub fn tick_count(&self) -> u64 {
        self.tick
    }

    /// Inputs from the last tick, in spec order.
    pub fn inputs(&self) -> &[(EntityId, VehicleInput)] {
        &self.outputs
    }

    /// Race events since the last call.
    pub fn drain_events(&mut self) -> Vec<OpponentEvent> {
        std::mem::take(&mut self.events)
    }

    /// Racer–racer contacts above [`AI_CONTACT_IMPULSE_NS`] (one per contact onset tick,
    /// counted per pair).
    pub fn ai_contacts(&self) -> &BTreeMap<(EntityId, EntityId), u32> {
        &self.contacts
    }

    /// Total racer–racer contacts above the threshold.
    pub fn ai_contact_count(&self) -> u32 {
        self.contacts.values().sum()
    }

    /// Current tactic of racer `k` (spec order).
    pub fn tactic_of(&self, k: usize) -> Tactic {
        self.racers[k].tactic
    }

    /// Race standings, leader first (finishers by finish tick, then by progress).
    pub fn standings(&self) -> Vec<Standing> {
        let mut v: Vec<(u8, u64, f32, Standing)> = self
            .racers
            .iter()
            .map(|r| {
                let st = Standing {
                    entity: r.entity,
                    status: r.status,
                    laps: r.laps_done,
                    progress_m: if r.progress.is_nan() { 0.0 } else { r.progress },
                    lap_times_s: r
                        .lap_times_ticks
                        .iter()
                        .map(|t| *t as f32 * PHYSICS_DT)
                        .collect(),
                };
                let (class, ft) = match r.status {
                    RacerStatus::Finished { tick } => (0, tick),
                    RacerStatus::Racing => (1, 0),
                    _ => (2, 0),
                };
                (class, ft, st.progress_m, st)
            })
            .collect();
        v.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(a.1.cmp(&b.1))
                .then(b.2.total_cmp(&a.2))
                .then(a.3.entity.cmp(&b.3.entity))
        });
        v.into_iter().map(|x| x.3).collect()
    }

    /// True when no racer is still racing.
    pub fn race_over(&self) -> bool {
        self.racers.iter().all(|r| r.status != RacerStatus::Racing)
    }

    /// Deterministic hash of the field state.
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        h.write_u64(self.tick);
        for r in &self.racers {
            h.write_u32(r.entity.0);
            h.write_u32(match r.status {
                RacerStatus::Racing => 0,
                RacerStatus::Finished { .. } => 1,
                RacerStatus::Disqualified => 2,
                RacerStatus::Retired => 3,
                RacerStatus::Fled => 4,
                RacerStatus::Busted => 5,
            });
            h.write_u32(r.tactic as u32);
            h.write_u32(r.laps_done);
            h.write_f32_q(
                if r.progress.is_nan() {
                    -1.0
                } else {
                    r.progress
                },
                1e3,
            );
            h.write_f32_q(if r.n_cmd.is_nan() { 0.0 } else { r.n_cmd }, 1e3);
            h.write_f32_q(r.brake_err_s, 1e4);
            for t in &r.lap_times_ticks {
                h.write_u64(*t);
            }
            h.write_u64(r.rng.clone().next_u64());
        }
        h.finish()
    }

    /// Advance one physics tick. `vehicles` holds every sim vehicle (racers included, any
    /// order); `events` are this tick's sim events (contacts). Returns the inputs to apply to
    /// each racer this tick, in spec order.
    pub fn tick(
        &mut self,
        world: &World,
        vehicles: &[VehicleObs],
        events: &[SimEvent],
        police: &dyn PoliceQuery,
    ) -> &[(EntityId, VehicleInput)] {
        self.record_contacts(events);
        self.record_police(events);
        if self
            .tick
            .is_multiple_of(self.config.decision_ticks.max(1) as u64)
        {
            let dt = self.config.decision_ticks.max(1) as f32 * PHYSICS_DT;
            self.observe(world, vehicles, police, dt);
            self.update_still(world, vehicles, dt);
            let snapshot = self.perceive_all(world, vehicles);
            let civs = self.civilians_near(world);
            for k in 0..self.racers.len() {
                let obstacles = match &self.racers[k].flight {
                    None => snapshot.clone(),
                    Some(f) => self.perceive_on(&f.corridor, k, world, vehicles),
                };
                let signal = self.signal_ahead(k, world);
                self.decide(k, &obstacles, &civs, signal, dt);
            }
            self.outputs = self.racers.iter().map(|r| (r.entity, r.input)).collect();
        } else {
            for (o, r) in self.outputs.iter_mut().zip(&self.racers) {
                o.1 = r.input;
                o.1.gear_up = false;
                o.1.gear_down = false;
            }
        }
        self.tick += 1;
        &self.outputs
    }

    fn is_still(&self, key: u64) -> bool {
        self.still.get(&key).is_some_and(|e| e.1 >= STILL_PARKED_S)
    }

    /// Standing-still timers of the civilians and other vehicles near any racer.
    fn update_still(&mut self, world: &World, vehicles: &[VehicleObs], dt: f32) {
        let near = |p: V2| self.racers.iter().any(|r| geom::dist(r.pos, p) < 200.0);
        let mut next = BTreeMap::new();
        let still = &self.still;
        let mut stand = |key: u64, p: V2, drift: f32| {
            let e = match still.get(&key) {
                Some(&(at, t)) if geom::dist(at, p) < drift => (at, t + dt),
                _ => (p, dt),
            };
            next.insert(key, e);
        };
        for a in world.agents() {
            let p = geom::xy(a.pos);
            if a.speed_mps.abs() < CIV_STILL_MPS && near(p) {
                stand(a.id as u64, p, f32::INFINITY);
            }
        }
        for v in vehicles {
            // Racers still in the race are tracked by the field itself; one out of it
            // (fleeing, busted, retired) is just another car that may stand in the road.
            if self
                .racer_index(v.entity)
                .is_some_and(|k| self.racers[k].in_race())
            {
                continue;
            }
            let p = geom::xy(v.state.pos);
            if v.state.speed_mps.abs() < STILL_MAX_MPS && near(p) {
                stand(STILL_VEHICLE_KEY + v.entity.0 as u64, p, STILL_DRIFT_M);
            }
        }
        self.still = next;
    }

    fn racer_index(&self, e: EntityId) -> Option<usize> {
        self.racers.iter().position(|r| r.entity == e)
    }

    fn record_contacts(&mut self, events: &[SimEvent]) {
        for ev in events {
            if let SimEvent::Contact {
                a, b, impulse_ns, ..
            } = ev
            {
                if *impulse_ns < AI_CONTACT_IMPULSE_NS {
                    continue;
                }
                if self.racer_index(*a).is_some() && self.racer_index(*b).is_some() {
                    let key = if a < b { (*a, *b) } else { (*b, *a) };
                    *self.contacts.entry(key).or_insert(0) += 1;
                    self.events.push(OpponentEvent::AiContact {
                        a: key.0,
                        b: key.1,
                        impulse_ns: *impulse_ns,
                    });
                }
            }
        }
    }

    /// Pursuit outcomes from the sim's police events: captures end a racer's event; a
    /// pursuit that ends otherwise releases a yielding racer.
    fn record_police(&mut self, events: &[SimEvent]) {
        for ev in events {
            let SimEvent::Police(PoliceEvent::PursuitEnded { racer, outcome, .. }) = ev else {
                continue;
            };
            let Some(k) = self.racer_index(*racer) else {
                continue;
            };
            let time_s = self.tick as f32 * PHYSICS_DT;
            let r = &mut self.racers[k];
            match outcome {
                PursuitOutcome::Captured | PursuitOutcome::CapturedTotalLoss => {
                    if r.status != RacerStatus::Busted {
                        r.status = RacerStatus::Busted;
                        r.tactic = Tactic::Park;
                        self.events.push(OpponentEvent::Busted {
                            entity: r.entity,
                            time_s,
                        });
                    }
                }
                _ => {
                    if r.status == RacerStatus::Fled && !r.escaped {
                        r.escaped = true;
                        r.v_cap = self.config.cooldown_speed_mps;
                        self.events.push(OpponentEvent::Escaped {
                            entity: r.entity,
                            time_s,
                        });
                    }
                    if r.tactic == Tactic::Yield {
                        r.tactic = Tactic::Rejoin;
                        r.tactic_ticks = 0;
                        r.yield_s = 0.0;
                    }
                }
            }
        }
    }

    /// Step 1: kinematics, progress, laps, damage, heat, civilian DQ.
    fn observe(
        &mut self,
        world: &World,
        vehicles: &[VehicleObs],
        police: &dyn PoliceQuery,
        dt: f32,
    ) {
        let laps = self.config.laps.max(1);
        let tick = self.tick;
        // Police positions, fetched once per decision and only if someone is wanted.
        let mut police_at: Option<Vec<V2>> = None;
        for k in 0..self.racers.len() {
            let Some(obs) = vehicles.iter().find(|v| v.entity == self.racers[k].entity) else {
                continue;
            };
            let st = &obs.state;
            let fwd = forward_of(st.rot);
            let pos = geom::xy(st.pos);
            let vel = [st.lin_vel[0], st.lin_vel[1]];
            let line_len;
            {
                let r = &mut self.racers[k];
                let corridor = match &r.flight {
                    Some(f) => &f.corridor,
                    None => &self.corridor,
                };
                let first = r.progress.is_nan();
                let hint = if first { None } else { Some(r.hint) };
                let cp = corridor.project(pos, hint, 40);
                let t = corridor.samples[cp.index].tangent;
                let prev_s = r.s;
                r.pos = pos;
                r.fwd = fwd;
                r.s = cp.s;
                r.n = cp.n;
                r.hint = cp.index;
                r.v = st.speed_mps;
                r.v_s = geom::dot(vel, t);
                r.v_n = geom::dot(vel, geom::right_of(t));
                r.vel = vel;
                r.yaw_rate = st.ang_vel[2];
                let v_long = geom::dot(vel, fwd);
                let v_lat = geom::dot(vel, geom::right_of(fwd));
                r.unstable = v_long > 8.0 && v_lat.abs() > 0.2 * v_long;
                let ca = geom::dot(fwd, t).abs();
                let sa = (1.0 - ca * ca).max(0.0).sqrt();
                let (w, l) = (r.cap.width_m, r.cap.length_m);
                r.ext = (0.5 * (l * ca + w * sa), 0.5 * (w * ca + l * sa));
                line_len = corridor.length_m;
                if r.flight.is_none() {
                    if first {
                        r.progress = if corridor.closed && cp.s > 0.5 * line_len {
                            cp.s - line_len
                        } else {
                            cp.s
                        };
                        r.started = r.progress >= 0.0;
                        r.lap_start_tick = tick;
                        r.n_cmd = cp.n;
                        r.start_n = cp.n;
                    } else {
                        r.progress += corridor.delta_s(prev_s, cp.s);
                    }
                } else if first {
                    r.n_cmd = cp.n;
                }
            }
            // Laps and finish.
            let r = &mut self.racers[k];
            if r.status == RacerStatus::Racing && r.flight.is_none() {
                if !r.started && r.progress >= 0.0 {
                    r.started = true;
                    r.lap_start_tick = tick;
                }
                if self.corridor.closed {
                    while r.started && r.progress >= (r.laps_done + 1) as f32 * line_len {
                        let t = tick - r.lap_start_tick;
                        r.lap_times_ticks.push(t);
                        r.lap_start_tick = tick;
                        r.laps_done += 1;
                        self.events.push(OpponentEvent::LapCompleted {
                            entity: r.entity,
                            lap: r.laps_done,
                            time_s: t as f32 * PHYSICS_DT,
                        });
                    }
                }
                let done = if self.corridor.closed {
                    r.laps_done >= laps
                } else {
                    r.progress >= line_len - 1.0
                };
                if done {
                    r.status = RacerStatus::Finished { tick };
                    self.finishers += 1;
                    self.events.push(OpponentEvent::Finished {
                        entity: r.entity,
                        position: self.finishers,
                        time_s: tick as f32 * PHYSICS_DT,
                    });
                    r.v_cap = self.config.cooldown_speed_mps;
                    if r.race_speed.is_none() {
                        r.race_speed = Some(r.speed.clone());
                    }
                    let cap = r.cap_now;
                    r.speed = SpeedProfile::build(
                        &self.corridor,
                        &self.lines[r.line_idx].1,
                        &cap,
                        &r.params.usage,
                        r.v_cap,
                    );
                }
            }
            // Damage awareness.
            let e = obs.effects;
            let changed = {
                let a = &r.effects;
                let d = |x: f32, y: f32| (x - y).abs() > 0.05;
                d(a.power_scale, e.power_scale)
                    || d(a.overheating, e.overheating)
                    || d(a.drag_scale, e.drag_scale)
                    || (0..4).any(|i| {
                        d(a.grip_scale[i], e.grip_scale[i])
                            || d(a.brake_scale[i], e.brake_scale[i])
                            || d(a.tire_puncture[i], e.tire_puncture[i])
                    })
            };
            if changed {
                r.effects = e;
                r.cap_now = r.cap.damaged(&e);
                // Nurse an overheating engine.
                let heat_cap = r.cap_now.top_speed_mps * (1.0 - 0.35 * e.overheating);
                let v_cap = if matches!(r.status, RacerStatus::Finished { .. }) {
                    self.config.cooldown_speed_mps
                } else {
                    heat_cap
                };
                r.v_cap = v_cap;
                if r.flight.is_none() {
                    if r.race_speed.is_none() {
                        r.race_speed = Some(r.speed.clone());
                    }
                    r.speed = SpeedProfile::build(
                        &self.corridor,
                        &self.lines[r.line_idx].1,
                        &r.cap_now,
                        &r.params.usage,
                        v_cap,
                    );
                }
            }
            let wrecked = e.engine_dead || e.wheel_lost.iter().any(|w| *w) || st.health < 0.12;
            if wrecked && matches!(r.status, RacerStatus::Racing) {
                r.status = RacerStatus::Retired;
                r.tactic = Tactic::Park;
                self.events
                    .push(OpponentEvent::Retired { entity: r.entity });
            }
            // Pressure: someone right behind or alongside (filled in by perception) decays.
            r.pressure = (r.pressure - 0.5 * dt).max(0.0);
            // Police heat.
            let lvl = police.wanted_level(r.entity);
            if lvl > 0 || r.status == RacerStatus::Fled || r.tactic == Tactic::Yield {
                let at = police_at.get_or_insert_with(|| police.police_positions());
                self.react_to_police(k, world, police, at, lvl, dt);
            }
        }
        // Civilian contact = DQ.
        if self.config.civilian_contact_dq {
            for k in 0..self.racers.len() {
                let r = &self.racers[k];
                if !matches!(r.status, RacerStatus::Racing | RacerStatus::Fled) {
                    continue;
                }
                let me = Obb {
                    c: r.pos,
                    f: r.fwd,
                    hl: 0.5 * r.cap.length_m,
                    hw: 0.5 * r.cap.width_m,
                };
                for a in world.agents() {
                    let c = geom::xy(a.pos);
                    if geom::dist(c, r.pos) > 8.0 {
                        continue;
                    }
                    let other = Obb {
                        c,
                        f: geom::dir_of_yaw(a.yaw),
                        hl: 0.5 * CAR_LENGTH_M,
                        hw: 0.5 * CAR_WIDTH_M,
                    };
                    // The racer's fault only if it is moving into the civilian (kinematic
                    // traffic can drive into a stopped racer; that is traffic's failure).
                    let toward = {
                        let st = &vehicles.iter().find(|v| v.entity == r.entity);
                        st.map(|v| {
                            let d = geom::normalize(geom::sub(c, r.pos));
                            geom::dot([v.state.lin_vel[0], v.state.lin_vel[1]], d)
                        })
                        .unwrap_or(0.0)
                    };
                    // ... and only while driving: a car shoved into a civilian (by police,
                    // say) with its foot off the throttle did nothing wrong.
                    let driving = r.throttle > 0.05;
                    if geom::obb_overlap(&me, &other) && toward > 0.5 && driving {
                        let r = &mut self.racers[k];
                        if r.status == RacerStatus::Racing {
                            // Out of the race; drives on at cool-down pace (a car parked on
                            // the road would block traffic and the field).
                            r.status = RacerStatus::Disqualified;
                            r.v_cap = self.config.cooldown_speed_mps;
                            self.events.push(OpponentEvent::Disqualified {
                                entity: r.entity,
                                civilian: a.id,
                                time_s: self.tick as f32 * PHYSICS_DT,
                            });
                        }
                        break;
                    }
                }
            }
        }
    }

    /// Flee, yield or keep going for racer `k` wanted at `lvl` (see the module docs).
    fn react_to_police(
        &mut self,
        k: usize,
        world: &World,
        police: &dyn PoliceQuery,
        police_at: &[V2],
        lvl: u8,
        dt: f32,
    ) {
        let time_s = self.tick as f32 * PHYSICS_DT;
        let r = &mut self.racers[k];
        let chased = police.in_pursuit(r.entity);
        let nearest = police_at
            .iter()
            .fold(f32::INFINITY, |m, p| m.min(geom::dist(*p, r.pos)));
        match r.status {
            RacerStatus::Racing
                if self.config.allow_abandon && lvl >= r.params.flee_wanted_level =>
            {
                r.status = RacerStatus::Fled;
                r.yield_s = 0.0;
                self.events.push(OpponentEvent::Abandoned {
                    entity: r.entity,
                    wanted_level: lvl,
                });
                self.start_flight(k, world, police_at);
            }
            RacerStatus::Racing | RacerStatus::Finished { .. } | RacerStatus::Disqualified => {
                // Wanted below my flee level: keep going until a unit is right on me, then
                // pull over and take the ticket.
                if r.tactic == Tactic::Yield {
                    if lvl == 0 || !chased {
                        r.tactic = Tactic::Rejoin;
                        r.tactic_ticks = 0;
                        r.yield_s = 0.0;
                    }
                } else if lvl > 0 && chased && nearest < YIELD_RADIUS_M {
                    r.yield_s += dt;
                    if r.yield_s >= YIELD_AFTER_S && r.tactic != Tactic::Park {
                        r.tactic = Tactic::Yield;
                        r.tactic_ticks = 0;
                        r.pass_target = None;
                        self.events.push(OpponentEvent::Yielded {
                            entity: r.entity,
                            wanted_level: lvl,
                        });
                    }
                } else {
                    r.yield_s = (r.yield_s - dt).max(0.0);
                }
            }
            RacerStatus::Fled if !r.escaped => {
                if lvl == 0 && !chased {
                    r.escaped = true;
                    r.v_cap = self.config.cooldown_speed_mps;
                    self.events.push(OpponentEvent::Escaped {
                        entity: r.entity,
                        time_s,
                    });
                    return;
                }
                r.replan_s += dt;
                let Some(f) = &r.flight else {
                    // No route could be planned last time: try again now and then.
                    if r.replan_s >= FLIGHT_REPLAN_S {
                        self.start_flight(k, world, police_at);
                    }
                    return;
                };
                let near_end = r.s > f.corridor.length_m - 40.0;
                // Police waiting further along my route (not the ones on my tail: a chaser
                // behind is beaten by driving on, not by turning around).
                let ahead: Vec<V2> = police_at
                    .iter()
                    .copied()
                    .filter(|q| geom::dot(geom::sub(*q, r.pos), r.fwd) > 15.0)
                    .collect();
                let threat_ahead = r.replan_s >= FLIGHT_REPLAN_S && !ahead.is_empty() && {
                    let mut x = 20.0;
                    let mut hit = false;
                    while x <= FLIGHT_LOOKAHEAD_M && !hit {
                        let i = ((r.s + x) / CORRIDOR_STEP_M) as isize;
                        let p = geom::xy(f.corridor.sample(i).pos);
                        hit = ahead.iter().any(|q| geom::dist(*q, p) < FLIGHT_THREAT_M);
                        x += 4.0 * CORRIDOR_STEP_M;
                    }
                    hit
                };
                if (near_end && r.replan_s >= 1.0) || threat_ahead {
                    self.start_flight(k, world, police_at);
                }
            }
            _ => {}
        }
    }

    /// Plan (or re-plan) racer `k`'s flight route away from `police_at`. Keeps the old route
    /// when no new one plans; parks when there is none at all.
    fn start_flight(&mut self, k: usize, world: &World, police_at: &[V2]) {
        let flight = plan_flight(world, &self.racers[k], &self.config, police_at);
        let r = &mut self.racers[k];
        r.replan_s = 0.0;
        match flight {
            Some(f) => {
                let cp = f.corridor.project(r.pos, None, 0);
                r.hint = cp.index;
                r.s = cp.s;
                r.n = cp.n;
                r.n_cmd = cp.n;
                if matches!(
                    r.tactic,
                    Tactic::Park | Tactic::Yield | Tactic::Overtake | Tactic::Defend
                ) {
                    r.tactic = Tactic::Follow;
                }
                r.pass_target = None;
                r.flight = Some(Box::new(f));
            }
            None if r.flight.is_none() => r.tactic = Tactic::Park,
            None => {}
        }
    }

    /// Step 2 for the shared corridor: every vehicle and nearby civilian as an obstacle.
    fn perceive_all(&mut self, world: &World, vehicles: &[VehicleObs]) -> Vec<Obstacle> {
        let mut out = Vec::new();
        let range = self.racers.iter().fold(0.0f32, |m, r| m.max(r.ahead_m())) + 20.0;
        for v in vehicles {
            let who = match self.racer_index(v.entity) {
                Some(k) => {
                    let r = &self.racers[k];
                    if !r.in_race() {
                        // A fleeing, busted or retired racer is just another vehicle on the
                        // race corridor (parked once it has stood still long enough: a car
                        // busted on a hairpin is passed like any car stopped there).
                        Who::Vehicle(r.entity)
                    } else {
                        // Sliding, running wide or reversing (it swings out): give it room.
                        let unstable = r.unstable || r.running_wide || r.tactic == Tactic::Reverse;
                        out.push(Obstacle {
                            who: Who::Racer(k),
                            s: r.s,
                            n: r.n,
                            v_s: r.v_s,
                            v_n: r.v_n,
                            half_len: r.ext.0,
                            half_w: r.ext.1,
                            unstable,
                            passing: r.tactic == Tactic::Overtake,
                            parked: matches!(r.tactic, Tactic::Park | Tactic::Yield),
                            facing: 1.0,
                        });
                        continue;
                    }
                }
                None => Who::Vehicle(v.entity),
            };
            let pos = geom::xy(v.state.pos);
            if !self.racers.iter().any(|r| geom::dist(r.pos, pos) < range) {
                continue;
            }
            let hint = self.hints.get(&v.entity).copied();
            let cp = self.corridor.project(pos, hint, 60);
            self.hints.insert(v.entity, cp.index);
            if let Some(mut o) = vehicle_obstacle(&self.corridor, who, cp, v) {
                o.parked = self.is_still(STILL_VEHICLE_KEY + v.entity.0 as u64);
                out.push(o);
            }
        }
        let mut civ = civilians_on(&self.corridor, &self.racers, world, range, false);
        for o in &mut civ {
            if let Who::Civilian(id) = o.who {
                o.parked |= self.is_still(id as u64);
            }
        }
        out.append(&mut civ);
        out
    }

    /// Step 2 for a fleeing racer's private route.
    fn perceive_on(
        &self,
        corridor: &Corridor,
        k: usize,
        world: &World,
        vehicles: &[VehicleObs],
    ) -> Vec<Obstacle> {
        let me = &self.racers[k];
        let mut out = Vec::new();
        for v in vehicles {
            if v.entity == me.entity {
                continue;
            }
            let pos = geom::xy(v.state.pos);
            if geom::dist(pos, me.pos) > me.ahead_m() + 20.0 {
                continue;
            }
            let cp = corridor.project(pos, Some(me.hint), 80);
            if let Some(mut o) = vehicle_obstacle(corridor, Who::Vehicle(v.entity), cp, v) {
                o.parked = self.is_still(STILL_VEHICLE_KEY + v.entity.0 as u64);
                out.push(o);
            }
        }
        let only_me = std::slice::from_ref(me);
        let mut civ = civilians_on(corridor, only_me, world, me.ahead_m() + 20.0, true);
        for o in &mut civ {
            if let Who::Civilian(id) = o.who {
                o.parked |= self.is_still(id as u64);
            }
        }
        out.append(&mut civ);
        out
    }

    /// Step 3 + 4 for racer `k`.
    /// Civilians near any racer, as `(id, position, velocity)` (junction awareness).
    fn civilians_near(&self, world: &World) -> Vec<Civ> {
        let mut out = Vec::new();
        for a in world.agents() {
            let p = geom::xy(a.pos);
            if self.racers.iter().any(|r| geom::dist(p, r.pos) < 160.0) {
                out.push(Civ {
                    id: a.id,
                    pos: p,
                    vel: geom::scale(geom::dir_of_yaw(a.yaw), a.speed_mps),
                    turn_signal: a.turn_signal,
                });
            }
        }
        out
    }

    /// Signal state of the next junction entry on racer `k`'s track (None: unsignalised,
    /// too far, or signals ignored by the race rules).
    fn signal_ahead(&self, k: usize, world: &World) -> Option<SignalAhead> {
        if !self.config.signals {
            return None;
        }
        let r = &self.racers[k];
        let corridor = match &r.flight {
            Some(f) => &f.corridor,
            None => &self.corridor,
        };
        let i0 = corridor.wrap((r.s / CORRIDOR_STEP_M) as isize);
        let steps = (SIGNAL_LOOKAHEAD_M / CORRIDOR_STEP_M) as usize;
        let (ahead, lane) = corridor.next_junction_lane(i0, steps)?;
        let (state, remaining_s) = world.map().lane_signal(lane)?;
        Some(SignalAhead {
            d_entry: ahead as f32 * CORRIDOR_STEP_M,
            state,
            remaining_s,
        })
    }

    fn decide(
        &mut self,
        k: usize,
        obstacles: &[Obstacle],
        civs: &[Civ],
        signal: Option<SignalAhead>,
        dt: f32,
    ) {
        // Take the racer's own track data out while it is mutated.
        let flight = self.racers[k].flight.take();
        let speed = std::mem::replace(
            &mut self.racers[k].speed,
            SpeedProfile {
                v: Vec::new(),
                a_lat: 0.0,
                a_brake: 0.0,
            },
        );
        let line_idx = self.racers[k].line_idx;
        let (corridor, line, profile): (&Corridor, &RacingLine, &SpeedProfile) = match &flight {
            Some(f) => (&f.corridor, &f.line, &f.profile),
            None => (&self.corridor, &self.lines[line_idx].1, &speed),
        };
        let draft = self.config.draft;
        let finish_m = if flight.is_some() {
            None
        } else {
            self.config.finish_m
        };
        decide_racer(
            &mut self.racers[k],
            civs,
            signal,
            draft,
            finish_m,
            k,
            corridor,
            line,
            profile,
            flight.is_some(),
            obstacles,
            dt,
        );
        self.racers[k].speed = speed;
        self.racers[k].flight = flight;
        if let Some(who) = self.racers[k].last_pass.take() {
            let other = match who {
                Who::Racer(j) => Some(self.racers[j].entity),
                Who::Vehicle(e) => Some(e),
                Who::Civilian(_) => None,
            };
            if let Some(other) = other {
                self.events.push(OpponentEvent::Overtook {
                    entity: self.racers[k].entity,
                    other,
                });
            }
        }
    }

    /// Last decision's debug readout for racer `k` (spec order). Obstacle ids: racers are
    /// `100 + spec index`, other vehicles their entity id, civilians `1_000_000 + agent id`.
    pub fn debug_of(&self, k: usize) -> Debug {
        self.racers[k].debug
    }

    /// Speed profile racer `k` raced to (before any cool-down / damage rebuild).
    pub fn race_profile_of(&self, k: usize) -> &SpeedProfile {
        let r = &self.racers[k];
        r.race_speed.as_ref().unwrap_or(&r.speed)
    }

    /// Racer `k`'s flight route (corridor) while fleeing, if any.
    pub fn flight_of(&self, k: usize) -> Option<&Corridor> {
        self.racers[k].flight.as_ref().map(|f| &f.corridor)
    }

    /// Who racer `k` (spec order) has passed on the road (debug / tests).
    pub fn passes_by(&self, k: usize) -> usize {
        self.racers[k].passes as usize
    }
}

/// Steps 3 + 4 for one racer on its current track.
#[allow(clippy::too_many_arguments)]
fn decide_racer(
    r: &mut Racer,
    civs: &[Civ],
    signal: Option<SignalAhead>,
    draft: bool,
    finish_m: Option<f32>,
    k: usize,
    corridor: &Corridor,
    line: &RacingLine,
    profile: &SpeedProfile,
    fleeing: bool,
    obstacles: &[Obstacle],
    dt: f32,
) {
    {
        let p = r.params;
        let hw = 0.5 * r.cap.width_m;
        let hl = 0.5 * r.cap.length_m;
        let v = r.v.max(0.0);
        let line_n = |s: f32| corridor.interp(&line.offset, s);
        let to_limits = |(l, rr): (f32, f32)| {
            let lo = -l + hw + 0.25;
            let hi = rr - hw - 0.25;
            if lo <= hi {
                (lo, hi)
            } else {
                (0.5 * (lo + hi), 0.5 * (lo + hi))
            }
        };
        let limits = |s: f32| to_limits(corridor.bounds(s));
        // Overtaking may use the opposing lanes on open roads: risk-takers always, everyone
        // to get round a pulled-over racer just ahead.
        let parked_ahead = obstacles.iter().any(|o| {
            o.parked && o.who != Who::Racer(k) && {
                let ds = corridor.delta_s(r.s, o.s);
                ds > -(hl + o.half_len) && ds < 60.0
            }
        });
        // Done racing (finished, disqualified): cool down in my lane, no passing — a pass on
        // the opposing lanes in front of a patrol after the flag is a needless violation.
        let done_racing = matches!(
            r.status,
            RacerStatus::Finished { .. } | RacerStatus::Disqualified
        ) || (!corridor.closed && finish_m.is_some_and(|f| r.progress >= f));
        // A non-racer crawling along ahead (under 70 % of my own pace here, or 6 m/s: a car
        // that just turned round, a slow climber) that has held me up for 4 s: even a careful
        // driver goes round it on the opposing lanes when the pass checks find them clear.
        let crawl_mps = (0.7 * corridor.interp(&profile.v, r.s + 30.0)).max(6.0);
        let crawler_ahead = r.blocked_s > 4.0
            && obstacles.iter().any(|o| {
                !matches!(o.who, Who::Racer(_)) && o.v_s > 0.5 && o.v_s < crawl_mps && {
                    let ds = corridor.delta_s(r.s, o.s);
                    ds > 0.0 && ds < 40.0
                }
            });
        let may_use_opposing = corridor.opposing == OpposingLanes::ForOvertaking
            && !done_racing
            && (r.profile.risk >= 0.5 || parked_ahead || crawler_ahead);
        let pass_limits = |s: f32| {
            if may_use_opposing {
                to_limits(corridor.pass_bounds(s))
            } else {
                limits(s)
            }
        };
        let v_line = |s: f32| corridor.interp(&profile.v, s);
        let me_who = Who::Racer(k);

        // Relative view of every obstacle.
        let rel: Vec<Rel> = obstacles
            .iter()
            .filter(|o| o.who != me_who && !matches!(o.who, Who::Vehicle(e) if e == r.entity))
            .map(|o| {
                let ds = corridor.delta_s(r.s, o.s);
                let gap = ds.abs() - hl - o.half_len;
                Rel { o: *o, ds, gap }
            })
            .filter(|x| x.ds > -p.mirror_range_m && x.ds < r.ahead_m())
            .collect();
        let is_civ = |o: &Obstacle| matches!(o.who, Who::Civilian(_));

        // Pressure from cars right behind / alongside.
        for x in &rel {
            if !is_civ(&x.o) && x.ds < 0.0 && x.gap < v.max(5.0) * 1.0 {
                r.pressure = 1.0;
            }
        }

        // Braking zone bookkeeping: braking-point error and line noise per zone.
        let ahead_drop = v_line(r.s + v * 1.0) < v - 3.0;
        if ahead_drop && !r.in_brake_zone {
            r.in_brake_zone = true;
            let amp = 1.0 + r.pressure * p.pressure_sensitivity;
            r.brake_err_s = (gauss(&mut r.rng) * p.brake_error_s * amp).clamp(-0.35, 0.35);
            r.line_noise = gauss(&mut r.rng) * p.line_noise_m * amp;
        } else if !ahead_drop && r.in_brake_zone && v_line(r.s + v * 0.5) > v + 1.0 {
            r.in_brake_zone = false;
            r.defended = false;
        }
        if line.curvature[corridor.wrap((r.s / CORRIDOR_STEP_M) as isize)].abs() > 1.0 / 60.0 {
            r.defended = false;
        }

        // Distance to the next braking point (profile drops 4 m/s below now), m; 0 while
        // braking for a corner.
        let brake_dist = if r.in_brake_zone {
            0.0
        } else {
            let mut d = f32::INFINITY;
            let here = v_line(r.s).min(v.max(5.0));
            let mut x = 0.0;
            while x < 250.0 {
                if v_line(r.s + x) < here - 4.0 {
                    d = x;
                    break;
                }
                x += CORRIDOR_STEP_M;
            }
            d
        };

        // Start discipline: hold the grid lane, no passing, until the field has strung out.
        let start_phase = !fleeing && r.laps_done == 0 && r.progress < START_HOLD_M;

        // ---- Tactical state machine ----
        let (lim_lo, lim_hi) = limits(r.s);
        let (pass_lo, pass_hi) = pass_limits(r.s);
        r.tactic_ticks += 1;
        let parked = matches!(r.status, RacerStatus::Retired | RacerStatus::Busted)
            || r.tactic == Tactic::Park;
        if parked {
            r.tactic = Tactic::Park;
        } else if r.tactic == Tactic::Yield {
            // Pulled over for the police: hold still until released.
            r.stuck_s = 0.0;
        } else {
            // Off the corridor / facing the wrong way → rejoin.
            let t = corridor.samples[r.hint].tangent;
            let heading_cos = geom::dot(r.fwd, t);
            // Off = outside the road itself (opposing lanes included where the corridor
            // knows them), not merely out of my lane: a car drifting over the centre line
            // in a hairpin steers back at speed instead of crawling back at rejoin pace.
            let (road_lo, road_hi) = to_limits(corridor.pass_bounds(r.s));
            // Grid slots may sit beside the lane (a two-wide grid on a one-lane road): a car
            // pulling away from one steers in at race pace.
            let on_grid = r.laps_done == 0 && r.progress < 30.0;
            let off =
                !on_grid && (r.n < road_lo.min(pass_lo) - 1.0 || r.n > road_hi.max(pass_hi) + 1.0);
            if r.tactic != Tactic::Reverse
                && r.tactic != Tactic::Rejoin
                && (off || heading_cos < 0.5)
            {
                r.tactic = Tactic::Rejoin;
                r.tactic_ticks = 0;
                r.stable_s = 0.0;
            }
            // Stuck against something → back out.
            let wants_to_move = v_line(r.s) > 3.0;
            let held_by_car = rel
                .iter()
                .any(|x| x.ds > 0.0 && x.gap < 4.0 && x.o.v_s.abs() < 1.0);
            // Nose to nose in the opposing lanes (passed into a lane a car then stopped in,
            // facing me): nobody can move until I back out into my own lane.
            // Or in my own lane facing a standing car that faces me (a civilian waiting to
            // turn across a hairpin, waiting in turn for me to move).
            let head_on = r.debug.v_target < 0.5
                && rel.iter().any(|x| {
                    x.ds > 0.0
                        && x.gap < 8.0
                        && x.o.v_s.abs() < 1.0
                        && !matches!(x.o.who, Who::Racer(_))
                        && (r.n < lim_lo - 1.0 || x.o.facing < -0.3)
                });
            if r.tactic != Tactic::Reverse
                && v.abs() < 0.8
                && wants_to_move
                && ((!held_by_car && r.throttle > 0.3) || head_on)
            {
                r.stuck_s += dt;
            } else if r.tactic != Tactic::Reverse {
                r.stuck_s = 0.0;
            }
            if r.stuck_s > 2.0 {
                r.tactic = Tactic::Reverse;
                r.tactic_ticks = 0;
                r.stuck_s = 0.0;
                // Nose to nose: back out far enough to steer into my own lane.
                r.reverse_s = if head_on { 3.0 } else { 1.6 };
            }
        }

        // Pass-target bookkeeping.
        let target_rel = |w: Who| rel.iter().find(|x| x.o.who == w);
        match r.tactic {
            Tactic::Overtake => {
                let done = match r.pass_target.and_then(target_rel) {
                    Some(x) => {
                        if x.ds < -(hl + x.o.half_len + 2.0) {
                            r.passes += 1;
                            r.last_pass = Some(x.o.who);
                            true
                        } else {
                            let alongside = x.ds.abs() < hl + x.o.half_len;
                            if alongside && brake_dist < v * 1.0 {
                                r.alongside_s += dt;
                            }
                            // Not alongside by the braking zone: tuck back in.
                            let too_late = brake_dist < v * 0.8 && !alongside && x.ds > 0.0;
                            // Side by side into the corner for longer than I tolerate while
                            // still behind: back out.
                            let lost = x.ds > 0.0 && r.alongside_s > p.side_by_side_s;
                            let gone = x.gap > pass_horizon(v, x.o.v_s) + 10.0;
                            // Something coming the other way in my lane: tuck back in.
                            let oncoming = rel.iter().any(|q| {
                                q.ds > 0.0
                                    && q.o.v_s < -1.0
                                    && (q.o.n - r.n_goal).abs() < hw + q.o.half_w + 1.0
                                    && q.gap / (v + -q.o.v_s).max(1.0) < 3.5
                            });
                            let stale = r.tactic_ticks as f32 * dt > 15.0;
                            too_late || lost || gone || stale || oncoming
                        }
                    }
                    None => true,
                };
                if done {
                    r.tactic = Tactic::Follow;
                    r.pass_target = None;
                    r.blocked_s = 0.0;
                }
            }
            Tactic::Defend => {
                if brake_dist < v * 1.2 || r.tactic_ticks as f32 * dt > 6.0 {
                    r.tactic = Tactic::Follow;
                }
            }
            Tactic::Reverse => {
                // Never back into anything: stop reversing when something is close behind in
                // the band I occupy.
                let blocked_behind = rel.iter().any(|x| {
                    x.ds < 0.0
                        && x.gap < 4.0 + 1.0 * (-v).max(0.0)
                        && (x.o.n - r.n).abs() < hw + x.o.half_w + 1.2
                });
                if r.tactic_ticks as f32 * dt > r.reverse_s || blocked_behind {
                    r.tactic = Tactic::Rejoin;
                    r.tactic_ticks = 0;
                    r.ctrl.resync(0.0);
                }
            }
            Tactic::Rejoin => {
                let t = corridor.samples[r.hint].tangent;
                let (road_lo, road_hi) = to_limits(corridor.pass_bounds(r.s));
                let ok = r.n >= road_lo.min(lim_lo) - 0.3
                    && r.n <= road_hi.max(lim_hi) + 0.3
                    && geom::dot(r.fwd, t) > 0.94;
                if ok {
                    r.stable_s += dt;
                } else {
                    r.stable_s = 0.0;
                }
                if r.stable_s > 0.5 {
                    r.tactic = Tactic::Follow;
                }
            }
            _ => {}
        }

        // Lateral band I occupy / am heading into.
        let my_band = |n_a: f32, n_b: f32| (n_a.min(n_b) - hw, n_a.max(n_b) + hw);
        let overlaps = |band: (f32, f32), o: &Obstacle, margin: f32, t_reach: f32| {
            // Civilians may be crossing: extrapolate up to 3 s. Racers steer toward their
            // lines all the time; only their next half second of drift counts.
            let horizon = if matches!(o.who, Who::Civilian(_)) {
                3.0
            } else {
                0.5
            };
            let drift = o.v_n * t_reach.min(horizon);
            let lo = o.n.min(o.n + drift) - o.half_w - margin;
            let hi = o.n.max(o.n + drift) + o.half_w + margin;
            band.0 < hi && band.1 > lo
        };
        // Civilian clearance grows with my speed: creep past a stopped car with 0.4 m, pass
        // at 15 m/s or more with the full margin.
        let civ_margin = 0.4 + (p.civilian_margin_m - 0.4) * (v / 15.0).clamp(0.0, 1.0);
        let margin_for = |o: &Obstacle| -> f32 {
            let base = if matches!(o.who, Who::Civilian(_)) {
                civ_margin
            } else {
                LAT_MARGIN_M
            };
            if parked_non_racer(o) {
                return parked_gap(v, base);
            }
            base + if o.unstable { 1.5 } else { 0.0 }
        };

        // Follow mode: who blocks the line? Held-up evidence = last decision's speed was set
        // by that car while my own pace here is clearly higher.
        let mut tuck: Option<f32> = None;
        if r.tactic == Tactic::Follow {
            let band = my_band(r.n, r.n_cmd);
            // Look for a pass early enough to change lanes without lifting: at least 30 m,
            // or 3.5 s at the closing speed (a civilian doing 12 m/s on a highway).
            let blocker = rel
                .iter()
                .filter(|x| x.ds > 0.0 && x.gap < pass_horizon(v, x.o.v_s))
                .filter(|x| overlaps(band, &x.o, margin_for(&x.o), x.gap / (v - x.o.v_s).max(1.0)))
                .min_by(|a, b| a.ds.total_cmp(&b.ds));
            let held = blocker.is_some_and(|x| {
                r.debug.limit == SpeedLimit::Obstacle(who_id(x.o.who))
                    && v_line(r.s) > r.debug.v_target + 1.0
            });
            if held {
                r.blocked_s += dt;
            } else {
                r.blocked_s = (r.blocked_s - 0.5 * dt).max(0.0);
            }
            if r.blocked_s <= 0.0
                || r.get_run
                    .is_some_and(|w| blocker.is_none_or(|b| b.o.who != w))
            {
                r.get_run = None;
            }
            if let Some(x) = blocker {
                // Slipstream position: tuck in right behind on a straight while waiting for
                // a chance (positioning only — wp_vehicle has no draft model yet).
                if held && !is_civ(&x.o) && brake_dist > v * 2.0 && x.gap < 25.0 {
                    tuck = Some(x.o.n);
                }
                // Patience when stuck behind; none needed when arriving with a real run.
                // A run: real closing speed, or (with slipstream) a smaller one built in the
                // tow right behind it.
                let tow_run = draft && v - x.o.v_s > 0.8 && x.gap < 12.0;
                let momentum = (v - x.o.v_s > 2.5 || tow_run)
                    && x.gap < pass_horizon(v, x.o.v_s)
                    && brake_dist > v * 1.5;
                if (r.blocked_s > p.overtake_patience_s || momentum)
                    && x.o.v_s > -1.0
                    && !done_racing
                    && (!start_phase || x.o.parked)
                {
                    let ctx = PassCtx {
                        corridor,
                        line,
                        profile,
                        brake_dist,
                        v,
                        margin: margin_for(&x.o),
                        draft,
                    };
                    let plan = plan_pass(r, x, &rel, &ctx, &pass_limits);
                    let slot = match plan {
                        Ok(_) => 0,
                        Err(e) => e as usize,
                    };
                    r.pass_checks[slot] += 1;
                    if let Ok(n_pass) = plan {
                        r.tactic = Tactic::Overtake;
                        r.tactic_ticks = 0;
                        r.alongside_s = 0.0;
                        r.pass_target = Some(x.o.who);
                        r.n_goal = n_pass;
                        r.get_run = None;
                    } else if !is_civ(&x.o) {
                        r.get_run = Some(x.o.who);
                    }
                }
            }
        }

        // Defend: one move toward the attacker's side.
        if r.tactic == Tactic::Follow
            && p.defends
            && !r.defended
            && !fleeing
            && !start_phase
            && !r.in_brake_zone
            && v > 15.0
            && brake_dist > v * 2.5
            && line.curvature[corridor.wrap((r.s / CORRIDOR_STEP_M) as isize)].abs() < 1.0 / 150.0
        {
            let attacker = rel.iter().find(|x| {
                !is_civ(&x.o)
                    && x.ds < 0.0
                    && x.gap > 3.0
                    && x.gap < 12.0
                    && x.o.v_s > v + 0.5
                    && (x.o.n - r.n).abs() > 1.0
            });
            let alongside = rel.iter().any(|x| !is_civ(&x.o) && x.gap < 1.0);
            if let (Some(a), false) = (attacker, alongside) {
                let dir = (a.o.n - r.n).signum();
                r.n_goal = (r.n + dir * (2.0 * hw).min((a.o.n - r.n).abs())).clamp(lim_lo, lim_hi);
                r.tactic = Tactic::Defend;
                r.tactic_ticks = 0;
                r.defended = true;
            }
        }

        // Re-aim an overtake at the target's current position.
        if r.tactic == Tactic::Overtake {
            if let Some(x) = r.pass_target.and_then(target_rel) {
                let side = (r.n_goal - x.o.n).signum();
                let need = hw + x.o.half_w + margin_for(&x.o) + pass_margin(&x.o, &p);
                let want = x.o.n + side * need;
                let (lo, hi) = squeeze_limits(pass_limits(r.s + x.ds.max(0.0)), &x.o);
                if want < lo - 0.05 || want > hi + 0.05 {
                    if x.ds > 0.0 && x.gap > 0.0 {
                        r.tactic = Tactic::Follow;
                        r.pass_target = None;
                        r.blocked_s = 0.0;
                    }
                } else {
                    r.n_goal = want;
                }
            }
        }

        // Desired lateral position (absolute, at my s).
        let follow_w;
        let desired = match r.tactic {
            Tactic::Follow if start_phase => {
                follow_w = 0.0;
                r.start_n
            }
            Tactic::Follow => match tuck {
                Some(n) => {
                    follow_w = 0.0;
                    n
                }
                None => {
                    follow_w = 1.0;
                    line_n(r.s) + r.line_noise
                }
            },
            Tactic::Overtake | Tactic::Defend => {
                follow_w = 0.0;
                r.n_goal
            }
            Tactic::Rejoin => {
                follow_w = 0.0;
                line_n(r.s).clamp(lim_lo, lim_hi)
            }
            Tactic::Reverse => {
                follow_w = 0.0;
                r.n
            }
            Tactic::Park | Tactic::Yield => {
                follow_w = 0.0;
                lim_hi
            }
        };
        let rate = match r.tactic {
            // Lane changes run a little quicker at speed (up to 1.6×), but taper off above
            // 30 m/s: a lane change at highway speed while braking hard has to stay gentle or
            // the car overshoots the target lane.
            Tactic::Overtake | Tactic::Defend => {
                (1.4 + 1.0 * r.profile.aggression)
                    * (v / 15.0).clamp(1.0, 1.6)
                    * (30.0 / v.max(30.0))
            }
            Tactic::Park | Tactic::Yield => 0.8,
            _ => 0.9,
        };
        if r.n_cmd.is_nan() {
            r.n_cmd = r.n;
        }
        let (act_lo, act_hi) = if r.tactic == Tactic::Overtake {
            match r.pass_target.and_then(target_rel) {
                Some(x) => squeeze_limits((pass_lo, pass_hi), &x.o),
                None => (pass_lo, pass_hi),
            }
        } else {
            (lim_lo, lim_hi)
        };
        let mut n_cmd = approach(r.n_cmd, desired.clamp(act_lo, act_hi), rate * dt);

        // Hard side constraints: never move toward a car that overlaps me longitudinally.
        // (Bounds are the active ones: an overtake may use the opposing lanes.)
        let mut lo = act_lo.min(r.n);
        let mut hi = act_hi.max(r.n);
        let mut squeezed_by_ahead = false;
        for x in &rel {
            // Alongside, or (mirrors) coming up from behind fast enough to be alongside
            // within 1.5 s.
            let alongside = x.ds.abs() <= hl + x.o.half_len + 2.5;
            let arriving = x.ds < 0.0 && x.gap < 1.5 * (x.o.v_s - v).max(0.0) + 2.5;
            if !alongside && !arriving {
                continue;
            }
            let civ_extra = if is_civ(&x.o) { civ_margin } else { 0.0 };
            let side_gap = if parked_non_racer(&x.o) {
                parked_gap(v, SIDE_GAP_M + civ_extra)
            } else {
                SIDE_GAP_M + civ_extra
            };
            let need = hw + x.o.half_w + side_gap;
            // Lateral closing speed (either car drifting toward the other) eats clearance.
            if x.o.n >= r.n {
                let closing = (r.v_n - x.o.v_n).max(0.0);
                hi = hi.min(x.o.n - need - LATERAL_LEAD_S * closing);
            } else {
                let closing = (x.o.v_n - r.v_n).max(0.0);
                lo = lo.max(x.o.n + need + LATERAL_LEAD_S * closing);
            }
            if x.ds > 0.0 {
                r.pressure = 1.0;
            }
        }
        if lo > hi {
            // No room: hold my lateral position and let the car behind back out.
            n_cmd = r.n;
            squeezed_by_ahead = rel
                .iter()
                .any(|x| x.ds > 0.0 && x.ds < hl + x.o.half_len + 1.5);
        } else {
            n_cmd = n_cmd.clamp(lo, hi.max(lo));
        }
        r.n_cmd = n_cmd;

        // ---- Longitudinal target ----
        let mut v_target = v_line(r.s + v * (0.15 + p.early_brake_s + r.brake_err_s));
        let mut limit = SpeedLimit::Profile;
        let mut cap_to = |v_target: &mut f32, cap: f32, why: SpeedLimit| {
            if cap < *v_target {
                *v_target = cap;
                limit = why;
            }
        };
        cap_to(&mut v_target, r.v_cap, SpeedLimit::Mode);
        if r.status == RacerStatus::Fled && fleeing && r.s > corridor.length_m - 15.0 {
            r.tactic = Tactic::Park;
        }
        // Done with a point-to-point race: pull over before the corridor runs out (past its
        // end there is only whatever the road leads into — a car park wall, a dead end).
        if matches!(r.status, RacerStatus::Finished { .. })
            && !corridor.closed
            && r.s > corridor.length_m - 25.0
        {
            r.tactic = Tactic::Park;
        }
        // Pulling over for the police, or out of the race (busted, retired, fled): roll on slowly
        // until at the kerb and clear of any junction, then stop there (a car stopped
        // mid-lane blocks everyone behind it — Hollins: a racer busted on a hairpin held the
        // rest of the field until the finish window closed).
        let out_of_race = matches!(
            r.status,
            RacerStatus::Busted | RacerStatus::Retired | RacerStatus::Fled
        );
        let pulling_over = (r.tactic == Tactic::Yield || (r.tactic == Tactic::Park && out_of_race))
            && {
                let i0 = (r.s / CORRIDOR_STEP_M) as isize;
                let near_junction = (-3..=12).any(|j| corridor.sample(i0 + j).junction);
                (r.n - lim_hi).abs() > 0.5 || near_junction
            };
        match r.tactic {
            // Rejoin at a crawl when pointing the wrong way, briskly when merely beside the
            // corridor and pointing along it.
            Tactic::Rejoin => {
                let t = corridor.samples[r.hint].tangent;
                let aligned = ((geom::dot(r.fwd, t) - 0.9) / 0.1).clamp(0.0, 1.0);
                cap_to(&mut v_target, 7.0 + 10.0 * aligned, SpeedLimit::Mode)
            }
            Tactic::Yield | Tactic::Park if pulling_over => {
                cap_to(&mut v_target, 6.0, SpeedLimit::Mode)
            }
            Tactic::Park | Tactic::Yield => cap_to(
                &mut v_target,
                (v - 3.0 * dt * 4.0).max(0.0),
                SpeedLimit::Mode,
            ),
            _ => {}
        }
        // Running wide in a corner (the car can't hold the commanded line): lift, and let
        // the others know (they give unstable cars extra room).
        let k_here = line.curvature[corridor.wrap((r.s / CORRIDOR_STEP_M) as isize)];
        let (outward, drift_out) = if k_here.abs() > 1.0 / 120.0 {
            let out = -k_here.signum();
            ((r.n - n_cmd) * out, r.v_n * out)
        } else {
            (0.0, 0.0)
        };
        r.running_wide = outward > 1.5 && drift_out > 0.5;
        if r.unstable {
            cap_to(&mut v_target, v - 1.5, SpeedLimit::Unstable);
        } else if r.running_wide {
            cap_to(&mut v_target, v - 0.7, SpeedLimit::Unstable);
        }
        // Off the racing line (passing, defending, squeezed): the path at constant offset
        // `n_cmd` has its own curvature — slow for it (inside lines are tighter).
        let a_brk = profile.a_brake.max(3.0) * 0.85;
        // Check both the commanded offset and where the car actually is.
        for n_path in [n_cmd, r.n] {
            if (n_path - line_n(r.s)).abs() <= 1.0 {
                continue;
            }
            let mut x = 0.0;
            let horizon = (v * v / (2.0 * a_brk) + 20.0).min(150.0);
            while x <= horizon {
                let kc = corridor.centre_curvature(r.s + x);
                let den = (1.0 - kc * n_path).max(0.2);
                let k = (kc / den).abs();
                if k > 1e-4 {
                    // 85 % of the grip used on the line: an off-line path is tighter, not
                    // optimised, and usually has a car beside it to run wide into.
                    let v_off = (0.85 * profile.a_lat / k).sqrt();
                    let cap = (v_off * v_off + 2.0 * a_brk * x).sqrt();
                    cap_to(&mut v_target, cap, SpeedLimit::Mode);
                }
                x += 2.0 * CORRIDOR_STEP_M;
            }
        }
        // Junction awareness: while a civilian is heading into (or is inside) the next
        // junction box, keep the ability to stop before entering it. With a green that lasts
        // until I am through, cross traffic is held at its red: only civilians already moving
        // inside the box count. At a red, careful drivers stop at the line; the rest go
        // through only when nobody is heading in.
        let mut held_by_stopped = false;
        if !civs.is_empty() || signal.is_some() {
            let i0 = corridor.wrap((r.s / CORRIDOR_STEP_M) as isize);
            let horizon = (v * v / (2.0 * a_brk) + 30.0).min(130.0);
            let steps = (horizon / CORRIDOR_STEP_M) as isize;
            let in_junction = corridor.samples[i0].junction;
            // Already in a junction box: its remaining part counts, entry distance 0.
            let entry = if in_junction {
                Some(0)
            } else {
                (1..steps).find(|&j| corridor.sample(i0 as isize + j).junction)
            };
            if let Some(j) = entry {
                let mut j_end = j;
                while j_end < j + 60 && corridor.sample(i0 as isize + j_end + 1).junction {
                    j_end += 1;
                }
                let c_j = geom::xy(corridor.sample(i0 as isize + (j + j_end) / 2).pos);
                let radius = ((j_end - j) as f32 * CORRIDOR_STEP_M * 0.5).max(8.0);
                let d_entry = j as f32 * CORRIDOR_STEP_M;
                let t_dir = corridor.sample(i0 as isize + (j + j_end) / 2).tangent;
                let watch = 25.0 + 20.0 * r.profile.awareness;
                // The signal belongs to this junction only if its entry is this entry.
                let sig = signal.filter(|g| {
                    !in_junction && (g.d_entry - d_entry).abs() <= 2.5 * CORRIDOR_STEP_M
                });
                let v_app = v.max(4.0);
                let t_arrive = d_entry / v_app;
                let t_through = t_arrive + (j_end - j) as f32 * CORRIDOR_STEP_M / v_app + 1.0;
                let stop_dist = v * v / (2.0 * a_brk);
                let can_stop = stop_dist < d_entry - 2.0;
                let protected =
                    sig.is_some_and(|g| g.state == SignalState::Green && g.remaining_s > t_through);
                let red_on_arrival = sig.is_some_and(|g| match g.state {
                    SignalState::Red => g.remaining_s > t_arrive - 0.5,
                    SignalState::Yellow => can_stop,
                    SignalState::Green => g.remaining_s < t_arrive && can_stop,
                    SignalState::Off => false,
                });
                // Nobody stops for a red with the police behind them.
                if red_on_arrival && can_stop && !fleeing && r.profile.risk < RED_LIGHT_RISK {
                    let free = (d_entry - 4.0).max(0.0);
                    let cap = (2.0 * a_brk * 0.8 * free).sqrt();
                    cap_to(&mut v_target, cap, SpeedLimit::Signal);
                }
                // My occupancy of the box: from reaching its entry to clearing its far
                // side, driving it as I would (accelerating from my speed, no faster than
                // my line through the box).
                let a_go = (r.cap.accel_mps2 * p.usage.accel).max(1.0);
                let t_at = |d: f32| -> f32 {
                    let u = v.max(0.0);
                    (-u + (u * u + 2.0 * a_go * d.max(0.0)).sqrt()) / a_go
                };
                let box_len = (j_end - j + 1) as f32 * CORRIDOR_STEP_M;
                let v_box = (j..=j_end)
                    .map(|q| {
                        corridor.interp(&profile.v, (i0 as isize + q) as f32 * CORRIDOR_STEP_M)
                    })
                    .fold(f32::INFINITY, f32::min)
                    .max(3.0);
                let t_in = t_at(d_entry);
                let clear = d_entry + box_len + 2.0 * hl;
                let t_out = t_at(clear).max(t_in + (box_len + 2.0 * hl) / v_box);
                // Crossing gap either side of my pass: 0.96 s (careful) .. 0.54 s (risk-taker).
                let gap_s = 0.6 * p.civilian_headway_s;
                // My swept strip through the box, in box coordinates (along `t_dir` from the
                // box middle, across from the corridor centre): the lane band I occupy now
                // and where my line takes me, the box length plus my car.
                let n_line = line_n(r.s + d_entry + 0.5 * box_len);
                let (n_a, n_b) = (r.n.min(n_line), r.n.max(n_line));
                let half_along = 0.5 * box_len + hl + 1.0;
                for c in civs {
                    let (id, pos, vel) = (&c.id, &c.pos, &c.vel);
                    let to = geom::sub(c_j, *pos);
                    let d = geom::len(to);
                    if d > radius + watch {
                        continue;
                    }
                    let speed = geom::len(*vel);
                    // Travelling with me through the junction: handled as a car ahead.
                    if speed > 0.5 && geom::dot(geom::scale(*vel, 1.0 / speed), t_dir) > 0.7 {
                        continue;
                    }
                    let rel_p = geom::sub(*pos, c_j);
                    let nrm = geom::right_of(t_dir);
                    let pa = geom::dot(rel_p, t_dir);
                    let pl = geom::dot(rel_p, nrm);
                    // Lateral room: both half widths and half a metre. A car signalling a
                    // turn may swing across: give it a lane more.
                    let side = hw + 0.9 + 0.5 + if c.turn_signal != 0 { 3.5 } else { 0.0 };
                    let (l_lo, l_hi) = (n_a - side, n_b + side);
                    let in_strip = pa.abs() <= half_along && pl >= l_lo && pl <= l_hi;
                    // Stopped civilians in my path through the box are waiting for me once
                    // I'm in the box myself (avoid a stand-off). A stopped car that has not
                    // moved while I waited is waiting for me: stop counting it.
                    let waited_out = r.junction_wait_s > JUNCTION_DEADLOCK_S;
                    let stopped_inside =
                        speed <= 0.5 && in_strip && !in_junction && !protected && !waited_out;
                    // A moving civilian: when does its present course enter / leave my strip?
                    // It only conflicts when that overlaps my own pass through the box, with
                    // my crossing gap either side. Cross traffic held at its red
                    // (`protected`) only counts while already moving inside the box.
                    let conflict = speed > 0.5 && (!protected || d < radius) && {
                        let ua = geom::dot(*vel, t_dir);
                        let ul = geom::dot(*vel, nrm);
                        let mut t0 = f32::NEG_INFINITY;
                        let mut t1 = f32::INFINITY;
                        for (p0, u, lo, hi) in
                            [(pa, ua, -half_along, half_along), (pl, ul, l_lo, l_hi)]
                        {
                            if u.abs() < 1e-3 {
                                if p0 < lo || p0 > hi {
                                    t0 = f32::INFINITY;
                                }
                            } else {
                                let (x0, x1) = ((lo - p0) / u, (hi - p0) / u);
                                t0 = t0.max(x0.min(x1));
                                t1 = t1.min(x0.max(x1));
                            }
                        }
                        let t_c_in = t0.max(0.0);
                        t1 > t_c_in && t_in - gap_s < t1 && t_c_in < t_out + gap_s
                    };
                    if stopped_inside || conflict {
                        if speed <= 0.5 {
                            held_by_stopped = true;
                        }
                        let free = if in_junction {
                            0.0
                        } else {
                            (d_entry - 3.0).max(0.0)
                        };
                        let cap = (2.0 * a_brk * 0.8 * free).sqrt();
                        cap_to(&mut v_target, cap, SpeedLimit::Obstacle(1_000_000 + id));
                    }
                }
            }
        }
        if held_by_stopped && v < 1.5 {
            r.junction_wait_s += dt;
        } else if v > 3.0 {
            r.junction_wait_s = 0.0;
        }
        // Obstacle caps: anything ahead in the band I'm driving through.
        let band = my_band(r.n, n_cmd);
        let mut decel_demand = 0.0f32;
        for x in &rel {
            if x.ds <= 0.0 {
                continue;
            }
            let o = &x.o;
            let civ = is_civ(o);
            let closing = (v - o.v_s).max(0.5);
            // Arriving fast next to a car: want more lateral room the faster I close.
            let margin =
                margin_for(o) + (0.15 * (closing - 2.0)).clamp(0.0, if civ { 0.5 } else { 1.0 });
            if !overlaps(band, o, margin, x.gap / closing) {
                continue;
            }
            // Steering out round a parked car (pulled over, or standing for good): once the
            // lane I am heading for is clear of it, creep instead of standing still behind it
            // (a stopped car cannot move sideways), never closer than the stop gap.
            let steering_round = o.parked
                && (r.tactic != Tactic::Overtake || r.pass_target == Some(o.who))
                && !overlaps(my_band(n_cmd, n_cmd), o, margin, 0.0);
            if steering_round && x.gap > CREEP_STOP_GAP_M {
                let creep = (a_brk * (x.gap - CREEP_STOP_GAP_M)).sqrt().min(CREEP_MPS);
                cap_to(&mut v_target, creep, SpeedLimit::Obstacle(who_id(o.who)));
                continue;
            }
            let in_corner = r.in_brake_zone
                || line.curvature[corridor.wrap((r.s / CORRIDOR_STEP_M) as isize)].abs()
                    > 1.0 / 80.0;
            // Held up on a straight with pace in hand: close up to a tow position (short
            // gap) before pulling out. With slipstream in the sim, stay flat out in the wake
            // down to the minimum gap (the braking-distance cap below still holds) so the
            // tow builds a run.
            let towing = (r.get_run == Some(o.who)
                || (draft && v_line(r.s + v) > o.v_s + 1.0 && x.gap < 25.0))
                && !in_corner
                && brake_dist > v * 2.0;
            // With slipstream, a time gap kept through the corner becomes a distance gap the
            // wake cannot bridge on the straight: stay tight on its exit too.
            let exit_tow = draft && !r.in_brake_zone && matches!(o.who, Who::Racer(_));
            let headway = if civ {
                p.civilian_headway_s
            } else if towing && draft {
                0.0
            } else if exit_tow {
                0.3
            } else if towing {
                (p.headway_s * 0.5).max(0.3)
            } else {
                p.headway_s
            } * if o.unstable { 1.6 } else { 1.0 };
            let min_gap = if civ { MIN_GAP_M + 2.0 } else { MIN_GAP_M };
            let vo = o.v_s.max(0.0);
            // Hold a time gap behind it (closing it over ~1.2 s), and never more speed than
            // can be shed before reaching the minimum gap if it brakes as hard as I can.
            let desired = min_gap + headway * vo;
            // Opening a gap is gradual (at most 3 m/s slower than it); the hard limit below
            // takes care of real emergencies.
            let v_follow = (vo + (x.gap - desired) / 1.2).max(vo - 3.0);
            let free = x.gap - min_gap;
            let v_brake = (vo * vo + 2.0 * a_brk * free.max(0.0)).sqrt();
            let cap = if free <= 0.0 {
                vo.min(v - 2.0).max(0.0)
            } else {
                v_follow.min(v_brake)
            };
            if v > vo && v > cap {
                // This car limits me: the deceleration needed to be at its speed by the
                // minimum gap.
                let a_req = (v * v - vo * vo) / (2.0 * free.max(0.5));
                decel_demand = decel_demand.max(a_req);
            }
            cap_to(
                &mut v_target,
                cap.max(0.0),
                SpeedLimit::Obstacle(who_id(o.who)),
            );
        }
        // Rejoining: wait for a racer coming from behind on my path.
        if r.tactic == Tactic::Rejoin {
            for x in &rel {
                if x.ds < 0.0 && x.ds > -50.0 && !is_civ(&x.o) && x.o.v_s > v + 3.0 {
                    let t_arrive = x.gap / (x.o.v_s - v).max(1.0);
                    if t_arrive < 3.0 && overlaps(band, &x.o, 1.0, 0.0) {
                        cap_to(&mut v_target, 0.0, SpeedLimit::RejoinWait);
                    }
                }
            }
        }
        // Squeezed side by side while behind, or past my side-by-side tolerance into a
        // braking zone: back out.
        let alongside_ahead = rel
            .iter()
            .find(|x| !is_civ(&x.o) && x.ds > 0.0 && x.ds < hl + x.o.half_len + 0.5);
        if let Some(x) = alongside_ahead {
            let into_corner = brake_dist < v * p.side_by_side_s && r.tactic != Tactic::Overtake;
            if x.o.parked || x.o.v_s.abs() < 1.0 {
                // Backing away from a stopped car gains nothing: creep past it.
                cap_to(&mut v_target, 4.0, SpeedLimit::BackOut(who_id(x.o.who)));
            } else if squeezed_by_ahead || into_corner || r.unstable {
                cap_to(
                    &mut v_target,
                    x.o.v_s - 1.5,
                    SpeedLimit::BackOut(who_id(x.o.who)),
                );
            }
        }

        // ---- Controls ----
        // Steering looks from where the car will be after its response lag (position and
        // heading 0.2 s on), so lane changes at highway speed don't overshoot and weave; the
        // lookahead stretches above 25 m/s for the same reason.
        const STEER_LAG_S: f32 = 0.2;
        let mut ld = r.ctrl.lookahead(v) + 0.3 * (v - 25.0).max(0.0) + v * STEER_LAG_S;
        // Held up and slowing (a car or a light ahead): aim no further than where I stop.
        // A target around the next corner turns the car in early, across the inside lane —
        // queued at a junction that is the oncoming lane (Lantern Row, round-12 tip: a racer
        // braking behind a queue ended nose to nose with a stopped oncoming car).
        if matches!(limit, SpeedLimit::Obstacle(_) | SpeedLimit::Signal) && v_target < v - 0.5 {
            ld = ld.min(1.5 + v * v / (2.0 * a_brk));
        }
        let s_t = r.s + ld;
        let (t_lo, t_hi) = if r.tactic == Tactic::Overtake {
            pass_limits(s_t)
        } else {
            limits(s_t)
        };
        // Curvature feedforward: in a tight corner the car settles wide of a pure-pursuit
        // target (tyre slip angles, steering response lag), by about κ·L²/8 (L = distance to
        // the target; measured on the Lantern junction turns, ~1.3 m). Aim that much inside
        // so the car holds the line instead of running out of the lane at the exit.
        let l_t = s_t - r.s;
        let k_mid = line.curvature[corridor.wrap(((r.s + 0.5 * l_t) / CORRIDOR_STEP_M) as isize)];
        let inside = (k_mid * l_t * l_t / 8.0).clamp(-1.5, 1.5);
        let n_t = (n_cmd + follow_w * (line_n(s_t) - line_n(r.s)) + inside)
            .clamp(t_lo.min(n_cmd), t_hi.max(n_cmd));
        let target = corridor.point(s_t, n_t);
        let pos_lag = geom::add(r.pos, geom::scale(r.vel, STEER_LAG_S));
        let (sn, cs) = wp_math::sin_cos(r.yaw_rate * STEER_LAG_S);
        let fwd_lag = [r.fwd[0] * cs - r.fwd[1] * sn, r.fwd[0] * sn + r.fwd[1] * cs];
        let mut steer = r.ctrl.steer(pos_lag, fwd_lag, target);
        steer = (steer - r.effects.steer_pull).clamp(-1.0, 1.0);
        let mut input = VehicleInput {
            headlights: true,
            ..Default::default()
        };
        if r.tactic == Tactic::Reverse {
            // Brake at a standstill engages reverse; the brake pedal then drives backwards
            // (at most 3 m/s: a reverse is a manoeuvre, not a getaway).
            input.brake = if r.v < -3.0 { 0.0 } else { 0.7 };
            input.steer = -steer;
            r.throttle = 0.0;
        } else {
            // Feedforward of the profile's own slope (`v · dv/ds`) where the profile sets the
            // pace: the brakes come on at full strength where the profile starts braking,
            // instead of once the car is already too fast for it.
            let ahead = r.s + v * (0.15 + p.early_brake_s + r.brake_err_s);
            let slope = (v_line(ahead + CORRIDOR_STEP_M) - v_line(ahead)) / CORRIDOR_STEP_M;
            let acc_ff = if limit == SpeedLimit::Profile {
                (v * slope).min(0.0)
            } else {
                0.0
            };
            let mut acc = (acc_ff + 2.5 * (v_target - v)).clamp(-12.0, 8.0);
            if decel_demand > 0.5 {
                acc = acc.min(-1.2 * decel_demand);
            }
            let hold = matches!(r.tactic, Tactic::Park | Tactic::Yield) && v < 0.6;
            let (mut th, br) = r.ctrl.pedals(v, acc, dt, hold);
            // The controller releases the brake pedal near a standstill (a held brake there
            // selects reverse): a car told to stop holds on the handbrake instead of idling
            // forward into whatever it stopped for.
            let holding =
                v < 1.5 && (v_target < 0.3 || matches!(r.tactic, Tactic::Park | Tactic::Yield));
            if holding {
                input.handbrake = 1.0;
                th = 0.0;
            }
            if r.unstable {
                th = th.min(0.2);
            }
            // Creeping (round a standing car, up to a queue): feather the throttle so the
            // crawl does not overshoot into whatever is a metre away — but enough to pull
            // away from rest on a steep hill (Hollins hairpins).
            if v_target < 3.0 && limit != SpeedLimit::Profile {
                th = th.min(if v.abs() < 0.5 { 0.45 } else { 0.25 });
            }
            th = th.min(r.throttle + p.throttle_rate_per_s * dt);
            r.throttle = th;
            input.throttle = th;
            input.brake = br;
            // Wheels straight while standing on the handbrake: handbrake + steer is the
            // keyboard / gamepad pivot aid (wp_vehicle::pivot), which turns a car the AI
            // drives with a player's assists (autopilot) on the spot — about 60° in 3 s.
            input.steer = if holding && v.abs() < 0.3 { 0.0 } else { steer };
        }
        r.input = input;
        r.debug = Debug {
            v_target,
            limit,
            n_cmd,
            s: r.s,
            n: r.n,
            brake_dist,
            pass_checks: r.pass_checks,
        };
    }
}

/// Extra lateral room beside a car being passed beyond the normal clearance, m: the
/// driver's overtake margin for racers (contact is a race incident), a little for anyone
/// else (civilians, physical traffic, police: passed in the next lane like any driver
/// does; their clearance is already in the margin), least for a parked sim vehicle
/// (squeezed past at walking pace).
fn pass_margin(o: &Obstacle, p: &DriverParams) -> f32 {
    if parked_non_racer(o) {
        0.1
    } else if !matches!(o.who, Who::Racer(_)) {
        0.3
    } else {
        p.overtake_margin_m
    }
}

/// How far ahead a slower car is considered for a pass, m: 30 m, or 3.5 s at the closing
/// speed when that is more.
fn pass_horizon(v: f32, v_other: f32) -> f32 {
    30.0f32.max(3.5 * (v - v_other))
}

fn who_id(w: Who) -> u32 {
    match w {
        Who::Racer(k) => 100 + k as u32,
        Who::Vehicle(e) => e.0,
        Who::Civilian(a) => 1_000_000 + a,
    }
}

fn vehicle_obstacle(
    c: &Corridor,
    who: Who,
    cp: crate::corridor::CorridorPos,
    v: &VehicleObs,
) -> Option<Obstacle> {
    let (l, r) = c.pass_bounds(cp.s);
    if cp.n < -l - 6.0 || cp.n > r + 6.0 {
        return None;
    }
    let t = c.samples[cp.index].tangent;
    let vel = [v.state.lin_vel[0], v.state.lin_vel[1]];
    let fwd = forward_of(v.state.rot);
    let v_long = geom::dot(vel, fwd);
    let v_lat = geom::dot(vel, geom::right_of(fwd));
    // Size along the corridor depends on the car's heading relative to it.
    let ca = geom::dot(fwd, t).abs();
    let sa = (1.0 - ca * ca).max(0.0).sqrt();
    let (w, len) = (v.size_m[0], v.size_m[1]);
    Some(Obstacle {
        who,
        s: cp.s,
        n: cp.n,
        v_s: geom::dot(vel, t),
        v_n: geom::dot(vel, geom::right_of(t)),
        half_len: 0.5 * (len * ca + w * sa),
        half_w: 0.5 * (w * ca + len * sa),
        unstable: v_long > 8.0 && v_lat.abs() > 0.2 * v_long,
        passing: false,
        parked: false,
        facing: geom::dot(fwd, t),
    })
}

fn civilians_on(
    c: &Corridor,
    racers: &[Racer],
    world: &World,
    range: f32,
    private: bool,
) -> Vec<Obstacle> {
    let mut out = Vec::new();
    for a in world.agents() {
        let p = geom::xy(a.pos);
        let mut near: Option<(f32, usize)> = None;
        for r in racers {
            if r.progress.is_nan() && !private {
                continue;
            }
            let d = geom::dist(p, r.pos);
            if d < range && near.is_none_or(|b| d < b.0) {
                near = Some((d, r.hint));
            }
        }
        let Some((_, hint)) = near else { continue };
        let cp = c.project(p, Some(hint), (range / CORRIDOR_STEP_M) as usize + 5);
        // The whole road, opposing lanes included: a racer passing over there must see
        // what stands in them.
        let (l, rr) = c.pass_bounds(cp.s);
        if cp.n < -l - 4.0 || cp.n > rr + 4.0 {
            continue;
        }
        let t = c.samples[cp.index].tangent;
        let f = geom::dir_of_yaw(a.yaw);
        let vel = geom::scale(f, a.speed_mps);
        let ca = geom::dot(f, t).abs();
        let sa = (1.0 - ca * ca).max(0.0).sqrt();
        out.push(Obstacle {
            who: Who::Civilian(a.id),
            s: cp.s,
            n: cp.n,
            v_s: geom::dot(vel, t),
            v_n: geom::dot(vel, geom::right_of(t)),
            half_len: 0.5 * (CAR_LENGTH_M * ca + CAR_WIDTH_M * sa),
            half_w: 0.5 * (CAR_WIDTH_M * ca + CAR_LENGTH_M * sa),
            unstable: false,
            passing: false,
            // Pulled over for a siren (or broken down) and stopped: it stays put while a
            // police car is near, possibly for minutes, so treat it like a parked car.
            parked: a.speed_mps.abs() < 0.5 && (a.yielding || a.hazard_lights),
            facing: geom::dot(f, t),
        });
    }
    out
}

/// Why [`plan_pass`] said no (counted per racer in [`Debug::pass_checks`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PassReject {
    NoRun = 1,
    TooLate = 2,
    NoRoom = 3,
    SlowLine = 4,
    LaneAhead = 5,
    LaneBehind = 6,
    TargetBusy = 7,
}

/// Context for [`plan_pass`].
struct PassCtx<'a> {
    corridor: &'a Corridor,
    line: &'a RacingLine,
    profile: &'a SpeedProfile,
    brake_dist: f32,
    v: f32,
    margin: f32,
    /// The sim applies slipstream.
    draft: bool,
}

/// Lateral limits `(lo, hi)` for passing `o`: a little nearer the road edges when it is a
/// parked sim vehicle (squeezed past slowly).
fn squeeze_limits((lo, hi): (f32, f32), o: &Obstacle) -> (f32, f32) {
    if parked_non_racer(o) {
        (lo - PARKED_EDGE_SLACK_M, hi + PARKED_EDGE_SLACK_M)
    } else {
        (lo, hi)
    }
}

/// Choose a passing lateral position beside `target`, or None if no side is clear far
/// enough or the pass cannot be finished before the braking zone (aggressive drivers accept
/// finishing it side by side into the zone for `side_by_side_s`).
fn plan_pass(
    r: &Racer,
    target: &Rel,
    rel: &[Rel],
    ctx: &PassCtx,
    limits: &dyn Fn(f32) -> (f32, f32),
) -> Result<f32, PassReject> {
    let p = &r.params;
    let o = target.o;
    let v = ctx.v;
    let hw = 0.5 * r.cap.width_m;
    let hl = 0.5 * r.cap.length_m;
    // Room needed: until clear of it at the current closing speed (at most 4 s), side by
    // side into the next corner if the road is wide enough.
    let clear_len = target.gap.max(0.0) + 2.0 * hl + 2.0 * o.half_len + 3.0;
    let potential = ctx.corridor.interp(&ctx.profile.v, r.s + v) - o.v_s;
    let t_clear = (clear_len / (v - o.v_s).max(0.6 * potential).max(1.0)).min(4.0);
    // A pulled-over car only needs room until I am past it.
    let pass_dist = if o.parked {
        clear_len + 5.0
    } else {
        (v * t_clear).max(40.0)
    };
    // No three-wide: never pass a car that is passing, nor start a pass while a car is
    // alongside me.
    // (A car right behind me in line is not alongside.)
    let engaged = rel.iter().any(|q| {
        q.o.who != o.who
            && !matches!(q.o.who, Who::Civilian(_))
            && q.ds <= 0.0
            && q.gap < 3.0
            && (q.o.n - r.n).abs() > hw + q.o.half_w - 0.5
    });
    if o.passing || engaged {
        return Err(PassReject::TargetBusy);
    }
    // A pass needs a run: closing speed now, or pace I'm being denied (my own speed profile
    // a second ahead vs its speed) with a straight ahead, or sitting right on its bumper.
    let potential = ctx.corridor.interp(&ctx.profile.v, r.s + v) - o.v_s;
    let straight = ctx.brake_dist > v * 2.0;
    let closing = (v - o.v_s).max(if straight { 0.6 * potential } else { 0.0 });
    let tow = (target.gap < 4.0 || (ctx.draft && target.gap < 8.0)) && straight;
    if closing < 0.5 && !tow {
        return Err(PassReject::NoRun);
    }
    // No lunges from far back at the last moment.
    if ctx.brake_dist < v * 0.6 && target.gap > 2.0 {
        return Err(PassReject::TooLate);
    }
    let need = hw + o.half_w + ctx.margin + pass_margin(&o, p);
    // Prefer the inside of the next corner (sign of the line curvature ahead, + = right).
    let mut ahead_k = 0.0f32;
    let mut x = 0.0;
    while x < 150.0 {
        let i = ctx.corridor.wrap(((r.s + x) / CORRIDOR_STEP_M) as isize);
        ahead_k += ctx.line.curvature[i];
        x += CORRIDOR_STEP_M;
    }
    let sides: [f32; 2] = if ahead_k >= 0.0 {
        [1.0, -1.0]
    } else {
        [-1.0, 1.0]
    };
    let mut why = PassReject::NoRoom;
    'side: for side in sides {
        let n_pass = o.n + side * need;
        // Room for the whole pass.
        let mut x = 0.0;
        while x <= pass_dist.max(10.0) {
            let (lo, hi) = squeeze_limits(limits(r.s + x), &o);
            if n_pass < lo || n_pass > hi {
                why = PassReject::NoRoom;
                continue 'side;
            }
            x += CORRIDOR_STEP_M * 2.0;
        }
        // The passing line must not be slower than the car being passed (tight inside lines
        // through a corner are).
        let a_brk = ctx.profile.a_brake.max(3.0) * 0.85;
        let mut x = 0.0;
        while x <= pass_dist {
            let kc = ctx.corridor.centre_curvature(r.s + x);
            let k = (kc / (1.0 - kc * n_pass).max(0.2)).abs();
            if k > 1e-4 {
                let v_off = (ctx.profile.a_lat / k).sqrt();
                if (v_off * v_off + 2.0 * a_brk * x).sqrt() < o.v_s.min(v) - 0.5 {
                    why = PassReject::SlowLine;
                    continue 'side;
                }
            }
            x += 2.0 * CORRIDOR_STEP_M;
        }
        // Nobody else in that lane (ahead within the pass, or coming up behind).
        for q in rel {
            if q.o.who == o.who {
                continue;
            }
            let in_lane = (q.o.n - n_pass).abs() < hw + q.o.half_w + ctx.margin + 0.3;
            if !in_lane {
                continue;
            }
            if q.ds > 0.0 && q.ds < target.ds + 2.0 * hl + 6.0 {
                why = PassReject::LaneAhead;
                continue 'side;
            }
            // Oncoming traffic in that lane: it covers the distance too. A car standing in
            // it anywhere along the pass closes it as well (I would end up nose to nose).
            let oncoming = q.o.v_s < -1.0 && q.ds < pass_dist + (-q.o.v_s) * t_clear + 30.0;
            let standing = q.o.v_s.abs() < 1.0 && q.ds < pass_dist + 10.0;
            if q.ds > 0.0 && (oncoming || standing) {
                why = PassReject::LaneAhead;
                continue 'side;
            }
            if q.ds <= 0.0 && q.gap < 12.0 && q.o.v_s > v - 2.0 {
                why = PassReject::LaneBehind;
                continue 'side;
            }
        }
        return Ok(n_pass);
    }
    Err(why)
}

/// Plan a flight route to the POI that is farthest from the known police (from the racer
/// itself when no police position is known), at least 150 m away.
fn plan_flight(world: &World, r: &Racer, config: &RaceConfig, police_at: &[V2]) -> Option<Flight> {
    let pois = &world.network().pois;
    let here = r.pos;
    let threat: Vec<V2> = if police_at.is_empty() {
        vec![here]
    } else {
        police_at.to_vec()
    };
    let score = |p: V2| {
        threat
            .iter()
            .fold(f32::INFINITY, |m, t| m.min(geom::dist(p, *t)))
    };
    // Best destination first; fall back down the list until a legal route plans.
    let mut order: Vec<(f32, usize)> = pois
        .iter()
        .enumerate()
        .filter(|(_, poi)| geom::dist(geom::xy(poi.pos), here) > 150.0)
        .map(|(i, poi)| (score(geom::xy(poi.pos)), i))
        .collect();
    order.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let corridor = order.iter().find_map(|&(_, i)| {
        let route = world.plan_route([here[0], here[1], 0.0], Some(r.fwd), pois[i].pos)?;
        if route.polyline.len() < 2 {
            return None;
        }
        // A route with no length (destination behind on the same segment) plans nothing:
        // try the next destination.
        let c = Corridor::try_from_route(world.map(), &route, &config.corridor)?;
        (c.len() >= 25).then_some(c)
    })?;
    let line = RacingLine::constant(&corridor, 0.0);
    let profile = SpeedProfile::build(
        &corridor,
        &line,
        &r.cap_now,
        &r.params.usage,
        r.cap_now.top_speed_mps,
    );
    Some(Flight {
        corridor,
        line,
        profile,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corridor::{CorridorSample, OpposingLanes};
    use crate::pursuit::NoPolice;
    use wp_contracts::TimeOfDay;

    fn standing(who: Who) -> Obstacle {
        Obstacle {
            who,
            s: 100.0,
            n: -2.0,
            v_s: 0.0,
            v_n: 0.0,
            half_len: 2.3,
            half_w: 1.0,
            unstable: false,
            passing: false,
            parked: true,
            facing: -1.0,
        }
    }

    #[test]
    fn parked_police_car_is_squeezed_past_slowly_civilians_and_racers_are_not() {
        let police = standing(Who::Vehicle(EntityId(11)));
        let civ = standing(Who::Civilian(7));
        let racer = standing(Who::Racer(2));
        assert!(parked_non_racer(&police));
        assert!(!parked_non_racer(&civ));
        assert!(!parked_non_racer(&racer));
        assert_eq!(squeeze_limits((-1.0, 1.0), &police), (-1.15, 1.15));
        assert_eq!(squeeze_limits((-1.0, 1.0), &civ), (-1.0, 1.0));
        assert_eq!(squeeze_limits((-1.0, 1.0), &racer), (-1.0, 1.0));
        // A moving police car keeps the normal limits.
        let moving = Obstacle {
            parked: false,
            ..police
        };
        assert_eq!(squeeze_limits((-1.0, 1.0), &moving), (-1.0, 1.0));
        // Clearance: tight at a crawl, the normal one from 15 m/s.
        assert!((parked_gap(0.0, 0.6) - PARKED_GAP_M).abs() < 1e-6);
        assert!((parked_gap(7.5, 0.6) - 0.4).abs() < 1e-6);
        assert!((parked_gap(30.0, 0.6) - 0.6).abs() < 1e-6);
    }

    /// A straight 400 m corridor along +Y, 5 m each side.
    fn straight() -> Corridor {
        let n = 200;
        Corridor {
            samples: (0..n)
                .map(|i| CorridorSample {
                    pos: [0.0, i as f32 * CORRIDOR_STEP_M, 0.0],
                    tangent: [0.0, 1.0],
                    left_m: 5.0,
                    left_pass_m: 5.0,
                    right_m: 5.0,
                    junction: false,
                })
                .collect(),
            closed: false,
            length_m: (n - 1) as f32 * CORRIDOR_STEP_M,
            opposing: OpposingLanes::Never,
            junction_lanes: Vec::new(),
        }
    }

    fn cap() -> CarCapability {
        CarCapability {
            max_lat_g: 0.9,
            brake_g: 1.0,
            accel_mps2: 4.0,
            top_speed_mps: 50.0,
            wheelbase_m: 2.7,
            max_steer_rad: 0.6,
            length_m: 4.6,
            width_m: 1.8,
        }
    }

    fn obs(entity: u32, y: f32, speed: f32, effects: HandlingEffects) -> VehicleObs {
        VehicleObs {
            entity: EntityId(entity),
            state: VehicleState {
                pos: [0.0, y, 0.0],
                rot: [0.0, 0.0, 0.0, 1.0],
                lin_vel: [0.0, speed, 0.0],
                speed_mps: speed,
                health: 1.0,
                ..Default::default()
            },
            effects,
            size_m: [1.8, 4.6],
        }
    }

    fn field(n: usize) -> Field {
        let specs: Vec<RacerSpec> = (0..n)
            .map(|i| RacerSpec {
                entity: EntityId(i as u32 + 1),
                profile: DriverProfile::default(),
                capability: cap(),
            })
            .collect();
        Field::new(straight(), &specs, RaceConfig::default())
    }

    /// One racer of `risk` at y = 70 m doing 15 m/s on a straight with a junction box at
    /// y = 100..120 m; returns its debug after one decision with `signal` and `civs`.
    fn junction_decision(risk: f32, signal: Option<SignalAhead>, civs: &[Civ]) -> Debug {
        let mut corridor = straight();
        for smp in &mut corridor.samples[50..60] {
            smp.junction = true;
        }
        let spec = RacerSpec {
            entity: EntityId(1),
            profile: DriverProfile {
                risk,
                ..DriverProfile::default()
            },
            capability: cap(),
        };
        let mut f = Field::new(corridor, &[spec], RaceConfig::default());
        let world = World::flat(TimeOfDay::from_hours(12.0), 1.0);
        // Past the start-discipline zone, so only the junction rules apply.
        f.racers[0].laps_done = 1;
        f.tick(
            &world,
            &[obs(1, 70.0, 15.0, HandlingEffects::NEUTRAL)],
            &[],
            &NoPolice,
        );
        f.decide(0, &[], civs, signal, 4.0 * PHYSICS_DT);
        f.debug_of(0)
    }

    #[test]
    fn careful_driver_stops_for_a_red_and_a_risk_taker_does_not() {
        let red = Some(SignalAhead {
            d_entry: 30.0,
            state: SignalState::Red,
            remaining_s: 20.0,
        });
        let careful = junction_decision(0.1, red, &[]);
        assert_eq!(careful.limit, SpeedLimit::Signal);
        // Capped to the stopping envelope for the line (4 m before the box).
        assert!(careful.v_target * careful.v_target < 2.0 * 12.0 * 26.0);
        let bold = junction_decision(0.6, red, &[]);
        assert_ne!(bold.limit, SpeedLimit::Signal);
        // A green that turns red before arrival counts as a red for the careful driver.
        let stale = junction_decision(
            0.1,
            Some(SignalAhead {
                d_entry: 30.0,
                state: SignalState::Green,
                remaining_s: 1.0,
            }),
            &[],
        );
        assert_eq!(stale.limit, SpeedLimit::Signal);
    }

    #[test]
    fn lasting_green_ignores_cross_traffic_held_at_its_red() {
        // A civilian stopped in the junction box, across my path.
        let waiting = [civ(7, [1.0, 110.0], [0.0, 0.0], 0)];
        let unsignalised = junction_decision(0.4, None, &waiting);
        assert_eq!(unsignalised.limit, SpeedLimit::Obstacle(1_000_007));
        // One waiting at its stop line beside my path is no obstacle.
        let at_line = [civ(7, [8.0, 110.0], [0.0, 0.0], 0)];
        let beside = junction_decision(0.4, None, &at_line);
        assert_ne!(beside.limit, SpeedLimit::Obstacle(1_000_007));
        let green = Some(SignalAhead {
            d_entry: 30.0,
            state: SignalState::Green,
            remaining_s: 30.0,
        });
        let protected = junction_decision(0.4, green, &waiting);
        assert_ne!(protected.limit, SpeedLimit::Obstacle(1_000_007));
        // A civilian still moving inside the box counts even on green.
        let moving = [civ(7, [4.0, 110.0], [-6.0, 0.0], 0)];
        let busy = junction_decision(0.4, green, &moving);
        assert_eq!(busy.limit, SpeedLimit::Obstacle(1_000_007));
    }

    fn civ(id: u32, pos: V2, vel: V2, turn_signal: i8) -> Civ {
        Civ {
            id,
            pos,
            vel,
            turn_signal,
        }
    }

    #[test]
    fn junction_gap_acceptance_uses_time_windows() {
        // Me: y = 70 at 15 m/s, box y = 100..120 (about 2–4 s away).
        // Cross traffic arriving together with me: wait.
        let together = [civ(7, [-40.0, 110.0], [14.0, 0.0], 0)];
        let d = junction_decision(0.4, None, &together);
        assert_eq!(d.limit, SpeedLimit::Obstacle(1_000_007));
        // Cross traffic that will be long through before I arrive: go.
        let early = [civ(7, [4.0, 110.0], [14.0, 0.0], 0)];
        let d = junction_decision(0.4, None, &early);
        assert_ne!(d.limit, SpeedLimit::Obstacle(1_000_007));
        // Cross traffic far off, arriving well after I'm through: go.
        let late = [civ(7, [-60.0, 110.0], [6.0, 0.0], 0)];
        let d = junction_decision(0.4, None, &late);
        assert_ne!(d.limit, SpeedLimit::Obstacle(1_000_007));
        // Oncoming, going straight on in the opposing lane: never in my way.
        let oncoming = [civ(7, [-3.5, 135.0], [0.0, -12.0], 0)];
        let d = junction_decision(0.4, None, &oncoming);
        assert_ne!(d.limit, SpeedLimit::Obstacle(1_000_007));
        // ...unless it signals a turn (it may cut across me).
        let turning = [civ(7, [-3.5, 135.0], [0.0, -12.0], -1)];
        let d = junction_decision(0.4, None, &turning);
        assert_eq!(d.limit, SpeedLimit::Obstacle(1_000_007));
    }

    #[test]
    fn grip_damage_rebuilds_a_slower_profile() {
        let mut f = field(1);
        let world = World::flat(TimeOfDay::from_hours(12.0), 1.0);
        f.tick(
            &world,
            &[obs(1, 20.0, 10.0, HandlingEffects::NEUTRAL)],
            &[],
            &NoPolice,
        );
        let before = f.speed_profile_of(0).a_lat;
        let hurt = HandlingEffects {
            grip_scale: [0.6, 1.0, 1.0, 1.0],
            overheating: 0.5,
            ..HandlingEffects::NEUTRAL
        };
        for _ in 0..4 {
            f.tick(&world, &[obs(1, 21.0, 10.0, hurt)], &[], &NoPolice);
        }
        let after = f.speed_profile_of(0);
        assert!(after.a_lat < before * 0.7, "{before} → {}", after.a_lat);
        // Overheating caps the speed below the car's top speed.
        assert!(after
            .v
            .iter()
            .all(|v| *v <= 50.0 * (1.0 - 0.35 * 0.5) + 1e-3));
    }

    #[test]
    fn dead_engine_retires_and_parks() {
        let mut f = field(1);
        let world = World::flat(TimeOfDay::from_hours(12.0), 1.0);
        let dead = HandlingEffects {
            engine_dead: true,
            ..HandlingEffects::NEUTRAL
        };
        f.tick(&world, &[obs(1, 20.0, 5.0, dead)], &[], &NoPolice);
        assert_eq!(f.standings()[0].status, RacerStatus::Retired);
        assert_eq!(f.tactic_of(0), Tactic::Park);
        assert!(f
            .drain_events()
            .iter()
            .any(|e| matches!(e, OpponentEvent::Retired { .. })));
    }

    #[test]
    fn follower_brakes_for_a_slower_car_ahead_and_never_steers_into_it() {
        let mut f = field(2);
        let world = World::flat(TimeOfDay::from_hours(12.0), 1.0);
        // Racer 2 is 12 m behind racer 1 and 15 m/s faster, both on the centreline.
        let cars = [
            obs(1, 160.0, 10.0, HandlingEffects::NEUTRAL),
            obs(2, 148.0, 25.0, HandlingEffects::NEUTRAL),
        ];
        let mut braked = false;
        for _ in 0..8 {
            let inputs = f.tick(&world, &cars, &[], &NoPolice).to_vec();
            braked |= inputs[1].1.brake > 0.5;
        }
        assert!(braked, "follower did not brake: {:?}", f.debug_of(1));
        assert!(matches!(f.debug_of(1).limit, SpeedLimit::Obstacle(100)));
        // Side by side, overlapping: neither steers toward the other.
        let mut f = field(2);
        let side = [
            obs(1, 100.0, 20.0, HandlingEffects::NEUTRAL),
            VehicleObs {
                state: VehicleState {
                    pos: [2.6, 101.0, 0.0],
                    ..side_state()
                },
                ..obs(2, 101.0, 20.0, HandlingEffects::NEUTRAL)
            },
        ];
        for _ in 0..40 {
            f.tick(&world, &side, &[], &NoPolice);
            // Neither moves toward the other (they start 2.6 m apart, need 1.8 + 0.6).
            assert!(f.debug_of(0).n_cmd <= 1e-3, "{:?}", f.debug_of(0));
            assert!(f.debug_of(1).n_cmd >= 2.6 - 1e-3, "{:?}", f.debug_of(1));
        }
    }

    fn side_state() -> VehicleState {
        VehicleState {
            rot: [0.0, 0.0, 0.0, 1.0],
            lin_vel: [0.0, 20.0, 0.0],
            speed_mps: 20.0,
            health: 1.0,
            ..Default::default()
        }
    }

    #[test]
    fn no_rubber_banding_pace_ignores_position() {
        // The same driver alone or last in a field gets the same speed profile.
        let a = field(1);
        let b = field(4);
        assert_eq!(a.speed_profile_of(0).v, b.speed_profile_of(3).v);
    }
}
