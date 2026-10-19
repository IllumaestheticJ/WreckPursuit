//! Timing of the main operations (not a pass/fail test). Run with
//! `cargo test -p wp_economy --release --test perf -- --ignored --nocapture`.

mod common;

use std::time::Instant;

use common::*;
use wp_contracts::{DamageMode, DayPhase, VehicleClass};
use wp_economy::career::{simulate_career, CareerConfig};
use wp_economy::capture::CaptureContext;
use wp_economy::{RaceResult, RaceSetup, RaceType, RepairOption, ShopContext, ViolationCounts, WantedLevel};
use wp_math::Rng;

fn time<F: FnMut()>(label: &str, n: u32, mut f: F) {
    let t = Instant::now();
    for _ in 0..n {
        f();
    }
    println!("{label}: {:.2} µs", t.elapsed().as_secs_f64() * 1e6 / n as f64);
}

#[test]
#[ignore]
fn perf() {
    let e = econ();
    let s = sedan();
    let build: Vec<String> = ["engine_intake_1", "engine_ecu_1", "engine_internals_1", "engine_fuel_system", "fi_turbo_2", "weight_strip_1", "tires_sport"]
        .iter().map(|x| x.to_string()).collect();
    time("preview (7 mods)", 2000, || {
        std::hint::black_box(e.preview(&s.def, &s.lattice, &build, None).unwrap());
    });
    let cage = vec!["prot_cage_2".to_string()];
    time("preview (cage)", 2000, || {
        std::hint::black_box(e.preview(&s.def, &s.lattice, &cage, None).unwrap());
    });
    let (a0, car) = account_with_sedan(&e, 100_000);
    let setup = RaceSetup { race_type: RaceType::Sprint, class: VehicleClass::Compact, route_length_m: 9000.0, phase: DayPhase::Night, mode: DamageMode::Arcade };
    let res = RaceResult { placement: 1, contacts: 0, violations: ViolationCounts::default(), wanted: WantedLevel::None };
    time("enter + settle race", 20000, || {
        let mut a = a0.clone();
        e.enter_race(&mut a, &setup, car).unwrap();
        std::hint::black_box(e.settle_race(&mut a, &setup, car, &res).unwrap());
    });
    let mut rng = Rng::new(1);
    let ctx = CaptureContext { mode: DamageMode::Simulation, wanted: WantedLevel::L3, car, total_loss: false, session_violations: ViolationCounts::default() };
    time("handle_capture", 20000, || {
        let mut a = a0.clone();
        std::hint::black_box(e.handle_capture(&mut a, &ctx, &mut rng).unwrap());
    });
    time("quote_repair (drive damaged)", 20000, || {
        std::hint::black_box(e.quote_repair(&a0, car, &RepairOption::DriveDamaged, &ShopContext::legit_default(), DamageMode::Arcade).unwrap());
    });
    time("20-night career", 200, || {
        std::hint::black_box(simulate_career(&e, &CareerConfig::mid_skill(DamageMode::Simulation, 3)));
    });
}
