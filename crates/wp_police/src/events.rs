//! Concrete payloads for the opaque contract variant `SimEvent::PoliceEvent { kind, payload }`.
//!
//! `kind` is [`PoliceEvent::kind`]; `payload` is the JSON encoding of the whole
//! [`PoliceEvent`] (small, debuggable; `wp_net` may switch to a binary codec later without
//! touching the variant). Decode with [`PoliceEvent::from_sim_event`].

use serde::{Deserialize, Serialize};
use wp_contracts::{EntityId, SimEvent};

use crate::heat::WantedLevel;

/// Police unit id (allocated by `wp_police`, never reused within an instance). Distinct from
/// `EntityId`: a unit may exist before `wp_sim` gives it a vehicle body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct UnitId(pub u32);

/// Roadblock / spike strip id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ToolId(pub u32);

/// Incident id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct IncidentId(pub u32);

/// How a pursuit ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PursuitOutcome {
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

/// Why a unit left service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisableCause {
    /// Crashed on its own (no racer contact this pursuit).
    SelfCrash,
    /// Wrecked after contact with a racer.
    RacerContact,
    /// Pulled over: health below the disengage threshold.
    Damaged,
    /// Pulled over: bent wheel / flat tire.
    BentWheel,
}

/// Coarse unit kind for events and views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum UnitKind {
    /// Ground patrol car.
    Car,
    /// Helicopter (kinematic, never a weapon).
    Helicopter,
}

/// Everything `wp_police` reports to the rest of the game.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PoliceEvent {
    /// Radio-log line (scanner audio, captain UI). Emitted for every assignment.
    Radio {
        /// Speaking unit (`None` = dispatch).
        unit: Option<UnitId>,
        /// Text.
        text: String,
    },
    /// A racer's wanted level changed.
    WantedLevelChanged {
        /// Racer.
        racer: EntityId,
        /// New level.
        level: WantedLevel,
    },
    /// A pursuit started.
    PursuitStarted {
        /// Racer.
        racer: EntityId,
        /// First unit in pursuit (`None` = helicopter / captain).
        unit: Option<UnitId>,
    },
    /// A pursuit ended.
    PursuitEnded {
        /// Racer.
        racer: EntityId,
        /// Outcome.
        outcome: PursuitOutcome,
    },
    /// A unit appeared. `wp_sim` spawns a vehicle body for it and binds it with
    /// `PoliceSystem::bind_unit_entity` when it drives units physically.
    UnitSpawned {
        /// Unit.
        unit: UnitId,
        /// Kind.
        kind: UnitKind,
        /// World position, m.
        pos: [f32; 3],
        /// Unit heading (XY unit vector).
        heading: [f32; 2],
    },
    /// A unit was removed.
    UnitDespawned {
        /// Unit.
        unit: UnitId,
    },
    /// A unit is out of service.
    UnitDisabled {
        /// Unit.
        unit: UnitId,
        /// Why.
        cause: DisableCause,
    },
    /// A roadblock was placed. Its colliders are in `PoliceSystem::static_colliders`.
    RoadblockPlaced {
        /// Tool.
        tool: ToolId,
        /// Racer it targets.
        racer: EntityId,
        /// Centre, m.
        pos: [f32; 3],
        /// Approach sight distance at placement, m.
        sight_m: f32,
    },
    /// A spike strip was placed (line collider from `a` to `b`).
    SpikesPlaced {
        /// Tool.
        tool: ToolId,
        /// Racer it targets.
        racer: EntityId,
        /// End A, m.
        a: [f32; 3],
        /// End B, m.
        b: [f32; 3],
        /// Approach sight distance at placement, m.
        sight_m: f32,
    },
    /// A roadblock or spike strip expired or was cleared.
    ToolRemoved {
        /// Tool.
        tool: ToolId,
    },
    /// A vehicle ran over a spike strip; flagged wheels deflate over `spikes.deflate_s`.
    SpikeHit {
        /// Vehicle.
        vehicle: EntityId,
        /// Wheels hit, FL, FR, RL, RR.
        wheels: [bool; 4],
    },
    /// Helicopter launched toward a racer.
    HeloLaunched {
        /// Helicopter unit.
        unit: UnitId,
        /// Target.
        target: EntityId,
    },
    /// A unit attempted a PIT manoeuvre (contact is resolved by physics).
    PitAttempt {
        /// Unit.
        unit: UnitId,
        /// Target.
        target: EntityId,
    },
}

/// `kind` discriminators used in `SimEvent::PoliceEvent`.
pub mod kind {
    /// [`super::PoliceEvent::Radio`].
    pub const RADIO: u16 = 1;
    /// [`super::PoliceEvent::WantedLevelChanged`].
    pub const WANTED_LEVEL: u16 = 2;
    /// [`super::PoliceEvent::PursuitStarted`].
    pub const PURSUIT_STARTED: u16 = 3;
    /// [`super::PoliceEvent::PursuitEnded`].
    pub const PURSUIT_ENDED: u16 = 4;
    /// [`super::PoliceEvent::UnitSpawned`].
    pub const UNIT_SPAWNED: u16 = 5;
    /// [`super::PoliceEvent::UnitDespawned`].
    pub const UNIT_DESPAWNED: u16 = 6;
    /// [`super::PoliceEvent::UnitDisabled`].
    pub const UNIT_DISABLED: u16 = 7;
    /// [`super::PoliceEvent::RoadblockPlaced`].
    pub const ROADBLOCK_PLACED: u16 = 8;
    /// [`super::PoliceEvent::SpikesPlaced`].
    pub const SPIKES_PLACED: u16 = 9;
    /// [`super::PoliceEvent::ToolRemoved`].
    pub const TOOL_REMOVED: u16 = 10;
    /// [`super::PoliceEvent::SpikeHit`].
    pub const SPIKE_HIT: u16 = 11;
    /// [`super::PoliceEvent::HeloLaunched`].
    pub const HELO_LAUNCHED: u16 = 12;
    /// [`super::PoliceEvent::PitAttempt`].
    pub const PIT_ATTEMPT: u16 = 13;
}

impl PoliceEvent {
    /// Discriminator for `SimEvent::PoliceEvent::kind`.
    pub fn kind(&self) -> u16 {
        match self {
            PoliceEvent::Radio { .. } => kind::RADIO,
            PoliceEvent::WantedLevelChanged { .. } => kind::WANTED_LEVEL,
            PoliceEvent::PursuitStarted { .. } => kind::PURSUIT_STARTED,
            PoliceEvent::PursuitEnded { .. } => kind::PURSUIT_ENDED,
            PoliceEvent::UnitSpawned { .. } => kind::UNIT_SPAWNED,
            PoliceEvent::UnitDespawned { .. } => kind::UNIT_DESPAWNED,
            PoliceEvent::UnitDisabled { .. } => kind::UNIT_DISABLED,
            PoliceEvent::RoadblockPlaced { .. } => kind::ROADBLOCK_PLACED,
            PoliceEvent::SpikesPlaced { .. } => kind::SPIKES_PLACED,
            PoliceEvent::ToolRemoved { .. } => kind::TOOL_REMOVED,
            PoliceEvent::SpikeHit { .. } => kind::SPIKE_HIT,
            PoliceEvent::HeloLaunched { .. } => kind::HELO_LAUNCHED,
            PoliceEvent::PitAttempt { .. } => kind::PIT_ATTEMPT,
        }
    }

    /// Wrap as the contract event.
    pub fn to_sim_event(&self) -> SimEvent {
        SimEvent::PoliceEvent {
            kind: self.kind(),
            payload: serde_json::to_vec(self).expect("PoliceEvent always serializes"),
        }
    }

    /// Decode a contract event (None for other variants or a malformed payload).
    pub fn from_sim_event(ev: &SimEvent) -> Option<Self> {
        match ev {
            SimEvent::PoliceEvent { payload, .. } => serde_json::from_slice(payload).ok(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_through_sim_event() {
        let e = PoliceEvent::Radio {
            unit: Some(UnitId(4)),
            text: "Unit 4 en route".into(),
        };
        let s = e.to_sim_event();
        match &s {
            SimEvent::PoliceEvent { kind, .. } => assert_eq!(*kind, kind::RADIO),
            _ => unreachable!(),
        }
        assert_eq!(PoliceEvent::from_sim_event(&s), Some(e));
    }
}
