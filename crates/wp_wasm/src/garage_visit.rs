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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldShop {
    /// Shop name (the `Garage` POI's name).
    pub name: &'static str,
    /// District the shop is in.
    pub district: DistrictId,
    /// Legit or crooked ([`shop_kind`]).
    pub kind: ShopKind,
    /// Spawn city id (`port_vesper`, `ashpeak_city`, `harnell_crossing`).
    pub city: &'static str,
}

/// Every repair shop of the combined world, in POI order.
pub const WORLD_SHOPS: [WorldShop; 5] = [
    WorldShop {
        name: "Meridian Motorworks",
        district: DistrictId::Core,
        kind: ShopKind::Legit,
        city: "port_vesper",
    },
    WorldShop {
        name: "Sal's Paint & Panel",
        district: DistrictId::Suburb,
        kind: ShopKind::Crooked,
        city: "port_vesper",
    },
    WorldShop {
        name: "Gantry Garage",
        district: DistrictId::Port,
        kind: ShopKind::Crooked,
        city: "port_vesper",
    },
    WorldShop {
        name: "Spire Auto Body",
        district: DistrictId::Core,
        kind: ShopKind::Legit,
        city: "ashpeak_city",
    },
    WorldShop {
        name: "Harnell Feed & Tractor",
        district: DistrictId::Suburb,
        kind: ShopKind::Legit,
        city: "harnell_crossing",
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
}

/// An open career place visit (JSON for the testbed's place panel; `place_json`): the car
/// stopped inside a garage or safehouse with no pursuit on and is held until the player
/// chooses (the testbed ends the run into the garage / workshop) or leaves.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlaceVisit {
    /// Increments with every visit (the panel redraws on a new one).
    pub seq: u32,
    /// Garage or safehouse.
    pub kind: PlaceKind,
    /// Place name.
    pub name: String,
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
