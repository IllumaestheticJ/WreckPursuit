//! Straight-line braking stability at speed (lead, 2026-10-21: opponent-pace saw hakone_gt yaw
//! against a −0.009 steer under full brake at 60 m/s and spin, on the one-version default).
//!
//! Per car, full brake from `V0` (default 60 m/s ≈ 216 km/h) until stopped or 8 s, on:
//! - the player path (rig marked player-driven: the shipped plain steering; keyboard assists,
//!   TCS off, ABS Full / Half / Off): keys released, and a 0.05 s left-key tap at the brake;
//! - the AI path (unmarked rig, `AssistProfile::ai()`): steer 0 and steer −0.009.
//!
//! Prints peak |yaw rate| (°/s), |heading change| at the stop (°), peak |body slip| (°), spun
//! (|slip| ≥ 45° or backwards) and the stopping distance (m). `cargo run --release -p
//! wp_vehicle --example brake_stability [-- car …]`; env `V0=m/s`, `LEGACY=1` (pre-unify stock
//! path), `GRIP='{…}'` / `RIDE='{…}'` (fields over the shipped balance / ride), `BIAS_ADD=x`
//! (front brake bias + x on every car).

use std::path::PathBuf;

use wp_contracts::{AbsLevel, AssistProfile, LoadedCar, TcsLevel, VehicleInput, PHYSICS_DT};
use wp_vehicle::drift_lab::slide_state;
use wp_vehicle::drift_lab_w::load_any_car;
use wp_vehicle::grip_lab::{self, GripBalance};
use wp_vehicle::ride_lab::{self, RideTune};
use wp_vehicle::rig::Rig;

const TPS: usize = 240;

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    load_any_car(&data, id).unwrap()
}

fn merged<T: serde::Serialize + serde::de::DeserializeOwned>(base: &T, json: &str) -> T {
    let mut v = serde_json::to_value(base).unwrap();
    let patch: serde_json::Value = serde_json::from_str(json).expect("json");
    for (k, x) in patch.as_object().expect("object") {
        v[k] = x.clone();
    }
    serde_json::from_value(v).expect("fields")
}

/// Heading (rad, about +Z, 0 = +Y) of a rotation quaternion `[x, y, z, w]`.
fn heading(q: [f32; 4]) -> f32 {
    let [x, y, z, w] = q;
    // The body's +Y axis in world.
    let fx = 2.0 * (x * y - w * z);
    let fy = 1.0 - 2.0 * (x * x + z * z);
    (-fx).atan2(fy)
}

#[derive(Default)]
struct Stop {
    yaw_dps: f32,
    heading_deg: f32,
    slip_deg: f32,
    spun: bool,
    dist_m: f32,
}

fn stop(
    c: &LoadedCar,
    player: bool,
    a: &AssistProfile,
    steer: impl Fn(f32) -> f32,
    v0: f32,
) -> Stop {
    let mut rig = Rig::new(c);
    if player {
        rig.set_player_input(true);
    }
    rig.teleport([0.0; 3], 0.0, v0, 0.0);
    // Settle 1 s at speed in a high gear (cruise throttle; the automatic picks the gear).
    for _ in 0..TPS {
        let v = rig.state().speed_mps;
        let throttle = (0.4 + 0.3 * (v0 - v)).clamp(0.0, 1.0);
        rig.step(
            &VehicleInput {
                throttle,
                ..Default::default()
            },
            a,
        );
    }
    let h0 = heading(rig.state().rot);
    let p0 = rig.state().pos;
    let mut out = Stop::default();
    for k in 0..8 * TPS {
        let t = k as f32 * PHYSICS_DT;
        rig.step(
            &VehicleInput {
                brake: 1.0,
                steer: steer(t),
                ..Default::default()
            },
            a,
        );
        let s = slide_state(&rig);
        let st = rig.state();
        out.yaw_dps = out.yaw_dps.max(st.ang_vel[2].abs().to_degrees());
        if st.speed_mps > 3.0 {
            out.slip_deg = out.slip_deg.max(s.body_slip.abs().to_degrees());
        }
        if s.body_slip.abs().to_degrees() >= 45.0 && st.speed_mps > 3.0 || s.v_fwd < -0.5 {
            out.spun = true;
        }
        if st.speed_mps < 0.3 {
            break;
        }
    }
    let st = rig.state();
    let mut dh = heading(st.rot) - h0;
    while dh > std::f32::consts::PI {
        dh -= std::f32::consts::TAU;
    }
    while dh < -std::f32::consts::PI {
        dh += std::f32::consts::TAU;
    }
    out.heading_deg = dh.to_degrees().abs();
    out.dist_m = ((st.pos[0] - p0[0]).powi(2) + (st.pos[1] - p0[1]).powi(2)).sqrt();
    out
}

fn cell(s: &Stop) -> String {
    format!(
        "{:.1} / {:.1} / {:.1}{} {:.0} m",
        s.yaw_dps,
        s.heading_deg,
        s.slip_deg,
        if s.spun { " **SPUN**" } else { "" },
        s.dist_m
    )
}

fn main() {
    if std::env::var_os("LEGACY").is_some() {
        wp_vehicle::lab_scope::set_legacy(true);
    }
    if let Ok(j) = std::env::var("GRIP") {
        grip_lab::set_override(Some(merged(&GripBalance::shipped(), &j)));
    }
    if let Ok(j) = std::env::var("RIDE") {
        ride_lab::set_override(Some(merged(&RideTune::owner(), &j)));
    }
    let v0: f32 = std::env::var("V0")
        .ok()
        .map_or(60.0, |v| v.parse().unwrap());
    let ids: Vec<String> = {
        let a: Vec<String> = std::env::args().skip(1).collect();
        if a.is_empty() {
            [
                "hakone_gt",
                "rampart_50",
                "parity_rwd_a",
                "parity_rwd_b",
                "parity_rwd_d",
                "shiro_turbo",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect()
        } else {
            a
        }
    };
    println!("Full brake from {v0} m/s; cells = peak yaw °/s / heading change ° / peak body slip ° , stopping distance");
    println!("| car | player ABS Full | Full + key tap | ABS Half | Half + tap | ABS Off | Off + tap | AI steer 0 | AI steer −0.009 |");
    println!("|---|---|---|---|---|---|---|---|---|");
    let tap = |t: f32| if t < 0.05 { -1.0 } else { 0.0 };
    // BIAS_ADD=x: + x to every car's front brake bias (a brake-balance A/B; data owner's call).
    let bias_add: f32 = std::env::var("BIAS_ADD")
        .ok()
        .map_or(0.0, |v| v.parse().unwrap());
    for id in &ids {
        let mut c = car(id);
        c.def.brakes.bias_front = (c.def.brakes.bias_front + bias_add).clamp(0.0, 1.0);
        let mut cells = Vec::new();
        for abs in [AbsLevel::Full, AbsLevel::Half, AbsLevel::Off] {
            let a = AssistProfile {
                abs,
                tcs: TcsLevel::Off,
                ..AssistProfile::keyboard_default()
            };
            cells.push(cell(&stop(&c, true, &a, |_| 0.0, v0)));
            cells.push(cell(&stop(&c, true, &a, tap, v0)));
        }
        let ai = AssistProfile::ai();
        cells.push(cell(&stop(&c, false, &ai, |_| 0.0, v0)));
        cells.push(cell(&stop(&c, false, &ai, |_| -0.009, v0)));
        println!("| {id} | {} |", cells.join(" | "));
    }
}
