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
        if std::env::var("WP_RACE_TRACE").is_ok() && s.tick().is_multiple_of(240 * 5) {
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
    let idle_rpm = s.vehicle(PLAYER).unwrap().vehicle.state().engine_rpm;
    // Countdown: input refused (only the throttle gets through, clutch down), nobody moves.
    for _ in 0..240 * 2 {
        assert!(!s.set_input(PLAYER, full).unwrap());
        s.run_ticks(1);
    }
    let revving = s.vehicle(PLAYER).unwrap().vehicle.state().engine_rpm;
    assert!(
        revving > idle_rpm + 2000.0,
        "revs on the line: {idle_rpm} -> {revving} rpm"
    );
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
    // A 750 m sprint: at least two finish within a sensible time. The grid sprint's speed
    // trap (`data/police/races.toml`) takes racers out (seed 7: one captured, one fled), as
    // in `wp_race`'s `race_picker::grid_highway_sprint`.
    let finishers: Vec<_> = r.rows.iter().filter(|x| x.status == "Finished").collect();
    assert!(finishers.len() >= 2, "{:?}", r.rows);
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
    // The race's own ledger lines charge the buy-in and pay purse + clean bonus (stake
    // adjustments included). Other run events can land in between (a pursuit escaped
    // mid-race pays its bonus), so the account is checked through the race's lines.
    let race_cash: i64 = p.ledger.iter().map(|l| l.cash_delta).sum();
    assert_eq!(
        race_cash,
        -p.entry_fee + p.purse + p.clean_bonus,
        "{:?}",
        p.ledger
    );
    let cash = s.run().unwrap().account().cash;
    let first = p.ledger.first().map_or(0, |l| l.seq);
    let others: i64 = s
        .run()
        .unwrap()
        .ledger()
        .iter()
        .map(crate::run::LedgerLine::of)
        .filter(|l| l.seq > first && !p.ledger.iter().any(|r| r.seq == l.seq))
        .map(|l| l.cash_delta)
        .sum();
    assert_eq!(cash, cash0_before_entry + race_cash + others);
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
    // What this checks is the race's side of a capture: a `PursuitEnded { Captured }` for the
    // player mid-race becomes `DNF Captured` and hands off to the end-of-run panel. How and
    // when a patrol makes the arrest is wp_police's business (and keeps being retuned), so
    // the capture is forced through E's public hook instead of waiting for a unit to box the
    // player in: the player is made wanted (debug heat), then `capture_abandoned` ends the
    // pursuit as `Captured` exactly like the capture rule does (same event, same reset).
    let def = wp_race::catalog::race("lantern_checkpoint").unwrap();
    let sc = race_scenario(&def, CAR, DamageMode::Arcade, false).unwrap();
    let mut s = Session::new(sc, false).unwrap();
    // Past the countdown and a few seconds into the race (the player stays on the grid).
    s.run_ticks(240 * 6);
    assert_eq!(s.race().unwrap().info(s.tick())[0], 1.0, "race running");
    assert!(s.race_report().is_none());
    let police = s.debug_sim_mut().unwrap().police_mut();
    police.set_heat(PLAYER, 30.0);
    assert!(
        police.is_wanted_or_pursued(PLAYER),
        "debug heat makes the player wanted"
    );
    assert!(
        police.capture_abandoned(PLAYER).is_some(),
        "the capture is accepted"
    );
    // The `PursuitEnded` event is drained on the next police step; the race and the run react
    // after that tick.
    for _ in 0..240 * 2 {
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
    panic!("the capture never reached the race");
}

/// `apps/simcli/scenarios/race/pick_<id>.ron`: each picker race as a simcli v2 scenario (the
/// browser's scenario with a 10 s duration). `WP_RACE_WRITE_SCENARIOS=1` rewrites the files;
/// otherwise the files must match what the browser builds.
#[test]
fn race_scenario_files_match_the_picker() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/simcli/scenarios/race");
    for def in wp_race::catalog::races() {
        let mut sc = race_scenario(&def, CAR, DamageMode::Arcade, false).unwrap();
        sc.duration_s = 10.0;
        sc.expect = vec![
            simcli::scenario::Expect::TotalLoss {
                entity: Some("player".into()),
                expected: false,
            },
            simcli::scenario::Expect::TotalLoss {
                entity: Some(crate::race::ai_entity_id(&def.ai[0])),
                expected: false,
            },
        ];
        let path = dir.join(format!("pick_{}.ron", def.id));
        if std::env::var("WP_RACE_WRITE_SCENARIOS").is_ok() {
            let header = format!(
                "// wp_race picker race \"{}\" ({}): {}\n\
                 // Generated from wp_race::catalog by wp_wasm's race_scenario_files_match_the_picker\n\
                 // (WP_RACE_WRITE_SCENARIOS=1); the browser's race_start builds the same scenario with\n\
                 // the player's car. Entity \"player\" sits on the last grid slot, the AI field ahead.\n\
                 // Run standalone, simcli has no race block or AI driver, so the cars wait on the grid\n\
                 // for 10 s; crates/wp_race/tests/race_picker.rs races it to the finish with wp_opponent\n\
                 // AI in every seat and checks the expectation ranges.\n",
                def.name,
                def.id,
                def.blurb
            );
            std::fs::write(&path, header + &sc.to_ron() + "\n").unwrap();
        }
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let file = crate::scenario::from_ron(&text).unwrap();
        assert_eq!(
            file,
            sc,
            "{} is out of date (WP_RACE_WRITE_SCENARIOS=1)",
            path.display()
        );
    }
}

#[test]
fn map_shows_every_racer_during_a_race() {
    use crate::map::{self, h, racer_flag, HEADER, RACER_STRIDE, TOOL_STRIDE, UNIT_STRIDE};
    let mut s = sprint(true);
    s.run_ticks(240 * 8);
    let d = map::map_dynamic_for(&s, PLAYER);
    let n = d[h::N_RACERS] as usize;
    let def = wp_race::catalog::race("grid_highway_sprint").unwrap();
    assert_eq!(n, def.entrants(), "every entrant on the map");
    let o = HEADER + d[h::N_UNITS] as usize * UNIT_STRIDE + d[h::N_TOOLS] as usize * TOOL_STRIDE;
    assert_eq!(d.len(), o + n * RACER_STRIDE);
    let rows: Vec<&[f32]> = d[o..].chunks(RACER_STRIDE).collect();
    // Position order, the player flagged, poses from the sim.
    for (i, r) in rows.iter().enumerate() {
        assert_eq!(r[4] as usize, i + 1, "{rows:?}");
        let e = EntityId(r[0] as u32);
        assert_eq!(r[5] as u32 & racer_flag::PLAYER != 0, e == PLAYER);
        let p = pos(&s, e);
        assert!((r[1] - p[0]).abs() < 1e-3 && (r[2] - p[1]).abs() < 1e-3);
    }
    assert_eq!(
        rows.iter()
            .filter(|r| r[5] as u32 & racer_flag::PLAYER != 0)
            .count(),
        1
    );
}

#[test]
fn bets_on_yourself_settle_through_the_career_economy() {
    let mut s = sprint(true);
    // A fresh account has no street rep: betting is locked.
    let (max, lock) = s.bet_status();
    assert_eq!(max, 0);
    assert!(lock.is_some());
    assert!(s.race_place_bet("win:player", 200).is_err());
    s.debug_run_mut().unwrap().debug_set_rep(2_000);
    let (max, lock) = s.bet_status();
    assert!(max >= 200 && lock.is_none(), "{max} {lock:?}");
    // A racer only backs themselves.
    assert!(s.race_place_bet("win:vic", 200).is_err());
    let cash0 = s.run().unwrap().account().cash;
    let t = s.race_place_bet("h2h:player>tito", 200).unwrap();
    assert_eq!(t.outcome, "open");
    assert!(t.odds > 1.0 && t.paid > 200);
    assert_eq!(s.run().unwrap().account().cash, cash0 - 200);
    let (report, _) = run_to_final(&mut s, 150);
    // Bets close at the green light.
    assert!(s.race_place_bet("win:player", 200).is_err());
    assert_eq!(report.bets.len(), 1);
    let b = &report.bets[0];
    assert_ne!(b.outcome, "open");
    match b.outcome {
        "won" => assert_eq!(b.paid, t.paid),
        "lost" => assert_eq!(b.paid, 0),
        _ => assert_eq!(b.paid, 200),
    }
    // Every racer's form rating moved, and the regulars' records remember the race.
    assert_eq!(report.rating_changes.len(), 4);
    assert!(crate::race::with_roster(|st| st.races) >= 1);
    let saved = crate::race::roster_state_text();
    crate::race::roster_state_load(&saved).unwrap();
    assert_eq!(crate::race::roster_state_text(), saved);
}

#[test]
fn picker_cards_carry_form_and_odds() {
    let car = simcli::data::load_car(crate::data::mem_data(), CAR).unwrap();
    for c in race_cards(Some(&car), DamageMode::Arcade) {
        assert!(c
            .drivers
            .iter()
            .all(|d| d.rating > 0 && !d.nickname.is_empty()));
        // win, head-to-head per rival, podium in fields of four or more
        let want = 1 + c.drivers.len() + usize::from(c.drivers.len() + 1 >= 4);
        assert_eq!(c.odds.len(), want, "{}", c.id);
        assert!(c
            .odds
            .iter()
            .all(|o| o.odds >= 1.05 && o.label.contains("player")));
    }
    assert_eq!(crate::race::regular_cards().len(), 12);
}
