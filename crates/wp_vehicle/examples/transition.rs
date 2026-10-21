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
//! Configurations (ride: the panel default `RideTune::firm()`, step 20; the slow-weight
//! config keeps its own): `W defaults` (plain steering, spin model tyres), `balance new`
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
    /// Speed at the flick, km/h.
    v_flick: f32,
    /// The sign change was a swing (yaw toward the new side, ≥ 25 km/h), not a pivot at a crawl.
    real_swing: bool,
    /// Time from the flick (start of the lift) until the slide changes sign, s (the reference
    /// swing: 0.5–0.8 s).
    cross_s: Option<f32>,
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

/// How the flick driver enters the held drift (`ENTRY`).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Entry {
    /// Key into the turn + full throttle (default).
    Power,
    /// Key into the turn + handbrake, no throttle, for 0.4 s, then full throttle.
    Handbrake,
    /// Key into the turn with the throttle lifted for 0.4 s, then full throttle.
    Lift,
}

/// Old-side angle where a lifting driver reverses the keys, °.
const REACT_DEG: f32 = 5.0;

/// Run one flick from a held left drift. `None` = the drift never held 30–45° for 0.5 s.
fn flick(c: &LoadedCar, gap: f32, init: Init) -> Option<Transition> {
    let a = harness_assists();
    // Diagnostic (lead option 3): `RADIUS` / `LATG` for the settle circle (default 40 m,
    // 0.5 g), `FLICK_DEG` = flick as soon as the old-side slide first reaches it (no settled
    // 30–45° hold first).
    let env_f = |k: &str, d: f32| std::env::var(k).ok().map_or(d, |v| v.parse().expect(k));
    let mut rig = settle_on_circle(c, &a, env_f("RADIUS", 40.0), env_f("LATG", 0.5));
    // `HOLD_GEAR`: the gear the driver holds through the drift (default 2nd, the lab's).
    let hold_gear: i8 = std::env::var("HOLD_GEAR")
        .ok()
        .map_or(DRIFT_GEAR, |v| v.parse().expect("HOLD_GEAR"));
    let entry = match std::env::var("ENTRY").as_deref() {
        Ok("handbrake") => Entry::Handbrake,
        Ok("lift") => Entry::Lift,
        _ => Entry::Power,
    };
    let flick_deg = std::env::var("FLICK_DEG")
        .ok()
        .map(|v| v.parse::<f32>().expect("FLICK_DEG"));
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
            let ready = match flick_deg {
                Some(fd) => old >= fd,
                None => in_band >= TPS / 2,
            };
            if ready {
                flick_at = Some(tick);
                t.at_flick = old;
                t.v_flick = rig.state().speed_mps * 3.6;
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
            (None, _) if entered => (1.0, 1.0),
            // Entry (lead: low-power cars cannot break the rear loose on power alone):
            // `ENTRY=handbrake` = the key into the turn with the handbrake and no throttle for
            // the first 0.4 s, `ENTRY=lift` = the key with the throttle lifted for 0.4 s; then
            // full throttle until the slide passes 12°. Default `power`: full throttle.
            (None, _) => {
                let early = tick < (0.4 * TPS as f32) as usize;
                match entry {
                    Entry::Handbrake | Entry::Lift if early => (-1.0, 0.0),
                    _ => (-1.0, 1.0),
                }
            }
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
        let handbrake = if flick_at.is_none()
            && !entered
            && entry == Entry::Handbrake
            && tick < (0.4 * TPS as f32) as usize
        {
            1.0
        } else {
            0.0
        };
        rig.step(
            &VehicleInput {
                steer,
                throttle,
                handbrake,
                gear_down: !a.auto_gears && rig.vehicle.gear() > hold_gear,
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
                if t.cross_s.is_none() && b > 0.0 {
                    t.cross_s = Some(k as f32 / TPS as f32);
                    // A real swing: rotating toward the new side (yaw right, −) at speed. A car
                    // that slows to a crawl and pivots on the spot also flips the slip sign.
                    t.real_swing = n.yaw_rate < 0.0
                        && n.v_fwd.hypot(n.v_fwd * wp_vehicle::math::tan(n.body_slip)) * 3.6
                            >= 25.0;
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
        t.caught = !t.spun && !t.swung_back && t.real_swing && in_band * 2 >= remaining;
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
        ("W defaults", RideTune::firm(), None),
        (
            "balance new",
            RideTune::firm(),
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
    // Grip-balance's default "balance: grip" (`GripBalance::grip()`), both steerings.
    for (steer_name, steer) in [("plain", p), ("VR", vr)] {
        let full: &'static str = Box::leak(format!("balance grip · {steer_name}").into_boxed_str());
        v.push(Config {
            name: full,
            plain: steer,
            ride: RideTune::firm(),
            grip: Some(GripBalance::grip()),
        });
    }
    // The owner's target (lead, 2026-10-21): grip-balance 7d6b66c9 — step-14 balance, the medium
    // ride — with springs / bars × 1.5; plain steering. `· plain` with W's features as they are,
    // `· no W` with the transition rack / catch / over-rotation rack / countersteer limit off.
    let owner_ride = RideTune {
        spring: 1.5,
        arb: 1.5,
        ..RideTune::medium_weight()
    };
    v.push(Config {
        name: "owner · plain",
        plain: p,
        ride: owner_ride,
        grip: Some(GripBalance::step14()),
    });
    v.push(Config {
        name: "owner · no W",
        plain: PlainSteer {
            trans_follow: false,
            trans_catch_gain: 0.0,
            over_rate: 1.0,
            counter_margin_deg: 0.0,
            ..p
        },
        ride: owner_ride,
        grip: Some(GripBalance::step14()),
    });
    // Grip-balance's "drift: best (test)" preset: VR steering + `GripBalance::drift_best()`.
    v.push(Config {
        name: "drift best · VR",
        plain: vr,
        ride: RideTune::firm(),
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
    let a = harness_assists();
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

/// Lift-off in a normal corner (owner: "no side grip and step out, even when steering and
/// lifting off"): must **not** step out. At 40 / 60 / 80 mph: (a) lift at turn-in — the left
/// key + throttle 0 from straight cruise, 1.5 s; (b) lift mid-corner — key held 3 s at cruise,
/// then throttle 0 for 1.5 s. Returns the number of runs whose body slip passes 8° (or that
/// spin), out of 6, and the worst peak body slip.
fn lift_corner_fails(c: &LoadedCar) -> (usize, f32) {
    use wp_vehicle::rig::Rig;
    let a = harness_assists();
    let hold = |rig: &Rig, v: f32| {
        let err = v - rig.state().speed_mps;
        if err > -1.0 {
            ((0.25 + 0.35 * err).clamp(0.0, 1.0), 0.0)
        } else {
            (0.0, (-0.2 * err).clamp(0.0, 1.0))
        }
    };
    let rolling = |v: f32| {
        let mut rig = Rig::new(c);
        rig.teleport([0.0; 3], 0.0, v, 0.0);
        let auto = wp_contracts::AssistProfile {
            auto_gears: true,
            ..a
        };
        for _ in 0..TPS {
            let (throttle, brake) = hold(&rig, v);
            rig.step(
                &VehicleInput {
                    throttle,
                    brake,
                    ..Default::default()
                },
                &auto,
            );
        }
        rig
    };
    let (mut fails, mut worst) = (0usize, 0.0f32);
    for mph in [40.0f32, 60.0, 80.0] {
        let v = mph * 0.44704;
        for mid in [false, true] {
            let mut rig = rolling(v);
            let lift_at = if mid { 3 * TPS } else { 0 };
            let mut peak = 0.0f32;
            let mut spun = false;
            for tick in 0..lift_at + 3 * TPS / 2 {
                let (throttle, brake) = if tick >= lift_at {
                    (0.0, 0.0)
                } else {
                    hold(&rig, v)
                };
                rig.step(
                    &VehicleInput {
                        steer: -1.0,
                        throttle,
                        brake,
                        ..Default::default()
                    },
                    &a,
                );
                let st = slide_state(&rig);
                if tick >= lift_at {
                    peak = peak.max(-st.body_slip.to_degrees());
                    spun |= st.v_fwd < 0.0;
                }
            }
            worst = worst.max(peak);
            if std::env::var("LIFTV").is_ok() {
                eprintln!(
                    "  {} {mph} mph {}: peak body slip {peak:.1}°{}",
                    c.def.id,
                    if mid {
                        "mid-corner lift"
                    } else {
                        "lift at turn-in"
                    },
                    if spun { " SPUN" } else { "" }
                );
            }
            if peak > 8.0 || spun {
                fails += 1;
            }
        }
    }
    (fails, worst)
}

/// One v2 candidate's score under one configuration.
#[derive(Debug, Clone, Default)]
struct V2Score {
    held: usize,
    caught: usize,
    flicks: usize,
    spun: usize,
    back: usize,
    /// Held-set cases that swung through to the other side (W's over-catch) / spun.
    hold_swapped: usize,
    hold_spun: usize,
    /// Lift-off in a normal corner that stepped out (fail case), of 6 per car; worst slip °.
    lift_fails: usize,
    lift_worst: f32,
    /// Flicks caught with a reference-like swing: sign change 0.5–0.8 s after the lift starts
    /// and settled within 0.5 s of crossing onto the new side (no spin / swing-back).
    fast: usize,
    /// Median sign-change time of the caught flicks, s.
    cross_med: f32,
    crosses: Vec<f32>,
    at_flick: Vec<f32>,
    v_flick: Vec<f32>,
    /// Settle times of the caught flicks (9 = never settled), s.
    settles: Vec<f32>,
}

/// Held-drift robust set (grip_balance `robust`: rwd_d / a / b × counter key at 6 / 12 / 20° ×
/// 0.3 / 0.5 / 0.7 g, full throttle) + lift flicks at the reference gaps (0.05 / 0.15 s ×
/// lifts 0.15 / 0.25 / 0.4 s × 3 cars), under whatever overrides are active.
fn v2_score(cars: &[(&str, LoadedCar)]) -> V2Score {
    use wp_vehicle::drift_lab_w::{plain_hold, PlainDriver};
    let a = harness_assists();
    let mut sc = V2Score::default();
    for (_, c) in cars {
        let (lf, lw) = lift_corner_fails(c);
        sc.lift_fails += lf;
        sc.lift_worst = sc.lift_worst.max(lw);
        for e in [6.0f32, 12.0, 20.0] {
            for g in [0.3f32, 0.5, 0.7] {
                let d = PlainDriver::HoldCounter {
                    entry_deg: e,
                    throttle: 1.0,
                };
                let m = plain_hold(c, &a, d, 40.0, g);
                if m.held {
                    sc.held += 1;
                }
                if m.side_swaps > 0 {
                    sc.hold_swapped += 1;
                }
                if m.spun {
                    sc.hold_spun += 1;
                }
            }
        }
        for gap in [0.05f32, 0.15] {
            for l in [0.15f32, 0.25, 0.4] {
                sc.flicks += 1;
                if let Some(t) = flick(c, gap, Init::LiftFlick(l)) {
                    sc.caught += t.caught as usize;
                    sc.at_flick.push(t.at_flick);
                    sc.v_flick.push(t.v_flick);
                    if t.caught {
                        sc.settles.push(t.settle_s.unwrap_or(9.0));
                        if let Some(x) = t.cross_s {
                            sc.crosses.push(x);
                            if (0.5..=0.8).contains(&x) && t.settle_s.is_some_and(|s| s <= 0.5) {
                                sc.fast += 1;
                            }
                        }
                    }
                    sc.spun += t.spun as usize;
                    sc.back += t.swung_back as usize;
                }
            }
        }
    }
    let mut v = sc.crosses.clone();
    v.sort_by(|a, b| a.total_cmp(b));
    sc.cross_med = v.get(v.len() / 2).copied().unwrap_or(f32::NAN);
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
        // V2SETTLE=1 (new-side settle search): only the spin weighting, rear relax and rear peak
        // slip move (no ellipse / drive floor / front falloff), around V2BASE.
        let t = if std::env::var("V2SETTLE").is_ok() && i > 0 {
            let mut pick = |cur: f32, lo: f32, hi: f32| {
                let r = rng.range(lo, hi);
                (cur + (r - cur) * spread).clamp(lo, hi)
            };
            CarTune {
                spin_b0: pick(base.spin_b0, 2.0, 12.0),
                spin_b2: pick(base.spin_b2, 0.8, 3.0),
                spin_c: pick(base.spin_c, 0.5, 1.0),
                spin_p: pick(base.spin_p, 1.0, 8.0).round(),
                rear_relax: pick(base.rear_relax, 0.4, 2.0),
                rear_peak_slip: pick(base.rear_peak_slip, 0.6, 1.5),
                spin_ellipse: base.spin_ellipse,
                spin_fx_slide: base.spin_fx_slide,
                ..base
            }
        } else {
            t
        };
        // V2EXTRA=1: also the drift lock, the front deep-slip falloff and the rear peak slip.
        let t = if std::env::var("V2EXTRA").is_ok() && i > 0 {
            let mut pick = |cur: f32, lo: f32, hi: f32| {
                let r = rng.range(lo, hi);
                (cur + (r - cur) * spread.max(0.6)).clamp(lo, hi)
            };
            CarTune {
                drift_lock_extra: pick(t.drift_lock_extra, -0.6, 0.6),
                drift_lock_from_deg: pick(t.drift_lock_from_deg, 10.0, 35.0),
                front_deep_falloff: pick(t.front_deep_falloff, 0.0, 0.3),
                front_deep_from_deg: pick(t.front_deep_from_deg, 8.0, 25.0),
                front_deep_full_deg: pick(t.front_deep_full_deg, 30.0, 60.0),
                rear_peak_slip: pick(t.rear_peak_slip, 0.7, 1.6),
                ..t
            }
        } else {
            t
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
            } else if std::env::var("V2STEER").is_ok() && i > 0 {
                // Plain steering: the swing rack (off at 1).
                let b = plain::active().unwrap();
                let on = rng.f() < 0.5;
                let p = PlainSteer {
                    swing_rate: if on { rng.range(1.0, 3.0) } else { 1.0 },
                    swing_yaw_dps: rng.range(60.0, 120.0),
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
                    "hold_swapped": sc.hold_swapped, "hold_spun": sc.hold_spun,
                    "lift_fails": sc.lift_fails, "lift_worst": sc.lift_worst,
                    "fast": sc.fast, "cross_med": sc.cross_med,
                    "flick_deg_mean": sc.at_flick.iter().sum::<f32>() / sc.at_flick.len().max(1) as f32,
                    "flick_kmh_mean": sc.v_flick.iter().sum::<f32>() / sc.v_flick.len().max(1) as f32,
                    "crosses": sc.crosses, "settles": sc.settles,
                    "steer": steer,
                })
            );
        }
    }
}

/// `holds`: the held-drift set case by case (`ONLY` config, `TYRE`, `PLAIN` overrides).
fn holds(cars: &[(&str, LoadedCar)]) {
    use wp_vehicle::drift_lab_w::{plain_hold, PlainDriver};
    let a = harness_assists();
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

/// One power-on run: straight at `kmh` with cruise throttle in `gear` (auto gears to reach it,
/// then held), optionally the left key held 1 s first (in a corner), then full throttle for
/// 2.5 s with the key still held (corner) or none (straight). Returns (spun, peak |body slip| °).
/// Spun = |body slip| ≥ 45° or travelling backwards.
/// The harness's assists: the lab reference (TCS off, manual gears, auto clutch), or with
/// `MANUAL_CLUTCH=1` the owner's manual clutch held engaged (lead, step 23).
fn harness_assists() -> wp_contracts::AssistProfile {
    let a = reference_assists();
    if std::env::var("MANUAL_CLUTCH").is_ok() {
        wp_contracts::AssistProfile {
            auto_clutch: false,
            ..a
        }
    } else {
        a
    }
}

/// The assist profile of the power-on / held tables: the lab's reference (manual gears, auto
/// clutch), or with `OWNER=1` the owner's (lead, step 23: TCS off, **manual clutch, auto
/// gears**).
fn owner_assists() -> wp_contracts::AssistProfile {
    let a = reference_assists();
    if std::env::var("OWNER").is_ok() {
        wp_contracts::AssistProfile {
            auto_gears: true,
            auto_clutch: false,
            ..a
        }
    } else {
        a
    }
}

/// Gear changes logged by the power-on / held runs (`OWNER=1`): (seconds into the phase,
/// from, to).
static GEAR_LOG: std::sync::Mutex<Vec<(f32, i8, i8)>> = std::sync::Mutex::new(Vec::new());

fn power_on(c: &LoadedCar, kmh: f32, gear: i8, corner: bool) -> (bool, f32, Option<f32>, f32) {
    use wp_vehicle::rig::Rig;
    let a = owner_assists();
    let v = kmh / 3.6;
    let hold = |rig: &Rig| {
        let err = v - rig.state().speed_mps;
        if err > -1.0 {
            ((0.25 + 0.35 * err).clamp(0.0, 1.0), 0.0)
        } else {
            (0.0, (-0.2 * err).clamp(0.0, 1.0))
        }
    };
    let mut rig = Rig::new(c);
    rig.teleport([0.0; 3], 0.0, v, 0.0);
    // Settle 1.5 s at speed, stepping to the gear.
    for _ in 0..(3 * TPS / 2) {
        let (throttle, brake) = hold(&rig);
        let g = rig.vehicle.gear();
        rig.step(
            &VehicleInput {
                throttle,
                brake,
                gear_up: !a.auto_gears && g < gear,
                gear_down: !a.auto_gears && g > gear,
                ..Default::default()
            },
            &a,
        );
    }
    let steer = if corner { -1.0 } else { 0.0 };
    if corner {
        for _ in 0..TPS {
            let (throttle, brake) = hold(&rig);
            rig.step(
                &VehicleInput {
                    steer,
                    throttle,
                    brake,
                    ..Default::default()
                },
                &a,
            );
        }
    }
    let (mut spun, mut peak) = (false, 0.0f32);
    let (mut t20, mut t_spin) = (None, 9.0f32);
    let mut last_gear = rig.vehicle.gear();
    for tick in 0..(5 * TPS / 2) {
        let g = rig.vehicle.gear();
        if g != last_gear {
            GEAR_LOG
                .lock()
                .unwrap()
                .push((tick as f32 / TPS as f32, last_gear, g));
            last_gear = g;
        }
        rig.step(
            &VehicleInput {
                steer,
                throttle: 1.0,
                ..Default::default()
            },
            &a,
        );
        let s = slide_state(&rig);
        let b = s.body_slip.to_degrees().abs();
        peak = peak.max(b);
        if t20.is_none() && b >= 20.0 {
            t20 = Some(tick as f32 / TPS as f32);
        }
        if b >= 45.0 || s.v_fwd < 0.0 {
            spun = true;
            t_spin = tick as f32 / TPS as f32;
            break;
        }
    }
    (spun, peak, t20, t_spin)
}

/// `poweron`: the owner's "spinning on throttle / when holding a drift" (lead, step 23) — power-on
/// (TCS off, raw throttle; 2nd / 3rd from 30 / 45 / 60 km/h; straight and with the key held in
/// a corner) and the held-drift set, per car, for: `ref` (grip-balance 7d6b66c9: step-14 balance,
/// medium ride, spin model b0 7.5), `step23` (balance new, firm ride, b0 7.5) and `B` (balance
/// new, firm ride, candidate B). `CARS` overrides the car list; `PLAIN` merges steering fields.
fn poweron_table() {
    use wp_vehicle::drift_lab_w::{plain_drift_run_with, HoldMetrics, PlainDriver};
    let ids: Vec<String> = match std::env::var("CARS") {
        Ok(l) => l.split(',').map(String::from).collect(),
        Err(_) => [
            "rampart_50",
            "parity_rwd_a",
            "parity_rwd_b",
            "parity_rwd_d",
            "shiro_turbo",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
    };
    let sets: Vec<(&str, RideTune, GripBalance, CarTune)> = vec![
        (
            "ref 7d6b66c9",
            RideTune::medium_weight(),
            GripBalance::step14(),
            CarTune::spin_step22(),
        ),
        (
            "step23",
            RideTune::firm(),
            GripBalance::lab_default(),
            CarTune::spin_step22(),
        ),
        (
            "B",
            RideTune::firm(),
            GripBalance::lab_default(),
            CarTune::spin_candidate(),
        ),
        (
            "owner 7d6b66c9 + 1.5/1.5",
            RideTune {
                spring: 1.5,
                arb: 1.5,
                ..RideTune::medium_weight()
            },
            GripBalance::step14(),
            CarTune::spin_step22(),
        ),
        (
            "owner + B tyres",
            RideTune {
                spring: 1.5,
                arb: 1.5,
                ..RideTune::medium_weight()
            },
            GripBalance::step14(),
            CarTune::spin_candidate(),
        ),
        (
            "ref ride + new balance",
            RideTune::medium_weight(),
            GripBalance::lab_default(),
            CarTune::spin_step22(),
        ),
        (
            "firm ride + step-14 balance",
            RideTune::firm(),
            GripBalance::step14(),
            CarTune::spin_step22(),
        ),
    ];
    let sets: Vec<_> = match std::env::var("SETS") {
        Ok(f) => sets.into_iter().filter(|s| s.0 == f.as_str()).collect(),
        Err(_) => sets,
    };
    let p = match std::env::var("PLAIN") {
        Ok(j) => merged(&PlainSteer::default(), &j),
        Err(_) => PlainSteer::default(),
    };
    println!("| set | car | power-on spins / 12 (straight / corner) | corner: median s to 20° slip / to spin | worst peak slip ° (straight / corner) | held drift / 9 (spun) | power-on gear changes | held-drift gear changes: first few |");
    println!("|---|---|---|---|---|---|---|---|");
    let a = owner_assists();
    for (name, ride, grip, tyre) in &sets {
        plain::set_override(Some(p));
        ride_lab::set_override(Some(*ride));
        grip_lab::set_override(Some(*grip));
        car_lab::set_override(Some(*tyre));
        for id in &ids {
            let c = car(id);
            let (mut ss, mut sc, mut ps, mut pc) = (0, 0, 0.0f32, 0.0f32);
            let (mut t20s, mut tsp) = (Vec::new(), Vec::new());
            GEAR_LOG.lock().unwrap().clear();
            for gear in [2i8, 3] {
                for kmh in [30.0f32, 45.0, 60.0] {
                    let (s1, p1, _, _) = power_on(&c, kmh, gear, false);
                    let (s2, p2, t20, ts) = power_on(&c, kmh, gear, true);
                    ss += s1 as usize;
                    sc += s2 as usize;
                    ps = ps.max(p1);
                    pc = pc.max(p2);
                    if let Some(t) = t20 {
                        t20s.push(t);
                    }
                    if s2 {
                        tsp.push(ts);
                    }
                }
            }
            let med = |mut v: Vec<f32>| {
                v.sort_by(|a, b| a.total_cmp(b));
                v.get(v.len() / 2)
                    .map_or("—".to_string(), |x| format!("{x:.2}"))
            };
            let (mut held, mut spun) = (0, 0);
            let mut held_shifts = 0usize;
            let mut held_shift_log: Vec<String> = Vec::new();
            for e in [6.0f32, 12.0, 20.0] {
                for g in [0.3f32, 0.5, 0.7] {
                    let (mut ang, mut back) = (Vec::new(), Vec::new());
                    let mut lg: Option<i8> = None;
                    let mut t = 0usize;
                    plain_drift_run_with(
                        &c,
                        &a,
                        PlainDriver::HoldCounter {
                            entry_deg: e,
                            throttle: 1.0,
                        },
                        40.0,
                        g,
                        |rig, _, _| {
                            let st = slide_state(rig);
                            ang.push(-st.body_slip.to_degrees());
                            back.push(st.v_fwd < 0.0);
                            let gg = rig.vehicle.gear();
                            if let Some(l) = lg {
                                if gg != l {
                                    held_shifts += 1;
                                    if held_shift_log.len() < 6 {
                                        held_shift_log.push(format!(
                                            "{e}°/{g}g @{:.1}s {l}→{gg} slip {:.0}°",
                                            t as f32 / TPS as f32,
                                            ang.last().unwrap()
                                        ));
                                    }
                                }
                            }
                            lg = Some(gg);
                            t += 1;
                        },
                    );
                    let m = HoldMetrics::from_angles(&ang, &back);
                    held += m.held as usize;
                    spun += m.spun as usize;
                }
            }
            println!(
                "| {name} | {id} | {} ({ss} / {sc}) | {} / {} | {ps:.0} / {pc:.0} | {held} ({spun} spun) | {} | {held_shifts}: {} |",
                ss + sc,
                med(t20s),
                med(tsp),
                GEAR_LOG.lock().unwrap().len(),
                held_shift_log.join(", ")
            );
        }
    }
}

/// Manual-gear shift in a held drift (lead, step 23: the owner drives manual gears + manual
/// clutch with no clutch key, so the clutch stays engaged through every shift).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shift {
    None,
    /// One upshift when the engine reaches the rev limiter mid-drift.
    UpAtLimiter,
    /// Enter in 3rd and shift down to 2nd as the key goes into the turn.
    DownAtEntry,
}

/// One held drift (40 m circle at `lat_g`, key into the turn to 6°, then the counter key +
/// full throttle, 6 s) with manual gears, the clutch held engaged (input 0) and the `shift`.
/// Returns (held metrics, spun, a log of the 0.6 s around the shift: (t, gear, rpm, rear wheel
/// speed km/h, car speed km/h, slide °)).
/// One logged tick of [`manual_drift`]: (t, gear, rpm, rear wheel speed km/h, car speed km/h,
/// slide °).
type ShiftRow = (f32, i8, f32, f32, f32, f32);

fn manual_drift(
    c: &LoadedCar,
    lat_g: f32,
    shift: Shift,
) -> (
    wp_vehicle::drift_lab_w::HoldMetrics,
    Option<f32>,
    Vec<ShiftRow>,
) {
    use wp_vehicle::drift_lab_w::HoldMetrics;
    // `AUTO_CLUTCH=1`: the lab's auto clutch instead (comparison).
    let a = wp_contracts::AssistProfile {
        auto_gears: false,
        auto_clutch: std::env::var("AUTO_CLUTCH").is_ok(),
        ..harness_assists()
    };
    let mut rig = settle_on_circle(c, &a, 40.0, lat_g);
    let start_gear: i8 = if shift == Shift::DownAtEntry { 3 } else { 2 };
    // Into the start gear (shifts while settled, before the drift).
    for _ in 0..TPS {
        let g = rig.vehicle.gear();
        let (throttle, brake) = {
            let err = (lat_g * 9.81 * 40.0).sqrt() - rig.state().speed_mps;
            if err > -1.0 {
                ((0.25 + 0.35 * err).clamp(0.0, 1.0), 0.0)
            } else {
                (0.0, (-0.2 * err).clamp(0.0, 1.0))
            }
        };
        rig.step(
            &VehicleInput {
                throttle,
                brake,
                gear_up: g < start_gear,
                gear_down: g > start_gear,
                ..Default::default()
            },
            &a,
        );
    }
    let limiter = c.def.engine.rev_limit_rpm;
    let (mut ang, mut back) = (Vec::new(), Vec::new());
    let mut entered = false;
    let mut shift_at: Option<usize> = None;
    let mut log = Vec::new();
    let mut hist: Vec<ShiftRow> = Vec::new();
    for tick in 0..6 * TPS {
        let st = slide_state(&rig);
        let slide = -st.body_slip.to_degrees();
        entered |= slide > 6.0;
        let steer = if entered { 1.0 } else { -1.0 };
        let rpm = rig.state().engine_rpm;
        let (mut up, mut down) = (false, false);
        if shift_at.is_none() {
            match shift {
                Shift::UpAtLimiter if entered && rpm >= limiter - 150.0 => up = true,
                Shift::DownAtEntry if tick == 0 => down = true,
                _ => {}
            }
            if up || down {
                shift_at = Some(tick);
            }
        }
        rig.step(
            &VehicleInput {
                steer,
                throttle: 1.0,
                clutch: 0.0,
                gear_up: up,
                gear_down: down,
                ..Default::default()
            },
            &a,
        );
        let n = slide_state(&rig);
        let w = &rig.state().wheel_states;
        let r = c.def.wheels[2].radius_m;
        let rear_kmh = 0.5 * (w[2].omega + w[3].omega) * r * 3.6;
        let row = (
            tick as f32 / TPS as f32,
            rig.vehicle.gear(),
            rig.state().engine_rpm,
            rear_kmh,
            rig.state().speed_mps * 3.6,
            -n.body_slip.to_degrees(),
        );
        hist.push(row);
        ang.push(-n.body_slip.to_degrees());
        back.push(n.v_fwd < 0.0);
    }
    if let Some(sa) = shift_at {
        let from = sa.saturating_sub(TPS / 10);
        let to = (sa + TPS / 2).min(hist.len());
        log = hist[from..to].iter().step_by(TPS / 20).copied().collect();
    }
    (
        HoldMetrics::from_angles(&ang, &back),
        shift_at.map(|t| t as f32 / TPS as f32),
        log,
    )
}

/// `manual`: [`manual_drift`] per set (ref 7d6b66c9 / step23) × car × lat g × shift.
fn manual_table() {
    let ids: Vec<String> = match std::env::var("CARS") {
        Ok(l) => l.split(',').map(String::from).collect(),
        Err(_) => ["parity_rwd_a", "parity_rwd_b", "parity_rwd_d", "rampart_50"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    };
    let sets: Vec<(&str, RideTune, GripBalance, CarTune)> = vec![
        (
            "ref 7d6b66c9",
            RideTune::medium_weight(),
            GripBalance::step14(),
            CarTune::spin_step22(),
        ),
        (
            "step23",
            RideTune::firm(),
            GripBalance::lab_default(),
            CarTune::spin_step22(),
        ),
    ];
    let verbose = std::env::var("VERBOSE").is_ok();
    println!("| set | car | no shift: held / 3 (spun) | upshift at the limiter: held / 3 (spun), shifted at s | downshift 3→2 at entry: held / 3 (spun) |");
    println!("|---|---|---|---|---|");
    for (name, ride, grip, tyre) in &sets {
        plain::set_override(Some(PlainSteer::default()));
        ride_lab::set_override(Some(*ride));
        grip_lab::set_override(Some(*grip));
        car_lab::set_override(Some(*tyre));
        for id in &ids {
            let c = car(id);
            let mut cells = Vec::new();
            for shift in [Shift::None, Shift::UpAtLimiter, Shift::DownAtEntry] {
                let (mut held, mut spun, mut at) = (0, 0, Vec::new());
                for g in [0.3f32, 0.5, 0.7] {
                    let (m, t, log) = manual_drift(&c, g, shift);
                    held += m.held as usize;
                    spun += m.spun as usize;
                    if let Some(t) = t {
                        at.push(format!("{t:.1}"));
                    }
                    if verbose && !log.is_empty() {
                        eprintln!("{name} {id} {shift:?} {g} g:");
                        for (t, gear, rpm, rk, vk, sl) in &log {
                            eprintln!("  t {t:.2} gear {gear} rpm {rpm:.0} rear wheels {rk:.0} km/h car {vk:.0} km/h slide {sl:+.0}°");
                        }
                    }
                }
                cells.push(if at.is_empty() {
                    format!("{held} ({spun})")
                } else {
                    format!("{held} ({spun}) @ {}", at.join("/"))
                });
            }
            println!(
                "| {name} | {id} | {} | {} | {} |",
                cells[0], cells[1], cells[2]
            );
        }
    }
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("manual") {
        manual_table();
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("poweron") {
        poweron_table();
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("holds") {
        let ids: Vec<String> = match std::env::var("CARS") {
            Ok(l) => l.split(',').map(String::from).collect(),
            Err(_) => CARS.iter().map(|s| s.to_string()).collect(),
        };
        let cars: Vec<(&str, LoadedCar)> = ids.iter().map(|id| (id.as_str(), car(id))).collect();
        holds(&cars);
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("v2search") {
        let ids: Vec<String> = match std::env::var("CARS") {
            Ok(l) => l.split(',').map(String::from).collect(),
            Err(_) => CARS.iter().map(|s| s.to_string()).collect(),
        };
        let cars: Vec<(&str, LoadedCar)> = ids.iter().map(|id| (id.as_str(), car(id))).collect();
        v2search(&cars);
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("trace") {
        let ids: Vec<String> = match std::env::var("CARS") {
            Ok(l) => l.split(',').map(String::from).collect(),
            Err(_) => CARS.iter().map(|s| s.to_string()).collect(),
        };
        let cars: Vec<(&str, LoadedCar)> = ids.iter().map(|id| (id.as_str(), car(id))).collect();
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
