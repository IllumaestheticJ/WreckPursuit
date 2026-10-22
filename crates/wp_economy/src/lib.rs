//! # wp_economy
//!
//! Money, reputation, repair billing, capture consequences, impound, race payouts, the mod
//! system and the garage (owned by workstream 08).
//!
//! Design: **pure functions over a small persistent state**. Every operation is a method on
//! [`Economy`] (immutable, data-driven tables loaded from `data/economy/*.toml` and
//! `data/mods/*.json`) that takes a `&mut` [`PlayerAccount`] and returns the
//! [`EconomyEvent`]s it posted. Operations are all-or-nothing: a refused operation leaves the
//! account untouched. The server persists accounts and events through an [`AccountStore`]
//! (SQLite behind the `native` feature, [`InMemoryStore`] otherwise).
//!
//! Server-authoritative: nothing here reads wall-clock time; randomness (Simulation seizures)
//! comes from an explicitly passed [`wp_math::Rng`]; maps are `BTreeMap`.
//!
//! Modules:
//! - [`ledger`] — accounts, events, transactions, stores.
//! - [`repair`] — price tables for `wp_damage`, damage reports, repair options, shops, insurance.
//! - [`capture`] — fines, bail, impound, seizure, escapes, the daily tick, cop XP.
//! - [`payouts`] — purses, buy-ins, clean bonus, pink slips, informants.
//! - [`mods`] — mod definitions, catalog, install rules, applying mods to definitions.
//! - [`garage`] — buy/sell, floor car, install/uninstall, preview, valuation, insurance.
//! - [`estimate`] — performance estimator and class index.
//! - [`pursuit`] — police pursuit endings → capture / escape settlement; decodes today's
//!   police events ([`PoliceSignal::from_sim_event`]).
//! - [`session`] — end-of-run glue for `wp_sim`: sim events + damage reports → one settled
//!   [`RunResult`].
//! - [`profile`] — the single-player career save (versioned, with carry-over rules).
//! - [`career`] — scripted multi-night career simulation used for tuning.
//! - [`progression`] — rep tiers, feature / dealer unlocks, betting limits, tier rewards.

#![warn(missing_docs)]

pub mod capture;
pub mod career;
pub mod data;
mod error;
pub mod estimate;
pub mod garage;
pub mod ledger;
pub mod mods;
pub mod payouts;
pub mod profile;
pub mod progression;
pub mod pursuit;
pub mod repair;
pub mod session;
#[cfg(feature = "native")]
pub mod sqlite;

pub use capture::{CaptureContext, CaptureOutcome, CarFate, CopAction, ImpoundStatus};
pub use data::{CareerRules, DataSources, Economy};
pub use error::{DataError, EconomyError};
pub use estimate::PerfMetrics;
pub use garage::{CarSpec, Condition, Preview};
pub use ledger::{
    account_id_of, player_of, AccountStore, CarLocation, CarUid, DamageSnapshot, EconResult,
    EconomyEvent, EventReason, InMemoryStore, Money, OwnedCar, PlayerAccount, StoreError,
    ViolationCounts,
};
pub use mods::{ModCatalog, ModDef, ModEffect, ModSlot, ToolLoadout};
pub use payouts::{PinkSlipConsent, RaceResult, RaceSettlement, RaceSetup, RaceType};
pub use profile::{AbandonedRun, CareerProfile, CareerStats, CarryOver, ProfileError, RunEnd};
pub use progression::{
    career_note, BetTicket, BetVenue, CareerProgress, CareerState, Credits, Feature,
    ProgressUpdate, ProgressionRules, RepTier,
};
pub use pursuit::{PoliceSignal, PursuitEnd, PursuitResolution, PursuitSettlement, PursuitTracker};
pub use repair::{
    district_from_key, district_key, DamageReport, RepairOption, RepairOrder, RepairQuote,
    ShopContext, ShopKind,
};
pub use session::{
    Binding, CarRunResult, CarStatus, EconomySession, PendingPursuit, RepairPolicy, RunResult,
    SessionConfig,
};
pub use wp_contracts::{AccountId, WantedLevel};
