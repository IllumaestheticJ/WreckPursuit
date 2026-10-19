//! Criterion benches. Every bench measures **one physics tick** (or one snapshot op); the perf
//! gate scales to per-frame numbers. Run: `cargo bench -p wp_bench` (or `just bench`).

use std::hint::black_box;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use wp_contracts::DamageSim;

fn sim_only(c: &mut Criterion) {
    let mut g = c.benchmark_group("sim_only");
    for n in [1usize, 8, 24, 64] {
        let mut sim = wp_bench::sim_with_vehicles(n);
        g.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                sim.step_fixed();
                black_box(sim.drain_events());
            })
        });
    }
    g.finish();
}

fn city(c: &mut Criterion) {
    // 24 vehicles + world step. Traffic agents arrive with prompt 05: until `wp_world` exposes a
    // traffic API this measures the world clock/signals only; the bench id stays fixed so the
    // baseline history is continuous.
    let mut sim = wp_bench::sim_with_vehicles(24);
    c.bench_function("city/24v_400traffic", |b| {
        b.iter(|| {
            sim.step_fixed();
            black_box(sim.drain_events());
        })
    });
}

fn lattice(c: &mut Criterion) {
    let car = wp_bench::car("debug_sedan");
    let mode =
        wp_contracts::load_mode_table(wp_bench::data_root().join("modes/sim.toml")).expect("mode");
    let mut dm = wp_damage::DamageModel::new(&car.lattice, &car.materials);
    let contacts = wp_bench::frontal_contacts(8);
    let mut out = Vec::new();
    c.bench_function("lattice/solver_8_contacts", |b| {
        b.iter(|| {
            dm.apply_contacts(
                black_box(&contacts),
                &mode,
                wp_contracts::PHYSICS_DT,
                &mut out,
            );
            out.clear();
            black_box(dm.node_positions());
        })
    });
}

fn snapshots(c: &mut Criterion) {
    let sim = wp_bench::sim_with_vehicles(24);
    let msg = wp_bench::snapshot(&sim);
    let bytes = wp_net::encode_server(&msg);
    let mut g = c.benchmark_group("snapshot");
    g.bench_function("encode_24", |b| {
        b.iter(|| black_box(wp_net::encode_server(&msg)))
    });
    g.bench_function("decode_24", |b| {
        b.iter(|| black_box(wp_net::decode_server(&bytes).expect("decodes")))
    });
    g.finish();
}

fn config() -> Criterion {
    Criterion::default()
        .sample_size(30)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(2))
}

criterion_group! {
    name = benches;
    config = config();
    targets = sim_only, city, lattice, snapshots
}
criterion_main!(benches);
