//! Garage workshop: part slots, removal order, inventory, catalog (`wp_economy::parts`).

mod common;

use common::*;
use wp_contracts::{BodyRegion, Component, DamageMode, HandlingEffects, RepairItem};
use wp_economy::repair::{reference_estimate, DamageReport};
use wp_economy::{CarSpec, CarUid, Condition, Economy, EconomyError, PlayerAccount};

const MODE: DamageMode = DamageMode::Arcade;

fn rich() -> (Economy, PlayerAccount, CarUid) {
    let e = econ();
    let (a, car) = account_with_sedan(&e, 200_000);
    (e, a, car)
}

fn remove(e: &Economy, a: &mut PlayerAccount, car: CarUid, slot: &str) -> u32 {
    e.remove_part(a, car, slot)
        .unwrap_or_else(|err| panic!("remove {slot}: {err}"))
        .0
}

fn fit(e: &Economy, a: &mut PlayerAccount, car: CarUid, slot: &str, part: u32) -> Vec<RepairItem> {
    e.fit_part(a, car, slot, part, MODE)
        .unwrap_or_else(|err| panic!("fit {slot}: {err}"))
        .0
        .restore
}

fn refused(r: Result<impl std::fmt::Debug, EconomyError>) -> String {
    match r {
        Err(EconomyError::PartRefused { reason, .. }) => reason,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn damage(e: &Economy, a: &mut PlayerAccount, car: CarUid) {
    let c = a.car(car).unwrap();
    let est = reference_estimate(
        &e.price_table_for(c),
        e.mode_table(MODE).repair_cost_scale,
        &[
            (RepairItem::Region(BodyRegion::FrontBumper), 0.6),
            (RepairItem::Component(Component::Radiator), 0.5),
        ],
    );
    let mut handling = HandlingEffects::NEUTRAL;
    handling.tire_puncture[0] = 0.75;
    let r = DamageReport {
        car,
        health: 0.8,
        total_loss: false,
        handling,
        estimate: est,
        lattice_state: vec![1, 2, 3],
        wanted_during_session: false,
        towed: false,
    };
    e.record_damage(a, &r).unwrap();
}

#[test]
fn embedded_slot_table_is_valid() {
    let e = econ();
    assert!(e.parts.slots.len() > 40);
    e.parts.validate(&e.catalog).unwrap();
}

#[test]
fn stock_car_has_stock_parts_and_empty_optional_slots() {
    let (e, a, car) = rich();
    let c = a.car(car).unwrap();
    assert!(c.parts.is_none(), "a new car has no stored parts state");
    let parts = e.car_parts(c);
    for s in &e.parts.slots {
        let fitted = parts.fitted(&s.id);
        if s.optional {
            assert!(fitted.is_none(), "{} should be empty", s.id);
        } else {
            assert_eq!(fitted.unwrap().grade, None, "{} should be stock", s.id);
        }
    }
    assert!(e.missing_parts(c).is_empty());
    assert_eq!(e.derive_mods(&c.mods, &parts), c.mods);
}

#[test]
fn removal_order_is_enforced_and_missing_parts_are_listed() {
    let (e, mut a, car) = rich();
    let before = a.clone();
    let why = refused(e.remove_part(&mut a, car, "radiator"));
    assert!(why.contains("Front bumper"), "{why}");
    assert_eq!(a, before, "a refused operation changes nothing");

    let bumper = remove(&e, &mut a, car, "front_bumper");
    let rad = remove(&e, &mut a, car, "radiator");
    assert_ne!(bumper, rad);
    assert_eq!(a.inventory.len(), 2);
    let c = a.car(car).unwrap();
    assert_eq!(e.missing_parts(c), vec!["Radiator".to_string()]);

    // Refit in reverse order: the radiator slot is blocked by the bumper.
    fit(&e, &mut a, car, "radiator", rad);
    fit(&e, &mut a, car, "front_bumper", bumper);
    assert!(a.inventory.is_empty());
    assert!(e.missing_parts(a.car(car).unwrap()).is_empty());
}

#[test]
fn fitting_needs_the_way_clear() {
    let (e, mut a, car) = rich();
    let bumper = remove(&e, &mut a, car, "front_bumper");
    let rad = remove(&e, &mut a, car, "radiator");
    fit(&e, &mut a, car, "front_bumper", bumper);
    let why = refused(e.fit_part(&mut a, car, "radiator", rad, MODE));
    assert!(why.contains("Front bumper"), "{why}");
}

#[test]
fn damage_travels_with_the_part() {
    let (e, mut a, car) = rich();
    damage(&e, &mut a, car);
    let c = a.car(car).unwrap();
    let total_before = c.damage.outstanding.total;
    let bumper_slot = e.parts.slot("front_bumper").unwrap();
    assert!((e.slot_condition(c, bumper_slot) - 0.4).abs() < 1e-4);

    let uid = remove(&e, &mut a, car, "front_bumper");
    let p = a.inventory.iter().find(|p| p.uid == uid).unwrap().clone();
    assert!((p.condition - 0.4).abs() < 1e-4);
    assert_eq!(p.damage.len(), 1);
    let c = a.car(car).unwrap();
    assert!(c.damage.outstanding.total < total_before);
    assert!(!c
        .damage
        .outstanding
        .lines
        .iter()
        .any(|l| l.item == RepairItem::Region(BodyRegion::FrontBumper)));

    // Putting the same damaged bumper back restores the bill and touches no lattice.
    let restore = fit(&e, &mut a, car, "front_bumper", uid);
    assert!(restore.is_empty());
    let c = a.car(car).unwrap();
    assert!((c.damage.outstanding.total - total_before).abs() < 1e-3);
}

#[test]
fn a_new_stock_part_clears_the_damage() {
    let (e, mut a, car) = rich();
    damage(&e, &mut a, car);
    let old_bumper = remove(&e, &mut a, car, "front_bumper");
    let old_rad = remove(&e, &mut a, car, "radiator");
    let cash = a.cash;
    let (uids, ev) = e.buy_part(&mut a, car, "radiator", None, MODE).unwrap();
    assert_eq!(uids.len(), 1);
    assert_eq!(ev.len(), 1);
    assert!(a.cash < cash);
    let restore = fit(&e, &mut a, car, "radiator", uids[0]);
    assert_eq!(restore, vec![RepairItem::Component(Component::Radiator)]);
    fit(&e, &mut a, car, "front_bumper", old_bumper);
    let c = a.car(car).unwrap();
    assert_eq!(e.slot_condition(c, e.parts.slot("radiator").unwrap()), 1.0);
    // The damaged radiator is worth less than a new one, and selling it removes it.
    let new_value = a.inventory.iter().find(|p| p.uid == old_rad).unwrap().value;
    let worn = e.part_sale_price(a.inventory.iter().find(|p| p.uid == old_rad).unwrap());
    assert!(worn < new_value);
    let cash = a.cash;
    let (price, _) = e.sell_part(&mut a, old_rad).unwrap();
    assert_eq!(price, worn);
    assert_eq!(a.cash, cash + price);
    assert!(a.inventory.iter().all(|p| p.uid != old_rad));
}

#[test]
fn tyre_condition_comes_from_the_puncture() {
    let (e, mut a, car) = rich();
    damage(&e, &mut a, car);
    let flat = remove(&e, &mut a, car, "tyre_fl");
    let p = a.inventory.iter().find(|p| p.uid == flat).unwrap();
    assert!((p.condition - 0.25).abs() < 1e-4);
    assert_eq!(a.car(car).unwrap().damage.handling.tire_puncture[0], 0.0);
    let (new, _) = e.buy_part(&mut a, car, "tyres", None, MODE).unwrap();
    fit(&e, &mut a, car, "tyre_fl", new[0]);
    assert_eq!(a.car(car).unwrap().damage.handling.tire_puncture[0], 0.0);
    // The flat tyre fits any corner and brings its puncture with it.
    let spare = remove(&e, &mut a, car, "tyre_rr");
    fit(&e, &mut a, car, "tyre_rr", flat);
    assert!((a.car(car).unwrap().damage.handling.tire_puncture[3] - 0.75).abs() < 1e-4);
    assert!(a.inventory.iter().any(|p| p.uid == spare));
}

#[test]
fn aftermarket_parts_become_mods() {
    let (e, mut a, car) = rich();
    let (uids, _) = e
        .buy_part(&mut a, car, "radiator", Some("cooling_radiator_1"), MODE)
        .unwrap();
    assert_eq!(uids.len(), 1);
    assert!(a.car(car).unwrap().mods.is_empty(), "bought, not fitted");
    remove(&e, &mut a, car, "front_bumper");
    let stock = remove(&e, &mut a, car, "radiator");
    fit(&e, &mut a, car, "radiator", uids[0]);
    assert_eq!(a.car(car).unwrap().mods, vec!["cooling_radiator_1"]);
    // Swap back to stock: the mod goes away, the upgrade sits in the inventory.
    let upgrade = remove(&e, &mut a, car, "radiator");
    let p = a.inventory.iter().find(|p| p.uid == upgrade).unwrap();
    assert_eq!(p.grade.as_deref(), Some("cooling_radiator_1"));
    fit(&e, &mut a, car, "radiator", stock);
    assert!(a.car(car).unwrap().mods.is_empty());
}

#[test]
fn a_set_counts_only_when_complete() {
    let (e, mut a, car) = rich();
    let (rims, _) = e
        .buy_part(&mut a, car, "wheels", Some("wheels_mesh"), MODE)
        .unwrap();
    assert_eq!(rims.len(), 4);
    for (i, corner) in ["fl", "fr", "rl", "rr"].iter().enumerate() {
        let tyre = remove(&e, &mut a, car, &format!("tyre_{corner}"));
        remove(&e, &mut a, car, &format!("wheel_{corner}"));
        fit(&e, &mut a, car, &format!("wheel_{corner}"), rims[i]);
        fit(&e, &mut a, car, &format!("tyre_{corner}"), tyre);
        let has = a
            .car(car)
            .unwrap()
            .mods
            .contains(&"wheels_mesh".to_string());
        assert_eq!(has, i == 3, "after corner {corner}");
    }
    let w = e.workshop(&a, car, MODE).unwrap();
    assert_eq!(w.inventory.len(), 4, "four stock wheels in the inventory");
    assert!(w.inventory.iter().all(|p| p.fits.is_empty()));
}

#[test]
fn requirements_hold_in_the_workshop() {
    let (e, mut a, car) = rich();
    // Start from a car with a stored parts state, then let the shop fit the parts.
    let b = remove(&e, &mut a, car, "front_bumper");
    fit(&e, &mut a, car, "front_bumper", b);
    e.install_mod(&mut a, car, "aero_splitter", MODE).unwrap();
    e.install_mod(&mut a, car, "aero_wing_2", MODE).unwrap();
    let parts = a.car(car).unwrap().parts.clone().unwrap();
    assert_eq!(
        parts.fitted("front_splitter").unwrap().grade.as_deref(),
        Some("aero_splitter")
    );
    assert_eq!(
        parts.fitted("rear_wing").unwrap().grade.as_deref(),
        Some("aero_wing_2")
    );
    remove(&e, &mut a, car, "front_bumper");
    let why = refused(e.remove_part(&mut a, car, "front_splitter"));
    assert!(why.contains("aero_splitter"), "{why}");
    // Shop removal of the wing frees the splitter.
    e.uninstall_mod(&mut a, car, "aero_wing_2").unwrap();
    assert!(a
        .car(car)
        .unwrap()
        .parts
        .as_ref()
        .unwrap()
        .fitted("rear_wing")
        .is_none());
    remove(&e, &mut a, car, "front_splitter");
    assert!(a.car(car).unwrap().mods.is_empty());
}

#[test]
fn stock_parts_fit_their_own_model_only() {
    let (e, mut a, sedan_uid) = rich();
    let spec = CarSpec::from(&coupe().def);
    let (coupe_uid, _) = e.buy_car(&mut a, &spec, Condition::New).unwrap();
    let door = remove(&e, &mut a, sedan_uid, "door_fl");
    remove(&e, &mut a, coupe_uid, "door_fl");
    let why = refused(e.fit_part(&mut a, coupe_uid, "door_fl", door, MODE));
    assert!(why.contains("another model"), "{why}");
}

#[test]
fn optional_slots_have_no_stock_part() {
    let (e, mut a, car) = rich();
    let why = refused(e.buy_part(&mut a, car, "rear_wing", None, MODE));
    assert!(why.contains("no stock part"), "{why}");
    let cat = e.part_catalog(&a, car, MODE).unwrap();
    assert!(cat
        .iter()
        .any(|c| c.kind == "rear_wing" && c.grade.as_deref() == Some("aero_wing_1")));
    assert!(!cat
        .iter()
        .any(|c| c.kind == "rear_wing" && c.grade.is_none()));
    let wheels = cat
        .iter()
        .find(|c| c.kind == "wheels" && c.grade.as_deref() == Some("wheels_forged"))
        .unwrap();
    assert_eq!(wheels.count, 4);
}

#[test]
fn workshop_view_shows_blockers() {
    let (e, a, car) = rich();
    let w = e.workshop(&a, car, MODE).unwrap();
    let rad = w.slots.iter().find(|s| s.id == "radiator").unwrap();
    assert_eq!(rad.blocked_by, vec!["front_bumper".to_string()]);
    assert!(rad.remove_blocker.is_some());
    let bumper = w.slots.iter().find(|s| s.id == "front_bumper").unwrap();
    assert!(bumper.remove_blocker.is_none());
    assert!(w.missing.is_empty());
}

#[test]
fn saves_without_parts_still_load() {
    let (e, mut a, car) = rich();
    let mut v = serde_json::to_value(&a).unwrap();
    v.as_object_mut().unwrap().remove("inventory");
    v.as_object_mut().unwrap().remove("next_part_uid");
    let old: PlayerAccount = serde_json::from_value(v).unwrap();
    assert_eq!(old, a);
    remove(&e, &mut a, car, "front_bumper");
    let text = serde_json::to_string(&a).unwrap();
    let back: PlayerAccount = serde_json::from_str(&text).unwrap();
    assert_eq!(back, a);
}

#[test]
fn the_pound_blocks_the_workshop() {
    let (e, mut a, car) = rich();
    a.car_mut(car).unwrap().location = wp_economy::CarLocation::Impound {
        since_day: 0,
        level: wp_economy::WantedLevel::None,
    };
    assert!(matches!(
        e.remove_part(&mut a, car, "front_bumper"),
        Err(EconomyError::CarUnavailable { .. })
    ));
}
