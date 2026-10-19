//! Runs the workspace determinism lint over this crate (machine I's `SIM_CRATES` list does
//! not include `wp_opponent` yet; requested in docs/integration/opponent.md).

use std::path::PathBuf;

#[test]
fn wp_opponent_passes_the_determinism_lint() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let findings = lint_determinism::lint_repo(&repo, &["wp_opponent"], &[]);
    let errors: Vec<_> = findings
        .iter()
        .filter(|f| f.severity == lint_determinism::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
}
