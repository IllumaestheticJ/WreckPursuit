//! High-speed stability sweep (owner playtest 2026-10-20: "way too tail happy at speed and
//! incredibly hard to catch a slide without snap oversteering"). Keyboard preset as the
//! testbed runs it (TCS/ESC off), plus gamepad. For each car:
//! - lane change at 80–170 km/h: full key left 0.5 s, right 0.6 s, centre; peak / final slip;
//! - turn-in: full key held 2 s; yaw rate after 0.3 s and 2 s, road-wheel angle reached;
//! - [`catch_snap_back`] at 110 / 150 km/h with a prompt and a late (0.25 s) key release.
//!
//! `cargo run --release -p wp_vehicle --example hs_stability [car_id ...]`

use wp_contracts::{AssistProfile, LoadedCar, VehicleInput};
use wp_vehicle::math::{atan2, V3};
use wp_vehicle::metrics::catch_snap_back;
use wp_vehicle::rig::Rig;

const TPS: usize = 240;

fn slip_deg(rig: &Rig) -> f32 {
    let ch = rig.vehicle.chassis();
    let v = ch.lin_vel;
    atan2(v.dot(ch.rot * -V3::X), v.dot(ch.rot * V3::Y)).to_degrees()
}

/// Rig rolling straight at `kmh`, settled for 1 s.
fn rolling(car: &LoadedCar, a: &AssistProfile, kmh: f32) -> Rig {
    let mut rig = Rig::new(car);
    rig.teleport([0.0; 3], 0.0, kmh / 3.6, 0.0);
    for _ in 0..TPS {
        step(&mut rig, a, kmh, 0.0);
    }
    rig
}

/// One tick with `steer`, throttle holding `kmh`.
fn step(rig: &mut Rig, a: &AssistProfile, kmh: f32, steer: f32) {
    let e = kmh / 3.6 - rig.state().speed_mps;
    let input = VehicleInput {
        steer,
        throttle: (0.3 + 0.3 * e).clamp(0.0, 1.0),
        ..Default::default()
    };
    rig.step(&input, a);
}

/// Lane change: (peak slip, final slip), degrees.
fn lane_change(car: &LoadedCar, a: &AssistProfile, kmh: f32) -> (f32, f32) {
    let mut rig = rolling(car, a, kmh);
    let mut peak = 0.0f32;
    for i in 0..4 * TPS {
        let t = i as f32 / TPS as f32;
        let steer = match t {
            t if t < 0.5 => -1.0,
            t if t < 1.1 => 1.0,
            _ => 0.0,
        };
        step(&mut rig, a, kmh, steer);
        peak = peak.max(slip_deg(&rig).abs());
    }
    (peak, slip_deg(&rig))
}

/// Turn-in: (yaw rate after 0.3 s, after 2 s, road-wheel angle in degrees).
fn turn_in(car: &LoadedCar, a: &AssistProfile, kmh: f32) -> (f32, f32, f32) {
    let mut rig = rolling(car, a, kmh);
    let mut early = 0.0;
    for i in 0..2 * TPS {
        step(&mut rig, a, kmh, -1.0);
        if i + 1 == 72 {
            early = rig.state().ang_vel[2];
        }
    }
    let steer = rig.vehicle.telemetry().steer_rad.to_degrees();
    (early, rig.state().ang_vel[2], steer)
}

fn main() {
    let mut ids: Vec<String> = std::env::args().skip(1).collect();
    if ids.is_empty() {
        ids = [
            "lindwurm_4t",
            "debug_sedan",
            "kestrel_ts",
            "rampart_50",
            "shiro_turbo",
        ]
        .map(String::from)
        .to_vec();
    }
    let kb = AssistProfile {
        tcs: false,
        esc: false,
        ..AssistProfile::keyboard_default()
    };
    let gp = AssistProfile {
        tcs: false,
        esc: false,
        ..AssistProfile::gamepad_default()
    };
    let cars: Vec<(String, LoadedCar)> = ids
        .iter()
        .map(|id| {
            let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
            (id.clone(), wp_contracts::load_car(p).expect("car loads"))
        })
        .collect();

    println!("lane change (keyboard) | car | km/h | peak slip° | final slip°");
    for (id, car) in &cars {
        for kmh in [80.0, 110.0, 140.0, 170.0] {
            let (peak, end) = lane_change(car, &kb, kmh);
            println!("lane | {id} | {kmh:.0} | {peak:.0} | {end:.0}");
        }
    }
    println!("\nturn-in (keyboard) | car | km/h | yaw rate @0.3 s | @2 s | road-wheel°");
    for (id, car) in &cars {
        for kmh in [50.0, 80.0, 110.0, 140.0] {
            let (early, late, steer) = turn_in(car, &kb, kmh);
            println!("turn-in | {id} | {kmh:.0} | {early:.3} | {late:.3} | {steer:.1}");
        }
    }
    println!("\ncatch | car | preset | km/h | release | slide° | snap-back° | final°");
    for (id, car) in &cars {
        for (name, a) in [("keyboard", kb), ("gamepad", gp)] {
            for kmh in [110.0, 150.0] {
                for (when, late) in [("prompt", 0.0), ("late", 0.25)] {
                    let r = catch_snap_back(car, &a, kmh / 3.6, 1.4, late);
                    println!(
                        "catch | {id} | {name} | {kmh:.0} | {when} | {:.0} | {:.0} | {:.0}",
                        r.first_deg, r.snap_deg, r.final_deg
                    );
                }
            }
        }
    }
}
