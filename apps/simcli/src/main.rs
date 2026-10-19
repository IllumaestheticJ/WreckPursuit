//! `simcli` — headless scenario runner, determinism suite, golden checks and fuzzer.
//! Run `simcli help` for usage; the full guide is `apps/simcli/README.md`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use simcli::fuzz::FuzzConfig;
use simcli::golden;
use simcli::replay::{self, Replay};
use simcli::{find_scenarios, load_scenario, FsData, MemData, RunOptions, Selection};

const USAGE: &str = "\
simcli — WreckPursuit headless harness

USAGE:
  simcli [run] <scenario.ron|dir>... [--data DIR] [--out DIR] [--systems LIST] [--strict]
                [--compare golden/<name>/hashes.txt] [--bless] [--golden DIR] [--no-write]
  simcli hashes <scenario.ron> [--data DIR] [--systems LIST]     print hashes.txt to stdout
  simcli determinism <scenario.ron|dir>... [--data DIR] [--no-fresh]
  simcli replay <file.replay.bin>... [--data DIR] [--json]
  simcli goldens [--bless] [--scenarios DIR] [--golden DIR] [--data DIR]
  simcli fuzz [--minutes M | --cases N] [--seed S] [--case-seconds S] [--max-vehicles N] [--out DIR]
  simcli --fuzz --minutes 5                                       (alias of `fuzz`)
  simcli bundle [--data DIR]                                      data root as JSON (WASM runner)

LIST is comma-separated from vehicle,damage,world,police,net (or `all`).
Exit codes: 0 pass, 1 expectation/golden/determinism/fuzz failure, 2 usage or load error.";

struct Args {
    positional: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

impl Args {
    /// Flags that take a value; everything else starting with `--` is a switch.
    const VALUED: [&'static str; 13] = [
        "--data",
        "--out",
        "--systems",
        "--compare",
        "--golden",
        "--scenarios",
        "--minutes",
        "--cases",
        "--seed",
        "--case-seconds",
        "--max-vehicles",
        "--replay-out",
        "--format",
    ];

    fn parse(raw: Vec<String>) -> Result<Self, String> {
        let mut positional = Vec::new();
        let mut flags = Vec::new();
        let mut it = raw.into_iter();
        while let Some(a) = it.next() {
            if a.starts_with("--") {
                if Self::VALUED.contains(&a.as_str()) {
                    let v = it.next().ok_or_else(|| format!("{a} needs a value"))?;
                    flags.push((a, Some(v)));
                } else {
                    flags.push((a, None));
                }
            } else {
                positional.push(a);
            }
        }
        Ok(Self { positional, flags })
    }

    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(f, _)| f == name)
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .rev()
            .find(|(f, _)| f == name)
            .and_then(|(_, v)| v.as_deref())
    }

    fn num<T: std::str::FromStr>(&self, name: &str) -> Result<Option<T>, String> {
        self.get(name)
            .map(|v| v.parse().map_err(|_| format!("{name}: bad number {v:?}")))
            .transpose()
    }
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn data_root(args: &Args) -> PathBuf {
    match args.get("--data") {
        Some(d) => PathBuf::from(d),
        None if Path::new("data/cars").is_dir() => PathBuf::from("data"),
        None => manifest_dir().join("../../data"),
    }
}

fn golden_root(args: &Args) -> PathBuf {
    args.get("--golden")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest_dir().join("golden"))
}

fn systems(args: &Args) -> Result<Option<Selection>, String> {
    args.get("--systems").map(Selection::parse).transpose()
}

fn scenario_paths(args: &Args) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for p in &args.positional {
        let found = find_scenarios(Path::new(p));
        if found.is_empty() {
            return Err(format!("no scenarios at {p}"));
        }
        out.extend(found);
    }
    if out.is_empty() {
        return Err("no scenario given".into());
    }
    Ok(out)
}

fn main() -> ExitCode {
    let mut raw: Vec<String> = std::env::args().skip(1).collect();
    if let Some(i) = raw.iter().position(|a| a == "--fuzz") {
        raw.remove(i);
        raw.insert(0, "fuzz".into());
    }
    let cmd = match raw.first().map(String::as_str) {
        Some(
            c @ ("run" | "hashes" | "determinism" | "replay" | "goldens" | "fuzz" | "bundle"
            | "help"),
        ) => {
            let c = c.to_string();
            raw.remove(0);
            c
        }
        Some("-h" | "--help") | None => "help".into(),
        Some(_) => "run".into(),
    };
    let args = match Args::parse(raw) {
        Ok(a) => a,
        Err(e) => return usage_error(&e),
    };
    let result = match cmd.as_str() {
        "run" => cmd_run(&args),
        "hashes" => cmd_hashes(&args),
        "determinism" => cmd_determinism(&args),
        "replay" => cmd_replay(&args),
        "goldens" => cmd_goldens(&args),
        "fuzz" => cmd_fuzz(&args),
        "bundle" => cmd_bundle(&args),
        _ => {
            println!("{USAGE}");
            Ok(true)
        }
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => usage_error(&e),
    }
}

fn usage_error(e: &str) -> ExitCode {
    eprintln!("error: {e}\n\n{USAGE}");
    ExitCode::from(2)
}

/// `run`: run scenarios, write outputs, compare/bless goldens. `Ok(false)` = some failed.
fn cmd_run(args: &Args) -> Result<bool, String> {
    let paths = scenario_paths(args)?;
    let data = FsData::new(data_root(args));
    let out_root = PathBuf::from(args.get("--out").unwrap_or("out"));
    let opts = RunOptions {
        systems: systems(args)?,
        record: !args.has("--no-write"),
        input_track: None,
        strict: args.has("--strict"),
    };
    if args.get("--compare").is_some() && paths.len() != 1 {
        return Err("--compare takes exactly one scenario".into());
    }
    if args.has("--bless") {
        golden::bless_allowed()?;
    }
    let mut all_ok = true;
    for path in paths {
        let sc = load_scenario(&path).map_err(|e| e.to_string())?;
        let out = simcli::run(&sc, &data, &opts).map_err(|e| e.to_string())?;
        let s = &out.summary;
        println!(
            "{} {} ticks={} hash={} events={}",
            if s.passed() { "PASS" } else { "FAIL" },
            s.name,
            s.ticks,
            s.final_hash,
            s.events_total
        );
        for w in &s.warnings {
            println!("  warning: {w}");
        }
        for f in &s.failures {
            println!("  failed: {f}");
        }
        for p in s.pending.iter().filter(|p| !p.passed) {
            println!(
                "  pending (not failing): {} (actual {})",
                p.expect, p.actual
            );
        }
        all_ok &= s.passed();
        if opts.record {
            let dir = simcli::output::write_outputs(&out_root, &out).map_err(|e| e.to_string())?;
            println!("  wrote {}", dir.display());
        }
        if let Some(golden_file) = args.get("--compare") {
            let golden_text = std::fs::read_to_string(golden_file)
                .map_err(|e| format!("--compare {golden_file}: {e}"))?;
            let diff = golden::compare_hashes(&golden_text, &out.hashes_txt());
            for n in &diff.notes {
                println!("  compare note: {n}");
            }
            match (&diff.first_mismatch, diff.matched) {
                (_, true) => println!("  compare: hashes match {golden_file}"),
                (Some((tick, g, n)), _) => {
                    println!("  compare: MISMATCH at tick {tick}: golden {g}, now {n}");
                    println!("  (for the diverging entity and magnitude run `simcli replay` on the golden replay)");
                }
                (None, _) => println!("  compare: MISMATCH (different number of checkpoints)"),
            }
            all_ok &= diff.matched;
        }
        if args.has("--bless") {
            for p in golden::bless_scenario(&path, &data, &golden_root(args))
                .map_err(|e| e.to_string())?
            {
                println!("  blessed {}", p.display());
            }
        }
    }
    Ok(all_ok)
}

/// `hashes`: print `hashes.txt` for one scenario (used by the fresh-process check).
fn cmd_hashes(args: &Args) -> Result<bool, String> {
    let paths = scenario_paths(args)?;
    let data = FsData::new(data_root(args));
    for path in paths {
        let sc = load_scenario(&path).map_err(|e| e.to_string())?;
        let out = simcli::run(
            &sc,
            &data,
            &RunOptions {
                systems: systems(args)?,
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())?;
        print!("{}", out.hashes_txt());
    }
    Ok(true)
}

/// `determinism`: twice in-process, once in a fresh process; all hashes must match.
fn cmd_determinism(args: &Args) -> Result<bool, String> {
    let paths = scenario_paths(args)?;
    let root = data_root(args);
    let data = FsData::new(&root);
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut ok = true;
    for path in paths {
        let sc = load_scenario(&path).map_err(|e| e.to_string())?;
        let in_process = match simcli::determinism::check_in_process(&sc, &data) {
            Ok(h) => h,
            Err(e) => {
                println!("FAIL {e}");
                ok = false;
                continue;
            }
        };
        if !args.has("--no-fresh") {
            let child = std::process::Command::new(&exe)
                .arg("hashes")
                .arg(&path)
                .arg("--data")
                .arg(&root)
                .output()
                .map_err(|e| format!("spawn {}: {e}", exe.display()))?;
            let fresh = String::from_utf8_lossy(&child.stdout);
            if !child.status.success() || fresh != in_process {
                let diff = golden::compare_hashes(&in_process, &fresh);
                println!(
                    "FAIL {}: fresh process differs (first mismatch {:?}) {}",
                    sc.name,
                    diff.first_mismatch,
                    String::from_utf8_lossy(&child.stderr)
                );
                ok = false;
                continue;
            }
        }
        println!("ok   {} deterministic", sc.name);
    }
    Ok(ok)
}

/// `replay`: verify replay files and print the divergence report.
fn cmd_replay(args: &Args) -> Result<bool, String> {
    if args.positional.is_empty() {
        return Err("no replay file given".into());
    }
    let data = FsData::new(data_root(args));
    let mut ok = true;
    for file in &args.positional {
        let bytes = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
        let golden = Replay::decode(&bytes).map_err(|e| format!("{file}: {e}"))?;
        let report = replay::verify(&golden, &data).map_err(|e| format!("{file}: {e}"))?;
        if args.has("--json") {
            println!(
                "{}",
                serde_json::to_string_pretty(&report).expect("report serializes")
            );
        } else {
            println!("{}", report.to_text().trim_end());
        }
        ok &= report.matched;
    }
    Ok(ok)
}

/// `goldens`: check (or `--bless`) every scenario's golden hashes and golden replays.
fn cmd_goldens(args: &Args) -> Result<bool, String> {
    let scen_root = args
        .get("--scenarios")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest_dir().join("scenarios"));
    let golden_root = golden_root(args);
    let data = FsData::new(data_root(args));
    let paths = find_scenarios(&scen_root);
    if paths.is_empty() {
        return Err(format!("no scenarios under {}", scen_root.display()));
    }
    if args.has("--bless") {
        golden::bless_allowed()?;
        for path in &paths {
            for p in golden::bless_scenario(path, &data, &golden_root).map_err(|e| e.to_string())? {
                println!("blessed {}", p.display());
            }
        }
        return Ok(true);
    }
    let mut ok = true;
    for path in &paths {
        let r = golden::check_scenario(path, &data, &golden_root).map_err(|e| e.to_string())?;
        print!("{}", r.to_text());
        ok &= r.ok();
    }
    if !ok {
        println!(
            "\ngoldens differ. If the change is intended: `cargo run -p simcli -- goldens --bless`, \
             commit with {} in the message.",
            golden::BLESS_TAG
        );
    }
    Ok(ok)
}

/// `fuzz`: random cases until the time or case budget runs out.
fn cmd_fuzz(args: &Args) -> Result<bool, String> {
    let root = data_root(args);
    let data = FsData::new(&root);
    let cars = simcli::data::car_ids(&root);
    if cars.is_empty() {
        return Err(format!("no cars under {}", root.join("cars").display()));
    }
    let seed: u64 = args.num("--seed")?.unwrap_or(0x5EED);
    let cfg = FuzzConfig {
        seed,
        cars,
        case_seconds: args.num("--case-seconds")?.unwrap_or(20.0),
        max_vehicles: args.num("--max-vehicles")?.unwrap_or(16),
    };
    let max_cases: Option<u64> = args.num("--cases")?;
    let minutes: f32 = args.num("--minutes")?.unwrap_or(if max_cases.is_some() {
        f32::INFINITY
    } else {
        5.0
    });
    // Wall clock is fine here: it only bounds how many cases run, never what a case does.
    let start = Instant::now();
    let budget = Duration::from_secs_f32((minutes * 60.0).min(1e9));
    let mut keep_going = |done: u64| {
        if done > 0 && done.is_multiple_of(10) {
            eprintln!("fuzz: {done} cases, {:.0} s", start.elapsed().as_secs_f32());
        }
        max_cases.is_none_or(|m| done < m) && start.elapsed() < budget
    };
    match simcli::fuzz::fuzz(&cfg, &data, &mut keep_going) {
        Ok((cases, ticks)) => {
            println!(
                "fuzz ok: seed {seed:#x}, {cases} cases, {ticks} ticks, {:.1} s",
                start.elapsed().as_secs_f32()
            );
            Ok(true)
        }
        Err(f) => {
            let out = PathBuf::from(args.get("--out").unwrap_or("out")).join("fuzz");
            let file = out.join(format!("{}.ron", f.scenario.name));
            let text = format!(
                "// fuzz failure at tick {}: {}\n// reproduce: cargo run -p simcli -- run {}\n{}",
                f.tick,
                f.message,
                file.display(),
                f.scenario.to_ron()
            );
            simcli::output::write_file(&file, text.as_bytes()).map_err(|e| e.to_string())?;
            println!("fuzz FAILED at tick {}: {}", f.tick, f.message);
            println!("repro scenario written to {}", file.display());
            Ok(false)
        }
    }
}

/// `bundle`: the data root as `{"files": {"cars/x.json": "...", ...}}` for the WASM runner.
fn cmd_bundle(args: &Args) -> Result<bool, String> {
    let bundle = MemData::from_dir(&data_root(args)).map_err(|e| e.to_string())?;
    println!(
        "{}",
        serde_json::to_string(&bundle).expect("bundle serializes")
    );
    Ok(true)
}
