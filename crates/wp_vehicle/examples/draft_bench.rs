//! Cost of the slipstream pass (`wp_vehicle::draft::drag_scales_into`, run by `wp_sim` once
//! per physics tick over every vehicle): `cargo run --release -p wp_vehicle --example
//! draft_bench`. Reports µs per tick and per 60 Hz frame (4 ticks) for 24 and 64 vehicles in
//! a tight race pack (every pair within reach: the O(n²) worst case) and spread over a city
//! (the broad phase skips almost every pair).

use std::time::Instant;

use wp_vehicle::draft::{
    drag_scales_all_pairs, drag_scales_into, DraftBody, DraftParams, DraftScratch,
};

/// `n` cars four abreast (3.5 m lanes), 8 m apart nose to tail, all at 40 m/s along +Y.
fn pack(n: usize) -> Vec<DraftBody> {
    (0..n)
        .map(|i| DraftBody {
            pos: [(i % 4) as f32 * 3.5, -((i / 4) as f32) * 8.0],
            vel: [0.0, 40.0],
            width_m: 1.8,
            length_m: 4.6,
        })
        .collect()
}

/// `n` cars on a 1.2 km square city grid, mixed headings and speeds (seeded).
fn spread(n: usize) -> Vec<DraftBody> {
    let mut rng = wp_math::Rng::new(3);
    (0..n)
        .map(|_| {
            let s = rng.range_f32(0.0, 35.0);
            let along_x = rng.range_f32(0.0, 1.0) < 0.5;
            DraftBody {
                pos: [rng.range_f32(-600.0, 600.0), rng.range_f32(-600.0, 600.0)],
                vel: if along_x { [s, 0.0] } else { [0.0, s] },
                width_m: 1.8,
                length_m: 4.6,
            }
        })
        .collect()
}

fn time(bodies: &[DraftBody], p: &DraftParams) -> (f64, f64) {
    let iters = 20_000;
    let (mut scratch, mut out) = (DraftScratch::default(), Vec::new());
    let mut sink = 0.0f32;
    let t0 = Instant::now();
    for _ in 0..iters {
        drag_scales_into(std::hint::black_box(bodies), p, &mut scratch, &mut out);
        sink += out[0];
    }
    let sweep = t0.elapsed().as_secs_f64() * 1e6 / f64::from(iters);
    let t1 = Instant::now();
    for _ in 0..iters {
        let v = drag_scales_all_pairs(std::hint::black_box(bodies), p);
        sink += v[0];
    }
    let pairs = t1.elapsed().as_secs_f64() * 1e6 / f64::from(iters);
    std::hint::black_box(sink);
    (sweep, pairs)
}

fn main() {
    let p = DraftParams::default();
    println!("slipstream pass, µs per physics tick (×4 per 60 Hz frame)");
    println!("layout      n   sweep µs/tick   ms/frame   all-pairs µs/tick");
    for n in [24usize, 64] {
        for (name, bodies) in [("pack", pack(n)), ("spread", spread(n))] {
            let (sweep, pairs) = time(&bodies, &p);
            println!(
                "{name:<8} {n:>4}   {sweep:>13.2}   {:>8.4}   {pairs:>17.2}",
                sweep * 4.0 / 1000.0
            );
        }
    }
}
