//! The road network file format (`data/city/<city>.road.bin` + `.road.json` debug dump).
//!
//! A [`RoadNetwork`] is plain data: nodes, directed lanes with centreline polylines and turn
//! connections, intersections with their control rule, fixed-time signal controllers, district
//! zones, points of interest and building footprints. Derived runtime data (connector curves,
//! conflict sets, spatial grid) is built at load time by [`crate::CityMap`].
//!
//! Binary layout: 4-byte magic `WPRD`, little-endian `u32` [`ROAD_FORMAT_VERSION`], then the
//! [`postcard`] encoding of [`RoadNetwork`]. The JSON dump is the `serde_json` encoding of the
//! same struct and loads identically.
//!
//! Index types are plain `u32` indices into the corresponding vectors.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Magic bytes at the start of a `.road.bin` file.
pub const ROAD_MAGIC: [u8; 4] = *b"WPRD";
/// Version of the road format. Bump on any layout change.
/// v2 (city v2): road surfaces, terrain heightfield, building base heights.
/// v3: `RoadSurface::median_barrier` (divided highways).
/// v4: [`RoadNetwork::features`] (waterways, bridges, tunnels, structures, landmarks,
/// hideouts).
pub const ROAD_FORMAT_VERSION: u32 = 4;

/// Index into [`RoadNetwork::nodes`].
pub type NodeId = u32;
/// Index into [`RoadNetwork::lanes`].
pub type LaneId = u32;
/// Index into [`RoadNetwork::intersections`].
pub type IntersectionId = u32;
/// Index into [`RoadNetwork::signals`].
pub type SignalId = u32;
/// Index into [`RoadNetwork::zones`].
pub type ZoneId = u32;

/// A complete city road network.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoadNetwork {
    /// Must equal [`ROAD_FORMAT_VERSION`].
    pub format_version: u32,
    /// City id (matches the file stem, e.g. `procedural_12x12`).
    pub id: String,
    /// Human-readable provenance, e.g. `procedural grid=12x12 seed=1` or `osm:extract.osm.pbf`.
    pub source: String,
    /// Attribution text that must be shown in credits when this data ships (OSM: ODbL).
    pub attribution: Option<String>,
    /// Axis-aligned bounds of all geometry, `[min_x, min_y]`, m.
    pub bounds_min: [f32; 2],
    /// Axis-aligned bounds of all geometry, `[max_x, max_y]`, m.
    pub bounds_max: [f32; 2],
    /// City core position (density falls off with distance from here), m.
    pub core: [f32; 2],
    /// Graph nodes (junctions, bends, dead ends).
    pub nodes: Vec<Node>,
    /// Directed lane segments (stop line to stop line).
    pub lanes: Vec<Lane>,
    /// Junctions with three or more roads.
    pub intersections: Vec<Intersection>,
    /// Fixed-time signal controllers.
    pub signals: Vec<SignalController>,
    /// District zones.
    pub zones: Vec<Zone>,
    /// Points of interest.
    pub pois: Vec<Poi>,
    /// Building footprints for the greybox mesh and line-of-sight.
    pub buildings: Vec<Building>,
    /// Paved surface of every source road (one per road, in road order): what physics and the
    /// greybox draw. Lane polylines lie on these surfaces.
    pub surfaces: Vec<RoadSurface>,
    /// Ground heightfield between roads (`None` = flat ground at z = 0).
    pub terrain: Option<crate::terrain::Heightfield>,
    /// Rivers, bridges, tunnels, landmarks and hiding spots (empty for the grid city and
    /// imports).
    pub features: CityFeatures,
}

/// The paved ribbon of one road between its two junction setbacks.
///
/// Height across the ribbon is linear: at lateral offset `lat` (m, + = right of the
/// `centre` direction) it is `centre.z + bank * lat`. Lanes of the road lie on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoadSurface {
    /// Source road index (`Lane::road`).
    pub road: u32,
    /// Trimmed centreline (a → b), m.
    pub centre: Vec<[f32; 3]>,
    /// Cross slope per centreline point (dz per metre to the right; > 0 = right side higher).
    pub bank: Vec<f32>,
    /// Paved width left of the centreline (driving + parking lanes), m.
    pub left_m: f32,
    /// Paved width right of the centreline, m.
    pub right_m: f32,
    /// Highway (concrete, no sidewalks).
    pub highway: bool,
    /// Kerb + sidewalk on the left / right edge.
    pub sidewalk: [bool; 2],
    /// A concrete median barrier runs along the left paved edge (one carriageway of a
    /// divided highway carries it; [`crate::surface::median_barriers`] gives its line).
    pub median_barrier: bool,
}

impl RoadSurface {
    /// Surface height at centreline point `k` and lateral offset `lat`.
    pub fn z_at(&self, k: usize, lat: f32) -> f32 {
        self.centre[k][2] + self.bank[k] * lat
    }
}

/// A road graph node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Position, m.
    pub pos: [f32; 3],
    /// Intersection at this node, if it is a junction.
    pub intersection: Option<IntersectionId>,
}

/// What a lane is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum LaneKind {
    /// General traffic.
    Through,
    /// Dedicated left-turn lane.
    TurnLeft,
    /// Dedicated right-turn lane.
    TurnRight,
    /// Buses only (civilian cars never use it).
    Bus,
    /// Kerbside parking (not driven; parked cars live here).
    Parking,
    /// Narrow service alley.
    Alley,
    /// Limited-access highway lane.
    Highway,
}

impl LaneKind {
    /// Whether civilian cars may drive in this lane.
    pub fn drivable(self) -> bool {
        !matches!(self, LaneKind::Bus | LaneKind::Parking)
    }
}

/// Manoeuvre made by a lane connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TurnKind {
    /// Straight on (also: any continuation through a non-junction bend).
    Straight,
    /// Left turn (crosses opposing traffic in right-hand traffic).
    Left,
    /// Right turn.
    Right,
    /// U-turn (dead ends only).
    UTurn,
}

/// An allowed movement from the far end of a lane into another lane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LaneConnection {
    /// Lane entered.
    pub to: LaneId,
    /// Manoeuvre type.
    pub turn: TurnKind,
}

/// A directed lane between two stop lines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lane {
    /// Node the lane leaves.
    pub from_node: NodeId,
    /// Node the lane arrives at (its stop line is the last polyline point).
    pub to_node: NodeId,
    /// Source road index (lanes of the same road share it; both directions).
    pub road: u32,
    /// Lane purpose.
    pub kind: LaneKind,
    /// Index within its direction's driving lanes, 0 = leftmost (next to the centre line).
    pub index_from_left: u8,
    /// Centreline in travel direction, m. At least two points.
    pub polyline: Vec<[f32; 3]>,
    /// Lane width, m.
    pub width_m: f32,
    /// Posted speed limit, m/s.
    pub speed_limit_mps: f32,
    /// Same-direction lane to the left (lane change target), if any.
    pub left: Option<LaneId>,
    /// Same-direction lane to the right (lane change target), if any.
    pub right: Option<LaneId>,
    /// Left edge is the edge of the paved ribbon (curb).
    pub edge_left: bool,
    /// Right edge is the edge of the paved ribbon (curb).
    pub edge_right: bool,
    /// Allowed movements at the far end. Empty = the lane leaves the map.
    pub connections: Vec<LaneConnection>,
}

/// How a junction is controlled.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum IntersectionControl {
    /// Fixed-time signals.
    Signal {
        /// Controller index.
        controller: SignalId,
    },
    /// All-way stop: first to stop goes first.
    AllWayStop,
    /// Major roads have priority; minor approaches stop and yield.
    Priority,
    /// No control (yield to the right of way by occupancy only).
    Uncontrolled,
}

/// One lane arriving at an intersection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IncomingLane {
    /// Arriving lane.
    pub lane: LaneId,
    /// Signal group (index into the controller's groups) for signalised junctions.
    pub signal_group: u8,
    /// For [`IntersectionControl::Priority`]: this approach has the right of way.
    pub major: bool,
}

/// A junction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Intersection {
    /// Node at the junction centre.
    pub node: NodeId,
    /// Control rule.
    pub control: IntersectionControl,
    /// Arriving lanes (driving lanes only).
    pub incoming: Vec<IncomingLane>,
    /// Right turn on red after a full stop is allowed.
    pub allow_rtor: bool,
    /// Radius of the junction box around the node, m (inside it the surface is asphalt and no
    /// lane is "current").
    pub radius_m: f32,
}

/// One phase of a fixed-time plan: green, then yellow, then all-red.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalPhase {
    /// Groups that are green in this phase.
    pub green_groups: Vec<u8>,
    /// Green duration, s.
    pub green_s: f32,
    /// Yellow duration, s.
    pub yellow_s: f32,
    /// All-red clearance after the yellow, s.
    pub all_red_s: f32,
}

/// Fixed-time signal controller for one intersection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalController {
    /// Intersection it controls.
    pub intersection: IntersectionId,
    /// Number of signal groups.
    pub groups: u8,
    /// Cycle offset, s (green waves along arterials).
    pub offset_s: f32,
    /// Phases in order; cycle length = sum of all phase durations.
    pub phases: Vec<SignalPhase>,
}

impl SignalController {
    /// Cycle length, s.
    pub fn cycle_s(&self) -> f32 {
        self.phases
            .iter()
            .map(|p| p.green_s + p.yellow_s + p.all_red_s)
            .sum()
    }
}

/// District kind of a zone. Drives traffic density curves and off-road surfaces.
/// Since contract v0.3.0 this is `wp_contracts::DistrictId` (same variants, `ALL`).
pub use wp_contracts::DistrictId as DistrictKind;

/// A district polygon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    /// District kind.
    pub kind: DistrictKind,
    /// Peak traffic density, vehicles per lane-km of driving lane.
    pub density_base: f32,
    /// Simple polygon (counter-clockwise), m.
    pub polygon: Vec<[f32; 2]>,
}

/// Kind of point of interest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PoiKind {
    /// Possible player safehouse.
    SafehouseCandidate,
    /// Repair / mod garage.
    Garage,
    /// Race start grid (pose = pole position, facing the race direction).
    RaceStartGrid,
    /// Police station (patrol spawn).
    PoliceStation,
    /// Police helipad.
    Helipad,
}

/// A point of interest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Poi {
    /// Kind.
    pub kind: PoiKind,
    /// Display name.
    pub name: String,
    /// Position, m.
    pub pos: [f32; 3],
    /// Facing (yaw about +Z, 0 = +Y), rad.
    pub heading_rad: f32,
    /// Nearest lane (for race grids and garages: the lane the pose sits on).
    pub lane: Option<LaneId>,
}

/// A building footprint extruded to a height.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Building {
    /// Footprint polygon (counter-clockwise), m.
    pub footprint: Vec<[f32; 2]>,
    /// Roof height above `base_z_m`, m.
    pub height_m: f32,
    /// Ground height the walls start from (lowest ground under the footprint), m.
    pub base_z_m: f32,
}

impl Building {
    /// Absolute roof height, m.
    pub fn roof_z(&self) -> f32 {
        self.base_z_m + self.height_m
    }
}

/// Water surface level of the sea (and of every waterway at its mouth), m. Terrain below it
/// is seabed.
pub const SEA_LEVEL_M: f32 = 0.25;

/// Everything about a city beyond roads, zones and plain buildings: water, named bridges,
/// tunnels, landmark structures and the places a racer can hide from the police.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CityFeatures {
    /// Rivers and canals.
    pub waterways: Vec<Waterway>,
    /// Named road bridges over water.
    pub bridges: Vec<Bridge>,
    /// Road tunnels.
    pub tunnels: Vec<Tunnel>,
    /// Solid prisms that are not ordinary buildings: tunnel linings and portals, canopies
    /// and their pillars, landmark parts, bridge trusses. They collide, render and block line
    /// of sight like buildings, but may float (`z0_m` above the ground).
    pub structures: Vec<Structure>,
    /// Named landmarks (orientation points for players and the minimap).
    pub landmarks: Vec<Landmark>,
    /// Places where a car is hidden from the air (tunnels, canopies).
    pub hideouts: Vec<Hideout>,
}

/// Kind of waterway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum WaterwayKind {
    /// A natural river (water surface falls towards the mouth).
    River,
    /// A level canal (water surface at sea level).
    Canal,
}

/// A river or canal: a water ribbon over a channel carved into the terrain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Waterway {
    /// Display name.
    pub name: String,
    /// River or canal.
    pub kind: WaterwayKind,
    /// Centreline from source to mouth; `z` is the water surface, m.
    pub centre: Vec<[f32; 3]>,
    /// Half width of the water surface per centreline point, m.
    pub half_width_m: Vec<f32>,
    /// Water depth in the middle of the channel, m.
    pub depth_m: f32,
}

/// Kind of bridge (looks and deck surface).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum BridgeKind {
    /// Concrete beam bridge (road surface on the deck).
    Beam,
    /// Steel truss with a steel-grate deck (`SurfaceType::Metal`).
    Truss,
    /// Long highway viaduct.
    Viaduct,
}

/// A named bridge: the deck stretch of one road surface over water.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bridge {
    /// Display name.
    pub name: String,
    /// Kind.
    pub kind: BridgeKind,
    /// Index into [`RoadNetwork::surfaces`].
    pub surface: u32,
    /// Deck centreline (on the road surface), m.
    pub centre: Vec<[f32; 3]>,
    /// Half width of the deck (widest paved side), m.
    pub half_width_m: f32,
}

/// A road tunnel bore: a box of `2 * half_width_m` by `height_m` along `centre`, under the
/// terrain. Terrain over it is natural ground; inside, the ground does not block line of
/// sight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tunnel {
    /// Display name.
    pub name: String,
    /// Floor centreline (road height at the centre), portal to portal, m.
    pub centre: Vec<[f32; 3]>,
    /// Cross slope of the floor per centreline point (dz per metre to the right).
    pub bank: Vec<f32>,
    /// Half width between the lining walls, m.
    pub half_width_m: f32,
    /// Clear height from the floor to the ceiling, m.
    pub height_m: f32,
}

impl Tunnel {
    /// Total length, m.
    pub fn length_m(&self) -> f32 {
        let c = crate::geom::cumulative(&self.centre);
        c[c.len() - 1]
    }

    /// Whether `p` is inside the bore (between the walls, with 0.5 m slack, the floor and
    /// the ceiling).
    pub fn contains(&self, p: [f32; 3]) -> bool {
        use crate::geom;
        let q = geom::xy(p);
        let r = self.half_width_m + self.height_m;
        let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
        for c in &self.centre {
            lo = [lo[0].min(c[0]), lo[1].min(c[1])];
            hi = [hi[0].max(c[0]), hi[1].max(c[1])];
        }
        if q[0] < lo[0] - r || q[0] > hi[0] + r || q[1] < lo[1] - r || q[1] > hi[1] + r {
            return false;
        }
        let cum = geom::cumulative(&self.centre);
        let pr = geom::project(&self.centre, &cum, q);
        if pr.s < 0.0 || pr.s > cum[cum.len() - 1] || pr.lateral.abs() > self.half_width_m + 0.5 {
            return false;
        }
        let n = self.centre.len();
        let i = cum.partition_point(|x| *x <= pr.s).clamp(1, n - 1);
        let f = ((pr.s - cum[i - 1]) / (cum[i] - cum[i - 1]).max(1e-6)).clamp(0.0, 1.0);
        let z = self.centre[i - 1][2] + (self.centre[i][2] - self.centre[i - 1][2]) * f;
        let bank = self.bank[i - 1] + (self.bank[i] - self.bank[i - 1]) * f;
        let floor = z + bank * pr.lateral;
        p[2] >= floor - 0.5 && p[2] <= floor + self.height_m
    }
}

/// What a [`Structure`] is (render material and diagnostics).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum StructureKind {
    /// Tunnel lining wall.
    TunnelWall,
    /// Tunnel ceiling slab.
    TunnelCeiling,
    /// Tunnel portal block (walls and roof at the mouth).
    TunnelPortal,
    /// Roof slab of a canopy you can drive under.
    Canopy,
    /// Pillar holding up a canopy.
    Pillar,
    /// Part of a landmark.
    Landmark,
    /// Steel truss member over a bridge deck.
    Truss,
}

/// A solid vertical prism from `z0_m` to `z1_m` over a convex footprint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Structure {
    /// What it is.
    pub kind: StructureKind,
    /// Convex footprint (counter-clockwise), m.
    pub footprint: Vec<[f32; 2]>,
    /// Bottom height, m.
    pub z0_m: f32,
    /// Top height, m.
    pub z1_m: f32,
}

/// Kind of landmark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum LandmarkKind {
    /// The tallest tower downtown.
    Tower,
    /// Lighthouse on the coast.
    Lighthouse,
    /// Observatory dome.
    Observatory,
    /// Industrial chimney.
    Chimney,
    /// Ship-to-shore container crane.
    Crane,
    /// Water tower.
    WaterTower,
    /// A named bridge with a distinctive structure.
    Bridge,
    /// A named tunnel.
    Tunnel,
}

/// A named landmark.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Landmark {
    /// Display name.
    pub name: String,
    /// Kind.
    pub kind: LandmarkKind,
    /// Base centre, m.
    pub pos: [f32; 3],
    /// Height above `pos`, m.
    pub height_m: f32,
    /// Its parts in [`CityFeatures::structures`] (may be empty for bridges and tunnels).
    pub structures: Vec<u32>,
}

/// Kind of hiding spot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum HideoutKind {
    /// Inside a road tunnel.
    Tunnel,
    /// Under a canopy (open sides: ground units still see in from close by).
    Canopy,
}

/// A place where a car is out of sight from the air (`WorldQuery::line_of_sight` from above
/// is blocked anywhere within `radius_m` of `pos` at ground level, under the cover).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hideout {
    /// Display name.
    pub name: String,
    /// Kind.
    pub kind: HideoutKind,
    /// Centre at ground level, m.
    pub pos: [f32; 3],
    /// Rough radius of the covered area, m.
    pub radius_m: f32,
}

/// Errors loading or validating a road network.
#[derive(Debug, Error)]
pub enum RoadError {
    /// File IO failed.
    #[error("io error on {path}: {source}")]
    Io {
        /// Path.
        path: String,
        /// Cause.
        source: std::io::Error,
    },
    /// Not a road file / wrong version.
    #[error("bad header: {0}")]
    BadHeader(String),
    /// Decoding failed.
    #[error("decode error: {0}")]
    Decode(String),
    /// Structural validation failed.
    #[error("invalid road network: {0}")]
    Invalid(String),
}

impl RoadNetwork {
    /// Ground height at `p` (terrain, or 0 without one). Roads are not included; see
    /// `CityMap::surface_height`.
    pub fn ground_z(&self, p: [f32; 2]) -> f32 {
        self.terrain.as_ref().map_or(0.0, |t| t.height_at(p))
    }

    /// Encode to the `.road.bin` format.
    pub fn to_bin(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&ROAD_MAGIC);
        out.extend_from_slice(&ROAD_FORMAT_VERSION.to_le_bytes());
        let body = postcard::to_allocvec(self).expect("road network serialises");
        out.extend_from_slice(&body);
        out
    }

    /// Decode from the `.road.bin` format and validate.
    pub fn from_bin(bytes: &[u8]) -> Result<Self, RoadError> {
        if bytes.len() < 8 || bytes[0..4] != ROAD_MAGIC {
            return Err(RoadError::BadHeader("missing WPRD magic".into()));
        }
        let ver = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if ver != ROAD_FORMAT_VERSION {
            return Err(RoadError::BadHeader(format!(
                "format version {ver}, expected {ROAD_FORMAT_VERSION}"
            )));
        }
        let net: RoadNetwork =
            postcard::from_bytes(&bytes[8..]).map_err(|e| RoadError::Decode(e.to_string()))?;
        net.validate()?;
        Ok(net)
    }

    /// Encode as the JSON debug dump.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("road network serialises")
    }

    /// Decode the JSON debug dump and validate.
    pub fn from_json(text: &str) -> Result<Self, RoadError> {
        let net: RoadNetwork =
            serde_json::from_str(text).map_err(|e| RoadError::Decode(e.to_string()))?;
        net.validate()?;
        Ok(net)
    }

    /// Load `.road.bin` or `.road.json` (chosen by extension).
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, RoadError> {
        let path = path.as_ref();
        let io = |source| RoadError::Io {
            path: path.display().to_string(),
            source,
        };
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            Self::from_json(&std::fs::read_to_string(path).map_err(io)?)
        } else {
            Self::from_bin(&std::fs::read(path).map_err(io)?)
        }
    }

    /// Check indices, polylines and signal plans.
    pub fn validate(&self) -> Result<(), RoadError> {
        let bad = |m: String| Err(RoadError::Invalid(m));
        if self.format_version != ROAD_FORMAT_VERSION {
            return bad(format!("format_version {}", self.format_version));
        }
        let nn = self.nodes.len() as u32;
        let nl = self.lanes.len() as u32;
        for (i, n) in self.nodes.iter().enumerate() {
            if let Some(ix) = n.intersection {
                if ix as usize >= self.intersections.len() {
                    return bad(format!("node {i}: intersection {ix} out of range"));
                }
            }
        }
        for (i, l) in self.lanes.iter().enumerate() {
            if l.from_node >= nn || l.to_node >= nn {
                return bad(format!("lane {i}: node out of range"));
            }
            if l.polyline.len() < 2 {
                return bad(format!("lane {i}: polyline needs >= 2 points"));
            }
            if !(l.width_m > 0.5 && l.speed_limit_mps > 0.0) {
                return bad(format!("lane {i}: bad width or speed limit"));
            }
            for n in [l.left, l.right].into_iter().flatten() {
                if n >= nl {
                    return bad(format!("lane {i}: neighbour out of range"));
                }
            }
            for c in &l.connections {
                if c.to >= nl {
                    return bad(format!("lane {i}: connection to {} out of range", c.to));
                }
                if self.lanes[c.to as usize].from_node != l.to_node {
                    return bad(format!(
                        "lane {i}: connection to {} not at its end node",
                        c.to
                    ));
                }
            }
        }
        for (i, ix) in self.intersections.iter().enumerate() {
            if ix.node >= nn {
                return bad(format!("intersection {i}: node out of range"));
            }
            for inc in &ix.incoming {
                if inc.lane >= nl || self.lanes[inc.lane as usize].to_node != ix.node {
                    return bad(format!(
                        "intersection {i}: incoming lane {} invalid",
                        inc.lane
                    ));
                }
            }
            if let IntersectionControl::Signal { controller } = ix.control {
                let Some(sc) = self.signals.get(controller as usize) else {
                    return bad(format!("intersection {i}: controller out of range"));
                };
                if sc.intersection as usize != i {
                    return bad(format!("intersection {i}: controller points elsewhere"));
                }
                if ix.incoming.iter().any(|inc| inc.signal_group >= sc.groups) {
                    return bad(format!("intersection {i}: signal group out of range"));
                }
            }
        }
        for (i, sc) in self.signals.iter().enumerate() {
            if sc.phases.is_empty() || sc.cycle_s() <= 0.0 {
                return bad(format!("signal {i}: empty plan"));
            }
            for p in &sc.phases {
                if p.green_s <= 0.0 || p.yellow_s < 0.0 || p.all_red_s < 0.0 {
                    return bad(format!("signal {i}: negative durations"));
                }
                if p.green_groups.iter().any(|g| *g >= sc.groups) {
                    return bad(format!("signal {i}: phase group out of range"));
                }
            }
        }
        for (i, z) in self.zones.iter().enumerate() {
            if z.polygon.len() < 3 || z.density_base < 0.0 {
                return bad(format!("zone {i}: bad polygon or density"));
            }
        }
        for (i, p) in self.pois.iter().enumerate() {
            if p.lane.is_some_and(|l| l >= nl) {
                return bad(format!("poi {i}: lane out of range"));
            }
        }
        for (i, s) in self.surfaces.iter().enumerate() {
            if s.centre.len() < 2 || s.bank.len() != s.centre.len() {
                return bad(format!("surface {i}: bad centreline / bank"));
            }
        }
        let f = &self.features;
        for (i, w) in f.waterways.iter().enumerate() {
            if w.centre.len() < 2 || w.half_width_m.len() != w.centre.len() {
                return bad(format!("waterway {i}: bad centreline / widths"));
            }
        }
        for (i, b) in f.bridges.iter().enumerate() {
            if b.centre.len() < 2 || b.surface as usize >= self.surfaces.len() {
                return bad(format!("bridge {i}: bad centreline / surface"));
            }
        }
        for (i, t) in f.tunnels.iter().enumerate() {
            if t.centre.len() < 2 || t.bank.len() != t.centre.len() || t.height_m <= 0.0 {
                return bad(format!("tunnel {i}: bad centreline / size"));
            }
        }
        for (i, s) in f.structures.iter().enumerate() {
            if s.footprint.len() < 3 || s.z1_m <= s.z0_m {
                return bad(format!("structure {i}: bad footprint / heights"));
            }
        }
        for (i, l) in f.landmarks.iter().enumerate() {
            if l.structures
                .iter()
                .any(|s| *s as usize >= f.structures.len())
            {
                return bad(format!("landmark {i}: structure out of range"));
            }
        }
        if let Some(t) = &self.terrain {
            if t.nx < 2 || t.ny < 2 || t.z.len() != (t.nx * t.ny) as usize || t.cell_m <= 0.0 {
                return bad("terrain: bad grid".into());
            }
        }
        Ok(())
    }
}
