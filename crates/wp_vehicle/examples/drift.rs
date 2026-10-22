//! Drift-hold table (owner row 91): for each car, keyboard and gamepad (TCS / ESC off) at
//! countersteer 30 / 60 / 100 %, run [`wp_vehicle::metrics::drift_run`] with a good driver
//! (key into the turn, throttle tapped to hold 25°), a sloppy one (full throttle held), a
//! naive one (keys released) and an abrupt lift after 2 s of holding.
//! `cargo run --release -p wp_vehicle --example drift [car_id ...]`

use wp_contracts::AssistProfile;
use wp_vehicle::metrics::{drift_run, DriftDriver, DriverStyle};

fn main() {
    let mut ids: Vec<String> = std::env::args().skip(1).collect();
    if ids.is_empty() {
        ids = [
            "shiro_turbo",
            "sperber_32",
            "rampart_50",
            "debug_sedan",
            "junkyard_beater",
        ]
        .map(String::from)
        .to_vec();
    }
    println!("| car | preset | help | good: drift s / sd ° / spun | sloppy: drift s / max ° / spun | naive: recovered | lift: max ° / recovered |");
    println!("|---|---|---|---|---|---|---|");
    for id in &ids {
        let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
        let car = wp_contracts::load_car(p).unwrap();
        for (name, base, style) in [
            (
                "keys",
                AssistProfile::keyboard_default(),
                DriverStyle::Keyboard,
            ),
            (
                "gamepad",
                AssistProfile::gamepad_default(),
                DriverStyle::Analog,
            ),
        ] {
            for help in [0.3f32, 0.6, 1.0] {
                let a = AssistProfile {
                    countersteer_help: help,
                    tcs: false,
                    esc: false,
                    ..base
                };
                let g = drift_run(&car, &a, style, DriftDriver::Good { target_deg: 25.0 });
                let s = drift_run(&car, &a, style, DriftDriver::Sloppy);
                let n = drift_run(&car, &a, style, DriftDriver::Released);
                let l = drift_run(&car, &a, style, DriftDriver::AbruptLift { after_s: 2.0 });
                println!(
                    "| {id} | {name} | {:.0} % | {:.1} / {:.1} / {} | {:.1} / {:.0} / {} | {} | {:.0} / {} |",
                    help * 100.0,
                    g.drift_s,
                    g.slip_sd_deg,
                    if g.spun { "spun" } else { "no" },
                    s.drift_s,
                    s.max_slip_deg,
                    if s.spun { "spun" } else { "no" },
                    if n.recovered { "yes" } else { "no" },
                    l.max_slip_deg,
                    if l.recovered { "yes" } else { "no" },
                );
            }
        }
    }
}
