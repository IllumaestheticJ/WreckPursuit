//! Judge real physics runs: simcli v2 scenarios in `apps/simcli/scenarios/race/` are run with
//! every tick's vehicle states and events fed to a `RaceJudge`, exactly as the server will do.
//! Also checks the `wp_world` bridge on the procedural city.

use std::path::{Path, PathBuf};

use simcli::{load_scenario, run_with_hook, FsData, RunOptions, Scenario};
use wp_contracts::{load_car, DayPhase, EntityId};
use wp_race::bridge::{
    activate_race_routes, closure_preview, course_from_route, plan_city_race, safehouses_from_map,
    venues_from_map, CityRace,
};
use wp_race::schedule::TrafficMode;
use wp_race::trace::{simulate, ScriptedRacer};
use wp_race::{
    Course, DqReason, EventId, MeetHeat, RaceData, RaceJudge, RaceKind, RacerEntry, RacerResult,
    RacerSample, RacerStatus, Scheduler,
};
use wp_world::procgen::ProcGenConfig;
use wp_world::{PropKind, World, WorldConfig};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Throttle the scenario applies to entity `i` at time `t_s` (last keyframe at or before).
fn throttle_at(sc: &Scenario, i: usize, t_s: f32) -> f32 {
    sc.entities[i]
        .inputs
        .iter()
        .rfind(|k| k.t_s <= t_s)
        .map_or(0.0, |k| k.input.throttle)
}

/// Run a race scenario and judge it. Racers enter in scenario order (= grid order).
fn judge_scenario(file: &str, kind: RaceKind, course: Course) -> Vec<RacerResult> {
    let sc = load_scenario(&repo().join("apps/simcli/scenarios/race").join(file)).unwrap();
    let data = RaceData::embedded();
    let mut judge = RaceJudge::new(EventId(1), data.rules.for_kind(kind), course);
    for (i, e) in sc.entities.iter().enumerate() {
        let car = load_car(repo().join("data/cars").join(format!("{}.json", e.car))).unwrap();
        let slot = judge
            .enter(RacerEntry {
                racer: EntityId(i as u32 + 1),
                redline_rpm: car.def.engine.redline_rpm,
            })
            .unwrap();
        // The scenario must place each car on its grid slot.
        assert!(
            (slot.pos[0] - e.pos[0]).abs() < 0.01 && (slot.pos[1] - e.pos[1]).abs() < 0.01,
            "{}: grid slot {:?} vs spawn {:?}",
            e.id,
            slot.pos,
            e.pos
        );
    }
    judge.start(0).unwrap();
    let dt = wp_contracts::PHYSICS_DT;
    let mut hook = |v: &simcli::run::TickView| -> Result<(), String> {
        let tick = v.sim.tick();
        let t_s = tick as f32 * dt;
        let samples: Vec<RacerSample> = v
            .sim
            .vehicle_states()
            .iter()
            // Scenario cars spawn first (ids 1..=n); police and traffic come after.
            .take(sc.entities.len())
            .enumerate()
            .map(|(i, (id, s))| RacerSample::from_state(*id, s, throttle_at(&sc, i, t_s)))
            .collect();
        judge.observe(tick, &samples, v.events);
        Ok(())
    };
    let out = run_with_hook(
        &sc,
        &FsData::new(repo().join("data")),
        &RunOptions::default(),
        &mut hook,
    )
    .unwrap();
    assert!(out.summary.passed(), "{:?}", out.summary.failures);
    assert!(
        judge.is_finished(),
        "race still running at the end of the scenario"
    );
    judge.results()
}

fn drag_course() -> Course {
    // A 12 m strip: two 6 m lanes.
    Course::straight([0.0, 3.0, 0.0], [0.0, 1.0], 402.0, 1000.0, 12.0)
}

#[test]
fn drag_coupe_beats_sedan_with_reaction_and_shifts() {
    let r = judge_scenario("drag_coupe_vs_sedan.ron", RaceKind::Drag, drag_course());
    let coupe = &r[0];
    let sedan = &r[1];
    assert_eq!(coupe.racer, EntityId(1));
    assert_eq!(coupe.status, RacerStatus::Finished);
    assert_eq!(sedan.status, RacerStatus::Finished);
    let et = coupe.elapsed_s().unwrap();
    assert!((11.0..16.0).contains(&et), "coupe ET {et}");
    let et_s = sedan.elapsed_s().unwrap();
    assert!(et_s > et + 1.0, "sedan ET {et_s}");
    let dc = coupe.drag.unwrap();
    let ds = sedan.drag.unwrap();
    assert!((dc.reaction_s().unwrap() - 0.20).abs() < 0.01);
    assert!((ds.reaction_s().unwrap() - 0.35).abs() < 0.01);
    assert!(dc.shifts >= 2, "{dc:?}");
    assert!((40.0..65.0).contains(&dc.trap_speed_mps), "{dc:?}");
    assert!(coupe.clean && sedan.clean);
}

#[test]
fn sprint_corridor_cutter_is_disqualified() {
    let course = Course::straight([0.0, 3.0, 0.0], [0.0, 1.0], 500.0, 100.0, 12.0);
    let r = judge_scenario("sprint_corridor_dq.ron", RaceKind::Sprint, course);
    assert_eq!(r[0].racer, EntityId(1));
    assert_eq!(r[0].status, RacerStatus::Finished);
    assert_eq!(r[0].gates_passed, 5);
    assert_eq!(r[1].status, RacerStatus::Dq(DqReason::LeftCorridor));
}

#[test]
fn judging_real_runs_is_deterministic() {
    let a = judge_scenario("drag_coupe_vs_sedan.ron", RaceKind::Drag, drag_course());
    let b = judge_scenario("drag_coupe_vs_sedan.ron", RaceKind::Drag, drag_course());
    assert_eq!(a, b);
}

#[test]
fn world_bridge_on_the_procedural_city() {
    let world = World::procedural(&ProcGenConfig::default(), WorldConfig::default());
    let map = world.map();
    let venues = venues_from_map(map);
    assert!(venues.len() >= 2, "{} race-start venues", venues.len());
    assert!(venues.iter().any(|v| v.closed_circuit));
    assert!(venues.iter().all(|v| v.district.is_some()));
    let _ = safehouses_from_map(map);

    // A sprint between two venues, judged with scripted racers along the planned route.
    let route = world
        .plan_route_between_pois(venues[0].id.0 as usize, venues[1].id.0 as usize)
        .expect("route between venues");
    let course = course_from_route(&route, 3.0);
    assert_eq!(course.gates.len(), route.checkpoints.len());
    let data = RaceData::embedded();
    let judge = RaceJudge::new(EventId(9), data.rules.for_kind(RaceKind::Sprint), course);
    let racers = [
        ScriptedRacer::new(1, 4.0, 20.0),
        ScriptedRacer::new(2, 4.0, 22.0),
    ];
    let out = simulate(judge, &racers, 4, 240 * 900);
    assert_eq!(out.results[0].racer, EntityId(2), "{:?}", out.results);
    assert!(out
        .results
        .iter()
        .all(|r| r.status == RacerStatus::Finished
            && r.gates_passed as usize == route.checkpoints.len()));

    // A circuit on a closed loop around a block near the core.
    let c = map.net.core;
    let s = 110.0;
    let poly = [
        [c[0] - s, c[1] - s],
        [c[0] - s, c[1] + s],
        [c[0] + s, c[1] + s],
        [c[0] + s, c[1] - s],
    ];
    let loop_route = world
        .plan_circuit(&poly)
        .expect("circuit around the core block");
    {
        let course = course_from_route(&loop_route, 3.0);
        assert!(course.closed_loop);
        let judge = RaceJudge::new(
            EventId(10),
            data.rules.for_kind(RaceKind::Circuit { laps: 2 }),
            course,
        );
        let out = simulate(judge, &racers, 4, 240 * 1200);
        assert!(
            out.results.iter().all(|r| r.laps_completed == 2),
            "{:?}",
            out.results
        );
    }

    // Calendars from real venues.
    let sched = Scheduler {
        rules: &data.schedule,
        stakes: &data.stakes,
        seed: 1,
    };
    let cal = sched.generate_day(0, &venues, &MeetHeat::default());
    assert!(cal.events.iter().any(|e| e.phase == DayPhase::Night));
}

/// The procedural city the `race_city_sprint` scenario runs in.
fn sprint_city(traffic: bool) -> World {
    let pg = ProcGenConfig {
        nx: 7,
        ny: 7,
        spacing_m: 110.0,
        seed: 1,
        ..Default::default()
    };
    let mut cfg = WorldConfig::default();
    cfg.traffic.enabled = traffic;
    World::procedural(&pg, cfg)
}

fn plan_sprint(world: &World) -> CityRace {
    plan_city_race(
        world,
        [60.55, -437.0, 0.0],
        [0.0, 1.0],
        [60.55, -140.0, 0.0],
        3.0,
    )
    .expect("arterial sprint route")
}

#[test]
fn city_sprint_on_real_roads() {
    let race = plan_sprint(&sprint_city(false));
    let len = race.course.length_m();
    assert!((280.0..310.0).contains(&len), "route length {len}");
    let r = judge_scenario("city_sprint.ron", RaceKind::Sprint, race.course.clone());
    // The coupe starts half a row back on the right and still wins.
    assert_eq!(r[0].racer, EntityId(2), "{r:#?}");
    assert!(r.iter().all(|x| x.status == RacerStatus::Finished));
    assert!(r
        .iter()
        .all(|x| x.gates_passed as usize == race.course.gates.len()));
    // wp_world reports speeding on the 24.6 m/s arterial; speeding alone keeps a race clean.
    assert!(r[0].violations[0] >= 1, "{:?}", r[0].violations);
    assert!(r.iter().all(|x| x.clean));
}

#[test]
fn city_sprint_route_closure_through_set_active_routes() {
    let mut world = sprint_city(true);
    let race = plan_sprint(&world);
    let preview = closure_preview(&world, &race.route);
    assert!(!preview.closed_lanes.is_empty());
    // Every lane the race drives is closed.
    for l in &race.route.lanes {
        assert!(preview.closed_lanes.contains(l), "lane {l} not closed");
    }
    let cones_before = world
        .static_props()
        .filter(|p| p.kind == PropKind::Cone)
        .count();
    assert_eq!(cones_before, 0);

    // Night meet: open route, focus only.
    activate_race_routes(&mut world, &[(&race.route, TrafficMode::Normal)]);
    assert_eq!(world.active_routes().len(), 1);
    assert!(!world.active_routes()[0].closed);
    assert_eq!(
        world
            .static_props()
            .filter(|p| p.kind == PropKind::Cone)
            .count(),
        0
    );

    // Sanctioned event: closed, cones placed, no civilian traffic on the race lanes.
    activate_race_routes(&mut world, &[(&race.route, TrafficMode::Suppressed)]);
    assert!(world.active_routes()[0].closed);
    let cones = world
        .static_props()
        .filter(|p| p.kind == PropKind::Cone)
        .count();
    assert_eq!(cones, preview.cones.len());
    world.populate();
    let mut ev = Vec::new();
    for _ in 0..240 * 20 {
        world.step(wp_contracts::PHYSICS_DT, &mut ev);
    }
    assert!(world.agent_count() > 0, "the city still has traffic");
    for a in world.agents() {
        let on_race = race.course.project(a.pos, None);
        let inside = on_race.off_m < 5.5
            && on_race.along_m > 5.0
            && on_race.along_m < race.course.length_m() - 5.0;
        assert!(!inside, "civilian {:?} on the closed race route", a.pos);
    }

    // Race over: routes cleared, cones gone.
    activate_race_routes(&mut world, &[]);
    assert!(world.active_routes().is_empty());
    assert_eq!(
        world
            .static_props()
            .filter(|p| p.kind == PropKind::Cone)
            .count(),
        0
    );
}
