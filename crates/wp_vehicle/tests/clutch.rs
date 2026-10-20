//! Manual clutch (owner row 59): the pedal sets clutch engagement (slip and torque transfer),
//! the engine stalls when the clutch drags it below half its idle speed (stopping in gear,
//! pulling away in too high a gear), pressing the pedal re-cranks it, and only the automatic
//! clutch has anti-stall. Same in both damage modes. (Letting the clutch up in 1st at idle on
//! flat ground creeps away without stalling, as most cars with an idle governor do.)

use wp_contracts::{AssistProfile, LoadedCar, VehicleInput};
use wp_vehicle::rig::Rig;

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap()
}

fn profile(auto_clutch: bool) -> AssistProfile {
    AssistProfile {
        auto_gears: false,
        auto_clutch,
        ..AssistProfile::wheel_default()
    }
}

fn step(rig: &mut Rig, a: &AssistProfile, throttle: f32, clutch: f32, seconds: f32) {
    for _ in 0..(seconds * 240.0) as usize {
        rig.step(
            &VehicleInput {
                throttle,
                clutch,
                ..Default::default()
            },
            a,
        );
    }
}

fn shift_to(rig: &mut Rig, a: &AssistProfile, gear: i8) {
    while rig.state().gear < gear {
        rig.step(
            &VehicleInput {
                clutch: 1.0,
                gear_up: true,
                ..Default::default()
            },
            a,
        );
        step(rig, a, 0.0, 1.0, 0.4);
    }
}

#[test]
fn stopping_in_gear_or_pulling_away_in_third_stalls_and_the_pedal_re_cranks() {
    for id in ["debug_coupe", "kestrel_ts", "rampart_50"] {
        let manual = profile(false);
        let idle = car(id).def.engine.idle_rpm;
        // Braking to a stop in 2nd with the clutch up: the engine dies.
        let mut rig = Rig::new(&car(id));
        rig.teleport([0.0; 3], 0.0, 6.0, 0.0);
        shift_to(&mut rig, &manual, 2);
        for _ in 0..3 * 240 {
            rig.step(
                &VehicleInput {
                    brake: 1.0,
                    ..Default::default()
                },
                &manual,
            );
        }
        assert!(
            rig.vehicle.telemetry().stalled,
            "{id}: {}",
            rig.state().engine_rpm
        );
        assert!(rig.state().engine_rpm < 0.5 * idle, "{id}");
        // A stalled engine makes no torque: throttle does nothing.
        step(&mut rig, &manual, 1.0, 0.0, 0.5);
        assert!(
            rig.vehicle.telemetry().stalled && rig.state().speed_mps.abs() < 0.3,
            "{id}"
        );
        // Clutch pressed: re-cranked to idle.
        step(&mut rig, &manual, 0.0, 1.0, 0.5);
        assert!(!rig.vehicle.telemetry().stalled, "{id}");
        assert!(rig.state().engine_rpm > 0.9 * idle, "{id}");
        // Dropping the clutch at rest in 3rd: stalls again.
        shift_to(&mut rig, &manual, 3);
        step(&mut rig, &manual, 0.0, 0.0, 1.0);
        assert!(
            rig.vehicle.telemetry().stalled,
            "{id} in 3rd: {}",
            rig.state().engine_rpm
        );
    }
}

#[test]
fn slipping_the_pedal_launches_without_stalling() {
    for id in ["debug_coupe", "kestrel_ts", "rampart_50"] {
        let manual = profile(false);
        let mut rig = Rig::new(&car(id));
        step(&mut rig, &manual, 0.0, 1.0, 0.5);
        let mut slipping = false;
        for k in 0..360 {
            rig.step(
                &VehicleInput {
                    throttle: 0.4,
                    clutch: 1.0 - k as f32 / 360.0,
                    ..Default::default()
                },
                &manual,
            );
            let t = rig.vehicle.telemetry();
            assert!(
                !t.stalled,
                "{id} stalled at pedal {}",
                1.0 - k as f32 / 360.0
            );
            // Engagement follows the pedal; near the top of its travel the clutch slips
            // (partial torque) instead of locking.
            if k == 60 {
                slipping = !t.clutch_locked && (t.clutch - 1.0 / 6.0).abs() < 0.02;
            }
        }
        step(&mut rig, &manual, 0.4, 0.0, 1.0);
        assert!(slipping, "{id}: a barely released pedal should slip");
        assert!(
            rig.state().speed_mps > 2.0,
            "{id}: {}",
            rig.state().speed_mps
        );
        assert!(!rig.vehicle.telemetry().stalled, "{id}");
    }
}

#[test]
fn the_automatic_clutch_never_stalls() {
    // Same "pedal up, no throttle, standing still" as the stall test: the automatic clutch
    // (any preset; a player who picks "automatic") opens instead.
    for id in ["debug_coupe", "kestrel_ts", "rampart_50"] {
        let auto = profile(true);
        let mut rig = Rig::new(&car(id));
        step(&mut rig, &auto, 0.0, 0.0, 2.0);
        assert!(!rig.vehicle.telemetry().stalled, "{id}");
        assert!(
            rig.state().engine_rpm > 0.9 * car(id).def.engine.idle_rpm,
            "{id}"
        );
        step(&mut rig, &auto, 0.5, 0.0, 2.0);
        assert!(rig.state().speed_mps > 3.0, "{id}");
    }
}
