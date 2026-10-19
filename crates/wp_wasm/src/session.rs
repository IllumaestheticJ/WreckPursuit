//! [`Session`]: the pure-Rust core behind the wasm-bindgen surface (natively testable).
//!
//! A session owns one [`wp_sim::Sim`] and drives it tick by tick with its own
//! [`wp_math::FixedStep`]. Every mutation (spawn, input change, reset, assists) goes through
//! [`Session::push_op`], which stamps it with the current tick, applies it and appends it to
//! the session log. That log plus the header *is* the replay; playback feeds the log back in
//! instead of live calls.

use std::collections::BTreeMap;

use wp_contracts::{
    AssistProfile, DamageMode, DamageSim, EntityId, FfbOutput, LoadedCar, SimEvent, TimeOfDay,
    VehicleInput, VehicleSim, VehicleState, CONTRACT_VERSION, GRAVITY_MPS2, PHYSICS_DT,
};
use wp_math::FixedStep;
use wp_sim::{Sim, SimConfig};

use crate::data;
use crate::replay::{Op, Replay, SessionHeader, TickOp, HASH_INTERVAL_TICKS};
use crate::scenario::{evaluate, CheckResult, InputScript, Scenario, VehicleMetrics};

/// At most this many ticks run per `step()` call (0.25 s of sim time).
pub const MAX_TICKS_PER_STEP: u32 = 60;

/// Event buffer cap; oldest events are dropped if the renderer stops draining.
const MAX_BUFFERED_EVENTS: usize = 4096;

/// Per-vehicle bookkeeping the sim does not keep.
#[derive(Debug, Clone)]
pub struct VehicleExtra {
    /// Resolved car data (for static info, lattice topology, tire estimates).
    pub car: LoadedCar,
    /// Spawn position (for distance metrics).
    pub spawn_pos: [f32; 3],
    /// Chassis-space acceleration over the last tick `[x, y, z]`, m/s² (gravity excluded).
    pub accel_local: [f32; 3],
    /// State before the last tick of the latest `step()` (render interpolation source).
    pub prev_state: Option<VehicleState>,
    /// Scenario input script, if scripted.
    pub script: Option<InputScript>,
    /// Scenario metrics.
    pub metrics: VehicleMetrics,
}

/// Replay playback state.
#[derive(Debug, Clone)]
pub struct Playback {
    /// The replay being played.
    pub replay: Replay,
    /// Index of the next op to apply.
    pub cursor: usize,
    /// Playback speed multiplier (0 = paused).
    pub speed: f32,
    /// Stored hashes that matched during re-simulation.
    pub verified: u32,
    /// Ticks whose hash did not match.
    pub drift_ticks: Vec<u64>,
    /// Stored hashes by tick.
    hashes: BTreeMap<u64, u64>,
}

/// Scenario progress.
#[derive(Debug, Clone)]
struct ScenarioRun {
    scenario: Scenario,
    end_tick: u64,
    done: bool,
}

/// One simulation session (live or replay).
pub struct Session {
    header: SessionHeader,
    sim: Sim,
    fixed: FixedStep,
    extras: BTreeMap<EntityId, VehicleExtra>,
    log: Vec<TickOp>,
    hashes: Vec<(u64, u64)>,
    last_input: BTreeMap<EntityId, VehicleInput>,
    events: Vec<SimEvent>,
    record_start: Option<u64>,
    playback: Option<Playback>,
    scenario: Option<ScenarioRun>,
}

fn sim_for(header: &SessionHeader) -> Result<Sim, String> {
    Ok(Sim::new(SimConfig {
        seed: header.seed,
        mode: data::mode_table(header.mode)?,
        time_of_day: TimeOfDay::from_hours(header.time_of_day_h),
        time_ratio: header.time_ratio,
    }))
}

/// Rotate a world vector into chassis space with the inverse of unit quaternion `q` `[x,y,z,w]`.
pub fn world_to_local(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let (qx, qy, qz, qw) = (-q[0], -q[1], -q[2], q[3]);
    // v' = v + 2w(u×v) + 2u×(u×v), u = (qx,qy,qz)
    let c1 = [
        qy * v[2] - qz * v[1],
        qz * v[0] - qx * v[2],
        qx * v[1] - qy * v[0],
    ];
    let c2 = [
        qy * c1[2] - qz * c1[1],
        qz * c1[0] - qx * c1[2],
        qx * c1[1] - qy * c1[0],
    ];
    [
        v[0] + 2.0 * (qw * c1[0] + c2[0]),
        v[1] + 2.0 * (qw * c1[1] + c2[1]),
        v[2] + 2.0 * (qw * c1[2] + c2[2]),
    ]
}

impl Session {
    fn empty(header: SessionHeader) -> Result<Self, String> {
        Ok(Self {
            sim: sim_for(&header)?,
            header,
            fixed: FixedStep::new(PHYSICS_DT, MAX_TICKS_PER_STEP),
            extras: BTreeMap::new(),
            log: Vec::new(),
            hashes: Vec::new(),
            last_input: BTreeMap::new(),
            events: Vec::new(),
            record_start: None,
            playback: None,
            scenario: None,
        })
    }

    /// New live session from a scenario. `seed` overrides the scenario seed when non-zero.
    /// Spawns vehicle 1 at the origin (facing +Y) plus `others`, and installs scripts when the
    /// scenario is scripted.
    pub fn from_scenario(seed: u64, mut sc: Scenario) -> Result<Self, String> {
        if seed != 0 {
            sc.seed = seed;
        }
        let mut s = Self::empty(SessionHeader {
            contract_version: CONTRACT_VERSION.to_string(),
            seed: sc.seed,
            mode: sc.mode,
            time_of_day_h: sc.time_of_day_h,
            time_ratio: 10.0,
            scenario: sc.name.clone(),
        })?;
        let first = s.spawn(&sc.car, [0.0; 3], 0.0)?;
        let mut scripted = vec![(first, sc.inputs.clone(), sc.cruise_kmh)];
        for o in &sc.others {
            let id = s.spawn(&o.car, o.pos, o.yaw)?;
            scripted.push((id, o.inputs.clone(), o.cruise_kmh));
        }
        if sc.is_scripted() {
            for (id, keys, cruise) in scripted {
                if let Some(x) = s.extras.get_mut(&id) {
                    x.script = Some(InputScript {
                        keys,
                        cruise_mps: cruise.map(|k| k / 3.6),
                    });
                }
            }
            s.scenario = Some(ScenarioRun {
                end_tick: (sc.duration_s / PHYSICS_DT).round() as u64,
                scenario: sc,
                done: false,
            });
        }
        Ok(s)
    }

    /// New playback session for a replay; fast-forwards to `view_start_tick`.
    pub fn from_replay(replay: Replay) -> Result<Self, String> {
        let mut s = Self::empty(replay.header.clone())?;
        let start = replay.view_start_tick;
        s.playback = Some(Playback {
            hashes: replay.hashes.iter().copied().collect(),
            replay,
            cursor: 0,
            speed: 1.0,
            verified: 0,
            drift_ticks: Vec::new(),
        });
        s.apply_due_ops();
        s.run_ticks(start);
        Ok(s)
    }

    /// Session header (seed, mode, clock).
    pub fn header(&self) -> &SessionHeader {
        &self.header
    }

    /// The simulation.
    pub fn sim(&self) -> &Sim {
        &self.sim
    }

    /// Per-vehicle extras.
    pub fn extra(&self, id: EntityId) -> Option<&VehicleExtra> {
        self.extras.get(&id)
    }

    /// Entity ids in ascending order.
    pub fn entities(&self) -> Vec<EntityId> {
        self.sim.vehicles().iter().map(|v| v.id).collect()
    }

    /// Whether this session is playing a replay.
    pub fn playback(&self) -> Option<&Playback> {
        self.playback.as_ref()
    }

    /// Current tick.
    pub fn tick(&self) -> u64 {
        self.sim.tick()
    }

    // ----- ops ---------------------------------------------------------------------------

    fn apply(&mut self, op: &Op) -> Result<(), String> {
        match op {
            Op::Spawn {
                entity,
                car_id,
                pos,
                yaw,
            } => {
                let car = data::load_car(car_id)?;
                let id = self
                    .sim
                    .spawn_vehicle(&car, *pos, *yaw)
                    .map_err(|e| e.to_string())?;
                if id.0 != *entity {
                    return Err(format!("replay spawn allocated {id:?}, log says {entity}"));
                }
                self.extras.insert(
                    id,
                    VehicleExtra {
                        car,
                        spawn_pos: *pos,
                        accel_local: [0.0; 3],
                        prev_state: None,
                        script: None,
                        metrics: VehicleMetrics::new(),
                    },
                );
            }
            Op::Input { entity, input } => {
                let id = EntityId(*entity);
                self.sim.set_input(id, *input).map_err(|e| e.to_string())?;
                self.last_input.insert(id, *input);
            }
            Op::Reset { entity } => {
                self.sim
                    .reset_vehicle(EntityId(*entity))
                    .map_err(|e| e.to_string())?;
            }
            Op::Assists { entity, profile } => {
                self.sim
                    .set_assists(EntityId(*entity), *profile)
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    /// Stamp `op` with the current tick, apply it and log it. Refused during playback.
    pub fn push_op(&mut self, op: Op) -> Result<(), String> {
        if self.playback.is_some() {
            return Err("session is playing a replay; live control is disabled".into());
        }
        self.apply(&op)?;
        self.log.push(TickOp {
            tick: self.sim.tick(),
            op,
        });
        Ok(())
    }

    /// Spawn a car (logged). Returns the new entity.
    pub fn spawn(&mut self, car_id: &str, pos: [f32; 3], yaw: f32) -> Result<EntityId, String> {
        let entity = EntityId(
            self.sim
                .vehicles()
                .iter()
                .map(|v| v.id.0)
                .max()
                .unwrap_or(0)
                + 1,
        );
        self.push_op(Op::Spawn {
            entity: entity.0,
            car_id: car_id.to_string(),
            pos,
            yaw,
        })?;
        Ok(entity)
    }

    /// Set live input for a vehicle. Ignored (returns `Ok(false)`) for script-driven vehicles
    /// while their scenario runs, and when unchanged.
    pub fn set_input(&mut self, id: EntityId, input: VehicleInput) -> Result<bool, String> {
        if self.extras.get(&id).is_some_and(|x| x.script.is_some()) {
            return Ok(false);
        }
        if !self.extras.contains_key(&id) {
            return Err(format!("unknown entity {id:?}"));
        }
        if self.last_input.get(&id) == Some(&input) {
            return Ok(false);
        }
        self.push_op(Op::Input {
            entity: id.0,
            input,
        })?;
        Ok(true)
    }

    /// Reset a vehicle to its spawn pose (logged).
    pub fn reset_vehicle(&mut self, id: EntityId) -> Result<(), String> {
        self.push_op(Op::Reset { entity: id.0 })
    }

    /// Set a vehicle's assists (logged).
    pub fn set_assists(&mut self, id: EntityId, profile: AssistProfile) -> Result<(), String> {
        self.push_op(Op::Assists {
            entity: id.0,
            profile,
        })
    }

    // ----- stepping ----------------------------------------------------------------------

    /// Advance by `frame_dt` seconds of wall time (×playback speed in replays). Runs whole
    /// ticks and returns the interpolation alpha `0..1`.
    pub fn step(&mut self, frame_dt: f32) -> f32 {
        let scale = self.playback.as_ref().map_or(1.0, |p| p.speed);
        let mut ticks = self.fixed.advance(frame_dt * scale) as u64;
        if let Some(p) = &self.playback {
            ticks = ticks.min(p.replay.end_tick.saturating_sub(self.sim.tick()));
        }
        self.run_ticks(ticks);
        if self.at_replay_end() {
            return 1.0;
        }
        self.fixed.alpha()
    }

    fn at_replay_end(&self) -> bool {
        self.playback
            .as_ref()
            .is_some_and(|p| self.sim.tick() >= p.replay.end_tick)
    }

    /// Run exactly `n` ticks (stops at the end of a replay).
    pub fn run_ticks(&mut self, n: u64) {
        for i in 0..n {
            if self.at_replay_end() {
                break;
            }
            let last = i + 1 == n;
            self.run_tick(last);
        }
    }

    fn run_tick(&mut self, capture_prev: bool) {
        let tick = self.sim.tick();
        // 1. Ops for this tick: from the replay log, or from scenario scripts.
        if self.playback.is_some() {
            self.apply_due_ops();
        } else {
            self.apply_scripts(tick);
        }

        // 2. Snapshot pre-tick state for interpolation and acceleration.
        let before: Vec<(EntityId, VehicleState)> = self.sim.vehicle_states();
        if capture_prev {
            for (id, s) in &before {
                if let Some(x) = self.extras.get_mut(id) {
                    x.prev_state = Some(s.clone());
                }
            }
        }

        // 3. Step.
        self.sim.step_fixed();
        let new_events = self.sim.drain_events();
        self.events.extend(new_events);
        if self.events.len() > MAX_BUFFERED_EVENTS {
            let excess = self.events.len() - MAX_BUFFERED_EVENTS;
            self.events.drain(..excess);
        }

        // 4. Derived per-vehicle data.
        let after = self.sim.vehicle_states();
        for (id, s) in &after {
            let Some(x) = self.extras.get_mut(id) else {
                continue;
            };
            let prev_vel = before
                .iter()
                .find(|(b, _)| b == id)
                .map_or(s.lin_vel, |(_, b)| b.lin_vel);
            let a_world = [
                (s.lin_vel[0] - prev_vel[0]) / PHYSICS_DT,
                (s.lin_vel[1] - prev_vel[1]) / PHYSICS_DT,
                (s.lin_vel[2] - prev_vel[2]) / PHYSICS_DT,
            ];
            x.accel_local = world_to_local(s.rot, a_world);
            if self.scenario.as_ref().is_some_and(|r| !r.done) {
                let decel_g = (-x.accel_local[1] * s.speed_mps.signum()) / GRAVITY_MPS2;
                x.metrics.update(s, x.spawn_pos, decel_g);
            }
        }

        // 5. Edge-triggered inputs (gear up/down) last exactly one tick (live sessions only;
        //    the clearing op is logged, so playback reproduces it).
        if self.playback.is_none() {
            let edges: Vec<(EntityId, VehicleInput)> = self
                .last_input
                .iter()
                .filter(|(_, i)| i.gear_up || i.gear_down)
                .map(|(id, i)| {
                    (
                        *id,
                        VehicleInput {
                            gear_up: false,
                            gear_down: false,
                            ..*i
                        },
                    )
                })
                .collect();
            for (id, input) in edges {
                let _ = self.push_op(Op::Input {
                    entity: id.0,
                    input,
                });
            }
        }

        // 6. Scenario end.
        let now = self.sim.tick();
        if let Some(run) = &mut self.scenario {
            if !run.done && now >= run.end_tick {
                run.done = true;
                for x in self.extras.values_mut() {
                    x.script = None;
                }
            }
        }

        // 7. Hashes: stored every HASH_INTERVAL_TICKS live; checked against every stored
        //    tick (including the final one) in playback.
        match &mut self.playback {
            None => {
                if now.is_multiple_of(HASH_INTERVAL_TICKS) {
                    self.hashes.push((now, self.sim.state_hash()));
                }
            }
            Some(p) => {
                if let Some(expected) = p.hashes.get(&now) {
                    if *expected == self.sim.state_hash() {
                        p.verified += 1;
                    } else {
                        p.drift_ticks.push(now);
                    }
                }
            }
        }
        self.apply_due_ops();
    }

    /// Playback: apply every logged op stamped at or before the current tick. Ops are applied
    /// as soon as their tick is reached (so a freshly loaded replay shows its tick-0 spawns),
    /// which is equivalent to live sessions applying them on the call.
    fn apply_due_ops(&mut self) {
        let tick = self.sim.tick();
        let Some(mut p) = self.playback.take() else {
            return;
        };
        while p.cursor < p.replay.ops.len() && p.replay.ops[p.cursor].tick <= tick {
            let op = p.replay.ops[p.cursor].op.clone();
            if self.apply(&op).is_err() {
                // A broken log is drift; keep going so the UI can show where.
                p.drift_ticks.push(tick);
            }
            p.cursor += 1;
        }
        self.playback = Some(p);
    }

    fn apply_scripts(&mut self, tick: u64) {
        let t = tick as f32 * PHYSICS_DT;
        let states = self.sim.vehicle_states();
        let mut changes = Vec::new();
        for (id, s) in &states {
            if let Some(script) = self.extras.get(id).and_then(|x| x.script.as_ref()) {
                let input = script.input_at(t, s.speed_mps);
                if self.last_input.get(id) != Some(&input) {
                    changes.push((*id, input));
                }
            }
        }
        for (id, input) in changes {
            let _ = self.push_op(Op::Input {
                entity: id.0,
                input,
            });
        }
    }

    // ----- read-out ----------------------------------------------------------------------

    /// Take buffered events.
    pub fn drain_events(&mut self) -> Vec<SimEvent> {
        std::mem::take(&mut self.events)
    }

    /// Force-feedback output of a vehicle.
    pub fn ffb(&self, id: EntityId) -> Option<FfbOutput> {
        self.sim
            .vehicles()
            .iter()
            .find(|v| v.id == id)
            .map(|v| v.vehicle.ffb())
    }

    /// Current lattice node positions (chassis space) of a vehicle.
    pub fn lattice_nodes(&self, id: EntityId) -> Option<&[[f32; 3]]> {
        self.sim
            .vehicles()
            .iter()
            .find(|v| v.id == id)
            .map(|v| v.damage.node_positions())
    }

    /// Current input of a vehicle.
    pub fn input(&self, id: EntityId) -> VehicleInput {
        self.last_input.get(&id).copied().unwrap_or_default()
    }

    /// Assists of a vehicle.
    pub fn assists(&self, id: EntityId) -> Option<AssistProfile> {
        self.sim
            .vehicles()
            .iter()
            .find(|v| v.id == id)
            .map(|v| v.assists)
    }

    /// Damage mode in effect.
    pub fn mode(&self) -> DamageMode {
        self.header.mode
    }

    /// Scenario result: `None` without a scripted scenario.
    pub fn scenario_result(&self) -> Option<ScenarioResult> {
        let run = self.scenario.as_ref()?;
        let metrics: Vec<VehicleMetrics> = self.extras.values().map(|x| x.metrics).collect();
        let mut checks = evaluate(&run.scenario.expect, &metrics, false);
        checks.extend(evaluate(&run.scenario.pending_expect, &metrics, true));
        Some(ScenarioResult {
            name: run.scenario.name.clone(),
            done: run.done,
            tick: self.sim.tick(),
            end_tick: run.end_tick,
            pass: run.done && checks.iter().all(|c| c.pass || c.pending),
            metrics,
            checks,
            state_hash: self.sim.state_hash(),
        })
    }

    /// The scenario this session was built from (scripted scenarios only).
    pub fn scenario(&self) -> Option<&Scenario> {
        self.scenario.as_ref().map(|r| &r.scenario)
    }

    // ----- recording & playback ----------------------------------------------------------

    /// Start recording: marks the view start. The op log always covers the whole session.
    pub fn record_start(&mut self) {
        self.record_start = Some(self.sim.tick());
    }

    /// Whether `record_start` is active.
    pub fn recording(&self) -> bool {
        self.record_start.is_some()
    }

    /// Stop recording and return the replay (session log from tick 0, viewed from the
    /// `record_start` tick). Works without `record_start` (views from tick 0).
    pub fn record_stop(&mut self) -> Replay {
        let now = self.sim.tick();
        let mut hashes = self.hashes.clone();
        if hashes.last().map(|h| h.0) != Some(now) {
            hashes.push((now, self.sim.state_hash()));
        }
        Replay {
            header: self.header.clone(),
            ops: self.log.clone(),
            hashes,
            view_start_tick: self.record_start.take().unwrap_or(0).min(now),
            end_tick: now,
        }
    }

    /// Seek a replay to `tick` (re-simulates from 0 when seeking backwards).
    pub fn replay_seek(&mut self, tick: u64) -> Result<(), String> {
        let Some(p) = &self.playback else {
            return Err("no replay loaded".into());
        };
        let target = tick.min(p.replay.end_tick);
        if target < self.sim.tick() {
            let mut p = self.playback.take().expect("checked");
            p.cursor = 0;
            p.verified = 0;
            p.drift_ticks.clear();
            self.sim = sim_for(&self.header)?;
            self.extras.clear();
            self.last_input.clear();
            self.events.clear();
            self.playback = Some(p);
            self.apply_due_ops();
        }
        let n = target - self.sim.tick();
        self.run_ticks(n);
        Ok(())
    }

    /// Set replay playback speed (0 = pause).
    pub fn replay_play(&mut self, speed: f32) -> Result<(), String> {
        let p = self.playback.as_mut().ok_or("no replay loaded")?;
        p.speed = speed.clamp(0.0, 16.0);
        Ok(())
    }

    /// Replay progress.
    pub fn replay_status(&self) -> Option<ReplayStatus> {
        let p = self.playback.as_ref()?;
        Some(ReplayStatus {
            tick: self.sim.tick(),
            start_tick: p.replay.view_start_tick,
            end_tick: p.replay.end_tick,
            speed: p.speed,
            verified_hashes: p.verified,
            stored_hashes: p.replay.hashes.len() as u32,
            drift_ticks: p.drift_ticks.clone(),
            state_hash: format!("{:016x}", self.sim.state_hash()),
            recorded_final_hash: p.replay.final_hash().map(|h| format!("{h:016x}")),
            scenario: self.header.scenario.clone(),
            mode: self.header.mode,
        })
    }
}

/// Scenario progress / outcome (serialized to JSON for the UI).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ScenarioResult {
    /// Scenario name.
    pub name: String,
    /// Whether the scripted duration elapsed.
    pub done: bool,
    /// Current tick.
    pub tick: u64,
    /// Tick the scenario ends at.
    pub end_tick: u64,
    /// All gating checks pass (pending checks ignored). Only meaningful when `done`.
    pub pass: bool,
    /// Per-vehicle metrics (index 0 = vehicle 1).
    pub metrics: Vec<VehicleMetrics>,
    /// Gating and pending checks.
    pub checks: Vec<CheckResult>,
    /// Current state hash.
    pub state_hash: u64,
}

/// Replay progress (serialized to JSON for the UI).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ReplayStatus {
    /// Current tick.
    pub tick: u64,
    /// First viewable tick.
    pub start_tick: u64,
    /// Last tick.
    pub end_tick: u64,
    /// Playback speed.
    pub speed: f32,
    /// Stored hashes matched so far.
    pub verified_hashes: u32,
    /// Hashes stored in the replay.
    pub stored_hashes: u32,
    /// Ticks where the hash did not match (drift).
    pub drift_ticks: Vec<u64>,
    /// Current state hash (hex).
    pub state_hash: String,
    /// The replay's final recorded hash (hex).
    pub recorded_final_hash: Option<String>,
    /// Scenario name.
    pub scenario: String,
    /// Damage mode.
    pub mode: DamageMode,
}
