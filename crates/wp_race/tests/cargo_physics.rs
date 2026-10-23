//! Delivery cargo is felt by the physics: the same car with 400 kg in the boot (via
//! `wp_race::cargo::load_cargo`) accelerates and brakes worse in the real `wp_sim` loop.

use std::path::{Path, PathBuf};

use wp_contracts::{load_car, load_mode_table, LoadedCar, TimeOfDay, VehicleInput, VehicleSim};
use wp_race::cargo::{load_cargo, CargoLoad};
use wp_race::RaceKind;
use wp_sim::{Sim, SimConfig};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Full throttle for 6 s from rest; separately, full brake from 25 m/s until stopped.
/// Returns (distance at 6 s, braking distance), m.
fn launch_and_stop(car: &LoadedCar) -> (f32, f32) {
    (launch(car), stop(car))
}

fn sim_with(car: &LoadedCar) -> (Sim, wp_contracts::EntityId) {
    let mode = load_mode_table(repo().join("data/modes/sim.toml")).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed: 3,
        mode,
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 1.0,
    });
    let id = sim.spawn_vehicle(car, [0.0, 0.0, 0.0], 0.0).unwrap();
    sim.set_assists(id, wp_contracts::AssistProfile::ai())
        .unwrap();
    // Settle on the springs.
    for _ in 0..240 {
        sim.step_fixed();
    }
    (sim, id)
}

fn stop(car: &LoadedCar) -> f32 {
    let (mut sim, id) = sim_with(car);
    sim.place_vehicle(id, [0.0, 0.0, 0.0], 0.0, 25.0).unwrap();
    sim.set_input(
        id,
        VehicleInput {
            brake: 1.0,
            ..VehicleInput::default()
        },
    )
    .unwrap();
    for _ in 0..240 * 12 {
        sim.step_fixed();
        if sim.vehicles()[0].vehicle.state().lin_vel[1].abs() < 0.2 {
            break;
        }
    }
    sim.vehicles()[0].vehicle.state().pos[1]
}

fn launch(car: &LoadedCar) -> f32 {
    let (mut sim, id) = sim_with(car);
    let y0 = sim.vehicles()[0].vehicle.state().pos[1];
    sim.set_input(
        id,
        VehicleInput {
            throttle: 1.0,
            ..VehicleInput::default()
        },
    )
    .unwrap();
    for _ in 0..240 * 6 {
        sim.step_fixed();
    }
    sim.vehicles()[0].vehicle.state().pos[1] - y0
}

#[test]
fn delivery_cargo_slows_the_car() {
    let car = load_car(repo().join("data/cars/debug_sedan.json")).unwrap();
    let kind = RaceKind::Delivery {
        cargo_kg: 400.0,
        cargo_value: 5000,
    };
    let cargo = CargoLoad::for_kind(kind, &car.def).expect("delivery has cargo");
    let loaded = load_cargo(&car, &cargo);
    let (empty_6s, empty_brake) = launch_and_stop(&car);
    let (loaded_6s, loaded_brake) = launch_and_stop(&loaded);
    eprintln!(
        "empty: {empty_6s:.1} m in 6 s, brakes in {empty_brake:.1} m; \
         loaded: {loaded_6s:.1} m, {loaded_brake:.1} m"
    );
    assert!(empty_6s > 40.0, "car barely moved: {empty_6s}");
    // ~27 % more mass: clearly slower off the line, longer to stop.
    assert!(
        loaded_6s < empty_6s * 0.95,
        "loaded {loaded_6s:.1} m vs empty {empty_6s:.1} m"
    );
    assert!(
        loaded_brake > empty_brake * 1.03,
        "loaded braking {loaded_brake:.1} m vs empty {empty_brake:.1} m"
    );
}
