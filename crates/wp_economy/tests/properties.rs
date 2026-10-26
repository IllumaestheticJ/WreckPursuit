//! Property tests required by prompt 08:
//! - cash never goes negative except through penalties (fines / bail / towing), and debt blocks
//!   race entry (except free meets);
//! - repairing a fully wrecked Simulation car costs >= 60 % of its value;
//! - durability mods always add mass and improve at most one performance stat;
//! - mod requirements / whitelists / mode restrictions are enforced;
//! - the class index is monotone in power and in (lower) mass.

mod common;

use common::*;
use proptest::prelude::*;
use wp_contracts::{DamageMode, DayPhase, RepairItem, VehicleClass, VehicleDef};
use wp_economy::capture::{CaptureContext, CopAction};
use wp_economy::data::{ALL_COMPONENTS, ALL_LEVELS, ALL_REGIONS};
use wp_economy::estimate::PerfMetrics;
use wp_economy::repair::{reference_estimate, DamageReport};
use wp_economy::{
    CarSpec, Condition, DamageSnapshot, Economy, EconomyError, EconomyEvent, OwnedCar,
    PlayerAccount, RaceResult, RaceSetup, RaceType, RepairOption, ShopContext, ShopKind,
    ViolationCounts, WantedLevel,
};
use wp_math::Rng;

// ---------------------------------------------------------------------------------------------
// Cash / debt invariant
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Op {
    Race {
        meet: bool,
        placement: u8,
        contacts: u32,
        wanted: usize,
    },
    Damage {
        severity: f32,
        towed: bool,
    },
    Repair {
        which: u8,
        crooked: bool,
    },
    InstallMod {
        pick: usize,
    },
    UninstallMod {
        pick: usize,
    },
    Capture {
        level: usize,
        total_loss: bool,
    },
    Escape {
        level: usize,
    },
    EndOfDay,
    Retrieve,
    BuyCar {
        worn: bool,
    },
    SellCar {
        pick: usize,
    },
    FloorCar,
    Insurance {
        full: bool,
    },
    Informant {
        tier: u8,
    },
    Cop {
        what: u8,
    },
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (any::<bool>(), 0u8..6, 0u32..3, 0usize..6)
            .prop_map(|(meet, placement, contacts, wanted)| Op::Race { meet, placement, contacts, wanted }),
        3 => (0.0f32..1.0, any::<bool>()).prop_map(|(severity, towed)| Op::Damage { severity, towed }),
        3 => (0u8..4, any::<bool>()).prop_map(|(which, crooked)| Op::Repair { which, crooked }),
        2 => (0usize..200).prop_map(|pick| Op::InstallMod { pick }),
        1 => (0usize..20).prop_map(|pick| Op::UninstallMod { pick }),
        2 => (0usize..6, any::<bool>()).prop_map(|(level, total_loss)| Op::Capture { level, total_loss }),
        1 => (1usize..6).prop_map(|level| Op::Escape { level }),
        2 => Just(Op::EndOfDay),
        1 => Just(Op::Retrieve),
        1 => any::<bool>().prop_map(|worn| Op::BuyCar { worn }),
        1 => (0usize..4).prop_map(|pick| Op::SellCar { pick }),
        1 => Just(Op::FloorCar),
        1 => any::<bool>().prop_map(|full| Op::Insurance { full }),
        1 => (0u8..3).prop_map(|tier| Op::Informant { tier }),
        1 => (0u8..6).prop_map(|what| Op::Cop { what }),
    ]
}

fn check_events(before: i64, events: &[EconomyEvent]) {
    let mut cash = before;
    for e in events {
        assert_eq!(cash + e.cash_delta, e.cash_after, "event cash arithmetic");
        cash = e.cash_after;
        if e.cash_delta < 0 && e.cash_after < 0 {
            assert!(
                e.reason.is_penalty(),
                "non-penalty {:?} drove cash to {}",
                e.reason,
                e.cash_after
            );
        }
    }
}

fn run_op(
    e: &Economy,
    a: &mut PlayerAccount,
    op: &Op,
    mode: DamageMode,
    rng: &mut Rng,
) -> Vec<EconomyEvent> {
    let before = a.cash;
    let car = a.active_car.or_else(|| a.garage.first().map(|c| c.uid));
    let shop = |crooked: bool| ShopContext {
        district: wp_contracts::DistrictId::Core,
        kind: if crooked {
            ShopKind::Crooked
        } else {
            ShopKind::Legit
        },
        wanted: WantedLevel::L2,
    };
    let res: Result<Vec<EconomyEvent>, EconomyError> = match op.clone() {
        Op::Race {
            meet,
            placement,
            contacts,
            wanted,
        } => match car {
            Some(car) => {
                let class = a.car(car).unwrap().class;
                let setup = RaceSetup {
                    race_type: if meet {
                        RaceType::Meet
                    } else {
                        RaceType::Sprint
                    },
                    kind: None,
                    legality: Default::default(),
                    class,
                    route_length_m: 8000.0,
                    phase: DayPhase::Night,
                    mode,
                };
                let fee = e.entry_fee(&setup);
                let r = e.enter_race(a, &setup, car);
                if a.in_debt() && fee > 0 {
                    assert!(r.is_err(), "debt must block a race with a buy-in");
                }
                match r {
                    Ok((_, mut ev)) => {
                        let res = RaceResult {
                            placement,
                            contacts,
                            violations: ViolationCounts::default(),
                            wanted: ALL_LEVELS[wanted],
                        };
                        let (_, ev2) = e.settle_race(a, &setup, car, &res).unwrap();
                        ev.extend(ev2);
                        Ok(ev)
                    }
                    Err(err) => Err(err),
                }
            }
            None => Ok(vec![]),
        },
        Op::Damage { severity, towed } => match car {
            Some(car) => {
                let c = a.car(car).unwrap().clone();
                let table = e.price_table_for(&c);
                let dmg: Vec<(RepairItem, f32)> = ALL_REGIONS
                    .iter()
                    .take(6)
                    .map(|r| (RepairItem::Region(*r), severity))
                    .collect();
                let est = reference_estimate(&table, e.mode_table(mode).repair_cost_scale, &dmg);
                let report = DamageReport {
                    car,
                    health: 1.0 - severity,
                    total_loss: severity > 0.9,
                    handling: wp_contracts::HandlingEffects::NEUTRAL,
                    estimate: est,
                    lattice_state: vec![1, 2, 3],
                    wanted_during_session: severity > 0.5,
                    towed,
                };
                e.record_damage(a, &report).map(|x| x.1)
            }
            None => Ok(vec![]),
        },
        Op::Repair { which, crooked } => match car {
            Some(car) => {
                let opt = match which {
                    0 => RepairOption::RepairAll,
                    1 => RepairOption::RepairSelected(
                        a.car(car)
                            .unwrap()
                            .damage
                            .outstanding
                            .lines
                            .iter()
                            .take(2)
                            .map(|l| l.item)
                            .collect(),
                    ),
                    2 => RepairOption::DriveDamaged,
                    _ => RepairOption::Scrap,
                };
                e.repair(a, car, &opt, &shop(crooked), mode).map(|x| x.1)
            }
            None => Ok(vec![]),
        },
        Op::InstallMod { pick } => match car {
            Some(car) => {
                let id = e
                    .catalog
                    .iter()
                    .nth(pick % e.catalog.len())
                    .unwrap()
                    .id
                    .clone();
                e.install_mod(a, car, &id, mode).map(|x| x.1)
            }
            None => Ok(vec![]),
        },
        Op::UninstallMod { pick } => match car {
            Some(car) => {
                let mods = a.car(car).unwrap().mods.clone();
                if mods.is_empty() {
                    Ok(vec![])
                } else {
                    e.uninstall_mod(a, car, &mods[pick % mods.len()])
                        .map(|x| x.1)
                }
            }
            None => Ok(vec![]),
        },
        Op::Capture { level, total_loss } => match car {
            Some(car) => {
                let ctx = CaptureContext {
                    mode,
                    wanted: ALL_LEVELS[level],
                    car,
                    total_loss,
                    session_violations: ViolationCounts {
                        speeding: 2,
                        red_light: 1,
                        ..Default::default()
                    },
                };
                e.handle_capture(a, &ctx, rng).map(|x| x.1)
            }
            None => Ok(vec![]),
        },
        Op::Escape { level } => e.handle_escape(a, ALL_LEVELS[level], 1.3).map(|x| x.1),
        Op::EndOfDay => e.end_of_day(a).map(|x| x.1),
        Op::Retrieve => {
            let imp = a
                .garage
                .iter()
                .find(|c| c.location != wp_economy::CarLocation::Garage)
                .map(|c| c.uid);
            match imp {
                Some(uid) => e.retrieve_from_impound(a, uid).map(|x| x.1),
                None => Ok(vec![]),
            }
        }
        Op::BuyCar { worn } => {
            let spec = CarSpec::from(&sedan().def);
            let cond = if worn {
                Condition::Worn
            } else {
                Condition::Used
            };
            e.buy_car(a, &spec, cond).map(|x| x.1)
        }
        Op::SellCar { pick } => {
            if a.garage.is_empty() {
                Ok(vec![])
            } else {
                let uid = a.garage[pick % a.garage.len()].uid;
                e.sell_car(a, uid).map(|x| x.1)
            }
        }
        Op::FloorCar => e.issue_floor_car(a).map(|x| x.1),
        Op::Insurance { full } => match car {
            Some(car) => e
                .buy_insurance(a, car, if full { "full" } else { "basic" })
                .map(|x| x.1),
            None => Ok(vec![]),
        },
        Op::Informant { tier } => {
            let t = ["whisper", "contact", "inside_man"][tier as usize];
            e.buy_informant_tip(a, t).map(|x| x.1)
        }
        Op::Cop { what } => {
            let action = match what {
                0 => CopAction::Capture {
                    level: WantedLevel::L3,
                    racer_bounty: 900,
                },
                1 => CopAction::Assist {
                    level: WantedLevel::L2,
                    racer_bounty: 300,
                },
                2 => CopAction::CleanDriving { minutes: 12 },
                3 => CopAction::CivilianCollision,
                4 => CopAction::SelfWreck,
                _ => CopAction::RiskBudgetOverspent { overspent: 1.5 },
            };
            e.award_cop(a, action).map(|x| x.1)
        }
    };
    assert!(a.rep >= 0);
    match res {
        Ok(events) => {
            check_events(before, &events);
            events
        }
        Err(_) => {
            assert_eq!(a.cash, before, "a refused operation must not change cash");
            Vec::new()
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    #[test]
    fn cash_goes_negative_only_through_penalties(
        ops in prop::collection::vec(op(), 1..60),
        sim in any::<bool>(),
        seed in any::<u64>(),
        start in 0i64..30_000,
    ) {
        let e = econ();
        let mode = if sim { DamageMode::Simulation } else { DamageMode::Arcade };
        let (mut a, _) = account_with_sedan(&e, start);
        let mut rng = Rng::new(seed);
        let mut next_seq = a.next_event_seq;
        for op in &ops {
            for ev in run_op(&e, &mut a, op, mode, &mut rng) {
                prop_assert_eq!(ev.seq, next_seq, "event sequence has a gap");
                next_seq += 1;
            }
            prop_assert_eq!(a.next_event_seq, next_seq);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Full wreck >= 60 % of value (Simulation)
// ---------------------------------------------------------------------------------------------

const CIVIL: [VehicleClass; 6] = [
    VehicleClass::Beater,
    VehicleClass::Compact,
    VehicleClass::Sport,
    VehicleClass::Muscle,
    VehicleClass::Exotic,
    VehicleClass::Truck,
];

proptest! {
    #[test]
    fn full_wreck_in_simulation_costs_at_least_60_percent(
        class_i in 0usize..6,
        value_t in 0.0f32..1.0,
        n_mods in 0usize..12,
        mod_seed in any::<u64>(),
        district in 0usize..6,
        crooked in any::<bool>(),
    ) {
        let e = econ();
        let class = CIVIL[class_i];
        let [lo, hi] = e.market.class_bands[&class];
        let base_price = (lo as f32 + (hi - lo) as f32 * value_t) as u32;
        let mut rng = Rng::new(mod_seed);
        let allowed: Vec<String> = e.catalog.available_for(class, None).map(|m| m.id.clone()).collect();
        let mut mods: Vec<String> = (0..n_mods).map(|_| allowed[rng.below(allowed.len() as u32) as usize].clone()).collect();
        mods.sort();
        mods.dedup();
        let mut car = OwnedCar {
            uid: wp_economy::CarUid(1),
            def_id: "x".into(),
            class,
            base_price,
            mods,
            damage: DamageSnapshot::pristine(),
            location: wp_economy::CarLocation::Garage,
            insurance: None,
            purchase_price: Some(base_price as i64),
            tires: Default::default(),
            variant_name: None,
            paint: None,
            tune: Default::default(),
            parts: None,
        };
        let table = e.price_table_for(&car);
        let all: Vec<(RepairItem, f32)> = ALL_REGIONS.iter().map(|r| (RepairItem::Region(*r), 1.0))
            .chain(ALL_COMPONENTS.iter().map(|c| (RepairItem::Component(*c), 1.0))).collect();
        let sim = e.mode_table(DamageMode::Simulation).repair_cost_scale;
        car.damage.outstanding = reference_estimate(&table, sim, &all);
        car.damage.total_loss = true;
        car.damage.health = 0.0;
        let value = e.car_value(&car);
        let mut a = PlayerAccount::empty(wp_contracts::PlayerId(1));
        a.garage.push(car);
        let shop = ShopContext {
            district: wp_contracts::DistrictId::ALL[district],
            kind: if crooked { ShopKind::Crooked } else { ShopKind::Legit },
            wanted: WantedLevel::None,
        };
        let q = e.quote_repair(&a, wp_economy::CarUid(1), &RepairOption::RepairAll, &shop, DamageMode::Simulation).unwrap();
        prop_assert!(q.gross as f32 >= 0.6 * value as f32, "class {class:?} value {value} repair {}", q.gross);
    }
}

// ---------------------------------------------------------------------------------------------
// Durability mods: always add mass, at most one performance stat improves
// ---------------------------------------------------------------------------------------------

fn improved(before: &PerfMetrics, after: &PerfMetrics) -> Vec<&'static str> {
    let eps = 1e-4;
    let better_hi = |a: f32, b: f32| b > a * (1.0 + eps) + eps;
    let better_lo = |a: f32, b: f32| b < a * (1.0 - eps) - eps;
    let mut v = Vec::new();
    if better_hi(before.peak_power_kw, after.peak_power_kw) {
        v.push("power");
    }
    if better_hi(
        before.power_to_weight_w_per_kg,
        after.power_to_weight_w_per_kg,
    ) {
        v.push("power_to_weight");
    }
    if better_lo(before.est_0_100_s, after.est_0_100_s) {
        v.push("0-100");
    }
    if better_hi(before.est_top_speed_mps, after.est_top_speed_mps) {
        v.push("top_speed");
    }
    if better_hi(before.lateral_grip_g, after.lateral_grip_g) {
        v.push("lateral_grip");
    }
    if better_hi(before.brake_decel_g, after.brake_decel_g) {
        v.push("braking");
    }
    v
}

fn police_sedan() -> wp_contracts::LoadedCar {
    let mut c = sedan();
    c.def.class = VehicleClass::PoliceCruiser;
    c
}

fn truck_sedan() -> wp_contracts::LoadedCar {
    let mut c = sedan();
    c.def.class = VehicleClass::Truck;
    c
}

fn check_durability(e: &Economy, car: &wp_contracts::LoadedCar, base_mods: &[String]) {
    let Ok(base) = e.preview(&car.def, &car.lattice, base_mods, None) else {
        return;
    };
    for m in e.catalog.iter().filter(|m| m.is_durability_mod()) {
        if !m.allows_class(car.def.class)
            || base_mods
                .iter()
                .any(|b| e.catalog.get(b).unwrap().line == m.line)
        {
            continue;
        }
        let mut mods = base_mods.to_vec();
        mods.push(m.id.clone());
        let Ok(p) = e.preview(&car.def, &car.lattice, &mods, None) else {
            continue;
        };
        assert!(
            p.mass_kg > base.mass_kg,
            "{} on {} did not add mass ({} -> {})",
            m.id,
            car.def.id,
            base.mass_kg,
            p.mass_kg
        );
        let imp = improved(&base.metrics, &p.metrics);
        assert!(imp.len() <= 1, "{} improved {imp:?}", m.id);
        // The lattice actually got stronger somewhere.
        assert_ne!(
            p.applied.lattice, base.applied.lattice,
            "{} changed nothing",
            m.id
        );
    }
}

#[test]
fn every_durability_mod_adds_mass_and_improves_at_most_one_stat() {
    let e = econ();
    let n = e.catalog.iter().filter(|m| m.is_durability_mod()).count();
    assert!(n >= 10, "only {n} durability mods");
    for car in [sedan(), coupe(), police_sedan(), truck_sedan()] {
        check_durability(&e, &car, &[]);
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    #[test]
    fn durability_rule_holds_on_random_builds(seed in any::<u64>(), n in 0usize..8, coupe_car in any::<bool>()) {
        let e = econ();
        let car = if coupe_car { coupe() } else { sedan() };
        // Random legal build of non-durability mods via the install rules.
        let mut rng = Rng::new(seed);
        let pool: Vec<String> = e.catalog.available_for(car.def.class, None)
            .filter(|m| !m.is_durability_mod()).map(|m| m.id.clone()).collect();
        let mut mods: Vec<String> = Vec::new();
        for _ in 0..n * 4 {
            let id = &pool[rng.below(pool.len() as u32) as usize];
            if let Ok(plan) = e.catalog.check_install(car.def.class, None, &mods, id) {
                mods.retain(|m| Some(m) != plan.replaces.as_ref());
                mods.push(id.clone());
            }
            if mods.len() >= n { break; }
        }
        check_durability(&e, &car, &mods);
    }
}

// ---------------------------------------------------------------------------------------------
// Requirements, whitelists, modes
// ---------------------------------------------------------------------------------------------

#[test]
fn whitelist_requirements_and_modes_are_enforced() {
    let e = econ();
    let (mut a, car) = account_with_sedan(&e, 200_000);
    let arcade = DamageMode::Arcade;
    // Whitelist: police push bar on a Compact.
    assert!(matches!(
        e.install_mod(&mut a, car, "prot_push_bar", arcade),
        Err(EconomyError::ModNotAllowed { .. })
    ));
    // Requirement: race ECU needs an intake.
    assert!(matches!(
        e.install_mod(&mut a, car, "engine_ecu_2", arcade),
        Err(EconomyError::MissingRequirement { .. })
    ));
    e.install_mod(&mut a, car, "engine_intake_1", arcade)
        .unwrap();
    e.install_mod(&mut a, car, "engine_ecu_2", arcade).unwrap();
    // Upgrading the prerequisite keeps the dependent valid.
    e.install_mod(&mut a, car, "engine_intake_2", arcade)
        .unwrap();
    assert!(!a
        .car(car)
        .unwrap()
        .mods
        .contains(&"engine_intake_1".to_string()));
    // Removing it is refused while the ECU needs it.
    assert!(matches!(
        e.uninstall_mod(&mut a, car, "engine_intake_2"),
        Err(EconomyError::RequiredBy { .. })
    ));
    // Downgrading below a requirement's tier is refused.
    e.install_mod(&mut a, car, "engine_ecu_1", arcade).unwrap(); // ECU line: 2 -> 1
    e.install_mod(&mut a, car, "engine_internals_1", arcade)
        .unwrap();
    e.install_mod(&mut a, car, "engine_internals_2", arcade)
        .unwrap();
    e.install_mod(&mut a, car, "engine_fuel_system", arcade)
        .unwrap();
    e.install_mod(&mut a, car, "cooling_intercooler_1", arcade)
        .unwrap();
    e.install_mod(&mut a, car, "fi_turbo_3", arcade).unwrap();
    assert!(matches!(
        e.install_mod(&mut a, car, "engine_internals_1", arcade),
        Err(EconomyError::RequiredBy { .. })
    ));
    // Mode: nitrous is Arcade only.
    assert!(matches!(
        e.install_mod(&mut a, car, "elec_nitrous_1", DamageMode::Simulation),
        Err(EconomyError::ModNotAllowed { .. })
    ));
    e.install_mod(&mut a, car, "elec_nitrous_1", arcade)
        .unwrap();
    // Installed nitrous is inactive in a Simulation preview.
    let s = sedan();
    let mods = a.car(car).unwrap().mods.clone();
    let p = e
        .preview(&s.def, &s.lattice, &mods, Some(DamageMode::Simulation))
        .unwrap();
    assert_eq!(p.applied.tools.nitrous, 0);
    assert!(p
        .applied
        .warnings
        .iter()
        .any(|w| w.contains("elec_nitrous_1")));
    let p = e.preview(&s.def, &s.lattice, &mods, Some(arcade)).unwrap();
    assert_eq!(p.applied.tools.nitrous, 1);
    // Preview rejects illegal sets too.
    assert!(e
        .preview(&s.def, &s.lattice, &["fi_turbo_2".to_string()], None)
        .is_err());
}

/// The Steering Angle Kit's quicker ratio reaches the rack pads and keys steer through, not
/// only the force feedback (owner, 2026-10-26: no half-working mods).
#[test]
fn steering_angle_kit_quickens_the_rack() {
    let e = econ();
    let s = sedan();
    let p = e
        .preview(&s.def, &s.lattice, &["susp_angle_kit".to_string()], None)
        .unwrap();
    let (base, kit) = (&s.def.steering, &p.applied.vehicle.steering);
    assert!(kit.ratio < base.ratio);
    assert!(
        (kit.rack_speed_rad_per_s * kit.ratio - base.rack_speed_rad_per_s * base.ratio).abs()
            < 1e-3,
        "rack {} -> {}",
        base.rack_speed_rad_per_s,
        kit.rack_speed_rad_per_s
    );
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 96, ..ProptestConfig::default() })]

    #[test]
    fn installed_sets_always_stay_legal(picks in prop::collection::vec((0usize..500, any::<bool>()), 1..40), sim in any::<bool>()) {
        let e = econ();
        let mode = if sim { DamageMode::Simulation } else { DamageMode::Arcade };
        let (mut a, car) = account_with_sedan(&e, 1_000_000);
        let class = a.car(car).unwrap().class;
        for (pick, install) in picks {
            let before = a.car(car).unwrap().mods.clone();
            if install {
                let m = e.catalog.iter().nth(pick % e.catalog.len()).unwrap().clone();
                let r = e.install_mod(&mut a, car, &m.id, mode);
                if r.is_ok() {
                    prop_assert!(m.allows_class(class));
                    prop_assert!(m.allows_mode(mode));
                } else {
                    prop_assert_eq!(&a.car(car).unwrap().mods, &before);
                    if !m.allows_class(class) {
                        let whitelist_refusal = matches!(r, Err(EconomyError::ModNotAllowed { .. }));
                        prop_assert!(whitelist_refusal);
                    }
                }
            } else if !before.is_empty() {
                let id = before[pick % before.len()].clone();
                let _ = e.uninstall_mod(&mut a, car, &id);
            }
            let mods = a.car(car).unwrap().mods.clone();
            prop_assert!(e.catalog.validate_set(class, &mods).is_ok(), "illegal set {:?}", mods);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Class index monotonicity
// ---------------------------------------------------------------------------------------------

fn with_torque_and_mass(def: &VehicleDef, torque: f32, mass: f32) -> VehicleDef {
    let mut d = def.clone();
    for p in d.engine.torque_curve_nm.iter_mut() {
        p.1 *= torque;
    }
    d.mass_kg = mass;
    d
}

proptest! {
    #[test]
    fn class_index_is_monotone_in_power_and_weight(
        t1 in 0.3f32..3.0, t2 in 0.3f32..3.0,
        m1 in 700.0f32..3000.0, m2 in 700.0f32..3000.0,
        coupe_car in any::<bool>(),
    ) {
        let e = econ();
        let base = if coupe_car { coupe().def } else { sedan().def };
        let (tlo, thi) = if t1 <= t2 { (t1, t2) } else { (t2, t1) };
        let (mlo, mhi) = if m1 <= m2 { (m1, m2) } else { (m2, m1) };
        // More power, same mass -> index does not drop.
        let a = e.class_index(&with_torque_and_mass(&base, tlo, mhi));
        let b = e.class_index(&with_torque_and_mass(&base, thi, mhi));
        prop_assert!(b >= a, "torque {tlo}->{thi}: {a} -> {b}");
        // Less mass, same power -> index does not drop.
        let c = e.class_index(&with_torque_and_mass(&base, tlo, mlo));
        prop_assert!(c >= a, "mass {mhi}->{mlo}: {a} -> {c}");
    }
}
