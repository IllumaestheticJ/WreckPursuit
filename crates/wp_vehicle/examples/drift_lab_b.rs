//! Drift Lab B measurement tables (docs/integration/drift-lab-b.md).
//! `cargo run --release -p wp_vehicle --example drift_lab_b [catch|drift] [car_id ...]`
//! Env: `GAMEPAD=1` (gamepad preset), `DRIFT_TUNING='{...}'` (constants override).

use wp_contracts::{AssistProfile, LoadedCar};
use wp_vehicle::drift_lab::{drift_run, kick_catch, CatchDriver, DriftDriver};

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
    let what = if args.first().is_some_and(|a| a == "catch" || a == "drift") {
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
        println!("\n## Drift drivers (40 m left-hander at 0.6 g, 6 s, keyboard TCS/ESC off)");
        println!(
            "| car | help | driver | drift s (mean) | longest s | max slip ° | snap ° | spins |"
        );
        println!("|---|---|---|---|---|---|---|---|");
        for id in &args {
            let c = car(id);
            for help in [0.3f32, 0.6, 1.0] {
                for d in [
                    DriftDriver::Good,
                    DriftDriver::Naive,
                    DriftDriver::Sloppy,
                    DriftDriver::AbruptLift,
                    DriftDriver::Flood,
                    DriftDriver::FlickLift,
                ] {
                    let seeds: Vec<u64> = if d == DriftDriver::Sloppy {
                        (1..=20).collect()
                    } else {
                        vec![1]
                    };
                    let (mut t, mut l, mut spins, mut ms, mut fl) =
                        (0.0f32, 0.0f32, 0, 0.0f32, 0.0f32);
                    for s in &seeds {
                        let r = drift_run(&c, &kb(help), d, 40.0, 0.6, *s);
                        t += r.drift_s;
                        l = l.max(r.longest_s);
                        spins += r.spun as usize;
                        ms = ms.max(r.max_slip_deg);
                        fl = fl.max(r.flip_deg);
                    }
                    println!(
                        "| {id} | {:.0} % | {d:?} | {:.2} | {l:.2} | {ms:.0} | {fl:.0} | {spins}/{} |",
                        help * 100.0,
                        t / seeds.len() as f32,
                        seeds.len()
                    );
                }
            }
        }
    }
}
