//! Repair pricing input (`PriceTable`) and output (`RepairEstimate`). `wp_damage` produces
//! estimates, `wp_economy` turns them into bills. Prices are in whole currency units.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{BodyRegion, Component};

/// Prices used to turn damage into a repair estimate. Supplied by `wp_economy`
/// (from `data/economy/prices.toml`), already resolved for one vehicle class.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct PriceTable {
    /// Cost to fully replace each body region's panel(s).
    pub region_replace_cost: BTreeMap<BodyRegion, f32>,
    /// Cost to fully replace each component.
    pub component_replace_cost: BTreeMap<Component, f32>,
    /// Labour rate per hour.
    pub labour_per_hour: f32,
    /// Labour hours to fully replace a region / component (applied per line item × severity).
    /// Fallback for items missing from `labour_hours`.
    pub labour_hours_per_item: f32,
    /// Per-item labour hours for a full replacement (v0.2.0; a roof costs more than a mirror).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labour_hours: BTreeMap<RepairItem, f32>,
    /// Fraction of the replacement parts cost charged for a *repair* (below the replace
    /// severity) (v0.2.0). `None` = the damage model's default (0.3 in `wp_damage`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair_parts_fraction: Option<f32>,
}

/// What a repair line item fixes.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum RepairItem {
    /// Body region (panel straightening / replacement).
    Region(BodyRegion),
    /// Mechanical component.
    Component(Component),
    /// Re-attach / replace a detached part group.
    DetachedPart(BodyRegion),
}

/// One line on a repair estimate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RepairLine {
    /// What is repaired.
    pub item: RepairItem,
    /// Damage severity 0 (cosmetic) ..= 1 (replace).
    pub severity: f32,
    /// Parts cost, **unscaled** (v0.2.0 clarification: the mode's `repair_cost_scale` is not in it).
    pub parts_cost: f32,
    /// Labour hours, **unscaled**.
    pub labour_hours: f32,
    /// Line total (parts + labour), the only figure that includes the mode's `repair_cost_scale`.
    pub total: f32,
}

/// Full repair estimate for one vehicle.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct RepairEstimate {
    /// Line items in deterministic order.
    pub lines: Vec<RepairLine>,
    /// Sum of all line totals.
    pub total: f32,
}
