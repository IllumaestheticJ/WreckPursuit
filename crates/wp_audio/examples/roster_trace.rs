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
    // Horn held 2.0 s: a standard horn and a truck horn (owner report #58).
    let horns: Vec<_> = ["tansy_13", "hauler_1500"]
        .iter()
        .map(|id| {
            let car = cars.iter().find(|c| c.def.id == *id).expect("roster car");
            (
                car,
                roster::HORN_HOLD_S,
                roster::run_horn(car, wp_audio::DriverKind::Player, roster::HORN_HOLD_S, 0.8),
            )
        })
        .collect();
    std::fs::write(&path, roster::render_json(&cases, &horns)).expect("write json");
    println!("{} cases -> {path}", cases.len());
}
