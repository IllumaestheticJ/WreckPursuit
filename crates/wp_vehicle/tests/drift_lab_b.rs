//! Drift Lab B (owner rows 90 / 91): fast keyboard countersteer and the semi-intelligent drift
//! assist, measured with the scripted drivers of `wp_vehicle::drift_lab`. Ranges, not exact
//! numbers (see docs/integration/drift-lab-b.md for the full tables).

use wp_contracts::{AssistProfile, LoadedCar};
use wp_vehicle::drift_lab::{drift_run, kick_catch, CatchDriver, DriftDriver};

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap()
}

/// Keyboard preset, TCS / ESC off, countersteer slider `help`.
fn kb(help: f32) -> AssistProfile {
    AssistProfile {
        countersteer_help: help,
        tcs: false,
        esc: false,
        ..AssistProfile::keyboard_default()
    }
}

const RWD: [&str; 3] = ["shiro_turbo", "sperber_32", "rampart_50"];

#[test]
fn late_counter_key_catches_kicks_at_speed() {
    // Before Drift Lab B the rack slewed at ~100 °/s and a 150 ms-late key caught 3 of 6
    // kicks at 140 km/h in each of these cars; now ≥ 350 °/s and all 6.
    for id in RWD {
        let c = car(id);
        for kick in [1.2f32, 2.0, -2.0] {
            let r = kick_catch(
                &c,
                &kb(0.6),
                CatchDriver::Counter { react_s: 0.15 },
                140.0 / 3.6,
                kick,
            );
            assert!(r.caught, "{id} kick {kick}: {r:?}");
            assert!(r.cs_slew_dps > 300.0, "{id}: {} °/s", r.cs_slew_dps);
        }
    }
}

#[test]
fn released_keys_straighten_a_kick() {
    for id in RWD {
        let r = kick_catch(&car(id), &kb(0.6), CatchDriver::Released, 100.0 / 3.6, 2.0);
        assert!(r.caught, "{id}: {r:?}");
        let t = r.onset_to_aligned_s.expect("aligned");
        assert!(t < 0.8, "{id}: aligned after {t} s");
    }
}

#[test]
fn hold_and_modulate_gives_a_long_steady_drift() {
    // 40 m left-hander: the good driver drifts most of the 6 s without spinning (at 60 % and
    // 100 %); before step 3 it spun within 0.8 s.
    for id in RWD {
        for help in [0.6f32, 1.0] {
            let r = drift_run(&car(id), &kb(help), DriftDriver::Good, 40.0, 0.6, 1);
            assert!(!r.spun && r.drift_s > 3.0, "{id} {help}: {r:?}");
        }
    }
}

#[test]
fn sloppy_driving_still_spins_sometimes() {
    // A defined, non-zero share: over 3 cars × 10 seeds at 60 %, between 1 and half.
    let mut spins = 0;
    for id in RWD {
        for seed in 1..=10 {
            spins +=
                drift_run(&car(id), &kb(0.6), DriftDriver::Sloppy, 40.0, 0.6, seed).spun as usize;
        }
    }
    assert!((1..=15).contains(&spins), "{spins} of 30");
}

#[test]
fn flooring_it_at_a_big_angle_spins_lifting_saves() {
    for id in RWD {
        let flood = drift_run(&car(id), &kb(0.6), DriftDriver::Flood, 40.0, 0.6, 1);
        let lift = drift_run(&car(id), &kb(0.6), DriftDriver::FlickLift, 40.0, 0.6, 1);
        assert!(flood.spun, "{id} floored: {flood:?}");
        assert!(!lift.spun, "{id} lifted: {lift:?}");
    }
}

#[test]
fn abrupt_lift_snaps_but_is_softened() {
    // The tail swings past straight (a snap) but the car is not lost. shiro_turbo lifts at
    // the 2nd-gear limiter and the automatic box upshifts on the lift (little engine braking):
    // it straightens without a snap, so 2 of the 3 cars must snap.
    let mut snaps = 0;
    for id in RWD {
        let r = drift_run(&car(id), &kb(0.6), DriftDriver::AbruptLift, 40.0, 0.6, 1);
        assert!(!r.spun, "{id}: {r:?}");
        snaps += (r.flip_deg > 5.0) as usize;
    }
    assert!(snaps >= 2, "{snaps} of 3 snapped");
}

#[test]
fn drift_runs_are_deterministic() {
    let c = car("rampart_50");
    let a = drift_run(&c, &kb(0.6), DriftDriver::Sloppy, 40.0, 0.6, 3);
    let b = drift_run(&c, &kb(0.6), DriftDriver::Sloppy, 40.0, 0.6, 3);
    assert_eq!(a.hash, b.hash);
}
