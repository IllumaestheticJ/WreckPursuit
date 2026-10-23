//! Economy data tables (`data/economy/*.toml`, `data/mods/*.json`) and the [`Economy`] bundle
//! that every operation takes.
//!
//! All tables are plain serde structs with `deny_unknown_fields`, validated for completeness
//! (every class / wanted level / day phase / mode present) when loaded. The browser embeds the
//! files ([`Economy::embedded`]); the server reads them from disk ([`Economy::load`]).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use wp_contracts::WantedLevel;
use wp_contracts::{
    parse_mode_table, BodyRegion, Component, DamageMode, DayPhase, ModeTable, VehicleClass,
};

use crate::error::{invalid, DataError};
use crate::garage::Condition;
use crate::ledger::Money;
use crate::mods::{ModCatalog, ModPack};
use crate::parts::PartsTable;
use crate::payouts::RaceType;
use crate::progression::ProgressionRules;

/// Every vehicle class, in declaration order.
pub const ALL_CLASSES: [VehicleClass; 9] = [
    VehicleClass::Beater,
    VehicleClass::Compact,
    VehicleClass::Sport,
    VehicleClass::Muscle,
    VehicleClass::Exotic,
    VehicleClass::Truck,
    VehicleClass::PoliceCruiser,
    VehicleClass::PoliceInterceptor,
    VehicleClass::PoliceSuv,
];

/// Every wanted level, lowest first.
pub const ALL_LEVELS: [WantedLevel; 6] = [
    WantedLevel::None,
    WantedLevel::L1,
    WantedLevel::L2,
    WantedLevel::L3,
    WantedLevel::L4,
    WantedLevel::L5,
];

/// Every day phase.
pub const ALL_PHASES: [DayPhase; 4] = [
    DayPhase::Dawn,
    DayPhase::Day,
    DayPhase::Dusk,
    DayPhase::Night,
];

/// Every damage mode.
pub const ALL_MODES: [DamageMode; 3] = [
    DamageMode::Arcade,
    DamageMode::Simulation,
    DamageMode::Extreme,
];

/// Every body region.
pub const ALL_REGIONS: [BodyRegion; 20] = [
    BodyRegion::FrontBumper,
    BodyRegion::Hood,
    BodyRegion::FrontLeftFender,
    BodyRegion::FrontRightFender,
    BodyRegion::LeftDoorF,
    BodyRegion::LeftDoorR,
    BodyRegion::RightDoorF,
    BodyRegion::RightDoorR,
    BodyRegion::Roof,
    BodyRegion::RearLeftQuarter,
    BodyRegion::RearRightQuarter,
    BodyRegion::Trunk,
    BodyRegion::RearBumper,
    BodyRegion::Windshield,
    BodyRegion::RearGlass,
    BodyRegion::FrameFront,
    BodyRegion::FrameRear,
    BodyRegion::FrameLeft,
    BodyRegion::FrameRight,
    BodyRegion::Cabin,
];

/// Every mechanical component.
pub const ALL_COMPONENTS: [Component; 21] = [
    Component::Engine,
    Component::Radiator,
    Component::Battery,
    Component::FuelTank,
    Component::SteeringRack,
    Component::SuspensionFL,
    Component::SuspensionFR,
    Component::SuspensionRL,
    Component::SuspensionRR,
    Component::WheelFL,
    Component::WheelFR,
    Component::WheelRL,
    Component::WheelRR,
    Component::Gearbox,
    Component::Driveshaft,
    Component::BrakeFL,
    Component::BrakeFR,
    Component::BrakeRL,
    Component::BrakeRR,
    Component::Exhaust,
    Component::Lights,
];

// ---------------------------------------------------------------------------------------------
// prices.toml
// ---------------------------------------------------------------------------------------------

/// Repair, towing, scrap and insurance prices (`data/economy/prices.toml`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PriceTables {
    /// Fallback labour hours to fully replace one region / component (× severity per line),
    /// for items missing from `region_labour_hours` / `component_labour_hours`.
    pub labour_hours_per_item: f32,
    /// Labour hours to fully replace each region (also used for re-fitting a detached part).
    /// Fills `PriceTable::labour_hours` (contract v0.2.0).
    #[serde(default)]
    pub region_labour_hours: BTreeMap<BodyRegion, f32>,
    /// Labour hours to fully replace each component.
    #[serde(default)]
    pub component_labour_hours: BTreeMap<Component, f32>,
    /// Fraction of the replacement parts price charged for a *repair* (below the damage
    /// model's replace severity). Fills `PriceTable::repair_parts_fraction`.
    pub repair_parts_fraction: f32,
    /// Clamp on `car_value / class.reference_value` when scaling parts prices.
    pub value_factor_clamp: [f32; 2],
    /// Parts cost to replace each region on the class-reference Compact.
    pub region_parts: BTreeMap<BodyRegion, f32>,
    /// Parts cost to replace each component on the class-reference Compact.
    pub component_parts: BTreeMap<Component, f32>,
    /// Per-class scaling.
    pub class: BTreeMap<VehicleClass, ClassPricing>,
    /// Labour multiplier per district (every contract `DistrictId`).
    pub district_labour: BTreeMap<wp_contracts::DistrictId, f32>,
    /// Shop behaviour.
    pub shop: ShopPricing,
    /// Salvage value formula.
    pub scrap: ScrapPricing,
    /// Insurance tiers keyed by id.
    pub insurance: BTreeMap<String, InsuranceTier>,
}

/// Pricing for one vehicle class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassPricing {
    /// Typical car value in this class; parts prices scale with `car_value / reference_value`.
    pub reference_value: f32,
    /// Multiplier on the reference parts table.
    pub parts_scale: f32,
    /// Labour rate per hour.
    pub labour_per_hour: f32,
    /// Flat towing charge.
    pub towing: Money,
}

/// How shops price and behave.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShopPricing {
    /// Labour markup at a crooked shop for a car that is not wanted.
    pub crooked_markup: f32,
    /// Labour markup at a crooked shop for a wanted car (no questions asked).
    pub crooked_wanted_markup: f32,
    /// A legit shop reports a car at or above this wanted level.
    pub legit_reports_at: WantedLevel,
}

/// Salvage value = base_price × (min_frac + health_frac × health) + mods_frac × mods value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScrapPricing {
    /// Fraction of base price paid for a hulk at zero health.
    pub min_frac: f32,
    /// Extra fraction of base price per unit of remaining health.
    pub health_frac: f32,
    /// Fraction of installed mods' price recovered (prompt: 0.3).
    pub mods_frac: f32,
}

/// One insurance tier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InsuranceTier {
    /// Daily premium as a fraction of the car's value.
    pub premium_per_day_frac: f32,
    /// Fraction of the bill (after deductible) covered.
    pub coverage: f32,
    /// Deductible per claim.
    pub deductible: Money,
}

// ---------------------------------------------------------------------------------------------
// stakes.toml
// ---------------------------------------------------------------------------------------------

/// Race purses, buy-ins, rep, bounties and informants (`data/economy/stakes.toml`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StakesTables {
    /// Fraction of the winner's purse paid per finishing position (index 0 = 1st). Positions
    /// beyond the list get nothing.
    pub placement_share: Vec<f32>,
    /// Rep per finishing position (index 0 = 1st). Positions beyond the list get 0.
    pub placement_rep: Vec<i64>,
    /// Rep for a DNF (usually negative).
    pub dnf_rep: i64,
    /// Clean-race bonus as a fraction of the payout.
    pub clean_bonus: f32,
    /// Extra rep for a clean race.
    pub clean_rep: i64,
    /// Per race type.
    pub race_types: BTreeMap<RaceType, RaceTypeStakes>,
    /// Per contract race kind (`RaceKind::key`): which `race_types` row prices it, and its
    /// purse / buy-in multipliers. Races from `wp_race` carry a kind; see `Economy::race_setup`.
    pub race_kinds: BTreeMap<String, RaceKindStakes>,
    /// Purse / buy-in multipliers by race legality (`Legality::key`).
    pub legality: BTreeMap<String, LegalityStakes>,
    /// Per day phase multipliers.
    pub day_phase: BTreeMap<DayPhase, PhaseStakes>,
    /// Purse multiplier by vehicle class.
    pub class_purse: BTreeMap<VehicleClass, f32>,
    /// Purse multiplier by the racer's wanted level during the race (risk premium).
    pub wanted_purse: BTreeMap<WantedLevel, f32>,
    /// Purse multiplier by damage mode (Simulation pays more for the risk).
    pub mode_purse: BTreeMap<DamageMode, f32>,
    /// Debt rules.
    pub debt: DebtRules,
    /// Pink-slip rules.
    pub pink_slip: PinkSlipRules,
    /// Bounty rules (racer side and cop rewards).
    pub bounty: BountyRules,
    /// Informant tip tiers keyed by id.
    pub informant: BTreeMap<String, InformantTier>,
}

/// Stakes for one race type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RaceTypeStakes {
    /// Winner's purse per km of route.
    pub base_per_km: f32,
    /// Minimum winner's purse.
    pub min_purse: Money,
    /// Buy-in as a fraction of the base purse (0 = free entry).
    pub entry_frac: f32,
    /// Route length beyond this does not increase the purse, km.
    pub max_km: f32,
}

/// How one contract `RaceKind` is priced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RaceKindStakes {
    /// The `race_types` row that sets the per-km purse, minimum and length cap; also the race
    /// type recorded in the ledger.
    pub race_type: RaceType,
    /// Purse multiplier on top of that row.
    pub purse: f32,
    /// Buy-in multiplier on top of that row's `entry_frac`.
    pub entry: f32,
}

/// Multipliers for one race legality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegalityStakes {
    /// Purse multiplier.
    pub purse: f32,
    /// Buy-in multiplier.
    pub entry: f32,
}

/// Every `Legality` key (exhaustive, so a new variant fails to compile here).
pub(crate) fn legality_keys() -> [&'static str; 3] {
    use wp_contracts::Legality;
    let key = |l: Legality| match l {
        Legality::Sanctioned | Legality::Street | Legality::IllegalDaytime => l.key(),
    };
    [
        key(Legality::Sanctioned),
        key(Legality::Street),
        key(Legality::IllegalDaytime),
    ]
}

/// Multipliers for one day phase.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseStakes {
    /// Purse multiplier.
    pub purse: f32,
    /// Buy-in multiplier.
    pub entry: f32,
    /// Rep multiplier.
    pub rep: f32,
    /// Bounty-gain multiplier.
    pub bounty: f32,
}

/// What debt blocks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DebtRules {
    /// Largest buy-in allowed while in debt (0 = only free races).
    pub max_entry_fee: Money,
}

/// Pink-slip rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinkSlipRules {
    /// Modes in which pink slips are allowed (default: Simulation only).
    pub modes: Vec<DamageMode>,
    /// Minimum rep for both racers.
    pub min_rep: i64,
}

/// Bounty growth and cop rewards.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BountyRules {
    /// Bounty added for winning a race while wanted at this level.
    pub race_won_while_wanted: BTreeMap<WantedLevel, u32>,
    /// Bounty added for escaping a pursuit at this level.
    pub escape: BTreeMap<WantedLevel, u32>,
    /// Rep for escaping at this level.
    pub escape_rep: BTreeMap<WantedLevel, i64>,
    /// Cash for escaping at this level (the crew pays the driver who loses the cops), × the
    /// day phase's bounty multiplier. Chases are the risky fast money of a career run.
    pub escape_cash: BTreeMap<WantedLevel, Money>,
    /// Base cash reward to the capturing cop by wanted level.
    pub cop_base_reward: BTreeMap<WantedLevel, Money>,
    /// Share of the racer's bounty paid to the capturing cop.
    pub cop_share: f32,
    /// Share of the capture reward paid to each assisting cop.
    pub assist_share: f32,
}

/// One informant tier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InformantTier {
    /// Cost.
    pub cost: Money,
    /// Heat points removed (0–100 scale).
    pub heat_reduction: f32,
}

// ---------------------------------------------------------------------------------------------
// consequences.toml
// ---------------------------------------------------------------------------------------------

/// Capture consequences, impound and cop XP (`data/economy/consequences.toml`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsequenceTables {
    /// Base fine by wanted level.
    pub fine_by_level: BTreeMap<WantedLevel, Money>,
    /// Fine increase per prior capture (fraction of the base fine).
    pub fine_per_prior_capture: f32,
    /// Prior captures counted at most.
    pub prior_capture_cap: u32,
    /// Fine per violation committed this session.
    pub fine_per_violation: ViolationFines,
    /// Lifetime violations count at this weight (fraction of a session violation).
    pub lifetime_violation_weight: f32,
    /// Lifetime violation surcharge is capped at this fraction of the base fine.
    pub lifetime_violation_cap: f32,
    /// Bail = this fraction of the outstanding bounty.
    pub bail_bounty_frac: f32,
    /// Rep lost at capture by wanted level.
    pub rep_loss_by_level: BTreeMap<WantedLevel, i64>,
    /// Pound rules.
    pub impound: ImpoundRules,
    /// Mode differences.
    pub mode: BTreeMap<DamageMode, ModeConsequences>,
    /// Cop XP table.
    pub cop_xp: CopXpTable,
    /// XP needed for cop rank 1, 2, 3... (increasing).
    pub cop_rank_thresholds: Vec<u32>,
    /// What a single-player career carries from one run to the next.
    pub career: CareerRules,
}

/// Career carry-over between runs (`consequences.toml [career]`, see [`crate::profile`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CareerRules {
    /// Heat left when a run ends free (escaped, or quit to the garage while still wanted) is
    /// multiplied by this for the next run: a night off cools the police down.
    pub heat_carry_frac: f32,
    /// Carried heat is capped here (police heat scale; 30 = L2, 50 = L3).
    pub heat_carry_max: f32,
    /// Carried heat below this is forgotten.
    pub heat_carry_min: f32,
    /// Ledger lines kept in a saved profile (older lines are dropped; the account keeps its
    /// sequence numbers).
    pub ledger_keep: usize,
}

/// Fine per violation kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViolationFines {
    /// Speeding.
    pub speeding: Money,
    /// Red light.
    pub red_light: Money,
    /// Wrong way.
    pub wrong_way: Money,
    /// Collision.
    pub collision: Money,
    /// Reckless driving.
    pub reckless: Money,
}

/// Pound rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImpoundRules {
    /// Retrieval fee on day 0, by class.
    pub base_fee: BTreeMap<VehicleClass, Money>,
    /// Storage added per day, by class.
    pub per_day: BTreeMap<VehicleClass, Money>,
    /// Days after which an unclaimed car is auctioned (lost).
    pub auction_after_days: u32,
    /// Fraction of the car's value minus fees returned to the owner after auction.
    pub auction_return_frac: f32,
}

/// Per-mode capture differences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeConsequences {
    /// Chance the car is seized outright (on top of impound) at or above `seizure_min_level`.
    pub seizure_chance: f32,
    /// Lowest wanted level at which seizure can happen.
    pub seizure_min_level: WantedLevel,
    /// A total loss at capture means the car is gone.
    pub total_loss_destroys_car: bool,
    /// Multiplier on fines.
    pub fine_scale: f32,
}

/// Cop XP awards (negative = loss).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CopXpTable {
    /// XP for a capture by wanted level.
    pub capture: BTreeMap<WantedLevel, i64>,
    /// XP for an assist by wanted level.
    pub assist: BTreeMap<WantedLevel, i64>,
    /// XP per clean minute of patrol / pursuit.
    pub clean_minute: i64,
    /// XP for hitting a civilian.
    pub civilian_collision: i64,
    /// XP for wrecking your own unit.
    pub self_wreck: i64,
    /// XP per unit of risk budget overspent (prompt 06's budget, 1.0 = a full budget).
    pub risk_overspend: i64,
}

// ---------------------------------------------------------------------------------------------
// market.toml
// ---------------------------------------------------------------------------------------------

/// Car market, valuation, economy floor and class index (`data/economy/market.toml`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketTables {
    /// Cash a new account starts with.
    pub starting_cash: Money,
    /// Price band `[min, max]` by class; dealer prices are clamped into it.
    pub class_bands: BTreeMap<VehicleClass, [Money; 2]>,
    /// Purchase price multiplier by condition.
    pub condition: BTreeMap<Condition, f32>,
    /// Dealer pays this fraction of a car's current value when you sell.
    pub dealer_buy_frac: f32,
    /// Fraction of a mod's price refunded on removal / counted when selling.
    pub mod_resale_frac: f32,
    /// Fraction of installed mods' price counted in the car's value.
    pub mod_value_frac: f32,
    /// A damaged car is never valued below this fraction of its pristine value.
    pub damaged_value_floor: f32,
    /// The economy-floor beater, always obtainable.
    pub floor_car: FloorCar,
    /// Performance index coefficients.
    pub class_index: ClassIndexCoeffs,
    /// Grip factor per tire id (street = 1.0) used by the estimator and class index.
    pub tire_grip: BTreeMap<String, f32>,
}

/// The always-affordable beater.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FloorCar {
    /// Base car id (`data/cars/<def_id>.json`, roster prompt 09).
    pub def_id: String,
    /// Its class (Beater).
    pub class: VehicleClass,
    /// Its base price.
    pub base_price: u32,
    /// Asking price; a player with less cash pays what they have (never into debt).
    pub price: Money,
}

/// Performance index ("class index") coefficients:
/// `PI = base + per_w_per_kg·P/m + per_lateral_g·(lat_g − 1) + per_brake_g·(brake_g − 1)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassIndexCoeffs {
    /// Constant term.
    pub base: f32,
    /// Points per W/kg of peak power to weight.
    pub per_w_per_kg: f32,
    /// Points per g of steady-state lateral grip above 1 g.
    pub per_lateral_g: f32,
    /// Points per g of braking above 1 g.
    pub per_brake_g: f32,
    /// Lowest index.
    pub min: u16,
    /// Highest index.
    pub max: u16,
    /// Lobby brackets, ascending `min`.
    pub brackets: Vec<Bracket>,
}

/// One lobby bracket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bracket {
    /// Display letter.
    pub name: String,
    /// Lowest index in the bracket.
    pub min: u16,
}

// ---------------------------------------------------------------------------------------------
// Economy bundle
// ---------------------------------------------------------------------------------------------

/// Raw file contents an [`Economy`] is built from.
#[derive(Debug, Clone, Default)]
pub struct DataSources {
    /// `data/economy/prices.toml`.
    pub prices: String,
    /// `data/economy/stakes.toml`.
    pub stakes: String,
    /// `data/economy/consequences.toml`.
    pub consequences: String,
    /// `data/economy/market.toml`.
    pub market: String,
    /// `data/economy/parts.toml` (workshop slots).
    pub parts: String,
    /// `data/economy/career.toml` (rep tiers, unlocks, betting limits).
    pub career: String,
    /// `data/economy/lots.toml` (dealerships, used lots, junkyards, tyres).
    pub lots: String,
    /// `data/mods/*.json` as `(file stem, contents)`.
    pub mods: Vec<(String, String)>,
    /// `data/modes/arcade.toml` (owned by workstream 03; read-only here).
    pub arcade_mode: String,
    /// `data/modes/sim.toml`.
    pub sim_mode: String,
    /// `data/modes/extreme.toml`.
    pub extreme_mode: String,
}

/// File stems of the embedded mod packs (`data/mods/<stem>.json`).
pub const MOD_PACKS: [&str; 16] = [
    "aero",
    "body",
    "brakes",
    "cooling",
    "drivetrain",
    "electronics",
    "engine",
    "forced_induction",
    "interior",
    "paint",
    "protection",
    "suspension",
    "tires",
    "vinyls",
    "weight",
    "wheels",
];

macro_rules! data_file {
    ($p:literal) => {
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/", $p))
    };
}

const EMBEDDED_MODS: [&str; 16] = [
    data_file!("mods/aero.json"),
    data_file!("mods/body.json"),
    data_file!("mods/brakes.json"),
    data_file!("mods/cooling.json"),
    data_file!("mods/drivetrain.json"),
    data_file!("mods/electronics.json"),
    data_file!("mods/engine.json"),
    data_file!("mods/forced_induction.json"),
    data_file!("mods/interior.json"),
    data_file!("mods/paint.json"),
    data_file!("mods/protection.json"),
    data_file!("mods/suspension.json"),
    data_file!("mods/tires.json"),
    data_file!("mods/vinyls.json"),
    data_file!("mods/weight.json"),
    data_file!("mods/wheels.json"),
];

impl DataSources {
    /// The repository's data files, compiled into the binary (browser / tests).
    pub fn embedded() -> Self {
        Self {
            prices: data_file!("economy/prices.toml").to_string(),
            stakes: data_file!("economy/stakes.toml").to_string(),
            consequences: data_file!("economy/consequences.toml").to_string(),
            market: data_file!("economy/market.toml").to_string(),
            parts: data_file!("economy/parts.toml").to_string(),
            career: data_file!("economy/career.toml").to_string(),
            lots: data_file!("economy/lots.toml").to_string(),
            mods: MOD_PACKS
                .iter()
                .zip(EMBEDDED_MODS.iter())
                .map(|(n, t)| (n.to_string(), t.to_string()))
                .collect(),
            arcade_mode: data_file!("modes/arcade.toml").to_string(),
            sim_mode: data_file!("modes/sim.toml").to_string(),
            extreme_mode: data_file!("modes/extreme.toml").to_string(),
        }
    }

    /// Read the files from a data root on disk (every `data/mods/*.json` is picked up).
    pub fn from_dir(root: &Path) -> Result<Self, DataError> {
        let read = |rel: &str| -> Result<String, DataError> {
            let p = root.join(rel);
            std::fs::read_to_string(&p).map_err(|source| DataError::Io {
                path: p.display().to_string(),
                source,
            })
        };
        let mods_dir = root.join("mods");
        let mut stems = Vec::new();
        let entries = std::fs::read_dir(&mods_dir).map_err(|source| DataError::Io {
            path: mods_dir.display().to_string(),
            source,
        })?;
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json") {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    stems.push(stem.to_string());
                }
            }
        }
        stems.sort();
        let mut mods = Vec::new();
        for s in stems {
            let text = read(&format!("mods/{s}.json"))?;
            mods.push((s, text));
        }
        Ok(Self {
            prices: read("economy/prices.toml")?,
            stakes: read("economy/stakes.toml")?,
            consequences: read("economy/consequences.toml")?,
            market: read("economy/market.toml")?,
            parts: read("economy/parts.toml")?,
            career: read("economy/career.toml")?,
            lots: read("economy/lots.toml")?,
            mods,
            arcade_mode: read("modes/arcade.toml")?,
            sim_mode: read("modes/sim.toml")?,
            extreme_mode: read("modes/extreme.toml")?,
        })
    }
}

/// Everything an economy operation needs: all tables, the mod catalog and the damage mode
/// tables (for `repair_cost_scale`). Immutable; build once and share.
#[derive(Debug, Clone, PartialEq)]
pub struct Economy {
    /// `prices.toml`.
    pub prices: PriceTables,
    /// `stakes.toml`.
    pub stakes: StakesTables,
    /// `consequences.toml`.
    pub consequences: ConsequenceTables,
    /// `market.toml`.
    pub market: MarketTables,
    /// `lots.toml`.
    pub lots: crate::lots::LotTables,
    /// All mods.
    pub catalog: ModCatalog,
    /// Workshop slots (`parts.toml`).
    pub parts: PartsTable,
    /// Career progression (`career.toml`, see [`crate::progression`]).
    pub progression: ProgressionRules,
    /// Damage mode tables keyed by mode.
    pub modes: BTreeMap<DamageMode, ModeTable>,
}

fn parse_toml<T: serde::de::DeserializeOwned>(name: &str, text: &str) -> Result<T, DataError> {
    toml::from_str(text).map_err(|source| DataError::Toml {
        name: name.to_string(),
        source,
    })
}

fn require_keys<K: Ord + std::fmt::Debug, V>(
    what: &str,
    map: &BTreeMap<K, V>,
    keys: &[K],
) -> Result<(), DataError> {
    for k in keys {
        if !map.contains_key(k) {
            return Err(invalid(what, format!("missing entry for {k:?}")));
        }
    }
    Ok(())
}

fn require_frac(what: &str, v: f32) -> Result<(), DataError> {
    if !(0.0..=1.0).contains(&v) {
        return Err(invalid(what, format!("{v} must be within 0..=1")));
    }
    Ok(())
}

fn require_positive(what: &str, v: f32) -> Result<(), DataError> {
    if !(v.is_finite() && v > 0.0) {
        return Err(invalid(what, format!("{v} must be > 0")));
    }
    Ok(())
}

impl Economy {
    /// Parse and validate from raw sources.
    pub fn from_sources(src: &DataSources) -> Result<Self, DataError> {
        let mode_err = |e: wp_contracts::LoadError| invalid("mode table", e.to_string());
        let arcade = parse_mode_table("modes/arcade.toml", &src.arcade_mode).map_err(mode_err)?;
        let sim = parse_mode_table("modes/sim.toml", &src.sim_mode).map_err(mode_err)?;
        let extreme =
            parse_mode_table("modes/extreme.toml", &src.extreme_mode).map_err(mode_err)?;
        let mut packs = Vec::new();
        for (name, text) in &src.mods {
            let pack: ModPack = serde_json::from_str(text).map_err(|source| DataError::Json {
                name: format!("mods/{name}.json"),
                source,
            })?;
            if &pack.id != name {
                return Err(invalid(
                    format!("mods/{name}.json"),
                    format!("id {:?} must equal the file stem", pack.id),
                ));
            }
            packs.push(pack);
        }
        let econ = Self {
            prices: parse_toml("economy/prices.toml", &src.prices)?,
            stakes: parse_toml("economy/stakes.toml", &src.stakes)?,
            consequences: parse_toml("economy/consequences.toml", &src.consequences)?,
            market: parse_toml("economy/market.toml", &src.market)?,
            lots: parse_toml("economy/lots.toml", &src.lots)?,
            catalog: ModCatalog::from_packs(packs)?,
            parts: PartsTable::parse(&src.parts)?,
            progression: parse_toml("economy/career.toml", &src.career)?,
            modes: [
                (DamageMode::Arcade, arcade),
                (DamageMode::Simulation, sim),
                (DamageMode::Extreme, extreme),
            ]
            .into_iter()
            .collect(),
        };
        econ.validate()?;
        Ok(econ)
    }

    /// The repository data compiled into the binary. Panics only if the checked-in data is
    /// invalid, which the crate's tests rule out.
    pub fn embedded() -> Self {
        Self::from_sources(&DataSources::embedded()).expect("embedded economy data is valid")
    }

    /// Load from a data root on disk (normally `data/`).
    pub fn load(root: &Path) -> Result<Self, DataError> {
        Self::from_sources(&DataSources::from_dir(root)?)
    }

    /// Mode table for a damage mode.
    pub fn mode_table(&self, mode: DamageMode) -> &ModeTable {
        &self.modes[&mode]
    }

    /// Check every table is complete and in range.
    pub fn validate(&self) -> Result<(), DataError> {
        self.parts.validate(&self.catalog)?;
        self.progression.validate()?;
        let k = &self.consequences.career;
        require_frac("consequences.career.heat_carry_frac", k.heat_carry_frac)?;
        if !(k.heat_carry_min >= 0.0 && k.heat_carry_min <= k.heat_carry_max) {
            return Err(invalid(
                "consequences.career",
                "need 0 <= heat_carry_min <= heat_carry_max",
            ));
        }
        if k.ledger_keep == 0 {
            return Err(invalid("consequences.career.ledger_keep", "must be > 0"));
        }
        self.validate_lots()?;
        let p = &self.prices;
        require_positive("prices.labour_hours_per_item", p.labour_hours_per_item)?;
        require_frac("prices.repair_parts_fraction", p.repair_parts_fraction)?;
        for (r, h) in &p.region_labour_hours {
            require_positive(&format!("prices.region_labour_hours.{r:?}"), *h)?;
        }
        for (c, h) in &p.component_labour_hours {
            require_positive(&format!("prices.component_labour_hours.{c:?}"), *h)?;
        }
        let [lo, hi] = p.value_factor_clamp;
        if !(lo > 0.0 && lo <= hi) {
            return Err(invalid("prices.value_factor_clamp", "need 0 < min <= max"));
        }
        require_keys("prices.region_parts", &p.region_parts, &ALL_REGIONS)?;
        require_keys(
            "prices.component_parts",
            &p.component_parts,
            &ALL_COMPONENTS,
        )?;
        require_keys("prices.class", &p.class, &ALL_CLASSES)?;
        for (c, cp) in &p.class {
            let w = format!("prices.class.{c:?}");
            require_positive(&w, cp.reference_value)?;
            require_positive(&w, cp.parts_scale)?;
            require_positive(&w, cp.labour_per_hour)?;
        }
        require_keys(
            "prices.district_labour",
            &p.district_labour,
            &wp_contracts::DistrictId::ALL,
        )?;
        for v in p.district_labour.values() {
            require_positive("prices.district_labour", *v)?;
        }
        require_positive("prices.shop.crooked_markup", p.shop.crooked_markup)?;
        require_positive(
            "prices.shop.crooked_wanted_markup",
            p.shop.crooked_wanted_markup,
        )?;
        require_frac("prices.scrap.mods_frac", p.scrap.mods_frac)?;
        require_frac(
            "prices.scrap.min_frac + health_frac",
            p.scrap.min_frac + p.scrap.health_frac,
        )?;
        if p.insurance.is_empty() {
            return Err(invalid("prices.insurance", "need at least one tier"));
        }
        for (id, t) in &p.insurance {
            require_frac(&format!("prices.insurance.{id}.coverage"), t.coverage)?;
            require_positive(
                &format!("prices.insurance.{id}.premium"),
                t.premium_per_day_frac,
            )?;
        }

        let s = &self.stakes;
        if s.placement_share.is_empty() {
            return Err(invalid("stakes.placement_share", "empty"));
        }
        for w in s.placement_share.windows(2) {
            if w[1] > w[0] {
                return Err(invalid("stakes.placement_share", "must be non-increasing"));
            }
        }
        require_keys("stakes.day_phase", &s.day_phase, &ALL_PHASES)?;
        require_keys("stakes.class_purse", &s.class_purse, &ALL_CLASSES)?;
        require_keys("stakes.wanted_purse", &s.wanted_purse, &ALL_LEVELS)?;
        require_keys("stakes.mode_purse", &s.mode_purse, &ALL_MODES)?;
        require_keys("stakes.race_types", &s.race_types, &RaceType::ALL)?;
        require_keys(
            "stakes.bounty.race_won_while_wanted",
            &s.bounty.race_won_while_wanted,
            &ALL_LEVELS,
        )?;
        require_keys("stakes.bounty.escape", &s.bounty.escape, &ALL_LEVELS)?;
        require_keys(
            "stakes.bounty.escape_rep",
            &s.bounty.escape_rep,
            &ALL_LEVELS,
        )?;
        require_keys(
            "stakes.bounty.escape_cash",
            &s.bounty.escape_cash,
            &ALL_LEVELS,
        )?;
        require_keys(
            "stakes.bounty.cop_base_reward",
            &s.bounty.cop_base_reward,
            &ALL_LEVELS,
        )?;
        require_frac("stakes.bounty.cop_share", s.bounty.cop_share)?;
        require_frac("stakes.bounty.assist_share", s.bounty.assist_share)?;
        let kind_keys: Vec<String> = wp_contracts::RaceKind::KEYS
            .iter()
            .map(|k| k.to_string())
            .collect();
        require_keys("stakes.race_kinds", &s.race_kinds, &kind_keys)?;
        let legality_keys: Vec<String> = legality_keys().iter().map(|k| k.to_string()).collect();
        require_keys("stakes.legality", &s.legality, &legality_keys)?;
        for (id, k) in &s.race_kinds {
            let w = format!("stakes.race_kinds.{id}");
            if !kind_keys.contains(id) {
                return Err(invalid(w, "not a RaceKind key"));
            }
            require_positive(&w, k.purse)?;
            if k.entry < 0.0 {
                return Err(invalid(w, "entry must be >= 0"));
            }
        }
        for (id, l) in &s.legality {
            let w = format!("stakes.legality.{id}");
            if !legality_keys.contains(id) {
                return Err(invalid(w, "not a Legality key"));
            }
            require_positive(&w, l.purse)?;
            if l.entry < 0.0 {
                return Err(invalid(w, "entry must be >= 0"));
            }
        }
        for (id, rt) in &s.race_types {
            let w = format!("stakes.race_types.{id:?}");
            require_positive(&w, rt.base_per_km)?;
            require_positive(&w, rt.max_km)?;
            if rt.entry_frac < 0.0 {
                return Err(invalid(w, "entry_frac must be >= 0"));
            }
        }

        let c = &self.consequences;
        require_keys("consequences.fine_by_level", &c.fine_by_level, &ALL_LEVELS)?;
        require_keys(
            "consequences.rep_loss_by_level",
            &c.rep_loss_by_level,
            &ALL_LEVELS,
        )?;
        require_keys(
            "consequences.impound.base_fee",
            &c.impound.base_fee,
            &ALL_CLASSES,
        )?;
        require_keys(
            "consequences.impound.per_day",
            &c.impound.per_day,
            &ALL_CLASSES,
        )?;
        require_keys("consequences.mode", &c.mode, &ALL_MODES)?;
        require_keys(
            "consequences.cop_xp.capture",
            &c.cop_xp.capture,
            &ALL_LEVELS,
        )?;
        require_keys("consequences.cop_xp.assist", &c.cop_xp.assist, &ALL_LEVELS)?;
        require_frac("consequences.bail_bounty_frac", c.bail_bounty_frac)?;
        require_frac(
            "consequences.impound.auction_return_frac",
            c.impound.auction_return_frac,
        )?;
        for (m, mc) in &c.mode {
            require_frac(
                &format!("consequences.mode.{m:?}.seizure_chance"),
                mc.seizure_chance,
            )?;
        }
        for w in c.cop_rank_thresholds.windows(2) {
            if w[1] <= w[0] {
                return Err(invalid(
                    "consequences.cop_rank_thresholds",
                    "must be strictly increasing",
                ));
            }
        }

        let m = &self.market;
        require_keys("market.class_bands", &m.class_bands, &ALL_CLASSES)?;
        require_keys("market.condition", &m.condition, &Condition::ALL)?;
        for (cl, [lo, hi]) in &m.class_bands {
            if lo > hi || *lo < 0 {
                return Err(invalid(
                    format!("market.class_bands.{cl:?}"),
                    "need 0 <= min <= max",
                ));
            }
        }
        require_frac("market.dealer_buy_frac", m.dealer_buy_frac)?;
        require_frac("market.mod_resale_frac", m.mod_resale_frac)?;
        require_frac("market.mod_value_frac", m.mod_value_frac)?;
        require_frac("market.damaged_value_floor", m.damaged_value_floor)?;
        if !m.tire_grip.contains_key("street") {
            return Err(invalid("market.tire_grip", "missing \"street\""));
        }
        let ci = &m.class_index;
        if ci.per_w_per_kg < 0.0 || ci.per_lateral_g < 0.0 || ci.per_brake_g < 0.0 {
            return Err(invalid(
                "market.class_index",
                "coefficients must be >= 0 (index must be monotone)",
            ));
        }
        if ci.brackets.is_empty() || ci.brackets[0].min > ci.min {
            return Err(invalid(
                "market.class_index.brackets",
                "first bracket must start at or below min",
            ));
        }
        for w in ci.brackets.windows(2) {
            if w[1].min <= w[0].min {
                return Err(invalid("market.class_index.brackets", "must be ascending"));
            }
        }

        // Every tire id a mod sets must have a grip factor.
        for md in self.catalog.iter() {
            for t in md.tire_ids() {
                if !m.tire_grip.contains_key(t) {
                    return Err(invalid(
                        format!("mod {}", md.id),
                        format!("tire id {t:?} has no market.tire_grip entry"),
                    ));
                }
            }
        }
        Ok(())
    }
}
