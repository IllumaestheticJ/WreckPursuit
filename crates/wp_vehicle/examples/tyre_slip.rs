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
    let mut c = load_any_car(&data, id).unwrap();
    // IZZ=0.9: offline experiment, the car's yaw inertia × this (spawn-time car data).
    if let Ok(k) = std::env::var("IZZ") {
        c.def.inertia_kgm2[2] *= k.parse::<f32>().expect("IZZ number");
    }
    c
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

/// One held-countersteer run's score: held → time in band minus unsteadiness; spun or
/// swapped → negative; never slid → worse.
fn run_score(h: &Hold) -> f32 {
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
}

/// The robustness set: `ROBUST=full` = parity_rwd_d / a / b × counter key at 6 / 12 / 20° ×
/// 0.3 / 0.5 / 0.7 g; default = parity_rwd_d × 6 / 12° × 0.5 / 0.6 / 0.7 g.
fn robust_set() -> Vec<(LoadedCar, PlainDriver, f32)> {
    let full = std::env::var("ROBUST").as_deref() == Ok("full");
    let ids: &[&str] = if full {
        &["parity_rwd_d", "parity_rwd_a", "parity_rwd_b"]
    } else {
        &["parity_rwd_d"]
    };
    let entries: &[f32] = if full {
        &[6.0, 12.0, 20.0]
    } else {
        &[6.0, 12.0]
    };
    let gs: &[f32] = if full {
        &[0.3, 0.5, 0.7]
    } else {
        &[0.5, 0.6, 0.7]
    };
    let mut v = Vec::new();
    for id in ids {
        let c = car(id);
        for &e in entries {
            for &g in gs {
                let d = PlainDriver::HoldCounter {
                    entry_deg: e,
                    throttle: 1.0,
                };
                v.push((c.clone(), d, g));
            }
        }
    }
    v
}

/// Robust score over [`robust_set`]: the worst case counts most.
fn robust(set: &[(LoadedCar, PlainDriver, f32)]) -> (f32, Vec<Hold>) {
    let v: Vec<Hold> = set.iter().map(|(c, d, g)| hold_at(c, *d, *g)).collect();
    let sc: Vec<f32> = v.iter().map(run_score).collect();
    let min = sc.iter().copied().fold(f32::INFINITY, f32::min);
    let mean = sc.iter().sum::<f32>() / sc.len() as f32;
    (
        min + 0.5 * mean + 0.2 * v.iter().filter(|h| h.m.held).count() as f32,
        v,
    )
}

/// Random spin-model + rack-compliance set (`SEARCH_W=1`: ellipse + `lat_under_spin`).
fn sample(u: &mut impl FnMut(f32, f32) -> f32) -> CarTune {
    if std::env::var("SEARCH_LOCK").is_ok() {
        // Step 3: lock / entry side around the spin model.
        let d = spin();
        let r: Vec<f32> = (0..10).map(|_| u(0.0, 1.0)).collect();
        let j = |x: f32, k: f32, r: f32| x * (1.0 - k + 2.0 * k * r);
        if std::env::var("SEARCH_DEEP").is_ok() {
            // Grip-neutral variant: base lock 1, front deep-slip falloff instead of front C.
            return CarTune {
                drift_lock_extra: -0.45 + 0.5 * r[1],
                drift_lock_from_deg: 5.0 + 25.0 * r[2],
                front_deep_falloff: 0.5 * r[3],
                front_deep_from_deg: 6.0 + 10.0 * r[4],
                front_deep_full_deg: 18.0 + 22.0 * r[0],
                rear_relax: 0.5 + 1.0 * r[5],
                spin_b0: j(d.spin_b0, 0.3, r[6]),
                spin_b2: j(d.spin_b2, 0.2, r[7]),
                spin_c: j(d.spin_c, 0.1, r[8]).min(1.0),
                spin_fx_slide: j(d.spin_fx_slide, 0.1, r[9]).min(1.0),
                ..d
            };
        }
        return CarTune {
            lock: 0.75 + 0.4 * r[0],
            drift_lock_extra: -0.4 + 0.8 * r[1],
            drift_lock_from_deg: 5.0 + 40.0 * r[2],
            front_lat_c: if r[3] < 0.5 { 0.0 } else { 1.2 + 0.6 * r[4] },
            rear_relax: 0.5 + 2.0 * r[5],
            spin_b0: j(d.spin_b0, 0.3, r[6]),
            spin_b2: j(d.spin_b2, 0.2, r[7]),
            spin_c: j(d.spin_c, 0.1, r[8]).min(1.0),
            spin_fx_slide: j(d.spin_fx_slide, 0.1, r[9]).min(1.0),
            ..d
        };
    }
    let w_model = std::env::var("SEARCH_W").is_ok();
    let comply = std::env::var("NO_RACK").is_err();
    let from = u(3.0, 10.0);
    CarTune {
        spin_model: !w_model,
        lat_under_spin: if w_model { u(0.05, 0.6) } else { 1.0 },
        spin_b0: u(2.0, 16.0),
        spin_b2: u(1.2, 4.0),
        spin_p: u(2.0, 8.0).round(),
        spin_c: u(0.6, 1.0),
        spin_fx_slide: u(0.0, 1.0),
        spin_fx_slide_from_deg: from,
        spin_fx_slide_full_deg: from + u(2.0, 15.0),
        spin_fx_yield: if u(0.0, 1.0) < 0.5 { 0.0 } else { u(0.0, 1.0) },
        rack_compliance: if comply { u(0.0, 60.0) } else { 0.0 },
        rack_compliance_tau_s: u(0.02, 0.2),
        ..CarTune::default()
    }
}

/// `search [n]`: seeded random search, ranked by [`robust`].
fn search(n: usize) {
    let set = robust_set();
    let grip_ref = std::env::var("GRIP_PENALTY").is_ok().then(|| {
        let d = car("parity_rwd_d");
        car_lab::set_override(None);
        let g0 = grip(&d);
        (d, g0)
    });
    let mut rng = wp_math::Rng::new(7);
    let mut u = |lo: f32, hi: f32| lo + (hi - lo) * rng.next_f32();
    let mut rows = Vec::new();
    for _ in 0..n {
        let t = sample(&mut u);
        car_lab::set_override(Some(t));
        let (mut s, v) = robust(&set);
        if let Some((d, g0)) = &grip_ref {
            // GRIP_PENALTY: grip-driving change beyond 5 % costs score.
            let worst = grip(d)
                .iter()
                .zip(g0)
                .map(|(a, b)| ((a - b) / b.abs().max(1e-3) * 100.0).abs())
                .fold(0.0f32, f32::max);
            s -= 0.3 * (worst - 5.0).max(0.0);
        }
        rows.push((s, t, v));
    }
    car_lab::set_override(None);
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (s, t, v) in rows.iter().take(8) {
        let held = v.iter().filter(|h| h.m.held).count();
        println!(
            "score {s:.2} held {held}/{}  TYRE='{}'",
            v.len(),
            serde_json::to_string(t).unwrap()
        );
        for h in v {
            println!("    {}", fmt_hold(h));
        }
    }
}

/// `refine [n]`: seeded hill-climb from `TYRE` (knobs ±15 %), ranked by [`robust`].
fn refine(n: usize) {
    let set = robust_set();
    let grip_ref = std::env::var("GRIP_PENALTY").is_ok().then(|| {
        let d = car("parity_rwd_d");
        car_lab::set_override(None);
        let g0 = grip(&d);
        (d, g0)
    });
    let score = |set: &[(LoadedCar, PlainDriver, f32)]| {
        let (mut s, v) = robust(set);
        if let Some((d, g0)) = &grip_ref {
            let worst = grip(d)
                .iter()
                .zip(g0)
                .map(|(a, b)| ((a - b) / b.abs().max(1e-3) * 100.0).abs())
                .fold(0.0f32, f32::max);
            s -= 0.3 * (worst - 5.0).max(0.0);
        }
        (s, v)
    };
    let mut best: CarTune =
        serde_json::from_str(&std::env::var("TYRE").expect("TYRE start")).expect("TYRE json");
    car_lab::set_override(Some(best));
    let (mut best_s, _) = score(&set);
    let mut rng = wp_math::Rng::new(11);
    for i in 0..n {
        let mut t = best;
        let lock_r = (rng.next_f32(), rng.next_f32());
        let mut j = |x: &mut f32, lo: f32, hi: f32| {
            if rng.next_f32() < 0.4 {
                *x = (*x * (0.85 + 0.3 * rng.next_f32())).clamp(lo, hi);
            }
        };
        j(&mut t.spin_b0, 0.5, 30.0);
        j(&mut t.spin_b2, 0.5, 8.0);
        j(&mut t.spin_c, 0.3, 1.0);
        j(&mut t.spin_fx_slide, 0.0, 1.0);
        j(&mut t.spin_fx_yield, 0.0, 1.0);
        j(&mut t.spin_fx_slide_from_deg, 3.0, 30.0);
        j(&mut t.spin_fx_slide_full_deg, 3.0, 45.0);
        if std::env::var("SEARCH_DEEP").is_ok() {
            // Signed knob: jitter by ± 0.05 absolute.
            if lock_r.0 < 0.4 {
                t.drift_lock_extra = (t.drift_lock_extra + 0.1 * (lock_r.1 - 0.5)).clamp(-0.7, 0.5);
            }
            j(&mut t.drift_lock_from_deg, 3.0, 45.0);
            j(&mut t.front_deep_falloff, 0.0, 0.9);
            j(&mut t.front_deep_from_deg, 6.0, 30.0);
            j(&mut t.front_deep_full_deg, 8.0, 60.0);
            j(&mut t.rear_relax, 0.2, 4.0);
        }
        if t.rack_compliance > 0.0 {
            j(&mut t.rack_compliance, 0.5, 120.0);
            j(&mut t.rack_compliance_tau_s, 0.01, 0.5);
        }
        if rng.next_f32() < 0.15 {
            t.spin_p = (t.spin_p + if rng.next_f32() < 0.5 { -1.0 } else { 1.0 }).clamp(1.0, 8.0);
        }
        car_lab::set_override(Some(t));
        let (s, _) = score(&set);
        if s > best_s {
            best_s = s;
            best = t;
            eprintln!("{i}: {s:.2} {}", serde_json::to_string(&t).unwrap());
        }
    }
    car_lab::set_override(Some(best));
    let (s, v) = score(&set);
    car_lab::set_override(None);
    let held = v.iter().filter(|h| h.m.held).count();
    println!(
        "best {s:.2} held {held}/{} TYRE='{}'",
        v.len(),
        serde_json::to_string(&best).unwrap()
    );
    for h in v {
        println!("    {}", fmt_hold(&h));
    }
}

/// `phase [car]`: phase plane of the held counter key + full throttle. A state from inside a
/// held drift (spin model, 3 s into the lab run: full counter-lock reached, rears spinning in
/// 2nd) is restored, set to slide angle β and yaw rate r at the same speed, then stepped 0.05 s
/// with the counter key + full throttle. Cells: `dβ/dt °/s , dr/dt °/s²` (measured over the
/// second half, after the tyre lag has settled). `TYRE='{...}'` = the model probed (default:
/// data and the spin model).
fn phase(id: &str) {
    use wp_contracts::VehicleInput;
    use wp_vehicle::math::V3;
    use wp_vehicle::rig::Rig;
    let c = car(id);
    let a = reference_assists();
    car_lab::set_override(Some(spin()));
    let mut snap = None;
    let mut tick = 0usize;
    plain_drift_run_with(&c, &a, COUNTER6, 40.0, 0.6, |rig, _, _| {
        if tick == 3 * 240 {
            snap = Some(rig.snapshot());
        }
        tick += 1;
    });
    let snap = snap.expect("3 s state");
    let mut models: Vec<(String, CarTune)> = vec![
        ("data (friction ellipse)".into(), CarTune::default()),
        ("spin model".into(), spin()),
    ];
    if let Ok(j) = std::env::var("TYRE") {
        models.push((
            format!("TYRE {j}"),
            serde_json::from_str(&j).expect("TYRE json"),
        ));
    }
    let betas: Vec<f32> = (0..=12).map(|i| i as f32 * 5.0).collect();
    let rates: Vec<f32> = (0..=7).map(|i| -20.0 + i as f32 * 20.0).collect();
    let input = VehicleInput {
        steer: 1.0,
        throttle: 1.0,
        ..Default::default()
    };
    let probe = |b: f32, r: f32| -> (f32, f32, f32, f32, f32) {
        let mut rig = Rig::new(&c);
        rig.restore(&snap).expect("restore");
        let ch = *rig.vehicle.chassis();
        let up = ch.rot * V3::Z;
        let (fwd, right) = (ch.rot * V3::Y, ch.rot * V3::X);
        let v = ch.lin_vel.dot(fwd).hypot(ch.lin_vel.dot(right));
        let (sb, cb) = wp_vehicle::math::sin_cos(b.to_radians());
        let want = fwd * (v * cb) + right * (v * sb);
        let dv = want - ch.lin_vel;
        let dw = up * (r.to_radians() - ch.ang_vel.dot(up));
        rig.kick([dv.x, dv.y, 0.0], [dw.x, dw.y, dw.z]);
        let st = |rig: &Rig| {
            let s = slide_state(rig);
            ((-s.body_slip).to_degrees(), s.yaw_rate.to_degrees())
        };
        for _ in 0..6 {
            rig.step(&input, &a);
        }
        let (b0, r0) = st(&rig);
        for _ in 0..6 {
            rig.step(&input, &a);
        }
        let (b1, r1) = st(&rig);
        let w = &rig.state().wheel_states;
        let dt = 6.0 / 240.0;
        (
            (b1 - b0) / dt,
            (r1 - r0) / dt,
            (w[0].fy_n + w[1].fy_n) / 1000.0,
            (w[2].fy_n + w[3].fy_n) / 1000.0,
            0.5 * (w[2].slip_ratio + w[3].slip_ratio),
        )
    };
    for (name, t) in models {
        car_lab::set_override(Some(t));
        println!("\n### {id}, {name}: dβ/dt °/s , dr/dt °/s² (β = slide angle, + = intended side)");
        print!("| β \\ r °/s |");
        for r in &rates {
            print!(" {r:+.0} |");
        }
        println!(" Fy front / rear kN, rear κ at r = +20 |");
        print!("|---|");
        for _ in &rates {
            print!("---|");
        }
        println!("---|");
        for &b in &betas {
            print!("| {b:.0}° |");
            let mut at20 = (0.0, 0.0, 0.0);
            for &r in &rates {
                let (db, dr, ff, fr, k) = probe(b, r);
                if r == 20.0 {
                    at20 = (ff, fr, k);
                }
                print!(" {db:+.0}, {dr:+.0} |");
            }
            println!(" {:+.1} / {:+.1}, {:.2} |", at20.0, at20.1, at20.2);
        }
    }
    car_lab::set_override(None);
}

/// Basin of the held drift on the phase plane at r = +20 °/s: `(edge, equilibrium)` slide
/// angles, degrees. Each probe pins (β, r) for 0.1 s (steering / tyre lags settle under the
/// active override), then measures dr/dt over 0.025 s. `edge` = where dr/dt turns positive
/// (below it yaw collapses: swing-through); `equilibrium` = where it turns back negative
/// (stable). `None` = no such crossing in 20–75°.
fn basin(
    c: &LoadedCar,
    snap: &wp_vehicle::snapshot::VehicleSnapshot,
) -> (Option<f32>, Option<f32>) {
    use wp_contracts::VehicleInput;
    use wp_vehicle::math::V3;
    use wp_vehicle::rig::Rig;
    let a = reference_assists();
    let input = VehicleInput {
        steer: 1.0,
        throttle: 1.0,
        ..Default::default()
    };
    let r_deg = 20.0f32;
    let drdt = |b: f32| -> f32 {
        let mut rig = Rig::new(c);
        rig.restore(snap).expect("restore");
        let v = {
            let ch = rig.vehicle.chassis();
            let (f, x) = (ch.rot * V3::Y, ch.rot * V3::X);
            ch.lin_vel.dot(f).hypot(ch.lin_vel.dot(x))
        };
        let pin = |rig: &mut Rig| {
            let ch = *rig.vehicle.chassis();
            let up = ch.rot * V3::Z;
            let (f, x) = (ch.rot * V3::Y, ch.rot * V3::X);
            let (sb, cb) = wp_vehicle::math::sin_cos(b.to_radians());
            let dv = f * (v * cb) + x * (v * sb) - ch.lin_vel;
            let dw = up * (r_deg.to_radians() - ch.ang_vel.dot(up));
            rig.kick([dv.x, dv.y, 0.0], [dw.x, dw.y, dw.z]);
        };
        for _ in 0..24 {
            pin(&mut rig);
            rig.step(&input, &a);
        }
        pin(&mut rig);
        let r0 = slide_state(&rig).yaw_rate;
        for _ in 0..6 {
            rig.step(&input, &a);
        }
        (slide_state(&rig).yaw_rate - r0).to_degrees() / (6.0 / 240.0)
    };
    let (mut edge, mut eq) = (None, None);
    let mut prev = drdt(20.0);
    let mut b = 21.0;
    while b <= 75.0 {
        let d = drdt(b);
        if edge.is_none() && prev < 0.0 && d >= 0.0 {
            edge = Some(b);
        } else if edge.is_some() && eq.is_none() && prev > 0.0 && d <= 0.0 {
            eq = Some(b);
        }
        prev = d;
        b += 1.0;
    }
    (edge, eq)
}

/// Snapshot 3 s into the lab's standard held-countersteer run with the spin model (the base
/// state for [`basin`]).
fn hold_snapshot(c: &LoadedCar) -> wp_vehicle::snapshot::VehicleSnapshot {
    let prev = car_lab::active();
    car_lab::set_override(Some(spin()));
    let mut snap = None;
    let mut tick = 0usize;
    plain_drift_run_with(c, &reference_assists(), COUNTER6, 40.0, 0.6, |rig, _, _| {
        if tick == 3 * 240 {
            snap = Some(rig.snapshot());
        }
        tick += 1;
    });
    car_lab::set_override(prev);
    snap.expect("3 s state")
}

/// Grip-driving numbers on `c` for the ±% check: taps (heading °) 0.1 / 0.2 / 0.4 s at 60,
/// 0.1 / 0.2 s at 100, 0.1 s at 140 km/h, lane change max slip °.
fn grip(c: &LoadedCar) -> Vec<f32> {
    let a = reference_assists();
    let mut v = Vec::new();
    for (kmh, taps) in [
        (60.0f32, &[0.1f32, 0.2, 0.4][..]),
        (100.0, &[0.1, 0.2]),
        (140.0, &[0.1]),
    ] {
        for &t in taps {
            v.push(tap_response(c, &a, kmh / 3.6, t).heading_deg);
        }
    }
    v.push(lane_change_slip(c, &a));
    v
}

/// `matrix`: for each config in `MATRIX` (a JSON list of `[name, CarTune]`; default: data +
/// spin model): held / 27 (the full robustness set), grip vs data (largest |Δ| %), basin.
fn matrix() {
    let configs: Vec<(String, CarTune)> = match std::env::var("MATRIX") {
        Ok(j) => serde_json::from_str(&j).expect("MATRIX json [[name, tune], ...]"),
        Err(_) => vec![
            ("data".into(), CarTune::default()),
            ("spin model".into(), spin()),
            ("spin + drift lock".into(), CarTune::spin_lock_candidate()),
        ],
    };
    std::env::set_var("ROBUST", "full");
    let set = robust_set();
    let d = car("parity_rwd_d");
    car_lab::set_override(None);
    let g0 = grip(&d);
    let snap = hold_snapshot(&d);
    println!("| config | held / 27 | held cases (car entry° g) | grip max |Δ| % (worst) | basin edge → eq ° (width) |");
    println!("|---|---|---|---|---|");
    for (name, t) in configs {
        car_lab::set_override(Some(t));
        let (_, v) = robust(&set);
        let held: Vec<String> = set
            .iter()
            .zip(&v)
            .filter(|(_, h)| h.m.held && h.band_s >= 2.0)
            .map(|((c, dr, g), h)| {
                let e = match dr {
                    PlainDriver::HoldCounter { entry_deg, .. } => *entry_deg,
                    _ => 0.0,
                };
                format!(
                    "{} {e:.0}° {g} ({:.0}° {:.1}s)",
                    c.def.id.trim_start_matches("parity_"),
                    h.late_mean,
                    h.band_s
                )
            })
            .collect();
        let n_held = v.iter().filter(|h| h.m.held).count();
        let g = grip(&d);
        let labels = [
            "tap.1@60",
            "tap.2@60",
            "tap.4@60",
            "tap.1@100",
            "tap.2@100",
            "tap.1@140",
            "lane chg",
        ];
        let (worst, wi) = g
            .iter()
            .zip(&g0)
            .map(|(a, b)| ((a - b) / b.abs().max(1e-3) * 100.0).abs())
            .enumerate()
            .fold(
                (0.0f32, 0usize),
                |acc, (i, x)| if x > acc.0 { (x, i) } else { acc },
            );
        let (edge, eq) = basin(&d, &snap);
        let f = |x: Option<f32>| x.map_or("—".to_string(), |b| format!("{b:.0}"));
        let width = match (edge, eq) {
            (Some(a), Some(b)) => format!("{:.0}", b - a),
            _ => "none".into(),
        };
        println!(
            "| {name} | {n_held} (band ≥ 2 s: {}) | {} | {worst:.1} ({}) | {} → {} ({width}) |",
            held.len(),
            held.join(", "),
            labels[wi],
            f(edge),
            f(eq)
        );
    }
    car_lab::set_override(None);
}

/// Straight launch (owner: "spins off the line"): rest → full throttle for 3 s, steer 0, TCS /
/// ESC off, manual gears (stays in 1st), plain steering. Returns (largest |heading change| °,
/// largest |body slip| ° above 2 m/s, km/h at 3 s, peak driven slip ratio).
fn straight_launch(c: &LoadedCar) -> (f32, f32, f32, f32) {
    use wp_contracts::VehicleInput;
    use wp_vehicle::math::{atan2, V3};
    use wp_vehicle::rig::Rig;
    let a = reference_assists();
    let mut rig = Rig::new(c);
    rig.teleport([0.0; 3], 0.0, 0.0, 0.0);
    let heading = |rig: &Rig| {
        let f = rig.vehicle.chassis().rot * V3::Y;
        atan2(-f.x, f.y).to_degrees()
    };
    // Settle on the springs first (keys off).
    for _ in 0..120 {
        rig.step(&VehicleInput::default(), &a);
    }
    let h0 = heading(&rig);
    // A real launch is never perfectly symmetric: LAUNCH_KICK rad/s of yaw at the start
    // (default 0.02 ≈ 1 °/s; 0 = the perfectly symmetric car, which cannot yaw).
    let kick: f32 = std::env::var("LAUNCH_KICK").map_or(0.02, |k| k.parse().expect("number"));
    rig.kick([0.0; 3], [0.0, 0.0, kick]);
    let (mut yaw, mut slip, mut kappa) = (0.0f32, 0.0f32, 0.0f32);
    for _ in 0..3 * 240 {
        rig.step(
            &VehicleInput {
                throttle: 1.0,
                ..Default::default()
            },
            &a,
        );
        yaw = yaw.max((heading(&rig) - h0).abs());
        let s = slide_state(&rig);
        if s.v_fwd > 2.0 {
            slip = slip.max(s.body_slip.to_degrees().abs());
        }
        let w = &rig.state().wheel_states;
        kappa = kappa.max(w[2].slip_ratio.max(w[3].slip_ratio));
    }
    (yaw, slip, slide_state(&rig).v_fwd * 3.6, kappa)
}

/// `launch`: [`straight_launch`] for the RWD parity cars under data + presets (+ `TYRE`).
fn launch_table() {
    let mut configs = vec![
        ("data".to_string(), CarTune::default()),
        ("spin model".to_string(), spin()),
        (
            "spin + drift lock".to_string(),
            CarTune::spin_lock_candidate(),
        ),
        (
            "step-1 spin model (drive floor always on)".to_string(),
            CarTune {
                spin_fx_slide_from_deg: 0.0,
                spin_fx_slide_full_deg: 0.0,
                ..spin()
            },
        ),
    ];
    if let Ok(j) = std::env::var("TYRE") {
        configs.push((
            format!("TYRE {j}"),
            serde_json::from_str(&j).expect("TYRE json"),
        ));
    }
    println!("| car | config | max yaw ° (< 3) | max body slip ° | km/h at 3 s | peak rear κ |");
    println!("|---|---|---|---|---|---|");
    for id in [
        "parity_rwd_a",
        "parity_rwd_b",
        "parity_rwd_d",
        "rampart_50",
        "shiro_turbo",
    ] {
        let c = car(id);
        for (name, t) in &configs {
            car_lab::set_override(Some(*t));
            let (y, s, v, k) = straight_launch(&c);
            let ok = if y < 3.0 { "" } else { " **FAIL**" };
            println!("| {id} | {name} | {y:.2}{ok} | {s:.1} | {v:.0} | {k:.2} |");
        }
    }
    car_lab::set_override(None);
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
    let what = if args.first().is_some_and(|a| {
        [
            "table", "grid", "trace", "search", "refine", "phase", "matrix", "launch",
        ]
        .contains(&a.as_str())
    }) {
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
        "phase" => phase(&args[0]),
        "matrix" => matrix(),
        "launch" => launch_table(),
        _ => table(&args),
    }
}
