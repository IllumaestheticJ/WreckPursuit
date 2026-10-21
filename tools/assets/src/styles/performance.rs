//! Performance-car styles (owner: wave-2 `style-performance`).
//!
//! `debug_coupe` below is the foundation's worked example of the API: a body style built from
//! a silhouette preset, a few details (front splitter, rear spoiler on two stands, side
//! skirts as surface patches, a grille), a custom detail function and wheel / mechanical
//! tweaks. Replace or extend it freely.

use wp_contracts::{BodyRegion, VehicleDef};

use crate::builder::{Builder, PartKey, Patch, PatchBand};
use crate::mesh::{MaterialSlot, PartKind};
use crate::style::{
    BodyStyle, CaliperKind, CarStyle, Detail, DetailCtx, EngineKind, ExhaustKind, HoodScoop, Shape,
    Silhouette,
};

/// Style for a performance car id, or `None` if this module does not own it.
pub fn style(def: &VehicleDef) -> Option<CarStyle> {
    match def.id.as_str() {
        "debug_coupe" => Some(debug_coupe()),
        _ => None,
    }
}

/// Worked example: a fastback sport coupe.
fn debug_coupe() -> CarStyle {
    let body = BodyStyle {
        roof_dz: -0.02,
        cabin_shift_m: -0.08,
        haunch: 0.03,
        flare: 0.015,
        crease_depth: 0.012,
        crease_z: 0.50,
        hood_scoop: HoodScoop::Vents,
        hood_scoop_h: 0.05,
        power_bulge_h: 0.025,
        a_pillar_rake_deg: 8.0,
        stance_pitch_deg: 0.3,
        ..BodyStyle::preset(Silhouette::Fastback)
    };
    let mut s = CarStyle::with_body(body);
    // Front splitter: a carbon plate under the bumper, skinned to the bumper (falls off with it).
    s.details.push(Detail::new(
        "splitter",
        BodyRegion::FrontBumper,
        MaterialSlot::Carbon,
        Shape::Custom(splitter),
    ));
    // Rear spoiler: a custom function that reads the deck height from the profile.
    s.details.push(Detail::new(
        "spoiler",
        BodyRegion::Trunk,
        MaterialSlot::Carbon,
        Shape::Custom(spoiler),
    ));
    // Side skirts: surface patches on the left sill, mirrored to the right.
    s.details.push(
        Detail::new(
            "skirt_l",
            BodyRegion::FrameLeft,
            MaterialSlot::Trim,
            Shape::Patch(Patch {
                y0: -0.75,
                y1: 0.65,
                band: PatchBand::Side { z0: 0.17, z1: 0.27 },
                offset_m: 0.0,
                thickness_m: 0.02,
                steps: (6, 2),
            }),
        )
        .mirrored(),
    );
    s.wheel.rim_material = MaterialSlot::Chrome;
    s.wheel.rear_camber_deg = 1.5;
    s.mechanical.engine = EngineKind::Inline6;
    s.mechanical.exhaust = ExhaustKind::Dual;
    s.mechanical.front_brakes.caliper = CaliperKind::Fixed4;
    s.mechanical.caliper_rgb = Some([200, 30, 30]);
    s.tints.push((MaterialSlot::Grille, [20, 20, 22]));
    s
}

/// Splitter: thin plate across the nose just under the bumper, plus a grille on the nose face.
fn splitter(ctx: &DetailCtx, key: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let w = p.half_width(p.y_nose) - 0.05;
    let z = p.sill(p.y_nose);
    b.beveled_box(
        key,
        [-w, p.y_nose - 0.25, z - 0.02],
        [w, p.y_nose + 0.05, z + 0.005],
        0.008,
    );
    let grille = PartKey::body(
        "grille",
        BodyRegion::FrontBumper,
        PartKind::Detail,
        MaterialSlot::Grille,
    );
    let zt = p.z_bumper_top.0 - 0.04;
    b.cuboid(
        &grille,
        [-0.35, p.y_nose - 0.01, zt - 0.14],
        [0.35, p.y_nose + 0.006, zt],
    );
}

/// Spoiler: wing on two stands at the trailing edge of the deck.
fn spoiler(ctx: &DetailCtx, key: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let y = p.y_tail + 0.22;
    let deck = p.top_point(y, 0.0).0[2];
    let w = p.half_width(y) - 0.12;
    b.beveled_box(
        key,
        [-w, y - 0.12, deck + 0.10],
        [w, y + 0.06, deck + 0.125],
        0.006,
    );
    for x in [-0.6 * w, 0.6 * w] {
        b.cuboid(
            key,
            [x - 0.012, y - 0.06, deck - 0.01],
            [x + 0.012, y + 0.02, deck + 0.105],
        );
    }
}
