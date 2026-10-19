//! `lint-determinism [--repo DIR] [--allow FILE]` — print findings, exit 1 on any error-level
//! finding or stale allow entry. See the library docs for the rules.

use std::path::PathBuf;
use std::process::ExitCode;

use lint_determinism::{lint_repo, parse_allow, unused_allows, Severity, SIM_CRATES};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let get = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from)
    };
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo = get("--repo").unwrap_or_else(|| here.join("../.."));
    let allow_path = get("--allow").unwrap_or_else(|| here.join("allow.txt"));
    let allow = match std::fs::read_to_string(&allow_path)
        .map_err(|e| format!("{}: {e}", allow_path.display()))
        .and_then(|t| parse_allow(&t))
    {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let findings = lint_repo(&repo, &SIM_CRATES, &allow);
    let mut errors = 0;
    for f in &findings {
        println!("{}", f.render());
        errors += (f.severity == Severity::Error) as usize;
    }
    for a in unused_allows(&repo, &SIM_CRATES, &allow) {
        println!(
            "error[stale-allow]: {} {} matches nothing; delete it",
            a.path, a.rule
        );
        errors += 1;
    }
    println!(
        "lint-determinism: {} finding(s), {errors} error(s) in {:?}",
        findings.len(),
        SIM_CRATES
    );
    if errors > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
