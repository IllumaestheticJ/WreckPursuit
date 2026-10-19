//! Surgical edits of data files for `promote`: replace one value's text in place and leave
//! every other byte (layout, key order, number spelling, TOML comments) untouched, so a
//! promoted change is a one-line diff.

use serde_json::Value;

use crate::path::Step;

/// Byte span `[start, end)` of the JSON value at `steps` in `text`.
pub fn json_span(text: &str, steps: &[Step]) -> Result<(usize, usize), String> {
    let b = text.as_bytes();
    let mut i = skip_ws(b, 0);
    for (depth, step) in steps.iter().enumerate() {
        let here = || crate::path::pointer(&steps[..depth]);
        match (b.get(i), step) {
            (Some(b'{'), Step::Key(want)) => {
                i += 1;
                loop {
                    i = skip_ws(b, i);
                    if b.get(i) == Some(&b'}') {
                        return Err(format!("no key {want:?} under {}", here()));
                    }
                    let (ks, ke) = string_span(b, i)?;
                    let key: String = serde_json::from_str(&text[ks..ke])
                        .map_err(|e| format!("bad key at byte {ks}: {e}"))?;
                    i = skip_ws(b, ke);
                    if b.get(i) != Some(&b':') {
                        return Err(format!("expected ':' at byte {i}"));
                    }
                    i = skip_ws(b, i + 1);
                    if &key == want {
                        break;
                    }
                    i = skip_ws(b, value_end(b, i)?);
                    match b.get(i) {
                        Some(b',') => i += 1,
                        Some(b'}') => return Err(format!("no key {want:?} under {}", here())),
                        _ => return Err(format!("expected ',' or '}}' at byte {i}")),
                    }
                }
            }
            (Some(b'['), Step::Index(want)) => {
                i += 1;
                let mut n = 0usize;
                loop {
                    i = skip_ws(b, i);
                    if b.get(i) == Some(&b']') {
                        return Err(format!("index {want} out of range under {}", here()));
                    }
                    if n == *want {
                        break;
                    }
                    i = skip_ws(b, value_end(b, i)?);
                    match b.get(i) {
                        Some(b',') => i += 1,
                        _ => return Err(format!("index {want} out of range under {}", here())),
                    }
                    n += 1;
                }
            }
            _ => return Err(format!("{} does not match the document shape", here())),
        }
    }
    let end = value_end(b, i)?;
    Ok((i, end))
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn string_span(b: &[u8], i: usize) -> Result<(usize, usize), String> {
    if b.get(i) != Some(&b'"') {
        return Err(format!("expected a string at byte {i}"));
    }
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            b'"' => return Ok((i, j + 1)),
            _ => j += 1,
        }
    }
    Err("unterminated string".into())
}

/// End (exclusive) of the JSON value starting at `i`.
fn value_end(b: &[u8], i: usize) -> Result<usize, String> {
    match b.get(i) {
        Some(b'"') => Ok(string_span(b, i)?.1),
        Some(b'{') | Some(b'[') => {
            let mut depth = 0i32;
            let mut j = i;
            while j < b.len() {
                match b[j] {
                    b'"' => {
                        j = string_span(b, j)?.1;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Ok(j + 1);
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            Err("unterminated container".into())
        }
        Some(_) => {
            let mut j = i;
            while j < b.len() && !matches!(b[j], b',' | b'}' | b']') && !b[j].is_ascii_whitespace()
            {
                j += 1;
            }
            if j == i {
                Err(format!("expected a value at byte {i}"))
            } else {
                Ok(j)
            }
        }
        None => Err("unexpected end of file".into()),
    }
}

/// Spell `new` the way the file spells numbers: a float field stays a float literal
/// (`37560.0`, not `37560`), integers stay integers.
pub fn format_like(old_token: &str, new: &Value) -> String {
    match new {
        Value::Number(n) if n.is_f64() => {
            let x = n.as_f64().expect("f64");
            let s = format!("{x}");
            if s.contains(['.', 'e', 'E']) || s.contains("inf") || s.contains("NaN") {
                s
            } else {
                format!("{s}.0")
            }
        }
        Value::Number(n) => {
            let old_is_float = old_token.contains(['.', 'e', 'E']);
            if old_is_float {
                format!("{n}.0")
            } else {
                n.to_string()
            }
        }
        other => serde_json::to_string(other).expect("scalar serializes"),
    }
}

/// Replace the value at `steps` in JSON `text` with `new`.
pub fn set_json(text: &str, steps: &[Step], new: &Value) -> Result<String, String> {
    let (s, e) = json_span(text, steps)?;
    let tok = &text[s..e];
    Ok(format!(
        "{}{}{}",
        &text[..s],
        format_like(tok, new),
        &text[e..]
    ))
}

/// Add a new member `steps.last()` (a key) with `new` to the JSON object at `steps[..n-1]`,
/// as its last member, matching the object's layout (one member per line or inline).
pub fn insert_json(text: &str, steps: &[Step], new: &Value) -> Result<String, String> {
    let Some((Step::Key(key), parent)) = steps.split_last() else {
        return Err("insert needs a key".into());
    };
    let (s, e) = json_span(text, parent)?;
    let obj = &text[s..e];
    if !obj.starts_with('{') {
        return Err(format!("{} is not an object", crate::path::pointer(parent)));
    }
    let member = format!(
        "{}: {}",
        serde_json::to_string(key).expect("key"),
        format_like("", new)
    );
    let close = e - 1;
    let inner = &text[s + 1..close];
    let last_nonws = inner.trim_end().len();
    let at = s + 1 + last_nonws;
    let insert = if inner.trim().is_empty() {
        member
    } else if let Some(nl) = inner.find('\n') {
        // Indentation of the first member line.
        let indent: String = inner[nl + 1..]
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        format!(",\n{indent}{member}")
    } else {
        format!(", {member}")
    };
    Ok(format!("{}{}{}", &text[..at], insert, &text[at..]))
}

/// Add `key = value` to the top-level table of TOML `text` (before the first `[table]`).
pub fn insert_toml(text: &str, steps: &[Step], new: &Value) -> Result<String, String> {
    let [Step::Key(key)] = steps else {
        return Err("TOML insert supports top-level keys only".into());
    };
    let line = format!("{key} = {}\n", format_like("", new));
    let mut pos = text.len();
    let mut off = 0;
    for l in text.split_inclusive('\n') {
        if l.trim_start().starts_with('[') {
            pos = off;
            break;
        }
        off += l.len();
    }
    let mut out = text[..pos].to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&line);
    out.push_str(&text[pos..]);
    Ok(out)
}

/// Replace the value of `key` (top level, or `[table]` → key for two steps) in TOML `text`,
/// keeping the line's indentation and trailing comment.
pub fn set_toml(text: &str, steps: &[Step], new: &Value) -> Result<String, String> {
    let (table, key) = match steps {
        [Step::Key(k)] => ("", k.as_str()),
        [Step::Key(t), Step::Key(k)] => (t.as_str(), k.as_str()),
        _ => {
            return Err(format!(
                "TOML promote supports `key` and `table.key` only, not {}",
                crate::path::pointer(steps)
            ))
        }
    };
    let mut current = String::new();
    let mut out = String::with_capacity(text.len() + 16);
    let mut done = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            current = trimmed
                .trim_end()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .trim()
                .to_string();
        } else if !done && current == table {
            if let Some((lhs, rhs)) = line.split_once('=') {
                if lhs.trim().trim_matches('"') == key {
                    let (val_part, comment) = split_comment(rhs);
                    let lead = val_part.len() - val_part.trim_start().len();
                    let tok = val_part.trim();
                    let trail = &val_part[lead + tok.len()..];
                    out.push_str(lhs);
                    out.push('=');
                    out.push_str(&val_part[..lead]);
                    out.push_str(&format_like(tok, new));
                    out.push_str(trail);
                    out.push_str(comment);
                    done = true;
                    continue;
                }
            }
        }
        out.push_str(line);
    }
    if !done {
        return Err(format!("no key {key:?} in table {table:?}"));
    }
    Ok(out)
}

/// Split a TOML right-hand side into (value part, `# comment` + newline) outside strings.
fn split_comment(rhs: &str) -> (&str, &str) {
    let mut in_str = false;
    for (i, c) in rhs.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '#' if !in_str => return (&rhs[..i], &rhs[i..]),
            _ => {}
        }
    }
    // No comment: keep the newline out of the value.
    let body = rhs.trim_end_matches(['\n', '\r']);
    (body, &rhs[body.len()..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn k(s: &str) -> Step {
        Step::Key(s.into())
    }

    #[test]
    fn json_edit_is_surgical() {
        let text = "{\n  \"id\": \"x\",\n  \"lateral\": { \"b\": 16.0, \"c\": 1.35 },\n  \"s\": [ {\"k\": 31300.0}, {\"k\": 2} ],\n  \"w\": 1e-05\n}\n";
        let out = set_json(text, &[k("lateral"), k("b")], &json!(14.5)).unwrap();
        assert_eq!(out, text.replace("16.0", "14.5"));
        let out = set_json(text, &[k("s"), Step::Index(0), k("k")], &json!(37560.0)).unwrap();
        assert_eq!(out, text.replace("31300.0", "37560.0"));
        let out = set_json(text, &[k("s"), Step::Index(1), k("k")], &json!(3)).unwrap();
        assert!(out.contains("{\"k\": 3}"));
        let out = set_json(text, &[k("w")], &json!(2e-5)).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["w"], json!(2e-5));
        assert!(set_json(text, &[k("nope")], &json!(1)).is_err());
        assert!(set_json(text, &[k("s"), Step::Index(5), k("k")], &json!(1)).is_err());
    }

    #[test]
    fn json_insert_matches_layout() {
        let text = "{\n  \"id\": \"x\",\n  \"d\": { \"a\": 1.0 },\n  \"w\": [ {\"k\": 1} ]\n}\n";
        let out = insert_json(text, &[k("efficiency")], &json!(0.9)).unwrap();
        assert_eq!(out, text.replace("]\n}", "],\n  \"efficiency\": 0.9\n}"));
        let out = insert_json(text, &[k("d"), k("b")], &json!(2.0)).unwrap();
        assert!(out.contains("{ \"a\": 1.0, \"b\": 2.0 }"));
        let out = insert_json(text, &[k("w"), Step::Index(0), k("i")], &json!(0.5)).unwrap();
        assert!(out.contains("{\"k\": 1, \"i\": 0.5}"));
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["w"][0]["i"], json!(0.5));
        let out = insert_toml("a = 1\n[t]\nb = 2\n", &[k("c")], &json!(0.5)).unwrap();
        assert_eq!(out, "a = 1\nc = 0.5\n[t]\nb = 2\n");
    }

    #[test]
    fn toml_edit_keeps_comments() {
        let text = "# head\nmode = \"Simulation\"\nplasticity_scale = 1.0                 # comment\nx = 2\n[t]\nx = 5\n";
        let out = set_toml(text, &[k("plasticity_scale")], &json!(0.85)).unwrap();
        assert!(out.contains("plasticity_scale = 0.85                 # comment\n"));
        let out = set_toml(text, &[k("t"), k("x")], &json!(7)).unwrap();
        assert!(out.ends_with("[t]\nx = 7\n") && out.contains("\nx = 2\n"));
        let out = set_toml(text, &[k("x")], &json!(3)).unwrap();
        assert!(out.contains("\nx = 3\n[t]\nx = 5\n"));
        assert!(set_toml(text, &[k("missing")], &json!(1)).is_err());
    }
}
