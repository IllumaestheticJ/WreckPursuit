//! Damage perf benchmark: 24 debug-sedan lattices, (a) idle, (b) all 24 resolving a 56 km/h
//! frontal crash at once (worst case), (c) one crash every 2 s per car (amortised).
//!
//! ```text
//! cargo run --release -p wp_damage --bin damage-bench
//! ```
//! Wall-clock timing here is benchmark tooling, never simulation input.

use std::path::PathBuf;
use std::time::Instant;

use wp_contracts::{DamageSim, PHYSICS_DT};
use wp_damage::barrier::{barrier_contacts, ImpactKind};
use wp_damage::DamageModel;

const CARS: usize = 24;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let car = wp_contracts::load_car(root.join("cars/debug_sedan.json")).expect("car");
    let mode = wp_contracts::load_mode_table(root.join("modes/sim.toml")).expect("mode");
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for n in &car.lattice.nodes {
        for k in 0..3 {
            lo[k] = lo[k].min(n.pos_m[k]);
            hi[k] = hi[k].max(n.pos_m[k]);
        }
    }
    let (c0, c1) = barrier_contacts(
        lo,
        hi,
        ImpactKind::Frontal,
        56.0 / 3.6,
        car.def.mass_kg,
        0.0,
    );
    println!(
        "lattice: {} nodes, {} beams",
        car.lattice.nodes.len(),
        car.lattice.beams.len()
    );

    let fresh = || -> Vec<DamageModel> {
        (0..CARS)
            .map(|i| {
                let mut m = DamageModel::new(&car.lattice, &car.materials);
                m.set_entity(wp_contracts::EntityId(i as u32 + 1));
                m
            })
            .collect()
    };
    let mut out = Vec::new();

    // How long one crash keeps a lattice awake.
    let mut m = DamageModel::new(&car.lattice, &car.materials);
    let mut awake = 0;
    for s in 0..2400 {
        let c: &[_] = match s {
            0 => &c0,
            1 => &c1,
            _ => &[],
        };
        m.apply_contacts(c, &mode, PHYSICS_DT, &mut out);
        if m.is_awake() {
            awake = s + 1;
        }
    }
    println!(
        "one 56 km/h crash keeps the lattice awake for {awake} steps ({:.2} s)",
        awake as f32 * PHYSICS_DT
    );

    // (a) idle
    let mut cars = fresh();
    let steps = 2400;
    let t = Instant::now();
    for _ in 0..steps {
        for m in &mut cars {
            m.apply_contacts(&[], &mode, PHYSICS_DT, &mut out);
        }
    }
    report("idle", t.elapsed().as_secs_f64(), steps);

    // (b) all 24 crash simultaneously; time the 0.25 s after impact.
    let mut cars = fresh();
    let steps = 60;
    let t = Instant::now();
    for s in 0..steps {
        let c: &[_] = match s {
            0 => &c0,
            1 => &c1,
            _ => &[],
        };
        for m in &mut cars {
            m.apply_contacts(c, &mode, PHYSICS_DT, &mut out);
        }
    }
    report(
        "all crashing (worst case)",
        t.elapsed().as_secs_f64(),
        steps,
    );

    // (c) staggered: each car crashes once every 2 s (480 steps), offset by car index.
    let mut cars = fresh();
    let steps = 4800;
    let t = Instant::now();
    for s in 0..steps {
        for (i, m) in cars.iter_mut().enumerate() {
            let phase = (s + i * 20) % 480;
            if phase == 0 {
                m.reset();
            }
            let c: &[_] = match phase {
                0 => &c0,
                1 => &c1,
                _ => &[],
            };
            m.apply_contacts(c, &mode, PHYSICS_DT, &mut out);
        }
        out.clear();
    }
    report(
        "one crash / car / 2 s (amortised)",
        t.elapsed().as_secs_f64(),
        steps,
    );
}

fn report(name: &str, secs: f64, steps: usize) {
    let per_step_car_us = secs * 1e6 / (steps * CARS) as f64;
    let per_frame_car_ms = per_step_car_us * 4.0 / 1000.0;
    println!(
        "{name:38} {per_step_car_us:8.2} µs/car/240Hz-step  {per_frame_car_ms:7.4} ms/car/60Hz-frame  (24 cars: {:.3} ms/frame)",
        per_frame_car_ms * CARS as f64
    );
}
