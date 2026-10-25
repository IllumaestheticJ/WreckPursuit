//! Playtest row 141 ("car correction algorithm does some really weird stuff"): the owner's clip
//! inputs replayed on the shipped player path, with each player steering aid switched off in
//! turn, to see which one moves the front wheels away from where the key puts them.
//!
//! Owner setup: rampart_50, ABS Half, TCS Off, ESC off, manual clutch, 2nd gear, countersteer
//! help 5 %, speed-sensitive steering and the throttle filter on, flat ground, from rest.
//! Inputs (lead's read of the clip's input panel): 0.8–3.8 s full left + full throttle;
//! 4.0 s steer released, throttle on; 4.2–4.8 s nothing; 5.0 s on full right, the clutch from
//! 5.4 s.
//!
//! Prints, every 0.1 s: the key, the front wheels' angle (°, + = left), the angle the plain
//! rack alone would give, the front axle's direction of travel (°), body slip (°), yaw rate
//! (°/s), speed; then per variant the wheel angle so the differences show which aid acts.
//!
//! `cargo run --release -p wp_vehicle --example row141`

use std::path::PathBuf;

use wp_contracts::{AbsLevel, AssistProfile, LoadedCar, TcsLevel, VehicleInput, PHYSICS_DT};
use wp_vehicle::drift_lab::slide_state;
use wp_vehicle::drift_lab_w::load_any_car;
use wp_vehicle::grip_lab::{self, GripBalance, LockMode};
use wp_vehicle::plain::{self, PlainSteer};
use wp_vehicle::rig::Rig;

const TPS: usize = 240;

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    load_any_car(&data, id).unwrap()
}

/// The owner's inputs at time `t` (s).
fn input(t: f32, gear: i8) -> VehicleInput {
    let (steer, throttle) = if t < 0.8 {
        (0.0, 0.0)
    } else if t < 4.0 {
        (-1.0, 1.0)
    } else if t < 4.2 {
        (0.0, 1.0)
    } else if t < 5.0 {
        (0.0, 0.0)
    } else {
        (1.0, 0.0)
    };
    VehicleInput {
        steer,
        throttle,
        clutch: if t >= 5.4 { 1.0 } else { 0.0 },
        gear_up: gear < 2 && t < 0.7,
        ..Default::default()
    }
}

/// One row of the log.
#[derive(Clone, Copy, Default)]
struct Row {
    t: f32,
    key: f32,
    wheel_deg: f32,
    travel_deg: f32,
    slip_deg: f32,
    yaw_dps: f32,
    kmh: f32,
    gear: i8,
}

fn run(c: &LoadedCar, a: &AssistProfile) -> Vec<Row> {
    let mut rig = Rig::new(c);
    rig.set_player_input(true);
    let mut out = Vec::new();
    let mut prev_gear_up = false;
    for k in 0..(6.25 * TPS as f32) as usize {
        let t = k as f32 * PHYSICS_DT;
        let mut inp = input(t, rig.vehicle.gear());
        // A key press is an edge: release between presses.
        if inp.gear_up && prev_gear_up {
            inp.gear_up = false;
        }
        prev_gear_up = inp.gear_up;
        // Launch on the automatic clutch (the clip starts from rest with the engine running);
        // the owner's manual clutch from 1 s.
        let launch = AssistProfile {
            auto_clutch: true,
            ..*a
        };
        rig.step(&inp, if t < 1.0 { &launch } else { a });
        if k % (TPS / 10) == 0 {
            let s = slide_state(&rig);
            out.push(Row {
                t,
                key: inp.steer,
                wheel_deg: rig.vehicle.telemetry().steer_rad.to_degrees(),
                travel_deg: s.front_axle_angle.to_degrees(),
                slip_deg: s.body_slip.to_degrees(),
                yaw_dps: s.yaw_rate.to_degrees(),
                kmh: rig.state().speed_mps * 3.6,
                gear: rig.vehicle.gear(),
            });
        }
    }
    out
}

fn main() {
    let c = car(&std::env::var("CAR").unwrap_or_else(|_| "rampart_50".into()));
    let help: f32 = std::env::var("CSTR")
        .ok()
        .map_or(0.05, |v| v.parse().unwrap());
    let a = AssistProfile {
        abs: AbsLevel::Half,
        tcs: TcsLevel::Off,
        esc: false,
        auto_gears: false,
        auto_clutch: false,
        countersteer_help: help,
        ..AssistProfile::keyboard_default()
    };
    let shipped_plain = wp_vehicle::lab_scope::shipped_plain();
    let shipped_grip = GripBalance::shipped();
    let variants: Vec<(&str, PlainSteer, GripBalance, f32)> = vec![
        ("shipped", shipped_plain, shipped_grip, help),
        (
            "no transition aids",
            PlainSteer {
                trans_follow: false,
                ..shipped_plain
            },
            shipped_grip,
            help,
        ),
        (
            "no countersteer limit",
            PlainSteer {
                counter_margin_deg: 0.0,
                ..shipped_plain
            },
            shipped_grip,
            help,
        ),
        (
            "no drift lock / over rack",
            PlainSteer {
                drift_lock_extra: 0.0,
                over_rate: 1.0,
                ..shipped_plain
            },
            shipped_grip,
            help,
        ),
        (
            "no held-key cut",
            shipped_plain,
            GripBalance {
                slide_lock: 1.0,
                slide_lock_release_s: 0.0,
                ..shipped_grip
            },
            help,
        ),
        (
            "no smart lock",
            shipped_plain,
            GripBalance {
                lock_mode: LockMode::Plain,
                ..shipped_grip
            },
            help,
        ),
        ("countersteer help 0", shipped_plain, shipped_grip, 0.0),
        (
            "smart lock: plain lock in slides both ways",
            shipped_plain,
            GripBalance {
                smart_fade_both: true,
                ..shipped_grip
            },
            help,
        ),
        (
            "all three off (cut, limit, smart)",
            PlainSteer {
                counter_margin_deg: 0.0,
                ..shipped_plain
            },
            GripBalance {
                slide_lock: 1.0,
                slide_lock_release_s: 0.0,
                lock_mode: LockMode::Plain,
                ..shipped_grip
            },
            help,
        ),
    ];
    let mut logs = Vec::new();
    for (name, p, g, h) in &variants {
        plain::set_override(Some(*p));
        grip_lab::set_override(Some(*g));
        logs.push((
            *name,
            run(
                &c,
                &AssistProfile {
                    countersteer_help: *h,
                    ..a
                },
            ),
        ));
    }
    plain::set_override(None);
    grip_lab::set_override(None);
    let base = &logs[0].1;
    println!(
        "| t s | key | gear | wheels ° | travel ° | body slip ° | yaw °/s | km/h | {} |",
        logs[1..]
            .iter()
            .map(|(n, _)| *n)
            .collect::<Vec<_>>()
            .join(" | ")
    );
    println!("|{}", "---|".repeat(8 + logs.len() - 1));
    for (i, r) in base.iter().enumerate() {
        let others: Vec<String> = logs[1..]
            .iter()
            .map(|(_, l)| format!("{:+.1}", l[i].wheel_deg))
            .collect();
        println!(
            "| {:.1} | {:+.0} | {} | {:+.1} | {:+.1} | {:+.1} | {:+.0} | {:.0} | {} |",
            r.t,
            r.key,
            r.gear,
            r.wheel_deg,
            r.travel_deg,
            r.slip_deg,
            r.yaw_dps,
            r.kmh,
            others.join(" | ")
        );
    }
}
