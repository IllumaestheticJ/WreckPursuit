//! Resale can never be farmed (lead round 12b, owner playtest: "I can buy a used car and sell
//! it for more").
//!
//! For every roster car × condition × random mod set: buying and selling at once (or
//! scrapping) returns less than was spent, and so does buy → crash → repair → sell, with or
//! without insurance. The floor car sells for less than it cost, whatever the player paid.
//! Insurance never pays more than the repair bill.

mod common;

use common::*;
use proptest::prelude::*;
use wp_contracts::{
    load_car, DamageMode, HandlingEffects, LoadedCar, PlayerId, RepairItem, VehicleClass,
};
use wp_economy::data::{ALL_COMPONENTS, ALL_REGIONS};
use wp_economy::repair::{reference_estimate, DamageReport};
use wp_economy::{
    CarSpec, CarUid, Condition, Economy, PlayerAccount, RepairOption, ShopContext, ShopKind,
};
use wp_math::Rng;

fn is_police(c: VehicleClass) -> bool {
    matches!(
        c,
        VehicleClass::PoliceCruiser | VehicleClass::PoliceInterceptor | VehicleClass::PoliceSuv
    )
}

/// Every street car the dealer could sell (no debug cars, no police), sorted by id.
fn roster() -> &'static [LoadedCar] {
    static CARS: std::sync::OnceLock<Vec<LoadedCar>> = std::sync::OnceLock::new();
    CARS.get_or_init(|| {
        let mut cars: Vec<LoadedCar> = std::fs::read_dir(repo().join("data/cars"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .filter(|p| {
                !p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("debug_")
            })
            .map(|p| load_car(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())))
            .filter(|c| !is_police(c.def.class))
            .collect();
        cars.sort_by(|a, b| a.def.id.cmp(&b.def.id));
        cars
    })
}

/// A rich account, so every purchase and repair is affordable.
fn rich(e: &Economy) -> PlayerAccount {
    let (mut a, _) = e.open_account(PlayerId(9));
    e.adjust(&mut a, 50_000_000, "test").unwrap();
    a
}

/// Buy `car` in `cond` and try to install up to `n_mods` random mods (illegal picks are
/// skipped). Returns the uid; the caller measures spending from the cash change.
fn buy_and_mod(
    e: &Economy,
    a: &mut PlayerAccount,
    car: &LoadedCar,
    cond: Condition,
    seed: u64,
    n_mods: usize,
    mode: DamageMode,
) -> CarUid {
    let spec = CarSpec::from(&car.def);
    let (uid, _) = e.buy_car(a, &spec, cond).unwrap();
    let allowed: Vec<String> = e
        .catalog
        .available_for(car.def.class, Some(mode))
        .map(|m| m.id.clone())
        .collect();
    let mut rng = Rng::new(seed);
    for _ in 0..n_mods {
        if allowed.is_empty() {
            break;
        }
        let id = &allowed[rng.below(allowed.len() as u32) as usize];
        let _ = e.install_mod(a, uid, id, mode);
    }
    uid
}

/// Damage the car with random severities on every repair item.
fn crash(e: &Economy, a: &mut PlayerAccount, uid: CarUid, seed: u64, mode: DamageMode) {
    let mut rng = Rng::new(seed ^ 0x5eed);
    let items: Vec<(RepairItem, f32)> = ALL_REGIONS
        .iter()
        .map(|r| RepairItem::Region(*r))
        .chain(ALL_COMPONENTS.iter().map(|c| RepairItem::Component(*c)))
        .map(|it| (it, rng.next_f32()))
        .collect();
    let table = e.price_table_for(a.car(uid).unwrap());
    let est = reference_estimate(&table, e.mode_table(mode).repair_cost_scale, &items);
    let report = DamageReport {
        car: uid,
        health: 0.2 + 0.6 * rng.next_f32(),
        total_loss: false,
        handling: HandlingEffects::NEUTRAL,
        estimate: est,
        lattice_state: Vec::new(),
        wanted_during_session: false,
        towed: false,
    };
    e.record_damage(a, &report).unwrap();
}

fn mode_of(sim: bool) -> DamageMode {
    if sim {
        DamageMode::Simulation
    } else {
        DamageMode::Arcade
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 192, ..ProptestConfig::default() })]

    /// Buy → (mods) → sell at once, or scrap at once: never more than was spent.
    #[test]
    fn buy_then_sell_or_scrap_never_profits(
        car_i in 0usize..64, cond_i in 0usize..3, seed in any::<u64>(), n_mods in 0usize..8,
        sim in any::<bool>(), scrap in any::<bool>(),
    ) {
        let e = econ();
        let cars = roster();
        let car = &cars[car_i % cars.len()];
        let mode = mode_of(sim);
        let mut a = rich(&e);
        let cash0 = a.cash;
        let uid = buy_and_mod(&e, &mut a, car, Condition::ALL[cond_i], seed, n_mods, mode);
        let spent = cash0 - a.cash;
        let back = if scrap {
            e.repair(&mut a, uid, &RepairOption::Scrap, &ShopContext::legit_default(), mode)
                .unwrap();
            a.cash - (cash0 - spent)
        } else {
            e.sell_car(&mut a, uid).unwrap().0
        };
        prop_assert!(back < spent, "{} {:?}: spent {spent}, got back {back}", car.def.id, Condition::ALL[cond_i]);
        prop_assert!(a.cash < cash0);
    }

    /// Buy → (mods, insurance) → crash → repair → sell: never more than everything spent.
    /// Insurance never pays more than the bill.
    #[test]
    fn buy_crash_repair_sell_never_profits(
        car_i in 0usize..64, cond_i in 0usize..3, seed in any::<u64>(), n_mods in 0usize..6,
        sim in any::<bool>(), tier in 0usize..3, crooked in any::<bool>(),
    ) {
        let e = econ();
        let cars = roster();
        let car = &cars[car_i % cars.len()];
        let mode = mode_of(sim);
        let mut a = rich(&e);
        let cash0 = a.cash;
        let uid = buy_and_mod(&e, &mut a, car, Condition::ALL[cond_i], seed, n_mods, mode);
        if tier > 0 {
            e.buy_insurance(&mut a, uid, ["basic", "full"][tier - 1]).unwrap();
        }
        crash(&e, &mut a, uid, seed, mode);
        let shop = ShopContext {
            kind: if crooked { ShopKind::Crooked } else { ShopKind::Legit },
            ..ShopContext::legit_default()
        };
        let q = e.quote_repair(&a, uid, &RepairOption::RepairAll, &shop, mode).unwrap();
        prop_assert!(q.gross > 0);
        prop_assert!(q.insurance_covers < q.gross, "payout {} >= bill {}", q.insurance_covers, q.gross);
        prop_assert!(q.player_pays > 0);
        e.repair(&mut a, uid, &RepairOption::RepairAll, &shop, mode).unwrap();
        let spent = cash0 - a.cash;
        let back = e.sell_car(&mut a, uid).unwrap().0;
        prop_assert!(back < spent, "{}: spent {spent} (incl. repair), got back {back}", car.def.id);
    }

    /// The floor car never sells (or scraps) for more than the player paid for it, whatever
    /// cash they had.
    #[test]
    fn floor_car_never_profits(cash in 0i64..20_000, scrap in any::<bool>()) {
        let e = econ();
        let (mut a, _) = e.open_account(PlayerId(4));
        let delta = cash - a.cash;
        e.adjust(&mut a, delta, "test").unwrap();
        let (uid, _) = e.issue_floor_car(&mut a).unwrap();
        let paid = cash - a.cash;
        prop_assert!(paid <= e.market.floor_car.price);
        let back = if scrap {
            let before = a.cash;
            e.repair(&mut a, uid, &RepairOption::Scrap, &ShopContext::legit_default(), DamageMode::Arcade)
                .unwrap();
            a.cash - before
        } else {
            e.sell_car(&mut a, uid).unwrap().0
        };
        prop_assert!(back <= paid, "floor car paid {paid}, got back {back}");
        prop_assert!(a.cash <= cash);
    }
}

/// The owner's two repro cases, spelled out.
#[test]
fn owner_repro_cases_are_fixed() {
    let e = econ();
    // Worn car bought at 0.6 × base no longer sells at 0.7 × base.
    for car in roster() {
        let spec = CarSpec::from(&car.def);
        for cond in Condition::ALL {
            let mut a = rich(&e);
            let price = e.market_price(&spec, cond);
            let (uid, _) = e.buy_car(&mut a, &spec, cond).unwrap();
            let sale = e.sale_price(a.car(uid).unwrap());
            assert!(
                sale < price,
                "{} {cond:?}: price {price}, sale {sale}",
                car.def.id
            );
        }
    }
    // Floor car: price 1 500, base 3 000 — used to sell for 2 100.
    let (mut a, _) = e.open_account(PlayerId(5));
    let (uid, _) = e.issue_floor_car(&mut a).unwrap();
    let sale = e.sale_price(a.car(uid).unwrap());
    assert!(
        sale <= e.market.floor_car.price,
        "floor car sells for {sale}"
    );
}

/// A car from an old save (no recorded price) is valued as if bought at the cheapest price.
#[test]
fn old_save_cars_use_the_cheapest_price() {
    let e = econ();
    for car in roster() {
        let spec = CarSpec::from(&car.def);
        let mut a = rich(&e);
        let (uid, _) = e.buy_car(&mut a, &spec, Condition::New).unwrap();
        let c = a.car_mut(uid).unwrap();
        c.purchase_price = None;
        let cheapest = Condition::ALL
            .iter()
            .map(|cond| e.market_price(&spec, *cond))
            .min()
            .unwrap();
        let cheapest = if car.def.id == e.market.floor_car.def_id {
            cheapest.min(e.market.floor_car.price)
        } else {
            cheapest
        };
        let c = a.car(uid).unwrap();
        assert_eq!(e.resale_basis(c), cheapest);
        assert!(e.sale_price(c) < cheapest);
        assert!(e.scrap_value(c) < cheapest);
    }
    let floor = &e.market.floor_car;
    let (mut a, _) = e.open_account(PlayerId(6));
    let (uid, _) = e.issue_floor_car(&mut a).unwrap();
    a.car_mut(uid).unwrap().purchase_price = None;
    let c = a.car(uid).unwrap();
    assert!(e.resale_basis(c) <= floor.price);
    assert!(e.sale_price(c) <= floor.price);
}

/// Every insurance tier pays strictly less than the bill for any bill size.
#[test]
fn insurance_payout_is_always_below_the_bill() {
    let e = econ();
    for (name, t) in &e.prices.insurance {
        assert!(
            t.coverage < 1.0 && t.coverage >= 0.0,
            "{name}: coverage {}",
            t.coverage
        );
        assert!(t.deductible > 0, "{name}: deductible {}", t.deductible);
        for gross in [1i64, 100, 250, 251, 500, 501, 1_000, 10_000, 1_000_000] {
            let covers = (t.coverage * (gross - t.deductible).max(0) as f32).round() as i64;
            assert!(covers < gross, "{name}: bill {gross} pays {covers}");
        }
    }
}
