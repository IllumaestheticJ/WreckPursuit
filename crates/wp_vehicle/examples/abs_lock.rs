//! Wheel-lock audit of the ABS levels (owner request 2026-10-26: "no wheels should lock up").
//!
//! Every roster car (or the cars given as arguments), on the player path with the keyboard
//! assists (TCS off), at each ABS level, through these stops:
//! - `dry60` / `dry30`: full brake from 60 / 30 m/s, keys released;
//! - `wet`: full brake from 30 m/s on a fully wet road (0.5 mm);
//! - `ice`: full brake from 20 m/s on ice (wet, −5 °C);
//! - `trail`: full brake from 30 m/s with the left key held (dry; trail braking into a turn);
//! - `trail_wet`: the same on a wet road;
//! - `split`: full brake from 25 m/s, left wheels on ice, right wheels dry;
//! - `hb`: full brake + handbrake from 25 m/s (rear lock expected, PR #93); only the fronts count.
//!
//! A wheel counts as locked while it touches the ground, the car moves faster than 3 m/s and its
//! slip ratio is below −0.9. Each cell prints the worst wheel's total locked time (ms), then
//! the stopping distance (m) and, for the trail and split stops, the heading change at the stop
//! (how far the car turned; on split grip it should stay small). `cargo run --release -p wp_vehicle --example abs_lock [-- car …]`.

use std::path::PathBuf;

use wp_contracts::{AbsLevel, AssistProfile, LoadedCar, TcsLevel, VehicleInput, PHYSICS_DT};
use wp_vehicle::drift_lab_w::load_any_car;
use wp_vehicle::rig::Rig;
use wp_vehicle::surface::SurfaceConditions;
use wp_vehicle::AbsHardware;

const TPS: usize = 240;
/// Slip ratio below which a rolling wheel counts as locked.
const LOCKED: f32 = -0.9;

fn data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

#[derive(Clone, Copy)]
struct Scene {
    name: &'static str,
    v0: f32,
    steer: f32,
    handbrake: f32,
    left: SurfaceConditions,
    right: SurfaceConditions,
}

const WET: SurfaceConditions = SurfaceConditions {
    water_depth_mm: 0.5,
    temperature_c: 15.0,
    painted_line: false,
};
const ICE: SurfaceConditions = SurfaceConditions {
    water_depth_mm: 0.5,
    temperature_c: -5.0,
    painted_line: false,
};
const DRY: SurfaceConditions = SurfaceConditions::DRY;

fn scenes() -> Vec<Scene> {
    let s = |name, v0, steer, handbrake, left, right| Scene {
        name,
        v0,
        steer,
        handbrake,
        left,
        right,
    };
    vec![
        s("dry60", 60.0, 0.0, 0.0, DRY, DRY),
        s("dry30", 30.0, 0.0, 0.0, DRY, DRY),
        s("wet", 30.0, 0.0, 0.0, WET, WET),
        s("ice", 20.0, 0.0, 0.0, ICE, ICE),
        s("trail", 30.0, 1.0, 0.0, DRY, DRY),
        s("trail_wet", 30.0, 1.0, 0.0, WET, WET),
        s("split", 25.0, 0.0, 0.0, ICE, DRY),
        s("hb", 25.0, 0.0, 1.0, DRY, DRY),
    ]
}

/// One stop's result.
#[derive(Default)]
struct Stop {
    /// Locked time per wheel, s.
    locked_s: [f32; 4],
    dist_m: f32,
    /// Heading change from the brake to the stop, ° (how far the car turned).
    turn_deg: f32,
}

/// Heading (rad, about +Z) of a rotation quaternion `[x, y, z, w]`.
fn heading(q: [f32; 4]) -> f32 {
    let [x, y, z, w] = q;
    let fx = 2.0 * (x * y - w * z);
    let fy = 1.0 - 2.0 * (x * x + z * z);
    (-fx).atan2(fy)
}

fn stop(c: &LoadedCar, hw: AbsHardware, abs: AbsLevel, sc: &Scene) -> Stop {
    let a = AssistProfile {
        abs,
        tcs: TcsLevel::Off,
        ..AssistProfile::keyboard_default()
    };
    let mut rig = Rig::new(c);
    rig.set_player_input(true);
    rig.vehicle.set_abs_hardware(hw);
    rig.vehicle
        .set_surface_conditions([sc.left, sc.right, sc.left, sc.right]);
    rig.teleport([0.0; 3], 0.0, sc.v0, 0.0);
    for _ in 0..TPS {
        let v = rig.state().speed_mps;
        let throttle = (0.4 + 0.3 * (sc.v0 - v)).clamp(0.0, 1.0);
        rig.step(
            &VehicleInput {
                throttle,
                ..Default::default()
            },
            &a,
        );
    }
    let p0 = rig.state().pos;
    let h0 = heading(rig.state().rot);
    let mut out = Stop::default();
    for _ in 0..12 * TPS {
        rig.step(
            &VehicleInput {
                brake: 1.0,
                steer: sc.steer,
                handbrake: sc.handbrake,
                ..Default::default()
            },
            &a,
        );
        let st = rig.state();
        if std::env::var_os("TRACE").is_some() {
            let t = rig.vehicle.telemetry();
            let ws = &st.wheel_states;
            println!(
                "p {:.2?} v {:5.1} gear {} rpm {:5.0} clutch {:.2} k [{:+.2} {:+.2} {:+.2} {:+.2}] fz [{:5.0} {:5.0} {:5.0} {:5.0}] a [{:+.2} {:+.2} {:+.2} {:+.2}]",
                rig.vehicle.abs_pressure(), st.speed_mps, st.gear, st.engine_rpm, t.clutch,
                ws[0].slip_ratio, ws[1].slip_ratio, ws[2].slip_ratio, ws[3].slip_ratio,
                ws[0].fz_n, ws[1].fz_n, ws[2].fz_n, ws[3].fz_n,
                ws[0].slip_angle, ws[1].slip_angle, ws[2].slip_angle, ws[3].slip_angle,
            );
        }
        let moving = (st.lin_vel[0].powi(2) + st.lin_vel[1].powi(2)).sqrt() > 3.0;
        for (i, w) in st.wheel_states.iter().enumerate() {
            if moving && w.contact && w.slip_ratio < LOCKED {
                out.locked_s[i] += PHYSICS_DT;
            }
        }
        if st.speed_mps.abs() < 0.3 {
            break;
        }
    }
    let st = rig.state();
    out.dist_m = ((st.pos[0] - p0[0]).powi(2) + (st.pos[1] - p0[1]).powi(2)).sqrt();
    let mut dh = heading(st.rot) - h0;
    while dh > std::f32::consts::PI {
        dh -= std::f32::consts::TAU;
    }
    while dh < -std::f32::consts::PI {
        dh += std::f32::consts::TAU;
    }
    out.turn_deg = dh.to_degrees();
    out
}

fn roster() -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(data().join("cars"))
        .unwrap()
        .filter_map(|e| {
            let p = e.ok()?.path();
            let name = p.file_name()?.to_str()?.to_string();
            name.strip_suffix(".json").map(str::to_string)
        })
        .collect();
    ids.sort();
    ids
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Ok(scene) = std::env::var("TRACE") {
        // TRACE=<scene> LEVEL=<Off|Half|Full|Motorsport> … car: print every tick of one stop.
        let level = std::env::var("LEVEL").unwrap_or_else(|_| "Motorsport".into());
        let (hw, abs) = match level.as_str() {
            "Off" => (AbsHardware::Stock, AbsLevel::Off),
            "Half" => (AbsHardware::Stock, AbsLevel::Half),
            "Full" => (AbsHardware::Stock, AbsLevel::Full),
            _ => (AbsHardware::Motorsport, AbsLevel::Full),
        };
        let sc = scenes()
            .into_iter()
            .find(|s| s.name == scene)
            .expect("scene");
        let c = load_any_car(&data(), &args[0]).unwrap();
        let s = stop(&c, hw, abs, &sc);
        println!(
            "locked ms {:?} dist {:.1} turn {:+.0}°",
            s.locked_s.map(|x| (x * 1000.0) as i32),
            s.dist_m,
            s.turn_deg
        );
        return;
    }
    let ids = if args.is_empty() { roster() } else { args };
    let levels: [(&str, AbsHardware, AbsLevel); 4] = [
        ("Off", AbsHardware::Stock, AbsLevel::Off),
        ("Half", AbsHardware::Stock, AbsLevel::Half),
        ("Full", AbsHardware::Stock, AbsLevel::Full),
        ("Motorsport", AbsHardware::Motorsport, AbsLevel::Full),
    ];
    let scenes = scenes();
    println!("Cells: worst wheel's locked time ms (fronts only for `hb`) / stopping distance m (/ heading change at the stop for trail and split)");
    let mut worst = [0.0f32; 4];
    for (li, (lname, hw, level)) in levels.iter().enumerate() {
        println!("\n### ABS {lname}\n");
        let head: Vec<&str> = scenes.iter().map(|s| s.name).collect();
        println!("| car | {} |", head.join(" | "));
        println!("|---|{}", "---|".repeat(scenes.len()));
        for id in &ids {
            let c = load_any_car(&data(), id).unwrap();
            let cells: Vec<String> = scenes
                .iter()
                .map(|sc| {
                    let s = stop(&c, *hw, *level, sc);
                    let n = if sc.handbrake > 0.0 { 2 } else { 4 };
                    let lock = s.locked_s[..n].iter().cloned().fold(0.0, f32::max) * 1000.0;
                    worst[li] = worst[li].max(lock);
                    if sc.name.starts_with("trail") || sc.name == "split" {
                        format!("{lock:.0} / {:.1} / {:+.0}°", s.dist_m, s.turn_deg)
                    } else {
                        format!("{lock:.0} / {:.1}", s.dist_m)
                    }
                })
                .collect();
            println!("| {id} | {} |", cells.join(" | "));
        }
    }
    println!();
    for (li, (lname, _, _)) in levels.iter().enumerate() {
        println!("worst lock, ABS {lname}: {:.0} ms", worst[li]);
    }
}
