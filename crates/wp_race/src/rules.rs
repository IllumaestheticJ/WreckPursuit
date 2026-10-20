//! Rule objects: per-kind start-grid, limits, tolerances and DNF/DQ settings
//! (`data/events/rules.toml`), resolved into one [`RaceRules`] for a concrete race.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::course::GridLayout;
use crate::error::RaceDataError;
use crate::kind::RaceKind;

/// Settings shared by every race kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommonRules {
    /// A contact above this normal impulse breaks clean-race status, N·s.
    pub contact_clean_threshold_ns: f32,
    /// A contact with a non-racer (civilian / traffic) above this impulse counts as a
    /// civilian hit, N·s.
    pub civilian_hit_threshold_ns: f32,
    /// Gate tolerance added on both sides of every gate, m.
    pub gate_tolerance_base_m: f32,
    /// Extra gate tolerance per m/s of speed, s (≈ the sample interval the server judges at).
    pub gate_tolerance_per_mps_s: f32,
    /// Displacement between two samples faster than this is implausible (teleport) → DQ, m/s.
    pub max_plausible_speed_mps: f32,
    /// Countdown from `start()` to green, s.
    pub countdown_s: f32,
}

/// Per-kind settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KindRules {
    /// Start-grid layout.
    pub grid: GridLayout,
    /// Minimum racers to start.
    pub min_racers: u8,
    /// Maximum racers.
    pub max_racers: u8,
    /// Time limit from green; racers still running are DNF (Outrun: the leader wins), s.
    pub time_limit_s: f32,
    /// After the first finisher, the rest have this long before DNF, s (0 = no cut-off).
    pub finish_window_s: f32,
}

/// Drag-specific settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DragRules {
    /// Pass length, m.
    pub distance_m: f32,
    /// Moving this far before green is a jump start (DQ), m.
    pub jump_start_m: f32,
    /// Throttle above this counts as the launch for reaction time.
    pub launch_throttle: f32,
    /// Ideal upshift window as fractions of redline `[lo, hi]`.
    pub shift_window: [f32; 2],
}

/// Outrun-specific settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutrunRules {
    /// The chaser must be this far ahead along the leader's heading to take the lead, m.
    pub overtake_margin_m: f32,
}

/// Delivery-specific settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryRules {
    /// Contacts below this impulse do not hurt the cargo, N·s.
    pub cargo_threshold_ns: f32,
    /// Accumulated impulse above the threshold that destroys the cargo, N·s.
    pub cargo_fragility_ns: f32,
}

/// The whole `data/events/rules.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulesTable {
    /// Shared settings.
    pub common: CommonRules,
    /// Per-kind settings, keyed by [`RaceKind::key`].
    pub kinds: BTreeMap<String, KindRules>,
    /// Drag settings.
    pub drag: DragRules,
    /// Outrun settings.
    pub outrun: OutrunRules,
    /// Delivery settings.
    pub delivery: DeliveryRules,
}

impl RulesTable {
    /// Parse and validate TOML.
    pub fn from_toml(text: &str) -> Result<RulesTable, RaceDataError> {
        let t: RulesTable =
            toml::from_str(text).map_err(|e| RaceDataError::parse("rules.toml", e))?;
        t.validate()?;
        Ok(t)
    }

    /// Check every kind is present and values are sane.
    pub fn validate(&self) -> Result<(), RaceDataError> {
        let bad = |what: &str, why: &str| Err(RaceDataError::invalid("rules.toml", what, why));
        for k in RaceKind::KEYS {
            let Some(r) = self.kinds.get(k) else {
                return bad(&format!("kinds.{k}"), "missing");
            };
            if r.min_racers == 0 || r.min_racers > r.max_racers {
                return bad(&format!("kinds.{k}"), "need 1 <= min_racers <= max_racers");
            }
            if r.time_limit_s <= 0.0 {
                return bad(&format!("kinds.{k}.time_limit_s"), "must be > 0");
            }
        }
        for k in self.kinds.keys() {
            if RaceKind::from_key(k).is_none() {
                return bad(&format!("kinds.{k}"), "unknown race kind");
            }
        }
        for k in ["outrun", "drag"] {
            if self.kinds[k].max_racers != 2 {
                return bad(
                    &format!("kinds.{k}.max_racers"),
                    "head-to-head kinds take 2",
                );
            }
        }
        let c = &self.common;
        if c.contact_clean_threshold_ns <= 0.0 || c.civilian_hit_threshold_ns <= 0.0 {
            return bad("common", "impulse thresholds must be > 0");
        }
        if c.max_plausible_speed_mps < 50.0 {
            return bad("common.max_plausible_speed_mps", "must be >= 50");
        }
        let w = self.drag.shift_window;
        if !(0.0 < w[0] && w[0] < w[1] && w[1] <= 1.05) {
            return bad("drag.shift_window", "need 0 < lo < hi <= 1.05");
        }
        if self.delivery.cargo_fragility_ns <= 0.0 {
            return bad("delivery.cargo_fragility_ns", "must be > 0");
        }
        Ok(())
    }

    /// Resolve the rules for one race.
    pub fn for_kind(&self, kind: RaceKind) -> RaceRules {
        RaceRules {
            kind,
            common: self.common.clone(),
            kind_rules: self.kinds[kind.key()].clone(),
            drag: self.drag.clone(),
            outrun: self.outrun.clone(),
            delivery: self.delivery.clone(),
        }
    }
}

/// Fully resolved rules for one race (what [`crate::judge::RaceJudge`] enforces).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RaceRules {
    /// The race kind (with its parameters).
    pub kind: RaceKind,
    /// Shared settings.
    pub common: CommonRules,
    /// This kind's settings.
    pub kind_rules: KindRules,
    /// Drag settings (used when `kind == Drag`).
    pub drag: DragRules,
    /// Outrun settings (used when `kind == Outrun`).
    pub outrun: OutrunRules,
    /// Delivery settings (used when `kind == Delivery`).
    pub delivery: DeliveryRules,
}

impl RaceRules {
    /// Gate tolerance at a given speed, m.
    pub fn gate_tolerance_m(&self, speed_mps: f32) -> f32 {
        self.common.gate_tolerance_base_m + self.common.gate_tolerance_per_mps_s * speed_mps.abs()
    }
}
