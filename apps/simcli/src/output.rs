//! Writes a run's artifacts to `out/<scenario>/`.

use std::path::{Path, PathBuf};

use crate::run::RunOutput;
use crate::RunError;

/// Files written by [`write_outputs`].
pub const OUTPUT_FILES: [&str; 5] = [
    "metrics.csv",
    "events.jsonl",
    "hashes.txt",
    "summary.json",
    "replay.bin",
];

/// Write `metrics.csv`, `events.jsonl`, `hashes.txt`, `summary.json` and `replay.bin` into
/// `<out_root>/<scenario name>/`. Returns that folder.
pub fn write_outputs(out_root: &Path, out: &RunOutput) -> Result<PathBuf, RunError> {
    let dir = out_root.join(&out.summary.name);
    std::fs::create_dir_all(&dir).map_err(|e| RunError::Io(dir.clone(), e.to_string()))?;
    let summary = serde_json::to_string_pretty(&out.summary).expect("summary serializes");
    let files: [(&str, Vec<u8>); 5] = [
        ("metrics.csv", out.metrics_csv.clone().into_bytes()),
        ("events.jsonl", out.events_jsonl.clone().into_bytes()),
        ("hashes.txt", out.hashes_txt().into_bytes()),
        ("summary.json", (summary + "\n").into_bytes()),
        ("replay.bin", out.replay.encode()),
    ];
    for (name, bytes) in files {
        write_file(&dir.join(name), &bytes)?;
    }
    Ok(dir)
}

/// Write a file, creating parent folders.
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<(), RunError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| RunError::Io(parent.to_path_buf(), e.to_string()))?;
    }
    std::fs::write(path, bytes).map_err(|e| RunError::Io(path.to_path_buf(), e.to_string()))
}
