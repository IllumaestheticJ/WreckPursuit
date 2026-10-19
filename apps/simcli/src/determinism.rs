//! In-process determinism checks. The CLI adds a fresh-process run (`simcli determinism`) and
//! `tools/wasm-determinism` adds the native-vs-WASM comparison.

use crate::data::DataSource;
use crate::run::{run, RunOptions, RunOutput};
use crate::{RunError, Scenario};

/// Run `sc` twice in this process with recording on and require identical per-tick state
/// hashes, per-entity hashes and checkpoint hashes. Returns the `hashes.txt` text on success.
pub fn check_in_process(sc: &Scenario, data: &dyn DataSource) -> Result<String, String> {
    let go = || -> Result<_, RunError> {
        run(
            sc,
            data,
            &RunOptions {
                record: true,
                ..Default::default()
            },
        )
    };
    let a = go().map_err(|e| e.to_string())?;
    let b = go().map_err(|e| e.to_string())?;
    compare_runs(sc, &a, &b)
}

/// Compare two recorded runs of `sc` (both made with `record: true`): per-tick state hashes,
/// per-entity hashes, checkpoints, summary and replay must be identical. Returns the
/// `hashes.txt` text on success. [`check_in_process`] is this applied to two fresh runs.
pub fn compare_runs(sc: &Scenario, a: &RunOutput, b: &RunOutput) -> Result<String, String> {
    if let Some(i) = a
        .replay
        .tick_hashes
        .iter()
        .zip(&b.replay.tick_hashes)
        .position(|(x, y)| x != y)
    {
        return Err(format!(
            "{}: in-process runs diverge at tick {}",
            sc.name,
            i + 1
        ));
    }
    if a.hashes != b.hashes || a.summary != b.summary || a.replay != b.replay {
        return Err(format!(
            "{}: in-process runs differ in summary/hashes",
            sc.name
        ));
    }
    Ok(a.hashes_txt())
}
