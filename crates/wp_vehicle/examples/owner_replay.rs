//! Owner-replay bench (rows 142 / 143): the owner's reference-game keyboard clips
//! (`docs/reference/wf-clips/*_track.json` / `*_keys.json`, 30 fps key states L / R / U / D /
//! Space) fed as keyboard input into the matching parity car on a flat pad, from rest, with
//! automatic gears (the clips' shifts are not readable).
//!
//! Per clip, assists (TCS Off / Half) and steering variant, prints:
//! - spins (|body slip| ≥ 90° above 15 km/h, counted per event);
//! - drift time: s with 10–50° of body slip above 30 km/h;
//! - held key: ticks where the wheels point against the held key (held ≥ 0.4 s and > 1° on
//!   the other side) or past its lock;
//! - hands-off: the wheel angle (% of full lock, + = countersteer) ≥ 0.5 s after the last steer
//!   key, throttle on, above 30 km/h, |body slip| ≥ 8°: p50 / p90 / max, and the time it takes
//!   to get within 2° of where it settles 0.5 s after a release in a slide (median).
//!
//! Reference (OWNER_KEY_TECHNIQUE.md): settled hands-off marker angle Rocket p50 16°, p90 24°,
//! max 34°; Hammerhead p50 28°, p90 45°, max 47°; roughly a quarter to a third of full lock.
//!
//! `cargo run --release -p wp_vehicle --example owner_replay`

use std::path::PathBuf;

use wp_contracts::{AbsLevel, AssistProfile, LoadedCar, TcsLevel, VehicleInput, PHYSICS_DT};
use wp_vehicle::drift_lab::slide_state;
use wp_vehicle::drift_lab_w::load_any_car;
use wp_vehicle::grip_lab::{self, GripBalance};
use wp_vehicle::plain::{self, PlainSteer};
use wp_vehicle::rig::Rig;

const TPS: usize = 240;

/// (clip file, parity car, the clip's TCS).
const CLIPS: [(&str, &str, &str); 4] = [
    (
        "2026-10-21_1630_rocket_rx_track.json",
        "parity_rwd_e",
        "Off",
    ),
    (
        "2026-10-21_1634_hammerhead_rs_track.json",
        "parity_rwd_f",
        "Off",
    ),
    (
        "2026-10-21_1637_hammerhead_rs_b_keys.json",
        "parity_rwd_f",
        "Off",
    ),
    (
        "2026-10-21_1639_hammerhead_rs_tcs_half_keys.json",
        "parity_rwd_f",
        "Half",
    ),
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Key frames `(t, steer, throttle, brake, handbrake)`.
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
struct Result {
    spins: usize,
    drift_s: f32,
    held_bad: usize,
    hands_off: Vec<f32>,
    settle: Vec<f32>,
}

fn pct(v: &mut [f32], q: f32) -> f32 {
    if v.is_empty() {
        return f32::NAN;
    }
    v.sort_by(f32::total_cmp);
    v[((v.len() - 1) as f32 * q).round() as usize]
}

fn run(c: &LoadedCar, frames: &[(f32, f32, f32, f32, f32)], a: &AssistProfile) -> Result {
    let full = c
        .def
        .wheels
        .iter()
        .filter(|w| w.steerable)
        .map(|w| w.max_steer_rad)
        .fold(0.0f32, f32::max);
    let p = plain::active().unwrap_or_else(wp_vehicle::lab_scope::shipped_plain);
    let mut rig = Rig::new(c);
    rig.set_player_input(true);
    let mut r = Result::default();
    let end = frames.last().unwrap().0;
    let (mut fi, mut held_s, mut prev_key, mut since_key) = (0usize, 0.0f32, 0.0f32, 0.0f32);
    let mut spinning = false;
    // A release in a slide: (tick it happened, the angle 0.5 s later once known).
    let mut releases: Vec<(usize, Vec<f32>)> = Vec::new();
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
        let s = slide_state(&rig);
        let v = rig.state().speed_mps * 3.6;
        let slip = s.body_slip.to_degrees();
        let wheel = rig.vehicle.telemetry().steer_rad;
        // Spins.
        let spin_now = slip.abs() >= 90.0 && v > 15.0;
        if spin_now && !spinning {
            r.spins += 1;
        }
        spinning = spin_now || (spinning && slip.abs() > 45.0);
        if v > 30.0 && (10.0..=50.0).contains(&slip.abs()) {
            r.drift_s += PHYSICS_DT;
        }
        // Held key.
        if steer != 0.0 {
            held_s = if steer == prev_key {
                held_s + PHYSICS_DT
            } else {
                PHYSICS_DT
            };
            since_key = 0.0;
            // The rack's lock uses the forward speed (not the total, larger in a slide).
            let lock = full * p.lock_frac(s.v_fwd);
            let against = (wheel * steer).max(0.0);
            if (held_s >= 0.4 && against > 1f32.to_radians())
                || wheel.abs() > lock + 0.2f32.to_radians()
            {
                if r.held_bad == 0 && std::env::var_os("DEBUG").is_some() {
                    eprintln!(
                        "first held-key miss t {t:.2} key {steer} held {held_s:.2} wheel {:.2}° lock {:.2}° slip {slip:.1} v {v:.0} pull? hb {hb}",
                        wheel.to_degrees(),
                        lock.to_degrees()
                    );
                }
                r.held_bad += 1;
            }
        } else {
            if prev_key != 0.0 && slip.abs() >= 8.0 && v > 30.0 {
                releases.push((k, Vec::new()));
            }
            since_key += PHYSICS_DT;
        }
        prev_key = steer;
        // Countersteer = the wheels toward the slide's side (+ wheel = left; body slip + =
        // moving left of the heading → countersteer is + wheel).
        let counter = (wheel * slip.signum()).to_degrees() / full.to_degrees() * 100.0;
        if steer == 0.0 && since_key >= 0.5 && thr > 0.5 && v > 30.0 && slip.abs() >= 8.0 {
            r.hands_off.push(counter);
        }
        for (k0, trace) in releases.iter_mut() {
            if k - *k0 <= TPS / 2 && trace.len() < TPS / 2 + 1 {
                trace.push(if steer == 0.0 {
                    wheel.to_degrees()
                } else {
                    f32::NAN
                });
            }
        }
    }
    for (_, trace) in releases {
        if trace.len() < TPS / 2 + 1 || trace.iter().any(|x| x.is_nan()) {
            continue;
        }
        let fin = trace[trace.len() - 1];
        let i = trace
            .iter()
            .position(|x| (x - fin).abs() <= 2.0)
            .unwrap_or(trace.len());
        r.settle.push(i as f32 * PHYSICS_DT);
    }
    r
}

/// A steering variant: name, the lab overrides (`None` = shipped), countersteer help.
type Variant = (String, Option<(PlainSteer, GripBalance)>, f32);

fn main() {
    // The variants: 0.12.44 (row 137: Drift Lab W's set with the catch at 0.8, the owner's grip
    // balance with the smart lock and the held-key cut, countersteer help 0) and row 142 with
    // countersteer help 0 / 30 / 60 / 100 %.
    let mut variants: Vec<Variant> = vec![(
        "0.12.44".into(),
        Some((PlainSteer::lab_w(), GripBalance::owner())),
        0.0,
    )];
    let helps: Vec<f32> = std::env::var("HELPS")
        .ok()
        .map(|s| s.split(',').map(|x| x.parse().unwrap()).collect())
        .unwrap_or(vec![0.0, 0.3, 0.6, 1.0]);
    for h in helps {
        variants.push((format!("row 142, help {:.0} %", h * 100.0), None, h));
    }
    println!("| clip | car | TCS | steering | spins | drift s | held key against / past | hands-off % lock p50 / p90 / max | settle s (median) |");
    println!("|---|---|---|---|---|---|---|---|---|");
    for (file, id, clip_tcs) in CLIPS {
        let c = load_any_car(&root().join("data"), id).unwrap();
        let frames = keys(file);
        for tcs in [TcsLevel::Off, TcsLevel::Half] {
            for (name, ov, help) in &variants {
                match ov {
                    Some((p, g)) => {
                        plain::set_override(Some(*p));
                        grip_lab::set_override(Some(*g));
                    }
                    None => {
                        plain::set_override(None);
                        grip_lab::set_override(None);
                    }
                }
                let a = AssistProfile {
                    abs: AbsLevel::Full,
                    tcs,
                    esc: false,
                    auto_gears: true,
                    auto_clutch: true,
                    throttle_filter: 0.0,
                    brake_filter: 0.0,
                    countersteer_help: *help,
                    ..AssistProfile::keyboard_default()
                };
                let mut r = run(&c, &frames, &a);
                let mut h = r.hands_off.clone();
                println!(
                    "| {} | {id} | {:?}{} | {name} | {} | {:.1} | {} | {:.0} / {:.0} / {:.0} (n {}) | {:.2} (n {}) |",
                    &file[11..15],
                    tcs,
                    if format!("{tcs:?}") == clip_tcs { " (clip)" } else { "" },
                    r.spins,
                    r.drift_s,
                    r.held_bad,
                    pct(&mut h, 0.5),
                    pct(&mut h, 0.9),
                    pct(&mut h, 1.0),
                    h.len(),
                    pct(&mut r.settle, 0.5),
                    r.settle.len(),
                );
            }
        }
    }
    plain::set_override(None);
    grip_lab::set_override(None);
}
