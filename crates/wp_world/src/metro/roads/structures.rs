//! Road structures as [`Feature`]s (filled in below).

use super::super::ground::Ground;
use super::super::plan::Plan;
use super::highways::Hw;
use super::{Crossing, RoadInfo};
use crate::builder::{Built, RoadSpec};
use crate::network::Feature;

/// Every road structure feature.
#[allow(clippy::too_many_arguments)]
pub fn features(
    _plan: &Plan,
    _ground: &Ground,
    _built: &Built,
    _specs: &[RoadSpec],
    _info: &[RoadInfo],
    _names: &[String],
    _hws: &[Hw],
    _crossings: &[Crossing],
) -> Vec<Feature> {
    Vec::new()
}
