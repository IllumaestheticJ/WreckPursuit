//! Garage API: accounts, buying / selling cars, the economy-floor beater, mod install /
//! uninstall, build preview, valuation and insurance.

use serde::{Deserialize, Serialize};
use wp_contracts::{DamageMode, LatticeDef, PlayerId, VehicleClass, VehicleDef};

use crate::data::Economy;
use crate::error::EconomyError;
use crate::estimate::{bracket, class_index, PerfMetrics};
use crate::ledger::{
    transact, CarLocation, CarUid, DamageSnapshot, Delta, EconResult, EventReason, InsurancePolicy,
    Money, OwnedCar, PlayerAccount,
};
use crate::mods::{apply_mods, AppliedMods};

/// Condition a car is sold in (purchase price multiplier from `market.condition`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Condition {
    /// Showroom.
    New,
    /// Normal used car.
    Used,
    /// Tired; cheap.
    Worn,
}

impl Condition {
    /// Every condition.
    pub const ALL: [Condition; 3] = [Condition::New, Condition::Used, Condition::Worn];

    /// Health a car bought in this condition starts with (wear is cosmetic only).
    pub fn health(self) -> f32 {
        match self {
            Condition::New => 1.0,
            Condition::Used => 0.9,
            Condition::Worn => 0.75,
        }
    }
}

/// The parts of a [`VehicleDef`] the market needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarSpec {
    /// `VehicleDef::id`.
    pub def_id: String,
    /// Class.
    pub class: VehicleClass,
    /// Base price.
    pub base_price: u32,
}

impl From<&VehicleDef> for CarSpec {
    fn from(d: &VehicleDef) -> Self {
        Self {
            def_id: d.id.clone(),
            class: d.class,
            base_price: d.base_price,
        }
    }
}

/// Garage preview of a build.
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    /// Modified definitions, tools, cosmetics, detachable requests, warnings.
    pub applied: AppliedMods,
    /// Performance index.
    pub class_index: u16,
    /// Lobby bracket letter.
    pub bracket: String,
    /// Mass, kg.
    pub mass_kg: f32,
    /// Estimated 0–100 km/h, s.
    pub est_0_100_s: f32,
    /// Estimated top speed, m/s.
    pub est_top_speed_mps: f32,
    /// Full metrics.
    pub metrics: PerfMetrics,
}

fn money(v: f32) -> Money {
    v.round() as Money
}

impl Economy {
    /// Create a new account with the starting cash (and no car: see
    /// [`Economy::buy_car`] / [`Economy::issue_floor_car`]).
    pub fn open_account(&self, player: PlayerId) -> (PlayerAccount, Vec<crate::EconomyEvent>) {
        let mut acct = PlayerAccount::empty(player);
        let start = self.market.starting_cash;
        let ((), ev) = transact(&mut acct, |tx| {
            tx.credit(start, EventReason::AccountOpened, None);
            Ok(())
        })
        .expect("opening an account cannot fail");
        (acct, ev)
    }

    /// Pristine value of a car: `base price + mod_value_frac × installed mods' price`.
    pub fn car_value(&self, car: &OwnedCar) -> Money {
        car.base_price as Money
            + money(self.market.mod_value_frac * self.catalog.price_of(&car.mods) as f32)
    }

    /// Current value: pristine value minus outstanding repairs, never below
    /// `damaged_value_floor × pristine` (and a total loss is worth its salvage).
    pub fn current_value(&self, car: &OwnedCar) -> Money {
        let pristine = self.car_value(car);
        if car.damage.total_loss {
            return self.scrap_value(car);
        }
        let floor = money(pristine as f32 * self.market.damaged_value_floor);
        (pristine - money(car.damage.outstanding.total)).max(floor)
    }

    /// Dealer price for a car spec in a condition, clamped into the class band.
    pub fn market_price(&self, spec: &CarSpec, condition: Condition) -> Money {
        let [lo, hi] = self.market.class_bands[&spec.class];
        money(spec.base_price as f32 * self.market.condition[&condition]).clamp(lo, hi)
    }

    /// What the dealer pays for an owned car.
    pub fn sale_price(&self, car: &OwnedCar) -> Money {
        let mods_back =
            money(self.market.mod_resale_frac * self.catalog.price_of(&car.mods) as f32);
        let body = money(self.market.dealer_buy_frac * car.base_price as f32);
        let pristine = body + mods_back;
        let ratio = self.current_value(car) as f32 / self.car_value(car).max(1) as f32;
        money(pristine as f32 * ratio.clamp(0.0, 1.0))
    }

    /// Net worth: cash + current value of every owned car (impounded cars minus their fee).
    pub fn net_worth(&self, acct: &PlayerAccount) -> Money {
        acct.cash
            + acct
                .garage
                .iter()
                .map(|c| {
                    let fee = self.retrieval_fee(acct, c.uid).unwrap_or(0);
                    (self.current_value(c) - fee).max(0)
                })
                .sum::<Money>()
    }

    /// Buy a car from the dealer.
    pub fn buy_car(
        &self,
        acct: &mut PlayerAccount,
        spec: &CarSpec,
        condition: Condition,
    ) -> EconResult<CarUid> {
        if matches!(
            spec.class,
            VehicleClass::PoliceCruiser | VehicleClass::PoliceInterceptor | VehicleClass::PoliceSuv
        ) {
            return Err(EconomyError::InvalidInput(
                "police vehicles are not for sale".into(),
            ));
        }
        let price = self.market_price(spec, condition);
        transact(acct, |tx| {
            tx.spend(
                price,
                EventReason::CarPurchased {
                    def_id: spec.def_id.clone(),
                },
                None,
            )?;
            let uid = tx.add_car(new_car(spec, condition));
            if let Some(ev) = tx.events.last_mut() {
                ev.car = Some(uid);
            }
            Ok(uid)
        })
    }

    /// Sell a car to the dealer (it must be in the garage).
    pub fn sell_car(&self, acct: &mut PlayerAccount, car: CarUid) -> EconResult<Money> {
        let c = acct.try_car(car)?;
        if c.location != CarLocation::Garage {
            return Err(EconomyError::CarUnavailable {
                car,
                reason: "in the pound".into(),
            });
        }
        let price = self.sale_price(c);
        transact(acct, |tx| {
            let removed = tx.remove_car(car)?;
            tx.credit(
                price,
                EventReason::CarSold {
                    def_id: removed.def_id,
                },
                Some(car),
            );
            Ok(price)
        })
    }

    /// Economy floor: give the player the junkyard beater for the asking price or whatever
    /// cash they have, whichever is lower (never into debt). Only when they have no usable car.
    pub fn issue_floor_car(&self, acct: &mut PlayerAccount) -> EconResult<CarUid> {
        if acct.has_usable_car() {
            return Err(EconomyError::InvalidInput(
                "the floor car is only for players with no usable car".into(),
            ));
        }
        let f = &self.market.floor_car;
        let price = f.price.min(acct.cash.max(0));
        let spec = CarSpec {
            def_id: f.def_id.clone(),
            class: f.class,
            base_price: f.base_price,
        };
        transact(acct, |tx| {
            let uid = tx.add_car(new_car(&spec, Condition::Worn));
            tx.acct.active_car = Some(uid);
            tx.spend(
                price,
                EventReason::FloorCarIssued {
                    def_id: spec.def_id.clone(),
                },
                Some(uid),
            )?;
            Ok(uid)
        })
    }

    /// Set the default car.
    pub fn set_active_car(
        &self,
        acct: &mut PlayerAccount,
        car: CarUid,
    ) -> Result<(), EconomyError> {
        acct.try_car(car)?;
        acct.active_car = Some(car);
        Ok(())
    }

    /// Install a mod (pays the price; a replaced same-line mod is refunded at resale value).
    pub fn install_mod(
        &self,
        acct: &mut PlayerAccount,
        car: CarUid,
        mod_id: &str,
        mode: DamageMode,
    ) -> EconResult<()> {
        let c = acct.try_car(car)?;
        if c.location != CarLocation::Garage {
            return Err(EconomyError::CarUnavailable {
                car,
                reason: "in the pound".into(),
            });
        }
        let plan = self
            .catalog
            .check_install(c.class, Some(mode), &c.mods, mod_id)?;
        let m = self.catalog.try_get(mod_id)?;
        transact(acct, |tx| {
            if let Some(old) = &plan.replaces {
                self.remove_mod_in(tx, car, old)?;
            }
            tx.spend(
                m.price as Money,
                EventReason::ModInstalled {
                    mod_id: mod_id.to_string(),
                },
                Some(car),
            )?;
            let c = tx.car_mut(car)?;
            c.mods.push(mod_id.to_string());
            c.mods.sort();
            Ok(())
        })
    }

    /// Remove a mod (refund = resale value). Fails if another installed mod needs it.
    pub fn uninstall_mod(
        &self,
        acct: &mut PlayerAccount,
        car: CarUid,
        mod_id: &str,
    ) -> EconResult<()> {
        let c = acct.try_car(car)?;
        if c.location != CarLocation::Garage {
            return Err(EconomyError::CarUnavailable {
                car,
                reason: "in the pound".into(),
            });
        }
        self.catalog.check_uninstall(c.class, &c.mods, mod_id)?;
        transact(acct, |tx| self.remove_mod_in(tx, car, mod_id))
    }

    fn remove_mod_in(
        &self,
        tx: &mut crate::ledger::Txn,
        car: CarUid,
        mod_id: &str,
    ) -> Result<(), EconomyError> {
        let m = self.catalog.try_get(mod_id)?;
        let refund = money(self.market.mod_resale_frac * m.price as f32);
        let c = tx.car_mut(car)?;
        c.mods.retain(|x| x != mod_id);
        tx.credit(
            refund,
            EventReason::ModRemoved {
                mod_id: mod_id.to_string(),
            },
            Some(car),
        );
        Ok(())
    }

    /// Preview a build: modified definitions, class index, mass and estimates. `mods` must be
    /// a legal set for the car's class (whitelist, one per line, requirements met).
    pub fn preview(
        &self,
        vehicle: &VehicleDef,
        lattice: &LatticeDef,
        mods: &[String],
        mode: Option<DamageMode>,
    ) -> Result<Preview, EconomyError> {
        let set = self.catalog.validate_set(vehicle.class, mods)?;
        let applied = apply_mods(vehicle, lattice, &set, mode)?;
        let metrics = self.performance(&applied.vehicle);
        let idx = class_index(&metrics, &self.market.class_index);
        Ok(Preview {
            class_index: idx,
            bracket: bracket(idx, &self.market.class_index).to_string(),
            mass_kg: metrics.mass_kg,
            est_0_100_s: metrics.est_0_100_s,
            est_top_speed_mps: metrics.est_top_speed_mps,
            metrics,
            applied,
        })
    }

    /// Buy (or replace) insurance on a car. The first day's premium is charged now.
    pub fn buy_insurance(
        &self,
        acct: &mut PlayerAccount,
        car: CarUid,
        tier: &str,
    ) -> EconResult<()> {
        let t = self.prices.insurance.get(tier).ok_or_else(|| {
            EconomyError::InvalidInput(format!("unknown insurance tier {tier:?}"))
        })?;
        let c = acct.try_car(car)?;
        if c.location != CarLocation::Garage {
            return Err(EconomyError::CarUnavailable {
                car,
                reason: "in the pound".into(),
            });
        }
        let premium = money(t.premium_per_day_frac * self.car_value(c) as f32).max(1);
        let policy = InsurancePolicy {
            tier: tier.to_string(),
            premium_per_day: premium,
            coverage: t.coverage,
            deductible: t.deductible,
            since_day: acct.day,
        };
        transact(acct, |tx| {
            tx.spend(
                premium,
                EventReason::InsurancePurchased {
                    tier: tier.to_string(),
                },
                Some(car),
            )?;
            tx.car_mut(car)?.insurance = Some(policy);
            Ok(())
        })
    }

    /// Admin / test adjustment (positive credit or voluntary-style debit).
    pub fn adjust(&self, acct: &mut PlayerAccount, cash: Money, note: &str) -> EconResult<()> {
        transact(acct, |tx| {
            tx.post(
                EventReason::Adjustment {
                    note: note.to_string(),
                },
                None,
                Delta::cash(cash),
            )
        })
    }
}

fn new_car(spec: &CarSpec, condition: Condition) -> OwnedCar {
    OwnedCar {
        uid: CarUid(0),
        def_id: spec.def_id.clone(),
        class: spec.class,
        base_price: spec.base_price,
        mods: Vec::new(),
        damage: DamageSnapshot {
            health: condition.health(),
            ..DamageSnapshot::pristine()
        },
        location: CarLocation::Garage,
        insurance: None,
    }
}
