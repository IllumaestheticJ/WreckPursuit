//! Tick trace of one twitch case (debugging aid): prints ticks where the rack moves faster
//! than 150 °/s (keyboard ramp 6/s in and out). `cargo run --release -p wp_vehicle --example twitch_trace -- <car> <case 0-4>`
use wp_contracts::AssistProfile;
use wp_vehicle::twitch_lab::{twitch_run_with, Case, KeyRamp};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let id = a.first().cloned().unwrap_or("sperber_32".into());
    let case = Case::ALL[a.get(1).map_or(3, |c| c.parse().unwrap())];
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    let car = wp_contracts::load_car(p).unwrap();
    let mut prev = 0.0f32;
    let mut t = 0;
    let r = twitch_run_with(
        &car,
        &AssistProfile::keyboard_default(),
        KeyRamp::RAW,
        case,
        |rig| {
            let tel = rig.vehicle.telemetry();
            let s = wp_vehicle::drift_lab::slide_state(rig);
            let rate = (tel.steer_rad - prev) * 240.0;
            prev = tel.steer_rad;
            if rate.abs().to_degrees() > 150.0 {
                println!(
                "t={:5.0}ms rate={:5.0} steer={:5.1} rear={:5.1} beta={:5.1} faa={:5.1} mode={:?} onset={:.2} sliding={}",
                t as f32 / 0.24,
                rate.to_degrees(),
                tel.steer_rad.to_degrees(),
                s.rear_axle_slip.to_degrees(),
                s.body_slip.to_degrees(),
                s.front_axle_angle.to_degrees(),
                tel.drift.mode,
                tel.drift.onset,
                tel.sliding
            );
            }
            t += 1;
        },
    );
    println!("{r:?}");
}
