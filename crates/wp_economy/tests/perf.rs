//! Timing of the main operations (not a pass/fail test). Run with
//! `cargo test -p wp_economy --release --test perf -- --ignored --nocapture`.

mod common;

use std::time::Instant;

use common::*;
use wp_contracts::{DamageMode, DayPhase, VehicleClass};
use wp_economy::capture::CaptureContext;
use wp_economy::career::{simulate_career, CareerConfig};
use wp_economy::{
    EconomySession, RaceResult, RaceSetup, RaceType, RepairOption, SessionConfig, ShopContext,
    ViolationCounts, WantedLevel,
};
use wp_math::Rng;

fn time<F: FnMut()>(label: &str, n: u32, mut f: F) {
    let t = Instant::now();
    for _ in 0..n {
        f();
    }
    println!(
        "{label}: {:.2} µs",
        t.elapsed().as_secs_f64() * 1e6 / n as f64
    );
}

#[test]
#[ignore]
fn perf() {
    let e = econ();
    let s = sedan();
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
    .map(|x| x.to_string())
    .collect();
    time("preview (7 mods)", 2000, || {
        std::hint::black_box(e.preview(&s.def, &s.lattice, &build, None).unwrap());
    });
    let cage = vec!["prot_cage_2".to_string()];
    time("preview (cage)", 2000, || {
        std::hint::black_box(e.preview(&s.def, &s.lattice, &cage, None).unwrap());
    });
    let (a0, car) = account_with_sedan(&e, 100_000);
    let setup = RaceSetup {
        race_type: RaceType::Sprint,
        kind: None,
        legality: Default::default(),
        class: VehicleClass::Compact,
        route_length_m: 9000.0,
        phase: DayPhase::Night,
        mode: DamageMode::Arcade,
        purse_scale: 1.0,
        entry_scale: 1.0,
    };
    let res = RaceResult {
        placement: 1,
        contacts: 0,
        violations: ViolationCounts::default(),
        wanted: WantedLevel::None,
    };
    time("enter + settle race", 20000, || {
        let mut a = a0.clone();
        e.enter_race(&mut a, &setup, car).unwrap();
        std::hint::black_box(e.settle_race(&mut a, &setup, car, &res).unwrap());
    });
    let mut rng = Rng::new(1);
    let ctx = CaptureContext {
        mode: DamageMode::Simulation,
        wanted: WantedLevel::L3,
        car,
        total_loss: false,
        session_violations: ViolationCounts::default(),
    };
    time("handle_capture", 20000, || {
        let mut a = a0.clone();
        std::hint::black_box(e.handle_capture(&mut a, &ctx, &mut rng).unwrap());
    });
    time("quote_repair (drive damaged)", 20000, || {
        std::hint::black_box(
            e.quote_repair(
                &a0,
                car,
                &RepairOption::DriveDamaged,
                &ShopContext::legit_default(),
                DamageMode::Arcade,
            )
            .unwrap(),
        );
    });
    // Session: `observe` runs inside the sim's event drain, so it is the per-tick cost.
    let events: Vec<wp_contracts::SimEvent> = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/capture_stationary.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let n = events.len() as f64;
    let t = Instant::now();
    for _ in 0..2000 {
        let mut s = EconomySession::new(SessionConfig::new(DamageMode::Arcade));
        for ev in &events {
            s.observe(ev);
        }
        std::hint::black_box(s);
    }
    println!(
        "session observe (recorded capture, mostly police events): {:.3} µs/event",
        t.elapsed().as_secs_f64() * 1e6 / (2000.0 * n)
    );
    let entity = wp_contracts::EntityId(7);
    time("session finish (capture + impound)", 5000, || {
        let mut a = a0.clone();
        let mut s = EconomySession::new(SessionConfig::new(DamageMode::Arcade));
        s.bind(entity, a.player, car);
        for ev in &events {
            s.observe(ev);
        }
        std::hint::black_box(s.finish(&e, &mut a, &[], &mut rng).unwrap());
    });
    time("20-night career", 200, || {
        std::hint::black_box(simulate_career(
            &e,
            &CareerConfig::mid_skill(DamageMode::Simulation, 3),
        ));
    });
}
