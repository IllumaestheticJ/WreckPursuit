//! Power-oversteer exit sweep (slide-control assists): for each car and assist preset, run
//! [`wp_vehicle::metrics::power_oversteer_exit`] over a grid of corner speeds and driver
//! reaction times and print the worst slip and the recover rate.
//! `cargo run --release -p wp_vehicle --example slide [car_id ...]`

use wp_contracts::AssistProfile;
use wp_vehicle::metrics::{power_oversteer_exit, DriverStyle};

fn main() {
    let mut ids: Vec<String> = std::env::args().skip(1).collect();
    if ids.is_empty() {
        ids = [
            "rampart_50",
            "lucciola_350",
            "sperber_32",
            "hakone_gt",
            "kestrel_ts",
        ]
        .map(String::from)
        .to_vec();
    }
    let no_tcs = AssistProfile {
        tcs: false,
        esc: false,
        ..AssistProfile::keyboard_default()
    };
    let gp_no_tcs = AssistProfile {
        tcs: false,
        ..AssistProfile::gamepad_default()
    };
    let presets = [
        (
            "keyboard",
            AssistProfile::keyboard_default(),
            DriverStyle::Keyboard,
        ),
        ("keyboard tcs/esc off", no_tcs, DriverStyle::Keyboard),
        (
            "gamepad",
            AssistProfile::gamepad_default(),
            DriverStyle::Analog,
        ),
        ("gamepad tcs off", gp_no_tcs, DriverStyle::Analog),
        ("wheel", AssistProfile::wheel_default(), DriverStyle::Analog),
    ];
    println!("car | preset | gear | worst slip ° | recovered | per-case slip°");
    for id in &ids {
        let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
        let car = wp_contracts::load_car(p).expect("car loads");
        for (name, a, style) in &presets {
            let mut worst = 0.0f32;
            let mut ok = 0;
            let mut n = 0;
            let mut gear = 0;
            let mut cases = String::new();
            for lat_g in [0.5, 0.7] {
                for react in [0.3, 0.8, 2.0] {
                    let r = power_oversteer_exit(&car, a, *style, 30.0, lat_g, react);
                    worst = worst.max(r.max_slip_deg);
                    ok += r.recovered as usize;
                    n += 1;
                    gear = r.gear_at_throttle;
                    cases += &format!(
                        " {:.0}{}",
                        r.max_slip_deg,
                        if r.recovered { "" } else { "✗" }
                    );
                }
            }
            println!("{id} | {name} | {gear} | {worst:.0} | {ok}/{n} |{cases}");
        }
    }
}
