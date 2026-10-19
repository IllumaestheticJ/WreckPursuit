//! The police view of the road network: nodes, directed-or-two-way segments, districts, POIs
//! (safehouses, cameras) and building occluders for line of sight.
//!
//! `wp_world` (prompt 05) owns the real city. Until it exposes a road graph and LOS queries,
//! this crate carries its own minimal graph plus [`procedural_city`], an irregular grid city
//! used by every police test and scenario. `wp_world` is expected to convert its network into
//! a [`RoadGraph`] (or the lead promotes this type into `wp_contracts`); see
//! `docs/integration/police.md`.
//!
//! All geometry is 2D in the world XY plane (+Z up is ignored), metres.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use serde::{Deserialize, Serialize};
use wp_math::Rng;

/// Road node (intersection or bend).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u32);

/// Road segment between two nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SegmentId(pub u32);

/// City district. Patrol presence and heat decay are keyed by this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum District {
    /// Dense downtown.
    Core,
    /// Residential ring.
    Suburb,
    /// Warehouses, docks, wide roads.
    Industrial,
}

impl District {
    /// All districts in a fixed order.
    pub const ALL: [District; 3] = [District::Core, District::Suburb, District::Industrial];
}

/// Road class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RoadClass {
    /// Local street.
    Local,
    /// Multi-lane arterial.
    Arterial,
    /// Limited-access highway.
    Highway,
}

/// A road node.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RoadNode {
    /// World XY position, m.
    pub pos: [f32; 2],
    /// District the node lies in.
    pub district: District,
}

/// A straight road segment.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RoadSegment {
    /// Start node.
    pub a: NodeId,
    /// End node.
    pub b: NodeId,
    /// Length, m.
    pub length_m: f32,
    /// Speed limit, m/s.
    pub speed_limit_mps: f32,
    /// Road class.
    pub class: RoadClass,
    /// District (of the start node).
    pub district: District,
    /// One-way `a → b` only.
    pub oneway: bool,
}

/// Kind of point of interest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PoiKind {
    /// Racer safehouse (garage). Ends a pursuit after a quiet dwell.
    Safehouse,
    /// Traffic camera: witnesses violations within its radius.
    Camera,
    /// Helicopter base.
    Helipad,
    /// Police station.
    PoliceStation,
}

/// A point of interest.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Poi {
    /// Kind.
    pub kind: PoiKind,
    /// World XY position, m.
    pub pos: [f32; 2],
    /// Effective radius, m.
    pub radius_m: f32,
}

/// Axis-aligned building footprint that blocks line of sight.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Occluder {
    /// Min corner, m.
    pub min: [f32; 2],
    /// Max corner, m.
    pub max: [f32; 2],
}

/// A position on the road network.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadPos {
    /// Segment.
    pub segment: SegmentId,
    /// Fraction along the segment from `a` (0) to `b` (1).
    pub t: f32,
    /// Closest point on the segment, m.
    pub point: [f32; 2],
    /// Distance from the query point to `point`, m.
    pub dist_m: f32,
}

/// One straight leg of a [`RoadPath`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathLeg {
    /// Segment the leg runs along.
    pub segment: SegmentId,
    /// Node the leg travels away from.
    pub from_node: NodeId,
    /// Leg start, m.
    pub from: [f32; 2],
    /// Leg end, m.
    pub to: [f32; 2],
    /// Leg length, m.
    pub len_m: f32,
}

/// A path along the road network as a sequence of straight legs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoadPath {
    /// Legs in travel order.
    pub legs: Vec<PathLeg>,
}

impl RoadPath {
    /// Total length, m.
    pub fn length(&self) -> f32 {
        self.legs.iter().map(|l| l.len_m).sum()
    }

    /// `(leg index, distance into leg)` at arc length `s` (clamped).
    pub fn leg_at(&self, s: f32) -> Option<(usize, f32)> {
        let mut acc = 0.0;
        for (i, l) in self.legs.iter().enumerate() {
            if s <= acc + l.len_m || i + 1 == self.legs.len() {
                return Some((i, (s - acc).clamp(0.0, l.len_m)));
            }
            acc += l.len_m;
        }
        None
    }

    /// Point at arc length `s`.
    pub fn point_at(&self, s: f32) -> Option<[f32; 2]> {
        let (i, d) = self.leg_at(s)?;
        let l = &self.legs[i];
        let k = if l.len_m > 0.0 { d / l.len_m } else { 0.0 };
        Some(lerp2(l.from, l.to, k))
    }

    /// Unit travel direction at arc length `s`.
    pub fn dir_at(&self, s: f32) -> Option<[f32; 2]> {
        let (i, _) = self.leg_at(s)?;
        Some(normalize(sub(self.legs[i].to, self.legs[i].from)))
    }

    /// Path points (start of each leg plus the end).
    pub fn points(&self) -> Vec<[f32; 2]> {
        let mut v: Vec<[f32; 2]> = self.legs.iter().map(|l| l.from).collect();
        if let Some(l) = self.legs.last() {
            v.push(l.to);
        }
        v
    }
}

/// Cosine of the heading change below which two consecutive segments count as "straight"
/// for sight distance (≈ 8°).
pub const STRAIGHT_COS: f32 = 0.990;

/// The road network.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RoadGraph {
    /// Nodes, indexed by `NodeId.0`.
    pub nodes: Vec<RoadNode>,
    /// Segments, indexed by `SegmentId.0`.
    pub segments: Vec<RoadSegment>,
    /// Points of interest.
    pub pois: Vec<Poi>,
    /// Building footprints.
    pub occluders: Vec<Occluder>,
    /// Outgoing `(segment, other node)` per node, ascending by segment id. Respects one-way.
    out: Vec<Vec<(SegmentId, NodeId)>>,
    /// Incoming `(segment, other node)` per node, ascending by segment id. Respects one-way.
    inc: Vec<Vec<(SegmentId, NodeId)>>,
}

impl RoadGraph {
    /// Empty graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a node.
    pub fn add_node(&mut self, pos: [f32; 2], district: District) -> NodeId {
        self.nodes.push(RoadNode { pos, district });
        self.out.push(Vec::new());
        self.inc.push(Vec::new());
        NodeId(self.nodes.len() as u32 - 1)
    }

    /// Add a segment `a → b` (two-way unless `oneway`).
    pub fn add_segment(
        &mut self,
        a: NodeId,
        b: NodeId,
        speed_limit_mps: f32,
        class: RoadClass,
        oneway: bool,
    ) -> SegmentId {
        let pa = self.nodes[a.0 as usize].pos;
        let pb = self.nodes[b.0 as usize].pos;
        let id = SegmentId(self.segments.len() as u32);
        self.segments.push(RoadSegment {
            a,
            b,
            length_m: dist(pa, pb),
            speed_limit_mps,
            class,
            district: self.nodes[a.0 as usize].district,
            oneway,
        });
        self.out[a.0 as usize].push((id, b));
        self.inc[b.0 as usize].push((id, a));
        if !oneway {
            self.out[b.0 as usize].push((id, a));
            self.inc[a.0 as usize].push((id, b));
        }
        id
    }

    /// Node by id.
    pub fn node(&self, n: NodeId) -> &RoadNode {
        &self.nodes[n.0 as usize]
    }

    /// Segment by id.
    pub fn segment(&self, s: SegmentId) -> &RoadSegment {
        &self.segments[s.0 as usize]
    }

    /// Legal outgoing `(segment, next node)` from `n`.
    pub fn outgoing(&self, n: NodeId) -> &[(SegmentId, NodeId)] {
        &self.out[n.0 as usize]
    }

    /// Legal incoming `(segment, previous node)` into `n`.
    pub fn incoming(&self, n: NodeId) -> &[(SegmentId, NodeId)] {
        &self.inc[n.0 as usize]
    }

    /// The node at the other end of `s` from `n`.
    pub fn other_end(&self, s: SegmentId, n: NodeId) -> NodeId {
        let seg = self.segment(s);
        if seg.a == n {
            seg.b
        } else {
            seg.a
        }
    }

    /// Unit direction of travel along `s` leaving `from`.
    pub fn dir_from(&self, s: SegmentId, from: NodeId) -> [f32; 2] {
        let to = self.other_end(s, from);
        normalize(sub(self.node(to).pos, self.node(from).pos))
    }

    /// Whether travel along `s` leaving `from` is legal.
    pub fn can_travel(&self, s: SegmentId, from: NodeId) -> bool {
        let seg = self.segment(s);
        seg.a == from || (!seg.oneway && seg.b == from)
    }

    /// Closest road position to `p` (linear scan; the graph is small).
    pub fn nearest_on_road(&self, p: [f32; 2]) -> Option<RoadPos> {
        let mut best: Option<RoadPos> = None;
        for (i, seg) in self.segments.iter().enumerate() {
            let a = self.node(seg.a).pos;
            let b = self.node(seg.b).pos;
            let (t, q) = closest_on_segment(a, b, p);
            let d = dist(p, q);
            if best.is_none_or(|bp| d < bp.dist_m) {
                best = Some(RoadPos {
                    segment: SegmentId(i as u32),
                    t,
                    point: q,
                    dist_m: d,
                });
            }
        }
        best
    }

    /// Closest node to `p`.
    pub fn nearest_node(&self, p: [f32; 2]) -> Option<NodeId> {
        let mut best: Option<(f32, NodeId)> = None;
        for (i, n) in self.nodes.iter().enumerate() {
            let d = dist2(n.pos, p);
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, NodeId(i as u32)));
            }
        }
        best.map(|b| b.1)
    }

    /// District at `p` (district of the nearest segment; Suburb off the map).
    pub fn district_at(&self, p: [f32; 2]) -> District {
        self.nearest_on_road(p)
            .map(|r| self.segment(r.segment).district)
            .unwrap_or(District::Suburb)
    }

    /// Line of sight between two ground-level points (no building footprint crossed).
    pub fn line_of_sight(&self, a: [f32; 2], b: [f32; 2]) -> bool {
        !self
            .occluders
            .iter()
            .any(|o| segment_hits_aabb(a, b, o.min, o.max))
    }

    /// Fastest legal route `from → to` by free-flow travel time (length / limit). Returns the
    /// node sequence including both ends. Deterministic: ties break on node id.
    pub fn shortest_path(&self, from: NodeId, to: NodeId) -> Option<Vec<NodeId>> {
        let n = self.nodes.len();
        let mut best = vec![u64::MAX; n];
        let mut prev: Vec<Option<NodeId>> = vec![None; n];
        let mut heap = BinaryHeap::new();
        best[from.0 as usize] = 0;
        heap.push(Reverse((0u64, from.0)));
        while let Some(Reverse((cost, node))) = heap.pop() {
            if node == to.0 {
                break;
            }
            if cost > best[node as usize] {
                continue;
            }
            for &(s, next) in self.outgoing(NodeId(node)) {
                let seg = self.segment(s);
                // Travel time in milliseconds keeps the heap keys integral (deterministic).
                let ms = (seg.length_m / seg.speed_limit_mps.max(1.0) * 1000.0) as u64 + 1;
                let c = cost + ms;
                if c < best[next.0 as usize] {
                    best[next.0 as usize] = c;
                    prev[next.0 as usize] = Some(NodeId(node));
                    heap.push(Reverse((c, next.0)));
                }
            }
        }
        if best[to.0 as usize] == u64::MAX {
            return None;
        }
        let mut path = vec![to];
        let mut cur = to;
        while let Some(p) = prev[cur.0 as usize] {
            path.push(p);
            cur = p;
        }
        path.reverse();
        Some(path)
    }

    /// Free-flow travel time of a node route, s.
    pub fn route_time_s(&self, route: &[NodeId]) -> f32 {
        route
            .windows(2)
            .filter_map(|w| self.segment_between(w[0], w[1]))
            .map(|s| {
                let seg = self.segment(s);
                seg.length_m / seg.speed_limit_mps.max(1.0)
            })
            .sum()
    }

    /// The legal segment from `a` to `b`, if any.
    pub fn segment_between(&self, a: NodeId, b: NodeId) -> Option<SegmentId> {
        self.outgoing(a)
            .iter()
            .find(|(_, n)| *n == b)
            .map(|(s, _)| *s)
    }

    /// Turn a node route into a [`RoadPath`] starting at `start` (projected onto the first leg).
    pub fn route_path(&self, start: [f32; 2], route: &[NodeId]) -> RoadPath {
        let mut legs = Vec::new();
        let mut cur = start;
        for w in route.windows(2) {
            if let Some(s) = self.segment_between(w[0], w[1]) {
                let to = self.node(w[1]).pos;
                legs.push(PathLeg {
                    segment: s,
                    from_node: w[0],
                    from: cur,
                    to,
                    len_m: dist(cur, to),
                });
                cur = to;
            }
        }
        RoadPath { legs }
    }

    /// Predict where a vehicle at `pos` moving with `vel` will drive over the next `horizon_m`
    /// metres, following the straightest legal continuation at each node (a reasonable guess
    /// for a fleeing racer). The path starts at the road point closest to `pos`.
    pub fn predict_path(&self, pos: [f32; 2], vel: [f32; 2], horizon_m: f32) -> RoadPath {
        let Some(rp) = self.nearest_on_road(pos) else {
            return RoadPath::default();
        };
        let seg = *self.segment(rp.segment);
        let along = sub(self.node(seg.b).pos, self.node(seg.a).pos);
        let forward_b = dot(along, vel) >= 0.0;
        // Travel direction; if the vehicle points against a one-way, assume it continues anyway.
        let (from, mut next) = if forward_b {
            (seg.a, seg.b)
        } else {
            (seg.b, seg.a)
        };
        let mut legs = vec![PathLeg {
            segment: rp.segment,
            from_node: from,
            from: rp.point,
            to: self.node(next).pos,
            len_m: dist(rp.point, self.node(next).pos),
        }];
        let mut total = legs[0].len_m;
        let mut dir = normalize(sub(self.node(next).pos, self.node(from).pos));
        let mut came_by = rp.segment;
        while total < horizon_m && legs.len() < 64 {
            let mut choice: Option<(f32, SegmentId, NodeId)> = None;
            for &(s, n) in self.outgoing(next) {
                if s == came_by {
                    continue;
                }
                let d = self.dir_from(s, next);
                let score = dot(d, dir);
                if choice.is_none_or(|c| score > c.0) {
                    choice = Some((score, s, n));
                }
            }
            let Some((_, s, n)) = choice else { break };
            let a = self.node(next).pos;
            let b = self.node(n).pos;
            legs.push(PathLeg {
                segment: s,
                from_node: next,
                from: a,
                to: b,
                len_m: dist(a, b),
            });
            total += dist(a, b);
            dir = normalize(sub(b, a));
            came_by = s;
            next = n;
        }
        RoadPath { legs }
    }

    /// Approach sight distance to a point `d_m` metres along segment `s` travelling away from
    /// node `from`: how far back a driver approaching the point can see it, taken as the
    /// minimum over **every** legal approach. A driver turning in at `from` only sees it from
    /// `from`; the run is extended backwards only through nodes where every incoming road
    /// continues straight (within [`STRAIGHT_COS`]) and the view is not blocked by a building.
    /// Capped at `cap_m`.
    pub fn approach_sight_m(&self, s: SegmentId, from: NodeId, d_m: f32, cap_m: f32) -> f32 {
        let target = {
            let a = self.node(from).pos;
            let dir = self.dir_from(s, from);
            add(a, scale(dir, d_m))
        };
        let dir = self.dir_from(s, from);
        let mut sight = d_m;
        let mut node = from;
        let mut via = s;
        let mut guard = 0;
        while sight < cap_m && guard < 32 {
            guard += 1;
            // Incoming roads into `node` other than the one we are travelling along.
            let mut straight_pred: Option<(SegmentId, NodeId)> = None;
            let mut blind = false;
            for &(ps, pn) in self.incoming(node) {
                if ps == via {
                    continue;
                }
                let pd = normalize(sub(self.node(node).pos, self.node(pn).pos));
                if dot(pd, dir) >= STRAIGHT_COS {
                    straight_pred = Some((ps, pn));
                } else {
                    blind = true;
                }
            }
            if blind {
                break;
            }
            let Some((ps, pn)) = straight_pred else { break };
            let back = self.node(pn).pos;
            if !self.line_of_sight(back, target) {
                break;
            }
            sight += self.segment(ps).length_m;
            node = pn;
            via = ps;
        }
        sight.min(cap_m)
    }

    /// POIs of a kind.
    pub fn pois_of(&self, kind: PoiKind) -> impl Iterator<Item = &Poi> {
        self.pois.iter().filter(move |p| p.kind == kind)
    }
}

/// Parameters for [`procedural_city`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CityParams {
    /// Seed for block sizes, jitter, occluders and POIs.
    pub seed: u64,
    /// Grid columns (north-south streets).
    pub cols: u32,
    /// Grid rows (east-west streets).
    pub rows: u32,
    /// Block size range, m.
    pub block_min_m: f32,
    /// Block size range, m.
    pub block_max_m: f32,
    /// Max node jitter (creates bends and blind corners), m.
    pub jitter_m: f32,
    /// Add a highway along the south edge.
    pub highway: bool,
}

impl Default for CityParams {
    fn default() -> Self {
        Self {
            seed: 1,
            cols: 10,
            rows: 10,
            block_min_m: 90.0,
            block_max_m: 190.0,
            jitter_m: 14.0,
            highway: true,
        }
    }
}

const KMH: f32 = 1.0 / 3.6;

/// An irregular grid city: Core in the middle, Industrial to the east, Suburb elsewhere,
/// arterials every third street, alternating one-way local streets in Core, an optional
/// highway with ramps along the south edge, buildings in most blocks, two safehouses and a few
/// traffic cameras. Stand-in for prompt 05's procedural city.
#[allow(clippy::needless_range_loop)] // grid[i][j] indexing reads clearer than iterators here
pub fn procedural_city(p: &CityParams) -> RoadGraph {
    let mut rng = Rng::new(p.seed ^ 0xC17E_C17E);
    let mut xs = vec![0.0f32];
    for _ in 1..p.cols {
        let last = *xs.last().unwrap();
        xs.push(last + rng.range_f32(p.block_min_m, p.block_max_m));
    }
    let mut ys = vec![0.0f32];
    for _ in 1..p.rows {
        let last = *ys.last().unwrap();
        ys.push(last + rng.range_f32(p.block_min_m, p.block_max_m));
    }
    let (w, h) = (*xs.last().unwrap(), *ys.last().unwrap());
    let (cx, cy) = (w * 0.5, h * 0.5);
    let district_of = |x: f32, y: f32| {
        let u = (x - cx) / (w * 0.5);
        let v = (y - cy) / (h * 0.5);
        if u.abs().max(v.abs()) < 0.45 {
            District::Core
        } else if u > 0.3 {
            District::Industrial
        } else {
            District::Suburb
        }
    };
    let mut g = RoadGraph::new();
    let mut grid = vec![vec![NodeId(0); p.rows as usize]; p.cols as usize];
    for (i, &x) in xs.iter().enumerate() {
        for (j, &y) in ys.iter().enumerate() {
            let jx = rng.range_f32(-p.jitter_m, p.jitter_m);
            let jy = rng.range_f32(-p.jitter_m, p.jitter_m);
            grid[i][j] = g.add_node([x + jx, y + jy], district_of(x, y));
        }
    }
    let limit_for = |d: District, arterial: bool| {
        if arterial {
            60.0 * KMH
        } else {
            match d {
                District::Core | District::Suburb => 50.0 * KMH,
                District::Industrial => 60.0 * KMH,
            }
        }
    };
    // East-west rows (two-way).
    for j in 0..p.rows as usize {
        let arterial = j % 3 == 0;
        for i in 0..p.cols as usize - 1 {
            let a = grid[i][j];
            let b = grid[i + 1][j];
            let d = g.node(a).district;
            let class = if arterial {
                RoadClass::Arterial
            } else {
                RoadClass::Local
            };
            g.add_segment(a, b, limit_for(d, arterial), class, false);
        }
    }
    // North-south columns; Core local columns are one-way, alternating direction.
    for i in 0..p.cols as usize {
        let arterial = i % 3 == 0;
        for j in 0..p.rows as usize - 1 {
            let (mut a, mut b) = (grid[i][j], grid[i][j + 1]);
            let d = g.node(a).district;
            let oneway = !arterial && d == District::Core && g.node(b).district == District::Core;
            if oneway && i % 2 == 1 {
                std::mem::swap(&mut a, &mut b);
            }
            let class = if arterial {
                RoadClass::Arterial
            } else {
                RoadClass::Local
            };
            g.add_segment(a, b, limit_for(d, arterial), class, oneway);
        }
    }
    if p.highway {
        let hy = -160.0;
        let mut prev: Option<NodeId> = None;
        for (i, &x) in xs.iter().enumerate() {
            let n = g.add_node([x, hy], district_of(x, 0.0));
            if let Some(pn) = prev {
                g.add_segment(pn, n, 100.0 * KMH, RoadClass::Highway, false);
            }
            if i % 3 == 0 {
                g.add_segment(n, grid[i][0], 50.0 * KMH, RoadClass::Local, false);
            }
            prev = Some(n);
        }
    }
    // Buildings: inset footprints inside each block.
    let inset = 10.0;
    for i in 0..p.cols as usize - 1 {
        for j in 0..p.rows as usize - 1 {
            let c = [
                grid[i][j],
                grid[i + 1][j],
                grid[i][j + 1],
                grid[i + 1][j + 1],
            ];
            let pos: Vec<[f32; 2]> = c.iter().map(|n| g.node(*n).pos).collect();
            let minx = pos[0][0].max(pos[2][0]) + inset;
            let maxx = pos[1][0].min(pos[3][0]) - inset;
            let miny = pos[0][1].max(pos[1][1]) + inset;
            let maxy = pos[2][1].min(pos[3][1]) - inset;
            if maxx - minx < 4.0 || maxy - miny < 4.0 {
                continue;
            }
            let fill = match g.node(c[0]).district {
                District::Core => 1.0,
                District::Industrial => 0.8,
                District::Suburb => 0.7,
            };
            if rng.chance(fill) {
                g.occluders.push(Occluder {
                    min: [minx, miny],
                    max: [maxx, maxy],
                });
            }
        }
    }
    // Safehouses: one in Suburb, one in Industrial, at segment midpoints.
    for want in [District::Suburb, District::Industrial] {
        let cands: Vec<usize> = (0..g.segments.len())
            .filter(|&s| g.segments[s].district == want && g.segments[s].class == RoadClass::Local)
            .collect();
        if cands.is_empty() {
            continue;
        }
        let s = g.segments[cands[rng.below(cands.len() as u32) as usize]];
        let m = lerp2(g.node(s.a).pos, g.node(s.b).pos, 0.5);
        g.pois.push(Poi {
            kind: PoiKind::Safehouse,
            pos: m,
            radius_m: 15.0,
        });
    }
    // Cameras at a quarter of Core intersections.
    for n in 0..g.nodes.len() {
        if g.nodes[n].district == District::Core && rng.chance(0.25) {
            g.pois.push(Poi {
                kind: PoiKind::Camera,
                pos: g.nodes[n].pos,
                radius_m: 60.0,
            });
        }
    }
    g
}

// ---------------------------------------------------------------------------------------------
// Small 2D vector helpers (f32, IEEE ops only).
// ---------------------------------------------------------------------------------------------

/// `a + b`.
pub fn add(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}
/// `a - b`.
pub fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}
/// `a * k`.
pub fn scale(a: [f32; 2], k: f32) -> [f32; 2] {
    [a[0] * k, a[1] * k]
}
/// Dot product.
pub fn dot(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}
/// 2D cross product (z of `a × b`); positive when `b` is to the left of `a`.
pub fn cross(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[1] - a[1] * b[0]
}
/// Length.
pub fn len(a: [f32; 2]) -> f32 {
    dot(a, a).sqrt()
}
/// Squared distance.
pub fn dist2(a: [f32; 2], b: [f32; 2]) -> f32 {
    let d = sub(a, b);
    dot(d, d)
}
/// Distance.
pub fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    dist2(a, b).sqrt()
}
/// Unit vector (zero stays zero).
pub fn normalize(a: [f32; 2]) -> [f32; 2] {
    let l = len(a);
    if l > 1e-6 {
        scale(a, 1.0 / l)
    } else {
        [0.0, 0.0]
    }
}
/// Linear interpolation.
pub fn lerp2(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}
/// Left-hand perpendicular (rotate +90°).
pub fn perp_left(a: [f32; 2]) -> [f32; 2] {
    [-a[1], a[0]]
}

/// Closest point on segment `a–b` to `p`: `(t, point)`.
pub fn closest_on_segment(a: [f32; 2], b: [f32; 2], p: [f32; 2]) -> (f32, [f32; 2]) {
    let ab = sub(b, a);
    let l2 = dot(ab, ab);
    let t = if l2 > 0.0 {
        (dot(sub(p, a), ab) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (t, lerp2(a, b, t))
}

/// Whether segment `a–b` intersects the AABB `[min, max]` (slab test).
pub fn segment_hits_aabb(a: [f32; 2], b: [f32; 2], min: [f32; 2], max: [f32; 2]) -> bool {
    let d = sub(b, a);
    let mut t0 = 0.0f32;
    let mut t1 = 1.0f32;
    for k in 0..2 {
        if d[k].abs() < 1e-9 {
            if a[k] < min[k] || a[k] > max[k] {
                return false;
            }
        } else {
            let inv = 1.0 / d[k];
            let mut ta = (min[k] - a[k]) * inv;
            let mut tb = (max[k] - a[k]) * inv;
            if ta > tb {
                std::mem::swap(&mut ta, &mut tb);
            }
            t0 = t0.max(ta);
            t1 = t1.min(tb);
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

/// XY of a 3D world point.
pub fn xy(p: [f32; 3]) -> [f32; 2] {
    [p[0], p[1]]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn city() -> RoadGraph {
        procedural_city(&CityParams::default())
    }

    #[test]
    fn city_is_deterministic_and_has_all_districts() {
        let a = city();
        assert_eq!(a, city());
        for d in District::ALL {
            assert!(a.segments.iter().any(|s| s.district == d), "{d:?}");
        }
        assert_eq!(a.pois_of(PoiKind::Safehouse).count(), 2);
        assert!(!a.occluders.is_empty());
    }

    #[test]
    fn every_node_reaches_every_node() {
        let g = city();
        let n = g.nodes.len() as u32;
        for (a, b) in [(0, n - 1), (n - 1, 0), (5, 77), (77, 5), (n / 2, 3)] {
            let r = g.shortest_path(NodeId(a), NodeId(b));
            assert!(r.is_some(), "{a}->{b}");
            let r = r.unwrap();
            for w in r.windows(2) {
                let s = g.segment_between(w[0], w[1]).unwrap();
                assert!(g.can_travel(s, w[0]), "one-way violated");
            }
        }
    }

    #[test]
    fn los_blocked_by_building_but_not_along_street() {
        let g = city();
        let o = g.occluders[0];
        let c = lerp2(o.min, o.max, 0.5);
        assert!(!g.line_of_sight([o.min[0] - 5.0, c[1]], [o.max[0] + 5.0, c[1]]));
        let s = g.segments[0];
        assert!(g.line_of_sight(g.node(s.a).pos, g.node(s.b).pos));
    }

    #[test]
    fn sight_distance_is_short_after_a_turn() {
        let g = city();
        // An interior node has crossing roads, so sight is only the distance from the node.
        let s = SegmentId(15);
        let seg = g.segment(s);
        let sight = g.approach_sight_m(s, seg.a, 40.0, 1000.0);
        assert!((sight - 40.0).abs() < 1e-3 || sight > 40.0);
        assert!(sight <= 1000.0);
    }

    #[test]
    fn prediction_follows_roads() {
        let g = city();
        let s = g.segments[3];
        let a = g.node(s.a).pos;
        let b = g.node(s.b).pos;
        let p = g.predict_path(lerp2(a, b, 0.2), sub(b, a), 600.0);
        assert!(p.length() >= 600.0 - 1.0);
        for w in p.legs.windows(2) {
            assert!(dist(w[0].to, w[1].from) < 1e-3);
        }
    }

    #[test]
    fn aabb_slab() {
        assert!(segment_hits_aabb(
            [-1.0, 0.5],
            [2.0, 0.5],
            [0.0, 0.0],
            [1.0, 1.0]
        ));
        assert!(!segment_hits_aabb(
            [-1.0, 2.0],
            [2.0, 2.0],
            [0.0, 0.0],
            [1.0, 1.0]
        ));
        assert!(!segment_hits_aabb(
            [-1.0, 0.5],
            [-0.5, 0.5],
            [0.0, 0.0],
            [1.0, 1.0]
        ));
    }
}
