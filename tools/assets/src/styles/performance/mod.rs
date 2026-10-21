//! Performance-car styles (owner: wave-2 `style-performance`).
//!
//! One [`CarStyle`] per performance car, following `docs/design/vehicles/research_performance.md`
//! (body knobs §2.1–2.3, detail lists §2.4, "top 5 cues per genre" §3):
//!
//! | Module | Cars |
//! |---|---|
//! | [`muscle`] | `ironside_440` (70s big-block coupe), `rampart_50` (2010s muscle coupe) |
//! | [`hatch`] | `vireo_xs` (90s FF hot hatch), `lindwurm_4t` (2010s AWD hot hatch) |
//! | [`tuner`] | `kestrel_ts` (rally sedan), `shiro_turbo` (FR drift coupe), `hakone_gt` (90s GT), `sperber_32` (sport sedan), `debug_coupe` (sport coupe) |
//! | [`exotic`] | `lucciola_350` (90s mid-engine wedge), `fulmine_v10` (2010s mid-engine supercar) |
//!
//! Each car = body knobs + one custom "body kit" detail function (see [`kit::Kit`]) that
//! replaces the foundation's generic lamps / mirrors with the car's own shapes and adds the
//! genre details, + wheel / mechanical settings for the mechanical agent's renderers, + a
//! paint tint. Every car gets a distinct tail-lamp pattern (the night-time identifier).

use wp_contracts::{BodyRegion, VehicleDef};

use crate::mesh::MaterialSlot;
use crate::style::{CapKind, CarStyle, Detail, DetailFn, RimKind, Shape};

pub mod exotic;
pub mod hatch;
pub mod kit;
pub mod muscle;
pub mod tuner;

/// Style for a performance car id, or `None` if this module does not own it.
pub fn style(def: &VehicleDef) -> Option<CarStyle> {
    let s = match def.id.as_str() {
        "ironside_440" => muscle::ironside(def),
        "rampart_50" => muscle::rampart(def),
        "vireo_xs" => hatch::vireo(def),
        "lindwurm_4t" => hatch::lindwurm(def),
        "kestrel_ts" => tuner::kestrel(def),
        "shiro_turbo" => tuner::shiro(def),
        "hakone_gt" => tuner::hakone(def),
        "sperber_32" => tuner::sperber(def),
        "debug_coupe" => tuner::debug_coupe(def),
        "lucciola_350" => exotic::lucciola(def),
        "fulmine_v10" => exotic::fulmine(def),
        _ => return None,
    };
    Some(s)
}

/// The car's body-kit detail: one custom function that emits every kit part.
fn kit_detail(f: DetailFn) -> Detail {
    Detail::new(
        "body_kit",
        BodyRegion::Cabin,
        MaterialSlot::Trim,
        Shape::Custom(f),
    )
}

/// sRGB from a hex colour.
const fn rgb(hex: u32) -> [u8; 3] {
    [(hex >> 16) as u8, (hex >> 8) as u8, hex as u8]
}

/// Wheel settings shared by every car: rim kind, spokes, rim radius as a fraction of the
/// front tire radius, rim material, centre cap, dish (face depth, lip width), camber.
#[allow(clippy::too_many_arguments)]
fn wheels(
    s: &mut CarStyle,
    def: &VehicleDef,
    rim: RimKind,
    spokes: u8,
    rim_frac: f32,
    mat: MaterialSlot,
    cap: CapKind,
    dish: (f32, f32),
    camber: (f32, f32),
) {
    let r = def.wheels.first().map(|w| w.radius_m).unwrap_or(0.32);
    s.wheel.rim = rim;
    s.wheel.spokes = spokes;
    s.wheel.rim_radius_m = Some(rim_frac * r);
    s.wheel.rim_material = mat;
    s.wheel.cap = cap;
    s.wheel.face_depth_m = dish.0;
    s.wheel.lip_width_m = dish.1;
    s.wheel.front_camber_deg = camber.0;
    s.wheel.rear_camber_deg = camber.1;
    s.wheel.segments = 24;
}
