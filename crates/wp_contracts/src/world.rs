//! Shared city vocabulary (v0.3.0; designed by workstream 05 / D with E and G): districts,
//! zones, road ids and the read-only road-network trait police, traffic and the minimap share.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// District kind of a city zone (was `wp_world::network::DistrictKind`, which re-exports it).
/// Economy and police per-district tables key by this.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum DistrictId {
    /// Downtown: dense all day, nightlife until ~02:00.
    Core,
    /// Shops and offices: commute destination.
    Commercial,
    /// Housing: commute origin.
    Residential,
    /// Factories and yards: shift peaks.
    Industrial,
    /// Docks: steady trucks.
    Port,
    /// Low-density outskirts.
    Suburb,
}

impl DistrictId {
    /// All districts, in declaration order.
    pub const ALL: [DistrictId; 6] = [
        DistrictId::Core,
        DistrictId::Commercial,
        DistrictId::Residential,
        DistrictId::Industrial,
        DistrictId::Port,
        DistrictId::Suburb,
    ];
}

/// One zone (district polygon) of the loaded city: index into `wp_world`'s
/// `RoadNetwork::zones`. Territory (crews) is per zone.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct ZoneId(pub u32);

/// A road of the loaded city: index into [`RoadNetworkQuery`] edges (= `wp_world`'s
/// `RoadGraphView::edges`). Captains, the minimap and race previews share it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct RoadEdgeId(pub u32);

/// Functional road class.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
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

/// One road (both directions) of the network.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RoadEdgeInfo {
    /// Start node.
    pub a: u32,
    /// End node.
    pub b: u32,
    /// Centreline length, m.
    pub length_m: f32,
    /// Speed limit, m/s.
    pub speed_limit_mps: f32,
    /// Functional class.
    pub class: RoadClass,
    /// Drivable lanes `a` → `b`.
    pub lanes_ab: u8,
    /// Drivable lanes `b` → `a` (0 = one-way `a` → `b`).
    pub lanes_ba: u8,
    /// District at the edge midpoint.
    pub district: Option<DistrictId>,
}

/// Kind of a point of interest.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum PoiKind {
    /// Possible player safehouse.
    Safehouse,
    /// Repair / mod garage.
    Garage,
    /// Race start grid (pose = pole position facing the race direction).
    RaceStartGrid,
    /// Police station.
    PoliceStation,
    /// Police helipad.
    Helipad,
    /// Traffic / speed camera.
    Camera,
}

/// A point of interest.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PoiInfo {
    /// Kind.
    pub kind: PoiKind,
    /// Position, world m.
    pub pos: [f32; 3],
    /// Facing (yaw about +Z, 0 = +Y), rad.
    pub heading_rad: f32,
    /// Radius of the area it covers, m.
    pub radius_m: f32,
}

/// Read-only road network of the loaded city (v0.3.0, world #7 / police #5). Implemented by
/// `wp_world::roads::RoadNetworkIndex`; ids are plain indices, stable for the city's life.
/// Line of sight stays on [`crate::WorldQuery`].
pub trait RoadNetworkQuery {
    /// Number of nodes (junctions and dead ends).
    fn node_count(&self) -> u32;
    /// Node position, world XY m.
    fn node_pos(&self, n: u32) -> [f32; 2];
    /// Number of edges.
    fn edge_count(&self) -> u32;
    /// Edge `e`.
    fn edge(&self, e: u32) -> RoadEdgeInfo;
    /// Centreline `a` → `b`, world XY m.
    fn edge_polyline(&self, e: u32) -> &[[f32; 2]];
    /// Edges that may be driven away from `n` (respects one-way), ascending ids.
    fn edges_from(&self, n: u32) -> &[u32];
    /// Closest point on any edge: `(edge, fraction from a, point, distance m)`.
    fn nearest_on_road(&self, p: [f32; 2]) -> Option<(u32, f32, [f32; 2], f32)>;
    /// District at a world XY point.
    fn district_at(&self, p: [f32; 2]) -> Option<DistrictId>;
    /// Points of interest.
    fn pois(&self) -> &[PoiInfo];
}
