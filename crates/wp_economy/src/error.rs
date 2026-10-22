//! Error type shared by every economy operation.

use thiserror::Error;

use crate::ledger::{CarUid, Money};

/// Why an economy operation was refused. A refused operation never changes the account.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum EconomyError {
    /// A voluntary purchase costs more than the cash on hand.
    #[error("insufficient funds: need {needed}, have {available}")]
    InsufficientFunds {
        /// Amount required.
        needed: Money,
        /// Cash available (may be negative = debt).
        available: Money,
    },
    /// The account is in debt and the operation is not allowed until it is paid off.
    #[error("account is {debt} in debt: {what} is blocked until the debt is paid")]
    InDebt {
        /// Outstanding debt (positive number).
        debt: Money,
        /// What was blocked.
        what: String,
    },
    /// No car with this uid in the garage.
    #[error("unknown car {0:?}")]
    UnknownCar(CarUid),
    /// The car exists but cannot be used for this (impounded, wrecked, ...).
    #[error("car {car:?} unavailable: {reason}")]
    CarUnavailable {
        /// The car.
        car: CarUid,
        /// Why.
        reason: String,
    },
    /// No mod with this id in the catalog.
    #[error("unknown mod {0:?}")]
    UnknownMod(String),
    /// The mod cannot go on this car (class whitelist, mode restriction, duplicate...).
    #[error("mod {mod_id:?} not allowed: {reason}")]
    ModNotAllowed {
        /// The mod.
        mod_id: String,
        /// Why.
        reason: String,
    },
    /// A required mod is missing.
    #[error("mod {mod_id:?} requires {requires:?}")]
    MissingRequirement {
        /// The mod being installed.
        mod_id: String,
        /// The requirement that is not met.
        requires: String,
    },
    /// The mod cannot be removed or replaced because another installed mod needs it.
    #[error("mod {mod_id:?} is required by installed mod {dependent:?}")]
    RequiredBy {
        /// The mod being removed / replaced.
        mod_id: String,
        /// The installed mod that depends on it.
        dependent: String,
    },
    /// The mod is not installed on this car.
    #[error("mod {0:?} is not installed")]
    NotInstalled(String),
    /// The requested repair has nothing to fix.
    #[error("nothing to repair")]
    NothingToRepair,
    /// Pink-slip race refused (mode, consent, rep...).
    #[error("pink slip not allowed: {0}")]
    PinkSlipNotAllowed(String),
    /// Bad argument (unknown tier, empty selection, ...).
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// A workshop operation on a slot was refused.
    #[error("{slot}: {reason}")]
    PartRefused {
        /// Slot (or part kind) concerned.
        slot: String,
        /// Why.
        reason: String,
    },
    /// No loose part with this uid in the inventory.
    #[error("no part {0} in the inventory")]
    UnknownPart(u32),
    /// Applying mods produced a definition the contract loader rejects.
    #[error("modified definition is invalid: {0}")]
    InvalidResult(String),
}

/// Errors while loading economy data (`data/economy/*.toml`, `data/mods/*.json`).
#[derive(Debug, Error)]
pub enum DataError {
    /// File could not be read.
    #[error("cannot read {path}: {source}")]
    Io {
        /// Offending path.
        path: String,
        /// Underlying error.
        source: std::io::Error,
    },
    /// TOML did not match the expected structure.
    #[error("invalid TOML in {name}: {source}")]
    Toml {
        /// File label.
        name: String,
        /// Underlying error.
        source: toml::de::Error,
    },
    /// JSON did not match the expected structure.
    #[error("invalid JSON in {name}: {source}")]
    Json {
        /// File label.
        name: String,
        /// Underlying error.
        source: serde_json::Error,
    },
    /// Structurally valid but semantically wrong data (missing table entry, bad reference...).
    #[error("{what}: {reason}")]
    Invalid {
        /// Which object.
        what: String,
        /// What is wrong.
        reason: String,
    },
}

pub(crate) fn invalid(what: impl Into<String>, reason: impl Into<String>) -> DataError {
    DataError::Invalid {
        what: what.into(),
        reason: reason.into(),
    }
}
