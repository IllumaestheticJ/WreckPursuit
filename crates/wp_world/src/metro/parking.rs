//! Port Vesper v3 — parking: multi-storey parking structures (drivable decks and ramps), surface lots with parked cars, the Starlite Drive-In.
//!
//! **Owned by the v3 parking workstream.** Contract with the pipeline (`metro::generate`):
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
pub const SALT: u64 = 0x9A;

/// Ground levelling this module needs before roads are profiled.
pub fn earthworks(_plan: &Plan, _nat: &Natural) -> Vec<Earthwork> {
    Vec::new()
}

/// Place this module's content.
pub fn generate(_ctx: &mut Ctx) {}
