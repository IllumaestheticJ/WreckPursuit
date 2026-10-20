//! Drift Lab W measurement tables (docs/integration/drift-lab-w.md): plain keyboard steering
//! (`wp_vehicle::plain`), TCS / ESC off, manual gears.
//! `cargo run --release -p wp_vehicle --example drift_lab_w [drift|tap|handbrake|hairpin] [car_id ...]`
//! Env: `PLAIN='{"t_full_s":0.2}'` (constants; missing fields = defaults), `PLAIN=off` (the
//! normal steering model with the same assists, for comparison).

use std::path::PathBuf;

use wp_contracts::LoadedCar;
use wp_vehicle::drift_lab::{drift_run, hairpin, tap_response, DriftDriver};
use wp_vehicle::drift_lab_w::{
    handbrake_pivot, load_any_car, reference_assists, PARITY_CARS, ROSTER_CARS,
};
use wp_vehicle::plain::{self, PlainSteer};

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
    println!("plain steering: {:?}", plain::active());
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let what = if args
        .first()
        .is_some_and(|a| ["drift", "tap", "handbrake", "hairpin"].contains(&a.as_str()))
    {
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
