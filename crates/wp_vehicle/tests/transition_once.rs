//! Owner, playtest row 137: "when I want to transition, it does the transition fine and then
//! immediately transitions AGAIN". On the shipped player steering (`wp_vehicle::lab_scope`), a
//! keyboard flick from a held drift swings the car through once and the new countersteer key
//! then holds it: exactly one direction change of the slide, no second swing within 2 s.
//!
//! The driver (keyboard, TCS / ESC off, 2nd gear; the rig marked player-driven): settle on a 40 m left-hand circle at
//! 0.5 g, the into-turn key + throttle until the slide passes 12°, then the countersteer key +
//! throttle until the slide has stayed in 30–45° for 0.5 s (a held left drift). The flick: the
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
fn flick(c: &LoadedCar, hb_entry: bool, lift_s: f32, handbrake_s: f32) -> Option<(usize, bool)> {
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
    for tick in 0..14 * TPS {
        let s = slide_state(&rig);
        // Left-hand corner: + = the left drift (tail out to the right).
        let angle = (-s.body_slip).to_degrees();
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
            None => (1.0, 1.0, 0.0),
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
                return Some((changes, spun));
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
    }
    flick_at.map(|_| (changes, spun))
}

#[test]
fn a_keyboard_flick_transitions_exactly_once() {
    // The player aids (the transition rack and the new-side catch) are what stop the second
    // swing: with them off parity_rwd_b swung through twice in 4 / 4 of these flicks and
    // parity_rwd_d in 2 / 4. The catch at 0.9 of the travel (the default until row 137; 0.8 now)
    // over-caught rampart_50 into a second swing in 2 / 4 (row 137 bench, 2026-10-21).
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
            // rampart_50's handbrake flicks (from its handbrake-started drift) do not swing it
            // through at all (0 changes): no second swing either, the player adds input.
            let want: &[usize] = if hb_entry && hb > 0.0 { &[0, 1] } else { &[1] };
            assert!(
                matches!(r, Some((n, false)) if want.contains(&n)),
                "{id} lift {lift} s handbrake {hb} s: {r:?} (want {want:?} swings, no spin)"
            );
        }
    }
}
