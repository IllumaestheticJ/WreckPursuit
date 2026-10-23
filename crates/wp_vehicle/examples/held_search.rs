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
//! - handling: cars outside `tests/handling_balance.rs`'s understeer-gradient ranges.
//!
//! AI pace and goldens are checked on the shortlist only (too slow for every candidate).
//! `N=150 SEED=1 cargo run --release -p wp_vehicle --example held_search`; `ONLY_BASE=1` scores
//! the shipped set alone; `PLAYER_LOCK=1` searches the player rack's speed curve instead of a
//! car-wide lock scale (AI unaffected).

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
    /// The player rack's speed-sensitivity reference speed, km/h (player cars only; AI keep
    /// their own steering). The shipped 113 km/h unless `PLAYER_LOCK=1`.
    sens_ref_kmh: f32,
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
            lock: c.lock,
            rear_stiffness: g.rear_stiffness,
            front_stiffness: g.front_stiffness,
            yaw_inertia: RideTune::owner().yaw_inertia,
            sens_ref_kmh: wp_vehicle::lab_scope::shipped_plain().sens_ref_kmh,
        }
    }

    /// A random candidate (`u` = uniform 0..1 draws).
    fn sample(u: &mut impl FnMut() -> f32) -> Self {
        // PLAYER_LOCK=1: no car-wide lock change (AI steer with it too); the player rack's
        // speed curve instead.
        let player_lock = std::env::var_os("PLAYER_LOCK").is_some();
        let mut r = |lo: f32, hi: f32| lo + (hi - lo) * u();
        let mut p = Self {
            spin_b0: r(5.0, 14.0),
            spin_b2: r(1.2, 3.0),
            spin_c: r(0.7, 1.0),
            side_floor: r(0.0, 0.6),
            rear_relax: r(0.8, 2.0),
            rear_peak_slip: r(0.9, 1.5),
            lock: r(0.7, 1.0),
            rear_stiffness: r(0.45, 1.0),
            front_stiffness: r(0.5, 1.0),
            yaw_inertia: r(0.9, 1.4),
            sens_ref_kmh: 113.0,
        };
        if player_lock {
            p.lock = 1.0;
            p.sens_ref_kmh = r(40.0, 113.0);
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
            ..CarTune::shipped()
        }));
        grip_lab::set_override(Some(GripBalance {
            rear_stiffness: self.rear_stiffness,
            front_stiffness: self.front_stiffness,
            ..GripBalance::shipped()
        }));
        ride_lab::set_override(Some(RideTune {
            yaw_inertia: self.yaw_inertia,
            ..RideTune::owner()
        }));
    }

    /// The player rack with this candidate's speed curve (set after a run's settle, which
    /// drives a bare rig that a plain-steering override would reach too).
    fn player_rack(&self) -> wp_vehicle::plain::PlainSteer {
        wp_vehicle::plain::PlainSteer {
            sens_ref_kmh: self.sens_ref_kmh,
            ..wp_vehicle::lab_scope::shipped_plain()
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
}

fn score(p: &Params, drift: &[LoadedCar], hand: &[(LoadedCar, f32, f32)]) -> Score {
    p.apply();
    let a = reference_assists();
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
    if std::env::var_os("ONLY_BASE").is_some() {
        return;
    }
    let mut results = Vec::new();
    for i in 0..n {
        let p = Params::sample(&mut u);
        let s = score(&p, &drift, &hand);
        let ok = s.taps + 2 >= base.taps && s.handling_fails <= base.handling_fails;
        println!(
            "cand {i} held {:.2} s yaw_ok {:.2} taps {}/36 handling_fails {} {} {p:?}",
            s.held_s,
            s.yaw_ok,
            s.taps,
            s.handling_fails,
            if ok { "OK" } else { "guard" }
        );
        results.push((s, p, ok));
    }
    results.sort_by(|a, b| (b.0.held_s + b.0.yaw_ok).total_cmp(&(a.0.held_s + a.0.yaw_ok)));
    println!("\n## Top 10 within the guard rails (base: held {:.2} s, yaw_ok {:.2}, taps {}/36, handling fails {})",
        base.held_s, base.yaw_ok, base.taps, base.handling_fails);
    for (s, p, _) in results.iter().filter(|r| r.2).take(10) {
        println!(
            "held {:.2} s yaw_ok {:.2} taps {}/36 handling_fails {} {p:?}",
            s.held_s, s.yaw_ok, s.taps, s.handling_fails
        );
    }
    car_lab::set_override(None);
    grip_lab::set_override(None);
    ride_lab::set_override(None);
    wp_vehicle::plain::set_override(None);
}
