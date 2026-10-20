//! Drift Lab W measurement tables (docs/integration/drift-lab-w.md): plain keyboard steering
//! (`wp_vehicle::plain`), TCS / ESC off, manual gears.
//! `cargo run --release -p wp_vehicle --example drift_lab_w [drift|tap|handbrake|hairpin] [car_id ...]`
//! Env: `PLAIN='{"t_full_s":0.2}'` (constants; missing fields = defaults), `PLAIN=off` (the
//! normal steering model with the same assists, for comparison), `RIDE_LAB=1` (the lab's
//! ride tuning default; `ride` sets its own), `RIDE='{...}'` (an extra row in `ride`).

use std::path::PathBuf;

use wp_contracts::LoadedCar;
use wp_vehicle::car_lab::{self, CarTune};
use wp_vehicle::drift_lab::{
    drift_run, drift_run_with, hairpin, slide_state, tap_response, DriftDriver,
};
use wp_vehicle::drift_lab_w::{
    handbrake_pivot, load_any_car, plain_drift_run, plain_hold, reference_assists, HoldMetrics,
    PlainDriver, PARITY_CARS, ROSTER_CARS,
};
use wp_vehicle::drift_lab_w::{roll_run, LANE_CHANGE_KEYS, S_BEND_KEYS};
use wp_vehicle::plain::{self, PlainSteer, SensApplies};
use wp_vehicle::ride::roll_kick;
use wp_vehicle::ride_lab::{self, RideTune};

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let mut c = load_any_car(&data, id).unwrap();
    // YAW_INERTIA=1.3: offline experiment, the car's yaw inertia × this (set before spawn).
    if let Ok(k) = std::env::var("YAW_INERTIA") {
        c.def.inertia_kgm2[2] *= k.parse::<f32>().expect("YAW_INERTIA number");
    }
    c
}

fn main() {
    match std::env::var("PLAIN").as_deref() {
        Ok("off") => plain::set_override(None),
        Ok(j) => plain::set_override(Some(serde_json::from_str(j).expect("PLAIN json"))),
        Err(_) => plain::set_override(Some(PlainSteer::default())),
    }
    if std::env::var("RIDE_LAB").is_ok() {
        // The lab's ride default (springs 0.9, bars 0.25, dampers 1.4 / 1.8).
        ride_lab::set_override(Some(RideTune::lab_default()));
    }
    println!("plain steering: {:?}", plain::active());
    println!("ride tuning: {:?}", ride_lab::active());
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("trace") {
        // trace <car> [naive|good|into]: every 0.05 s of a drift_run.
        let id = args.get(1).cloned().unwrap_or("parity_rwd_d".into());
        let d = match args.get(2).map(String::as_str) {
            Some("good") => DriftDriver::Good,
            Some("into") => DriftDriver::IntoTurn,
            _ => DriftDriver::Naive,
        };
        trace(&id, d);
        return;
    }
    let what = if args.first().is_some_and(|a| {
        [
            "drift",
            "plain",
            "sweep",
            "ride",
            "carlab",
            "weight",
            "vr",
            "sens",
            "ptrace",
            "tap",
            "handbrake",
            "hairpin",
        ]
        .contains(&a.as_str())
    }) {
        args.remove(0)
    } else {
        "all".into()
    };
    if args.is_empty() {
        args = PARITY_CARS
            .iter()
            .chain(ROSTER_CARS.iter())
            .map(|s| s.to_string())
            .collect();
    }
    if what == "all" || what == "drift" {
        drift_table(&args);
    }
    if what == "all" || what == "plain" {
        plain_table(&args);
    }
    if what == "ptrace" {
        ptrace(args.first().map_or("parity_rwd_d", String::as_str));
        return;
    }
    if what == "sens" {
        sens_table(&args);
        return;
    }
    if what == "vr" {
        vr_table(&args);
        return;
    }
    if what == "weight" {
        weight_table(&args);
        return;
    }
    if what == "carlab" {
        carlab_table(&args);
        return;
    }
    if what == "ride" {
        ride_table(&args);
        return;
    }
    if what == "sweep" {
        sweep(&args);
        throttle_diag(&args);
        return;
    }
    if what == "all" || what == "tap" {
        tap_table(&args);
    }
    if what == "all" || what == "handbrake" {
        handbrake_table(&args);
    }
    if what == "all" || what == "hairpin" {
        hairpin_table(&args);
    }
}

fn drift_table(args: &[String]) {
    println!("\n## Drift hold: 40 m circle at 0.6 g, enter with the key into the turn + throttle, then 6 s");
    println!(
        "| car | driver | intended side: drift s / band s / mean ° sd ° / outcome | max ° (either side) |"
    );
    println!("|---|---|---|---|");
    let a = reference_assists();
    for id in args {
        let c = car(id);
        for (name, d) in [
            ("Naive (counter key + full throttle)", DriftDriver::Naive),
            ("Good (counter key, throttle modulated)", DriftDriver::Good),
            ("AbruptLift", DriftDriver::AbruptLift),
            ("IntoTurn", DriftDriver::IntoTurn),
        ] {
            let (mut ang, mut back) = (Vec::new(), Vec::new());
            let r = drift_run_with(&c, &a, d, 40.0, 0.6, 1, |rig, _, _| {
                let s = slide_state(rig);
                ang.push(-s.body_slip.to_degrees());
                back.push(s.v_fwd < 0.0);
            });
            let m = HoldMetrics::from_angles(&ang, &back);
            println!("| {id} | {name} | {} | {:.0} |", m.cell(), r.max_slip_deg);
        }
        let seeds = [1u64, 2, 3, 4, 5];
        let rs: Vec<_> = seeds
            .iter()
            .map(|s| drift_run(&c, &a, DriftDriver::Sloppy, 40.0, 0.6, *s))
            .collect();
        let spins = rs.iter().filter(|r| r.spun).count();
        let mean_drift = rs.iter().map(|r| r.drift_s).sum::<f32>() / rs.len() as f32;
        println!(
            "| {id} | Sloppy (5 seeds; C metric, either side) | {mean_drift:.2} s mean, {spins}/5 spun | {:.0} |",
            rs.iter().map(|r| r.max_slip_deg).fold(0.0f32, f32::max)
        );
    }
}

fn tap_table(args: &[String]) {
    println!("\n## Tap response: one steering-key tap at speed (clip 2: taps of 0.1–0.25 s)");
    println!(
        "| car | km/h | tap s | peak wheel ° | peak yaw °/s | heading after 1.5 s ° | max slip ° |"
    );
    println!("|---|---|---|---|---|---|---|");
    let a = reference_assists();
    for id in args {
        let c = car(id);
        for kmh in [60.0f32, 100.0, 140.0] {
            for tap in [0.1f32, 0.15, 0.25] {
                let r = tap_response(&c, &a, kmh / 3.6, tap);
                println!(
                    "| {id} | {kmh:.0} | {tap:.2} | {:.1} | {:.1} | {:.1} | {:.1} |",
                    r.peak_wheel_deg, r.peak_yaw_dps, r.heading_deg, r.max_slip_deg
                );
            }
        }
    }
}

fn handbrake_table(args: &[String]) {
    println!("\n## Handbrake pivot: key into the turn + handbrake (throttle off), 0.3 s throttle on, then countersteer key + full throttle");
    println!("| car | km/h | handbrake s | pivot ° (while held) | total ° | max slip ° | catch s | spun |");
    println!("|---|---|---|---|---|---|---|---|");
    let a = reference_assists();
    for id in args {
        let c = car(id);
        for kmh in [40.0f32, 70.0] {
            for hb in [0.3f32, 0.6, 1.0] {
                let r = handbrake_pivot(&c, &a, kmh / 3.6, hb);
                println!(
                    "| {id} | {kmh:.0} | {hb:.1} | {:.0} | {:.0} | {:.0} | {} | {} |",
                    r.pivot_deg,
                    r.total_deg,
                    r.max_slip_deg,
                    r.catch_s.map_or("–".into(), |s| format!("{s:.2}")),
                    if r.spun { "SPUN" } else { "no" }
                );
            }
        }
    }
}

fn hairpin_table(args: &[String]) {
    println!("\n## Hairpin (clip 2): feint 0.3 s, key into the turn + handbrake 1 s, throttle 0.8 s, countersteer + throttle");
    println!(
        "| car | approach km/h | pivot ° (handbrake 1 s) | total ° | catch s | exit km/h | spun |"
    );
    println!("|---|---|---|---|---|---|---|");
    let a = reference_assists();
    for id in args {
        let c = car(id);
        for kmh in [30.0f32, 50.0] {
            let r = hairpin(&c, &a, kmh / 3.6);
            println!(
                "| {id} | {kmh:.0} | {:.0} | {:.0} | {} | {:.0} | {} |",
                r.pivot_deg,
                r.total_deg,
                r.catch_s.map_or("–".into(), |s| format!("{s:.2}")),
                r.exit_kmh,
                if r.spun { "SPUN" } else { "no" }
            );
        }
    }
}

fn trace(id: &str, d: DriftDriver) {
    let c = car(id);
    let a = reference_assists();
    let mut tick = 0usize;
    println!("t s | key | thr | wheel ° | front travel ° | body slip ° | yaw °/s | km/h | gear");
    let r = drift_run_with(&c, &a, d, 40.0, 0.6, 1, |rig, steer, thr| {
        if tick.is_multiple_of(12) {
            let s = slide_state(rig);
            println!(
                "{:.2} | {steer:+.0} | {thr:.0} | {:+.1} | {:+.1} | {:+.1} | {:+.0} | {:.0} | {}",
                tick as f32 / 240.0,
                s.steer.to_degrees(),
                s.front_axle_angle.to_degrees(),
                s.body_slip.to_degrees(),
                s.yaw_rate.to_degrees(),
                s.v_fwd * 3.6,
                rig.vehicle.gear()
            );
        }
        tick += 1;
    });
    println!("{r:?}");
}

const PLAIN_DRIVERS: [(&str, PlainDriver); 4] = [
    (
        "counter key at 3° + full throttle",
        PlainDriver::HoldCounter {
            entry_deg: 3.0,
            throttle: 1.0,
        },
    ),
    (
        "counter key at 6° + full throttle",
        PlainDriver::HoldCounter {
            entry_deg: 6.0,
            throttle: 1.0,
        },
    ),
    (
        "counter key at 12° + full throttle",
        PlainDriver::HoldCounter {
            entry_deg: 12.0,
            throttle: 1.0,
        },
    ),
    (
        "tapper (aim 30°, counter at 6°)",
        PlainDriver::Tapper {
            entry_deg: 6.0,
            target_deg: 30.0,
        },
    ),
];

fn plain_table(args: &[String]) {
    println!("\n## Plain-lab drivers: 40 m circle at 0.6 g, key into the turn + throttle to the entry angle, then 6 s");
    println!("| car | driver | intended side: drift s / band 30–45° s / mean ° sd ° / outcome | key changes /s |");
    println!("|---|---|---|---|");
    let a = reference_assists();
    for id in args {
        let c = car(id);
        for (name, d) in PLAIN_DRIVERS {
            let (_, k) = plain_drift_run(&c, &a, d, 40.0, 0.6);
            let m = plain_hold(&c, &a, d, 40.0, 0.6);
            println!("| {id} | {name} | {} | {k:.1} |", m.cell());
        }
    }
}

/// Diagnostic: counter key at 6° held with part throttle, default rack and instant rack.
fn throttle_diag(args: &[String]) {
    println!("\n## Diagnostic: counter key at 6° held, throttle fixed after the entry — intended-side drift s / band s / mean ° sd ° / outcome");
    println!("| car | rack | 100 % | 70 % | 50 % | 30 % |");
    println!("|---|---|---|---|---|---|");
    let a = reference_assists();
    let d = PlainSteer::default();
    for id in args {
        let c = car(id);
        for (name, p) in [
            ("default", d),
            (
                "instant 0.02 s",
                PlainSteer {
                    t_full_s: 0.02,
                    t_return_s: 0.02,
                    ..d
                },
            ),
        ] {
            plain::set_override(Some(p));
            let cells: Vec<String> = [1.0f32, 0.7, 0.5, 0.3]
                .iter()
                .map(|t| {
                    let dr = PlainDriver::HoldCounter {
                        entry_deg: 6.0,
                        throttle: *t,
                    };
                    plain_hold(&c, &a, dr, 40.0, 0.6).cell()
                })
                .collect();
            println!("| {id} | {name} | {} |", cells.join(" | "));
        }
    }
    plain::set_override(Some(d));
}

/// A/B of the plain constants on the drift drivers (drift s / spun per driver).
fn sweep(args: &[String]) {
    let d = PlainSteer::default();
    let configs = [
        ("default (0.28 / 0.33 s, 75 %)", d),
        (
            "self-align 50 %",
            PlainSteer {
                self_align: 0.5,
                ..d
            },
        ),
        (
            "self-align 100 %",
            PlainSteer {
                self_align: 1.0,
                ..d
            },
        ),
        (
            "rack 0.15 / 0.15 s",
            PlainSteer {
                t_full_s: 0.15,
                t_return_s: 0.15,
                ..d
            },
        ),
        (
            "sensitivity 0 %",
            PlainSteer {
                speed_sensitivity: 0.0,
                ..d
            },
        ),
        (
            "diagnostic: instant rack 0.02 s",
            PlainSteer {
                t_full_s: 0.02,
                t_return_s: 0.02,
                ..d
            },
        ),
    ];
    println!(
        "\n## Sweep: intended-side drift s / band s / mean ° sd ° / outcome, per plain driver"
    );
    println!("| car | constants | counter 3° | counter 6° | counter 12° | tapper |");
    println!("|---|---|---|---|---|---|");
    let a = reference_assists();
    for id in args {
        let c = car(id);
        for (name, p) in configs {
            plain::set_override(Some(p));
            let cells: Vec<String> = PLAIN_DRIVERS
                .iter()
                .map(|(_, dr)| plain_hold(&c, &a, *dr, 40.0, 0.6).cell())
                .collect();
            println!("| {id} | {name} | {} |", cells.join(" | "));
        }
    }
    plain::set_override(Some(d));
}

/// Ride tunings compared in `ride` (`RIDE='{...}'` adds one).
fn ride_configs() -> Vec<(String, RideTune)> {
    let d = RideTune::default();
    let mut v = vec![
        ("data (as is)".to_string(), d),
        ("**lab default**".to_string(), RideTune::lab_default()),
        (
            "bars 0.6, dampers 1.4".to_string(),
            RideTune {
                arb: 0.6,
                damper_bump: 1.4,
                damper_rebound: 1.4,
                ..d
            },
        ),
        (
            "springs 0.8, bars 0.5, dampers 1.5".to_string(),
            RideTune {
                spring: 0.8,
                arb: 0.5,
                damper_bump: 1.5,
                damper_rebound: 1.5,
                ..d
            },
        ),
        (
            "bars 0.6, dampers 1.4 / 1.8, rc 0.10 m".to_string(),
            RideTune {
                arb: 0.6,
                damper_bump: 1.4,
                damper_rebound: 1.8,
                roll_centre_m: 0.1,
                ..d
            },
        ),
    ];
    if let Ok(j) = std::env::var("RIDE") {
        v.push((
            "RIDE env".to_string(),
            serde_json::from_str(&j).expect("RIDE json"),
        ));
    }
    v
}

fn ride_table(args: &[String]) {
    println!("\n## Ride: roll model, roll kick (free response), keyboard lane change 100 km/h and S-bend 70 km/h");
    println!("| car | ride tune | model roll Hz / ζ / °/g | kick roll Hz / ζ | bump travel left F / R mm | lane change: peak roll ° / peak rate °/s / to peak s / rocks / settle s / offset m | S-bend: peak ° / rate °/s / rocks / settle s |");
    println!("|---|---|---|---|---|---|---|");
    let a = reference_assists();
    for id in args {
        let c = car(id);
        for (name, t) in ride_configs() {
            ride_lab::set_override(Some(t));
            let m = ride_lab::ride_model(&c.def, &t);
            let k = roll_kick(&c, 0.15);
            // Bump travel left at static ride height under this tune (front, rear), mm.
            let mut scaled = c.clone();
            for sd in &mut scaled.def.suspension {
                *sd = t.scaled(sd);
            }
            let ar = wp_vehicle::ride::axle_ride(&scaled);
            let lc = roll_run(&c, &a, 100.0 / 3.6, &LANE_CHANGE_KEYS);
            let sb = roll_run(&c, &a, 70.0 / 3.6, &S_BEND_KEYS);
            println!(
                "| {id} | {name} | {:.2} / {:.2} / {:.1} | {:.2} / {:.2} | {:.0} / {:.0} | {:.1} / {:.0} / {:.2} / {} / {:.2} / {:.1} | {:.1} / {:.0} / {} / {:.2} |",
                m.roll_hz,
                m.roll_zeta,
                m.roll_deg_per_g,
                k.natural_hz,
                k.damping_ratio,
                ar[0].bump_travel_m * 1000.0,
                ar[1].bump_travel_m * 1000.0,
                lc.peak_roll_deg,
                lc.peak_roll_rate_dps,
                lc.time_to_peak_s,
                lc.rocks_after,
                lc.settle_s,
                lc.lateral_m,
                sb.peak_roll_deg,
                sb.peak_roll_rate_dps,
                sb.rocks_after,
                sb.settle_s,
            );
        }
    }
    ride_lab::set_override(None);
}

/// Car / tyre knob sets for `carlab` (`CARLAB='{...}'` adds one, or replaces the list with
/// `CARLAB_ONLY=1`).
fn carlab_configs() -> Vec<(String, CarTune)> {
    let d = CarTune::default();
    let mut v: Vec<(String, CarTune)> = vec![
        ("data".into(), d),
        (
            "side grip under spin 0.5".into(),
            CarTune {
                lat_under_spin: 0.5,
                ..d
            },
        ),
        (
            "side grip under spin 0.25".into(),
            CarTune {
                lat_under_spin: 0.25,
                ..d
            },
        ),
        (
            "side grip under spin 0".into(),
            CarTune {
                lat_under_spin: 0.0,
                ..d
            },
        ),
        (
            "rear peak slip × 1.5".into(),
            CarTune {
                rear_peak_slip: 1.5,
                ..d
            },
        ),
        (
            "rear peak slip × 2".into(),
            CarTune {
                rear_peak_slip: 2.0,
                ..d
            },
        ),
        ("lock × 1.3 (≈50°)".into(), CarTune { lock: 1.3, ..d }),
        (
            "rear diff locked".into(),
            CarTune {
                rear_locked: true,
                ..d
            },
        ),
        ("torque × 0.7".into(), CarTune { torque: 0.7, ..d }),
        (
            "yaw damping 0.5 /s (diag)".into(),
            CarTune {
                yaw_damping: 0.5,
                ..d
            },
        ),
        (
            "yaw damping 1.5 /s (diag)".into(),
            CarTune {
                yaw_damping: 1.5,
                ..d
            },
        ),
    ];
    if let Ok(j) = std::env::var("CARLAB") {
        let t: CarTune = serde_json::from_str(&j).expect("CARLAB json");
        if std::env::var("CARLAB_ONLY").is_ok() {
            v = vec![("data".into(), d)];
        }
        v.push((format!("CARLAB {j}"), t));
    }
    v
}

fn carlab_table(args: &[String]) {
    let ids: Vec<String> = if args.is_empty() || args.len() > 3 {
        vec!["parity_rwd_d".into()]
    } else {
        args.to_vec()
    };
    println!("\n## Car / tyre lab: drift hold (6 s, 40 m circle, 2nd) + sanity");
    println!("| car | knobs | counter 6° + full thr: drift s / band s / mean ° sd ° / outcome | counter 12° + full thr | tapper | tap 100 km/h 0.25 s: heading ° / slip ° | lane change: slip ° | handbrake 40 km/h 0.6 s: pivot ° / caught |");
    println!("|---|---|---|---|---|---|---|---|");
    let a = reference_assists();
    for id in &ids {
        let c = car(id);
        for (name, t) in carlab_configs() {
            car_lab::set_override(Some(t));
            let cell = |d: PlainDriver| plain_hold(&c, &a, d, 40.0, 0.6).cell();
            let c6 = cell(PlainDriver::HoldCounter {
                entry_deg: 6.0,
                throttle: 1.0,
            });
            let c12 = cell(PlainDriver::HoldCounter {
                entry_deg: 12.0,
                throttle: 1.0,
            });
            let tp = cell(PlainDriver::Tapper {
                entry_deg: 6.0,
                target_deg: 30.0,
            });
            let tap = tap_response(&c, &a, 100.0 / 3.6, 0.25);
            let lc = wp_vehicle::drift_lab_w::lane_change_slip(&c, &a);
            let hb = handbrake_pivot(&c, &a, 40.0 / 3.6, 0.6);
            println!(
                "| {id} | {name} | {c6} | {c12} | {tp} | {:.1} / {:.1} | {lc:.1} | {:.0} / {} |",
                tap.heading_deg,
                tap.max_slip_deg,
                hb.pivot_deg,
                if hb.spun {
                    "spun"
                } else if hb.catch_s.is_some() {
                    "yes"
                } else {
                    "no"
                }
            );
        }
    }
    car_lab::set_override(None);
}

/// `ptrace [car]`: counter key at 6° + full throttle, every 0.05 s, with tyre forces
/// (`CARLAB='{...}'` applies a car / tyre override).
fn ptrace(id: &str) {
    if let Ok(j) = std::env::var("CARLAB") {
        car_lab::set_override(Some(serde_json::from_str(&j).expect("CARLAB json")));
    }
    let c = car(id);
    let a = reference_assists();
    let mut tick = 0usize;
    println!("t | key | wheel ° | front trav ° | slip ° | yaw °/s | km/h | Fy F kN | Fy R kN | Fx R kN | kappa RL/RR | alpha R °");
    let (r, _) = wp_vehicle::drift_lab_w::plain_drift_run_with(
        &c,
        &a,
        PlainDriver::HoldCounter {
            entry_deg: 6.0,
            throttle: 1.0,
        },
        40.0,
        0.6,
        |rig, steer, _| {
            if tick.is_multiple_of(12) && tick < 480 {
                let s = slide_state(rig);
                let st = rig.state();
                let w = &st.wheel_states;
                println!(
                    "{:.2} | {steer:+.0} | {:+.1} | {:+.1} | {:+.1} | {:+.0} | {:.0} | {:+.1} | {:+.1} | {:+.1} | {:.2}/{:.2} | {:+.1}",
                    tick as f32 / 240.0,
                    s.steer.to_degrees(),
                    s.front_axle_angle.to_degrees(),
                    s.body_slip.to_degrees(),
                    s.yaw_rate.to_degrees(),
                    s.v_fwd * 3.6,
                    (w[0].fy_n + w[1].fy_n) / 1000.0,
                    (w[2].fy_n + w[3].fy_n) / 1000.0,
                    (w[2].fx_n + w[3].fx_n) / 1000.0,
                    w[2].slip_ratio,
                    w[3].slip_ratio,
                    (0.5 * (w[2].slip_angle + w[3].slip_angle)).to_degrees(),
                );
            }
            tick += 1;
        },
    );
    println!("{r:?}");
}

/// Speed sensitivity A/B (lead, clip 8): the lock shrinks with speed (rack) vs full road-wheel
/// lock with the tyres limiting the car (tyre).
fn sens_table(args: &[String]) {
    let ids: Vec<String> = if args.is_empty() {
        [
            "parity_rwd_d",
            "parity_rwd_b",
            "parity_fwd_c",
            "shiro_turbo",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    } else {
        args.to_vec()
    };
    println!("\n## Speed sensitivity: rack (lock × curve) vs tyre (full lock, tyres limit)");
    println!("| car | applies to | km/h | lock % | tap 0.10 s: wheel ° / heading ° / slip ° | tap 0.15 s | tap 0.25 s | key held 1.0 s: heading ° / slip ° |");
    println!("|---|---|---|---|---|---|---|---|");
    let a = reference_assists();
    let base = plain::active().unwrap_or_default();
    for id in &ids {
        let c = car(id);
        for mode in [SensApplies::Rack, SensApplies::Tyre] {
            let p = PlainSteer {
                sens_applies: mode,
                ..base
            };
            plain::set_override(Some(p));
            for kmh in [60.0f32, 100.0, 150.0] {
                let cells: Vec<String> = [0.1f32, 0.15, 0.25, 1.0]
                    .iter()
                    .map(|t| {
                        let r = tap_response(&c, &a, kmh / 3.6, *t);
                        if *t >= 1.0 {
                            format!("{:.1} / {:.1}", r.heading_deg, r.max_slip_deg)
                        } else {
                            format!(
                                "{:.1} / {:.1} / {:.1}",
                                r.peak_wheel_deg, r.heading_deg, r.max_slip_deg
                            )
                        }
                    })
                    .collect();
                println!(
                    "| {id} | {mode:?} | {kmh:.0} | {:.0} | {} |",
                    p.lock_frac(kmh / 3.6) * 100.0,
                    cells.join(" | ")
                );
            }
        }
    }
    println!("\n| car | applies to | lane change 100 km/h max slip ° | counter 6° + full thr: drift s / band s / mean ° sd ° / outcome |");
    println!("|---|---|---|---|");
    for id in &ids {
        let c = car(id);
        for mode in [SensApplies::Rack, SensApplies::Tyre] {
            plain::set_override(Some(PlainSteer {
                sens_applies: mode,
                ..base
            }));
            let lc = wp_vehicle::drift_lab_w::lane_change_slip(&c, &a);
            let m = plain_hold(
                &c,
                &a,
                PlainDriver::HoldCounter {
                    entry_deg: 6.0,
                    throttle: 1.0,
                },
                40.0,
                0.6,
            );
            println!("| {id} | {mode:?} | {lc:.1} | {} |", m.cell());
        }
    }
    plain::set_override(Some(base));
}

/// Velocity-relative steering (reference clip 8) A/B, with the data tyres and the spin model.
fn vr_table(args: &[String]) {
    use wp_vehicle::drift_lab_w::{launch_yaw, plain_hold, HoldMetrics};
    let ids: Vec<String> = if args.is_empty() {
        [
            "parity_rwd_d",
            "parity_rwd_a",
            "parity_rwd_b",
            "rampart_50",
            "shiro_turbo",
            "parity_fwd_c",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    } else {
        args.to_vec()
    };
    let base = plain::active().unwrap_or_default();
    let configs = [
        ("plain, data", false, CarTune::default()),
        ("**VR**, data", true, CarTune::default()),
        ("plain, spin model", false, CarTune::spin_candidate()),
        ("**VR**, spin model", true, CarTune::spin_candidate()),
    ];
    println!("\n## Velocity-relative steering (VR) vs plain, data tyres vs spin model (40 m circle, 2nd, 6 s; intended side: drift s / band s / mean ° sd ° / outcome)");
    println!("| car | setup | counter key + full thr (clip 1) | into-turn key held (clip 8) | keys released mid-drift, throttle held | tap 100 0.15 / 0.25 s: heading ° (slip °) | lane change slip ° | launch yaw ° flat / 1° slope | handbrake 40 km/h 0.6 s: pivot ° / caught |");
    println!("|---|---|---|---|---|---|---|---|---|");
    let a = reference_assists();
    for id in &ids {
        let c = car(id);
        for (name, vr, t) in configs {
            plain::set_override(Some(PlainSteer {
                velocity_relative: vr,
                ..base
            }));
            car_lab::set_override(Some(t));
            let hold = |d: PlainDriver| -> HoldMetrics { plain_hold(&c, &a, d, 40.0, 0.6) };
            let ctr = hold(PlainDriver::HoldCounter {
                entry_deg: 6.0,
                throttle: 1.0,
            });
            let into = hold(PlainDriver::IntoTurnHeld { entry_deg: 12.0 });
            let rel = hold(PlainDriver::Released { entry_deg: 12.0 });
            // Release → spin / swap time (owner: the reference keeps rotating slowly and spins
            // after ~1–2 s).
            let mut tick = 0usize;
            let (mut released, mut failed): (Option<usize>, Option<usize>) = (None, None);
            let mut on_side = false;
            wp_vehicle::drift_lab_w::plain_drift_run_with(
                &c,
                &a,
                PlainDriver::Released { entry_deg: 12.0 },
                40.0,
                0.6,
                |rig, steer, _| {
                    let ss = slide_state(rig);
                    let ang = (-ss.body_slip).to_degrees();
                    if released.is_none() && steer == 0.0 {
                        released = Some(tick);
                    }
                    if ang >= 10.0 {
                        on_side = true;
                    }
                    if released.is_some()
                        && failed.is_none()
                        && (ang.abs() >= 90.0 || ss.v_fwd < 0.0 || (on_side && ang <= -10.0))
                    {
                        failed = Some(tick);
                    }
                    tick += 1;
                },
            );
            let rel_t = match (released, failed) {
                (Some(r), Some(f)) => format!("{:.1} s", (f - r) as f32 / 240.0),
                _ => "none".to_string(),
            };
            let t15 = tap_response(&c, &a, 100.0 / 3.6, 0.15);
            let t25 = tap_response(&c, &a, 100.0 / 3.6, 0.25);
            let lc = wp_vehicle::drift_lab_w::lane_change_slip(&c, &a);
            let l0 = launch_yaw(&c, &a, 0.0);
            let l1 = launch_yaw(&c, &a, 1.0);
            let hb = handbrake_pivot(&c, &a, 40.0 / 3.6, 0.6);
            println!(
                "| {id} | {name} | {} | {} | {} (release → spin/swap {rel_t}) | {:.1} ({:.1}) / {:.1} ({:.1}) | {lc:.1} | {l0:.1} / {l1:.1} | {:.0} / {} |",
                ctr.cell(),
                into.cell(),
                rel.cell(),
                t15.heading_deg,
                t15.max_slip_deg,
                t25.heading_deg,
                t25.max_slip_deg,
                hb.pivot_deg,
                if hb.spun { "spun" } else if hb.catch_s.is_some() { "yes" } else { "no" }
            );
        }
    }
    car_lab::set_override(None);
    plain::set_override(Some(base));
}

/// Weight transfer speed (owner / reference clip 4): ride tunings compared on the clip-4 step
/// (one key held 1.5 s at 80 km/h), the 100 km/h lane change, taps and the drift drivers.
/// `RIDE='{...}'` adds a row.
fn weight_table(args: &[String]) {
    use wp_vehicle::drift_lab_w::{plain_hold, roll_run, step_steer_roll, LANE_CHANGE_KEYS};
    let ids: Vec<String> = if args.is_empty() {
        [
            "parity_rwd_a",
            "parity_rwd_d",
            "rampart_50",
            "parity_rwd_b",
            "shiro_turbo",
            "parity_fwd_c",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    } else {
        args.to_vec()
    };
    let mut configs: Vec<(String, RideTune)> = vec![
        ("data".into(), RideTune::default()),
        ("lab default (step 3)".into(), RideTune::lab_default()),
        ("**slow weight (step 8)**".into(), RideTune::slow_weight()),
    ];
    if let Ok(j) = std::env::var("RIDE") {
        configs.push((
            format!("RIDE {j}"),
            serde_json::from_str(&j).expect("RIDE json"),
        ));
    }
    println!("\n## Weight transfer (clip 4): step = right key held 1.5 s at 80 km/h");
    println!("| car | ride | model roll Hz / ζ / °/g · pitch Hz / ζ | step: roll delay s / 10→90 % rise s / peak roll ° / overshoot % / lat g / heading starts s / max travel | lane change 100: peak roll ° / rate °/s / rocks | tap 100 0.15 s heading ° | counter hold (spin model) |");
    println!("|---|---|---|---|---|---|---|");
    let a = reference_assists();
    let saved_car = car_lab::active();
    car_lab::set_override(Some(CarTune::spin_candidate()));
    for id in &ids {
        for (name, t) in &configs {
            ride_lab::set_override(Some(*t));
            // The body inertia is set when the car's body is built: load after the override.
            let c = car(id);
            let m = ride_lab::ride_model(&c.def, t);
            let st = step_steer_roll(&c, &a, 80.0 / 3.6, 1.5);
            let lc = roll_run(&c, &a, 100.0 / 3.6, &LANE_CHANGE_KEYS);
            let tap = tap_response(&c, &a, 100.0 / 3.6, 0.15);
            let h = plain_hold(
                &c,
                &a,
                PlainDriver::HoldCounter {
                    entry_deg: 6.0,
                    throttle: 1.0,
                },
                40.0,
                0.6,
            );
            println!(
                "| {id} | {name} | {:.2} / {:.2} / {:.1} · {:.2} / {:.2} | {:.2} / {:.2} / {:.1} / {:.0} / {:.2} / {:.2} / {:.2} | {:.1} / {:.0} / {} | {:.1} | {} |",
                m.roll_hz, m.roll_zeta, m.roll_deg_per_g, m.pitch_hz, m.pitch_zeta,
                st.roll_delay_s, st.rise_s, st.peak_roll_deg, st.overshoot_pct, st.peak_lat_g, st.heading_start_s, st.max_travel_frac,
                lc.peak_roll_deg, lc.peak_roll_rate_dps, lc.rocks_after,
                tap.heading_deg,
                h.cell()
            );
        }
    }
    car_lab::set_override(saved_car);
    ride_lab::set_override(None);
}
