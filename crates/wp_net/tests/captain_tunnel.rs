//! Backlog B04 on the wire: the captain map loses a racer in a tunnel. A procedural night city
//! gets one covered span over the southbound lane of the x ≈ 55 m collector; a racer drives
//! south into it. While it is outside, the captain map shows it where the server has it; once
//! it is under cover the map keeps the last position it showed (where the racer went in) even
//! though the racer keeps driving.

mod common;

use common::Harness;
use wp_contracts::{DamageMode, Role, TimeOfDay};
use wp_net::client::default_damage_factory;
use wp_net::netsim::{server_world_config, CitySetup, NetSim, WpSim};
use wp_net::server::ServerConfig;
use wp_net::sim_link::LinkConfig;
use wp_net::testkit::{load_data, repo_data_dir};
use wp_sim::SimConfig;
use wp_world::network::{CoverKind, CoveredSpan};

/// Southbound lane of the x ≈ 55 m collector.
const LANE_X: f32 = 53.25;
/// The covered stretch of that lane, y from → to.
const SPAN_Y: [f32; 2] = [360.0, 150.0];

fn link(seed: u64) -> LinkConfig {
    LinkConfig {
        latency_ms: 30.0,
        jitter_ms: 5.0,
        loss: 0.0,
        retransmit_ms: 100.0,
        seed,
    }
}

#[test]
fn captain_map_loses_a_racer_in_a_tunnel() {
    let (cars, modes) = load_data(&repo_data_dir()).expect("data");
    let config = SimConfig {
        seed: 32,
        mode: modes[&DamageMode::Arcade].clone(),
        time_of_day: TimeOfDay::from_hours(22.0),
        time_ratio: 1.0,
    };
    let pg = wp_world::procgen::ProcGenConfig {
        nx: 6,
        ny: 6,
        spacing_m: 110.0,
        seed: 1,
        ..Default::default()
    };
    let mut net = wp_world::procgen::generate(&pg);
    net.covered.push(CoveredSpan {
        name: "test tunnel".into(),
        kind: CoverKind::Tunnel,
        road: 0,
        centre: vec![[LANE_X, SPAN_Y[0], 0.0], [LANE_X, SPAN_Y[1], 0.0]],
        half_width_m: 6.0,
        clearance_m: 6.0,
    });
    net.validate().expect("valid with the span");
    let mut wcfg = server_world_config(&config);
    wcfg.traffic.enabled = false;
    let world = wp_world::World::new(net, wcfg);
    let city = CitySetup {
        world,
        police_car: None,
        traffic_car: None,
    };
    let sim = WpSim::with_city(config, cars.clone(), city);
    let cfg = ServerConfig {
        racer_car: "debug_coupe".into(),
        spawn_points: vec![([LANE_X, 420.0, 0.0], std::f32::consts::PI)],
        ..ServerConfig::default()
    };
    let mut h = Harness::with_sim(cfg, sim, cars, modes, default_damage_factory);
    h.add_peer("racer", link(1), link(2));
    h.add_peer("captain", link(3), link(4));
    h.wait_joined();
    h.request_role(0, Role::Racer);
    h.request_role(1, Role::PoliceCaptain);
    h.run_secs(1.0);
    assert_eq!(h.peers[1].client.role(), Role::PoliceCaptain);
    let racer = h.peers[0].client.own_vehicle().expect("racer car");
    h.peers[0].manual_input.throttle = 0.35;

    let server_y = |h: &Harness<WpSim>| {
        h.server
            .sim()
            .vehicles()
            .into_iter()
            .find(|(e, _)| *e == racer)
            .map(|(_, s)| s.pos[1])
            .expect("racer on the server")
    };
    let map_y = |h: &Harness<WpSim>| {
        h.peers[1]
            .client
            .captain
            .units
            .get(&racer.0)
            .map(|u| u.y_dm as f32 / 10.0)
    };
    // Drive until the racer is 60 m into the tunnel.
    let (mut seen_outside, mut frozen_at) = (false, None);
    for _ in 0..60 * 40 {
        h.step();
        let y = server_y(&h);
        if y > SPAN_Y[0] + 5.0 {
            // Outside: the map follows the racer (within a couple of metres of latency).
            if let Some(m) = map_y(&h) {
                seen_outside |= (m - y).abs() < 3.0;
            }
        } else if y < SPAN_Y[0] - 20.0 && frozen_at.is_none() {
            frozen_at = map_y(&h);
        }
        if y < SPAN_Y[0] - 60.0 {
            break;
        }
    }
    assert!(seen_outside, "the captain saw the racer before the tunnel");
    let frozen = frozen_at.expect("the racer stays on the map at its last position");
    assert!(
        frozen > SPAN_Y[0] - 6.0 && frozen < SPAN_Y[0] + 8.0,
        "frozen at the portal: {frozen}"
    );
    let now = server_y(&h);
    assert!(now < SPAN_Y[0] - 55.0, "racer drove on inside: {now}");
    assert_eq!(map_y(&h), Some(frozen), "map does not follow it inside");
}
