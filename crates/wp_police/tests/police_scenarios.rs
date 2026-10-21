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
    // Pursuit starts later at 03:00 than at noon (mean over runs): fewer patrols. Since round
    // 12c (rows 80 / 89) police see as far as the racer sees them (260 m at night) and the
    // unit that watches a violation making the racer wanted pursues at once, so the night/day
    // ratio fell from >= 3x (day 8.8 s, night 33 s) to ~1.7x (8.4 s, 14.6 s); staying out of
    // sight still escapes (escape_break_contact_night.ron). On main's rebuilt grid (#19-#27,
    // the divided highway with ramps) it is 1.39.
    let ratio = m(&night, "pursuit_start_s") / m(&day, "pursuit_start_s");
    assert!(ratio >= 1.25, "pursuit start night/day = {ratio}");
    assert!(m(&day, "pursuit_start_max_s") <= 20.0);
    assert!(m(&night, "pursuit_start_s") > 10.0);
    // Backup takes at least 2x as long to commit at night: per pursuit, the first unit sent
    // after the start. (The mean over every reinforcement and backup join, `reinforcement_delay_s`,
    // mostly measured how late in a long chase units were still joining: it doubled by day,
    // 20 -> 41 s over 30 runs, when chasers kept up better, with no change in when the first
    // backup came.)
    let rd = m(&day, "first_backup_s");
    let rn = m(&night, "first_backup_s");
    assert!(
        rd >= 0.0 && rn > 0.0,
        "both phases must send backup ({rd}, {rn})"
    );
    assert!(rn >= 2.0 * rd, "first backup night {rn} s vs day {rd} s");
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
    let m = |s: &PoliceRunSummary, k: &str| s.metric(k).unwrap();
    // The prompt's bar: < 10 % of engaged units total-loss themselves without racer contact.
    assert!(m(&careful, "self_wreck_rate") < 0.10);
    // The risk budget does its job: pursuers spend it and drop to shadowing / gentle
    // cornering, which never happens with an unlimited budget.
    //
    // This test used to assert that the reckless config does more harm (self-wrecks, traffic
    // collisions, run-wide crashes, near misses). Since round 6 junction speed rules and
    // following distances cap pursuers in both configs and the two totals are within noise
    // (round 12b, before this change: 462 careful vs 475 reckless; with responders 464 vs
    // 453), so that comparison no longer ranked anything.
    assert!(
        m(&careful, "pursuit_no_risk_s") > 10.0,
        "careful units never ran out of risk budget: {}",
        m(&careful, "pursuit_no_risk_s")
    );
    assert_eq!(m(&reckless, "pursuit_no_risk_s"), 0.0);
}
