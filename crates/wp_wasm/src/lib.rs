//! # wp_wasm
//!
//! wasm-bindgen surface for the browser testbed (owned by workstream 04). **Current state:
//! minimal JSON API** over a single-vehicle [`wp_sim::Sim`]:
//!
//! | function | purpose |
//! |---|---|
//! | `init()` | create the sim (seed 1, Arcade) and spawn `debug_sedan` at the origin |
//! | `step(dt)` | advance by `dt` seconds of frame time, returns interpolation alpha |
//! | `get_vehicle_state_json()` | `[{ "entity": u32, "state": VehicleState }]` |
//! | `get_vehicle_info_json()` | `[{ "entity": u32, "car_id": str, "body_size_m": [w,l,h] }]` |
//! | `set_input_json(json)` | `VehicleInput` JSON (missing fields = 0/false) for the player car |
//! | `reset_vehicle()` | put the player car back at spawn |
//!
//! Car data is embedded at compile time (the browser has no filesystem).

#![warn(missing_docs)]

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde::Serialize;
use wasm_bindgen::prelude::*;
use wp_contracts::{EntityId, LoadedCar, TimeOfDay, VehicleInput, VehicleState};
use wp_sim::{Sim, SimConfig};

const SEDAN: &str = include_str!("../../../data/cars/debug_sedan.json");
const SEDAN_LATTICE: &str = include_str!("../../../data/lattices/debug_sedan.json");
const STREET_TIRE: &str = include_str!("../../../data/tires/street.json");
const ARCADE: &str = include_str!("../../../data/modes/arcade.toml");
const MATERIALS: [(&str, &str); 5] = [
    (
        "mild_steel",
        include_str!("../../../data/materials/mild_steel.json"),
    ),
    ("hss", include_str!("../../../data/materials/hss.json")),
    (
        "aluminum",
        include_str!("../../../data/materials/aluminum.json"),
    ),
    (
        "pp_bumper",
        include_str!("../../../data/materials/pp_bumper.json"),
    ),
    ("glass", include_str!("../../../data/materials/glass.json")),
];

struct App {
    sim: Sim,
    player: EntityId,
    car: LoadedCar,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn embedded_sedan() -> Result<LoadedCar, String> {
    let def = wp_contracts::parse_vehicle_def("debug_sedan", SEDAN).map_err(|e| e.to_string())?;
    let lattice =
        wp_contracts::parse_lattice_def("debug_sedan", SEDAN_LATTICE).map_err(|e| e.to_string())?;
    let tire = wp_contracts::parse_tire_def("street", STREET_TIRE).map_err(|e| e.to_string())?;
    let mut materials = BTreeMap::new();
    for (id, text) in MATERIALS {
        let m = wp_contracts::parse_material_def(id, text).map_err(|e| e.to_string())?;
        materials.insert(m.id.clone(), m);
    }
    let tires = BTreeMap::from([(tire.id.clone(), tire)]);
    wp_contracts::resolve_car(def, &tires, &[lattice], &materials).map_err(|e| e.to_string())
}

fn build() -> Result<App, String> {
    let car = embedded_sedan()?;
    let mode = wp_contracts::parse_mode_table("arcade", ARCADE).map_err(|e| e.to_string())?;
    let mut sim = Sim::new(SimConfig {
        seed: 1,
        mode,
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 10.0,
    });
    let player = sim
        .spawn_vehicle(&car, [0.0, 0.0, 0.0], 0.0)
        .map_err(|e| e.to_string())?;
    Ok(App { sim, player, car })
}

fn with_app<T>(f: impl FnOnce(&mut App) -> T) -> Result<T, JsValue> {
    APP.with(|app| match app.borrow_mut().as_mut() {
        Some(a) => Ok(f(a)),
        None => Err(JsValue::from_str("wp_wasm: call init() first")),
    })
}

/// Create the simulation and spawn the debug sedan. Safe to call again (resets everything).
#[wasm_bindgen]
pub fn init() -> Result<(), JsValue> {
    let app = build().map_err(|e| JsValue::from_str(&e))?;
    APP.with(|a| *a.borrow_mut() = Some(app));
    Ok(())
}

/// Advance by `dt` seconds of frame time; returns the render interpolation alpha `0..1`.
#[wasm_bindgen]
pub fn step(dt: f32) -> Result<f32, JsValue> {
    with_app(|a| a.sim.advance(dt))
}

#[derive(Serialize)]
struct StateEntry {
    entity: u32,
    state: VehicleState,
}

/// JSON array of `{ entity, state }` for every vehicle, ascending by entity.
#[wasm_bindgen]
pub fn get_vehicle_state_json() -> Result<String, JsValue> {
    with_app(|a| {
        let entries: Vec<StateEntry> = a
            .sim
            .vehicle_states()
            .into_iter()
            .map(|(id, state)| StateEntry {
                entity: id.0,
                state,
            })
            .collect();
        serde_json::to_string(&entries).expect("states serialize")
    })
}

#[derive(Serialize)]
struct InfoEntry<'a> {
    entity: u32,
    car_id: &'a str,
    body_size_m: [f32; 3],
}

/// JSON array of `{ entity, car_id, body_size_m }` for every vehicle.
#[wasm_bindgen]
pub fn get_vehicle_info_json() -> Result<String, JsValue> {
    with_app(|a| {
        let entries: Vec<InfoEntry> = a
            .sim
            .vehicles()
            .iter()
            .map(|v| InfoEntry {
                entity: v.id.0,
                car_id: &v.car_id,
                body_size_m: a.car.def.body_size_m,
            })
            .collect();
        serde_json::to_string(&entries).expect("info serializes")
    })
}

/// Set the player vehicle's input from `VehicleInput` JSON.
#[wasm_bindgen]
pub fn set_input_json(json: &str) -> Result<(), JsValue> {
    let input: VehicleInput =
        serde_json::from_str(json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    with_app(|a| a.sim.set_input(a.player, input).map_err(|e| e.to_string()))?
        .map_err(|e| JsValue::from_str(&e))
}

/// Return the player vehicle to its spawn pose.
#[wasm_bindgen]
pub fn reset_vehicle() -> Result<(), JsValue> {
    with_app(|a| a.sim.reset_vehicle(a.player).map_err(|e| e.to_string()))?
        .map_err(|e| JsValue::from_str(&e))
}

#[cfg(test)]
mod tests {
    #[test]
    fn embedded_data_resolves_and_drives() {
        let mut app = super::build().expect("embedded data valid");
        let input = wp_contracts::VehicleInput {
            throttle: 1.0,
            ..Default::default()
        };
        app.sim.set_input(app.player, input).unwrap();
        app.sim.advance(1.0);
        assert!(app.sim.vehicle_states()[0].1.speed_mps > 1.0);
    }
}
