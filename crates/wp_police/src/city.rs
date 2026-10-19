//! The police road graph comes from `wp_world`'s city: [`RoadGraph::from_view`] maps D's
//! road-level view (`wp_world::roads::RoadGraphView`, the proposed v0.3.0 shared road-network
//! shape) 1:1 onto police nodes and segments. `RoadGraph` keeps only what the police compute on
//! top of it: legal routing, predicted paths, approach sight distance and nearest-road lookup.
//!
//! Mapping:
//! - Node `i` = network node `i` (district from the zone under it; off-zone = Suburb).
//! - Edge `e` → one segment (`RoadSegment::edge = Some(e)`), or a chain of segments through
//!   the edge's interior polyline points for curved roads. `lanes_ba == 0` → one-way `a → b`;
//!   `lanes_ab == 0` → one-way `b → a`.
//! - Class: Highway → Highway, Arterial → Arterial, Collector / Local / Alley → Local.
//! - District kinds: Core/Commercial → Core, Residential/Suburb → Suburb, Industrial/Port →
//!   Industrial (the presence tables are keyed by these three).
//! - POIs: `SafehouseCandidate` → safehouse, `Helipad`, `PoliceStation`. Traffic cameras sit
//!   at every third signalised node (the network has none).
//! - No occluders: line of sight is `WorldQuery::line_of_sight` (exact building footprints).
//!
//! Turn restrictions (`Lane::connections`) are not modelled: any legal outgoing road may be
//! taken at a node.

use wp_world::builder::RoadClass as WorldClass;
use wp_world::network::{DistrictKind, PoiKind as WorldPoi};
pub use wp_world::procgen::ProcGenConfig;
use wp_world::roads::RoadGraphView;
use wp_world::CityMap;

use crate::graph::{District, NodeId, Poi, PoiKind, RoadClass, RoadGraph};

/// Police district for a world district kind.
pub fn district_of(kind: Option<DistrictKind>) -> District {
    match kind {
        Some(DistrictKind::Core | DistrictKind::Commercial) => District::Core,
        Some(DistrictKind::Industrial | DistrictKind::Port) => District::Industrial,
        Some(DistrictKind::Residential | DistrictKind::Suburb) | None => District::Suburb,
    }
}

/// Police road class for a world road class.
pub fn class_of(class: WorldClass) -> RoadClass {
    match class {
        WorldClass::Highway => RoadClass::Highway,
        WorldClass::Arterial => RoadClass::Arterial,
        WorldClass::Collector | WorldClass::Local | WorldClass::Alley => RoadClass::Local,
    }
}

impl RoadGraph {
    /// Build from D's road-level view (see the module docs for the mapping).
    pub fn from_view(view: &RoadGraphView) -> RoadGraph {
        let mut g = RoadGraph::new();
        for n in &view.nodes {
            g.add_node(n.pos, district_of(n.district));
        }
        for (e, edge) in view.edges.iter().enumerate() {
            if edge.lanes_ab == 0 && edge.lanes_ba == 0 {
                continue;
            }
            // Travel direction for one-way roads; two-way roads keep a → b.
            let reversed = edge.lanes_ab == 0;
            let oneway = edge.lanes_ab == 0 || edge.lanes_ba == 0;
            let mut pts = edge.polyline.clone();
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
                g.segments[s.0 as usize].edge = Some(e as u32);
                from = next;
            }
        }
        for p in &view.pois {
            let kind = match p.kind {
                WorldPoi::SafehouseCandidate => PoiKind::Safehouse,
                WorldPoi::Helipad => PoiKind::Helipad,
                WorldPoi::PoliceStation => PoiKind::PoliceStation,
                WorldPoi::Garage | WorldPoi::RaceStartGrid => continue,
            };
            g.pois.push(Poi {
                kind,
                pos: [p.pos[0], p.pos[1]],
                radius_m: 15.0,
            });
        }
        for (k, n) in view.nodes.iter().filter(|n| n.signalised).enumerate() {
            if k.is_multiple_of(3) {
                g.pois.push(Poi {
                    kind: PoiKind::Camera,
                    pos: n.pos,
                    radius_m: 60.0,
                });
            }
        }
        g
    }

    /// Build from a `wp_world` city map (via its road-level view).
    pub fn from_city(map: &CityMap) -> RoadGraph {
        Self::from_view(&RoadGraphView::from_map(map))
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
        assert_eq!(district_of(Some(DistrictKind::Commercial)), District::Core);
        assert_eq!(district_of(Some(DistrictKind::Port)), District::Industrial);
        assert_eq!(district_of(None), District::Suburb);
        assert_eq!(class_of(WorldClass::Collector), RoadClass::Local);
    }

    #[test]
    fn view_maps_one_to_one() {
        let map = CityMap::new(wp_world::procgen::generate(&ProcGenConfig::small()));
        let view = RoadGraphView::from_map(&map);
        let g = RoadGraph::from_view(&view);
        assert!(g.nodes.len() >= view.nodes.len());
        for (e, edge) in view.edges.iter().enumerate() {
            let segs: Vec<_> = g
                .segments
                .iter()
                .filter(|s| s.edge == Some(e as u32))
                .collect();
            assert!(!segs.is_empty(), "edge {e} lost");
            let len: f32 = segs.iter().map(|s| s.length_m).sum();
            assert!(
                (len - edge.length_m).abs() < 0.5,
                "edge {e}: {len} vs {}",
                edge.length_m
            );
        }
    }
}
