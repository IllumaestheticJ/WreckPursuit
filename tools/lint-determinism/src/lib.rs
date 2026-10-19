//! # lint-determinism
//!
//! A deliberately simple, line-based lint for simulation crates. It flags constructs that break
//! bit-exact determinism across runs or targets (native x86_64 vs `wasm32-unknown-unknown`):
//!
//! | rule | flags | why |
//! |---|---|---|
//! | `f64` | the `f64` type | hot paths stay `f32`; mixed precision drifts between targets |
//! | `transcendental` | `.sin()`, `.cos()`, `.powf()`, `.exp()`, `f32::atan2`, … | libm differs per platform; use `wp_math` |
//! | `hashmap` | `HashMap`, `HashSet` | iteration order is random; use `BTreeMap`/`IndexMap` |
//! | `wall-clock` | `std::time`, `Instant`, `SystemTime` | sim time comes from ticks only |
//! | `threads` | `std::thread`, `rayon` | unordered reductions |
//! | `unseeded-rng` | `rand::`, `thread_rng`, `getrandom` | randomness only via seeded `wp_math::Rng` |
//! | `f32-consts` (warning) | `std::f32::consts` | exact constants, but often a sign of hand-rolled trig that belongs in `wp_math` |
//!
//! Scanning stops at the first `#[cfg(test)]` line of a file (tests may do anything) and skips
//! `src/bin/`, `tests/`, `benches/` and comments. Suppress a finding on one line with a trailing
//! `// det-allow(<rule>): <reason>`, or for a whole file with an entry in `allow.txt`.

#![warn(missing_docs)]

use std::path::{Path, PathBuf};

/// Crates whose `src/` is linted. Everything reachable from a simulation step.
pub const SIM_CRATES: [&str; 9] = [
    "wp_contracts",
    "wp_math",
    "wp_vehicle",
    "wp_damage",
    "wp_world",
    "wp_police",
    "wp_economy",
    "wp_sim",
    "wp_weather",
];

/// Severity of a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Fails the lint.
    Error,
    /// Printed only.
    Warning,
}

/// One rule: name, severity, the patterns it matches and a fix hint.
pub struct Rule {
    /// Rule name used in `det-allow(...)` and `allow.txt`.
    pub name: &'static str,
    /// Severity.
    pub severity: Severity,
    /// Patterns; see [`Pattern`].
    pub patterns: &'static [Pattern],
    /// How to fix.
    pub hint: &'static str,
}

/// A pattern: `Word` must not be part of a longer identifier; `Text` is a plain substring.
pub enum Pattern {
    /// Whole identifier.
    Word(&'static str),
    /// Substring.
    Text(&'static str),
}

const TRIG: &[Pattern] = &[
    Pattern::Text(".sin("),
    Pattern::Text(".cos("),
    Pattern::Text(".tan("),
    Pattern::Text(".sin_cos("),
    Pattern::Text(".asin("),
    Pattern::Text(".acos("),
    Pattern::Text(".atan("),
    Pattern::Text(".atan2("),
    Pattern::Text(".sinh("),
    Pattern::Text(".cosh("),
    Pattern::Text(".tanh("),
    Pattern::Text(".asinh("),
    Pattern::Text(".acosh("),
    Pattern::Text(".atanh("),
    Pattern::Text(".exp("),
    Pattern::Text(".exp2("),
    Pattern::Text(".exp_m1("),
    Pattern::Text(".ln("),
    Pattern::Text(".log("),
    Pattern::Text(".log2("),
    Pattern::Text(".log10("),
    Pattern::Text(".ln_1p("),
    Pattern::Text(".powf("),
    Pattern::Text(".powi("),
    Pattern::Text(".cbrt("),
    Pattern::Text(".hypot("),
    Pattern::Text("f32::sin"),
    Pattern::Text("f32::cos"),
    Pattern::Text("f32::tan"),
    Pattern::Text("f32::asin"),
    Pattern::Text("f32::acos"),
    Pattern::Text("f32::atan"),
    Pattern::Text("f32::exp"),
    Pattern::Text("f32::ln"),
    Pattern::Text("f32::log"),
    Pattern::Text("f32::powf"),
    Pattern::Text("f32::powi"),
    Pattern::Text("f32::cbrt"),
    Pattern::Text("f32::hypot"),
];

/// Every rule, errors first.
pub const RULES: &[Rule] = &[
    Rule {
        name: "f64",
        severity: Severity::Error,
        patterns: &[Pattern::Word("f64")],
        hint: "keep simulation math in f32",
    },
    Rule {
        name: "transcendental",
        severity: Severity::Error,
        patterns: TRIG,
        hint: "use wp_math (sin_cos, …) or request a software implementation there",
    },
    Rule {
        name: "hashmap",
        severity: Severity::Error,
        patterns: &[Pattern::Word("HashMap"), Pattern::Word("HashSet")],
        hint: "use BTreeMap/BTreeSet or IndexMap",
    },
    Rule {
        name: "wall-clock",
        severity: Severity::Error,
        patterns: &[
            Pattern::Text("std::time"),
            Pattern::Word("Instant"),
            Pattern::Word("SystemTime"),
        ],
        hint: "derive time from the tick counter",
    },
    Rule {
        name: "threads",
        severity: Severity::Error,
        patterns: &[Pattern::Text("std::thread"), Pattern::Word("rayon")],
        hint: "no threads inside a sim step unless reduced in a fixed order",
    },
    Rule {
        name: "unseeded-rng",
        severity: Severity::Error,
        patterns: &[
            Pattern::Text("rand::"),
            Pattern::Word("thread_rng"),
            Pattern::Word("getrandom"),
        ],
        hint: "pass a seeded wp_math::Rng",
    },
    Rule {
        name: "f32-consts",
        severity: Severity::Warning,
        patterns: &[Pattern::Text("std::f32::consts")],
        hint: "constants are exact; make sure no libm call hides nearby",
    },
];

/// One finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// File, relative to the repo root.
    pub path: String,
    /// 1-based line.
    pub line: usize,
    /// Rule name.
    pub rule: &'static str,
    /// Severity.
    pub severity: Severity,
    /// The offending line, trimmed.
    pub text: String,
}

impl Finding {
    /// `path:line: error[rule]: text (hint)`.
    pub fn render(&self) -> String {
        let sev = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let hint = RULES
            .iter()
            .find(|r| r.name == self.rule)
            .map(|r| r.hint)
            .unwrap_or("");
        format!(
            "{}:{}: {sev}[{}]: {}  ({hint})",
            self.path, self.line, self.rule, self.text
        )
    }
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn matches(line: &str, p: &Pattern) -> bool {
    match p {
        // A float method always takes an argument or none; `x.log()` with no argument is never
        // `f32::log(self, base)`, so `name(` patterns skip an immediate `)` for `.log(`.
        Pattern::Text(t) if *t == ".log(" => line
            .match_indices(t)
            .any(|(i, _)| !line[i + t.len()..].starts_with(')')),
        Pattern::Text(t) => line.contains(t),
        Pattern::Word(w) => line.match_indices(w).any(|(i, _)| {
            let before = line[..i].chars().next_back();
            let after = line[i + w.len()..].chars().next();
            !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
        }),
    }
}

/// Remove `//` comments and the contents of simple `"..."` string literals.
fn code_part(line: &str) -> String {
    let mut out = String::new();
    let mut in_str = false;
    let mut prev = '\0';
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            if c == '"' && prev != '\\' {
                in_str = false;
                out.push('"');
            }
        } else if c == '"' {
            in_str = true;
            out.push('"');
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            break;
        } else {
            out.push(c);
        }
        prev = c;
        i += 1;
    }
    out
}

/// Lint one file's text. `path` is only used for reporting.
pub fn lint_source(path: &str, text: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let trimmed = raw.trim_start();
        if trimmed.starts_with("#[cfg(test)]") {
            break;
        }
        if trimmed.starts_with("//") {
            continue;
        }
        let code = code_part(raw);
        for rule in RULES {
            if !rule.patterns.iter().any(|p| matches(&code, p)) {
                continue;
            }
            if raw.contains(&format!("det-allow({})", rule.name)) {
                continue;
            }
            findings.push(Finding {
                path: path.to_string(),
                line: n + 1,
                rule: rule.name,
                severity: rule.severity,
                text: raw.trim().to_string(),
            });
        }
    }
    findings
}

/// A file-level suppression from `allow.txt`: `<path> <rule> <reason…>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allow {
    /// Repo-relative file path.
    pub path: String,
    /// Rule name.
    pub rule: String,
    /// Why it is fine.
    pub reason: String,
}

/// Parse `allow.txt` (`#` comments, blank lines ignored). Entries without a reason are rejected.
pub fn parse_allow(text: &str) -> Result<Vec<Allow>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.splitn(3, char::is_whitespace);
        let (Some(path), Some(rule), Some(reason)) = (it.next(), it.next(), it.next()) else {
            return Err(format!(
                "allow.txt:{}: need `<path> <rule> <reason>`",
                n + 1
            ));
        };
        if !RULES.iter().any(|r| r.name == rule) {
            return Err(format!("allow.txt:{}: unknown rule {rule:?}", n + 1));
        }
        out.push(Allow {
            path: path.to_string(),
            rule: rule.to_string(),
            reason: reason.trim().to_string(),
        });
    }
    Ok(out)
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == "bin") {
                continue;
            }
            rust_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

/// Lint every simulation crate under `repo/crates/<name>/src`, applying `allow`. Findings are
/// sorted by path and line.
pub fn lint_repo(repo: &Path, crates: &[&str], allow: &[Allow]) -> Vec<Finding> {
    let mut files = Vec::new();
    for c in crates {
        rust_files(&repo.join("crates").join(c).join("src"), &mut files);
    }
    files.sort();
    let mut findings = Vec::new();
    for f in files {
        let rel = f
            .strip_prefix(repo)
            .unwrap_or(&f)
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(&f).unwrap_or_default();
        findings.extend(
            lint_source(&rel, &text)
                .into_iter()
                .filter(|x| !allow.iter().any(|a| a.path == x.path && a.rule == x.rule)),
        );
    }
    findings
}

/// Allow entries that matched nothing (stale suppressions should be deleted).
pub fn unused_allows<'a>(repo: &Path, crates: &[&str], allow: &'a [Allow]) -> Vec<&'a Allow> {
    let all = lint_repo(repo, crates, &[]);
    allow
        .iter()
        .filter(|a| !all.iter().any(|f| f.path == a.path && f.rule == a.rule))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_arg_log_method_is_not_a_logarithm() {
        let log = Pattern::Text(".log(");
        assert!(!matches("    log: m.log().to_vec(),", &log));
        assert!(matches("let y = x.log(2.0);", &log));
    }

    fn rules(src: &str) -> Vec<&'static str> {
        lint_source("x.rs", src)
            .into_iter()
            .map(|f| f.rule)
            .collect()
    }

    #[test]
    fn flags_each_rule() {
        assert_eq!(rules("let a: f64 = 1.0;"), ["f64"]);
        assert_eq!(rules("let s = x.sin();"), ["transcendental"]);
        assert_eq!(rules("let s = f32::atan2(y, x);"), ["transcendental"]);
        assert_eq!(rules("let p = x.powf(2.0);"), ["transcendental"]);
        assert_eq!(rules("use std::collections::HashMap;"), ["hashmap"]);
        assert_eq!(rules("let t = std::time::Instant::now();"), ["wall-clock"]);
        assert_eq!(rules("std::thread::spawn(f);"), ["threads"]);
        assert_eq!(rules("let r = rand::random::<u8>();"), ["unseeded-rng"]);
        assert_eq!(rules("use std::f32::consts::PI;"), ["f32-consts"]);
    }

    #[test]
    fn ignores_safe_code_comments_strings_and_tests() {
        assert!(rules("let (s, c) = wp_math::sin_cos(x);").is_empty());
        assert!(rules("let v = x.sqrt().abs().mul_add(a, b);").is_empty());
        assert!(rules("let f64_count = 3; // uses f64 in a comment").is_empty());
        assert!(rules("let s = \"x.sin() HashMap\";").is_empty());
        assert!(rules("let my_HashMapish = 1;").is_empty());
        assert!(rules("#[cfg(test)]\nmod t { let a = 1.0f64.sin(); }").is_empty());
        assert!(rules("let a = 1.0_f64; // det-allow(f64): exact quantisation").is_empty());
    }

    #[test]
    fn allow_file_parses_and_rejects_bad_lines() {
        let a = parse_allow("# c\ncrates/a/src/x.rs f64 exact for f32 inputs\n").unwrap();
        assert_eq!(a[0].rule, "f64");
        assert!(parse_allow("crates/a.rs f64").is_err());
        assert!(parse_allow("crates/a.rs nope why").is_err());
    }
}
