//! Spin side-force floor (`wp_vehicle::tire::SPIN_SIDE_FLOOR`, docs/integration/side-floor.md):
//! every roster car floored from rest on a cambered road launches straight. With the old
//! friction ellipse a rear spinning at κ ≈ 5–8 keeps almost no side force, so a 1° road
//! cross-slope swung every RWD road car 35–180° within 3 s (the owner's "spins off the line").

use std::path::PathBuf;

use wp_contracts::{AssistProfile, LoadedCar, TcsLevel, VehicleInput};
use wp_vehicle::math::{atan2, sin, V3};
use wp_vehicle::rig::Rig;

fn roster() -> Vec<LoadedCar> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/cars");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|p| wp_contracts::load_car(p).unwrap())
        .collect()
}

/// TCS / ESC off, no steering help or filters, manual gears (stays in 1st): the raw car.
fn raw_assists() -> AssistProfile {
    AssistProfile {
        tcs: TcsLevel::Off,
        esc: false,
        countersteer_help: 0.0,
        throttle_filter: 0.0,
        auto_gears: false,
        ..AssistProfile::keyboard_default()
    }
}

/// Rest → full throttle for 3 s, steer 0, on a road cross-slope of `slope_deg` (the sideways
/// part of gravity, g·sin θ, added each tick) with a 0.02 rad/s yaw disturbance at the start
/// (a real launch is never perfectly symmetric). Largest heading change, degrees.
fn launch_yaw(c: &LoadedCar, a: &AssistProfile, slope_deg: f32) -> f32 {
    let mut rig = Rig::new(c);
    rig.teleport([0.0; 3], 0.0, 0.0, 0.0);
    for _ in 0..120 {
        rig.step(&VehicleInput::default(), a);
    }
    let heading = |rig: &Rig| {
        let f = rig.vehicle.chassis().rot * V3::Y;
        atan2(-f.x, f.y).to_degrees()
    };
    let h0 = heading(&rig);
    rig.kick([0.0; 3], [0.0, 0.0, 0.02]);
    let side_dv = 9.81 * sin(slope_deg.to_radians()) / 240.0;
    let mut yaw = 0.0f32;
    for _ in 0..3 * 240 {
        rig.kick([side_dv, 0.0, 0.0], [0.0; 3]);
        rig.step(
            &VehicleInput {
                throttle: 1.0,
                ..Default::default()
            },
            a,
        );
        yaw = yaw.max((heading(&rig) - h0).abs());
    }
    yaw
}

#[test]
fn roster_launches_straight_on_cambered_roads() {
    // Re-ranged (unify, 2026-10-21): owner-chosen balance (row 126); revisit after the owner's ×0.45 / 0.50 / 0.55 A/B. On the owner's soft
    // step-14 RWD rear the worst launch yaws 4.7° (ironside_440 on 3°); limit 5.5° (was 3°).
    // Re-ranged (physics lead, 2026-10-23, candidate G: softer bars and springs): ironside_440
    // on 3° yaws 5.5°; limit 6.0°.
    const LIMIT_DEG: f32 = 6.0;
    let a = raw_assists();
    let mut fails = Vec::new();
    for c in roster() {
        for slope in [0.0f32, 1.0, 2.0, 3.0] {
            let y = launch_yaw(&c, &a, slope);
            if y >= LIMIT_DEG {
                fails.push(format!("{} on {slope}°: {y:.1}°", c.def.id));
            }
        }
    }
    assert!(
        fails.is_empty(),
        "launches that yaw ≥ {LIMIT_DEG}°: {fails:?}"
    );
}
