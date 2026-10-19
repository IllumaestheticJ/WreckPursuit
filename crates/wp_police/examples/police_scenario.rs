//! `cargo run --release -p wp_police --example police_scenario -- <scenario.ron> [--tables <dir>]`
//! Runs a police scenario, optionally with alternative tuning tables (for tuning sessions).

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("usage: police_scenario <scenario.ron> [--tables <dir>]");
        std::process::exit(2);
    };
    let sc = wp_police::scenario::load_scenario(std::path::Path::new(path)).expect("scenario");
    let cfg = match args
        .iter()
        .position(|a| a == "--tables")
        .and_then(|i| args.get(i + 1))
    {
        Some(dir) => wp_police::PoliceConfig::load(dir).expect("tables"),
        None => wp_police::PoliceConfig::builtin(),
    };
    let s = wp_police::scenario::run_scenario_with(&sc, &cfg);
    println!("{}", serde_json::to_string_pretty(&s).expect("json"));
}
