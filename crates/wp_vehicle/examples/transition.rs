//! Drift transitions (lead, Drift Lab W step 14: "it's hard to catch drift transitions"):
//! drift left → swing through → drift right on the keyboard, the owner's flick from reference
//! clip 1 (keys reversed with 0.03–0.2 s gaps, the car swings through in ~0.5–0.8 s and is
//! caught on the new side at once with the opposite key + throttle).
//!
//! The driver: settle on the lab's 40 m left-hand circle at 0.5 g, into-turn key + throttle
//! until the slide passes 12°, then the countersteer key + throttle (a held left drift) for
//! 1.5 s. Then the flick: the countersteer key released for `gap` s (throttle held, or lifted
//! for 0.2 s from the release), then the opposite key (the new side's countersteer) + full
//! throttle for the rest of the 4 s after the flick.
//!
//! Per case, with `b` = the new-side slide angle (+ = tail out to the left, = body slip, °):
//! - `swing`: time from the release until `b` ≥ 10°, s;
//! - `peak`: largest `b` after crossing, ° (overshoot);
//! - `settle`: time from crossing until `b` stays within ±8° of its mean over the last 1 s;
//! - `caught`: crossed, no spin, never back below −10° (a swing back), and at least half of
//!   the remaining time in 10–50°;
//! - `spun`: |b| ≥ 90° or travelling backwards.
//!
//! Configurations: `W defaults` (plain steering, lab ride, spin model tyres), `balance new`
//! (+ `GripBalance::lab_default()`), `slow weight` (`RideTune::slow_weight()` +
//! `GripBalance::for_slow_weight()`), each with the plain steering and with the VR steering
//! of the "drift: spin + VR (test)" preset. Env overrides on top of every configuration
//! (search): `PLAIN='{…}'` (merged over that configuration's steering), `TYRE='{…}'` (car lab,
//! merged over the spin model), `YAW_INERTIA=k`, `ONLY=<config name substring>`.
//!
//! `cargo run --release -p wp_vehicle --example transition [-- cases]`
//! (`cases` prints every case; default prints one summary row per configuration).

use std::path::PathBuf;

use wp_contracts::{LoadedCar, VehicleInput};
use wp_vehicle::car_lab::{self, CarTune};
use wp_vehicle::drift_lab::{settle_on_circle, slide_state, DRIFT_GEAR};
use wp_vehicle::drift_lab_w::{load_any_car, reference_assists};
use wp_vehicle::grip_lab::{self, GripBalance};
use wp_vehicle::plain::{self, PlainSteer};
use wp_vehicle::ride_lab::{self, RideTune};

const TPS: usize = 240;
const CARS: [&str; 3] = ["parity_rwd_d", "parity_rwd_a", "parity_rwd_b"];
const GAPS: [f32; 3] = [0.05, 0.15, 0.3];

/// One transition's result.
#[derive(Debug, Clone, Copy, Default)]
struct Transition {
    /// Old-side slide angle at the flick, °.
    at_flick: f32,
    swing_s: Option<f32>,
    peak: f32,
    settle_s: Option<f32>,
    caught: bool,
    spun: bool,
    swung_back: bool,
    /// Largest |yaw rate| from the flick on, °/s.
    peak_yaw: f32,
    /// Largest |yaw acceleration| from the flick on (0.05 s average), °/s².
    peak_yaw_acc: f32,
    /// Largest |roll rate| from the flick on (0.05 s average), °/s.
    peak_roll_rate: f32,
}

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let mut c = load_any_car(&data, id).unwrap();
    if let Ok(k) = std::env::var("YAW_INERTIA") {
        c.def.inertia_kgm2[2] *= k.parse::<f32>().expect("YAW_INERTIA");
    }
    // Data experiments: anti-roll bar rates, N/m (`FRONT_ARB`, `REAR_ARB`).
    for (var, range) in [("FRONT_ARB", 0..2), ("REAR_ARB", 2..4)] {
        if let Ok(v) = std::env::var(var) {
            let v: f32 = v.parse().expect(var);
            for i in range {
                c.def.suspension[i].arb_n_per_m = v;
            }
        }
    }
    c
}

/// Merge the JSON object `over` onto `base` (serde round trip).
fn merged<T: serde::Serialize + serde::de::DeserializeOwned>(base: &T, over: &str) -> T {
    let mut v = serde_json::to_value(base).unwrap();
    let o: serde_json::Value = serde_json::from_str(over).expect("override json");
    for (k, x) in o.as_object().expect("override must be an object") {
        v[k] = x.clone();
    }
    serde_json::from_value(v).expect("override fields")
}

/// How the swing through is started.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Init {
    /// Keys reversed at the flick (old countersteer released, `gap`, new key), throttle held.
    Reverse,
    /// Throttle off for `.0` s with the old countersteer key still held; the keys are
    /// reversed (release, `gap`, new key + throttle) when the old-side angle drops below
    /// [`REACT_DEG`] (the car passing straight), at the latest 1.5 s after the lift.
    Lift(f32),
}

/// `SWING_PLAIN='{…}'`: steering fields merged over the configuration's steering from the
/// key reversal to the end of the run (what a "swing" steering mode would do); `on = false`
/// restores the configuration's steering.
fn swing_steer(on: bool) {
    thread_local!(static BASE: std::cell::Cell<Option<PlainSteer>> = const { std::cell::Cell::new(None) });
    let Ok(j) = std::env::var("SWING_PLAIN") else {
        return;
    };
    if on {
        let cur = plain::active().expect("plain steering on");
        BASE.with(|b| b.set(Some(cur)));
        plain::set_override(Some(merged(&cur, &j)));
    } else if let Some(b) = BASE.with(|b| b.take()) {
        plain::set_override(Some(b));
    }
}

/// Old-side angle where a lifting driver reverses the keys, °.
const REACT_DEG: f32 = 5.0;

/// Run one flick from a held left drift. `None` = the drift never held 30–45° for 0.5 s.
fn flick(c: &LoadedCar, gap: f32, init: Init) -> Option<Transition> {
    let a = reference_assists();
    let mut rig = settle_on_circle(c, &a, 40.0, 0.5);
    let mut entered = false;
    let mut in_band = 0usize;
    let mut flick_at: Option<usize> = None;
    let mut reverse_at: Option<usize> = None;
    let mut t = Transition::default();
    let mut crossed_at: Option<usize> = None;
    let mut b_log: Vec<f32> = Vec::new();
    let mut yaw_hist: Vec<f32> = Vec::new();
    let mut roll_hist: Vec<f32> = Vec::new();
    let gap_t = (gap * TPS as f32).round() as usize;
    let after = 4 * TPS;
    let mut tick = 0usize;
    loop {
        let s = slide_state(&rig);
        let old = (-s.body_slip).to_degrees();
        entered |= old > 12.0;
        if flick_at.is_none() && entered {
            in_band = if (30.0..=45.0).contains(&old) {
                in_band + 1
            } else {
                0
            };
            if in_band >= TPS / 2 {
                flick_at = Some(tick);
                t.at_flick = old;
                if init == Init::Reverse {
                    reverse_at = Some(tick);
                    swing_steer(true);
                }
            }
        }
        if let (Some(f), None, Init::Lift(l)) = (flick_at, reverse_at, init) {
            let since = tick - f;
            if old < REACT_DEG && since > 0 || since as f32 >= (l + 1.5) * TPS as f32 {
                reverse_at = Some(tick);
                swing_steer(true);
            }
        }
        let (steer, throttle) = match (flick_at, reverse_at) {
            (None, _) => (if entered { 1.0 } else { -1.0 }, 1.0),
            (Some(f), None) => {
                let l = match init {
                    Init::Lift(l) => l,
                    Init::Reverse => 0.0,
                };
                let thr = if ((tick - f) as f32) < l * TPS as f32 {
                    0.0
                } else {
                    1.0
                };
                (1.0, thr)
            }
            (Some(_), Some(r)) => (if tick - r < gap_t { 0.0 } else { -1.0 }, 1.0),
        };
        rig.step(
            &VehicleInput {
                steer,
                throttle,
                gear_down: !a.auto_gears && rig.vehicle.gear() > DRIFT_GEAR,
                ..Default::default()
            },
            &a,
        );
        tick += 1;
        match flick_at {
            None if tick > 8 * TPS => return None,
            None => continue,
            Some(f) => {
                let n = slide_state(&rig);
                let b = n.body_slip.to_degrees();
                let k = tick - f;
                let yaw = n.yaw_rate.to_degrees();
                t.peak_yaw = t.peak_yaw.max(yaw.abs());
                yaw_hist.push(yaw);
                roll_hist.push(wp_vehicle::drift_lab_w::body_roll(&rig).to_degrees());
                if roll_hist.len() > 12 {
                    let r =
                        (roll_hist[roll_hist.len() - 1] - roll_hist[roll_hist.len() - 13]) / 0.05;
                    t.peak_roll_rate = t.peak_roll_rate.max(r.abs());
                }
                if yaw_hist.len() > 12 {
                    let acc = (yaw - yaw_hist[yaw_hist.len() - 13]) / 0.05;
                    t.peak_yaw_acc = t.peak_yaw_acc.max(acc.abs());
                }
                if std::env::var("TRACE").is_ok() && k.is_multiple_of(24) {
                    let w = &rig.state().wheel_states;
                    eprintln!(
                        "t {:.1} key {steer:+} thr {throttle} | body slip {b:+.0}° yaw {:+.0}°/s wheel {:+.1}° | rear α {:+.0}° κ {:.2} Fy {:+.1} kN | front α {:+.0}° v {:.0} km/h",
                        k as f32 / TPS as f32,
                        n.yaw_rate.to_degrees(),
                        n.steer.to_degrees(),
                        (0.5 * (w[2].slip_angle + w[3].slip_angle)).to_degrees(),
                        0.5 * (w[2].slip_ratio + w[3].slip_ratio),
                        (w[2].fy_n + w[3].fy_n) / 1000.0,
                        (0.5 * (w[0].slip_angle + w[1].slip_angle)).to_degrees(),
                        n.v_fwd * 3.6,
                    );
                }
                if b.abs() >= 90.0 || n.v_fwd < 0.0 {
                    t.spun = true;
                    break;
                }
                if crossed_at.is_none() && b >= 10.0 {
                    crossed_at = Some(k);
                    t.swing_s = Some(k as f32 / TPS as f32);
                }
                if crossed_at.is_some() {
                    t.peak = t.peak.max(b);
                    if b <= -10.0 {
                        t.swung_back = true;
                    }
                    b_log.push(b);
                }
                if k >= after {
                    break;
                }
            }
        }
    }
    swing_steer(false);
    if let Some(cx) = crossed_at {
        let n = b_log.len();
        let last = &b_log[n.saturating_sub(TPS)..];
        let mean = last.iter().sum::<f32>() / last.len().max(1) as f32;
        // Settle: the first index from which b stays within ±8° of the final mean.
        let mut settle = n;
        for i in (0..n).rev() {
            if (b_log[i] - mean).abs() > 8.0 {
                break;
            }
            settle = i;
        }
        if settle < n.saturating_sub(TPS / 2) && !t.spun {
            t.settle_s = Some(settle as f32 / TPS as f32);
        }
        let in_band = b_log.iter().filter(|b| (10.0..=50.0).contains(*b)).count();
        let remaining = after.saturating_sub(cx).max(1);
        t.caught = !t.spun && !t.swung_back && in_band * 2 >= remaining;
    }
    Some(t)
}

struct Config {
    name: &'static str,
    plain: PlainSteer,
    ride: RideTune,
    grip: Option<GripBalance>,
}

fn configs() -> Vec<Config> {
    let vr = CarTune::spin_vr_plain();
    let p = PlainSteer::default();
    let base = [
        ("W defaults", RideTune::lab_default(), None),
        (
            "balance new",
            RideTune::lab_default(),
            Some(GripBalance::lab_default()),
        ),
        (
            "slow weight",
            RideTune::slow_weight(),
            Some(GripBalance::for_slow_weight()),
        ),
    ];
    let mut v = Vec::new();
    for (name, ride, grip) in base {
        for (steer_name, steer) in [("plain", p), ("VR", vr)] {
            let full: &'static str = Box::leak(format!("{name} · {steer_name}").into_boxed_str());
            v.push(Config {
                name: full,
                plain: steer,
                ride,
                grip,
            });
        }
    }
    if let Ok(o) = std::env::var("ONLY") {
        v.retain(|c| c.name.contains(o.as_str()));
    }
    v
}

fn apply(cfg: &Config) {
    let p = match std::env::var("PLAIN") {
        Ok(j) => merged(&cfg.plain, &j),
        Err(_) => cfg.plain,
    };
    plain::set_override(Some(p));
    ride_lab::set_override(Some(cfg.ride));
    grip_lab::set_override(cfg.grip);
    let tyre = match std::env::var("TYRE") {
        Ok(j) => merged(&CarTune::spin_candidate(), &j),
        Err(_) => CarTune::spin_candidate(),
    };
    car_lab::set_override(Some(tyre));
}

/// Swing starts measured (`INIT=reverse|lift` limits to one kind).
fn inits() -> Vec<Init> {
    match std::env::var("INIT").as_deref() {
        Ok("reverse") => vec![Init::Reverse],
        Ok("lift") => match std::env::var("LIFTS") {
            Ok(l) => l
                .split(',')
                .map(|x| Init::Lift(x.parse().expect("LIFTS")))
                .collect(),
            Err(_) => vec![Init::Lift(0.2), Init::Lift(0.4)],
        },
        _ => vec![Init::Reverse, Init::Lift(0.2), Init::Lift(0.4)],
    }
}

fn fmt_opt(x: Option<f32>) -> String {
    x.map_or("—".into(), |v| format!("{v:.2}"))
}

/// `trace`: the old-side angle every 0.25 s of a held drift (no flick), per config and car.
fn trace_hold(cars: &[(&str, LoadedCar)]) {
    let a = reference_assists();
    for cfg in configs() {
        apply(&cfg);
        for (id, c) in cars {
            let mut rig = settle_on_circle(c, &a, 40.0, 0.5);
            let mut entered = false;
            let mut row = Vec::new();
            for tick in 0..6 * TPS {
                let s = slide_state(&rig);
                let old = (-s.body_slip).to_degrees();
                entered |= old > 12.0;
                if tick % (TPS / 4) == 0 {
                    row.push(format!("{old:.0}"));
                }
                let steer = if entered { 1.0 } else { -1.0 };
                rig.step(
                    &VehicleInput {
                        steer,
                        throttle: 1.0,
                        gear_down: !a.auto_gears && rig.vehicle.gear() > DRIFT_GEAR,
                        ..Default::default()
                    },
                    &a,
                );
            }
            println!("{} | {id} | {}", cfg.name, row.join(" "));
        }
    }
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("trace") {
        let cars: Vec<(&str, LoadedCar)> = CARS.iter().map(|id| (*id, car(id))).collect();
        trace_hold(&cars);
        return;
    }
    let cases = std::env::args().nth(1).as_deref() == Some("cases");
    let ids: Vec<String> = match std::env::var("CARS") {
        Ok(l) => l.split(',').map(String::from).collect(),
        Err(_) => CARS.iter().map(|s| s.to_string()).collect(),
    };
    let cars: Vec<(&str, LoadedCar)> = ids.iter().map(|id| (id.as_str(), car(id))).collect();
    if !cases {
        println!("| config | caught / flicks | no hold | spun | swung back | swing s (median) | peak ° (median / max) | settle s (median) | peak yaw °/s (median) | peak yaw acc °/s² (median) | peak roll rate °/s (median) |");
        println!("|---|---|---|---|---|---|---|---|---|---|---|");
    }
    for cfg in configs() {
        apply(&cfg);
        let mut all = Vec::new();
        let mut no_hold = 0usize;
        if cases {
            println!("\n## {}\n", cfg.name);
            println!("| car | gap s | start | old side at flick ° | swing s | peak ° | settle s | result |");
            println!("|---|---|---|---|---|---|---|---|");
        }
        for (id, c) in &cars {
            for gap in GAPS {
                for init in inits() {
                    let Some(t) = flick(c, gap, init) else {
                        if cases {
                            println!("| {id} | {gap} | {init:?} | no hold | | | | — |");
                        }
                        no_hold += 1;
                        continue;
                    };
                    if cases {
                        let r = if t.spun {
                            "SPUN"
                        } else if t.swung_back {
                            "swung back"
                        } else if t.caught {
                            "**caught**"
                        } else if t.swing_s.is_none() {
                            "never crossed"
                        } else {
                            "not held"
                        };
                        println!(
                            "| {id} | {gap} | {init:?} | {:.0} | {} | {:.0} | {} | {r} |",
                            t.at_flick,
                            fmt_opt(t.swing_s),
                            t.peak,
                            fmt_opt(t.settle_s)
                        );
                    }
                    all.push(t);
                }
            }
        }
        let n = all.len();
        let caught = all.iter().filter(|t| t.caught).count();
        let spun = all.iter().filter(|t| t.spun).count();
        let back = all.iter().filter(|t| t.swung_back).count();
        let med = |mut v: Vec<f32>| -> Option<f32> {
            if v.is_empty() {
                return None;
            }
            v.sort_by(|a, b| a.total_cmp(b));
            Some(v[v.len() / 2])
        };
        let swing = med(all.iter().filter_map(|t| t.swing_s).collect());
        let peaks: Vec<f32> = all
            .iter()
            .filter(|t| t.swing_s.is_some())
            .map(|t| t.peak)
            .collect();
        let pmax = peaks.iter().copied().fold(0.0f32, f32::max);
        let settle = med(all.iter().filter_map(|t| t.settle_s).collect());
        println!(
            "| {} | **{caught} / {n}** | {no_hold} | {spun} | {back} | {} | {} / {pmax:.0} | {} | {} | {} | {} |",
            cfg.name,
            fmt_opt(swing),
            med(peaks).map_or("—".into(), |v| format!("{v:.0}")),
            fmt_opt(settle),
            med(all.iter().map(|t| t.peak_yaw).collect()).map_or("—".into(), |v| format!("{v:.0}")),
            med(all.iter().map(|t| t.peak_yaw_acc).collect()).map_or("—".into(), |v| format!("{v:.0}")),
            med(all.iter().map(|t| t.peak_roll_rate).collect()).map_or("—".into(), |v| format!("{v:.0}")),
        );
    }
}
