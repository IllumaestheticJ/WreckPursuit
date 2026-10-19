//! # wp_damage
//!
//! Semi-soft-body damage (owned by workstream 03): a rigid chassis carries a deformable
//! lattice of mass nodes and plastic beams that crumples permanently from real contact
//! impulses, tears parts off, fails mechanical components by intrusion and reports the
//! result as [`HandlingEffects`] so a damaged car drives damaged.
//!
//! - [`DamageModel`] implements [`wp_contracts::DamageSim`]. One code path for both modes;
//!   Arcade and Simulation differ only by the [`ModeTable`] passed in.
//! - [`lattice_gen`] + the `gen-lattice` binary build lattices from dimensions and a
//!   [`lattice_gen::BodyProfile`].
//! - [`barrier`] runs crash-barrier scenarios (`crates/wp_damage/scenarios/*.ron`).
//! - [`repair`] prices damage into a [`RepairEstimate`].
//!
//! See `README.md` for the model write-up and tuning knobs.

#![warn(missing_docs)]
// Index loops over parallel node/beam arrays read better than zipped iterators here.
#![allow(clippy::needless_range_loop)]

pub mod barrier;
mod contact;
pub mod debris;
pub mod hull;
pub mod lattice_gen;
mod model;
pub mod persist;
pub mod repair;
mod solver;
#[cfg(test)]
mod tests;
pub mod tuning;
pub mod vec3;

pub use debris::{DetachedKind, DetachedPart};
pub use model::{
    component_health_weight, region_health_weight, DamageLogEntry, DamageLogKind, DamageModel,
    GlassBreak, ALL_REGIONS, REGION_COUNT,
};
pub use persist::{DamageSnapshot, DamageSummary};
pub use repair::default_price_table;
pub use tuning::DamageTuning;
pub use wp_contracts::{HandlingEffects, ModeTable, PriceTable, RepairEstimate};
