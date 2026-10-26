//! Reinforced cooling (the career's radiator / oil cooler mods, `Sim::set_cooling_tier`)
//! through the full game loop: a 90 km/h frontal hit holes the radiator, and with the race
//! radiator (tier 2) the engine heats [`COOLING_OVERHEAT_SCALE`] as fast as on the stock one,
//! so the car limps further before it cooks.

use wp_contracts::{load_car, load_mode_table, AssistProfile, Component, TimeOfDay, VehicleInput};
use wp_sim::{Sim, SimConfig, COOLING_OVERHEAT_SCALE};

fn data(path: &str) -> String {
    format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
}

/// Engine heat 20 s after a 90 km/h frontal hit on a wall, and whether the radiator failed.
fn heat_after_hit(tier: u8) -> (bool, f32) {
    let car = load_car(data("cars/kestrel_ts.json")).unwrap();
    let mode = load_mode_table(data("modes/extreme.toml")).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed: 11,
        mode,
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 10.0,
    });
    sim.add_static_box([0.0, 12.0, 1.0], [12.0, 1.0, 2.0], 0.0);
    let id = sim.spawn_vehicle(&car, [0.0; 3], 0.0).unwrap();
    sim.set_assists(id, AssistProfile::wheel_default()).unwrap();
    sim.set_cooling_tier(id, tier).unwrap();
    for _ in 0..240 {
        sim.step_fixed();
    }
    sim.place_vehicle(id, [0.0; 3], 0.0, 90.0 / 3.6).unwrap();
    sim.set_input(id, VehicleInput::default()).unwrap();
    for _ in 0..240 * 20 {
        sim.step_fixed();
    }
    let v = &sim.vehicles()[0];
    (
        v.damage.component_failed(Component::Radiator),
        v.vehicle.telemetry().engine_heat,
    )
}

#[test]
fn reinforced_cooling_slows_a_holed_radiator() {
    let (failed, stock) = heat_after_hit(0);
    let (failed2, race) = heat_after_hit(2);
    println!("stock heat {stock:.3}, race radiator heat {race:.3}");
    assert!(failed && failed2, "the hit should hole the radiator");
    assert!(stock > 0.0, "a holed radiator heats the engine");
    let want = stock * COOLING_OVERHEAT_SCALE[2];
    assert!(
        (race - want).abs() <= 0.1 * stock,
        "race radiator heat {race:.3}, expected ~{want:.3}"
    );
}
