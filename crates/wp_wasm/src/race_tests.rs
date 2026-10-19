//! Native tests of picker races inside a live session (`crate::race`).

use wp_contracts::{DamageMode, EntityId, RaceEvent, SimEvent, VehicleInput, VehicleSim};

use crate::race::{race_cards, race_for_scenario, race_scenario, RaceReport};
use crate::session::Session;

const PLAYER: EntityId = EntityId(1);
const CAR: &str = "lindwurm_4t";

fn sprint(autopilot: bool) -> Session {
    let def = wp_race::catalog::race("grid_highway_sprint").unwrap();
    Session::new(
        race_scenario(&def, CAR, DamageMode::Arcade, autopilot).unwrap(),
        false,
    )
    .unwrap()
}

fn pos(s: &Session, id: EntityId) -> [f32; 3] {
    s.vehicle(id).unwrap().vehicle.state().pos
}

/// Run until the results are final (at most `max_s`), collecting race events.
fn run_to_final(s: &mut Session, max_s: u64) -> (RaceReport, Vec<RaceEvent>) {
    let mut race_events = Vec::new();
    for _ in 0..240 * max_s {
        s.run_ticks(1);
        for e in s.drain_events() {
            if let SimEvent::Race(r) = e {
                race_events.push(r);
            }
        }
        if let Some(r) = s.race_report().filter(|r| r.final_results) {
            return (r, race_events);
        }
        if std::env::var("WP_RACE_TRACE").is_ok() && s.tick() % (240 * 5) == 0 {
            for st in s.race().unwrap().standings() {
                eprintln!(
                    "t={} {:?} P{} {:?} gates {} prog {:.0} pos {:?}",
                    s.tick() / 240,
                    st.racer,
                    st.position,
                    st.status,
                    st.gates_passed,
                    st.progress_m,
                    pos(s, st.racer)
                );
            }
        }
    }
    panic!("race not over in {max_s} s");
}

#[test]
fn race_scenarios_name_the_race_and_put_the_player_last() {
    let def = wp_race::catalog::race("grid_highway_sprint").unwrap();
    let sc = race_scenario(&def, CAR, DamageMode::Arcade, false).unwrap();
    assert_eq!(race_for_scenario(&sc.name).unwrap().0.id, def.id);
    assert_eq!(sc.entities.len(), def.entrants());
    assert_eq!(sc.entities[0].id, "player");
    assert_eq!(sc.entities[0].car, CAR);
    let auto = race_scenario(&def, CAR, DamageMode::Arcade, true).unwrap();
    assert!(race_for_scenario(&auto.name).unwrap().1);
    // The picker advertises a purse and a buy-in for every race.
    let car = simcli::data::load_car(crate::data::mem_data(), CAR).unwrap();
    let cards = race_cards(Some(&car), DamageMode::Arcade);
    assert_eq!(cards.len(), wp_race::catalog::races().len());
    for c in &cards {
        assert!(
            c.purse > 0 && c.entry_fee > 0,
            "{}: {} / {}",
            c.id,
            c.purse,
            c.entry_fee
        );
        assert!((3..=5).contains(&c.drivers.len()));
    }
    // Illegal daytime pays more than a night street race of similar length.
    let by = |id: &str| cards.iter().find(|c| c.id == id).unwrap();
    assert!(by("lantern_checkpoint").purse > by("hollins_sprint").purse);
}

#[test]
fn player_is_held_on_the_grid_until_green() {
    let mut s = sprint(false);
    let start = pos(&s, PLAYER);
    let ai = EntityId(2);
    let ai_start = pos(&s, ai);
    let full = VehicleInput {
        throttle: 1.0,
        ..Default::default()
    };
    // Countdown: input refused, nobody moves.
    for _ in 0..240 * 2 {
        assert!(!s.set_input(PLAYER, full).unwrap());
        s.run_ticks(1);
    }
    let moved = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).hypot(a[1] - b[1]);
    assert!(moved(start, pos(&s, PLAYER)) < 0.5);
    assert!(moved(ai_start, pos(&s, ai)) < 0.5);
    let info = s.race().unwrap().info(s.tick());
    assert_eq!(info[0], 0.0, "still counting down");
    // After green the player drives and the AI pulls away.
    s.run_ticks(240 * 2);
    assert!(s.set_input(PLAYER, full).unwrap());
    s.run_ticks(240 * 3);
    assert!(moved(start, pos(&s, PLAYER)) > 10.0);
    assert!(moved(ai_start, pos(&s, ai)) > 20.0);
}

#[test]
fn short_sprint_with_ai_runs_to_results_and_pays_through_the_economy() {
    let mut s = sprint(true);
    let cash0_before_entry = {
        let run = s.run().expect("economy run");
        let entry = s.race().unwrap().entry().expect("buy-in charged").0;
        assert!(entry > 0);
        run.account().cash + entry
    };
    let (r, events) = run_to_final(&mut s, 120);
    assert!(r.final_results);
    assert_eq!(r.rows.len(), 4);
    // Race events reach the session's buffer, in order.
    assert!(matches!(events.first(), Some(RaceEvent::Countdown { .. })));
    assert!(events.iter().any(|e| matches!(e, RaceEvent::Green { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e, RaceEvent::CheckpointPassed { .. })));
    assert!(matches!(
        events.last(),
        Some(RaceEvent::RaceFinished { .. })
    ));
    // A 750 m sprint: at least three finish within a sensible time.
    let finishers: Vec<_> = r.rows.iter().filter(|x| x.status == "Finished").collect();
    assert!(finishers.len() >= 3, "{:?}", r.rows);
    let win = finishers[0].time_s.unwrap();
    assert!((15.0..60.0).contains(&win), "winner {win} s");
    assert_eq!(finishers[0].gap_s, Some(0.0));
    // The payout came from G's economy and the account reflects it.
    let p = r.payout.as_ref().expect("payout");
    let me = &r.rows[r.player_row];
    assert!(me.is_player && me.name == "You");
    if me.status == "Finished" {
        assert!(p.purse > 0 && p.rep > 0, "{p:?}");
    } else {
        assert_eq!(p.purse, 0);
    }
    if me.position == 1 {
        // The winner's purse is the advertised one within the agreed R5 band.
        let ratio = p.purse as f32 / r.advertised_purse as f32;
        assert!(
            (0.75..1.34).contains(&ratio),
            "purse {} vs {}",
            p.purse,
            r.advertised_purse
        );
    }
    let reasons: Vec<&str> = p.ledger.iter().map(|l| l.reason.as_str()).collect();
    assert!(
        reasons.contains(&"RaceEntryFee") && reasons.contains(&"RaceRep"),
        "{reasons:?}"
    );
    let cash = s.run().unwrap().account().cash;
    assert_eq!(
        cash,
        cash0_before_entry - p.entry_fee + p.purse + p.clean_bonus
    );
    assert_eq!(p.cash_after, cash);
    assert!(!r.handoff_run_end);
}

#[test]
fn race_replay_plays_back_without_drift() {
    let mut s = sprint(true);
    s.run_ticks(240 * 8);
    let replay = s.record_stop().unwrap();
    let mut p = Session::from_replay(replay.clone()).unwrap();
    assert!(p.race().is_some(), "playback rebuilds the race");
    p.run_ticks(replay.ticks);
    let st = p.replay_status().unwrap();
    assert_eq!(st.drift_count, 0, "{st:?}");
    assert_eq!(
        p.race().unwrap().standings(),
        s.race().unwrap().standings(),
        "same judging in playback"
    );
}

#[test]
fn a_capture_mid_race_is_a_dnf_and_hands_off_to_the_end_of_run_panel() {
    // The checkpoint run has no corridor rule (any path), so the player can be parked in
    // front of any patrol car without being DQ'd for leaving the route first.
    let def = wp_race::catalog::race("lantern_checkpoint").unwrap();
    let sc = race_scenario(&def, CAR, DamageMode::Arcade, false).unwrap();
    let mut s = Session::new(sc, false).unwrap();
    s.run_ticks(240 * 4);
    let mut parked = false;
    for _ in 0..240 * 30 {
        s.run_ticks(1);
        if s.debug_park_near_police(PLAYER, 14.0).is_ok() {
            s.debug_sim_mut()
                .unwrap()
                .police_mut()
                .set_heat(PLAYER, 30.0);
            parked = true;
            break;
        }
    }
    assert!(parked, "no police car spawned");
    let brake = VehicleInput {
        brake: 1.0,
        ..Default::default()
    };
    for _ in 0..240 * 60 {
        let _ = s.set_input(PLAYER, brake);
        s.run_ticks(1);
        if let Some(r) = s.race_report() {
            assert!(r.handoff_run_end, "{r:?}");
            let me = &r.rows[r.player_row];
            assert_eq!(me.status, "DNF");
            assert_eq!(me.reason.as_deref(), Some("Captured"));
            assert!(
                s.run().unwrap().is_over(),
                "the end-of-run panel takes over"
            );
            return;
        }
    }
    panic!("never captured");
}
