//! The physically modelled engine voice (row 73): what the director sends the engine
//! synthesizer, which cars get it, and that layers stay the fallback.

#[path = "support/engine_sweeps.rs"]
mod engine_sweeps;

use std::collections::BTreeMap;

use engine_sweeps::{self as sw, car, run, CARS};
use wp_audio::types::slot;
use wp_audio::*;
use wp_contracts::{EntityId, VehicleState};

fn plays(cmds: &[AudioCommand]) -> Vec<(u32, u32, String, BTreeMap<String, f32>)> {
    cmds.iter()
        .filter_map(|c| match c {
            AudioCommand::Play {
                voice, cue, synth, ..
            } => Some((voice.entity, voice.slot, cue.clone(), synth.clone())),
            _ => None,
        })
        .collect()
}

fn engine_frames(frames: &[sw::SweepFrame]) -> Vec<(f32, EngineFrame)> {
    let mut out = Vec::new();
    for f in frames {
        for c in &f.commands {
            let p = match c {
                AudioCommand::Play { voice, params, .. } | AudioCommand::Set { voice, params }
                    if voice.slot == slot::ENGINE_PHYSICAL =>
                {
                    params
                }
                _ => continue,
            };
            out.push((f.t, p.engine.expect("engine frame on the physical voice")));
        }
    }
    out
}

/// Every sweep car: one `engine/physical` voice, no layers, the car's firing events in the
/// synth map and the scripted state in every frame.
#[test]
fn focus_car_is_one_modelled_voice_with_its_engine() {
    for id in CARS {
        let c = car(id);
        let frames = run(&c, "wot", EngineTune::default());
        let first = plays(&frames[0].commands);
        let engine: Vec<_> = first
            .iter()
            .filter(|p| p.1 >= 0x100 && p.1 < 0x200)
            .collect();
        assert_eq!(engine.len(), 1, "{id}: one engine voice, got {engine:?}");
        let (_, s, cue, synth) = engine[0];
        assert_eq!(*s, slot::ENGINE_PHYSICAL, "{id}");
        assert_eq!(cue, "engine/physical");
        let def = AudioLibrary::embedded().unwrap().car_or_fallback(&c.def);
        assert_eq!(synth["n"], def.engine.cylinders as f32, "{id}");
        assert!(synth.values().all(|v| v.is_finite()), "{id}");
        assert_eq!(synth["redline_rpm"], c.def.engine.redline_rpm);
        assert_eq!(synth["turbo"], if def.engine.turbo { 1.0 } else { 0.0 });
        // No layer, limiter, turbo or pop voices ever (the synthesizer makes them).
        for f in &frames {
            for p in plays(&f.commands) {
                assert!(
                    !(p.1 >= slot::ENGINE_LAYER && p.1 <= slot::SUPERCHARGER),
                    "{id}: stray engine voice {p:?}"
                );
                assert!(
                    !p.2.starts_with("exhaust/") && !p.2.starts_with("turbo/"),
                    "{id}: {p:?}"
                );
            }
        }
        let ef = engine_frames(&frames);
        assert_eq!(ef.len(), frames.len(), "{id}: an engine frame every frame");
        for ((t, e), f) in ef.iter().zip(&frames) {
            assert_eq!(*t, f.t);
            assert_eq!(e.rpm, f.sample.rpm, "{id} rpm at {t}");
            assert_eq!(e.throttle, f.sample.applied);
            assert_eq!(e.gear, f.sample.gear as f32);
            assert!((e.speed_mps - f.sample.speed).abs() < 1e-4);
            assert_eq!(e.health, 1.0);
        }
    }
}

/// A lift under boost fires the blow-off counter; the shift sweep carries the cut; the
/// damaged sweep carries the engine health (misfires happen in the synthesizer).
#[test]
fn turbo_blowoff_shift_cut_and_health_reach_the_synth() {
    let turbo = car("shiro_turbo");
    let lift = engine_frames(&run(&turbo, "lift", EngineTune::default()));
    let before = lift.iter().find(|(t, _)| *t > 0.6).unwrap().1;
    let after = lift.last().unwrap().1;
    assert!(before.boost > 0.3, "boost under WOT: {}", before.boost);
    assert_eq!(
        after.blowoffs,
        before.blowoffs + 1.0,
        "one blow-off on the lift"
    );
    let v8 = car("rampart_50");
    let shift = engine_frames(&run(&v8, "shift", EngineTune::default()));
    assert!(shift
        .iter()
        .any(|(t, e)| *t > 1.2 && *t < 1.4 && e.throttle == 0.0 && e.pedal == 1.0));
    let dmg = engine_frames(&run(&v8, "damaged", EngineTune::default()));
    assert!(dmg.iter().all(|(_, e)| (e.health - 0.35).abs() < 1e-6));
    // The director keeps no boost for an atmospheric engine.
    assert!(shift.iter().all(|(_, e)| e.boost == 0.0));
}

/// Installed mods change the synth description (louder exhaust, less muffler, more crackle).
#[test]
fn exhaust_mods_change_the_voice() {
    let c = car("debug_sedan");
    let synth_of = |tune| {
        let f = run(&c, "steps", tune);
        plays(&f[0].commands)
            .into_iter()
            .find(|p| p.1 == slot::ENGINE_PHYSICAL)
            .unwrap()
            .3
    };
    let stock = synth_of(EngineTune::default());
    let modded = synth_of(EngineTune::from_mods(&["engine_exhaust_2", "fi_turbo_1"]));
    assert!(modded["exhaust_gain"] > stock["exhaust_gain"]);
    assert!(modded["muffler"] < stock["muffler"]);
    assert!(modded["crackle"] > stock["crackle"]);
    assert_eq!(stock["turbo"], 0.0);
    assert_eq!(modded["turbo"], 1.0);
}

fn crowd_frame(
    dir: &mut AudioDirector,
    tick: u64,
    cars: &[(EntityId, &str, VehicleState)],
    focus: EntityId,
) -> Vec<AudioCommand> {
    let inputs: Vec<VehicleAudioInput> = cars
        .iter()
        .map(|(e, id, st)| VehicleAudioInput {
            entity: *e,
            car_id: id,
            state: st,
            throttle: 0.5,
            throttle_applied: None,
            horn: false,
            siren: false,
            road_rumble: 0.0,
            kind: DriverKind::AiRacer,
        })
        .collect();
    dir.update(&AudioFrame {
        tick,
        listener: Listener {
            pos: [0.0, 0.0, 1.5],
            ..Default::default()
        },
        focus: Some(focus),
        vehicles: &inputs,
        events: &[],
    })
}

/// The focus car plus the six nearest within 60 m get the model; farther cars keep layers;
/// the set is sticky; switching back to layers moves every car over.
#[test]
fn modelled_set_is_focus_plus_six_nearest_with_hysteresis() {
    let c = car("rampart_50");
    let mut dir = AudioDirector::new(AudioLibrary::embedded().unwrap(), 3);
    dir.register_car(&c);
    dir.set_engine_model(EngineModelMode::Physical);
    let mk = |d: f32| {
        let s = sw::Sample {
            rpm: 3000.0,
            pedal: 0.5,
            applied: 0.5,
            gear: 3,
            speed: 0.0,
            health: 1.0,
        };
        let mut st = sw::state(&s);
        st.pos = [d, 0.0, 0.5];
        st
    };
    // Focus 90 m away (still modelled); others at 5, 10, … 50 m and 70 m.
    let mut cars: Vec<(EntityId, &str, VehicleState)> = vec![(EntityId(1), "rampart_50", mk(90.0))];
    for (i, d) in [5.0, 10.0, 15.0, 20.0, 25.0, 30.0, 35.0, 50.0, 70.0]
        .iter()
        .enumerate()
    {
        cars.push((EntityId(10 + i as u32), "rampart_50", mk(*d)));
    }
    let mut voiced = BTreeMap::<u32, bool>::new();
    let apply = |cmds: &[AudioCommand], voiced: &mut BTreeMap<u32, bool>| {
        for cmd in cmds {
            match cmd {
                AudioCommand::Play { voice, .. } if voice.slot == slot::ENGINE_PHYSICAL => {
                    voiced.insert(voice.entity, true);
                }
                AudioCommand::Stop { voice, .. } if voice.slot == slot::ENGINE_PHYSICAL => {
                    voiced.remove(&voice.entity);
                }
                _ => {}
            }
        }
    };
    let out = crowd_frame(&mut dir, 100, &cars, EntityId(1));
    apply(&out, &mut voiced);
    let set: Vec<u32> = voiced.keys().copied().collect();
    assert_eq!(set, vec![1, 10, 11, 12, 13, 14, 15], "focus + six nearest");
    // Cars outside the set still have their layer engines.
    for e in [16u32, 17, 18] {
        assert!(
            out.iter().any(|c| matches!(c, AudioCommand::Play { voice, .. }
                if voice.entity == e && voice.slot >= slot::ENGINE_LAYER && voice.slot < slot::ENGINE_LIMITER)),
            "car {e} keeps its layers"
        );
    }
    // A newcomer slightly closer than the 6th does not evict it (20 % hysteresis).
    cars[7].2.pos = [29.0, 0.0, 0.5]; // entity 16: 35 m → 29 m (6th is entity 15 at 30 m)
    let out = crowd_frame(&mut dir, 104, &cars, EntityId(1));
    apply(&out, &mut voiced);
    assert!(
        voiced.contains_key(&15) && !voiced.contains_key(&16),
        "sticky"
    );
    // Back to layers: every modelled voice stops.
    dir.set_engine_model(EngineModelMode::Layers);
    let out = crowd_frame(&mut dir, 108, &cars, EntityId(1));
    apply(&out, &mut voiced);
    assert!(voiced.is_empty(), "layers mode: no modelled voices left");
}

/// Same inputs → same commands (the physical voice included).
#[test]
fn physical_mode_is_deterministic() {
    let c = car("shiro_turbo");
    let a = run(&c, "shift", EngineTune::default());
    let b = run(&c, "shift", EngineTune::default());
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(x.commands, y.commands);
    }
}
