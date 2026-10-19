//! Field-by-field diff of two data files (two cars, two modes, a file before/after a promote).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One differing field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    /// JSON pointer.
    pub pointer: String,
    /// Value in A (null = missing).
    pub a: Value,
    /// Value in B (null = missing).
    pub b: Value,
    /// (b - a) / |a| for two numbers with a != 0.
    pub rel: Option<f64>,
}

/// Diff result.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Diff {
    /// File A.
    pub a: String,
    /// File B.
    pub b: String,
    /// Differing scalars (and missing keys), in document order, at most `limit`.
    pub rows: Vec<Row>,
    /// Total differences found.
    pub count: usize,
    /// Fields compared.
    pub compared: usize,
}

/// Diff two parsed documents; keeps the first `limit` rows.
pub fn diff(a: &Value, b: &Value, limit: usize) -> Diff {
    let mut d = Diff::default();
    walk(a, b, String::new(), &mut d, limit);
    d
}

fn push(d: &mut Diff, limit: usize, pointer: String, a: &Value, b: &Value) {
    d.count += 1;
    if d.rows.len() < limit {
        let rel = match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) if x != 0.0 => Some((y - x) / x.abs()),
            _ => None,
        };
        d.rows.push(Row {
            pointer,
            a: a.clone(),
            b: b.clone(),
            rel,
        });
    }
}

fn walk(a: &Value, b: &Value, at: String, d: &mut Diff, limit: usize) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let mut keys: Vec<&String> = x.keys().collect();
            for k in y.keys() {
                if !x.contains_key(k) {
                    keys.push(k);
                }
            }
            for k in keys {
                let p = format!("{at}/{}", k.replace('~', "~0").replace('/', "~1"));
                walk(
                    x.get(k).unwrap_or(&Value::Null),
                    y.get(k).unwrap_or(&Value::Null),
                    p,
                    d,
                    limit,
                );
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            for i in 0..x.len().max(y.len()) {
                walk(
                    x.get(i).unwrap_or(&Value::Null),
                    y.get(i).unwrap_or(&Value::Null),
                    format!("{at}/{i}"),
                    d,
                    limit,
                );
            }
        }
        _ => {
            d.compared += 1;
            let equal = match (a.as_f64(), b.as_f64()) {
                (Some(x), Some(y)) => x == y,
                _ => a == b,
            };
            if !equal {
                push(d, limit, at, a, b);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn finds_changed_missing_and_relative() {
        let a = json!({"m": 1400.0, "w": [1, 2], "x": "a", "only_a": true});
        let b = json!({"m": 1260.0, "w": [1, 3, 4], "x": "a", "only_b": 1});
        let d = diff(&a, &b, 100);
        let ptrs: Vec<&str> = d.rows.iter().map(|r| r.pointer.as_str()).collect();
        // Keys compare in sorted order (serde_json maps are BTreeMaps); B-only keys last.
        assert_eq!(ptrs, ["/m", "/only_a", "/w/1", "/w/2", "/only_b"]);
        assert!((d.rows[0].rel.unwrap() + 0.1).abs() < 1e-12);
        assert_eq!(diff(&a, &a, 100).count, 0);
        assert_eq!(diff(&a, &b, 2).rows.len(), 2);
        assert_eq!(diff(&a, &b, 2).count, 5);
    }
}
