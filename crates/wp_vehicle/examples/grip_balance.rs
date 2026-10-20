//! Grip-driving balance tables (docs/integration/grip-balance.md): how the Drift Lab W cars
//! behave at the limit **without** drifting (owner: "turn-in is still severe understeer").
//! Runs with the lab's defaults (plain keyboard steering, lab ride tuning, spin model, the
//! owner's assists) unless told otherwise.
//!
//! `cargo run --release -p wp_vehicle --example grip_balance [all|circle|ug|tap|lift|power|regress|sweep|trace] [car_id ...]`
//!
//! Env: `GRIP='{...}'` the grip-balance lab set (missing fields = the data); `GRIP=new` the lab
//! default (the "new" button); unset = the data ("before"). `PLAIN='{...}'` steering
//! constants. `AB=1` prints every table for both "before" and "new".

use std::path::PathBuf;

use wp_contracts::{LoadedCar, VehicleInput};
use wp_vehicle::car_lab::{self, CarTune};
use wp_vehicle::drift_lab::{slide_state, tap_response};
use wp_vehicle::drift_lab_w::{
    handbrake_pivot, lane_change_slip, launch_yaw, load_any_car, plain_hold, reference_assists,
    PlainDriver,
};
use wp_vehicle::grip_lab::{self, GripBalance};
use wp_vehicle::math::{atan2, V3};
use wp_vehicle::plain::{self, PlainSteer};
use wp_vehicle::ride_lab::{self, RideTune};
use wp_vehicle::rig::Rig;

const TPS: usize = 240;
const MPH: f32 = 0.44704;

/// Load a car; offline car-data experiments (set before spawn): `YAW_INERTIA=k` (yaw inertia
/// × k), `FRONT_WEIGHT=f` (static front weight fraction), `CG_H=k` (CG height × k).
fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let mut c = load_any_car(&data, id).unwrap();
    let env = |k: &str| std::env::var(k).ok().map(|v| v.parse::<f32>().expect(k));
    if let Some(k) = env("YAW_INERTIA") {
        c.def.inertia_kgm2[2] *= k;
    }
    if let Some(f) = env("FRONT_WEIGHT") {
        c.def.front_weight_frac = f;
    }
    if let Some(k) = env("CG_H") {
        c.def.cg_height_m *= k;
    }
    c
}

/// Speed controller used by every run (the lab's "cruise throttle").
fn hold_speed(rig: &Rig, target: f32) -> (f32, f32) {
    let err = target - rig.state().speed_mps;
    if err > -1.0 {
        ((0.25 + 0.35 * err).clamp(0.0, 1.0), 0.0)
    } else {
        (0.0, (-0.2 * err).clamp(0.0, 1.0))
    }
}

/// A rig rolling straight at `v` m/s, settled 1 s with automatic gears (then the lab's
/// manual gears keep that gear).
fn rolling(c: &LoadedCar, v: f32) -> Rig {
    let mut rig = Rig::new(c);
    rig.teleport([0.0; 3], 0.0, v, 0.0);
    let auto = wp_contracts::AssistProfile {
        auto_gears: true,
        ..reference_assists()
    };
    for _ in 0..TPS {
        let (throttle, brake) = hold_speed(&rig, v);
        rig.step(
            &VehicleInput {
                throttle,
                brake,
                ..Default::default()
            },
            &auto,
        );
    }
    rig
}

fn heading(rig: &Rig) -> f32 {
    let f = rig.vehicle.chassis().rot * V3::Y;
    atan2(-f.x, f.y)
}

/// Per-tick sample of a run (degrees, g, m/s).
#[derive(Debug, Clone, Copy, Default)]
struct Sample {
    v: f32,
    yaw_dps: f32,
    slip_deg: f32,
    wheel_deg: f32,
    front_slip_deg: f32,
    rear_slip_deg: f32,
    ay_g: f32,
}

fn sample(rig: &Rig) -> Sample {
    let s = slide_state(rig);
    Sample {
        v: s.v_fwd,
        yaw_dps: s.yaw_rate.to_degrees(),
        slip_deg: s.body_slip.to_degrees(),
        wheel_deg: s.steer.to_degrees(),
        front_slip_deg: (s.steer - s.front_axle_angle).to_degrees(),
        rear_slip_deg: s.rear_axle_slip.to_degrees(),
        ay_g: s.v_fwd * s.yaw_rate / 9.81,
    }
}

fn mean(xs: &[Sample], f: impl Fn(&Sample) -> f32) -> f32 {
    xs.iter().map(f).sum::<f32>() / xs.len().max(1) as f32
}

/// Steady circle (parity suite's): straight at `mph`, then the left key held with the cruise
/// controller for 6 s; means over the last 2 s.
#[derive(Debug, Clone, Copy)]
struct Circle {
    radius: f32,
    ay_g: f32,
    yaw: f32,
    slip: f32,
    wheel: f32,
    front: f32,
    rear: f32,
    mph: f32,
    spun: bool,
}

fn circle(c: &LoadedCar, mph: f32) -> Circle {
    let v = mph * MPH;
    let mut rig = rolling(c, v);
    let a = reference_assists();
    let mut tail = Vec::new();
    let mut spun = false;
    // THR=fixed: the throttle stays at the straight-line cruise value (no speed controller).
    let fixed = std::env::var("THR").as_deref() == Ok("fixed");
    let thr0 = hold_speed(&rig, v).0;
    for tick in 0..6 * TPS {
        let (throttle, brake) = if fixed {
            (thr0, 0.0)
        } else {
            hold_speed(&rig, v)
        };
        rig.step(
            &VehicleInput {
                steer: -1.0,
                throttle,
                brake,
                ..Default::default()
            },
            &a,
        );
        let s = sample(&rig);
        spun |= s.slip_deg.abs() > 60.0 || s.v < 0.0;
        if tick >= 4 * TPS {
            tail.push(s);
        }
    }
    let yaw = mean(&tail, |s| s.yaw_dps);
    let vv = mean(&tail, |s| s.v);
    Circle {
        radius: vv / yaw.to_radians().abs().max(1e-3),
        ay_g: mean(&tail, |s| s.ay_g),
        yaw,
        slip: -mean(&tail, |s| s.slip_deg),
        wheel: mean(&tail, |s| s.wheel_deg),
        front: mean(&tail, |s| s.front_slip_deg),
        rear: -mean(&tail, |s| s.rear_slip_deg),
        mph: vv / MPH,
        spun,
    }
}

/// Understeer gradient, deg/g: constant speed, the steer input ramped slowly 0 → full left
/// over 10 s (a fractional key: the rack target is key × lock). `δ − L·r/v` against lateral g,
/// fitted over 0.1–0.4 g and 0.4–0.7 g. Also the peak lateral g reached and the front / rear
/// slip angles there.
#[derive(Debug, Clone, Copy)]
struct Gradient {
    low: f32,
    high: f32,
    max_g: f32,
    slip_at_max: f32,
}

fn gradient(c: &LoadedCar, kmh: f32) -> Gradient {
    let v = kmh / 3.6;
    let mut rig = rolling(c, v);
    let a = reference_assists();
    let wb = c.def.wheelbase_m;
    let mut pts: Vec<(f32, f32, f32)> = Vec::new();
    for tick in 0..10 * TPS {
        let (throttle, brake) = hold_speed(&rig, v);
        let k = tick as f32 / (10 * TPS) as f32;
        rig.step(
            &VehicleInput {
                steer: -k,
                throttle,
                brake,
                ..Default::default()
            },
            &a,
        );
        let s = slide_state(&rig);
        if s.body_slip.abs().to_degrees() > 30.0 || s.v_fwd < 3.0 {
            break;
        }
        let ay = s.v_fwd * s.yaw_rate / 9.81;
        let dyn_deg = (s.steer - wb * s.yaw_rate / s.v_fwd).to_degrees();
        pts.push((ay, dyn_deg, -s.body_slip.to_degrees()));
    }
    let fit = |lo: f32, hi: f32| {
        let sel: Vec<_> = pts.iter().filter(|p| p.0 >= lo && p.0 <= hi).collect();
        if sel.len() < 10 {
            return f32::NAN;
        }
        let n = sel.len() as f32;
        let mx = sel.iter().map(|p| p.0).sum::<f32>() / n;
        let my = sel.iter().map(|p| p.1).sum::<f32>() / n;
        let sxy: f32 = sel.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
        let sxx: f32 = sel.iter().map(|p| (p.0 - mx) * (p.0 - mx)).sum();
        sxy / sxx.max(1e-6)
    };
    let (max_g, slip_at_max) =
        pts.iter().fold(
            (0.0f32, 0.0f32),
            |m, p| if p.0 > m.0 { (p.0, p.2) } else { m },
        );
    Gradient {
        low: fit(0.1, 0.4),
        high: fit(0.4, 0.7),
        max_g,
        slip_at_max,
    }
}

/// Lift-off at turn-in: straight at `mph` with cruise throttle, then the left key held with
/// the throttle either kept (cruise) or lifted to 0; 1.5 s. Returns (yaw °/s at 0.6 s, peak
/// body slip °) for (kept, lifted).
fn lift_off(c: &LoadedCar, mph: f32) -> ((f32, f32), (f32, f32)) {
    let v = mph * MPH;
    let run = |lift: bool| {
        let mut rig = rolling(c, v);
        let a = reference_assists();
        let (thr0, _) = hold_speed(&rig, v);
        let (mut yaw06, mut slip) = (0.0f32, 0.0f32);
        for tick in 0..3 * TPS / 2 {
            let throttle = if lift { 0.0 } else { thr0 };
            rig.step(
                &VehicleInput {
                    steer: -1.0,
                    throttle,
                    ..Default::default()
                },
                &a,
            );
            let s = sample(&rig);
            if tick == 6 * TPS / 10 {
                yaw06 = s.yaw_dps;
            }
            slip = slip.max(-s.slip_deg);
        }
        (yaw06, slip)
    };
    (run(false), run(true))
}

/// Lift mid-corner: the left key held 3 s with cruise throttle at `mph`, then the throttle
/// lifted (key still held) for 1.5 s. Returns (body slip ° before, peak body slip ° after
/// the lift, peak yaw-rate rise °/s).
fn lift_mid(c: &LoadedCar, mph: f32) -> (f32, f32, f32) {
    let v = mph * MPH;
    let mut rig = rolling(c, v);
    let a = reference_assists();
    let (mut before, mut after, mut yaw0, mut dyaw) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for tick in 0..(9 * TPS / 2) {
        let lifted = tick >= 3 * TPS;
        let (throttle, brake) = if lifted {
            (0.0, 0.0)
        } else {
            hold_speed(&rig, v)
        };
        rig.step(
            &VehicleInput {
                steer: -1.0,
                throttle,
                brake,
                ..Default::default()
            },
            &a,
        );
        let s = sample(&rig);
        if tick == 3 * TPS - 1 {
            before = -s.slip_deg;
            yaw0 = s.yaw_dps;
        }
        if lifted {
            after = after.max(-s.slip_deg);
            dyaw = dyaw.max(s.yaw_dps - yaw0);
        }
    }
    (before, after, dyaw)
}

/// RWD power-on: the left key held 3 s at `mph` with cruise throttle, then full throttle
/// (key held) for 1.5 s. Returns (body slip ° before, peak body slip ° after, spun).
fn power_on(c: &LoadedCar, mph: f32) -> (f32, f32, bool) {
    let v = mph * MPH;
    let mut rig = rolling(c, v);
    let a = reference_assists();
    let (mut before, mut after, mut spun) = (0.0f32, 0.0f32, false);
    for tick in 0..(9 * TPS / 2) {
        let full = tick >= 3 * TPS;
        let (throttle, brake) = if full {
            (1.0, 0.0)
        } else {
            hold_speed(&rig, v)
        };
        rig.step(
            &VehicleInput {
                steer: -1.0,
                throttle,
                brake,
                ..Default::default()
            },
            &a,
        );
        let s = sample(&rig);
        if tick == 3 * TPS - 1 {
            before = -s.slip_deg;
        }
        if full {
            after = after.max(-s.slip_deg);
            spun |= s.slip_deg.abs() > 60.0 || s.v < 0.0;
        }
    }
    (before, after, spun)
}

fn set_grip(g: Option<GripBalance>) {
    grip_lab::set_override(g);
}

fn configs() -> Vec<(String, Option<GripBalance>)> {
    match std::env::var("GRIP").as_deref() {
        Ok("new") => vec![("new".into(), Some(GripBalance::lab_default()))],
        Ok(j) if j.starts_with('[') => {
            let v: Vec<GripBalance> = serde_json::from_str(j).expect("GRIP json list");
            v.into_iter()
                .enumerate()
                .map(|(i, g)| (format!("GRIP[{i}]"), Some(g)))
                .collect()
        }
        Ok(j) => vec![(
            "GRIP".into(),
            Some(serde_json::from_str(j).expect("GRIP json")),
        )],
        Err(_) if std::env::var("AB").is_ok() => vec![
            ("before".into(), None),
            ("new".into(), Some(GripBalance::lab_default())),
        ],
        Err(_) => vec![("before".into(), None)],
    }
}

fn main() {
    // The Drift Lab W defaults (what the owner drives).
    match std::env::var("PLAIN").as_deref() {
        Ok(j) => plain::set_override(Some(serde_json::from_str(j).expect("PLAIN json"))),
        Err(_) => plain::set_override(Some(PlainSteer::default())),
    }
    // RIDE=data|slow|medium: the suspension data or a W ride preset instead of the lab's ride
    // default (comparison only).
    if std::env::var("RIDE").as_deref() == Ok("data") {
        ride_lab::set_override(None);
    } else if std::env::var("RIDE").as_deref() == Ok("slow") {
        // Drift Lab W's "slow weight (clip 4)" preset.
        ride_lab::set_override(Some(RideTune::slow_weight()));
    } else if std::env::var("RIDE").as_deref() == Ok("medium") {
        // Drift Lab W step 10's "medium weight" ride (W's "new" default).
        ride_lab::set_override(Some(RideTune::medium_weight()));
    } else {
        ride_lab::set_override(Some(RideTune::lab_default()));
    }
    // CARLAB=data: the tyre data instead of the lab's spin model (comparison only).
    if std::env::var("CARLAB").as_deref() == Ok("data") {
        car_lab::set_override(None);
    } else {
        car_lab::set_override(Some(CarTune::spin_candidate()));
    }
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let what = if args.first().is_some_and(|a| {
        [
            "all", "circle", "ug", "tap", "lift", "power", "regress", "sweep", "trace", "dtrace",
            "robust", "ltrace", "chatter", "pull",
        ]
        .contains(&a.as_str())
    }) {
        args.remove(0)
    } else {
        "all".into()
    };
    if args.is_empty() {
        args = ["parity_rwd_d", "parity_rwd_a", "parity_fwd_c", "rampart_50"]
            .iter()
            .map(|s| s.to_string())
            .collect();
    }
    if what == "sweep" {
        sweep(&args);
        return;
    }
    if what == "pull" {
        for (name, g) in configs() {
            set_grip(g);
            println!("\n# {name}");
            pull(&args);
        }
        return;
    }
    if what == "chatter" {
        for (name, g) in configs() {
            set_grip(g);
            println!("\n# {name}");
            chatter(&args);
        }
        return;
    }
    if what == "ltrace" {
        ltrace(args.first().map_or("parity_rwd_d", String::as_str));
        return;
    }
    if what == "dtrace" {
        dtrace(args.first().map_or("parity_rwd_d", String::as_str));
        return;
    }
    if what == "trace" {
        trace(args.first().map_or("parity_rwd_d", String::as_str));
        return;
    }
    for (name, g) in configs() {
        set_grip(g);
        println!("\n# balance: {name} {g:?}");
        if what == "all" || what == "circle" {
            circle_table(&args);
        }
        if what == "all" || what == "ug" {
            ug_table(&args);
        }
        if what == "all" || what == "tap" {
            tap_table(&args);
        }
        if what == "all" || what == "lift" || what == "power" {
            lift_table(&args);
        }
        if what == "all" || what == "regress" {
            regress_table(&args);
        }
        if what == "all" || what == "robust" {
            println!("\n## Drift robustness (tyre-slip set: rwd_d / a / b × counter key at 6 / 12 / 20° × 0.3 / 0.5 / 0.7 g): held {} / 27", robust());
        }
    }
}

fn circle_cell(r: &Circle) -> String {
    format!(
        "{:.0} mph: {:.0} m / {:.2} g / {:.1} °/s / slip {:.1}° / wheel {:.1}° / front {:.1}° rear {:.1}°{}",
        r.mph,
        r.radius,
        r.ay_g,
        r.yaw,
        r.slip,
        r.wheel,
        r.front,
        r.rear,
        if r.spun { " SPUN" } else { "" }
    )
}

fn circle_table(ids: &[String]) {
    println!("\n## Key held + cruise throttle (steady circle, last 2 s): radius / lat g / yaw / body slip / wheel / front, rear axle slip");
    println!("| car | 30 mph | 60 mph | 100 mph |");
    println!("|---|---|---|---|");
    for id in ids {
        let c = car(id);
        let cells: Vec<String> = [30.0f32, 60.0, 100.0]
            .iter()
            .map(|m| circle_cell(&circle(&c, *m)))
            .collect();
        println!("| {id} | {} |", cells.join(" | "));
    }
}

fn ug_table(ids: &[String]) {
    println!("\n## Understeer gradient (slow steer ramp at constant speed): deg/g over 0.1–0.4 g / 0.4–0.7 g; peak lat g (body slip there)");
    println!("| car | 60 km/h | 100 km/h |");
    println!("|---|---|---|");
    for id in ids {
        let c = car(id);
        let cells: Vec<String> = [60.0f32, 100.0]
            .iter()
            .map(|k| {
                let g = gradient(&c, *k);
                format!(
                    "{:.2} / {:.2} deg/g; {:.2} g (slip {:.1}°)",
                    g.low, g.high, g.max_g, g.slip_at_max
                )
            })
            .collect();
        println!("| {id} | {} |", cells.join(" | "));
    }
}

fn tap_table(ids: &[String]) {
    println!(
        "\n## Key taps holding speed: peak yaw °/s / heading ° after 1.5 s / peak body slip °"
    );
    println!("| car | mph | tap 0.1 s | 0.2 s | 0.4 s |");
    println!("|---|---|---|---|---|");
    let a = reference_assists();
    for id in ids {
        let c = car(id);
        for mph in [60.0f32, 100.0] {
            let cells: Vec<String> = [0.1f32, 0.2, 0.4]
                .iter()
                .map(|t| {
                    let r = tap_response(&c, &a, mph * MPH, *t);
                    format!(
                        "{:.1} / {:.1} / {:.1} (wheel {:.1}°)",
                        r.peak_yaw_dps, r.heading_deg, r.max_slip_deg, r.peak_wheel_deg
                    )
                })
                .collect();
            println!("| {id} | {mph:.0} | {} |", cells.join(" | "));
        }
    }
}

fn lift_table(ids: &[String]) {
    println!("\n## Lift-off and power-on (left key held)");
    println!("| car | turn-in 60 mph: yaw at 0.6 s / peak slip, throttle kept → lifted | mid-corner lift 60 mph: slip before → peak after, yaw rise | power-on 35 mph: slip before → peak after |");
    println!("|---|---|---|---|");
    for id in ids {
        let c = car(id);
        let ((yk, sk), (yl, sl)) = lift_off(&c, 60.0);
        let (b, a, dy) = lift_mid(&c, 60.0);
        let (pb, pa, ps) = power_on(&c, 35.0);
        println!(
            "| {id} | {yk:.1} °/s / {sk:.1}° → {yl:.1} °/s / {sl:.1}° | {b:.1}° → {a:.1}°, +{dy:.1} °/s | {pb:.1}° → {pa:.1}°{} |",
            if ps { " SPUN" } else { "" }
        );
    }
}

fn regress_table(ids: &[String]) {
    println!(
        "\n## Regression: drift hold (spin model), launch, released keys, lane change, handbrake"
    );
    println!("| car | counter key 6° + full thr | into-turn key held | keys released | launch yaw ° flat / 1° slope | lane change 100 km/h slip ° | handbrake 40 km/h 0.6 s |");
    println!("|---|---|---|---|---|---|---|");
    let a = reference_assists();
    for id in ids {
        let c = car(id);
        let hold = |d| plain_hold(&c, &a, d, 40.0, 0.6);
        let ctr = hold(PlainDriver::HoldCounter {
            entry_deg: 6.0,
            throttle: 1.0,
        });
        let into = hold(PlainDriver::IntoTurnHeld { entry_deg: 12.0 });
        let rel = hold(PlainDriver::Released { entry_deg: 12.0 });
        let l0 = launch_yaw(&c, &a, 0.0);
        let l1 = launch_yaw(&c, &a, 1.0);
        let lc = lane_change_slip(&c, &a);
        let hb = handbrake_pivot(&c, &a, 40.0 / 3.6, 0.6);
        println!(
            "| {id} | {} | {} | {} (end {:.0}°) | {l0:.1} / {l1:.1} | {lc:.1} | {:.0}° / {} |",
            ctr.cell(),
            into.cell(),
            rel.cell(),
            rel.end_deg,
            hb.pivot_deg,
            if hb.spun {
                "spun"
            } else if hb.catch_s.is_some() {
                "caught"
            } else {
                "no catch"
            }
        );
    }
}

/// Knob sweep: `grip_lab::sweep_sets()`, or the `GRIP` list (`GRIP='[{…}, …]'`).
fn sweep(ids: &[String]) {
    let sets: Vec<(String, GripBalance)> = if std::env::var("GRIP").is_ok() {
        configs()
            .into_iter()
            .map(|(n, g)| (n, g.unwrap_or_default()))
            .collect()
    } else {
        let mut v = vec![("data".to_string(), GripBalance::default())];
        v.extend(grip_lab::sweep_sets());
        v
    };
    println!("\n## Sweep (key held 60 mph circle: body slip ° / front axle slip ° / lat g; tap 60 mph peak yaw 0.2 → 0.4 s; turn-in lift yaw 0.6 s kept → lifted; counter-key drift)");
    println!("| car | set | 30 mph circle | 60 mph circle | 100 mph circle | tap 60 0.1 / 0.2 / 0.4 s: heading | lift kick | drift | launch 1° | lane change slip ° | robust /27 |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    let a = reference_assists();
    for id in ids {
        let c = car(id);
        for (name, g) in &sets {
            set_grip(Some(*g));
            let c30 = circle(&c, 30.0);
            let c60 = circle(&c, 60.0);
            let c100 = circle(&c, 100.0);
            let rb = if id == "parity_rwd_d" {
                robust().to_string()
            } else {
                "–".into()
            };
            let t1 = tap_response(&c, &a, 60.0 * MPH, 0.1);
            let t2 = tap_response(&c, &a, 60.0 * MPH, 0.2);
            let t4 = tap_response(&c, &a, 60.0 * MPH, 0.4);
            let ((yk, _), (yl, _)) = lift_off(&c, 60.0);
            let d = plain_hold(
                &c,
                &a,
                PlainDriver::HoldCounter {
                    entry_deg: 6.0,
                    throttle: 1.0,
                },
                40.0,
                0.6,
            );
            let l1 = launch_yaw(&c, &a, 1.0);
            let lc = lane_change_slip(&c, &a);
            println!(
                "| {id} | {name} | {:.1}° / {:.1}° / {:.2} g{} | {:.1}° / {:.1}° / {:.2} g{} | {:.1}° / {:.1}° / {:.2} g{} | {:.1} / {:.1} / {:.1}° | {yk:.1} → {yl:.1} | {} | {l1:.1} | {lc:.1} | {rb} |",
                c30.slip,
                c30.front,
                c30.ay_g,
                if c30.spun { " SPUN" } else { "" },
                c60.slip,
                c60.front,
                c60.ay_g,
                if c60.spun { " SPUN" } else { "" },
                c100.slip,
                c100.front,
                c100.ay_g,
                if c100.spun { " SPUN" } else { "" },
                t1.heading_deg,
                t2.heading_deg,
                t4.heading_deg,
                d.cell()
            );
        }
    }
    set_grip(None);
}

/// Tick trace of the 60 mph key-held circle (every 0.1 s).
fn trace(id: &str) {
    let c = car(id);
    for (name, g) in configs() {
        set_grip(g);
        let mph: f32 = std::env::var("MPH").map_or(60.0, |m| m.parse().unwrap());
        println!("\n# {name}: {mph} mph, left key held, cruise throttle");
        println!("t s | mph | thr | gear | wheel ° | front slip ° | rear slip ° | body slip ° | yaw °/s | lat g | heading °");
        let v = mph * MPH;
        let mut rig = rolling(&c, v);
        let a = reference_assists();
        let h0 = heading(&rig);
        for tick in 0..4 * TPS {
            let (throttle, brake) = hold_speed(&rig, v);
            rig.step(
                &VehicleInput {
                    steer: -1.0,
                    throttle,
                    brake,
                    ..Default::default()
                },
                &a,
            );
            if tick.is_multiple_of(TPS / 10) {
                let s = sample(&rig);
                println!(
                    "{:.2} | {:.1} | {throttle:.2} | {} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.2} | {:.0}",
                    tick as f32 / TPS as f32,
                    s.v / MPH,
                    rig.vehicle.gear(),
                    s.wheel_deg,
                    s.front_slip_deg,
                    -s.rear_slip_deg,
                    -s.slip_deg,
                    s.yaw_dps,
                    s.ay_g,
                    (heading(&rig) - h0).to_degrees()
                );
            }
        }
    }
}

/// Tick trace of the counter-key drift (plain_hold's HoldCounter 6°, every 0.05 s).
fn dtrace(id: &str) {
    let c = car(id);
    for (name, g) in configs() {
        set_grip(g);
        println!("\n# {name}: counter key at 6° + full throttle, 40 m / 0.6 g entry");
        println!("t s | steer key | thr | wheel ° | slide ° | yaw °/s | km/h");
        let mut n = 0usize;
        wp_vehicle::drift_lab_w::plain_drift_run_with(
            &c,
            &reference_assists(),
            PlainDriver::HoldCounter {
                entry_deg: 6.0,
                throttle: 1.0,
            },
            40.0,
            0.6,
            |rig, steer, thr| {
                if n.is_multiple_of(12) && n < 240 * 3 {
                    let s = sample(rig);
                    println!(
                        "{:.2} | {steer:.0} | {thr:.1} | {:.1} | {:.1} | {:.1} | {:.0}",
                        n as f32 / 240.0,
                        s.wheel_deg,
                        -s.slip_deg,
                        s.yaw_dps,
                        s.v * 3.6
                    );
                }
                n += 1;
            },
        );
    }
}

/// Held count over the tyre-slip robustness set (27 entries).
fn robust() -> usize {
    let a = reference_assists();
    let mut n = 0;
    for id in ["parity_rwd_d", "parity_rwd_a", "parity_rwd_b"] {
        let c = car(id);
        for e in [6.0f32, 12.0, 20.0] {
            for g in [0.3f32, 0.5, 0.7] {
                let d = PlainDriver::HoldCounter {
                    entry_deg: e,
                    throttle: 1.0,
                };
                let m = plain_hold(&c, &a, d, 40.0, g);
                if std::env::var("VERBOSE").is_ok() {
                    println!("  {id} {e}° {g} g: {}", m.cell());
                }
                if m.held {
                    n += 1;
                }
            }
        }
    }
    n
}

/// Tick trace of the 100 km/h keyboard lane change (drift_lab_w::LANE_CHANGE_KEYS).
fn ltrace(id: &str) {
    let c = car(id);
    for (name, g) in configs() {
        set_grip(g);
        println!("\n# {name}: lane change 100 km/h");
        println!("t s | key | thr | gear | wheel ° | front slip ° | rear slip ° | body slip ° | yaw °/s | front Fy kN | rear Fx kN | rear Fy kN | rear Fz kN");
        let v = 100.0 / 3.6;
        let mut rig = rolling(&c, v);
        let a = reference_assists();
        let script: Vec<f32> = wp_vehicle::drift_lab_w::LANE_CHANGE_KEYS
            .iter()
            .flat_map(|&(k, s)| std::iter::repeat_n(k, (s * TPS as f32) as usize))
            .collect();
        for tick in 0..script.len() + TPS {
            let (throttle, brake) = hold_speed(&rig, v);
            let steer = script.get(tick).copied().unwrap_or(0.0);
            rig.step(
                &VehicleInput {
                    steer,
                    throttle,
                    brake,
                    ..Default::default()
                },
                &a,
            );
            let every: usize = std::env::var("EVERY").map_or(TPS / 20, |e| e.parse().unwrap());
            if tick.is_multiple_of(every) {
                let s = sample(&rig);
                let t = rig.vehicle.telemetry();
                let fz = rig.vehicle.wheel_loads();
                println!(
                    "{:.2} | {steer:.0} | {throttle:.2} | {} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1}",
                    tick as f32 / TPS as f32,
                    rig.vehicle.gear(),
                    s.wheel_deg,
                    s.front_slip_deg,
                    -s.rear_slip_deg,
                    -s.slip_deg,
                    s.yaw_dps,
                    (t.fy[0] + t.fy[1]) / 1000.0,
                    (t.fx[2] + t.fx[3]) / 1000.0,
                    (t.fy[2] + t.fy[3]) / 1000.0,
                    (fz[2] + fz[3]) / 1000.0
                );
            }
        }
    }
}

/// Per-tick tyre load chatter: straight at 60 mph, then the 60 mph key-held circle; per
/// wheel the mean load and the RMS of its tick-to-tick change (kN), plus the same for the
/// side force.
fn chatter(ids: &[String]) {
    println!("\n## Tyre load chatter: per wheel mean Fz kN / RMS tick-to-tick ΔFz kN / RMS ΔFy kN (FL FR RL RR)");
    println!("| car | straight 60 mph | key held 60 mph (2–4 s) |");
    println!("|---|---|---|");
    let a = reference_assists();
    for id in ids {
        let c = car(id);
        let v = 60.0 * MPH;
        let mut rig = rolling(&c, v);
        let run = |rig: &mut Rig, steer: f32, n: usize, skip: usize| {
            let (mut sum, mut d2, mut dy2) = ([0.0f64; 4], [0.0f64; 4], [0.0f64; 4]);
            let mut prev: Option<([f32; 4], [f32; 4])> = None;
            let mut k = 0usize;
            for tick in 0..n {
                let (throttle, brake) = hold_speed(rig, v);
                rig.step(
                    &VehicleInput {
                        steer,
                        throttle,
                        brake,
                        ..Default::default()
                    },
                    &a,
                );
                if tick < skip {
                    continue;
                }
                let fz = rig.vehicle.wheel_loads();
                let fy = rig.vehicle.telemetry().fy;
                if let Some((pz, py)) = prev {
                    for i in 0..4 {
                        d2[i] += ((fz[i] - pz[i]) as f64).powi(2);
                        dy2[i] += ((fy[i] - py[i]) as f64).powi(2);
                    }
                    k += 1;
                }
                for i in 0..4 {
                    sum[i] += fz[i] as f64;
                }
                prev = Some((fz, fy));
            }
            (0..4)
                .map(|i| {
                    format!(
                        "{:.2} / {:.2} / {:.2}",
                        sum[i] / (k + 1) as f64 / 1000.0,
                        (d2[i] / k as f64).sqrt() / 1000.0,
                        (dy2[i] / k as f64).sqrt() / 1000.0
                    )
                })
                .collect::<Vec<_>>()
                .join(" · ")
        };
        let straight = run(&mut rig, 0.0, TPS, 0);
        let held = run(&mut rig, -1.0, 4 * TPS, 2 * TPS);
        println!("| {id} | {straight} | {held} |");
    }
}

/// Owner bug "rpm stuck / flat in 3rd": full throttle in 3rd from 40 km/h, steer 0, TCS off, with
/// the automatic clutch and with the manual clutch (pedal released). Every 0.5 s: km/h, engine
/// rpm, driven-wheel slip ratio, clutch engagement / locked.
fn pull(ids: &[String]) {
    for id in ids {
        let c = car(id);
        for auto_clutch in [true, false] {
            let a = wp_contracts::AssistProfile {
                auto_clutch,
                ..reference_assists()
            };
            let mut rig = rolling(&c, 40.0 / 3.6);
            // Into 3rd (manual gears).
            for _ in 0..240 {
                let g = rig.vehicle.gear();
                let (throttle, brake) = hold_speed(&rig, 40.0 / 3.6);
                rig.step(
                    &VehicleInput {
                        throttle,
                        brake,
                        gear_up: g < 3,
                        gear_down: g > 3,
                        ..Default::default()
                    },
                    &a,
                );
            }
            let driven: Vec<usize> = match c.def.drivetrain.layout {
                wp_contracts::DriveLayout::Fwd => vec![0, 1],
                wp_contracts::DriveLayout::Rwd => vec![2, 3],
                _ => vec![0, 1, 2, 3],
            };
            let mut line = Vec::new();
            for tick in 0..5 * TPS {
                rig.step(
                    &VehicleInput {
                        throttle: 1.0,
                        ..Default::default()
                    },
                    &a,
                );
                if tick % (TPS / 2) == TPS / 2 - 1 {
                    let st = rig.state();
                    let t = rig.vehicle.telemetry();
                    let v = st.speed_mps.max(0.5);
                    let slip = driven
                        .iter()
                        .map(|&i| st.wheel_states[i].omega * c.def.wheels[i].radius_m / v - 1.0)
                        .sum::<f32>()
                        / driven.len() as f32;
                    line.push(format!(
                        "{:.0} km/h {:.0} rpm g{} κ{:.2} c{:.2}{}",
                        st.speed_mps * 3.6,
                        st.engine_rpm,
                        rig.vehicle.gear(),
                        slip,
                        t.clutch,
                        if t.clutch_locked { "L" } else { "" }
                    ));
                }
            }
            println!(
                "{id} {}: {}",
                if auto_clutch {
                    "auto clutch"
                } else {
                    "manual clutch"
                },
                line.join(" | ")
            );
        }
    }
}
