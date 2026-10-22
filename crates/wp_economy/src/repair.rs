//! Repair billing: price tables for `wp_damage`, damage reports from the sim, garage repair
//! options (`RepairAll`, `RepairSelected`, `DriveDamaged`, `Scrap`), shop behaviour and
//! insurance claims.
//!
//! Billing re-prices each estimate line from its `parts_cost` and `labour_hours`:
//! `line = (parts + hours × class labour rate × district × shop markup) × mode repair_cost_scale`.
//! `parts_cost` / `labour_hours` are taken as *unscaled* (the mode scale is applied here once);
//! see the contract change requests in `docs/integration/economy.md`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::{
    DamageMode, HandlingEffects, PriceTable, RepairEstimate, RepairItem, RepairLine, VehicleClass,
};
use wp_contracts::{DistrictId, WantedLevel};

use crate::data::Economy;
use crate::error::EconomyError;
use crate::ledger::{
    transact, CarLocation, CarUid, DamageSnapshot, Delta, EconResult, EventReason, Money, OwnedCar,
    PlayerAccount,
};

/// Kind of repair shop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShopKind {
    /// Normal prices; reports wanted cars to the police.
    Legit,
    /// Marked-up labour (more for wanted cars); never reports.
    Crooked,
}

/// Where the car is being repaired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShopContext {
    /// District of the shop (labour multiplier from `prices.district_labour`).
    pub district: DistrictId,
    /// Shop kind.
    pub kind: ShopKind,
    /// The car's current wanted level.
    pub wanted: WantedLevel,
}

/// Parse a district key: a contract `DistrictId` name (`"Core"`, `"Port"`, ..) or one of the
/// pre-v0.3.0 `prices.toml` keys (`"default"` → `Residential`, `"Docks"` → `Port`, `"Hills"` →
/// `Suburb`). Case-sensitive.
pub fn district_from_key(key: &str) -> Option<DistrictId> {
    let legacy = match key {
        "default" => Some(DistrictId::Residential),
        "Docks" => Some(DistrictId::Port),
        "Hills" => Some(DistrictId::Suburb),
        _ => None,
    };
    legacy.or_else(|| {
        DistrictId::ALL
            .iter()
            .copied()
            .find(|d| format!("{d:?}") == key)
    })
}

/// Contract string for a district (its variant name; `ShopReported.district` is a `String` in
/// v0.3.0).
pub fn district_key(d: DistrictId) -> String {
    format!("{d:?}")
}

impl ShopContext {
    /// A legit shop in a residential district (labour × 1.0) with a car that is not wanted.
    pub fn legit_default() -> Self {
        Self {
            district: DistrictId::Residential,
            kind: ShopKind::Legit,
            wanted: WantedLevel::None,
        }
    }
}

/// What the player chooses at the garage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RepairOption {
    /// Fix every outstanding line.
    RepairAll,
    /// Fix only these items.
    RepairSelected(Vec<RepairItem>),
    /// Pay nothing; the car keeps its lattice state and handling effects.
    DriveDamaged,
    /// Sell the car for salvage.
    Scrap,
}

/// One priced line of a quote.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotedLine {
    /// Item.
    pub item: RepairItem,
    /// Severity 0..=1.
    pub severity: f32,
    /// Parts (mode-scaled).
    pub parts: Money,
    /// Labour (rate × multipliers, mode-scaled).
    pub labour: Money,
    /// Parts + labour.
    pub total: Money,
}

/// A priced repair decision, before anything is paid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepairQuote {
    /// Lines being repaired.
    pub lines: Vec<QuotedLine>,
    /// District × shop labour multiplier.
    pub labour_multiplier: f32,
    /// Sum of line totals.
    pub gross: Money,
    /// Paid by insurance.
    pub insurance_covers: Money,
    /// Insurance refused (damage outstanding when the car was captured).
    pub claim_denied: bool,
    /// Paid by the player.
    pub player_pays: Money,
    /// Salvage credited (Scrap only).
    pub salvage: Money,
    /// A legit shop will report the car to the police.
    pub reports_to_police: bool,
}

/// What the damage system must do to the stored lattice after a repair: restore `items`
/// (or everything when `full_reset`), then report a fresh [`DamageReport`] with `towed: false`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepairOrder {
    /// Car repaired.
    pub car: CarUid,
    /// Items restored.
    pub items: Vec<RepairItem>,
    /// Everything restored (pristine lattice).
    pub full_reset: bool,
    /// The car was scrapped and no longer exists.
    pub scrapped: bool,
}

/// Damage state reported by the sim when a session ends (or after a repair order is applied).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DamageReport {
    /// Car.
    pub car: CarUid,
    /// Global health 0..=1 (`wp_damage::DamageModel::health`).
    pub health: f32,
    /// Total loss reached.
    pub total_loss: bool,
    /// Current handling effects.
    pub handling: HandlingEffects,
    /// Current repair estimate (`DamageSim::repair_estimate` with [`Economy::price_table`]).
    pub estimate: RepairEstimate,
    /// Opaque lattice state to restore next session.
    pub lattice_state: Vec<u8>,
    /// The car was wanted at some point during the session. Informational: insurance covers
    /// damage taken while wanted; only a capture denies the claim.
    pub wanted_during_session: bool,
    /// The car had to be towed off the street (charged as a penalty).
    pub towed: bool,
}

/// Severity at and above which a part is replaced rather than repaired. Same value as
/// `wp_damage::repair::REPLACE_SEVERITY` (a test keeps them equal).
pub const REPLACE_SEVERITY: f32 = 0.6;
/// Parts fraction used when the table has none. Same as `wp_damage::repair::REPAIR_PARTS_FRACTION`.
pub const DEFAULT_REPAIR_PARTS_FRACTION: f32 = 0.3;
/// Minimum labour as a fraction of an item's full hours. Same as
/// `wp_damage::repair::MIN_LABOUR_FRACTION`.
pub const MIN_LABOUR_FRACTION: f32 = 0.1;
/// Lines below this severity are not billed. Same as `wp_damage::repair::MIN_SEVERITY`.
pub const MIN_SEVERITY: f32 = 0.02;

/// Price one line exactly like `wp_damage` does: at `severity >= REPLACE_SEVERITY` the full part
/// price, below it `repair_parts_fraction × severity / REPLACE_SEVERITY` of it; labour is the
/// item's hours (`PriceTable::labour_hours`, else `labour_hours_per_item`) × max(severity,
/// MIN_LABOUR_FRACTION). `parts_cost` / `labour_hours` are unscaled; `total` includes
/// `repair_cost_scale` (contract v0.2.0).
pub fn reference_line(
    table: &PriceTable,
    repair_cost_scale: f32,
    item: RepairItem,
    severity: f32,
) -> RepairLine {
    let severity = severity.clamp(0.0, 1.0);
    let cost = match item {
        RepairItem::Region(r) | RepairItem::DetachedPart(r) => {
            table.region_replace_cost.get(&r).copied().unwrap_or(0.0)
        }
        RepairItem::Component(c) => table.component_replace_cost.get(&c).copied().unwrap_or(0.0),
    };
    let parts = if severity >= REPLACE_SEVERITY {
        cost
    } else {
        let fraction = table
            .repair_parts_fraction
            .unwrap_or(DEFAULT_REPAIR_PARTS_FRACTION);
        cost * fraction * severity / REPLACE_SEVERITY
    };
    let full_hours = table
        .labour_hours
        .get(&item)
        .copied()
        .unwrap_or(table.labour_hours_per_item);
    let hours = full_hours * severity.max(MIN_LABOUR_FRACTION);
    RepairLine {
        item,
        severity,
        parts_cost: parts,
        labour_hours: hours,
        total: (parts + hours * table.labour_per_hour) * repair_cost_scale,
    }
}

/// Turn damage severities into an estimate with `wp_damage`'s pricing ([`reference_line`]).
/// Duplicate items keep the highest severity; detached parts are always full replacements;
/// lines under [`MIN_SEVERITY`] are dropped. Used by tests and the career scenario, which have
/// no lattice to simulate.
pub fn reference_estimate(
    table: &PriceTable,
    repair_cost_scale: f32,
    damage: &[(RepairItem, f32)],
) -> RepairEstimate {
    let mut sev: BTreeMap<RepairItem, f32> = BTreeMap::new();
    for (item, s) in damage {
        let s = match item {
            RepairItem::DetachedPart(_) => 1.0,
            _ => s.clamp(0.0, 1.0),
        };
        if s >= MIN_SEVERITY {
            let e = sev.entry(*item).or_insert(0.0);
            *e = e.max(s);
        }
    }
    let mut est = RepairEstimate::default();
    for (item, s) in sev {
        let line = reference_line(table, repair_cost_scale, item, s);
        est.total += line.total;
        est.lines.push(line);
    }
    est
}

fn money(v: f32) -> Money {
    v.round() as Money
}

impl Economy {
    /// Contract [`PriceTable`] for one car, handed to `DamageSim::repair_estimate`. Parts scale
    /// with the class and with `car_value / class reference value` (clamped).
    pub fn price_table(&self, class: VehicleClass, car_value: Money) -> PriceTable {
        let cp = &self.prices.class[&class];
        let [lo, hi] = self.prices.value_factor_clamp;
        let vf = (car_value as f32 / cp.reference_value).clamp(lo, hi);
        let k = cp.parts_scale * vf;
        PriceTable {
            region_replace_cost: self
                .prices
                .region_parts
                .iter()
                .map(|(r, c)| (*r, c * k))
                .collect(),
            component_replace_cost: self
                .prices
                .component_parts
                .iter()
                .map(|(c, v)| (*c, v * k))
                .collect(),
            labour_per_hour: cp.labour_per_hour,
            labour_hours_per_item: self.prices.labour_hours_per_item,
            labour_hours: self.labour_hours(),
            repair_parts_fraction: Some(self.prices.repair_parts_fraction),
        }
    }

    /// Per-item labour hours for `PriceTable::labour_hours`: region hours (also for detached
    /// parts) and component hours from `prices.toml`. Labour does not scale with car value.
    pub fn labour_hours(&self) -> BTreeMap<RepairItem, f32> {
        let p = &self.prices;
        let mut out = BTreeMap::new();
        for (r, h) in &p.region_labour_hours {
            out.insert(RepairItem::Region(*r), *h);
            out.insert(RepairItem::DetachedPart(*r), *h);
        }
        for (c, h) in &p.component_labour_hours {
            out.insert(RepairItem::Component(*c), *h);
        }
        out
    }

    /// Price table for an owned car (uses its pristine value).
    pub fn price_table_for(&self, car: &OwnedCar) -> PriceTable {
        self.price_table(car.class, self.car_value(car))
    }

    /// District × shop labour multiplier.
    pub fn labour_multiplier(&self, shop: &ShopContext) -> f32 {
        let district = self.prices.district_labour[&shop.district];
        let markup = match shop.kind {
            ShopKind::Legit => 1.0,
            ShopKind::Crooked if shop.wanted > WantedLevel::None => {
                self.prices.shop.crooked_wanted_markup
            }
            ShopKind::Crooked => self.prices.shop.crooked_markup,
        };
        district * markup
    }

    /// Salvage value: `base × (min_frac + health_frac × health) + mods_frac × mods price`.
    pub fn scrap_value(&self, car: &OwnedCar) -> Money {
        let s = &self.prices.scrap;
        let health = if car.damage.total_loss {
            car.damage.health.min(0.25)
        } else {
            car.damage.health
        }
        .clamp(0.0, 1.0);
        let mods = self.catalog.price_of(&car.mods) as f32;
        // The body's salvage never exceeds what the dealer would pay for it, so buying a cheap
        // car (or the floor car) and scrapping it is never a profit.
        let body = money(car.base_price as f32 * (s.min_frac + s.health_frac * health)).min(money(
            self.market.dealer_buy_frac * self.resale_basis(car) as f32,
        ));
        body + money(s.mods_frac * mods)
    }

    /// Price an arbitrary set of estimate lines for a car at a shop.
    pub fn price_lines(
        &self,
        car: &OwnedCar,
        lines: &[RepairLine],
        shop: &ShopContext,
        mode: DamageMode,
    ) -> (Vec<QuotedLine>, Money) {
        let rate = self.prices.class[&car.class].labour_per_hour;
        let mult = self.labour_multiplier(shop);
        let scale = self.mode_table(mode).repair_cost_scale;
        let mut gross = 0;
        let quoted = lines
            .iter()
            .map(|l| {
                let parts = money(l.parts_cost * scale);
                let labour = money(l.labour_hours * rate * mult * scale);
                gross += parts + labour;
                QuotedLine {
                    item: l.item,
                    severity: l.severity,
                    parts,
                    labour,
                    total: parts + labour,
                }
            })
            .collect();
        (quoted, gross)
    }

    /// Price a repair option without paying.
    pub fn quote_repair(
        &self,
        acct: &PlayerAccount,
        car: CarUid,
        option: &RepairOption,
        shop: &ShopContext,
        mode: DamageMode,
    ) -> Result<RepairQuote, EconomyError> {
        let c = acct.try_car(car)?;
        if c.location != CarLocation::Garage {
            return Err(EconomyError::CarUnavailable {
                car,
                reason: "in the pound; retrieve it first".into(),
            });
        }
        let outstanding = &c.damage.outstanding.lines;
        let lines: Vec<RepairLine> = match option {
            RepairOption::RepairAll => {
                if c.damage.is_pristine() {
                    return Err(EconomyError::NothingToRepair);
                }
                outstanding.clone()
            }
            RepairOption::RepairSelected(items) => {
                if items.is_empty() {
                    return Err(EconomyError::NothingToRepair);
                }
                let mut v = Vec::new();
                for it in items {
                    match outstanding.iter().find(|l| l.item == *it) {
                        Some(l) => v.push(l.clone()),
                        None => {
                            return Err(EconomyError::InvalidInput(format!(
                                "{it:?} is not on the outstanding estimate"
                            )))
                        }
                    }
                }
                v
            }
            RepairOption::DriveDamaged => {
                if !c.damage.is_drivable() {
                    return Err(EconomyError::CarUnavailable {
                        car,
                        reason: "not drivable (total loss, dead engine or lost wheel)".into(),
                    });
                }
                Vec::new()
            }
            RepairOption::Scrap => Vec::new(),
        };
        let (quoted, gross) = self.price_lines(c, &lines, shop, mode);
        let mut covers = 0;
        let mut denied = false;
        if let (Some(pol), true) = (&c.insurance, gross > 0) {
            if c.damage.sustained_before_capture {
                denied = true;
            } else {
                covers = money(pol.coverage * (gross - pol.deductible).max(0) as f32);
            }
        }
        let is_repair = matches!(
            option,
            RepairOption::RepairAll | RepairOption::RepairSelected(_)
        );
        Ok(RepairQuote {
            lines: quoted,
            labour_multiplier: self.labour_multiplier(shop),
            gross,
            insurance_covers: covers,
            claim_denied: denied,
            player_pays: gross - covers,
            salvage: if *option == RepairOption::Scrap {
                self.scrap_value(c)
            } else {
                0
            },
            reports_to_police: is_repair
                && shop.kind == ShopKind::Legit
                && shop.wanted > WantedLevel::None
                && shop.wanted >= self.prices.shop.legit_reports_at,
        })
    }

    /// Execute a repair option. Returns the order the damage system must apply to the stored
    /// lattice state.
    pub fn repair(
        &self,
        acct: &mut PlayerAccount,
        car: CarUid,
        option: &RepairOption,
        shop: &ShopContext,
        mode: DamageMode,
    ) -> EconResult<RepairOrder> {
        let quote = self.quote_repair(acct, car, option, shop, mode)?;
        transact(acct, |tx| {
            let some = Some(car);
            match option {
                RepairOption::Scrap => {
                    let removed = tx.remove_car(car)?;
                    tx.credit(
                        quote.salvage,
                        EventReason::CarScrapped {
                            def_id: removed.def_id,
                        },
                        some,
                    );
                    tx.acct.stats.cars_scrapped += 1;
                    return Ok(RepairOrder {
                        car,
                        items: Vec::new(),
                        full_reset: false,
                        scrapped: true,
                    });
                }
                RepairOption::DriveDamaged => {
                    tx.note(EventReason::DriveDamaged, some, Delta::default());
                    return Ok(RepairOrder {
                        car,
                        items: Vec::new(),
                        full_reset: false,
                        scrapped: false,
                    });
                }
                _ => {}
            }
            tx.spend(
                quote.player_pays,
                EventReason::RepairPaid {
                    lines: quote.lines.len() as u32,
                    labour_multiplier: quote.labour_multiplier,
                },
                some,
            )?;
            tx.acct.stats.repairs_paid += quote.player_pays;
            if quote.insurance_covers > 0 {
                tx.note(
                    EventReason::InsuranceClaim {
                        covered: quote.insurance_covers,
                    },
                    some,
                    Delta::default(),
                );
            }
            if quote.claim_denied {
                tx.note(EventReason::InsuranceClaimDenied, some, Delta::default());
            }
            if quote.reports_to_police {
                tx.note(
                    EventReason::ShopReported {
                        district: shop.district,
                        level: shop.wanted,
                    },
                    some,
                    Delta::default(),
                );
            }
            let items: Vec<RepairItem> = quote.lines.iter().map(|l| l.item).collect();
            let c = tx.car_mut(car)?;
            let before = c.damage.outstanding.total;
            c.damage
                .outstanding
                .lines
                .retain(|l| !items.contains(&l.item));
            let remaining: f32 = c.damage.outstanding.lines.iter().map(|l| l.total).sum();
            let full_reset = c.damage.outstanding.lines.is_empty();
            if full_reset {
                c.damage = DamageSnapshot::pristine();
            } else {
                let repaired_frac = if before > 0.0 {
                    ((before - remaining) / before).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                c.damage.outstanding.total = remaining;
                c.damage.health += (1.0 - c.damage.health) * repaired_frac;
            }
            Ok(RepairOrder {
                car,
                items,
                full_reset,
                scrapped: false,
            })
        })
    }

    /// Store the damage state reported at the end of a session (replaces the previous
    /// snapshot; the damage sim was spawned from it). Charges towing if `towed`.
    pub fn record_damage(&self, acct: &mut PlayerAccount, report: &DamageReport) -> EconResult<()> {
        transact(acct, |tx| {
            let some = Some(report.car);
            let class = {
                let c = tx.car_mut(report.car)?;
                // A capture flag survives as long as there is unrepaired damage; being wanted
                // without being caught does not affect insurance.
                let was_captured =
                    c.damage.sustained_before_capture && !report.estimate.lines.is_empty();
                c.damage = DamageSnapshot {
                    health: report.health.clamp(0.0, 1.0),
                    total_loss: report.total_loss,
                    handling: report.handling,
                    outstanding: report.estimate.clone(),
                    lattice_state: report.lattice_state.clone(),
                    sustained_before_capture: was_captured,
                };
                self.strip_empty_slot_lines(c);
                c.class
            };
            tx.note(
                EventReason::DamageRecorded {
                    health: report.health,
                    total_loss: report.total_loss,
                },
                some,
                Delta::default(),
            );
            if report.towed {
                tx.penalty(self.prices.class[&class].towing, EventReason::Towing, some);
            }
            Ok(())
        })
    }
}
