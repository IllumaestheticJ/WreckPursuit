//! Wheels and tires (owner: wave-2 `mechanical`).
//!
//! Entry point: [`wheel_parts`], called once per wheel by the generator. Parts are in hub
//! space: origin at the hub centre, axle along +X, so the renderer can spin them. `+X` is the
//! car's +X for every wheel, so the outboard face is at `x = g.outboard · width / 2`
//! (`outboard` = −1 for left wheels, +1 for right wheels).
//!
//! - [`tire`]: lathed tire from a sidewall profile (bead, max section width, shoulder, tread
//!   edge) chosen by the tire family and aspect ratio, V-notched circumferential grooves and
//!   all-terrain shoulder lugs (`research_mechanical.md` §4.3).
//! - [`rim`]: the 14 rim designs of [`RimKind`](crate::style::RimKind) on a common lathed
//!   barrel / lip, lug nuts and centre caps facing outboard (§4.1–4.2).
//!
//! Sizes (rim diameter, lugs, finish …) come from the per-car table in
//! [`crate::mechanical::spec`], overridden by the fields the car's [`WheelStyle`] sets.

pub mod rim;
pub mod tire;

use wp_contracts::WheelDef;

use crate::mechanical::spec::{self, BrakeSpec, Susp, TireKind, WheelSpec};
use crate::mesh::MeshPart;
use crate::style::WheelStyle;

/// Wheel name tokens by index (FL, FR, RL, RR).
pub const WHEEL_NAMES: [&str; 4] = ["fl", "fr", "rl", "rr"];

/// Geometry of one wheel, resolved from the `VehicleDef` and the [`WheelStyle`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WheelGeom {
    /// Wheel index 0–3 (FL, FR, RL, RR).
    pub index: u8,
    /// −1 for left wheels (outboard face toward −X), +1 for right wheels.
    pub outboard: f32,
    /// Tire outer radius, m.
    pub radius: f32,
    /// Tire section width, m.
    pub width: f32,
    /// Rim (bead seat) radius, m.
    pub rim_radius: f32,
    /// Sidewall height / section width.
    pub tire_aspect: f32,
    /// Steerable (front) wheel.
    pub steerable: bool,
    /// Front axle (y ≥ 0).
    pub front: bool,
    /// Hub position in chassis space, m (for parts that need it; wheel parts stay hub-space).
    pub hub: [f32; 3],
    /// Resolved rim look (table + style overrides).
    pub spec: WheelSpec,
    /// Brakes of this wheel's axle.
    pub brake: BrakeSpec,
    /// Suspension layout of this wheel's axle.
    pub susp: Susp,
    /// Tire family.
    pub tire: TireKind,
    /// Hero (dense) geometry.
    pub hero: bool,
    /// Segments around the wheel (tire, rim, disc share it).
    pub segments: usize,
    /// Rim layout along the axle (lip, face, hub planes).
    pub layout: rim::RimLayout,
    /// Inboard distance of the knuckle centre from the hub centre plane, m.
    pub knuckle_in: f32,
    /// Live-axle half tube length inboard of the knuckle (0 = independent), m.
    pub axle_in: f32,
}

/// Inboard distance of the knuckle / carrier centre from the wheel centre plane (shared by
/// the pivot parts and the body-fixed arms so they meet).
pub fn knuckle_in(width: f32) -> f32 {
    (0.42 * width).clamp(0.075, 0.13)
}

impl WheelGeom {
    /// Resolve the geometry of wheel `index` from its definition and the style.
    pub fn new(index: u8, w: &WheelDef, style: &WheelStyle) -> WheelGeom {
        let car = spec::spec_for_wheel(index as usize, w);
        let radius = w.radius_m;
        let width = w.width_m;
        let ws = spec::apply_wheel(car.wheel, style, radius, width);
        let front = w.position_m[1] >= 0.0;
        let tire = TireKind::from_id(&w.tire_id);
        let layout = rim::RimLayout::new(&ws, width, radius, tire);
        let susp = if front { car.front_susp } else { car.rear_susp };
        let axle_in = if susp.live_axle() {
            (w.position_m[0].abs() - knuckle_in(width) - 0.15).max(0.1)
        } else {
            0.0
        };
        WheelGeom {
            index,
            outboard: if w.position_m[0] < 0.0 { -1.0 } else { 1.0 },
            radius,
            width,
            rim_radius: ws.rim_r,
            tire_aspect: (radius - ws.rim_r) / width.max(0.05),
            steerable: w.steerable,
            front,
            hub: w.position_m,
            spec: ws,
            brake: if front { car.front } else { car.rear },
            susp,
            tire,
            hero: car.hero,
            segments: spec::segments(car.hero, style),
            layout,
            knuckle_in: knuckle_in(width),
            axle_in,
        }
    }

    /// Segments of the rim / disc lathes (a little coarser than the tire: they sit inside
    /// it, so the silhouettes still nest).
    pub fn lathe_n(&self) -> usize {
        (self.segments * 5 / 6).max(12)
    }

    /// Part name `wheel_<fl|fr|rl|rr>_<suffix>`.
    pub fn name(&self, suffix: &str) -> String {
        format!("wheel_{}_{suffix}", WHEEL_NAMES[self.index as usize & 3])
    }
}

/// Tire and rim parts of one wheel (hub space, not skinned).
pub fn wheel_parts(g: &WheelGeom, _style: &WheelStyle) -> Vec<MeshPart> {
    let mut b = crate::builder::Builder::new();
    tire::tire(g, &mut b);
    rim::rim(g, &mut b);
    b.finish()
}
