//! The master layout of Port Vesper v3: map bounds, water, districts, the highway network,
//! arterials, bridges, tunnels, rail, and every reserved site (airport, airfield, port,
//! parking structures, construction sites, landmarks).
//!
//! **Owned by the v3 lead.** Every other v3 module reads it; none edits it. A module that
//! needs a change (a site moved, a gate added) says so in its report and the lead edits
//! this file. Coordinates follow `docs/design/CITY.md` (origin at the centre, metres, +X
//! east, +Y north, +Z up) and extend it west (airport), north (airfield) and south (shore).
//!
//! Everything here is *intent*: positions of interchanges, gates and sites are where the
//! generator should put them; a module may move a point by a few tens of metres to make
//! geometry work (an interchange up to ±200 m along its highway) and must report it.

use crate::geom::V2;
use crate::network::{DistrictKind, PoiKind, Zone};

/// South-west corner of the map (terrain origin), m.
pub const MAP_LO: V2 = [-4400.0, -3600.0];
/// North-east corner of the map, m.
pub const MAP_HI: V2 = [3600.0, 3400.0];
/// Density core (Lantern Row), m.
pub const CORE: V2 = [450.0, 200.0];
/// Water surface height (sea, river, canal; the greybox draws one sea plane), m.
pub const WATER_Z: f32 = 0.25;
/// Land next to water never sits lower than this, m.
pub const MIN_LAND_Z: f32 = 1.5;

/// A polyline water body with a width (river, canal).
#[derive(Debug, Clone)]
pub struct Channel {
    /// Name.
    pub name: &'static str,
    /// Centreline, m.
    pub line: Vec<V2>,
    /// Half width at the water line, m.
    pub half_width_m: f32,
    /// Bed height at the centre, m.
    pub bed_z: f32,
    /// Walled banks (canal, harbour): vertical quay walls instead of sloped banks.
    pub walled: bool,
}

/// Kind of a reserved site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SiteKind {
    /// International airport (runways, terminal, apron, hangars). Module `airport`.
    Airport,
    /// General-aviation airfield. Module `airport`.
    Airfield,
    /// Container terminal (stacks, gantry cranes, gates). Module `docks`.
    ContainerTerminal,
    /// Quay apron with cranes / warehouses along the water. Module `docks`.
    Quay,
    /// Piers / ferry terminal / marina. Module `docks`.
    Piers,
    /// Rail yard (many parallel tracks, roundhouse). Module `rail`.
    RailYard,
    /// Passenger station (platforms, canopy). Module `rail`.
    Station,
    /// Multi-storey parking structure (drivable decks + ramps). Module `parking`.
    ParkingGarage,
    /// Surface parking lot (parked cars, paint, lamps). Module `parking`.
    SurfaceLot,
    /// Construction site. Module `construction`.
    Construction,
    /// Landmark building / complex (stadium, city hall, power plant, ...). Module `buildings`.
    Landmark,
    /// Park / forest / beach (trees, paths, no buildings). Module `scenery`.
    Park,
}

/// A vehicle entrance into a site: the road network must reach `pos` from outside.
#[derive(Debug, Clone, Copy)]
pub struct Gate {
    /// Gate position on the site boundary, m.
    pub pos: V2,
    /// Heading into the site (yaw, 0 = +Y, CCW), rad.
    pub heading: f32,
    /// Driveway width, m.
    pub width_m: f32,
}

/// A reserved area. Streets and generic buildings stay out of it (except a street that ends
/// at one of its gates); the owning module fills it.
#[derive(Debug, Clone)]
pub struct Site {
    /// Stable id (`snake_case`).
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// Kind (decides the owning module).
    pub kind: SiteKind,
    /// Boundary polygon (counter-clockwise), m.
    pub polygon: Vec<V2>,
    /// Level the ground to this height before roads are profiled (`None` = natural), m.
    pub pad_z: Option<f32>,
    /// Vehicle entrances.
    pub gates: Vec<Gate>,
}

/// Highway interchange type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterchangeKind {
    /// Full cloverleaf (four loops + four outer ramps, collector-distributor optional).
    Cloverleaf,
    /// Directional / stack interchange (flyovers, no loops).
    Stack,
    /// Trumpet (a spur ends at a through highway: one loop, one flyover).
    Trumpet,
    /// Diamond with a crossroad over or under the highway, two signalised terminals.
    Diamond,
    /// Half diamond (two ramps).
    HalfDiamond,
    /// Partial cloverleaf (parclo).
    Parclo,
}

/// A highway interchange.
#[derive(Debug, Clone, Copy)]
pub struct Interchange {
    /// Name.
    pub name: &'static str,
    /// Kind.
    pub kind: InterchangeKind,
    /// Approximate centre, m.
    pub pos: V2,
    /// Highway(s) involved (`Highway::name`).
    pub highway: &'static str,
    /// Crossing road (another highway or an arterial).
    pub crossroad: &'static str,
}

/// A planned highway.
#[derive(Debug, Clone)]
pub struct Highway {
    /// Name.
    pub name: &'static str,
    /// Control polygon / polyline (filleted by the road generator), m.
    pub ctrl: Vec<V2>,
    /// Closed loop.
    pub closed: bool,
    /// Lanes per direction.
    pub lanes: u8,
    /// Speed limit, m/s.
    pub speed_mps: f32,
    /// Design notes (elevated stretches, tunnels).
    pub notes: &'static str,
}

/// A planned arterial / named street.
#[derive(Debug, Clone)]
pub struct Arterial {
    /// Name.
    pub name: &'static str,
    /// Control polyline, m.
    pub ctrl: Vec<V2>,
    /// Lanes per direction.
    pub lanes: u8,
    /// Speed limit, m/s.
    pub speed_mps: f32,
    /// Notes (parking, bus lanes, special sections).
    pub notes: &'static str,
}

/// Kind of a planned crossing structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureKind {
    /// Fixed road bridge (deck + piers; `BridgeStructure` features for trusses/towers).
    Bridge,
    /// Bascule drawbridge (drawn with towers and counterweights; deck stays down).
    Drawbridge,
    /// Steel truss bridge.
    TrussBridge,
    /// Elevated viaduct.
    Viaduct,
    /// Tunnel (cut-and-cover under land, immersed tube under water).
    Tunnel,
    /// Covered trench / underpass.
    Underpass,
    /// Rail bridge (swing span).
    RailBridge,
}

/// A planned bridge / tunnel.
#[derive(Debug, Clone, Copy)]
pub struct Structure {
    /// Name.
    pub name: &'static str,
    /// Kind.
    pub kind: StructureKind,
    /// Road (or rail line) carried.
    pub carries: &'static str,
    /// Start and end (approximate), m.
    pub from: V2,
    /// End, m.
    pub to: V2,
}

/// Rail line kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RailKind {
    /// Freight.
    Freight,
    /// Passenger / commuter.
    Passenger,
}

/// A planned rail line.
#[derive(Debug, Clone)]
pub struct RailLine {
    /// Name.
    pub name: &'static str,
    /// Kind.
    pub kind: RailKind,
    /// Centreline (filleted by the rail module), m.
    pub line: Vec<V2>,
    /// Number of tracks.
    pub tracks: u8,
}

/// A named landmark the generators should build (by the module in `owner`).
#[derive(Debug, Clone, Copy)]
pub struct Landmark {
    /// Name.
    pub name: &'static str,
    /// Position, m.
    pub pos: V2,
    /// Owning module (`buildings`, `docks`, `rail`, `scenery`, `construction`, `airport`).
    pub owner: &'static str,
    /// What it is.
    pub note: &'static str,
}

/// The whole plan.
#[derive(Debug, Clone)]
pub struct Plan {
    /// River and canal.
    pub channels: Vec<Channel>,
    /// Sea (Calder Bay + the south sound): land is where this polygon is not, m.
    pub sea: Vec<Vec<V2>>,
    /// District zones (density and kind).
    pub zones: Vec<NamedZone>,
    /// Highways.
    pub highways: Vec<Highway>,
    /// Interchanges.
    pub interchanges: Vec<Interchange>,
    /// Arterials and named streets.
    pub arterials: Vec<Arterial>,
    /// Bridges and tunnels.
    pub structures: Vec<Structure>,
    /// Rail lines.
    pub rail: Vec<RailLine>,
    /// Reserved sites.
    pub sites: Vec<Site>,
    /// Landmarks.
    pub landmarks: Vec<Landmark>,
    /// Points of interest (snapped to the nearest lane by the pipeline).
    pub pois: Vec<PlannedPoi>,
}

/// A district zone with its name.
#[derive(Debug, Clone)]
pub struct NamedZone {
    /// Name.
    pub name: &'static str,
    /// The zone.
    pub zone: Zone,
    /// Base ground elevation hint, m.
    pub elevation: (f32, f32),
}

/// A planned POI.
#[derive(Debug, Clone)]
pub struct PlannedPoi {
    /// Kind.
    pub kind: PoiKind,
    /// Name.
    pub name: String,
    /// Position (z ignored; the pipeline snaps to a lane), m.
    pub pos: V2,
    /// Heading, rad.
    pub heading_rad: f32,
}

/// mph → m/s.
pub const fn mph(v: f32) -> f32 {
    v * 0.447_04
}

/// Axis-aligned rectangle polygon (CCW).
pub fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<V2> {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

/// Rectangle centred on `c` with size `w × h`.
pub fn rect_c(c: V2, w: f32, h: f32) -> Vec<V2> {
    rect(
        c[0] - w * 0.5,
        c[1] - h * 0.5,
        c[0] + w * 0.5,
        c[1] + h * 0.5,
    )
}

const N: f32 = 0.0;
const E: f32 = -core::f32::consts::FRAC_PI_2;
const S: f32 = core::f32::consts::PI;
const W: f32 = core::f32::consts::FRAC_PI_2;

fn gate(x: f32, y: f32, heading: f32) -> Gate {
    Gate {
        pos: [x, y],
        heading,
        width_m: 12.0,
    }
}

impl Plan {
    /// Site by id.
    pub fn site(&self, id: &str) -> Option<&Site> {
        self.sites.iter().find(|s| s.id == id)
    }

    /// Sites of one kind.
    pub fn sites_of(&self, kind: SiteKind) -> impl Iterator<Item = &Site> {
        self.sites.iter().filter(move |s| s.kind == kind)
    }

    /// Highway by name.
    pub fn highway(&self, name: &str) -> Option<&Highway> {
        self.highways.iter().find(|h| h.name == name)
    }

    /// The plan of Port Vesper v3.
    pub fn new() -> Self {
        Self {
            channels: channels(),
            sea: sea(),
            zones: zones(),
            highways: highways(),
            interchanges: interchanges(),
            arterials: arterials(),
            structures: structures(),
            rail: rail(),
            sites: sites(),
            landmarks: landmarks(),
            pois: pois(),
        }
    }
}

impl Default for Plan {
    fn default() -> Self {
        Self::new()
    }
}

fn channels() -> Vec<Channel> {
    vec![
        Channel {
            name: "Vesper River",
            line: vec![
                [-4500.0, -230.0],
                [-3900.0, -300.0],
                [-3300.0, -390.0],
                [-2700.0, -450.0],
                [-2000.0, -470.0],
                [-1200.0, -470.0],
                [-480.0, -460.0],
                [400.0, -460.0],
                [1500.0, -460.0],
                [2400.0, -470.0],
            ],
            half_width_m: 65.0,
            bed_z: -6.0,
            walled: false,
        },
        Channel {
            name: "Sluice Canal",
            line: vec![[-300.0, -480.0], [-300.0, -700.0], [-320.0, -930.0]],
            half_width_m: 16.0,
            bed_z: -3.0,
            walled: true,
        },
    ]
}

fn sea() -> Vec<Vec<V2>> {
    vec![
        // Calder Bay: east of x = 2300 north of the river mouth, east of 2800 south of it
        // (Gantry Row quay), east of 3300 along the reclaimed container terminal.
        vec![
            [2300.0, -380.0],
            [2300.0, 3500.0],
            [3700.0, 3500.0],
            [3700.0, -3700.0],
            [3300.0, -3700.0],
            [3300.0, -2250.0],
            [2800.0, -2250.0],
            [2800.0, -540.0],
            [2400.0, -540.0],
        ],
        // South sound: the shore runs along y = -3350 east of x = -3000.
        vec![
            [-3000.0, -3700.0],
            [3300.0, -3700.0],
            [3300.0, -3350.0],
            [-3000.0, -3350.0],
        ],
    ]
}

fn zone(
    name: &'static str,
    kind: DistrictKind,
    density_base: f32,
    polygon: Vec<V2>,
    elevation: (f32, f32),
) -> NamedZone {
    NamedZone {
        name,
        zone: Zone {
            kind,
            density_base,
            polygon,
        },
        elevation,
    }
}

fn zones() -> Vec<NamedZone> {
    use DistrictKind::*;
    vec![
        // CITY.md §2 (boxes as in data/city/handmade_v0.zones.json).
        zone(
            "Lantern Row",
            Core,
            30.0,
            rect(-300.0, -400.0, 1200.0, 800.0),
            (4.0, 12.0),
        ),
        zone(
            "Midtown",
            Commercial,
            22.0,
            rect(-1500.0, -400.0, -300.0, 800.0),
            (5.0, 15.0),
        ),
        zone(
            "Uptown",
            Commercial,
            20.0,
            rect(-300.0, 800.0, 1200.0, 1800.0),
            (12.0, 26.0),
        ),
        zone(
            "Old Harbor",
            Commercial,
            18.0,
            rect(1200.0, -400.0, 2300.0, 1100.0),
            (2.0, 5.0),
        ),
        zone(
            "Mercer",
            Residential,
            14.0,
            rect(-1500.0, 800.0, -300.0, 1800.0),
            (15.0, 25.0),
        ),
        zone(
            "Eastgate",
            Residential,
            12.0,
            rect(1200.0, 1100.0, 2300.0, 1800.0),
            (20.0, 30.0),
        ),
        zone(
            "Westline Yards",
            Industrial,
            8.0,
            rect(-3000.0, -400.0, -1500.0, 800.0),
            (3.0, 8.0),
        ),
        zone(
            "Larkspur",
            Suburb,
            5.0,
            rect(-3000.0, 800.0, -1500.0, 1800.0),
            (10.0, 30.0),
        ),
        zone(
            "Hollins Heights",
            Residential,
            7.0,
            rect(-3000.0, 1800.0, -1000.0, 3000.0),
            (25.0, 140.0),
        ),
        zone(
            "Northfield",
            Suburb,
            5.0,
            rect(-1000.0, 1800.0, 1000.0, 3000.0),
            (20.0, 40.0),
        ),
        zone(
            "Bayview Bluffs",
            Residential,
            9.0,
            rect(1000.0, 1800.0, 2300.0, 3000.0),
            (30.0, 55.0),
        ),
        zone(
            "Ferris Park",
            Residential,
            12.0,
            rect(-3000.0, -1700.0, -1500.0, -520.0),
            (5.0, 12.0),
        ),
        zone(
            "Canal Flats",
            Industrial,
            8.0,
            rect(-1500.0, -1700.0, 400.0, -520.0),
            (1.0, 4.0),
        ),
        zone(
            "Gantry Row",
            Port,
            6.0,
            rect(400.0, -1700.0, 2800.0, -520.0),
            (2.0, 4.0),
        ),
        zone(
            "Southmoor",
            Suburb,
            5.0,
            rect(-3000.0, -3000.0, -1500.0, -1700.0),
            (5.0, 15.0),
        ),
        zone(
            "Dunmore",
            Residential,
            11.0,
            rect(-1500.0, -3000.0, 400.0, -1700.0),
            (3.0, 8.0),
        ),
        zone(
            "Vesper Container Terminal",
            Port,
            5.0,
            rect(400.0, -3000.0, 2800.0, -1700.0),
            (2.0, 4.0),
        ),
        // v3 extensions.
        zone(
            "Vesper International",
            Commercial,
            4.0,
            rect(-4400.0, -3600.0, -3000.0, -600.0),
            (5.0, 7.0),
        ),
        zone(
            "Westline Outskirts",
            Industrial,
            5.0,
            rect(-4400.0, -600.0, -3000.0, 800.0),
            (4.0, 12.0),
        ),
        zone(
            "Westridge Woods",
            Suburb,
            3.0,
            rect(-4400.0, 800.0, -3000.0, 3400.0),
            (20.0, 90.0),
        ),
        zone(
            "Northfield Airfield",
            Suburb,
            3.0,
            rect(-1000.0, 3000.0, 2300.0, 3400.0),
            (35.0, 45.0),
        ),
        zone(
            "North Hollins",
            Residential,
            4.0,
            rect(-3000.0, 3000.0, -1000.0, 3400.0),
            (40.0, 90.0),
        ),
        zone(
            "South Shore",
            Suburb,
            4.0,
            rect(-3000.0, -3600.0, 400.0, -3000.0),
            (1.5, 6.0),
        ),
        zone(
            "Terminal Extension",
            Port,
            4.0,
            rect(2800.0, -3300.0, 3300.0, -2250.0),
            (2.5, 3.0),
        ),
    ]
}

fn highways() -> Vec<Highway> {
    vec![
        Highway {
            name: "Loop 9",
            ctrl: vec![
                [-2000.0, -2100.0],
                [1800.0, -2100.0],
                [1800.0, 1900.0],
                [-2000.0, 1900.0],
            ],
            closed: true,
            lanes: 3,
            speed_mps: mph(55.0),
            notes: "Rounded rectangle, corner radius 400 m. Divided 3+3 with a median barrier. \
                    East leg: the elevated Harbor Skyway (y 0..1900) then the Calloway Tunnel \
                    (y -900..0) under the river. West leg: the Westline Viaduct (y -900..0) over \
                    the yards and the river.",
        },
        Highway {
            name: "Route 40 West",
            ctrl: vec![[-4450.0, 340.0], [-3400.0, 330.0], [-2000.0, 300.0]],
            closed: false,
            lanes: 2,
            speed_mps: mph(65.0),
            notes: "From the west map edge to the Westline Cloverleaf with Loop 9, continuing \
                    east as the Crosstown Expressway.",
        },
        Highway {
            name: "Crosstown Expressway",
            ctrl: vec![
                [-2000.0, 300.0],
                [-1500.0, 800.0],
                [1300.0, 800.0],
                [1800.0, 500.0],
            ],
            closed: false,
            lanes: 3,
            speed_mps: mph(55.0),
            notes: "Elevated viaduct over the district line y = 800 (deck ~10 m) with diamonds \
                    at Mercer Street and Meridian Avenue; a stack interchange with Loop 9 (Harbor \
                    Skyway) at the east end.",
        },
        Highway {
            name: "Route 40 North",
            ctrl: vec![[-200.0, 1900.0], [-250.0, 2600.0], [-300.0, 3450.0]],
            closed: false,
            lanes: 2,
            speed_mps: mph(65.0),
            notes: "Trumpet with Loop 9 at the south end; north to the map edge.",
        },
        Highway {
            name: "Airport Expressway",
            ctrl: vec![[-2000.0, -1500.0], [-2700.0, -1600.0], [-3150.0, -1650.0]],
            closed: false,
            lanes: 2,
            speed_mps: mph(55.0),
            notes: "Trumpet with Loop 9 at the east end, diamond at Westline Avenue, ends in the \
                    airport's one-way terminal loop at the airport gate.",
        },
        Highway {
            name: "Port Connector",
            ctrl: vec![[900.0, -2100.0], [1000.0, -2350.0], [1500.0, -2500.0]],
            closed: false,
            lanes: 2,
            speed_mps: mph(45.0),
            notes: "Truck route: trumpet with Loop 9's south leg, ends at the container \
                    terminal main gate.",
        },
    ]
}

fn interchanges() -> Vec<Interchange> {
    use InterchangeKind::*;
    let ic = |name, kind, pos, highway, crossroad| Interchange {
        name,
        kind,
        pos,
        highway,
        crossroad,
    };
    vec![
        ic(
            "Westline Cloverleaf",
            Cloverleaf,
            [-2000.0, 300.0],
            "Loop 9",
            "Route 40 West / Crosstown Expressway",
        ),
        ic(
            "Skyway Stack",
            Stack,
            [1800.0, 500.0],
            "Loop 9",
            "Crosstown Expressway",
        ),
        ic(
            "North Trumpet",
            Trumpet,
            [-200.0, 1900.0],
            "Loop 9",
            "Route 40 North",
        ),
        ic(
            "Airport Junction",
            Trumpet,
            [-2000.0, -1500.0],
            "Loop 9",
            "Airport Expressway",
        ),
        ic(
            "Port Trumpet",
            Trumpet,
            [900.0, -2100.0],
            "Loop 9",
            "Port Connector",
        ),
        ic(
            "Meridian Avenue (south)",
            Diamond,
            [400.0, -2100.0],
            "Loop 9",
            "Meridian Avenue",
        ),
        ic(
            "Harbor Boulevard",
            Diamond,
            [1800.0, 1150.0],
            "Loop 9",
            "Founders Parkway",
        ),
        ic(
            "Dunmore Avenue",
            Diamond,
            [-600.0, -2100.0],
            "Loop 9",
            "Dunmore Avenue",
        ),
        ic(
            "Old Post Road",
            Parclo,
            [-2000.0, -2000.0],
            "Loop 9",
            "Old Post Road",
        ),
        ic(
            "Mercer Street",
            Diamond,
            [-900.0, 800.0],
            "Crosstown Expressway",
            "Mercer Street",
        ),
        ic(
            "Meridian Avenue (Crosstown)",
            Diamond,
            [400.0, 800.0],
            "Crosstown Expressway",
            "Meridian Avenue",
        ),
        ic(
            "Westline Avenue (airport)",
            Diamond,
            [-2500.0, -1570.0],
            "Airport Expressway",
            "Westline Avenue",
        ),
        ic(
            "Route 40 West (outskirts)",
            Diamond,
            [-3400.0, 330.0],
            "Route 40 West",
            "Skyline Drive",
        ),
        ic(
            "Founders Parkway (north)",
            HalfDiamond,
            [-2000.0, 1300.0],
            "Loop 9",
            "Founders Parkway",
        ),
    ]
}

fn arterials() -> Vec<Arterial> {
    let a = |name, ctrl, lanes, mph_: f32, notes| Arterial {
        name,
        ctrl,
        lanes,
        speed_mps: mph(mph_),
        notes,
    };
    vec![
        a("Meridian Avenue", vec![[400.0, -3250.0], [400.0, 2980.0]], 2, 40.0,
          "North-south spine, edge to edge; Meridian Bridge (high, fixed) over the river."),
        a("Kettle Street", vec![[1500.0, 1800.0], [1500.0, -1700.0]], 2, 35.0,
          "Downhill all the way south; Kettle Street Drawbridge over the river."),
        a("Harbor Boulevard", vec![[2150.0, -300.0], [2150.0, 600.0], [2050.0, 1900.0]], 2, 30.0,
          "Waterfront, parking both sides, ends at the ferry terminal in the south."),
        a("Lantern Way", vec![[-2900.0, 150.0], [2150.0, 150.0]], 2, 35.0,
          "Bus lanes downtown; dips under Meridian in the Lantern Underpass; eastbound green wave."),
        a("Founders Parkway", vec![[-2900.0, 1300.0], [2150.0, 1300.0]], 2, 40.0,
          "Passes under the civic plaza in Founders Cut (x 250..650)."),
        a("Canal Street", vec![[-2900.0, -1000.0], [2750.0, -1000.0]], 2, 40.0,
          "Long, flat, empty at night: the drag strip."),
        a("Terminal Avenue", vec![[-2900.0, -2400.0], [1450.0, -2400.0]], 2, 40.0,
          "Along the container terminal fence."),
        a("Old Post Road", vec![[-2600.0, -2950.0], [-1006.0, -1003.0], [-480.0, -460.0], [0.0, 150.0]], 1, 35.0,
          "Colonial diagonal: 1+1 over the narrow Iron Bridge, 2+2 elsewhere; meets Canal Street and the freight line at Five Points."),
        a("Westline Avenue", vec![[-2500.0, -2950.0], [-2500.0, 1800.0]], 2, 35.0,
          "West side; Westline Low Bridge next to a rail level crossing 400 m north."),
        a("Dunmore Avenue", vec![[-600.0, -3150.0], [-600.0, -520.0]], 2, 30.0, "South bank."),
        a("Mercer Street", vec![[-900.0, -400.0], [-900.0, 1800.0]], 1, 25.0,
          "Parking both sides; school zone."),
        a("Bluff Road", vec![[1000.0, 2950.0], [1700.0, 2700.0], [2250.0, 2300.0], [2200.0, 1850.0]], 1, 30.0,
          "Along the Bayview cliff edge; guard rail on the sea side."),
        a("Hollins Drive", vec![[-1900.0, 1950.0], [-2300.0, 2550.0]], 1, 25.0,
          "Switchback climb (target ~4.2 km of road, 11 hairpins) to the Knob Observatory Lot."),
        a("Skyline Drive", vec![[-3400.0, 330.0], [-3500.0, 1300.0], [-3200.0, 2300.0], [-2300.0, 2550.0]], 1, 35.0,
          "Scenic 1+1 through Westridge Woods with the Westridge Tunnel (x -3550..-3450, y 1150..1550) and up to the Knob."),
        a("Airport Road", vec![[-3150.0, -1650.0], [-3500.0, -1650.0], [-3500.0, -900.0], [-3200.0, -700.0], [-2700.0, -650.0]], 2, 35.0,
          "Airport terminal loop and the cargo road north to Westline Avenue."),
        a("Airfield Road", vec![[400.0, 2980.0], [400.0, 3050.0], [900.0, 3050.0]], 1, 25.0,
          "From Meridian Avenue's north end to the airfield gate."),
        a("Shore Drive", vec![[-2900.0, -3200.0], [300.0, -3200.0]], 1, 35.0,
          "Along the South Shore beach park to the power station."),
        a("Riverside Drive", vec![[-4300.0, -150.0], [-3600.0, -200.0], [-3600.0, -650.0]], 1, 30.0,
          "West outskirts: crosses the river on the Riverside Bridge (x -3600)."),
    ]
}

fn structures() -> Vec<Structure> {
    use StructureKind::*;
    let s = |name, kind, carries, from, to| Structure {
        name,
        kind,
        carries,
        from,
        to,
    };
    vec![
        s(
            "Westline Low Bridge",
            Bridge,
            "Westline Avenue",
            [-2500.0, -560.0],
            [-2500.0, -380.0],
        ),
        s(
            "Westline Viaduct",
            Viaduct,
            "Loop 9",
            [-2000.0, -900.0],
            [-2000.0, 0.0],
        ),
        s(
            "Westline Swing Bridge",
            RailBridge,
            "Westline Freight Line",
            [-1700.0, -580.0],
            [-1700.0, -370.0],
        ),
        s(
            "Iron Bridge",
            TrussBridge,
            "Old Post Road",
            [-560.0, -560.0],
            [-400.0, -370.0],
        ),
        s(
            "Meridian Bridge",
            Bridge,
            "Meridian Avenue",
            [400.0, -580.0],
            [400.0, -340.0],
        ),
        s(
            "Kettle Street Drawbridge",
            Drawbridge,
            "Kettle Street",
            [1500.0, -560.0],
            [1500.0, -360.0],
        ),
        s(
            "Calloway Tunnel",
            Tunnel,
            "Loop 9",
            [1800.0, -900.0],
            [1800.0, 0.0],
        ),
        s(
            "Harbor Skyway",
            Viaduct,
            "Loop 9",
            [1800.0, 0.0],
            [1800.0, 1500.0],
        ),
        s(
            "Crosstown Viaduct",
            Viaduct,
            "Crosstown Expressway",
            [-1500.0, 800.0],
            [1300.0, 800.0],
        ),
        s(
            "Founders Cut",
            Underpass,
            "Founders Parkway",
            [250.0, 1300.0],
            [650.0, 1300.0],
        ),
        s(
            "Lantern Underpass",
            Underpass,
            "Lantern Way",
            [330.0, 150.0],
            [470.0, 150.0],
        ),
        s(
            "Westridge Tunnel",
            Tunnel,
            "Skyline Drive",
            [-3550.0, 1150.0],
            [-3450.0, 1550.0],
        ),
        s(
            "Riverside Bridge",
            Bridge,
            "Riverside Drive",
            [-3600.0, -470.0],
            [-3600.0, -230.0],
        ),
        s(
            "Central Rail Bridge",
            RailBridge,
            "Vesper Central Line",
            [-900.0, -100.0],
            [-900.0, 100.0],
        ),
    ]
}

fn rail() -> Vec<RailLine> {
    vec![
        RailLine {
            name: "Westline Freight Line",
            kind: RailKind::Freight,
            line: vec![
                [2550.0, -1300.0],
                [-600.0, -1300.0],
                [-1006.0, -1003.0],
                [-1700.0, -850.0],
                [-1700.0, 0.0],
                [-4450.0, 0.0],
            ],
            tracks: 2,
        },
        RailLine {
            name: "Terminal Spur",
            kind: RailKind::Freight,
            line: vec![
                [1300.0, -1300.0],
                [1600.0, -1600.0],
                [1650.0, -2250.0],
                [2300.0, -2700.0],
                [3100.0, -2700.0],
            ],
            tracks: 1,
        },
        RailLine {
            name: "Vesper Central Line",
            kind: RailKind::Passenger,
            line: vec![
                [-1900.0, 40.0],
                [-1200.0, 60.0],
                [-900.0, 200.0],
                [-760.0, 330.0],
            ],
            tracks: 2,
        },
    ]
}

fn sites() -> Vec<Site> {
    use SiteKind::*;
    let site = |id, name, kind, polygon, pad_z, gates| Site {
        id,
        name,
        kind,
        polygon,
        pad_z,
        gates,
    };
    vec![
        // --- Airport / airfield ---
        site(
            "airport",
            "Vesper International Airport",
            Airport,
            rect(-4350.0, -3500.0, -3550.0, -900.0),
            Some(6.0),
            vec![gate(-3550.0, -1650.0, W), gate(-3550.0, -1000.0, W)],
        ),
        site(
            "airport_landside",
            "Airport terminal & parking",
            Airport,
            rect(-3550.0, -2200.0, -3150.0, -1100.0),
            Some(6.0),
            vec![gate(-3150.0, -1650.0, W)],
        ),
        site(
            "airfield",
            "Northfield Airfield",
            Airfield,
            rect(100.0, 3060.0, 1900.0, 3370.0),
            Some(40.0),
            vec![gate(900.0, 3060.0, N)],
        ),
        // --- Port ---
        site(
            "container_terminal",
            "Vesper Container Terminal",
            ContainerTerminal,
            rect(1500.0, -3315.0, 3300.0, -2250.0),
            Some(2.8),
            vec![gate(1500.0, -2500.0, E), gate(1500.0, -2950.0, E)],
        ),
        site(
            "gantry_quay",
            "Gantry Row Quay",
            Quay,
            rect(2560.0, -1650.0, 2800.0, -600.0),
            Some(2.6),
            vec![gate(2560.0, -1300.0, E), gate(2560.0, -900.0, E)],
        ),
        site(
            "old_harbor_piers",
            "Old Harbor Piers & Ferry Terminal",
            Piers,
            vec![
                [2250.0, -330.0],
                [2600.0, -330.0],
                [2600.0, 900.0],
                [2250.0, 900.0],
            ],
            None,
            vec![
                gate(2250.0, -150.0, E),
                gate(2250.0, 200.0, E),
                gate(2250.0, 500.0, E),
                gate(2250.0, 700.0, E),
            ],
        ),
        // --- Rail ---
        site(
            "westline_yard",
            "Westline Rail Yard",
            RailYard,
            rect(-2950.0, -130.0, -2050.0, 160.0),
            Some(5.0),
            vec![gate(-2500.0, 160.0, S)],
        ),
        site(
            "roundhouse",
            "Westline Roundhouse",
            RailYard,
            rect_c([-2300.0, 400.0], 220.0, 180.0),
            Some(6.0),
            vec![gate(-2300.0, 490.0, S)],
        ),
        site(
            "vesper_central",
            "Vesper Central Station",
            Station,
            rect(-900.0, 250.0, -600.0, 420.0),
            None,
            vec![gate(-750.0, 420.0, S)],
        ),
        // --- Parking ---
        site(
            "lantern_garage",
            "Lantern Garage",
            ParkingGarage,
            rect_c([600.0, 400.0], 80.0, 60.0),
            None,
            vec![gate(600.0, 370.0, N)],
        ),
        site(
            "midtown_mall_garage",
            "Midtown Mall Garage",
            ParkingGarage,
            rect_c([-1150.0, 550.0], 90.0, 70.0),
            None,
            vec![gate(-1150.0, 515.0, N)],
        ),
        site(
            "harbor_garage",
            "Old Harbor Garage",
            ParkingGarage,
            rect_c([1700.0, 250.0], 80.0, 60.0),
            None,
            vec![gate(1660.0, 250.0, E)],
        ),
        site(
            "airport_garage",
            "Airport Parking Structure",
            ParkingGarage,
            rect(-3475.0, -2030.0, -3375.0, -1950.0),
            Some(6.0),
            vec![gate(-3425.0, -1950.0, S)],
        ),
        site(
            "uptown_garage",
            "Founders Plaza Garage",
            ParkingGarage,
            rect_c([800.0, 1450.0], 70.0, 60.0),
            None,
            vec![gate(765.0, 1450.0, E)],
        ),
        site(
            "stadium_lot",
            "Vesper Bowl parking",
            SurfaceLot,
            rect(-1100.0, -2350.0, -800.0, -2150.0),
            None,
            vec![gate(-950.0, -2350.0, N)],
        ),
        site(
            "northfield_mall_lot",
            "Northfield Mall parking",
            SurfaceLot,
            rect(150.0, 2100.0, 350.0, 2250.0),
            None,
            vec![gate(150.0, 2175.0, E)],
        ),
        site(
            "park_and_ride",
            "Larkspur Park & Ride",
            SurfaceLot,
            rect_c([-2200.0, 1200.0], 160.0, 110.0),
            None,
            vec![gate(-2200.0, 1255.0, S)],
        ),
        site(
            "airport_long_stay",
            "Airport long-stay lot",
            SurfaceLot,
            rect(-3500.0, -2550.0, -3200.0, -2250.0),
            Some(6.0),
            vec![gate(-3350.0, -2250.0, S)],
        ),
        site(
            "drive_in",
            "Starlite Drive-In",
            SurfaceLot,
            rect_c([-2300.0, -1150.0], 200.0, 160.0),
            None,
            vec![gate(-2200.0, -1150.0, W)],
        ),
        // --- Construction ---
        site(
            "lantern_tower_site",
            "Lantern Tower construction",
            Construction,
            rect_c([700.0, -150.0], 120.0, 100.0),
            None,
            vec![gate(700.0, -100.0, S)],
        ),
        site(
            "unfinished_exchange",
            "The Unfinished Exchange",
            Construction,
            rect(-900.0, 2450.0, -450.0, 2750.0),
            None,
            vec![gate(-450.0, 2600.0, W)],
        ),
        site(
            "canal_earthworks",
            "Canal Flats earthworks",
            Construction,
            rect_c([-150.0, -1550.0], 300.0, 200.0),
            None,
            vec![gate(-150.0, -1450.0, S)],
        ),
        site(
            "eastgate_block",
            "Eastgate apartment block",
            Construction,
            rect_c([1650.0, 1450.0], 100.0, 80.0),
            None,
            vec![gate(1600.0, 1450.0, E)],
        ),
        // --- Landmarks ---
        site(
            "vesper_bowl",
            "Vesper Bowl stadium",
            Landmark,
            rect_c([-950.0, -2000.0], 280.0, 240.0),
            None,
            vec![gate(-950.0, -2120.0, N)],
        ),
        site(
            "founders_plaza",
            "Founders Plaza & City Hall",
            Landmark,
            rect_c([450.0, 1380.0], 260.0, 120.0),
            None,
            vec![],
        ),
        site(
            "calder_station",
            "Calder Power Station",
            Landmark,
            rect(0.0, -3330.0, 380.0, -3050.0),
            Some(3.0),
            vec![gate(150.0, -3050.0, S)],
        ),
        site(
            "steel_mill",
            "Old Vesper Steel Mill",
            Landmark,
            rect(-850.0, -960.0, -550.0, -780.0),
            None,
            vec![gate(-700.0, -960.0, N)],
        ),
        site(
            "observatory",
            "Hollins Knob Observatory",
            Landmark,
            rect_c([-2300.0, 2600.0], 120.0, 100.0),
            None,
            vec![],
        ),
        site(
            "northfield_mall",
            "Northfield Mall",
            Landmark,
            rect(150.0, 2250.0, 450.0, 2400.0),
            None,
            vec![],
        ),
        site(
            "midtown_mall",
            "Midtown Galleria",
            Landmark,
            rect_c([-1150.0, 650.0], 200.0, 110.0),
            None,
            vec![],
        ),
        site(
            "lighthouse",
            "Calder Point Lighthouse",
            Landmark,
            rect_c([2230.0, 3150.0], 60.0, 60.0),
            None,
            vec![],
        ),
        // --- Parks ---
        site(
            "lantern_park",
            "Lantern Promenade",
            Park,
            rect(900.0, -400.0, 1150.0, -330.0),
            None,
            vec![],
        ),
        site(
            "ferris_park",
            "Ferris Park",
            Park,
            rect_c([-2000.0, -900.0], 260.0, 200.0),
            None,
            vec![],
        ),
        site(
            "south_shore_park",
            "South Shore Beach",
            Park,
            rect(-2950.0, -3340.0, -100.0, -3220.0),
            None,
            vec![],
        ),
        site(
            "westridge_forest",
            "Westridge State Forest",
            Park,
            rect(-4350.0, 900.0, -3700.0, 3350.0),
            None,
            vec![],
        ),
        site(
            "mercer_park",
            "Mercer Green",
            Park,
            rect_c([-600.0, 1500.0], 180.0, 160.0),
            None,
            vec![],
        ),
    ]
}

fn landmarks() -> Vec<Landmark> {
    let l = |name, pos, owner, note| Landmark {
        name,
        pos,
        owner,
        note,
    };
    vec![
        l(
            "Hollins Knob Observatory",
            [-2300.0, 2600.0],
            "buildings",
            "Dome + lot at 140 m; safehouse SH06.",
        ),
        l(
            "Gantry cranes",
            [2700.0, -1100.0],
            "docks",
            "Ship-to-shore cranes along the quays, red aircraft lights.",
        ),
        l(
            "Kettle Street Drawbridge towers",
            [1500.0, -460.0],
            "roads",
            "Bascule towers and counterweights either side.",
        ),
        l(
            "Iron Bridge truss",
            [-480.0, -460.0],
            "roads",
            "1890s steel truss over a 1+1 deck.",
        ),
        l(
            "Founders Plaza",
            [450.0, 1380.0],
            "buildings",
            "City hall over Founders Cut.",
        ),
        l(
            "Starlite Drive-In screen",
            [-2350.0, -1150.0],
            "parking",
            "Derelict screen tower; SH10.",
        ),
        l(
            "Westline Roundhouse",
            [-2300.0, 400.0],
            "rail",
            "Disused roundhouse with a turntable; SH05.",
        ),
        l(
            "Pier 9 boathouse",
            [2450.0, 700.0],
            "docks",
            "End of a pier; SH02.",
        ),
        l(
            "Vesper Bowl",
            [-950.0, -2000.0],
            "buildings",
            "Football stadium, bowl stands, floodlight towers.",
        ),
        l(
            "Calder Power Station",
            [190.0, -3190.0],
            "buildings",
            "Two cooling towers, turbine hall, chimney.",
        ),
        l(
            "Old Vesper Steel Mill",
            [-700.0, -870.0],
            "buildings",
            "Ruined blast furnace and sheds.",
        ),
        l(
            "Calder Point Lighthouse",
            [2230.0, 3150.0],
            "buildings",
            "On the bluff tip.",
        ),
        l(
            "Lantern Tower",
            [700.0, -150.0],
            "construction",
            "Half-built skyscraper, two tower cranes.",
        ),
        l(
            "The Unfinished Exchange",
            [-675.0, 2600.0],
            "construction",
            "Abandoned flyover ramps ending in mid-air (a jump).",
        ),
        l(
            "Vesper Central",
            [-750.0, 335.0],
            "rail",
            "Terminus with a train shed over the platforms.",
        ),
        l(
            "Airport control tower",
            [-3600.0, -2300.0],
            "airport",
            "Tall tower beside the terminal.",
        ),
        l(
            "Westridge Tunnel portals",
            [-3500.0, 1350.0],
            "roads",
            "Stone portals.",
        ),
        l(
            "Ferry terminal",
            [2400.0, -150.0],
            "docks",
            "Ferry berths and a moored ferry.",
        ),
        l(
            "Water towers",
            [-2400.0, -2200.0],
            "scenery",
            "Suburban water towers (Southmoor, Larkspur, Northfield).",
        ),
    ]
}

fn pois() -> Vec<PlannedPoi> {
    // CITY.md §5 (as in data/city/handmade_v0.zones.json) plus v3 additions.
    let raw = include_str!("../../../../data/city/handmade_v0.zones.json");
    let v: serde_json::Value = serde_json::from_str(raw).expect("handmade_v0.zones.json parses");
    let mut out: Vec<PlannedPoi> = v["pois"]
        .as_array()
        .expect("pois array")
        .iter()
        .filter_map(|p| {
            let kind: PoiKind = serde_json::from_value(p["kind"].clone()).ok()?;
            let pos = p["pos"].as_array()?;
            Some(PlannedPoi {
                kind,
                name: p["name"].as_str()?.to_string(),
                pos: [pos[0].as_f64()? as f32, pos[1].as_f64()? as f32],
                heading_rad: p["heading_rad"].as_f64().unwrap_or(0.0) as f32,
            })
        })
        .collect();
    let extra = [
        (
            PoiKind::RaceStartGrid,
            "R16 Runway Drag",
            [-3150.0, -1650.0],
            W,
        ),
        (
            PoiKind::RaceStartGrid,
            "R17 Crosstown Express",
            [-1400.0, 800.0],
            E,
        ),
        (
            PoiKind::RaceStartGrid,
            "R18 Skyline Drive",
            [-3400.0, 400.0],
            N,
        ),
        (
            PoiKind::RaceStartGrid,
            "R19 Calloway Plunge",
            [1800.0, 300.0],
            S,
        ),
        (
            PoiKind::Garage,
            "Airport Rent-A-Wreck",
            [-3300.0, -1800.0],
            N,
        ),
        (
            PoiKind::SafehouseCandidate,
            "SH13 Hangar 3",
            [600.0, 3060.0],
            N,
        ),
        (
            PoiKind::SafehouseCandidate,
            "SH14 Unfinished Exchange",
            [-460.0, 2600.0],
            W,
        ),
        (
            PoiKind::Helipad,
            "Airport Police Pad",
            [-3300.0, -2700.0],
            N,
        ),
        (
            PoiKind::PoliceStation,
            "Airport Police",
            [-3250.0, -2650.0],
            E,
        ),
    ];
    for (kind, name, pos, heading_rad) in extra {
        out.push(PlannedPoi {
            kind,
            name: name.into(),
            pos,
            heading_rad,
        });
    }
    let _ = (N, E, S, W);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_is_inside_the_map_and_sites_do_not_overlap_carelessly() {
        let p = Plan::new();
        for s in &p.sites {
            for q in &s.polygon {
                assert!(
                    q[0] >= MAP_LO[0]
                        && q[0] <= MAP_HI[0]
                        && q[1] >= MAP_LO[1]
                        && q[1] <= MAP_HI[1],
                    "{} outside the map",
                    s.id
                );
            }
        }
        assert!(p.pois.len() >= 40);
        let ids: std::collections::BTreeSet<_> = p.sites.iter().map(|s| s.id).collect();
        assert_eq!(ids.len(), p.sites.len(), "site ids unique");
    }
}
