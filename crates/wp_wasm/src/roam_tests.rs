//! Native tests of free roam inside a live session (`crate::roam`).

use wp_contracts::{DamageMode, VehicleSim};
use wp_opponent::RoamPhase;

use crate::roam::{free_roam_in, haunt_of_id, is_roamer, FREE_ROAM_NAME};
use crate::session::Session;

const CAR: &str = "lindwurm_4t";

#[test]
fn free_roam_scenario_puts_roamers_on_the_roads() {
    let sc = free_roam_in(CAR, DamageMode::Arcade, "vesper_v2", 8).unwrap();
    assert_eq!(sc.name, FREE_ROAM_NAME);
    assert_eq!(sc.entities[0].id, "player");
    let roamers: Vec<_> = sc.entities.iter().filter(|e| is_roamer(&e.id)).collect();
    assert_eq!(roamers.len(), 8);
    for e in &roamers {
        assert!(haunt_of_id(&e.id).is_some(), "{}", e.id);
    }
    // Same scenario every time.
    let again = free_roam_in(CAR, DamageMode::Arcade, "vesper_v2", 8).unwrap();
    assert_eq!(sc.to_ron(), again.to_ron());
}

#[test]
fn roamers_drive_off_and_the_recording_replays_without_drift() {
    let sc = free_roam_in(CAR, DamageMode::Arcade, "vesper_v2", 6).unwrap();
    let mut s = Session::new(sc, false).unwrap();
    let roam = s.roam().expect("roamers attached");
    let ids = roam.roamers().entities();
    let start: Vec<[f32; 3]> = ids
        .iter()
        .map(|id| s.vehicle(*id).unwrap().vehicle.state().pos)
        .collect();
    s.run_ticks(240 * 40);
    let roam = s.roam().unwrap();
    let status = roam.status();
    assert_eq!(status.roamers.len(), 6);
    let mut moved = 0;
    for (k, id) in ids.iter().enumerate() {
        let p = s.vehicle(*id).unwrap().vehicle.state().pos;
        let d = ((p[0] - start[k][0]).powi(2) + (p[1] - start[k][1]).powi(2)).sqrt();
        assert!(
            roam.roamers().legs_of(k) >= 1,
            "roamer {k} never planned a leg"
        );
        assert_ne!(roam.roamers().phase_of(k), RoamPhase::Idle, "roamer {k}");
        if d > 100.0 {
            moved += 1;
        }
    }
    assert!(moved >= 5, "{moved}/6 roamers went somewhere: {status:?}");
    // Playback rebuilds the roamers' setup and drives them from the log.
    let replay = s.record_stop().unwrap();
    let mut p = Session::from_replay(replay.clone()).unwrap();
    p.run_ticks(replay.ticks);
    let st = p.replay_status().unwrap();
    assert_eq!(st.drift_count, 0, "{st:?}");
}
