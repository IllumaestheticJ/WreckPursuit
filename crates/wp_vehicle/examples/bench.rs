//! Per-car cost of the vehicle model: `cargo run --release -p wp_vehicle --example bench`.
//! Reports µs per physics tick (vehicle only, and vehicle + Rapier step) and ms per 60 Hz
//! frame (4 ticks), against the 0.15 ms/car/frame budget.

use std::time::Instant;

use wp_contracts::{AssistProfile, VehicleInput, VehicleSim};
use wp_vehicle::rig::{FlatWorld, Rig};
use wp_vehicle::{FlatGround, Vehicle};

fn main() {
    let car = wp_contracts::load_car(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/cars/debug_coupe.json"
    ))
    .expect("car");
    let assists = AssistProfile::keyboard_default();
    let input = VehicleInput {
        throttle: 0.8,
        steer: 0.3,
        ..Default::default()
    };
    let ticks = 240 * 60;

    // Vehicle force computation only (flat-ground probe, no rigid-body integration).
    let mut v = Vehicle::new(&car.def, &car.tires, [0.0; 3], 0.0);
    let world = FlatWorld::default();
    let mut out = Vec::new();
    let t = Instant::now();
    for _ in 0..ticks {
        v.step_with_ground(
            &input,
            &assists,
            &world,
            &FlatGround::default(),
            wp_contracts::PHYSICS_DT,
            &mut out,
        );
    }
    let us_vehicle = t.elapsed().as_secs_f64() * 1e6 / ticks as f64;
    std::hint::black_box(v.ffb());

    // Full rig: vehicle + Rapier query pipeline probes + Rapier step for one body.
    let mut rig = Rig::new(&car);
    let t = Instant::now();
    for _ in 0..ticks {
        rig.step(&input, &assists);
    }
    let us_rig = t.elapsed().as_secs_f64() * 1e6 / ticks as f64;
    println!(
        "vehicle model:        {us_vehicle:.2} µs/tick = {:.4} ms/frame (budget 0.15)",
        us_vehicle * 4.0 / 1000.0
    );
    println!(
        "vehicle + rapier rig: {us_rig:.2} µs/tick = {:.4} ms/frame",
        us_rig * 4.0 / 1000.0
    );
}
