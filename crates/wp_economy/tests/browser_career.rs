//! Economy sanity for the browser career (step 2): a new player can afford a beater and a few
//! runs, and chases are the fast money with real risk. Numbers are printed with
//! `--nocapture` and recorded in `docs/integration/economy.md`.

use wp_contracts::{DamageMode, DayPhase, PlayerId, VehicleClass};
use wp_economy::capture::CaptureContext;
use wp_economy::{CarSpec, Condition, Economy, ViolationCounts, WantedLevel};
use wp_math::Rng;

fn econ() -> Economy {
    Economy::embedded()
}

fn beater(e: &Economy) -> CarSpec {
    let f = &e.market.floor_car;
    CarSpec {
        def_id: f.def_id.clone(),
        class: f.class,
        base_price: f.base_price,
    }
}

/// A typical night's offences: a few speeding tickets and one red light.
fn night_violations() -> ViolationCounts {
    ViolationCounts {
        speeding: 4,
        red_light: 1,
        ..Default::default()
    }
}

/// Cash a bust costs on the night (fine + bail + pound fee to get the car back the next day).
fn bust_cost(e: &Economy, spec: &CarSpec, level: WantedLevel, mode: DamageMode) -> i64 {
    let (mut a, _) = e.open_account(PlayerId(1));
    a.cash = 1_000_000;
    let (car, _) = e.buy_car(&mut a, spec, Condition::Used).unwrap();
    let before = a.cash;
    let ctx = CaptureContext {
        mode,
        wanted: level,
        car,
        total_loss: false,
        session_violations: night_violations(),
    };
    e.handle_capture(&mut a, &ctx, &mut Rng::new(7)).unwrap();
    e.end_of_day(&mut a).unwrap();
    let fee = e.retrieval_fee(&a, car).unwrap_or(0);
    before - a.cash + fee
}

fn escape_cash(e: &Economy, level: WantedLevel, phase: DayPhase) -> i64 {
    let (mut a, _) = e.open_account(PlayerId(1));
    let before = a.cash;
    let pb = e.stakes.day_phase[&phase].bounty;
    e.handle_escape(&mut a, level, pb).unwrap();
    a.cash - before
}

#[test]
fn a_new_player_affords_a_beater_and_a_few_runs() {
    let e = econ();
    let (mut a, _) = e.open_account(PlayerId(1));
    let start = a.cash;
    let spec = beater(&e);
    let price = e.market_price(&spec, Condition::Worn);
    let (_, _) = e.buy_car(&mut a, &spec, Condition::Worn).unwrap();
    let left = a.cash;
    let l1 = bust_cost(&e, &spec, WantedLevel::L1, DamageMode::Arcade);
    let l2 = bust_cost(&e, &spec, WantedLevel::L2, DamageMode::Arcade);
    println!(
        "start {start}, worn beater {price}, left {left}; Arcade beater busts: L1 {l1}, L2 {l2}"
    );
    assert!(
        price * 2 <= start,
        "the beater takes at most half the start cash"
    );
    // Two L1 busts plus a third run's bust at L2 before the money runs out.
    assert!(left >= 2 * l1 + l2, "left {left} vs busts {l1}/{l2}");
    // Even broke, the floor rule hands out a car (garage tests cover the beater itself).
    assert!(e.market.floor_car.price <= price);
}

#[test]
fn chases_are_the_fast_money_with_real_risk() {
    let e = econ();
    let spec = beater(&e);
    let sport = CarSpec {
        def_id: "kestrel_ts".into(),
        class: VehicleClass::Sport,
        base_price: 22_000,
    };
    let night = DayPhase::Night;
    let esc: Vec<i64> = [WantedLevel::L1, WantedLevel::L2, WantedLevel::L3]
        .iter()
        .map(|l| escape_cash(&e, *l, night))
        .collect();
    let bust_beater_l1 = bust_cost(&e, &spec, WantedLevel::L1, DamageMode::Arcade);
    let bust_sport_l2 = bust_cost(&e, &sport, WantedLevel::L2, DamageMode::Arcade);
    let bust_sport_l3 = bust_cost(&e, &sport, WantedLevel::L3, DamageMode::Arcade);
    let bust_sport_l3_sim = bust_cost(&e, &sport, WantedLevel::L3, DamageMode::Simulation);
    println!(
        "night escape cash L1/L2/L3 {esc:?}; Arcade busts: beater L1 {bust_beater_l1}, \
         sport L2 {bust_sport_l2}, sport L3 {bust_sport_l3}; Simulation sport L3 {bust_sport_l3_sim}"
    );
    // Fast money: one L2 night escape pays more than an L1 bust costs in a beater, and an
    // escape is worth more the hotter it was.
    assert!(esc[1] > bust_beater_l1);
    assert!(esc[0] < esc[1] && esc[1] < esc[2]);
    // Real risk: at the same level, a bust in a real car costs more than the escape pays (so
    // a 50/50 chase loses money), and Simulation punishes harder than Arcade.
    assert!(bust_sport_l2 > esc[1], "{bust_sport_l2} vs {}", esc[1]);
    assert!(bust_sport_l3 > esc[2], "{bust_sport_l3} vs {}", esc[2]);
    assert!(bust_sport_l3_sim > bust_sport_l3);
    // Daytime escapes pay less than night ones.
    assert!(escape_cash(&e, WantedLevel::L2, DayPhase::Day) < esc[1]);
}
