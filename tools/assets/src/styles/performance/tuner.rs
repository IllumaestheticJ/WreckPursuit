//! Tuners / sport: `kestrel_ts` (rally sedan), `shiro_turbo` (FR drift coupe), `hakone_gt`
//! (90s twin-turbo GT), `sperber_32` (German I6 sport sedan), `debug_coupe` (sport coupe).
//!
//! Genre cues (research §3): a big rear wing on tall stands (pedestal / swan-neck GT / hoop),
//! a dropped stance with flush wheels and rear camber, aftermarket wheels, a body kit (wide
//! lip, skirts, flares), vinyl + a single cannon exhaust.

use wp_contracts::{BodyRegion, VehicleDef};

use crate::builder::{Builder, PartKey, PatchBand};
use crate::mesh::{MaterialSlot as M, PartKind};
use crate::style::{
    ArchShape, BodyStyle, CaliperKind, CapKind, CarStyle, DetailCtx, EngineKind, EngineLayout,
    ExhaustKind, HoodScoop, Induction, RimKind, Silhouette, SuspensionKind,
};

use super::kit::{
    dkey, headlight_key, lkey, sided, taillight_key, top_band, Kit, Stand, Wing, SIDES,
};
use super::{kit_detail, rgb, wheels};

/// 2000s AWD rally sedan: top-mount intercooler scoop, blistered box flares, tall pedestal
/// wing, big fog lamps, mud flaps, roof vent, gold wheels, round-element tails.
pub fn kestrel(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        nose_h: Some(0.68),
        nose_rake_deg: 20.0,
        nose_plan_round: 0.25,
        tail_plan_round: 0.22,
        hood_crown: 0.015,
        hood_bulge_h: 0.020,
        hood_bulge_w: 0.45,
        hood_scoop: HoodScoop::Scoop,
        hood_scoop_h: 0.07,
        windshield_rake_deg: Some(58.0),
        a_pillar_rake_deg: 8.0,
        rear_glass_rake_deg: Some(55.0),
        tumblehome: Some(0.13),
        crease_z: 0.60,
        crease_depth: 0.010,
        haunch: 0.015,
        front_haunch: 0.025,
        flare: 0.030,
        flare_shape: ArchShape::Box,
        ride_dz: -0.02,
        ducktail_h: 0.015,
        belt_kick: 0.01,
        roof_len_trim: 0.12,
        ..BodyStyle::preset(Silhouette::Notchback)
    });
    s.details.push(kit_detail(kestrel_kit));
    wheels(
        &mut s,
        def,
        RimKind::MultiSpoke,
        12,
        0.70,
        M::Metal,
        CapKind::Centre,
        (0.03, 0.015),
        (0.0, 1.0),
    );
    let m = &mut s.mechanical;
    m.engine = EngineKind::Inline4;
    m.induction = Induction::Turbo;
    m.front_suspension = SuspensionKind::Strut;
    m.rear_suspension = SuspensionKind::Multilink;
    m.caliper_rgb = Some(rgb(0xC81E1E));
    m.exhaust = ExhaustKind::Single;
    m.exhaust_tip_r_m = 0.055;
    s.tints.push((M::Paint, rgb(0x1D3F8C)));
    // Gold rally wheels (`rim` slot: discs and hardware stay metal).
    s.wheel.rim_rgb = Some(rgb(0xB8963E));
    // Subtle blue underglow, lit with the headlights (research §1.3 tuner neon).
    s.underglow_rgb = Some(rgb(0x2050D0));
    s
}

fn kestrel_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;

    // --- Front: rect lamps with projector, upper slot, big lower grille, big fogs, lip.
    k.grille(
        "grille",
        BodyRegion::Hood,
        0.28,
        nz1 - 0.11,
        nz1 - 0.035,
        2,
        M::Chrome,
        Some(M::Trim),
    );
    for sx in SIDES {
        let hk = headlight_key(sx);
        k.front_box(
            &hk,
            sx * 0.34,
            sx * (wn - 0.04),
            nz1 - 0.13,
            nz1 - 0.02,
            0.02,
            0.012,
        );
        let kr = sided(
            &dkey(
                "headlamp_projector_l",
                BodyRegion::FrontLeftFender,
                M::Chrome,
            ),
            sx,
        );
        k.round_lamp(&kr, true, sx * 0.47, nz1 - 0.075, 0.035, 0.026, None);
    }
    let (zm0, zm1) = (nz0 + 0.06, nz0 + 0.25);
    k.mesh_grille("lower_grille", fb, 0.42, zm0, zm1, 3, 6, M::Trim);
    k.fogs(wn - 0.15, 0.5 * (zm0 + zm1), 0.058);
    k.splitter(M::Trim, 0.05, 0.10, 0.025, false);
    let kt = dkey("tow_hook_front", fb, M::Caliper);
    k.front_box(&kt, 0.30, 0.36, zm0 - 0.01, zm0 + 0.03, 0.06, 0.004);

    // --- Rear: tall pedestal wing, round-element tails, mud flaps.
    k.wing(Wing {
        lead: 0.36,
        chord: 0.24,
        height: 0.19,
        half_span: 0.70,
        thick: 0.035,
        aoa_deg: 8.0,
        stand: Stand::Pedestal(0.50),
        endplates: Some((0.10, 0.24)),
        mat: M::Paint,
        stand_mat: M::Paint,
        gurney: 0.0,
    });
    for sx in SIDES {
        let tk = taillight_key(sx);
        let kh = sided(
            &dkey("taillight_housing_l", BodyRegion::RearLeftQuarter, M::Trim),
            sx,
        );
        let (z0, z1) = (tz1 - 0.14, tz1 - 0.03);
        let (xa, xb) = (sx * 0.36, sx * (wt - 0.02));
        k.rear_box(&kh, xa, xb, z0, z1, 0.012, 0.006);
        k.rear_box(
            &tk,
            xa - sx * 0.01,
            xb + sx * 0.0,
            z0 + 0.012,
            z1 - 0.012,
            0.018,
            0.006,
        );
        let kr = sided(
            &lkey(
                "taillight_round_l",
                BodyRegion::RearLeftQuarter,
                M::Taillight,
            ),
            sx,
        );
        let xm = 0.5 * (xa + xb);
        let r = 0.5 * (z1 - z0) - 0.02;
        for dx in [-0.09, 0.09] {
            k.round_lamp(&kr, false, xm + dx, 0.5 * (z0 + z1), r, 0.03, None);
        }
    }
    let kp = dkey("plate_recess", BodyRegion::Trunk, M::Trim);
    k.rear_box(&kp, -0.24, 0.24, tz1 - 0.15, tz1 - 0.04, 0.004, 0.0);
    let kv = dkey("rear_valance", BodyRegion::RearBumper, M::Trim);
    k.rear_box(
        &kv,
        -wt + 0.08,
        wt - 0.08,
        tz0 + 0.02,
        tz0 + 0.09,
        0.012,
        0.004,
    );
    k.mud_flaps(M::Trim);

    // --- Roof vent scoop (rally), skirts.
    let ry = p.roof.1 - 0.12;
    let rz = k.top_z(ry, 0.0);
    let kr = dkey("roof_scoop", BodyRegion::Roof, M::Paint);
    k.prism_x(
        &kr,
        &[
            [ry + 0.02, rz - 0.01],
            [ry - 0.20, rz - 0.01],
            [ry - 0.20, rz + 0.045],
            [ry - 0.05, rz + 0.045],
        ],
        -0.10,
        0.10,
    );
    let km = dkey("roof_scoop_mouth", BodyRegion::Roof, M::Grille);
    k.bx(
        &km,
        [-0.085, ry - 0.06, rz + 0.005],
        [0.085, ry - 0.045, rz + 0.038],
    );
    k.skirts(M::Paint, 0.025, 0.06);
    k.mirrors(0.10, 0.11, None);
    k.door_handles(M::Trim);
    k.fuel_filler(1.0, 0.10, M::Paint);
    k.wipers();
}

/// 2000s FR drift coupe: carbon vented hood, bolt-on box flares, wide lip with canards, carbon
/// GT wing on swan-neck stands, chrome-housed clear tails with round red inserts, vinyl band.
pub fn shiro(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: -0.08,
        nose_h: Some(0.62),
        nose_rake_deg: 28.0,
        nose_plan_round: 0.30,
        tail_plan_round: 0.22,
        hood_crown: 0.015,
        windshield_rake_deg: Some(60.0),
        a_pillar_rake_deg: 9.0,
        rear_glass_rake_deg: Some(62.0),
        tumblehome: Some(0.14),
        crease_z: 0.58,
        crease_depth: 0.010,
        haunch: 0.020,
        front_haunch: 0.015,
        flare: 0.040,
        flare_shape: ArchShape::Box,
        ride_dz: -0.045,
        stance_pitch_deg: 0.2,
        ducktail_h: 0.010,
        overhang_trim: (0.04, 0.0),
        belt_kick: 0.02,
        roof_len_trim: 0.15,
        ..BodyStyle::preset(Silhouette::Notchback)
    });
    s.details.push(kit_detail(shiro_kit));
    wheels(
        &mut s,
        def,
        RimKind::Mesh,
        10,
        0.74,
        M::Metal,
        CapKind::Small,
        (0.055, 0.035),
        (1.0, 3.0),
    );
    let m = &mut s.mechanical;
    m.engine = EngineKind::Inline4;
    m.induction = Induction::Turbo;
    m.show_engine_detail = true;
    m.front_suspension = SuspensionKind::Coilover;
    m.rear_suspension = SuspensionKind::Coilover;
    m.front_brakes.caliper = CaliperKind::Fixed4;
    m.caliper_rgb = Some(rgb(0xD4A017));
    m.spring_rgb = Some(rgb(0xE0301E));
    m.exhaust = ExhaustKind::Single;
    m.exhaust_tip_r_m = 0.06;
    s.tints.push((M::Paint, rgb(0xEEEDE6)));
    // Vinyl band in the accent slot (gold, matching the calipers).
    s.accent_rgb = Some(rgb(0xD4A017));
    // Cyan underglow, lit with the headlights (research §1.3 tuner neon).
    s.underglow_rgb = Some(rgb(0x18C8E8));
    s
}

fn shiro_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;

    // --- Front: angular lamps, big lower mouth, wide lip + fences + canards, tow hook.
    for sx in SIDES {
        k.top_lamp(sx, yn - 0.20, yn - 0.03, 0.46, wn - 0.07, 0.025);
        let kh = sided(
            &dkey("headlamp_housing_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        k.front_box(
            &kh,
            sx * 0.40,
            sx * (wn - 0.04),
            nz1 - 0.07,
            nz1 - 0.02,
            0.01,
            0.0,
        );
    }
    let (zm0, zm1) = (nz0 + 0.06, nz0 + 0.24);
    k.mesh_grille("lower_mouth", fb, 0.52, zm0, zm1, 3, 9, M::Trim);
    k.grille(
        "grille",
        BodyRegion::Hood,
        0.24,
        nz1 - 0.07,
        nz1 - 0.025,
        1,
        M::Trim,
        None,
    );
    k.splitter(M::Carbon, 0.09, 0.04, 0.024, true);
    k.canards(M::Carbon, zm0 + 0.05);
    let kt = dkey("tow_hook_front", fb, M::Caliper);
    k.front_box(&kt, 0.40, 0.46, zm0 - 0.02, zm0 + 0.03, 0.07, 0.004);

    // --- Carbon hood with two vents.
    let kc = dkey("carbon_hood", BodyRegion::Hood, M::Carbon);
    let hw = p.half_width(p.y_bf) - 0.10;
    k.patch(
        &kc,
        p.y_fw + 0.03,
        p.y_bf - 0.02,
        top_band(-hw, hw),
        0.004,
        0.0,
        (0, 6),
    );
    let span = p.y_bf - p.y_fw;
    for sx in SIDES {
        let name = if sx < 0.0 {
            "hood_vent_l"
        } else {
            "hood_vent_r"
        };
        k.top_vent(
            name,
            BodyRegion::Hood,
            p.y_fw + 0.45 * span,
            p.y_fw + 0.70 * span,
            (sx * 0.20).min(sx * 0.42),
            (sx * 0.20).max(sx * 0.42),
            4,
            M::Carbon,
        );
    }

    // --- Rear: GT wing on swan necks, clear tails with round red inserts, diffuser.
    k.wing(Wing {
        lead: 0.54,
        chord: 0.28,
        height: 0.24,
        half_span: 0.76,
        thick: 0.03,
        aoa_deg: 10.0,
        stand: Stand::SwanNeck(0.45),
        endplates: Some((0.17, 0.32)),
        mat: M::Carbon,
        stand_mat: M::Trim,
        gurney: 0.012,
    });
    for sx in SIDES {
        let (z0, z1) = (tz1 - 0.14, tz1 - 0.025);
        let (xa, xb) = (sx * 0.30, sx * (wt - 0.02));
        let kh = sided(
            &dkey(
                "taillight_housing_l",
                BodyRegion::RearLeftQuarter,
                M::Chrome,
            ),
            sx,
        );
        k.rear_box(&kh, xa, xb, z0, z1, 0.014, 0.008);
        let kg = sided(
            &dkey("taillight_lens_l", BodyRegion::RearLeftQuarter, M::Glass),
            sx,
        );
        k.rear_box(
            &kg,
            xa + sx * 0.012,
            xb - sx * 0.012,
            z0 + 0.01,
            z1 - 0.01,
            0.022,
            0.004,
        );
        let tk = taillight_key(sx);
        let r = 0.5 * (z1 - z0) - 0.022;
        let zc = 0.5 * (z0 + z1);
        k.round_lamp(&tk, false, sx * (wt - 0.09), zc, r, 0.034, None);
        k.round_lamp(&tk, false, sx * (wt - 0.09 - 2.3 * r), zc, r, 0.034, None);
    }
    k.diffuser(M::Carbon, 0.50, 0.12, 3, 0.04);

    // --- Body kit sides, vinyl band.
    k.skirts(M::Paint, 0.035, 0.07);
    let (yf, _, rf) = k.arch(true);
    let (yr, _, rr) = k.arch(false);
    let zv = 0.30 * p.roof.2;
    for sx in SIDES {
        let kv = dkey("vinyl_band_l", BodyRegion::LeftDoorF, M::Trim);
        k.side_patch(
            &kv,
            sx,
            yr + rr + 0.05,
            yf - rf - 0.08,
            zv + 0.10,
            zv + 0.17,
            0.004,
            0.0,
            (10, 1),
        );
        let ka = dkey("vinyl_slash_l", BodyRegion::LeftDoorF, M::Accent);
        k.side_patch(
            &ka,
            sx,
            yr + rr + 0.05,
            yf - rf - 0.40,
            zv + 0.18,
            zv + 0.21,
            0.004,
            0.0,
            (8, 1),
        );
    }
    k.mirrors(0.10, 0.10, Some(M::Carbon));
    k.door_handles(M::Trim);
    k.fuel_filler(1.0, 0.10, M::Carbon);
    k.wipers();
    let _ = tz0;
}

/// 90s twin-turbo GT liftback: bubbly smooth body, hoop wing, triple-projector lamps, big
/// central mouth, quad round tails in a body-wide black panel, fender vents.
pub fn hakone(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: -0.12,
        nose_h: Some(0.62),
        nose_rake_deg: 35.0,
        nose_plan_round: 0.40,
        tail_plan_round: 0.30,
        hood_crown: 0.025,
        hood_bulge_h: 0.015,
        hood_bulge_w: 0.60,
        windshield_rake_deg: Some(60.0),
        a_pillar_rake_deg: 12.0,
        roof_len_trim: 0.05,
        tumblehome: Some(0.20),
        crease_z: 0.50,
        crease_depth: 0.0,
        shoulder_round: 0.09,
        haunch: 0.045,
        front_haunch: 0.02,
        flare: 0.015,
        ride_dz: -0.02,
        belt_kick: 0.03,
        rear_glass_bow: 0.05,
        tail_round: 0.30,
        ..BodyStyle::preset(Silhouette::Fastback)
    });
    s.details.push(kit_detail(hakone_kit));
    wheels(
        &mut s,
        def,
        RimKind::FiveSpoke,
        5,
        0.70,
        M::Metal,
        CapKind::Centre,
        (0.03, 0.015),
        (0.0, 1.0),
    );
    let m = &mut s.mechanical;
    m.engine = EngineKind::Inline6;
    m.induction = Induction::Turbo;
    m.front_suspension = SuspensionKind::DoubleWishbone;
    m.rear_suspension = SuspensionKind::DoubleWishbone;
    m.front_brakes.caliper = CaliperKind::Fixed4;
    m.rear_brakes.caliper = CaliperKind::Fixed4;
    m.caliper_rgb = Some(rgb(0xC81E1E));
    m.exhaust = ExhaustKind::Dual;
    m.exhaust_tip_r_m = 0.045;
    s.tints.push((M::Paint, rgb(0x3B2350)));
    s
}

fn hakone_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;

    // --- Front: triple projector lamps on the nose top, big rounded central mouth.
    for sx in SIDES {
        k.top_lamp(sx, yn - 0.20, yn - 0.03, 0.44, wn - 0.07, 0.025);
        let kp = sided(
            &lkey(
                "headlamp_projector_l",
                BodyRegion::FrontLeftFender,
                M::Headlight,
            ),
            sx,
        );
        let kr = sided(
            &dkey(
                "headlamp_projector_ring_l",
                BodyRegion::FrontLeftFender,
                M::Chrome,
            ),
            sx,
        );
        for i in 0..3 {
            let x = sx * (0.48 + 0.11 * i as f32).min(wn - 0.09);
            k.round_lamp(&kp, true, x, nz1 - 0.05, 0.03, 0.024, Some(&kr));
        }
    }
    let (zm0, zm1) = (nz0 + 0.06, nz0 + 0.27);
    k.grille(
        "lower_mouth",
        fb,
        0.45,
        zm0,
        zm1,
        2,
        M::Paint,
        Some(M::Trim),
    );
    let ki = sided(&dkey("brake_duct_l", fb, M::Grille), -1.0);
    for sx in SIDES {
        let ki = sided(&ki, sx);
        k.front_box(
            &ki,
            sx * (wn - 0.24),
            sx * (wn - 0.10),
            zm0 + 0.02,
            zm0 + 0.10,
            0.006,
            0.0,
        );
        let ka = sided(&lkey("indicator_front_l", fb, M::EmissiveAmber), sx);
        k.front_box(
            &ka,
            sx * (wn - 0.24),
            sx * (wn - 0.10),
            zm0 + 0.12,
            zm0 + 0.16,
            0.014,
            0.004,
        );
    }
    k.splitter(M::Trim, 0.03, 0.14, 0.02, false);

    // --- Fender vents behind the front wheels.
    let (yf, zf, rf) = k.arch(true);
    for sx in SIDES {
        let kv = dkey("fender_vent_l", BodyRegion::FrontLeftFender, M::Grille);
        let (y0, y1) = (yf - rf - 0.18, yf - rf - 0.04);
        let z0 = zf + 0.10;
        k.side_patch(&kv, sx, y0, y1, z0, z0 + 0.09, 0.002, 0.008, (2, 1));
        let ks = dkey("fender_vent_slats_l", BodyRegion::FrontLeftFender, M::Paint);
        for dz in [0.03, 0.06] {
            k.side_patch(
                &ks,
                sx,
                y0,
                y1,
                z0 + dz - 0.006,
                z0 + dz + 0.006,
                0.004,
                0.012,
                (2, 1),
            );
        }
    }

    // --- Rear: hoop wing, quad round tails in a black body-wide panel, centre garnish.
    k.hoop_wing(0.42, 0.20, 0.12, M::Paint);
    let (pz0, pz1) = (tz1 - 0.20, tz1 - 0.025);
    k.rear_panel(
        "tail_panel",
        BodyRegion::Trunk,
        wt - 0.03,
        pz0,
        pz1,
        M::Grille,
    );
    let zc = 0.5 * (pz0 + pz1);
    let r = (0.5 * (pz1 - pz0) - 0.02).min(0.068);
    for sx in SIDES {
        let tk = taillight_key(sx);
        let kr = sided(
            &dkey("taillight_ring_l", BodyRegion::RearLeftQuarter, M::Chrome),
            sx,
        );
        for x in [wt - 0.12, wt - 0.12 - 2.45 * r] {
            k.round_lamp(&tk, false, sx * x, zc, r, 0.03, Some(&kr));
        }
    }
    let kg = dkey("tail_garnish", BodyRegion::Trunk, M::Chrome);
    k.rear_box(&kg, -0.22, 0.22, zc - 0.012, zc + 0.012, 0.016, 0.004);
    let kv = dkey("rear_valance", BodyRegion::RearBumper, M::Trim);
    k.rear_box(&kv, -0.55, 0.55, tz0 + 0.02, tz0 + 0.08, 0.012, 0.004);

    k.mirrors(0.12, 0.11, None);
    k.door_handles(M::Trim);
    k.fuel_filler(-1.0, 0.11, M::Paint);
    k.wipers();
    let _ = (p, PatchBand::Top { x0: 0.0, x1: 0.0 });
}

/// 2000s German I6 sport sedan: crisp notchback, power dome, split grille, fender gills,
/// boot lip, L-shaped wrap-around tails, carbon roof, diffuser + quad tips.
pub fn sperber(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: -0.10,
        nose_h: Some(0.68),
        nose_rake_deg: 12.0,
        nose_plan_round: 0.18,
        tail_plan_round: 0.20,
        hood_crown: 0.015,
        hood_bulge_h: 0.020,
        hood_bulge_w: 0.35,
        windshield_rake_deg: Some(59.0),
        a_pillar_rake_deg: 8.0,
        rear_glass_rake_deg: Some(56.0),
        roof_dz: -0.01,
        tumblehome: Some(0.15),
        crease_z: 0.62,
        crease_depth: 0.015,
        haunch: 0.020,
        front_haunch: 0.010,
        flare: 0.015,
        ride_dz: -0.015,
        ducktail_h: 0.020,
        belt_kick: 0.03,
        roof_len_trim: 0.12,
        ..BodyStyle::preset(Silhouette::Notchback)
    });
    s.details.push(kit_detail(sperber_kit));
    wheels(
        &mut s,
        def,
        RimKind::MultiSpoke,
        10,
        0.76,
        M::Metal,
        CapKind::Centre,
        (0.03, 0.015),
        (0.0, 0.5),
    );
    let m = &mut s.mechanical;
    m.engine = EngineKind::Inline6;
    m.induction = Induction::Stacks;
    m.front_suspension = SuspensionKind::Strut;
    m.rear_suspension = SuspensionKind::Multilink;
    m.caliper_rgb = Some(rgb(0x7A8088));
    m.exhaust = ExhaustKind::Quad;
    m.exhaust_tip_r_m = 0.042;
    s.tints.push((M::Paint, rgb(0x24407A)));
    s
}

fn sperber_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yt = k.yt;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (_, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;

    // --- Front: split grille (two chrome-framed halves), twin round lamps, 3-part intake.
    for sx in SIDES {
        let name = if sx < 0.0 { "grille_l" } else { "grille_r" };
        let g = dkey(name, BodyRegion::Hood, M::Grille);
        let (x0, x1) = (sx * 0.035, sx * 0.25);
        k.front_box(&g, x0, x1, nz1 - 0.12, nz1 - 0.03, 0.008, 0.012);
        let f = sided(&dkey("grille_frame_l", BodyRegion::Hood, M::Chrome), sx);
        k.front_box(
            &f,
            x0 - sx * 0.012,
            x1 + sx * 0.012,
            nz1 - 0.132,
            nz1 - 0.12,
            0.016,
            0.0,
        );
        k.front_box(
            &f,
            x0 - sx * 0.012,
            x1 + sx * 0.012,
            nz1 - 0.03,
            nz1 - 0.018,
            0.016,
            0.0,
        );
        let s2 = sided(&dkey("grille_slats_l", BodyRegion::Hood, M::Chrome), sx);
        for i in 1..5 {
            let x = x0 + (x1 - x0) * i as f32 / 5.0;
            k.front_box(
                &s2,
                x - 0.005,
                x + 0.005,
                nz1 - 0.12,
                nz1 - 0.03,
                0.014,
                0.0,
            );
        }
        let hk = headlight_key(sx);
        let kh = sided(
            &dkey("headlamp_housing_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        k.front_box(
            &kh,
            sx * 0.32,
            sx * (wn - 0.04),
            nz1 - 0.13,
            nz1 - 0.02,
            0.01,
            0.01,
        );
        let r = 0.042;
        for x in [0.42, (wn - 0.10).max(0.53)] {
            k.round_lamp(&hk, true, sx * x, nz1 - 0.075, r, 0.024, None);
        }
        let kd = sided(&dkey("side_duct_l", fb, M::Grille), sx);
        k.front_box(
            &kd,
            sx * (wn - 0.28),
            sx * (wn - 0.08),
            nz0 + 0.08,
            nz0 + 0.20,
            0.006,
            0.006,
        );
    }
    k.mesh_grille(
        "lower_intake",
        fb,
        0.38,
        nz0 + 0.06,
        nz0 + 0.22,
        2,
        6,
        M::Trim,
    );
    k.splitter(M::Trim, 0.035, 0.14, 0.02, false);

    // --- Fender gills behind the front wheels.
    let (yf, zf, rf) = k.arch(true);
    for sx in SIDES {
        let (y0, y1) = (yf - rf - 0.20, yf - rf - 0.05);
        let z0 = zf + 0.14;
        let kv = dkey("fender_gill_l", BodyRegion::FrontLeftFender, M::Grille);
        k.side_patch(&kv, sx, y0, y1, z0, z0 + 0.07, 0.002, 0.008, (2, 1));
        let ks = dkey(
            "fender_gill_slats_l",
            BodyRegion::FrontLeftFender,
            M::Chrome,
        );
        for dz in [0.023, 0.047] {
            k.side_patch(
                &ks,
                sx,
                y0,
                y1,
                z0 + dz - 0.005,
                z0 + dz + 0.005,
                0.004,
                0.012,
                (2, 1),
            );
        }
    }

    // --- Rear: L-shaped tails wrapping onto the quarters, boot lip, diffuser, carbon roof.
    for sx in SIDES {
        let tk = taillight_key(sx);
        let kh = sided(
            &dkey("taillight_housing_l", BodyRegion::RearLeftQuarter, M::Trim),
            sx,
        );
        let (z0, z1) = (tz1 - 0.15, tz1 - 0.03);
        k.rear_box(&kh, sx * 0.34, sx * (wt + 0.005), z0, z1, 0.012, 0.006);
        // L: a bar along the top plus a leg down the outer end.
        k.rear_box(
            &tk,
            sx * 0.36,
            sx * (wt - 0.01),
            z1 - 0.05,
            z1 - 0.012,
            0.026,
            0.006,
        );
        k.rear_box(
            &tk,
            sx * (wt - 0.13),
            sx * (wt - 0.01),
            z0 + 0.012,
            z1 - 0.05,
            0.026,
            0.006,
        );
        let kw = PartKey {
            kind: PartKind::Light,
            ..dkey(
                "taillight_wrap_l",
                BodyRegion::RearLeftQuarter,
                M::Taillight,
            )
        };
        k.side_patch(
            &kw,
            sx,
            yt + 0.01,
            yt + 0.14,
            z1 - 0.05,
            z1 - 0.014,
            0.002,
            0.012,
            (2, 1),
        );
        let kr = sided(
            &lkey("reverse_lamp_l", BodyRegion::RearLeftQuarter, M::Headlight),
            sx,
        );
        k.rear_box(
            &kr,
            sx * 0.40,
            sx * 0.52,
            z0 + 0.02,
            z1 - 0.065,
            0.018,
            0.004,
        );
    }
    k.lip_spoiler(M::Paint, yt + 0.20, 0.022, 0.05, 0.22);
    k.diffuser(M::Trim, 0.55, 0.13, 4, 0.04);
    let (r0, r1, _, _) = p.roof;
    let hw = k.roof_edge(0.5 * (r0 + r1)) - 0.01;
    k.top_stripe(
        "carbon_roof",
        BodyRegion::Roof,
        r0 + 0.02,
        r1 - 0.02,
        -hw,
        hw,
        M::Carbon,
    );

    k.skirts(M::Paint, 0.02, 0.05);
    k.mirrors(0.10, 0.11, Some(M::Carbon));
    k.door_handles(M::Chrome);
    k.fuel_filler(1.0, 0.10, M::Paint);
    k.wipers();
}

/// Sport coupe (the old worked example): fastback with splitter, slim LED lamps with DRL,
/// hood vents, ducktail, full-width rear light bar with thick ends, diffuser.
pub fn debug_coupe(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: -0.10,
        nose_h: Some(0.64),
        nose_rake_deg: 25.0,
        nose_plan_round: 0.30,
        tail_plan_round: 0.22,
        hood_crown: 0.020,
        hood_bulge_h: 0.015,
        hood_bulge_w: 0.40,
        windshield_rake_deg: Some(60.0),
        a_pillar_rake_deg: 10.0,
        roof_len_trim: 0.05,
        roof_dz: -0.02,
        tumblehome: Some(0.18),
        crease_z: 0.60,
        crease_depth: 0.012,
        haunch: 0.035,
        flare: 0.020,
        ride_dz: -0.01,
        ducktail_h: 0.035,
        belt_kick: 0.03,
        ..BodyStyle::preset(Silhouette::Fastback)
    });
    s.details.push(kit_detail(debug_coupe_kit));
    wheels(
        &mut s,
        def,
        RimKind::SplitSpoke,
        5,
        0.74,
        M::Engine,
        CapKind::Centre,
        (0.035, 0.015),
        (0.0, 0.5),
    );
    let m = &mut s.mechanical;
    m.engine = EngineKind::Inline6;
    m.induction = Induction::Turbo;
    m.layout = EngineLayout::Front;
    m.front_brakes.caliper = CaliperKind::Fixed4;
    m.caliper_rgb = Some(rgb(0xC81E1E));
    m.exhaust = ExhaustKind::Quad;
    m.exhaust_tip_r_m = 0.045;
    s.tints.push((M::Paint, rgb(0x3A3F45)));
    s
}

fn debug_coupe_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;

    for sx in SIDES {
        k.top_lamp(sx, yn - 0.17, yn - 0.03, 0.48, wn - 0.07, 0.022);
        let drl = sided(
            &lkey("drl_l", BodyRegion::FrontLeftFender, M::Headlight),
            sx,
        );
        k.front_box(
            &drl,
            sx * 0.46,
            sx * (wn - 0.06),
            nz1 - 0.06,
            nz1 - 0.044,
            0.016,
            0.0,
        );
        let kh = sided(
            &dkey("headlamp_housing_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        k.front_box(
            &kh,
            sx * 0.44,
            sx * (wn - 0.04),
            nz1 - 0.08,
            nz1 - 0.02,
            0.008,
            0.0,
        );
    }
    k.grille(
        "grille",
        BodyRegion::Hood,
        0.30,
        nz1 - 0.08,
        nz1 - 0.03,
        1,
        M::Trim,
        None,
    );
    k.mesh_grille(
        "lower_mouth",
        fb,
        0.55,
        nz0 + 0.06,
        nz0 + 0.24,
        3,
        8,
        M::Trim,
    );
    k.splitter(M::Carbon, 0.07, 0.10, 0.02, false);
    let span = yn - p.y_fw;
    for sx in SIDES {
        let name = if sx < 0.0 {
            "hood_vent_l"
        } else {
            "hood_vent_r"
        };
        let (a, c) = ((sx * 0.22).min(sx * 0.38), (sx * 0.22).max(sx * 0.38));
        k.top_vent(
            name,
            BodyRegion::Hood,
            yn - 0.50 * span,
            yn - 0.28 * span,
            a,
            c,
            4,
            M::Trim,
        );
    }

    // Full-width light bar with thicker ends.
    let z = tz1 - 0.06;
    let kb = lkey("taillight_bar", BodyRegion::Trunk, M::Taillight);
    k.rear_box(&kb, -wt + 0.30, wt - 0.30, z - 0.016, z + 0.016, 0.022, 0.0);
    for sx in SIDES {
        let tk = taillight_key(sx);
        k.rear_box(
            &tk,
            sx * (wt - 0.32),
            sx * (wt - 0.02),
            z - 0.045,
            z + 0.035,
            0.026,
            0.008,
        );
        let kh = sided(
            &dkey("taillight_housing_l", BodyRegion::RearLeftQuarter, M::Trim),
            sx,
        );
        k.rear_box(
            &kh,
            sx * (wt - 0.34),
            sx * (wt - 0.0),
            z - 0.06,
            z + 0.05,
            0.012,
            0.006,
        );
    }
    k.diffuser(M::Carbon, 0.55, 0.14, 3, 0.05);
    k.skirts(M::Carbon, 0.03, 0.05);
    k.mirrors(0.10, 0.11, Some(M::Carbon));
    k.door_handles(M::Trim);
    k.fuel_filler(1.0, 0.10, M::Paint);
    k.wipers();
    let _ = tz0;
}
