//! Owner (2026-10-25): "the cars should not be overriding directional changes on the keyboard
//! — it's still going in complete opposite directions". Rows 141 / 142 made the front wheels
//! follow a held key; this checks the car itself: after a key change (left held, then right)
//! the car must start turning the new way soon. Before `PlainSteer::t_reverse_s` the rack took
//! the release time (0.33 s) to bring the wheels back across the centre, so on quick left–right
//! changes the car kept yawing the old way for ~0.5 s (0.6 s on the heavy cars) and on a
//! slalom of 0.5–0.8 s holds it turned against the key about half of the time.
//!
//! Shipped player steering (keyboard preset assists), flat ground, slaloms of 0.5 / 0.8 / 1.2 s
//! holds at 40–160 km/h and 0 / 50 / 100 % throttle.

use std::path::PathBuf;

use wp_contracts::{AssistProfile, LoadedCar, VehicleInput, PHYSICS_DT};
use wp_vehicle::drift_lab::slide_state;
use wp_vehicle::drift_lab_w::load_any_car;
use wp_vehicle::rig::Rig;

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    load_any_car(&data, id).unwrap()
}

/// Time from each key change until the yaw rate turns the key's way, s: (mean, worst) over the
/// slalom grid at `kmh`.
fn reversal_lag(c: &LoadedCar, kmh: f32) -> (f32, f32) {
    let a = AssistProfile::keyboard_default();
    let (mut sum, mut n, mut worst) = (0.0f32, 0usize, 0.0f32);
    for thr in [0.0f32, 0.5, 1.0] {
        for hold in [0.5f32, 0.8, 1.2] {
            let mut rig = Rig::new(c);
            rig.set_player_input(true);
            rig.teleport([0.0, 0.0, 0.0], 0.0, kmh / 3.6, 0.0);
            for _ in 0..120 {
                let inp = VehicleInput {
                    throttle: 0.3,
                    ..Default::default()
                };
                rig.step(&inp, &a);
            }
            let ticks = (hold * 6.0 / PHYSICS_DT) as usize;
            let (mut since, mut matched, mut last_seg) = (0.0f32, true, 0usize);
            for k in 0..ticks {
                let seg = (k as f32 * PHYSICS_DT / hold) as usize;
                // Left first, then right, left, … (key + = right).
                let key = if seg % 2 == 0 { -1.0 } else { 1.0 };
                if seg != last_seg {
                    if !matched && last_seg > 0 {
                        // Never turned the key's way within the hold: count the whole hold.
                        sum += hold;
                        n += 1;
                        worst = worst.max(hold);
                    }
                    since = 0.0;
                    matched = false;
                    last_seg = seg;
                }
                let inp = VehicleInput {
                    steer: key,
                    throttle: thr,
                    ..Default::default()
                };
                rig.step(&inp, &a);
                since += PHYSICS_DT;
                // Key + = right wants a negative (clockwise) yaw rate.
                if !matched && -key * slide_state(&rig).yaw_rate > 0.0 {
                    matched = true;
                    if seg > 0 {
                        sum += since;
                        n += 1;
                        worst = worst.max(since);
                    }
                }
            }
        }
    }
    (sum / n.max(1) as f32, worst)
}

#[test]
fn the_car_turns_the_new_way_soon_after_a_key_change() {
    // Mean lag over 40 / 80 / 120 / 160 km/h. Measured (2026-10-25), shipped rack vs the old
    // release-rate reversal (t_reverse_s = 0.33): kestrel_ts 0.24 vs 0.42 s, rampart_50 0.29 vs
    // 0.47, hakone_gt 0.26 vs 0.45, ironside_440 (heavy, loose rear) 0.39 vs 0.54.
    for (id, limit) in [
        ("kestrel_ts", 0.33f32),
        ("rampart_50", 0.33),
        ("hakone_gt", 0.33),
        ("ironside_440", 0.45),
    ] {
        let c = car(id);
        let mut mean = 0.0;
        for kmh in [40.0f32, 80.0, 120.0, 160.0] {
            let (m, worst) = reversal_lag(&c, kmh);
            println!("{id} {kmh} km/h: mean {m:.3} s, worst {worst:.3} s");
            mean += m / 4.0;
        }
        assert!(
            mean <= limit,
            "{id}: the car keeps turning the old way {mean:.3} s after a key change (limit {limit} s)"
        );
    }
}
