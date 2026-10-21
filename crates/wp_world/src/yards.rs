//! Fenced residential lots: backyards, side-yard gates and perimeter walls.
//!
//! Real suburbs leave very little open ground between streets. A block is cut into lots that
//! meet back to back and side by side; front yards stay open to the street, but side and rear
//! yards are closed off by fences or walls on the lot lines (about 6 ft / 1.8 m in most US
//! codes, often one wall shared by two neighbours; concrete block in Sun Belt subdivisions,
//! timber elsewhere, brick walls and hedges in older European districts). Where the backs of
//! lots face a road the subdivision has a perimeter wall along it. So a car that leaves the
//! street can reach a front lawn, but not the space behind the houses.
//!
//! [`yard_fences`] rebuilds that from a city's building placement:
//!
//! 1. Every building is a lot *site* (its footprint centre). Lots are Voronoi cells of the
//!    sites: neighbouring lots split the ground between them on the perpendicular bisector, so
//!    the cells tile the block with no gaps and no overlaps. Roads cut every cell along their
//!    edges, set back behind the sidewalk: each paved edge (and junction box rim) is sampled,
//!    and a sample the lot lies in front of cuts it with the edge's tangent line.
//! 2. A lot that gets a yard is also cut by its gate line, a little behind the front of the
//!    house, parallel to the street it faces: in front of it is the open front yard.
//! 3. Every edge of a yard lot becomes a fence: shared edges once (the union of both lots'
//!    copies), edges facing a road as the perimeter wall, the gate line as side-yard gates.
//! 4. The edges are cut into short straight pieces that follow the ground; pieces hidden inside
//!    their own house or too close to a road, junction or dead-end turnaround are dropped.
//!
//! Everything is half-plane clipping of convex polygons in a fixed order: deterministic, no
//! trig, no hashing.

use std::collections::BTreeMap;

use crate::geom::{self, V2};
use crate::network::{Fence, FenceKind, RoadSurface};
use crate::terrain::Heightfield;

/// One building plot as [`yard_fences`] sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LotSite {
    /// Building footprint corners.
    pub corners: [V2; 4],
    /// Footprint centre (the power-diagram site).
    pub centre: V2,
    /// Unit vector from the street the building faces into the lot.
    pub inward: V2,
    /// A point on the gate line (the front yard is on the street side of it).
    pub gate: V2,
    /// Fence style of this lot's back and side yards; `None` = no yard (shops, offices,
    /// warehouses: the lot still takes its share of the ground from its neighbours).
    pub yard: Option<FenceStyle>,
}

/// Material and height of a fence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FenceStyle {
    /// Material.
    pub kind: FenceKind,
    /// Height above the ground, m.
    pub height_m: f32,
}

/// Tuning for [`yard_fences`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct YardParams {
    /// Largest distance from a site its lot can reach, m.
    pub reach_m: f32,
    /// Spacing of road edge samples, m.
    pub sample_m: f32,
    /// Lots end this far outside a paved edge or junction box (behind the sidewalk), m.
    pub road_setback_m: f32,
    /// A road sample cuts a lot only if the site lies within this cosine of the edge's
    /// outward normal (in front of the sample, not off to the side of a curve).
    pub facing_cos: f32,
    /// Longest straight fence piece (pieces follow the ground between their ends), m.
    pub piece_m: f32,
    /// Pieces closer than this to a paved edge or junction box are dropped, m.
    pub road_clearance_m: f32,
    /// Radius of the turnaround bulb kept clear at a dead end (U-turns swing past the road
    /// end); lots are cut around it like a junction box, m.
    pub dead_end_radius_m: f32,
    /// No fences where the ground is lower than this (shore, water), m.
    pub min_ground_z: f32,
    /// Style of lot edges facing a road (subdivision perimeter wall).
    pub perimeter: FenceStyle,
}

impl Default for YardParams {
    fn default() -> Self {
        Self {
            reach_m: 55.0,
            sample_m: 2.0,
            road_setback_m: 3.5,
            facing_cos: 0.5,
            piece_m: 6.0,
            road_clearance_m: 3.2,
            dead_end_radius_m: 9.0,
            min_ground_z: 0.5,
            perimeter: FenceStyle {
                kind: FenceKind::Masonry,
                height_m: 2.0,
            },
        }
    }
}

/// What cut a lot polygon's edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cut {
    /// The initial square (nothing else within reach).
    Bound,
    /// The lot's own gate line.
    Gate,
    /// A road edge.
    Road,
    /// The bisector with another site.
    Site(u32),
}

/// Convex polygon with a label per edge (`edges[k]` cuts `pts[k] → pts[k + 1]`).
#[derive(Debug, Clone, Default)]
struct Cell {
    pts: Vec<V2>,
    edges: Vec<Cut>,
}

impl Cell {
    /// Keep the part where `n · p <= d`; the new edge is labelled `cut`.
    fn clip(&mut self, n: V2, d: f32, cut: Cut) {
        let k = self.pts.len();
        if k == 0 {
            return;
        }
        let side: Vec<f32> = self.pts.iter().map(|p| geom::dot(n, *p) - d).collect();
        if side.iter().all(|s| *s <= 0.0) {
            return;
        }
        let mut pts = Vec::with_capacity(k + 1);
        let mut edges = Vec::with_capacity(k + 1);
        for i in 0..k {
            let j = (i + 1) % k;
            let (a, b) = (self.pts[i], self.pts[j]);
            let (sa, sb) = (side[i], side[j]);
            let cross = || geom::add(a, geom::scale(geom::sub(b, a), sa / (sa - sb)));
            match (sa <= 0.0, sb <= 0.0) {
                (true, true) => {
                    pts.push(a);
                    edges.push(self.edges[i]);
                }
                (true, false) => {
                    pts.push(a);
                    edges.push(self.edges[i]);
                    pts.push(cross());
                    edges.push(cut);
                }
                (false, true) => {
                    pts.push(cross());
                    edges.push(self.edges[i]);
                }
                (false, false) => {}
            }
        }
        if pts.len() < 3 {
            pts.clear();
            edges.clear();
        }
        self.pts = pts;
        self.edges = edges;
    }

    /// Largest vertex distance from the origin.
    fn radius(&self) -> f32 {
        self.pts.iter().map(|p| geom::len(*p)).fold(0.0, f32::max)
    }
}

/// Points bucketed in a square grid (deterministic: `BTreeMap`, insertion order per cell).
struct PointGrid {
    cell: f32,
    cells: BTreeMap<(i32, i32), Vec<(V2, u32)>>,
}

impl PointGrid {
    fn new(cell: f32) -> Self {
        Self {
            cell,
            cells: BTreeMap::new(),
        }
    }

    fn key(&self, p: V2) -> (i32, i32) {
        (
            (p[0] / self.cell).floor() as i32,
            (p[1] / self.cell).floor() as i32,
        )
    }

    fn insert(&mut self, p: V2, id: u32) {
        let k = self.key(p);
        self.cells.entry(k).or_default().push((p, id));
    }

    /// Every point within `r` of `c`, sorted by distance (ties by id).
    fn near(&self, c: V2, r: f32) -> Vec<(f32, V2, u32)> {
        let (lo, hi) = (
            self.key([c[0] - r, c[1] - r]),
            self.key([c[0] + r, c[1] + r]),
        );
        let mut out = Vec::new();
        for j in lo.1..=hi.1 {
            for i in lo.0..=hi.0 {
                for (p, id) in self.cells.get(&(i, j)).into_iter().flatten() {
                    let d = geom::dist(*p, c);
                    if d <= r {
                        out.push((d, *p, *id));
                    }
                }
            }
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.cmp(&b.2)));
        out
    }
}

/// Paved-surface segments and junction / turnaround discs, each bucketed in every grid cell
/// within `pad` of it, for clearance tests.
struct RoadGrid<'a> {
    surfaces: &'a [RoadSurface],
    discs: &'a [(V2, f32)],
    cell: f32,
    /// Item ids per cell: `(surface << 16) | segment`, or `DISC | disc index`.
    cells: BTreeMap<(i32, i32), Vec<u32>>,
}

impl<'a> RoadGrid<'a> {
    const DISC: u32 = 1 << 31;

    fn new(surfaces: &'a [RoadSurface], discs: &'a [(V2, f32)], pad: f32) -> Self {
        let mut g = Self {
            surfaces,
            discs,
            cell: 20.0,
            cells: BTreeMap::new(),
        };
        let cell = g.cell;
        let key = |v: f32| (v / cell).floor() as i32;
        let add = |cells: &mut BTreeMap<(i32, i32), Vec<u32>>, lo: V2, hi: V2, id: u32| {
            for j in key(lo[1])..=key(hi[1]) {
                for i in key(lo[0])..=key(hi[0]) {
                    cells.entry((i, j)).or_default().push(id);
                }
            }
        };
        for (si, s) in surfaces.iter().enumerate() {
            let r = s.left_m.max(s.right_m) + pad;
            for k in 0..s.centre.len().saturating_sub(1) {
                let (a, b) = (geom::xy(s.centre[k]), geom::xy(s.centre[k + 1]));
                let lo = [a[0].min(b[0]) - r, a[1].min(b[1]) - r];
                let hi = [a[0].max(b[0]) + r, a[1].max(b[1]) + r];
                // Surfaces have far fewer than 65 536 centreline points.
                add(&mut g.cells, lo, hi, ((si as u32) << 16) | k as u32);
            }
        }
        for (di, (c, radius)) in discs.iter().enumerate() {
            let r = radius + pad;
            add(
                &mut g.cells,
                [c[0] - r, c[1] - r],
                [c[0] + r, c[1] + r],
                Self::DISC | di as u32,
            );
        }
        g
    }

    /// Distance from `p` to the nearest paved edge or disc rim (negative = inside); infinite
    /// when nothing is within the grid's pad.
    fn clearance(&self, p: V2) -> f32 {
        let key = (
            (p[0] / self.cell).floor() as i32,
            (p[1] / self.cell).floor() as i32,
        );
        let mut best = f32::INFINITY;
        for &id in self.cells.get(&key).into_iter().flatten() {
            let d = if id & Self::DISC != 0 {
                let (c, r) = self.discs[(id & !Self::DISC) as usize];
                geom::dist(p, c) - r
            } else {
                let s = &self.surfaces[(id >> 16) as usize];
                let k = (id & 0xffff) as usize;
                let (a, b) = (geom::xy(s.centre[k]), geom::xy(s.centre[k + 1]));
                // Paved width on the side `p` is on (`cross > 0` = left of the centreline).
                let half = if geom::cross(geom::sub(b, a), geom::sub(p, a)) > 0.0 {
                    s.left_m
                } else {
                    s.right_m
                };
                geom::point_segment_distance(p, a, b) - half
            };
            best = best.min(d);
        }
        best
    }
}

/// A fence edge before it is cut into pieces: ends, style, and the sites whose footprints
/// may hide it.
struct Edge {
    a: V2,
    b: V2,
    style: FenceStyle,
    owners: [Option<u32>; 2],
}

/// Fences around every yard lot in `sites` (see the module docs). `junctions` are junction
/// boxes (centre, radius), `dead_ends` the positions of dead-end nodes (they get turnaround
/// bulbs of [`YardParams::dead_end_radius_m`]); heights come from `ground`.
pub fn yard_fences(
    sites: &[LotSite],
    surfaces: &[RoadSurface],
    junctions: &[(V2, f32)],
    dead_ends: &[V2],
    ground: &Heightfield,
    p: &YardParams,
) -> Vec<Fence> {
    let mut site_grid = PointGrid::new(40.0);
    for (i, s) in sites.iter().enumerate() {
        site_grid.insert(s.centre, i as u32);
    }
    // Road edge samples with their outward normals: both paved edges of every surface and the
    // rim of every junction box.
    let mut road_grid = PointGrid::new(20.0);
    let mut normals: Vec<V2> = Vec::new();
    let mut sample = |q: V2, m: V2| {
        road_grid.insert(q, normals.len() as u32);
        normals.push(m);
    };
    for s in surfaces {
        for w in s.centre.windows(2) {
            let (a, b) = (geom::xy(w[0]), geom::xy(w[1]));
            let len = geom::dist(a, b);
            if len < 1e-3 {
                continue;
            }
            let t = geom::scale(geom::sub(b, a), 1.0 / len);
            let r = geom::right_of(t);
            let n = (len / p.sample_m).ceil().max(1.0) as usize;
            for i in 0..=n {
                let c = geom::add(a, geom::scale(t, len * i as f32 / n as f32));
                sample(geom::add(c, geom::scale(r, s.right_m)), r);
                sample(geom::sub(c, geom::scale(r, s.left_m)), geom::scale(r, -1.0));
            }
        }
    }
    let discs: Vec<(V2, f32)> = junctions
        .iter()
        .copied()
        .chain(dead_ends.iter().map(|d| (*d, p.dead_end_radius_m)))
        .collect();
    for &(c, radius) in &discs {
        let n = ((core::f32::consts::TAU * radius) / p.sample_m)
            .ceil()
            .max(8.0) as u32;
        // A regular polygon from a rotation recurrence (no trig): step (cos, sin) of 2π / n
        // from the deterministic sin_cos.
        let (s, co) = wp_math::sin_cos(core::f32::consts::TAU / n as f32);
        let mut d = [radius, 0.0];
        for _ in 0..n {
            sample(geom::add(c, d), geom::scale(d, 1.0 / radius.max(1e-3)));
            d = [d[0] * co - d[1] * s, d[0] * s + d[1] * co];
        }
    }

    // 1–3: lot cells and their fence edges.
    let mut edges: Vec<Edge> = Vec::new();
    let mut shared: BTreeMap<(u32, u32), Vec<(V2, V2)>> = BTreeMap::new();
    for (i, site) in sites.iter().enumerate() {
        let Some(style) = site.yard else {
            continue;
        };
        let c = site.centre;
        let local = |q: V2| geom::sub(q, c);
        let n = site.inward;
        let s = [-n[1], n[0]];
        let r = p.reach_m;
        let mut cell = Cell {
            pts: [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .iter()
                .map(|(a, b)| geom::add(geom::scale(s, a * r), geom::scale(n, b * r)))
                .collect(),
            edges: vec![Cut::Bound; 4],
        };
        // Gate line: keep (p − gate) · inward ≥ 0.
        let g = local(site.gate);
        cell.clip(geom::scale(n, -1.0), -geom::dot(n, g), Cut::Gate);
        // The square's corners lie r·√2 out: every site whose bisector and every road edge
        // whose cut can reach the cell must be searched.
        let reach = r * core::f32::consts::SQRT_2;
        // Neighbouring sites: keep the half closer to this one.
        for (_, q, j) in site_grid.near(c, 2.0 * reach) {
            if j as usize == i || cell.pts.is_empty() {
                continue;
            }
            let d = local(q);
            let dl = geom::len(d);
            if dl < 1e-3 || dl * 0.5 > cell.radius() {
                continue;
            }
            cell.clip(d, geom::dot(d, d) * 0.5, Cut::Site(j));
        }
        // Road edges the site faces: keep (x − (q + m·setback)) · m ≥ 0.
        for (dq, q, id) in road_grid.near(c, reach + p.road_setback_m) {
            if cell.pts.is_empty() {
                break;
            }
            let m = normals[id as usize];
            let q = geom::add(local(q), geom::scale(m, p.road_setback_m));
            // `-q · m` = distance of the site in front of the cut line.
            let ahead = -geom::dot(q, m);
            if ahead <= 0.0 || ahead < p.facing_cos * dq || ahead > cell.radius() {
                continue;
            }
            cell.clip(geom::scale(m, -1.0), -geom::dot(m, q), Cut::Road);
        }
        let k = cell.pts.len();
        for e in 0..k {
            let (a, b) = (
                geom::add(cell.pts[e], c),
                geom::add(cell.pts[(e + 1) % k], c),
            );
            if geom::dist(a, b) < 0.05 {
                continue;
            }
            let i = i as u32;
            match cell.edges[e] {
                Cut::Site(j) if sites[j as usize].yard.is_some() => {
                    shared.entry((i.min(j), i.max(j))).or_default().push((a, b));
                }
                Cut::Site(j) => edges.push(Edge {
                    a,
                    b,
                    style,
                    owners: [Some(i), Some(j)],
                }),
                Cut::Road => edges.push(Edge {
                    a,
                    b,
                    style: p.perimeter,
                    owners: [Some(i), None],
                }),
                Cut::Gate | Cut::Bound => edges.push(Edge {
                    a,
                    b,
                    style,
                    owners: [Some(i), None],
                }),
            }
        }
    }
    // Shared lot lines: both lots cut the same bisector; fence the union of their copies once,
    // in the style of the lower-numbered lot.
    for ((i, j), segs) in shared {
        let style = sites[i as usize].yard.expect("yard lot");
        let (o, e) = segs[0];
        let dir = geom::normalize(geom::sub(e, o));
        let mut iv: Vec<(f32, f32)> = segs
            .iter()
            .map(|(a, b)| {
                let (sa, sb) = (
                    geom::dot(geom::sub(*a, o), dir),
                    geom::dot(geom::sub(*b, o), dir),
                );
                (sa.min(sb), sa.max(sb))
            })
            .collect();
        iv.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f32, f32)> = Vec::new();
        for (lo, hi) in iv {
            match merged.last_mut() {
                Some(m) if lo <= m.1 + 0.3 => m.1 = m.1.max(hi),
                _ => merged.push((lo, hi)),
            }
        }
        for (lo, hi) in merged {
            edges.push(Edge {
                a: geom::add(o, geom::scale(dir, lo)),
                b: geom::add(o, geom::scale(dir, hi)),
                style,
                owners: [Some(i), Some(j)],
            });
        }
    }

    // 4: pieces on the ground, filtered.
    let roads = RoadGrid::new(surfaces, &discs, p.road_clearance_m + 1.0);
    let mut out = Vec::new();
    for e in &edges {
        let len = geom::dist(e.a, e.b);
        let n = (len / p.piece_m).ceil().max(1.0) as usize;
        let at = |k: usize| geom::add(e.a, geom::scale(geom::sub(e.b, e.a), k as f32 / n as f32));
        let hidden = |q: V2| {
            e.owners
                .iter()
                .flatten()
                .any(|o| geom::point_in_polygon(q, &sites[*o as usize].corners))
        };
        let blocked =
            |q: V2| roads.clearance(q) < p.road_clearance_m || ground.height_at(q) < p.min_ground_z;
        let mut line: Vec<[f32; 3]> = Vec::new();
        let mut flush = |line: &mut Vec<[f32; 3]>| {
            if line.len() >= 2 {
                out.push(Fence {
                    kind: e.style.kind,
                    line: core::mem::take(line),
                    height_m: e.style.height_m,
                });
            }
            line.clear();
        };
        for k in 0..n {
            let (a, b) = (at(k), at(k + 1));
            let m = geom::scale(geom::add(a, b), 0.5);
            let drop =
                (hidden(a) && hidden(m) && hidden(b)) || blocked(a) || blocked(m) || blocked(b);
            if drop {
                flush(&mut line);
                continue;
            }
            if line.is_empty() {
                line.push([a[0], a[1], ground.height_at(a)]);
            }
            line.push([b[0], b[1], ground.height_at(b)]);
        }
        flush(&mut line);
    }
    out
}

/// Straight spans of a fence for colliders and meshes: consecutive pieces merge while the
/// ground under them stays within `tol_m` of the span's chord (a fence line is straight in
/// plan, so only its height profile decides).
pub fn fence_spans(f: &Fence, tol_m: f32) -> Vec<([f32; 3], [f32; 3])> {
    let mut out = Vec::new();
    let pts = &f.line;
    let mut a = 0;
    while a + 1 < pts.len() {
        let mut b = a + 1;
        while b + 1 < pts.len() {
            let (p, q) = (pts[a], pts[b + 1]);
            let run = geom::dist(geom::xy(p), geom::xy(q)).max(1e-3);
            let fits = (a + 1..=b).all(|k| {
                let t = geom::dist(geom::xy(p), geom::xy(pts[k])) / run;
                (p[2] + (q[2] - p[2]) * t - pts[k][2]).abs() <= tol_m
            });
            if !fits {
                break;
            }
            b += 1;
        }
        out.push((pts[a], pts[b]));
        a = b;
    }
    out
}

/// Total fence length, m.
pub fn fence_length_m(fences: &[Fence]) -> f32 {
    fences
        .iter()
        .flat_map(|f| f.line.windows(2))
        .map(|w| geom::dist(geom::xy(w[0]), geom::xy(w[1])))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn street(y: f32) -> RoadSurface {
        RoadSurface {
            road: 0,
            centre: vec![[-200.0, y, 0.0], [200.0, y, 0.0]],
            bank: vec![0.0, 0.0],
            left_m: 4.0,
            right_m: 4.0,
            highway: false,
            sidewalk: [true, true],
            median_barrier: false,
        }
    }

    const WOOD: FenceStyle = FenceStyle {
        kind: FenceKind::Wood,
        height_m: 1.8,
    };

    /// A row of 10 m × 10 m houses facing a street, `front` m from its centreline, on side
    /// `dir` (+1 north of it, −1 south).
    fn row(y: f32, dir: f32, xs: &[f32]) -> Vec<LotSite> {
        xs.iter()
            .map(|x| {
                let front = y + dir * 8.5;
                let c = [*x, front + dir * 5.0];
                let corners = [
                    [x - 5.0, front],
                    [x + 5.0, front],
                    [x + 5.0, front + dir * 10.0],
                    [x - 5.0, front + dir * 10.0],
                ];
                LotSite {
                    corners,
                    centre: c,
                    inward: [0.0, dir],
                    gate: [*x, front + dir * 2.0],
                    yard: Some(WOOD),
                }
            })
            .collect()
    }

    fn fenced(fences: &[Fence], a: V2, b: V2) -> bool {
        fences
            .iter()
            .flat_map(|f| f.line.windows(2))
            .any(|w| geom::segments_intersect(a, b, geom::xy(w[0]), geom::xy(w[1])))
    }

    /// Two streets 90 m apart with back-to-back rows: every path from either street into the
    /// space behind the houses crosses a fence, the front lawns stay open and nothing is
    /// fenced on the roads.
    #[test]
    fn back_to_back_block_is_closed_behind_the_houses() {
        let xs: Vec<f32> = (0..8).map(|k| -56.0 + 16.0 * k as f32).collect();
        let mut sites = row(0.0, 1.0, &xs);
        sites.extend(row(90.0, -1.0, &xs));
        let surfaces = [street(0.0), street(90.0)];
        let ground = Heightfield::flat([-300.0, -100.0], [300.0, 200.0], 5.0, 1.0);
        let fences = yard_fences(&sites, &surfaces, &[], &[], &ground, &YardParams::default());
        assert!(!fences.is_empty());
        for f in &fences {
            for q in &f.line {
                assert!(q[1] > 7.1 && q[1] < 82.9, "fence on a road: {q:?}");
            }
        }
        // Mid-block (y = 45) is behind fences from both streets, through the side yards
        // between houses and straight past them.
        for x in [-46.5f32, -41.0, 1.0, 9.5, 40.0] {
            assert!(
                fenced(&fences, [x, 3.0], [x, 47.0]),
                "open from the south at x={x}"
            );
            assert!(
                fenced(&fences, [x, 87.0], [x, 43.0]),
                "open from the north at x={x}"
            );
        }
        // Back yards are split between neighbours (no shared corridor along the block).
        assert!(fenced(&fences, [-50.0, 30.0], [-44.0, 30.0]));
        assert!(fenced(&fences, [3.0, 40.0], [3.0, 50.0]));
        // Front lawns are open.
        assert!(!fenced(&fences, [-48.0, 5.0], [-48.0, 9.0]));
        assert!(!fenced(&fences, [-60.0, 7.0], [60.0, 7.0]));
    }

    /// A lot that backs onto a road gets a perimeter wall short of it, and a lot cut is never
    /// fenced across a junction box.
    #[test]
    fn rear_road_gets_a_perimeter_wall_and_junctions_stay_clear() {
        let xs: Vec<f32> = (0..6).map(|k| -40.0 + 16.0 * k as f32).collect();
        let sites = row(0.0, 1.0, &xs);
        let surfaces = [street(0.0), street(40.0)];
        let ground = Heightfield::flat([-300.0, -100.0], [300.0, 200.0], 5.0, 1.0);
        let junctions = [([0.0, 40.0], 10.0)];
        let fences = yard_fences(
            &sites,
            &surfaces,
            &junctions,
            &[],
            &ground,
            &YardParams::default(),
        );
        let walls: Vec<&Fence> = fences
            .iter()
            .filter(|f| f.kind == FenceKind::Masonry)
            .collect();
        assert!(!walls.is_empty(), "no perimeter wall");
        for f in &fences {
            for q in &f.line {
                assert!(q[1] < 40.0 - 4.0 - 3.1, "fence on the rear road: {q:?}");
                assert!(
                    geom::dist(geom::xy(*q), [0.0, 40.0]) >= 13.1,
                    "in the junction"
                );
                assert_eq!(q[2], 1.0, "fences stand on the ground");
            }
        }
        assert!(fenced(&fences, [-30.0, 5.0], [-30.0, 34.0]));
    }

    #[test]
    fn deterministic() {
        let xs: Vec<f32> = (0..5).map(|k| -32.0 + 16.0 * k as f32).collect();
        let mut sites = row(0.0, 1.0, &xs);
        sites.extend(row(70.0, -1.0, &xs[1..]));
        let surfaces = [street(0.0), street(70.0)];
        let ground = Heightfield::flat([-300.0, -100.0], [300.0, 200.0], 5.0, 0.0);
        let p = YardParams {
            min_ground_z: -1.0,
            ..YardParams::default()
        };
        let a = yard_fences(&sites, &surfaces, &[], &[], &ground, &p);
        let b = yard_fences(&sites, &surfaces, &[], &[], &ground, &p);
        assert_eq!(a, b);
        assert!(fence_length_m(&a) > 100.0);
    }

    #[test]
    fn spans_merge_pieces_on_even_ground() {
        let f = Fence {
            kind: FenceKind::Wood,
            line: vec![
                [0.0, 0.0, 0.0],
                [6.0, 0.0, 0.3],
                [12.0, 0.0, 0.6],
                [18.0, 0.0, 0.6],
                [24.0, 0.0, 0.6],
            ],
            height_m: 1.8,
        };
        let spans = fence_spans(&f, 0.1);
        assert_eq!(spans, vec![(f.line[0], f.line[2]), (f.line[2], f.line[4])]);
        assert_eq!(fence_spans(&f, 1.0), vec![(f.line[0], f.line[4])]);
    }
}
