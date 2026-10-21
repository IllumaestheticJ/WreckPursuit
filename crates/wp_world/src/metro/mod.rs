//! Port Vesper v3 (`vesper_v3`): the full city after `docs/design/CITY.md`, extended to an
//! 8 × 7 km map with an airport, an airfield, docks, rail, parking structures, construction
//! sites, landmarks and scenery. Built on the side: Port Vesper v2 stays the default city
//! until v3 is ready.
//!
//! Pipeline ([`generate_with_report`]), one module per workstream:
//!
//! | Step | Module | Owner |
//! |---|---|---|
//! | layout | [`plan`] | v3 lead |
//! | natural ground, earthworks | [`ground`] + every module's `earthworks` | v3 lead / modules |
//! | road network, carving | [`roads`] | roads |
//! | shared placement context | [`ctx`] | v3 lead |
//! | airport, airfield | [`airport`] | airport |
//! | port, waterfront | [`docks`] | docks |
//! | rail | [`rail`] | rail |
//! | parking structures, lots | [`parking`] | parking |
//! | construction sites | [`construction`] | construction |
//! | buildings, landmarks | [`buildings`] | buildings |
//! | scenery, boundaries | [`scenery`] | scenery |
//!
//! Modules after `roads` run in that order on one [`ctx::Ctx`], so each sees what the
//! earlier ones placed. Each draws randomness from its own `ctx.rng(SALT)`, so a change in
//! one module never reshuffles another.

pub mod airport;
pub mod buildings;
pub mod construction;
pub mod ctx;
pub mod docks;
pub mod ground;
pub mod parking;
pub mod plan;
pub mod rail;
pub mod roads;
pub mod scenery;

use wp_math::Rng;

use crate::builder::Built;
use crate::geom::{self, V2, V3};
use crate::network::{Feature, FeatureKind, LaneId, LaneKind, Poi, RoadNetwork};

/// City id of this preset (file stem for its data files).
pub const CITY_ID: &str = "vesper_v3";

/// Generator parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct MetroConfig {
    /// Seed for terrain noise, street warping, lots, scenery.
    pub seed: u64,
    /// Heightfield vertex spacing, m.
    pub terrain_cell_m: f32,
}

impl Default for MetroConfig {
    fn default() -> Self {
        Self {
            seed: 1,
            terrain_cell_m: 6.0,
        }
    }
}

/// Generator diagnostics.
#[derive(Debug, Clone, Default)]
pub struct MetroReport {
    /// One line per noteworthy fact (roads, sites, counts).
    pub notes: Vec<String>,
    /// Feature count per kind.
    pub feature_counts: std::collections::BTreeMap<FeatureKind, usize>,
}

/// Generate Port Vesper v3.
pub fn generate(cfg: &MetroConfig) -> RoadNetwork {
    generate_with_report(cfg).0
}

/// Generate Port Vesper v3 plus diagnostics.
pub fn generate_with_report(cfg: &MetroConfig) -> (RoadNetwork, MetroReport) {
    let plan = plan::Plan::new();
    let mut rng = Rng::new(cfg.seed);
    let nat = ground::Natural::new(&plan, &mut rng);

    // Earthworks: module requests, then the plan's site pads.
    let mut works = Vec::new();
    works.extend(airport::earthworks(&plan, &nat));
    works.extend(docks::earthworks(&plan, &nat));
    works.extend(rail::earthworks(&plan, &nat));
    works.extend(parking::earthworks(&plan, &nat));
    works.extend(construction::earthworks(&plan, &nat));
    works.extend(buildings::earthworks(&plan, &nat));
    works.extend(scenery::earthworks(&plan, &nat));
    for s in &plan.sites {
        if let Some(z) = s.pad_z {
            works.push(ground::Earthwork::Pad {
                polygon: s.polygon.clone(),
                z,
                blend_m: 60.0,
            });
        }
    }
    let mut gnd = ground::Ground::new(&nat, cfg.terrain_cell_m, &works);

    let roads = roads::build(&plan, &mut gnd, cfg);
    let terrain = gnd.hf;

    let mut ctx = ctx::Ctx::new(&plan, &terrain, &roads, cfg.seed);
    airport::generate(&mut ctx);
    docks::generate(&mut ctx);
    rail::generate(&mut ctx);
    parking::generate(&mut ctx);
    construction::generate(&mut ctx);
    buildings::generate(&mut ctx);
    scenery::generate(&mut ctx);

    let mut planned = plan.pois.clone();
    planned.extend(ctx.pois.iter().cloned());
    let pois: Vec<Poi> = planned
        .iter()
        .filter_map(|p| {
            let (lane, q, yaw) = nearest_lane(&roads.built, p.pos)?;
            Some(Poi {
                kind: p.kind,
                name: p.name.clone(),
                pos: q,
                heading_rad: yaw,
                lane: Some(lane),
            })
        })
        .collect();
    let mut features: Vec<Feature> = roads.features.clone();
    features.append(&mut ctx.features);
    let buildings = std::mem::take(&mut ctx.buildings);
    drop(ctx);

    let mut report = MetroReport::default();
    report.notes.extend(roads.notes.iter().cloned());
    let zones = plan.zones.iter().map(|z| z.zone.clone()).collect();
    let mut net = roads.built.clone().into_network(
        CITY_ID,
        &format!("procedural port_vesper v3 seed={}", cfg.seed),
        None,
        plan::CORE,
        zones,
        pois,
        buildings,
    );
    net.bounds_min = [
        net.bounds_min[0].min(plan::MAP_LO[0]),
        net.bounds_min[1].min(plan::MAP_LO[1]),
    ];
    net.bounds_max = [
        net.bounds_max[0].max(plan::MAP_HI[0]),
        net.bounds_max[1].max(plan::MAP_HI[1]),
    ];
    net.terrain = Some(terrain);
    for f in &features {
        *report.feature_counts.entry(f.kind).or_default() += 1;
    }
    net.features = features;
    report.notes.push(format!(
        "{} lanes, {} junctions, {} buildings, {} features, {} POIs",
        net.lanes.len(),
        net.intersections.len(),
        net.buildings.len(),
        net.features.len(),
        net.pois.len()
    ));
    (net, report)
}

/// Nearest drivable (non-parking, non-highway) lane point to `p`: lane, point, lane yaw.
fn nearest_lane(built: &Built, p: V2) -> Option<(LaneId, V3, f32)> {
    let mut best: Option<(f32, LaneId, V3, f32)> = None;
    for (li, l) in built.lanes.iter().enumerate() {
        if matches!(l.kind, LaneKind::Parking | LaneKind::Highway) {
            continue;
        }
        for w in l.polyline.windows(2) {
            let (a, b) = (geom::xy(w[0]), geom::xy(w[1]));
            let ab = geom::sub(b, a);
            let l2 = geom::dot(ab, ab);
            if l2 <= 0.0 {
                continue;
            }
            let t = (geom::dot(geom::sub(p, a), ab) / l2).clamp(0.0, 1.0);
            let q = [
                w[0][0] + (w[1][0] - w[0][0]) * t,
                w[0][1] + (w[1][1] - w[0][1]) * t,
                w[0][2] + (w[1][2] - w[0][2]) * t,
            ];
            let d = geom::dist(p, geom::xy(q));
            if best.as_ref().is_none_or(|b| d < b.0) {
                best = Some((d, li as LaneId, q, ctx::yaw_of(a, b)));
            }
        }
    }
    best.map(|(_, l, q, y)| (l, q, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_and_validates() {
        let (net, report) = generate_with_report(&MetroConfig::default());
        for n in &report.notes {
            eprintln!("{n}");
        }
        net.validate().expect("valid network");
        assert_eq!(net.id, CITY_ID);
        assert!(net.terrain.is_some());
    }
}
