//! Wave-3 race cars: `morrigan_55` (front-engine GT endurance racer), `nyx_proto`
//! (closed-cockpit sports prototype) and `merlo_20t` (FWD touring-car racer).
//!
//! Race cars share a vocabulary: low ride height, big splitters and wings, flat floors with
//! diffusers, centre-lock wheels, tow hooks, roof fins / intakes, number panels. Mechanical
//! parts come from `mechanical::spec` (table).

use wp_contracts::VehicleDef;

use crate::mesh::MaterialSlot as M;
use crate::style::{ArchShape, BodyStyle, CapKind, CarStyle, RimKind, Silhouette};

use super::{rgb, wheels};

/// Front-engine GT endurance racer: long hood, vented fenders, swan-neck wing.
pub fn morrigan(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        nose_h: Some(0.52),
        nose_rake_deg: 40.0,
        nose_plan_round: 0.18,
        tail_plan_round: 0.10,
        hood_crown: 0.025,
        windshield_rake_deg: Some(62.0),
        a_pillar_rake_deg: 16.0,
        rear_glass_rake_deg: Some(68.0),
        tumblehome: Some(0.26),
        crease_z: 0.46,
        crease_depth: 0.020,
        haunch: 0.075,
        front_haunch: 0.065,
        flare: 0.040,
        flare_shape: ArchShape::Box,
        ride_dz: -0.03,
        stance_pitch_deg: 0.4,
        ducktail_h: 0.015,
        belt_kick: 0.05,
        belt_rake: 0.02,
        tail_round: 0.05,
        tail_drop: 0.01,
        ..BodyStyle::preset(Silhouette::Fastback)
    });
    wheels(
        &mut s,
        def,
        RimKind::Monoblock,
        10,
        0.74,
        M::Trim,
        CapKind::CentreLock,
        (0.03, 0.010),
        (-1.5, -1.0),
    );
    s.tints.push((M::Paint, rgb(0xFFD100)));
    s
}

/// Closed-cockpit sports prototype: bubble canopy, long tail, shark fin.
pub fn nyx(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: 0.30,
        nose_h: Some(0.40),
        nose_rake_deg: 30.0,
        nose_plan_round: 0.10,
        tail_plan_round: 0.04,
        hood_crown: 0.040,
        windshield_rake_deg: Some(64.0),
        a_pillar_rake_deg: 22.0,
        rear_glass_rake_deg: Some(74.0),
        roof_len_trim: 0.25,
        tumblehome: Some(0.36),
        crease_z: 0.36,
        crease_depth: 0.030,
        haunch: 0.080,
        front_haunch: 0.080,
        flare: 0.030,
        ride_dz: -0.03,
        stance_pitch_deg: 0.5,
        ducktail_h: 0.0,
        belt_kick: 0.04,
        belt_rake: 0.02,
        tail_round: 0.02,
        tail_drop: 0.0,
        ..BodyStyle::preset(Silhouette::Wedge)
    });
    wheels(
        &mut s,
        def,
        RimKind::Monoblock,
        9,
        0.74,
        M::Trim,
        CapKind::CentreLock,
        (0.03, 0.010),
        (-1.8, -1.2),
    );
    s.tints.push((M::Paint, rgb(0x1B1F3B)));
    s
}

/// FWD touring-car racer: production sedan shell, box flares, splitter, big single-element wing.
pub fn merlo(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        nose_h: Some(0.62),
        nose_rake_deg: 30.0,
        nose_plan_round: 0.22,
        tail_plan_round: 0.14,
        hood_crown: 0.020,
        windshield_rake_deg: Some(60.0),
        a_pillar_rake_deg: 14.0,
        rear_glass_rake_deg: Some(62.0),
        tumblehome: Some(0.20),
        crease_z: 0.55,
        crease_depth: 0.015,
        haunch: 0.040,
        front_haunch: 0.040,
        flare: 0.060,
        flare_shape: ArchShape::Box,
        ride_dz: -0.04,
        stance_pitch_deg: 0.3,
        ducktail_h: 0.0,
        belt_kick: 0.03,
        belt_rake: 0.02,
        tail_round: 0.06,
        tail_drop: 0.0,
        ..BodyStyle::preset(Silhouette::Notchback)
    });
    wheels(
        &mut s,
        def,
        RimKind::Monoblock,
        6,
        0.76,
        M::Trim,
        CapKind::Centre,
        (0.025, 0.010),
        (-2.0, -1.0),
    );
    s.tints.push((M::Paint, rgb(0x00A6D6)));
    s
}
