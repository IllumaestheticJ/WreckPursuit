//! A night street sprint on Port Vesper v2's Hollins Drive (hairpins, ~11 % grades, crests),
//! with `wp_opponent` AI entrants, in the full `wp_sim` loop
//! (`apps/simcli/scenarios/race/vesper_hollins_sprint.ron`).
//!
//! Same lifecycle as `race_police.rs` (judge + grid, `wp_race::entrants`, judge fed every tick)
//! on a hilly map: the course comes from the city's own `Hollins Ascent → Knob Observatory Lot`
//! route, the grid sits on the climb, and the AI has to race it to the top.

use std::path::{Path, PathBuf};

use simcli::run::build_sim;
use simcli::{load_scenario, FsData, RunOptions};
use wp_contracts::{DamageSim, VehicleSim};
use wp_opponent::{CarCapability, DriverProfile, EventHeat, VehicleObs};
use wp_race::bridge::course_from_route;
use wp_race::entrants::{AiEntrant, AiEntrants};
use wp_race::{
    DnfReason, EventId, RaceData, RaceJudge, RaceKind, RacerEntry, RacerResult, RacerSample,
    RacerStatus,
};
use wp_sim::{Sim, VehicleKind};
use wp_world::Route;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The city's named hill route (the same one `city/hill_road_ascent.ron` drives).
fn hollins_route(sim: &Sim) -> Route {
    let pois = &sim.world().network().pois;
    let poi = |name: &str| pois.iter().position(|p| p.name == name).unwrap();
    sim.world()
        .plan_route_between_pois(poi("Hollins Ascent"), poi("Knob Observatory Lot"))
        .expect("Hollins Drive route")
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

/// What happened in one hill sprint.
struct HillRace {
    results: Vec<RacerResult>,
    finished_tick: Option<u64>,
    green_tick: u64,
    route_m: f32,
    sim_hash: u64,
}

fn run_hill_sprint() -> HillRace {
    let sc = load_scenario(&repo().join("apps/simcli/scenarios/race/vesper_hollins_sprint.ron"))
        .unwrap();
    let data_src = FsData::new(repo().join("data"));
    let built = build_sim(&sc, &data_src, &RunOptions::default()).unwrap();
    let mut sim = built.sim;
    let ids = built.ids;
    let route = hollins_route(&sim);
    let route_m = route.length_m;
    let mut course = course_from_route(&route, 3.0);

    let data = RaceData::embedded();
    let kind = RaceKind::Sprint;
    // The grid inside the lanes, as the picker lays it out (its right column would start on
    // the verge of this one-lane-each-way street).
    let rules = data.rules.for_kind(kind);
    wp_race::catalog::fit_grid_to_road(
        sim.world(),
        &mut course,
        rules.kind_rules.grid,
        rules.kind_rules.max_racers as usize,
        sc.entities.len(),
    );
    let mut judge = RaceJudge::new(EventId(16), data.rules.for_kind(kind), course.clone());
    let mut entrants = Vec::new();
    for (spec, id) in sc.entities.iter().zip(&ids) {
        let car = &built.cars[&spec.car];
        let slot = judge
            .enter(RacerEntry {
                racer: *id,
                redline_rpm: car.def.engine.redline_rpm,
            })
            .unwrap();
        let yaw = wp_world::geom::yaw_of(slot.dir);
        assert!(
            (slot.pos[0] - spec.pos[0]).abs() < 0.01
                && (slot.pos[1] - spec.pos[1]).abs() < 0.01
                && (yaw - spec.yaw).abs() < 0.001,
            "{}: grid slot ({:.2}, {:.2}, {:.2}) yaw {:.4} vs spawn {:?} yaw {}",
            spec.id,
            slot.pos[0],
            slot.pos[1],
            slot.pos[2],
            yaw,
            spec.pos,
            spec.yaw
        );
        sim.set_vehicle_kind(*id, VehicleKind::AiRacer).unwrap();
        entrants.push(AiEntrant {
            racer: *id,
            profile: DriverProfile::default(),
            capability: CarCapability::for_car(car),
        });
    }
    let race = wp_race::bridge::CityRace { route, course };
    let mut ai = AiEntrants::new(sim.world(), &race, kind, false, &entrants, 16);
    let green_tick = judge.start(sim.tick()).unwrap();

    let heat = EventHeat::new();
    let mut events = Vec::new();
    let mut finished_tick = None;
    let max_ticks = 240 * 150;
    while sim.tick() < max_ticks {
        let tick = sim.tick();
        let inputs = ai.tick(&judge, tick, sim.world(), &obs(&sim), &events, &heat);
        for (e, inp) in inputs {
            sim.set_input(e, inp).unwrap();
        }
        sim.step_fixed();
        events = sim.drain_events();
        let tick = sim.tick();
        if std::env::var("WP_RACE_TRACE").is_ok() && tick.is_multiple_of(240) {
            for v in sim.vehicles().iter().filter(|v| ids.contains(&v.id)) {
                let st = v.vehicle.state();
                eprintln!(
                    "t={:.0} {:?} pos ({:.1},{:.1},{:.1}) v {:.1}",
                    tick as f32 / 240.0,
                    v.id,
                    st.pos[0],
                    st.pos[1],
                    st.pos[2],
                    st.speed_mps
                );
            }
        }
        let samples: Vec<RacerSample> = sim
            .vehicles()
            .iter()
            .filter(|v| ids.contains(&v.id))
            .map(|v| RacerSample::from_state(v.id, &v.vehicle.state(), v.input.throttle))
            .collect();
        judge.observe(tick, &samples, &events);
        ai.report_to(&mut judge, tick);
        judge.drain_signals();
        if judge.is_finished() {
            finished_tick = Some(tick);
            break;
        }
    }
    HillRace {
        results: judge.results(),
        finished_tick,
        green_tick,
        route_m,
        sim_hash: sim.state_hash(),
    }
}

#[test]
fn hollins_drive_sprint_is_raced_to_the_top() {
    let r = run_hill_sprint();
    eprintln!(
        "route {:.0} m, green {} finished {:?}",
        r.route_m, r.green_tick, r.finished_tick
    );
    for x in &r.results {
        eprintln!(
            "  {:?} pos {} {:?} t={:?} gates {} contacts {} viol {:?}",
            x.racer,
            x.position,
            x.status,
            x.elapsed_s(),
            x.gates_passed,
            x.contacts,
            x.violations
        );
    }
    assert!(r.finished_tick.is_some(), "race never finished");
    let winner = r
        .results
        .iter()
        .find(|x| x.status == RacerStatus::Finished && x.position == 1)
        .expect("a winner");
    // ~1.2 km uphill through six hairpins: slower than the flat arterial, faster than the
    // 25 mph route driver (~105 s).
    let t = winner.elapsed_s().unwrap();
    assert!((40.0..105.0).contains(&t), "winner took {t:.1} s");
    // The field reaches the top. One racing incident is allowed (at seed 16 the last car
    // follows another over the summit crest and runs into it when it brakes for the bend at
    // the line, ends up off the road short of the last gate and runs out the finish window), but
    // nobody totals a car on the hill.
    let finished = r
        .results
        .iter()
        .filter(|x| x.status == RacerStatus::Finished)
        .count();
    assert!(
        finished >= 3,
        "only {finished} of {} finished",
        r.results.len()
    );
    for x in &r.results {
        assert!(
            matches!(
                x.status,
                RacerStatus::Finished | RacerStatus::Dnf(DnfReason::FinishWindow)
            ),
            "{:?}: {:?}",
            x.racer,
            x.status
        );
        assert!(
            x.gates_passed >= 4,
            "{:?} stopped at gate {}",
            x.racer,
            x.gates_passed
        );
    }
}

#[test]
fn hollins_drive_sprint_is_deterministic() {
    let a = run_hill_sprint();
    let b = run_hill_sprint();
    assert_eq!(a.sim_hash, b.sim_hash);
    assert_eq!(a.results, b.results);
}
