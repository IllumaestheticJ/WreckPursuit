//! Sidewalks level with the lots and building bases (playtest #52: "the sidewalk should be at
//! the same height as the base of the buildings — otherwise it drops wheels too easily";
//! road edge kerb, sidewalk and the ground beyond it "all the same height").
//!
//! Two sweeps through the real `wp_sim` city colliders (downward rays), cross-checked with the
//! greybox the player sees, in Port Vesper v2 and the 12×12 grid:
//! - **Back edges:** every 1 m along every sidewalk's back edge, rays every 0.25 m straight
//!   out across the lot (along its cross-sections), up to a building wall, a guard rail,
//!   another street or [`VERGE_M`]: the ground stays within [`TOL_M`] of the sidewalk top
//!   (no step, no drop), and the greybox shows the same height. Stretches whose strip end was
//!   pulled back for a corner fill, and sidewalks on bridge decks (no lot, a rail), are
//!   counted and skipped.
//! - **Frontages:** every 0.5 m along every building wall standing on a lot (within the lot
//!   width of a sidewalk back edge, on its cross-section): the ground at the foot of the wall
//!   within [`TOL_M`] of that sidewalk's top, and the building's base not above the ground
//!   (no gap under the wall; it may continue below as a plinth).

use std::collections::BTreeMap;

use city_import::glb::build_greybox;
use wp_world::geom::{self, V2, V3};
use wp_world::network::Building;
use wp_world::surface::{lane_bank, lane_sidewalks, KERB_H_M, SIDEWALK_W_M};
use wp_world::CityMap;

/// Largest height change allowed between the sidewalk top and the lot / verge / wall foot, m.
const TOL_M: f32 = 0.02;
/// The flush verge checked beyond every sidewalk back edge where no building stands, m.
const VERGE_M: f32 = 3.0;

/// Surface kinds of the greybox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Paved,
    Sidewalk,
    Corner,
    Lot,
    Terrain,
    Other,
}

fn kind_of(material: &str) -> Kind {
    match material {
        "asphalt" | "highway_concrete" | "parking" | "bus_lane" | "gravel_road" | "dirt_road" => {
            Kind::Paved
        }
        "sidewalk" | "verge" => Kind::Sidewalk,
        "sidewalk_corner" => Kind::Corner,
        "lot" => Kind::Lot,
        // The slope from a lot's outer edge into the ground.
        "lot_skirt" => Kind::Terrain,
        m if m.starts_with("ground_") || m == "rock" || m == "seabed" => Kind::Terrain,
        _ => Kind::Other,
    }
}

/// Up-facing greybox triangles bucketed by xy.
struct Visual {
    tris: Vec<([V3; 3], Kind)>,
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
            let kind = kind_of(&p.material);
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
                tris.push((v, kind));
            }
        }
        Self { tris, grid }
    }

    /// Highest surface under `p` below `from` (height, kind); building roofs are `Other`.
    fn top(&self, p: V2, from: f32) -> Option<(f32, Kind)> {
        let key = ((p[0] / 8.0).floor() as i32, (p[1] / 8.0).floor() as i32);
        let mut best: Option<(f32, Kind)> = None;
        for &id in self.grid.get(&key).map(|v| v.as_slice()).unwrap_or(&[]) {
            let (v, k) = &self.tris[id as usize];
            let (x1, y1, x2, y2, x3, y3) = (v[0][0], v[0][1], v[1][0], v[1][1], v[2][0], v[2][1]);
            let d = (y2 - y3) * (x1 - x3) + (x3 - x2) * (y1 - y3);
            let l1 = ((y2 - y3) * (p[0] - x3) + (x3 - x2) * (p[1] - y3)) / d;
            let l2 = ((y3 - y1) * (p[0] - x3) + (x1 - x3) * (p[1] - y3)) / d;
            let l3 = 1.0 - l1 - l2;
            // Touching an outline counts (samples on shared edges hit either side).
            if l1 < -1e-5 || l2 < -1e-5 || l3 < -1e-5 {
                continue;
            }
            let z = l1 * v[0][2] + l2 * v[1][2] + l3 * v[2][2];
            // Pavement / sidewalk / lot win ties with terrain drawn just under them.
            if z <= from && best.is_none_or(|(bz, _)| z > bz + 1e-4) {
                best = Some((z, *k));
            }
        }
        best
    }
}

/// Buildings bucketed by xy (50 m), for "is this point inside a building?".
struct Buildings<'a> {
    list: &'a [Building],
    grid: BTreeMap<(i32, i32), Vec<usize>>,
}

impl<'a> Buildings<'a> {
    fn new(list: &'a [Building]) -> Self {
        let mut grid: BTreeMap<(i32, i32), Vec<usize>> = BTreeMap::new();
        for (i, b) in list.iter().enumerate() {
            let lo = b
                .footprint
                .iter()
                .fold([f32::INFINITY; 2], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
            let hi = b.footprint.iter().fold([f32::NEG_INFINITY; 2], |a, p| {
                [a[0].max(p[0]), a[1].max(p[1])]
            });
            for bx in (lo[0] / 50.0).floor() as i32..=(hi[0] / 50.0).floor() as i32 {
                for by in (lo[1] / 50.0).floor() as i32..=(hi[1] / 50.0).floor() as i32 {
                    grid.entry((bx, by)).or_default().push(i);
                }
            }
        }
        Self { list, grid }
    }

    /// Whether `p` is inside a building or within 3 cm of its walls (a ray down a wall line
    /// grazes the wall at any height).
    fn contains(&self, p: V2) -> bool {
        let key = ((p[0] / 50.0).floor() as i32, (p[1] / 50.0).floor() as i32);
        self.grid.get(&key).is_some_and(|v| {
            v.iter().any(|i| {
                let fp = &self.list[*i].footprint;
                geom::point_in_polygon(p, fp)
                    || (0..fp.len()).any(|k| {
                        geom::point_segment_distance(p, fp[k], fp[(k + 1) % fp.len()]) < 0.03
                    })
            })
        })
    }
}

/// What one sweep found.
#[derive(Default)]
struct Report {
    samples: usize,
    worst: f32,
    fails: Vec<String>,
}

impl Report {
    fn add(&mut self, err: f32, what: impl FnOnce() -> String) {
        self.samples += 1;
        self.worst = self.worst.max(err.abs());
        if err.abs() > TOL_M {
            self.fails.push(what());
        }
    }
}

/// A stretch of sidewalk back edge `a → b` (at the sidewalk-top height) with the lot's outer
/// line `oa → ob` behind it: the lot is built level along its cross-sections `a + t (b - a)`
/// → `oa + t (ob - oa)` (straight out from the road, mitred at the lane's vertices).
#[derive(Debug, Clone, Copy)]
struct BackSeg {
    a: V3,
    b: V3,
    oa: V2,
    ob: V2,
}

impl BackSeg {
    /// Back-edge point, its height and the unit cross-section direction at fraction `t`.
    fn at(&self, t: f32) -> (V2, f32, V2) {
        let lerp2 = |u: V2, v: V2| geom::add(u, geom::scale(geom::sub(v, u), t));
        let p = lerp2(geom::xy(self.a), geom::xy(self.b));
        let o = lerp2(self.oa, self.ob);
        (
            p,
            self.a[2] + (self.b[2] - self.a[2]) * t,
            geom::normalize(geom::sub(o, p)),
        )
    }
}

/// Every sidewalk back edge of the city, from the lanes (the strips' design lines, before any
/// pulling back at corners).
fn back_segments(map: &CityMap) -> Vec<BackSeg> {
    let net = &map.net;
    let lot_w = wp_world::surface::lot_width_m(net);
    let mut out = Vec::new();
    for l in &net.lanes {
        let [left, right] = lane_sidewalks(l);
        let half = l.width_m * 0.5;
        let bank = lane_bank(net, l);
        for (side, on) in [(1.0f32, right), (-1.0, left)] {
            if !on {
                continue;
            }
            let lat = side * (half + SIDEWALK_W_M);
            let mut line = geom::offset_polyline(&l.polyline, lat);
            for (k, p) in line.iter_mut().enumerate() {
                p[2] += bank.get(k).copied().unwrap_or(0.0) * lat + KERB_H_M;
            }
            let outer = geom::offset_polyline(&l.polyline, side * (half + SIDEWALK_W_M + lot_w));
            for k in 0..line.len() - 1 {
                let d = geom::sub(geom::xy(line[k + 1]), geom::xy(line[k]));
                if geom::len(d) < 1e-3 {
                    continue;
                }
                out.push(BackSeg {
                    a: line[k],
                    b: line[k + 1],
                    oa: geom::xy(outer[k]),
                    ob: geom::xy(outer[k + 1]),
                });
            }
        }
    }
    out
}

/// [`BackSeg`]s bucketed by xy (8 m).
struct BackIndex {
    segs: Vec<BackSeg>,
    grid: BTreeMap<(i32, i32), Vec<u32>>,
}

impl BackIndex {
    fn new(segs: Vec<BackSeg>, pad: f32) -> Self {
        let mut grid: BTreeMap<(i32, i32), Vec<u32>> = BTreeMap::new();
        for (i, s) in segs.iter().enumerate() {
            let lo = [
                s.a[0].min(s.b[0]).min(s.oa[0]).min(s.ob[0]) - pad,
                s.a[1].min(s.b[1]).min(s.oa[1]).min(s.ob[1]) - pad,
            ];
            let hi = [
                s.a[0].max(s.b[0]).max(s.oa[0]).max(s.ob[0]) + pad,
                s.a[1].max(s.b[1]).max(s.oa[1]).max(s.ob[1]) + pad,
            ];
            for bx in (lo[0] / 8.0).floor() as i32..=(hi[0] / 8.0).floor() as i32 {
                for by in (lo[1] / 8.0).floor() as i32..=(hi[1] / 8.0).floor() as i32 {
                    grid.entry((bx, by)).or_default().push(i as u32);
                }
            }
        }
        Self { segs, grid }
    }

    /// The nearest back edge whose lot cross-section runs through `q` (within `reach` out
    /// from the edge): (distance, back-edge height there).
    fn behind(&self, q: V2, reach: f32) -> Option<(f32, f32)> {
        let key = ((q[0] / 8.0).floor() as i32, (q[1] / 8.0).floor() as i32);
        let mut best: Option<(f32, f32)> = None;
        for &i in self.grid.get(&key).map(|v| v.as_slice()).unwrap_or(&[]) {
            let s = &self.segs[i as usize];
            // Which side of the cross-section at t the point is on.
            let side = |t: f32| {
                let (p, _, dir) = s.at(t);
                geom::cross(dir, geom::sub(q, p))
            };
            let (mut lo, mut hi) = (0.0f32, 1.0f32);
            let (f0, f1) = (side(lo), side(hi));
            if f0 * f1 > 0.0 {
                continue;
            }
            for _ in 0..30 {
                let mid = 0.5 * (lo + hi);
                if side(mid) * f0 > 0.0 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let (p, z, dir) = s.at(0.5 * (lo + hi));
            let d = geom::dot(geom::sub(q, p), dir);
            if d <= 0.0 || d > reach {
                continue;
            }
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, z));
            }
        }
        best
    }
}

/// Rays straight out from every sidewalk back edge (see the module doc).
fn back_edges(map: &CityMap, phys: &wp_sim::PhysicsWorld, visual: &Visual) -> Report {
    let blds = Buildings::new(&map.net.buildings);
    // Hedges, fences and tree trunks on the lots and verges: solid, a ray stops there.
    let decor = wp_world::residential::SolidDecorIndex::new(&map.net, 0.05);
    let mut rep = Report::default();
    let (mut rays, mut pulled, mut decks) = (0usize, 0usize, 0usize);
    for seg in back_segments(map) {
        let len = geom::dist(geom::xy(seg.a), geom::xy(seg.b));
        // Off the exact half-metre lines the strips are cut on (measure-zero seams).
        let mut s = 0.37f32.min(len * 0.5);
        while s < len {
            let t = s / len;
            s += 1.0;
            let (p, z_back, dir) = seg.at(t);
            let at = |d: f32| geom::add(p, geom::scale(dir, d));
            // The sidewalk top just inside the back edge; a strip pulled back at a corner has
            // the corner fill here instead (checked from the neighbouring samples).
            let inner = at(-0.1);
            let on_walk = matches!(visual.top(inner, z_back + 1.0), Some((z, Kind::Sidewalk)) if (z - z_back).abs() < 0.03);
            if !on_walk || blds.contains(inner) || decor.contains(inner) {
                pulled += 1;
                continue;
            }
            let Some(ref_z) = phys.static_ground_z(inner, z_back + 1.0, 5.0) else {
                rep.add(9.9, || format!("no physics on the sidewalk at {inner:?}"));
                continue;
            };
            // No lot by design: a bridge deck (the ground far below the sidewalk).
            let first = at(0.05);
            if !matches!(
                visual.top(first, ref_z + 1.0),
                Some((_, Kind::Sidewalk | Kind::Corner | Kind::Lot))
            ) {
                // Then a guard rail stands there (or the ground holds a lot after all).
                let ground = phys.static_ground_z(first, ref_z + 1.0, 60.0);
                if ground.is_some_and(|g| g > ref_z + 0.5) {
                    decks += 1;
                    continue;
                }
            }
            rays += 1;
            let mut d = 0.05f32;
            while d <= VERGE_M + 1e-3 {
                let q = at(d);
                d += 0.25;
                if blds.contains(q) || decor.contains(q) {
                    break; // reached the wall (or a hedge / fence / tree trunk)
                }
                let from = ref_z + 1.0;
                let vis = visual.top(q, from);
                if matches!(vis, Some((_, Kind::Paved))) {
                    break; // another street
                }
                if matches!(vis, Some((_, Kind::Corner))) && d > 0.3 {
                    break; // a corner fill: the next street's level takes over
                }
                let Some(z) = phys.static_ground_z(q, from, 30.0) else {
                    rep.add(9.9, || format!("hole at ({:.2}, {:.2})", q[0], q[1]));
                    break;
                };
                if z >= from - 1e-3 || z > ref_z + 0.5 {
                    break; // inside a collider (building corner), or a guard rail / wall
                }
                rep.add(z - ref_z, || {
                    format!(
                        "({:.2}, {:.2}) {:.2} m past the back edge: ground {z:.3} vs \
                         sidewalk {ref_z:.3} ({:+.3}), greybox {vis:?}",
                        q[0],
                        q[1],
                        d - 0.25,
                        z - ref_z
                    )
                });
                // The greybox shows the same level.
                if let Some((vz, Kind::Sidewalk | Kind::Corner | Kind::Lot)) = vis {
                    if (vz - z).abs() > TOL_M {
                        rep.fails.push(format!(
                            "({:.2}, {:.2}): greybox {vz:.3} vs physics {z:.3}",
                            q[0], q[1]
                        ));
                    }
                }
            }
        }
    }
    println!(
        "  back edges: {rays} rays out from the sidewalk; skipped {pulled} samples where a \
         corner fill replaces the strip, {decks} on bridge decks (no lot, a guard rail)"
    );
    rep
}

/// Ground at the foot of every building wall standing on a lot (see the module doc).
fn frontages(map: &CityMap, phys: &wp_sim::PhysicsWorld, visual: &Visual) -> Report {
    let lot_w = wp_world::surface::lot_width_m(&map.net);
    let reach = lot_w - 0.25;
    let backs = BackIndex::new(back_segments(map), reach + 1.0);
    let mut rep = Report::default();
    let mut walls = 0usize;
    for b in &map.net.buildings {
        let n = b.footprint.len();
        for i in 0..n {
            let (a, c) = (b.footprint[i], b.footprint[(i + 1) % n]);
            let len = geom::dist(a, c);
            if len < 1.0 {
                continue;
            }
            let d = geom::scale(geom::sub(c, a), 1.0 / len);
            // Counter-clockwise footprint: outward is the right of each edge.
            let out = geom::right_of(d);
            let mut counted = false;
            let mut t = 0.3;
            while t <= len - 0.3 {
                let foot = geom::add(geom::add(a, geom::scale(d, t)), geom::scale(out, 0.05));
                t += 0.5;
                let Some((_, walk)) = backs.behind(foot, reach) else {
                    continue;
                };
                // On pavement or the sidewalk itself (a lot overlapping the street): not a
                // frontage on a lot.
                let vis = visual.top(foot, walk + 1.0);
                if matches!(vis, Some((_, Kind::Paved))) {
                    continue;
                }
                if !counted {
                    counted = true;
                    walls += 1;
                }
                let from = walk + 1.0;
                let Some(z) = phys.static_ground_z(foot, from, 30.0) else {
                    rep.add(9.9, || format!("no ground at the wall foot {foot:?}"));
                    continue;
                };
                rep.add(z - walk, || {
                    format!(
                        "wall foot ({:.2}, {:.2}): ground {z:.3} vs sidewalk {walk:.3} ({:+.3}), \
                         base {:.3}, greybox {vis:?}",
                        foot[0],
                        foot[1],
                        z - walk,
                        b.base_z_m,
                    )
                });
                if b.base_z_m > z + TOL_M {
                    rep.fails.push(format!(
                        "wall foot ({:.2}, {:.2}): base {:.3} above the ground {z:.3}",
                        foot[0], foot[1], b.base_z_m
                    ));
                }
            }
        }
    }
    println!("  frontages: {walls} walls on a lot (within {reach} m of a sidewalk back edge)");
    rep
}

fn check(name: &str, map: &CityMap) {
    let visual = Visual::new(map);
    let mut phys = wp_sim::PhysicsWorld::new();
    phys.set_city_ground(map);
    println!("{name}:");
    let mut all = Vec::new();
    for (what, rep) in [
        ("back edges", back_edges(map, &phys, &visual)),
        ("frontages", frontages(map, &phys, &visual)),
    ] {
        println!(
            "  {what}: {} samples, worst height change {:.3} m, {} over {TOL_M} m",
            rep.samples,
            rep.worst,
            rep.fails.len()
        );
        let shown = std::env::var("SHOW_ALL").map_or(15, |_| usize::MAX);
        for f in rep.fails.iter().take(shown) {
            println!("    {f}");
        }
        assert!(rep.samples > 1000, "{name} {what}: {} samples", rep.samples);
        all.extend(rep.fails);
    }
    assert!(all.is_empty(), "{name}: {} samples off", all.len());
}

#[test]
fn vesper_v2_sidewalks_are_level_with_lots_and_buildings() {
    let net = wp_world::vesper::generate(&wp_world::vesper::VesperConfig::default());
    check("vesper_v2", &CityMap::new(net));
}

#[test]
fn grid_sidewalks_are_level_with_lots_and_buildings() {
    let net = wp_world::procgen::generate(&wp_world::procgen::ProcGenConfig::default());
    check("grid", &CityMap::new(net));
}
