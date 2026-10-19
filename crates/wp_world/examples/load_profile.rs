//! City load-time profile: `cargo run [--release] -p wp_world --example load_profile`.

use std::time::Instant;

use wp_world::procgen::{generate, ProcGenConfig};
use wp_world::{CityMap, World, WorldConfig};

fn main() {
    for (name, cfg) in [
        ("default 12x12", ProcGenConfig::default()),
        ("small 6x6", ProcGenConfig::small()),
    ] {
        let t = Instant::now();
        let net = generate(&cfg);
        let gen = t.elapsed();
        let t = Instant::now();
        let map = CityMap::new(net.clone());
        let map_t = t.elapsed();
        let t = Instant::now();
        let w = World::new(net, WorldConfig::default());
        println!(
            "{name}: generate {gen:?}, CityMap::new {map_t:?} ({} segs), World::new {:?}, {} static props",
            map.segs.len(),
            t.elapsed(),
            w.static_props().count()
        );
    }
    // Line-of-sight cost: random 300 m sight lines at eye height in the default city.
    let map = CityMap::new(generate(&ProcGenConfig::default()));
    let mut rng = wp_math::Rng::new(1);
    let n = 100_000;
    let mut blocked = 0;
    let t = Instant::now();
    for _ in 0..n {
        let a = [
            rng.range_f32(-700.0, 700.0),
            rng.range_f32(-700.0, 700.0),
            1.5,
        ];
        let ang = rng.range_f32(-3.1, 3.1);
        let (s, c) = wp_math::sin_cos(ang);
        let b = [a[0] + c * 300.0, a[1] + s * 300.0, 1.5];
        blocked += usize::from(!map.line_of_sight(a, b));
    }
    println!(
        "line_of_sight: {:.2} µs per 300 m query ({} % blocked)",
        t.elapsed().as_secs_f64() * 1e6 / n as f64,
        blocked * 100 / n
    );
}
