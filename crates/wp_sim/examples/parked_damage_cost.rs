//! Per-frame cost of parked-car damage (owner playtest 2026-10-21; numbers in
//! `docs/integration/parked-signals.md`): `cargo run --release -p wp_sim --example
//! parked_damage_cost [arcade|sim]`.
//!
//! 24 debug sedans on the flat world, each driving at 50 km/h into its own parked car (and
//! side-swiping the one after it), all at once: the worst case of many fresh dents in the same
//! frames. Each run is timed twice: with parked-car damage (`Sim::set_parked_car`) and without
//! (parked cars are shoved boxes, as before), so the difference is what dents cost. Also times
//! 400 untouched parked cars around 24 cruising cars (the city case: almost none are hit).
//! A frame is 4 physics ticks (60 Hz).

use std::time::Instant;

use wp_contracts::{EntityId, LoadedCar, TimeOfDay, VehicleInput};
use wp_sim::{Sim, SimConfig};

fn data(path: &str) -> String {
    format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
}

struct Timing {
    mean_ms: f32,
    worst_ms: f32,
    dented: usize,
    lattice_ticks: u64,
}

fn run(car: &LoadedCar, mode: &str, dents: bool, parked_rows: bool) -> Timing {
    let table = wp_contracts::load_mode_table(data(&format!("modes/{mode}.toml"))).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed: 7,
        mode: table,
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 1.0,
    });
    let mut ids: Vec<EntityId> = Vec::new();
    for k in 0..24 {
        let x = k as f32 * 12.0;
        let id = sim.spawn_vehicle(car, [x, 0.0, 0.0], 0.0).unwrap();
        sim.place_vehicle(id, [x, 0.0, 0.0], 0.0, 13.9).unwrap();
        ids.push(id);
        if parked_rows {
            // Straight ahead, and the next one offset so the car clips it after the first hit.
            sim.add_parked_car([x, 30.0, 0.0], 0.0);
            sim.add_parked_car([x + 1.4, 37.0, 0.0], 0.0);
        } else {
            // A parked row 5 m to the side of each lane: woken, never hit.
            for j in 0..16 {
                sim.add_parked_car([x + 5.0, 10.0 + j as f32 * 6.5, 0.0], 0.0);
            }
            sim.add_parked_car([x - 5.0, 40.0, 0.0], 0.0);
        }
    }
    if dents {
        sim.set_parked_car(car.clone());
    }
    for id in &ids {
        sim.set_input(
            *id,
            VehicleInput {
                throttle: 0.35,
                ..Default::default()
            },
        )
        .unwrap();
    }
    // 4 s at 60 Hz: impacts around 1.2 s and 1.8 s, then the shoved cars settle.
    let mut total = 0.0f64;
    let mut worst = 0.0f64;
    let frames = 240;
    for _ in 0..frames {
        let t0 = Instant::now();
        for _ in 0..4 {
            sim.step_fixed();
        }
        let ms = t0.elapsed().as_secs_f64() * 1e3;
        total += ms;
        worst = worst.max(ms);
    }
    Timing {
        mean_ms: (total / frames as f64) as f32,
        worst_ms: worst as f32,
        dented: sim.parked_wrecks().len(),
        lattice_ticks: sim.parked_stats().lattice_ticks,
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "arcade".into());
    let car = wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap();
    // Warm up caches / allocator once.
    run(&car, &mode, true, true);
    println!("24 cars, 50 km/h into 48 parked cars ({mode}), per 60 Hz frame (4 ticks):");
    let on = run(&car, &mode, true, true);
    let off = run(&car, &mode, false, true);
    for (name, t) in [("dents on ", &on), ("dents off", &off)] {
        println!(
            "  {name}: mean {:.3} ms, worst {:.3} ms, {} dented, {} lattice ticks",
            t.mean_ms, t.worst_ms, t.dented, t.lattice_ticks
        );
    }
    let per = if on.lattice_ticks > 0 {
        (on.mean_ms - off.mean_ms) * 240.0 / on.lattice_ticks as f32 * 4.0
    } else {
        0.0
    };
    println!(
        "  dent cost: +{:.3} ms mean, +{:.3} ms worst frame; ≈ {per:.3} ms per awake parked lattice per frame",
        on.mean_ms - off.mean_ms,
        on.worst_ms - off.worst_ms
    );
    let quiet_on = run(&car, &mode, true, false);
    let quiet_off = run(&car, &mode, false, false);
    println!("24 cars cruising past 408 parked cars, none hit:");
    for (name, t) in [("dents on ", &quiet_on), ("dents off", &quiet_off)] {
        println!(
            "  {name}: mean {:.3} ms, worst {:.3} ms, {} dented",
            t.mean_ms, t.worst_ms, t.dented
        );
    }
}
