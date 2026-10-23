//! Runtime city map built from a [`RoadNetwork`]: driveable segments (lanes plus intersection
//! connector curves), connector conflict sets, a uniform-grid spatial index, a zone lookup grid
//! and the [`WorldQuery`] implementation.

use std::sync::Arc;

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
    /// Paved width beside the lane's left / right edge that belongs to no other lane: half
    /// the median of a divided road on the inner side, a highway's shoulder on the outer
    /// side, m (playtest row 94). 0 for connectors.
    pub paved_side: [f32; 2],
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
    /// Cross slope per point of `pts` (dz per metre right of the direction of travel) from the
    /// road surface ([`crate::surface::lane_bank`]); empty for connectors and lanes that do not
    /// follow their surface point for point.
    pub bank: Vec<f32>,
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

    /// Cross slope at arc length `s` ([`Segment::bank`]; 0 without one).
    pub fn bank_at(&self, s: f32) -> f32 {
        if self.bank.len() != self.pts.len() || self.pts.len() < 2 {
            return 0.0;
        }
        let n = self.pts.len();
        let i = self.cum.partition_point(|c| *c <= s).clamp(1, n - 1);
        let span = (self.cum[i] - self.cum[i - 1]).max(1e-6);
        let f = ((s - self.cum[i - 1]) / span).clamp(0.0, 1.0);
        self.bank[i - 1] + (self.bank[i] - self.bank[i - 1]) * f
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
///
/// The network, segments and spatial grids never change after [`CityMap::new`] and are
/// shared (`Arc`), so cloning a map — and a whole `wp_sim::Sim` for crash-replay snapshots —
/// copies only the small mutable part.
#[derive(Debug, Clone)]
pub struct CityMap {
    /// Source network.
    pub net: Arc<RoadNetwork>,
    /// Lanes (indices `0..lanes.len()`) followed by connectors.
    pub segs: Arc<Vec<Segment>>,
    /// Arrival info per lane (None = lane does not end at a junction).
    pub approach: Arc<Vec<Option<Approach>>>,
    /// Drivable lane length per zone, km.
    pub zone_lane_km: Vec<f32>,
    /// Physics ticks (`PHYSICS_DT`) the signal controllers have run.
    pub signal_tick: u64,
    /// Current time of day (mirrors the world clock).
    pub time: TimeOfDay,
    /// The terrain as physics collides with it ([`crate::surface::collision_terrain`]); `None`
    /// without terrain. Off the roads this is the drawn ground, not `net.terrain`.
    pub ground: Option<Arc<crate::terrain::Heightfield>>,
    seg_grid: Arc<Grid>,
    ix_grid: Arc<Grid>,
    zone_grid: Arc<Grid>,
    building_grid: Arc<Grid>,
    /// Covered spans (tunnels) ready for queries.
    pub covers: crate::covered::CoverIndex,
    /// Whether covered spans block line of sight (default `true`; `false` is the A/B control
    /// for tunnel scenarios: the geometry stays, only sightlines ignore it).
    pub covered_los: bool,
    /// Bridge-deck pieces that occlude line of sight ([`crate::surface::deck_slabs`]).
    decks: Arc<Vec<crate::surface::DeckSlab>>,
    deck_grid: Arc<Grid>,
    /// Freeway paint derived at build time (dotted taper lines, gore chevrons).
    pub(crate) paint: Arc<crate::markings::FreewayPaint>,
    /// Paving per source road (`Lane::road`), from the road surfaces.
    road_material: Arc<Vec<RoadMaterial>>,
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
                paved_side: lane_paved_side(&net, l),
                drivable: l.kind.drivable(),
                next: Vec::new(),
                prev: Vec::new(),
                curve_speed: l.speed_limit_mps,
                conflicts: Vec::new(),
                siblings: Vec::new(),
                bank: crate::surface::lane_bank(&net, l),
            });
        }
        // Freeway merges / diverges (radius-0 junctions): their long taper connectors follow
        // the paving under them (graded, banked) instead of a straight line between the lane
        // ends. Candidate surfaces: every road with a lane at the node.
        let mut freeway_roads: Vec<Vec<usize>> = vec![Vec::new(); net.nodes.len()];
        for l in &net.lanes {
            for n in [l.from_node, l.to_node] {
                let freeway = net.nodes[n as usize]
                    .intersection
                    .is_some_and(|ix| net.intersections[ix as usize].radius_m <= 0.0);
                let list = &mut freeway_roads[n as usize];
                if freeway && !list.contains(&(l.road as usize)) {
                    list.push(l.road as usize);
                }
            }
        }
        // Connectors.
        for (i, l) in net.lanes.iter().enumerate() {
            for c in &l.connections {
                let to = &net.lanes[c.to as usize];
                let mut pts = connector_curve(&l.polyline, &to.polyline, c.turn);
                let roads = &freeway_roads[l.to_node as usize];
                if !roads.is_empty() {
                    let n = pts.len();
                    for p in pts.iter_mut().take(n - 1).skip(1) {
                        let best = roads
                            .iter()
                            .filter_map(|r| net.surfaces.get(*r))
                            .map(|s| crate::surface::height_under(s, geom::xy(*p)))
                            .filter(|(_, z)| (z - p[2]).abs() < 3.0)
                            .min_by(|a, b| a.0.total_cmp(&b.0));
                        if let Some((_, z)) = best {
                            p[2] = z;
                        }
                    }
                }
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
                    paved_side: [0.0; 2],
                    drivable: l.kind.drivable() && to.kind.drivable(),
                    next: vec![c.to],
                    prev: vec![i as SegId],
                    conflicts: Vec::new(),
                    siblings: Vec::new(),
                    bank: Vec::new(),
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
        let mut building_grid = Grid::new(lo, hi);
        for (i, b) in net.buildings.iter().enumerate() {
            let (blo, bhi) = bbox2(&b.footprint);
            building_grid.insert_box(blo, bhi, i as u32);
        }
        let decks = crate::surface::deck_slabs(&net);
        let mut deck_grid = Grid::new(lo, hi);
        for (i, d) in decks.iter().enumerate() {
            let pad = d.left_m.max(d.right_m);
            let (a, b) = (geom::xy(d.a), geom::xy(d.b));
            deck_grid.insert_box(
                [a[0].min(b[0]) - pad, a[1].min(b[1]) - pad],
                [a[0].max(b[0]) + pad, a[1].max(b[1]) + pad],
                i as u32,
            );
        }
        let roads = net.lanes.iter().map(|l| l.road + 1).max().unwrap_or(0) as usize;
        let mut road_material = vec![RoadMaterial::Asphalt; roads];
        for s in &net.surfaces {
            if let Some(m) = road_material.get_mut(s.road as usize) {
                *m = s.material;
            }
        }
        let mut map = Self {
            road_material: Arc::new(road_material),
            zone_lane_km: vec![0.0; net.zones.len()],
            net: Arc::new(net),
            segs: Arc::new(segs),
            approach: Arc::new(approach),
            signal_tick: 0,
            time: TimeOfDay::from_hours(12.0),
            ground: None,
            seg_grid: Arc::new(seg_grid),
            ix_grid: Arc::new(ix_grid),
            zone_grid: Arc::new(zone_grid),
            building_grid: Arc::new(building_grid),
            covers: crate::covered::CoverIndex::default(),
            covered_los: true,
            decks: Arc::new(decks),
            deck_grid: Arc::new(deck_grid),
            paint: Default::default(),
        };
        map.covers = crate::covered::CoverIndex::new(&map.net.covered);
        map.paint = Arc::new(crate::markings::freeway_paint(&map));
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
        map.ground = crate::surface::collision_terrain(&map).map(Arc::new);
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

    /// Drivable surface height at `p`: the lane or junction connector under it (the one
    /// closest to `near_z` when given, else the lowest — grade separation), else the terrain.
    pub fn ground_height(&self, p: V2, near_z: Option<f32>) -> f32 {
        let mut best: Option<f32> = None;
        let score = |z: f32| near_z.map_or(z, |h| (z - h).abs());
        for &id in self.seg_grid.at(p) {
            let s = &self.segs[id as usize];
            let pr = s.project(p);
            let pad = s.paved_side[usize::from(pr.lateral > 0.0)];
            if pr.dist > s.width * 0.5 + pad + 0.5 {
                continue;
            }
            let sc = pr.s.clamp(0.0, s.length);
            // The lane's height plus its road's cross slope out to `p` (a banked road's edge
            // sits bank × offset off the lane centre: up to ~5 cm on Bluff Road's curves, which
            // the drawn and solid surface follow).
            let z = s.point_at(sc).0[2] + s.bank_at(sc) * pr.lateral;
            if best.is_none_or(|b| score(z) < score(b)) {
                best = Some(z);
            }
        }
        if let Some(ix) = self.intersection_at(p) {
            let z = self.net.nodes[self.net.intersections[ix as usize].node as usize].pos[2];
            if best.is_none_or(|b| score(z) < score(b)) {
                best = Some(z);
            }
        }
        best.unwrap_or_else(|| {
            self.ground
                .as_ref()
                .map_or_else(|| self.net.ground_z(p), |g| g.height_on_triangles(p))
        })
    }

    /// Zone district kind at `p`.
    pub fn district_at(&self, p: V2) -> Option<DistrictKind> {
        self.zone_at(p).map(|z| self.net.zones[z as usize].kind)
    }
}

/// [`Segment::paved_side`] of a lane: the paved surface of its road beyond the lane's left
/// edge (outermost-left lane only: to the centreline of a two-way road, to the surface edge
/// of a one-way one) and beyond its right edge (outermost-right lane only), measured at the
/// lane's middle; at most 4 m.
fn lane_paved_side(net: &RoadNetwork, l: &Lane) -> [f32; 2] {
    let Some(sf) = net.surfaces.iter().find(|s| s.road == l.road) else {
        return [0.0; 2];
    };
    let n = l.polyline.len();
    if n < 2 || sf.centre.len() < 2 {
        return [0.0; 2];
    }
    let mid = geom::xy(l.polyline[n / 2]);
    let ldir = geom::normalize(geom::sub(
        geom::xy(l.polyline[(n / 2).max(1)]),
        geom::xy(l.polyline[(n / 2).max(1) - 1]),
    ));
    let cum = geom::cumulative(&sf.centre);
    let pr = geom::project(&sf.centre, &cum, mid);
    let (_, cdir) = geom::point_at(
        &sf.centre,
        &cum,
        pr.s.clamp(0.0, *cum.last().unwrap_or(&0.0)),
    );
    // Lane frame: +1 when the lane runs a → b along the surface.
    let sigma = if geom::dot(ldir, cdir) >= 0.0 {
        1.0
    } else {
        -1.0
    };
    let off = sigma * pr.lateral;
    let (lo, hi) = if sigma > 0.0 {
        (-sf.left_m, sf.right_m)
    } else {
        (-sf.right_m, sf.left_m)
    };
    let half = l.width_m * 0.5;
    let two_way = net
        .lanes
        .iter()
        .any(|o| o.road == l.road && o.from_node == l.to_node && o.to_node == l.from_node);
    let left = if l.left.is_none() && l.kind != LaneKind::Parking {
        let bound = if two_way { 0.0 } else { lo };
        (off - half - bound).clamp(0.0, 4.0)
    } else {
        0.0
    };
    let right = if l.right.is_none() {
        (hi - (off + half)).clamp(0.0, 4.0)
    } else {
        0.0
    };
    // Below a few cm it is rounding, not a strip.
    let tidy = |v: f32| if v < 0.05 { 0.0 } else { v };
    [tidy(left), tidy(right)]
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

impl Grid {
    /// Cells crossed by the segment `a`→`b` (2D DDA), in order; cells outside the grid are
    /// skipped.
    fn cells_on_segment(&self, a: V2, b: V2, out: &mut Vec<usize>) {
        let to_cell = |v: f32, o: f32| ((v - o) / CELL_M).floor();
        let (mut cx, mut cy) = (to_cell(a[0], self.origin[0]), to_cell(a[1], self.origin[1]));
        let (ex, ey) = (to_cell(b[0], self.origin[0]), to_cell(b[1], self.origin[1]));
        let d = geom::sub(b, a);
        let step_x = if d[0] > 0.0 { 1.0 } else { -1.0 };
        let step_y = if d[1] > 0.0 { 1.0 } else { -1.0 };
        // Parameter t at which the segment crosses the next cell boundary in x / y.
        let boundary =
            |c: f32, step: f32, o: f32| o + (c + if step > 0.0 { 1.0 } else { 0.0 }) * CELL_M;
        let mut t_max_x = if d[0] != 0.0 {
            (boundary(cx, step_x, self.origin[0]) - a[0]) / d[0]
        } else {
            f32::INFINITY
        };
        let mut t_max_y = if d[1] != 0.0 {
            (boundary(cy, step_y, self.origin[1]) - a[1]) / d[1]
        } else {
            f32::INFINITY
        };
        let t_dx = if d[0] != 0.0 {
            CELL_M / d[0].abs()
        } else {
            f32::INFINITY
        };
        let t_dy = if d[1] != 0.0 {
            CELL_M / d[1].abs()
        } else {
            f32::INFINITY
        };
        let max_steps = (ex - cx).abs() as usize + (ey - cy).abs() as usize + 1;
        for _ in 0..=max_steps {
            if cx >= 0.0 && cy >= 0.0 && cx < self.nx as f32 && cy < self.ny as f32 {
                out.push(cy as usize * self.nx + cx as usize);
            }
            if cx == ex && cy == ey {
                break;
            }
            if t_max_x < t_max_y {
                cx += step_x;
                t_max_x += t_dx;
            } else {
                cy += step_y;
                t_max_y += t_dy;
            }
        }
    }
}

fn bbox2(poly: &[V2]) -> (V2, V2) {
    let mut lo = [f32::INFINITY; 2];
    let mut hi = [f32::NEG_INFINITY; 2];
    for p in poly {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    (lo, hi)
}

/// Whether the 3D segment `a`→`b` passes through an extruded footprint (ground to `height`).
/// The segment's XY span inside the footprint is `[t_in, t_out]`; since its height is linear
/// in `t`, it is below the roof somewhere in that span iff it is at one of the two ends.
fn segment_hits_building(a: V3, b: V3, fp: &[V2], height: f32) -> bool {
    let (pa, pb) = (geom::xy(a), geom::xy(b));
    let d = geom::sub(pb, pa);
    let mut t_in = f32::INFINITY;
    let mut t_out = f32::NEG_INFINITY;
    let mut note = |t: f32| {
        t_in = t_in.min(t);
        t_out = t_out.max(t);
    };
    if geom::point_in_polygon(pa, fp) {
        note(0.0);
    }
    if geom::point_in_polygon(pb, fp) {
        note(1.0);
    }
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
            note(t);
        }
    }
    if t_in > t_out {
        return false;
    }
    let z = |t: f32| a[2] + (b[2] - a[2]) * t;
    z(t_in).min(z(t_out)) < height
}

impl CityMap {
    /// Whether the segment `a`→`b` clears every building (contract v0.2.0 line of sight).
    /// Buildings are extruded footprints from the ground to their height; the segment is
    /// blocked if any part of it inside a footprint is below the roof. Only buildings in grid
    /// cells the segment crosses are tested. With terrain, the ground occludes too (sampled
    /// every 8 m; ends excluded). Covered spans (tunnels) block sightlines in and out of them
    /// ([`crate::covered`]) unless [`CityMap::covered_los`] is off, and bridge decks with room
    /// for a car under them ([`crate::surface::deck_slabs`]) block too: from the air, the road
    /// under an overpass is hidden. Other road surfaces, parked cars and traffic do not occlude.
    pub fn line_of_sight(&self, a: V3, b: V3) -> bool {
        if self.covered_los && self.covers.blocks(a, b) {
            return false;
        }
        if let Some(t) = &self.net.terrain {
            let len = geom::dist(geom::xy(a), geom::xy(b));
            let n = (len / 8.0).ceil() as u32;
            for k in 1..n {
                let f = k as f32 / n as f32;
                let p = [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f];
                if a[2] + (b[2] - a[2]) * f < t.height_at(p) - 0.2 {
                    return false;
                }
            }
        }
        let mut cells = Vec::new();
        if !self.decks.is_empty() {
            self.deck_grid
                .cells_on_segment(geom::xy(a), geom::xy(b), &mut cells);
            let mut cands: Vec<u32> = cells
                .iter()
                .flat_map(|c| self.deck_grid.cells[*c].iter().copied())
                .collect();
            cands.sort_unstable();
            cands.dedup();
            if cands.iter().any(|&i| self.decks[i as usize].blocks(a, b)) {
                return false;
            }
        }
        if self.net.buildings.is_empty() {
            return true;
        }
        cells.clear();
        self.building_grid
            .cells_on_segment(geom::xy(a), geom::xy(b), &mut cells);
        let mut cands: Vec<u32> = cells
            .iter()
            .flat_map(|c| self.building_grid.cells[*c].iter().copied())
            .collect();
        cands.sort_unstable();
        cands.dedup();
        !cands.iter().any(|&i| {
            let bld = &self.net.buildings[i as usize];
            bld.footprint.len() >= 3 && segment_hits_building(a, b, &bld.footprint, bld.roof_z())
        })
    }

    /// Bridge-deck pieces that occlude line of sight (empty without terrain).
    pub fn decks(&self) -> &[crate::surface::DeckSlab] {
        &self.decks
    }
}

impl CityMap {
    /// Index into [`RoadNetwork::covered`] of the tunnel / covered road containing `p`, if any
    /// (independent of [`CityMap::covered_los`]).
    pub fn covered_at(&self, p: V3) -> Option<usize> {
        self.covers.covered_at(p)
    }
}

impl CityMap {
    /// Paving of lane `l`'s road ([`RoadSurface::material`]; asphalt when the city has no
    /// surfaces).
    pub fn lane_material(&self, l: LaneId) -> RoadMaterial {
        let road = self.net.lanes[l as usize].road;
        self.road_material
            .get(road as usize)
            .copied()
            .unwrap_or_default()
    }
}

/// Wheel surface of an unpaved road material.
fn material_surface(m: RoadMaterial) -> SurfaceType {
    match m {
        RoadMaterial::Asphalt => SurfaceType::Asphalt,
        RoadMaterial::Gravel => SurfaceType::Gravel,
        RoadMaterial::Dirt => SurfaceType::Dirt,
    }
}

impl WorldQuery for CityMap {
    fn surface_at(&self, p: [f32; 3]) -> SurfaceType {
        if self.net.lanes.is_empty() {
            return SurfaceType::Asphalt;
        }
        if crate::markings::painted_at(self, p) {
            return SurfaceType::PaintedLine;
        }
        let q = geom::xy(p);
        if let Some(ix) = self.intersection_at(q) {
            let node = self.net.intersections[ix as usize].node;
            if (self.net.nodes[node as usize].pos[2] - p[2]).abs() < 3.0 {
                return SurfaceType::Asphalt;
            }
        }
        let mut curb = false;
        for &id in self.seg_grid.at(q) {
            let s = &self.segs[id as usize];
            let pr = s.project(q);
            if pr.s < -0.5 || pr.s > s.length + 0.5 {
                continue;
            }
            // Grade separation: a deck above (or a road below) is not under this wheel.
            if (s.point_at(pr.s.clamp(0.0, s.length)).0[2] - p[2]).abs() > 3.0 {
                continue;
            }
            let half = s.width * 0.5;
            // The lane, or the median half / shoulder beside it.
            if pr.dist <= half + s.paved_side[usize::from(pr.lateral > 0.0)] {
                let highway = |l: LaneId| self.net.lanes[l as usize].kind == LaneKind::Highway;
                let unpaved = |l: LaneId| self.lane_material(l);
                return match s.kind {
                    SegKind::Lane { lane } if highway(lane) => SurfaceType::Concrete,
                    SegKind::Lane { lane } if unpaved(lane) != RoadMaterial::Asphalt => {
                        material_surface(unpaved(lane))
                    }
                    SegKind::Connector { from, to, .. }
                        if unpaved(from) != RoadMaterial::Asphalt
                            && unpaved(from) == unpaved(to) =>
                    {
                        material_surface(unpaved(from))
                    }
                    // Through a freeway merge / diverge (radius-0 junction).
                    SegKind::Connector { from, to, node, .. }
                        if highway(from)
                            && highway(to)
                            && self.net.nodes[node as usize]
                                .intersection
                                .is_some_and(|ix| {
                                    self.net.intersections[ix as usize].radius_m <= 0.0
                                }) =>
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::Building;

    fn city_with(buildings: Vec<Building>) -> CityMap {
        let mut net = crate::procgen::generate(&crate::procgen::ProcGenConfig::small());
        net.buildings = buildings;
        CityMap::new(net)
    }

    fn block(x0: f32, y0: f32, x1: f32, y1: f32, h: f32) -> Building {
        Building::block(vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]], h, 0.0)
    }

    /// Brute force over every building (reference for the grid walk).
    /// Every building and every deck slab (main's grid viaduct is decks since lead/main-sync).
    fn los_brute(map: &CityMap, a: V3, b: V3) -> bool {
        !map.net
            .buildings
            .iter()
            .any(|bl| segment_hits_building(a, b, &bl.footprint, bl.roof_z()))
            && !map.decks().iter().any(|d| d.blocks(a, b))
    }

    #[test]
    fn line_of_sight_cases() {
        let map = city_with(vec![block(100.0, 100.0, 140.0, 140.0, 20.0)]);
        // Straight through at eye height: blocked. Beside it: clear.
        assert!(!map.line_of_sight([80.0, 120.0, 1.5], [160.0, 120.0, 1.5]));
        assert!(map.line_of_sight([80.0, 150.0, 1.5], [160.0, 150.0, 1.5]));
        // High above the roof: clear.
        assert!(map.line_of_sight([80.0, 120.0, 30.0], [160.0, 120.0, 30.0]));
        // Enters above the roof but descends into it (helicopter to a car behind it): blocked.
        assert!(!map.line_of_sight([90.0, 120.0, 25.0], [130.0, 120.0, 1.0]));
        // Endpoint inside the building: blocked; both endpoints inside but above: clear.
        assert!(!map.line_of_sight([120.0, 120.0, 1.0], [200.0, 120.0, 1.0]));
        assert!(map.line_of_sight([110.0, 110.0, 25.0], [130.0, 130.0, 25.0]));
        // Diagonal clipping a corner.
        assert!(!map.line_of_sight([90.0, 130.0, 1.0], [110.0, 145.0, 1.0]));
        assert!(map.line_of_sight([90.0, 135.0, 1.0], [110.0, 150.0, 1.0]));
        // No buildings: always clear.
        assert!(city_with(vec![]).line_of_sight([0.0; 3], [500.0, 500.0, 0.0]));
    }

    #[test]
    fn grid_walk_matches_brute_force() {
        let map = CityMap::new(crate::procgen::generate(
            &crate::procgen::ProcGenConfig::default(),
        ));
        let mut rng = wp_math::Rng::new(5);
        let mut blocked = 0;
        for _ in 0..3000 {
            let mut p = || {
                [
                    rng.range_f32(-800.0, 800.0),
                    rng.range_f32(-800.0, 800.0),
                    rng.range_f32(0.5, 30.0),
                ]
            };
            let (a, b) = (p(), p());
            let los = map.line_of_sight(a, b);
            assert_eq!(los, los_brute(&map, a, b), "{a:?} -> {b:?}");
            blocked += usize::from(!los);
        }
        assert!(
            blocked > 1000,
            "city buildings should block most long sight lines"
        );
    }
}
