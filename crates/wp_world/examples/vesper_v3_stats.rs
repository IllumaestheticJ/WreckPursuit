//! Port Vesper v3 road statistics:
//! `cargo run --release -p wp_world --example vesper_v3_stats [-- --dump out.json]`.
//!
//! Prints generation and `CityMap::new` timings, network sizes, the road generator's notes
//! (interchanges, bridges, tunnels, grade separations, highway lengths) and grade / bank
//! extremes per road name. `--dump PATH` writes a compact JSON of the plan, the lanes, the
//! features and a downsampled terrain for plotting.

use serde_json::{json, Value};
use wp_world::geom;
use wp_world::metro::{self, plan::Plan, MetroConfig};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dump = args
        .iter()
        .position(|a| a == "--dump")
        .and_then(|i| args.get(i + 1).cloned());
    let t0 = std::time::Instant::now();
    let (net, report) = metro::generate_with_report(&MetroConfig::default());
    let gen_ms = t0.elapsed().as_secs_f64() * 1e3;
    let t1 = std::time::Instant::now();
    let map = wp_world::CityMap::new(net.clone());
    let map_ms = t1.elapsed().as_secs_f64() * 1e3;
    println!(
        "{}: generate {gen_ms:.0} ms, CityMap::new {map_ms:.0} ms; {} nodes, {} lanes, {} junctions ({} signalised), {} surfaces, {} features, {} POIs, {} segs",
        net.id,
        net.nodes.len(),
        net.lanes.len(),
        net.intersections.len(),
        net.signals.len(),
        net.surfaces.len(),
        net.features.len(),
        net.pois.len(),
        map.segs.len()
    );
    for n in &report.notes {
        println!("{n}");
    }
    for (k, c) in &report.feature_counts {
        println!("features {k:?}: {c}");
    }
    let mut kinds: std::collections::BTreeMap<String, usize> = Default::default();
    for l in &net.lanes {
        *kinds.entry(format!("{:?}", l.kind)).or_default() += 1;
    }
    println!("lanes by kind: {kinds:?}");
    let mut worst_grade = 0.0f32;
    let mut worst_bank = 0.0f32;
    for s in &net.surfaces {
        let cum = geom::cumulative(&s.centre);
        for k in 0..s.centre.len() - 1 {
            let ds = (cum[k + 1] - cum[k]).max(0.5);
            worst_grade = worst_grade.max(((s.centre[k + 1][2] - s.centre[k][2]) / ds).abs());
            worst_bank = worst_bank.max(s.bank[k].abs());
        }
    }
    println!("worst grade {worst_grade:.3}, worst bank {worst_bank:.3}");
    if args.iter().any(|a| a == "--check") {
        let (_, gnd, out) = metro::roads::build_standalone(&MetroConfig::default());
        let v = metro::roads::check::violations(&out, &gnd.hf);
        for line in metro::roads::check::summary(&out, &v) {
            println!("{line}");
        }
        let mut by: std::collections::BTreeMap<String, usize> = Default::default();
        for c in out.report.crossings.iter().filter(|c| c.clearance < 7.5) {
            let k = format!(
                "{:?}/{:?} {}|{}",
                out.report.info[c.upper].kind,
                out.report.info[c.lower].kind,
                out.names[c.upper],
                out.names[c.lower]
            );
            *by.entry(k).or_default() += 1;
        }
        for c in out
            .report
            .crossings
            .iter()
            .filter(|c| {
                c.clearance < 7.5 && out.report.info[c.upper].kind == out.report.info[c.lower].kind
            })
            .take(8)
        {
            let (a, b) = (&out.specs[c.upper], &out.specs[c.lower]);
            println!(
                "  LL {} ({}->{}) / {} ({}->{}) at {:?} clr {:.2}",
                c.upper, a.a, a.b, c.lower, b.a, b.b, c.pos, c.clearance
            );
        }
        let mut v: Vec<_> = by.into_iter().collect();
        v.sort_by_key(|x| std::cmp::Reverse(x.1));
        for (k, n) in v.iter().take(25) {
            println!("  low clearance {n}: {k}");
        }
    }
    if let Some(path) = dump {
        std::fs::write(&path, dump_json(&net).to_string()).expect("write dump");
        println!("wrote {path}");
    }
}

fn pts2(p: &[[f32; 2]]) -> Value {
    Value::Array(
        p.iter()
            .map(|q| json!([q[0].round(), q[1].round()]))
            .collect(),
    )
}

fn dump_json(net: &wp_world::network::RoadNetwork) -> Value {
    let plan = Plan::new();
    let lanes: Vec<Value> = net
        .lanes
        .iter()
        .map(|l| {
            json!({
                "k": format!("{:?}", l.kind),
                "p": l.polyline.iter().map(|p| json!([p[0].round(), p[1].round(), (p[2] * 10.0).round() / 10.0])).collect::<Vec<_>>(),
                "c": l.connections.len(),
            })
        })
        .collect();
    let surfaces: Vec<Value> = net
        .surfaces
        .iter()
        .map(|s| {
            json!({
                "p": s.centre.iter().map(|p| json!([p[0].round(), p[1].round(), (p[2] * 10.0).round() / 10.0])).collect::<Vec<_>>(),
                "w": s.left_m + s.right_m,
                "h": s.highway,
            })
        })
        .collect();
    let features: Vec<Value> = net
        .features
        .iter()
        .map(|f| {
            json!({
                "k": format!("{:?}", f.kind),
                "fp": pts2(&metro::ctx::footprint(&f.shape)),
            })
        })
        .collect();
    let t = net.terrain.as_ref().expect("terrain");
    let step = 4u32;
    let mut rows = Vec::new();
    let mut j = 0;
    while j < t.ny {
        let mut row = Vec::new();
        let mut i = 0;
        while i < t.nx {
            row.push(json!((t.at(i, j) * 10.0).round() / 10.0));
            i += step;
        }
        rows.push(Value::Array(row));
        j += step;
    }
    json!({
        "zones": plan.zones.iter().map(|z| json!({"name": z.name, "p": pts2(&z.zone.polygon)})).collect::<Vec<_>>(),
        "sites": plan.sites.iter().map(|s| json!({"id": s.id, "p": pts2(&s.polygon), "gates": s.gates.iter().map(|g| json!([g.pos[0], g.pos[1]])).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        "highways": plan.highways.iter().map(|h| json!({"name": h.name, "p": pts2(&h.ctrl), "closed": h.closed})).collect::<Vec<_>>(),
        "arterials": plan.arterials.iter().map(|a| json!({"name": a.name, "p": pts2(&a.ctrl)})).collect::<Vec<_>>(),
        "rail": plan.rail.iter().map(|r| json!({"name": r.name, "p": pts2(&r.line)})).collect::<Vec<_>>(),
        "structures": plan.structures.iter().map(|s| json!({"name": s.name, "p": pts2(&[s.from, s.to])})).collect::<Vec<_>>(),
        "interchanges": plan.interchanges.iter().map(|i| json!({"name": i.name, "p": [i.pos[0], i.pos[1]]})).collect::<Vec<_>>(),
        "lanes": lanes,
        "surfaces": surfaces,
        "features": features,
        "terrain": {"origin": t.origin, "cell": t.cell_m * step as f32, "rows": rows},
    })
}
