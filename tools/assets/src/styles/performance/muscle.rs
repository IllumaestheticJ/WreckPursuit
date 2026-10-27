//! Muscle: `ironside_440` (early-70s big-block hardtop coupe), `rampart_50` (2010s muscle
//! coupe), `bulwark_400` (early-70s full-size four-door muscle sedan) and `brigand_400`
//! (mid-70s full-size four-door sedan).
//!
//! Genre cues (research §3, car-shapes phase 2): long hood + set-back cabin, tall slab sides
//! under a short greenhouse (high belt), nose-down rake with fat rear haunches, hood scoops /
//! power domes, wide horizontal (70s) or vertical-bar (2010s) tail lamps, chrome bumper
//! blades and bright trim (70s) or black mesh, splitters and stripes (2010s).
//!
//! Every car has its own body knobs and kit; the two 70s sedans share one kit
//! ([`sedan70_kit`]) with a [`Sedan70`] variant for the period differences (round lamps and a
//! horizontal grille early in the decade; stacked square lamps, an upright grille, 5-mph
//! bumpers and a landau roof later).

use wp_contracts::{BodyRegion, VehicleDef};

use crate::builder::{Builder, PartKey, PatchBand};
use crate::lamp::{self, Lamp, LampView};
use crate::mesh::MaterialSlot as M;
use crate::style::{
    ArchShape, BodyStyle, CaliperKind, CapKind, CarStyle, DetailCtx, EngineKind, ExhaustKind,
    FuelDoor, HoodScoop, Induction, RimKind, SideSculpt, Silhouette, SuspensionKind, TrimFinish,
};

use super::kit::{
    dkey, headlight_key, indicator_front_key, indicator_rear_key, lkey, sided, taillight_key, Kit,
    Seg, SIDES,
};
use super::{kit_detail, rgb, wheels};

/// Wheel segments for the 70s cars: tall-sidewall bias-ply / radial tyres on 14–15" rims read
/// round at 18 segments, and the saving pays for the chrome trim and the sculpted body (hero
/// budget 22k).
const SEG_70S: u8 = 18;

// ---------------------------------------------------------------------------- local helpers

/// Front and rear axle stations (y) and the front wheel radius, m.
fn axles(def: &VehicleDef) -> (f32, f32, f32) {
    let ys = def.wheels.iter().map(|w| w.position_m[1]);
    let fy = ys.clone().fold(f32::MIN, f32::max);
    let ry = ys.fold(f32::MAX, f32::min);
    let r = def.wheels.first().map(|w| w.radius_m).unwrap_or(0.33);
    (fy, ry, r)
}

/// Shoulder / character line the whole length of the car: a band `z0..z1` standing `dx` out.
fn char_line(def: &VehicleDef, z0: f32, z1: f32, dx: f32) -> SideSculpt {
    let (fy, ry, r) = axles(def);
    SideSculpt {
        y0_m: ry - r - 1.2,
        y1_m: fy + r + 1.2,
        z0_m: z0,
        z1_m: z1,
        dx_m: dx,
        falloff_m: 0.035,
    }
}

/// Pontoon over the rear wheel (coke-bottle hip): `z0..z1` standing `dx` out, ±`half` along y.
fn rear_pontoon(def: &VehicleDef, half: f32, z0: f32, z1: f32, dx: f32) -> SideSculpt {
    let (_, ry, _) = axles(def);
    SideSculpt {
        y0_m: ry - half,
        y1_m: ry + half,
        z0_m: z0,
        z1_m: z1,
        dx_m: dx,
        falloff_m: 0.2,
    }
}

/// Forward-facing hood scoop on `region`'s top surface: a body-colour wedge centred at signed
/// `xc`, `hw` half wide, flush at `y_back` and `h` tall at its open front `y_front`, with a
/// dark intake mouth set into the front face.
#[allow(clippy::too_many_arguments)]
fn hood_scoop(
    k: &mut Kit,
    name: &str,
    region: BodyRegion,
    xc: f32,
    hw: f32,
    y_back: f32,
    y_front: f32,
    h: f32,
) {
    let top = |k: &Kit, y: f32| {
        k.top_z(y, xc - hw)
            .min(k.top_z(y, xc + hw))
            .min(k.top_z(y, xc))
    };
    let zb = top(k, y_back);
    let zf = top(k, y_front);
    let body = dkey(name, region, M::Paint);
    let prof = [
        [y_back, zb - 0.012],
        [y_front, zf - 0.012],
        [y_front, zf + h],
        [y_front - 0.04, zf + h + 0.004],
        [y_back, zb + 0.006],
    ];
    k.prism_x(&body, &prof, xc - hw, xc + hw);
    let mouth = dkey(&format!("{name}_mouth"), region, M::Grille);
    let mprof = [
        [y_front + 0.003, zf + 0.008],
        [y_front + 0.003, zf + h - 0.012],
        [y_front - 0.05, zf + h - 0.016],
        [y_front - 0.05, zf + 0.006],
    ];
    k.prism_x(&mouth, &mprof, xc - hw + 0.018, xc + hw - 0.018);
}

/// Period side-marker lamps (both sides): an amber lens on the front fender behind the nose
/// and a red one on the rear quarter ahead of the tail, `len` long, at `dz` below the belt.
fn side_markers(k: &mut Kit, len: f32, dz: f32, bezel: Option<M>) {
    let yf = k.yn - 0.16;
    let yr = k.yt + 0.16;
    for sx in SIDES {
        for (front, y) in [(true, yf), (false, yr)] {
            let z = k.p.belt(y) - dz;
            let (name, region, mat) = if front {
                (
                    "side_marker_front_l",
                    BodyRegion::FrontLeftFender,
                    M::EmissiveAmber,
                )
            } else {
                (
                    "side_marker_rear_l",
                    BodyRegion::RearLeftQuarter,
                    M::Taillight,
                )
            };
            if let Some(bm) = bezel {
                let kb = sided(&dkey(&format!("{name}_bezel"), region, bm), sx);
                k.side_box(&kb, sx, y, z, len + 0.02, 0.055, 0.008, 0.0);
            }
            let kl = sided(&dkey(name, region, mat), sx);
            k.side_box(&kl, sx, y, z, len, 0.035, 0.012, 0.0);
        }
    }
}

/// Bright side moulding (both sides) from `y0` to `y1` at height `z` (strip `h` tall).
#[allow(clippy::too_many_arguments)]
fn side_moulding(
    k: &mut Kit,
    name: &str,
    region: BodyRegion,
    mat: M,
    y0: f32,
    y1: f32,
    z: f32,
    h: f32,
    off: f32,
) {
    let key = dkey(name, region, mat);
    for sx in SIDES {
        k.side_patch(&key, sx, y0, y1, z, z + h, off, 0.006, (12, 1));
    }
}

/// Small blank badge (chrome disc standing out of the nose or tail face) at (x, z).
fn face_badge(k: &mut Kit, name: &str, region: BodyRegion, front: bool, x: f32, z: f32, r: f32) {
    let key = dkey(name, region, M::Chrome);
    let (y, d) = if front { (k.yn, 1.0) } else { (k.yt, -1.0) };
    k.cyl_y(&key, x, z, y - d * 0.01, y + d * 0.03, r, 12);
}

// ---------------------------------------------------------------------------- ironside_440

/// Early-70s big-block hardtop coupe (`ironside_440`): long flat hood with twin forward
/// scoops, set-back semi-fastback cabin with no B-pillar (hardtop), coke-bottle hips, full-width
/// grille with quad round lamps behind a chrome blade, full-width tail lamps, chrome drip rail
/// and window trim, vinyl roof, straight body-side stripe, slot mags.
pub fn ironside(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: -0.42,
        nose_h: Some(0.72),
        nose_rake_deg: 3.0,
        nose_plan_round: 0.10,
        tail_plan_round: 0.12,
        hood_crown: 0.010,
        hood_bulge_h: 0.0,
        hood_bulge_w: 0.40,
        power_bulge_h: 0.025,
        hood_scoop: HoodScoop::None,
        windshield_rake_deg: Some(57.0),
        a_pillar_rake_deg: 6.0,
        rear_glass_rake_deg: Some(61.0),
        roof_len_trim: 0.15,
        roof_dz: -0.05,
        roof_crown: 0.015,
        tumblehome: Some(0.17),
        belt_dz: 0.06,
        crease_z: 0.56,
        crease_depth: 0.012,
        shoulder_round: 0.03,
        haunch: 0.050,
        front_haunch: 0.035,
        flare: 0.010,
        arch_gap: 0.065,
        stance_pitch_deg: 0.8,
        ducktail_h: 0.035,
        tail_round: 0.14,
        tail_drop: 0.03,
        belt_kick: 0.06,
        // Hardtop: no B-pillar, only the glass seal between door and quarter glass.
        b_pillar_w_m: 0.05,
        a_pillar_w_m: 0.07,
        c_pillar_w_m: 0.16,
        drip_rail: TrimFinish::Chrome,
        windshield_wrap_m: 0.05,
        hood_valley: 0.0,
        ride_dz: 0.01,
        overhang_trim: (0.05, 0.06),
        sill_tuck: 0.05,
        deck_drop: 0.02,
        ..BodyStyle::preset(Silhouette::Notchback)
    });
    s.details.push(kit_detail(ironside_kit));
    // Coke bottle: full width over both wheels, pinched at the doors, a slightly narrower nose
    // and tail; fender tops above the hood, haunches above the deck; pontoon hips and a
    // shoulder line; a gently curved side section.
    let (fy, ry, r) = axles(def);
    let b = &mut s.body;
    b.plan_width = vec![
        [ry - r - 0.75, 0.95],
        [ry - r - 0.05, 1.0],
        [ry + r + 0.10, 1.0],
        [0.5 * (fy + ry), 0.935],
        [fy - r - 0.10, 1.0],
        [fy + r + 0.05, 1.0],
        [fy + r + 0.85, 0.96],
    ];
    b.front_fender_rise_m = 0.05;
    b.rear_fender_rise_m = 0.035;
    b.fender_peak_w = 0.85;
    b.side_sculpt = vec![
        rear_pontoon(def, 0.40, 0.56, 0.86, 0.035),
        char_line(def, 0.735, 0.765, 0.016),
    ];
    b.side_bulge_m = 0.02;
    b.rocker_in_m = 0.03;
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
    s.wheel.segments = SEG_70S;
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

    // --- Front: chrome blade, full-width recessed grille with fine bright bars and a blank
    // badge, quad round lamps in chrome bezels, black valance.
    let zb = nz0 + 0.40 * (nz1 - nz0);
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
            [sx * (w - 0.03), yn - 0.26, zb - 0.06],
            [sx * (w + 0.02), yn + 0.02, zb + 0.035],
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
        5,
        M::Chrome,
        Some(M::Chrome),
    );
    face_badge(
        &mut k,
        "grille_badge",
        fb,
        true,
        0.0,
        0.5 * (gz0 + gz1),
        0.03,
    );
    let zl = 0.5 * (gz0 + gz1);
    let r = (0.5 * (gz1 - gz0) - 0.01).min(0.08);
    for sx in SIDES {
        let bez = sided(
            &dkey("headlamp_bezel_l", BodyRegion::FrontLeftFender, M::Chrome),
            sx,
        );
        for x in [wn - 0.17, wn - 0.17 - 2.3 * r] {
            k.round_lamp(&headlight_key(sx), true, sx * x, zl, r, 0.03, Some(&bez));
        }
        // Rectangular amber parking / turn lens in the grille inboard of the lamp pair.
        let x_in = wn - 0.17 - 2.3 * r - r - 0.016 - 0.014;
        let ka = indicator_front_key(sx);
        k.front_box(
            &ka,
            sx * (x_in - 0.10),
            sx * x_in,
            zl - 0.5 * r,
            zl + 0.5 * r,
            0.024,
            0.006,
        );
    }
    let val = dkey("front_valance", fb, M::Grille);
    k.front_box(&val, -0.60, 0.60, nz0 + 0.05, zb - 0.08, 0.005, 0.0);
    k.hood_pins(yn - 0.14, 0.62);

    // --- Hood: twin forward-facing scoops on the power bulge.
    let span = yn - p.y_fw;
    for sx in SIDES {
        hood_scoop(
            &mut k,
            if sx < 0.0 {
                "hood_scoop_l"
            } else {
                "hood_scoop_r"
            },
            BodyRegion::Hood,
            sx * 0.25,
            0.12,
            yn - 0.62 * span,
            yn - 0.36 * span,
            0.055,
        );
    }

    // --- Rear: chrome blade, recessed black lamp panel, full-width lamps (three segments a
    // side with chrome dividers), centre badge, reverse lamps.
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
            [sx * (w - 0.03), yt + 0.24, zbr - 0.055],
            [sx * (w + 0.02), yt - 0.02, zbr + 0.035],
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
        let (xa, xb) = (sx * 0.13, sx * (wt - 0.06));
        k.rear_box(&kb, xa, xb, pz0 + 0.012, pz1 - 0.012, 0.018, 0.006);
        let ka = indicator_rear_key(sx, M::Taillight);
        let n = 3;
        for i in 0..n {
            let f0 = i as f32 / n as f32;
            let f1 = (i + 1) as f32 / n as f32;
            let x0 = xa + (xb - xa) * f0 + sx * 0.008;
            let x1 = xa + (xb - xa) * f1 - sx * 0.008;
            let key = if i == n - 1 { &ka } else { &tk };
            k.rear_box(key, x0, x1, pz0 + 0.025, pz1 - 0.025, 0.026, 0.004);
        }
    }
    // Reverse lamps set into the bumper blade either side of the plate.
    let kr = dkey("reverse_lamps", rb, M::Headlight);
    for sx in SIDES {
        let x = sx * 0.30;
        k.rear_box(
            &kr,
            x - 0.06,
            x + 0.06,
            zbr - 0.03,
            zbr + 0.012,
            0.072,
            0.004,
        );
    }
    let plate = dkey("plate_rear", rb, M::Grille);
    k.rear_box(&plate, -0.16, 0.16, zbr - 0.045, zbr + 0.025, 0.069, 0.0);

    // --- Sides: chrome rocker strip, black body-side stripe, side markers, vinyl roof.
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
        let zc = 0.60 * p.roof.2;
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
    }
    side_markers(&mut k, 0.07, 0.10, Some(M::Chrome));
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

    // Chrome bullet mirrors, chrome handles, chrome filler cap.
    k.mirrors(0.12, 0.12, Some(M::Chrome));
    k.door_handles(M::Chrome);
    k.fuel_filler(1.0, 0.12, M::Chrome);
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

// ---------------------------------------------------------------------------- rampart_50

/// 2010s muscle coupe (`rampart_50`): long hood with a power dome and heat extractors,
/// short chopped greenhouse on a high belt, fastback roof into a short ducktail deck, wide
/// rear hips, trapezoid mesh grille with a blank badge, angular headlamps with projector and
/// running-light blade, splitter, fake quarter scoops, split wedge tail lamps in a black panel,
/// diffuser, quad tips, twin over-the-top stripes, big split-spoke wheels.
pub fn rampart(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        silhouette: Silhouette::Fastback,
        cabin_shift_m: -0.25,
        nose_h: Some(0.66),
        nose_rake_deg: 8.0,
        nose_plan_round: 0.28,
        tail_plan_round: 0.22,
        hood_crown: 0.020,
        hood_bulge_h: 0.035,
        hood_bulge_w: 0.30,
        windshield_rake_deg: Some(62.0),
        a_pillar_rake_deg: 10.0,
        roof_len_trim: 0.0,
        roof_dz: -0.07,
        roof_arc: 0.5,
        belt_dz: 0.08,
        tumblehome: Some(0.24),
        crease_z: 0.62,
        crease_depth: 0.018,
        sill_tuck: 0.09,
        haunch: 0.045,
        front_haunch: 0.030,
        flare: 0.012,
        flare_shape: ArchShape::Round,
        arch_gap: 0.06,
        ride_dz: -0.01,
        stance_pitch_deg: 0.2,
        ducktail_h: 0.035,
        overhang_trim: (-0.06, -0.08),
        belt_kick: 0.05,
        rear_glass_bow: 0.03,
        tail_round: 0.24,
        tail_drop: 0.06,
        a_pillar_w_m: 0.08,
        c_pillar_w_m: 0.14,
        b_pillar_w_m: 0.08,
        mirrors: TrimFinish::Black,
        shoulder_round: 0.07,
        hood_valley: 0.0,
        deck_valley: 0.0,
        ..BodyStyle::preset(Silhouette::Fastback)
    });
    s.details.push(kit_detail(rampart_kit));
    // Narrow shark nose, waist at the doors, big hips; fender peaks front and rear; a rising
    // shoulder line, a cove low on the doors, a rounded section tucked in at the sill.
    let (fy, ry, r) = axles(def);
    let b = &mut s.body;
    b.plan_width = vec![
        [ry - r - 0.50, 0.94],
        [ry - r - 0.05, 1.0],
        [ry + r + 0.10, 1.0],
        [0.5 * (fy + ry), 0.95],
        [fy - r - 0.10, 1.0],
        [fy + r, 1.0],
        [fy + r + 0.55, 0.90],
    ];
    b.front_fender_rise_m = 0.035;
    b.rear_fender_rise_m = 0.05;
    b.fender_peak_w = 0.82;
    b.side_sculpt = vec![
        rear_pontoon(def, 0.35, 0.60, 0.88, 0.03),
        char_line(def, 0.78, 0.81, 0.02),
        SideSculpt {
            y0_m: ry + r + 0.15,
            y1_m: fy - r - 0.15,
            z0_m: 0.47,
            z1_m: 0.62,
            dx_m: -0.035,
            falloff_m: 0.10,
        },
    ];
    b.side_bulge_m = 0.03;
    b.side_bulge_z = 0.6;
    b.rocker_in_m = 0.045;
    b.tumblehome_m = Some(0.26);
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

    // --- Front: wide upper grille (trapezoid read: frame narrower at the bottom), lower
    // mouth, splitter, angular lamps with running-light blades, blank badge.
    let gz0 = nz1 - 0.20;
    let gz1 = nz1 - 0.03;
    k.mesh_grille("grille", fb, 0.50, gz0, gz1, 3, 8, M::Trim);
    let frame = dkey("grille_frame", fb, M::Trim);
    k.front_box(&frame, -0.54, 0.54, gz1, gz1 + 0.022, 0.02, 0.005);
    k.front_box(&frame, -0.46, 0.46, gz0 - 0.02, gz0, 0.02, 0.005);
    for sx in SIDES {
        // Raked sides of the trapezoid: three steps from the top corner in to the bottom.
        for i in 0..3 {
            let f = i as f32 / 3.0;
            let x = 0.54 - 0.08 * (f + 1.0 / 6.0);
            let z0 = gz1 - (gz1 - gz0) * (f + 1.0 / 3.0);
            let z1 = gz1 - (gz1 - gz0) * f;
            k.front_box(&frame, sx * (x - 0.03), sx * x, z0, z1, 0.02, 0.0);
        }
    }
    face_badge(
        &mut k,
        "grille_badge",
        fb,
        true,
        0.0,
        0.5 * (gz0 + gz1),
        0.04,
    );
    k.mesh_grille(
        "lower_mouth",
        fb,
        0.60,
        nz0 + 0.07,
        nz0 + 0.19,
        2,
        9,
        M::Trim,
    );
    k.splitter(M::Carbon, 0.06, 0.12, 0.022, false);
    for sx in SIDES {
        // Angular projected lamp wrapping onto the fender: inner end drops toward the grille,
        // black bezel, amber outer segment; a running-light lens along the lower edge.
        let (xi, xo) = (0.48, wn - 0.02);
        let (zt, zb) = (nz1 - 0.03, nz1 - 0.17);
        let outline = [[xi, zt - 0.04], [xo, zt], [xo, zt - 0.08], [xi + 0.08, zb]];
        k.proj_cluster(
            sx,
            true,
            LampView::Front,
            &outline,
            Some(Seg::Outboard(0.10)),
            M::Taillight,
            0.018,
            25.0,
        );
        let drl = sided(
            &lkey("drl_l", BodyRegion::FrontLeftFender, M::Headlight),
            sx,
        );
        let blade = vec![
            [sx * (xi + 0.10), zb - 0.030],
            [sx * (xo - 0.04), zt - 0.110],
            [sx * (xo - 0.04), zt - 0.095],
            [sx * (xi + 0.10), zb - 0.015],
        ];
        let mut lb = Lamp::new(LampView::Front, blade);
        lb.yaw_deg = 20.0;
        lamp::lens(k.b, p, &drl, &lb);
        let fog = sided(&lkey("fog_l", fb, M::Headlight), sx);
        k.front_box(
            &fog,
            sx * (wn - 0.22),
            sx * (wn - 0.10),
            nz0 + 0.10,
            nz0 + 0.14,
            0.014,
            0.004,
        );
    }
    // Hood heat extractors beside the power dome.
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

    // --- Rear: black lamp panel, one split wedge lamp per side, centre badge, diffuser,
    // ducktail lip.
    let pz0 = tz1 - 0.19;
    let pz1 = tz1 - 0.025;
    k.rear_panel(
        "tail_panel",
        BodyRegion::Trunk,
        0.30,
        pz0 + 0.03,
        pz1 - 0.02,
        M::Grille,
    );
    for sx in SIDES {
        // One wedge lens a side, taller outboard and wrapping onto the quarter; its outer end
        // is the red-on-red turn signal.
        let (xi, xo) = (0.34, wt - 0.03);
        let lamp = [
            [xi, pz0 + 0.06],
            [xo, pz0 + 0.02],
            [xo, pz1 - 0.02],
            [xi, pz1 - 0.035],
        ];
        k.proj_cluster(
            sx,
            false,
            LampView::Rear,
            &lamp,
            Some(Seg::Outboard(0.11)),
            M::Taillight,
            0.025,
            25.0,
        );
    }
    face_badge(
        &mut k,
        "tail_badge",
        BodyRegion::Trunk,
        false,
        0.0,
        0.5 * (pz0 + pz1),
        0.045,
    );
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

    // --- Sides: fake scoop ahead of each rear wheel (dark intake under a body-colour lip).
    let (yr, zr, rr) = k.arch(false);
    let ys0 = yr + rr + 0.06;
    let ys1 = (ys0 + 0.30).min(p.y_fd_rear - 0.04);
    let zs = zr + 0.45 * rr;
    let kin = dkey("quarter_scoop_l", BodyRegion::RearLeftQuarter, M::Grille);
    let klip = dkey("quarter_scoop_lip_l", BodyRegion::RearLeftQuarter, M::Paint);
    for sx in SIDES {
        k.side_patch(&kin, sx, ys0, ys1, zs, zs + 0.07, 0.003, 0.006, (3, 1));
        let kl = sided(&klip, sx);
        k.side_box(
            &kl,
            sx,
            0.5 * (ys0 + ys1),
            zs + 0.08,
            ys1 - ys0,
            0.02,
            0.03,
            0.0,
        );
    }

    // --- Twin over-the-top stripes (hood, roof, deck) + skirts.
    let (r0, r1, _, _) = p.roof;
    for sx in SIDES {
        let (x0, x1) = (sx * 0.07, sx * 0.25);
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
    k.mirrors(0.10, 0.12, Some(M::Trim));
    k.door_handles(M::Trim);
    k.fuel_filler(-1.0, 0.11, M::Paint);
}

// ---------------------------------------------------------------------------- 70s sedans

/// Which 70s full-size sedan [`sedan70_kit`] draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sedan70 {
    /// Early 70s (`bulwark_400`): horizontal egg-crate grille between quad round lamps, slim
    /// chrome blades, full-width two-row tail lamps, full vinyl roof.
    Early,
    /// Mid 70s (`brigand_400`): upright chrome waterfall grille, stacked square quad lamps,
    /// 5-mph bumpers with rubber strips and guards, hood ornament, vertical corner tail lamps,
    /// landau half-vinyl roof with a chrome band.
    Mid,
}

/// Early-70s American full-size four-door muscle sedan (`bulwark_400`): long flat hood and
/// deck, pillared four-door greenhouse on a high belt, horizontal egg-crate grille with quad
/// round lamps, chrome blades, full-width two-row tail lamps, bright belt and rocker mouldings,
/// vinyl roof, turbine wheel covers, dark blue paint.
pub fn bulwark(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: -0.12,
        nose_h: Some(0.72),
        nose_rake_deg: 2.0,
        nose_plan_round: 0.10,
        tail_plan_round: 0.10,
        hood_crown: 0.012,
        power_bulge_h: 0.02,
        hood_bulge_w: 0.35,
        hood_scoop: HoodScoop::None,
        windshield_rake_deg: Some(58.0),
        a_pillar_rake_deg: 5.0,
        rear_glass_rake_deg: Some(57.0),
        roof_len_trim: 0.12,
        roof_dz: -0.04,
        roof_crown: 0.015,
        tumblehome: Some(0.17),
        belt_dz: 0.05,
        crease_z: 0.52,
        crease_depth: 0.010,
        shoulder_round: 0.02,
        haunch: 0.040,
        flare: 0.008,
        arch_gap: 0.07,
        stance_pitch_deg: 0.3,
        ducktail_h: 0.0,
        tail_round: 0.10,
        tail_drop: 0.02,
        belt_kick: 0.03,
        a_pillar_w_m: 0.07,
        c_pillar_w_m: 0.18,
        b_pillar_w_m: 0.09,
        drip_rail: TrimFinish::Chrome,
        fuel_door: FuelDoor::None,
        windshield_wrap_m: 0.05,
        overhang_trim: (0.09, 0.08),
        front_haunch: 0.025,
        hood_valley: 0.0,
        sill_tuck: 0.045,
        ..BodyStyle::preset(Silhouette::Notchback)
    });
    s.details.push(kit_detail(bulwark_kit));
    // Long slab with a gentle waist, hips over the rear wheels, fender tops just above the
    // hood, a crisp shoulder line.
    let (fy, ry, r) = axles(def);
    let b = &mut s.body;
    b.plan_width = vec![
        [ry - r - 0.85, 0.96],
        [ry - r - 0.05, 1.0],
        [ry + r + 0.10, 1.0],
        [0.5 * (fy + ry), 0.955],
        [fy - r - 0.10, 1.0],
        [fy + r + 0.90, 0.97],
    ];
    b.front_fender_rise_m = 0.03;
    b.rear_fender_rise_m = 0.025;
    b.fender_peak_w = 0.86;
    b.side_sculpt = vec![
        rear_pontoon(def, 0.45, 0.58, 0.86, 0.03),
        char_line(def, 0.77, 0.80, 0.014),
    ];
    b.side_bulge_m = 0.015;
    b.rocker_in_m = 0.025;
    wheels(
        &mut s,
        def,
        RimKind::Turbine,
        10,
        0.56,
        M::Chrome,
        CapKind::Centre,
        (0.02, 0.015),
        (0.0, 0.0),
    );
    s.wheel.segments = SEG_70S;
    seventies_mech(&mut s);
    s.tints.push((M::Paint, rgb(0x1E3A5F)));
    s
}

/// Mid-70s American full-size four-door sedan (`brigand_400`): the getaway land yacht. Long
/// hood with an ornament, formal upright roof with wide C-pillars and a landau half-vinyl top,
/// stacked square quad lamps beside an upright chrome grille, 5-mph bumpers with rubber strips
/// and guards, vertical corner tail lamps, dog-dish steel wheels, brown paint.
pub fn brigand(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: -0.06,
        nose_h: Some(0.78),
        nose_rake_deg: 0.0,
        nose_plan_round: 0.08,
        tail_plan_round: 0.08,
        hood_crown: 0.010,
        hood_scoop: HoodScoop::None,
        windshield_rake_deg: Some(57.0),
        a_pillar_rake_deg: 4.0,
        rear_glass_rake_deg: Some(48.0),
        roof_len_trim: 0.15,
        roof_dz: 0.03,
        roof_crown: 0.010,
        tumblehome: Some(0.12),
        belt_dz: 0.05,
        crease_z: 0.50,
        crease_depth: 0.008,
        shoulder_round: 0.015,
        haunch: 0.020,
        flare: 0.006,
        arch_gap: 0.08,
        stance_pitch_deg: 0.0,
        ducktail_h: 0.0,
        tail_round: 0.08,
        tail_drop: 0.015,
        belt_kick: 0.02,
        a_pillar_w_m: 0.075,
        c_pillar_w_m: 0.26,
        b_pillar_w_m: 0.10,
        b_pillar: TrimFinish::Body,
        drip_rail: TrimFinish::Chrome,
        fuel_door: FuelDoor::None,
        windshield_wrap_m: 0.04,
        rear_glass_wrap_m: 0.02,
        overhang_trim: (0.10, 0.10),
        sill_tuck: 0.035,
        ..BodyStyle::preset(Silhouette::Notchback)
    });
    s.details.push(kit_detail(brigand_kit));
    // Square slab: straight sides, ends barely tapered, squared fender tops standing above
    // the hood, one crisp mid-side character line.
    let (fy, ry, r) = axles(def);
    let b = &mut s.body;
    b.plan_width = vec![
        [ry - r - 0.95, 0.975],
        [ry - r - 0.10, 1.0],
        [fy + r + 0.10, 1.0],
        [fy + r + 0.95, 0.98],
    ];
    b.front_fender_rise_m = 0.04;
    b.fender_peak_w = 0.88;
    b.side_sculpt = vec![char_line(def, 0.60, 0.63, 0.018)];
    b.rocker_in_m = 0.02;
    wheels(
        &mut s,
        def,
        RimKind::DogDish,
        5,
        0.55,
        M::Chrome,
        CapKind::Centre,
        (0.02, 0.015),
        (0.0, 0.0),
    );
    s.wheel.segments = SEG_70S;
    seventies_mech(&mut s);
    s.tints.push((M::Paint, rgb(0x6B4A2B)));
    s
}

/// Big pushrod V8, live leaf-sprung axle, disc / drum brakes, dual exhaust.
fn seventies_mech(s: &mut CarStyle) {
    let m = &mut s.mechanical;
    m.engine = EngineKind::V8;
    m.induction = Induction::Natural;
    m.show_engine_detail = true;
    m.front_suspension = SuspensionKind::DoubleWishbone;
    m.rear_suspension = SuspensionKind::SolidAxleLeaf;
    m.front_brakes.caliper = CaliperKind::Floating;
    m.rear_brakes.caliper = CaliperKind::Drum;
    m.exhaust = ExhaustKind::Dual;
    m.exhaust_tip_r_m = 0.036;
}

fn bulwark_kit(ctx: &DetailCtx, key: &PartKey, b: &mut Builder) {
    sedan70_kit(ctx, key, b, Sedan70::Early);
}

fn brigand_kit(ctx: &DetailCtx, key: &PartKey, b: &mut Builder) {
    sedan70_kit(ctx, key, b, Sedan70::Mid);
}

fn sedan70_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder, v: Sedan70) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let (yn, yt) = (k.yn, k.yt);
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;
    let rb = BodyRegion::RearBumper;
    crate::trim::remove_generic(k.b, &crate::trim::GENERIC_FUEL_DOOR);

    // --- Front bumper.
    let mid = v == Sedan70::Mid;
    let zb = nz0 + if mid { 0.30 } else { 0.38 } * (nz1 - nz0);
    let (bh0, bh1, bproud) = if mid {
        (0.09, 0.05, 0.12)
    } else {
        (0.06, 0.035, 0.07)
    };
    let blade = dkey("bumper_blade_front", fb, M::Chrome);
    k.front_box(
        &blade,
        -wn - 0.02,
        wn + 0.02,
        zb - bh0,
        zb + bh1,
        bproud,
        0.02,
    );
    for sx in SIDES {
        let w = p.half_width(yn - 0.18) + 0.01;
        k.bbx(
            &blade,
            [sx * (w - 0.03), yn - 0.26, zb - bh0],
            [sx * (w + 0.02), yn + 0.02, zb + bh1],
            0.012,
        );
    }
    if mid {
        // 5-mph bumper: black rubber strip along the face and two upright guards.
        let rub = dkey("bumper_strip_front", fb, M::Rubber);
        k.front_box(
            &rub,
            -wn + 0.05,
            wn - 0.05,
            zb - 0.025,
            zb + 0.01,
            bproud + 0.008,
            0.004,
        );
        let guard = dkey("bumper_guards_front", fb, M::Chrome);
        for sx in SIDES {
            let x = sx * 0.34;
            k.front_box(
                &guard,
                x - 0.035,
                x + 0.035,
                zb - bh0 - 0.01,
                zb + bh1 + 0.06,
                bproud + 0.03,
                0.012,
            );
            let gr = dkey("bumper_guard_pads", fb, M::Rubber);
            k.front_box(
                &gr,
                x - 0.02,
                x + 0.02,
                zb - 0.04,
                zb + bh1 + 0.04,
                bproud + 0.038,
                0.005,
            );
        }
    }
    // Licence plate recess (black) in the bumper centre.
    let plate = dkey("plate_front", fb, M::Grille);
    k.front_box(
        &plate,
        -0.16,
        0.16,
        zb - bh0 + 0.008,
        zb + bh1 - 0.008,
        bproud + 0.004,
        0.0,
    );
    let val = dkey("front_valance", fb, M::Grille);
    k.front_box(&val, -0.62, 0.62, nz0 + 0.04, zb - bh0 - 0.02, 0.005, 0.0);

    // --- Grille and lamps.
    let gz0 = zb + bh1 + 0.012;
    let gz1 = nz1 - 0.02;
    if mid {
        // Upright waterfall grille in the centre, raised frame, hood ornament.
        let gh = 0.36;
        k.mesh_grille("grille", fb, gh, gz0, gz1 + 0.01, 2, 14, M::Chrome);
        let fr = dkey("grille_frame", fb, M::Chrome);
        let t = 0.025;
        k.front_box(
            &fr,
            -gh - t,
            gh + t,
            gz1 + 0.01,
            gz1 + 0.01 + t,
            0.03,
            0.006,
        );
        k.front_box(&fr, -gh - t, gh + t, gz0 - t, gz0, 0.03, 0.006);
        k.front_box(&fr, -gh - t, -gh, gz0, gz1 + 0.01, 0.03, 0.006);
        k.front_box(&fr, gh, gh + t, gz0, gz1 + 0.01, 0.03, 0.006);
        // Stacked square lamps either side of the grille, in a chrome surround, amber turn
        // lens wrapping the corner outboard of them.
        let (lx0, lx1) = (gh + 0.12, gh + 0.12 + 0.24);
        let lh = 0.5 * (gz1 - gz0) - 0.012;
        for sx in SIDES {
            let bez = sided(
                &dkey("headlamp_bezel_l", BodyRegion::FrontLeftFender, M::Chrome),
                sx,
            );
            k.front_box(
                &bez,
                sx * (lx0 - 0.02),
                sx * (lx1 + 0.02),
                gz0 - 0.01,
                gz1 + 0.01,
                0.02,
                0.006,
            );
            let hk = headlight_key(sx);
            for z0 in [gz0, gz0 + lh + 0.024] {
                k.front_box(
                    &hk,
                    sx * (lx0 + 0.008),
                    sx * (lx1 - 0.008),
                    z0 + 0.004,
                    z0 + lh,
                    0.03,
                    0.008,
                );
            }
            let ka = indicator_front_key(sx);
            let xo = (wn - 0.015).max(lx1 + 0.10);
            k.front_box(
                &ka,
                sx * (lx1 + 0.03),
                sx * xo,
                gz0 + 0.01,
                gz1 - 0.01,
                0.022,
                0.006,
            );
        }
        // Hood ornament: a small chrome stand and blade at the hood's leading edge.
        let yo = yn - 0.10;
        let zo = k.top_z(yo, 0.0);
        let orn = dkey("hood_ornament", BodyRegion::Hood, M::Chrome);
        k.b.cylinder(
            &orn,
            [0.0, yo, zo - 0.005],
            [0.0, yo, zo + 0.025],
            0.025,
            0.018,
            10,
            true,
        );
        k.prism_x(
            &orn,
            &[
                [yo - 0.03, zo + 0.02],
                [yo + 0.03, zo + 0.02],
                [yo + 0.01, zo + 0.08],
                [yo - 0.02, zo + 0.075],
            ],
            -0.004,
            0.004,
        );
    } else {
        // Full-width horizontal egg-crate grille with quad round lamps at its ends, all in one
        // chrome surround; a centre badge.
        let half = wn - 0.05;
        k.mesh_grille("grille", fb, half, gz0, gz1, 3, 16, M::Chrome);
        let fr = dkey("grille_frame", fb, M::Chrome);
        let t = 0.02;
        k.front_box(&fr, -half - t, half + t, gz1, gz1 + t, 0.022, 0.005);
        k.front_box(&fr, -half - t, half + t, gz0 - t, gz0, 0.022, 0.005);
        k.front_box(&fr, -half - t, -half, gz0, gz1, 0.022, 0.005);
        k.front_box(&fr, half, half + t, gz0, gz1, 0.022, 0.005);
        face_badge(
            &mut k,
            "grille_badge",
            fb,
            true,
            0.0,
            0.5 * (gz0 + gz1),
            0.035,
        );
        let zl = 0.5 * (gz0 + gz1);
        let r = (0.5 * (gz1 - gz0) - 0.008).min(0.08);
        for sx in SIDES {
            let bez = sided(
                &dkey("headlamp_bezel_l", BodyRegion::FrontLeftFender, M::Chrome),
                sx,
            );
            for x in [wn - 0.16, wn - 0.16 - 2.3 * r] {
                k.round_lamp(&headlight_key(sx), true, sx * x, zl, r, 0.032, Some(&bez));
            }
            // Amber parking / turn lens wrapping the corner outboard of the outer lamp.
            let ka = indicator_front_key(sx);
            let x_out = wn - 0.16 + r + 0.016 + 0.012;
            k.front_box(
                &ka,
                sx * x_out,
                sx * (wn - 0.008),
                zl - 0.8 * r,
                zl + 0.8 * r,
                0.026,
                0.004,
            );
        }
        // Chrome hood-centre spear.
        let span = yn - p.y_fw;
        k.top_stripe(
            "hood_spear",
            BodyRegion::Hood,
            yn - 0.75 * span,
            yn - 0.05,
            -0.012,
            0.012,
            M::Chrome,
        );
    }

    // --- Rear bumper, lamps, plate.
    let zbr = tz0 + if mid { 0.30 } else { 0.34 } * (tz1 - tz0);
    let rblade = dkey("bumper_blade_rear", rb, M::Chrome);
    k.rear_box(
        &rblade,
        -wt - 0.02,
        wt + 0.02,
        zbr - bh0,
        zbr + bh1,
        bproud - 0.005,
        0.02,
    );
    for sx in SIDES {
        let w = p.half_width(yt + 0.18) + 0.01;
        k.bbx(
            &rblade,
            [sx * (w - 0.03), yt + 0.24, zbr - bh0],
            [sx * (w + 0.02), yt - 0.02, zbr + bh1],
            0.012,
        );
    }
    if mid {
        let rub = dkey("bumper_strip_rear", rb, M::Rubber);
        k.rear_box(
            &rub,
            -wt + 0.05,
            wt - 0.05,
            zbr - 0.025,
            zbr + 0.01,
            bproud + 0.003,
            0.004,
        );
        let guard = dkey("bumper_guards_rear", rb, M::Chrome);
        for sx in SIDES {
            let x = sx * 0.40;
            k.rear_box(
                &guard,
                x - 0.035,
                x + 0.035,
                zbr - bh0 - 0.01,
                zbr + bh1 + 0.04,
                bproud + 0.025,
                0.012,
            );
        }
    }
    let pz0 = zbr + bh1 + 0.02;
    let pz1 = tz1 - 0.03;
    if mid {
        // Tall lamps at the corners (wrapping), red outer strip = turn signal; a ribbed
        // reflector panel between them; plate on the panel.
        k.rear_panel(
            "tail_panel",
            BodyRegion::Trunk,
            wt - 0.25,
            pz0 + 0.02,
            pz1 - 0.03,
            M::Grille,
        );
        let rib = dkey("tail_panel_ribs", BodyRegion::Trunk, M::Chrome);
        for i in 0..4 {
            let z = pz0 + 0.05 + (pz1 - pz0 - 0.10) * i as f32 / 3.0;
            k.rear_box(
                &rib,
                -wt + 0.27,
                wt - 0.27,
                z - 0.005,
                z + 0.005,
                0.014,
                0.0,
            );
        }
        for sx in SIDES {
            // Tall projected lens at each corner, swung round onto the quarter; the bottom
            // strip is the red turn signal.
            let (xa, xb) = (wt - 0.21, wt - 0.02);
            let outline = [
                [xa, pz0 + 0.01],
                [xb, pz0 + 0.01],
                [xb, pz1 - 0.01],
                [xa, pz1 - 0.01],
            ];
            k.proj_cluster(
                sx,
                false,
                LampView::Rear,
                &outline,
                Some(Seg::Lower(0.06)),
                M::Taillight,
                0.02,
                35.0,
            );
        }
        let kr = dkey("reverse_lamps", BodyRegion::Trunk, M::Headlight);
        for sx in SIDES {
            let x = sx * (wt - 0.32);
            k.rear_box(&kr, x - 0.05, x + 0.05, pz0 + 0.03, pz0 + 0.07, 0.02, 0.004);
        }
        let plate = dkey("plate_rear", BodyRegion::Trunk, M::Grille);
        k.rear_box(
            &plate,
            -0.16,
            0.16,
            pz0 + 0.03,
            pz0 + 0.03 + 0.15,
            0.02,
            0.0,
        );
    } else {
        // Full-width lamps: two rows a side split by a chrome bar, the outer third of each
        // side the red turn signal, reverse lamps by the plate.
        k.rear_panel(
            "tail_panel",
            BodyRegion::Trunk,
            wt - 0.03,
            pz0,
            pz1,
            M::Grille,
        );
        for sx in SIDES {
            let (xa, xb) = (0.20, wt - 0.06);
            let kb = sided(
                &dkey("taillight_bezel_l", BodyRegion::RearLeftQuarter, M::Chrome),
                sx,
            );
            k.rear_box(&kb, sx * xa, sx * xb, pz0 + 0.01, pz1 - 0.01, 0.018, 0.006);
            k.rear_cluster(
                sx,
                xa + 0.012,
                xb - 0.012,
                pz0 + 0.022,
                pz1 - 0.022,
                0.026,
                0.004,
                Seg::Outboard(0.22),
                M::Taillight,
            );
            let bar = sided(&dkey("taillight_bar_l", BodyRegion::Trunk, M::Chrome), sx);
            let zm = 0.5 * (pz0 + pz1);
            k.rear_box(&bar, sx * xa, sx * xb, zm - 0.007, zm + 0.007, 0.032, 0.0);
        }
        let kr = dkey("reverse_lamps", BodyRegion::Trunk, M::Headlight);
        for sx in SIDES {
            let x = sx * 0.13;
            k.rear_box(&kr, x - 0.04, x + 0.04, pz0 + 0.03, pz1 - 0.03, 0.02, 0.004);
        }
        let plate = dkey("plate_rear", rb, M::Grille);
        k.rear_box(
            &plate,
            -0.16,
            0.16,
            zbr - bh0 + 0.008,
            zbr + bh1 - 0.008,
            bproud + 0.0,
            0.0,
        );
    }

    // --- Sides: bright belt moulding, rocker trim, (mid) rubber body-side strip, side
    // markers, chrome bullet mirrors and handles.
    let (yf, _, rf) = k.arch(true);
    let (yr, _, rr) = k.arch(false);
    let zr = p.z_door_bottom + 0.03;
    let kc = dkey("rocker_trim_l", BodyRegion::FrameLeft, M::Chrome);
    for sx in SIDES {
        k.side_patch(
            &kc,
            sx,
            yr + rr + 0.04,
            yf - rf - 0.04,
            zr,
            zr + 0.035,
            0.002,
            0.008,
            (8, 1),
        );
    }
    // Doors and quarters only: ahead of the windshield base the fender top falls to the nose.
    let (ym0, ym1) = (yt + 0.25, p.y_fw - 0.05);
    let zbm = (0..=8)
        .map(|i| p.belt(ym0 + (ym1 - ym0) * i as f32 / 8.0))
        .fold(f32::MAX, f32::min)
        - 0.045;
    side_moulding(
        &mut k,
        "belt_moulding_l",
        BodyRegion::LeftDoorF,
        M::Chrome,
        ym0,
        ym1,
        zbm,
        0.012,
        0.002,
    );
    if mid {
        let zs = 0.50 * p.roof.2;
        side_moulding(
            &mut k,
            "side_strip_l",
            BodyRegion::LeftDoorF,
            M::Rubber,
            yr + rr + 0.06,
            yf - rf - 0.06,
            zs,
            0.03,
            0.004,
        );
    }
    side_markers(&mut k, 0.08, 0.12, Some(M::Chrome));

    // --- Roof: full vinyl (early) or landau half vinyl with a chrome band (mid).
    let (r0, r1, _, _) = p.roof;
    let hw = k.roof_edge(0.5 * (r0 + r1)) - 0.01;
    if mid {
        let ym = r0 + 0.55 * (r1 - r0);
        k.top_stripe(
            "vinyl_roof",
            BodyRegion::Roof,
            r0 + 0.02,
            ym,
            -hw,
            hw,
            M::Trim,
        );
        let kb = dkey("landau_band", BodyRegion::Roof, M::Chrome);
        k.patch(
            &kb,
            ym,
            ym + 0.025,
            PatchBand::Top { x0: -hw, x1: hw },
            0.004,
            0.006,
            (1, 4),
        );
    } else {
        k.top_stripe(
            "vinyl_roof",
            BodyRegion::Roof,
            r0 + 0.02,
            r1 - 0.02,
            -hw,
            hw,
            M::Trim,
        );
    }
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
    k.mirrors(0.12, 0.12, Some(M::Chrome));
    k.door_handles(M::Chrome);
}
