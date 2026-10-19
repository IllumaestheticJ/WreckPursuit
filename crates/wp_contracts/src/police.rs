//! Typed police events and captain commands (v0.3.0; designed by workstream 06 / E).
//!
//! Replaces the opaque `SimEvent::PoliceEvent { kind, payload }` and
//! `ClientToServer::CaptainCommand(Vec<u8>)` of v0.1–0.2. `wp_police` emits
//! [`SimEvent::Police`](crate::SimEvent::Police) and validates [`CaptainCommand`]s; everyone
//! else matches on these types directly.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{EntityId, RoadEdgeId, WantedLevel};

/// Police unit id (allocated by `wp_police`, never reused within an instance). Distinct from
/// `EntityId`: a unit may exist before `wp_sim` gives it a vehicle body.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct UnitId(pub u32);

/// Roadblock / spike strip id (allocated by `wp_police`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct ToolId(pub u32);

/// Police incident id (allocated by `wp_police`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct IncidentId(pub u32);

/// How a pursuit ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

impl PursuitOutcome {
    /// Captured or captured-by-total-loss.
    pub fn is_capture(self) -> bool {
        matches!(
            self,
            PursuitOutcome::Captured | PursuitOutcome::CapturedTotalLoss
        )
    }
}

/// Why a unit left service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

/// Coarse police unit kind for events and views.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum UnitKind {
    /// Ground patrol car.
    Car,
    /// Helicopter (kinematic, never a weapon).
    Helicopter,
}

/// Police presence requested for a race event (v0.3.0, race R2); `wp_police` maps it onto
/// its dispatch tables.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum PolicePresence {
    /// Police ignore the event (sanctioned, marshals on site).
    None,
    /// Normal patrol behaviour for the area and time.
    Normal,
    /// Elevated: extra patrols near the venue.
    Elevated,
    /// Maximum: illegal daytime races (`police_presence_override: max`).
    Max,
}

/// Everything `wp_police` reports to the rest of the game (`SimEvent::Police`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
    /// A pursuit ended. Emitted **before** the `WantedLevelChanged` that resets the level.
    PursuitEnded {
        /// Racer.
        racer: EntityId,
        /// Outcome.
        outcome: PursuitOutcome,
        /// Wanted level when the pursuit ended (before any reset).
        level: WantedLevel,
        /// Unit credited with a capture (closest unit on the racer); `None` for non-captures.
        capturer: Option<UnitId>,
        /// Other units on the racer at a capture, ascending ids (empty for non-captures).
        assists: Vec<UnitId>,
    },
    /// A unit appeared. `wp_sim` spawns a vehicle body for it when it drives units physically.
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
    /// A roadblock was placed.
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
    /// A vehicle ran over a spike strip; flagged wheels deflate.
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

/// Stable `u16` codes of [`PoliceEvent`] variants (scenario expectations, logs, wire tags).
pub mod police_kind {
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
    /// Stable kind code (see [`police_kind`]).
    pub fn kind(&self) -> u16 {
        use police_kind as k;
        match self {
            PoliceEvent::Radio { .. } => k::RADIO,
            PoliceEvent::WantedLevelChanged { .. } => k::WANTED_LEVEL,
            PoliceEvent::PursuitStarted { .. } => k::PURSUIT_STARTED,
            PoliceEvent::PursuitEnded { .. } => k::PURSUIT_ENDED,
            PoliceEvent::UnitSpawned { .. } => k::UNIT_SPAWNED,
            PoliceEvent::UnitDespawned { .. } => k::UNIT_DESPAWNED,
            PoliceEvent::UnitDisabled { .. } => k::UNIT_DISABLED,
            PoliceEvent::RoadblockPlaced { .. } => k::ROADBLOCK_PLACED,
            PoliceEvent::SpikesPlaced { .. } => k::SPIKES_PLACED,
            PoliceEvent::ToolRemoved { .. } => k::TOOL_REMOVED,
            PoliceEvent::SpikeHit { .. } => k::SPIKE_HIT,
            PoliceEvent::HeloLaunched { .. } => k::HELO_LAUNCHED,
            PoliceEvent::PitAttempt { .. } => k::PIT_ATTEMPT,
        }
    }
}

/// An order for one police unit (forced on AI units for a hold time, a suggestion for player
/// units).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum UnitOrder {
    /// Resume patrol.
    Patrol,
    /// Pursue a racer.
    Pursue {
        /// Racer.
        target: EntityId,
    },
    /// Intercept a racer via the road graph.
    Intercept {
        /// Racer.
        target: EntityId,
    },
    /// Respond to an incident.
    Respond {
        /// Incident.
        incident: IncidentId,
    },
    /// Pull over and stay out of the chase.
    Disengage,
}

/// Police captain command (`ClientToServer::CaptainCommand`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum CaptainCommand {
    /// Give a unit an order.
    AssignUnit {
        /// Unit.
        unit: UnitId,
        /// Order.
        order: UnitOrder,
    },
    /// Roadblock ahead of a racer.
    RequestRoadblock {
        /// Racer.
        target: EntityId,
        /// Road, or `None` for automatic placement. (Decodes the pre-v0.3.0 name `segment`.)
        #[serde(alias = "segment")]
        road: Option<RoadEdgeId>,
    },
    /// Spike strip ahead of a racer.
    RequestSpikes {
        /// Racer.
        target: EntityId,
        /// Road, or `None` for automatic placement. (Decodes the pre-v0.3.0 name `segment`.)
        #[serde(alias = "segment")]
        road: Option<RoadEdgeId>,
    },
    /// Helicopter on a racer.
    RequestHelicopter {
        /// Racer.
        target: EntityId,
    },
    /// Racer whose reinforcements are dispatched first (`None` clears).
    SetPriorityTarget {
        /// Racer.
        target: Option<EntityId>,
    },
    /// Call off the pursuit of a racer.
    CallOff {
        /// Racer.
        target: EntityId,
    },
}

/// Why a roadblock / spike site was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SiteError {
    /// No point on the predicted path satisfies the rules.
    NoSite,
    /// The requested road is not on the racer's predicted path in range.
    NotOnPath,
    /// Sight distance too short (blind corner).
    BlindCorner,
    /// Highway not allowed at this wanted level.
    Highway,
}

/// Why a [`CaptainCommand`] was refused. The reply to a command is `Result<(), CaptainError>`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum CaptainError {
    /// No such unit.
    UnknownUnit,
    /// No such racer.
    UnknownTarget,
    /// No such incident.
    UnknownIncident,
    /// The racer is not being pursued.
    NotPursued,
    /// The tool unlocks at a higher wanted level.
    Locked {
        /// Required wanted level (1..=5).
        need_level: u8,
    },
    /// Tool still cooling down.
    Cooldown {
        /// Seconds left.
        remaining_s: f32,
    },
    /// Too many active tools / helicopters.
    Budget,
    /// Placement rejected.
    Site(SiteError),
    /// The helicopter is unavailable (night availability roll).
    Unavailable,
    /// The unit cannot take orders (wrecked, parked at a tool, helicopter).
    UnitUnavailable,
    /// Command did not decode.
    Malformed,
}
