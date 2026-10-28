//! Handbrake + held key vs the countersteer help (owner: "on the lindwurm 4T, hitting the
//! handbrake still forces the car to want to countersteer despite opposing keyboard press").
//!
//! At `KMH` (default 70) the driver goes full left + handbrake for `HB_S` (default 0.6 s),
//! then keeps holding left (handbrake released). Prints every 0.05 s: key, handbrake, the
//! centre rack angle and the front-left road-wheel angle (°, + = left = the key's side), the
//! front axle's travel, body slip and the rear-axle slip.
//!
//! `CAR=lindwurm_4t DEV=keyboard|gamepad CSTR=0.5 PLAYER=1 cargo run --release -p wp_vehicle --example hb_cstr`

use std::path::PathBuf;

use wp_contracts::{AssistProfile, LoadedCar, VehicleInput, PHYSICS_DT};
use wp_vehicle::drift_lab::slide_state;
use wp_vehicle::drift_lab_w::load_any_car;
use wp_vehicle::rig::Rig;

const TPS: usize = 240;

fn env<T: std::str::FromStr>(k: &str, d: T) -> T {
    std::env::var(k)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(d)
}

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    load_any_car(&data, id).unwrap()
}

fn main() {
    let id: String = env("CAR", "lindwurm_4t".to_string());
    let dev: String = env("DEV", "keyboard".to_string());
    let help: f32 = env("CSTR", 0.5);
    let kmh: f32 = env("KMH", 70.0);
    let hb_s: f32 = env("HB_S", 0.6);
    let player: u8 = env("PLAYER", 1);
    let throttle: f32 = env("THR", 0.0);
    // Key before the pull (PRE_S seconds of PRE_KEY), and the key during / after (KEY).
    let pre_s: f32 = env("PRE_S", 0.0);
    let pre_key: f32 = env("PRE_KEY", -1.0);
    let key: f32 = env("KEY", -1.0);
    let key_at: f32 = env("KEY_AT", 0.0);
    let hb_thr: f32 = env("HB_THR", 0.0);
    let quiet: u8 = env("QUIET", 0);
    // From KEY_END (s after the pull) the key is KEY2 (0 = released).
    let key_end: f32 = env("KEY_END", 1.0e3);
    let key2: f32 = env("KEY2", 0.0);
    // LAB=w: Drift Lab W's full plain set as the lab override (the testbed re-applies a
    // saved override on load).
    let lab: String = env("LAB", String::new());
    use wp_vehicle::car_lab::CarTune;
    use wp_vehicle::plain::{set_override, PlainSteer};
    match lab.as_str() {
        "w" => set_override(Some(PlainSteer::lab_w())),
        "vr" => set_override(Some(CarTune::spin_vr_plain())),
        "trans" => set_override(Some(CarTune::transition_plain())),
        "v2" => set_override(Some(CarTune::spin_v2_plain())),
        "smart" => {
            wp_vehicle::grip_lab::set_override(Some(wp_vehicle::grip_lab::GripBalance::step14()))
        }
        _ => {}
    }
    let c = car(&id);
    let base = if dev == "gamepad" {
        AssistProfile::gamepad_default()
    } else {
        AssistProfile::keyboard_default()
    };
    let a = AssistProfile {
        countersteer_help: help,
        ..base
    };
    let mut rig = Rig::new(&c);
    if player == 1 {
        rig.set_player_input(true);
    }
    rig.teleport([0.0; 3], 0.0, kmh / 3.6, 0.0);
    for _ in 0..TPS {
        rig.step(
            &VehicleInput {
                throttle: 0.3,
                ..Default::default()
            },
            &a,
        );
    }
    println!("car {id} dev {dev} cstr {help} player {player} hb {hb_s}s");
    println!("   t  key hb  rack°  FL°  travel° slip° rear°  km/h");
    let mut min_rack = f32::MAX;
    let mut min_fl = f32::MAX;
    let mut worst_t = 0.0;
    let mut peak = f32::MIN;
    let mut drop = 0.0f32;
    let mut drop_t = 0.0;
    let mut reached = false;
    for k in 0..((2.5 + pre_s) * TPS as f32) as usize {
        let t = k as f32 * PHYSICS_DT - pre_s;
        let hb = if (0.0..hb_s).contains(&t) { 1.0 } else { 0.0 };
        let steer = if t < 0.0 {
            pre_key
        } else if t < key_at {
            0.0
        } else if t >= key_end {
            key2
        } else {
            key
        };
        let inp = VehicleInput {
            steer,
            handbrake: hb,
            throttle: if t < 0.0 {
                0.3
            } else if t < hb_s {
                hb_thr
            } else {
                throttle
            },
            ..Default::default()
        };
        rig.step(&inp, &a);
        let s = slide_state(&rig);
        let rack = s.steer.to_degrees();
        let fl = rig.state().wheel_states[0].steer_rad.to_degrees();
        // Wheel angle on the key's side (+ = where the key points them).
        let side = -key.signum();
        // Measured once the rack has reached the key's side (before that it is still
        // unwinding from the previous key at the rack rate), while moving forward.
        if t >= key_at && rack * side > 0.5 {
            reached = true;
        }
        if reached && s.v_fwd > 3.0 {
            if rack * side < min_rack {
                worst_t = t;
            }
            min_rack = min_rack.min(rack * side);
            min_fl = min_fl.min(fl * side);
            peak = peak.max(rack * side);
            if peak - rack * side > drop {
                drop = peak - rack * side;
                drop_t = t;
            }
        }
        if quiet == 0 && k % (TPS / 20) == 0 {
            println!(
                "{t:5.2} {:3.0} {hb:2.0} {rack:6.1} {fl:5.1} {:6.1} {:6.1} {:5.1} {:5.0}",
                inp.steer,
                s.front_axle_angle.to_degrees(),
                s.body_slip.to_degrees(),
                s.rear_axle_slip.to_degrees(),
                rig.state().speed_mps * 3.6
            );
        }
    }
    println!(
        "min rack {min_rack:.1}° (t {worst_t:.2})  min FL {min_fl:.1}° on the key's side (< 0 = countersteer side); drop from peak {drop:.1}° (t {drop_t:.2})"
    );
}
