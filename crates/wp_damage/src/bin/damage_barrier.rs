//! Crash-barrier runner and calibration sweep.
//!
//! ```text
//! cargo run --release -p wp_damage --bin damage-barrier -- apps/simcli/scenarios/damage/frontal_sim.ron
//! cargo run --release -p wp_damage --bin damage-barrier -- --sweep [car] [--metrics a,b,c]
//! ```
//! Scenario mode prints one line per run and exits 1 on any failed check. `--sweep` prints a
//! table of headline metrics for every impact kind × speed × mode (the calibration view).

use std::path::{Path, PathBuf};

use wp_contracts::DamageSim;
use wp_damage::barrier::{load_scenario, run_barrier, run_scenario, ImpactKind};

fn data_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--sweep") {
        let car_id = args
            .get(1)
            .filter(|a| !a.starts_with("--"))
            .cloned()
            .unwrap_or_else(|| "debug_sedan".into());
        let metrics: Vec<String> = args
            .iter()
            .position(|a| a == "--metrics")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.split(',').map(str::to_string).collect())
            .unwrap_or_else(|| {
                [
                    "crush_m",
                    "cabin_intrusion_m",
                    "plastic_energy_kj",
                    "health",
                    "broken_beams",
                    "detached_parts",
                    "repair_total",
                ]
                .map(String::from)
                .to_vec()
            });
        sweep(&car_id, &metrics);
        return;
    }
    if args.first().map(String::as_str) == Some("--detail") {
        // --detail <car> <sim|arcade> <Frontal|Offset|Side|Rear> <km/h>
        detail(
            &args[1],
            &args[2],
            &args[3],
            args[4].parse().expect("speed"),
        );
        return;
    }
    let mut failed = false;
    for path in args.iter().filter(|a| !a.starts_with("--")) {
        let sc = load_scenario(Path::new(path)).unwrap_or_else(|e| panic!("{e}"));
        let report = run_scenario(&sc, &data_root()).unwrap_or_else(|e| panic!("{e}"));
        println!("{} hash {:016x}", report.name, report.hash);
        for r in &report.results {
            println!(
                "  {:5.1} km/h crush {:.3} m  cabin {:.3} m  health {:.2}  plastic {:.1} kJ  failed {:?}",
                r.speed_kmh,
                r.metrics["crush_m"],
                r.metrics["cabin_intrusion_m"],
                r.metrics["health"],
                r.metrics["plastic_energy_kj"],
                r.failed
            );
        }
        for f in &report.failures {
            eprintln!("FAIL {f}");
            failed = true;
        }
    }
    if failed {
        std::process::exit(1);
    }
}

fn detail(car_id: &str, mode_name: &str, kind: &str, speed: f32) {
    let root = data_root();
    let car = wp_contracts::load_car(root.join("cars").join(format!("{car_id}.json")))
        .expect("car loads");
    let file = if mode_name == "arcade" {
        "arcade.toml"
    } else {
        "sim.toml"
    };
    let mode = wp_contracts::load_mode_table(root.join("modes").join(file)).expect("mode loads");
    let (kind, other) = match kind {
        "Frontal" => (ImpactKind::Frontal, 0.0),
        "Offset" => (ImpactKind::Offset, 0.0),
        "Side" => (ImpactKind::Side, car.def.mass_kg),
        _ => (ImpactKind::Rear, 0.0),
    };
    let (m, r) = run_barrier(&car, &mode, kind, speed, other, 0.5, 1);
    for (k, v) in &r.metrics {
        println!("{k:28} {v:10.4}");
    }
    println!("-- log");
    for e in &r.log {
        println!("{:8.4} s  {:?}", e.time_s, e.kind);
    }
    println!("-- most displaced nodes along the impact normal");
    let n = kind.normal();
    let mut d: Vec<(f32, usize)> = (0..m.node_positions().len())
        .map(|i| {
            let p = m.node_positions()[i];
            let r0 = m.rest_position(i);
            (wp_damage::vec3::dot(wp_damage::vec3::sub(p, r0), n), i)
        })
        .collect();
    d.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (v, i) in d.iter().take(12) {
        let nd = &car.lattice.nodes[*i];
        println!(
            "node {i:4} {:?} rest {:?} disp {v:.3} detached {}",
            nd.region,
            nd.pos_m,
            m.node_detached(*i)
        );
    }
    println!("-- plastic energy by region (kJ)");
    for reg in wp_damage::ALL_REGIONS {
        let e = m.region_plastic_energy_j(reg);
        if e > 1.0 {
            println!("{reg:?}: {:.2}", e * 1e-3);
        }
    }
}

fn sweep(car_id: &str, metrics: &[String]) {
    let root = data_root();
    let car = wp_contracts::load_car(root.join("cars").join(format!("{car_id}.json")))
        .expect("car loads");
    for (mode_name, file) in [("sim", "sim.toml"), ("arcade", "arcade.toml")] {
        let mode =
            wp_contracts::load_mode_table(root.join("modes").join(file)).expect("mode loads");
        for (kind, other) in [
            (ImpactKind::Frontal, 0.0),
            (ImpactKind::Offset, 0.0),
            (ImpactKind::Side, car.def.mass_kg),
            (ImpactKind::Rear, 0.0),
        ] {
            println!("== {car_id} {mode_name} {kind:?} (other mass {other} kg)");
            print!("{:>6}", "km/h");
            for m in metrics {
                print!(" {:>14}", m.chars().take(14).collect::<String>());
            }
            println!("  failed");
            for speed in [15.0, 25.0, 40.0, 56.0, 80.0] {
                let (_, r) = run_barrier(&car, &mode, kind, speed, other, 0.5, 1);
                print!("{speed:>6.0}");
                for m in metrics {
                    print!(" {:>14.3}", r.metrics.get(m).copied().unwrap_or(f32::NAN));
                }
                println!("  {:?}", r.failed);
            }
        }
    }
}
