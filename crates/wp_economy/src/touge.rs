//! Pink slips against NPC racers (touge battles, `wp_race::touge`).
//!
//! [`Economy::settle_pink_slip`] moves a car between two player accounts. A touge boss or
//! challenger has no account, so these two halves do it against the house instead: the player
//! either takes the NPC's car exactly as it finished the race ([`Economy::take_pink_slip_car`]:
//! its damage snapshot and paint, no insurance, valued at the model's market basis) or hands
//! over their own
//! ([`Economy::lose_pink_slip_car`]). Who may put a pink slip up is decided by the caller
//! through `Feature::PinkSlips` (the rep tiers), not by `stakes.toml [pink_slip]`, which only
//! governs player-vs-player slips.
//!
//! A battle's rep, boss purse and side bets go through the career calls
//! ([`Economy::award_rep`], [`Economy::award_credits`], [`Economy::place_bet`] /
//! [`Economy::settle_bet`] with `BetVenue::Touge`): the one betting wallet.

use wp_contracts::VehicleClass;

use crate::ledger::{
    transact, CarLocation, CarUid, DamageSnapshot, Delta, EconResult, EventReason, OwnedCar,
};
use crate::{Economy, PlayerAccount};

/// The NPC car a player wins in a pink slip, as it crossed the line.
#[derive(Debug, Clone, PartialEq)]
pub struct WonCar {
    /// Car id (`data/cars`).
    pub def_id: String,
    /// Gameplay class.
    pub class: VehicleClass,
    /// Base price of the model.
    pub base_price: u32,
    /// Installed mod ids.
    pub mods: Vec<String>,
    /// Body colour 0xRRGGBB the NPC raced in (`None` = the garage's default colour), so the
    /// car in the garage looks like the one beaten.
    pub paint: Option<u32>,
    /// The car's damage at the finish (or where it stopped), priced like any end-of-session
    /// report (`Economy::record_damage` fields). The full lattice state, when the caller keeps
    /// one (the browser career's `CareerProfile::lattices`), is stored by the caller under the
    /// returned uid.
    pub damage: DamageSnapshot,
}

impl Economy {
    /// Add the car won from an NPC to the player's garage, damage and paint as it raced. Its
    /// resale basis (`purchase_price`) is the model's [`Economy::market_basis`] (the cheapest
    /// dealer price), so a won car repaired sells for a real amount, still below what any
    /// dealer charges for it. Posts `PinkSlipWon`. Returns the new car's uid.
    pub fn take_pink_slip_car(&self, acct: &mut PlayerAccount, car: WonCar) -> EconResult<CarUid> {
        let basis = self.market_basis(&car.def_id, car.class, car.base_price);
        transact(acct, |tx| {
            let def_id = car.def_id.clone();
            let uid = tx.add_car(OwnedCar {
                uid: CarUid(0),
                def_id: car.def_id,
                class: car.class,
                base_price: car.base_price,
                mods: car.mods,
                damage: car.damage,
                location: CarLocation::Garage,
                insurance: None,
                purchase_price: Some(basis),
                tune: Default::default(),
                parts: None,
                tires: Default::default(),
                variant_name: None,
                paint: car.paint,
            });
            tx.note(
                EventReason::PinkSlipWon { def_id },
                Some(uid),
                Delta::default(),
            );
            Ok(uid)
        })
    }

    /// Hand the player's car to an NPC (removed from the garage, counted as lost; insurance
    /// does not pay). Posts `PinkSlipLost`. Returns the car as it left.
    pub fn lose_pink_slip_car(
        &self,
        acct: &mut PlayerAccount,
        car: CarUid,
    ) -> EconResult<OwnedCar> {
        transact(acct, |tx| {
            let lost = tx.remove_car(car)?;
            tx.acct.stats.cars_lost += 1;
            tx.note(
                EventReason::PinkSlipLost {
                    def_id: lost.def_id.clone(),
                },
                Some(car),
                Delta::default(),
            );
            Ok(lost)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_contracts::PlayerId;

    fn won(health: f32) -> WonCar {
        WonCar {
            def_id: "hakone_gt".into(),
            class: VehicleClass::Sport,
            base_price: 48_000,
            mods: vec!["engine_stage1".into()],
            paint: Some(0x2a7fd0),
            damage: DamageSnapshot {
                health,
                ..DamageSnapshot::pristine()
            },
        }
    }

    #[test]
    fn won_car_keeps_its_finishing_damage() {
        let econ = Economy::embedded();
        let (mut acct, _) = econ.open_account(PlayerId(7));
        let before = acct.garage.len();
        let (uid, events) = econ.take_pink_slip_car(&mut acct, won(0.62)).unwrap();
        assert_eq!(acct.garage.len(), before + 1);
        let car = acct.car(uid).unwrap();
        assert_eq!(car.damage.health, 0.62);
        assert_eq!(
            car.purchase_price,
            Some(econ.market_basis("hakone_gt", VehicleClass::Sport, 48_000))
        );
        assert_eq!(car.paint, Some(0x2a7fd0), "keeps the colour it raced in");
        assert!(car.insurance.is_none());
        assert!(matches!(events[0].reason, EventReason::PinkSlipWon { .. }));
    }

    #[test]
    fn lost_car_leaves_the_garage() {
        let econ = Economy::embedded();
        let (mut acct, _) = econ.open_account(PlayerId(7));
        let (uid, _) = econ.take_pink_slip_car(&mut acct, won(1.0)).unwrap();
        let (car, events) = econ.lose_pink_slip_car(&mut acct, uid).unwrap();
        assert_eq!(car.def_id, "hakone_gt");
        assert!(acct.car(uid).is_none());
        assert_eq!(acct.stats.cars_lost, 1);
        assert!(matches!(events[0].reason, EventReason::PinkSlipLost { .. }));
        assert!(econ.lose_pink_slip_car(&mut acct, uid).is_err());
    }

    #[test]
    fn won_car_repaired_sells_for_a_real_amount_below_the_dealer_price() {
        use crate::{CarSpec, Condition};
        let econ = Economy::embedded();
        let (mut acct, _) = econ.open_account(PlayerId(7));
        let mut wreck = won(0.4);
        wreck.mods.clear();
        wreck.damage.outstanding.total = 20_000.0;
        let (uid, _) = econ.take_pink_slip_car(&mut acct, wreck).unwrap();
        let damaged = econ.sale_price(acct.car(uid).unwrap());
        // Repaired: no damage outstanding.
        acct.car_mut(uid).unwrap().damage = DamageSnapshot::pristine();
        let car = acct.car(uid).unwrap();
        let sale = econ.sale_price(car);
        assert!(
            sale > damaged,
            "repair raises the price: {damaged} -> {sale}"
        );
        let spec = CarSpec {
            def_id: car.def_id.clone(),
            class: car.class,
            base_price: car.base_price,
        };
        let cheapest_dealer = Condition::ALL
            .iter()
            .map(|c| econ.market_price(&spec, *c))
            .min()
            .unwrap();
        assert!(
            sale >= cheapest_dealer / 2,
            "a repaired won car is worth real money: {sale}"
        );
        assert!(
            sale < cheapest_dealer,
            "never more than a dealer charges: {sale} vs {cheapest_dealer}"
        );
    }
}
