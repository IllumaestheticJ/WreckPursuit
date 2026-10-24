//! The Ashpeak County master plan: map extent, district areas, connection points to Port
//! Vesper and the freeway corridors. Every district file keeps to these numbers, so districts
//! laid out by different files meet where they should and never overlap.
//!
//! ```text
//!   y
//!  5600 ┌──────────────────────────────────────────────────────────────┐
//!       │  farms (NW)   │  Route 12 │      ASHPEAK CITY (metro)    │R7│ sea
//!  4000 │               │     ══════ Crosstown Expressway ═════════│══│
//!       │   foothills   │ Grizzly   │  ▲ Crowfoot Pass (touge)     │ Silver Gorge Br.
//!  2600 │               │ Grade     │  ▲▲ Ashpeak ▲  saddle   ▲▲   │Talon Tunnel
//!       │  HARNELL FLATS │ (R12 pass)│      ASHPEAK RANGE           │R7│
//!  1750 │  (farmland)   ├───────────┴──────────────────────────────┴──┤
//!       │   Sable R.    │                PORT VESPER                   │
//! -1300 └──────────────────────────────────────────────────────────────┘
//!     -4600          -1700                                          1700  x
//! ```

use crate::geom::V2;

/// Combined map extent (Port Vesper is `[-1700, -1300] .. [1700, 1750]` inside it), m.
pub const LO: V2 = [-4600.0, -1300.0];
/// Combined map extent, upper corner, m.
pub const HI: V2 = [1700.0, 5600.0];
/// Heightfield vertex spacing of the combined map, m.
pub const TERRAIN_CELL_M: f32 = 8.0;

/// Port Vesper's own map extent (its generator's `MAP_LO`/`MAP_HI`), m.
pub const VESPER_LO: V2 = [-1700.0, -1300.0];
/// Port Vesper's own map extent, upper corner, m.
pub const VESPER_HI: V2 = [1700.0, 1750.0];
/// Ground blends from Port Vesper's into Ashpeak County's over this distance outside
/// Port Vesper's extent, m. No Ashpeak road comes closer to Port Vesper's edge than its
/// connectors.
pub const BLEND_M: f32 = 250.0;

// ---- Connection points (Port Vesper's dead ends and interchange outer terminals) ----------

/// Route 40 North's dead end at Port Vesper's north edge: Crowfoot Pass Road starts here
/// ([`super::mountains`]).
pub const R40N_END: V2 = [0.0, 1650.0];
/// Route 40 West's dead end at Port Vesper's west edge: Harnell Road starts here
/// ([`super::farms`]).
pub const R40W_END: V2 = [-1650.0, 100.0];
/// Near the Uptown Connector interchange on Loop 9 (north): Route 7 starts at its outer
/// terminal ([`super::freeways`]; find the node with `outer_terminal`).
pub const UPTOWN_SITE: V2 = [400.0, 1400.0];
/// Near the Lantern Way interchange on Loop 9 (west): Route 12 starts at its outer terminal.
pub const LANTERN_SITE: V2 = [-1515.0, 420.0];

// ---- Districts ----------------------------------------------------------------------------

/// Harnell Flats farmland (x < this west of Port Vesper, south of [`RANGE_SOUTH_Y`] plus the
/// north-west foothill farms), m.
pub const FARMS_EAST_X: f32 = -1700.0;
/// The Sable River runs south to the sea near this x (meanders ± 150 m), m.
pub const SABLE_RIVER_X: f32 = -2500.0;
/// Harnell Crossing, the farm village on Harnell Road.
pub const HARNELL_CROSSING: V2 = [-3300.0, 150.0];

/// Southern foot of the Ashpeak Range, m.
pub const RANGE_SOUTH_Y: f32 = 1900.0;
/// Northern foot of the Ashpeak Range, m.
pub const RANGE_NORTH_Y: f32 = 3450.0;
/// Ashpeak summit (≈ 560 m): Halcyon Observatory.
pub const ASHPEAK_SUMMIT: V2 = [-1900.0, 2750.0];
/// Crowfoot saddle, the touge's crest (≈ 250 m).
pub const CROWFOOT_SADDLE: V2 = [-250.0, 2620.0];
/// Grizzly saddle, Route 12's mountain-pass crest (≈ 280 m).
pub const GRIZZLY_SADDLE: V2 = [-3000.0, 2700.0];
/// Talon Ridge, the eastern ridge Route 7 tunnels through (portals ≈ y 2350 and 2850).
pub const TALON_RIDGE: V2 = [850.0, 2600.0];
/// The Silver River gorge crosses Route 7 here (flows east to the sea), m.
pub const SILVER_GORGE: V2 = [800.0, 3300.0];

/// Ashpeak City downtown core: block grid between these corners, m.
pub const METRO_CORE_LO: V2 = [-1300.0, 4150.0];
/// Ashpeak City downtown core, upper corner, m.
pub const METRO_CORE_HI: V2 = [-100.0, 5150.0];
/// Ashpeak City as a whole (core + midtown + waterfront + inner suburbs), m.
pub const METRO_LO: V2 = [-2100.0, 3650.0];
/// Ashpeak City as a whole, upper corner, m.
pub const METRO_HI: V2 = [1600.0, 5600.0];

// ---- Freeway corridors (centrelines; [`super::freeways`] lays them) ------------------------
//
// Other districts may cross a freeway (the crossing becomes a bridge automatically) but keep
// at-grade junctions and buildings 60 m away from these lines.

/// Route 7: from the Uptown Connector's outer terminal north through the Talon Tunnel, over
/// the Silver Gorge Bridge and up the east side of Ashpeak City.
pub const ROUTE_7: &[V2] = &[
    [400.0, 1560.0],
    [620.0, 1950.0],
    [850.0, 2350.0],
    [860.0, 2850.0],
    [800.0, 3300.0],
    [700.0, 3950.0],
    [550.0, 4700.0],
    [500.0, 5500.0],
];

/// Route 12: from Lantern Way's outer terminal west through Harnell Flats, north up the
/// valley, over the Grizzly Grade (mountain pass) and down the west side of Ashpeak City.
pub const ROUTE_12: &[V2] = &[
    [-1680.0, 420.0],
    [-2150.0, 650.0],
    [-2750.0, 1350.0],
    [-2950.0, 2100.0],
    [-3000.0, 2700.0],
    [-2750.0, 3400.0],
    [-2300.0, 3950.0],
    [-2100.0, 4700.0],
    [-2050.0, 5500.0],
];

/// Crosstown Expressway: east–west south of downtown, Route 12 to Route 7.
pub const CROSSTOWN: &[V2] = &[[-2300.0, 3950.0], [-800.0, 3950.0], [700.0, 3950.0]];

/// Distance every non-freeway junction and building keeps from a freeway corridor, m.
pub const FREEWAY_CLEAR_M: f32 = 60.0;

// ---- Points of interest every district must create (races and AI racers use the names) ---
//
// Race grids (`PoiKind::RaceStartGrid`, pole position facing the race direction):

/// Bottom of Crowfoot Pass Road, facing uphill ([`super::mountains`]).
pub const POI_CROWFOOT_ASCENT: &str = "Crowfoot Ascent";
/// Crowfoot saddle, facing downhill towards Ashpeak City ([`super::mountains`]).
pub const POI_CROWFOOT_SUMMIT: &str = "Crowfoot Summit";
/// North foot of Crowfoot Pass Road, facing uphill southwards ([`super::mountains`]).
pub const POI_CROWFOOT_NORTH: &str = "Crowfoot North Foot";
/// Halcyon Observatory lot at the Ashpeak summit (also a safehouse candidate)
/// ([`super::mountains`]).
pub const POI_OBSERVATORY: &str = "Halcyon Observatory";
/// Start of the farm dirt run on a dirt / gravel road ([`super::farms`]).
pub const POI_FARM_RUN: &str = "Harnell Farm Run";
/// Harnell Crossing village centre on Harnell Road ([`super::farms`]).
pub const POI_HARNELL_CROSSING: &str = "Harnell Crossing";
/// Downtown Ashpeak City start (core grid avenue) ([`super::metro`]).
pub const POI_DOWNTOWN: &str = "Ashpeak Downtown";
/// At the foot of the Meridian Spire ([`super::metro`]).
pub const POI_SPIRE: &str = "Meridian Spire";
/// Ashpeak City waterfront boulevard start ([`super::metro`]).
pub const POI_WATERFRONT: &str = "Ashpeak Waterfront";
/// Route 7 northbound, just north of the Uptown Connector ([`super::freeways`]).
pub const POI_ROUTE7_SOUTH: &str = "Route 7 South";
/// Route 7 near its north end ([`super::freeways`]).
pub const POI_ROUTE7_NORTH: &str = "Route 7 North";
/// Route 12 northbound at the foot of the Grizzly Grade ([`super::freeways`]).
pub const POI_GRIZZLY_GRADE: &str = "Grizzly Grade";
/// Crosstown Expressway eastbound, west end ([`super::freeways`]).
pub const POI_CROSSTOWN_WEST: &str = "Crosstown West";

// ---- Interchanges ---------------------------------------------------------------------------

/// An interchange between a freeway and a district road: [`super::freeways`] lays the
/// crossroad from terminal `a` to terminal `b` (it bridges the freeway) and the ramps; the
/// district named `a_by` / `b_by` joins its roads to the terminal node at exactly that
/// position (`g.node(pos)` returns the same node). Districts keep every other road
/// [`FREEWAY_CLEAR_M`] away from the crossroad.
#[derive(Debug, Clone, Copy)]
pub struct InterchangePlan {
    /// Interchange name (also the crossroad's road name).
    pub name: &'static str,
    /// Freeway it is on.
    pub freeway: &'static str,
    /// Terminal on one side, m.
    pub a: V2,
    /// District that connects to `a` (`"farms"`, `"mountains"`, `"metro"`).
    pub a_by: &'static str,
    /// Terminal on the other side, m.
    pub b: V2,
    /// District that connects to `b`.
    pub b_by: &'static str,
}

/// Every freeway interchange with a district road. Freeway-to-freeway junctions (Crosstown
/// with Route 7 and Route 12) and the freeways' own ends are [`super::freeways`]' business;
/// see [`FREEWAY_ENDS`].
pub const INTERCHANGES: &[InterchangePlan] = &[
    InterchangePlan {
        name: "Harnell Road",
        freeway: "Route 12",
        a: [-2650.0, 1040.0],
        a_by: "farms",
        b: [-2320.0, 1040.0],
        b_by: "farms",
    },
    InterchangePlan {
        name: "Ashpeak West",
        freeway: "Route 12",
        a: [-2320.0, 4500.0],
        a_by: "farms",
        b: [-1990.0, 4500.0],
        b_by: "metro",
    },
    InterchangePlan {
        name: "Crowfoot Interchange",
        freeway: "Crosstown Expressway",
        a: [-600.0, 3790.0],
        a_by: "mountains",
        b: [-600.0, 4110.0],
        b_by: "metro",
    },
    InterchangePlan {
        name: "Granite Street",
        freeway: "Crosstown Expressway",
        a: [-1500.0, 3790.0],
        a_by: "mountains",
        b: [-1500.0, 4110.0],
        b_by: "metro",
    },
    InterchangePlan {
        name: "Midtown Interchange",
        freeway: "Route 7",
        a: [440.0, 4450.0],
        a_by: "metro",
        b: [760.0, 4450.0],
        b_by: "metro",
    },
    InterchangePlan {
        name: "Northgate Interchange",
        freeway: "Route 7",
        a: [365.0, 5100.0],
        a_by: "metro",
        b: [685.0, 5100.0],
        b_by: "metro",
    },
];

/// Where a freeway ends in an at-grade junction with a district road: `(freeway, node
/// position, district that connects to it)`. Route 7 and Route 12 start at Port Vesper's
/// interchange outer terminals (`outer_terminal`), laid by [`super::freeways`] itself.
pub const FREEWAY_ENDS: &[(&str, V2, &str)] = &[
    ("Route 7", [500.0, 5500.0], "metro"),
    ("Route 12", [-2050.0, 5500.0], "metro"),
];
