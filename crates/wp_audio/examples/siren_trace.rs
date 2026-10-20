//! Write the director's siren command stream for `apps/testbed-web/e2e/siren_samples.mjs`,
//! which renders it through the browser's real voice pool and siren rack (recorded loops and
//! the procedural fallback) and checks cadence, loop seams and Doppler.
//!
//! Cases (a police cruiser, siren forced on through the testbed override):
//! - `wail`, `yelp`, `phaser`, `hilo`: parked 30 m to the listener's side, one pattern held;
//! - `passby`: hi-lo (steady tones: the Doppler shift is easy to measure) while driving past at
//!   27 m/s, 8 m from the listener.
//!
//! `cargo run -p wp_audio --example siren_trace -- out.json`

use wp_audio::*;
use wp_contracts::{load_car, EntityId, VehicleState};

/// Audio frames per second (the browser runs the director once per rendered frame).
const FPS: u64 = 60;
/// Physics ticks per audio frame.
const TICKS_PER_FRAME: u64 = 4;
/// Pass-by speed, m/s.
const PASS_SPEED: f32 = 27.0;
/// Pass-by lateral offset, m.
const PASS_OFFSET: f32 = 8.0;

fn listener() -> Listener {
    Listener {
        pos: [0.0, 0.0, 1.5],
        vel: [0.0, 0.0, 0.0],
        forward: [0.0, 1.0, 0.0],
        up: [0.0, 0.0, 1.0],
    }
}

/// Run one case: `pattern` forced for `secs`; `state(t)` places the car.
fn run(
    car: &wp_contracts::LoadedCar,
    pattern: SirenPattern,
    secs: f32,
    state: impl Fn(f32) -> VehicleState,
) -> Vec<serde_json::Value> {
    let mut d = AudioDirector::new(AudioLibrary::embedded().expect("audio data"), 5);
    d.register_car(car);
    let e = EntityId(42);
    d.set_siren_override(
        e,
        Some(SirenOverride {
            on: true,
            mode: SirenMode::Fixed(pattern),
        }),
    );
    let n = (secs * FPS as f32).round() as u64;
    (0..n)
        .map(|f| {
            let t = f as f32 / FPS as f32;
            let st = state(t);
            let inputs = [VehicleAudioInput {
                entity: e,
                car_id: &car.def.id,
                state: &st,
                throttle: 0.3,
                throttle_applied: Some(0.3),
                horn: false,
                siren: true,
                road_rumble: 0.0,
                kind: DriverKind::Police,
            }];
            let commands = d.update(&AudioFrame {
                tick: (f + 1) * TICKS_PER_FRAME,
                listener: listener(),
                focus: None,
                vehicles: &inputs,
                events: &[],
            });
            serde_json::json!({ "t": t, "commands": commands })
        })
        .collect()
}

fn parked(t: f32) -> VehicleState {
    let _ = t;
    VehicleState {
        pos: [30.0, 0.0, 0.5],
        rot: [0.0, 0.0, 0.0, 1.0],
        engine_rpm: 900.0,
        gear: 1,
        health: 1.0,
        ..Default::default()
    }
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "siren_trace.json".into());
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let car = load_car(repo.join("data/cars/sentinel_cruiser.json")).expect("car loads");
    let lib = AudioLibrary::embedded().expect("audio data");
    let pass_s = 8.0;
    let mut cases = Vec::new();
    for (name, p, secs) in [
        ("wail", SirenPattern::Wail, 12.0),
        ("yelp", SirenPattern::Yelp, 6.0),
        ("phaser", SirenPattern::Phaser, 4.0),
        ("hilo", SirenPattern::HiLo, 8.0),
    ] {
        cases.push(serde_json::json!({ "name": name, "frames": run(&car, p, secs, parked) }));
    }
    let passby = run(&car, SirenPattern::HiLo, pass_s, |t| {
        let y = -PASS_SPEED * pass_s / 2.0 + PASS_SPEED * t;
        VehicleState {
            pos: [PASS_OFFSET, y, 0.5],
            rot: [0.0, 0.0, 0.0, 1.0],
            lin_vel: [0.0, PASS_SPEED, 0.0],
            speed_mps: PASS_SPEED,
            engine_rpm: 3000.0,
            gear: 3,
            health: 1.0,
            ..Default::default()
        }
    });
    cases.push(serde_json::json!({ "name": "passby", "frames": passby }));
    let l = listener();
    let out = serde_json::json!({
        "library": { "config": lib.config, "packs": lib.packs },
        "siren_pack": lib.car_or_fallback(&car.def).siren.map(|s| s.pack),
        "listener": [l.pos, l.forward, l.up],
        "pass": { "speed": PASS_SPEED, "offset": PASS_OFFSET, "secs": pass_s },
        "cases": cases,
    });
    std::fs::write(&path, out.to_string()).expect("write json");
    println!("{} cases -> {path}", cases.len());
}
