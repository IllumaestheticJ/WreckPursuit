//! Accounts, the auditable event log and account persistence.
//!
//! Every change to a [`PlayerAccount`] happens inside a transaction ([`transact`]) that works on a
//! copy and writes it back only if the whole operation succeeds. Each individual change is
//! recorded as an [`EconomyEvent`] with a typed [`EventReason`], so the event log fully explains
//! the account (prompt 07 forwards the events to the client feed via
//! [`EconomyEvent::to_sim_event`]).
//!
//! Cash rule: a voluntary spend (purchases, repairs, entry fees, retrieval fees...) must be
//! affordable from cash on hand. Only *penalties* (fines, bail, towing) may push cash below zero;
//! negative cash is **debt**, and debt blocks every voluntary spend plus any race with a buy-in
//! until it is paid off by winnings or sales.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::WantedLevel;
use wp_contracts::{
    HandlingEffects, PlayerId, RepairEstimate, SimEvent, VehicleClass, ViolationKind,
};

use crate::error::EconomyError;

/// Whole in-game currency units. Negative cash = debt.
pub type Money = i64;

/// Identifier of one owned car, unique within an account (contract type since v0.3.0).
pub use wp_contracts::CarUid;

/// Where an owned car currently is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CarLocation {
    /// In the player's garage, usable.
    #[default]
    Garage,
    /// In the police pound since in-game day `since_day`. Retrieval fee grows daily; after
    /// `impound.auction_after_days` the car is auctioned and lost.
    Impound {
        /// In-game day the car was impounded.
        since_day: u32,
        /// Wanted level at capture (informational, shown in the UI).
        level: WantedLevel,
    },
}

/// Persistent damage state of an owned car between sessions.
///
/// `DriveDamaged` keeps this as-is: the car returns to the street with the same lattice state
/// and [`HandlingEffects`] (a car that drives crooked because you could not afford alignment).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DamageSnapshot {
    /// Global health 0..=1 as reported by the damage system (1 = pristine).
    pub health: f32,
    /// The car is a total loss (engine killed). It must be repaired or scrapped before driving.
    pub total_loss: bool,
    /// Damage → handling effects to re-apply when the car is spawned.
    pub handling: HandlingEffects,
    /// Repair line items not yet paid for.
    pub outstanding: RepairEstimate,
    /// Opaque lattice state from `wp_damage` (node positions, plastic strain, broken beams).
    /// Empty = pristine lattice. The economy never interprets it.
    pub lattice_state: Vec<u8>,
    /// Some of this damage was sustained while wanted (insurance will not pay for it).
    pub sustained_while_wanted: bool,
}

impl DamageSnapshot {
    /// An undamaged car.
    pub fn pristine() -> Self {
        Self {
            health: 1.0,
            total_loss: false,
            handling: HandlingEffects::NEUTRAL,
            outstanding: RepairEstimate::default(),
            lattice_state: Vec::new(),
            sustained_while_wanted: false,
        }
    }

    /// True when there is nothing to repair.
    pub fn is_pristine(&self) -> bool {
        self.outstanding.lines.is_empty() && !self.total_loss
    }

    /// Whether the car can be driven at all (not a total loss, engine alive, all wheels on).
    pub fn is_drivable(&self) -> bool {
        !self.total_loss && !self.handling.engine_dead && !self.handling.wheel_lost.contains(&true)
    }
}

impl Default for DamageSnapshot {
    fn default() -> Self {
        Self::pristine()
    }
}

/// An insurance policy on one car.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InsurancePolicy {
    /// Tier id from `prices.toml` `[insurance.<tier>]`.
    pub tier: String,
    /// Premium charged every in-game day.
    pub premium_per_day: Money,
    /// Fraction of the repair bill (after deductible) the insurer pays, 0..=1.
    pub coverage: f32,
    /// Amount the player pays before coverage applies, per claim.
    pub deductible: Money,
    /// In-game day the policy started.
    pub since_day: u32,
}

/// One car in a player's garage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OwnedCar {
    /// Account-unique id.
    pub uid: CarUid,
    /// `VehicleDef::id` of the base car.
    pub def_id: String,
    /// Gameplay class of the base car.
    pub class: VehicleClass,
    /// `VehicleDef::base_price` of the base car.
    pub base_price: u32,
    /// Installed mod ids, kept sorted.
    pub mods: Vec<String>,
    /// Persistent damage.
    pub damage: DamageSnapshot,
    /// Where the car is.
    pub location: CarLocation,
    /// Insurance policy, if any.
    pub insurance: Option<InsurancePolicy>,
}

impl OwnedCar {
    /// True when the car is in the garage and drivable.
    pub fn is_usable(&self) -> bool {
        self.location == CarLocation::Garage && self.damage.is_drivable()
    }
}

/// Per-kind traffic violation counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ViolationCounts {
    /// Speeding.
    pub speeding: u32,
    /// Red lights run.
    pub red_light: u32,
    /// Wrong-way driving.
    pub wrong_way: u32,
    /// Collisions with road users / property.
    pub collision: u32,
    /// Reckless driving / near misses.
    pub reckless: u32,
}

impl ViolationCounts {
    /// Count one violation event.
    pub fn record(&mut self, kind: &ViolationKind) {
        match kind {
            ViolationKind::Speeding { .. } => self.speeding += 1,
            ViolationKind::RedLight => self.red_light += 1,
            ViolationKind::WrongWay => self.wrong_way += 1,
            ViolationKind::Collision => self.collision += 1,
            ViolationKind::Reckless => self.reckless += 1,
        }
    }

    /// Add another set of counters.
    pub fn add(&mut self, o: &ViolationCounts) {
        self.speeding += o.speeding;
        self.red_light += o.red_light;
        self.wrong_way += o.wrong_way;
        self.collision += o.collision;
        self.reckless += o.reckless;
    }

    /// Violations other than speeding (these void the clean-race bonus).
    pub fn non_speeding(&self) -> u32 {
        self.red_light + self.wrong_way + self.collision + self.reckless
    }
}

/// Police history of a racer, used to scale fines and shown on the rap sheet.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HeatHistory {
    /// Times captured.
    pub captures: u32,
    /// Pursuits escaped.
    pub escapes: u32,
    /// Highest wanted level ever reached.
    pub max_wanted: WantedLevel,
    /// Lifetime violations reported to the economy.
    pub violations: ViolationCounts,
    /// In-game day of the last capture.
    pub last_capture_day: Option<u32>,
}

/// Lifetime statistics (UI and tuning; not used in pricing).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AccountStats {
    /// Races entered.
    pub races_entered: u32,
    /// Races won.
    pub races_won: u32,
    /// Top-3 finishes.
    pub podiums: u32,
    /// Races finished with no contact and no non-speeding violations.
    pub clean_races: u32,
    /// Total cash credited.
    pub total_earned: Money,
    /// Total cash debited.
    pub total_spent: Money,
    /// Total spent on repairs (player share).
    pub repairs_paid: Money,
    /// Total fines + bail + towing charged.
    pub penalties_charged: Money,
    /// Cars lost (seized, destroyed, auctioned, pink-slipped away).
    pub cars_lost: u32,
    /// Cars scrapped by the player.
    pub cars_scrapped: u32,
    /// Captures made as a cop.
    pub cop_captures: u32,
    /// Capture assists as a cop.
    pub cop_assists: u32,
}

/// Persistent per-player economy state. Owned by the server; clients see it through events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerAccount {
    /// Owner.
    pub player: PlayerId,
    /// Cash. Negative only through penalties (debt).
    pub cash: Money,
    /// Street reputation (never below 0).
    pub rep: i64,
    /// Outstanding bounty carried across sessions (police set priority from it).
    pub bounty: u32,
    /// Police career XP (see [`crate::Economy::cop_level`]).
    pub cop_rank: u32,
    /// Owned cars.
    pub garage: Vec<OwnedCar>,
    /// Car the player drives by default.
    pub active_car: Option<CarUid>,
    /// Police history.
    pub heat_history: HeatHistory,
    /// Lifetime stats.
    pub stats: AccountStats,
    /// Current in-game day (advanced by [`crate::Economy::end_of_day`], never by wall-clock).
    pub day: u32,
    /// Next car uid to allocate.
    pub next_car_uid: u32,
    /// Sequence number of the next event.
    pub next_event_seq: u64,
}

impl PlayerAccount {
    /// An empty account with no cash and no events (use [`crate::Economy::open_account`] for a
    /// real one with starting cash).
    pub fn empty(player: PlayerId) -> Self {
        Self {
            player,
            cash: 0,
            rep: 0,
            bounty: 0,
            cop_rank: 0,
            garage: Vec::new(),
            active_car: None,
            heat_history: HeatHistory::default(),
            stats: AccountStats::default(),
            day: 0,
            next_car_uid: 1,
            next_event_seq: 0,
        }
    }

    /// Outstanding debt (0 when cash ≥ 0).
    pub fn debt(&self) -> Money {
        (-self.cash).max(0)
    }

    /// True when cash is negative.
    pub fn in_debt(&self) -> bool {
        self.cash < 0
    }

    /// Look up an owned car.
    pub fn car(&self, uid: CarUid) -> Option<&OwnedCar> {
        self.garage.iter().find(|c| c.uid == uid)
    }

    /// Look up an owned car mutably.
    pub fn car_mut(&mut self, uid: CarUid) -> Option<&mut OwnedCar> {
        self.garage.iter_mut().find(|c| c.uid == uid)
    }

    /// Look up an owned car or fail with [`EconomyError::UnknownCar`].
    pub fn try_car(&self, uid: CarUid) -> Result<&OwnedCar, EconomyError> {
        self.car(uid).ok_or(EconomyError::UnknownCar(uid))
    }

    /// True if any car is in the garage and drivable.
    pub fn has_usable_car(&self) -> bool {
        self.garage.iter().any(OwnedCar::is_usable)
    }
}

/// Activity a player cop earned or lost XP for (contract type since v0.3.0).
pub use wp_contracts::CopXpReason;

/// Why an account changed (ledger / persistence form). Converts to the contract
/// `wp_contracts::EconomyReason` with [`EventReason::to_contract`]; the `u16` codes are the same.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EventReason {
    /// Account created with starting cash.
    AccountOpened,
    /// Race buy-in paid.
    RaceEntryFee {
        /// Race type id.
        race: String,
    },
    /// Race purse share paid.
    RacePurse {
        /// Race type id.
        race: String,
        /// Finishing position (1 = win).
        placement: u8,
    },
    /// Clean-race bonus (no contact, no violations beyond speeding).
    CleanRaceBonus,
    /// Reputation from a race result.
    RaceRep {
        /// Finishing position (0 = DNF).
        placement: u8,
    },
    /// Won the loser's car in a pink-slip race.
    PinkSlipWon {
        /// Car won (base car id).
        def_id: String,
    },
    /// Lost a car in a pink-slip race.
    PinkSlipLost {
        /// Car lost (base car id).
        def_id: String,
    },
    /// Repair paid at a shop.
    RepairPaid {
        /// Line items repaired.
        lines: u32,
        /// Labour multiplier applied (district × shop).
        labour_multiplier: f32,
    },
    /// Insurance paid part of a repair bill (informational; the bill was already net).
    InsuranceClaim {
        /// Amount covered by the insurer.
        covered: Money,
    },
    /// Insurance refused a claim (damage sustained while wanted).
    InsuranceClaimDenied,
    /// Player chose to drive the car damaged.
    DriveDamaged,
    /// Car scrapped for salvage.
    CarScrapped {
        /// Base car id.
        def_id: String,
    },
    /// A legit shop reported a wanted car to the police (wp_police should add heat).
    ShopReported {
        /// District of the shop.
        district: String,
        /// Wanted level at the time.
        level: WantedLevel,
    },
    /// Fine at capture.
    Fine {
        /// Wanted level at capture.
        level: WantedLevel,
    },
    /// Bail posted at capture.
    Bail,
    /// Reputation lost at capture.
    CaptureRepLoss {
        /// Wanted level at capture.
        level: WantedLevel,
    },
    /// Bounty cleared at capture (police heat resets).
    BountyCleared,
    /// Bounty grew (escape, race won while wanted).
    BountyGained,
    /// Towing charge (wrecked or undrivable car).
    Towing,
    /// Car moved to the pound.
    CarImpounded,
    /// Car retrieved from the pound.
    ImpoundRetrieved {
        /// Days spent in the pound.
        days: u32,
    },
    /// Car auctioned by the pound (lost).
    CarAuctioned {
        /// Base car id.
        def_id: String,
    },
    /// Car seized by the police (Simulation, high wanted level).
    CarSeized {
        /// Base car id.
        def_id: String,
    },
    /// Car destroyed: total loss while wanted in Simulation.
    CarDestroyed {
        /// Base car id.
        def_id: String,
    },
    /// Escaped a pursuit.
    Escape {
        /// Wanted level escaped from.
        level: WantedLevel,
    },
    /// Cop bounty reward for a capture or assist.
    CopReward {
        /// Wanted level of the captured racer.
        level: WantedLevel,
    },
    /// Cop XP change.
    CopXp {
        /// Why.
        reason: CopXpReason,
    },
    /// Paid an informant; wp_police should reduce heat by `heat_reduction`.
    InformantTip {
        /// Tier id.
        tier: String,
        /// Heat points to remove (0–100 scale).
        heat_reduction: f32,
    },
    /// Bought a car.
    CarPurchased {
        /// Base car id.
        def_id: String,
    },
    /// Sold a car to the dealer.
    CarSold {
        /// Base car id.
        def_id: String,
    },
    /// Issued the economy-floor beater (always affordable).
    FloorCarIssued {
        /// Base car id.
        def_id: String,
    },
    /// Mod installed.
    ModInstalled {
        /// Mod id.
        mod_id: String,
    },
    /// Mod removed (refund = resale value).
    ModRemoved {
        /// Mod id.
        mod_id: String,
    },
    /// Insurance policy bought.
    InsurancePurchased {
        /// Tier id.
        tier: String,
    },
    /// Daily insurance premium.
    InsurancePremium,
    /// Policy lapsed because the premium could not be paid.
    InsuranceLapsed,
    /// Policy voided: captured while wanted.
    InsuranceVoided,
    /// Damage report recorded at the end of a session.
    DamageRecorded {
        /// Global health reported.
        health: f32,
        /// Total loss.
        total_loss: bool,
    },
    /// Manual adjustment by an admin / test harness.
    Adjustment {
        /// Free-text note.
        note: String,
    },
}

impl EventReason {
    /// Stable `u16` discriminator (= `wp_contracts::EconomyReason::kind_code`).
    pub fn kind_code(&self) -> u16 {
        use EventReason::*;
        match self {
            AccountOpened => 1,
            RaceEntryFee { .. } => 10,
            RacePurse { .. } => 11,
            CleanRaceBonus => 12,
            RaceRep { .. } => 13,
            PinkSlipWon { .. } => 14,
            PinkSlipLost { .. } => 15,
            RepairPaid { .. } => 20,
            InsuranceClaim { .. } => 21,
            InsuranceClaimDenied => 22,
            DriveDamaged => 23,
            CarScrapped { .. } => 24,
            ShopReported { .. } => 25,
            DamageRecorded { .. } => 26,
            Fine { .. } => 30,
            Bail => 31,
            CaptureRepLoss { .. } => 32,
            BountyCleared => 33,
            BountyGained => 34,
            Towing => 35,
            CarImpounded => 36,
            ImpoundRetrieved { .. } => 37,
            CarAuctioned { .. } => 38,
            CarSeized { .. } => 39,
            CarDestroyed { .. } => 40,
            Escape { .. } => 41,
            CopReward { .. } => 50,
            CopXp { .. } => 51,
            InformantTip { .. } => 52,
            CarPurchased { .. } => 60,
            CarSold { .. } => 61,
            FloorCarIssued { .. } => 62,
            ModInstalled { .. } => 63,
            ModRemoved { .. } => 64,
            InsurancePurchased { .. } => 70,
            InsurancePremium => 71,
            InsuranceLapsed => 72,
            InsuranceVoided => 73,
            Adjustment { .. } => 999,
        }
    }

    /// Penalties are the only charges allowed to push cash below zero (debt).
    pub fn is_penalty(&self) -> bool {
        matches!(
            self,
            EventReason::Fine { .. } | EventReason::Bail | EventReason::Towing
        )
    }
}

/// One auditable change to an account.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EconomyEvent {
    /// Account owner.
    pub player: PlayerId,
    /// Per-account sequence number (0, 1, 2, ... with no gaps).
    pub seq: u64,
    /// In-game day.
    pub day: u32,
    /// Why.
    pub reason: EventReason,
    /// Car concerned, if any.
    pub car: Option<CarUid>,
    /// Cash change.
    pub cash_delta: Money,
    /// Reputation change (as applied, after clamping at 0).
    pub rep_delta: i64,
    /// Bounty change.
    pub bounty_delta: i64,
    /// Cop XP change (as applied).
    pub cop_xp_delta: i64,
    /// Cash after this event.
    pub cash_after: Money,
}

impl EconomyEvent {
    /// The contract form (v0.3.0): `player` becomes the durable `AccountId` (same number until
    /// the economy switches to meta's account ids), reasons are renamed
    /// (`RaceEntryFee` → `StakeCharged`, `RacePurse` → `RacePayout`).
    pub fn to_contract(&self) -> wp_contracts::EconomyEvent {
        wp_contracts::EconomyEvent {
            account: wp_contracts::AccountId(self.player.0),
            seq: self.seq,
            day: self.day,
            reason: self.reason.to_contract(),
            car: self.car,
            cash_delta: self.cash_delta,
            rep_delta: self.rep_delta,
            bounty_delta: self.bounty_delta,
            cop_xp_delta: self.cop_xp_delta,
            cash_after: self.cash_after,
        }
    }

    /// Inverse of [`EconomyEvent::to_contract`].
    pub fn from_contract(e: &wp_contracts::EconomyEvent) -> Self {
        Self {
            player: PlayerId(e.account.0),
            seq: e.seq,
            day: e.day,
            reason: EventReason::from_contract(&e.reason),
            car: e.car,
            cash_delta: e.cash_delta,
            rep_delta: e.rep_delta,
            bounty_delta: e.bounty_delta,
            cop_xp_delta: e.cop_xp_delta,
            cash_after: e.cash_after,
        }
    }

    /// Wrap as the contract event `SimEvent::Economy` for the net layer / client feed.
    pub fn to_sim_event(&self) -> SimEvent {
        SimEvent::Economy(self.to_contract())
    }

    /// Decode a `SimEvent::Economy`. Returns `None` for other event types.
    pub fn from_sim_event(ev: &SimEvent) -> Option<Self> {
        match ev {
            SimEvent::Economy(e) => Some(Self::from_contract(e)),
            _ => None,
        }
    }
}

impl EventReason {
    /// Contract form of this reason. Race type ids that `wp_contracts::RaceType` does not know
    /// (only possible for hand-edited records) become an `Adjustment` naming them.
    pub fn to_contract(&self) -> wp_contracts::EconomyReason {
        use wp_contracts::{EconomyReason as C, RaceType};
        use EventReason as R;
        let unknown_race = |race: &str| C::Adjustment {
            note: format!("unknown race type {race}"),
        };
        match self.clone() {
            R::AccountOpened => C::AccountOpened,
            R::RaceEntryFee { race } => match RaceType::from_id(&race) {
                Some(race) => C::StakeCharged { race },
                None => unknown_race(&race),
            },
            R::RacePurse { race, placement } => match RaceType::from_id(&race) {
                Some(race) => C::RacePayout { race, placement },
                None => unknown_race(&race),
            },
            R::CleanRaceBonus => C::CleanRaceBonus,
            R::RaceRep { placement } => C::RaceRep { placement },
            R::PinkSlipWon { def_id } => C::PinkSlipWon { def_id },
            R::PinkSlipLost { def_id } => C::PinkSlipLost { def_id },
            R::RepairPaid {
                lines,
                labour_multiplier,
            } => C::RepairPaid {
                lines,
                labour_multiplier,
            },
            R::InsuranceClaim { covered } => C::InsuranceClaim { covered },
            R::InsuranceClaimDenied => C::InsuranceClaimDenied,
            R::DriveDamaged => C::DriveDamaged,
            R::CarScrapped { def_id } => C::CarScrapped { def_id },
            R::ShopReported { district, level } => C::ShopReported { district, level },
            R::Fine { level } => C::Fine { level },
            R::Bail => C::Bail,
            R::CaptureRepLoss { level } => C::CaptureRepLoss { level },
            R::BountyCleared => C::BountyCleared,
            R::BountyGained => C::BountyGained,
            R::Towing => C::Towing,
            R::CarImpounded => C::CarImpounded,
            R::ImpoundRetrieved { days } => C::ImpoundRetrieved { days },
            R::CarAuctioned { def_id } => C::CarAuctioned { def_id },
            R::CarSeized { def_id } => C::CarSeized { def_id },
            R::CarDestroyed { def_id } => C::CarDestroyed { def_id },
            R::Escape { level } => C::Escape { level },
            R::CopReward { level } => C::CopReward { level },
            R::CopXp { reason } => C::CopXp { reason },
            R::InformantTip {
                tier,
                heat_reduction,
            } => C::InformantTip {
                tier,
                heat_reduction,
            },
            R::CarPurchased { def_id } => C::CarPurchased { def_id },
            R::CarSold { def_id } => C::CarSold { def_id },
            R::FloorCarIssued { def_id } => C::FloorCarIssued { def_id },
            R::ModInstalled { mod_id } => C::ModInstalled { mod_id },
            R::ModRemoved { mod_id } => C::ModRemoved { mod_id },
            R::InsurancePurchased { tier } => C::InsurancePurchased { tier },
            R::InsurancePremium => C::InsurancePremium,
            R::InsuranceLapsed => C::InsuranceLapsed,
            R::InsuranceVoided => C::InsuranceVoided,
            R::DamageRecorded { health, total_loss } => C::DamageRecorded { health, total_loss },
            R::Adjustment { note } => C::Adjustment { note },
        }
    }

    /// Inverse of [`EventReason::to_contract`].
    pub fn from_contract(c: &wp_contracts::EconomyReason) -> Self {
        use wp_contracts::EconomyReason as C;
        use EventReason as R;
        match c.clone() {
            C::AccountOpened => R::AccountOpened,
            C::StakeCharged { race } => R::RaceEntryFee {
                race: race.id().into(),
            },
            C::RacePayout { race, placement } => R::RacePurse {
                race: race.id().into(),
                placement,
            },
            C::CleanRaceBonus => R::CleanRaceBonus,
            C::RaceRep { placement } => R::RaceRep { placement },
            C::PinkSlipWon { def_id } => R::PinkSlipWon { def_id },
            C::PinkSlipLost { def_id } => R::PinkSlipLost { def_id },
            C::RepairPaid {
                lines,
                labour_multiplier,
            } => R::RepairPaid {
                lines,
                labour_multiplier,
            },
            C::InsuranceClaim { covered } => R::InsuranceClaim { covered },
            C::InsuranceClaimDenied => R::InsuranceClaimDenied,
            C::DriveDamaged => R::DriveDamaged,
            C::CarScrapped { def_id } => R::CarScrapped { def_id },
            C::ShopReported { district, level } => R::ShopReported { district, level },
            C::DamageRecorded { health, total_loss } => R::DamageRecorded { health, total_loss },
            C::Fine { level } => R::Fine { level },
            C::Bail => R::Bail,
            C::CaptureRepLoss { level } => R::CaptureRepLoss { level },
            C::BountyCleared => R::BountyCleared,
            C::BountyGained => R::BountyGained,
            C::Towing => R::Towing,
            C::CarImpounded => R::CarImpounded,
            C::ImpoundRetrieved { days } => R::ImpoundRetrieved { days },
            C::CarAuctioned { def_id } => R::CarAuctioned { def_id },
            C::CarSeized { def_id } => R::CarSeized { def_id },
            C::CarDestroyed { def_id } => R::CarDestroyed { def_id },
            C::Escape { level } => R::Escape { level },
            C::CopReward { level } => R::CopReward { level },
            C::CopXp { reason } => R::CopXp { reason },
            C::InformantTip {
                tier,
                heat_reduction,
            } => R::InformantTip {
                tier,
                heat_reduction,
            },
            C::CarPurchased { def_id } => R::CarPurchased { def_id },
            C::CarSold { def_id } => R::CarSold { def_id },
            C::FloorCarIssued { def_id } => R::FloorCarIssued { def_id },
            C::ModInstalled { mod_id } => R::ModInstalled { mod_id },
            C::ModRemoved { mod_id } => R::ModRemoved { mod_id },
            C::InsurancePurchased { tier } => R::InsurancePurchased { tier },
            C::InsurancePremium => R::InsurancePremium,
            C::InsuranceLapsed => R::InsuranceLapsed,
            C::InsuranceVoided => R::InsuranceVoided,
            C::Adjustment { note } => R::Adjustment { note },
        }
    }
}

/// Changes posted in one ledger entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Delta {
    pub cash: Money,
    pub rep: i64,
    pub bounty: i64,
    pub cop_xp: i64,
}

impl Delta {
    pub fn cash(cash: Money) -> Self {
        Self {
            cash,
            ..Self::default()
        }
    }
}

/// Working copy of an account plus the events posted so far.
pub(crate) struct Txn {
    pub acct: PlayerAccount,
    pub events: Vec<EconomyEvent>,
}

impl Txn {
    /// Post a ledger entry. Negative cash deltas must be affordable unless the reason is a
    /// penalty. Rep is clamped at 0 and bounty / cop XP saturate; events record applied deltas.
    pub fn post(
        &mut self,
        reason: EventReason,
        car: Option<CarUid>,
        d: Delta,
    ) -> Result<(), EconomyError> {
        let a = &mut self.acct;
        if d.cash < 0 && !reason.is_penalty() && a.cash + d.cash < 0 {
            return Err(EconomyError::InsufficientFunds {
                needed: -d.cash,
                available: a.cash,
            });
        }
        a.cash += d.cash;
        if d.cash > 0 {
            a.stats.total_earned += d.cash;
        } else {
            a.stats.total_spent -= d.cash;
        }
        if reason.is_penalty() {
            a.stats.penalties_charged -= d.cash.min(0);
        }
        let rep_before = a.rep;
        a.rep = (a.rep + d.rep).max(0);
        let bounty_before = a.bounty as i64;
        a.bounty = (bounty_before + d.bounty).clamp(0, u32::MAX as i64) as u32;
        let xp_before = a.cop_rank as i64;
        a.cop_rank = (xp_before + d.cop_xp).clamp(0, u32::MAX as i64) as u32;
        let ev = EconomyEvent {
            player: a.player,
            seq: a.next_event_seq,
            day: a.day,
            reason,
            car,
            cash_delta: d.cash,
            rep_delta: a.rep - rep_before,
            bounty_delta: a.bounty as i64 - bounty_before,
            cop_xp_delta: a.cop_rank as i64 - xp_before,
            cash_after: a.cash,
        };
        a.next_event_seq += 1;
        self.events.push(ev);
        Ok(())
    }

    /// Voluntary purchase: fails with `InsufficientFunds` if not affordable.
    pub fn spend(
        &mut self,
        amount: Money,
        reason: EventReason,
        car: Option<CarUid>,
    ) -> Result<(), EconomyError> {
        debug_assert!(amount >= 0);
        self.post(reason, car, Delta::cash(-amount))
    }

    /// Penalty charge (fines, bail, towing): may create debt. `reason` must be a penalty.
    pub fn penalty(&mut self, amount: Money, reason: EventReason, car: Option<CarUid>) {
        debug_assert!(reason.is_penalty() && amount >= 0);
        self.post(reason, car, Delta::cash(-amount))
            .expect("penalties never fail");
    }

    /// Credit cash.
    pub fn credit(&mut self, amount: Money, reason: EventReason, car: Option<CarUid>) {
        debug_assert!(amount >= 0);
        self.post(reason, car, Delta::cash(amount))
            .expect("credits never fail");
    }

    /// Record an event with no cash change (rep / bounty / XP or purely informational).
    pub fn note(&mut self, reason: EventReason, car: Option<CarUid>, d: Delta) {
        debug_assert!(d.cash == 0);
        self.post(reason, car, d)
            .expect("non-cash events never fail");
    }

    /// Mutable access to a car in the working copy.
    pub fn car_mut(&mut self, uid: CarUid) -> Result<&mut OwnedCar, EconomyError> {
        self.acct.car_mut(uid).ok_or(EconomyError::UnknownCar(uid))
    }

    /// Remove a car from the garage (sold, scrapped, lost) and clear it as active car.
    pub fn remove_car(&mut self, uid: CarUid) -> Result<OwnedCar, EconomyError> {
        let idx = self
            .acct
            .garage
            .iter()
            .position(|c| c.uid == uid)
            .ok_or(EconomyError::UnknownCar(uid))?;
        if self.acct.active_car == Some(uid) {
            self.acct.active_car = None;
        }
        Ok(self.acct.garage.remove(idx))
    }

    /// Add a car to the garage with a fresh uid; becomes the active car if none is set.
    pub fn add_car(&mut self, mut car: OwnedCar) -> CarUid {
        let uid = CarUid(self.acct.next_car_uid);
        self.acct.next_car_uid += 1;
        car.uid = uid;
        car.mods.sort();
        self.acct.garage.push(car);
        if self.acct.active_car.is_none() {
            self.acct.active_car = Some(uid);
        }
        uid
    }
}

/// Run `f` on a copy of `acct`. On success the copy replaces `acct` and the posted events are
/// returned; on error `acct` is untouched.
pub(crate) fn transact<T>(
    acct: &mut PlayerAccount,
    f: impl FnOnce(&mut Txn) -> Result<T, EconomyError>,
) -> Result<(T, Vec<EconomyEvent>), EconomyError> {
    let mut txn = Txn {
        acct: acct.clone(),
        events: Vec::new(),
    };
    let out = f(&mut txn)?;
    *acct = txn.acct;
    Ok((out, txn.events))
}

/// Result of an economy operation: its value plus the events it posted.
pub type EconResult<T> = Result<(T, Vec<EconomyEvent>), EconomyError>;

/// Persistence errors.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum StoreError {
    /// Storage backend failure.
    #[error("storage backend: {0}")]
    Backend(String),
    /// Stored data could not be decoded.
    #[error("corrupt stored data: {0}")]
    Corrupt(String),
    /// Events do not continue the stored sequence (lost update / double commit).
    #[error("event sequence conflict: expected seq {expected}, got {got}")]
    SeqConflict {
        /// Next sequence number the store expected.
        expected: u64,
        /// Sequence number received.
        got: u64,
    },
}

/// Storage backend for accounts and their event logs. The server uses the SQLite store
/// (`native` feature); tests and the browser use [`InMemoryStore`].
pub trait AccountStore {
    /// Load an account, `None` if the player has none yet.
    fn load(&self, player: PlayerId) -> Result<Option<PlayerAccount>, StoreError>;
    /// Atomically persist `account` and append `events` (which must continue the stored
    /// sequence with no gaps and end at `account.next_event_seq - 1`).
    fn commit(
        &mut self,
        account: &PlayerAccount,
        events: &[EconomyEvent],
    ) -> Result<(), StoreError>;
    /// Events with `seq >= from_seq`, in order.
    fn events_since(
        &self,
        player: PlayerId,
        from_seq: u64,
    ) -> Result<Vec<EconomyEvent>, StoreError>;
}

/// Check that `events` continue a log whose next expected sequence number is `next_stored`.
pub(crate) fn check_sequence(
    next_stored: u64,
    account: &PlayerAccount,
    events: &[EconomyEvent],
) -> Result<(), StoreError> {
    let mut expected = next_stored;
    for e in events {
        if e.seq != expected || e.player != account.player {
            return Err(StoreError::SeqConflict {
                expected,
                got: e.seq,
            });
        }
        expected += 1;
    }
    if account.next_event_seq != expected {
        return Err(StoreError::SeqConflict {
            expected,
            got: account.next_event_seq,
        });
    }
    Ok(())
}

/// Deterministic in-memory [`AccountStore`] (`BTreeMap`-backed).
#[derive(Debug, Clone, Default)]
pub struct InMemoryStore {
    accounts: BTreeMap<PlayerId, PlayerAccount>,
    events: BTreeMap<PlayerId, Vec<EconomyEvent>>,
}

impl AccountStore for InMemoryStore {
    fn load(&self, player: PlayerId) -> Result<Option<PlayerAccount>, StoreError> {
        Ok(self.accounts.get(&player).cloned())
    }

    fn commit(
        &mut self,
        account: &PlayerAccount,
        events: &[EconomyEvent],
    ) -> Result<(), StoreError> {
        let next = self
            .events
            .get(&account.player)
            .map_or(0, |v| v.len() as u64);
        check_sequence(next, account, events)?;
        self.events
            .entry(account.player)
            .or_default()
            .extend_from_slice(events);
        self.accounts.insert(account.player, account.clone());
        Ok(())
    }

    fn events_since(
        &self,
        player: PlayerId,
        from_seq: u64,
    ) -> Result<Vec<EconomyEvent>, StoreError> {
        Ok(self
            .events
            .get(&player)
            .map(|v| v.iter().filter(|e| e.seq >= from_seq).cloned().collect())
            .unwrap_or_default())
    }
}
