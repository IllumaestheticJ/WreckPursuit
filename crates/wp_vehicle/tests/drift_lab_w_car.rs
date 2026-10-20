//! Drift Lab W step 4: the car / tyre lab override (`wp_vehicle::car_lab`) with plain steering,
//! scored direction-aware ([`wp_vehicle::drift_lab_w::HoldMetrics`]). Own test binary: the
//! overrides are process-wide. Ranges, not exact numbers (docs/integration/drift-lab-w.md).

use std::path::PathBuf;

use wp_vehicle::car_lab::{self, CarTune};
use wp_vehicle::drift_lab_w::{load_any_car, plain_hold, reference_assists, PlainDriver};
use wp_vehicle::plain::{self, PlainSteer};

#[test]
fn held_countersteer_does_not_hold_on_data_nor_with_the_spin_grip_damper_set() {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let c = load_any_car(&data, "parity_rwd_d").unwrap();
    let a = reference_assists();
    plain::set_override(Some(PlainSteer::default()));
    let driver = PlainDriver::HoldCounter {
        entry_deg: 6.0,
        throttle: 1.0,
    };
    // Step 2: the car data spins.
    car_lab::set_override(None);
    let m = plain_hold(&c, &a, driver, 40.0, 0.6);
    assert!(m.spun && !m.held && m.drift_s < 1.0, "data: {m:?}");
    // Step 4's test set does not hold either: it swings through to the other side (or
    // spins); the intended side lasts well under half the run. (The first report of a
    // 4.7 s hold counted either side.)
    car_lab::set_override(Some(CarTune::spin_grip_damper()));
    let m = plain_hold(&c, &a, driver, 40.0, 0.6);
    assert!(!m.held && m.drift_s < 2.0, "spin grip + damper: {m:?}");
    car_lab::set_override(None);
}
