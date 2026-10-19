//! Native tests of the session / replay core behind the wasm surface.

use wp_contracts::{EntityId, VehicleInput};

use crate::replay::Replay;
use crate::scenario::Scenario;
use crate::session::Session;

fn drive(throttle: f32, steer: f32) -> VehicleInput {
    VehicleInput {
        throttle,
        steer,
        ..Default::default()
    }
}

/// A live session with a scripted-by-hand input sequence, stepped with irregular frame times.
fn live_session(frame_dts: &[f32]) -> Session {
    let mut s = Session::from_scenario(7, Scenario::default()).unwrap();
    let p = EntityId(1);
    let second = s.spawn("debug_coupe", [6.0, 0.0, 0.0], 0.3).unwrap();
    for (i, dt) in frame_dts.iter().enumerate() {
        let steer = if (i / 20) % 2 == 0 { 0.4 } else { -0.3 };
        s.set_input(p, drive(1.0, steer)).unwrap();
        s.set_input(second, drive(0.6, -steer)).unwrap();
        if i == 50 {
            s.set_input(
                p,
                VehicleInput {
                    gear_up: true,
                    ..drive(1.0, steer)
                },
            )
            .unwrap();
        }
        if i == 90 {
            s.reset_vehicle(second).unwrap();
        }
        s.step(*dt);
    }
    s
}

fn frames(n: usize, dt: f32) -> Vec<f32> {
    vec![dt; n]
}

#[test]
fn free_drive_moves_the_player() {
    let mut s = Session::from_scenario(1, Scenario::default()).unwrap();
    s.set_input(EntityId(1), drive(1.0, 0.0)).unwrap();
    for _ in 0..60 {
        s.step(1.0 / 60.0);
    }
    assert_eq!(s.tick(), 240);
    let (_, st) = &s.sim().vehicle_states()[0];
    assert!(st.speed_mps > 1.0);
}

#[test]
fn replay_round_trip_matches_every_hash() {
    let mut live = live_session(&frames(150, 1.0 / 60.0));
    let live_hash = live.sim().state_hash();
    let bytes = live.record_stop().to_bytes();

    let replay = Replay::from_bytes(&bytes).unwrap();
    let end = replay.end_tick;
    assert_eq!(replay.final_hash(), Some(live_hash));
    let mut play = Session::from_replay(replay).unwrap();
    play.replay_seek(end).unwrap();
    let st = play.replay_status().unwrap();
    assert!(st.drift_ticks.is_empty(), "drift at {:?}", st.drift_ticks);
    assert_eq!(st.verified_hashes, st.stored_hashes);
    assert_eq!(play.sim().state_hash(), live_hash);
}

#[test]
fn replay_plays_back_through_step_at_any_speed() {
    let mut live = live_session(&frames(120, 1.0 / 60.0));
    let replay = live.record_stop();
    let end = replay.end_tick;
    let live_hash = replay.final_hash().unwrap();
    let mut play = Session::from_replay(replay).unwrap();
    play.replay_play(3.0).unwrap();
    for _ in 0..200 {
        play.step(1.0 / 60.0);
    }
    assert_eq!(play.tick(), end);
    assert_eq!(play.sim().state_hash(), live_hash);
    assert!(play.replay_status().unwrap().drift_ticks.is_empty());
}

#[test]
fn live_sessions_are_deterministic() {
    let a = live_session(&frames(100, 1.0 / 60.0)).sim().state_hash();
    let b = live_session(&frames(100, 1.0 / 60.0)).sim().state_hash();
    assert_eq!(a, b);
}

#[test]
fn backward_seek_resimulates_identically() {
    let mut live = live_session(&frames(120, 1.0 / 60.0));
    let replay = live.record_stop();
    let mut play = Session::from_replay(replay).unwrap();
    play.replay_seek(300).unwrap();
    let h300 = play.sim().state_hash();
    play.replay_seek(400).unwrap();
    play.replay_seek(300).unwrap();
    assert_eq!(play.sim().state_hash(), h300);
    assert_eq!(play.tick(), 300);
}

#[test]
fn record_start_sets_view_start() {
    let mut s = live_session(&frames(30, 1.0 / 60.0));
    let t0 = s.tick();
    s.record_start();
    for _ in 0..30 {
        s.step(1.0 / 60.0);
    }
    let r = s.record_stop();
    assert_eq!(r.view_start_tick, t0);
    let play = Session::from_replay(r).unwrap();
    assert_eq!(play.tick(), t0);
}

#[test]
fn live_control_is_refused_during_playback() {
    let mut live = live_session(&frames(10, 1.0 / 60.0));
    let mut play = Session::from_replay(live.record_stop()).unwrap();
    assert!(play.set_input(EntityId(1), drive(1.0, 0.0)).is_err());
}

#[test]
fn gear_up_is_a_one_tick_edge() {
    let mut s = Session::from_scenario(1, Scenario::default()).unwrap();
    s.set_input(
        EntityId(1),
        VehicleInput {
            gear_up: true,
            ..Default::default()
        },
    )
    .unwrap();
    s.run_ticks(1);
    assert!(!s.input(EntityId(1)).gear_up);
}

#[test]
fn barrier_and_head_on_scenarios_run_and_report() {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/testbed-web/scenarios/crash"
    );
    for name in ["barrier_frontal.ron", "head_on.ron"] {
        let text = std::fs::read_to_string(format!("{dir}/{name}")).unwrap();
        let sc = Scenario::from_ron(&text).unwrap();
        let end = (sc.duration_s * 240.0).round() as u64;
        let mut s = Session::from_scenario(0, sc).unwrap();
        // Scripted vehicles ignore live input while the scenario runs.
        assert!(!s.set_input(EntityId(1), drive(0.0, 1.0)).unwrap());
        s.run_ticks(end);
        let r = s.scenario_result().unwrap();
        assert!(r.done, "{name}");
        assert!(
            r.checks.iter().all(|c| c.pass || c.pending),
            "{name}: {:?}",
            r.checks
        );
        assert!(r.pass);
        // Vehicles approach at the cruise speed.
        assert!(r.metrics[0].max_speed_mps > 10.0, "{name}: {:?}", r.metrics);
    }
}

#[test]
fn csv_export_has_one_row_per_sample() {
    let mut live = live_session(&frames(60, 1.0 / 60.0));
    let replay = live.record_stop();
    let end = replay.end_tick;
    let csv = crate::export_csv(replay, 1, 4).unwrap();
    let rows = csv.lines().count() - 1;
    assert_eq!(rows as u64, end / 4 + 1);
    assert!(csv.starts_with("tick,t_s,speed_mps"));
}
