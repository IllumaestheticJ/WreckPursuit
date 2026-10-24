//! # wasm-determinism
//!
//! `simcli`'s scenario runner built for `wasm32-unknown-unknown` behind a minimal C ABI (no
//! wasm-bindgen, so Node can load it with plain `WebAssembly.instantiate`). `run.mjs` passes a
//! scenario and a data bundle (`simcli bundle`) and prints the resulting `hashes.txt`, which CI
//! diffs against the native `simcli hashes` output.
//!
//! ABI (all lengths in bytes, strings UTF-8):
//! - `wp_alloc(len) -> ptr`, `wp_free(ptr, len)`: buffers the host writes inputs into.
//! - `wp_run(scenario_ptr, scenario_len, bundle_ptr, bundle_len) -> status` (0 ok, 1 error):
//!   runs the scenario; the result text (hashes.txt, or the error message) is then available via
//!   `wp_result_ptr()` / `wp_result_len()` until the next `wp_run`.

#![warn(missing_docs)]

use std::cell::RefCell;

use simcli::{MemData, RunOptions, Scenario};

thread_local! {
    static RESULT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Run a scenario (RON text) against a data bundle (`{"files": {...}}` JSON) and return
/// `hashes.txt`. The same function the native test calls, so both targets share one path.
pub fn run_hashes(scenario_ron: &str, bundle_json: &str) -> Result<String, String> {
    let data: MemData = serde_json::from_str(bundle_json).map_err(|e| format!("bundle: {e}"))?;
    let sc = Scenario::from_ron("wasm", scenario_ron).map_err(|e| e.to_string())?;
    let out = simcli::run(&sc, &data, &RunOptions::default()).map_err(|e| e.to_string())?;
    Ok(out.hashes_txt())
}

/// Allocate `len` bytes for the host to write into.
#[no_mangle]
pub extern "C" fn wp_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// Free a buffer from [`wp_alloc`].
///
/// # Safety
/// `ptr` must come from `wp_alloc(len)` with the same `len` and not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn wp_free(ptr: *mut u8, len: usize) {
    drop(Vec::from_raw_parts(ptr, 0, len.max(1)));
}

/// Run a scenario; see the crate docs. Returns 0 on success, 1 on error.
///
/// # Safety
/// Both pointer/length pairs must describe readable memory holding UTF-8 text.
#[no_mangle]
pub unsafe extern "C" fn wp_run(
    scenario_ptr: *const u8,
    scenario_len: usize,
    bundle_ptr: *const u8,
    bundle_len: usize,
) -> u32 {
    let text = |p: *const u8, n: usize| {
        String::from_utf8_lossy(std::slice::from_raw_parts(p, n)).into_owned()
    };
    let result = run_hashes(
        &text(scenario_ptr, scenario_len),
        &text(bundle_ptr, bundle_len),
    );
    let (status, bytes) = match result {
        Ok(h) => (0, h.into_bytes()),
        Err(e) => (1, e.into_bytes()),
    };
    RESULT.with(|r| *r.borrow_mut() = bytes);
    status
}

/// Pointer to the last result text.
#[no_mangle]
pub extern "C" fn wp_result_ptr() -> *const u8 {
    RESULT.with(|r| r.borrow().as_ptr())
}

/// Length of the last result text.
#[no_mangle]
pub extern "C" fn wp_result_len() -> usize {
    RESULT.with(|r| r.borrow().len())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn native_run_hashes_matches_simcli() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let bundle = simcli::MemData::from_dir(&root.join("data")).unwrap();
        let path = root.join("apps/simcli/scenarios/smoke/straight_line.ron");
        let ron = std::fs::read_to_string(&path).unwrap();
        let got = super::run_hashes(&ron, &serde_json::to_string(&bundle).unwrap()).unwrap();
        let golden =
            std::fs::read_to_string(root.join("apps/simcli/golden/smoke_straight_line/hashes.txt"))
                .unwrap();
        // Every `<tick> <hash>` line must match, as `simcli goldens` checks it. The header
        // must match too except its contract version: a contract bump (0.3.0 -> 0.3.1 for
        // `DnfReason::Stuck`) does not re-bless every golden, and `simcli goldens` accepts it.
        let diff = simcli::golden::compare_hashes(&golden, &got);
        assert!(diff.matched, "hashes differ: {diff:?}");
        let header = |t: &str| {
            t.lines()
                .next()
                .unwrap_or_default()
                .split_whitespace()
                .filter(|w| !w.starts_with("contract="))
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert_eq!(header(&got), header(&golden), "header (systems, hash spec)");
    }
}
