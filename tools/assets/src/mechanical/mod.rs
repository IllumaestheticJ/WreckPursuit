//! Engine, suspension, exhaust and brakes (owner: wave-2 `mechanical`).
//!
//! Entry points called by the generator:
//! - [`parts`]: body-fixed hardware (engine, suspension, frame, exhaust) into the car's
//!   [`Builder`], chassis space. Rigid keys (`PartKey::rigid()`: one lattice node, weight 255,
//!   moved by matrix, never re-skinned). Kinds: `Engine`, `Suspension`.
//! - [`brake_parts`]: per-wheel brake disc / drum (`BrakeDisc`, spins) and caliper, knuckle,
//!   strut lower body, live-axle half tube (`Caliper` / `Upright`: steer and follow
//!   compression, do not spin), hub space (axle +X).
//!
//! What gets drawn comes from [`spec`]: a per-car table (`research_mechanical.md` §1.3, §2.3,
//! §3.3, §4.4) overridden by the [`MechStyle`] / [`WheelStyle`] fields a style module set.
//! Detail follows the budget tier (`research_mechanical.md` §5): hero corners ≈ 1.2–1.5k
//! tris, traffic ≈ 400–550; the detailed engine only where [`spec::CarSpec::engine_detail`]
//! or `MechStyle::show_engine_detail` asks for it.
//!
//! The generator resolves the car's [`spec::CarSpec`] once ([`car_spec`]) and passes it to
//! [`parts_with_spec`] and [`crate::wheels::WheelGeom::new`]; [`brake_parts`] reads it from the
//! [`WheelGeom`] (no hidden per-thread state).

pub mod brakes;
pub mod engine;
pub mod exhaust;
pub mod prim;
pub mod spec;
pub mod suspension;

use wp_contracts::VehicleDef;

use crate::builder::Builder;
use crate::mesh::MeshPart;
use crate::profile::BodyProfile;
use crate::style::{MechStyle, Tier, WheelStyle};
use crate::wheels::WheelGeom;
use spec::CarSpec;

/// The resolved mechanical / wheel spec of a car: the per-id table (class fallback) for its
/// budget tier, with the set fields of `m` applied.
pub fn car_spec(def: &VehicleDef, m: &MechStyle, tier: Tier) -> CarSpec {
    spec::apply_mech(def, spec::base_spec(def, tier), m)
}

/// Body-fixed mechanical parts (engine, suspension, exhaust, frame), chassis space, for the
/// car's [`style_for`](crate::style::style_for) tier. Prefer [`parts_with_spec`] when the spec
/// is already resolved.
pub fn parts(def: &VehicleDef, p: &BodyProfile, m: &MechStyle, b: &mut Builder) {
    let spec = car_spec(def, m, crate::style::style_for(def).tier);
    parts_with_spec(def, p, &spec, b);
}

/// Body-fixed mechanical parts from a resolved spec ([`car_spec`]).
pub fn parts_with_spec(def: &VehicleDef, p: &BodyProfile, spec: &CarSpec, b: &mut Builder) {
    let out = engine::engine(def, p, spec, b);
    suspension::suspension(def, p, spec, &out, b);
    exhaust::exhaust(def, p, spec, &out, b);
}

/// Brake disc / drum + caliper + knuckle of one wheel, hub space.
pub fn brake_parts(g: &WheelGeom, _m: &MechStyle, _w: &WheelStyle) -> Vec<MeshPart> {
    let mut b = Builder::new();
    let place = brakes::brake(g, &mut b);
    brakes::pivot_hardware(g, &place, &mut b);
    b.finish()
}
