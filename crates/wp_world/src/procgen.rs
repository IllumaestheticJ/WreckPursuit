//! Procedural fallback city (`gen-city --grid 12x12 --seed N`): a street grid, an octagonal
//! ring road, a north–south highway spine and two diagonal arterials, with zones by radius,
//! building lots, alleys in the core and POIs. Nobody has to wait for an OSM extract.
//!
//! Layout (12×12, 110 m spacing): grid streets centred on the origin; rows/columns 2 and 9
//! are 2+2 arterials, the two central rows/columns are collectors, the rest are local streets
//! with kerbside parking. The highway runs between the two central columns: a divided viaduct
//! (two one-way 3-lane carriageways, shoulders and a median barrier) that rises from the ring
//! road at each end and bridges the arterial/collector rows; other rows end at the columns
//! beside it. Ramps reach the first and last crossing rows: half diamonds facing the ring on
//! every grid, full diamonds where the next block in is long enough too (the 12×12 city). Diagonals (square grids only) run corner
//! to corner through grid nodes with a kerbside bus lane. The ring road joins every street
//! end; its junctions with local streets are priority-controlled. The ground is a flat
//! heightfield just under the streets, so the viaduct is a real bridge in physics too.

use std::collections::BTreeMap;

use wp_math::Rng;

use crate::builder::{self, mph, BuildOptions, ControlHint, NodeSpec, RoadClass, RoadSpec};
use crate::density::default_density_base;
use crate::geom::{self, V2, V3};
use crate::network::*;
use crate::residential::{self, LotKind, LotSite, ObbIndex};
use crate::signals;
use crate::surface::{KERB_H_M, SIDEWALK_W_M, VERGE_W_M};
use crate::terrain::{smoothstep, Heightfield};
use crate::vesper::{CARVE_BELOW_M, CROSSING_CLEARANCE_M};

/// Highway lane width, m (`BuildOptions::highway_lane_width_m`).
const HW_LANE_M: f32 = 3.7;
/// Paved median shoulder between a carriageway's left lane and the barrier, m.
const MEDIAN_SHOULDER_M: f32 = 1.5;
/// Paved outer shoulder, m.
const OUTER_SHOULDER_M: f32 = 2.5;
/// Carriageway centreline offset from the highway centreline (the barrier), m.
const CARRIAGEWAY_OFFSET_M: f32 = MEDIAN_SHOULDER_M + 1.5 * HW_LANE_M;
/// Viaduct surface height over the streets it bridges, m.
const VIADUCT_Z: f32 = CROSSING_CLEARANCE_M + 0.4;
/// Flat run at each end of the highway from the ring junction, m: the end ramps' gores and
/// tapers stay at grade, the climb starts beyond them.
const END_FLAT_M: f32 = 115.0;
/// Climb from the ring to viaduct height (smoothstep; ≤ 6 % grade), m.
const CLIMB_M: f32 = 200.0;
/// Carriageways fan out from the shared ring node to their offset within this distance (inside
/// the junction setback), m.
const FAN_M: f32 = 8.0;
/// Gore (diverge / merge node) distance from the crossing row a ramp serves, m.
const RAMP_SPAN_M: f32 = 280.0;
/// Ramp length from its gore beside the carriageway (taper + parallel lane), m.
const RAMP_PARALLEL_M: f32 = 90.0;
/// Ramp lane centre beside a carriageway (from the carriageway centreline), m.
const RAMP_LANE_OFFSET_M: f32 = 2.0 * HW_LANE_M;
/// Ramp terminal junctions on a crossing row, from the highway centreline, m.
const TERMINAL_X_M: f32 = 22.0;
/// Flat run of a ramp into its terminal junction, m.
const TERMINAL_FLAT_M: f32 = 15.0;
/// Ramp speed limit, m/s.
const RAMP_SPEED: f32 = 20.1168; // 45 mph

/// One highway ramp in the grid city.
#[derive(Debug, Clone, Copy)]
struct GridRamp {
    /// +1 northbound carriageway (east of the centreline), -1 southbound (west).
    side: f32,
    /// Leaves the carriageway (off-ramp) rather than joins it.
    off: bool,
    /// Crossing row it serves, y.
    row_y: f32,
    /// Gore y on the carriageway.
    gore_y: f32,
}

/// Procedural city parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcGenConfig {
    /// Grid columns (north–south streets).
    pub nx: usize,
    /// Grid rows (east–west streets).
    pub ny: usize,
    /// Street spacing (centre to centre), m.
    pub spacing_m: f32,
    /// Distance from the outermost grid street to the ring road, m.
    pub ring_margin_m: f32,
    /// Ring corner chamfer, m.
    pub ring_chamfer_m: f32,
    /// Seed for lots, heights and POI choice.
    pub seed: u64,
}

impl Default for ProcGenConfig {
    fn default() -> Self {
        Self {
            nx: 12,
            ny: 12,
            spacing_m: 110.0,
            ring_margin_m: 140.0,
            ring_chamfer_m: 150.0,
            seed: 1,
        }
    }
}

impl ProcGenConfig {
    /// Small 6×6 city for fast tests (all features: arterials, highway, diagonals, ring, alleys,
    /// signals, priority and all-way junctions; ~650 lanes vs 1734, ~520 parked cars vs 3190).
    pub fn small() -> Self {
        Self {
            nx: 6,
            ny: 6,
            ..Self::default()
        }
    }

    /// City id used for file names, e.g. `procedural_12x12`.
    pub fn city_id(&self) -> String {
        format!("procedural_{}x{}", self.nx, self.ny)
    }
}

#[derive(Default)]
struct Reg {
    nodes: Vec<NodeSpec>,
    index: BTreeMap<(i64, i64), usize>,
}

impl Reg {
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
}

#[derive(Clone, Copy)]
struct LineStyle {
    class: RoadClass,
    lanes: u8,
    speed: f32,
    parking: bool,
    bus: bool,
}

fn style_for(class: RoadClass) -> LineStyle {
    match class {
        RoadClass::Highway => LineStyle {
            class,
            lanes: 3,
            speed: mph(55.0),
            parking: false,
            bus: false,
        },
        RoadClass::Arterial => LineStyle {
            class,
            lanes: 2,
            speed: mph(40.0),
            parking: false,
            bus: false,
        },
        RoadClass::Collector => LineStyle {
            class,
            lanes: 1,
            speed: mph(30.0),
            parking: true,
            bus: false,
        },
        RoadClass::Local => LineStyle {
            class,
            lanes: 1,
            speed: mph(25.0),
            parking: true,
            bus: false,
        },
        RoadClass::Alley => LineStyle {
            class,
            lanes: 1,
            speed: mph(15.0),
            parking: false,
            bus: false,
        },
    }
}

fn road(a: usize, b: usize, st: LineStyle) -> RoadSpec {
    let mut r = RoadSpec::new(a, b, st.class, st.lanes, st.lanes);
    r.speed_mps = st.speed;
    r.parking = st.parking;
    r.bus_lane = st.bus;
    r
}

/// Ray from `p` along unit `d` against a closed polygon; nearest hit point.
fn ray_polygon(p: V2, d: V2, poly: &[V2]) -> Option<V2> {
    let mut best: Option<(f32, V2)> = None;
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        let e = geom::sub(b, a);
        let den = geom::cross(d, e);
        if den.abs() < 1e-9 {
            continue;
        }
        let ap = geom::sub(a, p);
        let t = geom::cross(ap, e) / den;
        let u = geom::cross(ap, d) / den;
        if t > 1.0 && (-1e-4..=1.0 + 1e-4).contains(&u) && best.is_none_or(|(bt, _)| t < bt) {
            best = Some((t, geom::add(p, geom::scale(d, t))));
        }
    }
    best.map(|b| b.1)
}

/// Generate the procedural city.
pub fn generate(cfg: &ProcGenConfig) -> RoadNetwork {
    assert!(cfg.nx >= 4 && cfg.ny >= 4, "grid must be at least 4x4");
    let mut rng = Rng::new(cfg.seed);
    let sp = cfg.spacing_m;
    let (nx, ny) = (cfg.nx, cfg.ny);
    let xs: Vec<f32> = (0..nx)
        .map(|i| (i as f32 - (nx as f32 - 1.0) * 0.5) * sp)
        .collect();
    let ys: Vec<f32> = (0..ny)
        .map(|j| (j as f32 - (ny as f32 - 1.0) * 0.5) * sp)
        .collect();
    let hx = if nx % 2 == 0 { 0.0 } else { sp * 0.5 };
    let diagonals = nx == ny;

    let class_of = |idx: usize, n: usize| -> RoadClass {
        if n >= 6 && (idx == 2 || idx == n - 3) {
            RoadClass::Arterial
        } else if idx == n / 2 - 1 || idx == n / 2 {
            RoadClass::Collector
        } else {
            RoadClass::Local
        }
    };
    let row_class: Vec<RoadClass> = (0..ny).map(|j| class_of(j, ny)).collect();
    let col_class: Vec<RoadClass> = (0..nx).map(|i| class_of(i, nx)).collect();
    let crossing_row = |j: usize| row_class[j] != RoadClass::Local;

    // Highway plan: ends on the ring road, viaduct height profile and ramps.
    let rr = xs[nx - 1].max(ys[ny - 1]) + cfg.ring_margin_m;
    let (y_s, y_n) = (-rr, rr);
    let hw_z = |y: f32| {
        let d = (y - y_s).min(y_n - y);
        VIADUCT_Z * smoothstep(END_FLAT_M, END_FLAT_M + CLIMB_M, d)
    };
    let hw_x = |y: f32, side: f32| {
        let d = (y - y_s).min(y_n - y);
        hx + side * CARRIAGEWAY_OFFSET_M * smoothstep(0.0, FAN_M, d)
    };
    let mut cross_ys: Vec<f32> = (0..ny)
        .filter(|j| crossing_row(*j))
        .map(|j| ys[j])
        .collect();
    cross_ys.sort_by(f32::total_cmp);
    let mut ramps: Vec<GridRamp> = Vec::new();
    // Ramps at the first and last crossing rows: the outer half of each diamond (towards the
    // ring) whenever that block is long enough, the inner half too where the next block in is
    // (the 12×12 city: full diamonds).
    if let (Some(&y1), Some(&yk)) = (cross_ys.first(), cross_ys.last()) {
        let outer = RAMP_SPAN_M + 70.0;
        let inner = RAMP_SPAN_M + 40.0;
        let full = cross_ys.len() >= 2
            && cross_ys[1] - y1 >= inner
            && yk - cross_ys[cross_ys.len() - 2] >= inner;
        let mut plan: Vec<(f32, bool, f32, f32)> = Vec::new();
        if y1 - y_s >= outer && y_n - yk >= outer {
            // Southern row: northbound exit / southbound entry towards the ring, then (full
            // diamond) southbound exit / northbound entry from the next block north; the
            // northern row mirrored.
            plan.extend([
                (1.0, true, y1, y1 - RAMP_SPAN_M),
                (-1.0, false, y1, y1 - RAMP_SPAN_M),
            ]);
            if full {
                plan.extend([
                    (-1.0, true, y1, y1 + RAMP_SPAN_M),
                    (1.0, false, y1, y1 + RAMP_SPAN_M),
                ]);
            }
            plan.extend([
                (-1.0, true, yk, yk + RAMP_SPAN_M),
                (1.0, false, yk, yk + RAMP_SPAN_M),
            ]);
            if full {
                plan.extend([
                    (1.0, true, yk, yk - RAMP_SPAN_M),
                    (-1.0, false, yk, yk - RAMP_SPAN_M),
                ]);
            }
        }
        for (side, off, row_y, gore_y) in plan {
            ramps.push(GridRamp {
                side,
                off,
                row_y,
                gore_y,
            });
        }
    }
    let terminal_x = |side: f32| hx + side * TERMINAL_X_M;

    let mut reg = Reg::default();
    let mut roads: Vec<RoadSpec> = Vec::new();
    // Ring connection requests: (from node, direction, style).
    let mut ring_links: Vec<(usize, V2, LineStyle)> = Vec::new();

    // Alleys: E–W through core blocks that no highway/diagonal crosses.
    let mut alleys: Vec<(usize, usize, f32)> = Vec::new(); // (col i, col i+1, y)
    for j in 0..ny - 1 {
        for i in 0..nx - 1 {
            let cx = (xs[i] + xs[i + 1]) * 0.5;
            let cy = (ys[j] + ys[j + 1]) * 0.5;
            let crosses_hw = xs[i] < hx && xs[i + 1] > hx;
            let diag_block = diagonals && (i == j || i == nx - 2 - j);
            if geom::len([cx, cy]) < 260.0 && !crosses_hw && !diag_block {
                alleys.push((i, i + 1, cy));
            }
        }
    }

    // Rows.
    for (j, &y) in ys.iter().enumerate() {
        let st = style_for(row_class[j]);
        let mut pts: Vec<V2> = xs.iter().map(|&x| [x, y]).collect();
        // Crossing rows pass under the viaduct; ramp terminals split them beside it.
        for side in [-1.0f32, 1.0] {
            if ramps.iter().any(|r| r.row_y == y && r.side == side) {
                pts.push([terminal_x(side), y]);
            }
        }
        pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
        for w in pts.windows(2) {
            if !crossing_row(j) && w[0][0] < hx && w[1][0] > hx {
                continue;
            }
            let (a, b) = (reg.node(w[0]), reg.node(w[1]));
            roads.push(road(a, b, st));
        }
        ring_links.push((reg.node(pts[0]), [-1.0, 0.0], st));
        ring_links.push((reg.node(pts[pts.len() - 1]), [1.0, 0.0], st));
    }
    // Columns (with alley nodes).
    for (i, &x) in xs.iter().enumerate() {
        let st = style_for(col_class[i]);
        let mut pts: Vec<V2> = ys.iter().map(|&y| [x, y]).collect();
        for a in &alleys {
            if a.0 == i || a.1 == i {
                pts.push([x, a.2]);
            }
        }
        pts.sort_by(|a, b| a[1].total_cmp(&b[1]));
        pts.dedup_by(|a, b| (a[1] - b[1]).abs() < 0.05);
        for w in pts.windows(2) {
            let (a, b) = (reg.node(w[0]), reg.node(w[1]));
            roads.push(road(a, b, st));
        }
        ring_links.push((reg.node(pts[0]), [0.0, -1.0], st));
        ring_links.push((reg.node(pts[pts.len() - 1]), [0.0, 1.0], st));
    }
    // Alleys.
    for a in &alleys {
        let (na, nb) = (reg.node([xs[a.0], a.2]), reg.node([xs[a.1], a.2]));
        roads.push(road(na, nb, style_for(RoadClass::Alley)));
    }
    // Highway: two one-way carriageways from the ring node at each end, split at the gores.
    let hw_st = style_for(RoadClass::Highway);
    let ring_s = reg.node([hx, y_s]);
    let ring_n = reg.node([hx, y_n]);
    let mut gore_node: Vec<usize> = Vec::with_capacity(ramps.len());
    for r in &ramps {
        let n = reg.node([hw_x(r.gore_y, r.side), r.gore_y]);
        reg.nodes[n].z = hw_z(r.gore_y);
        reg.nodes[n].control = Some(ControlHint::Freeway);
        gore_node.push(n);
    }
    let hw_first_road = roads.len();
    for side in [1.0f32, -1.0] {
        // Cuts in travel order (northbound south → north).
        let mut cuts: Vec<(f32, usize)> = vec![(y_s, ring_s), (y_n, ring_n)];
        for (r, n) in ramps.iter().zip(&gore_node) {
            if r.side == side {
                cuts.push((r.gore_y, *n));
            }
        }
        cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
        if side < 0.0 {
            cuts.reverse();
        }
        for w in cuts.windows(2) {
            let ((ya, a), (yb, b)) = (w[0], w[1]);
            let n = ((yb - ya).abs() / 10.0).ceil().max(1.0) as usize;
            let mut ys_s: Vec<f32> = (0..=n)
                .map(|k| ya + (yb - ya) * k as f32 / n as f32)
                .collect();
            // Fine samples through the fan-out at the ring.
            for end in [y_s, y_n] {
                for k in 1..8 {
                    let y = end + (if end < 0.0 { 1.0 } else { -1.0 }) * k as f32;
                    if (y - ya) * (y - yb) < 0.0 {
                        ys_s.push(y);
                    }
                }
            }
            ys_s.sort_by(|p, q| {
                if side > 0.0 {
                    p.total_cmp(q)
                } else {
                    q.total_cmp(p)
                }
            });
            ys_s.dedup_by(|p, q| (*p - *q).abs() < 0.5);
            let pts: Vec<V3> = ys_s
                .iter()
                .map(|y| [hw_x(*y, side), *y, hw_z(*y)])
                .collect();
            let mut r = road(a, b, hw_st);
            r.lanes_bwd = 0;
            r.via = pts[1..pts.len() - 1].iter().map(|p| [p[0], p[1]]).collect();
            r.via_z = pts[1..pts.len() - 1].iter().map(|p| p[2]).collect();
            r.shoulder_left_m = MEDIAN_SHOULDER_M;
            r.shoulder_right_m = OUTER_SHOULDER_M;
            r.median_barrier = side > 0.0;
            roads.push(r);
        }
    }
    // Ramps: beside the carriageway on its plane for the first 90 m, then an S-curve to the
    // terminal on the crossing row, arriving (or leaving) along the highway.
    for (r, &g) in ramps.iter().zip(&gore_node) {
        let term = reg.node([terminal_x(r.side), r.row_y]);
        reg.nodes[term].control = Some(ControlHint::RampTerminal);
        let away = if r.off { r.side } else { -r.side };
        let mut path: Vec<V3> = Vec::new();
        let mut u = 0.0;
        while u <= RAMP_PARALLEL_M + 1e-3 {
            let y = r.gore_y + away * u;
            let x = hw_x(y, r.side)
                + r.side * RAMP_LANE_OFFSET_M * smoothstep(0.0, builder::FREEWAY_SETBACK_M, u);
            path.push([x, y, hw_z(y)]);
            u += 6.0;
        }
        let p0 = path[path.len() - 1];
        let slope0 = (p0[2] - path[path.len() - 2][2]) / 6.0;
        let t_end = [terminal_x(r.side), r.row_y];
        let chord = geom::dist([p0[0], p0[1]], t_end);
        let k = 0.4 * chord;
        let (c0, c3) = ([p0[0], p0[1]], t_end);
        let c1 = [c0[0], c0[1] + away * k];
        let c2 = [c3[0], c3[1] - away * k];
        let n = ((chord / 3.0).ceil() as usize).max(8);
        let bez: Vec<V2> = (1..=n)
            .map(|i| {
                let t = i as f32 / n as f32;
                let w = 1.0 - t;
                let f = |a: f32, b: f32, c: f32, d: f32| {
                    w * w * w * a + 3.0 * w * w * t * b + 3.0 * w * t * t * c + t * t * t * d
                };
                [f(c0[0], c1[0], c2[0], c3[0]), f(c0[1], c1[1], c2[1], c3[1])]
            })
            .collect();
        // Heights: cubic Hermite from the carriageway's height and grade to the flat terminal.
        let mut cum = vec![0.0f32];
        let mut prev = c0;
        for q in &bez {
            cum.push(cum[cum.len() - 1] + geom::dist(prev, *q));
            prev = *q;
        }
        let len = (cum[cum.len() - 1] - TERMINAL_FLAT_M).max(1.0);
        for (q, c) in bez.iter().zip(&cum[1..]) {
            let t = (c / len).min(1.0);
            let (t2, t3) = (t * t, t * t * t);
            let z = (2.0 * t3 - 3.0 * t2 + 1.0) * p0[2] + (t3 - 2.0 * t2 + t) * len * slope0;
            path.push([q[0], q[1], z]);
        }
        // Thin to about 6 m spacing (keep both ends).
        let mut thin: Vec<V3> = vec![path[0]];
        let mut acc = 0.0;
        for w in path.windows(2) {
            acc += geom::dist([w[0][0], w[0][1]], [w[1][0], w[1][1]]);
            if acc >= 5.9 {
                thin.push(w[1]);
                acc = 0.0;
            }
        }
        if thin.len() > 1
            && geom::dist([thin[thin.len() - 1][0], thin[thin.len() - 1][1]], t_end) < 2.0
        {
            thin.pop();
        }
        let (a, b) = if r.off { (g, term) } else { (term, g) };
        if !r.off {
            thin.reverse();
        }
        let interior = &thin[1..];
        let mut rs = RoadSpec::new(a, b, RoadClass::Highway, 1, 0);
        rs.speed_mps = RAMP_SPEED;
        rs.via = interior.iter().map(|p| [p[0], p[1]]).collect();
        rs.via_z = interior.iter().map(|p| p[2]).collect();
        rs.shoulder_left_m = 0.6;
        rs.shoulder_right_m = 1.5;
        roads.push(rs);
    }
    // Diagonals.
    let mut diag_st = style_for(RoadClass::Arterial);
    diag_st.lanes = 3;
    diag_st.bus = true;
    diag_st.speed = mph(35.0);
    if diagonals {
        for flip in [false, true] {
            let pts: Vec<V2> = (0..nx)
                .map(|i| {
                    let y = if flip { ys[ny - 1 - i] } else { ys[i] };
                    [xs[i], y]
                })
                .collect();
            for w in pts.windows(2) {
                // The diagonals stop at the highway: the block it runs through is skipped so
                // no cramped multi-road junction forms in the core.
                if w[0][0] < hx && w[1][0] > hx {
                    continue;
                }
                let (a, b) = (reg.node(w[0]), reg.node(w[1]));
                roads.push(road(a, b, diag_st));
            }
            let d = geom::normalize(geom::sub(pts[pts.len() - 1], pts[0]));
            ring_links.push((reg.node(pts[0]), geom::scale(d, -1.0), diag_st));
            ring_links.push((reg.node(pts[pts.len() - 1]), d, diag_st));
        }
    }
    // Ring road.
    let c = cfg.ring_chamfer_m;
    let octagon: Vec<V2> = vec![
        [rr, -rr + c],
        [rr, rr - c],
        [rr - c, rr],
        [-rr + c, rr],
        [-rr, rr - c],
        [-rr, -rr + c],
        [-rr + c, -rr],
        [rr - c, -rr],
    ];
    let mut ring_nodes: Vec<usize> = vec![ring_s, ring_n];
    let mut hits: Vec<V2> = vec![[hx, y_s], [hx, y_n]];
    for (from, d, st) in &ring_links {
        let p = reg.nodes[*from].pos;
        if let Some(hit) = ray_polygon(p, *d, &octagon) {
            let rn = reg.node(hit);
            ring_nodes.push(rn);
            hits.push(hit);
            roads.push(road(*from, rn, *st));
        }
    }
    // Octagon corners become bend nodes unless a street end is too close to them.
    for v in &octagon {
        if hits.iter().all(|h| geom::dist(*h, *v) > 30.0) {
            ring_nodes.push(reg.node(*v));
        }
    }
    ring_nodes.sort_by(|a, b| {
        geom::pseudo_angle(reg.nodes[*a].pos)
            .total_cmp(&geom::pseudo_angle(reg.nodes[*b].pos))
            .then(a.cmp(b))
    });
    ring_nodes.dedup();
    let ring_st = LineStyle {
        speed: mph(45.0),
        ..style_for(RoadClass::Arterial)
    };
    let ring_first_road = roads.len();
    for k in 0..ring_nodes.len() {
        let (a, b) = (ring_nodes[k], ring_nodes[(k + 1) % ring_nodes.len()]);
        roads.push(road(a, b, ring_st));
    }
    let row2_road = roads
        .iter()
        .position(|r| r.class == RoadClass::Arterial && reg.nodes[r.a].pos[1] == ys[2])
        .unwrap_or(0);

    let built = builder::build(&reg.nodes, &roads, &BuildOptions::default());

    // Zones: one per block plus four outer bands.
    let mut zones = Vec::new();
    let block_kind = |cx: f32, cy: f32| -> DistrictKind {
        let r = geom::len([cx, cy]);
        if r < 200.0 {
            DistrictKind::Core
        } else if r < 380.0 {
            DistrictKind::Commercial
        } else if cx > 0.0 && cy < 0.0 {
            DistrictKind::Industrial
        } else {
            DistrictKind::Residential
        }
    };
    let mut block_kinds: Vec<(usize, usize, DistrictKind)> = Vec::new();
    for j in 0..ny - 1 {
        for i in 0..nx - 1 {
            let kind = block_kind((xs[i] + xs[i + 1]) * 0.5, (ys[j] + ys[j + 1]) * 0.5);
            block_kinds.push((i, j, kind));
            zones.push(Zone {
                kind,
                density_base: default_density_base(kind),
                polygon: vec![
                    [xs[i], ys[j]],
                    [xs[i + 1], ys[j]],
                    [xs[i + 1], ys[j + 1]],
                    [xs[i], ys[j + 1]],
                ],
            });
        }
    }
    let (ex, ey) = (xs[nx - 1], ys[ny - 1]);
    let ob = rr + 150.0;
    for (kind, poly) in [
        (
            DistrictKind::Industrial,
            vec![[-ob, -ob], [ob, -ob], [ex, -ey], [-ex, -ey]],
        ),
        (
            DistrictKind::Port,
            vec![[ob, -ob], [ob, ob], [ex, ey], [ex, -ey]],
        ),
        (
            DistrictKind::Suburb,
            vec![[ob, ob], [-ob, ob], [-ex, ey], [ex, ey]],
        ),
        (
            DistrictKind::Suburb,
            vec![[-ob, ob], [-ob, -ob], [-ex, -ey], [-ex, ey]],
        ),
    ] {
        zones.push(Zone {
            kind,
            density_base: default_density_base(kind),
            polygon: poly,
        });
    }

    // Buildings.
    let mut buildings = Vec::new();
    let blocked = |lo: V2, hi: V2, alley_y: Option<f32>| -> bool {
        let corners = [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]];
        let band = |f: &dyn Fn(V2) -> f32, w: f32| {
            let v: Vec<f32> = corners.iter().map(|p| f(*p)).collect();
            let mn = v.iter().copied().fold(f32::INFINITY, f32::min);
            let mx = v.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            mn < w && mx > -w
        };
        if band(&|p| p[0] - hx, TERMINAL_X_M + 8.0) {
            return true;
        }
        if diagonals
            && (band(&|p| (p[0] - p[1]) * std::f32::consts::FRAC_1_SQRT_2, 16.0)
                || band(&|p| (p[0] + p[1]) * std::f32::consts::FRAC_1_SQRT_2, 16.0))
        {
            return true;
        }
        if let Some(ay) = alley_y {
            if band(&|p| p[1] - ay, 6.0) {
                return true;
            }
        }
        false
    };
    for &(i, j, kind) in &block_kinds {
        let inset = 13.0;
        let lo = [xs[i] + inset, ys[j] + inset];
        let hi = [xs[i + 1] - inset, ys[j + 1] - inset];
        let alley_y = alleys
            .iter()
            .find(|a| a.0 == i && (a.2 - (ys[j] + ys[j + 1]) * 0.5).abs() < 1.0)
            .map(|a| a.2);
        let lot = match kind {
            DistrictKind::Core => 28.0,
            DistrictKind::Industrial => 42.0,
            _ => 21.0,
        };
        let (w, h) = (hi[0] - lo[0], hi[1] - lo[1]);
        let cols = (w / lot).round().max(1.0) as usize;
        let rows = (h / lot).round().max(1.0) as usize;
        let (lw, lh) = (w / cols as f32, h / rows as f32);
        for r in 0..rows {
            for c in 0..cols {
                // Residential blocks keep an open courtyard.
                if kind == DistrictKind::Residential
                    && r > 0
                    && r + 1 < rows
                    && c > 0
                    && c + 1 < cols
                {
                    continue;
                }
                let setback = rng.range_f32(1.5, 3.5);
                let l0 = [
                    lo[0] + c as f32 * lw + setback,
                    lo[1] + r as f32 * lh + setback,
                ];
                let l1 = [
                    lo[0] + (c + 1) as f32 * lw - setback,
                    lo[1] + (r + 1) as f32 * lh - setback,
                ];
                if blocked(l0, l1, alley_y) {
                    continue;
                }
                let storeys = match kind {
                    DistrictKind::Core => 5 + rng.below(4),
                    DistrictKind::Commercial => 2 + rng.below(3),
                    DistrictKind::Residential => 1 + rng.below(2),
                    _ => 1 + rng.below(2),
                };
                let height = storeys as f32 * 3.2 + rng.range_f32(0.0, 1.0);
                // Residential blocks are houses on lots (below); the draws above still happen
                // so every later random choice (POIs) stays as it was.
                if !residential::is_residential(kind) {
                    buildings.push(Building::block(
                        vec![l0, [l1[0], l0[1]], l1, [l0[0], l1[1]]],
                        height,
                        0.0,
                    ));
                }
            }
        }
    }
    // Residential blocks (playtest row 95): houses on lots along all four block edges, front
    // yards, garages, driveways, fences / hedges, yard trees, street trees on the verge.
    let decor = residential_blocks(
        cfg,
        &xs,
        &ys,
        &row_class,
        &col_class,
        &block_kinds,
        &blocked,
        &mut buildings,
    );
    // Outer bands: warehouses (port/industrial) and houses (suburb) between street ends.
    let band_lo = ex.max(ey) + 22.0;
    let band_hi = rr - 22.0;
    if band_hi - band_lo > 20.0 {
        for side in 0..4 {
            for k in 0..ny.max(nx) - 1 {
                let along = if side % 2 == 0 {
                    (ys[k.min(ny - 2)] + ys[k.min(ny - 2) + 1]) * 0.5
                } else {
                    (xs[k.min(nx - 2)] + xs[k.min(nx - 2) + 1]) * 0.5
                };
                // side 0 east, 1 north, 2 west, 3 south
                let (kind_big, half_along) = match side {
                    0 => (true, 35.0),
                    3 => (true, 35.0),
                    _ => (false, 9.0),
                };
                let depth = if kind_big { band_hi - band_lo } else { 18.0 };
                let height = if kind_big {
                    rng.range_f32(7.0, 11.0)
                } else {
                    rng.range_f32(5.0, 7.0)
                };
                let (a0, a1) = (along - half_along, along + half_along);
                let (d0, d1) = (band_lo, band_lo + depth);
                let (lo, hi) = match side {
                    0 => ([d0, a0], [d1, a1]),
                    1 => ([a0, d0], [a1, d1]),
                    2 => ([-d1, a0], [-d0, a1]),
                    _ => ([a0, -d1], [a1, -d0]),
                };
                if blocked(lo, hi, None) {
                    continue;
                }
                let mut b =
                    Building::block(vec![lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]], height, 0.0);
                if !kind_big {
                    // Suburb houses between the street ends: low, hipped.
                    b.kind = BuildingKind::House;
                    b.height_m = height.min(5.8);
                    b.roof = RoofShape::Hip;
                    b.roof_rise_m = 3.0;
                }
                buildings.push(b);
            }
        }
    }

    // POIs.
    let nearest_lane = |p: V2| -> Option<(LaneId, f32)> {
        let mut best: Option<(LaneId, f32, f32)> = None;
        for (i, l) in built.lanes.iter().enumerate() {
            if !l.kind.drivable() {
                continue;
            }
            let cum = geom::cumulative(&l.polyline);
            let pr = geom::project(&l.polyline, &cum, p);
            if best.is_none_or(|b| pr.dist < b.2) {
                best = Some((i as LaneId, pr.s, pr.dist));
            }
        }
        best.map(|b| (b.0, b.1))
    };
    let mut pois = Vec::new();
    let block_poi = |i: usize, j: usize| -> V2 { [(xs[i] + xs[i + 1]) * 0.5, ys[j] + 20.0] };
    let push_block_poi = |pois: &mut Vec<Poi>, kind: PoiKind, name: String, p: V2| {
        pois.push(Poi {
            kind,
            name,
            pos: [p[0], p[1], 0.0],
            heading_rad: std::f32::consts::PI,
            lane: nearest_lane(p).map(|l| l.0),
        });
    };
    let police_blocks = [(nx / 4, ny - 1 - ny / 4), (nx - 1 - nx / 4, ny / 4)];
    for (k, (i, j)) in police_blocks.iter().enumerate() {
        let i = (*i).min(nx - 2);
        let j = (*j).min(ny - 2);
        push_block_poi(
            &mut pois,
            PoiKind::PoliceStation,
            format!("Precinct {}", k + 1),
            block_poi(i, j),
        );
        if k == 0 {
            let p = block_poi(i, j);
            push_block_poi(
                &mut pois,
                PoiKind::Helipad,
                "Precinct 1 helipad".into(),
                [p[0] + 22.0, p[1] + 18.0],
            );
        }
    }
    let pick = |rng: &mut Rng, kinds: &[DistrictKind], n: usize| -> Vec<(usize, usize)> {
        let cands: Vec<(usize, usize)> = block_kinds
            .iter()
            .filter(|b| kinds.contains(&b.2))
            .map(|b| (b.0, b.1))
            .collect();
        let mut out = Vec::new();
        let mut tries = 0;
        while out.len() < n.min(cands.len()) && tries < 1000 {
            tries += 1;
            let c = cands[rng.below(cands.len() as u32) as usize];
            if !out.contains(&c) && !police_blocks.contains(&c) {
                out.push(c);
            }
        }
        out
    };
    for (k, (i, j)) in pick(&mut rng, &[DistrictKind::Commercial, DistrictKind::Core], 3)
        .into_iter()
        .enumerate()
    {
        push_block_poi(
            &mut pois,
            PoiKind::Garage,
            format!("Garage {}", k + 1),
            block_poi(i, j),
        );
    }
    for (k, (i, j)) in pick(&mut rng, &[DistrictKind::Residential], 4)
        .into_iter()
        .enumerate()
    {
        push_block_poi(
            &mut pois,
            PoiKind::SafehouseCandidate,
            format!("Safehouse {}", k + 1),
            block_poi(i, j),
        );
    }
    for (name, road_idx) in [
        ("Highway sprint", hw_first_road),
        ("Arterial drag", row2_road),
        ("Ring circuit", ring_first_road),
    ] {
        let rl = &built.road_lanes[road_idx];
        let Some(&lane) = rl.fwd.last() else { continue };
        let l = &built.lanes[lane as usize];
        let cum = geom::cumulative(&l.polyline);
        let s = (cum[cum.len() - 1] * 0.25).min(25.0);
        let (p, d) = geom::point_at(&l.polyline, &cum, s);
        pois.push(Poi {
            kind: PoiKind::RaceStartGrid,
            name: name.into(),
            pos: p,
            heading_rad: geom::yaw_of(d),
            lane: Some(lane),
        });
    }

    let mut net = built.into_network(
        &cfg.city_id(),
        &format!(
            "procedural grid={}x{} spacing={} seed={}",
            nx, ny, sp, cfg.seed
        ),
        None,
        [0.0, 0.0],
        zones,
        pois,
        buildings,
    );
    net.decor = decor;
    apply_green_waves(&mut net, &xs, &ys, &row_class, &col_class, diagonals);
    // Flat ground just under the streets: the viaduct is a bridge, the rest is at grade.
    let (lo, hi) = (net.bounds_min, net.bounds_max);
    net.terrain = Some(Heightfield::flat(
        [lo[0] - 50.0, lo[1] - 50.0],
        [hi[0] + 50.0, hi[1] + 50.0],
        10.0,
        -CARVE_BELOW_M,
    ));
    net
}

/// Offset signals so the arterial approaches (eastbound rows, northbound columns, diagonals
/// heading away from the south-west/north-west) get a progression at 90 % of the limit.
fn apply_green_waves(
    net: &mut RoadNetwork,
    xs: &[f32],
    ys: &[f32],
    row_class: &[RoadClass],
    col_class: &[RoadClass],
    diagonals: bool,
) {
    let r2 = std::f32::consts::FRAC_1_SQRT_2;
    for si in 0..net.signals.len() {
        let ix = &net.intersections[net.signals[si].intersection as usize];
        let p = geom::xy(net.nodes[ix.node as usize].pos);
        let on_row = ys
            .iter()
            .zip(row_class)
            .any(|(y, c)| *c == RoadClass::Arterial && (p[1] - y).abs() < 0.5);
        let on_col = xs
            .iter()
            .zip(col_class)
            .any(|(x, c)| *c == RoadClass::Arterial && (p[0] - x).abs() < 0.5);
        let (dir, dist): (V2, f32) = if diagonals && (p[0] - p[1]).abs() < 0.5 {
            ([r2, r2], (p[0] + p[1]) * r2)
        } else if diagonals && (p[0] + p[1]).abs() < 0.5 {
            ([r2, -r2], (p[0] - p[1]) * r2)
        } else if on_row {
            ([1.0, 0.0], p[0])
        } else if on_col {
            ([0.0, 1.0], p[1])
        } else {
            continue;
        };
        let found = ix.incoming.iter().find(|inc| {
            let l = &net.lanes[inc.lane as usize];
            let n = l.polyline.len();
            let d = geom::normalize(geom::sub(
                geom::xy(l.polyline[n - 1]),
                geom::xy(l.polyline[n - 2]),
            ));
            geom::dot(d, dir) > 0.9
        });
        let Some(inc) = found else { continue };
        let v = net.lanes[inc.lane as usize].speed_limit_mps * 0.9;
        let sc = &net.signals[si];
        let start = signals::group_green_start(sc, inc.signal_group).unwrap_or(0.0);
        let cycle = sc.cycle_s();
        net.signals[si].offset_s = (dist / v - start).rem_euclid(cycle);
    }
}

/// Houses on the residential grid blocks (playtest row 95): along each edge of every
/// residential / suburban block, lots from 14 m past one corner to 14 m before the next
/// ([`residential::plan_lot`]); street trees on the verge of local streets and collectors in
/// front of them. Own rng (the block draws above keep the POI choice unchanged). Returns the
/// decor; houses are appended to `buildings`.
#[allow(clippy::too_many_arguments)]
fn residential_blocks(
    cfg: &ProcGenConfig,
    xs: &[f32],
    ys: &[f32],
    row_class: &[RoadClass],
    col_class: &[RoadClass],
    block_kinds: &[(usize, usize, DistrictKind)],
    blocked: &dyn Fn(V2, V2, Option<f32>) -> bool,
    buildings: &mut Vec<Building>,
) -> Vec<Decor> {
    let opt = BuildOptions::default();
    let mut rng = Rng::new(cfg.seed ^ 0x5E51_DE47);
    let mut placed = ObbIndex::default();
    let mut solid = ObbIndex::default();
    let mut decor = Vec::new();
    let mut shops = 0usize;
    let back = |class: RoadClass| {
        let st = style_for(class);
        builder::half_width_of(class, st.lanes, st.lanes, st.parking, &opt) + SIDEWALK_W_M
    };
    let lot_z = KERB_H_M;
    let flat_lot = crate::surface::FLAT_LOT_W_M;
    let aabb_free = |o: &geom::Obb| {
        let r = geom::right_of(o.f);
        let pts: Vec<V2> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .iter()
            .map(|(a, b)| {
                geom::add(
                    o.c,
                    geom::add(geom::scale(o.f, o.hl * a), geom::scale(r, o.hw * b)),
                )
            })
            .collect();
        let lo = [
            pts.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min),
            pts.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min),
        ];
        let hi = [
            pts.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max),
            pts.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max),
        ];
        !blocked(lo, hi, None)
    };
    for &(i, j, kind) in block_kinds {
        if !residential::is_residential(kind) {
            continue;
        }
        // Edges: (street line point, outward unit, along unit, edge length, street class).
        let (x0, x1, y0, y1) = (xs[i], xs[i + 1], ys[j], ys[j + 1]);
        let edges = [
            (
                [x0, y0 + back(row_class[j])],
                [0.0, 1.0],
                [1.0, 0.0],
                x1 - x0,
                row_class[j],
            ),
            (
                [x1, y1 - back(row_class[j + 1])],
                [0.0, -1.0],
                [-1.0, 0.0],
                x1 - x0,
                row_class[j + 1],
            ),
            (
                [x1 - back(col_class[i + 1]), y0],
                [-1.0, 0.0],
                [0.0, 1.0],
                y1 - y0,
                col_class[i + 1],
            ),
            (
                [x0 + back(col_class[i]), y1],
                [1.0, 0.0],
                [0.0, -1.0],
                y1 - y0,
                col_class[i],
            ),
        ];
        for (start, out, along, len, class) in edges {
            let mut mouths: Vec<f32> = Vec::new();
            let mut u = 14.0;
            let mut first = true;
            loop {
                let shop =
                    first && shops < 2 && kind == DistrictKind::Residential && rng.chance(0.08);
                first = false;
                let (lk, w) = residential::next_lot(&mut rng, kind, shop);
                if u + w > len - 14.0 {
                    break;
                }
                let centre = u + w * 0.5;
                u += w;
                let site = LotSite {
                    origin: geom::add(start, geom::scale(along, centre)),
                    along,
                    out,
                    width: w,
                    district: kind,
                    kind: lk,
                };
                let ground = |g: V2| {
                    if geom::dot(geom::sub(g, site.origin), out) < flat_lot - 0.05 {
                        lot_z
                    } else {
                        0.0
                    }
                };
                let plan = residential::plan_lot(&mut rng, &site, &ground);
                let shrink = |o: &geom::Obb| geom::Obb {
                    hl: o.hl - 0.05,
                    hw: o.hw - 0.05,
                    ..*o
                };
                let ok = |o: &geom::Obb| {
                    aabb_free(o) && !placed.clashes(&shrink(o)) && !solid.clashes(o)
                };
                if !ok(&plan.buildings[0].1) {
                    continue;
                }
                if lk == LotKind::Shop {
                    shops += 1;
                }
                let kept: Vec<bool> = plan
                    .buildings
                    .iter()
                    .enumerate()
                    .map(|(k, (_, o))| k == 0 || ok(o))
                    .collect();
                for (k, (b, o)) in plan.buildings.iter().enumerate() {
                    if kept[k] {
                        buildings.push(b.clone());
                        placed.insert(*o);
                    }
                }
                for (k, d, _) in &plan.tied {
                    if kept[*k] {
                        decor.push(*d);
                        mouths.push(geom::dot(geom::sub([d.pos[0], d.pos[1]], start), along));
                    }
                }
                for (d, o) in &plan.decor {
                    if aabb_free(o) && !placed.clashes(o) {
                        decor.push(*d);
                        solid.insert(*o);
                    }
                }
            }
            // Street trees on the verge in front (local streets and collectors).
            if !matches!(class, RoadClass::Local | RoadClass::Collector) {
                continue;
            }
            let inset = SIDEWALK_W_M - VERGE_W_M * 0.5;
            let mut a = 16.0;
            while a < len - 16.0 {
                if mouths
                    .iter()
                    .all(|m| (m - a).abs() >= residential::TREE_DRIVEWAY_CLEAR_M)
                {
                    let p = geom::add(
                        start,
                        geom::add(geom::scale(along, a), geom::scale(out, -inset)),
                    );
                    let trunk = geom::Obb {
                        c: p,
                        f: along,
                        hl: residential::TRUNK_R_M,
                        hw: residential::TRUNK_R_M,
                    };
                    if aabb_free(&trunk) {
                        decor.push(residential::street_tree(&mut rng, p, lot_z));
                    }
                }
                a += residential::STREET_TREE_SPACING_M;
            }
        }
    }
    decor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_city_shape() {
        let net = generate(&ProcGenConfig::default());
        net.validate().unwrap();
        assert!(net.lanes.len() > 1000, "lanes {}", net.lanes.len());
        assert!(net.intersections.len() > 150);
        assert!(net.signals.len() > 30);
        for k in [
            LaneKind::Through,
            LaneKind::Highway,
            LaneKind::Bus,
            LaneKind::Parking,
            LaneKind::Alley,
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
        assert!(net.buildings.len() > 200);
        for c in [
            IntersectionControl::AllWayStop,
            IntersectionControl::Priority,
        ] {
            assert!(net.intersections.iter().any(|i| i.control == c));
        }
        // No cramped junctions: every drivable lane holds at least two cars.
        for (i, l) in net.lanes.iter().enumerate() {
            let len = *geom::cumulative(&l.polyline).last().unwrap();
            assert!(len > 12.0, "lane {i} only {len} m");
        }
        // Every drivable lane can be left (connections) — the city is closed.
        for (i, l) in net.lanes.iter().enumerate() {
            if l.kind.drivable() {
                assert!(!l.connections.is_empty(), "lane {i} is a trap");
            }
        }
    }

    #[test]
    fn deterministic_and_seed_sensitive() {
        let a = generate(&ProcGenConfig::default());
        let b = generate(&ProcGenConfig::default());
        assert_eq!(a, b);
        let c = generate(&ProcGenConfig {
            seed: 2,
            ..Default::default()
        });
        assert_ne!(a.buildings, c.buildings);
        assert_eq!(a.lanes, c.lanes);
    }

    #[test]
    fn small_city_has_every_feature() {
        let net = generate(&ProcGenConfig::small());
        net.validate().unwrap();
        for k in [
            LaneKind::Highway,
            LaneKind::Bus,
            LaneKind::Parking,
            LaneKind::Alley,
        ] {
            assert!(net.lanes.iter().any(|l| l.kind == k), "no {k:?} lanes");
        }
        assert!(!net.signals.is_empty());
        assert!(net.lanes.len() < 800, "{}", net.lanes.len());
    }

    #[test]
    fn highway_is_a_divided_viaduct_with_ramps() {
        use crate::map::{CityMap, SegKind};
        for (n, ramps_expected) in [(12usize, 8usize), (7, 4), (6, 4)] {
            let net = generate(&ProcGenConfig {
                nx: n,
                ny: n,
                ..Default::default()
            });
            let map = CityMap::new(net.clone());
            assert!(net.terrain.is_some());
            let hw: Vec<&Lane> = net
                .lanes
                .iter()
                .filter(|l| l.kind == LaneKind::Highway)
                .collect();
            // Ramps are the one-lane highway roads.
            let mut ramp_roads: Vec<u32> = hw
                .iter()
                .filter(|l| l.speed_limit_mps < mph(50.0))
                .map(|l| l.road)
                .collect();
            ramp_roads.sort_unstable();
            ramp_roads.dedup();
            assert_eq!(ramp_roads.len(), ramps_expected, "{n}x{n}");
            // One way, three lanes per carriageway; no signal or stop on the main line.
            for (i, l) in net.lanes.iter().enumerate() {
                if l.kind != LaneKind::Highway || ramp_roads.contains(&l.road) {
                    continue;
                }
                assert!(l.index_from_left < 3);
                match map.lane_control(i as LaneId) {
                    None => {}
                    Some((IntersectionControl::Priority, ap, _)) => assert!(ap.major),
                    // The ring road junction at each end.
                    Some((IntersectionControl::Signal { .. }, _, _)) => {
                        let end = l.polyline[l.polyline.len() - 1];
                        assert!(end[2].abs() < 0.05, "signal on the viaduct at {end:?}");
                    }
                    Some(c) => panic!("highway lane {i} ends at {:?}", c.0),
                }
            }
            // The viaduct bridges every street it crosses with full clearance.
            let mut over = 0;
            for l in &net.lanes {
                if l.kind == LaneKind::Highway || l.kind == LaneKind::Parking {
                    continue;
                }
                let cum = geom::cumulative(&l.polyline);
                let len = cum[cum.len() - 1];
                for k in 0..=(len / 2.0) as usize {
                    let (p, _) = geom::point_at(&l.polyline, &cum, k as f32 * 2.0);
                    let high = map.ground_height(geom::xy(p), Some(100.0));
                    if high - p[2] > 1.0 {
                        assert!(high - p[2] > 7.0, "clearance {} at {p:?}", high - p[2]);
                        over += 1;
                    }
                }
            }
            assert!(over > 0, "{n}x{n}: nothing under the viaduct");
            // Grades and the median barrier.
            for l in &hw {
                for w in l.polyline.windows(2) {
                    let d = geom::dist(geom::xy(w[0]), geom::xy(w[1])).max(0.5);
                    assert!(
                        ((w[1][2] - w[0][2]) / d).abs() < 0.085,
                        "grade at {:?}",
                        w[0]
                    );
                }
            }
            let barrier: f32 = crate::surface::median_barriers(&net)
                .iter()
                .map(|l| *geom::cumulative(l).last().unwrap())
                .sum();
            assert!(barrier > 600.0, "{n}x{n}: barrier {barrier} m");
            // Taper connectors lie on the paving.
            for s in map.segs.iter() {
                let SegKind::Connector { node, .. } = s.kind else {
                    continue;
                };
                let Some(ix) = net.nodes[node as usize].intersection else {
                    continue;
                };
                if net.intersections[ix as usize].radius_m > 0.0 {
                    continue;
                }
                for p in &s.pts {
                    let z = net
                        .surfaces
                        .iter()
                        .map(|sf| crate::surface::height_under(sf, geom::xy(*p)))
                        .filter(|(d, z)| *d < 8.0 && (z - p[2]).abs() < 3.0)
                        .min_by(|a, b| a.0.total_cmp(&b.0))
                        .map(|x| x.1)
                        .unwrap();
                    assert!((z - p[2]).abs() < 0.05, "taper {} m off the road", z - p[2]);
                }
            }
        }
    }

    #[test]
    fn odd_and_rectangular_grids_build() {
        for (nx, ny) in [(7, 7), (8, 5), (5, 9)] {
            let net = generate(&ProcGenConfig {
                nx,
                ny,
                ..Default::default()
            });
            net.validate().unwrap();
        }
    }
}
