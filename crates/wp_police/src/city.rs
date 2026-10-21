//! The police road graph comes from the city's contract road network
//! ([`wp_contracts::RoadNetworkQuery`], v0.3.0; implemented by D's
//! `wp_world::roads::RoadNetworkIndex`). [`RoadGraph::from_network`] maps it 1:1 onto police
//! nodes and segments. `RoadGraph` is not a second road model: it is the police's routing
//! index over the shared network (legal routing, predicted paths, approach sight distance,
//! nearest-road lookup), rebuilt from the trait and never edited.
//!
//! Mapping:
//! - Node `i` = network node `i` (district from `district_at` its position; none = Suburb).
//! - Edge `e` → one segment (`RoadSegment::edge = Some(e)`), or a chain of segments through
//!   the edge's interior polyline points for curved roads. `lanes_ba == 0` → one-way `a → b`;
//!   `lanes_ab == 0` → one-way `b → a`.
//! - Class: Highway → Highway, Arterial → Arterial, Collector / Local / Alley → Local.
//! - Districts: Core/Commercial → Core, Residential/Suburb → Suburb, Industrial/Port →
//!   Industrial (the presence tables are keyed by these three).
//! - POIs: `Safehouse`, `Helipad`, `PoliceStation`, `Camera` (the network places cameras).
//! - Signalised junctions: the trait has no flag for them yet (contract request #7 in
//!   `docs/integration/police.md`), so [`RoadGraph::from_network`] takes them as a list;
//!   [`RoadGraph::from_city`] reads them from the city map.
//! - No occluders: line of sight is `WorldQuery::line_of_sight` (exact building footprints).
//!
//! Turn restrictions (`Lane::connections`) are not modelled: any legal outgoing road may be
//! taken at a node.

use wp_contracts::{DistrictId, PoiKind as NetPoi, RoadClass as NetClass, RoadNetworkQuery};
pub use wp_world::procgen::ProcGenConfig;
use wp_world::{CityMap, RoadNetworkIndex};

use crate::graph::{District, NodeId, Poi, PoiKind, RoadClass, RoadGraph};

/// Police district for a network district.
pub fn district_of(kind: Option<DistrictId>) -> District {
    match kind {
        Some(DistrictId::Core | DistrictId::Commercial) => District::Core,
        Some(DistrictId::Industrial | DistrictId::Port) => District::Industrial,
        Some(DistrictId::Residential | DistrictId::Suburb) | None => District::Suburb,
    }
}

/// Police road class for a network road class.
pub fn class_of(class: NetClass) -> RoadClass {
    match class {
        NetClass::Highway => RoadClass::Highway,
        NetClass::Arterial => RoadClass::Arterial,
        NetClass::Collector | NetClass::Local | NetClass::Alley => RoadClass::Local,
    }
}

/// Lane width of a road class, m (`wp_world::builder::BuildOptions` defaults: the contract
/// road network does not carry widths).
pub fn lane_width_of(class: NetClass) -> f32 {
    match class {
        NetClass::Highway => 3.7,
        NetClass::Alley => 3.0,
        NetClass::Arterial | NetClass::Collector | NetClass::Local => 3.5,
    }
}

impl RoadGraph {
    /// Build the police routing index over a contract road network (see the module docs).
    /// `signalised` lists the network nodes with traffic lights (any order; unknown ids are
    /// ignored).
    pub fn from_network(net: &dyn RoadNetworkQuery, signalised: &[u32]) -> RoadGraph {
        let node_count = net.node_count() as usize;
        let mut g = RoadGraph::new();
        for n in 0..node_count {
            let pos = net.node_pos(n as u32);
            g.add_node(pos, district_of(net.district_at(pos)));
        }
        let mut degree = vec![0u32; node_count];
        for e in 0..net.edge_count() {
            let edge = net.edge(e);
            if edge.lanes_ab == 0 && edge.lanes_ba == 0 {
                continue;
            }
            degree[edge.a as usize] += 1;
            degree[edge.b as usize] += 1;
            // Travel direction for one-way roads; two-way roads keep a → b.
            let reversed = edge.lanes_ab == 0;
            let oneway = edge.lanes_ab == 0 || edge.lanes_ba == 0;
            let mut pts = net.edge_polyline(e).to_vec();
            let (mut from, to) = if reversed {
                pts.reverse();
                (NodeId(edge.b), NodeId(edge.a))
            } else {
                (NodeId(edge.a), NodeId(edge.b))
            };
            let class = class_of(edge.class);
            let district = district_of(edge.district);
            let n = pts.len();
            for (k, p) in pts.iter().enumerate().skip(1) {
                let next = if k == n - 1 {
                    to
                } else {
                    g.add_node(*p, district)
                };
                let s = g.add_segment(from, next, edge.speed_limit_mps, class, oneway);
                let seg = &mut g.segments[s.0 as usize];
                seg.edge = Some(e);
                (seg.lanes_fwd, seg.lanes_back) = if reversed {
                    (edge.lanes_ba, edge.lanes_ab)
                } else {
                    (edge.lanes_ab, edge.lanes_ba)
                };
                seg.lane_width_m = lane_width_of(edge.class);
                from = next;
            }
        }
        let mut is_signal = vec![false; node_count];
        for &n in signalised {
            if let Some(f) = is_signal.get_mut(n as usize) {
                *f = true;
            }
        }
        for i in 0..node_count {
            g.nodes[i].junction = degree[i] >= 3 || is_signal[i];
            g.nodes[i].signalised = is_signal[i];
        }
        for p in net.pois() {
            let kind = match p.kind {
                NetPoi::Safehouse => PoiKind::Safehouse,
                NetPoi::Helipad => PoiKind::Helipad,
                NetPoi::PoliceStation => PoiKind::PoliceStation,
                NetPoi::Camera => PoiKind::Camera,
                NetPoi::Garage | NetPoi::RaceStartGrid => continue,
            };
            g.pois.push(Poi {
                kind,
                pos: [p.pos[0], p.pos[1]],
                radius_m: p.radius_m,
            });
        }
        g
    }

    /// Build from a `wp_world` city map: its contract road network
    /// ([`RoadNetworkIndex`]) plus the map's signalised junctions.
    pub fn from_city(map: &CityMap) -> RoadGraph {
        let net = RoadNetworkIndex::new(map);
        let signalised: Vec<u32> = net
            .view()
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.signalised)
            .map(|(i, _)| i as u32)
            .collect();
        Self::from_network(&net, &signalised)
    }
}

/// D's procedural city and its police graph (tests, scenarios, tools).
pub fn procedural(cfg: &ProcGenConfig) -> (RoadGraph, CityMap) {
    let map = CityMap::new(wp_world::procgen::generate(cfg));
    (RoadGraph::from_city(&map), map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn districts_and_classes_map_onto_police_tables() {
        assert_eq!(district_of(Some(DistrictId::Commercial)), District::Core);
        assert_eq!(district_of(Some(DistrictId::Port)), District::Industrial);
        assert_eq!(district_of(None), District::Suburb);
        assert_eq!(class_of(NetClass::Collector), RoadClass::Local);
    }

    #[test]
    fn network_maps_one_to_one() {
        let map = CityMap::new(wp_world::procgen::generate(&ProcGenConfig::small()));
        let net = RoadNetworkIndex::new(&map);
        let g = RoadGraph::from_city(&map);
        assert!(g.nodes.len() >= net.node_count() as usize);
        for e in 0..net.edge_count() {
            let edge = net.edge(e);
            let segs: Vec<_> = g.segments.iter().filter(|s| s.edge == Some(e)).collect();
            assert!(!segs.is_empty(), "edge {e} lost");
            let len: f32 = segs.iter().map(|s| s.length_m).sum();
            assert!(
                (len - edge.length_m).abs() < 0.5,
                "edge {e}: {len} vs {}",
                edge.length_m
            );
        }
        let signals = g.nodes.iter().filter(|n| n.signalised).count();
        assert!(signals > 0, "no signalised junctions");
        let cameras = g.pois.iter().filter(|p| p.kind == PoiKind::Camera).count();
        assert!(cameras > 0, "no cameras");
    }

    /// Any `RoadNetworkQuery` works, not just D's index: a hand-built one-way L.
    struct Tiny {
        nodes: Vec<[f32; 2]>,
        lines: Vec<Vec<[f32; 2]>>,
        out: Vec<Vec<u32>>,
    }

    impl RoadNetworkQuery for Tiny {
        fn node_count(&self) -> u32 {
            self.nodes.len() as u32
        }
        fn node_pos(&self, n: u32) -> [f32; 2] {
            self.nodes[n as usize]
        }
        fn edge_count(&self) -> u32 {
            2
        }
        fn edge(&self, e: u32) -> wp_contracts::RoadEdgeInfo {
            wp_contracts::RoadEdgeInfo {
                a: e,
                b: e + 1,
                length_m: 100.0,
                speed_limit_mps: 13.9,
                class: NetClass::Arterial,
                lanes_ab: if e == 0 { 1 } else { 0 },
                lanes_ba: 1,
                district: Some(DistrictId::Port),
            }
        }
        fn edge_polyline(&self, e: u32) -> &[[f32; 2]] {
            &self.lines[e as usize]
        }
        fn edges_from(&self, n: u32) -> &[u32] {
            &self.out[n as usize]
        }
        fn nearest_on_road(&self, _p: [f32; 2]) -> Option<(u32, f32, [f32; 2], f32)> {
            None
        }
        fn district_at(&self, _p: [f32; 2]) -> Option<DistrictId> {
            Some(DistrictId::Core)
        }
        fn pois(&self) -> &[wp_contracts::PoiInfo] {
            &[]
        }
    }

    #[test]
    fn any_network_query_builds_a_graph() {
        let nodes = vec![[0.0, 0.0], [0.0, 100.0], [100.0, 100.0]];
        let net = Tiny {
            lines: vec![vec![nodes[0], nodes[1]], vec![nodes[1], nodes[2]]],
            out: vec![vec![0], vec![0], vec![1]],
            nodes,
        };
        let g = RoadGraph::from_network(&net, &[1, 99]);
        assert_eq!(g.nodes.len(), 3);
        assert_eq!(g.segments.len(), 2);
        // Edge 1 is one-way b → a: stored reversed.
        let s1 = g.segments[1];
        assert!(s1.oneway);
        assert_eq!((s1.a, s1.b), (NodeId(2), NodeId(1)));
        assert_eq!(s1.edge, Some(1));
        assert_eq!(s1.class, RoadClass::Arterial);
        assert!(!g.segments[0].oneway);
        assert!(g.nodes[1].signalised && g.nodes[1].junction);
        assert!(!g.nodes[0].signalised);
        assert_eq!(g.nodes[0].district, District::Core);
    }
}
