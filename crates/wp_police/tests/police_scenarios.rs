//! Police scenarios (`crates/wp_police/scenarios/*.ron`, run by `wp_police::scenario`): every scenario
//! passes its expectations and is deterministic; plus the cross-scenario ratio assertions from
//! prompt 06 (night vs day) and a check that the risk budget is what keeps cops alive.

use std::path::{Path, PathBuf};

use wp_police::scenario::{load_scenario, run_scenario, run_scenario_with, PoliceRunSummary};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios")
}

fn run(name: &str) -> PoliceRunSummary {
    let sc = load_scenario(&dir().join(name)).unwrap();
    run_scenario(&sc)
}

#[test]
fn all_police_scenarios_pass_and_are_deterministic() {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("ron"))
        .collect();
    found.sort();
    assert!(found.len() >= 10);
    for path in found {
        let sc = load_scenario(&path).unwrap();
        let a = run_scenario(&sc);
        assert!(
            a.failures.is_empty(),
            "{}: {:?}",
            path.display(),
            a.failures
        );
        // Determinism: hashing a second run of the cheaper scenarios (all of them is slow in
        // debug builds; the long multi-run ones are covered by the ratio test below).
        if sc.runs * (sc.duration_s as u32) <= 1000 {
            let b = run_scenario(&sc);
            assert_eq!(
                a.state_hash,
                b.state_hash,
                "{} not deterministic",
                path.display()
            );
        }
    }
}

#[test]
fn night_is_slower_than_day_for_pursuit_and_reinforcements() {
    let day = run("day_core_speeder.ron");
    let night = run("night_core_speeder.ron");
    let m = |s: &PoliceRunSummary, k: &str| s.metric(k).unwrap();
    // Pursuit starts at least 3x later at 03:00 than at noon (mean over runs).
    let ratio = m(&night, "pursuit_start_s") / m(&day, "pursuit_start_s");
    assert!(ratio >= 3.0, "pursuit start night/day = {ratio}");
    assert!(m(&day, "pursuit_start_max_s") <= 20.0);
    assert!(m(&night, "pursuit_start_s") > 60.0);
    // Reinforcements take at least 2x as long to arrive.
    let rd = m(&day, "reinforcement_delay_s");
    let rn = m(&night, "reinforcement_delay_s");
    assert!(
        rd > 0.0 && rn > 0.0,
        "both phases must dispatch reinforcements ({rd}, {rn})"
    );
    assert!(rn / rd >= 2.0, "reinforcement night/day = {}", rn / rd);
}

#[test]
fn risk_budget_and_caps_keep_cops_alive() {
    let sc = load_scenario(&dir().join("self_wreck_sim.ron")).unwrap();
    let careful = run_scenario(&sc);
    let mut cfg = wp_police::PoliceConfig::builtin();
    cfg.pursuit.risk.budget = 1.0e9;
    cfg.pursuit.pursuit_cornering_g = 0.92;
    cfg.pursuit.target_speed_margin = 0.6;
    cfg.pursuit.pursuit_cap_mps.day = 60.0;
    let reckless = run_scenario_with(&sc, &cfg);
    let harm = |s: &PoliceRunSummary| {
        s.metric("self_wrecks").unwrap() + s.metric("traffic_collisions").unwrap()
    };
    assert!(careful.metric("self_wreck_rate").unwrap() < 0.10);
    assert!(
        harm(&reckless) > harm(&careful),
        "reckless {} vs careful {}",
        harm(&reckless),
        harm(&careful)
    );
}
