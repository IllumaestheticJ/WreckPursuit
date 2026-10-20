//! Handbrake turns (playtest row 72): yaw after 1.5 s at 60 km/h with 50 % steer and a 0.6 s
//! handbrake pull, against the same steer without the handbrake, per car and preset.
//! `cargo run --release -p wp_vehicle --example handbrake [car_id ...]`

use wp_contracts::AssistProfile;
use wp_vehicle::metrics::handbrake_turn;

fn main() {
    let mut ids: Vec<String> = std::env::args().skip(1).collect();
    if ids.is_empty() {
        ids = [
            "rampart_50",
            "hakone_gt",
            "kestrel_ts",
            "vireo_xs",
            "lindwurm_4t",
            "sentinel_wolfhound",
        ]
        .map(String::from)
        .to_vec();
    }
    let presets = [
        ("keyboard", AssistProfile::keyboard_default()),
        ("gamepad", AssistProfile::gamepad_default()),
        ("wheel", AssistProfile::wheel_default()),
    ];
    println!("| car | preset | yaw ° handbrake | yaw ° no handbrake | max slip ° (hb) | rear ω end of pull | min rpm |");
    println!("|---|---|---|---|---|---|---|");
    for id in &ids {
        let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
        let car = wp_contracts::load_car(p).expect("car loads");
        for (name, a) in &presets {
            let hb = handbrake_turn(&car, a, 60.0 / 3.6, 0.5, 0.6, 1.5);
            let plain = handbrake_turn(&car, a, 60.0 / 3.6, 0.5, 0.0, 1.5);
            println!(
                "| {id} | {name} | {:.0} | {:.0} | {:.0} | {:.1} | {:.0} |",
                hb.yaw_deg, plain.yaw_deg, hb.max_slip_deg, hb.rear_omega_end, hb.min_rpm
            );
        }
    }
}
