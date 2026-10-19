//! The determinism audit: runs the lint over every simulation crate as part of
//! `cargo test --workspace`, so a stray `f64`, libm call, `HashMap` or wall-clock read fails CI.

use std::path::Path;

use lint_determinism::{lint_repo, parse_allow, unused_allows, Severity, SIM_CRATES};

#[test]
fn sim_crates_pass_the_determinism_lint() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo = here.join("../..");
    let allow = parse_allow(&std::fs::read_to_string(here.join("allow.txt")).unwrap()).unwrap();
    let errors: Vec<String> = lint_repo(&repo, &SIM_CRATES, &allow)
        .into_iter()
        .filter(|f| f.severity == Severity::Error)
        .map(|f| f.render())
        .collect();
    assert!(
        errors.is_empty(),
        "determinism lint (cargo run -p lint-determinism):\n{}",
        errors.join("\n")
    );
    let stale: Vec<_> = unused_allows(&repo, &SIM_CRATES, &allow);
    assert!(stale.is_empty(), "stale allow.txt entries: {stale:?}");
}
