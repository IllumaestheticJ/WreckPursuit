//! Career profile: everything a single-player career keeps between runs (the browser save).
//!
//! A [`CareerProfile`] is the account (cash, rep, bounty, garage with mods, insurance, impound
//! status, damage summaries and outstanding repair lines, heat history, lifetime stats), the
//! tail of its ledger, what the last run carried over ([`CarryOver`]: police heat), career
//! counters ([`CareerStats`]) and each owned car's full lattice damage. The economy never
//! looks inside the lattice blobs (`lattices`): they are whatever the damage system saved
//! (`wp_damage::FullDamageSnapshot` JSON in the browser), keyed by car uid.
//!
//! The JSON is versioned ([`PROFILE_FORMAT`], [`PROFILE_VERSION`]). [`CareerProfile::from_json`]
//! migrates older versions step by step ([`migrate`]) and refuses saves from a newer build.
//! Version 2 added [`crate::OwnedCar::purchase_price`] (resale is valued from the price paid);
//! the 1 → 2 step marks every car of an old save as "price unknown", valued at the cheapest
//! price the player could have paid. Fields added later must be
//! `#[serde(default)]` so older saves of the same version still load.
//!
//! Carry-over rules (tables in `consequences.toml [career]`, [`Economy::carry_heat`]):
//! - a capture or a wreck ends the night with no heat (the capture already cleared the
//!   bounty; a wreck is towed home);
//! - a run that ends free (escaped, or back in the garage while still wanted) carries
//!   `heat × heat_carry_frac`, capped at `heat_carry_max`, dropped below `heat_carry_min`;
//! - the bounty, rep, fines owed (debt) and the impound clock live in the account and carry
//!   over as they are; each finished run is one in-game night ([`Economy::end_of_day`]).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::{DamageMode, PlayerId};

use crate::data::Economy;
use crate::ledger::{EconomyEvent, Money, PlayerAccount};

/// `format` field of every career profile.
pub const PROFILE_FORMAT: &str = "wreckpursuit-career";

/// Current profile version (bump it, and add a [`migrate`] step, when the shape changes in a
/// way `#[serde(default)]` cannot absorb).
pub const PROFILE_VERSION: u32 = 2;

/// What one run leaves for the next.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct CarryOver {
    /// Police heat the next run starts with (`wp_police` scale: L1 from 10, L2 from 30).
    pub heat: f32,
}

/// Career counters (UI; not used in pricing).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CareerStats {
    /// Runs finished (each one is an in-game night).
    pub runs: u32,
    /// Runs that ended free (escaped, or back in the garage).
    pub runs_free: u32,
    /// Runs that ended in a capture.
    pub runs_captured: u32,
    /// Runs that ended with the car wrecked (towed, or captured wrecked).
    pub runs_wrecked: u32,
    /// Largest single escape bounty.
    pub best_escape_bounty: Money,
    /// Sim time driven over the whole career, physics ticks (`PHYSICS_DT` each).
    pub driven_ticks: u64,
}

/// How a run ended, for the carry-over rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunEnd {
    /// The player left the run with the car free: escaped, never chased, or quit to the garage.
    Free,
    /// Captured by police.
    Captured,
    /// The car was wrecked (towed home, or captured as a total loss).
    Wrecked,
}

/// A save that cannot be loaded.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ProfileError {
    /// Not JSON, or JSON of the wrong shape.
    #[error("not a career save: {0}")]
    Invalid(String),
    /// Written by a newer build.
    #[error("career save version {found} is newer than this build (version {PROFILE_VERSION})")]
    TooNew {
        /// Version in the file.
        found: u32,
    },
    /// An older version with no migration path.
    #[error("career save version {found} cannot be migrated (oldest supported: 1)")]
    NoMigration {
        /// Version in the file.
        found: u32,
    },
}

/// The persistent state of one single-player career (see the module docs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CareerProfile {
    /// Always [`PROFILE_FORMAT`].
    pub format: String,
    /// [`PROFILE_VERSION`] when written by this build.
    pub version: u32,
    /// Damage mode of the career (chosen at the start; runs use it).
    pub mode: DamageMode,
    /// The account.
    pub account: PlayerAccount,
    /// Newest ledger lines, oldest first (at most `career.ledger_keep`).
    #[serde(default)]
    pub ledger: Vec<EconomyEvent>,
    /// Carried over from the last run.
    #[serde(default)]
    pub carry: CarryOver,
    /// Career counters.
    #[serde(default)]
    pub stats: CareerStats,
    /// Lattice damage per owned car uid: opaque text written by the damage system (JSON in
    /// the browser, kept as a string so it round-trips bit-exactly). A car with no entry is
    /// pristine.
    #[serde(default)]
    pub lattices: BTreeMap<u32, String>,
}

impl Economy {
    /// A new career: an account with the starting cash and no car (the dealer or the economy
    /// floor car is the first stop).
    pub fn new_career(&self, player: PlayerId, mode: DamageMode) -> CareerProfile {
        let (account, ledger) = self.open_account(player);
        CareerProfile {
            format: PROFILE_FORMAT.to_string(),
            version: PROFILE_VERSION,
            mode,
            account,
            ledger,
            carry: CarryOver::default(),
            stats: CareerStats::default(),
            lattices: BTreeMap::new(),
        }
    }

    /// Heat the next run starts with, given the heat when this one ended (module docs).
    pub fn carry_heat(&self, heat_at_end: f32, end: RunEnd) -> f32 {
        let k = &self.consequences.career;
        if end != RunEnd::Free || !heat_at_end.is_finite() {
            return 0.0;
        }
        let h = (heat_at_end.max(0.0) * k.heat_carry_frac).min(k.heat_carry_max);
        if h < k.heat_carry_min {
            0.0
        } else {
            h
        }
    }

    /// Close a finished run on the profile: counters, carried heat, ledger trim, dropped
    /// lattices of cars no longer owned, then one in-game night ([`Economy::end_of_day`]:
    /// insurance premiums, impound storage, auctions). Returns the night's ledger lines.
    pub fn close_run(
        &self,
        p: &mut CareerProfile,
        end: RunEnd,
        heat_at_end: f32,
        escape_bounty: Money,
        driven_ticks: u64,
    ) -> Vec<EconomyEvent> {
        let s = &mut p.stats;
        s.runs += 1;
        match end {
            RunEnd::Free => s.runs_free += 1,
            RunEnd::Captured => s.runs_captured += 1,
            RunEnd::Wrecked => s.runs_wrecked += 1,
        }
        s.best_escape_bounty = s.best_escape_bounty.max(escape_bounty);
        s.driven_ticks = s.driven_ticks.saturating_add(driven_ticks);
        p.carry.heat = self.carry_heat(heat_at_end, end);
        self.next_night(p)
    }

    /// Advance one in-game night with no run (the impound auction countdown runs, premiums
    /// are charged). Returns the night's ledger lines.
    pub fn next_night(&self, p: &mut CareerProfile) -> Vec<EconomyEvent> {
        let events = match self.end_of_day(&mut p.account) {
            Ok(((), ev)) => ev,
            // `end_of_day` only fails on an inconsistent account; keep the profile as it is.
            Err(_) => Vec::new(),
        };
        p.ledger.extend(events.iter().cloned());
        self.tidy_profile(p);
        events
    }

    /// Trim the ledger to `career.ledger_keep` lines and drop lattices of cars that are gone.
    pub fn tidy_profile(&self, p: &mut CareerProfile) {
        let keep = self.consequences.career.ledger_keep;
        if p.ledger.len() > keep {
            let cut = p.ledger.len() - keep;
            p.ledger.drain(..cut);
        }
        let owned: Vec<u32> = p.account.garage.iter().map(|c| c.uid.0).collect();
        p.lattices.retain(|uid, _| owned.contains(uid));
    }
}

impl CareerProfile {
    /// Compact JSON (the save file / localStorage value).
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a career profile always serializes")
    }

    /// Load a save, migrating older versions (see [`migrate`]).
    pub fn from_json(text: &str) -> Result<Self, ProfileError> {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| ProfileError::Invalid(e.to_string()))?;
        let value = migrate(value)?;
        let p: CareerProfile =
            serde_json::from_value(value).map_err(|e| ProfileError::Invalid(e.to_string()))?;
        if p.account.cash < -1_000_000_000 || p.account.cash > 1_000_000_000_000 {
            return Err(ProfileError::Invalid("cash out of range".into()));
        }
        Ok(p)
    }
}

/// Bring a save of any supported version up to [`PROFILE_VERSION`].
///
/// Steps, oldest first; each edits the JSON and sets `version` to `n + 1`. When the shape
/// changes again, bump [`PROFILE_VERSION`] and add the next step.
/// - 1 → 2: every owned car gets `"purchase_price": null` (price paid unknown).
pub fn migrate(mut value: serde_json::Value) -> Result<serde_json::Value, ProfileError> {
    let obj = value
        .as_object()
        .ok_or_else(|| ProfileError::Invalid("expected a JSON object".into()))?;
    if obj.get("format").and_then(|f| f.as_str()) != Some(PROFILE_FORMAT) {
        return Err(ProfileError::Invalid(format!(
            "missing \"format\": \"{PROFILE_FORMAT}\""
        )));
    }
    let mut version =
        obj.get("version")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| ProfileError::Invalid("missing \"version\"".into()))? as u32;
    if version > PROFILE_VERSION {
        return Err(ProfileError::TooNew { found: version });
    }
    if version == 1 {
        value = step_1_to_2(value)?;
        version = 2;
    }
    if version < PROFILE_VERSION {
        return Err(ProfileError::NoMigration { found: version });
    }
    value["version"] = serde_json::Value::from(version);
    Ok(value)
}

/// 1 → 2: cars carry the price paid; old cars get `null` (see [`Economy::resale_basis`]).
fn step_1_to_2(mut value: serde_json::Value) -> Result<serde_json::Value, ProfileError> {
    let garage = value
        .pointer_mut("/account/garage")
        .and_then(|g| g.as_array_mut())
        .ok_or_else(|| ProfileError::Invalid("missing \"account.garage\"".into()))?;
    for car in garage.iter_mut() {
        let car = car
            .as_object_mut()
            .ok_or_else(|| ProfileError::Invalid("a garage car is not an object".into()))?;
        car.entry("purchase_price")
            .or_insert(serde_json::Value::Null);
    }
    value["version"] = serde_json::Value::from(2);
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn econ() -> Economy {
        Economy::embedded()
    }

    #[test]
    fn heat_carry_rules() {
        let e = econ();
        assert_eq!(e.carry_heat(40.0, RunEnd::Captured), 0.0);
        assert_eq!(e.carry_heat(40.0, RunEnd::Wrecked), 0.0);
        assert_eq!(e.carry_heat(40.0, RunEnd::Free), 20.0);
        assert_eq!(e.carry_heat(200.0, RunEnd::Free), 45.0);
        assert_eq!(e.carry_heat(8.0, RunEnd::Free), 0.0);
        assert_eq!(e.carry_heat(f32::NAN, RunEnd::Free), 0.0);
    }

    #[test]
    fn save_round_trips() {
        let e = econ();
        let mut p = e.new_career(PlayerId(1), DamageMode::Arcade);
        p.lattices
            .insert(1, r#"{"anything":[1.5,2.25]}"#.to_string());
        p.carry.heat = 12.5;
        let back = CareerProfile::from_json(&p.to_json()).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn version_1_saves_migrate_with_unknown_purchase_price() {
        let e = econ();
        let mut p = e.new_career(PlayerId(1), DamageMode::Arcade);
        e.issue_floor_car(&mut p.account).unwrap();
        let mut v: serde_json::Value = serde_json::from_str(&p.to_json()).unwrap();
        v["version"] = 1.into();
        v["account"]["garage"][0]
            .as_object_mut()
            .unwrap()
            .remove("purchase_price");
        let back = CareerProfile::from_json(&v.to_string()).unwrap();
        assert_eq!(back.version, PROFILE_VERSION);
        let car = &back.account.garage[0];
        assert_eq!(car.purchase_price, None);
        assert!(e.sale_price(car) <= e.market.floor_car.price);
        // Written again, it is a version-2 save that keeps its (unknown) price.
        let again = CareerProfile::from_json(&back.to_json()).unwrap();
        assert_eq!(again, back);
    }

    #[test]
    fn versions_are_checked() {
        let e = econ();
        let p = e.new_career(PlayerId(1), DamageMode::Simulation);
        let mut v: serde_json::Value = serde_json::from_str(&p.to_json()).unwrap();
        v["version"] = 3.into();
        assert_eq!(
            CareerProfile::from_json(&v.to_string()),
            Err(ProfileError::TooNew { found: 3 })
        );
        v["version"] = 0.into();
        assert_eq!(
            CareerProfile::from_json(&v.to_string()),
            Err(ProfileError::NoMigration { found: 0 })
        );
        v["version"] = 2.into();
        v["format"] = "something-else".into();
        assert!(matches!(
            CareerProfile::from_json(&v.to_string()),
            Err(ProfileError::Invalid(_))
        ));
        assert!(matches!(
            CareerProfile::from_json("not json"),
            Err(ProfileError::Invalid(_))
        ));
    }

    #[test]
    fn optional_fields_default() {
        let e = econ();
        let p = e.new_career(PlayerId(1), DamageMode::Arcade);
        let mut v: serde_json::Value = serde_json::from_str(&p.to_json()).unwrap();
        let o = v.as_object_mut().unwrap();
        for k in ["ledger", "carry", "stats", "lattices"] {
            o.remove(k);
        }
        let back = CareerProfile::from_json(&v.to_string()).unwrap();
        assert_eq!(back.account, p.account);
        assert!(back.ledger.is_empty() && back.lattices.is_empty());
    }

    #[test]
    fn close_run_counts_a_night_and_trims() {
        let e = econ();
        let mut p = e.new_career(PlayerId(1), DamageMode::Arcade);
        p.lattices.insert(99, String::new()); // car 99 is not owned
        let day0 = p.account.day;
        e.close_run(&mut p, RunEnd::Free, 30.0, 250, 600);
        assert_eq!(p.account.day, day0 + 1);
        assert_eq!(p.stats.runs, 1);
        assert_eq!(p.stats.runs_free, 1);
        assert_eq!(p.stats.best_escape_bounty, 250);
        assert_eq!(p.carry.heat, 15.0);
        assert!(p.lattices.is_empty());
        e.close_run(&mut p, RunEnd::Captured, 30.0, 0, 240);
        assert_eq!(p.carry.heat, 0.0);
        assert_eq!(p.stats.runs_captured, 1);
        assert_eq!(p.stats.driven_ticks, 840);
    }
}
