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

/// The lab overrides are process-wide: tests in this binary take this lock.
static LAB: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    // Written against the pre-unify stock path: `None` overrides = the car data
    // (`wp_vehicle::lab_scope`; process-wide, every test in this binary sets it).
    wp_vehicle::lab_scope::set_legacy(true);
    let _lab = LAB.lock().unwrap_or_else(|e| e.into_inner());
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
    // The step-22 lab the panel ships: firm ride + grip-balance "new" (since the unify the
    // main-game default), where the spin model is tuned (candidate B).
    ride_lab::set_override(Some(RideTune::firm()));
    wp_vehicle::grip_lab::set_override(Some(wp_vehicle::grip_lab::GripBalance::lab_default()));
    car_lab::set_override(Some(CarTune::spin_candidate()));
    let (band, mean, sd, spun, _) = hold(&c);
    wp_vehicle::grip_lab::set_override(None);
    assert!(!spun, "spin model spun");
    // Candidate B on the step-22 lab: ~5.0 s in the band at ~40° (sd ~1.6°); the step-20 set
    // sat at ~44°, right under the 45° edge.
    assert!(band >= 4.0, "spin model: {band} s in 30–45°");
    assert!(
        (30.0..=46.0).contains(&mean) && sd <= 6.0,
        "spin model: last 3 s {mean}° sd {sd}"
    );
    car_lab::set_override(None);
    ride_lab::set_override(None);
    plain::set_override(None);
}

/// Rest → full throttle for 3 s, steer 0, TCS off, on a road cross-slope of `slope_deg`
/// (sideways gravity g·sin θ added each tick): largest heading change, degrees.
fn launch_yaw(c: &LoadedCar, slope_deg: f32) -> f32 {
    use wp_contracts::VehicleInput;
    use wp_vehicle::math::{atan2, sin, V3};
    use wp_vehicle::rig::Rig;
    let a = reference_assists();
    let mut rig = Rig::new(c);
    rig.teleport([0.0; 3], 0.0, 0.0, 0.0);
    for _ in 0..120 {
        rig.step(&VehicleInput::default(), &a);
    }
    let heading = |rig: &Rig| {
        let f = rig.vehicle.chassis().rot * V3::Y;
        atan2(-f.x, f.y).to_degrees()
    };
    let h0 = heading(&rig);
    let side_dv = 9.81 * sin(slope_deg.to_radians()) / 240.0;
    let mut yaw = 0.0f32;
    for _ in 0..3 * 240 {
        rig.kick([side_dv, 0.0, 0.0], [0.0; 3]);
        rig.step(
            &VehicleInput {
                throttle: 1.0,
                ..Default::default()
            },
            &a,
        );
        yaw = yaw.max((heading(&rig) - h0).abs());
    }
    yaw
}

#[test]
fn launches_straight_on_a_cambered_road() {
    // Written against the pre-unify stock path: `None` overrides = the car data
    // (`wp_vehicle::lab_scope`; process-wide, every test in this binary sets it).
    wp_vehicle::lab_scope::set_legacy(true);
    // Owner (Drift Lab W): "spins off the line, TCS off, no steering"; the reference smokes
    // the rears dead straight. With the old friction ellipse (side floor 0) a rear spinning at
    // κ ≈ 6–8 has almost no side force, so a 2° road slope swings the tail out. The shipped
    // side-force floor (no override) and both spin presets launch straight.
    let _lab = LAB.lock().unwrap_or_else(|e| e.into_inner());
    let c = car("parity_rwd_d");
    plain::set_override(Some(PlainSteer::default()));
    ride_lab::set_override(None);
    car_lab::set_override(Some(CarTune {
        side_floor: 0.0,
        ..CarTune::default()
    }));
    let old = launch_yaw(&c, 2.0);
    assert!(
        old > 30.0,
        "old ellipse on a 2° slope: {old}° (expected to spin)"
    );
    car_lab::set_override(None);
    let shipped = launch_yaw(&c, 2.0);
    assert!(shipped < 3.0, "shipped floor on a 2° slope: {shipped}°");
    for t in [CarTune::spin_candidate(), CarTune::spin_lock_candidate()] {
        car_lab::set_override(Some(t));
        let y = launch_yaw(&c, 2.0);
        assert!(y < 3.0, "spin preset on a 2° slope: {y}°");
        assert!(launch_yaw(&c, 0.0) < 0.1);
    }
    car_lab::set_override(None);
    plain::set_override(None);
}
