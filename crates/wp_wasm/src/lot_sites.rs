//! Car lots on the city map (feature F37): every lot of `lots.toml` (dealerships, used lots,
//! junkyards; PR #57) gets a spot on a street lane of the city, in a zone of the lot's district
//! when the city has one, away from the garages, safehouses and the other lots. Stopping there
//! offers to browse the lot (the testbed opens PR #57's lot visit). Pure function of the world
//! and the lot table, so every session of a city puts the lots in the same places.

use serde::Serialize;
use wp_contracts::DistrictId;
use wp_economy::{LotDef, LotKind};
use wp_world::network::{LaneKind, PoiKind};
use wp_world::World;

/// Radius around a lot's spot that counts as "at the lot", m.
pub const LOT_RADIUS_M: f32 = 15.0;

/// Candidate spots are taken along street lanes every this many metres.
const STEP_M: f32 = 40.0;

/// A lot keeps at least this far from a garage, a safehouse or another lot, m.
const SPACING_M: f32 = 180.0;

/// A lane more than this far above the terrain is a deck, not a street, m.
const DECK_GAP_M: f32 = 3.0;

/// One car lot on the map (`lot_sites_json`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LotSite {
    /// Lot id (`lots.toml`).
    pub id: String,
    /// Name.
    pub name: String,
    /// Dealership, used lot or junkyard.
    pub kind: LotKind,
    /// Tier 1 ..= 3 (dealerships).
    pub tier: u8,
    /// Position on its street lane, m.
    pub pos: [f32; 3],
    /// Lane direction (yaw about +Z, 0 = +Y), rad.
    pub heading_rad: f32,
    /// Radius that counts as at the lot, m.
    pub radius_m: f32,
    /// The district it is in.
    pub district: DistrictId,
}

/// Street-lane candidate spots of `world`: `(pos, yaw, district)` every [`STEP_M`] along
/// every street-level, non-highway, non-parking lane, in lane order.
fn candidates(world: &World) -> Vec<([f32; 3], f32, Option<DistrictId>)> {
    let net = world.network();
    let mut out = Vec::new();
    for l in &net.lanes {
        if !l.kind.drivable() || matches!(l.kind, LaneKind::Highway | LaneKind::Parking) {
            continue;
        }
        if net.surfaces.iter().any(|s| s.road == l.road && s.highway) {
            continue;
        }
        let mut since = STEP_M * 0.5;
        for w in l.polyline.windows(2) {
            let (a, b) = (w[0], w[1]);
            let len = (b[0] - a[0]).hypot(b[1] - a[1]);
            if len <= 0.0 {
                continue;
            }
            let mut t = since;
            while t < len {
                let f = t / len;
                let p = [
                    a[0] + (b[0] - a[0]) * f,
                    a[1] + (b[1] - a[1]) * f,
                    a[2] + (b[2] - a[2]) * f,
                ];
                let deck = net
                    .terrain
                    .as_ref()
                    .is_some_and(|tf| p[2] - tf.height_at([p[0], p[1]]) > DECK_GAP_M);
                if !deck {
                    let yaw = wp_math::atan2(-(b[0] - a[0]), b[1] - a[1]);
                    out.push((p, yaw, world.map().district_at([p[0], p[1]])));
                }
                t += STEP_M;
            }
            since = t - len;
        }
    }
    out
}

/// Where every lot of `lots` goes on `world` (lot order). A lot whose district the city lacks
/// goes to the nearest free spot of any district; a city with no street lanes gets none.
pub fn lot_sites(world: &World, lots: &[LotDef]) -> Vec<LotSite> {
    let cands = candidates(world);
    if cands.is_empty() {
        return Vec::new();
    }
    let net = world.network();
    let mut taken: Vec<[f32; 2]> = net
        .pois
        .iter()
        .filter(|p| matches!(p.kind, PoiKind::Garage | PoiKind::SafehouseCandidate))
        .map(|p| [p.pos[0], p.pos[1]])
        .chain(
            crate::garage_visit::sites(world)
                .iter()
                .map(|g| [g.pos[0], g.pos[1]]),
        )
        .collect();
    // The middle of each district's candidate spots: lots aim there (the district's heart).
    let centre_of = |d: DistrictId| -> Option<[f32; 2]> {
        let pts: Vec<[f32; 2]> = cands
            .iter()
            .filter(|c| c.2 == Some(d))
            .map(|c| [c.0[0], c.0[1]])
            .collect();
        (!pts.is_empty()).then(|| {
            let n = pts.len() as f32;
            [
                pts.iter().map(|p| p[0]).sum::<f32>() / n,
                pts.iter().map(|p| p[1]).sum::<f32>() / n,
            ]
        })
    };
    let all = {
        let n = cands.len() as f32;
        [
            cands.iter().map(|c| c.0[0]).sum::<f32>() / n,
            cands.iter().map(|c| c.0[1]).sum::<f32>() / n,
        ]
    };
    let mut out = Vec::new();
    for lot in lots {
        let target = centre_of(lot.district);
        let aim = target.unwrap_or(all);
        let free = |c: &&([f32; 3], f32, Option<DistrictId>)| {
            taken
                .iter()
                .all(|t| (t[0] - c.0[0]).hypot(t[1] - c.0[1]) >= SPACING_M)
        };
        let dist =
            |c: &([f32; 3], f32, Option<DistrictId>)| (c.0[0] - aim[0]).hypot(c.0[1] - aim[1]);
        let pick = cands
            .iter()
            .filter(|c| target.is_none() || c.2 == Some(lot.district))
            .filter(free)
            .min_by(|a, b| dist(a).total_cmp(&dist(b)))
            .or_else(|| {
                cands
                    .iter()
                    .filter(free)
                    .min_by(|a, b| dist(a).total_cmp(&dist(b)))
            });
        let Some(&(pos, heading_rad, district)) = pick else {
            continue;
        };
        taken.push([pos[0], pos[1]]);
        out.push(LotSite {
            id: lot.id.clone(),
            name: lot.name.clone(),
            kind: lot.kind,
            tier: lot.tier,
            pos,
            heading_rad,
            radius_m: LOT_RADIUS_M,
            district: district.unwrap_or(DistrictId::Residential),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_lot_has_a_spot_in_every_city_apart_from_the_garages() {
        use crate::data::mem_data;
        let lots = crate::garage::with_economy(|e| e.lots.lots.clone());
        assert!(!lots.is_empty());
        for id in ["vesper_v2", "procedural_12x12", "ashpeak"] {
            let sc = crate::scenario::free_drive_in(
                "debug_sedan",
                wp_contracts::DamageMode::Arcade,
                crate::scenario::city_spec(id),
            );
            let world =
                simcli::data::load_world(mem_data(), &sc.city, simcli::run::world_config(&sc))
                    .unwrap()
                    .unwrap();
            let sites = lot_sites(&world, &lots);
            assert_eq!(sites.len(), lots.len(), "{id}");
            let garages = crate::garage_visit::sites(&world);
            for s in &sites {
                assert!(
                    garages
                        .iter()
                        .all(|g| (g.pos[0] - s.pos[0]).hypot(g.pos[1] - s.pos[1]) >= SPACING_M),
                    "{id}: {} next to a garage",
                    s.name
                );
            }
            // Deterministic.
            assert_eq!(sites, lot_sites(&world, &lots));
        }
    }
}
