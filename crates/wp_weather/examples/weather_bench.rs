//! `cargo run --release -p wp_weather --example weather_bench` — cost of one internal update
//! (schedule + grid) on a city-sized grid with 400 vehicles, and of `conditions_at` +
//! `wheel_grip_multiplier` per wheel query. Wall-clock timing lives here, never in `src/`.

use std::time::Instant;

use wp_contracts::{SurfaceType, TimeOfDay};
use wp_weather::{
    GridSpec, Regime, TireProfile, WeatherConfig, WeatherData, WeatherKind, WeatherSim,
};

fn main() {
    let grid = GridSpec::covering([-800.0, -800.0], [800.0, 800.0], 8.0);
    let cfg = WeatherConfig {
        start: Regime::new(WeatherKind::HeavyRain, 0.8),
        ..WeatherConfig::default()
    };
    let mut w = WeatherSim::uniform(WeatherData::embedded(), cfg, grid, SurfaceType::Asphalt);
    let vehicles: Vec<[f32; 3]> = (0..400)
        .map(|i| {
            [
                (i % 20) as f32 * 70.0 - 700.0,
                (i / 20) as f32 * 70.0 - 700.0,
                0.0,
            ]
        })
        .collect();
    let t = TimeOfDay::from_hours(18.0);
    let n = 2000;
    let start = Instant::now();
    for _ in 0..n {
        w.update(2.5, t, &vehicles);
    }
    let per_update_ms = start.elapsed().as_secs_f64() * 1e3 / n as f64;

    let table = &w.data().surfaces.clone();
    let tire = TireProfile::default();
    let q = 1_000_000;
    let start = Instant::now();
    let mut acc = 0.0f32;
    for i in 0..q {
        let p = [
            (i % 1600) as f32 - 800.0,
            ((i / 1600) % 1600) as f32 - 800.0,
            0.0,
        ];
        let c = w.conditions_at(p);
        acc += table.wheel_grip_multiplier(&c, SurfaceType::Asphalt, false, 30.0, &tire);
    }
    let per_query_ns = start.elapsed().as_secs_f64() * 1e9 / q as f64;
    // Per 60 Hz frame: 4 Hz updates → 1/15 update per frame; 24 cars × 4 wheels × 4 ticks × 2 substeps.
    let wheel_queries_per_frame = 24.0 * 4.0 * 4.0 * 2.0;
    println!("cells: {}", grid.len());
    println!("update (schedule + grid, 400 vehicles): {per_update_ms:.4} ms");
    println!(
        "  amortised per 60 Hz frame at 4 Hz:    {:.4} ms",
        per_update_ms / 15.0
    );
    println!("wheel query (conditions_at + grip):     {per_query_ns:.1} ns");
    println!(
        "  per frame, 24 cars (768 queries):     {:.4} ms",
        per_query_ns * wheel_queries_per_frame / 1e6
    );
    println!("(checksum {acc:.1})");
}
