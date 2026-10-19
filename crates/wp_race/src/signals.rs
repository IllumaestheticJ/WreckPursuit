//! Events `wp_race` emits for other systems (`wp_net` lobby/HUD, `wp_economy`, `wp_police`).
//!
//! [`RaceSignal`] carries the full judge results; [`RaceSignal::to_event`] reduces it to the
//! contract `RaceEvent` for `SimEvent::Race` (v0.3.0, request R1).

use serde::{Deserialize, Serialize};
use wp_contracts::EntityId;

use crate::kind::{CrewId, DistrictId, EventId, Legality, RaceKind, VenueId};
use crate::result::{DnfReason, DqReason, RacerResult};

/// Police presence requested for an event (contract type since v0.3.0).
pub use wp_contracts::PolicePresence;

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

impl RaceSignal {
    /// The contract event for `SimEvent::Race` (results reduced to `RaceResultRow`s).
    pub fn to_event(&self) -> wp_contracts::RaceEvent {
        use wp_contracts::RaceEvent as E;
        match self.clone() {
            RaceSignal::LobbyOpened {
                event,
                venue,
                pos,
                kind,
                legality,
                police,
            } => E::LobbyOpened {
                event,
                venue,
                pos,
                kind,
                legality,
                police,
            },
            RaceSignal::Countdown { event, green_tick } => E::Countdown { event, green_tick },
            RaceSignal::Green { event, tick } => E::Green { event, tick },
            RaceSignal::CheckpointPassed {
                event,
                racer,
                gate,
                lap,
                tick,
            } => E::CheckpointPassed {
                event,
                racer,
                gate,
                lap,
                tick,
            },
            RaceSignal::LapCompleted {
                event,
                racer,
                lap,
                lap_subticks,
            } => E::LapCompleted {
                event,
                racer,
                lap,
                lap_subticks,
            },
            RaceSignal::LeadChanged { event, leader } => E::LeadChanged { event, leader },
            RaceSignal::RacerFinished {
                event,
                racer,
                elapsed_subticks,
            } => E::RacerFinished {
                event,
                racer,
                elapsed_subticks,
            },
            RaceSignal::RacerDnf {
                event,
                racer,
                reason,
            } => E::RacerDnf {
                event,
                racer,
                reason,
            },
            RaceSignal::RacerDq {
                event,
                racer,
                reason,
            } => E::RacerDq {
                event,
                racer,
                reason,
            },
            RaceSignal::RaceFinished { event, results } => E::RaceFinished {
                event,
                results: results.iter().map(Into::into).collect(),
            },
            RaceSignal::PatrolAttracted { venue, pos, heat } => {
                E::PatrolAttracted { venue, pos, heat }
            }
            RaceSignal::TerritoryChanged { district, crew } => E::TerritoryChanged {
                zone: district,
                crew,
            },
        }
    }

    /// Wrap as `SimEvent::Race`.
    pub fn to_sim_event(&self) -> wp_contracts::SimEvent {
        wp_contracts::SimEvent::Race(self.to_event())
    }
}
