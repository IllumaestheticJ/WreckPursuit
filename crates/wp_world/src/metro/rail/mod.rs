//! Port Vesper v3 — rail: freight and passenger lines (track, ballast beds, rail bridges), Westline yard and roundhouse, Vesper Central station, level crossings, parked trains.
//!
//! **Owned by the v3 rail workstream.** Contract with the pipeline (`metro::generate`):
//! - [`earthworks`] runs before roads exist (ground levelling beyond the plan's site pads);
//! - [`generate`] runs after roads, in pipeline order, and adds features / buildings /
//!   POIs to the [`Ctx`]. Use `ctx.rng(SALT)` for randomness, stay inside your sites
//!   (`ctx.plan.sites_of(..)`) or check `ctx.free(..)` before placing, and `ctx.place` /
//!   `ctx.add_building` so later modules see what you placed.
//!
//! Module status: built (see `align` for the alignments, `track` for the lines, `yard` for
//! the Westline yard and roundhouse, `station` for Vesper Central).

use super::ctx::Ctx;
use super::ground::{Earthwork, Natural};
use super::plan::Plan;

pub mod align;
mod cars;
mod geo;
mod station;
mod track;
mod yard;

pub use align::{rail_lines, BridgeSpan, RailAlignment, SpanKind, Support};

/// RNG salt of this module.
pub const SALT: u64 = 0x7A;

/// Ground levelling this module needs before roads are profiled: one
/// [`Earthwork::Corridor`] per run of at-grade / embankment alignment samples (never over
/// water or under bridges and viaducts), graded to the rail profile, and an
/// [`Earthwork::Pad`] under Vesper Central (elevated terminus) so its deck clears the
/// streets by the same margin as the Central Rail Bridge.
pub fn earthworks(plan: &Plan, nat: &Natural) -> Vec<Earthwork> {
    align::corridor_earthworks(plan, &rail_lines(plan, nat))
}

/// Place this module's content.
pub fn generate(ctx: &mut Ctx) {
    // The alignment depends on the natural ground, which the pipeline built from a fresh
    // `Rng::new(seed)` before anything else drew from it; rebuild it the same way.
    let nat = Natural::new(ctx.plan, &mut wp_math::Rng::new(ctx.seed));
    let lines = rail_lines(ctx.plan, &nat);
    let mut rng = ctx.rng(SALT);
    let mut out = track::Output::default();
    let paving = geo::Paving::new(ctx);
    let crossings = track::find_crossings(&lines, &paving);
    track::build_lines(ctx, &lines, &crossings, &paving, &mut rng, &mut out);
    yard::build_yard(ctx, &lines, &paving, &mut rng, &mut out);
    yard::build_roundhouse(ctx, &lines, &paving, &mut rng, &mut out);
    station::build_station(ctx, &lines, &paving, &mut rng, &mut out);
    yard::site_fences(ctx, &lines, &paving, &mut out);
    out.commit(ctx);
}
