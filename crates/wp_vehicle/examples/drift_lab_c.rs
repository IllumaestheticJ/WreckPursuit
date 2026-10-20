//! Drift Lab C measurement tables (docs/integration/drift-lab-c.md; B's are in drift-lab-b.md).
//! `cargo run --release -p wp_vehicle --example drift_lab_c [catch|drift] [car_id ...]`
//! Env: `GAMEPAD=1` (gamepad preset), `DRIFT_TUNING='{...}'` (constants override).

use wp_contracts::{AssistProfile, LoadedCar};
use wp_vehicle::drift_lab::{drift_run, kick_catch, transition_run, CatchDriver, DriftDriver};

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap()
}

/// Keyboard preset (GAMEPAD=1: gamepad preset), TCS / ESC off, countersteer `help`.
fn kb(help: f32) -> AssistProfile {
    let base = if std::env::var("GAMEPAD").is_ok() {
        AssistProfile::gamepad_default()
    } else {
        AssistProfile::keyboard_default()
    };
    AssistProfile {
        countersteer_help: help,
        tcs: false,
        esc: false,
        ..base
    }
}

fn main() {
    // DRIFT_TUNING='{"hold_gain":0.8}' overrides constants (missing fields = defaults).
    if let Ok(j) = std::env::var("DRIFT_TUNING") {
        wp_vehicle::drift::set_tuning_override(Some(
            serde_json::from_str(&j).expect("DRIFT_TUNING json"),
        ));
    }
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let what = if args
        .first()
        .is_some_and(|a| a == "catch" || a == "drift" || a == "transition")
    {
        args.remove(0)
    } else {
        "all".into()
    };
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
    if what == "all" || what == "transition" {
        transition_table(&args);
    }
    if what == "transition" {
        return;
    }
    if what != "drift" {
        println!("## Catch (keyboard, TCS/ESC off, countersteer 60 %)");
        println!("| car | km/h | CS slew °/s (late key) | onset→aligned ms (released, caught runs: mean, n aligned) | peak align err ° (caught) | released caught | late-key caught |");
        println!("|---|---|---|---|---|---|---|");
        for id in &args {
            let c = car(id);
            for kmh in [60.0f32, 100.0, 140.0] {
                let (mut rel_ok, mut late_ok, mut slew, mut align, mut perr) =
                    (0, 0, 0.0f32, Vec::new(), 0.0f32);
                let kicks = [0.8f32, 1.2, 1.6, 2.0, -1.2, -2.0];
                for k in kicks {
                    let r = kick_catch(&c, &kb(0.6), CatchDriver::Released, kmh / 3.6, k);
                    rel_ok += r.caught as usize;
                    if r.caught {
                        perr = perr.max(r.peak_align_err_deg);
                        if let Some(t) = r.onset_to_aligned_s {
                            align.push(t * 1000.0);
                        }
                    }
                    let l = kick_catch(
                        &c,
                        &kb(0.6),
                        CatchDriver::Counter { react_s: 0.15 },
                        kmh / 3.6,
                        k,
                    );
                    late_ok += l.caught as usize;
                    slew = slew.max(l.cs_slew_dps);
                }
                let mean = if align.is_empty() {
                    "never".to_string()
                } else {
                    format!(
                        "{:.0} ({}/{})",
                        align.iter().sum::<f32>() / align.len() as f32,
                        align.len(),
                        kicks.len()
                    )
                };
                println!(
                    "| {id} | {kmh:.0} | {slew:.0} | {mean} | {perr:.1} | {rel_ok}/{n} | {late_ok}/{n} |",
                    n = kicks.len()
                );
            }
        }
    }
    if what != "catch" {
        drift_tables(&args);
    }
}

const DRIVERS: [DriftDriver; 7] = [
    DriftDriver::Good,
    DriftDriver::Naive,
    DriftDriver::IntoTurn,
    DriftDriver::Sloppy,
    DriftDriver::AbruptLift,
    DriftDriver::Flood,
    DriftDriver::FlickLift,
];
const RWD: [&str; 3] = ["shiro_turbo", "sperber_32", "rampart_50"];

/// One row: (drift s mean, longest s max, max slip, snap, mean °, sd °, spins, runs).
type Row = (f32, f32, f32, f32, f32, f32, usize, usize);

fn drift_tables(args: &[String]) {
    let helps = [0.3f32, 0.6, 1.0];
    let mut rows: Vec<(String, f32, DriftDriver, Row)> = Vec::new();
    for id in args {
        let c = car(id);
        for help in helps {
            for d in DRIVERS {
                let seeds: Vec<u64> = if d == DriftDriver::Sloppy {
                    (1..=20).collect()
                } else {
                    vec![1]
                };
                let mut r: Row = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0, seeds.len());
                for s in &seeds {
                    let x = drift_run(&c, &kb(help), d, 40.0, 0.6, *s);
                    r.0 += x.drift_s / seeds.len() as f32;
                    r.1 = r.1.max(x.longest_s);
                    r.2 = r.2.max(x.max_slip_deg);
                    r.3 = r.3.max(x.flip_deg);
                    r.4 += x.mean_deg / seeds.len() as f32;
                    r.5 += x.sd_deg / seeds.len() as f32;
                    r.6 += x.spun as usize;
                }
                rows.push((id.clone(), help, d, r));
            }
        }
    }
    let rwd: Vec<&str> = RWD
        .iter()
        .copied()
        .filter(|id| args.iter().any(|a| a == id))
        .collect();
    if !rwd.is_empty() {
        println!(
            "\n## RWD summary ({}) — 40 m left-hander at 0.6 g, 6 s",
            rwd.join(", ")
        );
        println!("| slider | driver | drift s (mean of cars) | longest s (best) | mean angle ° | sd ° (steadiness) | spins |");
        println!("|---|---|---|---|---|---|---|");
        for help in helps {
            for d in DRIVERS {
                let sel: Vec<&Row> = rows
                    .iter()
                    .filter(|(id, h, dd, _)| rwd.contains(&id.as_str()) && *h == help && *dd == d)
                    .map(|(_, _, _, r)| r)
                    .collect();
                let n = sel.len() as f32;
                println!(
                    "| {:.0} % | {d:?} | {:.2} | {:.2} | {:.0} | {:.1} | {}/{} |",
                    help * 100.0,
                    sel.iter().map(|r| r.0).sum::<f32>() / n,
                    sel.iter().map(|r| r.1).fold(0.0, f32::max),
                    sel.iter().map(|r| r.4).sum::<f32>() / n,
                    sel.iter().map(|r| r.5).sum::<f32>() / n,
                    sel.iter().map(|r| r.6).sum::<usize>(),
                    sel.iter().map(|r| r.7).sum::<usize>(),
                );
            }
        }
    }
    println!("\n## Drift drivers (40 m left-hander at 0.6 g, 6 s, keyboard TCS/ESC off)");
    println!("| car | help | driver | drift s (mean) | longest s | max slip ° | snap ° | mean ° | sd ° | spins |");
    println!("|---|---|---|---|---|---|---|---|---|---|");
    for (id, help, d, r) in &rows {
        println!(
            "| {id} | {:.0} % | {d:?} | {:.2} | {:.2} | {:.0} | {:.0} | {:.0} | {:.1} | {}/{} |",
            help * 100.0,
            r.0,
            r.1,
            r.2,
            r.3,
            r.4,
            r.5,
            r.6,
            r.7
        );
    }
}

fn transition_table(args: &[String]) {
    println!("\n## Lift-off transition (row 107): left drift 2.5 s, lift + release, right drift (S-bend, 40 m at 0.6 g)");
    println!("| car | help | transition s | peak new-side slide ° | peak yaw rad/s | wobbles | new drift s | spun |");
    println!("|---|---|---|---|---|---|---|---|");
    let mut tot = [(0usize, 0usize, 0u32, 0.0f32); 3];
    for id in args {
        let c = car(id);
        for (i, help) in [0.3f32, 0.6, 1.0].into_iter().enumerate() {
            let r = transition_run(&c, &kb(help), 40.0, 0.6);
            println!(
                "| {id} | {:.0} % | {} | {:.0} | {:.2} | {} | {:.2} | {} |",
                help * 100.0,
                r.transition_s
                    .map_or("never".to_string(), |t| format!("{t:.2}")),
                r.peak_new_deg,
                r.peak_yaw_rps,
                r.wobbles,
                r.new_drift_s,
                if r.spun { "spun" } else { "no" }
            );
            tot[i].0 += 1;
            tot[i].1 += r.spun as usize;
            tot[i].2 += r.wobbles;
            tot[i].3 += r.new_drift_s;
        }
    }
    for (i, help) in [30, 60, 100].into_iter().enumerate() {
        let (n, sp, w, d) = tot[i];
        println!(
            "\n{help} %: spins {sp}/{n}, wobbles {w}, new drift {:.2} s mean",
            d / n as f32
        );
    }
}
