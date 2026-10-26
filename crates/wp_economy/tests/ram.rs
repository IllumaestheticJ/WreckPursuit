//! Battering-ram mods (`ModEffect::Ram`, owner 2026-10-26: "make the cars feel like a
//! battering ram"): every ramming / reinforcement mod has a ram face, pays for it in mass and
//! drag, and bigger bars hit harder.

mod common;

use common::*;
use wp_economy::mods::{ModEffect, RamZone, RAM_MAX_MASS_SCALE, RAM_MIN_TAKEN_SCALE};
use wp_economy::RamProfile;

const RAM_MODS: [&str; 10] = [
    "prot_cage_1",
    "prot_cage_2",
    "prot_bumper_front",
    "prot_bumper_rear",
    "prot_door_bars",
    "prot_armor_plate",
    "prot_push_bar",
    "prot_bull_bar_tube",
    "prot_bull_bar_guard",
    "prot_bull_bar_ram",
];

fn profile(ids: &[&str]) -> RamProfile {
    let e = econ();
    let set: Vec<_> = ids.iter().map(|m| e.catalog.get(m).unwrap()).collect();
    RamProfile::of_mods(&set)
}

#[test]
fn every_ramming_mod_has_a_ram_face_and_pays_for_it() {
    let e = econ();
    for id in RAM_MODS {
        let m = e.catalog.get(id).unwrap();
        assert!(
            m.effects.iter().any(|x| matches!(x, ModEffect::Ram { .. })),
            "{id} has no ram effect"
        );
        assert!(
            m.mass_delta_kg() >= 20.0,
            "{id} adds only {} kg",
            m.mass_delta_kg()
        );
    }
    // Every ram mod is in this list (a new one needs its place in the ladder below).
    for m in e.catalog.iter() {
        if m.effects.iter().any(|x| matches!(x, ModEffect::Ram { .. })) {
            assert!(
                RAM_MODS.contains(&m.id.as_str()),
                "{} is not in RAM_MODS",
                m.id
            );
        }
    }
}

#[test]
fn bigger_bull_bars_hit_harder_and_weigh_more() {
    let e = econ();
    let mut last = (1.0f32, 1.0f32, 1.0f32, 0.0f32);
    for id in [
        "prot_bull_bar_tube",
        "prot_bull_bar_guard",
        "prot_bull_bar_ram",
    ] {
        let f = profile(&[id]).face(RamZone::Front);
        let kg = e.catalog.get(id).unwrap().mass_delta_kg();
        assert!(f.mass_scale > last.0, "{id} mass scale {}", f.mass_scale);
        assert!(f.dealt_scale > last.1, "{id} dealt scale {}", f.dealt_scale);
        assert!(f.taken_scale < last.2, "{id} taken scale {}", f.taken_scale);
        assert!(kg > last.3, "{id} adds {kg} kg");
        last = (f.mass_scale, f.dealt_scale, f.taken_scale, kg);
        // A front bar does nothing for the tail or the doors.
        let p = profile(&[id]);
        assert!(p.face(RamZone::Rear) == wp_economy::RamFace::STOCK);
        assert!(p.face(RamZone::Side) == wp_economy::RamFace::STOCK);
    }
}

#[test]
fn ram_faces_stack_within_limits() {
    let all = profile(&["prot_bumper_front", "prot_bull_bar_ram", "prot_cage_2"]);
    let f = all.face(RamZone::Front);
    let bar = profile(&["prot_bull_bar_ram"]).face(RamZone::Front);
    assert!(f.mass_scale > bar.mass_scale && f.mass_scale <= RAM_MAX_MASS_SCALE);
    assert!(f.taken_scale < bar.taken_scale && f.taken_scale >= RAM_MIN_TAKEN_SCALE);
    assert!(RamProfile::default().is_stock());
}

#[test]
fn zones_follow_the_contact_normal() {
    // Normals point into the car; +Y forward.
    assert_eq!(RamZone::of_normal([0.0, -1.0, 0.0]), Some(RamZone::Front));
    assert_eq!(RamZone::of_normal([0.0, 1.0, 0.0]), Some(RamZone::Rear));
    assert_eq!(RamZone::of_normal([-1.0, 0.0, 0.0]), Some(RamZone::Side));
    assert_eq!(RamZone::of_normal([0.6, -0.6, 0.53]), Some(RamZone::Front));
    assert_eq!(RamZone::of_normal([0.0, 0.0, -1.0]), None);
}

#[test]
fn applied_mods_carry_the_ram_profile() {
    let e = econ();
    let car = sedan();
    let p = e
        .preview(
            &car.def,
            &car.lattice,
            &["prot_bull_bar_ram".to_string()],
            None,
        )
        .unwrap();
    assert_eq!(p.applied.ram, profile(&["prot_bull_bar_ram"]));
    let stock = e.preview(&car.def, &car.lattice, &[], None).unwrap();
    assert!(stock.applied.ram.is_stock());
}
