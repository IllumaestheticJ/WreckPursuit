//! Held drifts entered with the handbrake or a lift (unify, lead 2026-10-21): the low-power
//! cars (shiro_turbo, lucciola_350) cannot break the rear loose on throttle alone in 2nd, so
//! the throttle-only entries of `transition` / `grip_balance holds` never measure their hold.
//!
//! The driver: settle on a 40 m left-hand circle at `g`; entry with the into-turn key held and
//! either the handbrake pulled for 0.3 s (throttle on) or the throttle lifted for 0.4 s; once
//! the slide passes 12°: the held countersteer key + full throttle, or the tapper (the
//! keyboard drifter of `drift_lab_w::PlainDriver::Tapper`, target 35°). 6 s; held = no spin,
//! no side swap, ≥ half the run in 10–50° (`HoldMetrics`).
//!
//! Runs on the shipped handling (`wp_vehicle::lab_scope`, the rig marked player-driven) with
//! the lab's reference assists (TCS / ESC off). `cargo run --release -p wp_vehicle --example
//! entry_hold [-- car …]`; `PLAIN='{…}'` / `GRIP='{…}'` = fields over the shipped plain
//! steering / grip balance (A/B).

use std::path::PathBuf;

use wp_contracts::{LoadedCar, VehicleInput, PHYSICS_DT};
use wp_vehicle::drift_lab::{settle_on_circle, slide_state, DRIFT_GEAR};
use wp_vehicle::drift_lab_w::{load_any_car, reference_assists, HoldMetrics};

const TPS: usize = 240;

/// How the slide is started.
#[derive(Clone, Copy, Debug)]
enum Entry {
    Handbrake,
    Lift,
}

/// What the driver does once the slide passes 12°.
#[derive(Clone, Copy, Debug)]
enum Hold {
    Counter,
    Tapper,
}

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    load_any_car(&data, id).unwrap()
}

fn merged<T: serde::Serialize + serde::de::DeserializeOwned>(base: &T, json: &str) -> T {
    let mut v = serde_json::to_value(base).unwrap();
    let patch: serde_json::Value = serde_json::from_str(json).expect("json");
    for (k, x) in patch.as_object().expect("object") {
        v[k] = x.clone();
    }
    serde_json::from_value(v).expect("fields")
}

fn run(c: &LoadedCar, entry: Entry, hold: Hold, lat_g: f32) -> HoldMetrics {
    let a = reference_assists();
    let mut rig = settle_on_circle(c, &a, 40.0, lat_g);
    rig.set_player_input(true);
    let (mut angles, mut back) = (Vec::new(), Vec::new());
    let mut entered = false;
    let mut prev = (-slide_state(&rig).body_slip).to_degrees();
    for k in 0..6 * TPS {
        let t = k as f32 * PHYSICS_DT;
        let s = slide_state(&rig);
        // Left-hand corner: slide angle = −body slip; into the turn = −1, countersteer = +1.
        let angle = (-s.body_slip).to_degrees();
        let rate = (angle - prev) / PHYSICS_DT;
        prev = angle;
        entered |= angle > 12.0;
        let (steer, throttle, handbrake) = if !entered {
            match entry {
                Entry::Handbrake => (-1.0, 1.0, if t < 0.3 { 1.0 } else { 0.0 }),
                Entry::Lift => (-1.0, if t < 0.4 { 0.0 } else { 1.0 }, 0.0),
            }
        } else {
            match hold {
                Hold::Counter => (1.0, 1.0, 0.0),
                Hold::Tapper => {
                    let steer = if angle > 35.0 || rate > 15.0 {
                        1.0
                    } else if angle < 20.0 {
                        -1.0
                    } else {
                        0.0
                    };
                    (steer, if angle > 45.0 { 0.0 } else { 1.0 }, 0.0)
                }
            }
        };
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
        let n = slide_state(&rig);
        angles.push(-n.body_slip.to_degrees());
        back.push(n.v_fwd < 0.0);
    }
    HoldMetrics::from_angles(&angles, &back)
}

fn main() {
    // PLAIN='{…}' / GRIP='{…}': fields over the shipped plain steering / grip balance (A/B).
    if let Ok(j) = std::env::var("PLAIN") {
        wp_vehicle::plain::set_override(Some(merged(&wp_vehicle::lab_scope::shipped_plain(), &j)));
    }
    if let Ok(j) = std::env::var("GRIP") {
        wp_vehicle::grip_lab::set_override(Some(merged(
            &wp_vehicle::grip_lab::GripBalance::shipped(),
            &j,
        )));
    }
    let ids: Vec<String> = {
        let a: Vec<String> = std::env::args().skip(1).collect();
        if a.is_empty() {
            [
                "parity_rwd_a",
                "parity_rwd_b",
                "parity_rwd_d",
                "rampart_50",
                "shiro_turbo",
                "debug_coupe",
                "lucciola_350",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect()
        } else {
            a
        }
    };
    println!(
        "| car | handbrake + held key | handbrake + tapper | lift + held key | lift + tapper |"
    );
    println!("|---|---|---|---|---|");
    for id in &ids {
        let c = car(id);
        let mut cells = Vec::new();
        for entry in [Entry::Handbrake, Entry::Lift] {
            for hold in [Hold::Counter, Hold::Tapper] {
                let (mut held, mut spun, mut band) = (0, 0, 0.0f32);
                for g in [0.3f32, 0.5, 0.7] {
                    let m = run(&c, entry, hold, g);
                    held += m.held as usize;
                    spun += m.spun as usize;
                    band += m.band_s;
                }
                cells.push(format!(
                    "{held} / 3 held, {spun} spun, {:.1} s in 30–45°",
                    band / 3.0
                ));
            }
        }
        println!("| {id} | {} |", cells.join(" | "));
    }
}
