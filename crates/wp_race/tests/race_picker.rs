//! Every race in the browser's picker (`wp_race::catalog`), raced to the finish in the full
//! `wp_sim` loop from its simcli scenario (`apps/simcli/scenarios/race/pick_<id>.ron`), with
//! `wp_opponent` AI in every seat (the player's car on autopilot) and the same glue the
//! browser session uses: `LiveRace` before/after each tick, a hot meet's report delivered to
//! the sim's police, the illegal stake's heat on every racer.
//!
//! Expectations are ranges (winner time, finishers, incidents), not exact numbers; the
//! determinism test compares hashes and results of two runs.

use std::path::{Path, PathBuf};

use simcli::run::build_sim;
use simcli::{load_scenario, FsData, RunOptions};
use wp_contracts::{DamageSim, EntityId, PoliceEvent, SimEvent, VehicleSim};
use wp_opponent::{CarCapability, SimPolice, VehicleObs};
use wp_race::catalog::{self, RaceDef};
use wp_race::live::{grid, LiveEntrant, LiveRace};
use wp_race::{RaceSignal, RacerResult, RacerSample, RacerStatus};
use wp_sim::{Sim, VehicleKind};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
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

/// What happened in one race.
struct Raced {
    def: RaceDef,
    results: Vec<RacerResult>,
    signals: Vec<RaceSignal>,
    finished_tick: Option<u64>,
    green_tick: u64,
    route_m: f32,
    pursuits: u32,
    /// Each racer's police heat just after the green light (meet report + stake heat).
    heat_after_green: Vec<f32>,
    sim_hash: u64,
}

fn run_race(id: &str, max_s: u64) -> Raced {
    let mut def = catalog::race(id).unwrap();
    let path = repo().join(format!("apps/simcli/scenarios/race/pick_{id}.ron"));
    let mut sc = load_scenario(&path).unwrap();
    // Tuning knobs (exploration only): another seed / police presence / traffic density.
    if let Some(seed) = std::env::var("WP_RACE_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        def.seed = seed;
        sc.seed = seed;
    }
    if let Some(p) = std::env::var("WP_RACE_PRESENCE")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        sc.police_presence = Some(p);
    }
    if std::env::var("WP_RACE_NOPARK").is_ok() {
        sc.parked_cars = false;
    }
    if let Some(d) = std::env::var("WP_RACE_DENSITY")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        sc.traffic_density = Some(d);
    }
    assert_eq!(sc.name, def.scenario_name());
    let data_src = FsData::new(repo().join("data"));
    let built = build_sim(&sc, &data_src, &RunOptions::default()).unwrap();
    let mut sim = built.sim;
    let ids = built.ids;
    let race = catalog::plan_course(sim.world(), &def).expect("course plans");
    let route_m = race.route.length_m;
    assert!(
        (route_m - def.length_hint_m).abs() < 0.02 * def.length_hint_m,
        "{id}: route {route_m:.0} m vs hint {}",
        def.length_hint_m
    );
    // The scenario's cars stand on the judge's grid: AI field first, the player last.
    let slots = grid(&def, &race);
    let player_slot = slots.last().unwrap();
    let order: Vec<usize> = (1..ids.len()).chain(std::iter::once(0)).collect();
    for (slot, &i) in slots.iter().zip(&order) {
        let spec = &sc.entities[i];
        assert!(
            (slot.pos[0] - spec.pos[0]).abs() < 0.01 && (slot.pos[1] - spec.pos[1]).abs() < 0.01,
            "{id}/{}: slot {:?} vs spawn {:?}",
            spec.id,
            slot.pos,
            spec.pos
        );
    }
    assert_eq!(sc.entities[0].pos[0], player_slot.pos[0]);

    let entrant = |i: usize| {
        let car = &built.cars[&sc.entities[i].car];
        LiveEntrant {
            entity: ids[i],
            redline_rpm: car.def.engine.redline_rpm,
            capability: CarCapability::geometry_only(car),
        }
    };
    let ai: Vec<LiveEntrant> = (1..ids.len()).map(entrant).collect();
    for e in &ai {
        sim.set_vehicle_kind(e.entity, VehicleKind::AiRacer)
            .unwrap();
    }
    let abs_s = (def.time_of_day_h * 3600.0) as u64;
    let mut live = LiveRace::new(
        &def,
        sim.world(),
        race,
        &ai,
        entrant(0),
        sim.tick(),
        abs_s,
        true,
    );
    let green_tick = live.judge().green_tick();
    let racers = live.racers().to_vec();

    let mut events: Vec<SimEvent> = Vec::new();
    let mut signals = Vec::new();
    let mut pursuits = 0;
    let mut heat_after_green = Vec::new();
    let mut finished_tick = None;
    while sim.tick() < 240 * max_s {
        let inputs = live.pre_step(
            sim.tick(),
            sim.world(),
            &obs(&sim),
            &events,
            &SimPolice(sim.police()),
        );
        for (e, inp) in inputs {
            sim.set_input(e, inp).unwrap();
        }
        sim.step_fixed();
        events = sim.drain_events();
        pursuits += events
            .iter()
            .filter(|e| {
                matches!(e, SimEvent::Police(PoliceEvent::PursuitStarted { racer, .. })
                    if racers.contains(racer))
            })
            .count() as u32;
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
        let out = live.post_step(sim.tick(), &samples, &events, &police);
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
            wp_race::police::deliver(&mut p, sim.world(), &views, &out.reports);
            sim.set_police(p);
        }
        for (e, h) in &out.add_heat {
            let cur = sim.police().racer(*e).map_or(0.0, |r| r.heat);
            sim.police_mut().set_heat(*e, cur + h);
        }
        signals.extend(out.signals);
        if sim.tick() == green_tick + 1 {
            heat_after_green = racers
                .iter()
                .map(|e| sim.police().racer(*e).map_or(0.0, |r| r.heat))
                .collect();
        }
        if std::env::var("WP_RACE_TRACE").is_ok() && sim.tick().is_multiple_of(240 * 10) {
            for s in live.standings() {
                let st = sim
                    .vehicles()
                    .iter()
                    .find(|v| v.id == s.racer)
                    .map(|v| v.vehicle.state())
                    .unwrap();
                eprintln!(
                    "{id} t={} {:?} P{} {:?} gates {} prog {:.0} pos ({:.0},{:.0}) v {:.1}",
                    sim.tick() / 240,
                    s.racer,
                    s.position,
                    s.status,
                    s.gates_passed,
                    s.progress_m,
                    st.pos[0],
                    st.pos[1],
                    st.speed_mps
                );
            }
        }
        if std::env::var("WP_RACE_TRACE_NEAR").is_ok() && sim.tick().is_multiple_of(240 * 20) {
            let lead = sim
                .vehicles()
                .iter()
                .find(|v| v.id == racers[0])
                .unwrap()
                .vehicle
                .state()
                .pos;
            for a in sim.world().agents() {
                let d = (a.pos[0] - lead[0]).hypot(a.pos[1] - lead[1]);
                if d < 50.0 {
                    eprintln!(
                        "  agent {:?} {:?} pos ({:.0},{:.0}) v {:.1} yield {} hazard {}",
                        a.id, a.lod, a.pos[0], a.pos[1], a.speed_mps, a.yielding, a.hazard_lights
                    );
                }
            }
            for v in sim
                .vehicles()
                .iter()
                .filter(|v| v.kind == VehicleKind::Police)
            {
                let p = v.vehicle.state().pos;
                eprintln!(
                    "  police {:?} pos ({:.0},{:.0}) v {:.1} siren {}",
                    v.id,
                    p[0],
                    p[1],
                    v.vehicle.state().speed_mps,
                    v.input.siren
                );
            }
        }
        if live.is_finished() {
            finished_tick = Some(sim.tick());
            break;
        }
    }
    let results = live
        .final_results()
        .map(<[RacerResult]>::to_vec)
        .unwrap_or_else(|| live.judge().results());
    for x in &results {
        eprintln!(
            "{id}: {:?} P{} {:?} t={:?} gates {} contacts {} civ {}",
            x.racer,
            x.position,
            x.status,
            x.elapsed_s(),
            x.gates_passed,
            x.contacts,
            x.civilian_hits
        );
    }
    eprintln!(
        "{id}: route {route_m:.0} m, finished at {:?} s, pursuits {pursuits}",
        finished_tick.map(|t| t as f32 / 240.0)
    );
    Raced {
        def,
        results,
        signals,
        finished_tick,
        green_tick,
        route_m,
        pursuits,
        heat_after_green,
        sim_hash: sim.state_hash(),
    }
}

/// Shared expectations: the race ends, a winner inside `winner_s`, at least `min_finish`
/// finishers, nobody still running, every non-finisher has a reason.
fn check(r: &Raced, winner_s: std::ops::Range<f32>, min_finish: usize) {
    let id = r.def.id;
    assert!(r.finished_tick.is_some(), "{id}: never finished");
    assert!(r.finished_tick.unwrap() > r.green_tick);
    assert_eq!(r.results.len(), r.def.entrants());
    let winner = r
        .results
        .iter()
        .find(|x| x.position == 1)
        .expect("a winner");
    assert_eq!(
        winner.status,
        RacerStatus::Finished,
        "{id}: winner {winner:?}"
    );
    let t = winner.elapsed_s().unwrap();
    assert!(
        winner_s.contains(&t),
        "{id}: winner took {t:.1} s, want {winner_s:?}"
    );
    let finished = r
        .results
        .iter()
        .filter(|x| x.status == RacerStatus::Finished)
        .count();
    assert!(
        finished >= min_finish,
        "{id}: only {finished} of {} finished",
        r.results.len()
    );
    assert!(r.results.iter().all(|x| x.status != RacerStatus::Running));
    assert!(matches!(
        r.signals.last(),
        Some(RaceSignal::RaceFinished { .. })
    ));
    // Nobody wrecks a car (one racing incident, a DQ or a fled AI, is allowed per race).
    let incidents = r
        .results
        .iter()
        .filter(|x| x.status != RacerStatus::Finished)
        .count();
    assert!(
        r.results
            .iter()
            .all(|x| x.status != RacerStatus::Dnf(wp_race::DnfReason::TotalLoss)),
        "{id}: a car was wrecked"
    );
    assert!(incidents <= r.def.entrants() - min_finish);
}

#[test]
fn grid_highway_sprint() {
    let r = run_race("grid_highway_sprint", 150);
    // 747 m up a signalled highway with thin traffic.
    check(&r, 15.0..60.0, 3);
}

#[test]
fn grid_highway_sprint_is_deterministic() {
    let a = run_race("grid_highway_sprint", 150);
    let b = run_race("grid_highway_sprint", 150);
    assert_eq!(a.sim_hash, b.sim_hash);
    assert_eq!(a.results, b.results);
}

#[test]
fn hollins_hill_sprint() {
    let r = run_race("hollins_sprint", 200);
    // ~1 km uphill through six hairpins (the AI-only Hollins race: winner ~65 s).
    check(&r, 40.0..110.0, 3);
}

#[test]
fn loop9_circuit() {
    let r = run_race("loop9_circuit", 600);
    // One 10.1 km lap of the highway loop through late-night traffic.
    check(&r, 150.0..420.0, 4);
    assert!(r.route_m > 9000.0);
    // The meet at the Loop 9 grid is hot: the green light calls in a patrol.
    assert!(r
        .signals
        .iter()
        .any(|s| matches!(s, RaceSignal::PatrolAttracted { .. })));
}

#[test]
fn lantern_checkpoint_draws_the_police() {
    let r = run_race("lantern_checkpoint", 300);
    // 665 m zig-zag through daytime downtown (signals, junction queues): slow but done.
    check(&r, 40.0..170.0, 2);
    // A hot meet in broad daylight: a patrol is called to the grid, and every racer carries
    // the meet report's heat plus the illegal-daytime stake heat from the green light on.
    assert!(r
        .signals
        .iter()
        .any(|s| matches!(s, RaceSignal::PatrolAttracted { .. })));
    assert_eq!(r.heat_after_green.len(), r.def.entrants());
    for h in &r.heat_after_green {
        assert!(*h >= 4.0, "heat after green {:?}", r.heat_after_green);
    }
    eprintln!("lantern pursuits: {}", r.pursuits);
}
