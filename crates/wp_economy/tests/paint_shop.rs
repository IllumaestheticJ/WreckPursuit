//! Paint shop: a respray saves the colour on the car and costs `Economy::respray_fee`.

mod common;

use common::*;
use wp_economy::EconomyError;

#[test]
fn a_respray_saves_the_colour_and_charges_the_fee() {
    let e = econ();
    let (mut a, car) = account_with_sedan(&e, 50_000);
    let fee = e.respray_fee(a.car(car).unwrap());
    assert!(fee > 0);
    let cash = a.cash;
    let (paid, _) = e.respray(&mut a, car, 0x1f8a4c).unwrap();
    assert_eq!(paid, fee);
    assert_eq!(a.cash, cash - fee);
    assert_eq!(a.car(car).unwrap().paint, Some(0x1f8a4c));
}

#[test]
fn the_same_colour_again_or_no_money_changes_nothing() {
    let e = econ();
    let (mut a, car) = account_with_sedan(&e, 50_000);
    e.respray(&mut a, car, 0x1f8a4c).unwrap();
    let before = a.clone();
    assert!(e.respray(&mut a, car, 0x1f8a4c).is_err());
    assert!(e.respray(&mut a, car, 0x0100_0000).is_err());
    a.cash = 1;
    let poor = a.clone();
    assert!(matches!(
        e.respray(&mut a, car, 0x222222),
        Err(EconomyError::InsufficientFunds { .. })
    ));
    assert_eq!(a, poor);
    assert_eq!(before.car(car), a.car(car));
}
