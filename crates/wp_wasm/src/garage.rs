//! Live repair bill for the testbed's garage overlay (workstream 16, UI kit).
//!
//! Prices the damage of a *running* vehicle exactly as the game will: `wp_damage`'s
//! `DamageSim::repair_estimate` with the economy's `PriceTable` for the car, then
//! `wp_economy::Economy::quote_repair` for shop, district, mode, wanted-level markup, insurance
//! and salvage. Read-only: nothing here changes the simulation or any account.

use std::cell::RefCell;

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wp_contracts::{DamageSim, PlayerId, RepairEstimate, VehicleClass, WantedLevel};
use wp_economy::{
    CarLocation, CarUid, DamageSnapshot, Economy, OwnedCar, PlayerAccount, RepairOption,
    RepairQuote, ShopContext, ShopKind,
};

use crate::{err, with};

thread_local! {
    /// Parsed once: the embedded economy tables (`data/economy`, `data/mods`, `data/modes`).
    static ECONOMY: RefCell<Option<Economy>> = const { RefCell::new(None) };
}

/// Run `f` with the embedded economy tables (parsed once per thread).
pub(crate) fn with_economy<T>(f: impl FnOnce(&Economy) -> T) -> T {
    ECONOMY.with(|e| {
        let mut e = e.borrow_mut();
        f(e.get_or_insert_with(Economy::embedded))
    })
}

fn d_district() -> String {
    "default".to_string()
}

/// What the overlay asks for (JSON). Every field is optional.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QuoteRequest {
    /// Shop kind (default legit).
    #[serde(default = "d_shop")]
    shop: ShopKind,
    /// District for the labour multiplier: a `DistrictId` name (`prices.toml`
    /// `[district_labour]`) or `"default"` (= `Residential`).
    #[serde(default = "d_district")]
    district: String,
    /// Wanted level of the car (crooked-shop markup, legit-shop reports).
    #[serde(default)]
    wanted: WantedLevel,
    /// Repair option (default `RepairAll`).
    #[serde(default = "d_option")]
    option: RepairOption,
    /// Insurance tier id (`basic` / `full`), if the car is insured.
    #[serde(default)]
    insurance: Option<String>,
}

fn d_shop() -> ShopKind {
    ShopKind::Legit
}
fn d_option() -> RepairOption {
    RepairOption::RepairAll
}

/// The bill for one live vehicle.
#[derive(Debug, Serialize)]
struct LiveBill {
    entity: u32,
    car_id: String,
    display_name: String,
    class: VehicleClass,
    base_price: u32,
    /// Global health 0..=1 from the damage model.
    health: f32,
    total_loss: bool,
    /// Damage mode the bill is priced in.
    mode: String,
    /// Unscaled estimate from `wp_damage` (contract `RepairEstimate`).
    estimate: RepairEstimate,
    /// Priced quote from `wp_economy`, or `None` when there is nothing to quote.
    quote: Option<RepairQuote>,
    /// Why there is no quote (e.g. "nothing to repair").
    error: Option<String>,
}

/// Itemised repair bill for a live vehicle as JSON (`LiveBill`). `request_json` may be `""` or a
/// JSON object `{shop?: "Legit"|"Crooked", district?, wanted?: "None"|"L1"..,
/// option?: "RepairAll"|"DriveDamaged"|"Scrap"|{"RepairSelected":[items]}, insurance?: "basic"|"full"}`.
#[wasm_bindgen]
pub fn repair_quote_json(entity: u32, request_json: &str) -> Result<String, JsValue> {
    let req: QuoteRequest = if request_json.trim().is_empty() {
        serde_json::from_str("{}").map_err(err)?
    } else {
        serde_json::from_str(request_json).map_err(err)?
    };
    with(|s| {
        let sim = s.sim();
        let v = sim
            .vehicles()
            .iter()
            .find(|v| v.id.0 == entity)
            .ok_or_else(|| format!("unknown entity {entity}"))?;
        let def = &s
            .car(v.id)
            .ok_or_else(|| format!("no car data for entity {entity}"))?
            .def;
        let mode = s.mode();
        let bill = with_economy(|econ| {
            let price_table = econ.price_table(def.class, def.base_price as i64);
            let estimate = v.damage.repair_estimate(sim.mode_table(), &price_table);
            let car = OwnedCar {
                uid: CarUid(1),
                def_id: def.id.clone(),
                class: def.class,
                base_price: def.base_price,
                mods: Vec::new(),
                damage: DamageSnapshot {
                    health: v.damage.health(),
                    total_loss: v.damage.is_total_loss(),
                    handling: v.damage.handling_effects(),
                    outstanding: estimate.clone(),
                    lattice_state: Vec::new(),
                    sustained_before_capture: false,
                },
                location: CarLocation::Garage,
                insurance: None,
                purchase_price: None,
                tune: Default::default(),
                parts: None,
            };
            // A throwaway account holding just this car (cash only pays an optional premium).
            let mut acct = PlayerAccount::empty(PlayerId(1));
            acct.cash = 1_000_000;
            acct.garage.push(car);
            if let Some(tier) = &req.insurance {
                econ.buy_insurance(&mut acct, CarUid(1), tier)
                    .map_err(|e| e.to_string())?;
            }
            let shop = ShopContext {
                district: wp_economy::district_from_key(&req.district)
                    .ok_or_else(|| format!("unknown district {:?}", req.district))?,
                kind: req.shop,
                wanted: req.wanted,
            };
            let (quote, error) = match econ.quote_repair(&acct, CarUid(1), &req.option, &shop, mode)
            {
                Ok(q) => (Some(q), None),
                Err(e) => (None, Some(e.to_string())),
            };
            Ok::<_, String>(LiveBill {
                entity,
                car_id: def.id.clone(),
                display_name: def.display_name.clone(),
                class: def.class,
                base_price: def.base_price,
                health: v.damage.health(),
                total_loss: v.damage.is_total_loss(),
                mode: format!("{mode:?}"),
                estimate,
                quote,
                error,
            })
        })?;
        Ok(serde_json::to_string(&bill).expect("bill serializes"))
    })
}
