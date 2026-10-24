//! Front-drive steering audit (owner, 2026-10-24: "the car does weird countersteering still
//! on FF cars"). Replays the owner's reference keyboard clips (`docs/reference/wf-clips`) into
//! front-drive cars with the shipped handling and the keyboard assists, as a player car, and
//! splits what moves the front wheels besides the keys:
//! - **hands-off countersteer**: the rack (`Telemetry::steer_rad`) away from centre with no
//!   key held (the countersteer help, `PlainSteer::self_align`, capped 30 %);
//! - **torque steer**: the front wheels' mean angle minus the rack (drive-torque imbalance,
//!   `steering::torque_steer`);
//! - **against the key**: ticks where a key has been held ≥ 0.3 s and the wheels point the
//!   other way, or a released key's wheels still point the old way after 0.5 s.
//!
//! `cargo run --release -p wp_vehicle --example fwd_counter` (`CARS=a,b`, `TRACE=car` prints
//! the worst events).

use std::path::PathBuf;

use wp_contracts::{AssistProfile, VehicleInput, PHYSICS_DT};
use wp_vehicle::drift_lab::slide_state;
use wp_vehicle::drift_lab_w::load_any_car;
use wp_vehicle::rig::Rig;

const TPS: usize = 240;
const CLIPS: [&str; 4] = [
    "2026-10-21_1630_parity_rwd_e_track.json",
    "2026-10-21_1634_parity_rwd_f_track.json",
    "2026-10-21_1637_parity_rwd_f_b_keys.json",
    "2026-10-21_1639_parity_rwd_f_tcs_half_keys.json",
];
const CARS: [&str; 6] = [
    "parity_fwd_c",
    "kestrel_ts",
    "vireo_xs",
    "tansy_13",
    "stoat_16v",
    "debug_sedan",
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Key frames `(t, steer + = right, throttle, brake, handbrake)` from a clip's key timeline.
fn keys(file: &str) -> Vec<(f32, f32, f32, f32, f32)> {
    let p = root().join("docs/reference/wf-clips").join(file);
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    v.as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let t = row[0].as_f64().unwrap() as f32;
            let k = |n: &str| row[1][n].as_f64().unwrap_or(0.0) as f32;
            (t, k("R") - k("L"), k("U"), k("D"), k("SP"))
        })
        .collect()
}

#[derive(Default)]
struct Audit {
    /// Ticks above 20 km/h.
    ticks: usize,
    /// Hands-off ticks (no key ≥ 0.5 s) and those with the rack > 2° off centre.
    hands_off: usize,
    self_align_2deg: usize,
    self_align_max_deg: f32,
    /// |torque steer| samples, degrees.
    ts: Vec<f32>,
    /// Held key ≥ 0.3 s with the wheels > 1° the other way (rack / with torque steer).
    against_rack: usize,
    against_wheels: usize,
    /// |body slip| samples above 30 km/h, degrees.
    slip: Vec<f32>,
    /// Held key ≥ 0.4 s, > 30 km/h: ticks the car yaws against the key by > 15°/s.
    yaw_against: usize,
    /// Held-key ticks counted for [`Audit::yaw_against`].
    held_ticks: usize,
    /// No key ≥ 0.3 s, > 30 km/h: yaw-rate reversals (|r| > 15°/s on both sides).
    hands_off_reversals: usize,
    /// Worst events: (|deg|, text).
    worst: Vec<(f32, String)>,
}

fn pct(v: &mut [f32], q: f32) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    v[((v.len() - 1) as f32 * q).round() as usize]
}

fn run(id: &str, frames: &[(f32, f32, f32, f32, f32)], a: &AssistProfile, au: &mut Audit) {
    let c = load_any_car(&root().join("data"), id).unwrap();
    let mut rig = Rig::new(&c);
    rig.set_player_input(true);
    let end = frames.last().unwrap().0;
    let (mut fi, mut held_s, mut prev_key, mut since_key) = (0usize, 0.0f32, 0.0f32, 9.0f32);
    let mut last_big_yaw = 0.0f32;
    for k in 0..(end * TPS as f32) as usize {
        let t = k as f32 * PHYSICS_DT;
        while fi + 1 < frames.len() && frames[fi + 1].0 <= t {
            fi += 1;
        }
        let (_, steer, thr, brake, hb) = frames[fi];
        rig.step(
            &VehicleInput {
                steer,
                throttle: thr,
                brake,
                handbrake: hb,
                ..Default::default()
            },
            a,
        );
        if steer != 0.0 {
            held_s = if steer == prev_key {
                held_s + PHYSICS_DT
            } else {
                PHYSICS_DT
            };
            since_key = 0.0;
        } else {
            since_key += PHYSICS_DT;
        }
        prev_key = steer;
        let st = rig.state();
        let kmh = st.speed_mps * 3.6;
        if kmh < 20.0 {
            continue;
        }
        au.ticks += 1;
        let s = slide_state(&rig);
        let slip = s.body_slip.to_degrees();
        if kmh > 30.0 {
            au.slip.push(slip.abs());
        }
        // + = left for both.
        let rack = rig.vehicle.telemetry().steer_rad;
        let wheels = 0.5 * (st.wheel_states[0].steer_rad + st.wheel_states[1].steer_rad);
        let ts = (wheels - rack).to_degrees();
        au.ts.push(ts.abs());
        if ts.abs() > 1.0 {
            au.worst.push((
                ts.abs(),
                format!("torque steer {ts:+.2}° t {t:.2} key {steer} thr {thr} v {kmh:.0} slip {slip:+.1}"),
            ));
        }
        if steer == 0.0 && since_key >= 0.5 {
            au.hands_off += 1;
            let r = rack.to_degrees();
            if r.abs() > 2.0 {
                au.self_align_2deg += 1;
                au.worst.push((
                    r.abs(),
                    format!(
                        "hands-off rack {r:+.2}° t {t:.2} thr {thr} v {kmh:.0} slip {slip:+.1}"
                    ),
                ));
            }
            au.self_align_max_deg = au.self_align_max_deg.max(r.abs());
        }
        // Yaw against the held key (+ yaw = left; key + = right: against = same sign).
        let yaw = s.yaw_rate.to_degrees();
        if kmh > 30.0 && steer != 0.0 && held_s >= 0.4 {
            au.held_ticks += 1;
            if yaw * steer > 15.0 {
                au.yaw_against += 1;
            }
        }
        if kmh > 30.0 && steer == 0.0 && since_key >= 0.3 {
            if yaw.abs() > 15.0 {
                if last_big_yaw != 0.0 && yaw.signum() != last_big_yaw.signum() {
                    au.hands_off_reversals += 1;
                }
                last_big_yaw = yaw;
            }
        } else {
            last_big_yaw = 0.0;
        }
        // Key + = right; wheel + = left: against = wheel and key the same sign.
        if steer != 0.0 && held_s >= 0.3 {
            if (rack * steer).to_degrees() > 1.0 {
                au.against_rack += 1;
            }
            if (wheels * steer).to_degrees() > 1.0 {
                au.against_wheels += 1;
                au.worst.push((
                    (wheels * steer).to_degrees(),
                    format!(
                        "AGAINST held key {steer} wheels {:+.2}° rack {:+.2}° t {t:.2} v {kmh:.0} slip {slip:+.1}",
                        wheels.to_degrees(),
                        rack.to_degrees()
                    ),
                ));
            }
        }
    }
}

fn main() {
    let cars: Vec<String> = std::env::var("CARS")
        .map(|v| v.split(',').map(str::to_string).collect())
        .unwrap_or_else(|_| CARS.iter().map(|s| s.to_string()).collect());
    let trace = std::env::var("TRACE").ok();
    // PRE_G=1: the handling shipped before candidate G (tyres, grip balance, ride).
    if std::env::var_os("PRE_G").is_some() {
        wp_vehicle::car_lab::set_override(Some(wp_vehicle::car_lab::CarTune::pre_g()));
        wp_vehicle::grip_lab::set_override(Some(wp_vehicle::grip_lab::GripBalance::pre_g()));
        wp_vehicle::ride_lab::set_override(Some(wp_vehicle::ride_lab::RideTune::owner()));
    }
    // The game menu's defaults (ui-kit settings): TCS Off, countersteer 30 %. TCS=half|full.
    let mut a = AssistProfile::keyboard_default();
    a.tcs = match std::env::var("TCS").as_deref() {
        Ok("full") => wp_contracts::TcsLevel::Full,
        Ok("half") => wp_contracts::TcsLevel::Half,
        _ => wp_contracts::TcsLevel::Off,
    };
    println!(
        "assists: keyboard default (tcs {:?}, countersteer help {})",
        a.tcs, a.countersteer_help
    );
    for id in &cars {
        let mut au = Audit::default();
        for clip in CLIPS {
            run(id, &keys(clip), &a, &mut au);
        }
        let n = au.ticks.max(1) as f32;
        println!(
            "{id:14} yaw against held key {:.1}% | hands-off yaw reversals {} | hands-off rack>2°: {:5.1}% of hands-off (max {:.1}°) | torque steer p50 {:.2}° p99 {:.2}° max {:.2}° | held key, wheels against: rack {} wheels {} ticks ({:.2}%) | slip>30km/h p90 {:.1}° max {:.1}°",
            100.0 * au.yaw_against as f32 / au.held_ticks.max(1) as f32,
            au.hands_off_reversals,
            100.0 * au.self_align_2deg as f32 / au.hands_off.max(1) as f32,
            au.self_align_max_deg,
            pct(&mut au.ts, 0.5),
            pct(&mut au.ts, 0.99),
            pct(&mut au.ts, 1.0),
            au.against_rack,
            au.against_wheels,
            100.0 * au.against_wheels as f32 / n,
            pct(&mut au.slip, 0.9),
            pct(&mut au.slip, 1.0),
        );
        if trace.as_deref() == Some(id.as_str()) {
            au.worst.sort_by(|x, y| y.0.total_cmp(&x.0));
            for (_, w) in au.worst.iter().take(25) {
                println!("    {w}");
            }
        }
    }
}
