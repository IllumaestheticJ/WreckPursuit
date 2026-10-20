//! B05: disconnecting while wanted counts as a capture after the grace
//! (`WANTED_DISCONNECT_GRACE_TICKS`, 60 s), so quitting is never the best escape.
//!
//! One `ServerInstance<WpSim>` on a small procedural city with `wp_police`; one racer joins,
//! is made wanted (heat set directly), and then:
//! - drops its link and stays away → the car is held for the whole grace with its heat
//!   frozen, then the pursuit ends `Captured` and the bust is settled on the account (fine,
//!   car impounded);
//! - drops its link and comes back within the grace → same player, car, heat and wanted
//!   level; no capture;
//! - says `Bye` while wanted → captured and settled at once;
//! - drops its link while **not** wanted → no capture, the normal 30 s window applies.

mod common;

use common::{data, Harness};
use wp_contracts::{
    DamageMode, EconomyReason, PlayerId, PoliceEvent, PursuitOutcome, Role, SimEvent, TimeOfDay,
    WantedLevel,
};
use wp_economy::{AccountStore, CarLocation};
use wp_net::client::default_damage_factory;
use wp_net::netsim::{server_world_config, CitySetup, NetSim, WpSim};
use wp_net::server::{ServerConfig, WANTED_DISCONNECT_GRACE_TICKS};
use wp_net::sim_link::LinkConfig;
use wp_net::TICK_HZ;
use wp_sim::SimConfig;

const HEAT: f32 = 55.0;

fn ideal() -> LinkConfig {
    LinkConfig::ideal(20.0)
}

/// Server on a 4×4 procedural city (no traffic) with one racer joined and wanted.
fn wanted_racer() -> (Harness<WpSim>, wp_contracts::EntityId) {
    let (mut h, car) = racer();
    h.server
        .sim_mut()
        .sim_mut()
        .police_mut()
        .set_heat(car, HEAT);
    h.run_secs(0.1);
    assert!(h.server.sim().wanted_or_pursued(car), "racer is wanted");
    (h, car)
}

/// Server on a 4×4 procedural city (no traffic) with one racer joined (not wanted).
fn racer() -> (Harness<WpSim>, wp_contracts::EntityId) {
    let (cars, modes) = data();
    let config = SimConfig {
        seed: 5,
        mode: modes[&DamageMode::Arcade].clone(),
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 1.0,
    };
    let pg = wp_world::procgen::ProcGenConfig {
        nx: 4,
        ny: 4,
        spacing_m: 110.0,
        seed: 1,
        ..Default::default()
    };
    let mut wcfg = server_world_config(&config);
    wcfg.traffic.enabled = false;
    let world = wp_world::World::procedural(&pg, wcfg);
    let city = CitySetup {
        world,
        police_car: None,
        traffic_car: None,
    };
    let sim = WpSim::with_city(config, cars.clone(), city);
    let cfg = ServerConfig {
        racer_car: "debug_coupe".into(),
        spawn_points: vec![([53.25, 200.0, 0.0], std::f32::consts::PI)],
        ..Default::default()
    };
    let mut h = Harness::with_sim(cfg, sim, cars, modes, default_damage_factory);
    h.server.set_event_tap(true);
    h.add_peer("racer", ideal(), ideal());
    h.wait_joined();
    h.request_role(0, Role::Racer);
    h.run_secs(1.0);
    let car = h.peers[0].client.own_vehicle().expect("racer car");
    (h, car)
}

fn heat(h: &Harness<WpSim>, car: wp_contracts::EntityId) -> f32 {
    h.server
        .sim()
        .sim()
        .police()
        .racer(car)
        .map_or(0.0, |r| r.heat)
}

fn level(h: &Harness<WpSim>, car: wp_contracts::EntityId) -> WantedLevel {
    h.server.sim().sim().police().wanted_level(car)
}

/// `PursuitEnded` outcomes for `car` among the tapped events.
fn pursuit_ends(h: &mut Harness<WpSim>, car: wp_contracts::EntityId) -> Vec<PursuitOutcome> {
    h.server
        .take_tapped_events()
        .into_iter()
        .filter_map(|e| match e {
            SimEvent::Police(PoliceEvent::PursuitEnded { racer, outcome, .. }) if racer == car => {
                Some(outcome)
            }
            _ => None,
        })
        .collect()
}

#[test]
fn grace_is_sixty_seconds() {
    assert_eq!(WANTED_DISCONNECT_GRACE_TICKS, 60 * TICK_HZ);
    assert_eq!(
        ServerConfig::default().wanted_grace_ticks,
        WANTED_DISCONNECT_GRACE_TICKS
    );
}

#[test]
fn disconnect_while_wanted_is_captured_after_the_grace() {
    let (mut h, car) = wanted_racer();
    let player = h.peers[0].client.player().unwrap();
    let level_at_drop = level(&h, car);
    let heat_at_drop = heat(&h, car);
    let _ = h.server.take_tapped_events();

    h.drop_link(0);
    // Past the normal 30 s window, short of the grace: the car is still held, frozen.
    h.run_secs(59.0);
    assert!(
        h.server.sim().vehicles().iter().any(|(e, _)| *e == car),
        "car held in the world during the grace"
    );
    assert!(h.server.sim().sim().police().is_racer_held(car));
    assert_eq!(level(&h, car), level_at_drop, "wanted level frozen");
    assert_eq!(
        heat(&h, car),
        heat_at_drop,
        "heat frozen (no decay offline)"
    );
    let speed = h
        .server
        .sim()
        .vehicles()
        .into_iter()
        .find(|(e, _)| *e == car)
        .map(|(_, s)| (s.lin_vel[0].powi(2) + s.lin_vel[1].powi(2)).sqrt())
        .unwrap();
    assert!(speed < 0.5, "held car is stationary ({speed} m/s)");
    assert!(pursuit_ends(&mut h, car).is_empty(), "no outcome yet");
    assert_eq!(h.server.stats().players, 1);

    h.run_secs(1.5);
    assert_eq!(h.server.stats().players, 0, "removed after the grace");
    assert!(!h.server.sim().vehicles().iter().any(|(e, _)| *e == car));
    assert_eq!(pursuit_ends(&mut h, car), vec![PursuitOutcome::Captured]);
    assert_eq!(h.server.stats().abandoned_captures, 1);
    assert_eq!(h.server.stats().busts_settled, 1);
    assert_eq!(
        level(&h, car),
        WantedLevel::None,
        "heat cleared like a bust"
    );

    // Settled like a normal bust: fine charged, car impounded.
    let acct = h
        .server
        .accounts_mut()
        .load(player)
        .unwrap()
        .expect("account written");
    assert!(acct.cash < 0, "fine charged (cash {})", acct.cash);
    assert_eq!(acct.heat_history.captures, 1);
    assert!(acct
        .garage
        .iter()
        .any(|c| matches!(c.location, CarLocation::Impound { .. })));
    let reasons: Vec<EconomyReason> = h
        .server
        .accounts_mut()
        .events_since(player, 0)
        .unwrap()
        .iter()
        .map(|e| e.reason.to_contract())
        .collect();
    assert!(
        reasons
            .iter()
            .any(|r| matches!(r, EconomyReason::Fine { .. })),
        "{reasons:?}"
    );
    assert!(
        reasons.contains(&EconomyReason::CarImpounded),
        "{reasons:?}"
    );
}

#[test]
fn reconnect_within_the_grace_resumes_the_same_pursuit() {
    let (mut h, car) = wanted_racer();
    let player = h.peers[0].client.player().unwrap();
    let token = h.peers[0].client.token();
    let level_at_drop = level(&h, car);
    let heat_at_drop = heat(&h, car);
    let _ = h.server.take_tapped_events();

    h.drop_link(0);
    h.run_secs(45.0);
    h.connect(0, token);
    h.run_secs(1.0);
    let c = &h.peers[0].client;
    assert_eq!(c.player(), Some(player));
    assert_eq!(c.own_vehicle(), Some(car), "same car");
    assert_eq!(c.role(), Role::Racer);
    assert!(
        !h.server.sim().sim().police().is_racer_held(car),
        "released"
    );
    assert_eq!(level(&h, car), level_at_drop, "same wanted level");
    // Heat runs again after the reconnect; within a second it barely moves.
    assert!((heat(&h, car) - heat_at_drop).abs() < 2.0, "same heat");

    // Well past where the grace would have ended: no abandoned capture.
    h.run_secs(20.0);
    assert_eq!(h.server.stats().abandoned_captures, 0);
    assert_eq!(h.server.stats().players, 1);
    assert_eq!(h.peers[0].client.player(), Some(PlayerId(player.0)));
}

#[test]
fn bye_while_wanted_is_an_immediate_capture() {
    let (mut h, car) = wanted_racer();
    let _ = h.server.take_tapped_events();
    let now = h.now_ms;
    h.peers[0].client.bye(now);
    h.run_secs(0.5);
    assert_eq!(h.server.stats().players, 0);
    assert_eq!(h.server.stats().abandoned_captures, 1);
    assert_eq!(pursuit_ends(&mut h, car), vec![PursuitOutcome::Captured]);
}

#[test]
fn disconnect_while_not_wanted_is_free() {
    let (mut h, car) = racer();
    assert!(!h.server.sim().wanted_or_pursued(car));
    let _ = h.server.take_tapped_events();
    h.drop_link(0);
    h.run_secs(31.0);
    assert_eq!(h.server.stats().players, 0, "normal 30 s window");
    assert_eq!(h.server.stats().abandoned_captures, 0);
    assert_eq!(h.server.stats().busts_settled, 0);
    assert!(pursuit_ends(&mut h, car).is_empty());
}
