//! Build the police [`RoadGraph`] from `wp_world`'s city (`CityMap` / `RoadNetwork`), so the
//! police drive, route, predict and place tools on the same roads as traffic (lead feedback,
//! run 2: "use machine D's road network").
//!
//! Mapping:
//! - Every network node becomes a graph node; curved lanes add intermediate nodes for their
//!   interior polyline points (after dropping points within [`SIMPLIFY_M`] of the chord).
//! - One representative drivable lane per road and direction (bus and parking lanes are
//!   skipped). A straight road driven both ways becomes one two-way segment; otherwise each
//!   direction is its own one-way chain (lane-legal routing).
//! - Class: `LaneKind::Highway` → `Highway`; limit ≥ 15 m/s → `Arterial`; else `Local`.
//! - District from the zone under the node (`DistrictKind` → police `District`, see
//!   [`district_of`]). Off-zone nodes count as Suburb.
//! - POIs: `SafehouseCandidate` → safehouse, `Helipad` → helipad, `PoliceStation` → station.
//!   Traffic cameras sit at every third signalised intersection (the network has none).
//! - No occluders: line of sight comes from `WorldQuery::line_of_sight` (building footprints).
//!
//! Turn restrictions (`Lane::connections`) are not modelled: any legal outgoing road may be
//! taken at a node. Until the shared road-network trait lands (contract v0.3.0) this
//! conversion is the bridge.

use std::collections::BTreeMap;

use wp_world::network::{DistrictKind, IntersectionControl, LaneKind, PoiKind as WorldPoi};
use wp_world::CityMap;

use crate::graph::{
    closest_on_segment, dist, District, NodeId, Poi, PoiKind, RoadClass, RoadGraph,
};

/// Interior polyline points closer than this to the chord are dropped, m.
pub const SIMPLIFY_M: f32 = 1.5;

/// Police district for a world district kind.
pub fn district_of(kind: DistrictKind) -> District {
    match kind {
        DistrictKind::Core | DistrictKind::Commercial => District::Core,
        DistrictKind::Residential | DistrictKind::Suburb => District::Suburb,
        DistrictKind::Industrial | DistrictKind::Port => District::Industrial,
    }
}

fn xy(p: [f32; 3]) -> [f32; 2] {
    [p[0], p[1]]
}

/// Drop interior points within `tol` of the chord (Douglas–Peucker, iterative).
fn simplify(pts: &[[f32; 2]], tol: f32) -> Vec<[f32; 2]> {
    if pts.len() <= 2 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0usize, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let mut best = (0.0f32, 0usize);
        for (i, p) in pts.iter().enumerate().take(b).skip(a + 1) {
            let (_, q) = closest_on_segment(pts[a], pts[b], *p);
            let d = dist(q, *p);
            if d > best.0 {
                best = (d, i);
            }
        }
        if best.0 > tol {
            keep[best.1] = true;
            stack.push((a, best.1));
            stack.push((best.1, b));
        }
    }
    pts.iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(*p))
        .collect()
}

fn class_of(kind: LaneKind, limit: f32) -> RoadClass {
    if kind == LaneKind::Highway {
        RoadClass::Highway
    } else if limit >= 15.0 {
        RoadClass::Arterial
    } else {
        RoadClass::Local
    }
}

impl RoadGraph {
    /// Convert a `wp_world` city map (see the module docs for the mapping).
    pub fn from_city(map: &CityMap) -> RoadGraph {
        let net = &map.net;
        let district_at = |p: [f32; 2]| {
            map.district_at(p)
                .map(district_of)
                .unwrap_or(District::Suburb)
        };
        let mut g = RoadGraph::new();
        for n in &net.nodes {
            let p = xy(n.pos);
            g.add_node(p, district_at(p));
        }
        // One representative lane per (road, from, to): prefer through lanes, then the
        // leftmost (lowest index), then the lowest lane id.
        let mut reps: BTreeMap<(u32, u32, u32), (u8, u8, usize)> = BTreeMap::new();
        for (i, l) in net.lanes.iter().enumerate() {
            if !l.kind.drivable() || l.from_node == l.to_node {
                continue;
            }
            let rank = (l.kind != LaneKind::Through && l.kind != LaneKind::Highway) as u8;
            let key = (l.road, l.from_node, l.to_node);
            let cand = (rank, l.index_from_left, i);
            if reps.get(&key).is_none_or(|cur| cand < *cur) {
                reps.insert(key, cand);
            }
        }
        // Straight lanes keyed by unordered node pair, to merge two-way roads.
        let mut straight: BTreeMap<(u32, u32, u32), (usize, bool)> = BTreeMap::new();
        let mut curved: Vec<usize> = Vec::new();
        for &(_, _, i) in reps.values() {
            let l = &net.lanes[i];
            let mut pts: Vec<[f32; 2]> = vec![xy(net.nodes[l.from_node as usize].pos)];
            pts.extend(
                l.polyline
                    .iter()
                    .skip(1)
                    .take(l.polyline.len().saturating_sub(2))
                    .map(|p| xy(*p)),
            );
            pts.push(xy(net.nodes[l.to_node as usize].pos));
            if simplify(&pts, SIMPLIFY_M).len() == 2 {
                let key = (
                    l.road,
                    l.from_node.min(l.to_node),
                    l.from_node.max(l.to_node),
                );
                match straight.get_mut(&key) {
                    Some(e) => e.1 = true,
                    None => {
                        straight.insert(key, (i, false));
                    }
                }
            } else {
                curved.push(i);
            }
        }
        for &(i, both_ways) in straight.values() {
            let l = &net.lanes[i];
            g.add_segment(
                NodeId(l.from_node),
                NodeId(l.to_node),
                l.speed_limit_mps,
                class_of(l.kind, l.speed_limit_mps),
                !both_ways,
            );
        }
        for i in curved {
            let l = &net.lanes[i];
            let mut pts: Vec<[f32; 2]> = vec![xy(net.nodes[l.from_node as usize].pos)];
            pts.extend(
                l.polyline
                    .iter()
                    .skip(1)
                    .take(l.polyline.len().saturating_sub(2))
                    .map(|p| xy(*p)),
            );
            pts.push(xy(net.nodes[l.to_node as usize].pos));
            let pts = simplify(&pts, SIMPLIFY_M);
            let class = class_of(l.kind, l.speed_limit_mps);
            let mut prev = NodeId(l.from_node);
            for (k, p) in pts.iter().enumerate().skip(1) {
                let next = if k == pts.len() - 1 {
                    NodeId(l.to_node)
                } else {
                    g.add_node(*p, district_at(*p))
                };
                g.add_segment(prev, next, l.speed_limit_mps, class, true);
                prev = next;
            }
        }
        for p in &net.pois {
            let kind = match p.kind {
                WorldPoi::SafehouseCandidate => PoiKind::Safehouse,
                WorldPoi::Helipad => PoiKind::Helipad,
                WorldPoi::PoliceStation => PoiKind::PoliceStation,
                WorldPoi::Garage | WorldPoi::RaceStartGrid => continue,
            };
            g.pois.push(Poi {
                kind,
                pos: xy(p.pos),
                radius_m: 15.0,
            });
        }
        let mut signalised = 0usize;
        for ix in &net.intersections {
            if matches!(ix.control, IntersectionControl::Signal { .. }) {
                if signalised.is_multiple_of(3) {
                    g.pois.push(Poi {
                        kind: PoiKind::Camera,
                        pos: xy(net.nodes[ix.node as usize].pos),
                        radius_m: 60.0,
                    });
                }
                signalised += 1;
            }
        }
        g
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplify_keeps_corners_drops_noise() {
        let pts = [[0.0, 0.0], [10.0, 0.5], [20.0, 0.0], [20.0, 20.0]];
        let s = simplify(&pts, 1.5);
        assert_eq!(s, vec![[0.0, 0.0], [20.0, 0.0], [20.0, 20.0]]);
    }

    #[test]
    fn districts_map_onto_police_tables() {
        assert_eq!(district_of(DistrictKind::Commercial), District::Core);
        assert_eq!(district_of(DistrictKind::Port), District::Industrial);
        assert_eq!(district_of(DistrictKind::Residential), District::Suburb);
    }
}
