//! Runs every barrier scenario in `apps/simcli/scenarios/damage/` twice: all checks must pass
//! (ranges, failures, total loss, ordering, monotone crush vs speed, node containment) and
//! both runs must produce identical hashes.

use std::path::{Path, PathBuf};

fn scenario_files() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/simcli/scenarios/damage");
    let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("ron"))
        .collect();
    v.sort();
    v
}

#[test]
fn all_barrier_scenarios_pass_and_are_deterministic() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let files = scenario_files();
    assert!(
        files.len() >= 8,
        "expected frontal/offset/side/rear x both modes"
    );
    let mut failures = Vec::new();
    for path in files {
        let sc = wp_damage::barrier::load_scenario(&path).unwrap();
        let a = wp_damage::barrier::run_scenario(&sc, &data).unwrap();
        let b = wp_damage::barrier::run_scenario(&sc, &data).unwrap();
        assert_eq!(a.hash, b.hash, "{} not deterministic", path.display());
        failures.extend(a.failures);
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn every_kind_and_mode_is_covered() {
    use std::collections::BTreeSet;
    let mut seen = BTreeSet::new();
    for path in scenario_files() {
        let sc = wp_damage::barrier::load_scenario(&path).unwrap();
        let speeds: Vec<f32> = sc.runs.iter().map(|r| r.speed_kmh).collect();
        if sc.car == "debug_sedan" {
            assert_eq!(speeds, vec![15.0, 25.0, 40.0, 56.0, 80.0], "{}", sc.name);
            assert!(sc.monotone.iter().any(|m| m == "crush_m"), "{}", sc.name);
            seen.insert(format!("{:?}/{:?}", sc.impact, sc.mode));
        }
    }
    assert_eq!(seen.len(), 8, "{seen:?}");
}
