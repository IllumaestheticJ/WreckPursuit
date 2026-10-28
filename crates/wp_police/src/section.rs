//! Road cross-sections for roadblocks and spike strips (owner, 2026-10-28: "roadblocks and
//! spike strips completely block the road").
//!
//! A [`SectionIndex`] holds what a car can drive on and what stops it, from `wp_world`'s city:
//! - **paving**: every road surface ribbon (lanes, parking lanes and shoulders, `left_m` /
//!   `right_m`), its raised sidewalks ([`wp_world::surface::SIDEWALK_W_M`] past a kerbed edge)
//!   and the junction / bend / dead-end patches between ribbons;
//! - **hard stops**: building walls, tunnel walls, concrete barriers (highway edges and
//!   medians, down their sloped free ends to a third of full height) and guard rails
//!   (bridge decks, the bare back edges of deck sidewalks). The same lines `wp_sim` makes solid.
//!
//! [`SectionIndex::section`] walks across the road at a tool site, both ways from the site
//! line, and reports the [`Section`]: the paved run the site is on (road plus sidewalks), the
//! road part of it, and the first hard stop on each side. Everything is filtered by height,
//! so a road under a bridge deck sees neither the deck's rails nor its paving.
//!
//! Deterministic: f32 only, `BTreeMap` grids, fixed iteration order.

use std::collections::BTreeMap;

use crate::graph::{add, cross, dot, len, normalize, perp_left, scale, sub};

/// Grid cell size, m.
const CELL_M: f32 = 25.0;
/// Step of the walk across the road, m.
const STEP_M: f32 = 0.2;
/// Unpaved slivers narrower than this inside a paved run are bridged (seams between ribbons
/// and patches), m.
const SEAM_M: f32 = 0.6;
/// Paving or a stop counts for a site at height `z` when it lies within this below / above
/// it, m (bank and crown across a wide road; a deck above or a road below is further).
const Z_BELOW_M: f32 = 2.0;
/// See [`Z_BELOW_M`].
const Z_ABOVE_M: f32 = 2.5;
/// A barrier profile point counts as a stop from this fraction of full height up (median ends
/// slope to the road: a car rides up the low part only).
const STOP_HEIGHT_FRAC: f32 = 0.3;
/// Default reach of a walk across the road from the site, m.
pub const SECTION_REACH_M: f32 = 48.0;

/// What a point of paving is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaveKind {
    /// Carriageway: lanes, parking lanes, shoulders, junction boxes.
    Road,
    /// Raised sidewalk beyond a kerb.
    Sidewalk,
}

/// One road surface ribbon.
#[derive(Debug, Clone, PartialEq)]
struct Band {
    centre: Vec<[f32; 3]>,
    left_m: f32,
    right_m: f32,
    sidewalk: [bool; 2],
    /// +1: one-way along `centre`, −1: one-way against it, 0: two-way.
    flow: i8,
}

/// One hard-stop line (a wall face, barrier or rail), with its vertical extent.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Stop {
    a: [f32; 2],
    b: [f32; 2],
    z_lo: f32,
    z_hi: f32,
}

/// Paving and hard stops of a city on a grid (see the module doc).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SectionIndex {
    bands: Vec<Band>,
    /// Patch triangles (junctions, bends, dead ends).
    tris: Vec<[[f32; 3]; 3]>,
    stops: Vec<Stop>,
    /// Per cell: band segments `(band, k)`.
    band_grid: BTreeMap<(i32, i32), Vec<(u32, u32)>>,
    tri_grid: BTreeMap<(i32, i32), Vec<u32>>,
    stop_grid: BTreeMap<(i32, i32), Vec<u32>>,
}

/// A walk across the road at a site (see [`SectionIndex::section`]). Offsets are lateral, m,
/// positive to the right of the site's travel direction, from the site point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Section {
    /// Paved run (road + sidewalks, seams bridged) containing the site: left end.
    pub paved_lo: f32,
    /// ... right end.
    pub paved_hi: f32,
    /// Carriageway part of the paved run (no sidewalks): left end.
    pub road_lo: f32,
    /// ... right end.
    pub road_hi: f32,
    /// First hard stop to the left (negative), if one is within reach.
    pub stop_lo: Option<f32>,
    /// First hard stop to the right, if one is within reach.
    pub stop_hi: Option<f32>,
    /// Between the stops (or within the paved run where a side is open) there is one-way
    /// carriageway running against the site's travel direction: the far carriageway of a
    /// divided road with no median barrier at this point.
    pub opposing: bool,
    /// The site point itself is on carriageway.
    pub on_road: bool,
    /// Road height at the site, m (`None` without city geometry).
    pub z_m: Option<f32>,
}

impl Section {
    /// A section from road widths alone (no city geometry: hand-built and procedural graphs):
    /// the carriageway from `lo` to `hi`, kerbs (hard stops) at both edges.
    pub fn plain(lo: f32, hi: f32) -> Self {
        Self {
            paved_lo: lo,
            paved_hi: hi,
            road_lo: lo,
            road_hi: hi,
            stop_lo: Some(lo),
            stop_hi: Some(hi),
            opposing: false,
            on_road: true,
            z_m: None,
        }
    }

    /// Whether a roadblock here can seal the road: a hard stop on both sides within
    /// `verge_m` of the paved run, no far carriageway between them.
    pub fn sealable(&self, verge_m: f32) -> bool {
        self.on_road
            && !self.opposing
            && self
                .stop_lo
                .is_some_and(|s| s >= self.paved_lo - verge_m - 1e-3)
            && self
                .stop_hi
                .is_some_and(|s| s <= self.paved_hi + verge_m + 1e-3)
    }

    /// Whether a spike strip here can cover the paving: on carriageway, no far carriageway in
    /// the paved run.
    pub fn spikeable(&self) -> bool {
        self.on_road && !self.opposing
    }

    /// Left end of the corridor a roadblock spans: the hard stop (or the paved edge).
    pub fn block_lo(&self) -> f32 {
        self.stop_lo.unwrap_or(self.paved_lo)
    }

    /// Right end of the corridor a roadblock spans.
    pub fn block_hi(&self) -> f32 {
        self.stop_hi.unwrap_or(self.paved_hi)
    }
}

fn cell(p: [f32; 2]) -> (i32, i32) {
    (
        (p[0] / CELL_M).floor() as i32,
        (p[1] / CELL_M).floor() as i32,
    )
}

fn xy(p: [f32; 3]) -> [f32; 2] {
    [p[0], p[1]]
}

/// Insert `id` into every cell of the box around `a`–`b` grown by `grow`.
fn insert_span<T: Copy>(
    grid: &mut BTreeMap<(i32, i32), Vec<T>>,
    a: [f32; 2],
    b: [f32; 2],
    grow: f32,
    id: T,
) {
    let lo = cell([a[0].min(b[0]) - grow, a[1].min(b[1]) - grow]);
    let hi = cell([a[0].max(b[0]) + grow, a[1].max(b[1]) + grow]);
    for x in lo.0..=hi.0 {
        for y in lo.1..=hi.1 {
            grid.entry((x, y)).or_default().push(id);
        }
    }
}

impl SectionIndex {
    /// The index of `map` (see the module doc). Empty for a city without terrain (flat
    /// procedural grids: buildings and barriers do not block cars there either).
    pub fn from_city(map: &wp_world::CityMap) -> Self {
        let net = &map.net;
        let mut ix = Self::default();
        if !wp_world::surface::has_relief(net) {
            return ix;
        }
        // Flow of each source road from its lanes.
        let mut flows: BTreeMap<u32, (bool, bool)> = BTreeMap::new();
        let surface_dir: BTreeMap<u32, [f32; 2]> = net
            .surfaces
            .iter()
            .filter(|s| s.centre.len() >= 2)
            .map(|s| {
                let n = s.centre.len();
                (s.road, sub(xy(s.centre[n - 1]), xy(s.centre[0])))
            })
            .collect();
        for l in &net.lanes {
            let Some(d) = surface_dir.get(&l.road) else {
                continue;
            };
            let n = l.polyline.len();
            if n < 2 {
                continue;
            }
            let ld = sub(xy(l.polyline[n - 1]), xy(l.polyline[0]));
            let e = flows.entry(l.road).or_insert((false, false));
            if dot(ld, *d) >= 0.0 {
                e.0 = true;
            } else {
                e.1 = true;
            }
        }
        for s in net.surfaces.iter().filter(|s| s.centre.len() >= 2) {
            let flow = match flows.get(&s.road) {
                Some((true, false)) => 1,
                Some((false, true)) => -1,
                _ => 0,
            };
            let bi = ix.bands.len() as u32;
            let grow = s.left_m.max(s.right_m) + wp_world::surface::SIDEWALK_W_M + 1.0;
            for k in 0..s.centre.len() - 1 {
                insert_span(
                    &mut ix.band_grid,
                    xy(s.centre[k]),
                    xy(s.centre[k + 1]),
                    grow,
                    (bi, k as u32),
                );
            }
            ix.bands.push(Band {
                centre: s.centre.clone(),
                left_m: s.left_m,
                right_m: s.right_m,
                sidewalk: s.sidewalk,
                flow,
            });
        }
        for patch in wp_world::surface::node_patches(map) {
            let m = &patch.mesh;
            for t in &m.indices {
                let tri = [
                    m.positions[t[0] as usize],
                    m.positions[t[1] as usize],
                    m.positions[t[2] as usize],
                ];
                let id = ix.tris.len() as u32;
                let lo = [
                    tri[0][0].min(tri[1][0]).min(tri[2][0]),
                    tri[0][1].min(tri[1][1]).min(tri[2][1]),
                ];
                let hi = [
                    tri[0][0].max(tri[1][0]).max(tri[2][0]),
                    tri[0][1].max(tri[1][1]).max(tri[2][1]),
                ];
                insert_span(&mut ix.tri_grid, lo, hi, 0.0, id);
                ix.tris.push(tri);
            }
        }
        // Hard stops.
        let mut stops: Vec<Stop> = Vec::new();
        for b in &net.buildings {
            let n = b.footprint.len();
            for k in 0..n {
                stops.push(Stop {
                    a: b.footprint[k],
                    b: b.footprint[(k + 1) % n],
                    z_lo: b.base_z_m - 1.0,
                    z_hi: b.roof_z(),
                });
            }
        }
        for c in &net.covered {
            for w in c.centre.windows(2) {
                let d = normalize(sub(xy(w[1]), xy(w[0])));
                let nrm = perp_left(d);
                for side in [1.0f32, -1.0] {
                    let o = scale(nrm, side * c.half_width_m);
                    stops.push(Stop {
                        a: add(xy(w[0]), o),
                        b: add(xy(w[1]), o),
                        z_lo: w[0][2].min(w[1][2]) - 3.0,
                        z_hi: w[0][2].max(w[1][2]) + c.clearance_m,
                    });
                }
            }
        }
        let line_stop = |a: [f32; 3], b: [f32; 3], h: f32| Stop {
            a: xy(a),
            b: xy(b),
            z_lo: a[2].min(b[2]) - 0.5,
            z_hi: a[2].max(b[2]) + h,
        };
        for (a, b) in wp_world::surface::deck_rails(net) {
            stops.push(line_stop(a, b, 0.9));
        }
        for profile in wp_world::surface::median_barrier_profiles(net) {
            for w in profile.windows(2) {
                if w[0].1.max(w[1].1) >= STOP_HEIGHT_FRAC {
                    stops.push(line_stop(w[0].0, w[1].0, 0.81));
                }
            }
        }
        for line in wp_world::surface::edge_barriers(map) {
            for w in line.windows(2) {
                stops.push(line_stop(w[0], w[1], 0.81));
            }
        }
        for (a, b) in wp_world::surface::sidewalk_mesh(map).bare_backs {
            stops.push(line_stop(a, b, 0.9));
        }
        for (k, s) in stops.iter().enumerate() {
            insert_span(&mut ix.stop_grid, s.a, s.b, 0.0, k as u32);
        }
        ix.stops = stops;
        ix
    }

    /// No city geometry (flat or hand-built cities).
    pub fn is_empty(&self) -> bool {
        self.bands.is_empty()
    }

    /// Height of the carriageway at `p` closest to running along `dir` (a site's road, not a
    /// road crossing above or below it); among equally good ones the one nearest `z_hint`.
    pub fn road_z(&self, p: [f32; 2], dir: [f32; 2], z_hint: f32) -> Option<f32> {
        let mut best: Option<(f32, f32)> = None;
        for &(bi, k) in self
            .band_grid
            .get(&cell(p))
            .map_or(&[][..], |v| v.as_slice())
        {
            let b = &self.bands[bi as usize];
            let (a, c) = (b.centre[k as usize], b.centre[k as usize + 1]);
            let d = sub(xy(c), xy(a));
            let l = len(d);
            if l < 1e-3 {
                continue;
            }
            let u = scale(d, 1.0 / l);
            let r = sub(p, xy(a));
            let s_along = dot(r, u);
            // Only the ribbon segment `p` lies across (not one further along the road).
            if s_along < -0.05 || s_along > l + 0.05 {
                continue;
            }
            let t = (s_along / l).clamp(0.0, 1.0);
            let lat = dot(r, [u[1], -u[0]]);
            if lat < -b.left_m - 0.3 || lat > b.right_m + 0.3 {
                continue;
            }
            let along = dot(scale(d, 1.0 / l), dir).abs();
            let z = a[2] + (c[2] - a[2]) * t;
            // Prefer roads along the site (cos near 1), then the nearest height.
            let score = (1.0 - along) * 100.0 + (z - z_hint).abs();
            if best.is_none_or(|(s, _)| score < s) {
                best = Some((score, z));
            }
        }
        best.map(|(_, z)| z)
    }

    /// What is under `p` at height about `z`: `(kind, one-way flow direction)` (flow `None`
    /// on two-way roads, sidewalks and patches), or `None` off the paving.
    pub fn paving_at(&self, p: [f32; 2], z: f32) -> Option<(PaveKind, Option<[f32; 2]>)> {
        let c = cell(p);
        let mut found: Option<(PaveKind, Option<[f32; 2]>)> = None;
        for &(bi, k) in self.band_grid.get(&c).map_or(&[][..], |v| v.as_slice()) {
            let b = &self.bands[bi as usize];
            let (a, e) = (b.centre[k as usize], b.centre[k as usize + 1]);
            let d = sub(xy(e), xy(a));
            let l = len(d);
            if l < 1e-3 {
                continue;
            }
            let u = scale(d, 1.0 / l);
            let r = sub(p, xy(a));
            let s = dot(r, u);
            // Ribbon segments share their end points; a point just past one is on the next.
            if s < -0.05 || s > l + 0.05 {
                continue;
            }
            let lat = dot(r, [u[1], -u[0]]);
            let zz = a[2] + (e[2] - a[2]) * (s / l).clamp(0.0, 1.0);
            if zz < z - Z_ABOVE_M || zz > z + Z_BELOW_M {
                continue;
            }
            let w = wp_world::surface::SIDEWALK_W_M;
            if lat >= -b.left_m && lat <= b.right_m {
                let flow = match b.flow {
                    1 => Some(u),
                    -1 => Some(scale(u, -1.0)),
                    _ => None,
                };
                return Some((PaveKind::Road, flow));
            }
            if (b.sidewalk[0] && lat < -b.left_m && lat >= -b.left_m - w)
                || (b.sidewalk[1] && lat > b.right_m && lat <= b.right_m + w)
            {
                found = Some((PaveKind::Sidewalk, None));
            }
        }
        for &t in self.tri_grid.get(&c).map_or(&[][..], |v| v.as_slice()) {
            let tri = self.tris[t as usize];
            let zz = (tri[0][2] + tri[1][2] + tri[2][2]) / 3.0;
            if zz < z - Z_ABOVE_M || zz > z + Z_BELOW_M {
                continue;
            }
            if in_triangle(p, xy(tri[0]), xy(tri[1]), xy(tri[2])) {
                return Some((PaveKind::Road, None));
            }
        }
        found
    }

    /// Distance along the ray from `from` (unit `dir`) to the first hard stop at height about
    /// `z` within `max_m`.
    pub fn first_stop(&self, from: [f32; 2], dir: [f32; 2], max_m: f32, z: f32) -> Option<f32> {
        let to = add(from, scale(dir, max_m));
        let (a, b) = (cell(from), cell(to));
        let mut best: Option<f32> = None;
        let mut seen: Vec<u32> = Vec::new();
        for x in a.0.min(b.0)..=a.0.max(b.0) {
            for y in a.1.min(b.1)..=a.1.max(b.1) {
                for &k in self
                    .stop_grid
                    .get(&(x, y))
                    .map_or(&[][..], |v| v.as_slice())
                {
                    if seen.contains(&k) {
                        continue;
                    }
                    seen.push(k);
                    let s = &self.stops[k as usize];
                    if s.z_hi < z + 0.2 || s.z_lo > z + 1.0 {
                        continue;
                    }
                    if let Some(t) = ray_segment(from, dir, s.a, s.b) {
                        if t <= max_m && best.is_none_or(|bt| t < bt) {
                            best = Some(t);
                        }
                    }
                }
            }
        }
        best
    }

    /// The paved runs (road or sidewalk, seams bridged) crossed walking from lateral offset
    /// `lo` to `hi` across the road at `p` (travel direction `dir`, height `z`), as
    /// `(start, end)` offsets, + right of `dir`, left to right.
    pub fn paved_runs(
        &self,
        p: [f32; 2],
        dir: [f32; 2],
        z: f32,
        lo: f32,
        hi: f32,
    ) -> Vec<(f32, f32)> {
        let dir = normalize(dir);
        let right = [dir[1], -dir[0]];
        let mut runs: Vec<(f32, f32)> = Vec::new();
        let mut x = lo;
        while x <= hi + 1e-4 {
            if self.paving_at(add(p, scale(right, x)), z).is_some() {
                match runs.last_mut() {
                    Some(r) if x - r.1 <= SEAM_M + 1e-4 => r.1 = x,
                    _ => runs.push((x, x)),
                }
            }
            x += STEP_M;
        }
        runs
    }

    /// Walk across the road at `p` (travel direction `dir`, road height `z`), up to
    /// `reach_m` each way (see [`Section`]).
    pub fn section(&self, p: [f32; 2], dir: [f32; 2], z: f32, reach_m: f32) -> Section {
        let dir = normalize(dir);
        let right = [dir[1], -dir[0]];
        let stop_hi = self.first_stop(p, right, reach_m, z);
        let stop_lo = self
            .first_stop(p, scale(right, -1.0), reach_m, z)
            .map(|t| -t);
        let at = |x: f32| self.paving_at(add(p, scale(right, x)), z);
        let mut opposing = false;
        let mut note = |pv: Option<(PaveKind, Option<[f32; 2]>)>| {
            if let Some((PaveKind::Road, Some(f))) = pv {
                if dot(f, dir) < -0.3 {
                    opposing = true;
                }
            }
        };
        let here = at(0.0);
        let on_road = matches!(here, Some((PaveKind::Road, _)));
        note(here);
        // Walk one way: the paved run's end and the road part's end.
        let mut walk = |sign: f32, limit: f32| -> (f32, f32) {
            let mut paved_end = 0.0f32;
            let mut road_end = 0.0f32;
            let mut road_open = true;
            let mut x = STEP_M;
            while x <= limit + 1e-4 {
                let pv = at(sign * x);
                note(pv);
                match pv {
                    Some((kind, _)) => {
                        paved_end = x;
                        if kind == PaveKind::Road && road_open {
                            road_end = x;
                        } else if kind == PaveKind::Sidewalk {
                            road_open = false;
                        }
                    }
                    None if x - paved_end > SEAM_M => break,
                    None => {}
                }
                x += STEP_M;
            }
            (paved_end, road_end)
        };
        let hi_lim = stop_hi.unwrap_or(reach_m);
        let lo_lim = stop_lo.map_or(reach_m, |s| -s);
        let (ph, rh) = walk(1.0, hi_lim);
        let (pl, rl) = walk(-1.0, lo_lim);
        // Walk on past the paved run to the stops: far carriageway beyond a grass median.
        let mut far = |sign: f32, from: f32, limit: f32| {
            let mut x = from + STEP_M;
            while x <= limit {
                note(at(sign * x));
                x += STEP_M * 2.0;
            }
        };
        far(1.0, ph, hi_lim);
        far(-1.0, pl, lo_lim);
        // A run ending at a stop reaches it (the last sample is up to a step short).
        let snap = |end: f32, stop: Option<f32>| match stop {
            Some(s) if s.abs() - end <= STEP_M + 1e-3 => s.abs(),
            _ => end,
        };
        let ph = snap(ph, stop_hi);
        let pl = snap(pl, stop_lo);
        Section {
            paved_lo: -pl,
            paved_hi: ph,
            road_lo: -rl.min(pl),
            road_hi: rh.min(ph),
            stop_lo,
            stop_hi,
            opposing,
            on_road,
            z_m: Some(z),
        }
    }
}

/// Whether `p` is inside triangle `a b c` (either winding).
fn in_triangle(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> bool {
    let d1 = cross(sub(b, a), sub(p, a));
    let d2 = cross(sub(c, b), sub(p, b));
    let d3 = cross(sub(a, c), sub(p, c));
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

/// Distance along the ray `from + t·dir` (`dir` unit) to segment `a–b`, if it hits.
fn ray_segment(from: [f32; 2], dir: [f32; 2], a: [f32; 2], b: [f32; 2]) -> Option<f32> {
    let e = sub(b, a);
    let den = cross(dir, e);
    if den.abs() < 1e-9 {
        return None;
    }
    let w = sub(a, from);
    let t = cross(w, e) / den;
    let u = cross(w, dir) / den;
    (t >= 0.0 && (-1e-4..=1.0 + 1e-4).contains(&u)).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_hits_segment_ahead_only() {
        assert_eq!(
            ray_segment([0.0, 0.0], [1.0, 0.0], [5.0, -1.0], [5.0, 1.0]),
            Some(5.0)
        );
        assert_eq!(
            ray_segment([0.0, 0.0], [-1.0, 0.0], [5.0, -1.0], [5.0, 1.0]),
            None
        );
        assert_eq!(
            ray_segment([0.0, 0.0], [1.0, 0.0], [5.0, 1.0], [5.0, 2.0]),
            None
        );
    }

    #[test]
    fn plain_section_is_sealable_and_spikeable() {
        let s = Section::plain(-5.0, 5.0);
        assert!(s.sealable(0.0) && s.spikeable());
        assert_eq!((s.block_lo(), s.block_hi()), (-5.0, 5.0));
    }

    #[test]
    fn triangle_inside_either_winding() {
        let (a, b, c) = ([0.0, 0.0], [2.0, 0.0], [0.0, 2.0]);
        assert!(in_triangle([0.5, 0.5], a, b, c));
        assert!(in_triangle([0.5, 0.5], a, c, b));
        assert!(!in_triangle([2.0, 2.0], a, b, c));
    }
}
