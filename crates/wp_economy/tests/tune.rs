//! The free per-car tune: stored on the car, costs nothing, survives the career save.

use wp_contracts::{DamageMode, PlayerId};
use wp_economy::capture::CaptureContext;
use wp_economy::{
    CarSpec, CarTune, CareerProfile, Condition, Economy, EconomyError, ViolationCounts, WantedLevel,
};
use wp_math::Rng;

fn beater(e: &Economy) -> CarSpec {
    let f = &e.market.floor_car;
    CarSpec {
        def_id: f.def_id.clone(),
        class: f.class,
        base_price: f.base_price,
    }
}

const OWNER: CarTune = CarTune {
    suspension: 5,
    gearing: 5,
    diff: 4,
    brake_balance: 2,
};

#[test]
fn setting_a_tune_is_free_and_writes_no_ledger_line() {
    let e = Economy::embedded();
    let (mut a, _) = e.open_account(PlayerId(1));
    let (car, _) = e.buy_car(&mut a, &beater(&e), Condition::Used).unwrap();
    assert_eq!(a.try_car(car).unwrap().tune, CarTune::STOCK);
    let (cash, seq) = (a.cash, a.next_event_seq);
    e.set_tune(&mut a, car, OWNER).unwrap();
    assert_eq!(a.try_car(car).unwrap().tune, OWNER);
    assert_eq!((a.cash, a.next_event_seq), (cash, seq));
    e.set_tune(&mut a, car, CarTune::STOCK).unwrap();
    assert!(a.try_car(car).unwrap().tune.is_stock());
}

#[test]
fn invalid_steps_unknown_cars_and_impounded_cars_are_refused() {
    let e = Economy::embedded();
    let (mut a, _) = e.open_account(PlayerId(1));
    a.cash = 1_000_000;
    let (car, _) = e.buy_car(&mut a, &beater(&e), Condition::Used).unwrap();
    for bad in [0u8, 6] {
        let t = CarTune {
            gearing: bad,
            ..CarTune::STOCK
        };
        assert!(matches!(
            e.set_tune(&mut a, car, t),
            Err(EconomyError::InvalidInput(_))
        ));
    }
    assert!(matches!(
        e.set_tune(&mut a, wp_economy::CarUid(99), OWNER),
        Err(EconomyError::UnknownCar(_))
    ));
    let ctx = CaptureContext {
        mode: DamageMode::Arcade,
        wanted: WantedLevel::L2,
        car,
        total_loss: false,
        session_violations: ViolationCounts::default(),
    };
    e.handle_capture(&mut a, &ctx, &mut Rng::new(7)).unwrap();
    assert!(matches!(
        e.set_tune(&mut a, car, OWNER),
        Err(EconomyError::CarUnavailable { .. })
    ));
    assert!(a.try_car(car).unwrap().tune.is_stock());
}

#[test]
fn the_career_save_keeps_the_tune_and_old_saves_read_as_stock() {
    let e = Economy::embedded();
    let mut p = e.new_career(PlayerId(1), DamageMode::Arcade);
    let (car, _) = e
        .buy_car(&mut p.account, &beater(&e), Condition::Used)
        .unwrap();
    let stock_json = p.to_json();
    assert!(
        !stock_json.contains("\"tune\""),
        "a stock tune is not written"
    );
    e.set_tune(&mut p.account, car, OWNER).unwrap();
    let back = CareerProfile::from_json(&p.to_json()).unwrap();
    assert_eq!(back.account.try_car(car).unwrap().tune, OWNER);
    let old = CareerProfile::from_json(&stock_json).unwrap();
    assert_eq!(old.account.try_car(car).unwrap().tune, CarTune::STOCK);
}
