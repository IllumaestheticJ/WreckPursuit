//! Data files load, are complete and agree with the embedded copy.

mod common;

use std::collections::BTreeSet;

use common::*;
use wp_economy::data::{DataSources, MOD_PACKS};
use wp_economy::{Economy, ModSlot};

#[test]
fn embedded_and_disk_data_agree() {
    let disk = Economy::load(&repo().join("data")).unwrap();
    assert_eq!(
        disk,
        econ(),
        "embedded data differs from data/ (MOD_PACKS out of date?)"
    );
    let stems: Vec<String> = DataSources::from_dir(&repo().join("data"))
        .unwrap()
        .mods
        .into_iter()
        .map(|(s, _)| s)
        .collect();
    assert_eq!(stems, MOD_PACKS.to_vec());
}

#[test]
fn catalog_is_big_and_covers_every_slot() {
    let e = econ();
    assert!(e.catalog.len() >= 80, "only {} mods", e.catalog.len());
    use ModSlot::*;
    for slot in [
        Engine,
        ForcedInduction,
        Drivetrain,
        Suspension,
        Brakes,
        Tires,
        Weight,
        Aero,
        BodyFront,
        BodySides,
        BodyRear,
        Hood,
        Roof,
        Protection,
        Cooling,
        Electronics,
        Paint,
        Vinyls,
        Wheels,
        Interior,
    ] {
        assert!(
            e.catalog.iter().any(|m| m.slot == slot),
            "no mod in slot {slot:?}"
        );
    }
}

#[test]
fn mod_materials_exist() {
    let e = econ();
    for m in e.catalog.iter() {
        for mat in m.material_ids() {
            assert!(
                repo().join(format!("data/materials/{mat}.json")).exists(),
                "mod {} references missing material {mat}",
                m.id
            );
        }
    }
}

#[test]
fn every_mod_applies_to_a_car_it_allows() {
    let e = econ();
    let sedan = sedan();
    let coupe = coupe();
    for m in e.catalog.iter() {
        // Build the minimal legal set: the mod plus its requirements (recursively).
        let mut set = BTreeSet::new();
        let mut stack = vec![m.id.clone()];
        while let Some(id) = stack.pop() {
            if set.insert(id.clone()) {
                stack.extend(e.catalog.get(&id).unwrap().requires.iter().cloned());
            }
        }
        let ids: Vec<String> = set.into_iter().collect();
        for car in [&sedan, &coupe] {
            if ids
                .iter()
                .all(|id| e.catalog.get(id).unwrap().allows_class(car.def.class))
            {
                let p = e
                    .preview(&car.def, &car.lattice, &ids, None)
                    .unwrap_or_else(|err| panic!("{} on {}: {err}", m.id, car.def.id));
                assert!(p.class_index >= 100);
            }
        }
    }
}

#[test]
fn invalid_data_is_rejected() {
    let mut src = DataSources::embedded();
    src.prices = src.prices.replace("[class.Exotic]", "[class.Exotic_typo]");
    assert!(Economy::from_sources(&src).is_err());

    let mut src = DataSources::embedded();
    src.mods[0].1 = src.mods[0]
        .1
        .replacen("\"price\": ", "\"price\": 0, \"x\": ", 1);
    assert!(Economy::from_sources(&src).is_err());

    let mut src = DataSources::embedded();
    src.mods[0].0 = "wrong_stem".into();
    assert!(Economy::from_sources(&src).is_err());
}
