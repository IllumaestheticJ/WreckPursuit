//! Print lanes (with road names) passing through a box (X0 X1 Y0 Y1 env), every point inside.
use wp_world::vesper::ashpeak;

fn main() {
    let f = |k: &str| std::env::var(k).unwrap().parse::<f32>().unwrap();
    let (x0, x1, y0, y1) = (f("X0"), f("X1"), f("Y0"), f("Y1"));
    let (net, report) = ashpeak::generate_with_report(&ashpeak::default_config());
    eprintln!("crossovers {:?}", wp_world::crossover::crossovers(&net));
    for (i, l) in net.lanes.iter().enumerate() {
        let inside: Vec<_> = l
            .polyline
            .iter()
            .filter(|p| p[0] >= x0 && p[0] <= x1 && p[1] >= y0 && p[1] <= y1)
            .collect();
        if inside.is_empty() {
            continue;
        }
        println!(
            "lane {i} {:?} {:?} idx {} w {:.1} edges L{} R{} -> {:?}",
            report.road_names.get(l.road as usize),
            l.kind,
            l.index_from_left,
            l.width_m,
            l.edge_left,
            l.edge_right,
            l.connections.iter().map(|c| c.to).collect::<Vec<_>>()
        );
        for p in inside.iter().step_by(4.max(inside.len() / 12)) {
            println!("    ({:.1},{:.1},{:.2})", p[0], p[1], p[2]);
        }
    }
}
