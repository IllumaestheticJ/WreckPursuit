//! Native tests of the browser career's car lots (`crate::lots`): daily stock, buying lot cars
//! with real wear, junkyard cars that do not run, tyres outside Repair All, and the lot visit.

use wp_contracts::{Component, DamageMode, RepairItem};
use wp_economy::{Economy, Listing, ListingGrade, LotKind, RepairOption};

use crate::career::{Career, ShopRequest};
use crate::lots::space_pose;

fn econ() -> Economy {
    crate::garage::with_economy(|e| e.clone())
}

/// A career with plenty of cash and Legend rep (every class unlocked).
fn rich(mode: DamageMode) -> Career {
    let mut c = Career::new(econ(), mode);
    c.profile_mut().account.cash = 5_000_000;
    c.profile_mut().account.rep = 20_000;
    c
}

/// First listing matching `f` at a lot of `kind`, searching day by day.
fn find(c: &mut Career, kind: LotKind, f: impl Fn(&Listing) -> bool) -> (String, Listing) {
    let lots: Vec<String> = c
        .economy()
        .lots
        .lots
        .iter()
        .filter(|l| l.kind == kind)
        .map(|l| l.id.clone())
        .collect();
    for _ in 0..200 {
        for id in &lots {
            if let Some((l, _)) = c
                .lot_stock(id)
                .unwrap()
                .into_iter()
                .find(|(l, s)| !s && f(l))
            {
                return (id.clone(), l);
            }
        }
        c.profile_mut().account.day += 1;
    }
    panic!("no matching {kind:?} listing in 200 days");
}

#[test]
fn every_lot_shows_in_the_view_and_restocks_daily() {
    let mut c = rich(DamageMode::Arcade);
    let v = c.view();
    assert_eq!(v.lots.len(), c.economy().lots.lots.len());
    let kinds: Vec<LotKind> = v.lots.iter().map(|l| l.kind).collect();
    for k in [LotKind::Dealership, LotKind::UsedLot, LotKind::Junkyard] {
        assert!(kinds.contains(&k));
    }
    let today = c.lot_view("dockside_salvage").unwrap();
    c.profile_mut().account.day += 1;
    let tomorrow = c.lot_view("dockside_salvage").unwrap();
    assert_ne!(today.cars, tomorrow.cars);
    // Junkyards hide the mechanical state.
    assert!(today
        .cars
        .iter()
        .all(|l| l.condition.is_none() && l.runs.is_none()));
}

#[test]
fn a_new_car_is_pristine_and_leaves_the_lot() {
    let mut c = rich(DamageMode::Simulation);
    let (lot, l) = find(&mut c, LotKind::Dealership, |_| true);
    let cash = c.profile().account.cash;
    let uid = c.buy_listing(&lot, l.slot).unwrap();
    assert_eq!(c.profile().account.cash, cash - l.price);
    let car = c.profile().account.car(uid).unwrap();
    assert_eq!(car.damage.health, 1.0);
    assert!(car.damage.outstanding.lines.is_empty());
    assert!(c.lattice(uid).unwrap().is_none());
    assert!(c.buy_listing(&lot, l.slot).is_err(), "sold today");
    assert!(c
        .lot_view(&lot)
        .unwrap()
        .cars
        .iter()
        .any(|x| x.slot == l.slot && x.sold));
    // Tomorrow the lot restocks.
    c.profile_mut().account.day += 1;
    assert!(c.lot_view(&lot).unwrap().cars.iter().all(|x| !x.sold));
}

#[test]
fn a_used_car_runs_with_wear_that_repairs_except_the_tyres() {
    for mode in [
        DamageMode::Arcade,
        DamageMode::Simulation,
        DamageMode::Extreme,
    ] {
        let mut c = rich(mode);
        let (lot, l) = find(&mut c, LotKind::UsedLot, |l| {
            l.condition < 0.75 && l.tires.tread.iter().all(|t| *t < 0.9)
        });
        let uid = c.buy_listing(&lot, l.slot).unwrap();
        let car = c.profile().account.car(uid).unwrap().clone();
        assert!(car.is_usable(), "{mode:?}: a used car runs");
        assert!(car.damage.health < 1.0, "{mode:?}");
        assert!(car.damage.health > 0.3, "{mode:?}: {}", car.damage.health);
        assert!(!car.damage.outstanding.lines.is_empty());
        assert_eq!(car.tires, l.tires);
        c.repair(uid, &ShopRequest::parse("").unwrap()).unwrap();
        let car = c.profile().account.car(uid).unwrap();
        assert_eq!(car.damage.health, 1.0, "{mode:?}: repaired");
        assert_eq!(car.tires, l.tires, "{mode:?}: Repair All never fits tyres");
        c.replace_tires(uid, [true; 4]).unwrap();
        let car = c.profile().account.car(uid).unwrap();
        assert_eq!(car.tires, wp_economy::TireState::default());
    }
}

#[test]
fn a_dead_junkyard_car_does_not_run_until_the_engine_is_fixed() {
    let mut c = rich(DamageMode::Simulation);
    let (lot, l) = find(&mut c, LotKind::Junkyard, |l| !l.runs);
    let uid = c.buy_listing(&lot, l.slot).unwrap();
    let car = c.profile().account.car(uid).unwrap().clone();
    assert!(car.damage.handling.engine_dead);
    assert!(!car.is_usable());
    assert!(c.start_blocker().is_some());
    let engine = RepairItem::Component(Component::Engine);
    assert!(car
        .damage
        .outstanding
        .lines
        .iter()
        .any(|x| x.item == engine));
    let req = ShopRequest {
        option: RepairOption::RepairSelected(vec![engine]),
        district: None,
        shop: wp_economy::ShopKind::Legit,
    };
    c.repair(uid, &req).unwrap();
    let car = c.profile().account.car(uid).unwrap();
    assert!(!car.damage.handling.engine_dead, "engine fixed");
}

#[test]
fn missing_panels_are_missing_on_the_bought_car() {
    let mut c = rich(DamageMode::Arcade);
    let (lot, l) = find(&mut c, LotKind::Junkyard, |l| !l.wear.missing.is_empty());
    let uid = c.buy_listing(&lot, l.slot).unwrap();
    let car = c.profile().account.car(uid).unwrap();
    // Every roster lattice has a detachable hood, bumpers, trunk and front doors.
    assert!(car
        .damage
        .outstanding
        .lines
        .iter()
        .any(|x| matches!(x.item, RepairItem::DetachedPart(_))));
}

#[test]
fn a_rare_variant_comes_with_its_mods() {
    let mut c = rich(DamageMode::Arcade);
    let (lot, l) = find(&mut c, LotKind::Junkyard, |l| !l.mods.is_empty());
    assert_eq!(l.grade, ListingGrade::Junk);
    let uid = c.buy_listing(&lot, l.slot).unwrap();
    let car = c.profile().account.car(uid).unwrap();
    for m in &l.mods {
        assert!(car.mods.contains(m));
    }
}

#[test]
fn visiting_a_lot_parks_todays_stock_in_its_spaces() {
    let mut c = rich(DamageMode::Arcade);
    let (lot, l) = find(&mut c, LotKind::Junkyard, |l| {
        !l.wear.missing.is_empty() || !l.wear.dents.is_empty()
    });
    let unsold = c.lot_stock(&lot).unwrap().len();
    let (s, visit) = c.visit_lot(&lot).unwrap();
    assert_eq!(visit.lot, lot);
    assert_eq!(visit.own_car, None, "no car yet: you came on foot");
    assert_eq!(visit.cars.len(), unsold);
    assert!(s.scenario().name.starts_with(crate::lots::VISIT_PREFIX));
    let vc = visit.cars.iter().find(|v| v.slot == l.slot).unwrap();
    let v = s.vehicle(wp_contracts::EntityId(vc.entity)).unwrap();
    let (pos, _) = space_pose(l.slot);
    let st = wp_contracts::VehicleSim::state(&v.vehicle);
    assert!((st.pos[0] - pos[0]).abs() < 0.5 && (st.pos[1] - pos[1]).abs() < 0.5);
    // The lot car shows its wear.
    assert!(v.damage.health() < 1.0 || !v.damage.detached_groups().is_empty());

    // With a car of your own, you arrive in it.
    let (dl, d) = find(&mut c, LotKind::Dealership, |_| true);
    let uid = c.buy_listing(&dl, d.slot).unwrap();
    c.set_active(uid).unwrap();
    let (s, visit) = c.visit_lot(&dl).unwrap();
    let own = visit.own_car.expect("arrived in the active car");
    assert_eq!(s.entities()[0].0, own);
    assert!(
        visit.cars.iter().all(|v| v.slot != d.slot),
        "the bought car left the lot"
    );
}

#[test]
fn rep_tier_class_locks_apply_at_the_lots_too() {
    let mut c = rich(DamageMode::Arcade);
    let (lot, l) = find(&mut c, LotKind::Dealership, |l| {
        l.class == wp_contracts::VehicleClass::Exotic
    });
    c.profile_mut().account.rep = 0;
    let view = c.lot_view(&lot).unwrap();
    let shown = view.cars.iter().find(|x| x.slot == l.slot).unwrap();
    assert!(shown.locked, "an Exotic needs a higher rep tier");
    let cash = c.profile().account.cash;
    assert!(c.buy_listing(&lot, l.slot).is_err());
    assert_eq!(
        c.profile().account.cash,
        cash,
        "a refused purchase takes nothing"
    );
    c.profile_mut().account.rep = 20_000;
    assert!(!c.lot_view(&lot).unwrap().cars.iter().any(|x| x.locked));
    c.buy_listing(&lot, l.slot).unwrap();
}
