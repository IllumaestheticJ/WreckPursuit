//! Northfield Airfield: plan site `airfield` (x 100..1900, y 3040..3370, pad 40 m).
//!
//! Layout, relative to the fence rectangle `[x0, yf, x1, y1]` (the site with its south
//! edge moved north of Airfield Road, which runs inside the site to the gate):
//! - runway 09/27, 1400 × 30 m east–west at `y1 − 105` (in practice a drag strip), with a
//!   parallel taxiway and three connectors;
//! - a row of seven hangars along the south fence, doors north onto the apron, except
//!   **Hangar 3** (the plan's safehouse "SH13 Hangar 3"), which sits in the fence line with
//!   its door facing south onto Airfield Road, so it is reachable without entering the
//!   airside;
//! - an apron with eight light aircraft, two helipads with helicopters, a small tower, a
//!   fuel stand and a windsock; perimeter fence open only at the plan gate.

use wp_math::Rng;

use super::super::ctx::{box_on, cylinder, prism, Ctx};
use super::super::plan::rect;
use super::*;
use crate::network::{FeatureKind as K, Material as M};

/// Hangar width (east–west), m.
const HANGAR_W_M: f32 = 44.0;
/// Hangar depth, m.
const HANGAR_D_M: f32 = 30.0;
/// Hangar pitch along the row, m.
const HANGAR_PITCH_M: f32 = 50.0;

/// Build Northfield Airfield (no-op if the plan has no `airfield` site).
pub(super) fn build(ctx: &mut Ctx, rng: &mut Rng) {
    let plan = ctx.plan;
    let Some(site) = plan.site("airfield") else {
        return;
    };
    let fr = fence_rect(ctx, site);
    let [x0, yf, _x1, y1] = fr;
    let fr_poly = rect_poly(fr);
    let z = deck_z(ctx, &fr_poly);

    // --- Runway, taxiway, apron ---
    let ry = y1 - 105.0;
    let ty = y1 - 175.0;
    let mut f = Vec::new();
    runway(
        ctx,
        Runway {
            a: [x0 + 150.0, ry],
            b: [x0 + 1550.0, ry],
            width: 30.0,
        },
        z,
        &fr_poly,
        &mut f,
    );
    taxiway(
        [x0 + 180.0, ty],
        [x0 + 1520.0, ty],
        15.0,
        z,
        0.0,
        0.0,
        true,
        &mut f,
    );
    for x in [x0 + 180.0, x0 + 850.0, x0 + 1520.0] {
        taxiway([x, ty], [x, ry], 15.0, z, 7.5, 15.0, false, &mut f);
    }
    let apron = [x0 + 340.0, yf + 40.0, x0 + 1080.0, ty - 7.0];
    f.extend(deck_rect(K::Apron, M::Concrete, apron, z));
    put(ctx, f);

    // --- Hangar row (Hangar 3 = safehouse, door onto Airfield Road) ---
    let h3x = plan
        .pois
        .iter()
        .find(|p| p.name.starts_with("SH13"))
        .map(|p| p.pos[0])
        .unwrap_or(x0 + 500.0);
    let with_plane = {
        let others = [0usize, 1, 3, 4, 5, 6];
        let k = pick(rng, others.len(), 2);
        [others[k[0]], others[k[1]]]
    };
    let mut hangar3 = None;
    for i in 0..7usize {
        let x = h3x + HANGAR_PITCH_M * (i as f32 - 2.0);
        if i == 2 {
            let c = [x, yf + HANGAR_D_M * 0.5];
            let hf = Frame::new(c, core::f32::consts::PI, z);
            put(ctx, hangar(hf, HANGAR_W_M, HANGAR_D_M, 9.0, true));
            hangar3 = Some(rect(
                x - HANGAR_W_M * 0.5,
                yf,
                x + HANGAR_W_M * 0.5,
                yf + HANGAR_D_M,
            ));
        } else {
            let c = [x, yf + 4.0 + HANGAR_D_M * 0.5];
            let hf = Frame::new(c, 0.0, z);
            put(ctx, hangar(hf, HANGAR_W_M, HANGAR_D_M, 9.0, true));
            if with_plane.contains(&i) {
                let liv = livery(rng);
                put(ctx, light_aircraft(Frame::new(c, 0.0, z), liv));
            }
        }
    }

    // --- Light aircraft on the apron, helipads ---
    for k in 0..8 {
        let c = [apron[2] - 10.0 - 30.0 * k as f32, apron[1] + 25.0];
        let liv = livery(rng);
        put(ctx, light_aircraft(Frame::new(c, 0.0, z), liv));
    }
    for k in 0..2 {
        let c = [apron[2] + 40.0 + 40.0 * k as f32, yf + 60.0];
        let mut g = vec![box_on(
            K::Apron,
            M::Concrete,
            c,
            z - DECK_THICK_M,
            [20.0, 20.0, DECK_THICK_M],
            0.0,
            true,
        )];
        g.push(box_on(
            K::AirfieldMarking,
            M::PaintWhite,
            c,
            z,
            [12.0, 12.0, PAINT_THICK_M],
            0.0,
            false,
        ));
        let liv = if k == 0 { M::Red } else { M::Blue };
        g.extend(helicopter(Frame::new(c, 0.0, z), liv));
        put(ctx, g);
    }

    // --- Tower, fuel stand, windsock ---
    let tc = [x0 + 1250.0, yf + 25.0];
    let tz = ctx.ground_z(tc);
    let tf = Frame::new(tc, 0.0, tz);
    put(
        ctx,
        vec![
            prism(
                K::ControlTower,
                M::Concrete,
                rect(tc[0] - 4.0, tc[1] - 4.0, tc[0] + 4.0, tc[1] + 4.0),
                tz - 0.3,
                tz + 4.0,
                true,
            ),
            cylinder(
                K::ControlTower,
                M::Concrete,
                [tc[0], tc[1], tz],
                2.0,
                14.0,
                true,
            ),
            tf.part(
                K::ControlTower,
                M::Glass,
                0.0,
                0.0,
                14.0,
                [7.0, 7.0, 3.0],
                true,
            ),
            tf.part(
                K::ControlTower,
                M::DarkConcrete,
                0.0,
                0.0,
                17.0,
                [8.0, 8.0, 0.4],
                true,
            ),
        ],
    );
    let mut tanks = Vec::new();
    for k in 0..2 {
        let c = [x0 + 1350.0 + 15.0 * k as f32, yf + 20.0];
        tanks.push(cylinder(
            K::Tank,
            M::White,
            [c[0], c[1], ctx.ground_z(c) - 0.2],
            5.0,
            6.0,
            true,
        ));
    }
    put(ctx, tanks);
    let ws = [x0 + 850.0, y1 - 60.0];
    let g = ctx.ground_z(ws);
    put(ctx, windsock(Frame::new(ws, 1.2, g)));

    // --- Perimeter fence and gate ---
    let gates = fence_gates(site, fr);
    let closers: Vec<Vec<V2>> = hangar3.into_iter().collect();
    for piece in fence_loop(ctx, fr, &gates, &closers) {
        ctx.place(piece);
    }
    for (p, h, w) in gates {
        gate_kit(ctx, p, h, w);
    }
}
