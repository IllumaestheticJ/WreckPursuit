//! Car lots: dealerships, used car lots and junkyards (`data/economy/lots.toml`), their daily
//! stock, buying a listed car, and tyres (worn by driving, replaced on their own: a repair
//! never fits tyres).
//!
//! **Stock** is a pure function of (career stock seed, lot, in-game day, roster):
//! [`Economy::lot_stock`] seeds a [`wp_math::Rng`] from the career's
//! [`crate::CareerProfile::stock_seed`], the lot id and the day. So every career has its own
//! stock ("random per career by the day"), every lot restocks each day, the same day of the
//! same career always shows the same cars (reloading or reopening a lot never rerolls), and
//! not every model a lot carries is on the floor every day. Everything a listing rolls (the
//! model, used-car wear, junkyard faults, rare variants, paint) comes from that one stream.
//!
//! - Dealerships sell brand new cars (each model at most once a day).
//! - Used lots sell running cars with wear in the lot's condition band: dented panels, tired
//!   components, worn tyres. Everything but the tyres is fixed by an ordinary repair.
//! - Junkyards sell barely running or dead cars (a failed engine), sometimes with missing
//!   panels and bald or flat tyres. The mechanical state is hidden until bought
//!   ([`Listing::hidden`]); now and then a rare variant (a model with mods) turns up.
//!
//! A listing carries its wear as data ([`ListingWear`]); the damage system turns it into
//! lattice damage when the car is bought (the browser career does this with
//! `wp_damage::DamageModel::apply_preowned_wear`). The economy alone records the listing's
//! estimated condition.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use wp_contracts::{BodyRegion, Component, DistrictId, VehicleClass};
use wp_math::{Rng, StateHasher};

use crate::data::Economy;
use crate::error::EconomyError;
use crate::garage::{CarSpec, Condition};
use crate::ledger::{
    transact, CarLocation, CarUid, DamageSnapshot, EconResult, EventReason, Money, OwnedCar,
    PlayerAccount, TireState,
};

/// RNG salt of the daily lot stock.
const STOCK_SALT: u64 = 0x10_75_70_C4;

/// What kind of place a lot is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LotKind {
    /// Brand new cars.
    Dealership,
    /// Running used cars of varying condition.
    UsedLot,
    /// Barely running or dead cars, sometimes with missing parts; condition hidden.
    Junkyard,
}

/// One lot (`[[lots]]` in `lots.toml`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LotDef {
    /// Unique id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Dealership, used lot or junkyard.
    pub kind: LotKind,
    /// Tier 1 (cheapest) ..= 3 (prestige).
    pub tier: u8,
    /// District the lot is in.
    pub district: DistrictId,
    /// Classes the lot carries.
    pub classes: Vec<VehicleClass>,
    /// Lowest base price the lot carries.
    pub min_base_price: u32,
    /// Highest base price the lot carries.
    pub max_base_price: u32,
    /// Parking spaces.
    pub slots: u32,
    /// Chance each space holds a car on a given day.
    pub fill_chance: f32,
    /// Price multiplier on the lot's market price.
    pub price_scale: f32,
    /// Condition band `[lo, hi]` (used lots, junkyards).
    #[serde(default)]
    pub condition: Option<[f32; 2]>,
    /// Tyre tread band `[lo, hi]` (used lots, junkyards).
    #[serde(default)]
    pub tread: Option<[f32; 2]>,
    /// Junkyard price as a fraction of the base price, `[lo, hi]`.
    #[serde(default)]
    pub price_frac: Option<[f32; 2]>,
    /// Junkyard: chance a car does not run (failed engine).
    #[serde(default)]
    pub non_running_chance: f32,
    /// Junkyard: chance each bolt-on panel is missing.
    #[serde(default)]
    pub missing_part_chance: f32,
    /// Junkyard: chance each tyre is flat.
    #[serde(default)]
    pub flat_tire_chance: f32,
    /// Junkyard: chance a filled space holds a rare variant.
    #[serde(default)]
    pub rare_chance: f32,
}

/// A rare variant found only in junkyards (`[[rare_variants]]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RareVariant {
    /// Unique id.
    pub id: String,
    /// Base car id.
    pub def_id: String,
    /// Display name.
    pub name: String,
    /// Mods it comes with (installed as found).
    #[serde(default)]
    pub mods: Vec<String>,
}

/// Tyre prices and wear (`[tires]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TireTables {
    /// Price of one fitted tyre, by class.
    pub price_per_tire: BTreeMap<VehicleClass, Money>,
    /// Grip of a bald tyre relative to a new one.
    pub bald_grip: f32,
    /// Tread below this reads as bald.
    pub bald_below: f32,
    /// Tread lost per hour of driving.
    pub wear_per_hour: f32,
}

/// `data/economy/lots.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LotTables {
    /// Tyres.
    pub tires: TireTables,
    /// Every lot, in display order.
    pub lots: Vec<LotDef>,
    /// Junkyard rare variants.
    #[serde(default)]
    pub rare_variants: Vec<RareVariant>,
}

/// Wear a listed car comes with (applied to the lattice by the damage system on purchase).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ListingWear {
    /// Component health 0..=1 (0 = failed).
    pub components: BTreeMap<Component, f32>,
    /// Missing bolt-on parts (by region).
    pub missing: BTreeSet<BodyRegion>,
    /// Dents per region, as a fraction of a replace-deep dent.
    pub dents: BTreeMap<BodyRegion, f32>,
}

/// How a listed car is sold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListingGrade {
    /// Brand new.
    New,
    /// Used, running.
    Used,
    /// Junk: maybe running, maybe not.
    Junk,
}

/// One car on a lot today.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Listing {
    /// Lot id.
    pub lot: String,
    /// Parking space (stable within the day; the key a purchase names).
    pub slot: u32,
    /// Base car id.
    pub def_id: String,
    /// Class.
    pub class: VehicleClass,
    /// Base price of the model.
    pub base_price: u32,
    /// Rare variant id, if it is one.
    pub variant: Option<String>,
    /// Rare variant name, if it is one.
    pub variant_name: Option<String>,
    /// Mods the car comes with.
    pub mods: Vec<String>,
    /// New / used / junk.
    pub grade: ListingGrade,
    /// Asking price.
    pub price: Money,
    /// Rolled condition 0..=1 (the damage system computes the exact health on purchase).
    pub condition: f32,
    /// Wear (applied on purchase).
    pub wear: ListingWear,
    /// Tyres it comes on.
    pub tires: TireState,
    /// The engine runs.
    pub runs: bool,
    /// Junkyard: the mechanical state (components, `runs`, `condition`) is not shown before
    /// buying; only what can be seen (missing panels, dents, tyres).
    pub hidden: bool,
    /// Factory paint, 0xRRGGBB: the car is drawn in it on the lot and keeps it once bought.
    #[serde(default)]
    pub paint: u32,
}

/// Factory paints lot cars are sold in (0xRRGGBB): reds, blues, greens, golds, purples,
/// teals, white, silver, black, orange.
pub const FACTORY_PAINTS: [u32; 12] = [
    0xd04a2a, 0x2a7fd0, 0x3fb950, 0xd0a92a, 0x9b59b6, 0x1abc9c, 0xd8d8d2, 0x8a9099, 0x1e1e22,
    0x7a1f1f, 0x2b3a55, 0xe07a1f,
];

/// Panels a lot car can be dented in.
const DENT_REGIONS: [BodyRegion; 13] = [
    BodyRegion::FrontBumper,
    BodyRegion::RearBumper,
    BodyRegion::Hood,
    BodyRegion::Trunk,
    BodyRegion::FrontLeftFender,
    BodyRegion::FrontRightFender,
    BodyRegion::LeftDoorF,
    BodyRegion::RightDoorF,
    BodyRegion::LeftDoorR,
    BodyRegion::RightDoorR,
    BodyRegion::RearLeftQuarter,
    BodyRegion::RearRightQuarter,
    BodyRegion::Roof,
];

/// Bolt-on parts a junkyard car can be missing.
const MISSING_REGIONS: [BodyRegion; 6] = [
    BodyRegion::Hood,
    BodyRegion::FrontBumper,
    BodyRegion::RearBumper,
    BodyRegion::Trunk,
    BodyRegion::LeftDoorF,
    BodyRegion::RightDoorF,
];

/// Components that wear on a used car (wheels / hubs are not: tyres are separate).
const WEAR_COMPONENTS: [Component; 17] = [
    Component::Engine,
    Component::Gearbox,
    Component::Radiator,
    Component::Battery,
    Component::Exhaust,
    Component::SteeringRack,
    Component::Driveshaft,
    Component::FuelTank,
    Component::Lights,
    Component::SuspensionFL,
    Component::SuspensionFR,
    Component::SuspensionRL,
    Component::SuspensionRR,
    Component::BrakeFL,
    Component::BrakeFR,
    Component::BrakeRL,
    Component::BrakeRR,
];

fn money(v: f32) -> Money {
    v.round() as Money
}

fn round2(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

fn pick<T: Copy>(rng: &mut Rng, items: &[T]) -> T {
    items[rng.below(items.len() as u32) as usize]
}

impl Economy {
    /// A lot by id.
    pub fn lot(&self, id: &str) -> Option<&LotDef> {
        self.lots.lots.iter().find(|l| l.id == id)
    }

    /// The lot's stock on `day` for the career with `stock_seed`
    /// ([`crate::CareerProfile::stock_seed`]), by slot. `roster` is every model the market
    /// sells (order does not matter; police classes are ignored). Deterministic in
    /// (stock seed, lot, day, roster): a purchase names a listing by (lot, day, slot) and finds
    /// the same car by calling this again with the same career's seed.
    pub fn lot_stock(
        &self,
        stock_seed: u32,
        lot_id: &str,
        day: u32,
        roster: &[CarSpec],
    ) -> Vec<Listing> {
        let Some(lot) = self.lot(lot_id) else {
            return Vec::new();
        };
        let mut models: Vec<&CarSpec> = roster
            .iter()
            .filter(|s| {
                lot.classes.contains(&s.class)
                    && (lot.min_base_price..=lot.max_base_price).contains(&s.base_price)
            })
            .collect();
        models.sort_by(|a, b| a.def_id.cmp(&b.def_id));
        models.dedup_by(|a, b| a.def_id == b.def_id);
        let rares: Vec<&RareVariant> = self
            .lots
            .rare_variants
            .iter()
            .filter(|r| {
                roster
                    .iter()
                    .any(|s| s.def_id == r.def_id && lot.classes.contains(&s.class))
            })
            .collect();
        let mut h = StateHasher::new();
        h.write_u32(stock_seed);
        h.write_bytes(lot.id.as_bytes());
        h.write_u32(day);
        let mut rng = Rng::new(h.finish() ^ STOCK_SALT);
        let mut used: BTreeSet<&str> = BTreeSet::new();
        let mut out = Vec::new();
        for slot in 0..lot.slots {
            // Every slot draws the same number of values whatever it holds, so one slot's
            // outcome never shifts another's.
            let mut slot_rng = rng.fork();
            if !slot_rng.chance(lot.fill_chance) {
                continue;
            }
            let rare = (lot.kind == LotKind::Junkyard
                && !rares.is_empty()
                && slot_rng.chance(lot.rare_chance))
            .then(|| pick(&mut slot_rng, &rares));
            let spec = match rare {
                Some(r) => roster.iter().find(|s| s.def_id == r.def_id),
                None => {
                    let free: Vec<&CarSpec> = models
                        .iter()
                        .copied()
                        .filter(|m| {
                            lot.kind != LotKind::Dealership || !used.contains(m.def_id.as_str())
                        })
                        .collect();
                    if free.is_empty() {
                        continue;
                    }
                    Some(pick(&mut slot_rng, &free))
                }
            };
            let Some(spec) = spec else { continue };
            used.insert(spec.def_id.as_str());
            let mut l = self.roll_listing(lot, slot, spec, rare, &mut slot_rng);
            // A lot never sells a car for less than the economy pays back for it straight
            // away (a rare variant's mods are refunded at resale): no buy-and-flip profit.
            // Resale grows with the price paid but by less than it (`dealer_buy_frac` < 1), so
            // this settles within a few rounds.
            for _ in 0..64 {
                let resale = self.sale_price(&Self::listing_car(&l));
                if l.price >= resale {
                    break;
                }
                l.price = (resale + 49) / 50 * 50;
            }
            out.push(l);
        }
        out
    }

    fn roll_listing(
        &self,
        lot: &LotDef,
        slot: u32,
        spec: &CarSpec,
        rare: Option<&RareVariant>,
        rng: &mut Rng,
    ) -> Listing {
        let band = |b: Option<[f32; 2]>, d: [f32; 2]| b.unwrap_or(d);
        let mut wear = ListingWear::default();
        let mut tires = TireState::default();
        let mut runs = true;
        let (grade, condition, price) = match lot.kind {
            LotKind::Dealership => {
                let p = self.market_price(spec, Condition::New) as f32 * lot.price_scale;
                (ListingGrade::New, 1.0, money(p))
            }
            LotKind::UsedLot => {
                let [lo, hi] = band(lot.condition, [0.6, 0.9]);
                let c = round2(rng.range_f32(lo, hi));
                self.roll_wear(c, false, &mut wear, rng);
                let [tlo, thi] = band(lot.tread, [0.3, 0.8]);
                for t in &mut tires.tread {
                    *t = round2(rng.range_f32(tlo, thi));
                }
                // Used price for a 90 % car; rougher cars sell for less.
                let used = self.market_price(spec, Condition::Used) as f32;
                let p = used * lot.price_scale * (0.55 + 0.5 * c);
                (ListingGrade::Used, c, money(p))
            }
            LotKind::Junkyard => {
                let [lo, hi] = band(lot.condition, [0.15, 0.5]);
                let c = round2(rng.range_f32(lo, hi));
                self.roll_wear(c, true, &mut wear, rng);
                for r in MISSING_REGIONS {
                    if rng.chance(lot.missing_part_chance) {
                        wear.missing.insert(r);
                    }
                }
                if rng.chance(lot.non_running_chance) {
                    runs = false;
                    wear.components.insert(Component::Engine, 0.0);
                }
                let [tlo, thi] = band(lot.tread, [0.0, 0.4]);
                for w in 0..4 {
                    tires.tread[w] = round2(rng.range_f32(tlo, thi));
                    tires.flat[w] = rng.chance(lot.flat_tire_chance);
                }
                let [flo, fhi] = band(lot.price_frac, [0.15, 0.3]);
                let mut frac = rng.range_f32(flo, fhi);
                if !runs {
                    frac *= 0.75;
                }
                let p = (spec.base_price as f32 * frac * lot.price_scale).max(300.0);
                // Round to a yard price (nearest 50).
                (ListingGrade::Junk, c, (money(p) + 25) / 50 * 50)
            }
        };
        Listing {
            lot: lot.id.clone(),
            slot,
            def_id: spec.def_id.clone(),
            class: spec.class,
            base_price: spec.base_price,
            variant: rare.map(|r| r.id.clone()),
            variant_name: rare.map(|r| r.name.clone()),
            // From a copy of the slot's stream, so adding paint left every other roll as it was.
            paint: FACTORY_PAINTS[(rng.clone().next_u64() % FACTORY_PAINTS.len() as u64) as usize],
            mods: rare.map(|r| r.mods.clone()).unwrap_or_default(),
            grade,
            price,
            condition,
            wear,
            tires,
            runs,
            hidden: lot.kind == LotKind::Junkyard,
        }
    }

    /// Dents and tired components for a car of condition `c` (0..=1). Used cars keep every
    /// component above 0.35 (they run and drive); junk can go lower.
    fn roll_wear(&self, c: f32, junk: bool, wear: &mut ListingWear, rng: &mut Rng) {
        let s = (1.0 - c).clamp(0.0, 1.0);
        let dents = (s * 7.0).round() as u32;
        for _ in 0..dents {
            let r = pick(rng, &DENT_REGIONS);
            let d = round2(rng.range_f32(0.1, 0.2 + 0.6 * s));
            let e = wear.dents.entry(r).or_insert(0.0);
            *e = e.max(d);
        }
        let floor = if junk { 0.05 } else { 0.35 };
        for comp in WEAR_COMPONENTS {
            if rng.chance(0.25 + 0.6 * s) {
                let lo = (1.0 - 1.3 * s).max(floor);
                let hi = (1.0 - 0.25 * s).max(lo + 0.01);
                let h = round2(rng.range_f32(lo, hi));
                let e = wear.components.entry(comp).or_insert(1.0);
                *e = e.min(h);
            }
        }
    }

    /// The car a listing becomes once bought (uid assigned by the account).
    fn listing_car(listing: &Listing) -> OwnedCar {
        let mut mods = listing.mods.clone();
        mods.sort();
        let mut damage = DamageSnapshot {
            health: listing.condition.clamp(0.0, 1.0),
            ..DamageSnapshot::pristine()
        };
        damage.handling.engine_dead = !listing.runs;
        OwnedCar {
            uid: CarUid(0),
            def_id: listing.def_id.clone(),
            class: listing.class,
            base_price: listing.base_price,
            mods,
            damage,
            location: CarLocation::Garage,
            insurance: None,
            purchase_price: Some(listing.price),
            tune: Default::default(),
            parts: None,
            tires: listing.tires,
            variant_name: listing.variant_name.clone(),
            paint: Some(listing.paint),
        }
    }

    /// Buy a listed car: pays the asking price and adds the car with the listing's mods and
    /// tyres (`purchase_price` = the asking price). The damage system applies
    /// [`Listing::wear`] to the car's lattice afterwards; until it does, the car's health is
    /// the listing's condition and a non-running car reads as having a dead engine.
    pub fn buy_listing(&self, acct: &mut PlayerAccount, listing: &Listing) -> EconResult<CarUid> {
        if matches!(
            listing.class,
            VehicleClass::PoliceCruiser | VehicleClass::PoliceInterceptor | VehicleClass::PoliceSuv
        ) {
            return Err(EconomyError::InvalidInput(
                "police vehicles are not for sale".into(),
            ));
        }
        let car = Self::listing_car(listing);
        transact(acct, |tx| {
            tx.spend(
                listing.price,
                EventReason::CarPurchased {
                    def_id: listing.def_id.clone(),
                },
                None,
            )?;
            let uid = tx.add_car(car);
            if let Some(ev) = tx.events.last_mut() {
                ev.car = Some(uid);
            }
            Ok(uid)
        })
    }

    /// Price of replacing the tyres in `wheels` (FL, FR, RL, RR) on `car`.
    pub fn tire_quote(&self, car: &OwnedCar, wheels: [bool; 4]) -> Money {
        let each = self
            .lots
            .tires
            .price_per_tire
            .get(&car.class)
            .copied()
            .unwrap_or(0);
        each * wheels.iter().filter(|w| **w).count() as Money
    }

    /// Replace the tyres in `wheels` with new ones (the car must be in the garage). Tyres are
    /// the one thing a repair never fixes. Returns what was paid.
    pub fn replace_tires(
        &self,
        acct: &mut PlayerAccount,
        car: CarUid,
        wheels: [bool; 4],
    ) -> EconResult<Money> {
        let c = acct.try_car(car)?;
        if c.location != CarLocation::Garage {
            return Err(EconomyError::CarUnavailable {
                car,
                reason: "in the pound; retrieve it first".into(),
            });
        }
        let count = wheels.iter().filter(|w| **w).count();
        if count == 0 {
            return Err(EconomyError::InvalidInput("no tyres picked".into()));
        }
        let price = self.tire_quote(c, wheels);
        transact(acct, |tx| {
            // No contract reason for tyres yet (request in docs/integration/economy.md).
            tx.spend(
                price,
                EventReason::Adjustment {
                    note: format!("{TIRES_NOTE}{count}"),
                },
                Some(car),
            )?;
            let c = tx.car_mut(car)?;
            for (w, on) in wheels.iter().enumerate() {
                if *on {
                    c.tires.tread[w] = 1.0;
                    c.tires.flat[w] = false;
                }
            }
            Ok(price)
        })
    }

    /// Wear a car's tyres by `hours` of driving and flatten the ones in `flat` (spike strips).
    /// Not a ledger event (nothing is paid).
    pub fn wear_tires(&self, car: &mut OwnedCar, hours: f32, flat: [bool; 4]) {
        let wear = self.lots.tires.wear_per_hour * hours.max(0.0);
        for (w, spiked) in flat.iter().enumerate() {
            car.tires.tread[w] = (car.tires.tread[w] - wear).clamp(0.0, 1.0);
            car.tires.flat[w] |= *spiked;
        }
    }

    /// Grip multiplier per wheel from tread (`bald_grip` at 0, 1.0 when new) and the
    /// deflation per wheel (1 = flat), for the vehicle sim.
    pub fn tire_effects(&self, tires: &TireState) -> ([f32; 4], [f32; 4]) {
        let bald = self.lots.tires.bald_grip;
        let grip = tires.tread.map(|t| bald + (1.0 - bald) * t.clamp(0.0, 1.0));
        let flat = tires.flat.map(|f| if f { 1.0 } else { 0.0 });
        (grip, flat)
    }

    pub(crate) fn validate_lots(&self) -> Result<(), crate::DataError> {
        let invalid = |what: String, reason: String| crate::DataError::Invalid { what, reason };
        let t = &self.lots.tires;
        if !(0.0..=1.0).contains(&t.bald_grip) || !(0.0..=1.0).contains(&t.bald_below) {
            return Err(invalid(
                "lots.tires".into(),
                "bald_grip / bald_below must be within 0..=1".into(),
            ));
        }
        for class in [
            VehicleClass::Beater,
            VehicleClass::Compact,
            VehicleClass::Sport,
            VehicleClass::Muscle,
            VehicleClass::Exotic,
            VehicleClass::Truck,
        ] {
            if !t.price_per_tire.contains_key(&class) {
                return Err(invalid(
                    "lots.tires.price_per_tire".into(),
                    format!("missing {class:?}"),
                ));
            }
        }
        let mut ids = BTreeSet::new();
        for l in &self.lots.lots {
            let what = format!("lots[{}]", l.id);
            if !ids.insert(l.id.as_str()) {
                return Err(invalid(what, "duplicate id".into()));
            }
            if l.classes.is_empty() || l.slots == 0 || l.min_base_price > l.max_base_price {
                return Err(invalid(
                    what,
                    "needs classes, slots > 0 and min_base_price <= max_base_price".into(),
                ));
            }
            for p in [
                l.fill_chance,
                l.non_running_chance,
                l.missing_part_chance,
                l.flat_tire_chance,
                l.rare_chance,
            ] {
                if !(0.0..=1.0).contains(&p) {
                    return Err(invalid(what, format!("chance {p} must be within 0..=1")));
                }
            }
            for [lo, hi] in [l.condition, l.tread].into_iter().flatten() {
                if !(0.0 <= lo && lo <= hi && hi <= 1.0) {
                    return Err(invalid(
                        what,
                        format!("band [{lo}, {hi}] must be within 0..=1"),
                    ));
                }
            }
            if !(l.price_scale.is_finite() && l.price_scale > 0.0) {
                return Err(invalid(what, "price_scale must be > 0".into()));
            }
        }
        for r in &self.lots.rare_variants {
            let what = format!("rare_variants[{}]", r.id);
            if !ids.insert(r.id.as_str()) {
                return Err(invalid(what, "duplicate id".into()));
            }
            for m in &r.mods {
                if self.catalog.get(m).is_none() {
                    return Err(invalid(what, format!("unknown mod {m:?}")));
                }
            }
        }
        Ok(())
    }
}

/// Ledger note prefix of a tyre replacement (`EventReason::Adjustment`), followed by the
/// number of tyres.
pub const TIRES_NOTE: &str = "tires replaced: ";

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixed seed of careers built without a host seed.
    const S: u32 = crate::DEFAULT_STOCK_SEED;

    fn roster() -> Vec<CarSpec> {
        [
            ("junkyard_beater", VehicleClass::Beater, 3000),
            ("tansy_13", VehicleClass::Compact, 7000),
            ("vireo_xs", VehicleClass::Compact, 7500),
            ("kestrel_ts", VehicleClass::Sport, 22000),
            ("shiro_turbo", VehicleClass::Sport, 28000),
            ("ironside_440", VehicleClass::Muscle, 38000),
            ("rampart_50", VehicleClass::Muscle, 42000),
            ("hakone_gt", VehicleClass::Sport, 48000),
            ("fulmine_v10", VehicleClass::Exotic, 125000),
            ("furrow_24", VehicleClass::Truck, 21000),
        ]
        .into_iter()
        .map(|(id, class, p)| CarSpec {
            def_id: id.into(),
            class,
            base_price: p,
        })
        .collect()
    }

    #[test]
    fn stock_is_deterministic_and_changes_by_day() {
        let e = Economy::embedded();
        let r = roster();
        // Same career, same day: the same cars (reopening a lot never rerolls).
        for seed in [S, 0xDEAD_BEEF] {
            for lot in &e.lots.lots {
                assert_eq!(
                    e.lot_stock(seed, &lot.id, 3, &r),
                    e.lot_stock(seed, &lot.id, 3, &r)
                );
            }
        }
        // Same career, next day: a restock.
        for seed in [S, 0xDEAD_BEEF] {
            for lot in &e.lots.lots {
                let changed = (0..10).any(|d| {
                    e.lot_stock(seed, &lot.id, d, &r) != e.lot_stock(seed, &lot.id, d + 1, &r)
                });
                assert!(changed, "{} never restocks", lot.id);
            }
            let days: Vec<Vec<String>> = (0..10)
                .map(|d| {
                    e.lot_stock(seed, "harbourline_auto", d, &r)
                        .into_iter()
                        .map(|l| l.def_id)
                        .collect()
                })
                .collect();
            assert!(
                days.iter().any(|d| *d != days[0]),
                "stock never changes: {days:?}"
            );
        }
    }

    #[test]
    fn careers_with_different_seeds_see_different_stock() {
        let e = Economy::embedded();
        let r = roster();
        let seeds = [S, 1, 2, 0x1234_5678, 0xDEAD_BEEF, u32::MAX];
        let mut pairs = 0;
        let mut differ = 0;
        for day in 0..5 {
            for lot in &e.lots.lots {
                for (i, a) in seeds.iter().enumerate() {
                    for b in &seeds[i + 1..] {
                        pairs += 1;
                        if e.lot_stock(*a, &lot.id, day, &r) != e.lot_stock(*b, &lot.id, day, &r) {
                            differ += 1;
                        }
                    }
                }
            }
        }
        // Wear, prices and paint are rolled too, so nearly every pair differs; ask for most.
        assert!(
            differ * 10 >= pairs * 8,
            "only {differ} of {pairs} (career, career, lot, day) pairs differ"
        );
    }

    #[test]
    fn no_lot_car_resells_for_more_than_it_costs() {
        let e = Economy::embedded();
        let r = roster();
        for seed in [S, 7, 0xDEAD_BEEF, 0x1234_5678] {
            for d in 0..200 {
                for lot in &e.lots.lots {
                    for l in e.lot_stock(seed, &lot.id, d, &r) {
                        let resale = e.sale_price(&Economy::listing_car(&l));
                        assert!(resale <= l.price, "{l:?} resells for {resale}");
                    }
                }
            }
        }
    }

    #[test]
    fn every_career_keeps_the_stock_guarantees() {
        let e = Economy::embedded();
        let r = roster();
        for seed in [S, 7, 0xDEAD_BEEF] {
            for d in 0..30 {
                for lot in &e.lots.lots {
                    let s = e.lot_stock(seed, &lot.id, d, &r);
                    assert!(s.len() <= lot.slots as usize);
                    let slots: BTreeSet<_> = s.iter().map(|l| l.slot).collect();
                    assert_eq!(slots.len(), s.len(), "a slot holds one car");
                    if lot.kind == LotKind::Dealership {
                        let ids: BTreeSet<_> = s.iter().map(|l| &l.def_id).collect();
                        assert_eq!(ids.len(), s.len(), "{} doubled a model", lot.id);
                    }
                }
            }
        }
    }

    #[test]
    fn not_every_model_is_on_the_floor_every_day() {
        let e = Economy::embedded();
        let r = roster();
        let carried = r
            .iter()
            .filter(|s| {
                let l = e.lot("budget_motors").unwrap();
                l.classes.contains(&s.class) && s.base_price <= l.max_base_price
            })
            .count();
        let short = (0..20)
            .filter(|d| e.lot_stock(S, "budget_motors", *d, &r).len() < carried)
            .count();
        assert!(short > 0);
    }

    #[test]
    fn grades_follow_the_lot_kind() {
        let e = Economy::embedded();
        let r = roster();
        let mut saw_dead = false;
        let mut saw_rare = false;
        for d in 0..60 {
            for lot in &e.lots.lots {
                for l in e.lot_stock(S, &lot.id, d, &r) {
                    assert!(lot.classes.contains(&l.class));
                    match lot.kind {
                        LotKind::Dealership => {
                            assert_eq!(l.grade, ListingGrade::New);
                            assert!(l.wear.components.is_empty() && l.wear.dents.is_empty());
                            assert_eq!(l.tires, TireState::default());
                            assert!(l.runs && !l.hidden);
                        }
                        LotKind::UsedLot => {
                            assert_eq!(l.grade, ListingGrade::Used);
                            assert!(l.runs);
                            assert!(l.wear.missing.is_empty());
                            assert!(l.wear.components.values().all(|h| *h >= 0.35));
                            let spec = r.iter().find(|s| s.def_id == l.def_id).unwrap();
                            assert!(l.price <= e.market_price(spec, Condition::Used) * 11 / 10);
                        }
                        LotKind::Junkyard => {
                            assert_eq!(l.grade, ListingGrade::Junk);
                            assert!(l.hidden);
                            assert!(l.price < l.base_price as Money / 2);
                            saw_dead |= !l.runs;
                            saw_rare |= l.variant.is_some();
                        }
                    }
                }
            }
        }
        assert!(saw_dead && saw_rare);
    }

    #[test]
    fn dealerships_never_double_a_model() {
        let e = Economy::embedded();
        let r = roster();
        for d in 0..30 {
            let s = e.lot_stock(S, "harbourline_auto", d, &r);
            let ids: BTreeSet<_> = s.iter().map(|l| &l.def_id).collect();
            assert_eq!(ids.len(), s.len());
        }
    }

    #[test]
    fn buying_a_listing_pays_and_keeps_tyres_and_mods() {
        let e = Economy::embedded();
        let r = roster();
        let l = (0..1000)
            .flat_map(|d| e.lot_stock(S, "dockside_salvage", d, &r))
            .find(|l| l.variant.is_some() && !l.mods.is_empty())
            .expect("a rare variant with mods turns up within 1000 days");
        let (mut a, _) = e.open_account(wp_contracts::PlayerId(1));
        a.cash = 1_000_000;
        let (uid, ev) = e.buy_listing(&mut a, &l).unwrap();
        assert_eq!(ev.len(), 1);
        assert_eq!(a.cash, 1_000_000 - l.price);
        let car = a.car(uid).unwrap();
        assert_eq!(car.tires, l.tires);
        assert!(l.mods.iter().all(|m| car.mods.contains(m)));
        assert_eq!(car.purchase_price, Some(l.price));
        assert_eq!(car.damage.handling.engine_dead, !l.runs);
        // Resale never beats the yard price.
        assert!(e.sale_price(car) <= l.price);
    }

    #[test]
    fn tyres_are_bought_separately() {
        let e = Economy::embedded();
        let (mut a, _) = e.open_account(wp_contracts::PlayerId(1));
        let spec = &roster()[1];
        let (uid, _) = e.buy_car(&mut a, spec, Condition::Worn).unwrap();
        a.car_mut(uid).unwrap().tires = TireState {
            tread: [0.1, 0.1, 0.8, 0.8],
            flat: [true, false, false, false],
        };
        let cash = a.cash;
        let wheels = [true, true, false, false];
        let price = e.tire_quote(a.car(uid).unwrap(), wheels);
        let (paid, _) = e.replace_tires(&mut a, uid, wheels).unwrap();
        assert_eq!(paid, price);
        assert_eq!(a.cash, cash - price);
        let t = a.car(uid).unwrap().tires;
        assert_eq!(t.tread, [1.0, 1.0, 0.8, 0.8]);
        assert_eq!(t.flat, [false; 4]);
        let (grip, _) = e.tire_effects(&TireState {
            tread: [0.0, 1.0, 0.5, 1.0],
            flat: [false; 4],
        });
        assert!(grip[0] < grip[2] && grip[2] < grip[1]);
    }
}
