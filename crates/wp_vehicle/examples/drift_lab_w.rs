//! Drift Lab W measurement tables (docs/integration/drift-lab-w.md): plain keyboard steering
//! (`wp_vehicle::plain`), TCS / ESC off, manual gears.
//! `cargo run --release -p wp_vehicle --example drift_lab_w [drift|tap|handbrake|hairpin] [car_id ...]`
//! Env: `PLAIN='{"t_full_s":0.2}'` (constants; missing fields = defaults), `PLAIN=off` (the
//! normal steering model with the same assists, for comparison), `RIDE_LAB=1` (the lab's
//! ride tuning default; `ride` sets its own), `RIDE='{...}'` (an extra row in `ride`).

use std::path::PathBuf;

use wp_contracts::LoadedCar;
use wp_vehicle::drift_lab::{
    drift_run, drift_run_with, hairpin, slide_state, tap_response, DriftDriver,
};
use wp_vehicle::drift_lab_w::{
    handbrake_pivot, load_any_car, plain_drift_run, reference_assists, PlainDriver, PARITY_CARS,
    ROSTER_CARS,
};
use wp_vehicle::drift_lab_w::{roll_run, LANE_CHANGE_KEYS, S_BEND_KEYS};
use wp_vehicle::plain::{self, PlainSteer};
use wp_vehicle::ride::roll_kick;
use wp_vehicle::ride_lab::{self, RideTune};

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    load_any_car(&data, id).unwrap()
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
        "| car | driver | drift s (of 6) | longest s | mean ° | sd ° | max ° | flip ° | spun |"
    );
    println!("|---|---|---|---|---|---|---|---|---|");
    let a = reference_assists();
    for id in args {
        let c = car(id);
        for (name, d) in [
            ("Naive (counter key + full throttle)", DriftDriver::Naive),
            ("Good (counter key, throttle modulated)", DriftDriver::Good),
            ("AbruptLift", DriftDriver::AbruptLift),
            ("IntoTurn", DriftDriver::IntoTurn),
        ] {
            let r = drift_run(&c, &a, d, 40.0, 0.6, 1);
            println!(
                "| {id} | {name} | {:.2} | {:.2} | {:.1} | {:.1} | {:.0} | {:.0} | {} |",
                r.drift_s,
                r.longest_s,
                r.mean_deg,
                r.sd_deg,
                r.max_slip_deg,
                r.flip_deg,
                if r.spun { "SPUN" } else { "no" }
            );
        }
        let seeds = [1u64, 2, 3, 4, 5];
        let rs: Vec<_> = seeds
            .iter()
            .map(|s| drift_run(&c, &a, DriftDriver::Sloppy, 40.0, 0.6, *s))
            .collect();
        let spins = rs.iter().filter(|r| r.spun).count();
        let mean_drift = rs.iter().map(|r| r.drift_s).sum::<f32>() / rs.len() as f32;
        println!(
            "| {id} | Sloppy (5 seeds) | {mean_drift:.2} (mean) | – | – | – | {:.0} | – | {spins}/5 |",
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
    println!("| car | driver | drift s (of 6) | longest s | mean ° | sd ° | max ° | key changes /s | spun |");
    println!("|---|---|---|---|---|---|---|---|---|");
    let a = reference_assists();
    for id in args {
        let c = car(id);
        for (name, d) in PLAIN_DRIVERS {
            let (r, k) = plain_drift_run(&c, &a, d, 40.0, 0.6);
            println!(
                "| {id} | {name} | {:.2} | {:.2} | {:.1} | {:.1} | {:.0} | {k:.1} | {} |",
                r.drift_s,
                r.longest_s,
                r.mean_deg,
                r.sd_deg,
                r.max_slip_deg,
                if r.spun { "SPUN" } else { "no" }
            );
        }
    }
}

/// Diagnostic: counter key at 6° held with part throttle, default rack and instant rack.
fn throttle_diag(args: &[String]) {
    println!("\n## Diagnostic: counter key at 6° held, throttle fixed after the entry — drift s (of 6) / spun");
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
                    let (r, _) = plain_drift_run(&c, &a, dr, 40.0, 0.6);
                    format!(
                        "{:.1}{} (mean {:.0}°)",
                        r.drift_s,
                        if r.spun { " SPUN" } else { "" },
                        r.mean_deg
                    )
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
    println!("\n## Sweep: drift s (of 6) / spun, per plain driver");
    println!("| car | constants | counter 3° | counter 6° | counter 12° | tapper |");
    println!("|---|---|---|---|---|---|");
    let a = reference_assists();
    for id in args {
        let c = car(id);
        for (name, p) in configs {
            plain::set_override(Some(p));
            let cells: Vec<String> = PLAIN_DRIVERS
                .iter()
                .map(|(_, dr)| {
                    let (r, _) = plain_drift_run(&c, &a, *dr, 40.0, 0.6);
                    format!("{:.1}{}", r.drift_s, if r.spun { " SPUN" } else { "" })
                })
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
