//! Join/leave/reconnect, anti-cheat, roles + captain over the wire, and a full race.

mod common;

use common::Harness;
use wp_contracts::{PlayerId, Role, VehicleInput};
use wp_economy::{AccountStore, PlayerAccount};
use wp_net::client::Phase;
use wp_net::netsim::NetSim;
use wp_net::protocol::{CaptainOrder, ClientMsg, ServerMsg, PROTOCOL_VERSION};
use wp_net::quant::QInput;
use wp_net::server::ServerConfig;
use wp_net::sim_link::LinkConfig;

fn ideal() -> LinkConfig {
    LinkConfig::ideal(20.0)
}

#[test]
fn reconnect_within_window_keeps_car_and_expiry_removes_it() {
    let mut h = Harness::new(ServerConfig::default(), 5000.0);
    h.add_peer("a", ideal(), ideal());
    h.wait_joined();
    h.request_role(0, Role::Racer);
    h.peers[0].manual_input = VehicleInput {
        throttle: 1.0,
        ..Default::default()
    };
    h.run_secs(2.0);
    let player = h.peers[0].client.player().unwrap();
    let car = h.peers[0].client.own_vehicle().unwrap();
    let token = h.peers[0].client.token();
    assert!(h.server.sim().vehicles().iter().any(|(e, _)| *e == car));

    h.drop_link(0);
    h.run_secs(10.0);
    assert!(
        h.server.sim().vehicles().iter().any(|(e, _)| *e == car),
        "car stays in-world during the reconnect window"
    );
    h.connect(0, token);
    h.run_secs(1.0);
    let c = &h.peers[0].client;
    assert_eq!(*c.phase(), Phase::Joined);
    assert_eq!(c.player(), Some(player));
    assert_eq!(c.own_vehicle(), Some(car));
    assert_eq!(c.role(), Role::Racer);
    assert!(c.predicted_tick().is_some(), "prediction resumed");

    h.drop_link(0);
    h.run_secs(31.0);
    assert_eq!(h.server.stats().players, 0);
    assert!(!h.server.sim().vehicles().iter().any(|(e, _)| *e == car));
    // A stale token now joins as a new player.
    h.connect(0, token);
    h.run_secs(0.5);
    assert_ne!(h.peers[0].client.player(), Some(player));
}

#[test]
fn bye_removes_immediately_and_full_instance_rejects() {
    let cfg = ServerConfig {
        max_players: 2,
        ..Default::default()
    };
    let mut h = Harness::new(cfg, 5000.0);
    h.add_peer("a", ideal(), ideal());
    h.add_peer("b", ideal(), ideal());
    h.wait_joined();
    h.add_peer("c", ideal(), ideal());
    h.run_secs(0.5);
    assert!(matches!(h.peers[2].client.phase(), Phase::Rejected(r) if r.contains("full")));
    let now = h.now_ms;
    h.peers[0].client.bye(now);
    h.run_secs(0.5);
    assert_eq!(h.server.stats().players, 1);
}

#[test]
fn version_mismatch_is_rejected() {
    let mut h = Harness::new(ServerConfig::default(), 5000.0);
    let conn = h.server.connect();
    let hello = ClientMsg::Hello {
        protocol: PROTOCOL_VERSION + 1,
        contract: "0.0.0".into(),
        name: "old".into(),
        input_source: Default::default(),
        reconnect_token: 0,
    };
    h.server.receive(conn, &hello.encode());
    let out = h.server.drain_outgoing();
    assert!(out[0].close);
    assert!(matches!(
        ServerMsg::decode(&out[0].bytes).unwrap(),
        ServerMsg::Reject { .. }
    ));
}

#[test]
fn impossible_input_timing_is_rejected() {
    let cfg = ServerConfig {
        max_inputs_per_second: 100,
        ..Default::default()
    };
    let mut h = Harness::new(cfg, 5000.0);
    let conn = h.server.connect();
    let hello = ClientMsg::Hello {
        protocol: PROTOCOL_VERSION,
        contract: wp_contracts::CONTRACT_VERSION.into(),
        name: "cheater".into(),
        input_source: Default::default(),
        reconnect_token: 0,
    };
    h.server.receive(conn, &hello.encode());
    h.server
        .receive(conn, &ClientMsg::RoleRequest { role: Role::Racer }.encode());
    h.server.net_tick();
    let tick = h.server.sim().tick();
    let full = QInput::from_input(&VehicleInput {
        throttle: 1.0,
        ..Default::default()
    });
    // Two seconds in the future: impossible.
    let far = ClientMsg::Inputs {
        ack_snapshot_tick: 0,
        first_tick: tick + 480,
        inputs: vec![full; 8],
    };
    h.server.receive(conn, &far.encode());
    assert_eq!(h.server.stats().inputs_rejected, 8);
    // Flooding: 200 new ticks inside one second of server time, limit 100/s.
    for k in 0..(200 / 8) {
        let msg = ClientMsg::Inputs {
            ack_snapshot_tick: 0,
            first_tick: tick + k * 8,
            inputs: vec![full; 8],
        };
        h.server.receive(conn, &msg.encode());
    }
    let s = h.server.stats();
    assert_eq!(s.inputs_rejected, 8 + 100, "{s:?}");
    // Past ticks are ignored, not applied.
    let past = ClientMsg::Inputs {
        ack_snapshot_tick: 0,
        first_tick: tick.saturating_sub(10),
        inputs: vec![full; 5],
    };
    let before = h.server.stats().inputs_rejected;
    h.server.receive(conn, &past.encode());
    assert_eq!(h.server.stats().inputs_rejected, before);
    // Garbage does not crash the server.
    h.server.receive(conn, &[0x02, 0xff, 0xff]);
    assert_eq!(h.server.stats().decode_errors, 1);
}

#[test]
fn roles_captain_handoff_and_orders_over_the_wire() {
    let mut h = Harness::new(ServerConfig::default(), 5000.0);
    // Cop ranks for players 1..=5 (ids are assigned in join order).
    for (id, rank) in [(1, 5), (2, 50), (3, 30), (4, 0), (5, 0)] {
        h.server.accounts_mut().save(
            PlayerId(id),
            &PlayerAccount {
                cop_rank: rank,
                ..Default::default()
            },
        );
    }
    for i in 0..5 {
        h.add_peer(&format!("p{i}"), ideal(), ideal());
    }
    h.wait_joined();
    // 40 % of 5 = 2 police seats. Requests arrive in peer order.
    h.request_role(0, Role::PoliceCaptain); // rank 5, no chase units yet → granted
    h.request_role(1, Role::PoliceChase); // rank 50
    h.request_role(2, Role::PoliceChase); // third police request → racer
    h.request_role(3, Role::Racer);
    h.request_role(4, Role::Racer);
    h.run_secs(1.0);
    assert_eq!(h.peers[0].client.role(), Role::PoliceCaptain);
    assert_eq!(h.peers[1].client.role(), Role::PoliceChase);
    assert_eq!(h.peers[2].client.role(), Role::Racer, "police seats full");
    assert!(h.peers[2]
        .client
        .notices
        .iter()
        .any(|n| n.contains("police seats full")));

    // Captain sees the map (units + suspects) at 2 Hz and no 3D entities.
    h.run_secs(1.0);
    assert!(h.peers[0].client.captain.units.len() >= 4);
    assert!(h.peers[0].client.remotes().is_empty());
    // Orders reach the chase unit; a racer cannot command.
    let unit = h.peers[1].client.own_vehicle().unwrap().0;
    let order = CaptainOrder::MoveTo {
        units: vec![unit],
        x_dm: 1000,
        y_dm: 2000,
    };
    let now = h.now_ms;
    h.peers[0].client.captain_order(&order, now);
    h.peers[3].client.captain_order(&order, now);
    h.run_secs(0.5);
    let o = h.peers[1].client.unit_order.expect("order delivered");
    assert_eq!((o.kind, o.x_dm, o.y_dm), (1, 1000, 2000));
    assert!(h.peers[3]
        .client
        .notices
        .iter()
        .any(|n| n.contains("only the captain")));
    h.run_secs(0.6); // the captain view refreshes at 2 Hz
    assert_eq!(h.peers[0].client.captain.units[&unit].order, 1);

    // Captain leaves for good → seat offered to the highest-ranked chase unit, who accepts.
    let now = h.now_ms;
    h.peers[0].client.bye(now);
    h.run_secs(0.5);
    assert!(
        h.peers[1].client.captain_offer.is_some(),
        "offer to chase unit"
    );
    h.request_role(1, Role::PoliceCaptain);
    h.run_secs(0.5);
    assert_eq!(h.peers[1].client.role(), Role::PoliceCaptain);
    assert_eq!(
        h.server.roles().captain(),
        Some(h.peers[1].client.player().unwrap())
    );
    assert_eq!(
        h.peers[1].client.own_vehicle(),
        None,
        "captains do not drive"
    );
}

#[test]
fn race_from_lobby_to_payout() {
    let mut h = Harness::new(ServerConfig::default(), 5000.0);
    h.add_peer("fast", ideal(), ideal());
    h.add_peer("slow", ideal(), ideal());
    h.wait_joined();
    h.request_role(0, Role::Racer);
    h.request_role(1, Role::Racer);
    h.run_secs(0.5);
    // Spawn points: player 1 at x = -20, player 2 at x = -12; both face +Y.
    let now = h.now_ms;
    h.peers[0].client.send_reliable(
        &ClientMsg::RaceCreate {
            laps: 1,
            checkpoints: vec![[-160, 1000], [-160, 2500]],
        },
        now,
    );
    h.run_secs(0.5);
    let race = *h.peers[1]
        .client
        .races
        .keys()
        .next()
        .expect("race visible to all");
    let now = h.now_ms;
    h.peers[1]
        .client
        .send_reliable(&ClientMsg::RaceJoin { race }, now);
    h.run_secs(0.5);
    let now = h.now_ms;
    h.peers[0]
        .client
        .send_reliable(&ClientMsg::RaceStart { race }, now);
    h.run_secs(0.5);
    // Countdown (3 s) then go.
    h.run_secs(2.6);
    h.peers[0].manual_input = VehicleInput {
        throttle: 1.0,
        ..Default::default()
    };
    h.peers[1].manual_input = VehicleInput {
        throttle: 0.6,
        ..Default::default()
    };
    h.run_secs(40.0);
    let (_, results) = h.peers[0].client.race_results.clone().expect("results");
    let me = h.peers[0].client.player().unwrap().0;
    assert_eq!(results[0].player, me);
    assert_eq!(results[0].place, 1);
    assert_eq!(results[1].place, 2);
    let (cash_fast, ..) = h.peers[0].client.account;
    let (cash_slow, ..) = h.peers[1].client.account;
    assert!(cash_fast > 0 && cash_slow < 0, "{cash_fast} {cash_slow}");
    assert_eq!(cash_fast + cash_slow, 0, "the pot is fully paid out");
}
