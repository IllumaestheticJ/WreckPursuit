//! Drift Lab W step 12 — lift-flick transitions (lead / tyre-slip root cause: on a flick the
//! new key turned the fronts into the old corner and their side force kept the old rotation,
//! so no car swung). The transition rack ([`wp_vehicle::plain::PlainSteer::trans_follow`])
//! points the wheels along the front axle's travel while the new key is pressed and the car
//! still slides on the old side. On the lab's default tyres and ride (spin model, grip-balance
//! "new", firm). Own test binary: the overrides are process-wide. Ranges, not exact numbers.

use std::path::PathBuf;

use wp_contracts::LoadedCar;
use wp_vehicle::car_lab::{self, CarTune};
use wp_vehicle::drift_lab_w::{lift_flick, load_any_car, reference_assists};
use wp_vehicle::grip_lab::{self, GripBalance};
use wp_vehicle::plain::{self, PlainSteer};
use wp_vehicle::ride_lab::{self, RideTune};

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    load_any_car(&data, id).unwrap()
}

#[test]
fn lift_flicks_swing_through_with_the_transition_rack() {
    let a = reference_assists();
    car_lab::set_override(Some(CarTune::spin_candidate()));
    grip_lab::set_override(Some(GripBalance::lab_default()));
    ride_lab::set_override(Some(RideTune::firm()));
    for id in ["parity_rwd_d", "parity_rwd_a"] {
        let c = car(id);
        for lift in [0.15f32, 0.25, 0.4] {
            plain::set_override(Some(PlainSteer {
                trans_follow: false,
                ..PlainSteer::default()
            }));
            let off = lift_flick(&c, &a, lift, 0.15);
            plain::set_override(Some(PlainSteer::default()));
            let on = lift_flick(&c, &a, lift, 0.15);
            assert!(off.reached && on.reached, "{id} {lift}: {off:?} {on:?}");
            assert!(!off.caught, "{id} {lift} off: {off:?}");
            assert!(
                on.caught && on.swing_s.is_some_and(|s| s < 1.6),
                "{id} {lift} on: {on:?} (off {off:?})"
            );
        }
    }
    car_lab::set_override(None);
    grip_lab::set_override(None);
    ride_lab::set_override(None);
    plain::set_override(None);
}
