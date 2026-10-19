//! Shared test helpers.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::OnceLock;

use wp_contracts::{load_car, LoadedCar, PlayerId};
use wp_economy::{CarSpec, CarUid, Condition, Economy, PlayerAccount};

pub fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn econ() -> Economy {
    Economy::embedded()
}

pub fn sedan() -> LoadedCar {
    static CAR: OnceLock<LoadedCar> = OnceLock::new();
    CAR.get_or_init(|| load_car(repo().join("data/cars/debug_sedan.json")).unwrap())
        .clone()
}

pub fn coupe() -> LoadedCar {
    static CAR: OnceLock<LoadedCar> = OnceLock::new();
    CAR.get_or_init(|| load_car(repo().join("data/cars/debug_coupe.json")).unwrap())
        .clone()
}

/// Account with `cash` and one new debug sedan.
pub fn account_with_sedan(e: &Economy, cash: i64) -> (PlayerAccount, CarUid) {
    let (mut a, _) = e.open_account(PlayerId(7));
    static SPEC: OnceLock<CarSpec> = OnceLock::new();
    let spec = SPEC.get_or_init(|| CarSpec::from(&sedan().def));
    let price = e.market_price(spec, Condition::New);
    e.adjust(&mut a, price, "test").unwrap();
    let (uid, _) = e.buy_car(&mut a, spec, Condition::New).unwrap();
    let delta = cash - a.cash;
    e.adjust(&mut a, delta, "test").unwrap();
    (a, uid)
}
