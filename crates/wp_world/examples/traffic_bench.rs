//! Traffic benchmark + diagnostics on the procedural city.
//!
//! `cargo run --release -p wp_world --example traffic_bench -- [minutes] [agents] [hour] [seed]
//!  [aggressive_fraction]`
//!
//! Runs the world at `PHYSICS_DT` (traffic AI at 60 Hz inside) with collision diagnostics on,
//! and prints per-frame cost (one 60 Hz frame = 4 physics ticks) and traffic counters.

use std::time::Instant;

use wp_contracts::{TimeOfDay, PHYSICS_DT};
use wp_world::procgen::ProcGenConfig;
use wp_world::{TrafficConfig, World, WorldConfig};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let minutes: f32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(2.0);
    let agents: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(400);
    let hour: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(12.0);
    let seed: u64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(1);
    let aggressive: f32 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(0.05);
    let t_load = Instant::now();
    let mut w = World::procedural(
        &ProcGenConfig::default(),
        WorldConfig {
            seed,
            start_time: TimeOfDay::from_hours(hour),
            // Keep the time of day roughly constant for the run.
            time_ratio: 1.0,
            traffic: TrafficConfig {
                max_agents: agents,
                density_scale: 4.0,
                aggressive_fraction: aggressive,
                detect_collisions: std::env::var("NO_COLLISIONS").is_err(),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    println!(
        "generate + load: {:.1} ms, {:.1} drivable lane-km",
        t_load.elapsed().as_secs_f64() * 1000.0,
        w.map().zone_lane_km.iter().sum::<f32>()
    );
    let t_load = Instant::now();
    let w2 = World::load(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/city/procedural_12x12.road.bin"
        ),
        WorldConfig::default(),
    )
    .expect("data/city/procedural_12x12.road.bin");
    println!(
        "load .road.bin: {:.1} ms ({} lanes)",
        t_load.elapsed().as_secs_f64() * 1000.0,
        w2.network().lanes.len()
    );
    w.populate();
    println!("populated: {} agents", w.agent_count());
    let ticks = (minutes * 60.0 / PHYSICS_DT) as usize;
    let mut ev = Vec::new();
    let mut worst = 0.0f64;
    let mut frames_ms: Vec<f64> = Vec::new();
    let t0 = Instant::now();
    let mut frame_start = Instant::now();
    for i in 0..ticks {
        w.step(PHYSICS_DT, &mut ev);
        if i % 4 == 3 {
            let f = frame_start.elapsed().as_secs_f64();
            if i > 240 {
                worst = worst.max(f);
                frames_ms.push(f * 1000.0);
            }
            frame_start = Instant::now();
        }
        if i % (240 * 30) == 0 {
            let s = w.traffic_stats();
            println!(
                "t={:>5.0}s agents={} spawned={} despawned={} stuck={} collisions={} (aggr {}) lane_changes={} emergency={}",
                i as f32 * PHYSICS_DT,
                w.agent_count(),
                s.spawned,
                s.despawned,
                s.stuck_recycled,
                s.civilian_collisions,
                s.collisions_involving_aggressive,
                s.lane_changes,
                s.emergency_brakes
            );
        }
    }
    let total = t0.elapsed().as_secs_f64();
    frames_ms.sort_by(|a, b| a.total_cmp(b));
    let pct = |p: f64| frames_ms[((frames_ms.len() - 1) as f64 * p) as usize];
    println!(
        "frame ms: p50 {:.3} p99 {:.3} p99.9 {:.3} max {:.3}",
        pct(0.5),
        pct(0.99),
        pct(0.999),
        pct(1.0)
    );
    let frames = ticks as f64 / 4.0;
    let s = w.traffic_stats();
    let moving = w.agents().filter(|a| a.speed_mps > 1.0).count();
    let mean_speed: f32 =
        w.agents().map(|a| a.speed_mps).sum::<f32>() / w.agent_count().max(1) as f32;
    println!(
        "done: {:.1} min sim, {} agents, {:.3} ms/frame avg, {:.3} ms worst frame, moving {}/{}, mean speed {:.1} m/s",
        minutes,
        w.agent_count(),
        total * 1000.0 / frames,
        worst * 1000.0,
        moving,
        w.agent_count(),
        mean_speed
    );
    println!("{s:?}");
    println!("hash {:016x}", w.state_hash());
}
