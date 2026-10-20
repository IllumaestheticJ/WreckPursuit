//! Drift Lab B with the opt-in throttle shaping on. Its own test binary: the tuning override
//! is process-wide, so it must not run next to the default-tuning tests in
//! `tests/drift_lab_b.rs`.

use wp_contracts::AssistProfile;
use wp_vehicle::drift::{set_tuning_override, DriftTuning};
use wp_vehicle::drift_lab::{drift_run, DriftDriver};

#[test]
fn shaping_holds_the_powerful_car_and_flooring_still_spins() {
    let p = format!(
        "{}/../../data/cars/rampart_50.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let car = wp_contracts::load_car(p).unwrap();
    let kb = AssistProfile {
        countersteer_help: 0.6,
        tcs: wp_contracts::TcsLevel::Off,
        esc: false,
        ..AssistProfile::keyboard_default()
    };
    set_tuning_override(Some(DriftTuning {
        throttle_shape: 0.7,
        ..DriftTuning::default()
    }));
    let good = drift_run(&car, &kb, DriftDriver::Good, 40.0, 0.6, 1);
    let flood = drift_run(&car, &kb, DriftDriver::Flood, 40.0, 0.6, 1);
    set_tuning_override(None);
    assert!(!good.spun && good.drift_s > 3.0, "{good:?}");
    assert!(flood.spun, "{flood:?}");
}
