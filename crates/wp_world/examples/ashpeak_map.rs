//! Ashpeak County map dump for the top-down renderer:
//! `cargo run --release -p wp_world --example ashpeak_map -- out.json` then
//! `python3 tools/map-render/render_map.py out.json map.png [x0 y0 x1 y1]`.
//! Prints generation time and sizes; `vesper` as the second argument dumps Port Vesper alone.

use std::fmt::Write as _;

use wp_world::ashpeak;
use wp_world::vesper::{self, VesperConfig};

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "ashpeak_map.json".into());
    let vesper_only = std::env::args().nth(2).as_deref() == Some("vesper");
    let t0 = std::time::Instant::now();
    let (net, rep) = if vesper_only {
        vesper::generate_with_report(&VesperConfig::default())
    } else {
        ashpeak::generate_with_report(&ashpeak::default_config())
    };
    let ms = t0.elapsed().as_secs_f64() * 1e3;
    let t = net.terrain.as_ref().expect("terrain");
    println!(
        "{}: generate {ms:.0} ms; {} nodes, {} lanes, {} intersections, {} buildings, {} decor, {} POIs, {} covered; terrain {}x{} @ {} m; skipped roads {}",
        net.id,
        net.nodes.len(),
        net.lanes.len(),
        net.intersections.len(),
        net.buildings.len(),
        net.decor.len(),
        net.pois.len(),
        net.covered.len(),
        t.nx,
        t.ny,
        t.cell_m,
        rep.skipped_roads
    );
    let mut s = String::from("{");
    // Terrain at 4 cells per sample.
    let step = 4u32;
    let (nx, ny) = (t.nx.div_ceil(step), t.ny.div_ceil(step));
    let _ = write!(
        s,
        "\"terrain\":{{\"origin\":[{},{}],\"cell\":{},\"nx\":{nx},\"ny\":{ny},\"z\":[",
        t.origin[0],
        t.origin[1],
        t.cell_m * step as f32
    );
    let mut first = true;
    for j in (0..t.ny).step_by(step as usize) {
        for i in (0..t.nx).step_by(step as usize) {
            if !first {
                s.push(',');
            }
            first = false;
            let _ = write!(s, "{:.1}", t.at(i, j));
        }
    }
    s.push_str("]},\"roads\":[");
    for (k, r) in net.surfaces.iter().enumerate() {
        if k > 0 {
            s.push(',');
        }
        let name = rep
            .road_names
            .get(r.road as usize)
            .copied()
            .unwrap_or("");
        let _ = write!(
            s,
            "{{\"name\":{:?},\"w\":{:.1},\"hw\":{},\"mat\":\"{:?}\",\"pts\":[",
            name,
            r.left_m + r.right_m,
            r.highway,
            r.material
        );
        for (i, p) in r.centre.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let _ = write!(s, "[{:.1},{:.1},{:.1}]", p[0], p[1], p[2]);
        }
        s.push_str("]}");
    }
    s.push_str("],\"buildings\":[");
    for (k, b) in net.buildings.iter().enumerate() {
        if k > 0 {
            s.push(',');
        }
        let _ = write!(s, "{{\"h\":{:.0},\"k\":\"{:?}\",\"fp\":[", b.height_m, b.kind);
        for (i, p) in b.footprint.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let _ = write!(s, "[{:.1},{:.1}]", p[0], p[1]);
        }
        s.push_str("]}");
    }
    s.push_str("],\"decor\":[");
    for (k, d) in net.decor.iter().enumerate() {
        if k > 0 {
            s.push(',');
        }
        let _ = write!(s, "[\"{:?}\",{:.1},{:.1}]", d.kind, d.pos[0], d.pos[1]);
    }
    s.push_str("],\"zones\":[");
    for (k, z) in net.zones.iter().enumerate() {
        if k > 0 {
            s.push(',');
        }
        let _ = write!(s, "{{\"k\":\"{:?}\",\"poly\":{:?}}}", z.kind, z.polygon);
    }
    s.push_str("],\"pois\":[");
    for (k, p) in net.pois.iter().enumerate() {
        if k > 0 {
            s.push(',');
        }
        let _ = write!(
            s,
            "{{\"k\":\"{:?}\",\"name\":{:?},\"p\":[{:.1},{:.1}]}}",
            p.kind, p.name, p.pos[0], p.pos[1]
        );
    }
    s.push_str("],\"covered\":[");
    for (k, c) in net.covered.iter().enumerate() {
        if k > 0 {
            s.push(',');
        }
        let pts: Vec<[f32; 2]> = c.centre.iter().map(|p| [p[0], p[1]]).collect();
        let _ = write!(s, "{{\"name\":{:?},\"pts\":{:?}}}", c.name, pts);
    }
    s.push_str("]}");
    std::fs::write(&out, s).expect("write");
    println!("wrote {out}");
}
