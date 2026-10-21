//! Cost of tunnel sightlines on Port Vesper v2: times `CityMap::line_of_sight` for random
//! eye-height pairs within 250 m of each other around (and inside) the two covered roads, with
//! the cover rules on and off, and prints how many of the pairs the cover blocks.
//!
//! ```sh
//! cargo run --release -p wp_world --example tunnel_los_bench
//! ```

use std::time::Instant;

use wp_world::covered::CoverShape;
use wp_world::vesper::{generate, VesperConfig};
use wp_world::CityMap;

fn main() {
    let net = generate(&VesperConfig::default());
    let t = net.terrain.clone().expect("terrain");
    let mut map = CityMap::new(net);
    let mut rng = wp_math::Rng::new(7);
    let shapes: Vec<CoverShape> = map.net.covered.iter().map(CoverShape::new).collect();
    // Pairs: one end on or near a tunnel road (half inside), the other within 250 m.
    let mut pairs = Vec::new();
    for _ in 0..100_000 {
        let sh = &shapes[rng.next_u32() as usize % shapes.len()];
        let s = rng.range_f32(-150.0, sh.length() + 150.0);
        let (c, _) = wp_world::geom::point_at(&sh.span.centre, &sh.cum, s);
        let a = [
            c[0] + rng.range_f32(-6.0, 6.0),
            c[1] + rng.range_f32(-6.0, 6.0),
            c[2] + 1.3,
        ];
        let bx = [
            a[0] + rng.range_f32(-250.0, 250.0),
            a[1] + rng.range_f32(-250.0, 250.0),
        ];
        let b = [bx[0], bx[1], t.height_at(bx) + 1.3];
        pairs.push((a, b));
    }
    let mut run = |on: bool| {
        map.covered_los = on;
        let start = Instant::now();
        let clear = pairs
            .iter()
            .filter(|(a, b)| map.line_of_sight(*a, *b))
            .count();
        (
            start.elapsed().as_secs_f64() * 1e6 / pairs.len() as f64,
            clear,
        )
    };
    let (us_off, clear_off) = run(false);
    let (us_on, clear_on) = run(true);
    println!(
        "{} pairs: cover off {:.2} µs/query ({} clear), cover on {:.2} µs/query ({} clear, {} blocked by cover)",
        pairs.len(),
        us_off,
        clear_off,
        us_on,
        clear_on,
        clear_off - clear_on
    );
}
