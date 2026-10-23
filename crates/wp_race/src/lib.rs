//! # wp_race
//!
//! The content layer of the open world: which races exist, when, with what rules, how results
//! are judged, and the crew/territory meta that gives night racing stakes beyond cash.
//!
//! | module | what |
//! |---|---|
//! | [`kind`] | race kinds (`Sprint`, `Circuit(laps)`, `Outrun`, `Checkpoint`, `Cannonball`, `Delivery`, `Drag`, `TimeAttack`), legality, class brackets, ids |
//! | [`course`] | gates, corridor, start grids, gate-crossing test |
//! | [`rules`] | per-kind rule objects (`data/events/rules.toml`) |
//! | [`judge`] | server-side judging: checkpoints, laps, DNF/DQ, clean race, outrun gap, drag timing |
//! | [`result`] | results, DNF/DQ reasons, total-order ranking |
//! | [`stakes`] | purse / entry / heat / police presence (`data/events/stakes.toml`) |
//! | [`schedule`] | the day's calendar from seeded rules, meet heat (`data/events/schedule.toml`) |
//! | [`crew`] | crews, shared rep, bank cut, crew challenges (`data/events/crews.toml`) |
//! | [`territory`] | district control points, heat-decay bonus, safehouses, weekly decay |
//! | [`progression`] | class brackets, licence tiers, entry requirements (`data/events/progression.toml`) |
//! | [`signals`] | events emitted to `wp_net`, `wp_economy`, `wp_police` |
//! | [`trace`] | deterministic scripted race traces for tests and balancing |
//! | [`entrants`] | AI entrants: `wp_opponent` racers driven in a judged race |
//! | [`police`] | race signals → `wp_police` input events (hot meets call in a patrol) |
//! | [`cargo`] | Delivery cargo applied to the car's mass, CG and inertia |
//! | [`catalog`] | the browser's race picker: courses on Port Vesper, Ashpeak County and the grid city, named AI drivers |
//! | [`spots`] | honk-to-start meet spots at each race's start, horn edge trigger |
//! | [`live`] | a picker race run next to a live `wp_sim` loop: judge, AI, meet heat, standings with gaps |
//! | [`bridge`] | `wp_world` city race planning + race-route activation, POIs/zones; `wp_economy` payout inputs |
//!
//! `wp_net` owns the lobby transport; this crate owns the rules. Everything here is
//! deterministic: no wall-clock time, no `HashMap` iteration, randomness only through a seeded
//! `wp_math::Rng`, and times are integer ticks/sub-ticks.

#![warn(missing_docs)]

pub mod bridge;
pub mod cargo;
pub mod catalog;
pub mod course;
pub mod crew;
pub mod data;
pub mod entrants;
pub mod error;
pub mod judge;
pub mod kind;
pub mod live;
pub mod police;
pub mod progression;
pub mod result;
pub mod rules;
pub mod schedule;
pub mod signals;
pub mod spots;
pub mod stakes;
pub mod territory;
pub mod trace;
mod trail;

pub use course::{Course, Gate, GridLayout, GridSlot};
pub use crew::{Challenge, ChallengeId, ChallengeOutcome, Crew, CrewCut, CrewRegistry};
pub use data::RaceData;
pub use error::{CrewError, EntryDenied, RaceDataError, RaceSetupError};
pub use judge::{CourseReset, RaceJudge, RacePhase, RacerEntry, RacerSample, StartError};
pub use kind::{
    ClassBracket, CrewId, DistrictId, EventId, Legality, RaceKind, RacerAccount, VenueId,
};
pub use progression::{EntryRequirements, ProgressionTable, RacerProfile};
pub use result::{DnfReason, DqReason, RacerResult, RacerStatus};
pub use rules::{RaceRules, RulesTable};
pub use schedule::{DayCalendar, MeetHeat, ScheduleRules, ScheduledEvent, Scheduler, Venue};
pub use signals::{PolicePresence, RaceSignal};
pub use stakes::{Stake, StakeTable};
pub use territory::Territory;

#[cfg(test)]
mod tests;
