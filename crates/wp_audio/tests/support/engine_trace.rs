//! Engine-audio trace harness (shared by `tests/engine_shift.rs` and
//! `examples/engine_trace.rs`): drives one car on the flat `wp_vehicle` rig with a scripted
//! pedal, runs the [`AudioDirector`] at 60 Hz like the browser does, and records per frame
//! what the engine model reads (rpm, gear, pedal, applied throttle, clutch) and what it
//! outputs (every engine voice's gain, pitch, synth frequency and load).

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wp_audio::types::slot;
use wp_audio::*;
use wp_contracts::{load_car, AssistProfile, EntityId, LoadedCar, VehicleInput};
use wp_vehicle::metrics::metric_assists;
use wp_vehicle::rig::Rig;

/// Physics ticks per audio frame (240 Hz sim, 60 Hz frames).
pub const TICKS_PER_FRAME: usize = 4;

/// One scripted pedal segment.
#[derive(Debug, Clone, Copy)]
pub struct Segment {
    /// Length, s.
    pub seconds: f32,
    /// Throttle pedal 0..=1.
    pub throttle: f32,
    /// Brake pedal 0..=1.
    pub brake: f32,
}

/// Script helper.
pub const fn seg(seconds: f32, throttle: f32, brake: f32) -> Segment {
    Segment {
        seconds,
        throttle,
        brake,
    }
}

/// Engine voice state as the backend would hold it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VoiceView {
    /// Gain (incl. distance).
    pub gain: f32,
    /// Sample playback rate.
    pub pitch: f32,
    /// Synth fundamental, Hz.
    pub freq_hz: f32,
    /// Synth load / brightness.
    pub load: f32,
}

/// One 60 Hz frame of the trace.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Frame {
    /// Time, s.
    pub t: f32,
    /// Pedal.
    pub pedal: f32,
    /// Throttle reaching the engine (telemetry).
    pub applied: f32,
    /// Clutch engagement (telemetry).
    pub clutch: f32,
    /// Engine rpm (state).
    pub rpm: f32,
    /// Gear (state).
    pub gear: i8,
    /// Speed, km/h.
    pub kmh: f32,
    /// The car's looped voices by slot (any bus): engine layers (`ENGINE_LAYER + i`),
    /// limiter, turbo, supercharger, tire squeal / spin / scrub, surface, …
    pub voices: BTreeMap<u32, VoiceView>,
    /// One-shots started this frame (blow-off, pops, thumps): `(cue, gain)`.
    pub one_shots: Vec<(String, f32)>,
    /// Engine voices that were stopped this frame.
    pub stopped: Vec<u32>,
    /// Every command the director produced this frame (for the offline browser render).
    pub commands: Vec<AudioCommand>,
    /// Listener (chase camera) this frame.
    pub listener: Listener,
    /// Wheel states (FL, FR, RL, RR) this frame.
    pub wheels: [wp_contracts::WheelState; 4],
}

impl Frame {
    /// Sum of the squared gains of the looped engine layers (energy of the engine note).
    pub fn layer_energy(&self) -> f32 {
        self.voices
            .iter()
            .filter(|(s, _)| **s >= slot::ENGINE_LAYER && **s < slot::ENGINE_LIMITER)
            .map(|(_, v)| v.gain * v.gain)
            .sum()
    }

    /// Synth frequency of the loudest layer (0 if silent).
    pub fn note_hz(&self) -> f32 {
        self.voices
            .iter()
            .filter(|(s, _)| **s >= slot::ENGINE_LAYER && **s < slot::ENGINE_LIMITER)
            .max_by(|a, b| a.1.gain.total_cmp(&b.1.gain))
            .map_or(0.0, |(_, v)| v.freq_hz)
    }
}

/// Repo root.
pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Load a car from `data/cars`.
pub fn car(id: &str) -> LoadedCar {
    load_car(repo().join(format!("data/cars/{id}.json"))).expect("car loads")
}

/// Run `script` on `car_id` and return one [`Frame`] per 60 Hz audio frame. The engine model
/// gets the telemetry throttle as `throttle_applied`, like the wasm bridge. `assists`: the
/// `wp_vehicle` metric assists when `None`.
pub fn run(car_id: &str, script: &[Segment], assists: Option<AssistProfile>) -> Vec<Frame> {
    let lib = AudioLibrary::embedded().expect("audio data");
    // Engine-layer gains are recorded with the pack's level divided out (its idle layer's cue
    // gain: 1.0 in the procedural packs, the LEVEL_DB boost in the recorded ones), so the checks
    // see the engine model and the layers' relative balance, whatever the pack's loudness.
    let cue_gain = |pack: &str, cue: &str| {
        if !cue.starts_with("engine/") || cue == "engine/limiter" {
            return 1.0;
        }
        lib.packs
            .get(pack)
            .and_then(|p| p.cues.get("engine/idle"))
            .map_or(1.0, |c| c.gain)
            .max(1e-6)
    };
    let mut slot_gain: BTreeMap<u32, f32> = BTreeMap::new();
    let c = car(car_id);
    let mut dir = AudioDirector::new(lib.clone(), 7);
    dir.register_car(&c);
    let mut rig = Rig::new(&c);
    let a = assists.unwrap_or_else(metric_assists);
    let entity = EntityId(1);
    let mut voices: BTreeMap<u32, VoiceView> = BTreeMap::new();
    let mut frames = Vec::new();
    let mut tick: u64 = 0;
    for s in script {
        let input = VehicleInput {
            throttle: s.throttle,
            brake: s.brake,
            ..Default::default()
        };
        let n = (s.seconds * 60.0).round() as usize;
        for _ in 0..n {
            for _ in 0..TICKS_PER_FRAME {
                rig.step(&input, &a);
                tick += 1;
            }
            let st = rig.state();
            let tel = rig.vehicle.telemetry();
            // Chase camera: 6 m behind, 1.8 m up, moving with the car (no Doppler).
            let fwd = forward(st.rot);
            let listener = Listener {
                pos: [
                    st.pos[0] - 6.0 * fwd[0],
                    st.pos[1] - 6.0 * fwd[1],
                    st.pos[2] + 1.8,
                ],
                vel: st.lin_vel,
                forward: fwd,
                up: [0.0, 0.0, 1.0],
            };
            let inputs = [VehicleAudioInput {
                entity,
                car_id,
                state: &st,
                throttle: s.throttle,
                throttle_applied: Some(tel.throttle),
                horn: false,
                siren: false,
                road_rumble: 0.0,
                kind: DriverKind::Player,
            }];
            let cmds = dir.update(&AudioFrame {
                tick,
                listener,
                focus: Some(entity),
                vehicles: &inputs,
                events: &[],
            });
            let mut f = Frame {
                t: tick as f32 / 240.0,
                pedal: s.throttle,
                applied: tel.throttle,
                clutch: tel.clutch,
                rpm: st.engine_rpm,
                gear: st.gear,
                kmh: st.speed_mps * 3.6,
                wheels: st.wheel_states,
                ..Default::default()
            };
            f.commands = cmds.clone();
            f.listener = listener;
            for c in cmds {
                match c {
                    AudioCommand::Play {
                        voice,
                        pack,
                        cue,
                        looped,
                        params,
                        ..
                    } if voice.entity == entity.0 => {
                        let k = cue_gain(&pack, &cue);
                        if looped {
                            slot_gain.insert(voice.slot, k);
                            voices.insert(voice.slot, view(&params, k));
                        } else {
                            f.one_shots.push((cue, params.gain / k));
                        }
                    }
                    AudioCommand::Set { voice, params } if voice.entity == entity.0 => {
                        if let Some(v) = voices.get_mut(&voice.slot) {
                            *v = view(&params, slot_gain.get(&voice.slot).copied().unwrap_or(1.0));
                        }
                    }
                    AudioCommand::Stop { voice, .. }
                        if voice.entity == entity.0 && voices.remove(&voice.slot).is_some() =>
                    {
                        f.stopped.push(voice.slot);
                    }
                    _ => {}
                }
            }
            f.voices = voices.clone();
            frames.push(f);
        }
    }
    frames
}

/// `cue_gain`: the pack level, divided out of the commanded gain.
fn view(p: &VoiceParams, cue_gain: f32) -> VoiceView {
    VoiceView {
        gain: p.gain / cue_gain,
        pitch: p.pitch,
        freq_hz: p.freq_hz,
        load: p.load,
    }
}

/// Chassis +Y in world space from a quaternion `[x, y, z, w]`.
fn forward(q: [f32; 4]) -> [f32; 3] {
    let [x, y, z, w] = q;
    [
        2.0 * (x * y - w * z),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z + w * x),
    ]
}

/// WOT launch through the gears, a lift (overrun), a part-throttle cruise, then a kickdown.
pub fn standard_script() -> Vec<Segment> {
    vec![
        seg(1.0, 0.0, 0.0),   // idle
        seg(30.0, 1.0, 0.0),  // WOT 1 → 5
        seg(4.0, 0.0, 0.0),   // lift: overrun
        seg(4.0, 0.0, 0.6),   // brake: downshifts with blips
        seg(12.0, 0.35, 0.0), // part throttle up through 3rd/4th
        seg(6.0, 0.35, 0.0),  // cruise
        seg(5.0, 1.0, 0.0),   // kickdown
    ]
}

/// Standstill burnout (brakes hold the fronts), release, full-throttle launch, light cruise.
/// Run it with TCS off (`burnout_assists`).
pub fn burnout_script() -> Vec<Segment> {
    vec![
        seg(1.0, 0.0, 0.0),
        seg(3.0, 1.0, 0.4),
        seg(1.0, 0.0, 0.0),
        seg(4.0, 1.0, 0.0),
        seg(4.0, 0.3, 0.0),
    ]
}

/// The metric assists with TCS off (wheelspin traces).
pub fn burnout_assists() -> wp_contracts::AssistProfile {
    wp_contracts::AssistProfile {
        tcs: wp_contracts::TcsLevel::Off,
        ..metric_assists()
    }
}

/// CSV of a trace (one row per frame; layer columns by slot).
pub fn csv(frames: &[Frame]) -> String {
    let slots: Vec<u32> = {
        let mut s: Vec<u32> = frames
            .iter()
            .flat_map(|f| f.voices.keys().copied())
            .collect();
        s.sort();
        s.dedup();
        s
    };
    let mut out = String::from("t,pedal,applied,clutch,gear,rpm,kmh,energy,note_hz");
    for s in &slots {
        out.push_str(&format!(",g{s:x},p{s:x},f{s:x},l{s:x}"));
    }
    out.push_str(",one_shots\n");
    for f in frames {
        out.push_str(&format!(
            "{:.4},{:.2},{:.3},{:.3},{},{:.0},{:.1},{:.4},{:.1}",
            f.t,
            f.pedal,
            f.applied,
            f.clutch,
            f.gear,
            f.rpm,
            f.kmh,
            f.layer_energy(),
            f.note_hz()
        ));
        for s in &slots {
            let v = f.voices.get(s).copied().unwrap_or_default();
            out.push_str(&format!(
                ",{:.4},{:.3},{:.1},{:.3}",
                v.gain, v.pitch, v.freq_hz, v.load
            ));
        }
        let os: Vec<String> = f
            .one_shots
            .iter()
            .map(|(c, g)| format!("{c}:{g:.2}"))
            .collect();
        out.push_str(&format!(",{}\n", os.join(" ")));
    }
    out
}

/// JSON for `apps/testbed-web/e2e/engine_render.mjs`: `{ library, frames: [{t, gear, rpm,
/// pedal, commands}] }` (frames at 60 Hz).
pub fn render_json(frames: &[Frame]) -> String {
    let lib = AudioLibrary::embedded().expect("audio data");
    let fr: Vec<serde_json::Value> = frames
        .iter()
        .map(|f| {
            serde_json::json!({
                "t": f.t, "gear": f.gear, "rpm": f.rpm, "pedal": f.pedal,
                "listener": [f.listener.pos, f.listener.forward, f.listener.up],
                "commands": f.commands,
            })
        })
        .collect();
    serde_json::json!({
        "library": { "config": lib.config, "packs": lib.packs },
        "frames": fr,
    })
    .to_string()
}
