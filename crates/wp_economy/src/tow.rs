//! The tow service: "Call a tow truck" in a career run and "Tow to …" in the garage.
//!
//! A car that cannot be driven (or a player who asks during a run) is towed to a repair shop or
//! to one of the player's own garages. The price is a flat call-out fee plus a charge per road
//! kilometre (`prices.toml [tow]`, [`crate::data::TowPricing`]); the road distance is the
//! straight-line distance × `route_factor`, rounded to 100 m ([`Economy::tow_metres`]).
//!
//! Paying follows the ledger's cash rule: a tow is a penalty-type charge ([`EventReason::Tow`]),
//! but only a tow **on account** (the caller passes it for the nearest repair shop) may push
//! cash below zero. Any other destination must be affordable from cash on hand, so a player
//! with no money can always get the wreck to the nearest shop and nothing more; the shortfall
//! is ordinary debt (it blocks voluntary spends until winnings or sales pay it off, and the
//! economy-floor beater is free while in debt, so the career never dead-ends).
//!
//! Where the cars are, the police rules (no tow while wanted unless the car is totaled) and
//! the map geometry belong to the caller (`wp_wasm::tow`); this module only prices and posts.

use serde::{Deserialize, Serialize};

use crate::data::Economy;
use crate::error::EconomyError;
use crate::ledger::{transact, CarLocation, CarUid, EconResult, EventReason, Money, PlayerAccount};

/// One tow to price and charge ([`Economy::tow_car`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TowJob {
    /// Destination name for the ledger line (shop or garage name).
    pub to: String,
    /// Road distance, m ([`Economy::tow_metres`]).
    pub metres: u32,
    /// Charge it even if the player cannot pay (debt): the nearest repair shop only.
    pub on_account: bool,
}

impl Economy {
    /// Road distance of a tow over `straight_m` metres in a straight line: × `route_factor`,
    /// rounded to the nearest 100 m.
    pub fn tow_metres(&self, straight_m: f32) -> u32 {
        let road = straight_m.max(0.0) * self.prices.tow.route_factor;
        ((road / 100.0).round() * 100.0) as u32
    }

    /// Price of a tow of `metres` by road: call-out + per-km × km (rounded to whole money).
    pub fn tow_price(&self, metres: u32) -> Money {
        let t = &self.prices.tow;
        t.callout + (t.per_km * metres as Money + 500) / 1000
    }

    /// Tow `car` (posts one [`EventReason::Tow`] line). The car must be the player's and not
    /// in the pound (the impound has its own release flow). Refused with `InsufficientFunds`
    /// when the price is more than the cash on hand, unless `job.on_account`.
    pub fn tow_car(
        &self,
        acct: &mut PlayerAccount,
        car: CarUid,
        job: &TowJob,
    ) -> EconResult<Money> {
        let price = self.tow_price(job.metres);
        transact(acct, |tx| {
            let c = tx.car_mut(car)?;
            if c.location != CarLocation::Garage {
                return Err(EconomyError::CarUnavailable {
                    car,
                    reason: "the car is in the pound: release it there".into(),
                });
            }
            if !job.on_account && tx.acct.cash < price {
                return Err(EconomyError::InsufficientFunds {
                    needed: price,
                    available: tx.acct.cash,
                });
            }
            tx.penalty(
                price,
                EventReason::Tow {
                    to: job.to.clone(),
                    metres: job.metres,
                },
                Some(car),
            );
            Ok(price)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::garage::Condition;
    use wp_contracts::{PlayerId, WantedLevel};

    fn econ() -> Economy {
        Economy::embedded()
    }

    fn with_car(e: &Economy, cash: Money) -> (PlayerAccount, CarUid) {
        let (mut a, _) = e.open_account(PlayerId(7));
        a.cash = 1_000_000;
        a.rep = 1_000_000;
        let spec = crate::garage::CarSpec {
            def_id: "vireo_xs".into(),
            class: wp_contracts::VehicleClass::Compact,
            base_price: 18_000,
        };
        let (car, _) = e.buy_car(&mut a, &spec, Condition::Worn).unwrap();
        a.cash = cash;
        a.rep = 0;
        (a, car)
    }

    #[test]
    fn price_is_callout_plus_per_km_by_road() {
        let e = econ();
        let t = e.prices.tow;
        assert_eq!(e.tow_price(0), t.callout);
        assert_eq!(e.tow_price(1000), t.callout + t.per_km);
        assert_eq!(e.tow_price(4200), t.callout + (t.per_km * 42 + 5) / 10);
        // Straight line × route factor, to the nearest 100 m.
        let m = e.tow_metres(1000.0);
        assert_eq!(
            m,
            ((1000.0 * t.route_factor / 100.0).round() * 100.0) as u32
        );
        assert_eq!(m % 100, 0);
        assert_eq!(e.tow_metres(-5.0), 0);
        assert!(e.tow_metres(3000.0) > e.tow_metres(2000.0));
    }

    #[test]
    fn tuned_for_an_early_career() {
        let e = econ();
        let start = e.market.starting_cash;
        // A local tow (1 km apart) is a few hundred; a cross-map tow (~5.7 km apart, Harnell
        // to Ashpeak City) costs a meaningful slice of the starting cash, but well under it.
        let local = e.tow_price(e.tow_metres(1000.0));
        let cross = e.tow_price(e.tow_metres(5700.0));
        assert!((200..=400).contains(&local), "local {local}");
        assert!(
            cross >= start / 8 && cross <= start / 3,
            "cross {cross} vs start {start}"
        );
    }

    #[test]
    fn a_tow_posts_one_ledger_line() {
        let e = econ();
        let (mut a, car) = with_car(&e, 5_000);
        let job = TowJob {
            to: "Meridian Motorworks".into(),
            metres: 4200,
            on_account: false,
        };
        let (price, ev) = e.tow_car(&mut a, car, &job).unwrap();
        assert_eq!(price, e.tow_price(4200));
        assert_eq!(a.cash, 5_000 - price);
        assert_eq!(ev.len(), 1);
        assert_eq!(
            ev[0].reason,
            EventReason::Tow {
                to: "Meridian Motorworks".into(),
                metres: 4200
            }
        );
        assert_eq!(ev[0].cash_delta, -price);
        assert_eq!(ev[0].car, Some(car));
        assert_eq!(ev[0].reason.kind_code(), EventReason::Towing.kind_code());
        assert!(ev[0].reason.is_penalty());
    }

    #[test]
    fn short_of_cash_only_the_tow_on_account_goes_through_as_debt() {
        let e = econ();
        let (mut a, car) = with_car(&e, 100);
        let far = TowJob {
            to: "Spire Auto Body".into(),
            metres: 7400,
            on_account: false,
        };
        let before = a.clone();
        let err = e.tow_car(&mut a, car, &far).unwrap_err();
        assert!(
            matches!(err, EconomyError::InsufficientFunds { .. }),
            "{err:?}"
        );
        assert_eq!(a, before, "a refused tow changes nothing");
        let nearest = TowJob {
            to: "Gantry Garage".into(),
            metres: 1300,
            on_account: true,
        };
        let (price, _) = e.tow_car(&mut a, car, &nearest).unwrap();
        assert!(price > 100);
        assert_eq!(a.cash, 100 - price, "the shortfall is debt");
        assert_eq!(a.debt(), price - 100);
        // In debt, the nearest shop still tows (more debt); nowhere else does.
        assert!(e.tow_car(&mut a, car, &far).is_err());
        assert!(e.tow_car(&mut a, car, &nearest).is_ok());
    }

    #[test]
    fn a_car_in_the_pound_is_not_towed() {
        let e = econ();
        let (mut a, car) = with_car(&e, 5_000);
        a.car_mut(car).unwrap().location = CarLocation::Impound {
            since_day: 0,
            level: WantedLevel::L2,
        };
        let job = TowJob {
            to: "Gantry Garage".into(),
            metres: 1000,
            on_account: true,
        };
        assert!(matches!(
            e.tow_car(&mut a, car, &job),
            Err(EconomyError::CarUnavailable { .. })
        ));
        assert_eq!(a.cash, 5_000);
    }
}
