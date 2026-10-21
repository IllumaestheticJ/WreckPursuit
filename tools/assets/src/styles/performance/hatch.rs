//! Hot hatches: `vireo_xs` (late-90s FF 3-door) and `lindwurm_4t` (2010s AWD 5-door).
//!
//! Genre cues (research §3): two-box profile with a near-vertical hatch, a roof spoiler at the
//! top of the hatch, wheels filling the arches, a single big tip (90s) / quad tips in a black
//! diffuser (2010s), an accent colour (red lip on white, contrast mirror caps).

use wp_contracts::{BodyRegion, VehicleDef};

use crate::builder::{Builder, PartKey};
use crate::mesh::MaterialSlot as M;
use crate::style::{
    ArchShape, BodyStyle, CaliperKind, CapKind, CarStyle, DetailCtx, EngineKind, ExhaustKind,
    Induction, RimKind, Silhouette, SuspensionKind,
};

use super::kit::{dkey, headlight_key, lkey, sided, taillight_key, top_band, Kit, SIDES};
use super::{kit_detail, rgb, wheels};

/// Late-90s FF hot hatch: boxy, thin pillars, clear lamps, slim grille slot over a big lower
/// mouth with a red accent, small roof spoiler, vertical tri-colour tail clusters.
pub fn vireo(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        nose_h: Some(0.66),
        nose_rake_deg: 25.0,
        nose_plan_round: 0.30,
        tail_plan_round: 0.14,
        hood_crown: 0.015,
        windshield_rake_deg: Some(57.0),
        a_pillar_rake_deg: 8.0,
        rear_glass_rake_deg: Some(40.0),
        roof_len_trim: 0.05,
        tumblehome: Some(0.12),
        crease_z: 0.50,
        crease_depth: 0.006,
        haunch: 0.010,
        flare: 0.010,
        ride_dz: -0.02,
        tail_round: 0.10,
        tail_drop: 0.02,
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
    s
}

fn vireo_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;

    // --- Front: slim grille slot between clear lamps, big lower mouth, chin lip + red accent.
    k.grille(
        "grille",
        BodyRegion::Hood,
        0.30,
        nz1 - 0.09,
        nz1 - 0.035,
        1,
        M::Trim,
        None,
    );
    for sx in SIDES {
        let hk = headlight_key(sx);
        let (x0, x1) = (sx * 0.33, sx * (wn - 0.04));
        k.front_box(&hk, x0, x1, nz1 - 0.13, nz1 - 0.02, 0.022, 0.012);
        let kr = sided(
            &dkey(
                "headlamp_reflector_l",
                BodyRegion::FrontLeftFender,
                M::Chrome,
            ),
            sx,
        );
        k.round_lamp(&kr, true, sx * (wn - 0.22), nz1 - 0.075, 0.035, 0.026, None);
        let ka = sided(
            &lkey(
                "indicator_front_l",
                BodyRegion::FrontLeftFender,
                M::EmissiveAmber,
            ),
            sx,
        );
        k.side_box(&ka, sx, yn - 0.06, nz1 - 0.07, 0.07, 0.05, 0.012, 0.0);
    }
    let zm0 = nz0 + 0.07;
    let zm1 = nz0 + 0.24;
    k.grille("lower_mouth", fb, 0.50, zm0, zm1, 2, M::Trim, None);
    k.fogs(wn - 0.12, 0.5 * (zm0 + zm1), 0.04);
    k.splitter(M::Trim, 0.04, 0.12, 0.022, false);
    let accent = dkey("accent_stripe", fb, M::Caliper);
    k.front_box(&accent, -0.52, 0.52, zm1 + 0.012, zm1 + 0.028, 0.016, 0.0);
    k.front_box(
        &accent,
        -wn + 0.12,
        wn - 0.12,
        nz0 + 0.01,
        nz0 + 0.03,
        0.03,
        0.0,
    );

    // --- Rear: tall vertical tri-colour clusters at the corners, small roof spoiler.
    for sx in SIDES {
        let tk = taillight_key(sx);
        let (x0, x1) = (sx * (wt - 0.22), sx * (wt - 0.02));
        let zh = tz1 - 0.02;
        let zl = zh - 0.30;
        let zs = zl + 0.42 * (zh - zl);
        k.rear_box(&tk, x0, x1, zs + 0.005, zh, 0.024, 0.008);
        let ka = sided(
            &lkey(
                "indicator_rear_l",
                BodyRegion::RearLeftQuarter,
                M::EmissiveAmber,
            ),
            sx,
        );
        k.rear_box(&ka, x0, x1, zl + 0.06, zs - 0.005, 0.022, 0.006);
        let kc = sided(
            &lkey("reverse_lamp_l", BodyRegion::RearLeftQuarter, M::Headlight),
            sx,
        );
        k.rear_box(&kc, x0, x1, zl, zl + 0.055, 0.02, 0.006);
    }
    k.roof_spoiler(M::Paint, 0.20, 0.03, 0.04, 0.0);
    // Rear wiper on the hatch glass.
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
    let kp = dkey("plate_recess", BodyRegion::RearBumper, M::Trim);
    k.rear_box(&kp, -0.26, 0.26, tz0 + 0.20, tz0 + 0.33, 0.004, 0.0);
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

    k.skirts(M::Paint, 0.02, 0.05);
    k.mirrors(0.10, 0.11, None);
    k.door_handles(M::Trim);
    k.fuel_filler(1.0, 0.10, M::Paint);
    k.wipers();
    let _ = top_band;
}

/// 2010s AWD hot hatch: honeycomb grille with a blue accent bar, slim LED lamps, 3-mouth lower
/// intake, big roof spoiler with end plates, silver mirror caps, diffuser + quad tips,
/// horizontal wrap-around tails with a bright inner bar.
pub fn lindwurm(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        nose_h: Some(0.70),
        nose_rake_deg: 20.0,
        nose_plan_round: 0.25,
        tail_plan_round: 0.16,
        hood_crown: 0.020,
        hood_bulge_h: 0.010,
        hood_bulge_w: 0.50,
        windshield_rake_deg: Some(60.0),
        a_pillar_rake_deg: 10.0,
        rear_glass_rake_deg: Some(52.0),
        roof_dz: -0.02,
        tumblehome: Some(0.15),
        crease_z: 0.62,
        crease_depth: 0.012,
        haunch: 0.020,
        front_haunch: 0.012,
        flare: 0.015,
        flare_shape: ArchShape::Box,
        ride_dz: -0.015,
        belt_kick: 0.02,
        tail_round: 0.10,
        tail_drop: 0.03,
        ..BodyStyle::preset(Silhouette::Hatch)
    });
    s.details.push(kit_detail(lindwurm_kit));
    wheels(
        &mut s,
        def,
        RimKind::Turbine,
        10,
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

    // --- Front: honeycomb grille, accent bar through the lamps, slim LED lamps, 3-mouth intake.
    k.mesh_grille(
        "grille",
        BodyRegion::Hood,
        0.46,
        nz1 - 0.12,
        nz1 - 0.03,
        3,
        8,
        M::Trim,
    );
    let bar = dkey("accent_bar", BodyRegion::Hood, M::Caliper);
    k.front_box(
        &bar,
        -wn + 0.08,
        wn - 0.08,
        nz1 - 0.115,
        nz1 - 0.10,
        0.02,
        0.0,
    );
    for sx in SIDES {
        k.top_lamp(sx, yn - 0.17, yn - 0.03, 0.52, wn - 0.06, 0.022);
        let kh = sided(
            &dkey("headlamp_housing_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        k.front_box(
            &kh,
            sx * 0.48,
            sx * (wn - 0.03),
            nz1 - 0.095,
            nz1 - 0.02,
            0.008,
            0.006,
        );
        let drl = sided(
            &lkey("drl_l", BodyRegion::FrontLeftFender, M::Headlight),
            sx,
        );
        k.front_box(
            &drl,
            sx * 0.52,
            sx * (wn - 0.06),
            nz1 - 0.05,
            nz1 - 0.035,
            0.016,
            0.0,
        );
    }
    let (zi0, zi1) = (nz0 + 0.06, nz0 + 0.26);
    k.mesh_grille("lower_mouth", fb, 0.36, zi0, zi1, 3, 6, M::Trim);
    for sx in SIDES {
        let kd = sided(&dkey("side_duct_l", fb, M::Grille), sx);
        k.front_box(
            &kd,
            sx * 0.44,
            sx * (wn - 0.10),
            zi0 + 0.02,
            zi1 - 0.03,
            0.006,
            0.0,
        );
        let kb = sided(&dkey("side_duct_blade_l", fb, M::Trim), sx);
        for x in [0.56, 0.66] {
            k.front_box(
                &kb,
                sx * (x - 0.008),
                sx * (x + 0.008),
                zi0 + 0.02,
                zi1 - 0.03,
                0.02,
                0.0,
            );
        }
    }
    k.splitter(M::Trim, 0.05, 0.10, 0.02, false);

    // --- Rear: wrap-around horizontal tails with a bright inner bar, diffuser, big spoiler.
    for sx in SIDES {
        let tk = taillight_key(sx);
        let kh = sided(
            &dkey("taillight_housing_l", BodyRegion::RearLeftQuarter, M::Trim),
            sx,
        );
        let (z0, z1) = (tz1 - 0.13, tz1 - 0.03);
        k.rear_box(&kh, sx * 0.28, sx * (wt + 0.005), z0, z1, 0.014, 0.006);
        k.rear_box(
            &tk,
            sx * 0.30,
            sx * (wt - 0.01),
            z0 + 0.012,
            z1 - 0.012,
            0.022,
            0.006,
        );
        let ki = sided(
            &lkey("taillight_bar_l", BodyRegion::RearLeftQuarter, M::Taillight),
            sx,
        );
        k.rear_box(
            &ki,
            sx * 0.32,
            sx * (wt - 0.03),
            0.5 * (z0 + z1) - 0.008,
            0.5 * (z0 + z1) + 0.008,
            0.032,
            0.0,
        );
        // Wrap onto the quarter.
        let kw = dkey(
            "taillight_wrap_l",
            BodyRegion::RearLeftQuarter,
            M::Taillight,
        );
        let kw = PartKey {
            kind: crate::mesh::PartKind::Light,
            ..kw
        };
        k.side_patch(
            &kw,
            sx,
            yt + 0.01,
            yt + 0.16,
            z0 + 0.015,
            z1 - 0.015,
            0.002,
            0.012,
            (2, 1),
        );
    }
    k.diffuser(M::Trim, 0.62, 0.16, 3, 0.05);
    k.roof_spoiler(M::Paint, 0.30, 0.05, 0.02, 0.09);
    let kc = dkey("roof_spoiler_gap", BodyRegion::Trunk, M::Trim);
    let y0 = p.roof.0 + 0.04;
    let z0 = k.top_z(y0, 0.0);
    k.bx(
        &kc,
        [-0.25, y0 - 0.26, z0 - 0.045],
        [0.25, y0 - 0.20, z0 - 0.025],
    );
    let kp = dkey("plate_recess", BodyRegion::RearBumper, M::Trim);
    k.rear_box(&kp, -0.26, 0.26, tz0 + 0.22, tz0 + 0.35, 0.004, 0.0);

    k.skirts(M::Trim, 0.03, 0.06);
    k.mirrors(0.10, 0.12, Some(M::Metal));
    k.door_handles(M::Trim);
    k.fuel_filler(1.0, 0.11, M::Paint);
    k.wipers();
}
