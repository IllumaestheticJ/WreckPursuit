//! Civilian traffic styles (owner: wave-2 `style-traffic`). Spec:
//! `docs/design/vehicles/research_traffic.md` §3 (body knobs + detail lists per car).
//!
//! Every car is a [`BodyStyle`] built from a silhouette preset plus one custom detail function
//! that emits that car's trim kit through the small helpers below: grilles, bumpers with rub
//! strips, door handles, mirrors, plates, side markers, era-appropriate lamps (wipers: `trim`; the
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
//! Visual-only variants (taxi sign, ladder rack, roof box, liveries, paint) are the browser's
//! job (`apps/testbed-web/src/render/traffic.ts`); the meshes here are the base bodies.

use wp_contracts::{BodyRegion, VehicleDef};

use crate::builder::{Builder, PartKey, Patch, PatchBand};
use crate::mesh::{indicator_front_key, indicator_rear_key, MaterialSlot, PartKind};
use crate::profile::{smoothstep, BodyProfile};
use crate::style::{
    ArchShape, BodyStyle, CaliperKind, CapKind, CarStyle, Detail, DetailCtx, EngineKind,
    ExhaustKind, FuelDoor, RimKind, Shape, Silhouette, SuspensionKind, TrimFinish,
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
        "satchel_20" => Some(euro_panel_van(false)),
        "tumbril_20" => Some(euro_panel_van(true)),
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
        // Late-80s glasshouse: thin pillars, flat glass, small black sail.
        a_pillar_w_m: 0.06,
        c_pillar_w_m: 0.09,
        b_pillar_w_m: 0.10,
        windshield_wrap_m: 0.02,
        rear_glass_wrap_m: 0.02,
        a_sail_len_m: 0.10,
        end_round_m: 0.015,
        fuel_door: FuelDoor::Left,
        ..BodyStyle::preset(Silhouette::Notchback)
    };
    let mut s = with_kit(body, beater_kit);
    // Red-oxide primer on the replacement front-left fender.
    s.accent_rgb = Some([122, 58, 46]);
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
        // Short sloping bonnet, the windshield base pulled forward (one-box-and-a-bit
        // supermini), a softly arched tall roof.
        hood_length: Some(0.86),
        nose_h: Some(0.70),
        nose_rake_deg: 38.0,
        nose_plan_round: 0.44,
        tail_plan_round: 0.25,
        hood_crown: 0.025,
        windshield_rake_deg: Some(61.0),
        cowl_h: 0.06,
        a_pillar_rake_deg: 6.0,
        roof_arc: 0.30,
        roof_crown: 0.03,
        tumblehome: Some(0.13),
        a_pillar_w_m: 0.07,
        c_pillar_w_m: 0.16,
        windshield_wrap_m: 0.08,
        crease_z: 0.57,
        crease_depth: 0.008,
        haunch: 0.012,
        flare: 0.005,
        belt_kick: 0.05,
        belt_rake: 0.04,
        overhang_trim: (0.0, -0.02),
        end_round_m: 0.04,
        fuel_door: FuelDoor::Left,
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
        windshield_rake_deg: Some(56.0),
        // Upright cab back with a short roof run back to it (regular cab).
        rear_glass_rake_deg: Some(5.0),
        roof_len_trim: 0.10,
        roof_crown: 0.012,
        a_pillar_w_m: 0.065,
        c_pillar_w_m: 0.12,
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
    // Two-tone tape stripe along the sides.
    s.accent_rgb = Some([150, 28, 30]);
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
        // Crew cab: the cab moves back over the lattice's long bed (6-ft bed look) and the
        // windshield base forward to a 1.6 m hood.
        cabin_shift_m: -0.50,
        hood_length: Some(1.62),
        belt_dz: 0.08,
        nose_h: Some(1.10),
        nose_rake_deg: 3.0,
        nose_plan_round: 0.15,
        tail_plan_round: 0.08,
        hood_crown: 0.03,
        hood_bulge_h: 0.03,
        hood_bulge_w: 0.55,
        windshield_rake_deg: Some(57.0),
        // Near-upright cab back, roof run back to it (rear-door window ends at the C-pillar).
        rear_glass_rake_deg: Some(6.0),
        roof_len_trim: 0.15,
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
    // Keeps the crew cab (big loft, open bed + real wheels) under the 11k traffic budget.
    s.wheel.segments = 12;
    s.mechanical.engine = EngineKind::V8;
    s.mechanical.front_suspension = SuspensionKind::DoubleWishbone;
    s.mechanical.rear_suspension = SuspensionKind::SolidAxleLeaf;
    s.mechanical.exhaust = ExhaustKind::Single;
    s.mechanical.exhaust_tip_r_m = 0.045;
    // Red tow hooks use the caliper slot.
    s.mechanical.caliper_rgb = Some([170, 24, 24]);
    s
}

/// 2000s full-size RWD cargo van (research §3.6): long upright V8 nose, tall slab-sided box,
/// flush rear doors (see [`van_box`]).
fn drayman_cargo() -> CarStyle {
    let body = BodyStyle {
        // Short-ish engine nose under a tall windshield; the kit lifts the nose 10 cm.
        hood_length: Some(1.22),
        belt_dz: 0.06,
        nose_h: Some(1.12),
        nose_rake_deg: 12.0,
        nose_plan_round: 0.22,
        tail_plan_round: 0.06,
        hood_crown: 0.03,
        hood_bulge_h: 0.02,
        hood_bulge_w: 0.5,
        windshield_rake_deg: Some(50.0),
        a_pillar_w_m: 0.09,
        tumblehome: Some(0.10),
        glass_inset: 0.04,
        roof_crown: 0.02,
        crease_z: 0.50,
        crease_depth: 0.010,
        flare: 0.012,
        flare_shape: ArchShape::Box,
        stance_pitch_deg: 0.2,
        overhang_trim: (0.0, 0.03),
        mirrors: TrimFinish::Black,
        door_handles: TrimFinish::Black,
        drip_rail: TrimFinish::Black,
        ..BodyStyle::preset(Silhouette::Van)
    };
    let mut s = with_kit(body, drayman_kit);
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

/// 2000s European FWD high-roof panel van (`satchel_20`, `tumbril_20`): short sloping bonnet
/// that runs into a raked screen, tall slab-sided box, flush barn doors with windows, a
/// transverse I4, strut front and torsion-beam rear. `satchel` is the rounder one (swept lamps,
/// body-colour bumper tops); `tumbril` the squarer, longer one (steeper screen, upright square
/// lamps, full black plastic front, sliding-door track).
fn euro_panel_van(tumbril: bool) -> CarStyle {
    let body = BodyStyle {
        hood_length: Some(if tumbril { 1.0 } else { 0.92 }),
        cowl_h: if tumbril { 0.06 } else { 0.10 },
        nose_h: Some(if tumbril { 1.0 } else { 0.96 }),
        nose_rake_deg: if tumbril { 18.0 } else { 34.0 },
        nose_plan_round: if tumbril { 0.20 } else { 0.34 },
        tail_plan_round: 0.06,
        hood_crown: if tumbril { 0.02 } else { 0.035 },
        windshield_rake_deg: Some(if tumbril { 46.0 } else { 52.0 }),
        a_pillar_w_m: 0.085,
        windshield_wrap_m: if tumbril { 0.03 } else { 0.07 },
        tumblehome: Some(0.10),
        glass_inset: 0.04,
        roof_crown: if tumbril { 0.02 } else { 0.03 },
        crease_z: 0.55,
        crease_depth: 0.008,
        flare: 0.01,
        stance_pitch_deg: 0.1,
        overhang_trim: (0.0, if tumbril { 0.05 } else { 0.02 }),
        mirrors: TrimFinish::Black,
        door_handles: TrimFinish::Black,
        fuel_door: FuelDoor::Left,
        ..BodyStyle::preset(Silhouette::Van)
    };
    let mut s = with_kit(body, if tumbril { tumbril_kit } else { satchel_kit });
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
    // Replacement front-left fender still in primer (upper half, arch to door gap), and a
    // small primer patch low on the rear door.
    let pf = key("primer_fender", R::FrontLeftFender, M::Accent);
    let arch_top = p.arches[0].1 + p.arches[0].2;
    b.surface_patch(
        &pf,
        p,
        &Patch {
            y0: p.y_fd_hinge + 0.012,
            y1: p.y_nose - 0.38,
            band: PatchBand::Side {
                z0: arch_top + 0.03,
                z1: p.z_belt - 0.045,
            },
            offset_m: 0.0035,
            thickness_m: 0.002,
            steps: (8, 4),
        },
    );
    b.surface_patch(
        &pf,
        p,
        &Patch {
            y0: p.y_fd_hinge + 0.012,
            y1: p.arches[0].0 - p.arches[0].2 - 0.03,
            band: PatchBand::Side {
                z0: p.z_door_bottom + 0.02,
                z1: arch_top + 0.03,
            },
            offset_m: 0.0035,
            thickness_m: 0.002,
            steps: (3, 6),
        },
    );
    if let Some((rh, re)) = p.rear_door {
        let pd = key("primer_spot", R::LeftDoorR, M::Accent);
        b.surface_patch(
            &pd,
            p,
            &Patch {
                y0: re + 0.10,
                y1: 0.5 * (rh + re),
                band: PatchBand::Side {
                    z0: p.z_door_bottom + 0.05,
                    z1: p.z_door_bottom + 0.17,
                },
                offset_m: 0.0035,
                thickness_m: 0.002,
                steps: (2, 1),
            },
        );
    }
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

/// tansy_13 (2000s five-door supermini): big teardrop headlamps stepped down toward the
/// grille with an amber lower corner, slim slot grille with a chrome bar and blank badge, wide lower
/// intake with fog lamps, body-colour bumpers with a black lower lip / diffuser and rear
/// reflectors, tall tail lamps, roof spoiler with the third brake lamp, rear wiper, chrome
/// tailgate strip over the plate, black door rubbing strips, thin sill cladding.
fn supermini_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let zn = p.belt(p.y_nose);
    let wf = p.half_width(p.y_nose);
    let (h0, h1) = (zn - 0.13, zn - 0.015);
    reshape_pair(b, p, "headlight", |t, p, k| {
        lens_front(t, k, p, -(wf - 0.02), -0.33, h0, h1, 0.012, 0.035);
        // Inner end stepped down toward the grille (teardrop).
        lens_front(t, k, p, -0.33, -0.27, h0, h1 - 0.05, 0.012, 0.035);
    });
    // Amber lower outer corner of the lamp = the turn signal.
    reshape_indicators(b, p, true, M::EmissiveAmber, |t, p, k| {
        lens_front(
            t,
            k,
            p,
            -(wf - 0.02),
            -(wf - 0.13),
            h0 - 0.045,
            h0 - 0.005,
            0.012,
            0.035,
        );
    });
    // Upper slot grille with a chrome bar and a blank badge.
    let slot = key("grille_upper", R::FrontBumper, M::Grille);
    front_box(b, &slot, p, -0.26, 0.26, zn - 0.10, zn - 0.045, 0.006, 0.03);
    let bar = key("grille_bar", R::FrontBumper, M::Chrome);
    front_box(b, &bar, p, -0.26, 0.26, zn - 0.078, zn - 0.066, 0.012, 0.0);
    let badge = key("grille_badge", R::FrontBumper, M::Chrome);
    front_box(
        b,
        &badge,
        p,
        -0.055,
        0.055,
        zn - 0.105,
        zn - 0.04,
        0.016,
        0.0,
    );
    // Wide lower intake (trapezoid: centre box + raised ends) with fog lamps.
    let lower = key("grille_lower", R::FrontBumper, M::Grille);
    front_box(b, &lower, p, -0.34, 0.34, 0.26, 0.38, 0.006, 0.03);
    let fogb = key("fog_bezel_l", R::FrontBumper, M::Trim);
    front_box(b, &fogb, p, -0.62, -0.46, 0.27, 0.36, 0.006, 0.03);
    mirror(b, &fogb);
    let fog = light("foglamp_l", R::FrontBumper, M::Headlight);
    front_box(b, &fog, p, -0.60, -0.48, 0.29, 0.34, 0.012, 0.0);
    mirror(b, &fog);
    let lip = key("front_lip", R::FrontBumper, M::Trim);
    front_box(b, &lip, p, -(wf - 0.12), wf - 0.12, 0.19, 0.23, 0.02, 0.02);
    plate_front(b, p, 0.40);
    // Rear: tall lamps (red top and bottom, amber band between).
    let zt = p.belt(p.y_tail);
    let wt = p.half_width(p.y_tail);
    let (tx0, tx1) = (-(wt - 0.005), -(wt - 0.17));
    reshape_pair(b, p, "taillight", |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, zt - 0.22, zt - 0.095, 0.012, 0.03);
        lens_rear(t, k, p, tx0, tx1, zt - 0.025, zt + 0.06, 0.012, 0.03);
    });
    reshape_indicators(b, p, false, M::EmissiveAmber, |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, zt - 0.09, zt - 0.03, 0.012, 0.03);
    });
    plate_rear(b, p, zt - 0.29);
    let ts = key("tailgate_strip", R::Trunk, M::Chrome);
    rear_box(b, &ts, p, -0.30, 0.30, zt - 0.145, zt - 0.125, 0.008, 0.0);
    let dif = key("rear_diffuser", R::RearBumper, M::Trim);
    rear_box(b, &dif, p, -(wt - 0.14), wt - 0.14, 0.22, 0.30, 0.02, 0.02);
    let refl = light("rear_reflector_l", R::RearBumper, M::Taillight);
    rear_box(
        b,
        &refl,
        p,
        -(wt - 0.16),
        -(wt - 0.30),
        0.33,
        0.36,
        0.008,
        0.0,
    );
    mirror(b, &refl);
    // Roof spoiler: a wedge off the roof's rear edge, third brake lamp under its lip.
    let sp = key("hatch_spoiler", R::Roof, M::Paint);
    let (r0, _, _, _) = p.roof;
    let hw = p.roof_half_w(r0) - 0.05;
    let zr = p.top(r0 + 0.02);
    hexa(
        b,
        &sp,
        [
            [-hw, r0 - 0.12, zr - 0.035],
            [hw, r0 - 0.12, zr - 0.035],
            [-hw, r0 + 0.10, zr - 0.012],
            [hw, r0 + 0.10, zr - 0.012],
            [-hw, r0 - 0.15, zr - 0.005],
            [hw, r0 - 0.15, zr - 0.005],
            [-hw, r0 + 0.10, zr + 0.008],
            [hw, r0 + 0.10, zr + 0.008],
        ],
    );
    let bl = light("brake_lamp_c", R::Roof, M::Taillight);
    b.cuboid(
        &bl,
        [-0.20, r0 - 0.135, zr - 0.04],
        [0.20, r0 - 0.11, zr - 0.022],
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
    // Sides: black rubbing strips on the doors, thin sill cladding.
    let zr = p.z_door_bottom + 0.12;
    side_strip(
        b,
        p,
        "rub_strip",
        M::Trim,
        (p.y_quarter.0 - 0.05, p.y_fd_hinge - 0.02),
        (zr, zr + 0.045),
        0.010,
    );
    rocker_cladding(b, p, 0.06, M::Trim);
    // Amber side repeater on each front fender, behind the arch.
    let rp = light("side_repeater_l", R::FrontLeftFender, M::EmissiveAmber);
    let yr = p.arches[0].0 - p.arches[0].2 - 0.10;
    side_box(
        b,
        &rp,
        p,
        yr - 0.035,
        yr + 0.035,
        p.z_belt - 0.10,
        p.z_belt - 0.075,
        0.008,
        0.004,
    );
    mirror(b, &rp);
}

/// furrow_24 (90s compact regular-cab pickup): square lamps with wraparound amber corners in
/// one chrome bezel, chrome-framed bar grille with a centre post and blank badge, chrome bumper
/// with a black rubber strip over a slotted valance, two-tone tape stripe, tall black mirrors,
/// cab-back sliding window, vertical tail lamps with a reverse section, a recessed lettering
/// panel across the tailgate, chrome step bumper, bed rail caps, mud flaps.
fn compact_pickup_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let zn = p.belt(p.y_nose);
    let wf = p.half_width(p.y_nose);
    let (l0, l1) = (zn - 0.17, zn - 0.035);
    reshape_pair(b, p, "headlight", |t, p, k| {
        lens_front(t, k, p, -(wf - 0.10), -0.45, l0, l1, 0.008, 0.03);
    });
    // The amber corner lamp beside each square lamp is its turn signal.
    reshape_indicators(b, p, true, M::EmissiveAmber, |t, p, k| {
        lens_front(t, k, p, -(wf - 0.003), -(wf - 0.09), l0, l1, 0.008, 0.03);
    });
    let bez = key("lamp_bezel_l", R::FrontLeftFender, M::Chrome);
    front_box(
        b,
        &bez,
        p,
        -(wf - 0.001),
        -0.44,
        l0 - 0.015,
        l1 + 0.015,
        0.005,
        0.02,
    );
    mirror(b, &bez);
    grille(b, p, 0.43, l0 - 0.01, l1 + 0.01, 2, M::Chrome);
    let post = key("grille_post", R::FrontBumper, M::Chrome);
    front_box(b, &post, p, -0.012, 0.012, l0 - 0.01, l1 + 0.01, 0.014, 0.0);
    let badge = key("grille_badge", R::FrontBumper, M::Chrome);
    front_box(
        b,
        &badge,
        p,
        -0.07,
        0.07,
        0.5 * (l0 + l1) - 0.03,
        0.5 * (l0 + l1) + 0.03,
        0.02,
        0.0,
    );
    bumper(b, p, true, 0.42, 0.60, 0.09, M::Chrome, None);
    let rub = key("bumper_rub_f", R::FrontBumper, M::Trim);
    front_box(b, &rub, p, -(wf - 0.13), wf - 0.13, 0.49, 0.53, 0.096, 0.0);
    let val = key("valance", R::FrontBumper, M::Trim);
    front_box(b, &val, p, -(wf - 0.10), wf - 0.10, 0.30, 0.42, 0.02, 0.03);
    let slot = key("valance_slots", R::FrontBumper, M::Grille);
    for x in [-0.36f32, -0.12, 0.12] {
        front_box(b, &slot, p, x, x + 0.24 - 0.03, 0.34, 0.38, 0.024, 0.0);
    }
    plate_on_face(b, p, true, 0.445, 0.09);
    let zt = p.belt(p.y_tail);
    let wt = p.half_width(p.y_tail);
    // Vertical tail lamps: white reverse at the bottom, red lamp, red-on-red turn signal on top.
    let (tx0, tx1) = (-(wt - 0.005), -(wt - 0.11));
    reshape_pair(b, p, "taillight", |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, zt - 0.30, zt - 0.195, 0.012, 0.03);
    });
    let rev = light("reverse_lamp_l", R::RearLeftQuarter, M::Headlight);
    lens_rear(b, &rev, p, tx0, tx1, zt - 0.375, zt - 0.305, 0.012, 0.03);
    mirror(b, &rev);
    reshape_indicators(b, p, false, M::Taillight, |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, zt - 0.19, zt - 0.06, 0.012, 0.03);
    });
    tailgate_handle(b, p, zt - 0.09);
    // Recessed lettering panel across the tailgate (blank).
    let lp = key("tailgate_panel", R::Trunk, M::Trim);
    rear_box(b, &lp, p, -0.48, 0.48, zt - 0.32, zt - 0.20, 0.003, 0.0);
    step_bumper(b, p, 0.38, 0.54, 0.13, M::Chrome, false);
    plate_on_face(b, p, false, 0.40, 0.14);
    bed_rails(b, p, M::Trim);
    tall_mirrors(b, p, (0.10, 0.06, 0.18), 0.08);
    mud_flaps(b, p);
    handles(b, p, M::Trim, false);
    side_markers(b, p, p.z_bumper_top.0 + 0.05);
    cab_back_window(b, p, 0.42, (p.z_deck + 0.17, p.roof.2 - 0.13), true);
    // Two-tone tape stripe from the front fender to the tail, broken at the arches.
    let zs = p.z_door_bottom + 0.16;
    side_strip(
        b,
        p,
        "tape_stripe",
        M::Accent,
        (p.y_tail + 0.12, p.y_nose - 0.15),
        (zs, zs + 0.05),
        0.002,
    );
}

/// hauler_1500 (2010s full-size crew-cab pickup): tall flat nose (lifted 12 cm over the
/// lattice's hood line), tall three-section chrome grille with two heavy bars and a blank
/// badge, tall headlamps in chrome surrounds with an amber LED strip, chrome bumper with split
/// intakes, fog lamps and red tow hooks, crew-cab handles and rear-door cut lines, cab-back
/// sliding window, running boards, towing mirrors, cab clearance lamps, tailgate spoiler cap
/// and badge, tail lamps with a white reverse section at the bottom, chrome step bumper with
/// hitch.
fn fullsize_pickup_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    const LIFT: f32 = 0.12;
    lift_front(b, p, LIFT, 0.86, 1.16);
    let zn = p.belt(p.y_nose) + LIFT;
    let wf = p.half_width(p.y_nose);
    // Headlamps: tall rectangles outboard of the grille; the amber strip along the bottom of
    // the lens is the turn signal.
    let (h0, h1) = (zn - 0.25, zn - 0.045);
    reshape_pair(b, p, "headlight", |t, p, k| {
        lens_front(t, k, p, -(wf - 0.02), -0.62, h0 + 0.05, h1, 0.012, 0.03);
    });
    reshape_indicators(b, p, true, M::EmissiveAmber, |t, p, k| {
        lens_front(t, k, p, -(wf - 0.02), -0.62, h0, h0 + 0.045, 0.012, 0.03);
    });
    // Chrome bezel: one box round the lens, less proud than it.
    let sur = key("lamp_bezel_l", R::FrontLeftFender, M::Chrome);
    front_box(
        b,
        &sur,
        p,
        -(wf - 0.005),
        -0.60,
        h0 - 0.02,
        h1 + 0.02,
        0.008,
        0.02,
    );
    mirror(b, &sur);
    // Grille: black mesh, thick chrome surround, two heavy chrome bars, blank badge.
    let (g0, g1) = (0.80, zn - 0.035);
    let gm = key("grille", R::FrontBumper, M::Grille);
    front_box(b, &gm, p, -0.58, 0.58, g0, g1, 0.004, 0.03);
    let gf = key("grille_frame", R::FrontBumper, M::Chrome);
    frame_front(b, &gf, p, [-0.58, 0.58, g0, g1], 0.045, 0.03);
    for f in [0.36f32, 0.68] {
        let z = g0 + (g1 - g0) * f;
        front_box(b, &gf, p, -0.58, 0.58, z - 0.028, z + 0.028, 0.032, 0.0);
    }
    let zb = g0 + (g1 - g0) * 0.68;
    let badge = key("grille_badge", R::FrontBumper, M::Chrome);
    front_box(b, &badge, p, -0.10, 0.10, zb - 0.05, zb + 0.05, 0.045, 0.0);
    // Chrome bumper with two black intakes, a centre plate, fog lamps and red tow hooks.
    bumper(b, p, true, 0.50, 0.79, 0.11, M::Chrome, None);
    let it = key("bumper_intake_l", R::FrontBumper, M::Grille);
    front_box(b, &it, p, -0.50, -0.20, 0.56, 0.68, 0.118, 0.0);
    mirror(b, &it);
    let fog = light("foglamp_l", R::FrontBumper, M::Headlight);
    front_box(b, &fog, p, -0.76, -0.62, 0.58, 0.66, 0.118, 0.0);
    mirror(b, &fog);
    let dam = key("air_dam", R::FrontBumper, M::Trim);
    front_box(b, &dam, p, -(wf - 0.14), wf - 0.14, 0.42, 0.50, 0.07, 0.02);
    let hook = key("tow_hook_l", R::FrontBumper, M::Caliper);
    front_box(b, &hook, p, -0.58, -0.50, 0.44, 0.50, 0.13, 0.0);
    mirror(b, &hook);
    plate_on_face(b, p, true, 0.565, 0.12);
    // Rear: tall tail lamps (red signal on top, red lamp, white reverse at the bottom).
    let zt = p.belt(p.y_tail);
    let wt = p.half_width(p.y_tail);
    let (tx0, tx1) = (-(wt - 0.005), -(wt - 0.13));
    reshape_pair(b, p, "taillight", |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, zt - 0.395, zt - 0.175, 0.012, 0.03);
    });
    reshape_indicators(b, p, false, M::Taillight, |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, zt - 0.17, zt - 0.04, 0.012, 0.03);
    });
    let rev = light("reverse_lamp_l", R::RearLeftQuarter, M::Headlight);
    lens_rear(b, &rev, p, tx0, tx1, zt - 0.50, zt - 0.40, 0.012, 0.03);
    mirror(b, &rev);
    let cap = key("tailgate_cap", R::Trunk, M::Trim);
    rear_box(
        b,
        &cap,
        p,
        -(wt - 0.16),
        wt - 0.16,
        zt - 0.035,
        zt + 0.012,
        0.02,
        0.0,
    );
    tailgate_handle(b, p, zt - 0.11);
    let tb = key("tailgate_badge", R::Trunk, M::Chrome);
    rear_box(b, &tb, p, -0.16, 0.16, zt - 0.30, zt - 0.24, 0.012, 0.0);
    step_bumper(b, p, 0.42, 0.62, 0.14, M::Chrome, true);
    plate_on_face(b, p, false, 0.465, 0.15);
    bed_rails(b, p, M::Trim);
    // Running boards between the arches, on three brackets.
    let rb = key("running_board_l", R::FrameLeft, M::Trim);
    let (ya, yb) = (p.arches[2].0 + 0.50, p.arches[0].0 - 0.50);
    let zs = p.z_floor + 0.20;
    let xs = p.half_w + 0.02;
    b.cuboid(&rb, [-(xs + 0.12), ya, zs], [-(xs - 0.10), yb, zs + 0.05]);
    for y in [ya + 0.15, yb - 0.15] {
        b.cuboid(
            &rb,
            [-(xs - 0.10), y - 0.03, zs - 0.06],
            [-(xs - 0.25), y + 0.03, zs + 0.03],
        );
    }
    mirror(b, &rb);
    tall_mirrors(b, p, (0.13, 0.10, 0.27), 0.15);
    // Cab clearance lamps on the roof's front edge.
    let cl = light("clearance_lamps", R::Roof, M::EmissiveAmber);
    let yc = p.roof.1 - 0.05;
    let zc = p.top_point(yc, 0.0).0[2];
    for x in [-0.30f32, 0.0, 0.30] {
        b.cuboid(
            &cl,
            [x - 0.035, yc - 0.03, zc - 0.01],
            [x + 0.035, yc + 0.01, zc + 0.022],
        );
    }
    // Crew cab: chrome handles on both doors, the rear door's cut lines, cab-back window.
    let y_rd = p.roof.0 + 0.04;
    door_handles_at(
        b,
        p,
        M::Chrome,
        &[
            (R::LeftDoorF, p.y_fd_rear + 0.17),
            (R::LeftDoorR, y_rd + 0.17),
        ],
    );
    door_cut(b, p, R::LeftDoorR, y_rd, p.y_fd_rear);
    cab_back_window(b, p, 0.56, (p.z_deck + 0.20, p.roof.2 - 0.12), true);
    let fb = key("fender_badge_l", R::FrontLeftFender, M::Chrome);
    let yf = p.arches[0].0 - 0.62;
    side_box(
        b,
        &fb,
        p,
        yf - 0.12,
        yf + 0.12,
        p.z_belt - 0.20,
        p.z_belt - 0.16,
        0.008,
        0.003,
    );
    mirror(b, &fb);
    side_markers(b, p, p.z_bumper_top.0 + 0.06);
}

/// drayman_cargo (2000s US full-size cargo van): stacked square lamps with amber outer
/// sections in a black surround, wide three-bar grille with a chrome top bar, black bumpers with
/// a step pad, rub strip, flush barn doors (seam, hinges, handle, no glass), tall tail lamps in
/// the rear corners, high-mount brake lamp, big two-glass mirrors.
fn drayman_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    const LIFT: f32 = 0.10;
    lift_front(b, p, LIFT, 0.95, 1.20);
    let zn = p.belt(p.y_nose) + LIFT;
    let wf = p.half_width(p.y_nose);
    let (l0, l1) = (zn - 0.21, zn - 0.03);
    // Stacked lamps: the head lamp on top, the amber turn signal below it, same bezel.
    reshape_pair(b, p, "headlight", |t, p, k| {
        lens_front(t, k, p, -(wf - 0.04), -0.55, l0 + 0.075, l1, 0.012, 0.03);
    });
    reshape_indicators(b, p, true, M::EmissiveAmber, |t, p, k| {
        lens_front(t, k, p, -(wf - 0.04), -0.55, l0, l0 + 0.07, 0.012, 0.03);
    });
    let bez = key("lamp_surround_l", R::FrontLeftFender, M::Trim);
    front_box(
        b,
        &bez,
        p,
        -(wf - 0.01),
        -0.53,
        l0 - 0.02,
        l1 + 0.02,
        0.006,
        0.02,
    );
    mirror(b, &bez);
    grille(b, p, 0.50, l0, l1 - 0.02, 2, M::Trim);
    let top = key("grille_top_bar", R::FrontBumper, M::Chrome);
    front_box(b, &top, p, -0.52, 0.52, l1 - 0.035, l1 - 0.005, 0.02, 0.0);
    let badge = key("grille_badge", R::FrontBumper, M::Chrome);
    front_box(
        b,
        &badge,
        p,
        -0.08,
        0.08,
        0.5 * (l0 + l1) - 0.035,
        0.5 * (l0 + l1) + 0.035,
        0.022,
        0.0,
    );
    bumper(b, p, true, 0.40, 0.66, 0.08, M::Trim, None);
    plate_on_face(b, p, true, 0.47, 0.08);
    let zt = p.z_deck;
    let wt = p.half_width(p.y_tail);
    van_box(b, p, false);
    // Tall tail lamps in the rear corners, amber signal at the bottom.
    let (tx0, tx1) = (-(wt - 0.005), -(wt - 0.10));
    reshape_pair(b, p, "taillight", |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, zt - 0.17, zt + 0.22, 0.012, 0.03);
    });
    reshape_indicators(b, p, false, M::EmissiveAmber, |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, zt - 0.29, zt - 0.18, 0.012, 0.03);
    });
    step_bumper(b, p, 0.40, 0.56, 0.22, M::Trim, false);
    plate_rear(b, p, 0.60);
    side_strip(
        b,
        p,
        "rub_strip",
        M::Trim,
        (p.y_tail + 0.25, p.y_fd_hinge + 0.4),
        (0.78, 0.86),
        0.016,
    );
    tall_mirrors(b, p, (0.12, 0.08, 0.30), 0.16);
    handles(b, p, M::Trim, false);
    side_markers(b, p, p.z_bumper_top.0 + 0.08);
    square_up_sides(b, p, 0.06);
}

/// satchel_20 (rounder 2000s European panel van): big swept lamps with the amber signal at
/// their inner end, slim two-slat grille with a blank badge between them, body-colour bumper
/// top over a black lower bumper and intake, side rubbing strip, flush barn doors with
/// windows, tall tail lamps, high brake lamp.
fn satchel_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let zn = p.belt(p.y_nose);
    let wf = p.half_width(p.y_nose);
    let (l0, l1) = (zn - 0.17, zn - 0.02);
    reshape_pair(b, p, "headlight", |t, p, k| {
        lens_front(t, k, p, -(wf - 0.015), -0.50, l0 + 0.03, l1, 0.012, 0.035);
        lens_front(t, k, p, -0.50, -0.43, l0 + 0.06, l1, 0.012, 0.035);
    });
    reshape_indicators(b, p, true, M::EmissiveAmber, |t, p, k| {
        lens_front(
            t,
            k,
            p,
            -(wf - 0.015),
            -0.62,
            l0 - 0.01,
            l0 + 0.025,
            0.012,
            0.035,
        );
    });
    let gr = key("grille_upper", R::FrontBumper, M::Grille);
    front_box(b, &gr, p, -0.40, 0.40, l0 + 0.04, l1 - 0.02, 0.006, 0.03);
    let sl = key("grille_slats", R::FrontBumper, M::Trim);
    for z in [l0 + 0.075, l0 + 0.11] {
        front_box(b, &sl, p, -0.40, 0.40, z - 0.008, z + 0.008, 0.012, 0.0);
    }
    let badge = key("grille_badge", R::FrontBumper, M::Chrome);
    front_box(b, &badge, p, -0.07, 0.07, l0 + 0.05, l1 - 0.03, 0.018, 0.0);
    bumper(b, p, true, 0.36, 0.56, 0.06, M::Trim, None);
    let intake = key("grille_lower", R::FrontBumper, M::Grille);
    front_box(b, &intake, p, -0.45, 0.45, 0.42, 0.52, 0.065, 0.0);
    plate_on_face(b, p, true, 0.575, 0.0);
    euro_van_common(b, p, false);
}

/// tumbril_20 (squarer 2000s European panel van): upright square lamps with amber outer
/// columns, a full black plastic front panel between them with a wide slatted grille and blank
/// badge, deep black bumper with fog lamps, sliding-door track, flush barn doors with windows,
/// tall tail lamps, high brake lamp.
fn tumbril_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let zn = p.belt(p.y_nose);
    let wf = p.half_width(p.y_nose);
    let (l0, l1) = (zn - 0.22, zn - 0.025);
    reshape_pair(b, p, "headlight", |t, p, k| {
        lens_front(t, k, p, -(wf - 0.10), -0.52, l0, l1, 0.012, 0.03);
    });
    reshape_indicators(b, p, true, M::EmissiveAmber, |t, p, k| {
        lens_front(t, k, p, -(wf - 0.01), -(wf - 0.095), l0, l1, 0.012, 0.03);
    });
    let panel = key("front_panel", R::FrontBumper, M::Trim);
    front_box(
        b,
        &panel,
        p,
        -0.51,
        0.51,
        l0 - 0.03,
        l1 + 0.005,
        0.004,
        0.03,
    );
    let gr = key("grille", R::FrontBumper, M::Grille);
    front_box(b, &gr, p, -0.44, 0.44, l0 + 0.02, l1 - 0.04, 0.008, 0.0);
    let sl = key("grille_slats", R::FrontBumper, M::Trim);
    for i in 1..=3 {
        let z = l0 + 0.02 + (l1 - l0 - 0.06) * i as f32 / 4.0;
        front_box(b, &sl, p, -0.44, 0.44, z - 0.009, z + 0.009, 0.014, 0.0);
    }
    let badge = key("grille_badge", R::FrontBumper, M::Chrome);
    front_box(
        b,
        &badge,
        p,
        -0.08,
        0.08,
        0.5 * (l0 + l1) - 0.03,
        0.5 * (l0 + l1) + 0.04,
        0.02,
        0.0,
    );
    bumper(b, p, true, 0.36, 0.64, 0.08, M::Trim, None);
    let fog = light("foglamp_l", R::FrontBumper, M::Headlight);
    front_box(
        b,
        &fog,
        p,
        -(wf - 0.14),
        -(wf - 0.27),
        0.42,
        0.48,
        0.085,
        0.0,
    );
    mirror(b, &fog);
    let intake = key("grille_lower", R::FrontBumper, M::Grille);
    front_box(b, &intake, p, -0.40, 0.40, 0.40, 0.48, 0.085, 0.0);
    plate_on_face(b, p, true, 0.495, 0.08);
    sliding_door_track_at(b, p, p.z_belt + 0.02);
    euro_van_common(b, p, true);
}

/// Shared rear and sides of the two European panel vans.
fn euro_van_common(b: &mut Builder, p: &BodyProfile, tumbril: bool) {
    let zt = p.z_deck;
    let wt = p.half_width(p.y_tail);
    van_box(b, p, true);
    // Tall tail lamps in the rear corners: red, amber signal, red again (stacked).
    let (tx0, tx1) = (-(wt - 0.005), -(wt - if tumbril { 0.11 } else { 0.13 }));
    reshape_pair(b, p, "taillight", |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, zt - 0.30, zt - 0.17, 0.012, 0.03);
        lens_rear(t, k, p, tx0, tx1, zt + 0.02, zt + 0.14, 0.012, 0.03);
    });
    reshape_indicators(b, p, false, M::EmissiveAmber, |t, p, k| {
        lens_rear(t, k, p, tx0, tx1, zt - 0.16, zt + 0.01, 0.012, 0.03);
    });
    step_bumper(b, p, 0.36, 0.56, 0.12, M::Trim, false);
    plate_on_face(b, p, false, 0.42, 0.12);
    let zr = if tumbril { 0.70 } else { 0.76 };
    side_strip(
        b,
        p,
        "rub_strip",
        M::Trim,
        (p.y_tail + 0.20, p.y_nose - 0.25),
        (zr, zr + 0.07),
        0.014,
    );
    tall_mirrors(b, p, (0.11, 0.08, 0.27), 0.12);
    handles(b, p, M::Trim, false);
    // Amber side repeater on each front fender.
    let rp = light("side_repeater_l", R::FrontLeftFender, M::EmissiveAmber);
    let yr = p.arches[0].0 - p.arches[0].2 - 0.08;
    side_box(
        b,
        &rp,
        p,
        yr - 0.035,
        yr + 0.035,
        zr + 0.12,
        zr + 0.15,
        0.008,
        0.004,
    );
    mirror(b, &rp);
    square_up_sides(b, p, 0.065);
}

// =======================================================================================
// Surface helpers
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
    crate::trim::remove_generic(b, &crate::trim::GENERIC_HANDLES);
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
    crate::trim::remove_generic(b, &crate::trim::GENERIC_DRIP_RAIL);
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
    crate::trim::remove_generic(b, &crate::trim::GENERIC_MIRRORS);
}

/// Small black door mirrors (`size` = out × along × up) on a stub `arm` long.
fn black_mirrors(b: &mut Builder, p: &BodyProfile, size: (f32, f32, f32), arm: f32) {
    hide_generic_mirrors(b);
    // On the sail at the A-pillar foot.
    let y = 0.5 * (p.sail().0 + p.sail().1);
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
    // On the sail at the A-pillar foot.
    let y = 0.5 * (p.sail().0 + p.sail().1);
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

// =======================================================================================
// Car-shapes phase 2 helpers (traffic cars only; the older helpers above are shared with
// debug_sedan and stay unchanged)
// =======================================================================================

/// Lift the front of the body (every non-wheel part already built) by up to `h`: zero at the
/// windshield base, full ahead of the hood's front edge, faded in between heights `z0` (no
/// lift: bumper, arches) and `z1` (full lift). Raises a hood that the lattice keeps low (tall
/// truck / van noses). Call it before placing the kit's own parts.
fn lift_front(b: &mut Builder, p: &BodyProfile, h: f32, z0: f32, z1: f32) {
    let (yf, yb) = (p.y_fw, p.y_bf);
    b.transform(
        |k| k.wheel.is_none(),
        |q| {
            let t = smoothstep((q[1] - yf) / (yb - yf).max(0.1));
            let s = smoothstep((q[2] - z0) / (z1 - z0).max(0.01));
            [q[0], q[1], q[2] + h * t * s]
        },
    );
}

/// Frame of bars `t` wide round `[x0, x1, z0, z1]` on the front face, `out` proud.
fn frame_front(b: &mut Builder, k: &PartKey, p: &BodyProfile, r: [f32; 4], t: f32, out: f32) {
    let [x0, x1, z0, z1] = r;
    front_box(b, k, p, x0 - t, x1 + t, z1, z1 + t, out, 0.01);
    front_box(b, k, p, x0 - t, x1 + t, z0 - t, z0, out, 0.01);
    front_box(b, k, p, x0 - t, x0, z0, z1, out, 0.01);
    front_box(b, k, p, x1, x1 + t, z0, z1, out, 0.01);
}

/// Plate (dark frame + light plate) centred on the front or rear face at height `z` (bottom),
/// standing `out` proud of the body surface (on a bumper bar that far out).
fn plate_on_face(b: &mut Builder, p: &BodyProfile, front: bool, z: f32, out: f32) {
    let (fr, pl, region) = if front {
        ("plate_frame_f", "plate_f", R::FrontBumper)
    } else {
        ("plate_frame_r", "plate_r", R::RearBumper)
    };
    let f = key(fr, region, M::Trim);
    let q = key(pl, region, M::Metal);
    if front {
        front_box(
            b,
            &f,
            p,
            -0.27,
            0.27,
            z - 0.012,
            z + 0.132,
            out + 0.008,
            0.0,
        );
        front_box(b, &q, p, -0.255, 0.255, z, z + 0.12, out + 0.014, 0.0);
    } else {
        rear_box(
            b,
            &f,
            p,
            -0.27,
            0.27,
            z - 0.012,
            z + 0.132,
            out + 0.008,
            0.0,
        );
        rear_box(b, &q, p, -0.255, 0.255, z, z + 0.12, out + 0.014, 0.0);
    }
}

/// Door handles (LEFT, mirrored) at the given (region, y) stations, just below the belt;
/// replaces the generic handles.
fn door_handles_at(b: &mut Builder, p: &BodyProfile, mat: MaterialSlot, at: &[(BodyRegion, f32)]) {
    crate::trim::remove_generic(b, &crate::trim::GENERIC_HANDLES);
    let z = p.z_belt - 0.10;
    for (i, &(r, y)) in at.iter().enumerate() {
        let rec = key(&format!("door_handle_recess_{i}_l"), r, M::Trim);
        side_box(
            b,
            &rec,
            p,
            y - 0.085,
            y + 0.085,
            z - 0.024,
            z + 0.026,
            0.004,
            0.004,
        );
        mirror(b, &rec);
        let k = key(&format!("door_handle_{i}_l"), r, mat);
        side_box(
            b,
            &k,
            p,
            y - 0.07,
            y + 0.07,
            z - 0.012,
            z + 0.014,
            0.014,
            0.0,
        );
        mirror(b, &k);
    }
}

/// Seam lines of a door panel that the generic seams do not draw (a crew cab's rear door on a
/// lattice without a hinged rear door): rear edge at `y0`, bottom from `y0` to `y1`.
fn door_cut(b: &mut Builder, p: &BodyProfile, r: BodyRegion, y0: f32, y1: f32) {
    let k = key("door_cut_l", r, M::Trim);
    let w = 0.005;
    let zb = p.z_door_bottom + 0.03;
    side_box(b, &k, p, y0 - w, y0 + w, zb, p.z_belt - 0.02, 0.0025, 0.002);
    side_box(b, &k, p, y0, y1, zb - w, zb + w, 0.0025, 0.002);
    mirror(b, &k);
}

/// Window in a pickup's cab-back wall: glass `half` wide over `zs`, in a black seal; `slider`
/// adds the two divider bars of a sliding centre pane.
fn cab_back_window(b: &mut Builder, p: &BodyProfile, half: f32, zs: (f32, f32), slider: bool) {
    // The wall runs from the deck start (belt height) up to the roof's rear edge.
    let (y0, z0) = (p.y_deck, p.z_deck);
    let (y1, z1) = (p.roof.0, p.roof.2);
    let face = |z: f32| y0 + (y1 - y0) * ((z - z0) / (z1 - z0).max(0.1)).clamp(0.0, 1.0);
    let slab = |b: &mut Builder, k: &PartKey, x0: f32, x1: f32, za: f32, zb: f32, out: f32| {
        let mut c = [[0.0f32; 3]; 8];
        for (i, q) in c.iter_mut().enumerate() {
            let x = if i & 1 == 0 { x0 } else { x1 };
            let z = if i & 4 == 0 { za } else { zb };
            let y = face(z) + if i & 2 == 0 { -out } else { 0.01 };
            *q = [x, y, z];
        }
        hexa(b, k, c);
    };
    let seal = key("cab_back_glass_seal", R::Cabin, M::Trim);
    slab(
        b,
        &seal,
        -half - 0.025,
        half + 0.025,
        zs.0 - 0.025,
        zs.1 + 0.025,
        0.006,
    );
    let g = PartKey::body("cab_back_glass", R::Cabin, PartKind::Glass, M::Glass);
    slab(b, &g, -half, half, zs.0, zs.1, 0.010);
    if slider {
        for x in [-0.22f32, 0.22] {
            slab(b, &seal, x - 0.012, x + 0.012, zs.0, zs.1, 0.014);
        }
    }
}

/// Square up a van box: push the greenhouse sides out toward the body side above the belt
/// (the roof edge moves out by `out` m; the glass base at the belt stays), so the cargo box
/// reads slab-sided instead of tucked in. Moves every non-wheel part built so far (call it
/// last, so the kit's own parts move with the surface).
fn square_up_sides(b: &mut Builder, p: &BodyProfile, out: f32) {
    let (zb, k) = (p.z_belt + 0.02, out / p.roof_half.max(0.3));
    b.transform(
        |key| key.wheel.is_none(),
        |q| {
            let s = smoothstep((q[2] - zb) / 0.30);
            [q[0] * (1.0 + k * s), q[1], q[2]]
        },
    );
}

/// Van rear: the loft keeps the upper rear at least 25 cm ahead of the bumper face (deck
/// start ≥ tail + 0.25 m), which reads as a stepped estate tail. The greenhouse section at the
/// roof's rear edge is extruded straight back to the tail instead (rear corner pillars on the
/// quarters, the roof extension on the roof, a flat rear face on the rear doors), so the box
/// is flush from the belt to the roof. On its face: the barn doors' centre seam, hinges,
/// handle, high brake lamp and (with `windows`) two door windows in black seals.
fn van_box(b: &mut Builder, p: &BodyProfile, windows: bool) {
    let y_a = p.roof.0 + 0.01;
    let y_t = p.y_tail + 0.004;
    let ring = p.ring(y_a);
    // Left half from the glass base (8) to the centre top (17).
    let sec: Vec<[f32; 3]> = ring[8..].iter().map(|r| r.p).collect();
    let side = key("rear_corner_l", R::RearLeftQuarter, M::Paint);
    let roof = key("roof_extension", R::Roof, M::Paint);
    let side = PartKey {
        kind: PartKind::Panel,
        ..side
    };
    let roof = PartKey {
        kind: PartKind::Panel,
        ..roof
    };
    for j in 0..sec.len() - 1 {
        let (q0, q1) = (sec[j], sec[j + 1]);
        let (dx, dz) = (q1[0] - q0[0], q1[2] - q0[2]);
        let n = [-dz, 0.0, dx];
        let k = if j + 8 < crate::profile::RING_PILLAR_IN {
            &side
        } else {
            &roof
        };
        for sgn in [1.0f32, -1.0] {
            let m = |q: [f32; 3], y: f32| [q[0] * sgn, y, q[2]];
            let quad = [m(q0, y_a), m(q1, y_a), m(q1, y_t), m(q0, y_t)];
            b.poly_facing(k, &quad, [n[0] * sgn, 0.0, n[2]]);
        }
    }
    // Mirror the side strip into its own right-hand part.
    let right = side.mirrored();
    let (pos, _, idx) = b
        .geometry(&side.name)
        .map(|(a, u, i)| (a.to_vec(), u.to_vec(), i.to_vec()))
        .unwrap_or_default();
    let mut lp = Vec::new();
    let mut rp = Vec::new();
    let mut li = Vec::new();
    let mut ri = Vec::new();
    for t in idx.chunks_exact(3) {
        let c = (pos[t[0] as usize][0] + pos[t[1] as usize][0] + pos[t[2] as usize][0]) / 3.0;
        let (vp, vi) = if c < 0.0 {
            (&mut lp, &mut li)
        } else {
            (&mut rp, &mut ri)
        };
        for &v in t {
            vi.push(vp.len() as u32);
            vp.push(pos[v as usize]);
        }
    }
    let luv = vec![[0.5f32, 0.5]; lp.len()];
    let ruv = vec![[0.5f32, 0.5]; rp.len()];
    b.set_geometry(&side.name, lp, luv, li);
    b.part(&right);
    b.set_geometry(&right.name, rp, ruv, ri);
    // Flat rear face (the barn doors' upper half).
    let skin = PartKey::body("rear_door_skin", R::Trunk, PartKind::Panel, M::Paint);
    let mut face: Vec<[f32; 3]> = sec.iter().map(|q| [q[0], y_t, q[2]]).collect();
    face.extend(sec.iter().rev().skip(1).map(|q| [-q[0], y_t, q[2]]));
    let zc = face.iter().map(|q| q[2]).sum::<f32>() / face.len() as f32;
    for i in 0..face.len() {
        let j = (i + 1) % face.len();
        b.poly_facing(&skin, &[[0.0, y_t, zc], face[i], face[j]], [0.0, -1.0, 0.0]);
    }
    // Close the belt ledge behind the doors: the face carries on down to just below the deck.
    let (xb, zb) = (sec[0][0], sec[0][2]);
    let zl = p.z_deck - p.style.tail_drop.clamp(0.0, 0.25) - 0.05;
    b.poly_facing(
        &skin,
        &[[xb, y_t, zl], [-xb, y_t, zl], [-xb, y_t, zb], [xb, y_t, zb]],
        [0.0, -1.0, 0.0],
    );
    let z0 = sec[0][2];
    let z1 = sec[sec.len() - 1][2] - 0.03;
    let half_at = |z: f32| {
        // Half width of the section at height z (outermost point at or below it).
        sec.iter()
            .filter(|q| q[2] <= z + 1e-3)
            .map(|q| -q[0])
            .fold(sec[0][0].abs(), f32::max)
    };
    let slab = |b: &mut Builder, k: &PartKey, x0: f32, x1: f32, za: f32, zb: f32, out: f32| {
        let mut c = [[0.0f32; 3]; 8];
        for (i, q) in c.iter_mut().enumerate() {
            let x = if i & 1 == 0 { x0 } else { x1 };
            let z = if i & 4 == 0 { za } else { zb };
            let y = y_t + if i & 2 == 0 { -out } else { 0.01 };
            *q = [x, y, z];
        }
        hexa(b, k, c);
    };
    let seam = key("barn_seam", R::Trunk, M::Trim);
    slab(b, &seam, -0.005, 0.005, z0, z1 - 0.06, 0.003);
    rear_box(b, &seam, p, -0.005, 0.005, 0.70, z0, 0.003, 0.01);
    let hinge = key("barn_hinge_l", R::Trunk, M::Trim);
    for z in [z0 - 0.25, z1 - 0.30] {
        let x = -(half_at(z).min(p.half_width(p.y_tail)) - 0.02);
        if z < z0 {
            rear_box(b, &hinge, p, x, x + 0.065, z - 0.05, z + 0.05, 0.018, 0.0);
        } else {
            slab(b, &hinge, x, x + 0.065, z - 0.05, z + 0.05, 0.018);
        }
    }
    mirror(b, &hinge);
    let hd = key("barn_handle", R::Trunk, M::Chrome);
    slab(b, &hd, 0.03, 0.17, z0 + 0.03, z0 + 0.07, 0.022);
    let bl = light("brake_lamp_c", R::Trunk, M::Taillight);
    slab(b, &bl, -0.18, 0.18, z1 - 0.07, z1 - 0.03, 0.012);
    if windows {
        let (wz0, wz1) = (z0 + 0.18, z1 - 0.20);
        let seal = key("rear_door_glass_seal", R::Trunk, M::Trim);
        let g = PartKey::body("rear_door_glass", R::Trunk, PartKind::Glass, M::Glass);
        let xo = half_at(wz0) - 0.12;
        slab(b, &seal, -xo - 0.02, -0.05, wz0 - 0.02, wz1 + 0.02, 0.004);
        slab(b, &seal, 0.05, xo + 0.02, wz0 - 0.02, wz1 + 0.02, 0.004);
        slab(b, &g, -xo, -0.07, wz0, wz1, 0.007);
        slab(b, &g, 0.07, xo, wz0, wz1, 0.007);
    }
}

/// Sliding-door track on the RIGHT side at height `z` (black strip from behind the slider back
/// toward the tail) and the slider's handle.
fn sliding_door_track_at(b: &mut Builder, p: &BodyProfile, z: f32) {
    let (rh, re) = p.rear_door.unwrap_or((p.y_fd_rear, p.y_fd_rear - 1.0));
    let k = key("door_track_l", R::RearLeftQuarter, M::Trim);
    side_box(
        b,
        &k,
        p,
        p.y_tail + 0.35,
        re - 0.03,
        z - 0.016,
        z + 0.016,
        0.018,
        0.004,
    );
    b.mirror_part("door_track_l", &k.mirrored());
    b.set_geometry("door_track_l", Vec::new(), Vec::new(), Vec::new());
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
