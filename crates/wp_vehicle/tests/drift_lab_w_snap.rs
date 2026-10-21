//! Drift Lab W step 10 — snap-back (lead): leaving a held 30–45° drift by countersteering out
//! (counter key held, throttle lifted). "Before" (no countersteer limit, step-3 lab ride) the
//! counter-locked fronts end up pointing past the travel as the slide closes and yank the car
//! the other way (84° to the other side, spin); "new" (countersteer limit 0.25°, medium
//! weight) recovers progressively and keeps the hold. Own test binary: the overrides are
//! process-wide. Ranges, not exact numbers (docs/integration/drift-lab-w.md, step 10).

use std::path::PathBuf;

use wp_contracts::LoadedCar;
use wp_vehicle::car_lab::{self, CarTune};
use wp_vehicle::drift_lab_w::{
    load_any_car, plain_hold, reference_assists, snap_back, PlainDriver, SnapExit,
};
use wp_vehicle::plain::{self, PlainSteer};
use wp_vehicle::ride_lab::{self, RideTune};

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    load_any_car(&data, id).unwrap()
}

#[test]
fn countersteer_limit_stops_the_snap_back_and_keeps_the_hold() {
    let a = reference_assists();
    car_lab::set_override(Some(CarTune::spin_candidate()));
    // Before: steps 1–9.
    plain::set_override(Some(PlainSteer {
        counter_margin_deg: 0.0,
        ..PlainSteer::default()
    }));
    ride_lab::set_override(Some(RideTune::lab_default()));
    let mut before = Vec::new();
    for id in ["parity_rwd_d", "parity_rwd_b"] {
        let c = car(id);
        let b = snap_back(&c, &a, SnapExit::CounterLift);
        assert!(b.reached, "{id}: no hold before");
        assert!(
            b.spun || b.slip_overshoot_deg > 60.0,
            "{id} before: {b:?} (expected the snap-back)"
        );
        before.push(b);
    }
    // New: the W default.
    plain::set_override(Some(PlainSteer::default()));
    ride_lab::set_override(Some(RideTune::medium_weight()));
    for (id, b0) in ["parity_rwd_d", "parity_rwd_b"].iter().zip(&before) {
        let c = car(id);
        let b = snap_back(&c, &a, SnapExit::CounterLift);
        assert!(b.reached, "{id}: no hold");
        assert!(
            !b.spun && b.slip_overshoot_deg < 25.0,
            "{id} new: {b:?} (before {b0:?})"
        );
        assert!(
            b.yaw_overshoot < 0.85 * b0.yaw_overshoot,
            "{id}: yaw overshoot {} vs before {}",
            b.yaw_overshoot,
            b0.yaw_overshoot
        );
        // The owner's hold (counter key + full throttle) still holds.
        let h = plain_hold(
            &c,
            &a,
            PlainDriver::HoldCounter {
                entry_deg: 6.0,
                throttle: 1.0,
            },
            40.0,
            0.6,
        );
        assert!(h.held && h.band_s >= 2.0, "{id} hold: {h:?}");
    }
    car_lab::set_override(None);
    plain::set_override(None);
    ride_lab::set_override(None);
}
