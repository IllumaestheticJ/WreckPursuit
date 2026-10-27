//! Coilovers carry their own dampers: a car never has both coilovers and the separate
//! adjustable dampers (`ModDef::supersedes`, owner 2026-10-27).

mod common;

use common::*;
use wp_contracts::DamageMode;
use wp_economy::{EconomyError, PartSource};

const MODE: DamageMode = DamageMode::Arcade;
const DAMPERS: &str = "susp_dampers_1";

fn mods_of(a: &wp_economy::PlayerAccount, car: wp_economy::CarUid) -> Vec<String> {
    a.car(car).unwrap().mods.clone()
}

#[test]
fn both_coilovers_supersede_the_dampers() {
    let e = econ();
    for c in ["susp_coilovers_2", "susp_coilovers_3"] {
        assert!(e
            .catalog
            .get(c)
            .unwrap()
            .supersedes
            .contains(&DAMPERS.to_string()));
    }
    // The lift kit and lowering springs keep their dampers.
    for c in ["susp_springs_1", "susp_offroad_lift"] {
        assert!(e.catalog.get(c).unwrap().supersedes.is_empty());
    }
}

#[test]
fn a_set_with_both_is_illegal() {
    let e = econ();
    let set = vec!["susp_coilovers_3".to_string(), DAMPERS.to_string()];
    let r = e
        .catalog
        .validate_set(wp_contracts::VehicleClass::Sport, &set);
    assert!(
        matches!(&r, Err(EconomyError::ModNotAllowed { mod_id, .. }) if mod_id == DAMPERS),
        "{r:?}"
    );
}

#[test]
fn installing_coilovers_takes_the_dampers_off() {
    let e = econ();
    let (mut a, car) = account_with_sedan(&e, 20_000);
    e.install_mod(&mut a, car, DAMPERS, MODE).unwrap();
    let plan = e
        .catalog
        .check_install(
            a.car(car).unwrap().class,
            Some(MODE),
            &mods_of(&a, car),
            "susp_coilovers_3",
        )
        .unwrap();
    assert_eq!(plan.replaces, None);
    assert_eq!(plan.removes, vec![DAMPERS.to_string()]);
    let cash = a.cash;
    e.install_mod(&mut a, car, "susp_coilovers_3", MODE)
        .unwrap();
    assert_eq!(mods_of(&a, car), vec!["susp_coilovers_3".to_string()]);
    let refund = (1100.0 * e.market.mod_resale_frac).round() as i64;
    assert_eq!(
        a.cash,
        cash - 5500 + refund,
        "the dampers are refunded at resale value"
    );
}

#[test]
fn dampers_cannot_go_on_over_coilovers() {
    let e = econ();
    let (mut a, car) = account_with_sedan(&e, 20_000);
    e.install_mod(&mut a, car, "susp_coilovers_2", MODE)
        .unwrap();
    let snap = a.clone();
    let r = e.install_mod(&mut a, car, DAMPERS, MODE);
    assert!(
        matches!(&r, Err(EconomyError::ModNotAllowed { reason, .. }) if reason.contains("Street Coilovers")),
        "{r:?}"
    );
    assert_eq!(a, snap, "a refused install changes nothing");
    // Back on plain springs, the dampers fit again.
    e.install_mod(&mut a, car, "susp_springs_1", MODE).unwrap();
    e.install_mod(&mut a, car, DAMPERS, MODE).unwrap();
    assert!(mods_of(&a, car).contains(&DAMPERS.to_string()));
}

#[test]
fn the_mechanic_fitting_coilovers_takes_the_dampers_off() {
    let e = econ();
    let (mut a, car) = account_with_sedan(&e, 200_000);
    e.install_mod(&mut a, car, DAMPERS, MODE).unwrap();
    let cat = e.part_catalog(&a, car, MODE).unwrap();
    let entry = cat
        .iter()
        .find(|c| c.grade.as_deref() == Some("susp_coilovers_3"))
        .unwrap();
    assert_eq!(entry.replaces, vec!["Adjustable Dampers".to_string()]);
    e.mechanic_install(
        &mut a,
        car,
        "suspension_fl",
        &PartSource::Buy(Some("susp_coilovers_3".into())),
        true,
        MODE,
    )
    .unwrap();
    let mods = mods_of(&a, car);
    assert!(mods.contains(&"susp_coilovers_3".to_string()), "{mods:?}");
    assert!(!mods.contains(&DAMPERS.to_string()), "{mods:?}");
}

#[test]
fn an_old_save_with_both_loses_the_dampers_at_full_refund() {
    let e = econ();
    let (mut a, car) = account_with_sedan(&e, 0);
    a.car_mut(car).unwrap().mods = vec!["susp_coilovers_3".to_string(), DAMPERS.to_string()];
    let ((), ev) = e.settle_superseded_mods(&mut a).unwrap();
    assert_eq!(mods_of(&a, car), vec!["susp_coilovers_3".to_string()]);
    assert_eq!(a.cash, 1100);
    assert_eq!(ev.len(), 1);
    // Nothing more to settle.
    let ((), ev) = e.settle_superseded_mods(&mut a).unwrap();
    assert!(ev.is_empty());
}
