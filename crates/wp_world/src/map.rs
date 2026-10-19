//! Runtime city map built from a [`RoadNetwork`]: driveable segments (lanes plus intersection
//! connector curves), connector conflict sets, a uniform-grid spatial index, a zone lookup grid
//! and the [`WorldQuery`] implementation.

use wp_contracts::{SignalState, SurfaceType, TimeOfDay, WorldQuery};

use crate::geom::{self, V2, V3};
use crate::network::*;
use crate::signals;

/// Index into [`CityMap::segs`]. Lanes come first (`SegId == LaneId`), connectors after.
pub type SegId = u32;
/// "No segment" sentinel.
pub const NO_SEG: SegId = u32::MAX;

/// Spatial grid cell size, m.
const CELL_M: f32 = 32.0;

/// What a segment is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SegKind {
    /// A lane of the network.
    Lane {
        /// Lane id.
        lane: LaneId,
    },
    /// A curve through a junction (or bend) joining two lanes.
    Connector {
        /// Lane it starts from.
        from: LaneId,
        /// Lane it ends in.
        to: LaneId,
        /// Manoeuvre.
        turn: TurnKind,
        /// Node it crosses.
        node: NodeId,
    },
}

/// A drivable path piece with arc-length parametrisation.
#[derive(Debug, Clone)]
pub struct Segment {
    /// Lane or connector.
    pub kind: SegKind,
    /// Polyline, m.
    pub pts: Vec<V3>,
    /// Cumulative arc length per point.
    pub cum: Vec<f32>,
    /// Total length, m.
    pub length: f32,
    /// Speed limit, m/s.
    pub speed_limit: f32,
    /// Width, m.
    pub width: f32,
    /// Civilians may drive here.
    pub drivable: bool,
    /// Successor segments (lane → its connectors; connector → its target lane).
    pub next: Vec<SegId>,
    /// Predecessor segments.
    pub prev: Vec<SegId>,
    /// Comfortable maximum speed (curvature limited), m/s.
    pub curve_speed: f32,
    /// Connectors at the same node whose swept paths conflict with this one.
    pub conflicts: Vec<SegId>,
    /// Connectors leaving the same lane (diverging), excluding this one, each with the arc
    /// length along **that sibling** after which a car on it no longer overlaps this path.
    pub siblings: Vec<(SegId, f32)>,
}

impl Segment {
    /// Point and unit tangent at arc length `s`.
    pub fn point_at(&self, s: f32) -> (V3, V2) {
        geom::point_at(&self.pts, &self.cum, s)
    }

    /// Project a point onto this segment.
    pub fn project(&self, p: V2) -> geom::Projection {
        geom::project(&self.pts, &self.cum, p)
    }

    /// Lane id if this is a lane.
    pub fn lane(&self) -> Option<LaneId> {
        match self.kind {
            SegKind::Lane { lane } => Some(lane),
            SegKind::Connector { .. } => None,
        }
    }

    /// Whether this is a junction connector.
    pub fn is_connector(&self) -> bool {
        matches!(self.kind, SegKind::Connector { .. })
    }
}

/// How a lane arrives at its end node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Approach {
    /// Intersection at the end of the lane.
    pub intersection: IntersectionId,
    /// Signal group.
    pub group: u8,
    /// Has priority (for priority junctions).
    pub major: bool,
}

/// A segment found near a point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Located {
    /// Segment.
    pub seg: SegId,
    /// Arc length along the segment.
    pub s: f32,
    /// Signed lateral offset (+ right), m.
    pub lateral: f32,
    /// Unit tangent at `s`.
    pub tangent: V2,
}

#[derive(Debug, Clone)]
struct Grid {
    origin: V2,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<u32>>,
}

impl Grid {
    fn new(lo: V2, hi: V2) -> Self {
        let origin = [lo[0] - 64.0, lo[1] - 64.0];
        let nx = (((hi[0] - origin[0]) + 64.0) / CELL_M).ceil().max(1.0) as usize;
        let ny = (((hi[1] - origin[1]) + 64.0) / CELL_M).ceil().max(1.0) as usize;
        Self {
            origin,
            nx,
            ny,
            cells: vec![Vec::new(); nx * ny],
        }
    }

    fn cell_of(&self, p: V2) -> Option<usize> {
        let cx = ((p[0] - self.origin[0]) / CELL_M).floor();
        let cy = ((p[1] - self.origin[1]) / CELL_M).floor();
        if cx < 0.0 || cy < 0.0 || cx >= self.nx as f32 || cy >= self.ny as f32 {
            return None;
        }
        Some(cy as usize * self.nx + cx as usize)
    }

    fn insert_box(&mut self, lo: V2, hi: V2, id: u32) {
        let x0 = (((lo[0] - self.origin[0]) / CELL_M).floor().max(0.0) as usize).min(self.nx - 1);
        let y0 = (((lo[1] - self.origin[1]) / CELL_M).floor().max(0.0) as usize).min(self.ny - 1);
        let x1 = (((hi[0] - self.origin[0]) / CELL_M).floor().max(0.0) as usize).min(self.nx - 1);
        let y1 = (((hi[1] - self.origin[1]) / CELL_M).floor().max(0.0) as usize).min(self.ny - 1);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let c = &mut self.cells[y * self.nx + x];
                if c.last() != Some(&id) {
                    c.push(id);
                }
            }
        }
    }

    fn at(&self, p: V2) -> &[u32] {
        match self.cell_of(p) {
            Some(c) => &self.cells[c],
            None => &[],
        }
    }
}

/// Runtime city map.
#[derive(Debug, Clone)]
pub struct CityMap {
    /// Source network.
    pub net: RoadNetwork,
    /// Lanes (indices `0..lanes.len()`) followed by connectors.
    pub segs: Vec<Segment>,
    /// Arrival info per lane (None = lane does not end at a junction).
    pub approach: Vec<Option<Approach>>,
    /// Drivable lane length per zone, km.
    pub zone_lane_km: Vec<f32>,
    /// Physics ticks (`PHYSICS_DT`) the signal controllers have run.
    pub signal_tick: u64,
    /// Current time of day (mirrors the world clock).
    pub time: TimeOfDay,
    seg_grid: Grid,
    ix_grid: Grid,
    zone_grid: Grid,
}

impl CityMap {
    /// Build runtime data from a network.
    pub fn new(net: RoadNetwork) -> Self {
        let nl = net.lanes.len();
        let mut segs: Vec<Segment> = Vec::with_capacity(nl * 3);
        for (i, l) in net.lanes.iter().enumerate() {
            let cum = geom::cumulative(&l.polyline);
            segs.push(Segment {
                kind: SegKind::Lane { lane: i as LaneId },
                length: *cum.last().unwrap(),
                cum,
                pts: l.polyline.clone(),
                speed_limit: l.speed_limit_mps,
                width: l.width_m,
                drivable: l.kind.drivable(),
                next: Vec::new(),
                prev: Vec::new(),
                curve_speed: l.speed_limit_mps,
                conflicts: Vec::new(),
                siblings: Vec::new(),
            });
        }
        // Connectors.
        for (i, l) in net.lanes.iter().enumerate() {
            for c in &l.connections {
                let to = &net.lanes[c.to as usize];
                let pts = connector_curve(&l.polyline, &to.polyline, c.turn);
                let cum = geom::cumulative(&pts);
                let length = *cum.last().unwrap();
                let limit = l.speed_limit_mps.min(to.speed_limit_mps);
                let id = segs.len() as SegId;
                segs.push(Segment {
                    kind: SegKind::Connector {
                        from: i as LaneId,
                        to: c.to,
                        turn: c.turn,
                        node: l.to_node,
                    },
                    curve_speed: curve_speed(&pts, length, limit),
                    pts,
                    cum,
                    length,
                    speed_limit: limit,
                    width: l.width_m.min(to.width_m),
                    drivable: l.kind.drivable() && to.kind.drivable(),
                    next: vec![c.to],
                    prev: vec![i as SegId],
                    conflicts: Vec::new(),
                    siblings: Vec::new(),
                });
                segs[i].next.push(id);
                segs[c.to as usize].prev.push(id);
            }
        }
        // Conflicts and siblings per node.
        let mut by_node: Vec<Vec<SegId>> = vec![Vec::new(); net.nodes.len()];
        for (i, s) in segs.iter().enumerate() {
            if let SegKind::Connector { node, .. } = s.kind {
                by_node[node as usize].push(i as SegId);
            }
        }
        // Bounding boxes reject most pairs before the polyline distance test.
        let bbox: Vec<(V2, V2)> = segs
            .iter()
            .map(|s| {
                let mut lo = [f32::INFINITY; 2];
                let mut hi = [f32::NEG_INFINITY; 2];
                for p in &s.pts {
                    lo = [lo[0].min(p[0]), lo[1].min(p[1])];
                    hi = [hi[0].max(p[0]), hi[1].max(p[1])];
                }
                (lo, hi)
            })
            .collect();
        let near = |a: u32, b: u32, m: f32| {
            let (la, ha) = bbox[a as usize];
            let (lb, hb) = bbox[b as usize];
            la[0] - m <= hb[0] && lb[0] - m <= ha[0] && la[1] - m <= hb[1] && lb[1] - m <= ha[1]
        };
        for list in &by_node {
            for (a_i, &a) in list.iter().enumerate() {
                for &b in &list[a_i + 1..] {
                    let (fa, ta) = conn_ends(&segs[a as usize]);
                    let (fb, tb) = conn_ends(&segs[b as usize]);
                    if fa == fb {
                        let div_b = divergence(&segs[b as usize], &segs[a as usize]);
                        let div_a = divergence(&segs[a as usize], &segs[b as usize]);
                        segs[a as usize].siblings.push((b, div_b));
                        segs[b as usize].siblings.push((a, div_a));
                        continue;
                    }
                    let conflict = ta == tb
                        || (near(a, b, 2.4)
                            && geom::polylines_within(
                                &segs[a as usize].pts,
                                &segs[b as usize].pts,
                                2.4,
                            ));
                    if conflict {
                        segs[a as usize].conflicts.push(b);
                        segs[b as usize].conflicts.push(a);
                    }
                }
            }
        }
        // Approaches.
        let mut approach = vec![None; nl];
        for (ix_id, ix) in net.intersections.iter().enumerate() {
            for inc in &ix.incoming {
                approach[inc.lane as usize] = Some(Approach {
                    intersection: ix_id as IntersectionId,
                    group: inc.signal_group,
                    major: inc.major,
                });
            }
        }
        // Grids.
        let lo = net.bounds_min;
        let hi = net.bounds_max;
        let mut seg_grid = Grid::new(lo, hi);
        for (i, s) in segs.iter().enumerate() {
            let pad = s.width * 0.5 + 3.0;
            for w in s.pts.windows(2) {
                let a = geom::xy(w[0]);
                let b = geom::xy(w[1]);
                seg_grid.insert_box(
                    [a[0].min(b[0]) - pad, a[1].min(b[1]) - pad],
                    [a[0].max(b[0]) + pad, a[1].max(b[1]) + pad],
                    i as u32,
                );
            }
        }
        let mut ix_grid = Grid::new(lo, hi);
        for (i, ix) in net.intersections.iter().enumerate() {
            let c = geom::xy(net.nodes[ix.node as usize].pos);
            let r = ix.radius_m;
            ix_grid.insert_box([c[0] - r, c[1] - r], [c[0] + r, c[1] + r], i as u32);
        }
        let mut zone_grid = Grid::new(lo, hi);
        for (i, z) in net.zones.iter().enumerate() {
            let mut zlo = [f32::INFINITY; 2];
            let mut zhi = [f32::NEG_INFINITY; 2];
            for p in &z.polygon {
                zlo = [zlo[0].min(p[0]), zlo[1].min(p[1])];
                zhi = [zhi[0].max(p[0]), zhi[1].max(p[1])];
            }
            zone_grid.insert_box(zlo, zhi, i as u32);
        }
        let mut map = Self {
            zone_lane_km: vec![0.0; net.zones.len()],
            net,
            segs,
            approach,
            signal_tick: 0,
            time: TimeOfDay::from_hours(12.0),
            seg_grid,
            ix_grid,
            zone_grid,
        };
        // Drivable lane-km per zone (sampled every 10 m).
        for i in 0..nl {
            let s = &map.segs[i];
            if !s.drivable {
                continue;
            }
            let n = (s.length / 10.0).ceil().max(1.0) as usize;
            let step = s.length / n as f32;
            for k in 0..n {
                let (p, _) = s.point_at((k as f32 + 0.5) * step);
                if let Some(z) = map.zone_at(geom::xy(p)) {
                    map.zone_lane_km[z as usize] += step / 1000.0;
                }
            }
        }
        map
    }

    /// Number of lanes (lane segments are `0..n_lanes()`).
    pub fn n_lanes(&self) -> usize {
        self.net.lanes.len()
    }

    /// Segment by id.
    pub fn seg(&self, id: SegId) -> &Segment {
        &self.segs[id as usize]
    }

    /// Zone containing `p`, if any (first match in zone order).
    pub fn zone_at(&self, p: V2) -> Option<ZoneId> {
        self.zone_grid
            .at(p)
            .iter()
            .copied()
            .find(|z| geom::point_in_polygon(p, &self.net.zones[*z as usize].polygon))
    }

    /// Intersection whose box contains `p`.
    pub fn intersection_at(&self, p: V2) -> Option<IntersectionId> {
        self.ix_grid.at(p).iter().copied().find(|i| {
            let ix = &self.net.intersections[*i as usize];
            geom::dist(p, geom::xy(self.net.nodes[ix.node as usize].pos)) <= ix.radius_m
        })
    }

    /// Segment ids registered in the grid cell containing `p` (anything within ~3 m of its
    /// edge is guaranteed to be listed).
    pub fn segs_near(&self, p: V2) -> &[u32] {
        self.seg_grid.at(p)
    }

    /// Best segment under `p` within `extra` metres beyond its half width. With `heading`,
    /// only segments whose tangent agrees (dot > 0.3) qualify. Filters: drivable only,
    /// connectors allowed.
    pub fn locate(
        &self,
        p: V3,
        heading: Option<V2>,
        extra: f32,
        drivable_only: bool,
        connectors: bool,
    ) -> Option<Located> {
        let q = geom::xy(p);
        let mut best: Option<(f32, Located)> = None;
        for &id in self.seg_grid.at(q) {
            let s = &self.segs[id as usize];
            if (drivable_only && !s.drivable) || (!connectors && s.is_connector()) {
                continue;
            }
            let pr = s.project(q);
            if pr.dist > s.width * 0.5 + extra {
                continue;
            }
            let sc = pr.s.clamp(0.0, s.length);
            let (pt, tan) = s.point_at(sc);
            if (pt[2] - p[2]).abs() > 4.0 {
                continue;
            }
            if let Some(h) = heading {
                if geom::dot(h, tan) < 0.3 {
                    continue;
                }
            }
            // Prefer lanes over connectors at equal distance.
            let score = pr.dist + if s.is_connector() { 0.25 } else { 0.0 };
            if best.as_ref().is_none_or(|(b, _)| score < *b) {
                best = Some((
                    score,
                    Located {
                        seg: id,
                        s: sc,
                        lateral: pr.lateral,
                        tangent: tan,
                    },
                ));
            }
        }
        best.map(|b| b.1)
    }

    /// Signal state for a lane's approach right now (None if not signalised).
    pub fn lane_signal(&self, lane: LaneId) -> Option<(SignalState, f32)> {
        let ap = self.approach[lane as usize]?;
        let ix = &self.net.intersections[ap.intersection as usize];
        match ix.control {
            IntersectionControl::Signal { controller } => Some(signals::group_state(
                &self.net.signals[controller as usize],
                ap.group,
                self.signal_tick,
            )),
            _ => None,
        }
    }

    /// Control rule at the end of a lane.
    pub fn lane_control(&self, lane: LaneId) -> Option<(IntersectionControl, Approach, bool)> {
        let ap = self.approach[lane as usize]?;
        let ix = &self.net.intersections[ap.intersection as usize];
        Some((ix.control, ap, ix.allow_rtor))
    }

    /// Zone district kind at `p`.
    pub fn district_at(&self, p: V2) -> Option<DistrictKind> {
        self.zone_at(p).map(|z| self.net.zones[z as usize].kind)
    }
}

fn conn_ends(s: &Segment) -> (LaneId, LaneId) {
    match s.kind {
        SegKind::Connector { from, to, .. } => (from, to),
        SegKind::Lane { lane } => (lane, lane),
    }
}

/// Arc length along `a` after which a car centred on `a` is clear of path `b`
/// (centreline separation above a car width plus margin, i.e. bodies no longer overlap).
fn divergence(a: &Segment, b: &Segment) -> f32 {
    const CLEAR_M: f32 = 2.6;
    let mut last_close = 0.0;
    for (i, p) in a.pts.iter().enumerate() {
        let d = b
            .pts
            .windows(2)
            .map(|w| geom::point_segment_distance(geom::xy(*p), geom::xy(w[0]), geom::xy(w[1])))
            .fold(f32::INFINITY, f32::min);
        if d < CLEAR_M {
            last_close = a.cum[i];
        }
    }
    last_close
}

/// Cubic Bézier from the end of `from` to the start of `to`, tangent-continuous.
fn connector_curve(from: &[V3], to: &[V3], turn: TurnKind) -> Vec<V3> {
    let n = from.len();
    let p0 = from[n - 1];
    let d0 = geom::normalize(geom::sub(geom::xy(from[n - 1]), geom::xy(from[n - 2])));
    let p3 = to[0];
    let d3 = geom::normalize(geom::sub(geom::xy(to[1]), geom::xy(to[0])));
    let chord = geom::dist(geom::xy(p0), geom::xy(p3));
    let k = match turn {
        TurnKind::UTurn => chord.max(4.0) * 1.2,
        _ => chord * 0.4,
    };
    let p1 = [p0[0] + d0[0] * k, p0[1] + d0[1] * k, p0[2]];
    let p2 = [p3[0] - d3[0] * k, p3[1] - d3[1] * k, p3[2]];
    let segments = if chord < 3.0 && turn != TurnKind::UTurn {
        2
    } else {
        10
    };
    geom::bezier(p0, p1, p2, p3, segments)
}

/// Lateral-acceleration-limited speed (2.8 m/s²) for a curve.
fn curve_speed(pts: &[V3], length: f32, limit: f32) -> f32 {
    let n = pts.len();
    let d0 = geom::normalize(geom::sub(geom::xy(pts[1]), geom::xy(pts[0])));
    let d1 = geom::normalize(geom::sub(geom::xy(pts[n - 1]), geom::xy(pts[n - 2])));
    let angle = geom::atan2(geom::cross(d0, d1), geom::dot(d0, d1)).abs();
    if angle < 0.15 {
        return limit;
    }
    let r = length / angle;
    (2.8 * r).sqrt().clamp(3.0, limit)
}

impl CityMap {
    /// Whether the segment `a`→`b` clears every building (contract v0.2.0 line of sight; added
    /// by the lead, workstream D owns it). A building blocks when the 2D segment crosses or
    /// enters its footprint and the segment's height there is below the roof. Linear in the
    /// building count; D may index it with a grid.
    pub fn line_of_sight(&self, a: [f32; 3], b: [f32; 3]) -> bool {
        let (pa, pb) = ([a[0], a[1]], [b[0], b[1]]);
        let d = geom::sub(pb, pa);
        for bld in &self.net.buildings {
            let fp = &bld.footprint;
            if fp.len() < 3 {
                continue;
            }
            // Smallest segment parameter at which the 2D segment touches the footprint.
            let mut t_hit: Option<f32> = if point_in_polygon(pa, fp) {
                Some(0.0)
            } else {
                None
            };
            for i in 0..fp.len() {
                let (e0, e1) = (fp[i], fp[(i + 1) % fp.len()]);
                let e = geom::sub(e1, e0);
                let den = geom::cross(d, e);
                if den.abs() < 1e-9 {
                    continue;
                }
                let w = geom::sub(e0, pa);
                let t = geom::cross(w, e) / den;
                let u = geom::cross(w, d) / den;
                if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
                    t_hit = Some(t_hit.map_or(t, |h: f32| h.min(t)));
                }
            }
            if let Some(t) = t_hit {
                let z = a[2] + (b[2] - a[2]) * t;
                if z < bld.height_m {
                    return false;
                }
            }
        }
        true
    }
}

/// Even-odd point-in-polygon test.
fn point_in_polygon(p: geom::V2, poly: &[geom::V2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (pi, pj) = (poly[i], poly[j]);
        if (pi[1] > p[1]) != (pj[1] > p[1])
            && p[0] < (pj[0] - pi[0]) * (p[1] - pi[1]) / (pj[1] - pi[1]) + pi[0]
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

impl WorldQuery for CityMap {
    fn surface_at(&self, p: [f32; 3]) -> SurfaceType {
        if self.net.lanes.is_empty() {
            return SurfaceType::Asphalt;
        }
        let q = geom::xy(p);
        if self.intersection_at(q).is_some() {
            return SurfaceType::Asphalt;
        }
        let mut curb = false;
        for &id in self.seg_grid.at(q) {
            let s = &self.segs[id as usize];
            let pr = s.project(q);
            if pr.s < -0.5 || pr.s > s.length + 0.5 {
                continue;
            }
            let half = s.width * 0.5;
            if pr.dist <= half {
                return match s.kind {
                    SegKind::Lane { lane }
                        if self.net.lanes[lane as usize].kind == LaneKind::Highway =>
                    {
                        SurfaceType::Concrete
                    }
                    _ => SurfaceType::Asphalt,
                };
            }
            if let SegKind::Lane { lane } = s.kind {
                let l = &self.net.lanes[lane as usize];
                if pr.dist <= half + 0.4
                    && ((pr.lateral > 0.0 && l.edge_right) || (pr.lateral < 0.0 && l.edge_left))
                {
                    curb = true;
                }
            }
        }
        if curb {
            return SurfaceType::Curb;
        }
        match self.district_at(q) {
            Some(DistrictKind::Core) | Some(DistrictKind::Commercial) => SurfaceType::Concrete,
            Some(DistrictKind::Industrial) | Some(DistrictKind::Port) => SurfaceType::Dirt,
            Some(DistrictKind::Residential) | Some(DistrictKind::Suburb) | None => {
                SurfaceType::Grass
            }
        }
    }

    fn speed_limit_at(&self, p: [f32; 3]) -> Option<f32> {
        self.locate(p, None, 1.0, true, true)
            .map(|l| self.segs[l.seg as usize].speed_limit)
    }

    fn signal_state_ahead(&self, p: [f32; 3], heading: [f32; 3]) -> Option<SignalState> {
        let h = geom::normalize([heading[0], heading[1]]);
        let loc = self.locate(p, Some(h), 1.0, false, false)?;
        let mut lane = loc.seg;
        let mut dist = self.segs[lane as usize].length - loc.s;
        for _ in 0..8 {
            if let Some((st, _)) = self.lane_signal(lane) {
                return Some(st);
            }
            if dist > 150.0 {
                return None;
            }
            let l = &self.net.lanes[lane as usize];
            let c = l
                .connections
                .iter()
                .find(|c| c.turn == TurnKind::Straight)
                .or(l.connections.first())?;
            lane = c.to;
            dist += self.segs[lane as usize].length + 10.0;
        }
        None
    }

    fn time_of_day(&self) -> TimeOfDay {
        self.time
    }

    fn line_of_sight(&self, a: [f32; 3], b: [f32; 3]) -> bool {
        CityMap::line_of_sight(self, a, b)
    }
}
