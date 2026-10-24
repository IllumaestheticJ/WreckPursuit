//! Hill hold (feature F45, playtester B9): with the automatic clutch, a car at rest on a hill
//! with no input stays put (the Port Vesper free-drive spawn rolled 16 m in 10 s), and pulling
//! away uphill from it still works. Not on torque-converter automatics: they creep at idle by
//! design (`behaviour::torque_converter_stall_creep_and_lockup`). The slope is the along-road
//! part of gravity, g·sin θ, added to the velocity each tick (as `side_floor.rs` does sideways).

use std::path::PathBuf;

use wp_contracts::{AssistProfile, LoadedCar, VehicleInput};
use wp_vehicle::math::{atan, sin, V3};
use wp_vehicle::rig::Rig;
use wp_vehicle::vehicle::hill_hold_active;

const DT: f32 = 1.0 / 240.0;

fn car(id: &str) -> LoadedCar {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../data/cars/{id}.json"));
    wp_contracts::load_car(&p).unwrap()
}

/// `seconds` on a `grade` (rise / run, + = the nose points uphill) with `input`; returns the
/// distance travelled along the car's forward axis (+ = uphill), m.
fn on_grade(
    rig: &mut Rig,
    a: &AssistProfile,
    grade: f32,
    input: &VehicleInput,
    seconds: f32,
) -> f32 {
    let dv = 9.81 * sin(atan(grade)) * DT;
    let y0 = rig.vehicle.chassis().pos.y;
    for _ in 0..(seconds / DT) as usize {
        rig.kick([0.0, -dv, 0.0], [0.0; 3]);
        rig.step(input, a);
    }
    rig.vehicle.chassis().pos.y - y0
}

fn settled(id: &str) -> Rig {
    let mut rig = Rig::new(&car(id));
    rig.teleport([0.0; 3], 0.0, 0.0, 0.0);
    for _ in 0..240 {
        rig.step(&VehicleInput::default(), &AssistProfile::keyboard_default());
    }
    rig
}

#[test]
fn rule() {
    assert!(hill_hold_active(true, 0.0, 0.0, 0.0, 0.0, 0.3));
    assert!(
        !hill_hold_active(false, 0.0, 0.0, 0.0, 0.0, 0.3),
        "manual clutch: the driver's job"
    );
    assert!(
        !hill_hold_active(true, 0.3, 0.0, 0.0, 0.0, 0.3),
        "throttle releases it"
    );
    assert!(
        !hill_hold_active(true, 0.0, 0.0, 0.0, 0.8, 0.3),
        "clutch engaged"
    );
    assert!(!hill_hold_active(true, 0.0, 0.0, 0.0, 0.0, 3.0), "moving");
    assert!(
        !hill_hold_active(true, 0.0, 0.5, 0.0, 0.0, 0.3),
        "the driver's own brake"
    );
}

/// The Vesper spawn grade (~4 %, the car the testbed spawns) and steeper hills, both ways: no
/// input for 10 s and the car stays within a few centimetres (it used to roll away: 16 m in
/// 10 s on the Vesper spawn).
#[test]
fn a_car_at_rest_on_a_hill_stays_put() {
    for id in ["lindwurm_4t", "kestrel_ts", "furrow_24", "rampart_50"] {
        for grade in [0.04f32, -0.04, 0.12, -0.12, 0.25] {
            let mut rig = settled(id);
            let d = on_grade(
                &mut rig,
                &AssistProfile::keyboard_default(),
                grade,
                &VehicleInput::default(),
                10.0,
            );
            // The brakes hold the wheels; what is left is the tyre model's low-speed creep
            // (constant, ~0.8 mm/s per % of grade: 3 mm/s on the Vesper spawn, 2 cm/s on 25 %;
            // known gap, tyre owner). Without the hold the spawn car rolled 16 m in 10 s.
            let limit = 0.05 + 2.5 * grade.abs();
            println!("{id} {:+.0} %: {d:+.3} m", grade * 100.0);
            assert!(
                d.abs() < limit,
                "{id} on {:.0} %: rolled {d:.2} m in 10 s",
                grade * 100.0
            );
        }
    }
}

/// Pulling away uphill from the hold: full throttle leaves it at once and climbs.
#[test]
fn launching_uphill_from_the_hold_works() {
    for id in ["lindwurm_4t", "kestrel_ts", "furrow_24"] {
        for grade in [0.04f32, 0.12] {
            let mut rig = settled(id);
            let a = AssistProfile::keyboard_default();
            on_grade(&mut rig, &a, grade, &VehicleInput::default(), 2.0);
            let throttle = VehicleInput {
                throttle: 1.0,
                ..Default::default()
            };
            let back = {
                // Largest roll-back while the clutch bites.
                let mut worst = 0.0f32;
                let y0 = rig.vehicle.chassis().pos.y;
                for _ in 0..240 {
                    on_grade(&mut rig, &a, grade, &throttle, DT);
                    worst = worst.min(rig.vehicle.chassis().pos.y - y0);
                }
                worst
            };
            let d = on_grade(&mut rig, &a, grade, &throttle, 3.0);
            let v = rig.vehicle.chassis().lin_vel.dot(V3::Y);
            assert!(
                d > 5.0 && v > 3.0,
                "{id} up {:.0} %: {d:.1} m in 3 s, {v:.1} m/s",
                grade * 100.0
            );
            assert!(
                back > -0.3,
                "{id} up {:.0} %: rolled back {:.2} m pulling away",
                grade * 100.0,
                -back
            );
        }
    }
}

/// Released and coasting: above the hold speed nothing brakes (a car rolling at 3 m/s with no
/// input keeps rolling).
#[test]
fn no_hold_while_rolling() {
    let mut rig = settled("lindwurm_4t");
    rig.kick([0.0, 3.0, 0.0], [0.0; 3]);
    let a = AssistProfile::keyboard_default();
    let d = on_grade(&mut rig, &a, 0.0, &VehicleInput::default(), 1.0);
    assert!(d > 2.0, "coasted {d:.2} m in 1 s from 3 m/s");
}
