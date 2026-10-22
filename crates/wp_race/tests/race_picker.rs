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
    run_race_with(id, max_s, None)
}

/// [`run_race`] with the scenario's police presence replaced (`Some(0.0)` = no police), for
/// checks of the AI field's pace that the police (captures, flights) would mask.
fn run_race_with(id: &str, max_s: u64, presence: Option<f32>) -> Raced {
    let mut def = catalog::race(id).unwrap();
    let path = repo().join(format!("apps/simcli/scenarios/race/pick_{id}.ron"));
    let mut sc = load_scenario(&path).unwrap();
    if presence.is_some() {
        sc.police_presence = presence;
    }
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
    // The AI drives every seat, the player's car included: no player steering aids on it.
    sim.set_player_steering(ids[0], Some(false)).unwrap();
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
    // Every slot starts inside the paved lanes, a car's half width plus the margin from their
    // edge (not on a verge or shoulder past the kerb; a slot inside a junction box has no lane
    // under it and is not checked).
    for slot in &slots {
        let d = slot.dir;
        if let Some((lo, hi)) = catalog::paved_span(sim.world(), slot.pos, d, [d[1], -d[0]]) {
            let edge = catalog::GRID_CAR_HALF_WIDTH_M + catalog::GRID_EDGE_MARGIN_M - 0.01;
            assert!(
                lo <= -edge && hi >= edge,
                "{id}: grid slot {} at {:?} is outside the lanes ({lo:.2}..{hi:.2} m)",
                slot.index,
                slot.pos
            );
        }
    }
    // Nobody starts nose to tail with a parked car: no parked car within a car's width of a
    // slot's first 30 m (a two-wide grid on a narrow street put a column in the parking lane).
    if sc.parked_cars {
        for slot in &slots {
            for prop in sim.world().static_props() {
                if prop.kind != wp_world::PropKind::ParkedCar {
                    continue;
                }
                let rel = [prop.pos[0] - slot.pos[0], prop.pos[1] - slot.pos[1]];
                let ahead = rel[0] * slot.dir[0] + rel[1] * slot.dir[1];
                let side = (rel[0] * slot.dir[1] - rel[1] * slot.dir[0]).abs();
                assert!(
                    !((-3.0..33.0).contains(&ahead) && side < prop.half_extents[0] + 1.1),
                    "{id}: grid slot {} at {:?} runs into a parked car at {:?}",
                    slot.index,
                    slot.pos,
                    prop.pos
                );
            }
        }
    }

    let entrant = |i: usize| {
        let car = &built.cars[&sc.entities[i].car];
        LiveEntrant {
            entity: ids[i],
            redline_rpm: car.def.engine.redline_rpm,
            capability: CarCapability::for_car(car),
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
    )
    .expect("a drivable route");
    let green_tick = live.judge().green_tick();
    let racers = live.racers().to_vec();

    let mut events: Vec<SimEvent> = Vec::new();
    let mut signals = Vec::new();
    let mut pursuits = 0;
    let mut heat_after_green = Vec::new();
    let mut finished_tick = None;
    // `WP_RACE_TIME=1`: wall time spent in the race glue (AI field + judge), for the README.
    let timing = std::env::var("WP_RACE_TIME").is_ok();
    let mut glue = std::time::Duration::ZERO;
    let mut glue_ticks = 0u32;
    while sim.tick() < 240 * max_s {
        let t0 = timing.then(std::time::Instant::now);
        let inputs = live.pre_step(
            sim.tick(),
            sim.world(),
            &obs(&sim),
            &events,
            &SimPolice(sim.police()),
        );
        if let Some(t0) = t0 {
            glue += t0.elapsed();
        }
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
        let t1 = timing.then(std::time::Instant::now);
        let out = live.post_step(sim.tick(), &samples, &events, &police);
        if let Some(t1) = t1 {
            glue += t1.elapsed();
            glue_ticks += 1;
        }
        // Off-course racers go back on the route at rest, as the browser session does (it also
        // picks a clear lane spot there; the centreline point is enough here).
        for r in &out.resets {
            let yaw = wp_math::atan2(-r.dir[0], r.dir[1]);
            sim.place_on_ground(r.racer, [r.pos[0], r.pos[1]], yaw, 0.0, Some(r.pos[2]))
                .unwrap();
            live.relocate(r.racer, r.pos, sim.tick());
        }
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
            for (k, e) in live.ai().racers().iter().enumerate() {
                let d = live.ai().field().debug_of(k);
                if let wp_opponent::SpeedLimit::Obstacle(w) = d.limit {
                    if let Some(v) = sim.vehicles().iter().find(|v| v.id.0 == w) {
                        let st = v.vehicle.state();
                        let unit = sim.police().units().iter().find(|u| u.entity == Some(v.id));
                        eprintln!(
                            "{id}     blocker {:?} {:?} pos ({:.0},{:.0}) v {:.1} siren {} unit {:?}",
                            v.id,
                            v.kind,
                            st.pos[0],
                            st.pos[1],
                            st.speed_mps,
                            v.input.siren,
                            unit.map(|u| (&u.assignment, &u.tactic))
                        );
                    }
                }
                let pol = sim.police().racer(*e);
                eprintln!(
                    "{id}   ai {e:?} police {:?} s {:.0} n {:.1} v_target {:.1} {:?} {:?} {:?}",
                    pol.map(|p| (p.heat as i32, p.level, p.status)),
                    d.s,
                    d.n,
                    d.v_target,
                    d.limit,
                    live.ai().field().tactic_of(k),
                    d.pass_checks
                );
            }
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
    if timing && glue_ticks > 0 {
        eprintln!(
            "{id}: race glue {:.1} µs/tick ({:.3} ms per 60 Hz frame) for {} racers",
            glue.as_secs_f64() * 1e6 / glue_ticks as f64,
            glue.as_secs_f64() * 4e3 / glue_ticks as f64,
            racers.len()
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
        "{id}: only {finished} of {} finished: {:?}",
        r.results.len(),
        r.results
            .iter()
            .map(|x| (
                x.racer,
                x.status,
                x.elapsed_s(),
                x.gates_passed,
                x.violations
            ))
            .collect::<Vec<_>>()
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
    // With the police patrolling the hill a racer may be caught or flee legitimately (owner
    // decision, as in `loop9_circuit`; `WP_RACE_SEED=6`: two racers out to the police in the
    // first 45 s), so police outcomes count with the finishers. What still fails: a race
    // that never ends (a deadlock), no winner, a wreck. Since the owner's "racers never give
    // up" rule (2026-10-21, wp_opponent `RaceConfig::allow_abandon` off) a pursued racer keeps
    // racing at any wanted level, so only real captures take racers out and a winner is
    // required again (E had relaxed this in 6e63486d while pursued racers yielded at L1).
    // Seeds 1-8: see docs/integration/opponent-pace.md.
    check_with_police(&r, 40.0..110.0, 3);
}

/// [`check`] for a race with police on the course: racers the police caught or chased off
/// (`Dnf(Captured)` / `Dnf(Fled)`) count with the finishers towards `min_finish`, and the
/// winner's window stretches to 180 s when the police took someone out (the chase holds the
/// field up). The race must still end with a winner and nobody wrecked.
fn check_with_police(r: &Raced, winner_s: std::ops::Range<f32>, min_finish: usize) {
    check_with_police_inner(r, winner_s, min_finish, false);
}

fn check_with_police_inner(
    r: &Raced,
    winner_s: std::ops::Range<f32>,
    min_finish: usize,
    all_out_ok: bool,
) {
    use wp_race::DnfReason;
    let id = r.def.id;
    assert!(r.finished_tick.is_some(), "{id}: never finished");
    assert!(r.results.iter().all(|x| x.status != RacerStatus::Running));
    assert!(matches!(
        r.signals.last(),
        Some(RaceSignal::RaceFinished { .. })
    ));
    let police_out = r
        .results
        .iter()
        .filter(|x| {
            matches!(
                x.status,
                RacerStatus::Dnf(DnfReason::Captured | DnfReason::Fled)
            )
        })
        .count();
    let finished = r
        .results
        .iter()
        .filter(|x| x.status == RacerStatus::Finished)
        .count();
    assert!(
        r.results
            .iter()
            .all(|x| x.status != RacerStatus::Dnf(DnfReason::TotalLoss)),
        "{id}: a car was wrecked"
    );
    if all_out_ok && police_out == r.results.len() {
        return;
    }
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
    let window = if police_out > 0 {
        winner_s.start..180.0
    } else {
        winner_s
    };
    let t = winner.elapsed_s().unwrap();
    assert!(
        window.contains(&t),
        "{id}: winner took {t:.1} s, want {window:?} ({police_out} out to the police)"
    );
    assert!(
        finished + police_out >= min_finish,
        "{id}: {finished} finished + {police_out} out to the police < {min_finish}: {:?}",
        r.results
            .iter()
            .map(|x| (x.racer, x.status, x.elapsed_s()))
            .collect::<Vec<_>>()
    );
    assert!(
        r.results
            .iter()
            .all(|x| x.status != RacerStatus::Dnf(DnfReason::TotalLoss)),
        "{id}: a car was wrecked"
    );
}

#[test]
fn hollins_hill_sprint_pace_without_police() {
    let r = run_race_with("hollins_sprint", 200, Some(0.0));
    check(&r, 40.0..110.0, 3);
    assert_eq!(r.pursuits, 0);
}

#[test]
fn loop9_circuit() {
    let r = run_race("loop9_circuit", 600);
    let id = r.def.id;
    // One 10.1 km lap of the highway loop through late-night traffic.
    assert!(r.route_m > 9000.0);
    // The meet at the Loop 9 grid is hot: the green light calls in a patrol.
    assert!(r
        .signals
        .iter()
        .any(|s| matches!(s, RaceSignal::PatrolAttracted { .. })));
    // Since police round 12b/13 (backup converging, L2+ PIT/rams, pinned captures) the
    // patrol turns the race into pursuits, and races with police can end in captures (owner
    // decision), so the number of finishers is the police's call, not a pace measure (the
    // AI field's pace is checked without police in `loop9_circuit_pace_without_police`).
    // What must hold: the race is judged to the end, everybody raced, and a racer's race
    // ends at the flag, by the police, behind the winner's finish window (a racer the police
    // held up), or in at most one racing incident (DQ).
    assert!(r.finished_tick.is_some(), "{id}: never finished");
    assert_eq!(r.results.len(), r.def.entrants());
    assert!(matches!(
        r.signals.last(),
        Some(RaceSignal::RaceFinished { .. })
    ));
    // As before round 12b, one racing incident (a DQ) is allowed per race.
    let dqs = r
        .results
        .iter()
        .filter(|x| matches!(x.status, RacerStatus::Dq(_)))
        .count();
    assert!(dqs <= 1, "{id}: {dqs} disqualified");
    // "Left the grid" is a gate passed or 30 m of progress: since row 99 the AI races at its
    // car's real pace through the Max-presence daytime traffic, and a racer that the police
    // stop (or that flees them) 60–80 m into the run, before gate 1, did race. Seed sweep
    // 1-6 + 31 on the merged tree: 3 of 7 pass both before and after row 99, failing on
    // police outcomes / world deadlocks (see docs/integration/opponent-pace.md).
    for x in &r.results {
        assert!(
            x.gates_passed >= 1 || x.progress_m >= 30.0,
            "{id}: {x:?} never left the grid"
        );
        assert!(
            matches!(
                x.status,
                RacerStatus::Finished
                    | RacerStatus::Dq(_)
                    | RacerStatus::Dnf(wp_race::DnfReason::Captured)
                    | RacerStatus::Dnf(wp_race::DnfReason::Fled)
                    | RacerStatus::Dnf(wp_race::DnfReason::FinishWindow)
            ),
            "{id}: {x:?} ended by neither the flag, the police nor a racing incident"
        );
    }
    if let Some(w) = r.results.iter().find(|x| x.status == RacerStatus::Finished) {
        let t = w.elapsed_s().unwrap();
        assert!((150.0..420.0).contains(&t), "{id}: winner took {t:.1} s");
    }
    eprintln!(
        "loop9 pursuits: {}, finished: {}/{}",
        r.pursuits,
        r.results
            .iter()
            .filter(|x| x.status == RacerStatus::Finished)
            .count(),
        r.results.len()
    );
}

#[test]
fn loop9_circuit_pace_without_police() {
    // The same lap with police presence 0: the AI field's pace and racecraft through
    // late-night traffic (the pre-round-12b expectations of `loop9_circuit`). Round 13
    // (self-aligning steering, handbrake declutch, ride rework): 6 of 6 finish, winner
    // 215.6 s (pre-merge base: 5 of 6 + one DQ, winner 218.4 s); after the round-12b/13
    // merges up to ecf4d7b: 5 of 6 + one DQ (LeftCorridor at gate 34), winner 209.4 s.
    let r = run_race_with("loop9_circuit", 600, Some(0.0));
    check(&r, 150.0..420.0, 4);
    assert_eq!(r.pursuits, 0);
}

#[test]
fn lantern_checkpoint_draws_the_police() {
    // Seed 31 (the catalog's) is back since the box-in escape (follow-up (a), 2026-10-22): the
    // last racer, boxed in between captured cars, used to stay Running and the race was never
    // judged (seeds 5 and 31); seeds 1-8 and 31 now all end.
    let r = run_race("lantern_checkpoint", 300);
    let id = r.def.id;
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
    // Illegal daytime with Max presence is meant to bring every cop in town: pursuits start
    // while the race is on (the loop stops counting when the race is judged over).
    assert!(r.pursuits >= 1, "{id}: no pursuit during the race");
    // The race is judged to the end and nobody wrecks.
    assert!(r.finished_tick.is_some(), "{id}: never finished");
    assert_eq!(r.results.len(), r.def.entrants());
    assert!(r.results.iter().all(|x| x.status != RacerStatus::Running));
    assert!(matches!(
        r.signals.last(),
        Some(RaceSignal::RaceFinished { .. })
    ));
    // How a racer's race ends is up to the police and the driver (wp_opponent: pull over and
    // take the ticket below the flee level, abandon and run at or above it), so a run where
    // the whole field is busted or flees is a correct result. What must not happen is a racer
    // that never really raced (stuck on the grid in the parked cars: 0 gates) or one that
    // ends any other way than finishing or by the police.
    // "Left the grid" is a gate passed or 30 m of progress: since row 99 the AI races at its
    // car's real pace through the Max-presence daytime traffic, and a racer that the police
    // stop (or that flees them) 60–80 m into the run, before gate 1, did race. Seed sweep
    // 1-6 + 31 on the merged tree: 3 of 7 pass both before and after row 99, failing on
    // police outcomes / world deadlocks (see docs/integration/opponent-pace.md).
    for x in &r.results {
        assert!(
            x.gates_passed >= 1 || x.progress_m >= 30.0,
            "{id}: {x:?} never left the grid"
        );
        // Or behind the winner's finish window: racers never give up (owner, 2026-10-21), so a
        // racer held up past it ends there. On the default seed (31) after the round-12 police
        // and traffic merges, two racers sat ~25 s behind a civilian standing in their lane on
        // the narrow downtown street, then crawled behind it with no room to pass (police
        // status Clear throughout); seeds 1-6 end by the flag or captures (seed 5: a racer
        // wedged beside two captured cars, a separate AI follow-up in opponent-pace.md).
        assert!(
            matches!(
                x.status,
                RacerStatus::Finished
                    | RacerStatus::Dnf(wp_race::DnfReason::Captured)
                    | RacerStatus::Dnf(wp_race::DnfReason::Fled)
                    | RacerStatus::Dnf(wp_race::DnfReason::FinishWindow)
            ),
            "{id}: {x:?} ended by neither the flag, the police nor the finish window"
        );
    }
    // Anyone who does get through the 665 m daytime zig-zag (signals, junction queues) does
    // it in a sensible time.
    if let Some(w) = r.results.iter().find(|x| x.status == RacerStatus::Finished) {
        let t = w.elapsed_s().unwrap();
        assert!((40.0..170.0).contains(&t), "{id}: winner took {t:.1} s");
    }
    let police_outs = r
        .results
        .iter()
        .filter(|x| x.status != RacerStatus::Finished)
        .count();
    eprintln!(
        "lantern pursuits: {}, ended by the police: {police_outs}/{}",
        r.pursuits,
        r.results.len()
    );
}
