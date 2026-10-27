//! Hot hatches: `vireo_xs` (late-90s Japanese FF hatch), `stoat_16v` (early-90s European FF
//! 16-valve hatch, boxy), `marten_20` (early-90s European FF 2.0 hatch, wedge) and
//! `lindwurm_4t` (2010s European AWD 5-door hatch).
//!
//! Genre cues: two-box profile, short roof falling to a raked hatch glass over a short
//! near-vertical tailgate, a thick C-pillar with no quarter window, wheels filling the arches,
//! a roof spoiler at the top of the hatch, and one accent colour. Each car then has its own
//! lamp shapes, grille, bumpers and tail-lamp pattern (the night-time identifier):
//!
//! | car | front | rear |
//! |---|---|---|
//! | vireo | swept clear lamps wrapping onto the nose top, slim slot grille, big lower mouth | vertical tri-colour corner clusters, black hatch garnish |
//! | stoat | full-width black band: square twin-lamp housings and a slatted grille, black bumpers | wide ribbed horizontal lamps either side of a black plate panel |
//! | marten | slim wedge lamps almost meeting in the middle, grille-less nose | full-width red band (lamps + reflector panel) |
//! | lindwurm | slim LED lamps with a DRL brow, grille bars running into them, big honeycomb intake | two-piece LED lamps with a light-bar signature, diffuser + quad tips |
//!
//! The lattices of all four cars have front and rear doors (detachable), so the meshes are
//! 5-door hatches. Only render geometry is styled here; physics data is untouched.

// Geometry helpers take explicit coordinates on purpose (same convention as `kit.rs`).
#![allow(clippy::too_many_arguments)]

use wp_contracts::{BodyRegion, VehicleDef};

use crate::builder::{Builder, PartKey};
use crate::mesh::{MaterialSlot as M, PartKind, INDICATOR_SEAM_M};
use crate::style::{
    ArchShape, BodyStyle, CaliperKind, CapKind, CarStyle, DetailCtx, EngineKind, ExhaustKind,
    Induction, RimKind, Silhouette, SuspensionKind, TrimFinish,
};

use super::kit::{
    dkey, headlight_key, indicator_front_key, indicator_rear_key, lkey, sided, taillight_key,
    top_band, Kit, Seg, SIDES,
};
use super::{kit_detail, rgb, wheels};

// ---------------------------------------------------------------------------- local helpers

/// Convex outline `pts` ((x, z), any winding) on the nose (`front`) or tail face, swept from
/// 2 cm inside the face to `proud` out of it.
fn face_poly(k: &mut Kit, key: &PartKey, front: bool, pts: &[[f32; 2]], proud: f32) {
    let (y0, y1) = if front {
        (k.yn - 0.02, k.yn + proud)
    } else {
        (k.yt + 0.02, k.yt - proud)
    };
    k.prism_y(key, pts, y0, y1);
}

/// Dark lamp housing round a face lens: the lens outline `pts` grown by `border` (about its
/// centre), less proud than the lens.
fn face_housing(k: &mut Kit, sx: f32, pts: &[[f32; 2]], border: f32, proud: f32) {
    let n = pts.len().max(1) as f32;
    let cx = pts.iter().map(|q| q[0]).sum::<f32>() / n;
    let cz = pts.iter().map(|q| q[1]).sum::<f32>() / n;
    let grown: Vec<[f32; 2]> = pts
        .iter()
        .map(|q| {
            let (dx, dz) = (q[0] - cx, q[1] - cz);
            let l = (dx * dx + dz * dz).sqrt().max(1e-4);
            [q[0] + border * dx / l, q[1] + border * dz / l]
        })
        .collect();
    let kh = sided(
        &dkey("headlamp_housing_l", BodyRegion::FrontLeftFender, M::Trim),
        sx,
    );
    face_poly(k, &kh, true, &on_side(sx, &grown), proud);
}

/// Left-side outline `pts` ((|x|, z)) moved to side `sx`.
fn on_side(sx: f32, pts: &[[f32; 2]]) -> Vec<[f32; 2]> {
    pts.iter().map(|q| [sx * q[0], q[1]]).collect()
}

/// Blank badge (no lettering) centred at (x, z) on the nose or tail face.
fn badge(k: &mut Kit, region: BodyRegion, front: bool, x: f32, z: f32, w: f32, h: f32) {
    let kb = dkey(
        if front { "badge_front" } else { "badge_rear" },
        region,
        M::Chrome,
    );
    if front {
        k.front_box(
            &kb,
            x - 0.5 * w,
            x + 0.5 * w,
            z - 0.5 * h,
            z + 0.5 * h,
            0.022,
            0.006,
        );
    } else {
        k.rear_box(
            &kb,
            x - 0.5 * w,
            x + 0.5 * w,
            z - 0.5 * h,
            z + 0.5 * h,
            0.022,
            0.006,
        );
    }
}

/// Body-side moulding strip `z0..z1` along both sides, one part per panel it crosses (front
/// fender behind the arch, front door, rear door, rear quarter ahead of the arch), so each
/// piece rides and detaches with its panel. Gaps are left at the panel cut lines.
fn side_strip(k: &mut Kit, name: &str, z0: f32, z1: f32, mat: M, thick: f32) {
    use BodyRegion::*;
    let p = k.p;
    let (yf, _, rf) = k.arch(true);
    let (yr, _, rr) = k.arch(false);
    let g = 0.012;
    let mut spans: Vec<(BodyRegion, &str, f32, f32)> =
        vec![(FrontLeftFender, "fender", p.y_fd_hinge + g, yf - rf - 0.03)];
    match p.rear_door {
        Some((rh, re)) => {
            spans.push((LeftDoorF, "door_f", rh + g, p.y_fd_hinge - g));
            spans.push((LeftDoorR, "door_r", re + g, rh - g));
            spans.push((RearLeftQuarter, "quarter", yr + rr + 0.03, re - g));
        }
        None => {
            spans.push((LeftDoorF, "door_f", p.y_fd_rear + g, p.y_fd_hinge - g));
            spans.push((RearLeftQuarter, "quarter", yr + rr + 0.03, p.y_fd_rear - g));
        }
    }
    for (region, tag, y0, y1) in spans {
        if y1 - y0 < 0.06 {
            continue;
        }
        let kl = dkey(&format!("{name}_{tag}_l"), region, mat);
        let steps = (((y1 - y0) / 0.12).ceil() as usize).max(1);
        for sx in SIDES {
            k.side_patch(&kl, sx, y0, y1, z0, z1, 0.002, thick, (steps, 1));
        }
    }
}

/// Centre high-mounted brake lamp under the trailing edge of a [`Kit::roof_spoiler`]
/// (`chord`, `drop` as passed there), `half` wide.
fn high_brake_lamp(k: &mut Kit, chord: f32, drop: f32, half: f32) {
    let y0 = k.p.roof.0 + 0.06;
    let z0 = k.top_z(y0, 0.0);
    let kl = lkey("brake_lamp_high", BodyRegion::Trunk, M::Taillight);
    let ye = y0 - chord;
    k.bx(
        &kl,
        [-half, ye - 0.004, z0 - drop - 0.018],
        [half, ye + 0.03, z0 - drop + 0.004],
    );
}

/// Tail lamp wrap onto the rear quarter (both sides): a lamp lens on the side surface from
/// the tail face `len` forward, `z0..z1`.
fn tail_wrap(k: &mut Kit, len: f32, z0: f32, z1: f32) {
    let kw = PartKey {
        kind: PartKind::Light,
        ..dkey(
            "taillight_wrap_l",
            BodyRegion::RearLeftQuarter,
            M::Taillight,
        )
    };
    let yt = k.yt;
    for sx in SIDES {
        k.side_patch(&kw, sx, yt + 0.012, yt + len, z0, z1, 0.002, 0.012, (2, 1));
    }
}

/// Small red reflectors at the rear bumper corners.
fn rear_reflectors(k: &mut Kit, z: f32, inset: f32) {
    let wt = k.wt();
    for sx in SIDES {
        let kr = sided(
            &dkey("rear_reflector_l", BodyRegion::RearBumper, M::Taillight),
            sx,
        );
        k.rear_box(
            &kr,
            sx * (wt - inset - 0.10),
            sx * (wt - inset),
            z - 0.012,
            z + 0.012,
            0.016,
            0.003,
        );
    }
}

/// Licence plate (blank, light) on a dark recess on the tail face.
fn rear_plate(k: &mut Kit, region: BodyRegion, zc: f32) {
    let kp = dkey("plate_recess", region, M::Trim);
    k.rear_box(&kp, -0.27, 0.27, zc - 0.07, zc + 0.07, 0.006, 0.0);
    let kq = dkey("plate", region, M::Metal);
    k.rear_box(&kq, -0.25, 0.25, zc - 0.055, zc + 0.055, 0.012, 0.004);
}

/// Rear wiper parked on the hatch glass.
fn rear_wiper(k: &mut Kit) {
    let p = k.p;
    let wy = 0.5 * (p.roof.0 + p.y_deck);
    let kw = dkey("rear_wiper", BodyRegion::RearGlass, M::Trim);
    let za = k.top_z(wy - 0.10, 0.0) + 0.015;
    let zb = k.top_z(wy + 0.05, 0.35) + 0.015;
    k.plate(
        &kw,
        [0.0, wy - 0.10, za],
        [0.35, wy + 0.05, zb],
        [0.0, 0.0, 1.0],
        0.015,
        0.012,
    );
}

// ---------------------------------------------------------------------------- vireo

/// Late-90s Japanese FF hot hatch (`vireo_xs`): low rounded nose with swept clear lamps
/// wrapping onto the nose top, slim grille slot over a big lower mouth with a red accent,
/// short roof falling to a raked hatch, thick C-pillar, vertical tri-colour tail clusters,
/// body-colour roof spoiler. White with red accents and white 5-spokes.
pub fn vireo(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        nose_h: Some(0.60),
        nose_rake_deg: 32.0,
        nose_plan_round: 0.38,
        tail_plan_round: 0.16,
        hood_crown: 0.02,
        windshield_rake_deg: Some(60.0),
        a_pillar_rake_deg: 9.0,
        rear_glass_rake_deg: Some(40.0),
        roof_len_trim: -0.18,
        roof_arc: 0.30,
        roof_crown: 0.025,
        tumblehome: Some(0.14),
        belt_dz: 0.05,
        belt_rake: 0.05,
        crease_z: 0.50,
        crease_depth: 0.006,
        haunch: 0.012,
        flare: 0.010,
        ride_dz: -0.02,
        tail_round: 0.12,
        tail_drop: 0.03,
        a_pillar_w_m: 0.07,
        c_pillar_w_m: 0.22,
        windshield_wrap_m: 0.08,
        arch_gap: 0.05,
        overhang_trim: (-0.05, -0.02),
        ..BodyStyle::preset(Silhouette::Hatch)
    });
    s.details.push(kit_detail(vireo_kit));
    wheels(
        &mut s,
        def,
        RimKind::FiveSpoke,
        5,
        0.66,
        M::Metal,
        CapKind::Small,
        (0.02, 0.012),
        (0.0, 0.5),
    );
    let m = &mut s.mechanical;
    m.engine = EngineKind::Inline4;
    m.induction = Induction::Natural;
    m.front_suspension = SuspensionKind::DoubleWishbone;
    m.rear_suspension = SuspensionKind::DoubleWishbone;
    m.caliper_rgb = Some(rgb(0xC8102E));
    m.spring_rgb = Some(rgb(0xC8102E));
    m.exhaust = ExhaustKind::Single;
    m.exhaust_tip_r_m = 0.045;
    s.tints.push((M::Paint, rgb(0xF4F3EE)));
    // Red stripe in the accent slot; white 5-spoke wheels (`rim` slot).
    s.accent_rgb = Some(rgb(0xC8102E));
    s.wheel.rim_rgb = Some(rgb(0xF2F2EE));
    s
}

fn vireo_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let wn = k.wn();
    let wt = k.wt();
    let yn = k.yn;
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;
    let g = INDICATOR_SEAM_M;

    // --- Front lamps: a swept clear lens on the face (inner edge raked back toward the
    // grille) with the amber signal at its outer corner, continued onto the nose top.
    let (lt, lb) = (nz1 - 0.034, nz1 - 0.105);
    let xo = wn - 0.045;
    let xs = xo - 0.075;
    for sx in SIDES {
        let hk = headlight_key(sx);
        let lens = [[0.30, lt], [xs - g, lt], [xs - g, lb], [0.37, lb]];
        face_poly(&mut k, &hk, true, &on_side(sx, &lens), 0.022);
        // Housing runs up over the rounded nose edge to meet the top housing.
        let housing = [
            [0.285, nz1 - 0.004],
            [xo + 0.014, nz1 - 0.004],
            [xo + 0.014, lb - 0.014],
            [0.355, lb - 0.014],
        ];
        face_housing(&mut k, sx, &housing, 0.0, 0.012);
        let kh = sided(
            &dkey(
                "headlamp_top_housing_l",
                BodyRegion::FrontLeftFender,
                M::Trim,
            ),
            sx,
        );
        k.patch(
            &kh,
            yn - 0.145,
            yn - 0.002,
            top_band(sx * 0.31, sx * (xo + 0.005)),
            0.002,
            0.007,
            (3, 2),
        );
        k.patch(
            &hk,
            yn - 0.13,
            yn - 0.004,
            top_band(sx * 0.33, sx * (xo - 0.015)),
            0.003,
            0.010,
            (2, 2),
        );
        let ik = indicator_front_key(sx);
        let sig = [[xs, lt], [xo, lt], [xo, lb + 0.03], [xs, lb]];
        face_poly(&mut k, &ik, true, &on_side(sx, &sig), 0.022);
        // Projector reflectors behind the clear lens.
        let kr = sided(
            &dkey(
                "headlamp_reflector_l",
                BodyRegion::FrontLeftFender,
                M::Chrome,
            ),
            sx,
        );
        // Kept clear of the lens face centres so the lens stays visible face-on.
        for (x, r) in [(xs - 0.045, 0.026), (xs - 0.16, 0.020)] {
            k.round_lamp(&kr, true, sx * x, 0.5 * (lt + lb), r, 0.026, None);
        }
    }

    // --- Slim grille slot between the lamps, badge in it.
    k.grille(
        "grille",
        BodyRegion::Hood,
        0.27,
        lb + 0.012,
        lt - 0.02,
        1,
        M::Trim,
        None,
    );
    badge(
        &mut k,
        BodyRegion::Hood,
        true,
        0.0,
        0.5 * (lb + lt) - 0.004,
        0.10,
        0.036,
    );

    // --- Big lower mouth (trapezoid), bars, round fogs, red accent line and chin lip.
    let zm0 = nz0 + 0.07;
    let zm1 = nz0 + 0.23;
    let km = dkey("lower_mouth", fb, M::Grille);
    face_poly(
        &mut k,
        &km,
        true,
        &[[-0.50, zm1], [0.50, zm1], [0.44, zm0], [-0.44, zm0]],
        0.006,
    );
    let kb = dkey("lower_mouth_bars", fb, M::Trim);
    for i in 1..=2 {
        let z = zm0 + (zm1 - zm0) * i as f32 / 3.0;
        k.front_box(&kb, -0.47, 0.47, z - 0.007, z + 0.007, 0.014, 0.0);
    }
    k.fogs(wn - 0.13, 0.5 * (zm0 + zm1), 0.042);
    let accent = dkey("accent_stripe", fb, M::Accent);
    k.front_box(&accent, -0.52, 0.52, zm1 + 0.012, zm1 + 0.026, 0.016, 0.0);
    k.splitter(M::Trim, 0.035, 0.12, 0.022, false);
    k.front_box(
        &accent,
        -wn + 0.13,
        wn - 0.13,
        nz0 + 0.008,
        nz0 + 0.026,
        0.03,
        0.0,
    );

    // --- Rear: tall vertical tri-colour clusters at the corners, wrapping onto the quarters.
    let zh = tz1 - 0.05;
    let zl = zh - 0.30;
    let zs = zl + 0.42 * (zh - zl);
    for sx in SIDES {
        let tk = taillight_key(sx);
        let (x0, x1) = (sx * (wt - 0.24), sx * (wt - 0.045));
        k.rear_box(&tk, x0, x1, zs + 0.005, zh, 0.024, 0.008);
        let ka = indicator_rear_key(sx, M::EmissiveAmber);
        k.rear_box(&ka, x0, x1, zl + 0.06, zs - 0.005, 0.024, 0.008);
        let kc = sided(
            &lkey("reverse_lamp_l", BodyRegion::RearLeftQuarter, M::Headlight),
            sx,
        );
        k.rear_box(&kc, x0, x1, zl, zl + 0.055, 0.02, 0.006);
    }
    tail_wrap(&mut k, 0.10, zs + 0.02, zh - 0.02);
    // Black garnish across the tailgate between the clusters, badge on it.
    let kg = dkey("hatch_garnish", BodyRegion::Trunk, M::Trim);
    k.rear_box(
        &kg,
        -wt + 0.26,
        wt - 0.26,
        zh - 0.10,
        zh - 0.01,
        0.012,
        0.004,
    );
    badge(
        &mut k,
        BodyRegion::Trunk,
        false,
        0.0,
        zh - 0.055,
        0.09,
        0.03,
    );
    let (chord, drop) = (0.20, 0.03);
    k.roof_spoiler(M::Paint, chord, drop, 0.04, 0.0);
    high_brake_lamp(&mut k, chord, drop, 0.14);
    rear_wiper(&mut k);
    rear_plate(&mut k, BodyRegion::RearBumper, tz0 + 0.26);
    let kv = dkey("rear_valance", BodyRegion::RearBumper, M::Trim);
    k.rear_box(
        &kv,
        -wt + 0.06,
        wt - 0.06,
        tz0 + 0.02,
        tz0 + 0.10,
        0.012,
        0.004,
    );
    rear_reflectors(&mut k, tz0 + 0.14, 0.05);

    // --- Sides: small body-colour skirts, mirrors, repeaters, black handles, filler.
    k.skirts(M::Paint, 0.012, 0.06);
    k.mirrors(0.10, 0.11, None);
    k.fender_repeaters();
    k.door_handles(M::Trim);
    k.fuel_filler(1.0, 0.10, M::Paint);
}

// ---------------------------------------------------------------------------- stoat

/// Early-90s European FF 16-valve hot hatch (`stoat_16v`): upright and boxy, crisp edges,
/// a full-width black front band (square twin-lamp housings either side of a slatted grille
/// with a yellow pinstripe), black bumpers wrapping to the arches and black rubbing strips,
/// a very thick C-pillar, wide ribbed tail lamps either side of a black plate panel. Dark
/// green with silver multi-spokes and a yellow accent.
pub fn stoat(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        nose_h: Some(0.68),
        nose_rake_deg: 10.0,
        nose_plan_round: 0.16,
        tail_plan_round: 0.10,
        hood_crown: 0.008,
        windshield_rake_deg: Some(56.0),
        a_pillar_rake_deg: 6.0,
        rear_glass_rake_deg: Some(30.0),
        roof_len_trim: -0.10,
        roof_arc: 0.10,
        roof_crown: 0.012,
        tumblehome: Some(0.12),
        belt_dz: 0.02,
        crease_z: 0.50,
        crease_depth: 0.004,
        sill_tuck: 0.04,
        shoulder_round: 0.03,
        flare: 0.014,
        ride_dz: -0.01,
        tail_round: 0.06,
        tail_drop: 0.01,
        a_pillar_w_m: 0.075,
        c_pillar_w_m: 0.28,
        windshield_wrap_m: 0.03,
        rear_glass_wrap_m: 0.02,
        end_round_m: 0.02,
        arch_gap: 0.05,
        overhang_trim: (-0.05, -0.02),
        ..BodyStyle::preset(Silhouette::Hatch)
    });
    s.details.push(kit_detail(stoat_kit));
    wheels(
        &mut s,
        def,
        RimKind::MultiSpoke,
        8,
        0.64,
        M::Metal,
        CapKind::Small,
        (0.02, 0.012),
        (0.0, 0.5),
    );
    let m = &mut s.mechanical;
    m.engine = EngineKind::Inline4;
    m.induction = Induction::Natural;
    m.front_suspension = SuspensionKind::Strut;
    m.rear_suspension = SuspensionKind::DoubleWishbone;
    m.caliper_rgb = Some(rgb(0x2A2A2C));
    m.spring_rgb = Some(rgb(0xE8C21A));
    m.exhaust = ExhaustKind::Single;
    m.exhaust_tip_r_m = 0.04;
    s.tints.push((M::Paint, rgb(0x1F4D2B)));
    s.accent_rgb = Some(rgb(0xE8C21A));
    s.wheel.rim_rgb = Some(rgb(0xC9CCD1));
    s
}

fn stoat_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let wn = k.wn();
    let wt = k.wt();
    let yn = k.yn;
    let yt = k.yt;
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;
    let rb = BodyRegion::RearBumper;

    // --- Full-width black front band: square housings with twin round lamps, signal wrapping
    // the corner, slatted grille between them with a yellow pinstripe under it.
    let (zt, zb) = (nz1 - 0.025, nz1 - 0.175);
    let xi = 0.19;
    let xo = wn - 0.04;
    let band = dkey("front_band", BodyRegion::Hood, M::Trim);
    k.front_box(&band, -xo, xo, zb - 0.012, zt + 0.012, 0.010, 0.004);
    for sx in SIDES {
        k.front_cluster(
            sx,
            xi,
            xo - 0.01,
            zb,
            zt,
            0.020,
            0.006,
            Seg::Outboard(0.075),
        );
        let kr = sided(
            &dkey(
                "headlamp_reflector_l",
                BodyRegion::FrontLeftFender,
                M::Chrome,
            ),
            sx,
        );
        let zc = 0.5 * (zb + zt);
        let xr = xo - 0.01 - 0.075 - INDICATOR_SEAM_M;
        let pitch = (xr - xi) / 2.0;
        for i in 0..2 {
            let x = xi + pitch * (i as f32 + 0.5);
            k.round_lamp(&kr, true, sx * x, zc, 0.052, 0.026, None);
        }
    }
    k.grille(
        "grille",
        BodyRegion::Hood,
        xi - 0.015,
        zb + 0.01,
        zt - 0.01,
        3,
        M::Trim,
        None,
    );
    badge(
        &mut k,
        BodyRegion::Hood,
        true,
        0.0,
        0.5 * (zb + zt),
        0.08,
        0.05,
    );
    let accent = dkey("accent_stripe", BodyRegion::Hood, M::Accent);
    k.front_box(
        &accent,
        -xo + 0.02,
        xo - 0.02,
        zb - 0.026,
        zb - 0.016,
        0.014,
        0.0,
    );

    // --- Black front bumper with a lower slot, fogs, and side wraps to the arches.
    let (bz0, bz1) = (nz0 + 0.02, nz0 + 0.24);
    let kb = dkey("bumper_band", fb, M::Trim);
    k.front_box(&kb, -wn - 0.005, wn + 0.005, bz0, bz1, 0.035, 0.012);
    let (yf, _, rf) = k.arch(true);
    let kbs = dkey("bumper_band_side_l", fb, M::Trim);
    for sx in SIDES {
        k.side_patch(
            &kbs,
            sx,
            yf + rf + 0.03,
            yn - 0.03,
            bz0 + 0.04,
            bz1,
            0.004,
            0.02,
            (3, 1),
        );
    }
    k.grille(
        "lower_slot",
        fb,
        0.36,
        bz0 + 0.05,
        bz0 + 0.13,
        1,
        M::Trim,
        None,
    );
    for sx in SIDES {
        let kf = sided(&lkey("fog_l", fb, M::Headlight), sx);
        k.front_box(
            &kf,
            sx * (wn - 0.20),
            sx * (wn - 0.08),
            bz0 + 0.06,
            bz0 + 0.12,
            0.045,
            0.006,
        );
    }

    // --- Sides: black rubbing strips continuing the bumper line, black mirror caps.
    side_strip(&mut k, "rub_strip", bz1 - 0.07, bz1 - 0.01, M::Trim, 0.016);
    k.mirrors(0.10, 0.11, Some(M::Trim));
    k.fender_repeaters();
    k.door_handles(M::Trim);
    k.fuel_filler(1.0, 0.10, M::Paint);

    // --- Rear: wide ribbed horizontal lamps (red, amber outer, clear inner strip), black
    // plate panel between them, black bumper with wraps, small black roof spoiler.
    let (lz0, lz1) = (tz1 - 0.21, tz1 - 0.05);
    let (lxi, lxo) = (0.30, wt - 0.045);
    let kp = dkey("tail_panel", BodyRegion::Trunk, M::Trim);
    k.rear_box(&kp, -lxi + 0.01, lxi - 0.01, lz0, lz1, 0.010, 0.004);
    rear_plate(&mut k, BodyRegion::Trunk, 0.5 * (lz0 + lz1));
    for sx in SIDES {
        k.rear_cluster(
            sx,
            lxi + 0.07,
            lxo,
            lz0,
            lz1,
            0.022,
            0.006,
            Seg::Outboard(0.09),
            M::EmissiveAmber,
        );
        let kc = sided(
            &lkey("reverse_lamp_l", BodyRegion::RearLeftQuarter, M::Headlight),
            sx,
        );
        k.rear_box(&kc, sx * lxi, sx * (lxi + 0.06), lz0, lz1, 0.022, 0.006);
        let ribs = sided(
            &dkey("taillight_ribs_l", BodyRegion::RearLeftQuarter, M::Trim),
            sx,
        );
        for f in [0.36, 0.66] {
            let z = lz0 + f * (lz1 - lz0);
            k.rear_box(
                &ribs,
                sx * (lxi + 0.07),
                sx * (lxo - 0.10),
                z - 0.004,
                z + 0.004,
                0.026,
                0.0,
            );
        }
    }
    tail_wrap(&mut k, 0.07, lz0 + 0.02, lz1 - 0.02);
    let (rz0, rz1) = (tz0 + 0.02, tz0 + 0.22);
    let krb = dkey("rear_bumper_band", rb, M::Trim);
    k.rear_box(&krb, -wt - 0.005, wt + 0.005, rz0, rz1, 0.035, 0.012);
    let (yr, _, rr) = k.arch(false);
    let krs = dkey("rear_bumper_band_side_l", rb, M::Trim);
    for sx in SIDES {
        k.side_patch(
            &krs,
            sx,
            yt + 0.03,
            yr - rr - 0.03,
            rz0 + 0.04,
            rz1,
            0.004,
            0.02,
            (2, 1),
        );
    }
    rear_reflectors(&mut k, rz1 - 0.05, 0.06);
    let (chord, drop) = (0.14, 0.02);
    k.roof_spoiler(M::Trim, chord, drop, 0.05, 0.0);
    high_brake_lamp(&mut k, chord, drop, 0.12);
    rear_wiper(&mut k);
}

// ---------------------------------------------------------------------------- marten

/// Early-90s European FF 2.0 hot hatch (`marten_20`): a low wedge nose with slim lamps that
/// almost meet in the middle over a grille-less nose, a body-colour bumper with a wide intake
/// split by a body-colour bar, a rising wedge belt, a full-width red tail band (lamps plus a
/// reflector panel), black body-side mouldings, twin tips. Red, black accent, white turbines.
pub fn marten(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        nose_h: Some(0.58),
        nose_rake_deg: 36.0,
        nose_plan_round: 0.40,
        tail_plan_round: 0.18,
        hood_crown: 0.018,
        windshield_rake_deg: Some(61.0),
        a_pillar_rake_deg: 10.0,
        rear_glass_rake_deg: Some(36.0),
        roof_len_trim: -0.16,
        roof_arc: 0.40,
        roof_crown: 0.022,
        tumblehome: Some(0.15),
        belt_dz: 0.03,
        belt_rake: 0.07,
        crease_z: 0.46,
        crease_depth: 0.008,
        haunch: 0.014,
        front_haunch: 0.008,
        flare: 0.012,
        ride_dz: -0.025,
        tail_round: 0.14,
        tail_drop: 0.04,
        a_pillar_w_m: 0.07,
        c_pillar_w_m: 0.20,
        windshield_wrap_m: 0.09,
        arch_gap: 0.05,
        overhang_trim: (-0.05, -0.02),
        ..BodyStyle::preset(Silhouette::Hatch)
    });
    s.details.push(kit_detail(marten_kit));
    wheels(
        &mut s,
        def,
        RimKind::Turbine,
        12,
        0.64,
        M::Metal,
        CapKind::Centre,
        (0.015, 0.010),
        (0.0, 0.5),
    );
    let m = &mut s.mechanical;
    m.engine = EngineKind::Inline4;
    m.induction = Induction::Natural;
    m.front_suspension = SuspensionKind::Strut;
    m.rear_suspension = SuspensionKind::DoubleWishbone;
    m.caliper_rgb = Some(rgb(0x2A2A2C));
    m.spring_rgb = Some(rgb(0x2A2A2C));
    m.exhaust = ExhaustKind::Dual;
    m.exhaust_tip_r_m = 0.035;
    s.tints.push((M::Paint, rgb(0xB3141C)));
    s.accent_rgb = Some(rgb(0x18181A));
    s.wheel.rim_rgb = Some(rgb(0xF2F2EE));
    s
}

fn marten_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;
    let g = INDICATOR_SEAM_M;

    // --- Slim wedge lamps nearly meeting in the middle, lens continued onto the nose top,
    // amber signal tucked under the outer end; a thin black slot and badge between them.
    let (lt, lb) = (nz1 - 0.034, nz1 - 0.092);
    let (xi, xo) = (0.14, wn - 0.05);
    for sx in SIDES {
        let hk = headlight_key(sx);
        let lens = [[xi, lt], [xo, lt], [xo - 0.05, lb], [xi + 0.03, lb]];
        face_poly(&mut k, &hk, true, &on_side(sx, &lens), 0.020);
        face_housing(&mut k, sx, &lens, 0.012, 0.010);
        let ik = indicator_front_key(sx);
        let sig = [
            [xo - 0.24, lb - g],
            [xo - 0.05, lb - g],
            [xo - 0.07, lb - 0.032],
            [xo - 0.24, lb - 0.032],
        ];
        face_poly(&mut k, &ik, true, &on_side(sx, &sig), 0.018);
        let kr = sided(
            &dkey(
                "headlamp_reflector_l",
                BodyRegion::FrontLeftFender,
                M::Chrome,
            ),
            sx,
        );
        for x in [xi + 0.14, xo - 0.17] {
            k.round_lamp(&kr, true, sx * x, 0.5 * (lt + lb), 0.022, 0.024, None);
        }
    }
    k.grille(
        "grille",
        BodyRegion::Hood,
        xi - 0.02,
        lb + 0.008,
        lt - 0.012,
        0,
        M::Trim,
        None,
    );
    badge(&mut k, BodyRegion::Hood, true, 0.0, lb - 0.035, 0.07, 0.045);

    // --- Body-colour bumper: wide intake split by a body-colour bar, rectangular fogs at its
    // ends, black chin splitter.
    let (zi0, zi1) = (nz0 + 0.06, nz0 + 0.22);
    let ki = dkey("lower_mouth", fb, M::Grille);
    face_poly(
        &mut k,
        &ki,
        true,
        &[[-0.56, zi1], [0.56, zi1], [0.50, zi0], [-0.50, zi0]],
        0.006,
    );
    let kbar = dkey("lower_mouth_bar", fb, M::Paint);
    let zc = 0.5 * (zi0 + zi1);
    k.front_box(&kbar, -0.55, 0.55, zc - 0.016, zc + 0.016, 0.02, 0.006);
    for sx in SIDES {
        let kf = sided(&lkey("fog_l", fb, M::Headlight), sx);
        k.front_box(
            &kf,
            sx * (wn - 0.20),
            sx * (wn - 0.07),
            zi0 + 0.03,
            zi1 - 0.03,
            0.016,
            0.006,
        );
    }
    k.splitter(M::Trim, 0.04, 0.10, 0.02, false);

    // --- Sides: black body-side mouldings, body-colour mirrors, black handles.
    side_strip(&mut k, "side_moulding", 0.38, 0.44, M::Trim, 0.014);
    k.skirts(M::Trim, 0.010, 0.05);
    k.mirrors(0.10, 0.11, None);
    k.fender_repeaters();
    k.door_handles(M::Trim);
    k.fuel_filler(-1.0, 0.10, M::Paint);

    // --- Rear: full-width red band (lamps with amber inboard sections + red reflector panel
    // across the tailgate, badge on it), black lower valance, body-colour roof spoiler.
    let (lz0, lz1) = (tz1 - 0.17, tz1 - 0.055);
    let (lxi, lxo) = (0.25, wt - 0.04);
    for sx in SIDES {
        k.rear_cluster(
            sx,
            lxi,
            lxo,
            lz0,
            lz1,
            0.022,
            0.006,
            Seg::Inboard(0.10),
            M::EmissiveAmber,
        );
        let kc = sided(
            &lkey("reverse_lamp_l", BodyRegion::RearLeftQuarter, M::Headlight),
            sx,
        );
        k.rear_box(
            &kc,
            sx * (lxi + 0.12),
            sx * (lxi + 0.22),
            lz0 + 0.012,
            lz0 + 0.045,
            0.026,
            0.004,
        );
    }
    tail_wrap(&mut k, 0.12, lz0 + 0.015, lz1 - 0.015);
    let kpanel = dkey("tail_reflector_panel", BodyRegion::Trunk, M::Taillight);
    k.rear_box(
        &kpanel,
        -lxi + 0.008,
        lxi - 0.008,
        lz0 + 0.01,
        lz1 - 0.01,
        0.014,
        0.004,
    );
    badge(
        &mut k,
        BodyRegion::Trunk,
        false,
        0.0,
        0.5 * (lz0 + lz1),
        0.10,
        0.035,
    );
    rear_plate(&mut k, BodyRegion::RearBumper, tz0 + 0.25);
    let kv = dkey("rear_valance", BodyRegion::RearBumper, M::Trim);
    k.rear_box(
        &kv,
        -wt + 0.05,
        wt - 0.05,
        tz0 + 0.02,
        tz0 + 0.12,
        0.014,
        0.004,
    );
    rear_reflectors(&mut k, tz0 + 0.155, 0.05);
    let (chord, drop) = (0.16, 0.025);
    k.roof_spoiler(M::Paint, chord, drop, 0.05, 0.0);
    high_brake_lamp(&mut k, chord, drop, 0.12);
    rear_wiper(&mut k);
}

// ---------------------------------------------------------------------------- lindwurm

/// 2010s European AWD hot hatch (`lindwurm_4t`): 5-door, wide and planted, a sharp shoulder
/// line, slim LED head lamps with a DRL brow and grille bars that run into them, a big
/// honeycomb lower intake with bladed side ducts, thick C-pillar, big roof spoiler with end
/// plates, two-piece LED tail lamps with a light-bar signature, diffuser and quad tips,
/// silver mirror caps. Blue with a red pinstripe and dark turbines.
pub fn lindwurm(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        nose_h: Some(0.68),
        nose_rake_deg: 24.0,
        nose_plan_round: 0.28,
        tail_plan_round: 0.18,
        hood_crown: 0.020,
        hood_bulge_h: 0.010,
        hood_bulge_w: 0.50,
        windshield_rake_deg: Some(62.0),
        a_pillar_rake_deg: 11.0,
        rear_glass_rake_deg: Some(52.0),
        roof_len_trim: -0.12,
        roof_arc: 0.22,
        roof_dz: -0.02,
        tumblehome: Some(0.17),
        belt_dz: 0.06,
        belt_rake: 0.03,
        belt_kick: 0.02,
        crease_z: 0.66,
        crease_depth: 0.014,
        haunch: 0.020,
        front_haunch: 0.012,
        flare: 0.015,
        flare_shape: ArchShape::Box,
        ride_dz: -0.02,
        tail_round: 0.10,
        tail_drop: 0.03,
        a_pillar_w_m: 0.08,
        c_pillar_w_m: 0.26,
        windshield_wrap_m: 0.10,
        rear_glass_wrap_m: 0.06,
        door_handles: TrimFinish::Body,
        arch_gap: 0.05,
        overhang_trim: (-0.05, -0.02),
        ..BodyStyle::preset(Silhouette::Hatch)
    });
    s.details.push(kit_detail(lindwurm_kit));
    wheels(
        &mut s,
        def,
        RimKind::SplitSpoke,
        5,
        0.75,
        M::Engine,
        CapKind::Centre,
        (0.03, 0.012),
        (0.0, 0.5),
    );
    let m = &mut s.mechanical;
    m.engine = EngineKind::Inline4;
    m.induction = Induction::Turbo;
    m.front_suspension = SuspensionKind::Strut;
    m.rear_suspension = SuspensionKind::Multilink;
    m.front_brakes.caliper = CaliperKind::Floating;
    m.front_brakes.disc_radius_frac = 0.86;
    m.caliper_rgb = Some(rgb(0x1E5BD6));
    m.exhaust = ExhaustKind::Quad;
    m.exhaust_tip_r_m = 0.045;
    s.tints.push((M::Paint, rgb(0x1E5BD6)));
    s.accent_rgb = Some(rgb(0xC8102E));
    s
}

fn lindwurm_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let yt = k.yt;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;

    // --- Front: slim grille with horizontal bars running out into the lamps, a red pinstripe
    // along its lower edge, slim LED lamps on the nose top with an L-shaped DRL brow.
    let (gz0, gz1) = (nz1 - 0.10, nz1 - 0.02);
    k.grille("grille", BodyRegion::Hood, 0.44, gz0, gz1, 0, M::Trim, None);
    let kbar = dkey("grille_bars", BodyRegion::Hood, M::Trim);
    for f in [0.33, 0.66] {
        let z = gz0 + f * (gz1 - gz0);
        k.front_box(
            &kbar,
            -wn + 0.06,
            wn - 0.06,
            z - 0.006,
            z + 0.006,
            0.016,
            0.0,
        );
    }
    let pin = dkey("accent_bar", BodyRegion::Hood, M::Accent);
    k.front_box(&pin, -0.44, 0.44, gz0 - 0.002, gz0 + 0.008, 0.018, 0.0);
    badge(
        &mut k,
        BodyRegion::Hood,
        true,
        0.0,
        0.5 * (gz0 + gz1),
        0.10,
        0.06,
    );
    for sx in SIDES {
        k.top_lamp_split(
            sx,
            yn - 0.17,
            yn - 0.03,
            0.50,
            wn - 0.06,
            0.022,
            Seg::Outboard(0.09),
        );
        let kh = sided(
            &dkey("headlamp_face_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        k.front_box(
            &kh,
            sx * 0.46,
            sx * (wn - 0.03),
            gz0,
            nz1 - 0.012,
            0.008,
            0.006,
        );
        // Clear lens on the face under the nose-top lens (same head-lamp part).
        let hk = headlight_key(sx);
        k.front_box(
            &hk,
            sx * 0.50,
            sx * (wn - 0.07),
            gz0 + 0.008,
            nz1 - 0.036,
            0.012,
            0.004,
        );
        let drl = sided(
            &lkey("drl_l", BodyRegion::FrontLeftFender, M::Headlight),
            sx,
        );
        // DRL brow along the top of the lamp face, turning down at its inner end.
        k.front_box(
            &drl,
            sx * 0.50,
            sx * (wn - 0.07),
            nz1 - 0.032,
            nz1 - 0.020,
            0.016,
            0.0,
        );
        k.front_box(
            &drl,
            sx * 0.48,
            sx * 0.50,
            gz0 + 0.01,
            nz1 - 0.020,
            0.016,
            0.0,
        );
        let kr = sided(
            &dkey(
                "headlamp_reflector_l",
                BodyRegion::FrontLeftFender,
                M::Chrome,
            ),
            sx,
        );
        k.round_lamp(&kr, true, sx * 0.60, gz0 + 0.03, 0.024, 0.018, None);
    }

    // --- Big honeycomb lower intake (trapezoid) with bladed side ducts, splitter.
    let (zi0, zi1) = (nz0 + 0.05, nz0 + 0.27);
    let ki = dkey("lower_mouth", fb, M::Grille);
    face_poly(
        &mut k,
        &ki,
        true,
        &[[-0.40, zi1], [0.40, zi1], [0.34, zi0], [-0.34, zi0]],
        0.006,
    );
    let km = dkey("lower_mouth_mesh", fb, M::Trim);
    for i in 1..3 {
        let z = zi0 + (zi1 - zi0) * i as f32 / 3.0;
        k.front_box(&km, -0.38, 0.38, z - 0.005, z + 0.005, 0.012, 0.0);
    }
    for i in 1..8 {
        let x = -0.36 + 0.72 * i as f32 / 8.0;
        k.front_box(
            &km,
            x - 0.005,
            x + 0.005,
            zi0 + 0.01,
            zi1 - 0.01,
            0.012,
            0.0,
        );
    }
    for sx in SIDES {
        let kd = sided(&dkey("side_duct_l", fb, M::Grille), sx);
        let duct = [
            [0.46, zi1 - 0.02],
            [wn - 0.08, zi1 - 0.05],
            [wn - 0.10, zi0 + 0.03],
            [0.46, zi0 + 0.02],
        ];
        face_poly(&mut k, &kd, true, &on_side(sx, &duct), 0.006);
        let kb = sided(&dkey("side_duct_blade_l", fb, M::Trim), sx);
        for x in [0.56, 0.66] {
            k.front_box(
                &kb,
                sx * (x - 0.008),
                sx * (x + 0.008),
                zi0 + 0.03,
                zi1 - 0.05,
                0.02,
                0.0,
            );
        }
    }
    k.splitter(M::Trim, 0.05, 0.10, 0.02, false);

    // --- Rear: two-piece wrap-around LED lamps with a light-bar signature, black garnish
    // with badge between them, diffuser, big roof spoiler with plates and a brake lamp.
    let (z0, z1) = (tz1 - 0.13, tz1 - 0.03);
    for sx in SIDES {
        let kh = sided(
            &dkey("taillight_housing_l", BodyRegion::RearLeftQuarter, M::Trim),
            sx,
        );
        k.rear_box(&kh, sx * 0.28, sx * (wt - 0.025), z0, z1, 0.014, 0.006);
        k.rear_cluster(
            sx,
            0.30,
            wt - 0.04,
            z0 + 0.012,
            z1 - 0.012,
            0.022,
            0.006,
            Seg::Inboard(0.10),
            M::EmissiveAmber,
        );
        let ki = sided(
            &lkey("taillight_bar_l", BodyRegion::RearLeftQuarter, M::Taillight),
            sx,
        );
        // Light bar: along the top of the lamp, then down its outer end (L signature).
        k.rear_box(
            &ki,
            sx * 0.42,
            sx * (wt - 0.06),
            z1 - 0.032,
            z1 - 0.020,
            0.032,
            0.0,
        );
        k.rear_box(
            &ki,
            sx * (wt - 0.08),
            sx * (wt - 0.06),
            z0 + 0.024,
            z1 - 0.020,
            0.032,
            0.0,
        );
    }
    tail_wrap(&mut k, 0.16, z0 + 0.015, z1 - 0.015);
    let kg = dkey("hatch_garnish", BodyRegion::Trunk, M::Trim);
    k.rear_box(&kg, -0.27, 0.27, z0 + 0.02, z1 - 0.02, 0.010, 0.004);
    badge(
        &mut k,
        BodyRegion::Trunk,
        false,
        0.0,
        0.5 * (z0 + z1),
        0.10,
        0.045,
    );
    k.diffuser(M::Trim, 0.62, 0.16, 3, 0.05);
    let (chord, drop) = (0.30, 0.05);
    k.roof_spoiler(M::Paint, chord, drop, 0.02, 0.09);
    high_brake_lamp(&mut k, chord, drop, 0.16);
    let kc = dkey("roof_spoiler_gap", BodyRegion::Trunk, M::Trim);
    let y0 = p.roof.0 + 0.04;
    let zr = k.top_z(y0, 0.0);
    k.bx(
        &kc,
        [-0.25, y0 - 0.26, zr - 0.045],
        [0.25, y0 - 0.20, zr - 0.025],
    );
    rear_plate(&mut k, BodyRegion::RearBumper, tz0 + 0.28);
    rear_reflectors(&mut k, tz0 + 0.20, 0.04);
    rear_wiper(&mut k);
    let _ = yt;

    // --- Sides: black skirts, silver mirror caps with repeaters, body handles.
    k.skirts(M::Trim, 0.03, 0.06);
    k.mirrors(0.10, 0.12, Some(M::Metal));
    k.mirror_repeaters(0.10, 0.12, true);
    k.door_handles(M::Paint);
    k.fuel_filler(1.0, 0.11, M::Paint);
}
