//! Owner, playtest rows 137 / 142: keyboard flicks from a held drift on the shipped player
//! steering (`wp_vehicle::lab_scope`). Row 142: "this behavior of full lock the opposite way to
//! enable transitioning needs to be removed entirely". So the front wheels never go past what
//! the key commands: with a key held they stay on the key's side, within the key's lock at
//! that speed; with no key they only return toward the centre. The slide's direction changes
//! (swings) and spins are reported, not asserted: with no transition aids the player adds
//! input.
//!
//! The driver (keyboard, TCS / ESC off, 2nd gear; the rig marked player-driven): settle on a 40 m left-hand circle at
//! 0.5 g, the into-turn key + throttle until the slide passes 12°, then tapping (countersteer key
//! above 35° or while the slide grows fast, into-turn key below 20°, throttle off past 45°)
//! until the slide has stayed in 30–45° for 0.5 s (a held left drift). The flick: the
//! throttle lifted (and the handbrake pulled for a moment) with the countersteer key still held
//! until the slide comes through, then the left key (the new side's countersteer) + full
//! throttle for the rest of the run.

use std::path::PathBuf;

use wp_contracts::{LoadedCar, VehicleInput};
use wp_vehicle::drift_lab::{settle_on_circle, slide_state, DRIFT_GEAR};
use wp_vehicle::drift_lab_w::{load_any_car, reference_assists};

const TPS: usize = 240;

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    load_any_car(&data, id).unwrap()
}

/// One flick. Returns `None` if the drift never held before the flick, else the number of
/// direction changes of the slide (sign changes past ±10°) within 2 s of the first one (or
/// within 3 s of the flick when it never swung), and whether the car spun.
/// One flick's result.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Flick {
    /// Direction changes of the slide (sign changes past ±10°) within 2 s of the first one (or
    /// within 3 s of the flick when it never swung).
    swings: usize,
    spun: bool,
    /// Worst wheel movement past what the key commands over the whole run, °: with a key held,
    /// moving further toward the side away from it or past the key's lock at that speed; with
    /// no key, moving away from the centre (per tick).
    over_key_deg: f32,
}

fn flick(c: &LoadedCar, hb_entry: bool, lift_s: f32, handbrake_s: f32) -> Option<Flick> {
    let a = reference_assists();
    let mut rig = settle_on_circle(c, &a, 40.0, 0.5);
    rig.set_player_input(true);
    let mut entered = false;
    let mut in_band = 0usize;
    let mut flick_at: Option<usize> = None;
    let mut side = 1.0f32;
    let mut changes = 0usize;
    let mut first_change: Option<usize> = None;
    let mut spun = false;
    let mut prev_angle = 0.0f32;
    let mut over_key = 0.0f32;
    let mut prev_wheel = 0.0f32;
    let mut prev_key = 0.0f32;
    let full_lock = c
        .def
        .wheels
        .iter()
        .filter(|w| w.steerable)
        .map(|w| w.max_steer_rad)
        .fold(0.0f32, f32::max);
    let p = wp_vehicle::lab_scope::shipped_plain();
    for tick in 0..14 * TPS {
        let s = slide_state(&rig);
        // Left-hand corner: + = the left drift (tail out to the right).
        let angle = (-s.body_slip).to_degrees();
        let rate = (angle - prev_angle) / wp_contracts::PHYSICS_DT;
        prev_angle = angle;
        entered |= angle > 12.0;
        if flick_at.is_none() {
            in_band = if entered && (30.0..=45.0).contains(&angle) {
                in_band + 1
            } else {
                0
            };
            if in_band >= TPS / 2 {
                flick_at = Some(tick);
            } else if tick > 8 * TPS {
                return None;
            }
        }
        let (steer, throttle, handbrake) = match flick_at {
            // Entry: the into-turn key + throttle (with `hb_entry` the handbrake for the first
            // 0.3 s too: rampart_50 cannot break the rear loose on power alone here).
            None if !entered => (
                -1.0,
                1.0,
                if hb_entry && tick < 3 * TPS / 10 {
                    1.0
                } else {
                    0.0
                },
            ),
            // The held drift: the owner's tapping (OWNER_KEY_TECHNIQUE.md; with no held-key
            // aids a held countersteer key does not hold these drifts any more, row 142).
            None => {
                let steer = if angle > 35.0 || rate > 15.0 {
                    1.0
                } else if angle < 20.0 {
                    -1.0
                } else {
                    0.0
                };
                (steer, if angle > 45.0 { 0.0 } else { 1.0 }, 0.0)
            }
            Some(f) => {
                // The countersteer key stays held with the throttle lifted (and the handbrake
                // pulled for a moment) until the slide has come through to the new side, then
                // the left key (the new side's countersteer) + full throttle.
                let t = (tick - f) as f32 / TPS as f32;
                let hb = if t < handbrake_s { 1.0 } else { 0.0 };
                if first_change.is_none() && angle > 0.0 {
                    (1.0, if t < lift_s { 0.0 } else { 0.3 }, hb)
                } else {
                    (-1.0, 1.0, hb)
                }
            }
        };
        if let Some(f) = flick_at {
            if angle * side < -10.0 {
                side = -side;
                changes += 1;
                first_change.get_or_insert(tick);
            }
            spun |= angle.abs() >= 90.0 || s.v_fwd < 0.0;
            let end = first_change.map_or(f + 3 * TPS, |c| c + 2 * TPS);
            if tick >= end {
                return Some(Flick {
                    swings: changes,
                    spun,
                    over_key_deg: over_key.to_degrees(),
                });
            }
        }
        rig.step(
            &VehicleInput {
                steer,
                throttle,
                handbrake,
                gear_down: !a.auto_gears && rig.vehicle.gear() > DRIFT_GEAR,
                ..Default::default()
            },
            &a,
        );
        // The wheels against the key (+ angle = left, key + = right).
        let wheel = rig.vehicle.telemetry().steer_rad;
        let lock = full_lock * p.lock_frac(rig.state().speed_mps) + 1e-4;
        let past = if steer.abs() > 0.5 {
            // A held key: within the key's lock on its side, and never further toward the
            // other side (a reversal's wheels coming back across at the rack rate are fine;
            // a limiter pulling them toward the centre is fine).
            let opposite = (wheel * steer).max(0.0);
            let prev_opposite = if prev_key * steer > 0.0 {
                (prev_wheel * steer).max(0.0)
            } else {
                f32::INFINITY
            };
            (opposite - prev_opposite)
                .max(0.0)
                .max(-wheel * steer - lock)
        } else if prev_key.abs() <= 0.5 {
            // No key: only back toward the centre.
            (wheel.abs() - prev_wheel.abs()).max(0.0)
        } else {
            0.0
        };
        over_key = over_key.max(past);
        prev_wheel = wheel;
        prev_key = steer;
    }
    flick_at.map(|_| Flick {
        swings: changes,
        spun,
        over_key_deg: over_key.to_degrees(),
    })
}

#[test]
fn keyboard_flicks_never_get_auto_opposite_lock() {
    let mut report = Vec::new();
    let mut no_hold = 0usize;
    let mut spins = 0usize;
    for (id, hb_entry) in [
        ("rampart_50", true),
        ("parity_rwd_a", false),
        ("parity_rwd_b", false),
        ("parity_rwd_d", false),
    ] {
        let c = car(id);
        for (lift, hb) in [(0.2, 0.0), (0.3, 0.0), (0.3, 0.15), (0.4, 0.25)] {
            let r = flick(&c, hb_entry, lift, hb);
            println!("{id} lift {lift} s handbrake {hb} s: {r:?}");
            let Some(r) = r else {
                no_hold += 1;
                continue;
            };
            assert!(
                r.over_key_deg < 0.05,
                "{id} lift {lift} s handbrake {hb} s: the wheels went {:.2}° past the key ({r:?})",
                r.over_key_deg
            );
            report.push(r.swings);
            spins += r.spun as usize;
        }
    }
    // Reported, not asserted (row 142): how many flicks swung through once / twice / not at all.
    let n = |k: usize| report.iter().filter(|&&s| s == k).count();
    println!(
        "swings: once {} / {}, twice {}, none {}; spun {spins}; drift never held before the flick: {no_hold}",
        n(1),
        report.len(),
        report.iter().filter(|&&s| s >= 2).count(),
        n(0)
    );
    // The check has teeth: Drift Lab W's old set (the transition rack and new-side catch, lab
    // only since row 142) does turn the wheels past the key on these flicks.
    wp_vehicle::plain::set_override(Some(wp_vehicle::plain::PlainSteer::lab_w()));
    let worst = ["parity_rwd_a", "parity_rwd_b", "parity_rwd_d"]
        .iter()
        .filter_map(|id| flick(&car(id), false, 0.3, 0.0))
        .map(|r| r.over_key_deg)
        .fold(0.0f32, f32::max);
    wp_vehicle::plain::set_override(None);
    println!("lab_w (old aids): worst {worst:.1}° past the key");
    assert!(worst > 1.0, "the old aids should fail this check: {worst}°");
}
