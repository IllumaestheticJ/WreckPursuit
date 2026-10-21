//! Muscle: `ironside_440` (early-70s big-block coupe) and `rampart_50` (2010s muscle coupe).
//!
//! Genre cues (research §3): long hood + set-back cabin, nose-down rake with fat rears, hood
//! bulge / scoop, wide horizontal (70s) or vertical-bar (2010s) tail lamps with twin / quad
//! tips, chrome bumper blades (70s) or contrast stripes (2010s).

use wp_contracts::{BodyRegion, VehicleDef};

use crate::builder::{Builder, PartKey, PatchBand};
use crate::mesh::MaterialSlot as M;
use crate::style::{
    ArchShape, BodyStyle, CaliperKind, CapKind, CarStyle, DetailCtx, EngineKind, ExhaustKind,
    HoodScoop, Induction, RimKind, Silhouette, SuspensionKind,
};

use super::kit::{dkey, headlight_key, lkey, sided, taillight_key, Kit, SIDES};
use super::{kit_detail, rgb, wheels};

/// 70s big-block coupe: long flat slab, cowl scoop, chrome blades, quad round headlamps,
/// wide recessed tail lamps, vinyl roof + side stripe.
pub fn ironside(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: -0.35,
        nose_h: Some(0.72),
        nose_rake_deg: 3.0,
        nose_plan_round: 0.10,
        tail_plan_round: 0.12,
        hood_crown: 0.010,
        hood_bulge_h: 0.0,
        hood_bulge_w: 0.40,
        power_bulge_h: 0.035,
        hood_scoop: HoodScoop::Cowl,
        hood_scoop_h: 0.05,
        windshield_rake_deg: Some(58.0),
        a_pillar_rake_deg: 6.0,
        rear_glass_rake_deg: Some(60.0),
        roof_len_trim: 0.10,
        roof_dz: -0.02,
        tumblehome: Some(0.16),
        crease_z: 0.56,
        crease_depth: 0.012,
        shoulder_round: 0.03,
        haunch: 0.035,
        flare: 0.010,
        stance_pitch_deg: 0.6,
        ducktail_h: 0.03,
        tail_round: 0.14,
        tail_drop: 0.03,
        belt_kick: 0.06,
        ..BodyStyle::preset(Silhouette::Notchback)
    });
    s.details.push(kit_detail(ironside_kit));
    wheels(
        &mut s,
        def,
        RimKind::SlotMag,
        5,
        0.55,
        M::Chrome,
        CapKind::Centre,
        (0.03, 0.02),
        (0.0, 0.0),
    );
    let m = &mut s.mechanical;
    m.engine = EngineKind::V8;
    m.induction = Induction::Natural;
    m.show_engine_detail = true;
    m.front_suspension = SuspensionKind::DoubleWishbone;
    m.rear_suspension = SuspensionKind::SolidAxleLeaf;
    m.front_brakes.caliper = CaliperKind::Floating;
    m.rear_brakes.caliper = CaliperKind::Drum;
    m.exhaust = ExhaustKind::Dual;
    m.exhaust_tip_r_m = 0.038;
    s.tints.push((M::Paint, rgb(0x5B1E5A)));
    s
}

fn ironside_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let (yn, yt) = (k.yn, k.yt);
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;
    let rb = BodyRegion::RearBumper;

    // --- Front: chrome blade, full-width recessed grille with quad round lamps, valance.
    let zb = nz0 + 0.42 * (nz1 - nz0);
    let blade = dkey("bumper_blade_front", fb, M::Chrome);
    k.front_box(
        &blade,
        -wn - 0.015,
        wn + 0.015,
        zb - 0.06,
        zb + 0.035,
        0.07,
        0.018,
    );
    for sx in SIDES {
        let w = p.half_width(yn - 0.18) + 0.01;
        k.bbx(
            &blade,
            [sx * (w - 0.03), yn - 0.30, zb - 0.06],
            [sx * (w + 0.025), yn + 0.02, zb + 0.035],
            0.012,
        );
    }
    let gz0 = zb + 0.045;
    let gz1 = nz1 - 0.025;
    k.grille(
        "grille",
        fb,
        wn - 0.06,
        gz0,
        gz1,
        4,
        M::Chrome,
        Some(M::Chrome),
    );
    let zl = 0.5 * (gz0 + gz1);
    let r = (0.5 * (gz1 - gz0) - 0.01).min(0.08);
    for sx in SIDES {
        let bez = sided(
            &dkey("headlamp_bezel_l", BodyRegion::FrontLeftFender, M::Chrome),
            sx,
        );
        for x in [wn - 0.18, wn - 0.18 - 2.3 * r] {
            k.round_lamp(&headlight_key(sx), true, sx * x, zl, r, 0.03, Some(&bez));
        }
    }
    let val = dkey("front_valance", fb, M::Grille);
    k.front_box(&val, -0.55, 0.55, nz0 + 0.05, zb - 0.08, 0.005, 0.0);
    for sx in SIDES {
        let ka = sided(&lkey("indicator_front_l", fb, M::EmissiveAmber), sx);
        k.front_box(
            &ka,
            sx * 0.62,
            sx * 0.80,
            nz0 + 0.07,
            zb - 0.09,
            0.012,
            0.004,
        );
    }
    k.hood_pins(yn - 0.16, 0.55);

    // --- Rear: chrome blade, recessed black lamp panel with two wide lamps per side.
    let zbr = tz0 + 0.36 * (tz1 - tz0);
    let rblade = dkey("bumper_blade_rear", rb, M::Chrome);
    k.rear_box(
        &rblade,
        -wt - 0.015,
        wt + 0.015,
        zbr - 0.055,
        zbr + 0.035,
        0.065,
        0.018,
    );
    for sx in SIDES {
        let w = p.half_width(yt + 0.18) + 0.01;
        k.bbx(
            &rblade,
            [sx * (w - 0.03), yt + 0.28, zbr - 0.055],
            [sx * (w + 0.025), yt - 0.02, zbr + 0.035],
            0.012,
        );
    }
    let pz0 = zbr + 0.05;
    let pz1 = tz1 - 0.03;
    k.rear_panel(
        "tail_panel",
        BodyRegion::Trunk,
        wt - 0.03,
        pz0,
        pz1,
        M::Grille,
    );
    for sx in SIDES {
        let tk = taillight_key(sx);
        let kb = sided(
            &dkey("taillight_bezel_l", BodyRegion::RearLeftQuarter, M::Chrome),
            sx,
        );
        let (xa, xb) = (sx * 0.17, sx * (wt - 0.07));
        k.rear_box(&kb, xa, xb, pz0 + 0.012, pz1 - 0.012, 0.018, 0.006);
        // Three segments per side (reads as one wide lamp with chrome dividers).
        let n = 3;
        for i in 0..n {
            let f0 = i as f32 / n as f32;
            let f1 = (i + 1) as f32 / n as f32;
            let x0 = xa + (xb - xa) * f0 + sx * 0.008;
            let x1 = xa + (xb - xa) * f1 - sx * 0.008;
            k.rear_box(&tk, x0, x1, pz0 + 0.025, pz1 - 0.025, 0.026, 0.004);
        }
    }
    let kr = dkey("reverse_lamps", BodyRegion::Trunk, M::Headlight);
    k.rear_box(&kr, -0.12, 0.12, pz0 + 0.03, pz1 - 0.03, 0.02, 0.004);

    // --- Sides: chrome rocker strip, black hockey-stick stripe, vinyl roof.
    let (yf, _, rf) = k.arch(true);
    let (yr, _, rr) = k.arch(false);
    let zr = p.z_door_bottom + 0.03;
    for sx in SIDES {
        let kc = dkey("rocker_trim_l", BodyRegion::FrameLeft, M::Chrome);
        k.side_patch(
            &kc,
            sx,
            yr + rr + 0.04,
            yf - rf - 0.04,
            zr,
            zr + 0.03,
            0.002,
            0.008,
            (8, 1),
        );
        let ks = dkey("side_stripe_l", BodyRegion::LeftDoorF, M::Trim);
        let zc = 0.56 * p.roof.2;
        k.side_patch(
            &ks,
            sx,
            yr + 0.15,
            yf - rf - 0.10,
            zc + 0.03,
            zc + 0.08,
            0.004,
            0.0,
            (14, 1),
        );
        let kh = dkey("side_stripe_kick_l", BodyRegion::RearLeftQuarter, M::Trim);
        k.side_patch(
            &kh,
            sx,
            yr + 0.05,
            yr + 0.15,
            zc + 0.03,
            p.belt(yr + 0.1) - 0.04,
            0.004,
            0.0,
            (2, 2),
        );
    }
    let (r0, r1, _, _) = p.roof;
    let hw = k.roof_edge(0.5 * (r0 + r1)) - 0.01;
    k.top_stripe(
        "vinyl_roof",
        BodyRegion::Roof,
        r0 + 0.02,
        r1 - 0.02,
        -hw,
        hw,
        M::Trim,
    );

    k.mirrors(0.12, 0.13, None);
    k.door_handles(M::Chrome);
    k.fuel_filler(1.0, 0.12, M::Chrome);
    k.wipers();
    // Chrome window surround on the deck edge (rear glass base).
    let kt = dkey("deck_trim", BodyRegion::Trunk, M::Chrome);
    let yd = p.y_deck - 0.01;
    let hwd = p.half_width(yd) - 0.20;
    k.patch(
        &kt,
        yd - 0.025,
        yd,
        PatchBand::Top { x0: -hwd, x1: hwd },
        0.002,
        0.006,
        (1, 4),
    );
}

/// 2010s muscle coupe: shark nose, slim lamps, hood vents, splitter, triple vertical tail
/// bars in a black panel, diffuser, twin over-the-top stripes.
pub fn rampart(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        silhouette: Silhouette::Fastback,
        cabin_shift_m: -0.25,
        nose_h: Some(0.70),
        nose_rake_deg: 6.0,
        nose_plan_round: 0.22,
        tail_plan_round: 0.18,
        hood_crown: 0.020,
        hood_bulge_h: 0.030,
        hood_bulge_w: 0.30,
        windshield_rake_deg: Some(63.0),
        a_pillar_rake_deg: 10.0,
        roof_len_trim: -0.10,
        roof_dz: -0.05,
        belt_dz: 0.03,
        tumblehome: Some(0.20),
        crease_z: 0.60,
        crease_depth: 0.015,
        haunch: 0.050,
        flare: 0.020,
        flare_shape: ArchShape::Round,
        ride_dz: -0.01,
        stance_pitch_deg: 0.2,
        ducktail_h: 0.025,
        overhang_trim: (0.0, -0.03),
        belt_kick: 0.04,
        rear_glass_bow: 0.03,
        tail_round: 0.18,
        tail_drop: 0.04,
        ..BodyStyle::preset(Silhouette::Fastback)
    });
    s.details.push(kit_detail(rampart_kit));
    wheels(
        &mut s,
        def,
        RimKind::SplitSpoke,
        5,
        0.74,
        M::Engine,
        CapKind::Centre,
        (0.035, 0.015),
        (0.0, 0.0),
    );
    let m = &mut s.mechanical;
    m.engine = EngineKind::V8;
    m.show_engine_detail = true;
    m.front_suspension = SuspensionKind::Strut;
    m.rear_suspension = SuspensionKind::Multilink;
    m.front_brakes.caliper = CaliperKind::Fixed6;
    m.front_brakes.drilled = true;
    m.front_brakes.disc_radius_frac = 0.88;
    m.caliper_rgb = Some(rgb(0xC81E1E));
    m.exhaust = ExhaustKind::Quad;
    m.exhaust_tip_r_m = 0.045;
    s.tints.push((M::Paint, rgb(0xC1121C)));
    s
}

fn rampart_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let (yn, yt) = (k.yn, k.yt);
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;

    // --- Front: shark-nose upper grille, lower mouth, splitter, slim lamps with DRL.
    k.mesh_grille("grille", fb, 0.50, nz1 - 0.17, nz1 - 0.03, 3, 7, M::Trim);
    let frame = dkey("grille_frame", fb, M::Trim);
    k.front_box(&frame, -0.53, 0.53, nz1 - 0.035, nz1 - 0.012, 0.02, 0.005);
    k.mesh_grille(
        "lower_mouth",
        fb,
        0.62,
        nz0 + 0.07,
        nz0 + 0.20,
        2,
        9,
        M::Trim,
    );
    k.splitter(M::Carbon, 0.06, 0.12, 0.022, false);
    for sx in SIDES {
        // Lens across the nose top edge, plus the DRL blade on the face.
        k.top_lamp(sx, yn - 0.20, yn - 0.03, 0.58, wn - 0.07, 0.025);
        let drl = sided(
            &lkey("drl_l", BodyRegion::FrontLeftFender, M::Headlight),
            sx,
        );
        k.front_box(
            &drl,
            sx * 0.56,
            sx * (wn - 0.06),
            nz1 - 0.05,
            nz1 - 0.032,
            0.018,
            0.0,
        );
        let ki = sided(
            &dkey("headlamp_housing_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        k.front_box(
            &ki,
            sx * 0.54,
            sx * (wn - 0.04),
            nz1 - 0.13,
            nz1 - 0.05,
            0.008,
            0.0,
        );
        let fog = sided(&lkey("fog_l", fb, M::Headlight), sx);
        k.front_box(
            &fog,
            sx * (wn - 0.22),
            sx * (wn - 0.10),
            nz0 + 0.10,
            nz0 + 0.15,
            0.014,
            0.004,
        );
    }
    // Hood heat-extractor vents beside the centre bulge.
    let span = yn - p.y_fw;
    for sx in SIDES {
        let (x0, x1) = (sx * 0.24, sx * 0.40);
        k.top_vent(
            if sx < 0.0 {
                "hood_vent_l"
            } else {
                "hood_vent_r"
            },
            BodyRegion::Hood,
            yn - 0.44 * span,
            yn - 0.24 * span,
            x0.min(x1),
            x0.max(x1),
            4,
            M::Trim,
        );
    }

    // --- Rear: black lamp panel, three vertical bars per side, diffuser, lip spoiler.
    let pz0 = tz1 - 0.19;
    let pz1 = tz1 - 0.025;
    k.rear_panel(
        "tail_panel",
        BodyRegion::Trunk,
        wt - 0.04,
        pz0,
        pz1,
        M::Grille,
    );
    for sx in SIDES {
        let tk = taillight_key(sx);
        for i in 0..3 {
            let xc = 0.40 + 0.13 * i as f32;
            k.rear_box(
                &tk,
                sx * (xc - 0.04),
                sx * (xc + 0.04),
                pz0 + 0.02,
                pz1 - 0.02,
                0.026,
                0.006,
            );
        }
    }
    k.diffuser(M::Carbon, 0.66, 0.15, 4, 0.05);
    k.lip_spoiler(M::Paint, yt + 0.22, 0.035, 0.08, 0.15);
    let kr = dkey("rear_valance", BodyRegion::RearBumper, M::Grille);
    k.rear_box(
        &kr,
        -wt + 0.06,
        wt - 0.06,
        tz0 + 0.17,
        tz0 + 0.20,
        0.008,
        0.0,
    );

    // --- Twin over-the-top stripes (hood, roof, deck) + skirts.
    let (r0, r1, _, _) = p.roof;
    for sx in SIDES {
        let (x0, x1) = (sx * 0.07, sx * 0.27);
        let (a, c) = (x0.min(x1), x0.max(x1));
        let n = if sx < 0.0 { "l" } else { "r" };
        k.top_stripe(
            &format!("stripe_hood_{n}"),
            BodyRegion::Hood,
            p.y_fw + 0.04,
            yn - 0.04,
            a,
            c,
            M::Trim,
        );
        k.top_stripe(
            &format!("stripe_roof_{n}"),
            BodyRegion::Roof,
            r0 + 0.02,
            r1 - 0.02,
            a,
            c,
            M::Trim,
        );
        k.top_stripe(
            &format!("stripe_deck_{n}"),
            BodyRegion::Trunk,
            yt + 0.10,
            p.y_deck - 0.02,
            a,
            c,
            M::Trim,
        );
    }
    k.skirts(M::Carbon, 0.03, 0.06);
    k.mirrors(0.10, 0.12, None);
    k.door_handles(M::Trim);
    k.fuel_filler(-1.0, 0.11, M::Paint);
    k.wipers();
}
