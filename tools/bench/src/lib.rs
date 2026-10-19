//! # wp_bench
//!
//! Shared setup for the criterion benches in `benches/sim.rs` and the perf gate
//! (`src/bin/perf_gate.rs`, see [`gate`]). Benches measure one physics tick (or one snapshot
//! encode/decode); the gate converts them to per-frame numbers (60 Hz = 4 ticks) and compares
//! them against blessed baselines per runner class.

#![warn(missing_docs)]

pub mod gate;

use std::path::PathBuf;

use wp_contracts::{
    AssistProfile, EntityId, LoadedCar, ServerToClient, TimeOfDay, VehicleInput,
    PHYSICS_TICKS_PER_NET_TICK,
};
use wp_sim::{Sim, SimConfig};

/// Repo `data/` folder.
pub fn data_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

/// Load a car by id from `data/cars`.
pub fn car(id: &str) -> LoadedCar {
    wp_contracts::load_car(data_root().join("cars").join(format!("{id}.json")))
        .unwrap_or_else(|e| panic!("load {id}: {e}"))
}

/// A sim with `n` vehicles (alternating debug sedan / coupe) on a 10 m grid, each with a
/// different steady throttle/steer, warmed up for one second so nothing is at rest.
pub fn sim_with_vehicles(n: usize) -> Sim {
    let mode = wp_contracts::load_mode_table(data_root().join("modes/sim.toml")).expect("mode");
    let mut sim = Sim::new(SimConfig {
        seed: 42,
        mode,
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 10.0,
    });
    let cars = [car("debug_sedan"), car("debug_coupe")];
    for i in 0..n {
        let pos = [(i % 8) as f32 * 10.0, (i / 8) as f32 * 10.0, 0.0];
        let id = sim
            .spawn_vehicle(&cars[i % 2], pos, 0.0)
            .expect("under MAX_VEHICLES");
        sim.set_assists(id, AssistProfile::ai()).expect("exists");
        let input = VehicleInput {
            throttle: 0.3 + 0.7 * ((i % 5) as f32 / 4.0),
            steer: ((i % 7) as f32 - 3.0) / 6.0,
            ..Default::default()
        };
        sim.set_input(id, input).expect("exists");
    }
    for _ in 0..240 {
        sim.step_fixed();
    }
    sim.drain_events();
    sim
}

/// A 60 Hz snapshot of every vehicle in `sim`, as the server would send it.
pub fn snapshot(sim: &Sim) -> ServerToClient {
    ServerToClient::Snapshot {
        tick: sim.tick() - sim.tick() % PHYSICS_TICKS_PER_NET_TICK as u64,
        vehicles: sim.vehicle_states(),
        events: Vec::new(),
    }
}

/// Synthetic frontal contacts for the lattice bench: `n` impulses spread across the bumper.
pub fn frontal_contacts(n: usize) -> Vec<wp_contracts::ContactImpulse> {
    (0..n)
        .map(|i| wp_contracts::ContactImpulse {
            point_local_m: [-0.8 + 1.6 * i as f32 / n.max(1) as f32, 2.3, 0.4],
            normal_local: [0.0, -1.0, 0.0],
            impulse_ns: 900.0,
            tangent_impulse_local_ns: [0.0; 3],
            rel_speed_mps: 15.6,
            other: Some(EntityId(2)),
            other_mass_kg: 1400.0,
        })
        .collect()
}
