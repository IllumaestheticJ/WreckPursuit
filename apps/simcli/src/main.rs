//! `simcli <scenario.ron> [--data <dir>]` — run a scenario headless, print the state hash and
//! a JSON summary, exit 1 if any expectation fails.

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(scenario_path) = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(PathBuf::from)
    else {
        eprintln!("usage: simcli <scenario.ron> [--data <dir>]");
        std::process::exit(2);
    };
    let data = args
        .iter()
        .position(|a| a == "--data")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data"));
    // Police scenarios (scenarios/police/*.ron) have their own format and runner (wp_police).
    if scenario_path
        .components()
        .any(|c| c.as_os_str() == "police")
    {
        let sc = match wp_police::scenario::load_scenario(&scenario_path) {
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
        return;
    }
    let result =
        simcli::load_scenario(&scenario_path).and_then(|sc| simcli::run_scenario(&sc, &data));
    match result {
        Ok(summary) => {
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
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    }
}
