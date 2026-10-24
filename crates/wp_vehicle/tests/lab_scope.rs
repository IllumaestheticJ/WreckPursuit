//! The unified handling (`wp_vehicle::lab_scope`, lead 2026-10-21): with no lab override, a
//! player-driven car gets the shipped plain steering on top of the shipped physics, and an
//! AI-driven one the same physics with the normal steering model. The legacy switch brings
//! back the pre-unify stock path. Own test binary: the switches are process-wide.

use std::path::PathBuf;

use wp_contracts::{AssistProfile, LoadedCar, VehicleInput};
use wp_vehicle::drift_lab_w::load_any_car;
use wp_vehicle::lab_scope;
use wp_vehicle::rig::Rig;

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    load_any_car(&data, id).unwrap()
}

/// 3 s at 60 km/h: steer key left, throttle, a handbrake pull: plain rack, ride, tyres and
/// the pivot aid all act. State hash at the end.
fn drive(c: &LoadedCar, a: &AssistProfile, player: bool) -> u64 {
    let mut rig = Rig::new(c);
    rig.set_player_input(player);
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
fn shipped_physics_on_every_car_steering_aids_on_player_cars_only() {
    let c = car("parity_rwd_d");
    let a = AssistProfile::ai();
    let player = drive(&c, &a, true);
    let ai = drive(&c, &a, false);
    // A bare rig nobody marked: the physics with the normal steering model (as AI).
    let bare = {
        let mut rig = Rig::new(&c);
        rig.teleport([0.0; 3], 0.0, 60.0 / 3.6, 0.0);
        for t in 0..720 {
            rig.step(
                &VehicleInput {
                    steer: -1.0,
                    throttle: 0.8,
                    handbrake: if (240..360).contains(&t) { 1.0 } else { 0.0 },
                    ..Default::default()
                },
                &a,
            );
        }
        rig.state_hash()
    };
    assert_eq!(bare, ai, "an unmarked rig drives like an AI car");
    assert_ne!(player, ai, "the plain steering changes the player's drive");
    // AI = the shipped physics without the plain steering.
    lab_scope::set_plain_off(true);
    let player_plain_off = drive(&c, &a, true);
    lab_scope::set_plain_off(false);
    assert_eq!(player_plain_off, ai, "same physics for AI and player cars");
    // The pre-unify stock path differs from the shipped physics.
    lab_scope::set_legacy(true);
    let legacy = drive(&c, &a, false);
    lab_scope::set_legacy(false);
    assert_ne!(legacy, ai, "the shipped physics is not the old data path");
    assert_eq!(drive(&c, &a, false), ai, "deterministic");
}
