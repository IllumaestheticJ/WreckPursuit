//! Grid city (8×8): lanes through a box (X0 X1 Y0 Y1), for the diverge-ramp wedge.
use wp_world::procgen::{self, ProcGenConfig};

fn main() {
    let f = |k: &str| std::env::var(k).unwrap().parse::<f32>().unwrap();
    let (x0, x1, y0, y1) = (f("X0"), f("X1"), f("Y0"), f("Y1"));
    let net = procgen::generate(&ProcGenConfig {
        nx: 8,
        ny: 8,
        ..ProcGenConfig::default()
    });
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
            "lane {i} road {} {:?} idx {} w {:.1} edges L{} R{}",
            l.road, l.kind, l.index_from_left, l.width_m, l.edge_left, l.edge_right
        );
        for p in &inside {
            println!("    ({:.1},{:.1},{:.2})", p[0], p[1], p[2]);
        }
    }
}
