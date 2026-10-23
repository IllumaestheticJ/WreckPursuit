//! Police audio investigation (owner feedback on preview 0.12.34: "police car audio pitch is
//! wrong, some of it sounds like it's singing, some of it sounds like it's laughing").
//!
//! An ignored test that drives a real city pursuit (player wanted at L2, full throttle, listener
//! riding with the player like the chase camera, modelled engines on as in the browser) and
//! summarises every police voice the director plays: recorded engine-layer pitch ratios, engine
//! model switches (modelled ↔ layers), siren pattern switches and Doppler swings. Optionally
//! writes the per-frame trace as JSON for offline rendering.
//!
//! `POLICE_TRACE=/tmp/police.json cargo test -p wp_wasm --release --lib police_audio_trace -- --ignored --nocapture`

use std::collections::BTreeMap;

use serde::Serialize;
use wp_audio::types::slot;
use wp_audio::{AudioCommand, AudioDirector, AudioLibrary, Listener};
use wp_contracts::{DamageMode, EntityId, VehicleInput};

use crate::scenario::free_drive;
use crate::session::Session;

/// One police voice in one frame.
#[derive(Debug, Clone, Serialize)]
struct Row {
    t: f32,
    entity: u32,
    slot: u32,
    cue: String,
    pack: String,
    pitch: f32,
    freq_hz: f32,
    gain: f32,
    doppler: f32,
    dist_m: f32,
    rpm: f32,
    speed: f32,
}

#[test]
#[ignore = "investigation tool: long city run, prints a report"]
fn police_audio_trace() {
    let mut s = Session::new(free_drive("debug_sedan", DamageMode::Arcade, true), false)
        .expect("city session");
    let player = EntityId(1);
    let mut d = AudioDirector::new(AudioLibrary::embedded().unwrap(), 0xA0D10);
    // The browser runs the modelled engine for the focus + nearest cars once its worklet starts.
    d.set_engine_model(wp_audio::director::EngineModelMode::Physical);
    // A close chase: park in front of the first patrol car (as e2e/run.mjs does), make the
    // player wanted, then flee at part throttle so units stay close, pass and circle.
    let heat = wp_police::PoliceConfig::builtin()
        .heat
        .heat
        .level_thresholds[2];
    let mut parked = false;
    for _ in 0..60 * 120 {
        s.set_input(player, VehicleInput::default()).unwrap();
        s.step(1.0 / 60.0);
        s.drain_events();
        if s.debug_park_near_police(player, 30.0).is_ok() {
            parked = true;
            break;
        }
    }
    assert!(parked, "a police car spawned");
    s.sim_mut().police_mut().set_heat(player, heat + 1.0);
    let mut voices: BTreeMap<(u32, u32), (String, String)> = BTreeMap::new();
    let mut rows: Vec<Row> = Vec::new();
    let frames = 60 * 90;
    for f in 0..frames {
        // Weave a little so the chase has corners and units pass / circle.
        let steer = if (f / 240) % 2 == 0 { 0.15 } else { -0.15 };
        s.set_input(
            player,
            VehicleInput {
                throttle: if f < 120 { 0.0 } else { 0.65 },
                steer,
                ..Default::default()
            },
        )
        .unwrap();
        s.step(1.0 / 60.0);
        let events = s.drain_events();
        let sim = s.sim();
        let states: BTreeMap<u32, wp_contracts::VehicleState> = sim
            .vehicle_states()
            .into_iter()
            .map(|(id, st)| (id.0, st))
            .collect();
        let pos = states.get(&player.0).map(|st| st.pos).unwrap_or([0.0; 3]);
        let vel = states
            .get(&player.0)
            .map(|st| st.lin_vel)
            .unwrap_or([0.0; 3]);
        let lis = Listener {
            pos: [pos[0], pos[1], pos[2] + 1.5],
            vel,
            forward: [0.0, 1.0, 0.0],
            up: [0.0, 0.0, 1.0],
        };
        let police: Vec<u32> = sim
            .vehicles()
            .iter()
            .filter(|v| v.kind == wp_sim::VehicleKind::Police)
            .map(|v| v.id.0)
            .collect();
        let cmds = crate::audio::frame_commands(&s, &mut d, lis, Some(player), &events);
        for c in &cmds {
            let (voice, params) = match c {
                AudioCommand::Play {
                    voice,
                    params,
                    cue,
                    pack,
                    ..
                } => {
                    voices.insert((voice.entity, voice.slot), (cue.clone(), pack.clone()));
                    (voice, params)
                }
                AudioCommand::Set { voice, params } => (voice, params),
                AudioCommand::Stop { voice, .. } => {
                    voices.remove(&(voice.entity, voice.slot));
                    continue;
                }
                _ => continue,
            };
            if !police.contains(&voice.entity) {
                continue;
            }
            let Some((cue, pack)) = voices.get(&(voice.entity, voice.slot)) else {
                continue;
            };
            let st = &states[&voice.entity];
            let dp = [st.pos[0] - pos[0], st.pos[1] - pos[1], st.pos[2] - pos[2]];
            rows.push(Row {
                t: f as f32 / 60.0,
                entity: voice.entity,
                slot: voice.slot,
                cue: cue.clone(),
                pack: pack.clone(),
                pitch: params.pitch,
                freq_hz: params.freq_hz,
                gain: params.gain,
                doppler: params.doppler,
                dist_m: (dp[0] * dp[0] + dp[1] * dp[1] + dp[2] * dp[2]).sqrt(),
                rpm: st.engine_rpm,
                speed: st.speed_mps,
            });
        }
    }
    if let Ok(path) = std::env::var("POLICE_TRACE") {
        std::fs::write(&path, serde_json::to_string(&rows).unwrap()).unwrap();
        eprintln!("trace → {path} ({} rows)", rows.len());
    }

    // ---- report -------------------------------------------------------------------------
    let is_layer = |r: &Row| r.slot >= slot::ENGINE_LAYER && r.slot < slot::ENGINE_LIMITER;
    let is_siren = |r: &Row| (slot::SIREN..slot::SIREN + 4).contains(&r.slot);
    let mut ents: Vec<u32> = rows.iter().map(|r| r.entity).collect();
    ents.sort();
    ents.dedup();
    eprintln!("police units heard: {ents:?}");
    for e in ents {
        let mine: Vec<&Row> = rows.iter().filter(|r| r.entity == e).collect();
        let layers: Vec<&&Row> = mine
            .iter()
            .filter(|r| is_layer(r) && r.gain > 0.01)
            .collect();
        let mut pitches: Vec<f32> = layers
            .iter()
            .map(|r| r.pitch / r.doppler.max(0.1))
            .collect();
        pitches.sort_by(f32::total_cmp);
        let pct = |p: f32| {
            pitches
                .get(((pitches.len() as f32 - 1.0) * p) as usize)
                .copied()
                .unwrap_or(0.0)
        };
        let loud_high = layers
            .iter()
            .filter(|r| r.pitch / r.doppler.max(0.1) > 1.6)
            .count();
        // Engine model switches: frames alternating between `engine/physical` and layer cues.
        let mut model: Vec<(f32, bool)> = Vec::new();
        for r in mine.iter().filter(|r| r.cue.starts_with("engine/")) {
            let phys = r.cue == "engine/physical";
            if model.last().is_none_or(|m| m.0 != r.t) {
                model.push((r.t, phys));
            } else if phys {
                model.last_mut().unwrap().1 = true;
            }
        }
        let switches = model.windows(2).filter(|w| w[0].1 != w[1].1).count();
        let phys_share = model.iter().filter(|m| m.1).count() as f32 / model.len().max(1) as f32;
        // Siren patterns heard, switches, Doppler swing.
        let sir: Vec<&&Row> = mine.iter().filter(|r| is_siren(r)).collect();
        let mut pat_switches = 0;
        let mut last_slot = None;
        let mut last_t = -1.0;
        for r in &sir {
            if r.t != last_t {
                if last_slot.is_some_and(|s| s != r.slot) {
                    pat_switches += 1;
                }
                last_slot = Some(r.slot);
                last_t = r.t;
            }
        }
        let dop: Vec<f32> = sir.iter().map(|r| r.doppler).collect();
        let dmin = dop.iter().copied().fold(f32::INFINITY, f32::min);
        let dmax = dop.iter().copied().fold(0.0f32, f32::max);
        let siren_s = sir.len() as f32 / 60.0;
        let cues: std::collections::BTreeSet<&str> = mine.iter().map(|r| r.cue.as_str()).collect();
        let packs: std::collections::BTreeSet<&str> =
            mine.iter().map(|r| r.pack.as_str()).collect();
        let dist: Vec<f32> = mine.iter().map(|r| r.dist_m).collect();
        eprintln!(
            "unit {e}: {:.0}–{:.0} m, packs {packs:?}\n  engine: modelled {:.0} % of frames, {switches} model switches; \
             layer pitch (no Doppler) p50 {:.2} p90 {:.2} max {:.2}; {} layer-frames above 1.6×\n  \
             siren: {siren_s:.1} s, {pat_switches} pattern switches, Doppler {dmin:.3}–{dmax:.3}\n  cues {cues:?}",
            dist.iter().copied().fold(f32::INFINITY, f32::min),
            dist.iter().copied().fold(0.0f32, f32::max),
            phys_share * 100.0,
            pct(0.5),
            pct(0.9),
            pct(1.0),
            loud_high,
        );
    }
}
