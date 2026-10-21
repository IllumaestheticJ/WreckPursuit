//! Lane-graph builder shared by the procedural generator and the OSM importer.
//!
//! Input is an abstract road graph ([`NodeSpec`] + [`RoadSpec`]: centreline, lane counts per
//! direction, class, speed, parking, optional `turn:lanes`). The builder:
//! 1. computes a junction setback per road end from road widths and junction angles,
//! 2. emits directed lanes offset from the trimmed centreline (right-hand traffic),
//! 3. assigns turn movements per approach lane (US defaults or explicit `turn:lanes`) and wires
//!    lane connections,
//! 4. creates intersections with a control rule (signals / priority / all-way stop) and a
//!    fixed-time signal plan pairing opposite approaches into phases.

use std::collections::BTreeSet;

use crate::geom::{self, V2, V3};
use crate::network::*;

/// Functional road class. Drives defaults (speed, lane width, signal timing, junction control).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum RoadClass {
    /// Limited-access highway.
    Highway,
    /// Major arterial.
    Arterial,
    /// Collector street.
    Collector,
    /// Local / residential street.
    Local,
    /// Service alley.
    Alley,
}

impl RoadClass {
    /// Priority rank (higher = more important).
    pub fn rank(self) -> u8 {
        match self {
            RoadClass::Highway => 4,
            RoadClass::Arterial => 3,
            RoadClass::Collector => 2,
            RoadClass::Local => 1,
            RoadClass::Alley => 0,
        }
    }

    /// US default speed limit when the source has none, m/s
    /// (residential 25 mph, collector 30 mph, arterial 40 mph, highway 60 mph, alley 15 mph).
    pub fn default_speed_mps(self) -> f32 {
        mph(match self {
            RoadClass::Highway => 60.0,
            RoadClass::Arterial => 40.0,
            RoadClass::Collector => 30.0,
            RoadClass::Local => 25.0,
            RoadClass::Alley => 15.0,
        })
    }

    /// Green time for a phase serving this class, s.
    pub fn green_s(self) -> f32 {
        match self {
            RoadClass::Highway => 30.0,
            RoadClass::Arterial => 26.0,
            RoadClass::Collector => 20.0,
            RoadClass::Local => 14.0,
            RoadClass::Alley => 10.0,
        }
    }
}

/// Miles per hour → m/s.
pub fn mph(v: f32) -> f32 {
    v * 0.447_04
}

/// Explicit control request for a junction node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlHint {
    /// Force signals.
    Signal,
    /// Force all-way stop.
    AllWayStop,
    /// Force priority (major = highest-class roads).
    Priority,
    /// Force no control.
    Uncontrolled,
    /// Freeway merge or diverge between one-way roads (a carriageway and a ramp): every lane
    /// is set back [`FREEWAY_SETBACK_M`] so the ramp lane tapers in or out along a connector,
    /// every road's paving runs to the node (no junction disc), the main road keeps
    /// its lanes and the ramp lanes join or leave its rightmost lanes. Control is
    /// [`IntersectionControl::Priority`] with the main road major and a radius-0 junction
    /// (no box); traffic on a merging ramp yields without stopping. Only applies to a node with three one-way roads (one in and
    /// two out, or two in and one out); anything else is built as [`ControlHint::Priority`].
    Freeway,
    /// Ramp terminal on a street: priority where the two-way roads are major and the one-way
    /// ramps minor (an off-ramp stops and yields; the street keeps flowing).
    RampTerminal,
}

/// Lane setback at a [`ControlHint::Freeway`] node: half the length of the taper connector, m.
pub const FREEWAY_SETBACK_M: f32 = 30.0;

/// A graph node to build.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeSpec {
    /// Position, m.
    pub pos: V2,
    /// Elevation, m.
    pub z: f32,
    /// Tagged as signalised in the source data.
    pub signal: bool,
    /// Explicit control override.
    pub control: Option<ControlHint>,
}

/// Movements allowed from one lane (`turn:lanes` entry).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TurnSet {
    /// Left allowed.
    pub left: bool,
    /// Straight allowed.
    pub straight: bool,
    /// Right allowed.
    pub right: bool,
}

/// A road (both directions) between two nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct RoadSpec {
    /// Start node index.
    pub a: usize,
    /// End node index.
    pub b: usize,
    /// Interior centreline points (a → b), m.
    pub via: Vec<V2>,
    /// Centreline height at each `via` point, m (empty = linear between the node heights by
    /// arc length).
    pub via_z: Vec<f32>,
    /// Cross slope at each centreline point `[a, via.., b]` (dz per metre to the right of
    /// a → b; empty = flat). Keep it 0 near junctions.
    pub bank: Vec<f32>,
    /// Functional class.
    pub class: RoadClass,
    /// Driving lanes a → b (≥ 1).
    pub lanes_fwd: u8,
    /// Driving lanes b → a (0 = one-way).
    pub lanes_bwd: u8,
    /// Speed limit, m/s.
    pub speed_mps: f32,
    /// Kerbside parking lanes on each side.
    pub parking: bool,
    /// Rightmost lane of each direction is a bus lane (needs ≥ 2 lanes in that direction).
    pub bus_lane: bool,
    /// Explicit `turn:lanes` for a → b, left to right (empty = defaults).
    pub turn_lanes_fwd: Vec<TurnSet>,
    /// Explicit `turn:lanes` for b → a, left to right (empty = defaults).
    pub turn_lanes_bwd: Vec<TurnSet>,
    /// Extra paving (shoulder) beyond the lanes on the left of a → b, m. Paving only: lanes
    /// and markings are unchanged.
    pub shoulder_left_m: f32,
    /// Extra paving (shoulder) beyond the lanes on the right of a → b, m.
    pub shoulder_right_m: f32,
    /// A median barrier runs along the left paved edge (see [`RoadSurface::median_barrier`]).
    pub median_barrier: bool,
}

impl RoadSpec {
    /// A road with class defaults: given lane counts, default speed, no parking/bus/turn tags.
    pub fn new(a: usize, b: usize, class: RoadClass, lanes_fwd: u8, lanes_bwd: u8) -> Self {
        Self {
            a,
            b,
            via: Vec::new(),
            via_z: Vec::new(),
            bank: Vec::new(),
            class,
            lanes_fwd,
            lanes_bwd,
            speed_mps: class.default_speed_mps(),
            parking: false,
            bus_lane: false,
            turn_lanes_fwd: Vec::new(),
            turn_lanes_bwd: Vec::new(),
            shoulder_left_m: 0.0,
            shoulder_right_m: 0.0,
            median_barrier: false,
        }
    }
}

/// Builder tuning.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildOptions {
    /// Urban lane width, m.
    pub lane_width_m: f32,
    /// Highway lane width, m.
    pub highway_lane_width_m: f32,
    /// Alley lane width, m.
    pub alley_lane_width_m: f32,
    /// Parking lane width, m.
    pub parking_width_m: f32,
    /// Allow right turn on red at signals.
    pub allow_rtor: bool,
    /// Extra junction setback for crosswalks, m.
    pub crosswalk_m: f32,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            lane_width_m: 3.5,
            highway_lane_width_m: 3.7,
            alley_lane_width_m: 3.0,
            parking_width_m: 2.5,
            allow_rtor: true,
            crosswalk_m: 2.0,
        }
    }
}

/// Lanes emitted for one road.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoadLanes {
    /// Driving lanes a → b, left to right.
    pub fwd: Vec<LaneId>,
    /// Driving lanes b → a, left to right.
    pub bwd: Vec<LaneId>,
    /// Parking lanes.
    pub parking: Vec<LaneId>,
}

/// Builder output (everything but zones, POIs and buildings).
#[derive(Debug, Clone, PartialEq)]
pub struct Built {
    /// Nodes.
    pub nodes: Vec<Node>,
    /// Lanes.
    pub lanes: Vec<Lane>,
    /// Intersections.
    pub intersections: Vec<Intersection>,
    /// Signal controllers.
    pub signals: Vec<SignalController>,
    /// Lanes per input road (same order as the input).
    pub road_lanes: Vec<RoadLanes>,
    /// Paved surface per input road (same order as the input).
    pub surfaces: Vec<RoadSurface>,
}

impl Built {
    /// Wrap into a [`RoadNetwork`] (bounds computed from lanes, buildings and zones).
    #[allow(clippy::too_many_arguments)]
    pub fn into_network(
        self,
        id: &str,
        source: &str,
        attribution: Option<String>,
        core: V2,
        zones: Vec<Zone>,
        pois: Vec<Poi>,
        buildings: Vec<Building>,
    ) -> RoadNetwork {
        let mut lo = [f32::INFINITY; 2];
        let mut hi = [f32::NEG_INFINITY; 2];
        let mut grow = |p: V2| {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        };
        for l in &self.lanes {
            for p in &l.polyline {
                grow(geom::xy(*p));
            }
        }
        for z in &zones {
            for p in &z.polygon {
                grow(*p);
            }
        }
        for b in &buildings {
            for p in &b.footprint {
                grow(*p);
            }
        }
        if !lo[0].is_finite() {
            lo = [0.0; 2];
            hi = [0.0; 2];
        }
        RoadNetwork {
            format_version: ROAD_FORMAT_VERSION,
            id: id.to_string(),
            source: source.to_string(),
            attribution,
            bounds_min: lo,
            bounds_max: hi,
            core,
            nodes: self.nodes,
            lanes: self.lanes,
            intersections: self.intersections,
            signals: self.signals,
            zones,
            pois,
            buildings,
            surfaces: self.surfaces,
            terrain: None,
        }
    }
}

/// Roles at a [`ControlHint::Freeway`] node (indices into the node's incidence list).
#[derive(Debug, Clone, Copy)]
struct FreewayNode {
    /// Main road arriving.
    in_main: usize,
    /// Main road leaving.
    out_main: usize,
    /// The ramp (arriving at a merge, leaving at a diverge).
    ramp: usize,
    /// The ramp arrives (merge) rather than leaves (diverge).
    merge: bool,
}

/// Lanes arriving at the node along incidence `x` and lanes leaving along it.
fn in_out_counts(r: &RoadSpec, x: &Incidence) -> (u8, u8) {
    if x.at_a {
        (r.lanes_bwd, r.lanes_fwd)
    } else {
        (r.lanes_fwd, r.lanes_bwd)
    }
}

/// Freeway roles at a node, if it is a valid [`ControlHint::Freeway`] node.
fn freeway_node(node: &NodeSpec, list: &[Incidence], roads: &[RoadSpec]) -> Option<FreewayNode> {
    if node.control != Some(ControlHint::Freeway) || list.len() != 3 {
        return None;
    }
    if list.iter().any(|x| roads[x.road].lanes_bwd != 0) {
        return None;
    }
    let lanes = |k: usize| {
        let (i, o) = in_out_counts(&roads[list[k].road], &list[k]);
        i.max(o)
    };
    let ins: Vec<usize> = (0..3)
        .filter(|k| in_out_counts(&roads[list[*k].road], &list[*k]).0 > 0)
        .collect();
    let outs: Vec<usize> = (0..3).filter(|k| !ins.contains(k)).collect();
    // The main road has more lanes; on a tie, the straighter continuation.
    let pick = |pair: &[usize], other: usize| -> (usize, usize) {
        let straight = |k: usize| -geom::dot(list[k].dir_out, list[other].dir_out);
        let (a, b) = (pair[0], pair[1]);
        let a_main = lanes(a) > lanes(b) || (lanes(a) == lanes(b) && straight(a) >= straight(b));
        if a_main {
            (a, b)
        } else {
            (b, a)
        }
    };
    match (ins.len(), outs.len()) {
        (1, 2) => {
            let (out_main, ramp) = pick(&outs, ins[0]);
            Some(FreewayNode {
                in_main: ins[0],
                out_main,
                ramp,
                merge: false,
            })
        }
        (2, 1) => {
            let (in_main, ramp) = pick(&ins, outs[0]);
            Some(FreewayNode {
                in_main,
                out_main: outs[0],
                ramp,
                merge: true,
            })
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
struct Incidence {
    road: usize,
    at_a: bool,
    /// Unit direction leaving the node along the road.
    dir_out: V2,
}

fn road_points(nodes: &[NodeSpec], r: &RoadSpec) -> Vec<V3> {
    let a = nodes[r.a].pos;
    let b = nodes[r.b].pos;
    let mut xy: Vec<V2> = Vec::with_capacity(r.via.len() + 2);
    xy.push(a);
    xy.extend_from_slice(&r.via);
    xy.push(b);
    // Interpolate elevation by arc length.
    let mut cum = vec![0.0f32];
    for w in xy.windows(2) {
        let l = cum[cum.len() - 1] + geom::dist(w[0], w[1]);
        cum.push(l);
    }
    let total = cum[cum.len() - 1].max(1e-6);
    let (za, zb) = (nodes[r.a].z, nodes[r.b].z);
    let explicit = r.via_z.len() == r.via.len() && !r.via.is_empty();
    xy.iter()
        .zip(&cum)
        .enumerate()
        .map(|(k, (p, c))| {
            let z = if explicit && k > 0 && k <= r.via.len() {
                r.via_z[k - 1]
            } else {
                za + (zb - za) * c / total
            };
            [p[0], p[1], z]
        })
        .collect()
}

/// Bank (cross slope) at each point of `trimmed`, which starts `s0` metres into the road's
/// full centreline `full` (interpolated by arc length; 0 without bank data).
fn trimmed_bank(r: &RoadSpec, full: &[V3], trimmed: &[V3], s0: f32) -> Vec<f32> {
    if r.bank.len() != full.len() {
        return vec![0.0; trimmed.len()];
    }
    let cum_full = geom::cumulative(full);
    let cum_t = geom::cumulative(trimmed);
    cum_t
        .iter()
        .map(|c| {
            let s = s0 + c;
            let i = match cum_full.binary_search_by(|x| x.total_cmp(&s)) {
                Ok(i) => i.min(full.len() - 2),
                Err(i) => i.saturating_sub(1).min(full.len() - 2),
            };
            let len = (cum_full[i + 1] - cum_full[i]).max(1e-6);
            let t = ((s - cum_full[i]) / len).clamp(0.0, 1.0);
            r.bank[i] + (r.bank[i + 1] - r.bank[i]) * t
        })
        .collect()
}

/// Offset `center` by `off` (right of its direction) and lift each point by `bank * lat`,
/// where `lat = off * sign` is the offset in the road's a → b frame.
fn lane_on_surface(center: &[V3], bank: &[f32], off: f32, sign: f32) -> Vec<V3> {
    let mut pts = geom::offset_polyline(center, off);
    for (p, b) in pts.iter_mut().zip(bank) {
        p[2] += b * off * sign;
    }
    pts
}

fn lane_width(class: RoadClass, opt: &BuildOptions) -> f32 {
    match class {
        RoadClass::Highway => opt.highway_lane_width_m,
        RoadClass::Alley => opt.alley_lane_width_m,
        _ => opt.lane_width_m,
    }
}

/// Half width of the side of the road a lane group occupies (max of both sides), m.
fn half_width(r: &RoadSpec, opt: &BuildOptions) -> f32 {
    let w = lane_width(r.class, opt);
    let p = if r.parking { opt.parking_width_m } else { 0.0 };
    if r.lanes_bwd == 0 {
        r.lanes_fwd as f32 * w * 0.5 + p
    } else {
        r.lanes_fwd.max(r.lanes_bwd) as f32 * w + p
    }
}

/// Build the lane graph.
pub fn build(nodes: &[NodeSpec], roads: &[RoadSpec], opt: &BuildOptions) -> Built {
    let pts: Vec<Vec<V3>> = roads.iter().map(|r| road_points(nodes, r)).collect();

    // A. Incidences per node, sorted counter-clockwise.
    let mut inc: Vec<Vec<Incidence>> = vec![Vec::new(); nodes.len()];
    for (ri, r) in roads.iter().enumerate() {
        let p = &pts[ri];
        let n = p.len();
        let d_a = geom::normalize(geom::sub(geom::xy(p[1]), geom::xy(p[0])));
        let d_b = geom::normalize(geom::sub(geom::xy(p[n - 2]), geom::xy(p[n - 1])));
        inc[r.a].push(Incidence {
            road: ri,
            at_a: true,
            dir_out: d_a,
        });
        inc[r.b].push(Incidence {
            road: ri,
            at_a: false,
            dir_out: d_b,
        });
    }
    for list in &mut inc {
        list.sort_by(|x, y| {
            geom::pseudo_angle(x.dir_out)
                .total_cmp(&geom::pseudo_angle(y.dir_out))
                .then(x.road.cmp(&y.road))
                .then(x.at_a.cmp(&y.at_a))
        });
    }

    let freeway: Vec<Option<FreewayNode>> = inc
        .iter()
        .enumerate()
        .map(|(ni, list)| freeway_node(&nodes[ni], list, roads))
        .collect();

    // B. Setbacks per (road, end). Paving is trimmed like the lanes except where a freeway
    // main road runs through a merge or diverge.
    let hw: Vec<f32> = roads.iter().map(|r| half_width(r, opt)).collect();
    let mut setback_a = vec![0.0f32; roads.len()];
    let mut setback_b = vec![0.0f32; roads.len()];
    let mut paving_a = vec![f32::INFINITY; roads.len()];
    let mut paving_b = vec![f32::INFINITY; roads.len()];
    let mut node_radius = vec![0.0f32; nodes.len()];
    for (ni, list) in inc.iter().enumerate() {
        let deg = list.len();
        let mut sb = vec![0.0f32; deg];
        if freeway[ni].is_some() {
            // Every road's paving runs to the node: the ramp's lies on the carriageway's plane
            // there, so the gore between the taper and the ramp is paved too.
            for (k, x) in list.iter().enumerate() {
                sb[k] = FREEWAY_SETBACK_M;
                if x.at_a {
                    paving_a[x.road] = 0.0;
                } else {
                    paving_b[x.road] = 0.0;
                }
            }
            for (k, x) in list.iter().enumerate() {
                if x.at_a {
                    setback_a[x.road] = sb[k];
                } else {
                    setback_b[x.road] = sb[k];
                }
            }
            continue;
        }
        if deg >= 3 {
            for i in 0..deg {
                sb[i] = hw[list[i].road] + opt.crosswalk_m;
            }
            for i in 0..deg {
                let j = (i + 1) % deg;
                let (di, dj) = (list[i].dir_out, list[j].dir_out);
                let s = geom::cross(di, dj); // sin of CCW angle i → j
                let c = geom::dot(di, dj);
                if s < 0.2 {
                    continue; // ≥ ~168° apart: no overlap on this side
                }
                let (hi, hj) = (hw[list[i].road], hw[list[j].road]);
                let d_i = (hj + hi * c) / s;
                let d_j = (hi + hj * c) / s;
                sb[i] = sb[i].max(d_i + opt.crosswalk_m);
                sb[j] = sb[j].max(d_j + opt.crosswalk_m);
            }
            for v in &mut sb {
                *v = v.min(45.0);
            }
        } else if deg == 2 {
            let (d0, d1) = (list[0].dir_out, list[1].dir_out);
            let c = geom::dot(d0, d1);
            if c < -0.97 {
                sb = vec![0.5, 0.5];
            } else {
                let s = geom::cross(d0, d1).abs().max(0.2);
                let h = hw[list[0].road].max(hw[list[1].road]);
                let d = (h + h * c.max(-1.0)) / s;
                sb = vec![d.clamp(0.5, 30.0) + 0.5; 2];
            }
        }
        for (k, x) in list.iter().enumerate() {
            if x.at_a {
                setback_a[x.road] = sb[k];
            } else {
                setback_b[x.road] = sb[k];
            }
            node_radius[ni] = node_radius[ni].max(sb[k]);
        }
    }
    for ri in 0..roads.len() {
        let total = *geom::cumulative(&pts[ri]).last().unwrap();
        let need = setback_a[ri] + setback_b[ri] + 4.0;
        if need > total {
            let k = ((total - 4.0).max(0.5)) / (setback_a[ri] + setback_b[ri]).max(1e-3);
            setback_a[ri] *= k.min(1.0);
            setback_b[ri] *= k.min(1.0);
        }
        paving_a[ri] = paving_a[ri].min(setback_a[ri]);
        paving_b[ri] = paving_b[ri].min(setback_b[ri]);
    }

    // C. Lanes.
    let mut lanes: Vec<Lane> = Vec::new();
    let mut road_lanes: Vec<RoadLanes> = vec![RoadLanes::default(); roads.len()];
    let mut surfaces: Vec<RoadSurface> = Vec::with_capacity(roads.len());
    for (ri, r) in roads.iter().enumerate() {
        let trimmed = geom::trim_polyline(&pts[ri], setback_a[ri], setback_b[ri]);
        let total = *geom::cumulative(&pts[ri]).last().unwrap();
        let s0 = if setback_a[ri] + setback_b[ri] < total - 0.5 {
            setback_a[ri]
        } else {
            total * 0.5 - 0.25
        };
        let bank_fwd = trimmed_bank(r, &pts[ri], &trimmed, s0);
        let paved = if paving_a[ri] == setback_a[ri] && paving_b[ri] == setback_b[ri] {
            trimmed.clone()
        } else {
            geom::trim_polyline(&pts[ri], paving_a[ri], paving_b[ri])
        };
        let paved_bank = trimmed_bank(r, &pts[ri], &paved, paving_a[ri].min(s0));
        let mut bank_rev = bank_fwd.clone();
        bank_rev.reverse();
        let mut rev = trimmed.clone();
        rev.reverse();
        let w = lane_width(r.class, opt);
        let one_way = r.lanes_bwd == 0;
        let park = if r.parking { opt.parking_width_m } else { 0.0 };
        let (left_m, right_m) = if one_way {
            let h = r.lanes_fwd as f32 * w * 0.5 + park;
            (h, h)
        } else {
            (r.lanes_bwd as f32 * w + park, r.lanes_fwd as f32 * w + park)
        };
        surfaces.push(RoadSurface {
            road: ri as u32,
            centre: paved,
            bank: paved_bank,
            left_m: left_m + r.shoulder_left_m,
            right_m: right_m + r.shoulder_right_m,
            highway: r.class == RoadClass::Highway,
            sidewalk: [r.class != RoadClass::Highway, r.class != RoadClass::Highway],
            median_barrier: r.median_barrier,
        });
        let base_kind = match r.class {
            RoadClass::Highway => LaneKind::Highway,
            RoadClass::Alley => LaneKind::Alley,
            _ => LaneKind::Through,
        };
        for (dir_fwd, count) in [(true, r.lanes_fwd), (false, r.lanes_bwd)] {
            if count == 0 {
                continue;
            }
            let center = if dir_fwd { &trimmed } else { &rev };
            let bank = if dir_fwd { &bank_fwd } else { &bank_rev };
            let sign = if dir_fwd { 1.0 } else { -1.0 };
            let (from, to) = if dir_fwd { (r.a, r.b) } else { (r.b, r.a) };
            let first = lanes.len() as LaneId;
            for j in 0..count {
                let off = if one_way {
                    (j as f32 + 0.5 - count as f32 * 0.5) * w
                } else {
                    (j as f32 + 0.5) * w
                };
                let kind = if r.bus_lane && count >= 2 && j == count - 1 {
                    LaneKind::Bus
                } else {
                    base_kind
                };
                lanes.push(Lane {
                    from_node: from as NodeId,
                    to_node: to as NodeId,
                    road: ri as u32,
                    kind,
                    index_from_left: j,
                    polyline: lane_on_surface(center, bank, off, sign),
                    width_m: w,
                    speed_limit_mps: r.speed_mps,
                    left: if j > 0 {
                        Some(first + j as u32 - 1)
                    } else {
                        None
                    },
                    right: if j + 1 < count {
                        Some(first + j as u32 + 1)
                    } else {
                        None
                    },
                    edge_left: one_way && j == 0,
                    edge_right: j == count - 1 && !r.parking,
                    connections: Vec::new(),
                });
                if dir_fwd {
                    road_lanes[ri].fwd.push(first + j as u32);
                } else {
                    road_lanes[ri].bwd.push(first + j as u32);
                }
            }
            if r.parking {
                let edge = if one_way {
                    count as f32 * w * 0.5
                } else {
                    count as f32 * w
                };
                let id = lanes.len() as LaneId;
                lanes.push(Lane {
                    from_node: from as NodeId,
                    to_node: to as NodeId,
                    road: ri as u32,
                    kind: LaneKind::Parking,
                    index_from_left: count,
                    polyline: lane_on_surface(center, bank, edge + opt.parking_width_m * 0.5, sign),
                    width_m: opt.parking_width_m,
                    speed_limit_mps: r.speed_mps,
                    left: None,
                    right: None,
                    edge_left: false,
                    edge_right: true,
                    connections: Vec::new(),
                });
                road_lanes[ri].parking.push(id);
            }
        }
    }

    // D. Connections.
    let lane_dir_end = |l: &Lane| {
        let n = l.polyline.len();
        geom::normalize(geom::sub(
            geom::xy(l.polyline[n - 1]),
            geom::xy(l.polyline[n - 2]),
        ))
    };
    let lane_dir_start =
        |l: &Lane| geom::normalize(geom::sub(geom::xy(l.polyline[1]), geom::xy(l.polyline[0])));
    let incoming_of = |x: &Incidence| -> Vec<LaneId> {
        let rl = &road_lanes[x.road];
        if x.at_a {
            rl.bwd.clone()
        } else {
            rl.fwd.clone()
        }
    };
    let outgoing_of = |x: &Incidence| -> Vec<LaneId> {
        let rl = &road_lanes[x.road];
        if x.at_a {
            rl.fwd.clone()
        } else {
            rl.bwd.clone()
        }
    };
    let mut new_conns: Vec<(LaneId, LaneConnection)> = Vec::new();
    for (ni, list) in inc.iter().enumerate() {
        if let Some(fw) = freeway[ni] {
            // Main lanes keep going left-aligned; the ramp's lanes join (merge) or leave
            // (diverge) the main road's rightmost lanes.
            let main_in = incoming_of(&list[fw.in_main]);
            let main_out = outgoing_of(&list[fw.out_main]);
            for (k, l) in main_in.iter().enumerate() {
                let to = main_out[k.min(main_out.len() - 1)];
                new_conns.push((
                    *l,
                    LaneConnection {
                        to,
                        turn: TurnKind::Straight,
                    },
                ));
            }
            let (from, to, turn) = if fw.merge {
                (incoming_of(&list[fw.ramp]), main_out, TurnKind::Straight)
            } else {
                (main_in, outgoing_of(&list[fw.ramp]), TurnKind::Right)
            };
            // Right-align the smaller side against the larger.
            let n = from.len().min(to.len());
            for j in 0..n {
                new_conns.push((
                    from[from.len() - n + j],
                    LaneConnection {
                        to: to[to.len() - n + j],
                        turn,
                    },
                ));
            }
            continue;
        }
        let deg = list.len();
        for (ai, app) in list.iter().enumerate() {
            let in_lanes = incoming_of(app);
            if in_lanes.is_empty() {
                continue;
            }
            let d_in = lane_dir_end(&lanes[in_lanes[0] as usize]);
            // Exits with their movement kind.
            let mut exits: Vec<(TurnKind, Vec<LaneId>)> = Vec::new();
            for (ei, ex) in list.iter().enumerate() {
                let out = outgoing_of(ex);
                if out.is_empty() {
                    continue;
                }
                let kind = if deg == 1 {
                    TurnKind::UTurn
                } else if ei == ai {
                    continue;
                } else if deg == 2 {
                    TurnKind::Straight
                } else {
                    let d_out = lane_dir_start(&lanes[out[0] as usize]);
                    let c = geom::dot(d_in, d_out);
                    if c > 0.819 {
                        TurnKind::Straight
                    } else if c < -0.94 {
                        continue;
                    } else if geom::cross(d_in, d_out) > 0.0 {
                        TurnKind::Left
                    } else {
                        TurnKind::Right
                    }
                };
                exits.push((kind, out));
            }
            if exits.is_empty() {
                continue;
            }
            let avail: BTreeSet<TurnKind> = exits.iter().map(|e| e.0).collect();
            let general: Vec<LaneId> = in_lanes
                .iter()
                .copied()
                .filter(|l| lanes[*l as usize].kind != LaneKind::Bus)
                .collect();
            let bus: Vec<LaneId> = in_lanes
                .iter()
                .copied()
                .filter(|l| lanes[*l as usize].kind == LaneKind::Bus)
                .collect();
            let road = &roads[app.road];
            let tags = if app.at_a {
                &road.turn_lanes_bwd
            } else {
                &road.turn_lanes_fwd
            };
            let allowed = lane_movements(&general, &avail, tags, in_lanes.len());
            // Bus lanes: straight and right (or whatever exists).
            let mut bus_allowed: Vec<BTreeSet<TurnKind>> = Vec::new();
            for _ in &bus {
                let mut s: BTreeSet<TurnKind> = [TurnKind::Straight, TurnKind::Right]
                    .into_iter()
                    .filter(|k| avail.contains(k))
                    .collect();
                if s.is_empty() {
                    s = avail.clone();
                }
                bus_allowed.push(s);
            }
            for (kind, out) in &exits {
                let out_general: Vec<LaneId> = out
                    .iter()
                    .copied()
                    .filter(|l| lanes[*l as usize].kind != LaneKind::Bus)
                    .collect();
                let out_bus: Vec<LaneId> = out
                    .iter()
                    .copied()
                    .filter(|l| lanes[*l as usize].kind == LaneKind::Bus)
                    .collect();
                if out_general.is_empty() {
                    continue;
                }
                let src: Vec<LaneId> = general
                    .iter()
                    .zip(&allowed)
                    .filter(|(_, a)| a.contains(kind))
                    .map(|(l, _)| *l)
                    .collect();
                let s = src.len();
                let t = out_general.len();
                for (k, l) in src.iter().enumerate() {
                    let ti = match kind {
                        TurnKind::Left | TurnKind::UTurn => k.min(t - 1),
                        _ => (t + k).saturating_sub(s).min(t - 1),
                    };
                    new_conns.push((
                        *l,
                        LaneConnection {
                            to: out_general[ti],
                            turn: *kind,
                        },
                    ));
                }
                for (b, ba) in bus.iter().zip(&bus_allowed) {
                    if ba.contains(kind) {
                        let to = if *kind == TurnKind::Straight && !out_bus.is_empty() {
                            out_bus[0]
                        } else {
                            out_general[t - 1]
                        };
                        new_conns.push((*b, LaneConnection { to, turn: *kind }));
                    }
                }
            }
        }
    }
    for (l, c) in new_conns {
        lanes[l as usize].connections.push(c);
    }
    for l in &mut lanes {
        if matches!(l.kind, LaneKind::Through) && !l.connections.is_empty() {
            if l.connections.iter().all(|c| c.turn == TurnKind::Left) {
                l.kind = LaneKind::TurnLeft;
            } else if l.connections.iter().all(|c| c.turn == TurnKind::Right) {
                l.kind = LaneKind::TurnRight;
            }
        }
    }

    // E. Intersections and signal plans.
    let mut out_nodes: Vec<Node> = nodes
        .iter()
        .map(|n| Node {
            pos: [n.pos[0], n.pos[1], n.z],
            intersection: None,
        })
        .collect();
    let mut intersections = Vec::new();
    let mut signals = Vec::new();
    for (ni, list) in inc.iter().enumerate() {
        if list.len() < 3 {
            continue;
        }
        let fw = freeway[ni];
        let ranks: Vec<u8> = list.iter().map(|x| roads[x.road].class.rank()).collect();
        let max_rank = *ranks.iter().max().unwrap();
        let min_rank = *ranks.iter().min().unwrap();
        // Signals where every road is collector or above, or at complex (5+ way) junctions;
        // priority (major road keeps going) where classes differ; all-way stop otherwise.
        let control_kind = match nodes[ni].control {
            Some(ControlHint::Freeway) | Some(ControlHint::RampTerminal) => ControlHint::Priority,
            Some(h) => h,
            None if nodes[ni].signal => ControlHint::Signal,
            None if min_rank >= 2 || list.len() >= 5 => ControlHint::Signal,
            None if max_rank > min_rank => ControlHint::Priority,
            None => ControlHint::AllWayStop,
        };
        let ix_id = intersections.len() as IntersectionId;
        let mut incoming = Vec::new();
        let mut approaches: Vec<(u8, V2, RoadClass, f32)> = Vec::new(); // group, dir_out, class, speed
        for (k, x) in list.iter().enumerate() {
            let in_lanes: Vec<LaneId> = if x.at_a {
                road_lanes[x.road].bwd.clone()
            } else {
                road_lanes[x.road].fwd.clone()
            };
            if in_lanes.is_empty() {
                continue;
            }
            let g = approaches.len() as u8;
            let r = &roads[x.road];
            approaches.push((g, x.dir_out, r.class, r.speed_mps));
            for l in in_lanes {
                incoming.push(IncomingLane {
                    lane: l,
                    signal_group: g,
                    major: match fw {
                        Some(f) => k == f.in_main,
                        None if nodes[ni].control == Some(ControlHint::RampTerminal) => {
                            r.lanes_bwd > 0
                        }
                        None => r.class.rank() == max_rank,
                    },
                });
            }
        }
        let control = match control_kind {
            ControlHint::Signal => {
                let sid = signals.len() as SignalId;
                signals.push(signal_plan(ix_id, &approaches));
                IntersectionControl::Signal { controller: sid }
            }
            ControlHint::AllWayStop => IntersectionControl::AllWayStop,
            ControlHint::Priority => IntersectionControl::Priority,
            ControlHint::Uncontrolled => IntersectionControl::Uncontrolled,
            ControlHint::Freeway | ControlHint::RampTerminal => {
                unreachable!("mapped to Priority above")
            }
        };
        out_nodes[ni].intersection = Some(ix_id);
        intersections.push(Intersection {
            node: ni as NodeId,
            control,
            incoming,
            allow_rtor: opt.allow_rtor,
            radius_m: node_radius[ni],
        });
    }

    Built {
        nodes: out_nodes,
        lanes,
        intersections,
        signals,
        road_lanes,
        surfaces,
    }
}

/// Movements per general (non-bus) approach lane, left to right.
fn lane_movements(
    general: &[LaneId],
    avail: &BTreeSet<TurnKind>,
    tags: &[TurnSet],
    total_lanes: usize,
) -> Vec<BTreeSet<TurnKind>> {
    let n = general.len();
    let pick = |ks: &[TurnKind]| -> BTreeSet<TurnKind> {
        ks.iter().copied().filter(|k| avail.contains(k)).collect()
    };
    let mut out: Vec<BTreeSet<TurnKind>> = Vec::with_capacity(n);
    for j in 0..n {
        let mut s = if !tags.is_empty() && tags.len() == total_lanes {
            let t = tags[j];
            let mut ks = Vec::new();
            if t.left {
                ks.push(TurnKind::Left);
            }
            if t.straight {
                ks.push(TurnKind::Straight);
            }
            if t.right {
                ks.push(TurnKind::Right);
            }
            pick(&ks)
        } else if n == 1 {
            avail.clone()
        } else if j == 0 {
            pick(&[TurnKind::Left, TurnKind::Straight])
        } else if j == n - 1 {
            pick(&[TurnKind::Right, TurnKind::Straight])
        } else {
            pick(&[TurnKind::Straight])
        };
        if avail.contains(&TurnKind::UTurn) {
            s.insert(TurnKind::UTurn);
        }
        if s.is_empty() {
            let pref: &[TurnKind] = if j * 2 < n {
                &[TurnKind::Left, TurnKind::Straight, TurnKind::Right]
            } else {
                &[TurnKind::Right, TurnKind::Straight, TurnKind::Left]
            };
            if let Some(k) = pref.iter().find(|k| avail.contains(k)) {
                s.insert(*k);
            }
        }
        out.push(s);
    }
    out
}

/// Fixed-time plan: opposite approaches share a phase; leftovers get their own phase.
fn signal_plan(ix: IntersectionId, approaches: &[(u8, V2, RoadClass, f32)]) -> SignalController {
    let n = approaches.len();
    let mut used = vec![false; n];
    let mut phases = Vec::new();
    for i in 0..n {
        if used[i] {
            continue;
        }
        used[i] = true;
        let mut best: Option<(usize, f32)> = None;
        for j in 0..n {
            if used[j] {
                continue;
            }
            let c = geom::dot(approaches[i].1, approaches[j].1);
            if c < -0.85 && best.is_none_or(|(_, bc)| c < bc) {
                best = Some((j, c));
            }
        }
        let mut members = vec![i];
        if let Some((j, _)) = best {
            used[j] = true;
            members.push(j);
        }
        let class = members
            .iter()
            .map(|m| approaches[*m].2)
            .min_by_key(|c| std::cmp::Reverse(c.rank()))
            .unwrap();
        let v = members
            .iter()
            .map(|m| approaches[*m].3)
            .fold(0.0f32, f32::max);
        phases.push(SignalPhase {
            green_groups: members.iter().map(|m| approaches[*m].0).collect(),
            green_s: class.green_s(),
            yellow_s: (1.0 + v / (2.0 * 3.05)).clamp(3.0, 5.0),
            all_red_s: if v > 20.0 { 2.0 } else { 1.5 },
        });
    }
    SignalController {
        intersection: ix,
        groups: n as u8,
        offset_s: 0.0,
        phases,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(x: f32, y: f32) -> NodeSpec {
        NodeSpec {
            pos: [x, y],
            z: 0.0,
            signal: false,
            control: None,
        }
    }

    /// A plus-shaped junction of two 2+2 arterials.
    fn cross() -> Built {
        let nodes = vec![
            node(0.0, 0.0),
            node(0.0, 200.0),
            node(200.0, 0.0),
            node(0.0, -200.0),
            node(-200.0, 0.0),
        ];
        let roads: Vec<RoadSpec> = (1..5)
            .map(|i| RoadSpec::new(0, i, RoadClass::Arterial, 2, 2))
            .collect();
        build(&nodes, &roads, &BuildOptions::default())
    }

    #[test]
    fn cross_junction_wiring() {
        let b = cross();
        assert_eq!(b.lanes.len(), 16);
        assert_eq!(b.intersections.len(), 1);
        let ix = &b.intersections[0];
        assert_eq!(ix.incoming.len(), 8);
        assert!(matches!(ix.control, IntersectionControl::Signal { .. }));
        let sc = &b.signals[0];
        assert_eq!(sc.phases.len(), 2);
        assert_eq!(sc.groups, 4);
        // Every incoming lane has at least one connection; left lanes turn left, right lanes right.
        for inc in &ix.incoming {
            let l = &b.lanes[inc.lane as usize];
            assert!(!l.connections.is_empty());
            if l.index_from_left == 0 {
                assert!(l.connections.iter().any(|c| c.turn == TurnKind::Left));
                assert!(!l.connections.iter().any(|c| c.turn == TurnKind::Right));
            } else {
                assert!(l.connections.iter().any(|c| c.turn == TurnKind::Right));
            }
        }
        // Lanes are trimmed back from the junction centre.
        for l in &b.lanes {
            let end = l.polyline[l.polyline.len() - 1];
            let start = l.polyline[0];
            let r_end = geom::len([end[0], end[1]]);
            let r_start = geom::len([start[0], start[1]]);
            assert!(r_end.min(r_start) > 7.0, "lane reaches into the box");
        }
    }

    #[test]
    fn right_hand_traffic_offsets() {
        let b = cross();
        // Road 0 goes from (0,0) north to (0,200): forward lanes are on the east (x > 0) side.
        let fwd = &b.road_lanes[0].fwd;
        for l in fwd {
            assert!(b.lanes[*l as usize].polyline[0][0] > 0.0);
        }
        for l in &b.road_lanes[0].bwd {
            assert!(b.lanes[*l as usize].polyline[0][0] < 0.0);
        }
    }

    #[test]
    fn dead_end_gets_u_turn_and_local_cross_is_all_way_stop() {
        let nodes = vec![
            node(0.0, 0.0),
            node(0.0, 100.0),
            node(100.0, 0.0),
            node(0.0, -100.0),
            node(-100.0, 0.0),
        ];
        let roads: Vec<RoadSpec> = (1..5)
            .map(|i| RoadSpec::new(0, i, RoadClass::Local, 1, 1))
            .collect();
        let b = build(&nodes, &roads, &BuildOptions::default());
        assert!(matches!(
            b.intersections[0].control,
            IntersectionControl::AllWayStop
        ));
        // Lanes arriving at the dead ends (nodes 1..4) U-turn.
        for l in &b.lanes {
            if l.to_node != 0 {
                assert_eq!(l.connections.len(), 1);
                assert_eq!(l.connections[0].turn, TurnKind::UTurn);
            }
        }
    }

    #[test]
    fn explicit_turn_lanes_respected() {
        let nodes = vec![
            node(0.0, 0.0),
            node(0.0, 200.0),
            node(200.0, 0.0),
            node(0.0, -200.0),
            node(-200.0, 0.0),
        ];
        let mut roads: Vec<RoadSpec> = (1..5)
            .map(|i| RoadSpec::new(0, i, RoadClass::Arterial, 2, 2))
            .collect();
        // Southbound approach (road 0 backward, arriving at node 0): left-only + straight/right.
        roads[0].turn_lanes_bwd = vec![
            TurnSet {
                left: true,
                ..Default::default()
            },
            TurnSet {
                straight: true,
                right: true,
                ..Default::default()
            },
        ];
        let b = build(&nodes, &roads, &BuildOptions::default());
        let l0 = &b.lanes[b.road_lanes[0].bwd[0] as usize];
        assert_eq!(l0.kind, LaneKind::TurnLeft);
        let l1 = &b.lanes[b.road_lanes[0].bwd[1] as usize];
        assert!(l1.connections.iter().all(|c| c.turn != TurnKind::Left));
    }
}
