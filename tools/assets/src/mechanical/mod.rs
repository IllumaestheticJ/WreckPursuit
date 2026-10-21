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
//! The per-wheel calls only receive a `WheelDef`, so [`parts`] (which the generator runs
//! first, with the whole `VehicleDef`) records the car in a thread-local
//! ([`spec::set_current`]); [`crate::wheels::WheelGeom::new`] looks it up by hub position.

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
use crate::style::{MechStyle, WheelStyle};
use crate::wheels::WheelGeom;

/// Body-fixed mechanical parts (engine, suspension, exhaust, frame), chassis space.
pub fn parts(def: &VehicleDef, p: &BodyProfile, m: &MechStyle, b: &mut Builder) {
    let tier = crate::style::style_for(def).tier;
    let spec = spec::apply_mech(def, spec::base_spec(def, tier), m);
    spec::set_current(def, spec);
    let out = engine::engine(def, p, &spec, b);
    suspension::suspension(def, p, &spec, &out, b);
    exhaust::exhaust(def, p, &spec, &out, b);
}

/// Brake disc / drum + caliper + knuckle of one wheel, hub space.
pub fn brake_parts(g: &WheelGeom, _m: &MechStyle, _w: &WheelStyle) -> Vec<MeshPart> {
    let mut b = Builder::new();
    let place = brakes::brake(g, &mut b);
    brakes::pivot_hardware(g, &place, &mut b);
    b.finish()
}
