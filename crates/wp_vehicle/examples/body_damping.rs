//! Roll and pitch free-response damping ratios (lead request, row 92 / Drift Lab W "far too
//! switchy"): kick a settled car about its roll or pitch axis and fit the decay.
//!
//! `cargo run --release -p wp_vehicle --example body_damping [car_id ...]`; with
//! `SUSPENSION_FROM=<dir>` the suspension blocks come from `<dir>/<car_id>.json` instead (to
//! compare against another branch's car data with the same code).

use wp_contracts::{LoadedCar, SuspensionDef};
use wp_vehicle::ride::{pitch_kick, roll_kick, Oscillation};

fn load(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    let mut car = wp_contracts::load_car(p).unwrap_or_else(|e| panic!("{id}: {e}"));
    if let Ok(dir) = std::env::var("SUSPENSION_FROM") {
        let text = std::fs::read_to_string(format!("{dir}/{id}.json")).expect("override file");
        let v: serde_json::Value = serde_json::from_str(&text).expect("json");
        let s: [SuspensionDef; 4] =
            serde_json::from_value(v["suspension"].clone()).expect("suspension");
        car.def.suspension = s;
    }
    car
}

fn osc(o: &Oscillation) -> String {
    if o.damped_hz > 0.0 {
        format!("{:.2} Hz ζ {:.2}", o.natural_hz, o.damping_ratio)
    } else {
        "no overshoot (ζ ≥ ~0.9)".into()
    }
}

fn main() {
    let ids: Vec<String> = std::env::args().skip(1).collect();
    println!("| car | roll kick | pitch kick |");
    println!("|---|---|---|");
    for id in &ids {
        let car = load(id);
        println!(
            "| {id} | {} | {} |",
            osc(&roll_kick(&car, 0.3)),
            osc(&pitch_kick(&car, 0.3))
        );
    }
}
