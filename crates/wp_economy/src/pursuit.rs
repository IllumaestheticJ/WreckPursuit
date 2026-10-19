//! Pursuit → economy wiring (draft for contract v0.3.0).
//!
//! `wp_police` reports pursuits through the opaque `SimEvent::PoliceEvent` today; contract
//! v0.3.0 will type them. To keep `wp_economy` free of a `wp_police` dependency, this module
//! defines the two police facts the economy needs ([`PoliceSignal`]) and a small per-racer
//! [`PursuitTracker`] that turns a pursuit ending into a [`PursuitResolution`]. The caller
//! (`wp_sim` / the server, which see both crates) decodes police events into `PoliceSignal`s
//! (see `tests/pursuit_wiring.rs` for the ten-line adapter from today's
//! `wp_police::PoliceEvent`), maps the racer's `EntityId` to an account + car, and calls
//! [`Economy::settle_pursuit`], which dispatches to [`Economy::handle_capture`] /
//! [`Economy::handle_escape`]. When v0.3.0 lands only the adapter changes.
//!
//! Event order matters: at a capture `wp_police` emits `PursuitEnded` **before** the
//! `WantedLevelChanged` that drops the level, so the tracker's last known level is the level at
//! capture (a capture with no level seen is billed at `L1`). Violations are counted from `SimEvent::TrafficViolation` since the racer's last
//! capture (they scale the fine).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::{DamageMode, DayPhase, EntityId, SimEvent, WantedLevel};
use wp_math::Rng;

use crate::capture::{CaptureContext, CaptureOutcome};
use crate::data::Economy;
use crate::ledger::{CarUid, EconResult, PlayerAccount, ViolationCounts};

/// How a pursuit ended. Mirrors `wp_police::PursuitOutcome` variant for variant (a test keeps
/// them in step) until the contract owns the type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PursuitEnd {
    /// No unit had line of sight for the escape time.
    Escaped,
    /// Quiet dwell inside a safehouse.
    Safehouse,
    /// Stopped next to a unit.
    Captured,
    /// The racer's car became a total loss while wanted.
    CapturedTotalLoss,
    /// Called off by the captain.
    CalledOff,
}

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
    },
}

/// `SimEvent::PoliceEvent::kind` of `wp_police`'s `WantedLevelChanged` (mirrors
/// `wp_police::events::kind::WANTED_LEVEL`; a test keeps them equal).
pub const POLICE_KIND_WANTED_LEVEL: u16 = 2;
/// `SimEvent::PoliceEvent::kind` of `wp_police`'s `PursuitEnded` (mirrors
/// `wp_police::events::kind::PURSUIT_ENDED`).
pub const POLICE_KIND_PURSUIT_ENDED: u16 = 4;

/// The two `wp_police::PoliceEvent` variants the economy reads, with the same serde shape
/// (externally tagged, field names as in `wp_police`). Unknown extra fields are ignored, so
/// police adding `level` / `capturer` to `PursuitEnded` does not break decoding.
#[derive(Deserialize)]
enum WirePoliceEvent {
    WantedLevelChanged {
        racer: EntityId,
        level: WantedLevel,
    },
    PursuitEnded {
        racer: EntityId,
        outcome: PursuitEnd,
    },
}

impl PoliceSignal {
    /// Decode the police facts the economy needs from a contract v0.2.0 event
    /// (`SimEvent::PoliceEvent { kind, payload }` with `wp_police`'s JSON payload). `None` for
    /// every other event, other police kinds and malformed payloads.
    ///
    /// This is the **only** place that knows the opaque police encoding: when contract v0.3.0
    /// types police events, this becomes a plain `match` on the typed variant.
    pub fn from_sim_event(ev: &SimEvent) -> Option<Self> {
        let SimEvent::PoliceEvent { kind, payload } = ev else {
            return None;
        };
        if *kind != POLICE_KIND_WANTED_LEVEL && *kind != POLICE_KIND_PURSUIT_ENDED {
            return None;
        }
        match serde_json::from_slice::<WirePoliceEvent>(payload).ok()? {
            WirePoliceEvent::WantedLevelChanged { racer, level } => {
                Some(PoliceSignal::WantedLevelChanged { racer, level })
            }
            WirePoliceEvent::PursuitEnded { racer, outcome } => {
                Some(PoliceSignal::PursuitEnded { racer, outcome })
            }
        }
    }
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
            PoliceSignal::PursuitEnded { racer, outcome } => {
                let t = self.racers.entry(racer).or_default();
                let captured = matches!(
                    outcome,
                    PursuitEnd::Captured | PursuitEnd::CapturedTotalLoss
                );
                // Police only capture wanted racers. If no level change was seen (a silent
                // heat change such as `PoliceSystem::set_heat`), bill the lowest wanted level
                // rather than a plain traffic stop. v0.3.0 puts the level in the event.
                let level = if captured {
                    t.level.max(WantedLevel::L1)
                } else {
                    t.level
                };
                let res = PursuitResolution {
                    racer,
                    outcome,
                    level,
                    max_level: t.max_level.max(level),
                    violations: t.violations,
                };
                // The next pursuit's maximum starts fresh (its current level is folded in
                // when it resolves); a capture also wipes the violations it was fined for.
                t.max_level = WantedLevel::None;
                if captured {
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
