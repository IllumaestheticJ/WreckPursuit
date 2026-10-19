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
use wp_contracts::{EntityId, HandlingEffects, SimEvent, VehicleInput, VehicleState, PHYSICS_DT};
use wp_math::{Rng, StateHasher};
use wp_world::drive::{forward_of, DriveController, DriveGains};
use wp_world::geom::{self, Obb, V2, V3};
use wp_world::traffic::{CAR_LENGTH_M, CAR_WIDTH_M};
use wp_world::World;

use crate::capability::CarCapability;
use crate::corridor::{Corridor, CorridorOptions, CORRIDOR_STEP_M};
use crate::line::{LineParams, RacingLine, SpeedProfile};
use crate::profile::{DriverParams, DriverProfile};
use crate::pursuit::PoliceQuery;

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
/// Seconds of lateral closing speed added to the side clearance.
const LATERAL_LEAD_S: f32 = 0.6;

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

    /// Grid slots behind the start line (`s = 0`), front first: `(pos, yaw)`. Two staggered
    /// columns where the corridor is wide enough for two cars plus a gap, else single file.
    pub fn grid_slots(&self, count: usize, car_len_m: f32, car_width_m: f32) -> Vec<(V3, f32)> {
        let c = &self.corridor;
        let s_front = if c.closed {
            c.length_m - car_len_m
        } else {
            car_len_m + 2.0
        };
        let dir = if c.closed { -1.0 } else { 1.0 };
        let (l, r) = c.min_bounds(c.wrap_s(s_front - 60.0), 70.0);
        let two = l + r >= 2.0 * car_width_m + 2.5;
        let off = 0.5 * car_width_m + 0.6;
        let centre = 0.5 * (r - l);
        (0..count)
            .map(|k| {
                let (row_gap, n) = if two {
                    let n = if k % 2 == 0 {
                        centre - off
                    } else {
                        centre + off
                    };
                    (k as f32 * (car_len_m + 3.0) * 0.5, n)
                } else {
                    (k as f32 * (car_len_m + 3.5), centre)
                };
                let s = c.wrap_s(s_front + dir * row_gap);
                let s = if c.closed { s } else { s_front - row_gap };
                let p = c.point(s, n);
                let t = c.samples[(c.wrap_s(s) / CORRIDOR_STEP_M) as usize % c.len()].tangent;
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
        if self
            .tick
            .is_multiple_of(self.config.decision_ticks.max(1) as u64)
        {
            let dt = self.config.decision_ticks.max(1) as f32 * PHYSICS_DT;
            self.observe(world, vehicles, police, dt);
            let snapshot = self.perceive_all(world, vehicles);
            for k in 0..self.racers.len() {
                let obstacles = match &self.racers[k].flight {
                    None => snapshot.clone(),
                    Some(f) => self.perceive_on(&f.corridor, k, world, vehicles),
                };
                self.decide(k, &obstacles, dt);
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
            if self.config.allow_abandon
                && r.status == RacerStatus::Racing
                && lvl >= r.params.flee_wanted_level
            {
                r.status = RacerStatus::Fled;
                self.events.push(OpponentEvent::Abandoned {
                    entity: r.entity,
                    wanted_level: lvl,
                });
                let flight = plan_flight(world, &self.racers[k], &self.config);
                let r = &mut self.racers[k];
                match flight {
                    Some(f) => {
                        let cp = f.corridor.project(r.pos, None, 0);
                        r.hint = cp.index;
                        r.s = cp.s;
                        r.n = cp.n;
                        r.n_cmd = cp.n;
                        r.tactic = Tactic::Follow;
                        r.flight = Some(Box::new(f));
                    }
                    None => r.tactic = Tactic::Park,
                }
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
                    if geom::obb_overlap(&me, &other) {
                        let r = &mut self.racers[k];
                        if r.status == RacerStatus::Racing {
                            r.status = RacerStatus::Disqualified;
                            r.tactic = Tactic::Park;
                            self.events.push(OpponentEvent::Disqualified {
                                entity: r.entity,
                                civilian: a.id,
                            });
                        }
                        break;
                    }
                }
            }
        }
    }

    /// Step 2 for the shared corridor: every vehicle and nearby civilian as an obstacle.
    fn perceive_all(&mut self, world: &World, vehicles: &[VehicleObs]) -> Vec<Obstacle> {
        let mut out = Vec::new();
        let range = self
            .racers
            .iter()
            .fold(0.0f32, |m, r| m.max(r.params.perception_ahead_m))
            + 20.0;
        for v in vehicles {
            let who = match self.racer_index(v.entity) {
                Some(k) => {
                    let r = &self.racers[k];
                    if r.flight.is_some() {
                        // A fleeing racer is just another vehicle on the race corridor.
                        Who::Vehicle(r.entity)
                    } else {
                        let unstable = r.unstable;
                        out.push(Obstacle {
                            who: Who::Racer(k),
                            s: r.s,
                            n: r.n,
                            v_s: r.v_s,
                            v_n: r.v_n,
                            half_len: r.ext.0,
                            half_w: r.ext.1,
                            unstable,
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
            if let Some(o) = vehicle_obstacle(&self.corridor, who, cp, v) {
                out.push(o);
            }
        }
        let mut civ = civilians_on(&self.corridor, &self.racers, world, range, false);
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
            if geom::dist(pos, me.pos) > me.params.perception_ahead_m + 20.0 {
                continue;
            }
            let cp = corridor.project(pos, Some(me.hint), 80);
            if let Some(o) = vehicle_obstacle(corridor, Who::Vehicle(v.entity), cp, v) {
                out.push(o);
            }
        }
        let only_me = std::slice::from_ref(me);
        out.append(&mut civilians_on(
            corridor,
            only_me,
            world,
            me.params.perception_ahead_m + 20.0,
            true,
        ));
        out
    }

    /// Step 3 + 4 for racer `k`.
    fn decide(&mut self, k: usize, obstacles: &[Obstacle], dt: f32) {
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
        decide_racer(
            &mut self.racers[k],
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

    /// Who racer `k` (spec order) has passed on the road (debug / tests).
    pub fn passes_by(&self, k: usize) -> usize {
        self.racers[k].passes as usize
    }
}

/// Steps 3 + 4 for one racer on its current track.
#[allow(clippy::too_many_arguments)]
fn decide_racer(
    r: &mut Racer,
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
        let limits = |s: f32| {
            let (l, rr) = corridor.bounds(s);
            let lo = -l + hw + 0.25;
            let hi = rr - hw - 0.25;
            if lo <= hi {
                (lo, hi)
            } else {
                (0.5 * (lo + hi), 0.5 * (lo + hi))
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
            .filter(|x| x.ds > -p.mirror_range_m && x.ds < p.perception_ahead_m)
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

        // Distance to the next braking point (profile drops 4 m/s below now), m.
        let brake_dist = {
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

        // ---- Tactical state machine ----
        let (lim_lo, lim_hi) = limits(r.s);
        r.tactic_ticks += 1;
        let parked = matches!(r.status, RacerStatus::Disqualified | RacerStatus::Retired)
            || r.tactic == Tactic::Park;
        if parked {
            r.tactic = Tactic::Park;
        } else {
            // Off the corridor / facing the wrong way → rejoin.
            let t = corridor.samples[r.hint].tangent;
            let heading_cos = geom::dot(r.fwd, t);
            let off = r.n < lim_lo - 1.0 || r.n > lim_hi + 1.0;
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
            if r.tactic != Tactic::Reverse
                && v.abs() < 0.8
                && wants_to_move
                && !held_by_car
                && r.throttle > 0.3
            {
                r.stuck_s += dt;
            } else if r.tactic != Tactic::Reverse {
                r.stuck_s = 0.0;
            }
            if r.stuck_s > 2.0 {
                r.tactic = Tactic::Reverse;
                r.tactic_ticks = 0;
                r.stuck_s = 0.0;
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
                            true
                        } else {
                            // Abort if the pass cannot be completed before the braking zone.
                            let alongside = x.ds.abs() < hl + x.o.half_len;
                            let too_late = brake_dist < v * 0.6 && !alongside && x.ds > 0.0;
                            let stale = r.tactic_ticks as f32 * dt > 10.0;
                            too_late || stale
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
                if r.tactic_ticks as f32 * dt > 1.6 {
                    r.tactic = Tactic::Rejoin;
                    r.tactic_ticks = 0;
                    r.ctrl.resync(0.0);
                }
            }
            Tactic::Rejoin => {
                let t = corridor.samples[r.hint].tangent;
                let ok = r.n >= lim_lo - 0.3 && r.n <= lim_hi + 0.3 && geom::dot(r.fwd, t) > 0.94;
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
        let margin_for = |o: &Obstacle| -> f32 {
            let base = if matches!(o.who, Who::Civilian(_)) {
                p.civilian_margin_m
            } else {
                LAT_MARGIN_M
            };
            base + if o.unstable { 1.5 } else { 0.0 }
        };

        // Follow mode: who blocks the line?
        if r.tactic == Tactic::Follow {
            let band = my_band(r.n, r.n_cmd);
            let blocker = rel
                .iter()
                .filter(|x| x.ds > 0.0 && x.gap < 30.0)
                .filter(|x| overlaps(band, &x.o, margin_for(&x.o), x.gap / (v - x.o.v_s).max(1.0)))
                .min_by(|a, b| a.ds.total_cmp(&b.ds));
            match blocker {
                Some(x) if x.o.v_s < v_line(x.o.s) - 0.7 => r.blocked_s += dt,
                _ => r.blocked_s = (r.blocked_s - dt).max(0.0),
            }
            if let Some(x) = blocker {
                if r.blocked_s > p.overtake_patience_s && x.o.v_s > -1.0 {
                    let ctx = PassCtx {
                        corridor,
                        line,
                        profile,
                        brake_dist,
                        v,
                        margin: margin_for(&x.o),
                    };
                    if let Some(n_pass) = plan_pass(r, x, &rel, &ctx, &limits) {
                        r.tactic = Tactic::Overtake;
                        r.tactic_ticks = 0;
                        r.pass_target = Some(x.o.who);
                        r.n_goal = n_pass;
                    }
                }
            }
        }

        // Defend: one move toward the attacker's side.
        if r.tactic == Tactic::Follow
            && p.defends
            && !r.defended
            && r.flight.is_none()
            && brake_dist > v * 2.0
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
                let need = hw + x.o.half_w + margin_for(&x.o) + p.overtake_margin_m;
                let want = x.o.n + side * need;
                let (lo, hi) = limits(r.s + x.ds.max(0.0));
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
            Tactic::Follow => {
                follow_w = 1.0;
                line_n(r.s) + r.line_noise
            }
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
            Tactic::Park => {
                follow_w = 0.0;
                lim_hi
            }
        };
        let rate = match r.tactic {
            Tactic::Overtake | Tactic::Defend => 1.4 + 1.0 * r.profile.aggression,
            Tactic::Park => 0.8,
            _ => 0.9,
        };
        if r.n_cmd.is_nan() {
            r.n_cmd = r.n;
        }
        let mut n_cmd = approach(r.n_cmd, desired.clamp(lim_lo, lim_hi), rate * dt);

        // Hard side constraints: never move toward a car that overlaps me longitudinally.
        let mut lo = lim_lo.min(r.n);
        let mut hi = lim_hi.max(r.n);
        let mut squeezed_by_ahead = false;
        for x in &rel {
            if x.ds.abs() > hl + x.o.half_len + 2.5 {
                continue;
            }
            let civ_extra = if is_civ(&x.o) {
                p.civilian_margin_m
            } else {
                0.0
            };
            let need = hw + x.o.half_w + SIDE_GAP_M + civ_extra;
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
        let mut v_target = v_line(r.s + v * (0.15 + r.brake_err_s));
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
        match r.tactic {
            Tactic::Rejoin => cap_to(&mut v_target, 7.0, SpeedLimit::Mode),
            Tactic::Park => cap_to(
                &mut v_target,
                (v - 3.0 * dt * 4.0).max(0.0),
                SpeedLimit::Mode,
            ),
            _ => {}
        }
        if r.unstable {
            cap_to(&mut v_target, v - 1.0, SpeedLimit::Unstable);
        }
        // Off the racing line (passing, defending, squeezed): the path at constant offset
        // `n_cmd` has its own curvature — slow for it (inside lines are tighter).
        let a_brk = profile.a_brake.max(3.0) * 0.85;
        if (n_cmd - line_n(r.s)).abs() > 1.0 {
            let mut x = 0.0;
            let horizon = (v * v / (2.0 * a_brk) + 20.0).min(150.0);
            while x <= horizon {
                let kc = corridor.centre_curvature(r.s + x);
                let den = (1.0 - kc * n_cmd).max(0.2);
                let k = (kc / den).abs();
                if k > 1e-4 {
                    let v_off = (profile.a_lat / k).sqrt();
                    let cap = (v_off * v_off + 2.0 * a_brk * x).sqrt();
                    cap_to(&mut v_target, cap, SpeedLimit::Mode);
                }
                x += 2.0 * CORRIDOR_STEP_M;
            }
        }
        // Obstacle caps: anything ahead in the band I'm driving through.
        let band = my_band(r.n, n_cmd);
        for x in &rel {
            if x.ds <= 0.0 {
                continue;
            }
            let o = &x.o;
            let civ = is_civ(o);
            let closing = (v - o.v_s).max(0.5);
            if !overlaps(band, o, margin_for(o), x.gap / closing) {
                continue;
            }
            let headway = if civ {
                p.civilian_headway_s
            } else {
                p.headway_s
            } * if o.unstable { 1.6 } else { 1.0 };
            let min_gap = if civ { MIN_GAP_M + 2.0 } else { MIN_GAP_M };
            let free = x.gap - min_gap;
            let vo = o.v_s.max(0.0);
            let v_brake = (vo * vo + 2.0 * a_brk * free.max(0.0)).sqrt();
            let v_time = vo + free / headway;
            let cap = if free <= 0.0 {
                vo.min(v - 2.0).max(0.0)
            } else {
                v_brake.min(v_time)
            };
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
            let into_corner = brake_dist < v * p.side_by_side_s;
            if squeezed_by_ahead || into_corner || r.unstable {
                cap_to(
                    &mut v_target,
                    x.o.v_s - 1.5,
                    SpeedLimit::BackOut(who_id(x.o.who)),
                );
            }
        }

        // ---- Controls ----
        let ld = r.ctrl.lookahead(v);
        let s_t = r.s + ld;
        let (t_lo, t_hi) = limits(s_t);
        let n_t = (n_cmd + follow_w * (line_n(s_t) - line_n(r.s)))
            .clamp(t_lo.min(n_cmd), t_hi.max(n_cmd));
        let target = corridor.point(s_t, n_t);
        let mut steer = r.ctrl.steer(r.pos, r.fwd, target);
        steer = (steer - r.effects.steer_pull).clamp(-1.0, 1.0);
        let mut input = VehicleInput {
            headlights: true,
            ..Default::default()
        };
        if r.tactic == Tactic::Reverse {
            // Brake at a standstill engages reverse; the brake pedal then drives backwards.
            input.brake = 0.7;
            input.steer = -steer;
            r.throttle = 0.0;
        } else {
            let acc = (2.5 * (v_target - v)).clamp(-12.0, 8.0);
            let hold = r.tactic == Tactic::Park && v < 0.6;
            let (mut th, br) = r.ctrl.pedals(v, acc, dt, hold);
            if r.unstable {
                th = th.min(0.2);
            }
            th = th.min(r.throttle + p.throttle_rate_per_s * dt);
            r.throttle = th;
            input.throttle = th;
            input.brake = br;
            input.steer = steer;
        }
        r.input = input;
        r.debug = Debug {
            v_target,
            limit,
            n_cmd,
            s: r.s,
            n: r.n,
            brake_dist,
        };
    }
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
    let (l, r) = c.bounds(cp.s);
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
        let (l, rr) = c.bounds(cp.s);
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
        });
    }
    out
}

/// Context for [`plan_pass`].
struct PassCtx<'a> {
    corridor: &'a Corridor,
    line: &'a RacingLine,
    profile: &'a SpeedProfile,
    brake_dist: f32,
    v: f32,
    margin: f32,
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
) -> Option<f32> {
    let p = &r.params;
    let o = target.o;
    let v = ctx.v;
    let hw = 0.5 * r.cap.width_m;
    let hl = 0.5 * r.cap.length_m;
    let closing = (ctx.corridor.interp(&ctx.profile.v, o.s) - o.v_s).max(1.5);
    let pass_len = target.gap.max(0.0) + 2.0 * hl + 2.0 * o.half_len + 3.0;
    let pass_dist = v.max(5.0) * pass_len / closing;
    if pass_dist > ctx.brake_dist + v * p.side_by_side_s || pass_dist > 400.0 {
        return None;
    }
    let need = hw + o.half_w + ctx.margin + p.overtake_margin_m;
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
    'side: for side in sides {
        let n_pass = o.n + side * need;
        // Room for the whole pass.
        let mut x = 0.0;
        while x <= pass_dist.max(10.0) {
            let (lo, hi) = limits(r.s + x);
            if n_pass < lo || n_pass > hi {
                continue 'side;
            }
            x += CORRIDOR_STEP_M * 2.0;
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
            if q.ds > 0.0 && q.ds < pass_dist + 15.0 {
                continue 'side;
            }
            if q.ds <= 0.0 && q.gap < 12.0 && q.o.v_s > v - 2.0 {
                continue 'side;
            }
        }
        return Some(n_pass);
    }
    None
}

/// Plan a flight route from the racer's position to the POI farthest away.
fn plan_flight(world: &World, r: &Racer, config: &RaceConfig) -> Option<Flight> {
    let pois = &world.network().pois;
    let here = r.pos;
    let best = pois
        .iter()
        .enumerate()
        .max_by(|a, b| {
            geom::dist(geom::xy(a.1.pos), here)
                .total_cmp(&geom::dist(geom::xy(b.1.pos), here))
                .then(b.0.cmp(&a.0))
        })?
        .1
        .pos;
    let route = world.plan_route([here[0], here[1], 0.0], Some(r.fwd), best)?;
    if route.polyline.len() < 2 {
        return None;
    }
    let corridor = Corridor::from_route(world.map(), &route, &config.corridor);
    if corridor.len() < 5 {
        return None;
    }
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
