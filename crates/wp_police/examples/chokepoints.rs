//! `cargo run --release -p wp_police --example chokepoints`
//! Lists the escape points (`wp_police::strategy::ChokeIndex`) the police find on D's small
//! procedural city and on Port Vesper v2, with the time to build the index. Wall-clock timing
//! lives only in this example, never in sim code.

use std::time::Instant;

use wp_police::{PoliceConfig, PoliceSetup, PoliceSystem, RoadGraph};

fn report(name: &str, graph: RoadGraph) {
    let (nodes, segs) = (graph.nodes.len(), graph.segments.len());
    let mut p = PoliceSystem::with_city(PoliceConfig::builtin(), graph, PoliceSetup::default());
    let t0 = Instant::now();
    let chains = p.chokepoints().chains.len();
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let idx = p.chokepoints().clone();
    println!(
        "{name}: {nodes} nodes, {segs} segments, {chains} chains, {} escape points ({ms:.1} ms)",
        idx.chokepoints.len()
    );
    let mut cps = idx.chokepoints.clone();
    cps.sort_by(|a, b| b.score.total_cmp(&a.score));
    for c in &cps {
        let ch = &idx.chains[c.chain];
        let a = p.graph().node(ch.nodes[0]).pos;
        let b = p.graph().node(*ch.nodes.last().unwrap()).pos;
        println!(
            "  {:?} {:?} len {:5.0} m  detour +{:5.0} m  ({:6.0},{:6.0}) -> ({:6.0},{:6.0})",
            c.kind, ch.class, ch.length_m, c.detour_extra_m, a[0], a[1], b[0], b[1]
        );
    }
}

fn main() {
    let (g, _) = wp_police::city::procedural(&wp_police::city::ProcGenConfig::small());
    report("small procedural", g);
    let world = wp_world::World::vesper(
        &wp_world::vesper::VesperConfig::default(),
        wp_world::WorldConfig::default(),
    );
    report("Port Vesper", RoadGraph::from_city(world.map()));
}
