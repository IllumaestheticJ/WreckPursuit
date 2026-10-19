//! # wp_opponent
//!
//! AI street racers (workstream 17) that race *cleanly and believably* in a game where
//! contact is expensive.
//!
//! - [`corridor`]: a race route (`wp_world::routes`) → sampled centreline with drivable bounds.
//! - [`line`]: minimum-curvature racing line inside the corridor + speed profile from the car's
//!   measured limits ([`capability`], via `wp_vehicle::metrics`) and the driver's usage.
//! - [`profile`]: [`DriverProfile`] (skill, aggression, consistency, risk, awareness) →
//!   [`DriverParams`].
//! - [`field`]: the event — perception of racers, other vehicles and civilians, tactics
//!   (follow, overtake with margin, one defensive move, back out of side-by-side, rejoin,
//!   reverse, park), braking-point noise, damage awareness, civilian-contact DQ, police flight,
//!   laps and standings.
//! - [`pursuit`]: police awareness ([`PoliceQuery`]; [`EventHeat`] tracks wanted levels and
//!   police sightings from `wp_police` events).
//!
//! Steering and pedals come from the shared controller `wp_world::drive::DriveController`
//! (machine D's pure pursuit + PID, also used by physical traffic). **No rubber-banding**:
//! nothing reads race position; pace comes from car capability × driver usage.
//!
//! Deterministic: f32 only, seeded `wp_math::Rng` per driver, fixed iteration order, no
//! transcendental libm calls (polynomial `atan` from `wp_world::geom`).

#![warn(missing_docs)]

pub mod capability;
pub mod corridor;
pub mod field;
pub mod line;
pub mod profile;
pub mod pursuit;

pub use capability::CarCapability;
pub use corridor::{
    Corridor, CorridorOptions, CorridorPos, CorridorSample, OpposingLanes, CORRIDOR_STEP_M,
};
pub use field::{
    Debug, Field, GridLayout, OpponentEvent, RaceConfig, RacerSpec, RacerStatus, SpeedLimit,
    Standing, Tactic, VehicleObs, AI_CONTACT_IMPULSE_NS, YIELD_AFTER_S, YIELD_RADIUS_M,
};
pub use line::{LineParams, RacingLine, SpeedProfile, Usage};
pub use profile::{DriverParams, DriverProfile};
pub use pursuit::{EventHeat, FixedHeat, NoPolice, PoliceQuery, SimPolice};
