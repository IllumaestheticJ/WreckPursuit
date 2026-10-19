//! Per-car cost of the vehicle model: `cargo run --release -p wp_vehicle --example bench`.
//! Reports µs per physics tick (vehicle only, and vehicle + Rapier step) and ms per 60 Hz
//! frame (4 ticks), against the 0.15 ms/car/frame budget; then the vehicle-only cost of a
//! 24-car field (the whole roster, cycled) on dry and on wet / puddled roads.

use std::time::Instant;

use wp_contracts::{AssistProfile, VehicleInput, VehicleSim};
use wp_vehicle::rig::{FlatWorld, Rig};
use wp_vehicle::{FlatGround, SurfaceConditions, Vehicle};

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

    for (label, cond) in [
        ("dry", SurfaceConditions::DRY),
        (
            "wet, 4 mm puddles",
            SurfaceConditions {
                water_depth_mm: 4.0,
                temperature_c: 4.0,
                painted_line: true,
            },
        ),
    ] {
        let us = field_of_24(cond);
        println!(
            "24-car field, {label:<17}: {us:.2} µs/car/tick = {:.4} ms/car/frame, {:.3} ms/frame for 24",
            us * 4.0 / 1000.0,
            us * 4.0 * 24.0 / 1000.0
        );
    }
}

/// Vehicle-only cost per car per tick (µs) of 24 cars from the roster, all stepped every tick
/// with varied inputs, after a 1 s warm-up.
fn field_of_24(cond: SurfaceConditions) -> f64 {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/cars");
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .expect("data/cars")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    let cars: Vec<_> = paths
        .iter()
        .filter_map(|p| wp_contracts::load_car(p).ok())
        .collect();
    assert!(!cars.is_empty());
    let mut field: Vec<Vehicle> = (0..24)
        .map(|i| {
            let car = &cars[i % cars.len()];
            let mut v = Vehicle::new(&car.def, &car.tires, [i as f32 * 5.0, 0.0, 0.0], 0.0);
            v.set_rolling(10.0 + i as f32);
            v.set_surface_conditions([cond; 4]);
            v
        })
        .collect();
    let assists = AssistProfile::keyboard_default();
    let world = FlatWorld::default();
    let ground = FlatGround::default();
    let mut out = Vec::new();
    let mut tick = |field: &mut Vec<Vehicle>, t: usize| {
        for (i, v) in field.iter_mut().enumerate() {
            let phase = (t / 240 + i) % 4;
            let input = VehicleInput {
                throttle: if phase == 3 { 0.0 } else { 0.9 },
                brake: if phase == 3 { 0.6 } else { 0.0 },
                steer: if phase == 1 { 0.4 } else { -0.1 },
                ..Default::default()
            };
            v.step_with_ground(
                &input,
                &assists,
                &world,
                &ground,
                wp_contracts::PHYSICS_DT,
                &mut out,
            );
            out.clear();
        }
    };
    for t in 0..240 {
        tick(&mut field, t);
    }
    let ticks = 240 * 20;
    let start = Instant::now();
    for t in 0..ticks {
        tick(&mut field, t);
    }
    let us = start.elapsed().as_secs_f64() * 1e6 / (ticks as f64 * field.len() as f64);
    std::hint::black_box(field[0].telemetry());
    us
}
