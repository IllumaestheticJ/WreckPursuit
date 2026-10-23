//! Garage workshop made easy: the simpler removal order, "take off what's in the way" and the
//! paid mechanic install (`wp_economy::parts`).

mod common;

use common::*;
use wp_contracts::DamageMode;
use wp_economy::{CarUid, Economy, EconomyError, PartSource, PlayerAccount};

const MODE: DamageMode = DamageMode::Arcade;

fn rich() -> (Economy, PlayerAccount, CarUid) {
    let e = econ();
    let (a, car) = account_with_sedan(&e, 200_000);
    (e, a, car)
}

fn fitted_grade(e: &Economy, a: &PlayerAccount, car: CarUid, slot: &str) -> Option<String> {
    e.car_parts(a.car(car).unwrap())
        .fitted(slot)
        .map(|f| f.grade.clone().unwrap_or_else(|| "stock".into()))
}

fn slot_view(e: &Economy, a: &PlayerAccount, car: CarUid, slot: &str) -> wp_economy::SlotView {
    e.workshop(a, car, MODE)
        .unwrap()
        .slots
        .into_iter()
        .find(|s| s.id == slot)
        .unwrap()
}

#[test]
fn a_wheel_comes_off_with_its_tyre_and_brakes_only_need_the_wheel() {
    let (e, mut a, car) = rich();
    // No tyre first: the wheel comes straight off.
    e.remove_part(&mut a, car, "wheel_fl").unwrap();
    // The brake and the suspension only need the wheel off.
    e.remove_part(&mut a, car, "brake_fl").unwrap();
    e.remove_part(&mut a, car, "suspension_fl").unwrap();
    assert_eq!(a.inventory.len(), 3);
}

#[test]
fn clear_way_takes_off_everything_in_the_way() {
    let (e, mut a, car) = rich();
    let v = slot_view(&e, &a, car, "radiator");
    assert_eq!(v.in_the_way, vec!["front_bumper".to_string()]);
    assert_eq!(v.mechanic_fee, e.mechanic_fee(1));

    let cash = a.cash;
    let (off, _) = e.clear_way(&mut a, car, "radiator").unwrap();
    assert_eq!(off.len(), 1);
    assert_eq!(a.cash, cash, "doing it yourself is free");
    assert!(slot_view(&e, &a, car, "radiator").in_the_way.is_empty());
    e.remove_part(&mut a, car, "radiator").unwrap();
}

#[test]
fn the_mechanic_fits_a_new_part_and_puts_everything_back() {
    let (e, mut a, car) = rich();
    let cash = a.cash;
    let (job, _) = e
        .mechanic_install(&mut a, car, "radiator", &PartSource::Buy(None), false, MODE)
        .unwrap();
    assert_eq!(job.touched, 2, "bumper and radiator");
    assert_eq!(job.fee, e.mechanic_fee(1));
    assert!(job.bought > 0);
    assert_eq!(a.cash, cash - job.fee - job.bought);
    // The bumper is back on, the old radiator is in the inventory.
    assert!(fitted_grade(&e, &a, car, "front_bumper").is_some());
    assert!(e.missing_parts(a.car(car).unwrap()).is_empty());
    assert_eq!(job.kept.len(), 1);
    assert_eq!(a.inventory.len(), 1);
    assert_eq!(a.inventory[0].uid, job.kept[0]);
}

#[test]
fn the_mechanic_can_sell_the_old_part() {
    let (e, mut a, car) = rich();
    let cash = a.cash;
    let (job, _) = e
        .mechanic_install(&mut a, car, "radiator", &PartSource::Buy(None), true, MODE)
        .unwrap();
    assert!(a.inventory.is_empty(), "the old radiator was sold");
    assert!(job.kept.is_empty());
    assert!(job.sold > 0);
    assert_eq!(a.cash, cash - job.fee - job.bought + job.sold);
}

#[test]
fn the_mechanic_fits_a_whole_set() {
    let (e, mut a, car) = rich();
    let (job, _) = e
        .mechanic_install(
            &mut a,
            car,
            "wheel_rr",
            &PartSource::Buy(Some("wheels_mesh".into())),
            false,
            MODE,
        )
        .unwrap();
    for corner in ["fl", "fr", "rl", "rr"] {
        assert_eq!(
            fitted_grade(&e, &a, car, &format!("wheel_{corner}")).as_deref(),
            Some("wheels_mesh")
        );
    }
    assert_eq!(job.kept.len(), 4);
    assert_eq!(job.touched, 4);
    assert!(a
        .car(car)
        .unwrap()
        .mods
        .contains(&"wheels_mesh".to_string()));
    let cat = e.part_catalog(&a, car, MODE).unwrap();
    let mesh = cat
        .iter()
        .find(|c| c.kind == "wheels" && c.grade.as_deref() == Some("wheels_mesh"))
        .unwrap();
    assert_eq!(mesh.install_fee, job.fee, "the catalog quotes the set fee");
}

#[test]
fn the_mechanic_fits_a_part_from_the_inventory() {
    let (e, mut a, car) = rich();
    let (bought, _) = e.buy_part(&mut a, car, "front_bumper", None, MODE).unwrap();
    let (job, _) = e
        .mechanic_install(
            &mut a,
            car,
            "front_bumper",
            &PartSource::Inventory(bought[0]),
            false,
            MODE,
        )
        .unwrap();
    assert_eq!(job.bought, 0);
    assert_eq!(job.touched, 1);
    assert_eq!(a.inventory.len(), 1);
    assert_ne!(
        a.inventory[0].uid, bought[0],
        "the old bumper is what's left"
    );
}

#[test]
fn the_mechanic_refuses_when_you_cannot_pay() {
    let e = econ();
    let (mut a, car) = account_with_sedan(&e, 200_000);
    a.cash = 10;
    let before = a.clone();
    let r = e.mechanic_install(&mut a, car, "radiator", &PartSource::Buy(None), false, MODE);
    assert!(
        matches!(r, Err(EconomyError::InsufficientFunds { .. })),
        "{r:?}"
    );
    assert_eq!(a, before, "a refused job changes nothing");
}
