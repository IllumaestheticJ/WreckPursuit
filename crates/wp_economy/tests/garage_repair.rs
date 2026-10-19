//! Garage and repair behaviour.

mod common;

use common::*;
use wp_contracts::{BodyRegion, Component, DamageMode, HandlingEffects, RepairItem};
use wp_economy::repair::{reference_estimate, DamageReport};
use wp_economy::{
    CarLocation, CarSpec, Condition, EconomyError, EventReason, RepairOption, ShopContext,
    ShopKind, WantedLevel,
};

fn damaged_report(
    e: &wp_economy::Economy,
    a: &wp_economy::PlayerAccount,
    car: wp_economy::CarUid,
    mode: DamageMode,
) -> DamageReport {
    let c = a.car(car).unwrap();
    let table = e.price_table_for(c);
    let est = reference_estimate(
        &table,
        e.mode_table(mode).repair_cost_scale,
        &[
            (RepairItem::Region(BodyRegion::FrontBumper), 0.6),
            (RepairItem::Region(BodyRegion::Hood), 0.4),
            (RepairItem::Component(Component::Radiator), 0.5),
            (RepairItem::Component(Component::SuspensionFL), 0.8),
        ],
    );
    let mut handling = HandlingEffects::NEUTRAL;
    handling.toe_delta[0] = 0.01;
    handling.steer_pull = 0.08;
    DamageReport {
        car,
        health: 0.7,
        total_loss: false,
        handling,
        estimate: est,
        lattice_state: vec![9, 9, 9],
        wanted_during_session: false,
        towed: false,
    }
}

#[test]
fn drive_damaged_keeps_lattice_and_handling() {
    let e = econ();
    let (mut a, car) = account_with_sedan(&e, 0);
    let r = damaged_report(&e, &a, car, DamageMode::Arcade);
    e.record_damage(&mut a, &r).unwrap();
    let before = a.car(car).unwrap().damage.clone();
    let (order, ev) = e
        .repair(
            &mut a,
            car,
            &RepairOption::DriveDamaged,
            &ShopContext::legit_default(),
            DamageMode::Arcade,
        )
        .unwrap();
    assert!(order.items.is_empty() && !order.full_reset);
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].cash_delta, 0);
    let after = &a.car(car).unwrap().damage;
    assert_eq!(after, &before);
    assert_eq!(after.lattice_state, vec![9, 9, 9]);
    assert_eq!(after.handling.steer_pull, 0.08);
}

#[test]
fn repair_all_and_selected() {
    let e = econ();
    let mode = DamageMode::Simulation;
    let shop = ShopContext::legit_default();
    let (mut a, car) = account_with_sedan(&e, 50_000);
    let r = damaged_report(&e, &a, car, mode);
    e.record_damage(&mut a, &r).unwrap();

    // Selected: only the suspension.
    let sel = RepairOption::RepairSelected(vec![RepairItem::Component(Component::SuspensionFL)]);
    let q = e.quote_repair(&a, car, &sel, &shop, mode).unwrap();
    assert_eq!(q.lines.len(), 1);
    let cash0 = a.cash;
    let (order, _) = e.repair(&mut a, car, &sel, &shop, mode).unwrap();
    assert_eq!(
        order.items,
        vec![RepairItem::Component(Component::SuspensionFL)]
    );
    assert!(!order.full_reset);
    assert_eq!(a.cash, cash0 - q.player_pays);
    let d = &a.car(car).unwrap().damage;
    assert_eq!(d.outstanding.lines.len(), 3);
    assert!(d.health > 0.7);

    // Unknown item is refused.
    let bad = RepairOption::RepairSelected(vec![RepairItem::Component(Component::Engine)]);
    assert!(matches!(
        e.repair(&mut a, car, &bad, &shop, mode),
        Err(EconomyError::InvalidInput(_))
    ));

    // All: pristine afterwards.
    let (order, _) = e
        .repair(&mut a, car, &RepairOption::RepairAll, &shop, mode)
        .unwrap();
    assert!(order.full_reset);
    assert!(a.car(car).unwrap().damage.is_pristine());
    assert!(matches!(
        e.repair(&mut a, car, &RepairOption::RepairAll, &shop, mode),
        Err(EconomyError::NothingToRepair)
    ));
}

#[test]
fn unaffordable_repair_is_refused_without_side_effects() {
    let e = econ();
    let mode = DamageMode::Simulation;
    let (mut a, car) = account_with_sedan(&e, 10);
    let r = damaged_report(&e, &a, car, mode);
    e.record_damage(&mut a, &r).unwrap();
    let snapshot = a.clone();
    let r = e.repair(
        &mut a,
        car,
        &RepairOption::RepairAll,
        &ShopContext::legit_default(),
        mode,
    );
    assert!(matches!(r, Err(EconomyError::InsufficientFunds { .. })));
    assert_eq!(a, snapshot);
}

#[test]
fn arcade_repairs_cost_half_of_simulation() {
    let e = econ();
    let (mut a, car) = account_with_sedan(&e, 0);
    // Same lines (unscaled estimate) priced in both modes.
    let mut r = damaged_report(&e, &a, car, DamageMode::Simulation);
    r.estimate = reference_estimate(
        &e.price_table_for(a.car(car).unwrap()),
        1.0,
        &[(RepairItem::Region(BodyRegion::Roof), 1.0)],
    );
    e.record_damage(&mut a, &r).unwrap();
    let shop = ShopContext::legit_default();
    let sim = e
        .quote_repair(
            &a,
            car,
            &RepairOption::RepairAll,
            &shop,
            DamageMode::Simulation,
        )
        .unwrap();
    let arc = e
        .quote_repair(&a, car, &RepairOption::RepairAll, &shop, DamageMode::Arcade)
        .unwrap();
    assert!((arc.gross as f32 - 0.5 * sim.gross as f32).abs() <= 1.0);
}

#[test]
fn shops_district_markup_and_reporting() {
    let e = econ();
    let mode = DamageMode::Simulation;
    let (mut a, car) = account_with_sedan(&e, 100_000);
    let r = damaged_report(&e, &a, car, mode);
    e.record_damage(&mut a, &r).unwrap();
    let q = |kind, wanted, district: &str| {
        e.quote_repair(
            &a,
            car,
            &RepairOption::RepairAll,
            &ShopContext {
                district: district.into(),
                kind,
                wanted,
            },
            mode,
        )
        .unwrap()
    };
    let legit = q(ShopKind::Legit, WantedLevel::None, "default");
    let core = q(ShopKind::Legit, WantedLevel::None, "Core");
    let docks = q(ShopKind::Legit, WantedLevel::None, "Docks");
    let crooked = q(ShopKind::Crooked, WantedLevel::None, "default");
    let crooked_wanted = q(ShopKind::Crooked, WantedLevel::L3, "default");
    let legit_wanted = q(ShopKind::Legit, WantedLevel::L3, "default");
    let legit_l1 = q(ShopKind::Legit, WantedLevel::L1, "default");
    assert!(core.gross > legit.gross && docks.gross < legit.gross);
    assert!(crooked.gross > legit.gross && crooked_wanted.gross > crooked.gross);
    assert!(!crooked_wanted.reports_to_police);
    assert!(legit_wanted.reports_to_police);
    assert!(!legit_l1.reports_to_police);
    assert_eq!(legit_wanted.gross, legit.gross);

    let shop = ShopContext {
        district: "Suburb".into(),
        kind: ShopKind::Legit,
        wanted: WantedLevel::L3,
    };
    let (_, ev) = e
        .repair(&mut a, car, &RepairOption::RepairAll, &shop, mode)
        .unwrap();
    assert!(ev
        .iter()
        .any(|x| matches!(x.reason, EventReason::ShopReported { .. })));
}

#[test]
fn scrap_value_formula() {
    let e = econ();
    let (mut a, car) = account_with_sedan(&e, 100_000);
    e.install_mod(&mut a, car, "engine_intake_1", DamageMode::Arcade)
        .unwrap();
    e.install_mod(&mut a, car, "susp_coilovers_2", DamageMode::Arcade)
        .unwrap();
    let mut r = damaged_report(&e, &a, car, DamageMode::Arcade);
    r.health = 0.5;
    e.record_damage(&mut a, &r).unwrap();
    let c = a.car(car).unwrap();
    let s = &e.prices.scrap;
    let expect = 18_000.0 * (s.min_frac + s.health_frac * 0.5) + s.mods_frac * (600.0 + 2800.0);
    assert_eq!(e.scrap_value(c), expect.round() as i64);
    let cash0 = a.cash;
    let (order, _) = e
        .repair(
            &mut a,
            car,
            &RepairOption::Scrap,
            &ShopContext::legit_default(),
            DamageMode::Arcade,
        )
        .unwrap();
    assert!(order.scrapped);
    assert!(a.car(car).is_none());
    assert_eq!(a.cash, cash0 + expect.round() as i64);
}

#[test]
fn towing_is_a_penalty_and_can_create_debt() {
    let e = econ();
    let (mut a, car) = account_with_sedan(&e, 0);
    let mut r = damaged_report(&e, &a, car, DamageMode::Simulation);
    r.towed = true;
    r.total_loss = true;
    let (_, ev) = e.record_damage(&mut a, &r).unwrap();
    assert!(a.cash < 0, "towing should push a broke player into debt");
    assert!(ev.iter().any(|x| x.reason == EventReason::Towing));
    // An undrivable car cannot be driven damaged.
    assert!(e
        .repair(
            &mut a,
            car,
            &RepairOption::DriveDamaged,
            &ShopContext::legit_default(),
            DamageMode::Simulation
        )
        .is_err());
}

#[test]
fn insurance_pays_unless_damage_was_wanted() {
    let e = econ();
    let mode = DamageMode::Simulation;
    let shop = ShopContext::legit_default();
    let (mut a, car) = account_with_sedan(&e, 100_000);
    e.buy_insurance(&mut a, car, "full").unwrap();
    let r = damaged_report(&e, &a, car, mode);
    e.record_damage(&mut a, &r).unwrap();
    let q = e
        .quote_repair(&a, car, &RepairOption::RepairAll, &shop, mode)
        .unwrap();
    let pol = a.car(car).unwrap().insurance.clone().unwrap();
    let expected = (pol.coverage * (q.gross - pol.deductible) as f32).round() as i64;
    assert_eq!(q.insurance_covers, expected);
    assert_eq!(q.player_pays, q.gross - expected);

    // Damage sustained while wanted: claim denied.
    let mut r = damaged_report(&e, &a, car, mode);
    r.wanted_during_session = true;
    e.record_damage(&mut a, &r).unwrap();
    let q = e
        .quote_repair(&a, car, &RepairOption::RepairAll, &shop, mode)
        .unwrap();
    assert!(q.claim_denied);
    assert_eq!(q.insurance_covers, 0);

    // Daily premium is charged; lapses when unaffordable.
    let cash0 = a.cash;
    e.end_of_day(&mut a).unwrap();
    assert_eq!(a.cash, cash0 - pol.premium_per_day);
    let debit = -a.cash;
    e.adjust(&mut a, debit, "drain").unwrap();
    e.end_of_day(&mut a).unwrap();
    assert!(a.car(car).unwrap().insurance.is_none());
    assert_eq!(a.cash, 0);
}

#[test]
fn buy_sell_and_floor_car() {
    let e = econ();
    let spec = CarSpec::from(&coupe().def);
    let (mut a, _) = e.open_account(wp_contracts::PlayerId(3));
    assert!(matches!(
        e.buy_car(&mut a, &spec, Condition::Used),
        Err(EconomyError::InsufficientFunds { .. })
    ));
    // Floor car: always obtainable, never into debt.
    let cash0 = a.cash;
    let (uid, _) = e.issue_floor_car(&mut a).unwrap();
    let floor = &e.market.floor_car;
    assert_eq!(a.cash, cash0 - floor.price.min(cash0));
    assert!(
        e.issue_floor_car(&mut a).is_err(),
        "only when no usable car"
    );
    let sold = e.sell_car(&mut a, uid).unwrap().0;
    assert!(sold > 0 && sold < floor.base_price as i64);
    // Broke player gets it for what they have.
    let debit = -a.cash;
    e.adjust(&mut a, debit, "drain").unwrap();
    e.adjust(&mut a, 300, "pocket money").unwrap();
    e.issue_floor_car(&mut a).unwrap();
    assert_eq!(a.cash, 0);

    // Dealer prices sit in the class band.
    for c in Condition::ALL {
        let p = e.market_price(&spec, c);
        let [lo, hi] = e.market.class_bands[&spec.class];
        assert!(p >= lo && p <= hi);
    }
    // Police cars are not for sale.
    let mut cop = spec.clone();
    cop.class = wp_contracts::VehicleClass::PoliceInterceptor;
    assert!(e.buy_car(&mut a, &cop, Condition::New).is_err());
}

#[test]
fn mod_install_charges_and_replacement_refunds() {
    let e = econ();
    let (mut a, car) = account_with_sedan(&e, 10_000);
    e.install_mod(&mut a, car, "susp_springs_1", DamageMode::Arcade)
        .unwrap();
    assert_eq!(a.cash, 10_000 - 900);
    e.install_mod(&mut a, car, "susp_coilovers_2", DamageMode::Arcade)
        .unwrap();
    let refund = (900.0 * e.market.mod_resale_frac).round() as i64;
    assert_eq!(a.cash, 10_000 - 900 + refund - 2800);
    assert_eq!(
        a.car(car).unwrap().mods,
        vec!["susp_coilovers_2".to_string()]
    );
    e.uninstall_mod(&mut a, car, "susp_coilovers_2").unwrap();
    assert!(a.car(car).unwrap().mods.is_empty());
    // Too expensive: refused, nothing changes.
    let snap = a.clone();
    assert!(e
        .install_mod(&mut a, car, "engine_v8_swap", DamageMode::Arcade)
        .is_err());
    assert_eq!(a, snap);
    // Impounded cars cannot be modded or repaired.
    a.car_mut(car).unwrap().location = CarLocation::Impound {
        since_day: 0,
        level: WantedLevel::L1,
    };
    assert!(e
        .install_mod(&mut a, car, "engine_intake_1", DamageMode::Arcade)
        .is_err());
}

#[test]
fn preview_reports_sensible_numbers() {
    let e = econ();
    let s = sedan();
    let base = e.preview(&s.def, &s.lattice, &[], None).unwrap();
    // Base definitions are never mutated.
    assert_eq!(base.applied.vehicle, s.def);
    assert_eq!(base.applied.lattice, s.lattice);
    assert!(
        base.est_0_100_s > 5.0 && base.est_0_100_s < 11.0,
        "0-100 {}",
        base.est_0_100_s
    );
    assert!(base.est_top_speed_mps > 50.0 && base.est_top_speed_mps < 75.0);

    let build: Vec<String> = [
        "engine_intake_1",
        "engine_ecu_1",
        "engine_internals_1",
        "engine_fuel_system",
        "fi_turbo_2",
        "weight_strip_1",
        "tires_sport",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let p = e.preview(&s.def, &s.lattice, &build, None).unwrap();
    assert!(p.metrics.peak_power_kw > base.metrics.peak_power_kw * 1.3);
    assert!(p.est_0_100_s < base.est_0_100_s);
    assert!(p.class_index > base.class_index);
    assert_eq!(p.applied.vehicle.wheels[0].tire_id, "sport");

    // Cage: adds cabin beams, mass and detachable requests come from body kits.
    let caged = e
        .preview(
            &s.def,
            &s.lattice,
            &["prot_cage_2".to_string(), "body_front_lip".to_string()],
            None,
        )
        .unwrap();
    assert!(caged.applied.lattice.beams.len() > s.lattice.beams.len());
    assert!((caged.mass_kg - s.def.mass_kg - 45.0).abs() < 1e-3);
    assert_eq!(caged.applied.detachable_requests.len(), 1);
    assert_eq!(
        caged.applied.detachable_requests[0].region,
        BodyRegion::FrontBumper
    );

    // Lattice node mass grows by the durability mass.
    let node_mass = |l: &wp_contracts::LatticeDef| l.nodes.iter().map(|n| n.mass_kg).sum::<f32>();
    assert!((node_mass(&caged.applied.lattice) - node_mass(&s.lattice) - 45.0).abs() < 0.01);
}

#[test]
fn class_index_brackets_debug_cars_apart() {
    let e = econ();
    let s = e
        .preview(&sedan().def, &sedan().lattice, &[], None)
        .unwrap();
    let c = e
        .preview(&coupe().def, &coupe().lattice, &[], None)
        .unwrap();
    assert!(c.class_index > s.class_index + 100);
    assert_ne!(s.bracket, c.bracket);
}
