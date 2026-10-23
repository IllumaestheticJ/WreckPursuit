//! Owner playtest row 162: ramp merges are flush and level with the carriageway and nothing
//! stands in them. Along every ramp → carriageway merge in Port Vesper (interchange and
//! rest-stop ramps; `wp_world::surface::ramp_merges`), on the car's track (the lane centre and
//! 0.8 m either side), every 0.25 m:
//!
//! - the drawn greybox's top surface is pavement and the physics drivable ground matches it
//!   (within 3 cm);
//! - neither has a step: from one sample to the next, the surface changes height by less
//!   than 2 cm more than the lane's own profile does;
//! - nothing solid stands over the physics ground (a barrier, rail or wall in the path).

use std::collections::BTreeMap;

use city_import::glb::build_greybox;
use wp_world::geom::{self, V2, V3};
use wp_world::CityMap;

/// Largest height step between neighbouring samples beyond the lane profile's own change, m.
const MAX_STEP_M: f32 = 0.02;
/// Sample spacing along the path, m.
const DS: f32 = 0.25;
/// Paved greybox materials.
const PAVED: [&str; 4] = ["asphalt", "highway_concrete", "ramp_taper", "parking"];

/// Greybox triangles bucketed by 8 m cells (by bounding box).
struct Drawn {
    tris: Vec<([V3; 3], String)>,
    cells: BTreeMap<(i32, i32), Vec<u32>>,
}

const CELL_M: f32 = 8.0;

fn cell(q: V2) -> (i32, i32) {
    (
        (q[0] / CELL_M).floor() as i32,
        (q[1] / CELL_M).floor() as i32,
    )
}

impl Drawn {
    fn new(map: &CityMap) -> Self {
        let mut tris = Vec::new();
        for p in build_greybox(map) {
            for t in p.indices.chunks(3) {
                let v = [0, 1, 2].map(|k| p.positions[t[k] as usize]);
                tris.push((v, p.material.clone()));
            }
        }
        let mut cells: BTreeMap<(i32, i32), Vec<u32>> = BTreeMap::new();
        for (i, (v, _)) in tris.iter().enumerate() {
            let (lo, hi) = (
                cell([
                    v.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min),
                    v.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min),
                ]),
                cell([
                    v.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max),
                    v.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max),
                ]),
            );
            if (hi.0 - lo.0) * (hi.1 - lo.1) > 400 {
                continue; // terrain-sized sheet (sea plane): never the road surface
            }
            for x in lo.0..=hi.0 {
                for y in lo.1..=hi.1 {
                    cells.entry((x, y)).or_default().push(i as u32);
                }
            }
        }
        Self { tris, cells }
    }

    /// Highest drawn surface at `q` below `top`: (height, material).
    fn top(&self, q: V2, top: f32) -> Option<(f32, &str)> {
        let mut best: Option<(f32, &str)> = None;
        for &i in self.cells.get(&cell(q)).into_iter().flatten() {
            let (v, m) = &self.tris[i as usize];
            if let Some(z) = z_on(v, q) {
                if z < top && best.is_none_or(|(b, _)| z > b) {
                    best = Some((z, m.as_str()));
                }
            }
        }
        best
    }
}

/// Height of triangle `v` over `q` (barycentric), if `q` is inside it and it is not vertical.
fn z_on(v: &[V3; 3], q: V2) -> Option<f32> {
    let (a, b, c) = (geom::xy(v[0]), geom::xy(v[1]), geom::xy(v[2]));
    let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if d.abs() < 1e-9 {
        return None;
    }
    let w0 = ((b[1] - c[1]) * (q[0] - c[0]) + (c[0] - b[0]) * (q[1] - c[1])) / d;
    let w1 = ((c[1] - a[1]) * (q[0] - c[0]) + (a[0] - c[0]) * (q[1] - c[1])) / d;
    let w2 = 1.0 - w0 - w1;
    let e = -1e-5;
    (w0 >= e && w1 >= e && w2 >= e).then(|| w0 * v[0][2] + w1 * v[1][2] + w2 * v[2][2])
}

#[test]
fn vesper_ramp_merges_are_flush_and_clear() {
    let map = CityMap::new(wp_world::vesper::generate(
        &wp_world::vesper::VesperConfig::default(),
    ));
    let merges = wp_world::surface::ramp_merges(&map, 60.0, 80.0);
    // Interchange ramps onto Loop 9 and the rest stops' exit ramps.
    assert!(merges.len() >= 10, "{} merges", merges.len());
    let drawn = Drawn::new(&map);
    let mut phys = wp_sim::PhysicsWorld::new();
    phys.set_city_ground(&map);
    let (mut samples, mut worst_step, mut worst_gap) = (0usize, 0.0f32, 0.0f32);
    let mut bad: Vec<String> = Vec::new();
    for m in &merges {
        let cum = geom::cumulative(&m.path);
        let len = cum[cum.len() - 1];
        for lat in [-0.8f32, 0.0, 0.8] {
            let mut prev: Option<(f32, f32, f32)> = None; // (path z, physics z, drawn z)
            let mut s = 0.0;
            while s <= len {
                let (p, d) = geom::point_at(&m.path, &cum, s);
                let q = geom::add(geom::xy(p), geom::scale(geom::right_of(d), lat));
                s += DS;
                let Some(g) = phys.drivable_ground_z(q, p[2] + 1.0, 2.0) else {
                    bad.push(format!(
                        "connector {}: no physics ground at {q:?}",
                        m.connector
                    ));
                    continue;
                };
                // Nothing solid over the ground: the first static hit from 3 m up is it.
                let solid = phys.static_ground_z(q, p[2] + 3.0, 4.0);
                if solid.is_some_and(|z| z > g + 0.01) {
                    bad.push(format!(
                        "connector {}: obstacle {:.2} m over the road at {q:?}",
                        m.connector,
                        solid.unwrap() - g
                    ));
                }
                let Some((dz, mat)) = drawn.top(q, p[2] + 3.0) else {
                    bad.push(format!("connector {}: nothing drawn at {q:?}", m.connector));
                    continue;
                };
                if !PAVED.contains(&mat) {
                    bad.push(format!(
                        "connector {}: {mat} drawn in the path at {q:?}",
                        m.connector
                    ));
                }
                worst_gap = worst_gap.max((dz - g).abs());
                if (dz - g).abs() > 0.03 {
                    bad.push(format!(
                        "connector {}: drawn {dz:.3} vs physics {g:.3} at {q:?}",
                        m.connector
                    ));
                }
                if let Some((pz, pg, pd)) = prev {
                    let lane = p[2] - pz;
                    for (what, step) in [("physics", g - pg - lane), ("drawn", dz - pd - lane)] {
                        worst_step = worst_step.max(step.abs());
                        if step.abs() >= MAX_STEP_M {
                            bad.push(format!(
                                "connector {}: {what} step {step:.3} m at {q:?}",
                                m.connector
                            ));
                        }
                    }
                }
                prev = Some((p[2], g, dz));
                samples += 1;
            }
        }
    }
    println!(
        "{} merges, {samples} samples: worst step {worst_step:.3} m, worst drawn-physics {worst_gap:.3} m, {} problems",
        merges.len(),
        bad.len()
    );
    assert!(
        bad.is_empty(),
        "{} problems, first: {:?}",
        bad.len(),
        &bad[..bad.len().min(12)]
    );
}

/// Row 162 for through traffic (opponent-pace: an AI car at Loop 9 race speed hit "world
/// geometry" mid-carriageway at the Hollins rest off-ramp gore): over every freeway taper (the
/// connector strips where a ramp leaves or joins a carriageway, merges and diverges alike)
/// the physics ground has no step between neighbouring samples 0.25 m apart anywhere on the
/// paving (0.3 m inside its edges): a level strip across a banked carriageway stood up to ~11
/// cm proud of it.
#[test]
fn vesper_carriageways_are_level_over_every_ramp_taper() {
    use wp_world::map::SegKind;
    let map = CityMap::new(wp_world::vesper::generate(
        &wp_world::vesper::VesperConfig::default(),
    ));
    let net = &map.net;
    let mut phys = wp_sim::PhysicsWorld::new();
    phys.set_city_ground(&map);
    let lanes_of = |road: u32| net.lanes.iter().filter(|l| l.road == road).count();
    let mut tapers = 0;
    let (mut samples, mut worst) = (0usize, 0.0f32);
    let mut bad = Vec::new();
    for (ci, c) in map.segs.iter().enumerate() {
        let SegKind::Connector { from, to, .. } = c.kind else {
            continue;
        };
        let (rf, rt) = (net.lanes[from as usize].road, net.lanes[to as usize].road);
        let highway = |r: u32| net.surfaces.iter().any(|s| s.road == r && s.highway);
        if !highway(rf) || !highway(rt) || lanes_of(rf) == lanes_of(rt) {
            continue;
        }
        tapers += 1;
        let mut s = 0.0;
        while s <= c.length {
            let (p, d) = c.point_at(s);
            let r = geom::right_of(d);
            s += 2.0;
            for k in -40..=40 {
                let q = geom::add(geom::xy(p), geom::scale(r, k as f32 * 0.25));
                let paved = |x: V2, z: f32| {
                    wp_world::surface::inside_other_paving(net, usize::MAX, [x[0], x[1], z])
                };
                let Some(g) = phys.drivable_ground_z(q, p[2] + 1.5, 3.0) else {
                    continue;
                };
                if !paved(q, g) {
                    continue;
                }
                for n in [
                    geom::add(q, geom::scale(d, 0.25)),
                    geom::add(q, geom::scale(r, 0.25)),
                ] {
                    let Some(h) = phys.drivable_ground_z(n, p[2] + 1.5, 3.0) else {
                        continue;
                    };
                    if !paved(n, h) {
                        continue;
                    }
                    samples += 1;
                    // Grades and banks change height by at most ~2 cm over 0.25 m.
                    let step = (h - g).abs();
                    worst = worst.max(step);
                    if step >= 0.035 {
                        bad.push(format!("taper {ci}: step {step:.3} m at {q:?}"));
                    }
                }
            }
        }
    }
    println!(
        "{tapers} tapers, {samples} sample pairs: worst {worst:.3} m, {} steps",
        bad.len()
    );
    assert!(tapers >= 20, "{tapers} tapers");
    assert!(
        bad.is_empty(),
        "{} steps, first: {:?}",
        bad.len(),
        &bad[..bad.len().min(12)]
    );
}
