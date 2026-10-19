//! `perf-gate` — turn criterion results into `docs/PERF.md` entries and gate regressions.
//!
//! ```text
//! perf-gate report [--criterion DIR] [--class C] [--append docs/PERF.md]
//! perf-gate check  [--criterion DIR] [--class C] [--baseline FILE] [--threshold 0.20] [--append FILE]
//! perf-gate bless  [--criterion DIR] [--class C] [--baseline FILE]
//! ```
//! `--class` defaults to `$WP_RUNNER_CLASS`, else `local`; `--baseline` defaults to
//! `tools/bench/baselines/<class>.json`; `--criterion` to `target/criterion`.

use std::path::PathBuf;
use std::process::ExitCode;

use wp_bench::gate::{
    comparable, compare, fingerprint, fmt_ns, perf_md_section, read_criterion, Baseline,
    DEFAULT_THRESHOLD,
};

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("perf-gate: {e}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<bool, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let get = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let repo = repo.canonicalize().unwrap_or(repo);
    let class = get("--class")
        .or_else(|| std::env::var("WP_RUNNER_CLASS").ok())
        .unwrap_or_else(|| "local".into());
    let criterion = get("--criterion")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join("target/criterion"));
    let baseline_path = get("--baseline")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join(format!("tools/bench/baselines/{class}.json")));
    let threshold: f64 = get("--threshold")
        .map(|t| t.parse().map_err(|_| format!("bad --threshold {t}")))
        .transpose()?
        .unwrap_or(DEFAULT_THRESHOLD);

    let results = read_criterion(&criterion)?;
    let fp = fingerprint(&class);
    let mode = args.first().map(String::as_str).unwrap_or("report");

    let baseline: Option<Baseline> = std::fs::read_to_string(&baseline_path)
        .ok()
        .map(|t| serde_json::from_str(&t).map_err(|e| format!("{}: {e}", baseline_path.display())))
        .transpose()?;
    let cmp = baseline.as_ref().map(|b| compare(&results, b, threshold));

    let section = perf_md_section(&fp, &results, cmp.as_deref());
    print!("{section}");
    if let Some(path) = get("--append") {
        let mut text = std::fs::read_to_string(&path).unwrap_or_default();
        text.push_str(&section);
        std::fs::write(&path, text).map_err(|e| format!("{path}: {e}"))?;
        println!("\nappended to {path}");
    }

    match mode {
        "report" => Ok(true),
        "bless" => {
            let b = Baseline {
                fingerprint: fp,
                benches: results
                    .iter()
                    .map(|r| (r.id.clone(), r.median_ns))
                    .collect(),
            };
            if let Some(dir) = baseline_path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let text = serde_json::to_string_pretty(&b).expect("baseline serializes") + "\n";
            std::fs::write(&baseline_path, text)
                .map_err(|e| format!("{}: {e}", baseline_path.display()))?;
            println!("blessed {}", baseline_path.display());
            Ok(true)
        }
        "check" => {
            let Some(b) = baseline else {
                println!(
                    "\nno baseline at {} for class {class:?}: nothing to gate (bless one with `perf-gate bless`)",
                    baseline_path.display()
                );
                return Ok(true);
            };
            if let Err(why) = comparable(&b, &fp) {
                println!("\nnot gating: {why}");
                return Ok(true);
            }
            let regressions: Vec<_> = cmp
                .unwrap_or_default()
                .into_iter()
                .filter(|c| c.regressed)
                .collect();
            for r in &regressions {
                println!(
                    "REGRESSION {}: {} -> {} ({:+.1}% > {:.0}%)",
                    r.id,
                    fmt_ns(r.baseline_ns.unwrap_or(0.0)),
                    fmt_ns(r.now_ns),
                    r.change.unwrap_or(0.0) * 100.0,
                    threshold * 100.0
                );
            }
            if regressions.is_empty() {
                println!("\nperf gate ok (threshold {:.0}%)", threshold * 100.0);
            }
            Ok(regressions.is_empty())
        }
        other => Err(format!("unknown mode {other:?} (report | check | bless)")),
    }
}
