//! Tire definition (`TireDef`, `data/tires/*.json`) and surface types.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Road/ground surface classification returned by [`crate::WorldQuery::surface_at`].
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
)]
pub enum SurfaceType {
    /// Dry asphalt (reference surface, multiplier 1.0).
    #[default]
    Asphalt,
    /// Wet asphalt.
    WetAsphalt,
    /// Concrete.
    Concrete,
    /// Loose gravel.
    Gravel,
    /// Grass.
    Grass,
    /// Packed dirt.
    Dirt,
    /// Kerb / curb.
    Curb,
    /// Metal plates, rails, bridge joints.
    Metal,
}

/// Simplified Pacejka Magic Formula coefficients for one axis:
/// `F = D · sin(C · atan(B·x − E·(B·x − atan(B·x))))`, with `D` a friction coefficient
/// (force = `D · Fz` before load sensitivity).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MagicFormula {
    /// Stiffness factor B, dimensionless (per rad for lateral, per unit slip ratio for longitudinal).
    pub b: f32,
    /// Shape factor C, dimensionless.
    pub c: f32,
    /// Peak factor D as a friction coefficient at `fz0_n`, dimensionless.
    pub d: f32,
    /// Curvature factor E, dimensionless.
    pub e: f32,
}

/// Tire compound definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TireDef {
    /// Unique id; must equal the file stem (`data/tires/<id>.json`).
    pub id: String,
    /// Lateral (slip-angle driven) Magic Formula coefficients.
    pub lateral: MagicFormula,
    /// Longitudinal (slip-ratio driven) Magic Formula coefficients.
    pub longitudinal: MagicFormula,
    /// Load sensitivity `ls`: effective `D = d · (1 − ls · (Fz/Fz0 − 1))`, dimensionless.
    pub load_sensitivity: f32,
    /// Nominal vertical load the coefficients were fitted at, N.
    pub fz0_n: f32,
    /// Relaxation length for the first-order slip lag, m.
    pub relaxation_length_m: f32,
    /// Rolling resistance coefficient (force = coeff · Fz), dimensionless.
    pub rolling_resistance: f32,
    /// Slip angle at peak lateral force, rad (informational / for assists).
    pub peak_slip_angle_rad: f32,
    /// Slip ratio at peak longitudinal force, dimensionless (for ABS/TCS targets).
    pub peak_slip_ratio: f32,
    /// Grip multiplier per surface. Missing surfaces default to 1.0.
    pub surface_multipliers: BTreeMap<SurfaceType, f32>,
    /// Tread wear per metre of sliding distance at nominal load, fraction of tread (0..1) per m.
    pub wear_rate: f32,
}
