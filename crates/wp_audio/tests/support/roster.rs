//! Roster audio scenarios (shared by `tests/roster.rs` and `examples/roster_trace.rs`; lead
//! round 12b, playtest #47 "some cars are missing sounds").
//!
//! Every car in `data/cars` drives a short scripted pass in each role — player (focus, chase
//! camera), AI racer, police unit (siren on) and traffic — at a role-typical distance: the
//! engine sweeps idle → 90 % of redline under throttle, then the driven wheels spin up
//! (wheelspin loop) while the fronts slide (squeal). The states are synthesised (no physics),
//! so this only depends on the audio data and the director.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wp_audio::types::slot;
use wp_audio::*;
use wp_contracts::{load_car, EntityId, LoadedCar, SurfaceType, VehicleState, WheelState};

/// Audio frames per second (the browser runs the director once per rendered frame).
pub const FPS: u64 = 60;
/// Physics ticks per audio frame.
pub const TICKS_PER_FRAME: u64 = 4;
/// Length of the engine sweep, s.
pub const SWEEP_S: f32 = 1.6;
/// Length of the wheelspin / slide part, s.
pub const SPIN_S: f32 = 0.9;

/// Repo root.
pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every car in `data/cars`, sorted by id.
pub fn roster() -> Vec<LoadedCar> {
    let mut ids: Vec<String> = std::fs::read_dir(repo().join("data/cars"))
        .expect("data/cars")
        .filter_map(|e| {
            let p = e.ok()?.path();
            (p.extension().and_then(|x| x.to_str()) == Some("json"))
                .then(|| p.file_stem().unwrap().to_string_lossy().to_string())
        })
        .collect();
    ids.sort();
    ids.iter()
        .map(|id| load_car(repo().join(format!("data/cars/{id}.json"))).expect("car loads"))
        .collect()
}

/// The roles a car is heard in, with the listener distance used for each.
pub const ROLES: [(DriverKind, f32); 4] = [
    (DriverKind::Player, 6.0),
    (DriverKind::AiRacer, 25.0),
    (DriverKind::Police, 40.0),
    (DriverKind::Traffic, 60.0),
];

/// Short role name (JSON / messages).
pub fn role_name(k: DriverKind) -> &'static str {
    match k {
        DriverKind::Player => "player",
        DriverKind::AiRacer => "ai",
        DriverKind::Police => "police",
        DriverKind::Traffic => "traffic",
    }
}

/// The listener: at the origin looking along +Y.
pub fn listener() -> Listener {
    Listener {
        pos: [0.0, 0.0, 1.5],
        vel: [0.0, 0.0, 0.0],
        forward: [0.0, 1.0, 0.0],
        up: [0.0, 0.0, 1.0],
    }
}

/// State of `car` at time `t` of the script, `dist` m to the listener's side.
pub fn state_at(car: &LoadedCar, t: f32, dist: f32) -> VehicleState {
    let e = &car.def.engine;
    let static_n = car.def.mass_kg * 9.81 / 4.0;
    let mut st = VehicleState {
        pos: [dist, 0.0, 0.5],
        rot: [0.0, 0.0, 0.0, 1.0],
        lin_vel: [0.0, 0.0, 0.0],
        gear: 2,
        health: 1.0,
        ..Default::default()
    };
    let wheel = |slip_ratio: f32, slip_angle: f32, omega: f32| WheelState {
        omega,
        slip_ratio,
        slip_angle,
        fz_n: static_n,
        surface: SurfaceType::Asphalt,
        contact: true,
        compression_m: 0.08,
        ..Default::default()
    };
    if t < SWEEP_S {
        let f = t / SWEEP_S;
        st.engine_rpm = e.idle_rpm + f * (0.9 * e.redline_rpm - e.idle_rpm);
        st.speed_mps = 2.0 + 20.0 * f;
        st.wheel_states = [wheel(0.0, 0.0, st.speed_mps / 0.32); 4];
    } else {
        // Burnout-ish: every wheel spinning (covers RWD, FWD and AWD), fronts sliding.
        st.engine_rpm = 0.7 * e.redline_rpm;
        st.speed_mps = 4.0;
        st.wheel_states = [
            wheel(2.5, 0.3, 45.0),
            wheel(2.5, 0.3, 45.0),
            wheel(2.5, 0.0, 45.0),
            wheel(2.5, 0.0, 45.0),
        ];
    }
    st.lin_vel = [0.0, st.speed_mps, 0.0];
    st
}

/// One 60 Hz frame of a roster case.
#[derive(Debug, Clone)]
pub struct CaseFrame {
    /// Time since the case started, s.
    pub t: f32,
    /// Director output.
    pub commands: Vec<AudioCommand>,
    /// Looped voices playing after this frame: slot → (bus, gain).
    pub voices: BTreeMap<u32, (Bus, f32)>,
}

/// Run one car in one role, alone, for the whole script.
pub fn run_case(car: &LoadedCar, kind: DriverKind, dist: f32) -> Vec<CaseFrame> {
    let mut d = AudioDirector::new(AudioLibrary::embedded().expect("audio data"), 11);
    d.register_car(car);
    let entity = EntityId(7);
    let focus = (kind == DriverKind::Player).then_some(entity);
    let mut voices: BTreeMap<u32, (Bus, f32)> = BTreeMap::new();
    let n = ((SWEEP_S + SPIN_S) * FPS as f32).round() as u64;
    let mut out = Vec::with_capacity(n as usize);
    for f in 0..n {
        let t = f as f32 / FPS as f32;
        let st = state_at(car, t, dist);
        let throttle = 1.0;
        let inputs = [VehicleAudioInput {
            entity,
            car_id: &car.def.id,
            state: &st,
            throttle,
            throttle_applied: Some(throttle),
            horn: false,
            siren: kind == DriverKind::Police,
            road_rumble: 0.0,
            kind,
        }];
        let commands = d.update(&AudioFrame {
            tick: (f + 1) * TICKS_PER_FRAME,
            listener: listener(),
            focus,
            vehicles: &inputs,
            events: &[],
        });
        for c in &commands {
            match c {
                AudioCommand::Play {
                    voice,
                    bus,
                    looped: true,
                    params,
                    ..
                } => {
                    voices.insert(voice.slot, (*bus, params.gain));
                }
                AudioCommand::Set { voice, params } => {
                    if let Some(v) = voices.get_mut(&voice.slot) {
                        v.1 = params.gain;
                    }
                }
                AudioCommand::Stop { voice, .. } => {
                    voices.remove(&voice.slot);
                }
                _ => {}
            }
        }
        out.push(CaseFrame {
            t,
            commands,
            voices: voices.clone(),
        });
    }
    out
}

/// RMS of the engine-layer gains (incl. limiter) of a frame.
pub fn engine_level(f: &CaseFrame) -> f32 {
    f.voices
        .iter()
        .filter(|(s, (b, _))| {
            *b == Bus::Engine && (slot::ENGINE_LAYER..=slot::ENGINE_LIMITER).contains(*s)
        })
        .map(|(_, (_, g))| g * g)
        .sum::<f32>()
        .sqrt()
}

/// Gain of `slot` in a frame (0 if not playing).
pub fn slot_gain(f: &CaseFrame, s: u32) -> f32 {
    f.voices.get(&s).map_or(0.0, |v| v.1)
}

/// Gain of the siren loop (any pattern) in a frame (0 if not playing).
pub fn siren_gain(f: &CaseFrame) -> f32 {
    (0..4)
        .map(|p| slot_gain(f, slot::SIREN + p))
        .fold(0.0, f32::max)
}

/// JSON for `apps/testbed-web/e2e/audio_roster.mjs`: `{ library, cases: [{ car, role, dist,
/// engine_pack, frames: [{ t, commands }] }] }`.
pub fn render_json(cases: &[(&LoadedCar, DriverKind, f32, Vec<CaseFrame>)]) -> String {
    let lib = AudioLibrary::embedded().expect("audio data");
    let cs: Vec<serde_json::Value> = cases
        .iter()
        .map(|(car, kind, dist, frames)| {
            let fr: Vec<serde_json::Value> = frames
                .iter()
                .map(|f| serde_json::json!({ "t": f.t, "commands": f.commands }))
                .collect();
            serde_json::json!({
                "car": car.def.id,
                "role": role_name(*kind),
                "dist": dist,
                "engine_pack": lib.car_or_fallback(&car.def).engine_pack,
                "frames": fr,
            })
        })
        .collect();
    let l = listener();
    serde_json::json!({
        "library": { "config": lib.config, "packs": lib.packs },
        "listener": [l.pos, l.forward, l.up],
        "cases": cs,
    })
    .to_string()
}
