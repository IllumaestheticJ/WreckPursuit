//! Run one police scenario (`crates/wp_police/scenarios/*.ron`, [`wp_police::scenario`] format)
//! and print its state hash and JSON summary. Exit 1 on failed expectations, 2 on load errors.
//!
//! ```sh
//! cargo run --release -p wp_police --bin police-scenario -- crates/wp_police/scenarios/day_core_speeder.ron
//! ```
//! (Moved here from simcli by the integration lead: simcli only runs scenario format v2.)

use std::path::PathBuf;

fn main() {
    let Some(path) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: police-scenario <scenario.ron>");
        std::process::exit(2);
    };
    let sc = match wp_police::scenario::load_scenario(&path) {
        Ok(sc) => sc,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };
    let summary = wp_police::scenario::run_scenario(&sc);
    println!("state_hash {:016x}", summary.state_hash);
    println!(
        "{}",
        serde_json::to_string_pretty(&summary).expect("summary serializes")
    );
    if !summary.failures.is_empty() {
        eprintln!("FAILED: {:?}", summary.failures);
        std::process::exit(1);
    }
}
