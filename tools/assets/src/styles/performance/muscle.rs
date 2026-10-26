//! Muscle: `ironside_440` (early-70s big-block coupe), `rampart_50` (2010s muscle coupe),
//! `bulwark_400` (early-70s formal four-door, stacked lamps) and `brigand_400` (mid-70s
//! rounded "fuselage" four-door). Each has its own body and kit.
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

use super::kit::{
    dkey, headlight_key, indicator_front_key, indicator_rear_key, lkey, sided, taillight_key, Kit,
    Seg, SIDES,
};
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
        windshield_rake_deg: Some(59.0),
        a_pillar_rake_deg: 6.0,
        rear_glass_rake_deg: Some(63.0),
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

/// Early-70s American full-size four-door muscle sedan (`bulwark_400`): the [`ironside`]
/// family (long flat slab, cowl scoop, chrome blades, quad round lamps) with the cabin moved
/// forward and a longer roof for the rear doors, dark blue paint. Shares the ironside body kit.
pub fn bulwark(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: 0.0,
        nose_h: Some(0.73),
        nose_rake_deg: 2.0,
        nose_plan_round: 0.08,
        tail_plan_round: 0.08,
        hood_crown: 0.006,
        power_bulge_h: 0.0,
        hood_scoop: HoodScoop::None,
        windshield_rake_deg: Some(55.0),
        a_pillar_rake_deg: 3.0,
        rear_glass_rake_deg: Some(44.0),
        roof_len_trim: 0.20,
        roof_dz: 0.03,
        roof_crown: 0.010,
        tumblehome: Some(0.14),
        glass_inset: 0.08,
        crease_z: 0.54,
        crease_depth: 0.008,
        shoulder_round: 0.045,
        sill_tuck: 0.05,
        haunch: 0.015,
        flare: 0.0,
        flare_shape: ArchShape::Box,
        ride_dz: 0.01,
        stance_pitch_deg: 0.25,
        deck_drop: -0.01,
        ducktail_h: 0.0,
        tail_round: 0.09,
        tail_drop: 0.02,
        belt_kick: 0.0,
        overhang_trim: (0.03, 0.04),
        ..BodyStyle::preset(Silhouette::Notchback)
    });
    s.details.push(kit_detail(bulwark_kit));
    wheels(
        &mut s,
        def,
        RimKind::SlotMag,
        5,
        0.55,
        M::Chrome,
        CapKind::Centre,
        (0.025, 0.015),
        (0.0, 0.0),
    );
    let m = &mut s.mechanical;
    m.engine = EngineKind::V8;
    m.induction = Induction::Natural;
    m.front_suspension = SuspensionKind::DoubleWishbone;
    m.rear_suspension = SuspensionKind::SolidAxleLeaf;
    m.front_brakes.caliper = CaliperKind::Floating;
    m.rear_brakes.caliper = CaliperKind::Drum;
    m.exhaust = ExhaustKind::Dual;
    m.exhaust_tip_r_m = 0.038;
    s.tints.push((M::Paint, rgb(0x1E3A5F)));
    s
}

/// `bulwark_400`: stacked twin round lamps in chrome bezels either side of a wide eggcrate
/// grille, a flat chrome blade bumper, vertical corner tail lamps on a chrome tail panel,
/// chrome window and rocker trim, a vinyl roof over the formal C-pillar.
fn bulwark_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let (yn, yt) = (k.yn, k.yt);
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;
    let rb = BodyRegion::RearBumper;

    // --- Front: flat chrome blade low on the nose, eggcrate grille above it, stacked round
    //     lamps in chrome bezels at both ends.
    let zb = nz0 + 0.20;
    let blade = dkey("bumper_blade_front", fb, M::Chrome);
    k.front_box(
        &blade,
        -wn - 0.02,
        wn + 0.02,
        zb - 0.07,
        zb + 0.05,
        0.075,
        0.016,
    );
    let (gz0, gz1) = (zb + 0.055, nz1 - 0.03);
    k.mesh_grille("grille", fb, wn - 0.30, gz0, gz1, 2, 6, M::Chrome);
    let r = ((gz1 - gz0) * 0.25 - 0.010).min(0.055);
    for sx in SIDES {
        let bez = sided(
            &dkey("headlamp_bezel_l", BodyRegion::FrontLeftFender, M::Chrome),
            sx,
        );
        let x = wn - 0.09 - r;
        for z in [gz0 + 0.3 * (gz1 - gz0), gz0 + 0.74 * (gz1 - gz0)] {
            k.round_lamp(&headlight_key(sx), true, sx * x, z, r, 0.026, Some(&bez));
        }
        // Amber park lens under the lamp stack, in the blade's top face line.
        let ka = indicator_front_key(sx);
        k.front_box(
            &ka,
            sx * (x - r - 0.01),
            sx * (x + r + 0.01),
            gz0 - 0.005,
            gz0 + 0.045,
            0.022,
            0.006,
        );
    }
    k.hood_pins(yn - 0.20, 0.50);

    // --- Rear: chrome blade, chrome tail panel with vertical lamps at the corners.
    let zbr = tz0 + 0.18;
    let rblade = dkey("bumper_blade_rear", rb, M::Chrome);
    k.rear_box(
        &rblade,
        -wt - 0.02,
        wt + 0.02,
        zbr - 0.07,
        zbr + 0.05,
        0.07,
        0.016,
    );
    let (pz0, pz1) = (zbr + 0.065, tz1 - 0.03);
    k.rear_panel(
        "tail_panel",
        BodyRegion::Trunk,
        wt - 0.03,
        pz0,
        pz1,
        M::Chrome,
    );
    for sx in SIDES {
        k.rear_cluster(
            sx,
            wt - 0.26,
            wt - 0.06,
            pz0 + 0.02,
            pz1 - 0.02,
            0.022,
            0.006,
            Seg::Lower(0.045),
            M::Taillight,
        );
    }
    let kr = dkey("reverse_lamps", BodyRegion::Trunk, M::Headlight);
    for x in [-0.30, 0.22] {
        k.rear_box(&kr, x, x + 0.08, pz0 + 0.03, pz0 + 0.075, 0.018, 0.004);
    }
    let kp = dkey("plate_recess", BodyRegion::Trunk, M::Trim);
    k.rear_box(&kp, -0.22, 0.22, pz0 + 0.025, pz1 - 0.025, 0.006, 0.0);

    // --- Sides: chrome rocker strip and belt trim, vinyl roof over the formal C-pillar.
    let (yf, _, rf) = k.arch(true);
    let (yr, _, rr) = k.arch(false);
    let zr = p.z_door_bottom + 0.025;
    for sx in SIDES {
        let kc = dkey("rocker_trim_l", BodyRegion::FrameLeft, M::Chrome);
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
    let (r0, r1, _, _) = p.roof;
    let hw = k.roof_edge(0.5 * (r0 + r1)) - 0.01;
    k.top_stripe(
        "vinyl_roof",
        BodyRegion::Roof,
        r0 - 0.10,
        r1 - 0.02,
        -hw,
        hw,
        M::Trim,
    );
    let _ = yt;

    k.mirrors(0.12, 0.12, None);
    k.door_handles(M::Chrome);
    k.fender_repeaters();
    k.fuel_filler(1.0, 0.12, M::Chrome);
    k.wipers();
}

/// Mid-70s American full-size four-door sedan (`brigand_400`): the [`ironside`] family as a
/// plainer sedan (no cowl scoop, long roof, level stance), brown paint and dog-dish steel
/// wheels. Shares the ironside body kit.
pub fn brigand(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: -0.05,
        nose_h: Some(0.74),
        nose_rake_deg: 10.0,
        nose_plan_round: 0.34,
        tail_plan_round: 0.30,
        hood_crown: 0.018,
        hood_scoop: HoodScoop::None,
        power_bulge_h: 0.0,
        windshield_rake_deg: Some(58.0),
        a_pillar_rake_deg: 7.0,
        rear_glass_rake_deg: Some(56.0),
        roof_len_trim: 0.22,
        roof_dz: 0.02,
        roof_arc: 0.35,
        tumblehome: Some(0.18),
        crease_z: 0.50,
        crease_depth: 0.0,
        shoulder_round: 0.09,
        sill_tuck: 0.07,
        haunch: 0.030,
        front_haunch: 0.025,
        flare: 0.0,
        flare_shape: ArchShape::Round,
        ride_dz: 0.025,
        stance_pitch_deg: 0.0,
        ducktail_h: 0.0,
        tail_round: 0.22,
        tail_drop: 0.07,
        belt_kick: 0.0,
        overhang_trim: (0.06, 0.07),
        ..BodyStyle::preset(Silhouette::Notchback)
    });
    s.details.push(kit_detail(brigand_kit));
    let m = &mut s.mechanical;
    m.engine = EngineKind::V8;
    m.induction = Induction::Natural;
    m.front_suspension = SuspensionKind::DoubleWishbone;
    m.rear_suspension = SuspensionKind::SolidAxleLeaf;
    m.front_brakes.caliper = CaliperKind::Floating;
    m.rear_brakes.caliper = CaliperKind::Drum;
    m.exhaust = ExhaustKind::Single;
    m.exhaust_tip_r_m = 0.038;
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
    // Dog-dish wheels + the long roof push this long body past the hero budget with the hero
    // tier's 24 wheel segments (20 is `WheelStyle`'s "tier default" marker, so 22).
    s.wheel.segments = 22;
    s.tints.push((M::Paint, rgb(0x6B4A2B)));
    s
}

/// `brigand_400`: a plain mid-70s full-size sedan — one wide chrome-framed grille with the
/// head lamps sunk behind its outer ends, chrome blade bumpers front and rear, a full-width
/// slim tail lamp band, an opera window in the C-pillar and a landau vinyl roof.
fn brigand_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;
    let rb = BodyRegion::RearBumper;

    // --- Front: chrome blade, full-width grille with the lamps sunk in its outer ends.
    let zb = nz0 + 0.16;
    let blade = dkey("bumper_blade_front", fb, M::Chrome);
    k.front_box(
        &blade,
        -wn - 0.02,
        wn + 0.02,
        zb - 0.05,
        zb + 0.04,
        0.065,
        0.016,
    );
    let (gz0, gz1) = (zb + 0.06, nz1 - 0.035);
    k.grille(
        "grille",
        fb,
        wn - 0.05,
        gz0,
        gz1,
        3,
        M::Chrome,
        Some(M::Chrome),
    );
    for sx in SIDES {
        k.front_cluster(
            sx,
            wn - 0.28,
            wn - 0.07,
            gz0 + 0.015,
            gz1 - 0.015,
            0.010,
            0.004,
            Seg::Outboard(0.07),
        );
    }

    // --- Rear: chrome blade and a full-width slim lamp band across the tail.
    let zbr = tz0 + 0.15;
    let rblade = dkey("bumper_blade_rear", rb, M::Chrome);
    k.rear_box(
        &rblade,
        -wt - 0.02,
        wt + 0.02,
        zbr - 0.05,
        zbr + 0.04,
        0.06,
        0.016,
    );
    let (pz0, pz1) = (tz1 - 0.14, tz1 - 0.035);
    k.rear_panel(
        "tail_panel",
        BodyRegion::Trunk,
        wt - 0.03,
        pz0,
        pz1,
        M::Chrome,
    );
    for sx in SIDES {
        k.rear_cluster(
            sx,
            0.16,
            wt - 0.06,
            pz0 + 0.018,
            pz1 - 0.018,
            0.016,
            0.005,
            Seg::Outboard(0.10),
            M::Taillight,
        );
    }
    let kp = dkey("plate_recess", BodyRegion::RearBumper, M::Trim);
    k.rear_box(&kp, -0.22, 0.22, zbr - 0.03, zbr + 0.03, 0.075, 0.0);

    // --- Sides: chrome rocker strip, opera window in the C-pillar, landau vinyl roof.
    let (yf, _, rf) = k.arch(true);
    let (yr, _, rr) = k.arch(false);
    let zr = p.z_door_bottom + 0.03;
    for sx in SIDES {
        let kc = dkey("rocker_trim_l", BodyRegion::FrameLeft, M::Chrome);
        k.side_patch(
            &kc,
            sx,
            yr + rr + 0.05,
            yf - rf - 0.05,
            zr,
            zr + 0.03,
            0.002,
            0.008,
            (8, 1),
        );
        let ko = sided(
            &dkey("opera_window_l", BodyRegion::RearLeftQuarter, M::Glass),
            sx,
        );
        let yq = p.roof.0 - 0.04;
        let zq = p.belt(yq) + 0.06;
        k.side_box(&ko, sx, yq, zq, 0.14, 0.12, 0.012, 0.0);
    }
    let (r0, r1, _, _) = p.roof;
    let hw = k.roof_edge(0.5 * (r0 + r1)) - 0.01;
    k.top_stripe(
        "vinyl_roof",
        BodyRegion::Roof,
        r0 - 0.06,
        r1 - 0.02,
        -hw,
        hw,
        M::Trim,
    );

    k.mirrors(0.12, 0.11, None);
    k.door_handles(M::Chrome);
    k.fender_repeaters();
    k.fuel_filler(1.0, 0.12, M::Chrome);
    k.wipers();
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
        // Rectangular amber parking / turn lens set in the grille inboard of the lamp pair,
        // flush with the bezel faces.
        let x_in = wn - 0.18 - 2.3 * r - r - 0.016 - 0.014;
        let ka = indicator_front_key(sx);
        k.front_box(
            &ka,
            sx * (x_in - 0.11),
            sx * x_in,
            zl - 0.55 * r,
            zl + 0.55 * r,
            0.024,
            0.006,
        );
    }
    let val = dkey("front_valance", fb, M::Grille);
    k.front_box(&val, -0.55, 0.55, nz0 + 0.05, zb - 0.08, 0.005, 0.0);
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
        // Three segments per side (reads as one wide lamp with chrome dividers); the outer
        // one is the red-on-red turn signal.
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
    let kr = dkey("reverse_lamps", BodyRegion::Trunk, M::Headlight);
    for x in [-0.09, 0.05] {
        k.rear_box(&kr, x, x + 0.04, pz0 + 0.03, pz1 - 0.03, 0.02, 0.004);
    }

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
        // Lens across the nose top edge with an amber outboard segment, plus the DRL blade on
        // the face.
        k.top_lamp_split(
            sx,
            yn - 0.20,
            yn - 0.03,
            0.58,
            wn - 0.07,
            0.025,
            Seg::Outboard(0.08),
        );
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
        // Three vertical bars; the outer one is the red-on-red turn signal.
        let ka = indicator_rear_key(sx, M::Taillight);
        for i in 0..3 {
            let xc = 0.40 + 0.13 * i as f32;
            k.rear_box(
                if i == 2 { &ka } else { &tk },
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
