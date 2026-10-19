//! Harness integration tests: every scenario passes and is deterministic (in-process and in a
//! fresh process), goldens match, outputs are written, replays verify and report divergence,
//! blessing is guarded in CI, and a short fuzz passes.

use std::path::{Path, PathBuf};
use std::process::Command;

use simcli::golden;
use simcli::replay::{self, Replay};
use simcli::{find_scenarios, load_scenario, FsData, RunOptions};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn data() -> FsData {
    FsData::new(root().join("../../data"))
}

fn all_scenarios() -> Vec<PathBuf> {
    let found = find_scenarios(&root().join("scenarios"));
    assert!(found.len() >= 6, "scenario library missing");
    found
}

fn tmp(name: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&p);
    p
}

fn simcli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_simcli"))
}

#[test]
fn all_scenarios_pass_and_are_deterministic_in_process() {
    for path in all_scenarios() {
        let sc = load_scenario(&path).unwrap();
        let out = simcli::run(&sc, &data(), &RunOptions::default()).unwrap();
        assert!(
            out.summary.passed(),
            "{}: {:?}",
            path.display(),
            out.summary.failures
        );
        simcli::determinism::check_in_process(&sc, &data()).unwrap();
    }
}

#[test]
fn fresh_process_matches_in_process() {
    for path in all_scenarios() {
        let sc = load_scenario(&path).unwrap();
        let in_process = simcli::determinism::check_in_process(&sc, &data()).unwrap();
        let child = simcli().arg("hashes").arg(&path).output().unwrap();
        assert!(child.status.success());
        assert_eq!(
            String::from_utf8(child.stdout).unwrap(),
            in_process,
            "{} differs across processes",
            path.display()
        );
    }
}

#[test]
fn goldens_match() {
    let golden_root = root().join("golden");
    let mut report = String::new();
    let mut ok = true;
    for path in all_scenarios() {
        let r = golden::check_scenario(&path, &data(), &golden_root).unwrap();
        ok &= r.ok();
        report.push_str(&r.to_text());
    }
    assert!(
        ok,
        "{report}\nIf intended: `cargo run -p simcli -- goldens --bless` and commit with [bless]"
    );
}

#[test]
fn run_writes_all_outputs() {
    let out = tmp("outputs");
    let status = simcli()
        .args(["run", "scenarios/smoke/straight_line.ron", "--out"])
        .arg(&out)
        .current_dir(root())
        .status()
        .unwrap();
    assert!(status.success());
    let dir = out.join("smoke_straight_line");
    for f in simcli::output::OUTPUT_FILES {
        let len = std::fs::metadata(dir.join(f)).unwrap().len();
        assert!(len > 0 || f == "events.jsonl", "{f} empty");
    }
    let csv = std::fs::read_to_string(dir.join("metrics.csv")).unwrap();
    assert!(csv.starts_with("t_s,tick,entity,"));
    assert_eq!(csv.lines().count(), 1 + 14 * 60, "60 Hz samples for 14 s");
    let summary: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("summary.json")).unwrap()).unwrap();
    assert_eq!(summary["name"], "smoke_straight_line");
    let replay = Replay::decode(&std::fs::read(dir.join("replay.bin")).unwrap()).unwrap();
    assert!(replay::verify(&replay, &data()).unwrap().matched);
}

#[test]
fn compare_flag_detects_mismatch() {
    let dir = tmp("compare");
    std::fs::create_dir_all(&dir).unwrap();
    let golden_file = dir.join("hashes.txt");
    let good = root().join("golden/smoke_straight_line/hashes.txt");
    let ok = simcli()
        .args([
            "run",
            "scenarios/smoke/straight_line.ron",
            "--no-write",
            "--compare",
        ])
        .arg(&good)
        .current_dir(root())
        .status()
        .unwrap();
    assert!(ok.success());
    let text = std::fs::read_to_string(&good).unwrap();
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    lines[3] = format!("{} 0000000000000000", lines[3].split(' ').next().unwrap());
    std::fs::write(&golden_file, lines.join("\n")).unwrap();
    let bad = simcli()
        .args([
            "run",
            "scenarios/smoke/straight_line.ron",
            "--no-write",
            "--compare",
        ])
        .arg(&golden_file)
        .current_dir(root())
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad.stdout).contains("MISMATCH at tick 720"));
}

#[test]
fn replay_divergence_report_names_tick_and_entity() {
    let sc = load_scenario(&root().join("scenarios/golden/tbone.ron")).unwrap();
    let rec = simcli::run(
        &sc,
        &data(),
        &RunOptions {
            record: true,
            ..Default::default()
        },
    )
    .unwrap();
    let golden = Replay::decode(&rec.replay.encode()).unwrap();
    assert!(replay::verify(&golden, &data()).unwrap().matched);

    // Perturb: give the striker a different input from tick 100 on.
    let mut changed = golden.clone();
    changed.inputs.retain(|e| !(e.entity == 1 && e.tick >= 100));
    let mut input = changed
        .inputs
        .iter()
        .rev()
        .find(|e| e.entity == 1)
        .unwrap()
        .input;
    input.steer = 0.5;
    let at = changed
        .inputs
        .iter()
        .position(|e| e.tick > 100)
        .unwrap_or(changed.inputs.len());
    changed.inputs.insert(
        at,
        simcli::replay::InputEvent {
            tick: 100,
            entity: 1,
            input,
        },
    );
    // Verify the perturbed run against the original golden hashes.
    let out = simcli::run(
        &sc,
        &data(),
        &RunOptions {
            record: true,
            input_track: Some(changed.inputs.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let report = replay::compare(&sc, &golden, &out.replay);
    assert!(!report.matched);
    assert_eq!(report.first_divergent_tick, Some(101));
    assert_eq!(report.divergent_entities, vec!["striker".to_string()]);
    assert!(report
        .final_diff
        .iter()
        .any(|d| d.entity == "striker" && d.pos_delta_m > 0.1));
    assert!(report.to_text().contains("first divergent tick: 101"));
}

#[test]
fn bless_is_refused_in_ci_without_tag() {
    let g = tmp("bless_guard");
    let refused = simcli()
        .args(["goldens", "--bless", "--golden"])
        .arg(&g)
        .env("CI", "true")
        .env("WP_COMMIT_MESSAGE", "wp_vehicle: tweak tires")
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(2));
    assert!(!g.exists());
    let allowed = simcli()
        .args(["goldens", "--bless", "--golden"])
        .arg(&g)
        .env("CI", "true")
        .env("WP_COMMIT_MESSAGE", "wp_vehicle: tweak tires [bless]")
        .output()
        .unwrap();
    assert!(allowed.status.success());
    assert!(g.join("smoke_straight_line/hashes.txt").exists());
    assert!(g.join("golden_tbone.replay.bin").exists());
}

#[test]
fn fuzz_alias_runs() {
    let out = simcli()
        .args([
            "--fuzz",
            "--cases",
            "3",
            "--case-seconds",
            "2",
            "--seed",
            "7",
        ])
        .current_dir(root())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("fuzz ok"));
}

#[test]
fn scenarios_md_example_parses_and_passes() {
    let md = std::fs::read_to_string(root().join("SCENARIOS.md")).unwrap();
    let start = md.rfind("```ron").expect("example block") + "```ron".len();
    let end = start + md[start..].find("```").unwrap();
    let sc = simcli::Scenario::from_ron("SCENARIOS.md", &md[start..end]).unwrap();
    let out = simcli::run(&sc, &data(), &RunOptions::default()).unwrap();
    assert!(out.summary.passed(), "{:?}", out.summary.failures);
}
