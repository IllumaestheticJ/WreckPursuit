//! Playtest #52 ("the sidewalk should be at the same height as the base of the buildings —
//! otherwise it drops wheels too easily"): behind every kerb the raised surface is one flush
//! band — the sidewalk, then at least `VERGE_W_M` of verge, or all the way to the facade where a
//! building stands further back — with no drop between the sidewalk and the building wall.
//!
//! Along every sidewalk strip of Port Vesper v2 and the 12×12 grid, cross-sections every ~2 m:
//! rays go straight down through the `wp_sim` colliders every 5 cm from just behind the kerb
//! out to the band's outer edge (or to 10 cm short of a facade). The surface never changes
//! height by more than 2 cm between samples (so the ground at a facade is the sidewalk's level,
//! carried on with the road's grade and bank), the greybox draws sidewalk there within 3 cm,
//! and a building fronting the band starts at or below it (never floats). Bands cut short by
//! another road (no facade, narrower than `FLUSH_W_M`) are counted. Sections under another
//! road's pavement, or within 80 m of Port Vesper node 81 (two roads leaving 6° apart, whose
//! ribbons overlap: a road-layout issue), are skipped and counted.

use std::collections::BTreeMap;

use city_import::glb::build_greybox;
use wp_world::geom::{self, V2, V3};
use wp_world::surface::{sidewalk_mesh, FLUSH_W_M};
use wp_world::CityMap;

/// Highest greybox surface material under `p` below `from` (up-facing triangles only).
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
                    continue;
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

/// First facade along `p + t·d` (unit `d`) within `max`: distance and the building's base.
fn facade(map: &CityMap, p: V2, d: V2, max: f32) -> Option<(f32, f32)> {
    let mut best: Option<(f32, f32)> = None;
    for b in &map.net.buildings {
        let fp = &b.footprint;
        if geom::dist(fp[0], p) > max + 80.0 {
            continue;
        }
        for k in 0..fp.len() {
            let (a, c) = (fp[k], fp[(k + 1) % fp.len()]);
            let e = geom::sub(c, a);
            let den = geom::cross(d, e);
            if den.abs() < 1e-9 {
                continue;
            }
            let w = geom::sub(a, p);
            let t = geom::cross(w, e) / den;
            let u = geom::cross(w, d) / den;
            if (0.0..=1.0).contains(&u) && t > 0.0 && t <= max && best.is_none_or(|x| t < x.0) {
                best = Some((t, b.base_z_m));
            }
        }
    }
    best
}

fn check(name: &str, map: &CityMap) {
    let walks = sidewalk_mesh(map);
    let visual = Visual::new(map);
    let mut phys = wp_sim::PhysicsWorld::new();
    phys.set_city_ground(map);
    let (mut rays, mut frontages, mut narrow, mut fails) = (0usize, 0usize, 0usize, Vec::new());
    let mut under_road = 0usize;
    let overlapping: Vec<V2> = wp_world::surface::node_patches(map)
        .iter()
        .filter(|p| p.overlapping_ends)
        .map(|p| geom::xy(map.net.nodes[p.node as usize].pos))
        .collect();
    let mut worst_step = 0.0f32;
    let mut worst_front = 0.0f32;
    for band in &walks.bands {
        // Cross-sections every ~2 m, 1 m clear of the strip's ends (corner fills' business).
        let lerp = |a: V3, b: V3, f: f32| {
            [
                a[0] + (b[0] - a[0]) * f,
                a[1] + (b[1] - a[1]) * f,
                a[2] + (b[2] - a[2]) * f,
            ]
        };
        let kerb_line: Vec<V3> = band.iter().map(|[k, _]| *k).collect();
        let cum = geom::cumulative(&kerb_line);
        let total = cum[cum.len() - 1];
        let mut sections: Vec<[V3; 2]> = Vec::new();
        for k in 0..band.len().saturating_sub(1) {
            let seg = cum[k + 1] - cum[k];
            let n = (seg / 2.0).ceil().max(1.0) as usize;
            for i in 0..n {
                let f = i as f32 / n as f32;
                let s = cum[k] + seg * f;
                if s < 1.0 || total - s < 1.0 {
                    continue;
                }
                sections.push([
                    lerp(band[k][0], band[k + 1][0], f),
                    lerp(band[k][1], band[k + 1][1], f),
                ]);
            }
        }
        for [kerb, outer] in sections.iter() {
            let len = geom::dist(geom::xy(*kerb), geom::xy(*outer));
            if len < 0.5 {
                continue;
            }
            let d = geom::scale(geom::sub(geom::xy(*outer), geom::xy(*kerb)), 1.0 / len);
            let front = facade(map, geom::xy(*kerb), d, len + 2.0);
            let end = match front {
                Some((f, _)) => (f - 0.1).min(len - 0.05),
                None => len - 0.05,
            };
            if front.is_none() && len < FLUSH_W_M - 0.05 {
                narrow += 1; // cut short by another road or its band
            }
            let plane = |t: f32| kerb[2] + (outer[2] - kerb[2]) * t / len;
            // A sidewalk lying under another road's pavement: Port Vesper node 81, where two
            // roads leave 6° apart and their ribbons overlap (a road-layout issue, counted).
            let q0 = geom::add(geom::xy(*kerb), geom::scale(d, 0.3));
            if overlapping
                .iter()
                .any(|c| geom::dist(*c, geom::xy(*kerb)) < 80.0)
                || matches!(
                    visual.top(q0, kerb[2] + 1.0),
                    Some((_, "asphalt" | "parking" | "bus_lane" | "highway_concrete"))
                )
            {
                under_road += 1;
                continue;
            }
            rays += 1;
            let mut prev: Option<f32> = None;
            let mut t = 0.3;
            while t <= end {
                let q = geom::add(geom::xy(*kerb), geom::scale(d, t));
                let z = plane(t);
                // Of a 3 mm cluster, the hit nearest the band plane: a single ray exactly on an edge shared by two
                // collider tiles can slip through, or catch the top of a vertical kerb face.
                let mut hs: Vec<f32> = [
                    [0.0, 0.0],
                    [0.003, 0.0],
                    [-0.003, 0.0],
                    [0.0, 0.003],
                    [0.0, -0.003],
                ]
                .iter()
                .filter_map(|o| phys.static_ground_z([q[0] + o[0], q[1] + o[1]], z + 1.0, 3.0))
                .filter(|h| (h - z).abs() < 0.6)
                .collect();
                hs.sort_by(|a, b| (a - z).abs().total_cmp(&(b - z).abs()));
                let h = hs.first().copied();
                let Some(h) = h else {
                    fails.push(format!(
                        "{name}: nothing under the band at {q:?} (band {z:.2}, first hit {:?})",
                        phys.static_ground_z(q, z + 30.0, 60.0)
                    ));
                    break;
                };
                if let Some(p) = prev {
                    worst_step = worst_step.max((h - p).abs());
                    if (h - p).abs() > 0.02 {
                        fails.push(format!(
                            "{name}: {:+.3} m step at {q:?} (from {p:.3})",
                            h - p
                        ));
                        break;
                    }
                }
                match visual.top(q, z + 1.0) {
                    Some((vz, "sidewalk")) if (vz - h).abs() < 0.03 => {}
                    other => {
                        fails.push(format!(
                            "{name}: greybox {other:?} at {q:?}, physics {h:.3}"
                        ));
                        break;
                    }
                }
                prev = Some(h);
                t += 0.05;
            }
            if let (Some((f, base)), Some(h)) = (front, prev) {
                if f <= len + 0.05 && t > end {
                    frontages += 1;
                    // How far the ground at the wall is off the straight line across the band
                    // (a band quad twisted by the kerb line's grade and bank changes; any real
                    // step on the way has failed above).
                    worst_front = worst_front.max((h - plane(end)).abs());
                    if base > h + 0.02 {
                        fails.push(format!(
                            "{name}: frontage at {:?}: ground {h:.3}, sidewalk plane {:.3}, \
                             building base {base:.3}",
                            geom::add(geom::xy(*kerb), geom::scale(d, f)),
                            plane(end)
                        ));
                    }
                }
            }
        }
    }
    println!(
        "{name}: {rays} band cross-sections, {frontages} frontages; worst step {worst_step:.4} m, \
         ground at the wall at most {worst_front:.4} m off the straight line across the band; {narrow} cut short by another road; \
         {under_road} under another road or within 80 m of overlapping road ends; {} fails",
        fails.len()
    );
    for f in fails.iter().take(20) {
        println!("  {f}");
    }
    assert!(rays > 1000, "{name}: {rays}");
    assert!(fails.is_empty(), "{name}: {} fails", fails.len());
}

#[test]
fn vesper_v2_sidewalks_are_flush_to_the_buildings() {
    let net = wp_world::vesper::generate(&wp_world::vesper::VesperConfig::default());
    check("vesper_v2", &CityMap::new(net));
}

#[test]
fn grid_sidewalks_are_flush_to_the_buildings() {
    let net = wp_world::procgen::generate(&wp_world::procgen::ProcGenConfig::default());
    check("grid", &CityMap::new(net));
}
