//! Touge mountain passes (`touge_*`): single-road hill climbs for boss and challenge races,
//! cut into a mountainside. Coordinates in metres, +X east, +Y north, +Z up.
//!
//! Each pass is described by a small JSON spec in `data/touge/<id>.json` (embedded at build
//! time, see [`BUILTIN_SPECS`]): a turtle path of straights and arcs from the base, the total
//! climb, lane and shoulder widths, how deep the mountain falls away beside the road, the
//! stretches where the low side is an open dirt drop with no rail, and how wooded the slopes
//! are. [`generate`] turns a spec into:
//!
//! - one two-lane road (one lane each way) from the base node to the summit node, with a
//!   vertical profile that climbs `climb_m` (hairpins flatter than the straights) and banking
//!   from the curvature;
//! - a mountain heightfield around it (the road heights spread out, falling away from the
//!   road, with several octaves of noise for ridges and gullies), on a fine grid that the
//!   renderer draws as it is ([`crate::surface::drawn_spacing`]);
//! - a roadside edge on each side of every metre of road ([`EdgeKind`]): the uphill side is a
//!   cut bank (a narrow verge, then the slope rising to the mountain); the downhill side is a
//!   steel guard rail ([`PropKind::GuardRail`], light, breaks away from a square hit) on a
//!   verge above a fill slope, an open dirt drop where the spec asks for one, or a gravel
//!   runoff apron (on the outside of hairpins and tight bends) with a rail at its back;
//! - pine forest on the slopes ([`DecorKind::PineTree`]) and dirt zones on the runoff aprons
//!   and drops (`WorldQuery::surface_at` reads them as dirt);
//! - fixed walls ([`DecorKind::Fence`] boxes, solid in `wp_sim`) across both ends of the road;
//! - four race-grid POIs: `<name> base start` / `<name> summit finish` (uphill) and
//!   `<name> summit start` / `<name> base finish` (downhill).
//!
//! The race rules (a car that falls off the mountain or leaves the road's corridor is totaled
//! and the battle is over) live in `wp_race::touge`; this module only builds the place.
//!
//! Everything is plain `f32` arithmetic plus `wp_math`'s deterministic trig and RNG, so the
//! same spec gives bit-identical worlds on native and wasm.

use serde::{Deserialize, Serialize};
use wp_math::Rng;

use crate::builder::{self, BuildOptions, NodeSpec, RoadClass, RoadSpec};
use crate::geom::{self, V2, V3};
use crate::network::*;
use crate::props::{PropKind, StaticProp};
use crate::terrain::{smoothstep, Heightfield};

/// The four built-in passes, in menu order.
pub const TOUGE_IDS: [&str; 4] = [
    "touge_hollow_crown",
    "touge_mistral_ridge",
    "touge_ashfall_grade",
    "touge_cedar_switchback",
];

/// Embedded specs, `(id, json)`, in [`TOUGE_IDS`] order.
pub const BUILTIN_SPECS: [(&str, &str); 4] = [
    (
        "touge_hollow_crown",
        include_str!("../../../data/touge/touge_hollow_crown.json"),
    ),
    (
        "touge_mistral_ridge",
        include_str!("../../../data/touge/touge_mistral_ridge.json"),
    ),
    (
        "touge_ashfall_grade",
        include_str!("../../../data/touge/touge_ashfall_grade.json"),
    ),
    (
        "touge_cedar_switchback",
        include_str!("../../../data/touge/touge_cedar_switchback.json"),
    ),
];

/// Centreline sample spacing, m.
pub const STEP_M: f32 = 2.0;
/// End wall: width, m.
pub const WALL_W_M: f32 = 0.5;
/// End wall: height above the road, m.
pub const WALL_H_M: f32 = 1.2;
/// Gap between the paved edge and the end walls, m.
pub const WALL_GAP_M: f32 = 0.05;
/// Guard rail section: length (one W-beam panel over two posts), m.
pub const RAIL_LEN_M: f32 = 4.0;
/// Guard rail section: depth from the beam's face to the back of the posts, m (the collider
/// is a box this deep).
pub const RAIL_W_M: f32 = 0.4;
/// Guard rail section: height of the posts above the verge, m (the beam's top is at ~0.72 m).
pub const RAIL_H_M: f32 = 0.8;
/// Guard rail section mass, kg: the steel beam and its posts once they have snapped. Light
/// next to a car, so a car that breaks through carries it along instead of stopping.
pub const RAIL_MASS_KG: f32 = 120.0;
/// Gap between the paved edge and the guard rail's face, m.
pub const RAIL_GAP_M: f32 = 0.3;
/// Level verge behind a guard rail before the fill slope drops away, m from the paved edge.
pub const RAIL_VERGE_M: f32 = 2.5;
/// Gravel runoff apron on the outside of tight bends, m from the paved edge.
pub const RUNOFF_W_M: f32 = 8.0;
/// Dirt verge of an open drop before the ground falls away, m from the paved edge.
pub const DROP_VERGE_M: f32 = 1.0;
/// Fill slope below a rail or a runoff apron, m of fall per m out.
pub const FILL_SLOPE: f32 = 0.9;
/// Slope of an open dirt drop, m of fall per m out.
pub const DROP_SLOPE: f32 = 1.4;
/// Cut bank: gravel verge at the foot of the bank, m from the paved edge.
pub const BANK_VERGE_M: f32 = 1.0;
/// Cut bank: rise per m out.
pub const BANK_SLOPE: f32 = 1.0;
/// Cut bank: least height of the bank above the road edge (an earth berm where the
/// mountain itself is lower), m.
pub const BERM_H_M: f32 = 1.5;
/// Steepest rock cut between two stacked legs of the road (hairpin stacks climb up to ~1.3
/// m per m between legs), m per m.
pub const LEG_CUT_SLOPE: f32 = 2.5;
/// Amplitude of the mountain's noise (ridges, gullies) away from the road, m.
pub const RELIEF_M: f32 = 12.0;
/// A car more than this far beyond the paved edge has left the course (past every runoff
/// apron and verge), m. `wp_race::touge` judges with it.
pub const COURSE_EXTENT_M: f32 = RUNOFF_W_M + 12.0;
/// Terrain grid spacing, m (drawn and collided at this resolution).
pub const TERRAIN_CELL_M: f32 = 3.0;
/// Terrain sits this far below the road surface under the paving, m.
pub const CARVE_BELOW_M: f32 = 0.12;
/// ... and this much further within a cell of the paved edge, so the ground just off the
/// edge can rise into a bank without a terrain triangle poking up through the paving, m.
pub const EDGE_CARVE_M: f32 = 0.3;
/// Ground margin around the road's bounding box, m.
pub const MARGIN_M: f32 = 100.0;
/// Start grids sit this far in from each end of the road, m.
pub const GRID_IN_M: f32 = 60.0;
/// Finish lines sit this far in from each end of the road, m.
pub const FINISH_IN_M: f32 = 40.0;
/// A spec drop on the outside of a bend tighter than this (radius, m) keeps its guard rail
/// there.
pub const DROP_RAIL_BEND_R_M: f32 = 200.0;
/// ... from this far before the bend to this far after it, m.
pub const DROP_RAIL_MARGIN_M: f32 = 25.0;
/// Bends this tight (radius, m) or this sharp (turn, degrees) get a runoff apron outside.
pub const RUNOFF_MAX_R_M: f32 = 30.0;
/// See [`RUNOFF_MAX_R_M`].
pub const RUNOFF_MIN_TURN_DEG: f32 = 60.0;
/// Most pine trees on one pass.
pub const MAX_TREES: usize = 8000;

/// One piece of the turtle path.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Leg {
    /// Straight ahead for `straight` metres.
    Straight {
        /// Length, m.
        straight: f32,
    },
    /// An arc turning `turn` degrees (positive = left) on radius `r`.
    Arc {
        /// Turn angle, degrees (+ = left / counter-clockwise).
        turn: f32,
        /// Radius of the centreline, m.
        r: f32,
    },
}

fn default_forest() -> f32 {
    0.5
}

/// A pass as authored in `data/touge/<id>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TougeSpec {
    /// Id (= file stem, `touge_*`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// One-line description for the menu.
    pub blurb: String,
    /// Height of the summit end above the base end, m.
    pub climb_m: f32,
    /// Lane width, m (one lane each way).
    pub lane_width_m: f32,
    /// Paved shoulder outside each lane, m.
    pub shoulder_m: f32,
    /// Posted speed, m/s (traffic and AI reference only; racers ignore it).
    pub speed_mps: f32,
    /// How far the mountain falls below the road beside every downhill edge (at least), m.
    pub cliff_drop_m: f32,
    /// Open drops `[from_m, to_m]` along the road from the base: no guard rail on the
    /// downhill side, a dirt verge and a steep fall. The side (the lower ground) is picked by
    /// the generator.
    #[serde(default)]
    pub drops: Vec<[f32; 2]>,
    /// How wooded the slopes are, 0 (bare rock and scrub) to 1 (dense forest).
    #[serde(default = "default_forest")]
    pub forest: f32,
    /// Turtle path from the base, starting northwards.
    pub legs: Vec<Leg>,
}

impl TougeSpec {
    /// Parse a spec from JSON.
    pub fn from_json(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }
}

/// The spec of built-in pass `id`, or `None`.
pub fn spec(id: &str) -> Option<TougeSpec> {
    BUILTIN_SPECS
        .iter()
        .find(|(k, _)| *k == id)
        .map(|(_, j)| TougeSpec::from_json(j).expect("embedded touge spec parses"))
}

/// Whether `id` names a built-in pass.
pub fn is_touge(id: &str) -> bool {
    TOUGE_IDS.contains(&id)
}

/// What lies beside the paved edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeKind {
    /// Uphill: a gravel verge, then the cut slope rising to the mountain. No barrier.
    Bank,
    /// Downhill: a breakaway steel guard rail on a level verge, the fill slope below it.
    Rail,
    /// Outside of a tight bend: a gravel / dirt runoff apron [`RUNOFF_W_M`] wide, then a guard
    /// rail (downhill) or the cut bank (uphill).
    Runoff,
    /// Downhill with no rail: a dirt verge and a steep drop.
    Drop,
}

/// A stretch of one edge as built.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EdgeSpan {
    /// Start, m along the road from the base.
    pub from_m: f32,
    /// End, m along the road from the base.
    pub to_m: f32,
    /// Side: `1.0` = right of the base → summit direction, `-1.0` = left.
    pub side: f32,
    /// What is there.
    pub kind: EdgeKind,
}

/// What the generator produced besides the network: the track's own description.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TougeTrack {
    /// Pass id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Road length base → summit, m.
    pub length_m: f32,
    /// Summit height above the base, m.
    pub climb_m: f32,
    /// Steepest grade along the road (rise over run, 0.1 = 10 %).
    pub max_grade: f32,
    /// Paved half width (centreline to edge), m.
    pub half_width_m: f32,
    /// Both edges, end to end, as runs of one kind (ordered by side, then distance).
    pub edges: Vec<EdgeSpan>,
    /// Number of guard rail sections.
    pub rail_sections: usize,
    /// Number of pine trees.
    pub trees: usize,
}

impl TougeTrack {
    /// The edge runs of `kind`.
    pub fn spans(&self, kind: EdgeKind) -> impl Iterator<Item = &EdgeSpan> + '_ {
        self.edges.iter().filter(move |e| e.kind == kind)
    }

    /// Metres of edge (both sides summed) of `kind`.
    pub fn edge_m(&self, kind: EdgeKind) -> f32 {
        self.spans(kind).map(|e| e.to_m - e.from_m).sum()
    }
}

/// Everything [`generate`] builds.
#[derive(Debug, Clone, PartialEq)]
pub struct TougeBuild {
    /// The road network (one road, terrain, end walls, trees, dirt zones, four POIs).
    pub net: RoadNetwork,
    /// Guard rail props.
    pub rails: Vec<StaticProp>,
    /// Track summary.
    pub track: TougeTrack,
}

/// Turtle path sampled every [`STEP_M`]: points and the signed curvature (1/m, + = left) of
/// the leg each sample belongs to.
fn turtle(legs: &[Leg]) -> (Vec<V2>, Vec<f32>) {
    let mut p: V2 = [0.0, 0.0];
    let mut d: V2 = [0.0, 1.0];
    let mut pts = vec![p];
    let mut kap = vec![0.0];
    let rotate = |d: V2, a: f32| -> V2 {
        let (s, c) = wp_math::sin_cos(a);
        [d[0] * c - d[1] * s, d[0] * s + d[1] * c]
    };
    for leg in legs {
        match *leg {
            Leg::Straight { straight } => {
                let n = (straight / STEP_M).round().max(1.0) as usize;
                let ds = straight / n as f32;
                for _ in 0..n {
                    p = geom::add(p, geom::scale(d, ds));
                    pts.push(p);
                    kap.push(0.0);
                }
            }
            Leg::Arc { turn, r } => {
                let a = turn.to_radians();
                let n = (a.abs() * r / STEP_M).round().max(1.0) as usize;
                let da = a / n as f32;
                let (half_sin, _) = wp_math::sin_cos(da.abs() * 0.5);
                let chord = 2.0 * r * half_sin;
                for _ in 0..n {
                    d = rotate(d, da * 0.5);
                    p = geom::add(p, geom::scale(d, chord));
                    d = geom::normalize(rotate(d, da * 0.5));
                    pts.push(p);
                    kap.push(a.signum() / r);
                }
            }
        }
    }
    (pts, kap)
}

/// Moving average over `half` samples each side, `passes` times; the first and last `keep`
/// samples are left alone.
fn smooth(v: &[f32], half: usize, passes: usize, keep: usize) -> Vec<f32> {
    let n = v.len();
    let mut cur = v.to_vec();
    for _ in 0..passes {
        let prev = cur.clone();
        for (i, out) in cur
            .iter_mut()
            .enumerate()
            .take(n.saturating_sub(keep))
            .skip(keep)
        {
            let lo = i.saturating_sub(half);
            let hi = (i + half).min(n - 1);
            let sum: f32 = prev[lo..=hi].iter().sum();
            *out = sum / (hi - lo + 1) as f32;
        }
    }
    cur
}

/// Vertical profile: `climb` spread by arc length, tight arcs and both ends flatter.
fn profile(kap: &[f32], climb: f32) -> Vec<f32> {
    let n = kap.len();
    let len = (n - 1) as f32 * STEP_M;
    let weight = |i: usize| -> f32 {
        let s = i as f32 * STEP_M;
        if s < GRID_IN_M + 10.0 || s > len - GRID_IN_M - 10.0 {
            return 0.15;
        }
        let r = if kap[i] == 0.0 {
            f32::INFINITY
        } else {
            1.0 / kap[i].abs()
        };
        if r < 25.0 {
            0.45
        } else if r < 60.0 {
            0.8
        } else {
            1.0
        }
    };
    let w: Vec<f32> = (0..n).map(weight).collect();
    let w = smooth(&w, 8, 3, 0);
    let mut cum = vec![0.0f32; n];
    for i in 1..n {
        cum[i] = cum[i - 1] + 0.5 * (w[i - 1] + w[i]);
    }
    let total = cum[n - 1].max(1e-6);
    cum.iter().map(|c| climb * c / total).collect()
}

/// Banking from curvature: the outside of each bend higher, capped at 6 %, faded to 0 near
/// both ends.
fn banking(kap: &[f32]) -> Vec<f32> {
    let n = kap.len();
    let raw: Vec<f32> = kap.iter().map(|k| (k * 1.6).clamp(-0.06, 0.06)).collect();
    let mut b = smooth(&raw, 4, 3, 0);
    for (i, v) in b.iter_mut().enumerate() {
        let s = (i as f32 * STEP_M).min((n - 1 - i) as f32 * STEP_M);
        *v *= smoothstep(20.0, 45.0, s);
    }
    b
}

/// Spatial buckets of polyline segments (for nearest-point queries).
struct SegGrid {
    lo: V2,
    cell: f32,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<u32>>,
}

impl SegGrid {
    fn new(pts: &[V3], lo: V2, hi: V2, cell: f32) -> Self {
        let nx = ((hi[0] - lo[0]) / cell).ceil() as usize + 1;
        let ny = ((hi[1] - lo[1]) / cell).ceil() as usize + 1;
        let mut cells = vec![Vec::new(); nx * ny];
        for k in 0..pts.len() - 1 {
            let (a, b) = (geom::xy(pts[k]), geom::xy(pts[k + 1]));
            let i0 = ((a[0].min(b[0]) - lo[0]) / cell).floor().max(0.0) as usize;
            let i1 = (((a[0].max(b[0]) - lo[0]) / cell).floor() as usize).min(nx - 1);
            let j0 = ((a[1].min(b[1]) - lo[1]) / cell).floor().max(0.0) as usize;
            let j1 = (((a[1].max(b[1]) - lo[1]) / cell).floor() as usize).min(ny - 1);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    cells[j * nx + i].push(k as u32);
                }
            }
        }
        Self {
            lo,
            cell,
            nx,
            ny,
            cells,
        }
    }

    /// Segment indices in the cells within `r` of `p` (may repeat).
    fn near(&self, p: V2, r: f32, out: &mut Vec<u32>) {
        out.clear();
        let i0 = ((p[0] - r - self.lo[0]) / self.cell).floor().max(0.0) as usize;
        let j0 = ((p[1] - r - self.lo[1]) / self.cell).floor().max(0.0) as usize;
        let i1 = (((p[0] + r - self.lo[0]) / self.cell).floor().max(0.0) as usize).min(self.nx - 1);
        let j1 = (((p[1] + r - self.lo[1]) / self.cell).floor().max(0.0) as usize).min(self.ny - 1);
        for j in j0..=j1 {
            for i in i0..=i1 {
                out.extend_from_slice(&self.cells[j * self.nx + i]);
            }
        }
    }
}

/// Nearest point on the centreline among segments `segs`: `(s, lateral, dist, k, t)` with
/// lateral positive to the right of base → summit.
fn nearest(pts: &[V3], cum: &[f32], segs: &[u32], p: V2) -> Option<(f32, f32, f32, usize, f32)> {
    let mut best: Option<(f32, f32, f32, usize, f32)> = None;
    for &k in segs {
        let k = k as usize;
        let (a, b) = (geom::xy(pts[k]), geom::xy(pts[k + 1]));
        let ab = geom::sub(b, a);
        let l2 = geom::dot(ab, ab).max(1e-9);
        let t = (geom::dot(geom::sub(p, a), ab) / l2).clamp(0.0, 1.0);
        let q = geom::add(a, geom::scale(ab, t));
        let dist = geom::dist(p, q);
        if best.is_none_or(|b| dist < b.2) {
            let lat = -geom::cross(geom::normalize(ab), geom::sub(p, a));
            let s = cum[k] + t * (cum[k + 1] - cum[k]);
            best = Some((s, lat, dist, k, t));
        }
    }
    best
}

/// The mountain's mean slope: the least-squares plane through the road samples, as
/// `(centre, height at the centre, gradient)`.
fn fit_plane(samples: &[(V2, f32)]) -> (V2, f32, V2) {
    let n = samples.len().max(1) as f32;
    let c = samples
        .iter()
        .fold([0.0f32; 2], |a, (q, _)| [a[0] + q[0] / n, a[1] + q[1] / n]);
    let zc = samples.iter().map(|s| s.1).sum::<f32>() / n;
    let (mut sxx, mut sxy, mut syy, mut sxz, mut syz) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for (q, z) in samples {
        let (x, y, dz) = (q[0] - c[0], q[1] - c[1], z - zc);
        sxx += x * x;
        sxy += x * y;
        syy += y * y;
        sxz += x * dz;
        syz += y * dz;
    }
    let det = sxx * syy - sxy * sxy;
    if det.abs() < 1e-3 {
        return (c, zc, [0.0, 0.0]);
    }
    let gx = (sxz * syy - syz * sxy) / det;
    let gy = (syz * sxx - sxz * sxy) / det;
    (c, zc, [gx, gy])
}

/// Natural mountain ground at `p`: the mean slope ([`fit_plane`]), lifted or lowered by the
/// road heights near `p` (their offsets from the plane, weighted by distance, fading out
/// beyond ~80 m from the road), clamped to 150 m beyond the road's range.
fn natural_at(p: V2, samples: &[(V2, f32)], plane: (V2, f32, V2), range: (f32, f32)) -> f32 {
    let (c, zc, g) = plane;
    let flat = |q: V2| zc + g[0] * (q[0] - c[0]) + g[1] * (q[1] - c[1]);
    let (mut sw, mut sr, mut dmin) = (0.0f32, 0.0f32, f32::INFINITY);
    for &(q, z) in samples {
        let d = geom::dist(p, q);
        dmin = dmin.min(d);
        let u = d / 30.0;
        let w = 1.0 / ((1.0 + u * u) * (1.0 + u * u));
        sw += w;
        sr += w * (z - flat(q));
    }
    let lift = if sw > 0.0 { sr / sw } else { 0.0 };
    let u = dmin / 80.0;
    let z = flat(p) + lift / (1.0 + u * u);
    z.clamp(range.0 - 150.0, range.1 + 150.0)
}

/// Point and unit tangent at arc length `s` on `pts`.
fn at_s(pts: &[V3], cum: &[f32], s: f32) -> (V3, V2) {
    geom::point_at(pts, cum, s)
}

/// Arc legs as `(from_m, to_m, turn_deg, r)` along the turtle path (the same sampling as
/// [`turtle`], so the distances match the road's).
fn arcs(legs: &[Leg]) -> Vec<(f32, f32, f32, f32)> {
    let mut s = 0.0f32;
    let mut out = Vec::new();
    for leg in legs {
        match *leg {
            Leg::Straight { straight } => s += straight,
            Leg::Arc { turn, r } => {
                let a = turn.to_radians();
                let n = (a.abs() * r / STEP_M).round().max(1.0);
                let (half_sin, _) = wp_math::sin_cos(a.abs() / n * 0.5);
                let len = n * 2.0 * r * half_sin;
                out.push((s, s + len, turn, r));
                s += len;
            }
        }
    }
    out
}

/// Seed of a pass's noise and trees, from its id (FNV-1a).
fn seed_of(id: &str) -> u32 {
    id.bytes().fold(0x811c_9dc5u32, |h, b| {
        (h ^ b as u32).wrapping_mul(0x0100_0193)
    })
}

/// Hash of a lattice point, in [0, 1).
fn hash01(ix: i32, iy: i32, seed: u32) -> f32 {
    let mut h = (ix as u32).wrapping_mul(0x8da6_b343)
        ^ (iy as u32).wrapping_mul(0xd816_3841)
        ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 16_777_216.0
}

/// Smooth value noise at `p` on a lattice of `cell` m, in [-1, 1].
fn value_noise(p: V2, cell: f32, seed: u32) -> f32 {
    let (x, y) = (p[0] / cell, p[1] / cell);
    let (fx, fy) = (x.floor(), y.floor());
    let (ix, iy) = (fx as i32, fy as i32);
    let (tx, ty) = (x - fx, y - fy);
    let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let a = hash01(ix, iy, seed);
    let b = hash01(ix + 1, iy, seed);
    let c = hash01(ix, iy + 1, seed);
    let d = hash01(ix + 1, iy + 1, seed);
    let lo = a + (b - a) * sx;
    let hi = c + (d - c) * sx;
    (lo + (hi - lo) * sy) * 2.0 - 1.0
}

/// Mountain relief at `p`: four octaves of value noise (shoulders 150 m across down to rocky
/// lumps of 10 m), with the larger ones folded into ridges. Roughly [-1, 1].
fn relief(p: V2, seed: u32) -> f32 {
    let ridge = 1.0 - 2.0 * value_noise(p, 150.0, seed).abs();
    0.45 * ridge
        + 0.3 * value_noise(p, 64.0, seed.wrapping_add(1))
        + 0.17 * value_noise(p, 26.0, seed.wrapping_add(2))
        + 0.08 * value_noise(p, 10.0, seed.wrapping_add(3))
}

/// Ground beside the road at `out` m beyond the paved edge (`out > 0`), for an edge of
/// `kind`; `e` = the road edge's height, `zn` = the mountain there, `berm` = least bank height,
/// `deep` = how far a downhill edge falls at least, `low` = this is the downhill side.
fn edge_ground(kind: EdgeKind, out: f32, e: f32, zn: f32, berm: f32, deep: f32, low: bool) -> f32 {
    let top = e - CARVE_BELOW_M;
    // Downhill: a verge falling 3 %, then the slope down to the mountain (at least `deep`).
    let fall = |verge: f32, slope: f32| -> f32 {
        let v = top - 0.03 * out.min(verge);
        let z = v - slope * (out - verge).max(0.0);
        z.max(zn.min(e - deep))
    };
    // Uphill: a verge, then the cut slope up to the mountain (an earth berm where it is lower).
    let bank = |verge: f32| -> f32 {
        let z = top - 0.06 * out.min(verge) + BANK_SLOPE * (out - verge).max(0.0);
        z.min(zn.max(e + berm))
    };
    let near = match kind {
        EdgeKind::Bank => bank(BANK_VERGE_M),
        EdgeKind::Rail => fall(RAIL_VERGE_M, FILL_SLOPE),
        EdgeKind::Drop => fall(DROP_VERGE_M, DROP_SLOPE),
        EdgeKind::Runoff if low => fall(RUNOFF_W_M, FILL_SLOPE),
        EdgeKind::Runoff => bank(RUNOFF_W_M),
    };
    // Far from the road the mountain takes over.
    let w = smoothstep(32.0, 70.0, out);
    near + (zn - near) * w
}

/// Downhill side per centreline sample (`1.0` = right, `-1.0` = left): the lower mountain
/// 25 m out, smoothed along the road, with runs shorter than 80 m folded into their
/// neighbours so the edges do not flicker.
fn low_sides(centre: &[V3], ground: &Heightfield) -> Vec<f32> {
    let n = centre.len();
    let diff: Vec<f32> = (0..n)
        .map(|k| {
            let p = geom::xy(centre[k]);
            let r = geom::right_of(seg_dir(centre, k));
            ground.height_at(geom::add(p, geom::scale(r, 25.0)))
                - ground.height_at(geom::add(p, geom::scale(r, -25.0)))
        })
        .collect();
    let diff = smooth(&diff, 15, 2, 0);
    let mut side: Vec<f32> = diff
        .iter()
        .map(|d| if *d <= 0.0 { 1.0 } else { -1.0 })
        .collect();
    let min_run = (80.0 / STEP_M) as usize;
    let mut k = 0;
    while k < n {
        let mut j = k;
        while j < n && side[j] == side[k] {
            j += 1;
        }
        if j - k < min_run && k > 0 {
            let prev = side[k - 1];
            for v in &mut side[k..j] {
                *v = prev;
            }
        }
        k = j;
    }
    side
}

/// Runs of equal kind along one edge, as spans.
fn spans_of(kinds: &[EdgeKind], cum: &[f32], side: f32, out: &mut Vec<EdgeSpan>) {
    let n = kinds.len();
    let mut k = 0;
    while k < n {
        let mut j = k;
        while j < n && kinds[j] == kinds[k] {
            j += 1;
        }
        out.push(EdgeSpan {
            from_m: cum[k],
            to_m: cum[j - 1],
            side,
            kind: kinds[k],
        });
        k = j;
    }
}

/// Polyline `lat` m to the side of `centre` (positive = right), at the road edge's height.
fn offset_line(centre: &[V3], surf_z: impl Fn(usize) -> f32, lat: f32, ks: &[usize]) -> Vec<V3> {
    ks.iter()
        .map(|&k| {
            let r = geom::right_of(seg_dir(centre, k));
            let q = geom::add(geom::xy(centre[k]), geom::scale(r, lat));
            [q[0], q[1], surf_z(k)]
        })
        .collect()
}

/// Build pass `spec` (see the module docs).
pub fn generate(spec: &TougeSpec) -> TougeBuild {
    let seed = seed_of(&spec.id);
    let (xy, kap) = turtle(&spec.legs);
    let z = smooth(&profile(&kap, spec.climb_m), 6, 3, 0);
    let bank = banking(&kap);
    let n = xy.len();

    // The road: one node at each end, every sample in between as a via point.
    let nodes = vec![
        NodeSpec {
            pos: xy[0],
            z: z[0],
            signal: false,
            control: None,
        },
        NodeSpec {
            pos: xy[n - 1],
            z: z[n - 1],
            signal: false,
            control: None,
        },
    ];
    let mut road = RoadSpec::new(0, 1, RoadClass::Collector, 1, 1);
    road.via = xy[1..n - 1].to_vec();
    road.via_z = z[1..n - 1].to_vec();
    road.bank = bank.clone();
    road.speed_mps = spec.speed_mps;
    road.shoulder_left_m = spec.shoulder_m;
    road.shoulder_right_m = spec.shoulder_m;
    let opt = BuildOptions {
        lane_width_m: spec.lane_width_m,
        ..BuildOptions::default()
    };
    let mut built = builder::build(&nodes, &[road], &opt);
    // A mountain road: no kerbs or sidewalks. The surface code puts a raised sidewalk (and a
    // guard rail behind it where nothing else is) on every non-highway lane edge, which would
    // seal the edges, so the lanes are tagged as open road. Traffic is off on a touge, so the
    // highway speed bias never applies.
    built.surfaces[0].sidewalk = [false, false];
    for lane in &mut built.lanes {
        lane.kind = LaneKind::Highway;
    }
    let surf = built.surfaces[0].clone();
    let lanes = built.road_lanes[0].clone();
    let centre = surf.centre.clone();
    let nc = centre.len();
    let cum = geom::cumulative(&centre);
    let length = *cum.last().unwrap_or(&0.0);
    let half = surf.left_m.max(surf.right_m);
    let wall_off = half + WALL_GAP_M;
    let k_at = |s: f32| -> (usize, f32) {
        let k = cum
            .partition_point(|c| *c <= s)
            .saturating_sub(1)
            .min(nc - 2);
        let t = ((s - cum[k]) / (cum[k + 1] - cum[k]).max(1e-6)).clamp(0.0, 1.0);
        (k, t)
    };

    // Bounds and helpers.
    let mut lo = [f32::INFINITY; 2];
    let mut hi = [f32::NEG_INFINITY; 2];
    for p in &centre {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    let (lo, hi) = (
        [lo[0] - MARGIN_M, lo[1] - MARGIN_M],
        [hi[0] + MARGIN_M, hi[1] + MARGIN_M],
    );
    let seg_grid = SegGrid::new(&centre, lo, hi, 24.0);
    let road_xy = |k: usize, t: f32| -> V2 {
        let (a, b) = (geom::xy(centre[k]), geom::xy(centre[(k + 1).min(nc - 1)]));
        geom::add(a, geom::scale(geom::sub(b, a), t))
    };
    let road_z = |k: usize, t: f32, lat: f32| -> f32 {
        let a = surf.z_at(k, lat);
        let b = surf.z_at((k + 1).min(nc - 1), lat);
        a + (b - a) * t
    };

    // The mountain without the road cut into it: the mean slope through the road, bent to the
    // road heights near it (samples every 8 m), on a coarse grid; then relief that grows away
    // from the road.
    let samples: Vec<(V2, f32)> = centre
        .iter()
        .step_by(4)
        .map(|p| (geom::xy(*p), p[2]))
        .collect();
    let plane = fit_plane(&samples);
    let range = (
        centre[0][2].min(centre[nc - 1][2]),
        centre[0][2].max(centre[nc - 1][2]),
    );
    let coarse_cell = 16.0;
    let mut coarse = Heightfield::flat(lo, hi, coarse_cell, 0.0);
    for j in 0..coarse.ny {
        for i in 0..coarse.nx {
            let p = coarse.vertex_pos(i, j);
            coarse.z[(j * coarse.nx + i) as usize] = natural_at(p, &samples, plane, range);
        }
    }
    // Edges: uphill = cut bank, downhill = guard rail; the spec's open drops; runoff aprons
    // outside the tight bends.
    let low = low_sides(&centre, &coarse);
    let mut edge_r: Vec<EdgeKind> = low
        .iter()
        .map(|l| {
            if *l > 0.0 {
                EdgeKind::Rail
            } else {
                EdgeKind::Bank
            }
        })
        .collect();
    let mut edge_l: Vec<EdgeKind> = low
        .iter()
        .map(|l| {
            if *l < 0.0 {
                EdgeKind::Rail
            } else {
                EdgeKind::Bank
            }
        })
        .collect();
    let in_range = |k: usize, a: f32, b: f32| cum[k] >= a && cum[k] <= b;
    // Where a car runs wide (the outside of a bend, its approach and exit, either way) a drop
    // keeps its rail: open drops are on straights and the inside of bends.
    let bends = arcs(&spec.legs);
    let outside_of_bend = |k: usize, side: f32| {
        bends.iter().any(|&(s0, s1, turn, r)| {
            r < DROP_RAIL_BEND_R_M
                && in_range(k, s0 - DROP_RAIL_MARGIN_M, s1 + DROP_RAIL_MARGIN_M)
                && turn.signum() == side
        })
    };
    for &[a, b] in &spec.drops {
        let (a, b) = (a.clamp(0.0, length), b.clamp(0.0, length));
        let side = low[k_at(0.5 * (a + b)).0];
        for k in (0..nc).filter(|k| in_range(*k, a, b) && !outside_of_bend(*k, side)) {
            let (this, other) = if side > 0.0 {
                (&mut edge_r, &mut edge_l)
            } else {
                (&mut edge_l, &mut edge_r)
            };
            this[k] = EdgeKind::Drop;
            if other[k] == EdgeKind::Rail {
                other[k] = EdgeKind::Bank;
            }
        }
    }
    for (s0, s1, turn, r) in arcs(&spec.legs) {
        if r > RUNOFF_MAX_R_M && turn.abs() < RUNOFF_MIN_TURN_DEG {
            continue;
        }
        let (a, b) = (
            (s0 + 0.35 * (s1 - s0)).max(GRID_IN_M),
            (s1 + 20.0).min(length - GRID_IN_M),
        );
        // Outside of a left turn is the right edge.
        let edge = if turn > 0.0 { &mut edge_r } else { &mut edge_l };
        let ks: Vec<usize> = (0..nc).filter(|k| in_range(*k, a, b)).collect();
        if ks.iter().any(|k| edge[*k] == EdgeKind::Drop) {
            continue;
        }
        for k in ks {
            edge[k] = EdgeKind::Runoff;
        }
    }
    let edge_of = |k: usize, side: f32| if side > 0.0 { edge_r[k] } else { edge_l[k] };
    // Each edge's ground as a mix of its kinds over ±12 m of road, so a bank turns into a fill
    // slope (or a rail verge into a drop) over 24 m instead of at a cliff across the slope.
    // Slots: bank, rail, runoff downhill, runoff uphill, drop.
    let slot = |k: usize, side: f32| match edge_of(k, side) {
        EdgeKind::Bank => 0,
        EdgeKind::Rail => 1,
        EdgeKind::Runoff if low[k] == side => 2,
        EdgeKind::Runoff => 3,
        EdgeKind::Drop => 4,
    };
    let blend_half = (12.0 / STEP_M) as usize;
    let mix_of = |side: f32| -> Vec<[f32; 5]> {
        (0..nc)
            .map(|k| {
                let (a, b) = (k.saturating_sub(blend_half), (k + blend_half).min(nc - 1));
                let mut m = [0.0f32; 5];
                for q in a..=b {
                    m[slot(q, side)] += 1.0 / (b - a + 1) as f32;
                }
                m
            })
            .collect()
    };
    let (mix_l, mix_r) = (mix_of(-1.0), mix_of(1.0));
    const SLOT_KIND: [(EdgeKind, bool); 5] = [
        (EdgeKind::Bank, false),
        (EdgeKind::Rail, true),
        (EdgeKind::Runoff, true),
        (EdgeKind::Runoff, false),
        (EdgeKind::Drop, true),
    ];
    // Ground beside road point `(s, lat, d, k, t)` (from `nearest`) over mountain height `zn`.
    let ground = |(s, lat, d, k, t): (f32, f32, f32, usize, f32), zn: f32| -> f32 {
        let out = d - half;
        if out <= 0.0 {
            // Deeper within a cell and a half of the edge (see the edge cap below).
            let edge = smoothstep(-1.5 * TERRAIN_CELL_M, -TERRAIN_CELL_M, out);
            return road_z(k, t, lat.clamp(-half, half)) - CARVE_BELOW_M - EDGE_CARVE_M * edge;
        }
        let side_ground = |side: f32| -> f32 {
            let e = road_z(k, t, side * half);
            let berm = BERM_H_M + 1.5 * (1.0 + value_noise([s, side], 40.0, seed));
            let mix = if side > 0.0 { &mix_r[k] } else { &mix_l[k] };
            mix.iter()
                .zip(SLOT_KIND)
                .filter(|(w, _)| **w > 0.0)
                .map(|(w, (kind, low))| {
                    w * edge_ground(kind, out, e, zn, berm, spec.cliff_drop_m, low)
                })
                .sum()
        };
        let beyond_end = (k == 0 && t <= 0.0) || (k == nc - 2 && t >= 1.0);
        if beyond_end && lat.abs() < half {
            // Past either end of the road: both sides meet across its extended line.
            let w = smoothstep(-half, half, lat);
            side_ground(-1.0) * (1.0 - w) + side_ground(1.0) * w
        } else {
            side_ground(if lat >= 0.0 { 1.0 } else { -1.0 })
        }
    };

    // Fine terrain: the mountain, with the road and its edges cut into it.
    let mut hf = Heightfield::flat(lo, hi, TERRAIN_CELL_M, 0.0);
    let mut near_segs = Vec::new();
    let mut hits: Vec<(f32, f32, f32, usize, f32)> = Vec::new();
    for j in 0..hf.ny {
        for i in 0..hf.nx {
            let p = hf.vertex_pos(i, j);
            seg_grid.near(p, 60.0, &mut near_segs);
            near_segs.sort_unstable();
            near_segs.dedup();
            hits.clear();
            hits.extend(
                near_segs
                    .iter()
                    .filter_map(|sg| nearest(&centre, &cum, &[*sg], p)),
            );
            let dmin = hits.iter().map(|h| h.2).fold(f32::INFINITY, f32::min);
            let zn = coarse.height_at(p)
                + RELIEF_M * relief(p, seed) * smoothstep(8.0, 60.0, dmin.min(60.0));
            let mut zt = zn;
            if dmin <= half {
                // On the paving.
                let a = hits.iter().find(|h| h.2 == dmin).copied().unwrap();
                zt = ground(a, zn);
            } else if dmin.is_finite() {
                // Every road point a little further than the nearest has a say, the nearer
                // the more (within 1 m at the verge, 8 m from 8 m out): around a bend and
                // between two legs of a hairpin stack the grounds meet smoothly.
                let reach = (dmin - half).clamp(1.0, 8.0);
                let (mut sw, mut sz) = (0.0f32, 0.0f32);
                for h in hits.iter().filter(|h| h.2 - dmin < reach) {
                    let w = 1.0 - smoothstep(0.0, reach, h.2 - dmin);
                    sw += w;
                    sz += w * ground(*h, zn);
                }
                zt = sz / sw;
                // The ground never undermines another leg's verge nor rises above its paving
                // (a rock cut between stacked legs, at most [`LEG_CUT_SLOPE`]). Another leg:
                // much further along the road than across the ground.
                let a = hits.iter().find(|h| h.2 == dmin).copied().unwrap();
                let pa = road_xy(a.3, a.4);
                let others = || {
                    hits.iter().filter(move |h| {
                        h.2 - half < 40.0
                            && (h.0 - a.0).abs() - geom::dist(road_xy(h.3, h.4), pa) > 12.0
                    })
                };
                for &(_, lat_o, d_o, ko, to) in others() {
                    let eo = road_z(ko, to, lat_o.clamp(-half, half)) - CARVE_BELOW_M;
                    zt = zt.max(eo - LEG_CUT_SLOPE * (d_o - half - RAIL_VERGE_M).max(0.0));
                }
                for &(_, lat_o, d_o, ko, to) in others() {
                    let eo = road_z(ko, to, lat_o.clamp(-half, half)) - CARVE_BELOW_M;
                    zt = zt.min(eo + LEG_CUT_SLOPE * (d_o - half - BANK_VERGE_M).max(0.0));
                }
                // Edge cap: a terrain triangle reaching from under the paving (at most
                // `depth` below it, `reach - x` in) to this vertex `x` out stays under the
                // paved edge.
                let x = dmin - half;
                let reach = TERRAIN_CELL_M * core::f32::consts::SQRT_2;
                if x < reach {
                    let depth = CARVE_BELOW_M + EDGE_CARVE_M;
                    let e = road_z(a.3, a.4, a.1.clamp(-half, half));
                    zt = zt.min(e + depth * x / (reach - x).max(0.05));
                }
            }
            hf.z[(j * hf.nx + i) as usize] = zt;
        }
    }

    // End walls across the road, just inside each end.
    let mut decor = Vec::new();
    for s in [1.0f32, length - 1.0] {
        let (p, d) = at_s(&centre, &cum, s);
        let width = 2.0 * (wall_off + WALL_W_M);
        decor.push(Decor {
            kind: DecorKind::Fence,
            pos: [p[0], p[1], p[2] - 0.05 - 0.06 * half],
            yaw: geom::yaw_of(geom::right_of(d)),
            size: [WALL_W_M, width, WALL_H_M + 0.15 * half],
        });
    }

    // Guard rails along every downhill rail run, and at the back of downhill runoff aprons.
    // Sections follow the rail's own line (longer than the centreline outside a bend), the
    // beam (local −X) towards the road.
    let mut rails = Vec::new();
    for side in [-1.0f32, 1.0] {
        let mut k = 0;
        while k < nc {
            let kind = edge_of(k, side);
            let rail_lat = match kind {
                EdgeKind::Rail => Some(half + RAIL_GAP_M + 0.5 * RAIL_W_M),
                EdgeKind::Runoff if low[k] == side => Some(half + RUNOFF_W_M - 0.5 * RAIL_W_M),
                _ => None,
            };
            let mut j = k;
            while j < nc && edge_of(j, side) == kind && (low[j] == side) == (low[k] == side) {
                j += 1;
            }
            let Some(lat) = rail_lat else {
                k = j;
                continue;
            };
            let ks: Vec<usize> = (k..j.min(nc)).collect();
            let line = offset_line(&centre, |q| surf.z_at(q, side * half), side * lat, &ks);
            k = j;
            if line.len() < 2 {
                continue;
            }
            let lcum = geom::cumulative(&line);
            let run = *lcum.last().unwrap_or(&0.0);
            if run < 0.5 * RAIL_LEN_M {
                continue;
            }
            let count = (run / RAIL_LEN_M).round().max(1.0) as usize;
            let pitch = run / count as f32;
            for c in 0..count {
                let (pa, _) = geom::point_at(&line, &lcum, c as f32 * pitch);
                let (pb, _) = geom::point_at(&line, &lcum, (c + 1) as f32 * pitch);
                let (pm, _) = geom::point_at(&line, &lcum, (c as f32 + 0.5) * pitch);
                let along = geom::normalize(geom::sub(geom::xy(pb), geom::xy(pa)));
                let facing = if side > 0.0 {
                    along
                } else {
                    geom::scale(along, -1.0)
                };
                let len = geom::dist(geom::xy(pa), geom::xy(pb)).max(0.5);
                rails.push(StaticProp {
                    kind: PropKind::GuardRail,
                    pos: [pm[0], pm[1], pm[2] - CARVE_BELOW_M + RAIL_H_M * 0.5],
                    yaw: geom::yaw_of(facing),
                    half_extents: [RAIL_W_M * 0.5, len * 0.5 + 0.02, RAIL_H_M * 0.5],
                    mass_kg: RAIL_MASS_KG,
                });
            }
        }
    }

    // Edge runs, and dirt zones over the runoff aprons and drops (first zone wins, so they
    // go before the mountain's).
    let mut edges = Vec::new();
    spans_of(&edge_l, &cum, -1.0, &mut edges);
    spans_of(&edge_r, &cum, 1.0, &mut edges);
    let mut zones = Vec::new();
    for e in edges
        .iter()
        .filter(|e| matches!(e.kind, EdgeKind::Runoff | EdgeKind::Drop))
    {
        let ks: Vec<usize> = (0..nc)
            .filter(|k| in_range(*k, e.from_m - 2.0, e.to_m + 2.0))
            .collect();
        if ks.len() < 2 {
            continue;
        }
        let reach = if e.kind == EdgeKind::Runoff {
            RUNOFF_W_M
        } else {
            DROP_VERGE_M + 10.0
        };
        let inner = offset_line(&centre, |_| 0.0, e.side * (half - 0.2), &ks);
        let outer = offset_line(&centre, |_| 0.0, e.side * (half + reach), &ks);
        let mut polygon: Vec<V2> = inner.iter().map(|p| geom::xy(*p)).collect();
        polygon.extend(outer.iter().rev().map(|p| geom::xy(*p)));
        zones.push(Zone {
            kind: DistrictKind::Industrial,
            density_base: 0.0,
            polygon,
        });
    }
    zones.push(Zone {
        kind: DistrictKind::Suburb,
        density_base: 0.0,
        polygon: vec![lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]],
    });

    // Pine forest on the slopes: patchy (relief noise), clear of the verges, aprons and every
    // leg of the road, and off the cliffs.
    let mut rng = Rng::new(seed as u64);
    let pitch = 8.0;
    let (tx, ty) = (
        ((hi[0] - lo[0]) / pitch) as usize,
        ((hi[1] - lo[1]) / pitch) as usize,
    );
    let mut trees = 0usize;
    'grid: for j in 0..ty {
        for i in 0..tx {
            let q = [
                lo[0] + (i as f32 + 0.5) * pitch + rng.range_f32(-3.0, 3.0),
                lo[1] + (j as f32 + 0.5) * pitch + rng.range_f32(-3.0, 3.0),
            ];
            let patch = 0.5 + 0.5 * value_noise(q, 90.0, seed.wrapping_add(7));
            let keep = rng.chance((spec.forest * (0.25 + 0.75 * patch)).clamp(0.0, 1.0));
            let size = rng.next_f32();
            if !keep {
                continue;
            }
            seg_grid.near(q, 30.0, &mut near_segs);
            if nearest(&centre, &cum, &near_segs, q).is_some_and(|b| b.2 < half + RUNOFF_W_M + 3.0)
            {
                continue;
            }
            if !hf.contains(q) || hf.normal_at(q)[2] < 0.72 {
                continue;
            }
            let h = 9.0 + 11.0 * size;
            decor.push(Decor {
                kind: DecorKind::PineTree,
                pos: [q[0], q[1], hf.height_at(q)],
                yaw: 0.0,
                size: [0.32 * h, 0.32 * h, h],
            });
            trees += 1;
            if trees >= MAX_TREES {
                break 'grid;
            }
        }
    }

    // Race grids and finish lines.
    let lane_off = spec.lane_width_m * 0.5;
    let poi = |name: String, s: f32, uphill: bool, lane: Option<LaneId>| {
        let (p, d) = at_s(&centre, &cum, s);
        let travel = if uphill { d } else { geom::scale(d, -1.0) };
        let q = geom::add(geom::xy(p), geom::scale(geom::right_of(travel), lane_off));
        let (k, t) = k_at(s);
        let lat = if uphill { lane_off } else { -lane_off };
        Poi {
            kind: PoiKind::RaceStartGrid,
            name,
            pos: [q[0], q[1], road_z(k, t, lat)],
            heading_rad: geom::yaw_of(travel),
            lane,
        }
    };
    let up_lane = lanes.fwd.first().copied();
    let down_lane = lanes.bwd.first().copied();
    let pois = vec![
        poi(
            format!("{} base start", spec.name),
            GRID_IN_M,
            true,
            up_lane,
        ),
        poi(
            format!("{} summit finish", spec.name),
            length - FINISH_IN_M,
            true,
            up_lane,
        ),
        poi(
            format!("{} summit start", spec.name),
            length - GRID_IN_M,
            false,
            down_lane,
        ),
        poi(
            format!("{} base finish", spec.name),
            FINISH_IN_M,
            false,
            down_lane,
        ),
    ];

    let mut net = built.into_network(
        &spec.id,
        &format!("touge spec={}", spec.id),
        None,
        geom::xy(centre[0]),
        zones,
        pois,
        Vec::new(),
    );
    net.bounds_min = lo;
    net.bounds_max = hi;
    net.terrain = Some(hf);
    net.decor = decor;

    let mut max_grade = 0.0f32;
    for k in 0..nc - 1 {
        let run = cum[k + 1] - cum[k];
        if run > 0.5 {
            max_grade = max_grade.max((centre[k + 1][2] - centre[k][2]).abs() / run);
        }
    }
    let track = TougeTrack {
        id: spec.id.clone(),
        name: spec.name.clone(),
        length_m: length,
        climb_m: centre[nc - 1][2] - centre[0][2],
        max_grade,
        half_width_m: half,
        edges,
        rail_sections: rails.len(),
        trees,
    };
    TougeBuild { net, rails, track }
}

/// Unit XY direction of centreline segment `k`.
fn seg_dir(pts: &[V3], k: usize) -> V2 {
    let k = k.min(pts.len() - 2);
    geom::normalize(geom::sub(geom::xy(pts[k + 1]), geom::xy(pts[k])))
}

/// Build built-in pass `id`.
pub fn generate_builtin(id: &str) -> Option<TougeBuild> {
    spec(id).map(|s| generate(&s))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<TougeBuild> {
        TOUGE_IDS
            .iter()
            .map(|id| generate_builtin(id).unwrap())
            .collect()
    }

    /// Edge of `b` at distance `s` on `side`.
    fn edge_at(b: &TougeBuild, s: f32, side: f32) -> EdgeKind {
        b.track
            .edges
            .iter()
            .rfind(|e| e.side == side && s >= e.from_m)
            .unwrap()
            .kind
    }

    /// Point `out` m beyond the paved edge on `side` at distance `s`, and the road edge height.
    fn beside(b: &TougeBuild, s: f32, side: f32, out: f32) -> (V2, f32) {
        let c = &b.net.surfaces[0].centre;
        let cum = geom::cumulative(c);
        let (p, d) = geom::point_at(c, &cum, s);
        let half = b.track.half_width_m;
        let q = geom::add(
            geom::xy(p),
            geom::scale(geom::right_of(d), side * (half + out)),
        );
        let k = cum.partition_point(|x| *x <= s).saturating_sub(1);
        (q, b.net.surfaces[0].z_at(k, side * half))
    }

    #[test]
    fn every_pass_parses_and_validates() {
        for b in all() {
            b.net
                .validate()
                .unwrap_or_else(|e| panic!("{}: {e}", b.track.id));
            assert_eq!(b.net.surfaces.len(), 1, "{}", b.track.id);
            assert_eq!(b.net.pois.len(), 4, "{}", b.track.id);
        }
    }

    #[test]
    fn passes_are_long_steep_and_lined_with_rails_banks_runoff_and_drops() {
        for b in all() {
            let t = &b.track;
            eprintln!(
                "{}: {:.0} m, climb {:.0} m, max grade {:.1} %, {} rail sections, {} trees, \
                 rail {:.0} m, bank {:.0} m, runoff {:.0} m, drop {:.0} m",
                t.id,
                t.length_m,
                t.climb_m,
                t.max_grade * 100.0,
                t.rail_sections,
                t.trees,
                t.edge_m(EdgeKind::Rail),
                t.edge_m(EdgeKind::Bank),
                t.edge_m(EdgeKind::Runoff),
                t.edge_m(EdgeKind::Drop),
            );
            assert!(
                t.length_m > 2200.0 && t.length_m < 3600.0,
                "{}: {}",
                t.id,
                t.length_m
            );
            assert!(t.climb_m > 190.0, "{}: climb {}", t.id, t.climb_m);
            assert!(t.max_grade < 0.17, "{}: grade {}", t.id, t.max_grade);
            // The spec's drops are built, except on the outside of bends.
            let spec = spec(&t.id).unwrap();
            let asked: f32 = spec.drops.iter().map(|d| d[1] - d[0]).sum();
            assert!(t.spans(EdgeKind::Drop).count() >= 2, "{}", t.id);
            assert!(t.edge_m(EdgeKind::Drop) > 0.4 * asked, "{}", t.id);
            // Rails line most of the downhill side; banks most of the uphill one; tight bends
            // have runoff.
            assert!(t.edge_m(EdgeKind::Rail) > 0.5 * t.length_m, "{}", t.id);
            assert!(t.edge_m(EdgeKind::Bank) > 0.5 * t.length_m, "{}", t.id);
            assert!(t.spans(EdgeKind::Runoff).count() >= 2, "{}", t.id);
            assert!(
                t.rail_sections as f32 * RAIL_LEN_M > 0.5 * t.length_m,
                "{}: {} sections",
                t.id,
                t.rail_sections
            );
            // Never a rail or a drop on both sides at once: one side is always the mountain.
            for s in (0..t.length_m as usize).step_by(10) {
                let s = s as f32;
                let (l, r) = (edge_at(&b, s, -1.0), edge_at(&b, s, 1.0));
                let falls = |k: EdgeKind| matches!(k, EdgeKind::Rail | EdgeKind::Drop);
                assert!(!(falls(l) && falls(r)), "{} s={s}: {l:?} / {r:?}", t.id);
            }
            if spec.forest > 0.0 {
                assert!(t.trees > 300, "{}: {} trees", t.id, t.trees);
            }
            // The runoff aprons and drops read as dirt.
            let dirt = b
                .net
                .zones
                .iter()
                .filter(|z| z.kind == DistrictKind::Industrial)
                .count();
            assert_eq!(
                dirt,
                t.spans(EdgeKind::Runoff).count() + t.spans(EdgeKind::Drop).count(),
                "{}",
                t.id
            );
        }
    }

    #[test]
    fn hairpins_have_runoff_on_the_outside() {
        let b = generate_builtin("touge_cedar_switchback").unwrap();
        let spec = spec("touge_cedar_switchback").unwrap();
        let hairpins: Vec<_> = arcs(&spec.legs)
            .into_iter()
            .filter(|a| a.2.abs() > 150.0)
            .collect();
        assert_eq!(hairpins.len(), 2);
        for (s0, s1, turn, _) in hairpins {
            let outside = if turn > 0.0 { 1.0 } else { -1.0 };
            assert_eq!(
                edge_at(&b, 0.5 * (s0 + s1) + 5.0, outside),
                EdgeKind::Runoff
            );
        }
    }

    #[test]
    fn open_drops_are_never_on_the_outside_of_a_bend() {
        for b in all() {
            let spec = spec(&b.track.id).unwrap();
            for (s0, s1, turn, r) in arcs(&spec.legs) {
                if r >= DROP_RAIL_BEND_R_M {
                    continue;
                }
                let outside = turn.signum();
                let mut s = s0 - DROP_RAIL_MARGIN_M + 3.0;
                while s < s1 + DROP_RAIL_MARGIN_M - 3.0 {
                    assert_ne!(
                        edge_at(&b, s, outside),
                        EdgeKind::Drop,
                        "{} s={s}",
                        b.track.id
                    );
                    s += 2.0;
                }
            }
        }
    }

    #[test]
    fn road_never_comes_close_to_itself() {
        for b in all() {
            let c = &b.net.surfaces[0].centre;
            let cum = geom::cumulative(c);
            let min_gap = 2.0 * (b.track.half_width_m + RAIL_GAP_M + RAIL_W_M) + 4.0;
            for i in (0..c.len()).step_by(3) {
                for j in (i..c.len()).step_by(3) {
                    if cum[j] - cum[i] < 80.0 {
                        continue;
                    }
                    let d = geom::dist(geom::xy(c[i]), geom::xy(c[j]));
                    assert!(
                        d > min_gap,
                        "{}: s={} and s={} only {d} m apart",
                        b.track.id,
                        cum[i],
                        cum[j]
                    );
                }
            }
        }
    }

    #[test]
    fn terrain_stays_under_the_road_falls_away_downhill_and_rises_uphill() {
        for b in all() {
            let id = &b.track.id;
            let t = b.net.terrain.as_ref().unwrap();
            let s = &b.net.surfaces[0];
            // Under the paving right out to its edges.
            let half = b.track.half_width_m;
            for k in (1..s.centre.len() - 1).step_by(2) {
                let p = s.centre[k];
                let r = geom::right_of(geom::normalize(geom::sub(
                    geom::xy(s.centre[k + 1]),
                    geom::xy(s.centre[k - 1]),
                )));
                for lat in [-half, -0.5 * half, 0.0, 0.5 * half, half] {
                    let q = geom::add(geom::xy(p), geom::scale(r, lat));
                    assert!(
                        t.height_at(q) <= s.z_at(k, lat) + 0.02,
                        "{id} k={k} lat={lat}: {} above the road",
                        t.height_at(q) - s.z_at(k, lat)
                    );
                }
            }
            // Behind every open drop the mountain falls well away.
            for span in b.track.spans(EdgeKind::Drop) {
                let (q, e) = beside(&b, 0.5 * (span.from_m + span.to_m), span.side, 12.0);
                assert!(t.height_at(q) < e - 8.0, "{id}: no drop at {}", span.from_m);
            }
            // Behind most rail (a car that breaks through falls), above most bank.
            let (mut rail, mut rail_fall, mut bank, mut bank_up) = (0, 0, 0, 0);
            for e in &b.track.edges {
                let mut s = e.from_m + 2.0;
                while s < e.to_m - 2.0 {
                    match e.kind {
                        EdgeKind::Rail => {
                            let (q, z) = beside(&b, s, e.side, RAIL_VERGE_M + 8.0);
                            rail += 1;
                            rail_fall += usize::from(t.height_at(q) < z - 4.0);
                            // The rail stands on a level verge (away from the 12 m blends
                            // into the neighbouring edges).
                            if s - e.from_m > 14.0 && e.to_m - s > 14.0 {
                                let (v, z) = beside(&b, s, e.side, 0.5);
                                let dz = t.height_at(v) - z;
                                assert!(dz.abs() < 0.6, "{id} s={s}: verge {dz}");
                            }
                        }
                        EdgeKind::Bank => {
                            let (q, z) = beside(&b, s, e.side, BANK_VERGE_M + 3.0);
                            bank += 1;
                            bank_up += usize::from(t.height_at(q) > z + 1.0);
                        }
                        _ => {}
                    }
                    s += 5.0;
                }
            }
            eprintln!("{id}: rail falls {rail_fall}/{rail}, bank rises {bank_up}/{bank}");
            assert!(rail_fall * 10 >= rail * 8, "{id}: {rail_fall}/{rail}");
            assert!(bank_up * 10 >= bank * 8, "{id}: {bank_up}/{bank}");
        }
    }

    #[test]
    fn rails_face_the_road_from_beside_it() {
        for b in all() {
            let c = &b.net.surfaces[0].centre;
            let cum = geom::cumulative(c);
            let half = b.track.half_width_m;
            for r in &b.rails {
                assert_eq!(r.kind, PropKind::GuardRail);
                assert_eq!(r.mass_kg, RAIL_MASS_KG);
                let p = geom::project(c, &cum, geom::xy(r.pos));
                let out = p.dist - half;
                assert!(
                    (RAIL_GAP_M..=RUNOFF_W_M + 0.1).contains(&out),
                    "{}: rail {out} m beyond the edge",
                    b.track.id
                );
                // Local −X (the beam) towards the road.
                let x = geom::right_of(geom::dir_of_yaw(r.yaw));
                let (q, _) = geom::point_at(c, &cum, p.s);
                let to_road = geom::normalize(geom::sub(geom::xy(q), geom::xy(r.pos)));
                assert!(
                    geom::dot(x, to_road) < -0.9,
                    "{}: rail faces away",
                    b.track.id
                );
            }
        }
    }

    #[test]
    fn generation_is_deterministic() {
        let a = generate_builtin(TOUGE_IDS[0]).unwrap();
        let b = generate_builtin(TOUGE_IDS[0]).unwrap();
        assert_eq!(a, b);
    }
}
