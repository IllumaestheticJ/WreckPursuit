//! Tyre-slip workstream measurements (docs/integration/tyre-slip.md): the combined-slip model
//! A/B (`wp_vehicle::car_lab::CarTune::spin_model`, `wp_vehicle::tire::SpinGrip`) under Drift
//! Lab W's plain keyboard steering, TCS / ESC off, manual gears, lab ride default.
//!
//! `cargo run --release -p wp_vehicle --example tyre_slip -- [table|grid|trace] [car ...]`
//! - `table`: hold / tapper / taps / lane change / handbrake for each model config.
//! - `grid`: a sweep of the model's knobs on the held countersteer only (ranked).
//! - `trace`: counter key at 6° + full throttle every 0.05 s with tyre forces.
//!
//! Env: `TYRE='{...}'` adds a `CarTune` row (`TYRE_ONLY=1`: only data + that row),
//! `RIDE=data` (the car's own ride instead of the lab default).

use std::path::PathBuf;

use wp_contracts::LoadedCar;
use wp_vehicle::car_lab::{self, CarTune};
use wp_vehicle::drift_lab::{slide_state, tap_response};
use wp_vehicle::drift_lab_w::{
    handbrake_pivot, lane_change_slip, load_any_car, plain_drift_run_with, reference_assists,
    HoldMetrics, PlainDriver,
};
use wp_vehicle::plain::{self, PlainSteer};
use wp_vehicle::ride_lab::{self, RideTune};

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    load_any_car(&data, id).unwrap()
}

/// One held-countersteer (or tapper) run: Drift Lab W's shared [`HoldMetrics`] (intended
/// side, 10–50° drift, 30–45° band, side swap = fail) plus steadiness over the last 3 s and
/// the speeds.
#[derive(Debug, Clone, Copy)]
struct Hold {
    m: HoldMetrics,
    /// Ticks with the slide in 10–50° on the intended side, s (= `m.drift_s`).
    drift_s: f32,
    /// Time in the 30–45° band (clip 1), s (= `m.band_s`).
    band_s: f32,
    /// Mean / sd of the slide angle over the last 3 s (NaN if spun or swapped), degrees.
    late_mean: f32,
    late_sd: f32,
    /// Spun or swapped sides (both fail the hold).
    spun: bool,
    /// km/h at entry and at the end.
    v0: f32,
    v1: f32,
}

fn hold(c: &LoadedCar, d: PlainDriver) -> Hold {
    hold_at(c, d, 0.6)
}

/// [`hold`] entering from the 40 m circle at `lat_g` (0.3–0.7 g ≈ 39–60 km/h).
fn hold_at(c: &LoadedCar, d: PlainDriver, lat_g: f32) -> Hold {
    let a = reference_assists();
    let (mut angles, mut back) = (Vec::with_capacity(1440), Vec::with_capacity(1440));
    let mut v0 = None;
    let mut v1 = 0.0;
    plain_drift_run_with(c, &a, d, 40.0, lat_g, |rig, _, _| {
        let s = slide_state(rig);
        angles.push((-s.body_slip).to_degrees());
        back.push(s.v_fwd < 0.0);
        let v = s.v_fwd * 3.6;
        v0.get_or_insert(v);
        v1 = v;
    });
    let m = HoldMetrics::from_angles(&angles, &back);
    let fail = m.spun || m.side_swaps > 0;
    let (lm, ls) = stats(&angles[angles.len() - 3 * 240..]);
    Hold {
        m,
        drift_s: m.drift_s,
        band_s: m.band_s,
        late_mean: if fail { f32::NAN } else { lm },
        late_sd: if fail { f32::NAN } else { ls },
        spun: fail,
        v0: v0.unwrap_or(0.0),
        v1,
    }
}

fn stats(x: &[f32]) -> (f32, f32) {
    let n = x.len().max(1) as f32;
    let m = x.iter().sum::<f32>() / n;
    let v = x.iter().map(|a| (a - m) * (a - m)).sum::<f32>() / n;
    (m, v.sqrt())
}

fn fmt_hold(h: &Hold) -> String {
    if h.spun {
        h.m.cell()
    } else {
        format!(
            "{} / last 3 s {:.0}° sd {:.1} / {:.0}→{:.0} km/h",
            h.m.cell(),
            h.late_mean,
            h.late_sd,
            h.v0,
            h.v1
        )
    }
}

const COUNTER6: PlainDriver = PlainDriver::HoldCounter {
    entry_deg: 6.0,
    throttle: 1.0,
};
const COUNTER12: PlainDriver = PlainDriver::HoldCounter {
    entry_deg: 12.0,
    throttle: 1.0,
};
const TAPPER: PlainDriver = PlainDriver::Tapper {
    entry_deg: 6.0,
    target_deg: 30.0,
};

/// The spin-weighted model with the shipped [`CarTune`] defaults for its knobs.
fn spin() -> CarTune {
    CarTune {
        spin_model: true,
        ..CarTune::default()
    }
}

fn configs() -> Vec<(String, CarTune)> {
    let d = CarTune::default();
    let mut v: Vec<(String, CarTune)> = vec![
        ("data (friction ellipse)".into(), d),
        ("spin model (`CarTune::spin_candidate`)".into(), spin()),
        (
            "step-4 drift candidate (side grip 0.15, lock 0.9, damper 0.2)".into(),
            CarTune::spin_grip_damper(),
        ),
        (
            "rear diff open only".into(),
            CarTune {
                rear_open: true,
                ..d
            },
        ),
        (
            "aligning moment on body only".into(),
            CarTune {
                aligning_on_body: true,
                ..d
            },
        ),
        (
            "rear relaxation × 3 only".into(),
            CarTune {
                rear_relax: 3.0,
                ..d
            },
        ),
        (
            "rear relaxation × 0.3 only".into(),
            CarTune {
                rear_relax: 0.3,
                ..d
            },
        ),
    ];
    if let Ok(j) = std::env::var("TYRE") {
        let t: CarTune = serde_json::from_str(&j).expect("TYRE json");
        if std::env::var("TYRE_ONLY").is_ok() {
            v = vec![("data (friction ellipse)".into(), d)];
        }
        v.push((format!("TYRE {j}"), t));
    }
    v
}

fn table(ids: &[String]) {
    let a = reference_assists();
    println!("\n## Held countersteer (6 s, 40 m circle, 2nd) + grip sanity");
    println!("| car | model | counter 6° + full thr | counter 12° + full thr | tapper 30° | taps heading ° 0.1/0.2/0.4 s @60 · @100 · @140 km/h | lane change slip ° | handbrake 40 km/h 0.6 s: pivot ° / caught |");
    println!("|---|---|---|---|---|---|---|---|");
    for id in ids {
        let c = car(id);
        for (name, t) in configs() {
            car_lab::set_override(Some(t));
            let h6 = hold(&c, COUNTER6);
            let h12 = hold(&c, COUNTER12);
            let tp = hold(&c, TAPPER);
            let mut taps = Vec::new();
            for kmh in [60.0f32, 100.0, 140.0] {
                let t: Vec<String> = [0.1f32, 0.2, 0.4]
                    .iter()
                    .map(|&s| format!("{:.1}", tap_response(&c, &a, kmh / 3.6, s).heading_deg))
                    .collect();
                taps.push(t.join("/"));
            }
            let lc = lane_change_slip(&c, &a);
            let hb = handbrake_pivot(&c, &a, 40.0 / 3.6, 0.6);
            println!(
                "| {id} | {name} | {} | {} | {} | {} | {lc:.1} | {:.0} / {} |",
                fmt_hold(&h6),
                fmt_hold(&h12),
                fmt_hold(&tp),
                taps.join(" · "),
                hb.pivot_deg,
                if hb.spun {
                    "spun".to_string()
                } else {
                    hb.catch_s.map_or("no".into(), |s| format!("{s:.2} s"))
                }
            );
        }
    }
    car_lab::set_override(None);
}

/// Score of a held countersteer for the grid: time in the 30–45° band, minus unsteadiness.
fn score(h: &Hold) -> f32 {
    if h.spun {
        return h.drift_s - 10.0;
    }
    h.band_s - 0.1 * h.late_sd
}

fn grid(ids: &[String]) {
    let id = ids.first().map_or("parity_rwd_d", String::as_str);
    let c = car(id);
    let base = spin();
    let mut rows = Vec::new();
    let list = |k: &str, d: &[f32]| -> Vec<f32> {
        std::env::var(k).map_or(d.to_vec(), |v| {
            v.split(',').map(|x| x.parse().expect("number")).collect()
        })
    };
    for b0 in list("B0", &[2.0, 4.3, 8.0, 16.0]) {
        for b2 in list("B2", &[2.0, 3.0, 4.0, 5.6, 8.0]) {
            for p in list("P", &[2.0, 4.0]) {
                for cc in list("C", &[1.0]) {
                    for y in list("Y", &[0.0, 1.0]) {
                        for fs in list("FS", &[0.0]) {
                            let t = CarTune {
                                spin_fx_slide: fs,
                                spin_b0: b0,
                                spin_b2: b2,
                                spin_p: p,
                                spin_c: cc,
                                spin_fx_yield: y,
                                ..base
                            };
                            car_lab::set_override(Some(t));
                            let h6 = hold(&c, COUNTER6);
                            let h12 = hold(&c, COUNTER12);
                            rows.push((score(&h6) + score(&h12), [b0, b2, p, fs, y], h6, h12));
                        }
                    }
                }
            }
        }
    }
    car_lab::set_override(None);
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!("| score | b0 / b2 / p / fx_slide / fx_yield | counter 6° | counter 12° |");
    println!("|---|---|---|---|");
    for (s, k, h6, h12) in rows.iter().take(25) {
        println!(
            "| {s:.2} | {} / {} / {} / {} / {} | {} | {} |",
            k[0],
            k[1],
            k[2],
            k[3],
            k[4],
            fmt_hold(h6),
            fmt_hold(h12)
        );
    }
}

/// Robust score over several entries: the worst case counts most.
fn robust(c: &LoadedCar) -> (f32, Vec<Hold>) {
    let mut v = Vec::new();
    for g in [0.5f32, 0.6, 0.7] {
        v.push(hold_at(c, COUNTER6, g));
        v.push(hold_at(c, COUNTER12, g));
    }
    let sc: Vec<f32> = v
        .iter()
        .map(|h| {
            if h.spun {
                -5.0 + 0.5 * h.drift_s
            } else if h.drift_s < 1.0 {
                -6.0
            } else {
                let off = if (28.0..=47.0).contains(&h.late_mean) {
                    0.0
                } else {
                    2.0
                };
                h.band_s - 0.15 * (h.late_sd - 3.0).max(0.0) - off
            }
        })
        .collect();
    let min = sc.iter().copied().fold(f32::INFINITY, f32::min);
    let mean = sc.iter().sum::<f32>() / sc.len() as f32;
    (min + 0.5 * mean, v)
}

/// `search [n]`: seeded random search over the spin model's knobs, ranked by [`robust`].
fn search(n: usize) {
    let c = car("parity_rwd_d");
    // SEARCH_DAMP=1: also sample the diagnostic yaw damper (penalised), to find how much
    // non-physical damping each tyre set still needs. SEARCH_W=1: sample the ellipse model with
    // `lat_under_spin` instead of the spin model.
    let damp = std::env::var("SEARCH_DAMP").is_ok();
    let w_model = std::env::var("SEARCH_W").is_ok();
    let mut rng = wp_math::Rng::new(7);
    let mut u = |lo: f32, hi: f32| lo + (hi - lo) * rng.next_f32();
    let mut rows = Vec::new();
    for _ in 0..n {
        let k_damp = if damp {
            (u(0.0, 0.3) * 20.0).round() / 20.0
        } else {
            0.0
        };
        let w = u(0.05, 0.6);
        let t = CarTune {
            spin_model: !w_model,
            lat_under_spin: if w_model { w } else { 1.0 },
            yaw_damping: k_damp,
            spin_b0: u(2.0, 16.0),
            spin_b2: u(1.2, 4.0),
            spin_p: u(2.0, 8.0).round(),
            spin_c: u(0.6, 1.0),
            spin_fx_slide: u(0.0, 1.0),
            spin_fx_yield: if u(0.0, 1.0) < 0.5 { 0.0 } else { u(0.0, 1.0) },
            ..CarTune::default()
        };
        car_lab::set_override(Some(t));
        let (s, v) = robust(&c);
        rows.push((s - 4.0 * k_damp, t, v));
    }
    car_lab::set_override(None);
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (s, t, v) in rows.iter().take(12) {
        println!("score {s:.2}  TYRE='{}'", serde_json::to_string(t).unwrap());
        for h in v {
            println!("    {}", fmt_hold(h));
        }
    }
}

/// `refine [n]`: seeded hill-climb from `TYRE` (spin model knobs ±15 %), ranked by [`robust`].
fn refine(n: usize) {
    let c = car("parity_rwd_d");
    let mut best: CarTune =
        serde_json::from_str(&std::env::var("TYRE").expect("TYRE start")).expect("TYRE json");
    car_lab::set_override(Some(best));
    let (mut best_s, _) = robust(&c);
    let mut rng = wp_math::Rng::new(11);
    for i in 0..n {
        let mut t = best;
        let mut j = |x: &mut f32, lo: f32, hi: f32| {
            if rng.next_f32() < 0.5 {
                *x = (*x * (0.85 + 0.3 * rng.next_f32())).clamp(lo, hi);
            }
        };
        j(&mut t.spin_b0, 0.5, 30.0);
        j(&mut t.spin_b2, 0.5, 8.0);
        j(&mut t.spin_c, 0.3, 1.0);
        j(&mut t.spin_fx_slide, 0.0, 1.0);
        j(&mut t.spin_fx_yield, 0.0, 1.0);
        if rng.next_f32() < 0.15 {
            t.spin_p = (t.spin_p + if rng.next_f32() < 0.5 { -1.0 } else { 1.0 }).clamp(1.0, 8.0);
        }
        car_lab::set_override(Some(t));
        let (s, _) = robust(&c);
        if s > best_s {
            best_s = s;
            best = t;
            eprintln!("{i}: {s:.2} {}", serde_json::to_string(&t).unwrap());
        }
    }
    car_lab::set_override(Some(best));
    let (s, v) = robust(&c);
    car_lab::set_override(None);
    println!(
        "best {s:.2} TYRE='{}'",
        serde_json::to_string(&best).unwrap()
    );
    for h in v {
        println!("    {}", fmt_hold(&h));
    }
}

fn trace(id: &str) {
    let c = car(id);
    let a = reference_assists();
    if let Ok(j) = std::env::var("TYRE") {
        car_lab::set_override(Some(serde_json::from_str(&j).expect("TYRE json")));
    }
    let mut tick = 0usize;
    println!("t | key | wheel ° | front trav ° | slide ° | yaw °/s | km/h | Fy F kN | Fy R kN | Fx R kN | kappa RL/RR | alpha R °");
    let d = if std::env::var("ENTRY12").is_ok() {
        COUNTER12
    } else {
        COUNTER6
    };
    plain_drift_run_with(&c, &a, d, 40.0, 0.6, |rig, steer, _| {
        if tick.is_multiple_of(12) {
            let s = slide_state(rig);
            let w = &rig.state().wheel_states;
            println!(
                "{:.2} | {steer:+.0} | {:+.1} | {:+.1} | {:+.1} | {:+.0} | {:.0} | {:+.1} | {:+.1} | {:+.1} | {:.2}/{:.2} | {:+.1}",
                tick as f32 / 240.0,
                s.steer.to_degrees(),
                s.front_axle_angle.to_degrees(),
                (-s.body_slip).to_degrees(),
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
    });
    car_lab::set_override(None);
}

fn main() {
    plain::set_override(Some(PlainSteer::default()));
    if std::env::var("RIDE").as_deref() != Ok("data") {
        ride_lab::set_override(Some(RideTune::lab_default()));
    }
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let what = if args
        .first()
        .is_some_and(|a| ["table", "grid", "trace", "search", "refine"].contains(&a.as_str()))
    {
        args.remove(0)
    } else {
        "table".into()
    };
    if args.is_empty() {
        args = vec![if what == "search" {
            "300"
        } else {
            "parity_rwd_d"
        }
        .into()];
    }
    match what.as_str() {
        "grid" => grid(&args),
        "search" => search(args[0].parse().unwrap_or(300)),
        "refine" => refine(args[0].parse().unwrap_or(150)),
        "trace" => trace(&args[0]),
        _ => table(&args),
    }
}
