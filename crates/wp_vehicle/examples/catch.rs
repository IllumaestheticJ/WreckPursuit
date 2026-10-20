//! Slide-catch timing table (owner row 90): for each car, keyboard and gamepad presets (TCS /
//! ESC off, the slides the countersteer help exists for) at countersteer 30 / 60 / 100 %, run
//! [`wp_vehicle::metrics::slide_catch`] over power-oversteer, lift-off and 100 km/h yaw-kick slides with the
//! steering held in for 0.1 / 0.3 / 0.6 s, and print the median and worst time the wheels lag
//! the travel by more than 3°, the peak error, plus catches for a released-keys driver and one who countersteers 150 ms late.
//! `cargo run --release -p wp_vehicle --example catch [car_id ...]`

use wp_contracts::AssistProfile;
use wp_vehicle::metrics::{slide_catch, Catcher, DriverStyle, KeyRamp, SlideCase};

/// The testbed's keyboard steering ramp (`apps/testbed-web/src/input.ts`): since row 90 a
/// reversal starts from centre and a release runs at 12/s (was 6/s, no snap).
const TESTBED_RAMP: KeyRamp = KeyRamp {
    up: 4.0,
    down: 12.0,
    through_zero: true,
};

fn main() {
    let mut ids: Vec<String> = std::env::args().skip(1).collect();
    if ids.is_empty() {
        ids = ["shiro_turbo", "sperber_32", "rampart_50", "debug_sedan"]
            .map(String::from)
            .to_vec();
    }
    println!("| car | preset | help | lag > 3° median / worst s | peak error | released caught | late 150 ms caught | key press → aligned, median s |");
    println!("|---|---|---|---|---|---|---|---|");
    for id in &ids {
        let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
        let car = wp_contracts::load_car(p).unwrap();
        let kb = AssistProfile::keyboard_default();
        for (name, base, style, ramp) in [
            ("keys", kb, DriverStyle::Keyboard, None),
            (
                "keys + testbed ramp",
                kb,
                DriverStyle::Keyboard,
                Some(TESTBED_RAMP),
            ),
            (
                "gamepad",
                AssistProfile::gamepad_default(),
                DriverStyle::Analog,
                None,
            ),
        ] {
            for help in [0.3f32, 0.6, 1.0] {
                let a = AssistProfile {
                    countersteer_help: help,
                    tcs: false,
                    esc: false,
                    ..base
                };
                let mut times = Vec::new();
                let mut peak = 0.0f32;
                let mut counter = Vec::new();
                let (mut rel, mut late, mut n) = (0, 0, 0);
                for case in [
                    SlideCase::PowerOversteer,
                    SlideCase::LiftOff,
                    SlideCase::YawKick,
                ] {
                    for hold in [0.1f32, 0.3, 0.6] {
                        n += 1;
                        let r = slide_catch(&car, &a, style, case, Catcher::Released, hold, ramp);
                        times.push(r.lag_s);
                        peak = peak.max(r.peak_err_deg);
                        rel += r.caught as usize;
                        let l = Catcher::LateCountersteer { delay_s: 0.15 };
                        let lr = slide_catch(&car, &a, style, case, l, hold, ramp);
                        late += lr.caught as usize;
                        if let Some(t) = lr.counter_s {
                            counter.push(t);
                        }
                    }
                }
                times.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let med = times.get(times.len() / 2).copied().unwrap_or(f32::NAN);
                let worst = times.last().copied().unwrap_or(f32::NAN);
                counter.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let cmed = counter.get(counter.len() / 2).copied().unwrap_or(f32::NAN);
                println!(
                    "| {id} | {name} | {:.0} % | {med:.3} / {worst:.3} | {peak:.0}° | {rel}/{n} | {late}/{n} | {cmed:.3} |",
                    help * 100.0
                );
            }
        }
    }
}
