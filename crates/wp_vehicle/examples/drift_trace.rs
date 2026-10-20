//! Tick trace of a released-keys yaw kick (Drift Lab B debugging).
//! `cargo run --release -p wp_vehicle --example drift_trace <car> <kmh> <kick> [help] [counter]`

use wp_contracts::{AssistProfile, VehicleInput};
use wp_vehicle::drift_lab::slide_state;
use wp_vehicle::math::{atan2, V3};
use wp_vehicle::rig::Rig;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let id = a.first().cloned().unwrap_or("rampart_50".into());
    let kmh: f32 = a.get(1).map(|s| s.parse().unwrap()).unwrap_or(100.0);
    let kick: f32 = a.get(2).map(|s| s.parse().unwrap()).unwrap_or(1.2);
    let help: f32 = a.get(3).map(|s| s.parse().unwrap()).unwrap_or(0.6);
    let counter = a.get(4).is_some();
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    let car = wp_contracts::load_car(p).unwrap();
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
            let n = slide_state(&rig);
            println!(
                "t={:4.0}ms beta={:6.1} rear={:6.1} faa={:6.1} steer={:6.1} yaw={:5.2} v={:5.1}",
                t as f32 / 0.24,
                n.body_slip.to_degrees(),
                n.rear_axle_slip.to_degrees(),
                n.front_axle_angle.to_degrees(),
                n.steer.to_degrees(),
                n.yaw_rate,
                n.v_fwd * 3.6
            );
        }
    }
}
