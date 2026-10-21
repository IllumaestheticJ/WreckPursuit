//! Engine, suspension, exhaust and brakes (owner: wave-2 `mechanical`).
//!
//! Entry points called by the generator:
//! - [`parts`]: body-fixed hardware (engine, suspension, exhaust) into the car's [`Builder`],
//!   chassis space. Use rigid keys (`PartKey::rigid()`): one lattice node, weight 255, so the
//!   renderer moves them without re-skinning. Kinds: `Engine`, `Suspension`.
//! - [`brake_parts`]: per-wheel brake disc (`BrakeDisc`, spins) and caliper / upright
//!   (`Caliper` / `Upright`: steer and follow compression, do not spin), hub space (axle +X).
//!
//! Today's content is a minimal placeholder (block engine, one strut per corner, exhaust
//! tips, plain disc + box caliper + upright) so every part kind and material is exercised.
//! See `research_mechanical.md` for the real thing.

use wp_contracts::{BodyRegion, VehicleDef};

use crate::builder::{Builder, PartKey};
use crate::mesh::{MaterialSlot, MeshPart, PartKind};
use crate::profile::BodyProfile;
use crate::style::{CaliperKind, EngineKind, EngineLayout, ExhaustKind, MechStyle, WheelStyle};
use crate::wheels::WheelGeom;

/// Body-fixed mechanical parts (engine, suspension, exhaust), chassis space.
pub fn parts(def: &VehicleDef, p: &BodyProfile, m: &MechStyle, b: &mut Builder) {
    let front_axle = def.wheels[0].position_m[1];
    let rear_axle = def.wheels[2].position_m[1];
    // Engine block.
    if m.engine != EngineKind::None {
        let (region, yc) = match m.layout {
            EngineLayout::Front => (
                BodyRegion::FrameFront,
                0.5 * (front_axle + p.lattice_y_fw) + 0.1,
            ),
            EngineLayout::Mid => (BodyRegion::FrameRear, rear_axle + 0.55),
            EngineLayout::Rear => (BodyRegion::FrameRear, rear_axle - 0.35),
        };
        let len = match m.engine {
            EngineKind::Inline6 | EngineKind::V10 | EngineKind::V12 => 0.75,
            EngineKind::V8 => 0.62,
            _ => 0.5,
        };
        let top = match m.layout {
            EngineLayout::Front => p.z_hood.0.min(p.z_hood.1) - 0.08,
            _ => p.z_deck - 0.08,
        };
        let key = PartKey::body(
            "engine_block",
            region,
            PartKind::Engine,
            MaterialSlot::Engine,
        )
        .rigid();
        let z0 = p.z_floor + 0.18;
        b.beveled_box(
            &key,
            [-0.28, yc - 0.5 * len, z0],
            [0.28, yc + 0.5 * len, top.max(z0 + 0.2)],
            0.03,
        );
        if m.show_engine_detail {
            let cover =
                PartKey::body("engine_cover", region, PartKind::Engine, MaterialSlot::Trim).rigid();
            b.beveled_box(
                &cover,
                [-0.18, yc - 0.4 * len, top - 0.02],
                [0.18, yc + 0.4 * len, top + 0.03],
                0.015,
            );
        }
    }
    // One strut per corner (from above the hub up toward the body).
    for (i, w) in def.wheels.iter().enumerate() {
        let [x, y, z] = w.position_m;
        let region = if y >= 0.0 {
            BodyRegion::FrameFront
        } else {
            BodyRegion::FrameRear
        };
        let name = format!("strut_{}", crate::wheels::WHEEL_NAMES[i]);
        let key = PartKey::body(&name, region, PartKind::Suspension, MaterialSlot::Metal).rigid();
        let xi = x - x.signum() * 0.12;
        b.cylinder(
            &key,
            [xi, y, z + 0.12],
            [xi - x.signum() * 0.06, y, z + 0.45],
            0.03,
            0.03,
            8,
            false,
        );
    }
    // Exhaust tips under the rear bumper.
    let tips: &[f32] = match m.exhaust {
        ExhaustKind::None | ExhaustKind::SidePipes => &[],
        ExhaustKind::Single => &[-0.45],
        ExhaustKind::Dual => &[-0.45, 0.45],
        ExhaustKind::Quad => &[-0.52, -0.40, 0.40, 0.52],
        ExhaustKind::Centre => &[0.0],
    };
    let r = m.exhaust_tip_r_m.clamp(0.025, 0.06);
    let zt = p.z_floor + 0.12;
    for (k, &x) in tips.iter().enumerate() {
        let key = PartKey::body(
            &format!("exhaust_tip_{k}"),
            BodyRegion::FrameRear,
            PartKind::Suspension,
            MaterialSlot::Exhaust,
        )
        .rigid();
        let x = x * p.half_w / 0.9;
        b.cylinder(
            &key,
            [x, p.y_tail + 0.25, zt],
            [x, p.y_tail - 0.02, zt],
            r,
            r,
            10,
            true,
        );
    }
}

/// Brake disc + caliper + upright of one wheel, hub space.
pub fn brake_parts(g: &WheelGeom, m: &MechStyle, w: &WheelStyle) -> Vec<MeshPart> {
    let bs = if g.front {
        &m.front_brakes
    } else {
        &m.rear_brakes
    };
    let n = (w.segments as usize).clamp(8, 48);
    let mut b = Builder::new();
    let o = g.outboard;
    let r = g.rim_radius * bs.disc_radius_frac.clamp(0.5, 0.95);
    let t = bs.disc_thickness_m.clamp(0.008, 0.045);
    // Disc plane slightly inboard of the wheel centre.
    let xc = -o * 0.15 * g.width;
    let disc = PartKey::wheel(
        &g.name("disc"),
        g.index,
        PartKind::BrakeDisc,
        MaterialSlot::Metal,
    );
    if bs.caliper == CaliperKind::Drum {
        b.cylinder(
            &disc,
            [xc - 0.05, 0.0, 0.0],
            [xc + 0.05, 0.0, 0.0],
            r * 0.9,
            r * 0.9,
            n,
            true,
        );
    } else {
        b.cylinder(
            &disc,
            [xc - 0.5 * t, 0.0, 0.0],
            [xc + 0.5 * t, 0.0, 0.0],
            r,
            r,
            n,
            true,
        );
        // Caliper at the rear of the disc, straddling it.
        let cal = PartKey::wheel(
            &g.name("caliper"),
            g.index,
            PartKind::Caliper,
            MaterialSlot::Caliper,
        );
        let h = match bs.caliper {
            CaliperKind::Fixed6 => 0.16,
            CaliperKind::Fixed4 => 0.12,
            _ => 0.09,
        };
        b.beveled_box(
            &cal,
            [xc - 0.5 * t - 0.025, -r - 0.012, -0.5 * h],
            [xc + 0.5 * t + 0.025, -r + 0.05, 0.5 * h],
            0.008,
        );
    }
    // Upright / knuckle inboard of the disc.
    let up = PartKey::wheel(
        &g.name("upright"),
        g.index,
        PartKind::Upright,
        MaterialSlot::Engine,
    );
    let xi = xc - o * (0.5 * t + 0.03);
    b.cuboid(
        &up,
        [xi.min(xi - o * 0.04), -0.04, -0.12],
        [xi.max(xi - o * 0.04), 0.04, 0.12],
    );
    b.finish()
}
