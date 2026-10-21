//! Per-frame sim cost of a city chase (release): `cargo run --release -p simcli --example
//! city_frame_cost -- [vesper|grid]`. Builds the chase like the testbed (23:00, full police,
//! traffic, parked cars, the player on the first race grid at 30 m/s), warms up 2 s, then
//! times 5 s of ticks with every system and with each system off. Numbers quoted in
//! `docs/integration/world.md` (city v2).

use std::time::Instant;

use simcli::data::FsData;
use simcli::run::{build_sim, RunOptions};
use simcli::scenario::{AssistSpec, CitySpec, Driver, EntitySpec};
use wp_sim::Systems;

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "vesper".into());
    let data = FsData::new(std::path::PathBuf::from("data"));
    let city = if which == "grid" {
        CitySpec::Procedural {
            seed: 1,
            blocks_x: 12,
            blocks_y: 12,
            block_m: 110.0,
        }
    } else {
        CitySpec::vesper()
    };
    let world = simcli::data::load_world(&data, &city, Default::default())
        .expect("city")
        .expect("not flat");
    let grid = world
        .network()
        .pois
        .iter()
        .find(|p| p.kind == wp_world::network::PoiKind::RaceStartGrid)
        .expect("race grid")
        .clone();
    let sc = simcli::Scenario {
        name: "city_frame_cost".into(),
        seed: 23,
        city,
        time_of_day_h: 23.0,
        time_ratio: 10.0,
        mode: wp_contracts::DamageMode::Arcade,
        assists: AssistSpec::Keyboard,
        traffic_density: None,
        traffic_cars: true,
        parked_cars: true,
        police_cars: true,
        covered_los: true,
        police_presence: Some(1.0),
        systems: None,
        barriers: Vec::new(),
        parked: Vec::new(),
        police_orders: Vec::new(),
        weather: None,
        entities: vec![EntitySpec {
            id: "player".into(),
            car: "debug_sedan".into(),
            pos: grid.pos,
            yaw: grid.heading_rad,
            speed_mps: 30.0,
            police_heat: 0.0,
            assists: None,
            player_steering: None,
            tune: Default::default(),
            driver: Driver::Scripted,
            inputs: Vec::new(),
        }],
        duration_s: 10.0,
        sample_hz: 60.0,
        hash_every: 240,
        expect: Vec::new(),
        expect_pending: Vec::new(),
        restart_after_capture: false,
        panels: false,
        slow: false,
    };
    let t = Instant::now();
    let mut sim = build_sim(&sc, &data, &RunOptions::default())
        .expect("build sim")
        .sim;
    println!("{which}: build {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    for _ in 0..480 {
        sim.step_fixed();
    }
    let off = |f: fn(&mut Systems)| {
        let mut s = Systems::ALL;
        f(&mut s);
        s
    };
    for (name, sys) in [
        ("all systems", Systems::ALL),
        ("police off", off(|s| s.police = false)),
        ("world off", off(|s| s.world = false)),
        ("vehicles off", off(|s| s.vehicle = false)),
    ] {
        sim.set_systems(sys);
        let t = Instant::now();
        for _ in 0..1200 {
            sim.step_fixed();
        }
        println!(
            "  {name:13} {:.3} ms per 60 Hz frame ({} vehicles, {} agents, {} bodies awake)",
            t.elapsed().as_secs_f64() * 1e3 / 300.0,
            sim.vehicles().len(),
            sim.world().agent_count(),
            sim.awake_body_count()
        );
    }
}
