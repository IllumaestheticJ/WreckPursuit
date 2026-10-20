//! Tick traces for Drift Lab B tuning.
//! `cargo run --release -p wp_vehicle --example drift_trace -- drift <car> <driver> [help] [seed]`
//! (driver: good | naive | into | sloppy | lift | flood), `-- transition <car> [help]` (row 107), `-- handbrake <car> [kmh] [tap_s] [help] [counter|released]` (row 108) or `-- kick <car> <kmh> <kick> [help] [counter]`.

use wp_contracts::{AssistProfile, VehicleInput};
use wp_vehicle::drift_lab::{drift_run_with, slide_state, DriftDriver};
use wp_vehicle::math::{atan2, V3};
use wp_vehicle::rig::Rig;

fn line(rig: &Rig, steer: f32, thr: f32, t: usize) {
    let n = slide_state(rig);
    let tel = rig.vehicle.telemetry();
    println!(
        "t={:5.0}ms beta={:6.1} rear={:6.1} faa={:6.1} steer={:6.1} yaw={:5.2} v={:5.1} in=({steer:+.0},{thr:.0}) thr={:.2} rpm={:4.0} g={} mode={:?} env={:4.1} auth={:.2} held={:4.1} slide={}",
        t as f32 / 0.24,
        n.body_slip.to_degrees(),
        n.rear_axle_slip.to_degrees(),
        n.front_axle_angle.to_degrees(),
        n.steer.to_degrees(),
        n.yaw_rate,
        n.v_fwd * 3.6,
        tel.throttle,
        rig.state().engine_rpm,
        rig.vehicle.gear(),
        tel.drift.mode,
        tel.drift.envelope.to_degrees(),
        tel.drift.authority,
        tel.drift.hold_angle.to_degrees(),
        if tel.sliding { "yes" } else { "no" },
    );
}

fn main() {
    // DRIFT_TUNING='{"hold_gain":0.8}' overrides constants (missing fields = defaults).
    if let Ok(j) = std::env::var("DRIFT_TUNING") {
        wp_vehicle::drift::set_tuning_override(Some(
            serde_json::from_str(&j).expect("DRIFT_TUNING json"),
        ));
    }
    let a: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize, d: &str| a.get(i).cloned().unwrap_or(d.into());
    let id = arg(1, "rampart_50");
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    let car = wp_contracts::load_car(p).unwrap();
    if arg(0, "drift") == "handbrake" {
        let kmh: f32 = arg(2, "80").parse().unwrap();
        let tap: f32 = arg(3, "0.6").parse().unwrap();
        let help: f32 = arg(4, "0.6").parse().unwrap();
        let counter = arg(5, "counter") == "counter";
        let asst = AssistProfile {
            countersteer_help: help,
            tcs: false,
            esc: false,
            auto_gears: false,
            ..AssistProfile::keyboard_default()
        };
        let mut t = 0;
        let r = wp_vehicle::drift_lab::handbrake_entry_with(
            &car,
            &asst,
            kmh / 3.6,
            tap,
            counter,
            |rig, s, th| {
                if t % 6 == 0 {
                    line(rig, s, th, t);
                }
                t += 1;
            },
        );
        println!("{r:?}");
        return;
    }
    if arg(0, "drift") == "transition" {
        let help: f32 = arg(2, "0.6").parse().unwrap();
        let asst = AssistProfile {
            countersteer_help: help,
            tcs: false,
            esc: false,
            ..AssistProfile::keyboard_default()
        };
        let mut t = 0;
        let r = wp_vehicle::drift_lab::transition_run_with(&car, &asst, 40.0, 0.6, |rig, s, th| {
            if t % 12 == 0 {
                line(rig, s, th, t);
            }
            t += 1;
        });
        println!("{r:?}");
        return;
    }
    if arg(0, "drift") == "drift" {
        let driver = match arg(2, "good").as_str() {
            "naive" => DriftDriver::Naive,
            "sloppy" => DriftDriver::Sloppy,
            "lift" => DriftDriver::AbruptLift,
            "flood" => DriftDriver::Flood,
            "into" => DriftDriver::IntoTurn,
            _ => DriftDriver::Good,
        };
        let help: f32 = arg(3, "0.6").parse().unwrap();
        let seed: u64 = arg(4, "1").parse().unwrap();
        let asst = AssistProfile {
            countersteer_help: help,
            tcs: false,
            esc: false,
            ..AssistProfile::keyboard_default()
        };
        let mut t = 0;
        let r = drift_run_with(&car, &asst, driver, 40.0, 0.6, seed, |rig, s, th| {
            if t % 12 == 0 {
                line(rig, s, th, t);
            }
            t += 1;
        });
        println!("{r:?}");
        return;
    }
    let kmh: f32 = arg(2, "100").parse().unwrap();
    let kick: f32 = arg(3, "1.2").parse().unwrap();
    let help: f32 = arg(4, "0.6").parse().unwrap();
    let counter = a.get(5).is_some();
    let asst = AssistProfile {
        countersteer_help: help,
        tcs: false,
        esc: false,
        ..AssistProfile::keyboard_default()
    };
    let mut rig = Rig::new(&car);
    rig.teleport([0.0; 3], 0.0, kmh / 3.6, 0.0);
    for _ in 0..240 {
        rig.step(
            &VehicleInput {
                throttle: 0.3,
                ..Default::default()
            },
            &asst,
        );
    }
    let s = rig.state();
    let fwd = rig.vehicle.chassis().rot * V3::Y;
    rig.teleport(s.pos, atan2(-fwd.x, fwd.y), s.speed_mps, kick);
    for t in 0..480 {
        let st = slide_state(&rig);
        let steer = if counter && t > 36 && st.body_slip.to_degrees().abs() > 2.0 {
            -st.body_slip.signum()
        } else {
            0.0
        };
        rig.step(
            &VehicleInput {
                steer,
                throttle: 0.3,
                ..Default::default()
            },
            &asst,
        );
        if t % 6 == 0 {
            line(&rig, steer, 0.3, t);
        }
    }
}
