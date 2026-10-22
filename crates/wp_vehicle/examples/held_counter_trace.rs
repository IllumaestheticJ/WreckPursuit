//! Why a held countersteer key spins (the reference holds the drift: docs/reference/wf-clips,
//! 26 countersteer holds of 0.8–8.2 s with throttle, 1 spin). The entry-hold driver
//! (`examples/entry_hold.rs`): settle on a 40 m left-hand circle at `LATG` (default 0.5 g), the
//! into-turn key with the handbrake for 0.3 s (or `ENTRY=lift`), then past 12° of slide the
//! countersteer key + full throttle (`THR`) held, on the shipped player steering. `RADIUS` (m,
//! default 40), `SECS`.
//!
//! Prints every 0.1 s: slide angle (+ = the left drift, °), yaw rate (°/s), speed, the front
//! wheels' angle and the front axle's travel (°), front / rear axle slip angle (°), rear
//! slip ratio, front and rear lateral force (kN, body frame, + = left) and rear drive force (kN).
//!
//! `cargo run --release -p wp_vehicle --example held_counter_trace [-- car]`

use std::path::PathBuf;

use wp_contracts::{LoadedCar, VehicleInput, PHYSICS_DT};
use wp_vehicle::drift_lab::{settle_on_circle, slide_state, DRIFT_GEAR};
use wp_vehicle::drift_lab_w::{load_any_car, reference_assists};

const TPS: usize = 240;

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    load_any_car(&data, id).unwrap()
}

fn main() {
    let id = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "parity_rwd_d".into());
    let lat_g: f32 = std::env::var("LATG")
        .ok()
        .map_or(0.5, |v| v.parse().unwrap());
    let lift = std::env::var("ENTRY").as_deref() == Ok("lift");
    let secs: f32 = std::env::var("SECS")
        .ok()
        .map_or(5.0, |v| v.parse().unwrap());
    let radius: f32 = std::env::var("RADIUS")
        .ok()
        .map_or(40.0, |v| v.parse().unwrap());
    let thr_hold: f32 = std::env::var("THR")
        .ok()
        .map_or(1.0, |v| v.parse().unwrap());
    // TYRE='{…}': fields over the shipped car / tyre tune (A/B).
    if let Ok(j) = std::env::var("TYRE") {
        let mut v = serde_json::to_value(wp_vehicle::car_lab::CarTune::shipped()).unwrap();
        for (k, x) in serde_json::from_str::<serde_json::Value>(&j)
            .unwrap()
            .as_object()
            .unwrap()
        {
            v[k] = x.clone();
        }
        wp_vehicle::car_lab::set_override(Some(serde_json::from_value(v).unwrap()));
    }
    let c = car(&id);
    let a = reference_assists();
    let mut rig = settle_on_circle(&c, &a, radius, lat_g);
    rig.set_player_input(true);
    let mut entered = false;
    println!(
        "{id}, {} entry, {radius} m at {lat_g} g, held throttle {thr_hold}",
        if lift { "lift" } else { "handbrake" }
    );
    println!("| t | key | thr | slide ° | yaw °/s | km/h | wheels ° | travel ° | front α ° | rear α ° | rear κ | Fy front kN | Fy rear kN | Fx rear kN |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for k in 0..(secs * TPS as f32) as usize {
        let t = k as f32 * PHYSICS_DT;
        let s = slide_state(&rig);
        let angle = (-s.body_slip).to_degrees();
        entered |= angle > 12.0;
        let (steer, throttle, hb) = if !entered {
            if lift {
                (-1.0, if t < 0.4 { 0.0 } else { 1.0 }, 0.0)
            } else {
                (-1.0, 1.0, if t < 0.3 { 1.0 } else { 0.0 })
            }
        } else {
            (1.0, thr_hold, 0.0)
        };
        rig.step(
            &VehicleInput {
                steer,
                throttle,
                handbrake: hb,
                gear_down: !a.auto_gears && rig.vehicle.gear() > DRIFT_GEAR,
                ..Default::default()
            },
            &a,
        );
        if k % (TPS / 10) == 0 {
            let st = rig.state();
            let w = &st.wheel_states;
            let tel = rig.vehicle.telemetry();
            let s = slide_state(&rig);
            let avg = |f: &dyn Fn(usize) -> f32, i: [usize; 2]| 0.5 * (f(i[0]) + f(i[1]));
            println!(
                "| {t:.1} | {steer:+.0} | {throttle:.1} | {:+.1} | {:+.0} | {:.0} | {:+.1} | {:+.1} | {:+.1} | {:+.1} | {:+.2} | {:+.1} | {:+.1} | {:+.1} |",
                -s.body_slip.to_degrees(),
                s.yaw_rate.to_degrees(),
                st.speed_mps * 3.6,
                tel.steer_rad.to_degrees(),
                s.front_axle_angle.to_degrees(),
                avg(&|i| w[i].slip_angle.to_degrees(), [0, 1]),
                avg(&|i| w[i].slip_angle.to_degrees(), [2, 3]),
                avg(&|i| w[i].slip_ratio, [2, 3]),
                (tel.fy[0] + tel.fy[1]) / 1000.0,
                (tel.fy[2] + tel.fy[3]) / 1000.0,
                (tel.fx[2] + tel.fx[3]) / 1000.0,
            );
        }
    }
}
