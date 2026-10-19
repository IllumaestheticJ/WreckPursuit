//! Parameter paths: which data file and which field(s) a sweep parameter changes.
//!
//! A target is `<file>:<field path>`:
//!
//! | file part | resolves to |
//! |---|---|
//! | `cars/debug_sedan.json` (any path ending in `.json`/`.toml`) | that file under the data root |
//! | `car:debug_sedan` | `cars/debug_sedan.json` |
//! | `tire:street` | `tires/street.json` |
//! | `lattice:debug_sedan` | `lattices/debug_sedan.json` |
//! | `material:hss` | `materials/hss.json` |
//! | `mode:sim` / `mode:arcade` | `modes/sim.toml` / `modes/arcade.toml` |
//! | `scenario` | the probe's simcli scenario itself (in memory only, never promoted) |
//!
//! The field path is dot-separated keys, each optionally followed by selectors in brackets:
//! `[2]` one index, `[0,1]` several, `[*]` all, `[material_id=hss]` every array element whose
//! `material_id` field equals `hss` (string or number compare). Examples:
//!
//! ```text
//! tire:street:lateral.b
//! car:debug_sedan:suspension[0,1].spring_n_per_m
//! lattice:debug_sedan:beams[material_id=hss].area_m2
//! mode:sim:plasticity_scale
//! scenario:entities[*].car
//! ```
//!
//! Resolution yields RFC 6901 JSON pointers (`/suspension/0/spring_n_per_m`), which is what
//! results record and what `promote` writes back.

use serde_json::Value;

/// A parsed `<file>:<field path>` target.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    /// Data-root-relative file (`cars/x.json`), or `scenario`.
    pub file: String,
    /// Field path segments.
    pub segments: Vec<Segment>,
}

/// One dot-separated part of a field path.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    /// Object key (empty for a leading bare selector on a root array).
    pub key: String,
    /// Array selectors applied after the key, in order.
    pub selectors: Vec<Selector>,
}

/// Array element selector.
#[derive(Debug, Clone, PartialEq)]
pub enum Selector {
    /// Every element.
    All,
    /// These indices.
    Indices(Vec<usize>),
    /// Elements whose field `.0` equals `.1`.
    Filter(String, String),
}

/// The scenario pseudo-file name.
pub const SCENARIO_FILE: &str = "scenario";

/// Expand a file shorthand (`car:x`, `mode:sim`, …) to a data-root-relative path.
pub fn expand_file(file: &str) -> Result<String, String> {
    if file == SCENARIO_FILE {
        return Ok(file.to_string());
    }
    if let Some((kind, id)) = file.split_once(':') {
        let dir = match kind {
            "car" => "cars",
            "tire" => "tires",
            "lattice" => "lattices",
            "material" => "materials",
            "mode" => {
                return match id {
                    "sim" | "simulation" | "Simulation" => Ok("modes/sim.toml".into()),
                    "arcade" | "Arcade" => Ok("modes/arcade.toml".into()),
                    _ => Err(format!("unknown mode {id:?} (sim | arcade)")),
                }
            }
            _ => return Err(format!("unknown file kind {kind:?} in {file:?}")),
        };
        return Ok(format!("{dir}/{id}.json"));
    }
    if file.ends_with(".json") || file.ends_with(".toml") {
        if file.contains("..") || file.starts_with('/') {
            return Err(format!("{file:?} must be relative to the data root"));
        }
        return Ok(file.to_string());
    }
    Err(format!(
        "{file:?}: expected a data file (`cars/x.json`) or a shorthand (`car:x`, `mode:sim`)"
    ))
}

/// Parse `<file>:<field path>`.
pub fn parse_target(s: &str) -> Result<Target, String> {
    // The field path is after the last ':' that is not inside brackets.
    let mut depth = 0i32;
    let mut split = None;
    for (i, c) in s.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            ':' if depth == 0 => split = Some(i),
            _ => {}
        }
    }
    let i = split.ok_or_else(|| format!("{s:?}: expected `<file>:<field path>`"))?;
    let (file, path) = (&s[..i], &s[i + 1..]);
    Ok(Target {
        file: expand_file(file)?,
        segments: parse_path(path).map_err(|e| format!("{s:?}: {e}"))?,
    })
}

/// Parse a dot/bracket field path.
pub fn parse_path(path: &str) -> Result<Vec<Segment>, String> {
    if path.is_empty() {
        return Err("empty field path".into());
    }
    let mut out = Vec::new();
    let mut chars = path.chars().peekable();
    loop {
        let mut key = String::new();
        while let Some(&c) = chars.peek() {
            if c == '.' || c == '[' {
                break;
            }
            key.push(c);
            chars.next();
        }
        let mut selectors = Vec::new();
        while chars.peek() == Some(&'[') {
            chars.next();
            let mut inner = String::new();
            loop {
                match chars.next() {
                    Some(']') => break,
                    Some(c) => inner.push(c),
                    None => return Err("unclosed '['".into()),
                }
            }
            selectors.push(parse_selector(inner.trim())?);
        }
        if key.is_empty() && (selectors.is_empty() || !out.is_empty()) {
            return Err("empty key".into());
        }
        out.push(Segment { key, selectors });
        match chars.next() {
            None => break,
            Some('.') => continue,
            Some(c) => return Err(format!("unexpected {c:?}")),
        }
    }
    Ok(out)
}

fn parse_selector(s: &str) -> Result<Selector, String> {
    if s == "*" {
        return Ok(Selector::All);
    }
    if let Some((k, v)) = s.split_once('=') {
        return Ok(Selector::Filter(k.trim().into(), v.trim().into()));
    }
    let idx: Result<Vec<usize>, _> = s.split(',').map(|p| p.trim().parse::<usize>()).collect();
    idx.map(Selector::Indices)
        .map_err(|_| format!("bad selector [{s}] (use [*], [0,1] or [field=value])"))
}

/// One concrete location, as JSON pointer steps.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
    /// Object key.
    Key(String),
    /// Array index.
    Index(usize),
}

/// Format steps as an RFC 6901 JSON pointer.
pub fn pointer(steps: &[Step]) -> String {
    let mut s = String::new();
    for st in steps {
        s.push('/');
        match st {
            Step::Key(k) => s.push_str(&k.replace('~', "~0").replace('/', "~1")),
            Step::Index(i) => s.push_str(&i.to_string()),
        }
    }
    s
}

/// Parse an RFC 6901 JSON pointer against `root` (array vs key is decided by the value).
pub fn parse_pointer(root: &Value, ptr: &str) -> Result<Vec<Step>, String> {
    if ptr.is_empty() {
        return Ok(Vec::new());
    }
    if !ptr.starts_with('/') {
        return Err(format!("pointer {ptr:?} must start with '/'"));
    }
    let mut steps = Vec::new();
    let mut cur = root;
    for raw in ptr[1..].split('/') {
        let tok = raw.replace("~1", "/").replace("~0", "~");
        match cur {
            Value::Array(a) => {
                let i: usize = tok
                    .parse()
                    .map_err(|_| format!("pointer {ptr:?}: {tok:?} is not an index"))?;
                cur = a
                    .get(i)
                    .ok_or_else(|| format!("pointer {ptr:?}: index {i} out of range"))?;
                steps.push(Step::Index(i));
            }
            Value::Object(o) => {
                cur = o
                    .get(&tok)
                    .ok_or_else(|| format!("pointer {ptr:?}: no key {tok:?}"))?;
                steps.push(Step::Key(tok));
            }
            _ => return Err(format!("pointer {ptr:?}: descends into a scalar")),
        }
    }
    Ok(steps)
}

fn matches_filter(v: &Value, field: &str, want: &str) -> bool {
    match v.get(field) {
        Some(Value::String(s)) => s == want,
        Some(Value::Number(n)) => n.to_string() == want || n.as_f64() == want.parse::<f64>().ok(),
        Some(Value::Bool(b)) => b.to_string() == want,
        _ => false,
    }
}

/// Every concrete location `segments` selects in `root`, in document order. Errors when a
/// key is missing, a selector hits a non-array, or nothing matches.
pub fn resolve(root: &Value, segments: &[Segment]) -> Result<Vec<Vec<Step>>, String> {
    let mut cur: Vec<(Vec<Step>, &Value)> = vec![(Vec::new(), root)];
    for seg in segments {
        let mut next = Vec::new();
        for (steps, v) in cur {
            let (steps, v) = if seg.key.is_empty() {
                (steps, v)
            } else {
                let child = v.get(&seg.key).ok_or_else(|| {
                    format!("no field {:?} at {}", seg.key, display_pointer(&steps))
                })?;
                let mut s = steps;
                s.push(Step::Key(seg.key.clone()));
                (s, child)
            };
            let mut items = vec![(steps, v)];
            for sel in &seg.selectors {
                let mut sel_out = Vec::new();
                for (steps, v) in items {
                    let arr = v.as_array().ok_or_else(|| {
                        format!("selector on non-array at {}", display_pointer(&steps))
                    })?;
                    let picked: Vec<usize> = match sel {
                        Selector::All => (0..arr.len()).collect(),
                        Selector::Indices(ix) => {
                            for &i in ix {
                                if i >= arr.len() {
                                    return Err(format!(
                                        "index {i} out of range (len {}) at {}",
                                        arr.len(),
                                        display_pointer(&steps)
                                    ));
                                }
                            }
                            ix.clone()
                        }
                        Selector::Filter(f, w) => (0..arr.len())
                            .filter(|&i| matches_filter(&arr[i], f, w))
                            .collect(),
                    };
                    for i in picked {
                        let mut s = steps.clone();
                        s.push(Step::Index(i));
                        sel_out.push((s, &arr[i]));
                    }
                }
                items = sel_out;
            }
            next.extend(items);
        }
        cur = next;
    }
    if cur.is_empty() {
        return Err("path matches nothing".into());
    }
    Ok(cur.into_iter().map(|(s, _)| s).collect())
}

fn display_pointer(steps: &[Step]) -> String {
    if steps.is_empty() {
        "the root".into()
    } else {
        pointer(steps)
    }
}

/// Value at `steps`.
pub fn get<'a>(root: &'a Value, steps: &[Step]) -> Option<&'a Value> {
    let mut cur = root;
    for st in steps {
        cur = match st {
            Step::Key(k) => cur.get(k)?,
            Step::Index(i) => cur.get(*i)?,
        };
    }
    Some(cur)
}

/// Mutable value at `steps`.
pub fn get_mut<'a>(root: &'a mut Value, steps: &[Step]) -> Option<&'a mut Value> {
    let mut cur = root;
    for st in steps {
        cur = match st {
            Step::Key(k) => cur.get_mut(k)?,
            Step::Index(i) => cur.get_mut(*i)?,
        };
    }
    Some(cur)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn shorthands_expand() {
        assert_eq!(
            expand_file("car:debug_sedan").unwrap(),
            "cars/debug_sedan.json"
        );
        assert_eq!(expand_file("mode:arcade").unwrap(), "modes/arcade.toml");
        assert_eq!(
            expand_file("tires/street.json").unwrap(),
            "tires/street.json"
        );
        assert!(expand_file("../etc/passwd.json").is_err());
        assert!(expand_file("bogus:x").is_err());
    }

    #[test]
    fn targets_parse_and_resolve() {
        let doc = json!({
            "suspension": [{"k": 1.0}, {"k": 2.0}, {"k": 3.0}, {"k": 4.0}],
            "beams": [{"m": "hss", "a": 1.0}, {"m": "pp", "a": 2.0}, {"m": "hss", "a": 3.0}],
            "lateral": {"b": 16.0},
        });
        let t = parse_target("car:x:suspension[0,1].k").unwrap();
        assert_eq!(t.file, "cars/x.json");
        let r = resolve(&doc, &t.segments).unwrap();
        assert_eq!(
            r.iter().map(|s| pointer(s)).collect::<Vec<_>>(),
            ["/suspension/0/k", "/suspension/1/k"]
        );
        let t = parse_target("lattice:x:beams[m=hss].a").unwrap();
        assert_eq!(resolve(&doc, &t.segments).unwrap().len(), 2);
        let t = parse_target("tire:street:lateral.b").unwrap();
        assert_eq!(
            pointer(&resolve(&doc, &t.segments).unwrap()[0]),
            "/lateral/b"
        );
        let t = parse_target("car:x:suspension[*].k").unwrap();
        assert_eq!(resolve(&doc, &t.segments).unwrap().len(), 4);
        assert!(resolve(&doc, &parse_path("nope").unwrap()).is_err());
        assert!(resolve(&doc, &parse_path("suspension[9].k").unwrap()).is_err());
        assert!(resolve(&doc, &parse_path("beams[m=none].a").unwrap()).is_err());
    }

    #[test]
    fn pointers_round_trip() {
        let doc = json!({"a/b": [0, {"c": 1}]});
        let steps = parse_pointer(&doc, "/a~1b/1/c").unwrap();
        assert_eq!(pointer(&steps), "/a~1b/1/c");
        assert_eq!(get(&doc, &steps), Some(&json!(1)));
    }
}
