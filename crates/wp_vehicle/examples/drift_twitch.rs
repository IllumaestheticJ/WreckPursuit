//! Keyboard twitch tables (docs/integration/drift-twitch.md, owner row 102).
//! `cargo run --release -p wp_vehicle --example drift_twitch [car_id ...]`
//! Env: `RAMP=row90` (machine A's keyboard ramp) or `RAMP=up,down[,rev]`, `ASSISTS=off` (TCS/ESC off),
//! `DRIFT_TUNING='{...}'` (constants override, Drift Lab B branches only).

use wp_contracts::{AssistProfile, LoadedCar};
use wp_vehicle::twitch_lab::{twitch_run, Case, KeyRamp};

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap()
}

fn main() {
    if let Ok(j) = std::env::var("DRIFT_TUNING") {
        wp_vehicle::drift::set_tuning_override(Some(
            serde_json::from_str(&j).expect("DRIFT_TUNING json"),
        ));
    }
    // RAMP=main / row90 (a testbed-side ramp on top of the sim's), or RAMP=up,down[,rev].
    let ramp = match std::env::var("RAMP").ok().as_deref() {
        Some("row90") => KeyRamp::ROW90,
        Some(r) if r.contains(',') => {
            let v: Vec<&str> = r.split(',').collect();
            KeyRamp {
                up: v[0].parse().expect("ramp up"),
                down: v[1].parse().expect("ramp down"),
                reverse_from_centre: v.get(2) == Some(&"rev"),
            }
        }
        Some("main") => KeyRamp::MAIN,
        // As shipped (Drift Lab C step 6): raw keys, the sim's key ramp.
        _ => KeyRamp::RAW,
    };
    let mut assists = AssistProfile::keyboard_default();
    if std::env::var("ASSISTS").is_ok_and(|a| a == "off") {
        assists.tcs = wp_contracts::TcsLevel::Off;
        assists.esc = false;
    }
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        args = [
            "shiro_turbo",
            "sperber_32",
            "rampart_50",
            "debug_sedan",
            "junkyard_beater",
        ]
        .map(String::from)
        .to_vec();
    }
    println!("| case | car | reversals/s | peak rack °/s | fast slew not sliding % | yaw overshoot °/s | settle s | rms err m | max rear slip ° |");
    println!("|---|---|---|---|---|---|---|---|---|");
    let mut sums = vec![[0.0f32; 7]; Case::ALL.len()];
    for id in &args {
        let c = car(id);
        for (i, case) in Case::ALL.iter().enumerate() {
            let t = twitch_run(&c, &assists, ramp, *case);
            let settle = t.settle_s.map_or("—".to_string(), |s| format!("{s:.2}"));
            println!(
                "| {} | {id} | {:.2} | {:.0} | {:.1} | {:.1} | {settle} | {:.2} | {:.1} |",
                case.name(),
                t.reversals_ps,
                t.peak_rate_dps,
                t.fast_not_sliding_pct,
                t.yaw_overshoot_dps,
                t.rms_err_m,
                t.max_rear_slip_deg
            );
            let s = &mut sums[i];
            s[0] += t.reversals_ps;
            s[1] += t.peak_rate_dps;
            s[2] += t.fast_not_sliding_pct;
            s[3] += t.yaw_overshoot_dps;
            if let Some(x) = t.settle_s {
                s[4] += x;
                s[6] += 1.0;
            }
            s[5] += t.rms_err_m;
        }
    }
    let n = args.len() as f32;
    println!("\n**Mean of cars**\n");
    println!("| case | reversals/s | peak rack °/s | fast slew not sliding % | yaw overshoot °/s | settle s (mean, n settled) | rms err m |");
    println!("|---|---|---|---|---|---|---|");
    for (i, case) in Case::ALL.iter().enumerate() {
        let s = sums[i];
        let settle = if s[6] == 0.0 {
            "—".to_string()
        } else {
            format!("{:.2} ({:.0}/{n})", s[4] / s[6], s[6])
        };
        println!(
            "| {} | {:.2} | {:.0} | {:.1} | {:.1} | {settle} | {:.2} |",
            case.name(),
            s[0] / n,
            s[1] / n,
            s[2] / n,
            s[3] / n,
            s[5] / n
        );
    }
}
