//! Drift Lab W: plain keyboard steering (`wp_vehicle::plain`) on a real car, and the
//! reference-parity test cars. The plain profile is a process-wide override, so every test in
//! this binary turns it on (and nothing here needs it off).

use std::path::PathBuf;

use wp_contracts::{LoadedCar, VehicleInput};
use wp_vehicle::drift_lab_w::{load_any_car, reference_assists, PARITY_CARS};
use wp_vehicle::plain::{self, PlainSteer};
use wp_vehicle::rig::Rig;

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    load_any_car(&data, id).unwrap()
}

fn plain_on() {
    plain::set_override(Some(PlainSteer::default()));
}

/// Centre road-wheel angle after `ticks` of input `steer` from rest-of-steering at `speed`.
fn steer_after(c: &LoadedCar, speed: f32, steer: f32, ticks: usize) -> (Rig, f32) {
    let mut rig = Rig::new(c);
    rig.teleport([0.0; 3], 0.0, speed, 0.0);
    let a = reference_assists();
    for _ in 0..ticks {
        rig.step(
            &VehicleInput {
                steer,
                ..Default::default()
            },
            &a,
        );
    }
    let s = rig.vehicle.telemetry().steer_rad;
    (rig, s)
}

#[test]
fn parity_cars_match_the_reference_specs() {
    // (id, kW, N·m, kg, gears, rwd)
    for (id, kw, nm, kg, gears, rwd) in [
        ("parity_rwd_a", 345.0f32, 793.0f32, 1369.0f32, 5usize, true),
        ("parity_rwd_b", 254.0, 405.0, 1047.0, 6, true),
        ("parity_rwd_d", 390.0, 807.0, 1633.0, 4, true),
        ("parity_fwd_c", 121.0, 179.0, 982.0, 5, false),
    ] {
        let c = car(id);
        let d = &c.def;
        assert_eq!(d.mass_kg, kg, "{id}");
        assert_eq!(d.drivetrain.gear_ratios.len(), gears, "{id}");
        let peak_nm = d
            .engine
            .torque_curve_nm
            .iter()
            .map(|p| p.1)
            .fold(0.0f32, f32::max);
        let peak_kw = d
            .engine
            .torque_curve_nm
            .iter()
            .map(|p| p.0 * p.1 * std::f32::consts::TAU / 60.0 / 1000.0)
            .fold(0.0f32, f32::max);
        assert!((peak_nm - nm).abs() < 1.0, "{id}: {peak_nm} N·m");
        assert!((peak_kw - kw).abs() < 2.0, "{id}: {peak_kw} kW");
        let rear = d.wheels[2].position_m[1] < 0.0;
        assert!(rear, "{id}: wheel 2 is a rear wheel");
        assert_eq!(
            format!("{:?}", d.drivetrain.layout) == "Rwd",
            rwd,
            "{id}: {:?}",
            d.drivetrain.layout
        );
    }
    assert_eq!(PARITY_CARS.len(), 4);
}

#[test]
fn plain_rack_reaches_full_lock_in_t_full_and_returns_in_t_return() {
    plain_on();
    let c = car("parity_rwd_d");
    let max = c.def.wheels[0].max_steer_rad;
    // Walking pace: lock fraction ≈ 1. Right key = negative angle.
    let speed = 1.5;
    let (_, at_027) = steer_after(&c, speed, 1.0, 65);
    let (mut rig, at_029) = steer_after(&c, speed, 1.0, 70);
    assert!(at_027.abs() < 0.99 * max, "{at_027} vs {max}");
    assert!(at_029.abs() > 0.99 * max, "{at_029} vs {max}");
    // Constant rate: about half way at half the time.
    let (_, half) = steer_after(&c, speed, 1.0, 34);
    assert!((half.abs() / max - 0.5).abs() < 0.03, "{}", half / max);
    // Release: back to centre in ~0.33 s.
    let a = reference_assists();
    let mut t_back = None;
    for tick in 0..240 {
        rig.step(&VehicleInput::default(), &a);
        if rig.vehicle.telemetry().steer_rad.abs() < 1e-4 {
            t_back = Some((tick + 1) as f32 / 240.0);
            break;
        }
    }
    let t = t_back.expect("returned to centre");
    assert!((0.31..0.35).contains(&t), "{t}");
}

#[test]
fn plain_lock_shrinks_with_speed() {
    plain_on();
    let c = car("parity_rwd_d");
    let max = c.def.wheels[0].max_steer_rad;
    // 160 km/h, key held 0.4 s: lock fraction 0.35–0.45 (75 % default curve).
    let (rig, s) = steer_after(&c, 160.0 / 3.6, 1.0, 96);
    let frac = s.abs() / max;
    let (_, f) = rig.vehicle.plain_readout().unwrap();
    assert!((0.35..0.47).contains(&frac), "{frac} (readout {f})");
}

#[test]
fn plain_throttle_is_raw() {
    plain_on();
    let c = car("parity_rwd_d");
    let mut rig = Rig::new(&c);
    rig.teleport([0.0; 3], 0.0, 15.0, 0.0);
    let a = reference_assists();
    rig.step(
        &VehicleInput {
            throttle: 1.0,
            ..Default::default()
        },
        &a,
    );
    // One tick after the key: the engine sees the full key (no pedal filter, no shaping).
    assert_eq!(rig.vehicle.telemetry().throttle, 1.0);
}
