//! Capture consequences, impound, cop career, race payouts, debt, pink slips.

mod common;

use common::*;
use wp_contracts::{DamageMode, DayPhase, PlayerId, VehicleClass};
use wp_economy::capture::{CaptureContext, CopAction};
use wp_economy::{
    CarFate, CarLocation, EconomyError, EventReason, PinkSlipConsent, RaceResult, RaceSetup,
    RaceType, ViolationCounts, WantedLevel,
};
use wp_math::Rng;

fn ctx(
    mode: DamageMode,
    wanted: WantedLevel,
    car: wp_economy::CarUid,
    total_loss: bool,
) -> CaptureContext {
    CaptureContext {
        mode,
        wanted,
        car,
        total_loss,
        session_violations: ViolationCounts {
            speeding: 4,
            red_light: 1,
            ..Default::default()
        },
    }
}

#[test]
fn arcade_always_impounds() {
    let e = econ();
    let mut rng = Rng::new(1);
    for level in [WantedLevel::L1, WantedLevel::L3, WantedLevel::L5] {
        for total_loss in [false, true] {
            for _ in 0..20 {
                let (mut a, car) = account_with_sedan(&e, 5_000);
                let (out, _) = e
                    .handle_capture(
                        &mut a,
                        &ctx(DamageMode::Arcade, level, car, total_loss),
                        &mut rng,
                    )
                    .unwrap();
                assert_eq!(out.car_fate, CarFate::Impounded);
                assert!(matches!(
                    a.car(car).unwrap().location,
                    CarLocation::Impound { .. }
                ));
                assert!(out.fine > 0);
            }
        }
    }
}

#[test]
fn simulation_seizes_about_30_percent_at_level_4_and_5() {
    let e = econ();
    let mut rng = Rng::new(42);
    let n = 2000;
    let mut seized = 0;
    for i in 0..n {
        let level = if i % 2 == 0 {
            WantedLevel::L4
        } else {
            WantedLevel::L5
        };
        let (mut a, car) = account_with_sedan(&e, 5_000);
        let (out, _) = e
            .handle_capture(
                &mut a,
                &ctx(DamageMode::Simulation, level, car, false),
                &mut rng,
            )
            .unwrap();
        if out.car_fate == CarFate::Seized {
            seized += 1;
            assert!(a.car(car).is_none());
        }
    }
    let frac = seized as f32 / n as f32;
    assert!((0.26..0.34).contains(&frac), "seizure rate {frac}");
    // Never below level 4.
    for _ in 0..200 {
        let (mut a, car) = account_with_sedan(&e, 5_000);
        let (out, _) = e
            .handle_capture(
                &mut a,
                &ctx(DamageMode::Simulation, WantedLevel::L3, car, false),
                &mut rng,
            )
            .unwrap();
        assert_eq!(out.car_fate, CarFate::Impounded);
    }
}

#[test]
fn simulation_total_loss_at_capture_means_car_is_gone() {
    let e = econ();
    let (mut a, car) = account_with_sedan(&e, 5_000);
    let (out, ev) = e
        .handle_capture(
            &mut a,
            &ctx(DamageMode::Simulation, WantedLevel::L1, car, true),
            &mut Rng::new(3),
        )
        .unwrap();
    assert_eq!(out.car_fate, CarFate::Destroyed);
    assert!(a.garage.is_empty());
    assert!(ev
        .iter()
        .any(|x| matches!(x.reason, EventReason::CarDestroyed { .. })));
    assert_eq!(a.stats.cars_lost, 1);
}

#[test]
fn fines_scale_with_level_history_and_mode_and_can_create_debt() {
    let e = econ();
    let (a, car) = account_with_sedan(&e, 0);
    let f = |a: &wp_economy::PlayerAccount, mode, level| {
        e.capture_fine(a, &ctx(mode, level, car, false))
    };
    let mut prev = 0;
    for level in [
        WantedLevel::L1,
        WantedLevel::L2,
        WantedLevel::L3,
        WantedLevel::L4,
        WantedLevel::L5,
    ] {
        let fine = f(&a, DamageMode::Simulation, level);
        assert!(fine > prev);
        prev = fine;
        assert!(f(&a, DamageMode::Arcade, level) < fine);
    }
    let mut repeat = a.clone();
    repeat.heat_history.captures = 3;
    assert!(
        f(&repeat, DamageMode::Arcade, WantedLevel::L2)
            > f(&a, DamageMode::Arcade, WantedLevel::L2)
    );

    // Broke racer with a bounty: fine + bail push into debt; bounty cleared; rep lost.
    let (mut a, car) = account_with_sedan(&e, 0);
    a.bounty = 2_000;
    a.rep = 1_000;
    let (out, ev) = e
        .handle_capture(
            &mut a,
            &ctx(DamageMode::Arcade, WantedLevel::L3, car, false),
            &mut Rng::new(1),
        )
        .unwrap();
    assert_eq!(a.cash, -(out.fine + out.bail));
    assert_eq!(out.bail, 1_000);
    assert_eq!(a.bounty, 0);
    assert_eq!(a.rep, 1_000 - out.rep_loss);
    assert!(ev.iter().any(|x| x.reason == EventReason::BountyCleared));
    assert_eq!(a.heat_history.captures, 1);
}

#[test]
fn capture_while_wanted_voids_insurance() {
    let e = econ();
    let (mut a, car) = account_with_sedan(&e, 50_000);
    e.buy_insurance(&mut a, car, "basic").unwrap();
    let (_, ev) = e
        .handle_capture(
            &mut a,
            &ctx(DamageMode::Arcade, WantedLevel::L2, car, false),
            &mut Rng::new(1),
        )
        .unwrap();
    assert!(a.car(car).unwrap().insurance.is_none());
    assert!(ev.iter().any(|x| x.reason == EventReason::InsuranceVoided));
    assert!(a.car(car).unwrap().damage.sustained_while_wanted);
}

#[test]
fn impound_fee_grows_daily_then_auction() {
    let e = econ();
    let (mut a, car) = account_with_sedan(&e, 100_000);
    e.handle_capture(
        &mut a,
        &ctx(DamageMode::Arcade, WantedLevel::L1, car, false),
        &mut Rng::new(1),
    )
    .unwrap();
    let fee0 = e.retrieval_fee(&a, car).unwrap();
    e.end_of_day(&mut a).unwrap();
    let fee1 = e.retrieval_fee(&a, car).unwrap();
    assert_eq!(
        fee1 - fee0,
        e.consequences.impound.per_day[&VehicleClass::Compact]
    );
    // Racing an impounded car is refused.
    let setup = sprint(VehicleClass::Compact);
    assert!(e.enter_race(&mut a, &setup, car).is_err());
    // Retrieve.
    let cash = a.cash;
    e.retrieve_from_impound(&mut a, car).unwrap();
    assert_eq!(a.cash, cash - fee1);
    assert_eq!(a.car(car).unwrap().location, CarLocation::Garage);

    // Leave it too long: auctioned.
    e.handle_capture(
        &mut a,
        &ctx(DamageMode::Arcade, WantedLevel::L1, car, false),
        &mut Rng::new(1),
    )
    .unwrap();
    for _ in 0..e.consequences.impound.auction_after_days {
        e.end_of_day(&mut a).unwrap();
    }
    assert!(a.car(car).is_none());
    assert_eq!(a.stats.cars_lost, 1);
}

#[test]
fn escape_grows_bounty_and_rep() {
    let e = econ();
    let (mut a, _) = account_with_sedan(&e, 0);
    e.handle_escape(&mut a, WantedLevel::L3, 1.3).unwrap();
    assert_eq!(a.bounty, (700.0f32 * 1.3).round() as u32);
    assert_eq!(a.rep, 80);
    assert_eq!(a.heat_history.escapes, 1);
}

#[test]
fn cop_career() {
    let e = econ();
    let (mut cop, _) = e.open_account(PlayerId(50));
    let cash0 = cop.cash;
    e.award_cop(
        &mut cop,
        CopAction::Capture {
            level: WantedLevel::L4,
            racer_bounty: 4_000,
        },
    )
    .unwrap();
    assert_eq!(cop.cop_rank, 300);
    assert_eq!(cop.cash, cash0 + 1_000 + 1_000);
    e.award_cop(&mut cop, CopAction::CleanDriving { minutes: 100 })
        .unwrap();
    assert_eq!(cop.cop_rank, 500);
    assert_eq!(e.cop_level(cop.cop_rank), 1);
    e.award_cop(&mut cop, CopAction::CivilianCollision).unwrap();
    e.award_cop(&mut cop, CopAction::SelfWreck).unwrap();
    e.award_cop(&mut cop, CopAction::RiskBudgetOverspent { overspent: 2.0 })
        .unwrap();
    assert_eq!(cop.cop_rank, 500 - 40 - 150 - 120);
    for _ in 0..10 {
        e.award_cop(&mut cop, CopAction::SelfWreck).unwrap();
    }
    assert_eq!(cop.cop_rank, 0, "XP saturates at zero");
    let before = cop.cash;
    e.award_cop(
        &mut cop,
        CopAction::Assist {
            level: WantedLevel::L2,
            racer_bounty: 0,
        },
    )
    .unwrap();
    assert_eq!(cop.cash, before + (350.0f32 * 0.3).round() as i64);
}

fn sprint(class: VehicleClass) -> RaceSetup {
    RaceSetup {
        race_type: RaceType::Sprint,
        kind: None,
        legality: Default::default(),
        class,
        route_length_m: 9_000.0,
        phase: DayPhase::Night,
        mode: DamageMode::Arcade,
    }
}

fn result(placement: u8, contacts: u32, wanted: WantedLevel) -> RaceResult {
    RaceResult {
        placement,
        contacts,
        violations: ViolationCounts {
            speeding: 5,
            ..Default::default()
        },
        wanted,
    }
}

#[test]
fn purse_formula_and_multipliers() {
    let e = econ();
    let s = sprint(VehicleClass::Compact);
    // 230/km × 9 km × Compact 1.0 × Night 1.35 × Arcade 1.0
    assert_eq!(e.base_purse(&s), (230.0f32 * 9.0 * 1.35).round() as i64);
    assert_eq!(
        e.entry_fee(&s),
        (230.0f32 * 9.0 * 0.12 * 1.2).round() as i64
    );
    let win = e.race_payout(&s, &result(1, 1, WantedLevel::None));
    let second = e.race_payout(&s, &result(2, 1, WantedLevel::None));
    assert_eq!(second.purse, (win.purse as f32 * 0.5).round() as i64);
    assert_eq!(win.clean_bonus, 0);
    let clean = e.race_payout(&s, &result(1, 0, WantedLevel::None));
    assert_eq!(
        clean.clean_bonus,
        (clean.purse as f32 * 0.25).round() as i64
    );
    let wanted = e.race_payout(&s, &result(1, 1, WantedLevel::L3));
    assert!(wanted.purse > win.purse && wanted.bounty > 0);
    let dnf = e.race_payout(&s, &result(0, 0, WantedLevel::None));
    assert_eq!(dnf.purse, 0);
    assert!(dnf.rep < 0);
    let fifth = e.race_payout(&s, &result(5, 0, WantedLevel::None));
    assert_eq!(fifth.purse, 0);
    // Day pays less than night; Simulation more than Arcade; Exotic more than Beater.
    let mut day = s.clone();
    day.phase = DayPhase::Day;
    assert!(e.base_purse(&day) < e.base_purse(&s));
    let mut sim = s.clone();
    sim.mode = DamageMode::Simulation;
    assert!(e.base_purse(&sim) > e.base_purse(&s));
    assert!(
        e.base_purse(&sprint(VehicleClass::Exotic)) > e.base_purse(&sprint(VehicleClass::Beater))
    );
    // A red light voids the clean bonus but speeding does not.
    let mut r = result(1, 0, WantedLevel::None);
    assert!(r.is_clean());
    r.violations.red_light = 1;
    assert!(!r.is_clean());
}

/// The scaled variants charge and pay the lobby's advertised stake (R5 scales): the buy-in
/// and the purse (and the clean bonus on it) scale; rep does not.
#[test]
fn scaled_entry_and_purse_match_the_advertised_stake() {
    let e = econ();
    let s = sprint(VehicleClass::Compact);
    let clean = result(1, 0, WantedLevel::None);
    let plain = e.race_payout(&s, &clean);
    let scaled = e.race_payout_scaled(&s, &clean, 2.5);
    assert_eq!(scaled.purse, (plain.purse as f32 * 2.5).round() as i64);
    assert_eq!(
        scaled.clean_bonus,
        (scaled.purse as f32 * 0.25).round() as i64
    );
    assert_eq!(scaled.rep, plain.rep);
    assert_eq!(e.race_payout_scaled(&s, &clean, 1.0), plain);

    let (mut a, car) = account_with_sedan(&e, 50_000);
    let cash = a.cash;
    let (fee, _) = e.enter_race_scaled(&mut a, &s, car, 0.5).unwrap();
    assert_eq!(fee, (e.entry_fee(&s) as f32 * 0.5).round() as i64);
    assert_eq!(a.cash, cash - fee);
    let (pay, _) = e.settle_race_scaled(&mut a, &s, car, &clean, 2.5).unwrap();
    assert_eq!(pay, scaled);
    assert_eq!(a.cash, cash - fee + scaled.purse + scaled.clean_bonus);
}

#[test]
fn debt_caps_race_entry_until_paid() {
    let e = econ();
    let (mut a, car) = account_with_sedan(&e, 0);
    e.handle_capture(
        &mut a,
        &ctx(DamageMode::Arcade, WantedLevel::None, car, false),
        &mut Rng::new(1),
    )
    .unwrap();
    e.retrieve_from_impound(&mut a, car).unwrap_err();
    // Put the car back by hand (the pound let it go) to test entry rules.
    a.car_mut(car).unwrap().location = CarLocation::Garage;
    assert!(a.in_debt());
    let s = sprint(VehicleClass::Compact);
    assert!(matches!(
        e.enter_race(&mut a, &s, car),
        Err(EconomyError::InDebt { .. })
    ));
    let mut meet = s.clone();
    meet.race_type = RaceType::Meet;
    assert_eq!(e.entry_fee(&meet), 0);
    e.enter_race(&mut a, &meet, car).unwrap();
    // Winnings pay the debt down automatically.
    let debt = a.debt();
    e.settle_race(&mut a, &meet, car, &result(1, 0, WantedLevel::None))
        .unwrap();
    assert!(a.debt() < debt);
    // Voluntary purchases are refused while in debt.
    if a.in_debt() {
        assert!(e.buy_informant_tip(&mut a, "whisper").is_err());
    }
}

#[test]
fn pink_slips() {
    let e = econ();
    let (mut w, wcar) = account_with_sedan(&e, 1_000);
    let (mut l, lcar) = account_with_sedan(&e, 1_000);
    w.player = PlayerId(1);
    l.player = PlayerId(2);
    let yes = PinkSlipConsent {
        winner: true,
        loser: true,
    };
    // Arcade: off by default.
    assert!(matches!(
        e.settle_pink_slip(&mut w, wcar, &mut l, lcar, DamageMode::Arcade, yes),
        Err(EconomyError::PinkSlipNotAllowed(_))
    ));
    // Rep requirement.
    assert!(e
        .settle_pink_slip(&mut w, wcar, &mut l, lcar, DamageMode::Simulation, yes)
        .is_err());
    w.rep = 600;
    l.rep = 600;
    // Consent required.
    let no = PinkSlipConsent {
        winner: true,
        loser: false,
    };
    assert!(e
        .settle_pink_slip(&mut w, wcar, &mut l, lcar, DamageMode::Simulation, no)
        .is_err());
    e.install_mod(&mut l, lcar, "engine_intake_1", DamageMode::Simulation)
        .unwrap();
    e.buy_insurance(&mut l, lcar, "basic").unwrap();
    let (uid, wev, lev) = e
        .settle_pink_slip(&mut w, wcar, &mut l, lcar, DamageMode::Simulation, yes)
        .unwrap();
    assert!(l.car(lcar).is_none());
    let won = w.car(uid).unwrap();
    assert_eq!(won.mods, vec!["engine_intake_1".to_string()]);
    assert!(won.insurance.is_none());
    assert_eq!(w.garage.len(), 2);
    assert!(wev.iter().all(|x| x.player == PlayerId(1)));
    assert!(lev.iter().all(|x| x.player == PlayerId(2)));
}

#[test]
fn informant_tip_reports_heat_reduction() {
    let e = econ();
    let (mut a, _) = account_with_sedan(&e, 5_000);
    let (heat, ev) = e.buy_informant_tip(&mut a, "contact").unwrap();
    assert_eq!(heat, 25.0);
    assert_eq!(a.cash, 3_800);
    assert!(matches!(ev[0].reason, EventReason::InformantTip { .. }));
    assert!(e.buy_informant_tip(&mut a, "nope").is_err());
}
