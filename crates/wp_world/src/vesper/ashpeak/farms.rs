//! Harlan Flats: farmland west of Port Vesper (and the north-west foothill farms).

use super::super::*;

/// Lay out this district's roads.
pub(super) fn layout(_g: &mut Gen) {}

/// Covered roads (tunnels) on this district's roads.
pub(super) fn covers() -> Vec<CoverSpec> {
    Vec::new()
}

/// This district's zones.
pub(super) fn zones() -> Vec<Zone> {
    Vec::new()
}

/// Whether the generic street-front building pass may build at `p`.
pub(super) fn generic_buildings_at(_p: V2) -> bool {
    true
}

/// Hand-placed buildings and decor.
pub(super) fn extra_buildings(_site: &mut Site) -> (Vec<Building>, Vec<Decor>) {
    (Vec::new(), Vec::new())
}

/// Points of interest.
pub(super) fn pois(_site: &mut Site) -> Vec<Poi> {
    Vec::new()
}
