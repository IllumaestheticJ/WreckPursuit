//! Engine-synthesizer sweeps (row 73, "simulate accurate engine sounds"), shared by
//! `tests/engine_physical.rs` and `examples/engine_sweeps.rs`.
//!
//! One car, the director in [`EngineModelMode::Physical`], a chase listener riding along (no
//! Doppler), and synthesised engine states (no physics, so the rpm is exactly known):
//! - `wot`: idle → redline at full throttle in 2nd (2.6 s), then 0.4 s on the rev limiter;
//! - `lift`: full throttle near 80 % of redline, then a lift: the rpm falls in gear (overrun);
//! - `shift`: 2nd gear pull, a 0.25 s clutch shift (throttle cut, rpm drop), 3rd gear pull;
//! - `steps`: half-throttle plateaus at 20 … 90 % of redline (pitch-accuracy check);
//! - `damaged`: the `wot` sweep with the engine at 35 % health (misfires).
//!
//! The browser renders the command streams through the real voice pool and engine worklet
//! (`apps/testbed-web/e2e/engine_synth.mjs`).

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use wp_audio::*;
use wp_contracts::{load_car, Component, EntityId, LoadedCar, VehicleState};

/// Audio frames per second.
pub const FPS: u64 = 60;
/// Physics ticks per audio frame.
pub const TICKS_PER_FRAME: u64 = 4;
/// Length of every sweep, s.
pub const SWEEP_S: f32 = 3.0;
/// The sweeps.
pub const SWEEPS: [&str; 5] = ["wot", "lift", "shift", "steps", "damaged"];
/// Cars rendered: I4, I6, cross-plane V8, flat-plane V8, turbo I4.
pub const CARS: [&str; 5] = [
    "debug_sedan",
    "sperber_32",
    "rampart_50",
    "lucciola_350",
    "shiro_turbo",
];
/// Rpm plateaus of the `steps` sweep, fractions of redline.
pub const STEPS: [f32; 8] = [0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9];

/// Repo root.
pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Load a car from `data/cars`.
pub fn car(id: &str) -> LoadedCar {
    load_car(repo().join(format!("data/cars/{id}.json"))).expect("car loads")
}

/// Driver input and engine state at `t` of `sweep`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Crank rpm.
    pub rpm: f32,
    /// Pedal.
    pub pedal: f32,
    /// Throttle reaching the engine.
    pub applied: f32,
    /// Gear.
    pub gear: i8,
    /// Road speed, m/s.
    pub speed: f32,
    /// Engine health.
    pub health: f32,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

/// The scripted state of `car` at time `t` of `sweep`.
pub fn sample(car: &LoadedCar, sweep: &str, t: f32) -> Sample {
    let e = &car.def.engine;
    let (idle, red, lim) = (e.idle_rpm, e.redline_rpm, e.rev_limit_rpm);
    let mut s = Sample {
        rpm: idle,
        pedal: 1.0,
        applied: 1.0,
        gear: 2,
        speed: 10.0,
        health: 1.0,
    };
    match sweep {
        "wot" | "damaged" => {
            if t < 2.6 {
                // Torque-limited climb: faster low down, slower near the top.
                let f = t / 2.6;
                s.rpm = lerp(
                    idle,
                    red,
                    1.0 - (1.0 - f) * (1.0 - f) * 0.35 - (1.0 - f) * 0.65,
                );
            } else {
                s.rpm = lim;
            }
            s.speed = 4.0 + 8.0 * t;
            if sweep == "damaged" {
                s.health = 0.35;
            }
        }
        "lift" => {
            s.gear = 3;
            if t < 0.7 {
                s.rpm = lerp(0.72 * red, 0.8 * red, t / 0.7);
                s.speed = 30.0;
            } else {
                // Engine braking in gear: rpm decays toward 30 % of redline.
                let k = ((t - 0.7) / 0.9).min(8.0);
                let target = 0.3 * red;
                s.rpm = target + (0.8 * red - target) * (1.0 / (1.0 + k + 0.5 * k * k));
                s.pedal = 0.0;
                s.applied = 0.0;
                s.speed = 30.0 - 1.5 * (t - 0.7);
            }
        }
        "shift" => {
            if t < 1.2 {
                s.rpm = lerp(0.45 * red, 0.92 * red, t / 1.2);
                s.speed = 8.0 + 10.0 * t;
            } else if t < 1.45 {
                // Clutch shift: cut, rpm falls to the next gear's speed.
                s.applied = 0.0;
                s.rpm = lerp(0.92 * red, 0.62 * red, (t - 1.2) / 0.2);
                s.gear = if t < 1.3 { 2 } else { 3 };
                s.speed = 20.0;
            } else {
                s.gear = 3;
                s.rpm = lerp(0.62 * red, 0.85 * red, (t - 1.45) / 1.55);
                s.speed = 20.0 + 6.0 * (t - 1.45);
            }
        }
        "steps" => {
            let i = ((t / SWEEP_S * STEPS.len() as f32) as usize).min(STEPS.len() - 1);
            s.rpm = STEPS[i] * red;
            s.pedal = 0.5;
            s.applied = 0.5;
            s.gear = 3;
            s.speed = 15.0;
        }
        other => panic!("unknown sweep {other}"),
    }
    s
}

/// One 60 Hz frame.
#[derive(Debug, Clone)]
pub struct SweepFrame {
    /// Time, s.
    pub t: f32,
    /// Scripted state.
    pub sample: Sample,
    /// Director output this frame.
    pub commands: Vec<AudioCommand>,
}

/// Engine state for `s` (chase listener rides along: no Doppler).
pub fn state(s: &Sample) -> VehicleState {
    let mut st = VehicleState {
        pos: [0.0, 0.0, 0.5],
        rot: [0.0, 0.0, 0.0, 1.0],
        lin_vel: [0.0, s.speed, 0.0],
        gear: s.gear,
        speed_mps: s.speed,
        engine_rpm: s.rpm,
        health: 1.0,
        ..Default::default()
    };
    st.component_health.insert(Component::Engine, s.health);
    st
}

/// Chase listener 6 m behind, 1.8 m up, moving with the car.
pub fn listener(speed: f32) -> Listener {
    Listener {
        pos: [0.0, -6.0, 2.3],
        vel: [0.0, speed, 0.0],
        forward: [0.0, 1.0, 0.0],
        up: [0.0, 0.0, 1.0],
    }
}

/// Run `sweep` on `car` with the physical engine model (`tune` installed).
pub fn run(car: &LoadedCar, sweep: &str, tune: EngineTune) -> Vec<SweepFrame> {
    run_mode(car, sweep, tune, EngineModelMode::Physical)
}

/// Run `sweep` on `car` with engines voiced by `mode` (layers: the level reference).
pub fn run_mode(
    car: &LoadedCar,
    sweep: &str,
    tune: EngineTune,
    mode: EngineModelMode,
) -> Vec<SweepFrame> {
    let mut dir = AudioDirector::new(AudioLibrary::embedded().expect("audio data"), 7);
    dir.register_car(car);
    dir.set_engine_model(mode);
    let entity = EntityId(1);
    dir.set_engine_tune(entity, tune);
    let n = (SWEEP_S * FPS as f32).round() as u64;
    let mut frames = Vec::new();
    for i in 0..n {
        let t = i as f32 / FPS as f32;
        let s = sample(car, sweep, t);
        let st = state(&s);
        let inputs = [VehicleAudioInput {
            entity,
            car_id: &car.def.id,
            state: &st,
            throttle: s.pedal,
            throttle_applied: Some(s.applied),
            horn: false,
            siren: false,
            road_rumble: 0.0,
            kind: DriverKind::Player,
        }];
        let commands = dir.update(&AudioFrame {
            tick: 1000 + i * TICKS_PER_FRAME,
            listener: listener(s.speed),
            focus: Some(entity),
            vehicles: &inputs,
            events: &[],
        });
        frames.push(SweepFrame {
            t,
            sample: s,
            commands,
        });
    }
    frames
}

/// JSON for the browser render: `{ library, cases: [{ car, sweep, cylinders, layout, firing,
/// turbo, redline_rpm, rev_limit_rpm, frames: [{ t, rpm, firing_hz, commands }] }] }`.
pub fn render_json(cases: &[(&LoadedCar, &str, EngineModelMode, Vec<SweepFrame>)]) -> String {
    let lib = AudioLibrary::embedded().expect("audio data");
    let cs: Vec<serde_json::Value> = cases
        .iter()
        .map(|(car, sweep, mode, frames)| {
            let def = lib.car_or_fallback(&car.def);
            let fr: Vec<serde_json::Value> = frames
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "t": f.t,
                        "rpm": f.sample.rpm,
                        "pedal": f.sample.pedal,
                        "applied": f.sample.applied,
                        "firing_hz": def.engine.firing_hz(f.sample.rpm),
                        "commands": f.commands,
                    })
                })
                .collect();
            serde_json::json!({
                "car": car.def.id,
                "class": car.def.class,
                "sweep": sweep,
                "mode": mode,
                "cylinders": def.engine.cylinders,
                "layout": def.engine.layout,
                "firing": def.engine.firing,
                "turbo": def.engine.turbo,
                "redline_rpm": car.def.engine.redline_rpm,
                "rev_limit_rpm": car.def.engine.rev_limit_rpm,
                "frames": fr,
            })
        })
        .collect();
    serde_json::json!({
        "library": { "config": lib.config, "packs": lib.packs },
        "cases": cs,
    })
    .to_string()
}
