//! Harness integration tests: every scenario passes and is deterministic (in-process and in a
//! fresh process), goldens match (these three share one parallel pass over the scenarios), outputs are written, replays verify and report divergence,
//! blessing is guarded in CI, and a short fuzz passes.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

use simcli::golden;
use simcli::replay::{self, Replay};
use simcli::{find_scenarios, load_scenario, FsData, RunOptions, RunOutput, Scenario};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn data() -> FsData {
    FsData::new(root().join("../../data"))
}

/// Every scenario, minus `scenarios/soak/` in debug builds: soaks simulate minutes of a full
/// city and take ~200× longer unoptimised. CI's release jobs (`simcli goldens`,
/// `simcli determinism`, the wasm compare) still run them.
fn all_scenarios() -> Vec<PathBuf> {
    let found = find_scenarios(&root().join("scenarios"));
    assert!(found.len() >= 6, "scenario library missing");
    found
        .into_iter()
        .filter(|p| !(cfg!(debug_assertions) && p.components().any(|c| c.as_os_str() == "soak")))
        .collect()
}

fn tmp(name: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&p);
    p
}

fn simcli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_simcli"))
}

/// Everything the three long tests below need for one scenario, simulated once.
struct ScenarioRuns {
    path: PathBuf,
    scenario: Scenario,
    /// Two recorded in-process runs (the determinism pair; run 0 also serves expectations
    /// and goldens: the summary does not depend on `record`).
    runs: [Result<RunOutput, String>; 2],
    /// Stdout of `simcli hashes <path>` in a fresh process (records nothing).
    fresh_process: Result<String, String>,
    /// Golden replay verification (re-simulates the recorded inputs).
    replay: Result<golden::ReplayCheck, String>,
}

/// One independent simulation job of the shared pass.
#[derive(Clone, Copy)]
enum Job {
    Recorded(usize, usize),
    FreshProcess(usize),
    Replay(usize),
}

enum JobResult {
    Recorded(Box<Result<RunOutput, String>>),
    FreshProcess(Result<String, String>),
    Replay(Result<golden::ReplayCheck, String>),
}

fn panic_message(p: Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<String>()
        .cloned()
        .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "panic".into())
}

/// Rough cost of a scenario, to start the slowest jobs first.
fn cost(sc: &Scenario) -> u64 {
    let city = !matches!(sc.city, simcli::scenario::CitySpec::Flat);
    sc.ticks() * if city { 20 } else { 1 } * (1 + sc.entities.len() as u64)
}

fn run_job(job: Job, scenarios: &[(PathBuf, Scenario)]) -> JobResult {
    let catch = |f: &dyn Fn() -> Result<JobResult, String>| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
            .map_err(panic_message)
            .and_then(|r| r)
    };
    match job {
        Job::Recorded(i, _) => {
            let sc = &scenarios[i].1;
            let r = catch(&|| {
                let opts = RunOptions {
                    record: true,
                    ..Default::default()
                };
                let out = simcli::run(sc, &data(), &opts).map_err(|e| e.to_string())?;
                Ok(JobResult::Recorded(Box::new(Ok(out))))
            });
            r.unwrap_or_else(|e| JobResult::Recorded(Box::new(Err(e))))
        }
        Job::FreshProcess(i) => {
            let path = &scenarios[i].0;
            let r = (|| {
                let child = simcli()
                    .arg("hashes")
                    .arg(path)
                    .output()
                    .map_err(|e| e.to_string())?;
                if !child.status.success() {
                    return Err(format!(
                        "simcli hashes failed: {}",
                        String::from_utf8_lossy(&child.stderr)
                    ));
                }
                String::from_utf8(child.stdout).map_err(|e| e.to_string())
            })();
            JobResult::FreshProcess(r)
        }
        Job::Replay(i) => {
            let (path, sc) = &scenarios[i];
            let r = catch(&|| {
                Ok(JobResult::Replay(Ok(golden::check_replay(
                    path,
                    sc,
                    &data(),
                    &root().join("golden"),
                )
                .map_err(|e| e.to_string())?)))
            });
            r.unwrap_or_else(|e| JobResult::Replay(Err(e)))
        }
    }
}

/// Simulate every scenario once for all three long tests, spread over all cores. Each job is
/// one whole run on one thread (a sim never shares state with another), so threading cannot
/// change a hash; the two in-process runs of a scenario even run concurrently, which would
/// expose any shared global state.
fn shared_runs() -> &'static [ScenarioRuns] {
    static RUNS: OnceLock<Vec<ScenarioRuns>> = OnceLock::new();
    RUNS.get_or_init(|| {
        let scenarios: Vec<(PathBuf, Scenario)> = all_scenarios()
            .into_iter()
            .map(|p| {
                let sc = load_scenario(&p).unwrap();
                (p, sc)
            })
            .collect();
        let mut jobs = Vec::new();
        for (i, (path, _)) in scenarios.iter().enumerate() {
            jobs.push(Job::Recorded(i, 0));
            jobs.push(Job::Recorded(i, 1));
            jobs.push(Job::FreshProcess(i));
            if golden::is_replay_scenario(path) {
                jobs.push(Job::Replay(i));
            }
        }
        let job_scenario = |j: &Job| match *j {
            Job::Recorded(i, _) | Job::FreshProcess(i) | Job::Replay(i) => i,
        };
        jobs.sort_by_key(|j| std::cmp::Reverse(cost(&scenarios[job_scenario(j)].1)));

        let next = AtomicUsize::new(0);
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let mut results: Vec<(usize, JobResult)> = std::thread::scope(|s| {
            let workers: Vec<_> = (0..threads)
                .map(|_| {
                    s.spawn(|| {
                        let mut done = Vec::new();
                        loop {
                            let k = next.fetch_add(1, Ordering::Relaxed);
                            let Some(job) = jobs.get(k) else { break };
                            done.push((k, run_job(*job, &scenarios)));
                        }
                        done
                    })
                })
                .collect();
            workers
                .into_iter()
                .flat_map(|w| w.join().expect("worker thread"))
                .collect()
        });
        results.sort_by_key(|(k, _)| *k);

        let mut out: Vec<ScenarioRuns> = scenarios
            .into_iter()
            .map(|(path, scenario)| ScenarioRuns {
                path,
                scenario,
                runs: [Err("not run".into()), Err("not run".into())],
                fresh_process: Err("not run".into()),
                replay: Ok((None, None)),
            })
            .collect();
        for (k, r) in results {
            match (jobs[k], r) {
                (Job::Recorded(i, n), JobResult::Recorded(r)) => out[i].runs[n] = *r,
                (Job::FreshProcess(i), JobResult::FreshProcess(r)) => out[i].fresh_process = r,
                (Job::Replay(i), JobResult::Replay(r)) => out[i].replay = r,
                _ => unreachable!("job and result kinds match"),
            }
        }
        out
    })
}

/// The in-process determinism verdict for one scenario: its `hashes.txt` text if both
/// recorded runs succeeded and are identical.
fn in_process(r: &ScenarioRuns) -> Result<String, String> {
    let a = r.runs[0].as_ref().map_err(Clone::clone)?;
    let b = r.runs[1].as_ref().map_err(Clone::clone)?;
    simcli::determinism::compare_runs(&r.scenario, a, b)
}

#[test]
fn all_scenarios_pass_and_are_deterministic_in_process() {
    let mut failures = Vec::new();
    for r in shared_runs() {
        match &r.runs[0] {
            Ok(out) if !out.summary.passed() => {
                failures.push(format!("{}: {:?}", r.path.display(), out.summary.failures))
            }
            Ok(_) => {}
            Err(e) => failures.push(format!("{}: {e}", r.path.display())),
        }
        if let Err(e) = in_process(r) {
            failures.push(format!("{}: {e}", r.path.display()));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn fresh_process_matches_in_process() {
    let mut failures = Vec::new();
    for r in shared_runs() {
        match (in_process(r), &r.fresh_process) {
            (Ok(a), Ok(b)) if a == *b => {}
            (Ok(_), Ok(_)) => {
                failures.push(format!("{} differs across processes", r.path.display()))
            }
            (Err(e), _) => failures.push(format!("{}: {e}", r.path.display())),
            (_, Err(e)) => failures.push(format!("{}: {e}", r.path.display())),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn goldens_match() {
    let golden_root = root().join("golden");
    let mut report = String::new();
    let mut ok = true;
    for r in shared_runs() {
        let out = r.runs[0].as_ref().unwrap();
        let replay = r.replay.clone().unwrap();
        let g = golden::check_run(&r.path, &r.scenario, out, replay, &golden_root);
        ok &= g.ok();
        report.push_str(&g.to_text());
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
    // One golden scenario is enough to exercise the guard and the files it writes.
    let scenarios = root().join("scenarios/golden/tbone.ron");
    let refused = simcli()
        .args(["goldens", "--bless", "--golden"])
        .arg(&g)
        .arg("--scenarios")
        .arg(&scenarios)
        .env("CI", "true")
        .env("WP_COMMIT_MESSAGE", "wp_vehicle: tweak tires")
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(2));
    assert!(!g.exists());
    let allowed = simcli()
        .args(["goldens", "--bless", "--golden"])
        .arg(&g)
        .arg("--scenarios")
        .arg(&scenarios)
        .env("CI", "true")
        .env("WP_COMMIT_MESSAGE", "wp_vehicle: tweak tires [bless]")
        .output()
        .unwrap();
    assert!(allowed.status.success());
    assert!(g.join("golden_tbone/hashes.txt").exists());
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
