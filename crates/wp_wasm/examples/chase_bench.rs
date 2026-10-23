//! Native cost of the browser city chase (the same `Session` the wasm build runs, including
//! the per-tick replay hash).
//!
//! `cargo run --release -p wp_wasm --example chase_bench -- [car] [seconds] [city]`
//! (`city`: `vesper_v2` by default, or `ashpeak`, `procedural_12x12`)
//!
//! Drives the player with full throttle and a slow weave, steps in 60 Hz frames, and prints
//! sim ms per 60 Hz frame (mean / p95 / max), the vehicle count and how many city props are
//! awake. Browser (wasm) cost is roughly 1.2–2× native; `apps/testbed-web/e2e/chase_perf.mjs`
//! measures it in headless Chromium.

use std::time::Instant;

use wp_contracts::{DamageMode, EntityId, VehicleInput};
use wp_wasm::scenario::{city_chase_in, city_spec};
use wp_wasm::session::Session;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let car = args.get(1).map(String::as_str).unwrap_or("hakone_gt");
    let seconds: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(30.0);
    let city = args.get(3).map(String::as_str).unwrap_or("vesper_v2");
    let mut s = Session::new(
        city_chase_in(car, DamageMode::Arcade, city_spec(city)),
        false,
    )
    .expect("session");
    let player = EntityId(1);
    let frames = (seconds * 60.0) as usize;
    let mut ms = Vec::with_capacity(frames);
    let (mut max_vehicles, mut max_props) = (0, 0);
    for i in 0..frames {
        let steer = 0.25 * if (i / 120) % 2 == 0 { 1.0 } else { -1.0 };
        let input = VehicleInput {
            throttle: 1.0,
            steer,
            ..Default::default()
        };
        let _ = s.set_input(player, input);
        let t0 = Instant::now();
        s.step(1.0 / 60.0);
        ms.push(t0.elapsed().as_secs_f64() * 1e3);
        max_vehicles = max_vehicles.max(s.sim().vehicles().len());
        max_props = max_props.max(s.sim().active_prop_count());
    }
    let t0 = Instant::now();
    let mut acc = 0u64;
    for _ in 0..100 {
        acc = acc.wrapping_add(s.sim().state_hash());
    }
    let hash_ms = t0.elapsed().as_secs_f64() * 10.0;
    let mean = ms.iter().sum::<f64>() / ms.len() as f64;
    let mut sorted = ms.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let p95 = sorted[(sorted.len() as f64 * 0.95) as usize];
    let max = sorted[sorted.len() - 1];
    println!(
        "city chase ({city}, {car}, {seconds} s): sim {mean:.3} ms/frame mean, p95 {p95:.3}, max {max:.3}; \
         up to {max_vehicles} vehicles, {max_props}/{} props awake; state hash {hash_ms:.3} ms \
         per tick ({acc:x})",
        s.sim().prop_count()
    );
}
