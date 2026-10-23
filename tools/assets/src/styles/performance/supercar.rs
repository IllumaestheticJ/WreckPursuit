//! Wave-3 supercars: `halberd_40` (2020s carbon-tub TT V8), `ombra_v12` (2010s V12 flagship),
//! `ostwind_6t` (2020s rear-engine flat-six) and `kagero_hx` (2010s hybrid V6).
//!
//! Like the wave-2 exotics, the mid-engine lattices carry a front-engine cabin, so the
//! cab-forward look comes from `cabin_shift_m`. The rear-engine Ostwind keeps a classic
//! long-roof fastback (small shift). Mechanical parts come from `mechanical::spec` (table).

use wp_contracts::VehicleDef;

use crate::mesh::MaterialSlot as M;
use crate::style::{ArchShape, BodyStyle, CapKind, CarStyle, RimKind, Silhouette};

use super::{rgb, wheels};

/// 2020s British carbon-tub supercar: teardrop glass canopy, dihedral doors, active wing.
pub fn halberd(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: 0.42,
        nose_h: Some(0.50),
        nose_rake_deg: 42.0,
        nose_plan_round: 0.24,
        tail_plan_round: 0.16,
        hood_crown: 0.018,
        windshield_rake_deg: Some(66.0),
        a_pillar_rake_deg: 20.0,
        rear_glass_rake_deg: Some(66.0),
        roof_len_trim: 0.12,
        tumblehome: Some(0.30),
        crease_z: 0.44,
        crease_depth: 0.020,
        haunch: 0.060,
        front_haunch: 0.035,
        flare: 0.018,
        ride_dz: -0.01,
        stance_pitch_deg: 0.2,
        ducktail_h: 0.020,
        belt_kick: 0.07,
        belt_rake: 0.03,
        tail_round: 0.12,
        tail_drop: 0.03,
        ..BodyStyle::preset(Silhouette::Wedge)
    });
    wheels(
        &mut s,
        def,
        RimKind::Monoblock,
        10,
        0.78,
        M::Trim,
        CapKind::CentreLock,
        (0.035, 0.012),
        (0.0, 0.5),
    );
    s.tints.push((M::Paint, rgb(0xFF6A13)));
    s
}

/// 2010s Italian V12 flagship: very low and wide, hexagonal motifs, scissor doors.
pub fn ombra(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: 0.45,
        nose_h: Some(0.48),
        nose_rake_deg: 50.0,
        nose_plan_round: 0.12,
        tail_plan_round: 0.10,
        hood_crown: 0.012,
        windshield_rake_deg: Some(67.0),
        a_pillar_rake_deg: 16.0,
        rear_glass_rake_deg: Some(68.0),
        roof_len_trim: 0.16,
        tumblehome: Some(0.30),
        crease_z: 0.46,
        crease_depth: 0.030,
        haunch: 0.065,
        front_haunch: 0.030,
        flare: 0.020,
        flare_shape: ArchShape::Box,
        ride_dz: -0.015,
        stance_pitch_deg: 0.3,
        ducktail_h: 0.010,
        belt_kick: 0.08,
        belt_rake: 0.035,
        tail_round: 0.06,
        tail_drop: 0.02,
        ..BodyStyle::preset(Silhouette::Wedge)
    });
    wheels(
        &mut s,
        def,
        RimKind::SplitSpoke,
        5,
        0.79,
        M::Trim,
        CapKind::CentreLock,
        (0.03, 0.012),
        (0.0, 0.5),
    );
    s.tints.push((M::Paint, rgb(0x2B2D42)));
    s
}

/// 2020s German rear-engine flagship: long sloping roof, round-ish lamps, wide rear haunches.
pub fn ostwind(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: 0.12,
        nose_h: Some(0.60),
        nose_rake_deg: 38.0,
        nose_plan_round: 0.30,
        tail_plan_round: 0.22,
        hood_crown: 0.030,
        windshield_rake_deg: Some(62.0),
        a_pillar_rake_deg: 18.0,
        rear_glass_rake_deg: Some(70.0),
        roof_len_trim: 0.0,
        tumblehome: Some(0.24),
        crease_z: 0.50,
        crease_depth: 0.010,
        haunch: 0.070,
        front_haunch: 0.025,
        flare: 0.012,
        ride_dz: -0.005,
        stance_pitch_deg: 0.1,
        ducktail_h: 0.025,
        belt_kick: 0.04,
        belt_rake: 0.02,
        tail_round: 0.14,
        tail_drop: 0.04,
        ..BodyStyle::preset(Silhouette::Fastback)
    });
    wheels(
        &mut s,
        def,
        RimKind::MultiSpoke,
        10,
        0.80,
        M::Metal,
        CapKind::CentreLock,
        (0.025, 0.012),
        (0.0, 0.5),
    );
    s.tints.push((M::Paint, rgb(0x8A9BA8)));
    s
}

/// 2010s Japanese hybrid supercar: crisp folded surfaces, floating buttresses, slim lamps.
pub fn kagero(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: 0.40,
        nose_h: Some(0.54),
        nose_rake_deg: 44.0,
        nose_plan_round: 0.16,
        tail_plan_round: 0.12,
        hood_crown: 0.016,
        windshield_rake_deg: Some(64.0),
        a_pillar_rake_deg: 18.0,
        rear_glass_rake_deg: Some(62.0),
        roof_len_trim: 0.14,
        tumblehome: Some(0.26),
        crease_z: 0.47,
        crease_depth: 0.028,
        haunch: 0.050,
        front_haunch: 0.030,
        flare: 0.016,
        ride_dz: -0.01,
        stance_pitch_deg: 0.2,
        ducktail_h: 0.020,
        belt_kick: 0.07,
        belt_rake: 0.03,
        tail_round: 0.08,
        tail_drop: 0.02,
        ..BodyStyle::preset(Silhouette::Wedge)
    });
    wheels(
        &mut s,
        def,
        RimKind::Turbine,
        15,
        0.76,
        M::Metal,
        CapKind::Centre,
        (0.025, 0.012),
        (0.0, 0.5),
    );
    s.tints.push((M::Paint, rgb(0xD9DDE2)));
    s
}
