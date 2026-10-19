//! Deterministic damage replication: server + 4 headless clients in-process, 2 minutes of
//! scripted driving and crashing in the crash arena. Every client's replicated lattice hashes
//! must match the server's except transiently (< 1 % of checks), and every mismatch must be
//! resolved by a lattice snapshot within 2 s.

mod common;

use common::Harness;
use wp_contracts::{EntityId, Role, VehicleInput};
use wp_net::lattice::lattice_hash;
use wp_net::netsim::NetSim;
use wp_net::server::ServerConfig;
use wp_net::sim_link::LinkConfig;

#[test]
fn four_clients_replicate_damage_deterministically() {
    let mut h = Harness::new(ServerConfig::default(), 45.0);
    for i in 0..4u64 {
        let link = LinkConfig {
            latency_ms: 25.0,
            jitter_ms: 5.0,
            loss: 0.0,
            retransmit_ms: 0.0,
            seed: 10 + i,
        };
        h.add_peer(
            &format!("c{i}"),
            link,
            LinkConfig {
                seed: 20 + i,
                ..link
            },
        );
    }
    h.wait_joined();
    for i in 0..4 {
        h.request_role(
            i,
            if i == 3 {
                Role::PoliceChase
            } else {
                Role::Racer
            },
        );
        h.give_bot(i, 100 + i as u64, 40.0);
    }

    // Two minutes of driving. Halfway through, corrupt one client's replica of its own car to
    // prove a desync is detected and repaired.
    for step in 0..120 * 60 {
        h.step();
        if step == 60 * 60 {
            let own = h.peers[0].client.own_vehicle().expect("peer 0 drives").0;
            h.peers[0].client.corrupt_replica(own, 0, [0.0, 0.05, 0.0]);
        }
    }

    // Quiesce: stop everyone so no new contacts happen, let the last events and hashes land.
    for p in &mut h.peers {
        p.bot = None;
        p.manual_input = VehicleInput {
            brake: 1.0,
            ..Default::default()
        };
    }
    h.run_secs(4.0);

    let sim = h.server.sim();
    let dented = sim
        .vehicles()
        .iter()
        .filter(|(e, _)| {
            let nodes = sim.lattice_nodes(*e).unwrap();
            let rest = sim.lattice_rest(*e).unwrap();
            lattice_hash(&nodes, &rest) != lattice_hash(&rest, &rest)
        })
        .count();
    assert!(
        dented >= 2,
        "the arena must actually crash cars ({dented} dented)"
    );

    let (mut checks, mut mismatches) = (0, 0);
    for (i, p) in h.peers.iter().enumerate() {
        let s = p.client.stats();
        println!(
            "client {i}: checks {} mismatches {} resyncs {} max_resync {} ticks, replicas {}",
            s.hash_checks,
            s.hash_mismatches,
            s.resyncs,
            s.max_resync_ticks,
            p.client.remotes().len() + 1
        );
        checks += s.hash_checks;
        mismatches += s.hash_mismatches;
        assert!(
            s.max_resync_ticks <= 2 * 240,
            "resync took {} ticks",
            s.max_resync_ticks
        );
        assert!(p.client.pending_resyncs().is_empty());
        // Final state: every replica equals the server's lattice.
        let mut ids: Vec<u32> = p.client.remotes().keys().copied().collect();
        ids.extend(p.client.own_vehicle().map(|e| e.0));
        for e in ids {
            let nodes = sim.lattice_nodes(EntityId(e)).unwrap();
            let rest = sim.lattice_rest(EntityId(e)).unwrap();
            let (_, client_hash) = p.client.replica_hash(e).expect("replica exists");
            assert_eq!(
                client_hash,
                lattice_hash(&nodes, &rest),
                "client {i} vehicle {e}"
            );
        }
    }
    println!("total checks {checks}, mismatches {mismatches}");
    assert!(checks > 400, "hash checks ran ({checks})");
    assert!(mismatches >= 1, "the injected desync was detected");
    assert!(
        (mismatches as f64) < 0.01 * checks as f64,
        "{mismatches} of {checks} hash checks mismatched"
    );
    assert!(h.server.stats().lattice_resyncs >= 1);
}
