//! Drift Lab W step 10 — snap-back (lead): leaving a held 30–45° drift by countersteering out
//! (counter key held, throttle lifted), on the lab's default tyres (spin model + grip-balance
//! "new"). "Before" (no countersteer limit, step-3 lab ride) the counter-locked fronts end up
//! pointing past the travel as the slide closes and yank the car the other way (spin); "new"
//! (countersteer limit 0.25°, medium weight) does not spin, with less yaw acceleration and
//! overshoot, and the key-tapping hold stays. Own test binary: the overrides are process-wide.
//! Ranges, not exact numbers (docs/integration/drift-lab-w.md, step 10).

use std::path::PathBuf;

use wp_contracts::LoadedCar;
use wp_vehicle::car_lab::{self, CarTune};
use wp_vehicle::drift_lab_w::{
    load_any_car, plain_hold, reference_assists, snap_back, PlainDriver, SnapExit,
};
use wp_vehicle::grip_lab::{self, GripBalance};
use wp_vehicle::plain::{self, PlainSteer};
use wp_vehicle::ride_lab::{self, RideTune};

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    load_any_car(&data, id).unwrap()
}

#[test]
fn countersteer_limit_stops_the_snap_back_spin_and_keeps_the_hold() {
    let a = reference_assists();
    car_lab::set_override(Some(CarTune::spin_candidate()));
    // Steps 1–9 had no held-key slide lock (grip-balance added it in "new" later): "before"
    // pins it off, "new" below uses the current default.
    grip_lab::set_override(Some(GripBalance {
        slide_lock: 1.0,
        ..GripBalance::lab_default()
    }));
    let ids = ["parity_rwd_d", "parity_rwd_a"];
    // Before: steps 1–9.
    plain::set_override(Some(PlainSteer {
        counter_margin_deg: 0.0,
        counter_closing_only: false,
        trans_follow: false,
        ..PlainSteer::default()
    }));
    // The panel's ride (step 20: firm) for both halves; the step-3 lab ride no longer reaches
    // a 30–45° drift on the step-20 tyres.
    ride_lab::set_override(Some(RideTune::firm()));
    let before: Vec<_> = ids
        .iter()
        .map(|id| {
            let b = snap_back(&car(id), &a, SnapExit::CounterLift);
            assert!(b.reached, "{id}: no drift before");
            assert!(
                b.spun || b.slip_overshoot_deg > 60.0,
                "{id} before: {b:?} (expected the snap-back)"
            );
            b
        })
        .collect();
    // New: the W default (with the current grip balance).
    grip_lab::set_override(Some(GripBalance::lab_default()));
    plain::set_override(Some(PlainSteer::default()));
    for (id, b0) in ids.iter().zip(&before) {
        let c = car(id);
        let b = snap_back(&c, &a, SnapExit::CounterLift);
        assert!(b.reached && !b.spun, "{id} new: {b:?} (before {b0:?})");
        assert!(
            b.peak_yaw_acc < 0.8 * b0.peak_yaw_acc && b.yaw_overshoot < 0.85 * b0.yaw_overshoot,
            "{id}: yaw acc {} / overshoot {} vs before {} / {}",
            b.peak_yaw_acc,
            b.yaw_overshoot,
            b0.peak_yaw_acc,
            b0.yaw_overshoot
        );
        // A key-tapping driver still holds the drift.
        let h = plain_hold(
            &c,
            &a,
            PlainDriver::Tapper {
                entry_deg: 6.0,
                target_deg: 35.0,
            },
            40.0,
            0.6,
        );
        assert!(h.held && h.band_s >= 2.5, "{id} hold: {h:?}");
    }
    car_lab::set_override(None);
    grip_lab::set_override(None);
    plain::set_override(None);
    ride_lab::set_override(None);
}
