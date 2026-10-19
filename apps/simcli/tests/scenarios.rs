//! Runs every scenario under `scenarios/` twice: expectations must pass and both runs must
//! produce the same state hash (determinism).

use std::path::{Path, PathBuf};

fn scenarios(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            // `damage/` holds wp_damage barrier scenarios (a different format), run by
            // `wp_damage/tests/barrier_scenarios.rs`.
            if p.file_name().and_then(|n| n.to_str()) == Some("damage") {
                continue;
            }
            scenarios(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("ron") {
            out.push(p);
        }
    }
}

#[test]
fn all_scenarios_pass_and_are_deterministic() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let data = root.join("../../data");
    let mut found = Vec::new();
    scenarios(&root.join("scenarios"), &mut found);
    found.sort();
    assert!(!found.is_empty());
    for path in found {
        let sc = simcli::load_scenario(&path).unwrap();
        let a = simcli::run_scenario(&sc, &data).unwrap();
        let b = simcli::run_scenario(&sc, &data).unwrap();
        assert!(
            a.failures.is_empty(),
            "{}: {:?}",
            path.display(),
            a.failures
        );
        assert_eq!(
            a.state_hash,
            b.state_hash,
            "{} not deterministic",
            path.display()
        );
    }
}
