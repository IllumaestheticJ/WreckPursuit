//! Typed race events (v0.3.0; final list from workstream 15 / L, request R1).
//!
//! `wp_race` emits [`SimEvent::Race`](crate::SimEvent::Race); the full judge result
//! (`wp_race::RacerResult`, with drag and violation detail) stays server-side and is reduced to
//! [`RaceResultRow`] here.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{EntityId, PolicePresence, ZoneId};

/// Identifier of a scheduled or ad-hoc race event (scheduler ids are `day * 1000 + n`).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
)]
pub struct RaceEventId(pub u32);

/// A race venue: index of a `wp_world` POI of kind `RaceStartGrid` (or a scheduler-local
/// venue index when no city is loaded).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct VenueId(pub u32);

/// Identifier of a crew (allocated by `wp_race`, never reused).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct CrewId(pub u32);

/// What kind of race this is, with the parameters that change its rules.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum RaceKind {
    /// Point to point along a fixed corridor; first across the finish wins.
    Sprint,
    /// Closed loop; first to complete `laps` wins.
    Circuit {
        /// Laps to complete (≥ 1).
        laps: u8,
    },
    /// 1v1 gap race in open traffic: a lead of `lead_m` wins outright, else the leader at the
    /// time limit.
    Outrun {
        /// Winning lead (straight-line gap), m.
        lead_m: f32,
    },
    /// Open route: pass every checkpoint in order by any path, within the time limit.
    Checkpoint,
    /// Cross-city point to point, any path, through heavy traffic.
    Cannonball,
    /// Point to point with cargo; cargo damage costs money, destroyed cargo is a DNF.
    Delivery {
        /// Cargo mass added to the car, kg.
        cargo_kg: f32,
        /// Cargo value, money units.
        cargo_value: i64,
    },
    /// Straight-line pass from a standing start.
    Drag,
    /// Each racer on their own clock from the start gate; best elapsed time wins.
    TimeAttack,
}

impl RaceKind {
    /// Stable lower-case key used in data files.
    pub fn key(&self) -> &'static str {
        match self {
            RaceKind::Sprint => "sprint",
            RaceKind::Circuit { .. } => "circuit",
            RaceKind::Outrun { .. } => "outrun",
            RaceKind::Checkpoint => "checkpoint",
            RaceKind::Cannonball => "cannonball",
            RaceKind::Delivery { .. } => "delivery",
            RaceKind::Drag => "drag",
            RaceKind::TimeAttack => "time_attack",
        }
    }

    /// Every kind key, in declaration order.
    pub const KEYS: [&'static str; 8] = [
        "sprint",
        "circuit",
        "outrun",
        "checkpoint",
        "cannonball",
        "delivery",
        "drag",
        "time_attack",
    ];

    /// A representative value of the kind for a key (default parameters).
    pub fn from_key(key: &str) -> Option<RaceKind> {
        Some(match key {
            "sprint" => RaceKind::Sprint,
            "circuit" => RaceKind::Circuit { laps: 3 },
            "outrun" => RaceKind::Outrun { lead_m: 250.0 },
            "checkpoint" => RaceKind::Checkpoint,
            "cannonball" => RaceKind::Cannonball,
            "delivery" => RaceKind::Delivery {
                cargo_kg: 150.0,
                cargo_value: 4000,
            },
            "drag" => RaceKind::Drag,
            "time_attack" => RaceKind::TimeAttack,
            _ => return None,
        })
    }

    /// Racers must pass checkpoints in order and stay inside the route corridor.
    pub fn uses_corridor(&self) -> bool {
        matches!(
            self,
            RaceKind::Sprint
                | RaceKind::Circuit { .. }
                | RaceKind::Drag
                | RaceKind::TimeAttack
                | RaceKind::Delivery { .. }
        )
    }

    /// Exactly two racers.
    pub fn is_head_to_head(&self) -> bool {
        matches!(self, RaceKind::Outrun { .. } | RaceKind::Drag)
    }
}

/// Legal status of an event; drives police presence and the stake risk premium.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
)]
pub enum Legality {
    /// Sanctioned daytime event on a closed route: no police interest.
    Sanctioned,
    /// Ordinary illegal street race.
    #[default]
    Street,
    /// Rare illegal race in broad daylight: police presence forced to max.
    IllegalDaytime,
}

impl Legality {
    /// Stable key used in data files.
    pub fn key(&self) -> &'static str {
        match self {
            Legality::Sanctioned => "sanctioned",
            Legality::Street => "street",
            Legality::IllegalDaytime => "illegal_daytime",
        }
    }
}

/// Why a racer did not finish (they still rank above every DQ).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum DnfReason {
    /// The car became a total loss.
    TotalLoss,
    /// Arrested by police.
    Captured,
    /// The race time limit ran out.
    TimeLimit,
    /// Did not finish within the window after the winner.
    FinishWindow,
    /// Quit / left the race voluntarily.
    Retired,
    /// Delivery cargo destroyed.
    CargoDestroyed,
    /// Lost an outrun.
    Outrun,
    /// Disconnected from the server.
    Disconnected,
    /// Abandoned the race to run from the police (AI entrants).
    Fled,
}

/// Why a racer was disqualified (ranked below every finisher and every DNF).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum DqReason {
    /// Outside the route corridor longer than the grace time (shortcut).
    LeftCorridor,
    /// Moved before green (drag).
    JumpStart,
    /// Moved faster than physically plausible between two samples.
    ImplausibleMovement,
    /// Disqualified by the server / a marshal / an admin.
    Marshal,
}

/// A racer's state in the race.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum RacerStatus {
    /// Entered; race not over for them yet.
    Running,
    /// Crossed the finish (or won the outrun).
    Finished,
    /// Did not finish.
    Dnf(DnfReason),
    /// Disqualified.
    Dq(DqReason),
}

impl RacerStatus {
    /// Ranking class: finishers 0, still running / DNF 1, DQ 2.
    pub fn class(&self) -> u8 {
        match self {
            RacerStatus::Finished => 0,
            RacerStatus::Running | RacerStatus::Dnf(_) => 1,
            RacerStatus::Dq(_) => 2,
        }
    }
}

/// One racer's final result as replicated (the judge's `RacerResult` minus drag / violation
/// detail).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RaceResultRow {
    /// Racer vehicle.
    pub racer: EntityId,
    /// Final position, 1-based, unique.
    pub position: u8,
    /// Status.
    pub status: RacerStatus,
    /// Elapsed time, sub-ticks (1/1000 physics tick; finishers only).
    pub elapsed_subticks: Option<u64>,
    /// Best completed lap, sub-ticks (circuits).
    pub best_lap_subticks: Option<u64>,
    /// Clean race (no contacts, civilian hits or collision violations; finished).
    pub clean: bool,
    /// Contacts above the clean threshold.
    pub contacts: u32,
    /// Contacts with civilians / traffic.
    pub civilian_hits: u32,
    /// Money owed for cargo damage (delivery only).
    pub cargo_damage_cost: i64,
}

/// Everything `wp_race` tells the rest of the game (`SimEvent::Race`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum RaceEvent {
    /// The lobby for a scheduled event opened.
    LobbyOpened {
        /// Event.
        event: RaceEventId,
        /// Venue.
        venue: VenueId,
        /// Venue position, m.
        pos: [f32; 3],
        /// Race kind.
        kind: RaceKind,
        /// Legality.
        legality: Legality,
        /// Police presence for `wp_police`.
        police: PolicePresence,
    },
    /// Countdown started; green at `green_tick`.
    Countdown {
        /// Event.
        event: RaceEventId,
        /// Physics tick of the green light.
        green_tick: u64,
    },
    /// Green light.
    Green {
        /// Event.
        event: RaceEventId,
        /// Tick.
        tick: u64,
    },
    /// A racer passed a checkpoint gate.
    CheckpointPassed {
        /// Event.
        event: RaceEventId,
        /// Racer.
        racer: EntityId,
        /// Gate index.
        gate: u32,
        /// Lap (1-based; 1 for point-to-point).
        lap: u32,
        /// Tick of the sample after the crossing.
        tick: u64,
    },
    /// A racer completed a lap.
    LapCompleted {
        /// Event.
        event: RaceEventId,
        /// Racer.
        racer: EntityId,
        /// Laps completed so far.
        lap: u32,
        /// Lap time, sub-ticks.
        lap_subticks: u64,
    },
    /// Outrun lead changed.
    LeadChanged {
        /// Event.
        event: RaceEventId,
        /// New leader.
        leader: EntityId,
    },
    /// A racer finished.
    RacerFinished {
        /// Event.
        event: RaceEventId,
        /// Racer.
        racer: EntityId,
        /// Elapsed, sub-ticks.
        elapsed_subticks: u64,
    },
    /// A racer did not finish.
    RacerDnf {
        /// Event.
        event: RaceEventId,
        /// Racer.
        racer: EntityId,
        /// Why.
        reason: DnfReason,
    },
    /// A racer was disqualified.
    RacerDq {
        /// Event.
        event: RaceEventId,
        /// Racer.
        racer: EntityId,
        /// Why.
        reason: DqReason,
    },
    /// Race over; final ranked results.
    RaceFinished {
        /// Event.
        event: RaceEventId,
        /// Ranked results.
        results: Vec<RaceResultRow>,
    },
    /// Too many meets at one venue: a patrol should be sent.
    PatrolAttracted {
        /// Venue.
        venue: VenueId,
        /// Venue position, m.
        pos: [f32; 3],
        /// Rolling meet heat at the venue.
        heat: f32,
    },
    /// Control of a city zone changed hands (`None` = nobody controls it any more).
    TerritoryChanged {
        /// Zone (territory is per zone, not per district kind).
        zone: ZoneId,
        /// New controller.
        crew: Option<CrewId>,
    },
}

/// Race commands from a client (v0.3.0, race R6; `ClientToServer::RaceCommand`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum RaceCommand {
    /// Join a lobby.
    Enter {
        /// Event.
        event: RaceEventId,
    },
    /// Leave a lobby / retire.
    Withdraw {
        /// Event.
        event: RaceEventId,
    },
    /// Ready on the grid.
    Ready,
}
