//! Port Vesper v3 — buildings: lots along every street (district-specific towers, offices,
//! row houses, suburban houses, warehouses), landmark complexes (stadium, city hall, power
//! station, steel mill, observatory, malls, lighthouse) and building-ish district props
//! (water towers, churches, gas stations).
//!
//! **Owned by the v3 buildings workstream.** Contract with the pipeline (`metro::generate`):
//! - [`earthworks`] runs before roads exist (none needed: walls start from the lowest
//!   ground under each footprint);
//! - [`generate`] runs after roads, airport, docks, rail, parking and construction, before
//!   scenery.
//!
//! Order inside [`generate`]:
//! 1. [`landmarks`]: the plan's `Landmark` sites (anchors searched so the main masses clear
//!    whatever roads cross a site; other parts placed where clear).
//! 2. Special lots ([`lots::church`], [`lots::gas_station`], [`lots::water_tower`]) on the
//!    free street frontage nearest to a target in their district.
//! 3. [`lots::walk`]: every non-highway street side, both directions, lot after lot. The
//!    district ([`lots::Style`], from `plan.zones` by name) picks the template; a lot is
//!    placed only if its whole area is clear of road, verge, water, sites, buildings and
//!    features, not claimed by another lot and not too steep. Runs of contiguous lots get
//!    closers between buildings, side lot lines at the run ends and a rear lot line, so the
//!    land behind the street frontage cannot be driven into.
//!
//! Every building footprint is convex (a convex-hull collider): L-shapes are two buildings,
//! towers on podiums are a second, narrower building on the same base.

pub mod landmarks;
pub mod lots;
pub mod parts;

use super::ctx::Ctx;
use super::ground::{bounds, Earthwork, Natural};
use super::plan::Plan;
use crate::geom::V2;

/// Feature budget of this module: lot lines behind street-wall lots are left out beyond it
/// (everything else is always placed).
pub const FEATURE_BUDGET: usize = 18_000;

/// RNG salt of this module.
pub const SALT: u64 = 0xB1;

/// Landmark sites levelled by this module (those the plan gives no pad): the stadium bowl,
/// the steel mill, both malls and the observatory lot.
const LEVELLED: [&str; 5] = [
    "vesper_bowl",
    "steel_mill",
    "northfield_mall",
    "midtown_mall",
    "observatory",
];

/// Ground levelling this module needs before roads are profiled: a pad at the mean natural
/// height for each [`LEVELLED`] landmark site without a plan pad (walls of generic
/// buildings start from the lowest ground, so lots need none).
pub fn earthworks(plan: &Plan, nat: &Natural) -> Vec<Earthwork> {
    plan.sites
        .iter()
        .filter(|s| s.pad_z.is_none() && LEVELLED.contains(&s.id))
        .map(|s| {
            let (lo, hi) = bounds(&s.polygon);
            let mut sum = 0.0;
            let mut n = 0.0;
            for j in 0..=4 {
                for i in 0..=4 {
                    let p = [
                        lo[0] + (hi[0] - lo[0]) * i as f32 / 4.0,
                        lo[1] + (hi[1] - lo[1]) * j as f32 / 4.0,
                    ];
                    sum += nat.z(p);
                    n += 1.0;
                }
            }
            Earthwork::Pad {
                polygon: s.polygon.clone(),
                z: (sum / n).max(super::plan::MIN_LAND_Z + 0.5),
                blend_m: 40.0,
            }
        })
        .collect()
}

/// Centre of the named district's bounding box (`None` if the plan has no such zone).
fn zone_centre(plan: &Plan, name: &str) -> Option<V2> {
    plan.zones.iter().find(|z| z.name == name).map(|z| {
        let (lo, hi) = bounds(&z.zone.polygon);
        [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5]
    })
}

/// Special lots: (template, district, offset from the district centre, arterial only).
fn specials() -> Vec<(lots::Lot, &'static str, V2, bool)> {
    use lots::{church, gas_station, water_tower};
    vec![
        (water_tower(), "Southmoor", [200.0, 150.0], false),
        (water_tower(), "Larkspur", [300.0, 200.0], false),
        (water_tower(), "Northfield", [-300.0, 250.0], false),
        (church(), "Mercer", [0.0, 0.0], false),
        (church(), "Dunmore", [-200.0, 0.0], false),
        (church(), "Larkspur", [-300.0, -150.0], false),
        (gas_station(), "Larkspur", [0.0, 0.0], true),
        (gas_station(), "Southmoor", [0.0, 0.0], true),
        (gas_station(), "Northfield", [-500.0, 0.0], true),
        (gas_station(), "Northfield", [500.0, 0.0], true),
        (gas_station(), "South Shore", [0.0, 0.0], true),
    ]
}

/// Place this module's content.
pub fn generate(ctx: &mut Ctx) {
    let mut rng = ctx.rng(SALT);
    let n0 = ctx.features.len();
    landmarks::generate(ctx, &mut rng);
    let mut claims = lots::Claims::new();
    let mut deferred = Vec::new();
    for (lot, zone, off, art) in specials() {
        if let Some(c) = zone_centre(ctx.plan, zone) {
            let target = [c[0] + off[0], c[1] + off[1]];
            lots::place_special(ctx, &mut claims, &mut deferred, &lot, zone, target, art);
        }
    }
    let (row_group, spread) = lots::budget(ctx);
    let mut st = lots::State {
        row_group,
        spread,
        ..Default::default()
    };
    lots::walk(ctx, &mut rng, &mut claims, &mut st, &mut deferred);
    let used = ctx.features.len() - n0;
    lots::emit_deferred(ctx, &claims, &deferred, FEATURE_BUDGET.saturating_sub(used));
}

#[cfg(test)]
mod tests;
