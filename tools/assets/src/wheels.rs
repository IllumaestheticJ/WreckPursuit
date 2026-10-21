//! Wheels and tires (owner: wave-2 `mechanical`).
//!
//! Entry point: [`wheel_parts`], called once per wheel by the generator. Parts are in hub
//! space: origin at the hub centre, axle along +X, so the renderer can spin them. `+X` is the
//! car's +X for every wheel, so the outboard face is at `x = g.outboard · width / 2`
//! (`outboard` = −1 for left wheels, +1 for right wheels).
//!
//! Today's content (foundation default for every [`RimKind`]): a 3-quad-per-segment tire and
//! the original flat 10-point star rim. The mechanical agent replaces this with real rim
//! styles and tire profiles (`research_mechanical.md` §4).

use wp_contracts::WheelDef;

use crate::builder::{Builder, PartKey};
use crate::mesh::{MaterialSlot, MeshPart, PartKind};
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
}

impl WheelGeom {
    /// Resolve the geometry of wheel `index` from its definition and the style.
    pub fn new(index: u8, w: &WheelDef, style: &WheelStyle) -> WheelGeom {
        let radius = w.radius_m;
        let width = w.width_m;
        let rim_radius = match (style.tire_aspect, style.rim_radius_m) {
            (Some(a), _) => radius - a.clamp(0.15, 0.9) * width,
            (None, Some(r)) => r,
            (None, None) => 0.64 * radius,
        }
        .clamp(0.4 * radius, 0.9 * radius);
        WheelGeom {
            index,
            outboard: if w.position_m[0] < 0.0 { -1.0 } else { 1.0 },
            radius,
            width,
            rim_radius,
            tire_aspect: (radius - rim_radius) / width.max(0.05),
            steerable: w.steerable,
            front: w.position_m[1] >= 0.0,
            hub: w.position_m,
        }
    }

    /// Part name `wheel_<fl|fr|rl|rr>_<suffix>`.
    pub fn name(&self, suffix: &str) -> String {
        format!("wheel_{}_{suffix}", WHEEL_NAMES[self.index as usize & 3])
    }
}

/// Tire and rim parts of one wheel (hub space, not skinned).
pub fn wheel_parts(g: &WheelGeom, style: &WheelStyle) -> Vec<MeshPart> {
    let n = (style.segments as usize).clamp(8, 48);
    let hw = 0.5 * g.width;
    let mut b = Builder::new();
    // Tire: inner sidewall, tread, outer sidewall (lathe about +X).
    let tire = PartKey::wheel(
        &g.name("tire"),
        g.index,
        PartKind::Tire,
        MaterialSlot::Rubber,
    );
    b.lathe(
        &tire,
        [0.0; 3],
        [1.0, 0.0, 0.0],
        &[
            [g.rim_radius, -hw],
            [g.radius, -hw],
            [g.radius, hw],
            [g.rim_radius, hw],
        ],
        n,
    );
    // Rim: flat 10-point star on both faces (spin stays visible).
    let rim = PartKey::wheel(&g.name("rim"), g.index, PartKind::Rim, style.rim_material);
    for x in [-hw * 0.6, hw * 0.6] {
        let pt = |i: usize| {
            let (s, c) = crate::builder::turn(i, 10);
            let r = if i.is_multiple_of(2) {
                g.rim_radius * 1.02
            } else {
                g.rim_radius * 0.55
            };
            [x, s * r, c * r]
        };
        for i in 0..10 {
            let (a, c) = (pt(i), pt((i + 1) % 10));
            if x > 0.0 {
                b.poly(&rim, &[[x, 0.0, 0.0], c, a]);
            } else {
                b.poly(&rim, &[[x, 0.0, 0.0], a, c]);
            }
        }
    }
    b.finish()
}
