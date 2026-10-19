//! Events `wp_race` emits for other systems (`wp_net` lobby/HUD, `wp_economy`, `wp_police`).
//!
//! These stay crate-local until contract v0.2.0 adds a typed `SimEvent::Race(RaceEvent)`
//! (request R1 in `docs/integration/race.md`); the variants below are the proposed payload.

use serde::{Deserialize, Serialize};
use wp_contracts::EntityId;

use crate::kind::{CrewId, DistrictId, EventId, Legality, RaceKind, VenueId};
use crate::result::{DnfReason, DqReason, RacerResult};

/// Police presence requested for an event (`wp_police` maps it onto its dispatch tables).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

/// Everything `wp_race` tells the rest of the game.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RaceSignal {
    /// The lobby for a scheduled event opened (players near the venue may join).
    LobbyOpened {
        /// Event.
        event: EventId,
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
        event: EventId,
        /// Physics tick of the green light.
        green_tick: u64,
    },
    /// Green light.
    Green {
        /// Event.
        event: EventId,
        /// Tick.
        tick: u64,
    },
    /// A racer passed a checkpoint gate.
    CheckpointPassed {
        /// Event.
        event: EventId,
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
        event: EventId,
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
        event: EventId,
        /// New leader.
        leader: EntityId,
    },
    /// A racer finished.
    RacerFinished {
        /// Event.
        event: EventId,
        /// Racer.
        racer: EntityId,
        /// Elapsed, sub-ticks.
        elapsed_subticks: u64,
    },
    /// A racer did not finish.
    RacerDnf {
        /// Event.
        event: EventId,
        /// Racer.
        racer: EntityId,
        /// Why.
        reason: DnfReason,
    },
    /// A racer was disqualified.
    RacerDq {
        /// Event.
        event: EventId,
        /// Racer.
        racer: EntityId,
        /// Why.
        reason: DqReason,
    },
    /// Race over; final ranked results. `wp_economy` settles purses from this,
    /// `wp_police` adds heat for illegal races, territory adds control points.
    RaceFinished {
        /// Event.
        event: EventId,
        /// Ranked results.
        results: Vec<RacerResult>,
    },
    /// Too many meets at one venue: a patrol should be sent (`wp_police`).
    PatrolAttracted {
        /// Venue.
        venue: VenueId,
        /// Venue position, m.
        pos: [f32; 3],
        /// Rolling meet heat at the venue.
        heat: f32,
    },
    /// Control of a district changed hands (`None` = nobody controls it any more).
    TerritoryChanged {
        /// District.
        district: DistrictId,
        /// New controller.
        crew: Option<CrewId>,
    },
}
