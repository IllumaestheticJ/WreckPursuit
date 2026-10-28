//! List every free end of a barrier or rail (median and edge K-rails, deck rails, sidewalk
//! bare-back rails) that faces oncoming Loop 9 traffic: a car following a lane runs at its
//! blunt end (owner 2026-10-27 "sections of railing that abruptly start that will cause
//! massive crashes").
//!
//! `cargo run --release -p wp_sim --example loop9_barrier_ends` (ROAD=name for another road,
//! ROAD='*' for all).
use wp_world::geom;
use wp_world::vesper::ashpeak;
use wp_world::{TrafficConfig, World, WorldConfig};

fn main() {
    let target = std::env::var("ROAD").unwrap_or_else(|_| "Loop 9".into());
    let world = World::ashpeak(
        &ashpeak::default_config(),
        WorldConfig {
            traffic: TrafficConfig {
                enabled: false,
                ..Default::default()
            },
            parked_cars: false,
            ..Default::default()
        },
    );
    let names: Vec<String> = world.road_names().to_vec();
    let net = &world.map().net;
    let all = wp_world::barrier_ends::exposed_ends(world.map());
    let road_of = |e: &wp_world::barrier_ends::BarrierEnd| {
        names
            .get(net.lanes[e.lane].road as usize)
            .cloned()
            .unwrap_or_default()
    };
    let report: Vec<_> = all
        .iter()
        .filter(|e| target == "*" || road_of(e) == target)
        .collect();
    for e in &report {
        println!(
            "{:6} at ({:7.1},{:7.1}) z {:5.1}  lane {} heading {:4.0}°  in-line {:4.1} m",
            e.kind.name(),
            e.pos[0],
            e.pos[1],
            e.pos[2],
            road_of(e),
            geom::yaw_of(e.out).to_degrees(),
            e.lateral_m
        );
    }
    println!(
        "{} exposed ends on {target} ({} in the world)",
        report.len(),
        all.len()
    );
}
