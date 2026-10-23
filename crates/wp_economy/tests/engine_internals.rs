//! Engine internals in the garage workshop: parts inside the engine, the engine stand, wear
//! from engine damage and rebuilding it yourself (`wp_economy::parts`).

mod common;

use common::*;
use wp_contracts::{Component, DamageMode, HandlingEffects, RepairItem};
use wp_economy::repair::{reference_estimate, DamageReport, RepairOption, ShopContext};
use wp_economy::{CarUid, Economy, EconomyError, PlayerAccount};

const MODE: DamageMode = DamageMode::Arcade;
const ENGINE: RepairItem = RepairItem::Component(Component::Engine);

fn rich() -> (Economy, PlayerAccount, CarUid) {
    let e = econ();
    let (a, car) = account_with_sedan(&e, 500_000);
    (e, a, car)
}

fn fitted(e: &Economy, a: &PlayerAccount, car: CarUid, slot: &str) -> bool {
    e.car_parts(a.car(car).unwrap()).fitted(slot).is_some()
}

fn wear(e: &Economy, a: &PlayerAccount, car: CarUid, slot: &str) -> f32 {
    e.car_parts(a.car(car).unwrap())
        .fitted(slot)
        .map_or(f32::NAN, |f| f.wear)
}

/// Take everything in the way of `slot` off the car (depth first), not `slot` itself.
fn clear_way(e: &Economy, a: &mut PlayerAccount, car: CarUid, slot: &str) {
    let blockers = e.parts.slot(slot).unwrap().blocked_by.clone();
    for b in blockers {
        if fitted(e, a, car, &b) {
            clear_way(e, a, car, &b);
            remove(e, a, car, &b);
        }
    }
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

fn buy(e: &Economy, a: &mut PlayerAccount, car: CarUid, kind: &str, grade: Option<&str>) -> u32 {
    e.buy_part(a, car, kind, grade, MODE)
        .unwrap_or_else(|err| panic!("buy {kind}: {err}"))
        .0[0]
}

fn refused(r: Result<impl std::fmt::Debug, EconomyError>) -> String {
    match r {
        Err(EconomyError::PartRefused { reason, .. }) => reason,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// Record a session that left the engine at `severity`.
fn engine_damage(e: &Economy, a: &mut PlayerAccount, car: CarUid, severity: f32) {
    let c = a.car(car).unwrap();
    let est = reference_estimate(
        &e.price_table_for(c),
        e.mode_table(MODE).repair_cost_scale,
        &[(ENGINE, severity)],
    );
    let r = DamageReport {
        car,
        health: 0.7,
        total_loss: false,
        handling: HandlingEffects::NEUTRAL,
        estimate: est,
        lattice_state: vec![1],
        wanted_during_session: false,
        towed: false,
    };
    e.record_damage(a, &r).unwrap();
}

fn engine_line(a: &PlayerAccount, car: CarUid) -> Option<f32> {
    a.car(car)
        .unwrap()
        .damage
        .outstanding
        .lines
        .iter()
        .find(|l| l.item == ENGINE)
        .map(|l| l.severity)
}

fn internals(e: &Economy) -> Vec<String> {
    e.parts.children("engine").map(|s| s.id.clone()).collect()
}

#[test]
fn the_engine_has_internals_with_a_removal_order() {
    let e = econ();
    let ids = internals(&e);
    for id in [
        "cylinder_head",
        "camshafts",
        "valvetrain",
        "head_gasket",
        "pistons",
        "con_rods",
        "crankshaft",
        "oil_pump",
        "oil_pan",
        "timing",
        "flywheel",
        "clutch",
    ] {
        assert!(ids.iter().any(|i| i == id), "{id} is not inside the engine");
    }
    assert!(ids.len() >= 14);
    let head = e.parts.slot("cylinder_head").unwrap();
    assert!(head.blocked_by.contains(&"camshafts".to_string()));
    let crank = e.parts.slot("crankshaft").unwrap();
    assert!(crank.blocked_by.contains(&"con_rods".to_string()));
}

#[test]
fn internals_come_out_one_by_one_in_order() {
    let (e, mut a, car) = rich();
    let why = refused(e.remove_part(&mut a, car, "pistons"));
    assert!(why.contains("Head gasket"), "{why}");
    clear_way(&e, &mut a, car, "pistons");
    for gone in [
        "cylinder_head",
        "camshafts",
        "valve_cover",
        "intake_manifold",
    ] {
        assert!(!fitted(&e, &a, car, gone), "{gone} should be off");
    }
    let p = remove(&e, &mut a, car, "pistons");
    let missing = e.missing_parts(a.car(car).unwrap());
    assert!(
        missing.iter().any(|m| m == "Pistons and rings"),
        "{missing:?}"
    );
    // Back in, in reverse order.
    let refuse = refused(e.fit_part(&mut a, car, "cylinder_head", p, MODE));
    assert_eq!(refuse, "wrong kind of part");
    fit(&e, &mut a, car, "pistons", p);
}

#[test]
fn the_engine_comes_out_whole_with_its_internals() {
    let (e, mut a, car) = rich();
    let before = e.car_parts(a.car(car).unwrap());
    clear_way(&e, &mut a, car, "engine");
    let eng = remove(&e, &mut a, car, "engine");
    let loose = a.inventory.iter().find(|p| p.uid == eng).unwrap().clone();
    assert_eq!(loose.contents.len(), internals(&e).len());
    for id in internals(&e) {
        assert!(
            !fitted(&e, &a, car, &id),
            "{id} left the car with the engine"
        );
    }
    let missing = e.missing_parts(a.car(car).unwrap());
    assert!(missing.iter().any(|m| m == "Engine block"), "{missing:?}");
    assert!(
        !missing.iter().any(|m| m == "Pistons and rings"),
        "internals of a missing engine are not listed: {missing:?}"
    );
    // The stand sees it, and its sale price includes the internals.
    let view = e.workshop(&a, car, MODE).unwrap();
    assert_eq!(view.stands.len(), 1);
    assert!(view.stands[0].missing.is_empty());
    let alone = {
        let mut bare = loose.clone();
        bare.contents.clear();
        e.part_sale_price(&bare)
    };
    assert!(e.part_sale_price(&loose) > alone);
    // Back in: everything is where it was.
    fit(&e, &mut a, car, "engine", eng);
    let after = e.car_parts(a.car(car).unwrap());
    for id in internals(&e) {
        assert_eq!(before.fitted(&id), after.fitted(&id), "{id}");
    }
}

#[test]
fn the_engine_stand_strips_and_builds_in_order() {
    let (e, mut a, car) = rich();
    clear_way(&e, &mut a, car, "engine");
    let eng = remove(&e, &mut a, car, "engine");
    let why = refused(e.strip_part(&mut a, eng, "pistons"));
    assert!(why.contains("Head gasket"), "{why}");
    let mut out = Vec::new();
    for slot in [
        "intake_manifold",
        "throttle_body",
        "exhaust_manifold",
        "valve_cover",
        "timing",
        "camshafts",
        "valvetrain",
        "cylinder_head",
        "head_gasket",
        "pistons",
    ] {
        let uid = e
            .strip_part(&mut a, eng, slot)
            .unwrap_or_else(|err| panic!("strip {slot}: {err}"))
            .0;
        out.push((slot, uid));
    }
    let stand = &e.workshop(&a, car, MODE).unwrap().stands[0];
    assert!(stand.missing.iter().any(|m| m == "Cylinder head"));
    // A loose piston set fits back on the stand, not on the car (no engine there).
    let view = e.workshop(&a, car, MODE).unwrap();
    let pistons = view
        .inventory
        .iter()
        .find(|p| p.part.kind == "pistons")
        .unwrap();
    assert!(pistons.fits.is_empty());
    assert_eq!(pistons.stand_fits.len(), 1);
    // Build back in reverse order.
    let refuse = refused(e.build_part(&mut a, eng, "cylinder_head", out[7].1));
    assert!(
        refuse.contains("take off first") || refuse.contains("Head gasket"),
        "{refuse}"
    );
    for (slot, uid) in out.iter().rev() {
        e.build_part(&mut a, eng, slot, *uid)
            .unwrap_or_else(|err| panic!("build {slot}: {err}"));
    }
    fit(&e, &mut a, car, "engine", eng);
    assert!(e.missing_parts(a.car(car).unwrap()).len() < 10);
}

#[test]
fn stock_internals_fit_their_own_engine_only() {
    let (e, mut a, car) = rich();
    clear_way(&e, &mut a, car, "engine");
    let stock = remove(&e, &mut a, car, "engine");
    // A swap engine comes complete with its own internals.
    let swap = buy(&e, &mut a, car, "engine", Some("engine_v8_swap"));
    let v8 = a.inventory.iter().find(|p| p.uid == swap).unwrap();
    assert_eq!(v8.contents.len(), internals(&e).len());
    assert_eq!(
        v8.contents[0].model.as_deref(),
        Some("engine_v8_swap"),
        "stock internals of the swap engine belong to it"
    );
    // Pull the oil pan from the stock engine and try it on the V8.
    let pan = e.strip_part(&mut a, stock, "oil_pan").unwrap().0;
    e.strip_part(&mut a, swap, "oil_pan").unwrap();
    let why = refused(e.build_part(&mut a, swap, "oil_pan", pan));
    assert!(why.contains("another engine"), "{why}");
    e.build_part(&mut a, stock, "oil_pan", pan).unwrap();
}

#[test]
fn engine_damage_wears_the_internals_once() {
    let (e, mut a, car) = rich();
    engine_damage(&e, &mut a, car, 0.4);
    let head_gasket = wear(&e, &a, car, "head_gasket");
    let crank = wear(&e, &a, car, "crankshaft");
    assert!((head_gasket - 0.48).abs() < 1e-4, "{head_gasket}");
    assert!((crank - 0.2).abs() < 1e-4, "{crank}");
    assert!((wear(&e, &a, car, "engine") - 0.16).abs() < 1e-4);
    // The same damage reported again (the next session started from it) adds nothing.
    engine_damage(&e, &mut a, car, 0.4);
    assert!((wear(&e, &a, car, "head_gasket") - head_gasket).abs() < 1e-6);
    // More damage adds only the rise.
    engine_damage(&e, &mut a, car, 0.5);
    assert!((wear(&e, &a, car, "crankshaft") - 0.25).abs() < 1e-4);
    // While the line is open the damage sim takes the power; wear costs nothing extra.
    assert_eq!(e.wear_torque_factor(a.car(car).unwrap()), 1.0);
    let view = e.workshop(&a, car, MODE).unwrap();
    let slot = view.slots.iter().find(|s| s.id == "head_gasket").unwrap();
    let cond = slot.fitted.as_ref().unwrap().condition;
    assert!((cond - 0.4).abs() < 1e-4, "{cond}");
}

#[test]
fn replacing_the_worn_internals_rebuilds_the_engine() {
    let (e, mut a, car) = rich();
    engine_damage(&e, &mut a, car, 0.3);
    assert!(engine_line(&a, car).is_some());
    let worn: Vec<String> = e
        .parts
        .children("engine")
        .filter(|s| wear(&e, &a, car, &s.id) > s.wear_limit())
        .map(|s| s.id.clone())
        .collect();
    assert!(worn.len() > 5, "{worn:?}");
    // Strip down to the deepest worn part, swap every worn part for a new one on the way back.
    let mut restored = Vec::new();
    let mut order: Vec<String> = Vec::new();
    for w in &worn {
        clear_way_recording(&e, &mut a, car, w, &mut order);
        if fitted(&e, &a, car, w) {
            remove(&e, &mut a, car, w);
            order.push(w.clone());
        }
    }
    for slot in order.iter().rev() {
        let uid = if worn.contains(slot) {
            buy(&e, &mut a, car, slot, None)
        } else {
            a.inventory
                .iter()
                .rev()
                .find(|p| &p.kind == slot)
                .unwrap()
                .uid
        };
        restored.extend(fit(&e, &mut a, car, slot, uid));
    }
    assert_eq!(
        restored.iter().filter(|i| **i == ENGINE).count(),
        1,
        "the last worn part completes the rebuild: {restored:?}"
    );
    assert!(engine_line(&a, car).is_none());
    // The block keeps its own (serviceable) wear; that costs a little torque now.
    let f = e.wear_torque_factor(a.car(car).unwrap());
    assert!(f < 1.0 && f > 0.9, "{f}");
}

fn clear_way_recording(
    e: &Economy,
    a: &mut PlayerAccount,
    car: CarUid,
    slot: &str,
    order: &mut Vec<String>,
) {
    let blockers = e.parts.slot(slot).unwrap().blocked_by.clone();
    for b in blockers {
        if fitted(e, a, car, &b) {
            clear_way_recording(e, a, car, &b, order);
            remove(e, a, car, &b);
            order.push(b);
        }
    }
}

#[test]
fn a_worn_part_costs_torque_in_a_healthy_engine() {
    let (e, mut a, car) = rich();
    clear_way(&e, &mut a, car, "pistons");
    let p = remove(&e, &mut a, car, "pistons");
    a.inventory
        .iter_mut()
        .find(|x| x.uid == p)
        .unwrap()
        .condition = 0.5;
    fit(&e, &mut a, car, "pistons", p);
    let f = e.wear_torque_factor(a.car(car).unwrap());
    assert!((f - (1.0 - 0.35 * 0.5)).abs() < 1e-4, "{f}");
}

#[test]
fn a_shop_engine_repair_rebuilds_every_internal() {
    let (e, mut a, car) = rich();
    engine_damage(&e, &mut a, car, 0.5);
    assert!(wear(&e, &a, car, "pistons") > 0.0);
    let shop = ShopContext::legit_default();
    e.repair(&mut a, car, &RepairOption::RepairAll, &shop, MODE)
        .unwrap();
    for s in e.parts.children("engine") {
        assert_eq!(wear(&e, &a, car, &s.id), 0.0, "{}", s.id);
    }
    assert_eq!(wear(&e, &a, car, "engine"), 0.0);
}

#[test]
fn aftermarket_internals_are_mods() {
    let (e, mut a, car) = rich();
    clear_way(&e, &mut a, car, "camshafts");
    remove(&e, &mut a, car, "camshafts");
    // Race cams need stiff valve springs.
    let race = buy(&e, &mut a, car, "camshafts", Some("engine_cam_race"));
    let why = refused(e.fit_part(&mut a, car, "camshafts", race, MODE));
    assert!(why.contains("engine_valves_stiff"), "{why}");
    let cam = buy(&e, &mut a, car, "camshafts", Some("engine_cam_street"));
    fit(&e, &mut a, car, "camshafts", cam);
    assert!(a
        .car(car)
        .unwrap()
        .mods
        .contains(&"engine_cam_street".to_string()));
}
