//! Single-player career in the browser (machine G, workstream 08): a persistent account,
//! garage and ledger that carry over between runs.
//!
//! [`Career`] owns a [`CareerProfile`] (`wp_economy::profile`): cash, ledger, owned cars with
//! their mods, insurance, impound status and damage, carried heat and stats. Each owned car's
//! full lattice damage is kept as `wp_damage::FullDamageSnapshot` JSON in
//! `CareerProfile::lattices`, so a dented car comes back dented.
//!
//! **A run** ([`Career::start_run`]) takes any live scenario (free drive, night run, a race),
//! puts the career's active car in the first entity slot with the career's damage mode and
//! carried heat, builds the car from its installed mods (the modified `VehicleDef` /
//! `LatticeDef` from `Economy::preview`, overlaid on the embedded data under the same car
//! id), restores its saved lattice into the sim and hands the account to the session's
//! [`PlayerRun`] (end-of-run settlement, "Repair & continue"). While the run is driven the
//! session holds the account; [`Career::finish_run`] takes it back with the car's damage,
//! applies the carry-over rules and advances one in-game night.
//!
//! **The garage** (between runs): repair (all or per item, at a district shop, legit or
//! crooked; the stored lattice is repaired with `DamageSim::apply_repair`), buy / sell cars
//! (the street roster, class bands), install / remove mods (with stat deltas), the impound lot
//! (release, or let the auction countdown run with [`Career::next_night`]) and insurance.
//!
//! Persistence is the testbed's job: [`Career::save_json`] / [`Career::load_json`] (versioned
//! by `wp_economy::profile`), stored in `localStorage` and exported / imported as a file.
//!
//! **The tune** ([`Career::set_tune`]): each car's free four-slider tune is stored on the car
//! and goes into the first entity's `tune` when a run starts, so the sim builds it at spawn.
//!
//! Known gap: a replay of a career run whose car has mods or started damaged does not verify
//! (the replay rebuilds the stock car, pristine). Plain careers (stock, undamaged) replay fine.

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wp_contracts::{
    DamageMode, DamageSim, DistrictId, EntityId, LoadedCar, RepairItem, VehicleClass, VehicleDef,
    WantedLevel,
};
use wp_damage::{DamageModel, FullDamageSnapshot, PreownedWear};
use wp_economy::{
    CarLocation, CarSpec, CarUid, CareerProfile, CareerProgress, Condition, DamageReport, Economy,
    EconomyError, EconomyEvent, EventReason, ImpoundStatus, Listing, ListingGrade, LotKind, Money,
    OwnedCar, ProgressUpdate, RepTier, RepairOption, RepairQuote, RunEnd, ShopContext, ShopKind,
    ToolLoadout,
};

use crate::data::mem_data;
use crate::run::{LedgerLine, PlayerRun, RunOutcome, PLAYER_ACCOUNT};
use crate::session::Session;

/// RNG salt of a career run's economy (capture consequences), mixed with the in-game day so
/// each night rolls differently.
const CAREER_SEED_SALT: u64 = 0xCA4E_E400;

/// Extra RNG salt for settling a run abandoned mid-pursuit (`Career::load_json`).
const ABANDONED_SEED_SALT: u64 = 0xAB_A4D0;

/// Spike-strip deflation at the end of a run at which a tyre stays flat.
const TIRE_FLAT_PUNCTURE: f32 = 0.5;

/// Ledger lines in the garage view.
const VIEW_LEDGER_LINES: usize = 25;

thread_local! {
    /// Parsed roster cars by id (stock definitions; the dealer and the garage read them often).
    static BASE_CARS: RefCell<BTreeMap<String, LoadedCar>> = const { RefCell::new(BTreeMap::new()) };
}

/// A stock roster car (parsed once).
fn base_car(id: &str) -> Result<LoadedCar, String> {
    if let Some(c) = BASE_CARS.with(|m| m.borrow().get(id).cloned()) {
        return Ok(c);
    }
    let c = crate::data::load_car(id)?;
    BASE_CARS.with(|m| m.borrow_mut().insert(id.to_string(), c.clone()));
    Ok(c)
}

// ----- data overlay ------------------------------------------------------------------------

/// The embedded data with some files replaced (the career car built with its mods).
pub struct Overlay<'a> {
    /// Replacement files by relative path (`cars/<id>.json`, `lattices/<id>.json`).
    pub files: &'a BTreeMap<String, String>,
}

impl simcli::data::DataSource for Overlay<'_> {
    fn read(&self, rel: &str) -> Result<String, simcli::RunError> {
        match self.files.get(rel) {
            Some(text) => Ok(text.clone()),
            None => mem_data().read(rel),
        }
    }
}

// ----- views (JSON for the garage screen) ----------------------------------------------------

/// Performance numbers of a build.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatsView {
    /// Performance index.
    pub class_index: u16,
    /// Lobby bracket letter (`D` … `X`).
    pub bracket: String,
    /// Mass, kg.
    pub mass_kg: f32,
    /// Estimated 0–100 km/h, s.
    pub est_0_100_s: f32,
    /// Estimated top speed, km/h.
    pub top_speed_kmh: f32,
    /// Estimated lateral grip, g.
    pub lateral_g: f32,
    /// Estimated braking, g.
    pub brake_g: f32,
}

/// One outstanding repair line.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RepairLineView {
    /// The item (serde form, sent back in `RepairSelected`).
    pub item: RepairItem,
    /// Short label (`Front bumper`, `Radiator`, `Detached: hood`).
    pub label: String,
    /// Severity 0..=1.
    pub severity: f32,
    /// Price at the default shop (legit, residential labour), mode-scaled.
    pub price: Money,
}

/// An installed mod.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstalledModView {
    /// Mod id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Slot (`Engine`, `Protection`, …).
    pub slot: String,
}

/// One owned car.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CarView {
    /// Car uid.
    pub uid: u32,
    /// Car id (`data/cars`).
    pub def_id: String,
    /// Display name (the variant's name for a rare variant).
    pub name: String,
    /// A rare variant (bought as one from a lot).
    pub rare: bool,
    /// Body colour 0xRRGGBB: the paint it was sold or resprayed in, else a fixed factory colour
    /// picked by uid ([`car_paint`]), so the car looks the same in every view and session.
    pub paint: u32,
    /// What a respray at the paint shop costs.
    pub respray_fee: Money,
    /// Class.
    pub class: VehicleClass,
    /// The career's active car.
    pub active: bool,
    /// `Garage` or `Impound`.
    pub location: &'static str,
    /// Health 0..=1.
    pub health: f32,
    /// Total loss (repair or scrap it).
    pub total_loss: bool,
    /// Can be driven now (in the garage, not wrecked).
    pub drivable: bool,
    /// Current value (pristine value minus outstanding repairs).
    pub value: Money,
    /// What the dealer pays for it now (never more than was paid for it).
    pub sale_price: Money,
    /// What the car body cost (`Economy::resale_basis`: price paid, mods excluded), shown next
    /// to the sale price.
    pub paid: Money,
    /// Salvage if scrapped.
    pub scrap_value: Money,
    /// Installed mods.
    pub mods: Vec<InstalledModView>,
    /// The free tune (four 1..=5 sliders, 3 = stock), applied at the next run's spawn.
    pub tune: wp_economy::CarTune,
    /// Insurance tier, if insured.
    pub insurance: Option<String>,
    /// Daily premium, if insured.
    pub premium_per_day: Option<Money>,
    /// `RepairAll` at the default shop (`None` = nothing to repair, or impounded).
    pub repair_all: Option<RepairQuote>,
    /// Outstanding repair lines.
    pub repair_lines: Vec<RepairLineView>,
    /// Pound status (impounded cars).
    pub impound: Option<ImpoundStatus>,
    /// Performance of the build as it is.
    pub stats: Option<StatsView>,
    /// Some outstanding damage dates from a police capture (insurance will not pay for it).
    pub damage_before_capture: bool,
    /// Tyres (FL, FR, RL, RR). Repairs never fix them: replace them on their own.
    pub tires: TireView,
}

/// A car's tyres in the garage.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TireView {
    /// Tread 0 (bald) ..= 1 (new), FL, FR, RL, RR.
    pub tread: [f32; 4],
    /// Flat.
    pub flat: [bool; 4],
    /// Needs replacing (flat or bald).
    pub worn_out: [bool; 4],
    /// Grip relative to new tyres.
    pub grip: [f32; 4],
    /// Price of one fitted tyre for this car.
    pub price_each: Money,
}

/// A lot on the lots list (the garage's "Car lots" tab).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LotSummary {
    /// Lot id.
    pub id: String,
    /// Name.
    pub name: String,
    /// `Dealership`, `UsedLot` or `Junkyard`.
    pub kind: LotKind,
    /// Tier 1 ..= 3.
    pub tier: u8,
    /// District name.
    pub district: String,
    /// Classes it carries.
    pub classes: Vec<VehicleClass>,
    /// Cars on the lot today (not yet sold).
    pub in_stock: usize,
    /// Cheapest car on the lot today.
    pub from_price: Option<Money>,
}

/// One car on a lot today. Junkyard cars hide what cannot be seen (`condition`, `runs`,
/// `tired_parts` are `None` until bought).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ListingView {
    /// Parking space (what `career_buy_listing` names).
    pub slot: u32,
    /// Car id.
    pub def_id: String,
    /// Display name (the variant's name for a rare variant).
    pub name: String,
    /// Class.
    pub class: VehicleClass,
    /// New / used / junk.
    pub grade: ListingGrade,
    /// Asking price.
    pub price: Money,
    /// A rare variant.
    pub rare: bool,
    /// Mods it comes with (names).
    pub mods: Vec<String>,
    /// Condition 0..=1: the car's health as the damage model has it (hidden at a junkyard).
    pub condition: Option<f32>,
    /// What repairing everything would cost at a legit residential shop, tyres excluded
    /// (hidden at a junkyard).
    pub est_repairs: Option<Money>,
    /// Runs (hidden at a junkyard).
    pub runs: Option<bool>,
    /// Worn components with their health (hidden at a junkyard).
    pub tired_parts: Option<Vec<(String, f32)>>,
    /// Missing panels (visible).
    pub missing: Vec<String>,
    /// Dented panels (visible).
    pub dents: Vec<String>,
    /// Tyre tread (visible).
    pub tread: [f32; 4],
    /// Flat tyres (visible).
    pub flat: [bool; 4],
    /// Factory paint 0xRRGGBB (drawn on the lot; the car keeps it once bought).
    pub paint: u32,
    /// Performance as built (with the variant's mods).
    pub stats: Option<StatsView>,
    /// Sold today.
    pub sold: bool,
    /// Rep tier needed to buy a car of this class (`None`: never for sale).
    pub unlock_tier: Option<RepTier>,
    /// The player's tier is below `unlock_tier`: it can be looked at, not bought.
    pub locked: bool,
}

/// One lot's stock today (`career_lot_json`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LotStockView {
    /// The lot.
    pub lot: LotSummary,
    /// In-game day the stock is for (restocks every day).
    pub day: u32,
    /// Cars by slot, sold ones included (`sold`).
    pub cars: Vec<ListingView>,
}

/// A car the dealer sells.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DealerCar {
    /// Car id.
    pub def_id: String,
    /// Display name.
    pub name: String,
    /// Class.
    pub class: VehicleClass,
    /// Base price.
    pub base_price: u32,
    /// Price new / used / worn.
    pub price_new: Money,
    /// Used.
    pub price_used: Money,
    /// Worn.
    pub price_worn: Money,
    /// Stock performance.
    pub stats: StatsView,
    /// Rep tier at which the dealer sells this class (career progression).
    pub unlock_tier: RepTier,
    /// True while the player's tier is below `unlock_tier` (refreshed by every view).
    pub locked: bool,
}

/// An insurance tier on offer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InsuranceView {
    /// Tier id (`basic`, `full`).
    pub id: String,
    /// Daily premium as a fraction of the car's value.
    pub premium_per_day_frac: f32,
    /// Share of the bill covered after the deductible.
    pub coverage: f32,
    /// Deductible per claim.
    pub deductible: Money,
}

/// A district's labour multiplier.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DistrictView {
    /// `DistrictId` name.
    pub id: String,
    /// Labour × at a legit shop.
    pub labour: f32,
}

/// A repair shop of the world as the Repair panel offers it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShopView {
    /// Shop name.
    pub name: String,
    /// `DistrictId` name of the shop's district.
    pub district: String,
    /// Legit or crooked.
    pub kind: ShopKind,
    /// Spawn city id the shop is in ([`crate::scenario::SPAWNS`]).
    pub city: String,
    /// That city's name (the spawn's label).
    pub city_label: String,
    /// The district's labour × (before any crooked-shop markup).
    pub labour: f32,
}

/// The whole garage screen.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CareerView {
    /// Profile version.
    pub version: u32,
    /// Career damage mode.
    pub mode: DamageMode,
    /// In-game day (nights driven + nights skipped).
    pub day: u32,
    /// Cash (negative = debt).
    pub cash: Money,
    /// Debt.
    pub debt: Money,
    /// Rep.
    pub rep: i64,
    /// Bounty on the player's head.
    pub bounty: u32,
    /// Net worth (cash + cars).
    pub net_worth: Money,
    /// Heat the next run starts with.
    pub carried_heat: f32,
    /// Wanted level the next run starts at.
    pub carried_level: WantedLevel,
    /// A career run is being driven (the garage is read-only until it ends).
    pub run_active: bool,
    /// Active car uid.
    pub active_car: Option<u32>,
    /// The active car can start a run.
    pub can_start: bool,
    /// Why not, when it cannot.
    pub start_blocker: Option<String>,
    /// Owned cars.
    pub cars: Vec<CarView>,
    /// The dealer (every roster model; the browser now buys from the lots instead).
    pub dealer: Vec<DealerCar>,
    /// Dealerships, used lots and junkyards with today's stock counts.
    pub lots: Vec<LotSummary>,
    /// Economy floor: the beater and its price, offered when no car is usable.
    pub floor_car: Option<(String, Money)>,
    /// Insurance tiers.
    pub insurance: Vec<InsuranceView>,
    /// Districts and labour multipliers.
    pub districts: Vec<DistrictView>,
    /// The world's repair shops ([`crate::garage_visit::WORLD_SHOPS`]), in POI order.
    pub shops: Vec<ShopView>,
    /// Career counters.
    pub stats: wp_economy::CareerStats,
    /// Rep tier, next tier, unlocked features and classes, bet limits.
    pub progress: CareerProgress,
    /// Police history.
    pub captures: u32,
    /// Escapes.
    pub escapes: u32,
    /// Newest ledger lines, newest last.
    pub ledger: Vec<LedgerLine>,
}

/// One mod in the tree for a car.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModOption {
    /// Mod id.
    pub id: String,
    /// Name.
    pub name: String,
    /// Slot.
    pub slot: String,
    /// Upgrade line.
    pub line: String,
    /// Tier 1..=5.
    pub tier: u8,
    /// Price.
    pub price: u32,
    /// What you give up.
    pub tradeoff: String,
    /// `performance`, `durability`, `cosmetic` or `tool`.
    pub group: &'static str,
    /// Installed on this car.
    pub installed: bool,
    /// Can be installed now (or, if installed, removed).
    pub allowed: bool,
    /// Why not.
    pub reason: Option<String>,
    /// Same-line mod it replaces.
    pub replaces: Option<String>,
    /// Stats with the change applied (install, or remove when installed).
    pub stats_after: Option<StatsView>,
    /// Required mods.
    pub requires: Vec<String>,
}

/// The mod tree for one car.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModTreeView {
    /// Car uid.
    pub uid: u32,
    /// Stats as built.
    pub stats: StatsView,
    /// Every mod the class can take in this mode, by slot then line then tier.
    pub mods: Vec<ModOption>,
}

/// What a garage shop request asks for (JSON). Every field is optional.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShopRequest {
    /// `RepairAll` (default), `{"RepairSelected": [items]}`, `Scrap`.
    #[serde(default = "d_option")]
    pub option: RepairOption,
    /// `DistrictId` name (default `Residential`).
    #[serde(default)]
    pub district: Option<String>,
    /// `Legit` (default) or `Crooked`.
    #[serde(default = "d_shop")]
    pub shop: ShopKind,
}

impl ShopRequest {
    /// Parse a request (`""` = repair all at a legit residential shop).
    pub fn parse(request_json: &str) -> Result<Self, String> {
        let text = if request_json.trim().is_empty() {
            "{}"
        } else {
            request_json
        };
        serde_json::from_str(text).map_err(|e| e.to_string())
    }
}

fn d_option() -> RepairOption {
    RepairOption::RepairAll
}
fn d_shop() -> ShopKind {
    ShopKind::Legit
}

/// How a career run ended (`career_end_run`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunClose {
    /// Free / captured / wrecked.
    pub end: RunEnd,
    /// Heat the next run starts with.
    pub carried_heat: f32,
    /// Cash at the end of the night.
    pub cash: Money,
    /// Ledger lines of the run and the night.
    pub ledger: Vec<LedgerLine>,
    /// Tier changes from the run (promotion, rewards paid, features opened or closed).
    pub progress: ProgressUpdate,
}

// ----- the career ------------------------------------------------------------------------------

/// A career run being driven.
#[derive(Debug, Clone, Copy)]
struct ActiveRun {
    car: CarUid,
    /// First ledger seq of the run (lines from the session's ledger at or after it are new).
    start_seq: u64,
}

/// One single-player career (see the module docs).
pub struct Career {
    econ: Economy,
    profile: CareerProfile,
    run: Option<ActiveRun>,
    dealer: Option<Vec<DealerCar>>,
    /// A run the game was closed in during a pursuit, settled as a bust when the save was
    /// loaded (B05); taken once by the UI ([`Career::take_abandoned`]).
    abandoned: Option<RunClose>,
}

fn e2s(e: EconomyError) -> String {
    e.to_string()
}

/// The economy's stored tune as the sim's (same fields; one type once the contract carries
/// `CarTune`, request WP-4).
fn vehicle_tune(t: wp_economy::CarTune) -> wp_vehicle::CarTune {
    wp_vehicle::CarTune {
        suspension: t.suspension,
        gearing: t.gearing,
        diff: t.diff,
        brake_balance: t.brake_balance,
    }
}

fn stats_of(p: &wp_economy::Preview) -> StatsView {
    StatsView {
        class_index: p.class_index,
        bracket: p.bracket.clone(),
        mass_kg: p.mass_kg,
        est_0_100_s: p.est_0_100_s,
        top_speed_kmh: p.est_top_speed_mps * 3.6,
        lateral_g: p.metrics.lateral_grip_g,
        brake_g: p.metrics.brake_decel_g,
    }
}

fn label_of(item: &RepairItem) -> String {
    let words = |s: String| {
        let mut out = String::new();
        for (i, ch) in s.chars().enumerate() {
            if ch.is_uppercase() && i > 0 {
                out.push(' ');
                out.extend(ch.to_lowercase());
            } else {
                out.push(ch);
            }
        }
        out
    };
    match item {
        RepairItem::Region(r) => words(format!("{r:?}")),
        RepairItem::Component(c) => words(format!("{c:?}")),
        RepairItem::DetachedPart(r) => format!("Refit {}", words(format!("{r:?}")).to_lowercase()),
    }
}

fn mod_group(m: &wp_economy::ModDef) -> &'static str {
    use wp_economy::ModEffect;
    if m.is_durability_mod() {
        return "durability";
    }
    if m.effects
        .iter()
        .any(|e| matches!(e, ModEffect::Vehicle { .. }))
    {
        return "performance";
    }
    if m.effects
        .iter()
        .any(|e| matches!(e, ModEffect::Tool { .. }))
    {
        return "tool";
    }
    if m.effects
        .iter()
        .any(|e| matches!(e, ModEffect::Durability { .. }))
    {
        // Lightweight panels: weaker body for less mass.
        return "performance";
    }
    "cosmetic"
}

/// Wanted level for a heat value (the built-in police thresholds).
pub fn level_for_heat(heat: f32) -> WantedLevel {
    let cfg = wp_police::PoliceConfig::builtin();
    WantedLevel::from_heat(heat, &cfg.heat.heat.level_thresholds)
}

/// Heat at the start of a wanted level (the built-in police thresholds).
fn heat_for_level(level: WantedLevel) -> f32 {
    let cfg = wp_police::PoliceConfig::builtin();
    let t = cfg.heat.heat.level_thresholds;
    match level {
        WantedLevel::None => 0.0,
        WantedLevel::L1 => t[0],
        WantedLevel::L2 => t[1],
        WantedLevel::L3 => t[2],
        WantedLevel::L4 => t[3],
        WantedLevel::L5 => t[4],
    }
}

impl Career {
    /// A new career in `mode` (starting cash, no car yet).
    pub fn new(econ: Economy, mode: DamageMode) -> Self {
        let profile = econ.new_career(PLAYER_ACCOUNT, mode);
        Self::from_profile(econ, profile)
    }

    /// A career from a loaded profile.
    pub fn from_profile(econ: Economy, profile: CareerProfile) -> Self {
        Self {
            econ,
            profile,
            run: None,
            dealer: None,
            abandoned: None,
        }
    }

    /// Load a save (see `wp_economy::profile` for versions and migration). A save written
    /// during a pursuit (the game was closed mid-chase) is settled as a bust here
    /// ([`Economy::settle_abandoned_run`]); the result waits in [`Career::take_abandoned`].
    pub fn load_json(econ: Economy, text: &str) -> Result<Self, String> {
        let mut p = CareerProfile::from_json(text).map_err(|e| e.to_string())?;
        for (uid, v) in &p.lattices {
            serde_json::from_str::<FullDamageSnapshot>(v)
                .map_err(|e| format!("car {uid}: bad lattice damage: {e}"))?;
        }
        let first_seq = p.account.next_event_seq;
        let mut rng = wp_math::Rng::new(
            CAREER_SEED_SALT ^ ABANDONED_SEED_SALT ^ ((p.account.day as u64) << 32),
        );
        let settled = econ.settle_abandoned_run(&mut p, &mut rng).map(|_| {
            // The bust can cost a tier; progression only credits, so it never refuses.
            let progress = match econ.update_progression(&mut p.account) {
                Ok((up, ev)) => {
                    p.ledger.extend(ev);
                    up
                }
                Err(_) => ProgressUpdate::default(),
            };
            RunClose {
                end: RunEnd::Captured,
                carried_heat: p.carry.heat,
                cash: p.account.cash,
                ledger: p
                    .ledger
                    .iter()
                    .filter(|e| e.seq >= first_seq)
                    .map(LedgerLine::of)
                    .collect(),
                progress,
            }
        });
        let mut career = Self::from_profile(econ, p);
        career.abandoned = settled;
        Ok(career)
    }

    /// The bust settled on load for a run the game was closed in during a pursuit, once.
    pub fn take_abandoned(&mut self) -> Option<RunClose> {
        self.abandoned.take()
    }

    /// The profile (between runs; during a run the account lives in the session, see
    /// [`Career::live_profile`]).
    pub fn profile(&self) -> &CareerProfile {
        &self.profile
    }

    /// Test access to the profile.
    #[cfg(test)]
    pub(crate) fn profile_mut(&mut self) -> &mut CareerProfile {
        &mut self.profile
    }

    /// The economy tables.
    pub fn economy(&self) -> &Economy {
        &self.econ
    }

    /// A career run is being driven.
    pub fn run_active(&self) -> bool {
        self.run.is_some()
    }

    /// The save JSON (between runs).
    pub fn save_json(&self) -> String {
        self.profile.to_json()
    }

    fn guard(&self) -> Result<(), String> {
        if self.run.is_some() {
            return Err("finish the run first (back to the garage)".into());
        }
        Ok(())
    }

    fn mode(&self) -> DamageMode {
        self.profile.mode
    }

    // ----- cars, mods and lattices ---------------------------------------------------------

    /// Overlay files that build `car` with its mods (empty for a stock car). The modified
    /// definitions keep the base ids (no other vehicle uses a roster car's files: police
    /// drive `sentinel_*`, traffic `debug_sedan`, and neither is sold).
    pub fn car_files(&self, car: &OwnedCar) -> Result<BTreeMap<String, String>, String> {
        Ok(self.car_build(car)?.0)
    }

    /// [`Self::car_files`] and the gadgets the car's mods fit (run-flat tyres...).
    fn car_build(&self, car: &OwnedCar) -> Result<(BTreeMap<String, String>, ToolLoadout), String> {
        let mut files = BTreeMap::new();
        if car.mods.is_empty() && self.econ.wear_torque_factor(car) >= 1.0 {
            return Ok((files, ToolLoadout::default()));
        }
        let base = base_car(&car.def_id)?;
        let p = self
            .econ
            .car_preview(&base.def, &base.lattice, car, Some(self.mode()))
            .map_err(e2s)?;
        let v = &p.applied.vehicle;
        let l = &p.applied.lattice;
        files.insert(
            format!("cars/{}.json", v.id),
            serde_json::to_string(v).map_err(|e| e.to_string())?,
        );
        files.insert(
            format!("lattices/{}.json", l.id),
            serde_json::to_string(l).map_err(|e| e.to_string())?,
        );
        Ok((files, p.applied.tools))
    }

    /// The car as the sim builds it (mods applied).
    pub fn loaded_car(&self, car: &OwnedCar) -> Result<LoadedCar, String> {
        let files = self.car_files(car)?;
        simcli::data::load_car(&Overlay { files: &files }, &car.def_id).map_err(|e| e.to_string())
    }

    /// The race picker priced for the active car as it is: its mods move its class index,
    /// and with it the class bracket the purse and buy-in come from, so the stock car's cards
    /// would advertise a different stake than the run pays (owner report 2026-10-26). Empty
    /// stakes when there is no usable active car.
    pub fn race_cards(&self) -> Vec<crate::race::RaceCard> {
        let car = self
            .profile
            .account
            .active_car
            .and_then(|uid| self.profile.account.car(uid))
            .and_then(|car| self.loaded_car(car).ok());
        crate::race::race_cards(car.as_ref(), self.mode())
    }

    /// The car's saved lattice damage, if any.
    pub fn lattice(&self, uid: CarUid) -> Result<Option<FullDamageSnapshot>, String> {
        self.profile
            .lattices
            .get(&uid.0)
            .map(|v| serde_json::from_str(v).map_err(|e| e.to_string()))
            .transpose()
    }

    /// A damage model of the car with its saved damage restored.
    pub fn damage_model(&self, car: &OwnedCar) -> Result<DamageModel, String> {
        let loaded = self.loaded_car(car)?;
        let mut model = DamageModel::new(&loaded.lattice, &loaded.materials);
        if let Some(snap) = self.lattice(car.uid)? {
            // A panel torn off in a session is saved as a synthetic detached group
            // (>= SYNTHETIC_GROUP_BASE), which exists only once the car's panels are attached
            // (as the session did): attach them before restoring (playtest row 163).
            if snap
                .snapshot
                .detached_groups
                .iter()
                .any(|g| *g >= wp_damage::panels::SYNTHETIC_GROUP_BASE)
            {
                let files = self.car_files(car)?;
                let rel = format!("panels/{}.json", car.def_id);
                let text = simcli::data::DataSource::read(&Overlay { files: &files }, &rel)
                    .map_err(|e| e.to_string())?;
                let spec =
                    wp_damage::PanelSpec::parse(&car.def_id, &text).map_err(|e| e.to_string())?;
                model.attach_panels(&spec).map_err(|e| e.to_string())?;
            }
            model.restore_full(&snap)?;
        }
        Ok(model)
    }

    /// Store `model` as car `uid`'s damage: the lattice blob (dropped when the body is
    /// pristine) and a fresh `DamageReport` priced for the car (no towing).
    fn store_damage(
        &mut self,
        uid: CarUid,
        model: &DamageModel,
        wanted: bool,
        lattice: &wp_contracts::LatticeDef,
        materials: &BTreeMap<String, wp_contracts::MaterialDef>,
    ) -> Result<Vec<EconomyEvent>, String> {
        let car = self.profile.account.try_car(uid).map_err(e2s)?;
        let price = self.econ.price_table_for(car);
        let mode_table = self.econ.mode_table(self.mode()).clone();
        // A garage model was never stepped: derive its effects (a dead engine reads as dead).
        let mut model = model.clone();
        model.refresh_effects(&mode_table);
        let model = &model;
        let summary = model.summary();
        let report = DamageReport {
            car: uid,
            health: summary.health,
            total_loss: summary.total_loss,
            handling: model.handling_effects(),
            estimate: model.repair_estimate(&mode_table, &price),
            lattice_state: Vec::new(),
            wanted_during_session: wanted,
            towed: false,
        };
        let full = model.save_full();
        let pristine = DamageModel::new(lattice, materials).save_full();
        if full.snapshot == pristine.snapshot {
            self.profile.lattices.remove(&uid.0);
        } else {
            self.profile.lattices.insert(
                uid.0,
                serde_json::to_string(&full).map_err(|e| e.to_string())?,
            );
        }
        let ((), ev) = self
            .econ
            .record_damage(&mut self.profile.account, &report)
            .map_err(e2s)?;
        self.profile.ledger.extend(ev.iter().cloned());
        Ok(ev)
    }

    fn stats_for(&self, def_id: &str, mods: &[String]) -> Result<StatsView, String> {
        let base = base_car(def_id)?;
        let p = self
            .econ
            .preview(&base.def, &base.lattice, mods, Some(self.mode()))
            .map_err(e2s)?;
        Ok(stats_of(&p))
    }

    /// Cars the dealer sells: the street roster (not the debug cars, not police), cheapest
    /// first.
    pub fn dealer(&mut self) -> &[DealerCar] {
        if self.dealer.is_none() {
            let mut list = Vec::new();
            for id in crate::data::car_ids() {
                if id.starts_with("debug_") {
                    continue;
                }
                let Ok(car) = base_car(&id) else {
                    continue;
                };
                let d: &VehicleDef = &car.def;
                if matches!(
                    d.class,
                    VehicleClass::PoliceCruiser
                        | VehicleClass::PoliceInterceptor
                        | VehicleClass::PoliceSuv
                ) {
                    continue;
                }
                let spec = CarSpec::from(d);
                let Ok(stats) = self.stats_for(&id, &[]) else {
                    continue;
                };
                let Some(unlock_tier) = self.econ.progression.class_tier(d.class) else {
                    continue;
                };
                list.push(DealerCar {
                    def_id: id.clone(),
                    name: d.display_name.clone(),
                    class: d.class,
                    base_price: d.base_price,
                    price_new: self.econ.market_price(&spec, Condition::New),
                    price_used: self.econ.market_price(&spec, Condition::Used),
                    price_worn: self.econ.market_price(&spec, Condition::Worn),
                    stats,
                    unlock_tier,
                    locked: false,
                });
            }
            list.sort_by(|a, b| (a.price_worn, &a.def_id).cmp(&(b.price_worn, &b.def_id)));
            self.dealer = Some(list);
        }
        self.dealer.as_deref().unwrap_or_default()
    }

    // ----- views ---------------------------------------------------------------------------

    fn car_view(&self, c: &OwnedCar) -> CarView {
        let acct = &self.profile.account;
        let shop = ShopContext::legit_default();
        let repair_all = self
            .econ
            .quote_repair(acct, c.uid, &RepairOption::RepairAll, &shop, self.mode())
            .ok();
        let (lines, _) = self
            .econ
            .price_lines(c, &c.damage.outstanding.lines, &shop, self.mode());
        let repair_lines = lines
            .iter()
            .map(|l| RepairLineView {
                item: l.item,
                label: label_of(&l.item),
                severity: l.severity,
                price: l.total,
            })
            .collect();
        // A rare variant keeps the name it was sold under.
        let name = c.variant_name.clone().unwrap_or_else(|| {
            base_car(&c.def_id)
                .map(|l| l.def.display_name)
                .unwrap_or_else(|_| c.def_id.clone())
        });
        let mods = c
            .mods
            .iter()
            .map(|id| {
                let m = self.econ.catalog.get(id);
                InstalledModView {
                    id: id.clone(),
                    name: m.map_or_else(|| id.clone(), |m| m.name.clone()),
                    slot: m.map_or_else(String::new, |m| format!("{:?}", m.slot)),
                }
            })
            .collect();
        CarView {
            uid: c.uid.0,
            def_id: c.def_id.clone(),
            name,
            rare: c.variant_name.is_some(),
            paint: car_paint(c),
            respray_fee: self.econ.respray_fee(c),
            class: c.class,
            active: acct.active_car == Some(c.uid),
            location: match c.location {
                CarLocation::Garage => "Garage",
                CarLocation::Impound { .. } => "Impound",
            },
            health: c.damage.health,
            total_loss: c.damage.total_loss,
            drivable: c.is_usable(),
            value: self.econ.current_value(c),
            sale_price: self.econ.sale_price(c),
            paid: self.econ.resale_basis(c),
            scrap_value: self.econ.scrap_value(c),
            mods,
            tune: c.tune,
            insurance: c.insurance.as_ref().map(|i| i.tier.clone()),
            premium_per_day: c.insurance.as_ref().map(|i| i.premium_per_day),
            repair_all,
            repair_lines,
            impound: self.econ.impound_status(acct, c.uid).ok(),
            stats: self.stats_for(&c.def_id, &c.mods).ok(),
            damage_before_capture: c.damage.sustained_before_capture,
            tires: self.tire_view(c),
        }
    }

    fn tire_view(&self, c: &OwnedCar) -> TireView {
        let (grip, _) = self.econ.tire_effects(&c.tires);
        TireView {
            tread: c.tires.tread,
            flat: c.tires.flat,
            worn_out: c.tires.worn_out(self.econ.lots.tires.bald_below),
            grip,
            price_each: self.econ.tire_quote(c, [true, false, false, false]),
        }
    }

    // ----- car lots ------------------------------------------------------------------------

    /// The models the lots stock from: the street roster the dealer sells.
    fn roster(&mut self) -> Vec<CarSpec> {
        self.dealer()
            .iter()
            .map(|d| CarSpec {
                def_id: d.def_id.clone(),
                class: d.class,
                base_price: d.base_price,
            })
            .collect()
    }

    /// Today's stock of `lot_id` with whether each car was sold today.
    pub fn lot_stock(&mut self, lot_id: &str) -> Result<Vec<(Listing, bool)>, String> {
        if self.econ.lot(lot_id).is_none() {
            return Err(format!("no lot {lot_id:?}"));
        }
        let roster = self.roster();
        let day = self.profile.account.day;
        let sales = &self.profile.lot_sales;
        Ok(self
            .econ
            .lot_stock(lot_id, day, &roster)
            .into_iter()
            .map(|l| {
                let sold = sales.is_sold(day, lot_id, l.slot);
                (l, sold)
            })
            .collect())
    }

    fn lot_summary(&mut self, lot_id: &str) -> Result<LotSummary, String> {
        let stock = self.lot_stock(lot_id)?;
        let lot = self.econ.lot(lot_id).expect("checked by lot_stock");
        let open: Vec<&Listing> = stock.iter().filter(|(_, s)| !s).map(|(l, _)| l).collect();
        Ok(LotSummary {
            id: lot.id.clone(),
            name: lot.name.clone(),
            kind: lot.kind,
            tier: lot.tier,
            district: format!("{:?}", lot.district),
            classes: lot.classes.clone(),
            in_stock: open.len(),
            from_price: open.iter().map(|l| l.price).min(),
        })
    }

    /// Every lot with today's counts.
    pub fn lots(&mut self) -> Vec<LotSummary> {
        let ids: Vec<String> = self.econ.lots.lots.iter().map(|l| l.id.clone()).collect();
        ids.iter()
            .filter_map(|id| self.lot_summary(id).ok())
            .collect()
    }

    fn listing_view(&self, l: &Listing, sold: bool) -> ListingView {
        let name = match &l.variant_name {
            Some(n) => n.clone(),
            None => base_car(&l.def_id)
                .map(|c| c.def.display_name)
                .unwrap_or_else(|_| l.def_id.clone()),
        };
        let region = |r: &wp_contracts::BodyRegion| label_of(&RepairItem::Region(*r));
        let tired: Vec<(String, f32)> = l
            .wear
            .components
            .iter()
            .map(|(c, h)| (label_of(&RepairItem::Component(*c)), *h))
            .collect();
        let shown = !l.hidden;
        let (health, est) = if shown {
            self.listing_condition(l).unwrap_or((l.condition, 0))
        } else {
            (l.condition, 0)
        };
        ListingView {
            slot: l.slot,
            def_id: l.def_id.clone(),
            name,
            class: l.class,
            grade: l.grade,
            price: l.price,
            rare: l.variant.is_some(),
            paint: l.paint,
            mods: l
                .mods
                .iter()
                .map(|m| {
                    self.econ
                        .catalog
                        .get(m)
                        .map_or_else(|| m.clone(), |d| d.name.clone())
                })
                .collect(),
            condition: shown.then_some(health),
            est_repairs: shown.then_some(est),
            runs: shown.then_some(l.runs),
            tired_parts: shown.then_some(tired),
            missing: l.wear.missing.iter().map(region).collect(),
            dents: l.wear.dents.keys().map(region).collect(),
            tread: l.tires.tread,
            flat: l.tires.flat,
            stats: self.stats_for(&l.def_id, &l.mods).ok(),
            sold,
            unlock_tier: self.econ.progression.class_tier(l.class),
            locked: !self.econ.class_unlocked(&self.profile.account, l.class),
        }
    }

    /// One lot's stock today.
    pub fn lot_view(&mut self, lot_id: &str) -> Result<LotStockView, String> {
        let lot = self.lot_summary(lot_id)?;
        let stock = self.lot_stock(lot_id)?;
        Ok(LotStockView {
            lot,
            day: self.profile.account.day,
            cars: stock
                .iter()
                .map(|(l, s)| self.listing_view(l, *s))
                .collect(),
        })
    }

    /// The wear a listing's lattice gets (the damage-model form of `ListingWear`).
    pub fn preowned_wear(l: &Listing) -> PreownedWear {
        PreownedWear {
            components: l.wear.components.clone(),
            missing: l.wear.missing.clone(),
            dents: l.wear.dents.clone(),
        }
    }

    /// Buy the car in `slot` of `lot_id` today: pays, adds the car with the listing's mods and
    /// tyres, and damages its stored lattice with the listing's wear (so a junkyard car really
    /// arrives with a dead engine or a missing hood). Becomes the active car if there is none
    /// usable.
    pub fn buy_listing(&mut self, lot_id: &str, slot: u32) -> Result<CarUid, String> {
        self.guard()?;
        let (listing, sold) = self
            .lot_stock(lot_id)?
            .into_iter()
            .find(|(l, _)| l.slot == slot)
            .ok_or_else(|| format!("nothing in space {slot} at this lot today"))?;
        if sold {
            return Err("already sold today".into());
        }
        // The same rep-tier class locks as the dealer.
        self.econ
            .require_class(&self.profile.account, listing.class)
            .map_err(e2s)?;
        let had_usable = self.active_usable();
        let mut acct = self.profile.account.clone();
        let (uid, ev) = self.econ.buy_listing(&mut acct, &listing).map_err(e2s)?;
        let car = acct.try_car(uid).map_err(e2s)?.clone();
        // Build the damaged car before committing, so broken data cannot take money.
        let loaded = self.loaded_car(&car)?;
        let mut model = DamageModel::new(&loaded.lattice, &loaded.materials);
        let wear = Self::preowned_wear(&listing);
        if !wear.is_empty() {
            model.apply_preowned_wear(&wear, self.econ.mode_table(self.mode()));
        }
        self.profile.account = acct;
        if !had_usable {
            self.profile.account.active_car = Some(uid);
        }
        self.post(ev);
        self.store_damage(uid, &model, false, &loaded.lattice, &loaded.materials)?;
        let day = self.profile.account.day;
        self.profile.lot_sales.record(day, lot_id, slot);
        Ok(uid)
    }

    /// Replace the tyres in `wheels` (FL, FR, RL, RR). Repairs never fix tyres.
    pub fn replace_tires(&mut self, uid: CarUid, wheels: [bool; 4]) -> Result<Money, String> {
        self.guard()?;
        let (paid, ev) = self
            .econ
            .replace_tires(&mut self.profile.account, uid, wheels)
            .map_err(e2s)?;
        self.post(ev);
        Ok(paid)
    }

    /// Grip and deflation of car `uid`'s tyres for the sim.
    pub fn tire_effects(&self, uid: CarUid) -> Option<([f32; 4], [f32; 4])> {
        let c = self.profile.account.car(uid)?;
        Some(self.econ.tire_effects(&c.tires))
    }

    /// Health and Repair All price (legit residential shop) of a listing once its wear is on
    /// the lattice: what the buyer of a used car can see on the lot.
    fn listing_condition(&self, l: &Listing) -> Result<(f32, Money), String> {
        let wear = Self::preowned_wear(l);
        if wear.is_empty() {
            return Ok((1.0, 0));
        }
        let base = base_car(&l.def_id)?;
        let mut model = DamageModel::new(&base.lattice, &base.materials);
        let mode_table = self.econ.mode_table(self.mode());
        model.apply_preowned_wear(&wear, mode_table);
        let car = OwnedCar {
            uid: CarUid(0),
            def_id: l.def_id.clone(),
            class: l.class,
            base_price: l.base_price,
            mods: l.mods.clone(),
            damage: Default::default(),
            location: CarLocation::Garage,
            insurance: None,
            purchase_price: Some(l.price),
            tune: Default::default(),
            parts: None,
            tires: l.tires,
            variant_name: None,
            paint: None,
        };
        let estimate = model.repair_estimate(mode_table, &self.econ.price_table_for(&car));
        let (_, gross) = self.econ.price_lines(
            &car,
            &estimate.lines,
            &ShopContext::legit_default(),
            self.mode(),
        );
        Ok((model.health(), gross))
    }

    /// Lattice snapshot of a listing as it sits on the lot (for the lot visit's parked cars),
    /// `None` for a pristine car. Mods are not applied (the lot shows the stock body).
    pub fn listing_damage(&self, l: &Listing) -> Result<Option<FullDamageSnapshot>, String> {
        let wear = Self::preowned_wear(l);
        if wear.is_empty() {
            return Ok(None);
        }
        let base = base_car(&l.def_id)?;
        let mut model = DamageModel::new(&base.lattice, &base.materials);
        model.apply_preowned_wear(&wear, self.econ.mode_table(self.mode()));
        Ok(Some(model.save_full()))
    }

    /// Why the active car cannot start a run (`None` = it can).
    pub fn start_blocker(&self) -> Option<String> {
        if self.run.is_some() {
            return Some("a run is in progress".into());
        }
        let acct = &self.profile.account;
        let Some(c) = acct.active_car.and_then(|uid| acct.car(uid)) else {
            let impounded = acct
                .garage
                .iter()
                .any(|c| matches!(c.location, CarLocation::Impound { .. }));
            return Some(if acct.has_usable_car() {
                "pick a car".into()
            } else if impounded {
                "the car is in the pound: release it, or buy another".into()
            } else {
                "no car: buy one (or take the street beater)".into()
            });
        };
        if c.location != CarLocation::Garage {
            return Some("the car is in the pound".into());
        }
        if !c.damage.is_drivable() {
            return Some("the car is wrecked: repair or scrap it".into());
        }
        let missing = self.econ.missing_parts(c);
        if !missing.is_empty() {
            return Some(format!(
                "the car is missing parts: {} (fit them in the workshop)",
                missing.join(", ")
            ));
        }
        None
    }

    /// The garage screen.
    pub fn view(&mut self) -> CareerView {
        self.dealer();
        let lots = self.lots();
        let p = &self.profile;
        let a = &p.account;
        let floor = &self.econ.market.floor_car;
        CareerView {
            version: p.version,
            mode: p.mode,
            day: a.day,
            cash: a.cash,
            debt: a.debt(),
            rep: a.rep,
            bounty: a.bounty,
            net_worth: self.econ.net_worth(a),
            carried_heat: p.carry.heat,
            carried_level: level_for_heat(p.carry.heat),
            run_active: self.run.is_some(),
            active_car: a.active_car.map(|u| u.0),
            can_start: self.start_blocker().is_none(),
            start_blocker: self.start_blocker(),
            cars: a.garage.iter().map(|c| self.car_view(c)).collect(),
            dealer: self
                .dealer
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|mut d| {
                    d.locked = !self.econ.class_unlocked(a, d.class);
                    d
                })
                .collect(),
            lots,
            floor_car: (!a.has_usable_car())
                .then(|| (floor.def_id.clone(), floor.price.min(a.cash.max(0)))),
            insurance: self
                .econ
                .prices
                .insurance
                .iter()
                .map(|(id, t)| InsuranceView {
                    id: id.clone(),
                    premium_per_day_frac: t.premium_per_day_frac,
                    coverage: t.coverage,
                    deductible: t.deductible,
                })
                .collect(),
            districts: DistrictId::ALL
                .iter()
                .map(|d| DistrictView {
                    id: format!("{d:?}"),
                    labour: self
                        .econ
                        .prices
                        .district_labour
                        .get(d)
                        .copied()
                        .unwrap_or(1.0),
                })
                .collect(),
            shops: crate::garage_visit::WORLD_SHOPS
                .iter()
                .map(|w| ShopView {
                    name: w.name.to_string(),
                    district: format!("{:?}", w.district),
                    kind: w.kind,
                    city: w.city.to_string(),
                    city_label: crate::scenario::spawn(w.city)
                        .map_or(w.city, |s| s.label)
                        .to_string(),
                    labour: self
                        .econ
                        .prices
                        .district_labour
                        .get(&w.district)
                        .copied()
                        .unwrap_or(1.0),
                })
                .collect(),
            stats: p.stats.clone(),
            progress: self.econ.career_progress(a),
            captures: a.heat_history.captures,
            escapes: a.heat_history.escapes,
            ledger: p
                .ledger
                .iter()
                .rev()
                .take(VIEW_LEDGER_LINES)
                .rev()
                .map(LedgerLine::of)
                .collect(),
        }
    }

    /// The mod tree for car `uid`, with the stats each change would give.
    pub fn mod_tree(&self, uid: CarUid) -> Result<ModTreeView, String> {
        let c = self.profile.account.try_car(uid).map_err(e2s)?;
        let base = base_car(&c.def_id)?;
        let mode = self.mode();
        let now = self
            .econ
            .preview(&base.def, &base.lattice, &c.mods, Some(mode))
            .map_err(e2s)?;
        let mut mods: Vec<&wp_economy::ModDef> = self
            .econ
            .catalog
            .available_for(c.class, Some(mode))
            .collect();
        mods.sort_by(|a, b| (a.slot, &a.line, a.tier).cmp(&(b.slot, &b.line, b.tier)));
        let mut out = Vec::new();
        for m in mods {
            let installed = c.mods.contains(&m.id);
            let (next, replaces, check) = if installed {
                let next: Vec<String> = c.mods.iter().filter(|x| **x != m.id).cloned().collect();
                let check = self
                    .econ
                    .catalog
                    .check_uninstall(c.class, &c.mods, &m.id)
                    .map(|_| None);
                (next, None, check)
            } else {
                match self
                    .econ
                    .catalog
                    .check_install(c.class, Some(mode), &c.mods, &m.id)
                {
                    Ok(plan) => {
                        let mut next: Vec<String> = c
                            .mods
                            .iter()
                            .filter(|x| Some(*x) != plan.replaces.as_ref())
                            .cloned()
                            .collect();
                        next.push(m.id.clone());
                        (next, plan.replaces.clone(), Ok(plan.replaces))
                    }
                    Err(e) => (Vec::new(), None, Err(e)),
                }
            };
            let mut reason = check.as_ref().err().map(|e| e.to_string());
            let stats_after = if check.is_ok() {
                match self
                    .econ
                    .preview(&base.def, &base.lattice, &next, Some(mode))
                {
                    Ok(p) => {
                        if reason.is_none() {
                            reason = self.structure_blocker(c, &now, &p);
                        }
                        Some(stats_of(&p))
                    }
                    Err(e) => {
                        reason = Some(e.to_string());
                        None
                    }
                }
            } else {
                None
            };
            if reason.is_none() && !installed && self.profile.account.cash < m.price as Money {
                reason = Some(format!("costs {}", m.price));
            }
            if reason.is_none() && c.location != CarLocation::Garage {
                reason = Some("the car is in the pound".into());
            }
            out.push(ModOption {
                id: m.id.clone(),
                name: m.name.clone(),
                slot: format!("{:?}", m.slot),
                line: m.line.clone(),
                tier: m.tier,
                price: m.price,
                tradeoff: m.tradeoff.clone(),
                group: mod_group(m),
                installed,
                allowed: reason.is_none(),
                reason,
                replaces,
                stats_after,
                requires: m.requires.clone(),
            });
        }
        Ok(ModTreeView {
            uid: uid.0,
            stats: stats_of(&now),
            mods: out,
        })
    }

    /// A mod change that alters the lattice's structure (a cage adds beams) cannot carry the
    /// saved damage over: the body must be repaired first.
    fn structure_blocker(
        &self,
        c: &OwnedCar,
        now: &wp_economy::Preview,
        next: &wp_economy::Preview,
    ) -> Option<String> {
        let (a, b) = (&now.applied.lattice, &next.applied.lattice);
        let same = a.nodes.len() == b.nodes.len() && a.beams.len() == b.beams.len();
        (!same && self.profile.lattices.contains_key(&c.uid.0))
            .then(|| "repair the body first (this changes the car's structure)".into())
    }

    // ----- garage actions ------------------------------------------------------------------

    fn post(&mut self, ev: Vec<EconomyEvent>) -> Vec<EconomyEvent> {
        self.profile.ledger.extend(ev.iter().cloned());
        self.econ.tidy_profile(&mut self.profile);
        ev
    }

    /// Buy a roster car in a condition; it becomes the active car if there is none usable.
    pub fn buy_car(&mut self, def_id: &str, condition: Condition) -> Result<CarUid, String> {
        self.guard()?;
        let d = self
            .dealer()
            .iter()
            .find(|d| d.def_id == def_id)
            .cloned()
            .ok_or_else(|| format!("the dealer does not sell {def_id:?}"))?;
        self.econ
            .require_class(&self.profile.account, d.class)
            .map_err(e2s)?;
        let spec = CarSpec {
            def_id: d.def_id,
            class: d.class,
            base_price: d.base_price,
        };
        let had_usable = self.active_usable();
        let (uid, ev) = self
            .econ
            .buy_car(&mut self.profile.account, &spec, condition)
            .map_err(e2s)?;
        if !had_usable {
            self.profile.account.active_car = Some(uid);
        }
        self.post(ev);
        Ok(uid)
    }

    fn active_usable(&self) -> bool {
        let a = &self.profile.account;
        a.active_car
            .and_then(|u| a.car(u))
            .is_some_and(OwnedCar::is_usable)
    }

    /// The economy floor: the street beater for its price or all the cash (no usable car only).
    pub fn floor_car(&mut self) -> Result<CarUid, String> {
        self.guard()?;
        let (uid, ev) = self
            .econ
            .issue_floor_car(&mut self.profile.account)
            .map_err(e2s)?;
        self.post(ev);
        Ok(uid)
    }

    /// Sell a car to the dealer.
    pub fn sell_car(&mut self, uid: CarUid) -> Result<Money, String> {
        self.guard()?;
        let (price, ev) = self
            .econ
            .sell_car(&mut self.profile.account, uid)
            .map_err(e2s)?;
        let a = &mut self.profile.account;
        if a.active_car == Some(uid) {
            a.active_car = a
                .garage
                .iter()
                .find(|c| c.is_usable())
                .or(a.garage.first())
                .map(|c| c.uid);
        }
        self.post(ev);
        Ok(price)
    }

    /// Make `uid` the car runs use.
    pub fn set_active(&mut self, uid: CarUid) -> Result<(), String> {
        self.guard()?;
        self.econ
            .set_active_car(&mut self.profile.account, uid)
            .map_err(e2s)
    }

    /// Set a car's free tune (no money; applied when the next run spawns the car).
    pub fn set_tune(&mut self, uid: CarUid, tune: wp_economy::CarTune) -> Result<(), String> {
        self.guard()?;
        self.econ
            .set_tune(&mut self.profile.account, uid, tune)
            .map_err(e2s)
    }

    /// Install a mod (pays; a same-line mod is replaced and refunded at resale value).
    pub fn install_mod(&mut self, uid: CarUid, mod_id: &str) -> Result<(), String> {
        self.guard()?;
        self.change_mods(uid, |e, acct, mode| e.install_mod(acct, uid, mod_id, mode))
    }

    /// Respray car `uid` in `paint` (0xRRGGBB) at the paint shop (see `Economy::respray`).
    /// Returns the fee paid.
    pub fn respray(&mut self, uid: CarUid, paint: u32) -> Result<Money, String> {
        self.guard()?;
        let (fee, _) = self
            .econ
            .respray(&mut self.profile.account, uid, paint)
            .map_err(e2s)?;
        Ok(fee)
    }

    /// Remove a mod (refund at resale value).
    pub fn uninstall_mod(&mut self, uid: CarUid, mod_id: &str) -> Result<(), String> {
        self.guard()?;
        self.change_mods(uid, |e, acct, _| e.uninstall_mod(acct, uid, mod_id))
    }

    fn change_mods<T>(
        &mut self,
        uid: CarUid,
        f: impl FnOnce(
            &Economy,
            &mut wp_economy::PlayerAccount,
            DamageMode,
        ) -> wp_economy::EconResult<T>,
    ) -> Result<T, String> {
        let before = self.profile.account.try_car(uid).map_err(e2s)?.clone();
        let base = base_car(&before.def_id)?;
        let now = self
            .econ
            .preview(&base.def, &base.lattice, &before.mods, Some(self.mode()))
            .map_err(e2s)?;
        let mut acct = self.profile.account.clone();
        let (out, ev) = f(&self.econ, &mut acct, self.mode()).map_err(e2s)?;
        let after = acct.try_car(uid).map_err(e2s)?.clone();
        let next = self
            .econ
            .preview(&base.def, &base.lattice, &after.mods, Some(self.mode()))
            .map_err(e2s)?;
        if let Some(why) = self.structure_blocker(&before, &now, &next) {
            return Err(why);
        }
        // Carry the saved damage over to the new build (same structure, maybe new areas).
        if let Some(mut snap) = self.lattice(uid)? {
            snap.snapshot.lattice_id = next.applied.lattice.id.clone();
            self.profile.lattices.insert(
                uid.0,
                serde_json::to_string(&snap).map_err(|e| e.to_string())?,
            );
        }
        self.profile.account = acct;
        self.post(ev);
        Ok(out)
    }

    // ----- garage workshop -----------------------------------------------------------------

    /// The workshop screen of car `uid`: slots, missing parts, inventory and parts catalog.
    pub fn workshop(&self, uid: CarUid) -> Result<WorkshopScreen, String> {
        let a = &self.profile.account;
        let car = a.try_car(uid).map_err(e2s)?;
        Ok(WorkshopScreen {
            view: self.econ.workshop(a, uid, self.mode()).map_err(e2s)?,
            catalog: self.econ.part_catalog(a, uid, self.mode()).map_err(e2s)?,
            cash: a.cash,
            def_id: car.def_id.clone(),
        })
    }

    /// Take the part in `slot` off car `uid` into the inventory. Returns the part's uid.
    pub fn remove_part(&mut self, uid: CarUid, slot: &str) -> Result<u32, String> {
        self.guard()?;
        self.change_mods(uid, |e, acct, _| e.remove_part(acct, uid, slot))
    }

    /// Fit inventory part `part` into `slot` of car `uid`. A new part restores its area of the
    /// stored lattice (`apply_repair`), and the car's damage is re-recorded from it.
    pub fn fit_part(&mut self, uid: CarUid, slot: &str, part: u32) -> Result<(), String> {
        self.guard()?;
        let fit = self.change_mods(uid, |e, acct, mode| e.fit_part(acct, uid, slot, part, mode))?;
        if fit.restore.is_empty() || !self.profile.lattices.contains_key(&uid.0) {
            return Ok(());
        }
        let car = self.profile.account.try_car(uid).map_err(e2s)?.clone();
        let loaded = self.loaded_car(&car)?;
        let mut model = self.damage_model(&car)?;
        model.apply_repair(&fit.restore);
        let wanted = car.damage.sustained_before_capture;
        let ev = self.store_damage(uid, &model, wanted, &loaded.lattice, &loaded.materials)?;
        self.post(ev);
        Ok(())
    }

    /// Take everything in the way of `slot` off car `uid` (DIY, free). Returns the uids of
    /// the parts taken off, in removal order.
    pub fn clear_way(&mut self, uid: CarUid, slot: &str) -> Result<Vec<u32>, String> {
        self.guard()?;
        self.change_mods(uid, |e, acct, _| e.clear_way(acct, uid, slot))
    }

    /// Pay the mechanic to fit a part in `slot` of car `uid` (see
    /// [`Economy::mechanic_install`]). New parts restore their area of the stored lattice,
    /// as [`Self::fit_part`] does.
    pub fn mechanic_install(
        &mut self,
        uid: CarUid,
        slot: &str,
        source: &wp_economy::PartSource,
        sell_old: bool,
    ) -> Result<wp_economy::MechanicJob, String> {
        self.guard()?;
        let job = self.change_mods(uid, |e, acct, mode| {
            e.mechanic_install(acct, uid, slot, source, sell_old, mode)
        })?;
        if job.restore.is_empty() || !self.profile.lattices.contains_key(&uid.0) {
            return Ok(job);
        }
        let car = self.profile.account.try_car(uid).map_err(e2s)?.clone();
        let loaded = self.loaded_car(&car)?;
        let mut model = self.damage_model(&car)?;
        model.apply_repair(&job.restore);
        let wanted = car.damage.sustained_before_capture;
        let ev = self.store_damage(uid, &model, wanted, &loaded.lattice, &loaded.materials)?;
        self.post(ev);
        Ok(job)
    }

    /// Buy a part of `kind` for car `uid` into the inventory (`grade` = a mod id, or `None`
    /// for the stock replacement). Returns the new parts' uids.
    pub fn buy_part(
        &mut self,
        uid: CarUid,
        kind: &str,
        grade: Option<&str>,
    ) -> Result<Vec<u32>, String> {
        self.guard()?;
        let mode = self.mode();
        let (uids, ev) = self
            .econ
            .buy_part(&mut self.profile.account, uid, kind, grade, mode)
            .map_err(e2s)?;
        self.post(ev);
        Ok(uids)
    }

    /// Engine stand: take the part in `slot` out of loose assembly `assembly` (an engine in
    /// the inventory). Returns the part's uid.
    pub fn strip_part(&mut self, assembly: u32, slot: &str) -> Result<u32, String> {
        self.guard()?;
        let (uid, ev) = self
            .econ
            .strip_part(&mut self.profile.account, assembly, slot)
            .map_err(e2s)?;
        self.post(ev);
        Ok(uid)
    }

    /// Engine stand: put inventory part `part` into `slot` of loose assembly `assembly`.
    pub fn build_part(&mut self, assembly: u32, slot: &str, part: u32) -> Result<(), String> {
        self.guard()?;
        let ((), ev) = self
            .econ
            .build_part(&mut self.profile.account, assembly, slot, part)
            .map_err(e2s)?;
        self.post(ev);
        Ok(())
    }

    /// Sell inventory part `part` to the dealer. Returns the price.
    pub fn sell_part(&mut self, part: u32) -> Result<Money, String> {
        self.guard()?;
        let (price, ev) = self
            .econ
            .sell_part(&mut self.profile.account, part)
            .map_err(e2s)?;
        self.post(ev);
        Ok(price)
    }

    fn shop(&self, req: &ShopRequest) -> Result<ShopContext, String> {
        let district = match &req.district {
            None => DistrictId::Residential,
            Some(k) => {
                wp_economy::district_from_key(k).ok_or_else(|| format!("unknown district {k:?}"))?
            }
        };
        Ok(ShopContext {
            district,
            kind: req.shop,
            wanted: level_for_heat(self.profile.carry.heat),
        })
    }

    /// Price a repair without paying.
    pub fn quote(&self, uid: CarUid, req: &ShopRequest) -> Result<RepairQuote, String> {
        let shop = self.shop(req)?;
        self.econ
            .quote_repair(&self.profile.account, uid, &req.option, &shop, self.mode())
            .map_err(e2s)
    }

    /// Pay for a repair and repair the stored lattice to match (`apply_repair` with the paid
    /// items; everything for a full repair; `Scrap` sells the car for salvage). A legit shop
    /// reports a wanted car: the next run starts at least at that wanted level.
    pub fn repair(&mut self, uid: CarUid, req: &ShopRequest) -> Result<RepairQuote, String> {
        self.guard()?;
        let shop = self.shop(req)?;
        let quote = self.quote(uid, req)?;
        let car = self.profile.account.try_car(uid).map_err(e2s)?.clone();
        // Build the model before paying, so a broken save cannot take money for nothing.
        let loaded = self.loaded_car(&car)?;
        let mut model = self.damage_model(&car)?;
        let mut acct = self.profile.account.clone();
        let (order, ev) = self
            .econ
            .repair(&mut acct, uid, &req.option, &shop, self.mode())
            .map_err(e2s)?;
        self.profile.account = acct;
        let reported = ev.iter().find_map(|e| match e.reason {
            EventReason::ShopReported { level, .. } => Some(level),
            _ => None,
        });
        self.post(ev);
        if order.scrapped {
            self.profile.lattices.remove(&uid.0);
            self.reseat_active(uid);
        } else {
            if order.full_reset {
                model.reset();
            } else {
                model.apply_repair(&order.items);
            }
            let wanted = car.damage.sustained_before_capture;
            self.store_damage(uid, &model, wanted, &loaded.lattice, &loaded.materials)?;
            self.econ.tidy_profile(&mut self.profile);
        }
        if let Some(level) = reported {
            let h = heat_for_level(level);
            if h > self.profile.carry.heat {
                self.profile.carry.heat = h;
            }
        }
        Ok(quote)
    }

    fn reseat_active(&mut self, gone: CarUid) {
        let a = &mut self.profile.account;
        if a.active_car == Some(gone) || a.active_car.and_then(|u| a.car(u)).is_none() {
            a.active_car = a
                .garage
                .iter()
                .find(|c| c.is_usable())
                .or(a.garage.first())
                .map(|c| c.uid);
        }
    }

    /// Pay the pound and bring the car home (its damage comes with it).
    pub fn release(&mut self, uid: CarUid) -> Result<Money, String> {
        self.guard()?;
        let fee = self
            .econ
            .retrieval_fee(&self.profile.account, uid)
            .map_err(e2s)?;
        let ((), ev) = self
            .econ
            .retrieve_from_impound(&mut self.profile.account, uid)
            .map_err(e2s)?;
        self.post(ev);
        Ok(fee)
    }

    /// Insure a car (`tier` = `basic` / `full`), or drop its policy (`tier` = `""`).
    pub fn insure(&mut self, uid: CarUid, tier: &str) -> Result<(), String> {
        self.guard()?;
        let ev = if tier.is_empty() {
            let ((), ev) = self
                .econ
                .cancel_insurance(&mut self.profile.account, uid)
                .map_err(e2s)?;
            ev
        } else {
            let ((), ev) = self
                .econ
                .buy_insurance(&mut self.profile.account, uid, tier)
                .map_err(e2s)?;
            ev
        };
        self.post(ev);
        Ok(())
    }

    /// Let a night pass without driving (the auction countdown runs, premiums are charged,
    /// carried heat cools as if the run had ended free).
    pub fn next_night(&mut self) -> Result<Vec<EconomyEvent>, String> {
        self.guard()?;
        let heat = self.profile.carry.heat;
        self.profile.carry.heat = self.econ.carry_heat(heat, RunEnd::Free);
        let mut ev = self.econ.next_night(&mut self.profile);
        ev.extend(self.update_progression().1);
        Ok(ev)
    }

    /// Pay first-time tier rewards and record tier changes after rep moved.
    fn update_progression(&mut self) -> (ProgressUpdate, Vec<EconomyEvent>) {
        match self.econ.update_progression(&mut self.profile.account) {
            Ok((up, ev)) => {
                let ev = self.post(ev);
                (up, ev)
            }
            // Progression never refuses (it only credits); keep the run closing if it ever did.
            Err(_) => (ProgressUpdate::default(), Vec::new()),
        }
    }

    // ----- runs ------------------------------------------------------------------------------

    /// A session of `sc` with owned car `uid` in the first slot: the career's mode, its mods
    /// (overlay files), its saved damage, `heat` as police heat outside the flat lot.
    fn car_session(
        &self,
        uid: CarUid,
        mut sc: simcli::Scenario,
        heat: f32,
    ) -> Result<(Session, EntityId), String> {
        let car = self.profile.account.try_car(uid).map_err(e2s)?.clone();
        let first = sc
            .entities
            .first_mut()
            .ok_or("the scenario has no player entity")?;
        first.car = car.def_id.clone();
        first.tune = vehicle_tune(car.tune);
        // No heat on the flat lot or a touge pass (no police there; the carried heat waits
        // for the next city run).
        let touge = crate::touge::setup_for_scenario(&sc.name).is_some();
        first.police_heat = if sc.city == simcli::scenario::CitySpec::Flat || touge {
            0.0
        } else {
            heat
        };
        sc.mode = self.mode();
        let (files, tools) = self.car_build(&car)?;
        let mut session = Session::new_with_data(sc, false, &Overlay { files: &files })?;
        let player = *session.entities().first().ok_or("no player entity")?;
        if let Some(snap) = self.lattice(uid)? {
            let sim = session.sim_before_start_mut()?;
            // A save that no longer fits the car starts with a pristine body; the unpaid
            // repair lines stay on the account, so nothing is gained.
            let _ = sim
                .damage_mut(player)
                .map_err(|e| e.to_string())?
                .restore_full(&snap);
        }
        // Worn or flat tyres drive like it.
        if let Some((grip, flat)) = self.tire_effects(uid) {
            session
                .sim_before_start_mut()?
                .set_tire_condition(player, grip, flat)
                .map_err(|e| e.to_string())?;
        }
        // Bull bars, push bars and armour hit like a battering ram.
        if !car.mods.is_empty() {
            let base = base_car(&car.def_id)?;
            let ram = self
                .econ
                .preview(&base.def, &base.lattice, &car.mods, Some(self.mode()))
                .map_err(e2s)?
                .applied
                .ram;
            session
                .sim_before_start_mut()?
                .set_ram_profile(player, ram)
                .map_err(|e| e.to_string())?;
        }
        // Run-flat inserts: a spiked or flat tyre limps on instead of folding onto the rim.
        session
            .sim_before_start_mut()?
            .set_run_flat(player, tools.run_flat > 0)
            .map_err(|e| e.to_string())?;
        Ok((session, player))
    }

    /// The garage workshop's view of car `uid`: a sandbox session of `sc` (a flat lot) with
    /// the car as it is (mods, saved damage), not a run. Any owned car in the garage, even one
    /// that cannot drive (parts missing, wrecked).
    pub fn showroom(&self, uid: CarUid, sc: simcli::Scenario) -> Result<Session, String> {
        if self.run.is_some() {
            return Err("finish the run first (back to the garage)".into());
        }
        let car = self.profile.account.try_car(uid).map_err(e2s)?;
        if car.location != CarLocation::Garage {
            return Err("the car is in the pound".into());
        }
        Ok(self.car_session(uid, sc, 0.0)?.0)
    }

    /// Start a career run of `sc` (any live scenario): the active car in the first slot, the
    /// career's mode and carried heat, mods applied, saved damage restored, the account bound
    /// to the session's economy run. Returns the session (install it with `crate::install`).
    pub fn start_run(&mut self, sc: simcli::Scenario) -> Result<Session, String> {
        if let Some(why) = self.start_blocker() {
            return Err(why);
        }
        let acct = &self.profile.account;
        let uid = acct.active_car.expect("start_blocker checked");
        let heat = self.profile.carry.heat;
        let (mut session, player) = self.car_session(uid, sc, heat)?;
        let acct = &self.profile.account;
        let run = if session.run().is_some() {
            let seed = session.scenario().seed ^ CAREER_SEED_SALT ^ ((acct.day as u64) << 32);
            Some(PlayerRun::for_career_car(
                self.econ.clone(),
                acct.clone(),
                self.profile.ledger.clone(),
                player,
                uid,
                self.mode(),
                seed,
            )?)
        } else {
            None
        };
        session.begin_career(run)?;
        // A picker race the career cannot pay the buy-in for does not start (owner report
        // 2026-10-26: 651 cash still raced Loop 9 day). The account is untouched.
        if let Some(why) = session.race().and_then(|r| r.entry_refused()) {
            return Err(why.replace("buy-in refused: ", "Can't race: "));
        }
        self.run = Some(ActiveRun {
            car: uid,
            start_seq: acct.next_event_seq,
        });
        Ok(session)
    }

    /// End the career run driven in `s`: take the account back (settlements, "Repair &
    /// continue"), store the car's damage, apply the carry-over rules, advance one night.
    pub fn finish_run(&mut self, s: &mut Session) -> Result<RunClose, String> {
        let start = self.run.take().ok_or("no career run")?;
        let player: Option<EntityId> = s.entities().first().copied();
        let heat = player
            .and_then(|p| s.sim().police().racer(p))
            .map_or(0.0, |r| r.heat);
        let driven_ticks = s.tick();
        let wrecked = player
            .and_then(|p| s.vehicle(p))
            .is_some_and(|v| v.damage.summary().total_loss);
        let mut end = if wrecked {
            RunEnd::Wrecked
        } else {
            RunEnd::Free
        };
        let mut wanted = false;
        // A pink slip won in a touge battle: the car's lattice as it finished.
        let won_lattice = s
            .race()
            .and_then(|r| r.touge())
            .and_then(|t| t.won_lattice())
            .cloned();
        if let Some(run) = s.end_career() {
            if run.is_over() {
                end = match run.report().map(|r| r.outcome) {
                    Some(RunOutcome::Captured) => RunEnd::Captured,
                    _ => RunEnd::Wrecked,
                };
            }
            wanted = run.was_wanted();
            let (acct, ledger) = run.into_account();
            self.profile
                .ledger
                .extend(ledger.into_iter().filter(|e| e.seq >= start.start_seq));
            self.profile.account = acct;
            if let Some((uid, json)) = won_lattice {
                if self.profile.account.car(uid).is_some() {
                    self.profile.lattices.insert(uid.0, json);
                }
            }
        }
        let bounty_cash: Money = self
            .profile
            .ledger
            .iter()
            .filter(|e| e.seq >= start.start_seq)
            .filter(|e| matches!(e.reason, EventReason::Escape { .. }))
            .map(|e| e.cash_delta)
            .sum();
        // Tyres wear with the time driven; spike strips leave them flat.
        let spiked = player.map_or([0.0; 4], |p| s.sim().police().tire_puncture(p));
        let hours = driven_ticks as f32 / (240.0 * 3600.0);
        if let Some(c) = self.profile.account.car_mut(start.car) {
            self.econ
                .wear_tires(c, hours, spiked.map(|p| p >= TIRE_FLAT_PUNCTURE));
        }
        match self.profile.account.car(start.car).cloned() {
            Some(car) => {
                let vehicle = player.and_then(|p| s.vehicle(p));
                // Data for the car always loads (it loaded at the start); if it ever does not,
                // the account keeps the damage it had and the run still closes.
                if let (Some(v), Ok(loaded)) = (vehicle, self.loaded_car(&car)) {
                    let _ = self.store_damage(
                        start.car,
                        &v.damage,
                        wanted,
                        &loaded.lattice,
                        &loaded.materials,
                    );
                }
            }
            None => self.reseat_active(start.car),
        }
        // A touge battle has no police: it leaves the carried heat as it was.
        let touge = crate::touge::setup_for_scenario(&s.scenario().name).is_some();
        let carried = self.profile.carry.heat;
        self.econ
            .close_run(&mut self.profile, end, heat, bounty_cash, driven_ticks);
        if touge {
            self.profile.carry.heat = carried;
        }
        let (progress, _) = self.update_progression();
        let ledger = self
            .profile
            .ledger
            .iter()
            .filter(|e| e.seq >= start.start_seq)
            .map(LedgerLine::of)
            .collect();
        Ok(RunClose {
            end,
            carried_heat: self.profile.carry.heat,
            cash: self.profile.account.cash,
            ledger,
            progress,
        })
    }

    /// The profile as it would be saved now: between runs the profile itself; during a run
    /// with the session's account and the car's live damage (no night is counted).
    pub fn live_profile(&self, s: Option<&Session>) -> CareerProfile {
        let mut p = self.profile.clone();
        let (Some(start), Some(s)) = (self.run, s.filter(|s| s.is_career())) else {
            return p;
        };
        if let Some(run) = s.run() {
            p.account = run.account().clone();
            p.ledger.extend(
                run.ledger()
                    .iter()
                    .filter(|e| e.seq >= start.start_seq)
                    .cloned(),
            );
        }
        let player = s.entities().first().copied();
        if let (Some(v), true) = (
            player.and_then(|id| s.vehicle(id)),
            p.account.car(start.car).is_some(),
        ) {
            if let Ok(value) = serde_json::to_string(&v.damage.save_full()) {
                p.lattices.insert(start.car.0, value);
            }
        }
        // B05: closing the game is never an escape. Saved in a pursuit, the save carries a
        // marker the next load settles as a bust; merely wanted, it carries the heat a free
        // end of the run would (no penalty when the police are not on the player).
        let over = s.run().is_some_and(|r| r.is_over());
        if let Some(r) = player.and_then(|id| s.sim().police().racer(id)) {
            if r.pursued() && !over && p.account.car(start.car).is_some() {
                p.run_in_pursuit = Some(wp_economy::AbandonedRun {
                    car: start.car,
                    level: r.level,
                    heat: r.heat,
                    driven_ticks: s.tick(),
                });
            } else if !over {
                p.carry.heat = self.econ.carry_heat(r.heat, RunEnd::Free);
            }
        }
        self.econ.tidy_profile(&mut p);
        p
    }

    /// Forget an active run without absorbing it (a save was loaded over it).
    fn drop_run(&mut self) {
        self.run = None;
    }
}

// ----- the browser's career (thread-local) and its exports ---------------------------------

thread_local! {
    static CAREER: RefCell<Option<Career>> = const { RefCell::new(None) };
}

/// The loaded career's account as saved between runs (`None` without a career): the touge
/// menu reads rep and cash from it.
pub(crate) fn career_account() -> Option<wp_economy::PlayerAccount> {
    CAREER.with(|c| c.borrow().as_ref().map(|c| c.profile.account.clone()))
}

pub(crate) fn with_career<T>(
    f: impl FnOnce(&mut Career) -> Result<T, String>,
) -> Result<T, JsValue> {
    CAREER.with(|c| match c.borrow_mut().as_mut() {
        Some(c) => f(c).map_err(crate::err),
        None => Err(crate::err(
            "no career: call career_new() or career_load_json()",
        )),
    })
}

/// End the career run of a session that is being replaced (`crate::install`).
pub(crate) fn absorb(prev: &mut Session) {
    CAREER.with(|c| {
        if let Some(c) = c.borrow_mut().as_mut() {
            if c.run_active() {
                let _ = c.finish_run(prev);
                return;
            }
        }
        // No career to absorb into: just detach.
        prev.end_career();
    });
}

/// The workshop screen ([`career_workshop_json`]).
#[derive(Debug, Clone, Serialize)]
pub struct WorkshopScreen {
    /// Slots, missing parts, upgrades, inventory.
    #[serde(flatten)]
    pub view: wp_economy::WorkshopView,
    /// Parts catalog for the car.
    pub catalog: Vec<wp_economy::CatalogEntry>,
    /// Cash on hand.
    pub cash: Money,
    /// Car model (panel mesh id).
    pub def_id: String,
}

fn json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).expect("serializable")
}

fn view_json(c: &mut Career) -> Result<String, String> {
    Ok(json(&c.view()))
}

/// End a live career run in the current session, if there is one.
pub(crate) fn end_active_run() -> Result<Option<RunClose>, JsValue> {
    let active = CAREER.with(|c| c.borrow().as_ref().is_some_and(Career::run_active));
    if !active {
        return Ok(None);
    }
    let close = crate::with_session_opt(|s| {
        CAREER.with(|c| {
            let mut c = c.borrow_mut();
            let c = c.as_mut().expect("checked");
            match s {
                Some(s) if s.is_career() => c.finish_run(s).map(Some),
                // The session went away: nothing to take back.
                _ => {
                    c.drop_run();
                    Ok(None)
                }
            }
        })
    });
    close.map_err(crate::err)
}

/// Start a new career (`mode` = `"Arcade"` | `"Simulation"`), replacing any current one.
/// Returns the garage view JSON ([`CareerView`]).
#[wasm_bindgen]
pub fn career_new(mode: &str) -> Result<String, JsValue> {
    let mode = crate::data::parse_mode(mode).map_err(crate::err)?;
    detach_session();
    let econ = crate::garage::with_economy(|e| e.clone());
    CAREER.with(|c| *c.borrow_mut() = Some(Career::new(econ, mode)));
    with_career(view_json)
}

/// Start a new career like [`career_new`] but with `starting_cash` instead of the market's
/// (owner request 2026-10-21: preview / demo builds start with $150 000 so every feature can be
/// tested). The economy tables are unchanged; only this career's opening balance differs.
#[wasm_bindgen]
pub fn career_new_with_cash(mode: &str, starting_cash: f64) -> Result<String, JsValue> {
    let mode = crate::data::parse_mode(mode).map_err(crate::err)?;
    detach_session();
    let mut econ = crate::garage::with_economy(|e| e.clone());
    econ.market.starting_cash = starting_cash.max(0.0).round() as wp_economy::Money;
    let career = Career::new(econ, mode);
    // Keep the shared tables for later runs: only the opening balance came from the override.
    let econ = crate::garage::with_economy(|e| e.clone());
    CAREER.with(|c| *c.borrow_mut() = Some(Career::from_profile(econ, career.profile)));
    with_career(view_json)
}

/// Load a career save (localStorage or an imported file), replacing any current one. A run
/// in progress is dropped. Returns the garage view JSON; an error leaves the current career.
#[wasm_bindgen]
pub fn career_load_json(text: &str) -> Result<String, JsValue> {
    let econ = crate::garage::with_economy(|e| e.clone());
    let career = Career::load_json(econ, text).map_err(crate::err)?;
    detach_session();
    CAREER.with(|c| *c.borrow_mut() = Some(career));
    with_career(view_json)
}

/// The current session stops being a career run without settling (a new or loaded career
/// replaces the old one).
fn detach_session() {
    let _ = crate::with_session_opt(|s| {
        if let Some(s) = s {
            if s.is_career() {
                s.end_career();
            }
        }
        Ok::<_, String>(())
    });
}

/// The bust settled when the save was loaded because the game had been closed during a
/// pursuit (B05): [`RunClose`] JSON once, then `null`.
#[wasm_bindgen]
pub fn career_take_abandoned_json() -> Result<String, JsValue> {
    with_career(|c| Ok(json(&c.take_abandoned())))
}

/// A career exists.
#[wasm_bindgen]
pub fn career_exists() -> bool {
    CAREER.with(|c| c.borrow().is_some())
}

/// The save JSON (`CareerProfile`): between runs the profile, during a run including the
/// live account and the car's current damage.
#[wasm_bindgen]
pub fn career_save_json() -> Result<String, JsValue> {
    let live = CAREER.with(|c| c.borrow().as_ref().is_some_and(Career::run_active));
    if live {
        crate::with_session_opt(|s| {
            CAREER.with(|c| {
                let c = c.borrow();
                let c = c.as_ref().expect("checked");
                Ok::<_, String>(c.live_profile(s.as_deref()).to_json())
            })
        })
        .map_err(crate::err)
    } else {
        with_career(|c| Ok(c.save_json()))
    }
}

/// The garage view JSON ([`CareerView`]).
#[wasm_bindgen]
pub fn career_view_json() -> Result<String, JsValue> {
    with_career(view_json)
}

/// The career garage's race picker ([`crate::race::RaceCard`] array, as `race_list`): stakes
/// for the active car with its mods, in the career's mode ([`Career::race_cards`]).
#[wasm_bindgen]
pub fn career_race_list() -> Result<String, JsValue> {
    with_career(|c| Ok(json(&c.race_cards())))
}

/// The mod tree of car `uid` ([`ModTreeView`] JSON).
#[wasm_bindgen]
pub fn career_mods_json(uid: u32) -> Result<String, JsValue> {
    with_career(|c| c.mod_tree(CarUid(uid)).map(|t| json(&t)))
}

/// The current session is a career run.
#[wasm_bindgen]
pub fn career_run_active() -> bool {
    CAREER.with(|c| c.borrow().as_ref().is_some_and(Career::run_active))
}

/// Start a career run of `scenario_json` (free drive, night run, a race: any live scenario;
/// the first entity becomes the career car). Ends a career run in progress first. Returns the
/// entity ids like `init`.
#[wasm_bindgen]
pub fn career_start_run(scenario_json: &str) -> Result<Vec<u32>, JsValue> {
    let sc: simcli::Scenario = serde_json::from_str(scenario_json).map_err(crate::err)?;
    end_active_run()?;
    let session = with_career(|c| c.start_run(sc))?;
    Ok(crate::install(session))
}

/// Show career car `uid` in the garage workshop: installs a sandbox session of
/// `scenario_json` (use the flat lot) with the car as it is, not a run. Returns the entity ids.
#[wasm_bindgen]
pub fn career_workshop_session(uid: u32, scenario_json: &str) -> Result<Vec<u32>, JsValue> {
    let sc: simcli::Scenario = serde_json::from_str(scenario_json).map_err(crate::err)?;
    end_active_run()?;
    let session = with_career(|c| c.showroom(CarUid(uid), sc))?;
    Ok(crate::install(session))
}

/// End the career run (back to the garage): the run's result goes into the career, one night
/// passes. Returns [`RunClose`] JSON, or `null` when no career run was active.
#[wasm_bindgen]
pub fn career_end_run() -> Result<String, JsValue> {
    Ok(json(&end_active_run()?))
}

fn condition(name: &str) -> Result<Condition, String> {
    match name {
        "New" | "new" => Ok(Condition::New),
        "Used" | "used" => Ok(Condition::Used),
        "Worn" | "worn" => Ok(Condition::Worn),
        o => Err(format!("unknown condition {o:?} (New | Used | Worn)")),
    }
}

/// Buy a roster car (`condition` = `New` | `Used` | `Worn`). Returns the garage view JSON.
#[wasm_bindgen]
pub fn career_buy_car(def_id: &str, condition_name: &str) -> Result<String, JsValue> {
    with_career(|c| {
        c.buy_car(def_id, condition(condition_name)?)?;
        view_json(c)
    })
}

/// Take the street beater (economy floor; only with no usable car). Returns the view JSON.
#[wasm_bindgen]
pub fn career_floor_car() -> Result<String, JsValue> {
    with_career(|c| {
        c.floor_car()?;
        view_json(c)
    })
}

/// Sell car `uid` to the dealer. Returns the view JSON.
#[wasm_bindgen]
pub fn career_sell_car(uid: u32) -> Result<String, JsValue> {
    with_career(|c| {
        c.sell_car(CarUid(uid))?;
        view_json(c)
    })
}

/// Make car `uid` the one runs use. Returns the view JSON.
#[wasm_bindgen]
pub fn career_set_active(uid: u32) -> Result<String, JsValue> {
    with_career(|c| {
        c.set_active(CarUid(uid))?;
        view_json(c)
    })
}

/// Set car `uid`'s free tune from JSON (`{"suspension":1..5,"gearing":…,"diff":…,
/// "brake_balance":…}`; missing sliders are stock). Free. Returns the view JSON.
#[wasm_bindgen]
pub fn career_set_tune(uid: u32, tune_json: &str) -> Result<String, JsValue> {
    with_career(|c| {
        let tune: wp_economy::CarTune =
            serde_json::from_str(tune_json).map_err(|e| format!("bad tune: {e}"))?;
        c.set_tune(CarUid(uid), tune)?;
        view_json(c)
    })
}

/// Install mod `mod_id` on car `uid`. Returns the view JSON.
#[wasm_bindgen]
pub fn career_install_mod(uid: u32, mod_id: &str) -> Result<String, JsValue> {
    with_career(|c| {
        c.install_mod(CarUid(uid), mod_id)?;
        view_json(c)
    })
}

/// Respray car `uid` in `paint` (0xRRGGBB) at the paint shop. Returns the view JSON.
#[wasm_bindgen]
pub fn career_respray(uid: u32, paint: u32) -> Result<String, JsValue> {
    with_career(|c| {
        c.respray(CarUid(uid), paint)?;
        view_json(c)
    })
}

/// Remove mod `mod_id` from car `uid`. Returns the view JSON.
#[wasm_bindgen]
pub fn career_uninstall_mod(uid: u32, mod_id: &str) -> Result<String, JsValue> {
    with_career(|c| {
        c.uninstall_mod(CarUid(uid), mod_id)?;
        view_json(c)
    })
}

/// The garage workshop of car `uid`: `WorkshopScreen` JSON (slots with condition and what
/// blocks them, missing parts, upgrades, inventory with where each part fits, parts catalog).
#[wasm_bindgen]
pub fn career_workshop_json(uid: u32) -> Result<String, JsValue> {
    with_career(|c| c.workshop(CarUid(uid)).map(|w| json(&w)))
}

/// Take the part in `slot` off car `uid` into the inventory. Returns the workshop JSON.
#[wasm_bindgen]
pub fn career_remove_part(uid: u32, slot: &str) -> Result<String, JsValue> {
    with_career(|c| {
        c.remove_part(CarUid(uid), slot)?;
        c.workshop(CarUid(uid)).map(|w| json(&w))
    })
}

/// Fit inventory part `part` into `slot` of car `uid`. Returns the workshop JSON.
#[wasm_bindgen]
pub fn career_fit_part(uid: u32, slot: &str, part: u32) -> Result<String, JsValue> {
    with_career(|c| {
        c.fit_part(CarUid(uid), slot, part)?;
        c.workshop(CarUid(uid)).map(|w| json(&w))
    })
}

/// Take everything in the way of `slot` off car `uid`. Returns the workshop JSON.
#[wasm_bindgen]
pub fn career_clear_way(uid: u32, slot: &str) -> Result<String, JsValue> {
    with_career(|c| {
        c.clear_way(CarUid(uid), slot)?;
        c.workshop(CarUid(uid)).map(|w| json(&w))
    })
}

/// Pay the mechanic to fit a part in `slot` of car `uid`: inventory part `part` when it is
/// not 0, otherwise one bought now (`grade` = mod id, `""` = stock). The old part is sold when
/// `sell_old`, kept otherwise. Returns `{"job": MechanicJob, "workshop": WorkshopScreen}`.
#[wasm_bindgen]
pub fn career_mechanic_install(
    uid: u32,
    slot: &str,
    part: u32,
    grade: &str,
    sell_old: bool,
) -> Result<String, JsValue> {
    with_career(|c| {
        let source = if part != 0 {
            wp_economy::PartSource::Inventory(part)
        } else {
            wp_economy::PartSource::Buy((!grade.is_empty()).then(|| grade.to_string()))
        };
        let job = c.mechanic_install(CarUid(uid), slot, &source, sell_old)?;
        let workshop = c.workshop(CarUid(uid))?;
        Ok(serde_json::json!({ "job": job, "workshop": workshop }).to_string())
    })
}

/// Buy a part of `kind` for car `uid` (`grade` = mod id, `""` = stock replacement). Returns
/// the workshop JSON.
#[wasm_bindgen]
pub fn career_buy_part(uid: u32, kind: &str, grade: &str) -> Result<String, JsValue> {
    with_career(|c| {
        let grade = (!grade.is_empty()).then_some(grade);
        c.buy_part(CarUid(uid), kind, grade)?;
        c.workshop(CarUid(uid)).map(|w| json(&w))
    })
}

/// Engine stand: take the part in `slot` out of loose engine `assembly`. Returns the workshop
/// JSON of car `uid`.
#[wasm_bindgen]
pub fn career_strip_part(uid: u32, assembly: u32, slot: &str) -> Result<String, JsValue> {
    with_career(|c| {
        c.strip_part(assembly, slot)?;
        c.workshop(CarUid(uid)).map(|w| json(&w))
    })
}

/// Engine stand: put inventory part `part` into `slot` of loose engine `assembly`. Returns
/// the workshop JSON of car `uid`.
#[wasm_bindgen]
pub fn career_build_part(
    uid: u32,
    assembly: u32,
    slot: &str,
    part: u32,
) -> Result<String, JsValue> {
    with_career(|c| {
        c.build_part(assembly, slot, part)?;
        c.workshop(CarUid(uid)).map(|w| json(&w))
    })
}

/// Sell inventory part `part`; returns the workshop JSON of car `uid`.
#[wasm_bindgen]
pub fn career_sell_part(uid: u32, part: u32) -> Result<String, JsValue> {
    with_career(|c| {
        c.sell_part(part)?;
        c.workshop(CarUid(uid)).map(|w| json(&w))
    })
}

/// Price a repair of car `uid` (`request_json`: `{option?, district?, shop?}`, see
/// [`ShopRequest`]). Returns `RepairQuote` JSON.
#[wasm_bindgen]
pub fn career_quote_json(uid: u32, request_json: &str) -> Result<String, JsValue> {
    with_career(|c| {
        c.quote(CarUid(uid), &ShopRequest::parse(request_json)?)
            .map(|q| json(&q))
    })
}

/// Pay for a repair of car `uid` (same request as [`career_quote_json`]); the stored damage
/// is repaired to match. Returns the view JSON.
#[wasm_bindgen]
pub fn career_repair(uid: u32, request_json: &str) -> Result<String, JsValue> {
    with_career(|c| {
        c.repair(CarUid(uid), &ShopRequest::parse(request_json)?)?;
        view_json(c)
    })
}

/// Pay the pound for car `uid`. Returns the view JSON.
#[wasm_bindgen]
pub fn career_release(uid: u32) -> Result<String, JsValue> {
    with_career(|c| {
        c.release(CarUid(uid))?;
        view_json(c)
    })
}

/// Insure car `uid` (`tier` = `basic` | `full`; `""` drops the policy). Returns the view JSON.
#[wasm_bindgen]
pub fn career_insure(uid: u32, tier: &str) -> Result<String, JsValue> {
    with_career(|c| {
        c.insure(CarUid(uid), tier)?;
        view_json(c)
    })
}

/// Let a night pass without driving. Returns the view JSON.
#[wasm_bindgen]
pub fn career_next_night() -> Result<String, JsValue> {
    with_career(|c| {
        c.next_night()?;
        view_json(c)
    })
}

/// Test helper: put `career` in the browser slot.
#[cfg(test)]
pub(crate) fn set_career(career: Option<Career>) {
    CAREER.with(|c| *c.borrow_mut() = career);
}

/// A career car's body colour: its own paint, or (cars bought before paint was saved, dealer
/// and floor cars) a factory colour fixed by its uid, never by the view or the session.
pub fn car_paint(c: &wp_economy::OwnedCar) -> u32 {
    c.paint.unwrap_or_else(|| {
        let n = wp_economy::FACTORY_PAINTS.len();
        wp_economy::FACTORY_PAINTS[c.uid.0 as usize % n]
    })
}
