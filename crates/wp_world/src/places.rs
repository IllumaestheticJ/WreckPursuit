//! Landmark places (lead, 2026-10-29). Owner: "the dealership icons look nothing like
//! dealerships. They should be put on the map with an actual dealership - same with police
//! stations, other landmarks, and garages."
//!
//! Every drive-to place of Port Vesper and Ashpeak County gets a building that reads at a
//! glance from the road, on a plot beside the road its ring is on:
//!
//! | Place | Building |
//! |---|---|
//! | dealership | glass showroom, display cars out front, flag poles, pylon sign |
//! | used-car lot | rows of cars, sales cabin, string-flag poles, price-board sign |
//! | junkyard | fenced yard, stacked wrecks, a crane with a magnet, yard office |
//! | repair shop (`Garage` POI) | workshop with three roll-up bays (the middle one open and lit, a car on a lift), fascia sign |
//! | police station | civic block, portico, "POLICE" signs, flagpole, blue lamp, cruisers out front |
//! | safehouse | an unmarked warehouse with one roll-up door |
//! | Dockside Lock-up | a row of lock-up units, the middle one open and lit |
//! | Spire Basement Garage | a tower over a lit ramp portal |
//! | Harnell Barn | a barn with its doors to the road |
//!
//! The places are the `Garage`, `PoliceStation` and `SafehouseCandidate` POIs (their ring is
//! the POI) and [`DRIVE_TO_SPOTS`] (car lots and the player's garages, whose ring is the lane
//! point nearest the anchor, as `wp_wasm::garage_visit::snap_to_lane` puts it).
//!
//! Everything solid is an ordinary [`Building`] (kinds [`BuildingKind::is_place`], plus a
//! plain block for the Spire tower) or a [`Structure`] of kind
//! [`StructureKind::Place`]: `wp_sim` collides with both like any other building or
//! structure, `city-import` draws them into the greybox. Text (sign faces) and string flags
//! are not geometry: [`PlaceLayout::signs`] and [`PlaceLayout::bunting`] tell the renderer
//! where they go.
//!
//! Placement ([`plan`]) only reads the lanes, road surfaces, junctions, car parks, water,
//! terrain and the protected buildings and structures (landmarks, farms, silos, canopies),
//! never the generic buildings it clears or the place pieces themselves, so the testbed can
//! recompute the same layouts from the finished map (`wp_wasm` `place_layouts_json`).

use serde::Serialize;

use crate::geom::{self, V2, V3};
use crate::map::CityMap;
use crate::network::{
    Building, BuildingKind, LaneId, PlacePart, PoiKind, RoadNetwork, RoofShape, Structure,
    StructureKind,
};
use crate::surface::SIDEWALK_W_M;

/// Radius of a place's ring (every POI's 15 m, `wp_wasm::garage_visit::PLACE_RADIUS_M`), m.
/// Nothing solid stands inside it.
pub const RING_RADIUS_M: f32 = 15.0;
/// Extra clearance between the ring and the nearest solid piece, m.
pub const RING_CLEAR_M: f32 = 0.5;
/// The farthest a place's main building may stand from its ring, m (tests).
pub const MAX_RING_DISTANCE_M: f32 = 45.0;
/// Gap between a road's sidewalk (or bare paved edge) and the front of a plot, m.
const FRONT_GAP_M: f32 = 0.6;
/// Verge kept between a road without a sidewalk and the front of a plot, m.
const VERGE_M: f32 = 1.5;
/// Largest ground height range under a plot, m.
const MAX_PLOT_RISE_M: f32 = 2.5;
/// Plot offsets along the road from the ring tried in turn, m (the plot lines up with the
/// ring when it can).
const OFFSETS_M: [f32; 15] = [
    0.0, 3.0, -3.0, 6.0, -6.0, 9.0, -9.0, 12.0, -12.0, 16.0, -16.0, 20.0, -20.0, 25.0, -25.0,
];
/// Sample spacing of the plot clearance check, m.
const SAMPLE_M: f32 = 2.0;

/// What a place looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum PlaceStyle {
    /// New-car dealership.
    Dealership,
    /// Used-car lot.
    UsedLot,
    /// Junkyard / scrapyard.
    Junkyard,
    /// Repair shop (`Garage` POI).
    RepairShop,
    /// Police station (`PoliceStation` POI).
    PoliceStation,
    /// Safehouse (`SafehouseCandidate` POI).
    Safehouse,
    /// The Dockside Lock-up (home garage).
    LockUp,
    /// The Spire Basement Garage (home garage).
    TowerGarage,
    /// The Harnell Barn (home garage).
    Barn,
}

/// A drive-to place that is not a POI: a car lot (`data/economy/lots.toml` id) or one of the
/// player's garages (`data/economy/career.toml [[home_garages]]` id), at a fixed anchor of the
/// combined world. `wp_wasm::garage_visit::{WORLD_LOTS, HOME_GARAGES}` use the same anchors
/// (checked by `wp_wasm`'s tests).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriveToSpot {
    /// Lot or home garage id.
    pub id: &'static str,
    /// Display name (the sign).
    pub name: &'static str,
    /// Look.
    pub style: PlaceStyle,
    /// Anchor `[x, y]`, m: the ring sits on the lane nearest it.
    pub anchor: V2,
}

/// Every car lot and home garage of the combined world.
pub const DRIVE_TO_SPOTS: [DriveToSpot; 10] = [
    DriveToSpot {
        id: "budget_motors",
        name: "Budget Motors",
        style: PlaceStyle::Dealership,
        anchor: [-1025.0, 115.0],
    },
    DriveToSpot {
        id: "harbourline_auto",
        name: "Harbourline Auto",
        style: PlaceStyle::Dealership,
        anchor: [50.0, -535.0],
    },
    DriveToSpot {
        id: "meridian_prestige",
        name: "Meridian Prestige",
        style: PlaceStyle::Dealership,
        anchor: [-700.0, 4650.0],
    },
    DriveToSpot {
        id: "lucky_lou_used",
        name: "Lucky Lou's Used Cars",
        style: PlaceStyle::UsedLot,
        anchor: [-700.0, -975.0],
    },
    DriveToSpot {
        id: "certified_preowned",
        name: "Northgate Certified Pre-Owned",
        style: PlaceStyle::UsedLot,
        anchor: [0.0, 660.0],
    },
    DriveToSpot {
        id: "dockside_salvage",
        name: "Dockside Salvage",
        style: PlaceStyle::Junkyard,
        anchor: [1000.0, -975.0],
    },
    DriveToSpot {
        id: "ridgeback_scrap",
        name: "Ridgeback Scrap & Pull",
        style: PlaceStyle::Junkyard,
        anchor: [-3150.0, 300.0],
    },
    DriveToSpot {
        id: "vesper_lockup",
        name: "Dockside Lock-up",
        style: PlaceStyle::LockUp,
        anchor: [0.0, 225.0],
    },
    DriveToSpot {
        id: "spire_loft",
        name: "Spire Basement Garage",
        style: PlaceStyle::TowerGarage,
        anchor: [190.0, 4650.0],
    },
    DriveToSpot {
        id: "harnell_barn",
        name: "Harnell Barn",
        style: PlaceStyle::Barn,
        anchor: [-3300.0, 150.0],
    },
];

/// Text on a sign face, for the renderer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SignText {
    /// The text (made-up place names, or the generic "POLICE").
    pub text: String,
    /// Centre of the face, m.
    pub centre: V3,
    /// Unit normal of the face (horizontal).
    pub normal: V2,
    /// Face width and height, m.
    pub size: [f32; 2],
    /// Also write it on the back face (`centre - normal × depth_m`).
    pub both_sides: bool,
    /// Panel thickness, m (where the back face is).
    pub depth_m: f32,
}

/// One place as placed: its ring, plot frame, signs and string flags.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlaceLayout {
    /// Lot / home id, else the POI name.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Look.
    pub style: PlaceStyle,
    /// Ring centre (on the lane), m.
    pub ring: V3,
    /// Middle of the plot's road-side edge, m.
    pub origin: V2,
    /// Unit vector along the plot front (`right_of(out)`).
    pub along: V2,
    /// Unit vector from the road into the plot.
    pub out: V2,
    /// Plot width (along) and depth (out), m.
    pub size: [f32; 2],
    /// Floor level of the main building, m.
    pub floor_z: f32,
    /// Footprint of the main building (the one the tests measure from the ring), m.
    pub main: Vec<V2>,
    /// Sign faces to write.
    pub signs: Vec<SignText>,
    /// String flags between pole tops, m.
    pub bunting: Vec<[V3; 2]>,
}

impl PlaceLayout {
    /// World point of plot-local `(u, v)` (u along the front, v into the plot).
    pub fn world(&self, u: f32, v: f32) -> V2 {
        geom::add(
            self.origin,
            geom::add(geom::scale(self.along, u), geom::scale(self.out, v)),
        )
    }

    /// The plot rectangle (counter-clockwise), m.
    pub fn plot(&self) -> Vec<V2> {
        let (hw, d) = (self.size[0] * 0.5, self.size[1]);
        vec![
            self.world(-hw, 0.0),
            self.world(hw, 0.0),
            self.world(hw, d),
            self.world(-hw, d),
        ]
    }
}

/// A place to lay out: its id, name, look and ring.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaceSpec {
    /// Lot / home id, else the POI name.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Look.
    pub style: PlaceStyle,
    /// Ring centre, m.
    pub ring: V3,
    /// Lane the ring is on.
    pub lane: Option<LaneId>,
}

/// Whether `net` has landmark places (Port Vesper v2 and Ashpeak County do; the procedural
/// grid, imported and touge maps do not).
pub fn has_places(net: &RoadNetwork) -> bool {
    net.features
        .structures
        .iter()
        .any(|s| matches!(s.kind, StructureKind::Place(_)))
}

/// The places of `map`, in order: the `Garage`, `PoliceStation` and `SafehouseCandidate`
/// POIs (POI order), then the [`DRIVE_TO_SPOTS`] inside the map whose lane point is within
/// 80 m of the anchor.
pub fn place_specs(map: &CityMap) -> Vec<PlaceSpec> {
    let net = &map.net;
    let mut out = Vec::new();
    for p in &net.pois {
        let style = match p.kind {
            PoiKind::Garage => PlaceStyle::RepairShop,
            PoiKind::PoliceStation => PlaceStyle::PoliceStation,
            PoiKind::SafehouseCandidate => PlaceStyle::Safehouse,
            _ => continue,
        };
        out.push(PlaceSpec {
            id: p.name.clone(),
            name: p.name.clone(),
            style,
            ring: p.pos,
            lane: p.lane,
        });
    }
    for s in &DRIVE_TO_SPOTS {
        let a = s.anchor;
        let inside = a[0] >= net.bounds_min[0]
            && a[1] >= net.bounds_min[1]
            && a[0] <= net.bounds_max[0]
            && a[1] <= net.bounds_max[1];
        if !inside {
            continue;
        }
        let Some((seg, at)) = crate::routes::nearest_lane(map, [a[0], a[1], 0.0], None) else {
            continue;
        };
        let sg = map.seg(seg);
        let (pos, _) = sg.point_at(at);
        if geom::dist(geom::xy(pos), a) > 80.0 {
            continue;
        }
        out.push(PlaceSpec {
            id: s.id.to_string(),
            name: s.name.to_string(),
            style: s.style,
            ring: pos,
            lane: sg.lane(),
        });
    }
    out
}

/// Plot size `[width, depth]` of a look, m (before any push back from the ring).
fn plot_size(style: PlaceStyle) -> [f32; 2] {
    match style {
        PlaceStyle::Dealership => [38.0, 34.0],
        PlaceStyle::UsedLot => [34.0, 30.0],
        PlaceStyle::Junkyard => [40.0, 34.0],
        PlaceStyle::RepairShop => [30.0, 23.0],
        PlaceStyle::PoliceStation => [38.0, 31.0],
        PlaceStyle::Safehouse => [18.0, 16.0],
        PlaceStyle::LockUp => [24.0, 14.0],
        PlaceStyle::TowerGarage => [30.0, 31.0],
        PlaceStyle::Barn => [26.0, 27.0],
    }
}

/// A solid piece in plot-local coordinates.
#[derive(Debug, Clone, PartialEq)]
enum Piece {
    /// A building: footprint (u, v), wall height above the floor, roof.
    Building {
        kind: BuildingKind,
        poly: Vec<V2>,
        height: f32,
        roof: RoofShape,
        rise: f32,
    },
    /// A structure: footprint, bottom and top heights. `z0 <= 0` = from the ground under it.
    /// Free-standing pieces (`free`) measure from the ground under them, the others from the
    /// floor level of the main building.
    Part {
        part: PlacePart,
        poly: Vec<V2>,
        z0: f32,
        z1: f32,
        free: bool,
    },
}

impl Piece {
    fn poly(&self) -> &[V2] {
        match self {
            Piece::Building { poly, .. } | Piece::Part { poly, .. } => poly,
        }
    }

    fn shifted(&self, dv: f32) -> Piece {
        let mv = |p: &[V2]| p.iter().map(|q| [q[0], q[1] + dv]).collect::<Vec<_>>();
        match self {
            Piece::Building {
                kind,
                poly,
                height,
                roof,
                rise,
            } => Piece::Building {
                kind: *kind,
                poly: mv(poly),
                height: *height,
                roof: *roof,
                rise: *rise,
            },
            Piece::Part {
                part,
                poly,
                z0,
                z1,
                free,
            } => Piece::Part {
                part: *part,
                poly: mv(poly),
                z0: *z0,
                z1: *z1,
                free: *free,
            },
        }
    }
}

/// Local rectangle `[u0, u1] × [v0, v1]` (counter-clockwise).
fn rect(u0: f32, u1: f32, v0: f32, v1: f32) -> Vec<V2> {
    vec![[u0, v0], [u1, v0], [u1, v1], [u0, v1]]
}

/// Local rectangle centred on `c`, `w` across and `l` long, its length along `dir` (unit).
fn rect_dir(c: V2, dir: V2, w: f32, l: f32) -> Vec<V2> {
    let r = geom::right_of(dir);
    let p = |a: f32, b: f32| geom::add(c, geom::add(geom::scale(dir, a), geom::scale(r, b)));
    let (hl, hw) = (l * 0.5, w * 0.5);
    // right_of(dir) is clockwise of dir: (r, dir) is a right-handed pair, so this order is
    // counter-clockwise.
    vec![p(-hl, -hw), p(-hl, hw), p(hl, hw), p(hl, -hw)]
}

/// Ensure a polygon is counter-clockwise.
fn ccw(mut p: Vec<V2>) -> Vec<V2> {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (q, r) = (p[i], p[(i + 1) % p.len()]);
        a += q[0] * r[1] - r[0] * q[1];
    }
    if a < 0.0 {
        p.reverse();
    }
    p
}

/// A sign face in plot-local coordinates.
#[derive(Debug, Clone, PartialEq)]
struct KitSign {
    text: String,
    /// Centre of the face (u, v).
    c: V2,
    /// Height of the centre above the floor (or the ground under it when `free`).
    z: f32,
    /// Face normal (local).
    normal: V2,
    size: [f32; 2],
    both: bool,
    depth: f32,
    free: bool,
}

/// Builder of one layout's pieces and texts (plot-local).
#[derive(Default)]
struct Kit {
    pieces: Vec<Piece>,
    /// Sign faces.
    signs: Vec<KitSign>,
    /// String flags: local (u, v, z above the ground) pairs.
    bunting: Vec<([f32; 3], [f32; 3])>,
}

impl Kit {
    fn building(&mut self, kind: BuildingKind, poly: Vec<V2>, height: f32) {
        self.pieces.push(Piece::Building {
            kind,
            poly,
            height,
            roof: RoofShape::Flat,
            rise: 0.0,
        });
    }

    fn part(&mut self, part: PlacePart, poly: Vec<V2>, z0: f32, z1: f32) {
        self.pieces.push(Piece::Part {
            part,
            poly,
            z0,
            z1,
            free: false,
        });
    }

    fn free(&mut self, part: PlacePart, poly: Vec<V2>, z0: f32, z1: f32) {
        self.pieces.push(Piece::Part {
            part,
            poly,
            z0,
            z1,
            free: true,
        });
    }

    /// A post `w` square at `(u, v)`, `h` tall.
    fn post(&mut self, u: f32, v: f32, w: f32, h: f32) {
        let s = w * 0.5;
        self.free(PlacePart::Frame, rect(u - s, u + s, v - s, v + s), 0.0, h);
    }

    /// A flag pole at `(u, v)`, `h` tall, its flag flying along `+u` (`dir` = ±1).
    fn flag_pole(&mut self, u: f32, v: f32, h: f32, colour: u8, dir: f32) {
        self.post(u, v, 0.16, h);
        let (a, b) = if dir > 0.0 {
            (u + 0.1, u + 1.9)
        } else {
            (u - 1.9, u - 0.1)
        };
        self.free(
            PlacePart::Flag(colour),
            rect(a, b, v - 0.03, v + 0.03),
            h - 1.25,
            h - 0.1,
        );
    }

    /// A car: centre `c`, facing `dir` (unit, local), body paint `paint`, `lift` m off the
    /// ground, optional light bar.
    fn car(&mut self, c: V2, dir: V2, paint: u8, lift: f32, lightbar: bool) {
        let r = geom::right_of(dir);
        let at = |a: f32, b: f32| geom::add(c, geom::add(geom::scale(dir, a), geom::scale(r, b)));
        let z = |h: f32| lift + h;
        // Wheels (0.65 m), body, cabin.
        for (a, b) in [(1.35, 0.78), (1.35, -0.78), (-1.35, 0.78), (-1.35, -0.78)] {
            self.free(
                PlacePart::Wheel,
                rect_dir(at(a, b), dir, 0.26, 0.66),
                if lift > 0.0 { z(0.0) } else { 0.0 },
                z(0.64),
            );
        }
        self.free(
            PlacePart::CarBody(paint),
            rect_dir(c, dir, 1.82, 4.45),
            z(0.32),
            z(0.92),
        );
        let cab = at(-0.25, 0.0);
        self.free(
            PlacePart::CarGlass,
            rect_dir(cab, dir, 1.6, 2.3),
            z(0.92),
            z(1.36),
        );
        if lightbar {
            self.free(
                PlacePart::RedLamp,
                rect_dir(geom::add(cab, geom::scale(r, -0.35)), dir, 0.65, 0.3),
                z(1.36),
                z(1.5),
            );
            self.free(
                PlacePart::BlueLamp,
                rect_dir(geom::add(cab, geom::scale(r, 0.35)), dir, 0.65, 0.3),
                z(1.36),
                z(1.5),
            );
        }
    }

    /// A sign face (text) on a panel whose front face is at local `(u, v)`.
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        text: &str,
        c: V2,
        z: f32,
        normal: V2,
        size: [f32; 2],
        both: bool,
        depth: f32,
        free: bool,
    ) {
        self.signs.push(KitSign {
            text: text.to_string(),
            c,
            z,
            normal,
            size,
            both,
            depth,
            free,
        });
    }
}

/// Dealership: showroom at the back, display cars on the forecourt, flags, pylon sign.
fn dealership(k: &mut Kit, name: &str) {
    k.building(BuildingKind::Showroom, rect(-16.0, 16.0, 17.0, 33.0), 7.5);
    // Big front window and side returns.
    k.part(PlacePart::Glass, rect(-14.5, 14.5, 16.75, 17.0), 0.0, 6.2);
    k.part(PlacePart::Glass, rect(-16.25, -16.0, 18.5, 24.0), 0.0, 6.2);
    k.part(PlacePart::Glass, rect(16.0, 16.25, 18.5, 24.0), 0.0, 6.2);
    // Fascia band with the name over the window.
    k.part(PlacePart::Sign, rect(-9.0, 9.0, 16.7, 17.0), 6.35, 7.35);
    k.text(
        name,
        [0.0, 16.7],
        6.85,
        [0.0, -1.0],
        [17.6, 0.9],
        false,
        0.3,
        false,
    );
    // Entrance canopy.
    k.part(PlacePart::Frame, rect(-4.0, 4.0, 13.5, 16.75), 4.6, 5.0);
    // Display cars, angled to the road, and a car in the window.
    let d = |deg: f32| geom::dir_of_yaw(deg.to_radians());
    k.car([-13.0, 9.0], d(-35.0), 0, 0.0, false);
    k.car([-6.5, 11.0], d(-20.0), 1, 0.0, false);
    k.car([6.5, 11.0], d(20.0), 2, 0.0, false);
    k.car([13.0, 9.0], d(35.0), 3, 0.0, false);
    // Flag poles along the front.
    for (i, u) in [-17.5f32, -14.0, -10.5].iter().enumerate() {
        k.flag_pole(*u, 1.2, 8.5, i as u8 % 4, 1.0);
    }
    // Pylon sign by the road (readable both ways along it).
    k.post(16.8, 2.0, 0.7, 5.0);
    k.free(PlacePart::Sign, rect(16.6, 17.0, 0.3, 3.7), 5.0, 7.4);
    k.text(
        name,
        [17.0, 2.0],
        6.2,
        [1.0, 0.0],
        [3.2, 2.2],
        true,
        0.4,
        true,
    );
}

/// Used-car lot: two rows of cars, sales cabin, string flags, price board.
fn used_lot(k: &mut Kit, name: &str) {
    for (row, v) in [10.5f32, 18.0].iter().enumerate() {
        for (i, u) in [-13.0f32, -8.0, -3.0, 2.0].iter().enumerate() {
            let paint = ((row * 4 + i) % 6) as u8;
            k.car([*u, *v], [0.0, -1.0], paint, 0.0, false);
        }
    }
    // Sales cabin with a lit window.
    k.building(BuildingKind::Kiosk, rect(8.0, 15.0, 21.0, 26.0), 3.0);
    k.part(PlacePart::Glass, rect(9.0, 14.0, 20.8, 21.0), 1.0, 2.3);
    k.part(PlacePart::Sign, rect(8.5, 14.5, 20.75, 21.0), 2.45, 2.95);
    k.text(
        "SALES",
        [11.5, 20.75],
        2.7,
        [0.0, -1.0],
        [5.6, 0.45],
        false,
        0.25,
        false,
    );
    // Price board on two posts.
    k.post(8.6, 3.4, 0.25, 2.2);
    k.post(15.4, 3.4, 0.25, 2.2);
    k.free(PlacePart::Sign, rect(8.2, 15.8, 3.2, 3.5), 2.2, 4.4);
    k.text(
        name,
        [12.0, 3.2],
        3.3,
        [0.0, -1.0],
        [7.2, 2.0],
        false,
        0.3,
        true,
    );
    // String-flag poles at the corners and middles, strings between their tops.
    let poles = [
        [-16.5f32, 1.0],
        [0.5, 1.0],
        [16.5, 1.0],
        [16.5, 28.5],
        [0.5, 28.5],
        [-16.5, 28.5],
    ];
    for p in &poles {
        k.post(p[0], p[1], 0.14, 5.0);
    }
    for w in poles.windows(2) {
        k.bunting
            .push(([w[0][0], w[0][1], 4.9], [w[1][0], w[1][1], 4.9]));
    }
    k.bunting.push(([-16.5, 28.5, 4.9], [-16.5, 1.0, 4.9]));
    k.bunting.push(([-16.5, 1.0, 4.9], [16.5, 28.5, 4.9]));
}

/// Junkyard: fence with a gate, stacked wrecks, crane with a magnet, office.
fn junkyard(k: &mut Kit, name: &str) {
    let h = 2.6;
    k.part(PlacePart::Fence, rect(-20.0, -19.75, 4.0, 33.0), 0.0, h);
    k.part(PlacePart::Fence, rect(19.75, 20.0, 4.0, 33.0), 0.0, h);
    k.part(PlacePart::Fence, rect(-20.0, 20.0, 32.75, 33.0), 0.0, h);
    k.part(PlacePart::Fence, rect(-20.0, -6.5, 4.0, 4.25), 0.0, h);
    k.part(PlacePart::Fence, rect(6.5, 20.0, 4.0, 4.25), 0.0, h);
    // Name board on the fence by the gate.
    k.part(PlacePart::Sign, rect(-15.0, -7.0, 3.75, 4.0), 1.0, 2.5);
    k.text(
        name,
        [-11.0, 3.75],
        1.75,
        [0.0, -1.0],
        [7.6, 1.3],
        false,
        0.25,
        false,
    );
    // Wreck stacks along both sides and the back (crushed, slightly skewed).
    let mut stack = |c: V2, n: usize, along_u: bool| {
        for j in 0..n {
            let skew = if j % 2 == 0 { 0.08 } else { -0.06 };
            let dir = if along_u {
                geom::normalize([1.0, skew])
            } else {
                geom::normalize([skew, 1.0])
            };
            let z0 = j as f32 * 0.78;
            k.free(PlacePart::Wreck, rect_dir(c, dir, 1.8, 4.2), z0, z0 + 0.74);
        }
    };
    for (i, v) in [8.5f32, 13.5, 18.5, 23.5].iter().enumerate() {
        stack([-17.0, *v], 1 + (i + 1) % 3, false);
        stack([17.0, *v + 1.5], 1 + i % 3, false);
    }
    for (i, u) in [-12.0f32, -6.5, -1.0, 4.5].iter().enumerate() {
        stack([*u, 30.0], 2 + i % 2, true);
    }
    // Crane: mast, boom, cable, magnet.
    k.post(12.0, 26.5, 1.0, 11.0);
    k.free(PlacePart::Frame, rect(1.0, 12.5, 26.1, 26.9), 10.2, 11.0);
    k.free(PlacePart::Frame, rect(3.0, 3.1, 26.45, 26.55), 6.6, 10.2);
    k.free(PlacePart::Frame, rect(2.2, 3.9, 25.65, 27.35), 6.0, 6.6);
    // Yard office inside the gate.
    k.building(BuildingKind::Kiosk, rect(9.0, 16.0, 6.5, 11.0), 3.0);
    k.part(PlacePart::Glass, rect(10.0, 15.0, 6.3, 6.5), 1.0, 2.2);
}

/// Repair shop: three bays, the middle one open (lit, a car on a lift), fascia sign.
fn repair_shop(k: &mut Kit, name: &str) {
    let (top, front, back) = (6.0, 6.0, 22.0);
    k.building(BuildingKind::Workshop, rect(-14.0, -3.3, front, back), top);
    k.building(BuildingKind::Workshop, rect(3.3, 14.0, front, back), top);
    k.building(BuildingKind::Workshop, rect(-3.3, 3.3, 14.5, back), top);
    // Roof over the open bay, its lit back wall and a light strip.
    k.part(PlacePart::Frame, rect(-3.3, 3.3, front, 14.5), 5.0, top);
    k.part(PlacePart::BayLight, rect(-3.1, 3.1, 14.3, 14.5), 0.0, 4.9);
    k.part(PlacePart::Lamp, rect(-2.4, 2.4, 9.8, 10.6), 4.85, 5.0);
    // Closed roll-up doors.
    k.part(
        PlacePart::Door,
        rect(-11.8, -5.6, front - 0.2, front),
        0.0,
        4.4,
    );
    k.part(
        PlacePart::Door,
        rect(5.6, 11.8, front - 0.2, front),
        0.0,
        4.4,
    );
    // Lift: two posts and arms, the car on it.
    k.part(PlacePart::Frame, rect(-1.75, -1.4, 9.8, 10.2), 0.0, 3.2);
    k.part(PlacePart::Frame, rect(1.4, 1.75, 9.8, 10.2), 0.0, 3.2);
    k.part(PlacePart::Frame, rect(-1.4, 1.4, 8.4, 8.6), 1.55, 1.7);
    k.part(PlacePart::Frame, rect(-1.4, 1.4, 11.4, 11.6), 1.55, 1.7);
    k.car([0.0, 10.0], [0.0, 1.0], 4, 1.7, false);
    // Fascia sign across the front.
    k.part(
        PlacePart::Sign,
        rect(-13.5, 13.5, front - 0.25, front),
        4.55,
        5.85,
    );
    k.text(
        name,
        [0.0, front - 0.25],
        5.2,
        [0.0, -1.0],
        [26.0, 1.15],
        false,
        0.25,
        false,
    );
    // A lamp over each closed door.
    k.part(
        PlacePart::Lamp,
        rect(-9.2, -8.2, front - 0.45, front - 0.25),
        4.45,
        4.55,
    );
    k.part(
        PlacePart::Lamp,
        rect(8.2, 9.2, front - 0.45, front - 0.25),
        4.45,
        4.55,
    );
}

/// Police station: civic block, portico with the sign, blue lamp, flagpole, cruisers.
fn police_station(k: &mut Kit) {
    let (front, back, top) = (14.0, 30.0, 9.0);
    k.building(BuildingKind::Civic, rect(-16.0, 16.0, front, back), top);
    // Two dark window bands.
    for (z0, z1) in [(1.4, 3.0), (5.2, 6.8)] {
        k.part(
            PlacePart::Window,
            rect(-15.0, -6.0, front - 0.2, front),
            z0,
            z1,
        );
        k.part(
            PlacePart::Window,
            rect(6.0, 15.0, front - 0.2, front),
            z0,
            z1,
        );
    }
    k.part(
        PlacePart::Window,
        rect(-5.0, 5.0, front - 0.2, front),
        5.2,
        6.8,
    );
    // Portico: roof, pillars, the sign on top of its front.
    k.part(PlacePart::Frame, rect(-5.5, 5.5, 10.0, front), 3.8, 4.3);
    k.part(PlacePart::Frame, rect(-5.2, -4.6, 10.1, 10.7), 0.0, 3.8);
    k.part(PlacePart::Frame, rect(4.6, 5.2, 10.1, 10.7), 0.0, 3.8);
    k.part(PlacePart::Sign, rect(-4.5, 4.5, 10.0, 10.3), 4.3, 5.3);
    k.text(
        "POLICE",
        [0.0, 10.0],
        4.8,
        [0.0, -1.0],
        [8.6, 0.95],
        false,
        0.3,
        false,
    );
    // The same word big across the top of the façade.
    k.part(
        PlacePart::Sign,
        rect(-6.0, 6.0, front - 0.25, front),
        7.4,
        8.7,
    );
    k.text(
        "POLICE",
        [0.0, front - 0.25],
        8.05,
        [0.0, -1.0],
        [11.6, 1.25],
        false,
        0.25,
        false,
    );
    // Lit entrance and the blue lamp on its post.
    k.part(
        PlacePart::BayLight,
        rect(-2.0, 2.0, front - 0.2, front),
        0.0,
        3.0,
    );
    k.post(-6.6, 10.6, 0.18, 2.8);
    k.free(PlacePart::BlueLamp, rect(-6.9, -6.3, 10.3, 10.9), 2.8, 3.5);
    // Flagpole.
    k.flag_pole(18.0, 9.0, 11.0, 2, -1.0);
    // Cruisers parked nose-in out front.
    for u in [-14.6f32, -11.4, 8.4, 11.6] {
        k.car([u, 9.6], [0.0, 1.0], 6, 0.0, true);
    }
}

/// Safehouse: an unmarked warehouse with one roll-up door and a dim lamp.
fn safehouse(k: &mut Kit) {
    k.building(BuildingKind::LockUp, rect(-8.0, 8.0, 4.5, 15.5), 6.0);
    k.part(PlacePart::Door, rect(-2.4, 2.4, 4.3, 4.5), 0.0, 4.0);
    k.part(PlacePart::Lamp, rect(-0.4, 0.4, 4.15, 4.5), 4.35, 4.5);
}

/// Dockside Lock-up: three lock-up units, the middle one (the player's) open and lit.
fn lock_up(k: &mut Kit, name: &str) {
    let (front, back, top) = (4.5, 13.0, 3.6);
    k.building(BuildingKind::LockUp, rect(-11.0, -3.1, front, back), top);
    k.building(BuildingKind::LockUp, rect(3.1, 11.0, front, back), top);
    k.building(BuildingKind::LockUp, rect(-3.1, 3.1, 10.5, back), top);
    k.part(PlacePart::Frame, rect(-3.1, 3.1, front, 10.5), 3.0, top);
    k.part(PlacePart::BayLight, rect(-2.9, 2.9, 10.3, 10.5), 0.0, 2.95);
    k.part(PlacePart::Lamp, rect(-1.8, 1.8, 7.0, 7.6), 2.85, 3.0);
    k.part(
        PlacePart::Door,
        rect(-9.2, -4.8, front - 0.2, front),
        0.0,
        2.6,
    );
    k.part(
        PlacePart::Door,
        rect(4.8, 9.2, front - 0.2, front),
        0.0,
        2.6,
    );
    k.part(
        PlacePart::Sign,
        rect(-2.9, 2.9, front - 0.25, front),
        3.05,
        3.55,
    );
    k.text(
        name,
        [0.0, front - 0.25],
        3.3,
        [0.0, -1.0],
        [5.6, 0.45],
        false,
        0.25,
        false,
    );
}

/// Spire Basement Garage: a tower over a lit ramp portal.
fn tower_garage(k: &mut Kit, name: &str) {
    k.building(BuildingKind::Block, rect(-14.0, 14.0, 11.0, 30.0), 44.0);
    // Portal: wing walls, roof slab, dark ramp floor, lit mouth, raised barrier arm.
    k.building(BuildingKind::Civic, rect(-5.4, -3.8, 3.0, 11.0), 3.4);
    k.building(BuildingKind::Civic, rect(3.8, 5.4, 3.0, 11.0), 3.4);
    k.part(PlacePart::Frame, rect(-5.4, 5.4, 3.0, 11.0), 3.4, 4.1);
    k.part(PlacePart::Dark, rect(-3.8, 3.8, 3.0, 11.0), 0.0, 0.03);
    k.part(PlacePart::BayLight, rect(-3.6, 3.6, 10.8, 11.0), 0.0, 3.3);
    k.part(PlacePart::Lamp, rect(-2.6, 2.6, 6.6, 7.4), 3.25, 3.4);
    k.post(3.3, 3.6, 0.3, 1.1);
    k.free(PlacePart::Door, rect(3.2, 3.4, 3.5, 3.7), 1.1, 3.2);
    k.part(PlacePart::Sign, rect(-4.8, 4.8, 2.75, 3.0), 4.15, 5.05);
    k.text(
        name,
        [0.0, 2.75],
        4.6,
        [0.0, -1.0],
        [9.2, 0.8],
        false,
        0.25,
        false,
    );
}

/// Harnell Barn: a barn, its doors to the road, a lamp and a sign on posts.
fn barn(k: &mut Kit, name: &str) {
    k.pieces.push(Piece::Building {
        kind: BuildingKind::HomeBarn,
        poly: rect(-7.5, 7.5, 7.0, 25.0),
        height: 5.0,
        roof: RoofShape::Gable,
        rise: 4.0,
    });
    k.part(PlacePart::BarnDoor, rect(-3.2, 3.2, 6.8, 7.0), 0.0, 4.6);
    k.part(PlacePart::Lamp, rect(-0.4, 0.4, 6.65, 7.0), 4.75, 4.9);
    k.post(7.4, 2.5, 0.2, 1.9);
    k.post(11.6, 2.5, 0.2, 1.9);
    k.free(PlacePart::Sign, rect(7.2, 11.8, 2.3, 2.55), 1.2, 2.3);
    k.text(
        name,
        [9.5, 2.3],
        1.75,
        [0.0, -1.0],
        [4.4, 1.0],
        false,
        0.25,
        true,
    );
}

fn kit_for(style: PlaceStyle, name: &str) -> Kit {
    let mut k = Kit::default();
    match style {
        PlaceStyle::Dealership => dealership(&mut k, name),
        PlaceStyle::UsedLot => used_lot(&mut k, name),
        PlaceStyle::Junkyard => junkyard(&mut k, name),
        PlaceStyle::RepairShop => repair_shop(&mut k, name),
        PlaceStyle::PoliceStation => police_station(&mut k),
        PlaceStyle::Safehouse => safehouse(&mut k),
        PlaceStyle::LockUp => lock_up(&mut k, name),
        PlaceStyle::TowerGarage => tower_garage(&mut k, name),
        PlaceStyle::Barn => barn(&mut k, name),
    }
    k
}

/// Distance from `p` to a convex polygon (0 inside).
fn dist_to_poly(p: V2, poly: &[V2]) -> f32 {
    let n = poly.len();
    let mut inside = true;
    let mut best = f32::INFINITY;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if geom::cross(geom::sub(b, a), geom::sub(p, a)) < 0.0 {
            inside = false;
        }
        best = best.min(geom::point_segment_distance(p, a, b));
    }
    if inside {
        0.0
    } else {
        best
    }
}

/// Separating-axis overlap of two convex polygons.
pub fn convex_overlap(a: &[V2], b: &[V2]) -> bool {
    for poly in [a, b] {
        let n = poly.len();
        for i in 0..n {
            let e = geom::sub(poly[(i + 1) % n], poly[i]);
            let axis = [-e[1], e[0]];
            let range = |p: &[V2]| {
                p.iter()
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), q| {
                        let d = geom::dot(*q, axis);
                        (lo.min(d), hi.max(d))
                    })
            };
            let (a0, a1) = range(a);
            let (b0, b1) = range(b);
            if a1 < b0 || b1 < a0 {
                return false;
            }
        }
    }
    true
}

/// Everything near a plot that it must stay clear of (gathered once per place).
struct Surroundings<'a> {
    map: &'a CityMap,
    /// `(surface index, centre cumulative lengths)` of road surfaces near the ring.
    surfaces: Vec<(usize, Vec<f32>)>,
    /// Junction centres and clear radii near the ring.
    junctions: Vec<(V2, f32)>,
    /// Footprints that may not be built over: landmarks, farms, silos, structures.
    protected: Vec<Vec<V2>>,
}

impl<'a> Surroundings<'a> {
    fn new(map: &'a CityMap, ring: V2, reach: f32) -> Self {
        let net = &map.net;
        let near = |pts: &mut dyn Iterator<Item = V2>, pad: f32| {
            let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
            for q in pts {
                lo = [lo[0].min(q[0]), lo[1].min(q[1])];
                hi = [hi[0].max(q[0]), hi[1].max(q[1])];
            }
            ring[0] > lo[0] - reach - pad
                && ring[0] < hi[0] + reach + pad
                && ring[1] > lo[1] - reach - pad
                && ring[1] < hi[1] + reach + pad
        };
        let surfaces = net
            .surfaces
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                s.centre.len() >= 2
                    && near(
                        &mut s.centre.iter().map(|p| geom::xy(*p)),
                        s.left_m.max(s.right_m) + SIDEWALK_W_M,
                    )
            })
            .map(|(i, s)| (i, geom::cumulative(&s.centre)))
            .collect();
        let junctions = net
            .intersections
            .iter()
            .map(|ix| {
                (
                    geom::xy(net.nodes[ix.node as usize].pos),
                    ix.radius_m.max(0.0) + SIDEWALK_W_M + 1.0,
                )
            })
            .filter(|(c, r)| geom::dist(*c, ring) < reach + r)
            .collect();
        let mut protected: Vec<Vec<V2>> = Vec::new();
        for b in &net.buildings {
            let keep = matches!(
                b.kind,
                BuildingKind::Landmark | BuildingKind::Farm | BuildingKind::Silo
            );
            if keep && !b.footprint.is_empty() && near(&mut b.footprint.iter().copied(), 0.0) {
                protected.push(b.footprint.clone());
            }
        }
        for s in &net.features.structures {
            if matches!(s.kind, StructureKind::Place(_)) {
                continue;
            }
            if near(&mut s.footprint.iter().copied(), 0.0) {
                protected.push(s.footprint.clone());
            }
        }
        Self {
            map,
            surfaces,
            junctions,
            protected,
        }
    }

    /// `q` is clear of every road surface (and its sidewalks), junction, car park and water.
    fn clear(&self, q: V2) -> bool {
        let net = &self.map.net;
        for (i, cum) in &self.surfaces {
            let s = &net.surfaces[*i];
            let pr = geom::project(&s.centre, cum, q);
            let right = pr.lateral > 0.0;
            let edge = if right { s.right_m } else { s.left_m };
            let walk = if s.sidewalk[usize::from(right)] {
                SIDEWALK_W_M
            } else {
                VERGE_M
            };
            if pr.dist < edge + walk + FRONT_GAP_M * 0.5 {
                return false;
            }
        }
        if self.junctions.iter().any(|(c, r)| geom::dist(q, *c) < *r) {
            return false;
        }
        if self.map.car_parks.iter().any(|c| c.surface_z(q).is_some()) {
            return false;
        }
        self.map.water_surface_at(q).is_none()
    }
}

/// Ground height the place pieces stand on (the drawn ground `wp_sim` collides with).
fn ground(map: &CityMap, q: V2) -> f32 {
    let drawn = map
        .ground
        .as_ref()
        .map_or_else(|| map.net.ground_z(q), |g| g.height_on_triangles(q));
    drawn.min(map.net.ground_z(q))
}

/// Points every [`SAMPLE_M`] over a local polygon's bounding box that lie inside it (and its
/// corners).
fn samples(poly: &[V2]) -> Vec<V2> {
    let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
    for q in poly {
        lo = [lo[0].min(q[0]), lo[1].min(q[1])];
        hi = [hi[0].max(q[0]), hi[1].max(q[1])];
    }
    let mut out: Vec<V2> = poly.to_vec();
    let nu = ((hi[0] - lo[0]) / SAMPLE_M).ceil().max(1.0) as usize;
    let nv = ((hi[1] - lo[1]) / SAMPLE_M).ceil().max(1.0) as usize;
    for i in 0..=nu {
        for j in 0..=nv {
            let q = [
                lo[0] + (hi[0] - lo[0]) * i as f32 / nu as f32,
                lo[1] + (hi[1] - lo[1]) * j as f32 / nv as f32,
            ];
            if dist_to_poly(q, poly) == 0.0 {
                out.push(q);
            }
        }
    }
    out
}

/// A candidate frame: origin, along, out.
#[derive(Debug, Clone, Copy)]
struct Frame {
    origin: V2,
    along: V2,
    out: V2,
}

impl Frame {
    fn world(&self, q: V2) -> V2 {
        geom::add(
            self.origin,
            geom::add(geom::scale(self.along, q[0]), geom::scale(self.out, q[1])),
        )
    }

    fn local(&self, p: V2) -> V2 {
        let d = geom::sub(p, self.origin);
        [geom::dot(d, self.along), geom::dot(d, self.out)]
    }
}

/// The surface the ring's lane belongs to (else the nearest surface to the ring).
fn ring_surface(map: &CityMap, spec: &PlaceSpec) -> Option<usize> {
    let net = &map.net;
    let ring = geom::xy(spec.ring);
    if let Some(l) = spec.lane.and_then(|l| net.lanes.get(l as usize)) {
        if let Some(i) = net.surfaces.iter().position(|s| s.road == l.road) {
            return Some(i);
        }
    }
    net.surfaces
        .iter()
        .enumerate()
        .filter(|(_, s)| s.centre.len() >= 2)
        .map(|(i, s)| {
            let cum = geom::cumulative(&s.centre);
            (geom::project(&s.centre, &cum, ring).dist, i)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|(_, i)| i)
}

/// A laid-out place: the layout and its pieces in plot-local coordinates.
struct Placed {
    layout: PlaceLayout,
    frame: Frame,
    pieces: Vec<Piece>,
    kit_signs: Vec<KitSign>,
    kit_bunting: Vec<([f32; 3], [f32; 3])>,
}

/// Lay out one place: try both sides of its road and a few offsets along it, push the
/// pieces back until none is inside the ring, and keep the first plot that is clear of every
/// road, sidewalk, junction, car park, water, protected building and earlier plot.
fn lay_out(map: &CityMap, spec: &PlaceSpec, taken: &[Vec<V2>]) -> Option<Placed> {
    let net = &map.net;
    let si = ring_surface(map, spec)?;
    let s = &net.surfaces[si];
    let cum = geom::cumulative(&s.centre);
    let len = cum[cum.len() - 1];
    let ring = geom::xy(spec.ring);
    let pr = geom::project(&s.centre, &cum, ring);
    let preferred = if pr.lateral >= 0.0 { 1.0f32 } else { -1.0 };
    let base = kit_for(spec.style, &spec.name);
    let size = plot_size(spec.style);
    let around = Surroundings::new(map, ring, size[0] + size[1] + 30.0);
    for side in [preferred, -preferred] {
        let right = side > 0.0;
        let edge = if right { s.right_m } else { s.left_m };
        let walk = if s.sidewalk[usize::from(right)] {
            SIDEWALK_W_M
        } else {
            VERGE_M
        };
        for offset in OFFSETS_M {
            let at = pr.s.clamp(0.0, len) + offset;
            if at < 0.0 || at > len {
                continue;
            }
            let (c, t) = geom::point_at(&s.centre, &cum, at);
            let out = geom::scale(geom::right_of(t), side);
            let frame = Frame {
                origin: geom::add(geom::xy(c), geom::scale(out, edge + walk + FRONT_GAP_M)),
                along: geom::right_of(out),
                out,
            };
            let ring_l = frame.local(ring);
            // Push back until every piece clears the ring.
            let mut dv = 0.0f32;
            for _ in 0..12 {
                let worst = base
                    .pieces
                    .iter()
                    .map(|p| {
                        let poly: Vec<V2> = p.poly().iter().map(|q| [q[0], q[1] + dv]).collect();
                        dist_to_poly(ring_l, &poly)
                    })
                    .fold(f32::INFINITY, f32::min);
                let need = RING_RADIUS_M + RING_CLEAR_M;
                if worst >= need {
                    break;
                }
                dv += need - worst + 0.1;
            }
            let depth = size[1] + dv;
            let plot_l = rect(-size[0] * 0.5, size[0] * 0.5, 0.0, depth);
            let plot_w: Vec<V2> = ccw(plot_l.iter().map(|q| frame.world(*q)).collect());
            // Clear of roads, sidewalks, junctions, car parks and water.
            let pts = samples(&plot_l);
            if !pts.iter().all(|q| around.clear(frame.world(*q))) {
                continue;
            }
            // Level enough.
            let zs: Vec<f32> = pts.iter().map(|q| ground(map, frame.world(*q))).collect();
            let (zlo, zhi) = zs
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), z| {
                    (lo.min(*z), hi.max(*z))
                });
            if zhi - zlo > MAX_PLOT_RISE_M || (zhi - spec.ring[2]).abs() > 4.0 {
                continue;
            }
            // Clear of protected buildings and structures and of earlier plots.
            if around
                .protected
                .iter()
                .chain(taken.iter())
                .any(|p| convex_overlap(&plot_w, p))
            {
                continue;
            }
            let pieces: Vec<Piece> = base.pieces.iter().map(|p| p.shifted(dv)).collect();
            let main_l = pieces
                .iter()
                .find_map(|p| match p {
                    Piece::Building { poly, .. } => Some(poly.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| plot_l.clone());
            let floor_z = samples(&main_l)
                .iter()
                .map(|q| ground(map, frame.world(*q)))
                .fold(f32::NEG_INFINITY, f32::max);
            let layout = PlaceLayout {
                id: spec.id.clone(),
                name: spec.name.clone(),
                style: spec.style,
                ring: spec.ring,
                origin: frame.origin,
                along: frame.along,
                out: frame.out,
                size: [size[0], depth],
                floor_z,
                main: ccw(main_l.iter().map(|q| frame.world(*q)).collect()),
                signs: Vec::new(),
                bunting: Vec::new(),
            };
            let shift = |q: V2| [q[0], q[1] + dv];
            return Some(Placed {
                layout,
                frame,
                pieces,
                kit_signs: base
                    .signs
                    .iter()
                    .map(|s| KitSign {
                        c: shift(s.c),
                        ..s.clone()
                    })
                    .collect(),
                kit_bunting: base
                    .bunting
                    .iter()
                    .map(|(a, b)| ([a[0], a[1] + dv, a[2]], [b[0], b[1] + dv, b[2]]))
                    .collect(),
            });
        }
    }
    None
}

/// The solid pieces of a placed place in world coordinates, and its finished layout (sign and
/// bunting heights resolved).
fn finish(map: &CityMap, p: Placed) -> (PlaceLayout, Vec<Building>, Vec<Structure>) {
    let Placed {
        mut layout,
        frame,
        pieces,
        kit_signs,
        kit_bunting,
    } = p;
    let floor = layout.floor_z;
    let world_poly = |poly: &[V2]| ccw(poly.iter().map(|q| frame.world(*q)).collect());
    let ground_range = |poly: &[V2]| {
        samples(poly)
            .iter()
            .map(|q| ground(map, frame.world(*q)))
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), z| {
                (lo.min(z), hi.max(z))
            })
    };
    let mut buildings = Vec::new();
    let mut structures = Vec::new();
    for piece in &pieces {
        match piece {
            Piece::Building {
                kind,
                poly,
                height,
                roof,
                rise,
            } => {
                let (lo, _) = ground_range(poly);
                let base = lo - 0.3;
                buildings.push(Building {
                    footprint: world_poly(poly),
                    height_m: floor + height - base,
                    base_z_m: base,
                    kind: *kind,
                    roof: *roof,
                    roof_rise_m: *rise,
                });
            }
            Piece::Part {
                part,
                poly,
                z0,
                z1,
                free,
            } => {
                let (lo, hi) = ground_range(poly);
                let reference = if *free { hi } else { floor };
                let bottom = if *z0 <= 0.0 {
                    lo - 0.15
                } else {
                    reference + z0
                };
                structures.push(Structure {
                    kind: StructureKind::Place(*part),
                    footprint: world_poly(poly),
                    z0_m: bottom,
                    z1_m: reference + z1,
                });
            }
        }
    }
    let rot = |n: V2| geom::add(geom::scale(frame.along, n[0]), geom::scale(frame.out, n[1]));
    for k in kit_signs {
        let at = frame.world(k.c);
        let reference = if k.free { ground(map, at) } else { floor };
        layout.signs.push(SignText {
            text: k.text,
            centre: [at[0], at[1], reference + k.z],
            normal: rot(k.normal),
            size: k.size,
            both_sides: k.both,
            depth_m: k.depth,
        });
    }
    for (a, b) in kit_bunting {
        let (pa, pb) = (frame.world([a[0], a[1]]), frame.world([b[0], b[1]]));
        layout.bunting.push([
            [pa[0], pa[1], ground(map, pa) + a[2]],
            [pb[0], pb[1], ground(map, pb) + b[2]],
        ]);
    }
    (layout, buildings, structures)
}

/// Lay out every place of `map` ([`place_specs`]); places without a clear plot are left out.
/// Returns the layouts with the buildings and structures of each.
pub fn plan_with_pieces(map: &CityMap) -> Vec<(PlaceLayout, Vec<Building>, Vec<Structure>)> {
    let mut taken: Vec<Vec<V2>> = Vec::new();
    let mut out = Vec::new();
    for spec in place_specs(map) {
        if let Some(p) = lay_out(map, &spec, &taken) {
            let (layout, b, s) = finish(map, p);
            taken.push(layout.plot());
            out.push((layout, b, s));
        }
    }
    out
}

/// The layouts of `map`'s places (empty for maps without places, [`has_places`]): what the
/// renderer needs for sign text and string flags.
pub fn plan(map: &CityMap) -> Vec<PlaceLayout> {
    if !has_places(&map.net) {
        return Vec::new();
    }
    plan_with_pieces(map).into_iter().map(|p| p.0).collect()
}

/// Build every place of `net`: drop earlier place pieces, lay the places out, clear the
/// generic buildings and the decor on their plots, add their buildings and structures.
/// Port Vesper v2 and Ashpeak County call this last thing in generation; Ashpeak passes
/// `base` (`vesper_v2`), whose pieces it keeps for every place laid out on exactly the same
/// plot (the two maps' ground differs by centimetres, and identical pieces ship once in the
/// shared greybox part).
pub fn add_place_buildings(net: &mut RoadNetwork, base: Option<&RoadNetwork>) {
    net.buildings.retain(|b| !b.kind.is_place());
    net.features
        .structures
        .retain(|s| !matches!(s.kind, StructureKind::Place(_)));
    let map = CityMap::new(net.clone());
    let mut placed = plan_with_pieces(&map);
    drop(map);
    if let Some(base) = base.filter(|b| has_places(b)) {
        let from_base = plan_with_pieces(&CityMap::new(base.clone()));
        for (l, b, s) in &mut placed {
            let same = from_base.iter().find(|(x, _, _)| {
                x.id == l.id
                    && x.style == l.style
                    && x.origin == l.origin
                    && x.along == l.along
                    && x.size == l.size
            });
            if let Some((_, bb, bs)) = same {
                *b = bb.clone();
                *s = bs.clone();
            }
        }
    }
    let plots: Vec<Vec<V2>> = placed
        .iter()
        .map(|(l, _, _)| {
            // The plot grown by 1 m all round.
            let (hw, d) = (l.size[0] * 0.5 + 1.0, l.size[1] + 1.0);
            ccw(vec![
                l.world(-hw, -0.5),
                l.world(hw, -0.5),
                l.world(hw, d),
                l.world(-hw, d),
            ])
        })
        .collect();
    let protected = |k: BuildingKind| {
        matches!(
            k,
            BuildingKind::Landmark | BuildingKind::Farm | BuildingKind::Silo
        )
    };
    net.buildings.retain(|b| {
        protected(b.kind)
            || b.footprint.len() < 3
            || !plots.iter().any(|p| convex_overlap(&b.footprint, p))
    });
    net.decor.retain(|d| {
        let q = [d.pos[0], d.pos[1]];
        !plots.iter().any(|p| dist_to_poly(q, p) == 0.0)
    });
    for (_, b, s) in placed {
        net.buildings.extend(b);
        net.features.structures.extend(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_dir_is_counter_clockwise_and_overlap_works() {
        let r = rect_dir([0.0, 0.0], geom::normalize([1.0, 1.0]), 2.0, 4.0);
        assert_eq!(ccw(r.clone()), r);
        let a = rect(0.0, 2.0, 0.0, 2.0);
        assert!(convex_overlap(&a, &rect(1.0, 3.0, 1.0, 3.0)));
        assert!(!convex_overlap(&a, &rect(2.5, 3.0, 0.0, 2.0)));
        assert_eq!(dist_to_poly([1.0, 1.0], &a), 0.0);
        assert!((dist_to_poly([3.0, 1.0], &a) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn every_look_has_a_main_building_and_text_where_it_should() {
        for style in [
            PlaceStyle::Dealership,
            PlaceStyle::UsedLot,
            PlaceStyle::Junkyard,
            PlaceStyle::RepairShop,
            PlaceStyle::PoliceStation,
            PlaceStyle::Safehouse,
            PlaceStyle::LockUp,
            PlaceStyle::TowerGarage,
            PlaceStyle::Barn,
        ] {
            let k = kit_for(style, "Test Name");
            assert!(
                k.pieces.iter().any(|p| matches!(p, Piece::Building { .. })),
                "{style:?} has no building"
            );
            let size = plot_size(style);
            for p in &k.pieces {
                for q in p.poly() {
                    assert!(
                        q[0].abs() <= size[0] * 0.5 + 1e-3 && q[1] >= 0.0 && q[1] <= size[1],
                        "{style:?} piece outside its plot at {q:?}"
                    );
                }
            }
            // Only the safehouse is unmarked; the police sign says the generic word.
            assert_eq!(
                k.signs.is_empty(),
                style == PlaceStyle::Safehouse,
                "{style:?}"
            );
            if style == PlaceStyle::PoliceStation {
                assert!(k.signs.iter().all(|s| s.text == "POLICE"));
            }
        }
    }
}
