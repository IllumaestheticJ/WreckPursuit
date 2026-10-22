//! Street races in the browser (workstream 15, machine L): the race picker's races
//! ([`wp_race::catalog`]) run inside a live [`crate::session::Session`].
//!
//! A race session is an ordinary simcli v2 scenario named `race_<id>` ([`race_scenario`]):
//! the player (scenario entity 0, sim entity 1) at the back of the grid and the AI field on
//! the judge's grid slots ahead. When the session sees that name it attaches a [`RaceRun`]:
//!
//! - **before each tick** the `wp_opponent` field drives the AI cars (logged inputs, so
//!   recordings replay) and the player is held on the handbrake until green;
//! - **after each tick**, outside the deterministic step, the judge reads the drained events
//!   and vehicle states and emits `SimEvent::Race` into the session's event buffer (goldens
//!   and the state hash of every non-race scenario are unchanged);
//! - a hot meet calls in a patrol (`PatrolAttracted` → `wp_race::police` reports delivered to
//!   the sim's police), and illegal races put their stake heat on every racer;
//! - the buy-in is charged at the start and the result paid at the end through G's economy
//!   (`Economy::enter_race` / `settle_race`, priced by kind and legality: the R5 scale), on
//!   the player's [`crate::run::PlayerRun`] account. A police capture mid-race is a DNF and
//!   the existing end-of-run panel takes over (`handoff_run_end`).
//!
//! In replay playback the race is rebuilt from the scenario name and re-judged (AI inputs
//! come from the log), so the browser's playback verifies tick hashes; `simcli replay` does
//! not know about races and drifts after the meet report (see `docs/integration/race.md`).

use std::collections::BTreeMap;

use serde::Serialize;
use simcli::run::world_config;
use simcli::Scenario;
use wp_contracts::{DamageMode, EntityId, LoadedCar, RaceKind, SimEvent, VehicleInput, VehicleSim};
use wp_opponent::{CarCapability, SimPolice, VehicleObs};
use wp_race::catalog::{self, RaceDef};
use wp_race::live::{LiveEntrant, LiveRace, Standing};
use wp_race::{RaceData, RacerResult, RacerSample, RacerStatus};
use wp_sim::{Sim, VehicleKind};

use crate::data::mem_data;
use crate::run::{LedgerLine, PlayerRun, RacePayout};
use crate::scenario::{city_spec, entity, free_drive_in};

/// Scenario-name suffix of a race whose player car is driven by the AI (tests, e2e).
pub const AUTOPILOT_SUFFIX: &str = "+autopilot";

/// Floats per racer in `race_standings()`.
pub const STANDING_STRIDE: usize = 10;

/// Floats in `race_info()`.
pub const INFO_LEN: usize = 12;

/// The race a scenario name stands for, and whether the player's car is on autopilot.
pub fn race_for_scenario(name: &str) -> Option<(RaceDef, bool)> {
    match name.strip_suffix(AUTOPILOT_SUFFIX) {
        Some(base) => catalog::race_for_scenario(base).map(|d| (d, true)),
        None => catalog::race_for_scenario(name).map(|d| (d, false)),
    }
}

/// Scenario id of an AI driver (`ai_<first name>`).
pub fn ai_entity_id(d: &catalog::AiDriverDef) -> String {
    let first = d.name.split(' ').next().unwrap_or(d.name);
    format!("ai_{}", first.to_lowercase())
}

/// The live scenario of race `def` with the player in `car`: the race's city, hour, seed and
/// police presence; the player (`"player"`, entity 0) on the last grid slot and the AI field
/// (`ai_<name>`) on the slots ahead, all standing still. Time runs at 1× during a race.
pub fn race_scenario(
    def: &RaceDef,
    car: &str,
    mode: DamageMode,
    autopilot: bool,
) -> Result<Scenario, String> {
    let mut sc = free_drive_in(car, mode, city_spec(def.city.id()));
    sc.name = def.scenario_name();
    if autopilot {
        sc.name.push_str(AUTOPILOT_SUFFIX);
    }
    sc.seed = def.seed;
    sc.time_of_day_h = def.time_of_day_h;
    sc.time_ratio = 1.0;
    // AI cars drive with the AI assist profile; the testbed applies the player's own preset.
    sc.assists = simcli::scenario::AssistSpec::Ai;
    sc.police_presence = Some(def.police_presence());
    sc.traffic_density = Some(def.traffic_density);
    let world = simcli::data::load_world(mem_data(), &sc.city, world_config(&sc))
        .map_err(|e| e.to_string())?
        .ok_or("race city did not load")?;
    let race = catalog::plan_course(&world, def)
        .ok_or_else(|| format!("{}: no route on {}", def.id, def.city.id()))?;
    let slots = wp_race::live::grid(def, &race);
    let yaw = |s: &wp_race::GridSlot| wp_world::geom::yaw_of(s.dir);
    let player = slots.last().ok_or("empty grid")?;
    sc.entities = vec![entity("player", car, player.pos, yaw(player))];
    for (d, s) in def.ai.iter().zip(&slots) {
        sc.entities
            .push(entity(&ai_entity_id(d), d.car, s.pos, yaw(s)));
    }
    sc.validate()?;
    Ok(sc)
}

/// One AI driver in the picker (`race_list`).
#[derive(Debug, Clone, Serialize)]
pub struct DriverCard {
    /// Name.
    pub name: &'static str,
    /// Personality.
    pub style: &'static str,
    /// Car id.
    pub car: &'static str,
}

/// One race in the picker (`race_list`).
#[derive(Debug, Clone, Serialize)]
pub struct RaceCard {
    /// Id for `race_start`.
    pub id: &'static str,
    /// Name.
    pub name: &'static str,
    /// Description.
    pub blurb: &'static str,
    /// City id.
    pub city: &'static str,
    /// Kind key (`sprint`, `circuit`, `checkpoint`).
    pub kind: &'static str,
    /// Laps.
    pub laps: u32,
    /// Legality key (`street`, `illegal_daytime`).
    pub legality: &'static str,
    /// Start hour.
    pub time_of_day_h: f32,
    /// Course length, m.
    pub length_m: f32,
    /// Police presence multiplier (1 normal, 2 max).
    pub police: f32,
    /// The meet is hot: a patrol is called in at the green light.
    pub hot_meet: bool,
    /// The AI field.
    pub drivers: Vec<DriverCard>,
    /// Advertised winner purse for `car` (`data/events/stakes.toml`, reconciled with G's).
    pub purse: i64,
    /// Advertised buy-in for `car`.
    pub entry_fee: i64,
}

/// The picker for a player in `car` (stakes depend on the car's class bracket).
pub fn race_cards(car: Option<&LoadedCar>, mode: DamageMode) -> Vec<RaceCard> {
    let data = RaceData::embedded();
    crate::garage::with_economy(|econ| {
        catalog::races()
            .iter()
            .map(|d| {
                let (purse, entry_fee) = car.map_or((0, 0), |c| {
                    let r = advertised(econ, &data, d, c, d.length_hint_m, mode);
                    (r.purse, r.entry_fee)
                });
                RaceCard {
                    id: d.id,
                    name: d.name,
                    blurb: d.blurb,
                    city: d.city.id(),
                    kind: d.kind.key(),
                    laps: d.laps(),
                    legality: d.legality.key(),
                    time_of_day_h: d.time_of_day_h,
                    length_m: d.length_hint_m,
                    police: d.police_presence(),
                    hot_meet: d.prior_meets >= 2,
                    drivers: d
                        .ai
                        .iter()
                        .map(|a| DriverCard {
                            name: a.name,
                            style: a.style,
                            car: a.car,
                        })
                        .collect(),
                    purse,
                    entry_fee,
                }
            })
            .collect()
    })
}

fn phase_of(def: &RaceDef) -> wp_contracts::DayPhase {
    wp_contracts::TimeOfDay::from_hours(def.time_of_day_h).phase()
}

/// Ledger note of the adjustment that brings the economy's price to the advertised stake.
pub const STAKE_NOTE: &str = "race stake (advertised purse / buy-in)";

/// `amount × scale`, rounded (how [`wp_race::bridge::economy_race`] scales the economy's
/// numbers to the advertised stake).
pub fn to_stake(amount: i64, scale: f32) -> i64 {
    (amount as f32 * scale).round() as i64
}

/// The economy's purse and clean bonus brought to the advertised stake (`purse_scale`).
pub fn stake_settlement(purse: i64, clean_bonus: i64, purse_scale: f32) -> (i64, i64) {
    (
        to_stake(purse, purse_scale),
        to_stake(clean_bonus, purse_scale),
    )
}

/// Advertised stake (`wp_race::bridge::economy_race`, request R5).
pub(crate) fn advertised(
    econ: &wp_economy::Economy,
    data: &RaceData,
    def: &RaceDef,
    car: &LoadedCar,
    length_m: f32,
    mode: DamageMode,
) -> wp_race::bridge::EconomyRace {
    let bracket = data.progression.bracket_for(econ.class_index(&car.def));
    wp_race::bridge::economy_race(
        econ,
        &data.stakes,
        def.kind,
        def.legality,
        car.def.class,
        bracket,
        length_m,
        phase_of(def),
        mode,
    )
}

/// One row of the results table.
#[derive(Debug, Clone, Serialize)]
pub struct ResultRow {
    /// Final position.
    pub position: u8,
    /// Racer entity.
    pub entity: u32,
    /// Driver name ("You" for the player).
    pub name: String,
    /// Car id.
    pub car: String,
    /// The player.
    pub is_player: bool,
    /// `Finished`, `Running`, `DNF`, `DQ`.
    pub status: &'static str,
    /// DNF / DQ reason (`Captured`, `LeftCorridor`, …).
    pub reason: Option<String>,
    /// Finish time, s.
    pub time_s: Option<f32>,
    /// Gap to the winner, s.
    pub gap_s: Option<f32>,
    /// Best lap, s (circuits).
    pub best_lap_s: Option<f32>,
    /// Clean race.
    pub clean: bool,
    /// Contacts above the clean threshold.
    pub contacts: u32,
    /// Civilian hits.
    pub civilian_hits: u32,
}

/// The race result for the testbed's results panel (`race_result_json`).
#[derive(Debug, Clone, Serialize)]
pub struct RaceReport {
    /// Race id.
    pub id: &'static str,
    /// Race name.
    pub name: &'static str,
    /// Kind key.
    pub kind: &'static str,
    /// Legality key.
    pub legality: &'static str,
    /// Course length, m.
    pub length_m: f32,
    /// Results are final (everyone finished, DNF'd or was DQ'd, or the window closed).
    pub final_results: bool,
    /// Rows in position order.
    pub rows: Vec<ResultRow>,
    /// The player's row index in `rows`.
    pub player_row: usize,
    /// Advertised purse and buy-in for the player's car.
    pub advertised_purse: i64,
    /// Advertised buy-in.
    pub advertised_entry_fee: i64,
    /// What the race paid the player (once final; `None` without an economy run).
    pub payout: Option<RacePayout>,
    /// Why the buy-in or payout was refused, if it was.
    pub economy_error: Option<String>,
    /// The police caught the player (or the run ended): the end-of-run panel owns the screen.
    pub handoff_run_end: bool,
}

/// A picker race attached to a session (see the module docs).
#[derive(Debug, Clone)]
pub struct RaceRun {
    live: LiveRace,
    /// `(name, car)` per racer.
    names: BTreeMap<EntityId, (String, String)>,
    prev_events: Vec<SimEvent>,
    setup: wp_economy::RaceSetup,
    advertised: (i64, i64),
    /// `(purse_scale, entry_scale)` of the advertised stake ([`wp_race::bridge::EconomyRace`]):
    /// the economy prices the race, these bring it to what the lobby advertised.
    stake_scale: (f32, f32),
    entry: Option<(i64, Vec<LedgerLine>)>,
    payout: Option<RacePayout>,
    economy_error: Option<String>,
    settled: bool,
    /// Off-course resets from the last judged tick ([`RaceRun::take_resets`]).
    resets: Vec<wp_race::CourseReset>,
}

impl RaceRun {
    /// Attach race `def` to a freshly built session (before the first tick): `ids` are the
    /// scenario entities in scenario order (player first, then `def.ai`), `cars` by car id.
    /// Marks the AI cars `VehicleKind::AiRacer`, starts the countdown and charges the buy-in
    /// on `run`'s account (when there is one).
    pub fn new(
        def: &RaceDef,
        autopilot: bool,
        sim: &mut Sim,
        ids: &[EntityId],
        cars: &BTreeMap<String, LoadedCar>,
        sc: &Scenario,
        run: Option<&mut PlayerRun>,
    ) -> Result<Self, String> {
        if ids.len() != def.entrants() || sc.entities.len() != ids.len() {
            return Err(format!(
                "{}: scenario has {} entities, the race needs {}",
                def.id,
                ids.len(),
                def.entrants()
            ));
        }
        let race = catalog::plan_course(sim.world(), def)
            .ok_or_else(|| format!("{}: no route", def.id))?;
        let entrant = |i: usize| -> Result<LiveEntrant, String> {
            let car = cars
                .get(&sc.entities[i].car)
                .ok_or_else(|| format!("car {} not loaded", sc.entities[i].car))?;
            Ok(LiveEntrant {
                entity: ids[i],
                redline_rpm: car.def.engine.redline_rpm,
                capability: CarCapability::for_car(car),
            })
        };
        let player = entrant(0)?;
        let ai = (1..ids.len()).map(entrant).collect::<Result<Vec<_>, _>>()?;
        for e in &ai {
            sim.set_vehicle_kind(e.entity, VehicleKind::AiRacer)
                .map_err(|e| e.to_string())?;
        }
        // Autopilot: the AI drives the player's car, so no player steering aids on it (the AI
        // steers with the normal model, like the field).
        if autopilot {
            sim.set_player_steering(player.entity, Some(false))
                .map_err(|e| e.to_string())?;
        }
        let abs_s = (def.time_of_day_h * 3600.0) as u64;
        let live = LiveRace::new(
            def,
            sim.world(),
            race,
            &ai,
            player,
            sim.tick(),
            abs_s,
            autopilot,
        );
        let mut names = BTreeMap::new();
        names.insert(ids[0], ("You".to_string(), sc.entities[0].car.clone()));
        for (i, d) in def.ai.iter().enumerate() {
            names.insert(ids[i + 1], (d.name.to_string(), d.car.to_string()));
        }
        let player_car = &cars[&sc.entities[0].car];
        let length = live.race().route.length_m;
        let data = RaceData::embedded();
        let (setup, adv) = crate::garage::with_economy(|econ| {
            let setup = econ.race_setup(
                def.kind,
                def.legality,
                player_car.def.class,
                length,
                phase_of(def),
                sc.mode,
            );
            let adv = advertised(econ, &data, def, player_car, length, sc.mode);
            (setup, adv)
        });
        let mut me = Self {
            live,
            names,
            prev_events: Vec::new(),
            setup,
            advertised: (adv.purse, adv.entry_fee),
            stake_scale: (adv.purse_scale, adv.entry_scale),
            entry: None,
            payout: None,
            economy_error: None,
            settled: false,
            resets: Vec::new(),
        };
        if let Some(run) = run {
            me.charge_entry(run);
        }
        Ok(me)
    }

    /// Charge the buy-in at the advertised stake: G's `enter_race` prices it, an adjustment
    /// line brings it to `entry_fee × entry_scale` (as `wp_race::bridge::economy_race`).
    fn charge_entry(&mut self, run: &mut PlayerRun) {
        match run.enter_race(&self.setup) {
            Ok((fee, mut lines)) => {
                let want = to_stake(fee, self.stake_scale.1);
                if want != fee {
                    match run.adjust(fee - want, STAKE_NOTE) {
                        Ok(l) => lines.extend(l),
                        Err(e) => self.economy_error = Some(format!("buy-in stake: {e}")),
                    }
                }
                self.entry = Some((want, lines));
            }
            Err(e) => self.economy_error = Some(format!("buy-in refused: {e}")),
        }
    }

    /// Charge the buy-in on another account (the session adopted the previous run's account
    /// after this race was attached).
    pub fn reenter(&mut self, run: &mut PlayerRun) {
        self.economy_error = None;
        self.entry = None;
        self.charge_entry(run);
    }

    /// The car was moved on purpose (reset, debug park): judge its next sample from `pos`.
    pub fn relocate(&mut self, racer: EntityId, pos: [f32; 3], tick: u64) {
        self.live.relocate(racer, pos, tick);
    }

    /// Racers the judge wants put back on the course (off it past the grace time), oldest
    /// first. The session moves each car and calls [`RaceRun::relocate`].
    pub fn take_resets(&mut self) -> Vec<wp_race::CourseReset> {
        std::mem::take(&mut self.resets)
    }

    /// The live race (judge, standings, course).
    pub fn live(&self) -> &LiveRace {
        &self.live
    }

    /// The player's throttle while held on the grid (revs on the line during the countdown).
    pub fn set_grid_throttle(&mut self, throttle: f32) {
        self.live.set_grid_throttle(throttle);
    }

    /// Inputs for this tick (AI cars, the player's grid hold), before `Sim::step_fixed`.
    pub fn pre_step(&mut self, sim: &mut Sim) -> Vec<(EntityId, VehicleInput)> {
        let obs: Vec<VehicleObs> = sim
            .vehicles()
            .iter()
            .map(|v| VehicleObs {
                entity: v.id,
                state: v.vehicle.state(),
                effects: wp_contracts::DamageSim::handling_effects(&v.damage),
                size_m: [
                    v.vehicle.def().body_size_m[0],
                    v.vehicle.def().body_size_m[1],
                ],
            })
            .collect();
        // Slipstream: every car's drag from the wake geometry, live and in playback alike (it
        // is computed from the sim state, so replays reproduce it). The AI field drives for
        // the tow (`wp_race::AiEntrants` sets `RaceConfig::draft`).
        for (e, k) in self.live.ai().slipstream(&obs) {
            let _ = sim.set_drag_scale(e, k);
        }
        let police = SimPolice(sim.police());
        self.live
            .pre_step(sim.tick(), sim.world(), &obs, &self.prev_events, &police)
    }

    /// The race holds the player's input: the countdown is running, or the AI drives.
    pub fn holds_player(&self, tick: u64) -> bool {
        self.live.autopilot() || self.live.in_countdown(tick)
    }

    /// Judge one stepped tick. Returns the sim events to add to the session's buffer (race
    /// signals, police events from a delivered meet report). Mutates the sim's police only for
    /// a hot meet's report and the stake heat (once, at the green light).
    pub fn post_step(&mut self, sim: &mut Sim, events: &[SimEvent]) -> Vec<SimEvent> {
        let racers = self.live.racers().to_vec();
        let samples: Vec<RacerSample> = sim
            .vehicles()
            .iter()
            .filter(|v| racers.contains(&v.id))
            .map(|v| RacerSample::from_state(v.id, &v.vehicle.state(), v.input.throttle))
            .collect();
        let police: Vec<EntityId> = sim
            .vehicles()
            .iter()
            .filter(|v| v.kind == VehicleKind::Police)
            .map(|v| v.id)
            .collect();
        let out = self.live.post_step(sim.tick(), &samples, events, &police);
        self.prev_events = events.to_vec();
        self.resets.extend_from_slice(&out.resets);
        let mut emitted: Vec<SimEvent> = out.signals.iter().map(|s| s.to_sim_event()).collect();
        if !out.reports.is_empty() {
            let views: Vec<wp_police::RacerView> = samples
                .iter()
                .map(|s| wp_police::RacerView {
                    entity: s.racer,
                    pos: s.pos,
                    vel: s.vel,
                })
                .collect();
            let mut p = sim.police().clone();
            emitted.extend(wp_race::police::deliver(
                &mut p,
                sim.world(),
                &views,
                &out.reports,
            ));
            sim.set_police(p);
        }
        for (e, h) in &out.add_heat {
            let cur = sim.police().racer(*e).map_or(0.0, |r| r.heat);
            sim.police_mut().set_heat(*e, cur + h);
        }
        emitted
    }

    /// Pay the player once the results are final (G's `settle_race` on `run`'s account).
    pub fn settle(&mut self, run: Option<&mut PlayerRun>) {
        if self.settled || !self.live.is_finished() {
            return;
        }
        self.settled = true;
        let (Some(run), Some(entry)) = (run, self.entry.take()) else {
            return;
        };
        let Some(r) = self.player_result() else {
            return;
        };
        let res = wp_race::bridge::economy_result(&r, run.wanted_level());
        match run.settle_race(&self.setup, &res, entry) {
            Ok(mut p) => {
                // Pay the advertised stake: purse and clean bonus × purse_scale (the bridge's
                // R5 scale; the economy alone pays its own per-km base).
                let (purse, bonus) = stake_settlement(p.purse, p.clean_bonus, self.stake_scale.0);
                let diff = purse + bonus - p.purse - p.clean_bonus;
                if diff != 0 {
                    match run.adjust(diff, STAKE_NOTE) {
                        Ok(lines) => {
                            p.ledger.extend(lines);
                            p.purse = purse;
                            p.clean_bonus = bonus;
                            p.cash_after = run.account().cash;
                        }
                        Err(e) => self.economy_error = Some(format!("payout stake: {e}")),
                    }
                }
                self.payout = Some(p);
            }
            Err(e) => self.economy_error = Some(format!("payout refused: {e}")),
        }
    }

    fn player_result(&self) -> Option<RacerResult> {
        let player = self.live.player();
        let results = match self.live.final_results() {
            Some(r) => r.to_vec(),
            None => self.live.judge().results(),
        };
        results.into_iter().find(|r| r.racer == player)
    }

    /// Live standings.
    pub fn standings(&self) -> Vec<Standing> {
        self.live.standings()
    }

    /// `race_standings()`: [`STANDING_STRIDE`] floats per racer in position order: `[entity,
    /// position, status (0 running, 1 finished, 2 DNF, 3 DQ), lap, next_gate, gates_passed,
    /// progress_m, gap_ahead_s (−1 none), gap_behind_s (−1 none), elapsed_s]`.
    pub fn standings_packed(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(STANDING_STRIDE * self.live.racers().len());
        for s in self.standings() {
            out.extend_from_slice(&[
                s.racer.0 as f32,
                s.position as f32,
                status_code(s.status),
                s.lap as f32,
                s.next_gate as f32,
                s.gates_passed as f32,
                s.progress_m,
                s.gap_ahead_s.unwrap_or(-1.0),
                s.gap_behind_s.unwrap_or(-1.0),
                s.elapsed_s,
            ]);
        }
        out
    }

    /// `race_info()`: `[phase (0 countdown, 1 running, 2 finished), countdown_s, elapsed_s,
    /// laps, gates, player entity, racers, closed loop (0/1), length_m, autopilot (0/1),
    /// kind (0 sprint, 1 circuit, 2 checkpoint, 3 other), player done (0/1)]`.
    pub fn info(&self, tick: u64) -> Vec<f32> {
        let l = &self.live;
        let phase = if l.is_finished() {
            2.0
        } else if l.in_countdown(tick) {
            0.0
        } else {
            1.0
        };
        let kind = match l.def().kind {
            RaceKind::Sprint => 0.0,
            RaceKind::Circuit { .. } => 1.0,
            RaceKind::Checkpoint => 2.0,
            _ => 3.0,
        };
        let player_done = self
            .player_result()
            .is_some_and(|r| r.status != RacerStatus::Running);
        vec![
            phase,
            l.countdown_s(tick),
            l.elapsed_s(tick),
            l.def().laps() as f32,
            l.race().course.gates.len() as f32,
            l.player().0 as f32,
            l.racers().len() as f32,
            if l.race().course.closed_loop {
                1.0
            } else {
                0.0
            },
            l.race().route.length_m,
            if l.autopilot() { 1.0 } else { 0.0 },
            kind,
            if player_done { 1.0 } else { 0.0 },
        ]
    }

    /// `race_course()`: the gates (6 floats each: `x, y, z, dir_x, dir_y, width_m`), then the
    /// route centreline thinned to ≤ ~10 m spacing (3 floats per point), with the gate count
    /// first: `[gates, x, y, z, dx, dy, w, …, x, y, z, …]`.
    pub fn course_packed(&self) -> Vec<f32> {
        let c = &self.live.race().course;
        let mut out = vec![c.gates.len() as f32];
        for g in &c.gates {
            out.extend_from_slice(&[g.pos[0], g.pos[1], g.pos[2], g.dir[0], g.dir[1], g.width_m]);
        }
        let mut last: Option<[f32; 3]> = None;
        for (i, p) in c.polyline.iter().enumerate() {
            let keep = i + 1 == c.polyline.len()
                || last.is_none_or(|q| (p[0] - q[0]).hypot(p[1] - q[1]) >= 10.0);
            if keep {
                out.extend_from_slice(p);
                last = Some(*p);
            }
        }
        out
    }

    /// The results panel, once the player is no longer racing (`None` before).
    pub fn report(&self, player_run_over: bool) -> Option<RaceReport> {
        let me = self.player_result()?;
        if me.status == RacerStatus::Running {
            return None;
        }
        let def = self.live.def();
        let results = match self.live.final_results() {
            Some(r) => r.to_vec(),
            None => self.live.judge().results(),
        };
        let winner = results
            .iter()
            .find(|r| r.status == RacerStatus::Finished)
            .and_then(RacerResult::elapsed_s);
        let rows: Vec<ResultRow> = results
            .iter()
            .map(|r| {
                let (name, car) = self.names.get(&r.racer).cloned().unwrap_or_default();
                let (status, reason) = match r.status {
                    RacerStatus::Finished => ("Finished", None),
                    RacerStatus::Running => ("Running", None),
                    RacerStatus::Dnf(x) => ("DNF", Some(format!("{x:?}"))),
                    RacerStatus::Dq(x) => ("DQ", Some(format!("{x:?}"))),
                };
                ResultRow {
                    position: r.position,
                    entity: r.racer.0,
                    name,
                    car,
                    is_player: r.racer == self.live.player(),
                    status,
                    reason,
                    time_s: r.elapsed_s(),
                    gap_s: r.elapsed_s().zip(winner).map(|(t, w)| t - w),
                    best_lap_s: r.best_lap_subticks.map(wp_race::result::subticks_to_s),
                    clean: r.clean,
                    contacts: r.contacts,
                    civilian_hits: r.civilian_hits,
                }
            })
            .collect();
        let player_row = rows.iter().position(|r| r.is_player).unwrap_or(0);
        Some(RaceReport {
            id: def.id,
            name: def.name,
            kind: def.kind.key(),
            legality: def.legality.key(),
            length_m: self.live.race().route.length_m,
            final_results: self.live.is_finished(),
            rows,
            player_row,
            advertised_purse: self.advertised.0,
            advertised_entry_fee: self.advertised.1,
            payout: self.payout.clone(),
            economy_error: self.economy_error.clone(),
            handoff_run_end: player_run_over
                || me.status == RacerStatus::Dnf(wp_race::DnfReason::Captured),
        })
    }

    /// The buy-in charged at the start (fee, ledger lines), if any.
    pub fn entry(&self) -> Option<&(i64, Vec<LedgerLine>)> {
        self.entry.as_ref()
    }

    /// The settled payout, once final.
    pub fn payout(&self) -> Option<&RacePayout> {
        self.payout.as_ref()
    }
}

fn status_code(s: RacerStatus) -> f32 {
    match s {
        RacerStatus::Running => 0.0,
        RacerStatus::Finished => 1.0,
        RacerStatus::Dnf(_) => 2.0,
        RacerStatus::Dq(_) => 3.0,
    }
}
