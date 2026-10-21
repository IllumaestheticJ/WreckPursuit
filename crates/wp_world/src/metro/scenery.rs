//! Port Vesper v3 — scenery and boundaries: parks and forest, street trees, lamps, poles, street furniture, lot fences and hedges, water-edge railings, cliff guard rails, highway sound walls, map-edge walls — everything that keeps cars out of places not meant to be driven.
//!
//! **Owned by the v3 scenery workstream.** Contract with the pipeline (`metro::generate`):
//! - [`earthworks`] runs before roads exist (ground levelling beyond the plan's site pads);
//! - [`generate`] runs after roads, in pipeline order, and adds features / buildings /
//!   POIs to the [`Ctx`]. Use `ctx.rng(SALT)` for randomness, stay inside your sites
//!   (`ctx.plan.sites_of(..)`) or check `ctx.free(..)` before placing, and `ctx.place` /
//!   `ctx.add_building` so later modules see what you placed.
//!
//! Stub: places nothing yet.

use super::ctx::Ctx;
use super::ground::{Earthwork, Natural};
use super::plan::Plan;

/// RNG salt of this module.
pub const SALT: u64 = 0x5C;

/// Ground levelling this module needs before roads are profiled.
pub fn earthworks(_plan: &Plan, _nat: &Natural) -> Vec<Earthwork> {
    Vec::new()
}

/// Place this module's content.
pub fn generate(_ctx: &mut Ctx) {}
