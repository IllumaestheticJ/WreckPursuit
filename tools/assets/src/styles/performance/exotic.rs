//! Mid-engine: `lucciola_350` (90s doorstop wedge exotic) and `fulmine_v10` (2010s V10
//! supercar).
//!
//! Both lattices carry a front-engine cabin, so the cab-forward look comes from
//! `cabin_shift_m` (+0.40 / +0.45 m forward: roof centre ≈ 46 % of the length from the nose,
//! windshield base ≈ 18–19 %) and a steeper rear glass toward the engine deck. Genre cues
//! (research §3): knife-edge wedge, louvred engine deck, quad round tails in a black panel,
//! side strakes, pop-up lamps (exotic); low wide cab-forward wedge, giant side intakes, thin
//! light-bar tails + black mesh, big diffuser + wing (supercar).

use wp_contracts::{BodyRegion, VehicleDef};

use crate::builder::{Builder, PartKey};
use crate::mesh::{MaterialSlot as M, PartKind, INDICATOR_SEAM_M};
use crate::style::{
    ArchShape, BodyStyle, CaliperKind, CapKind, CarStyle, DetailCtx, EngineKind, EngineLayout,
    ExhaustKind, Induction, RimKind, Silhouette, SuspensionKind,
};

use super::kit::{
    dkey, indicator_front_key, indicator_rear_key, lkey, sided, taillight_key, top_band, Kit, Seg,
    Stand, Wing, SIDES,
};
use super::{kit_detail, rgb, wheels};

/// Fraction `f` of the body length measured from the nose → chassis y.
fn at(k: &Kit, f: f32) -> f32 {
    k.yn - f * (k.yn - k.yt)
}

/// 90s mid-engine doorstop wedge: closed pop-up lamp pods, slatted nose, side intake with
/// strakes, louvred deck, quad round tails (outer amber-ringed) in a black rear mesh panel.
pub fn lucciola(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: 0.40,
        nose_h: Some(0.55),
        nose_rake_deg: 55.0,
        nose_plan_round: 0.15,
        tail_plan_round: 0.12,
        hood_crown: 0.010,
        windshield_rake_deg: Some(64.0),
        a_pillar_rake_deg: 14.0,
        rear_glass_rake_deg: Some(65.0),
        roof_len_trim: 0.15,
        tumblehome: Some(0.24),
        crease_z: 0.50,
        crease_depth: 0.020,
        haunch: 0.040,
        flare: 0.015,
        flare_shape: ArchShape::Box,
        ride_dz: -0.01,
        stance_pitch_deg: 0.3,
        ducktail_h: -0.02,
        overhang_trim: (0.0, -0.03),
        belt_kick: 0.06,
        belt_rake: 0.04,
        tail_round: 0.08,
        tail_drop: 0.02,
        ..BodyStyle::preset(Silhouette::Wedge)
    });
    s.details.push(kit_detail(lucciola_kit));
    wheels(
        &mut s,
        def,
        RimKind::FiveSpoke,
        5,
        0.72,
        M::Metal,
        CapKind::Centre,
        (0.025, 0.012),
        (0.0, 0.5),
    );
    let m = &mut s.mechanical;
    m.engine = EngineKind::V8;
    m.layout = EngineLayout::Mid;
    m.induction = Induction::Natural;
    m.show_engine_detail = true;
    m.front_suspension = SuspensionKind::Coilover;
    m.rear_suspension = SuspensionKind::Coilover;
    m.front_brakes.caliper = CaliperKind::Fixed4;
    m.front_brakes.drilled = true;
    m.rear_brakes.caliper = CaliperKind::Fixed4;
    m.rear_brakes.drilled = true;
    m.caliper_rgb = Some(rgb(0xC81E1E));
    m.exhaust = ExhaustKind::CentreQuad;
    m.exhaust_tip_r_m = 0.05;
    s.tints.push((M::Paint, rgb(0xC4161C)));
    s
}

fn lucciola_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;
    let hood = BodyRegion::Hood;

    // --- Pop-up lamp pods (closed): raised paint lids with a dark seam, on the hood top.
    for sx in SIDES {
        let (y0, y1) = (yn - 0.56, yn - 0.26);
        let (x0, x1) = (sx * 0.38, sx * (wn - 0.12));
        let ks = sided(&dkey("popup_seam_l", hood, M::Grille), sx);
        k.patch(
            &ks,
            y0 - 0.012,
            y1 + 0.012,
            top_band(x0 - sx * 0.012, x1 + sx * 0.012),
            0.002,
            0.004,
            (2, 2),
        );
        let kl = sided(&dkey("popup_lid_l", hood, M::Paint), sx);
        k.patch(&kl, y0, y1, top_band(x0, x1), 0.004, 0.012, (3, 2));
        // Flush parking / flash lamp strip in the nose face below the pods, its outer corner
        // the amber turn signal, which wraps round onto the fender side.
        k.front_cluster(
            sx,
            0.40,
            wn - 0.02,
            nz1 - 0.06,
            nz1 - 0.02,
            0.012,
            0.006,
            Seg::Outboard(0.11),
        );
        k.side_patch(
            &indicator_front_key(-1.0),
            sx,
            yn - 0.10,
            yn - 0.025,
            nz1 - 0.06,
            nz1 - 0.02,
            0.002,
            0.008,
            (2, 1),
        );
    }
    // --- Black slatted nose intake + small side ducts, lip.
    let (zi0, zi1) = (nz0 + 0.04, nz0 + 0.19);
    k.grille("nose_intake", fb, 0.58, zi0, zi1, 4, M::Trim, None);
    for sx in SIDES {
        let kd = sided(&dkey("nose_duct_l", fb, M::Grille), sx);
        k.front_box(
            &kd,
            sx * 0.64,
            sx * (wn - 0.06),
            zi0 + 0.02,
            zi1 - 0.03,
            0.006,
            0.0,
        );
    }
    k.splitter(M::Trim, 0.03, 0.12, 0.018, false);
    // Small hood vents ahead of the windshield.
    for sx in SIDES {
        let name = if sx < 0.0 {
            "hood_vent_l"
        } else {
            "hood_vent_r"
        };
        let (a, c) = ((sx * 0.10).min(sx * 0.30), (sx * 0.10).max(sx * 0.30));
        k.top_vent(name, hood, p.y_fw + 0.06, p.y_fw + 0.22, a, c, 3, M::Trim);
    }

    // --- Side intake behind the door with five horizontal strakes.
    let (yr, zr, rr) = k.arch(false);
    let (y0, y1) = (yr + rr + 0.04, at(&k, 0.47).min(p.y_quarter.0 + 0.45));
    let (z0, z1) = (zr + 0.04, (p.z_belt - 0.08).max(zr + 0.24));
    for sx in SIDES {
        let ki = dkey("side_intake_l", BodyRegion::RearLeftQuarter, M::Grille);
        k.side_patch(&ki, sx, y0, y1, z0, z1, 0.002, 0.006, (3, 2));
        let kk = dkey("side_strakes_l", BodyRegion::RearLeftQuarter, M::Paint);
        for i in 0..5 {
            let z = z0 + (z1 - z0) * (i as f32 + 0.5) / 5.0;
            k.side_patch(
                &kk,
                sx,
                y0 - 0.02,
                y1 + 0.20,
                z - 0.011,
                z + 0.011,
                0.006,
                0.016,
                (4, 1),
            );
        }
    }

    // --- Louvred engine deck.
    let ld0 = p.y_deck - 0.05;
    let ld1 = (yr - rr + 0.0).max(k.yt + 0.45);
    let half = p.half_width(0.5 * (ld0 + ld1)) - 0.28;
    k.louvres(BodyRegion::Trunk, ld1, ld0, 8, 0.0, half, M::Paint);

    // --- Rear: black mesh panel between quad round lamps.
    let (pz0, pz1) = (tz1 - 0.22, tz1 - 0.025);
    k.rear_panel(
        "tail_panel",
        BodyRegion::Trunk,
        wt - 0.02,
        pz0,
        pz1,
        M::Grille,
    );
    let km = dkey("tail_mesh", BodyRegion::Trunk, M::Trim);
    for i in 1..5 {
        let z = pz0 + (pz1 - pz0) * i as f32 / 5.0;
        k.rear_box(&km, -0.38, 0.38, z - 0.006, z + 0.006, 0.016, 0.0);
    }
    let zc = 0.5 * (pz0 + pz1);
    let r = (0.5 * (pz1 - pz0) - 0.03).min(0.06);
    for sx in SIDES {
        let tk = taillight_key(sx);
        let kring = sided(
            &dkey("taillight_ring_l", BodyRegion::RearLeftQuarter, M::Chrome),
            sx,
        );
        let kc = sided(
            &dkey(
                "taillight_inner_ring_l",
                BodyRegion::RearLeftQuarter,
                M::Chrome,
            ),
            sx,
        );
        let xo = wt - 0.10;
        let xi = xo - 2.6 * r;
        k.round_lamp(&tk, false, sx * xo, zc, r, 0.03, Some(&kring));
        // The inner round lamp is the amber turn signal.
        let ka = indicator_rear_key(sx, M::EmissiveAmber);
        k.round_lamp(&ka, false, sx * xi, zc, r, 0.03, Some(&kc));
    }
    let kv = dkey("rear_valance", BodyRegion::RearBumper, M::Grille);
    k.rear_box(
        &kv,
        -wt + 0.10,
        wt - 0.10,
        tz0 + 0.02,
        tz0 + 0.12,
        0.01,
        0.0,
    );

    k.mirrors(0.18, 0.10, None);
    k.fender_repeaters();
    k.wipers();
    k.fuel_filler(1.0, 0.09, M::Trim);
}

/// 2010s mid-engine V10 AWD supercar: 3-mouth black front, slim LED lamps with angular DRL
/// strokes, hood vents, giant side intakes, carbon skirts, glass engine cover between louvres,
/// fixed carbon wing, thin C-shaped light-bar tails over a black mesh fascia, 6-fin diffuser,
/// haunch scoops.
pub fn fulmine(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: 0.45,
        nose_h: Some(0.52),
        nose_rake_deg: 45.0,
        nose_plan_round: 0.20,
        tail_plan_round: 0.14,
        hood_crown: 0.020,
        windshield_rake_deg: Some(65.0),
        a_pillar_rake_deg: 18.0,
        rear_glass_rake_deg: Some(64.0),
        roof_len_trim: 0.15,
        tumblehome: Some(0.28),
        crease_z: 0.45,
        crease_depth: 0.025,
        haunch: 0.060,
        front_haunch: 0.030,
        flare: 0.020,
        ride_dz: -0.01,
        stance_pitch_deg: 0.2,
        ducktail_h: 0.030,
        belt_kick: 0.08,
        belt_rake: 0.03,
        tail_round: 0.10,
        tail_drop: 0.02,
        ..BodyStyle::preset(Silhouette::Wedge)
    });
    s.details.push(kit_detail(fulmine_kit));
    wheels(
        &mut s,
        def,
        RimKind::Monoblock,
        10,
        0.77,
        M::Engine,
        CapKind::CentreLock,
        (0.035, 0.012),
        (0.0, 0.5),
    );
    let m = &mut s.mechanical;
    m.engine = EngineKind::V10;
    m.layout = EngineLayout::Mid;
    m.induction = Induction::Natural;
    m.show_engine_detail = true;
    m.front_suspension = SuspensionKind::Coilover;
    m.rear_suspension = SuspensionKind::Coilover;
    m.front_brakes.caliper = CaliperKind::Fixed6;
    m.front_brakes.disc_radius_frac = 0.90;
    m.front_brakes.disc_thickness_m = 0.038;
    m.front_brakes.drilled = true;
    m.rear_brakes.caliper = CaliperKind::Fixed4;
    m.rear_brakes.disc_radius_frac = 0.85;
    m.rear_brakes.drilled = true;
    m.caliper_rgb = Some(rgb(0xF6C700));
    m.spring_rgb = Some(rgb(0xF57E20));
    m.exhaust = ExhaustKind::HighCentre;
    m.exhaust_tip_r_m = 0.05;
    s.tints.push((M::Paint, rgb(0x8DC63F)));
    s
}

fn fulmine_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;
    let hood = BodyRegion::Hood;

    // --- Slim LED lamps on the nose top with two angular DRL strokes each.
    for sx in SIDES {
        let (y0, y1) = (yn - 0.28, yn - 0.08);
        // LED lens with the amber turn signal in its outer corner.
        k.top_lamp_split(sx, y0, y1, 0.52, wn - 0.08, 0.035, Seg::Outboard(0.09));
        let kd = sided(
            &lkey("drl_l", BodyRegion::FrontLeftFender, M::Headlight),
            sx,
        );
        let za = k.top_z(y0 + 0.06, sx * 0.50) + 0.016;
        let zb = k.top_z(y1 - 0.08, sx * (wn - 0.10)) + 0.016;
        k.plate(
            &kd,
            [sx * 0.50, y0 + 0.06, za],
            [sx * (wn - 0.10), y1 - 0.08, zb],
            [0.0, 0.0, 1.0],
            0.016,
            0.008,
        );
        let zc = k.top_z(y1 - 0.06, sx * 0.55) + 0.016;
        k.plate(
            &kd,
            [sx * 0.52, y1 - 0.06, zc],
            [sx * (wn - 0.09), y1 - 0.08, zb],
            [0.0, 0.0, 1.0],
            0.016,
            0.008,
        );
    }
    // --- Three-mouth black front: big outer ducts + centre slot, carbon splitter.
    let (zd0, zd1) = (nz0 + 0.05, nz1 - 0.08);
    for sx in SIDES {
        let name = if sx < 0.0 {
            "front_duct_l"
        } else {
            "front_duct_r"
        };
        let x0 = sx * 0.46;
        let x1 = sx * (wn - 0.05);
        let g = dkey(name, fb, M::Grille);
        k.front_box(&g, x0, x1, zd0, zd1, 0.006, 0.0);
        let s2 = dkey(&format!("{name}_mesh"), fb, M::Trim);
        for i in 1..4 {
            let z = zd0 + (zd1 - zd0) * i as f32 / 4.0;
            k.front_box(&s2, x0, x1, z - 0.005, z + 0.005, 0.012, 0.0);
        }
        // Diagonal blade across each duct (angular "hex" read).
        let kb = sided(&dkey("front_duct_blade_l", fb, M::Carbon), sx);
        k.plate(
            &kb,
            [x0, yn + 0.012, zd1],
            [x1, yn + 0.012, zd0 + 0.02],
            [0.0, 1.0, 0.0],
            0.018,
            0.02,
        );
    }
    k.mesh_grille(
        "centre_slot",
        fb,
        0.34,
        nz0 + 0.07,
        nz0 + 0.15,
        2,
        8,
        M::Trim,
    );
    k.splitter(M::Carbon, 0.11, 0.06, 0.024, true);
    let span = yn - p.y_fw;
    for sx in SIDES {
        let name = if sx < 0.0 {
            "hood_vent_l"
        } else {
            "hood_vent_r"
        };
        let (a, c) = ((sx * 0.12).min(sx * 0.30), (sx * 0.12).max(sx * 0.30));
        k.top_vent(
            name,
            hood,
            yn - 0.62 * span,
            yn - 0.30 * span,
            a,
            c,
            4,
            M::Trim,
        );
    }

    // --- Giant side intakes ahead of the rear wheels: dark mesh, scallop wall, strake blade.
    let (yr, zr, rr) = k.arch(false);
    let (y0, y1) = (yr + rr + 0.03, (yr + rr + 0.78).min(p.y_quarter.0 + 0.70));
    let (z0, z1) = (zr - 0.07, (p.z_belt - 0.02).max(zr + 0.30));
    for sx in SIDES {
        let ki = dkey("side_intake_l", BodyRegion::RearLeftQuarter, M::Grille);
        k.side_patch(&ki, sx, y0, y1, z0, z1, 0.002, 0.008, (4, 3));
        let kw = sided(
            &dkey("side_intake_wall_l", BodyRegion::RearLeftQuarter, M::Paint),
            sx,
        );
        let (pa, _) = p.side_point(y1, z0);
        let (pb, _) = p.side_point(y1 - 0.10, z1);
        let xa = sx * (-pa[0] + 0.035);
        let xb = sx * (-pb[0] + 0.035);
        k.plate(
            &kw,
            [xa, y1, z0],
            [xb, y1 - 0.10, z1],
            [sx, 0.0, 0.0],
            0.07,
            0.02,
        );
        let kb = dkey("side_intake_blade_l", BodyRegion::RearLeftQuarter, M::Paint);
        let zm = 0.5 * (z0 + z1);
        k.side_patch(
            &kb,
            sx,
            y0,
            y1 - 0.04,
            zm - 0.015,
            zm + 0.015,
            0.008,
            0.02,
            (4, 1),
        );
    }
    k.skirts(M::Carbon, 0.045, 0.05);

    // --- Engine deck: glass cover strip between louvre fields; haunch scoops.
    let ld0 = p.y_deck - 0.06;
    let ld1 = (yr - rr + 0.05).max(k.yt + 0.55);
    let kg = PartKey {
        kind: PartKind::Detail,
        ..dkey("engine_cover_glass", BodyRegion::Trunk, M::Glass)
    };
    k.patch(&kg, ld1, ld0, top_band(-0.20, 0.20), 0.004, 0.006, (4, 2));
    let half = p.half_width(0.5 * (ld0 + ld1)) - 0.36;
    k.louvres(
        BodyRegion::Trunk,
        ld1,
        ld0,
        6,
        0.24,
        half.max(0.34),
        M::Trim,
    );
    for sx in SIDES {
        let name = if sx < 0.0 {
            "haunch_scoop_l"
        } else {
            "haunch_scoop_r"
        };
        let xo = p.half_width(yr) - 0.08;
        let (a, c) = (
            (sx * (xo - 0.16)).min(sx * xo),
            (sx * (xo - 0.16)).max(sx * xo),
        );
        k.top_vent(
            name,
            BodyRegion::Trunk,
            yr + 0.05,
            yr + 0.30,
            a,
            c,
            2,
            M::Paint,
        );
    }

    // --- Rear: fixed carbon wing, black mesh fascia, C-shaped light-bar tails + centre strip.
    k.wing(Wing {
        lead: 0.36,
        chord: 0.26,
        height: 0.13,
        half_span: 0.78,
        thick: 0.03,
        aoa_deg: 7.0,
        stand: Stand::Pedestal(0.42),
        endplates: Some((0.12, 0.30)),
        mat: M::Carbon,
        stand_mat: M::Carbon,
        gurney: 0.01,
    });
    let (fz0, fz1) = (tz0 + 0.12, tz1 - 0.09);
    k.rear_panel(
        "rear_fascia",
        BodyRegion::RearBumper,
        wt - 0.06,
        fz0,
        fz1,
        M::Grille,
    );
    let km = dkey("rear_fascia_mesh", BodyRegion::RearBumper, M::Trim);
    for i in 1..4 {
        let z = fz0 + (fz1 - fz0) * i as f32 / 4.0;
        k.rear_box(&km, -wt + 0.08, wt - 0.08, z - 0.005, z + 0.005, 0.016, 0.0);
    }
    let zt = tz1 - 0.05;
    for sx in SIDES {
        let tk = taillight_key(sx);
        let kh = sided(
            &dkey("taillight_housing_l", BodyRegion::RearLeftQuarter, M::Trim),
            sx,
        );
        k.rear_box(
            &kh,
            sx * 0.45,
            sx * (wt - 0.0),
            zt - 0.07,
            zt + 0.03,
            0.01,
            0.004,
        );
        // C: top stroke, outer vertical, bottom stroke; the outer vertical is the amber turn
        // signal, the strokes stop a seam short of it.
        let x_sig = wt - 0.06;
        let x_stroke = x_sig - INDICATOR_SEAM_M;
        k.rear_box(
            &tk,
            sx * 0.47,
            sx * x_stroke,
            zt - 0.004,
            zt + 0.018,
            0.024,
            0.0,
        );
        let ka = indicator_rear_key(sx, M::EmissiveAmber);
        k.rear_box(
            &ka,
            sx * x_sig,
            sx * (wt - 0.02),
            zt - 0.06,
            zt + 0.018,
            0.024,
            0.0,
        );
        k.rear_box(
            &tk,
            sx * (wt - 0.24),
            sx * x_stroke,
            zt - 0.06,
            zt - 0.04,
            0.024,
            0.0,
        );
    }
    let kb = lkey("taillight_bar", BodyRegion::Trunk, M::Taillight);
    k.rear_box(&kb, -0.44, 0.44, zt + 0.0, zt + 0.014, 0.02, 0.0);
    k.diffuser(M::Carbon, 0.70, 0.17, 6, 0.08);

    k.mirrors(0.20, 0.10, Some(M::Carbon));
    k.mirror_repeaters(0.20, 0.10, true);
    k.wipers();
}
