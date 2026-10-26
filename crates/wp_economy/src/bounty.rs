//! What the career bounty ([`PlayerAccount::bounty`]) does besides the bail a capture takes
//! (`capture.rs`): paying it off in the garage, and the high-stakes races it opens and pays a
//! bonus on (`stakes.toml` `[bounty.payoff]`, `[bounty.high_stakes]`).
//!
//! The police side (a wanted career gets backup sooner and heavier) lives in `wp_police`
//! (`PoliceSystem::carry_career_bounty`); the browser career passes the bounty in at the start
//! of every run.

use wp_contracts::Legality;

use crate::data::Economy;
use crate::ledger::{transact, Delta, EventReason, Money, PlayerAccount};
use crate::{EconResult, EconomyError};

impl Economy {
    /// What paying off the whole bounty costs now (0 without a bounty).
    pub fn bounty_payoff_cost(&self, acct: &PlayerAccount) -> Money {
        if acct.bounty == 0 {
            return 0;
        }
        let p = &self.stakes.bounty.payoff;
        ((acct.bounty as f32 * p.cost_frac).round() as Money).max(p.min_cost)
    }

    /// Pay off the whole bounty (one `BountyCleared` line with the cost and the bounty).
    /// Refused without a bounty or without the cash; returns what it cost.
    pub fn pay_off_bounty(&self, acct: &mut PlayerAccount) -> EconResult<Money> {
        let bounty = acct.bounty;
        if bounty == 0 {
            return Err(EconomyError::InvalidInput(
                "there is no bounty to pay off".into(),
            ));
        }
        let cost = self.bounty_payoff_cost(acct);
        transact(acct, |tx| {
            tx.post(
                EventReason::BountyCleared,
                None,
                Delta {
                    cash: -cost,
                    bounty: -(bounty as i64),
                    ..Delta::default()
                },
            )?;
            Ok(cost)
        })
    }

    /// The race legality is high stakes (`[bounty.high_stakes] legality`).
    pub fn is_high_stakes(&self, legality: Legality) -> bool {
        self.stakes
            .bounty
            .high_stakes
            .legality
            .iter()
            .any(|k| k == legality.key())
    }

    /// Career bounty a race of this legality needs (0 = open to everyone).
    pub fn high_stakes_bounty_needed(&self, legality: Legality) -> u32 {
        if self.is_high_stakes(legality) {
            self.stakes.bounty.high_stakes.min_bounty
        } else {
            0
        }
    }

    /// Refuse a high-stakes race to a career whose bounty is too small.
    pub fn require_high_stakes_bounty(
        &self,
        acct: &PlayerAccount,
        legality: Legality,
    ) -> Result<(), EconomyError> {
        let needed = self.high_stakes_bounty_needed(legality);
        if acct.bounty < needed {
            return Err(EconomyError::InvalidInput(format!(
                "high-stakes race: invite only, needs a bounty of {needed} (you have {})",
                acct.bounty
            )));
        }
        Ok(())
    }

    /// Purse bonus (fraction of the purse) a career with `bounty` gets in a race of this
    /// legality: 0 outside high-stakes races.
    pub fn bounty_purse_bonus(&self, bounty: u32, legality: Legality) -> f32 {
        if !self.is_high_stakes(legality) {
            return 0.0;
        }
        let hs = &self.stakes.bounty.high_stakes;
        (bounty as f32 / 1000.0 * hs.purse_bonus_per_1000).min(hs.purse_bonus_max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_contracts::PlayerId;

    fn econ() -> Economy {
        Economy::embedded()
    }

    fn acct(cash: Money, bounty: u32) -> PlayerAccount {
        let mut a = PlayerAccount::empty(PlayerId(1));
        a.cash = cash;
        a.bounty = bounty;
        a
    }

    #[test]
    fn payoff_clears_the_bounty_for_cash() {
        let e = econ();
        let mut a = acct(10_000, 2_000);
        let cost = e.bounty_payoff_cost(&a);
        assert_eq!(cost, 1_500);
        let (paid, ev) = e.pay_off_bounty(&mut a).unwrap();
        assert_eq!(paid, cost);
        assert_eq!(a.bounty, 0);
        assert_eq!(a.cash, 10_000 - cost);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].bounty_delta, -2_000);
        assert_eq!(ev[0].cash_delta, -cost);
    }

    #[test]
    fn payoff_refused_without_cash_or_bounty() {
        let e = econ();
        let mut a = acct(100, 2_000);
        let before = a.clone();
        assert!(e.pay_off_bounty(&mut a).is_err());
        assert_eq!(a, before);
        let mut b = acct(10_000, 0);
        assert!(e.pay_off_bounty(&mut b).is_err());
        assert_eq!(e.bounty_payoff_cost(&b), 0);
    }

    #[test]
    fn payoff_dearer_than_bail() {
        let e = econ();
        let a = acct(0, 3_000);
        let bail = a.bounty as f32 * e.consequences.bail_bounty_frac;
        assert!(e.bounty_payoff_cost(&a) as f32 > bail);
    }

    #[test]
    fn high_stakes_gate_and_bonus() {
        let e = econ();
        assert!(e.is_high_stakes(Legality::IllegalDaytime));
        assert!(!e.is_high_stakes(Legality::Street));
        assert_eq!(e.high_stakes_bounty_needed(Legality::Street), 0);
        let need = e.high_stakes_bounty_needed(Legality::IllegalDaytime);
        assert!(need > 0);
        assert!(e
            .require_high_stakes_bounty(&acct(0, need - 1), Legality::IllegalDaytime)
            .is_err());
        assert!(e
            .require_high_stakes_bounty(&acct(0, need), Legality::IllegalDaytime)
            .is_ok());
        assert!(e
            .require_high_stakes_bounty(&acct(0, 0), Legality::Street)
            .is_ok());
        assert_eq!(e.bounty_purse_bonus(5_000, Legality::Street), 0.0);
        let small = e.bounty_purse_bonus(1_000, Legality::IllegalDaytime);
        let big = e.bounty_purse_bonus(1_000_000, Legality::IllegalDaytime);
        assert!(small > 0.0 && small < big);
        assert!((big - e.stakes.bounty.high_stakes.purse_bonus_max).abs() < 1e-6);
    }
}
