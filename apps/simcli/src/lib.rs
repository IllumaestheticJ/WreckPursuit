//! # simcli
//!
//! Headless simulation harness (owned by workstream 10):
//!
//! - [`scenario`]: the `.ron` scenario format (reference: `SCENARIOS.md`).
//! - [`run`]: builds a [`wp_sim::Sim`] with the selected systems, drives it, collects metrics,
//!   events and hashes, and evaluates expectations.
//! - [`replay`]: the versioned `replay.bin` format and the golden-replay verifier.
//! - [`golden`]: golden `hashes.txt` comparison and blessing.
//! - [`determinism`]: in-process repeat checks (the fresh-process and WASM checks drive the
//!   binary and `tools/wasm-determinism`).
//! - [`fuzz`]: random-input/random-collision soak with invariant checks.
//! - [`net`]: snapshot round trip on `wp_net`'s wire codec.
//! - [`data`]: one loading code path for disk and in-memory (WASM) data.
//!
//! Nothing in here reads the wall clock; the CLI owns time limits.

#![warn(missing_docs)]

pub mod data;
pub mod determinism;
pub mod fuzz;
pub mod golden;
pub mod net;
pub mod output;
pub mod replay;
pub mod run;
pub mod scenario;

use std::path::{Path, PathBuf};

use thiserror::Error;

pub use data::{DataSource, FsData, MemData};
pub use run::{
    run, run_with_hook, RunOptions, RunOutput, RunSummary, Selection, TickView,
    PURSUIT_STARTED_KIND,
};
pub use scenario::Scenario;

/// Runner errors.
#[derive(Debug, Error)]
pub enum RunError {
    /// Scenario file unreadable, malformed or inconsistent.
    #[error("scenario {0}: {1}")]
    Scenario(String, String),
    /// Data failed to load.
    #[error(transparent)]
    Load(#[from] wp_contracts::LoadError),
    /// Sim refused an operation.
    #[error(transparent)]
    Sim(#[from] wp_sim::SimError),
    /// Replay file malformed or inconsistent with its scenario.
    #[error("replay: {0}")]
    Replay(String),
    /// A per-tick invariant hook failed (fuzzing).
    #[error("invariant violated at tick {tick}: {message}")]
    Invariant {
        /// Tick at which the hook failed.
        tick: u64,
        /// What failed.
        message: String,
    },
    /// Filesystem error writing outputs or goldens.
    #[error("io {0}: {1}")]
    Io(PathBuf, String),
}

/// Parse a scenario file.
pub fn load_scenario(path: &Path) -> Result<Scenario, RunError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| RunError::Scenario(path.display().to_string(), e.to_string()))?;
    Scenario::from_ron(&path.display().to_string(), &text)
}

/// Every `.ron` file under `dir` (recursively), sorted. A file path is returned as-is.
pub fn find_scenarios(path: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().and_then(|e| e.to_str()) == Some("ron") {
                out.push(p);
            }
        }
    }
    if path.is_file() {
        return vec![path.to_path_buf()];
    }
    let mut out = Vec::new();
    walk(path, &mut out);
    out.sort();
    out
}

/// Run a scenario with every system and no recording; convenience for tests.
pub fn run_scenario(sc: &Scenario, data_root: &Path) -> Result<RunSummary, RunError> {
    Ok(run(sc, &FsData::new(data_root), &RunOptions::default())?.summary)
}
