//! Pursuit → economy wiring.
//!
//! `wp_police` reports pursuits as the typed contract `SimEvent::Police` (v0.3.0).
//! [`PoliceSignal::from_sim_event`] extracts the two facts the economy needs and a small
//! per-racer [`PursuitTracker`] turns a pursuit ending into a [`PursuitResolution`]. The caller
//! maps the racer's `EntityId` to an account + car and calls [`Economy::settle_pursuit`], which
//! dispatches to [`Economy::handle_capture`] / [`Economy::handle_escape`]. (`PursuitEnded` now
//! also carries `level`, `capturer` and `assists`; paying player cops from them is G's next
//! step.)
//!
//! Event order matters: at a capture `wp_police` emits `PursuitEnded` **before** the
//! `WantedLevelChanged` that drops the level, so the tracker's last known level is the level at
//! capture. Violations are counted from `SimEvent::TrafficViolation` since the racer's last
//! capture (they scale the fine).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::{DamageMode, DayPhase, EntityId, PoliceEvent, SimEvent, WantedLevel};
use wp_math::Rng;

use crate::capture::{CaptureContext, CaptureOutcome};
use crate::data::Economy;
use crate::ledger::{CarUid, EconResult, PlayerAccount, ViolationCounts};

/// How a pursuit ended: the contract `PursuitOutcome` (v0.3.0; the old local mirror is gone).
pub use wp_contracts::PursuitOutcome as PursuitEnd;

/// The police facts the economy consumes, decoded from `SimEvent::PoliceEvent` by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PoliceSignal {
    /// `PoliceEvent::WantedLevelChanged`.
    WantedLevelChanged {
        /// Racer vehicle.
        racer: EntityId,
        /// New level.
        level: WantedLevel,
    },
    /// `PoliceEvent::PursuitEnded`.
    PursuitEnded {
        /// Racer vehicle.
        racer: EntityId,
        /// Outcome.
        outcome: PursuitEnd,
        /// Wanted level when the pursuit ended, as carried by the v0.3.0 `PursuitEnded` event
        /// (`None` if unknown: the tracker's last `WantedLevelChanged` is used).
        level: WantedLevel,
    },
}

/// A pursuit that ended, with what the economy needs to settle it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PursuitResolution {
    /// Racer vehicle.
    pub racer: EntityId,
    /// Outcome.
    pub outcome: PursuitEnd,
    /// Wanted level when the pursuit ended.
    pub level: WantedLevel,
    /// Highest wanted level during the pursuit.
    pub max_level: WantedLevel,
    /// Violations since the racer's last capture (or since tracking started).
    pub violations: ViolationCounts,
}

impl PoliceSignal {
    /// Decode the economy-relevant police facts from a drained `SimEvent` (v0.3.0 typed
    /// `SimEvent::Police`). `None` for everything else.
    pub fn from_sim_event(ev: &SimEvent) -> Option<Self> {
        match ev {
            SimEvent::Police(PoliceEvent::WantedLevelChanged { racer, level }) => {
                Some(PoliceSignal::WantedLevelChanged {
                    racer: *racer,
                    level: *level,
                })
            }
            SimEvent::Police(PoliceEvent::PursuitEnded {
                racer,
                outcome,
                level,
                ..
            }) => Some(PoliceSignal::PursuitEnded {
                racer: *racer,
                outcome: *outcome,
                level: *level,
            }),
            _ => None,
        }
    }
}

/// What [`Economy::settle_pursuit`] did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PursuitSettlement {
    /// Capture consequences applied.
    Captured(CaptureOutcome),
    /// Escape bounty and rep applied (escapes and safehouse dwells).
    Escaped {
        /// Wanted level escaped from.
        level: WantedLevel,
    },
    /// Nothing to settle (called off, or ended while not wanted).
    NoChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct RacerTrack {
    level: WantedLevel,
    max_level: WantedLevel,
    violations: ViolationCounts,
}

/// Per-racer pursuit bookkeeping between police events and the economy. Deterministic
/// (`BTreeMap`), cheap, and owned by whoever drains the sim's events.
#[derive(Debug, Clone, Default)]
pub struct PursuitTracker {
    racers: BTreeMap<EntityId, RacerTrack>,
}

impl PursuitTracker {
    /// Empty tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed every drained `SimEvent` (counts `TrafficViolation`s per vehicle). Police events
    /// go through [`PursuitTracker::observe_police`] after decoding.
    pub fn observe_sim_event(&mut self, ev: &SimEvent) {
        if let SimEvent::TrafficViolation { vehicle, kind, .. } = ev {
            self.racers
                .entry(*vehicle)
                .or_default()
                .violations
                .record(kind);
        }
    }

    /// Feed a decoded police signal. Returns a resolution when a pursuit ends.
    pub fn observe_police(&mut self, sig: &PoliceSignal) -> Option<PursuitResolution> {
        match *sig {
            PoliceSignal::WantedLevelChanged { racer, level } => {
                let t = self.racers.entry(racer).or_default();
                t.level = level;
                t.max_level = t.max_level.max(level);
                None
            }
            PoliceSignal::PursuitEnded {
                racer,
                outcome,
                level,
            } => {
                let t = self.racers.entry(racer).or_default();
                // The event's level covers heat set without a `WantedLevelChanged` (v0.3.0).
                let at_end = t.level.max(level);
                let res = PursuitResolution {
                    racer,
                    outcome,
                    level: at_end,
                    max_level: t.max_level.max(at_end),
                    violations: t.violations,
                };
                // The next pursuit's maximum starts fresh (its current level is folded in
                // when it resolves); a capture also wipes the violations it was fined for.
                t.max_level = WantedLevel::None;
                if matches!(
                    outcome,
                    PursuitEnd::Captured | PursuitEnd::CapturedTotalLoss
                ) {
                    t.violations = ViolationCounts::default();
                }
                Some(res)
            }
        }
    }

    /// Current wanted level the tracker knows for a racer.
    pub fn level(&self, racer: EntityId) -> WantedLevel {
        self.racers
            .get(&racer)
            .map_or(WantedLevel::None, |t| t.level)
    }

    /// Forget a racer (left the instance).
    pub fn remove(&mut self, racer: EntityId) {
        self.racers.remove(&racer);
    }
}

impl Economy {
    /// Settle a finished pursuit on the racer's account: captures go to
    /// [`Economy::handle_capture`] (total loss flagged for `CapturedTotalLoss`), escapes and
    /// safehouse dwells to [`Economy::handle_escape`] with the day phase's bounty multiplier;
    /// called-off pursuits and pursuits that ended with no wanted level change nothing.
    pub fn settle_pursuit(
        &self,
        acct: &mut PlayerAccount,
        car: CarUid,
        res: &PursuitResolution,
        mode: DamageMode,
        phase: DayPhase,
        rng: &mut Rng,
    ) -> EconResult<PursuitSettlement> {
        match res.outcome {
            PursuitEnd::Captured | PursuitEnd::CapturedTotalLoss => {
                let ctx = CaptureContext {
                    mode,
                    wanted: res.level,
                    car,
                    total_loss: res.outcome == PursuitEnd::CapturedTotalLoss,
                    session_violations: res.violations,
                };
                let (out, ev) = self.handle_capture(acct, &ctx, rng)?;
                Ok((PursuitSettlement::Captured(out), ev))
            }
            PursuitEnd::Escaped | PursuitEnd::Safehouse if res.level > WantedLevel::None => {
                let mult = self.stakes.day_phase[&phase].bounty;
                let ((), ev) = self.handle_escape(acct, res.level, mult)?;
                Ok((PursuitSettlement::Escaped { level: res.level }, ev))
            }
            _ => Ok((PursuitSettlement::NoChange, Vec::new())),
        }
    }
}
