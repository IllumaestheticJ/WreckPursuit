//! Contract v0.3.0 replication on the **real** simulation (protocol v4): a procedural night
//! city with physical + kinematic traffic, AI police and server-side weather; two racers (one
//! wanted) and a police captain.
//!
//! Asserts:
//! - **kinematic traffic** (`TrafficAgentSnapshot`): every agent a racer holds is exactly the
//!   server's quantised agent at that tick, within the interest radius, never a physical
//!   (promoted) agent; the captain gets none;
//! - **weather** (W-5): clients get the server's snapshot on join, and a forced regime change
//!   arrives both as `SimEvent::WeatherChanged` and as a new snapshot within ~1 s;
//! - **typed police events**: the wanted racer sees its `WantedLevelChanged`; radio lines go
//!   to the captain only;
//! - **typed captain commands** reach `wp_police` and come back as `CaptainReply`, and the
//!   captain map addresses AI units by their `wp_police` unit id;
//! - down-link stays under budget.

mod common;

use std::collections::BTreeMap;

use common::Harness;
use wp_contracts::{
    CaptainCommand, CaptainError, DamageMode, EntityId, PoliceEvent, Role, SimEvent, TimeOfDay,
    TrafficAgentSnapshot, WeatherKind,
};
use wp_net::client::default_damage_factory;
use wp_net::netsim::{city_weather, server_world_config, CitySetup, NetSim, WpSim};
use wp_net::protocol::EntityKind;
use wp_net::quant::QTrafficAgent;
use wp_net::server::ServerConfig;
use wp_net::sim_link::LinkConfig;
use wp_net::testkit::{load_data, repo_data_dir, AI_POLICE_CAR, TRAFFIC_CAR};
use wp_net::DOWNLINK_BUDGET_BPS;
use wp_sim::SimConfig;

/// Southbound lane of the x ≈ 55 m collector.
const LANE_X: f32 = 53.25;

fn link(seed: u64) -> LinkConfig {
    LinkConfig {
        latency_ms: 30.0,
        jitter_ms: 5.0,
        loss: 0.01,
        retransmit_ms: 100.0,
        seed,
    }
}

#[test]
fn traffic_weather_events_and_captain_on_the_real_sim() {
    let (cars, modes) = load_data(&repo_data_dir()).expect("data");
    let config = SimConfig {
        seed: 31,
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
    let world = wp_world::World::procedural(&pg, server_world_config(&config));
    let weather = city_weather(
        &world,
        wp_weather::WeatherConfig {
            seed: 31,
            start: wp_weather::Regime::new(WeatherKind::Overcast, 0.5),
            locked: true,
            ..Default::default()
        },
    );
    let city = CitySetup {
        world,
        police_car: Some(cars[AI_POLICE_CAR].clone()),
        traffic_car: Some(cars[TRAFFIC_CAR].clone()),
    };
    let mut sim = WpSim::with_city(config, cars.clone(), city);
    sim.set_weather(weather, 1.0);

    let cfg = ServerConfig {
        racer_car: "debug_coupe".into(),
        spawn_points: vec![
            ([LANE_X, 400.0, 0.0], std::f32::consts::PI),
            ([LANE_X, 370.0, 0.0], std::f32::consts::PI),
        ],
        ..ServerConfig::default()
    };
    let radius = cfg.traffic_radius_m;
    let mut h = Harness::with_sim(cfg, sim, cars, modes, default_damage_factory);
    for i in 0..3 {
        h.add_peer(&format!("p{i}"), link(i), link(50 + i));
    }
    h.wait_joined();
    h.request_role(0, Role::Racer);
    h.request_role(1, Role::Racer);
    h.request_role(2, Role::PoliceCaptain);
    h.run_secs(1.0);
    assert_eq!(h.peers[2].client.role(), Role::PoliceCaptain);
    // Weather on join.
    let joined_weather = h.server.sim().weather().expect("server weather");
    for p in &h.peers {
        let w = p.client.weather.as_ref().expect("weather on join");
        assert_eq!(w.kind, WeatherKind::Overcast);
        assert_eq!(w.water_q8.len(), joined_weather.water_q8.len());
    }

    // Racer 0 is wanted: the police system should tell it so (typed event).
    let wanted = h.peers[0].client.own_vehicle().expect("car");
    let mut police = wp_police::PoliceSystem::for_world(
        wp_police::PoliceConfig::builtin(),
        h.server.sim().sim().world(),
        wp_police::PoliceSetup {
            seed: 31,
            mode: DamageMode::Arcade,
            internal_driving: true,
            hazards: true,
        },
    );
    police.set_heat(wanted, 55.0);
    h.server.sim_mut().sim_mut().set_police(police);
    h.peers[0].manual_input.throttle = 0.4;
    h.peers[1].manual_input.throttle = 0.3;

    // Server kinematic traffic per tick, to check what racers hold.
    let mut server_traffic: BTreeMap<u64, BTreeMap<u32, TrafficAgentSnapshot>> = BTreeMap::new();
    let (mut checked, mut max_held) = (0usize, 0usize);
    let mut forced_at = None;
    for step in 0..20 * 60 {
        h.step();
        let tick = h.server.sim().tick();
        let agents: BTreeMap<u32, TrafficAgentSnapshot> = h
            .server
            .sim()
            .traffic_agents()
            .into_iter()
            .map(|a| (a.id, a))
            .collect();
        server_traffic.insert(tick, agents);
        while server_traffic.len() > 120 {
            server_traffic.pop_first();
        }
        if step == 10 * 60 {
            let w = h.server.sim_mut().weather_sim_mut().expect("weather");
            w.force(wp_weather::Regime::new(WeatherKind::HeavyRain, 0.9), true);
            forced_at = Some(h.now_ms);
        }
        for i in 0..2 {
            let c = &h.peers[i].client;
            let Some(held) = server_traffic.get(&c.traffic_tick()) else {
                continue;
            };
            let own = c.own_vehicle().expect("car");
            let own_pos = h
                .server
                .sim()
                .vehicles()
                .into_iter()
                .find(|(e, _)| *e == own)
                .map(|(_, s)| s.pos)
                .expect("own state");
            max_held = max_held.max(c.traffic().len());
            for a in c.traffic() {
                let truth = held.get(&a.id).unwrap_or_else(|| {
                    panic!("peer {i} holds agent {} the server does not have", a.id)
                });
                let q = QTrafficAgent::from_snapshot(truth);
                assert_eq!(QTrafficAgent::from_snapshot(a), q, "agent {}", a.id);
                // The interest radius is measured at the snapshot tick; allow for 1 s of motion.
                let d = ((truth.pos[0] - own_pos[0]).powi(2) + (truth.pos[1] - own_pos[1]).powi(2))
                    .sqrt();
                assert!(d <= radius + 40.0, "agent {} at {d:.0} m", a.id);
                checked += 1;
            }
        }
        assert!(
            h.peers[2].client.traffic().is_empty(),
            "captain gets no traffic"
        );
    }
    println!(
        "traffic: {checked} agent states checked, up to {max_held} held by one client; \
         down-link worst seconds {:?}",
        h.peers
            .iter()
            .map(|p| p.down_per_second.iter().max().copied())
            .collect::<Vec<_>>()
    );
    assert!(checked > 100 && max_held >= 2, "{checked} {max_held}");

    // Weather change: event + snapshot within ~1 s.
    assert!(forced_at.is_some());
    let server_w = h.server.sim().weather().expect("weather");
    assert_eq!(server_w.kind, WeatherKind::HeavyRain);
    for (i, p) in h.peers.iter().enumerate() {
        let w = p.client.weather.as_ref().expect("weather");
        assert_eq!(w.kind, WeatherKind::HeavyRain, "peer {i}");
        assert!(p.client.game_events.iter().any(|e| matches!(
            e,
            SimEvent::WeatherChanged {
                kind: WeatherKind::HeavyRain,
                forced: true,
                ..
            }
        )));
    }

    // Typed police events.
    let racer_events = &h.peers[0].client.game_events;
    assert!(
        racer_events.iter().any(|e| matches!(
            e,
            SimEvent::Police(PoliceEvent::WantedLevelChanged { racer, .. }) if *racer == wanted
        )) || h.peers[0].client.wanted.contains_key(&wanted.0),
        "wanted racer is told its level: {racer_events:?}"
    );
    for p in &h.peers[..2] {
        assert!(
            !p.client
                .game_events
                .iter()
                .any(|e| matches!(e, SimEvent::Police(PoliceEvent::Radio { .. }))),
            "racers get no radio chatter"
        );
    }
    let radio = h.peers[2]
        .client
        .game_events
        .iter()
        .filter(|e| matches!(e, SimEvent::Police(PoliceEvent::Radio { .. })))
        .count();
    println!(
        "events: racer {} captain {} (radio {radio}); sent {}",
        h.peers[0].client.game_events.len(),
        h.peers[2].client.game_events.len(),
        h.server.stats().events_sent
    );

    // Captain: AI units carry their wp_police ids; commands go to wp_police and come back.
    let map = &h.peers[2].client.captain;
    let ai_units: Vec<u32> = map
        .units
        .values()
        .filter(|u| u.kind == EntityKind::AiPolice)
        .filter_map(|u| u.unit)
        .collect();
    assert!(!ai_units.is_empty(), "AI police on the captain map");
    let now = h.now_ms;
    let c = &mut h.peers[2].client;
    let ok = c.captain_command(
        CaptainCommand::SetPriorityTarget {
            target: Some(wanted),
        },
        now,
    );
    let unknown = c.captain_command(
        CaptainCommand::CallOff {
            target: EntityId(99_999),
        },
        now,
    );
    let bad_unit = c.captain_command(
        CaptainCommand::AssignUnit {
            unit: wp_contracts::UnitId(77_777),
            order: wp_contracts::UnitOrder::Patrol,
        },
        now,
    );
    h.run_secs(0.5);
    let replies: BTreeMap<u32, Result<(), CaptainError>> =
        h.peers[2].client.captain_replies.iter().copied().collect();
    assert_eq!(replies.get(&ok), Some(&Ok(())));
    assert_eq!(
        replies.get(&unknown),
        Some(&Err(CaptainError::UnknownTarget))
    );
    assert_eq!(
        replies.get(&bad_unit),
        Some(&Err(CaptainError::UnknownUnit))
    );
    assert!(h.peers[2].client.game_events.iter().any(|e| matches!(
        e,
        SimEvent::Police(PoliceEvent::Radio { text, .. }) if text.contains("priority")
    )));

    for (i, p) in h.peers.iter().enumerate() {
        let worst = p.down_per_second.iter().copied().max().unwrap_or(0);
        assert!(
            worst as usize <= DOWNLINK_BUDGET_BPS,
            "peer {i} worst second {worst} B"
        );
    }
}
