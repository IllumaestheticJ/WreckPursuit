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
    /// Racers may give up on the police: abandon the race and run (at their
    /// `flee_wanted_level`), or pull over when a unit is right on them. Off by default — the
    /// owner's rule (2026-10-21): a racer in a race never gives up; it keeps racing and evading
    /// until the police actually capture it or it is wrecked / DNF'd. Racers already out of
    /// the race (finished, disqualified) still pull over either way.
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
            allow_abandon: false,
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
    /// A racer out of the race (fled, busted, retired) driving or standing on the road: not
    /// racing anyone, so anyone may go round it when it is slower than they are.
    out_of_race: bool,
    /// A racer stuck nose-in that cannot back out because something stands right behind it.
    wants_room: bool,
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
    /// Speed profile on the whole-road line (`Field::wide_lines`), if there is one.
    wide_speed: Option<SpeedProfile>,
    /// Following the whole-road line right now.
    wide_on: bool,
    /// Lane picked by the gap planner (lateral offset of its centre), and how long it has
    /// been held, s.
    weave_n: Option<f32>,
    weave_s: f32,
    ctrl: DriveController,
    rng: Rng,
    status: RacerStatus,
    tactic: Tactic,
    // Kinematics (decision rate).
    pos: V2,
    fwd: V2,
    s: f32,
    /// `s` without the clamp at the ends of an open course (negative on a grid behind the
    /// start line): where cars are relative to each other.
    s_rel: f32,
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
    /// Lane-splitting: the car abreast of `pass_target` I am going between, if any.
    split_with: Option<Who>,
    tactic_ticks: u64,
    blocked_s: f32,
    /// Time an overtake has been held below pace by a car other than its target, s.
    pass_blocked_s: f32,
    stuck_s: f32,
    /// Boxed in nose-to-tail against a car standing for good: the car, the lateral position
    /// beside it I am working towards (reverse with the nose swinging out, creep forward,
    /// repeat), and how long I have been at it, s.
    escape: Option<(Who, f32, f32)>,
    /// Stuck and wanting to back out, but something stands right behind me (the car there
    /// backs off for me).
    wants_room: bool,
    stable_s: f32,
    defended: bool,
    in_brake_zone: bool,
    brake_err_s: f32,
    line_noise: f32,
    /// Stuck against something I cannot see (a wall, a shoved K-rail section, debris: no car
    /// explains it): where (race progress, m), how many times in a row at that spot, and
    /// which side of the road I was on the first time (true: left of the middle).
    blind_stuck: Option<(f32, u32, bool)>,
    /// Detour round an unseen obstacle after [`BLIND_DETOUR_AFTER`] blind stucks at one spot:
    /// the lateral position to drive at and the race progress where it ends, m.
    detour: Option<(f32, f32)>,
    effects: HandlingEffects,
    v_cap: f32,
    throttle: f32,
    input: VehicleInput,
    flight: Option<Box<Flight>>,
    passes: u32,
    /// Of `passes`, the ones made between two cars (lane-splitting).
    splits: u32,
    /// Lane-splits given up (a car drifted toward the gap, or it closed).
    split_aborts: u32,
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
    /// Out on the opposing lanes (my body left of my own lanes' edge).
    pub opposing: bool,
    /// Standing nose to nose with a standing car that faces me (or in the opposing lanes with
    /// a standing car right ahead): the stand-off a racer has to back out of.
    #[serde(default)]
    pub head_on: bool,
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
    /// Per entry of `lines`: the same car width's line over the whole road (my lanes and the
    /// opposing ones), where the corridor has opposing lanes to use. Followed while the road
    /// ahead is clear ([`wide_line_clear`]).
    wide_lines: Vec<Option<RacingLine>>,
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
/// A car out of the race doesn't stop where the road bends tighter than this (it rolls on to
/// a straighter bit first): nobody can get past a car stopped in a hairpin, m.
const PARK_MIN_RADIUS_M: f32 = 35.0;

/// Minimum time between flight re-plans, s.
const FLIGHT_REPLAN_S: f32 = 6.0;
/// Line optimiser sweeps for a flight route (the race line uses `RaceConfig::line`'s,
/// 100 / 150): about a fifth of the cost per sample.
const FLIGHT_LINE_SMOOTH_SWEEPS: u32 = 30;
/// See [`FLIGHT_LINE_SMOOTH_SWEEPS`].
const FLIGHT_LINE_CURVATURE_SWEEPS: u32 = 30;
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
/// Lateral clearance to a parked car's body that lets a creeping racer roll on alongside it
/// inside the stop gap, m.
const CREEP_CLEAR_M: f32 = 0.15;

/// Gap planner: city speeds only. At circuit speeds (Loop 9, 50-67 m/s) a weave off the line
/// cost time and, on seeds 3, a car (into the next bend off its line), m/s.
const WEAVE_MAX_MPS: f32 = 35.0;
/// Gap planner: how far ahead lanes are compared, m.
const WEAVE_RANGE_M: f32 = 100.0;
/// Gap planner: time over which a car's speed counts towards the room in its lane, s.
const WEAVE_LOOK_S: f32 = 3.0;
/// Gap planner: extra room another lane must offer before I move over, m.
const WEAVE_GAIN_M: f32 = 20.0;
/// Gap planner: works only where my own lanes leave this much room to move across (more than
/// one lane of mine), m.
const WEAVE_MIN_SPAN_M: f32 = 3.0;

/// A civilian whose speed across the corridor is below this fraction of its speed along it
/// (about 1.4°) is read as keeping its lane (no drift look-ahead): the corridor's sampled
/// direction differs from its lane's by about that much. Loop 9 at 23:00: a civilian in the
/// next lane reading ±0.37 m/s across the corridor at 27 m/s held racers at its speed for
/// 50 s, a metre of phantom drift over 3 s. Scaled with speed, so a slow city car pulling
/// across still counts.
const CIV_DRIFT_DEADBAND: f32 = 0.025;

/// How far along the corridor a relocated racer is looked for, either way, m.
const RELOCATE_WINDOW_M: f32 = 1500.0;

/// A finished racer whose corridor ends at the finish line rolls on this far past it before
/// pulling over, m, at most this fast, m/s.
const FINISH_CLEAR_M: f32 = 15.0;
const FINISH_CLEAR_MPS: f32 = 5.0;
/// A corridor that runs on less than this past the finish has no run-off leg (it ends at the
/// line, give or take its sampling), m.
const NO_RUN_OFF_M: f32 = 5.0;

/// Lateral step between the pass positions tried beside a car (about a lane), m.
const PASS_LANE_STEP_M: f32 = 3.5;

/// Road width (my lanes plus any usable for passing) from which a car that is itself passing
/// may be passed (three wide), m.
const THREE_WIDE_MIN_ROAD_M: f32 = 10.0;

/// An overtake held below pace by a car other than its target this long ends (re-plan), s.
const OVERTAKE_BLOCKED_S: f32 = 1.0;

/// A sim vehicle this close to a physical traffic agent is that agent's body, m.
const TRAFFIC_BODY_MATCH_M: f32 = 1.0;

/// Gap a reversing racer keeps to a car standing behind it for good (parked or out of the
/// race), m.
const REVERSE_STANDING_GAP_M: f32 = 1.0;
/// Escape from a box-in: lateral clearance to the standing car aimed for beside it, m.
const ESCAPE_CLEAR_M: f32 = 0.4;
/// Escape from a box-in: held at a standstill behind a car standing for good this long, s.
const ESCAPE_AFTER_S: f32 = 3.0;
/// Escape from a box-in: given up (back to normal driving) after this long, s.
const ESCAPE_MAX_S: f32 = 30.0;

/// Lane-splitting: room needed between two cars abreast beyond my own width, m (0.4 m a
/// side).
const SPLIT_ROOM_M: f32 = 0.8;
/// Lane-splitting: back out (while still behind) once the gap shrinks below my width plus
/// this, m.
const SPLIT_ABORT_ROOM_M: f32 = 0.5;
/// Lane-splitting: lateral clearance kept to the two cars while between them, m.
/// Two stucks with no car to explain them within this much race progress count as the same
/// spot (a racer wedged on a K-rail pile or a wall reverses and drives into it again), m.
const BLIND_SAME_SPOT_M: f32 = 8.0;
/// After this many blind stucks at one spot, the next attempt detours to the other side of
/// the road (races audit P2: touge cars reversed 1.6 s and drove back into the same shoved
/// K-rail sections for the rest of the battle).
const BLIND_DETOUR_AFTER: u32 = 2;
/// A detour holds its lateral position this far past the stuck spot, m.
const BLIND_DETOUR_M: f32 = 14.0;
const SPLIT_MARGIN_M: f32 = 0.25;
/// Lane-splitting: only drivers at least this skilled go between two cars.
const SPLIT_MIN_SKILL: f32 = 0.75;
/// Lane-splitting: largest closing speed on the slower of the two at full traffic risk,
/// m/s (scaled down with the driver's traffic risk, floor [`SPLIT_MIN_RISK`]).
const SPLIT_MAX_CLOSING_MPS: f32 = 5.0;
/// Lane-splitting: the traffic risk the closing limit is scaled by never counts as less
/// than this (a skilled, careful driver still closes at up to 3 m/s; lead, 2026-10-21).
const SPLIT_MIN_RISK: f32 = 0.6;
/// Lane-splitting: lateral speed of either car toward the gap that ends the split, m/s.
const SPLIT_DRIFT_MPS: f32 = 0.3;

/// The whole-road line puts me on the opposing side this close to a junction only when its arms
/// are empty ([`junction_arms_empty`]), m.
const WIDE_JUNCTION_CLEAR_M: f32 = 30.0;
/// How far up a junction's arms civilians coming to it are looked for, m.
const JUNCTION_ARM_LOOK_M: f32 = 60.0;
/// Margin over my own time through a junction before a civilian heading to it gets there, s.
const JUNCTION_GAP_S: f32 = 1.5;

/// Extra clearance of the whole-road line from the far edge of the opposing lanes, m.
const WIDE_EDGE_EXTRA_M: f32 = 0.6;

impl Field {
    /// Build a field on `corridor`. Lines are optimised once per distinct car width.
    pub fn new(corridor: Corridor, specs: &[RacerSpec], config: RaceConfig) -> Self {
        let mut master = Rng::new(config.seed ^ 0x0BB0_57E5_u64);
        let mut lines: Vec<(u32, RacingLine)> = Vec::new();
        let mut wide_lines: Vec<Option<RacingLine>> = Vec::new();
        // The road including the opposing lanes, where the corridor may use them.
        let wide_corridor = (corridor.opposing == OpposingLanes::ForOvertaking
            && corridor
                .samples
                .iter()
                .any(|c| c.left_pass_m > c.left_m + 1.0))
        .then(|| {
            let mut w = corridor.clone();
            for c in &mut w.samples {
                // A little more room from the far edge of the opposing lanes than from my own
                // kerb: it is a kerb or a wall with nothing to run wide on to (Hollins s95 seed
                // 7: a racer on that edge into a hairpin braked for the car ahead and slid into
                // the wall). My own side keeps the own-lane line's margin.
                if c.left_pass_m > c.left_m {
                    c.left_m = (c.left_pass_m - WIDE_EDGE_EXTRA_M).max(c.left_m);
                }
            }
            w
        });
        let mut racers = Vec::with_capacity(specs.len());
        for spec in specs {
            let key = (spec.capability.width_m * 100.0).round() as u32;
            let line_idx = match lines.iter().position(|(k, _)| *k == key) {
                Some(i) => i,
                None => {
                    let half = spec.capability.width_m * 0.5;
                    lines.push((key, RacingLine::optimise(&corridor, half, &config.line)));
                    wide_lines.push(
                        wide_corridor
                            .as_ref()
                            .map(|w| RacingLine::optimise(w, half, &config.line)),
                    );
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
            let wide_speed = wide_lines[line_idx].as_ref().map(|w| {
                SpeedProfile::build(&corridor, w, &spec.capability, &params.usage, f32::INFINITY)
            });
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
                wide_speed,
                wide_on: false,
                weave_n: None,
                weave_s: 0.0,
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
                s_rel: 0.0,
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
                split_with: None,
                tactic_ticks: 0,
                blocked_s: 0.0,
                pass_blocked_s: 0.0,
                stuck_s: 0.0,
                escape: None,
                wants_room: false,
                stable_s: 0.0,
                defended: false,
                in_brake_zone: false,
                brake_err_s: 0.0,
                line_noise: 0.0,
                effects: HandlingEffects::NEUTRAL,
                v_cap: f32::INFINITY,
                blind_stuck: None,
                detour: None,
                throttle: 0.0,
                input: VehicleInput::default(),
                flight: None,
                passes: 0,
                splits: 0,
                split_aborts: 0,
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
            wide_lines,
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

    /// Build a field on a `wp_world` route. Panics on a route with nothing to drive (see
    /// [`Field::try_from_route`]).
    pub fn from_route(
        world: &World,
        route: &wp_world::Route,
        specs: &[RacerSpec],
        config: RaceConfig,
    ) -> Self {
        Self::try_from_route(world, route, specs, config).expect("route with no length")
    }

    /// Build a field on a `wp_world` route, or `None` when the route has nothing to drive
    /// (no length, or a one-segment route whose end lies behind its start: row 154, where the
    /// panic froze the browser).
    pub fn try_from_route(
        world: &World,
        route: &wp_world::Route,
        specs: &[RacerSpec],
        config: RaceConfig,
    ) -> Option<Self> {
        let corridor = Corridor::try_from_route(world.map(), route, &config.corridor)?;
        Some(Self::new(corridor, specs, config))
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
                let priority = self.priority_ahead(k, world);
                self.decide(k, &obstacles, &civs, signal, priority, dt);
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

    /// The server moved racer `entity` on purpose (a reset back onto the course after being
    /// stranded off it): find it on the corridor again near `pos` (a reset can move it
    /// hundreds of metres back), count the move in its progress, and drive on from rest.
    pub fn relocate(&mut self, entity: EntityId, pos: V2) {
        let Some(k) = self.racer_index(entity) else {
            return;
        };
        if self.racers[k].progress.is_nan() {
            return;
        }
        let corridor = match &self.racers[k].flight {
            Some(f) => &f.corridor,
            None => &self.corridor,
        };
        let window = (RELOCATE_WINDOW_M / CORRIDOR_STEP_M) as usize;
        let cp = corridor.project(pos, Some(self.racers[k].hint), window);
        let ds = corridor.delta_s(self.racers[k].s, cp.s);
        let r = &mut self.racers[k];
        if r.flight.is_none() {
            r.progress += ds;
        }
        r.pos = pos;
        r.s = cp.s;
        r.n = cp.n;
        r.n_cmd = cp.n;
        r.hint = cp.index;
        r.tactic = Tactic::Follow;
        r.tactic_ticks = 0;
        r.stuck_s = 0.0;
        r.stable_s = 0.0;
        r.escape = None;
        r.wants_room = false;
        r.throttle = 0.0;
        r.ctrl.resync(0.0);
    }

    fn racer_index(&self, e: EntityId) -> Option<usize> {
        self.racers.iter().position(|r| r.entity == e)
    }
        r.blind_stuck = None;
        r.detour = None;

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
                r.s_rel = corridor.unclamped_s(pos, &cp);
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
                    // Cooling down after the flag: my own lanes only.
                    r.wide_speed = None;
                    r.wide_on = false;
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
                    if r.wide_speed.is_some() {
                        r.wide_speed = self.wide_lines[r.line_idx].as_ref().map(|w| {
                            SpeedProfile::build(
                                &self.corridor,
                                w,
                                &r.cap_now,
                                &r.params.usage,
                                v_cap,
                            )
                        });
                    }
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
            // Owner's rule (2026-10-21, `RaceConfig::allow_abandon` off): a racer in a race
            // never gives up — it neither abandons the race nor pulls over, at any wanted
            // level; it keeps racing and evading until the police actually capture it
            // (police-side capture rules) or it is wrecked / DNF'd. A racer already out of the
            // race (finished, disqualified) pulls over as before.
            RacerStatus::Racing if !self.config.allow_abandon => {
                r.yield_s = 0.0;
                if r.tactic == Tactic::Yield {
                    r.tactic = Tactic::Rejoin;
                    r.tactic_ticks = 0;
                }
            }
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
        let bodies = traffic_bodies(world);
        for v in vehicles {
            if self.racer_index(v.entity).is_none() && is_traffic_body(&bodies, v) {
                continue;
            }
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
                            s: r.s_rel,
                            n: r.n,
                            v_s: r.v_s,
                            v_n: r.v_n,
                            half_len: r.ext.0,
                            half_w: r.ext.1,
                            unstable,
                            passing: r.tactic == Tactic::Overtake,
                            parked: matches!(r.tactic, Tactic::Park | Tactic::Yield),
                            facing: 1.0,
                            out_of_race: false,
                            wants_room: r.wants_room,
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
            let cp = self.corridor.project_unclamped(pos, hint, 60);
            self.hints.insert(v.entity, cp.index);
            if let Some(mut o) = vehicle_obstacle(&self.corridor, who, cp, v) {
                o.parked = self.is_still(STILL_VEHICLE_KEY + v.entity.0 as u64);
                o.out_of_race = self
                    .racer_index(v.entity)
                    .is_some_and(|k| !self.racers[k].in_race());
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
        let bodies = traffic_bodies(world);
        for v in vehicles {
            if v.entity == me.entity
                || (self.racer_index(v.entity).is_none() && is_traffic_body(&bodies, v))
            {
                continue;
            }
            let pos = geom::xy(v.state.pos);
            if geom::dist(pos, me.pos) > me.ahead_m() + 20.0 {
                continue;
            }
            let cp = corridor.project_unclamped(pos, Some(me.hint), 80);
            if let Some(mut o) = vehicle_obstacle(corridor, Who::Vehicle(v.entity), cp, v) {
                o.parked = self.is_still(STILL_VEHICLE_KEY + v.entity.0 as u64);
                o.out_of_race = self
                    .racer_index(v.entity)
                    .is_some_and(|k| !self.racers[k].in_race());
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
    /// Only civilians on the racer's own level count: traffic on a bridge overhead (or the
    /// road under one) never enters the racer's junctions.
    fn civilians_near(&self, world: &World) -> Vec<Civ> {
        let mut out = Vec::new();
        let level = |r: &Racer| {
            let c = r.flight.as_ref().map_or(&self.corridor, |f| &f.corridor);
            c.samples[r.hint.min(c.samples.len() - 1)].pos[2]
        };
        for a in world.agents() {
            let p = geom::xy(a.pos);
            if self
                .racers
                .iter()
                .any(|r| geom::dist(p, r.pos) < 160.0 && (a.pos[2] - level(r)).abs() < 4.0)
            {
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

    /// Entry distance of the next junction on racer `k`'s track when it is a priority
    /// junction and the racer arrives on the major road (civilians stopped on the minor
    /// approaches are waiting for it).
    fn priority_ahead(&self, k: usize, world: &World) -> Option<f32> {
        let r = &self.racers[k];
        let corridor = match &r.flight {
            Some(f) => &f.corridor,
            None => &self.corridor,
        };
        let i0 = corridor.wrap((r.s / CORRIDOR_STEP_M) as isize);
        let steps = (SIGNAL_LOOKAHEAD_M / CORRIDOR_STEP_M) as usize;
        let (ahead, lane) = corridor.next_junction_lane(i0, steps)?;
        match world.map().lane_control(lane)? {
            (wp_world::network::IntersectionControl::Priority, ap, _) if ap.major => {
                Some(ahead as f32 * CORRIDOR_STEP_M)
            }
            _ => None,
        }
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
        priority: Option<f32>,
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
        let wide_speed = self.racers[k].wide_speed.take();
        let line_idx = self.racers[k].line_idx;
        let wide = match (&flight, &self.wide_lines[line_idx], &wide_speed) {
            (None, Some(l), Some(p)) => Some((l, p)),
            _ => None,
        };
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
            priority,
            draft,
            finish_m,
            k,
            corridor,
            line,
            profile,
            wide,
            flight.is_some(),
            obstacles,
            dt,
        );
        self.racers[k].speed = speed;
        self.racers[k].wide_speed = wide_speed;
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

    /// Of [`Self::passes_by`], the passes racer `k` made between two cars (lane-splitting).
    pub fn splits_by(&self, k: usize) -> usize {
        self.racers[k].splits as usize
    }

    /// Lane-splits racer `k` gave up and tucked back in from.
    pub fn split_aborts_by(&self, k: usize) -> usize {
        self.racers[k].split_aborts as usize
    }
}

/// Steps 3 + 4 for one racer on its current track.
#[allow(clippy::too_many_arguments)]
fn decide_racer(
    r: &mut Racer,
    civs: &[Civ],
    signal: Option<SignalAhead>,
    priority: Option<f32>,
    draft: bool,
    finish_m: Option<f32>,
    k: usize,
    corridor: &Corridor,
    line: &RacingLine,
    profile: &SpeedProfile,
    wide: Option<(&RacingLine, &SpeedProfile)>,
    fleeing: bool,
    obstacles: &[Obstacle],
    dt: f32,
) {
    {
        let p = r.params;
        let hw = 0.5 * r.cap.width_m;
        let hl = 0.5 * r.cap.length_m;
        let v = r.v.max(0.0);
        let mut head_on_now = false;
        // Street racing: the whole road when it is clear. Follow the line over my lanes and
        // the opposing ones (apexes, chicanes, empty junctions) while nothing is in its way on
        // the opposing side or in a junction it crosses, out to past the next braking zone;
        // back to my own lanes as soon as something is.
        let finished_or_dq = matches!(
            r.status,
            RacerStatus::Finished { .. } | RacerStatus::Disqualified
        );
        // Finished where the corridor ends at the finish line itself (no run-off leg: Hollins
        // finishes in the Knob Observatory Lot): roll on straight past the end into the space
        // beyond before pulling over. Stopped on the line, the winner held the three behind it
        // short of it until the finish window closed (s95 seed 3, 15-17 rejoins each).
        let no_run_off =
            !corridor.closed && finish_m.is_none_or(|f| f >= corridor.length_m - NO_RUN_OFF_M);
        let clearing_line = matches!(r.status, RacerStatus::Finished { .. })
            && no_run_off
            && r.s_rel < corridor.length_m + FINISH_CLEAR_M;
        r.wide_on = match wide {
            Some((wl, wp)) if !fleeing && !finished_or_dq && r.flight.is_none() => {
                let a_brk = wp.a_brake.max(3.0);
                let horizon = (v * v / (2.0 * a_brk) + 1.5 * v + 30.0).min(250.0);
                let may_start = matches!(r.tactic, Tactic::Follow);
                // Open road only: once a car or a light ahead sets my speed I am in traffic —
                // back into my lanes while there is room. (Lantern s95 seed 4: a racer stopped
                // in a junction queue on the opposing side facing a civilian, the whole field
                // gridlocked; Hollins s95 seed 7: a racer on the opposing edge right behind
                // another braked for it into a hairpin and slid off the road.)
                let in_traffic =
                    matches!(r.debug.limit, SpeedLimit::Obstacle(_) | SpeedLimit::Signal);
                (r.wide_on || may_start)
                    && !in_traffic
                    && wide_line_clear(corridor, wl, r, Who::Racer(k), obstacles, civs, horizon)
            }
            _ => false,
        };
        let wide_now = r.wide_on;
        let (line, profile) = match wide {
            Some((wl, wp)) if wide_now => (wl, wp),
            _ => (line, profile),
        };
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
        let limits = |s: f32| {
            if wide_now {
                to_limits(corridor.pass_bounds(s))
            } else {
                to_limits(corridor.bounds(s))
            }
        };
        // Overtaking may use the opposing lanes on open roads: risk-takers always, everyone
        // to get round a pulled-over racer just ahead.
        let parked_ahead = obstacles.iter().any(|o| {
            o.parked && o.who != Who::Racer(k) && {
                let ds = corridor.delta_s(r.s_rel, o.s);
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
        // A racer out of the race (fled, busted, retired, rolling on to a place to stop) is
        // gone round as soon as it holds me up at all (Hollins, police seed 6: the field crawled
        // behind a fled racer through the hairpins for a minute and got caught there).
        let crawler_ahead = obstacles.iter().any(|o| {
            let ds = corridor.delta_s(r.s_rel, o.s);
            ds > 0.0
                && ds < 40.0
                && !matches!(o.who, Who::Racer(_))
                && o.v_s > 0.5
                && if o.out_of_race {
                    r.blocked_s > 1.5 && o.v_s < 0.9 * corridor.interp(&profile.v, o.s)
                } else {
                    r.blocked_s > 4.0 && o.v_s < crawl_mps
                }
        });
        // A non-racer standing in my way (a civilian at a light, a stalled car) that has held me
        // up for a second: street racers go round it on the opposing lanes like round a parked
        // car, when the pass checks find them clear (main's Lantern downtown: the field sat 20 s
        // behind a civilian stopped at a light until it counted as parked).
        let standing_ahead = r.blocked_s > 1.0
            && obstacles.iter().any(|o| {
                let ds = corridor.delta_s(r.s_rel, o.s);
                ds > 0.0
                    && ds < 40.0
                    && !matches!(o.who, Who::Racer(_))
                    && o.v_s.abs() < 0.5
                    && (o.n - r.n).abs() < hw + o.half_w + 0.5
            });
        let may_use_opposing = corridor.opposing == OpposingLanes::ForOvertaking
            && !done_racing
            && (p.traffic_risk >= 0.5 || parked_ahead || crawler_ahead || standing_ahead);
        // The next junction (within 120 m) has nobody coming to it: a pass on the opposing side
        // may go into it (risk check, as for the whole-road line).
        // (The cars queued in my own lane ahead are the ones being passed, not traffic coming
        // to the junction: a civilian stopped at the line would otherwise block its own pass.)
        let junction_clear = {
            let (own_lo, own_hi) = corridor.bounds(r.s);
            let queued: Vec<u32> = obstacles
                .iter()
                .filter_map(|o| match o.who {
                    Who::Civilian(id)
                        if o.n > -own_lo - 0.5
                            && o.n < own_hi + 0.5
                            && corridor.delta_s(r.s_rel, o.s) > 0.0
                            && corridor.delta_s(r.s_rel, o.s) < 120.0
                            && o.v_s > -0.5 =>
                    {
                        Some(id)
                    }
                    _ => None,
                })
                .collect();
            let others: Vec<Civ> = civs
                .iter()
                .filter(|c| !queued.contains(&c.id))
                .copied()
                .collect();
            let i0 = (r.s / CORRIDOR_STEP_M).floor() as isize;
            let steps = (120.0 / CORRIDOR_STEP_M) as isize;
            (0..steps)
                .find(|&j| corridor.sample(i0 + j).junction)
                .is_none_or(|j| {
                    // From where I am (often standing behind the car I am passing),
                    // accelerating as I would: to the junction and through it.
                    let d = j as f32 * CORRIDOR_STEP_M + 20.0;
                    let a = (r.cap.accel_mps2 * p.usage.accel).max(1.0);
                    let t_clear = (-v + (v * v + 2.0 * a * d).sqrt()) / a + 0.5;
                    junction_arms_empty(geom::xy(corridor.sample(i0 + j).pos), &others, t_clear)
                })
        };
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
                let ds = corridor.delta_s(r.s_rel, o.s);
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
            // Finished and past the open end of the corridor (clearing the finish line): that
            // is meant to be off it, not a reason to drive back to it.
            let past_end = matches!(r.status, RacerStatus::Finished { .. })
                && !corridor.closed
                && r.s_rel > corridor.length_m - 1.0;
            if r.tactic != Tactic::Reverse
                && r.tactic != Tactic::Rejoin
                && !past_end
                && (off || heading_cos < 0.5)
            {
                r.tactic = Tactic::Rejoin;
                r.tactic_ticks = 0;
                r.stable_s = 0.0;
            }
            // Stuck against something → back out.
            if v > 1.0 {
                r.wants_room = false;
            }
            let wants_to_move = v_line(r.s) > 3.0 || clearing_line;
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
            head_on_now = head_on && v.abs() < 0.5;
            // Nose up to a parked car (closer than the creep stop gap allows a car to steer out
            // round it): back out a little first (Hollins, police seed 6: the last racer stopped
            // 0.9 m behind a busted car and waited there for the rest of the race).
            // A racer out of the race standing there is as good as parked (Loop 9 with police:
            // a racer nosed up to a captured one waited behind it for the rest of the race).
            let nosed_to = rel.iter().find(|x| {
                let standing_out = x.o.out_of_race && x.o.v_s.abs() < 0.5;
                x.ds > 0.0
                    && (x.o.parked || standing_out)
                    && x.gap < CREEP_STOP_GAP_M + if standing_out { 1.5 } else { 0.5 }
                    && (x.o.n - r.n).abs() < hw + x.o.half_w + if standing_out { 0.5 } else { 0.0 }
            });
            let nosed_up = nosed_to.is_some();
            if r.tactic != Tactic::Reverse
                && v.abs() < 0.8
                && wants_to_move
                && ((!held_by_car && r.throttle > 0.3) || head_on || nosed_up)
            {
                r.stuck_s += dt;
            } else if r.tactic != Tactic::Reverse {
                r.stuck_s = 0.0;
            }
            if r.stuck_s > 2.0 {
                r.tactic = Tactic::Reverse;
                r.tactic_ticks = 0;
                r.stuck_s = 0.0;
                // Nose to nose, or nosed up to a parked car: back out far enough to steer
                // round it (a car needs ~5 m of travel to move 1.5 m sideways).
                r.reverse_s = if head_on || nosed_up { 3.0 } else { 1.6 };
                // Nosed up to a car standing for good: pick the side with more room beside it
                // and work towards it over as many reverse / creep cycles as it takes (Lantern
                // police, seeds 5 and 31: boxed in between captured cars, 0.4 m ahead and
                // 2.4 m behind, a straight reverse and back to the line went nowhere).
                if let Some(x) = nosed_to {
                    r.escape = escape_beside(r, x, hw, &pass_limits);
                }
            }
            // Held at a standstill behind a car standing for good, with no pass that fits (a
                // Nothing I can see holds me: count the stucks at this spot, and from the
                // second one back out further and come back on the other side of the road.
                if !head_on && !nosed_up && !held_by_car {
                    let (lo, hi) = (road_lo.min(pass_lo), road_hi.max(pass_hi));
                    let mid = 0.5 * (lo + hi);
                    let (cycles, stuck_left) = match r.blind_stuck {
                        Some((at, k, left)) if (r.progress - at).abs() < BLIND_SAME_SPOT_M => {
                            (k + 1, left)
                        }
                        _ => (1, r.n < mid),
                    };
                    r.blind_stuck = Some((r.progress, cycles, stuck_left));
                    if cycles >= BLIND_DETOUR_AFTER {
                        r.reverse_s = 3.0;
                        // Always to the side away from where I first got stuck: on a touge the
                        // other side of the pile is where the K-rails were shoved out, over
                        // the edge.
                        let edge = hw + 0.6;
                        let n = if stuck_left { hi - edge } else { lo + edge };
                        let n = n.clamp(lo + edge, (hi - edge).max(lo + edge));
                        r.detour = Some((n, r.progress + BLIND_DETOUR_M));
                    }
                }
            // pass wants its full margin): squeeze past it with the escape clearance instead
            // (Lantern police seed 31: 3.4 m behind one captured car with another alongside,
            // 12 000 passes refused for room).
            if r.escape.is_none() && v.abs() < 1.0 && r.blocked_s > ESCAPE_AFTER_S {
                if let Some(x) = rel.iter().find(|x| {
                    x.ds > 0.0
                        && x.gap < 8.0
                        && standing_for_good(&x.o)
                        && r.debug.limit == SpeedLimit::Obstacle(who_id(x.o.who))
                }) {
                    r.escape = escape_beside(r, x, hw, &pass_limits);
                }
            }
            // An escape ends once I am past that car (or it has gone, or it is taking long).
            if let Some((who, n, age)) = r.escape {
                let past = rel
                    .iter()
                    .find(|x| x.o.who == who)
                    .is_none_or(|x| x.ds < -(hl + x.o.half_len));
                r.escape = (!past && age < ESCAPE_MAX_S).then_some((who, n, age + dt));
            }
            // The car right in front of me is stuck and cannot back out because I stand
            // A detour ends past the stuck spot.
            if r.detour.is_some_and(|(_, end)| r.progress > end) {
                r.detour = None;
            }
            // behind it: back off for it (otherwise both wait for good).
            let make_room = rel.iter().any(|x| {
                x.ds > 0.0
                    && x.gap < 4.0
                    && x.o.wants_room
                    && (x.o.n - r.n).abs() < hw + x.o.half_w + 1.2
            });
            if r.tactic != Tactic::Reverse && v.abs() < 0.8 && make_room {
                r.tactic = Tactic::Reverse;
                r.tactic_ticks = 0;
                r.stuck_s = 0.0;
                r.reverse_s = 1.6;
            }
        }

        // A red (or one turning red) at the next junction when I get there: nobody may be out
        // on the opposing side then — a racer stopping for it there stands nose to nose with
        // the queue coming the other way (main's Lantern downtown seed 7: the leader stopped
        // for a red mid-pass on the opposing side and the whole field stood for 2 minutes).
        let red_ahead = signal.is_some_and(|g| {
            let t = g.d_entry / v.max(4.0);
            g.d_entry < 150.0
                && match g.state {
                    SignalState::Red => g.remaining_s > t - 1.0,
                    SignalState::Yellow => true,
                    SignalState::Green => g.remaining_s < t + 2.0,
                    SignalState::Off => false,
                }
        });
        // Pass-target bookkeeping.
        let target_rel = |w: Who| rel.iter().find(|x| x.o.who == w);
        match r.tactic {
            Tactic::Overtake => {
                let split = r.split_with.and_then(target_rel);
                let done = match r.pass_target.and_then(target_rel) {
                    Some(x) => {
                        // Splitting: past both cars, not just the target.
                        let past = |q: &Rel| q.ds < -(hl + q.o.half_len + 2.0);
                        if past(x) && split.is_none_or(past) {
                            r.passes += 1;
                            r.splits += u32::from(split.is_some());
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
                            // Held well below my pace by some other car (one in the lane I
                            // moved into) for a second: this pass is not happening, re-plan
                            // (Loop 9 at 23:00 with traffic: a racer passing a rival sat 11 s
                            // behind a civilian at 24 m/s with its pace at 67 m/s).
                            let held_by_other = matches!(r.debug.limit,
                                SpeedLimit::Obstacle(w) if w != who_id(x.o.who))
                                && v_line(r.s) > r.debug.v_target + 3.0;
                            if held_by_other {
                                r.pass_blocked_s += dt;
                            } else {
                                r.pass_blocked_s = 0.0;
                            }
                            let blocked = r.pass_blocked_s > OVERTAKE_BLOCKED_S;
                            // Still behind, out on the opposing lanes, and a junction comes
                            // up before I could be past: tuck back in.
                            let junction = x.ds > 0.0
                                && !x.o.parked
                                && opposing_through_junction(
                                    corridor,
                                    r.s,
                                    brake_dist.max(v * 2.0) + 10.0,
                                    r.n_goal,
                                    hw,
                                );
                            // Out on the opposing side, still behind, a red coming: tuck back in.
                            let red = x.ds > 0.0
                                && red_ahead
                                && r.n - hw
                                    < -corridor.sample((r.s / CORRIDOR_STEP_M) as isize).left_m
                                && signal
                                    .is_some_and(|g| g.d_entry < brake_dist.max(v * 2.0) + 30.0);
                            // Splitting: either car drifting toward the gap, or the gap closing,
                            // while it is still ahead of me: tuck back in.
                            let split_off = r.split_with.is_some()
                                && match split {
                                    None => true,
                                    Some(q) => split_closing(x, q, hw, true),
                                };
                            too_late
                                || lost
                                || gone
                                || stale
                                || oncoming
                                || junction
                                || red
                                || blocked
                                || {
                                    r.split_aborts += u32::from(split_off);
                                    split_off
                                }
                        }
                    }
                    None => true,
                };
                if done {
                    r.tactic = Tactic::Follow;
                    r.pass_target = None;
                    r.split_with = None;
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
                // A car standing behind for good (parked, or out of the race) will not move into
                // me: back up to a metre from it (boxed in between captured cars with 2.4 m
                // behind, a 4 m rule left no reverse at all).
                let blocked_behind = rel.iter().any(|x| {
                    let standing = (x.o.parked || x.o.out_of_race) && x.o.v_s.abs() < 0.5;
                    let keep = if standing {
                        REVERSE_STANDING_GAP_M
                    } else {
                        4.0
                    };
                    x.ds < 0.0
                        && x.gap < keep + 1.0 * (-v).max(0.0)
                        && (x.o.n - r.n).abs() < hw + x.o.half_w + 1.2
                });
                // Cut short by something behind me: ask it for room.
                r.wants_room = blocked_behind && r.tactic_ticks as f32 * dt <= r.reverse_s;
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
            // A civilian keeping its lane reads a few tenths of a m/s across the corridor
            // (the corridor's sampled direction against its lane's); over the 3 s look-ahead
            // that is a metre of phantom drift into the next lane.
            let v_n = if matches!(o.who, Who::Civilian(_))
                && o.v_n.abs() < CIV_DRIFT_DEADBAND * o.v_s.abs()
            {
                0.0
            } else {
                o.v_n
            };
            let drift = v_n * t_reach.min(horizon);
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
        // Clearance used against a car ahead: grows with the closing speed. The same figure
        // decides what I brake for and who blocks me (Loop 9 at 23:00 with traffic: with a
        // smaller one for "who blocks me" than for braking, a racer closing at 20 m/s on a
        // civilian 2.6 m to one side braked to its speed from 110 m out and never tried to
        // pass it). Where a pass goes keeps the plain margin (the closing one there cost
        // Lantern s95 seed 8 two cars, refused passes in a downtown queue).
        let ahead_margin = |o: &Obstacle| -> f32 {
            let closing = (v - o.v_s).max(0.5);
            let extra = if is_civ(o) { 0.5 } else { 1.0 };
            margin_for(o) + (0.15 * (closing - 2.0)).clamp(0.0, extra)
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
                .filter(|x| {
                    overlaps(
                        band,
                        &x.o,
                        ahead_margin(&x.o),
                        x.gap / (v - x.o.v_s).max(1.0),
                    )
                })
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
                // Street racing: a civilian ahead is gone round, not queued behind — look for
                // a gap as soon as I am closing on it.
                let weave = is_civ(&x.o) && v - x.o.v_s > 1.0;
                if (r.blocked_s > p.overtake_patience_s || momentum || weave)
                    && x.o.v_s > -1.0
                    && !done_racing
                    // Start discipline is about the field: a civilian stopped in my lane
                    // (at a light) is gone round as usual (main's Lantern downtown: the
                    // field sat 20 s behind one just after the start).
                    && (!start_phase || x.o.parked || !matches!(x.o.who, Who::Racer(_)))
                {
                    let ctx = PassCtx {
                        corridor,
                        line,
                        profile,
                        brake_dist,
                        v,
                        margin: margin_for(&x.o),
                        draft,
                        junction_clear,
                        red_ahead,
                    };
                    let plan = plan_pass(r, x, &rel, &ctx, &pass_limits);
                    let slot = match plan {
                        Ok(_) => 0,
                        Err(e) => e as usize,
                    };
                    r.pass_checks[slot] += 1;
                    if let Ok((n_pass, split_with)) = plan {
                        r.tactic = Tactic::Overtake;
                        r.tactic_ticks = 0;
                        r.pass_blocked_s = 0.0;
                        r.alongside_s = 0.0;
                        r.pass_target = Some(x.o.who);
                        r.split_with = split_with;
                        r.n_goal = n_pass;
                        r.get_run = None;
                    } else if !is_civ(&x.o) {
                        r.get_run = Some(x.o.who);
                    }
                }
            }
        }

        // Gap planner (weaving): in traffic, pick the lane with the most room over the next
        // ~100 m and move into it early, instead of driving up to the car in mine and waiting
        // for a pass. A lane counts only if moving over is safe now (nothing alongside on the
        // way, nothing closing from behind in it).
        // Straights only: weaving into a bend takes me off my line where the off-line cap
        // would have to brake mid-corner.
        if r.tactic == Tactic::Follow
            && !done_racing
            && !start_phase
            && !fleeing
            && v < WEAVE_MAX_MPS
            && brake_dist > 2.5 * v.max(8.0)
        {
            let room = |c: f32| -> f32 {
                rel.iter()
                    .filter(|q| q.ds > 0.0 && (q.o.n - c).abs() < hw + q.o.half_w + 0.3)
                    .map(|q| (q.gap + (q.o.v_s - v) * WEAVE_LOOK_S).max(q.gap.min(0.0)))
                    .fold(WEAVE_RANGE_M, f32::min)
            };
            let safe = |c: f32| -> bool {
                let band = my_band(r.n, c);
                !rel.iter().any(|q| {
                    let lat =
                        band.0 < q.o.n + q.o.half_w + 0.4 && band.1 > q.o.n - q.o.half_w - 0.4;
                    lat && ((q.ds.abs() < hl + q.o.half_len + 3.0)
                        || (q.ds < 0.0 && q.gap < 15.0 && q.o.v_s > v + 0.5))
                })
            };
            // My own direction's lanes only, here and over the next 60 m (two-lane roads have
            // no other lane of mine: the opposing one is for passes and the whole-road line,
            // which check for oncoming traffic).
            let own = |s: f32| to_limits(corridor.bounds(s));
            // (Out to 4 s of travel at speed: a lane that ends a second ahead at 65 m/s is no
            // lane — Loop 9 s95, a racer weaved into one and spun off where it merged.)
            let (o_lo, o_hi) = [0.0, 30.0, 60.0, 2.0 * v, 4.0 * v]
                .iter()
                .map(|x| own(r.s + x))
                .fold((f32::NEG_INFINITY, f32::INFINITY), |(a, b), (l, h)| {
                    (a.max(l), b.min(h))
                });
            // Time to get back to my line before the next braking zone (Loop 9 s95 seed 3: a
            // racer 5 m off its line at 55 m/s ended its weave into the bend and lost it).
            let back_in_time = |c: f32| {
                let back_s = (c - line_n(r.s)).abs() / 0.9;
                brake_dist > v * (2.5 + back_s)
            };
            let here = room(r.n);
            let current = r.weave_n.filter(|c| *c >= o_lo - 0.1 && *c <= o_hi + 0.1);
            let mut best: Option<(f32, f32)> = None;
            let mut c = o_lo;
            while o_hi - o_lo >= WEAVE_MIN_SPAN_M && c <= o_hi + 0.01 {
                let f = room(c);
                if (c - r.n).abs() > 1.5
                    && f > here + WEAVE_GAIN_M
                    && back_in_time(c)
                    && safe(c)
                    && best.is_none_or(|b| f > b.1)
                {
                    best = Some((c, f));
                }
                c += PASS_LANE_STEP_M;
            }
            r.weave_s += dt;
            r.weave_n = match (current, best) {
                // Keep my pick while it still pays and is not stale.
                (Some(cur), _)
                    if r.weave_s < 8.0
                        && back_in_time(cur)
                        && room(cur) > here.min(room(line_n(r.s))) - 5.0 =>
                {
                    Some(cur)
                }
                (_, Some((c, _))) if here < WEAVE_RANGE_M - 1.0 => {
                    r.weave_s = 0.0;
                    Some(c)
                }
                _ => None,
            };
            if let Some(c) = r.weave_n {
                tuck = Some(c);
            }
        } else {
            r.weave_n = None;
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
        if r.tactic == Tactic::Overtake && r.split_with.is_some() {
            // Splitting: aim at the middle of the gap.
            let pair = (
                r.pass_target.and_then(target_rel),
                r.split_with.and_then(target_rel),
            );
            if let (Some(x), Some(q)) = pair {
                let (a, b) = if x.o.n < q.o.n { (x, q) } else { (q, x) };
                r.n_goal = 0.5 * ((a.o.n + a.o.half_w) + (b.o.n - b.o.half_w));
            }
        } else if r.tactic == Tactic::Overtake {
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
            Tactic::Follow | Tactic::Rejoin | Tactic::Reverse if r.escape.is_some() => {
                follow_w = 0.0;
                r.escape.map_or(r.n, |e| e.1)
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
            Tactic::Follow | Tactic::Rejoin if r.detour.is_some() => {
                follow_w = 0.0;
                r.detour.map_or(r.n, |d| d.0)
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
            // On the whole-road line: keep up with it as it swings across the road.
            Tactic::Follow if follow_w > 0.0 && r.wide_on => {
                let slope = (line_n(r.s + 2.0) - line_n(r.s)) / 2.0;
                (1.2 * slope.abs() * v).max(0.9)
            }
            _ => 0.9,
        };
        if r.n_cmd.is_nan() {
            r.n_cmd = r.n;
        }
        // Lane-splitting: the two cars I am going between get a small fixed clearance.
        let split_pair = if r.tactic == Tactic::Overtake && r.split_with.is_some() {
            [r.pass_target, r.split_with]
        } else {
            [None, None]
        };
        let splitting = |o: &Obstacle| split_pair.contains(&Some(o.who));
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
            // A civilian facing me that we meet within 1.5 s: keep to my side of it instead
            // of stopping for it (it would stop for me in turn). Police and racers keep the
            // old rules: a box of standing units is squeezed past as before.
            let meeting = is_civ(&x.o)
                && x.ds > 0.0
                && x.o.facing < -0.3
                && x.gap < 1.5 * (v - x.o.v_s).max(0.0) + 2.5;
            if !alongside && !arriving && !meeting {
                continue;
            }
            // A car in line right behind me is not beside me: it bounds neither side (grid
            // highway: the car on my bumper at a red light pinned me in my lane, so I could not
            // pull out round the civilian ahead with the next lane empty).
            let in_line_behind =
                x.ds < 0.0 && x.gap > 0.5 && (x.o.n - r.n).abs() < hw + x.o.half_w - 0.3;
            if in_line_behind {
                continue;
            }
            let civ_extra = if is_civ(&x.o) { civ_margin } else { 0.0 };
            let side_gap = if splitting(&x.o) {
                SPLIT_MARGIN_M
            } else if parked_non_racer(&x.o) {
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
        if clearing_line {
            // The profile stops at the corridor's end; roll on past it.
            v_target = FINISH_CLEAR_MPS;
        }
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
        // end there is only whatever the road leads into — a car park wall, a dead end), or
        // once past its end where it ends at the finish line (`clearing_line`).
        if clearing_line {
            cap_to(&mut v_target, FINISH_CLEAR_MPS, SpeedLimit::Mode);
        } else if matches!(r.status, RacerStatus::Finished { .. })
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
                // Out of the race, not in a tight bend either: there is no room to pass a car
                // stopped in a hairpin (Hollins, police seeds 3-6: a racer busted in the first
                // hairpin held the field for the rest of the race).
                // Same for a car pulling over for the police: stopped in a hairpin it closes
                // the road while the unit takes its time (Hollins, police seed 6: a racer
                // yielding mid-hairpin held the two behind it for a minute until they were
                // caught too).
                let in_bend = (out_of_race || r.tactic == Tactic::Yield)
                    && (-5..=10).any(|j| {
                        line.curvature[corridor.wrap(i0 + j)].abs() > 1.0 / PARK_MIN_RADIUS_M
                    });
                (r.n - lim_hi).abs() > 0.5 || near_junction || in_bend
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
        let mut v_offline = f32::INFINITY;
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
                    v_offline = v_offline.min(cap);
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
                // On the major road of a priority junction: civilians stopped at its minor
                // approaches are giving way to me.
                let has_way = !in_junction
                    && priority.is_some_and(|d| (d - d_entry).abs() <= 2.5 * CORRIDOR_STEP_M);
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
                    let stopped_inside = speed <= 0.5
                        && in_strip
                        && !in_junction
                        && !protected
                        && !has_way
                        && !waited_out;
                    // With the right of way (major road of a priority junction), only a
                    // civilian too close to stop before the box (about 4 m/s² of braking) is
                    // a threat; the rest give way (main's racer AI fix, grid ramps).
                    let committed = speed * speed / (2.0 * 4.0) > d - radius - 1.0;
                    // A moving civilian: when does its present course enter / leave my strip?
                    // It only conflicts when that overlaps my own pass through the box, with
                    // my crossing gap either side. Cross traffic held at its red
                    // (`protected`) only counts while already moving inside the box.
                    let conflict =
                        speed > 0.5 && (!protected || d < radius) && (!has_way || committed) && {
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
            let margin = if splitting(o) {
                SPLIT_MARGIN_M
            } else {
                ahead_margin(o)
            };
            if !overlaps(band, o, margin, x.gap / closing) {
                continue;
            }
            // Steering out round a parked car (pulled over, or standing for good): once the
            // lane I am heading for is clear of it, creep instead of standing still behind it
            // (a stopped car cannot move sideways), never closer than the stop gap.
            let steering_round = standing_for_good(o)
                && (r.tactic != Tactic::Overtake || r.pass_target == Some(o.who))
                && !overlaps(my_band(n_cmd, n_cmd), o, margin, 0.0);
            if steering_round && x.gap > CREEP_STOP_GAP_M {
                let creep = (a_brk * (x.gap - CREEP_STOP_GAP_M)).sqrt().min(CREEP_MPS);
                cap_to(&mut v_target, creep, SpeedLimit::Obstacle(who_id(o.who)));
                continue;
            }
            // Already clear of its body, only inside the margin, and steering further away:
            // keep creeping alongside it (rolling is what moves me sideways; going forward
            // cannot touch it). Otherwise a car stopped at the stop gap stays there.
            if steering_round && !overlaps(my_band(r.n, r.n), o, CREEP_CLEAR_M, 0.0) {
                cap_to(
                    &mut v_target,
                    CREEP_MPS,
                    SpeedLimit::Obstacle(who_id(o.who)),
                );
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
        let mut target = corridor.point(s_t, n_t);
        // Clearing the finish line past the end of the corridor: straight on along its last
        // direction.
        if clearing_line && r.s_rel + ld > corridor.length_m {
            // Towards the kerb on my side as I roll on, round whatever stands at the end of
            // the road straight ahead (the Knob lot: a broken-down car in the entrance).
            let (_, kerb) = limits(corridor.length_m);
            let end = corridor.point(corridor.length_m, kerb);
            let t = corridor.samples[corridor.samples.len() - 1].tangent;
            // From where I am past the end (`s` stops at the end; `s_rel` runs on).
            let past = r.s_rel + ld - corridor.length_m;
            target = [end[0] + t[0] * past, end[1] + t[1] * past, end[2]];
        }
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
            // Slowing for an off-line path: trail the brakes, never stamp on them. Mid-bend
            // a full brake does not make a tighter path any safer and unsettles the car (Loop
            // 9, skill 0.95, one-version handling: hakone_gt braked flat out at 60 m/s for a
            // 2 m drift off its line, yawed against its steering and spun).
            if limit == SpeedLimit::Mode && v_target >= v_offline - 1e-3 {
                acc = acc.max(-0.4 * profile.a_brake.max(3.0));
            }
            if decel_demand > 0.5 {
                acc = acc.min(-1.2 * decel_demand);
            }
            // A parked / yielding car rolling on to the kerb or out of a bend (`v_target` above
            // a crawl) must be able to pull away from rest: holding it there kept a racer
            // busted in a Hollins hairpin standing in the field's way for the rest of the race.
            let park_hold = matches!(r.tactic, Tactic::Park | Tactic::Yield) && v_target < 0.3;
            let hold = park_hold && v < 0.6;
            let (mut th, br) = r.ctrl.pedals(v, acc, dt, hold);
            // The controller releases the brake pedal near a standstill (a held brake there
            // selects reverse): a car told to stop holds on the handbrake instead of idling
            // forward into whatever it stopped for.
            let holding = v < 1.5 && (v_target < 0.3 || park_hold);
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
            opposing: r.n - hw < -corridor.sample((r.s / CORRIDOR_STEP_M) as isize).left_m,
            head_on: head_on_now,
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
        out_of_race: false,
        wants_room: false,
    })
}

/// Positions of the traffic agents that have a physical body in the sim (near a player).
fn traffic_bodies(world: &World) -> Vec<V2> {
    world
        .agents()
        .filter(|a| a.lod == wp_world::Lod::Physical)
        .map(|a| geom::xy(a.pos))
        .collect()
}

/// The sim vehicle `v` is the body of a physical traffic agent. That car is already seen as
/// a civilian (its agent), with the civilian rules: 3 s drift look-ahead, civilian margins and
/// headway. Seen a second time as a plain vehicle, those did not apply (Loop 9 at 23:00: a
/// racer hit a traffic car changing lanes next to the player, its limit set by the body).
fn is_traffic_body(bodies: &[V2], v: &VehicleObs) -> bool {
    let p = geom::xy(v.state.pos);
    bodies
        .iter()
        .any(|b| geom::dist(*b, p) < TRAFFIC_BODY_MATCH_M)
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
        // Beyond the ends of an open course (a grid behind the start line, a flight route that
        // starts at the racer) a car keeps its distance instead of landing on the end.
        let cp = c.project_unclamped(p, Some(hint), (range / CORRIDOR_STEP_M) as usize + 5);
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
            out_of_race: false,
            wants_room: false,
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

/// Whether the whole-road line `wide` is clear for racer `r` over the next `horizon` metres:
/// wherever it puts my body on the opposing lanes, nothing will be there when I get there
/// (oncoming cars by where they will have got to), and no junction it takes me through on the
/// opposing side has anyone near it (the side roads' traffic waits in the box: a risk check,
/// not a ban — an empty junction may be cut).
fn wide_line_clear(
    corridor: &Corridor,
    wide: &RacingLine,
    r: &Racer,
    me: Who,
    obstacles: &[Obstacle],
    civs: &[Civ],
    horizon: f32,
) -> bool {
    let hw = 0.5 * r.cap.width_m;
    let hl = 0.5 * r.cap.length_m;
    let v = r.v.max(5.0);
    let others: Vec<(&Obstacle, f32)> = obstacles
        .iter()
        .filter(|o| o.who != me && !matches!(o.who, Who::Vehicle(e) if e == r.entity))
        .map(|o| (o, corridor.delta_s(r.s_rel, o.s)))
        .filter(|(_, ds)| *ds > -10.0 && *ds < horizon + 120.0)
        .collect();
    let mut x = 0.0;
    while x <= horizon {
        let s = r.s + x;
        let c = corridor.sample((s / CORRIDOR_STEP_M).floor() as isize);
        let n_w = corridor.interp(&wide.offset, s);
        if n_w - hw < -c.left_m - 0.1 {
            // On the opposing side near a junction only when nobody is coming to it: look up
            // its arms, not just my corridor — a civilian turning in from a side road is not on
            // the corridor until it is in the way, and one meeting a racer nose to nose there
            // stays wedged (Lantern s95 seed 4).
            let i = (s / CORRIDOR_STEP_M).floor() as isize;
            let back = (WIDE_JUNCTION_CLEAR_M / CORRIDOR_STEP_M).ceil() as isize;
            if let Some(j) = (-back..=back).find(|&j| corridor.sample(i + j).junction) {
                let at = geom::xy(corridor.sample(i + j).pos);
                let t_clear = (x + j as f32 * CORRIDOR_STEP_M + 20.0) / v;
                if !junction_arms_empty(at, civs, t_clear) {
                    return false;
                }
                // Anything else (police, parked or physical cars) standing at it.
                if others.iter().any(|(o, ds)| {
                    !matches!(o.who, Who::Racer(_))
                        && (ds - (x + j as f32 * CORRIDOR_STEP_M)).abs() < 35.0
                }) {
                    return false;
                }
            }
            let t = x / v;
            for (o, ds) in &others {
                // Where it will be when I get here (oncoming cars close the distance).
                let ds_then = ds + o.v_s * t;
                let margin = 6.0 + 0.25 * (v - o.v_s).max(0.0);
                if (ds_then - x).abs() < hl + o.half_len + margin
                    && (o.n - n_w).abs() < hw + o.half_w + 0.8
                {
                    return false;
                }
            }
        }
        x += 2.0 * CORRIDOR_STEP_M;
    }
    true
}

/// Whether I can be through the junction at `at` before anyone else gets there, needing
/// `t_clear` s to reach it and get clear: a gap check, not "nobody about" (busy downtown junctions
/// are never empty). A civilian inside the box (within 12 m) closes it; one standing further out
/// is waiting at its line and does not; one heading to it closes it if it arrives within
/// `t_clear` + [`JUNCTION_GAP_S`].
fn junction_arms_empty(at: V2, civs: &[Civ], t_clear: f32) -> bool {
    !civs.iter().any(|c| {
        let to = geom::sub(at, c.pos);
        let d = geom::len(to);
        if d > JUNCTION_ARM_LOOK_M {
            return false;
        }
        if d < 12.0 {
            return true;
        }
        let speed = geom::len(c.vel);
        speed >= 0.5 && geom::dot(c.vel, to) > 0.0 && d / speed < t_clear + JUNCTION_GAP_S
    })
}

/// Whether a car at lateral offset `n` (half width `hw`) is out on the opposing lanes (left of
/// my lane's left edge) anywhere a junction box lies within `span` m ahead of `s`.
fn opposing_through_junction(corridor: &Corridor, s: f32, span: f32, n: f32, hw: f32) -> bool {
    let i0 = (s / CORRIDOR_STEP_M).floor() as isize;
    // (`span` is a braking distance, infinite with no braking point ahead: look 250 m at most.)
    let k = (span.clamp(0.0, 250.0) / CORRIDOR_STEP_M).ceil() as isize;
    (0..=k).any(|j| {
        let c = corridor.sample(i0 + j);
        c.junction && n - hw < -c.left_m
    })
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
    /// The next junction has nobody coming to it ([`junction_arms_empty`]).
    junction_clear: bool,
    /// The next junction's light is red (or turning red) when I get there.
    red_ahead: bool,
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

/// A car that will not move: parked (stood still for long), or out of the race and standing.
fn standing_for_good(o: &Obstacle) -> bool {
    o.parked || (o.out_of_race && o.v_s.abs() < 0.5)
}

/// Escape target beside the standing car `x`: the side with more room, with
/// [`ESCAPE_CLEAR_M`] to its body (keeps an escape already under way round the same car).
/// `None` when neither side has room for my car.
fn escape_beside(
    r: &Racer,
    x: &Rel,
    hw: f32,
    pass_limits: &dyn Fn(f32) -> (f32, f32),
) -> Option<(Who, f32, f32)> {
    if let Some(e) = r.escape.filter(|e| e.0 == x.o.who) {
        return Some(e);
    }
    let (lo, hi) = squeeze_limits(pass_limits(r.s + x.ds), &x.o);
    let beside = x.o.half_w + hw + ESCAPE_CLEAR_M;
    let up = (x.o.n + beside, hi - (x.o.n + beside));
    let down = (x.o.n - beside, (x.o.n - beside) - lo);
    let (n, room) = if up.1 >= down.1 { up } else { down };
    (room >= 0.0).then_some((x.o.who, n, 0.0))
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
) -> Result<(f32, Option<Who>), PassReject> {
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
    // No three-wide: never pass a car that is passing, nor pull out towards a car alongside
    // me. (A car right behind me in line is not alongside; one alongside on the other side
    // does not stop me moving away from it: grid highway, a racer boxed between a civilian
    // at the light and a rival on its left sat out a whole green with the right lane empty.)
    // (Three wide fits on a road of three lanes or more: there the lane checks below decide.
    // Loop 9 at 23:00 with traffic: the racers ahead were always mid-pass round civilians, so
    // the cars behind them were refused every pass for the whole race.)
    let here = ctx.corridor.sample((r.s / CORRIDOR_STEP_M) as isize);
    if o.passing && here.left_pass_m + here.right_m < THREE_WIDE_MIN_ROAD_M {
        return Err(PassReject::TargetBusy);
    }
    let alongside_towards = |n_pass: f32| {
        let dir = n_pass - r.n;
        rel.iter().any(|q| {
            let side = q.o.n - r.n;
            q.o.who != o.who
                && !matches!(q.o.who, Who::Civilian(_))
                && q.ds <= 0.0
                && q.gap < 3.0
                && side.abs() > hw + q.o.half_w - 0.5
                && (dir.abs() < 0.3 || side * dir > 0.0)
        })
    };
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
    // Right beside it first (preferred side, then the other), then a lane and two lanes
    // further out: on a multi-lane road the lane next to it may hold a car abreast of it while
    // the one beyond is empty (grid highway: passes were refused "lane ahead" with a free lane
    // two over).
    let candidates = [0.0f32, PASS_LANE_STEP_M, 2.0 * PASS_LANE_STEP_M]
        .into_iter()
        .flat_map(|extra| sides.map(|side| (side, extra)));
    'side: for (side, extra) in candidates {
        let n_pass = o.n + side * (need + extra);
        if alongside_towards(n_pass) {
            why = PassReject::TargetBusy;
            continue 'side;
        }
        // No passing on the opposing lanes through a junction (round a parked car aside): a
        // car out there meets the side roads' traffic waiting in the box, and with the field
        // queued behind it nobody can move (Lantern Row, skill 0.95 seed 8: the whole field
        // gridlocked in a junction for the rest of the race).
        // (Looking 40 m past the pass: a pass that ends just short of a junction stalls there
        // when the queue it was passing stops at the junction — Lantern s95 seed 4.)
        if (ctx.red_ahead || (!o.parked && !ctx.junction_clear))
            && opposing_through_junction(ctx.corridor, r.s, pass_dist + 40.0, n_pass, hw)
        {
            why = PassReject::LaneAhead;
            continue 'side;
        }
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
            // Round a parked car at a crawl only what stands beside it closes that lane: a car
            // standing further up it (the unit that caught a busted racer) is dodged by tucking
            // back in behind the parked one (Hollins, police seed 6).
            let beside = if o.parked {
                target.ds + hl + o.half_len + 1.5
            } else {
                target.ds + 2.0 * hl + 6.0
            };
            if q.ds > 0.0 && q.ds < beside {
                why = PassReject::LaneAhead;
                continue 'side;
            }
            // Oncoming traffic in that lane: it covers the distance too. A car standing in
            // it anywhere along the pass closes it as well (I would end up nose to nose).
            let oncoming = q.o.v_s < -1.0 && q.ds < pass_dist + (-q.o.v_s) * t_clear + 30.0;
            let standing =
                q.o.v_s.abs() < 1.0 && q.ds < if o.parked { beside } else { pass_dist + 10.0 };
            if q.ds > 0.0 && (oncoming || standing) {
                why = PassReject::LaneAhead;
                continue 'side;
            }
            // A car right behind me in line is following me whichever way I go: pulling out
            // does not cut it off.
            // (Only a car that is closing on me: one behind going no faster cannot run into
            // me when I move over.)
            let in_line = (q.o.n - r.n).abs() < hw + q.o.half_w - 0.5;
            if q.ds <= 0.0 && (q.gap < 3.0 || (q.gap < 12.0 && q.o.v_s > v + 0.5)) && !in_line {
                why = PassReject::LaneBehind;
                continue 'side;
            }
        }
        return Ok((n_pass, None));
    }
    // No side clear: go between it and a car abreast of it, if the gap is wide enough.
    if let Some((n_split, q)) = plan_split(r, target, rel, ctx, limits, pass_dist, t_clear) {
        return Ok((n_split, Some(q)));
    }
    Err(why)
}

/// The gap between two cars abreast, `a` and `b`, is closing on me: either drifts toward it
/// faster than [`SPLIT_DRIFT_MPS`] (`drift`), or it is narrower than my width plus
/// [`SPLIT_ABORT_ROOM_M`] — while either is still ahead of my centre.
fn split_closing(a: &Rel, b: &Rel, hw: f32, drift: bool) -> bool {
    if a.ds <= 0.0 && b.ds <= 0.0 {
        return false;
    }
    let (lo, hi) = if a.o.n < b.o.n { (a, b) } else { (b, a) };
    let gap = (hi.o.n - hi.o.half_w) - (lo.o.n + lo.o.half_w);
    let drifting = drift && (lo.o.v_n > SPLIT_DRIFT_MPS || hi.o.v_n < -SPLIT_DRIFT_MPS);
    gap < 2.0 * hw + SPLIT_ABORT_ROOM_M || drifting
}

/// Lane-splitting: a skilled driver closing gently on `target` goes between it and a car
/// abreast of it in my own lanes, when the gap is at least my width plus [`SPLIT_ROOM_M`],
/// neither car drifts toward it, and nothing else is in that slot. Returns the lateral
/// position (middle of the gap) and the other car.
fn plan_split(
    r: &Racer,
    target: &Rel,
    rel: &[Rel],
    ctx: &PassCtx,
    limits: &dyn Fn(f32) -> (f32, f32),
    pass_dist: f32,
    t_clear: f32,
) -> Option<(f32, Who)> {
    let o = &target.o;
    let hw = 0.5 * r.cap.width_m;
    let hl = 0.5 * r.cap.length_m;
    let risk = r.params.traffic_risk.clamp(SPLIT_MIN_RISK, 1.0);
    if r.profile.skill < SPLIT_MIN_SKILL || o.v_s < 1.0 || o.parked || o.out_of_race {
        return None;
    }
    for q in rel {
        let b = &q.o;
        if b.who == o.who
            || q.ds <= 0.0
            || b.v_s < 1.0
            || b.parked
            || b.out_of_race
            || b.passing
            || (q.ds - target.ds).abs() > o.half_len + b.half_len
        {
            continue;
        }
        if ctx.v - o.v_s.min(b.v_s) > SPLIT_MAX_CLOSING_MPS * risk {
            continue;
        }
        let (lo, hi) = if o.n < b.n { (target, q) } else { (q, target) };
        let gap = (hi.o.n - hi.o.half_w) - (lo.o.n + lo.o.half_w);
        if gap < 2.0 * hw + SPLIT_ROOM_M || split_closing(lo, hi, hw, true) {
            continue;
        }
        let n_split = 0.5 * ((lo.o.n + lo.o.half_w) + (hi.o.n - hi.o.half_w));
        // My own lanes only, for the whole pass.
        let mut x = 0.0;
        let mut room = true;
        while x <= pass_dist.max(10.0) {
            let (l, h) = limits(r.s + x);
            let left = ctx
                .corridor
                .sample(((r.s + x) / CORRIDOR_STEP_M) as isize)
                .left_m;
            if n_split - hw < l.max(-left) || n_split + hw > h {
                room = false;
                break;
            }
            x += CORRIDOR_STEP_M * 2.0;
        }
        if !room {
            continue;
        }
        // Nobody else in that slot: ahead just beyond the pair, coming the other way or
        // standing along the pass, or closing from behind.
        let clear = rel.iter().all(|w| {
            if w.o.who == o.who || w.o.who == b.who {
                return true;
            }
            if (w.o.n - n_split).abs() >= hw + w.o.half_w + 0.3 {
                return true;
            }
            let beyond = target.ds.max(q.ds) + 2.0 * hl + 6.0;
            let oncoming = w.o.v_s < -1.0 && w.ds < pass_dist + (-w.o.v_s) * t_clear + 30.0;
            let standing = w.o.v_s.abs() < 1.0 && w.ds < pass_dist + 10.0;
            if w.ds > 0.0 && (w.ds < beyond || oncoming || standing) {
                return false;
            }
            !(w.ds <= 0.0 && (w.gap < 3.0 || (w.gap < 12.0 && w.o.v_s > ctx.v + 0.5)))
        });
        if clear {
            return Some((n_split, b.who));
        }
    }
    None
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
    // A real line over every lane of the flight route, not its lane centre: a fleeing racer
    // on a multi-lane road held the planned lane's centre to the centimetre (owner, Loop 9:
    // "AI racers only follow the green line"). Fewer sweeps than the race line (that one is
    // built once; flights replan every few seconds).
    let line = RacingLine::optimise(
        &corridor,
        0.5 * r.cap.width_m,
        &LineParams {
            smooth_sweeps: FLIGHT_LINE_SMOOTH_SWEEPS,
            curvature_sweeps: FLIGHT_LINE_CURVATURE_SWEEPS,
            ..config.line
        },
    );
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
            out_of_race: false,
            wants_room: false,
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
                    surface: wp_contracts::SurfaceType::Asphalt,
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
            loose: Default::default(),
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

    /// Lane-splitting between two civilians abreast at 15 m/s, 2.3 m either side of the
    /// centre, me at 50 m doing `v` with `skill` and `risk`; the lower one drifts at `drift`.
    fn split_case(skill: f32, risk: f32, v: f32, half_gap: f32, drift: f32) -> Option<(f32, Who)> {
        let spec = RacerSpec {
            entity: EntityId(1),
            profile: DriverProfile {
                skill,
                risk,
                ..DriverProfile::default()
            },
            capability: cap(),
        };
        let mut f = Field::new(straight(), &[spec], RaceConfig::default());
        f.racers[0].s = 50.0;
        f.racers[0].n = 0.0;
        let car = |id: u32, n: f32, v_n: f32| Rel {
            o: Obstacle {
                who: Who::Civilian(id),
                s: 60.0,
                n,
                v_s: 15.0,
                v_n,
                half_len: 2.3,
                half_w: 0.9,
                unstable: false,
                passing: false,
                parked: false,
                facing: 1.0,
                out_of_race: false,
                wants_room: false,
            },
            ds: 10.0,
            gap: 5.4,
        };
        let rel = [car(1, -half_gap, drift), car(2, half_gap, 0.0)];
        let ctx = PassCtx {
            corridor: &f.corridor,
            line: &f.lines[0].1,
            profile: &f.racers[0].speed,
            brake_dist: 500.0,
            v,
            margin: 0.6,
            draft: false,
            junction_clear: true,
            red_ahead: false,
        };
        let limits = |_s: f32| (-5.0, 5.0);
        plan_split(&f.racers[0], &rel[0], &rel, &ctx, &limits, 60.0, 3.0)
    }

    #[test]
    fn skilled_driver_splits_a_wide_gap_closing_gently_and_nobody_else_does() {
        // Gap 2.8 m for a 1.8 m car (needs 2.6 m): through the middle, beside car 2.
        let (n, with) = split_case(0.9, 0.8, 18.0, 2.3, 0.0).expect("split");
        assert!(n.abs() < 1e-4);
        assert_eq!(with, Who::Civilian(2));
        // Not skilled enough.
        assert!(split_case(0.5, 0.8, 18.0, 2.3, 0.0).is_none());
        // Gap too narrow (2.2 m).
        assert!(split_case(0.9, 0.8, 18.0, 2.0, 0.0).is_none());
        // Closing too fast (10 m/s), or fast for a careful driver (4 m/s at risk 0.3: the
        // limit is 3 m/s from the 0.6 floor), which 3 m/s is not.
        assert!(split_case(0.9, 0.8, 25.0, 2.3, 0.0).is_none());
        assert!(split_case(0.9, 0.3, 19.0, 2.3, 0.0).is_none());
        assert!(split_case(0.9, 0.3, 17.9, 2.3, 0.0).is_some());
        // One of them drifting toward the gap.
        assert!(split_case(0.9, 0.8, 18.0, 2.3, 0.5).is_none());
        assert!(split_case(0.9, 0.8, 18.0, 2.3, -0.5).is_some());
    }

    /// Regression scenario for lane-splitting: a 3-lane road (11 m, own direction only), two
    /// civilians abreast at 15 m/s in the outer lanes with a 3.0 m gap between them, and a
    /// skilled racer coming up at 17.5 m/s (closing 2.5 m/s, inside the 3 m/s floor). Driven
    /// closed loop at the decision rate with simple kinematics for the racer. Returns
    /// (splits, split aborts, smallest lateral clearance to either civilian while alongside).
    fn split_scenario(skill: f32) -> (u32, u32, f32) {
        let mut corridor = straight();
        for smp in &mut corridor.samples {
            smp.left_m = 5.5;
            smp.left_pass_m = 5.5;
            smp.right_m = 5.5;
        }
        let spec = RacerSpec {
            entity: EntityId(1),
            profile: DriverProfile {
                skill,
                risk: 0.3,
                ..DriverProfile::default()
            },
            capability: cap(),
        };
        let mut f = Field::new(corridor, &[spec], RaceConfig::default());
        f.racers[0].laps_done = 1;
        let world = World::flat(TimeOfDay::from_hours(12.0), 1.0);
        let dt = 4.0 * PHYSICS_DT;
        let (mut x, mut y, mut v) = (0.0f32, 40.0f32, 17.5f32);
        let mut civ_s = 80.0f32;
        let civ_n = [-2.4f32, 2.4];
        let mut min_clear = f32::INFINITY;
        for _ in 0..(20.0 / dt) as usize {
            let mut me = obs(1, y, v, HandlingEffects::NEUTRAL);
            me.state.pos[0] = x;
            f.observe(&world, &[me], &NoPolice, dt);
            let civs: Vec<Obstacle> = civ_n
                .iter()
                .enumerate()
                .map(|(i, &n)| Obstacle {
                    who: Who::Civilian(i as u32 + 1),
                    s: civ_s,
                    n,
                    v_s: 15.0,
                    v_n: 0.0,
                    half_len: 2.3,
                    half_w: 0.9,
                    unstable: false,
                    passing: false,
                    parked: false,
                    facing: 1.0,
                    out_of_race: false,
                    wants_room: false,
                })
                .collect();
            f.decide(0, &civs, &[], None, None, dt);
            let d = f.debug_of(0);
            // Kinematics: lateral toward the command at up to 2 m/s, speed toward the
            // target at up to 4 m/s² / 9 m/s² braking.
            x += (d.n_cmd - x).clamp(-2.0 * dt, 2.0 * dt);
            v += (d.v_target - v).clamp(-9.0 * dt, 4.0 * dt);
            y += v * dt;
            civ_s += 15.0 * dt;
            let r = &f.racers[0];
            for &n in &civ_n {
                if (r.s - civ_s).abs() < 2.3 + 2.3 {
                    min_clear = min_clear.min((x - n).abs() - 0.9 - 0.9);
                }
            }
        }
        let r = &f.racers[0];
        (r.splits, r.split_aborts, min_clear)
    }

    #[test]
    fn skilled_racer_splits_two_civilians_abreast_on_a_three_lane_road() {
        let (splits, aborts, clear) = split_scenario(0.9);
        assert_eq!((splits, aborts), (1, 0), "clearance {clear:.2}");
        assert!(clear > 0.2, "clearance {clear:.2}");
        // A middling driver waits behind them instead.
        let (splits, _, _) = split_scenario(0.5);
        assert_eq!(splits, 0);
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
        f.decide(0, &[], civs, signal, None, 4.0 * PHYSICS_DT);
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

    /// The speed limit of a racer at 30 m/s on the centreline with a civilian doing 20 m/s
    /// 60 m ahead in the next lane (3.8 m to the right) moving across the corridor at `v_n`.
    fn next_lane_civ_limit(v_n: f32) -> SpeedLimit {
        let mut f = Field::new(
            straight(),
            &[RacerSpec {
                entity: EntityId(1),
                profile: DriverProfile::default(),
                capability: cap(),
            }],
            RaceConfig::default(),
        );
        f.racers[0].laps_done = 1;
        let world = World::flat(TimeOfDay::from_hours(12.0), 1.0);
        let dt = 4.0 * PHYSICS_DT;
        f.observe(
            &world,
            &[obs(1, 40.0, 30.0, HandlingEffects::NEUTRAL)],
            &NoPolice,
            dt,
        );
        let civ = Obstacle {
            who: Who::Civilian(1),
            s: 100.0,
            n: 3.8,
            v_s: 20.0,
            v_n,
            half_len: 2.3,
            half_w: 0.9,
            unstable: false,
            passing: false,
            parked: false,
            facing: 1.0,
            out_of_race: false,
            wants_room: false,
        };
        f.decide(0, &[civ], &[], None, None, dt);
        f.debug_of(0).limit
    }

    #[test]
    fn a_civilian_keeping_its_lane_next_to_mine_does_not_hold_me_up() {
        // Holding its lane, with the few tenths of a m/s across the corridor a lane reads
        // against the corridor's sampled direction: not in my way.
        assert_ne!(next_lane_civ_limit(0.0), SpeedLimit::Obstacle(1_000_001));
        assert_ne!(next_lane_civ_limit(-0.4), SpeedLimit::Obstacle(1_000_001));
        // Changing lanes into mine: braked for.
        assert_eq!(next_lane_civ_limit(-1.5), SpeedLimit::Obstacle(1_000_001));
    }

    #[test]
    fn a_finisher_rolls_on_past_a_corridor_that_ends_at_the_line_before_parking() {
        // `straight()` ends at 398 m and the config has no run-off (`finish_m` unset): the
        // finish line is the end of the corridor.
        let mut f = field(1);
        let world = World::flat(TimeOfDay::from_hours(12.0), 1.0);
        let dt = 4.0 * PHYSICS_DT;
        let at = |f: &mut Field, y: f32, v: f32| {
            f.observe(
                &world,
                &[obs(1, y, v, HandlingEffects::NEUTRAL)],
                &NoPolice,
                dt,
            );
            f.decide(0, &[], &[], None, None, dt);
            (f.debug_of(0), f.tactic_of(0))
        };
        at(&mut f, 380.0, 10.0);
        at(&mut f, 397.8, 8.0);
        assert!(
            matches!(f.racers[0].status, RacerStatus::Finished { .. }),
            "{:?}",
            f.racers[0].status
        );
        // 4 m past the line: still rolling on, not parked on it.
        let (d, tac) = at(&mut f, 402.0, 4.0);
        assert_ne!(tac, Tactic::Park);
        assert!(d.v_target >= FINISH_CLEAR_MPS - 1e-3, "{d:?}");
        // Clear of it: pull over.
        let (_, tac) = at(&mut f, 398.0 + FINISH_CLEAR_M + 2.0, 4.0);
        assert_eq!(tac, Tactic::Park);
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
