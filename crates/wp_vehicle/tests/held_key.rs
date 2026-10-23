//! Owner, playtest rows 141 / 142: "when I turn towards the inside of the corner, the car goes
//! full lock outside (so the exact opposite direction)". On the shipped player steering
//! (`wp_vehicle::lab_scope`), with a steer key held the front wheels never point against it:
//! - they never move further toward the side away from the key (after a key reversal they
//!   come back across the centre at the rack rate), and are within 1° of the centre or on the
//!   key's side once the key has been held for 0.4 s (the rack's return time, 0.33 s, + margin);
//! - they never go past the key-commanded angle, `full lock × lock(speed)`.
//!
//! Scenarios (keyboard, rampart_50 and the parity RWD cars): the entry-hold drifts (handbrake /
//! lift entry, held countersteer key / tapping), lift-flick transitions from a held drift, and
//! the owner's row-141 clip inputs (full left + throttle from rest into a spin, released,
//! then full right).

use std::path::PathBuf;

use wp_contracts::{AbsLevel, AssistProfile, LoadedCar, TcsLevel, VehicleInput, PHYSICS_DT};
use wp_vehicle::drift_lab::{settle_on_circle, slide_state, DRIFT_GEAR};
use wp_vehicle::drift_lab_w::{load_any_car, reference_assists};
use wp_vehicle::rig::Rig;

const TPS: usize = 240;
const CARS: [&str; 4] = ["rampart_50", "parity_rwd_a", "parity_rwd_b", "parity_rwd_d"];

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    load_any_car(&data, id).unwrap()
}

/// The first held-key violation of a run (printed through `Debug`).
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct Violation {
    t: f32,
    key: f32,
    wheel_deg: f32,
    limit_deg: f32,
    held_s: f32,
    body_slip_deg: f32,
    kmh: f32,
}

/// Checks every tick of a run against the held key.
struct Checker {
    full_lock: f32,
    key: f32,
    held_s: f32,
    prev_wheel: f32,
    first: Option<Violation>,
}

impl Checker {
    fn new(c: &LoadedCar) -> Self {
        Self {
            full_lock: c
                .def
                .wheels
                .iter()
                .filter(|w| w.steerable)
                .map(|w| w.max_steer_rad)
                .fold(0.0, f32::max),
            key: 0.0,
            held_s: 0.0,
            prev_wheel: 0.0,
            first: None,
        }
    }

    /// After a step with steer `key` (+ = right), at time `t`.
    fn check(&mut self, rig: &Rig, key: f32, t: f32) {
        let key = if key.abs() > 0.5 { key.signum() } else { 0.0 };
        self.held_s = if key != 0.0 && key == self.key {
            self.held_s + PHYSICS_DT
        } else {
            PHYSICS_DT
        };
        self.key = key;
        // + wheel = left; the key's side is −key.
        let wheel = rig.vehicle.telemetry().steer_rad;
        if key != 0.0 && self.first.is_none() {
            let p = wp_vehicle::lab_scope::shipped_plain();
            let speed = rig.state().speed_mps;
            // The rack's lock uses the forward speed (not the total, larger in a slide).
            let limit = self.full_lock * p.lock_frac(slide_state(rig).v_fwd) + 0.2f32.to_radians();
            let against = (wheel * key).max(0.0);
            let prev_against = (self.prev_wheel * key).max(0.0);
            let growing = self.held_s > PHYSICS_DT * 1.5 && against > prev_against + 1e-5;
            let settled_against = self.held_s >= 0.4 && against > 1f32.to_radians();
            let past = wheel.abs() > limit;
            if growing || settled_against || past {
                let s = slide_state(rig);
                self.first = Some(Violation {
                    t,
                    key,
                    wheel_deg: wheel.to_degrees(),
                    limit_deg: limit.to_degrees(),
                    held_s: self.held_s,
                    body_slip_deg: s.body_slip.to_degrees(),
                    kmh: speed * 3.6,
                });
            }
        }
        self.prev_wheel = wheel;
    }
}

fn step(rig: &mut Rig, ck: &mut Checker, inp: &VehicleInput, a: &AssistProfile, t: f32) {
    rig.step(inp, a);
    ck.check(rig, inp.steer, t);
}

/// Entry-hold drift on the 40 m circle at `lat_g`: into-turn key + handbrake (0.3 s) or a lift
/// (0.4 s), then past 12° the held countersteer key (`tap` = false) or a tapping driver.
fn entry_hold(c: &LoadedCar, handbrake: bool, tap: bool, lat_g: f32) -> Option<Violation> {
    let a = reference_assists();
    let mut rig = settle_on_circle(c, &a, 40.0, lat_g);
    rig.set_player_input(true);
    let mut ck = Checker::new(c);
    let mut entered = false;
    let mut prev = (-slide_state(&rig).body_slip).to_degrees();
    for k in 0..6 * TPS {
        let t = k as f32 * PHYSICS_DT;
        let angle = (-slide_state(&rig).body_slip).to_degrees();
        let rate = (angle - prev) / PHYSICS_DT;
        prev = angle;
        entered |= angle > 12.0;
        let (steer, throttle, hb) = if !entered {
            if handbrake {
                (-1.0, 1.0, if t < 0.3 { 1.0 } else { 0.0 })
            } else {
                (-1.0, if t < 0.4 { 0.0 } else { 1.0 }, 0.0)
            }
        } else if !tap {
            (1.0, 1.0, 0.0)
        } else {
            let s = if angle > 35.0 || rate > 15.0 {
                1.0
            } else if angle < 20.0 {
                -1.0
            } else {
                0.0
            };
            (s, if angle > 45.0 { 0.0 } else { 1.0 }, 0.0)
        };
        let inp = VehicleInput {
            steer,
            throttle,
            handbrake: hb,
            gear_down: !a.auto_gears && rig.vehicle.gear() > DRIFT_GEAR,
            ..Default::default()
        };
        step(&mut rig, &mut ck, &inp, &a, t);
    }
    ck.first
}

/// Lift-flick from a held left drift: countersteer key held through the lift until the slide
/// crosses, then the new countersteer key + throttle (tests/transition_once.rs's driver); also
/// the flick with the new key pressed at once (the into-old-corner key mid-slide).
fn flick(c: &LoadedCar, key_at_once: bool) -> Option<Violation> {
    let a = reference_assists();
    let mut rig = settle_on_circle(c, &a, 40.0, 0.5);
    rig.set_player_input(true);
    let mut ck = Checker::new(c);
    let hb_entry = c.def.id == "rampart_50";
    let (mut entered, mut in_band, mut flick_at, mut crossed) = (false, 0usize, None, false);
    for k in 0..10 * TPS {
        let t = k as f32 * PHYSICS_DT;
        let angle = (-slide_state(&rig).body_slip).to_degrees();
        entered |= angle > 12.0;
        if flick_at.is_none() {
            in_band = if entered && (30.0..=45.0).contains(&angle) {
                in_band + 1
            } else {
                0
            };
            if in_band >= TPS / 2 {
                flick_at = Some(k);
            }
        }
        crossed |= flick_at.is_some() && angle < 0.0;
        let (steer, throttle, hb) = match flick_at {
            None if !entered => (-1.0, 1.0, if hb_entry && t < 0.3 { 1.0 } else { 0.0 }),
            None => (1.0, 1.0, 0.0),
            Some(f) => {
                let since = (k - f) as f32 * PHYSICS_DT;
                if key_at_once || crossed {
                    (-1.0, if since < 0.3 { 0.0 } else { 1.0 }, 0.0)
                } else {
                    (1.0, if since < 0.3 { 0.0 } else { 0.3 }, 0.0)
                }
            }
        };
        let inp = VehicleInput {
            steer,
            throttle,
            handbrake: hb,
            gear_down: !a.auto_gears && rig.vehicle.gear() > DRIFT_GEAR,
            ..Default::default()
        };
        step(&mut rig, &mut ck, &inp, &a, t);
        if flick_at.is_some_and(|f| k > f + 4 * TPS) {
            break;
        }
    }
    ck.first
}

/// The owner's row-141 clip inputs (examples/row141.rs): from rest, 2nd gear, full left + full
/// throttle 0.8–4.0 s, throttle only to 4.2 s, nothing to 5.0 s, then full right.
fn row141(c: &LoadedCar) -> Option<Violation> {
    let a = AssistProfile {
        abs: AbsLevel::Half,
        tcs: TcsLevel::Off,
        esc: false,
        auto_gears: false,
        auto_clutch: false,
        countersteer_help: 0.05,
        ..AssistProfile::keyboard_default()
    };
    let launch = AssistProfile {
        auto_clutch: true,
        ..a
    };
    let mut rig = Rig::new(c);
    rig.set_player_input(true);
    let mut ck = Checker::new(c);
    let mut prev_up = false;
    for k in 0..(6.25 * TPS as f32) as usize {
        let t = k as f32 * PHYSICS_DT;
        let (steer, throttle) = match t {
            t if t < 0.8 => (0.0, 0.0),
            t if t < 4.0 => (-1.0, 1.0),
            t if t < 4.2 => (0.0, 1.0),
            t if t < 5.0 => (0.0, 0.0),
            _ => (1.0, 0.0),
        };
        let up = rig.vehicle.gear() < 2 && t < 0.7 && !prev_up;
        prev_up = up;
        let inp = VehicleInput {
            steer,
            throttle,
            clutch: if t >= 5.4 { 1.0 } else { 0.0 },
            gear_up: up,
            ..Default::default()
        };
        rig.step(&inp, if t < 1.0 { &launch } else { &a });
        ck.check(&rig, steer, t);
    }
    ck.first
}

/// Every scenario on every car: `(case, first violation)`.
fn all_cases() -> Vec<(String, Option<Violation>)> {
    let mut out = Vec::new();
    for id in CARS {
        let c = car(id);
        for handbrake in [true, false] {
            for tap in [false, true] {
                for g in [0.3, 0.5, 0.7] {
                    out.push((
                        format!("{id} entry hb {handbrake} tap {tap} {g} g"),
                        entry_hold(&c, handbrake, tap, g),
                    ));
                }
            }
        }
        for at_once in [false, true] {
            out.push((
                format!("{id} flick key at once {at_once}"),
                flick(&c, at_once),
            ));
        }
        out.push((format!("{id} row141"), row141(&c)));
    }
    out
}

#[test]
fn wheels_never_point_against_the_held_key() {
    let cases = all_cases();
    let bad: Vec<_> = cases.iter().filter(|(_, v)| v.is_some()).collect();
    for (name, v) in &bad {
        println!("{name}: {v:?}");
    }
    assert!(
        bad.is_empty(),
        "{} / {} cases: {:?}",
        bad.len(),
        cases.len(),
        bad.first()
    );

    // The check has teeth: the pre-row-142 player steering (Drift Lab W's full set and the
    // owner's grip balance with the smart lock and the held-key cut) fails it.
    wp_vehicle::plain::set_override(Some(wp_vehicle::plain::PlainSteer::lab_w()));
    wp_vehicle::grip_lab::set_override(Some(wp_vehicle::grip_lab::GripBalance::owner()));
    let old = all_cases();
    wp_vehicle::plain::set_override(None);
    wp_vehicle::grip_lab::set_override(None);
    let old_bad: Vec<_> = old.iter().filter(|(_, v)| v.is_some()).collect();
    println!(
        "pre-row-142 steering: {} / {} cases point against the key",
        old_bad.len(),
        old.len()
    );
    for (name, v) in &old_bad {
        println!("  {name}: {v:?}");
    }
    assert!(
        !old_bad.is_empty(),
        "the old steering should fail this check"
    );
}
