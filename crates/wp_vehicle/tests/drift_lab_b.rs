//! Drift Lab B / C (owner rows 90 / 91 / 104 / 106): fast keyboard countersteer and the
//! semi-intelligent drift assist (C: the countersteer key holds the slide, the key into the
//! turn adds angle, throttle raw by default), measured with the scripted drivers of
//! `wp_vehicle::drift_lab`. Ranges, not exact numbers (see docs/integration/drift-lab-c.md
//! for the full tables).

use wp_contracts::{AssistProfile, LoadedCar};
use wp_vehicle::drift_lab::{
    drift_run, handbrake_entry, handbrake_stab, kick_catch, transition_run, CatchDriver,
    DriftDriver,
};

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap()
}

/// Keyboard preset, TCS / ESC off, countersteer slider `help`.
fn kb(help: f32) -> AssistProfile {
    AssistProfile {
        countersteer_help: help,
        tcs: wp_contracts::TcsLevel::Off,
        esc: false,
        ..AssistProfile::keyboard_default()
    }
}

const RWD: [&str; 3] = ["shiro_turbo", "sperber_32", "rampart_50"];

#[test]
fn late_counter_key_catches_kicks_at_speed() {
    // Before Drift Lab B the rack slewed at ~100 °/s and a 150 ms-late key caught 3 of 6
    // kicks at 140 km/h in each of these cars; now ≥ 250 °/s and all 6.
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
            // Drift Lab C: the held counter key is Hold, which aims at the opposite lock that
            // keeps the slide rather than the key's full lock — less travel, same fast rate.
            assert!(r.cs_slew_dps > 250.0, "{id}: {} °/s", r.cs_slew_dps);
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
fn countersteer_hold_gives_a_long_steady_drift() {
    // 40 m left-hander: the good driver (countersteer key + modulated throttle) drifts ~5 s
    // of the 6 s without spinning; before Drift Lab B step 3 it spun within 0.8 s.
    for id in RWD {
        for help in [0.6f32, 1.0] {
            let r = drift_run(&car(id), &kb(help), DriftDriver::Good, 40.0, 0.6, 1);
            assert!(
                !r.spun && r.drift_s > 4.5 && r.sd_deg < 9.0,
                "{id} {help}: {r:?}"
            );
        }
    }
    // The shaped case lives in tests/drift_lab_b_shaped.rs (the override is process-wide).
}

#[test]
fn held_counter_key_with_full_throttle_holds_too() {
    // Owner clip 1, the primary criterion: the countersteer key + full throttle holds a
    // steady drift (2nd gear, no TCS) without spinning. (Mean from 24°: since round12's TCS
    // levels, TCS Off skips the keyboard pedal filter — row 104 — and sperber_32 holds 24.6°.)
    for id in RWD {
        for a in [kb(0.6), reference(0.6)] {
            let r = drift_run(&car(id), &a, DriftDriver::Naive, 40.0, 0.6, 1);
            assert!(
                !r.spun && r.drift_s > 4.5 && (24.0..=45.0).contains(&r.mean_deg),
                "{id}: {r:?}"
            );
        }
    }
}

#[test]
fn key_into_the_turn_adds_angle() {
    // At 100 % the into-turn driver drifts as long as the good one, at a bigger peak angle.
    for id in RWD {
        let good = drift_run(&car(id), &kb(1.0), DriftDriver::Good, 40.0, 0.6, 1);
        let into = drift_run(&car(id), &kb(1.0), DriftDriver::IntoTurn, 40.0, 0.6, 1);
        assert!(!into.spun && into.drift_s > 4.0, "{id}: {into:?}");
        assert!(
            into.max_slip_deg >= good.max_slip_deg,
            "{id}: {into:?} vs {good:?}"
        );
    }
}

#[test]
fn sloppy_driving_still_spins_sometimes() {
    // A defined, meaningful share: over 3 cars × 10 seeds at 60 %, 3 to two thirds.
    let mut spins = 0;
    for id in RWD {
        for seed in 1..=10 {
            spins +=
                drift_run(&car(id), &kb(0.6), DriftDriver::Sloppy, 40.0, 0.6, seed).spun as usize;
        }
    }
    assert!((3..=20).contains(&spins), "{spins} of 30");
}

/// The owner's reference (Wreckfest: TCS off, ESC off, manual shifting): manual gears, the
/// drift drivers shift down to 2nd.
fn reference(help: f32) -> AssistProfile {
    AssistProfile {
        auto_gears: false,
        ..kb(help)
    }
}

#[test]
fn flooring_it_at_a_big_angle_spins_lifting_saves() {
    // Reference settings (2nd gear, no TCS): flicked into a big angle and floored with the
    // key into the turn, the angle creeps past the 48° envelope and the car spins; lifting
    // saves it. (With the automatic box the upshift saves the floored car since step 4.)
    for id in RWD {
        let flood = drift_run(&car(id), &reference(0.6), DriftDriver::Flood, 40.0, 0.6, 1);
        let lift = drift_run(
            &car(id),
            &reference(0.6),
            DriftDriver::FlickLift,
            40.0,
            0.6,
            1,
        );
        assert!(flood.spun, "{id} floored: {flood:?}");
        assert!(!lift.spun, "{id} lifted: {lift:?}");
    }
}

#[test]
fn abrupt_lift_closes_the_drift() {
    // Drift Lab C: with the countersteer key held a full lift (raw, no softening) closes the
    // slide — the drift is lost early, the car is not.
    for id in RWD {
        let good = drift_run(&car(id), &kb(0.6), DriftDriver::Good, 40.0, 0.6, 1);
        let r = drift_run(&car(id), &kb(0.6), DriftDriver::AbruptLift, 40.0, 0.6, 1);
        assert!(!r.spun && r.drift_s < good.drift_s - 1.0, "{id}: {r:?}");
    }
}

#[test]
fn drift_runs_are_deterministic() {
    let c = car("rampart_50");
    let a = drift_run(&c, &kb(0.6), DriftDriver::Sloppy, 40.0, 0.6, 3);
    let b = drift_run(&c, &kb(0.6), DriftDriver::Sloppy, 40.0, 0.6, 3);
    assert_eq!(a.hash, b.hash);
}

#[test]
fn lift_off_transition_swaps_smoothly() {
    // Row 107: left drift, lift + flick, right drift. At 60 % a smooth swap: no spin, no
    // tank-slapper, the new drift within ~1 s of the lift and held after. (Before: the hold
    // kept the old drift for ~1 s and the swap mostly never happened.)
    for id in RWD {
        let r = transition_run(&car(id), &kb(0.6), 40.0, 0.6);
        assert!(!r.spun && r.wobbles == 0, "{id}: {r:?}");
        assert!(r.transition_s.is_some_and(|t| t < 1.2), "{id}: {r:?}");
        assert!(r.new_drift_s > 2.5 && r.peak_new_deg < 40.0, "{id}: {r:?}");
    }
}

#[test]
fn released_keys_catch_a_handbrake_entry() {
    // Row 108: after a handbrake tap the self-aligning catch is back within ~0.1 s (it was off
    // for 0.5 s and released keys caught 12 of 30 entries at 60 %). 80 km/h, 0.45 s tap.
    for id in RWD {
        let r = handbrake_entry(&car(id), &kb(0.6), 80.0 / 3.6, 0.45, false);
        assert!(r.caught, "{id}: {r:?}");
        let t = r.release_to_aligned_s.expect("aligned");
        assert!(t < 0.2, "{id}: aligned {t} s after the release");
        let c = handbrake_entry(&car(id), &kb(0.6), 80.0 / 3.6, 0.45, true);
        assert!(c.caught, "{id} counter key: {c:?}");
    }
}

#[test]
fn handbrake_stabs_add_rotation_and_drive_comes_back() {
    // Owner clip 5 (reference settings, 2nd gear): three 0.2 s stabs on the throttle add
    // rotation, drive is back within 0.1 s of each, and the power drift goes on.
    for id in RWD {
        let r = handbrake_stab(&car(id), &reference(0.6), 84.0, 0.6);
        assert!(!r.spun && r.slide_after_deg > 25.0, "{id}: {r:?}");
        assert!(r.drive_back_s < 0.1 && r.drift_s > 2.0, "{id}: {r:?}");
    }
}
