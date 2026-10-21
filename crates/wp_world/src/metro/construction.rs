//! Port Vesper v3 — construction sites: Lantern Tower, the Unfinished Exchange (ramps to nowhere), Canal Flats earthworks, the Eastgate block.
//!
//! **Owned by the v3 construction workstream.** Contract with the pipeline (`metro::generate`):
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
pub const SALT: u64 = 0xC0;

/// Ground levelling this module needs before roads are profiled.
pub fn earthworks(_plan: &Plan, _nat: &Natural) -> Vec<Earthwork> {
    Vec::new()
}

/// Place this module's content.
pub fn generate(_ctx: &mut Ctx) {}
