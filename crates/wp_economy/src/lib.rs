//! # wp_economy
//!
//! Money, reputation, repair billing, impound, mods and garage (owned by workstream 08).
//! **Current state: stub** — an in-memory account store and a pass-through repair bill.

#![warn(missing_docs)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::{PlayerId, RepairEstimate};

/// Persistent per-player economy state.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PlayerAccount {
    /// Cash in whole currency units (may go negative only via fines = debt).
    pub cash: i64,
    /// Street reputation.
    pub rep: i64,
    /// Outstanding bounty.
    pub bounty: u32,
    /// Police career rank XP.
    pub cop_rank: u32,
    /// Ids of owned cars (`VehicleDef::id`).
    pub garage: Vec<String>,
}

/// Storage backend for accounts (in-memory now; SQLite behind a `native` feature later).
pub trait AccountStore {
    /// Load an account, or a fresh default one.
    fn load(&self, player: PlayerId) -> PlayerAccount;
    /// Persist an account.
    fn save(&mut self, player: PlayerId, account: &PlayerAccount);
}

/// Deterministic in-memory [`AccountStore`].
#[derive(Debug, Clone, Default)]
pub struct InMemoryStore {
    accounts: BTreeMap<PlayerId, PlayerAccount>,
}

impl AccountStore for InMemoryStore {
    fn load(&self, player: PlayerId) -> PlayerAccount {
        self.accounts.get(&player).cloned().unwrap_or_default()
    }

    fn save(&mut self, player: PlayerId, account: &PlayerAccount) {
        self.accounts.insert(player, account.clone());
    }
}

/// Repair bill total for an estimate (stub: the estimate total, rounded up).
pub fn repair_bill(estimate: &RepairEstimate) -> i64 {
    estimate.total.ceil() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_round_trips() {
        let mut s = InMemoryStore::default();
        let acc = PlayerAccount {
            cash: 500,
            ..Default::default()
        };
        s.save(PlayerId(3), &acc);
        assert_eq!(s.load(PlayerId(3)), acc);
        assert_eq!(repair_bill(&RepairEstimate::default()), 0);
    }
}
