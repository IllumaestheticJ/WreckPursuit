//! Power-oversteer exit sweep (slide-control assists): for each car and assist preset, run
//! [`wp_vehicle::metrics::power_oversteer_exit`] over a grid of corner speeds and driver
//! reaction times, then [`wp_vehicle::metrics::high_speed_catch`] at 110/130 km/h, and print
//! the worst slip and the recover rate.
//! `cargo run --release -p wp_vehicle --example slide [car_id ...]`

use wp_contracts::AssistProfile;
use wp_vehicle::metrics::{high_speed_catch, power_oversteer_exit, DriverStyle};

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
        tcs: wp_contracts::TcsLevel::Off,
        esc: false,
        ..AssistProfile::keyboard_default()
    };
    let gp_no_tcs = AssistProfile {
        tcs: wp_contracts::TcsLevel::Off,
        ..AssistProfile::gamepad_default()
    };
    // Countersteer slider (countersteer_help) at 0 / 50 / 100 % on the keyboard and gamepad
    // presets, plus the wheel preset (no assists) as the reference.
    let kb = |h: f32, tcs: bool| AssistProfile {
        countersteer_help: h,
        tcs: tcs.into(),
        esc: tcs,
        ..AssistProfile::keyboard_default()
    };
    let gp = |h: f32| AssistProfile {
        countersteer_help: h,
        ..AssistProfile::gamepad_default()
    };
    let _ = (no_tcs, gp_no_tcs);
    let presets = [
        ("keyboard cst 0", kb(0.0, true), DriverStyle::Keyboard),
        ("keyboard cst 50", kb(0.5, true), DriverStyle::Keyboard),
        ("keyboard cst 100", kb(1.0, true), DriverStyle::Keyboard),
        (
            "keyboard tcs/esc off cst 0",
            kb(0.0, false),
            DriverStyle::Keyboard,
        ),
        (
            "keyboard tcs/esc off cst 50",
            kb(0.5, false),
            DriverStyle::Keyboard,
        ),
        (
            "keyboard tcs/esc off cst 100",
            kb(1.0, false),
            DriverStyle::Keyboard,
        ),
        ("gamepad cst 0", gp(0.0), DriverStyle::Analog),
        ("gamepad cst 50", gp(0.5), DriverStyle::Analog),
        ("gamepad cst 100", gp(1.0), DriverStyle::Analog),
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
    println!("\nhigh-speed catch | car | preset | worst slip ° | caught | per-case slip°");
    for id in &ids {
        let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
        let car = wp_contracts::load_car(p).expect("car loads");
        for (name, a, style) in &presets {
            let mut worst = 0.0f32;
            let mut ok = 0;
            let mut cases = String::new();
            for kmh in [110.0, 130.0] {
                for kick in [0.8, 1.2] {
                    let r = high_speed_catch(&car, a, *style, kmh / 3.6, kick);
                    worst = worst.max(r.max_slip_deg);
                    ok += r.recovered as usize;
                    cases += &format!(
                        " {:.0}{}",
                        r.max_slip_deg,
                        if r.recovered { "" } else { "✗" }
                    );
                }
            }
            println!("catch | {id} | {name} | {worst:.0} | {ok}/4 |{cases}");
        }
    }
}
