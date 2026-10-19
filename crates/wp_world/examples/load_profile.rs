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
}
