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
//! - a run-off past both ends of the spec's course ([`RUN_OFF_M`] of paved road beyond each
//!   finish line, straight on and levelling out, lined like the rest of the pass), then a
//!   gravel arrester bed ([`TRAP_M`], dirt zone, flat) inside one continuous guard rail that
//!   flares out from the road's rail line, rounds the bed's end and comes back to the cut bank
//!   on the other side, with a fixed wall ([`DecorKind::Fence`] box, solid in `wp_sim`)
//!   behind it;
//! - four race-grid POIs: `<name> base start` / `<name> summit finish` (uphill) and
//!   `<name> summit start` / `<name> base finish` (downhill), at the course's ends
//!   ([`GRID_IN_M`], [`FINISH_IN_M`]); the run-offs lie beyond the finish lines.
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
/// Length of the flare where a guard rail swings out from its verge to the back of a runoff
/// apron (or back in), m along the road.
pub const RAIL_FLARE_M: f32 = 24.0;
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
/// Finish lines sit this far in from each end of the spec's course, m (the course as raced
/// ends here; the road runs on past it into the run-off).
pub const FINISH_IN_M: f32 = 40.0;
/// Paved run-off past each finish line, m: the road runs on this far beyond the line (straight
/// on, levelling out) before its gravel arrester bed, so a car finishing at 200 km/h can brake
/// to a stop on it, wet or dry (test `run_offs_stop_a_car_from_200_kmh_dry_and_wet`). Owner 2026-10-28: "touge tracks need more run off so we can
/// slow down after we finish".
pub const RUN_OFF_M: f32 = 340.0;
/// Road added before the spec's course at the base and after it at the summit, m (the paved
/// run-off less the [`FINISH_IN_M`] of the course that was already past the line).
pub const RUN_OFF_EXT_M: f32 = RUN_OFF_M - FINISH_IN_M;
/// Over this much of the run-off its grade fades from the course's end grade to level, m.
pub const RUN_OFF_FADE_M: f32 = 60.0;
/// Gravel arrester bed past each end of the road, m from the end of the paving to the back of
/// its rail (the apex of the rounded end).
pub const TRAP_M: f32 = 45.0;
/// Level ground kept outside an arrester bed's rail, m (a terrain cell and a bit: the ground
/// between the grid's vertices stays level right up to the rail).
pub const TRAP_SHELF_M: f32 = 1.2 * TERRAIN_CELL_M;
/// Gravel arrester bed: half width between the faces of its rails, m (wider than the road:
/// the rails flare out to it from the road's rail line over [`RAIL_FLARE_M`]).
pub const TRAP_HALF_W_M: f32 = 8.0;
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
    /// Road length end to end (run-offs included), m. Distances along the road (`edges`,
    /// the fields below) are from the base end of the base run-off.
    pub length_m: f32,
    /// Where the spec's course starts (its base end) along the road, m: the base run-off is
    /// the road before it.
    pub course_from_m: f32,
    /// Where the spec's course ends (its summit end), m: the summit run-off is the road after.
    pub course_to_m: f32,
    /// Finish lines along the road, m: `[downhill (base), uphill (summit)]`.
    pub finish_m: [f32; 2],
    /// Start grids (pole) along the road, m: `[uphill (base), downhill (summit)]`.
    pub start_m: [f32; 2],
    /// The arrester beds past the road's ends: `[base, summit]`.
    pub traps: [Trap; 2],
    /// Summit height above the base (the course's ends), m.
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

/// A gravel arrester bed past one end of the road (see the module docs), in plan.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Trap {
    /// The end of the paving the bed starts at, m (XY).
    pub origin: V2,
    /// Unit XY direction out of the road into the bed.
    pub dir: V2,
    /// Height of the bed's flat floor, m.
    pub floor_z: f32,
}

impl Trap {
    /// `p` in the bed's frame: `(along, lateral)`, m: along from the end of the paving
    /// (negative = back on the road), lateral positive to the right of [`Trap::dir`].
    pub fn local(&self, p: V2) -> (f32, f32) {
        let d = geom::sub(p, self.origin);
        (
            geom::dot(d, self.dir),
            geom::dot(d, geom::right_of(self.dir)),
        )
    }

    /// World XY of bed point `(along, lateral)`.
    pub fn world(&self, along: f32, lateral: f32) -> V2 {
        geom::add(
            geom::add(self.origin, geom::scale(self.dir, along)),
            geom::scale(geom::right_of(self.dir), lateral),
        )
    }

    /// How far outside the bed's rail line `p` is, m (negative inside, `None` beside the road
    /// before the bed starts). `rail0` = the rail line's offset where it leaves the road.
    pub fn outside(&self, p: V2, rail0: f32) -> Option<f32> {
        let (a, l) = self.local(p);
        if a < 0.0 {
            return None;
        }
        let (centre, r) = trap_back(rail0);
        Some(if a <= centre {
            l.abs() - trap_rail_w(a, rail0)
        } else {
            geom::dist([a, l], [centre, 0.0]) - r
        })
    }
}

/// Offset of an arrester bed's rail line `along` m into the bed: flaring out from the road's
/// rail line (`rail0`) to the bed's full width over [`RAIL_FLARE_M`].
fn trap_rail_w(along: f32, rail0: f32) -> f32 {
    let full = TRAP_HALF_W_M + 0.5 * RAIL_W_M;
    rail0 + (full - rail0) * smoothstep(0.0, RAIL_FLARE_M, along)
}

/// An arrester bed's rounded end: `(along of the centre, radius)` of its rail line.
fn trap_back(rail0: f32) -> (f32, f32) {
    let r = trap_rail_w(RAIL_FLARE_M, rail0);
    (TRAP_M - r, r)
}

/// An arrester bed's rail line in its own frame `(along, lateral)`, every ~2 m: out along the
/// `side` (+1 right of [`Trap::dir`], -1 left) from the road, round the end, back along the
/// other side to the road. The first and last points sit on the road's rail line at the end
/// of the paving.
fn trap_rail_line(side: f32, rail0: f32) -> Vec<V2> {
    let (centre, r) = trap_back(rail0);
    let mut out = Vec::new();
    let n_side = (centre / STEP_M).ceil() as usize;
    for i in 0..=n_side {
        let a = centre * i as f32 / n_side as f32;
        out.push([a, side * trap_rail_w(a, rail0)]);
    }
    let n_arc = (core::f32::consts::PI * r / STEP_M).ceil() as usize;
    for i in 1..n_arc {
        // From the `side` side (angle 0 = along +lateral·side) round through the apex.
        let t = core::f32::consts::PI * i as f32 / n_arc as f32;
        let (sn, cs) = wp_math::sin_cos(t);
        out.push([centre + r * sn, side * r * cs]);
    }
    for i in (0..=n_side).rev() {
        let a = centre * i as f32 / n_side as f32;
        out.push([a, -side * trap_rail_w(a, rail0)]);
    }
    out
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

/// The course's samples with a run-off of [`RUN_OFF_EXT_M`] added at each end: straight on
/// along the course's end direction, every [`STEP_M`], its grade fading from the course's end
/// grade to level over [`RUN_OFF_FADE_M`], unbanked. Returns `(xy, z, bank, pre)` with `pre`
/// = the index of the course's first sample.
fn with_run_offs(xy: &[V2], z: &[f32], bank: &[f32]) -> (Vec<V2>, Vec<f32>, Vec<f32>, usize) {
    let n = xy.len();
    let steps = (RUN_OFF_EXT_M / STEP_M).round() as usize;
    // Height `x` m out along a run-off leaving the course at grade `g`: the grade fades
    // linearly to 0 over the fade length, then the road is level.
    let rise = |g: f32, x: f32| -> f32 {
        let f = x.min(RUN_OFF_FADE_M);
        g * (f - f * f / (2.0 * RUN_OFF_FADE_M))
    };
    // `(end point, outward direction, outward grade)` at either end.
    let end = |a: usize, b: usize| -> (V2, V2, f32) {
        let run = geom::dist(xy[a], xy[b]).max(1e-6);
        (
            xy[a],
            geom::normalize(geom::sub(xy[a], xy[b])),
            (z[a] - z[b]) / run,
        )
    };
    let (p0, d0, g0) = end(0, 1);
    let (p1, d1, g1) = end(n - 1, n - 2);
    let mut out_xy = Vec::with_capacity(n + 2 * steps);
    let mut out_z = Vec::with_capacity(n + 2 * steps);
    for i in (1..=steps).rev() {
        let x = i as f32 * STEP_M;
        out_xy.push(geom::add(p0, geom::scale(d0, x)));
        out_z.push(z[0] + rise(g0, x));
    }
    out_xy.extend_from_slice(xy);
    out_z.extend_from_slice(z);
    for i in 1..=steps {
        let x = i as f32 * STEP_M;
        out_xy.push(geom::add(p1, geom::scale(d1, x)));
        out_z.push(z[n - 1] + rise(g1, x));
    }
    let mut out_bank = vec![0.0; steps];
    out_bank.extend_from_slice(bank);
    out_bank.resize(n + 2 * steps, 0.0);
    (out_xy, out_z, out_bank, steps)
}

/// Build pass `spec` (see the module docs).
pub fn generate(spec: &TougeSpec) -> TougeBuild {
    let seed = seed_of(&spec.id);
    // The spec's course (exactly as before the run-offs), then the run-offs added before and
    // after it.
    let (course_xy, course_kap) = turtle(&spec.legs);
    let course_z = smooth(&profile(&course_kap, spec.climb_m), 6, 3, 0);
    let course_bank = banking(&course_kap);
    let (xy, z, bank, pre) = with_run_offs(&course_xy, &course_z, &course_bank);
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
    // The spec's course along the road (distances in the spec are from its base end).
    let course_from = cum[pre];
    let course_to = cum[pre + course_xy.len() - 1];
    let half = surf.left_m.max(surf.right_m);
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

    // The gravel arrester beds past both ends of the paving, and the line of the road's guard
    // rails (the beds' rails leave the road there).
    let rail0 = half + RAIL_GAP_M + 0.5 * RAIL_W_M;
    let trap_at = |k: usize, j: usize| Trap {
        origin: geom::xy(centre[k]),
        dir: geom::normalize(geom::sub(geom::xy(centre[k]), geom::xy(centre[j]))),
        floor_z: centre[k][2] - CARVE_BELOW_M,
    };
    let traps = [trap_at(0, 1), trap_at(nc - 1, nc - 2)];

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
    let bends: Vec<(f32, f32, f32, f32)> = arcs(&spec.legs)
        .into_iter()
        .map(|(s0, s1, turn, r)| (s0 + course_from, s1 + course_from, turn, r))
        .collect();
    let outside_of_bend = |k: usize, side: f32| {
        bends.iter().any(|&(s0, s1, turn, r)| {
            r < DROP_RAIL_BEND_R_M
                && in_range(k, s0 - DROP_RAIL_MARGIN_M, s1 + DROP_RAIL_MARGIN_M)
                && turn.signum() == side
        })
    };
    for &[a, b] in &spec.drops {
        let (a, b) = (
            a.clamp(0.0, course_to - course_from) + course_from,
            b.clamp(0.0, course_to - course_from) + course_from,
        );
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
    for &(s0, s1, turn, r) in &bends {
        if r > RUNOFF_MAX_R_M && turn.abs() < RUNOFF_MIN_TURN_DEG {
            continue;
        }
        let (a, b) = (
            (s0 + 0.35 * (s1 - s0)).max(course_from + GRID_IN_M),
            (s1 + 20.0).min(course_to - GRID_IN_M),
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
            // An arrester bed: flat inside its rail and a terrain cell past it (so the ground
            // between the vertices stays level right up to the rail), then back to the ground
            // over 4 m.
            for t in &traps {
                if let Some(e) = t.outside(p, rail0) {
                    let w = smoothstep(TRAP_SHELF_M, TRAP_SHELF_M + 4.0, e);
                    zt = t.floor_z + (zt - t.floor_z) * w;
                }
            }
            hf.z[(j * hf.nx + i) as usize] = zt;
        }
    }

    // End walls across the back of each arrester bed, just behind its rail.
    let mut decor = Vec::new();
    for t in &traps {
        let (_, r) = trap_back(rail0);
        let p = t.world(TRAP_M + 0.5 * RAIL_W_M + WALL_GAP_M + 0.5 * WALL_W_M, 0.0);
        let width = 2.0 * (r + RAIL_W_M + WALL_W_M);
        decor.push(Decor {
            kind: DecorKind::Fence,
            pos: [p[0], p[1], t.floor_z + CARVE_BELOW_M - 0.05 - 0.06 * half],
            yaw: geom::yaw_of(geom::right_of(t.dir)),
            size: [WALL_W_M, width, WALL_H_M + 0.15 * half],
        });
    }

    // Guard rails along every downhill rail run, and at the back of downhill runoff aprons,
    // as one continuous line per stretch: where a rail run meets a runoff apron the rail
    // flares out to the apron's back (and back in) over [`RAIL_FLARE_M`] instead of stopping.
    // Sections follow the rail's own line (longer than the centreline outside a bend), share
    // their end points with their neighbours (in plan and in height, pitched to the grade),
    // and stand on the ground; the beam (local −X) faces the road.
    // Each arrester bed's rail carries on from the road's rail line at that end (round the
    // bed and back to the cut bank on the other side).
    let mut rails = Vec::new();
    let flare = (0.5 * RAIL_FLARE_M / STEP_M).round() as usize;
    // `(side, foot line, first sample, one past the last sample)` per stretch along the road.
    let mut stretches: Vec<(f32, Vec<V3>, usize, usize)> = Vec::new();
    for side in [-1.0f32, 1.0] {
        let lat_of = |k: usize| match edge_of(k, side) {
            EdgeKind::Rail => Some(half + RAIL_GAP_M + 0.5 * RAIL_W_M),
            EdgeKind::Runoff if low[k] == side => Some(half + RUNOFF_W_M - 0.5 * RAIL_W_M),
            _ => None,
        };
        let mut k = 0;
        while k < nc {
            if lat_of(k).is_none() {
                k += 1;
                continue;
            }
            let mut j = k;
            while j < nc && lat_of(j).is_some() {
                j += 1;
            }
            // Offset at each sample: the mean over ±`flare` samples within the run, so a step
            // between the verge and the apron becomes a straight flare.
            let line: Vec<V3> = (k..j)
                .map(|q| {
                    let (a, b) = (q.saturating_sub(flare).max(k), (q + flare).min(j - 1));
                    let lat = (a..=b).filter_map(lat_of).sum::<f32>() / (b - a + 1) as f32;
                    let r = geom::right_of(seg_dir(&centre, q));
                    let p = geom::add(geom::xy(centre[q]), geom::scale(r, side * lat));
                    let foot = rail_foot_z(
                        surf.z_at(q, side * half),
                        hf.height_on_triangles(p),
                        lat - half,
                    );
                    [p[0], p[1], foot]
                })
                .collect();
            stretches.push((side, line, k, j));
            k = j;
        }
    }
    for (end, t) in traps.iter().enumerate() {
        let (k_end, sign) = if end == 0 { (0, -1.0) } else { (nc - 1, 1.0) };
        let touches = |st: &(f32, Vec<V3>, usize, usize)| {
            if end == 0 {
                st.2 == 0
            } else {
                st.3 == nc
            }
        };
        let i = match stretches.iter().position(touches) {
            Some(i) => i,
            None => {
                stretches.push((low[k_end], Vec::new(), k_end, k_end + 1));
                stretches.len() - 1
            }
        };
        let side = stretches[i].0;
        // The bed's frame runs out of the road: the road's right is its right at the summit,
        // its left at the base.
        let mut u: Vec<V3> = trap_rail_line(side * sign, rail0)
            .into_iter()
            .map(|[a, l]| {
                let q = t.world(a, l);
                [q[0], q[1], t.floor_z]
            })
            .collect();
        let line = &mut stretches[i].1;
        if end == 0 {
            // Prepended: from the far side round to this one, the road's rail line next.
            u.reverse();
            if !line.is_empty() {
                u.pop();
            }
            u.append(line);
            *line = u;
        } else {
            let skip = usize::from(!line.is_empty());
            line.extend_from_slice(&u[skip..]);
        }
    }
    for (side, line, _, _) in stretches {
        {
            if line.len() < 2 {
                continue;
            }
            // Where the foot stands on the ground, average out the terrain's small bumps over
            // ±4 m so neighbouring sections don't kink up and down.
            let line: Vec<V3> = (0..line.len())
                .map(|i| {
                    let (a, b) = (i.saturating_sub(2), (i + 2).min(line.len() - 1));
                    let z = line[a..=b].iter().map(|p| p[2]).sum::<f32>() / (b - a + 1) as f32;
                    [line[i][0], line[i][1], z]
                })
                .collect();
            let lcum = geom::cumulative(&line);
            let run = *lcum.last().unwrap_or(&0.0);
            if run < 0.5 * RAIL_LEN_M {
                continue;
            }
            let count = (run / RAIL_LEN_M).round().max(1.0) as usize;
            let pitch = run / count as f32;
            let ends: Vec<V3> = (0..=count)
                .map(|c| geom::point_at(&line, &lcum, c as f32 * pitch).0)
                .collect();
            for c in 0..count {
                let (pa, pb) = (ends[c], ends[c + 1]);
                // Forward (local +Y) runs with the road on the right side, against it on the
                // left, so the beam (local −X) faces the road on both.
                let (from, to) = if side > 0.0 { (pa, pb) } else { (pb, pa) };
                let flat = geom::dist(geom::xy(from), geom::xy(to)).max(0.1);
                let rise = to[2] - from[2];
                let len = (flat * flat + rise * rise).sqrt();
                let facing = geom::scale(geom::sub(geom::xy(to), geom::xy(from)), 1.0 / flat);
                // Each end reaches past the joint by half the beam's mitre gap (its face is
                // ~0.2 m off the line, so a bend opens a wedge there), plus 2 cm.
                let turn_at = |e: usize| {
                    if e == 0 || e == count {
                        return 0.0;
                    }
                    let d0 = geom::normalize(geom::sub(geom::xy(ends[e]), geom::xy(ends[e - 1])));
                    let d1 = geom::normalize(geom::sub(geom::xy(ends[e + 1]), geom::xy(ends[e])));
                    let cross = d0[0] * d1[1] - d0[1] * d1[0];
                    let dot = d0[0] * d1[0] + d0[1] * d1[1];
                    wp_math::atan2(cross, dot).abs()
                };
                let mitre = 0.25 * RAIL_W_M * turn_at(c).max(turn_at(c + 1));
                let pitch_rad = wp_math::atan2(rise, flat);
                let (sp, cp) = wp_math::sin_cos(pitch_rad);
                // Box centre: the middle of the foot line, raised half the rail's height along
                // its (pitched) up axis.
                let up = [-sp * facing[0], -sp * facing[1], cp];
                let mid = [
                    0.5 * (pa[0] + pb[0]),
                    0.5 * (pa[1] + pb[1]),
                    0.5 * (pa[2] + pb[2]),
                ];
                rails.push(StaticProp {
                    kind: PropKind::GuardRail,
                    pos: [
                        mid[0] + up[0] * 0.5 * RAIL_H_M,
                        mid[1] + up[1] * 0.5 * RAIL_H_M,
                        mid[2] + up[2] * 0.5 * RAIL_H_M,
                    ],
                    yaw: geom::yaw_of(facing),
                    pitch: pitch_rad,
                    half_extents: [RAIL_W_M * 0.5, len * 0.5 + 0.02 + mitre, RAIL_H_M * 0.5],
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
    // The arrester beds: gravel out to their rails.
    for t in &traps {
        let polygon: Vec<V2> = trap_rail_line(1.0, rail0)
            .into_iter()
            .map(|[a, l]| t.world(a, l))
            .collect();
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
            if traps
                .iter()
                .any(|t| t.outside(q, rail0).is_some_and(|e| e < 4.0))
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
    let start_m = [course_from + GRID_IN_M, course_to - GRID_IN_M];
    let finish_m = [course_from + FINISH_IN_M, course_to - FINISH_IN_M];
    let pois = vec![
        poi(
            format!("{} base start", spec.name),
            start_m[0],
            true,
            up_lane,
        ),
        poi(
            format!("{} summit finish", spec.name),
            finish_m[1],
            true,
            up_lane,
        ),
        poi(
            format!("{} summit start", spec.name),
            start_m[1],
            false,
            down_lane,
        ),
        poi(
            format!("{} base finish", spec.name),
            finish_m[0],
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
        course_from_m: course_from,
        course_to_m: course_to,
        finish_m,
        start_m,
        traps,
        climb_m: centre[pre + course_xy.len() - 1][2] - centre[pre][2],
        max_grade,
        half_width_m: half,
        edges,
        rail_sections: rails.len(),
        trees,
    };
    TougeBuild { net, rails, track }
}

/// Height of a guard rail's foot `out` m beyond the paved edge (height `edge`), over terrain
/// height `ground`: on the verge it follows the paved edge's line (less [`CARVE_BELOW_M`]),
/// smooth along the road where the terrain triangles dip into the carve beside the paving;
/// further out (a flare towards a runoff apron's back, the apron itself) it stands on the
/// ground.
fn rail_foot_z(edge: f32, ground: f32, out: f32) -> f32 {
    let line = edge - CARVE_BELOW_M;
    let w = smoothstep(1.5, RAIL_VERGE_M + 0.5, out);
    line + (ground - line) * w
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

    /// Ends of rail section `r`'s foot line (its box's bottom edge along the middle), m.
    fn rail_ends(r: &StaticProp) -> (V3, V3) {
        let (sy, cy) = wp_math::sin_cos(r.yaw);
        let (sp, cp) = wp_math::sin_cos(r.pitch);
        let fwd = [-sy * cp, cy * cp, sp];
        let up = [sy * sp, -cy * sp, cp];
        let h = r.half_extents[1];
        let foot = |k: f32| -> V3 {
            [
                r.pos[0] - up[0] * r.half_extents[2] + fwd[0] * h * k,
                r.pos[1] - up[1] * r.half_extents[2] + fwd[1] * h * k,
                r.pos[2] - up[2] * r.half_extents[2] + fwd[2] * h * k,
            ]
        };
        (foot(-1.0), foot(1.0))
    }

    #[test]
    fn guard_rail_sections_join_end_to_end() {
        // Owner 2026-10-26: "the touge guardrails need to be connected together properly".
        // Consecutive sections of a stretch meet (in plan and in height, on every grade);
        // where a rail run meets a runoff apron the rail flares out to it instead of stopping
        // (that left 7.6 m gaps); a stretch only ends where the rail does (a bank, a drop,
        // the high side).
        for b in all() {
            let id = &b.track.id;
            let mut stretches = 1;
            for w in b.rails.windows(2) {
                let (a0, a1) = rail_ends(&w[0]);
                let (b0, b1) = rail_ends(&w[1]);
                let d3 = |p: V3, q: V3| {
                    let d = [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
                    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
                };
                let gap = [d3(a0, b0), d3(a0, b1), d3(a1, b0), d3(a1, b1)]
                    .into_iter()
                    .fold(f32::INFINITY, f32::min);
                if gap > 20.0 {
                    stretches += 1;
                    continue;
                }
                // The joint: each end reaches 2 cm (plus its mitre) past it.
                assert!(
                    gap < 0.15,
                    "{id}: rail sections at {:?} and {:?} are {gap:.2} m apart",
                    w[0].pos,
                    w[1].pos
                );
                // Neighbours line up: the beam doesn't kink by more than a bend's worth.
                let dyaw = (w[0].yaw - w[1].yaw).rem_euclid(core::f32::consts::TAU);
                let dyaw = dyaw.min(core::f32::consts::TAU - dyaw);
                assert!(dyaw < 0.6, "{id}: kink {dyaw:.2} rad at {:?}", w[1].pos);
                assert!(
                    (w[0].pitch - w[1].pitch).abs() < 0.12,
                    "{id}: pitch step at {:?}",
                    w[1].pos
                );
            }
            // Each stretch is a run of rail, runoff and flare between banks and drops.
            let ends = b
                .track
                .edges
                .iter()
                .filter(|e| matches!(e.kind, EdgeKind::Bank | EdgeKind::Drop))
                .count();
            assert!(
                stretches <= ends + 2,
                "{id}: {stretches} stretches, {ends} bank/drop runs"
            );
            // On the pass's steepest grades the sections are pitched with it.
            let steepest = b.rails.iter().map(|r| r.pitch.abs()).fold(0.0, f32::max);
            assert!(steepest > 0.06, "{id}: steepest rail pitch {steepest:.3}");
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
            let course = t.course_to_m - t.course_from_m;
            assert!(course > 2200.0 && course < 3600.0, "{}: {course}", t.id);
            assert!(
                (t.length_m - course - 2.0 * RUN_OFF_EXT_M).abs() < 1.0,
                "{}: {} m of road for a {course} m course",
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
            // The runoff aprons, drops and both arrester beds read as dirt.
            let dirt = b
                .net
                .zones
                .iter()
                .filter(|z| z.kind == DistrictKind::Industrial)
                .count();
            assert_eq!(
                dirt,
                t.spans(EdgeKind::Runoff).count() + t.spans(EdgeKind::Drop).count() + 2,
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
                edge_at(&b, b.track.course_from_m + 0.5 * (s0 + s1) + 5.0, outside),
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
                let (s0, s1) = (s0 + b.track.course_from_m, s1 + b.track.course_from_m);
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
            let len = *cum.last().unwrap();
            let rail0 = half + RAIL_GAP_M + 0.5 * RAIL_W_M;
            for r in &b.rails {
                assert_eq!(r.kind, PropKind::GuardRail);
                assert_eq!(r.mass_kg, RAIL_MASS_KG);
                let p = geom::project(c, &cum, geom::xy(r.pos));
                let x = geom::right_of(geom::dir_of_yaw(r.yaw));
                if p.s < 0.0 || p.s > len {
                    // Round an arrester bed: on its rail line, the beam towards its inside
                    // (the bed's axis, or the centre of its rounded end).
                    let t = &b.track.traps[usize::from(p.s > len)];
                    let q = geom::xy(r.pos);
                    let e = t.outside(q, rail0).unwrap();
                    // Sections are chords of the rounded end: their middles sit inside the arc.
                    assert!(e.abs() < 0.5, "{}: bed rail {e} m off its line", b.track.id);
                    let (a, _) = t.local(q);
                    let (centre, _) = trap_back(rail0);
                    let inside = t.world(a.clamp(0.0, centre), 0.0);
                    let to_inside = geom::normalize(geom::sub(inside, q));
                    assert!(
                        geom::dot(x, to_inside) < -0.9,
                        "{}: bed rail faces away",
                        b.track.id
                    );
                    continue;
                }
                let out = p.dist - half;
                assert!(
                    (RAIL_GAP_M..=RUNOFF_W_M + 0.1).contains(&out),
                    "{}: rail {out} m beyond the edge",
                    b.track.id
                );
                // Local −X (the beam) towards the road.
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
    fn the_course_as_raced_is_the_spec_course() {
        // The run-offs are added outside the spec's course: its road, start grids and finish
        // lines are where they were, so race lengths, times and AI pacing do not move.
        for b in all() {
            let t = &b.track;
            let spec = spec(&t.id).unwrap();
            let (xy, _) = turtle(&spec.legs);
            let c = &b.net.surfaces[0].centre;
            let cum = geom::cumulative(c);
            let pre = cum.partition_point(|s| *s < t.course_from_m - 0.01);
            assert_eq!(pre, (RUN_OFF_EXT_M / STEP_M).round() as usize, "{}", t.id);
            for (i, q) in xy.iter().enumerate() {
                assert!(
                    geom::dist(geom::xy(c[pre + i]), *q) < 1e-3,
                    "{} i={i}",
                    t.id
                );
            }
            let course = t.course_to_m - t.course_from_m;
            assert!((course - arcs_len(&spec.legs)).abs() < 0.05, "{}", t.id);
            assert_eq!(t.finish_m[0] - t.course_from_m, FINISH_IN_M);
            assert!((t.course_to_m - t.finish_m[1] - FINISH_IN_M).abs() < 1e-3);
            assert_eq!(t.start_m[0] - t.course_from_m, GRID_IN_M);
            assert!((t.course_to_m - t.start_m[1] - GRID_IN_M).abs() < 1e-3);
            // The POIs stand on those distances.
            for (name, s) in [
                ("base finish", t.finish_m[0]),
                ("summit finish", t.finish_m[1]),
                ("base start", t.start_m[0]),
                ("summit start", t.start_m[1]),
            ] {
                let poi = b.net.pois.iter().find(|p| p.name.ends_with(name)).unwrap();
                let pr = geom::project(c, &cum, geom::xy(poi.pos));
                assert!((pr.s - s).abs() < 0.05, "{} {name}: {} vs {s}", t.id, pr.s);
            }
        }
    }

    /// Length of the turtle path of `legs` (straights plus arc chords, as [`turtle`] samples).
    fn arcs_len(legs: &[Leg]) -> f32 {
        let (xy, _) = turtle(legs);
        xy.windows(2).map(|w| geom::dist(w[0], w[1])).sum()
    }

    /// The paved run-off past one finish line: centreline samples `(s, z)` from the line to the
    /// end of the paving, in the direction of travel.
    fn run_off(b: &TougeBuild, uphill: bool) -> Vec<(f32, f32)> {
        let c = &b.net.surfaces[0].centre;
        let cum = geom::cumulative(c);
        let t = &b.track;
        let mut out: Vec<(f32, f32)> = if uphill {
            let f = t.finish_m[1];
            let mut v = vec![(f, geom::point_at(c, &cum, f).0[2])];
            v.extend(
                cum.iter()
                    .zip(c)
                    .filter(|(s, _)| **s > f)
                    .map(|(s, p)| (*s, p[2])),
            );
            v
        } else {
            let f = t.finish_m[0];
            let mut v = vec![(f, geom::point_at(c, &cum, f).0[2])];
            v.extend(
                cum.iter()
                    .zip(c)
                    .filter(|(s, _)| **s < f)
                    .rev()
                    .map(|(s, p)| (*s, p[2])),
            );
            v
        };
        // Distances from the line.
        let s0 = out[0].0;
        for p in &mut out {
            p.0 = (p.0 - s0).abs();
        }
        out
    }

    /// Distance to brake to a stop from `v` m/s on run-off `pts` (`(distance, z)` along it) with
    /// tyre-road friction `mu` after `reaction_s` at speed, the grade helping uphill and
    /// hindering downhill. `None` if the car is still moving at the end of the paving.
    fn stopping_distance(pts: &[(f32, f32)], v: f32, mu: f32, reaction_s: f32) -> Option<f32> {
        let g = 9.81;
        let react = v * reaction_s;
        let mut v2 = v * v;
        for w in pts.windows(2) {
            let (s0, z0) = w[0];
            let (s1, z1) = w[1];
            if s1 <= react {
                continue;
            }
            let ds = s1 - s0.max(react);
            let run = (s1 - s0).max(1e-6);
            let sin = (z1 - z0) / run;
            let cos = (1.0 - sin * sin).max(0.0).sqrt();
            let a = g * (mu * cos + sin);
            if v2 <= 2.0 * a * ds {
                return Some(s0.max(react) + v2 / (2.0 * a));
            }
            v2 -= 2.0 * a * ds;
        }
        None
    }

    /// Finish speed the run-offs are sized for: 200 km/h, m/s.
    const FINISH_SPEED_MPS: f32 = 200.0 / 3.6;
    /// Dry tyre on the pass's road, friction coefficient.
    const MU_DRY: f32 = 0.9;
    /// Fully wet road (touge battles run in heavy rain): ~0.6 of the dry grip (the street /
    /// sport / all-terrain tyres' `WetAsphalt` ratios are 0.62–0.72), friction coefficient.
    const MU_WET: f32 = 0.55;
    /// Driver reaction at the line before the brakes are full on, s.
    const REACTION_S: f32 = 0.5;

    #[test]
    fn run_offs_stop_a_car_from_200_kmh_dry_and_wet() {
        // Owner 2026-10-28: "touge tracks need more run off so we can slow down after we
        // finish". Both directions of every pass: from 200 km/h at the finish line, after
        // half a second at speed, a car brakes to a stop on the paved run-off, dry (μ 0.9) or
        // wet (μ 0.55), with the gravel arrester bed beyond as a last resort.
        for b in all() {
            for uphill in [true, false] {
                let id = &b.track.id;
                let pts = run_off(&b, uphill);
                let paved = pts.last().unwrap().0;
                let grade = (pts.last().unwrap().1 - pts[0].1) / paved;
                let dry = stopping_distance(&pts, FINISH_SPEED_MPS, MU_DRY, REACTION_S);
                let wet = stopping_distance(&pts, FINISH_SPEED_MPS, MU_WET, REACTION_S);
                eprintln!(
                    "{id} {}: run-off {paved:.0} m paved + {TRAP_M} m bed, mean grade {:+.2} %, \
                     stops from 200 km/h in {:.0} m dry / {:.0} m wet",
                    if uphill { "uphill" } else { "downhill" },
                    grade * 100.0,
                    dry.unwrap_or(f32::NAN),
                    wet.unwrap_or(f32::NAN),
                );
                assert!(paved >= RUN_OFF_M - 0.5, "{id}: {paved} m of run-off");
                let dry = dry.unwrap_or_else(|| panic!("{id} uphill={uphill}: no dry stop"));
                let wet = wet.unwrap_or_else(|| panic!("{id} uphill={uphill}: no wet stop"));
                assert!(dry < 0.75 * paved, "{id} uphill={uphill}: dry {dry} m");
                assert!(wet < paved - 10.0, "{id} uphill={uphill}: wet {wet} m");
            }
        }
    }

    #[test]
    fn run_offs_are_sealed_on_both_sides_and_end_in_a_railed_gravel_bed() {
        for b in all() {
            let id = b.track.id.clone();
            let t = b.net.terrain.as_ref().unwrap();
            let half = b.track.half_width_m;
            let rail0 = half + RAIL_GAP_M + 0.5 * RAIL_W_M;
            let near_rail = |q: V2| {
                b.rails
                    .iter()
                    .map(|r| geom::dist(geom::xy(r.pos), q))
                    .fold(f32::INFINITY, f32::min)
            };
            let c = &b.net.surfaces[0].centre;
            let cum = geom::cumulative(c);
            for (end, (from, to)) in [
                (0.0, b.track.finish_m[0]),
                (b.track.finish_m[1], b.track.length_m),
            ]
            .into_iter()
            .enumerate()
            {
                // Along the paved run-off: on each side a guard rail (the downhill side) or
                // the cut bank rising beside the road (uphill), never an open drop or an
                // apron; and nothing falls away inside the rail.
                let mut s = from + 1.0;
                while s < to - 1.0 {
                    for side in [-1.0f32, 1.0] {
                        match edge_at(&b, s, side) {
                            EdgeKind::Rail => {
                                let (q, _) = beside(&b, s, side, rail0 - half);
                                let d = near_rail(q);
                                assert!(d < 2.6, "{id} s={s} side={side}: no rail ({d} m)");
                                let (v, z) = beside(&b, s, side, 0.2);
                                assert!(t.height_at(v) > z - 0.6, "{id} s={s}: verge falls away");
                            }
                            EdgeKind::Bank => {
                                let (q, z) = beside(&b, s, side, BANK_VERGE_M + 3.0);
                                assert!(
                                    t.height_at(q) > z + 1.0,
                                    "{id} s={s} side={side}: bank only {} m up",
                                    t.height_at(q) - z
                                );
                            }
                            k => panic!("{id} s={s} side={side}: {k:?} in the run-off"),
                        }
                    }
                    s += 2.0;
                }
                // The bed: flat gravel inside one rail that leaves the road's rail line,
                // rounds the end and comes back to the road on the other side, a fixed wall
                // behind it, no trees in it.
                let trap = &b.track.traps[end];
                let (p_end, _) = geom::point_at(c, &cum, if end == 0 { 0.0 } else { to });
                assert!(geom::dist(trap.origin, geom::xy(p_end)) < 1e-3, "{id}");
                for [a, l] in trap_rail_line(1.0, rail0) {
                    let q = trap.world(a, l);
                    let d = near_rail(q);
                    assert!(d < 2.6, "{id}: bed {end} has no rail at ({a}, {l}): {d} m");
                }
                // (From a terrain cell past the paving: the ground under the paving is carved.)
                let mut a = TERRAIN_CELL_M + 0.5;
                while a < TRAP_M - 1.0 {
                    let mut l = -TRAP_HALF_W_M;
                    while l <= TRAP_HALF_W_M {
                        let q = trap.world(a, l);
                        if trap.outside(q, rail0).unwrap() < -0.5 {
                            let dz = t.height_at(q) - trap.floor_z;
                            assert!(dz.abs() < 0.05, "{id}: bed {end} at ({a}, {l}): {dz}");
                            assert!(
                                b.net
                                    .zones
                                    .iter()
                                    .find(|z| geom::point_in_polygon(q, &z.polygon))
                                    .is_some_and(|z| z.kind == DistrictKind::Industrial),
                                "{id}: bed {end} at ({a}, {l}) is not gravel"
                            );
                        }
                        l += 1.0;
                    }
                    a += 1.5;
                }
                let behind = trap.world(TRAP_M + 0.5, 0.0);
                assert!(
                    b.net.decor.iter().any(|d| d.kind == DecorKind::Fence
                        && geom::dist(geom::xy(d.pos), behind) < 1.0
                        && d.size[1] > 2.0 * TRAP_HALF_W_M),
                    "{id}: no wall behind bed {end}"
                );
                assert!(
                    !b.net.decor.iter().any(|d| d.kind == DecorKind::PineTree
                        && trap
                            .outside(geom::xy(d.pos), rail0)
                            .is_some_and(|e| e < 3.0)),
                    "{id}: a tree in bed {end}"
                );
            }
            // No other wall across the road: the run-offs are open to their beds.
            assert_eq!(
                b.net
                    .decor
                    .iter()
                    .filter(|d| d.kind == DecorKind::Fence)
                    .count(),
                2,
                "{id}"
            );
        }
    }

    #[test]
    fn generation_is_deterministic() {
        let a = generate_builtin(TOUGE_IDS[0]).unwrap();
        let b = generate_builtin(TOUGE_IDS[0]).unwrap();
        assert_eq!(a, b);
    }
}
