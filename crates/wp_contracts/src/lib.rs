//! # wp_contracts
//!
//! Shared types, data schemas, events, traits and constants for every WreckPursuit crate.
//!
//! This crate is **owned by the integration lead**. Other workstreams may read it but never
//! edit it; propose changes in `docs/integration/<workstream>.md` under
//! "Contract change requests". Every change bumps [`CONTRACT_VERSION`] and is announced in
//! `docs/CONTRACT_CHANGELOG.md`.
//!
//! Conventions (see `CLAUDE.md`):
//! - SI units everywhere (m, kg, s, N, rad). Field names carry a unit suffix where useful.
//! - Vehicle/chassis space: +X right, +Y forward, +Z up, right-handed. World: +Z up, gravity -Z.
//! - All data types derive `serde` + `schemars`; JSON Schemas are generated with
//!   `cargo run -p wp_contracts --bin gen-schemas`.
//! - Maps use `BTreeMap` so iteration order is deterministic.

#![warn(missing_docs)]

pub mod damage;
pub mod economy;
pub mod events;
pub mod ids;
pub mod lattice;
pub mod load;
pub mod material;
pub mod mode;
pub mod net;
pub mod police;
pub mod race;
pub mod repair;
pub mod runtime;
pub mod schema;
pub mod time;
pub mod tire;
pub mod traits;
pub mod vehicle;
pub mod weather;
pub mod world;

pub use damage::*;
pub use economy::*;
pub use events::*;
pub use ids::*;
pub use lattice::*;
pub use load::*;
pub use material::*;
pub use mode::*;
pub use net::*;
pub use police::*;
pub use race::*;
pub use repair::*;
pub use runtime::*;
pub use time::*;
pub use tire::*;
pub use traits::*;
pub use vehicle::*;
pub use weather::*;
pub use world::*;

/// Semantic version of the contract. Bumped on every change; see `docs/CONTRACT_CHANGELOG.md`.
pub const CONTRACT_VERSION: &str = "0.4.1";

/// Fixed physics timestep in seconds (240 Hz). The only `dt` simulation code may integrate with.
pub const PHYSICS_DT: f32 = 1.0 / 240.0;

/// Network snapshot rate in Hz. One net tick = `240 / 60 = 4` physics ticks.
pub const NET_TICK_HZ: u32 = 60;

/// Number of vehicle sub-steps inside one [`PHYSICS_DT`] step.
pub const VEHICLE_SUBSTEPS: u32 = 2;

/// Maximum number of simulated vehicles (players + police + physical traffic) per instance.
pub const MAX_VEHICLES: usize = 64;

/// Physics ticks per network tick (derived: 240 / 60).
pub const PHYSICS_TICKS_PER_NET_TICK: u32 = 4;

/// Standard gravity magnitude in m/s². Applied along world -Z.
pub const GRAVITY_MPS2: f32 = 9.81;
