//! Tyre-slip workstream: the spin-weighted combined-slip model (`wp_vehicle::tire::SpinGrip`,
//! `CarTune::spin_model`) under Drift Lab W's plain steering. Own test binary: the overrides
//! are process-wide. Ranges, not exact numbers (docs/integration/tyre-slip.md).

use std::path::PathBuf;

use wp_contracts::LoadedCar;
use wp_vehicle::car_lab::{self, CarTune};
use wp_vehicle::drift_lab::slide_state;
use wp_vehicle::drift_lab_w::{load_any_car, plain_drift_run_with, reference_assists, PlainDriver};
use wp_vehicle::plain::{self, PlainSteer};
use wp_vehicle::ride_lab::{self, RideTune};

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    load_any_car(&data, id).unwrap()
}

/// Held countersteer (key at 6° + full throttle, 0.6 g entry): (seconds on the intended side
/// in 30–45°, mean / sd of the slide over the last 3 s, spun, state hash).
fn hold(c: &LoadedCar) -> (f32, f32, f32, bool, u64) {
    let a = reference_assists();
    let mut angles = Vec::new();
    let (r, _) = plain_drift_run_with(
        c,
        &a,
        PlainDriver::HoldCounter {
            entry_deg: 6.0,
            throttle: 1.0,
        },
        40.0,
        0.6,
        |rig, _, _| angles.push((-slide_state(rig).body_slip).to_degrees()),
    );
    let band = angles
        .iter()
        .take_while(|a| a.abs() < 90.0)
        .filter(|a| (30.0..=45.0).contains(*a))
        .count() as f32
        / 240.0;
    let late = &angles[angles.len() - 3 * 240..];
    let m = late.iter().sum::<f32>() / late.len() as f32;
    let sd = (late.iter().map(|a| (a - m) * (a - m)).sum::<f32>() / late.len() as f32).sqrt();
    (band, m, sd, r.spun, r.hash)
}

#[test]
fn spin_model_holds_the_clip_1_drift_where_the_ellipse_spins() {
    let c = car("parity_rwd_d");
    plain::set_override(Some(PlainSteer::default()));
    ride_lab::set_override(Some(RideTune::lab_default()));

    car_lab::set_override(None);
    let (band, _, _, spun, hash_none) = hold(&c);
    assert!(spun && band < 1.0, "data: band {band} spun {spun}");
    // The default override is the data, bit for bit.
    car_lab::set_override(Some(CarTune::default()));
    assert_eq!(
        hold(&c).4,
        hash_none,
        "CarTune::default must not change the step"
    );

    // Spin model, no yaw damper: a steady hold inside clip 1's 30–45° (tuned set; it is a
    // knife edge, see docs/integration/tyre-slip.md — a failure here after other physics
    // changes means re-tuning, not necessarily a bug).
    car_lab::set_override(Some(CarTune::spin_candidate()));
    let (band, mean, sd, spun, _) = hold(&c);
    assert!(!spun, "spin model spun");
    assert!(band >= 4.0, "spin model: {band} s in 30–45°");
    assert!(
        (30.0..=46.0).contains(&mean) && sd <= 6.0,
        "spin model: last 3 s {mean}° sd {sd}"
    );
    car_lab::set_override(None);
    ride_lab::set_override(None);
    plain::set_override(None);
}
