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
//! - **Loop 9**: a divided highway loop with banked sweepers: two one-way 3-lane
//!   carriageways, shoulders and a concrete median barrier, reached only through six
//!   interchanges (five diamonds whose crossroad bridges the highway, a half diamond at the
//!   port; see `divide_loop`). **Route 40 West** passes under it (the loop rises on an
//!   overpass) and **Route 40 North** dips under it (underpass). Its north leg runs through
//!   the **Hollins Tunnel** under Hollins Heights' northern shoulder.
//! - **Vesper River** runs north to south between Larkspur and downtown into the sea, crossed
//!   by eight bridges (among them the steel-truss **Iron Bridge**); the level **Calder Canal**
//!   branches west through Canal Flats to a turning basin under three more.
//! - **Landmarks** (Lantern Tower, Vesper Point Lighthouse, Hollins Observatory, Calder Mill
//!   Chimney, the Gantry Cranes, Larkspur Water Tower) and **hideouts** (the tunnel and five
//!   canopies that hide a car from the helicopter), all in [`CityFeatures`].
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

use crate::builder::{self, BuildOptions, Built, ControlHint, NodeSpec, RoadClass, RoadSpec};
use crate::covered::{CoverShape, DECK_OVERHANG_M, ROOF_M, WALL_M};
use crate::density::default_density_base;
use crate::geom::{self, Obb, V2, V3};
use crate::network::*;
use crate::residential;
use crate::surface::{self, is_elevated, KERB_H_M, LOT_W_M, SIDEWALK_W_M};
use crate::terrain::{bump, smoothstep, Heightfield, ValueNoise};

pub mod ashpeak;
mod crossovers;
mod divided;

use divided::*;

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

/// Chance that a local block edge of a residential grid gets a court (row 95).
const COURT_CHANCE: f32 = 0.45;
/// Shortest block edge that gets a court, m.
const COURT_MIN_EDGE_M: f32 = 150.0;
/// Court length (± 15 %), m.
const COURT_LEN_M: f32 = 75.0;
/// Clearance of a court from every other road beyond its first 25 m, m (room for a house lot
/// on both sides of each).
const COURT_CLEARANCE_M: f32 = 40.0;

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

/// Water surface of the sea and every waterway mouth, m.
const SEA: f32 = SEA_LEVEL_M;

/// A river or canal channel in the natural ground.
struct Channel {
    name: &'static str,
    kind: WaterwayKind,
    /// Centreline, source to mouth, about every 10 m.
    pts: Vec<V2>,
    /// Water surface per point, m.
    zw: Vec<f32>,
    /// Water half width per point, m.
    half: Vec<f32>,
    /// Bank width beyond the water's edge to natural ground, m.
    bank: f32,
    depth: f32,
    lo: V2,
    hi: V2,
}

impl Channel {
    fn new(
        name: &'static str,
        kind: WaterwayKind,
        pts: Vec<V2>,
        zw: Vec<f32>,
        half: Vec<f32>,
        bank: f32,
        depth: f32,
    ) -> Self {
        let reach = half.iter().copied().fold(0.0f32, f32::max) + bank;
        let mut lo = [f32::INFINITY; 2];
        let mut hi = [f32::NEG_INFINITY; 2];
        for p in &pts {
            lo = [lo[0].min(p[0] - reach), lo[1].min(p[1] - reach)];
            hi = [hi[0].max(p[0] + reach), hi[1].max(p[1] + reach)];
        }
        Self {
            name,
            kind,
            pts,
            zw,
            half,
            bank,
            depth,
            lo,
            hi,
        }
    }

    /// Nearest centreline point: (distance, water surface, half width), or `None` beyond
    /// the bounding box. Coarse search over every 6th segment, then refined around it.
    fn near(&self, p: V2) -> Option<(f32, f32, f32)> {
        if p[0] < self.lo[0] || p[0] > self.hi[0] || p[1] < self.lo[1] || p[1] > self.hi[1] {
            return None;
        }
        let n = self.pts.len();
        let seg = |i: usize| -> (f32, f32) {
            let (a, b) = (self.pts[i], self.pts[i + 1]);
            let ab = geom::sub(b, a);
            let l2 = geom::dot(ab, ab).max(1e-6);
            let t = (geom::dot(geom::sub(p, a), ab) / l2).clamp(0.0, 1.0);
            (geom::dist(p, geom::add(a, geom::scale(ab, t))), t)
        };
        let mut best = (f32::INFINITY, 0usize, 0.0f32);
        let mut i = 0;
        while i + 1 < n {
            let (d, t) = seg(i);
            if d < best.0 {
                best = (d, i, t);
            }
            i += 6;
        }
        let (a, b) = (best.1.saturating_sub(8), (best.1 + 8).min(n - 2));
        for i in a..=b {
            let (d, t) = seg(i);
            if d < best.0 {
                best = (d, i, t);
            }
        }
        let (d, i, t) = best;
        let zw = self.zw[i] + (self.zw[i + 1] - self.zw[i]) * t;
        let half = self.half[i] + (self.half[i + 1] - self.half[i]) * t;
        Some((d, zw, half))
    }

    /// Ground carved into the channel at `p` (`land` beyond the banks).
    fn carve(&self, p: V2, land: f32) -> f32 {
        let Some((d, zw, half)) = self.near(p) else {
            return land;
        };
        if d >= half + self.bank {
            return land;
        }
        let bed = zw - self.depth;
        // A flat bed, then a bank rising through the water's edge to natural ground.
        let t = smoothstep(half - 0.5 * self.depth - 3.0, half + self.bank, d);
        land.min(bed + (land - bed) * t)
    }

    /// Whether `p` is in the channel (water or bank), with `margin` m extra.
    fn covers(&self, p: V2, margin: f32) -> bool {
        self.near(p)
            .is_some_and(|(d, _, half)| d < half + self.bank + margin)
    }

    fn waterway(&self) -> Waterway {
        Waterway {
            name: self.name.into(),
            kind: self.kind,
            centre: self
                .pts
                .iter()
                .zip(&self.zw)
                .map(|(p, z)| [p[0], p[1], *z])
                .collect(),
            half_width_m: self.half.clone(),
            depth_m: self.depth,
        }
    }
}

/// The natural (uncarved) ground: hills, the sea, and the Vesper River and Calder Canal
/// channels.
struct Natural {
    n1: ValueNoise,
    n2: ValueNoise,
    /// A region generated together with Port Vesper ([`Region`]): its ground from the point
    /// and Port Vesper's own ground there.
    ext: Option<Box<dyn Fn(V2, f32) -> f32>>,
    channels: Vec<Channel>,
}

impl Natural {
    fn new(rng: &mut Rng) -> Self {
        let mut nat = Self {
            n1: ValueNoise::new(rng, MAP_LO, MAP_HI, 420.0),
            n2: ValueNoise::new(rng, MAP_LO, MAP_HI, 150.0),
            ext: None,
            channels: Vec::new(),
        };
        nat.channels = vec![nat.river(), nat.canal()];
        nat
    }

    fn z(&self, p: V2) -> f32 {
        let own = self.vesper_z(p);
        match &self.ext {
            Some(f) => f(p, own),
            None => own,
        }
    }

    /// The Vesper River: north edge to the sea, between Larkspur and downtown. Its surface
    /// follows the ground 3 m down, never rising towards the mouth.
    fn river(&self) -> Channel {
        let pts = resample(&catmull_rom(&RIVER, 10.0), 10.0);
        let n = pts.len();
        let raw: Vec<f32> = pts.iter().map(|p| self.land(*p) - 3.0).collect();
        let smooth = smooth_series(&raw, 8);
        let mut zw = Vec::with_capacity(n);
        let mut lowest = f32::INFINITY;
        for (k, z) in smooth.iter().enumerate() {
            lowest = lowest.min(*z).min(raw[k] + 0.5);
            zw.push(lowest.max(SEA));
        }
        let half: Vec<f32> = (0..n)
            .map(|k| 16.0 + 14.0 * k as f32 / (n - 1) as f32)
            .collect();
        Channel::new(
            "Vesper River",
            WaterwayKind::River,
            pts,
            zw,
            half,
            12.0,
            2.5,
        )
    }

    /// The Calder Canal: a level cut west from the river through Canal Flats, ending in a
    /// turning basin.
    fn canal(&self) -> Channel {
        let pts = resample(&catmull_rom(&CANAL, 10.0), 10.0);
        let cum = cum2(&pts);
        let total = cum[cum.len() - 1];
        let half: Vec<f32> = cum
            .iter()
            .map(|s| 10.0 + 20.0 * smoothstep(total - 90.0, total - 20.0, *s))
            .collect();
        let zw = vec![SEA; pts.len()];
        Channel::new("Calder Canal", WaterwayKind::Canal, pts, zw, half, 4.0, 3.0)
    }

    /// Ground before the waterways are cut.
    fn land(&self, p: V2) -> f32 {
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

    /// Port Vesper's own ground (inside its map; meaningless far outside): the land with the
    /// river and canal channels cut into it.
    fn vesper_z(&self, p: V2) -> f32 {
        let land = self.land(p);
        self.channels
            .iter()
            .fold(land, |z, c| z.min(c.carve(p, land)))
    }

    /// Whether `p` is in a river or canal channel (water or bank), with `margin` m extra.
    fn wet(&self, p: V2, margin: f32) -> bool {
        self.channels.iter().any(|c| c.covers(p, margin))
    }
}

/// Vesper River control points, source (north edge) to mouth (into the sea).
const RIVER: [V2; 11] = [
    [-560.0, 1800.0],
    [-525.0, 1500.0],
    [-495.0, 1200.0],
    [-512.0, 950.0],
    [-500.0, 650.0],
    [-515.0, 350.0],
    [-495.0, 50.0],
    [-508.0, -250.0],
    [-500.0, -600.0],
    [-505.0, -950.0],
    [-505.0, -1250.0],
];

/// Calder Canal control points, from the river west to its basin.
const CANAL: [V2; 4] = [
    [-505.0, -822.0],
    [-800.0, -815.0],
    [-1050.0, -824.0],
    [-1240.0, -820.0],
];

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
    /// Paving.
    material: RoadMaterial,
}

const HIGHWAY: Style = Style {
    class: RoadClass::Highway,
    fwd: 3,
    bwd: 3,
    speed: 24.586_72, // 55 mph
    parking: false,
    grade: 0.06,
    bank_max: 0.06,
    material: RoadMaterial::Asphalt,
};
const ARTERIAL: Style = Style {
    class: RoadClass::Arterial,
    fwd: 2,
    bwd: 2,
    speed: 17.8816, // 40 mph
    parking: false,
    grade: 0.08,
    bank_max: 0.02,
    material: RoadMaterial::Asphalt,
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
    material: RoadMaterial::Asphalt,
};
const LOCAL: Style = Style {
    class: RoadClass::Local,
    fwd: 1,
    bwd: 1,
    speed: 11.176, // 25 mph
    parking: true,
    grade: 0.10,
    bank_max: 0.0,
    material: RoadMaterial::Asphalt,
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
    material: RoadMaterial::Asphalt,
};

/// Loop 9 ramps: one-way, one lane.
const RAMP: Style = Style {
    class: RoadClass::Highway,
    fwd: 1,
    bwd: 0,
    speed: 20.1168, // 45 mph
    parking: false,
    grade: 0.08,
    bank_max: 0.05,
    material: RoadMaterial::Asphalt,
};
/// One Loop 9 carriageway (one-way, three lanes).
const HIGHWAY_CARRIAGEWAY: Style = Style { bwd: 0, ..HIGHWAY };
/// Crossroad of a Loop 9 interchange (bridges over the highway).
const CROSSROAD: Style = Style {
    parking: false,
    grade: 0.10,
    ..ARTERIAL
};

/// Loop 9 lane width, m (`BuildOptions::highway_lane_width_m`).
const HW_LANE_M: f32 = 3.7;
/// Paved median shoulder between a carriageway's left lane and the barrier, m.
const MEDIAN_SHOULDER_M: f32 = 1.5;
/// Paved outer shoulder right of a carriageway, m.
const OUTER_SHOULDER_M: f32 = 2.5;
/// Carriageway centreline offset from the loop centreline (the barrier), m.
const CARRIAGEWAY_OFFSET_M: f32 = MEDIAN_SHOULDER_M + 1.5 * HW_LANE_M;
/// Gore (diverge / merge node) distance from an interchange's centre along the loop, m.
const RAMP_SPAN_M: f32 = 300.0;
/// Ramp length from its gore that runs beside the carriageway (taper + parallel lane), m.
const RAMP_PARALLEL_M: f32 = 90.0;
/// Ramp lane centre beside a carriageway: next to the rightmost lane (from the carriageway
/// centreline), m.
const RAMP_LANE_OFFSET_M: f32 = 2.0 * HW_LANE_M;
/// Terminal junction distance from the loop centreline at a full interchange, m.
const FULL_REACH_M: f32 = 150.0;

/// Loop 9 rest stop (playtest row 153): gore distance from its centre along the loop, m.
const REST_SPAN_M: f32 = 275.0;
/// Half length of a rest stop's parking road (between its ramp ends), m.
const REST_HALF_M: f32 = 50.0;
/// Parking road centreline from the loop centreline, m (beyond the carriageway, its outer
/// shoulder and the ramp lane).
const REST_OFFSET_M: f32 = 34.0;
/// A rest stop's parking road: one-way, one lane, parking on both kerbs, walking pace.
const REST_ROAD: Style = Style {
    class: RoadClass::Local,
    fwd: 1,
    bwd: 0,
    speed: 8.9408, // 20 mph
    parking: true,
    // The local-street cap: the Hollins one runs across the hill's western slope.
    grade: 0.10,
    bank_max: 0.0,
    material: RoadMaterial::Asphalt,
};

/// Loop 9 rest stops: (near point on the loop, carriageway side +1 outer / -1 inner, name).
/// Placed on the stretches long enough for their ramps (clear of the interchanges, Route 40's
/// crossings and the Hollins Tunnel); the south ones sit on the inner carriageway (the sea
/// starts just beyond the outer one there). The north-east stretch (Bluff Road to Uptown) only
/// has room on the banked corner, where a gore does not meet the drawn surface; none there.
const REST_SITES: [(V2, f32, &str); 3] = [
    ([-1490.0, 1120.0], 1.0, "Hollins Rest Area"),
    ([-960.0, -1035.0], -1.0, "Larkspur Rest Area"),
    ([40.0, -1063.0], -1.0, "Harbor Rest Area"),
];

/// A Loop 9 rest stop: an off-ramp from one carriageway to its parking road and an on-ramp
/// back.
#[derive(Debug, Clone, Copy)]
struct RestStop {
    /// Centre on the loop centreline (before profiling), m.
    centre: V2,
    /// +1 outer (counter-clockwise) carriageway, -1 inner.
    side: f32,
    /// Parking road start (off-ramp end) and end (on-ramp start).
    t_in: usize,
    t_out: usize,
    /// Draft index of the parking road.
    road: usize,
    name: &'static str,
}

/// A Loop 9 interchange: a diamond whose crossroad bridges the highway (full), or a half
/// diamond with ramps to the inner carriageway only.
#[derive(Debug, Clone, Copy)]
struct Interchange {
    /// Centre on the loop centreline (before profiling), m.
    centre: V2,
    /// Inner terminal (the local road's end).
    inner: usize,
    /// Outer terminal (full interchanges only).
    outer: Option<usize>,
}

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
    /// Internal minimum heights `(s0, s1, z)` (bridges over water: the deck clears the banks
    /// and the water, and may stand higher where the road's line wants it).
    floors: Vec<(f32, f32, f32)>,
    /// Arc ranges that are bridge decks (no terrain carving).
    decks: Vec<(f32, f32)>,
    /// Covered stretches (tunnels) on this road.
    covers: Vec<CoverPlan>,
    /// Bridges over a highway it crosses (interchange crossroad).
    over: bool,
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
    /// Road indices of the highway loop's carriageways (outer counter-clockwise pieces
    /// first, then the inner clockwise ones).
    pub loop_roads: Vec<usize>,
    /// Road indices of the interchange ramps.
    pub ramp_roads: Vec<usize>,
    /// Names per source road.
    pub road_names: Vec<&'static str>,
    /// Speed limit per source road, m/s.
    pub road_speeds: Vec<f32>,
    /// Grade cap per source road.
    pub road_grades: Vec<f32>,
    /// Road indices of a region's open-freeway carriageway pieces.
    pub freeway_roads: Vec<usize>,
    /// Road indices of a region's open-freeway diamond ramps and links.
    pub freeway_ramps: Vec<usize>,
    /// Road indices of the links (freeway-to-freeway ramps; subset of `freeway_ramps`).
    pub freeway_links: Vec<usize>,
}

struct Gen {
    rng: Rng,
    nat: Natural,
    /// Map extent (terrain, building grid, bounds).
    lo: V2,
    hi: V2,
    nodes: Vec<NodeSpec>,
    index: BTreeMap<(i64, i64), usize>,
    drafts: Vec<Draft>,
    skipped: usize,
    /// Loop 9 interchanges.
    interchanges: Vec<Interchange>,
    /// Loop 9 rest stops.
    rest_stops: Vec<RestStop>,
    /// Ramps that will end at each node (counted into its junction flat length).
    ramp_ends: Vec<usize>,
    /// Open divided freeways a region lays out (split by [`divide_freeways`]).
    freeways: Vec<FreewayPlan>,
    /// Freeway-to-freeway links (system ramps) between those freeways.
    links: Vec<LinkPlan>,
    /// Plan paths of ramps built after profiling: a non-highway road crossing one is dropped
    /// like one crossing another road at grade.
    reserved: Vec<Vec<V2>>,
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
            }) || self
                .reserved
                .iter()
                .any(|r| !path_crossings(&path, r).is_empty());
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
            floors: Vec::new(),
            decks: Vec::new(),
            covers: Vec::new(),
            over: false,
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
        let ltotal = lcum[lcum.len() - 1];
        // Interchange sites: (near point, terminal reach, full diamond, crossroad name).
        let sites: [(V2, f32, bool, &'static str); 6] = [
            ([-1490.0, -480.0], FULL_REACH_M, true, "Harbor Boulevard"),
            ([1505.0, -450.0], FULL_REACH_M, true, "Harbor Boulevard"),
            ([-1515.0, 420.0], FULL_REACH_M, true, "Lantern Way"),
            ([920.0, -1050.0], 50.0, false, "Port Connector"),
            ([1510.0, 300.0], FULL_REACH_M, true, "Bluff Road"),
            ([400.0, 1400.0], FULL_REACH_M, true, "Uptown Connector"),
        ];
        let mut site_s: Vec<f32> = sites
            .iter()
            .map(|t| project2(&closed, &lcum, t.0).0)
            .collect();
        // Terminals: inner on the city side (left of the counter-clockwise loop), outer
        // beyond the highway; the crossroad between them bridges both carriageways.
        let mut terminal = Vec::new();
        for (k, &(_, reach, full, name)) in sites.iter().enumerate() {
            let (c, d) = point_on(&closed, &lcum, site_s[k]);
            let inward = [-d[1], d[0]];
            let inner = self.node(geom::add(c, geom::scale(inward, reach)));
            let outer = full.then(|| self.node(geom::sub(c, geom::scale(inward, reach))));
            if let Some(o) = outer {
                let path = bent(self.pos(inner), self.pos(o), 0.0, 8.0);
                if let Some(r) = self.road(inner, o, path, CROSSROAD, name) {
                    self.drafts[r].over = true;
                }
                self.ramp_ends.extend([o, o]);
            }
            self.ramp_ends.extend([inner, inner]);
            self.interchanges.push(Interchange {
                centre: c,
                inner,
                outer,
            });
            terminal.push(inner);
        }
        // Rest stops: the parking road runs beside the loop, ramps at either end.
        for (near, side, name) in REST_SITES {
            let sc = project2(&closed, &lcum, near).0;
            // Beside the loop at arc `u` from the centre: outward of the outer carriageway,
            // inward (city side) of the inner one.
            let beside = |u: f32| {
                let (c, d) = point_on(&closed, &lcum, (sc + u).rem_euclid(ltotal));
                let inward = [-d[1], d[0]];
                geom::add(c, geom::scale(inward, -side * REST_OFFSET_M))
            };
            // Travel runs with the ring on the outer carriageway, against it on the inner.
            let (u_in, u_out) = (-side * REST_HALF_M, side * REST_HALF_M);
            let t_in = self.node(beside(u_in));
            let t_out = self.node(beside(u_out));
            let n = (2.0 * REST_HALF_M / 6.0).ceil() as usize;
            let path: Vec<V2> = (0..=n)
                .map(|k| beside(u_in + (u_out - u_in) * k as f32 / n as f32))
                .collect();
            let road = self
                .road(t_in, t_out, path, REST_ROAD, name)
                .expect("rest stop road crosses another road");
            self.ramp_ends.extend([t_in, t_out]);
            self.rest_stops.push(RestStop {
                centre: point_on(&closed, &lcum, sc).0,
                side,
                t_in,
                t_out,
                road,
                name,
            });
            site_s.push(sc);
        }
        // The loop is cut half-way between interchanges (plain bend nodes), but not near
        // where Route 40 West / North cross it (their grade separations need the room).
        site_s.sort_by(f32::total_cmp);
        // Nor over the river (its bridges).
        let route_40 = [
            [-1530.0, 100.0],
            [0.0, 1390.0],
            [-505.0, 1370.0],
            [-505.0, -1065.0],
        ];
        let cuts: Vec<f32> = (0..site_s.len())
            .map(|k| {
                let a = site_s[k];
                let b = if k + 1 < site_s.len() {
                    site_s[k + 1]
                } else {
                    site_s[0] + ltotal
                };
                (0.5 * (a + b)) % ltotal
            })
            .filter(|c| {
                let p = point_on(&closed, &lcum, *c).0;
                route_40.iter().all(|q| geom::dist(p, *q) > 300.0)
            })
            .collect();
        let (_, loop_roads) = self.ring(&loop_path, &cuts, HIGHWAY, "Loop 9");
        let [lj_bw, lj_be, lj_lw, lj_port, lj_bluff, lj_up] = [
            terminal[0],
            terminal[1],
            terminal[2],
            terminal[3],
            terminal[4],
            terminal[5],
        ];

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
            -1110.0, -900.0, -700.0,
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
        let quay = [[380.0, -925.0], [1250.0, -925.0]];
        let qcuts = [450.0 - 380.0, 920.0 - 380.0];
        let (qj, q_w, q_e) = self.chain(&quay, &qcuts, PORT, "Terminal Avenue");
        self.link(bj[7], qj[0], 0.0, PORT, "Gantry Row");
        self.link(bj[9], qj[1], 0.0, PORT, "Gantry Row");
        self.link(bj[6], q_w, -10.0, SERVICE, "Gantry Row");
        self.link(qj[1], lj_port, 0.0, PORT, "Port Connector");
        // Terminal Avenue ends at the east quay (Loop 9 is reached through the port).
        let _ = q_e;
        let s1 = self.node([450.0, -770.0]);
        let s2 = self.node([920.0, -770.0]);
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
        // West of the river; the Calder Canal runs between the two rows.
        let cx = [-1110.0, -900.0, -700.0];
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
            [1255.0, 560.0],
            [1250.0, 380.0],
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
            [-430.0, 1160.0],
            [-300.0, 1000.0],
            self.pos(mer[3][1]),
        ];
        let ridge = resample(&catmull_rom(&ridge_ctrl, 5.0), 5.0);
        self.road(knob, mer[3][1], ridge, HILL_ROAD, "Ridge Road");

        // ---- Residential courts (playtest row 95) -------------------------------------
        // Laid last so every road above keeps its shape: curving dead-end streets that leave
        // the middle of a block edge (a T-junction) and end in a turnaround inside the block.
        for (grid, name) in [
            (&sub, "Larkspur Court"),
            (&mer, "Mercer Court"),
            (&eg, "Eastgate Court"),
            (&bv, "Bayview Court"),
        ] {
            self.courts(grid, name);
        }

        (loop_roads, hollins, knob, lot, r40w, r40n)
    }

    /// Courts off a residential grid (playtest row 95): for each local block edge at least
    /// [`COURT_MIN_EDGE_M`] long, with chance [`COURT_CHANCE`], a curving dead-end street from
    /// the edge's middle (a T-junction splitting the edge) [`COURT_LEN_M`] into the block on
    /// one side, kept [`COURT_CLEARANCE_M`] from every other road beyond its first 25 m.
    fn courts(&mut self, grid: &[Vec<usize>], name: &'static str) {
        let ids: std::collections::BTreeSet<usize> = grid.iter().flatten().copied().collect();
        // Candidate drafts: local streets between two nodes of this grid.
        let cands: Vec<(usize, usize)> = self
            .drafts
            .iter()
            .filter(|d| ids.contains(&d.a) && ids.contains(&d.b))
            .filter(|d| d.st.class == RoadClass::Local)
            .map(|d| (d.a, d.b))
            .collect();
        for (a, b) in cands {
            let roll = self.rng.next_f32();
            let side = if self.rng.next_f32() < 0.5 { 1.0 } else { -1.0 };
            let len = COURT_LEN_M * self.rng.range_f32(0.8, 1.15);
            let bulge = self.rng.range_f32(-10.0, 10.0);
            if roll >= COURT_CHANCE {
                continue;
            }
            let Some(k) = self.drafts.iter().position(|d| d.a == a && d.b == b) else {
                continue;
            };
            let path = self.drafts[k].path.clone();
            let cum = cum2(&path);
            let total = cum[cum.len() - 1];
            if total < COURT_MIN_EDGE_M {
                continue;
            }
            let (mid, t) = point_on(&path, &cum, total * 0.5);
            let dir = geom::scale(geom::right_of(t), side);
            let end = geom::add(mid, geom::scale(dir, len));
            let stub = bent(mid, end, bulge, 8.0);
            // Clear of every other road (and of other courts) past its mouth.
            let scum = cum2(&stub);
            let clear = (0..stub.len()).filter(|i| scum[*i] > 25.0).all(|i| {
                // Its own block edge runs away behind it.
                self.drafts
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != k)
                    .all(|(_, d)| {
                        d.path.windows(2).all(|w| {
                            geom::point_segment_distance(stub[i], w[0], w[1]) > COURT_CLEARANCE_M
                        })
                    })
            });
            let crosses = self.drafts.iter().any(|d| {
                d.st.class != RoadClass::Highway && !path_crossings(&stub[1..], &d.path).is_empty()
            });
            if !clear || crosses {
                continue;
            }
            // Split the edge at its middle (keeping its curve) and lay the court.
            // In place (the first half keeps the draft's index; road indices handed out
            // earlier, e.g. Hollins Drive's, stay valid).
            let d = self.drafts[k].clone();
            let m = self.node(mid);
            let first = sub_path(&d.path, &cum, 0.0, total * 0.5);
            let second = sub_path(&d.path, &cum, total * 0.5, total);
            self.drafts[k] = Draft {
                b: m,
                path: first,
                ..d.clone()
            };
            self.drafts.push(Draft {
                a: m,
                path: second,
                ..d
            });
            let e = self.node(end);
            self.road(m, e, stub, SERVICE, name);
        }
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
    for &n in &g.ramp_ends {
        deg[n] += 1;
        hw[n] = hw[n].max(half_width(&RAMP, opt));
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
/// (sample ranges with fixed heights), stays on or above `floors` (sample ranges with minimum
/// heights) and is never steeper than `grade`; then smoothed with
/// enough binomial passes that a full grade reversal (crest or sag) bends no tighter than
/// `kmax` (1/m). During smoothing only the two end samples and the middle of each internal
/// anchor (decks) stay pinned, so junction approaches round off gently instead of kinking.
// The three hold kinds (end anchors, internal anchors, floors) are separate on purpose:
// each is pinned and smoothed differently.
#[allow(clippy::too_many_arguments)]
fn profile(
    desired: &[f32],
    ds: f32,
    anchors: &[(usize, usize, f32)],
    internal: &[(usize, usize, f32)],
    floors: &[(usize, usize, f32)],
    pins: (usize, usize),
    grade: f32,
    kmax: f32,
) -> Vec<f32> {
    let n = desired.len();
    if floors.is_empty() {
        return profile_pass(desired, ds, anchors, internal, &[], pins, grade, kmax);
    }
    // Floors (decks over water) are not pinned: pinning one side of each corner where the
    // approach meets the deck height left it rounded on the other side only, and the final
    // clamp put the kink back (a 4.6 % grade break in 10 m on Loop 9's canal bridge launched
    // cars at 59 m/s). Instead nothing is pinned and each floor is lifted by whatever the
    // smoothing still took off it, until the smoothed profile clears it: the curvature limit
    // holds over the deck too. (Holding floors further out instead lengthens the
    // embankments and lifts the lots beside them off the drawn terrain.)
    let mut lift = vec![0.0f32; floors.len()];
    let mut z = Vec::new();
    for _ in 0..8 {
        let held: Vec<(usize, usize, f32)> = floors
            .iter()
            .zip(&lift)
            .map(|(&(i0, i1, f), &l)| (i0, i1, f + l))
            .collect();
        z = profile_pass(desired, ds, anchors, internal, &held, pins, grade, kmax);
        let mut clear = true;
        for (k, &(i0, i1, f)) in floors.iter().enumerate() {
            let short = (i0..=i1.min(n - 1))
                .map(|i| f - z[i])
                .fold(0.0f32, f32::max);
            if short > 0.002 {
                lift[k] += short * 1.1;
                clear = false;
            }
        }
        if clear {
            break;
        }
    }
    // Safety: nothing inside a floor range ends up under it (millimetres at most).
    for &(i0, i1, f) in floors {
        for v in z.iter_mut().take(i1.min(n - 1) + 1).skip(i0) {
            *v = v.max(f);
        }
    }
    z
}

/// One pass of [`profile`]: cones, grade limit and smoothing with `floors` as minimum
/// heights on the cones only (never pinned).
#[allow(clippy::too_many_arguments)]
fn profile_pass(
    desired: &[f32],
    ds: f32,
    anchors: &[(usize, usize, f32)],
    internal: &[(usize, usize, f32)],
    floors: &[(usize, usize, f32)],
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
    for &(i0, i1, z) in floors {
        for (i, l) in lo.iter_mut().enumerate() {
            let d = if i < i0 {
                (i0 - i) as f32
            } else if i > i1 {
                (i - i1) as f32
            } else {
                0.0
            };
            *l = l.max(z - g * d);
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
/// `bounds`: height ranges some nodes must stay in (interchange terminals, so their ramps
/// can reach the highway).
fn profile_roads(
    g: &mut Gen,
    opt: &BuildOptions,
    bounds: &BTreeMap<usize, (f32, f32)>,
) -> Vec<RoadSpec> {
    let flat = flat_lengths(g, opt);
    // Node heights: natural ground, relaxed so every road can meet its grade cap (end to end
    // and to its grade-separation anchors).
    for k in 0..g.nodes.len() {
        let p = g.nodes[k].pos;
        g.nodes[k].z = g.nat.z(p);
        if let Some((lo, hi)) = bounds.get(&k) {
            g.nodes[k].z = g.nodes[k].z.clamp(*lo, *hi);
        }
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
            // Floors: lift an end node that could not climb onto one.
            for &(s0, s1, z) in &d.floors {
                for (n, room) in [
                    (d.a, s0 - flat[d.a] - spread),
                    (d.b, len - s1 - flat[d.b] - spread),
                ] {
                    let allow = d.st.grade * room.max(len * 0.05) * 0.85;
                    let lack = z - allow - g.nodes[n].z;
                    if lack > 0.01 {
                        g.nodes[n].z += lack;
                        moved = true;
                    }
                }
            }
            // Anchors are fixed: pull the end nodes into reach of them.
            for &(s0, s1, z) in &d.anchors {
                for (n, room) in [
                    (d.a, s0 - flat[d.a] - spread),
                    (d.b, len - s1 - flat[d.b] - spread),
                ] {
                    let allow = d.st.grade * room.max(len * 0.05) * 0.85;
                    let zn = g.nodes[n].z;
                    let excess = (zn - z).abs() - allow;
                    if excess > 0.01 {
                        g.nodes[n].z -= excess * (zn - z).signum();
                        moved = true;
                    }
                }
            }
        }
        for (k, (lo, hi)) in bounds {
            let z = g.nodes[*k].z;
            if z < *lo - 0.01 || z > *hi + 0.01 {
                g.nodes[*k].z = z.clamp(*lo, *hi);
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
        let floors: Vec<(usize, usize, f32)> = d
            .floors
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
        let z = if short && d.anchors.is_empty() && d.floors.is_empty() {
            (0..n)
                .map(|i| za + (zb - za) * i as f32 / (n - 1) as f32)
                .collect()
        } else {
            profile(
                &desired,
                dsr,
                &anchors,
                &internal,
                &floors,
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
        r.material = d.st.material;
        // 3 + 3 arterials (Ashpeak City's grand avenues; Port Vesper has none) carry a bus
        // lane each way.
        r.bus_lane = d.st.class == RoadClass::Arterial && d.st.fwd >= 3 && d.st.bwd >= 3;
        if linear_dev < 0.25 && d.anchors.is_empty() && d.floors.is_empty() {
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

/// Bridge every road over the river and the canal: across the channel (water + banks) the
/// road stays at or above deck height and that stretch is a deck (not carved). Deck height:
/// just over the lower bank, but at least the clearance above the water. A floor, not a hold
/// (PR #38 held it there): a road whose line passes higher keeps it, so Ridge Road and the
/// Hollins Descent still climb onto Hollins Knob instead of cutting 26 m into it.
fn plan_water_crossings(g: &mut Gen) {
    for di in 0..g.drafts.len() {
        let path = g.drafts[di].path.clone();
        let highway = g.drafts[di].st.class == RoadClass::Highway;
        let cum = cum2(&path);
        let len = cum[cum.len() - 1];
        for c in &g.nat.channels {
            for (s_road, _, _) in path_crossings(&path, &c.pts) {
                let wet = |s: f32| c.covers(point_on(&path, &cum, s).0, 3.0);
                let (mut s0, mut s1) = (s_road, s_road);
                while s0 > 0.0 && wet(s0) {
                    s0 -= 2.0;
                }
                while s1 < len && wet(s1) {
                    s1 += 2.0;
                }
                if s0 <= 0.0 || s1 >= len {
                    continue;
                }
                let za = g.nat.z(point_on(&path, &cum, s0 - 4.0).0);
                let zb = g.nat.z(point_on(&path, &cum, s1 + 4.0).0);
                let zw = c.near(point_on(&path, &cum, s_road).0).map_or(SEA, |n| n.1);
                let clear = match (highway, c.kind) {
                    (true, _) => 4.5,
                    (false, WaterwayKind::River) => 3.0,
                    (false, WaterwayKind::Canal) => 2.2,
                };
                let deck = (za.min(zb) + 0.3).max(zw + clear);
                let d = &mut g.drafts[di];
                d.floors.push((s0, s1, deck));
                d.decks.push((s0 - 3.0, s1 + 3.0));
            }
        }
    }
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
                if g.drafts[lo].over {
                    // Interchange crossroad: the highway stays near the ground, the
                    // crossroad bridges both carriageways (margin for the highway's bank).
                    let z_hw = ground + 0.5;
                    let half = CARRIAGEWAY_OFFSET_M + 1.5 * HW_LANE_M + OUTER_SHOULDER_M;
                    g.drafts[hi].anchors.push((s_hi - 30.0, s_hi + 30.0, z_hw));
                    g.drafts[lo].anchors.push((
                        s_lo - half - 3.0,
                        s_lo + half + 3.0,
                        z_hw + CROSSING_CLEARANCE_M + 0.4,
                    ));
                    g.drafts[lo]
                        .decks
                        .push((s_lo - half - 6.0, s_lo + half + 6.0));
                    out.push(Crossing {
                        upper: lo,
                        lower: hi,
                        pos: p,
                        underpass: false,
                    });
                    continue;
                }
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
fn plan_covers(g: &mut Gen, extra: &[CoverSpec]) {
    let mut plans: Vec<(&str, V2, V2, V2, f32, CoverKind, &'static str)> = vec![
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
            [416.0, 1000.0],
            [418.0, 905.0],
            [414.0, 1110.0],
            6.0,
            CoverKind::CoveredTrench,
            "Founders Cut",
        ),
    ];
    plans.extend(
        extra
            .iter()
            .map(|c| (c.road, c.probe, c.p0, c.p1, c.dip, c.kind, c.name)),
    );
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

/// Covered spans on the built road surfaces (portal to portal), one per plan. A plan on a
/// Loop 9 draft covers the divided highway as one span along the loop's shared centreline
/// ([`loop_cover`]). Any other plan follows its road's surface (`remap`: draft → builder
/// road); an open freeway's divided draft (Ashpeak County: `remap` is `None`, `divided(draft)`
/// lists its carriageway pieces) gets one tube spanning both carriageways through the
/// stretch: its centreline runs midway between them and its half width reaches the far paved
/// edge.
fn covered_spans(
    g: &Gen,
    built: &Built,
    ring: &Ring,
    loop_drafts: &[usize],
    remap: &[Option<usize>],
    divided: &dyn Fn(usize) -> Vec<usize>,
) -> Vec<CoveredSpan> {
    let mut out = Vec::new();
    for (di, d) in g.drafts.iter().enumerate() {
        if d.covers.is_empty() {
            continue;
        }
        if loop_drafts.contains(&di) {
            out.extend(d.covers.iter().filter_map(|c| loop_cover(built, ring, c)));
            continue;
        }
        for c in &d.covers {
            let roads: Vec<usize> = match remap.get(di).copied().flatten() {
                Some(r) => vec![r],
                // A carriageway passes through the covered stretch when both portals project
                // onto it (inside its ends) within reach of its centreline.
                None => divided(di)
                    .into_iter()
                    .filter(|r| {
                        let s = &built.surfaces[*r];
                        let cum = geom::cumulative(&s.centre);
                        let len = cum[cum.len() - 1];
                        [c.p0, c.p1].iter().all(|p| {
                            let pr = geom::project(&s.centre, &cum, *p);
                            pr.s > 0.0 && pr.s < len && pr.dist < 2.0 * CARRIAGEWAY_OFFSET_M
                        })
                    })
                    .collect(),
            };
            let Some(&ri) = roads.first() else {
                continue;
            };
            let s = &built.surfaces[ri];
            let cum = geom::cumulative(&s.centre);
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
            let mut edge = s.left_m.max(s.right_m);
            // The other carriageways: move the centreline midway to the nearest point of each
            // and widen the tube by that half spacing.
            for &rj in &roads[1..] {
                let o = &built.surfaces[rj];
                let ocum = geom::cumulative(&o.centre);
                let mut half = 0.0f32;
                for p in centre.iter_mut() {
                    let pr = geom::project(&o.centre, &ocum, geom::xy(*p));
                    let q = geom::point_at(&o.centre, &ocum, pr.s).0;
                    half = half.max(0.5 * geom::dist(geom::xy(*p), geom::xy(q)));
                    *p = [
                        0.5 * (p[0] + q[0]),
                        0.5 * (p[1] + q[1]),
                        0.5 * (p[2] + q[2]),
                    ];
                }
                edge = edge.max(o.left_m.max(o.right_m)) + half;
            }
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

/// A cover on Loop 9 (divided highway): one span along the loop's shared centreline between
/// the portals, wide enough for both carriageways and their outer shoulders (the median
/// barrier runs on through it). Its `road` is the carriageway piece travelling with the loop
/// (counter-clockwise, right of the centreline) nearest the span's middle.
fn loop_cover(built: &Built, ring: &Ring, c: &CoverPlan) -> Option<CoveredSpan> {
    let (a, b) = (ring.project(c.p0), ring.project(c.p1));
    let (s0, s1) = (a.min(b), a.max(b));
    if s1 - s0 < 1.0 || s1 - s0 > ring.total() * 0.5 {
        return None;
    }
    let mut centre = Vec::new();
    let mut s = s0;
    loop {
        let (p, _, z, _) = ring.at(s.min(s1));
        centre.push([p[0], p[1], z]);
        if s >= s1 {
            break;
        }
        s += 4.0;
    }
    let (mid, dir, _, _) = ring.at(0.5 * (s0 + s1));
    let probe = geom::add(mid, geom::scale(geom::right_of(dir), CARRIAGEWAY_OFFSET_M));
    let road = built
        .surfaces
        .iter()
        .enumerate()
        .filter(|(_, sf)| sf.highway && sf.median_barrier && sf.centre.len() >= 2)
        .map(|(ri, sf)| {
            let cum = geom::cumulative(&sf.centre);
            (ri, geom::project(&sf.centre, &cum, probe).dist)
        })
        .min_by(|x, y| x.1.total_cmp(&y.1))?
        .0;
    let sf = &built.surfaces[road];
    Some(CoveredSpan {
        name: c.name.into(),
        kind: c.kind,
        road: road as u32,
        centre,
        half_width_m: CARRIAGEWAY_OFFSET_M + sf.right_m + 1.5,
        clearance_m: TUNNEL_CLEARANCE_HIGHWAY_M,
    })
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
// Divided highway and interchanges
// ------------------------------------------------------------------------------------------

/// What a built road is (one per builder road, after [`divide_loop`]).
#[derive(Debug, Clone, Copy)]
struct RoadInfo {
    name: &'static str,
    speed: f32,
    grade: f32,
    /// Where its bridge-deck ranges come from.
    deck: DeckRef,
}

/// Source of a road's deck ranges for terrain carving.
#[derive(Debug, Clone, Copy)]
enum DeckRef {
    /// A draft's own `decks` (arc lengths along its path).
    Draft(usize),
    /// Loop 9's ring decks (carriageways).
    Ring,
    /// No planned decks (ramps; elevated stretches are still found by height).
    None,
    /// An open freeway's line decks (its carriageways; index into the freeway lines).
    Line(usize),
}

/// Loop 9's profiled centreline as one closed ring (counter-clockwise), the plane of its
/// cross-section (height + bank) at every point, and its deck ranges.
/// Also used for an open freeway's centreline (`closed == false`: arc lengths clamp to its
/// ends, "counter-clockwise" reads "forward").
struct Ring {
    pts: Vec<V2>,
    z: Vec<f32>,
    bank: Vec<f32>,
    cum: Vec<f32>,
    decks: Vec<(f32, f32)>,
    closed: bool,
}

impl Ring {
    /// An open line from profiled roads (in order, each a → b forward).
    fn open(g: &Gen, specs: &[RoadSpec], roads: &[usize]) -> Self {
        let mut r = Self::new(g, specs, roads);
        r.closed = false;
        r
    }

    /// A flat open line along a plan path (layout-time geometry only).
    fn from_xy(pts: &[V2]) -> Self {
        let cum = cum2(pts);
        Self {
            pts: pts.to_vec(),
            z: vec![0.0; pts.len()],
            bank: vec![0.0; pts.len()],
            cum,
            decks: Vec::new(),
            closed: false,
        }
    }

    /// Build from the loop's profiled roads (in ring order, each a → b counter-clockwise).
    fn new(g: &Gen, specs: &[RoadSpec], loop_roads: &[usize]) -> Self {
        let mut pts: Vec<V2> = Vec::new();
        let mut z: Vec<f32> = Vec::new();
        let mut bank: Vec<f32> = Vec::new();
        for &r in loop_roads {
            let sp = &specs[r];
            let mut xy = vec![g.nodes[sp.a].pos];
            xy.extend_from_slice(&sp.via);
            xy.push(g.nodes[sp.b].pos);
            let c = cum2(&xy);
            let total = c[c.len() - 1].max(1e-6);
            let (za, zb) = (g.nodes[sp.a].z, g.nodes[sp.b].z);
            let explicit = sp.via_z.len() == sp.via.len() && !sp.via.is_empty();
            let zs: Vec<f32> = (0..xy.len())
                .map(|k| {
                    if explicit && k > 0 && k <= sp.via.len() {
                        sp.via_z[k - 1]
                    } else {
                        za + (zb - za) * c[k] / total
                    }
                })
                .collect();
            let bs: Vec<f32> = if sp.bank.len() == xy.len() {
                sp.bank.clone()
            } else {
                vec![0.0; xy.len()]
            };
            let skip = usize::from(!pts.is_empty());
            pts.extend_from_slice(&xy[skip..]);
            z.extend_from_slice(&zs[skip..]);
            bank.extend_from_slice(&bs[skip..]);
        }
        let cum = cum2(&pts);
        let mut ring = Self {
            pts,
            z,
            bank,
            cum,
            decks: Vec::new(),
            closed: true,
        };
        for &r in loop_roads {
            let d = &g.drafts[r];
            let dc = cum2(&d.path);
            for &(s0, s1) in &d.decks {
                let a = ring.project(point_on(&d.path, &dc, s0).0);
                let b = ring.project(point_on(&d.path, &dc, s1).0);
                ring.decks.push((a.min(b), a.max(b)));
            }
        }
        ring
    }

    fn total(&self) -> f32 {
        self.cum[self.cum.len() - 1]
    }

    /// Arc length of the closest ring point to `q`.
    fn project(&self, q: V2) -> f32 {
        project2(&self.pts, &self.cum, q).0
    }

    /// Point, unit tangent, height and bank at arc length `s` (wrapped).
    fn at(&self, s: f32) -> (V2, V2, f32, f32) {
        let s = if self.closed {
            s.rem_euclid(self.total())
        } else {
            s.clamp(0.0, self.total())
        };
        let n = self.pts.len();
        let i = match self.cum.binary_search_by(|c| c.total_cmp(&s)) {
            Ok(i) => i.min(n - 2),
            Err(i) => i.saturating_sub(1).min(n - 2),
        };
        let l = (self.cum[i + 1] - self.cum[i]).max(1e-6);
        let t = (s - self.cum[i]) / l;
        let p = geom::add(
            self.pts[i],
            geom::scale(geom::sub(self.pts[i + 1], self.pts[i]), t),
        );
        let d = geom::normalize(geom::sub(self.pts[i + 1], self.pts[i]));
        let z = self.z[i] + (self.z[i + 1] - self.z[i]) * t;
        let b = self.bank[i] + (self.bank[i + 1] - self.bank[i]) * t;
        (p, d, z, b)
    }

    /// Point on the cross-section plane at arc length `s`, `lat` metres right of
    /// counter-clockwise travel.
    fn offset(&self, s: f32, lat: f32) -> V3 {
        let (p, d, z, b) = self.at(s);
        let q = geom::add(p, geom::scale(geom::right_of(d), lat));
        [q[0], q[1], z + b * lat]
    }

    /// Height of the cross-section plane under `q` (nearest ring point).
    fn plane_z(&self, q: V2) -> f32 {
        let s = self.project(q);
        let (p, d, z, b) = self.at(s);
        z + b * geom::dot(geom::sub(q, p), geom::right_of(d))
    }

    fn in_deck(&self, q: V2) -> bool {
        let s = self.project(q);
        self.decks.iter().any(|(a, b)| s >= *a && s <= *b)
    }
}

/// A road spec from a 3D centreline `pts` (ends on nodes `a`, `b`) with bank per point.
fn spec_from(a: usize, b: usize, pts: &[V3], bank: Vec<f32>, st: &Style) -> RoadSpec {
    let mut r = RoadSpec::new(a, b, st.class, st.fwd, st.bwd);
    r.speed_mps = st.speed;
    r.via = pts[1..pts.len() - 1].iter().map(|p| geom::xy(*p)).collect();
    r.via_z = pts[1..pts.len() - 1].iter().map(|p| p[2]).collect();
    r.bank = bank;
    r
}

/// Arc lengths from `s0` to `s1` (either direction) about `step` apart, both ends kept.
fn arc_samples(s0: f32, s1: f32, step: f32) -> Vec<f32> {
    let n = (((s1 - s0).abs() / step).ceil() as usize).max(1);
    (0..=n)
        .map(|k| s0 + (s1 - s0) * k as f32 / n as f32)
        .collect()
}

/// One ramp's centreline (gore end first) and its travel direction along the loop.
struct RampPath {
    /// Gore node (set once created), terminal node.
    gore: usize,
    terminal: usize,
    /// Gore position on the carriageway and its ring arc length.
    gore_p: V3,
    gore_s: f32,
    /// Leaves the carriageway (off-ramp) rather than joins it.
    off: bool,
    /// Centreline from the gore to the terminal, xy.
    path: Vec<V2>,
    /// +1 counter-clockwise (outer carriageway), -1 clockwise (inner).
    dir: f32,
}

/// Ramp centreline from the gore at ring arc `gore_s` on the carriageway on `side` (+1
/// outer, -1 inner) to `terminal`, arriving parallel to the highway at the interchange. An
/// off-ramp runs downstream from the gore, an on-ramp upstream (the path is still listed
/// gore first).
fn ramp_path(ring: &Ring, gore_s: f32, side: f32, off: bool, terminal: V2) -> Vec<V2> {
    ramp_path_at(
        ring,
        gore_s,
        side,
        off,
        terminal,
        CARRIAGEWAY_OFFSET_M,
        RAMP_LANE_OFFSET_M,
    )
}

/// [`ramp_path`] for a carriageway centred `offset` m from the line with the ramp lane
/// `lane_off` m beyond that.
fn ramp_path_at(
    ring: &Ring,
    gore_s: f32,
    side: f32,
    off: bool,
    terminal: V2,
    offset: f32,
    lane_off: f32,
) -> Vec<V2> {
    // Travel along the ring is counter-clockwise on the outer carriageway.
    let dir = side;
    // Away from the gore along the ring: downstream for an off-ramp, upstream for an on-ramp.
    let away = if off { dir } else { -dir };
    let mut path: Vec<V2> = Vec::new();
    let mut u = 0.0;
    while u <= RAMP_PARALLEL_M + 1e-3 {
        let x = lane_off * smoothstep(0.0, crate::builder::FREEWAY_SETBACK_M, u);
        let p = ring.offset(gore_s + away * u, side * (offset + x));
        path.push(geom::xy(p));
        u += 6.0;
    }
    // Then an S-curve to the terminal, arriving (or leaving) along the highway's direction.
    let p0 = path[path.len() - 1];
    let (_, t0, _, _) = ring.at(gore_s + away * RAMP_PARALLEL_M);
    let d0 = geom::scale(t0, away);
    let (_, tc, _, _) = ring.at(ring.project(terminal));
    let d3 = geom::scale(tc, away);
    let chord = geom::dist(p0, terminal);
    let k = 0.4 * chord;
    let p1 = geom::add(p0, geom::scale(d0, k));
    let p2 = geom::sub(terminal, geom::scale(d3, k));
    let n = ((chord / 6.0).ceil() as usize).max(4) * 2;
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let w = 1.0 - t;
        let q = [
            w * w * w * p0[0]
                + 3.0 * w * w * t * p1[0]
                + 3.0 * w * t * t * p2[0]
                + t * t * t * terminal[0],
            w * w * w * p0[1]
                + 3.0 * w * w * t * p1[1]
                + 3.0 * w * t * t * p2[1]
                + t * t * t * terminal[1],
        ];
        path.push(q);
    }
    path
}

/// Profile a ramp (gore end first): beside the carriageway it follows the carriageway's
/// plane, then it runs to the terminal under the ramp grade cap, flat over the junction box.
/// Returns 3D points and bank (both in gore → terminal order, bank right of that travel).
fn profile_ramp(
    ring: &Ring,
    rp: &RampPath,
    z_gore: f32,
    z_term: f32,
    flat_term: f32,
) -> (Vec<V3>, Vec<f32>) {
    let path = resample(&rp.path, 4.0);
    let cum = cum2(&path);
    let len = cum[cum.len() - 1];
    let n = path.len();
    let dsr = len / (n - 1) as f32;
    let par_end = RAMP_PARALLEL_M.min(len * 0.5);
    let k_par = path
        .iter()
        .zip(&cum)
        .position(|(_, c)| *c >= par_end)
        .unwrap_or(n - 1);
    let kmax = max_vertical_curvature(RAMP.speed);
    let spread = 2.0 * smoothing_sigma(RAMP.grade, kmax, dsr);
    let fb = ((flat_term + spread) / dsr).ceil() as usize;
    let pb = (flat_term / dsr).ceil() as usize;
    // Row 162: as long as its paving overlaps the carriageway's (the parallel lane and the
    // taper beyond it), the ramp lies in the carriageway's plane; it climbs or drops towards
    // its terminal only after that (a ramp that left the plane at the end of the parallel lane
    // stood up to 0.7 m below the carriageway's paved edge further along, a wall in its lane).
    let overlap = |q: V2| {
        let (p, d, _, _) = ring.at(ring.project(q));
        let lat = geom::dot(geom::sub(q, p), geom::right_of(d)).abs();
        lat - RAMP_OVERLAP_HALF_M < CARRIAGEWAY_OFFSET_M + 1.5 * HW_LANE_M + OUTER_SHOULDER_M
    };
    let k_flush = path.iter().take_while(|q| overlap(**q)).count();
    let k_hold = k_par.max(k_flush).min((n - 1).saturating_sub(fb + 1));
    let z_hold = ring.plane_z(path[k_hold]);
    let desired: Vec<f32> = (0..n)
        .map(|i| {
            if i <= k_hold {
                ring.plane_z(path[i])
            } else {
                let t = (cum[i] - cum[k_hold]) / (len - cum[k_hold]).max(1e-3);
                z_hold + (z_term - z_hold) * smoothstep(0.0, 1.0, t)
            }
        })
        .collect();
    let anchors = [(0, 0, z_gore), ((n - 1).saturating_sub(fb), n - 1, z_term)];
    // Beside the carriageway the ramp shares its plane exactly (shoulder overlap): pinned
    // there, so the profile's smoothing rounds off only after it.
    let same = (k_par.saturating_sub(4) + 1).max(k_flush.min(k_hold));
    let pinned: Vec<(usize, usize, f32)> = (0..same).map(|i| (i, i, desired[i])).collect();
    let mut z = profile(
        &desired,
        dsr,
        &anchors,
        &pinned,
        &[],
        (1, pb),
        RAMP.grade,
        kmax,
    );
    z[..same].copy_from_slice(&desired[..same]);
    let hold_s = cum[k_flush.min(n - 1)].max(par_end);
    // Resample to 6 m for the road spec.
    let res = resample(&path, 6.0);
    let rc = cum2(&res);
    let pts: Vec<V3> = res
        .iter()
        .zip(&rc)
        .map(|(p, s)| {
            let f = s / dsr;
            let i = (f.floor() as usize).min(n - 2);
            let t = f - i as f32;
            [p[0], p[1], z[i] + (z[i + 1] - z[i]) * t]
        })
        .collect();
    // Bank: the carriageway's beside it, the curve's own further on, none at the terminal.
    let kappa = curvature(&res);
    let v2 = RAMP.speed * RAMP.speed;
    let spacing = (len / (res.len() - 1).max(1) as f32).max(1.0);
    let raw: Vec<f32> = kappa
        .iter()
        .map(|k| (k * v2 / 9.81 * 0.55).clamp(-RAMP.bank_max, RAMP.bank_max))
        .collect();
    let curve = smooth_series(&raw, (30.0 / spacing) as usize);
    // Beside the carriageway the ramp's cross slope (right of its path, gore → terminal) is
    // the carriageway plane's: the plane's gradient (grade along the ring, bank across it)
    // taken across the ramp's own direction. (The ring's bank alone missed the grade's share
    // where a diverging ramp runs ~12° off a 5.6 % carriageway: 2-5 cm at its edge.)
    let bank: Vec<f32> = res
        .iter()
        .zip(&rc)
        .zip(&curve)
        .enumerate()
        .map(|(i, ((p, s), c))| {
            let rs = ring.project(*p);
            let (_, d, _, b_ring) = ring.at(rs);
            let grade = 0.5 * (ring.at(rs + 1.0).2 - ring.at(rs - 1.0).2);
            let gradient = geom::add(
                geom::scale(d, grade),
                geom::scale(geom::right_of(d), b_ring),
            );
            let t = geom::normalize(geom::sub(
                res[(i + 1).min(res.len() - 1)],
                res[i.saturating_sub(1)],
            ));
            let b_plane = geom::dot(gradient, geom::right_of(t));
            // The carriageway's bank as long as the paving overlaps it (row 162: blending to
            // the curve's own bank there stood the ramp's edge 9 cm proud in the right lane),
            // then over 40 m to the curve's.
            let w = smoothstep(hold_s + 40.0, hold_s, *s);
            let fade = smoothstep(0.0, 60.0, len - s - flat_term);
            // Not clamped to the ramp's own limit: beside the carriageway it takes the
            // carriageway's bank exactly (Loop 9 banks up to 6.4 %, the ramp style 5 %: the
            // clamp left a 6 cm ridge at the off-ramp's inner edge in the right lane, where an
            // AI car at race speed crashed). The curve's part is clamped already.
            w * b_plane + (1.0 - w) * c * fade
        })
        .collect();
    (pts, bank)
}

/// A ramp's paving reaches this far from its centreline towards the carriageway, m (its widest
/// paved side; [`profile_ramp`] holds it in the carriageway's plane while they overlap).
const RAMP_OVERLAP_HALF_M: f32 = 3.4;

/// Every interchange ramp (gore nodes not created yet).
fn plan_ramps(g: &Gen, ring: &Ring) -> Vec<RampPath> {
    let total = ring.total();
    let mut ramps = Vec::new();
    for ic in &g.interchanges {
        let sc = ring.project(ic.centre);
        let mut sides = vec![(-1.0f32, ic.inner)];
        if let Some(out) = ic.outer {
            sides.push((1.0, out));
        }
        for (side, term) in sides {
            for off in [true, false] {
                // Upstream gore for the off-ramp, downstream for the on-ramp.
                let up = if off { -side } else { side };
                let gs = (sc + up * RAMP_SPAN_M).rem_euclid(total);
                ramps.push(RampPath {
                    gore: usize::MAX,
                    terminal: term,
                    gore_p: ring.offset(gs, side * CARRIAGEWAY_OFFSET_M),
                    gore_s: gs,
                    off,
                    path: ramp_path(ring, gs, side, off, g.nodes[term].pos),
                    dir: side,
                });
            }
        }
    }
    // Rest stops: off-ramp upstream of the centre, on-ramp downstream.
    for rs in &g.rest_stops {
        let sc = ring.project(rs.centre);
        for off in [true, false] {
            let up = if off { -rs.side } else { rs.side };
            let gs = (sc + up * REST_SPAN_M).rem_euclid(total);
            let term = if off { rs.t_in } else { rs.t_out };
            ramps.push(RampPath {
                gore: usize::MAX,
                terminal: term,
                gore_p: ring.offset(gs, rs.side * CARRIAGEWAY_OFFSET_M),
                gore_s: gs,
                off,
                path: ramp_path(ring, gs, rs.side, off, g.nodes[term].pos),
                dir: rs.side,
            });
        }
    }
    ramps
}

/// Height range for each interchange terminal from which all its ramps reach their gores
/// under the ramp grade cap (after the first profiling pass).
fn terminal_bounds(
    g: &Gen,
    specs: &[RoadSpec],
    loop_roads: &[usize],
    opt: &BuildOptions,
) -> BTreeMap<usize, (f32, f32)> {
    let ring = Ring::new(g, specs, loop_roads);
    let flat = flat_lengths(g, opt);
    let spread = 2.0 * smoothing_sigma(RAMP.grade, max_vertical_curvature(RAMP.speed), 4.0);
    let mut out: BTreeMap<usize, (f32, f32)> = BTreeMap::new();
    for rp in plan_ramps(g, &ring) {
        let c = cum2(&rp.path);
        let room = c[c.len() - 1] - RAMP_PARALLEL_M - flat[rp.terminal] - 2.0 * spread;
        let allow = RAMP.grade * room.max(20.0) * 0.8;
        // Beside the carriageway the ramp climbs or drops with it.
        let z = ring.plane_z(point_on(&rp.path, &c, RAMP_PARALLEL_M).0);
        let e = out
            .entry(rp.terminal)
            .or_insert((f32::NEG_INFINITY, f32::INFINITY));
        e.0 = e.0.max(z - allow);
        e.1 = e.1.min(z + allow);
    }
    for v in out.values_mut() {
        if v.0 > v.1 {
            let m = 0.5 * (v.0 + v.1);
            *v = (m, m);
        }
    }
    out
}

/// Divide Loop 9 into two one-way carriageways with a median barrier and add the
/// interchange ramps. Returns the builder roads (outer carriageway pieces, inner pieces,
/// every other road in draft order, ramps), their infos, the ring, the new index of each
/// draft (`None` for the loop's drafts) and the carriageway piece indices (outer first).
#[allow(clippy::type_complexity)]
fn divide_loop(
    g: &mut Gen,
    specs: Vec<RoadSpec>,
    loop_roads: &[usize],
    skip: &[usize],
    opt: &BuildOptions,
) -> (
    Vec<RoadSpec>,
    Vec<RoadInfo>,
    Ring,
    Vec<Option<usize>>,
    Vec<usize>,
    Vec<usize>,
) {
    let ring = Ring::new(g, &specs, loop_roads);
    let total = ring.total();
    let o = CARRIAGEWAY_OFFSET_M;
    let flat = flat_lengths(g, opt);

    // Gores: (ring arc, node) per carriageway, and the ramps.
    let mut outer_cuts: Vec<(f32, usize)> = Vec::new();
    let mut inner_cuts: Vec<(f32, usize)> = Vec::new();
    let mut ramps = plan_ramps(g, &ring);
    for rp in &mut ramps {
        let p = rp.gore_p;
        let node = g.node(geom::xy(p));
        g.nodes[node].z = p[2];
        g.nodes[node].control = Some(ControlHint::Freeway);
        rp.gore = node;
        if rp.dir > 0.0 {
            outer_cuts.push((rp.gore_s, node));
        } else {
            inner_cuts.push((rp.gore_s, node));
        }
    }
    outer_cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    inner_cuts.sort_by(|a, b| a.0.total_cmp(&b.0));

    let mut out: Vec<RoadSpec> = Vec::new();
    let mut infos: Vec<RoadInfo> = Vec::new();
    let hw_info = RoadInfo {
        name: "Loop 9",
        speed: HIGHWAY.speed,
        grade: HIGHWAY.grade,
        deck: DeckRef::Ring,
    };
    let carriageway = HIGHWAY_CARRIAGEWAY;
    let mut pieces = Vec::new();
    for (cuts, side) in [(&outer_cuts, 1.0f32), (&inner_cuts, -1.0f32)] {
        let m = cuts.len();
        for k in 0..m {
            let (s_a, n_a) = cuts[k];
            let (mut s_b, n_b) = cuts[(k + 1) % m];
            if s_b <= s_a {
                s_b += total;
            }
            // Outer travels with the ring (a = cut k), inner against it (a = cut k + 1).
            let samples = if side > 0.0 {
                arc_samples(s_a, s_b, 10.0)
            } else {
                arc_samples(s_b, s_a, 10.0)
            };
            let pts: Vec<V3> = samples.iter().map(|s| ring.offset(*s, side * o)).collect();
            let bank: Vec<f32> = samples.iter().map(|s| side * ring.at(*s).3).collect();
            let (a, b) = if side > 0.0 { (n_a, n_b) } else { (n_b, n_a) };
            let mut r = spec_from(a, b, &pts, bank, &carriageway);
            r.shoulder_left_m = MEDIAN_SHOULDER_M;
            r.shoulder_right_m = OUTER_SHOULDER_M;
            r.median_barrier = side > 0.0;
            pieces.push(out.len());
            out.push(r);
            infos.push(hw_info);
        }
    }
    let n_outer = outer_cuts.len();
    let mut remap = vec![None; specs.len()];
    for (i, sp) in specs.into_iter().enumerate() {
        if loop_roads.contains(&i) || skip.contains(&i) {
            continue;
        }
        remap[i] = Some(out.len());
        let d = &g.drafts[i];
        infos.push(RoadInfo {
            name: d.name,
            speed: d.st.speed,
            grade: d.st.grade,
            deck: DeckRef::Draft(i),
        });
        out.push(sp);
    }
    let mut ramp_roads = Vec::new();
    for rp in &ramps {
        let (pts, bank) = profile_ramp(
            &ring,
            rp,
            g.nodes[rp.gore].z,
            g.nodes[rp.terminal].z,
            flat[rp.terminal],
        );
        let mut pts = pts;
        let mut bank = bank;
        let (a, b) = if rp.off {
            (rp.gore, rp.terminal)
        } else {
            // Travel terminal → gore: reverse, and the bank's "right" flips with it.
            pts.reverse();
            bank.reverse();
            for b in &mut bank {
                *b = -*b;
            }
            (rp.terminal, rp.gore)
        };
        let mut r = spec_from(a, b, &pts, bank, &RAMP);
        r.shoulder_left_m = 0.6;
        r.shoulder_right_m = 1.5;
        ramp_roads.push(out.len());
        out.push(r);
        infos.push(RoadInfo {
            name: "Loop 9 ramp",
            speed: RAMP.speed,
            grade: RAMP.grade,
            deck: DeckRef::None,
        });
    }
    debug_assert!(pieces[..n_outer].iter().all(|p| out[*p].median_barrier));
    (out, infos, ring, remap, pieces, ramp_roads)
}

/// Plan-view centreline of a road spec.
fn spec_path(nodes: &[NodeSpec], r: &RoadSpec) -> Vec<V2> {
    let mut p = vec![nodes[r.a].pos];
    p.extend_from_slice(&r.via);
    p.push(nodes[r.b].pos);
    p
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
/// point index)` says whether a surface point is a bridge deck (left uncarved, ground kept
/// below it).
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
// Bridges
// ------------------------------------------------------------------------------------------

/// Name and kind of the bridge carrying road `road` over `channel` at `p`.
fn bridge_name(road: &str, channel: &Channel, p: V2) -> (String, BridgeKind) {
    if channel.kind == WaterwayKind::Canal {
        let name = if p[0] < -1000.0 {
            "Basin Bridge"
        } else if p[0] < -800.0 {
            "Mill Bridge"
        } else {
            "Lock Bridge"
        };
        return (name.into(), BridgeKind::Beam);
    }
    match road {
        "Loop 9" if p[1] > 0.0 => ("Vesper Viaduct".into(), BridgeKind::Viaduct),
        "Loop 9" => ("Mouth Viaduct".into(), BridgeKind::Viaduct),
        "Canal Street" => ("Iron Bridge".into(), BridgeKind::Truss),
        "Hollins Drive" => ("Hollins Gorge Bridge".into(), BridgeKind::Beam),
        "Lantern Way" => ("Lantern Bridge".into(), BridgeKind::Beam),
        other => (format!("{other} Bridge"), BridgeKind::Beam),
    }
}

/// Every deck stretch of a built road over a river or canal.
fn find_bridges(built: &Built, infos: &[RoadInfo], nat: &Natural) -> Vec<Bridge> {
    let mut out = Vec::new();
    for (si, s) in built.surfaces.iter().enumerate() {
        for c in &nat.channels {
            let n = s.centre.len();
            let over: Vec<bool> = s
                .centre
                .iter()
                .map(|p| {
                    c.near(geom::xy(*p))
                        .is_some_and(|(d, zw, half)| d < half + 1.0 && p[2] > zw + 1.0)
                })
                .collect();
            let mut k = 0;
            while k < n {
                if !over[k] {
                    k += 1;
                    continue;
                }
                let start = k;
                while k < n && over[k] {
                    k += 1;
                }
                let (a, b) = (start.saturating_sub(2), (k + 1).min(n - 1));
                let centre = s.centre[a..=b].to_vec();
                if centre.len() < 2 {
                    continue;
                }
                let mid = geom::xy(centre[centre.len() / 2]);
                let (name, kind) = bridge_name(infos[si].name, c, mid);
                out.push(Bridge {
                    name,
                    kind,
                    surface: si as u32,
                    centre,
                    half_width_m: s.left_m.max(s.right_m),
                });
            }
        }
    }
    out
}

/// Steel truss over a truss bridge: posts beside both paved edges every ~8 m, top chords and
/// cross beams 5.5 m over the deck (cars pass under).
fn truss(b: &Bridge) -> Vec<Structure> {
    let pts: Vec<V2> = b.centre.iter().map(|p| geom::xy(*p)).collect();
    let cum = cum2(&pts);
    let len = cum[cum.len() - 1];
    let n = ((len / 8.0).round() as usize).max(1);
    let half = b.half_width_m;
    let mut out = Vec::new();
    let frame = |s: f32| {
        let (p, t) = point_on(&pts, &cum, s);
        let c3 = geom::point_at(&b.centre, &geom::cumulative(&b.centre), s).0;
        (p, t, c3[2])
    };
    let quad = |c: V2, t: V2, a0: f32, a1: f32, l0: f32, l1: f32| {
        let r = geom::right_of(t);
        let mut fp: Vec<V2> = [(a0, l0), (a1, l0), (a1, l1), (a0, l1)]
            .iter()
            .map(|(a, l)| geom::add(c, geom::add(geom::scale(t, *a), geom::scale(r, *l))))
            .collect();
        if signed_area(&fp) < 0.0 {
            fp.reverse();
        }
        fp
    };
    for k in 0..=n {
        let s = len * k as f32 / n as f32;
        let (p, t, z) = frame(s);
        for side in [-1.0f32, 1.0] {
            let l = side * (half + 0.45);
            out.push(Structure {
                kind: StructureKind::Truss,
                footprint: quad(p, t, -0.18, 0.18, l - 0.18, l + 0.18),
                z0_m: z - 0.3,
                z1_m: z + 5.9,
            });
        }
        if k % 2 == 0 {
            out.push(Structure {
                kind: StructureKind::Truss,
                footprint: quad(p, t, -0.2, 0.2, -half - 0.65, half + 0.65),
                z0_m: z + 5.5,
                z1_m: z + 5.9,
            });
        }
        if k < n {
            let s1 = len * (k + 1) as f32 / n as f32;
            let (_, _, z1) = frame(s1);
            let seg = s1 - s;
            for side in [-1.0f32, 1.0] {
                let l = side * (half + 0.45);
                out.push(Structure {
                    kind: StructureKind::Truss,
                    footprint: quad(p, t, 0.0, seg, l - 0.2, l + 0.2),
                    z0_m: z.max(z1) + 5.5,
                    z1_m: z.max(z1) + 5.9,
                });
            }
        }
    }
    out
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
    lo: V2,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<(u32, u32)>>,
}

impl SurfaceGrid {
    fn new(built: &Built, lo: V2, hi: V2) -> Self {
        let cell = 40.0;
        let nx = ((hi[0] - lo[0]) / cell).ceil() as usize + 1;
        let ny = ((hi[1] - lo[1]) / cell).ceil() as usize + 1;
        let mut g = Self {
            cell,
            lo,
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
        let o = self.lo;
        let (i0, i1) = (ci(lo[0], o[0], self.nx), ci(hi[0], o[0], self.nx));
        let (j0, j1) = (ci(lo[1], o[1], self.ny), ci(hi[1], o[1], self.ny));
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

// ------------------------------------------------------------------------------------------
// Landmarks and hideouts
// ------------------------------------------------------------------------------------------

/// A free, dry, fairly level lot (axis-aligned) for a landmark or hideout.
struct Lot {
    obb: Obb,
    zmin: f32,
    zmax: f32,
}

/// What a lot must satisfy.
struct LotRule {
    /// Half size along x and y, m.
    half: V2,
    /// Minimum distance from any paved edge, m.
    clear: f32,
    /// Some probe must be at most this far from a paved edge (reachable from the road).
    reach: Option<f32>,
    /// Allowed ground height range over the lot, m.
    relief: f32,
}

/// Search outwards from `target` (square rings 6 m apart, up to 180 m) for the first lot
/// that satisfies `rule` and overlaps nothing in `taken`.
fn find_lot(
    grid: &SurfaceGrid,
    built: &Built,
    hf: &Heightfield,
    nat: &Natural,
    taken: &[Obb],
    target: V2,
    rule: &LotRule,
) -> Option<Lot> {
    let fits = |c: V2| -> Option<Lot> {
        let obb = Obb {
            c,
            f: [0.0, 1.0],
            hl: rule.half[1],
            hw: rule.half[0],
        };
        let k = obb_corners(&obb);
        let mid = |a: V2, b: V2| geom::scale(geom::add(a, b), 0.5);
        let probes = [
            k[0],
            k[1],
            k[2],
            k[3],
            c,
            mid(k[0], k[1]),
            mid(k[1], k[2]),
            mid(k[2], k[3]),
            mid(k[3], k[0]),
        ];
        let clear: Vec<f32> = probes.iter().map(|q| grid.clearance(built, *q)).collect();
        if clear.iter().any(|d| *d < rule.clear) {
            return None;
        }
        if rule
            .reach
            .is_some_and(|r| clear.iter().copied().fold(f32::INFINITY, f32::min) > r)
        {
            return None;
        }
        if probes.iter().any(|q| nat.wet(*q, 3.0)) {
            return None;
        }
        let zs: Vec<f32> = probes.iter().map(|q| hf.height_at(*q)).collect();
        let zmin = zs.iter().copied().fold(f32::INFINITY, f32::min);
        let zmax = zs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        if zmin < 1.0 || zmax - zmin > rule.relief {
            return None;
        }
        if taken.iter().any(|t| geom::obb_overlap(t, &obb)) {
            return None;
        }
        Some(Lot { obb, zmin, zmax })
    };
    for r in 0..=30i32 {
        let mut ring: Vec<(i32, i32)> = Vec::new();
        if r == 0 {
            ring.push((0, 0));
        }
        for k in -r..r {
            ring.extend([(k, -r), (r, k), (-k, r), (-r, -k)]);
        }
        for (dx, dy) in ring {
            let c = [target[0] + 6.0 * dx as f32, target[1] + 6.0 * dy as f32];
            if let Some(l) = fits(c) {
                return Some(l);
            }
        }
    }
    None
}

/// Regular octagon (counter-clockwise) of circumradius `r` around `c`.
fn octagon(c: V2, r: f32) -> Vec<V2> {
    (0..8)
        .map(|k| {
            let (s, co) = wp_math::sin_cos((k as f32 + 0.5) / 8.0 * std::f32::consts::TAU);
            [c[0] + co * r, c[1] + s * r]
        })
        .collect()
}

/// Axis-aligned square of half size `h` around `c`.
fn square(c: V2, h: f32) -> Vec<V2> {
    rect(c[0] - h, c[1] - h, c[0] + h, c[1] + h)
}

/// The parts of a landmark at `c` on ground `z` (relative heights), and its height.
fn landmark_parts(kind: LandmarkKind, c: V2, z: f32) -> (Vec<(Vec<V2>, f32, f32)>, f32) {
    let [x, y] = c;
    let parts: Vec<(Vec<V2>, f32, f32)> = match kind {
        LandmarkKind::Tower => vec![
            (square(c, 13.0), 0.0, 150.0),
            (square(c, 9.0), 150.0, 185.0),
            (square(c, 1.25), 185.0, 230.0),
        ],
        LandmarkKind::Lighthouse => vec![
            (octagon(c, 6.0), 0.0, 3.0),
            (octagon(c, 3.6), 3.0, 30.0),
            (octagon(c, 4.4), 30.0, 31.0),
            (octagon(c, 2.6), 31.0, 35.0),
            (octagon(c, 1.2), 35.0, 37.0),
        ],
        LandmarkKind::Observatory => vec![
            (octagon(c, 10.0), 0.0, 7.0),
            (octagon(c, 7.5), 7.0, 10.0),
            (octagon(c, 4.5), 10.0, 12.0),
            (octagon(c, 1.5), 12.0, 13.0),
        ],
        LandmarkKind::Chimney => vec![
            (octagon(c, 4.5), 0.0, 6.0),
            (octagon(c, 3.2), 6.0, 55.0),
            (octagon(c, 3.8), 55.0, 56.5),
        ],
        LandmarkKind::Crane => {
            let mut v: Vec<(Vec<V2>, f32, f32)> = Vec::new();
            for (dx, dy) in [(-8.0, -6.0), (8.0, -6.0), (8.0, 6.0), (-8.0, 6.0)] {
                v.push((square([x + dx, y + dy], 0.6), 0.0, 40.0));
            }
            for dy in [-6.0, 6.0] {
                v.push((
                    rect(x - 9.0, y + dy - 0.6, x + 9.0, y + dy + 0.6),
                    37.0,
                    40.0,
                ));
            }
            // Boom out over the shore, the back reach over the quay; A-frame on top.
            v.push((rect(x - 1.5, y - 52.0, x + 1.5, y + 26.0), 40.0, 42.5));
            v.push((rect(x - 1.0, y + 3.0, x + 1.0, y + 6.0), 42.5, 55.0));
            v
        }
        LandmarkKind::WaterTower => {
            let mut v: Vec<(Vec<V2>, f32, f32)> = Vec::new();
            for (dx, dy) in [(-4.0, -4.0), (4.0, -4.0), (4.0, 4.0), (-4.0, 4.0)] {
                v.push((square([x + dx, y + dy], 0.4), 0.0, 24.5));
            }
            v.push((octagon(c, 6.5), 24.0, 32.0));
            v.push((octagon(c, 3.0), 32.0, 34.0));
            v
        }
        LandmarkKind::Bridge | LandmarkKind::Tunnel => Vec::new(),
    };
    let top = parts.iter().map(|p| p.2).fold(0.0f32, f32::max);
    (
        parts
            .into_iter()
            .map(|(fp, a, b)| (fp, z + a, z + b))
            .collect(),
        top,
    )
}

/// Place the landmarks (tower, lighthouse, observatory, chimney, cranes, water tower; plus
/// the Iron Bridge and Hollins Tunnel already built) and the canopy hideouts (plus the
/// tunnel). Returns the lots to keep clear of ordinary buildings.
fn place_landmarks_and_hideouts(
    features: &mut CityFeatures,
    built: &Built,
    hf: &Heightfield,
    nat: &Natural,
    covered: &[CoveredSpan],
    grid: &SurfaceGrid,
) -> Vec<Obb> {
    let mut taken: Vec<Obb> = Vec::new();
    let landmarks: [(&str, LandmarkKind, V2, LotRule); 8] = [
        (
            "Lantern Tower",
            LandmarkKind::Tower,
            [50.0, 100.0],
            LotRule {
                half: [13.0, 13.0],
                clear: 3.0,
                reach: None,
                relief: 4.0,
            },
        ),
        (
            "Vesper Point Lighthouse",
            LandmarkKind::Lighthouse,
            [1560.0, -1080.0],
            LotRule {
                half: [6.0, 6.0],
                clear: 6.0,
                reach: None,
                relief: 3.0,
            },
        ),
        (
            "Hollins Observatory",
            LandmarkKind::Observatory,
            [-1085.0, 1150.0],
            LotRule {
                half: [10.5, 10.5],
                clear: 3.0,
                reach: None,
                relief: 7.0,
            },
        ),
        (
            "Calder Mill Chimney",
            LandmarkKind::Chimney,
            [-1010.0, -650.0],
            LotRule {
                half: [4.5, 4.5],
                clear: 4.0,
                reach: None,
                relief: 3.0,
            },
        ),
        (
            "Gantry Crane 1",
            LandmarkKind::Crane,
            [600.0, -985.0],
            LotRule {
                half: [9.0, 7.0],
                clear: 2.5,
                reach: None,
                relief: 2.5,
            },
        ),
        (
            "Gantry Crane 2",
            LandmarkKind::Crane,
            [820.0, -985.0],
            LotRule {
                half: [9.0, 7.0],
                clear: 2.5,
                reach: None,
                relief: 2.5,
            },
        ),
        (
            "Gantry Crane 3",
            LandmarkKind::Crane,
            [1060.0, -985.0],
            LotRule {
                half: [9.0, 7.0],
                clear: 2.5,
                reach: None,
                relief: 2.5,
            },
        ),
        (
            "Larkspur Water Tower",
            LandmarkKind::WaterTower,
            [-1150.0, 180.0],
            LotRule {
                half: [7.0, 7.0],
                clear: 4.0,
                reach: None,
                relief: 4.0,
            },
        ),
    ];
    for (name, kind, target, rule) in &landmarks {
        let Some(lot) = find_lot(grid, built, hf, nat, &taken, *target, rule) else {
            continue;
        };
        let base = lot.zmin - 0.3;
        let (parts, top) = landmark_parts(*kind, lot.obb.c, base);
        let first = features.structures.len() as u32;
        for (fp, z0, z1) in parts {
            features.structures.push(Structure {
                kind: StructureKind::Landmark,
                footprint: fp,
                z0_m: z0,
                z1_m: z1,
            });
        }
        features.landmarks.push(Landmark {
            name: (*name).into(),
            kind: *kind,
            pos: [lot.obb.c[0], lot.obb.c[1], base],
            height_m: top,
            structures: (first..features.structures.len() as u32).collect(),
        });
        taken.push(Obb {
            hl: lot.obb.hl + 4.0,
            hw: lot.obb.hw + 4.0,
            ..lot.obb
        });
    }
    // The truss bridge and the tunnel are landmarks too (their parts are already built).
    let truss_parts: Vec<u32> = (0..features.structures.len() as u32)
        .filter(|i| features.structures[*i as usize].kind == StructureKind::Truss)
        .collect();
    if let Some(b) = features
        .bridges
        .iter()
        .find(|b| b.kind == BridgeKind::Truss)
    {
        let c = b.centre[b.centre.len() / 2];
        features.landmarks.push(Landmark {
            name: b.name.clone(),
            kind: LandmarkKind::Bridge,
            pos: c,
            height_m: 5.9,
            structures: truss_parts,
        });
    }
    // The Hollins Tunnel (a covered span, its walls and roof are the span's): a landmark and
    // the hideout the helicopter cannot see into.
    if let Some(c) = covered.iter().find(|c| c.name == "Hollins Tunnel") {
        let shape = CoverShape::new(c);
        let len = shape.length();
        let mid = geom::point_at(&c.centre, &shape.cum, len * 0.5).0;
        features.landmarks.push(Landmark {
            name: c.name.clone(),
            kind: LandmarkKind::Tunnel,
            pos: mid,
            height_m: c.clearance_m,
            structures: Vec::new(),
        });
        features.hideouts.push(Hideout {
            name: c.name.clone(),
            kind: HideoutKind::Tunnel,
            pos: mid,
            radius_m: 0.5 * len,
        });
    }
    // Canopies: a roof slab 4.2 m over the lot on pillars, close to a road.
    let canopies: [(&str, V2, V2); 5] = [
        ("Calder Mill Shed", [-950.0, -640.0], [18.0, 12.0]),
        ("Gantry Row Reefer Dock", [660.0, -840.0], [16.0, 10.0]),
        ("Larkspur Carports", [-1000.0, -210.0], [14.0, 6.0]),
        ("Eastgate Parking Deck", [820.0, 40.0], [20.0, 15.0]),
        ("Lantern Row Arcade", [-150.0, -200.0], [12.0, 8.0]),
    ];
    for (name, target, half) in canopies {
        let rule = LotRule {
            half,
            clear: 2.0,
            reach: Some(12.0),
            relief: 2.5,
        };
        let Some(lot) = find_lot(grid, built, hf, nat, &taken, target, &rule) else {
            continue;
        };
        let c = lot.obb.c;
        let roof = lot.zmax + 4.2;
        features.structures.push(Structure {
            kind: StructureKind::Canopy,
            footprint: rect(
                c[0] - half[0],
                c[1] - half[1],
                c[0] + half[0],
                c[1] + half[1],
            ),
            z0_m: roof,
            z1_m: roof + 0.5,
        });
        let cols = ((2.0 * half[0] / 12.0).ceil() as usize).max(1);
        let rows = ((2.0 * half[1] / 12.0).ceil() as usize).max(1);
        for a in 0..=cols {
            for b in 0..=rows {
                if a != 0 && a != cols && b != 0 && b != rows {
                    continue;
                }
                let px = c[0] - half[0] + 0.6 + (2.0 * half[0] - 1.2) * a as f32 / cols as f32;
                let py = c[1] - half[1] + 0.6 + (2.0 * half[1] - 1.2) * b as f32 / rows as f32;
                features.structures.push(Structure {
                    kind: StructureKind::Pillar,
                    footprint: square([px, py], 0.3),
                    z0_m: lot.zmin - 0.3,
                    z1_m: roof,
                });
            }
        }
        features.hideouts.push(Hideout {
            name: name.into(),
            kind: HideoutKind::Canopy,
            pos: [c[0], c[1], hf.height_at(c)],
            radius_m: half[0].min(half[1]),
        });
        taken.push(Obb {
            hl: lot.obb.hl + 14.0,
            hw: lot.obb.hw + 14.0,
            ..lot.obb
        });
    }
    taken
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
///
/// Residential and suburban frontages (playtest row 95) are laid out as houses on lots
/// instead ([`residential::plan_lot`]): front yards, garages and driveways, fences, hedges
/// and yard trees, a corner shop or two per district, and street trees on the verge
/// ([`surface::verge_street`]). Returns the buildings and the decor.
///
/// Clear of `reserved` (hand-placed buildings, Ashpeak County) and `reserved_decor`, only
/// where `allowed`, and none along the `no_buildings` roads (the Loop 9 rest stops), on the
/// landmark and hideout `lots` or in the water (`wet`: the river and canal channels).
#[allow(clippy::too_many_arguments)]
fn buildings(
    rng: &mut Rng,
    built: &Built,
    grid: &SurfaceGrid,
    nodes: &[NodeSpec],
    zones: &[Zone],
    hf: &Heightfield,
    dead_ends: &[V2],
    no_buildings: &[usize],
    reserved: &[Building],
    reserved_decor: &[Decor],
    allowed: &dyn Fn(V2) -> bool,
    lots: &[Obb],
    wet: &dyn Fn(V2) -> bool,
    skip: &dyn Fn(usize) -> bool,
) -> (Vec<Building>, Vec<Decor>) {
    // Junction / bend areas with the turning paths through them, plus a sidewalk.
    let node_r = node_radii(built, nodes);
    let junctions: Vec<(V2, f32)> = nodes
        .iter()
        .zip(&node_r)
        .filter(|(_, r)| **r > 0.0)
        .map(|(n, r)| (n.pos, r + BUILDING_CLEARANCE_M))
        .collect();
    // Signalised / controlled crossings: corner shops stand next to them.
    let crossings: Vec<V2> = built
        .intersections
        .iter()
        .map(|ix| geom::xy(built.nodes[ix.node as usize].pos))
        .collect();
    let mut placed = residential::ObbIndex::default();
    let mut solid_decor = residential::ObbIndex::default();
    for b in reserved {
        placed.insert(footprint_obb(&b.footprint));
    }
    for o in lots {
        placed.insert(*o);
    }
    for d in reserved_decor {
        solid_decor.insert(Obb {
            c: [d.pos[0], d.pos[1]],
            f: geom::dir_of_yaw(d.yaw),
            hl: d.size[1] * 0.5,
            hw: d.size[0] * 0.5,
        });
    }
    let mut shops: BTreeMap<usize, usize> = BTreeMap::new();
    let mut out = Vec::new();
    let mut decor: Vec<Decor> = Vec::new();
    // Clear of every paved edge by `clear`, of junction areas and dead-end turnarounds.
    let site_ok = |o: &Obb, clear: f32| {
        let corners = obb_corners(o);
        let probes = [corners[0], corners[1], corners[2], corners[3], o.c];
        probes.iter().all(|q| grid.clearance(built, *q) >= clear)
            && !probes.iter().any(|q| wet(*q))
            && !probes
                .iter()
                .any(|q| junctions.iter().any(|(c, r)| geom::dist(*q, *c) < *r))
            && !probes
                .iter()
                .any(|q| dead_ends.iter().any(|d| geom::dist(*q, *d) < 20.0))
    };
    for (si, s) in built.surfaces.iter().enumerate() {
        // Rest stops: a lit parking road, no houses along it. Skipped streets keep the lots
        // the caller reserved for them (a region's copy of Port Vesper's own).
        if s.highway || no_buildings.contains(&(s.road as usize)) || skip(si) {
            continue;
        }
        let pts: Vec<V2> = s.centre.iter().map(|p| geom::xy(*p)).collect();
        let cum = cum2(&pts);
        let len = cum[cum.len() - 1];
        // Lane speed and kind of this road (for the verge rule).
        let lane = built.road_lanes[s.road as usize]
            .fwd
            .first()
            .or(built.road_lanes[s.road as usize].bwd.first())
            .map(|l| &built.lanes[*l as usize]);
        for side in [1.0f32, -1.0] {
            let edge = if side > 0.0 { s.right_m } else { s.left_m };
            // Sidewalk-top height `off` beyond this side's paved edge at arc length `a`.
            let walk_z = |a: f32, off: f32| {
                let (zc, bank) = surface_z_bank(s, &cum, a);
                zc + bank * side * (edge + off) + KERB_H_M
            };
            // Top of the lot behind this side's sidewalk at arc length `a`.
            let lot_z = |a: f32| walk_z(a, SIDEWALK_W_M);
            // Point `off` beyond this side's paved edge at arc length `a`.
            let beside = |a: f32, off: f32| {
                let (q, tq) = point_on(&pts, &cum, a.clamp(0.0, len));
                geom::add(q, geom::scale(geom::right_of(tq), side * (edge + off)))
            };
            // Whether the level lot exists behind the sidewalk at `a` (not beside a deck).
            let has_lot =
                |a: f32| (hf.height_at(beside(a, SIDEWALK_W_M + 1.75)) - lot_z(a)).abs() <= 0.3;
            let mut drive_mouths: Vec<f32> = Vec::new();
            let mut at = 8.0;
            let mut after_skip = true;
            while at < len - 8.0 {
                let at0 = at;
                let (p, t) = point_on(&pts, &cum, at);
                let kind = zone_kind(zones, p);
                if !allowed(p) {
                    at += 12.0;
                    after_skip = true;
                    continue;
                }
                if residential::is_residential(kind) {
                    // A corner shop on the first lot after a crossing, at most two per zone.
                    let zone = zones
                        .iter()
                        .position(|z| geom::point_in_polygon(p, &z.polygon))
                        .unwrap_or(usize::MAX);
                    let near_crossing = crossings.iter().any(|c| geom::dist(*c, p) < 45.0);
                    let shop = after_skip
                        && near_crossing
                        && shops.get(&zone).copied().unwrap_or(0) < residential::SHOPS_PER_ZONE
                        && rng.chance(0.25);
                    let (lk, w) = residential::next_lot(rng, kind, shop);
                    let centre = at + w * 0.5;
                    at += w;
                    if centre > len - 8.0 {
                        break;
                    }
                    let (q, tq) = point_on(&pts, &cum, centre);
                    let out_n = geom::scale(geom::right_of(tq), side);
                    let along = if side > 0.0 {
                        tq
                    } else {
                        geom::scale(tq, -1.0)
                    };
                    let site = residential::LotSite {
                        origin: geom::add(q, geom::scale(out_n, edge + SIDEWALK_W_M)),
                        along,
                        out: out_n,
                        width: w,
                        district: kind,
                        kind: lk,
                    };
                    // The level lot at the point's own arc length (lots slope along the
                    // street), the terrain beyond it.
                    let ground = |g: V2| {
                        let rel = geom::sub(g, site.origin);
                        if geom::dot(rel, site.out) < LOT_W_M - 0.05 {
                            lot_z((centre + geom::dot(rel, tq)).clamp(0.0, len))
                        } else {
                            hf.height_at(g)
                        }
                    };
                    if !has_lot(centre - w * 0.4) || !has_lot(centre) || !has_lot(centre + w * 0.4)
                    {
                        after_skip = true;
                        continue;
                    }
                    let plan = residential::plan_lot(rng, &site, &ground);
                    let (main, mo) = &plan.buildings[0];
                    let shrink = |o: &Obb| Obb {
                        hl: o.hl - 0.05,
                        hw: o.hw - 0.05,
                        ..*o
                    };
                    let steep = {
                        let zs: Vec<f32> =
                            obb_corners(mo).iter().map(|c| hf.height_at(*c)).collect();
                        let lo = zs.iter().copied().fold(f32::INFINITY, f32::min);
                        let hi = zs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                        hi - lo > 4.0 || lo < 1.0
                    };
                    if steep
                        || !site_ok(mo, BUILDING_CLEARANCE_M)
                        || placed.clashes(&shrink(mo))
                        || solid_decor.clashes(mo)
                    {
                        after_skip = true;
                        continue;
                    }
                    after_skip = false;
                    if lk == residential::LotKind::Shop {
                        *shops.entry(zone).or_default() += 1;
                    }
                    let mut kept = vec![false; plan.buildings.len()];
                    kept[0] = true;
                    out.push(main.clone());
                    for (k, (b, o)) in plan.buildings.iter().enumerate().skip(1) {
                        if site_ok(o, BUILDING_CLEARANCE_M)
                            && !placed.clashes(&shrink(o))
                            && !solid_decor.clashes(o)
                        {
                            kept[k] = true;
                            out.push(b.clone());
                        }
                    }
                    for (k, (_, o)) in plan.buildings.iter().enumerate() {
                        if kept[k] {
                            placed.insert(*o);
                        }
                    }
                    for (k, d, o) in &plan.tied {
                        // The driveway crosses the lot from the sidewalk: only its far half
                        // needs the clearance.
                        if kept[*k] && !placed.clashes(&shrink(o)) {
                            decor.push(*d);
                            let u = geom::dot(geom::sub([d.pos[0], d.pos[1]], q), tq);
                            drive_mouths.push(centre + u);
                        }
                    }
                    for (d, o) in &plan.decor {
                        if site_ok(o, SIDEWALK_W_M + 0.2) && !placed.clashes(o) {
                            decor.push(*d);
                            solid_decor.insert(*o);
                        }
                    }
                    continue;
                }
                after_skip = false;
                let (lot, depth, gap) = match kind {
                    DistrictKind::Core => (30.0, 24.0, 2.0),
                    DistrictKind::Commercial => (24.0, 18.0, 3.0),
                    _ => (42.0, 30.0, 6.0),
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
                if !site_ok(&o, BUILDING_CLEARANCE_M) || placed.clashes(&o) {
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
                if fronts.iter().any(|a| {
                    (hf.height_at(beside(*a, SIDEWALK_W_M + FRONT_SETBACK_M)) - lot_z(*a)).abs()
                        > 0.3
                }) {
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
                out.push(Building::block(
                    fp,
                    storeys as f32 * 3.2 + rng.range_f32(0.0, 1.0) + (zmax - base),
                    base,
                ));
                placed.insert(o);
            }
            // Street trees on the verge, clear of driveway mouths, junctions and dead ends.
            let verge = lane.is_some_and(|l| {
                let mid = point_on(&pts, &cum, len * 0.5).0;
                surface::verge_street(Some(zone_kind(zones, mid)), l.kind, l.speed_limit_mps)
            });
            if !verge {
                continue;
            }
            let mut a = residential::STREET_TREE_SPACING_M * 0.5;
            while a < len {
                let here = a;
                a += residential::STREET_TREE_SPACING_M;
                let p = beside(here, surface::VERGE_W_M * 0.5);
                if !residential::is_residential(zone_kind(zones, p))
                    || drive_mouths
                        .iter()
                        .any(|m| (m - here).abs() < residential::TREE_DRIVEWAY_CLEAR_M)
                    || !has_lot(here)
                {
                    continue;
                }
                let trunk = Obb {
                    c: p,
                    f: [1.0, 0.0],
                    hl: residential::TRUNK_R_M,
                    hw: residential::TRUNK_R_M,
                };
                // On its own street's verge: no other street's pavement nearer than its own.
                if grid.clearance(built, p) < surface::VERGE_W_M * 0.5 - 0.1 {
                    continue;
                }
                if junctions.iter().any(|(c, r)| geom::dist(p, *c) < *r + 2.0)
                    || dead_ends.iter().any(|d| geom::dist(p, *d) < 25.0)
                    || placed.clashes(&trunk)
                {
                    continue;
                }
                decor.push(residential::street_tree(
                    rng,
                    p,
                    walk_z(here, surface::VERGE_W_M * 0.5),
                ));
            }
        }
    }
    (out, decor)
}

/// Oriented box around a footprint, aligned with its first edge.
/// For a region map: Port Vesper's structures, landmarks and hideouts exactly as `base`
/// (`vesper_v2`) has them, where the region placed the same one (same kind and footprint, or
/// same name): their heights follow `vesper_v2`'s finer terrain, so Port Vesper's canopies and
/// landmarks look the same on both maps. Structure indices are unchanged.
fn reuse_vesper_features(base: &RoadNetwork, features: &mut CityFeatures) {
    let bf = &base.features;
    for st in &mut features.structures {
        if let Some(b) = bf
            .structures
            .iter()
            .find(|b| b.kind == st.kind && b.footprint == st.footprint)
        {
            *st = b.clone();
        }
    }
    for lm in &mut features.landmarks {
        if let Some(b) = bf
            .landmarks
            .iter()
            .find(|b| b.name == lm.name && b.kind == lm.kind)
        {
            // Keep the region's structure indices (its own structures come first).
            lm.pos = b.pos;
            lm.height_m = b.height_m;
        }
    }
    for h in &mut features.hideouts {
        if let Some(b) = bf
            .hideouts
            .iter()
            .find(|b| b.name == h.name && b.kind == h.kind)
        {
            *h = b.clone();
        }
    }
}

/// How far a street's lots reach from its paved edge, m: Port Vesper buildings and decor this
/// close to a street a region changed (or added) are left to the region's own pass.
const REUSE_CLEAR_M: f32 = 45.0;

/// Whether two road surfaces are the same street, bit for bit (the road index aside).
fn same_surface(a: &RoadSurface, b: &RoadSurface) -> bool {
    a.centre == b.centre
        && a.bank == b.bank
        && a.left_m == b.left_m
        && a.right_m == b.right_m
        && a.highway == b.highway
        && a.sidewalk == b.sidewalk
        && a.median_barrier == b.median_barrier
        && a.material == b.material
}

/// For a region map: which of its surfaces are Port Vesper's, unchanged (`base` is
/// `vesper_v2`), and the buildings and decor of `base` it keeps. An item is kept when nothing
/// the region changed or added is near it: no other street within [`REUSE_CLEAR_M`] of it, no
/// clash with the region's own buildings, decor or landmark lots, and generic buildings are
/// allowed there.
fn reuse_vesper_lots(
    base: &RoadNetwork,
    built: &Built,
    extra_b: &[Building],
    extra_d: &[Decor],
    lots: &[Obb],
    allowed: &dyn Fn(V2) -> bool,
) -> (Vec<Building>, Vec<Decor>, Vec<bool>) {
    let key = |s: &RoadSurface| {
        s.centre
            .first()
            .map(|p| (p[0].to_bits(), p[1].to_bits(), s.centre.len()))
    };
    let mut by_key: BTreeMap<(u32, u32, usize), Vec<usize>> = BTreeMap::new();
    for (i, s) in base.surfaces.iter().enumerate() {
        if let Some(k) = key(s) {
            by_key.entry(k).or_default().push(i);
        }
    }
    let same: Vec<bool> = built
        .surfaces
        .iter()
        .map(|s| {
            key(s).is_some_and(|k| {
                by_key
                    .get(&k)
                    .is_some_and(|c| c.iter().any(|i| same_surface(&base.surfaces[*i], s)))
            })
        })
        .collect();
    // Segments of the changed and added streets, in 64 m cells.
    const CELL: f32 = 64.0;
    let cell = |q: V2| ((q[0] / CELL).floor() as i32, (q[1] / CELL).floor() as i32);
    let mut segs: Vec<(V2, V2, f32)> = Vec::new();
    let mut cells: BTreeMap<(i32, i32), Vec<usize>> = BTreeMap::new();
    for (s, _) in built.surfaces.iter().zip(&same).filter(|(_, same)| !**same) {
        let hw = s.left_m.max(s.right_m);
        for w in s.centre.windows(2) {
            let (a, b) = (geom::xy(w[0]), geom::xy(w[1]));
            let r = hw + REUSE_CLEAR_M;
            let (lo, hi) = (
                cell([a[0].min(b[0]) - r, a[1].min(b[1]) - r]),
                cell([a[0].max(b[0]) + r, a[1].max(b[1]) + r]),
            );
            for x in lo.0..=hi.0 {
                for y in lo.1..=hi.1 {
                    cells.entry((x, y)).or_default().push(segs.len());
                }
            }
            segs.push((a, b, hw));
        }
    }
    let near_changed = |q: V2| {
        cells.get(&cell(q)).is_some_and(|v| {
            v.iter().any(|i| {
                let (a, b, hw) = segs[*i];
                geom::point_segment_distance(q, a, b) - hw < REUSE_CLEAR_M
            })
        })
    };
    let mut region = residential::ObbIndex::default();
    for b in extra_b {
        region.insert(footprint_obb(&b.footprint));
    }
    for d in extra_d {
        region.insert(Obb {
            c: [d.pos[0], d.pos[1]],
            f: geom::dir_of_yaw(d.yaw),
            hl: d.size[1] * 0.5,
            hw: d.size[0] * 0.5,
        });
    }
    for o in lots {
        region.insert(*o);
    }
    let keep_at = |probes: &[V2]| probes.iter().all(|q| allowed(*q) && !near_changed(*q));
    let kept_b: Vec<Building> = base
        .buildings
        .iter()
        .filter(|b| keep_at(&b.footprint) && !region.clashes(&footprint_obb(&b.footprint)))
        .cloned()
        .collect();
    let kept_d: Vec<Decor> = base
        .decor
        .iter()
        .filter(|d| {
            let o = Obb {
                c: [d.pos[0], d.pos[1]],
                f: geom::dir_of_yaw(d.yaw),
                hl: d.size[1] * 0.5,
                hw: d.size[0] * 0.5,
            };
            keep_at(&[o.c]) && !region.clashes(&o)
        })
        .cloned()
        .collect();
    (kept_b, kept_d, same)
}

fn footprint_obb(fp: &[V2]) -> Obb {
    let f = if fp.len() >= 2 && geom::dist(fp[0], fp[1]) > 1e-3 {
        geom::normalize(geom::sub(fp[1], fp[0]))
    } else {
        [0.0, 1.0]
    };
    let r = geom::right_of(f);
    let (mut a0, mut a1, mut b0, mut b1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for q in fp {
        let (a, b) = (geom::dot(*q, f), geom::dot(*q, r));
        (a0, a1, b0, b1) = (a0.min(a), a1.max(a), b0.min(b), b1.max(b));
    }
    let (ca, cb) = (0.5 * (a0 + a1), 0.5 * (b0 + b1));
    Obb {
        c: geom::add(geom::scale(f, ca), geom::scale(r, cb)),
        f,
        hl: 0.5 * (a1 - a0),
        hw: 0.5 * (b1 - b0),
    }
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

/// Pose on the kerb lane travelling with a Loop 9 cover (`c.centre` runs with the loop),
/// `dist` m (along the loop) before its first portal (`entry`) or after its last.
fn loop_cover_pose(
    built: &Built,
    ring: &Ring,
    c: &CoveredSpan,
    entry: bool,
    dist: f32,
) -> Option<(V3, f32, LaneId)> {
    let n = c.centre.len();
    let b = if entry { c.centre[0] } else { c.centre[n - 1] };
    let s = ring.project(geom::xy(b)) + if entry { -dist } else { dist };
    let (q, dir, z, _) = ring.at(s);
    let p = geom::add(
        q,
        geom::scale(geom::right_of(dir), CARRIAGEWAY_OFFSET_M + HW_LANE_M),
    );
    lane_pose_near(built, p, z, dir)
}

/// Whether the road of cover `c` ends less than 60 m past its last portal (no room for an
/// "exit" route point on it).
fn exit_past_road_end(built: &Built, c: &CoveredSpan) -> bool {
    let s = &built.surfaces[c.road as usize];
    let n = c.centre.len();
    geom::dist(
        geom::xy(s.centre[s.centre.len() - 1]),
        geom::xy(c.centre[n - 1]),
    ) < 60.0
}

/// Pose on the drivable lane nearest `p` (within 8 m) travelling along `dir` at about height
/// `z` (±3 m: not a road over or under it).
fn lane_pose_near(built: &Built, p: V2, z: f32, dir: V2) -> Option<(V3, f32, LaneId)> {
    let mut best: Option<(LaneId, V3, f32, f32)> = None;
    for (i, l) in built.lanes.iter().enumerate() {
        if !l.kind.drivable() || l.polyline.len() < 2 {
            continue;
        }
        let cum = geom::cumulative(&l.polyline);
        let pr = geom::project(&l.polyline, &cum, p);
        if pr.s < 0.0 || pr.s > cum[cum.len() - 1] {
            continue;
        }
        let (q, d) = geom::point_at(&l.polyline, &cum, pr.s);
        // Same carriageway level (the loop climbs ≤ 6 %; not a road over or under it).
        if geom::dot(d, dir) < 0.9 || (q[2] - z).abs() > 3.0 {
            continue;
        }
        if best.as_ref().is_none_or(|x| pr.dist < x.3) {
            best = Some((i as LaneId, q, geom::yaw_of(d), pr.dist));
        }
    }
    best.filter(|x| x.3 < 8.0).map(|(l, q, yaw, _)| (q, yaw, l))
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
    generate_region(cfg, None)
}

/// A region generated together with Port Vesper as one map (one road network, one terrain):
/// Port Vesper is laid out first, unchanged, then the region adds its roads around it and
/// joins them to Port Vesper's edge roads. Implemented by [`ashpeak`].
trait Region {
    /// City id of the combined map.
    fn city_id(&self) -> &'static str;
    /// Source description of the combined map (`RoadNetwork::source`).
    fn description(&self, seed: u64) -> String;
    /// Map extent of the combined map (contains Port Vesper's `MAP_LO..MAP_HI`), m.
    fn extent(&self) -> (V2, V2);
    /// Natural ground of the combined map: `f(p, vesper_z)` (`vesper_z` is Port Vesper's own
    /// ground at `p`, meaningful near and inside its map). Own noise from `seed`.
    fn terrain(&self, seed: u64) -> Box<dyn Fn(V2, f32) -> f32>;
    /// Lay out the region's roads (after Port Vesper's layout).
    fn layout(&self, g: &mut Gen);
    /// Covered roads (tunnels) on the region's roads.
    fn covers(&self) -> Vec<CoverSpec> {
        Vec::new()
    }
    /// The region's zones (looked up before Port Vesper's, so they may overlap its catch-all).
    fn zones(&self) -> Vec<Zone>;
    /// Whether the generic street-front building pass may build at `p` (farmland and mountains
    /// place their own buildings, or none).
    fn generic_buildings_at(&self, _p: V2) -> bool {
        true
    }
    /// Hand-placed buildings and decor (landmarks, farms, construction sites, forests), placed
    /// before the generic pass, which keeps clear of them.
    fn extra_buildings(&self, _site: &mut Site) -> (Vec<Building>, Vec<Decor>) {
        (Vec::new(), Vec::new())
    }
    /// The region's points of interest.
    fn pois(&self, _site: &mut Site) -> Vec<Poi> {
        Vec::new()
    }
}

/// A covered road a [`Region`] plans: `road` is the draft name, `probe` a point within 40 m of
/// the draft, `p0`/`p1` the portals, `dip` how far the road is pinned below the lowest ground
/// over the span (0 = the profile is left alone).
#[derive(Debug, Clone, Copy)]
struct CoverSpec {
    road: &'static str,
    probe: V2,
    p0: V2,
    p1: V2,
    dip: f32,
    kind: CoverKind,
    name: &'static str,
}

/// The built map as a [`Region`] sees it when it places buildings and POIs.
struct Site<'a> {
    rng: &'a mut Rng,
    built: &'a Built,
    nodes: &'a [NodeSpec],
    drafts: &'a [Draft],
    /// Draft index → builder road index (`None` for Loop 9's drafts).
    remap: &'a [Option<usize>],
    hf: &'a Heightfield,
    grid: &'a SurfaceGrid,
    /// `(name, side, road)` of every open-freeway carriageway piece (side +1 = forward).
    divided: &'a [(&'static str, f32, usize)],
}

impl Site<'_> {
    /// Builder road of freeway `name`'s carriageway piece on `side` (+1 forward) nearest
    /// `near`, with its distance from `near`.
    fn carriageway(&self, name: &str, side: f32, near: V2) -> Option<(usize, f32)> {
        let mut best: Option<(usize, f32)> = None;
        for &(n, sd, r) in self.divided {
            if n != name || sd != side {
                continue;
            }
            let s = &self.built.surfaces[r];
            let cum = geom::cumulative(&s.centre);
            let d = geom::project(&s.centre, &cum, near).dist;
            if best.is_none_or(|b| d < b.1) {
                best = Some((r, d));
            }
        }
        best
    }

    /// Smallest distance from `p` to any paved edge, m (negative = on a road; highways count
    /// 12 m wider).
    fn clearance(&self, p: V2) -> f32 {
        self.grid.clearance(self.built, p)
    }

    /// Ground height (carved terrain) at `p`, m.
    fn ground(&self, p: V2) -> f32 {
        self.hf.height_at(p)
    }

    /// Builder road index of the draft named `name` nearest to `near` (within `reach` m).
    fn road_named(&self, name: &str, near: V2, reach: f32) -> Option<usize> {
        let mut best: Option<(f32, usize)> = None;
        for (k, d) in self.drafts.iter().enumerate() {
            if d.name != name {
                continue;
            }
            let Some(r) = self.remap.get(k).copied().flatten() else {
                continue;
            };
            let dist = project2(&d.path, &cum2(&d.path), near).1;
            if dist <= reach && best.is_none_or(|b| dist < b.0) {
                best = Some((dist, r));
            }
        }
        best.map(|b| b.1)
    }

    /// A POI on road `road`'s forward kerb lane (`fwd`, else backward) `s` m from its start
    /// (negative = from its end).
    fn lane_poi(&self, kind: PoiKind, name: &str, road: usize, s: f32, fwd: bool) -> Option<Poi> {
        lane_pose(self.built, road, s, fwd).map(|(p, yaw, lane)| Poi {
            kind,
            name: name.into(),
            pos: p,
            heading_rad: yaw,
            lane: Some(lane),
        })
    }

    /// A POI on the lane nearest to `p` (the "block" POIs: stations, garages, safehouses).
    fn block_poi(&self, kind: PoiKind, name: &str, p: V2) -> Option<Poi> {
        nearest_lane(self.built, p).map(|(lane, q, yaw)| Poi {
            kind,
            name: name.into(),
            pos: q,
            heading_rad: yaw,
            lane: Some(lane),
        })
    }
}

/// Generate Port Vesper alone (`region = None`) or Port Vesper plus a region as one map.
/// Salt of a region's own random stream (see [`generate_region`]).
const REGION_RNG_SALT: u64 = 0xa5_4ea7_c0de;

fn generate_region(cfg: &VesperConfig, region: Option<&dyn Region>) -> (RoadNetwork, VesperReport) {
    let mut rng = Rng::new(cfg.seed);
    let mut nat = Natural::new(&mut rng);
    let (lo, hi) = region.map_or((MAP_LO, MAP_HI), |r| r.extent());
    if let Some(r) = region {
        nat.ext = Some(r.terrain(cfg.seed));
    }
    let mut g = Gen {
        rng: Rng::new(cfg.seed ^ 0x5eed_c17e),
        nat,
        lo,
        hi,
        nodes: Vec::new(),
        index: BTreeMap::new(),
        drafts: Vec::new(),
        skipped: 0,
        interchanges: Vec::new(),
        rest_stops: Vec::new(),
        ramp_ends: Vec::new(),
        freeways: Vec::new(),
        links: Vec::new(),
        reserved: Vec::new(),
    };
    let (loop_drafts, hollins_draft, _knob, _lot, _r40w, _r40n) = g.layout();
    // The region draws from its own stream (layout, extra buildings, POIs), so Port Vesper's
    // draws (its buildings and decor) come out exactly as in `vesper_v2`: the combined map
    // can ship Port Vesper's greybox unchanged (ashpeak-port's de-dup).
    let mut region_rng = Rng::new(cfg.seed ^ REGION_RNG_SALT);
    if let Some(r) = region {
        std::mem::swap(&mut g.rng, &mut region_rng);
        r.layout(&mut g);
        std::mem::swap(&mut g.rng, &mut region_rng);
    }
    // Every road over the river and the canal is a bridge deck.
    plan_water_crossings(&mut g);
    let draft_crossings = plan_crossings(&mut g);
    plan_covers(&mut g, &region.map_or_else(Vec::new, |r| r.covers()));
    let opt = BuildOptions::default();
    // Two passes: the first places the highway, the second holds each interchange terminal
    // within reach of its ramps' gores (the highway itself comes out the same).
    let first = profile_roads(&mut g, &opt, &BTreeMap::new());
    let mut bounds = terminal_bounds(&g, &first, &loop_drafts, &opt);
    freeway_bounds(&g, &first, &opt, &mut bounds);
    let profiled = profile_roads(&mut g, &opt, &bounds);
    // Loop 9 is profiled as one road, then split into two carriageways plus ramps; from here
    // on road indices are builder indices (`remap` maps draft indices). A region's open
    // freeways are split the same way (their roads follow Loop 9's ramps).
    let lines = freeway_lines(&g, &profiled);
    let freeway_drafts: Vec<usize> = g.freeways.iter().flat_map(|f| f.drafts.clone()).collect();
    let (mut specs, mut infos, ring, remap, loop_roads, ramp_roads) =
        divide_loop(&mut g, profiled, &loop_drafts, &freeway_drafts, &opt);
    let fw = divide_freeways(&mut g, lines, &mut specs, &mut infos, &opt);
    let (specs, infos) = (specs, infos);
    let divided = |draft: usize| -> Vec<usize> {
        if loop_drafts.contains(&draft) {
            loop_roads.clone()
        } else {
            fw.pieces_of(&g, draft)
        }
    };
    let is_divided = |draft: usize| remap[draft].is_none();
    let road_of = |draft: usize| remap[draft].expect("not a divided draft");
    let hollins = road_of(hollins_draft);
    let mut crossings = Vec::new();
    for c in &draft_crossings {
        let upper_loop = is_divided(c.upper);
        let lower_loop = is_divided(c.lower);
        if !upper_loop && !lower_loop {
            crossings.push(Crossing {
                upper: road_of(c.upper),
                lower: road_of(c.lower),
                ..*c
            });
            continue;
        }
        // One crossing per carriageway.
        let other = road_of(if upper_loop { c.lower } else { c.upper });
        let op = spec_path(&g.nodes, &specs[other]);
        for pc in divided(if upper_loop { c.upper } else { c.lower }) {
            for (_, _, p) in path_crossings(&spec_path(&g.nodes, &specs[pc]), &op) {
                let (upper, lower) = if upper_loop { (pc, other) } else { (other, pc) };
                crossings.push(Crossing {
                    upper,
                    lower,
                    pos: p,
                    underpass: c.underpass,
                });
            }
        }
    }
    // Unused nodes (dropped optional roads) stay in the list without lanes; harmless.
    let mut built = builder::build(&g.nodes, &specs, &opt);
    feed_fan_starts(&mut built, &fw.fan_starts);
    let built = built;

    // Terrain: natural ground, carved to the roads except bridge decks.
    let mut hf = Heightfield::flat(g.lo, g.hi, cfg.terrain_cell_m, 0.0);
    for j in 0..hf.ny {
        for i in 0..hf.nx {
            let p = hf.vertex_pos(i, j);
            hf.z[(j * hf.nx + i) as usize] = g.nat.z(p);
        }
    }
    let drafts = &g.drafts;
    let nat = &g.nat;
    let surfaces = &built.surfaces;
    // Rest stops' parking roads sit at the highway's level beside it: on an embankment (the
    // ground is filled up to them), never on a deck.
    let rest_roads: Vec<usize> = g.rest_stops.iter().map(|r| road_of(r.road)).collect();
    let deck = |road: usize, k: usize| -> bool {
        if rest_roads.contains(&road) {
            return false;
        }
        let s = &surfaces[road];
        let p = s.centre[k];
        if is_elevated(s, k, &|q| nat.z(q), 3.0) {
            return true;
        }
        match infos[road].deck {
            DeckRef::Draft(di) => {
                let d = &drafts[di];
                if d.decks.is_empty() {
                    return false;
                }
                let cum = cum2(&d.path);
                let (s, _) = project2(&d.path, &cum, geom::xy(p));
                d.decks.iter().any(|(a, b)| s >= *a && s <= *b)
            }
            DeckRef::Ring => ring.in_deck(geom::xy(p)),
            DeckRef::Line(k) => fw.lines[k].in_deck(geom::xy(p)),
            DeckRef::None => false,
        }
    };
    carve(&mut hf, &built, &g.nodes, &deck);
    let covered = covered_spans(&g, &built, &ring, &loop_drafts, &remap, &divided);
    backfill_covers(&mut hf, &g.nat, &covered);
    // Bridges and waterways (the Hollins Tunnel is a covered span).
    let mut features = CityFeatures {
        bridges: find_bridges(&built, &infos, &g.nat),
        waterways: g.nat.channels.iter().map(Channel::waterway).collect(),
        ..Default::default()
    };
    let trusses: Vec<Structure> = features
        .bridges
        .iter()
        .filter(|b| b.kind == BridgeKind::Truss)
        .flat_map(truss)
        .collect();
    features.structures.extend(trusses);

    let mut zones = region.map_or_else(Vec::new, |r| r.zones());
    zones.extend(self::zones());
    let mut degree = vec![0usize; g.nodes.len()];
    for r in &specs {
        degree[r.a] += 1;
        degree[r.b] += 1;
    }
    let dead_ends: Vec<V2> = (0..g.nodes.len())
        .filter(|n| degree[*n] == 1)
        .map(|n| g.nodes[n].pos)
        .collect();
    let grid = SurfaceGrid::new(&built, g.lo, g.hi);
    let fw_pieces: Vec<(&'static str, f32, usize)> = fw
        .pieces
        .iter()
        .map(|&(k, side, r)| (g.freeways[k].name, side, r))
        .collect();
    let (extra_b, extra_d) = match region {
        Some(r) => r.extra_buildings(&mut Site {
            rng: &mut region_rng,
            built: &built,
            nodes: &g.nodes,
            drafts: &g.drafts,
            remap: &remap,
            hf: &hf,
            grid: &grid,
            divided: &fw_pieces,
        }),
        None => (Vec::new(), Vec::new()),
    };
    let generic_at = |p: V2| region.is_none_or(|r| r.generic_buildings_at(p));
    // Landmarks and canopy hideouts first: their lots are kept clear of buildings.
    let lots = place_landmarks_and_hideouts(&mut features, &built, &hf, &g.nat, &covered, &grid);
    let wet = |q: V2| g.nat.wet(q, 2.0);
    // A region keeps Port Vesper's own buildings and decor (generated by `vesper_v2` itself)
    // along every street it left unchanged, so Port Vesper looks the same on both maps; its
    // own generic pass fills only the other streets, clear of what is kept.
    let base = region.map(|_| {
        generate(&VesperConfig {
            seed: cfg.seed,
            ..VesperConfig::default()
        })
    });
    if let Some(base) = &base {
        reuse_vesper_features(base, &mut features);
    }
    let reused = base
        .as_ref()
        .map(|base| reuse_vesper_lots(base, &built, &extra_b, &extra_d, &lots, &generic_at));
    let (reuse_b, reuse_d, same) = match reused {
        Some(r) => r,
        None => (Vec::new(), Vec::new(), vec![false; built.surfaces.len()]),
    };
    let mut reserved_b = reuse_b.clone();
    reserved_b.extend(extra_b.iter().cloned());
    let mut reserved_d = reuse_d.clone();
    reserved_d.extend(extra_d.iter().cloned());
    let (generic_b, generic_d) = buildings(
        &mut g.rng,
        &built,
        &grid,
        &g.nodes,
        &zones,
        &hf,
        &dead_ends,
        &rest_roads,
        &reserved_b,
        &reserved_d,
        &generic_at,
        &lots,
        &wet,
        &|si| same[si],
    );
    let mut buildings = reuse_b;
    buildings.extend(generic_b);
    buildings.extend(extra_b);
    let mut decor = reuse_d;
    decor.extend(generic_d);
    decor.extend(extra_d);
    let cover_shapes: Vec<CoverShape> = covered.iter().map(CoverShape::new).collect();
    buildings.retain(|b| !in_cover_corridor(&cover_shapes, &b.footprint));
    decor.retain(|d| !in_cover_corridor(&cover_shapes, &[[d.pos[0], d.pos[1]]]));

    // POIs. The first race grid is the city-chase / free-drive spawn.
    let mut pois = Vec::new();
    // Race grids on Loop 9 use the outer (counter-clockwise) carriageway.
    let outer_loop: Vec<usize> = loop_roads
        .iter()
        .copied()
        .filter(|r| specs[*r].median_barrier)
        .collect();
    let path_len = |r: usize| {
        let p = spec_path(&g.nodes, &specs[r]);
        let c = cum2(&p);
        c[c.len() - 1]
    };
    let longest_loop = *outer_loop
        .iter()
        .max_by(|a, b| path_len(**a).total_cmp(&path_len(**b)).then(b.cmp(a)))
        .expect("loop has roads");
    let named = |n: &str| g.drafts.iter().position(|d| d.name == n).map_or(0, road_of);
    let blvd = named("Harbor Boulevard");
    let bluff = named("Bluff Road");
    let (ridge, spur) = (named("Ridge Road"), named("Knob Observatory Lot"));
    // The loop road over Hollins Heights' northern shoulder (the highway's big crest).
    let loop_north = *outer_loop
        .iter()
        .find(|r| {
            let p = spec_path(&g.nodes, &specs[**r]);
            project2(&p, &cum2(&p), [-1000.0, 1390.0]).1 < 40.0
        })
        .unwrap_or(&longest_loop);
    // Lantern Way's western approach into downtown (from Larkspur), heading east.
    let lantern = (0..g.drafts.len())
        .find(|k| {
            let d = &g.drafts[*k];
            d.name == "Lantern Way" && geom::dist(g.nodes[d.b].pos, [-300.0, 150.0]) < 1.0
        })
        .map_or(0, road_of);
    // The Hollins climb starts across the gorge bridge (a staggered grid does not fit on the
    // deck between its parapets).
    let ascent_s = features
        .bridges
        .iter()
        .find(|b| b.name == "Hollins Gorge Bridge")
        .and_then(|b| {
            let lane = *built.road_lanes[hollins].fwd.last()?;
            let l = &built.lanes[lane as usize];
            let cum = geom::cumulative(&l.polyline);
            let s_end = [b.centre[0], b.centre[b.centre.len() - 1]]
                .iter()
                .map(|q| geom::project(&l.polyline, &cum, geom::xy(*q)).s)
                .fold(0.0f32, f32::max);
            Some(s_end + 45.0)
        })
        .unwrap_or(12.0);
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
            ascent_s,
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
    // Rest stops: a named site in the middle of each parking road (night meets, bets).
    for rs in &g.rest_stops {
        if let Some((p, yaw, lane)) = lane_pose(&built, road_of(rs.road), REST_HALF_M, true) {
            pois.push(Poi {
                kind: PoiKind::RestStop,
                name: rs.name.into(),
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
        // Exit 120 m past the west portal: the Hollins Rest Area's off-ramp gore is ~210 m
        // past it (row 153).
        ("Hollins Tunnel", -200.0, 120.0),
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
        // On Loop 9 (divided, cut into pieces at its gores) the entry and exit sit
        // `-s0` / `s1` m (as positive distances) before and after the portals on the kerb lane
        // travelling with the loop; elsewhere `s0` / `s1` are along the road (negative: from
        // its end).
        let on_loop = built.surfaces[c.road as usize].median_barrier;
        for (suffix, s) in [
            ("entry", s0),
            ("lay-by", lay_by.unwrap_or(s0)),
            ("exit", s1),
        ] {
            let pose = if on_loop && suffix != "lay-by" {
                loop_cover_pose(&built, &ring, c, suffix == "entry", s.abs())
            } else if suffix == "exit" && exit_past_road_end(&built, c) {
                // The road ends right past the portal (Founders Cut at main's Uptown
                // interchange terminal): 40 m past the portal on the lane continuing ahead.
                let n = c.centre.len();
                let (a, b) = (c.centre[n - 2], c.centre[n - 1]);
                let dir = geom::normalize(geom::sub(geom::xy(b), geom::xy(a)));
                lane_pose_near(
                    &built,
                    geom::add(geom::xy(b), geom::scale(dir, 40.0)),
                    b[2],
                    dir,
                )
            } else {
                lane_pose(&built, c.road as usize, s, true)
            };
            if let Some((p, yaw, lane)) = pose {
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

    if let Some(r) = region {
        pois.extend(r.pois(&mut Site {
            rng: &mut region_rng,
            built: &built,
            nodes: &g.nodes,
            drafts: &g.drafts,
            remap: &remap,
            hf: &hf,
            grid: &grid,
            divided: &fw_pieces,
        }));
    }
    // Loop 9's median crossovers (after every other POI, so their indices stay put).
    pois.extend(crossovers::crossovers(
        &ring,
        &built,
        &loop_roads,
        &covered,
        &crossings,
    ));
    let (city_id, source) = match region {
        Some(r) => (r.city_id(), r.description(cfg.seed)),
        None => (
            CITY_ID,
            format!("procedural port_vesper v2 seed={}", cfg.seed),
        ),
    };
    let mut net = built.into_network(city_id, &source, None, [0.0, -50.0], zones, pois, buildings);
    net.decor = decor;
    net.bounds_min = [
        net.bounds_min[0].min(g.lo[0]),
        net.bounds_min[1].min(g.lo[1]),
    ];
    net.bounds_max = [
        net.bounds_max[0].max(g.hi[0]),
        net.bounds_max[1].max(g.hi[1]),
    ];
    net.terrain = Some(hf);
    net.covered = covered;
    net.features = features;
    let report = VesperReport {
        crossings,
        skipped_roads: g.skipped,
        hollins_drive: hollins,
        loop_roads,
        ramp_roads,
        road_names: infos.iter().map(|i| i.name).collect(),
        road_speeds: infos.iter().map(|i| i.speed).collect(),
        road_grades: infos.iter().map(|i| i.grade).collect(),
        freeway_roads: fw.pieces.iter().map(|p| p.2).collect(),
        freeway_ramps: fw.ramps.clone(),
        freeway_links: fw.links.clone(),
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
        let z = profile(&desired, ds, &anchors, &[], &[], (2, 2), 0.1, 0.02);
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

    #[test]
    fn a_deck_floor_rounds_off_within_the_curvature_limit() {
        // Loop 9's canal bridge: a 600 m flat road held 2.25 m up over a 90 m crossing. The
        // floor used to be pinned, leaving a 4.6 % grade break in 10 m at each end.
        let ds = 2.0;
        let (grade, kmax) = (0.06, max_vertical_curvature(30.0));
        let desired = vec![2.5f32; 301];
        let anchors = [(0, 2, 2.5), (298, 300, 2.5)];
        let floors = [(130, 175, 4.75)];
        let z = profile(&desired, ds, &anchors, &[], &floors, (2, 2), grade, kmax);
        for (i, v) in z.iter().enumerate().take(176).skip(130) {
            assert!(*v >= 4.75 - 1e-4, "{v} under the floor at {i}");
        }
        for i in 1..300 {
            let k = ((z[i + 1] - 2.0 * z[i] + z[i - 1]) / (ds * ds)).abs();
            assert!(k <= 1.25 * kmax, "curvature {k} at {i} (limit {kmax})");
        }
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
                // A ramp beside the carriageway lies in its plane (row 162): its cross slope is
                // the plane's across the ramp's own direction, bank·cos θ + grade·sin θ, up to
                // 7.6 % where it runs ~12° off a 5.6 % carriageway.
                let bank_max = if s.highway && !s.median_barrier {
                    0.0801
                } else {
                    0.0601
                };
                assert!(
                    s.bank[k].abs() <= bank_max,
                    "{} bank {}",
                    rep.road_names[r],
                    s.bank[k]
                );
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
        // Freeway merges / diverges (radius 0) lie on the graded highway, not in a flat box.
        for ix in net.intersections.iter().filter(|ix| ix.radius_m > 0.0) {
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
        // Route 40 West and North plus five interchange crossroads, each over or under both
        // carriageways.
        assert_eq!(rep.crossings.len(), 14, "{:?}", rep.crossings);
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

    /// Playtest row 153: rest stops on Loop 9 — a named site on a lit parking road with parked
    /// cars, reached from the carriageway by an off-ramp and left by an on-ramp, no houses.
    #[test]
    fn loop_rest_stops_pull_off_and_back() {
        let (net, rep) = city();
        let map = crate::map::CityMap::new(net.clone());
        let sites: Vec<&Poi> = net
            .pois
            .iter()
            .filter(|p| p.kind == PoiKind::RestStop)
            .collect();
        assert_eq!(sites.len(), REST_SITES.len());
        let mut parked = crate::props::parked_cars(&map, 1);
        // Kerbside and in the car parks' bays.
        parked.extend(crate::props::car_park_cars(&map, 1));
        let loop_lane = |l: LaneId| {
            rep.loop_roads
                .contains(&(net.lanes[l as usize].road as usize))
        };
        for site in &sites {
            let lane = site.lane.expect("site on a lane");
            let road = net.lanes[lane as usize].road as usize;
            assert_eq!(rep.road_names[road], site.name.as_str());
            // Lanes into the parking road come from an off-ramp that leaves the loop; the lane
            // out of it feeds an on-ramp that joins the loop.
            let into: Vec<usize> = (0..net.lanes.len())
                .filter(|i| net.lanes[*i].connections.iter().any(|c| c.to == lane))
                .collect();
            assert!(!into.is_empty(), "{}", site.name);
            for &r in &into {
                assert!(
                    rep.ramp_roads.contains(&(net.lanes[r].road as usize)),
                    "{}",
                    site.name
                );
                let feeds: Vec<usize> = (0..net.lanes.len())
                    .filter(|i| net.lanes[*i].connections.iter().any(|c| c.to as usize == r))
                    .collect();
                assert!(
                    feeds.iter().any(|f| loop_lane(*f as LaneId)),
                    "{} off-ramp",
                    site.name
                );
            }
            let outs = &net.lanes[lane as usize].connections;
            assert!(!outs.is_empty(), "{}", site.name);
            for c in outs {
                let r = &net.lanes[c.to as usize];
                assert!(rep.ramp_roads.contains(&(r.road as usize)), "{}", site.name);
                assert!(
                    r.connections.iter().any(|d| loop_lane(d.to)),
                    "{} on-ramp",
                    site.name
                );
            }
            // A couple of parked cars at least; no buildings within 25 m of the site.
            let q = [site.pos[0], site.pos[1]];
            let cars = parked
                .iter()
                .filter(|p| geom::dist(geom::xy(p.pos), q) < REST_HALF_M + 10.0)
                .count();
            assert!(cars >= 2, "{}: {cars} parked cars", site.name);
            for b in &net.buildings {
                for c in &b.footprint {
                    assert!(geom::dist(*c, q) > 25.0, "{}: building at {c:?}", site.name);
                }
            }
            // Near the loop (a pull-off), flat enough to park on.
            assert!(
                net.lanes.iter().any(|l| loop_lane_road(rep, l)
                    && l.polyline
                        .iter()
                        .any(|p| geom::dist(geom::xy(*p), q) < 60.0)),
                "{} beside the loop",
                site.name
            );
        }
    }

    fn loop_lane_road(rep: &VesperReport, l: &Lane) -> bool {
        rep.loop_roads.contains(&(l.road as usize))
    }

    #[test]
    fn loop_is_a_divided_highway_with_ramps() {
        let (net, rep) = city();
        let map = crate::map::CityMap::new(net.clone());
        // Two one-way three-lane carriageways, one of them carrying the median barrier.
        let barrier: f32 = crate::surface::median_barriers(net)
            .iter()
            .map(|l| *geom::cumulative(l).last().unwrap())
            .sum();
        assert!(barrier > 9000.0, "barrier only {barrier} m");
        let loop_lanes: Vec<&Lane> = net
            .lanes
            .iter()
            .filter(|l| rep.loop_roads.contains(&(l.road as usize)))
            .collect();
        assert!(loop_lanes.iter().all(|l| l.kind == LaneKind::Highway));
        assert!(loop_lanes.iter().all(|l| l.index_from_left < 3));
        // No signal, stop or crosswalk anywhere on the main line.
        for (i, l) in net.lanes.iter().enumerate() {
            if !rep.loop_roads.contains(&(l.road as usize)) {
                continue;
            }
            match map.lane_control(i as LaneId) {
                None => {}
                Some((IntersectionControl::Priority, ap, _)) => assert!(ap.major, "lane {i}"),
                Some(c) => panic!("loop lane {i} ends at {:?}", c.0),
            }
            assert!(!crate::markings::must_stop(&map, i as LaneId));
        }
        // 28 ramps: 4 per full diamond, 2 at the port, 2 per rest stop; each diverges from /
        // merges into the rightmost lane, yields without a stop line and reaches a terminal.
        assert_eq!(rep.ramp_roads.len(), 22 + 2 * REST_SITES.len());
        let mut merges = 0;
        let mut diverges = 0;
        for &r in &rep.ramp_roads {
            let lanes: Vec<usize> = (0..net.lanes.len())
                .filter(|i| net.lanes[*i].road as usize == r)
                .collect();
            assert_eq!(lanes.len(), 1, "ramp {r}");
            let id = lanes[0];
            let l = &net.lanes[id];
            let from_loop: Vec<&Lane> = net
                .lanes
                .iter()
                .filter(|m| m.connections.iter().any(|c| c.to as usize == id))
                .collect();
            if from_loop
                .iter()
                .any(|m| rep.loop_roads.contains(&(m.road as usize)))
            {
                diverges += 1;
                assert!(from_loop.iter().all(|m| m.index_from_left == 2));
            } else {
                merges += 1;
                assert_eq!(l.connections.len(), 1);
                let to = &net.lanes[l.connections[0].to as usize];
                assert!(rep.loop_roads.contains(&(to.road as usize)));
                assert_eq!(to.index_from_left, 2);
                assert!(matches!(
                    map.lane_control(id as LaneId),
                    Some((IntersectionControl::Priority, ap, _)) if !ap.major
                ));
                assert!(!crate::markings::must_stop(&map, id as LaneId));
            }
            // Ramp lanes are long enough to brake or accelerate on.
            let len = *geom::cumulative(&l.polyline).last().unwrap();
            assert!(len > 180.0, "ramp {r} lane only {len} m");
        }
        assert_eq!(
            (diverges, merges),
            (11 + REST_SITES.len(), 11 + REST_SITES.len())
        );
        // Ramps never cross another road at grade (their crossroad bridges the highway).
        for &r in &rep.ramp_roads {
            let s = &net.surfaces[r];
            for (o, t) in net.surfaces.iter().enumerate() {
                if o == r {
                    continue;
                }
                let hits = geom::polylines_within(&s.centre, &t.centre, 0.01)
                    && path_crossings(
                        &s.centre.iter().map(|p| geom::xy(*p)).collect::<Vec<_>>(),
                        &t.centre.iter().map(|p| geom::xy(*p)).collect::<Vec<_>>(),
                    )
                    .iter()
                    .any(|(sa, _, _)| *sa > 1.0);
                assert!(!hits, "ramp {r} crosses {} {o}", rep.road_names[o]);
            }
        }
        // Beside the carriageway, a ramp lies on the carriageway's plane.
        let mut beside = 0;
        for &r in &rep.ramp_roads {
            let s = &net.surfaces[r];
            for p in s.centre.iter().take(8) {
                if let Some(z) = (0..net.surfaces.len())
                    .filter(|o| rep.loop_roads.contains(o))
                    .find_map(|o| {
                        let c = &net.surfaces[o];
                        let cum = geom::cumulative(&c.centre);
                        let pr = geom::project(&c.centre, &cum, geom::xy(*p));
                        (pr.dist < c.right_m + 6.0 && pr.s > 0.0 && pr.s < cum[cum.len() - 1]).then(
                            || {
                                let (q, _) = geom::point_at(&c.centre, &cum, pr.s);
                                let k = cum.partition_point(|x| *x < pr.s).min(c.bank.len() - 1);
                                q[2] + c.bank[k] * pr.lateral
                            },
                        )
                    })
                {
                    assert!(
                        (z - p[2]).abs() < 0.12,
                        "ramp {r} {:.2} m off the carriageway",
                        z - p[2]
                    );
                    beside += 1;
                }
            }
        }
        assert!(beside > 60, "{beside}");
    }

    #[test]
    fn overpass_decks_hide_the_road_below_from_the_air() {
        let (net, rep) = city();
        let map = crate::map::CityMap::new(net.clone());
        assert!(!map.decks().is_empty());
        let z_on = |road: usize, p: V2| {
            let s = &net.surfaces[road];
            let pts: Vec<V2> = s.centre.iter().map(|q| geom::xy(*q)).collect();
            let (at, _) = project2(&pts, &cum2(&pts), p);
            geom::point_at(&s.centre, &geom::cumulative(&s.centre), at).0[2]
        };
        for c in &rep.crossings {
            let (lo, hi) = (z_on(c.lower, c.pos), z_on(c.upper, c.pos));
            // A helicopter 120 m up, a little off to the side.
            let heli = [c.pos[0] + 20.0, c.pos[1] + 15.0, hi + 120.0];
            let under = [c.pos[0], c.pos[1], lo + 1.0];
            let on_deck = [c.pos[0], c.pos[1], hi + 1.0];
            assert!(
                !map.line_of_sight(heli, under),
                "under the deck at {:?}",
                c.pos
            );
            // ... because of the deck itself, not only the cutting's walls.
            assert!(map.decks().iter().any(|d| d.blocks(heli, under)));
            assert!(
                map.line_of_sight(heli, on_deck),
                "on the deck at {:?}",
                c.pos
            );
            // Ground level beside the deck is not covered: a car on the lower road 60 m
            // away is visible from straight above.
            let s = &net.surfaces[c.lower];
            let pts: Vec<V2> = s.centre.iter().map(|q| geom::xy(*q)).collect();
            let cum = cum2(&pts);
            let (at, _) = project2(&pts, &cum, c.pos);
            let f = geom::cumulative(&s.centre);
            let away = geom::point_at(&s.centre, &f, (at + 60.0).min(*f.last().unwrap())).0;
            let above = [away[0] + 3.0, away[1] + 3.0, away[2] + 120.0];
            assert!(map.line_of_sight(above, [away[0], away[1], away[2] + 1.0]));
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

    #[test]
    fn river_canal_and_their_bridges() {
        let (net, _) = city();
        let f = &net.features;
        let kinds: Vec<WaterwayKind> = f.waterways.iter().map(|w| w.kind).collect();
        assert_eq!(kinds, [WaterwayKind::River, WaterwayKind::Canal]);
        let river = &f.waterways[0];
        // The river falls (never rises) to the sea.
        for w in river.centre.windows(2) {
            assert!(w[1][2] <= w[0][2] + 1e-4);
        }
        assert!(river.centre[0][2] > 15.0);
        assert!((river.centre[river.centre.len() - 1][2] - SEA).abs() < 1e-4);
        for name in [
            "Vesper Viaduct",
            "Mouth Viaduct",
            "Ridge Road Bridge",
            "Hollins Gorge Bridge",
            "Lantern Bridge",
            "Larkspur Avenue Bridge",
            "Iron Bridge",
            "Harbor Boulevard Bridge",
            "Basin Bridge",
            "Mill Bridge",
            "Lock Bridge",
        ] {
            assert!(f.bridges.iter().any(|b| b.name == name), "no {name}");
        }
        assert_eq!(
            f.bridges
                .iter()
                .filter(|b| b.kind == BridgeKind::Truss)
                .count(),
            1
        );
        // Every road that crosses a waterway does so on a bridge, well above the water.
        for (si, s) in net.surfaces.iter().enumerate() {
            let xy: Vec<V2> = s.centre.iter().map(|p| geom::xy(*p)).collect();
            for w in &f.waterways {
                let wxy: Vec<V2> = w.centre.iter().map(|p| geom::xy(*p)).collect();
                for (_, sw, p) in path_crossings(&xy, &wxy) {
                    let b = f
                        .bridges
                        .iter()
                        .find(|b| b.surface as usize == si)
                        .unwrap_or_else(|| panic!("surface {si} crosses {} at {p:?}", w.name));
                    let wc = cum2(&wxy);
                    let k = wc.partition_point(|c| *c <= sw).min(wc.len() - 1);
                    let deck = b.centre.iter().map(|q| q[2]).fold(f32::INFINITY, f32::min);
                    assert!(deck > w.centre[k][2] + 1.5, "{} deck {deck}", b.name);
                }
            }
        }
        let map = crate::CityMap::new(net.clone());
        let mid = |w: &Waterway| geom::xy(w.centre[w.centre.len() / 2]);
        assert!(map.water_surface_at(mid(river)).is_some());
        assert!(map.water_surface_at(mid(&f.waterways[1])).is_some());
        assert!(map.water_surface_at([0.0, 0.0]).is_none());
        // The Iron Bridge's steel-grate deck.
        let iron = f.bridges.iter().find(|b| b.name == "Iron Bridge").unwrap();
        let c = iron.centre[iron.centre.len() / 2];
        assert_eq!(
            wp_contracts::WorldQuery::surface_at(&map, [c[0], c[1], c[2] + 0.1]),
            wp_contracts::SurfaceType::Metal
        );
    }

    #[test]
    fn hollins_tunnel_hides_cars_from_the_air() {
        // The Hollins Tunnel is a covered span; the features list it as a landmark and the
        // hideout a car is invisible from the air in.
        let (net, _) = city();
        let f = &net.features;
        let h = f
            .hideouts
            .iter()
            .find(|h| h.kind == HideoutKind::Tunnel && h.name == "Hollins Tunnel")
            .expect("tunnel hideout");
        assert!(h.radius_m > 150.0, "{h:?}");
        assert!(f
            .landmarks
            .iter()
            .any(|l| l.kind == LandmarkKind::Tunnel && l.name == "Hollins Tunnel"));
        let map = crate::CityMap::new(net.clone());
        let car = [h.pos[0], h.pos[1], h.pos[2] + 1.2];
        assert!(map.in_tunnel(car));
        assert!(!map.line_of_sight([car[0] + 60.0, car[1] + 40.0, car[2] + 150.0], car));
    }

    #[test]
    fn landmarks_and_canopy_hideouts() {
        let (net, _) = city();
        let f = &net.features;
        for k in [
            LandmarkKind::Tower,
            LandmarkKind::Lighthouse,
            LandmarkKind::Observatory,
            LandmarkKind::Chimney,
            LandmarkKind::Crane,
            LandmarkKind::WaterTower,
            LandmarkKind::Bridge,
            LandmarkKind::Tunnel,
        ] {
            assert!(f.landmarks.iter().any(|l| l.kind == k), "no {k:?}");
        }
        let tower = f
            .landmarks
            .iter()
            .find(|l| l.kind == LandmarkKind::Tower)
            .unwrap();
        assert!(tower.height_m > 200.0);
        let canopies: Vec<&Hideout> = f
            .hideouts
            .iter()
            .filter(|h| h.kind == HideoutKind::Canopy)
            .collect();
        assert_eq!(canopies.len(), 5);
        let map = crate::CityMap::new(net.clone());
        for h in &canopies {
            let car = [h.pos[0], h.pos[1], h.pos[2] + 1.0];
            assert!(
                !map.line_of_sight([h.pos[0] + 30.0, h.pos[1] + 20.0, h.pos[2] + 150.0], car),
                "{} seen from the air",
                h.name
            );
        }
        // Ordinary buildings stay off landmark and hideout lots.
        let bbox = |fp: &[V2]| {
            fp.iter().fold(
                ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]),
                |(lo, hi), p| {
                    (
                        [lo[0].min(p[0]), lo[1].min(p[1])],
                        [hi[0].max(p[0]), hi[1].max(p[1])],
                    )
                },
            )
        };
        for st in f.structures.iter().filter(|s| {
            matches!(s.kind, StructureKind::Landmark | StructureKind::Pillar)
                && s.z0_m < 15.0 + net.ground_z(s.footprint[0])
        }) {
            let (lo, hi) = bbox(&st.footprint);
            for b in &net.buildings {
                let (blo, bhi) = bbox(&b.footprint);
                let overlap = blo[0] < hi[0] && bhi[0] > lo[0] && blo[1] < hi[1] && bhi[1] > lo[1];
                assert!(!overlap, "building on a {:?} at {lo:?}", st.kind);
            }
        }
    }

    #[test]
    fn highway_edges_are_walled_except_at_ramps_decks_and_the_tunnel() {
        let (net, _) = city();
        let lines = crate::surface::edge_barriers(&crate::CityMap::new(net.clone()));
        let walled: f32 = lines.iter().map(|l| geom::cumulative(l)[l.len() - 1]).sum();
        // Outer edges: the right edge of every highway surface, the left one too unless a
        // median barrier walls it (a divided highway's median carries one barrier, owned by one
        // carriageway).
        let medians = crate::surface::median_barriers(net);
        let near_median = |q: V2| {
            medians.iter().any(|m| {
                m.windows(2).any(|w| {
                    geom::point_segment_distance(q, geom::xy(w[0]), geom::xy(w[1]))
                        < crate::surface::MEDIAN_CLEAR_M
                })
            })
        };
        let mut edges = 0.0;
        for s in net.surfaces.iter().filter(|s| s.highway) {
            let len = geom::cumulative(&s.centre)[s.centre.len() - 1];
            edges += len;
            if !s.median_barrier {
                let (l, _) = crate::surface::surface_edges(s);
                let lc = geom::cumulative(&l);
                let line = geom::offset_polyline(&l, -crate::surface::BARRIER_HALF_WIDTH_M);
                for k in 0..l.len() - 1 {
                    if !near_median(geom::xy(line[k])) {
                        edges += lc[k + 1] - lc[k];
                    }
                }
            }
        }
        // Measured 61 % before the median-side duplicates were dropped; open at ramp tapers,
        // gores and junctions (and 40 m either side of them), on decks and in the Hollins
        // Tunnel.
        assert!(walled > 0.55 * edges, "walled {walled} of {edges} m");
        // Never on any paving (ramp joins stay open), never in a covered span (the Hollins
        // Tunnel's walls; the barrier stops at its portals), never beside a median barrier
        // (one ran 0.25 m beside Loop 9's north median and stuck out of its face).
        let covers: Vec<CoverShape> = net.covered.iter().map(CoverShape::new).collect();
        let medians = crate::surface::median_barriers(net);
        for l in &lines {
            for p in l {
                for m in &medians {
                    for w in m.windows(2) {
                        let d = geom::point_segment_distance(
                            geom::xy(*p),
                            geom::xy(w[0]),
                            geom::xy(w[1]),
                        );
                        assert!(
                            d >= crate::surface::MEDIAN_CLEAR_M - 0.01
                                || (p[2] - w[0][2]).abs() > 3.0,
                            "edge barrier {d:.2} m from a median barrier at {p:?}"
                        );
                    }
                }
                assert!(
                    !crate::surface::inside_other_paving(net, usize::MAX, *p),
                    "barrier on paving at {p:?}"
                );
                assert!(
                    !covers.iter().any(|c| c.contains([p[0], p[1], p[2] + 1.0])),
                    "barrier in a covered span at {p:?}"
                );
            }
        }
        // Row 160: every ramp (the rest-stop ramps too) meets its carriageway and its terminal
        // through an open gap: the 40 m at each end of an undivided highway surface have no
        // other barrier line across their paved width.
        let mut ends = 0;
        for (si, s) in net.surfaces.iter().enumerate() {
            if !s.highway || s.median_barrier {
                continue;
            }
            let cum = geom::cumulative(&s.centre);
            let len = cum[cum.len() - 1];
            for at in [0.0f32, 10.0, 20.0, 30.0, 40.0]
                .iter()
                .flat_map(|d| [*d, len - *d])
                .filter(|a| (0.0..=len).contains(a))
            {
                let q = geom::xy(geom::point_at(&s.centre, &cum, at).0);
                // (Its own edge barriers stand 0.25 m outside its paved edges.)
                for l in &lines {
                    for w in l.windows(2) {
                        let d = geom::point_segment_distance(q, geom::xy(w[0]), geom::xy(w[1]));
                        assert!(
                            d > s.left_m.min(s.right_m) - 0.1,
                            "surface {si}: a barrier {d:.2} m from its centre {at:.0} m along"
                        );
                    }
                }
                ends += 1;
            }
        }
        assert!(ends > 20, "{ends}");
    }
}
