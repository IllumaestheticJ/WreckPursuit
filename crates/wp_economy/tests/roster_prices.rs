//! Economy prices checked against the real car roster (`data/cars/*.json`, workstream 09).
//!
//! Prints the tuning table with `cargo test -p wp_economy --test roster_prices -- --nocapture`
//! (numbers are recorded in `crates/wp_economy/README.md`). The assertions are the feel
//! targets from `data/economy/prices.toml` and `market.toml`, applied to every roster car.

mod common;

use common::*;
use wp_contracts::{
    load_car, BodyRegion, Component, DamageMode, DayPhase, LoadedCar, RepairItem, VehicleClass,
};
use wp_economy::repair::reference_estimate;
use wp_economy::{CarSpec, Condition, Economy, RaceSetup, RaceType};

fn is_police(c: VehicleClass) -> bool {
    matches!(
        c,
        VehicleClass::PoliceCruiser | VehicleClass::PoliceInterceptor | VehicleClass::PoliceSuv
    )
}

/// Every car file except the `debug_*` test cars, sorted by id.
fn roster() -> Vec<LoadedCar> {
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
        .collect();
    cars.sort_by(|a, b| a.def.id.cmp(&b.def.id));
    cars
}

/// Above this mass a vehicle is a heavy commercial one (no race-purse feel target).
const HEAVY_COMMERCIAL_KG: f32 = 4500.0;

struct Row {
    id: String,
    class: VehicleClass,
    value: i64,
    pi: u16,
    wreck: f32,
    front_hit: f32,
    scrape: f32,
    purse: i64,
    buy_in: i64,
    impound: i64,
    premium: f32,
}

fn row(e: &Economy, car: &LoadedCar, mode: DamageMode) -> Row {
    use RepairItem::{Component as C, Region as R};
    let spec = CarSpec::from(&car.def);
    let value = e.market_price(&spec, Condition::New);
    let table = e.price_table(car.def.class, value);
    let scale = e.mode_table(mode).repair_cost_scale;
    let everything: Vec<(RepairItem, f32)> = table
        .region_replace_cost
        .keys()
        .map(|r| (R(*r), 1.0))
        .chain(table.component_replace_cost.keys().map(|c| (C(*c), 1.0)))
        .collect();
    let front = [
        (R(BodyRegion::FrontBumper), 0.8),
        (R(BodyRegion::Hood), 0.6),
        (R(BodyRegion::FrontLeftFender), 0.5),
        (R(BodyRegion::FrontRightFender), 0.4),
        (C(Component::Radiator), 0.5),
        (C(Component::Lights), 0.7),
        (R(BodyRegion::FrameFront), 0.2),
    ];
    let setup = RaceSetup {
        race_type: RaceType::Sprint,
        kind: None,
        legality: Default::default(),
        class: car.def.class,
        route_length_m: 5_000.0,
        phase: DayPhase::Night,
        mode,
    };
    let basic = &e.prices.insurance["basic"];
    Row {
        id: car.def.id.clone(),
        class: car.def.class,
        value,
        pi: e.class_index(&car.def),
        wreck: reference_estimate(&table, scale, &everything).total,
        front_hit: reference_estimate(&table, scale, &front).total,
        scrape: reference_estimate(&table, scale, &[(R(BodyRegion::FrontBumper), 0.25)]).total,
        purse: e.base_purse(&setup),
        buy_in: e.entry_fee(&setup),
        impound: e.consequences.impound.base_fee[&car.def.class],
        premium: basic.premium_per_day_frac * value as f32,
    }
}

#[test]
fn roster_table_and_feel_targets() {
    let e = econ();
    let cars = roster();
    assert!(
        cars.len() >= 13,
        "expected the full roster, got {}",
        cars.len()
    );
    for mode in [DamageMode::Simulation, DamageMode::Arcade] {
        println!("\n{mode:?}");
        println!(
            "{:20} {:17} {:>7} {:>4} {:>9} {:>9} {:>6} {:>6} {:>7} {:>7} {:>6} {:>6}",
            "car",
            "class",
            "value",
            "PI",
            "wreck/val",
            "front",
            "/purse",
            "scrape",
            "purse",
            "buy-in",
            "pound",
            "ins/d"
        );
        for car in &cars {
            let r = row(&e, car, mode);
            println!(
                "{:20} {:17} {:>7} {:>4} {:>9.2} {:>9.0} {:>6.2} {:>6.0} {:>7} {:>7} {:>6} {:>6.0}",
                r.id,
                format!("{:?}", r.class),
                r.value,
                r.pi,
                r.wreck / r.value as f32,
                r.front_hit,
                r.front_hit / r.purse as f32,
                r.scrape,
                r.purse,
                r.buy_in,
                r.impound,
                r.premium
            );
            // A full wreck is never cheap to fix (prices.toml hard floor).
            assert!(
                r.wreck >= 0.6 * r.value as f32,
                "{}: full wreck {:.0} < 60% of value {}",
                r.id,
                r.wreck,
                r.value
            );
            if is_police(r.class) {
                continue;
            }
            // A bumper scrape costs less than a buy-in.
            assert!(
                r.scrape < r.buy_in as f32,
                "{}: scrape {:.0} >= buy-in {}",
                r.id,
                r.scrape,
                r.buy_in
            );
            // A medium front-end hit costs about one race win in Simulation. Arcade halves the
            // bill but also pays 1/1.2 of the Simulation purse, so its ratio is 0.6x.
            // Heavy commercial vehicles (box truck, tractor, tipper, armoured truck) are not
            // race cars: their repair bills are meant to dwarf a purse. They stay under Truck
            // until the requested HeavyTruck class exists (docs/integration/cars.md, round 9).
            if car.def.mass_kg > HEAVY_COMMERCIAL_KG {
                continue;
            }
            let ratio = r.front_hit / r.purse as f32;
            let (lo, hi) = match mode {
                DamageMode::Simulation => (0.5, 1.6),
                DamageMode::Arcade => (0.3, 1.0),
            };
            assert!(
                (lo..=hi).contains(&ratio),
                "{} {mode:?}: front hit / win purse = {ratio:.2}, want {lo}..{hi}",
                r.id
            );
            // The pound costs a noticeable but not ruinous slice of the car.
            let pound = r.impound as f32 / r.value as f32;
            assert!(
                (0.01..=0.08).contains(&pound),
                "{}: pound base fee is {:.1}% of value",
                r.id,
                100.0 * pound
            );
        }
    }
}

#[test]
fn roster_prices_sit_inside_their_class_bands() {
    let e = econ();
    for car in roster() {
        if is_police(car.def.class) {
            continue;
        }
        let spec = CarSpec::from(&car.def);
        let [lo, hi] = e.market.class_bands[&spec.class];
        for cond in Condition::ALL {
            let raw = (spec.base_price as f32 * e.market.condition[&cond]).round() as i64;
            assert!(
                (lo..=hi).contains(&raw),
                "{} {cond:?}: {raw} is outside the {:?} band [{lo}, {hi}] (would be clamped)",
                spec.def_id,
                spec.class
            );
            assert_eq!(e.market_price(&spec, cond), raw);
        }
    }
}

#[test]
fn floor_car_is_in_the_roster_with_matching_data() {
    let e = econ();
    let floor = &e.market.floor_car;
    let car = roster()
        .into_iter()
        .find(|c| c.def.id == floor.def_id)
        .expect("floor car file exists");
    assert_eq!(car.def.class, floor.class);
    assert_eq!(car.def.base_price, floor.base_price);
    assert!(floor.price <= e.market.starting_cash);
}

#[test]
fn pricier_cars_are_not_slower() {
    // Value for money across the street-racing classes (trucks are utility, police not for
    // sale): a car that costs 25% more than another must not have a lower class index. Known
    // outliers are listed with the request to 09 in docs/integration/economy.md; drop them from
    // KNOWN once 09 retunes the price (the test then keeps them honest).
    const KNOWN: &[&str] = &["hakone_gt", "ironside_440"];
    let e = econ();
    let cars: Vec<_> = roster()
        .into_iter()
        .filter(|c| !is_police(c.def.class) && c.def.class != VehicleClass::Truck)
        .collect();
    let mut outliers = Vec::new();
    for a in &cars {
        for b in &cars {
            if (a.def.base_price as f32) < 1.25 * b.def.base_price as f32 {
                continue;
            }
            let (pa, pb) = (e.class_index(&a.def), e.class_index(&b.def));
            if pa < pb {
                outliers.push(format!(
                    "{} ({}, PI {pa}) vs {} ({}, PI {pb})",
                    a.def.id, a.def.base_price, b.def.id, b.def.base_price
                ));
                assert!(
                    KNOWN.contains(&a.def.id.as_str()),
                    "{} is overpriced for its performance: {}",
                    a.def.id,
                    outliers.last().unwrap()
                );
            }
        }
    }
    println!("known value-for-money outliers: {outliers:#?}");
    for k in KNOWN {
        assert!(
            outliers.iter().any(|o| o.starts_with(k)),
            "{k} is no longer an outlier: remove it from KNOWN"
        );
    }
}
