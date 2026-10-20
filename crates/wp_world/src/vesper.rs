//! Port Vesper v2 (`vesper_v2`): the procedural city with curves and elevation, loosely after
//! `docs/design/CITY.md`. Coordinates in metres, +X east, +Y north, +Z up.
//!
//! Districts (scaled down from the design's 6 × 6 km to about 3.3 × 2.9 km):
//! - **Lantern Row** downtown: a 7 × 7 street grid (100 m blocks) with arterials, signals,
//!   towers; gently rising to the north.
//! - **Harbor Boulevard** waterfront: a curving arterial along the flat south shore, the
//!   **Gantry Row** port grid and Terminal Avenue on the quay below it.
//! - **Mercer** (north), **Eastgate** (east) and **Larkspur** (west suburb): organic streets —
//!   warped grids with curved edges, a few dropped edges and cul-de-sacs.
//! - **Hollins Heights** (north-west hill, ≈ 110 m): **Hollins Drive** climbs it in
//!   switchbacks; **Ridge Road** comes back down over the crests; a lot at the knob.
//! - **Bayview Bluffs** (north-east plateau): **Bluff Road** winds along the edge down to the
//!   highway.
//! - **Loop 9**: a 3+3 highway loop with banked sweepers. **Route 40 West** passes under it
//!   (the loop rises on an overpass) and **Route 40 North** dips under it (underpass).
//!
//! - **Covered roads** (backlog B04): the **Hollins Tunnel** carries Loop 9 through the
//!   northern shoulder of Hollins Heights (ground backfilled over the roof), and **Founders
//!   Cut** drops the Uptown Connector into a covered trench under a plaza deck. Both are
//!   [`CoveredSpan`]s: police and the helicopter cannot see in or out.
//!
//! Elevation: a smooth deterministic heightfield (authored hill, plateau, northward rise,
//! seeded value noise, sea at the south and east edges). Every road gets a vertical profile
//! that follows the terrain, is grade-capped per class (highway 6 %, hill road 11.5 %), is
//! flat over each junction approach and is smoothed until crests and sags are passable at the
//! speed limit. The heightfield is then carved to the roads (cut and fill), except under
//! bridge decks.

use std::collections::BTreeMap;

use wp_math::Rng;

use crate::builder::{self, BuildOptions, Built, NodeSpec, RoadClass, RoadSpec};
use crate::covered::{CoverShape, DECK_OVERHANG_M, ROOF_M, WALL_M};
use crate::density::default_density_base;
use crate::geom::{self, Obb, V2, V3};
use crate::network::*;
use crate::surface::{is_elevated, KERB_H_M, LOT_W_M, SIDEWALK_W_M};
use crate::terrain::{bump, smoothstep, Heightfield, ValueNoise};

/// City id of this preset (file stem for its data files).
pub const CITY_ID: &str = "vesper_v2";

/// Vertical clearance under a grade separation (deck surface to road surface), m.
pub const CROSSING_CLEARANCE_M: f32 = 7.5;

/// Terrain sits this far below an at-grade road surface, m.
pub const CARVE_BELOW_M: f32 = 0.12;

/// Full-strength carve corridor beyond a highway's paved edge, m.
const SHOULDER_M: f32 = 7.5;

/// Under a sidewalk road's lots (playtest #52) the ground is carved to this far below the lot
/// top, m: the lot (`wp_world::surface::sidewalk_mesh`) is level with the sidewalk's back edge
/// and the ground beyond it starts within a few cm of that level.
const LOT_GROUND_GAP_M: f32 = 0.12;

/// Sidewalk + lot behind a street's paved edge, m.
const FLUSH_W_M: f32 = SIDEWALK_W_M + LOT_W_M;

/// Full-strength carve corridor beyond a street's paved edge, m: sidewalk, lot, skirt and one
/// terrain cell more before the slope to the natural ground starts, so no terrain triangle
/// under the lot or skirt reaches up the slope (it would poke through the lot).
const STREET_SHOULDER_M: f32 = FLUSH_W_M + 1.0 + 5.0;

/// Beyond its paved radius a junction disc carves at full strength only this far (the
/// streets' lots take over from there), m.
const NODE_SHOULDER_M: f32 = 1.5;

/// Carve height under a street at lateral distance `d_out` beyond its paved edge, relative to
/// the lot level there: the road's own `- CARVE_BELOW_M` under the kerb, rising across the
/// sidewalk and lot to `- LOT_GROUND_GAP_M` just inside the lot's outer edge.
fn street_carve_offset(d_out: f32) -> f32 {
    let t = ((d_out - 1.0) / (FLUSH_W_M - 1.5)).clamp(0.0, 1.0);
    -(KERB_H_M + CARVE_BELOW_M) + t * (KERB_H_M + CARVE_BELOW_M - LOT_GROUND_GAP_M)
}

/// Smallest angle between a dead-end stub and the other roads at its start node, rad.
const MIN_STUB_ANGLE_RAD: f32 = 0.7;

/// Generator parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct VesperConfig {
    /// Seed for terrain noise, street warping, lots and heights.
    pub seed: u64,
    /// Heightfield vertex spacing, m.
    pub terrain_cell_m: f32,
}

impl Default for VesperConfig {
    fn default() -> Self {
        Self {
            seed: 1,
            terrain_cell_m: 5.0,
        }
    }
}

const HILL: V2 = [-1000.0, 1050.0];
const KNOB: V2 = [-1000.0, 1080.0];
const BLUFF: V2 = [1000.0, 950.0];
const MAP_LO: V2 = [-1700.0, -1300.0];
const MAP_HI: V2 = [1700.0, 1750.0];

/// The natural (uncarved) ground.
struct Natural {
    n1: ValueNoise,
    n2: ValueNoise,
}

impl Natural {
    fn new(rng: &mut Rng) -> Self {
        Self {
            n1: ValueNoise::new(rng, MAP_LO, MAP_HI, 420.0),
            n2: ValueNoise::new(rng, MAP_LO, MAP_HI, 150.0),
        }
    }

    fn z(&self, p: V2) -> f32 {
        let base = 2.5 + 22.0 * smoothstep(-650.0, 1400.0, p[1]);
        let hill = 88.0 * bump(p, HILL, 560.0);
        let plateau = 26.0 * smoothstep(520.0, 330.0, geom::dist(p, BLUFF));
        // No noise on the port flat; damped downtown.
        let flat = smoothstep(-700.0, -450.0, p[1]);
        let core = 0.35 + 0.65 * smoothstep(250.0, 600.0, geom::dist(p, [0.0, -50.0]));
        let noise = (6.0 * self.n1.at(p) + 2.0 * self.n2.at(p)) * flat * core;
        let land = (base + hill + plateau + noise).max(1.5);
        let sea = smoothstep(-1110.0, -1180.0, p[1]).max(smoothstep(1600.0, 1660.0, p[0]));
        land + (-3.0 - land) * sea
    }
}

/// Road style.
#[derive(Debug, Clone, Copy)]
struct Style {
    class: RoadClass,
    fwd: u8,
    bwd: u8,
    speed: f32,
    parking: bool,
    /// Grade cap.
    grade: f32,
    /// Superelevate curves (max cross slope).
    bank_max: f32,
}

const HIGHWAY: Style = Style {
    class: RoadClass::Highway,
    fwd: 3,
    bwd: 3,
    speed: 24.586_72, // 55 mph
    parking: false,
    grade: 0.06,
    bank_max: 0.06,
};
const ARTERIAL: Style = Style {
    class: RoadClass::Arterial,
    fwd: 2,
    bwd: 2,
    speed: 17.8816, // 40 mph
    parking: false,
    grade: 0.08,
    bank_max: 0.02,
};
const ROUTE: Style = Style {
    speed: 20.1168, // 45 mph
    ..ARTERIAL
};
const BLVD: Style = Style {
    speed: 15.6464, // 35 mph
    ..ARTERIAL
};
const COLLECTOR: Style = Style {
    class: RoadClass::Collector,
    fwd: 1,
    bwd: 1,
    speed: 13.4112, // 30 mph
    parking: true,
    grade: 0.10,
    bank_max: 0.0,
};
const LOCAL: Style = Style {
    class: RoadClass::Local,
    fwd: 1,
    bwd: 1,
    speed: 11.176, // 25 mph
    parking: true,
    grade: 0.10,
    bank_max: 0.0,
};
const SERVICE: Style = Style {
    parking: false,
    ..LOCAL
};
const HILL_ROAD: Style = Style {
    parking: false,
    grade: 0.115,
    ..LOCAL
};
const BLUFF_ROAD: Style = Style {
    parking: false,
    bank_max: 0.03,
    ..COLLECTOR
};
const PORT: Style = Style {
    class: RoadClass::Collector,
    fwd: 2,
    bwd: 2,
    speed: 15.6464,
    parking: false,
    grade: 0.06,
    bank_max: 0.0,
};

/// A road before profiling.
#[derive(Debug, Clone)]
struct Draft {
    a: usize,
    b: usize,
    /// Full centreline a → b (ends are the node positions).
    path: Vec<V2>,
    st: Style,
    name: &'static str,
    /// Dips under a highway it crosses (underpass) instead of the highway rising over it.
    dips: bool,
    /// Internal height anchors `(s0, s1, z)` (grade separations).
    anchors: Vec<(f32, f32, f32)>,
    /// Arc ranges that are bridge decks (no terrain carving).
    decks: Vec<(f32, f32)>,
    /// Covered stretches (tunnels) on this road.
    covers: Vec<CoverPlan>,
}

/// A planned covered stretch of one draft road.
#[derive(Debug, Clone, Copy)]
struct CoverPlan {
    /// Portal plan points (matched onto the built surface).
    p0: V2,
    p1: V2,
    kind: CoverKind,
    name: &'static str,
}

/// Ceiling height over a covered highway, m.
pub const TUNNEL_CLEARANCE_HIGHWAY_M: f32 = 6.5;
/// Ceiling height over other covered roads, m.
pub const TUNNEL_CLEARANCE_M: f32 = 6.0;

/// A planned grade separation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossing {
    /// Upper road (source road index).
    pub upper: usize,
    /// Lower road.
    pub lower: usize,
    /// Plan position, m.
    pub pos: V2,
    /// Underpass (lower road cut down) rather than overpass (upper road raised).
    pub underpass: bool,
}

/// Generator diagnostics (tests and the README's numbers).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VesperReport {
    /// Grade separations built.
    pub crossings: Vec<Crossing>,
    /// Optional roads dropped because they would have crossed another road at grade.
    pub skipped_roads: usize,
    /// Road index of Hollins Drive.
    pub hollins_drive: usize,
    /// Road indices of the highway loop.
    pub loop_roads: Vec<usize>,
    /// Names per source road.
    pub road_names: Vec<&'static str>,
    /// Speed limit per source road, m/s.
    pub road_speeds: Vec<f32>,
    /// Grade cap per source road.
    pub road_grades: Vec<f32>,
}

struct Gen {
    rng: Rng,
    nat: Natural,
    nodes: Vec<NodeSpec>,
    index: BTreeMap<(i64, i64), usize>,
    drafts: Vec<Draft>,
    skipped: usize,
}

// ------------------------------------------------------------------------------------------
// Polyline helpers (2D)
// ------------------------------------------------------------------------------------------

fn cum2(p: &[V2]) -> Vec<f32> {
    let mut out = Vec::with_capacity(p.len());
    let mut acc = 0.0;
    out.push(0.0);
    for w in p.windows(2) {
        acc += geom::dist(w[0], w[1]);
        out.push(acc);
    }
    out
}

/// Point and unit tangent at arc length `s` (clamped).
fn point_on(p: &[V2], cum: &[f32], s: f32) -> (V2, V2) {
    let n = p.len();
    let s = s.clamp(0.0, cum[n - 1]);
    let i = match cum.binary_search_by(|c| c.total_cmp(&s)) {
        Ok(i) => i.min(n - 2),
        Err(i) => i.saturating_sub(1).min(n - 2),
    };
    let l = (cum[i + 1] - cum[i]).max(1e-6);
    let t = (s - cum[i]) / l;
    let d = geom::normalize(geom::sub(p[i + 1], p[i]));
    (
        geom::add(p[i], geom::scale(geom::sub(p[i + 1], p[i]), t)),
        d,
    )
}

/// Uniform resampling at about `step` metres, keeping both ends.
fn resample(p: &[V2], step: f32) -> Vec<V2> {
    let cum = cum2(p);
    let total = cum[cum.len() - 1];
    let n = ((total / step).ceil() as usize).max(1);
    (0..=n)
        .map(|k| point_on(p, &cum, total * k as f32 / n as f32).0)
        .collect()
}

/// Closest point: (arc length, distance).
fn project2(p: &[V2], cum: &[f32], q: V2) -> (f32, f32) {
    let mut best = (0.0, f32::INFINITY);
    for i in 0..p.len() - 1 {
        let ab = geom::sub(p[i + 1], p[i]);
        let l2 = geom::dot(ab, ab).max(1e-12);
        let t = (geom::dot(geom::sub(q, p[i]), ab) / l2).clamp(0.0, 1.0);
        let c = geom::add(p[i], geom::scale(ab, t));
        let d = geom::dist(q, c);
        if d < best.1 {
            best = (cum[i] + t * (cum[i + 1] - cum[i]), d);
        }
    }
    best
}

/// Sub-path between arc lengths `s0 < s1`.
fn sub_path(p: &[V2], cum: &[f32], s0: f32, s1: f32) -> Vec<V2> {
    let mut out = vec![point_on(p, cum, s0).0];
    for (i, q) in p.iter().enumerate() {
        if cum[i] > s0 + 0.5 && cum[i] < s1 - 0.5 {
            out.push(*q);
        }
    }
    out.push(point_on(p, cum, s1).0);
    out
}

/// Catmull-Rom curve through `ctrl`, sampled about every `step` metres.
fn catmull_rom(ctrl: &[V2], step: f32) -> Vec<V2> {
    let n = ctrl.len();
    let get = |i: isize| -> V2 {
        if i < 0 {
            geom::sub(geom::scale(ctrl[0], 2.0), ctrl[1])
        } else if i as usize >= n {
            geom::sub(geom::scale(ctrl[n - 1], 2.0), ctrl[n - 2])
        } else {
            ctrl[i as usize]
        }
    };
    let mut out = vec![ctrl[0]];
    for i in 0..n - 1 {
        let (p0, p1, p2, p3) = (
            get(i as isize - 1),
            get(i as isize),
            get(i as isize + 1),
            get(i as isize + 2),
        );
        let m = ((geom::dist(p1, p2) / step).ceil() as usize).max(1);
        for k in 1..=m {
            let t = k as f32 / m as f32;
            let (t2, t3) = (t * t, t * t * t);
            let c = |a: f32, b: f32, c: f32, d: f32| {
                0.5 * (2.0 * b
                    + (-a + c) * t
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
                    + (-a + 3.0 * b - 3.0 * c + d) * t3)
            };
            out.push([c(p0[0], p1[0], p2[0], p3[0]), c(p0[1], p1[1], p2[1], p3[1])]);
        }
    }
    out
}

/// Round the corners of a polyline (closed when `closed`) with circular arcs of radius
/// `radius[i]` at vertex `i` (limited so arcs never overlap), sampled every `step` metres.
fn fillet(ctrl: &[V2], radius: &[f32], closed: bool, step: f32) -> Vec<V2> {
    let n = ctrl.len();
    let mut out: Vec<V2> = Vec::new();
    if !closed {
        out.push(ctrl[0]);
    }
    let range: Vec<usize> = if closed {
        (0..n).collect()
    } else {
        (1..n - 1).collect()
    };
    for i in range {
        let prev = ctrl[(i + n - 1) % n];
        let next = ctrl[(i + 1) % n];
        let c = ctrl[i];
        let (l1, l2) = (geom::dist(prev, c), geom::dist(c, next));
        let d1 = geom::normalize(geom::sub(c, prev));
        let d2 = geom::normalize(geom::sub(next, c));
        let theta = geom::atan2(geom::cross(d1, d2), geom::dot(d1, d2));
        let half = theta.abs() * 0.5;
        if half < 1e-3 {
            out.push(c);
            continue;
        }
        let (s, co) = wp_math::sin_cos(half);
        let tan_half = s / co;
        let t = (radius[i] * tan_half).min(0.45 * l1.min(l2));
        let r = t / tan_half;
        let a = geom::sub(c, geom::scale(d1, t));
        // Centre to the left of d1 for a left turn, to the right for a right turn.
        let left = [-d1[1], d1[0]];
        let sign = if theta > 0.0 { 1.0 } else { -1.0 };
        let centre = geom::add(a, geom::scale(left, r * sign));
        let arm = geom::sub(a, centre);
        let m = ((r * theta.abs() / step).ceil() as usize).max(2);
        for k in 0..=m {
            let (sn, cs) = wp_math::sin_cos(theta * k as f32 / m as f32);
            out.push(geom::add(
                centre,
                [arm[0] * cs - arm[1] * sn, arm[0] * sn + arm[1] * cs],
            ));
        }
    }
    if !closed {
        out.push(ctrl[n - 1]);
    }
    out
}

/// Gentle curve from `a` to `b` bulging `bulge` metres to the left at the middle.
fn bent(a: V2, b: V2, bulge: f32, step: f32) -> Vec<V2> {
    let d = geom::normalize(geom::sub(b, a));
    let mid = geom::scale(geom::add(a, b), 0.5);
    let ctrl = geom::add(mid, geom::scale([-d[1], d[0]], 2.0 * bulge));
    let n = ((geom::dist(a, b) / step).ceil() as usize).max(2);
    (0..=n)
        .map(|k| {
            let t = k as f32 / n as f32;
            let u = 1.0 - t;
            [
                u * u * a[0] + 2.0 * u * t * ctrl[0] + t * t * b[0],
                u * u * a[1] + 2.0 * u * t * ctrl[1] + t * t * b[1],
            ]
        })
        .collect()
}

/// Intersection of segments `a0a1` and `b0b1` (parameters along each).
fn seg_cross(a0: V2, a1: V2, b0: V2, b1: V2) -> Option<(f32, f32)> {
    let r = geom::sub(a1, a0);
    let s = geom::sub(b1, b0);
    let den = geom::cross(r, s);
    if den.abs() < 1e-9 {
        return None;
    }
    let w = geom::sub(b0, a0);
    let t = geom::cross(w, s) / den;
    let u = geom::cross(w, r) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((t, u))
}

/// Every crossing of two paths away from shared end points: (s on a, s on b, point).
fn path_crossings(a: &[V2], b: &[V2]) -> Vec<(f32, f32, V2)> {
    let (ca, cb) = (cum2(a), cum2(b));
    let mut out = Vec::new();
    for i in 0..a.len() - 1 {
        for j in 0..b.len() - 1 {
            if let Some((t, u)) = seg_cross(a[i], a[i + 1], b[j], b[j + 1]) {
                let p = geom::add(a[i], geom::scale(geom::sub(a[i + 1], a[i]), t));
                let near_end = [a[0], a[a.len() - 1]]
                    .iter()
                    .chain([b[0], b[b.len() - 1]].iter())
                    .any(|e| geom::dist(*e, p) < 1.0);
                if !near_end {
                    out.push((
                        ca[i] + t * (ca[i + 1] - ca[i]),
                        cb[j] + u * (cb[j + 1] - cb[j]),
                        p,
                    ));
                }
            }
        }
    }
    out
}

// ------------------------------------------------------------------------------------------
// Layout
// ------------------------------------------------------------------------------------------

impl Gen {
    fn node(&mut self, p: V2) -> usize {
        let key = ((p[0] * 10.0).round() as i64, (p[1] * 10.0).round() as i64);
        if let Some(i) = self.index.get(&key) {
            return *i;
        }
        let i = self.nodes.len();
        self.nodes.push(NodeSpec {
            pos: p,
            z: 0.0,
            signal: false,
            control: None,
        });
        self.index.insert(key, i);
        i
    }

    fn pos(&self, n: usize) -> V2 {
        self.nodes[n].pos
    }

    /// Add a road along `path` between nodes `a` and `b` (path ends are snapped onto them).
    /// Non-highway roads that would cross another non-highway road are dropped (`None`).
    fn road(
        &mut self,
        a: usize,
        b: usize,
        mut path: Vec<V2>,
        st: Style,
        name: &'static str,
    ) -> Option<usize> {
        if path.len() < 2 {
            path = vec![self.pos(a), self.pos(b)];
        }
        path[0] = self.pos(a);
        let last = path.len() - 1;
        path[last] = self.pos(b);
        path.dedup_by(|x, y| geom::dist(*x, *y) < 0.05);
        if path.len() < 2 {
            return None;
        }
        if st.class != RoadClass::Highway {
            let bad = self.drafts.iter().any(|d| {
                d.st.class != RoadClass::Highway && !path_crossings(&path, &d.path).is_empty()
            });
            if bad {
                self.skipped += 1;
                return None;
            }
        }
        self.drafts.push(Draft {
            a,
            b,
            path,
            st,
            name,
            dips: false,
            anchors: Vec::new(),
            decks: Vec::new(),
            covers: Vec::new(),
        });
        Some(self.drafts.len() - 1)
    }

    /// Straight or bent road between two nodes.
    fn link(&mut self, a: usize, b: usize, bulge: f32, st: Style, name: &'static str) {
        let path = bent(self.pos(a), self.pos(b), bulge, 8.0);
        self.road(a, b, path, st, name);
    }

    /// Split an open `path` at arc lengths `cuts` into roads; returns the node at each cut
    /// (input order) plus the two end nodes `(cut_nodes, start, end)`.
    fn chain(
        &mut self,
        path: &[V2],
        cuts: &[f32],
        st: Style,
        name: &'static str,
    ) -> (Vec<usize>, usize, usize) {
        let cum = cum2(path);
        let total = cum[cum.len() - 1];
        let mut order: Vec<(f32, usize)> = cuts.iter().copied().zip(0..).collect();
        order.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut stops = vec![0.0];
        stops.extend(order.iter().map(|o| o.0));
        stops.push(total);
        let nodes: Vec<usize> = stops
            .iter()
            .map(|s| {
                let p = point_on(path, &cum, *s).0;
                self.node(p)
            })
            .collect();
        for k in 0..stops.len() - 1 {
            let sp = sub_path(path, &cum, stops[k], stops[k + 1]);
            self.road(nodes[k], nodes[k + 1], sp, st, name);
        }
        let mut cut_nodes = vec![0; cuts.len()];
        for (k, o) in order.iter().enumerate() {
            cut_nodes[o.1] = nodes[k + 1];
        }
        (cut_nodes, nodes[0], nodes[nodes.len() - 1])
    }

    /// Closed loop split at arc lengths `cuts` (≥ 2); returns the node per cut and the
    /// road indices.
    fn ring(
        &mut self,
        path: &[V2],
        cuts: &[f32],
        st: Style,
        name: &'static str,
    ) -> (Vec<usize>, Vec<usize>) {
        let mut closed = path.to_vec();
        closed.push(path[0]);
        let cum = cum2(&closed);
        let total = cum[cum.len() - 1];
        let mut order: Vec<(f32, usize)> = cuts.iter().copied().zip(0..).collect();
        order.sort_by(|x, y| x.0.total_cmp(&y.0));
        let nodes: Vec<usize> = order
            .iter()
            .map(|o| {
                let p = point_on(&closed, &cum, o.0).0;
                self.node(p)
            })
            .collect();
        let mut roads = Vec::new();
        for k in 0..order.len() {
            let s0 = order[k].0;
            let (s1, wrap) = if k + 1 < order.len() {
                (order[k + 1].0, false)
            } else {
                (order[0].0 + total, true)
            };
            let sp = if wrap {
                let mut a = sub_path(&closed, &cum, s0, total);
                let b = sub_path(&closed, &cum, 0.0, s1 - total);
                a.pop();
                a.extend(b);
                a
            } else {
                sub_path(&closed, &cum, s0, s1)
            };
            let nb = nodes[(k + 1) % nodes.len()];
            if let Some(r) = self.road(nodes[k], nb, sp, st, name) {
                roads.push(r);
            }
        }
        let mut cut_nodes = vec![0; cuts.len()];
        for (k, o) in order.iter().enumerate() {
            cut_nodes[o.1] = nodes[k];
        }
        (cut_nodes, roads)
    }

    /// Warped grid with curved edges. `drop` = chance of removing an optional edge (kept
    /// connected, no new dead ends). Returns node ids `[row][col]`.
    // Grid code reads clearer with explicit row/column indices.
    #[allow(clippy::too_many_arguments, clippy::needless_range_loop)]
    fn warped_grid(
        &mut self,
        xs: &[f32],
        ys: &[f32],
        warp: f32,
        bulge: f32,
        drop: f32,
        style: &dyn Fn(bool, usize) -> Style,
        name: &'static str,
    ) -> Vec<Vec<usize>> {
        let mut ids = vec![vec![0usize; xs.len()]; ys.len()];
        for (j, y) in ys.iter().enumerate() {
            for (i, x) in xs.iter().enumerate() {
                let p = [
                    x + self.rng.range_f32(-warp, warp),
                    y + self.rng.range_f32(-warp, warp),
                ];
                ids[j][i] = self.node(p);
            }
        }
        // Candidate edges: (a, b, row?, index of the row/column, bulge).
        let mut edges: Vec<(usize, usize, bool, usize, f32)> = Vec::new();
        for (j, row) in ids.iter().enumerate() {
            for i in 0..xs.len() - 1 {
                let b = self.rng.range_f32(-bulge, bulge);
                edges.push((row[i], row[i + 1], true, j, b));
            }
        }
        for i in 0..xs.len() {
            for j in 0..ys.len() - 1 {
                let b = self.rng.range_f32(-bulge, bulge);
                edges.push((ids[j][i], ids[j + 1][i], false, i, b));
            }
        }
        let mut keep = vec![true; edges.len()];
        for k in 0..edges.len() {
            let roll = self.rng.next_f32();
            if roll >= drop {
                continue;
            }
            keep[k] = false;
            let deg_ok = [edges[k].0, edges[k].1].iter().all(|n| {
                edges
                    .iter()
                    .zip(&keep)
                    .filter(|(e, kp)| **kp && (e.0 == *n || e.1 == *n))
                    .count()
                    >= 2
            });
            if !deg_ok || !connected(&edges, &keep) {
                keep[k] = true;
            }
        }
        for (k, e) in edges.iter().enumerate() {
            if keep[k] {
                let st = style(e.2, e.3);
                self.link(e.0, e.1, e.4, st, name);
            }
        }
        ids
    }

    /// Unit directions in which the roads laid so far leave node `n` (15 m along each).
    fn headings_at(&self, n: usize) -> Vec<V2> {
        let c = self.pos(n);
        let mut out = Vec::new();
        for d in &self.drafts {
            if d.a != n && d.b != n {
                continue;
            }
            let path: Vec<V2> = if d.a == n {
                d.path.clone()
            } else {
                d.path.iter().rev().copied().collect()
            };
            let cum = cum2(&path);
            let (p, _) = point_on(&path, &cum, 15f32.min(cum[cum.len() - 1]));
            out.push(geom::normalize(geom::sub(p, c)));
        }
        out
    }

    /// Dead-end stub from `from` towards `towards` (`len` m), slightly curved. A stub that
    /// would leave within [`MIN_STUB_ANGLE_RAD`] of a road already at `from` (their ribbons
    /// and sidewalks would overlap: Port Vesper node 81) is turned to the middle of the widest
    /// free angle there instead.
    fn cul_de_sac(&mut self, from: usize, towards: V2, len: f32, name: &'static str) {
        let a = self.pos(from);
        let mut d = geom::normalize(geom::sub(towards, a));
        let heads = self.headings_at(from);
        let angle = |u: V2| wp_math::atan2(u[1], u[0]);
        let gap = |x: f32, y: f32| (y - x).rem_euclid(std::f32::consts::TAU);
        let near = heads.iter().any(|h| {
            let g = gap(angle(*h), angle(d));
            g.min(std::f32::consts::TAU - g) < MIN_STUB_ANGLE_RAD
        });
        if near && !heads.is_empty() {
            let mut hs: Vec<f32> = heads.iter().map(|h| angle(*h)).collect();
            hs.sort_by(f32::total_cmp);
            let (mut best, mut widest) = (0.0f32, -1.0f32);
            for k in 0..hs.len() {
                let (x, y) = (hs[k], hs[(k + 1) % hs.len()]);
                let g = if hs.len() == 1 {
                    std::f32::consts::TAU
                } else {
                    gap(x, y)
                };
                if g > widest {
                    widest = g;
                    best = x + g * 0.5;
                }
            }
            let (sn, cs) = wp_math::sin_cos(best);
            d = [cs, sn];
        }
        let end = geom::add(a, geom::scale(d, len));
        let b = self.node(end);
        let bulge = self.rng.range_f32(-8.0, 8.0);
        let path = bent(a, end, bulge, 8.0);
        if self.road(from, b, path, SERVICE, name).is_none() {
            // Dropped: forget the orphan end node by leaving it unconnected (filtered later).
        }
    }

    /// Lay out every road; returns (loop roads, Hollins Drive, knob, lot, Route 40 W end,
    /// Route 40 N end).
    #[allow(clippy::needless_range_loop)]
    fn layout(&mut self) -> (Vec<usize>, usize, usize, usize, usize, usize) {
        // ---- Loop 9 -------------------------------------------------------------------
        let ctrl: Vec<V2> = vec![
            [-1460.0, -1000.0],
            [-400.0, -1075.0],
            [500.0, -1050.0],
            [1500.0, -1000.0],
            [1530.0, 250.0],
            [1500.0, 1300.0],
            [500.0, 1420.0],
            [-350.0, 1350.0],
            [-1460.0, 1430.0],
            [-1530.0, 350.0],
        ];
        let radii = [
            420.0, 1500.0, 1500.0, 420.0, 1500.0, 420.0, 1500.0, 1500.0, 420.0, 1500.0,
        ];
        let loop_path = resample(&fillet(&ctrl, &radii, true, 6.0), 6.0);
        let mut closed = loop_path.clone();
        closed.push(loop_path[0]);
        let lcum = cum2(&closed);
        let targets: [V2; 7] = [
            [-1490.0, -480.0], // Harbor Blvd west
            [1505.0, -450.0],  // Harbor Blvd east
            [-1515.0, 300.0],  // Lantern Way west
            [920.0, -1050.0],  // port connector
            [1300.0, -1030.0], // Terminal Ave east
            [1510.0, 300.0],   // Bluff Road
            [400.0, 1400.0],   // Uptown connector
        ];
        let cuts: Vec<f32> = targets
            .iter()
            .map(|t| project2(&closed, &lcum, *t).0)
            .collect();
        let (lj, loop_roads) = self.ring(&loop_path, &cuts, HIGHWAY, "Loop 9");
        let [lj_bw, lj_be, lj_lw, lj_port, lj_quay, lj_bluff, lj_up] =
            [lj[0], lj[1], lj[2], lj[3], lj[4], lj[5], lj[6]];

        // ---- Lantern Row downtown grid ------------------------------------------------
        let xs = [-300.0, -200.0, -100.0, 0.0, 100.0, 200.0, 300.0];
        let ys = [-350.0, -250.0, -150.0, -50.0, 50.0, 150.0, 250.0];
        let cls = |k: usize| match k {
            1 | 5 => ARTERIAL,
            3 => COLLECTOR,
            _ => LOCAL,
        };
        let mut dt = [[0usize; 7]; 7];
        for (j, y) in ys.iter().enumerate() {
            for (i, x) in xs.iter().enumerate() {
                dt[j][i] = self.node([*x, *y]);
            }
        }
        for j in 0..7 {
            for i in 0..6 {
                self.road(dt[j][i], dt[j][i + 1], Vec::new(), cls(j), "Lantern Row");
            }
        }
        for i in 0..7 {
            for j in 0..6 {
                self.road(dt[j][i], dt[j + 1][i], Vec::new(), cls(i), "Lantern Row");
            }
        }

        // ---- Harbor Boulevard ---------------------------------------------------------
        let blvd_ctrl = [
            self.pos(lj_bw),
            [-1150.0, -480.0],
            [-700.0, -560.0],
            [-300.0, -560.0],
            [300.0, -590.0],
            [800.0, -560.0],
            [1250.0, -470.0],
            self.pos(lj_be),
        ];
        let blvd = resample(&catmull_rom(&blvd_ctrl, 6.0), 6.0);
        let bcum = cum2(&blvd);
        let blvd_x = [
            -1000.0, -600.0, -300.0, -200.0, 0.0, 200.0, 300.0, 450.0, 720.0, 920.0, 1120.0,
            -1110.0, -800.0, -450.0,
        ];
        let bcuts: Vec<f32> = blvd_x
            .iter()
            .map(|x| {
                // First point at this x.
                let k = blvd
                    .windows(2)
                    .position(|w| (w[0][0] - x) * (w[1][0] - x) <= 0.0);
                let k = k.unwrap_or(0);
                let (a, b) = (blvd[k], blvd[k + 1]);
                let t = ((x - a[0]) / (b[0] - a[0]).max(1e-6)).clamp(0.0, 1.0);
                bcum[k] + t * (bcum[k + 1] - bcum[k])
            })
            .collect();
        let (bj, b_w, b_e) = self.chain(&blvd, &bcuts, BLVD, "Harbor Boulevard");
        // The ends are the loop nodes already.
        debug_assert!(b_w == lj_bw && b_e == lj_be);
        for (k, i) in [(2usize, 0usize), (3, 1), (4, 3), (5, 5), (6, 6)] {
            self.link(dt[0][i], bj[k], 0.0, cls(i), "Lantern Row");
        }

        // ---- Gantry Row port & Terminal Avenue ----------------------------------------
        let quay = [[380.0, -950.0], [1250.0, -950.0]];
        let qcuts = [450.0 - 380.0, 920.0 - 380.0];
        let (qj, q_w, q_e) = self.chain(&quay, &qcuts, PORT, "Terminal Avenue");
        self.link(bj[7], qj[0], 0.0, PORT, "Gantry Row");
        self.link(bj[9], qj[1], 0.0, PORT, "Gantry Row");
        self.link(bj[6], q_w, -10.0, SERVICE, "Gantry Row");
        self.link(qj[1], lj_port, 0.0, PORT, "Port Connector");
        self.link(q_e, lj_quay, 0.0, PORT, "Terminal Avenue");
        let s1 = self.node([450.0, -780.0]);
        let s2 = self.node([920.0, -780.0]);
        // Service road across the yard (splitting the two port columns).
        self.split_link(bj[7], qj[0], s1);
        self.split_link(bj[9], qj[1], s2);
        self.road(s1, s2, Vec::new(), SERVICE, "Gantry Row");

        // ---- Larkspur suburb (west) ---------------------------------------------------
        let lx = [-1250.0, -1050.0, -850.0, -650.0];
        let ly = [-300.0, -100.0, 100.0, 300.0];
        let sub = self.warped_grid(
            &lx,
            &ly,
            30.0,
            22.0,
            0.2,
            &|row, k| if row && k == 2 { COLLECTOR } else { LOCAL },
            "Larkspur",
        );
        self.link(sub[1][3], dt[2][0], 12.0, COLLECTOR, "Canal Street");
        self.link(sub[2][3], dt[4][0], -10.0, COLLECTOR, "Larkspur Avenue");
        self.link(sub[3][3], dt[5][0], 8.0, ARTERIAL, "Lantern Way");
        self.link(sub[3][0], lj_lw, -6.0, ARTERIAL, "Lantern Way");
        self.link(sub[0][1], bj[0], 6.0, LOCAL, "Larkspur");
        self.link(sub[0][3], bj[1], -6.0, COLLECTOR, "Ferris Road");
        let r40w = self.node([-1650.0, 100.0]);
        let r40w_start = sub[2][0];
        let path = bent(self.pos(r40w_start), [-1650.0, 100.0], 5.0, 8.0);
        self.road(r40w_start, r40w, path, ROUTE, "Route 40 West");
        for (j, i, dx, dy) in [
            (1, 1, 1.0, 1.0),
            (2, 2, -1.0, 1.0),
            (0, 2, 1.0, 1.0),
            (2, 1, -1.0, -1.0),
        ] {
            let a = self.pos(sub[j][i]);
            self.cul_de_sac(
                sub[j][i],
                [a[0] + 100.0 * dx, a[1] + 100.0 * dy],
                70.0,
                "Larkspur",
            );
        }

        // ---- Canal Flats (industrial, south-west waterfront) ---------------------------
        let cx = [-1110.0, -800.0, -450.0];
        let cy = [-900.0, -740.0];
        let canal = self.warped_grid(&cx, &cy, 12.0, 10.0, 0.0, &|_, _| SERVICE, "Canal Flats");
        self.link(canal[1][0], bj[11], 4.0, SERVICE, "Canal Flats");
        self.link(canal[1][1], bj[12], -4.0, SERVICE, "Canal Flats");
        self.link(canal[1][2], bj[13], 4.0, SERVICE, "Canal Flats");

        // ---- Mercer (north) -----------------------------------------------------------
        let mx = [-400.0, -200.0, 0.0, 200.0, 400.0];
        let my = [400.0, 560.0, 720.0, 880.0];
        let mer = self.warped_grid(
            &mx,
            &my,
            22.0,
            18.0,
            0.1,
            &|row, k| {
                if (!row && k == 2) || (row && k == 1) {
                    COLLECTOR
                } else {
                    LOCAL
                }
            },
            "Mercer",
        );
        for (i, m) in [(0usize, 0usize), (1, 1), (3, 2), (5, 3), (6, 4)] {
            let bulge = self.rng.range_f32(-10.0, 10.0);
            self.link(dt[6][i], mer[0][m], bulge, cls(i), "Mercer");
        }
        let r40n = self.node([0.0, 1650.0]);
        let r40n_path = bent(self.pos(mer[3][2]), [0.0, 1650.0], 4.0, 8.0);
        if let Some(r) = self.road(mer[3][2], r40n, r40n_path, ROUTE, "Route 40 North") {
            self.drafts[r].dips = true;
        }
        self.link(mer[3][4], lj_up, -12.0, ARTERIAL, "Uptown Connector");

        // ---- Eastgate (east) ----------------------------------------------------------
        let ex = [520.0, 720.0, 920.0, 1120.0];
        let ey = [-250.0, -50.0, 150.0, 330.0];
        let eg = self.warped_grid(
            &ex,
            &ey,
            25.0,
            20.0,
            0.15,
            &|row, k| if !row && k == 1 { COLLECTOR } else { LOCAL },
            "Eastgate",
        );
        self.link(dt[1][6], eg[0][0], 0.0, ARTERIAL, "Kettle Street");
        self.link(dt[3][6], eg[1][0], 6.0, COLLECTOR, "Eastgate");
        self.link(dt[5][6], eg[2][0], -6.0, ARTERIAL, "Lantern Way");
        self.link(eg[0][1], bj[8], 8.0, COLLECTOR, "Kettle Street");
        self.link(eg[0][3], bj[10], -8.0, LOCAL, "Eastgate");
        self.link(eg[3][0], mer[0][4], 6.0, LOCAL, "Eastgate");

        // ---- Bayview Bluffs: Bluff Road -----------------------------------------------
        let bluff_ctrl = [
            self.pos(mer[3][4]),
            [600.0, 960.0],
            [850.0, 1060.0],
            [1100.0, 1020.0],
            [1300.0, 840.0],
            [1380.0, 600.0],
            self.pos(lj_bluff),
        ];
        let bluff = resample(&catmull_rom(&bluff_ctrl, 6.0), 6.0);
        let fcum = cum2(&bluff);
        let fcuts = [
            project2(&bluff, &fcum, [900.0, 1060.0]).0,
            project2(&bluff, &fcum, [1310.0, 820.0]).0,
        ];
        let (fj, _, _) = self.chain(&bluff, &fcuts, BLUFF_ROAD, "Bluff Road");
        let bv = self.warped_grid(
            &[650.0, 900.0, 1150.0],
            &[600.0, 800.0],
            20.0,
            15.0,
            0.0,
            &|_, _| LOCAL,
            "Bayview",
        );
        self.link(eg[3][1], bv[0][0], 10.0, LOCAL, "Bayview");
        self.link(eg[3][3], bv[0][2], -10.0, LOCAL, "Bayview");
        self.link(bv[1][1], fj[0], 8.0, LOCAL, "Bayview");
        self.link(bv[1][2], fj[1], -8.0, LOCAL, "Bayview");

        // ---- Hollins Heights: Hollins Drive, Ridge Road, the knob lot ------------------
        let start = self.pos(mer[3][0]);
        let p1: V2 = [-620.0, 900.0];
        let u = geom::normalize(geom::sub(KNOB, p1));
        let v = [u[1], -u[0]]; // right of the climb direction
        let v = if v[1] < 0.0 { geom::scale(v, -1.0) } else { v };
        let mut ctrl = vec![start, p1];
        let mut p = p1;
        for k in 0..6 {
            let side = if k % 2 == 0 { 1.0 } else { -1.0 };
            p = geom::add(
                p,
                geom::add(geom::scale(v, 170.0 * side), geom::scale(u, 58.0)),
            );
            ctrl.push(p);
        }
        ctrl.push(KNOB);
        let mut rad = vec![0.0; ctrl.len()];
        rad[1] = 60.0;
        for r in rad.iter_mut().take(ctrl.len() - 1).skip(2) {
            *r = 22.0;
        }
        let hollins_path = resample(&fillet(&ctrl, &rad, false, 3.0), 5.0);
        let knob = self.node(KNOB);
        let hollins = self
            .road(mer[3][0], knob, hollins_path, HILL_ROAD, "Hollins Drive")
            .expect("Hollins Drive is laid first in its area");
        let lot = self.node([-1060.0, 1135.0]);
        self.link(knob, lot, 4.0, SERVICE, "Knob Observatory Lot");
        let ridge_ctrl = [
            KNOB,
            [-900.0, 1240.0],
            [-700.0, 1300.0],
            [-480.0, 1190.0],
            [-300.0, 1000.0],
            self.pos(mer[3][1]),
        ];
        let ridge = resample(&catmull_rom(&ridge_ctrl, 5.0), 5.0);
        self.road(knob, mer[3][1], ridge, HILL_ROAD, "Ridge Road");

        (loop_roads, hollins, knob, lot, r40w, r40n)
    }

    /// Replace the straight road `a`–`b` by two roads through `mid`.
    fn split_link(&mut self, a: usize, b: usize, mid: usize) {
        if let Some(k) = self
            .drafts
            .iter()
            .position(|d| (d.a == a && d.b == b) || (d.a == b && d.b == a))
        {
            let d = self.drafts.remove(k);
            self.road(d.a, mid, Vec::new(), d.st, d.name);
            self.road(mid, d.b, Vec::new(), d.st, d.name);
        }
    }
}

fn connected(edges: &[(usize, usize, bool, usize, f32)], keep: &[bool]) -> bool {
    let mut nodes: Vec<usize> = edges.iter().flat_map(|e| [e.0, e.1]).collect();
    nodes.sort_unstable();
    nodes.dedup();
    let mut seen = vec![nodes[0]];
    let mut stack = vec![nodes[0]];
    while let Some(n) = stack.pop() {
        for (e, k) in edges.iter().zip(keep) {
            if !*k {
                continue;
            }
            let o = if e.0 == n {
                e.1
            } else if e.1 == n {
                e.0
            } else {
                continue;
            };
            if !seen.contains(&o) {
                seen.push(o);
                stack.push(o);
            }
        }
    }
    seen.len() == nodes.len()
}

// ------------------------------------------------------------------------------------------
// Vertical profiles
// ------------------------------------------------------------------------------------------

/// Half paved width of a style (widest side), m.
fn half_width(st: &Style, opt: &BuildOptions) -> f32 {
    builder::half_width_of(st.class, st.fwd, st.bwd, st.parking, opt)
}

/// Pinned flat length at each node (junction box estimate), m.
fn flat_lengths(g: &Gen, opt: &BuildOptions) -> Vec<f32> {
    let mut deg = vec![0usize; g.nodes.len()];
    let mut hw = vec![0.0f32; g.nodes.len()];
    for d in &g.drafts {
        for n in [d.a, d.b] {
            deg[n] += 1;
            hw[n] = hw[n].max(half_width(&d.st, opt));
        }
    }
    (0..g.nodes.len())
        .map(|n| if deg[n] >= 3 { hw[n] * 1.6 + 4.0 } else { 4.0 })
        .collect()
}

/// Design crest / sag limit: vertical acceleration ≤ 0.3 g at the limit (+15 %).
fn max_vertical_curvature(speed: f32) -> f32 {
    let v = speed * 1.15;
    0.3 * 9.81 / (v * v)
}

/// Profile `z(s)` on uniform samples `ds` apart: follows `desired`, holds `anchors`
/// (sample ranges with fixed heights) and is never steeper than `grade`; then smoothed with
/// enough binomial passes that a full grade reversal (crest or sag) bends no tighter than
/// `kmax` (1/m). During smoothing only the two end samples and the middle of each internal
/// anchor (decks) stay pinned, so junction approaches round off gently instead of kinking.
fn profile(
    desired: &[f32],
    ds: f32,
    anchors: &[(usize, usize, f32)],
    internal: &[(usize, usize, f32)],
    pins: (usize, usize),
    grade: f32,
    kmax: f32,
) -> Vec<f32> {
    let n = desired.len();
    let g = grade * ds;
    let mut fixed: Vec<Option<f32>> = vec![None; n];
    let all: Vec<(usize, usize, f32)> = anchors.iter().chain(internal).copied().collect();
    for &(i0, i1, z) in &all {
        for f in fixed.iter_mut().take(i1.min(n - 1) + 1).skip(i0) {
            *f = Some(z);
        }
    }
    // Cones from the anchors.
    let mut lo = vec![f32::NEG_INFINITY; n];
    let mut hi = vec![f32::INFINITY; n];
    for &(i0, i1, z) in &all {
        for i in 0..n {
            let d = if i < i0 {
                (i0 - i) as f32
            } else if i > i1 {
                (i - i1) as f32
            } else {
                0.0
            };
            lo[i] = lo[i].max(z - g * d);
            hi[i] = hi[i].min(z + g * d);
        }
    }
    let mut z: Vec<f32> = (0..n)
        .map(|i| match fixed[i] {
            Some(f) => f,
            None if lo[i] <= hi[i] => desired[i].clamp(lo[i], hi[i]),
            None => 0.5 * (lo[i] + hi[i]),
        })
        .collect();
    // Grade limit: a forward-only and a backward-only clamp, averaged (both are within the
    // anchor cones and grade-limited, so their mean is too; the result is symmetric).
    let mut fwd = z.clone();
    for i in 1..n {
        if fixed[i].is_none() {
            fwd[i] = fwd[i].clamp(fwd[i - 1] - g, fwd[i - 1] + g);
        }
    }
    let mut bwd = z.clone();
    for i in (0..n - 1).rev() {
        if fixed[i].is_none() {
            bwd[i] = bwd[i].clamp(bwd[i + 1] - g, bwd[i + 1] + g);
        }
    }
    for i in 0..n {
        if fixed[i].is_none() {
            z[i] = 0.5 * (fwd[i] + bwd[i]);
        }
    }
    // Binomial smoothing ≈ Gaussian with sigma = ds * sqrt(passes / 2). A slope change of
    // 2 * grade smoothed by sigma peaks at 2 * grade / (sigma * sqrt(2 pi)) curvature.
    let sigma = smoothing_sigma(grade, kmax, ds);
    let passes = (2.0 * (sigma / ds) * (sigma / ds)).ceil() as usize;
    let mut pin = vec![false; n];
    for p in pin.iter_mut().take(pins.0.max(1)) {
        *p = true;
    }
    for p in pin.iter_mut().skip(n.saturating_sub(pins.1.max(1))) {
        *p = true;
    }
    for &(i0, i1, _) in internal {
        let q = (i1.saturating_sub(i0)) / 4;
        for p in pin.iter_mut().take((i1 - q).min(n - 1) + 1).skip(i0 + q) {
            *p = true;
        }
    }
    for _ in 0..passes {
        let prev = z.clone();
        for i in 1..n - 1 {
            if !pin[i] {
                z[i] = 0.25 * prev[i - 1] + 0.5 * prev[i] + 0.25 * prev[i + 1];
            }
        }
    }
    z
}

/// Smoothing width (m) that rounds a full grade reversal to curvature `kmax`.
fn smoothing_sigma(grade: f32, kmax: f32, ds: f32) -> f32 {
    (2.0 * grade / (kmax * 2.5)).max(2.0 * ds)
}

/// Signed curvature (1/m, + = left turn) at each point of a polyline.
fn curvature(p: &[V2]) -> Vec<f32> {
    let n = p.len();
    (0..n)
        .map(|i| {
            if i == 0 || i + 1 >= n {
                return 0.0;
            }
            let a = geom::sub(p[i], p[i - 1]);
            let b = geom::sub(p[i + 1], p[i]);
            let ang = geom::atan2(geom::cross(a, b), geom::dot(a, b));
            ang / (0.5 * (geom::len(a) + geom::len(b))).max(1e-3)
        })
        .collect()
}

/// Moving average over `half` points each side.
fn smooth_series(v: &[f32], half: usize) -> Vec<f32> {
    let n = v.len();
    (0..n)
        .map(|i| {
            let a = i.saturating_sub(half);
            let b = (i + half).min(n - 1);
            v[a..=b].iter().sum::<f32>() / (b - a + 1) as f32
        })
        .collect()
}

/// Turn drafts into builder roads: heights, via points, bank.
fn profile_roads(g: &mut Gen, opt: &BuildOptions) -> Vec<RoadSpec> {
    let flat = flat_lengths(g, opt);
    // Node heights: natural ground, relaxed so every road can meet its grade cap.
    for k in 0..g.nodes.len() {
        let p = g.nodes[k].pos;
        g.nodes[k].z = g.nat.z(p);
    }
    for _ in 0..200 {
        let mut moved = false;
        for d in &g.drafts {
            let len = *cum2(&d.path).last().unwrap();
            let spread = 2.0 * smoothing_sigma(d.st.grade, max_vertical_curvature(d.st.speed), 4.0);
            let room = (len - flat[d.a] - flat[d.b] - 2.0 * spread).max(len * 0.3);
            let allow = d.st.grade * room * 0.85;
            let (za, zb) = (g.nodes[d.a].z, g.nodes[d.b].z);
            let excess = (za - zb).abs() - allow;
            if excess > 0.01 {
                let step = 0.5 * excess * (za - zb).signum();
                g.nodes[d.a].z -= step * 0.5;
                g.nodes[d.b].z += step * 0.5;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }

    let ds = 4.0;
    let mut out = Vec::with_capacity(g.drafts.len());
    for d in &g.drafts {
        let cum = cum2(&d.path);
        let len = cum[cum.len() - 1];
        let n = ((len / ds).ceil() as usize).max(1) + 1;
        let dsr = len / (n - 1) as f32;
        let pts: Vec<V2> = (0..n)
            .map(|i| point_on(&d.path, &cum, i as f32 * dsr).0)
            .collect();
        let raw: Vec<f32> = pts.iter().map(|p| g.nat.z(*p)).collect();
        let desired = smooth_series(&raw, (12.0 / dsr) as usize);
        let (za, zb) = (g.nodes[d.a].z, g.nodes[d.b].z);
        let kmax = max_vertical_curvature(d.st.speed);
        // Junction box (pinned flat) + smoothing run-out (still flat) before any grade.
        let spread = 2.0 * smoothing_sigma(d.st.grade, kmax, dsr);
        let pa = (flat[d.a] / dsr).ceil() as usize;
        let pb = (flat[d.b] / dsr).ceil() as usize;
        let fa = ((flat[d.a] + spread) / dsr).ceil() as usize;
        let fb = ((flat[d.b] + spread) / dsr).ceil() as usize;
        let anchors = vec![
            (0, fa.min(n - 1), za),
            ((n - 1).saturating_sub(fb), n - 1, zb),
        ];
        let internal: Vec<(usize, usize, f32)> = d
            .anchors
            .iter()
            .map(|&(s0, s1, z)| {
                (
                    (s0 / dsr).floor().max(0.0) as usize,
                    ((s1 / dsr).ceil() as usize).min(n - 1),
                    z,
                )
            })
            .collect();
        let short = fa + fb + 3 >= n;
        let z = if short && d.anchors.is_empty() {
            (0..n)
                .map(|i| za + (zb - za) * i as f32 / (n - 1) as f32)
                .collect()
        } else {
            profile(
                &desired,
                dsr,
                &anchors,
                &internal,
                (pa, pb),
                d.st.grade,
                kmax,
            )
        };
        let linear_dev = (0..n)
            .map(|i| (z[i] - (za + (zb - za) * i as f32 / (n - 1) as f32)).abs())
            .fold(0.0f32, f32::max);
        let mut r = RoadSpec::new(d.a, d.b, d.st.class, d.st.fwd, d.st.bwd);
        r.speed_mps = d.st.speed;
        r.parking = d.st.parking;
        if linear_dev < 0.25 && d.anchors.is_empty() {
            r.via = d.path[1..d.path.len() - 1].to_vec();
        } else {
            let step = if d.st.class == RoadClass::Highway {
                10.0
            } else {
                6.0
            };
            let res = resample(&d.path, step);
            let rc = cum2(&res);
            r.via = res[1..res.len() - 1].to_vec();
            r.via_z = rc[1..rc.len() - 1]
                .iter()
                .map(|s| {
                    let f = s / dsr;
                    let i = (f.floor() as usize).min(n - 2);
                    let t = f - i as f32;
                    z[i] + (z[i + 1] - z[i]) * t
                })
                .collect();
        }
        if d.st.bank_max > 0.0 {
            let mut all = vec![d.path[0]];
            all.extend_from_slice(&r.via);
            all.push(d.path[d.path.len() - 1]);
            let ac = cum2(&all);
            let kappa = curvature(&all);
            let v2 = d.st.speed * d.st.speed;
            let raw: Vec<f32> = kappa
                .iter()
                .map(|k| (k * v2 / 9.81 * 0.55).clamp(-d.st.bank_max, d.st.bank_max))
                .collect();
            let spacing = (len / (all.len() - 1).max(1) as f32).max(1.0);
            let mut bank = smooth_series(&raw, (40.0 / spacing) as usize);
            for (b, s) in bank.iter_mut().zip(&ac) {
                let from_a = s - flat[d.a];
                let from_b = len - s - flat[d.b];
                *b *= smoothstep(0.0, 60.0, from_a.min(from_b));
            }
            r.bank = bank;
        }
        out.push(r);
    }
    out
}

/// Plan grade separations where the highway crosses another road.
fn plan_crossings(g: &mut Gen) -> Vec<Crossing> {
    let mut out = Vec::new();
    for hi in 0..g.drafts.len() {
        if g.drafts[hi].st.class != RoadClass::Highway {
            continue;
        }
        for lo in 0..g.drafts.len() {
            if g.drafts[lo].st.class == RoadClass::Highway {
                continue;
            }
            for (s_hi, s_lo, p) in path_crossings(&g.drafts[hi].path, &g.drafts[lo].path) {
                let ground = g.nat.z(p);
                let underpass = g.drafts[lo].dips;
                let (z_hi, z_lo) = if underpass {
                    (ground + 0.5, ground + 0.5 - CROSSING_CLEARANCE_M)
                } else {
                    (ground + CROSSING_CLEARANCE_M, ground)
                };
                let deck = 30.0;
                g.drafts[hi].anchors.push((s_hi - deck, s_hi + deck, z_hi));
                g.drafts[hi]
                    .decks
                    .push((s_hi - deck - 4.0, s_hi + deck + 4.0));
                g.drafts[lo].anchors.push((s_lo - 14.0, s_lo + 14.0, z_lo));
                out.push(Crossing {
                    upper: hi,
                    lower: lo,
                    pos: p,
                    underpass,
                });
            }
        }
    }
    out
}

/// Plan the covered roads: the Hollins Tunnel on Loop 9 and Founders Cut on the Uptown
/// Connector. A dipped cover (`dip` m below the lowest natural ground over it) gets an internal
/// height anchor, so the profile ramps down into it within the grade cap. The plans live on
/// the drafts.
fn plan_covers(g: &mut Gen) {
    let plans: [(&str, V2, V2, V2, f32, CoverKind, &'static str); 2] = [
        // Loop 9 through the Hollins Heights shoulder (it already runs in a cut there).
        (
            "Loop 9",
            [-1000.0, 1390.0],
            [-720.0, 1376.0],
            [-1110.0, 1399.0],
            0.0,
            CoverKind::Tunnel,
            "Hollins Tunnel",
        ),
        // The Uptown Connector under a plaza deck.
        (
            "Uptown Connector",
            [416.0, 1100.0],
            [418.0, 1005.0],
            [414.0, 1255.0],
            7.0,
            CoverKind::CoveredTrench,
            "Founders Cut",
        ),
    ];
    for (road, probe, p0, p1, dip, kind, name) in plans {
        let Some(ri) = (0..g.drafts.len()).find(|k| {
            let d = &g.drafts[*k];
            d.name == road && project2(&d.path, &cum2(&d.path), probe).1 < 40.0
        }) else {
            continue;
        };
        let d = &g.drafts[ri];
        let cum = cum2(&d.path);
        let (a, b) = (project2(&d.path, &cum, p0).0, project2(&d.path, &cum, p1).0);
        let (s0, s1, p0, p1) = if a <= b {
            (a, b, p0, p1)
        } else {
            (b, a, p1, p0)
        };
        if dip > 0.0 {
            let mut lowest = f32::INFINITY;
            let mut s = s0;
            while s <= s1 {
                lowest = lowest.min(g.nat.z(point_on(&d.path, &cum, s).0));
                s += 4.0;
            }
            g.drafts[ri].anchors.push((s0, s1, lowest - dip));
        }
        g.drafts[ri].covers.push(CoverPlan { p0, p1, kind, name });
    }
}

/// Covered spans on the built road surfaces (portal to portal), one per plan.
fn covered_spans(g: &Gen, built: &Built) -> Vec<CoveredSpan> {
    let mut out = Vec::new();
    for (ri, d) in g.drafts.iter().enumerate() {
        let s = &built.surfaces[ri];
        let cum = geom::cumulative(&s.centre);
        for c in &d.covers {
            let u0 = geom::project(&s.centre, &cum, c.p0).s.max(0.0);
            let u1 = geom::project(&s.centre, &cum, c.p1)
                .s
                .min(cum[cum.len() - 1]);
            let (u0, u1) = (u0.min(u1), u0.max(u1));
            let mut centre = vec![geom::point_at(&s.centre, &cum, u0).0];
            for (k, p) in s.centre.iter().enumerate() {
                if cum[k] > u0 + 0.5 && cum[k] < u1 - 0.5 {
                    centre.push(*p);
                }
            }
            centre.push(geom::point_at(&s.centre, &cum, u1).0);
            let edge = s.left_m.max(s.right_m);
            let walk = if s.sidewalk[0] || s.sidewalk[1] {
                3.5
            } else {
                1.5
            };
            out.push(CoveredSpan {
                name: c.name.into(),
                kind: c.kind,
                road: ri as u32,
                centre,
                half_width_m: edge + walk,
                clearance_m: if s.highway {
                    TUNNEL_CLEARANCE_HIGHWAY_M
                } else {
                    TUNNEL_CLEARANCE_M
                },
            });
        }
    }
    out
}

/// Fill the ground back over a covered span: beyond the carved shoulder each side, up to the
/// roof deck (or the natural ground, if lower). Inside the walls and under the deck overhang
/// the carve stays, so the terrain never reaches into the tube.
fn backfill_covers(hf: &mut Heightfield, nat: &Natural, spans: &[CoveredSpan]) {
    for span in spans {
        let shape = CoverShape::new(span);
        let inner = span.half_width_m + WALL_M + DECK_OVERHANG_M - 1.0;
        let reach = span.half_width_m + 45.0;
        let (lo, hi) = (shape.bounds[0], shape.bounds[1]);
        let cell = hf.cell_m;
        let i0 = (((lo[0] - 45.0 - hf.origin[0]) / cell).floor().max(0.0)) as u32;
        let j0 = (((lo[1] - 45.0 - hf.origin[1]) / cell).floor().max(0.0)) as u32;
        let i1 = ((((hi[0] + 45.0 - hf.origin[0]) / cell).ceil()) as u32).min(hf.nx - 1);
        let j1 = ((((hi[1] + 45.0 - hf.origin[1]) / cell).ceil()) as u32).min(hf.ny - 1);
        for j in j0..=j1 {
            for i in i0..=i1 {
                let p = hf.vertex_pos(i, j);
                let q = shape.locate(p);
                if q.s < 0.0 || q.s > shape.length() {
                    continue;
                }
                let lat = q.lateral.abs();
                if lat < inner || lat > reach {
                    continue;
                }
                let roof_top = q.road_z + span.clearance_m + ROOF_M;
                let v = (j * hf.nx + i) as usize;
                let fill = nat.z(p).min(roof_top);
                if fill > hf.z[v] {
                    hf.z[v] = (fill * 1000.0).round() / 1000.0;
                }
            }
        }
    }
}

/// Whether a building footprint would stand in a covered span's trench (inside the deck).
fn in_cover_corridor(shapes: &[CoverShape], fp: &[V2]) -> bool {
    shapes.iter().any(|sh| {
        let r = sh.span.half_width_m + WALL_M + DECK_OVERHANG_M + 1.0;
        let mut c = [0.0f32; 2];
        for q in fp {
            c = [c[0] + q[0] / fp.len() as f32, c[1] + q[1] / fp.len() as f32];
        }
        fp.iter().chain(std::iter::once(&c)).any(|q| {
            let l = sh.locate(*q);
            l.s >= -2.0 && l.s <= sh.length() + 2.0 && l.lateral.abs() <= r
        })
    })
}

// ------------------------------------------------------------------------------------------
// Terrain carving
// ------------------------------------------------------------------------------------------

/// Paved radius of every node's junction / bend area: reaching the trimmed road ends (their
/// centre distance plus half width) and at least the intersection radius; 0 for unused nodes.
fn node_radii(built: &Built, nodes: &[NodeSpec]) -> Vec<f32> {
    let mut radius = vec![0.0f32; nodes.len()];
    for (s, rl) in built.surfaces.iter().zip(&built.road_lanes) {
        let _ = rl;
        let lane0 = built.lanes.iter().find(|l| l.road == s.road);
        let Some(lane0) = lane0 else { continue };
        let (na, nb) = if lane0.from_node as usize == lane0.to_node as usize {
            (lane0.from_node as usize, lane0.to_node as usize)
        } else {
            // Surface centre runs a → b of the source road; find which node is which by
            // distance.
            let (f, t) = (lane0.from_node as usize, lane0.to_node as usize);
            let c0 = geom::xy(s.centre[0]);
            if geom::dist(c0, nodes[f].pos) <= geom::dist(c0, nodes[t].pos) {
                (f, t)
            } else {
                (t, f)
            }
        };
        let w = s.left_m.max(s.right_m);
        radius[na] = radius[na].max(geom::dist(geom::xy(s.centre[0]), nodes[na].pos) + w);
        let e = geom::xy(s.centre[s.centre.len() - 1]);
        radius[nb] = radius[nb].max(geom::dist(e, nodes[nb].pos) + w);
    }
    for ix in &built.intersections {
        let n = ix.node as usize;
        radius[n] = radius[n].max(ix.radius_m);
    }
    radius
}

/// Carve `hf` (natural ground) to the built road surfaces and junction areas. `deck(road,
/// point index)` says whether a surface point is a bridge deck (left uncarved).
fn carve(
    hf: &mut Heightfield,
    built: &Built,
    nodes: &[NodeSpec],
    deck: &dyn Fn(usize, usize) -> bool,
) {
    let nv = hf.z.len();
    let natural = hf.z.clone();
    let mut core_min = vec![f32::INFINITY; nv];
    let mut acc_w = vec![0.0f32; nv];
    let mut acc_wt = vec![0.0f32; nv];
    let mut best_d = vec![f32::INFINITY; nv];
    let mut best_t = vec![0.0f32; nv];
    let mut stamp = vec![u32::MAX; nv];
    let mut touched: Vec<usize> = Vec::new();
    let cell = hf.cell_m;
    let (nx, ny) = (hf.nx as i64, hf.ny as i64);
    let origin = hf.origin;

    // A "source" is one road surface or one junction disc; per vertex the nearest point of
    // each source counts once.
    let finish_source = |touched: &mut Vec<usize>,
                         best_d: &[f32],
                         best_t: &[f32],
                         core_min: &mut [f32],
                         acc_w: &mut [f32],
                         acc_wt: &mut [f32],
                         shoulder: f32,
                         blend: f32| {
        for &v in touched.iter() {
            let d = best_d[v];
            let t = best_t[v];
            if d <= shoulder {
                core_min[v] = core_min[v].min(t);
            } else {
                let w = smoothstep(shoulder + blend, shoulder, d);
                acc_w[v] += w;
                acc_wt[v] += w * t;
            }
        }
        touched.clear();
    };
    let visit = |id: u32,
                 lo: V2,
                 hi: V2,
                 touched: &mut Vec<usize>,
                 best_d: &mut [f32],
                 best_t: &mut [f32],
                 stamp: &mut [u32],
                 f: &mut dyn FnMut(V2) -> (f32, f32)| {
        let i0 = (((lo[0] - origin[0]) / cell).floor() as i64).clamp(0, nx - 1);
        let i1 = (((hi[0] - origin[0]) / cell).ceil() as i64).clamp(0, nx - 1);
        let j0 = (((lo[1] - origin[1]) / cell).floor() as i64).clamp(0, ny - 1);
        let j1 = (((hi[1] - origin[1]) / cell).ceil() as i64).clamp(0, ny - 1);
        for j in j0..=j1 {
            for i in i0..=i1 {
                let v = (j * nx + i) as usize;
                let p = [origin[0] + i as f32 * cell, origin[1] + j as f32 * cell];
                let (d, t) = f(p);
                if stamp[v] != id {
                    stamp[v] = id;
                    best_d[v] = d;
                    best_t[v] = t;
                    touched.push(v);
                } else if d < best_d[v] {
                    best_d[v] = d;
                    best_t[v] = t;
                }
            }
        }
    };

    let mut source = 0u32;
    for (ri, s) in built.surfaces.iter().enumerate() {
        let blend = if s.highway { 22.0 } else { 15.0 };
        let shoulder = if s.highway {
            SHOULDER_M
        } else {
            STREET_SHOULDER_M
        };
        let reach = s.left_m.max(s.right_m) + shoulder + blend;
        for k in 0..s.centre.len() - 1 {
            let (a, b) = (s.centre[k], s.centre[k + 1]);
            if deck(ri, k) || deck(ri, k + 1) {
                continue;
            }
            let (pa, pb) = (geom::xy(a), geom::xy(b));
            let lo = [pa[0].min(pb[0]) - reach, pa[1].min(pb[1]) - reach];
            let hi = [pa[0].max(pb[0]) + reach, pa[1].max(pb[1]) + reach];
            let ab = geom::sub(pb, pa);
            let l = geom::len(ab).max(1e-4);
            let dir = geom::scale(ab, 1.0 / l);
            let (bank_a, bank_b) = (s.bank[k], s.bank[k + 1]);
            let mut f = |p: V2| {
                let w = geom::sub(p, pa);
                let along = geom::dot(w, dir);
                let t = (along / l).clamp(0.0, 1.0);
                let lat = geom::dot(w, geom::right_of(dir));
                let edge = if lat > 0.0 { s.right_m } else { s.left_m };
                let d_out = (lat.abs() - edge).max(0.0);
                let d_end = (-along).max(along - l).max(0.0);
                let d = (d_out * d_out + d_end * d_end).sqrt();
                let zc = a[2] + (b[2] - a[2]) * t;
                let bank = bank_a + (bank_b - bank_a) * t;
                let tz = if s.highway || d_out <= 0.0 {
                    zc + bank * lat.clamp(-s.left_m, s.right_m) - CARVE_BELOW_M
                } else {
                    // Streets: up to just under the sidewalk / lot level (the sidewalk
                    // continues the bank to its back edge; the lot is level beyond).
                    let lat_walk = lat.signum() * (edge + d_out.min(SIDEWALK_W_M));
                    zc + bank * lat_walk + KERB_H_M + street_carve_offset(d_out)
                };
                (d, tz)
            };
            visit(
                source,
                lo,
                hi,
                &mut touched,
                &mut best_d,
                &mut best_t,
                &mut stamp,
                &mut f,
            );
        }
        finish_source(
            &mut touched,
            &best_d,
            &best_t,
            &mut core_min,
            &mut acc_w,
            &mut acc_wt,
            shoulder,
            blend,
        );
        source += 1;
    }
    // Junction and bend areas: a disc per node reaching the trimmed road ends.
    let radius = node_radii(built, nodes);
    // Nodes where a street meets: their lots take over just past the paved radius.
    let mut street_node = vec![false; nodes.len()];
    for l in &built.lanes {
        if l.kind != LaneKind::Highway {
            street_node[l.from_node as usize] = true;
            street_node[l.to_node as usize] = true;
        }
    }
    for (n, node) in nodes.iter().enumerate() {
        if radius[n] <= 0.0 {
            continue;
        }
        let blend = 15.0;
        let shoulder = if street_node[n] {
            NODE_SHOULDER_M
        } else {
            SHOULDER_M
        };
        let r = radius[n];
        let reach = r + shoulder + blend;
        let c = node.pos;
        let lo = [c[0] - reach, c[1] - reach];
        let hi = [c[0] + reach, c[1] + reach];
        let z = node.z - CARVE_BELOW_M;
        let mut f = |p: V2| ((geom::dist(p, c) - r).max(0.0), z);
        visit(
            source,
            lo,
            hi,
            &mut touched,
            &mut best_d,
            &mut best_t,
            &mut stamp,
            &mut f,
        );
        finish_source(
            &mut touched,
            &best_d,
            &best_t,
            &mut core_min,
            &mut acc_w,
            &mut acc_wt,
            shoulder,
            blend,
        );
        source += 1;
    }
    // Under bridge decks the ground stays at least 0.5 m below the deck (the blend from the
    // carved approaches would otherwise lift it into the deck).
    let mut under = vec![f32::INFINITY; nv];
    for (ri, s) in built.surfaces.iter().enumerate() {
        for k in 0..s.centre.len() - 1 {
            if !(deck(ri, k) || deck(ri, k + 1)) {
                continue;
            }
            let (a, b) = (s.centre[k], s.centre[k + 1]);
            let (pa, pb) = (geom::xy(a), geom::xy(b));
            let reach = s.left_m.max(s.right_m) + 1.0;
            let lo = [pa[0].min(pb[0]) - reach, pa[1].min(pb[1]) - reach];
            let hi = [pa[0].max(pb[0]) + reach, pa[1].max(pb[1]) + reach];
            let ab = geom::sub(pb, pa);
            let l = geom::len(ab).max(1e-4);
            let dir = geom::scale(ab, 1.0 / l);
            let i0 = (((lo[0] - origin[0]) / cell).floor() as i64).clamp(0, nx - 1);
            let i1 = (((hi[0] - origin[0]) / cell).ceil() as i64).clamp(0, nx - 1);
            let j0 = (((lo[1] - origin[1]) / cell).floor() as i64).clamp(0, ny - 1);
            let j1 = (((hi[1] - origin[1]) / cell).ceil() as i64).clamp(0, ny - 1);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let p = [origin[0] + i as f32 * cell, origin[1] + j as f32 * cell];
                    let w = geom::sub(p, pa);
                    let along = geom::dot(w, dir);
                    let lat = geom::dot(w, geom::right_of(dir));
                    if along < -cell
                        || along > l + cell
                        || lat < -s.left_m - cell
                        || lat > s.right_m + cell
                    {
                        continue;
                    }
                    let t = (along / l).clamp(0.0, 1.0);
                    let lat = lat.clamp(-s.left_m, s.right_m);
                    let z = a[2]
                        + (b[2] - a[2]) * t
                        + (s.bank[k] + (s.bank[k + 1] - s.bank[k]) * t) * lat;
                    let v = (j * nx + i) as usize;
                    under[v] = under[v].min(z - 0.5);
                }
            }
        }
    }
    for v in 0..nv {
        hf.z[v] = if core_min[v].is_finite() {
            core_min[v]
        } else if acc_w[v] <= 1.0 {
            natural[v] + acc_wt[v] - acc_w[v] * natural[v]
        } else {
            acc_wt[v] / acc_w[v]
        };
        hf.z[v] = hf.z[v].min(under[v]);
        // Quantise to millimetres: keeps the JSON dump small and the data obviously exact.
        hf.z[v] = (hf.z[v] * 1000.0).round() / 1000.0;
    }
}

// ------------------------------------------------------------------------------------------
// Zones, buildings, POIs
// ------------------------------------------------------------------------------------------

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<V2> {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

fn zones() -> Vec<Zone> {
    let z = |kind: DistrictKind, poly: Vec<V2>| Zone {
        kind,
        density_base: default_density_base(kind),
        polygon: poly,
    };
    vec![
        z(DistrictKind::Core, rect(-250.0, -300.0, 250.0, 200.0)),
        z(DistrictKind::Commercial, rect(-350.0, -420.0, 350.0, 320.0)),
        z(
            DistrictKind::Commercial,
            rect(-1200.0, -650.0, 1300.0, -420.0),
        ),
        z(DistrictKind::Port, rect(300.0, -1300.0, 1700.0, -650.0)),
        z(
            DistrictKind::Industrial,
            rect(-1700.0, -1300.0, 300.0, -650.0),
        ),
        z(
            DistrictKind::Residential,
            rect(-500.0, 320.0, 500.0, 1000.0),
        ),
        z(
            DistrictKind::Residential,
            rect(350.0, -420.0, 1300.0, 400.0),
        ),
        z(
            DistrictKind::Residential,
            rect(-1400.0, 650.0, -500.0, 1500.0),
        ),
        z(
            DistrictKind::Residential,
            rect(500.0, 400.0, 1700.0, 1500.0),
        ),
        z(DistrictKind::Suburb, rect(-1700.0, -420.0, -350.0, 650.0)),
        z(DistrictKind::Suburb, rect(-1700.0, -1300.0, 1700.0, 1750.0)),
    ]
}

fn zone_kind(zones: &[Zone], p: V2) -> DistrictKind {
    zones
        .iter()
        .find(|z| geom::point_in_polygon(p, &z.polygon))
        .map_or(DistrictKind::Suburb, |z| z.kind)
}

/// Uniform grid of road-surface segments for clearance tests.
struct SurfaceGrid {
    cell: f32,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<(u32, u32)>>,
}

impl SurfaceGrid {
    fn new(built: &Built) -> Self {
        let cell = 40.0;
        let nx = ((MAP_HI[0] - MAP_LO[0]) / cell).ceil() as usize + 1;
        let ny = ((MAP_HI[1] - MAP_LO[1]) / cell).ceil() as usize + 1;
        let mut g = Self {
            cell,
            nx,
            ny,
            cells: vec![Vec::new(); nx * ny],
        };
        for (si, s) in built.surfaces.iter().enumerate() {
            let pad = s.left_m.max(s.right_m) + 2.0;
            for k in 0..s.centre.len() - 1 {
                let (a, b) = (geom::xy(s.centre[k]), geom::xy(s.centre[k + 1]));
                let lo = [a[0].min(b[0]) - pad, a[1].min(b[1]) - pad];
                let hi = [a[0].max(b[0]) + pad, a[1].max(b[1]) + pad];
                for c in g.cells_in(lo, hi) {
                    g.cells[c].push((si as u32, k as u32));
                }
            }
        }
        g
    }

    fn cells_in(&self, lo: V2, hi: V2) -> Vec<usize> {
        let ci =
            |v: f32, o: f32, n: usize| (((v - o) / self.cell).floor().max(0.0) as usize).min(n - 1);
        let (i0, i1) = (ci(lo[0], MAP_LO[0], self.nx), ci(hi[0], MAP_LO[0], self.nx));
        let (j0, j1) = (ci(lo[1], MAP_LO[1], self.ny), ci(hi[1], MAP_LO[1], self.ny));
        let mut out = Vec::new();
        for j in j0..=j1 {
            for i in i0..=i1 {
                out.push(j * self.nx + i);
            }
        }
        out
    }

    /// Smallest distance from `p` to any paved edge (negative = on a road).
    fn clearance(&self, built: &Built, p: V2) -> f32 {
        let mut best = f32::INFINITY;
        for c in self.cells_in(p, p) {
            for &(si, k) in &self.cells[c] {
                let s = &built.surfaces[si as usize];
                let (a, b) = (
                    geom::xy(s.centre[k as usize]),
                    geom::xy(s.centre[k as usize + 1]),
                );
                let d = geom::point_segment_distance(p, a, b) - s.left_m.max(s.right_m);
                let d = if s.highway { d - 12.0 } else { d };
                best = best.min(d);
            }
        }
        best
    }
}

fn obb_corners(o: &Obb) -> [V2; 4] {
    let r = geom::right_of(o.f);
    let f = geom::scale(o.f, o.hl);
    let w = geom::scale(r, o.hw);
    [
        geom::sub(geom::sub(o.c, f), w),
        geom::add(geom::sub(o.c, f), w),
        geom::add(geom::add(o.c, f), w),
        geom::sub(geom::add(o.c, f), w),
    ]
}

/// Facade setback behind the sidewalk's back edge (on the lot), m.
const FRONT_SETBACK_M: f32 = 1.5;

/// Clearance from every paved edge a building corner keeps: the sidewalk and a metre more,
/// so no building stands on another street's sidewalk, m.
const BUILDING_CLEARANCE_M: f32 = SIDEWALK_W_M + 1.0;

/// Centre height and cross slope of surface `s` at arc length `at` along its centre line
/// (`cum` = [`cum2`] of its xy points).
fn surface_z_bank(s: &RoadSurface, cum: &[f32], at: f32) -> (f32, f32) {
    let n = s.centre.len();
    let at = at.clamp(0.0, cum[n - 1]);
    let k = cum.partition_point(|c| *c <= at).clamp(1, n - 1);
    let t = ((at - cum[k - 1]) / (cum[k] - cum[k - 1]).max(1e-6)).clamp(0.0, 1.0);
    (
        s.centre[k - 1][2] + (s.centre[k][2] - s.centre[k - 1][2]) * t,
        s.bank[k - 1] + (s.bank[k] - s.bank[k - 1]) * t,
    )
}

/// Buildings along every street, facades [`FRONT_SETBACK_M`] behind the sidewalk on the level
/// lot (playtest #52): the base reaches down to the lowest ground under the footprint (a
/// plinth on the downhill side), never above the lot at the frontage. Lots that would stand
/// next to a bridge deck (no lot there), on another street's sidewalk, in a junction's
/// turning area or at a dead end's turnaround are skipped.
fn buildings(
    rng: &mut Rng,
    built: &Built,
    nodes: &[NodeSpec],
    zones: &[Zone],
    hf: &Heightfield,
    dead_ends: &[V2],
) -> Vec<Building> {
    let grid = SurfaceGrid::new(built);
    // Junction / bend areas with the turning paths through them, plus a sidewalk.
    let node_r = node_radii(built, nodes);
    let junctions: Vec<(V2, f32)> = nodes
        .iter()
        .zip(&node_r)
        .filter(|(_, r)| **r > 0.0)
        .map(|(n, r)| (n.pos, r + BUILDING_CLEARANCE_M))
        .collect();
    let mut placed: Vec<Obb> = Vec::new();
    let mut bgrid: BTreeMap<(i32, i32), Vec<usize>> = BTreeMap::new();
    let key = |p: V2| ((p[0] / 50.0).floor() as i32, (p[1] / 50.0).floor() as i32);
    let mut out = Vec::new();
    for s in &built.surfaces {
        if s.highway {
            continue;
        }
        let pts: Vec<V2> = s.centre.iter().map(|p| geom::xy(*p)).collect();
        let cum = cum2(&pts);
        let len = cum[cum.len() - 1];
        for side in [1.0f32, -1.0] {
            let edge = if side > 0.0 { s.right_m } else { s.left_m };
            // Top of the lot behind this side's sidewalk at arc length `a`.
            let lot_z = |a: f32| {
                let (zc, bank) = surface_z_bank(s, &cum, a);
                zc + bank * side * (edge + SIDEWALK_W_M) + KERB_H_M
            };
            let mut at = 8.0;
            while at < len - 8.0 {
                let at0 = at;
                let (p, t) = point_on(&pts, &cum, at);
                let kind = zone_kind(zones, p);
                let (lot, depth, gap) = match kind {
                    DistrictKind::Core => (30.0, 24.0, 2.0),
                    DistrictKind::Commercial => (24.0, 18.0, 3.0),
                    DistrictKind::Residential => (16.0, 12.0, 5.0),
                    DistrictKind::Suburb => (15.0, 11.0, 7.0),
                    DistrictKind::Industrial | DistrictKind::Port => (42.0, 30.0, 6.0),
                };
                let n = geom::scale(geom::right_of(t), side);
                let front = edge + SIDEWALK_W_M + FRONT_SETBACK_M;
                let c = geom::add(p, geom::scale(n, front + depth * 0.5));
                let o = Obb {
                    c,
                    f: t,
                    hl: lot * 0.5 - gap * 0.5,
                    hw: depth * 0.5,
                };
                at += lot;
                let corners = obb_corners(&o);
                let probes = [corners[0], corners[1], corners[2], corners[3], c];
                if probes
                    .iter()
                    .any(|q| grid.clearance(built, *q) < BUILDING_CLEARANCE_M)
                {
                    continue;
                }
                // Keep junction turning areas (and their sidewalks) clear.
                if probes
                    .iter()
                    .any(|q| junctions.iter().any(|(c, r)| geom::dist(*q, *c) < *r))
                {
                    continue;
                }
                // Keep dead-end turnarounds clear (U-turns swing past the road end).
                if probes
                    .iter()
                    .any(|q| dead_ends.iter().any(|d| geom::dist(*q, *d) < 20.0))
                {
                    continue;
                }
                let k = key(c);
                let clash = (-1..=1).any(|dx| {
                    (-1..=1).any(|dy| {
                        bgrid
                            .get(&(k.0 + dx, k.1 + dy))
                            .is_some_and(|v| v.iter().any(|b| geom::obb_overlap(&placed[*b], &o)))
                    })
                });
                if clash {
                    continue;
                }
                let zs: Vec<f32> = probes.iter().map(|q| hf.height_at(*q)).collect();
                let zmin = zs.iter().copied().fold(f32::INFINITY, f32::min);
                let zmax = zs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                if zmax - zmin > 8.0 || zmin < 1.0 {
                    continue;
                }
                // The frontage stands on the lot: the lot top along the facade, and the ground
                // carved to it there (not a deck or its approach, where there is no lot).
                let half_len = o.hl;
                let fronts = [at0 - half_len, at0, at0 + half_len];
                let lot_min = fronts
                    .iter()
                    .map(|a| lot_z(*a))
                    .fold(f32::INFINITY, f32::min);
                let facade = |a: f32| {
                    let (q, tq) = point_on(&pts, &cum, a.clamp(0.0, len));
                    let nq = geom::scale(geom::right_of(tq), side);
                    geom::add(q, geom::scale(nq, front))
                };
                if fronts
                    .iter()
                    .any(|a| (hf.height_at(facade(*a)) - lot_z(*a)).abs() > 0.3)
                {
                    continue;
                }
                let storeys = match kind {
                    DistrictKind::Core => {
                        if rng.next_f32() < 0.18 {
                            14 + rng.below(10)
                        } else {
                            5 + rng.below(6)
                        }
                    }
                    DistrictKind::Commercial => 2 + rng.below(4),
                    DistrictKind::Residential => 1 + rng.below(3),
                    DistrictKind::Suburb => 1 + rng.below(2),
                    _ => 2 + rng.below(2),
                };
                let mut fp = corners.to_vec();
                if signed_area(&fp) < 0.0 {
                    fp.reverse();
                }
                // Down to the lowest ground under the footprint, and under the lot at the
                // frontage (the walls meet the lot; on the downhill side they continue down
                // as a plinth).
                let base = zmin.min(lot_min) - 0.3;
                out.push(Building {
                    footprint: fp,
                    height_m: storeys as f32 * 3.2 + rng.range_f32(0.0, 1.0) + (zmax - base),
                    base_z_m: base,
                });
                bgrid.entry(k).or_default().push(placed.len());
                placed.push(o);
            }
        }
    }
    out
}

fn signed_area(p: &[V2]) -> f32 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (u, v) = (p[i], p[(i + 1) % p.len()]);
        a += u[0] * v[1] - v[0] * u[1];
    }
    a * 0.5
}

/// Pose on the rightmost forward (or backward) lane of road `road`, `s` metres in (negative:
/// from the end).
fn lane_pose(built: &Built, road: usize, s: f32, fwd: bool) -> Option<(V3, f32, LaneId)> {
    let rl = &built.road_lanes[road];
    let lane = *if fwd { rl.fwd.last() } else { rl.bwd.last() }?;
    let l = &built.lanes[lane as usize];
    let cum = geom::cumulative(&l.polyline);
    let total = cum[cum.len() - 1];
    // Negative `s` counts back from the lane end.
    let s = if s >= 0.0 {
        s.min(total * 0.5)
    } else {
        (total + s).max(total * 0.5)
    };
    let (p, d) = geom::point_at(&l.polyline, &cum, s);
    Some((p, geom::yaw_of(d), lane))
}

fn nearest_lane(built: &Built, p: V2) -> Option<(LaneId, V3, f32)> {
    let mut best: Option<(LaneId, V3, f32, f32)> = None;
    for (i, l) in built.lanes.iter().enumerate() {
        if !l.kind.drivable() {
            continue;
        }
        let cum = geom::cumulative(&l.polyline);
        let pr = geom::project(&l.polyline, &cum, p);
        if best.as_ref().is_none_or(|b| pr.dist < b.3) {
            let (q, d) = geom::point_at(&l.polyline, &cum, pr.s.clamp(0.0, cum[cum.len() - 1]));
            best = Some((i as LaneId, q, geom::yaw_of(d), pr.dist));
        }
    }
    best.map(|b| (b.0, b.1, b.2))
}

// ------------------------------------------------------------------------------------------
// Entry points
// ------------------------------------------------------------------------------------------

/// Generate Port Vesper v2.
pub fn generate(cfg: &VesperConfig) -> RoadNetwork {
    generate_with_report(cfg).0
}

/// Generate Port Vesper v2 plus generator diagnostics.
pub fn generate_with_report(cfg: &VesperConfig) -> (RoadNetwork, VesperReport) {
    let mut rng = Rng::new(cfg.seed);
    let nat = Natural::new(&mut rng);
    let mut g = Gen {
        rng: Rng::new(cfg.seed ^ 0x5eed_c17e),
        nat,
        nodes: Vec::new(),
        index: BTreeMap::new(),
        drafts: Vec::new(),
        skipped: 0,
    };
    let (loop_roads, hollins, _knob, _lot, _r40w, _r40n) = g.layout();
    let crossings = plan_crossings(&mut g);
    plan_covers(&mut g);
    let opt = BuildOptions::default();
    let specs = profile_roads(&mut g, &opt);
    // Unused nodes (dropped optional roads) stay in the list without lanes; harmless.
    let built = builder::build(&g.nodes, &specs, &opt);

    // Terrain: natural ground, carved to the roads except bridge decks.
    let mut hf = Heightfield::flat(MAP_LO, MAP_HI, cfg.terrain_cell_m, 0.0);
    for j in 0..hf.ny {
        for i in 0..hf.nx {
            let p = hf.vertex_pos(i, j);
            hf.z[(j * hf.nx + i) as usize] = g.nat.z(p);
        }
    }
    let drafts = &g.drafts;
    let nat = &g.nat;
    let surfaces = &built.surfaces;
    let deck = |road: usize, k: usize| -> bool {
        let d = &drafts[road];
        let s = &surfaces[road];
        let p = s.centre[k];
        if is_elevated(s, k, &|q| nat.z(q), 3.0) {
            return true;
        }
        if d.decks.is_empty() {
            return false;
        }
        let cum = cum2(&d.path);
        let (s, _) = project2(&d.path, &cum, geom::xy(p));
        d.decks.iter().any(|(a, b)| s >= *a && s <= *b)
    };
    carve(&mut hf, &built, &g.nodes, &deck);
    let covered = covered_spans(&g, &built);
    backfill_covers(&mut hf, &g.nat, &covered);

    let zones = zones();
    let mut degree = vec![0usize; g.nodes.len()];
    for d in &g.drafts {
        degree[d.a] += 1;
        degree[d.b] += 1;
    }
    let dead_ends: Vec<V2> = (0..g.nodes.len())
        .filter(|n| degree[*n] == 1)
        .map(|n| g.nodes[n].pos)
        .collect();
    let mut buildings = buildings(&mut g.rng, &built, &g.nodes, &zones, &hf, &dead_ends);
    let cover_shapes: Vec<CoverShape> = covered.iter().map(CoverShape::new).collect();
    buildings.retain(|b| !in_cover_corridor(&cover_shapes, &b.footprint));

    // POIs. The first race grid is the city-chase / free-drive spawn.
    let mut pois = Vec::new();
    let longest_loop = *loop_roads
        .iter()
        .max_by(|a, b| {
            let la = cum2(&g.drafts[**a].path);
            let lb = cum2(&g.drafts[**b].path);
            la[la.len() - 1].total_cmp(&lb[lb.len() - 1]).then(b.cmp(a))
        })
        .expect("loop has roads");
    let blvd = g
        .drafts
        .iter()
        .position(|d| d.name == "Harbor Boulevard")
        .unwrap_or(0);
    let bluff = g
        .drafts
        .iter()
        .position(|d| d.name == "Bluff Road")
        .unwrap_or(0);
    let named = |n: &str| g.drafts.iter().position(|d| d.name == n).unwrap_or(0);
    let (ridge, spur) = (named("Ridge Road"), named("Knob Observatory Lot"));
    // The loop road over Hollins Heights' northern shoulder (the highway's big crest).
    let loop_north = *loop_roads
        .iter()
        .find(|r| {
            let p = &g.drafts[**r].path;
            project2(p, &cum2(p), [-1000.0, 1390.0]).1 < 40.0
        })
        .unwrap_or(&longest_loop);
    // Lantern Way's western approach into downtown (from Larkspur), heading east.
    let lantern = (0..g.drafts.len())
        .find(|k| {
            let d = &g.drafts[*k];
            d.name == "Lantern Way" && geom::dist(g.nodes[d.b].pos, [-300.0, 150.0]) < 1.0
        })
        .unwrap_or(0);
    for (kind, name, road, s, fwd) in [
        (PoiKind::RaceStartGrid, "Lantern Run", lantern, 40.0, true),
        (
            PoiKind::RaceStartGrid,
            "Loop 9 sprint",
            longest_loop,
            80.0,
            true,
        ),
        (
            PoiKind::RaceStartGrid,
            "Hollins Ascent",
            hollins,
            12.0,
            true,
        ),
        (
            PoiKind::RaceStartGrid,
            "Hollins Descent",
            hollins,
            12.0,
            false,
        ),
        (PoiKind::RaceStartGrid, "Ridge Road", ridge, 12.0, true),
        (
            PoiKind::RaceStartGrid,
            "Harbor Boulevard drag",
            blvd,
            20.0,
            true,
        ),
        (PoiKind::RaceStartGrid, "Bluff Road", bluff, 20.0, true),
        (
            PoiKind::RaceStartGrid,
            "Loop 9 north",
            loop_north,
            40.0,
            true,
        ),
        (
            PoiKind::RaceStartGrid,
            "Loop 9 west",
            loop_north,
            -80.0,
            true,
        ),
        (
            PoiKind::SafehouseCandidate,
            "Knob Observatory Lot",
            spur,
            -8.0,
            true,
        ),
    ] {
        if let Some((p, yaw, lane)) = lane_pose(&built, road, s, fwd) {
            pois.push(Poi {
                kind,
                name: name.into(),
                pos: p,
                heading_rad: yaw,
                lane: Some(lane),
            });
        }
    }
    // Route ends either side of each covered road (drive-through and pursuit scenarios):
    // "<name> entry" sits before the first portal in the forward direction, "<name> exit"
    // after the second.
    let tunnel_ends: [(&str, f32, f32); 2] = [
        ("Hollins Tunnel", 950.0, -1100.0),
        ("Founders Cut", 40.0, -40.0),
    ];
    for (name, s0, s1) in tunnel_ends {
        let Some(c) = covered.iter().find(|c| c.name == name) else {
            continue;
        };
        // "<name> lay-by": the forward kerb lane in the middle of the covered stretch (a
        // place to hide from the helicopter).
        let mid = {
            let sh = CoverShape::new(c);
            geom::xy(geom::point_at(&c.centre, &sh.cum, sh.length() * 0.5).0)
        };
        let lay_by = built.road_lanes[c.road as usize].fwd.last().map(|l| {
            let pl = &built.lanes[*l as usize].polyline;
            geom::project(pl, &geom::cumulative(pl), mid).s
        });
        for (suffix, s) in [
            ("entry", s0),
            ("lay-by", lay_by.unwrap_or(s0)),
            ("exit", s1),
        ] {
            if let Some((p, yaw, lane)) = lane_pose(&built, c.road as usize, s, true) {
                pois.push(Poi {
                    kind: PoiKind::RaceStartGrid,
                    name: format!("{name} {suffix}"),
                    pos: p,
                    heading_rad: yaw,
                    lane: Some(lane),
                });
            }
        }
    }
    let mut block_poi = |kind: PoiKind, name: &str, p: V2| {
        if let Some((lane, q, yaw)) = nearest_lane(&built, p) {
            pois.push(Poi {
                kind,
                name: name.into(),
                pos: q,
                heading_rad: yaw,
                lane: Some(lane),
            });
        }
    };
    block_poi(PoiKind::PoliceStation, "Central Precinct", [-150.0, 100.0]);
    block_poi(
        PoiKind::Helipad,
        "Central Precinct helipad",
        [-130.0, 120.0],
    );
    block_poi(PoiKind::PoliceStation, "North Precinct", [100.0, 640.0]);
    block_poi(PoiKind::PoliceStation, "Harbor Precinct", [800.0, -400.0]);
    block_poi(PoiKind::Garage, "Meridian Motorworks", [150.0, -100.0]);
    block_poi(PoiKind::Garage, "Sal's Paint & Panel", [-900.0, 0.0]);
    block_poi(PoiKind::Garage, "Gantry Garage", [700.0, -870.0]);
    block_poi(
        PoiKind::SafehouseCandidate,
        "Mercer Wash-n-Fold Lot",
        [-250.0, 650.0],
    );
    block_poi(
        PoiKind::SafehouseCandidate,
        "Cliffside Carport",
        [1250.0, 900.0],
    );
    block_poi(
        PoiKind::SafehouseCandidate,
        "The Coldroom",
        [1000.0, -800.0],
    );

    let mut net = built.into_network(
        CITY_ID,
        &format!("procedural port_vesper v2 seed={}", cfg.seed),
        None,
        [0.0, -50.0],
        zones,
        pois,
        buildings,
    );
    net.bounds_min = [
        net.bounds_min[0].min(MAP_LO[0]),
        net.bounds_min[1].min(MAP_LO[1]),
    ];
    net.bounds_max = [
        net.bounds_max[0].max(MAP_HI[0]),
        net.bounds_max[1].max(MAP_HI[1]),
    ];
    net.terrain = Some(hf);
    net.covered = covered;
    let report = VesperReport {
        crossings,
        skipped_roads: g.skipped,
        hollins_drive: hollins,
        loop_roads,
        road_names: g.drafts.iter().map(|d| d.name).collect(),
        road_speeds: g.drafts.iter().map(|d| d.st.speed).collect(),
        road_grades: g.drafts.iter().map(|d| d.st.grade).collect(),
    };
    (net, report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fillet_rounds_a_square() {
        let sq = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]];
        let p = fillet(&sq, &[20.0; 4], true, 2.0);
        // Every point stays inside the square and corners are cut.
        for q in &p {
            assert!(q[0] >= -0.01 && q[0] <= 100.01 && q[1] >= -0.01 && q[1] <= 100.01);
            assert!(geom::dist(*q, [100.0, 100.0]) > 5.0);
        }
    }

    #[test]
    fn profile_respects_grade_anchors_and_curvature() {
        // A 400 m road whose ground has a 30 m wall in the middle.
        let ds = 4.0;
        let desired: Vec<f32> = (0..101)
            .map(|i| if (40..60).contains(&i) { 30.0 } else { 0.0 })
            .collect();
        let anchors = [(0, 3, 0.0), (97, 100, 0.0)];
        let z = profile(&desired, ds, &anchors, &[], (2, 2), 0.1, 0.02);
        for w in z.windows(2) {
            assert!(((w[1] - w[0]) / ds).abs() <= 0.1 + 1e-4);
        }
        assert_eq!(z[0], 0.0);
        assert_eq!(z[100], 0.0);
        for i in 1..100 {
            let k = ((z[i + 1] - 2.0 * z[i] + z[i - 1]) / (ds * ds)).abs();
            assert!(k <= 0.03, "curvature {k} at {i}");
        }
        // The road cuts through the wall but still rises over it.
        assert!(z[50] > 3.0 && z[50] < 30.0, "{}", z[50]);
    }

    fn city() -> &'static (RoadNetwork, VesperReport) {
        static CITY: std::sync::OnceLock<(RoadNetwork, VesperReport)> = std::sync::OnceLock::new();
        CITY.get_or_init(|| generate_with_report(&VesperConfig::default()))
    }

    #[test]
    fn city_shape_and_wiring() {
        let (net, rep) = city();
        net.validate().unwrap();
        assert_eq!(net.id, CITY_ID);
        assert!(net.lanes.len() > 800, "lanes {}", net.lanes.len());
        assert!(net.intersections.len() > 100);
        assert!(net.signals.len() > 20);
        assert!(
            net.buildings.len() > 2000,
            "buildings {}",
            net.buildings.len()
        );
        assert_eq!(
            rep.skipped_roads, 0,
            "a planned road crossed another at grade"
        );
        for k in [
            LaneKind::Highway,
            LaneKind::Parking,
            LaneKind::Through,
            LaneKind::TurnLeft,
        ] {
            assert!(net.lanes.iter().any(|l| l.kind == k), "no {k:?} lanes");
        }
        for k in [
            PoiKind::PoliceStation,
            PoiKind::Helipad,
            PoiKind::Garage,
            PoiKind::SafehouseCandidate,
            PoiKind::RaceStartGrid,
        ] {
            assert!(net.pois.iter().any(|p| p.kind == k), "no {k:?}");
        }
        // The free-drive / city-chase spawn: heading into downtown on Lantern Way.
        assert_eq!(net.pois[0].name, "Lantern Run");
        for (i, l) in net.lanes.iter().enumerate() {
            if l.kind.drivable() {
                assert!(!l.connections.is_empty(), "lane {i} is a trap");
            }
            let len = *geom::cumulative(&l.polyline).last().unwrap();
            assert!(len > 8.0, "lane {i} only {len} m");
        }
        // Every district kind is present.
        for k in DistrictKind::ALL {
            assert!(net.zones.iter().any(|z| z.kind == k), "no {k:?} zone");
        }
    }

    #[test]
    fn deterministic_and_seed_sensitive() {
        let a = generate(&VesperConfig::default());
        assert_eq!(&a, &city().0);
        let b = generate(&VesperConfig {
            seed: 2,
            ..Default::default()
        });
        assert_ne!(a.terrain, b.terrain);
        b.validate().unwrap();
    }

    #[test]
    fn grades_crests_and_banks_within_design() {
        let (net, rep) = city();
        for s in &net.surfaces {
            let r = s.road as usize;
            let cum = geom::cumulative(&s.centre);
            let v = rep.road_speeds[r];
            for k in 0..s.centre.len() - 1 {
                let ds = (cum[k + 1] - cum[k]).max(0.5);
                let g = (s.centre[k + 1][2] - s.centre[k][2]) / ds;
                assert!(
                    g.abs() <= rep.road_grades[r] + 0.005,
                    "{} grade {:.3} > cap {}",
                    rep.road_names[r],
                    g,
                    rep.road_grades[r]
                );
                assert!(s.bank[k].abs() <= 0.0601);
                if k > 0 {
                    let ds0 = (cum[k] - cum[k - 1]).max(0.5);
                    let g0 = (s.centre[k][2] - s.centre[k - 1][2]) / ds0;
                    let kv = (g - g0) / (0.5 * (ds + ds0));
                    // Over a crest at the limit a car keeps well over half its weight.
                    assert!(
                        -kv * v * v < 0.35 * 9.81,
                        "{} crest {:.2} g at {:?}",
                        rep.road_names[r],
                        -kv * v * v / 9.81,
                        s.centre[k]
                    );
                }
            }
        }
        // The hill road climbs most of the hill.
        let h = &net.surfaces[rep.hollins_drive];
        let rise = h.centre[h.centre.len() - 1][2] - h.centre[0][2];
        assert!(rise > 60.0, "Hollins Drive rises only {rise} m");
    }

    #[test]
    fn junctions_are_flat_and_lanes_meet_them() {
        let (net, _) = city();
        for ix in &net.intersections {
            let c = net.nodes[ix.node as usize].pos;
            for inc in &ix.incoming {
                let l = &net.lanes[inc.lane as usize];
                let end = l.polyline[l.polyline.len() - 1];
                assert!(
                    (end[2] - c[2]).abs() < 0.3,
                    "approach {} m off the box",
                    end[2] - c[2]
                );
            }
        }
    }

    #[test]
    fn grade_separations_have_clearance() {
        let (net, rep) = city();
        assert_eq!(rep.crossings.len(), 2, "{:?}", rep.crossings);
        assert!(rep.crossings.iter().any(|c| c.underpass));
        assert!(rep.crossings.iter().any(|c| !c.underpass));
        for c in &rep.crossings {
            let z_of = |road: usize| {
                let s = &net.surfaces[road];
                let pts: Vec<V2> = s.centre.iter().map(|p| geom::xy(*p)).collect();
                let cum = cum2(&pts);
                let (at, d) = project2(&pts, &cum, c.pos);
                assert!(d < 1.0);
                let f = geom::cumulative(&s.centre);
                geom::point_at(&s.centre, &f, at).0[2]
            };
            let gap = z_of(c.upper) - z_of(c.lower);
            assert!(gap > 6.5 && gap < 9.0, "clearance {gap} m at {:?}", c.pos);
        }
    }

    #[test]
    fn terrain_stays_under_roads_and_buildings_sit_on_it() {
        let (net, _) = city();
        let t = net.terrain.as_ref().unwrap();
        let mut checked = 0;
        for s in &net.surfaces {
            for k in 0..s.centre.len() {
                let p = s.centre[k];
                if is_elevated(s, k, &|q| t.height_at(q), 1.0) {
                    continue; // bridge deck
                }
                let tan = if k + 1 < s.centre.len() {
                    geom::normalize(geom::sub(geom::xy(s.centre[k + 1]), geom::xy(p)))
                } else {
                    geom::normalize(geom::sub(geom::xy(p), geom::xy(s.centre[k - 1])))
                };
                let r = geom::right_of(tan);
                for lat in [-s.left_m, -0.5 * s.left_m, 0.0, 0.5 * s.right_m, s.right_m] {
                    let q = geom::add(geom::xy(p), geom::scale(r, lat));
                    let road = s.z_at(k, lat);
                    let g = t.height_at(q);
                    assert!(g <= road + 0.03, "terrain {g} above road {road} at {q:?}");
                    checked += 1;
                }
            }
        }
        assert!(checked > 10_000);
        for b in &net.buildings {
            for q in &b.footprint {
                assert!(b.base_z_m <= t.height_at(*q) + 0.01);
            }
            assert!(b.roof_z() > b.base_z_m + 3.0);
        }
    }
    #[test]
    fn covered_roads_block_sight_in_and_out() {
        let (net, _) = city();
        let names: Vec<&str> = net.covered.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Hollins Tunnel", "Founders Cut"]);
        let mut map = crate::CityMap::new(net.clone());
        let eye = |p: V3| [p[0], p[1], p[2] + 1.3];
        for (i, c) in net.covered.iter().enumerate() {
            let sh = CoverShape::new(c);
            let len = sh.length();
            assert!(len > 200.0, "{} only {len} m", c.name);
            let at = |s: f32| eye(geom::point_at(&c.centre, &sh.cum, s).0);
            let (a, b) = (at(30.0), at(len * 0.5));
            assert_eq!(map.covered_at(a), Some(i));
            // Inside the same tunnel: clear. From the road 50 m before the portal: blocked.
            assert!(map.line_of_sight(a, b), "{}: inside", c.name);
            let (p0, d0) = geom::point_at(&c.centre, &sh.cum, 0.0);
            let out = eye([p0[0] - d0[0] * 50.0, p0[1] - d0[1] * 50.0, p0[2]]);
            assert_eq!(map.covered_at(out), None);
            assert!(!map.line_of_sight(out, a), "{}: portal", c.name);
            assert!(!map.line_of_sight(a, out), "{}: portal back", c.name);
            // The helicopter overhead loses the car inside, unless cover is switched off.
            let heli = [b[0], b[1], b[2] + 120.0];
            assert!(!map.line_of_sight(heli, b), "{}: helicopter", c.name);
            map.covered_los = false;
            assert!(map.line_of_sight(heli, b));
            map.covered_los = true;
            // The deck (or backfill) is above the ceiling and terrain never enters the tube.
            let t = net.terrain.as_ref().unwrap();
            for k in 0..c.centre.len() {
                let p = c.centre[k];
                assert!(
                    t.height_at(geom::xy(p)) <= p[2] + 0.05,
                    "{}: ground in the tube",
                    c.name
                );
            }
        }
        // No building stands on a deck or in a trench.
        let shapes: Vec<CoverShape> = net.covered.iter().map(CoverShape::new).collect();
        assert!(net
            .buildings
            .iter()
            .all(|b| !in_cover_corridor(&shapes, &b.footprint)));
    }
}
