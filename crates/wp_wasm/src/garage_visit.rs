//! Garage visits (lead): the city's `Garage` POIs as repair shops in the single-player demo.
//!
//! Stopping inside a garage's radius (no active pursuit) opens a visit: the session holds the
//! player's car on the brakes and offers G's `RepairAll` quote at that shop (its legit / crooked
//! kind, its district's labour rate, the car's current wanted level). "Repair" pays from the
//! ledger and repairs the car in place (replay op `Repair`); "Leave" just gives control back.
//! The car has to leave every garage radius before another visit can open.
//!
//! Like [`crate::run`], all of this sits outside the deterministic sim step: it reads the sim
//! and only acts on it through logged inputs and ops, so replays reproduce a visit.
//!
//! Shop kind: the road network's POIs carry no legit / crooked flag yet (`docs/design/CITY.md`
//! change request C3). Until they do, [`shop_kind`] uses the CITY.md §5.2 shop list by name and
//! falls back to the district (crooked in industrial and port districts, legit elsewhere).

use serde::Serialize;
use wp_contracts::{DistrictId, WantedLevel};
use wp_economy::{RepairQuote, ShopContext, ShopKind};
use wp_world::network::PoiKind;
use wp_world::World;

use crate::run::LedgerLine;

/// Radius around a garage POI that counts as "in the garage", m (the same 15 m `wp_world`
/// gives every POI in `wp_contracts::PoiInfo::radius_m`).
pub const GARAGE_RADIUS_M: f32 = 15.0;

/// The car must be below this speed to pull into a garage, m/s (≈ 11 km/h): driving through
/// one at speed does not stop the car.
pub const GARAGE_ENTER_SPEED_MPS: f32 = 3.0;

/// Crooked shops by name (`docs/design/CITY.md` §5.2). Every other named shop there is legit.
const CROOKED_SHOPS: [&str; 3] = ["Sal's Paint & Panel", "Gantry Garage", "Roundhouse Customs"];

/// Legit shops by name (`docs/design/CITY.md` §5.2).
const LEGIT_SHOPS: [&str; 3] = [
    "Meridian Motorworks",
    "Harbor Auto Body",
    "Northfield Tire & Service",
];

/// A repair shop of the combined world (`ashpeak`) as the career garage's Repair panel offers
/// it: the shop's name, its district (labour rate), legit or crooked, and the spawn city
/// ([`crate::scenario::SPAWNS`] id) it belongs to. The garage screen has no world loaded, so
/// this is a fixed table; `spawn_tests::world_shops_match_the_generated_world` checks it against
/// [`sites`] of the generated world (nearest spawn grid = the city).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldShop {
    /// Shop name (the `Garage` POI's name).
    pub name: &'static str,
    /// District the shop is in.
    pub district: DistrictId,
    /// Legit or crooked ([`shop_kind`]).
    pub kind: ShopKind,
    /// Spawn city id (`port_vesper`, `ashpeak_city`, `harnell_crossing`).
    pub city: &'static str,
    /// Where the shop's POI stands `[x, y]`, m (tow distances; checked against the world).
    pub pos: [f32; 2],
}

/// Every repair shop of the combined world, in POI order.
pub const WORLD_SHOPS: [WorldShop; 5] = [
    WorldShop {
        name: "Meridian Motorworks",
        district: DistrictId::Core,
        kind: ShopKind::Legit,
        city: "port_vesper",
        pos: [193.25, -100.0],
    },
    WorldShop {
        name: "Sal's Paint & Panel",
        district: DistrictId::Suburb,
        kind: ShopKind::Crooked,
        city: "port_vesper",
        pos: [-897.88, -5.29],
    },
    WorldShop {
        name: "Gantry Garage",
        district: DistrictId::Port,
        kind: ShopKind::Crooked,
        city: "port_vesper",
        pos: [700.0, -919.75],
    },
    WorldShop {
        name: "Spire Auto Body",
        district: DistrictId::Core,
        kind: ShopKind::Legit,
        city: "ashpeak_city",
        pos: [-450.0, 4798.25],
    },
    WorldShop {
        name: "Harnell Feed & Tractor",
        district: DistrictId::Suburb,
        kind: ShopKind::Legit,
        city: "harnell_crossing",
        pos: [-3696.49, 165.76],
    },
];

/// One repair shop on the map (JSON for the testbed's markers; `garages_json`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GarageSite {
    /// Index in the session's garage list.
    pub index: u32,
    /// Shop name (the POI's name).
    pub name: String,
    /// Position, m.
    pub pos: [f32; 3],
    /// Facing (yaw about +Z, 0 = +Y), rad.
    pub heading_rad: f32,
    /// Radius that counts as inside, m.
    pub radius_m: f32,
    /// Legit or crooked.
    pub kind: ShopKind,
    /// District the shop is in (labour rate).
    pub district: DistrictId,
}

impl GarageSite {
    /// The shop as the economy prices it, for a car at `wanted`.
    pub fn shop(&self, wanted: WantedLevel) -> ShopContext {
        ShopContext {
            district: self.district,
            kind: self.kind,
            wanted,
        }
    }

    /// The point `[x, y]` is inside the garage radius.
    pub fn contains(&self, xy: [f32; 2]) -> bool {
        (xy[0] - self.pos[0]).hypot(xy[1] - self.pos[1]) <= self.radius_m
    }
}

/// Legit or crooked for a shop named `name` in `district` (see the module docs).
pub fn shop_kind(name: &str, district: DistrictId) -> ShopKind {
    if CROOKED_SHOPS.contains(&name) {
        ShopKind::Crooked
    } else if LEGIT_SHOPS.contains(&name) {
        ShopKind::Legit
    } else if matches!(district, DistrictId::Industrial | DistrictId::Port) {
        ShopKind::Crooked
    } else {
        ShopKind::Legit
    }
}

/// Every `Garage` POI of the world's road network, in POI order. Shops outside every zone are
/// priced as residential.
pub fn sites(world: &World) -> Vec<GarageSite> {
    world
        .network()
        .pois
        .iter()
        .filter(|p| p.kind == PoiKind::Garage)
        .enumerate()
        .map(|(i, p)| {
            let district = world
                .map()
                .district_at([p.pos[0], p.pos[1]])
                .unwrap_or(DistrictId::Residential);
            GarageSite {
                index: i as u32,
                name: p.name.clone(),
                pos: p.pos,
                heading_rad: p.heading_rad,
                radius_m: GARAGE_RADIUS_M,
                kind: shop_kind(&p.name, district),
                district,
            }
        })
        .collect()
}

/// An open garage visit: what the shop offers (JSON for the testbed's garage panel;
/// `garage_visit_json`). Priced once when the car pulls in (it is held still meanwhile).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GarageOffer {
    /// 1, 2, … per visit in this session.
    pub seq: u32,
    /// Sim tick the visit opened.
    pub tick: u64,
    /// The shop.
    pub site: GarageSite,
    /// The car's wanted level when it pulled in.
    pub wanted: WantedLevel,
    /// Car health 0..=1.
    pub health: f32,
    /// G's `RepairAll` quote at this shop (`None`: nothing to repair, or refused; see `note`).
    pub quote: Option<RepairQuote>,
    /// Why there is no quote (`"nothing to repair"`, or the economy's refusal).
    pub note: Option<String>,
    /// Cash on the account.
    pub cash: i64,
    /// "Repair" is possible (there is a quote and the player can pay it).
    pub can_repair: bool,
}

/// What "Repair" at a garage did (`garage_repair`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GarageRepairReport {
    /// Shop name.
    pub shop: String,
    /// What the player paid.
    pub paid: i64,
    /// Paid by insurance.
    pub insurance_covers: i64,
    /// The shop reported the car to the police (legit shop, wanted car).
    pub reported_to_police: bool,
    /// Cash afterwards.
    pub cash_after: i64,
    /// Ledger lines it added.
    pub ledger: Vec<LedgerLine>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_shops_follow_the_city_doc_and_others_their_district() {
        assert_eq!(
            shop_kind("Gantry Garage", DistrictId::Core),
            ShopKind::Crooked
        );
        assert_eq!(
            shop_kind("Meridian Motorworks", DistrictId::Port),
            ShopKind::Legit
        );
        assert_eq!(shop_kind("Garage 1", DistrictId::Port), ShopKind::Crooked);
        assert_eq!(
            shop_kind("Garage 2", DistrictId::Industrial),
            ShopKind::Crooked
        );
        assert_eq!(shop_kind("Garage 3", DistrictId::Core), ShopKind::Legit);
    }

    #[test]
    fn every_city_has_garages() {
        use crate::data::mem_data;
        for id in ["vesper_v2", "procedural_12x12"] {
            let sc = crate::scenario::free_drive_in(
                "debug_sedan",
                wp_contracts::DamageMode::Arcade,
                crate::scenario::city_spec(id),
            );
            let world =
                simcli::data::load_world(mem_data(), &sc.city, simcli::run::world_config(&sc))
                    .unwrap()
                    .unwrap();
            let s = sites(&world);
            assert!(!s.is_empty(), "{id}: no garages");
            assert!(s.iter().enumerate().all(|(i, g)| g.index == i as u32));
        }
    }
}

// ----- career places (garages and safehouses in a career run) ---------------------------------

/// Radius around a safehouse POI that counts as inside, m (the police safehouse rule uses the
/// POI's own radius; this is the same 15 m `wp_world` gives every POI).
pub const SAFEHOUSE_RADIUS_M: f32 = 15.0;

/// One safehouse on the map (JSON for the testbed's markers; `safehouses_json`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SafehouseSite {
    /// Safehouse name (the POI's name).
    pub name: String,
    /// Position, m.
    pub pos: [f32; 3],
    /// Radius that counts as inside, m.
    pub radius_m: f32,
}

impl SafehouseSite {
    /// The point `[x, y]` is inside the safehouse radius.
    pub fn contains(&self, xy: [f32; 2]) -> bool {
        (xy[0] - self.pos[0]).hypot(xy[1] - self.pos[1]) <= self.radius_m
    }
}

/// Every safehouse POI of the world's road network, in POI order.
pub fn safehouse_sites(world: &World) -> Vec<SafehouseSite> {
    world
        .network()
        .pois
        .iter()
        .filter(|p| p.kind == PoiKind::SafehouseCandidate)
        .map(|p| SafehouseSite {
            name: p.name.clone(),
            pos: p.pos,
            radius_m: SAFEHOUSE_RADIUS_M,
        })
        .collect()
}

/// Kind of career place the car pulled into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PlaceKind {
    /// A repair / mod garage: the workshop and the career garage open from here.
    Garage,
    /// A safehouse: end the night safely (the run is saved) or work on the car.
    Safehouse,
    /// A car lot: walk the rows and buy (`career_visit_lot`).
    Lot,
    /// One of the player's own garages: the workshop, the garage, or end the night.
    Home,
}

/// An open career place visit (JSON for the testbed's place panel; `place_json`): the car
/// stopped inside a garage or safehouse with no pursuit on. It stays open until the player
/// chooses (the testbed ends the run into the garage / workshop), leaves, or drives out of the
/// ring; the car is not held meanwhile.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlaceVisit {
    /// Increments with every visit (the panel redraws on a new one).
    pub seq: u32,
    /// Garage or safehouse.
    pub kind: PlaceKind,
    /// Place name.
    pub name: String,
    /// Lot or home garage id (`None` for shops and safehouses, which go by name).
    pub id: Option<String>,
    /// Legit or crooked (garages only).
    pub shop: Option<ShopKind>,
    /// The car's wanted level (heat left over without a pursuit).
    pub wanted: WantedLevel,
}

/// Laying low in a safehouse during a pursuit (JSON; `place_json`): the police safehouse rule
/// ends the pursuit after `need_s` inside with no unit within `clear_radius_m`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LayingLow {
    /// Safehouse name.
    pub name: String,
    /// Seconds of quiet dwell so far.
    pub dwell_s: f32,
    /// Seconds needed.
    pub need_s: f32,
    /// No unit may be closer than this, m.
    pub clear_radius_m: f32,
}

/// What `place_json` returns: the open visit and the safehouse dwell during a pursuit.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct PlaceStatus {
    /// The open visit, if any.
    pub visit: Option<PlaceVisit>,
    /// Inside a safehouse while chased, if so.
    pub laying_low: Option<LayingLow>,
}

// ----- drive-to places: car lots and the player's own garages ---------------------------------
//
// Owner, 2026-10-27: dealers, garages and workshops can only be reached by driving to them (the
// menus only set a map waypoint), and the player has three garages of their own, unlocked at 0,
// 2 000 and 5 000 rep, that career runs start from. The road network has no POIs for either, so
// both stand at fixed anchors of the combined world (`ashpeak`) snapped onto the nearest lane
// when a session loads ([`snap_to_lane`]); `spawn_tests::drive_to_places_stand_on_roads_apart`
// checks the anchors against the generated world.

/// Radius around a car lot or home garage that counts as inside, m (as every POI).
pub const PLACE_RADIUS_M: f32 = 15.0;

/// Where a car lot (`data/economy/lots.toml` id) stands in the combined world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldLot {
    /// Lot id.
    pub lot: &'static str,
    /// Anchor `[x, y]`, m; the lot's ring sits on the lane nearest it.
    pub anchor: [f32; 2],
    /// Spawn city id the lot belongs to ([`crate::scenario::SPAWNS`]).
    pub city: &'static str,
}

/// Every car lot of the combined world: each in a zone of its kind of district where the city
/// has one (Port Vesper has most; Ashpeak City the prestige dealer, Harnell the scrapyard).
pub const WORLD_LOTS: [WorldLot; 7] = [
    WorldLot {
        lot: "budget_motors",
        anchor: [-1025.0, 115.0],
        city: "port_vesper",
    },
    WorldLot {
        lot: "harbourline_auto",
        anchor: [50.0, -535.0],
        city: "port_vesper",
    },
    WorldLot {
        lot: "meridian_prestige",
        anchor: [-700.0, 4650.0],
        city: "ashpeak_city",
    },
    WorldLot {
        lot: "lucky_lou_used",
        anchor: [-700.0, -975.0],
        city: "port_vesper",
    },
    WorldLot {
        lot: "certified_preowned",
        anchor: [0.0, 660.0],
        city: "port_vesper",
    },
    WorldLot {
        lot: "dockside_salvage",
        anchor: [1000.0, -975.0],
        city: "port_vesper",
    },
    WorldLot {
        lot: "ridgeback_scrap",
        anchor: [-3150.0, 300.0],
        city: "harnell_crossing",
    },
];

/// Where one of the player's own garages (`career.toml [[home_garages]]` id) stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HomeSpot {
    /// Home garage id.
    pub id: &'static str,
    /// Anchor `[x, y]`, m; the garage's ring (and the spawn) sits on the lane nearest it.
    pub anchor: [f32; 2],
    /// Spawn city id it is in.
    pub city: &'static str,
}

/// The player's garages: one per spawn city, in unlock order.
pub const HOME_GARAGES: [HomeSpot; 3] = [
    HomeSpot {
        id: "vesper_lockup",
        anchor: [0.0, 225.0],
        city: "port_vesper",
    },
    HomeSpot {
        id: "spire_loft",
        anchor: [190.0, 4650.0],
        city: "ashpeak_city",
    },
    HomeSpot {
        id: "harnell_barn",
        anchor: [-3300.0, 150.0],
        city: "harnell_crossing",
    },
];

/// Home garage `id`'s spot, if there is one.
pub fn home_spot(id: &str) -> Option<&'static HomeSpot> {
    HOME_GARAGES.iter().find(|h| h.id == id)
}

/// The home garage whose anchor is (within 1 m of) `xy`: the career puts the player there to
/// start a run from that garage ([`crate::session::Session::new`] then spawns on its lane).
pub fn home_at_anchor(xy: [f32; 2]) -> Option<&'static HomeSpot> {
    HOME_GARAGES
        .iter()
        .find(|h| (h.anchor[0] - xy[0]).hypot(h.anchor[1] - xy[1]) < 1.0)
}

/// The point of the drivable lane nearest `anchor` and the lane's facing (yaw, rad).
pub fn snap_to_lane(world: &World, anchor: [f32; 2]) -> Option<([f32; 3], f32)> {
    let map = world.map();
    let (seg, s) = wp_world::routes::nearest_lane(map, [anchor[0], anchor[1], 0.0], None)?;
    let (pos, tan) = map.seg(seg).point_at(s);
    Some((pos, wp_world::geom::yaw_of(tan)))
}

/// Kind of drive-to place on the map (`places_json`; the waypoint picker uses the same names).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MapPlaceKind {
    /// A repair shop ([`GarageSite`]).
    Garage,
    /// A safehouse.
    Safehouse,
    /// A car lot (dealership, used lot, junkyard).
    Lot,
    /// One of the player's own garages.
    Home,
}

/// A car lot or home garage in a career session.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlaceSite {
    /// Lot or home garage id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Position (on the lane), m.
    pub pos: [f32; 3],
    /// Facing along the lane, rad.
    pub heading_rad: f32,
    /// Radius that counts as inside, m.
    pub radius_m: f32,
}

impl PlaceSite {
    /// The point `[x, y]` is inside the place's radius.
    pub fn contains(&self, xy: [f32; 2]) -> bool {
        (xy[0] - self.pos[0]).hypot(xy[1] - self.pos[1]) <= self.radius_m
    }

    /// `(id, name)` placed at `anchor` in `world` (`None`: the world has no lanes).
    pub fn at(world: &World, id: &str, name: &str, anchor: [f32; 2]) -> Option<Self> {
        let (pos, heading_rad) = snap_to_lane(world, anchor)?;
        Some(Self {
            id: id.to_string(),
            name: name.to_string(),
            pos,
            heading_rad,
            radius_m: PLACE_RADIUS_M,
        })
    }
}

/// One entry of `places_json()`: every drive-to place of the session, for the map markers and
/// the waypoint (the menus name a place; its position comes from here).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MapPlace {
    /// Kind.
    pub kind: MapPlaceKind,
    /// Lot / home id, else the name.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Position, m.
    pub pos: [f32; 3],
    /// Radius that counts as inside, m.
    pub radius_m: f32,
    /// Repair shops: legit or crooked.
    pub shop: Option<ShopKind>,
}

// ----- where a career run starts ---------------------------------------------------------------
//
// Owner, 2026-10-29: a run starts where the active car is (the shop the tow took it to, one of
// the player's garages), on the road by that place's ring, facing along the road and just
// outside the ring, so the place's panel does not open the moment the run starts.

/// Clearance a run's spawn keeps from every place ring, m (beyond the ring's radius).
pub const SPAWN_CLEAR_M: f32 = 4.0;

/// Farthest a run's spawn may be from its place, m.
pub const SPAWN_MAX_M: f32 = 55.0;

/// Distances along the road from the place, m, tried in order (each ahead, then behind).
const SPAWN_STEPS_M: [f32; 5] = [24.0, 30.0, 38.0, 46.0, 54.0];

/// A place a career run starts from ([`start_place_at`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StartPlace {
    /// One of the player's garages (its ring sits on the lane nearest its anchor).
    Home(&'static HomeSpot),
    /// A repair shop (its ring is centred on the shop's POI).
    Shop(&'static WorldShop),
}

impl StartPlace {
    /// The point the career puts the player's scenario position on to start here
    /// ([`start_place_at`] finds the place again from it).
    pub fn anchor(&self) -> [f32; 2] {
        match self {
            Self::Home(h) => h.anchor,
            Self::Shop(s) => s.pos,
        }
    }

    /// Where the place's ring is centred in `world`, `[x, y]` m.
    pub fn ring_centre(&self, world: &World) -> Option<[f32; 2]> {
        match self {
            Self::Home(h) => snap_to_lane(world, h.anchor).map(|(p, _)| [p[0], p[1]]),
            Self::Shop(s) => Some(s.pos),
        }
    }
}

/// The place a career run starts from whose anchor ([`StartPlace::anchor`]) is within 1 m of
/// `xy`: a home garage ([`HOME_GARAGES`]) or a repair shop ([`WORLD_SHOPS`]). The career puts
/// the player's scenario position there ([`crate::career::Career::start_run`]);
/// [`crate::session::Session::new`] then spawns by that place's ring ([`run_spawn`]).
pub fn start_place_at(xy: [f32; 2]) -> Option<StartPlace> {
    let near = |p: [f32; 2]| (p[0] - xy[0]).hypot(p[1] - xy[1]) < 1.0;
    if let Some(h) = HOME_GARAGES.iter().find(|h| near(h.anchor)) {
        return Some(StartPlace::Home(h));
    }
    WORLD_SHOPS
        .iter()
        .find(|s| near(s.pos))
        .map(StartPlace::Shop)
}

/// Every place ring of `world` a career run can see: repair shops, safehouses, car lots and
/// the player's garages, as `(centre [x, y], radius m)`.
pub fn all_rings(world: &World) -> Vec<([f32; 2], f32)> {
    let mut out: Vec<([f32; 2], f32)> = sites(world)
        .iter()
        .map(|g| ([g.pos[0], g.pos[1]], g.radius_m))
        .collect();
    out.extend(
        safehouse_sites(world)
            .iter()
            .map(|h| ([h.pos[0], h.pos[1]], h.radius_m)),
    );
    let anchors = WORLD_LOTS
        .iter()
        .map(|l| l.anchor)
        .chain(HOME_GARAGES.iter().map(|h| h.anchor));
    for a in anchors {
        if let Some((p, _)) = snap_to_lane(world, a) {
            out.push(([p[0], p[1]], PLACE_RADIUS_M));
        }
    }
    out
}

/// Where a run that starts at the place whose ring is centred on `centre` spawns: on a drivable
/// lane by the ring, facing along it (away from the place), outside every ring of `rings`
/// (`(centre, radius)`, [`all_rings`]) by [`SPAWN_CLEAR_M`], at most [`SPAWN_MAX_M`] from
/// `centre`. Returns `(pos, yaw)`; `None`: no such lane point (the caller falls back to the
/// lane at the place).
pub fn run_spawn(
    world: &World,
    centre: [f32; 2],
    rings: &[([f32; 2], f32)],
) -> Option<([f32; 3], f32)> {
    let map = world.map();
    let (seg, s) = wp_world::routes::nearest_lane(map, [centre[0], centre[1], 0.0], None)?;
    let (p0, tan) = map.seg(seg).point_at(s);
    let dist = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
    let clear = |q: [f32; 2]| rings.iter().all(|(c, r)| dist(q, *c) > r + SPAWN_CLEAR_M);
    for d in SPAWN_STEPS_M {
        // Ahead along the lane, then behind the place on a lane running the other way.
        for sign in [1.0f32, -1.0] {
            let dir = [tan[0] * sign, tan[1] * sign];
            let probe = [p0[0] + dir[0] * d, p0[1] + dir[1] * d, 0.0];
            let Some((seg2, s2)) = wp_world::routes::nearest_lane(map, probe, Some(dir)) else {
                continue;
            };
            let (p, t) = map.seg(seg2).point_at(s2);
            let q = [p[0], p[1]];
            let from = dist(q, centre);
            if from <= PLACE_RADIUS_M + SPAWN_CLEAR_M || from > SPAWN_MAX_M || !clear(q) {
                continue;
            }
            return Some((p, wp_world::geom::yaw_of(t)));
        }
    }
    None
}
