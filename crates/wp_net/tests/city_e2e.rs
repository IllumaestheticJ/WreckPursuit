//! End-to-end on the **real** simulation: a headless server running `wp_sim` in a procedural
//! city at night with physical traffic and AI police cars, and 4 bot clients (3 racers, 1
//! police seat) on realistic links for 60 s. Clients replicate damage with the real
//! `wp_damage::DamageModel`.
//!
//! Asserts:
//! - own-car prediction corrections stay within the chase correction budget (RMS < 0.5 m;
//!   rams are not predicted, see NET-N11) and the predictor never resets;
//! - replicas built from replicated contacts never mismatch the server's lattice; overall
//!   mismatches (all on replicas restored from a dented snapshot, which carries node
//!   positions only — NET-N5) stay under 5 % and are resynced;
//! - every client that holds a state for an AI police unit holds exactly the server's
//!   quantised state for that tick (all clients see the same police units), and every police
//!   car near a client is known to it with the right kind;
//! - every client's down-link stays under the 40 kB/s budget.
//!
//! The racers start wanted (heat 55, level 3), so AI police pursue them; bots jink across the
//! lanes of a busy collector, so cars hit traffic, police and each other. Prints the numbers
//! recorded in `crates/wp_net/README.md` (`cargo test -p wp_net --release --test city_e2e --
//! --nocapture`).

mod common;

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use common::Harness;
use wp_contracts::{DamageMode, DamageSim, EntityId, Role, TimeOfDay};
use wp_net::bot::BotDriver;
use wp_net::client::default_damage_factory;
use wp_net::netsim::{NetSim, WpSim};
use wp_net::protocol::EntityKind;
use wp_net::quant::QVehicle;
use wp_net::server::ServerConfig;
use wp_net::sim_link::LinkConfig;
use wp_net::testkit::{load_data, repo_data_dir};
use wp_net::DOWNLINK_BUDGET_BPS;
use wp_sim::{Sim, SimConfig};

const POLICE_CAR: &str = "sentinel_cruiser";
const TRAFFIC_CAR: &str = "debug_sedan";
/// Southbound lane of the x ≈ 55 m collector (same street as simcli's `golden_city_chase`).
const LANE_X: f32 = 53.25;

fn link(seed: u64, latency_ms: f64) -> LinkConfig {
    LinkConfig {
        latency_ms,
        jitter_ms: 10.0,
        loss: 0.01,
        retransmit_ms: 120.0,
        seed,
    }
}

fn city_sim(
    seed: u64,
) -> (
    WpSim,
    BTreeMap<String, wp_contracts::LoadedCar>,
    BTreeMap<DamageMode, wp_contracts::ModeTable>,
) {
    let (mut cars, modes) = load_data(&repo_data_dir()).expect("data");
    let police_car =
        wp_contracts::load_car(repo_data_dir().join(format!("cars/{POLICE_CAR}.json")))
            .expect("police car");
    cars.insert(POLICE_CAR.to_string(), police_car.clone());
    let mut sim = Sim::new(SimConfig {
        seed,
        mode: modes[&DamageMode::Arcade].clone(),
        time_of_day: TimeOfDay::from_hours(23.0),
        time_ratio: 1.0,
    });
    let pg = wp_world::procgen::ProcGenConfig {
        nx: 8,
        ny: 8,
        spacing_m: 110.0,
        seed: 1,
        ..Default::default()
    };
    let world = wp_world::World::procedural(
        &pg,
        wp_world::WorldConfig {
            seed,
            start_time: TimeOfDay::from_hours(23.0),
            time_ratio: 1.0,
            traffic: wp_world::TrafficConfig {
                enabled: true,
                detect_collisions: true,
                ..Default::default()
            },
            // Thousands of parked-car bodies make debug runs very slow and add nothing here.
            parked_cars: false,
        },
    );
    // `set_world` builds `wp_police` on the city's roads (built-in patrol tables).
    sim.set_world(world);
    sim.set_police_car(police_car);
    sim.set_traffic_car(cars[TRAFFIC_CAR].clone());
    (WpSim::from_sim(sim, cars.clone()), cars, modes)
}

/// Quantised states of every AI police car, exactly as the server sends them.
fn police_states(sim: &WpSim) -> BTreeMap<u32, QVehicle> {
    let police: BTreeSet<EntityId> = sim
        .sim()
        .vehicles()
        .iter()
        .filter(|v| v.police_unit.is_some())
        .map(|v| v.id)
        .collect();
    sim.vehicles()
        .into_iter()
        .filter(|(e, _)| police.contains(e))
        .map(|(e, s)| (e.0, QVehicle::from_state(&s)))
        .collect()
}

#[test]
fn four_players_in_a_police_city() {
    let secs: u64 = 60;
    let (sim, cars, modes) = city_sim(108);
    let spawns: Vec<([f32; 3], f32)> = (0..4)
        .map(|i| ([LANE_X, 420.0 - 18.0 * i as f32, 0.0], std::f32::consts::PI))
        .collect();
    let cfg = ServerConfig {
        racer_car: "debug_coupe".into(),
        police_car: POLICE_CAR.into(),
        spawn_points: spawns,
        ..ServerConfig::default()
    };
    let mut h = Harness::with_sim(cfg, sim, cars, modes, default_damage_factory);
    for (i, lat) in [20.0, 40.0, 60.0, 35.0].into_iter().enumerate() {
        let i = i as u64;
        h.add_peer(&format!("p{i}"), link(i, lat), link(100 + i, lat));
    }
    h.wait_joined();
    for i in 0..4 {
        let role = if i == 3 {
            Role::PoliceChase
        } else {
            Role::Racer
        };
        h.request_role(i, role);
        // Up and down the collector (where traffic and the first patrol are), with a jink
        // across the lanes now and then.
        let x = LANE_X + if i % 2 == 0 { 0.0 } else { 3.5 };
        let route = vec![[x, 120.0], [x + 3.0, 60.0], [x, 300.0], [x - 3.0, 520.0]];
        let mut bot = BotDriver::new(700 + i as u64, route);
        bot.crash_chance_per_s = 0.12;
        h.peers[i].bot = Some(bot);
    }
    h.run_secs(1.0);
    assert_eq!(h.server.roles().police_count(), 1);

    // The racers are already wanted (L3 heat), so AI police pursue and ram them: this is the
    // greybox chase loop, and the source of real car-to-car contacts for damage replication.
    let racers: Vec<EntityId> = h.peers[..3]
        .iter()
        .map(|p| p.client.own_vehicle().expect("racer car"))
        .collect();
    let mut police = wp_police::PoliceSystem::for_world(
        wp_police::PoliceConfig::builtin(),
        h.server.sim().sim().world(),
        wp_police::PoliceSetup {
            seed: 108,
            mode: DamageMode::Arcade,
            internal_driving: true,
            hazards: true,
        },
    );
    for r in &racers {
        police.set_heat(*r, 55.0);
    }
    h.server.sim_mut().sim_mut().set_police(police);

    // Server police states by tick, for comparing what the clients hold.
    let mut history: VecDeque<(u64, BTreeMap<u32, QVehicle>)> = VecDeque::new();
    let mut compared = 0u64;
    let mut differing = 0u64;
    let mut seen_police: BTreeSet<u32> = BTreeSet::new();
    let mut max_police = 0usize;
    let mut max_traffic = 0usize;
    let mut max_wanted = 0u8;
    let mut near_missing = 0u64;
    let mut near_checks = 0u64;
    for step in 0..secs * 60 {
        h.step();
        let tick = h.server.sim().tick();
        let police = police_states(h.server.sim());
        max_police = max_police.max(police.len());
        max_traffic = max_traffic.max(
            h.server
                .sim()
                .sim()
                .vehicles()
                .iter()
                .filter(|v| v.traffic_agent.is_some())
                .count(),
        );
        history.push_back((tick, police));
        while history.len() > 240 {
            history.pop_front();
        }
        if step % 30 != 29 {
            continue;
        }
        let server_states: BTreeMap<EntityId, wp_contracts::VehicleState> =
            h.server.sim().vehicles().into_iter().collect();
        for p in &h.peers {
            let own = p.client.own_vehicle();
            if let Some(v) = own {
                max_wanted = max_wanted.max(h.server.sim().wanted_level(v));
            }
            for (e, r) in p.client.remotes() {
                let Some((t, q)) = r.buffer.latest() else {
                    continue;
                };
                let Some((_, at)) = history.iter().find(|(ht, _)| *ht == t) else {
                    continue;
                };
                let Some(sq) = at.get(e) else { continue };
                assert_eq!(r.kind, EntityKind::AiPolice, "unit {e} kind");
                seen_police.insert(*e);
                compared += 1;
                if sq != q {
                    differing += 1;
                }
            }
            // Police cars well inside the near radius (60 m, half of NEAR_RADIUS_M) for a
            // second must be known to the client.
            let Some(own_state) = own.and_then(|v| server_states.get(&v)) else {
                continue;
            };
            let latest = &history.back().expect("history").1;
            let old = &history[history.len().saturating_sub(60)].1;
            for e in latest.keys().filter(|e| old.contains_key(e)) {
                let s = &server_states[&EntityId(*e)];
                let d2: f32 = (0..2).map(|k| (s.pos[k] - own_state.pos[k]).powi(2)).sum();
                if d2 < 60.0 * 60.0 {
                    near_checks += 1;
                    if !p.client.remotes().contains_key(e) {
                        near_missing += 1;
                    }
                }
            }
        }
    }

    // Damage reached the clients: dented cars on the server, dented replicas on clients.
    let server_dented = h
        .server
        .sim()
        .sim()
        .vehicles()
        .iter()
        .filter(|v| {
            v.damage
                .node_positions()
                .iter()
                .zip(&v.damage.lattice().nodes)
                .any(|(a, b)| *a != b.pos_m)
        })
        .count();
    let replica_dented: usize = h
        .peers
        .iter()
        .map(|p| {
            p.client
                .remotes()
                .keys()
                .filter_map(|e| p.client.replica_nodes(*e).map(|n| (*e, n)))
                .filter(|(e, n)| {
                    let rest = h.server.sim().lattice_rest(EntityId(*e));
                    rest.is_some_and(|r| n.iter().zip(&r).any(|(a, b)| a != b))
                })
                .count()
        })
        .sum();

    println!("bytes by message tag: {:x?}", h.server.stats().bytes_by_tag);
    let mut total_hash = 0;
    let mut total_mismatch = 0;
    for (i, p) in h.peers.iter().enumerate() {
        let s = p.client.stats();
        let down: Vec<u64> = p.down_per_second.iter().skip(2).copied().collect();
        let mean = down.iter().sum::<u64>() as f64 / down.len().max(1) as f64;
        let worst = down.iter().copied().max().unwrap_or(0);
        println!(
            "p{i} {:?}: rtt {:.0} ms, reconciles {}, mispredictions {}, rms {:.3} m, max {:.2} m, \
             resets {}; remotes {}; hash {}/{} mismatched, resyncs {}, max resync {} ticks; \
             down mean {mean:.0} B/s worst {worst} B/s",
            p.client.role(),
            s.rtt_ms,
            s.reconciles,
            s.mispredictions,
            s.correction_rms_m(),
            s.max_correction_m,
            s.prediction_resets,
            p.client.remotes().len(),
            s.hash_mismatches,
            s.hash_checks,
            s.resyncs,
            s.max_resync_ticks,
        );
        total_hash += s.hash_checks;
        total_mismatch += s.hash_mismatches;
        assert_eq!(
            s.hash_mismatches_contact_only, 0,
            "p{i}: a replica built only from contacts diverged"
        );
        assert_eq!(s.decode_errors, 0);
        // Chase budget: 0.5 m RMS. Police rams and traffic hits are not predicted (the client
        // simulates only its own car; NET-N11), so corrections are larger than the 0.3 m of
        // contact-free driving (`tests/prediction.rs`).
        assert!(
            s.correction_rms_m() < 0.5,
            "p{i} rms correction {}",
            s.correction_rms_m()
        );
        assert_eq!(s.prediction_resets, 0, "p{i} prediction reset");
        assert!(
            worst as usize <= DOWNLINK_BUDGET_BPS,
            "p{i} worst second {worst} B/s"
        );
        assert!(p.client.pending_resyncs().len() <= 2, "p{i} stuck resyncs");
    }
    let stats = h.server.stats();
    println!(
        "server: police cars max {max_police}, physical traffic max {max_traffic}, max wanted \
         {max_wanted}, dented cars {server_dented} (dented replicas on clients \
         {replica_dented}); police states compared {compared} ({differing} differ), units seen {}; \
         near police known {}/{near_checks}; lattice hashes {total_mismatch}/{total_hash} \
         mismatched; downlink max {:.0} B/s",
        seen_police.len(),
        near_checks - near_missing,
        stats.downlink_bps_max,
    );
    assert!(max_police >= 1, "no AI police cars spawned");
    assert!(max_traffic >= 1, "no physical traffic");
    assert!(max_wanted >= 2, "racers not wanted");
    assert!(
        server_dented >= 1 && replica_dented >= 1,
        "no damage replicated"
    );
    assert!(compared > 0, "no client ever held a police unit's state");
    assert_eq!(
        differing, 0,
        "clients disagree with the server on police states"
    );
    assert_eq!(
        near_missing, 0,
        "police cars near a client were not replicated"
    );
    assert!(total_hash > 0, "no lattice hash checks");
    assert!(
        (total_mismatch as f64) < 0.05 * total_hash as f64,
        "{total_mismatch}/{total_hash} lattice hash mismatches"
    );
    assert!(stats.downlink_bps_max <= DOWNLINK_BUDGET_BPS as f64);
}
