//! Real-car model styles (painted-car pipeline, [`crate::style::Tier::Painted`]).
//!
//! Each family has a measured section spec `data/panels/models/<family>.json`
//! ([`crate::section::SectionSpec`]) and a style module here that adds the family's kit (lamp
//! lenses, wing, mirrors) and per-trim settings. Car ids map to (family, trim) in [`MODELS`];
//! [`style`] loads the family file, fits the trim to the car's axles and returns the style with
//! `BodyStyle::sections` set. Detail (grilles, intakes, badges, plates, handles, gaps, spokes)
//! is painted into the car's texture by [`crate::paint`] from the trim's `texture_layers`.
//!
//! | Family | Ids (trim) |
//! |---|---|
//! | `nissan_skyline_r34` ([`skyline_r34`]) | `nissan_skyline_r34_25gt` (`25gt`), `nissan_skyline_r34_25gt_t` (`25gt_t`), `nissan_skyline_r34_gtr` (`gtr`), `nissan_skyline_r34_gtr_vspec2` (`gtr_vspec2`) |

pub mod skyline_r34;

use std::sync::Arc;

use wp_contracts::VehicleDef;

use crate::section::SectionSpec;
use crate::style::CarStyle;

/// Car id → (family file stem, trim key).
pub const MODELS: &[(&str, &str, &str)] = &[
    ("nissan_skyline_r34_25gt", "nissan_skyline_r34", "25gt"),
    ("nissan_skyline_r34_25gt_t", "nissan_skyline_r34", "25gt_t"),
    ("nissan_skyline_r34_gtr", "nissan_skyline_r34", "gtr"),
    (
        "nissan_skyline_r34_gtr_vspec2",
        "nissan_skyline_r34",
        "gtr_vspec2",
    ),
];

/// (family, trim) of a car id, if a model style owns it.
pub fn model_of(id: &str) -> Option<(&'static str, &'static str)> {
    MODELS
        .iter()
        .find(|(i, _, _)| *i == id)
        .map(|&(_, f, t)| (f, t))
}

/// The section spec of a car (its family's trim, fitted to its front axle), if it is a model.
/// Panics with the file's problems if the family file is missing or invalid (a model id
/// without its spec is a data error, caught by `cargo test -p car-assets`).
pub fn spec_for(def: &VehicleDef) -> Option<SectionSpec> {
    let (family, trim) = model_of(&def.id)?;
    let spec = SectionSpec::load(&crate::repo_root(), family)
        .unwrap_or_else(|e| panic!("{}: section spec: {e}", def.id));
    let front_axle = def
        .wheels
        .iter()
        .map(|w| w.position_m[1])
        .fold(f32::MIN, f32::max);
    let mut fitted = spec.for_car(trim, front_axle);
    fitted.active_trim = trim.to_string();
    Some(fitted)
}

/// Style for a model car id, or `None` if no model module owns it.
pub fn style(def: &VehicleDef) -> Option<CarStyle> {
    let spec = Arc::new(spec_for(def)?);
    Some(match spec.family.as_str() {
        "nissan_skyline_r34" => skyline_r34::style(def, spec),
        other => panic!("{}: no style module for family {other}", def.id),
    })
}
