//! Audio surface (added by workstream 13, audio): runs `wp_audio::AudioDirector` against the
//! live session and returns its commands as JSON for the Web Audio backend in
//! `apps/testbed-web/src/audio/`. Purely additive; nothing else in this crate depends on it.
//!
//! | function | what |
//! |---|---|
//! | `audio_library()` | JSON `{ config, packs }` (the backend needs pack cue definitions) |
//! | `audio_frame(listener, events_json, focus)` | JSON `{ commands, stats }` for this frame |
//! | `audio_set_siren(entity, mode)` | `""` (default) / `"Off"` / `"Auto"` / `"Wail"` / `"Yelp"` / `"Phaser"` / `"HiLo"` |
//! | `audio_set_scanner_tier(tier)` | `"None"` / `"Basic"` / `"Pro"` / `"Elite"` (wp_police tiers 0/1/2) |
//! | `audio_radio(unit, priority, text)` | queue a scanner line (debug until `PoliceEvent::Radio` exists) |
//! | `audio_reset()` | stop everything and forget transient state |

use std::cell::RefCell;

use serde::Serialize;
use wasm_bindgen::prelude::*;
use wp_audio::{
    AudioCommand, AudioDirector, AudioFrame, AudioLibrary, DirectorStats, Listener, RadioLine,
    RadioPriority, ScannerTier, SirenMode, SirenOverride, SirenPattern, VehicleAudioInput,
};
use wp_contracts::{EntityId, SimEvent};

use crate::{err, json, with_app};

thread_local! {
    static AUDIO: RefCell<Option<AudioDirector>> = const { RefCell::new(None) };
}

/// Fixed seed for cosmetic pitch variation (audio never feeds back into the simulation).
const AUDIO_SEED: u64 = 0xA0D10;

fn with_audio<T>(f: impl FnOnce(&mut AudioDirector) -> Result<T, String>) -> Result<T, JsValue> {
    AUDIO.with(|a| {
        let mut a = a.borrow_mut();
        if a.is_none() {
            let lib = AudioLibrary::embedded().map_err(err)?;
            *a = Some(AudioDirector::new(lib, AUDIO_SEED));
        }
        f(a.as_mut().expect("just created")).map_err(err)
    })
}

#[derive(Serialize)]
struct LibraryOut<'a> {
    config: &'a wp_audio::AudioConfig,
    packs: &'a std::collections::BTreeMap<String, wp_audio::SamplePack>,
}

/// JSON `{ config, packs }` of the embedded audio library.
#[wasm_bindgen]
pub fn audio_library() -> Result<String, JsValue> {
    with_audio(|d| {
        let lib = d.library();
        Ok(json(&LibraryOut {
            config: &lib.config,
            packs: &lib.packs,
        }))
    })
}

#[derive(Serialize)]
struct FrameOut<'a> {
    commands: &'a [AudioCommand],
    stats: &'a DirectorStats,
}

/// Run the director for this frame. `listener` = `[pos xyz, vel xyz, forward xyz, up xyz]`
/// (world, m and m/s), `events_json` = the `SimEvent`s drained this frame as JSON (as returned
/// by `events_decode`; empty string = none), `focus` = entity the camera follows (0 = none).
/// Returns JSON `{ commands: AudioCommand[], stats }`.
#[wasm_bindgen]
pub fn audio_frame(listener: &[f32], events_json: &str, focus: u32) -> Result<String, JsValue> {
    let at = |i: usize| listener.get(i).copied().unwrap_or(0.0);
    let lis = Listener {
        pos: [at(0), at(1), at(2)],
        vel: [at(3), at(4), at(5)],
        forward: [at(6), at(7), at(8)],
        up: [at(9), at(10), at(11)],
    };
    let events: Vec<SimEvent> = if events_json.trim().is_empty() {
        Vec::new()
    } else {
        serde_json::from_str(events_json).map_err(err)?
    };
    with_app(|app| {
        let s = &app.session;
        let states = s.sim().vehicle_states();
        let mut car_ids = Vec::with_capacity(states.len());
        for (id, _) in &states {
            let x = s
                .extra(*id)
                .ok_or_else(|| format!("unknown entity {}", id.0))?;
            car_ids.push(x.car.def.id.clone());
        }
        AUDIO.with(|a| -> Result<String, String> {
            let mut a = a.borrow_mut();
            if a.is_none() {
                let lib = AudioLibrary::embedded().map_err(|e| e.to_string())?;
                *a = Some(AudioDirector::new(lib, AUDIO_SEED));
            }
            let d = a.as_mut().expect("just created");
            for (id, _) in &states {
                let car = &s.extra(*id).expect("checked above").car;
                if !d.has_car(&car.def.id) {
                    d.register_car(car);
                }
            }
            let inputs: Vec<VehicleAudioInput> = states
                .iter()
                .zip(&car_ids)
                .map(|((id, st), car_id)| {
                    let input = s.input(*id);
                    VehicleAudioInput {
                        entity: *id,
                        car_id,
                        state: st,
                        throttle: input.throttle,
                        horn: input.horn,
                        siren: input.siren,
                        road_rumble: s.ffb(*id).map(|f| f.road_rumble).unwrap_or(0.0),
                    }
                })
                .collect();
            let commands = d.update(&AudioFrame {
                tick: s.tick(),
                listener: lis,
                focus: (focus != 0).then_some(EntityId(focus)),
                vehicles: &inputs,
                events: &events,
            });
            Ok(json(&FrameOut {
                commands: &commands,
                stats: d.stats(),
            }))
        })
    })
}

/// Override an entity's siren: `""`/`"Default"` = follow input and car default, `"Off"`,
/// `"Auto"`, or a fixed pattern `"Wail"`/`"Yelp"`/`"Phaser"`/`"HiLo"` (forces it on).
#[wasm_bindgen]
pub fn audio_set_siren(entity: u32, mode: &str) -> Result<(), JsValue> {
    let ov = match mode {
        "" | "Default" => None,
        "Off" => Some(SirenOverride {
            on: false,
            mode: SirenMode::Off,
        }),
        "Auto" => Some(SirenOverride {
            on: true,
            mode: SirenMode::Auto,
        }),
        other => Some(SirenOverride {
            on: true,
            mode: SirenMode::Fixed(
                SirenPattern::parse(other)
                    .ok_or_else(|| err(format!("unknown siren mode {other:?}")))?,
            ),
        }),
    };
    with_audio(|d| {
        d.set_siren_override(EntityId(entity), ov);
        Ok(())
    })
}

/// Set the listener's scanner tier.
#[wasm_bindgen]
pub fn audio_set_scanner_tier(tier: &str) -> Result<(), JsValue> {
    let t =
        ScannerTier::parse(tier).ok_or_else(|| err(format!("unknown scanner tier {tier:?}")))?;
    with_audio(|d| {
        d.set_scanner_tier(t);
        Ok(())
    })
}

/// Queue a scanner line as if police emitted it now. `priority`: `"Chatter"` / `"Routine"` /
/// `"Dispatch"` / `"Pursuit"` / `"Urgent"`.
#[wasm_bindgen]
pub fn audio_radio(unit: u32, priority: &str, text: &str) -> Result<(), JsValue> {
    let priority: RadioPriority = serde_json::from_str(&format!("{priority:?}")).map_err(err)?;
    let tick = with_app(|a| Ok(a.session.tick()))?;
    with_audio(|d| {
        d.push_radio_line(RadioLine {
            unit,
            text: text.to_string(),
            priority,
            emitted_tick: tick,
        });
        Ok(())
    })
}

/// Stop every voice and forget transient audio state. Returns the stop commands as JSON.
#[wasm_bindgen]
pub fn audio_reset() -> Result<String, JsValue> {
    with_audio(|d| {
        let mut out = Vec::new();
        d.reset(&mut out);
        Ok(json(&out))
    })
}
