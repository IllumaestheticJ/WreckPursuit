//! Race route helper: lane-legal paths between POIs or points, closed circuits through a
//! polygon's vertices, checkpoints, and closure markers (closed lanes + cones) for controlled
//! day events.

use std::cmp::Ordering;
use std::collections::{BTreeSet, BinaryHeap};

use crate::geom::{self, V2, V3};
use crate::map::{CityMap, SegId, SegKind};
use crate::network::{LaneId, TurnKind};
use crate::props::{cone, StaticProp};

/// A race checkpoint gate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Checkpoint {
    /// Order along the route.
    pub index: u32,
    /// Gate centre, m.
    pub pos: V3,
    /// Unit travel direction through the gate.
    pub dir: V2,
    /// Gate width, m (the carriageway width).
    pub width_m: f32,
    /// Distance along the route, m.
    pub distance_m: f32,
}

/// A lane-legal race route.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    /// Segments driven, in order (lanes and connectors).
    pub segs: Vec<SegId>,
    /// Lanes driven, in order.
    pub lanes: Vec<LaneId>,
    /// Centreline, m.
    pub polyline: Vec<V3>,
    /// Checkpoints (last one is the finish).
    pub checkpoints: Vec<Checkpoint>,
    /// Length, m.
    pub length_m: f32,
    /// Closed loop (circuit).
    pub closed_loop: bool,
}

/// Markers for a controlled event on a route.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RouteClosure {
    /// Lanes closed to civilian traffic (both directions of every road on the route).
    pub closed_lanes: BTreeSet<LaneId>,
    /// Cones across side-street approaches.
    pub cones: Vec<StaticProp>,
}

/// A route the world should treat as active (focus for spawning / physical LOD and optionally
/// closed to traffic).
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveRoute {
    /// The route.
    pub route: Route,
    /// Suppress civilian traffic on it and place cones.
    pub closed: bool,
}

/// Nearest drivable lane to `p` (optionally agreeing with `heading`): `(lane, s)`.
pub fn nearest_lane(map: &CityMap, p: V3, heading: Option<V2>) -> Option<(SegId, f32)> {
    nearest_lane_impl(map, p, heading, false)
}

/// [`nearest_lane`] for a point whose height is real (a POI on the ground): when `p` is not
/// on a lane, lanes more than [`SNAP_HEIGHT_M`] above or below it are skipped as long as
/// any lane at its height is left (a garage beside a viaduct snaps to the street it stands
/// on, not to the deck 8 m over it).
pub fn nearest_lane_at_height(map: &CityMap, p: V3, heading: Option<V2>) -> Option<(SegId, f32)> {
    nearest_lane_impl(map, p, heading, true).or_else(|| nearest_lane_impl(map, p, heading, false))
}

/// Height difference beyond which [`nearest_lane_at_height`] skips a lane, m (the same
/// tolerance as [`CityMap::locate`]).
pub const SNAP_HEIGHT_M: f32 = 4.0;

fn nearest_lane_impl(
    map: &CityMap,
    p: V3,
    heading: Option<V2>,
    match_height: bool,
) -> Option<(SegId, f32)> {
    if let Some(l) = map.locate(p, heading, 2.0, true, false) {
        return Some((l.seg, l.s));
    }
    let q = geom::xy(p);
    let mut best: Option<(f32, SegId, f32)> = None;
    for i in 0..map.n_lanes() {
        let s = &map.segs[i];
        if !s.drivable {
            continue;
        }
        let pr = s.project(q);
        let sc = pr.s.clamp(0.0, s.length);
        let (pt, tan) = s.point_at(sc);
        if let Some(h) = heading {
            if geom::dot(tan, h) < 0.3 {
                continue;
            }
        }
        if match_height && (pt[2] - p[2]).abs() > SNAP_HEIGHT_M {
            continue;
        }
        if best.is_none_or(|b| pr.dist < b.0) {
            best = Some((pr.dist, i as SegId, sc));
        }
    }
    best.map(|b| (b.1, b.2))
}

#[derive(PartialEq)]
struct Open {
    f: f32,
    seg: SegId,
}
impl Eq for Open {}
impl Ord for Open {
    fn cmp(&self, o: &Self) -> Ordering {
        o.f.total_cmp(&self.f).then(o.seg.cmp(&self.seg))
    }
}
impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// Fastest legal path (travel time at the limit) from lane position `from` to lane position
/// `to`, as a segment list starting with `from.0` and ending with `to.0`.
pub fn shortest_path(map: &CityMap, from: (SegId, f32), to: (SegId, f32)) -> Option<Vec<SegId>> {
    if from.0 == to.0 && to.1 >= from.1 {
        return Some(vec![from.0]);
    }
    let target = geom::xy(map.seg(to.0).point_at(to.1).0);
    let n = map.segs.len();
    let mut g = vec![f32::INFINITY; n];
    let mut came = vec![u32::MAX; n];
    let mut heap = BinaryHeap::new();
    let h = |s: SegId| geom::dist(geom::xy(map.seg(s).pts[0]), target) / 30.0;
    let start = map.seg(from.0);
    for &nx in &start.next {
        let cost = (start.length - from.1) / start.speed_limit;
        g[nx as usize] = cost;
        came[nx as usize] = from.0;
        heap.push(Open {
            f: cost + h(nx),
            seg: nx,
        });
    }
    while let Some(Open { seg, .. }) = heap.pop() {
        if seg == to.0 {
            let mut path = vec![seg];
            let mut cur = seg;
            while came[cur as usize] != u32::MAX && cur != from.0 {
                cur = came[cur as usize];
                path.push(cur);
                if path.len() > n {
                    return None;
                }
            }
            if *path.last().unwrap() != from.0 {
                path.push(from.0);
            }
            path.reverse();
            return Some(path);
        }
        let s = map.seg(seg);
        if !s.drivable {
            continue;
        }
        for &nx in &s.next {
            let cost = g[seg as usize] + s.length / s.speed_limit.max(1.0);
            if cost < g[nx as usize] {
                g[nx as usize] = cost;
                came[nx as usize] = seg;
                heap.push(Open {
                    f: cost + h(nx),
                    seg: nx,
                });
            }
        }
    }
    None
}

fn build_route(
    map: &CityMap,
    segs: Vec<SegId>,
    s_start: f32,
    s_end: f32,
    closed_loop: bool,
) -> Route {
    let mut polyline: Vec<V3> = Vec::new();
    let mut lanes = Vec::new();
    let mut turn_marks: Vec<usize> = Vec::new();
    let last = segs.len() - 1;
    for (k, &sid) in segs.iter().enumerate() {
        let s = map.seg(sid);
        let a = if k == 0 { s_start } else { 0.0 };
        let b = if k == last { s_end } else { s.length };
        if let Some(l) = s.lane() {
            lanes.push(l);
        }
        let mut pts = vec![s.point_at(a).0];
        for (i, p) in s.pts.iter().enumerate() {
            if s.cum[i] > a + 0.01 && s.cum[i] < b - 0.01 {
                pts.push(*p);
            }
        }
        pts.push(s.point_at(b).0);
        for p in pts {
            if polyline
                .last()
                .is_none_or(|q| geom::dist(geom::xy(*q), geom::xy(p)) > 0.05)
            {
                polyline.push(p);
            }
        }
        if let SegKind::Connector { turn, .. } = s.kind {
            if turn != TurnKind::Straight {
                turn_marks.push(polyline.len() - 1);
            }
        }
    }
    let cum = geom::cumulative(&polyline);
    let length = *cum.last().unwrap_or(&0.0);
    let road_width = |p: V3| -> f32 {
        map.locate(p, None, 2.0, true, false)
            .and_then(|l| map.seg(l.seg).lane())
            .map(|l| {
                let lane = &map.net.lanes[l as usize];
                let mut n = 1.0;
                let mut cur = lane.left;
                while let Some(c) = cur {
                    n += 1.0;
                    cur = map.net.lanes[c as usize].left;
                }
                let mut cur = lane.right;
                while let Some(c) = cur {
                    n += 1.0;
                    cur = map.net.lanes[c as usize].right;
                }
                n * lane.width_m
            })
            .unwrap_or(8.0)
    };
    let mut marks: Vec<f32> = turn_marks.iter().map(|i| cum[*i]).collect();
    let mut d = 200.0;
    while d < length - 50.0 {
        marks.push(d);
        d += 200.0;
    }
    marks.push(length);
    marks.sort_by(|a, b| a.total_cmp(b));
    marks.dedup_by(|a, b| (*a - *b).abs() < 40.0);
    if let Some(l) = marks.last_mut() {
        *l = length;
    }
    let checkpoints = marks
        .iter()
        .enumerate()
        .map(|(i, &dm)| {
            let (pos, dir) = geom::point_at(&polyline, &cum, dm);
            Checkpoint {
                index: i as u32,
                pos,
                dir,
                width_m: road_width(pos),
                distance_m: dm,
            }
        })
        .collect();
    Route {
        segs,
        lanes,
        polyline,
        checkpoints,
        length_m: length,
        closed_loop,
    }
}

/// Lane-legal route between two points (start snapped to a lane agreeing with
/// `start_heading` if given).
pub fn plan_route(
    map: &CityMap,
    start: V3,
    start_heading: Option<V2>,
    finish: V3,
) -> Option<Route> {
    let a = nearest_lane(map, start, start_heading)?;
    let b = nearest_lane(map, finish, None)?;
    let segs = shortest_path(map, a, b)?;
    Some(build_route(map, segs, a.1, b.1, false))
}

/// Route between two POIs (indices into the network's POI list); starts facing the POI
/// heading.
pub fn plan_route_between_pois(
    map: &CityMap,
    start_poi: usize,
    finish_poi: usize,
) -> Option<Route> {
    let a = map.net.pois.get(start_poi)?;
    let b = map.net.pois.get(finish_poi)?;
    // POI heights are real ground heights: snap to a lane at that height (the grid's Garage 3
    // stands mid-block beside the highway viaduct; snapping in 2D only sent its routes up
    // onto the deck and round a 2 km detour).
    let sa = nearest_lane_at_height(map, a.pos, Some(geom::dir_of_yaw(a.heading_rad)))?;
    let sb = nearest_lane_at_height(map, b.pos, None)?;
    let segs = shortest_path(map, sa, sb)?;
    Some(build_route(map, segs, sa.1, sb.1, false))
}

/// `next` moved onto `cur`'s lane when it is a parallel lane of the same road and direction
/// (routes do not change lanes mid-road: a waypoint on the neighbouring lane of a wide road,
/// e.g. Loop 9's three lanes beside its median, would otherwise be unreachable).
fn same_lane_group(map: &CityMap, cur: (SegId, f32), next: (SegId, f32), p: V3) -> (SegId, f32) {
    let (SegKind::Lane { lane: a }, SegKind::Lane { lane: b }) =
        (map.seg(cur.0).kind, map.seg(next.0).kind)
    else {
        return next;
    };
    if a == b {
        return next;
    }
    let lanes = &map.net.lanes;
    let mut l = Some(a);
    while let Some(x) = l {
        l = lanes[x as usize].left;
        if l == Some(b) {
            break;
        }
    }
    let mut r = Some(a);
    while let Some(x) = r {
        r = lanes[x as usize].right;
        if r == Some(b) {
            break;
        }
    }
    if l == Some(b) || r == Some(b) {
        let s = map.seg(cur.0);
        let pr = s.project(geom::xy(p));
        return (cur.0, pr.s.clamp(0.0, s.length));
    }
    next
}

/// The other lanes of `seg`'s road in its direction (left and right neighbours, nearest
/// first), as lane segments; empty for a connector.
fn parallel_lanes(map: &CityMap, seg: SegId) -> Vec<SegId> {
    let SegKind::Lane { lane } = map.seg(seg).kind else {
        return Vec::new();
    };
    let lanes = &map.net.lanes;
    let mut out = Vec::new();
    for side in [false, true] {
        let mut l = Some(lane);
        while let Some(x) = l {
            l = if side {
                lanes[x as usize].right
            } else {
                lanes[x as usize].left
            };
            if let Some(y) = l {
                out.push(y as SegId);
            }
        }
    }
    out
}

/// Closed circuit visiting the polygon vertices in order and returning to the first.
pub fn plan_circuit(map: &CityMap, polygon: &[V2]) -> Option<Route> {
    if polygon.len() < 2 {
        return None;
    }
    let n = polygon.len();
    let pos = |i: usize| -> V3 { [polygon[i % n][0], polygon[i % n][1], 0.0] };
    let heading = |i: usize| geom::normalize(geom::sub(polygon[(i + 1) % n], polygon[i % n]));
    let first = nearest_lane(map, pos(0), Some(heading(0)))?;
    let leg = |cur: (SegId, f32), next: (SegId, f32)| -> Option<Vec<SegId>> {
        if cur.0 == next.0 && next.1 < cur.1 {
            // Same lane but behind us: go round the block.
            let s = map.seg(cur.0);
            let via = *s.next.first()?;
            let mut p = vec![cur.0];
            p.extend(shortest_path(map, (via, 0.0), next)?);
            Some(p)
        } else {
            shortest_path(map, cur, next)
        }
    };
    let mut segs: Vec<SegId> = Vec::new();
    let mut cur = first;
    for i in 1..=n {
        let mut next = if i == n {
            first
        } else {
            same_lane_group(
                map,
                cur,
                nearest_lane(map, pos(i), Some(heading(i)))?,
                pos(i),
            )
        };
        let mut part = match leg(cur, next) {
            Some(p) => p,
            // The lane it snapped to cannot be reached without a lane change (a divided
            // highway's carriageways run lane-locked from piece to piece: the rightmost lane
            // only ever leads to the next rightmost lane): the reachable lane beside it with
            // the shortest path.
            None if i < n => {
                let (alt, p) = parallel_lanes(map, next.0)
                    .into_iter()
                    .filter_map(|l| {
                        let s = map.seg(l);
                        let at = (l, s.project(geom::xy(pos(i))).s.clamp(0.0, s.length));
                        leg(cur, at).map(|p| (at, p))
                    })
                    .min_by(|a, b| {
                        let len = |p: &[SegId]| p.iter().map(|x| map.seg(*x).length).sum::<f32>();
                        len(&a.1).total_cmp(&len(&b.1)).then(a.0 .0.cmp(&b.0 .0))
                    })?;
                next = alt;
                p
            }
            None => return None,
        };
        if !segs.is_empty() {
            part.remove(0);
        }
        segs.extend(part);
        cur = next;
    }
    Some(build_route(map, segs, first.1, first.1, true))
}

/// Closure markers for a controlled event: every road on the route is closed in both
/// directions and each side-street approach to a junction on the route gets three cones
/// across its stop line.
pub fn closure_for_route(map: &CityMap, route: &Route) -> RouteClosure {
    let mut roads: BTreeSet<u32> = BTreeSet::new();
    for l in &route.lanes {
        roads.insert(map.net.lanes[*l as usize].road);
    }
    let mut closed_lanes = BTreeSet::new();
    for (i, l) in map.net.lanes.iter().enumerate() {
        if roads.contains(&l.road) {
            closed_lanes.insert(i as LaneId);
        }
    }
    let mut nodes: BTreeSet<u32> = BTreeSet::new();
    for &s in &route.segs {
        if let SegKind::Connector { node, .. } = map.seg(s).kind {
            nodes.insert(node);
        }
    }
    let mut cones = Vec::new();
    for node in nodes {
        let Some(ix) = map.net.nodes[node as usize].intersection else {
            continue;
        };
        for inc in &map.net.intersections[ix as usize].incoming {
            if closed_lanes.contains(&inc.lane) {
                continue;
            }
            let seg = map.seg(inc.lane);
            let (p, t) = seg.point_at(seg.length);
            let r = geom::right_of(t);
            let w = seg.width;
            for k in [-1.0f32, 0.0, 1.0] {
                let o = k * w / 3.0;
                cones.push(cone(
                    [p[0] + r[0] * o + t[0], p[1] + r[1] * o + t[1], p[2]],
                    geom::yaw_of(t),
                ));
            }
        }
    }
    RouteClosure {
        closed_lanes,
        cones,
    }
}
