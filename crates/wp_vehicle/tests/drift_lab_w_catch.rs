//! Drift Lab W step 11 — over-rotation catch (owner: "more steering lock so we can catch
//! over-rotations a little easier"): the directional countersteer limit, the drift lock
//! extension and the over-rotation rack ([`wp_vehicle::plain::PlainSteer`]) against the
//! "before" (step-10 limit at every slide rate, no transition rack). On the lab's default
//! tyres and ride (spin model + grip-balance "new", firm; step 20), at the 35° push: the
//! step-20 balance already lets "before" catch pushes from 40–50°. Own test binary: the overrides are process-wide.
//! Ranges, not exact numbers (docs/integration/drift-lab-w.md, step 11).

use std::path::PathBuf;

use wp_contracts::LoadedCar;
use wp_vehicle::car_lab::{self, CarTune};
use wp_vehicle::drift_lab_w::{
    load_any_car, over_rotation_catch, reference_assists, snap_back, SnapExit,
};
use wp_vehicle::grip_lab::{self, GripBalance};
use wp_vehicle::plain::{self, PlainSteer};
use wp_vehicle::ride_lab::{self, RideTune};

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    load_any_car(&data, id).unwrap()
}

#[test]
fn over_rotations_are_caught_without_a_worse_snap_back() {
    let a = reference_assists();
    car_lab::set_override(Some(CarTune::spin_candidate()));
    grip_lab::set_override(Some(GripBalance::lab_default()));
    ride_lab::set_override(Some(RideTune::firm()));
    for id in ["parity_rwd_a", "parity_rwd_d"] {
        let c = car(id);
        plain::set_override(Some(PlainSteer {
            counter_closing_only: false,
            trans_follow: false,
            ..PlainSteer::default()
        }));
        let b0 = over_rotation_catch(&c, &a, 35.0);
        let s0 = snap_back(&c, &a, SnapExit::CounterLift);
        plain::set_override(Some(PlainSteer::default()));
        let b1 = over_rotation_catch(&c, &a, 35.0);
        let s1 = snap_back(&c, &a, SnapExit::CounterLift);
        assert!(b0.reached && b1.reached, "{id}: {b0:?} {b1:?}");
        assert!(!b0.caught, "{id} before: {b0:?}");
        assert!(
            b1.caught && b1.peak_deg < b0.peak_deg,
            "{id} new: {b1:?} (before {b0:?})"
        );
        assert!(s0.reached && s1.reached && !s1.spun, "{id} snap: {s1:?}");
        assert!(
            s1.yaw_overshoot <= s0.yaw_overshoot * 1.05
                && s1.peak_yaw_acc <= s0.peak_yaw_acc * 1.05,
            "{id} snap: {s1:?} vs before {s0:?}"
        );
    }
    car_lab::set_override(None);
    grip_lab::set_override(None);
    ride_lab::set_override(None);
    plain::set_override(None);
}
