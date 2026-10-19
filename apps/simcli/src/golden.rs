//! Golden hashes and golden replays.
//!
//! Layout under the golden root (default `apps/simcli/golden/`):
//! - `<scenario name>/hashes.txt` for **every** scenario under `scenarios/`;
//! - `<scenario name>.replay.bin` for every scenario under `scenarios/golden/` (the "feel"
//!   library).
//!
//! A change that alters them fails CI until re-blessed (`simcli goldens --bless`); in CI blessing
//! is refused unless the commit message carries `[bless]` (see [`bless_allowed`]).

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::data::DataSource;
use crate::output::write_file;
use crate::replay::{self, Replay, ReplayReport};
use crate::run::{run, RunOptions, RunOutput};
use crate::{load_scenario, RunError, Scenario};

/// Commit-message tag that authorises blessing in CI.
pub const BLESS_TAG: &str = "[bless]";

/// Refuse to bless in CI (`CI` env set) unless `WP_COMMIT_MESSAGE` contains [`BLESS_TAG`].
/// Locally blessing is always allowed.
pub fn bless_allowed() -> Result<(), String> {
    let ci = std::env::var("CI").map(|v| !v.is_empty()).unwrap_or(false);
    if !ci {
        return Ok(());
    }
    let msg = std::env::var("WP_COMMIT_MESSAGE").unwrap_or_default();
    if msg.contains(BLESS_TAG) {
        Ok(())
    } else {
        Err(format!(
            "refusing to bless in CI: the commit message must contain {BLESS_TAG} (WP_COMMIT_MESSAGE)"
        ))
    }
}

/// First difference between two `hashes.txt` files.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HashDiff {
    /// All `<tick> <hash>` lines equal.
    pub matched: bool,
    /// First tick whose hash differs: `(tick, golden, now)`; `None` with `matched == false`
    /// means the line counts differ.
    pub first_mismatch: Option<(u64, String, String)>,
    /// Header differences (systems, hash spec, contract version).
    pub notes: Vec<String>,
}

fn data_lines(text: &str) -> Vec<(u64, String)> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((it.next()?.parse().ok()?, it.next()?.to_string()))
        })
        .collect()
}

/// Compare a golden `hashes.txt` with a fresh one.
pub fn compare_hashes(golden: &str, now: &str) -> HashDiff {
    let header = |t: &str| {
        t.lines()
            .next()
            .filter(|l| l.starts_with('#'))
            .map(str::to_string)
    };
    let mut notes = Vec::new();
    if header(golden) != header(now) {
        notes.push(format!(
            "header changed: {:?} -> {:?}",
            header(golden).unwrap_or_default(),
            header(now).unwrap_or_default()
        ));
    }
    let (g, n) = (data_lines(golden), data_lines(now));
    let first_mismatch = g
        .iter()
        .zip(&n)
        .find(|(a, b)| a != b)
        .map(|(a, b)| (a.0, a.1.clone(), b.1.clone()));
    HashDiff {
        matched: first_mismatch.is_none() && g.len() == n.len(),
        first_mismatch,
        notes,
    }
}

/// Golden hashes path for a scenario.
pub fn hashes_path(golden_root: &Path, name: &str) -> PathBuf {
    golden_root.join(name).join("hashes.txt")
}

/// Golden replay path for a scenario.
pub fn replay_path(golden_root: &Path, name: &str) -> PathBuf {
    golden_root.join(format!("{name}.replay.bin"))
}

/// Whether a scenario file belongs to the golden replay library (`scenarios/golden/…`).
pub fn is_replay_scenario(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == "golden")
}

/// Outcome of checking one scenario's goldens.
#[derive(Debug, Clone, Serialize)]
pub struct GoldenResult {
    /// Scenario name.
    pub name: String,
    /// Scenario file.
    pub path: String,
    /// Expectations passed.
    pub expectations_passed: bool,
    /// Expectation failures.
    pub failures: Vec<String>,
    /// Hash comparison (`None` = no golden on disk).
    pub hashes: Option<HashDiff>,
    /// Replay verification (`None` = not a replay scenario or no golden replay).
    pub replay: Option<ReplayReport>,
    /// Missing golden files.
    pub missing: Vec<String>,
}

impl GoldenResult {
    /// Everything matched and passed.
    pub fn ok(&self) -> bool {
        self.expectations_passed
            && self.missing.is_empty()
            && self.hashes.as_ref().is_none_or(|h| h.matched)
            && self.replay.as_ref().is_none_or(|r| r.matched)
    }

    /// One-paragraph human-readable report.
    pub fn to_text(&self) -> String {
        let mut s = format!(
            "{} {} ({})\n",
            if self.ok() { "ok  " } else { "FAIL" },
            self.name,
            self.path
        );
        for f in &self.failures {
            s.push_str(&format!("  expectation: {f}\n"));
        }
        for m in &self.missing {
            s.push_str(&format!(
                "  missing golden {m} (run `simcli goldens --bless`)\n"
            ));
        }
        if let Some(h) = &self.hashes {
            for n in &h.notes {
                s.push_str(&format!("  hashes note: {n}\n"));
            }
            if let Some((tick, g, n)) = &h.first_mismatch {
                s.push_str(&format!(
                    "  hashes: first mismatch at tick {tick}: golden {g}, now {n}\n"
                ));
            } else if !h.matched {
                s.push_str("  hashes: different number of checkpoints\n");
            }
        }
        if let Some(r) = &self.replay {
            if !r.matched {
                for line in r.to_text().lines().skip(1) {
                    s.push_str(&format!("  replay:{line}\n"));
                }
            }
        }
        s
    }
}

fn record(sc: &Scenario, data: &dyn DataSource) -> Result<RunOutput, RunError> {
    run(
        sc,
        data,
        &RunOptions {
            record: true,
            ..Default::default()
        },
    )
}

/// Check one scenario file against the goldens.
pub fn check_scenario(
    path: &Path,
    data: &dyn DataSource,
    golden_root: &Path,
) -> Result<GoldenResult, RunError> {
    let sc = load_scenario(path)?;
    let out = record(&sc, data)?;
    let mut missing = Vec::new();
    let hp = hashes_path(golden_root, &sc.name);
    let hashes = match std::fs::read_to_string(&hp) {
        Ok(text) => Some(compare_hashes(&text, &out.hashes_txt())),
        Err(_) => {
            missing.push(hp.display().to_string());
            None
        }
    };
    let mut replay_report = None;
    if is_replay_scenario(path) {
        let rp = replay_path(golden_root, &sc.name);
        match std::fs::read(&rp) {
            Ok(bytes) => {
                let golden = Replay::decode(&bytes)?;
                // Replay the recorded inputs (not the scenario's drivers): that is what the
                // testbed recordings will contain.
                replay_report = Some(replay::verify(&golden, data)?);
            }
            Err(_) => missing.push(rp.display().to_string()),
        }
    }
    Ok(GoldenResult {
        name: sc.name.clone(),
        path: path.display().to_string(),
        expectations_passed: out.summary.passed(),
        failures: out.summary.failures.clone(),
        hashes,
        replay: replay_report,
        missing,
    })
}

/// Regenerate the goldens for one scenario file. Returns the files written.
pub fn bless_scenario(
    path: &Path,
    data: &dyn DataSource,
    golden_root: &Path,
) -> Result<Vec<PathBuf>, RunError> {
    let sc = load_scenario(path)?;
    let out = record(&sc, data)?;
    let mut written = Vec::new();
    let hp = hashes_path(golden_root, &sc.name);
    write_file(&hp, out.hashes_txt().as_bytes())?;
    written.push(hp);
    if is_replay_scenario(path) {
        let rp = replay_path(golden_root, &sc.name);
        write_file(&rp, &out.replay.encode())?;
        written.push(rp);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_compare_finds_first_mismatch() {
        let g = "# h\n240 aa\n480 bb\n720 cc\n";
        assert!(compare_hashes(g, g).matched);
        let d = compare_hashes(g, "# h\n240 aa\n480 xx\n720 yy\n");
        assert!(!d.matched);
        assert_eq!(d.first_mismatch, Some((480, "bb".into(), "xx".into())));
        let short = compare_hashes(g, "# h\n240 aa\n");
        assert!(!short.matched && short.first_mismatch.is_none());
        assert_eq!(
            compare_hashes(g, "# other\n240 aa\n480 bb\n720 cc\n")
                .notes
                .len(),
            1
        );
    }

    #[test]
    fn replay_scenarios_are_under_golden() {
        assert!(is_replay_scenario(Path::new("scenarios/golden/x.ron")));
        assert!(!is_replay_scenario(Path::new("scenarios/smoke/x.ron")));
    }
}
