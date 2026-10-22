//! # wp_bench
//!
//! Shared setup for the criterion benches in `benches/sim.rs` and the perf gate
//! (`src/bin/perf_gate.rs`, see [`gate`]). Benches measure one physics tick (or one snapshot
//! encode/decode); the gate converts them to per-frame numbers (60 Hz = 4 ticks) and compares
//! them against blessed baselines per runner class.

#![warn(missing_docs)]

pub mod gate;

use std::path::PathBuf;

use std::collections::BTreeMap;

use wp_contracts::{
    AssistProfile, EntityId, LoadedCar, TimeOfDay, VehicleInput, VehicleState,
    PHYSICS_TICKS_PER_NET_TICK,
};
use wp_net::protocol::ServerMsg;
use wp_net::quant::QVehicle;
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
    let mode = wp_contracts::load_mode_table(data_root().join("modes/extreme.toml")).expect("mode");
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

/// Two consecutive 60 Hz snapshots of every vehicle in `sim` on `wp_net`'s wire codec, as the
/// server sends them: `(baseline, sender state after the baseline, delta snapshot)`. The
/// delta snapshot is quantised and delta-encoded against the baseline one net tick earlier.
pub fn snapshot_pair(sim: &mut Sim) -> SnapshotPair {
    let mut sent = BTreeMap::new();
    let t0 = sim.tick() - sim.tick() % PHYSICS_TICKS_PER_NET_TICK as u64;
    let base_msg = simcli::net::build_snapshot(t0, &sim.vehicle_states(), &mut sent, true);
    let after_base = sent.clone();
    for _ in 0..PHYSICS_TICKS_PER_NET_TICK {
        sim.step_fixed();
    }
    sim.drain_events();
    let states = sim.vehicle_states();
    let t1 = t0 + PHYSICS_TICKS_PER_NET_TICK as u64;
    let delta_msg = simcli::net::build_snapshot(t1, &states, &mut sent, false);
    SnapshotPair {
        tick: t1,
        states,
        sender_baseline: after_base,
        client_baseline: match &base_msg {
            ServerMsg::Snapshot { entities, .. } => entities
                .iter()
                .map(|d| (d.entity, d.apply(None).expect("absolute entry")))
                .collect(),
            _ => unreachable!("build_snapshot returns a snapshot"),
        },
        delta_bytes: delta_msg.encode(),
    }
}

/// Inputs for the snapshot benches (see [`snapshot_pair`]).
pub struct SnapshotPair {
    /// Tick of the delta snapshot.
    pub tick: u64,
    /// Vehicle states at that tick.
    pub states: Vec<(EntityId, VehicleState)>,
    /// Server's per-entity baseline (what it sent one net tick earlier).
    pub sender_baseline: BTreeMap<u32, (u64, QVehicle)>,
    /// Client's reconstructed copy of the baseline.
    pub client_baseline: BTreeMap<u32, QVehicle>,
    /// The encoded delta snapshot.
    pub delta_bytes: Vec<u8>,
}

/// Decode a snapshot and re-apply its deltas against the client's baseline (what a client does
/// on receipt). Returns the reconstructed entities.
pub fn decode_snapshot(bytes: &[u8], client: &BTreeMap<u32, QVehicle>) -> Vec<QVehicle> {
    match ServerMsg::decode(bytes).expect("decodes") {
        ServerMsg::Snapshot { entities, .. } => entities
            .iter()
            .map(|d| d.apply(client.get(&d.entity)).expect("applies"))
            .collect(),
        _ => panic!("not a snapshot"),
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

#[cfg(test)]
mod tests {
    #[test]
    fn snapshot_pair_decodes_to_the_sent_state() {
        let mut sim = super::sim_with_vehicles(8);
        let pair = super::snapshot_pair(&mut sim);
        let got = super::decode_snapshot(&pair.delta_bytes, &pair.client_baseline);
        let want: Vec<_> = pair
            .states
            .iter()
            .map(|(_, s)| wp_net::quant::QVehicle::from_state(s))
            .collect();
        assert_eq!(got, want);
        assert!(
            pair.delta_bytes.len() < 8 * 40,
            "{} bytes",
            pair.delta_bytes.len()
        );
    }
}
