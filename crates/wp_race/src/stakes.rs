//! Stake table (`data/events/stakes.toml`): advertised purse, entry fee, heat cost and police
//! presence for an event, keyed by race kind, `DayPhase`, class bracket and legality.
//!
//! ```text
//! risk  = kind.risk + phase.risk + legality.risk                       (dimensionless)
//! purse = kind.base_purse × bracket.purse × risk_curve(risk)           (money)
//! entry = purse × kind.entry_frac × phase.entry × legality.entry
//! heat  = kind.heat + phase.heat + legality.heat   (0 when police presence is None)
//! ```
//!
//! A race listed under `[timed_purse.races]` is paid by its expected finishing time instead of
//! `kind.base_purse` (owner, 2026-10-26: "make the ashcroft race payout a function of expected
//! finishing time"):
//!
//! ```text
//! expected_min = course length / race.pace_kmh                          (minutes)
//! purse        = timed_purse.per_minute × expected_min × bracket.purse × risk_curve(risk)
//! ```
//!
//! The expected time is the course's (the field's winning pace), not the player's actual
//! time, so the picker can advertise it before the start and a faster car is not paid less.
//!
//! `risk_curve` is a piecewise-linear, strictly increasing table, so for a fixed kind and
//! bracket the purse is monotone in risk by construction (property-tested).
//! `wp_economy` still owns the money: these numbers are what the lobby advertises and what the
//! economy bridge passes on (see `docs/integration/race.md`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::DayPhase;

use crate::error::RaceDataError;
use crate::kind::{ClassBracket, Legality, RaceKind};
use crate::signals::PolicePresence;

/// Per-kind stake inputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KindStake {
    /// Winner purse at risk-curve 1.0 and bracket D, money.
    pub base_purse: i64,
    /// Entry fee as a fraction of the purse.
    pub entry_frac: f32,
    /// Risk contribution.
    pub risk: f32,
    /// Heat added to every racer at the start (illegal kinds), heat units.
    pub heat: f32,
}

/// Per-day-phase stake inputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseStake {
    /// Risk contribution.
    pub risk: f32,
    /// Entry multiplier.
    pub entry: f32,
    /// Heat contribution.
    pub heat: f32,
}

/// Per-bracket stake inputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BracketStake {
    /// Purse multiplier.
    pub purse: f32,
}

/// Per-legality stake inputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegalityStake {
    /// Risk contribution.
    pub risk: f32,
    /// Entry multiplier.
    pub entry: f32,
    /// Heat contribution.
    pub heat: f32,
    /// Police presence for events of this legality.
    pub police: PolicePresence,
}

/// One race paid by expected finishing time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimedRace {
    /// Expected winning average speed over the course, km/h.
    pub pace_kmh: f32,
}

/// Purses paid by expected finishing time (`[timed_purse]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimedPurse {
    /// Winner purse per expected minute at risk-curve 1.0 and bracket D, money.
    pub per_minute: f32,
    /// Keyed by race id (`wp_race::catalog`).
    pub races: BTreeMap<String, TimedRace>,
}

impl TimedPurse {
    /// Expected finishing time of `race_id` over `length_m`, minutes (`None` when the race is
    /// not paid by time).
    pub fn expected_min(&self, race_id: &str, length_m: f32) -> Option<f32> {
        let r = self.races.get(race_id)?;
        Some(length_m / (r.pace_kmh / 3.6) / 60.0)
    }
}

/// The whole `stakes.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StakeTable {
    /// `[risk, purse multiplier]` points, risk strictly ascending, multiplier strictly
    /// ascending.
    pub risk_curve: Vec<[f32; 2]>,
    /// Keyed by [`RaceKind::key`].
    pub kinds: BTreeMap<String, KindStake>,
    /// Keyed by `DayPhase`.
    pub phases: BTreeMap<DayPhase, PhaseStake>,
    /// Keyed by bracket letter.
    pub brackets: BTreeMap<String, BracketStake>,
    /// Keyed by [`Legality::key`].
    pub legality: BTreeMap<String, LegalityStake>,
    /// Races paid by expected finishing time (none when the section is absent).
    #[serde(default)]
    pub timed_purse: Option<TimedPurse>,
}

/// Everything an event puts at stake.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Stake {
    /// Advertised winner purse, money.
    pub purse: i64,
    /// Entry fee (buy-in), money.
    pub entry_fee: i64,
    /// Risk score.
    pub risk: f32,
    /// Heat each racer picks up at the start, heat units.
    pub heat: f32,
    /// Police presence.
    pub police: PolicePresence,
}

impl StakeTable {
    /// Parse and validate TOML.
    pub fn from_toml(text: &str) -> Result<StakeTable, RaceDataError> {
        let t: StakeTable =
            toml::from_str(text).map_err(|e| RaceDataError::parse("stakes.toml", e))?;
        t.validate()?;
        Ok(t)
    }

    /// Check completeness and monotonicity.
    pub fn validate(&self) -> Result<(), RaceDataError> {
        let bad = |what: &str, why: &str| Err(RaceDataError::invalid("stakes.toml", what, why));
        if self.risk_curve.len() < 2 {
            return bad("risk_curve", "needs at least two points");
        }
        for w in self.risk_curve.windows(2) {
            if !(w[0][0] < w[1][0] && w[0][1] < w[1][1]) {
                return bad(
                    "risk_curve",
                    "risk and multiplier must both strictly ascend",
                );
            }
        }
        if self.risk_curve[0][1] <= 0.0 {
            return bad("risk_curve", "multipliers must be > 0");
        }
        for k in RaceKind::KEYS {
            match self.kinds.get(k) {
                None => return bad(&format!("kinds.{k}"), "missing"),
                Some(s) if s.base_purse <= 0 || s.entry_frac < 0.0 || s.risk < 0.0 => {
                    return bad(
                        &format!("kinds.{k}"),
                        "base_purse > 0, entry_frac/risk >= 0",
                    )
                }
                _ => {}
            }
        }
        for p in [
            DayPhase::Dawn,
            DayPhase::Day,
            DayPhase::Dusk,
            DayPhase::Night,
        ] {
            if !self.phases.contains_key(&p) {
                return bad(&format!("phases.{p:?}"), "missing");
            }
        }
        let mut last = 0.0;
        for b in ClassBracket::ALL {
            match self.brackets.get(b.letter()) {
                None => return bad(&format!("brackets.{}", b.letter()), "missing"),
                Some(s) if s.purse <= last => {
                    return bad("brackets", "purse multiplier must ascend D → X")
                }
                Some(s) => last = s.purse,
            }
        }
        for l in [
            Legality::Sanctioned,
            Legality::Street,
            Legality::IllegalDaytime,
        ] {
            if !self.legality.contains_key(l.key()) {
                return bad(&format!("legality.{}", l.key()), "missing");
            }
        }
        if self.legality[Legality::IllegalDaytime.key()].police != PolicePresence::Max {
            return bad("legality.illegal_daytime.police", "must be Max");
        }
        if let Some(t) = &self.timed_purse {
            if t.per_minute <= 0.0 {
                return bad("timed_purse.per_minute", "must be > 0");
            }
            for (id, r) in &t.races {
                if r.pace_kmh <= 0.0 {
                    return bad(&format!("timed_purse.races.{id}.pace_kmh"), "must be > 0");
                }
            }
        }
        Ok(())
    }

    /// Purse multiplier for a risk score (clamped to the curve's ends).
    pub fn risk_multiplier(&self, risk: f32) -> f32 {
        let c = &self.risk_curve;
        if risk <= c[0][0] {
            return c[0][1];
        }
        for w in c.windows(2) {
            if risk <= w[1][0] {
                let t = (risk - w[0][0]) / (w[1][0] - w[0][0]);
                return w[0][1] + t * (w[1][1] - w[0][1]);
            }
        }
        c[c.len() - 1][1]
    }

    /// The stake for catalog race `race_id` over a `length_m` course: [`StakeTable::stake`],
    /// except that a race listed under `[timed_purse]` is paid by its expected finishing time
    /// (the buy-in, heat and police follow from that purse as usual).
    pub fn race_stake(
        &self,
        race_id: &str,
        length_m: f32,
        kind: RaceKind,
        phase: DayPhase,
        bracket: ClassBracket,
        legality: Legality,
    ) -> Stake {
        let mut stake = self.stake(kind, phase, bracket, legality);
        let Some(t) = &self.timed_purse else {
            return stake;
        };
        let Some(minutes) = t.expected_min(race_id, length_m) else {
            return stake;
        };
        let k = &self.kinds[kind.key()];
        let p = &self.phases[&phase];
        let b = &self.brackets[bracket.letter()];
        let l = &self.legality[legality.key()];
        stake.purse =
            (t.per_minute * minutes * b.purse * self.risk_multiplier(stake.risk)).round() as i64;
        stake.entry_fee = (stake.purse as f32 * k.entry_frac * p.entry * l.entry).round() as i64;
        stake
    }

    /// The stake for an event.
    pub fn stake(
        &self,
        kind: RaceKind,
        phase: DayPhase,
        bracket: ClassBracket,
        legality: Legality,
    ) -> Stake {
        let k = &self.kinds[kind.key()];
        let p = &self.phases[&phase];
        let b = &self.brackets[bracket.letter()];
        let l = &self.legality[legality.key()];
        let risk = k.risk + p.risk + l.risk;
        let purse = (k.base_purse as f32 * b.purse * self.risk_multiplier(risk)).round() as i64;
        let entry_fee = (purse as f32 * k.entry_frac * p.entry * l.entry).round() as i64;
        Stake {
            purse,
            entry_fee,
            risk,
            // Police ignore sanctioned events entirely.
            heat: if l.police == PolicePresence::None {
                0.0
            } else {
                (k.heat + p.heat + l.heat).max(0.0)
            },
            police: l.police,
        }
    }
}
