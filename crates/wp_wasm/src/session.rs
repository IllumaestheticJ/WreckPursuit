//! [`Session`]: the pure-Rust core behind the wasm-bindgen surface (natively testable).
//!
//! A session is a simcli scenario (format v2) being run interactively. The sim is built by
//! `simcli::run::build_sim` and scripted entities are driven by `simcli::run::driver_input`, so
//! a scripted session ticks exactly like `simcli run`. Every input change and op (reset,
//! assists, mode switch) is logged with its tick into a `simcli::replay::Replay`, together with
//! the state hash after every tick: [`Session::record_stop`] returns canonical `replay.bin`
//! bytes that `simcli replay` verifies headlessly. Playback re-simulates that log and compares
//! every tick hash.

use std::collections::BTreeMap;

use wp_contracts::{
    AssistProfile, DamageMode, EntityId, FfbOutput, LoadedCar, SimEvent, VehicleInput, VehicleSim,
    VehicleState, PHYSICS_DT,
};
use wp_math::FixedStep;
use wp_sim::Sim;

use simcli::replay::{Checkpoint, EntityCheckpoint, InputEvent, OpEvent, OpKind, Replay};
use simcli::run::{
    apply_op, build_sim, driver_input, scenario_states, world_config, BuiltSim, RunOptions,
    Selection,
};
use simcli::scenario::{CitySpec, EntitySpec};
use simcli::{RunSummary, Scenario};

use crate::data::mem_data;
use crate::run::{ContinueReport, PlayerCar, PlayerRun};
use crate::scenario::entity;
use crate::street::{Preview, StreetRacers};

thread_local! {
    /// Street racers spawn in new live city sessions ([`set_street_racers_enabled`]).
    static STREET_ENABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Spawn NPC street racers in live city sessions started from now on (default on; e2e and
/// benchmarks may turn them off).
pub fn set_street_racers_enabled(on: bool) {
    STREET_ENABLED.with(|c| c.set(on));
}

/// At most this many ticks run per `step()` call (0.25 s of sim time).
pub const MAX_TICKS_PER_STEP: u32 = 60;

/// Event buffer cap; oldest events are dropped if the renderer stops draining.
const MAX_BUFFERED_EVENTS: usize = 4096;

/// Drift ticks kept in the playback report.
const MAX_DRIFT_TICKS: usize = 64;

/// Per-entity bookkeeping the sim does not keep.
#[derive(Debug, Clone, Default)]
pub struct EntityExtra {
    /// Chassis-space acceleration over the last tick `[x, y, z]`, m/s² (gravity excluded).
    pub accel_local: [f32; 3],
    /// State before the last tick of the latest `step()` (render interpolation source).
    pub prev_state: Option<VehicleState>,
}

/// Replay playback state.
#[derive(Debug, Clone)]
pub struct Playback {
    /// The replay being played.
    pub replay: Replay,
    input_cursor: usize,
    op_cursor: usize,
    /// Playback speed multiplier (0 = paused).
    pub speed: f32,
    /// Ticks whose recorded hash matched.
    pub verified: u64,
    /// First ticks whose hash did not match (at most 64 kept).
    pub drift_ticks: Vec<u64>,
    /// Total mismatching ticks.
    pub drift_count: u64,
}

/// One simulation session (live or replay).
pub struct Session {
    sc: Scenario,
    scripted: bool,
    script_end: u64,
    load_city: bool,
    systems: Selection,
    sim: Sim,
    /// Scenario entities, in scenario order (vehicles the sim spawns itself — police units,
    /// physical traffic — are not in here; see [`Session::vehicle`]).
    ids: Vec<EntityId>,
    /// Resolved cars by car id (scenario cars and any car the sim spawned).
    cars: BTreeMap<String, LoadedCar>,
    /// Per-vehicle extras for every sim vehicle, by entity.
    extras: BTreeMap<EntityId, EntityExtra>,
    last_input: Vec<VehicleInput>,
    fixed: FixedStep,
    log: Replay,
    recording: bool,
    events: Vec<SimEvent>,
    playback: Option<Playback>,
    mode: DamageMode,
    live_end_hash: Option<u64>,
    outcome: Option<RunSummary>,
    /// The player's economy (live sessions with police; see [`crate::run`]).
    run: Option<PlayerRun>,
    /// NPC street racers (live city sessions with an economy run; see [`crate::street`]).
    street: Option<StreetRacers>,
}

fn opts(systems: Option<Selection>) -> RunOptions {
    RunOptions {
        systems,
        ..Default::default()
    }
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
    fn build(
        sc: Scenario,
        scripted: bool,
        systems: Option<Selection>,
        playback: Option<Replay>,
    ) -> Result<Self, String> {
        let load_city = sc.city != CitySpec::Flat;
        let BuiltSim {
            mut sim,
            ids,
            cars,
            selection,
            ..
        } = build_sim(&sc, mem_data(), &opts(systems)).map_err(|e| e.to_string())?;
        let log = Replay::new(&sc, &selection.to_string(), ids.len());
        let n = ids.len();
        // The player's economy: live (not replayed, not scripted) sessions with police.
        let run = match (ids.first(), &sc.entities.first()) {
            (Some(&player), Some(spec))
                if playback.is_none() && !scripted && sim.systems().police =>
            {
                cars.get(&spec.car).map(|car| {
                    let car = PlayerCar {
                        def_id: car.def.id.clone(),
                        class: car.def.class,
                        base_price: car.def.base_price,
                    };
                    let econ = crate::garage::with_economy(|e| e.clone());
                    PlayerRun::new(econ, player, &car, sc.mode, sc.seed)
                })
            }
            _ => None,
        };
        // Street racers: live city sessions with police, and their playback (the roaming AI is
        // deterministic, so a replay re-creates it; see `crate::street`).
        let street = match ids.first() {
            Some(&player)
                if load_city
                    && !scripted
                    && sim.systems().police
                    && STREET_ENABLED.with(|c| c.get()) =>
            {
                let at = sim
                    .vehicles()
                    .iter()
                    .find(|v| v.id == player)
                    .map_or([0.0; 2], |v| {
                        let p = v.vehicle.state().pos;
                        [p[0], p[1]]
                    });
                StreetRacers::spawn(&mut sim, at, sc.seed)
            }
            _ => None,
        };
        let mut session = Self {
            run,
            street,
            script_end: if scripted { sc.ticks() } else { 0 },
            mode: sc.mode,
            sc,
            scripted,
            load_city,
            systems: selection,
            sim,
            ids,
            cars,
            extras: BTreeMap::new(),
            last_input: vec![VehicleInput::default(); n],
            fixed: FixedStep::new(PHYSICS_DT, MAX_TICKS_PER_STEP),
            log,
            recording: false,
            events: Vec::new(),
            playback: playback.map(|replay| Playback {
                replay,
                input_cursor: 0,
                op_cursor: 0,
                speed: 1.0,
                verified: 0,
                drift_ticks: Vec::new(),
                drift_count: 0,
            }),
            live_end_hash: None,
            outcome: None,
        };
        // Street racers' car models (the renderer asks for them before the first tick).
        session.sync_cars();
        Ok(session)
    }

    /// Live session. `scripted` = drive every entity by its scenario driver until
    /// `duration_s` (menu scenarios); otherwise every entity is live-controlled (free drive).
    /// A free-drive city session spawns the first entity on the city's race start grid.
    pub fn new(mut sc: Scenario, scripted: bool) -> Result<Self, String> {
        sc.validate()?;
        if !scripted && sc.city != CitySpec::Flat {
            let city = simcli::data::load_world(mem_data(), &sc.city, world_config(&sc))
                .map_err(|e| e.to_string())?;
            if let Some(world) = city {
                let grid = world
                    .network()
                    .pois
                    .iter()
                    .find(|p| p.kind == wp_world::network::PoiKind::RaceStartGrid);
                if let (Some(p), Some(e)) = (grid, sc.entities.first_mut()) {
                    e.pos = p.pos;
                    e.yaw = p.heading_rad;
                }
            }
        }
        Self::build(sc, scripted, None, None)
    }

    /// Playback session for `replay.bin` bytes.
    pub fn from_replay_bytes(bytes: &[u8]) -> Result<Self, String> {
        let replay = Replay::decode(bytes).map_err(|e| e.to_string())?;
        Self::from_replay(replay)
    }

    /// Playback session for a decoded replay.
    pub fn from_replay(replay: Replay) -> Result<Self, String> {
        let sc = replay.scenario().map_err(|e| e.to_string())?;
        let systems = Selection::parse(&replay.systems)?;
        Self::build(sc, false, Some(systems), Some(replay))
    }

    /// The scenario this session runs (entities include tick-0 spawns).
    pub fn scenario(&self) -> &Scenario {
        &self.sc
    }

    /// Whether entities are script-driven (menu scenario).
    pub fn scripted(&self) -> bool {
        self.scripted
    }

    /// The simulation.
    pub fn sim(&self) -> &Sim {
        &self.sim
    }

    /// Entity ids in scenario order.
    pub fn entities(&self) -> &[EntityId] {
        &self.ids
    }

    /// Scenario index of an entity.
    pub fn index(&self, id: EntityId) -> Option<usize> {
        self.ids.iter().position(|x| *x == id)
    }

    /// Scenario entity spec of an entity.
    pub fn spec(&self, id: EntityId) -> Option<&EntitySpec> {
        self.index(id).map(|i| &self.sc.entities[i])
    }

    /// A sim vehicle by entity (scenario entities and sim-spawned police / traffic).
    pub fn vehicle(&self, id: EntityId) -> Option<&wp_sim::SimVehicle> {
        self.sim.vehicles().iter().find(|v| v.id == id)
    }

    /// Resolved car of any sim vehicle.
    pub fn car(&self, id: EntityId) -> Option<&LoadedCar> {
        self.vehicle(id).and_then(|v| self.cars.get(&v.car_id))
    }

    /// Per-vehicle extras.
    pub fn extra(&self, id: EntityId) -> Option<&EntityExtra> {
        self.extras.get(&id)
    }

    /// Load the car of every sim vehicle not seen before (the sim may spawn police units and
    /// physical traffic on its own).
    fn sync_cars(&mut self) {
        for v in self.sim.vehicles() {
            if !self.cars.contains_key(&v.car_id) {
                if let Ok(car) = simcli::data::load_car(mem_data(), &v.car_id) {
                    self.cars.insert(v.car_id.clone(), car);
                }
            }
        }
    }

    /// Playback state, if this is a replay.
    pub fn playback(&self) -> Option<&Playback> {
        self.playback.as_ref()
    }

    /// Current tick.
    pub fn tick(&self) -> u64 {
        self.sim.tick()
    }

    /// Damage mode in effect.
    pub fn mode(&self) -> DamageMode {
        self.mode
    }

    /// Whether the scenario's city was loaded.
    pub fn has_city(&self) -> bool {
        self.load_city
    }

    // ----- live control ------------------------------------------------------------------

    fn live_check(&self) -> Result<(), String> {
        if self.playback.is_some() {
            return Err("session is playing a replay; live control is disabled".into());
        }
        Ok(())
    }

    fn idx(&self, id: EntityId) -> Result<usize, String> {
        self.index(id)
            .ok_or_else(|| format!("unknown entity {id:?}"))
    }

    fn log_input(&mut self, idx: usize, input: VehicleInput) {
        if self.last_input[idx] == input {
            return;
        }
        // `set_input` cannot fail for an id from `ids`.
        let _ = self.sim.set_input(self.ids[idx], input);
        self.last_input[idx] = input;
        self.log.inputs.push(InputEvent {
            tick: self.sim.tick(),
            entity: idx as u16,
            input,
        });
    }

    fn log_op(&mut self, idx: usize, op: OpKind) -> Result<(), String> {
        let ev = OpEvent {
            tick: self.sim.tick(),
            entity: idx as u16,
            op,
        };
        apply_op(&mut self.sim, &self.ids, mem_data(), &ev).map_err(|e| e.to_string())?;
        self.log.ops.push(ev);
        Ok(())
    }

    /// Spawn a car. Only before the first tick: the spawn becomes part of the scenario setup
    /// (entity id `car<N>`, scenario-default assists), so replays stay canonical.
    pub fn spawn(&mut self, car_id: &str, pos: [f32; 3], yaw: f32) -> Result<EntityId, String> {
        self.live_check()?;
        if self.sim.tick() != 0 {
            return Err("vehicles can only be spawned before the first tick".into());
        }
        let car = simcli::data::load_car(mem_data(), car_id).map_err(|e| e.to_string())?;
        let id = self
            .sim
            .spawn_vehicle(&car, pos, yaw)
            .map_err(|e| e.to_string())?;
        self.sim
            .set_assists(id, self.sc.assists.profile())
            .map_err(|e| e.to_string())?;
        let mut n = self.ids.len() + 1;
        while self.sc.entities.iter().any(|e| e.id == format!("car{n}")) {
            n += 1;
        }
        self.sc
            .entities
            .push(entity(&format!("car{n}"), car_id, pos, yaw));
        self.ids.push(id);
        self.cars.insert(car_id.to_string(), car);
        self.last_input.push(VehicleInput::default());
        self.log.entities = self.ids.len() as u32;
        Ok(id)
    }

    /// Set live input. Ignored (`Ok(false)`) while a scripted scenario is running, and when
    /// unchanged. `gear_up`/`gear_down` are one-tick edges (cleared after the next tick).
    pub fn set_input(&mut self, id: EntityId, input: VehicleInput) -> Result<bool, String> {
        self.live_check()?;
        let idx = self.idx(id)?;
        if self.script_running() || self.last_input[idx] == input || self.held(id) {
            return Ok(false);
        }
        self.log_input(idx, input);
        Ok(true)
    }

    fn script_running(&self) -> bool {
        self.scripted && self.sim.tick() < self.script_end
    }

    /// Reset a vehicle to its spawn pose, pristine (logged op).
    pub fn reset_vehicle(&mut self, id: EntityId) -> Result<(), String> {
        self.live_check()?;
        let idx = self.idx(id)?;
        self.log_op(idx, OpKind::Reset)
    }

    /// Set a vehicle's assists (logged op).
    pub fn set_assists(&mut self, id: EntityId, profile: AssistProfile) -> Result<(), String> {
        self.live_check()?;
        let idx = self.idx(id)?;
        self.log_op(idx, OpKind::Assists(profile))
    }

    /// Switch the damage mode without restarting (logged op; existing damage is kept).
    pub fn set_mode(&mut self, mode: DamageMode) -> Result<(), String> {
        self.live_check()?;
        self.log_op(0, OpKind::Mode(mode))?;
        self.mode = mode;
        if let Some(r) = &mut self.run {
            r.set_mode(mode);
        }
        Ok(())
    }

    // ----- end of run (lead; see `crate::run`) ------------------------------------------

    /// The player's economy run, if this session has one.
    pub fn run(&self) -> Option<&PlayerRun> {
        self.run.as_ref()
    }

    /// Take the run (its account carries over to the next session).
    pub fn take_run(&mut self) -> Option<PlayerRun> {
        self.run.take()
    }

    /// Continue the account of a previous session's run (restart): cash and the ledger carry
    /// over; this session's car is bound to it.
    pub fn adopt_account(&mut self, previous: PlayerRun) {
        let Some(run) = &self.run else {
            return;
        };
        let player = run.player();
        let Some(car) = self.car(player).map(|c| PlayerCar {
            def_id: c.def.id.clone(),
            class: c.def.class,
            base_price: c.def.base_price,
        }) else {
            return;
        };
        let (acct, ledger) = previous.into_account();
        let econ = crate::garage::with_economy(|e| e.clone());
        self.run = Some(PlayerRun::with_account(
            econ,
            acct,
            ledger,
            player,
            &car,
            self.mode,
            self.sc.seed,
        ));
    }

    /// Mutable sim for debug tools (`debug_set_heat`) and tests. Live sessions only; changes
    /// made here are not in the replay log.
    pub fn debug_sim_mut(&mut self) -> Result<&mut Sim, String> {
        self.live_check()?;
        Ok(&mut self.sim)
    }

    /// Debug / e2e: park vehicle `id` (stopped) `ahead_m` in front of the first police car, in
    /// its lane. Returns the police car's entity, or an error when there is none yet. Not in
    /// the replay log.
    pub fn debug_park_near_police(
        &mut self,
        id: EntityId,
        ahead_m: f32,
    ) -> Result<EntityId, String> {
        self.live_check()?;
        let cop = self
            .sim
            .vehicles()
            .iter()
            .find(|v| v.kind == wp_sim::VehicleKind::Police)
            .map(|v| (v.id, v.vehicle.state()))
            .ok_or("no police car yet")?;
        let (cop, st) = (cop.0, cop.1);
        let q = st.rot;
        // Chassis +Y in world (XY), then yaw about +Z with 0 = +Y.
        let fx = 2.0 * (q[0] * q[1] - q[3] * q[2]);
        let fy = 1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]);
        let yaw = wp_math::atan2(-fx, fy);
        let (sn, cs) = wp_math::sin_cos(yaw);
        let xy = [st.pos[0] - sn * ahead_m, st.pos[1] + cs * ahead_m];
        self.sim
            .place_on_ground(id, xy, yaw, 0.0, Some(st.pos[2]))
            .map_err(|e| e.to_string())?;
        Ok(cop)
    }

    /// The vehicle is held on the brakes because its run is over (capture or wreck).
    pub fn held(&self, id: EntityId) -> bool {
        self.run.as_ref().is_some_and(|r| {
            r.player() == id
                && (r.is_over()
                    || self
                        .street
                        .as_ref()
                        .is_some_and(|s| s.holds_player(self.sim.tick())))
        })
    }

    // ----- street racers (see `crate::street`) -------------------------------------------

    /// The session's street racers, if any.
    pub fn street(&self) -> Option<&StreetRacers> {
        self.street.as_ref()
    }

    /// Street racers, mutable (tests).
    #[cfg(test)]
    pub(crate) fn street_mut_for_tests(&mut self) -> &mut StreetRacers {
        self.street.as_mut().expect("street racers")
    }

    /// Plan a street race from the player to `finish` (the open challenge's B).
    pub fn street_preview(&self, finish: [f32; 2]) -> Result<Preview, String> {
        let (street, run) = self
            .street
            .as_ref()
            .zip(self.run.as_ref())
            .ok_or("no street racers in this session")?;
        street.preview(&self.sim, run.player(), finish)
    }

    /// Accept the open challenge: race to `finish` for `stake` dollars.
    pub fn street_accept(&mut self, finish: [f32; 2], stake: i64) -> Result<(), String> {
        self.live_check()?;
        let (street, run) = self
            .street
            .as_mut()
            .zip(self.run.as_mut())
            .ok_or("no street racers in this session")?;
        street.accept(&mut self.sim, run, finish, stake)
    }

    /// Decline the open challenge.
    pub fn street_decline(&mut self) {
        let tick = self.sim.tick();
        if let Some(s) = &mut self.street {
            s.decline(tick);
        }
    }

    /// "Repair & continue": pay the impound fee and the repair from the ledger, repair the car
    /// in place (logged replay op) and give control back.
    pub fn repair_and_continue(&mut self) -> Result<ContinueReport, String> {
        self.live_check()?;
        let run = self.run.as_mut().ok_or("no economy run in this session")?;
        let player = run.player();
        let report = run.repair_and_continue()?;
        let idx = self.idx(player)?;
        self.log_op(idx, OpKind::Repair)?;
        let keep = self.last_input[idx];
        self.log_input(
            idx,
            VehicleInput {
                headlights: keep.headlights,
                ..Default::default()
            },
        );
        Ok(report)
    }

    // ----- stepping ----------------------------------------------------------------------

    /// Advance by `frame_dt` seconds of wall time (×playback speed in replays). Runs whole
    /// ticks and returns the interpolation alpha `0..1`.
    pub fn step(&mut self, frame_dt: f32) -> f32 {
        let scale = self.playback.as_ref().map_or(1.0, |p| p.speed);
        let mut ticks = self.fixed.advance(frame_dt * scale) as u64;
        if let Some(p) = &self.playback {
            ticks = ticks.min(p.replay.ticks.saturating_sub(self.sim.tick()));
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
            .is_some_and(|p| self.sim.tick() >= p.replay.ticks)
    }

    /// Run exactly `n` ticks (stops at the end of a replay).
    pub fn run_ticks(&mut self, n: u64) {
        for i in 0..n {
            if self.at_replay_end() {
                break;
            }
            self.run_tick(i + 1 == n);
        }
    }

    fn run_tick(&mut self, capture_prev: bool) {
        let tick = self.sim.tick();
        // 1. Inputs for this tick: replay track, scenario drivers, or (live) already applied.
        if let Some(mut p) = self.playback.take() {
            let r = &p.replay;
            while p.op_cursor < r.ops.len() && r.ops[p.op_cursor].tick <= tick {
                let ev = r.ops[p.op_cursor];
                if apply_op(&mut self.sim, &self.ids, mem_data(), &ev).is_err() {
                    p.drift_count += 1;
                } else if let OpKind::Mode(m) = ev.op {
                    self.mode = m;
                }
                p.op_cursor += 1;
            }
            while p.input_cursor < r.inputs.len() && r.inputs[p.input_cursor].tick <= tick {
                let ev = r.inputs[p.input_cursor];
                if let Some(id) = self.ids.get(ev.entity as usize) {
                    let _ = self.sim.set_input(*id, ev.input);
                    self.last_input[ev.entity as usize] = ev.input;
                }
                p.input_cursor += 1;
            }
            self.playback = Some(p);
        } else if let Some(idx) = self
            .run
            .as_ref()
            .filter(|r| r.is_over())
            .and_then(|r| self.index(r.player()))
        {
            // The run is over: the player's car brakes to a halt and stays there.
            let keep = self.last_input[idx];
            self.log_input(
                idx,
                VehicleInput {
                    brake: 1.0,
                    handbrake: 1.0,
                    headlights: keep.headlights,
                    ..Default::default()
                },
            );
        } else if let Some(idx) = self
            .street
            .as_ref()
            .filter(|s| s.holds_player(tick))
            .and(self.run.as_ref())
            .and_then(|r| self.index(r.player()))
        {
            // Street race countdown: the player waits on the brakes for the green light.
            let keep = self.last_input[idx];
            self.log_input(
                idx,
                VehicleInput {
                    brake: 1.0,
                    headlights: keep.headlights,
                    ..Default::default()
                },
            );
        } else if self.script_running() {
            // Scenario entities only: city runs add police / traffic cars (not scripted).
            let states = scenario_states(&self.sim, &self.ids);
            for (idx, (_, state)) in states.iter().enumerate() {
                let input = driver_input(&self.sc, idx, tick, state, &self.last_input[idx]);
                self.log_input(idx, input);
            }
        }

        // Street racers drive themselves (inputs straight into the sim, not the replay log: the
        // playback re-runs the same AI from the same state and the logged horn).
        if let (Some(street), Some(&player)) = (&mut self.street, self.ids.first()) {
            let horn = self.last_input[0].horn;
            let over = self.run.as_ref().is_some_and(|r| r.is_over());
            street.before_step(&mut self.sim, player, horn, over);
        }

        // 2. Step, keeping the pre-tick state for interpolation and acceleration.
        // All sim vehicles (city police / traffic cars are drawn and interpolated too);
        // scripting and replay checkpoints use scenario entities only.
        let before = self.sim.vehicle_states();
        if capture_prev {
            for (id, s) in &before {
                self.extras.entry(*id).or_default().prev_state = Some(s.clone());
            }
        }
        self.sim.step_fixed();
        self.sync_cars();
        let drained = self.sim.drain_events();
        self.observe_run(&drained);
        if let Some(street) = &mut self.street {
            street.after_step(&self.sim, &drained, self.run.as_mut());
        }
        self.events.extend(drained);
        if self.events.len() > MAX_BUFFERED_EVENTS {
            let excess = self.events.len() - MAX_BUFFERED_EVENTS;
            self.events.drain(..excess);
        }
        let after = self.sim.vehicle_states();
        let before: BTreeMap<EntityId, VehicleState> = before.into_iter().collect();
        self.extras
            .retain(|id, _| after.iter().any(|(a, _)| a == id));
        for (id, s) in &after {
            let Some(b) = before.get(id) else {
                continue;
            };
            let x = self.extras.entry(*id).or_default();
            let a = [
                (s.lin_vel[0] - b.lin_vel[0]) / PHYSICS_DT,
                (s.lin_vel[1] - b.lin_vel[1]) / PHYSICS_DT,
                (s.lin_vel[2] - b.lin_vel[2]) / PHYSICS_DT,
            ];
            x.accel_local = world_to_local(s.rot, a);
        }

        // 3. Hashes: logged every tick live (replay.bin stores one per tick), compared in
        //    playback.
        let now = self.sim.tick();
        let hash = self.sim.state_hash();
        match &mut self.playback {
            Some(p) => match p.replay.tick_hashes.get(now as usize - 1) {
                Some(h) if *h == hash => p.verified += 1,
                Some(_) => {
                    p.drift_count += 1;
                    if p.drift_ticks.len() < MAX_DRIFT_TICKS {
                        p.drift_ticks.push(now);
                    }
                }
                None => {}
            },
            None => {
                self.log.tick_hashes.push(hash);
                if now.is_multiple_of(self.sc.hash_every) {
                    self.log.checkpoints.push(self.checkpoint());
                }
                if self.scripted && now == self.script_end {
                    self.live_end_hash = Some(hash);
                }
                // Live edge inputs (gear up/down) last exactly one tick.
                if !self.script_running() {
                    for idx in 0..self.ids.len() {
                        let i = self.last_input[idx];
                        if i.gear_up || i.gear_down {
                            self.log_input(
                                idx,
                                VehicleInput {
                                    gear_up: false,
                                    gear_down: false,
                                    ..i
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    /// Feed one tick's events to the player's economy (read-only on the sim) and settle a
    /// chase that just ended.
    fn observe_run(&mut self, events: &[SimEvent]) {
        let Some(run) = &mut self.run else {
            return;
        };
        let tick = self.sim.tick();
        let Some(outcome) = run.observe(tick, events, self.sim.police().phase()) else {
            return;
        };
        let player = run.player();
        let damage = self
            .sim
            .vehicles()
            .iter()
            .find(|v| v.id == player)
            .map(|v| {
                (
                    &v.damage as &dyn wp_contracts::DamageSim,
                    self.sim.mode_table(),
                )
            });
        // A refused settlement leaves the account unchanged; the run simply goes on.
        let _ = run.settle(tick, outcome, damage);
    }

    fn checkpoint(&self) -> Checkpoint {
        // Replay checkpoints cover the scenario's own entities (the replay format's entity list).
        let states = scenario_states(&self.sim, &self.ids);
        let hashes: Vec<_> = self
            .sim
            .entity_hashes()
            .into_iter()
            .filter(|(id, _)| self.ids.contains(id))
            .collect();
        Checkpoint {
            tick: self.sim.tick(),
            entities: states
                .iter()
                .zip(&hashes)
                .map(|((_, s), (_, hash))| EntityCheckpoint {
                    hash: *hash,
                    pos: s.pos,
                    lin_vel: s.lin_vel,
                    health: s.health,
                })
                .collect(),
        }
    }

    // ----- read-out ----------------------------------------------------------------------

    /// Take buffered events.
    pub fn drain_events(&mut self) -> Vec<SimEvent> {
        std::mem::take(&mut self.events)
    }

    /// Force-feedback output of a vehicle.
    pub fn ffb(&self, id: EntityId) -> Option<FfbOutput> {
        self.vehicle(id).map(|v| v.vehicle.ffb())
    }

    /// Input a vehicle is driving with (live, scripted, replayed or AI).
    pub fn input(&self, id: EntityId) -> VehicleInput {
        self.vehicle(id).map(|v| v.input).unwrap_or_default()
    }

    /// Assists of a vehicle.
    pub fn assists(&self, id: EntityId) -> Option<AssistProfile> {
        self.vehicle(id).map(|v| v.assists)
    }

    // ----- scenario outcome --------------------------------------------------------------

    /// Scripted-scenario progress / outcome; `None` for free drive and replays. Once the
    /// scripted duration has elapsed, the scenario is re-run headless through `simcli::run`
    /// (once, cached) for its full expectation report, and the live end-of-script hash is
    /// compared with simcli's final hash.
    pub fn scenario_result(&mut self) -> Option<ScenarioResult> {
        if !self.scripted || self.playback.is_some() {
            return None;
        }
        let done = self.sim.tick() >= self.script_end;
        if done && self.outcome.is_none() {
            self.outcome = simcli::run(&self.sc, mem_data(), &opts(Some(self.systems)))
                .ok()
                .map(|o| o.summary);
        }
        let live_hash = self.live_end_hash.map(|h| format!("{h:016x}"));
        Some(ScenarioResult {
            name: self.sc.name.clone(),
            done,
            tick: self.sim.tick(),
            end_tick: self.script_end,
            pass: self.outcome.as_ref().is_some_and(RunSummary::passed),
            matches_simcli: match (&self.outcome, &live_hash) {
                (Some(o), Some(h)) => Some(&o.final_hash == h),
                _ => None,
            },
            live_hash,
            summary: self.outcome.clone(),
        })
    }

    // ----- recording & playback ----------------------------------------------------------

    /// Mark recording as active (the log always covers the whole session from tick 0).
    pub fn record_start(&mut self) {
        self.recording = true;
    }

    /// Whether `record_start` is active.
    pub fn recording(&self) -> bool {
        self.recording
    }

    /// Stop recording and return the session as a canonical `replay.bin` (whole session from
    /// tick 0; the header scenario's duration is the recorded length).
    pub fn record_stop(&mut self) -> Result<Replay, String> {
        self.recording = false;
        let now = self.sim.tick();
        if now == 0 {
            return Err("nothing recorded yet (no ticks run)".into());
        }
        let mut sc = self.sc.clone();
        sc.duration_s = now as f32 * PHYSICS_DT;
        let mut r = self.log.clone();
        r.scenario_ron = sc.to_ron();
        r.ticks = now;
        r.entities = self.ids.len() as u32;
        if r.checkpoints.last().map(|c| c.tick) != Some(now) {
            r.checkpoints.push(self.checkpoint());
        }
        Ok(r)
    }

    /// Seek a replay to `tick` (re-simulates from 0 when seeking backwards).
    pub fn replay_seek(&mut self, tick: u64) -> Result<(), String> {
        let p = self.playback.as_ref().ok_or("no replay loaded")?;
        let target = tick.min(p.replay.ticks);
        if target < self.sim.tick() {
            let speed = p.speed;
            let replay = p.replay.clone();
            *self = Self::from_replay(replay)?;
            if let Some(p) = &mut self.playback {
                p.speed = speed;
            }
        }
        let n = target - self.sim.tick();
        self.run_ticks(n);
        Ok(())
    }

    /// Set replay playback speed (0 = pause, max 16).
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
            end_tick: p.replay.ticks,
            speed: p.speed,
            verified_ticks: p.verified,
            drift_count: p.drift_count,
            drift_ticks: p.drift_ticks.clone(),
            state_hash: format!("{:016x}", self.sim.state_hash()),
            recorded_final_hash: p.replay.tick_hashes.last().map(|h| format!("{h:016x}")),
            scenario: self.sc.name.clone(),
            mode: self.mode,
            contract_version: p.replay.contract_version.clone(),
            hash_spec: p.replay.hash_spec,
        })
    }
}

/// Scripted-scenario progress / outcome (serialized to JSON for the UI).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScenarioResult {
    /// Scenario name.
    pub name: String,
    /// Whether the scripted duration elapsed.
    pub done: bool,
    /// Current tick.
    pub tick: u64,
    /// Tick the script ends at.
    pub end_tick: u64,
    /// simcli's verdict (all `expect` pass). Only meaningful when `done`.
    pub pass: bool,
    /// Live state hash at `end_tick`.
    pub live_hash: Option<String>,
    /// Whether the live run reached the same final hash as the headless simcli run.
    pub matches_simcli: Option<bool>,
    /// The headless simcli run summary (expectations, pending, metrics), once done.
    pub summary: Option<RunSummary>,
}

/// Replay progress (serialized to JSON for the UI).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ReplayStatus {
    /// Current tick.
    pub tick: u64,
    /// Last tick.
    pub end_tick: u64,
    /// Playback speed.
    pub speed: f32,
    /// Ticks whose hash matched the recording so far.
    pub verified_ticks: u64,
    /// Ticks whose hash did not match.
    pub drift_count: u64,
    /// First mismatching ticks (at most 64).
    pub drift_ticks: Vec<u64>,
    /// Current state hash (hex).
    pub state_hash: String,
    /// The replay's final recorded hash (hex).
    pub recorded_final_hash: Option<String>,
    /// Scenario name.
    pub scenario: String,
    /// Damage mode in effect.
    pub mode: DamageMode,
    /// Contract version at record time.
    pub contract_version: String,
    /// Hash spec at record time.
    pub hash_spec: u32,
}
