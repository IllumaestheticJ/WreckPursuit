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
    apply_op, apply_police_orders, build_sim, driver_input, scenario_states, world_config,
    BuiltSim, RunOptions, Selection,
};
use simcli::scenario::{CitySpec, EntitySpec};
use simcli::{RunSummary, Scenario};

use crate::crash_replay::{
    CrashMark, CrashRecorder, CrashReplayInfo, Snapshot, POST_IMPACT_TICKS, PRE_IMPACT_TICKS,
};
use crate::data::mem_data;
use crate::garage_visit::{GarageOffer, GarageRepairReport, GarageSite, GARAGE_ENTER_SPEED_MPS};
use crate::race::RaceRun;
use crate::race_spots::MeetSpots;
use crate::roam::RoamRun;
use crate::run::{ContinueReport, PlayerCar, PlayerRun};
use crate::scenario::entity;
use crate::street::{Preview, StreetRacers};

thread_local! {
    /// Street racers spawn in new live city sessions ([`set_street_racers_enabled`]).
    static STREET_ENABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Add NPC street racers to live free-drive / night-run sessions in a city started from now
/// on (default on; e2e and benchmarks may turn them off). Sessions of a scenario that already
/// has street racers (a replay, a saved street session) keep them either way.
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

/// Live sessions hash the state every this many ticks (once per 60 Hz frame) for the replay
/// log; the ticks between are logged as 0 = not recorded.
pub const LIVE_HASH_EVERY: u64 = 4;

/// Replay playback state.
#[derive(Debug, Clone)]
pub struct Playback {
    /// The replay being played.
    pub replay: Replay,
    input_cursor: usize,
    op_cursor: usize,
    /// Playback speed multiplier (0 = paused).
    pub speed: f32,
    /// Ticks verified: up to the last tick whose recorded hash matched (live sessions record a
    /// hash every [`LIVE_HASH_EVERY`] ticks; a match vouches for the ticks since the last one).
    pub verified: u64,
    /// The tick verification has reached (the playback's start tick at first).
    checked_to: u64,
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
    /// Modified cars of scenario entities with `mods` (they drive these, not `cars`).
    modded: BTreeMap<EntityId, LoadedCar>,
    /// Per-vehicle extras for every sim vehicle, by entity.
    extras: BTreeMap<EntityId, EntityExtra>,
    last_input: Vec<VehicleInput>,
    fixed: FixedStep,
    log: Replay,
    recording: bool,
    events: Vec<SimEvent>,
    /// The city's repair shops (sessions with an economy run; see [`crate::garage_visit`]).
    garages: Vec<GarageSite>,
    /// The open garage visit (the player's car is held while it is open).
    visit: Option<GarageOffer>,
    /// The player's car has been outside every garage since the last visit (or the start).
    garage_armed: bool,
    /// Garage visits opened so far.
    visit_seq: u32,
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
    /// Honk-to-start race spots (see [`crate::race_spots`]).
    meet_spots: MeetSpots,
    /// The player's horn last tick. The one horn edge every challenge reads (owner: one
    /// challenge system on the horn action): a street racer in reach first, else the race
    /// spot the player stands in (see [`Session::observe_horn`]).
    horn_was: bool,
    /// Free-roam street racers (scenario entities `roam_*`; see [`crate::roam`]). Driven
    /// only live; in playback the log supplies their inputs.
    roam: Option<RoamRun>,
    /// Rolling snapshot buffer + big-crash detector (live sessions; see [`crate::crash_replay`]).
    crash: CrashRecorder,
    /// The street AI (live sessions whose scenario has street racers; see [`crate::street`]).
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
        data: &dyn simcli::data::DataSource,
    ) -> Result<Self, String> {
        let load_city = sc.city != CitySpec::Flat;
        let BuiltSim {
            mut sim,
            ids,
            cars,
            modded,
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
        // Street racers (scenario entities `street_*`, see `crate::street`): AI racers in every
        // session that has them, driven by the street AI in live sessions only (a playback
        // takes their inputs and moves from the log).
        crate::street::mark_racers(&mut sim, &sc, &ids);
        let street = if playback.is_none() && !scripted && sim.systems().police {
            StreetRacers::new(&sc, &ids)
        } else {
            None
        };
        // The touge wager the menu prepared: taken (and cleared) by every live session, so a
        // stale one never attaches to a later battle; a replay never takes it.
        let wager = if playback.is_none() {
            crate::touge::take_next_wager()
        } else {
            wp_race::touge::Wager::None
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
                wager,
            )?),
            _ => None,
        };
        // Free roam: built in playback too (it marks the roamers' cars as AI racers).
        let roam = if scripted {
            None
        } else {
            RoamRun::new(&mut sim, &ids, &cars, &sc)?
        };
        let crash = if playback.is_some() {
            CrashRecorder::off()
        } else {
            CrashRecorder::default()
        };
        let garages = if run.is_some() {
            crate::garage_visit::sites(sim.world())
        } else {
            Vec::new()
        };
        Ok(Self {
            street,
            crash,
            race,
            meet_spots: MeetSpots::default(),
            horn_was: false,
            roam,
            run,
            career: false,
            garages,
            visit: None,
            garage_armed: false,
            visit_seq: 0,
            script_end: if scripted { sc.ticks() } else { 0 },
            mode: sc.mode,
            sc,
            scripted,
            load_city,
            systems: selection,
            sim,
            ids,
            cars,
            modded,
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
                checked_to: 0,
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
                // Street racers join the scenario as entities before the sim is built, so a
                // recording carries them and playback spawns them the same way.
                if STREET_ENABLED.with(|c| c.get()) && crate::street::scenario_wants_racers(&sc) {
                    let at = sc
                        .entities
                        .first()
                        .map_or([0.0; 2], |e| [e.pos[0], e.pos[1]]);
                    let night = wp_contracts::TimeOfDay::from_hours(sc.time_of_day_h).phase()
                        == wp_contracts::DayPhase::Night;
                    let roster = crate::street::plan_roster(&world, at, sc.seed, night);
                    sc.entities.extend(roster);
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

    /// The simulation, mutable (crate tests: script police state).
    #[cfg(test)]
    pub(crate) fn sim_mut(&mut self) -> &mut Sim {
        &mut self.sim
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

    /// Mods installed on a scenario entity (`EntitySpec::mods`; empty for stock cars and for
    /// vehicles the sim spawned itself).
    pub fn entity_mods(&self, id: EntityId) -> &[String] {
        self.ids
            .iter()
            .position(|x| *x == id)
            .and_then(|i| self.sc.entities.get(i))
            .map_or(&[], |e| e.mods.as_slice())
    }

    /// Resolved car of any sim vehicle (a modded scenario car resolves to its build), or of a
    /// dented parked car by its `wp_sim::PARKED_ENTITY_BASE` id.
    pub fn car(&self, id: EntityId) -> Option<&LoadedCar> {
        if let Some(car) = self.modded.get(&id) {
            return Some(car);
        }
        if let Some(w) = self.parked_wreck(id) {
            return self.cars.get(&w.car_id);
        }
        self.vehicle(id).and_then(|v| self.cars.get(&v.car_id))
    }

    /// A dented parked car by its `wp_sim::PARKED_ENTITY_BASE` id.
    pub fn parked_wreck(&self, id: EntityId) -> Option<&wp_sim::ParkedWreck> {
        wp_sim::prop_of_entity(id).and_then(|i| self.sim.parked_wreck(i))
    }

    /// Per-vehicle extras.
    pub fn extra(&self, id: EntityId) -> Option<&EntityExtra> {
        self.extras.get(&id)
    }

    /// Load the car of every sim vehicle and dented parked car not seen before (the sim may
    /// spawn police units and physical traffic, and promote hit parked cars, on its own).
    fn sync_cars(&mut self) {
        let ids = self
            .sim
            .vehicles()
            .iter()
            .map(|v| &v.car_id)
            .chain(self.sim.parked_wrecks().iter().map(|w| &w.car_id));
        for car_id in ids {
            if !self.cars.contains_key(car_id) {
                if let Ok(car) = simcli::data::load_car(mem_data(), car_id) {
                    self.cars.insert(car_id.clone(), car);
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
        if self.race_holds(id) && !self.held(id) {
            // On the grid only the throttle gets through (revving on the line; the grid hold
            // keeps the handbrake on and the clutch down).
            let tick = self.sim.tick();
            if let Some(r) = self.race.as_mut().filter(|r| !r.live().autopilot()) {
                if r.live().in_countdown(tick) {
                    r.set_grid_throttle(input.throttle);
                }
            }
        }
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

    /// The free-roam street racers attached to this session, if any.
    pub fn roam(&self) -> Option<&RoamRun> {
        self.roam.as_ref()
    }

    /// Bet on yourself in this session's race while the countdown runs (see
    /// [`crate::race::RaceRun::place_bet`]). Not recorded in replays: bets move money, not
    /// cars.
    pub fn race_place_bet(
        &mut self,
        label: &str,
        stake: i64,
    ) -> Result<crate::race::BetCard, String> {
        let race = self.race.as_mut().ok_or("no race in this session")?;
        let run = self.run.as_mut().ok_or("no account in this session")?;
        race.place_bet(run, label, stake)
    }

    /// `(max stake, lock reason)` for picker-race bets (venue `City`) on this session's
    /// account (`(0, Some(..))` without an account). Street-racer challenges have their own
    /// limits (`street_status_json`'s offer).
    pub fn bet_status(&self) -> (i64, Option<String>) {
        match &self.run {
            Some(run) => (
                run.max_bet(wp_economy::BetVenue::City),
                run.bet_lock(wp_economy::BetVenue::City),
            ),
            None => (0, Some("no account in this session".into())),
        }
    }

    /// The race's results panel (once the player is no longer racing).
    pub fn race_report(&self) -> Option<crate::race::RaceReport> {
        let over = self.run.as_ref().is_some_and(PlayerRun::is_over);
        self.race.as_ref().and_then(|r| r.report(over))
    }

    /// Honk-to-start race spots are live: a live (not replayed, not scripted) city session
    /// that is not a race or a career run, with the player's run still going.
    pub fn race_spots_active(&self) -> bool {
        self.load_city
            && self.playback.is_none()
            && !self.scripted
            && self.race.is_none()
            && !self.career
            && !self.run.as_ref().is_some_and(PlayerRun::is_over)
            && !self.ids.is_empty()
    }

    /// The race spots of this session's city (empty when [`Session::race_spots_active`] is
    /// false). Planned on the first call.
    pub fn race_spots(&mut self) -> &[wp_race::spots::RaceSpot] {
        if !self.race_spots_active() {
            return &[];
        }
        self.meet_spots.spots(self.sim.world())
    }

    /// Take the race the player just honked for in a spot (see [`crate::race_spots`]).
    pub fn take_race_spot_honked(&mut self) -> Option<&'static str> {
        self.meet_spots.take_honked()
    }

    /// A honk (`honk`: the horn's rising edge this tick) the street racers did not take: when
    /// the player stands nearly stopped in a race spot, record its race for the testbed
    /// (`race_spot_honked()`). The same rules as a street challenge (owner row 150): not while
    /// the player is wanted or chased, not with a street challenge or street race open, not
    /// with the run over. Reads the sim only (no writes), before the step.
    fn observe_horn(&mut self, honk: bool) {
        if !honk || !self.race_spots_active() {
            return;
        }
        let Some(&player) = self.ids.first() else {
            return;
        };
        if self.sim.police().is_wanted_or_pursued(player)
            || self.street.as_ref().is_some_and(|s| s.busy())
        {
            return;
        }
        let Some(v) = self.sim.vehicles().iter().find(|v| v.id == player) else {
            return;
        };
        let st = v.vehicle.state();
        let speed = (st.lin_vel[0].powi(2) + st.lin_vel[1].powi(2) + st.lin_vel[2].powi(2)).sqrt();
        self.meet_spots.honk(self.sim.world(), st.pos, speed);
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
        let st = self
            .vehicle(id)
            .map(|v| v.vehicle.state())
            .ok_or_else(|| format!("unknown vehicle {id:?}"))?;
        let dir = crate::reset::travel_dir(st.rot, st.lin_vel);
        self.reset_near(id, st.pos, dir, repair)
    }

    /// Put racer `id` back on the race course at `pos` heading `dir` (the judge's off-course
    /// reset): the nearest clear lane spot there, at rest, damage kept. Logged like
    /// [`Self::reset_in_place`].
    fn reset_onto_course(&mut self, id: EntityId, pos: [f32; 3], dir: [f32; 2]) {
        // Only an unknown id fails, and a racer is always a scenario entity.
        let _ = self.reset_near(id, pos, dir, false);
    }

    /// The shared part of [`Self::reset_in_place`] and [`Self::reset_onto_course`]: the
    /// nearest clear lane spot to `origin` heading `dir` ([`crate::reset::reset_pose`]).
    fn reset_near(
        &mut self,
        id: EntityId,
        origin: [f32; 3],
        dir: [f32; 2],
        repair: bool,
    ) -> Result<crate::reset::ResetPose, String> {
        use crate::reset::{reset_pose, Obstacle, OTHER_HALF};
        self.live_check()?;
        let idx = self.idx(id)?;
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
        let pose = reset_pose(Some(world.map()), origin, dir, &obstacles);
        self.log_op(
            idx,
            OpKind::ResetAt {
                xy: pose.xy,
                yaw: pose.yaw,
                near_z: pose.near_z,
                repair,
            },
        )?;
        // Ghost for 1 s so the car cannot land stuck in (or be hit by) traffic.
        self.log_op(
            idx,
            OpKind::Ghost {
                ticks: crate::reset::GHOST_TICKS,
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

    /// The run, mutably (testbed debug tools only; not in the replay log).
    pub fn debug_run_mut(&mut self) -> Option<&mut PlayerRun> {
        self.run.as_mut()
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

    /// Re-apply vehicle `id`'s body inertia after a lab ride override
    /// (`wp_sim::Sim::refresh_body_inertia`). Not a logged op; allowed in playback too.
    pub fn refresh_body_inertia(&mut self, id: EntityId) -> Result<(), String> {
        self.sim
            .refresh_body_inertia(id)
            .map_err(|e| format!("{e:?}"))
    }

    /// Mutable sim for debug tools (`debug_set_heat`) and tests. Live sessions only; changes
    /// made here are not in the replay log.
    pub fn debug_sim_mut(&mut self) -> Result<&mut Sim, String> {
        self.live_check()?;
        // The change is not in the log: the crash-replay buffer starts again later.
        self.crash.clear_snapshots();
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
        self.crash_resnapshot();
        Ok(())
    }

    /// Debug / e2e: put vehicle `id` on the ground at `xy` facing `yaw`, rolling forward at
    /// `speed_mps` (height from the surface nearest `near_z`). Not in the replay log; a race
    /// (if any) is told the car was moved on purpose.
    pub fn debug_place(
        &mut self,
        id: EntityId,
        xy: [f32; 2],
        yaw: f32,
        speed_mps: f32,
        near_z: Option<f32>,
    ) -> Result<(), String> {
        self.live_check()?;
        self.sim
            .place_on_ground(id, xy, yaw, speed_mps, near_z)
            .map_err(|e| e.to_string())?;
        self.relocate_racer(id);
        self.crash_resnapshot();
        Ok(())
    }

    /// Debug / e2e: aim vehicle `id` at a parked car so it knocks it: placed 5.5 m off the
    /// car's flank (on a drivable lane, no other prop there), facing it, rolling at `speed_mps`.
    /// `prop` = index into [`crate::props::static_list`], or `None` for the nearest parked car
    /// that has such a spot. Returns the prop index. Not in the replay log.
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
        // With no prop given, the nearest parked car that has a usable approach.
        let candidates: Vec<usize> = match prop {
            Some(i) if i < self.sim.prop_count() => vec![i],
            Some(i) => return Err(format!("no prop {i}")),
            None => crate::props::parked_cars_by_distance(&self.sim, [here[0], here[1]])
                .into_iter()
                .take(64)
                .collect(),
        };
        let (target, xy, yaw, z) = candidates
            .into_iter()
            .find_map(|i| self.knock_approach(i).map(|(xy, yaw, z)| (i, xy, yaw, z)))
            .ok_or("no parked car with a clear lane beside it")?;
        self.sim
            .place_on_ground(id, xy, yaw, speed_mps, Some(z))
            .map_err(|e| e.to_string())?;
        self.relocate_racer(id);
        self.crash_resnapshot();
        Ok(target)
    }

    /// Where to put a car to knock prop `target`: 5.5 m off one of its flanks, on a drivable
    /// lane (so never on a sidewalk or inside a building), with no other prop within 4.5 m;
    /// facing the prop. `(xy, yaw, z hint)`, or `None` when neither flank qualifies.
    fn knock_approach(&self, target: usize) -> Option<([f32; 2], f32, f32)> {
        const OFFSET_M: f32 = 5.5;
        let (p, q) = self.sim.prop_pose(target)?;
        let r = crate::props::prop_right_xy(q);
        let map = self.sim.world().map();
        let spot = |side: f32| [p[0] + r[0] * side * OFFSET_M, p[1] + r[1] * side * OFFSET_M];
        let usable = |side: f32| {
            let xy = spot(side);
            // The whole run-up (car centre, and its far end 2.5 m further out) is road.
            let on_lane = [0.0, 2.5].iter().all(|back| {
                let d = OFFSET_M + back;
                let pt = [p[0] + r[0] * side * d, p[1] + r[1] * side * d, p[2]];
                map.locate(pt, None, 0.0, true, false).is_some()
            });
            on_lane
                && (0..self.sim.prop_count()).all(|i| {
                    i == target
                        || self.sim.prop_pose(i).is_none_or(|(o, _)| {
                            (o[0] - xy[0]).powi(2) + (o[1] - xy[1]).powi(2) > 4.5 * 4.5
                        })
                })
        };
        let side = [-1.0f32, 1.0].into_iter().find(|s| usable(*s))?;
        // Facing the prop: forward at yaw ψ is (−sin ψ, cos ψ) = −side · r.
        let yaw = wp_math::atan2(side * r[0], -side * r[1]);
        Some((spot(side), yaw, p[2]))
    }

    /// The vehicle is held on the brakes because its run is over (capture or wreck), it is in
    /// a garage visit, or a street race holds it.
    pub fn held(&self, id: EntityId) -> bool {
        self.run.as_ref().is_some_and(|r| {
            r.player() == id
                && (r.is_over()
                    || self.visit.is_some()
                    || self
                        .street
                        .as_ref()
                        .is_some_and(|s| s.holds_player(self.sim.tick())))
        })
    }

    // ----- garage visits (lead; see `crate::garage_visit`) ------------------------------

    /// The city's repair shops (empty without an economy run or a city, in picker races and in
    /// career runs).
    pub fn garages(&self) -> &[GarageSite] {
        if self.race.is_some() || self.career {
            return &[];
        }
        &self.garages
    }

    /// The open garage visit, if any.
    pub fn garage_visit(&self) -> Option<&GarageOffer> {
        self.visit.as_ref()
    }

    /// The player's car's damage model and the mode table (for pricing).
    fn player_damage(
        sim: &Sim,
        player: EntityId,
    ) -> Option<(&dyn wp_contracts::DamageSim, &wp_contracts::ModeTable)> {
        sim.vehicles()
            .iter()
            .find(|v| v.id == player)
            .map(|v| (&v.damage as &dyn wp_contracts::DamageSim, sim.mode_table()))
    }

    /// Open a visit when the player's car has stopped inside a garage (and close one a new
    /// pursuit or the end of the run interrupts). Called after every live tick.
    fn check_garage(&mut self) {
        let Some(run) = &self.run else {
            return;
        };
        let player = run.player();
        let chased = self.sim.police().racer(player).is_some_and(|r| {
            matches!(
                r.status,
                wp_police::PursuitStatus::Pursuit | wp_police::PursuitStatus::Searching
            )
        });
        if self.visit.is_some() {
            if run.is_over() || chased {
                self.garage_leave();
            }
            return;
        }
        // Not in picker races (a stop in a shop would hold a racer) nor career runs (the
        // career garage screen owns repairs and the career account).
        if self.garages.is_empty() || run.is_over() || self.race.is_some() || self.career {
            return;
        }
        let Some(st) = self.vehicle(player).map(|v| v.vehicle.state()) else {
            return;
        };
        let Some(site) = self
            .garages
            .iter()
            .find(|g| g.contains([st.pos[0], st.pos[1]]))
        else {
            self.garage_armed = true;
            return;
        };
        let speed = st.lin_vel[0].hypot(st.lin_vel[1]);
        if !self.garage_armed || chased || speed >= GARAGE_ENTER_SPEED_MPS {
            return;
        }
        let wanted = self
            .sim
            .police()
            .racer(player)
            .map_or(wp_contracts::WantedLevel::None, |r| r.level);
        let site = site.clone();
        let damage = Self::player_damage(&self.sim, player);
        let (quote, note) = match run.garage_quote(&site.shop(wanted), damage) {
            Ok(q) => (Some(q), None),
            Err(wp_economy::EconomyError::NothingToRepair) => {
                (None, Some("nothing to repair".to_string()))
            }
            Err(e) => (None, Some(e.to_string())),
        };
        let cash = run.account().cash;
        let health = damage.map_or(1.0, |(d, _)| d.summary().health);
        self.visit_seq += 1;
        self.garage_armed = false;
        self.visit = Some(GarageOffer {
            seq: self.visit_seq,
            tick: self.sim.tick(),
            can_repair: quote.as_ref().is_some_and(|q| q.player_pays <= cash),
            site,
            wanted,
            health,
            quote,
            note,
            cash,
        });
    }

    /// Release the player's car after a hold: neutral input, headlights kept.
    fn release_player(&mut self, idx: usize) {
        let keep = self.last_input[idx];
        self.log_input(
            idx,
            VehicleInput {
                headlights: keep.headlights,
                ..Default::default()
            },
        );
    }

    /// "Repair" in the open garage visit: pay the shop's `RepairAll` from the ledger, repair
    /// the car in place (logged replay op) and give control back.
    pub fn garage_repair(&mut self) -> Result<GarageRepairReport, String> {
        self.live_check()?;
        let offer = self.visit.as_ref().ok_or("not in a garage")?;
        let (name, shop) = (offer.site.name.clone(), offer.site.shop(offer.wanted));
        let run = self.run.as_mut().ok_or("no economy run in this session")?;
        let player = run.player();
        let damage = Self::player_damage(&self.sim, player);
        let report = run.garage_repair(&name, &shop, damage)?;
        let idx = self.idx(player)?;
        self.log_op(idx, OpKind::Repair)?;
        self.visit = None;
        self.release_player(idx);
        Ok(report)
    }

    /// "Leave" the open garage visit without repairing (no-op without one).
    pub fn garage_leave(&mut self) {
        if self.visit.take().is_none() {
            return;
        }
        if let Some(idx) = self.run.as_ref().and_then(|r| self.index(r.player())) {
            self.release_player(idx);
        }
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

    /// Tests: put scenario entity `id` at `xy` facing `yaw`, at rest, as a logged `ResetAt`
    /// op (unlike `debug_sim_mut`, recordings and crash replays keep the move).
    #[cfg(test)]
    pub(crate) fn place_logged(
        &mut self,
        id: EntityId,
        xy: [f32; 2],
        yaw: f32,
        near_z: f32,
    ) -> Result<(), String> {
        let idx = self.idx(id)?;
        self.log_op(
            idx,
            OpKind::ResetAt {
                xy,
                yaw,
                near_z,
                repair: false,
            },
        )
    }

    /// Plan a street race from the player through the waypoints `via` (in order) to
    /// `finish` (the open challenge's B).
    pub fn street_preview(&self, finish: [f32; 2], via: &[[f32; 2]]) -> Result<Preview, String> {
        let (street, run) = self
            .street
            .as_ref()
            .zip(self.run.as_ref())
            .ok_or("no street racers in this session")?;
        street.preview(&self.sim, run.player(), finish, via)
    }

    /// Accept the open challenge: race through `via` (in order) to `finish` for `stake`
    /// dollars. Both cars are reset side by side at the start (logged ops).
    pub fn street_accept(
        &mut self,
        finish: [f32; 2],
        via: &[[f32; 2]],
        stake: i64,
    ) -> Result<(), String> {
        self.live_check()?;
        let (street, run) = self
            .street
            .as_mut()
            .zip(self.run.as_mut())
            .ok_or("no street racers in this session")?;
        let moves = street.accept(&self.sim, run, finish, via, stake)?;
        for m in moves {
            self.apply_street_move(m);
        }
        Ok(())
    }

    /// Log and apply a street racer's move ([`crate::street::Move`]) as replay ops: `ResetAt`,
    /// then `Ghost` when it asks for a ghost window.
    fn apply_street_move(&mut self, m: crate::street::Move) {
        let Some(idx) = self.index(m.entity) else {
            return;
        };
        // Only an unknown entity index fails, and street racers are scenario entities.
        let _ = self.log_op(
            idx,
            OpKind::ResetAt {
                xy: m.xy,
                yaw: m.yaw,
                near_z: m.near_z,
                repair: m.repair,
            },
        );
        if m.ghost_ticks > 0 {
            let _ = self.log_op(
                idx,
                OpKind::Ghost {
                    ticks: m.ghost_ticks,
                },
            );
        }
    }

    /// Debug / e2e: park street racer `entity` idle at rest stop `name` (a logged move).
    pub fn street_park_at_rest_stop(&mut self, entity: EntityId, name: &str) -> Result<(), String> {
        self.live_check()?;
        let street = self
            .street
            .as_mut()
            .ok_or("no street racers in this session")?;
        let m = street.park_at_rest_stop(&self.sim, entity, name)?;
        self.apply_street_move(m);
        Ok(())
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
        self.release_player(idx);
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
        // The crash-replay buffer starts with the session's first state (inputs and ops
        // logged at tick 0 are applied again on top of it, which leaves it unchanged).
        if tick == 0 {
            self.crash_snapshot();
        }
        // 0. Then the scenario's police orders (roadblocks, spikes) due now; refusals are ignored
        // here (simcli lists them in its summary).
        let _ = apply_police_orders(&self.sc, &mut self.sim, &self.ids, tick);
        // 1. Inputs for this tick: replay track, scenario drivers, or (live) already applied.
        if let Some(mut p) = self.playback.take() {
            let r = &p.replay;
            while p.op_cursor < r.ops.len() && r.ops[p.op_cursor].tick <= tick {
                let ev = r.ops[p.op_cursor];
                if apply_op(&mut self.sim, &self.ids, mem_data(), &ev).is_err() {
                    p.drift_count += 1;
                } else if let OpKind::Mode(m) = ev.op {
                    self.mode = m;
                } else if let (OpKind::Reset | OpKind::ResetAt { .. }, Some(&id)) =
                    (ev.op, self.ids.get(ev.entity as usize))
                {
                    // As live: the race does not judge the move as a teleport.
                    self.relocate_racer(id);
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
            .filter(|r| r.is_over() || self.visit.is_some())
            .and_then(|r| self.index(r.player()))
        {
            // The run is over, or the car is in a garage: it brakes to a halt and stays there.
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
            // Street race countdown: the player waits on the handbrake for the green light (a
            // held brake pedal at a standstill would engage reverse).
            let keep = self.last_input[idx];
            self.log_input(
                idx,
                VehicleInput {
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

        // 1c. The horn: one rising edge for every challenge (street racers, then race spots).
        let horn = self.last_input.first().is_some_and(|i| i.horn);
        let honk = horn && !self.horn_was;
        self.horn_was = horn;
        let mut honk_taken = false;
        // 1d. Street racers (live only: playback has no street AI): their moves (logged ops)
        //     and inputs (logged like a race's AI), so recordings replay them.
        if let (Some(street), Some(&player)) = (&mut self.street, self.ids.first()) {
            let over = self.run.as_ref().is_some_and(|r| r.is_over());
            let step = street.before_step(&self.sim, player, honk, over);
            honk_taken = step.challenged;
            for m in step.moves {
                self.apply_street_move(m);
            }
            for e in step.sleep {
                if let Some(idx) = self.index(e) {
                    // Only an unknown entity index fails, and street racers are scenario entities.
                    let _ = self.log_op(idx, OpKind::Dormant { on: true });
                }
            }
            for (e, on) in step.light {
                if let Some(idx) = self.index(e) {
                    let _ = self.log_op(idx, OpKind::LightWhenFar { on });
                }
            }
            for (e, input) in step.inputs {
                if let Some(idx) = self.index(e) {
                    self.log_input(idx, input);
                }
            }
        }

        // 1e. Race spots: a honk no street racer took (reads the sim only).
        self.observe_horn(honk && !honk_taken);
        // 1f. Free roam (live): put stranded roamers back on the road out of the player's
        //     sight (replay ops, as the testbed's reset in place), then the roamers drive.
        if self.playback.is_none() {
            let rescues = self
                .roam
                .as_ref()
                .map(|r| r.rescues(&self.sim, self.ids.first().copied()))
                .unwrap_or_default();
            for r in rescues {
                let placed = self
                    .log_op(
                        r.idx,
                        OpKind::ResetAt {
                            xy: [r.slot.pos[0], r.slot.pos[1]],
                            yaw: r.slot.yaw,
                            near_z: r.slot.pos[2],
                            repair: true,
                        },
                    )
                    .and_then(|_| {
                        self.log_op(
                            r.idx,
                            OpKind::Ghost {
                                ticks: crate::reset::GHOST_TICKS,
                            },
                        )
                    });
                if placed.is_ok() {
                    if let Some(roam) = &mut self.roam {
                        roam.rescued(&r);
                    }
                }
            }
            if let Some(inputs) = self.roam.as_mut().map(|r| r.pre_step(&self.sim)) {
                for (e, input) in inputs {
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
        let player = self.ids.first().copied();
        let player_before = player.and_then(|p| {
            before
                .iter()
                .find(|(id, _)| *id == p)
                .map(|(_, s)| s.clone())
        });
        if capture_prev {
            for (id, s) in &before {
                self.extras.entry(*id).or_default().prev_state = Some(s.clone());
            }
        }
        self.sim.step_fixed();
        self.sync_cars();
        let drained = self.sim.drain_events();
        self.observe_run(&drained);
        if let Some(roam) = &mut self.roam {
            roam.post_step(&drained);
        }
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
        // Off-course racers go back onto the route. Live only: playback applies the logged
        // `ResetAt` op (and relocates the racer) instead.
        let resets = self
            .race
            .as_mut()
            .map(|r| r.take_resets())
            .unwrap_or_default();
        if self.playback.is_none() {
            for r in resets {
                self.reset_onto_course(r.racer, r.pos, r.dir);
            }
            self.check_garage();
        }
        if self.playback.is_none() {
            if let Some(p) = player {
                self.observe_crash(p, player_before.as_ref(), &drained);
            }
        }
        if let Some(street) = &mut self.street {
            street.after_step(&self.sim, &drained, self.run.as_mut());
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

        // 3. Hashes: replay.bin keeps one per tick; live sessions compute one every
        //    `LIVE_HASH_EVERY` ticks (the state hash is ~0.06 ms per car per frame, the lattice
        //    nodes) and log 0 = "not recorded" for the ticks between. Playback compares the
        //    recorded ones.
        let now = self.sim.tick();
        match &mut self.playback {
            Some(p) => match p.replay.tick_hashes.get(now as usize - 1) {
                Some(&h) if h != 0 => {
                    if self.sim.state_hash() == h {
                        p.verified += now.saturating_sub(p.checked_to);
                    } else {
                        p.drift_count += 1;
                        if p.drift_ticks.len() < MAX_DRIFT_TICKS {
                            p.drift_ticks.push(now);
                        }
                    }
                    p.checked_to = now;
                }
                _ => {}
            },
            None => {
                let due = now.is_multiple_of(LIVE_HASH_EVERY)
                    || (self.scripted && now == self.script_end);
                let hash = if due { self.sim.state_hash() } else { 0 };
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
                // Crash-replay buffer: a snapshot at the end of the tick, after everything
                // this tick logs (inputs logged later carry tick `now` and are replayed).
                if self.crash.snapshot_due(now) {
                    self.crash_snapshot();
                }
            }
        }
    }

    /// Push a crash-replay snapshot of the current state (see [`crate::crash_replay`]).
    fn crash_snapshot(&mut self) {
        if !self.crash.enabled() || self.playback.is_some() {
            return;
        }
        let snap = Snapshot {
            tick: self.sim.tick(),
            sim: self.sim.clone(),
            race: self.race.clone(),
            last_input: self.last_input.clone(),
            extras: self.extras.clone(),
            mode: self.mode,
        };
        self.crash.push_snapshot(snap);
    }

    /// A debug tool moved something outside the log: older snapshots would replay without
    /// it, so the buffer restarts from the current state.
    fn crash_resnapshot(&mut self) {
        self.crash.clear_snapshots();
        self.crash_snapshot();
    }

    /// Feed the crash detector one stepped live tick of the player's car.
    fn observe_crash(
        &mut self,
        player: EntityId,
        before: Option<&VehicleState>,
        events: &[SimEvent],
    ) {
        let tick = self.sim.tick();
        let race_quiet = self.race.as_ref().is_some_and(|r| {
            r.live().player() == player && (r.holds_player(tick) || r.report(false).is_some())
        });
        let armed = !race_quiet && !self.held(player);
        let mass = self.car(player).map_or(1500.0, |c| c.def.mass_kg);
        let after = self.vehicle(player).map(|v| v.vehicle.state());
        self.crash.observe(&crate::crash_replay::PlayerTick {
            now: tick,
            player,
            mass_kg: mass,
            before,
            after: after.as_ref(),
            events,
            armed,
        });
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
        // The last tick always carries its hash (the sim is at it now).
        if let Some(h) = r.tick_hashes.last_mut().filter(|h| **h == 0) {
            *h = self.sim.state_hash();
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

    // ----- crash replay (lead; see `crate::crash_replay`) -----------------------------------

    /// The crash recorder (tuning, buffered snapshot ticks, last crash).
    pub fn crash(&self) -> &CrashRecorder {
        &self.crash
    }

    /// The crash recorder, mutable (tuning, on/off).
    pub fn crash_mut(&mut self) -> &mut CrashRecorder {
        &mut self.crash
    }

    /// Take the big crash detected since the last call (live sessions).
    pub fn crash_take(&mut self) -> Option<CrashMark> {
        self.crash.take()
    }

    /// A playback session showing crash `crash` (`impact − 3 s .. impact + 2 s`, clamped to the
    /// buffer), built from the buffered snapshot before it and this session's log. `self` is
    /// not changed. Ticks past the current live tick replay the last logged inputs.
    pub fn crash_replay(&self, crash: &CrashMark) -> Result<(Session, CrashReplayInfo), String> {
        self.live_check()?;
        let now = self.sim.tick();
        if crash.tick > now {
            return Err("that crash has not happened yet".into());
        }
        let want = crash.tick.saturating_sub(PRE_IMPACT_TICKS);
        let snap = self
            .crash
            .snapshot_for(want)
            .ok_or("no replay buffer (crash replays are off in this session)")?;
        if snap.tick > crash.tick {
            return Err("that crash is no longer in the replay buffer".into());
        }
        let start = want.max(snap.tick);
        let end = crash.tick + POST_IMPACT_TICKS;
        let mut replay = self.log.clone();
        replay.ticks = end;
        let input_cursor = replay.inputs.partition_point(|e| e.tick < snap.tick);
        let op_cursor = replay.ops.partition_point(|e| e.tick < snap.tick);
        let n = self.ids.len();
        let mut s = Session {
            // No street AI in a crash replay: the racers are scenario entities, so the snapshot
            // holds their cars and the log their inputs and moves, like any other entity.
            street: None,
            // A crash replay is playback: no garage visits.
            garages: Vec::new(),
            visit: None,
            garage_armed: false,
            visit_seq: 0,
            sc: self.sc.clone(),
            scripted: self.scripted,
            script_end: self.script_end,
            load_city: self.load_city,
            systems: self.systems,
            sim: snap.sim.clone(),
            ids: self.ids.clone(),
            cars: self.cars.clone(),
            modded: self.modded.clone(),
            extras: snap.extras.clone(),
            last_input: snap.last_input.clone(),
            fixed: FixedStep::new(PHYSICS_DT, MAX_TICKS_PER_STEP),
            log: Replay::new(&self.sc, &self.systems.to_string(), n),
            recording: false,
            events: Vec::new(),
            playback: Some(Playback {
                replay,
                input_cursor,
                op_cursor,
                speed: 1.0,
                verified: 0,
                checked_to: snap.tick,
                drift_ticks: Vec::new(),
                drift_count: 0,
            }),
            mode: snap.mode,
            live_end_hash: None,
            outcome: None,
            run: None,
            career: false,
            race: snap.race.clone(),
            meet_spots: MeetSpots::default(),
            horn_was: false,
            roam: None,
            crash: CrashRecorder::off(),
        };
        s.run_ticks(start - snap.tick);
        s.events.clear();
        let info = CrashReplayInfo {
            crash: crash.clone(),
            start_tick: start,
            end_tick: end,
            live_tick: now,
            live_hash: format!("{:016x}", self.sim.state_hash()),
        };
        Ok((s, info))
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
            recorded_final_hash: p
                .replay
                .tick_hashes
                .iter()
                .rev()
                .find(|h| **h != 0)
                .map(|h| format!("{h:016x}")),
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
