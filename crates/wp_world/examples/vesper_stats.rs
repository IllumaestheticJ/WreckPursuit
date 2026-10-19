//! Port Vesper v2 generator statistics: `cargo run --release -p wp_world --example vesper_stats`.
//! Prints generation time, sizes, grade / crest / bank extremes per road class and the grade
//! separations (numbers quoted in `crates/wp_world/README.md`). An optional argument writes
//! the network as JSON there.

use wp_world::geom;
use wp_world::vesper::{generate_with_report, VesperConfig};

fn main() {
    let t0 = std::time::Instant::now();
    let (net, rep) = generate_with_report(&VesperConfig::default());
    let gen_ms = t0.elapsed().as_secs_f64() * 1e3;
    let t1 = std::time::Instant::now();
    let map = wp_world::CityMap::new(net.clone());
    let map_ms = t1.elapsed().as_secs_f64() * 1e3;
    if let Some(path) = std::env::args().nth(1) {
        std::fs::write(&path, net.to_json()).expect("write json");
    }
    let t = net.terrain.as_ref().unwrap();
    let (lo, hi) = t.range();
    println!(
        "{}: generate {gen_ms:.0} ms, CityMap {map_ms:.0} ms; {} nodes, {} lanes, {} intersections ({} signalised), {} buildings, {} POIs, {} segs",
        net.id,
        net.nodes.len(),
        net.lanes.len(),
        net.intersections.len(),
        net.signals.len(),
        net.buildings.len(),
        net.pois.len(),
        map.segs.len()
    );
    println!(
        "terrain {}x{} @ {} m, z {lo:.1}..{hi:.1}; skipped optional roads {}",
        t.nx, t.ny, t.cell_m, rep.skipped_roads
    );
    for c in &rep.crossings {
        println!(
            "crossing {:?}: {} over {} at ({:.0},{:.0})",
            if c.underpass { "underpass" } else { "overpass" },
            rep.road_names[c.upper],
            rep.road_names[c.lower],
            c.pos[0],
            c.pos[1]
        );
    }
    // Worst grade / crest per named road.
    let mut worst: std::collections::BTreeMap<&str, (f32, f32, f32, f32)> = Default::default();
    for s in &net.surfaces {
        let r = s.road as usize;
        let name = rep.road_names[r];
        let cum = geom::cumulative(&s.centre);
        let e = worst.entry(name).or_insert((0.0, 0.0, 0.0, 0.0));
        for k in 0..s.centre.len() - 1 {
            let ds = (cum[k + 1] - cum[k]).max(0.5);
            let g = ((s.centre[k + 1][2] - s.centre[k][2]) / ds).abs();
            e.0 = e.0.max(g);
            e.3 = e.3.max(s.bank[k].abs());
            if k > 0 {
                let ds0 = (cum[k] - cum[k - 1]).max(0.5);
                let g0 = (s.centre[k][2] - s.centre[k - 1][2]) / ds0;
                let g1 = (s.centre[k + 1][2] - s.centre[k][2]) / ds;
                let kv = (g1 - g0) / (0.5 * (ds + ds0));
                let v = rep.road_speeds[r];
                // Vertical acceleration at the limit over a crest (−) or sag (+), in g.
                e.1 = e.1.max(-kv * v * v / 9.81);
                e.2 = e.2.max(kv * v * v / 9.81);
            }
        }
    }
    println!("road: max grade %, crest g at limit, sag g at limit, max bank %");
    for (n, (g, c, sg, b)) in worst {
        println!(
            "  {n:24} {:5.1} {c:5.2} {sg:5.2} {:4.1}",
            g * 100.0,
            b * 100.0
        );
    }
}
