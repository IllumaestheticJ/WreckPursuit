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
/// v3 (tunnels, backlog B04): covered spans ([`RoadNetwork::covered`]).
/// v4 (residential streets, playtest rows 94–95): building kind and roof
/// ([`Building::kind`], [`Building::roof`], [`Building::roof_rise_m`]) and street / yard
/// furniture ([`RoadNetwork::decor`]).
/// v5 (divided highways): `RoadSurface::median_barrier`.
pub const ROAD_FORMAT_VERSION: u32 = 5;

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
    /// Covered stretches of road (tunnels, covered trenches): line of sight does not pass
    /// in or out of them (see [`crate::covered`]). Empty for cities without any.
    #[serde(default)]
    pub covered: Vec<CoveredSpan>,
    /// Street and yard furniture: street trees, yard trees, hedges, fences, driveways
    /// ([`Decor`]). Trees, hedges and fences are solid (`wp_sim` colliders); driveways are
    /// drawn only.
    #[serde(default)]
    pub decor: Vec<Decor>,
}

/// What kind of cover a [`CoveredSpan`] is (drawing and radio flavour; line of sight treats
/// every kind the same).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CoverKind {
    /// A tunnel through high ground (the road keeps its profile; ground is backfilled over
    /// the roof).
    Tunnel,
    /// A road lowered into a trench with a deck over it (cut and cover).
    CoveredTrench,
}

/// A covered stretch of one road: an enclosure from the road surface up to a ceiling, between
/// two walls, from one portal to the other.
///
/// Inside = within `half_width_m` of the centreline (laterally), between the two portals
/// (along) and from 3 m below the road up to the ceiling (height). Line of sight never passes
/// from inside to outside or back, and two points inside see each other only along a line
/// that stays inside (a curved tunnel's wall blocks).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoveredSpan {
    /// Display name, e.g. `Hollins Tunnel`.
    pub name: String,
    /// Kind of cover.
    pub kind: CoverKind,
    /// Source road index (`Lane::road`, `RoadSurface::road`).
    pub road: u32,
    /// Road-surface centreline through the covered stretch, portal to portal, m. At least two
    /// points.
    pub centre: Vec<[f32; 3]>,
    /// Inner half width (centreline to the wall face), m.
    pub half_width_m: f32,
    /// Ceiling height above the road surface, m.
    pub clearance_m: f32,
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

/// What a building is (drawing: windows, materials).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum BuildingKind {
    /// Generic block (offices, flats, warehouses).
    #[default]
    Block,
    /// Detached or semi-detached house.
    House,
    /// Garage (detached, or the attached garage wing of a house).
    Garage,
    /// Corner shop in a residential district.
    Shop,
}

/// Roof shape over a building's walls.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum RoofShape {
    /// Flat roof at the wall top.
    #[default]
    Flat,
    /// Gable: a ridge along the footprint's longer axis (footprint must be a rectangle).
    Gable,
    /// Hip: four slopes, a shortened ridge along the longer axis (rectangle footprint).
    Hip,
}

/// A building footprint extruded to a height.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Building {
    /// Footprint polygon (counter-clockwise), m.
    pub footprint: Vec<[f32; 2]>,
    /// Wall (eave) height above `base_z_m`, m.
    pub height_m: f32,
    /// Ground height the walls start from (lowest ground under the footprint), m.
    pub base_z_m: f32,
    /// What the building is.
    #[serde(default)]
    pub kind: BuildingKind,
    /// Roof shape over the walls.
    #[serde(default)]
    pub roof: RoofShape,
    /// Ridge height above the wall top for a pitched roof, m (0 for flat).
    #[serde(default)]
    pub roof_rise_m: f32,
}

impl Building {
    /// A flat-roofed block (the pre-v4 building).
    pub fn block(footprint: Vec<[f32; 2]>, height_m: f32, base_z_m: f32) -> Self {
        Self {
            footprint,
            height_m,
            base_z_m,
            kind: BuildingKind::Block,
            roof: RoofShape::Flat,
            roof_rise_m: 0.0,
        }
    }

    /// Absolute wall-top (eave) height, m: what line of sight and the police feelers use.
    pub fn roof_z(&self) -> f32 {
        self.base_z_m + self.height_m
    }

    /// Absolute top of the roof (ridge for pitched roofs), m.
    pub fn ridge_z(&self) -> f32 {
        match self.roof {
            RoofShape::Flat => self.roof_z(),
            RoofShape::Gable | RoofShape::Hip => self.roof_z() + self.roof_rise_m,
        }
    }

    /// Ridge line (two points at [`Building::ridge_z`]) of a pitched roof over a rectangular
    /// footprint, along its longer axis; `None` for flat roofs or other footprints. A hip
    /// roof's ridge is shortened by the short side's width at each end.
    pub fn ridge(&self) -> Option<[[f32; 3]; 2]> {
        if self.roof == RoofShape::Flat || self.footprint.len() != 4 {
            return None;
        }
        let f = &self.footprint;
        let d = crate::geom::dist;
        let mid = |a: [f32; 2], b: [f32; 2]| [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        // Ridge between the midpoints of the two short sides.
        let (e0, e1, short) = if d(f[0], f[1]) <= d(f[1], f[2]) {
            (mid(f[0], f[1]), mid(f[2], f[3]), d(f[0], f[1]))
        } else {
            (mid(f[1], f[2]), mid(f[3], f[0]), d(f[1], f[2]))
        };
        let (a, b) = if self.roof == RoofShape::Hip {
            let len = d(e0, e1).max(1e-3);
            let k = (short * 0.5 / len).min(0.45);
            (
                [e0[0] + (e1[0] - e0[0]) * k, e0[1] + (e1[1] - e0[1]) * k],
                [e1[0] + (e0[0] - e1[0]) * k, e1[1] + (e0[1] - e1[1]) * k],
            )
        } else {
            (e0, e1)
        };
        let z = self.ridge_z();
        Some([[a[0], a[1], z], [b[0], b[1], z]])
    }
}

/// Kind of street / yard furniture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DecorKind {
    /// Street tree on a grass verge (trunk + crown; the trunk is solid).
    StreetTree,
    /// Tree in a front or back yard (trunk + crown; the trunk is solid).
    YardTree,
    /// Low hedge along a front lot line (solid box).
    Hedge,
    /// Low fence along a front or side lot line (solid box).
    Fence,
    /// Paved driveway from the sidewalk to a garage (drawn only; level with the yard).
    Driveway,
}

/// One piece of street / yard furniture: an oriented box standing on the ground.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Decor {
    /// Kind.
    pub kind: DecorKind,
    /// Centre of the base (ground point), m.
    pub pos: [f32; 3],
    /// Yaw about +Z (0 = the box's length along +Y), rad.
    pub yaw: f32,
    /// Full size: width (local X), length (local Y), height, m. Trees: crown diameter,
    /// crown diameter, total height.
    pub size: [f32; 3],
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
    /// The terrain when it has relief (hills, embankments); `None` for a city without terrain
    /// or with a flat one (the grid city's, under its viaduct). Sidewalk lots, wedges and
    /// slope support follow the flat-city rules then.
    pub fn relief(&self) -> Option<&crate::terrain::Heightfield> {
        self.terrain
            .as_ref()
            .filter(|t| t.z.iter().any(|z| *z != t.z[0]))
    }

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
        for (i, c) in self.covered.iter().enumerate() {
            if c.centre.len() < 2
                || c.half_width_m <= 1.0
                || c.clearance_m <= 2.0
                || c.road as usize >= self.surfaces.len().max(1)
            {
                return bad(format!("covered span {i}: bad centreline, size or road"));
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
