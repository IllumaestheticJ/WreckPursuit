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

#[test]
fn every_tire_file_has_a_grip_factor() {
    let e = econ();
    for entry in std::fs::read_dir(repo().join("data/tires"))
        .unwrap()
        .flatten()
    {
        let p = entry.path();
        if p.extension().is_some_and(|x| x == "json") {
            let id = p.file_stem().unwrap().to_str().unwrap().to_string();
            assert!(
                e.market.tire_grip.contains_key(&id),
                "market.tire_grip lacks {id}"
            );
        }
    }
}

#[test]
fn price_table_fills_contract_v0_2_fields() {
    use wp_contracts::{RepairItem, VehicleClass};
    use wp_economy::data::{ALL_COMPONENTS, ALL_REGIONS};
    let e = econ();
    let t = e.price_table(VehicleClass::Compact, 18_000);
    assert_eq!(
        t.repair_parts_fraction,
        Some(e.prices.repair_parts_fraction)
    );
    for r in ALL_REGIONS {
        assert!(t.labour_hours.contains_key(&RepairItem::Region(r)), "{r:?}");
        assert!(
            t.labour_hours.contains_key(&RepairItem::DetachedPart(r)),
            "{r:?}"
        );
    }
    for c in ALL_COMPONENTS {
        assert!(
            t.labour_hours.contains_key(&RepairItem::Component(c)),
            "{c:?}"
        );
    }
    // Structure takes longer than a bolt-on panel.
    let h = |i| t.labour_hours[&i];
    use wp_contracts::BodyRegion::{Cabin, FrontBumper};
    assert!(h(RepairItem::Region(Cabin)) > 4.0 * h(RepairItem::Region(FrontBumper)));
}

#[test]
fn reference_pricing_matches_wp_damage() {
    use wp_economy::repair::{
        DEFAULT_REPAIR_PARTS_FRACTION, MIN_LABOUR_FRACTION, MIN_SEVERITY, REPLACE_SEVERITY,
    };
    assert_eq!(REPLACE_SEVERITY, wp_damage::repair::REPLACE_SEVERITY);
    assert_eq!(
        DEFAULT_REPAIR_PARTS_FRACTION,
        wp_damage::repair::REPAIR_PARTS_FRACTION
    );
    assert_eq!(MIN_LABOUR_FRACTION, wp_damage::repair::MIN_LABOUR_FRACTION);
    assert_eq!(MIN_SEVERITY, wp_damage::repair::MIN_SEVERITY);
}

#[test]
fn reference_line_repairs_below_and_replaces_above_threshold() {
    use wp_contracts::{BodyRegion, RepairItem, VehicleClass};
    use wp_economy::repair::reference_line;
    let e = econ();
    let t = e.price_table(VehicleClass::Compact, 18_000);
    let item = RepairItem::Region(BodyRegion::Hood);
    let cost = t.region_replace_cost[&BodyRegion::Hood];
    let repair = reference_line(&t, 1.0, item, 0.3);
    assert!((repair.parts_cost - cost * 0.3 * 0.3 / 0.6).abs() < 1e-3);
    let replace = reference_line(&t, 1.0, item, 0.8);
    assert_eq!(replace.parts_cost, cost);
    assert!((replace.labour_hours - t.labour_hours[&item] * 0.8).abs() < 1e-5);
    // Unscaled parts/hours; only the total carries the mode scale.
    let arcade = reference_line(&t, 0.5, item, 0.8);
    assert_eq!(arcade.parts_cost, replace.parts_cost);
    assert!((arcade.total - 0.5 * replace.total).abs() < 1e-3);
}

#[test]
fn a_tyre_the_car_already_runs_does_nothing() {
    let e = econ();
    let coupe = coupe(); // sold on sport tyres
    let stock = coupe.def.wheels[0].tire_id.clone();
    let same = e
        .catalog
        .iter()
        .find(|m| m.tire_ids().eq([stock.as_str()]))
        .unwrap_or_else(|| panic!("no tyre mod sets {stock}"));
    assert!(
        same.does_nothing_on(&coupe.def),
        "{} on {stock} tyres",
        same.id
    );
    let other = e
        .catalog
        .iter()
        .find(|m| m.tire_ids().next().is_some_and(|t| t != stock))
        .expect("a tyre mod with another compound");
    assert!(!other.does_nothing_on(&coupe.def), "{}", other.id);
    // Mods with any other effect always do something.
    let wing = e.catalog.get("aero_wing_1").unwrap();
    assert!(!wing.does_nothing_on(&coupe.def));
}
