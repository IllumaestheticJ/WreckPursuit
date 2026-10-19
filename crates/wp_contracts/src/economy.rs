//! Typed economy events (v0.3.0; final list from workstream 08 / G, with F's NET-4 and NET-5).
//!
//! Replaces the opaque `SimEvent::EconomyEvent { kind, payload }`. `wp_economy` keeps its own
//! ledger types for persistence and converts with `to_contract` / `to_sim_event`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{AccountId, WantedLevel};

/// Identifier of one owned car, unique within an account (never reused).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
)]
pub struct CarUid(pub u32);

/// Economy race type; keys `data/economy/stakes.toml [race_types.*]`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum RaceType {
    /// Point-to-point through traffic.
    Sprint,
    /// Laps of a closed-ish loop.
    Circuit,
    /// Quarter-mile.
    Drag,
    /// Mountain / hill road duel.
    Touge,
    /// Informal street meet sprint: free entry, small purse.
    Meet,
}

impl RaceType {
    /// Every race type.
    pub const ALL: [RaceType; 5] = [
        RaceType::Sprint,
        RaceType::Circuit,
        RaceType::Drag,
        RaceType::Touge,
        RaceType::Meet,
    ];

    /// Stable lower-case id used in data files and ledger records.
    pub fn id(self) -> &'static str {
        match self {
            RaceType::Sprint => "sprint",
            RaceType::Circuit => "circuit",
            RaceType::Drag => "drag",
            RaceType::Touge => "touge",
            RaceType::Meet => "meet",
        }
    }

    /// Inverse of [`RaceType::id`].
    pub fn from_id(id: &str) -> Option<RaceType> {
        RaceType::ALL.iter().copied().find(|t| t.id() == id)
    }
}

/// Activity a player cop earned or lost XP for.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum CopXpReason {
    /// Made a capture.
    Capture,
    /// Assisted a capture.
    Assist,
    /// Minutes of patrol / pursuit without incident.
    CleanDriving,
    /// Hit a civilian.
    CivilianCollision,
    /// Wrecked own unit.
    SelfWreck,
    /// Kept pushing after the risk budget was spent.
    RiskBudgetOverspent,
}

/// Why an account changed. Codes ([`EconomyReason::kind_code`]) are stable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum EconomyReason {
    /// 1: account created with starting cash.
    AccountOpened,
    /// 10: race buy-in paid (was `RaceEntryFee`).
    StakeCharged {
        /// Race type.
        race: RaceType,
    },
    /// 11: race purse share paid (was `RacePurse`).
    RacePayout {
        /// Race type.
        race: RaceType,
        /// Finishing position (1 = win).
        placement: u8,
    },
    /// 12: clean-race bonus.
    CleanRaceBonus,
    /// 13: reputation from a race result.
    RaceRep {
        /// Finishing position (0 = DNF).
        placement: u8,
    },
    /// 14: won the loser's car in a pink-slip race.
    PinkSlipWon {
        /// Base car id.
        def_id: String,
    },
    /// 15: lost a car in a pink-slip race.
    PinkSlipLost {
        /// Base car id.
        def_id: String,
    },
    /// 20: repair paid at a shop.
    RepairPaid {
        /// Line items repaired.
        lines: u32,
        /// Labour multiplier applied (district × shop).
        labour_multiplier: f32,
    },
    /// 21: insurance paid part of a repair bill.
    InsuranceClaim {
        /// Amount covered by the insurer.
        covered: i64,
    },
    /// 22: insurance refused a claim.
    InsuranceClaimDenied,
    /// 23: player chose to drive the car damaged.
    DriveDamaged,
    /// 24: car scrapped for salvage.
    CarScrapped {
        /// Base car id.
        def_id: String,
    },
    /// 25: a legit shop reported a wanted car (police add heat).
    ShopReported {
        /// District key of the shop (`data/economy/prices.toml [district_labour]`; still a
        /// string in v0.3.0 because those keys are not all [`crate::DistrictId`]s yet).
        district: String,
        /// Wanted level at the time.
        level: WantedLevel,
    },
    /// 26: damage report recorded at the end of a session.
    DamageRecorded {
        /// Global health reported.
        health: f32,
        /// Total loss.
        total_loss: bool,
    },
    /// 30: fine at capture.
    Fine {
        /// Wanted level at capture.
        level: WantedLevel,
    },
    /// 31: bail posted at capture.
    Bail,
    /// 32: reputation lost at capture.
    CaptureRepLoss {
        /// Wanted level at capture.
        level: WantedLevel,
    },
    /// 33: bounty cleared at capture (police heat resets).
    BountyCleared,
    /// 34: bounty grew.
    BountyGained,
    /// 35: towing charge.
    Towing,
    /// 36: car moved to the pound (must not spawn).
    CarImpounded,
    /// 37: car retrieved from the pound.
    ImpoundRetrieved {
        /// Days spent in the pound.
        days: u32,
    },
    /// 38: car auctioned by the pound (lost).
    CarAuctioned {
        /// Base car id.
        def_id: String,
    },
    /// 39: car seized by the police (must not spawn).
    CarSeized {
        /// Base car id.
        def_id: String,
    },
    /// 40: car destroyed (must not spawn).
    CarDestroyed {
        /// Base car id.
        def_id: String,
    },
    /// 41: escaped a pursuit.
    Escape {
        /// Wanted level escaped from.
        level: WantedLevel,
    },
    /// 50: cop bounty reward for a capture or assist.
    CopReward {
        /// Wanted level of the captured racer.
        level: WantedLevel,
    },
    /// 51: cop XP change.
    CopXp {
        /// Why.
        reason: CopXpReason,
    },
    /// 52: paid an informant (police reduce heat).
    InformantTip {
        /// Tier id.
        tier: String,
        /// Heat points to remove (0–100 scale).
        heat_reduction: f32,
    },
    /// 60: bought a car.
    CarPurchased {
        /// Base car id.
        def_id: String,
    },
    /// 61: sold a car.
    CarSold {
        /// Base car id.
        def_id: String,
    },
    /// 62: issued the economy-floor beater.
    FloorCarIssued {
        /// Base car id.
        def_id: String,
    },
    /// 63: mod installed.
    ModInstalled {
        /// Mod id.
        mod_id: String,
    },
    /// 64: mod removed.
    ModRemoved {
        /// Mod id.
        mod_id: String,
    },
    /// 70: insurance policy bought.
    InsurancePurchased {
        /// Tier id.
        tier: String,
    },
    /// 71: daily insurance premium.
    InsurancePremium,
    /// 72: policy lapsed.
    InsuranceLapsed,
    /// 73: policy voided (captured while wanted).
    InsuranceVoided,
    /// 999: manual adjustment (admin / harness only).
    Adjustment {
        /// Free-text note.
        note: String,
    },
}

impl EconomyReason {
    /// Stable `u16` code of the variant (same codes as the v0.2.0 opaque `kind`).
    pub fn kind_code(&self) -> u16 {
        use EconomyReason::*;
        match self {
            AccountOpened => 1,
            StakeCharged { .. } => 10,
            RacePayout { .. } => 11,
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
}

/// One auditable change to an account (`SimEvent::Economy`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EconomyEvent {
    /// Durable account id (F NET-5 / meta MC-1).
    pub account: AccountId,
    /// Per-account sequence number, gap-free.
    pub seq: u64,
    /// In-game day.
    pub day: u32,
    /// Why.
    pub reason: EconomyReason,
    /// Car concerned, if any.
    pub car: Option<CarUid>,
    /// Cash change.
    pub cash_delta: i64,
    /// Reputation change (as applied).
    pub rep_delta: i64,
    /// Bounty change.
    pub bounty_delta: i64,
    /// Cop XP change (as applied).
    pub cop_xp_delta: i64,
    /// Cash after this event.
    pub cash_after: i64,
}
