//! Drift Lab W "use W handling everywhere" (`wp_vehicle::lab_scope`): with the overrides
//! limited to tagged vehicles, the tagged (player) car drives exactly as under the overrides
//! and an untagged one (AI, police) exactly as without them. Own test binary: the overrides
//! are process-wide.

use std::path::PathBuf;

use wp_contracts::{AssistProfile, LoadedCar, VehicleInput};
use wp_vehicle::car_lab::{self, CarTune};
use wp_vehicle::drift_lab_w::{load_any_car, reference_assists};
use wp_vehicle::lab_scope;
use wp_vehicle::plain::{self, PlainSteer};
use wp_vehicle::ride_lab::{self, RideTune};
use wp_vehicle::rig::Rig;

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    load_any_car(&data, id).unwrap()
}

/// 3 s at 60 km/h: steer key left, throttle, a handbrake pull: plain rack, ride, tyres and
/// the pivot aid all act. State hash at the end.
fn drive(c: &LoadedCar, a: &AssistProfile, tagged: bool) -> u64 {
    let mut rig = Rig::new(c);
    rig.set_lab_tagged(tagged);
    rig.teleport([0.0; 3], 0.0, 60.0 / 3.6, 0.0);
    for t in 0..720 {
        rig.step(
            &VehicleInput {
                steer: -1.0,
                throttle: 0.8,
                handbrake: if (240..360).contains(&t) { 1.0 } else { 0.0 },
                ..Default::default()
            },
            a,
        );
    }
    rig.state_hash()
}

#[test]
fn tagged_only_mode_keeps_the_w_profile_on_the_tagged_car_only() {
    let c = car("parity_rwd_d");
    let a = AssistProfile {
        tcs: wp_contracts::TcsLevel::Off,
        ..AssistProfile::keyboard_default()
    };
    let stock = drive(&c, &a, false);
    plain::set_override(Some(PlainSteer::default()));
    ride_lab::set_override(Some(RideTune::slow_weight()));
    car_lab::set_override(Some(CarTune::spin_candidate()));
    let w = drive(&c, &a, false);
    assert_ne!(w, stock, "the overrides change the drive");
    // Default scope: every vehicle, tagged or not.
    assert_eq!(drive(&c, &a, true), w);
    lab_scope::set_tagged_only(true);
    let tagged = drive(&c, &a, true);
    let untagged = drive(&c, &a, false);
    lab_scope::set_tagged_only(false);
    plain::set_override(None);
    ride_lab::set_override(None);
    car_lab::set_override(None);
    assert_eq!(tagged, w, "tagged car: the W profile");
    assert_eq!(untagged, stock, "untagged car: stock handling");
    // The lab's own assists make no difference to the scoping.
    let _ = reference_assists();
}
