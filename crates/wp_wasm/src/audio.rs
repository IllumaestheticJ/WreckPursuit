//! Audio surface (added by workstream 13, audio): runs `wp_audio::AudioDirector` against the
//! live session and returns its commands as JSON for the Web Audio backend in
//! `apps/testbed-web/src/audio/`. Purely additive; nothing else in this crate depends on it.
//!
//! | function | what |
//! |---|---|
//! | `audio_library()` | JSON `{ config, packs }` (the backend needs pack cue definitions) |
//! | `audio_frame(listener, events_json, focus)` | JSON `{ commands, stats, siren }` for this frame (`siren` = siren v2 acoustics, `wp_audio::SirenSpaceFrame`) |
//! | `audio_set_siren(entity, mode)` | `""` (default) / `"Off"` / `"Auto"` / `"Wail"` / `"Yelp"` / `"Phaser"` / `"HiLo"` |
//! | `audio_set_scanner_tier(tier)` | `"None"` / `"Basic"` / `"Pro"` / `"Elite"` (wp_police tiers 0/1/2) |
//! | `audio_radio(unit, priority, text)` | queue a scanner line (debug until `PoliceEvent::Radio` exists) |
//! | `audio_reset()` | stop everything and forget transient state |

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde::Serialize;
use wasm_bindgen::prelude::*;
use wp_audio::{
    AudioCommand, AudioDirector, AudioFrame, AudioLibrary, DirectorStats, DriverKind, Listener,
    RadioLine, RadioPriority, ScannerTier, SirenMode, SirenOverride, SirenPattern,
    VehicleAudioInput,
};
use wp_contracts::{EntityId, SimEvent};

use crate::{err, json, with};

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
    /// Siren v2 acoustics for this frame's sirens (occlusion, slap echo, tunnel, ducks).
    siren: &'a wp_audio::SirenSpaceFrame,
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
    with(|s| {
        AUDIO.with(|a| -> Result<String, String> {
            let mut a = a.borrow_mut();
            if a.is_none() {
                let lib = AudioLibrary::embedded().map_err(|e| e.to_string())?;
                *a = Some(AudioDirector::new(lib, AUDIO_SEED));
            }
            let d = a.as_mut().expect("just created");
            let focus = (focus != 0).then_some(EntityId(focus));
            let commands = frame_commands(s, d, lis, focus, &events);
            let siren = d.siren_space(&lis, &crate::siren_cue::WorldAcoustics(s.sim().world()));
            Ok(json(&FrameOut {
                commands: &commands,
                stats: d.stats(),
                siren: &siren,
            }))
        })
    })
}

/// One director frame against the live session (native-testable core of [`audio_frame`]).
/// Applied input and driver kind come from each sim vehicle, so AI racers, police units and
/// physical traffic (spawned by the sim, not the session) are heard truthfully. Cars the
/// director does not know yet are registered from the session or the embedded data; an
/// unknown car is skipped rather than silencing everything.
pub(crate) fn frame_commands(
    s: &crate::session::Session,
    d: &mut AudioDirector,
    listener: Listener,
    focus: Option<EntityId>,
    events: &[SimEvent],
) -> Vec<AudioCommand> {
    let sim = s.sim();
    let states = sim.vehicle_states();
    let vehicles: BTreeMap<EntityId, &wp_sim::SimVehicle> =
        sim.vehicles().iter().map(|v| (v.id, v)).collect();
    let positions: BTreeMap<EntityId, [f32; 3]> =
        states.iter().map(|(id, st)| (*id, st.pos)).collect();
    let mut inputs: Vec<VehicleAudioInput> = Vec::with_capacity(states.len());
    for (id, st) in &states {
        let Some(v) = vehicles.get(id) else {
            continue;
        };
        if !d.has_car(&v.car_id) {
            match s.car(*id) {
                Some(car) => d.register_car(car),
                None => match crate::data::load_car(&v.car_id) {
                    Ok(car) => d.register_car(&car),
                    Err(_) => continue,
                },
            }
        }
        // Siren v2: the unit's AI state picks the pattern (read-only; audio never feeds the sim).
        d.set_siren_cue(*id, crate::siren_cue::vehicle_cue(sim, v, &positions));
        inputs.push(VehicleAudioInput {
            entity: *id,
            car_id: &v.car_id,
            state: st,
            throttle: v.input.throttle,
            // What reaches the engine (filters, TCS, the gearbox's shift cut): shift notes.
            throttle_applied: Some(v.vehicle.telemetry().throttle),
            horn: v.input.horn,
            siren: crate::police::siren_on(sim, v),
            road_rumble: s.ffb(*id).map(|f| f.road_rumble).unwrap_or(0.0),
            kind: driver_kind(v.kind),
        });
    }
    d.update(&AudioFrame {
        tick: s.tick(),
        listener,
        focus,
        vehicles: &inputs,
        events,
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
    let tick = with(|s| Ok(s.tick()))?;
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

fn driver_kind(k: wp_sim::VehicleKind) -> DriverKind {
    match k {
        wp_sim::VehicleKind::Player => DriverKind::Player,
        wp_sim::VehicleKind::AiRacer => DriverKind::AiRacer,
        wp_sim::VehicleKind::Police => DriverKind::Police,
        wp_sim::VehicleKind::Traffic => DriverKind::Traffic,
    }
}

#[cfg(test)]
mod tests {
    //! The bridge against real lead-run-5 police units and physical traffic in the city.
    use super::*;
    use crate::scenario::free_drive;
    use crate::session::Session;
    use wp_audio::types::slot;
    use wp_audio::{Bus, Priority};
    use wp_contracts::{DamageMode, VehicleInput};

    /// A speeding player in the city: police units arrive as sim vehicles with their siren on,
    /// the bridge gives them siren voices, traffic cars get low-priority engines, and nothing
    /// is skipped as unregistered.
    #[test]
    fn city_police_sirens_and_traffic_engines_are_heard() {
        let mut s = Session::new(free_drive("debug_sedan", DamageMode::Arcade, true), false)
            .expect("city session");
        let player = EntityId(1);
        let mut d = AudioDirector::new(AudioLibrary::embedded().unwrap(), AUDIO_SEED);
        let (mut police_siren_on, mut siren_voice, mut traffic_engine) = (false, false, false);
        let mut police_ids = std::collections::BTreeSet::new();
        // Both show up about 17 s in (seed 1); 30 s of sim time leaves margin.
        for _ in 0..60 * 30 {
            s.set_input(
                player,
                VehicleInput {
                    throttle: 1.0,
                    ..Default::default()
                },
            )
            .unwrap();
            s.step(1.0 / 60.0);
            let events = s.drain_events();
            let sim = s.sim();
            for v in sim.vehicles() {
                if v.kind == wp_sim::VehicleKind::Police {
                    police_ids.insert(v.id.0);
                    // The bridge's rule: lights and siren follow the unit's assignment.
                    police_siren_on |= crate::police::siren_on(sim, v);
                }
            }
            // Listener rides with the player, like the chase camera.
            let pos = sim
                .vehicle_states()
                .iter()
                .find(|(id, _)| *id == player)
                .map(|(_, st)| st.pos)
                .unwrap_or([0.0; 3]);
            let lis = Listener {
                pos,
                vel: [0.0; 3],
                forward: [0.0, 1.0, 0.0],
                up: [0.0, 0.0, 1.0],
            };
            let traffic: std::collections::BTreeSet<u32> = sim
                .vehicles()
                .iter()
                .filter(|v| v.kind == wp_sim::VehicleKind::Traffic)
                .map(|v| v.id.0)
                .collect();
            for c in frame_commands(&s, &mut d, lis, Some(player), &events) {
                if let AudioCommand::Play {
                    voice,
                    bus,
                    priority,
                    ..
                } = c
                {
                    let is_siren = (slot::SIREN..slot::SIREN + 8).contains(&voice.slot);
                    if is_siren && bus == Bus::Police && police_ids.contains(&voice.entity) {
                        siren_voice = true;
                    }
                    if bus == Bus::Engine
                        && traffic.contains(&voice.entity)
                        && priority == Priority::Low
                    {
                        traffic_engine = true;
                    }
                }
            }
            assert_eq!(d.stats().unregistered, 0, "every sim car has audio");
            if siren_voice && traffic_engine {
                break;
            }
        }
        assert!(!police_ids.is_empty(), "police units became sim vehicles");
        assert!(
            police_siren_on,
            "a responding / pursuing unit has its siren on"
        );
        assert!(siren_voice, "police sirens get voices on the police bus");
        assert!(
            traffic_engine,
            "physical traffic engines play at low priority"
        );
    }
}
