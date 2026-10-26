//! Civilian traffic styles (owner: wave-2 `style-traffic`). Spec:
//! `docs/design/vehicles/research_traffic.md` §3 (body knobs + detail lists per car).
//!
//! Every car is a [`BodyStyle`] built from a silhouette preset plus one custom detail function
//! that emits that car's trim kit through the small helpers below: grilles, bumpers with rub
//! strips, door handles, mirrors, wipers, plates, side markers, era-appropriate lamps (the
//! generic head / tail light boxes are reshaped in place), bed rails, step bumpers, mud flaps,
//! van door tracks and hinges.
//!
//! Helpers work on the LEFT side (x < 0) and mirror to the right. They read the styled
//! [`BodyProfile`] so details sit on the real surface:
//! - [`nose_y`] / [`tail_y`]: the front / rear face y at a lateral offset (follows the
//!   plan-view corner rounding);
//! - [`front_box`] / [`rear_box`] / [`side_box`]: boxes whose inner face follows that surface;
//! - [`side_strip`]: a thin shell along the side, split at panel seams and skipping wheel
//!   arches, each piece skinned to the panel it sits on.
//!
//! No two cars share a body or a kit (owner 2026-10-26, "a lot of repeat game models"): the
//! two European panel vans are a short-bonnet van (`satchel_20`) and a cab-over
//! (`tumbril_20`), each with its own face.
//!
//! Visual-only variants (taxi sign, ladder rack, roof box, liveries, paint) are the browser's
//! job (`apps/testbed-web/src/render/traffic.ts`); the meshes here are the base bodies.

use wp_contracts::{BodyRegion, VehicleDef};

use crate::builder::{Builder, PartKey, Patch, PatchBand};
use crate::mesh::{indicator_front_key, indicator_rear_key, MaterialSlot, PartKind};
use crate::profile::BodyProfile;
use crate::style::{
    ArchShape, BodyStyle, CaliperKind, CapKind, CarStyle, Detail, DetailCtx, EngineKind,
    ExhaustKind, RimKind, Shape, Silhouette, SuspensionKind,
};

use BodyRegion as R;
use MaterialSlot as M;

/// Style for a traffic car id, or `None` if this module does not own it.
pub fn style(def: &VehicleDef) -> Option<CarStyle> {
    match def.id.as_str() {
        "debug_sedan" => Some(debug_sedan()),
        "junkyard_beater" => Some(junkyard_beater()),
        "tansy_13" => Some(tansy_13()),
        "furrow_24" => Some(furrow_24()),
        "hauler_1500" => Some(hauler_1500()),
        "drayman_cargo" => Some(drayman_cargo()),
        "satchel_20" => Some(satchel_20()),
        "tumbril_20" => Some(tumbril_20()),
        _ => None,
    }
}

/// A style whose details are one custom trim-kit function.
fn with_kit(body: BodyStyle, kit: fn(&DetailCtx, &PartKey, &mut Builder)) -> CarStyle {
    let mut s = CarStyle::with_body(body);
    s.details.push(Detail::new(
        "trim_kit",
        R::Cabin,
        M::Trim,
        Shape::Custom(kit),
    ));
    s.tints.push((M::Grille, [14, 15, 17]));
    s
}

// =======================================================================================
// Cars
// =======================================================================================

/// 2010s mid-size FF family sedan (research §3.1).
fn debug_sedan() -> CarStyle {
    let body = BodyStyle {
        nose_h: Some(0.68),
        nose_rake_deg: 28.0,
        nose_plan_round: 0.45,
        tail_plan_round: 0.32,
        hood_crown: 0.03,
        windshield_rake_deg: Some(62.0),
        a_pillar_rake_deg: 6.0,
        rear_glass_rake_deg: Some(60.0),
        tumblehome: Some(0.15),
        crease_z: 0.60,
        crease_depth: 0.012,
        haunch: 0.015,
        flare: 0.01,
        ducktail_h: 0.02,
        belt_kick: 0.04,
        shoulder_round: 0.05,
        ..BodyStyle::preset(Silhouette::Notchback)
    };
    let mut s = with_kit(body, sedan_kit);
    s.wheel.rim = RimKind::OemAlloy;
    s.wheel.spokes = 5;
    s.wheel.cap = CapKind::Centre;
    s.mechanical.engine = EngineKind::Inline4;
    s.mechanical.front_suspension = SuspensionKind::Strut;
    s.mechanical.rear_suspension = SuspensionKind::Multilink;
    s.mechanical.exhaust = ExhaustKind::Single;
    s
}

/// Late-80s compact FF sedan, high mileage (research §3.2).
fn junkyard_beater() -> CarStyle {
    let body = BodyStyle {
        nose_h: Some(0.72),
        nose_rake_deg: 10.0,
        nose_plan_round: 0.10,
        tail_plan_round: 0.12,
        hood_crown: 0.01,
        windshield_rake_deg: Some(54.0),
        rear_glass_rake_deg: Some(42.0),
        roof_dz: 0.02,
        roof_crown: 0.01,
        tumblehome: Some(0.10),
        crease_z: 0.58,
        crease_depth: 0.006,
        shoulder_round: 0.025,
        sill_tuck: 0.04,
        flare_shape: ArchShape::Box,
        ride_dz: -0.015,
        stance_pitch_deg: -0.25,
        overhang_trim: (0.03, 0.05),
        tail_round: 0.12,
        tail_drop: 0.03,
        ..BodyStyle::preset(Silhouette::Notchback)
    };
    let mut s = with_kit(body, beater_kit);
    s.wheel.rim = RimKind::SteelHubcap;
    s.wheel.spokes = 0;
    s.wheel.cap = CapKind::Hubcap;
    s.wheel.lugs = 4;
    s.wheel.pcd_r_m = 0.05;
    s.mechanical.engine = EngineKind::Inline4;
    s.mechanical.front_suspension = SuspensionKind::Strut;
    s.mechanical.rear_suspension = SuspensionKind::TorsionBeam;
    s.mechanical.rear_brakes.caliper = CaliperKind::Drum;
    s.mechanical.front_brakes.vented = false;
    s.mechanical.exhaust = ExhaustKind::Single;
    s
}

/// 2000s FF supermini 5-door (research §3.3).
fn tansy_13() -> CarStyle {
    let body = BodyStyle {
        nose_rake_deg: 30.0,
        nose_plan_round: 0.40,
        tail_plan_round: 0.25,
        hood_crown: 0.02,
        windshield_rake_deg: Some(60.0),
        cowl_h: 0.03,
        tumblehome: Some(0.13),
        crease_z: 0.57,
        crease_depth: 0.008,
        haunch: 0.01,
        flare: 0.005,
        belt_kick: 0.05,
        belt_rake: 0.03,
        overhang_trim: (0.0, -0.02),
        ..BodyStyle::preset(Silhouette::Hatch)
    };
    let mut s = with_kit(body, supermini_kit);
    s.wheel.rim = RimKind::SteelHubcap;
    s.wheel.spokes = 0;
    s.wheel.cap = CapKind::Hubcap;
    s.wheel.lugs = 4;
    s.wheel.pcd_r_m = 0.05;
    s.mechanical.engine = EngineKind::Inline4;
    s.mechanical.front_suspension = SuspensionKind::Strut;
    s.mechanical.rear_suspension = SuspensionKind::TorsionBeam;
    s.mechanical.rear_brakes.caliper = CaliperKind::Drum;
    s.mechanical.exhaust = ExhaustKind::Single;
    s.mechanical.exhaust_tip_r_m = 0.028;
    s
}

/// 90s compact RWD pickup, regular cab (research §3.4).
fn furrow_24() -> CarStyle {
    let body = BodyStyle {
        nose_h: Some(0.88),
        nose_rake_deg: 8.0,
        nose_plan_round: 0.12,
        tail_plan_round: 0.08,
        hood_crown: 0.015,
        windshield_rake_deg: Some(55.0),
        tumblehome: Some(0.11),
        crease_z: 0.59,
        crease_depth: 0.010,
        flare: 0.015,
        shoulder_round: 0.03,
        sill_tuck: 0.04,
        stance_pitch_deg: 0.25,
        ..BodyStyle::preset(Silhouette::Pickup)
    };
    let mut s = with_kit(body, compact_pickup_kit);
    s.wheel.rim = RimKind::Steel;
    s.wheel.spokes = 0;
    s.wheel.cap = CapKind::Centre;
    s.wheel.lugs = 6;
    s.wheel.pcd_r_m = 0.07;
    s.mechanical.engine = EngineKind::Inline4;
    s.mechanical.front_suspension = SuspensionKind::DoubleWishbone;
    s.mechanical.rear_suspension = SuspensionKind::SolidAxleLeaf;
    s.mechanical.rear_brakes.caliper = CaliperKind::Drum;
    s.mechanical.exhaust = ExhaustKind::Single;
    s
}

/// 2010s full-size half-ton crew-cab pickup (research §3.5).
fn hauler_1500() -> CarStyle {
    let body = BodyStyle {
        nose_h: Some(1.05),
        nose_rake_deg: 5.0,
        nose_plan_round: 0.15,
        tail_plan_round: 0.08,
        hood_crown: 0.03,
        hood_bulge_h: 0.03,
        hood_bulge_w: 0.55,
        windshield_rake_deg: Some(58.0),
        tumblehome: Some(0.12),
        roof_crown: 0.015,
        crease_z: 0.56,
        crease_depth: 0.015,
        haunch: 0.02,
        front_haunch: 0.015,
        flare: 0.03,
        shoulder_round: 0.03,
        sill_tuck: 0.04,
        stance_pitch_deg: 0.25,
        ..BodyStyle::preset(Silhouette::Pickup)
    };
    let mut s = with_kit(body, fullsize_pickup_kit);
    s.wheel.rim = RimKind::Truck;
    s.wheel.spokes = 6;
    s.wheel.cap = CapKind::Centre;
    s.wheel.lugs = 6;
    s.wheel.pcd_r_m = 0.07;
    s.wheel.rim_material = M::Chrome;
    // Keeps the crew cab (big loft + real wheels) under the 10k traffic budget.
    s.wheel.segments = 14;
    s.mechanical.engine = EngineKind::V8;
    s.mechanical.front_suspension = SuspensionKind::DoubleWishbone;
    s.mechanical.rear_suspension = SuspensionKind::SolidAxleLeaf;
    s.mechanical.exhaust = ExhaustKind::Single;
    s.mechanical.exhaust_tip_r_m = 0.045;
    // Red tow hooks use the caliper slot.
    s.mechanical.caliper_rgb = Some([170, 24, 24]);
    s
}

/// 2000s full-size RWD cargo van (research §3.6).
fn drayman_cargo() -> CarStyle {
    let body = BodyStyle {
        nose_h: Some(1.10),
        nose_rake_deg: 15.0,
        nose_plan_round: 0.20,
        tail_plan_round: 0.06,
        hood_crown: 0.03,
        windshield_rake_deg: Some(48.0),
        tumblehome: Some(0.10),
        crease_z: 0.50,
        crease_depth: 0.010,
        flare: 0.01,
        stance_pitch_deg: 0.2,
        overhang_trim: (0.0, 0.03),
        ..BodyStyle::preset(Silhouette::Van)
    };
    let mut s = with_kit(body, van_kit);
    s.wheel.rim = RimKind::Steel;
    s.wheel.spokes = 0;
    s.wheel.cap = CapKind::Small;
    s.wheel.lugs = 8;
    s.wheel.pcd_r_m = 0.075;
    s.mechanical.engine = EngineKind::V8;
    s.mechanical.front_suspension = SuspensionKind::DoubleWishbone;
    s.mechanical.rear_suspension = SuspensionKind::SolidAxleLeaf;
    s.mechanical.exhaust = ExhaustKind::Single;
    s
}

/// 2000s European FWD short-bonnet panel van (`satchel_20`): a rounded, car-like nose with a
/// stubby bonnet under a steeply raked screen, soft shoulders and a medium roof. Own kit
/// (swept corner lamps, body-colour bumpers, black cladding, sliding door, barn doors).
fn satchel_20() -> CarStyle {
    let body = BodyStyle {
        nose_h: Some(0.92),
        nose_rake_deg: 34.0,
        nose_plan_round: 0.38,
        tail_plan_round: 0.10,
        hood_crown: 0.035,
        windshield_rake_deg: Some(46.0),
        a_pillar_rake_deg: 8.0,
        tumblehome: Some(0.12),
        roof_crown: 0.045,
        crease_z: 0.52,
        crease_depth: 0.0,
        shoulder_round: 0.10,
        sill_tuck: 0.08,
        front_haunch: 0.02,
        flare: 0.01,
        stance_pitch_deg: 0.1,
        tail_round: 0.10,
        tail_drop: 0.03,
        overhang_trim: (0.0, 0.02),
        ..BodyStyle::preset(Silhouette::Van)
    };
    let mut s = with_kit(body, short_nose_van_kit);
    s.wheel.rim = RimKind::Steel;
    s.wheel.spokes = 0;
    s.wheel.cap = CapKind::Small;
    s.wheel.lugs = 5;
    s.mechanical.engine = EngineKind::Inline4;
    s.mechanical.front_suspension = SuspensionKind::Strut;
    s.mechanical.rear_suspension = SuspensionKind::TorsionBeam;
    s.mechanical.exhaust = ExhaustKind::Single;
    s
}

/// 2000s European FWD cab-over panel van (`tumbril_20`): a flat, upright face with almost no
/// bonnet, a near-vertical screen, square corners, a tall flat roof and a long rear overhang.
/// Own kit (low square lamps either side of a full-width black face panel, deep black bumper
/// with a centre step, ribbed flanks).
fn tumbril_20() -> CarStyle {
    let body = BodyStyle {
        nose_h: Some(1.12),
        nose_rake_deg: 8.0,
        nose_plan_round: 0.10,
        tail_plan_round: 0.05,
        hood_crown: 0.008,
        windshield_rake_deg: Some(30.0),
        a_pillar_rake_deg: 2.0,
        tumblehome: Some(0.07),
        roof_crown: 0.02,
        crease_z: 0.60,
        crease_depth: 0.010,
        shoulder_round: 0.025,
        sill_tuck: 0.03,
        flare: 0.01,
        flare_shape: ArchShape::Box,
        stance_pitch_deg: 0.0,
        tail_round: 0.05,
        tail_drop: 0.015,
        overhang_trim: (-0.04, 0.08),
        ..BodyStyle::preset(Silhouette::Van)
    };
    let mut s = with_kit(body, cabover_van_kit);
    s.wheel.rim = RimKind::Steel;
    s.wheel.spokes = 0;
    s.wheel.cap = CapKind::Small;
    s.wheel.lugs = 5;
    s.mechanical.engine = EngineKind::Inline4;
    s.mechanical.front_suspension = SuspensionKind::Strut;
    s.mechanical.rear_suspension = SuspensionKind::TorsionBeam;
    s.mechanical.exhaust = ExhaustKind::Single;
    s
}

// =======================================================================================
// Trim kits (one per car)
// =======================================================================================

/// debug_sedan: slim swept lamps, slot grille + wide lower intake, fog lamps, chrome belt
/// trim, flush handles, third brake lamp, diffuser strip.
fn sedan_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let zn = p.belt(p.y_nose);
    let wf = p.half_width(p.y_nose);
    // Lamps: slim, wide, swept to the corner; the corner end of the lens is the amber turn
    // signal.
    reshape_pair(b, p, "headlight", |t, p, k| {
        lens_front(
            t,
            k,
            p,
            -(wf - 0.115),
            -0.42,
            zn - 0.10,
            zn - 0.025,
            0.012,
            0.03,
        );
    });
    reshape_indicators(b, p, true, M::EmissiveAmber, |t, p, k| {
        lens_front(
            t,
            k,
            p,
            -(wf - 0.01),
            -(wf - 0.11),
            zn - 0.10,
            zn - 0.025,
            0.012,
            0.03,
        );
    });
    // Tail lamps with an amber inboard section.
    let zt = p.belt(p.y_tail);
    let wt = p.half_width(p.y_tail);
    reshape_pair(b, p, "taillight", |t, p, k| {
        lens_rear(
            t,
            k,
            p,
            -(wt - 0.005),
            -0.495,
            zt - 0.14,
            zt - 0.03,
            0.012,
            0.03,
        );
    });
    reshape_indicators(b, p, false, M::EmissiveAmber, |t, p, k| {
        lens_rear(t, k, p, -0.49, -0.40, zt - 0.14, zt - 0.03, 0.012, 0.03);
    });
    // Grille: upper slot between the lamps, wide lower intake, fog lamps.
    let g = key("grille_upper", R::FrontBumper, M::Grille);
    front_box(b, &g, p, -0.40, 0.40, zn - 0.13, zn - 0.05, 0.006, 0.03);
    let c = key("grille_bar", R::FrontBumper, M::Chrome);
    front_box(b, &c, p, -0.40, 0.40, zn - 0.095, zn - 0.085, 0.012, 0.0);
    let lower = key("grille_lower", R::FrontBumper, M::Grille);
    front_box(b, &lower, p, -0.52, 0.52, 0.28, 0.42, 0.006, 0.03);
    let lip = key("splitter_lip", R::FrontBumper, M::Trim);
    front_box(b, &lip, p, -(wf - 0.08), wf - 0.08, 0.21, 0.25, 0.03, 0.02);
    let fog = light("foglamp_l", R::FrontBumper, M::Headlight);
    front_box(b, &fog, p, -0.70, -0.58, 0.30, 0.36, 0.01, 0.02);
    mirror(b, &fog);
    plate_front(b, p, 0.44);
    // Rear: plate, diffuser strip, third brake lamp.
    plate_rear(b, p, zt - 0.30);
    let dif = key("diffuser", R::RearBumper, M::Trim);
    rear_box(b, &dif, p, -(wt - 0.10), wt - 0.10, 0.20, 0.28, 0.02, 0.02);
    third_brake_lamp(b, p, 0.26);
    // Sides.
    belt_chrome(b, p);
    handles(b, p, M::Paint, false);
    side_markers(b, p, p.z_bumper_top.0 + 0.0);
    wipers(b, p, 0.25);
    shark_fin(b, p);
}

/// junkyard_beater: square sealed beams + amber corner lamps, chrome-framed slat grille,
/// black 5-mph bumpers, body rub strip, chrome drip rails, small black mirrors, full-width
/// tail lamp band, whip antenna.
fn beater_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let zn = p.belt(p.y_nose);
    let wf = p.half_width(p.y_nose);
    let (l0, l1) = (zn - 0.16, zn - 0.02);
    reshape_pair(b, p, "headlight", |t, p, k| {
        lens_front(t, k, p, -(wf - 0.12), -0.37, l0, l1, 0.008, 0.03);
    });
    // The amber corner lamp beside each sealed beam is its turn signal (same bezel).
    reshape_indicators(b, p, true, M::EmissiveAmber, |t, p, k| {
        lens_front(t, k, p, -(wf - 0.005), -(wf - 0.11), l0, l1, 0.008, 0.03);
    });
    // Lamp bezels (chrome) around head + corner lamps.
    let bez = key("lamp_bezel_l", R::FrontLeftFender, M::Chrome);
    front_box(
        b,
        &bez,
        p,
        -(wf - 0.002),
        -0.355,
        l0 - 0.012,
        l1 + 0.012,
        0.004,
        0.03,
    );
    mirror(b, &bez);
    grille(b, p, 0.34, l0, l1, 3, M::Chrome);
    // Black bumper bars with a chrome top strip, wrapping round the corners.
    bumper(b, p, true, 0.27, 0.43, 0.10, M::Trim, Some(M::Chrome));
    bumper(b, p, false, 0.29, 0.45, 0.11, M::Trim, Some(M::Chrome));
    plate_front(b, p, 0.30);
    let zt = p.belt(p.y_tail);
    let wt = p.half_width(p.y_tail);
    // Full-width tail band: lamps outboard (amber inboard section), dark garnish between.
    reshape_pair(b, p, "taillight", |t, p, k| {
        lens_rear(
            t,
            k,
            p,
            -(wt - 0.01),
            -0.325,
            zt - 0.17,
            zt - 0.04,
            0.01,
            0.03,
        );
    });
    reshape_indicators(b, p, false, M::EmissiveAmber, |t, p, k| {
        lens_rear(t, k, p, -0.32, -0.22, zt - 0.17, zt - 0.04, 0.01, 0.03);
    });
    let gar = key("tail_garnish", R::Trunk, M::Trim);
    rear_box(b, &gar, p, -0.22, 0.22, zt - 0.15, zt - 0.06, 0.008, 0.03);
    plate_rear(b, p, zt - 0.32);
    // Sides: rub strip, drip rails, chrome handles, small black mirrors, antenna.
    side_strip(
        b,
        p,
        "rub_strip",
        M::Trim,
        (p.y_tail + 0.35, p.y_nose - 0.3),
        (0.56, 0.61),
        0.014,
    );
    drip_rails(b, p);
    handles(b, p, M::Chrome, false);
    black_mirrors(b, p, (0.12, 0.07, 0.09), 0.05);
    wipers(b, p, 0.22);
    let ant = key("antenna", R::FrontRightFender, M::Chrome);
    let ya = p.y_fw + 0.25;
    let xa = p.half_width(ya) - 0.10;
    let za = p.belt(ya);
    b.cylinder(
        &ant,
        [xa, ya, za],
        [xa, ya - 0.06, za + 0.62],
        0.005,
        0.003,
        4,
        false,
    );
    b.cylinder(
        &ant,
        [xa, ya, za - 0.005],
        [xa, ya, za + 0.02],
        0.016,
        0.012,
        6,
        true,
    );
}

/// tansy_13: teardrop lamps, smile intake, tall tail lamps, hatch spoiler, rear wiper, black
/// bumper strips and rocker cladding.
fn supermini_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let zn = p.belt(p.y_nose);
    let wf = p.half_width(p.y_nose);
    reshape_pair(b, p, "headlight", |t, p, k| {
        lens_front(
            t,
            k,
            p,
            -(wf - 0.005),
            -0.30,
            zn - 0.12,
            zn - 0.045,
            0.012,
            0.035,
        );
    });
    // Teardrop tail of the lamp = the amber turn signal: a narrower box sweeping up toward the
    // fender corner, same face. It stops 1 cm short of the corner: its flat face is a chord of
    // the rounded plan, and past the corner the bumper wrap would cover it
    // (`lights_sit_outside_the_shell`).
    reshape_indicators(b, p, true, M::EmissiveAmber, |t, p, k| {
        lens_front(
            t,
            k,
            p,
            -(wf - 0.01),
            -(wf - 0.10),
            zn - 0.04,
            zn + 0.0,
            0.012,
            0.05,
        );
    });
    let zt = p.belt(p.y_tail);
    let wt = p.half_width(p.y_tail);
    // Tall tail lamps: red top and bottom, an amber band between them.
    let (tx0, tx1) = (-(wt - 0.005), -(wt - 0.17));
    reshape_pair(b, p, "taillight", |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, zt - 0.22, zt - 0.095, 0.012, 0.03);
        lens_rear(t, k, p, tx0, tx1, zt - 0.025, zt + 0.06, 0.012, 0.03);
    });
    reshape_indicators(b, p, false, M::EmissiveAmber, |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, zt - 0.09, zt - 0.03, 0.012, 0.03);
    });
    let slot = key("grille_upper", R::FrontBumper, M::Grille);
    front_box(b, &slot, p, -0.24, 0.24, zn - 0.10, zn - 0.05, 0.006, 0.03);
    // Smile intake: centre box + two raised ends.
    let smile = key("grille_lower", R::FrontBumper, M::Grille);
    front_box(b, &smile, p, -0.30, 0.30, 0.27, 0.37, 0.006, 0.03);
    front_box(b, &smile, p, -0.42, -0.30, 0.30, 0.40, 0.006, 0.03);
    front_box(b, &smile, p, 0.30, 0.42, 0.30, 0.40, 0.006, 0.03);
    bumper(b, p, true, 0.44, 0.50, 0.025, M::Trim, None);
    bumper(b, p, false, 0.40, 0.46, 0.025, M::Trim, None);
    plate_front(b, p, 0.51);
    plate_rear(b, p, zt - 0.29);
    // Hatch spoiler over the rear glass + rear wiper.
    let sp = key("hatch_spoiler", R::Roof, M::Paint);
    let (r0, _, rz, _) = p.roof;
    let hw = p.roof_half_w(r0) - 0.04;
    b.beveled_box(
        &sp,
        [-hw, r0 - 0.16, rz - 0.02],
        [hw, r0 + 0.06, rz + 0.025],
        0.012,
    );
    let wip = key("rear_wiper", R::Trunk, M::Trim);
    let yw = p.y_deck + 0.10;
    let zw = p.top(yw) + 0.012;
    b.oriented_box(
        &wip,
        [-0.10, yw + 0.02, zw + 0.02],
        unit3([0.94, 0.0, 0.34], [0.0, 1.0, 0.0]),
        [0.18, 0.008, 0.008],
    );
    rocker_cladding(b, p, 0.09, M::Trim);
    handles(b, p, M::Trim, false);
    side_markers(b, p, p.z_bumper_top.0 + 0.04);
    wipers(b, p, 0.22);
}

/// furrow_24: framed bar grille, square lamps + amber corners, chrome bumper, bed rail caps,
/// tailgate handle, vertical tail lamps, chrome step bumper, tall black mirrors, mud flaps.
fn compact_pickup_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let zn = p.belt(p.y_nose);
    let wf = p.half_width(p.y_nose);
    let (l0, l1) = (zn - 0.17, zn - 0.03);
    reshape_pair(b, p, "headlight", |t, p, k| {
        lens_front(t, k, p, -(wf - 0.10), -0.45, l0, l1, 0.008, 0.03);
    });
    // The amber corner lamp beside each square lamp is its turn signal.
    reshape_indicators(b, p, true, M::EmissiveAmber, |t, p, k| {
        lens_front(t, k, p, -(wf - 0.003), -(wf - 0.09), l0, l1, 0.008, 0.03);
    });
    grille(b, p, 0.43, l0 - 0.01, l1 + 0.01, 2, M::Chrome);
    bumper(b, p, true, 0.42, 0.60, 0.09, M::Chrome, None);
    let val = key("valance", R::FrontBumper, M::Trim);
    front_box(b, &val, p, -(wf - 0.10), wf - 0.10, 0.30, 0.42, 0.02, 0.03);
    plate_front(b, p, 0.44);
    let zt = p.belt(p.y_tail);
    let wt = p.half_width(p.y_tail);
    // Vertical tail lamps; their top section is the red-on-red turn signal.
    let (tx0, tx1) = (-(wt - 0.005), -(wt - 0.11));
    reshape_pair(b, p, "taillight", |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, zt - 0.36, zt - 0.155, 0.012, 0.03);
    });
    reshape_indicators(b, p, false, M::Taillight, |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, zt - 0.15, zt - 0.06, 0.012, 0.03);
    });
    tailgate_handle(b, p, zt - 0.10);
    step_bumper(b, p, 0.38, 0.54, 0.13, M::Chrome, false);
    plate_rear(b, p, 0.56);
    bed_rails(b, p, M::Trim);
    tall_mirrors(b, p, (0.10, 0.06, 0.18), 0.08);
    mud_flaps(b, p);
    handles(b, p, M::Trim, false);
    side_markers(b, p, p.z_bumper_top.0 + 0.05);
    wipers(b, p, 0.22);
}

/// hauler_1500: big two-bar chrome grille, tall lamps, chrome bumper + red tow hooks, running
/// boards, towing mirrors, cab clearance lamps, bed caps, step bumper with hitch, crew-cab
/// handles.
fn fullsize_pickup_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let zn = p.belt(p.y_nose);
    let wf = p.half_width(p.y_nose);
    let (l0, l1) = (zn - 0.20, zn - 0.03);
    // Tall lamps with an amber strip along the bottom of the lens.
    reshape_pair(b, p, "headlight", |t, p, k| {
        lens_front(t, k, p, -(wf - 0.01), -0.64, l0 + 0.05, l1, 0.012, 0.03);
    });
    reshape_indicators(b, p, true, M::EmissiveAmber, |t, p, k| {
        lens_front(t, k, p, -(wf - 0.01), -0.64, l0, l0 + 0.045, 0.012, 0.03);
    });
    grille(b, p, 0.62, l0 - 0.10, l1 + 0.0, 2, M::Chrome);
    // Lower grille in the bumper.
    let lg = key("grille_lower", R::FrontBumper, M::Grille);
    front_box(b, &lg, p, -0.45, 0.45, 0.50, 0.60, 0.008, 0.03);
    bumper(b, p, true, 0.40, 0.70, 0.11, M::Chrome, None);
    // Re-cut the lower grille through the bumper face.
    front_box(b, &lg, p, -0.45, 0.45, 0.50, 0.60, 0.12, 0.0);
    let hook = key("tow_hook_l", R::FrontBumper, M::Caliper);
    front_box(b, &hook, p, -0.56, -0.48, 0.33, 0.40, 0.14, 0.0);
    mirror(b, &hook);
    plate_front(b, p, 0.42);
    let zt = p.belt(p.y_tail);
    let wt = p.half_width(p.y_tail);
    // Tall tail lamps; their top section is the red-on-red turn signal.
    let (tx0, tx1) = (-(wt - 0.005), -(wt - 0.12));
    reshape_pair(b, p, "taillight", |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, zt - 0.42, zt - 0.165, 0.012, 0.03);
    });
    reshape_indicators(b, p, false, M::Taillight, |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, zt - 0.16, zt - 0.04, 0.012, 0.03);
    });
    tailgate_handle(b, p, zt - 0.10);
    step_bumper(b, p, 0.42, 0.62, 0.14, M::Chrome, true);
    plate_rear(b, p, 0.64);
    bed_rails(b, p, M::Trim);
    // Running boards between the arches.
    let rb = key("running_board_l", R::FrameLeft, M::Trim);
    let (ya, yb) = (p.y_quarter.0 + 0.30, p.y_fd_hinge + 0.05);
    let zs = p.z_floor + 0.10;
    let xs = p.half_w + 0.02;
    b.cuboid(&rb, [-(xs + 0.13), ya, zs], [-(xs - 0.10), yb, zs + 0.045]);
    mirror(b, &rb);
    tall_mirrors(b, p, (0.12, 0.08, 0.24), 0.14);
    // Cab clearance lamps on the roof's front edge.
    let cl = light("clearance_lamps", R::Roof, M::EmissiveAmber);
    let yc = p.roof.1 - 0.05;
    let zc = p.top_point(yc, 0.0).0[2];
    for x in [-0.25f32, 0.0, 0.25] {
        b.cuboid(
            &cl,
            [x - 0.035, yc - 0.03, zc - 0.01],
            [x + 0.035, yc + 0.01, zc + 0.022],
        );
    }
    handles(b, p, M::Chrome, true);
    side_markers(b, p, p.z_bumper_top.0 + 0.06);
    wipers(b, p, 0.30);
}

/// drayman_cargo: wide bar grille, black bumpers + rub strip, sliding-door track (right),
/// barn-door seam + hinges, tall tail lamps, roof marker lamps, big two-glass mirrors, step
/// bumper.
fn van_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let zn = p.belt(p.y_nose);
    let wf = p.half_width(p.y_nose);
    let (l0, l1) = (zn - 0.20, zn - 0.03);
    // Lamps with a vertical amber strip in the outer end of the lens.
    reshape_pair(b, p, "headlight", |t, p, k| {
        lens_front(t, k, p, -(wf - 0.095), -0.53, l0, l1, 0.012, 0.03);
    });
    reshape_indicators(b, p, true, M::EmissiveAmber, |t, p, k| {
        lens_front(t, k, p, -(wf - 0.01), -(wf - 0.09), l0, l1, 0.012, 0.03);
    });
    grille(b, p, 0.51, l0, l1 - 0.02, 3, M::Trim);
    bumper(b, p, true, 0.40, 0.66, 0.08, M::Trim, None);
    plate_front(b, p, 0.42);
    let zt = p.top(p.y_tail + 0.05);
    let wt = p.half_width(p.y_tail);
    // Tall tail lamps with the amber turn signal as their bottom section.
    let (tx0, tx1) = (-(wt - 0.005), -(wt - 0.10));
    reshape_pair(b, p, "taillight", |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, 1.065, 1.40, 0.012, 0.03);
    });
    reshape_indicators(b, p, false, M::EmissiveAmber, |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, 0.95, 1.06, 0.012, 0.03);
    });
    // Barn doors: centre seam, hinges, handle.
    let seam = key("barn_seam", R::Trunk, M::Trim);
    rear_box(b, &seam, p, -0.006, 0.006, 0.72, zt - 0.10, 0.004, 0.02);
    let hinge = key("barn_hinge_l", R::Trunk, M::Trim);
    for z in [0.95f32, 1.70] {
        rear_box(
            b,
            &hinge,
            p,
            -(wt - 0.005),
            -(wt - 0.06),
            z - 0.05,
            z + 0.05,
            0.02,
            0.01,
        );
    }
    mirror(b, &hinge);
    let hd = key("barn_handle", R::Trunk, M::Chrome);
    rear_box(b, &hd, p, 0.03, 0.17, 1.10, 1.14, 0.025, 0.0);
    // Roof marker lamps above the barn doors.
    let ml = light("roof_markers", R::Roof, M::Taillight);
    let ym = p.y_tail + 0.06;
    for x in [-0.25f32, 0.0, 0.25] {
        let z = p.top_point(ym, x).0[2];
        b.cuboid(
            &ml,
            [x - 0.03, ym - 0.06, z - 0.02],
            [x + 0.03, ym - 0.03, z + 0.02],
        );
    }
    step_bumper(b, p, 0.40, 0.56, 0.22, M::Trim, false);
    plate_rear(b, p, 0.58);
    side_strip(
        b,
        p,
        "rub_strip",
        M::Trim,
        (p.y_tail + 0.25, p.y_fd_hinge + 0.4),
        (0.78, 0.86),
        0.016,
    );
    sliding_door_track(b, p);
    tall_mirrors(b, p, (0.12, 0.08, 0.30), 0.16);
    handles(b, p, M::Trim, false);
    side_markers(b, p, p.z_bumper_top.0 + 0.08);
    wipers(b, p, 0.30);
}

// =======================================================================================
// Surface helpers
/// satchel_20: swept corner lamps wrapping onto the fenders, a slim body-colour grille bar
/// over a black lower intake, body-colour bumpers with a black valance, black rocker
/// cladding, sliding-door track, barn doors with a centre seam and small car-like mirrors.
fn short_nose_van_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let zn = p.belt(p.y_nose);
    let zs = p.sill(p.y_nose);
    let wf = p.half_width(p.y_nose);
    // Tall swept lamps in the nose corners; the amber signal is their lower section.
    let (l0, l1) = (zn - 0.30, zn - 0.05);
    reshape_pair(b, p, "headlight", |t, p, k| {
        lens_front(
            t,
            k,
            p,
            -(wf - 0.02),
            -(wf - 0.26),
            l0 + 0.08,
            l1,
            0.014,
            0.03,
        );
    });
    reshape_indicators(b, p, true, M::EmissiveAmber, |t, p, k| {
        lens_front(
            t,
            k,
            p,
            -(wf - 0.02),
            -(wf - 0.26),
            l0,
            l0 + 0.065,
            0.014,
            0.03,
        );
    });
    // Slim grille bar between the lamps, black intake under it.
    grille(b, p, wf - 0.30, l1 - 0.085, l1 - 0.015, 1, M::Paint);
    let lower = key("lower_intake", R::FrontBumper, M::Grille);
    front_box(b, &lower, p, -0.44, 0.44, zs + 0.14, zs + 0.30, 0.004, 0.03);
    bumper(b, p, true, zs + 0.06, zs + 0.38, 0.07, M::Paint, None);
    let valance = key("front_valance", R::FrontBumper, M::Trim);
    front_box(b, &valance, p, -wf, wf, zs, zs + 0.09, 0.055, 0.02);
    plate_front(b, p, zs + 0.16);

    // Rear: tall narrow lamps standing on the barn-door corners, step bumper.
    let zt = p.top(p.y_tail + 0.05);
    let zb = p.sill(p.y_tail);
    let wt = p.half_width(p.y_tail);
    let (tx0, tx1) = (-(wt - 0.01), -(wt - 0.11));
    let (t0, t1) = (zb + 0.42 * (zt - zb), zb + 0.78 * (zt - zb));
    reshape_pair(b, p, "taillight", |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, t0 + 0.12, t1, 0.012, 0.03);
    });
    reshape_indicators(b, p, false, M::EmissiveAmber, |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, t0, t0 + 0.11, 0.012, 0.03);
    });
    let seam = key("barn_seam", R::Trunk, M::Trim);
    rear_box(
        b,
        &seam,
        p,
        -0.006,
        0.006,
        zb + 0.30,
        zt - 0.10,
        0.004,
        0.02,
    );
    let hd = key("barn_handle", R::Trunk, M::Trim);
    rear_box(b, &hd, p, 0.03, 0.16, t1 - 0.02, t1 + 0.02, 0.022, 0.0);
    step_bumper(b, p, zb + 0.04, zb + 0.22, 0.18, M::Paint, false);
    plate_rear(b, p, zb + 0.26);

    rocker_cladding(b, p, 0.10, M::Trim);
    sliding_door_track(b, p);
    black_mirrors(b, p, (0.10, 0.07, 0.20), 0.10);
    handles(b, p, M::Paint, false);
    side_markers(b, p, p.z_bumper_top.0 + 0.10);
    wipers(b, p, 0.34);
}

/// tumbril_20: a flat cab-over face — square lamps low in the corners either side of a
/// full-width black face panel with a slim grille slot, a deep black bumper with a centre
/// step, ribbed flanks, barn doors and tall van mirrors.
fn cabover_van_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let zn = p.belt(p.y_nose);
    let zs = p.sill(p.y_nose);
    let wf = p.half_width(p.y_nose);
    // Square lamps low in the corners; the amber signal is their outer end.
    let (l0, l1) = (zs + 0.26, zs + 0.44);
    reshape_pair(b, p, "headlight", |t, p, k| {
        lens_front(t, k, p, -(wf - 0.13), -(wf - 0.40), l0, l1, 0.014, 0.03);
    });
    reshape_indicators(b, p, true, M::EmissiveAmber, |t, p, k| {
        lens_front(t, k, p, -(wf - 0.01), -(wf - 0.12), l0, l1, 0.014, 0.03);
    });
    // Full-width black face panel between the lamps and the screen, slim grille slot in it.
    let face = key("face_panel", R::Hood, M::Trim);
    front_box(
        b,
        &face,
        p,
        -wf + 0.02,
        wf - 0.02,
        l1 + 0.04,
        zn - 0.03,
        0.006,
        0.02,
    );
    grille(b, p, 0.46, l1 + 0.09, l1 + 0.17, 1, M::Trim);
    bumper(b, p, true, zs, zs + 0.24, 0.085, M::Trim, None);
    let step = key("front_step", R::FrontBumper, M::Trim);
    front_box(b, &step, p, -0.24, 0.24, zs + 0.24, zs + 0.265, 0.075, 0.0);
    plate_front(b, p, zs + 0.07);

    // Rear: tall barn doors with ribs, square lamps standing on the bumper, step bumper.
    let zt = p.top(p.y_tail + 0.05);
    let zb = p.sill(p.y_tail);
    let wt = p.half_width(p.y_tail);
    let (tx0, tx1) = (-(wt - 0.01), -(wt - 0.12));
    let (t0, t1) = (zb + 0.24, zb + 0.62);
    reshape_pair(b, p, "taillight", |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, t0 + 0.13, t1, 0.012, 0.03);
    });
    reshape_indicators(b, p, false, M::EmissiveAmber, |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, t0, t0 + 0.12, 0.012, 0.03);
    });
    let seam = key("barn_seam", R::Trunk, M::Trim);
    rear_box(
        b,
        &seam,
        p,
        -0.006,
        0.006,
        zb + 0.18,
        zt - 0.08,
        0.004,
        0.02,
    );
    let hinge = key("barn_hinge_l", R::Trunk, M::Trim);
    for f in [0.30f32, 0.78] {
        let z = zb + f * (zt - zb);
        rear_box(
            b,
            &hinge,
            p,
            -(wt - 0.005),
            -(wt - 0.06),
            z - 0.05,
            z + 0.05,
            0.02,
            0.01,
        );
    }
    mirror(b, &hinge);
    let hd = key("barn_handle", R::Trunk, M::Chrome);
    rear_box(b, &hd, p, 0.03, 0.17, t1 + 0.05, t1 + 0.09, 0.025, 0.0);
    step_bumper(b, p, zb + 0.02, zb + 0.20, 0.22, M::Trim, false);
    plate_rear(b, p, zb + 0.24);

    // Ribbed flanks: two thin strips along the panel sides.
    let (ry0, ry1) = (p.y_tail + 0.22, p.y_fd_hinge + 0.35);
    let zr = p.z_bumper_top.0;
    for (i, z) in [zr + 0.16, zr + 0.34].into_iter().enumerate() {
        side_strip(
            b,
            p,
            &format!("rib_{i}"),
            M::Paint,
            (ry0, ry1),
            (z, z + 0.035),
            0.010,
        );
    }
    tall_mirrors(b, p, (0.12, 0.08, 0.32), 0.18);
    handles(b, p, M::Trim, false);
    side_markers(b, p, zr + 0.06);
    wipers(b, p, 0.40);
}

// =======================================================================================

/// A detail key.
fn key(name: &str, region: BodyRegion, mat: MaterialSlot) -> PartKey {
    PartKey::body(name, region, PartKind::Detail, mat)
}

/// A lamp key (kind `Light`).
fn light(name: &str, region: BodyRegion, mat: MaterialSlot) -> PartKey {
    PartKey::body(name, region, PartKind::Light, mat)
}

/// Mirror part `k` (LEFT) to the right side.
fn mirror(b: &mut Builder, k: &PartKey) {
    b.mirror_part(&k.name, &k.mirrored());
}

/// Two orthonormal box axes from an x axis and a y hint (z = x × y).
fn unit3(x: [f32; 3], y_hint: [f32; 3]) -> [[f32; 3]; 3] {
    let n = |v: [f32; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
        [v[0] / l, v[1] / l, v[2] / l]
    };
    let cr = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let x = n(x);
    let z = n(cr(x, y_hint));
    let y = cr(z, x);
    [x, y, z]
}

/// Front-face y at lateral offset `x` (follows the plan-view corner rounding).
pub(crate) fn nose_y(p: &BodyProfile, x: f32) -> f32 {
    end_y(p, x, true)
}

/// Rear-face y at lateral offset `x`.
pub(crate) fn tail_y(p: &BodyProfile, x: f32) -> f32 {
    end_y(p, x, false)
}

fn end_y(p: &BodyProfile, x: f32, front: bool) -> f32 {
    let ax = x.abs();
    let (end, dir) = if front {
        (p.y_nose, -1.0)
    } else {
        (p.y_tail, 1.0)
    };
    if ax <= p.half_width(end) {
        return end;
    }
    // Bisection between the end (narrower than ax) and 0.6 m inward (wide enough).
    let (mut a, mut c) = (end, end + dir * 0.6);
    if p.half_width(c) < ax {
        return c;
    }
    for _ in 0..24 {
        let m = 0.5 * (a + c);
        if p.half_width(m) >= ax {
            c = m;
        } else {
            a = m;
        }
    }
    c
}

/// Emit a hexahedron from 8 corners indexed by bits (1 = +x side, 2 = +y side, 4 = +z side);
/// faces are oriented away from its centroid.
fn hexa(b: &mut Builder, k: &PartKey, c: [[f32; 3]; 8]) {
    hexa_open(b, k, c, None);
}

/// [`hexa`] without face `skip` (index into its face list: 0 / 1 = −x / +x, 2 / 3 = −y / +y,
/// 4 / 5 = −z / +z), for a face that is always hidden inside the body.
fn hexa_open(b: &mut Builder, k: &PartKey, c: [[f32; 3]; 8], skip: Option<usize>) {
    let mut ctr = [0.0f32; 3];
    for q in &c {
        for i in 0..3 {
            ctr[i] += q[i] / 8.0;
        }
    }
    let faces = [
        [0, 4, 6, 2],
        [1, 3, 7, 5],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 2, 3, 1],
        [4, 5, 7, 6],
    ];
    for (i, f) in faces.into_iter().enumerate() {
        if skip == Some(i) {
            continue;
        }
        let q = [c[f[0]], c[f[1]], c[f[2]], c[f[3]]];
        let mut m = [0.0f32; 3];
        for v in &q {
            for i in 0..3 {
                m[i] += v[i] / 4.0;
            }
        }
        b.poly_facing(k, &q, [m[0] - ctr[0], m[1] - ctr[1], m[2] - ctr[2]]);
    }
}

/// Box on the front face between `x0 < x1`, `z0..z1`, standing `out` proud of the surface
/// and sunk `inn` into it; its faces follow the plan rounding at both x ends.
#[allow(clippy::too_many_arguments)]
fn front_box(
    b: &mut Builder,
    k: &PartKey,
    p: &BodyProfile,
    x0: f32,
    x1: f32,
    z0: f32,
    z1: f32,
    out: f32,
    inn: f32,
) {
    hexa(b, k, front_corners(p, [x0, x1, z0, z1], out, inn));
}

/// Corners of a [`front_box`] over `[x0, x1, z0, z1]`.
fn front_corners(p: &BodyProfile, r: [f32; 4], out: f32, inn: f32) -> [[f32; 3]; 8] {
    let mut c = [[0.0f32; 3]; 8];
    for (i, q) in c.iter_mut().enumerate() {
        let x = if i & 1 == 0 { r[0] } else { r[1] };
        let y = nose_y(p, x) + if i & 2 == 0 { -inn } else { out };
        let z = if i & 4 == 0 { r[2] } else { r[3] };
        *q = [x, y, z];
    }
    c
}

/// Lamp lens on the front face: a [`front_box`] without its back face (sunk `inn` into the
/// body, never seen). Head lamps and the turn-signal segments cut out of them use it.
#[allow(clippy::too_many_arguments)]
fn lens_front(
    b: &mut Builder,
    k: &PartKey,
    p: &BodyProfile,
    x0: f32,
    x1: f32,
    z0: f32,
    z1: f32,
    out: f32,
    inn: f32,
) {
    hexa_open(b, k, front_corners(p, [x0, x1, z0, z1], out, inn), Some(2));
}

/// Box on the rear face (as [`front_box`], `out` = further back).
#[allow(clippy::too_many_arguments)]
fn rear_box(
    b: &mut Builder,
    k: &PartKey,
    p: &BodyProfile,
    x0: f32,
    x1: f32,
    z0: f32,
    z1: f32,
    out: f32,
    inn: f32,
) {
    hexa(b, k, rear_corners(p, [x0, x1, z0, z1], out, inn));
}

/// Corners of a [`rear_box`] over `[x0, x1, z0, z1]`.
fn rear_corners(p: &BodyProfile, r: [f32; 4], out: f32, inn: f32) -> [[f32; 3]; 8] {
    let mut c = [[0.0f32; 3]; 8];
    for (i, q) in c.iter_mut().enumerate() {
        let x = if i & 1 == 0 { r[0] } else { r[1] };
        let y = tail_y(p, x) + if i & 2 == 0 { -out } else { inn };
        let z = if i & 4 == 0 { r[2] } else { r[3] };
        *q = [x, y, z];
    }
    c
}

/// Lamp lens on the rear face: a [`rear_box`] without its back face (inside the body).
#[allow(clippy::too_many_arguments)]
fn lens_rear(
    b: &mut Builder,
    k: &PartKey,
    p: &BodyProfile,
    x0: f32,
    x1: f32,
    z0: f32,
    z1: f32,
    out: f32,
    inn: f32,
) {
    hexa_open(b, k, rear_corners(p, [x0, x1, z0, z1], out, inn), Some(3));
}

/// Box on the LEFT side surface over `y0..y1`, `z0..z1`, `out` proud, `inn` sunk.
#[allow(clippy::too_many_arguments)]
fn side_box(
    b: &mut Builder,
    k: &PartKey,
    p: &BodyProfile,
    y0: f32,
    y1: f32,
    z0: f32,
    z1: f32,
    out: f32,
    inn: f32,
) {
    let mut c = [[0.0f32; 3]; 8];
    for (i, q) in c.iter_mut().enumerate() {
        let y = if i & 2 == 0 { y0 } else { y1 };
        let z = if i & 4 == 0 { z0 } else { z1 };
        let (s, n) = p.side_point(y, z);
        let d = if i & 1 == 0 { out } else { -inn };
        *q = [s[0] + n[0] * d, y, s[2] + n[2] * d];
    }
    hexa(b, k, c);
}

/// LEFT-side panel region at station `y` (between the sill and the belt).
fn side_region(p: &BodyProfile, y: f32) -> BodyRegion {
    if y > p.y_fd_hinge {
        R::FrontLeftFender
    } else if y < p.y_quarter.0 {
        R::RearLeftQuarter
    } else if let Some((rh, re)) = p.rear_door {
        if y > rh {
            R::LeftDoorF
        } else if y > re {
            R::LeftDoorR
        } else {
            R::RearLeftQuarter
        }
    } else if y > p.y_fd_rear {
        R::LeftDoorF
    } else {
        R::LeftDoorR
    }
}

/// True if the wheel arch cuts into the side at `y` below height `z`.
fn in_arch(p: &BodyProfile, y: f32, z: f32) -> bool {
    p.arch_floor(y).is_some_and(|a| a > z - 0.03)
}

/// Thin shell along the LEFT side (mirrored) over `ys`, heights `zs`, split at panel seams
/// and wheel arches; piece `n` is `{base}_{n}_l`, skinned to its panel.
fn side_strip(
    b: &mut Builder,
    p: &BodyProfile,
    base: &str,
    mat: MaterialSlot,
    ys: (f32, f32),
    zs: (f32, f32),
    thick: f32,
) {
    const STEP: f32 = 0.02;
    let n = ((ys.1 - ys.0) / STEP).ceil().max(1.0) as usize;
    let tag = |y: f32| -> Option<BodyRegion> {
        if in_arch(p, y, zs.0) {
            None
        } else {
            Some(side_region(p, y))
        }
    };
    let mut runs: Vec<(f32, f32, BodyRegion)> = Vec::new();
    let mut start = ys.0;
    let mut cur = tag(ys.0);
    for i in 1..=n {
        let y = (ys.0 + i as f32 * STEP).min(ys.1);
        let t = tag(y);
        if t != cur || i == n {
            if let Some(r) = cur {
                runs.push((start, y, r));
            }
            start = y;
            cur = t;
        }
    }
    for (i, (a, c, r)) in runs.into_iter().enumerate() {
        if c - a < 0.06 {
            continue;
        }
        let k = key(&format!("{base}_{i}_l"), r, mat);
        b.surface_patch(
            &k,
            p,
            &Patch {
                y0: a + 0.005,
                y1: c - 0.005,
                band: PatchBand::Side { z0: zs.0, z1: zs.1 },
                offset_m: 0.002,
                thickness_m: thick,
                // Sides are nearly straight: a station every 0.25 m is enough.
                steps: ((((c - a) / 0.25).ceil() as usize).max(1), 1),
            },
        );
        mirror(b, &k);
    }
}

/// Replace the foundation's turn-signal lenses of the front (`front`) or rear end with what
/// `f` emits for the LEFT one into the key it is given (the right is its mirror image). The
/// lens is a segment of that end's lamp cluster (same face and depth as the lamp's
/// [`front_box`] / [`rear_box`]); `mat` is `EmissiveAmber`, or `Taillight` for a red-on-red
/// rear signal.
fn reshape_indicators(
    b: &mut Builder,
    p: &BodyProfile,
    front: bool,
    mat: MaterialSlot,
    f: impl Fn(&mut Builder, &BodyProfile, &PartKey),
) {
    let (kl, kr) = if front {
        (indicator_front_key(false), indicator_front_key(true))
    } else {
        (
            indicator_rear_key(false, mat),
            indicator_rear_key(true, mat),
        )
    };
    let mut t = Builder::new();
    f(&mut t, p, &kl);
    t.mirror_part(&kl.name, &kr);
    for k in [&kl, &kr] {
        b.remove_part(&k.name);
        let (pos, uv, idx) = t
            .geometry(&k.name)
            .map(|(a, u, i)| (a.to_vec(), u.to_vec(), i.to_vec()))
            .unwrap_or_default();
        b.part(k);
        b.set_geometry(&k.name, pos, uv, idx);
    }
}

/// Replace the geometry of the generic `{base}_l` / `{base}_r` parts (head / tail lights)
/// with what `f` emits for the LEFT one (the right is its mirror image).
fn reshape_pair(
    b: &mut Builder,
    p: &BodyProfile,
    base: &str,
    f: impl Fn(&mut Builder, &BodyProfile, &PartKey),
) {
    let keys = b.keys();
    let (Some(kl), Some(kr)) = (
        keys.iter().find(|k| k.name == format!("{base}_l")),
        keys.iter().find(|k| k.name == format!("{base}_r")),
    ) else {
        return;
    };
    let mut t = Builder::new();
    f(&mut t, p, kl);
    t.mirror_part(&kl.name, kr);
    for k in [kl, kr] {
        let (pos, uv, idx) = t
            .geometry(&k.name)
            .map(|(a, u, i)| (a.to_vec(), u.to_vec(), i.to_vec()))
            .unwrap_or_default();
        b.set_geometry(&k.name, pos, uv, idx);
    }
}

// =======================================================================================
// Trim pieces
// =======================================================================================

/// Grille between the head lamps: dark mesh box, a frame (`frame` material) and `slats`
/// horizontal bars.
fn grille(
    b: &mut Builder,
    p: &BodyProfile,
    half: f32,
    z0: f32,
    z1: f32,
    slats: usize,
    frame: MaterialSlot,
) {
    let g = key("grille", R::FrontBumper, M::Grille);
    front_box(b, &g, p, -half, half, z0, z1, 0.004, 0.03);
    let f = key("grille_frame", R::FrontBumper, frame);
    let t = 0.022;
    front_box(b, &f, p, -half - t, half + t, z1, z1 + t, 0.016, 0.01);
    front_box(b, &f, p, -half - t, half + t, z0 - t, z0, 0.016, 0.01);
    front_box(b, &f, p, -half - t, -half, z0, z1, 0.016, 0.01);
    front_box(b, &f, p, half, half + t, z0, z1, 0.016, 0.01);
    for i in 1..=slats {
        let z = z0 + (z1 - z0) * i as f32 / (slats + 1) as f32;
        front_box(b, &f, p, -half, half, z - 0.008, z + 0.008, 0.012, 0.0);
    }
}

/// Bumper bar across the front (or rear) face, `out` proud, wrapping 0.22 m round the
/// corners; optional thin top strip in `strip`.
#[allow(clippy::too_many_arguments)]
fn bumper(
    b: &mut Builder,
    p: &BodyProfile,
    front: bool,
    z0: f32,
    z1: f32,
    out: f32,
    mat: MaterialSlot,
    strip: Option<MaterialSlot>,
) {
    let (region, name) = if front {
        (R::FrontBumper, "bumper_bar_f")
    } else {
        (R::RearBumper, "bumper_bar_r")
    };
    let k = key(name, region, mat);
    let y_end = if front { p.y_nose } else { p.y_tail };
    let w = p.half_width(y_end) + 0.01;
    let face = |b: &mut Builder, k: &PartKey, x0: f32, x1: f32, za: f32, zb: f32, o: f32| {
        if front {
            front_box(b, k, p, x0, x1, za, zb, o, 0.03);
        } else {
            rear_box(b, k, p, x0, x1, za, zb, o, 0.03);
        }
    };
    face(b, &k, -(w - 0.10), w - 0.10, z0, z1, out);
    // Corner wraps: boxes running back along each side.
    let dir = if front { -1.0 } else { 1.0 };
    let yb = y_end + dir * 0.28;
    let wb = p.half_width(yb);
    let o = (0.35 * out).clamp(0.015, 0.04);
    let xe = -(w - 0.10);
    let ye = if front { nose_y(p, xe) } else { tail_y(p, xe) };
    // Plan quad: outer front (bar end face), inner front (sunk), outer back, inner back.
    let plan = [
        [xe, ye - dir * out],
        [xe + 0.02, ye + dir * 0.03],
        [-(wb + o), yb],
        [-(wb - 0.03), yb],
    ];
    let mut c = [[0.0f32; 3]; 8];
    for (i, q) in c.iter_mut().enumerate() {
        let pp = plan[i & 3];
        *q = [pp[0], pp[1], if i & 4 == 0 { z0 } else { z1 }];
    }
    let kw = key(&format!("{name}_wrap_l"), region, mat);
    hexa(b, &kw, c);
    mirror(b, &kw);
    if let Some(sm) = strip {
        let ks = key(&format!("{name}_strip"), region, sm);
        face(
            b,
            &ks,
            -(w - 0.12),
            w - 0.12,
            z1 - 0.012,
            z1 + 0.004,
            out + 0.004,
        );
    }
}

/// Plate recess on the front bumper (dark frame + light plate) at height `z` (bottom).
fn plate_front(b: &mut Builder, p: &BodyProfile, z: f32) {
    let f = key("plate_frame_f", R::FrontBumper, M::Trim);
    front_box(b, &f, p, -0.27, 0.27, z - 0.012, z + 0.132, 0.008, 0.02);
    let pl = key("plate_f", R::FrontBumper, M::Metal);
    front_box(b, &pl, p, -0.255, 0.255, z, z + 0.12, 0.014, 0.0);
}

/// Plate recess on the rear at height `z` (bottom).
fn plate_rear(b: &mut Builder, p: &BodyProfile, z: f32) {
    let region = if z > p.z_bumper_top.1 {
        R::Trunk
    } else {
        R::RearBumper
    };
    let f = key("plate_frame_r", region, M::Trim);
    rear_box(b, &f, p, -0.27, 0.27, z - 0.012, z + 0.132, 0.008, 0.02);
    let pl = key("plate_r", region, M::Metal);
    rear_box(b, &pl, p, -0.255, 0.255, z, z + 0.12, 0.014, 0.0);
}

/// Door handles: one per door, just ahead of the door's rear edge, below the belt.
fn handles(b: &mut Builder, p: &BodyProfile, mat: MaterialSlot, crew_cab: bool) {
    let mut doors = vec![(
        R::LeftDoorF,
        match p.rear_door {
            Some((rh, _)) => rh,
            None => p.y_fd_rear,
        },
    )];
    match p.rear_door {
        Some((_, re)) => doors.push((R::LeftDoorR, re)),
        // Crew cabs without a hinged rear door in the lattice: a handle on the rear panel.
        None if crew_cab => doors.push((R::LeftDoorR, p.y_quarter.0)),
        None => {}
    }
    let z = p.z_belt - 0.10;
    for (i, (r, y_rear)) in doors.into_iter().enumerate() {
        let y = y_rear + 0.17;
        let k = key(&format!("door_handle_{i}_l"), r, mat);
        side_box(
            b,
            &k,
            p,
            y - 0.07,
            y + 0.07,
            z - 0.014,
            z + 0.016,
            0.014,
            0.004,
        );
        mirror(b, &k);
    }
}

/// Amber marker on each front fender, red marker on each rear quarter, at height `z`.
fn side_markers(b: &mut Builder, p: &BodyProfile, z: f32) {
    let yf = p.y_nose - 0.22;
    let kf = light("side_marker_front_l", R::FrontLeftFender, M::EmissiveAmber);
    side_box(
        b,
        &kf,
        p,
        yf - 0.05,
        yf + 0.03,
        z - 0.02,
        z + 0.02,
        0.008,
        0.004,
    );
    mirror(b, &kf);
    let yr = p.y_tail + 0.22;
    let kr = light("side_marker_rear_l", R::RearLeftQuarter, M::Taillight);
    side_box(
        b,
        &kr,
        p,
        yr - 0.03,
        yr + 0.05,
        z - 0.02,
        z + 0.02,
        0.008,
        0.004,
    );
    mirror(b, &kr);
}

/// Two parked wipers along the windshield base, `len` long each.
fn wipers(b: &mut Builder, p: &BodyProfile, len: f32) {
    let k = key("wipers", R::Cabin, M::Trim);
    let y = p.y_fw - 0.07;
    for cx in [-0.32f32, 0.12] {
        let z = p.top_point(y, cx).0[2] + 0.012;
        b.oriented_box(
            &k,
            [cx, y, z],
            unit3([1.0, 0.12, 0.06], [0.0, 1.0, 0.0]),
            [len, 0.008, 0.007],
        );
    }
}

/// Chrome belt trim along the door glass bases.
fn belt_chrome(b: &mut Builder, p: &BodyProfile) {
    let z = p.z_belt;
    side_strip(
        b,
        p,
        "belt_trim",
        M::Chrome,
        (p.y_quarter.0 + 0.02, p.y_fd_hinge - 0.02),
        (z - 0.035, z - 0.012),
        0.006,
    );
}

/// Third brake lamp on the rear glass just above the deck.
fn third_brake_lamp(b: &mut Builder, p: &BodyProfile, half: f32) {
    let k = light("brake_lamp_c", R::Trunk, M::Taillight);
    let y = p.y_deck + 0.04;
    let z = p.top(y);
    b.cuboid(
        &k,
        [-half, y - 0.025, z - 0.01],
        [half, y + 0.015, z + 0.03],
    );
}

/// Shark-fin antenna on the roof's rear edge.
fn shark_fin(b: &mut Builder, p: &BodyProfile) {
    let k = key("roof_antenna", R::Roof, M::Trim);
    let y = p.roof.0 + 0.10;
    let z = p.top_point(y, 0.0).0[2];
    hexa(
        b,
        &k,
        [
            [-0.03, y - 0.08, z - 0.01],
            [0.03, y - 0.08, z - 0.01],
            [-0.03, y + 0.10, z - 0.01],
            [0.03, y + 0.10, z - 0.01],
            [-0.012, y - 0.06, z + 0.06],
            [0.012, y - 0.06, z + 0.06],
            [-0.012, y - 0.03, z + 0.06],
            [0.012, y - 0.03, z + 0.06],
        ],
    );
}

/// Chrome drip rails along both roof edges.
fn drip_rails(b: &mut Builder, p: &BodyProfile) {
    let k = key("drip_rail_l", R::Roof, M::Chrome);
    let (r0, r1, _, _) = p.roof;
    let n = 4;
    for i in 0..n {
        let ya = r0 + (r1 - r0) * i as f32 / n as f32;
        let yb = r0 + (r1 - r0) * (i + 1) as f32 / n as f32;
        let x = |y: f32| -(p.roof_half_w(y) + 0.004);
        let z = |y: f32| p.top(y) - 0.035;
        hexa(
            b,
            &k,
            [
                [x(ya) - 0.012, ya, z(ya)],
                [x(ya) + 0.01, ya, z(ya)],
                [x(yb) - 0.012, yb, z(yb)],
                [x(yb) + 0.01, yb, z(yb)],
                [x(ya) - 0.012, ya, z(ya) + 0.016],
                [x(ya) + 0.01, ya, z(ya) + 0.016],
                [x(yb) - 0.012, yb, z(yb) + 0.016],
                [x(yb) + 0.01, yb, z(yb) + 0.016],
            ],
        );
    }
    mirror(b, &k);
}

/// Hide the generic paint mirrors.
fn hide_generic_mirrors(b: &mut Builder) {
    for n in ["mirror_l", "mirror_r"] {
        b.set_geometry(n, Vec::new(), Vec::new(), Vec::new());
    }
}

/// Small black door mirrors (`size` = out × along × up) on a stub `arm` long.
fn black_mirrors(b: &mut Builder, p: &BodyProfile, size: (f32, f32, f32), arm: f32) {
    hide_generic_mirrors(b);
    let y = p.y_fd_hinge - 0.14;
    let zb = p.belt(y);
    let (s, _) = p.side_point(y, zb - 0.02);
    let x0 = s[0];
    let k = PartKey::body("door_mirror_l", R::LeftDoorF, PartKind::Mirror, M::Trim);
    b.cuboid(
        &k,
        [x0 - arm, y - 0.03, zb + 0.0],
        [x0 + 0.01, y + 0.02, zb + 0.03],
    );
    b.beveled_box(
        &k,
        [x0 - arm - size.0 + 0.02, y - size.1 * 0.5, zb + 0.01],
        [x0 - arm + 0.02, y + size.1 * 0.5, zb + 0.01 + size.2],
        0.012,
    );
    mirror(b, &k);
}

/// Tall truck / van mirrors: two arms, a black head with a glass face toward the rear.
fn tall_mirrors(b: &mut Builder, p: &BodyProfile, size: (f32, f32, f32), arm: f32) {
    hide_generic_mirrors(b);
    let y = p.y_fd_hinge - 0.12;
    let zb = p.belt(y);
    let (s, _) = p.side_point(y, zb - 0.03);
    let x0 = s[0];
    let k = PartKey::body("door_mirror_l", R::LeftDoorF, PartKind::Mirror, M::Trim);
    let xh = x0 - arm;
    for dz in [0.0f32, size.2 - 0.05] {
        b.cuboid(
            &k,
            [xh, y - 0.015, zb + dz],
            [x0 + 0.01, y + 0.015, zb + dz + 0.025],
        );
    }
    let lo = [xh - size.0, y - size.1 * 0.5, zb - 0.02];
    let hi = [xh, y + size.1 * 0.5, zb - 0.02 + size.2];
    b.cuboid(&k, lo, hi);
    mirror(b, &k);
    let g = PartKey::body(
        "door_mirror_glass_l",
        R::LeftDoorF,
        PartKind::Mirror,
        M::Chrome,
    );
    b.cuboid(
        &g,
        [lo[0] + 0.012, lo[1] - 0.004, lo[2] + 0.012],
        [hi[0] - 0.012, lo[1] + 0.002, hi[2] - 0.012],
    );
    mirror(b, &g);
}

/// Black rocker cladding along the sills between the arches.
fn rocker_cladding(b: &mut Builder, p: &BodyProfile, h: f32, mat: MaterialSlot) {
    let z0 = p.z_floor + 0.07;
    side_strip(
        b,
        p,
        "cladding",
        mat,
        (p.y_tail + 0.3, p.y_nose - 0.3),
        (z0, z0 + h),
        0.012,
    );
}

/// Tailgate handle (black bezel) at height `z` on the rear face.
fn tailgate_handle(b: &mut Builder, p: &BodyProfile, z: f32) {
    let k = key("tailgate_handle", R::Trunk, M::Trim);
    rear_box(b, &k, p, -0.13, 0.13, z - 0.035, z + 0.035, 0.018, 0.0);
}

/// Rear step bumper (`out` deep) with a step pad; `hitch` adds a receiver + ball.
fn step_bumper(
    b: &mut Builder,
    p: &BodyProfile,
    z0: f32,
    z1: f32,
    out: f32,
    mat: MaterialSlot,
    hitch: bool,
) {
    bumper(b, p, false, z0, z1, out, mat, None);
    let pad = key("step_pad", R::RearBumper, M::Trim);
    rear_box(b, &pad, p, -0.22, 0.22, z1, z1 + 0.012, out - 0.01, 0.0);
    if hitch {
        let h = key("hitch", R::RearBumper, M::Trim);
        rear_box(b, &h, p, -0.04, 0.04, z0 - 0.08, z0 - 0.01, out + 0.12, 0.0);
        let ball = key("hitch_ball", R::RearBumper, M::Chrome);
        let y = tail_y(p, 0.0) - out - 0.08;
        b.cylinder(
            &ball,
            [0.0, y, z0 - 0.01],
            [0.0, y, z0 + 0.03],
            0.012,
            0.012,
            6,
            false,
        );
        b.cylinder(
            &ball,
            [0.0, y, z0 + 0.03],
            [0.0, y, z0 + 0.08],
            0.026,
            0.0,
            8,
            true,
        );
    }
}

/// Black caps along both bed rails (pickups with an open bed).
fn bed_rails(b: &mut Builder, p: &BodyProfile, mat: MaterialSlot) {
    let Some((y0, y1, _)) = p.bed else {
        return;
    };
    let k = key("bed_cap_l", R::RearLeftQuarter, mat);
    let n = 4;
    for i in 0..n {
        let ya = y0 + 0.02 + (y1 - y0 - 0.04) * i as f32 / n as f32;
        let yb = y0 + 0.02 + (y1 - y0 - 0.04) * (i + 1) as f32 / n as f32;
        let corner = |y: f32| {
            let r = p.ring(y);
            // Ring point 7 = shoulder top, 8 = rail inner edge (open bed).
            (r[7].p, r[8].p)
        };
        let (oa, ia) = corner(ya);
        let (ob, ib) = corner(yb);
        let lift = 0.022;
        hexa(
            b,
            &k,
            [
                [oa[0] - 0.012, ya, oa[2] - 0.01],
                [ia[0] + 0.012, ya, ia[2] - 0.01],
                [ob[0] - 0.012, yb, ob[2] - 0.01],
                [ib[0] + 0.012, yb, ib[2] - 0.01],
                [oa[0] - 0.012, ya, oa[2] + lift],
                [ia[0] + 0.012, ya, ia[2] + lift],
                [ob[0] - 0.012, yb, ob[2] + lift],
                [ib[0] + 0.012, yb, ib[2] + lift],
            ],
        );
    }
    mirror(b, &k);
}

/// Rubber mud flaps behind the rear wheels.
fn mud_flaps(b: &mut Builder, p: &BodyProfile) {
    let Some(&(wy, wz, r)) = p.arches.iter().find(|a| a.0 < 0.0) else {
        return;
    };
    let k = key("mud_flap_l", R::RearLeftQuarter, M::Trim);
    let y = wy - r + 0.02;
    let x0 = -(p.half_width(y) + 0.005);
    b.cuboid(&k, [x0, y - 0.012, wz - 0.20], [x0 + 0.24, y, wz + 0.12]);
    mirror(b, &k);
}

/// Sliding-door track on the RIGHT side only (black strip from the slider back 1.3 m).
fn sliding_door_track(b: &mut Builder, p: &BodyProfile) {
    let (rh, re) = p.rear_door.unwrap_or((p.y_fd_rear, p.y_fd_rear - 1.0));
    let z = p.z_belt + 0.16;
    let k = key("door_track_l", R::Cabin, M::Trim);
    side_box(
        b,
        &k,
        p,
        re - 1.2,
        re - 0.03,
        z - 0.018,
        z + 0.018,
        0.02,
        0.004,
    );
    b.mirror_part("door_track_l", &k.mirrored());
    b.set_geometry("door_track_l", Vec::new(), Vec::new(), Vec::new());
    // Slider handle (right side).
    let h = key("slider_handle_l", R::LeftDoorR, M::Trim);
    let yh = rh - 0.12;
    let zh = p.z_belt - 0.10;
    side_box(
        b,
        &h,
        p,
        yh - 0.08,
        yh + 0.08,
        zh - 0.015,
        zh + 0.015,
        0.016,
        0.004,
    );
    b.mirror_part("slider_handle_l", &h.mirrored());
    b.set_geometry("slider_handle_l", Vec::new(), Vec::new(), Vec::new());
}
