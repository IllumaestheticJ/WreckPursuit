//! Crate-level tests: every race kind through scripted traces, ranking, stakes, scheduler,
//! crews, territory and progression, plus property tests.

use proptest::prelude::*;
use wp_contracts::{DayPhase, EntityId, ViolationKind, WORLD_ENTITY};

use crate::course::Course;
use crate::crew::{ChallengeState, CrewRegistry};
use crate::data::RaceData;
use crate::error::{CrewError, EntryDenied};
use crate::judge::{RaceJudge, RacerEntry, RacerSample, StartError};
use crate::kind::{
    ClassBracket, CrewId, DistrictId, EventId, Legality, RaceKind, RacerAccount, VenueId,
};
use crate::progression::{EntryRequirements, RacerProfile};
use crate::result::{DnfReason, DqReason, RacerResult, RacerStatus};
use crate::schedule::{MeetHeat, Scheduler, TrafficMode, Venue};
use crate::signals::{PolicePresence, RaceSignal};
use crate::territory::Territory;
use crate::trace::{simulate, ScriptedIncident, ScriptedRacer, SimulatedRace};

fn data() -> RaceData {
    RaceData::embedded()
}

fn judge(kind: RaceKind, course: Course) -> RaceJudge {
    RaceJudge::new(EventId(1), data().rules.for_kind(kind), course)
}

#[test]
fn a_course_grid_layout_overrides_the_kind_grid() {
    let e = |id| RacerEntry {
        racer: EntityId(id),
        redline_rpm: 7000.0,
    };
    // The sprint's own grid is two columns.
    let mut j = judge(RaceKind::Sprint, sprint_course());
    let a = j.enter(e(1)).unwrap();
    let b = j.enter(e(2)).unwrap();
    assert!((a.pos[0] - b.pos[0]).abs() > 1.0, "two columns");
    // A narrow start street: the planner sets single file and every car sits on the centreline.
    let mut course = sprint_course();
    course.grid_layout = Some(crate::course::GridLayout::SingleFile { gap_m: 8.0 });
    let mut j = judge(RaceKind::Sprint, course);
    let slots: Vec<_> = (1..=4).map(|id| j.enter(e(id)).unwrap()).collect();
    for (i, s) in slots.iter().enumerate() {
        assert!(
            s.pos[0].abs() < 1e-4,
            "slot {i} on the centreline: {:?}",
            s.pos
        );
        if i > 0 {
            assert!((slots[i - 1].pos[1] - s.pos[1] - 8.0).abs() < 1e-3);
        }
    }
    // Withdrawing re-packs the grid with the same layout.
    assert!(j.withdraw(EntityId(1)));
    assert!(j.grid().iter().all(|(_, s)| s.pos[0].abs() < 1e-4));
}

fn sprint_course() -> Course {
    Course::straight([0.0, 0.0, 0.0], [0.0, 1.0], 1000.0, 200.0, 12.0)
}

fn square_course() -> Course {
    Course::closed(
        &[
            [0.0, 0.0, 0.0],
            [0.0, 200.0, 0.0],
            [200.0, 200.0, 0.0],
            [200.0, 0.0, 0.0],
        ],
        14.0,
    )
}

fn run(kind: RaceKind, course: Course, racers: &[ScriptedRacer], every: u64) -> SimulatedRace {
    simulate(judge(kind, course), racers, every, 240 * 2000)
}

fn by_racer(results: &[RacerResult], id: u32) -> &RacerResult {
    results.iter().find(|r| r.racer == EntityId(id)).unwrap()
}

// ---------------------------------------------------------------- data

#[test]
fn embedded_data_is_valid() {
    let d = data();
    assert_eq!(d.rules.kinds.len(), RaceKind::KEYS.len());
    assert_eq!(d.progression.licences[0].clean_races, 0);
}

#[test]
fn brackets_match_economy_market_table() {
    let market: toml::Value =
        toml::from_str(include_str!("../../../data/economy/market.toml")).unwrap();
    let econ = market["class_index"]["brackets"].as_array().unwrap();
    let ours = &data().progression.brackets;
    assert_eq!(econ.len(), ours.len());
    for (e, o) in econ.iter().zip(ours) {
        assert_eq!(e["name"].as_str().unwrap(), o.letter);
        assert_eq!(e["min"].as_integer().unwrap() as u16, o.min);
    }
}

#[test]
fn bad_data_is_rejected() {
    let d = data();
    let mut r = d.rules.clone();
    r.kinds.remove("drag");
    assert!(r.validate().is_err());
    let mut s = d.stakes.clone();
    s.risk_curve[2][1] = 0.1;
    assert!(s.validate().is_err());
    let mut p = d.progression.clone();
    p.licences[1].clean_races = 0;
    assert!(p.validate().is_err());
    let mut sc = d.schedule.clone();
    sc.sanctioned.hours = [20.0, 23.0];
    assert!(sc.validate().is_err());
}

// ---------------------------------------------------------------- sprint & basics

#[test]
fn sprint_faster_car_wins_and_signals_flow() {
    let racers = [
        ScriptedRacer::new(1, 5.0, 40.0),
        ScriptedRacer::new(2, 6.0, 45.0),
    ];
    let out = run(RaceKind::Sprint, sprint_course(), &racers, 1);
    let r = &out.results;
    assert_eq!(r[0].racer, EntityId(2));
    assert_eq!(r[0].position, 1);
    assert_eq!(r[1].position, 2);
    assert!(r
        .iter()
        .all(|x| x.status == RacerStatus::Finished && x.clean));
    let t = r[0].elapsed_s().unwrap();
    assert!((25.0..28.0).contains(&t), "winner time {t}");
    assert_eq!(r[0].gates_passed, 5);
    let kinds: Vec<&str> = out
        .signals
        .iter()
        .map(|s| match s {
            RaceSignal::Countdown { .. } => "countdown",
            RaceSignal::Green { .. } => "green",
            RaceSignal::CheckpointPassed { .. } => "cp",
            RaceSignal::RacerFinished { .. } => "finished",
            RaceSignal::RaceFinished { .. } => "race",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds[0], "countdown");
    assert_eq!(kinds[1], "green");
    assert_eq!(kinds.iter().filter(|k| **k == "cp").count(), 10);
    assert_eq!(*kinds.last().unwrap(), "race");
    // Contract v0.3.0: every signal converts to `SimEvent::Race`, results reduced to rows.
    for sig in &out.signals {
        let ev = sig.to_event();
        assert!(matches!(sig.to_sim_event(), wp_contracts::SimEvent::Race(ref e) if *e == ev));
    }
    match out.signals.last().unwrap().to_event() {
        wp_contracts::RaceEvent::RaceFinished { results, .. } => {
            assert_eq!(results.len(), r.len());
            for (row, full) in results.iter().zip(r) {
                assert_eq!(*row, wp_contracts::RaceResultRow::from(full));
                assert_eq!(row.position, full.position);
            }
        }
        other => panic!("expected RaceFinished, got {other:?}"),
    }
}

#[test]
fn judging_is_sample_rate_independent() {
    let racers = [
        ScriptedRacer::new(1, 5.0, 40.0),
        ScriptedRacer::new(2, 5.2, 40.0),
        ScriptedRacer::new(3, 4.8, 42.0),
    ];
    let a = run(RaceKind::Sprint, sprint_course(), &racers, 1);
    let b = run(RaceKind::Sprint, sprint_course(), &racers, 4);
    for (x, y) in a.results.iter().zip(&b.results) {
        assert_eq!(x.racer, y.racer);
        assert_eq!(x.status, y.status);
        assert_eq!(x.gates_passed, y.gates_passed);
        // Sub-tick interpolation keeps finish times within a fraction of a tick.
        let dx = x.elapsed_subticks.unwrap() as i64 - y.elapsed_subticks.unwrap() as i64;
        assert!(dx.abs() < 1000, "{dx} subticks");
    }
}

#[test]
fn entry_and_start_rules() {
    let mut j = judge(
        RaceKind::Drag,
        Course::straight([0.0; 3], [0.0, 1.0], 402.0, 500.0, 10.0),
    );
    let e = |id| RacerEntry {
        racer: EntityId(id),
        redline_rpm: 7000.0,
    };
    j.enter(e(1)).unwrap();
    assert_eq!(j.enter(e(1)), Err(EntryDenied::AlreadyEntered));
    assert_eq!(
        j.start(0),
        Err(StartError::NotEnoughRacers { min: 2, have: 1 })
    );
    j.enter(e(2)).unwrap();
    assert_eq!(j.enter(e(3)), Err(EntryDenied::Full));
    assert!(j.withdraw(EntityId(1)));
    assert_eq!(j.grid()[0].0, EntityId(2));
    assert_eq!(j.grid()[0].1.index, 0);
    j.enter(e(1)).unwrap();
    let green = j.start(100).unwrap();
    assert_eq!(green, 100 + 720);
    assert_eq!(j.enter(e(4)), Err(EntryDenied::LobbyClosed));
    assert_eq!(j.start(200), Err(StartError::AlreadyStarted));
}

#[test]
fn server_reset_is_not_a_teleport() {
    let mut j = judge(RaceKind::Sprint, sprint_course());
    for id in [1, 2] {
        j.enter(RacerEntry {
            racer: EntityId(id),
            redline_rpm: 0.0,
        })
        .unwrap();
    }
    let green = j.start(0).unwrap();
    let sample = |id, y: f32| RacerSample {
        racer: EntityId(id),
        pos: [0.0, y, 0.0],
        vel: [0.0, 20.0, 0.0],
        speed_mps: 20.0,
        throttle: 1.0,
        gear: 3,
        engine_rpm: 5000.0,
        up_z: 1.0,
    };
    j.observe(green, &[sample(1, 50.0), sample(2, 50.0)], &[]);
    // Racer 1 is reset 40 m back by the server; racer 2 "moves" 40 m on its own.
    j.relocate(EntityId(1), [0.0, 10.0, 0.0], green + 1);
    j.observe(green + 1, &[sample(1, 10.0), sample(2, 90.0)], &[]);
    let r = j.results();
    assert_eq!(by_racer(&r, 1).status, RacerStatus::Running);
    assert_eq!(
        by_racer(&r, 2).status,
        RacerStatus::Dq(DqReason::ImplausibleMovement)
    );
}

#[test]
fn a_stranded_ai_racer_is_put_back_after_its_grace_and_the_player_is_not() {
    // Resets off (a picker race): only racer 1, an AI entrant, is on the stranded rule.
    let mut j = judge(RaceKind::Sprint, sprint_course());
    j.set_stranded_reset(&[EntityId(1)]);
    for id in [1, 2] {
        j.enter(RacerEntry {
            racer: EntityId(id),
            redline_rpm: 0.0,
        })
        .unwrap();
    }
    let green = j.start(0).unwrap();
    let at = |id: u32, x: f32, y: f32| RacerSample {
        racer: EntityId(id),
        pos: [x, y, 0.0],
        vel: [0.0, 0.0, 0.0],
        speed_mps: 0.0,
        throttle: 0.0,
        gear: 1,
        engine_rpm: 1000.0,
        up_z: 1.0,
    };
    // Both on course at y = 120, then both 40 m to the side.
    for id in [1, 2] {
        j.relocate(EntityId(id), [0.0, 120.0, 0.0], green);
    }
    j.observe(green, &[at(1, 0.0, 120.0), at(2, 0.0, 120.0)], &[]);
    for id in [1, 2] {
        j.relocate(EntityId(id), [40.0, 120.0, 0.0], green);
    }
    let grace = (crate::judge::STRANDED_GRACE_S / wp_contracts::PHYSICS_DT) as u64;
    let mut resets = Vec::new();
    for k in 1..=grace + 240 {
        j.observe(green + k, &[at(1, 40.0, 120.0), at(2, 40.0, 120.0)], &[]);
        let r = j.drain_resets();
        if !r.is_empty() {
            // Not before the grace is up (the AI keeps trying to rejoin until then).
            assert!(k > grace, "reset at tick {k}, grace {grace}");
        }
        resets.extend(r);
    }
    assert_eq!(resets.len(), 1, "{resets:?}");
    assert_eq!(resets[0].racer, EntityId(1));
    assert!(
        resets[0].pos[0].abs() < 1e-3 && (resets[0].pos[1] - 120.0).abs() < 1e-3,
        "{:?}",
        resets[0]
    );
}

#[test]
fn a_flipped_ai_racer_is_righted_on_the_course_and_the_player_is_not() {
    // On its roof inside the corridor (never off course): only the AI entrant is reset,
    // after `FLIPPED_RESET_S`, where it lies.
    let mut j = judge(RaceKind::Sprint, sprint_course());
    j.set_stranded_reset(&[EntityId(1)]);
    for id in [1, 2] {
        j.enter(RacerEntry {
            racer: EntityId(id),
            redline_rpm: 0.0,
        })
        .unwrap();
    }
    let green = j.start(0).unwrap();
    let at = |id: u32, up_z: f32| RacerSample {
        racer: EntityId(id),
        pos: [0.0, 120.0, 0.0],
        vel: [0.0, 0.0, 0.0],
        speed_mps: 0.0,
        throttle: 0.0,
        gear: 1,
        engine_rpm: 1000.0,
        up_z,
    };
    for id in [1, 2] {
        j.relocate(EntityId(id), [0.0, 120.0, 0.0], green);
    }
    j.observe(green, &[at(1, 1.0), at(2, 1.0)], &[]);
    let limit = (crate::judge::FLIPPED_RESET_S / wp_contracts::PHYSICS_DT) as u64;
    let mut resets = Vec::new();
    for k in 1..=limit + 240 {
        j.observe(green + k, &[at(1, -0.95), at(2, -0.95)], &[]);
        let r = j.drain_resets();
        if !r.is_empty() {
            assert!(k > limit, "reset at tick {k}, limit {limit}");
        }
        resets.extend(r);
    }
    assert_eq!(resets.len(), 1, "{resets:?}");
    assert_eq!(resets[0].racer, EntityId(1));
    assert!((resets[0].pos[1] - 120.0).abs() < 1e-3, "{:?}", resets[0]);
    // Upright again: the count starts over.
    j.observe(green + limit + 241, &[at(1, 1.0), at(2, 1.0)], &[]);
    assert!(j.drain_resets().is_empty());
}

#[test]
fn a_stalled_ai_racer_is_put_back_on_the_course_and_the_player_is_not() {
    // Standing still on the course (wedged in a pile, never off it or flipped): only the AI
    // entrant is reset, once per `STALLED_RESET_S` without progress.
    let mut j = judge(RaceKind::Sprint, sprint_course());
    j.set_stranded_reset(&[EntityId(1)]);
    for id in [1, 2] {
        j.enter(RacerEntry {
            racer: EntityId(id),
            redline_rpm: 0.0,
        })
        .unwrap();
    }
    let green = j.start(0).unwrap();
    let at = |id: u32| RacerSample {
        racer: EntityId(id),
        pos: [0.5, 120.0, 0.0],
        vel: [0.0, 0.0, 0.0],
        speed_mps: 0.0,
        throttle: 0.0,
        gear: 1,
        engine_rpm: 1000.0,
        up_z: 1.0,
    };
    for id in [1, 2] {
        j.relocate(EntityId(id), [0.5, 120.0, 0.0], green);
    }
    let limit = (crate::judge::STALLED_RESET_S / wp_contracts::PHYSICS_DT) as u64;
    let mut resets = Vec::new();
    for k in 0..=2 * limit + 240 {
        j.observe(green + k, &[at(1), at(2)], &[]);
        let r = j.drain_resets();
        if !r.is_empty() {
            assert!(k > limit, "reset at tick {k}, limit {limit}");
        }
        resets.extend(r.into_iter().map(|r| (k, r)));
    }
    // Once per stall period: at ~limit and again ~limit later, never every tick.
    assert_eq!(resets.len(), 2, "{resets:?}");
    assert!(resets.iter().all(|(_, r)| r.racer == EntityId(1)));
    assert!(resets[1].0 > resets[0].0 + limit, "{resets:?}");
    assert!((resets[0].1.pos[1] - 120.0).abs() < 1e-3, "{resets:?}");
}

#[test]
fn reset_mode_puts_an_off_course_racer_back_instead_of_a_dq() {
    let mut j = judge(RaceKind::Sprint, sprint_course());
    j.set_reset_off_course(true);
    for id in [1, 2] {
        j.enter(RacerEntry {
            racer: EntityId(id),
            redline_rpm: 0.0,
        })
        .unwrap();
    }
    let green = j.start(0).unwrap();
    // Racer 2 waits on the grid; racer 1 is the one that leaves the road.
    let at = |x: f32, y: f32| RacerSample {
        racer: EntityId(1),
        pos: [x, y, 0.0],
        vel: [0.0, 0.0, 0.0],
        speed_mps: 0.0,
        throttle: 0.0,
        gear: 1,
        engine_rpm: 1000.0,
        up_z: 1.0,
    };
    // On course at y = 120, then parked 40 m to the side for 4 s (grace is 3 s).
    // (Both moves are announced so they are not judged as teleports.)
    j.relocate(EntityId(1), [0.0, 120.0, 0.0], green);
    j.observe(green, &[at(0.0, 120.0)], &[]);
    j.relocate(EntityId(1), [40.0, 120.0, 0.0], green);
    let grace = (data().rules.common.corridor_grace_s / wp_contracts::PHYSICS_DT) as u64;
    let mut resets = Vec::new();
    for k in 1..=grace + 240 {
        j.observe(green + k, &[at(40.0, 120.0)], &[]);
        resets.extend(j.drain_resets());
    }
    assert_eq!(resets.len(), 1, "one reset, then the clock starts again");
    let r = resets[0];
    assert_eq!(r.racer, EntityId(1));
    assert!(
        r.pos[0].abs() < 1e-3 && (r.pos[1] - 120.0).abs() < 1e-3,
        "{r:?}"
    );
    assert!(
        (r.dir[1] - 1.0).abs() < 1e-4,
        "faces along the course: {r:?}"
    );
    // The server moved the car; it races on.
    j.relocate(EntityId(1), r.pos, green + grace + 241);
    j.observe(green + grace + 241, &[at(r.pos[0], r.pos[1])], &[]);
    assert_eq!(by_racer(&j.results(), 1).status, RacerStatus::Running);
}

// ---------------------------------------------------------------- circuit

#[test]
fn circuit_counts_laps_and_best_lap() {
    let racers = [
        ScriptedRacer::new(1, 5.0, 35.0),
        ScriptedRacer::new(2, 5.0, 38.0),
    ];
    let out = run(RaceKind::Circuit { laps: 3 }, square_course(), &racers, 2);
    let w = &out.results[0];
    assert_eq!(w.racer, EntityId(2));
    assert_eq!(w.laps_completed, 3);
    assert_eq!(w.gates_passed, 12);
    // Laps 2 and 3 are flying laps: 800 m at 38 m/s ≈ 21 s.
    let best = crate::result::subticks_to_s(w.best_lap_subticks.unwrap());
    assert!((20.5..22.0).contains(&best), "best lap {best}");
    let laps = out
        .signals
        .iter()
        .filter(|s| matches!(s, RaceSignal::LapCompleted { racer, .. } if *racer == EntityId(2)))
        .count();
    assert_eq!(laps, 3);
}

// ---------------------------------------------------------------- DNF / DQ

#[test]
fn leaving_the_route_is_never_a_dq_but_gates_still_count() {
    let racers = [
        ScriptedRacer::new(1, 5.0, 40.0)
            .with(5.0, ScriptedIncident::Swerve { offset_m: 30.0 })
            .with(6.0, ScriptedIncident::Swerve { offset_m: 0.0 }),
        ScriptedRacer::new(2, 5.0, 40.0).with(5.0, ScriptedIncident::Swerve { offset_m: 30.0 }),
    ];
    // A short excursion off the route and back still finishes. Staying off the route is not
    // a DQ either, but the racer misses the gates, never finishes and runs out of time after
    // the winner. Corridor kinds and open-route kinds judge this the same way.
    for kind in [RaceKind::Sprint, RaceKind::Checkpoint] {
        let out = run(kind, sprint_course(), &racers, 1);
        assert_eq!(by_racer(&out.results, 1).status, RacerStatus::Finished);
        let r2 = by_racer(&out.results, 2);
        assert_eq!(
            r2.status,
            RacerStatus::Dnf(DnfReason::FinishWindow),
            "{kind:?}"
        );
        assert_eq!(r2.gates_passed, 0);
    }
}

#[test]
fn teleport_is_implausible() {
    let racers = [
        ScriptedRacer::new(1, 5.0, 40.0),
        ScriptedRacer::new(2, 5.0, 40.0).with(4.0, ScriptedIncident::Teleport { jump_m: 300.0 }),
    ];
    let out = run(RaceKind::Sprint, sprint_course(), &racers, 1);
    assert_eq!(
        by_racer(&out.results, 2).status,
        RacerStatus::Dq(DqReason::ImplausibleMovement)
    );
}

#[test]
fn dnf_and_dq_ordering() {
    let racers = [
        ScriptedRacer::new(1, 5.0, 40.0).with(3.0, ScriptedIncident::TotalLoss),
        ScriptedRacer::new(2, 5.0, 40.0).with(10.0, ScriptedIncident::Captured),
        ScriptedRacer::new(3, 4.0, 30.0),
        ScriptedRacer::new(4, 6.0, 50.0).with(15.0, ScriptedIncident::Teleport { jump_m: 400.0 }),
        ScriptedRacer::new(5, 5.0, 40.0).with(12.0, ScriptedIncident::Retire),
    ];
    let out = run(RaceKind::Sprint, sprint_course(), &racers, 1);
    let order: Vec<u32> = out.results.iter().map(|r| r.racer.0).collect();
    // Finisher, then DNFs by progress (retired at 12 s > captured at 10 s > total loss at 3 s),
    // then the DQ.
    assert_eq!(order, vec![3, 5, 2, 1, 4]);
    assert_eq!(
        by_racer(&out.results, 1).status,
        RacerStatus::Dnf(DnfReason::TotalLoss)
    );
    assert_eq!(
        by_racer(&out.results, 2).status,
        RacerStatus::Dnf(DnfReason::Captured)
    );
}

#[test]
fn time_limit_and_finish_window() {
    // Checkpoint: 420 s limit; a racer crawling at 1.5 m/s never finishes the 1 km.
    let racers = [ScriptedRacer::new(1, 5.0, 1.5)];
    let out = run(RaceKind::Checkpoint, sprint_course(), &racers, 4);
    assert_eq!(
        out.results[0].status,
        RacerStatus::Dnf(DnfReason::TimeLimit)
    );
    // Sprint: 60 s after the winner the rest are DNF.
    let racers = [
        ScriptedRacer::new(1, 5.0, 40.0),
        ScriptedRacer::new(2, 5.0, 40.0).with(5.0, ScriptedIncident::Stop),
    ];
    let out = run(RaceKind::Sprint, sprint_course(), &racers, 4);
    assert_eq!(
        by_racer(&out.results, 2).status,
        RacerStatus::Dnf(DnfReason::FinishWindow)
    );
}

// ---------------------------------------------------------------- clean race

#[test]
fn clean_race_detection() {
    let th = data().rules.common.contact_clean_threshold_ns;
    let racers = [
        // A light lean on the other racer: still clean.
        ScriptedRacer::new(1, 5.0, 40.0).with(
            4.0,
            ScriptedIncident::Contact {
                other: EntityId(2),
                impulse_ns: th * 0.5,
            },
        ),
        // Hard hit on a wall: not clean.
        ScriptedRacer::new(2, 5.0, 40.0).with(
            4.0,
            ScriptedIncident::Contact {
                other: WORLD_ENTITY,
                impulse_ns: th * 2.0,
            },
        ),
        // Clipped a civilian (low impulse, but over the civilian threshold): not clean.
        ScriptedRacer::new(3, 5.0, 40.0).with(
            4.0,
            ScriptedIncident::Contact {
                other: EntityId(900),
                impulse_ns: 600.0,
            },
        ),
        // Bumped by police: a contact, not a civilian hit.
        ScriptedRacer::new(4, 5.0, 40.0).with(
            4.0,
            ScriptedIncident::Contact {
                other: EntityId(800),
                impulse_ns: th * 1.5,
            },
        ),
        // Collision violation reported by wp_world: not clean; speeding alone is fine.
        ScriptedRacer::new(5, 5.0, 40.0)
            .with(
                3.0,
                ScriptedIncident::Violation(ViolationKind::Speeding { over_mps: 9.0 }),
            )
            .with(4.0, ScriptedIncident::Violation(ViolationKind::Collision)),
        ScriptedRacer::new(6, 5.0, 40.0).with(
            3.0,
            ScriptedIncident::Violation(ViolationKind::Speeding { over_mps: 9.0 }),
        ),
    ];
    let mut j = judge(RaceKind::Sprint, sprint_course());
    j.set_police(&[EntityId(800)]);
    let out = simulate(j, &racers, 1, 240 * 600);
    let r = &out.results;
    assert!(by_racer(r, 1).clean);
    assert!(!by_racer(r, 2).clean && by_racer(r, 2).contacts == 1);
    assert!(!by_racer(r, 3).clean && by_racer(r, 3).civilian_hits == 1);
    let p = by_racer(r, 4);
    assert!(!p.clean && p.contacts == 1 && p.civilian_hits == 0);
    assert!(!by_racer(r, 5).clean);
    assert!(by_racer(r, 6).clean && by_racer(r, 6).violations[0] == 1);
}

// ---------------------------------------------------------------- delivery

#[test]
fn delivery_cargo_damage_costs_and_destruction_is_dnf() {
    let kind = RaceKind::Delivery {
        cargo_kg: 150.0,
        cargo_value: 5000,
    };
    let d = data().rules.delivery;
    let hit = |_at: f32, ns| ScriptedIncident::Contact {
        other: WORLD_ENTITY,
        impulse_ns: ns,
    };
    let _ = d.cargo_threshold_ns;
    let racers = [
        ScriptedRacer::new(1, 5.0, 40.0),
        ScriptedRacer::new(2, 5.0, 40.0).with(4.0, hit(4.0, d.cargo_threshold_ns + 3000.0)),
        ScriptedRacer::new(3, 5.0, 40.0)
            .with(4.0, hit(4.0, d.cargo_threshold_ns + d.cargo_fragility_ns)),
    ];
    let out = run(kind, sprint_course(), &racers, 1);
    let r = &out.results;
    assert_eq!(by_racer(r, 1).cargo_damage_cost, 0);
    let dmg = by_racer(r, 2);
    assert_eq!(dmg.status, RacerStatus::Finished);
    assert_eq!(dmg.cargo_damage_cost, 1250, "3000 / 12000 of 5000");
    assert_eq!(
        by_racer(r, 3).status,
        RacerStatus::Dnf(DnfReason::CargoDestroyed)
    );
}

// ---------------------------------------------------------------- drag

fn drag_course() -> Course {
    Course::straight([0.0; 3], [0.0, 1.0], 402.0, 1000.0, 10.0)
}

#[test]
fn drag_reaction_shifts_trap_and_jump_start() {
    let mut a = ScriptedRacer::new(1, 7.0, 60.0);
    a.launch_s = 0.25;
    a.shift_frac = 0.95;
    let mut b = ScriptedRacer::new(2, 7.0, 60.0);
    b.launch_s = 0.40;
    b.shift_frac = 0.75; // short-shifting
    let out = run(RaceKind::Drag, drag_course(), &[a.clone(), b.clone()], 1);
    let ra = by_racer(&out.results, 1);
    let rb = by_racer(&out.results, 2);
    assert_eq!(ra.position, 1, "better reaction wins an equal-car pass");
    let da = ra.drag.unwrap();
    let db = rb.drag.unwrap();
    assert!((da.reaction_s().unwrap() - 0.25).abs() < 0.01);
    assert!((db.reaction_s().unwrap() - 0.40).abs() < 0.01);
    assert!(da.shifts >= 3 && da.shift_score() == 1.0);
    assert!(db.shifts >= 3 && db.shift_score() == 0.0);
    assert!((50.0..=60.0).contains(&da.trap_speed_mps));
    let et = ra.elapsed_s().unwrap();
    assert!((11.0..13.0).contains(&et), "ET {et}");

    // Jump start.
    a.launch_s = -0.6;
    let out = run(RaceKind::Drag, drag_course(), &[a, b], 1);
    assert_eq!(
        by_racer(&out.results, 1).status,
        RacerStatus::Dq(DqReason::JumpStart)
    );
    assert_eq!(by_racer(&out.results, 2).position, 1);
}

// ---------------------------------------------------------------- outrun

fn road() -> Course {
    Course::straight([0.0; 3], [0.0, 1.0], 20_000.0, 20_000.0, 12.0)
}

#[test]
fn outrun_lead_wins() {
    let kind = RaceKind::Outrun { lead_m: 100.0 };
    // The second-slot car is faster: it takes the lead, then builds 100 m.
    let racers = [
        ScriptedRacer::new(1, 5.0, 40.0),
        ScriptedRacer::new(2, 5.0, 45.0),
    ];
    let out = run(kind, road(), &racers, 4);
    assert_eq!(out.results[0].racer, EntityId(2));
    assert_eq!(
        by_racer(&out.results, 1).status,
        RacerStatus::Dnf(DnfReason::Outrun)
    );
    assert!(out
        .signals
        .iter()
        .any(|s| matches!(s, RaceSignal::LeadChanged { leader, .. } if *leader == EntityId(2))));
    // 100 m at 5 m/s closing after both top out: well under the 300 s limit.
    assert!(out.ticks < 240 * 60);
}

#[test]
fn outrun_time_limit_and_survivor() {
    let kind = RaceKind::Outrun { lead_m: 10_000.0 };
    let racers = [
        ScriptedRacer::new(1, 5.0, 41.0),
        ScriptedRacer::new(2, 5.0, 40.0),
    ];
    let out = run(kind, road(), &racers, 4);
    assert_eq!(
        out.results[0].racer,
        EntityId(1),
        "leader at the limit wins"
    );
    let racers = [
        ScriptedRacer::new(1, 5.0, 45.0).with(5.0, ScriptedIncident::TotalLoss),
        ScriptedRacer::new(2, 5.0, 40.0),
    ];
    let out = run(kind, road(), &racers, 4);
    assert_eq!(out.results[0].racer, EntityId(2));
    assert_eq!(out.results[0].status, RacerStatus::Finished);
}

#[test]
fn outrun_gap_is_measured_along_the_road() {
    // A switchback: 150 m legs, 30 m apart. The straight-line gap between the two cars never
    // exceeds ~155 m, but the leader pulls 250 m ahead along the road and wins on the gap well
    // before the 300 s limit. (Measured in a straight line it would go to the time limit.)
    let mut pts = Vec::new();
    for k in 0..40 {
        let x = k as f32 * 30.0;
        if k % 2 == 0 {
            pts.push([x, 0.0, 0.0]);
            pts.push([x, 150.0, 0.0]);
        } else {
            pts.push([x, 150.0, 0.0]);
            pts.push([x, 0.0, 0.0]);
        }
    }
    let switchback = Course::new(pts, Vec::new(), false, 12.0);
    let kind = RaceKind::Outrun { lead_m: 250.0 };
    let racers = [
        ScriptedRacer::new(1, 5.0, 25.0),
        ScriptedRacer::new(2, 5.0, 20.0),
    ];
    let out = run(kind, switchback, &racers, 4);
    assert_eq!(out.results[0].racer, EntityId(1));
    assert_eq!(
        by_racer(&out.results, 2).status,
        RacerStatus::Dnf(DnfReason::Outrun)
    );
    // 250 m at 5 m/s closing ≈ 50 s after both top out.
    assert!(out.ticks < 240 * 120, "took {} s", out.ticks / 240);
}

// ---------------------------------------------------------------- time attack

#[test]
fn time_attack_ranks_by_own_clock() {
    let course = Course::straight([0.0; 3], [0.0, 1.0], 600.0, 200.0, 12.0);
    // Slot 1 starts 12 m further back but is faster: it crosses the finish later yet wins.
    let racers = [
        ScriptedRacer::new(1, 5.0, 38.0),
        ScriptedRacer::new(2, 5.0, 39.0),
    ];
    let out = run(RaceKind::TimeAttack, course, &racers, 1);
    let fin: Vec<EntityId> = out
        .signals
        .iter()
        .filter_map(|s| match s {
            RaceSignal::RacerFinished { racer, .. } => Some(*racer),
            _ => None,
        })
        .collect();
    assert_eq!(fin, vec![EntityId(1), EntityId(2)], "slot 0 crosses first");
    assert_eq!(
        out.results[0].racer,
        EntityId(2),
        "slot 1 has the better time"
    );
}

// ---------------------------------------------------------------- ties

#[test]
fn identical_racers_resolve_by_grid_slot() {
    let racers = [
        ScriptedRacer::new(7, 5.0, 40.0),
        ScriptedRacer::new(3, 5.0, 40.0),
    ];
    // Drag grid is side by side: same distance, same time → pole (first entered) wins.
    let out = run(RaceKind::Drag, drag_course(), &racers, 1);
    assert_eq!(
        out.results[0].elapsed_subticks,
        out.results[1].elapsed_subticks
    );
    assert_eq!(out.results[0].racer, EntityId(7));
    assert_eq!(out.results[0].grid_slot, 0);
}

// ---------------------------------------------------------------- determinism / properties

#[test]
fn simulation_is_deterministic() {
    let racers = [
        ScriptedRacer::new(1, 5.0, 40.0).with(
            3.0,
            ScriptedIncident::Contact {
                other: EntityId(2),
                impulse_ns: 3000.0,
            },
        ),
        ScriptedRacer::new(2, 5.5, 39.0),
        ScriptedRacer::new(3, 4.5, 42.0),
    ];
    let a = run(RaceKind::Circuit { laps: 2 }, square_course(), &racers, 4);
    let b = run(RaceKind::Circuit { laps: 2 }, square_course(), &racers, 4);
    assert_eq!(a, b);
}

fn incident() -> impl Strategy<Value = Option<(f32, ScriptedIncident)>> {
    prop_oneof![
        4 => Just(None),
        1 => (1.0f32..20.0).prop_map(|t| Some((t, ScriptedIncident::TotalLoss))),
        1 => (1.0f32..20.0).prop_map(|t| Some((t, ScriptedIncident::Captured))),
        1 => (1.0f32..20.0).prop_map(|t| Some((t, ScriptedIncident::Swerve { offset_m: 40.0 }))),
        1 => (1.0f32..20.0).prop_map(|t| Some((t, ScriptedIncident::Teleport { jump_m: 300.0 }))),
        1 => (1.0f32..20.0).prop_map(|t| Some((t, ScriptedIncident::Contact { other: WORLD_ENTITY, impulse_ns: 5000.0 }))),
    ]
}

fn racer_strategy() -> impl Strategy<Value = (f32, f32, f32, Option<(f32, ScriptedIncident)>)> {
    // Quantised speeds make exact ties likely, exercising the tie-breaks.
    (
        (8u32..14).prop_map(|a| a as f32 * 0.5),
        (70u32..90).prop_map(|v| v as f32 * 0.5),
        (0u32..4).prop_map(|l| l as f32 * 0.1),
        incident(),
    )
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    #[test]
    fn scoring_is_a_total_consistent_order(
        specs in prop::collection::vec(racer_strategy(), 2..7),
        circuit in any::<bool>(),
        every in 1u64..5,
    ) {
        let racers: Vec<ScriptedRacer> = specs
            .iter()
            .enumerate()
            .map(|(i, (a, v, l, inc))| {
                let mut r = ScriptedRacer::new(10 + i as u32, *a, *v);
                r.launch_s = *l;
                if let Some((t, w)) = inc {
                    r = r.with(*t, *w);
                }
                r
            })
            .collect();
        let (kind, course) = if circuit {
            (RaceKind::Circuit { laps: 2 }, square_course())
        } else {
            (RaceKind::Sprint, sprint_course())
        };
        let out = run(kind, course, &racers, every);
        let r = &out.results;
        // Positions are exactly 1..=n and every racer appears once.
        let mut pos: Vec<u8> = r.iter().map(|x| x.position).collect();
        pos.sort();
        prop_assert_eq!(pos, (1..=r.len() as u8).collect::<Vec<_>>());
        let mut ids: Vec<u32> = r.iter().map(|x| x.racer.0).collect();
        ids.sort();
        ids.dedup();
        prop_assert_eq!(ids.len(), r.len());
        // No unresolved ties: rank keys are strictly increasing.
        for w in r.windows(2) {
            prop_assert!(w[0].rank_key() < w[1].rank_key());
            // Finishers ≺ DNF ≺ DQ.
            prop_assert!(w[0].status.class() <= w[1].status.class());
            if w[0].status == RacerStatus::Finished && w[1].status == RacerStatus::Finished {
                prop_assert!(w[0].elapsed_subticks <= w[1].elapsed_subticks);
            }
        }
        // Race always ends and nobody is left running.
        prop_assert!(r.iter().all(|x| x.status != RacerStatus::Running));
        // Clean implies finished.
        prop_assert!(r.iter().all(|x| !x.clean || x.status == RacerStatus::Finished));
    }

    #[test]
    fn stakes_are_monotone_with_risk(
        kind_i in 0usize..8,
        bracket_i in 0usize..6,
        a in (0usize..4, 0usize..3),
        b in (0usize..4, 0usize..3),
    ) {
        let d = data();
        let kind = RaceKind::from_key(RaceKind::KEYS[kind_i]).unwrap();
        let bracket = ClassBracket::ALL[bracket_i];
        let phases = [DayPhase::Dawn, DayPhase::Day, DayPhase::Dusk, DayPhase::Night];
        let legal = [Legality::Sanctioned, Legality::Street, Legality::IllegalDaytime];
        let sa = d.stakes.stake(kind, phases[a.0], bracket, legal[a.1]);
        let sb = d.stakes.stake(kind, phases[b.0], bracket, legal[b.1]);
        if sa.risk < sb.risk {
            prop_assert!(sa.purse <= sb.purse, "{:?} vs {:?}", sa, sb);
        }
        // Higher bracket never pays less at equal risk.
        if bracket_i > 0 {
            let lower = d.stakes.stake(kind, phases[a.0], ClassBracket::ALL[bracket_i - 1], legal[a.1]);
            prop_assert!(lower.purse <= sa.purse);
        }
    }
}

// ---------------------------------------------------------------- stakes

#[test]
fn stake_table_feel() {
    let s = &data().stakes;
    let night = s.stake(
        RaceKind::Sprint,
        DayPhase::Night,
        ClassBracket::D,
        Legality::Street,
    );
    let day_illegal = s.stake(
        RaceKind::Sprint,
        DayPhase::Day,
        ClassBracket::D,
        Legality::IllegalDaytime,
    );
    let sanctioned = s.stake(
        RaceKind::Circuit { laps: 3 },
        DayPhase::Day,
        ClassBracket::D,
        Legality::Sanctioned,
    );
    assert!((2500..3200).contains(&night.purse), "{night:?}");
    assert!(day_illegal.purse > 2 * night.purse, "{day_illegal:?}");
    assert_eq!(day_illegal.police, PolicePresence::Max);
    assert!(day_illegal.heat > night.heat);
    assert_eq!(sanctioned.heat, 0.0);
    assert_eq!(sanctioned.police, PolicePresence::None);
    assert!(sanctioned.purse < night.purse);
    assert!(night.entry_fee > 0 && night.entry_fee < night.purse / 5);
}

// ---------------------------------------------------------------- scheduler

fn venues() -> Vec<Venue> {
    (0..6)
        .map(|i| Venue {
            id: VenueId(i),
            name: format!("venue {i}"),
            pos: [i as f32 * 500.0, 0.0, 0.0],
            district: Some(DistrictId(i / 2)),
            closed_circuit: i == 4,
        })
        .collect()
}

#[test]
fn calendar_is_deterministic_and_well_formed() {
    let d = data();
    let s = Scheduler {
        rules: &d.schedule,
        stakes: &d.stakes,
        seed: 42,
    };
    let v = venues();
    let heat = MeetHeat::default();
    let a = s.generate_day(3, &v, &heat);
    assert_eq!(a, s.generate_day(3, &v, &heat));
    assert_ne!(a, s.generate_day(4, &v, &heat));
    let mut sanctioned = 0;
    let mut meets = 0;
    for e in &a.events {
        assert!(e.spawn_s <= e.lobby_open_s && e.lobby_open_s <= e.start_s);
        assert_eq!(e.requirements.min_bracket, e.bracket);
        match e.legality {
            Legality::Sanctioned => {
                sanctioned += 1;
                assert_eq!(e.phase, DayPhase::Day);
                assert_eq!(e.traffic, TrafficMode::Suppressed);
                assert_eq!(e.marshals, d.schedule.marshals);
                assert_eq!(e.venue, VenueId(4), "only the circuit venue");
                assert_eq!(e.police_presence_override, PolicePresence::None);
            }
            Legality::Street => {
                meets += 1;
                assert_eq!(e.phase, DayPhase::Night, "{e:?}");
            }
            Legality::IllegalDaytime => {
                assert_eq!(e.phase, DayPhase::Day);
                assert_eq!(e.police_presence_override, PolicePresence::Max);
            }
        }
    }
    assert!((1..=2).contains(&sanctioned));
    assert!(meets >= 4, "{meets} meets");
    // Sorted, and same-venue events respect the gap.
    for w in a.events.windows(2) {
        assert!(w[0].start_s <= w[1].start_s);
    }
    for x in &a.events {
        for y in &a.events {
            if x.id != y.id && x.venue == y.venue {
                assert!(x.start_s.abs_diff(y.start_s) >= d.schedule.venue_gap_min * 60);
            }
        }
    }
}

#[test]
fn illegal_daytime_races_are_rare_and_rich() {
    let d = data();
    let s = Scheduler {
        rules: &d.schedule,
        stakes: &d.stakes,
        seed: 7,
    };
    let v = venues();
    let heat = MeetHeat::default();
    let mut days_with = 0;
    let mut max_night = 0;
    let mut min_illegal = i64::MAX;
    for day in 0..1000 {
        let cal = s.generate_day(day, &v, &heat);
        for e in &cal.events {
            match e.legality {
                Legality::IllegalDaytime => min_illegal = min_illegal.min(e.stake.purse),
                Legality::Street if e.bracket == ClassBracket::D => {
                    max_night = max_night.max(e.stake.purse)
                }
                _ => {}
            }
        }
        if cal
            .events
            .iter()
            .any(|e| e.legality == Legality::IllegalDaytime)
        {
            days_with += 1;
        }
    }
    let p = d.schedule.illegal_daytime.chance_per_day;
    let expect = (p * 1000.0) as i32;
    assert!(
        (days_with - expect).abs() < 30,
        "{days_with} days with an illegal daytime race (expected ~{expect})"
    );
    assert!(min_illegal > 0);
    let _ = max_night;
}

#[test]
fn meet_heat_attracts_patrols_and_steers_the_scheduler() {
    let d = data();
    let rules = &d.schedule.meet_heat;
    let v = venues();
    let mut heat = MeetHeat::default();
    let t0 = 22 * 3600;
    assert!(heat.record_race(&v[0], t0, rules).is_none());
    assert!(heat.record_race(&v[0], t0 + 600, rules).is_none());
    let sig = heat.record_race(&v[0], t0 + 1200, rules);
    assert!(matches!(sig, Some(RaceSignal::PatrolAttracted { venue, .. }) if venue == VenueId(0)));
    // Decays over a day.
    assert_eq!(heat.heat_at(VenueId(0), t0 + 86_400, rules), 0.0);

    // A venue kept hot is chosen much less often.
    let s = Scheduler {
        rules: &d.schedule,
        stakes: &d.stakes,
        seed: 11,
    };
    let mut hot_picks = 0;
    let mut cold_picks = 0;
    for day in 0..200u32 {
        let mut h = MeetHeat::default();
        let night = day as u64 * 86_400 + 21 * 3600;
        for k in 0..5 {
            h.record_race(&v[0], night - 60 * k, rules);
        }
        for e in s.generate_day(day, &v, &h).events {
            if e.legality == Legality::Street {
                match e.venue.0 {
                    0 => hot_picks += 1,
                    1 => cold_picks += 1,
                    _ => {}
                }
            }
        }
    }
    assert!(
        hot_picks * 2 < cold_picks,
        "hot {hot_picks} cold {cold_picks}"
    );
}

// ---------------------------------------------------------------- crews

fn acct(n: u64) -> RacerAccount {
    RacerAccount(n)
}

#[test]
fn crew_lifecycle_and_bank_cut() {
    let d = data();
    let mut reg = CrewRegistry::new(d.crew.clone());
    let c = reg.create("Night Owls", acct(1)).unwrap();
    assert!(matches!(
        reg.create("night owls", acct(2)),
        Err(CrewError::BadName(_))
    ));
    assert_eq!(
        reg.create("Other", acct(1)),
        Err(CrewError::AlreadyInCrew(acct(1)))
    );
    for i in 2..=d.crew.max_members as u64 {
        reg.join(c, acct(i)).unwrap();
    }
    assert_eq!(reg.join(c, acct(99)), Err(CrewError::Full));
    // Bank cut.
    assert_eq!(reg.set_bank_cut(acct(2), 0.2), Err(CrewError::NotLeader));
    assert_eq!(reg.set_bank_cut(acct(1), 0.9).unwrap(), d.crew.max_bank_cut);
    reg.set_bank_cut(acct(1), 0.2).unwrap();
    let cut = reg.credit_race(acct(3), 3000, 120);
    assert_eq!(cut.crew_purse, 600);
    assert_eq!(cut.racer_purse, 2400);
    assert_eq!(cut.crew_rep, 30);
    assert_eq!(reg.crew(c).unwrap().bank, 600);
    let solo = reg.credit_race(acct(500), 3000, 120);
    assert_eq!(solo.racer_purse, 3000);
    // Leader leaves → lowest account leads.
    reg.leave(acct(1)).unwrap();
    assert_eq!(reg.crew(c).unwrap().leader, acct(2));
    assert_eq!(reg.withdraw(acct(2), 10_000).unwrap(), 600);
    // Last one out dissolves the crew.
    for i in 2..=d.crew.max_members as u64 {
        reg.leave(acct(i)).unwrap();
    }
    assert!(reg.crew(c).is_none());
}

fn two_crews() -> (CrewRegistry, CrewId, CrewId) {
    let mut reg = CrewRegistry::new(data().crew);
    let a = reg.create("Alpha", acct(1)).unwrap();
    reg.join(a, acct(2)).unwrap();
    let b = reg.create("Bravo", acct(10)).unwrap();
    reg.join(b, acct(11)).unwrap();
    reg.credit_race(acct(1), 10_000, 0);
    reg.credit_race(acct(10), 10_000, 0);
    (reg, a, b)
}

fn crew_lookup(a: CrewId, b: CrewId) -> impl Fn(EntityId) -> Option<CrewId> {
    // Entities 1, 2 race for crew a; 10, 11 for crew b.
    move |e: EntityId| match e.0 {
        1 | 2 => Some(a),
        10 | 11 => Some(b),
        _ => None,
    }
}

#[test]
fn crew_challenge_with_pink_slip() {
    let (mut reg, a, b) = two_crews();
    let bank_a = reg.crew(a).unwrap().bank;
    let id = reg
        .issue_challenge(
            acct(1),
            b,
            DistrictId(2),
            RaceKind::Sprint,
            ClassBracket::C,
            500,
            true,
        )
        .unwrap();
    assert_eq!(
        reg.answer_challenge(acct(1), id, true),
        Err(CrewError::NotLeader)
    );
    assert_eq!(
        reg.answer_challenge(acct(10), id, true).unwrap(),
        ChallengeState::Accepted
    );
    assert_eq!(reg.crew(a).unwrap().bank, bank_a - 500);
    let racers = [
        ScriptedRacer::new(1, 5.0, 38.0),
        ScriptedRacer::new(10, 5.0, 40.0),
        ScriptedRacer::new(2, 5.0, 37.0),
        ScriptedRacer::new(11, 5.0, 36.0),
    ];
    let out = run(RaceKind::Sprint, sprint_course(), &racers, 4);
    let lookup = crew_lookup(a, b);
    let o = reg
        .resolve_challenge(id, &out.results, &lookup)
        .unwrap()
        .unwrap();
    assert_eq!(o.winner, b);
    assert_eq!(o.pot, 1000);
    // Winner's best racer takes the loser crew's last-placed racer's car.
    assert_eq!(o.pink_slip, Some((EntityId(10), EntityId(2))));
    assert_eq!(reg.challenge(id).unwrap().state, ChallengeState::Resolved);
    assert!(reg.resolve_challenge(id, &out.results, &lookup).is_err());
}

#[test]
fn crew_challenge_without_finishers_refunds() {
    let (mut reg, a, b) = two_crews();
    let id = reg
        .issue_challenge(
            acct(1),
            b,
            DistrictId(2),
            RaceKind::Sprint,
            ClassBracket::C,
            500,
            false,
        )
        .unwrap();
    reg.answer_challenge(acct(10), id, true).unwrap();
    let racers = [
        ScriptedRacer::new(1, 5.0, 38.0).with(2.0, ScriptedIncident::TotalLoss),
        ScriptedRacer::new(10, 5.0, 40.0).with(2.0, ScriptedIncident::Captured),
    ];
    let out = run(RaceKind::Sprint, sprint_course(), &racers, 4);
    let lookup = crew_lookup(a, b);
    assert_eq!(
        reg.resolve_challenge(id, &out.results, &lookup).unwrap(),
        None
    );
    assert_eq!(reg.crew(a).unwrap().bank, 1000);
    assert_eq!(reg.crew(b).unwrap().bank, 1000);
}

// ---------------------------------------------------------------- territory

fn win_for(racer: u32, clean: bool) -> Vec<RacerResult> {
    let racers = [
        ScriptedRacer::new(racer, 5.0, 45.0).with(
            if clean { 999.0 } else { 3.0 },
            ScriptedIncident::Contact {
                other: WORLD_ENTITY,
                impulse_ns: 9000.0,
            },
        ),
        ScriptedRacer::new(99, 5.0, 30.0),
    ];
    run(RaceKind::Sprint, sprint_course(), &racers, 4).results
}

#[test]
fn territory_control_decay_and_benefits() {
    let d = data();
    let a = CrewId(1);
    let b = CrewId(2);
    let lookup = crew_lookup(a, b);
    let mut t = Territory::new(d.territory.clone());
    let dist = DistrictId(5);
    t.set_safehouse(dist, [10.0, 20.0, 0.0]);
    // Street wins until the threshold.
    let per_win = d.territory.win_points["street"] + d.territory.clean_bonus;
    let wins_needed = d.territory.control_threshold.div_ceil(per_win);
    let mut changed = Vec::new();
    for _ in 0..wins_needed {
        assert_eq!(t.owner(dist), None);
        changed.extend(t.award_race(dist, Legality::Street, &win_for(1, true), &lookup));
    }
    assert_eq!(t.owner(dist), Some(a));
    assert_eq!(
        changed,
        vec![RaceSignal::TerritoryChanged {
            district: dist,
            crew: Some(a)
        }]
    );
    assert_eq!(
        t.heat_decay_multiplier(Some(a), dist),
        1.0 + d.territory.heat_decay_bonus
    );
    assert_eq!(t.heat_decay_multiplier(Some(b), dist), 1.0);
    assert_eq!(t.safehouse_for(Some(a), dist), Some([10.0, 20.0, 0.0]));
    assert_eq!(t.safehouse_for(Some(b), dist), None);
    assert_eq!(t.controlled_by(a), vec![dist]);

    // A tie keeps the incumbent: give b exactly a's points.
    let pa = t.district(dist).unwrap().points[&a];
    let mut tb = t.clone();
    let lookup_b = crew_lookup(a, b);
    while tb
        .district(dist)
        .unwrap()
        .points
        .get(&b)
        .copied()
        .unwrap_or(0)
        < pa
    {
        tb.award_race(dist, Legality::Sanctioned, &win_for(10, false), &lookup_b);
    }
    let pb = tb.district(dist).unwrap().points[&b];
    if pb == pa {
        assert_eq!(tb.owner(dist), Some(a));
    } else {
        assert_eq!(tb.owner(dist), Some(b));
    }

    // Challenge: b wins and steals a share of a's points.
    let before = t.district(dist).unwrap().points[&a];
    t.award_challenge(dist, b, a);
    let after = t.district(dist).unwrap().points[&a];
    let stolen = (before as f32 * d.territory.challenge_steal).round() as u32;
    assert_eq!(after, before - stolen);
    assert_eq!(
        t.district(dist).unwrap().points[&b],
        d.territory.challenge_points + stolen
    );

    // a wins its turf back, then nobody races for weeks: control lapses through decay.
    while t.owner(dist) != Some(a) {
        t.award_race(dist, Legality::IllegalDaytime, &win_for(1, true), &lookup);
    }
    let mut lost = false;
    for day in 1..=90 {
        for s in t.advance_to_day(day) {
            if s == (RaceSignal::TerritoryChanged {
                district: dist,
                crew: None,
            }) {
                lost = true;
            }
        }
    }
    assert!(lost);
    assert_eq!(t.owner(dist), None);
}

// ---------------------------------------------------------------- progression

#[test]
fn progression_gates() {
    let p = data().progression;
    assert_eq!(p.bracket_for(50), ClassBracket::D);
    assert_eq!(p.bracket_for(299), ClassBracket::D);
    assert_eq!(p.bracket_for(300), ClassBracket::C);
    assert_eq!(p.bracket_for(999), ClassBracket::X);
    assert_eq!(p.licence_for(0), 0);
    assert_eq!(p.licence_for(5), 1);
    assert_eq!(p.licence_for(1000), 4);
    assert_eq!(p.clean_races_to_next(3), Some(2));
    assert_eq!(p.clean_races_to_next(100), None);
    assert_eq!(p.licence_name(2), "Pro");

    let prof = RacerProfile {
        account: acct(1),
        rep: 500,
        clean_races: 6,
        crew: Some(CrewId(3)),
    };
    let mut req = EntryRequirements::bracket(ClassBracket::C);
    assert!(p.check_entry(&prof, 350, &req).is_ok());
    assert!(matches!(
        p.check_entry(&prof, 460, &req),
        Err(EntryDenied::WrongBracket { car: "B", .. })
    ));
    req.min_licence = 2;
    assert_eq!(
        p.check_entry(&prof, 350, &req),
        Err(EntryDenied::Licence { needed: 2, has: 1 })
    );
    req.min_licence = 0;
    req.min_rep = 1000;
    assert_eq!(
        p.check_entry(&prof, 350, &req),
        Err(EntryDenied::Rep {
            needed: 1000,
            has: 500
        })
    );
    req.min_rep = 0;
    req.crews = vec![CrewId(4)];
    assert_eq!(p.check_entry(&prof, 350, &req), Err(EntryDenied::Crew));
    req.crews.push(CrewId(3));
    assert!(p.check_entry(&prof, 350, &req).is_ok());
}

// ---------------------------------------------------------------- economy bridge

#[test]
fn economy_bridge_maps_results() {
    use wp_contracts::WantedLevel;
    let racers = [
        ScriptedRacer::new(1, 5.0, 40.0)
            .with(3.0, ScriptedIncident::Violation(ViolationKind::RedLight))
            .with(
                4.0,
                ScriptedIncident::Contact {
                    other: EntityId(500),
                    impulse_ns: 900.0,
                },
            ),
        ScriptedRacer::new(2, 5.0, 30.0).with(3.0, ScriptedIncident::TotalLoss),
    ];
    let out = run(RaceKind::Sprint, sprint_course(), &racers, 4);
    let w = crate::bridge::economy_result(by_racer(&out.results, 1), WantedLevel::None);
    assert_eq!(w.placement, 1);
    assert_eq!(w.contacts, 1);
    assert_eq!(w.violations.red_light, 1);
    assert!(!w.is_clean());
    let l = crate::bridge::economy_result(by_racer(&out.results, 2), WantedLevel::None);
    assert_eq!(l.placement, 0);
    assert_eq!(
        crate::bridge::economy_race_type(RaceKind::Outrun { lead_m: 1.0 }),
        wp_economy::RaceType::Touge
    );
}

#[test]
fn economy_race_pays_the_advertised_stake() {
    use crate::bridge::economy_race;
    use crate::kind::{ClassBracket, Legality};
    use wp_contracts::{DamageMode, DayPhase, VehicleClass, WantedLevel};
    use wp_economy::{Economy, RaceResult as EconResult, ViolationCounts};
    let econ = Economy::embedded();
    let data = crate::data::RaceData::embedded();
    let kinds = [
        RaceKind::Sprint,
        RaceKind::Circuit { laps: 3 },
        RaceKind::Outrun { lead_m: 200.0 },
        RaceKind::Checkpoint,
        RaceKind::Cannonball,
        RaceKind::Delivery {
            cargo_kg: 80.0,
            cargo_value: 4000,
        },
        RaceKind::Drag,
        RaceKind::TimeAttack,
    ];
    let legalities = [
        Legality::Sanctioned,
        Legality::Street,
        Legality::IllegalDaytime,
    ];
    let phases = [
        DayPhase::Dawn,
        DayPhase::Day,
        DayPhase::Dusk,
        DayPhase::Night,
    ];
    for kind in kinds {
        for legality in legalities {
            for phase in phases {
                let km = if kind == RaceKind::Drag { 0.402 } else { 6.0 };
                let r = economy_race(
                    &econ,
                    &data.stakes,
                    kind,
                    legality,
                    VehicleClass::Compact,
                    ClassBracket::D,
                    km * 1000.0,
                    phase,
                    DamageMode::Arcade,
                );
                let stake = data.stakes.stake(kind, phase, ClassBracket::D, legality);
                // Arcade: the economy, scaled, pays exactly what the lobby advertised.
                assert!(r.purse_scale.is_finite() && r.purse_scale > 0.0, "{kind:?}");
                assert!(
                    (r.purse - stake.purse).abs() <= 1,
                    "{kind:?} {legality:?} {phase:?}"
                );
                assert!((r.entry_fee - stake.entry_fee).abs() <= 1, "{kind:?}");
                // Settling a win with the scale applied reproduces the advertised purse.
                let win = EconResult {
                    placement: 1,
                    contacts: 1,
                    violations: ViolationCounts::default(),
                    wanted: WantedLevel::None,
                };
                let paid = econ.race_payout(&r.setup, &win).purse as f32 * r.purse_scale;
                let share = paid / r.purse as f32;
                assert!(share > 0.0 && share <= 1.001, "{kind:?}: share {share}");
            }
        }
    }
    // Both tables were tuned to the same feel target (a D-class / Compact 9 km night street
    // sprint pays ~2 800), so no scale is needed there.
    let r = economy_race(
        &econ,
        &data.stakes,
        RaceKind::Sprint,
        Legality::Street,
        VehicleClass::Compact,
        ClassBracket::D,
        9000.0,
        DayPhase::Night,
        DamageMode::Arcade,
    );
    assert!((0.9..1.1).contains(&r.purse_scale), "{}", r.purse_scale);
    // Simulation keeps the economy's mode premium on top of the stake.
    let sim = economy_race(
        &econ,
        &data.stakes,
        RaceKind::Sprint,
        Legality::Street,
        VehicleClass::Compact,
        ClassBracket::D,
        9000.0,
        DayPhase::Night,
        DamageMode::Simulation,
    );
    assert!(sim.purse > r.purse);
    // Agreed with G (round 9, race.md R5): both tables price legality, so the scale is not
    // what carries it. At every kind × legality × phase the calendar actually schedules, the
    // economy's own purse is within a third of the advertised stake (scale 0.75-1.33), and the
    // advertised illegal daytime race still pays more than 2x a sanctioned one.
    let rules = &data.schedule;
    let slots = [
        (&rules.sanctioned, Legality::Sanctioned),
        (&rules.illegal_daytime.slot, Legality::IllegalDaytime),
        (&rules.night_meets, Legality::Street),
    ];
    for (slot, legality) in slots {
        for h in [slot.hours[0], slot.hours[1] - 0.01] {
            let phase = wp_contracts::time::TimeOfDay::from_hours(h).phase();
            for key in slot.kinds.keys() {
                let kind = RaceKind::from_key(key).unwrap();
                let km = if kind == RaceKind::Drag { 0.402 } else { 9.0 };
                let r = economy_race(
                    &econ,
                    &data.stakes,
                    kind,
                    legality,
                    VehicleClass::Compact,
                    ClassBracket::D,
                    km * 1000.0,
                    phase,
                    DamageMode::Arcade,
                );
                assert!(
                    (0.75..=1.33).contains(&r.purse_scale),
                    "{key} {legality:?} {phase:?}: scale {:.2}",
                    r.purse_scale
                );
                assert!(
                    (0.5..=2.0).contains(&r.entry_scale),
                    "{key} {legality:?} {phase:?}: entry scale {:.2}",
                    r.entry_scale
                );
            }
        }
    }
    let advertised = |kind, legality| {
        economy_race(
            &econ,
            &data.stakes,
            kind,
            legality,
            VehicleClass::Compact,
            ClassBracket::D,
            9000.0,
            DayPhase::Day,
            DamageMode::Arcade,
        )
        .purse
    };
    // Illegal daytime cannonball vs the best-paid sanctioned event (a circuit).
    assert!(
        advertised(RaceKind::Cannonball, Legality::IllegalDaytime)
            > 2 * advertised(RaceKind::Circuit { laps: 3 }, Legality::Sanctioned)
    );
}

#[test]
fn economy_race_type_mapping_is_locked() {
    // The R5 mapping agreed for wp_economy's five contract race types.
    use crate::bridge::economy_race_type as t;
    use wp_economy::RaceType as E;
    assert_eq!(t(RaceKind::Sprint), E::Sprint);
    assert_eq!(t(RaceKind::Circuit { laps: 2 }), E::Circuit);
    assert_eq!(t(RaceKind::TimeAttack), E::Circuit);
    assert_eq!(t(RaceKind::Drag), E::Drag);
    assert_eq!(t(RaceKind::Outrun { lead_m: 150.0 }), E::Touge);
    assert_eq!(t(RaceKind::Checkpoint), E::Sprint);
    assert_eq!(t(RaceKind::Cannonball), E::Sprint);
    assert_eq!(
        t(RaceKind::Delivery {
            cargo_kg: 1.0,
            cargo_value: 1
        }),
        E::Sprint
    );
}

#[test]
fn a_race_where_everyone_left_is_stuck_ends_after_the_stuck_limit() {
    // The only racer stops 2 s in: no progress, so 180 s later the judge ends the race
    // (the checkpoint time limit is 420 s).
    let racers = [ScriptedRacer::new(1, 5.0, 40.0).with(2.0, ScriptedIncident::Stop)];
    let out = run(RaceKind::Checkpoint, sprint_course(), &racers, 4);
    let r = &out.results[0];
    assert_eq!(r.status, RacerStatus::Dnf(DnfReason::Stuck));
    let ended_s = r.status_tick as f32 / 240.0;
    assert!((180.0..190.0).contains(&ended_s), "ended at {ended_s} s");
    assert!(out.signals.iter().any(|s| matches!(
        s,
        RaceSignal::RacerDnf {
            reason: DnfReason::Stuck,
            ..
        }
    )));
    // One racer still crawling forward keeps the race going: nobody is called stuck.
    let racers = [
        ScriptedRacer::new(1, 5.0, 40.0).with(2.0, ScriptedIncident::Stop),
        ScriptedRacer::new(2, 5.0, 1.5),
    ];
    let out = run(RaceKind::Checkpoint, sprint_course(), &racers, 4);
    for r in &out.results {
        assert_eq!(r.status, RacerStatus::Dnf(DnfReason::TimeLimit), "{r:?}");
    }
}

/// The stuck rule follows lap wraps: a slow circuit (600 s at 4 m/s) is never `Stuck`.
#[test]
fn a_slow_circuit_racer_is_not_stuck() {
    let racers = [
        ScriptedRacer::new(1, 5.0, 4.0),
        ScriptedRacer::new(2, 5.0, 4.2),
    ];
    let out = run(RaceKind::Circuit { laps: 3 }, square_course(), &racers, 4);
    for r in &out.results {
        assert_eq!(r.status, RacerStatus::Finished, "{r:?}");
    }
}
