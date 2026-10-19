//! Repair estimate: per-region plastic energy, detached parts and component health priced
//! with a [`PriceTable`] and scaled by `ModeTable::repair_cost_scale`.
//!
//! Severity per line, 0 ..= 1:
//! - region: plastic energy / the region's "replace" energy (the work to plastically strain
//!   all its beams by `DamageTuning::region_replace_strain`); shattered glass = 1;
//! - detached part: 1 (the region line is replaced by a `DetachedPart` line);
//! - component: `1 - health` (failed = 1).
//!
//! Severity ≥ [`REPLACE_SEVERITY`] bills the full part price ("replace"); below it the part
//! is straightened/repaired for a fraction of the price. Labour is
//! `labour_hours_per_item × severity` (at least a minimum). Lines under
//! [`MIN_SEVERITY`] are omitted.

use std::collections::BTreeMap;

use wp_contracts::{
    BodyRegion, Component, ModeTable, PriceTable, RepairEstimate, RepairItem, RepairLine,
};

use crate::model::{DamageModel, ALL_REGIONS};

/// Severity at and above which a part is replaced rather than repaired.
pub const REPLACE_SEVERITY: f32 = 0.6;
/// Lines below this severity are not billed.
pub const MIN_SEVERITY: f32 = 0.02;
/// Fraction of the replace price billed for a repair at severity just below replace.
pub const REPAIR_PARTS_FRACTION: f32 = 0.3;
/// Minimum labour as a fraction of `labour_hours_per_item` for any billed line.
pub const MIN_LABOUR_FRACTION: f32 = 0.1;

fn line(
    item: RepairItem,
    severity: f32,
    replace_cost: f32,
    mode: &ModeTable,
    p: &PriceTable,
) -> RepairLine {
    let severity = severity.clamp(0.0, 1.0);
    let parts = if severity >= REPLACE_SEVERITY {
        replace_cost
    } else {
        replace_cost * REPAIR_PARTS_FRACTION * severity / REPLACE_SEVERITY
    };
    let hours = p.labour_hours_per_item * severity.max(MIN_LABOUR_FRACTION);
    let parts_cost = parts * mode.repair_cost_scale;
    let labour_cost = hours * p.labour_per_hour * mode.repair_cost_scale;
    RepairLine {
        item,
        severity,
        parts_cost,
        labour_hours: hours,
        total: parts_cost + labour_cost,
    }
}

/// Severity of each damaged region (0 ..= 1), regions with no damage omitted.
pub fn region_severities(m: &DamageModel) -> BTreeMap<BodyRegion, f32> {
    let mut out = BTreeMap::new();
    for r in ALL_REGIONS {
        let i = r as usize;
        let mut sev = if m.region_ref_j[i] > 0.0 {
            m.region_plastic_j[i] / m.region_ref_j[i]
        } else {
            0.0
        };
        if m.shattered.contains(&r) {
            sev = 1.0;
        }
        if sev > 0.0 {
            out.insert(r, sev.min(1.0));
        }
    }
    out
}

/// Build the estimate (see the module docs). Deterministic line order: regions, detached
/// parts, components, each in enum order.
pub fn estimate(m: &DamageModel, mode: &ModeTable, p: &PriceTable) -> RepairEstimate {
    let mut lines = Vec::new();
    let detached_regions: Vec<BodyRegion> = m
        .groups
        .iter()
        .filter(|g| g.detached)
        .map(|g| g.region)
        .collect();
    for (r, sev) in region_severities(m) {
        if sev < MIN_SEVERITY || detached_regions.contains(&r) {
            continue;
        }
        let cost = p.region_replace_cost.get(&r).copied().unwrap_or(0.0);
        lines.push(line(RepairItem::Region(r), sev, cost, mode, p));
    }
    let mut detached_sorted = detached_regions.clone();
    detached_sorted.sort();
    detached_sorted.dedup();
    for r in detached_sorted {
        let cost = p.region_replace_cost.get(&r).copied().unwrap_or(0.0);
        lines.push(line(RepairItem::DetachedPart(r), 1.0, cost, mode, p));
    }
    let mut comps: BTreeMap<Component, f32> = BTreeMap::new();
    for c in &m.components {
        let sev = if c.failed { 1.0 } else { 1.0 - c.health };
        let e = comps.entry(c.kind).or_insert(0.0);
        *e = e.max(sev);
    }
    for (c, sev) in comps {
        if sev < MIN_SEVERITY {
            continue;
        }
        let cost = p.component_replace_cost.get(&c).copied().unwrap_or(0.0);
        lines.push(line(RepairItem::Component(c), sev, cost, mode, p));
    }
    let total = lines.iter().map(|l| l.total).sum();
    RepairEstimate { lines, total }
}

/// A plausible mid-range price table for tests, the testbed and until `wp_economy` supplies
/// real ones (prompt 08 owns pricing). Whole currency units.
pub fn default_price_table() -> PriceTable {
    use BodyRegion::*;
    use Component::*;
    let regions = [
        (FrontBumper, 450.0),
        (Hood, 700.0),
        (FrontLeftFender, 380.0),
        (FrontRightFender, 380.0),
        (LeftDoorF, 900.0),
        (LeftDoorR, 850.0),
        (RightDoorF, 900.0),
        (RightDoorR, 850.0),
        (Roof, 1500.0),
        (RearLeftQuarter, 1100.0),
        (RearRightQuarter, 1100.0),
        (Trunk, 650.0),
        (RearBumper, 420.0),
        (Windshield, 600.0),
        (RearGlass, 400.0),
        (FrameFront, 2500.0),
        (FrameRear, 2200.0),
        (FrameLeft, 1800.0),
        (FrameRight, 1800.0),
        (Cabin, 6000.0),
    ];
    let components = [
        (Engine, 4500.0),
        (Radiator, 450.0),
        (Battery, 180.0),
        (FuelTank, 500.0),
        (SteeringRack, 900.0),
        (SuspensionFL, 650.0),
        (SuspensionFR, 650.0),
        (SuspensionRL, 600.0),
        (SuspensionRR, 600.0),
        (WheelFL, 300.0),
        (WheelFR, 300.0),
        (WheelRL, 300.0),
        (WheelRR, 300.0),
        (Gearbox, 2800.0),
        (Driveshaft, 700.0),
        (BrakeFL, 250.0),
        (BrakeFR, 250.0),
        (BrakeRL, 220.0),
        (BrakeRR, 220.0),
        (Exhaust, 400.0),
        (Lights, 350.0),
    ];
    PriceTable {
        region_replace_cost: regions.into_iter().collect(),
        component_replace_cost: components.into_iter().collect(),
        labour_per_hour: 60.0,
        labour_hours_per_item: 3.0,
    }
}
