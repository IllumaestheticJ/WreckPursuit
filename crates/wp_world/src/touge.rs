//! Touge mountain passes (`touge_*`): sealed, single-road hill climbs for boss and challenge
//! races. Coordinates in metres, +X east, +Y north, +Z up.
//!
//! Each pass is described by a small JSON spec in `data/touge/<id>.json` (embedded at build
//! time, see [`BUILTIN_SPECS`]): a turtle path of straights and arcs from the base, the total
//! climb, lane and shoulder widths and the stretches where the outboard wall is a line of
//! destructible K-rail sections instead of a fixed wall. [`generate`] turns a spec into:
//!
//! - one two-lane road (one lane each way) from the base node to the summit node, with a
//!   vertical profile that climbs `climb_m` (hairpins flatter than the straights) and banking
//!   from the curvature;
//! - a terrain heightfield shaped like a mountain around the road, carved under it, with a
//!   cliff dug out beyond every K-rail stretch so a car that breaks through falls;
//! - fixed walls ([`DecorKind::Fence`] boxes, solid in `wp_sim`) along both paved edges and
//!   across both ends, so the track is sealed;
//! - K-rail props ([`PropKind::KRail`]) on the outboard edge of every K-rail stretch: static
//!   until a car comes near, then heavy dynamic boxes that a hard enough hit shoves aside;
//! - four race-grid POIs: `<name> base start` / `<name> summit finish` (uphill) and
//!   `<name> summit start` / `<name> base finish` (downhill).
//!
//! The race rules (a car past the wall line is totaled and the battle is over) live in
//! `wp_race::touge`; this module only builds the place.
//!
//! Everything is plain `f32` arithmetic plus `wp_math`'s deterministic trig, so the same spec
//! gives bit-identical worlds on native and wasm.

use serde::{Deserialize, Serialize};

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
/// Fixed wall: width, m.
pub const WALL_W_M: f32 = 0.5;
/// Fixed wall: height above the road edge, m.
pub const WALL_H_M: f32 = 1.2;
/// Gap between the paved edge and the wall's inner face, m.
pub const WALL_GAP_M: f32 = 0.05;
/// K-rail section: length, m.
pub const KRAIL_LEN_M: f32 = 2.0;
/// K-rail section: width at the base, m (the collider is a box this wide).
pub const KRAIL_W_M: f32 = 0.6;
/// K-rail section: height, m.
pub const KRAIL_H_M: f32 = 0.81;
/// K-rail section mass, kg (a light precast section: a hard hit shoves it aside, a scrape
/// only nudges it).
pub const KRAIL_MASS_KG: f32 = 600.0;
/// Terrain grid spacing, m.
pub const TERRAIN_CELL_M: f32 = 4.0;
/// Terrain sits this far below the road surface under the paving, m.
pub const CARVE_BELOW_M: f32 = 0.12;
/// Ground margin around the road's bounding box, m.
pub const MARGIN_M: f32 = 150.0;
/// Start grids sit this far in from each end of the road, m.
pub const GRID_IN_M: f32 = 60.0;
/// Finish lines sit this far in from each end of the road, m.
pub const FINISH_IN_M: f32 = 40.0;

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
    /// Depth of the cliff beyond each K-rail stretch, m.
    pub cliff_drop_m: f32,
    /// K-rail stretches `[from_m, to_m]` along the road from the base. The outboard side
    /// (the lower ground) is picked by the generator.
    pub krail: Vec<[f32; 2]>,
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

/// A K-rail stretch as built.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct KRailSpan {
    /// Start, m along the road from the base.
    pub from_m: f32,
    /// End, m along the road from the base.
    pub to_m: f32,
    /// Outboard side: `1.0` = right of the base → summit direction, `-1.0` = left.
    pub side: f32,
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
    /// Centreline to the inner face of the walls / K-rails, m.
    pub wall_offset_m: f32,
    /// K-rail stretches with their outboard side.
    pub krail: Vec<KRailSpan>,
    /// Number of fixed wall pieces.
    pub wall_pieces: usize,
    /// Number of K-rail sections.
    pub krail_sections: usize,
}

/// Everything [`generate`] builds.
#[derive(Debug, Clone, PartialEq)]
pub struct TougeBuild {
    /// The road network (one road, terrain, wall decor, four POIs).
    pub net: RoadNetwork,
    /// K-rail props.
    pub krails: Vec<StaticProp>,
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

/// Natural mountain ground at `p` from the road samples `zs` near it (weighted by distance),
/// falling away beyond 25 m from the nearest one.
fn natural_at(p: V2, samples: &[(V2, f32)], grid: &PointGrid) -> f32 {
    let mut ids = Vec::new();
    grid.near(p, 170.0, &mut ids);
    let (mut sw, mut sz, mut dmin) = (0.0f32, 0.0f32, f32::INFINITY);
    for &i in &ids {
        let (q, z) = samples[i as usize];
        let d = geom::dist(p, q);
        dmin = dmin.min(d);
        let u = d / 30.0;
        let w = 1.0 / ((1.0 + u * u) * (1.0 + u * u));
        sw += w;
        sz += w * z;
    }
    if sw <= 0.0 {
        return samples.iter().map(|s| s.1).fold(f32::INFINITY, f32::min) - 40.0;
    }
    sz / sw - 0.25 * (dmin - 25.0).max(0.0)
}

/// Spatial buckets of points.
struct PointGrid {
    lo: V2,
    cell: f32,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<u32>>,
}

impl PointGrid {
    fn new(pts: &[(V2, f32)], lo: V2, hi: V2, cell: f32) -> Self {
        let nx = ((hi[0] - lo[0]) / cell).ceil() as usize + 1;
        let ny = ((hi[1] - lo[1]) / cell).ceil() as usize + 1;
        let mut cells = vec![Vec::new(); nx * ny];
        for (n, (p, _)) in pts.iter().enumerate() {
            let i = (((p[0] - lo[0]) / cell).floor().max(0.0) as usize).min(nx - 1);
            let j = (((p[1] - lo[1]) / cell).floor().max(0.0) as usize).min(ny - 1);
            cells[j * nx + i].push(n as u32);
        }
        Self {
            lo,
            cell,
            nx,
            ny,
            cells,
        }
    }

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

/// Point and unit tangent at arc length `s` on `pts`.
fn at_s(pts: &[V3], cum: &[f32], s: f32) -> (V3, V2) {
    geom::point_at(pts, cum, s)
}

/// Build pass `spec` (see the module docs).
pub fn generate(spec: &TougeSpec) -> TougeBuild {
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
    let built = builder::build(&nodes, &[road], &opt);
    let mut built = built;
    // A mountain road: no kerbs or sidewalks. The surface code puts a raised sidewalk (and a
    // guard rail behind it where nothing else is) on every non-highway lane edge, which would
    // seal the K-rail drops, so the lanes are tagged as open road. Traffic is off on a touge,
    // so the highway speed bias never applies.
    built.surfaces[0].sidewalk = [false, false];
    for lane in &mut built.lanes {
        lane.kind = LaneKind::Highway;
    }
    let surf = built.surfaces[0].clone();
    let lanes = built.road_lanes[0].clone();
    let centre = surf.centre.clone();
    let cum = geom::cumulative(&centre);
    let length = *cum.last().unwrap_or(&0.0);
    let half = surf.left_m.max(surf.right_m);
    let wall_off = half + WALL_GAP_M;

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
    let road_z = |k: usize, t: f32, lat: f32| -> f32 {
        let a = surf.z_at(k, lat);
        let b = surf.z_at((k + 1).min(centre.len() - 1), lat);
        a + (b - a) * t
    };

    // Natural ground on a coarse grid (road samples every 8 m), bilinear on the fine one.
    let samples: Vec<(V2, f32)> = centre
        .iter()
        .step_by(4)
        .map(|p| (geom::xy(*p), p[2]))
        .collect();
    let pgrid = PointGrid::new(&samples, lo, hi, 40.0);
    let coarse_cell = TERRAIN_CELL_M * 4.0;
    let mut coarse = Heightfield::flat(lo, hi, coarse_cell, 0.0);
    for j in 0..coarse.ny {
        for i in 0..coarse.nx {
            let p = coarse.vertex_pos(i, j);
            coarse.z[(j * coarse.nx + i) as usize] = natural_at(p, &samples, &pgrid);
        }
    }

    // K-rail outboard side: the lower natural ground 20 m out at the stretch's middle.
    let krail: Vec<KRailSpan> = spec
        .krail
        .iter()
        .map(|&[a, b]| {
            let (from_m, to_m) = (a.clamp(0.0, length), b.clamp(0.0, length));
            let (p, d) = at_s(&centre, &cum, 0.5 * (from_m + to_m));
            let r = geom::right_of(d);
            let zr = coarse.height_at(geom::add(geom::xy(p), geom::scale(r, 20.0)));
            let zl = coarse.height_at(geom::add(geom::xy(p), geom::scale(r, -20.0)));
            KRailSpan {
                from_m,
                to_m,
                side: if zr <= zl { 1.0 } else { -1.0 },
            }
        })
        .collect();

    // Fine terrain: natural ground, carved under the road, cliffs beyond the K-rails.
    let mut hf = Heightfield::flat(lo, hi, TERRAIN_CELL_M, 0.0);
    let mut segs = Vec::new();
    // Flat ground out to one terrain cell past the K-rails, so a rail stands level (the
    // terrain is bilinear between 4 m vertices) and slides before it tips over the edge.
    let shelf = wall_off + KRAIL_W_M + 0.5 + TERRAIN_CELL_M;
    for j in 0..hf.ny {
        for i in 0..hf.nx {
            let p = hf.vertex_pos(i, j);
            let mut zt = coarse.height_at(p);
            seg_grid.near(p, 40.0, &mut segs);
            if let Some((s, lat, dist, k, t)) = nearest(&centre, &cum, &segs, p) {
                let edge_lat = lat.clamp(-half, half);
                let under = road_z(k, t, edge_lat) - CARVE_BELOW_M;
                let blend = smoothstep(shelf, shelf + 14.0, dist);
                zt = under + (zt - under) * blend;
                for span in &krail {
                    if lat * span.side <= half {
                        continue;
                    }
                    let along = smoothstep(span.from_m - 12.0, span.from_m, s)
                        * smoothstep(span.to_m + 12.0, span.to_m, s);
                    if along <= 0.0 {
                        continue;
                    }
                    let out = lat.abs() - half;
                    let cliff = under
                        - spec.cliff_drop_m
                            * smoothstep(shelf - half, shelf - half + 6.0, out)
                            * along;
                    zt = zt.min(cliff);
                }
            }
            hf.z[(j * hf.nx + i) as usize] = zt;
        }
    }

    // Walls along both edges, except the outboard edge of each K-rail stretch.
    let mut decor = Vec::new();
    let in_krail = |s: f32, side: f32| {
        krail
            .iter()
            .any(|k| k.side == side && s >= k.from_m - 1.0 && s <= k.to_m + 1.0)
    };
    let piece = 2usize; // samples per wall piece (4 m)
    for side in [-1.0f32, 1.0] {
        let lat = side * (wall_off + WALL_W_M * 0.5);
        let mut k = 0;
        while k + 1 < centre.len() {
            let k1 = (k + piece).min(centre.len() - 1);
            let mid_s = 0.5 * (cum[k] + cum[k1]);
            if !in_krail(mid_s, side) {
                let a = geom::add(
                    geom::xy(centre[k]),
                    geom::scale(geom::right_of(seg_dir(&centre, k)), lat),
                );
                let b = geom::add(
                    geom::xy(centre[k1]),
                    geom::scale(geom::right_of(seg_dir(&centre, k1.saturating_sub(1))), lat),
                );
                let za = surf.z_at(k, side * half);
                let zb = surf.z_at(k1, side * half);
                let len = geom::dist(a, b);
                if len > 0.05 {
                    let base = za.min(zb) - 0.05;
                    decor.push(Decor {
                        kind: DecorKind::Fence,
                        pos: [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1]), base],
                        yaw: geom::yaw_of(geom::normalize(geom::sub(b, a))),
                        size: [WALL_W_M, len + 0.15, WALL_H_M + (za - zb).abs() + 0.05],
                    });
                }
            }
            k = k1;
        }
    }
    let wall_pieces_sides = decor.len();
    // End walls across the road, just inside each end.
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

    // K-rail sections on the outboard edge of each stretch.
    let mut krails = Vec::new();
    for span in &krail {
        let lat = span.side * (wall_off + KRAIL_W_M * 0.5);
        let count = ((span.to_m - span.from_m) / (KRAIL_LEN_M + 0.05))
            .floor()
            .max(1.0) as usize;
        let pitch = (span.to_m - span.from_m) / count as f32;
        for c in 0..count {
            let s = span.from_m + (c as f32 + 0.5) * pitch;
            let (p, d) = at_s(&centre, &cum, s);
            let q = geom::add(geom::xy(p), geom::scale(geom::right_of(d), lat));
            let k = cum
                .partition_point(|c| *c <= s)
                .saturating_sub(1)
                .min(centre.len() - 2);
            let t = ((s - cum[k]) / (cum[k + 1] - cum[k]).max(1e-6)).clamp(0.0, 1.0);
            let ze = road_z(k, t, span.side * half);
            krails.push(StaticProp {
                kind: PropKind::KRail,
                pos: [q[0], q[1], ze + KRAIL_H_M * 0.5],
                yaw: geom::yaw_of(d),
                half_extents: [KRAIL_W_M * 0.5, KRAIL_LEN_M * 0.5, KRAIL_H_M * 0.5],
                mass_kg: KRAIL_MASS_KG,
            });
        }
    }

    // Race grids and finish lines.
    let lane_off = spec.lane_width_m * 0.5;
    let poi = |name: String, s: f32, uphill: bool, lane: Option<LaneId>| {
        let (p, d) = at_s(&centre, &cum, s);
        let travel = if uphill { d } else { geom::scale(d, -1.0) };
        let q = geom::add(geom::xy(p), geom::scale(geom::right_of(travel), lane_off));
        let k = cum
            .partition_point(|c| *c <= s)
            .saturating_sub(1)
            .min(centre.len() - 2);
        let t = ((s - cum[k]) / (cum[k + 1] - cum[k]).max(1e-6)).clamp(0.0, 1.0);
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

    let zone = Zone {
        kind: DistrictKind::Suburb,
        density_base: 0.0,
        polygon: vec![lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]],
    };
    let mut net = built.into_network(
        &spec.id,
        &format!("touge spec={}", spec.id),
        None,
        geom::xy(centre[0]),
        vec![zone],
        pois,
        Vec::new(),
    );
    net.bounds_min = lo;
    net.bounds_max = hi;
    net.terrain = Some(hf);
    net.decor = decor;

    let mut max_grade = 0.0f32;
    for k in 0..centre.len() - 1 {
        let run = cum[k + 1] - cum[k];
        if run > 0.5 {
            max_grade = max_grade.max((centre[k + 1][2] - centre[k][2]).abs() / run);
        }
    }
    let track = TougeTrack {
        id: spec.id.clone(),
        name: spec.name.clone(),
        length_m: length,
        climb_m: centre[centre.len() - 1][2] - centre[0][2],
        max_grade,
        half_width_m: half,
        wall_offset_m: wall_off,
        krail,
        wall_pieces: wall_pieces_sides,
        krail_sections: krails.len(),
    };
    TougeBuild { net, krails, track }
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
    fn passes_are_long_steep_and_sealed() {
        for b in all() {
            let t = &b.track;
            eprintln!(
                "{}: {:.0} m, climb {:.0} m, max grade {:.1} %, {} wall pieces, {} K-rails, sides {:?}",
                t.id,
                t.length_m,
                t.climb_m,
                t.max_grade * 100.0,
                t.wall_pieces,
                t.krail_sections,
                t.krail.iter().map(|k| k.side).collect::<Vec<_>>()
            );
            assert!(
                t.length_m > 2200.0 && t.length_m < 3600.0,
                "{}: {}",
                t.id,
                t.length_m
            );
            assert!(t.climb_m > 190.0, "{}: climb {}", t.id, t.climb_m);
            assert!(t.max_grade < 0.17, "{}: grade {}", t.id, t.max_grade);
            assert_eq!(t.krail.len(), 4, "{}", t.id);
            assert!(t.krail_sections >= 4 * 35, "{}: {}", t.id, t.krail_sections);
            // Walls cover both edges end to end except the K-rail stretches (4 m pieces).
            let krail_m: f32 = t.krail.iter().map(|k| k.to_m - k.from_m).sum();
            let expect = ((2.0 * t.length_m - krail_m) / 4.0) as usize;
            assert!(
                t.wall_pieces + 8 >= expect,
                "{}: {} walls, expected ≈ {expect}",
                t.id,
                t.wall_pieces
            );
        }
    }

    #[test]
    fn road_never_comes_close_to_itself() {
        for b in all() {
            let c = &b.net.surfaces[0].centre;
            let cum = geom::cumulative(c);
            let min_gap = 2.0 * (b.track.wall_offset_m + WALL_W_M) + 4.0;
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
    fn terrain_stays_under_the_road_and_falls_away_behind_krails() {
        for b in all() {
            let t = b.net.terrain.as_ref().unwrap();
            let s = &b.net.surfaces[0];
            for k in (0..s.centre.len()).step_by(5) {
                let p = s.centre[k];
                assert!(
                    t.height_at(geom::xy(p)) <= p[2] + 0.02,
                    "{} k={k}",
                    b.track.id
                );
            }
            let c = &s.centre;
            let cum = geom::cumulative(c);
            for span in &b.track.krail {
                let (p, d) = geom::point_at(c, &cum, 0.5 * (span.from_m + span.to_m));
                let q = geom::add(
                    geom::xy(p),
                    geom::scale(geom::right_of(d), span.side * (b.track.half_width_m + 12.0)),
                );
                assert!(
                    t.height_at(q) < p[2] - 10.0,
                    "{}: no drop behind the K-rails at {}",
                    b.track.id,
                    span.from_m
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
