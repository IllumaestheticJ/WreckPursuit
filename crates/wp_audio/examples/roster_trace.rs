//! Write the roster audio scenarios (every car in `data/cars` as player, AI racer, police unit
//! and traffic; see `tests/support/roster.rs`) as JSON for
//! `apps/testbed-web/e2e/audio_roster.mjs`, which renders each through the browser's real
//! voice pool, recorded and procedural.
//!
//! `cargo run -p wp_audio --example roster_trace -- out.json`

#[path = "../tests/support/roster.rs"]
mod roster;

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "roster_trace.json".into());
    let cars = roster::roster();
    let mut cases = Vec::new();
    for car in &cars {
        for (kind, dist) in roster::ROLES {
            cases.push((car, kind, dist, roster::run_case(car, kind, dist)));
        }
    }
    std::fs::write(&path, roster::render_json(&cases)).expect("write json");
    println!("{} cases -> {path}", cases.len());
}
