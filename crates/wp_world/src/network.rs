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
/// v4: static scenery and structures ([`RoadNetwork::features`]).
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
    /// Static scenery and structures (v4): walls, fences, trees, parked cars, rail, cranes,
    /// parking decks, runways, ... Solid ones are static colliders in `wp_sim`; box and
    /// cylinder shapes are drawn instanced by the testbed, prisms go into the greybox.
    /// Empty for the procedural grid and Port Vesper v2.
    pub features: Vec<Feature>,
}

/// One static scenery / structure element (format v4).
///
/// Features are plain shapes with a semantic [`FeatureKind`] (what it is: gameplay, tests,
/// minimap) and a [`Material`] (how it is drawn). `solid` features become static colliders
/// (`wp_sim::physics::PhysicsWorld::set_city_ground`); a solid feature with a flat or gently
/// sloped top is drivable (parking decks, ramps, runway slabs, tunnel plugs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    /// What it is.
    pub kind: FeatureKind,
    /// How it is drawn.
    pub material: Material,
    /// Geometry.
    pub shape: FeatureShape,
    /// Static collider in physics (cars hit it / drive on it).
    pub solid: bool,
}

/// Geometry of a [`Feature`]. World space, metres, +Z up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FeatureShape {
    /// Oriented box. `half` = half extents along the local axes (x right, y forward, z up)
    /// before rotation; the box is pitched about its local +X by `pitch` (positive = the +Y
    /// end up), then yawed about world +Z by `yaw` (0 = local +Y points north, positive =
    /// counter-clockwise), then moved to `centre`.
    Box {
        /// Centre, m.
        centre: [f32; 3],
        /// Half extents, m.
        half: [f32; 3],
        /// Yaw about +Z, rad.
        yaw: f32,
        /// Pitch about the local +X axis, rad (ramps).
        pitch: f32,
    },
    /// Vertical upright cylinder (trunks, tanks, silos, columns).
    Cylinder {
        /// Centre of the bottom face, m.
        base: [f32; 3],
        /// Radius, m.
        radius: f32,
        /// Height, m.
        height: f32,
    },
    /// Vertical prism: a simple counter-clockwise footprint extruded from `base_z` to `top_z`.
    /// A solid prism's collider is the convex hull of its footprint, so solid prisms must be
    /// convex (split concave shapes into several features).
    Prism {
        /// Footprint polygon (counter-clockwise), m.
        footprint: Vec<[f32; 2]>,
        /// Bottom, m.
        base_z: f32,
        /// Top, m.
        top_z: f32,
    },
}

/// Semantic kind of a [`Feature`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FeatureKind {
    // --- Boundaries (keep cars out of places that are not meant to be driven) ---
    /// Concrete or masonry wall.
    Wall,
    /// Chain-link / palisade fence.
    Fence,
    /// Hedge.
    Hedge,
    /// Jersey (concrete) barrier.
    JerseyBarrier,
    /// Steel guard rail.
    GuardRail,
    /// Highway sound wall.
    SoundWall,
    /// Bollard / post.
    Bollard,
    /// Gate or barrier arm.
    Gate,
    /// Invisible-in-spirit map edge wall (still drawn, low-key).
    MapEdge,
    // --- Water edge ---
    /// Quay or river wall.
    QuayWall,
    /// Pier / jetty deck.
    Pier,
    /// Mooring bollard.
    MooringBollard,
    /// Ship hull or superstructure part.
    Ship,
    /// Small boat.
    Boat,
    // --- Rail ---
    /// Track (ballast + rails), one straight piece.
    RailTrack,
    /// Station platform.
    Platform,
    /// Rail car (freight wagon / coach / locomotive part).
    RailCar,
    /// Level-crossing gate, signal mast, catenary pole.
    RailFurniture,
    /// Rail bridge deck / pier.
    RailBridge,
    // --- Airport / airfield ---
    /// Runway slab.
    Runway,
    /// Taxiway slab.
    Taxiway,
    /// Apron slab.
    Apron,
    /// Runway / taxiway paint.
    AirfieldMarking,
    /// Hangar.
    Hangar,
    /// Terminal building part.
    Terminal,
    /// Control tower part.
    ControlTower,
    /// Parked aircraft part.
    Aircraft,
    /// Jet bridge, windsock, beacon, approach light.
    AirportFurniture,
    // --- Port ---
    /// Shipping container.
    Container,
    /// Ship-to-shore gantry crane part.
    GantryCrane,
    /// Straddle carrier / reach stacker / yard machine.
    PortMachine,
    /// Storage tank / silo.
    Tank,
    // --- Parking ---
    /// Parking structure deck (drivable).
    ParkingDeck,
    /// Parking structure ramp (drivable).
    ParkingRamp,
    /// Column / pillar.
    Pillar,
    /// Parked car (static).
    ParkedCar,
    /// Parking bay paint.
    ParkingPaint,
    // --- Construction ---
    /// Site hoarding / fence.
    ConstructionFence,
    /// Scaffolding.
    Scaffold,
    /// Structural steel / concrete frame of an unfinished building.
    Frame,
    /// Tower crane part.
    TowerCrane,
    /// Excavator, dozer, dump truck part.
    ConstructionMachine,
    /// Dirt / gravel mound (drivable if solid).
    Earthwork,
    /// Material pile, pipes, pallets, portable toilets.
    SiteClutter,
    // --- Roads ---
    /// Tunnel wall / roof / portal / cover plug.
    Tunnel,
    /// Bridge structure (truss, tower, cable, pier) that is not road surface.
    BridgeStructure,
    /// Road sign, gantry, signal pole.
    RoadFurniture,
    // --- Scenery ---
    /// Tree trunk.
    TreeTrunk,
    /// Tree canopy.
    TreeCanopy,
    /// Bush.
    Bush,
    /// Street lamp.
    Lamp,
    /// Utility pole.
    UtilityPole,
    /// Bench, bin, hydrant, mailbox.
    StreetFurniture,
    /// Bus shelter.
    BusShelter,
    /// Billboard.
    Billboard,
    /// Lawn, park or plaza surface.
    Ground,
    /// Rock / boulder.
    Rock,
    /// Statue, fountain, monument.
    Monument,
    /// Landmark structure part (stadium stand, cooling tower, lighthouse, screen, ...).
    Landmark,
}

/// Render material of a [`Feature`] (greybox colour; the testbed may shade it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Material {
    /// Light concrete.
    Concrete,
    /// Dark concrete.
    DarkConcrete,
    /// Asphalt.
    Asphalt,
    /// Painted steel (grey).
    Steel,
    /// Rusty steel.
    Rust,
    /// Wood.
    Wood,
    /// Brick.
    Brick,
    /// Glass.
    Glass,
    /// Grass.
    Grass,
    /// Foliage (trees, hedges).
    Foliage,
    /// Bare earth.
    Dirt,
    /// Gravel / ballast.
    Gravel,
    /// Sand.
    Sand,
    /// White paint.
    PaintWhite,
    /// Yellow paint.
    PaintYellow,
    /// Safety orange (cones, machines, hoarding).
    SafetyOrange,
    /// Safety yellow (cranes, machines).
    SafetyYellow,
    /// Red (containers, hulls).
    Red,
    /// Blue (containers).
    Blue,
    /// Green (containers).
    Green,
    /// White (aircraft, hulls).
    White,
    /// Black (tyres, rubber).
    Black,
    /// Car paint (the testbed picks a colour per instance).
    CarPaint,
    /// Emissive light.
    Light,
}

impl FeatureShape {
    /// Axis-aligned 2D bounds `(min, max)` of the shape, m.
    pub fn bounds_2d(&self) -> ([f32; 2], [f32; 2]) {
        match self {
            FeatureShape::Box {
                centre,
                half,
                yaw,
                pitch,
            } => {
                let (s, c) = wp_math::sin_cos(*yaw);
                let (_, cp) = wp_math::sin_cos(*pitch);
                // Pitch shortens the y extent's footprint (never lengthens past the full
                // diagonal; this bound is conservative).
                let hy = half[1] * cp.abs() + half[2] * (1.0 - cp.abs());
                let ex = (half[0] * c).abs() + (hy * s).abs();
                let ey = (half[0] * s).abs() + (hy * c).abs();
                (
                    [centre[0] - ex, centre[1] - ey],
                    [centre[0] + ex, centre[1] + ey],
                )
            }
            FeatureShape::Cylinder { base, radius, .. } => (
                [base[0] - radius, base[1] - radius],
                [base[0] + radius, base[1] + radius],
            ),
            FeatureShape::Prism { footprint, .. } => {
                let mut lo = [f32::MAX; 2];
                let mut hi = [f32::MIN; 2];
                for p in footprint {
                    lo = [lo[0].min(p[0]), lo[1].min(p[1])];
                    hi = [hi[0].max(p[0]), hi[1].max(p[1])];
                }
                (lo, hi)
            }
        }
    }
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
        if let Some(t) = &self.terrain {
            if t.nx < 2 || t.ny < 2 || t.z.len() != (t.nx * t.ny) as usize || t.cell_m <= 0.0 {
                return bad("terrain: bad grid".into());
            }
        }
        Ok(())
    }
}
