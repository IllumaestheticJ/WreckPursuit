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
pub const ROAD_FORMAT_VERSION: u32 = 1;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DistrictKind {
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

impl DistrictKind {
    /// All kinds, in declaration order.
    pub const ALL: [DistrictKind; 6] = [
        DistrictKind::Core,
        DistrictKind::Commercial,
        DistrictKind::Residential,
        DistrictKind::Industrial,
        DistrictKind::Port,
        DistrictKind::Suburb,
    ];
}

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
    /// Height above ground, m.
    pub height_m: f32,
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
        Ok(())
    }
}
