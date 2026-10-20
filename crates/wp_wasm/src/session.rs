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
use crate::race::RaceRun;
use crate::run::{ContinueReport, PlayerCar, PlayerRun};
use crate::scenario::entity;

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
    /// This is a career run (machine G, [`crate::career`]): the player drives a career car
    /// and the account belongs to the career.
    career: bool,
    /// A picker race (scenario `race_<id>`, live or replayed; see [`crate::race`]).
    race: Option<RaceRun>,
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
        data: &dyn simcli::data::DataSource,
    ) -> Result<Self, String> {
        let load_city = sc.city != CitySpec::Flat;
        let BuiltSim {
            mut sim,
            ids,
            cars,
            selection,
            ..
        } = build_sim(&sc, data, &opts(systems)).map_err(|e| e.to_string())?;
        let log = Replay::new(&sc, &selection.to_string(), ids.len());
        let n = ids.len();
        // The player's economy: live (not replayed, not scripted) sessions with police.
        let mut run = match (ids.first(), &sc.entities.first()) {
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
        // A picker race: judged next to the sim, live and in playback (not scripted menu runs).
        let race = match crate::race::race_for_scenario(&sc.name) {
            Some((def, autopilot)) if !scripted => Some(RaceRun::new(
                &def,
                autopilot,
                &mut sim,
                &ids,
                &cars,
                &sc,
                run.as_mut(),
            )?),
            _ => None,
        };
        Ok(Self {
            race,
            run,
            career: false,
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
        })
    }

    /// Live session. `scripted` = drive every entity by its scenario driver until
    /// `duration_s` (menu scenarios); otherwise every entity is live-controlled (free drive).
    /// A free-drive city session spawns the first entity on the city's race start grid.
    pub fn new(sc: Scenario, scripted: bool) -> Result<Self, String> {
        Self::new_with_data(sc, scripted, mem_data())
    }

    /// [`Session::new`] with the scenario's cars read from `data` (the career's modded car
    /// overlays the embedded files; city, tires and materials come from `data` too).
    pub fn new_with_data(
        mut sc: Scenario,
        scripted: bool,
        data: &dyn simcli::data::DataSource,
    ) -> Result<Self, String> {
        sc.validate()?;
        // Race scenarios already stand on their grid.
        let race = crate::race::race_for_scenario(&sc.name).is_some();
        if !scripted && !race && sc.city != CitySpec::Flat {
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
        Self::build(sc, scripted, None, None, data)
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
        Self::build(sc, false, Some(systems), Some(replay), mem_data())
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
        if self.script_running()
            || self.last_input[idx] == input
            || self.held(id)
            || self.race_holds(id)
        {
            return Ok(false);
        }
        self.log_input(idx, input);
        Ok(true)
    }

    /// The race holds this car's input (the player during the countdown, or on autopilot).
    fn race_holds(&self, id: EntityId) -> bool {
        self.race
            .as_ref()
            .is_some_and(|r| r.live().player() == id && r.holds_player(self.sim.tick()))
    }

    /// The picker race attached to this session, if any.
    pub fn race(&self) -> Option<&RaceRun> {
        self.race.as_ref()
    }

    /// The race's results panel (once the player is no longer racing).
    pub fn race_report(&self) -> Option<crate::race::RaceReport> {
        let over = self.run.as_ref().is_some_and(PlayerRun::is_over);
        self.race.as_ref().and_then(|r| r.report(over))
    }

    fn script_running(&self) -> bool {
        self.scripted && self.sim.tick() < self.script_end
    }

    /// Reset a vehicle to its spawn pose, pristine (logged op).
    pub fn reset_vehicle(&mut self, id: EntityId) -> Result<(), String> {
        self.live_check()?;
        let idx = self.idx(id)?;
        self.log_op(idx, OpKind::Reset)?;
        self.relocate_racer(id);
        Ok(())
    }

    /// Reset in place (testbed R, playtest row 65): pick the nearest clear lane spot within
    /// 15 m heading the car's own way ([`crate::reset::reset_pose`]), put the car there upright
    /// and at rest, and repair it if `repair` (false in a career run). Logged as the replay op
    /// `OpKind::ResetAt` carrying the pose, so playback does not redo the search. Police heat,
    /// pursuit and the run carry on; a race is told the car moved on purpose (position and lap
    /// kept).
    pub fn reset_in_place(
        &mut self,
        id: EntityId,
        repair: bool,
    ) -> Result<crate::reset::ResetPose, String> {
        use crate::reset::{reset_pose, travel_dir, Obstacle, OTHER_HALF};
        self.live_check()?;
        let idx = self.idx(id)?;
        let st = self
            .vehicle(id)
            .map(|v| v.vehicle.state())
            .ok_or_else(|| format!("unknown vehicle {id:?}"))?;
        let yaw_of_rot = |q: [f32; 4]| {
            let fx = 2.0 * (q[0] * q[1] - q[3] * q[2]);
            let fy = 1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]);
            wp_math::atan2(-fx, fy)
        };
        let mut obstacles: Vec<Obstacle> = Vec::new();
        for v in self.sim.vehicles() {
            if v.id == id {
                continue;
            }
            let o = v.vehicle.state();
            obstacles.push(Obstacle {
                pos: o.pos,
                yaw: yaw_of_rot(o.rot),
                half: OTHER_HALF,
            });
        }
        for b in &self.sc.barriers {
            obstacles.push(Obstacle {
                pos: b.pos,
                yaw: b.yaw,
                half: [b.size[0] * 0.5, b.size[1] * 0.5],
            });
        }
        let world = self.sim.world();
        for a in world.agents() {
            obstacles.push(Obstacle {
                pos: a.pos,
                yaw: a.yaw,
                half: OTHER_HALF,
            });
        }
        for p in world.static_props() {
            obstacles.push(Obstacle {
                pos: p.pos,
                yaw: p.yaw,
                half: [p.half_extents[0], p.half_extents[1]],
            });
        }
        let pose = reset_pose(
            Some(world.map()),
            st.pos,
            travel_dir(st.rot, st.lin_vel),
            &obstacles,
        );
        self.log_op(
            idx,
            OpKind::ResetAt {
                xy: pose.xy,
                yaw: pose.yaw,
                near_z: pose.near_z,
                repair,
            },
        )?;
        self.relocate_racer(id);
        Ok(pose)
    }

    /// Tell the race (if any) that `id` was moved on purpose, so the judge does not read the
    /// jump as a teleport.
    fn relocate_racer(&mut self, id: EntityId) {
        let tick = self.sim.tick();
        if let (Some(race), Some(v)) = (
            &mut self.race,
            self.sim.vehicles().iter().find(|v| v.id == id),
        ) {
            race.relocate(id, v.vehicle.state().pos, tick);
        }
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
        if self.career && mode != self.mode {
            return Err("a career run keeps the career's damage mode".into());
        }
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
        // The race's buy-in was charged on the fresh account: charge the adopted one instead.
        if let (Some(race), Some(run)) = (&mut self.race, &mut self.run) {
            race.reenter(run);
        }
    }

    // ----- career (machine G; see `crate::career`) -----------------------------------------

    /// This session is a career run.
    pub fn is_career(&self) -> bool {
        self.career
    }

    /// Make this live session a career run, with `run` as the player's economy (the career's
    /// account bound to the career car; `None` for sessions without police).
    pub fn begin_career(&mut self, run: Option<PlayerRun>) -> Result<(), String> {
        self.live_check()?;
        if self.sim.tick() != 0 {
            return Err("a career run starts before the first tick".into());
        }
        if let Some(run) = run {
            if self.run.is_none() {
                return Err("this session has no economy run (no police)".into());
            }
            self.run = Some(run);
        }
        self.career = true;
        Ok(())
    }

    /// End the career run: the session goes on as a plain session; returns its economy run
    /// (the career absorbs it).
    pub fn end_career(&mut self) -> Option<PlayerRun> {
        self.career = false;
        self.run.take()
    }

    /// The sim before the first tick (the career restores the car's saved damage). Not in
    /// the replay log: replays of a run that started damaged do not verify.
    pub fn sim_before_start_mut(&mut self) -> Result<&mut Sim, String> {
        self.live_check()?;
        if self.sim.tick() != 0 {
            return Err("only before the first tick".into());
        }
        Ok(&mut self.sim)
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
            .map(|v| v.id)
            .ok_or("no police car yet")?;
        self.debug_park_in_front_of(id, cop, ahead_m)?;
        Ok(cop)
    }

    /// Debug / e2e: park vehicle `id` (stopped) `ahead_m` in front of vehicle `other`, in its
    /// lane. Not in the replay log; a race (if any) is told the car was moved on purpose.
    pub fn debug_park_in_front_of(
        &mut self,
        id: EntityId,
        other: EntityId,
        ahead_m: f32,
    ) -> Result<(), String> {
        self.live_check()?;
        let st = self
            .vehicle(other)
            .map(|v| v.vehicle.state())
            .ok_or_else(|| format!("unknown vehicle {other:?}"))?;
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
        self.relocate_racer(id);
        Ok(())
    }

    /// Debug / e2e: aim vehicle `id` at a parked car so it knocks it: placed 5.5 m off the
    /// car's flank (a side with a drivable lane and no other prop), facing it, rolling at `speed_mps`.
    /// `prop` = index into [`crate::props::static_list`], or `None` for the parked car nearest
    /// the vehicle. Returns the prop index. Not in the replay log.
    pub fn debug_knock_prop(
        &mut self,
        id: EntityId,
        prop: Option<usize>,
        speed_mps: f32,
    ) -> Result<usize, String> {
        self.live_check()?;
        let here = self
            .vehicle(id)
            .map(|v| v.vehicle.state().pos)
            .ok_or_else(|| format!("unknown vehicle {id:?}"))?;
        let target = match prop {
            Some(i) if i < self.sim.prop_count() => i,
            Some(i) => return Err(format!("no prop {i}")),
            None => crate::props::nearest_parked_car(&self.sim, [here[0], here[1]])
                .ok_or("no parked cars in this session")?,
        };
        let (p, q) = self.sim.prop_pose(target).ok_or("no prop pose")?;
        let r = crate::props::prop_right_xy(q);
        // Come from a flank with a drivable lane next to it (the other is the kerb), from a
        // spot no other prop is on (streets have parking on both sides).
        const OFFSET_M: f32 = 5.5;
        let map = self.sim.world().map();
        let lane_at = |side: f32| {
            let probe = [p[0] + r[0] * side * 3.5, p[1] + r[1] * side * 3.5, p[2]];
            map.locate(probe, None, 0.0, true, false).is_some()
        };
        let spot = |side: f32| [p[0] + r[0] * side * OFFSET_M, p[1] + r[1] * side * OFFSET_M];
        let clear = |side: f32| {
            let xy = spot(side);
            (0..self.sim.prop_count()).all(|i| {
                i == target
                    || self.sim.prop_pose(i).is_none_or(|(o, _)| {
                        (o[0] - xy[0]).powi(2) + (o[1] - xy[1]).powi(2) > 4.5 * 4.5
                    })
            })
        };
        let side = [-1.0, 1.0]
            .into_iter()
            .find(|s| lane_at(*s) && clear(*s))
            .or_else(|| [-1.0, 1.0].into_iter().find(|s| clear(*s)))
            .ok_or("no clear spot next to that prop")?;
        let xy = spot(side);
        // Facing the prop: forward at yaw ψ is (−sin ψ, cos ψ) = −side · r.
        let yaw = wp_math::atan2(side * r[0], -side * r[1]);
        self.sim
            .place_on_ground(id, xy, yaw, speed_mps, Some(p[2]))
            .map_err(|e| e.to_string())?;
        self.relocate_racer(id);
        Ok(target)
    }

    /// The vehicle is held on the brakes because its run is over (capture or wreck).
    pub fn held(&self, id: EntityId) -> bool {
        self.run
            .as_ref()
            .is_some_and(|r| r.is_over() && r.player() == id)
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
        } else if self.script_running() {
            // Scenario entities only: city runs add police / traffic cars (not scripted).
            let states = scenario_states(&self.sim, &self.ids);
            for (idx, (_, state)) in states.iter().enumerate() {
                let input = driver_input(&self.sc, idx, tick, state, &self.last_input[idx]);
                self.log_input(idx, input);
            }
        }

        // 1b. Race: the AI field drives its cars (and the player's grid hold). In playback the
        //     field still runs (its decisions feed the judge) but the log supplies the inputs.
        if let Some(inputs) = self.race.as_mut().map(|r| r.pre_step(&self.sim)) {
            if self.playback.is_none() {
                for (e, input) in inputs {
                    if self.held(e) {
                        continue;
                    }
                    if let Some(idx) = self.index(e) {
                        self.log_input(idx, input);
                    }
                }
            }
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
        // Race judging, outside the step: SimEvent::Race signals (and the police events of a
        // delivered meet report) join this tick's events.
        let extra = match &mut self.race {
            Some(race) => race.post_step(&mut self.sim, &drained),
            None => Vec::new(),
        };
        self.observe_run(&extra);
        if let Some(race) = &mut self.race {
            race.settle(self.run.as_mut());
        }
        self.events.extend(drained);
        self.events.extend(extra);
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
