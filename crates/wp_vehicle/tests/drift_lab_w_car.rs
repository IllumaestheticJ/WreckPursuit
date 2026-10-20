//! Drift Lab W step 4: the car / tyre lab override (`wp_vehicle::car_lab`) with plain steering.
//! Own test binary: the overrides are process-wide. Ranges, not exact numbers
//! (docs/integration/drift-lab-w.md).

use std::path::PathBuf;

use wp_vehicle::car_lab::{self, CarTune};
use wp_vehicle::drift_lab_w::{load_any_car, plain_drift_run, reference_assists, PlainDriver};
use wp_vehicle::plain::{self, PlainSteer};

#[test]
fn held_countersteer_spins_on_data_and_holds_with_the_drift_candidate() {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let c = load_any_car(&data, "parity_rwd_d").unwrap();
    let a = reference_assists();
    plain::set_override(Some(PlainSteer::default()));
    let driver = PlainDriver::HoldCounter {
        entry_deg: 6.0,
        throttle: 1.0,
    };
    // The step-2 finding: the car data cannot hold clip 1's drift.
    car_lab::set_override(None);
    let (r, _) = plain_drift_run(&c, &a, driver, 40.0, 0.6);
    assert!(r.spun && r.drift_s < 1.0, "data: {r:?}");
    // The step-4 candidate holds it (with the diagnostic yaw damper).
    car_lab::set_override(Some(CarTune::drift_candidate()));
    let (r, _) = plain_drift_run(&c, &a, driver, 40.0, 0.6);
    assert!(!r.spun && r.drift_s > 3.5, "candidate: {r:?}");
    assert!((15.0..40.0).contains(&r.mean_deg), "candidate: {r:?}");
    car_lab::set_override(None);
}
