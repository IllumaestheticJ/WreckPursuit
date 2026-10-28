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
//! - [`learned`]: corrections learned in offline training runs (per car and stretch of road:
//!   corner-speed scale and lateral aim; traffic and police behaviour numbers), applied by
//!   [`Field`] when [`RaceConfig::learned`] is on.
//! - [`draft`]: slipstream model (drag multiplier per vehicle from wake geometry) for the sim
//!   loop to apply.
//! - [`evader`]: scripted escapee for the police benchmark (`simcli police-bench`): flees
//!   police it can see on the road graph, three skills.
//! - [`roam`]: free-roam street racers that chain legs around their haunt (highway, touge,
//!   dirt, downtown, rural) on open roads, each leg driven by a one-car [`Field`].
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
pub mod capability_table;
pub mod corridor;
pub mod draft;
pub mod evader;
pub mod field;
pub mod learned;
pub mod line;
pub mod profile;
pub mod pursuit;
pub mod roam;

pub use capability::{CarCapability, LooseGrip};
pub use corridor::{
    Corridor, CorridorOptions, CorridorPos, CorridorSample, OpposingLanes, CORRIDOR_STEP_M,
};
pub use draft::{drag_scales, DraftBody, DraftParams};
pub use evader::{
    Evader, EvaderDebug, EvaderMode, EvaderParams, EvaderSkill, EvaderStats, PoliceCar,
};
pub use field::{
    Debug, Field, GridLayout, OpponentEvent, RaceConfig, RacerSpec, RacerStatus, SpeedLimit,
    Standing, Tactic, VehicleObs, AI_CONTACT_IMPULSE_NS, RED_LIGHT_RISK, YIELD_AFTER_S,
    YIELD_RADIUS_M,
};
pub use learned::{LearnedPath, LearnedTable, LearnedTraffic};
pub use line::{drive_accel, LineParams, RacingLine, SpeedProfile, Usage};
pub use profile::{DriverParams, DriverProfile, RiskLevel};
pub use pursuit::{EventHeat, FixedHeat, NoPolice, PoliceQuery, SimPolice};
pub use roam::{
    haunt_by_name, haunt_cars, roster_seed, spawn_slots, stock_profile, stock_roster, Haunt,
    HauntMap, HauntParams, RoamConfig, RoamEvent, RoamPhase, RoamerSpec, Roamers, RosterEntry,
    SpawnSlot,
};
