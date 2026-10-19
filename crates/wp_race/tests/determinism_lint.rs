//! Run the repo's determinism lint (`tools/lint-determinism`) over `wp_race/src`. The race judge
//! runs inside the server tick, so it follows the same rules as the simulation crates.

use std::path::Path;

use lint_determinism::{lint_repo, Severity};

#[test]
fn wp_race_passes_the_determinism_lint() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let findings = lint_repo(&repo, &["wp_race"], &[]);
    let errors: Vec<String> = findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .map(|f| f.render())
        .collect();
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}
