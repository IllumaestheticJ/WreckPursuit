//! Drift Lab C measurement tables (docs/integration/drift-lab-c.md; B's are in drift-lab-b.md).
//! `cargo run --release -p wp_vehicle --example drift_lab_c [catch|drift] [car_id ...]`
//! Env: `GAMEPAD=1` (gamepad preset), `AUTO_GEARS=1` (automatic box; default manual, the
//! owner's reference), `DRIFT_TUNING='{...}'` (constants override).

use wp_contracts::{AssistProfile, LoadedCar};
use wp_vehicle::drift_lab::{
    brake_tap, drift_run, fwd_corner, hairpin, handbrake_entry, handbrake_stab, kick_catch,
    tap_response, transition_run, CatchDriver, DriftDriver,
};

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap()
}

/// Keyboard preset (GAMEPAD=1: gamepad preset), TCS / ESC off, manual gears, countersteer
/// `help`.
fn kb(help: f32) -> AssistProfile {
    let base = if std::env::var("GAMEPAD").is_ok() {
        AssistProfile::gamepad_default()
    } else {
        AssistProfile::keyboard_default()
    };
    // The owner's reference (Wreckfest: TCS off, ESC off, manual shifting): manual gears, the
    // driver holds the gear the automatic box picked while settling. AUTO_GEARS=1: automatic.
    AssistProfile {
        countersteer_help: help,
        tcs: wp_contracts::TcsLevel::Off,
        esc: false,
        auto_gears: std::env::var("AUTO_GEARS").is_ok(),
        ..base
    }
}

fn main() {
    // Lab tool: `None` overrides mean the car data, as when this was written (pre-unify;
    // `wp_vehicle::lab_scope`).
    wp_vehicle::lab_scope::set_legacy(true);
    // DRIFT_TUNING='{"hold_gain":0.8}' overrides constants (missing fields = defaults).
    if let Ok(j) = std::env::var("DRIFT_TUNING") {
        wp_vehicle::drift::set_tuning_override(Some(
            serde_json::from_str(&j).expect("DRIFT_TUNING json"),
        ));
    }
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let what = if args.first().is_some_and(|a| {
        [
            "catch",
            "drift",
            "transition",
            "handbrake",
            "tap",
            "hairpin",
            "clips",
        ]
        .contains(&a.as_str())
    }) {
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
    if what == "all" || what == "clips" {
        clips_tables();
    }
    if what == "all" || what == "tap" {
        tap_table(&args);
    }
    if what == "all" || what == "hairpin" {
        hairpin_table(&args);
    }
    if what == "all" || what == "handbrake" {
        handbrake_table(&args);
    }
    if what == "all" || what == "transition" {
        transition_table(&args);
    }
    if ["transition", "handbrake", "tap", "hairpin", "clips"].contains(&what.as_str()) {
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

fn handbrake_table(args: &[String]) {
    println!("\n## Handbrake entry (row 108): key into the turn + handbrake tap, then countersteer key (or keys released) + throttle");
    println!("| car | km/h | tap s | counter key: release→aligned ms (30 / 60 / 100 %) | keys released: release→aligned ms (30 / 60 / 100 %) | caught counter (30/60/100) | caught released (30/60/100) |");
    println!("|---|---|---|---|---|---|---|");
    let helps = [0.3f32, 0.6, 1.0];
    let mut caught = [[0usize; 3]; 2];
    let mut aligned = [[(0.0f32, 0usize); 3]; 2];
    let mut n = 0usize;
    for id in args {
        let c = car(id);
        for kmh in [50.0f32, 80.0] {
            for tap in [0.3f32, 0.45, 0.6] {
                n += 1;
                let mut cells = Vec::new();
                let mut caughts = Vec::new();
                for (k, counter) in [true, false].into_iter().enumerate() {
                    let rs: Vec<_> = helps
                        .iter()
                        .map(|h| handbrake_entry(&c, &kb(*h), kmh / 3.6, tap, counter))
                        .collect();
                    let mut al = Vec::new();
                    let mut cs = Vec::new();
                    for (i, r) in rs.iter().enumerate() {
                        caught[k][i] += r.caught as usize;
                        match r.release_to_aligned_s {
                            Some(t) => {
                                aligned[k][i].0 += t * 1000.0;
                                aligned[k][i].1 += 1;
                                al.push(format!("{:.0}", t * 1000.0));
                            }
                            None => al.push("–".into()),
                        }
                        cs.push(if r.caught { "y" } else { "SPUN" });
                    }
                    cells.push(al.join(" / "));
                    caughts.push(cs.join(" / "));
                }
                println!(
                    "| {id} | {kmh:.0} | {tap:.2} | {} | {} | {} | {} |",
                    cells[0], cells[1], caughts[0], caughts[1]
                );
            }
        }
    }
    for (k, name) in ["counter key", "keys released"].into_iter().enumerate() {
        for (i, h) in [30, 60, 100].into_iter().enumerate() {
            let (sum, m) = aligned[k][i];
            println!(
                "\n{name} {h} %: caught {}/{n}, release→aligned mean {:.0} ms ({m} aligned)",
                caught[k][i],
                if m > 0 { sum / m as f32 } else { 0.0 }
            );
        }
    }
}

fn tap_table(args: &[String]) {
    println!("\n## Tap response (clip 2): one steering-key tap at speed, countersteer 60 %");
    println!(
        "| car | km/h | tap s | peak wheel ° | peak yaw °/s | heading after 1.5 s ° | max slip ° |"
    );
    println!("|---|---|---|---|---|---|---|");
    for id in args {
        let c = car(id);
        for kmh in [60.0f32, 100.0, 140.0] {
            for tap in [0.1f32, 0.2, 0.4] {
                let r = tap_response(&c, &kb(0.6), kmh / 3.6, tap);
                println!(
                    "| {id} | {kmh:.0} | {tap:.1} | {:.1} | {:.1} | {:.1} | {:.1} |",
                    r.peak_wheel_deg, r.peak_yaw_dps, r.heading_deg, r.max_slip_deg
                );
            }
        }
    }
}

fn hairpin_table(args: &[String]) {
    println!("\n## Handbrake hairpin (clip 2): feint, key into the turn + handbrake 1 s, throttle, countersteer + throttle");
    println!("| car | approach km/h | help | pivot ° (handbrake 1 s) | total ° | catch s | exit km/h | spun |");
    println!("|---|---|---|---|---|---|---|---|");
    for id in args {
        let c = car(id);
        for kmh in [30.0f32, 50.0] {
            for help in [0.3f32, 0.6, 1.0] {
                let r = hairpin(&c, &kb(help), kmh / 3.6);
                println!(
                    "| {id} | {kmh:.0} | {:.0} % | {:.0} | {:.0} | {} | {:.0} | {} |",
                    help * 100.0,
                    r.pivot_deg,
                    r.total_deg,
                    r.catch_s.map_or("–".to_string(), |t| format!("{t:.2}")),
                    r.exit_kmh,
                    if r.spun { "SPUN" } else { "no" }
                );
            }
        }
    }
}

/// Owner clips 3 and 5 (fixed car lists: front-drive for clip 3, rear-drive for clip 5).
fn clips_tables() {
    println!("\n## Front-drive corner (clip 3): brake 1.3 s from 160 km/h, key into the turn + handbrake 0.3 s, full throttle + 0.35 s opposite tap");
    println!(
        "| car | help | turn-in km/h | peak slip ° | throttle → straight s | turned ° | spun |"
    );
    println!("|---|---|---|---|---|---|---|");
    for id in ["kestrel_ts", "debug_sedan", "junkyard_beater"] {
        let c = car(id);
        for help in [0.3f32, 0.6, 1.0] {
            let r = fwd_corner(&c, &kb(help), 160.0 / 3.6);
            println!(
                "| {id} | {:.0} % | {:.0} | {:.0} | {} | {:.0} | {} |",
                help * 100.0,
                r.turn_in_kmh,
                r.peak_slip_deg,
                r.straight_s.map_or("–".to_string(), |t| format!("{t:.2}")),
                r.turned_deg,
                if r.spun { "SPUN" } else { "no" }
            );
        }
    }
    println!("\n## Handbrake stabs on the throttle (clip 5): 80 km/h corner (R84 at 0.6 g), 3 × 0.2 s stabs, then countersteer + throttle");
    println!("| car | help | slide before ° | max slide in stabs ° | drive back after a stab s (worst) | drift s after (of 3) | spun |");
    println!("|---|---|---|---|---|---|---|");
    for id in ["shiro_turbo", "sperber_32", "rampart_50"] {
        let c = car(id);
        for help in [0.3f32, 0.6, 1.0] {
            let r = handbrake_stab(&c, &kb(help), 84.0, 0.6);
            println!(
                "| {id} | {:.0} % | {:.0} | {:.0} | {:.2} | {:.2} | {} |",
                help * 100.0,
                r.slide_before_deg,
                r.slide_after_deg,
                r.drive_back_s,
                r.drift_s,
                if r.spun { "SPUN" } else { "no" }
            );
        }
    }
    println!("\n## Braking with a steering tap (clip 5): full brake from 130 km/h, a left tap 0.2 s in, 60 %");
    println!("| car | tap s | peak yaw °/s | max slip ° | heading after 2 s ° | spun |");
    println!("|---|---|---|---|---|---|");
    for id in ["shiro_turbo", "rampart_50", "kestrel_ts", "debug_sedan"] {
        let c = car(id);
        for tap in [0.15f32, 0.3] {
            let r = brake_tap(&c, &kb(0.6), 130.0 / 3.6, tap);
            println!(
                "| {id} | {tap:.2} | {:.1} | {:.1} | {:.1} | {} |",
                r.peak_yaw_dps,
                r.max_slip_deg,
                r.heading_deg,
                if r.spun { "SPUN" } else { "no" }
            );
        }
    }
}
