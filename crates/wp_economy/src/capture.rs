//! Getting caught (fines, bail, rep loss, impound, seizure), escaping, the pound, the daily tick
//! and the cop career (XP and capture rewards).
//!
//! Mode differences (`consequences.toml [mode.*]`):
//! - **Arcade**: always impound + fine. A wrecked car is impounded wrecked.
//! - **Simulation**: impound + fine, plus a 30 % seizure chance at wanted level 4–5, and a
//!   `TotalLoss` at capture means the car is gone.

use serde::{Deserialize, Serialize};
use wp_contracts::DamageMode;
use wp_contracts::WantedLevel;
use wp_math::Rng;

use crate::data::Economy;
use crate::error::EconomyError;
use crate::ledger::{
    transact, CarLocation, CarUid, CopXpReason, Delta, EconResult, EventReason, Money,
    PlayerAccount, Txn, ViolationCounts,
};

/// Everything the economy needs to know about a capture (from `wp_police`'s capture event).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureContext {
    /// Damage mode of the instance.
    pub mode: DamageMode,
    /// Wanted level at capture.
    pub wanted: WantedLevel,
    /// Car the racer was driving.
    pub car: CarUid,
    /// The car was a total loss at capture.
    pub total_loss: bool,
    /// Violations committed this session.
    pub session_violations: ViolationCounts,
}

/// What happened to the car.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CarFate {
    /// Moved to the pound (retrievable for a growing fee).
    Impounded,
    /// Seized by the police (gone).
    Seized,
    /// Destroyed: total loss while wanted in Simulation (gone).
    Destroyed,
}

/// Summary of a capture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureOutcome {
    /// Fine charged.
    pub fine: Money,
    /// Bail charged.
    pub bail: Money,
    /// Rep lost.
    pub rep_loss: i64,
    /// Bounty cleared (police heat for this racer resets; prompt 06 should zero heat).
    pub bounty_cleared: u32,
    /// Car fate.
    pub car_fate: CarFate,
}

/// Something a player cop did that changes XP.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CopAction {
    /// Captured a racer.
    Capture {
        /// Racer's wanted level.
        level: WantedLevel,
        /// Racer's bounty at capture.
        racer_bounty: u32,
    },
    /// Assisted a capture.
    Assist {
        /// Racer's wanted level.
        level: WantedLevel,
        /// Racer's bounty at capture.
        racer_bounty: u32,
    },
    /// Minutes of driving with no incident.
    CleanDriving {
        /// Minutes.
        minutes: u32,
    },
    /// Hit a civilian.
    CivilianCollision,
    /// Wrecked own unit.
    SelfWreck,
    /// Kept pushing after the risk budget was spent (`overspent` = budgets over, 1.0 = one).
    RiskBudgetOverspent {
        /// Budgets overspent.
        overspent: f32,
    },
}

fn money(v: f32) -> Money {
    v.round() as Money
}

impl Economy {
    /// Fine for a capture (before it is charged).
    pub fn capture_fine(&self, acct: &PlayerAccount, ctx: &CaptureContext) -> Money {
        let c = &self.consequences;
        let base = c.fine_by_level[&ctx.wanted] as f32;
        let priors = acct.heat_history.captures.min(c.prior_capture_cap) as f32;
        let f = &c.fine_per_violation;
        let per = |v: &ViolationCounts| -> f32 {
            (v.speeding as i64 * f.speeding
                + v.red_light as i64 * f.red_light
                + v.wrong_way as i64 * f.wrong_way
                + v.collision as i64 * f.collision
                + v.reckless as i64 * f.reckless) as f32
        };
        let session = per(&ctx.session_violations);
        let lifetime = (per(&acct.heat_history.violations) * c.lifetime_violation_weight)
            .min(base * c.lifetime_violation_cap);
        let scale = c.mode[&ctx.mode].fine_scale;
        money((base * (1.0 + c.fine_per_prior_capture * priors) + session + lifetime) * scale)
    }

    /// Apply a capture: fine and bail (may create debt), rep loss, bounty cleared, insurance
    /// voided, car impounded / seized / destroyed. `rng` decides Simulation seizures.
    pub fn handle_capture(
        &self,
        acct: &mut PlayerAccount,
        ctx: &CaptureContext,
        rng: &mut Rng,
    ) -> EconResult<CaptureOutcome> {
        acct.try_car(ctx.car)?;
        let fine = self.capture_fine(acct, ctx);
        let bail = money(acct.bounty as f32 * self.consequences.bail_bounty_frac);
        let rep_loss = self.consequences.rep_loss_by_level[&ctx.wanted];
        let rules = &self.consequences.mode[&ctx.mode];
        let fate = if ctx.total_loss && rules.total_loss_destroys_car {
            CarFate::Destroyed
        } else if ctx.wanted >= rules.seizure_min_level
            && ctx.wanted > WantedLevel::None
            && rng.chance(rules.seizure_chance)
        {
            CarFate::Seized
        } else {
            CarFate::Impounded
        };
        transact(acct, |tx| {
            let car = Some(ctx.car);
            let level = ctx.wanted;
            tx.penalty(fine, EventReason::Fine { level }, car);
            if bail > 0 {
                tx.penalty(bail, EventReason::Bail, car);
            }
            tx.note(
                EventReason::CaptureRepLoss { level },
                car,
                Delta {
                    rep: -rep_loss,
                    ..Delta::default()
                },
            );
            let bounty = tx.acct.bounty;
            if bounty > 0 {
                tx.note(
                    EventReason::BountyCleared,
                    car,
                    Delta {
                        bounty: -(bounty as i64),
                        ..Delta::default()
                    },
                );
            }
            let h = &mut tx.acct.heat_history;
            h.captures += 1;
            h.max_wanted = h.max_wanted.max(level);
            h.violations.add(&ctx.session_violations);
            h.last_capture_day = Some(tx.acct.day);

            if level > WantedLevel::None {
                let c = tx.car_mut(ctx.car)?;
                c.damage.sustained_while_wanted = true;
                if c.insurance.take().is_some() {
                    tx.note(EventReason::InsuranceVoided, car, Delta::default());
                }
            }
            match fate {
                CarFate::Impounded => {
                    let day = tx.acct.day;
                    let c = tx.car_mut(ctx.car)?;
                    c.location = CarLocation::Impound {
                        since_day: day,
                        level,
                    };
                    if tx.acct.active_car == Some(ctx.car) {
                        tx.acct.active_car = None;
                    }
                    tx.note(EventReason::CarImpounded, car, Delta::default());
                }
                CarFate::Seized | CarFate::Destroyed => {
                    let removed = tx.remove_car(ctx.car)?;
                    tx.acct.stats.cars_lost += 1;
                    let def_id = removed.def_id;
                    let reason = if fate == CarFate::Seized {
                        EventReason::CarSeized { def_id }
                    } else {
                        EventReason::CarDestroyed { def_id }
                    };
                    tx.note(reason, car, Delta::default());
                }
            }
            Ok(CaptureOutcome {
                fine,
                bail,
                rep_loss,
                bounty_cleared: bounty,
                car_fate: fate,
            })
        })
    }

    /// Apply an escape: bounty and rep by wanted level (scaled by the night bounty multiplier
    /// via `phase_bounty`, normally `stakes.day_phase[phase].bounty`).
    pub fn handle_escape(
        &self,
        acct: &mut PlayerAccount,
        level: WantedLevel,
        phase_bounty: f32,
    ) -> EconResult<()> {
        let b = &self.stakes.bounty;
        let bounty = money(b.escape[&level] as f32 * phase_bounty);
        let rep = b.escape_rep[&level];
        transact(acct, |tx| {
            tx.note(
                EventReason::Escape { level },
                None,
                Delta {
                    rep,
                    bounty,
                    ..Delta::default()
                },
            );
            let h = &mut tx.acct.heat_history;
            h.escapes += 1;
            h.max_wanted = h.max_wanted.max(level);
            Ok(())
        })
    }

    /// Current fee to get a car out of the pound.
    pub fn retrieval_fee(&self, acct: &PlayerAccount, car: CarUid) -> Result<Money, EconomyError> {
        let c = acct.try_car(car)?;
        match c.location {
            CarLocation::Impound { since_day, .. } => {
                let days = acct.day.saturating_sub(since_day) as Money;
                let i = &self.consequences.impound;
                Ok(i.base_fee[&c.class] + i.per_day[&c.class] * days)
            }
            CarLocation::Garage => Err(EconomyError::CarUnavailable {
                car,
                reason: "not impounded".into(),
            }),
        }
    }

    /// Pay the retrieval fee (voluntary: must be affordable) and move the car to the garage.
    pub fn retrieve_from_impound(&self, acct: &mut PlayerAccount, car: CarUid) -> EconResult<()> {
        let fee = self.retrieval_fee(acct, car)?;
        transact(acct, |tx| {
            let days = match tx.car_mut(car)?.location {
                CarLocation::Impound { since_day, .. } => tx.acct.day.saturating_sub(since_day),
                CarLocation::Garage => 0,
            };
            tx.spend(fee, EventReason::ImpoundRetrieved { days }, Some(car))?;
            tx.car_mut(car)?.location = CarLocation::Garage;
            if tx.acct.active_car.is_none() {
                tx.acct.active_car = Some(car);
            }
            Ok(())
        })
    }

    /// Advance the account by one in-game day: charge insurance premiums (lapse if
    /// unaffordable), auction cars left in the pound too long.
    pub fn end_of_day(&self, acct: &mut PlayerAccount) -> EconResult<()> {
        transact(acct, |tx| {
            tx.acct.day += 1;
            let day = tx.acct.day;
            let uids: Vec<CarUid> = tx.acct.garage.iter().map(|c| c.uid).collect();
            for uid in uids {
                self.daily_car_tick(tx, uid, day)?;
            }
            Ok(())
        })
    }

    fn daily_car_tick(&self, tx: &mut Txn, uid: CarUid, day: u32) -> Result<(), EconomyError> {
        let imp = &self.consequences.impound;
        let car = tx.acct.try_car(uid)?.clone();
        if let CarLocation::Impound { since_day, .. } = car.location {
            if day.saturating_sub(since_day) >= imp.auction_after_days {
                let fees = imp.base_fee[&car.class]
                    + imp.per_day[&car.class] * day.saturating_sub(since_day) as Money;
                let value = self.current_value(&car);
                let back = money((value - fees).max(0) as f32 * imp.auction_return_frac);
                tx.remove_car(uid)?;
                tx.acct.stats.cars_lost += 1;
                tx.credit(
                    back,
                    EventReason::CarAuctioned {
                        def_id: car.def_id.clone(),
                    },
                    Some(uid),
                );
                return Ok(());
            }
        }
        if let Some(pol) = &car.insurance {
            if tx.acct.cash >= pol.premium_per_day {
                tx.spend(
                    pol.premium_per_day,
                    EventReason::InsurancePremium,
                    Some(uid),
                )?;
            } else {
                tx.car_mut(uid)?.insurance = None;
                tx.note(EventReason::InsuranceLapsed, Some(uid), Delta::default());
            }
        }
        Ok(())
    }

    /// Cop rank (0 = recruit) for an XP total.
    pub fn cop_level(&self, xp: u32) -> u8 {
        self.consequences
            .cop_rank_thresholds
            .iter()
            .take_while(|t| xp >= **t)
            .count() as u8
    }

    /// Award (or take) cop XP and pay capture / assist rewards.
    pub fn award_cop(&self, acct: &mut PlayerAccount, action: CopAction) -> EconResult<()> {
        let x = &self.consequences.cop_xp;
        let b = &self.stakes.bounty;
        transact(acct, |tx| {
            let reward = |level: WantedLevel, bounty: u32| -> Money {
                b.cop_base_reward[&level] + money(b.cop_share * bounty as f32)
            };
            let (reason, xp, cash, level) = match action {
                CopAction::Capture {
                    level,
                    racer_bounty,
                } => {
                    tx.acct.stats.cop_captures += 1;
                    (
                        CopXpReason::Capture,
                        x.capture[&level],
                        reward(level, racer_bounty),
                        Some(level),
                    )
                }
                CopAction::Assist {
                    level,
                    racer_bounty,
                } => {
                    tx.acct.stats.cop_assists += 1;
                    let cash = money(reward(level, racer_bounty) as f32 * b.assist_share);
                    (CopXpReason::Assist, x.assist[&level], cash, Some(level))
                }
                CopAction::CleanDriving { minutes } => (
                    CopXpReason::CleanDriving,
                    x.clean_minute * minutes as i64,
                    0,
                    None,
                ),
                CopAction::CivilianCollision => (
                    CopXpReason::CivilianCollision,
                    x.civilian_collision,
                    0,
                    None,
                ),
                CopAction::SelfWreck => (CopXpReason::SelfWreck, x.self_wreck, 0, None),
                CopAction::RiskBudgetOverspent { overspent } => (
                    CopXpReason::RiskBudgetOverspent,
                    (x.risk_overspend as f32 * overspent.max(0.0)).round() as i64,
                    0,
                    None,
                ),
            };
            tx.note(
                EventReason::CopXp { reason },
                None,
                Delta {
                    cop_xp: xp,
                    ..Delta::default()
                },
            );
            if let (Some(level), true) = (level, cash > 0) {
                tx.credit(cash, EventReason::CopReward { level }, None);
            }
            Ok(())
        })
    }
}

/// Convenience for the server: the capturing cop's [`CopAction`] for a racer capture.
pub fn capture_action(level: WantedLevel, racer_bounty: u32) -> CopAction {
    CopAction::Capture {
        level,
        racer_bounty,
    }
}
