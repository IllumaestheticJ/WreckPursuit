//! Perf gate: reads criterion results, fingerprints the machine, appends `docs/PERF.md`, and
//! compares against the blessed baseline of the same runner class.
//!
//! Baselines live in `tools/bench/baselines/<runner class>.json`. A comparison only gates when
//! the runner class **and** CPU model match the baseline (shared CI runners change CPUs; a
//! different CPU is reported, not failed).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

/// Allowed slowdown before the gate fails (20 %).
pub const DEFAULT_THRESHOLD: f64 = 0.20;

/// Absolute slowdown below which a bench never counts as regressed (timer noise on stub
/// systems that take nanoseconds).
pub const NOISE_FLOOR_NS: f64 = 100.0;

/// Physics ticks per rendered/net frame at 60 Hz.
pub const TICKS_PER_FRAME: f64 = 4.0;

/// Median time of one bench iteration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchResult {
    /// Criterion full id, e.g. `sim_only/24`.
    pub id: String,
    /// Median, nanoseconds per iteration.
    pub median_ns: f64,
}

/// Where and how numbers were measured.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fingerprint {
    /// Runner class (`WP_RUNNER_CLASS`, e.g. `gha-ubuntu-latest`, or `local`).
    pub runner_class: String,
    /// CPU model string.
    pub cpu: String,
    /// Logical cores.
    pub cores: usize,
    /// `os-arch`.
    pub platform: String,
    /// `rustc --version`.
    pub rustc: String,
    /// `git rev-parse --short HEAD`.
    pub commit: String,
    /// UTC date `YYYY-MM-DD`.
    pub date: String,
}

/// A blessed set of numbers for one runner class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Baseline {
    /// Machine the numbers came from.
    pub fingerprint: Fingerprint,
    /// Median ns per bench id.
    pub benches: BTreeMap<String, f64>,
}

/// One bench compared with its baseline.
#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    /// Bench id.
    pub id: String,
    /// Baseline median ns (None = new bench).
    pub baseline_ns: Option<f64>,
    /// Current median ns.
    pub now_ns: f64,
    /// `now / baseline - 1`.
    pub change: Option<f64>,
    /// Slower than the threshold allows.
    pub regressed: bool,
}

fn read_json(path: &Path) -> Result<serde_json::Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Read every `<criterion dir>/**/new/{benchmark,estimates}.json`, sorted by id.
pub fn read_criterion(dir: &Path) -> Result<Vec<BenchResult>, String> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n == "new") && p.join("estimates.json").exists() {
                    out.push(p);
                } else {
                    walk(&p, out);
                }
            }
        }
    }
    let mut dirs = Vec::new();
    walk(dir, &mut dirs);
    let mut out = Vec::new();
    for d in dirs {
        let bench = read_json(&d.join("benchmark.json"))?;
        let est = read_json(&d.join("estimates.json"))?;
        let id = bench["full_id"]
            .as_str()
            .ok_or_else(|| format!("{}: no full_id", d.display()))?
            .to_string();
        let median_ns = est["median"]["point_estimate"]
            .as_f64()
            .ok_or_else(|| format!("{}: no median", d.display()))?;
        out.push(BenchResult { id, median_ns });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    if out.is_empty() {
        return Err(format!(
            "no criterion results under {} (run `cargo bench -p wp_bench` first)",
            dir.display()
        ));
    }
    Ok(out)
}

fn cmd(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Days since 1970-01-01 → `(year, month, day)` (Howard Hinnant's civil-from-days).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + (m <= 2) as i64;
    (y, m, d)
}

/// Fingerprint this machine.
pub fn fingerprint(runner_class: &str) -> Fingerprint {
    let cpu = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|t| {
            t.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split(':').nth(1))
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| cmd("sysctl", &["-n", "machdep.cpu.brand_string"]));
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    Fingerprint {
        runner_class: runner_class.to_string(),
        cpu,
        cores: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(0),
        platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        rustc: cmd("rustc", &["--version"]),
        commit: cmd("git", &["rev-parse", "--short", "HEAD"]),
        date: format!("{y:04}-{m:02}-{d:02}"),
    }
}

/// Compare results with a baseline. A bench regresses when `now > baseline * (1 + threshold)`
/// and the slowdown is at least [`NOISE_FLOOR_NS`].
pub fn compare(results: &[BenchResult], baseline: &Baseline, threshold: f64) -> Vec<Comparison> {
    results
        .iter()
        .map(|r| {
            let base = baseline.benches.get(&r.id).copied();
            let change = base.map(|b| r.median_ns / b - 1.0);
            Comparison {
                id: r.id.clone(),
                baseline_ns: base,
                now_ns: r.median_ns,
                change,
                regressed: change.is_some_and(|c| c > threshold)
                    && base.is_some_and(|b| r.median_ns - b >= NOISE_FLOOR_NS),
            }
        })
        .collect()
}

/// Whether a baseline may gate this machine (same runner class and CPU model).
pub fn comparable(baseline: &Baseline, fp: &Fingerprint) -> Result<(), String> {
    if baseline.fingerprint.runner_class != fp.runner_class {
        return Err(format!(
            "baseline is for runner class {:?}, this is {:?}",
            baseline.fingerprint.runner_class, fp.runner_class
        ));
    }
    if baseline.fingerprint.cpu != fp.cpu {
        return Err(format!(
            "baseline CPU {:?} differs from this CPU {:?}",
            baseline.fingerprint.cpu, fp.cpu
        ));
    }
    Ok(())
}

/// `(per-frame label, budget ms)` for a bench id: what one frame (4 ticks) costs, per car where
/// the budget is per car. Budgets from CLAUDE.md (native, release, @60 Hz).
fn frame_view(id: &str, ns: f64) -> (String, String) {
    let frame_ms = ns * TICKS_PER_FRAME / 1e6;
    if let Some(n) = id
        .strip_prefix("sim_only/")
        .and_then(|n| n.parse::<f64>().ok())
    {
        let per_car = frame_ms / n;
        return (
            format!("{per_car:.4} ms/car/frame"),
            "0.40 ms/car (vehicle 0.15 + lattice 0.25)".into(),
        );
    }
    match id {
        "city/24v_400traffic" => (
            format!("{frame_ms:.4} ms/frame"),
            "11.6 ms (24 × 0.40 + traffic 2.0)".into(),
        ),
        i if i.starts_with("lattice/") => {
            (format!("{frame_ms:.4} ms/car/frame"), "0.25 ms/car".into())
        }
        i if i.starts_with("snapshot/") => (format!("{:.4} ms/snapshot", ns / 1e6), "—".into()),
        _ => (format!("{frame_ms:.4} ms/frame"), "—".into()),
    }
}

/// `850 ns` / `4.57 µs` / `1.20 ms`.
pub fn fmt_ns(ns: f64) -> String {
    if ns < 1e3 {
        format!("{ns:.1} ns")
    } else if ns < 1e6 {
        format!("{:.2} µs", ns / 1e3)
    } else {
        format!("{:.2} ms", ns / 1e6)
    }
}

/// Markdown section for `docs/PERF.md`.
pub fn perf_md_section(
    fp: &Fingerprint,
    results: &[BenchResult],
    cmp: Option<&[Comparison]>,
) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "\n## {} — `{}` @ {}\n\n- runner class: `{}`\n- cpu: {} ({} logical cores), {}\n- {}\n",
        fp.date,
        fp.commit,
        fp.runner_class,
        fp.runner_class,
        fp.cpu,
        fp.cores,
        fp.platform,
        fp.rustc
    );
    let _ = writeln!(
        s,
        "| bench | median / iter | per frame (60 Hz) | budget (native) | vs baseline |\n|---|---|---|---|---|"
    );
    for r in results {
        let (frame, budget) = frame_view(&r.id, r.median_ns);
        let delta = cmp
            .and_then(|c| c.iter().find(|c| c.id == r.id))
            .and_then(|c| c.change)
            .map(|c| format!("{:+.1}%", c * 100.0))
            .unwrap_or_else(|| "—".into());
        let _ = writeln!(
            s,
            "| `{}` | {} | {frame} | {budget} | {delta} |",
            r.id,
            fmt_ns(r.median_ns)
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(class: &str, cpu: &str) -> Fingerprint {
        Fingerprint {
            runner_class: class.into(),
            cpu: cpu.into(),
            cores: 4,
            platform: "linux-x86_64".into(),
            rustc: "rustc".into(),
            commit: "abc".into(),
            date: "2026-01-01".into(),
        }
    }

    #[test]
    fn regression_over_threshold_fails() {
        let base = Baseline {
            fingerprint: fp("ci", "X"),
            benches: BTreeMap::from([
                ("a".to_string(), 1000.0),
                ("b".to_string(), 1000.0),
                ("tiny".to_string(), 1.0),
            ]),
        };
        let now = [
            BenchResult {
                id: "a".into(),
                median_ns: 1190.0,
            },
            BenchResult {
                id: "b".into(),
                median_ns: 1210.0,
            },
            BenchResult {
                id: "tiny".into(),
                median_ns: 3.0,
            },
            BenchResult {
                id: "new".into(),
                median_ns: 5.0,
            },
        ];
        let c = compare(&now, &base, DEFAULT_THRESHOLD);
        assert_eq!(c.iter().filter(|c| c.regressed).count(), 1);
        assert!(c[1].regressed && !c[2].regressed && c[3].baseline_ns.is_none());
        assert!(comparable(&base, &fp("ci", "X")).is_ok());
        assert!(comparable(&base, &fp("local", "X")).is_err());
        assert!(comparable(&base, &fp("ci", "Y")).is_err());
    }

    #[test]
    fn dates_and_markdown() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_745), (2026, 10, 19));
        let md = perf_md_section(
            &fp("ci", "X"),
            &[BenchResult {
                id: "sim_only/24".into(),
                median_ns: 24_000.0,
            }],
            None,
        );
        assert!(md.contains("0.0040 ms/car/frame"), "{md}");
    }
}
