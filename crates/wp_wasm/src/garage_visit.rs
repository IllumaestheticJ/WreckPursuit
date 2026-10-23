//! Garage visits (lead): the city's `Garage` POIs as repair shops in the single-player demo.
//!
//! Stopping inside a garage's radius (no active pursuit) opens a visit: the session holds the
//! player's car on the brakes and offers G's `RepairAll` quote at that shop (its legit / crooked
//! kind, its district's labour rate, the car's current wanted level). "Repair" pays from the
//! ledger and repairs the car in place (replay op `Repair`); "Leave" just gives control back.
//! The car has to leave every garage radius before another visit can open.
//!
//! Each shop sits on a street-level lane: [`sites`] puts it on the street-level, non-highway
//! lane nearest its POI (the grid city's block POIs are 18 m off the road or under the
//! viaduct), and a car more than [`GARAGE_HEIGHT_WINDOW_M`] above or below it is not inside.
//! In a career run the visit offers "Enter garage" (the testbed ends the run there and opens
//! the career garage priced at this shop) instead of an in-place repair.
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
use wp_world::network::{LaneKind, PoiKind, RoadNetwork};
use wp_world::World;

use crate::run::LedgerLine;

/// Radius around a garage POI that counts as "in the garage", m (the same 15 m `wp_world`
/// gives every POI in `wp_contracts::PoiInfo::radius_m`).
pub const GARAGE_RADIUS_M: f32 = 15.0;

/// The car must be below this speed to pull into a garage, m/s (≈ 11 km/h): driving through
/// one at speed does not stop the car.
pub const GARAGE_ENTER_SPEED_MPS: f32 = 3.0;

/// A car more than this far above or below a shop is not in it (bridges, viaducts), m.
pub const GARAGE_HEIGHT_WINDOW_M: f32 = 4.0;

/// A lane more than this far above the terrain is a deck, not a street, m.
const STREET_DECK_GAP_M: f32 = 3.0;

/// A shop moves onto a street lane at most this far from its POI, m (else it keeps the POI).
const SHOP_SNAP_MAX_M: f32 = 60.0;

/// Crooked shops by name (`docs/design/CITY.md` §5.2). Every other named shop there is legit.
const CROOKED_SHOPS: [&str; 3] = ["Sal's Paint & Panel", "Gantry Garage", "Roundhouse Customs"];

/// Legit shops by name (`docs/design/CITY.md` §5.2).
const LEGIT_SHOPS: [&str; 3] = [
    "Meridian Motorworks",
    "Harbor Auto Body",
    "Northfield Tire & Service",
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

    /// The point `pos` is inside the garage: within the radius horizontally and
    /// [`GARAGE_HEIGHT_WINDOW_M`] vertically.
    pub fn contains(&self, pos: [f32; 3]) -> bool {
        (pos[0] - self.pos[0]).hypot(pos[1] - self.pos[1]) <= self.radius_m
            && (pos[2] - self.pos[2]).abs() <= GARAGE_HEIGHT_WINDOW_M
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

/// The pose on the street-level, non-highway drivable lane nearest `p` (3D distance), within
/// [`SHOP_SNAP_MAX_M`]: `(pos, yaw)`. `None` when there is none.
pub fn street_pose(net: &RoadNetwork, p: [f32; 3]) -> Option<([f32; 3], f32)> {
    let mut best: Option<([f32; 3], f32, f32)> = None;
    for l in &net.lanes {
        if !l.kind.drivable() || matches!(l.kind, LaneKind::Highway | LaneKind::Parking) {
            continue;
        }
        if net
            .surfaces
            .iter()
            .any(|sf| sf.road == l.road && sf.highway)
        {
            continue;
        }
        for w in l.polyline.windows(2) {
            let (a, b) = (w[0], w[1]);
            let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let len2 = ab[0] * ab[0] + ab[1] * ab[1];
            if len2 <= 0.0 {
                continue;
            }
            let t = (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / len2).clamp(0.0, 1.0);
            let q = [a[0] + ab[0] * t, a[1] + ab[1] * t, a[2] + ab[2] * t];
            if let Some(tf) = &net.terrain {
                if q[2] - tf.height_at([q[0], q[1]]) > STREET_DECK_GAP_M {
                    continue;
                }
            }
            let (dx, dy, dz) = (q[0] - p[0], q[1] - p[1], q[2] - p[2]);
            let d = (dx * dx + dy * dy + dz * dz).sqrt();
            if d <= SHOP_SNAP_MAX_M && best.is_none_or(|b| d < b.2) {
                best = Some((q, wp_math::atan2(-ab[0], ab[1]), d));
            }
        }
    }
    best.map(|b| (b.0, b.1))
}

/// Every `Garage` POI of the world's road network, in POI order, each on its nearest
/// street-level lane ([`street_pose`]; the POI itself when there is none). Shops outside every
/// zone are priced as residential.
pub fn sites(world: &World) -> Vec<GarageSite> {
    let net = world.network();
    net.pois
        .iter()
        .filter(|p| p.kind == PoiKind::Garage)
        .enumerate()
        .map(|(i, p)| {
            let (pos, heading_rad) = street_pose(net, p.pos).unwrap_or((p.pos, p.heading_rad));
            let district = world
                .map()
                .district_at([pos[0], pos[1]])
                .unwrap_or(DistrictId::Residential);
            GarageSite {
                index: i as u32,
                name: p.name.clone(),
                pos,
                heading_rad,
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
    /// "Repair" is possible (there is a quote and the player can pay it; never in a career
    /// run).
    pub can_repair: bool,
    /// A career run: the panel offers "Enter garage" (the career garage at this shop).
    pub career: bool,
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
    fn every_city_has_garages_on_a_street_lane() {
        use crate::data::mem_data;
        for id in ["vesper_v2", "procedural_12x12", "ashpeak"] {
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
            for g in &s {
                let (q, _) = street_pose(world.network(), g.pos)
                    .unwrap_or_else(|| panic!("{id}: {} has no street lane", g.name));
                let d = (q[0] - g.pos[0]).hypot(q[1] - g.pos[1]);
                assert!(d < 0.5, "{id}: {} is {d:.1} m off its street lane", g.name);
            }
        }
    }

    #[test]
    fn a_car_above_or_below_a_shop_is_not_in_it() {
        let g = GarageSite {
            index: 0,
            name: "Test".into(),
            pos: [0.0, 0.0, 2.0],
            heading_rad: 0.0,
            radius_m: GARAGE_RADIUS_M,
            kind: ShopKind::Legit,
            district: DistrictId::Core,
        };
        assert!(g.contains([3.0, 4.0, 3.0]));
        assert!(!g.contains([3.0, 4.0, 10.6]), "on a deck above");
        assert!(!g.contains([3.0, 4.0, -4.5]), "in a tunnel below");
        assert!(!g.contains([20.0, 0.0, 2.0]));
    }
}
