//! A street race that pulls in the police, with AI entrants, in the full `wp_sim` loop
//! (`apps/simcli/scenarios/race/city_sprint_police.ron`: traffic and police on).
//!
//! The server-side race lifecycle runs here exactly as `wp_net` will run it: judge + grid,
//! `wp_opponent` AI entrants (`wp_race::entrants`), meet heat → `PatrolAttracted` →
//! `wp_race::police` reports into `wp_police`, judge fed every tick.

use std::path::{Path, PathBuf};

use simcli::run::build_sim;
use simcli::{load_scenario, FsData, RunOptions};
use wp_contracts::{DamageSim, EntityId, SimEvent, VehicleSim};
use wp_opponent::{CarCapability, DriverProfile, EventHeat, VehicleObs};
use wp_police::{PoliceEvent, PoliceEventExt as _, RacerView};
use wp_race::bridge::{plan_city_race, CityRace};
use wp_race::entrants::{AiEntrant, AiEntrants};
use wp_race::schedule::Venue;
use wp_race::{
    police, DnfReason, EventId, MeetHeat, RaceData, RaceJudge, RaceKind, RaceSignal, RacerEntry,
    RacerResult, RacerSample, RacerStatus, VenueId,
};
use wp_sim::{Sim, VehicleKind};
use wp_world::World;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn plan_sprint(world: &World) -> CityRace {
    plan_city_race(
        world,
        [60.55, -437.0, 0.0],
        [0.0, 1.0],
        [60.55, -230.0, 0.0],
        3.0,
    )
    .expect("arterial sprint route")
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

/// What happened in one police race.
struct PoliceRace {
    results: Vec<RacerResult>,
    signals: Vec<RaceSignal>,
    /// `(tick, racer)` of every pursuit start.
    pursuits: Vec<(u64, EntityId)>,
    /// Tick the race finished (judge), if it did.
    finished_tick: Option<u64>,
    green_tick: u64,
    heat_after_report: Vec<f32>,
    sim_hash: u64,
    ai_events: usize,
}

fn run_police_race(call_it_in: bool) -> PoliceRace {
    let sc =
        load_scenario(&repo().join("apps/simcli/scenarios/race/city_sprint_police.ron")).unwrap();
    let data_src = FsData::new(repo().join("data"));
    let built = build_sim(&sc, &data_src, &RunOptions::default()).unwrap();
    let mut sim = built.sim;
    let ids = built.ids;
    let race = plan_sprint(sim.world());

    // Judge + grid: the scenario spawns sit on the judge's slots.
    let data = RaceData::embedded();
    let kind = RaceKind::Sprint;
    let mut judge = RaceJudge::new(EventId(15), data.rules.for_kind(kind), race.course.clone());
    let mut entrants = Vec::new();
    for (spec, id) in sc.entities.iter().zip(&ids) {
        let car = &built.cars[&spec.car];
        let slot = judge
            .enter(RacerEntry {
                racer: *id,
                redline_rpm: car.def.engine.redline_rpm,
            })
            .unwrap();
        assert!(
            (slot.pos[0] - spec.pos[0]).abs() < 0.01 && (slot.pos[1] - spec.pos[1]).abs() < 0.01,
            "{}: grid slot {:?} vs spawn {:?}",
            spec.id,
            slot.pos,
            spec.pos
        );
        sim.set_vehicle_kind(*id, VehicleKind::AiRacer).unwrap();
        entrants.push(AiEntrant {
            racer: *id,
            profile: DriverProfile::default(),
            // Generic limits keep debug test runs fast; the server measures each car once.
            capability: CarCapability::geometry_only(car),
        });
    }
    // Open roads (illegal night meet): the AI may only borrow opposing lanes to overtake.
    let mut ai = AiEntrants::new(sim.world(), &race, kind, false, &entrants, 15);
    let green_tick = judge.start(sim.tick()).unwrap();

    // Two earlier meets tonight at this venue: this one makes it hot.
    let venue = Venue {
        id: VenueId(1),
        name: "arterial north".into(),
        pos: race.course.start().0,
        district: None,
        closed_circuit: false,
    };
    let rules = &data.schedule.meet_heat;
    let mut meet = MeetHeat::default();
    let tonight = 22 * 3600;
    assert!(meet.record_race(&venue, tonight - 3600, rules).is_none());
    assert!(meet.record_race(&venue, tonight - 1800, rules).is_none());

    let mut heat = EventHeat::new();
    let mut events: Vec<SimEvent> = Vec::new();
    let mut signals = Vec::new();
    let mut pursuits = Vec::new();
    let mut finished_tick = None;
    let mut heat_after_report = Vec::new();
    let max_ticks = 240 * 120;
    while sim.tick() < max_ticks {
        let tick = sim.tick();
        if call_it_in && tick == green_tick {
            // The race starts: record it at the venue (→ PatrolAttracted) and call it in.
            if let Some(sig) = meet.record_race(&venue, tonight, rules) {
                let racers: Vec<(EntityId, [f32; 3])> = sim
                    .vehicle_states()
                    .iter()
                    .filter(|(id, _)| ids.contains(id))
                    .map(|(id, s)| (*id, s.pos))
                    .collect();
                let reports = police::report_events(&sig, &racers);
                assert_eq!(reports.len(), ids.len(), "every entrant reported");
                let views: Vec<RacerView> = sim
                    .vehicle_states()
                    .iter()
                    .filter(|(id, _)| ids.contains(id))
                    .map(|(id, s)| RacerView {
                        entity: *id,
                        pos: s.pos,
                        vel: s.lin_vel,
                    })
                    .collect();
                let mut p = sim.police().clone();
                let out = police::deliver(&mut p, sim.world(), &views, &reports);
                sim.set_police(p);
                events.extend(out);
                heat_after_report = ids
                    .iter()
                    .map(|id| sim.police().racer(*id).map_or(0.0, |r| r.heat))
                    .collect();
                signals.push(sig);
            }
        }
        heat.observe(&events);
        let inputs = ai.tick(&judge, tick, sim.world(), &obs(&sim), &events, &heat);
        for (e, inp) in inputs {
            sim.set_input(e, inp).unwrap();
        }
        sim.step_fixed();
        events = sim.drain_events();
        let tick = sim.tick();
        if tick < green_tick {
            // Held on the grid until green.
            for (spec, id) in sc.entities.iter().zip(&ids) {
                let p = sim
                    .vehicles()
                    .iter()
                    .find(|v| v.id == *id)
                    .unwrap()
                    .vehicle
                    .state()
                    .pos;
                let moved = ((p[0] - spec.pos[0]).powi(2) + (p[1] - spec.pos[1]).powi(2)).sqrt();
                assert!(moved < 0.3, "{} rolled {moved:.2} m before green", spec.id);
            }
        }
        if std::env::var("WP_POLICE_TRACE").is_ok() && tick.is_multiple_of(480) {
            let p = sim.police();
            eprintln!("t={:.0} units:", tick as f32 / 240.0);
            for u in p.units() {
                eprintln!(
                    "   {:?} {:?} at ({:.0},{:.0}) {:?} {:?}",
                    u.id, u.role, u.body.pos[0], u.body.pos[1], u.assignment, u.tactic
                );
            }
            for i in p.incidents() {
                eprintln!("   incident {:?} at {:?} unit {:?}", i.kind, i.pos, i.unit);
            }
            for id in &ids {
                if let Some(r) = p.racer(*id) {
                    eprintln!(
                        "   racer {:?} heat {:.1} {:?} {:?} at ({:.0},{:.0})",
                        id, r.heat, r.level, r.status, r.pos[0], r.pos[1]
                    );
                }
            }
        }
        if std::env::var("WP_RACE_TRACE").is_ok() && tick.is_multiple_of(120) {
            for v in sim.vehicles().iter().filter(|v| ids.contains(&v.id)) {
                let st = v.vehicle.state();
                eprintln!(
                    "t={:.1} {:?} pos ({:.1},{:.1}) v {:.1} in thr {:.2} brk {:.2} steer {:.2} tactic {:?}",
                    tick as f32 / 240.0, v.id, st.pos[0], st.pos[1], st.speed_mps,
                    v.input.throttle, v.input.brake, v.input.steer,
                    ai.field().tactic_of(ids.iter().position(|i| *i == v.id).unwrap())
                );
            }
        }
        for e in &events {
            if let Some(PoliceEvent::PursuitStarted { racer, .. }) = PoliceEvent::from_sim_event(e)
            {
                pursuits.push((tick, racer));
            }
        }
        let police_ids: Vec<EntityId> = sim
            .vehicles()
            .iter()
            .filter(|v| v.kind == VehicleKind::Police)
            .map(|v| v.id)
            .collect();
        judge.set_police(&police_ids);
        let samples: Vec<RacerSample> = sim
            .vehicles()
            .iter()
            .filter(|v| ids.contains(&v.id))
            .map(|v| RacerSample::from_state(v.id, &v.vehicle.state(), v.input.throttle))
            .collect();
        judge.observe(tick, &samples, &events);
        ai.report_to(&mut judge, tick);
        signals.extend(judge.drain_signals());
        if finished_tick.is_none() && judge.is_finished() {
            finished_tick = Some(tick);
        }
        // Stop once the race is over and a pursuit has started (or 20 s after the finish).
        if let Some(f) = finished_tick {
            if !pursuits.is_empty() || tick > f + 240 * 20 {
                break;
            }
        }
    }
    PoliceRace {
        results: judge.results(),
        signals,
        pursuits,
        finished_tick,
        green_tick,
        heat_after_report,
        sim_hash: sim.state_hash(),
        ai_events: ai.events().len(),
    }
}

#[test]
#[ignore = "L (round 8): contract v0.3.0 moved police patrol spawns, so this tuned sprint no \
            longer meets a patrol; re-aim it (see golden/city_chase's round-8 re-aim) and \
            un-ignore."]
fn city_sprint_pulls_in_police() {
    let r = run_police_race(true);
    eprintln!(
        "green {} finished {:?} pursuits {:?} heat {:?} ai events {}",
        r.green_tick, r.finished_tick, r.pursuits, r.heat_after_report, r.ai_events
    );
    for x in &r.results {
        eprintln!(
            "  {:?} pos {} {:?} t={:?} gates {} contacts {} civ {} viol {:?}",
            x.racer,
            x.position,
            x.status,
            x.elapsed_s(),
            x.gates_passed,
            x.contacts,
            x.civilian_hits,
            x.violations
        );
    }
    // The hot venue attracted a patrol and the report raised every entrant's heat.
    assert!(r
        .signals
        .iter()
        .any(|s| matches!(s, RaceSignal::PatrolAttracted { .. })));
    assert!(
        r.heat_after_report.iter().all(|h| *h > 0.0),
        "{:?}",
        r.heat_after_report
    );
    // The race ran: the AI entrants raced and the judge ranked them.
    assert!(r.finished_tick.is_some(), "race never finished");
    assert!(r
        .results
        .iter()
        .any(|x| x.status == RacerStatus::Finished && x.position == 1));
    // Racers either finished or ran from the police; none was wrecked or disqualified.
    for x in &r.results {
        assert!(
            matches!(
                x.status,
                RacerStatus::Finished | RacerStatus::Dnf(DnfReason::Fled | DnfReason::Captured)
            ),
            "{:?}: {:?}",
            x.racer,
            x.status
        );
    }
    // A pursuit started during or after the race.
    assert!(!r.pursuits.is_empty(), "no pursuit started");
    assert!(r.pursuits[0].0 >= r.green_tick);
}

#[test]
fn without_the_meet_report_nobody_is_chased() {
    // Control run: same race, same seed, but nobody calls the hot meet in. The racers still
    // speed and pick up some heat (a witnessed knock after the finish), but stay below wanted
    // level 1, so no pursuit starts in the same window. The meet report is what tips it.
    // (Tuning-sensitive by design: if police or traffic tuning changes, re-check both runs.)
    let r = run_police_race(false);
    assert!(r.finished_tick.is_some());
    assert!(
        !r.signals
            .iter()
            .any(|s| matches!(s, RaceSignal::PatrolAttracted { .. })),
        "control run must not call the meet in"
    );
    assert!(
        r.pursuits.is_empty(),
        "pursuits without a report: {:?}",
        r.pursuits
    );
}

#[test]
fn police_race_is_deterministic() {
    let a = run_police_race(true);
    let b = run_police_race(true);
    assert_eq!(a.sim_hash, b.sim_hash);
    assert_eq!(a.results, b.results);
    assert_eq!(a.pursuits, b.pursuits);
}
