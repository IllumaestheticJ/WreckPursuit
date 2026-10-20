//! Junction holes (playtest #46, "intersections still have holes in them").
//!
//! Casts rays straight down through the `wp_sim` city colliders on a fine grid over every
//! drivable lane and turning path at every junction, bend and dead end of Port Vesper v2 and
//! the 12×12 grid, and looks at the greybox the player sees at the same points: physics must
//! hit pavement within 0.2 m of the path height (a miss or a drop is a hole a car falls into),
//! and the top visual surface must be pavement too (terrain or a sidewalk drawn there is a
//! hole or a kerb in the road).
//!
//! `cargo test -p city-import --test junction_cover -- --ignored dump_junctions` writes the
//! junction list `apps/testbed-web/e2e/junction_shots.mjs` renders.

use std::collections::BTreeMap;

use city_import::glb::build_greybox;
use wp_world::geom::{self, V2, V3};
use wp_world::map::SegKind;
use wp_world::surface::node_patches;
use wp_world::CityMap;

/// Spacing of the samples along and across a path, m.
const STEP_M: f32 = 0.5;
/// Samples stay this far inside a path's edges, m.
const INSET_M: f32 = 0.15;

/// Up-facing greybox triangles bucketed by xy, with their material.
struct Visual {
    tris: Vec<([V3; 3], String)>,
    grid: BTreeMap<(i32, i32), Vec<u32>>,
}

impl Visual {
    fn new(map: &CityMap) -> Self {
        let mut tris = Vec::new();
        let mut grid: BTreeMap<(i32, i32), Vec<u32>> = BTreeMap::new();
        for p in build_greybox(map) {
            if p.material == "water" {
                continue;
            }
            for t in p.indices.chunks(3) {
                let v = [
                    p.positions[t[0] as usize],
                    p.positions[t[1] as usize],
                    p.positions[t[2] as usize],
                ];
                let a = [v[1][0] - v[0][0], v[1][1] - v[0][1]];
                let b = [v[2][0] - v[0][0], v[2][1] - v[0][1]];
                if (a[0] * b[1] - a[1] * b[0]).abs() < 1e-4 {
                    continue; // walls, kerb faces
                }
                let id = tris.len() as u32;
                let lo = [
                    v.iter().map(|q| q[0]).fold(f32::INFINITY, f32::min),
                    v.iter().map(|q| q[1]).fold(f32::INFINITY, f32::min),
                ];
                let hi = [
                    v.iter().map(|q| q[0]).fold(f32::NEG_INFINITY, f32::max),
                    v.iter().map(|q| q[1]).fold(f32::NEG_INFINITY, f32::max),
                ];
                for bx in (lo[0] / 8.0).floor() as i32..=(hi[0] / 8.0).floor() as i32 {
                    for by in (lo[1] / 8.0).floor() as i32..=(hi[1] / 8.0).floor() as i32 {
                        grid.entry((bx, by)).or_default().push(id);
                    }
                }
                tris.push((v, p.material.clone()));
            }
        }
        Self { tris, grid }
    }

    /// Highest surface under `p` below `from` (height, material).
    fn top(&self, p: V2, from: f32) -> Option<(f32, &str)> {
        let key = ((p[0] / 8.0).floor() as i32, (p[1] / 8.0).floor() as i32);
        let mut best: Option<(f32, &str)> = None;
        for &id in self.grid.get(&key).map(|v| v.as_slice()).unwrap_or(&[]) {
            let (v, m) = &self.tris[id as usize];
            let (x1, y1, x2, y2, x3, y3) = (v[0][0], v[0][1], v[1][0], v[1][1], v[2][0], v[2][1]);
            let d = (y2 - y3) * (x1 - x3) + (x3 - x2) * (y1 - y3);
            let l1 = ((y2 - y3) * (p[0] - x3) + (x3 - x2) * (p[1] - y3)) / d;
            let l2 = ((y3 - y1) * (p[0] - x3) + (x1 - x3) * (p[1] - y3)) / d;
            let l3 = 1.0 - l1 - l2;
            if l1 < -1e-5 || l2 < -1e-5 || l3 < -1e-5 {
                continue;
            }
            let z = l1 * v[0][2] + l2 * v[1][2] + l3 * v[2][2];
            if z <= from && best.is_none_or(|(bz, _)| z > bz + 1e-4) {
                best = Some((z, m.as_str()));
            }
        }
        best
    }
}

/// Sample points (xy, path height) over the drivable lanes and turning paths of every node
/// patch (junction, bend, dead end), deduplicated on the sample grid.
fn samples(map: &CityMap) -> Vec<(u32, V2, f32)> {
    let mut out = Vec::new();
    for patch in node_patches(map) {
        let c = map.net.nodes[patch.node as usize].pos;
        let reach = patch
            .mesh
            .positions
            .iter()
            .map(|p| geom::dist(geom::xy(*p), geom::xy(c)))
            .fold(0.0f32, f32::max)
            + 4.0;
        let mut seen = std::collections::BTreeSet::new();
        for s in map.segs.iter().filter(|s| s.drivable) {
            if let SegKind::Connector { node, .. } = s.kind {
                if node != patch.node {
                    continue;
                }
            }
            let half = s.width * 0.5 - INSET_M;
            let mut d = 0.0;
            while d <= s.length {
                let (p, dir) = s.point_at(d);
                d += STEP_M;
                if geom::dist(geom::xy(p), geom::xy(c)) > reach || (p[2] - c[2]).abs() > 3.0 {
                    continue;
                }
                let r = geom::right_of(dir);
                let mut lat = -half;
                while lat <= half + 1e-3 {
                    let q = geom::add(geom::xy(p), geom::scale(r, lat));
                    lat += STEP_M;
                    let key = ((q[0] * 4.0).round() as i32, (q[1] * 4.0).round() as i32);
                    if seen.insert(key) {
                        out.push((patch.node, q, p[2]));
                    }
                }
            }
        }
    }
    out
}

fn check(name: &str, map: &CityMap) {
    let visual = Visual::new(map);
    let mut phys = wp_sim::PhysicsWorld::new();
    phys.set_city_ground(map);
    let pts = samples(map);
    let (mut holes, mut not_paved, mut worst) = (Vec::new(), Vec::new(), 0.0f32);
    let mut in_building: Vec<u32> = Vec::new();
    for (node, q, z) in &pts {
        let from = z + 2.0;
        match phys.static_ground_z(*q, from, 40.0) {
            Some(h) if (h - z).abs() <= 0.2 => worst = worst.max((h - z).abs()),
            // The ray starts inside (or just over) a building: Port Vesper lets a few turning
            // paths cross a building lot (nodes 78 and 97, unrelated to the pavement); counted
            // and printed, not a hole.
            Some(h) if h >= from - 0.7 => {
                in_building.push(*node);
                continue;
            }
            other => holes.push(format!(
                "node {node} at ({:.2}, {:.2}): physics {other:?}, path {z:.2}",
                q[0], q[1]
            )),
        }
        match visual.top(*q, from) {
            Some((_, "asphalt" | "highway_concrete" | "parking" | "bus_lane")) => {}
            other => not_paved.push(format!(
                "node {node} at ({:.2}, {:.2}): greybox {other:?}, path {z:.3}",
                q[0], q[1]
            )),
        }
    }
    println!(
        "{name}: {} samples over {} node patches; physics within {worst:.3} m of the paths; \
         {} misses, {} not drawn as pavement; {} in a building at nodes {:?}",
        pts.len(),
        node_patches(map).len(),
        holes.len(),
        not_paved.len(),
        in_building.len(),
        in_building
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
    );
    assert!(pts.len() > 100_000, "{name}: {} samples", pts.len());
    assert!(
        holes.is_empty(),
        "{name}: {:?}",
        &holes[..holes.len().min(10)]
    );
    assert!(
        not_paved.is_empty(),
        "{name}: {:?}",
        &not_paved[..not_paved.len().min(10)]
    );
}

#[test]
fn vesper_v2_junctions_have_no_holes() {
    let net = wp_world::vesper::generate(&wp_world::vesper::VesperConfig::default());
    check("vesper_v2", &CityMap::new(net));
}

#[test]
fn grid_junctions_have_no_holes() {
    let net = wp_world::procgen::generate(&wp_world::procgen::ProcGenConfig::default());
    check("grid", &CityMap::new(net));
}

/// Writes `target/junctions_<city>.json` (`[{node, kind, x, y, z, r, cam}]`): every node
/// patch with its centre, reach and a street-level camera on an arriving lane, for
/// `apps/testbed-web/e2e/junction_shots.mjs`.
#[test]
#[ignore = "writes the junction list for the screenshot script"]
fn dump_junctions() {
    for (name, map) in [
        (
            "vesper_v2",
            CityMap::new(wp_world::vesper::generate(
                &wp_world::vesper::VesperConfig::default(),
            )),
        ),
        (
            "procedural_12x12",
            CityMap::new(wp_world::procgen::generate(
                &wp_world::procgen::ProcGenConfig::default(),
            )),
        ),
    ] {
        let net = &map.net;
        let mut rows = Vec::new();
        for p in node_patches(&map) {
            let c = net.nodes[p.node as usize].pos;
            let r = p
                .mesh
                .positions
                .iter()
                .map(|q| geom::dist(geom::xy(*q), geom::xy(c)))
                .fold(0.0f32, f32::max);
            let kind = match p.kind {
                wp_world::surface::NodePatchKind::Junction => "junction",
                wp_world::surface::NodePatchKind::Bend => "bend",
                wp_world::surface::NodePatchKind::DeadEnd => "deadend",
            };
            // Street-level camera 14 m out on the first arriving lane.
            let cam = net
                .lanes
                .iter()
                .find(|l| l.to_node == p.node)
                .map(|l| {
                    let cum = geom::cumulative(&l.polyline);
                    let s = (cum[cum.len() - 1] - (r + 14.0)).max(0.0);
                    let (q, _) = geom::point_at(&l.polyline, &cum, s);
                    format!(",\"cam\":[{},{},{}]", q[0], q[1], q[2] + 1.2)
                })
                .unwrap_or_default();
            rows.push(format!(
                "{{\"node\":{},\"kind\":\"{kind}\",\"x\":{},\"y\":{},\"z\":{},\"r\":{r}{cam}}}",
                p.node, c[0], c[1], c[2]
            ));
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("junctions_{name}.json"));
        std::fs::write(&path, format!("[{}]\n", rows.join(",\n"))).unwrap();
        println!("wrote {}", path.display());
    }
}

/// Largest grade change across a stop line (0.5 m windows, 1 m apart). Port Vesper's worst is
/// 6.9 % (junction 3, a street arriving on a 3.4 % grade next to a highway corner); a car at
/// 120 km/h stays on the ground across every approach above 3 %
/// (`wp_sim/tests/stop_lines.rs`).
const MAX_KINK: f32 = 0.08;

/// Playtest #51 ("the painted lines at the traffic lights make the car jump"): the surface a
/// car drives over at every junction entry, sampled by rays through the `wp_sim` colliders
/// every 5 cm from 3 m before each arriving lane's end (the stop line) to 3 m after it along
/// its turning paths, on the lane centre and 0.5 m inside each edge. No step > 1 cm between
/// samples, no grade change above [`MAX_KINK`]. (The old flat junction disc left 8–15 cm
/// steps right at the stop lines.)
fn check_stop_lines(name: &str, map: &CityMap) {
    let mut phys = wp_sim::PhysicsWorld::new();
    phys.set_city_ground(map);
    let (mut profiles, mut bad, mut worst_step, mut worst_kink) = (0usize, Vec::new(), 0f32, 0f32);
    let overlapping: Vec<u32> = node_patches(map)
        .iter()
        .filter(|p| p.overlapping_ends)
        .map(|p| p.node)
        .collect();
    for s in map.segs.iter().filter(|s| s.drivable && s.is_connector()) {
        let SegKind::Connector { from, node, .. } = s.kind else {
            continue;
        };
        // Ribbons lying on the patch at their own heights (known, see the junctions doc).
        if overlapping.contains(&node) {
            continue;
        }
        let lane = &map.segs[from as usize];
        for lat in [-(s.width * 0.5 - 0.5), 0.0, s.width * 0.5 - 0.5] {
            // Path: the last 3 m of the lane, then 3 m of the connector.
            let mut pts: Vec<(V2, f32)> = Vec::new();
            let mut d = -3.0f32;
            while d <= 3.0 + 1e-3 {
                let (p, dir) = if d < 0.0 {
                    lane.point_at((lane.length + d).max(0.0))
                } else {
                    s.point_at(d.min(s.length))
                };
                pts.push((
                    geom::add(geom::xy(p), geom::scale(geom::right_of(dir), lat)),
                    p[2],
                ));
                d += 0.05;
            }
            let zs: Vec<Option<f32>> = pts
                .iter()
                .map(|(q, z)| {
                    // Highest of a 3 mm cluster: a single ray exactly on an edge shared by two
                    // collider tiles can slip between them; a crack wider than ~6 mm cannot.
                    [
                        [0.0, 0.0],
                        [0.003, 0.0],
                        [-0.003, 0.0],
                        [0.0, 0.003],
                        [0.0, -0.003],
                    ]
                    .iter()
                    .filter_map(|d| phys.static_ground_z([q[0] + d[0], q[1] + d[1]], z + 1.0, 3.0))
                    .filter(|h| (h - z).abs() < 0.5)
                    .reduce(f32::max)
                })
                .collect();
            if zs.iter().any(|z| z.is_none()) {
                continue; // a building lot over the path (see the hole test)
            }
            let zs: Vec<f32> = zs.into_iter().flatten().collect();
            profiles += 1;
            let gap = |a: usize, b: usize| geom::dist(pts[a].0, pts[b].0);
            for k in 1..zs.len() {
                // Where the lane hands over to a tight turning path the offset line jumps
                // sideways: compare only neighbouring samples.
                if gap(k - 1, k) > 0.07 {
                    continue;
                }
                let step = (zs[k] - zs[k - 1]).abs();
                worst_step = worst_step.max(step);
                if step > 0.01 {
                    bad.push(format!("step {step:.3} m at {:?}", pts[k].0));
                }
            }
            // Grade over 0.5 m windows, compared 1 m apart (over true distances).
            for k in 10..zs.len() - 10 {
                let (d1, d2) = (gap(k - 10, k), gap(k, k + 10));
                if !(0.45..0.55).contains(&d1) || !(0.45..0.55).contains(&d2) {
                    continue;
                }
                let g1 = (zs[k] - zs[k - 10]) / d1;
                let g2 = (zs[k + 10] - zs[k]) / d2;
                let kink = (g2 - g1).abs();
                worst_kink = worst_kink.max(kink);
                if kink > MAX_KINK {
                    bad.push(format!(
                        "grade change {:.1} % at {:?}",
                        kink * 100.0,
                        pts[k].0
                    ));
                }
            }
        }
    }
    println!(
        "{name}: {profiles} stop-line profiles, worst step {worst_step:.4} m, worst grade change {:.2} %",
        worst_kink * 100.0
    );
    assert!(profiles > 1000, "{name}: {profiles}");
    assert!(bad.is_empty(), "{name}: {bad:?}");
}

#[test]
fn vesper_v2_stop_lines_are_smooth() {
    let net = wp_world::vesper::generate(&wp_world::vesper::VesperConfig::default());
    check_stop_lines("vesper_v2", &CityMap::new(net));
}

#[test]
fn grid_stop_lines_are_smooth() {
    let net = wp_world::procgen::generate(&wp_world::procgen::ProcGenConfig::default());
    check_stop_lines("grid", &CityMap::new(net));
}
