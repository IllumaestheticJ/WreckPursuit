//! Performance-car styles (owner: wave-2 `style-performance`).
//!
//! One [`CarStyle`] per performance car, following `docs/design/vehicles/research_performance.md`
//! (body knobs §2.1–2.3, detail lists §2.4, "top 5 cues per genre" §3):
//!
//! | Module | Cars |
//! |---|---|
//! | [`muscle`] | `ironside_440` (70s big-block coupe), `rampart_50` (2010s muscle coupe), `bulwark_400` / `brigand_400` (70s four-door sedans, ironside family) |
//! | [`hatch`] | `vireo_xs` (90s FF hot hatch), `lindwurm_4t` (2010s AWD hot hatch), `stoat_16v` / `marten_20` (early-90s FF hot hatches, vireo family) |
//! | [`tuner`] | `kestrel_ts` (rally sedan), `arashi_t4` / `talus_20t` (90s AWD rally saloons, kestrel family), `shiro_turbo` (FR drift coupe), `hakone_gt` (90s GT), `sperber_32` (sport sedan), `debug_coupe` (sport coupe) |
//! | [`exotic`] | `lucciola_350` (90s mid-engine wedge), `fulmine_v10` (2010s mid-engine supercar) |
//! | [`supercar`] | `halberd_40`, `ombra_v12`, `kagero_hx` (mid-engine), `ostwind_6t` (rear-engine) |
//! | [`race`] | `morrigan_55` (GT endurance), `nyx_proto` (sports prototype), `merlo_20t` (touring car) |
//! | [`offroad`] | `mesquite_pr` (pre-runner pickup), `erg_hamada` (rally-raid prototype) |
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
pub mod offroad;
pub mod race;
pub mod supercar;
pub mod tuner;

/// Style for a performance car id, or `None` if this module does not own it.
pub fn style(def: &VehicleDef) -> Option<CarStyle> {
    let s = match def.id.as_str() {
        "ironside_440" => muscle::ironside(def),
        "rampart_50" => muscle::rampart(def),
        "bulwark_400" => muscle::bulwark(def),
        "brigand_400" => muscle::brigand(def),
        "vireo_xs" => hatch::vireo(def),
        "stoat_16v" => hatch::stoat(def),
        "marten_20" => hatch::marten(def),
        "lindwurm_4t" => hatch::lindwurm(def),
        "kestrel_ts" => tuner::kestrel(def),
        "arashi_t4" => tuner::arashi(def),
        "talus_20t" => tuner::talus(def),
        "shiro_turbo" => tuner::shiro(def),
        "hakone_gt" => tuner::hakone(def),
        "sperber_32" => tuner::sperber(def),
        "debug_coupe" => tuner::debug_coupe(def),
        "lucciola_350" => exotic::lucciola(def),
        "fulmine_v10" => exotic::fulmine(def),
        "halberd_40" => supercar::halberd(def),
        "ombra_v12" => supercar::ombra(def),
        "ostwind_6t" => supercar::ostwind(def),
        "kagero_hx" => supercar::kagero(def),
        "morrigan_55" => race::morrigan(def),
        "nyx_proto" => race::nyx(def),
        "merlo_20t" => race::merlo(def),
        "mesquite_pr" => offroad::mesquite(def),
        "erg_hamada" => offroad::erg(def),
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
