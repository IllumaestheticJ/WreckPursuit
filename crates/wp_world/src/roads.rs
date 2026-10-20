//! Road-level view of the city for systems that do not need lanes: police routing and
//! dispatch, the captain's minimap, race-route previews.
//!
//! One [`RoadEdge`] per source road (both directions of a street are one edge) between graph
//! nodes, with class, limit, one-way flag and district, plus POIs and building occluders. It
//! maps 1:1 onto `wp_police::RoadGraph` (`add_node` / `add_segment`), and it is the proposed
//! shape of the shared read-only road-network trait for contract v0.3.0
//! (`docs/integration/world.md`).

use serde::{Deserialize, Serialize};

use crate::builder::{mph, RoadClass};
use crate::geom::{self, V2};
use crate::map::CityMap;
use crate::network::{DistrictKind, LaneKind, NodeId, Poi};

/// A road graph node (junction, bend or dead end).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RoadNodeView {
    /// Position (world XY), m.
    pub pos: V2,
    /// District at the node (None outside every zone).
    pub district: Option<DistrictKind>,
    /// The node is a signalised junction.
    pub signalised: bool,
}

/// A road between two nodes (both directions).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoadEdge {
    /// Start node (index into [`RoadGraphView::nodes`] = network node id).
    pub a: NodeId,
    /// End node.
    pub b: NodeId,
    /// Centreline `a` → `b` through the junction centres, m.
    pub polyline: Vec<V2>,
    /// Polyline length, m.
    pub length_m: f32,
    /// Speed limit, m/s.
    pub speed_limit_mps: f32,
    /// Functional class (derived from lane kinds, lane counts and limit).
    pub class: RoadClass,
    /// Drivable lanes `a` → `b`.
    pub lanes_ab: u8,
    /// Drivable lanes `b` → `a` (0 = one-way `a` → `b`).
    pub lanes_ba: u8,
    /// District at the edge midpoint.
    pub district: Option<DistrictKind>,
}

impl RoadEdge {
    /// One-way `a` → `b`.
    pub fn oneway(&self) -> bool {
        self.lanes_ba == 0
    }
}

/// Axis-aligned bounds of a building, with its height (for cheap 2D occlusion).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OccluderBox {
    /// Min corner, m.
    pub min: V2,
    /// Max corner, m.
    pub max: V2,
    /// Height, m.
    pub height_m: f32,
}

/// The road-level view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoadGraphView {
    /// Nodes, indexed by network node id.
    pub nodes: Vec<RoadNodeView>,
    /// Roads.
    pub edges: Vec<RoadEdge>,
    /// Points of interest (as in the network).
    pub pois: Vec<Poi>,
    /// Building bounds (exact footprints: `RoadNetwork::buildings`; exact LOS:
    /// `WorldQuery::line_of_sight`).
    pub occluders: Vec<OccluderBox>,
}

impl RoadGraphView {
    /// Build the view from a city map.
    pub fn from_map(map: &CityMap) -> Self {
        let net = &map.net;
        let nodes = net
            .nodes
            .iter()
            .map(|n| RoadNodeView {
                pos: geom::xy(n.pos),
                district: map.district_at(geom::xy(n.pos)),
                signalised: n.intersection.is_some_and(|i| {
                    matches!(
                        net.intersections[i as usize].control,
                        crate::network::IntersectionControl::Signal { .. }
                    )
                }),
            })
            .collect();
        // Group drivable lanes by source road, in road order.
        let n_roads = net.lanes.iter().map(|l| l.road + 1).max().unwrap_or(0) as usize;
        let mut by_road: Vec<Vec<usize>> = vec![Vec::new(); n_roads];
        for (i, l) in net.lanes.iter().enumerate() {
            if l.kind != LaneKind::Parking {
                by_road[l.road as usize].push(i);
            }
        }
        let mut edges = Vec::new();
        for lanes in by_road.iter().filter(|l| !l.is_empty()) {
            let first = &net.lanes[lanes[0]];
            let (a, b) = (first.from_node, first.to_node);
            let count = |from: NodeId, kinds_ok: fn(LaneKind) -> bool| {
                lanes
                    .iter()
                    .filter(|&&i| net.lanes[i].from_node == from && kinds_ok(net.lanes[i].kind))
                    .count() as u8
            };
            let lanes_ab = count(a, LaneKind::drivable);
            let lanes_ba = count(b, LaneKind::drivable);
            let kinds: Vec<LaneKind> = lanes.iter().map(|&i| net.lanes[i].kind).collect();
            let speed = first.speed_limit_mps;
            let class = if kinds.contains(&LaneKind::Highway) {
                RoadClass::Highway
            } else if kinds.contains(&LaneKind::Alley) {
                RoadClass::Alley
            } else if lanes_ab.max(lanes_ba) >= 2 || speed >= mph(35.0) - 0.1 {
                RoadClass::Arterial
            } else if speed >= mph(30.0) - 0.1 {
                RoadClass::Collector
            } else {
                RoadClass::Local
            };
            // Centreline: node centres plus the interior bends of the road's surface centreline
            // (exact geometry lives in the lanes). Networks without surfaces use the first
            // lane's bends, which sit up to half a road width off the centre. Consumers offset
            // from this line to find a lane (police drive half a lane right of it), so on
            // curved roads it must be the real centre, not a lane.
            let (pa, pb) = (
                geom::xy(net.nodes[a as usize].pos),
                geom::xy(net.nodes[b as usize].pos),
            );
            let mut bends: Vec<V2> = match net.surfaces.iter().find(|s| s.road == first.road) {
                Some(surf) if surf.centre.len() >= 2 => {
                    let c = &surf.centre;
                    let mut v: Vec<V2> = c[1..c.len() - 1].iter().map(|p| geom::xy(*p)).collect();
                    // The surface runs along the road's own a → b, which may be this edge's b → a.
                    let (c0, c1) = (geom::xy(c[0]), geom::xy(c[c.len() - 1]));
                    if geom::dist(c0, pa) + geom::dist(c1, pb)
                        > geom::dist(c0, pb) + geom::dist(c1, pa)
                    {
                        v.reverse();
                    }
                    v
                }
                _ => {
                    let pl = &first.polyline;
                    pl.iter()
                        .take(pl.len() - 1)
                        .skip(1)
                        .map(|p| geom::xy(*p))
                        .collect()
                }
            };
            let mut polyline = vec![pa];
            polyline.append(&mut bends);
            polyline.push(pb);
            let length_m = polyline.windows(2).map(|w| geom::dist(w[0], w[1])).sum();
            let (mp, _) = {
                let pts: Vec<[f32; 3]> = polyline.iter().map(|p| [p[0], p[1], 0.0]).collect();
                let cum = geom::cumulative(&pts);
                geom::point_at(&pts, &cum, cum[cum.len() - 1] * 0.5)
            };
            edges.push(RoadEdge {
                a,
                b,
                polyline,
                length_m,
                speed_limit_mps: speed,
                class,
                lanes_ab,
                lanes_ba,
                district: map.district_at(geom::xy(mp)),
            });
        }
        let occluders = net
            .buildings
            .iter()
            .filter(|b| !b.footprint.is_empty())
            .map(|b| {
                let mut lo = [f32::INFINITY; 2];
                let mut hi = [f32::NEG_INFINITY; 2];
                for p in &b.footprint {
                    lo = [lo[0].min(p[0]), lo[1].min(p[1])];
                    hi = [hi[0].max(p[0]), hi[1].max(p[1])];
                }
                OccluderBox {
                    min: lo,
                    max: hi,
                    height_m: b.height_m,
                }
            })
            .collect();
        Self {
            nodes,
            edges,
            pois: net.pois.clone(),
            occluders,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procgen::{generate, ProcGenConfig};

    /// The old linear scan (lead's first cut), kept as the reference answer.
    fn nearest_brute(view: &RoadGraphView, p: V2) -> Option<(u32, f32, V2, f32)> {
        let mut best: Option<(u32, f32, V2, f32)> = None;
        for (i, e) in view.edges.iter().enumerate() {
            let pts: Vec<[f32; 3]> = e.polyline.iter().map(|q| [q[0], q[1], 0.0]).collect();
            let cum = geom::cumulative(&pts);
            let len = cum[cum.len() - 1];
            let pr = geom::project(&pts, &cum, p);
            if best.is_none_or(|b| pr.dist < b.3) {
                let s = pr.s.clamp(0.0, len);
                let (q, _) = geom::point_at(&pts, &cum, s);
                let frac = if len > 0.0 { s / len } else { 0.0 };
                best = Some((i as u32, frac, geom::xy(q), pr.dist));
            }
        }
        best
    }

    #[test]
    fn nearest_on_road_matches_linear_scan() {
        use wp_contracts::RoadNetworkQuery;
        let map = CityMap::new(generate(&ProcGenConfig::small()));
        let idx = RoadNetworkIndex::new(&map);
        let (lo, hi) = (map.net.bounds_min, map.net.bounds_max);
        let mut rng = wp_math::Rng::new(7);
        for k in 0..4000 {
            // Mostly inside the city, some well outside it.
            let m = if k % 10 == 0 { 400.0 } else { 0.0 };
            let p = [
                lo[0] - m + rng.next_f32() * (hi[0] - lo[0] + 2.0 * m),
                lo[1] - m + rng.next_f32() * (hi[1] - lo[1] + 2.0 * m),
            ];
            let a = idx.nearest_on_road(p).unwrap();
            let b = nearest_brute(idx.view(), p).unwrap();
            assert!((a.3 - b.3).abs() < 1e-3, "{p:?}: {a:?} vs {b:?}");
            if (a.3 - b.3).abs() < 1e-6 && a.0 != b.0 {
                // Equidistant from two roads (a shared junction centre): either is right.
                continue;
            }
            assert_eq!(a.0, b.0, "{p:?}: {a:?} vs {b:?}");
            assert!((a.1 - b.1).abs() < 1e-3, "{p:?}: {a:?} vs {b:?}");
            assert!(geom::dist(a.2, b.2) < 1e-2, "{p:?}: {a:?} vs {b:?}");
        }
    }

    /// `cargo test -p wp_world --release --lib nearest_on_road_timing -- --ignored --nocapture`
    #[test]
    #[ignore = "timing report, run in release"]
    fn nearest_on_road_timing() {
        use wp_contracts::RoadNetworkQuery;
        let map = CityMap::new(generate(&ProcGenConfig::default()));
        let idx = RoadNetworkIndex::new(&map);
        let (lo, hi) = (map.net.bounds_min, map.net.bounds_max);
        let mut rng = wp_math::Rng::new(3);
        let pts: Vec<V2> = (0..20_000)
            .map(|_| {
                [
                    lo[0] + rng.next_f32() * (hi[0] - lo[0]),
                    lo[1] + rng.next_f32() * (hi[1] - lo[1]),
                ]
            })
            .collect();
        let t0 = std::time::Instant::now();
        let mut acc = 0.0;
        for p in &pts {
            acc += idx.nearest_on_road(*p).unwrap().3;
        }
        let grid = t0.elapsed().as_secs_f64() / pts.len() as f64;
        let t0 = std::time::Instant::now();
        for p in pts.iter().take(500) {
            acc += nearest_brute(idx.view(), *p).unwrap().3;
        }
        let brute = t0.elapsed().as_secs_f64() / 500.0;
        println!(
            "{} edges: grid {:.2} us/query, linear scan {:.1} us/query ({acc:.0})",
            idx.edge_count(),
            grid * 1e6,
            brute * 1e6
        );
    }

    #[test]
    fn road_network_index_implements_the_contract_trait() {
        use wp_contracts::RoadNetworkQuery;
        let map = CityMap::new(generate(&ProcGenConfig::default()));
        let idx = RoadNetworkIndex::new(&map);
        let q: &dyn RoadNetworkQuery = &idx;
        assert_eq!(q.node_count() as usize, map.net.nodes.len());
        assert_eq!(q.edge_count() as usize, idx.view().edges.len());
        for n in 0..q.node_count() {
            let out = q.edges_from(n);
            assert!(out.windows(2).all(|w| w[0] < w[1]));
            for &e in out {
                let info = q.edge(e);
                assert!(info.a == n || info.b == n);
            }
        }
        // A point on an edge's centreline is found on that road at distance ~0.
        let e = q.edge_count() / 2;
        let pl = q.edge_polyline(e);
        let p = [(pl[0][0] + pl[1][0]) * 0.5, (pl[0][1] + pl[1][1]) * 0.5];
        let (_, frac, at, d) = q.nearest_on_road(p).unwrap();
        assert!(d < 1e-3 && (0.0..=1.0).contains(&frac), "{d} {frac}");
        assert!((at[0] - p[0]).abs() < 1e-2 && (at[1] - p[1]).abs() < 1e-2);
        assert_eq!(q.district_at(p), map.district_at(p));
        assert!(q
            .pois()
            .iter()
            .any(|p| p.kind == wp_contracts::PoiKind::Camera));
    }

    #[test]
    fn procedural_city_road_view() {
        let map = CityMap::new(generate(&ProcGenConfig::default()));
        let v = RoadGraphView::from_map(&map);
        assert_eq!(v.nodes.len(), map.net.nodes.len());
        assert_eq!(v.occluders.len(), map.net.buildings.len());
        assert!(v.edges.len() > 300, "{}", v.edges.len());
        for e in &v.edges {
            assert!((e.a as usize) < v.nodes.len() && (e.b as usize) < v.nodes.len());
            assert!(e.lanes_ab >= 1 && e.length_m > 10.0);
            // Procedural roads are two-way and straight between junction centres.
            assert!(!e.oneway());
            let straight = geom::dist(v.nodes[e.a as usize].pos, v.nodes[e.b as usize].pos);
            assert!((e.length_m - straight).abs() < 0.5);
        }
        for c in [
            RoadClass::Highway,
            RoadClass::Arterial,
            RoadClass::Collector,
            RoadClass::Local,
            RoadClass::Alley,
        ] {
            assert!(v.edges.iter().any(|e| e.class == c), "no {c:?} edges");
        }
        assert!(v.nodes.iter().filter(|n| n.signalised).count() == map.net.signals.len());
        // Connected: every node reachable from node 0 over edges (ignoring direction).
        let mut seen = vec![false; v.nodes.len()];
        let mut stack = vec![0usize];
        seen[0] = true;
        while let Some(n) = stack.pop() {
            for e in &v.edges {
                for (x, y) in [(e.a, e.b), (e.b, e.a)] {
                    if x as usize == n && !seen[y as usize] {
                        seen[y as usize] = true;
                        stack.push(y as usize);
                    }
                }
            }
        }
        assert!(seen.iter().all(|s| *s), "road graph is not connected");
    }
}

impl From<RoadClass> for wp_contracts::RoadClass {
    fn from(c: RoadClass) -> Self {
        match c {
            RoadClass::Highway => wp_contracts::RoadClass::Highway,
            RoadClass::Arterial => wp_contracts::RoadClass::Arterial,
            RoadClass::Collector => wp_contracts::RoadClass::Collector,
            RoadClass::Local => wp_contracts::RoadClass::Local,
            RoadClass::Alley => wp_contracts::RoadClass::Alley,
        }
    }
}

/// Contract [`wp_contracts::RoadNetworkQuery`] over a city map (v0.3.0): the
/// [`RoadGraphView`] plus out-edge lists and contract POIs. Build once per city.
#[derive(Debug, Clone)]
pub struct RoadNetworkIndex<'a> {
    map: &'a CityMap,
    view: RoadGraphView,
    out: Vec<Vec<u32>>,
    pois: Vec<wp_contracts::PoiInfo>,
    near: EdgeGrid,
}

/// Uniform grid over every edge's polyline pieces, for [`RoadNetworkIndex`]'s
/// `nearest_on_road` (rings searched outwards until no closer piece can exist).
#[derive(Debug, Clone)]
struct EdgeGrid {
    origin: V2,
    nx: i32,
    ny: i32,
    /// `(edge, piece)` pairs per cell, ascending.
    cells: Vec<Vec<(u32, u32)>>,
    /// Cumulative arc length per edge polyline point.
    cum: Vec<Vec<f32>>,
}

/// Edge grid cell size, m.
const EDGE_CELL_M: f32 = 32.0;

impl EdgeGrid {
    fn new(edges: &[RoadEdge]) -> Self {
        let mut lo = [f32::INFINITY; 2];
        let mut hi = [f32::NEG_INFINITY; 2];
        for e in edges {
            for p in &e.polyline {
                lo = [lo[0].min(p[0]), lo[1].min(p[1])];
                hi = [hi[0].max(p[0]), hi[1].max(p[1])];
            }
        }
        if lo[0] > hi[0] {
            lo = [0.0; 2];
            hi = [0.0; 2];
        }
        let nx = (((hi[0] - lo[0]) / EDGE_CELL_M).floor() as i32 + 1).max(1);
        let ny = (((hi[1] - lo[1]) / EDGE_CELL_M).floor() as i32 + 1).max(1);
        let mut g = Self {
            origin: lo,
            nx,
            ny,
            cells: vec![Vec::new(); (nx * ny) as usize],
            cum: Vec::with_capacity(edges.len()),
        };
        for (ei, e) in edges.iter().enumerate() {
            let mut c = Vec::with_capacity(e.polyline.len());
            let mut acc = 0.0;
            c.push(0.0);
            for (k, w) in e.polyline.windows(2).enumerate() {
                acc += geom::dist(w[0], w[1]);
                c.push(acc);
                let (x0, y0) = g.cell_xy([w[0][0].min(w[1][0]), w[0][1].min(w[1][1])]);
                let (x1, y1) = g.cell_xy([w[0][0].max(w[1][0]), w[0][1].max(w[1][1])]);
                for cy in y0..=y1 {
                    for cx in x0..=x1 {
                        g.cells[(cy * nx + cx) as usize].push((ei as u32, k as u32));
                    }
                }
            }
            g.cum.push(c);
        }
        g
    }

    /// Cell coordinates of `p`, clamped to the grid.
    fn cell_xy(&self, p: V2) -> (i32, i32) {
        let cx = ((p[0] - self.origin[0]) / EDGE_CELL_M).floor() as i32;
        let cy = ((p[1] - self.origin[1]) / EDGE_CELL_M).floor() as i32;
        (cx.clamp(0, self.nx - 1), cy.clamp(0, self.ny - 1))
    }

    /// Closest point on any edge: `(edge, fraction from a, point, distance)`. Ties go to the
    /// lowest edge id, then the lowest piece (the same answer as a linear scan).
    fn nearest(&self, edges: &[RoadEdge], p: V2) -> Option<(u32, f32, V2, f32)> {
        if edges.is_empty() {
            return None;
        }
        let (cx, cy) = self.cell_xy(p);
        // (dist, edge, piece, t)
        let mut best: Option<(f32, u32, u32, f32)> = None;
        let max_r = self.nx.max(self.ny);
        for r in 0..=max_r {
            for y in (cy - r)..=(cy + r) {
                if y < 0 || y >= self.ny {
                    continue;
                }
                let on_row_edge = y == cy - r || y == cy + r;
                let mut x = cx - r;
                while x <= cx + r {
                    if x >= 0 && x < self.nx {
                        for &(e, k) in &self.cells[(y * self.nx + x) as usize] {
                            let pl = &edges[e as usize].polyline;
                            let (a, b) = (pl[k as usize], pl[k as usize + 1]);
                            let ab = geom::sub(b, a);
                            let l2 = geom::dot(ab, ab);
                            let t = if l2 < 1e-12 {
                                0.0
                            } else {
                                (geom::dot(geom::sub(p, a), ab) / l2).clamp(0.0, 1.0)
                            };
                            let d = geom::dist(p, geom::add(a, geom::scale(ab, t)));
                            let better = match best {
                                None => true,
                                Some((bd, be, bk, _)) => d < bd || (d == bd && (e, k) < (be, bk)),
                            };
                            if better {
                                best = Some((d, e, k, t));
                            }
                        }
                    }
                    // Interior rows only visit the ring's two side columns.
                    x += if on_row_edge || r == 0 || x == cx + r {
                        1
                    } else {
                        2 * r
                    };
                }
            }
            // Everything not yet visited is outside the box of rings 0..=r.
            let bx0 = self.origin[0] + (cx - r) as f32 * EDGE_CELL_M;
            let by0 = self.origin[1] + (cy - r) as f32 * EDGE_CELL_M;
            let bx1 = self.origin[0] + (cx + r + 1) as f32 * EDGE_CELL_M;
            let by1 = self.origin[1] + (cy + r + 1) as f32 * EDGE_CELL_M;
            let covered =
                cx - r <= 0 && cy - r <= 0 && cx + r >= self.nx - 1 && cy + r >= self.ny - 1;
            let inner = (p[0] - bx0).min(bx1 - p[0]).min(p[1] - by0).min(by1 - p[1]);
            if covered || best.is_some_and(|b| b.0 <= inner) {
                break;
            }
        }
        let (d, e, k, t) = best?;
        let cum = &self.cum[e as usize];
        let len = cum[cum.len() - 1];
        let pl = &edges[e as usize].polyline;
        let (a, b) = (pl[k as usize], pl[k as usize + 1]);
        let q = geom::add(a, geom::scale(geom::sub(b, a), t));
        let s = cum[k as usize] + (cum[k as usize + 1] - cum[k as usize]) * t;
        let frac = if len > 0.0 {
            (s / len).clamp(0.0, 1.0)
        } else {
            0.0
        };
        Some((e, frac, q, d))
    }
}

impl<'a> RoadNetworkIndex<'a> {
    /// Index `map`'s road graph.
    pub fn new(map: &'a CityMap) -> Self {
        let view = RoadGraphView::from_map(map);
        let mut out = vec![Vec::new(); view.nodes.len()];
        for (i, e) in view.edges.iter().enumerate() {
            out[e.a as usize].push(i as u32);
            if !e.oneway() {
                out[e.b as usize].push(i as u32);
            }
        }
        for v in &mut out {
            v.sort_unstable();
            v.dedup();
        }
        let mut pois: Vec<wp_contracts::PoiInfo> = view
            .pois
            .iter()
            .map(|p| {
                use crate::network::PoiKind as W;
                use wp_contracts::PoiKind as C;
                let (kind, radius_m) = match p.kind {
                    W::SafehouseCandidate => (C::Safehouse, 15.0),
                    W::Garage => (C::Garage, 15.0),
                    W::RaceStartGrid => (C::RaceStartGrid, 15.0),
                    W::PoliceStation => (C::PoliceStation, 15.0),
                    W::Helipad => (C::Helipad, 15.0),
                };
                wp_contracts::PoiInfo {
                    kind,
                    pos: p.pos,
                    heading_rad: p.heading_rad,
                    radius_m,
                }
            })
            .collect();
        // Traffic cameras: every third signalised junction (the same rule `wp_police` uses).
        for (k, n) in view.nodes.iter().filter(|n| n.signalised).enumerate() {
            if k.is_multiple_of(3) {
                pois.push(wp_contracts::PoiInfo {
                    kind: wp_contracts::PoiKind::Camera,
                    pos: [n.pos[0], n.pos[1], 0.0],
                    heading_rad: 0.0,
                    radius_m: 60.0,
                });
            }
        }
        let near = EdgeGrid::new(&view.edges);
        Self {
            map,
            view,
            out,
            pois,
            near,
        }
    }

    /// The underlying view.
    pub fn view(&self) -> &RoadGraphView {
        &self.view
    }
}

impl wp_contracts::RoadNetworkQuery for RoadNetworkIndex<'_> {
    fn node_count(&self) -> u32 {
        self.view.nodes.len() as u32
    }

    fn node_pos(&self, n: u32) -> [f32; 2] {
        self.view.nodes[n as usize].pos
    }

    fn edge_count(&self) -> u32 {
        self.view.edges.len() as u32
    }

    fn edge(&self, e: u32) -> wp_contracts::RoadEdgeInfo {
        let r = &self.view.edges[e as usize];
        wp_contracts::RoadEdgeInfo {
            a: r.a,
            b: r.b,
            length_m: r.length_m,
            speed_limit_mps: r.speed_limit_mps,
            class: r.class.into(),
            lanes_ab: r.lanes_ab,
            lanes_ba: r.lanes_ba,
            district: r.district,
        }
    }

    fn edge_polyline(&self, e: u32) -> &[[f32; 2]] {
        &self.view.edges[e as usize].polyline
    }

    fn edges_from(&self, n: u32) -> &[u32] {
        &self.out[n as usize]
    }

    fn nearest_on_road(&self, p: [f32; 2]) -> Option<(u32, f32, [f32; 2], f32)> {
        self.near.nearest(&self.view.edges, p)
    }

    fn district_at(&self, p: [f32; 2]) -> Option<wp_contracts::DistrictId> {
        self.map.district_at(p)
    }

    fn pois(&self) -> &[wp_contracts::PoiInfo] {
        &self.pois
    }
}
