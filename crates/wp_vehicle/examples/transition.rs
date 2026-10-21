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
    /// The owner's flick (reference): throttle off for `.0` s from the flick **while the keys
    /// are switched** (old countersteer released at once, `gap`, new key), throttle back on
    /// after the lift.
    LiftFlick(f32),
    /// Throttle off for `.0` s with the old key held; the keys are switched (release, `gap`,
    /// new key) at the end of the lift, throttle back on then.
    LiftSwitchEnd(f32),
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
                if matches!(init, Init::Reverse | Init::LiftFlick(_)) {
                    reverse_at = Some(tick);
                    swing_steer(true);
                }
            }
        }
        if let (Some(f), None, Init::LiftSwitchEnd(l)) = (flick_at, reverse_at, init) {
            if (tick - f) as f32 >= l * TPS as f32 {
                reverse_at = Some(tick);
                swing_steer(true);
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
                    Init::Lift(l) | Init::LiftSwitchEnd(l) => l,
                    Init::Reverse | Init::LiftFlick(_) => 0.0,
                };
                let thr = if ((tick - f) as f32) < l * TPS as f32 {
                    0.0
                } else {
                    1.0
                };
                (1.0, thr)
            }
            (Some(f), Some(r)) => {
                let thr = match init {
                    Init::LiftFlick(l) if ((tick - f) as f32) < l * TPS as f32 => 0.0,
                    _ => 1.0,
                };
                (if tick - r < gap_t { 0.0 } else { -1.0 }, thr)
            }
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
    // Grip-balance's "drift: best (test)" preset: VR steering + `GripBalance::drift_best()`.
    v.push(Config {
        name: "drift best · VR",
        plain: vr,
        ride: RideTune::lab_default(),
        grip: Some(GripBalance::drift_best()),
    });
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
    // `GRIP='{…}'`: fields merged over the configuration's grip balance (if it has one).
    let grip = match (cfg.grip, std::env::var("GRIP")) {
        (Some(g), Ok(j)) => Some(merged(&g, &j)),
        (g, _) => g,
    };
    grip_lab::set_override(grip);
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
        Ok("lift_then") => vec![Init::Lift(0.2), Init::Lift(0.4)],
        Ok("switch_end") => vec![
            Init::LiftSwitchEnd(0.15),
            Init::LiftSwitchEnd(0.25),
            Init::LiftSwitchEnd(0.4),
        ],
        Ok("all") => vec![
            Init::Reverse,
            Init::Lift(0.2),
            Init::Lift(0.4),
            Init::LiftFlick(0.15),
            Init::LiftFlick(0.25),
            Init::LiftFlick(0.4),
        ],
        // Default: the owner's flick (reference: a brief lift while switching keys).
        _ => vec![
            Init::LiftFlick(0.15),
            Init::LiftFlick(0.25),
            Init::LiftFlick(0.4),
        ],
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

/// One v2 candidate's score under one configuration.
#[derive(Debug, Clone, Copy, Default)]
struct V2Score {
    held: usize,
    caught: usize,
    flicks: usize,
    spun: usize,
    back: usize,
}

/// Held-drift robust set (grip_balance `robust`: rwd_d / a / b × counter key at 6 / 12 / 20° ×
/// 0.3 / 0.5 / 0.7 g, full throttle) + lift flicks at the reference gaps (0.05 / 0.15 s ×
/// lifts 0.15 / 0.25 / 0.4 s × 3 cars), under whatever overrides are active.
fn v2_score(cars: &[(&str, LoadedCar)]) -> V2Score {
    use wp_vehicle::drift_lab_w::{plain_hold, PlainDriver};
    let a = reference_assists();
    let mut sc = V2Score::default();
    for (_, c) in cars {
        for e in [6.0f32, 12.0, 20.0] {
            for g in [0.3f32, 0.5, 0.7] {
                let d = PlainDriver::HoldCounter {
                    entry_deg: e,
                    throttle: 1.0,
                };
                if plain_hold(c, &a, d, 40.0, g).held {
                    sc.held += 1;
                }
            }
        }
        for gap in [0.05f32, 0.15] {
            for l in [0.15f32, 0.25, 0.4] {
                sc.flicks += 1;
                if let Some(t) = flick(c, gap, Init::LiftFlick(l)) {
                    sc.caught += t.caught as usize;
                    sc.spun += t.spun as usize;
                    sc.back += t.swung_back as usize;
                }
            }
        }
    }
    sc
}

/// Small deterministic generator for the search (xorshift).
struct Xs(u64);
impl Xs {
    fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f()
    }
}

/// `v2search`: random search for "spin model v2 (ellipse)" (the spin model with the friction
/// ellipse on). `N` candidates (default 60), `SEED`, `V2CONFIGS` (config name substrings, comma
/// separated; default the three below). Prints every candidate's score per config as JSON
/// lines: `{"config", "held", "caught", "flicks", "spun", "back", "tune"}`; `V2BASE='{…}'`
/// sets fields every candidate starts from (refine around a winner with `V2SPREAD`, 0..1).
fn v2search(cars: &[(&str, LoadedCar)]) {
    let n: usize = std::env::var("N").ok().map_or(60, |v| v.parse().unwrap());
    let mut rng = Xs(std::env::var("SEED")
        .ok()
        .map_or(0x9E37_79B9_7F4A_7C15, |v| v.parse().unwrap()));
    let names: Vec<String> = std::env::var("V2CONFIGS")
        .unwrap_or_else(|_| "W defaults · plain,W defaults · VR,drift best · VR".into())
        .split(',')
        .map(String::from)
        .collect();
    let cfgs: Vec<Config> = configs()
        .into_iter()
        .filter(|c| names.iter().any(|n| c.name == n.as_str()))
        .collect();
    let base = match std::env::var("V2BASE") {
        Ok(j) => merged(&CarTune::spin_candidate(), &j),
        Err(_) => CarTune {
            spin_ellipse: 1.0,
            ..CarTune::spin_candidate()
        },
    };
    let spread: f32 = std::env::var("V2SPREAD")
        .ok()
        .map_or(1.0, |v| v.parse().unwrap());
    for i in 0..=n {
        // Candidate 0 = the base itself.
        let t = if i == 0 {
            base
        } else {
            let mut pick = |cur: f32, lo: f32, hi: f32| {
                let r = rng.range(lo, hi);
                (cur + (r - cur) * spread).clamp(lo, hi)
            };
            CarTune {
                spin_ellipse: pick(base.spin_ellipse, 0.5, 1.0),
                spin_fx_slide: pick(base.spin_fx_slide, 0.2, 1.0),
                spin_b0: pick(base.spin_b0, 1.0, 12.0),
                spin_b2: pick(base.spin_b2, 0.6, 2.5),
                spin_c: pick(base.spin_c, 0.5, 1.0),
                spin_p: pick(base.spin_p, 1.0, 8.0).round(),
                rear_relax: pick(base.rear_relax, 0.6, 2.5),
                ..base
            }
        };
        for cfg in &cfgs {
            apply(cfg);
            car_lab::set_override(Some(t));
            // V2STEER=1: also vary the VR steering of VR configurations (around the config's).
            let steer = if std::env::var("V2STEER").is_ok() && cfg.plain.velocity_relative && i > 0
            {
                let b = plain::active().unwrap();
                let mut pick = |cur: f32, lo: f32, hi: f32| {
                    let r = rng.range(lo, hi);
                    (cur + (r - cur) * spread).clamp(lo, hi)
                };
                let p = PlainSteer {
                    align_gain: pick(b.align_gain, 0.2, 1.0),
                    align_from_deg: pick(b.align_from_deg, 2.0, 15.0),
                    align_full_deg: pick(b.align_full_deg, 8.0, 30.0),
                    t_align_s: pick(b.t_align_s, 0.1, 0.5),
                    slide_key_gain: pick(b.slide_key_gain, 0.0, 1.0),
                    ..b
                };
                plain::set_override(Some(p));
                Some(p)
            } else {
                None
            };
            let sc = v2_score(cars);
            println!(
                "{}",
                serde_json::json!({
                    "config": cfg.name, "held": sc.held, "caught": sc.caught,
                    "flicks": sc.flicks, "spun": sc.spun, "back": sc.back, "tune": t,
                    "steer": steer,
                })
            );
        }
    }
}

/// `holds`: the held-drift set case by case (`ONLY` config, `TYRE`, `PLAIN` overrides).
fn holds(cars: &[(&str, LoadedCar)]) {
    use wp_vehicle::drift_lab_w::{plain_hold, PlainDriver};
    let a = reference_assists();
    for cfg in configs() {
        apply(&cfg);
        println!("## {}", cfg.name);
        for (id, c) in cars {
            for e in [6.0f32, 12.0, 20.0] {
                for g in [0.3f32, 0.5, 0.7] {
                    let d = PlainDriver::HoldCounter {
                        entry_deg: e,
                        throttle: 1.0,
                    };
                    let m = plain_hold(c, &a, d, 40.0, g);
                    println!("  {id} {e}° {g} g: {}", m.cell());
                }
            }
        }
    }
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("holds") {
        let cars: Vec<(&str, LoadedCar)> = CARS.iter().map(|id| (*id, car(id))).collect();
        holds(&cars);
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("v2search") {
        let cars: Vec<(&str, LoadedCar)> = CARS.iter().map(|id| (*id, car(id))).collect();
        v2search(&cars);
        return;
    }
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
