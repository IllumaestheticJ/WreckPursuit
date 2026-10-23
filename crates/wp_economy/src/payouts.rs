//! Race purses, buy-ins, clean-race bonus, rep, pink slips and informant tips.
//!
//! `winner purse = max(min_purse, base_per_km × km) × class × day phase × mode × purse_scale × wanted`;
//! position `n` gets `placement_share[n-1]` of it; a clean race (no contact, no violations
//! beyond speeding) adds `clean_bonus` on top. Buy-in = `entry_frac × base purse × phase.entry`.

use serde::{Deserialize, Serialize};
use wp_contracts::WantedLevel;
use wp_contracts::{DamageMode, DayPhase, Legality, RaceKind, VehicleClass};

use crate::data::Economy;
use crate::error::EconomyError;
use crate::ledger::{
    transact, CarUid, Delta, EconResult, EconomyEvent, EventReason, Money, PlayerAccount,
    ViolationCounts,
};

/// Race type; keys `stakes.toml [race_types.*]` (contract type since v0.3.0).
pub use wp_contracts::RaceType;

/// A race as set up in the lobby (prompt 07).
///
/// Races from `wp_race` carry their contract `kind` and `legality`; build them with
/// [`Economy::race_setup`]. When `kind` is set, `stakes.toml [race_kinds]` decides which race
/// type prices the race (and is recorded in the ledger), whatever `race_type` says.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RaceSetup {
    /// Type (used when `kind` is `None`, e.g. street meets and the career harness).
    pub race_type: RaceType,
    /// Contract race kind, for races scheduled by `wp_race`.
    #[serde(default)]
    pub kind: Option<RaceKind>,
    /// Legality (default `Street`).
    #[serde(default)]
    pub legality: Legality,
    /// Class bracket of the race.
    pub class: VehicleClass,
    /// Route length, m.
    pub route_length_m: f32,
    /// Day phase at the start.
    pub phase: DayPhase,
    /// Damage mode of the instance.
    pub mode: DamageMode,
    /// Multiplier on the winner purse so the economy pays the stake the lobby advertised
    /// (`wp_race::bridge::economy_race`, R5: the event stake table is flat per kind, the
    /// economy prices per km). 1 = the economy's own purse.
    #[serde(default = "unit_scale")]
    pub purse_scale: f32,
    /// Multiplier on the buy-in, for the same reason. 1 = the economy's own buy-in.
    #[serde(default = "unit_scale")]
    pub entry_scale: f32,
}

fn unit_scale() -> f32 {
    1.0
}

/// One racer's result (server-validated).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RaceResult {
    /// Finishing position, 1 = win, 0 = did not finish.
    pub placement: u8,
    /// Contacts with other vehicles / walls during the race.
    pub contacts: u32,
    /// Violations during the race.
    pub violations: ViolationCounts,
    /// Highest wanted level during the race.
    pub wanted: WantedLevel,
}

impl RaceResult {
    /// No contact and no violations beyond speeding.
    pub fn is_clean(&self) -> bool {
        self.placement > 0 && self.contacts == 0 && self.violations.non_speeding() == 0
    }
}

/// What a race paid.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RaceSettlement {
    /// Purse share.
    pub purse: Money,
    /// Clean-race bonus.
    pub clean_bonus: Money,
    /// Rep change.
    pub rep: i64,
    /// Bounty gained.
    pub bounty: u32,
}

/// Both racers agreed to a pink slip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinkSlipConsent {
    /// Winner consented before the race.
    pub winner: bool,
    /// Loser consented before the race.
    pub loser: bool,
}

fn money(v: f32) -> Money {
    v.round() as Money
}

impl Economy {
    /// The race type that prices a contract race kind (`stakes.toml [race_kinds]`).
    pub fn race_type_for(&self, kind: &RaceKind) -> RaceType {
        self.stakes.race_kinds[kind.key()].race_type
    }

    /// A `RaceSetup` for a race scheduled by `wp_race` (kind + legality; R5).
    pub fn race_setup(
        &self,
        kind: RaceKind,
        legality: Legality,
        class: VehicleClass,
        route_length_m: f32,
        phase: DayPhase,
        mode: DamageMode,
    ) -> RaceSetup {
        RaceSetup {
            race_type: self.race_type_for(&kind),
            kind: Some(kind),
            legality,
            class,
            route_length_m,
            phase,
            mode,
            purse_scale: 1.0,
            entry_scale: 1.0,
        }
    }

    /// The race type that actually prices `s` (the kind's row when it has a kind).
    pub fn effective_race_type(&self, s: &RaceSetup) -> RaceType {
        s.kind
            .as_ref()
            .map_or(s.race_type, |k| self.race_type_for(k))
    }

    /// `(purse, entry)` multipliers from the race kind and legality.
    fn kind_multipliers(&self, s: &RaceSetup) -> (f32, f32) {
        let (kp, ke) = s.kind.as_ref().map_or((1.0, 1.0), |k| {
            let k = &self.stakes.race_kinds[k.key()];
            (k.purse, k.entry)
        });
        let l = &self.stakes.legality[s.legality.key()];
        (kp * l.purse, ke * l.entry)
    }

    fn base_purse_raw(&self, s: &RaceSetup) -> f32 {
        let rt = &self.stakes.race_types[&self.effective_race_type(s)];
        let km = (s.route_length_m / 1000.0).clamp(0.0, rt.max_km);
        (rt.base_per_km * km).max(rt.min_purse as f32)
            * self.stakes.class_purse[&s.class]
            * self.kind_multipliers(s).0
    }

    /// Winner's purse before the wanted multiplier and clean bonus.
    pub fn base_purse(&self, s: &RaceSetup) -> Money {
        money(
            self.base_purse_raw(s)
                * self.stakes.day_phase[&s.phase].purse
                * self.stakes.mode_purse[&s.mode]
                * s.purse_scale,
        )
    }

    /// Buy-in for a race.
    pub fn entry_fee(&self, s: &RaceSetup) -> Money {
        let rt = &self.stakes.race_types[&self.effective_race_type(s)];
        money(
            self.base_purse_raw(s)
                * rt.entry_frac
                * self.kind_multipliers(s).1
                * self.stakes.day_phase[&s.phase].entry
                * s.entry_scale,
        )
    }

    /// What a result pays (pure; nothing is posted).
    pub fn race_payout(&self, s: &RaceSetup, r: &RaceResult) -> RaceSettlement {
        let st = &self.stakes;
        let phase = &st.day_phase[&s.phase];
        if r.placement == 0 {
            return RaceSettlement {
                purse: 0,
                clean_bonus: 0,
                rep: st.dnf_rep,
                bounty: 0,
            };
        }
        let idx = (r.placement - 1) as usize;
        let share = st.placement_share.get(idx).copied().unwrap_or(0.0);
        let purse = money(self.base_purse(s) as f32 * st.wanted_purse[&r.wanted] * share);
        let clean = r.is_clean();
        let clean_bonus = if clean {
            money(purse as f32 * st.clean_bonus)
        } else {
            0
        };
        let rep_base = st.placement_rep.get(idx).copied().unwrap_or(0);
        let rep =
            (rep_base as f32 * phase.rep).round() as i64 + if clean { st.clean_rep } else { 0 };
        let bounty = if r.placement == 1 && r.wanted > WantedLevel::None {
            money(st.bounty.race_won_while_wanted[&r.wanted] as f32 * phase.bounty) as u32
        } else {
            0
        };
        RaceSettlement {
            purse,
            clean_bonus,
            rep,
            bounty,
        }
    }

    /// Pay the buy-in. Blocked while in debt unless the buy-in is within
    /// `stakes.debt.max_entry_fee` (free street meets).
    pub fn enter_race(
        &self,
        acct: &mut PlayerAccount,
        s: &RaceSetup,
        car: CarUid,
    ) -> EconResult<Money> {
        let c = acct.try_car(car)?;
        if !c.is_usable() {
            return Err(EconomyError::CarUnavailable {
                car,
                reason: "impounded or undrivable".into(),
            });
        }
        let fee = self.entry_fee(s);
        if acct.in_debt() && fee > self.stakes.debt.max_entry_fee {
            return Err(EconomyError::InDebt {
                debt: acct.debt(),
                what: format!(
                    "{} race with buy-in {fee}",
                    self.effective_race_type(s).id()
                ),
            });
        }
        transact(acct, |tx| {
            if fee > 0 {
                tx.spend(
                    fee,
                    EventReason::RaceEntryFee {
                        race: self.effective_race_type(s).id().into(),
                    },
                    Some(car),
                )?;
            }
            tx.acct.stats.races_entered += 1;
            Ok(fee)
        })
    }

    /// Pay out a race result (purse share, clean bonus, rep, bounty while wanted).
    pub fn settle_race(
        &self,
        acct: &mut PlayerAccount,
        s: &RaceSetup,
        car: CarUid,
        r: &RaceResult,
    ) -> EconResult<RaceSettlement> {
        acct.try_car(car)?;
        let pay = self.race_payout(s, r);
        transact(acct, |tx| {
            let some = Some(car);
            if pay.purse > 0 {
                tx.credit(
                    pay.purse,
                    EventReason::RacePurse {
                        race: self.effective_race_type(s).id().into(),
                        placement: r.placement,
                    },
                    some,
                );
            }
            if pay.clean_bonus > 0 {
                tx.credit(pay.clean_bonus, EventReason::CleanRaceBonus, some);
            }
            tx.note(
                EventReason::RaceRep {
                    placement: r.placement,
                },
                some,
                Delta {
                    rep: pay.rep,
                    ..Delta::default()
                },
            );
            if pay.bounty > 0 {
                tx.note(
                    EventReason::BountyGained,
                    some,
                    Delta {
                        bounty: pay.bounty as i64,
                        ..Delta::default()
                    },
                );
            }
            let st = &mut tx.acct.stats;
            if r.placement == 1 {
                st.races_won += 1;
            }
            if (1..=3).contains(&r.placement) {
                st.podiums += 1;
            }
            if r.is_clean() {
                st.clean_races += 1;
            }
            tx.acct.heat_history.violations.add(&r.violations);
            Ok(pay)
        })
    }

    /// Whether a racer may put a car up in a pink-slip race in this mode.
    pub fn check_pink_slip(
        &self,
        acct: &PlayerAccount,
        car: CarUid,
        mode: DamageMode,
    ) -> Result<(), EconomyError> {
        let p = &self.stakes.pink_slip;
        if !p.modes.contains(&mode) {
            return Err(EconomyError::PinkSlipNotAllowed(format!(
                "disabled in {mode:?} mode"
            )));
        }
        if acct.rep < p.min_rep {
            return Err(EconomyError::PinkSlipNotAllowed(format!(
                "needs {} rep, has {}",
                p.min_rep, acct.rep
            )));
        }
        if acct.in_debt() {
            return Err(EconomyError::InDebt {
                debt: acct.debt(),
                what: "pink slip".into(),
            });
        }
        if !acct.try_car(car)?.is_usable() {
            return Err(EconomyError::CarUnavailable {
                car,
                reason: "impounded or undrivable".into(),
            });
        }
        Ok(())
    }

    /// Transfer the loser's car (with its mods and damage; insurance does not transfer) to the
    /// winner. Both must have consented and pass [`Economy::check_pink_slip`].
    /// Returns the car's new uid in the winner's garage and both accounts' events.
    pub fn settle_pink_slip(
        &self,
        winner: &mut PlayerAccount,
        winner_car: CarUid,
        loser: &mut PlayerAccount,
        loser_car: CarUid,
        mode: DamageMode,
        consent: PinkSlipConsent,
    ) -> Result<(CarUid, Vec<EconomyEvent>, Vec<EconomyEvent>), EconomyError> {
        if !(consent.winner && consent.loser) {
            return Err(EconomyError::PinkSlipNotAllowed(
                "both racers must consent".into(),
            ));
        }
        if winner.player == loser.player {
            return Err(EconomyError::PinkSlipNotAllowed("same player".into()));
        }
        self.check_pink_slip(winner, winner_car, mode)?;
        self.check_pink_slip(loser, loser_car, mode)?;
        let (car, loser_events) = transact(loser, |tx| {
            let car = tx.remove_car(loser_car)?;
            tx.acct.stats.cars_lost += 1;
            tx.note(
                EventReason::PinkSlipLost {
                    def_id: car.def_id.clone(),
                },
                Some(loser_car),
                Delta::default(),
            );
            Ok(car)
        })?;
        let (uid, winner_events) = transact(winner, |tx| {
            let mut car = car;
            car.insurance = None;
            let def_id = car.def_id.clone();
            let uid = tx.add_car(car);
            tx.note(
                EventReason::PinkSlipWon { def_id },
                Some(uid),
                Delta::default(),
            );
            Ok(uid)
        })?;
        Ok((uid, winner_events, loser_events))
    }

    /// Pay an informant. Returns the heat reduction `wp_police` should apply.
    pub fn buy_informant_tip(&self, acct: &mut PlayerAccount, tier: &str) -> EconResult<f32> {
        let t = self.stakes.informant.get(tier).ok_or_else(|| {
            EconomyError::InvalidInput(format!("unknown informant tier {tier:?}"))
        })?;
        transact(acct, |tx| {
            tx.spend(
                t.cost,
                EventReason::InformantTip {
                    tier: tier.to_string(),
                    heat_reduction: t.heat_reduction,
                },
                None,
            )?;
            Ok(t.heat_reduction)
        })
    }
}
