//! Wave-3 supercars: `halberd_40` (2020s carbon-tub TT V8), `ombra_v12` (2010s V12 flagship),
//! `ostwind_6t` (2020s rear-engine flat-six) and `kagero_hx` (2010s hybrid V6).
//!
//! Like the wave-2 exotics, the mid-engine lattices carry a front-engine cabin, so the
//! cab-forward look comes from `cabin_shift_m`. The rear-engine Ostwind keeps a classic
//! long-roof fastback (small shift). Mechanical parts come from `mechanical::spec` (table);
//! the styles here only add caliper / spring colours on top of it.
//!
//! Each car has its own body kit and its own night identifier (tail-lamp pattern):
//! - Halberd: one hair-thin full-width LED line over an exposed black rear mesh.
//! - Ombra: a sideways-Y lamp at each corner in an angular black housing.
//! - Ostwind: a full-width red bar through a black band, four square brake dots per side.
//! - Kagero: a thin full-width bar whose ends fold down into a Z at each corner.

// Kit code takes explicit coordinates (x0, x1, z0, z1, …), as in `kit.rs`.
#![allow(clippy::too_many_arguments)]

use wp_contracts::{BodyRegion, VehicleDef};

use crate::builder::{Builder, PartKey};
use crate::mesh::MaterialSlot as M;
use crate::style::{ArchShape, BodyStyle, CapKind, CarStyle, DetailCtx, RimKind, Silhouette};

use super::kit::{
    dkey, headlight_key, indicator_front_key, lkey, sided, taillight_key, top_band, Kit, Stand,
    Wing, SIDES,
};
use super::{kit_detail, rgb, wheels};

/// A straight stroke from `a` to `c` lying on a surface with normal `n`: `width` is the
/// visible width across the stroke, `depth` how far it stands along the normal.
fn stroke(
    k: &mut Kit,
    key: &PartKey,
    a: [f32; 3],
    c: [f32; 3],
    n: [f32; 3],
    width: f32,
    depth: f32,
) {
    k.plate(key, a, c, n, depth, width);
}

/// A stroke on the nose face from (x, z) `a` to `c`, standing `proud` out of the face.
fn front_stroke(k: &mut Kit, key: &PartKey, a: [f32; 2], c: [f32; 2], width: f32, proud: f32) {
    let y = k.yn + 0.5 * (proud - 0.015);
    let depth = proud + 0.015;
    stroke(
        k,
        key,
        [a[0], y, a[1]],
        [c[0], y, c[1]],
        [0.0, 1.0, 0.0],
        width,
        depth,
    );
}

/// A stroke on the tail face from (x, z) `a` to `c`, standing `proud` out of the face.
fn rear_stroke(k: &mut Kit, key: &PartKey, a: [f32; 2], c: [f32; 2], width: f32, proud: f32) {
    let y = k.yt - 0.5 * (proud - 0.015);
    let depth = proud + 0.015;
    stroke(
        k,
        key,
        [a[0], y, a[1]],
        [c[0], y, c[1]],
        [0.0, -1.0, 0.0],
        width,
        depth,
    );
}

/// A stroke on the top surface from (x, y) `a` to `c`, `lift` m above the surface.
fn top_stroke(k: &mut Kit, key: &PartKey, a: [f32; 2], c: [f32; 2], width: f32, lift: f32) {
    let pa = [a[0], a[1], k.top_z(a[1], a[0]) + lift];
    let pc = [c[0], c[1], k.top_z(c[1], c[0]) + lift];
    let n = k.p.top_point(0.5 * (a[1] + c[1]), 0.5 * (a[0] + c[0])).1;
    stroke(k, key, pa, pc, n, width, 0.012);
}

/// Thin vertical door-shut seams (dark) at the front and rear edges of the front doors.
fn door_seams(k: &mut Kit) {
    let p = k.p;
    let key = dkey("door_seam_l", BodyRegion::LeftDoorF, M::Grille);
    for y in [p.y_fd_hinge - 0.004, p.y_fd_rear + 0.004] {
        let (z0, z1) = (p.sill(y) + 0.06, p.belt(y) - 0.012);
        for sx in SIDES {
            k.side_patch(&key, sx, y - 0.006, y + 0.006, z0, z1, 0.002, 0.0, (1, 3));
        }
    }
}

/// Air outlet on the front fender behind the front wheel: dark slot with `slats` blades.
fn fender_outlet(k: &mut Kit, slats: usize, slat_mat: M) {
    let p = k.p;
    let (yf, zf, rf) = k.arch(true);
    let (y0, y1) = (p.y_fd_hinge + 0.04, yf - rf - 0.02);
    if y1 - y0 < 0.08 {
        return;
    }
    let (z0, z1) = (zf + 0.02, (p.belt(y1) - 0.06).max(zf + 0.16));
    for sx in SIDES {
        let kg = dkey("fender_outlet_l", BodyRegion::FrontLeftFender, M::Grille);
        k.side_patch(&kg, sx, y0, y1, z0, z1, 0.002, 0.006, (2, 2));
        let ks = dkey(
            "fender_outlet_slats_l",
            BodyRegion::FrontLeftFender,
            slat_mat,
        );
        for i in 0..slats {
            let z = z0 + (z1 - z0) * (i as f32 + 0.5) / slats as f32;
            k.side_patch(&ks, sx, y0, y1, z - 0.008, z + 0.008, 0.008, 0.010, (2, 1));
        }
    }
}

/// Dark roof sheet (glass canopy / black floating roof) over the roof top, `inset` from the
/// roof edges, `off` above the paint, `mat`.
fn roof_sheet(k: &mut Kit, name: &str, inset: f32, off: f32, mat: M) {
    let (r0, r1, _, _) = k.p.roof;
    let hw = k.roof_edge(0.5 * (r0 + r1)) - inset;
    let key = dkey(name, BodyRegion::Roof, mat);
    k.patch(
        &key,
        r0 + 0.02,
        r1 - 0.02,
        top_band(-hw, hw),
        off,
        0.0,
        (4, 3),
    );
}

// ===================================================================================== Halberd

/// 2020s British carbon-tub supercar: teardrop glass canopy, dihedral doors, deep "eye socket"
/// lamps, door-to-haunch air channel, flat active wing on stands, hair-line tail over mesh.
pub fn halberd(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: 0.42,
        nose_h: Some(0.50),
        nose_rake_deg: 42.0,
        nose_plan_round: 0.24,
        tail_plan_round: 0.16,
        hood_crown: 0.018,
        windshield_rake_deg: Some(66.0),
        a_pillar_rake_deg: 20.0,
        rear_glass_rake_deg: Some(66.0),
        roof_len_trim: 0.12,
        tumblehome: Some(0.30),
        crease_z: 0.44,
        crease_depth: 0.020,
        haunch: 0.060,
        front_haunch: 0.035,
        flare: 0.018,
        ride_dz: -0.01,
        stance_pitch_deg: 0.2,
        ducktail_h: 0.020,
        belt_kick: 0.07,
        belt_rake: 0.03,
        tail_round: 0.12,
        tail_drop: 0.03,
        ..BodyStyle::preset(Silhouette::Wedge)
    });
    s.details.push(kit_detail(halberd_kit));
    wheels(
        &mut s,
        def,
        RimKind::Monoblock,
        10,
        0.78,
        M::Trim,
        CapKind::CentreLock,
        (0.035, 0.012),
        (0.0, 0.5),
    );
    let m = &mut s.mechanical;
    m.caliper_rgb = Some(rgb(0xFF6A13));
    m.spring_rgb = Some(rgb(0x1C1C1E));
    s.tints.push((M::Paint, rgb(0xFF6A13)));
    s
}

fn halberd_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;

    // --- Deep "eye sockets": a black socket on the nose corner, a slim LED eyebrow along its
    //     top edge with a short outer drop, and a vertical intake slot inside the socket.
    for sx in SIDES {
        let (xi, xo) = (0.46, wn - 0.05);
        let ks = sided(
            &dkey("eye_socket_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        k.front_box(&ks, sx * xi, sx * xo, nz1 - 0.19, nz1 - 0.004, 0.010, 0.004);
        k.patch(
            &ks,
            yn - 0.16,
            yn - 0.005,
            top_band(sx * xi, sx * xo),
            0.002,
            0.006,
            (2, 1),
        );
        let hk = headlight_key(sx);
        front_stroke(
            &mut k,
            &hk,
            [sx * (xi + 0.02), nz1 - 0.028],
            [sx * (xo - 0.03), nz1 - 0.022],
            0.020,
            0.022,
        );
        front_stroke(
            &mut k,
            &hk,
            [sx * (xo - 0.03), nz1 - 0.022],
            [sx * (xo - 0.06), nz1 - 0.12],
            0.018,
            0.022,
        );
        // Lens behind the eyebrow on the nose top.
        k.patch(
            &hk,
            yn - 0.12,
            yn - 0.03,
            top_band(sx * (xi + 0.10), sx * (xo - 0.06)),
            0.004,
            0.008,
            (1, 1),
        );
        let kv = sided(
            &dkey("socket_intake_l", BodyRegion::FrontLeftFender, M::Grille),
            sx,
        );
        k.front_box(
            &kv,
            sx * (xi + 0.05),
            sx * (xi + 0.17),
            nz1 - 0.175,
            nz1 - 0.05,
            0.016,
            0.0,
        );
        let kb = sided(
            &dkey(
                "socket_intake_vane_l",
                BodyRegion::FrontLeftFender,
                M::Carbon,
            ),
            sx,
        );
        k.front_box(
            &kb,
            sx * (xi + 0.105),
            sx * (xi + 0.115),
            nz1 - 0.175,
            nz1 - 0.05,
            0.022,
            0.0,
        );
    }
    // --- Wide low centre intake, carbon splitter with fences, hood duct.
    k.mesh_grille(
        "front_intake",
        fb,
        0.40,
        nz0 + 0.05,
        nz0 + 0.15,
        2,
        6,
        M::Trim,
    );
    k.splitter(M::Carbon, 0.09, 0.05, 0.022, true);
    k.top_vent(
        "hood_duct",
        BodyRegion::Hood,
        yn - 0.52,
        yn - 0.30,
        -0.20,
        0.20,
        2,
        M::Paint,
    );

    // --- Glass canopy: dark roof sheet, dihedral door cut lines in the roof, door seams.
    roof_sheet(&mut k, "canopy_roof_base", 0.03, 0.004, M::Trim);
    roof_sheet(&mut k, "canopy_roof", 0.05, 0.010, M::Glass);
    let (r0, r1, _, _) = p.roof;
    let ws = dkey("door_roof_seam_l", BodyRegion::Roof, M::Grille);
    for sx in SIDES {
        let xe = k.roof_edge(0.5 * (r0 + r1)) - 0.10;
        let ws = sided(&ws, sx);
        k.patch(
            &ws,
            r0 + 0.04,
            r1 - 0.02,
            top_band(sx * xe, sx * (xe + 0.012)),
            0.007,
            0.0,
            (3, 1),
        );
    }
    door_seams(&mut k);

    // --- Air channel scooped into the door, feeding a big intake on the rear haunch whose
    //     front edge steps back toward the sill (teardrop read), carbon lip under it.
    let (yr, zr, rr) = k.arch(false);
    let ya = yr + rr + 0.02;
    let zc = p.belt(p.y_fd_rear) - 0.21;
    for sx in SIDES {
        let kc = dkey("door_channel_l", BodyRegion::LeftDoorF, M::Grille);
        k.side_patch(
            &kc,
            sx,
            p.y_fd_rear + 0.01,
            p.y_fd_hinge - 0.28,
            zc,
            zc + 0.08,
            0.002,
            0.004,
            (5, 1),
        );
        let ki = dkey("haunch_intake_l", BodyRegion::RearLeftQuarter, M::Grille);
        let bands = [
            (zc - 0.02, zc + 0.13, p.y_fd_rear - 0.01),
            (zr + 0.13, zc - 0.02, p.y_fd_rear - 0.08),
            (zr + 0.03, zr + 0.13, p.y_fd_rear - 0.18),
        ];
        for (z0, z1, y1) in bands {
            k.side_patch(&ki, sx, ya, y1, z0, z1, 0.002, 0.008, (2, 1));
        }
        let kw = dkey(
            "haunch_intake_lip_l",
            BodyRegion::RearLeftQuarter,
            M::Carbon,
        );
        k.side_patch(
            &kw,
            sx,
            ya,
            p.y_fd_rear - 0.18,
            zr + 0.005,
            zr + 0.03,
            0.010,
            0.014,
            (2, 1),
        );
        // Slanted carbon blade along the stepped front edge of the intake.
        let ke = sided(
            &dkey(
                "haunch_intake_edge_l",
                BodyRegion::RearLeftQuarter,
                M::Carbon,
            ),
            sx,
        );
        let (ya0, za0) = (p.y_fd_rear - 0.02, zc + 0.12);
        let (ya1, za1) = (p.y_fd_rear - 0.19, zr + 0.04);
        let (pa, _) = p.side_point(ya0, za0);
        let (pc, _) = p.side_point(ya1, za1);
        k.plate(
            &ke,
            [sx * (-pa[0] + 0.012), ya0, za0],
            [sx * (-pc[0] + 0.012), ya1, za1],
            [sx, 0.0, 0.0],
            0.03,
            0.05,
        );
    }
    k.skirts(M::Carbon, 0.035, 0.05);
    fender_outlet(&mut k, 2, M::Carbon);
    for sx in SIDES {
        let name = if sx < 0.0 {
            "engine_vent_l"
        } else {
            "engine_vent_r"
        };
        let (a, c) = (sx * 0.16, sx * 0.46);
        let (d0, d1) = (yr - rr + 0.20, p.y_deck - 0.10);
        k.top_vent(
            name,
            BodyRegion::Trunk,
            d0,
            d1,
            a.min(c),
            a.max(c),
            5,
            M::Carbon,
        );
    }

    // --- Flying-buttress hint: a carbon fin along each rear-glass edge, from the roof corner
    //     down onto the deck.
    let yb = p.y_deck - 0.12;
    for sx in SIDES {
        let kf = sided(
            &dkey("buttress_l", BodyRegion::RearLeftQuarter, M::Carbon),
            sx,
        );
        let xa = k.roof_edge(r0) + 0.01;
        let xb = p.half_width(yb) - 0.24;
        let a = [sx * xa, r0 - 0.03, k.top_z(r0 - 0.03, sx * xa)];
        let c = [sx * xb, yb, k.top_z(yb, sx * xb) + 0.01];
        stroke(&mut k, &kf, a, c, [sx * 0.6, 0.0, 0.8], 0.07, 0.03);
    }

    // --- Flat full-width active wing raised on carbon stands from the deck.
    k.wing(Wing {
        lead: 0.34,
        chord: 0.24,
        height: 0.17,
        half_span: 0.90,
        thick: 0.024,
        aoa_deg: 2.0,
        stand: Stand::Pedestal(0.30),
        endplates: None,
        mat: M::Paint,
        stand_mat: M::Carbon,
        gurney: 0.0,
    });

    // --- Rear: hair-line LED tail over an exposed black mesh, high exhaust surrounds.
    let zt = tz1 - 0.035;
    let kb = lkey("taillight_bar", BodyRegion::Trunk, M::Taillight);
    k.rear_box(&kb, -0.30, 0.30, zt - 0.007, zt + 0.007, 0.022, 0.0);
    for sx in SIDES {
        let tk = taillight_key(sx);
        k.rear_box(
            &tk,
            sx * 0.30,
            sx * (wt - 0.02),
            zt - 0.007,
            zt + 0.007,
            0.022,
            0.0,
        );
    }
    let (mz0, mz1) = (tz0 + 0.17, zt - 0.025);
    k.rear_panel(
        "rear_mesh_panel",
        BodyRegion::RearBumper,
        wt - 0.05,
        mz0,
        mz1,
        M::Grille,
    );
    let km = dkey("rear_mesh", BodyRegion::RearBumper, M::Trim);
    for i in 1..4 {
        let z = mz0 + (mz1 - mz0) * i as f32 / 4.0;
        k.rear_box(&km, -wt + 0.07, wt - 0.07, z - 0.004, z + 0.004, 0.014, 0.0);
    }
    for i in 0..10 {
        let x = -wt + 0.12 + (2.0 * wt - 0.24) * i as f32 / 9.0;
        k.rear_box(&km, x - 0.004, x + 0.004, mz0, mz1, 0.014, 0.0);
    }
    let ze = (p.z_bumper_top.1 - 0.12).max(p.z_floor + 0.26);
    let kx = dkey("exhaust_surround", BodyRegion::RearBumper, M::Carbon);
    for sx in SIDES {
        k.rear_box(
            &kx,
            sx * 0.005,
            sx * 0.25,
            ze - 0.065,
            ze + 0.065,
            0.02,
            0.01,
        );
    }
    k.diffuser(M::Carbon, 0.72, 0.17, 7, 0.10);

    k.mirrors(0.16, 0.10, Some(M::Carbon));
    k.wipers();

    // Turn signals as segments beside the lamps (playtest row 136).
    k.signals_beside_lamps(0.06, M::EmissiveAmber);
}

// ======================================================================================= Ombra

/// 2010s Italian V12 flagship: very low and wide, Y / hexagon motifs, trapezoid intakes,
/// hood spine with ducts, giant angular side intakes + shoulder air ears, glass-louvred engine
/// cover, Y-shaped tails, quad centre exhaust in a hexagonal surround.
pub fn ombra(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: 0.45,
        nose_h: Some(0.48),
        nose_rake_deg: 50.0,
        nose_plan_round: 0.12,
        tail_plan_round: 0.10,
        hood_crown: 0.012,
        windshield_rake_deg: Some(67.0),
        a_pillar_rake_deg: 16.0,
        rear_glass_rake_deg: Some(68.0),
        roof_len_trim: 0.16,
        tumblehome: Some(0.30),
        crease_z: 0.46,
        crease_depth: 0.030,
        haunch: 0.065,
        front_haunch: 0.030,
        flare: 0.020,
        flare_shape: ArchShape::Box,
        ride_dz: -0.015,
        stance_pitch_deg: 0.3,
        ducktail_h: 0.010,
        belt_kick: 0.08,
        belt_rake: 0.035,
        tail_round: 0.06,
        tail_drop: 0.02,
        ..BodyStyle::preset(Silhouette::Wedge)
    });
    s.details.push(kit_detail(ombra_kit));
    wheels(
        &mut s,
        def,
        RimKind::SplitSpoke,
        5,
        0.79,
        M::Trim,
        CapKind::CentreLock,
        (0.03, 0.012),
        (0.0, 0.5),
    );
    let m = &mut s.mechanical;
    m.caliper_rgb = Some(rgb(0xE87A10));
    m.spring_rgb = Some(rgb(0xE87A10));
    s.tints.push((M::Paint, rgb(0x2B2D42)));
    s
}

fn ombra_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;
    let hood = BodyRegion::Hood;

    // --- Angular lamps on the nose top with a two-stroke chevron DRL.
    for sx in SIDES {
        let (y0, y1) = (yn - 0.30, yn - 0.05);
        let (xi, xo) = (0.52, wn - 0.06);
        let kh = sided(
            &dkey("headlamp_housing_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        k.patch(
            &kh,
            y0,
            y1,
            top_band(sx * (xi - 0.02), sx * xo),
            0.002,
            0.007,
            (3, 2),
        );
        let hk = headlight_key(sx);
        k.patch(
            &hk,
            y0 + 0.03,
            y0 + 0.09,
            top_band(sx * (xi + 0.24), sx * (xi + 0.34)),
            0.004,
            0.010,
            (1, 1),
        );
        // Amber turn-signal segment of the same lens, outboard of the lamp.
        let ik = indicator_front_key(sx);
        k.patch(
            &ik,
            y0 + 0.03,
            y0 + 0.09,
            top_band(sx * (xi + 0.35), sx * (xi + 0.41)),
            0.004,
            0.010,
            (1, 1),
        );
        let kd = sided(
            &lkey("drl_l", BodyRegion::FrontLeftFender, M::Headlight),
            sx,
        );
        let j = [sx * (xi + 0.20), 0.5 * (y0 + y1)];
        top_stroke(&mut k, &kd, [sx * xi, j[1] - 0.02], j, 0.018, 0.016);
        top_stroke(&mut k, &kd, j, [sx * (xo - 0.04), y0 + 0.01], 0.018, 0.016);
    }
    // --- Huge trapezoid intakes with a diamond ("hex") mesh, narrow centre slot, splitter.
    let (zi0, zi1) = (nz0 + 0.03, nz1 - 0.07);
    for sx in SIDES {
        let (xi, xo) = (0.36, wn - 0.04);
        let kg = sided(&dkey("front_intake_l", fb, M::Grille), sx);
        k.front_box(&kg, sx * xi, sx * xo, zi0, zi1, 0.006, 0.0);
        // Trapezoid: slanted carbon inner edge, paint brow across the top.
        let ke = sided(&dkey("front_intake_edge_l", fb, M::Carbon), sx);
        front_stroke(
            &mut k,
            &ke,
            [sx * (xi + 0.10), zi1],
            [sx * xi, zi0],
            0.03,
            0.02,
        );
        let kw = sided(&dkey("front_intake_brow_l", fb, M::Paint), sx);
        front_stroke(
            &mut k,
            &kw,
            [sx * (xi + 0.10), zi1],
            [sx * xo, zi1 - 0.04],
            0.03,
            0.02,
        );
        // Diamond mesh: crossing diagonals.
        let km = sided(&dkey("front_intake_mesh_l", fb, M::Trim), sx);
        let n = 4;
        let span = xo - xi - 0.14;
        for i in 0..n {
            let x = xi + 0.12 + span * i as f32 / (n - 1) as f32;
            let d = 0.5 * (zi1 - zi0 - 0.06);
            front_stroke(
                &mut k,
                &km,
                [sx * (x - d), zi0 + 0.02],
                [sx * x, zi1 - 0.05],
                0.008,
                0.012,
            );
            front_stroke(
                &mut k,
                &km,
                [sx * x, zi0 + 0.02],
                [sx * (x - d), zi1 - 0.05],
                0.008,
                0.012,
            );
        }
    }
    k.mesh_grille(
        "centre_slot",
        fb,
        0.26,
        nz0 + 0.03,
        nz0 + 0.10,
        1,
        6,
        M::Trim,
    );
    k.splitter(M::Carbon, 0.10, 0.04, 0.022, true);

    // --- Hood spine with a flush duct either side.
    let ks = dkey("hood_spine", hood, M::Paint);
    let ys = [yn - 0.08, yn - 0.40, p.y_fw + 0.35, p.y_fw + 0.04];
    for w in ys.windows(2) {
        top_stroke(&mut k, &ks, [0.0, w[0]], [0.0, w[1]], 0.05, 0.010);
    }
    for sx in SIDES {
        let name = if sx < 0.0 {
            "flush_duct_l"
        } else {
            "flush_duct_r"
        };
        let ya = p.y_fw + 0.12;
        let yb = p.y_fw + 0.40;
        let (a0, a1) = (sx * 0.10, sx * 0.26);
        k.top_vent(
            name,
            hood,
            ya,
            yb - 0.12,
            a0.min(a1),
            a0.max(a1),
            0,
            M::Trim,
        );
        let (c0, c1) = (sx * 0.15, sx * 0.21);
        k.top_vent(
            name,
            hood,
            yb - 0.12,
            yb,
            c0.min(c1),
            c0.max(c1),
            0,
            M::Trim,
        );
    }

    // --- Scissor-door seams, giant angular side intake, shoulder air "ears".
    door_seams(&mut k);
    let (yr, zr, rr) = k.arch(false);
    let (y0, y1) = (yr + rr + 0.03, p.y_fd_rear - 0.01);
    let (z0, z1) = (zr - 0.06, p.belt(y1) - 0.03);
    for sx in SIDES {
        let ki = dkey("side_intake_l", BodyRegion::RearLeftQuarter, M::Grille);
        k.side_patch(&ki, sx, y0, y1, z0, z1, 0.002, 0.008, (3, 3));
        // Angular front wall leaning forward, and a diagonal blade across the mouth.
        let kw = sided(
            &dkey("side_intake_wall_l", BodyRegion::RearLeftQuarter, M::Paint),
            sx,
        );
        let (pa, _) = p.side_point(y1, z0);
        let (pb, _) = p.side_point(y1 + 0.12, z1);
        let xa = sx * (-pa[0] + 0.03);
        let xb = sx * (-pb[0] + 0.03);
        k.plate(
            &kw,
            [xa, y1, z0],
            [xb, y1 + 0.12, z1],
            [sx, 0.0, 0.0],
            0.06,
            0.02,
        );
        let kb = sided(
            &dkey(
                "side_intake_blade_l",
                BodyRegion::RearLeftQuarter,
                M::Carbon,
            ),
            sx,
        );
        let (pc, _) = p.side_point(y0 + 0.05, z1 - 0.04);
        let (pd, _) = p.side_point(y1 - 0.02, z0 + 0.06);
        k.plate(
            &kb,
            [sx * (-pc[0] + 0.02), y0 + 0.05, z1 - 0.04],
            [sx * (-pd[0] + 0.02), y1 - 0.02, z0 + 0.06],
            [sx, 0.0, 0.0],
            0.04,
            0.022,
        );
        // Air ear on the shoulder behind the side glass: a wedge with a dark mouth.
        let ke = sided(
            &dkey("air_ear_l", BodyRegion::RearLeftQuarter, M::Paint),
            sx,
        );
        let (ye0, ye1) = (p.y_deck - 0.40, p.y_deck + 0.02);
        let xo = p.half_width(ye1) - 0.10;
        let xi = xo - 0.20;
        let zb = k.top_z(ye1, sx * (xo - 0.10)) - 0.02;
        let prof = [
            [sx * xi, zb],
            [sx * xo, zb],
            [sx * xo, zb + 0.09],
            [sx * (xi + 0.03), zb + 0.11],
        ];
        k.prism_y(&ke, &prof, ye0, ye1);
        let km = sided(
            &dkey("air_ear_mouth_l", BodyRegion::RearLeftQuarter, M::Grille),
            sx,
        );
        k.bx(
            &km,
            [sx * (xi + 0.03), ye1 - 0.01, zb + 0.03],
            [sx * (xo - 0.015), ye1 + 0.004, zb + 0.095],
        );
    }
    k.skirts(M::Carbon, 0.04, 0.06);
    fender_outlet(&mut k, 3, M::Paint);

    // --- Engine cover: glass panel over the V12 with dark louvres across it, side vents.
    let (le0, le1) = (yr - rr + 0.10, p.y_deck - 0.08);
    let kg = dkey("engine_cover_glass", BodyRegion::Trunk, M::Glass);
    k.patch(&kg, le0, le1, top_band(-0.32, 0.32), 0.004, 0.006, (4, 2));
    let kl = dkey("engine_louvres", BodyRegion::Trunk, M::Trim);
    for i in 0..6 {
        let y = le0 + (le1 - le0) * (i as f32 + 0.5) / 6.0;
        let z = k.top_z(y, 0.0).max(k.top_z(y, 0.3)) + 0.008;
        let prof = [
            [y + 0.02, z],
            [y - 0.02, z],
            [y - 0.03, z + 0.018],
            [y + 0.0, z + 0.024],
        ];
        k.prism_x(&kl, &prof, -0.30, 0.30);
    }
    for sx in SIDES {
        let name = if sx < 0.0 {
            "deck_vent_l"
        } else {
            "deck_vent_r"
        };
        let (a, c) = (sx * 0.40, sx * 0.62);
        k.top_vent(
            name,
            BodyRegion::Trunk,
            le0 + 0.10,
            le1 - 0.05,
            a.min(c),
            a.max(c),
            3,
            M::Trim,
        );
    }

    for sx in SIDES {
        let name = if sx < 0.0 {
            "haunch_louvre_l"
        } else {
            "haunch_louvre_r"
        };
        let xo = p.half_width(yr) - 0.10;
        let (a, c) = (sx * (xo - 0.20), sx * xo);
        k.top_vent(
            name,
            BodyRegion::Trunk,
            yr - 0.12,
            yr + 0.20,
            a.min(c),
            a.max(c),
            3,
            M::Trim,
        );
    }
    let kbl = lkey("brake_light_high", BodyRegion::Trunk, M::Taillight);
    let ybl = le0 - 0.06;
    let zbl = k.top_z(ybl, 0.0);
    k.bx(
        &kbl,
        [-0.14, ybl - 0.015, zbl - 0.002],
        [0.14, ybl + 0.015, zbl + 0.010],
    );

    // --- Deck-mounted carbon wing.
    k.wing(Wing {
        lead: 0.30,
        chord: 0.22,
        height: 0.07,
        half_span: 0.84,
        thick: 0.028,
        aoa_deg: 9.0,
        stand: Stand::Pedestal(0.40),
        endplates: Some((0.08, 0.22)),
        mat: M::Carbon,
        stand_mat: M::Carbon,
        gurney: 0.012,
    });

    // --- Rear: Y-shaped tails in angular black housings, mesh fascia, hex exhaust surround.
    let zt = tz1 - 0.07;
    for sx in SIDES {
        let kh = sided(
            &dkey("taillight_housing_l", BodyRegion::RearLeftQuarter, M::Trim),
            sx,
        );
        k.rear_box(
            &kh,
            sx * 0.38,
            sx * (wt - 0.01),
            zt - 0.08,
            zt + 0.065,
            0.010,
            0.004,
        );
        let tk = taillight_key(sx);
        let j = [sx * 0.62, zt];
        rear_stroke(&mut k, &tk, [sx * 0.40, zt], j, 0.022, 0.026);
        rear_stroke(&mut k, &tk, j, [sx * (wt - 0.03), zt + 0.05], 0.022, 0.026);
        rear_stroke(&mut k, &tk, j, [sx * (wt - 0.03), zt - 0.06], 0.022, 0.026);
    }
    let (fz0, fz1) = (tz0 + 0.12, zt - 0.10);
    k.rear_panel(
        "rear_fascia",
        BodyRegion::RearBumper,
        wt - 0.06,
        fz0,
        fz1,
        M::Grille,
    );
    let km = dkey("rear_fascia_mesh", BodyRegion::RearBumper, M::Trim);
    for i in 0..8 {
        let x = -wt + 0.20 + (2.0 * wt - 0.40) * i as f32 / 7.0;
        rear_stroke(
            &mut k,
            &km,
            [x - 0.04, fz0 + 0.01],
            [x + 0.04, fz1 - 0.01],
            0.008,
            0.014,
        );
    }
    let ze = (p.z_floor + 0.14).max(0.24);
    let kx = dkey("exhaust_hex", BodyRegion::RearBumper, M::Trim);
    let (hw, hh) = (0.27, 0.085);
    let hex = [
        [-hw, ze],
        [-hw + 0.06, ze - hh],
        [hw - 0.06, ze - hh],
        [hw, ze],
        [hw - 0.06, ze + hh],
        [-hw + 0.06, ze + hh],
    ];
    k.prism_y(&kx, &hex, k.yt + 0.03, k.yt - 0.022);
    k.diffuser(M::Carbon, 0.80, 0.15, 5, 0.12);

    k.mirrors(0.18, 0.09, Some(M::Trim));
    k.wipers();

    // Turn signals as segments beside the lamps (playtest row 136).
    k.signals_beside_lamps(0.06, M::EmissiveAmber);
}

// ===================================================================================== Ostwind

/// 2020s German rear-engine flagship: round quad-LED lamps on high front fenders, front trunk
/// lid, slatted low intakes, small haunch intakes, longitudinal engine grille in the deck lid,
/// raised lip spoiler, full-width light bar, twin rectangular tips in the valance.
pub fn ostwind(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: 0.12,
        nose_h: Some(0.60),
        nose_rake_deg: 38.0,
        nose_plan_round: 0.30,
        tail_plan_round: 0.22,
        hood_crown: 0.030,
        windshield_rake_deg: Some(62.0),
        a_pillar_rake_deg: 18.0,
        rear_glass_rake_deg: Some(70.0),
        roof_len_trim: 0.0,
        tumblehome: Some(0.24),
        crease_z: 0.50,
        crease_depth: 0.010,
        haunch: 0.070,
        front_haunch: 0.055,
        flare: 0.012,
        ride_dz: -0.005,
        stance_pitch_deg: 0.1,
        ducktail_h: 0.025,
        belt_kick: 0.04,
        belt_rake: 0.02,
        tail_round: 0.14,
        tail_drop: 0.04,
        ..BodyStyle::preset(Silhouette::Fastback)
    });
    s.details.push(kit_detail(ostwind_kit));
    wheels(
        &mut s,
        def,
        RimKind::MultiSpoke,
        10,
        0.80,
        M::Metal,
        CapKind::CentreLock,
        (0.025, 0.012),
        (0.0, 0.5),
    );
    let m = &mut s.mechanical;
    m.caliper_rgb = Some(rgb(0xF0C419));
    s.tints.push((M::Paint, rgb(0x8A9BA8)));
    s
}

fn ostwind_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;
    let hood = BodyRegion::Hood;

    // --- Round lamps on the fender tops: chrome bezel, dark bowl, four LED dots.
    // Two overlapping discs along y give the oval; the LED dots sit in the front one.
    let yc = yn - 0.26;
    let xc = wn - 0.17;
    for sx in SIDES {
        let kb = sided(
            &dkey("headlamp_bezel_l", BodyRegion::FrontLeftFender, M::Chrome),
            sx,
        );
        let kd = sided(
            &dkey("headlamp_bowl_l", BodyRegion::FrontLeftFender, M::Grille),
            sx,
        );
        for dy in [0.0, -0.07] {
            let (q, n) = p.top_point(yc + dy, sx * xc);
            let along = |d: f32| [q[0] + n[0] * d, q[1] + n[1] * d, q[2] + n[2] * d];
            k.b.cylinder(&kb, along(-0.01), along(0.018), 0.125, 0.125, 20, true);
            k.b.cylinder(&kd, along(0.0), along(0.022), 0.108, 0.108, 20, true);
        }
        let (pt, n) = p.top_point(yc - 0.035, sx * xc);
        let hk = headlight_key(sx);
        for (dx, dy) in [
            (-0.045, -0.05),
            (0.045, -0.05),
            (-0.045, 0.05),
            (0.045, 0.05),
        ] {
            let c = [pt[0] + dx, pt[1] + dy, k.top_z(pt[1] + dy, pt[0] + dx)];
            let c0 = [
                c[0] + n[0] * 0.012,
                c[1] + n[1] * 0.012,
                c[2] + n[2] * 0.012,
            ];
            let c1 = [
                c[0] + n[0] * 0.032,
                c[1] + n[1] * 0.032,
                c[2] + n[2] * 0.032,
            ];
            k.b.cylinder(&hk, c0, c1, 0.03, 0.03, 10, true);
        }
    }
    // --- Front trunk lid shut lines.
    let kt = dkey("frunk_seam", hood, M::Grille);
    let xs = wn - 0.32;
    let (ya, yb) = (p.y_fw + 0.10, yn - 0.10);
    for sx in SIDES {
        let (a, c) = (sx * xs, sx * (xs + 0.012));
        k.patch(&kt, ya, yb, top_band(a, c), 0.004, 0.0, (6, 1));
    }
    for y in [ya, yb] {
        k.patch(
            &kt,
            y - 0.006,
            y + 0.006,
            top_band(-xs, xs),
            0.004,
            0.0,
            (1, 4),
        );
    }

    // --- Low wide intakes with horizontal active-flap slats; slim DRL strip over each side.
    let (zi0, zi1) = (nz0 + 0.04, nz0 + 0.17);
    k.grille("front_intake", fb, 0.34, zi0, zi1 - 0.02, 3, M::Trim, None);
    for sx in SIDES {
        let (xi, xo) = (0.42, wn - 0.08);
        let kg = sided(&dkey("side_intake_front_l", fb, M::Grille), sx);
        k.front_box(&kg, sx * xi, sx * xo, zi0, zi1, 0.006, 0.0);
        let ks = sided(&dkey("side_intake_front_slats_l", fb, M::Trim), sx);
        for i in 1..4 {
            let z = zi0 + (zi1 - zi0) * i as f32 / 4.0;
            k.front_box(
                &ks,
                sx * (xi + 0.02),
                sx * (xo - 0.02),
                z - 0.008,
                z + 0.008,
                0.014,
                0.0,
            );
        }
        let kd = sided(&lkey("drl_l", fb, M::Headlight), sx);
        k.front_box(
            &kd,
            sx * (xi + 0.02),
            sx * (xo - 0.02),
            zi1 + 0.02,
            zi1 + 0.034,
            0.012,
            0.0,
        );
    }
    k.splitter(M::Trim, 0.03, 0.10, 0.016, false);
    // Blank number plate in a dark frame above the centre intake.
    let (pz0, pz1) = (zi1 + 0.02, (zi1 + 0.13).min(nz1 - 0.02));
    let kpf = dkey("number_plate_front_frame", fb, M::Trim);
    k.front_box(&kpf, -0.25, 0.25, pz0, pz1, 0.008, 0.003);
    let kp = dkey("number_plate_front", fb, M::Metal);
    k.front_box(&kp, -0.235, 0.235, pz0 + 0.012, pz1 - 0.012, 0.012, 0.0);
    // Vertical air curtains at the bumper corners, amber side markers.
    for sx in SIDES {
        let kc = sided(&dkey("air_curtain_l", fb, M::Grille), sx);
        k.front_box(
            &kc,
            sx * (wn - 0.06),
            sx * (wn - 0.02),
            zi0,
            zi1 + 0.06,
            0.008,
            0.0,
        );
        let km = sided(&lkey("side_marker_l", fb, M::EmissiveAmber), sx);
        k.front_box(
            &km,
            sx * (wn - 0.05),
            sx * (wn - 0.03),
            zi1 + 0.08,
            zi1 + 0.11,
            0.010,
            0.0,
        );
    }

    // --- Small intakes on the rear haunches just behind the doors.
    let (yr, _, rr) = k.arch(false);
    for sx in SIDES {
        let ki = dkey("haunch_intake_l", BodyRegion::RearLeftQuarter, M::Grille);
        let (y0, y1) = (yr + rr - 0.04, p.y_fd_rear - 0.03);
        let z1 = p.belt(y1) - 0.06;
        k.side_patch(&ki, sx, y0, y1, z1 - 0.10, z1, 0.002, 0.006, (2, 1));
        let ks = dkey(
            "haunch_intake_slat_l",
            BodyRegion::RearLeftQuarter,
            M::Paint,
        );
        k.side_patch(
            &ks,
            sx,
            y0,
            y1,
            z1 - 0.058,
            z1 - 0.042,
            0.007,
            0.010,
            (2, 1),
        );
    }
    door_seams(&mut k);
    k.skirts(M::Paint, 0.015, 0.05);
    // Chrome window line along the belt from the A-pillar to the quarter window.
    let kw = dkey("window_line_l", BodyRegion::LeftDoorF, M::Chrome);
    let (w0, w1) = (p.y_fd_rear - 0.30, p.y_fw - 0.10);
    let zw = p.belt(0.5 * (w0 + w1));
    for sx in SIDES {
        k.side_patch(
            &kw,
            sx,
            w0,
            w1,
            zw - 0.012,
            zw + 0.002,
            0.003,
            0.004,
            (8, 1),
        );
    }

    // --- Engine grille in the deck lid: dark base, longitudinal fins, centre brake light.
    let (g0, g1) = (yr - rr + 0.20, p.y_deck - 0.06);
    let gw = 0.40;
    let kgb = dkey("engine_grille", BodyRegion::Trunk, M::Grille);
    k.patch(&kgb, g0, g1, top_band(-gw, gw), 0.003, 0.005, (3, 2));
    let kf = dkey("engine_grille_fins", BodyRegion::Trunk, M::Trim);
    for i in 0..9 {
        let x = -gw + 0.04 + (2.0 * gw - 0.08) * i as f32 / 8.0;
        let za = k.top_z(g0 + 0.02, x) + 0.012;
        let zb = k.top_z(g1 - 0.02, x) + 0.012;
        k.plate(
            &kf,
            [x, g0 + 0.02, za],
            [x, g1 - 0.02, zb],
            [0.0, 0.0, 1.0],
            0.016,
            0.014,
        );
    }
    let kbl = lkey("brake_light_high", BodyRegion::Trunk, M::Taillight);
    let zbl = k.top_z(g1 + 0.03, 0.0) + 0.006;
    k.bx(
        &kbl,
        [-0.16, g1 + 0.015, zbl - 0.004],
        [0.16, g1 + 0.045, zbl + 0.008],
    );

    // --- Extending spoiler: body-colour blade raised on two short stands.
    k.wing(Wing {
        lead: 0.26,
        chord: 0.20,
        height: 0.05,
        half_span: 0.62,
        thick: 0.024,
        aoa_deg: 12.0,
        stand: Stand::Pedestal(0.34),
        endplates: None,
        mat: M::Paint,
        stand_mat: M::Trim,
        gurney: 0.008,
    });

    // --- Tail: black band with a full-width red bar, four brake dots per side.
    let zt = tz1 - 0.06;
    let kh = dkey("tail_band", BodyRegion::Trunk, M::Trim);
    k.rear_box(
        &kh,
        -wt + 0.02,
        wt - 0.02,
        zt - 0.045,
        zt + 0.03,
        0.010,
        0.004,
    );
    let kb = lkey("taillight_bar", BodyRegion::Trunk, M::Taillight);
    k.rear_box(
        &kb,
        -wt + 0.06,
        wt - 0.06,
        zt - 0.006,
        zt + 0.008,
        0.02,
        0.0,
    );
    for sx in SIDES {
        let tk = taillight_key(sx);
        for i in 0..4 {
            let x = wt - 0.08 - 0.065 * i as f32;
            k.rear_box(
                &tk,
                sx * (x - 0.022),
                sx * (x + 0.022),
                zt - 0.038,
                zt - 0.012,
                0.024,
                0.0,
            );
        }
    }
    // --- Valance with twin rectangular tips (round pipes inside chrome rectangles).
    let ze = (p.z_floor + 0.14).max(0.24);
    k.diffuser(M::Trim, 0.60, 0.12, 4, 0.03);
    let kc = dkey("exhaust_frame", BodyRegion::RearBumper, M::Chrome);
    let kd = dkey("exhaust_frame_inner", BodyRegion::RearBumper, M::Grille);
    for sx in SIDES {
        k.rear_box(&kc, sx * 0.20, sx * 0.40, ze - 0.05, ze + 0.05, 0.02, 0.006);
        k.rear_box(
            &kd,
            sx * 0.215,
            sx * 0.385,
            ze - 0.037,
            ze + 0.037,
            0.024,
            0.0,
        );
    }
    // Blank rear number plate between the light bar and the valance.
    let (pz0, pz1) = (ze + 0.09, (ze + 0.20).min(zt - 0.07));
    let kpf = dkey("number_plate_rear_frame", BodyRegion::Trunk, M::Trim);
    k.rear_box(&kpf, -0.25, 0.25, pz0, pz1, 0.008, 0.003);
    let kp = dkey("number_plate_rear", BodyRegion::Trunk, M::Metal);
    k.rear_box(&kp, -0.235, 0.235, pz0 + 0.012, pz1 - 0.012, 0.012, 0.0);
    // Red reflectors at the valance corners.
    let kr = dkey("rear_reflector_l", BodyRegion::RearBumper, M::Taillight);
    for sx in SIDES {
        let kr = sided(&kr, sx);
        k.rear_box(
            &kr,
            sx * (wt - 0.16),
            sx * (wt - 0.05),
            tz0 + 0.07,
            tz0 + 0.085,
            0.012,
            0.0,
        );
    }

    k.mirrors(0.10, 0.11, None);
    k.door_handles(M::Paint);
    k.fuel_filler(1.0, 0.10, M::Paint);
    k.wipers();

    // Turn signals as segments beside the lamps (playtest row 136).
    k.signals_beside_lamps(0.06, M::EmissiveAmber);
}

// ====================================================================================== Kagero

/// 2010s Japanese hybrid supercar: boomerang LED lamps, black floating roof + A-pillars,
/// flying buttresses with a C-pillar air channel, folded door intakes, Z-shaped full-width
/// tails over a black mesh, quad exhaust in a carbon tray, blue hybrid accent strips.
pub fn kagero(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: 0.40,
        nose_h: Some(0.54),
        nose_rake_deg: 44.0,
        nose_plan_round: 0.16,
        tail_plan_round: 0.12,
        hood_crown: 0.016,
        windshield_rake_deg: Some(64.0),
        a_pillar_rake_deg: 18.0,
        rear_glass_rake_deg: Some(62.0),
        roof_len_trim: 0.14,
        tumblehome: Some(0.26),
        crease_z: 0.47,
        crease_depth: 0.028,
        haunch: 0.050,
        front_haunch: 0.030,
        flare: 0.016,
        ride_dz: -0.01,
        stance_pitch_deg: 0.2,
        ducktail_h: 0.020,
        belt_kick: 0.07,
        belt_rake: 0.03,
        tail_round: 0.08,
        tail_drop: 0.02,
        ..BodyStyle::preset(Silhouette::Wedge)
    });
    s.details.push(kit_detail(kagero_kit));
    wheels(
        &mut s,
        def,
        RimKind::Turbine,
        15,
        0.76,
        M::Metal,
        CapKind::Centre,
        (0.025, 0.012),
        (0.0, 0.5),
    );
    let m = &mut s.mechanical;
    m.caliper_rgb = Some(rgb(0x1E6FD9));
    s.accent_rgb = Some(rgb(0x1E8CFF));
    s.tints.push((M::Paint, rgb(0xD9DDE2)));
    s
}

fn kagero_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;
    let hood = BodyRegion::Hood;

    // --- Slim lamps with a boomerang DRL: along the front edge, then back along the fender.
    for sx in SIDES {
        let (xi, xo) = (0.46, wn - 0.05);
        k.top_lamp(sx, yn - 0.13, yn - 0.06, xi + 0.04, xo - 0.04, 0.025);
        // The boomerang belongs to the lens part: it runs in front of the slim lens, so from
        // ahead it is what you see of the lamp.
        let kd = headlight_key(sx);
        let corner = [sx * (xo - 0.01), yn - 0.04];
        top_stroke(&mut k, &kd, [sx * xi, yn - 0.05], corner, 0.016, 0.016);
        top_stroke(
            &mut k,
            &kd,
            corner,
            [sx * (xo - 0.07), yn - 0.34],
            0.016,
            0.016,
        );
    }
    // --- Tall outer intakes with a folded blade, low centre mesh, blue accent, splitter.
    let (zo0, zo1) = (nz0 + 0.04, nz1 - 0.07);
    for sx in SIDES {
        let (xi, xo) = (0.44, wn - 0.04);
        let kg = sided(&dkey("front_intake_l", fb, M::Grille), sx);
        k.front_box(&kg, sx * xi, sx * xo, zo0, zo1, 0.006, 0.0);
        let km = sided(&dkey("front_intake_bars_l", fb, M::Trim), sx);
        for i in 1..3 {
            let z = zo0 + (zo1 - zo0) * i as f32 / 3.0;
            k.front_box(&km, sx * xi, sx * xo, z - 0.005, z + 0.005, 0.012, 0.0);
        }
        let kb = sided(&dkey("front_intake_blade_l", fb, M::Paint), sx);
        front_stroke(
            &mut k,
            &kb,
            [sx * xi, zo1],
            [sx * xo, zo0 + 0.03],
            0.035,
            0.022,
        );
    }
    k.mesh_grille(
        "centre_intake",
        fb,
        0.36,
        nz0 + 0.03,
        nz0 + 0.12,
        1,
        8,
        M::Trim,
    );
    let ka = dkey("hybrid_accent_front", fb, M::Accent);
    k.front_box(&ka, -0.36, 0.36, nz0 + 0.135, nz0 + 0.15, 0.012, 0.0);
    k.splitter(M::Carbon, 0.07, 0.06, 0.02, false);
    for sx in SIDES {
        let name = if sx < 0.0 {
            "hood_vent_l"
        } else {
            "hood_vent_r"
        };
        let (a, c) = (sx * 0.14, sx * 0.32);
        k.top_vent(
            name,
            hood,
            p.y_fw + 0.10,
            p.y_fw + 0.28,
            a.min(c),
            a.max(c),
            3,
            M::Paint,
        );
    }

    // --- Black floating roof and A-pillars.
    roof_sheet(&mut k, "roof_black", 0.0, 0.005, M::Trim);
    let (r0, r1, _, _) = p.roof;
    let kp = dkey("a_pillar_black_l", BodyRegion::Roof, M::Trim);
    for sx in SIDES {
        let kp = sided(&kp, sx);
        let mut ya = p.y_fw;
        while ya > r1 && !p.has_greenhouse(ya) {
            ya -= 0.02;
        }
        let yb = r1 + 0.02;
        let xa = k.roof_edge(ya) - 0.02;
        let xb = k.roof_edge(yb) - 0.02;
        let a = [sx * xa, ya, k.top_z(ya, sx * xa) + 0.006];
        let c = [sx * xb, yb, k.top_z(yb, sx * xb) + 0.006];
        let n = p.top_point(0.5 * (ya + yb), sx * 0.5 * (xa + xb)).1;
        stroke(&mut k, &kp, a, c, n, 0.06, 0.012);
    }

    // --- Flying buttresses (body colour) over a dark air channel through the C-pillar.
    let yb = p.y_deck - 0.30;
    for sx in SIDES {
        let xa = k.roof_edge(r0) - 0.03;
        let xb = p.half_width(yb) - 0.16;
        let a = [sx * xa, r0 - 0.02, k.top_z(r0 - 0.02, sx * xa) + 0.005];
        let c = [sx * xb, yb, k.top_z(yb, sx * xb) + 0.01];
        let kc = sided(
            &dkey("c_pillar_channel_l", BodyRegion::RearLeftQuarter, M::Grille),
            sx,
        );
        let a2 = [a[0] - sx * 0.09, a[1] - 0.04, a[2] - 0.02];
        let c2 = [c[0] - sx * 0.09, c[1], c[2] - 0.01];
        stroke(&mut k, &kc, a2, c2, [sx * 0.5, 0.0, 0.85], 0.10, 0.02);
        let kf = sided(
            &dkey("buttress_l", BodyRegion::RearLeftQuarter, M::Paint),
            sx,
        );
        stroke(&mut k, &kf, a, c, [sx * 0.5, 0.0, 0.85], 0.13, 0.04);
    }

    // --- Deep folded intakes in the doors, running into a big quarter intake.
    let (yr, zr, rr) = k.arch(false);
    let zb = p.belt(p.y_fd_rear);
    for sx in SIDES {
        let kd = dkey("door_intake_l", BodyRegion::LeftDoorF, M::Grille);
        k.side_patch(
            &kd,
            sx,
            p.y_fd_rear + 0.01,
            p.y_fd_hinge - 0.40,
            zb - 0.24,
            zb - 0.07,
            0.010,
            0.006,
            (4, 2),
        );
        let kf = dkey("door_fold_l", BodyRegion::LeftDoorF, M::Paint);
        k.side_patch(
            &kf,
            sx,
            p.y_fd_rear + 0.01,
            p.y_fd_hinge - 0.20,
            zb - 0.07,
            zb - 0.045,
            0.008,
            0.012,
            (5, 1),
        );
        // Folded front edge of the door intake.
        let ke = sided(
            &dkey("door_intake_edge_l", BodyRegion::LeftDoorF, M::Paint),
            sx,
        );
        let (ya0, ya1) = (p.y_fd_hinge - 0.40, p.y_fd_hinge - 0.56);
        let (pa, _) = p.side_point(ya0, zb - 0.075);
        let (pc, _) = p.side_point(ya1, zb - 0.235);
        k.plate(
            &ke,
            [sx * (-pa[0] + 0.012), ya0, zb - 0.075],
            [sx * (-pc[0] + 0.012), ya1, zb - 0.235],
            [sx, 0.0, 0.0],
            0.03,
            0.05,
        );
        let ka = dkey("door_accent_l", BodyRegion::LeftDoorF, M::Accent);
        k.side_patch(
            &ka,
            sx,
            p.y_fd_rear + 0.01,
            p.y_fd_hinge - 0.30,
            zb - 0.255,
            zb - 0.24,
            0.006,
            0.006,
            (4, 1),
        );
        let kq = dkey("quarter_intake_l", BodyRegion::RearLeftQuarter, M::Grille);
        let (q0, q1) = (yr + rr + 0.03, p.y_fd_rear - 0.01);
        k.side_patch(&kq, sx, q0, q1, zr + 0.03, zb - 0.07, 0.002, 0.008, (3, 2));
        // Folded blade across the quarter intake (top-front to bottom-rear).
        let kb = sided(
            &dkey(
                "quarter_intake_fold_l",
                BodyRegion::RearLeftQuarter,
                M::Paint,
            ),
            sx,
        );
        let (pa, _) = p.side_point(q1, zb - 0.08);
        let (pc, _) = p.side_point(q0 + 0.02, zr + 0.05);
        k.plate(
            &kb,
            [sx * (-pa[0] + 0.015), q1, zb - 0.08],
            [sx * (-pc[0] + 0.015), q0 + 0.02, zr + 0.05],
            [sx, 0.0, 0.0],
            0.03,
            0.05,
        );
    }
    door_seams(&mut k);
    k.skirts(M::Trim, 0.03, 0.05);
    fender_outlet(&mut k, 2, M::Paint);
    // Blue accent line along the outer face of each skirt (skirt: 0.03 out of the side).
    let (yf, _, rf) = k.arch(true);
    let ys = 0.5 * (yr + rr + yf - rf);
    let zs = p.sill(ys);
    let xs = k.side_x(ys, zs + 0.06) + 0.03;
    for sx in SIDES {
        let ka = sided(
            &dkey("skirt_accent_l", BodyRegion::FrameLeft, M::Accent),
            sx,
        );
        k.bx(
            &ka,
            [sx * (xs - 0.004), yr + rr + 0.08, zs - 0.004],
            [sx * (xs + 0.004), yf - rf - 0.08, zs + 0.010],
        );
    }

    // --- Engine deck vents either side of the rear glass base.
    for sx in SIDES {
        let name = if sx < 0.0 {
            "deck_vent_l"
        } else {
            "deck_vent_r"
        };
        let (a, c) = (sx * 0.22, sx * 0.48);
        k.top_vent(
            name,
            BodyRegion::Trunk,
            yr - rr + 0.25,
            p.y_deck - 0.08,
            a.min(c),
            a.max(c),
            4,
            M::Trim,
        );
    }
    let kg = dkey("engine_cover_glass", BodyRegion::Trunk, M::Glass);
    k.patch(
        &kg,
        yr - rr + 0.25,
        p.y_deck - 0.08,
        top_band(-0.17, 0.17),
        0.004,
        0.006,
        (3, 1),
    );
    // --- Small carbon active spoiler at the deck edge.
    k.lip_spoiler(M::Carbon, k.yt + 0.22, 0.05, 0.07, 0.24);

    // --- Rear: black mesh fascia, thin full-width bar folding into a Z at each corner.
    let zt = tz1 - 0.04;
    let (fz0, fz1) = (tz0 + 0.10, zt - 0.03);
    k.rear_panel(
        "rear_fascia",
        BodyRegion::RearBumper,
        wt - 0.04,
        fz0,
        fz1,
        M::Grille,
    );
    let km = dkey("rear_fascia_mesh", BodyRegion::RearBumper, M::Trim);
    for i in 1..4 {
        let z = fz0 + (fz1 - fz0) * i as f32 / 4.0;
        k.rear_box(&km, -wt + 0.08, wt - 0.08, z - 0.005, z + 0.005, 0.016, 0.0);
    }
    let kb = lkey("taillight_bar", BodyRegion::Trunk, M::Taillight);
    k.rear_box(&kb, -0.42, 0.42, zt - 0.006, zt + 0.008, 0.022, 0.0);
    let ka = dkey("hybrid_accent_rear", BodyRegion::Trunk, M::Accent);
    k.rear_box(&ka, -0.30, 0.30, zt - 0.03, zt - 0.02, 0.018, 0.0);
    for sx in SIDES {
        let tk = taillight_key(sx);
        let (xa, xb) = (0.42, wt - 0.02);
        k.rear_box(&tk, sx * xa, sx * xb, zt - 0.006, zt + 0.008, 0.022, 0.0);
        rear_stroke(
            &mut k,
            &tk,
            [sx * (xb - 0.01), zt],
            [sx * (xb - 0.17), zt - 0.075],
            0.014,
            0.022,
        );
        k.rear_box(
            &tk,
            sx * (xb - 0.18),
            sx * xb,
            zt - 0.082,
            zt - 0.068,
            0.022,
            0.0,
        );
    }
    // --- Quad tips in a carbon tray, diffuser.
    let ze = (p.z_floor + 0.14).max(0.24);
    let kx = dkey("exhaust_tray", BodyRegion::RearBumper, M::Carbon);
    k.rear_box(&kx, -0.42, 0.42, ze - 0.065, ze + 0.065, 0.02, 0.012);
    k.diffuser(M::Carbon, 0.66, 0.12, 4, 0.06);

    k.mirrors(0.16, 0.10, Some(M::Trim));
    k.wipers();

    // Turn signals as segments beside the lamps (playtest row 136).
    k.signals_beside_lamps(0.06, M::EmissiveAmber);
}
