//! A picker race ([`crate::catalog::RaceDef`]) run live next to a `wp_sim` loop: one judge,
//! the `wp_opponent` AI field, the meet's heat and the HUD's standings.
//!
//! Everything here reads the sim; nothing steps it. The caller (the browser session in
//! `wp_wasm`, or a test loop) does, per physics tick:
//!
//! ```ignore
//! let inputs = race.pre_step(tick, sim.world(), &obs, &last_events, &SimPolice(sim.police()));
//! for (e, inp) in inputs { sim.set_input(e, inp)?; }   // AI cars + the player's grid hold
//! sim.step_fixed();
//! let events = sim.drain_events();
//! let out = race.post_step(sim.tick(), &samples, &events, &police_entities);
//! // out.signals → SimEvent::Race for the HUD / economy; out.reports → wp_police
//! // (`police::deliver`); out.add_heat → the stake's heat for illegal racers.
//! ```
//!
//! The player starts last on the grid and is held on the handbrake until green (unless
//! `autopilot` hands the player's car to the AI as well, for tests and e2e). Police captures
//! (`PursuitEnded { Captured* }`) become judge DNFs; `wp_opponent`'s flee/retire decisions
//! become `Dnf(Fled)` / `Dnf(Retired)` through [`crate::entrants`].

use std::collections::BTreeMap;

use wp_contracts::{
    DayPhase, EntityId, Legality, PoliceEvent, SimEvent, TimeOfDay, VehicleInput, PHYSICS_DT,
};
use wp_opponent::{CarCapability, PoliceQuery, VehicleObs};
use wp_world::World;

use crate::bridge::CityRace;
use crate::catalog::{venue_pos, RaceDef};
use crate::data::RaceData;
use crate::entrants::{AiEntrant, AiEntrants};
use crate::judge::{CourseReset, RaceJudge, RacePhase, RacerEntry, RacerSample};
use crate::kind::{ClassBracket, EventId, VenueId};
use crate::result::{DnfReason, RacerResult, RacerStatus};
use crate::schedule::{MeetHeat, MeetHeatRules, Venue};
use crate::signals::RaceSignal;

/// Gap history is sampled this often, ticks (4 Hz).
const HISTORY_EVERY_TICKS: u64 = 60;

/// Prior meets at the venue are this far apart, s (see `RaceDef::prior_meets`).
const PRIOR_MEET_SPACING_S: u64 = 900;

/// One car in the race.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveEntrant {
    /// Sim entity.
    pub entity: EntityId,
    /// Engine redline, rpm.
    pub redline_rpm: f32,
    /// Car limits for the AI: `CarCapability::for_car` (the car's measured limits from the
    /// pre-measured table; cheap and deterministic). `geometry_only` makes every car drive
    /// like a generic 55 m/s street car (unit tests only).
    pub capability: CarCapability,
}

/// What one tick of the race produced.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StepOut {
    /// Race signals (convert with `RaceSignal::to_sim_event`).
    pub signals: Vec<RaceSignal>,
    /// Police input events (a hot meet's reports) for `police::deliver`.
    pub reports: Vec<SimEvent>,
    /// Heat to add per racer (the stake's heat for illegal races, at the green light).
    pub add_heat: Vec<(EntityId, f32)>,
    /// Racers that stayed off the course too long: move each car to the reset pose (on the
    /// route, at rest) and call [`LiveRace::relocate`].
    pub resets: Vec<CourseReset>,
}

/// One racer's live standing (HUD).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Standing {
    /// Racer.
    pub racer: EntityId,
    /// Position, 1-based (provisional while running).
    pub position: u8,
    /// Status.
    pub status: RacerStatus,
    /// Current lap, 1-based (capped at the race's laps).
    pub lap: u32,
    /// Next gate to pass (index into the course's gates).
    pub next_gate: u32,
    /// Gates passed so far.
    pub gates_passed: u32,
    /// Distance along the course (plus completed laps), m.
    pub progress_m: f32,
    /// Seconds behind the car ahead (`None` for the leader or without data yet).
    pub gap_ahead_s: Option<f32>,
    /// Seconds the car behind is behind this one.
    pub gap_behind_s: Option<f32>,
    /// Elapsed race time (finish time for finishers), s.
    pub elapsed_s: f32,
}

/// A picker race in progress (see the module docs).
#[derive(Debug, Clone)]
pub struct LiveRace {
    def: RaceDef,
    race: CityRace,
    judge: RaceJudge,
    ai: AiEntrants,
    player: EntityId,
    autopilot: bool,
    /// The player's throttle on the grid (see [`LiveRace::set_grid_throttle`]).
    grid_throttle: f32,
    /// Judge entry order: the AI field (pole first), then the player.
    order: Vec<EntityId>,
    meet: MeetHeat,
    meet_rules: MeetHeatRules,
    venue: Venue,
    start_abs_s: u64,
    stake_heat: f32,
    /// `(tick, progress)` every [`HISTORY_EVERY_TICKS`] per racer (gaps).
    history: BTreeMap<EntityId, Vec<(u64, f32)>>,
    next_gate: BTreeMap<EntityId, u32>,
    final_results: Option<Vec<RacerResult>>,
    last_tick: u64,
}

/// The grid for `def` on `race`: slot per judge entry, AI field first (pole), the player last.
pub fn grid(def: &RaceDef, race: &CityRace) -> Vec<crate::course::GridSlot> {
    let data = RaceData::embedded();
    let mut judge = RaceJudge::new(
        EventId(0),
        data.rules.for_kind(def.kind),
        race.course.clone(),
    );
    (0..def.entrants())
        .map(|i| {
            judge
                .enter(RacerEntry {
                    racer: EntityId(10_000 + i as u32),
                    redline_rpm: 0.0,
                })
                .expect("the picker never overfills a grid")
        })
        .collect()
}

impl LiveRace {
    /// Set up the race and start its countdown at `tick`. `ai` are the AI cars in
    /// `def.ai` order (pole first), `player` starts behind them. `abs_s`: in-game time of
    /// the start, s (meet heat). `autopilot`: the AI drives the player's car too.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        def: &RaceDef,
        world: &World,
        race: CityRace,
        ai: &[LiveEntrant],
        player: LiveEntrant,
        tick: u64,
        abs_s: u64,
        autopilot: bool,
    ) -> LiveRace {
        let data = RaceData::embedded();
        let mut judge = RaceJudge::new(
            EventId(def.seed as u32),
            data.rules.for_kind(def.kind),
            race.course.clone(),
        );
        // A picker race puts a car that wanders off the route back on it (the glue moves the
        // car: `StepOut::resets`) rather than disqualifying it.
        judge.set_reset_off_course(true);
        let mut order = Vec::new();
        for e in ai.iter().chain(std::iter::once(&player)) {
            judge
                .enter(RacerEntry {
                    racer: e.entity,
                    redline_rpm: e.redline_rpm,
                })
                .expect("the picker never overfills a grid");
            order.push(e.entity);
        }
        let mut drivers: Vec<AiEntrant> = ai
            .iter()
            .zip(def.ai)
            .map(|(e, d)| AiEntrant {
                racer: e.entity,
                profile: d.profile,
                capability: e.capability,
            })
            .collect();
        if autopilot {
            drivers.push(AiEntrant {
                racer: player.entity,
                profile: wp_opponent::DriverProfile::default(),
                capability: player.capability,
            });
        }
        let closed = def.legality == Legality::Sanctioned;
        let field = AiEntrants::new(world, &race, def.kind, closed, &drivers, def.seed);
        judge.start(tick).expect("enough racers");

        let venue = Venue {
            id: VenueId(def.seed as u32),
            name: def.name.to_string(),
            pos: venue_pos(&race),
            district: None,
            closed_circuit: false,
        };
        let meet_rules = data.schedule.meet_heat.clone();
        let mut meet = MeetHeat::default();
        for k in (1..=def.prior_meets as u64).rev() {
            let at = abs_s.saturating_sub(k * PRIOR_MEET_SPACING_S);
            meet.record_race(&venue, at, &meet_rules);
        }
        let phase = TimeOfDay::from_hours(def.time_of_day_h).phase();
        let stake_heat = data
            .stakes
            .stake(def.kind, phase, ClassBracket::D, def.legality)
            .heat;
        let closed_loop = race.course.closed_loop;
        let next_gate = order
            .iter()
            .map(|e| (*e, if closed_loop { 1 } else { 0 }))
            .collect();
        LiveRace {
            def: def.clone(),
            race,
            judge,
            ai: field,
            player: player.entity,
            autopilot,
            grid_throttle: 0.0,
            order,
            meet,
            meet_rules,
            venue,
            start_abs_s: abs_s,
            stake_heat,
            history: BTreeMap::new(),
            next_gate,
            final_results: None,
            last_tick: tick,
        }
    }

    /// The race definition.
    pub fn def(&self) -> &RaceDef {
        &self.def
    }

    /// The planned route and judged course.
    pub fn race(&self) -> &CityRace {
        &self.race
    }

    /// The judge (phase, course, provisional results).
    pub fn judge(&self) -> &RaceJudge {
        &self.judge
    }

    /// The player's entity.
    pub fn player(&self) -> EntityId {
        self.player
    }

    /// Racers in grid order (AI pole first, the player last).
    pub fn racers(&self) -> &[EntityId] {
        &self.order
    }

    /// The AI field (debug views: `field().debug_of(k)` in AI entrant order).
    pub fn ai(&self) -> &AiEntrants {
        &self.ai
    }

    /// Whether the AI drives the player's car.
    pub fn autopilot(&self) -> bool {
        self.autopilot
    }

    /// `wp_opponent` events so far (overtakes, abandons, busts…).
    pub fn ai_events(&self) -> &[wp_opponent::OpponentEvent] {
        self.ai.events()
    }

    /// Day phase of the race (stake / economy).
    pub fn day_phase(&self) -> DayPhase {
        TimeOfDay::from_hours(self.def.time_of_day_h).phase()
    }

    /// Inputs for this tick, before `Sim::step_fixed`: every AI car (held on the handbrake
    /// until green) and the player's grid hold during the countdown: handbrake on, clutch
    /// pedal down and the player's own throttle (see [`LiveRace::set_grid_throttle`]), so the
    /// engine revs freely on the line without moving the car. `vehicles`: every sim
    /// vehicle; `events`: the previous tick's events; `police`: e.g. `SimPolice(sim.police())`.
    pub fn pre_step(
        &mut self,
        tick: u64,
        world: &World,
        vehicles: &[VehicleObs],
        events: &[SimEvent],
        police: &dyn PoliceQuery,
    ) -> Vec<(EntityId, VehicleInput)> {
        let mut out = self
            .ai
            .tick(&self.judge, tick, world, vehicles, events, police);
        if !self.autopilot && self.in_countdown(tick) {
            out.push((
                self.player,
                VehicleInput {
                    handbrake: 1.0,
                    clutch: 1.0,
                    throttle: self.grid_throttle,
                    ..VehicleInput::default()
                },
            ));
        }
        out
    }

    /// The player's throttle while held on the grid (0..=1): revs the engine during the
    /// countdown through the grid hold's declutched driveline. Ignored once green.
    pub fn set_grid_throttle(&mut self, throttle: f32) {
        self.grid_throttle = throttle.clamp(0.0, 1.0);
    }

    /// The server moved a racer on purpose (reset, respawn): not judged as a teleport.
    pub fn relocate(&mut self, racer: EntityId, pos: [f32; 3], tick: u64) {
        self.judge.relocate(racer, pos, tick);
    }

    /// The race has not gone green yet at `tick` (cars must stay on the grid).
    pub fn in_countdown(&self, tick: u64) -> bool {
        match self.judge.phase() {
            RacePhase::Lobby => true,
            RacePhase::Countdown { green_tick } => tick < green_tick,
            _ => false,
        }
    }

    /// Seconds to the green light (0 once racing).
    pub fn countdown_s(&self, tick: u64) -> f32 {
        match self.judge.phase() {
            RacePhase::Countdown { green_tick } if tick < green_tick => {
                (green_tick - tick) as f32 * PHYSICS_DT
            }
            _ => 0.0,
        }
    }

    /// Race clock, s (negative during the countdown).
    pub fn elapsed_s(&self, tick: u64) -> f32 {
        (tick as f32 - self.judge.green_tick() as f32) * PHYSICS_DT
    }

    /// Feed one stepped tick: `samples` for the racers, the tick's sim `events`, and the
    /// police vehicles currently in the sim (contacts with them are not civilian hits; the
    /// judge remembers every police id it has seen, see [`RaceJudge::add_police`]).
    pub fn post_step(
        &mut self,
        tick: u64,
        samples: &[RacerSample],
        events: &[SimEvent],
        police: &[EntityId],
    ) -> StepOut {
        self.last_tick = tick;
        self.judge.add_police(police);
        for e in events {
            if let SimEvent::Police(PoliceEvent::PursuitEnded { racer, outcome, .. }) = e {
                if outcome.is_capture() {
                    self.judge.dnf(*racer, DnfReason::Captured, tick);
                }
            }
        }
        self.judge.observe(tick, samples, events);
        self.ai.report_to(&mut self.judge, tick);

        let mut out = StepOut {
            resets: self.judge.drain_resets(),
            ..StepOut::default()
        };
        for sig in self.judge.drain_signals() {
            match &sig {
                RaceSignal::Green { .. } => self.on_green(samples, &mut out),
                RaceSignal::CheckpointPassed { racer, gate, .. } => {
                    let n = self.race.course.gates.len().max(1) as u32;
                    self.next_gate.insert(*racer, (gate + 1) % n);
                }
                RaceSignal::RaceFinished { .. } => {
                    self.final_results = Some(self.judge.results());
                }
                _ => {}
            }
            out.signals.push(sig);
        }
        if tick.is_multiple_of(HISTORY_EVERY_TICKS) && !self.in_countdown(tick) {
            for r in self.judge.results() {
                if r.status == RacerStatus::Running {
                    self.history
                        .entry(r.racer)
                        .or_default()
                        .push((tick, r.progress_m));
                }
            }
        }
        out
    }

    /// Green light: a hot meet calls in a patrol (`PatrolAttracted` + one `Reckless` report
    /// per racer near the venue), and illegal races put their stake heat on every racer.
    fn on_green(&mut self, samples: &[RacerSample], out: &mut StepOut) {
        if self.def.legality == Legality::Sanctioned {
            return;
        }
        if let Some(sig) = self
            .meet
            .record_race(&self.venue, self.start_abs_s, &self.meet_rules)
        {
            let pos: Vec<(EntityId, [f32; 3])> = samples.iter().map(|s| (s.racer, s.pos)).collect();
            out.reports = crate::police::report_events(&sig, &pos);
            out.signals.push(sig);
        }
        if self.stake_heat > 0.0 {
            out.add_heat = self.order.iter().map(|e| (*e, self.stake_heat)).collect();
        }
    }

    /// Whether the race is over (results final).
    pub fn is_finished(&self) -> bool {
        self.judge.is_finished()
    }

    /// Final results, once the race is over.
    pub fn final_results(&self) -> Option<&[RacerResult]> {
        self.final_results.as_deref()
    }

    /// Live standings, position order.
    pub fn standings(&self) -> Vec<Standing> {
        let results = self.judge.results();
        let laps = self.def.laps();
        let now = self.last_tick;
        let elapsed_now = self.elapsed_s(now).max(0.0);
        let mut out: Vec<Standing> = results
            .iter()
            .map(|r| Standing {
                racer: r.racer,
                position: r.position,
                status: r.status,
                lap: (r.laps_completed + 1).min(laps),
                next_gate: self.next_gate.get(&r.racer).copied().unwrap_or(0),
                gates_passed: r.gates_passed,
                progress_m: r.progress_m,
                gap_ahead_s: None,
                gap_behind_s: None,
                elapsed_s: r.elapsed_s().unwrap_or(elapsed_now),
            })
            .collect();
        for i in 1..out.len() {
            let gap = self.gap_s(&results[i - 1], &results[i], now);
            out[i].gap_ahead_s = gap;
            out[i - 1].gap_behind_s = gap;
        }
        out
    }

    /// How long ago `ahead` was where `behind` is now, s. Finishers compare finish times.
    fn gap_s(&self, ahead: &RacerResult, behind: &RacerResult, now: u64) -> Option<f32> {
        if let (Some(a), Some(b)) = (ahead.elapsed_s(), behind.elapsed_s()) {
            return Some(b - a);
        }
        if behind.status != RacerStatus::Running {
            return None;
        }
        let h = self.history.get(&ahead.racer)?;
        let i = h.partition_point(|(_, p)| *p < behind.progress_m);
        let (t, _) = *h.get(i)?;
        Some(now.saturating_sub(t) as f32 * PHYSICS_DT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog;

    fn grid_city() -> World {
        World::procedural(
            &wp_world::procgen::ProcGenConfig::default(),
            wp_world::WorldConfig::default(),
        )
    }

    #[test]
    fn grid_puts_the_player_last_and_the_meet_report_fires_when_hot() {
        let world = grid_city();
        let mut def = catalog::race("grid_highway_sprint").unwrap();
        let race = catalog::plan_course(&world, &def).unwrap();
        let slots = grid(&def, &race);
        assert_eq!(slots.len(), def.entrants());
        // Pole is closest to the start line; the player's slot is furthest back.
        let (start, dir) = race.course.start();
        let along = |p: [f32; 3]| (p[0] - start[0]) * dir[0] + (p[1] - start[1]) * dir[1];
        let last = along(slots.last().unwrap().pos);
        assert!(slots.iter().all(|s| along(s.pos) >= last - 1e-3));

        let car = wp_contracts::load_car(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/cars/debug_sedan.json"
        ))
        .unwrap();
        let cap = CarCapability::geometry_only(&car);
        let entrant = |i: u32| LiveEntrant {
            entity: EntityId(i),
            redline_rpm: 7000.0,
            capability: cap,
        };
        def.prior_meets = 2;
        let ai: Vec<LiveEntrant> = (2..=def.entrants() as u32).map(entrant).collect();
        let mut live = LiveRace::new(&def, &world, race, &ai, entrant(1), 0, 23 * 3600, false);
        // The player is held through the countdown.
        let held = live.pre_step(0, &world, &[], &[], &wp_opponent::NoPolice);
        assert!(held.iter().any(|(e, i)| *e == EntityId(1)
            && i.handbrake == 1.0
            && i.clutch == 1.0
            && i.throttle == 0.0));
        // ...and may rev on the line: the throttle passes through the hold.
        live.set_grid_throttle(0.8);
        let revving = live.pre_step(1, &world, &[], &[], &wp_opponent::NoPolice);
        assert!(revving
            .iter()
            .any(|(e, i)| *e == EntityId(1) && i.handbrake == 1.0 && i.throttle == 0.8));
        assert!(live.countdown_s(0) > 2.0);
        let samples: Vec<RacerSample> = slots
            .iter()
            .zip(live.racers().to_vec())
            .map(|(s, e)| RacerSample {
                racer: e,
                pos: s.pos,
                vel: [0.0; 3],
                speed_mps: 0.0,
                engine_rpm: 1000.0,
                gear: 1,
                throttle: 0.0,
            })
            .collect();
        let green = live.judge().green_tick();
        let out = live.post_step(green, &samples, &[], &[]);
        assert!(out
            .signals
            .iter()
            .any(|s| matches!(s, RaceSignal::PatrolAttracted { .. })));
        assert_eq!(out.reports.len(), def.entrants());
        // After green nobody holds the player any more.
        let after = live.pre_step(green + 1, &world, &[], &[], &wp_opponent::NoPolice);
        assert!(!after.iter().any(|(e, _)| *e == EntityId(1)));
        // A capture is a DNF.
        let cap_ev = SimEvent::Police(PoliceEvent::PursuitEnded {
            racer: EntityId(2),
            outcome: wp_contracts::PursuitOutcome::Captured,
            level: wp_contracts::WantedLevel::L1,
            capturer: None,
            assists: Vec::new(),
        });
        live.post_step(green + 2, &samples, &[cap_ev], &[]);
        let st = live.standings();
        let two = st.iter().find(|s| s.racer == EntityId(2)).unwrap();
        assert_eq!(two.status, RacerStatus::Dnf(DnfReason::Captured));

        // A police unit seen on one tick and despawned (retired) by the tick its contact is
        // reported: still police, so a contact, not a civilian hit.
        let unit = EntityId(500);
        live.post_step(green + 3, &samples, &[], &[unit]);
        let bump = SimEvent::Contact {
            a: EntityId(3),
            b: unit,
            point: [0.0; 3],
            normal: [1.0, 0.0, 0.0],
            impulse_ns: 5000.0,
            rel_speed: 4.0,
        };
        live.post_step(green + 4, &samples, &[bump], &[]);
        let three = live
            .judge()
            .results()
            .into_iter()
            .find(|r| r.racer == EntityId(3))
            .unwrap();
        assert_eq!(three.civilian_hits, 0);
        assert_eq!(three.contacts, 1);
    }
}
