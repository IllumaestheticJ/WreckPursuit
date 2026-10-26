//! Crash replay (lead): a real crash is detected, its replay re-simulates the buffered
//! snapshot with the logged inputs and matches every live tick hash, and the live session is
//! unchanged by snapshots and replays.

use wp_contracts::{DamageMode, EntityId, VehicleInput, VehicleSim};

use crate::crash_replay::{
    CrashMark, BUFFER_TICKS, POST_IMPACT_TICKS, PRE_IMPACT_TICKS, SNAPSHOT_EVERY_TICKS,
};
use crate::scenario::{from_ron, night_run};
use crate::session::Session;

fn scenario_file(rel: &str) -> simcli::Scenario {
    let path = format!(
        "{}/../../apps/simcli/scenarios/{rel}",
        env!("CARGO_MANIFEST_DIR")
    );
    from_ron(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// Step `s` in 60 Hz frames until it reports a crash (or `max_frames` pass).
fn until_crash(s: &mut Session, max_frames: usize) -> Option<CrashMark> {
    for _ in 0..max_frames {
        s.step(1.0 / 60.0);
        if let Some(m) = s.crash_take() {
            return Some(m);
        }
    }
    None
}

#[test]
fn barrier_crash_is_detected_and_replays_exactly() {
    let sc = scenario_file("crash/barrier_frontal.ron");
    let mut live = Session::new(sc.clone(), true).unwrap();
    let mut twin = Session::new(sc, true).unwrap();
    twin.crash_mut().set_enabled(false);
    let crash = until_crash(&mut live, 7 * 60).expect("56 km/h into a barrier is a big crash");
    assert_eq!(crash.other, None, "hit the static barrier");
    assert_eq!(crash.side, crate::crash_replay::ImpactSide::Front);
    assert!(crash.closing_mps > 10.0, "{crash:?}");
    // The offer shows ~0.5 s later; accept then.
    live.run_ticks(120);
    let (tick, hash) = (live.tick(), live.sim().state_hash());

    let (mut replay, info) = live.crash_replay(&crash).unwrap();
    assert_eq!(info.start_tick, crash.tick - PRE_IMPACT_TICKS);
    assert_eq!(info.end_tick, crash.tick + POST_IMPACT_TICKS);
    assert_eq!(replay.tick(), info.start_tick);
    replay.replay_play(0.25).unwrap();
    while replay.tick() < info.end_tick {
        replay.step(1.0 / 60.0);
    }
    let st = replay.replay_status().unwrap();
    assert_eq!(st.drift_count, 0, "replay drifted at {:?}", st.drift_ticks);
    // Every tick up to the live tick was compared (the rest is extrapolated).
    assert_eq!(
        st.verified_ticks,
        tick.min(info.end_tick) - snapshot_floor(info.start_tick)
    );
    assert!(
        replay.crash_take().is_none(),
        "replays do not detect crashes"
    );

    // The live session is exactly where it was, and goes on like a session that never
    // snapshotted or replayed.
    assert_eq!((live.tick(), live.sim().state_hash()), (tick, hash));
    twin.run_ticks(tick - twin.tick());
    assert_eq!(twin.sim().state_hash(), hash);
    live.run_ticks(240);
    twin.run_ticks(240);
    assert_eq!(live.sim().state_hash(), twin.sim().state_hash());
}

/// The snapshot a replay starting at `start` re-simulates from (1 s grid).
fn snapshot_floor(start: u64) -> u64 {
    start / SNAPSHOT_EVERY_TICKS * SNAPSHOT_EVERY_TICKS
}

#[test]
fn buffer_keeps_ten_seconds_of_snapshots() {
    let mut s = Session::new(scenario_file("crash/barrier_frontal.ron"), true).unwrap();
    s.run_ticks(15 * 240);
    let ticks = s.crash().snapshot_ticks();
    let now = s.tick();
    assert!(
        ticks[0] + BUFFER_TICKS <= now && ticks[0] + BUFFER_TICKS + SNAPSHOT_EVERY_TICKS > now,
        "{ticks:?} at {now}"
    );
    assert!(ticks.len() <= (BUFFER_TICKS / SNAPSHOT_EVERY_TICKS) as usize + 1);
}

#[test]
fn night_run_crash_into_parked_car_replays_without_drift() {
    // Traffic, police, props and weather all come back from the snapshot.
    let mut live = Session::new(night_run("debug_sedan", DamageMode::Arcade), false).unwrap();
    let p = EntityId(1);
    live.run_ticks(300);
    live.debug_knock_prop(p, None, 14.0).unwrap();
    live.set_input(
        p,
        VehicleInput {
            throttle: 0.6,
            ..Default::default()
        },
    )
    .unwrap();
    let crash = until_crash(&mut live, 240).expect("14 m/s into a parked car is a big crash");
    live.run_ticks(60);
    let (tick, hash) = (live.tick(), live.sim().state_hash());
    let t0 = std::time::Instant::now();
    let (mut replay, info) = live.crash_replay(&crash).unwrap();
    let build = t0.elapsed();
    replay.replay_play(1.0).unwrap();
    replay.run_ticks(info.end_tick - replay.tick());
    let st = replay.replay_status().unwrap();
    assert_eq!(st.drift_count, 0, "replay drifted at {:?}", st.drift_ticks);
    assert!(st.verified_ticks >= tick - info.start_tick, "{st:?}");
    assert_eq!((live.tick(), live.sim().state_hash()), (tick, hash));
    // Snapshot cost (debug build here; release numbers are in docs/integration/crash-replay.md).
    let t0 = std::time::Instant::now();
    let _copy = live.sim().clone();
    eprintln!(
        "night run: sim clone {:?}, replay build {build:?}",
        t0.elapsed()
    );
}

#[test]
fn race_crash_replays_with_the_judge_and_no_crash_on_the_grid() {
    let def = wp_race::catalog::race("grid_highway_sprint").unwrap();
    let sc = crate::race::race_scenario(&def, "lindwurm_4t", DamageMode::Arcade, false).unwrap();
    let mut live = Session::new(sc, false).unwrap();
    let p = EntityId(1);
    let full = VehicleInput {
        throttle: 1.0,
        ..Default::default()
    };
    // Countdown, then 4 s flat out.
    for _ in 0..(240 * 10) / 60 {
        live.set_input(p, full).unwrap();
        live.run_ticks(60);
        assert!(
            live.crash_take().is_none(),
            "no crash on the grid or the straight"
        );
    }
    // Park the AI racer nearest the player: since the round-13 ride / steering changes the
    // player's no-steer launch ends up nose to tail with one AI racer on the straight, and
    // parking another racer ahead left that car between them (a 0 m/s shunt, no crash).
    let pos = |e: EntityId| live.vehicle(e).unwrap().vehicle.state().pos;
    let me = pos(p);
    let dist = |e: &EntityId| {
        let q = pos(*e);
        (q[0] - me[0]).hypot(q[1] - me[1])
    };
    let ai = *live.entities()[1..]
        .iter()
        .min_by(|a, b| dist(a).total_cmp(&dist(b)))
        .unwrap();
    live.debug_park_in_front_of(ai, p, 18.0).unwrap();
    let crash = until_crash(&mut live, 240).expect("rear-ending a stopped racer is a big crash");
    // Usually the parked racer; a bolder racer may tuck in right ahead of the player first
    // (bolder-racer change, docs/integration/opponent.md). Either way an AI racer.
    assert!(
        live.entities()[1..]
            .iter()
            .any(|e| crash.other == Some(e.0)),
        "{:?} vs parked {:?}",
        crash.other,
        ai
    );
    live.run_ticks(120);
    let (tick, hash) = (live.tick(), live.sim().state_hash());
    let (mut replay, info) = live.crash_replay(&crash).unwrap();
    assert!(replay.race().is_some(), "the replay judges the race too");
    replay.run_ticks(info.end_tick - replay.tick());
    let st = replay.replay_status().unwrap();
    assert_eq!(st.drift_count, 0, "replay drifted at {:?}", st.drift_ticks);
    assert_eq!((live.tick(), live.sim().state_hash()), (tick, hash));
}
