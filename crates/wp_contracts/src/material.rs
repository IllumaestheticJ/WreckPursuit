//! Damage-lattice material definitions (`MaterialDef`, `data/materials/*.json`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Mechanical behaviour family of a material.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum MaterialKind {
    /// Thin body panel: dents easily, plastic.
    Panel,
    /// Structural member (rails, pillars, crossmembers): high yield, hardening, buckles.
    Structural,
    /// Brittle; no plastic range, shatters.
    Glass,
    /// Polymer (bumper covers): low stiffness, large failure strain, cracks and detaches.
    Plastic,
    /// Cast metal (knuckles, hubs): stiff, brittle failure.
    Cast,
}

/// Material properties used to derive lattice beam stiffness, yield and failure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MaterialDef {
    /// Unique id; must equal the file stem (`data/materials/<id>.json`).
    pub id: String,
    /// Density, kg/m³.
    pub density_kg_m3: f32,
    /// Young's modulus, MPa.
    pub young_mpa: f32,
    /// Yield strength, MPa.
    pub yield_mpa: f32,
    /// Plastic (tangent) modulus after yield, MPa.
    pub plastic_modulus_mpa: f32,
    /// Engineering strain at which a beam of this material breaks, dimensionless.
    pub failure_strain: f32,
    /// Coefficient of restitution for contacts, 0..1.
    pub restitution: f32,
    /// Coulomb friction coefficient for contacts.
    pub friction: f32,
    /// Behaviour family.
    pub kind: MaterialKind,
}
