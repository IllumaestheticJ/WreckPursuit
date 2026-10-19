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
