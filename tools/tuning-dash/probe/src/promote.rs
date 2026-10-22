//! `promote`: write chosen sweep values back into data files (surgical text edits), validate
//! them with the contract loaders, and append a line per file to the tuning changelog.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::overrides::parse_file;
use crate::path::{self, SCENARIO_FILE};
use crate::textedit;

/// One value to write.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    /// Data-root-relative file (or a shorthand like `car:x`).
    pub file: String,
    /// JSON pointer.
    pub pointer: String,
    /// Value the sweep started from. When given, the file must still hold it (else the
    /// promote is refused as stale unless `force`).
    #[serde(default)]
    pub old: Option<Value>,
    /// Value to write.
    pub new: Value,
}

/// A promote request (`tuning-probe promote <request.json>`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Values to write.
    pub changes: Vec<Change>,
    /// Free-text reason, goes into the changelog line.
    #[serde(default)]
    pub note: String,
    /// Where the values came from (`sweep tire_bcde point p0007`).
    #[serde(default)]
    pub source: String,
    /// Date for the changelog line (`YYYY-MM-DD`; the caller supplies it).
    #[serde(default)]
    pub date: String,
    /// Changelog file (default `<data>/TUNING_CHANGELOG.md`).
    #[serde(default)]
    pub log: Option<PathBuf>,
    /// Write even when a file no longer holds `old`.
    #[serde(default)]
    pub force: bool,
    /// Validate and report, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// Promote outcome.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Outcome {
    /// Everything was written (or would be, for a dry run).
    pub ok: bool,
    /// Why not.
    pub error: Option<String>,
    /// Files changed.
    pub files: Vec<String>,
    /// Changelog lines appended.
    pub log_lines: Vec<String>,
}

/// Validate a data file's text with the contract loader for its folder.
pub fn validate(rel: &str, text: &str) -> Result<(), String> {
    let r = if rel.starts_with("cars/") {
        wp_contracts::parse_vehicle_def(rel, text).map(|_| ())
    } else if rel.starts_with("tires/") {
        wp_contracts::parse_tire_def(rel, text).map(|_| ())
    } else if rel.starts_with("lattices/") {
        wp_contracts::parse_lattice_def(rel, text).map(|_| ())
    } else if rel.starts_with("materials/") {
        wp_contracts::parse_material_def(rel, text).map(|_| ())
    } else if rel.starts_with("modes/") {
        wp_contracts::parse_mode_table(rel, text).map(|_| ())
    } else {
        return parse_file(rel, text).map(|_| ());
    };
    r.map_err(|e| e.to_string())
}

fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x == y || (x - y).abs() <= 1e-9 * x.abs().max(y.abs()),
        _ => a == b,
    }
}

fn short(v: &Value) -> String {
    match v {
        Value::Null => "(unset)".into(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Run a promote request against `data_root`.
pub fn promote(req: &Request, data_root: &Path) -> Outcome {
    match promote_inner(req, data_root) {
        Ok(o) => o,
        Err(e) => Outcome {
            ok: false,
            error: Some(e),
            ..Default::default()
        },
    }
}

fn promote_inner(req: &Request, data_root: &Path) -> Result<Outcome, String> {
    if req.changes.is_empty() {
        return Err("nothing to promote".into());
    }
    let mut by_file: BTreeMap<String, Vec<&Change>> = BTreeMap::new();
    for c in &req.changes {
        let file = path::expand_file(&c.file)?;
        if file == SCENARIO_FILE {
            return Err(
                "scenario overrides are not promotable (scenarios belong to their owners); \
                 edit the .ron by hand"
                    .into(),
            );
        }
        by_file.entry(file).or_default().push(c);
    }
    let mut new_texts = BTreeMap::new();
    let mut log_lines = Vec::new();
    for (rel, changes) in &by_file {
        let disk = data_root.join(rel);
        let mut text =
            std::fs::read_to_string(&disk).map_err(|e| format!("{}: {e}", disk.display()))?;
        let mut parts = Vec::new();
        for c in changes {
            let doc = parse_file(rel, &text)?;
            // A missing last key is an optional field the sweep added (`set` creates it).
            let (steps, missing) = match path::parse_pointer(&doc, &c.pointer) {
                Ok(s) => (s, false),
                Err(e) => {
                    let (parent, key) = c
                        .pointer
                        .rsplit_once('/')
                        .ok_or_else(|| format!("{rel}: {e}"))?;
                    let mut ps =
                        path::parse_pointer(&doc, parent).map_err(|_| format!("{rel}: {e}"))?;
                    let key = key.replace("~1", "/").replace("~0", "~");
                    if !path::get(&doc, &ps).is_some_and(Value::is_object) {
                        return Err(format!("{rel}: {e}"));
                    }
                    ps.push(path::Step::Key(key));
                    (ps, true)
                }
            };
            let current = if missing {
                Value::Null
            } else {
                path::get(&doc, &steps).expect("pointer resolved").clone()
            };
            if let Some(old) = &c.old {
                if !same(old, &current) && !req.force {
                    return Err(format!(
                        "{rel} {}: file holds {current}, the sweep started from {old} \
                         (stale sweep; re-run it or promote with force)",
                        c.pointer
                    ));
                }
            }
            text = match (rel.ends_with(".toml"), missing) {
                (true, false) => textedit::set_toml(&text, &steps, &c.new)?,
                (false, false) => textedit::set_json(&text, &steps, &c.new)?,
                (true, true) => textedit::insert_toml(&text, &steps, &c.new)?,
                (false, true) => textedit::insert_json(&text, &steps, &c.new)?,
            };
            parts.push(format!(
                "`{}` {} → {}",
                c.pointer,
                short(&current),
                short(&c.new)
            ));
        }
        validate(rel, &text).map_err(|e| format!("{rel} would no longer load: {e}"))?;
        // Belt and braces: every promoted value reads back as written.
        let doc = parse_file(rel, &text)?;
        for c in changes {
            let steps = path::parse_pointer(&doc, &c.pointer)?;
            let got = path::get(&doc, &steps).expect("pointer");
            if !same(got, &c.new) {
                return Err(format!(
                    "{rel} {}: wrote {got}, wanted {}",
                    c.pointer, c.new
                ));
            }
        }
        let mut line = format!("- {} · `{rel}`: {}", req.date, parts.join(", "));
        if !req.source.is_empty() {
            line.push_str(&format!(" · from {}", req.source));
        }
        if !req.note.is_empty() {
            line.push_str(&format!(" · {}", req.note.replace('\n', " ")));
        }
        log_lines.push(line);
        new_texts.insert(rel.clone(), text);
    }
    if !req.dry_run {
        for (rel, text) in &new_texts {
            let p = data_root.join(rel);
            std::fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))?;
        }
        let log = req
            .log
            .clone()
            .unwrap_or_else(|| data_root.join("TUNING_CHANGELOG.md"));
        let mut existing =
            std::fs::read_to_string(&log).unwrap_or_else(|_| CHANGELOG_HEADER.into());
        if !existing.ends_with('\n') {
            existing.push('\n');
        }
        for l in &log_lines {
            existing.push_str(l);
            existing.push('\n');
        }
        std::fs::write(&log, existing).map_err(|e| format!("{}: {e}", log.display()))?;
    }
    Ok(Outcome {
        ok: true,
        error: None,
        files: new_texts.keys().cloned().collect(),
        log_lines,
    })
}

/// First lines of a new changelog.
pub const CHANGELOG_HEADER: &str = "# Tuning changelog\n\n\
Values promoted from parameter sweeps by `tools/tuning-dash` (see `docs/TUNING.md`). One line \
per promoted file: date · file: `pointer` old → new · source sweep/point · note. Newest last. \
The data file's owner reviews these like any other data change.\n\n";

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scratch(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("tuning-probe-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("modes")).unwrap();
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../data");
        // The tests expect the calibrated reference (plasticity 1.0), Extreme since v0.4.0.
        std::fs::copy(repo.join("modes/extreme.toml"), d.join("modes/sim.toml")).unwrap();
        d
    }

    fn req(new: Value, old: Option<Value>) -> Request {
        Request {
            changes: vec![Change {
                file: "mode:sim".into(),
                pointer: "/plasticity_scale".into(),
                old,
                new,
            }],
            note: "softer".into(),
            source: "sweep t point p0001".into(),
            date: "2026-10-19".into(),
            log: None,
            force: false,
            dry_run: false,
        }
    }

    #[test]
    fn promote_writes_one_line_and_logs() {
        let d = scratch("ok");
        let before = std::fs::read_to_string(d.join("modes/sim.toml")).unwrap();
        let o = promote(&req(json!(0.85), Some(json!(1.0))), &d);
        assert!(o.ok, "{o:?}");
        let after = std::fs::read_to_string(d.join("modes/sim.toml")).unwrap();
        let changed: Vec<_> = before
            .lines()
            .zip(after.lines())
            .filter(|(a, b)| a != b)
            .collect();
        assert_eq!(changed.len(), 1);
        assert!(changed[0].1.starts_with("plasticity_scale = 0.85"));
        assert!(changed[0].1.contains('#'), "comment kept");
        let log = std::fs::read_to_string(d.join("TUNING_CHANGELOG.md")).unwrap();
        assert!(log.contains("`/plasticity_scale` 1.0 → 0.85 · from sweep t point p0001 · softer"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn stale_and_invalid_are_refused() {
        let d = scratch("bad");
        let before = std::fs::read_to_string(d.join("modes/sim.toml")).unwrap();
        assert!(!promote(&req(json!(0.85), Some(json!(0.5))), &d).ok);
        assert!(
            !promote(&req(json!("soft"), None), &d).ok,
            "string into f32 field"
        );
        let mut r = req(json!(0.85), None);
        r.dry_run = true;
        assert!(promote(&r, &d).ok);
        assert_eq!(
            std::fs::read_to_string(d.join("modes/sim.toml")).unwrap(),
            before
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
