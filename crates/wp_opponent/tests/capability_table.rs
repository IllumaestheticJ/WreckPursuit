//! The pre-measured capability table (`wp_opponent::capability_table`) matches fresh
//! measurements of every car in `data/cars`. `WP_WRITE_CAPS=1` regenerates it.

use std::path::{Path, PathBuf};

use wp_contracts::{load_car, LoadedCar};
use wp_opponent::capability_table::{fingerprint, lookup, TABLE};
use wp_opponent::CarCapability;

fn cars() -> Vec<LoadedCar> {
    let dir: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/cars");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    paths.iter().map(|p| load_car(p).unwrap()).collect()
}

#[test]
#[cfg_attr(debug_assertions, ignore = "measures every car (slow in debug)")]
fn table_matches_fresh_measurements() {
    let write = std::env::var("WP_WRITE_CAPS").is_ok();
    let mut src = String::new();
    let mut stale = Vec::new();
    for car in cars() {
        let c = CarCapability::measure(&car);
        let (m, cda, tq, rl) = fingerprint(&car);
        src.push_str(&format!(
            "    CapRow {{\n        id: \"{}\",\n        mass_kg: {:?},\n        drag_cd_a_m2: {:?},\n        peak_torque_nm: {:?},\n        redline_rpm: {:?},\n        max_lat_g: {:?},\n        brake_g: {:?},\n        accel_mps2: {:?},\n        top_speed_mps: {:?},\n    }},\n",
            car.def.id, m, cda, tq, rl, c.max_lat_g, c.brake_g, c.accel_mps2, c.top_speed_mps
        ));
        let ok = lookup(&car).is_some_and(|r| {
            let near = |a: f32, b: f32| (a - b).abs() <= 0.02 * b.abs();
            near(r.max_lat_g, c.max_lat_g)
                && near(r.brake_g, c.brake_g)
                && near(r.accel_mps2, c.accel_mps2)
                && near(r.top_speed_mps, c.top_speed_mps)
        });
        if !ok {
            stale.push(car.def.id.clone());
        }
    }
    if write {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/capability_table.rs");
        let old = std::fs::read_to_string(&path).unwrap();
        let start = old.find("pub const TABLE: &[CapRow] = &[").unwrap();
        let new = format!(
            "{}pub const TABLE: &[CapRow] = &[\n{}];\n",
            &old[..start],
            src
        );
        std::fs::write(&path, new).unwrap();
        eprintln!("wrote {} rows", TABLE.len().max(1));
        return;
    }
    assert!(
        stale.is_empty(),
        "capability table out of date for {stale:?}; regenerate with \
         WP_WRITE_CAPS=1 cargo test --release -p wp_opponent --test capability_table"
    );
}

#[test]
fn every_car_has_a_row() {
    for car in cars() {
        assert!(
            lookup(&car).is_some(),
            "{}: no capability row (or its fingerprint changed); regenerate with \
             WP_WRITE_CAPS=1 cargo test --release -p wp_opponent --test capability_table",
            car.def.id
        );
    }
}
