//! Keyboard throttle smoothing table (owner row 98): keyboard preset with TCS / ESC off, the
//! smoothing at 0 (before) and the preset default (60 %): standing-start 0–100 km/h and
//! wheelspin time with the key held, and power-oversteer corner exits (30 m circle, lat 0.5 /
//! 0.7 g × steering held 0.3 / 0.8 / 2 s with full throttle, then released): spins and
//! recoveries of 6. `cargo run --release -p wp_vehicle --example throttle [car_id ...]`

use wp_contracts::AssistProfile;
use wp_vehicle::metrics::{keyboard_launch, power_oversteer_exit_tuned, DriverStyle};
use wp_vehicle::DriftTuning;

fn main() {
    let mut ids: Vec<String> = std::env::args().skip(1).collect();
    if ids.is_empty() {
        ids = [
            "shiro_turbo",
            "sperber_32",
            "rampart_50",
            "hakone_gt",
            "debug_sedan",
        ]
        .map(String::from)
        .to_vec();
    }
    let a = AssistProfile {
        tcs: wp_contracts::TcsLevel::Off,
        esc: false,
        ..AssistProfile::keyboard_default()
    };
    println!("| car | smoothing | 0–100 s | wheelspin s | exits spun / recovered of 6 | held 2 s exits spun |");
    println!("|---|---|---|---|---|---|");
    for id in &ids {
        let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
        let car = wp_contracts::load_car(p).unwrap();
        for (name, shaping) in [("0 (default)", 0.0f32), ("60 % (lab)", 0.6)] {
            let t = DriftTuning {
                throttle_shaping: shaping,
                ..DriftTuning::default()
            };
            let l = keyboard_launch(&car, &a, t);
            let (mut spun, mut rec, mut held_spun) = (0, 0, 0);
            for lat in [0.5f32, 0.7] {
                for react in [0.3f32, 0.8, 2.0] {
                    let r = power_oversteer_exit_tuned(
                        &car,
                        &a,
                        DriverStyle::Keyboard,
                        30.0,
                        lat,
                        react,
                        t,
                    );
                    spun += (r.max_slip_deg >= 90.0) as usize;
                    rec += r.recovered as usize;
                    if react == 2.0 {
                        held_spun += (r.max_slip_deg >= 90.0) as usize;
                    }
                }
            }
            println!(
                "| {id} | {name} | {} | {:.2} | {spun} / {rec} | {held_spun} of 2 |",
                l.to_100_s.map_or("—".into(), |v| format!("{v:.2}")),
                l.wheelspin_s
            );
        }
    }
}
