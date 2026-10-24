//! Seeded multi-parameter search for a held countersteer drift (lead, 2026-10-22; owner rows
//! 141 / 142). The reference (docs/reference/wf-clips, tools/ref-clips) holds a drift for 3–8 s
//! with the countersteer key held at full lock and the throttle lifted 1–7 times; ours swaps
//! sides after 2–4 s. Samples tyre / balance / inertia parameters and scores each candidate.
//!
//! Score (higher = better):
//! - `held_s`: mean time a held countersteer key + throttle worked by the slide angle holds the
//!   drift before the car swaps sides or spins (cap 6 s): 6 cars × (40 m at 0.5 g, 80 m at
//!   0.7 g), handbrake entry, the shipped player steering;
//! - `yaw_ok`: share of that held time with the yaw rate at 10–60 °/s (the clips' holds).
//!
//! Guard rails (reported; a candidate that breaks them is marked):
//! - taps: the tapping driver's held entries (lift and handbrake, 6 cars × 0.3 / 0.5 / 0.7 g);
//! - handling: cars outside `tests/handling_balance.rs`'s understeer-gradient ranges;
//! - lock at speed: the front wheels' angle a held steer key reaches at 60 / 100 / 140 km/h
//!   (6 cars, keyboard assists) must stay ≥ 90 % of today's (owner, preview 0.12.49: a held
//!   key that cannot reach full lock is not acceptable).
//!
//! AI pace and goldens are checked on the shortlist only (too slow for every candidate).
//! `N=150 SEED=1 cargo run --release -p wp_vehicle --example held_search`; `ONLY_BASE=1` scores
//! the shipped set alone. The steering lock and the player rack are fixed (never searched):
//! the shipped rack, or with `RACK=full` the shipped rack without its speed reduction (full
//! lock at every speed, player cars only), or `RACK=ref` the reference clips' curve (98 / 92 /
//! 75 % of lock at 60 / 100 / 140 km/h). Searched: spin tyre, side floor, rear relax /
//! peak slip, grip balance stiffness, yaw inertia, springs, anti-roll bars, roll inertia; with
//! `FRONT=1` also the front tyres' shape (peak slip, past-peak `C`, deep-slip falloff).

use std::path::PathBuf;

use wp_contracts::{AssistProfile, LoadedCar, VehicleInput, PHYSICS_DT};
use wp_vehicle::car_lab::{self, CarTune};
use wp_vehicle::drift_lab::{settle_on_circle, slide_state, DRIFT_GEAR};
use wp_vehicle::drift_lab_w::{load_any_car, reference_assists, HoldMetrics};
use wp_vehicle::grip_lab::{self, GripBalance};
use wp_vehicle::handling::{presets, ramp_steer};
use wp_vehicle::ride_lab::{self, RideTune};

const TPS: usize = 240;
const DRIFT_CARS: [&str; 6] = [
    "rampart_50",
    "parity_rwd_a",
    "parity_rwd_b",
    "parity_rwd_d",
    "parity_rwd_e",
    "parity_rwd_f",
];
/// (id, understeer gradient range) from tests/handling_balance.rs.
const HANDLING: [(&str, f32, f32); 17] = [
    ("vireo_xs", 0.25, 2.0),
    ("kestrel_ts", 0.25, 2.0),
    ("junkyard_beater", 0.25, 2.0),
    ("tansy_13", 0.25, 2.0),
    ("shiro_turbo", -0.95, 0.6),
    ("hakone_gt", -0.95, 0.6),
    ("rampart_50", -0.95, 0.6),
    ("sperber_32", -0.95, 0.6),
    ("lucciola_350", -0.95, 0.6),
    ("sentinel_cruiser", -0.95, 0.6),
    ("lindwurm_4t", 0.1, 0.6),
    ("sentinel_wolfhound", 0.1, 0.6),
    ("hauler_1500", -0.9, 2.3),
    ("ironside_440", -0.9, 2.3),
    ("drayman_cargo", -0.9, 2.3),
    ("furrow_24", -0.9, 2.3),
    ("sentinel_bastion", -0.9, 2.3),
];

fn data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

/// The searched parameters.
#[derive(Debug, Clone, Copy)]
struct Params {
    spin_b0: f32,
    spin_b2: f32,
    spin_c: f32,
    side_floor: f32,
    rear_relax: f32,
    rear_peak_slip: f32,
    lock: f32,
    rear_stiffness: f32,
    front_stiffness: f32,
    yaw_inertia: f32,
    spring: f32,
    arb: f32,
    roll_inertia: f32,
    /// Front tyre shape (`FRONT=1` searches it): lateral peak slip scale, Magic Formula `C`
    /// override (0 = data), deep-slip falloff (fraction) from / to front slip, °.
    front_peak_slip: f32,
    front_lat_c: f32,
    front_deep_falloff: f32,
    front_deep_from_deg: f32,
    front_deep_full_deg: f32,
}

impl Params {
    fn shipped() -> Self {
        let c = CarTune::shipped();
        let g = GripBalance::shipped();
        Self {
            spin_b0: c.spin_b0,
            spin_b2: c.spin_b2,
            spin_c: c.spin_c,
            side_floor: c.side_floor,
            rear_relax: c.rear_relax,
            rear_peak_slip: c.rear_peak_slip,
            lock: c.lock, // fixed: never searched
            rear_stiffness: g.rear_stiffness,
            front_stiffness: g.front_stiffness,
            yaw_inertia: RideTune::shipped().yaw_inertia,
            spring: RideTune::shipped().spring,
            arb: RideTune::shipped().arb,
            roll_inertia: RideTune::shipped().roll_inertia,
            front_peak_slip: c.front_peak_slip,
            front_lat_c: c.front_lat_c,
            front_deep_falloff: c.front_deep_falloff,
            front_deep_from_deg: c.front_deep_from_deg,
            front_deep_full_deg: c.front_deep_full_deg,
        }
    }

    /// A random candidate (`u` = uniform 0..1 draws). The steering lock and the player rack
    /// are not searched (lead, 2026-10-22: a held key keeps its lock at every speed).
    fn sample(u: &mut impl FnMut() -> f32) -> Self {
        let b = Self::shipped();
        let mut r = |lo: f32, hi: f32| lo + (hi - lo) * u();
        let front = std::env::var_os("FRONT").is_some();
        let mut p = Self {
            spin_b0: r(5.0, 14.0),
            spin_b2: r(1.2, 3.0),
            spin_c: r(0.7, 1.0),
            side_floor: r(0.0, 0.6),
            rear_relax: r(0.8, 2.0),
            rear_peak_slip: r(0.9, 1.5),
            rear_stiffness: r(0.45, 1.0),
            front_stiffness: r(0.5, 1.0),
            yaw_inertia: r(0.9, 1.6),
            spring: r(1.0, 2.0),
            arb: r(0.5, 2.0),
            roll_inertia: r(1.0, 2.6),
            ..b
        };
        if front {
            // How the reference may limit the front tyres (not the wheel angle) once the
            // wheels point past the travel.
            p.front_peak_slip = r(0.8, 1.6);
            p.front_lat_c = if r(0.0, 1.0) < 0.5 { 0.0 } else { r(1.0, 1.5) };
            p.front_deep_falloff = r(0.0, 0.6);
            p.front_deep_from_deg = r(5.0, 20.0);
            p.front_deep_full_deg = p.front_deep_from_deg + r(5.0, 25.0);
        }
        p
    }

    fn apply(&self) {
        car_lab::set_override(Some(CarTune {
            spin_b0: self.spin_b0,
            spin_b2: self.spin_b2,
            spin_c: self.spin_c,
            side_floor: self.side_floor,
            rear_relax: self.rear_relax,
            rear_peak_slip: self.rear_peak_slip,
            lock: self.lock,
            front_peak_slip: self.front_peak_slip,
            front_lat_c: self.front_lat_c,
            front_deep_falloff: self.front_deep_falloff,
            front_deep_from_deg: self.front_deep_from_deg,
            front_deep_full_deg: self.front_deep_full_deg,
            ..CarTune::shipped()
        }));
        grip_lab::set_override(Some(GripBalance {
            rear_stiffness: self.rear_stiffness,
            front_stiffness: self.front_stiffness,
            ..GripBalance::shipped()
        }));
        ride_lab::set_override(Some(RideTune {
            yaw_inertia: self.yaw_inertia,
            spring: self.spring,
            arb: self.arb,
            roll_inertia: self.roll_inertia,
            // TRAVEL / DBUMP: suspension travel / bump damper scale over the shipped ride.
            travel: std::env::var("TRAVEL")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(RideTune::shipped().travel),
            damper_bump: std::env::var("DBUMP")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(RideTune::shipped().damper_bump),
            ..RideTune::shipped()
        }));
    }

    /// The player rack: the shipped one, `RACK=ref` the reference curve, or with `RACK=full` the shipped one without its
    /// speed reduction (full lock at every speed; player cars only). Set after a run's settle,
    /// which drives a bare rig that a plain-steering override would reach too.
    fn player_rack(&self) -> wp_vehicle::plain::PlainSteer {
        let p = wp_vehicle::lab_scope::shipped_plain();
        match std::env::var("RACK").as_deref() {
            Ok("full") => wp_vehicle::plain::PlainSteer {
                speed_sensitivity: 0.0,
                ..p
            },
            // The reference clips' curve (tune panel "rack · reference").
            Ok("ref") => wp_vehicle::plain::PlainSteer {
                sens_ref_kmh: 172.0,
                sens_exponent: 4.0,
                ..p
            },
            _ => p,
        }
    }
}

/// Held countersteer + throttle worked by the slide angle: (held s until swap / spin, cap 6;
/// held s with the yaw rate in 10–60 °/s).
fn held(
    c: &LoadedCar,
    a: &AssistProfile,
    rack: wp_vehicle::plain::PlainSteer,
    radius: f32,
    lat_g: f32,
) -> (f32, f32) {
    let mut rig = settle_on_circle(c, a, radius, lat_g);
    rig.set_player_input(true);
    wp_vehicle::plain::set_override(Some(rack));
    let r = held_run(&mut rig, a);
    wp_vehicle::plain::set_override(None);
    r
}

fn held_run(rig: &mut wp_vehicle::rig::Rig, a: &AssistProfile) -> (f32, f32) {
    let (mut entered, mut entered_at, mut thr_on, mut prev) = (false, 0.0f32, true, 0.0f32);
    let mut yaw_ok = 0.0f32;
    for k in 0..9 * TPS {
        let t = k as f32 * PHYSICS_DT;
        let s = slide_state(rig);
        let angle = (-s.body_slip).to_degrees();
        if !entered && angle > 12.0 {
            entered = true;
            entered_at = t;
        }
        let (steer, throttle, hb) = if !entered {
            (-1.0, 1.0, if t < 0.3 { 1.0 } else { 0.0 })
        } else {
            let rate = (angle - prev) / PHYSICS_DT;
            thr_on = if angle > 45.0 || rate > 40.0 {
                false
            } else if angle < 30.0 || rate < -40.0 {
                true
            } else {
                thr_on
            };
            (1.0, if thr_on { 1.0 } else { 0.0 }, 0.0)
        };
        prev = angle;
        if entered {
            let lost = angle < -10.0 || angle.abs() >= 90.0 || s.v_fwd < 0.0;
            let held_s = t - entered_at;
            if lost || held_s >= 6.0 {
                if std::env::var_os("ENDS").is_some() {
                    let why = if held_s >= 6.0 {
                        "held"
                    } else if angle < -10.0 {
                        "swap"
                    } else if s.v_fwd < 0.0 {
                        "backward"
                    } else {
                        "spin"
                    };
                    eprintln!("end {why} {held_s:.2}");
                }
                return (held_s.min(6.0), yaw_ok);
            }
            if (10.0..=60.0).contains(&s.yaw_rate.to_degrees().abs()) {
                yaw_ok += PHYSICS_DT;
            }
        }
        rig.step(
            &VehicleInput {
                steer,
                throttle,
                handbrake: hb,
                gear_down: !a.auto_gears && rig.vehicle.gear() > DRIFT_GEAR,
                ..Default::default()
            },
            a,
        );
    }
    (0.0, yaw_ok)
}

/// Lock-at-speed guard rail (lead, 2026-10-22): the front wheels' angle a held steer key
/// reaches (max over 0.6 s from a straight run at `kmh`, keyboard assists, player car), as a
/// share of the car's full lock. `rack` = the player steering (`None` = shipped).
fn lock_at(c: &LoadedCar, rack: Option<wp_vehicle::plain::PlainSteer>, kmh: f32) -> f32 {
    let a = AssistProfile::keyboard_default();
    let full = c
        .def
        .wheels
        .iter()
        .filter(|w| w.steerable)
        .map(|w| w.max_steer_rad)
        .fold(0.0f32, f32::max);
    let mut rig = wp_vehicle::rig::Rig::new(c);
    rig.teleport([0.0; 3], 0.0, kmh / 3.6, 0.0);
    rig.set_player_input(true);
    wp_vehicle::plain::set_override(rack);
    let mut best = 0.0f32;
    for _ in 0..(0.6 * TPS as f32) as usize {
        rig.step(
            &VehicleInput {
                steer: -1.0,
                throttle: 0.3,
                ..Default::default()
            },
            &a,
        );
        best = best.max(rig.vehicle.telemetry().steer_rad.abs());
    }
    wp_vehicle::plain::set_override(None);
    best / full
}

/// Mean [`lock_at`] over the drift cars at 60 / 100 / 140 km/h.
fn locks(drift: &[LoadedCar], rack: Option<wp_vehicle::plain::PlainSteer>) -> [f32; 3] {
    let mut out = [0.0; 3];
    for (i, v) in [60.0, 100.0, 140.0].into_iter().enumerate() {
        out[i] = drift.iter().map(|c| lock_at(c, rack, v)).sum::<f32>() / drift.len() as f32;
    }
    out
}

/// The tapping driver (examples/entry_hold.rs, `Hold::Tapper`): held entry?
fn tapped(
    c: &LoadedCar,
    a: &AssistProfile,
    rack: wp_vehicle::plain::PlainSteer,
    handbrake: bool,
    lat_g: f32,
) -> bool {
    let mut rig = settle_on_circle(c, a, 40.0, lat_g);
    rig.set_player_input(true);
    wp_vehicle::plain::set_override(Some(rack));
    let r = tapped_run(&mut rig, a, handbrake);
    wp_vehicle::plain::set_override(None);
    r
}

fn tapped_run(rig: &mut wp_vehicle::rig::Rig, a: &AssistProfile, handbrake: bool) -> bool {
    let (mut angles, mut back) = (Vec::new(), Vec::new());
    let mut entered = false;
    let mut prev = (-slide_state(rig).body_slip).to_degrees();
    for k in 0..6 * TPS {
        let t = k as f32 * PHYSICS_DT;
        let angle = (-slide_state(rig).body_slip).to_degrees();
        let rate = (angle - prev) / PHYSICS_DT;
        prev = angle;
        entered |= angle > 12.0;
        let (steer, throttle, hb) = if !entered {
            if handbrake {
                (-1.0, 1.0, if t < 0.3 { 1.0 } else { 0.0 })
            } else {
                (-1.0, if t < 0.4 { 0.0 } else { 1.0 }, 0.0)
            }
        } else {
            let steer = if angle > 35.0 || rate > 15.0 {
                1.0
            } else if angle < 20.0 {
                -1.0
            } else {
                0.0
            };
            (steer, if angle > 45.0 { 0.0 } else { 1.0 }, 0.0)
        };
        rig.step(
            &VehicleInput {
                steer,
                throttle,
                handbrake: hb,
                gear_down: !a.auto_gears && rig.vehicle.gear() > DRIFT_GEAR,
                ..Default::default()
            },
            a,
        );
        let n = slide_state(rig);
        angles.push(-n.body_slip.to_degrees());
        back.push(n.v_fwd < 0.0);
    }
    HoldMetrics::from_angles(&angles, &back).held
}

/// One candidate's scores.
#[derive(Debug, Clone, Copy)]
struct Score {
    held_s: f32,
    yaw_ok: f32,
    taps: usize,
    handling_fails: usize,
    /// [`locks`]: the held key's wheel angle at 60 / 100 / 140 km/h, share of full lock.
    lock: [f32; 3],
}

fn score(p: &Params, drift: &[LoadedCar], hand: &[(LoadedCar, f32, f32)]) -> Score {
    p.apply();
    // TCS=half|full and CS=<0..1> (hands-off countersteer help) for the row-148 assist levers;
    // default: the reference settings (TCS Off, no help).
    let mut a = reference_assists();
    match std::env::var("TCS").as_deref() {
        Ok("half") => a.tcs = wp_contracts::TcsLevel::Half,
        Ok("full") => a.tcs = wp_contracts::TcsLevel::Full,
        _ => {}
    }
    if let Some(cs) = std::env::var("CS").ok().and_then(|v| v.parse::<f32>().ok()) {
        a.countersteer_help = cs.clamp(0.0, 1.0);
    }
    eprintln!(
        "assists: tcs {:?}, countersteer_help {}",
        a.tcs, a.countersteer_help
    );
    let (mut h, mut y, mut n) = (0.0, 0.0, 0.0);
    for c in drift {
        for (r, g) in [(40.0, 0.5), (80.0, 0.7)] {
            let (hs, ys) = held(c, &a, p.player_rack(), r, g);
            h += hs;
            y += ys;
            n += 1.0;
        }
    }
    let mut taps = 0;
    for c in drift {
        for hb in [false, true] {
            for g in [0.3, 0.5, 0.7] {
                taps += tapped(c, &a, p.player_rack(), hb, g) as usize;
            }
        }
    }
    // The handling check drives bare rigs, which a plain-steering override would reach too:
    // measure it on the shipped steering path (the player rack only acts on player cars).
    wp_vehicle::plain::set_override(None);
    let wheel = presets()[2].1;
    let handling_fails = hand
        .iter()
        .filter(|(c, lo, hi)| {
            let u = ramp_steer(c, &wheel, 80.0 / 3.6).understeer_deg_per_g;
            !(*lo..=*hi).contains(&u)
        })
        .count();
    Score {
        held_s: h / n,
        yaw_ok: if h > 0.0 { y / h } else { 0.0 },
        taps,
        handling_fails,
        lock: locks(drift, Some(p.player_rack())),
    }
}

fn main() {
    let n: usize = std::env::var("N").ok().map_or(150, |v| v.parse().unwrap());
    let mut seed: u64 = std::env::var("SEED").ok().map_or(1, |v| v.parse().unwrap());
    // splitmix64: deterministic, no extra dependency.
    let mut u = move || {
        seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = seed;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 40) as f32 / (1u64 << 24) as f32
    };
    let drift: Vec<LoadedCar> = DRIFT_CARS
        .iter()
        .map(|id| load_any_car(&data(), id).unwrap())
        .collect();
    let hand: Vec<(LoadedCar, f32, f32)> = HANDLING
        .iter()
        .map(|(id, lo, hi)| (load_any_car(&data(), id).unwrap(), *lo, *hi))
        .collect();
    let base = score(&Params::shipped(), &drift, &hand);
    println!("base {base:?} {:?}", Params::shipped());
    Params::shipped().apply();
    let today = locks(drift.as_slice(), None);
    let full = wp_vehicle::plain::PlainSteer {
        speed_sensitivity: 0.0,
        ..wp_vehicle::lab_scope::shipped_plain()
    };
    println!(
        "lock at 60 / 100 / 140 km/h (share of full lock): shipped {:?}, no speed reduction {:?}",
        today,
        locks(&drift, Some(full))
    );
    if std::env::var_os("ONLY_BASE").is_some() {
        return;
    }
    // CAND=G / G_firm: score one named candidate (G = grip-balance's best inside every guard rail,
    // the owner's blind pick, row physics-4; G_firm = G's tyres with the owner's springs / bars).
    if let Ok(name) = std::env::var("CAND") {
        let g = Params {
            spin_b0: 6.48,
            spin_b2: 1.75,
            spin_c: 0.90,
            side_floor: 0.38,
            rear_relax: 1.41,
            rear_peak_slip: 1.10,
            rear_stiffness: 0.533,
            front_stiffness: 0.666,
            yaw_inertia: 1.35,
            spring: 1.04,
            arb: 0.63,
            roll_inertia: 1.34,
            ..Params::shipped()
        };
        let p = match name.as_str() {
            "G" => g,
            "G_firm" => Params {
                spring: 1.5,
                arb: 1.5,
                roll_inertia: Params::shipped().roll_inertia,
                ..g
            },
            // G's tyres and grip balance only; the shipped ride and inertia (AI-safest).
            "G_tyre" => Params {
                yaw_inertia: Params::shipped().yaw_inertia,
                spring: Params::shipped().spring,
                arb: Params::shipped().arb,
                roll_inertia: Params::shipped().roll_inertia,
                ..g
            },
            other => panic!("unknown CAND {other}"),
        };
        // G_YAW / G_SPRING / G_ARB / G_ROLL: override one of the named candidate's ride values.
        let env = |k: &str, v: f32| {
            std::env::var(k)
                .ok()
                .and_then(|x| x.parse().ok())
                .unwrap_or(v)
        };
        let p = Params {
            yaw_inertia: env("G_YAW", p.yaw_inertia),
            spring: env("G_SPRING", p.spring),
            arb: env("G_ARB", p.arb),
            roll_inertia: env("G_ROLL", p.roll_inertia),
            ..p
        };
        let s = score(&p, &drift, &hand);
        println!("cand {name} {s:?} {p:?}");
        return;
    }
    let mut results = Vec::new();
    for i in 0..n {
        let p = Params::sample(&mut u);
        let s = score(&p, &drift, &hand);
        let lock_ok = (0..3).all(|k| s.lock[k] >= 0.9 * today[k]);
        let ok = s.taps + 2 >= base.taps && s.handling_fails <= base.handling_fails && lock_ok;
        println!(
            "cand {i} held {:.2} s yaw_ok {:.2} taps {}/36 handling_fails {} lock {:.2?} {} {p:?}",
            s.held_s,
            s.yaw_ok,
            s.taps,
            s.handling_fails,
            s.lock,
            if ok { "OK" } else { "guard" }
        );
        results.push((s, p, ok));
    }
    results.sort_by(|a, b| (b.0.held_s + b.0.yaw_ok).total_cmp(&(a.0.held_s + a.0.yaw_ok)));
    println!("\n## Top 10 within the guard rails (base: held {:.2} s, yaw_ok {:.2}, taps {}/36, handling fails {})",
        base.held_s, base.yaw_ok, base.taps, base.handling_fails);
    for (s, p, _) in results.iter().filter(|r| r.2).take(10) {
        println!(
            "held {:.2} s yaw_ok {:.2} taps {}/36 handling_fails {} lock {:.2?} {p:?}",
            s.held_s, s.yaw_ok, s.taps, s.handling_fails, s.lock
        );
    }
    car_lab::set_override(None);
    grip_lab::set_override(None);
    ride_lab::set_override(None);
    wp_vehicle::plain::set_override(None);
}
