//! Wave-3 off-road: `mesquite_pr` (2010s full-size pre-runner pickup) and `erg_hamada`
//! (2020s desert rally-raid prototype).
//!
//! Desert-racing vocabulary: tall stance on fat all-terrains, wide fibreglass flares that clear
//! long-travel suspension, tube bumpers / skid plates, light bars, roof scoops, spare wheels.
//! Both are hero cars (the performance module claims them), so they get the hero budget even
//! though their class is `Truck`. Mechanical parts come from `mechanical::spec` (table).

use wp_contracts::VehicleDef;

use crate::mesh::MaterialSlot as M;
use crate::style::{ArchShape, BodyStyle, CapKind, CarStyle, RimKind, Silhouette};

use super::{rgb, wheels};

/// Pre-runner pickup: crew cab, short bed, bolt-on fibreglass fenders, tube bumpers.
pub fn mesquite(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        nose_h: Some(1.05),
        hood_crown: 0.020,
        windshield_rake_deg: Some(58.0),
        tumblehome: Some(0.10),
        haunch: 0.020,
        front_haunch: 0.020,
        flare: 0.090,
        flare_shape: ArchShape::Box,
        ride_dz: 0.04,
        stance_pitch_deg: -0.3,
        ..BodyStyle::preset(Silhouette::Pickup)
    });
    wheels(
        &mut s,
        def,
        RimKind::Truck,
        6,
        0.40,
        M::Metal,
        CapKind::Centre,
        (0.04, 0.015),
        (0.0, 0.0),
    );
    s.tints.push((M::Paint, rgb(0xC9B79C)));
    s
}

/// Rally-raid prototype: tube-frame buggy body, short nose, roof scoop, spare wheels in the tail.
pub fn erg(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        nose_h: Some(0.95),
        nose_rake_deg: 35.0,
        hood_crown: 0.040,
        windshield_rake_deg: Some(55.0),
        tumblehome: Some(0.18),
        haunch: 0.030,
        front_haunch: 0.030,
        flare: 0.080,
        flare_shape: ArchShape::Box,
        ride_dz: 0.03,
        stance_pitch_deg: -0.2,
        ..BodyStyle::preset(Silhouette::Suv)
    });
    wheels(
        &mut s,
        def,
        RimKind::Steel,
        6,
        0.38,
        M::Trim,
        CapKind::Centre,
        (0.045, 0.015),
        (0.0, 0.0),
    );
    s.tints.push((M::Paint, rgb(0xE4572E)));
    s
}
