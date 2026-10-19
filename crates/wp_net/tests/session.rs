//! Join/leave/reconnect, anti-cheat, roles + captain over the wire, and a full race.

mod common;

use common::Harness;
use wp_contracts::{
    AccountId, CaptainCommand, CaptainError, EconomyReason, EntityId, PlayerId, RaceCommand,
    RaceEventId, Role, SimEvent, UnitId, UnitOrder, VehicleInput,
};
use wp_economy::{AccountStore, PlayerAccount};
use wp_net::client::Phase;
use wp_net::netsim::NetSim;
use wp_net::protocol::{ClientMsg, ServerMsg, PLAYER_UNIT_BASE, PROTOCOL_VERSION};
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
        join_token: String::new(),
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
        join_token: String::new(),
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
        let mut acct = PlayerAccount::empty(PlayerId(id));
        acct.cop_rank = rank;
        h.server.accounts_mut().commit(&acct, &[]).unwrap();
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
    // Typed orders reach the chase unit (addressed by its map unit id); a racer cannot
    // command; every command gets a reply with its sequence number.
    let unit = h.peers[1].client.own_vehicle().unwrap().0;
    let suspect = h.peers[3].client.own_vehicle().unwrap();
    let map_unit = h.peers[0].client.captain.units[&unit].unit;
    assert_eq!(map_unit, Some(PLAYER_UNIT_BASE + unit));
    let order = CaptainCommand::AssignUnit {
        unit: UnitId(PLAYER_UNIT_BASE + unit),
        order: UnitOrder::Pursue { target: suspect },
    };
    let now = h.now_ms;
    let ok_seq = h.peers[0].client.captain_command(order, now);
    let bad_unit = CaptainCommand::AssignUnit {
        unit: UnitId(PLAYER_UNIT_BASE + suspect.0),
        order: UnitOrder::Patrol,
    };
    let bad_seq = h.peers[0].client.captain_command(bad_unit, now);
    let bad_target = CaptainCommand::AssignUnit {
        unit: UnitId(PLAYER_UNIT_BASE + unit),
        order: UnitOrder::Intercept {
            target: EntityId(9999),
        },
    };
    let target_seq = h.peers[0].client.captain_command(bad_target, now);
    // Tools go to `wp_police`; the arena test sim has none.
    let tool = CaptainCommand::RequestHelicopter { target: suspect };
    let tool_seq = h.peers[0].client.captain_command(tool, now);
    let racer_seq = h.peers[3].client.captain_command(order, now);
    h.run_secs(0.5);
    let o = h.peers[1].client.unit_order.expect("order delivered");
    assert_eq!(o, UnitOrder::Pursue { target: suspect });
    let replies: Vec<_> = h.peers[0].client.captain_replies.iter().copied().collect();
    assert_eq!(
        replies,
        vec![
            (ok_seq, Ok(())),
            (bad_seq, Err(CaptainError::UnknownUnit)),
            (target_seq, Err(CaptainError::UnknownTarget)),
            (tool_seq, Err(CaptainError::Unavailable)),
        ]
    );
    assert_eq!(
        h.peers[3].client.captain_replies.back(),
        Some(&(racer_seq, Err(CaptainError::Unavailable)))
    );
    assert!(h.peers[3]
        .client
        .notices
        .iter()
        .any(|n| n.contains("only the captain")));
    h.run_secs(0.6); // the captain view refreshes at 2 Hz
    assert_eq!(h.peers[0].client.captain.units[&unit].order, 2, "pursue");

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
    let event = RaceEventId(race);
    h.peers[1]
        .client
        .race_command(RaceCommand::Enter { event }, now);
    h.run_secs(0.5);
    // Only the host's Ready starts the countdown.
    let now = h.now_ms;
    h.peers[1].client.race_command(RaceCommand::Ready, now);
    h.run_secs(0.5);
    assert!(h.peers[1].client.notices.iter().any(|n| n.contains("host")));
    let now = h.now_ms;
    h.peers[0].client.race_command(RaceCommand::Ready, now);
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

    // Every cash change arrived as a typed economy event for its own account only, with a
    // gap-free sequence that ends at the balance shown.
    for (k, peer) in h.peers.iter().enumerate() {
        let me = AccountId(peer.client.player().unwrap().0);
        let econ: Vec<_> = peer
            .client
            .game_events
            .iter()
            .filter_map(|e| match e {
                SimEvent::Economy(ev) => Some(ev.clone()),
                _ => None,
            })
            .collect();
        assert!(econ.iter().all(|e| e.account == me), "peer {k}: {econ:?}");
        assert!(matches!(econ[0].reason, EconomyReason::StakeCharged { .. }));
        for (i, e) in econ.iter().enumerate() {
            assert_eq!(e.seq, i as u64);
        }
        assert_eq!(econ.last().unwrap().cash_after, peer.client.account.0);
        if k == 0 {
            assert!(econ
                .iter()
                .any(|e| matches!(e.reason, EconomyReason::RacePayout { placement: 1, .. })));
            assert!(econ
                .iter()
                .any(|e| matches!(e.reason, EconomyReason::RaceRep { .. }) && e.rep_delta == 10));
        }
    }
    // Withdrawing from a finished race is a no-op, not an error.
    let now = h.now_ms;
    h.peers[1]
        .client
        .race_command(RaceCommand::Withdraw { event }, now);
    h.run_secs(0.2);
}

#[test]
fn join_token_gate_parks_hellos_until_the_host_decides() {
    let cfg = ServerConfig {
        require_join_token: true,
        ..Default::default()
    };
    let mut h = Harness::new(cfg, 5000.0);
    h.add_peer_with_join_token("good", "mm-ok", ideal(), ideal());
    h.add_peer_with_join_token("bad", "mm-forged", ideal(), ideal());
    h.run_secs(0.5);
    assert_eq!(h.server.stats().players, 0, "nobody joins unverified");
    let requests = h.server.take_join_requests();
    assert_eq!(
        requests
            .iter()
            .map(|r| (r.name.as_str(), r.join_token.as_str()))
            .collect::<Vec<_>>(),
        vec![("good", "mm-ok"), ("bad", "mm-forged")]
    );
    assert!(h.server.take_join_requests().is_empty(), "handed out once");
    let pid = h
        .server
        .approve_join(
            requests[0].conn,
            Some("Verified".into()),
            "meta-rt-1".into(),
        )
        .expect("joined");
    h.server.reject_join(requests[1].conn, "invalid join token");
    h.run_secs(0.5);
    let good = &h.peers[0].client;
    assert_eq!(*good.phase(), Phase::Joined);
    assert_eq!(good.player(), Some(pid));
    assert_eq!(good.meta_reconnect_token, "meta-rt-1");
    assert_eq!(h.server.player_names(), vec![(pid, "Verified".to_string())]);
    assert!(matches!(h.peers[1].client.phase(), Phase::Rejected(r) if r.contains("join token")));
    // The in-window reattach (u64 token) needs no new verification.
    let token = h.peers[0].client.token();
    h.drop_link(0);
    h.run_secs(1.0);
    h.connect(0, token);
    h.run_secs(0.5);
    assert_eq!(h.peers[0].client.player(), Some(pid));
    assert!(h.server.take_join_requests().is_empty());
}
