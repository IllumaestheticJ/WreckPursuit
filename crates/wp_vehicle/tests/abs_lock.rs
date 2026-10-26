//! ABS never locks a wheel (owner request 2026-10-26: "no wheels should lock up"), on every
//! roster car: stock ABS Full and the motorsport ABS upgrade through straight stops (dry, wet,
//! ice), a trail-braked stop and a split-grip stop. The full table is
//! `cargo run --release -p wp_vehicle --example abs_lock`.

use std::path::PathBuf;

use wp_contracts::{AbsLevel, AssistProfile, LoadedCar, TcsLevel, VehicleInput, PHYSICS_DT};
use wp_vehicle::drift_lab_w::load_any_car;
use wp_vehicle::rig::Rig;
use wp_vehicle::{AbsHardware, SurfaceConditions};

/// Slip ratio below which a rolling wheel counts as locked.
const LOCKED: f32 = -0.9;

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

fn data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn roster() -> Vec<LoadedCar> {
    let mut ids: Vec<String> = std::fs::read_dir(data().join("cars"))
        .unwrap()
        .filter_map(|e| {
            let p = e.ok()?.path();
            let name = p.file_name()?.to_str()?.to_string();
            name.strip_suffix(".json").map(str::to_string)
        })
        .collect();
    ids.sort();
    assert!(ids.len() > 30, "roster {ids:?}");
    ids.iter()
        .map(|id| load_any_car(&data(), id).unwrap())
        .collect()
}

/// One stop: settle 1 s at `v0`, then full brake (+ `steer`, `handbrake`) until stopped.
/// Returns (locked time per wheel s, stopping distance m).
#[allow(clippy::too_many_arguments)]
fn stop(
    c: &LoadedCar,
    hw: AbsHardware,
    abs: AbsLevel,
    v0: f32,
    steer: f32,
    handbrake: f32,
    left: SurfaceConditions,
    right: SurfaceConditions,
) -> ([f32; 4], f32) {
    let a = AssistProfile {
        abs,
        tcs: TcsLevel::Off,
        ..AssistProfile::keyboard_default()
    };
    let mut rig = Rig::new(c);
    rig.set_player_input(true);
    rig.vehicle.set_abs_hardware(hw);
    rig.vehicle
        .set_surface_conditions([left, right, left, right]);
    rig.teleport([0.0; 3], 0.0, v0, 0.0);
    for _ in 0..240 {
        let v = rig.state().speed_mps;
        let throttle = (0.4 + 0.3 * (v0 - v)).clamp(0.0, 1.0);
        rig.step(
            &VehicleInput {
                throttle,
                ..Default::default()
            },
            &a,
        );
    }
    let p0 = rig.state().pos;
    let mut locked = [0.0f32; 4];
    for _ in 0..12 * 240 {
        rig.step(
            &VehicleInput {
                brake: 1.0,
                steer,
                handbrake,
                ..Default::default()
            },
            &a,
        );
        let st = rig.state();
        let moving = (st.lin_vel[0].powi(2) + st.lin_vel[1].powi(2)).sqrt() > 3.0;
        for (i, w) in st.wheel_states.iter().enumerate() {
            if moving && w.contact && w.slip_ratio < LOCKED {
                locked[i] += PHYSICS_DT;
            }
        }
        if st.speed_mps.abs() < 0.3 {
            break;
        }
    }
    let st = rig.state();
    let d = ((st.pos[0] - p0[0]).powi(2) + (st.pos[1] - p0[1]).powi(2)).sqrt();
    (locked, d)
}

fn worst(locked: &[f32]) -> f32 {
    locked.iter().cloned().fold(0.0, f32::max)
}

#[test]
fn abs_full_and_motorsport_never_lock_a_wheel_on_any_car() {
    // (name, v0, steer, left, right)
    let scenes = [
        ("dry", 30.0, 0.0, DRY, DRY),
        ("wet", 30.0, 0.0, WET, WET),
        ("ice", 20.0, 0.0, ICE, ICE),
        ("trail", 30.0, 1.0, DRY, DRY),
        ("trail_wet", 30.0, 1.0, WET, WET),
        ("split", 25.0, 0.0, ICE, DRY),
    ];
    let mut fails = Vec::new();
    for c in roster() {
        for (name, v0, steer, l, r) in scenes {
            let (full, d_full) = stop(&c, AbsHardware::Stock, AbsLevel::Full, v0, steer, 0.0, l, r);
            let (ms, d_ms) = stop(
                &c,
                AbsHardware::Motorsport,
                AbsLevel::Full,
                v0,
                steer,
                0.0,
                l,
                r,
            );
            // Stock Full: a wheel may brush the lock for a tick or two at a release, never more
            // than 20 ms over a stop (it was up to the whole 12 s stop on ice).
            if worst(&full) > 0.02 {
                fails.push(format!("{} {name}: ABS Full locked {:?} s", c.def.id, full));
            }
            if worst(&ms) > 0.0 {
                fails.push(format!(
                    "{} {name}: motorsport ABS locked {:?} s",
                    c.def.id, ms
                ));
            }
            // Straight-line stops: motorsport ABS never stops longer than stock Full.
            if steer == 0.0 && l.water_depth_mm == r.water_depth_mm && d_ms > d_full * 1.01 {
                fails.push(format!(
                    "{} {name}: motorsport {d_ms:.1} m vs Full {d_full:.1} m",
                    c.def.id
                ));
            }
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

#[test]
fn motorsport_abs_leaves_the_handbrake_locking_the_rears() {
    // PR #93: the handbrake locks the rear wheels; the ABS keeps the fronts turning.
    let c = load_any_car(&data(), "hakone_gt").unwrap();
    let (locked, _) = stop(
        &c,
        AbsHardware::Motorsport,
        AbsLevel::Full,
        25.0,
        0.0,
        1.0,
        DRY,
        DRY,
    );
    assert_eq!(worst(&locked[..2]), 0.0, "fronts {locked:?}");
    assert!(worst(&locked[2..]) > 0.5, "rears {locked:?}");
}

#[test]
fn motorsport_abs_only_replaces_full() {
    // Half and Off are the driver's choice: the module changes nothing there.
    let c = load_any_car(&data(), "kestrel_ts").unwrap();
    for level in [AbsLevel::Off, AbsLevel::Half] {
        let stock = stop(&c, AbsHardware::Stock, level, 30.0, 0.0, 0.0, WET, WET);
        let ms = stop(&c, AbsHardware::Motorsport, level, 30.0, 0.0, 0.0, WET, WET);
        assert_eq!(stock, ms, "{level:?}");
    }
}
