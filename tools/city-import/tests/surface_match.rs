//! Visual vs physics surface sweep (playtest #20, "I can fall into the sidewalk").
//!
//! Samples the greybox mesh the player sees (`glb::build_greybox`) against downward raycasts
//! through the `wp_sim` city colliders (`PhysicsWorld::set_city_ground`) at thousands of
//! points over road edges, kerbs, sidewalks and junction corners, in Port Vesper v2 and the
//! 12×12 grid. Wherever the top visual surface is paved or a sidewalk, physics must agree
//! within 3 cm; wherever the player sees terrain next to a road, physics must not be lower.

use std::collections::BTreeMap;

use city_import::glb::{build_greybox, Prim};
use wp_world::geom::{self, V2, V3};
use wp_world::network::LaneKind;
use wp_world::CityMap;

/// Agreement tolerance, m.
const TOL_M: f32 = 0.03;
/// Spatial bucket size of the visual triangle index, m.
const BUCKET_M: f32 = 8.0;

/// Visual surface kinds the sweep distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Paved,
    Sidewalk,
    Terrain,
    Other,
}

fn kind_of(material: &str) -> Kind {
    match material {
        "asphalt" | "highway_concrete" | "parking" | "bus_lane" => Kind::Paved,
        "sidewalk" => Kind::Sidewalk,
        m if m.starts_with("ground_") || m == "rock" || m == "seabed" => Kind::Terrain,
        _ => Kind::Other,
    }
}

/// Up-facing visual triangles bucketed by xy.
struct Visual {
    tris: Vec<([V3; 3], Kind)>,
    grid: BTreeMap<(i32, i32), Vec<u32>>,
}

impl Visual {
    fn new(prims: &[Prim]) -> Self {
        let mut tris = Vec::new();
        let mut grid: BTreeMap<(i32, i32), Vec<u32>> = BTreeMap::new();
        for p in prims {
            let kind = kind_of(&p.material);
            if p.material == "water" {
                continue;
            }
            for t in p.indices.chunks(3) {
                let v = [
                    p.positions[t[0] as usize],
                    p.positions[t[1] as usize],
                    p.positions[t[2] as usize],
                ];
                // Skip walls, kerb faces, parapets (near-vertical) and degenerate triangles.
                let a = [v[1][0] - v[0][0], v[1][1] - v[0][1]];
                let b = [v[2][0] - v[0][0], v[2][1] - v[0][1]];
                let area2 = (a[0] * b[1] - a[1] * b[0]).abs();
                if area2 < 1e-4 {
                    continue;
                }
                let id = tris.len() as u32;
                tris.push((v, kind));
                let lo = [
                    v.iter().map(|q| q[0]).fold(f32::INFINITY, f32::min),
                    v.iter().map(|q| q[1]).fold(f32::INFINITY, f32::min),
                ];
                let hi = [
                    v.iter().map(|q| q[0]).fold(f32::NEG_INFINITY, f32::max),
                    v.iter().map(|q| q[1]).fold(f32::NEG_INFINITY, f32::max),
                ];
                for bx in (lo[0] / BUCKET_M).floor() as i32..=(hi[0] / BUCKET_M).floor() as i32 {
                    for by in (lo[1] / BUCKET_M).floor() as i32..=(hi[1] / BUCKET_M).floor() as i32
                    {
                        grid.entry((bx, by)).or_default().push(id);
                    }
                }
            }
        }
        Self { tris, grid }
    }

    /// Highest visual surface at `p` below `from_z` (height, kind), counting only triangles
    /// that contain `p` by at least ~1 mm.
    fn top(&self, p: V2, from_z: f32) -> Option<(f32, Kind)> {
        self.top_with(p, from_z, 1e-4)
    }

    /// [`Visual::top`] counting triangles whose outline `p` touches (within ~0.1 mm).
    fn top_touching(&self, p: V2, from_z: f32) -> Option<(f32, Kind)> {
        self.top_with(p, from_z, -1e-5)
    }

    fn top_with(&self, p: V2, from_z: f32, inside: f32) -> Option<(f32, Kind)> {
        let key = (
            (p[0] / BUCKET_M).floor() as i32,
            (p[1] / BUCKET_M).floor() as i32,
        );
        let mut best: Option<(f32, Kind)> = None;
        for &id in self.grid.get(&key).map(|v| v.as_slice()).unwrap_or(&[]) {
            let (v, kind) = self.tris[id as usize];
            if let Some(z) = tri_z(&v, p, inside) {
                // Paved/sidewalk win ties with terrain drawn just under them.
                if z <= from_z && best.is_none_or(|(bz, _)| z > bz + 1e-4) {
                    best = Some((z, kind));
                }
            }
        }
        best
    }
}

/// Height of triangle `v` at `p` if `p` is inside its xy projection by at least `inside`
/// (barycentric; negative accepts points just outside).
fn tri_z(v: &[V3; 3], p: V2, inside: f32) -> Option<f32> {
    let (x1, y1, x2, y2, x3, y3) = (v[0][0], v[0][1], v[1][0], v[1][1], v[2][0], v[2][1]);
    let d = (y2 - y3) * (x1 - x3) + (x3 - x2) * (y1 - y3);
    if d.abs() < 1e-9 {
        return None;
    }
    let l1 = ((y2 - y3) * (p[0] - x3) + (x3 - x2) * (p[1] - y3)) / d;
    let l2 = ((y3 - y1) * (p[0] - x3) + (x1 - x3) * (p[1] - y3)) / d;
    let l3 = 1.0 - l1 - l2;
    if l1 < inside || l2 < inside || l3 < inside {
        return None;
    }
    Some(l1 * v[0][2] + l2 * v[1][2] + l3 * v[2][2])
}

/// One sample: where, what the player sees there, what physics says.
#[derive(Debug, Clone, Copy)]
struct Sample {
    p: V2,
    visual: f32,
    kind: Kind,
    /// Top visual height counting outlines the sample touches (a sample exactly on the edge of
    /// a sidewalk cell may hit either surface).
    visual_touching: f32,
    physics: Option<f32>,
    /// Kerb line (within 5 cm of a paved edge with a sidewalk): the kerb step is allowed.
    at_kerb: bool,
}

/// Sample points: across every sidewalk-edged lane (road side, kerb line, sidewalk, past the
/// sidewalk) every 2 m, plus a 1 m grid around every junction (corners).
fn sample_points(map: &CityMap) -> Vec<(V2, f32, bool)> {
    let net = &map.net;
    let mut pts = Vec::new();
    for l in &net.lanes {
        if l.kind == LaneKind::Highway || !(l.edge_left || l.edge_right) {
            continue;
        }
        let half = l.width_m * 0.5;
        let cum = geom::cumulative(&l.polyline);
        let total = *cum.last().unwrap();
        let mut s = 0.5;
        while s < total - 0.5 {
            let k = cum.partition_point(|c| *c <= s).clamp(1, cum.len() - 1);
            let (a, b) = (l.polyline[k - 1], l.polyline[k]);
            let t = (s - cum[k - 1]) / (cum[k] - cum[k - 1]).max(1e-6);
            let c = [
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            ];
            let dir = geom::normalize(geom::sub(geom::xy(b), geom::xy(a)));
            let r = geom::right_of(dir);
            for (side, on) in [(1.0f32, l.edge_right), (-1.0, l.edge_left)] {
                if !on {
                    continue;
                }
                for off in [
                    -0.6f32, -0.15, -0.03, 0.03, 0.15, 0.4, 1.0, 1.7, 2.4, 2.9, 3.4,
                ] {
                    let lat = side * (half + off);
                    let p = geom::add(geom::xy(c), geom::scale(r, lat));
                    pts.push((p, c[2] + 2.5, off.abs() < 0.05));
                }
            }
            s += 2.0;
        }
    }
    for ix in &net.intersections {
        let c = net.nodes[ix.node as usize].pos;
        let reach = ix.radius_m + 4.0;
        let n = (reach * 2.0) as i32;
        for j in 0..=n {
            for i in 0..=n {
                // Off exact metre lines: a ray exactly on a heightfield grid line can slip
                // through Parry's cell test (measure zero, never a wheel in motion).
                let p = [
                    c[0] - reach + i as f32 + 0.371,
                    c[1] - reach + j as f32 + 0.137,
                ];
                pts.push((p, c[2] + 2.5, false));
            }
        }
    }
    pts
}

/// Whether `p` is within 0.25 m of a deck guard rail (the visible parapet: physics hits the
/// rail top there, the greybox draws it as a wall without a top face).
fn near_rail(rails: &[(V3, V3)], p: V2) -> bool {
    rails
        .iter()
        .any(|(a, b)| geom::point_segment_distance(p, geom::xy(*a), geom::xy(*b)) < 0.25)
}

fn sweep(map: &CityMap) -> Vec<Sample> {
    let visual = Visual::new(&build_greybox(map));
    let rails = wp_world::surface::deck_rails(&map.net);
    let mut phys = wp_sim::PhysicsWorld::new();
    phys.set_city_ground(map);
    sample_points(map)
        .into_iter()
        .filter_map(|(p, from, at_kerb)| {
            if near_rail(&rails, p) {
                return None;
            }
            let (z, kind) = visual.top(p, from)?;
            let touching = visual.top_touching(p, from).map_or(z, |t| t.0);
            let physics = phys.static_ground_z(p, from, 80.0);
            // Ray starting inside a building (junction-corner grid): nothing to compare.
            if physics.is_some_and(|h| h >= from - 1e-3) {
                return None;
            }
            Some(Sample {
                p,
                visual: z,
                kind,
                visual_touching: touching,
                physics,
                at_kerb,
            })
        })
        .collect()
}

/// Report per kind and return the failures.
fn check(name: &str, samples: &[Sample]) -> Vec<String> {
    let mut fails = Vec::new();
    let mut stats: BTreeMap<Kind, (usize, f32, f32)> = BTreeMap::new();
    for s in samples {
        let err = s.physics.map_or(-99.0, |z| z - s.visual);
        let e = stats.entry(s.kind).or_insert((0, 0.0, 0.0));
        e.0 += 1;
        e.1 = e.1.min(err);
        e.2 = e.2.max(err);
        let tol = if s.at_kerb { 0.16 } else { TOL_M };
        let err_touching = s.physics.map_or(-99.0, |z| z - s.visual_touching);
        let bad = match s.kind {
            Kind::Paved | Kind::Sidewalk => err.abs() > tol && err_touching.abs() > tol,
            // Terrain the player sees next to a road: physics must not be lower.
            Kind::Terrain => err < -TOL_M,
            Kind::Other => false,
        };
        if bad {
            fails.push(format!(
                "{name}: {:?} at ({:.1}, {:.1}): visual {:.3}, physics {:?} ({err:+.3})",
                s.kind, s.p[0], s.p[1], s.visual, s.physics
            ));
        }
    }
    for (k, (n, lo, hi)) in &stats {
        println!("{name}: {k:?}: {n} samples, physics − visual in [{lo:+.3}, {hi:+.3}] m");
    }
    println!("{name}: {} of {} samples off", fails.len(), samples.len());
    fails
}

fn assert_clean(name: &str, map: &CityMap) {
    let samples = sweep(map);
    assert!(
        samples.len() > 5000,
        "{name}: only {} samples",
        samples.len()
    );
    let fails = check(name, &samples);
    for f in fails.iter().take(40) {
        println!("{f}");
    }
    assert!(fails.is_empty(), "{name}: {} samples disagree", fails.len());
}

#[test]
fn vesper_v2_physics_matches_the_visual_surface() {
    let net = wp_world::vesper::generate(&wp_world::vesper::VesperConfig::default());
    assert_clean("vesper_v2", &CityMap::new(net));
}

#[test]
fn grid_physics_matches_the_visual_surface() {
    let net = wp_world::procgen::generate(&wp_world::procgen::ProcGenConfig::default());
    assert_clean("grid", &CityMap::new(net));
}
