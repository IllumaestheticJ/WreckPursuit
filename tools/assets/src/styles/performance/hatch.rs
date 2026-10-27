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
use crate::lamp::{ellipse, Lamp, LampView};
use crate::mesh::{MaterialSlot as M, INDICATOR_SEAM_M};
use crate::style::{
    ArchShape, BodyStyle, CaliperKind, CapKind, CarStyle, DetailCtx, EngineKind, ExhaustKind,
    Induction, RimKind, SideSculpt, Silhouette, SuspensionKind, TrimFinish,
};

use super::kit::{dkey, lkey, sided, Kit, Seg, SIDES};
use super::{kit_detail, rgb, wheels};

// ---------------------------------------------------------------------------- local helpers

/// Flat part lying on the nose (`front`) or tail surface: outline `pts` ((x, z), signed x,
/// any simple polygon) cast onto the body from ahead / behind ([`crate::lamp`]) and standing
/// `proud` over it, so it follows the face rake, chin and plan bow.
fn face_poly(k: &mut Kit, key: &PartKey, front: bool, pts: &[[f32; 2]], proud: f32) {
    let view = if front {
        LampView::Front
    } else {
        LampView::Rear
    };
    let mut lamp = Lamp::new(view, pts.to_vec());
    lamp.proud_m = proud.clamp(0.002, 0.03);
    lamp.step_m = 0.08;
    crate::lamp::lens(k.b, k.p, key, &lamp);
}

/// Rectangle `x0..x1` × `z0..z1` lying on the nose / tail surface (see [`face_poly`]).
fn face_box(
    k: &mut Kit,
    key: &PartKey,
    front: bool,
    x0: f32,
    x1: f32,
    z0: f32,
    z1: f32,
    proud: f32,
) {
    face_poly(
        k,
        key,
        front,
        &[[x0, z0], [x1, z0], [x1, z1], [x0, z1]],
        proud,
    );
}

/// Thin bar `x0..x1` × `z0..z1` across the middle of the nose / tail: a cheap flat prism
/// ([`Kit::face_poly`]) that the end-face follow pass moves onto a raked face (keep it clear of
/// the plan-bowed corners, where [`face_box`] follows the curve).
fn face_bar(
    k: &mut Kit,
    key: &PartKey,
    front: bool,
    x0: f32,
    x1: f32,
    z0: f32,
    z1: f32,
    proud: f32,
) {
    k.face_poly(key, front, &[[x0, z0], [x1, z0], [x1, z1], [x0, z1]], proud);
}

/// Slot grille on the nose surface: dark insert `±half` × `z0..z1` with `slats` bars.
fn slot_grille(
    k: &mut Kit,
    name: &str,
    region: BodyRegion,
    half: f32,
    z0: f32,
    z1: f32,
    slats: usize,
    slat_mat: M,
) {
    let g = dkey(name, region, M::Grille);
    face_box(k, &g, true, -half, half, z0, z1, 0.006);
    let sk = dkey(&format!("{name}_slats"), region, slat_mat);
    let h = (z1 - z0) / (slats as f32 + 1.0);
    for i in 1..=slats {
        let z = z0 + h * i as f32;
        face_bar(
            k,
            &sk,
            true,
            -half + 0.02,
            half - 0.02,
            z - 0.008,
            z + 0.008,
            0.014,
        );
    }
}

/// Round fog lamps (lens `Headlight`, black ring) at ±`x`, `z`, radius `r`, on the nose
/// surface.
fn round_fogs(k: &mut Kit, x: f32, z: f32, r: f32) {
    for sx in SIDES {
        let kf = sided(&lkey("fog_l", BodyRegion::FrontBumper, M::Headlight), sx);
        let kr = sided(&dkey("fog_ring_l", BodyRegion::FrontBumper, M::Trim), sx);
        let mut lamp = Lamp::new(LampView::Front, ellipse([sx * x, z], [r, r], 14));
        lamp.proud_m = 0.012;
        crate::lamp::lens(k.b, k.p, &kf, &lamp);
        crate::lamp::bezel(k.b, k.p, &kr, &lamp, 0.012);
    }
}

/// Rear diffuser: an underside plate, a dark valance `±half` × `h` lying on the tail surface
/// and `fins` vertical fins sticking out `fin_out` behind it.
fn rear_diffuser(k: &mut Kit, mat: M, half: f32, h: f32, fins: usize, fin_out: f32) {
    let key = dkey("diffuser", BodyRegion::RearBumper, mat);
    let y = k.yt;
    let z0 = k.p.sill(y) - 0.02;
    k.bx(&key, [-half, y + 0.30, z0], [half, y - 0.02, z0 + 0.025]);
    face_box(k, &key, false, -half, half, z0, z0 + h, 0.012);
    let f = dkey("diffuser_fins", BodyRegion::RearBumper, mat);
    for i in 0..fins {
        let x = -half + 0.06 + (2.0 * half - 0.12) * i as f32 / (fins - 1).max(1) as f32;
        let prof = [
            [y + 0.25, z0 + 0.02],
            [y - fin_out, z0 + 0.02],
            [y - fin_out, z0 + 0.6 * h],
            [y - 0.01, z0 + h],
        ];
        k.prism_x(&f, &prof, x - 0.006, x + 0.006);
    }
}

/// Left-side outline `pts` ((|x|, z)) moved to side `sx`.
fn on_side(sx: f32, pts: &[[f32; 2]]) -> Vec<[f32; 2]> {
    pts.iter().map(|q| [sx * q[0], q[1]]).collect()
}

/// Extra projected lens (not a head / tail lamp part: reverse lamps, DRLs, light bars) of
/// side `sx` on the body ([`crate::lamp`]); `left` is the left-side outline in `view`'s
/// coordinates, `proud` its height over the surface (above a lamp lens it sits on).
fn proj_lens(
    k: &mut Kit,
    key: &PartKey,
    sx: f32,
    view: LampView,
    left: &[[f32; 2]],
    yaw_deg: f32,
    proud: f32,
) {
    let pts: Vec<[f32; 2]> = left.iter().map(|q| [sx * q[0].abs(), q[1]]).collect();
    let mut lamp = Lamp::new(view, pts);
    lamp.yaw_deg = yaw_deg;
    lamp.proud_m = proud;
    crate::lamp::lens(k.b, k.p, key, &lamp);
}

/// Blank badge (no lettering) centred at (x, z) on the nose or tail face.
fn badge(k: &mut Kit, region: BodyRegion, front: bool, x: f32, z: f32, w: f32, h: f32) {
    let kb = dkey(
        if front { "badge_front" } else { "badge_rear" },
        region,
        M::Chrome,
    );
    if front {
        face_box(
            k,
            &kb,
            true,
            x - 0.5 * w,
            x + 0.5 * w,
            z - 0.5 * h,
            z + 0.5 * h,
            0.022,
        );
    } else {
        face_box(
            k,
            &kb,
            false,
            x - 0.5 * w,
            x + 0.5 * w,
            z - 0.5 * h,
            z + 0.5 * h,
            0.022,
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

/// Small red reflectors at the rear bumper corners.
fn rear_reflectors(k: &mut Kit, z: f32, inset: f32) {
    let wt = k.wt();
    for sx in SIDES {
        let kr = sided(
            &dkey("rear_reflector_l", BodyRegion::RearBumper, M::Taillight),
            sx,
        );
        face_box(
            k,
            &kr,
            false,
            sx * (wt - inset - 0.10),
            sx * (wt - inset),
            z - 0.012,
            z + 0.012,
            0.016,
        );
    }
}

/// Licence plate (blank, light) on a dark recess on the tail face.
fn rear_plate(k: &mut Kit, region: BodyRegion, zc: f32) {
    let kp = dkey("plate_recess", region, M::Trim);
    face_box(k, &kp, false, -0.27, 0.27, zc - 0.07, zc + 0.07, 0.006);
    let kq = dkey("plate", region, M::Metal);
    face_box(k, &kq, false, -0.25, 0.25, zc - 0.055, zc + 0.055, 0.012);
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

/// Front and rear wheel stations of the def: (front y, front radius, rear y, rear radius).
fn axles(def: &VehicleDef) -> (f32, f32, f32, f32) {
    let ys = def.wheels.iter().map(|w| (w.position_m[1], w.radius_m));
    let f = ys
        .clone()
        .fold((f32::MIN, 0.3), |a, w| if w.0 > a.0 { w } else { a });
    let r = ys.fold((f32::MAX, 0.3), |a, w| if w.0 < a.0 { w } else { a });
    (f.0, f.1, r.0, r.1)
}

/// Plan-view taper: full width from just ahead of the rear wheel to just behind the front
/// wheel, easing to `nose` (scale) at the nose and `tail` at the tail, plus an optional waist
/// scale at the doors (1.0 = none).
fn plan_taper(def: &VehicleDef, nose: f32, tail: f32, waist: f32) -> Vec<[f32; 2]> {
    let (fy, fr, ry, rr) = axles(def);
    let mut v = vec![[ry - rr - 0.55, tail], [ry - rr + 0.05, 1.0]];
    if waist < 0.999 {
        v.push([ry + rr + 0.10, 1.0]);
        v.push([0.5 * (fy + ry), waist]);
        v.push([fy - fr - 0.10, 1.0]);
    }
    v.push([fy + fr - 0.05, 1.0]);
    v.push([fy + fr + 0.60, nose]);
    v
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
    // Rounded 90s section: soft bulged sides tucking under, strong tumblehome, a rounded-wedge
    // nose in plan, fender tops just above the hood.
    let b = &mut s.body;
    b.plan_width = plan_taper(def, 0.87, 0.93, 1.0);
    b.side_bulge_m = 0.035;
    b.side_bulge_z = 0.6;
    b.rocker_in_m = 0.045;
    b.tumblehome_m = Some(0.17);
    b.front_fender_rise_m = 0.015;
    b.fender_peak_w = 0.82;
    // Rounded nose in side view and plan (raked face, undercut chin, bowed corners), tailgate
    // leaning in (capped by the short tail section), roof narrowing toward the hatch.
    b.nose_face_rake_deg = 20.0;
    b.nose_chin_m = 0.05;
    b.nose_plan_bow_m = 0.25;
    b.nose_tip_w = 0.65;
    b.tail_face_rake_deg = 15.0;
    b.roof_plan_taper = 0.10;
    // Tailgate lip under the hatch glass; blind C-pillar.
    b.glass_base_lip_m = 0.03;
    b.quarter_glass = false;
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
    // Face width just behind the plan bow (the face itself is narrower; projected parts
    // follow the curve).
    let wn = k.p.half_width(k.yn - 0.12);
    let wt = k.wt();
    let yn = k.yn;
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;

    // --- Front lamps: teardrop lenses lying on the rounded nose top, swept back along the
    // fender, amber signal at the outer tip, black bezel.
    let (lt, lb) = (nz1 - 0.034, nz1 - 0.105);
    for sx in SIDES {
        // Head lamp on the raked face, cast round the bowed corner, signal at the outer end.
        let face = [
            [-0.30, nz1 - 0.03],
            [-(wn - 0.02), nz1 - 0.03],
            [-(wn - 0.02), nz1 - 0.065],
            [-(wn - 0.12), lb],
            [-0.34, lb],
        ];
        k.proj_cluster(
            sx,
            true,
            LampView::Front,
            &face,
            Some(Seg::Outboard(0.10)),
            M::EmissiveAmber,
            0.010,
            25.0,
        );
        // Its lens carries on over the nose top as a swept teardrop (a fender light part, not
        // an end lamp), the outer points inside the top surface at their own station.
        let y0 = yn - k.p.face_depth(true);
        let edge = |y: f32, inset: f32| -(k.p.half_width(y) - inset);
        let tear = [
            [edge(y0 - 0.01, 0.07), y0 - 0.01],
            [edge(y0 - 0.03, 0.32), y0 - 0.03],
            [edge(y0 - 0.17, 0.28), y0 - 0.17],
            [edge(y0 - 0.30, 0.12), y0 - 0.30],
            [edge(y0 - 0.22, 0.06), y0 - 0.22],
        ];
        let kt = sided(
            &lkey("headlamp_top_l", BodyRegion::FrontLeftFender, M::Headlight),
            sx,
        );
        proj_lens(&mut k, &kt, sx, LampView::Top, &tear, 0.0, 0.010);
    }

    // --- Slim grille slot between the lamps, badge in it.
    slot_grille(
        &mut k,
        "grille",
        BodyRegion::Hood,
        0.27,
        lb + 0.012,
        lt - 0.02,
        1,
        M::Trim,
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
        face_box(&mut k, &kb, true, -0.47, 0.47, z - 0.007, z + 0.007, 0.014);
    }
    round_fogs(&mut k, wn - 0.13, 0.5 * (zm0 + zm1), 0.042);
    let accent = dkey("accent_stripe", fb, M::Accent);
    face_box(
        &mut k,
        &accent,
        true,
        -0.52,
        0.52,
        zm1 + 0.012,
        zm1 + 0.026,
        0.016,
    );
    k.splitter(M::Trim, 0.035, 0.12, 0.022, false);
    face_box(
        &mut k,
        &accent,
        true,
        -wn + 0.13,
        wn - 0.13,
        nz0 + 0.008,
        nz0 + 0.026,
        0.03,
    );

    // --- Rear: tall tri-colour clusters at the corners (red over amber over a clear reverse
    // lens), cast round onto the quarters.
    let zh = tz1 - 0.065;
    let zl = zh - 0.30;
    for sx in SIDES {
        let cl = [
            [-(wt - 0.03), zh],
            [-(wt - 0.21), zh - 0.012],
            [-(wt - 0.21), zl + 0.062],
            [-(wt - 0.03), zl + 0.062],
        ];
        k.proj_cluster(
            sx,
            false,
            LampView::Rear,
            &cl,
            Some(Seg::Lower(0.09)),
            M::EmissiveAmber,
            0.010,
            25.0,
        );
        let rev = [
            [-(wt - 0.03), zl + 0.05],
            [-(wt - 0.21), zl + 0.05],
            [-(wt - 0.21), zl],
            [-(wt - 0.03), zl],
        ];
        let kc = sided(
            &lkey("reverse_lamp_l", BodyRegion::RearLeftQuarter, M::Headlight),
            sx,
        );
        proj_lens(&mut k, &kc, sx, LampView::Rear, &rev, 25.0, 0.008);
    }
    // Black garnish across the tailgate between the clusters, badge on it.
    let kg = dkey("hatch_garnish", BodyRegion::Trunk, M::Trim);
    face_box(
        &mut k,
        &kg,
        false,
        -wt + 0.26,
        wt - 0.26,
        zh - 0.10,
        zh - 0.01,
        0.012,
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
    face_box(
        &mut k,
        &kv,
        false,
        -wt + 0.06,
        wt - 0.06,
        tz0 + 0.02,
        tz0 + 0.10,
        0.012,
    );
    rear_reflectors(&mut k, tz0 + 0.14, 0.05);

    // --- Sides (no skirt: the sill tucks under): mirrors, repeaters, black handles, filler.
    k.mirrors(0.10, 0.11, None);
    k.fender_repeaters();
    k.door_handles(M::Trim);
    k.fuel_filler(1.0, 0.10, M::Paint);
}

// ---------------------------------------------------------------------------- stoat

/// Early-90s European FF 16-valve hot hatch (`stoat_16v`): upright and boxy, crisp edges,
/// a full-width black front band (square twin-lamp housings either side of a slatted grille
/// between them), black bumpers wrapping to the arches and black rubbing strips with a yellow insert,
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
    // The squarest of the four: near-flat sides with a small bulge high up, little tumblehome,
    // barely any plan taper, a flat bonnet between slightly raised fender tops.
    let b = &mut s.body;
    b.plan_width = plan_taper(def, 0.96, 0.98, 1.0);
    b.side_bulge_m = 0.012;
    b.side_bulge_z = 0.75;
    b.rocker_in_m = 0.02;
    b.tumblehome_m = Some(0.10);
    b.front_fender_rise_m = 0.02;
    b.fender_peak_w = 0.86;
    // Upright nose (the boxy identity), a slightly leaning tailgate with a small lip under
    // the hatch glass, a near-square roof; it keeps its small quarter window.
    b.tail_face_rake_deg = 8.0;
    b.quarter_glass = true;
    b.glass_base_lip_m = 0.02;
    b.roof_plan_taper = 0.04;
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
    // the corner, slatted grille between them.
    let (zt, zb) = (nz1 - 0.025, nz1 - 0.175);
    let xi = 0.19;
    let xo = wn - 0.04;
    let band = dkey("front_band", BodyRegion::Hood, M::Trim);
    face_box(&mut k, &band, true, -xo, xo, zb - 0.012, zt + 0.012, 0.010);
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
    slot_grille(
        &mut k,
        "grille",
        BodyRegion::Hood,
        xi - 0.015,
        zb + 0.01,
        zt - 0.01,
        3,
        M::Trim,
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

    // --- Black front bumper with a lower slot, fogs, and side wraps to the arches.
    let (bz0, bz1) = (nz0 + 0.02, nz0 + 0.24);
    let kb = dkey("bumper_band", fb, M::Trim);
    face_box(&mut k, &kb, true, -wn - 0.005, wn + 0.005, bz0, bz1, 0.035);
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
    slot_grille(
        &mut k,
        "lower_slot",
        fb,
        0.36,
        bz0 + 0.05,
        bz0 + 0.13,
        1,
        M::Trim,
    );
    for sx in SIDES {
        let kf = sided(&lkey("fog_l", fb, M::Headlight), sx);
        face_box(
            &mut k,
            &kf,
            true,
            sx * (wn - 0.20),
            sx * (wn - 0.08),
            bz0 + 0.06,
            bz0 + 0.12,
            0.045,
        );
    }

    // --- Sides: black rubbing strips continuing the bumper line, black mirror caps.
    side_strip(&mut k, "rub_strip", bz1 - 0.07, bz1 - 0.01, M::Trim, 0.016);
    // Yellow insert line along the strips (the car's accent).
    side_strip(
        &mut k,
        "rub_insert",
        bz1 - 0.045,
        bz1 - 0.035,
        M::Accent,
        0.022,
    );
    k.mirrors(0.10, 0.11, Some(M::Trim));
    k.fender_repeaters();
    k.door_handles(M::Trim);
    k.fuel_filler(1.0, 0.10, M::Paint);

    // --- Rear: wide ribbed horizontal lamps (red, amber outer, clear inner strip), black
    // plate panel between them, black bumper with wraps, small black roof spoiler.
    let (lz0, lz1) = (tz1 - 0.21, tz1 - 0.05);
    let (lxi, lxo) = (0.30, wt - 0.045);
    let kp = dkey("tail_panel", BodyRegion::Trunk, M::Trim);
    face_box(&mut k, &kp, false, -lxi + 0.01, lxi - 0.01, lz0, lz1, 0.010);
    rear_plate(&mut k, BodyRegion::Trunk, 0.5 * (lz0 + lz1));
    for sx in SIDES {
        let cl = [
            [-(lxi + 0.07), lz1],
            [-(wt - 0.02), lz1],
            [-(wt - 0.02), lz0],
            [-(lxi + 0.07), lz0],
        ];
        k.proj_cluster(
            sx,
            false,
            LampView::Rear,
            &cl,
            Some(Seg::Outboard(0.10)),
            M::EmissiveAmber,
            0.008,
            20.0,
        );
        let kc = sided(
            &lkey("reverse_lamp_l", BodyRegion::RearLeftQuarter, M::Headlight),
            sx,
        );
        face_box(
            &mut k,
            &kc,
            false,
            sx * lxi,
            sx * (lxi + 0.06),
            lz0,
            lz1,
            0.022,
        );
        let ribs = sided(
            &dkey("taillight_ribs_l", BodyRegion::RearLeftQuarter, M::Trim),
            sx,
        );
        for f in [0.36, 0.66] {
            let z = lz0 + f * (lz1 - lz0);
            face_box(
                &mut k,
                &ribs,
                false,
                sx * (lxi + 0.07),
                sx * (lxo - 0.12),
                z - 0.004,
                z + 0.004,
                0.026,
            );
        }
    }
    let (rz0, rz1) = (tz0 + 0.02, tz0 + 0.22);
    let krb = dkey("rear_bumper_band", rb, M::Trim);
    face_box(
        &mut k,
        &krb,
        false,
        -wt - 0.005,
        wt + 0.005,
        rz0,
        rz1,
        0.035,
    );
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
    // Wedge: pointed nose in plan, sides leaning in above the moulding (bulge low), a lean
    // greenhouse, tail tapered.
    let b = &mut s.body;
    b.plan_width = plan_taper(def, 0.90, 0.92, 1.0);
    // Pointed wedge nose: strongly raked face with a chin, plan bow to a narrow tip.
    b.nose_face_rake_deg = 28.0;
    b.nose_chin_m = 0.06;
    b.nose_plan_bow_m = 0.30;
    b.nose_tip_w = 0.50;
    b.tail_face_rake_deg = 18.0;
    b.roof_plan_taper = 0.12;
    b.glass_base_lip_m = 0.03;
    b.quarter_glass = false;
    b.side_bulge_m = 0.022;
    b.side_bulge_z = 0.42;
    b.rocker_in_m = 0.035;
    b.tumblehome_m = Some(0.19);
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
    // Face width just behind the plan bow (the face itself is narrower; projected parts
    // follow the curve).
    let wn = k.p.half_width(k.yn - 0.12);
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;

    // --- Slim wedge lamps nearly meeting in the middle, cast round onto the fenders, amber
    // signal at the outer end, thin black bezel; a black slot and badge between them.
    let (lt, lb) = (nz1 - 0.034, nz1 - 0.092);
    let xi = 0.14;
    for sx in SIDES {
        let wedge = [
            [-xi, lt],
            [-(wn - 0.02), lt],
            [-(wn - 0.02), lb + 0.025],
            [-(wn - 0.08), lb],
            [-(xi + 0.03), lb],
        ];
        k.proj_cluster(
            sx,
            true,
            LampView::Front,
            &wedge,
            Some(Seg::Outboard(0.11)),
            M::EmissiveAmber,
            0.008,
            25.0,
        );
    }
    slot_grille(
        &mut k,
        "grille",
        BodyRegion::Hood,
        xi - 0.02,
        lb + 0.008,
        lt - 0.012,
        0,
        M::Trim,
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
    face_box(
        &mut k,
        &kbar,
        true,
        -0.55,
        0.55,
        zc - 0.016,
        zc + 0.016,
        0.02,
    );
    for sx in SIDES {
        let kf = sided(&lkey("fog_l", fb, M::Headlight), sx);
        face_box(
            &mut k,
            &kf,
            true,
            sx * (wn - 0.20),
            sx * (wn - 0.07),
            zi0 + 0.03,
            zi1 - 0.03,
            0.016,
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
    let lxi = 0.25;
    for sx in SIDES {
        let band = [
            [-lxi, lz1],
            [-(wt - 0.015), lz1],
            [-(wt - 0.015), lz0],
            [-lxi, lz0],
        ];
        k.proj_cluster(
            sx,
            false,
            LampView::Rear,
            &band,
            Some(Seg::Inboard(0.10)),
            M::EmissiveAmber,
            0.0,
            30.0,
        );
        let rev = [
            [-(lxi + 0.13), lz0 + 0.045],
            [-(lxi + 0.23), lz0 + 0.045],
            [-(lxi + 0.23), lz0 + 0.012],
            [-(lxi + 0.13), lz0 + 0.012],
        ];
        let kc = sided(
            &lkey("reverse_lamp_l", BodyRegion::RearLeftQuarter, M::Headlight),
            sx,
        );
        proj_lens(&mut k, &kc, sx, LampView::Rear, &rev, 30.0, 0.014);
    }
    let kpanel = dkey("tail_reflector_panel", BodyRegion::Trunk, M::Taillight);
    face_box(
        &mut k,
        &kpanel,
        false,
        -lxi + 0.008,
        lxi - 0.008,
        lz0 + 0.01,
        lz1 - 0.01,
        0.014,
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
    face_box(
        &mut k,
        &kv,
        false,
        -wt + 0.05,
        wt - 0.05,
        tz0 + 0.02,
        tz0 + 0.12,
        0.014,
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
/// silver mirror caps. Blue with red side-duct blades and dark turbines.
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
    // Muscular and wide: fenders wider than the doors (waist), a crisp shoulder with the lower
    // door drawn in under it, rear haunches over the arches, fender tops above the hood.
    // Heights from the styled profile: sill ≈ 0.15 m, belt ≈ 0.875 m.
    let (fy, fr, ry, rr) = axles(def);
    let b = &mut s.body;
    b.plan_width = plan_taper(def, 0.90, 0.93, 0.965);
    b.side_bulge_m = 0.02;
    b.side_bulge_z = 0.7;
    b.rocker_in_m = 0.035;
    b.tumblehome_m = Some(0.19);
    b.front_fender_rise_m = 0.025;
    b.rear_fender_rise_m = 0.015;
    b.fender_peak_w = 0.80;
    // Modern raked bumper with an undercut, slight bow; leaning tailgate; roof tapering back.
    b.nose_face_rake_deg = 22.0;
    b.nose_chin_m = 0.05;
    b.nose_plan_bow_m = 0.22;
    b.nose_tip_w = 0.70;
    b.tail_face_rake_deg = 20.0;
    b.roof_plan_taper = 0.12;
    b.glass_base_lip_m = 0.03;
    b.quarter_glass = false;
    // Window line kicking up at the rear of the rear-door glass.
    b.dlo_kink_m = 0.06;
    b.dlo_kink_len_m = 0.20;
    b.side_sculpt = vec![
        // Shoulder line along the whole side, 8 cm under the belt.
        SideSculpt {
            y0_m: ry - rr - 0.4,
            y1_m: fy + fr + 0.4,
            z0_m: 0.775,
            z1_m: 0.805,
            dx_m: 0.02,
            falloff_m: 0.035,
        },
        // Lower doors drawn in between the arches.
        SideSculpt {
            y0_m: ry + rr + 0.15,
            y1_m: fy - fr - 0.15,
            z0_m: 0.30,
            z1_m: 0.58,
            dx_m: -0.025,
            falloff_m: 0.12,
        },
        // Rear haunch over the arch.
        SideSculpt {
            y0_m: ry - rr,
            y1_m: ry + rr,
            z0_m: 0.55,
            z1_m: 0.76,
            dx_m: 0.02,
            falloff_m: 0.18,
        },
    ];
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
    let yt = k.yt;
    // Face width just behind the plan bow (the face itself is narrower; projected parts
    // follow the curve).
    let wn = k.p.half_width(k.yn - 0.12);
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;

    // --- Front: slim grille whose bars run out to the lamps, swept LED head lamps cast round
    // onto the fenders (amber outer end, black bezel) with a DRL brow along their top edge.
    let (gz0, gz1) = (nz1 - 0.10, nz1 - 0.035);
    slot_grille(
        &mut k,
        "grille",
        BodyRegion::Hood,
        0.44,
        gz0,
        gz1,
        0,
        M::Trim,
    );
    let kbar = dkey("grille_bars", BodyRegion::Hood, M::Trim);
    for f in [0.33, 0.66] {
        let z = gz0 + f * (gz1 - gz0);
        face_bar(
            &mut k,
            &kbar,
            true,
            -0.47,
            0.47,
            z - 0.006,
            z + 0.006,
            0.016,
        );
    }
    badge(
        &mut k,
        BodyRegion::Hood,
        true,
        0.0,
        0.5 * (gz0 + gz1),
        0.10,
        0.05,
    );
    for sx in SIDES {
        let lamp = [
            [-0.47, gz1],
            [-(wn - 0.02), nz1 - 0.032],
            [-(wn - 0.02), nz1 - 0.075],
            [-(wn - 0.12), gz0 + 0.004],
            [-0.49, gz0 + 0.004],
        ];
        k.proj_cluster(
            sx,
            true,
            LampView::Front,
            &lamp,
            Some(Seg::Outboard(0.10)),
            M::EmissiveAmber,
            0.010,
            30.0,
        );
        let drl = sided(
            &lkey("drl_l", BodyRegion::FrontLeftFender, M::Headlight),
            sx,
        );
        let brow = [
            [-0.49, gz1 - 0.004],
            [-(wn - 0.13), nz1 - 0.036],
            [-(wn - 0.13), nz1 - 0.048],
            [-0.49, gz1 - 0.016],
        ];
        proj_lens(&mut k, &drl, sx, LampView::Front, &brow, 30.0, 0.014);
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
        face_bar(&mut k, &km, true, -0.38, 0.38, z - 0.005, z + 0.005, 0.012);
    }
    for i in 1..8 {
        let x = -0.36 + 0.72 * i as f32 / 8.0;
        face_bar(
            &mut k,
            &km,
            true,
            x - 0.005,
            x + 0.005,
            zi0 + 0.01,
            zi1 - 0.01,
            0.012,
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
        let kb = sided(&dkey("side_duct_blade_l", fb, M::Accent), sx);
        for x in [0.56, 0.66] {
            face_box(
                &mut k,
                &kb,
                true,
                sx * (x - 0.008),
                sx * (x + 0.008),
                zi0 + 0.03,
                zi1 - 0.05,
                0.02,
            );
        }
    }
    k.splitter(M::Trim, 0.05, 0.10, 0.02, false);

    // --- Rear: two-piece wrap-around LED lamps with a light-bar signature, black garnish
    // with badge between them, diffuser, big roof spoiler with plates and a brake lamp.
    let (z0, z1) = (tz1 - 0.17, tz1 - 0.07);
    for sx in SIDES {
        let lamp = [
            [-0.30, z1],
            [-(wt - 0.015), z1],
            [-(wt - 0.015), z0 + 0.025],
            [-0.33, z0],
        ];
        k.proj_cluster(
            sx,
            false,
            LampView::Rear,
            &lamp,
            Some(Seg::Inboard(0.10)),
            M::EmissiveAmber,
            0.010,
            30.0,
        );
        // Light bar along the top of the lamp (the signature), over the lens.
        let ki = sided(
            &lkey("taillight_bar_l", BodyRegion::RearLeftQuarter, M::Taillight),
            sx,
        );
        let bar = [
            [-0.44, z1 - 0.018],
            [-(wt - 0.05), z1 - 0.018],
            [-(wt - 0.05), z1 - 0.030],
            [-0.44, z1 - 0.030],
        ];
        proj_lens(&mut k, &ki, sx, LampView::Rear, &bar, 30.0, 0.014);
    }
    let kg = dkey("hatch_garnish", BodyRegion::Trunk, M::Trim);
    face_box(&mut k, &kg, false, -0.27, 0.27, z0 + 0.02, z1 - 0.02, 0.010);
    badge(
        &mut k,
        BodyRegion::Trunk,
        false,
        0.0,
        0.5 * (z0 + z1),
        0.10,
        0.045,
    );
    rear_diffuser(&mut k, M::Trim, 0.62, 0.16, 3, 0.05);
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
    k.mirrors(0.20, 0.12, Some(M::Metal));
    k.mirror_repeaters(0.20, 0.12, true);
    k.door_handles(M::Paint);
    k.fuel_filler(1.0, 0.11, M::Paint);
}
