//! Mid-engine: `lucciola_350` (90s rounded mid-engine berlinetta) and `fulmine_v10` (2010s V10
//! supercar).
//!
//! Silhouettes (owner 2026-10-26: "the exotic cars all basically look like each other"): the
//! Lucciola is a soft-nosed notchback with an upright rear glass between flying-buttress sails
//! and a rounded tail; the Fulmine a low knife-edge wedge with sharp plan corners and box arches.
//!
//! Both lattices carry a front-engine cabin, so the cab-forward look comes from
//! `cabin_shift_m` (+0.40 / +0.45 m forward: roof centre ≈ 46 % of the length from the nose,
//! windshield base ≈ 18–19 %) and a steeper rear glass toward the engine deck. Genre cues
//! (research §3): knife-edge wedge, louvred engine deck, quad round tails in a black panel,
//! side strakes, pop-up lamps (exotic); low wide cab-forward wedge, giant side intakes, thin
//! light-bar tails + black mesh, big diffuser + wing (supercar).

use wp_contracts::{BodyRegion, VehicleDef};

use crate::builder::{Builder, PartKey};
use crate::lamp::LampView;
use crate::mesh::{MaterialSlot as M, PartKind};
use crate::style::{
    ArchShape, BodyStyle, CaliperKind, CapKind, CarStyle, DetailCtx, EngineKind, EngineLayout,
    ExhaustKind, FuelDoor, Induction, RimKind, Silhouette, SuspensionKind, TrimFinish,
};

use super::kit::{
    dkey, headlight_key, indicator_front_key, indicator_rear_key, sided, taillight_key, top_band,
    Kit, Seg, SIDES,
};
use super::supercar::{
    end_poly, flip_x, rear_stroke, shape3, side_poly, stadium, top_poly, top_stroke, Shape3,
};
use super::{kit_detail, rgb, wheels};

/// 90s mid-engine berlinetta: soft round nose, rounded roof, upright rear glass between
/// flying-buttress sails, closed pop-up lamp pods, oval slatted nose intake, one teardrop
/// side scoop, louvred deck, one wide pill-shaped tail lamp per side (amber inner end) in a
/// chrome surround on a black rear mesh panel.
pub fn lucciola(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: 0.38,
        nose_h: Some(0.47),
        cowl_h: 0.05,
        belt_dz: 0.07,
        nose_rake_deg: 48.0,
        nose_plan_round: 0.40,
        tail_plan_round: 0.26,
        hood_crown: 0.022,
        windshield_rake_deg: Some(62.0),
        a_pillar_rake_deg: 12.0,
        rear_glass_rake_deg: Some(38.0),
        roof_arc: 0.55,
        tumblehome: Some(0.26),
        crease_z: 0.46,
        crease_depth: 0.012,
        shoulder_round: 0.10,
        haunch: 0.07,
        front_haunch: 0.05,
        flare: 0.008,
        ride_dz: -0.025,
        stance_pitch_deg: 0.3,
        ducktail_h: 0.025,
        deck_valley: 0.02,
        overhang_trim: (0.0, -0.03),
        belt_kick: 0.03,
        belt_rake: 0.03,
        tail_round: 0.22,
        tail_drop: 0.06,
        sill_tuck: 0.09,
        ..BodyStyle::preset(Silhouette::Notchback)
    });
    s.details.push(kit_detail(lucciola_kit));
    shape3(
        &mut s,
        def,
        Shape3 {
            nose: 0.86,
            tail: 0.92,
            waist: 0.95,
            scallop: -0.05,
            shoulder: 0.0,
            bulge: 0.03,
            rocker: 0.04,
            rise: (0.02, 0.03),
        },
    );
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
    // --- Oval centre intake with four slats, small corner ducts, black lip.
    let (zi0, zi1) = (nz0 + 0.04, nz0 + 0.19);
    let kg = dkey("nose_intake", fb, M::Grille);
    let (hw, zm, hh) = (0.50, 0.5 * (zi0 + zi1), 0.5 * (zi1 - zi0));
    let oval: Vec<[f32; 2]> = (0..16)
        .map(|i| {
            let t = i as f32 / 16.0 * std::f32::consts::TAU;
            // Squared-off ellipse: wide flat top and bottom, round ends.
            let (s, c) = t.sin_cos();
            let cx = c.signum() * c.abs().powf(0.45);
            let sz = s.signum() * s.abs().powf(0.45);
            [hw * cx, zm + hh * sz]
        })
        .collect();
    end_poly(&mut k, &kg, true, &oval, 0.006, 0.03);
    let ks = dkey("nose_intake_slats", fb, M::Trim);
    for i in 1..5 {
        let z = zi0 + (zi1 - zi0) * i as f32 / 5.0;
        k.front_box(&ks, -hw + 0.06, hw - 0.06, z - 0.007, z + 0.007, 0.014, 0.0);
    }
    for sx in SIDES {
        let kd = sided(&dkey("nose_duct_l", fb, M::Grille), sx);
        let duct = [
            [0.62, zi0 + 0.02],
            [wn - 0.06, zi0 + 0.02],
            [wn - 0.05, zi1 - 0.05],
            [0.64, zi1 - 0.03],
        ];
        end_poly(&mut k, &kd, true, &flip_x(&duct, sx), 0.006, 0.03);
    }
    k.splitter(M::Trim, 0.03, 0.12, 0.018, false);

    // --- One smooth teardrop scoop in the quarter behind the door, its front lip in body
    //     colour, feeding the engine bay.
    let (yr, zr, rr) = k.arch(false);
    let (y0, y1) = (yr + rr + 0.03, (p.y_fd_rear + 0.02).max(yr + rr + 0.40));
    let (z0, z1) = (zr + 0.02, (p.z_belt - 0.07).max(zr + 0.22));
    for sx in SIDES {
        let ki = dkey("side_intake_l", BodyRegion::RearLeftQuarter, M::Grille);
        let scoop = [
            [y0, z0 + 0.02],
            [y1 - 0.10, z0 + 0.06],
            [y1, 0.5 * (z0 + z1) + 0.02],
            [y1 - 0.06, z1 - 0.01],
            [y1 - 0.20, z1],
            [y0, z1 - 0.02],
        ];
        side_poly(&mut k, &ki, sx, &scoop, 0.002, 0.012);
        let kl = dkey("side_intake_lip_l", BodyRegion::RearLeftQuarter, M::Paint);
        let lip = [
            [y1 - 0.06, z0 + 0.06],
            [y1 + 0.01, 0.5 * (z0 + z1) + 0.02],
            [y1 - 0.05, z1 + 0.01],
            [y1 - 0.08, z1 - 0.01],
            [y1 - 0.025, 0.5 * (z0 + z1) + 0.02],
            [y1 - 0.09, z0 + 0.07],
        ];
        side_poly(&mut k, &kl, sx, &lip, 0.014, 0.02);
    }

    // --- Louvred engine deck.
    let ld0 = p.y_deck - 0.05;
    let ld1 = (yr - rr + 0.0).max(k.yt + 0.45);
    let half = p.half_width(0.5 * (ld0 + ld1)) - 0.28;
    k.louvres(BodyRegion::Trunk, ld1, ld0, 8, 0.0, half, M::Paint);

    // --- Flying buttresses: body-colour sails from the roof's rear corners down to the tail,
    //     either side of the upright rear glass and the louvred deck.
    let r0 = p.roof.0;
    let xs = k.roof_edge(r0 + 0.02) - 0.03;
    for sx in SIDES {
        let ks = sided(&dkey("sail_l", BodyRegion::RearLeftQuarter, M::Paint), sx);
        k.sail(&ks, sx, r0 + 0.02, k.yt + 0.30, xs, 0.07, 0.035);
    }

    // --- Rear: black mesh panel between the pill lamps.
    let (pz0, pz1) = (tz1 - 0.22, tz1 - 0.025);
    // Flatter than `Kit::rear_panel` (2 mm proud) so the cast lamps stand clear of it.
    let ktp = dkey("tail_panel", BodyRegion::Trunk, M::Grille);
    k.rear_box(&ktp, -(wt - 0.02), wt - 0.02, pz0, pz1, 0.002, 0.0);
    let km = dkey("tail_mesh", BodyRegion::Trunk, M::Trim);
    for i in 1..5 {
        let z = pz0 + (pz1 - pz0) * i as f32 / 5.0;
        k.rear_box(&km, -0.38, 0.38, z - 0.006, z + 0.006, 0.016, 0.0);
    }
    // One wide pill-shaped lamp per side (this car's night signature), cast onto the tail
    // with a black bezel; its inner end is the amber turn signal.
    let zc = 0.5 * (pz0 + pz1);
    let hh = (0.5 * (pz1 - pz0) - 0.035).min(0.055);
    for sx in SIDES {
        let (xo, xi) = (wt - 0.07, wt - 0.46);
        let pill = stadium(-0.5 * (xo + xi), zc, 0.5 * (xo - xi), hh, 5);
        k.proj_cluster(
            sx,
            false,
            LampView::Rear,
            &pill,
            Some(Seg::Inboard(0.11)),
            M::EmissiveAmber,
            0.016,
            10.0,
        );
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
    k.fuel_filler(1.0, 0.09, M::Trim);
}

/// 2010s mid-engine V10 AWD supercar: low sharp-cornered wedge with the hood running in one
/// line from the nose lip to the windshield; slim angular head lamps with a lightning-bolt
/// DRL (this fictional maker's signature); a black lower front with two big trapezoid intakes; a sharp diagonal side
/// line rising from the front wheel into the big quarter intake behind the door; louvred
/// engine cover over a glass strip; integrated lip spoiler (no wing on the stock car); thin
/// lightning-bolt tail strokes over a slatted black fascia, twin tips each side low in the
/// diffuser.
pub fn fulmine(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: 0.45,
        nose_h: Some(0.45),
        nose_rake_deg: 52.0,
        nose_plan_round: 0.14,
        tail_plan_round: 0.08,
        hood_crown: 0.014,
        cowl_h: 0.14,
        windshield_rake_deg: Some(66.0),
        a_pillar_rake_deg: 18.0,
        a_pillar_w_m: 0.07,
        a_sail_len_m: 0.16,
        c_pillar_rake_deg: 10.0,
        c_pillar_w_m: 0.16,
        b_pillar_w_m: 0.07,
        windshield_wrap_m: 0.09,
        roof_arc: 0.42,
        tumblehome: Some(0.30),
        crease_z: 0.40,
        crease_depth: 0.026,
        shoulder_round: 0.03,
        sill_tuck: 0.08,
        haunch: 0.075,
        front_haunch: 0.045,
        flare: 0.012,
        flare_shape: ArchShape::Box,
        ride_dz: -0.03,
        stance_pitch_deg: 0.3,
        belt_dz: 0.08,
        ducktail_h: 0.012,
        deck_drop: 0.0,
        rear_glass_rake_deg: Some(64.0),
        belt_kick: 0.0,
        belt_rake: 0.04,
        tail_round: 0.06,
        tail_drop: 0.0,
        roof_dz: -0.02,
        door_handles: TrimFinish::None,
        fuel_door: FuelDoor::None,
        ..BodyStyle::preset(Silhouette::Wedge)
    });
    s.details.push(kit_detail(fulmine_kit));
    shape3(
        &mut s,
        def,
        Shape3 {
            nose: 0.88,
            tail: 0.95,
            waist: 0.96,
            scallop: -0.06,
            shoulder: 0.0,
            bulge: 0.015,
            rocker: 0.04,
            rise: (0.0, 0.03),
        },
    );
    wheels(
        &mut s,
        def,
        RimKind::Monoblock,
        10,
        0.78,
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
    m.exhaust = ExhaustKind::Quad;
    m.exhaust_tip_r_m = 0.045;
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

    // --- Slim angular head lamps on the nose top: a dark lens that tapers back along the
    //     fender, a lightning-bolt DRL (the car's own signature) inside it, a small
    //     projector, the amber signal at its nose.
    for sx in SIDES {
        let wf = p.half_width(yn - 0.30);
        let lens = [
            [0.44, yn - 0.13],
            [wn - 0.05, yn - 0.035],
            [wf - 0.06, yn - 0.32],
            [0.56, yn - 0.25],
        ];
        let kh = sided(
            &dkey("headlamp_housing_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        let hous = [
            [0.42, yn - 0.145],
            [wn - 0.03, yn - 0.02],
            [wf - 0.04, yn - 0.34],
            [0.55, yn - 0.27],
        ];
        top_poly(&mut k, &kh, &flip_x(&hous, sx), 0.004, 0.012);
        let kl = sided(
            &dkey("headlamp_lens_l", BodyRegion::FrontLeftFender, M::Glass),
            sx,
        );
        top_poly(&mut k, &kl, &flip_x(&lens, sx), 0.009, 0.012);
        let hk = headlight_key(sx);
        // Bolt: out along the front edge, a sharp kink back inward, then out to the rear tip.
        let b0 = [sx * 0.48, yn - 0.135];
        let b1 = [sx * (wn - 0.20), yn - 0.08];
        let b2 = [sx * (wn - 0.27), yn - 0.17];
        let b3 = [sx * (wf - 0.09), yn - 0.27];
        for (a, c) in [(b0, b1), (b1, b2), (b2, b3)] {
            top_stroke(&mut k, &hk, a, c, 0.016, 0.012);
        }
        let proj = [
            [0.64, yn - 0.17],
            [0.74, yn - 0.15],
            [0.74, yn - 0.21],
            [0.64, yn - 0.22],
        ];
        top_poly(&mut k, &hk, &flip_x(&proj, sx), 0.012, 0.01);
        let ik = indicator_front_key(sx);
        let sig = [
            [wn - 0.13, yn - 0.06],
            [wn - 0.06, yn - 0.045],
            [wn - 0.08, yn - 0.09],
            [wn - 0.14, yn - 0.10],
        ];
        top_poly(&mut k, &ik, &flip_x(&sig, sx), 0.022, 0.02);
    }

    // --- Black lower front: two big trapezoid intakes split by a slanted blade, centre
    //     slot under a body-colour nose tip, carbon splitter.
    let (zd0, zd1) = (nz0 + 0.04, nz1 - 0.05);
    for sx in SIDES {
        let kg = sided(&dkey("front_duct_l", fb, M::Grille), sx);
        let duct = [
            [0.36, zd0],
            [wn - 0.03, zd0],
            [wn - 0.03, zd1 - 0.02],
            [0.52, zd1],
        ];
        end_poly(&mut k, &kg, true, &flip_x(&duct, sx), 0.006, 0.03);
        let km = sided(&dkey("front_duct_mesh_l", fb, M::Trim), sx);
        for i in 1..4 {
            let z = zd0 + (zd1 - zd0) * i as f32 / 4.0;
            let xi = 0.36 + (0.52 - 0.36) * (z - zd0) / (zd1 - zd0);
            k.front_box(
                &km,
                sx * (xi + 0.02),
                sx * (wn - 0.05),
                z - 0.005,
                z + 0.005,
                0.012,
                0.0,
            );
        }
        // Slanted blade across each intake.
        let kb = sided(&dkey("front_duct_blade_l", fb, M::Paint), sx);
        let blade = [
            [0.58, zd1 - 0.005],
            [0.66, zd1 - 0.005],
            [wn - 0.10, zd0 + 0.01],
            [wn - 0.18, zd0 + 0.01],
        ];
        end_poly(&mut k, &kb, true, &flip_x(&blade, sx), 0.018, 0.03);
    }
    k.mesh_grille(
        "centre_slot",
        fb,
        0.30,
        nz0 + 0.05,
        nz0 + 0.12,
        2,
        8,
        M::Trim,
    );
    k.splitter(M::Carbon, 0.08, 0.06, 0.022, true);

    // --- Sharp diagonal line from behind the front wheel up into the quarter intake, which
    //     sits behind the door ahead of the rear wheel (front edge leaning back).
    let (yr, zr, rr) = k.arch(false);
    let (yf, zf, rf) = k.arch(true);
    let yi1 = p.y_fd_rear + 0.02;
    let yi0 = yr + rr + 0.04;
    let zi1 = p.belt(yi1) - 0.05;
    let zi0 = zr - 0.02;
    for sx in SIDES {
        let ki = dkey("side_intake_l", BodyRegion::RearLeftQuarter, M::Grille);
        let intake = [
            [yi0, zi0],
            [yi1 - 0.16, zi0],
            [yi1, zi1 - 0.02],
            [yi1 - 0.08, zi1],
            [yi0, zi1 - 0.04],
        ];
        side_poly(&mut k, &ki, sx, &intake, 0.002, 0.01);
        let kb = dkey("side_intake_blade_l", BodyRegion::RearLeftQuarter, M::Paint);
        let zm = 0.5 * (zi0 + zi1);
        let blade = [
            [yi0, zm - 0.012],
            [yi1 - 0.10, zm - 0.012],
            [yi1 - 0.08, zm + 0.012],
            [yi0, zm + 0.012],
        ];
        side_poly(&mut k, &kb, sx, &blade, 0.014, 0.02);
        // The diagonal crease: a thin dark groove from the front arch's top rear to the
        // intake's lower front corner.
        let kc = dkey("side_line_l", BodyRegion::LeftDoorF, M::Grille);
        let (ya, za) = (yf - rf + 0.02, zf + 0.10);
        let (yb, zb) = (yi1 - 0.14, zi0 + 0.06);
        let line = [
            [ya, za],
            [ya - 0.04, za - 0.012],
            [yb, zb - 0.010],
            [yb + 0.02, zb + 0.004],
        ];
        side_poly(&mut k, &kc, sx, &line, 0.003, 0.008);
    }
    k.skirts(M::Carbon, 0.04, 0.05);

    // --- Engine cover: glass strip between louvre fields, small haunch vents.
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
        let kv = sided(&dkey("haunch_vent_l", BodyRegion::Trunk, M::Grille), sx);
        let xo = p.half_width(yr) - 0.08;
        let vent = [
            [xo - 0.18, yr + 0.30],
            [xo - 0.04, yr + 0.30],
            [xo, yr + 0.06],
            [xo - 0.14, yr + 0.02],
        ];
        top_poly(&mut k, &kv, &flip_x(&vent, sx), 0.004, 0.01);
    }

    // --- Rear: integrated lip spoiler, slatted black fascia, lightning-bolt tail strokes.
    k.lip_spoiler(M::Paint, k.yt + 0.12, 0.035, 0.10, 0.10);
    let (fz0, fz1) = (tz0 + 0.10, tz0 + 0.55 * (tz1 - tz0));
    k.rear_panel(
        "rear_fascia",
        BodyRegion::RearBumper,
        wt - 0.06,
        fz0,
        fz1,
        M::Grille,
    );
    let km = dkey("rear_fascia_mesh", BodyRegion::RearBumper, M::Trim);
    for i in 1..5 {
        let z = fz0 + (fz1 - fz0) * i as f32 / 5.0;
        k.rear_box(&km, -wt + 0.10, wt - 0.10, z - 0.006, z + 0.006, 0.016, 0.0);
    }
    let zt = tz1 - 0.05;
    for sx in SIDES {
        let kh = sided(
            &dkey("taillight_housing_l", BodyRegion::RearLeftQuarter, M::Trim),
            sx,
        );
        let hous = [
            [0.40, zt + 0.03],
            [wt - 0.01, zt + 0.035],
            [wt - 0.01, zt - 0.08],
            [wt - 0.10, zt - 0.08],
            [0.42, zt - 0.02],
        ];
        end_poly(&mut k, &kh, false, &flip_x(&hous, sx), 0.010, 0.03);
        let tk = taillight_key(sx);
        // Bolt: along the top, a kink down and back inward, then out to the corner.
        let b0 = [sx * 0.44, zt + 0.005];
        let b1 = [sx * (wt - 0.24), zt + 0.012];
        let b2 = [sx * (wt - 0.30), zt - 0.055];
        let b3 = [sx * (wt - 0.08), zt - 0.04];
        for (a, c) in [(b0, b1), (b1, b2), (b2, b3)] {
            rear_stroke(&mut k, &tk, a, c, 0.018, 0.022);
        }
        // Amber signal: a short bar at the outer top corner of the lamp.
        let ka = indicator_rear_key(sx, M::EmissiveAmber);
        let sig = [
            [wt - 0.06, zt + 0.03],
            [wt - 0.025, zt + 0.03],
            [wt - 0.025, zt + 0.005],
            [wt - 0.06, zt + 0.005],
        ];
        end_poly(&mut k, &ka, false, &flip_x(&sig, sx), 0.022, 0.03);
    }
    k.diffuser(M::Carbon, 0.70, 0.16, 6, 0.07);

    k.mirrors(0.20, 0.10, Some(M::Carbon));
    k.mirror_repeaters(0.20, 0.10, true);
}
