//! Roster audibility (lead round 12b, playtest #47 "some cars are missing sounds"): every car
//! in `data/cars`, as the player, an AI racer, a police unit and traffic, gets an audible
//! engine, tyre squeal and wheelspin from the director, police units get a siren, and in a
//! 24-car crowd the voice limits no longer starve nearby cars of their engine.
//!
//! The browser half (the same scenarios rendered through the real Web Audio voice pool, recorded
//! and procedural) is `apps/testbed-web/e2e/audio_roster.mjs`.

#[path = "support/roster.rs"]
mod roster;

use roster::*;
use wp_audio::types::slot;
use wp_audio::*;
use wp_contracts::{EntityId, LoadedCar, VehicleState};

#[test]
fn every_car_is_audible_in_every_role() {
    let mut problems = Vec::new();
    for car in roster() {
        for (kind, dist) in ROLES {
            let frames = run_case(&car, kind, dist);
            let name = format!("{} as {} @{dist} m", car.def.id, role_name(kind));
            // Engine: from the second frame of the sweep to the end, never silent.
            let sweep_end = (SWEEP_S * FPS as f32) as usize;
            let silent = frames[1..]
                .iter()
                .filter(|f| engine_level(f) <= 0.0)
                .count();
            if silent > 0 {
                problems.push(format!("{name}: engine silent in {silent} frames"));
            }
            // A clearly audible level, scaled for distance (inverse distance from 6 m).
            let want = 0.25
                * (6.0 / dist).min(1.0)
                * if kind == DriverKind::Traffic {
                    0.6
                } else {
                    1.0
                };
            let peak = frames.iter().map(engine_level).fold(0.0, f32::max);
            if peak < want {
                problems.push(format!("{name}: engine peak {peak:.4} < {want:.4}"));
            }
            // Wheelspin and squeal in the spin phase.
            let spin = &frames[sweep_end + 20..];
            if !spin.iter().all(|f| slot_gain(f, slot::SPIN) > 0.0) {
                problems.push(format!("{name}: no wheelspin loop"));
            }
            if !spin.iter().all(|f| slot_gain(f, slot::SQUEAL) > 0.0) {
                problems.push(format!("{name}: no tyre squeal"));
            }
            // Siren on police units.
            if kind == DriverKind::Police && !frames[2..].iter().all(|f| siren_gain(f) > 0.0) {
                problems.push(format!("{name}: no siren"));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// A car cruising along +Y at `pos`, engine at 45 % of the way from idle to redline.
fn cruising(car: &LoadedCar, pos: [f32; 3]) -> VehicleState {
    let e = &car.def.engine;
    VehicleState {
        pos,
        rot: [0.0, 0.0, 0.0, 1.0],
        lin_vel: [0.0, 15.0, 0.0],
        engine_rpm: e.idle_rpm + 0.45 * (e.redline_rpm - e.idle_rpm),
        gear: 3,
        speed_mps: 15.0,
        health: 1.0,
        ..Default::default()
    }
}

/// 24 cars (the roster, repeated) on a spiral from 10 m to 194 m, a quarter of them police
/// with sirens, half traffic. Returns per car: (distance, engine level, layers playing).
fn crowd(frames: u64) -> Vec<(String, DriverKind, f32, f32, usize)> {
    let cars = roster();
    let mut set = Vec::new();
    for i in 0..24usize {
        let c = &cars[i % cars.len()];
        let ang = i as f32 * 0.7;
        let r = 10.0 + 8.0 * i as f32;
        let kind = match i % 4 {
            0 => DriverKind::AiRacer,
            2 => DriverKind::Police,
            _ => DriverKind::Traffic,
        };
        let (s, co) = (wp_math::sin_cos(ang).0, wp_math::sin_cos(ang).1);
        set.push((c, cruising(c, [r * co, r * s, 0.5]), kind, r));
    }
    let mut d = AudioDirector::new(AudioLibrary::embedded().unwrap(), 3);
    for (c, ..) in &set {
        d.register_car(c);
    }
    let mut playing: std::collections::BTreeMap<(u32, u32), (Bus, f32)> = Default::default();
    for f in 0..frames {
        let inputs: Vec<VehicleAudioInput> = set
            .iter()
            .enumerate()
            .map(|(i, (c, st, kind, _))| VehicleAudioInput {
                entity: EntityId(i as u32 + 1),
                car_id: &c.def.id,
                state: st,
                throttle: 0.5,
                throttle_applied: Some(0.5),
                horn: false,
                siren: *kind == DriverKind::Police,
                road_rumble: 0.0,
                kind: *kind,
            })
            .collect();
        for c in d.update(&AudioFrame {
            tick: TICKS_PER_FRAME * (f + 1),
            listener: listener(),
            focus: None,
            vehicles: &inputs,
            events: &[],
        }) {
            match c {
                AudioCommand::Play {
                    voice,
                    bus,
                    looped: true,
                    params,
                    ..
                } => {
                    playing.insert((voice.entity, voice.slot), (bus, params.gain));
                }
                AudioCommand::Set { voice, params } => {
                    if let Some(v) = playing.get_mut(&(voice.entity, voice.slot)) {
                        v.1 = params.gain;
                    }
                }
                AudioCommand::Stop { voice, .. } => {
                    playing.remove(&(voice.entity, voice.slot));
                }
                _ => {}
            }
        }
    }
    set.iter()
        .enumerate()
        .map(|(i, (c, _, kind, r))| {
            let e = i as u32 + 1;
            let layers: Vec<f32> = playing
                .iter()
                .filter(|((pe, s), (b, _))| {
                    *pe == e
                        && *b == Bus::Engine
                        && (slot::ENGINE_LAYER..=slot::ENGINE_LIMITER).contains(s)
                })
                .map(|(_, (_, g))| *g)
                .collect();
            let level = layers.iter().map(|g| g * g).sum::<f32>().sqrt();
            (c.def.id.clone(), *kind, *r, level, layers.len())
        })
        .collect()
}

#[test]
fn a_crowd_does_not_starve_nearby_cars_of_their_engine() {
    // Before the round-12b fix (per-layer culling, 16 Engine voices) traffic from 50 m and
    // every car from 154 m was silent here.
    let rows = crowd(30);
    let mut problems = Vec::new();
    for (id, kind, r, level, layers) in &rows {
        if *r <= 140.0 && *level <= 0.0 {
            problems.push(format!("{id} ({kind:?}) at {r} m has no engine"));
        }
        // Engine LOD: two layers near, one beyond lod_two_layer_m.
        if *r > 40.0 && *layers > 1 {
            problems.push(format!(
                "{id} at {r} m plays {layers} engine layers (LOD wants 1)"
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn engine_lod_keeps_loudness() {
    // The same car alone at 80 m: one layer, as loud (energy) as all its layers would be.
    let cars = roster();
    let car = cars.iter().find(|c| c.def.id == "rampart_50").unwrap();
    let lib = AudioLibrary::embedded().unwrap();
    let mut energies = Vec::new();
    for (focus, want_layers) in [(true, 2usize), (false, 1)] {
        let mut d = AudioDirector::new(lib.clone(), 1);
        d.register_car(car);
        let st = cruising(car, [80.0, 0.0, 0.5]);
        let mut gains = std::collections::BTreeMap::new();
        for f in 0..30u64 {
            let inp = [VehicleAudioInput {
                entity: EntityId(1),
                car_id: &car.def.id,
                state: &st,
                throttle: 0.5,
                throttle_applied: Some(0.5),
                horn: false,
                siren: false,
                road_rumble: 0.0,
                kind: DriverKind::AiRacer,
            }];
            for c in d.update(&AudioFrame {
                tick: 4 * (f + 1),
                listener: listener(),
                focus: focus.then_some(EntityId(1)),
                vehicles: &inp,
                events: &[],
            }) {
                match c {
                    AudioCommand::Play { voice, params, .. }
                    | AudioCommand::Set { voice, params } => {
                        if (slot::ENGINE_LAYER..slot::ENGINE_LIMITER).contains(&voice.slot) {
                            gains.insert(voice.slot, params.gain);
                        }
                    }
                    AudioCommand::Stop { voice, .. } => {
                        gains.remove(&voice.slot);
                    }
                    _ => {}
                }
            }
        }
        let e: f32 = gains.values().map(|g| g * g).sum();
        if focus {
            assert!(
                gains.len() >= want_layers,
                "focus keeps its crossfade layers: {gains:?}"
            );
        } else {
            assert_eq!(gains.len(), want_layers, "far AI car: {gains:?}");
        }
        energies.push(e);
    }
    let (all, lod) = (energies[0], energies[1]);
    assert!(all > 0.0);
    assert!(
        (all - lod).abs() <= 1e-3 * all,
        "LOD changed loudness: {energies:?}"
    );
}

#[test]
fn kinematic_traffic_and_police_are_heard() {
    // What the wasm bridge would feed for body-less agents (request in docs/integration/audio.md).
    for car in roster() {
        for (kind, siren, dist) in [
            (DriverKind::Traffic, false, 110.0),
            (DriverKind::Police, true, 150.0),
        ] {
            let mut d = AudioDirector::new(AudioLibrary::embedded().unwrap(), 5);
            d.register_car(&car);
            let mut engine = false;
            let mut siren_on = false;
            for f in 0..20u64 {
                let st = wp_audio::kinematic::cruise_state(&car.def, [dist, 0.0, 0.4], 0.0, 13.0);
                let inp = [VehicleAudioInput {
                    entity: EntityId(0x4000_0001),
                    car_id: &car.def.id,
                    state: &st,
                    throttle: 0.3,
                    throttle_applied: Some(0.3),
                    horn: false,
                    siren,
                    road_rumble: 0.0,
                    kind,
                }];
                for c in d.update(&AudioFrame {
                    tick: 4 * (f + 1),
                    listener: listener(),
                    focus: None,
                    vehicles: &inp,
                    events: &[],
                }) {
                    if let AudioCommand::Play { voice, .. } = c {
                        engine |= (slot::ENGINE_LAYER..slot::ENGINE_LIMITER).contains(&voice.slot);
                        siren_on |= (slot::SIREN..slot::SIREN + 4).contains(&voice.slot);
                    }
                }
            }
            assert!(
                engine,
                "{} as kinematic {kind:?} at {dist} m: no engine",
                car.def.id
            );
            assert_eq!(
                siren_on, siren,
                "{} as kinematic {kind:?}: siren",
                car.def.id
            );
        }
    }
}
