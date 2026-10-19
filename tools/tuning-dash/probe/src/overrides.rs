//! Applying sweep parameter overrides to an in-memory data root (and the probe's scenario).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use simcli::{DataSource, RunError};

use crate::path::{self, Step, SCENARIO_FILE};

/// A data root on disk with some files replaced in memory. Reads go to the patched copy
/// first, then to the disk.
#[derive(Debug, Clone, Default)]
pub struct Overlay {
    /// Data root on disk.
    pub root: PathBuf,
    /// Patched files by data-root-relative path (`/` separators).
    pub files: BTreeMap<String, String>,
}

impl Overlay {
    /// An overlay over `root` with nothing patched yet.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            files: BTreeMap::new(),
        }
    }

    /// Current text of `rel` (patched or on disk).
    pub fn text(&self, rel: &str) -> Result<String, String> {
        if let Some(t) = self.files.get(rel) {
            return Ok(t.clone());
        }
        std::fs::read_to_string(self.root.join(rel))
            .map_err(|e| format!("{}: {e}", self.root.join(rel).display()))
    }
}

impl DataSource for Overlay {
    fn read(&self, rel: &str) -> Result<String, RunError> {
        if let Some(t) = self.files.get(rel) {
            return Ok(t.clone());
        }
        simcli::FsData::new(&self.root).read(rel)
    }
}

/// How an override changes the selected values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// Replace with `value` (any JSON scalar).
    #[default]
    Set,
    /// Multiply the current number by `value`.
    Scale,
    /// Add `value` to the current number.
    Add,
}

/// One parameter override.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Override {
    /// `<file>:<field path>` (see [`crate::path`]).
    pub target: String,
    /// What to do.
    #[serde(default)]
    pub op: Op,
    /// Operand.
    pub value: Value,
    /// Round computed numbers (`scale`/`add`) to this many significant digits, so the
    /// simulated value is exactly the one `promote` writes (default 6).
    #[serde(default)]
    pub round_sig: Option<u32>,
}

/// A concrete change made by an override.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Applied {
    /// Data-root-relative file, or `scenario`.
    pub file: String,
    /// JSON pointer of the changed value.
    pub pointer: String,
    /// Value before.
    pub old: Value,
    /// Value after.
    pub new: Value,
}

/// Round to `sig` significant digits (0 = unchanged).
pub fn round_sig(x: f64, sig: u32) -> f64 {
    if sig == 0 || x == 0.0 || !x.is_finite() {
        return x;
    }
    let mag = x.abs().log10().floor() as i32;
    let decimals = sig as i32 - 1 - mag;
    // Format/parse instead of multiplying, so the result is the shortest decimal string.
    if decimals >= 0 {
        format!("{:.*}", decimals as usize, x).parse().unwrap_or(x)
    } else {
        let p = 10f64.powi(-decimals);
        (x / p).round() * p
    }
}

fn number(v: f64, like_integer: bool) -> Result<Value, String> {
    if like_integer {
        let r = v.round();
        if r >= 0.0 && r <= u64::MAX as f64 {
            return Ok(Value::from(r as u64));
        }
        return Ok(Value::from(r as i64));
    }
    serde_json::Number::from_f64(v)
        .map(Value::Number)
        .ok_or_else(|| format!("{v} is not a finite number"))
}

/// The new value for one location.
pub fn new_value(old: &Value, ov: &Override) -> Result<Value, String> {
    match ov.op {
        Op::Set => {
            // Keep integers integers when the field is one and the value is integral.
            let old_is_int = old.is_u64() || old.is_i64();
            match ov.value.as_f64() {
                Some(x) if old_is_int && x.fract() == 0.0 => number(x, true),
                _ => Ok(ov.value.clone()),
            }
        }
        Op::Scale | Op::Add => {
            let x = old
                .as_f64()
                .ok_or_else(|| format!("{} needs a number, found {old}", op_name(ov.op)))?;
            let k = ov
                .value
                .as_f64()
                .ok_or_else(|| format!("{} operand must be a number", op_name(ov.op)))?;
            let y = if ov.op == Op::Scale { x * k } else { x + k };
            let is_int = old.is_u64() || old.is_i64();
            number(round_sig(y, ov.round_sig.unwrap_or(6)), is_int)
        }
    }
}

fn op_name(op: Op) -> &'static str {
    match op {
        Op::Set => "set",
        Op::Scale => "scale",
        Op::Add => "add",
    }
}

/// Parse a data file into JSON (TOML files become JSON tables).
pub fn parse_file(rel: &str, text: &str) -> Result<Value, String> {
    if rel.ends_with(".toml") {
        toml::from_str::<Value>(text).map_err(|e| format!("{rel}: {e}"))
    } else {
        serde_json::from_str(text).map_err(|e| format!("{rel}: {e}"))
    }
}

/// Serialize a data file back to text in its own format.
pub fn render_file(rel: &str, v: &Value) -> Result<String, String> {
    if rel.ends_with(".toml") {
        toml::to_string(v).map_err(|e| format!("{rel}: {e}"))
    } else {
        Ok(serde_json::to_string(v).expect("JSON value serializes"))
    }
}

/// Apply `overrides` in order: data files in `data`, the `scenario` pseudo-file in `scenario`
/// (when given). Returns every concrete change.
pub fn apply(
    data: &mut Overlay,
    mut scenario: Option<&mut Value>,
    overrides: &[Override],
) -> Result<Vec<Applied>, String> {
    let mut docs: BTreeMap<String, Value> = BTreeMap::new();
    let mut applied = Vec::new();
    for ov in overrides {
        let t = path::parse_target(&ov.target)?;
        let doc: &mut Value = if t.file == SCENARIO_FILE {
            scenario
                .as_deref_mut()
                .ok_or_else(|| format!("{}: only scenario probes have a scenario", ov.target))?
        } else {
            if !docs.contains_key(&t.file) {
                let text = data
                    .text(&t.file)
                    .map_err(|e| format!("{}: no data file {} ({e})", ov.target, t.file))?;
                docs.insert(t.file.clone(), parse_file(&t.file, &text)?);
            }
            docs.get_mut(&t.file).expect("inserted")
        };
        let locations: Vec<Vec<Step>> = match path::resolve(doc, &t.segments) {
            Ok(l) => l,
            // `set` may create a missing optional leaf (e.g. v0.2.0's `drivetrain.efficiency`);
            // the contract loader rejects it if the field does not exist in the schema.
            Err(e) => match create_missing_leaf(doc, &t.segments, ov.op) {
                Some(l) => l,
                None => return Err(format!("{}: {e}", ov.target)),
            },
        };
        for steps in locations {
            let slot = path::get_mut(doc, &steps).expect("resolved");
            // Whole lists/objects may be replaced only in the scenario (e.g. `scenario:expect`
            // to drop a Simulation-only expectation when a variant runs it in Arcade); data
            // files stay scalar-only so every promote is a value edit.
            let replace_container = t.file == SCENARIO_FILE
                && ov.op == Op::Set
                && (ov.value.is_array() || ov.value.is_object());
            if (slot.is_object() || slot.is_array()) && !replace_container {
                return Err(format!(
                    "{}: {} is not a scalar",
                    ov.target,
                    path::pointer(&steps)
                ));
            }
            let new = new_value(slot, ov).map_err(|e| format!("{}: {e}", ov.target))?;
            let old = std::mem::replace(slot, new.clone());
            applied.push(Applied {
                file: t.file.clone(),
                pointer: path::pointer(&steps),
                old,
                new,
            });
        }
    }
    for (rel, doc) in docs {
        let text = render_file(&rel, &doc)?;
        data.files.insert(rel, text);
    }
    Ok(applied)
}

/// For `set` on a path whose last key is missing: add the key (as null, filled by the caller)
/// to every parent object the rest of the path selects. None when that does not apply.
fn create_missing_leaf(
    doc: &mut Value,
    segments: &[path::Segment],
    op: Op,
) -> Option<Vec<Vec<Step>>> {
    let (last, parents) = segments.split_last()?;
    if op != Op::Set || last.key.is_empty() || !last.selectors.is_empty() {
        return None;
    }
    let parent_locs = if parents.is_empty() {
        vec![Vec::new()]
    } else {
        path::resolve(doc, parents).ok()?
    };
    let mut out = Vec::new();
    for steps in parent_locs {
        let obj = path::get_mut(doc, &steps)?.as_object_mut()?;
        obj.entry(last.key.clone()).or_insert(Value::Null);
        let mut s = steps;
        s.push(Step::Key(last.key.clone()));
        out.push(s);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mem() -> Overlay {
        let mut m = Overlay::new("/nonexistent-data-root");
        m.files.insert(
            "cars/x.json".into(),
            r#"{"id":"x","mass_kg":1400.0,"class_n":3,"susp":[{"k":31300.0},{"k":31300.0},{"k":22300.0},{"k":22300.0}]}"#.into(),
        );
        m.files.insert(
            "modes/sim.toml".into(),
            "mode = \"Simulation\"\nplasticity_scale = 1.0\n".into(),
        );
        m
    }

    #[test]
    fn scale_set_add() {
        let mut m = mem();
        let ovs = vec![
            Override {
                target: "car:x:susp[0,1].k".into(),
                op: Op::Scale,
                value: json!(1.2),
                round_sig: None,
            },
            Override {
                target: "car:x:class_n".into(),
                op: Op::Set,
                value: json!(4.0),
                round_sig: None,
            },
            Override {
                target: "mode:sim:plasticity_scale".into(),
                op: Op::Add,
                value: json!(0.25),
                round_sig: None,
            },
        ];
        let a = apply(&mut m, None, &ovs).unwrap();
        assert_eq!(a.len(), 4);
        assert_eq!(a[0].new, json!(37560.0));
        assert_eq!(a[2].new, json!(4));
        let car: Value = serde_json::from_str(&m.files["cars/x.json"]).unwrap();
        assert_eq!(car["susp"][1]["k"], json!(37560.0));
        assert_eq!(car["susp"][2]["k"], json!(22300.0));
        let mode = parse_file("modes/sim.toml", &m.files["modes/sim.toml"]).unwrap();
        assert_eq!(mode["plasticity_scale"], json!(1.25));
    }

    #[test]
    fn set_creates_missing_optional_leaf() {
        let mut m = mem();
        let ov = |t: &str, op| Override {
            target: t.into(),
            op,
            value: json!(0.9),
            round_sig: None,
        };
        let a = apply(&mut m, None, &[ov("car:x:susp[*].inertia", Op::Set)]).unwrap();
        assert_eq!(a.len(), 4);
        assert_eq!(a[0].old, Value::Null);
        let car: Value = serde_json::from_str(&m.files["cars/x.json"]).unwrap();
        assert_eq!(car["susp"][3]["inertia"], json!(0.9));
        // Only `set`, only a missing leaf: scale and missing parents still fail.
        assert!(apply(&mut m, None, &[ov("car:x:nope_s", Op::Scale)]).is_err());
        assert!(apply(&mut m, None, &[ov("car:x:a.b", Op::Set)]).is_err());
    }

    #[test]
    fn scenario_lists_can_be_replaced_data_lists_cannot() {
        let mut m = mem();
        let mut sc = json!({"expect": [{"a": 1}], "mode": "Simulation"});
        let set = |t: &str, v: Value| Override {
            target: t.into(),
            op: Op::Set,
            value: v,
            round_sig: None,
        };
        apply(&mut m, Some(&mut sc), &[set("scenario:expect", json!([]))]).unwrap();
        assert_eq!(sc["expect"], json!([]));
        assert!(apply(&mut m, None, &[set("car:x:susp", json!([]))]).is_err());
    }

    #[test]
    fn rounding() {
        assert_eq!(round_sig(0.1 + 0.2, 6), 0.3);
        assert_eq!(round_sig(31300.0 * 1.1, 6), 34430.0);
        assert_eq!(round_sig(123456789.0, 3), 123000000.0);
    }

    #[test]
    fn errors_are_reported() {
        let mut m = mem();
        let bad = |t: &str| Override {
            target: t.into(),
            op: Op::Scale,
            value: json!(2.0),
            round_sig: None,
        };
        assert!(apply(&mut m, None, &[bad("car:x:nope")]).is_err());
        assert!(apply(&mut m, None, &[bad("car:missing:k")]).is_err());
        assert!(apply(&mut m, None, &[bad("car:x:susp")]).is_err());
        assert!(apply(&mut m, None, &[bad("scenario:name")]).is_err());
    }
}
