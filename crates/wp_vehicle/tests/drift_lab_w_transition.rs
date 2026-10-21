//! Drift Lab W step 12 — lift-flick transitions (lead / tyre-slip root cause: on a flick the
//! new key turned the fronts into the old corner and their side force kept the old rotation,
//! so no car swung). The transition rack ([`wp_vehicle::plain::PlainSteer::trans_follow`])
//! points the wheels along the front axle's travel while the new key is pressed and the car
//! still slides on the old side; the new-side catch (step 13, `trans_catch_gain`) then points
//! the countersteer at the travel while the new slide grows, so it settles. On the lab's
//! default tyres and ride (spin model, grip-balance
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
    // Written against the pre-unify stock path: `None` overrides = the car data
    // (`wp_vehicle::lab_scope`; process-wide, every test in this binary sets it).
    wp_vehicle::lab_scope::set_legacy(true);
    let a = reference_assists();
    car_lab::set_override(Some(CarTune::spin_candidate()));
    grip_lab::set_override(Some(GripBalance::lab_default()));
    ride_lab::set_override(Some(RideTune::firm()));
    for id in ["parity_rwd_d", "parity_rwd_a", "parity_rwd_b"] {
        let c = car(id);
        let (mut settled_off, mut settled_on) = (Vec::new(), Vec::new());
        for lift in [0.15f32, 0.25, 0.4] {
            plain::set_override(Some(PlainSteer {
                trans_follow: false,
                ..PlainSteer::default()
            }));
            let off = lift_flick(&c, &a, lift, 0.15);
            // Step 12 only (the new-side catch off): swings, settles slowly.
            plain::set_override(Some(PlainSteer {
                trans_catch_deg: -1.0,
                ..PlainSteer::default()
            }));
            let s12 = lift_flick(&c, &a, lift, 0.15);
            settled_off.push(s12.settle_s.unwrap_or(9.0));
            plain::set_override(Some(PlainSteer::default()));
            let on = lift_flick(&c, &a, lift, 0.15);
            assert!(off.reached && on.reached, "{id} {lift}: {off:?} {on:?}");
            assert!(!off.caught, "{id} {lift} off: {off:?}");
            assert!(
                on.caught && on.swing_s.is_some_and(|s| s < 1.6),
                "{id} {lift} on: {on:?} (off {off:?})"
            );
            assert!(s12.caught, "{id} {lift} step 12: {s12:?}");
            // The settle time is step 13's catch at its own gain, 0.9 of the travel. The default
            // is 0.8 since playtest row 137 (0.9 over-caught rampart_50 into a second swing,
            // tests/transition_once.rs): it still catches every flick here (above) but settles
            // slower in this pre-unify lab setup (parity_rwd_d median 1.6 s).
            plain::set_override(Some(PlainSteer {
                trans_catch_gain: 0.9,
                ..PlainSteer::default()
            }));
            let s13 = lift_flick(&c, &a, lift, 0.15);
            settled_on.push(s13.settle_s.unwrap_or(9.0));
        }
        // Step 13, the new-side catch (at 0.9): settles on the new side within ~0.5 s (median),
        // where step 12 alone took ~1.5 s.
        let med = |v: &mut Vec<f32>| {
            v.sort_by(f32::total_cmp);
            v[v.len() / 2]
        };
        let (s12, s13) = (med(&mut settled_off), med(&mut settled_on));
        assert!(
            s13 <= 0.6 && s12 >= 1.0,
            "{id}: settle {s13} s (step 12: {s12} s)"
        );
    }
    car_lab::set_override(None);
    grip_lab::set_override(None);
    ride_lab::set_override(None);
    plain::set_override(None);
}
