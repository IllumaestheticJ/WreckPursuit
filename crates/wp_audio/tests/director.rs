//! Integration tests: shipped data validates, the command stream is deterministic on real
//! physics output, voice culling respects the limits, engine layer selection is monotone in
//! rpm for every car, and the crate passes the determinism lint.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use wp_audio::data::ThrottleSide;
use wp_audio::engine::EngineLayers;
use wp_audio::types::slot;
use wp_audio::*;
use wp_contracts::{
    load_car, load_mode_table, EntityId, LoadedCar, SimEvent, TimeOfDay, VehicleInput, VehicleState,
};
use wp_math::StateHasher;
use wp_sim::{Sim, SimConfig};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn car(id: &str) -> LoadedCar {
    load_car(repo().join(format!("data/cars/{id}.json"))).expect("car loads")
}

fn lib() -> AudioLibrary {
    AudioLibrary::embedded().expect("embedded audio data parses")
}

#[test]
fn shipped_data_is_valid_and_complete() {
    let lib = lib();
    let errs = lib.validate();
    assert!(errs.is_empty(), "audio data problems:\n{}", errs.join("\n"));
    // Disk and embedded copies agree.
    let disk = AudioLibrary::load_dir(repo().join("data/audio")).unwrap();
    assert_eq!(disk, lib);
    // Every car in data/cars has an audio def (no silent fallbacks in shipped data).
    let mut missing = Vec::new();
    for e in std::fs::read_dir(repo().join("data/cars")).unwrap() {
        let p = e.unwrap().path();
        if p.extension().and_then(|x| x.to_str()) == Some("json") {
            let id = p.file_stem().unwrap().to_string_lossy().to_string();
            if !lib.cars.contains_key(&id) {
                missing.push(id);
            }
        }
    }
    assert!(
        missing.is_empty(),
        "cars without data/audio/cars/<id>.json: {missing:?}"
    );
    // And no audio def for a car that does not exist.
    for id in lib.cars.keys() {
        assert!(
            repo().join(format!("data/cars/{id}.json")).exists(),
            "orphan audio def {id}"
        );
    }
}

#[test]
fn engine_layer_selection_is_monotone_in_rpm_for_every_car() {
    let lib = lib();
    for (id, def) in &lib.cars {
        let c = car(id);
        let e = &c.def.engine;
        let layers = EngineLayers::resolve(&lib.packs[&def.engine_pack], e.idle_rpm, e.redline_rpm);
        let mut prev_on = f32::MIN;
        let mut prev_off = f32::MIN;
        let mut prev_dom_root = 0.0;
        let mut rpm = 0.0;
        while rpm <= e.rev_limit_rpm + 500.0 {
            let on = layers.band_position(rpm, ThrottleSide::On);
            let off = layers.band_position(rpm, ThrottleSide::Off);
            assert!(on >= prev_on, "{id}: on band went down at {rpm} rpm");
            assert!(off >= prev_off, "{id}: off band went down at {rpm} rpm");
            let dom = layers.layers[layers.dominant_on_layer(rpm)].root_rpm;
            assert!(
                dom >= prev_dom_root,
                "{id}: dominant layer went down at {rpm} rpm"
            );
            let energy: f32 = layers.weights(rpm, 0.6).iter().map(|w| w * w).sum();
            assert!(
                (energy - 1.0).abs() < 1e-3,
                "{id}: weights not energy preserving at {rpm}"
            );
            prev_on = on;
            prev_off = off;
            prev_dom_root = dom;
            rpm += 25.0;
        }
        // Idle sits on the idle layer, the top of the range on the highest on-layer.
        assert_eq!(
            layers.layers[layers.dominant_on_layer(e.idle_rpm)].root_rpm,
            e.idle_rpm
        );
        let top = layers
            .layers
            .iter()
            .rfind(|l| l.throttle != ThrottleSide::Off)
            .unwrap();
        assert_eq!(
            layers.layers[layers.dominant_on_layer(e.rev_limit_rpm)].cue,
            top.cue
        );
    }
}

/// Head-on crash between two cars plus a police car with its siren on, on real physics.
/// Returns the hash of the whole command stream and some counters.
fn run_crash(seed: u64) -> (u64, usize, BTreeMap<String, usize>) {
    let mode = load_mode_table(repo().join("data/modes/arcade.toml")).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed,
        mode,
        time_of_day: TimeOfDay::from_hours(22.0),
        time_ratio: 1.0,
    });
    let cars = [
        car("debug_sedan"),
        car("hakone_gt"),
        car("sentinel_cruiser"),
    ];
    let a = sim.spawn_vehicle(&cars[0], [0.0, -25.0, 0.0], 0.0).unwrap();
    let b = sim
        .spawn_vehicle(&cars[1], [0.3, 25.0, 0.0], std::f32::consts::PI)
        .unwrap();
    let p = sim.spawn_vehicle(&cars[2], [8.0, -60.0, 0.0], 0.0).unwrap();
    sim.place_vehicle(a, [0.0, -25.0, 0.0], 0.0, 14.0).unwrap();
    sim.place_vehicle(b, [0.3, 25.0, 0.0], std::f32::consts::PI, 14.0)
        .unwrap();
    let car_of: BTreeMap<EntityId, &str> = [
        (a, "debug_sedan"),
        (b, "hakone_gt"),
        (p, "sentinel_cruiser"),
    ]
    .into_iter()
    .collect();
    let throttle: BTreeMap<EntityId, f32> = [(a, 0.6), (b, 0.8), (p, 1.0)].into_iter().collect();
    for (id, t) in &throttle {
        sim.set_input(
            *id,
            VehicleInput {
                throttle: *t,
                siren: *id == p,
                horn: *id == a,
                ..Default::default()
            },
        )
        .unwrap();
    }

    let mut dir = AudioDirector::new(lib(), seed);
    for c in &cars {
        dir.register_car(c);
    }
    dir.set_scanner_tier(ScannerTier::Elite);
    dir.push_radio_line(RadioLine {
        unit: p.0,
        text: "unit twelve in pursuit of a grey coupe".into(),
        priority: RadioPriority::Pursuit,
        emitted_tick: 0,
    });
    let listener = Listener {
        pos: [12.0, 0.0, 2.0],
        ..Default::default()
    };
    let mut h = StateHasher::new();
    let mut n = 0;
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    // 6 s at 60 Hz frames (4 ticks each).
    for _ in 0..360 {
        for _ in 0..4 {
            sim.step_fixed();
        }
        let events: Vec<SimEvent> = sim.drain_events();
        let states: Vec<(EntityId, VehicleState)> = sim.vehicle_states();
        let inputs: Vec<VehicleAudioInput> = states
            .iter()
            .map(|(id, s)| VehicleAudioInput {
                entity: *id,
                car_id: car_of[id],
                state: s,
                throttle: throttle[id],
                throttle_applied: None,
                horn: *id == a,
                siren: *id == p,
                road_rumble: 0.0,
                kind: if *id == p {
                    DriverKind::Police
                } else {
                    DriverKind::Player
                },
            })
            .collect();
        let cmds = dir.update(&AudioFrame {
            tick: sim.tick(),
            listener,
            focus: Some(a),
            vehicles: &inputs,
            events: &events,
        });
        for c in &cmds {
            h.write_bytes(serde_json::to_string(c).unwrap().as_bytes());
            let k = match c {
                AudioCommand::Play { cue, .. } => format!("play {cue}"),
                AudioCommand::Set { .. } => "set".into(),
                AudioCommand::Stop { .. } => "stop".into(),
                AudioCommand::Duck { .. } => "duck".into(),
                AudioCommand::RadioSay { .. } => "radio".into(),
                AudioCommand::RadioEnd { .. } => "radio_end".into(),
            };
            *kinds.entry(k).or_default() += 1;
        }
        n += cmds.len();
    }
    (h.finish(), n, kinds)
}

#[test]
fn command_stream_is_deterministic_on_real_physics() {
    let (h1, n1, kinds) = run_crash(7);
    let (h2, n2, _) = run_crash(7);
    assert_eq!(n1, n2);
    assert_eq!(h1, h2, "same inputs must give the same command stream");
    // The stream is not trivially empty: engines, siren, horn, the crash and the radio all play.
    let has = |prefix: &str| kinds.keys().any(|k| k.starts_with(prefix));
    assert!(has("play engine/"), "{kinds:?}");
    assert!(has("play siren/"), "{kinds:?}");
    assert!(has("play horn/"), "{kinds:?}");
    assert!(
        has("play impact/"),
        "no impact sounds from the head-on: {kinds:?}"
    );
    assert!(kinds.contains_key("radio"), "{kinds:?}");
    assert!(kinds["set"] > 1000);
}

#[test]
fn voice_culling_respects_limits() {
    let lib = lib();
    let cfg = lib.config.clone();
    let mut dir = AudioDirector::new(lib, 3);
    let sedan = car("debug_sedan");
    let cruiser = car("sentinel_cruiser");
    dir.register_car(&sedan);
    dir.register_car(&cruiser);
    // 40 cars packed around the listener, all screaming: revving, sliding, honking, sirens.
    let n = 40u32;
    let mut states = Vec::new();
    for i in 0..n {
        let mut s = VehicleState {
            pos: [
                (i % 8) as f32 * 4.0 - 14.0,
                (i / 8) as f32 * 6.0 - 12.0,
                0.5,
            ],
            rot: [0.0, 0.0, 0.0, 1.0],
            lin_vel: [0.0, 15.0, 0.0],
            engine_rpm: 2000.0 + 120.0 * i as f32,
            speed_mps: 15.0,
            ..Default::default()
        };
        for w in &mut s.wheel_states {
            w.contact = true;
            w.slip_angle = 0.35;
            w.fz_n = 4000.0;
            w.omega = 40.0;
        }
        states.push(s);
    }
    for i in 0..n {
        if i % 3 == 0 {
            dir.set_siren_override(
                EntityId(i + 1),
                Some(SirenOverride {
                    on: true,
                    mode: SirenMode::Fixed(SirenPattern::Yelp),
                }),
            );
        }
    }
    let focus = EntityId(17);
    let mut playing: BTreeSet<VoiceId> = BTreeSet::new();
    for frame in 0..120u64 {
        // Wobble rpm so layers cross over and voices get culled / re-admitted.
        for (i, s) in states.iter_mut().enumerate() {
            s.engine_rpm = 1500.0 + ((frame * 37 + i as u64 * 91) % 5000) as f32;
        }
        let inputs: Vec<VehicleAudioInput> = states
            .iter()
            .enumerate()
            .map(|(i, s)| VehicleAudioInput {
                entity: EntityId(i as u32 + 1),
                car_id: if i % 3 == 0 {
                    "sentinel_cruiser"
                } else {
                    "debug_sedan"
                },
                state: s,
                throttle: if frame % 20 < 10 { 1.0 } else { 0.0 },
                throttle_applied: None,
                horn: true,
                siren: false,
                road_rumble: 0.5,
                kind: DriverKind::Player,
            })
            .collect();
        // Crash events every frame too.
        let events: Vec<SimEvent> = (0..10)
            .map(|k| SimEvent::Contact {
                a: EntityId(k + 1),
                b: EntityId(k + 2),
                point: [0.0, 0.0, 0.5],
                normal: [1.0, 0.0, 0.0],
                impulse_ns: 4000.0,
                rel_speed: 3.0 + frame as f32,
            })
            .collect();
        let cmds = dir.update(&AudioFrame {
            tick: frame * 4,
            listener: Listener::default(),
            focus: Some(focus),
            vehicles: &inputs,
            events: &events,
        });
        // Replay the stream like a backend would and check it is well formed.
        for c in &cmds {
            match c {
                AudioCommand::Play {
                    voice,
                    looped: true,
                    ..
                } => {
                    assert!(playing.insert(*voice), "double play {voice:?}");
                }
                AudioCommand::Play {
                    voice,
                    looped: false,
                    ..
                } => {
                    assert!(slot::is_one_shot(voice.slot));
                }
                AudioCommand::Stop { voice, .. } => {
                    assert!(
                        playing.remove(voice),
                        "stop of a voice not playing {voice:?}"
                    );
                }
                AudioCommand::Set { voice, .. } => {
                    assert!(
                        playing.contains(voice),
                        "set of a voice not playing {voice:?}"
                    );
                }
                _ => {}
            }
        }
        let st = dir.stats();
        assert_eq!(st.looped, playing.len());
        assert!(
            st.looped + st.one_shots <= cfg.max_voices,
            "frame {frame}: {st:?}"
        );
        for (bus, count) in &st.per_bus {
            if let Some(max) = cfg.bus_max_voices.get(bus) {
                assert!(
                    count <= max,
                    "frame {frame}: bus {bus:?} has {count} > {max}"
                );
            }
        }
        assert!(
            st.culled > 0,
            "the stress test must actually overflow the limits"
        );
        // The focused car's engine is never culled.
        assert!(
            playing.iter().any(|v| v.entity == focus.0
                && (slot::ENGINE_LAYER..slot::ENGINE_LIMITER).contains(&v.slot)),
            "frame {frame}: focus engine culled"
        );
    }
}

#[test]
fn time_going_backwards_resets_and_stops_everything() {
    let mut dir = AudioDirector::new(lib(), 1);
    let sedan = car("debug_sedan");
    dir.register_car(&sedan);
    let s = VehicleState {
        rot: [0.0, 0.0, 0.0, 1.0],
        engine_rpm: 3000.0,
        ..Default::default()
    };
    let inputs = [VehicleAudioInput {
        entity: EntityId(1),
        car_id: "debug_sedan",
        state: &s,
        throttle: 0.5,
        throttle_applied: None,
        horn: false,
        siren: false,
        road_rumble: 0.0,
        kind: DriverKind::Player,
    }];
    let frame = |tick| AudioFrame {
        tick,
        listener: Listener::default(),
        focus: None,
        vehicles: &inputs,
        events: &[],
    };
    let first = dir.update(&frame(100));
    let plays = first
        .iter()
        .filter(|c| matches!(c, AudioCommand::Play { .. }))
        .count();
    assert!(plays > 0);
    let back = dir.update(&frame(10));
    let stops = back
        .iter()
        .filter(|c| matches!(c, AudioCommand::Stop { .. }))
        .count();
    assert_eq!(stops, plays, "every looped voice stopped on reset");
}

#[test]
fn passes_determinism_lint() {
    let findings = lint_determinism::lint_repo(&repo(), &["wp_audio"], &[]);
    let errors: Vec<String> = findings
        .iter()
        .filter(|f| f.severity == lint_determinism::Severity::Error)
        .map(|f| format!("{}:{} {} {}", f.path, f.line, f.rule, f.text))
        .collect();
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn scanner_delays_match_police_tools_toml() {
    let text = std::fs::read_to_string(repo().join("data/police/tools.toml")).unwrap();
    let v: toml::Value = toml::from_str(&text).unwrap();
    let police: Vec<f32> = v["gadgets"]["scanner_delay_s"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_float().unwrap() as f32)
        .collect();
    assert_eq!(police, lib().config.scanner.tier_delay_s.to_vec(), "audio_config scanner.tier_delay_s must equal data/police/tools.toml gadgets.scanner_delay_s");
}

fn one_car_dir() -> (AudioDirector, VehicleState) {
    let mut dir = AudioDirector::new(lib(), 5);
    dir.register_car(&car("sentinel_cruiser"));
    let s = VehicleState {
        rot: [0.0, 0.0, 0.0, 1.0],
        lin_vel: [0.0, 15.0, 0.0],
        speed_mps: 15.0,
        engine_rpm: 2500.0,
        ..Default::default()
    };
    (dir, s)
}

fn step(
    dir: &mut AudioDirector,
    s: &VehicleState,
    tick: u64,
    events: &[SimEvent],
) -> Vec<AudioCommand> {
    let inputs = [VehicleAudioInput {
        entity: EntityId(1),
        car_id: "sentinel_cruiser",
        state: s,
        throttle: 0.3,
        throttle_applied: None,
        horn: false,
        siren: false,
        road_rumble: 0.5,
        kind: DriverKind::Player,
    }];
    dir.update(&AudioFrame {
        tick,
        listener: Listener::default(),
        focus: Some(EntityId(1)),
        vehicles: &inputs,
        events,
    })
}

fn plays(cmds: &[AudioCommand], cue_name: &str) -> usize {
    cmds.iter()
        .filter(|c| matches!(c, AudioCommand::Play { cue, .. } if cue == cue_name))
        .count()
}

#[test]
fn police_radio_events_reach_the_scanner_with_tier_delay() {
    use wp_police::{PoliceEvent, PoliceEventExt, UnitId};
    let (mut dir, s) = one_car_dir();
    dir.set_scanner_tier(ScannerTier::Pro); // 6 s
    let ev = [
        PoliceEvent::PursuitStarted {
            racer: EntityId(1),
            unit: Some(UnitId(4)),
        }
        .to_sim_event(),
        PoliceEvent::Radio {
            unit: Some(UnitId(4)),
            text: "four in pursuit northbound".into(),
        }
        .to_sim_event(),
    ];
    step(&mut dir, &s, 0, &ev);
    let mut said = Vec::new();
    for f in 1..=400u64 {
        for c in step(&mut dir, &s, f * 4, &[]) {
            if let AudioCommand::RadioSay {
                text,
                priority,
                unit,
                ..
            } = c
            {
                said.push((f * 4, text, priority, unit));
            }
        }
    }
    assert_eq!(said.len(), 1, "{said:?}");
    let (t, text, priority, unit) = &said[0];
    assert_eq!(*t, 6 * 240, "released exactly after the Pro delay");
    assert_eq!(text, "four in pursuit northbound");
    assert_eq!(
        *priority,
        RadioPriority::Pursuit,
        "unit line during a pursuit"
    );
    assert_eq!(*unit, 4);
}

#[test]
fn glass_event_shatters_once_and_disables_contact_inference() {
    use wp_contracts::BodyRegion;
    let (mut dir, s) = one_car_dir();
    step(&mut dir, &s, 0, &[]);
    let ev = SimEvent::GlassShattered {
        vehicle: EntityId(1),
        region: BodyRegion::Windshield,
    };
    let out = step(&mut dir, &s, 4, std::slice::from_ref(&ev));
    assert_eq!(plays(&out, "impact/glass_shatter"), 1);
    let out = step(&mut dir, &s, 8, std::slice::from_ref(&ev));
    assert_eq!(
        plays(&out, "impact/glass_shatter"),
        0,
        "same region shatters once"
    );
    // A hard hit on the (inferred) rear glass no longer shatters by itself.
    let lattice_glass = SimEvent::Contact {
        a: EntityId(1),
        b: wp_contracts::WORLD_ENTITY,
        point: [0.0, -2.0, 1.1],
        normal: [0.0, -1.0, 0.0],
        impulse_ns: 8000.0,
        rel_speed: 15.0,
    };
    let out = step(&mut dir, &s, 200, &[lattice_glass]);
    assert_eq!(plays(&out, "impact/glass_shatter"), 0);
}

#[test]
fn spike_hit_bursts_and_panel_damage_rattles() {
    use wp_contracts::BodyRegion;
    use wp_police::{PoliceEvent, PoliceEventExt};
    let (mut dir, s) = one_car_dir();
    step(&mut dir, &s, 0, &[]);
    let spikes = PoliceEvent::SpikeHit {
        vehicle: EntityId(1),
        wheels: [true, true, false, false],
    }
    .to_sim_event();
    assert_eq!(plays(&step(&mut dir, &s, 4, &[spikes]), "tire/burst"), 2);
    let dmg = SimEvent::RegionDamaged {
        vehicle: EntityId(1),
        region: BodyRegion::Hood,
        severity: 0.7,
    };
    let out = step(&mut dir, &s, 8, &[dmg]);
    // The rattle loop starts on the frame after the damage is known.
    let out2 = step(&mut dir, &s, 12, &[]);
    assert_eq!(plays(&out, "body/rattle") + plays(&out2, "body/rattle"), 1);
}

fn frame_for(
    dir: &mut AudioDirector,
    s: &VehicleState,
    car_id: &str,
    kind: DriverKind,
    siren: bool,
    tick: u64,
) -> Vec<AudioCommand> {
    let inputs = [VehicleAudioInput {
        entity: EntityId(1),
        car_id,
        state: s,
        throttle: 0.5,
        throttle_applied: None,
        horn: false,
        siren,
        road_rumble: 0.0,
        kind,
    }];
    dir.update(&AudioFrame {
        tick,
        listener: Listener {
            pos: [10.0, 0.0, 1.0],
            ..Default::default()
        },
        focus: None,
        vehicles: &inputs,
        events: &[],
    })
}

#[test]
fn police_kind_gets_a_siren_on_any_car() {
    // debug_sedan has no siren in its audio def; driven by police it must still siren.
    let s = VehicleState {
        rot: [0.0, 0.0, 0.0, 1.0],
        engine_rpm: 2000.0,
        speed_mps: 25.0,
        ..Default::default()
    };
    let mut dir = AudioDirector::new(lib(), 1);
    dir.register_car(&car("debug_sedan"));
    let out = frame_for(&mut dir, &s, "debug_sedan", DriverKind::Player, true, 0);
    assert!(!out
        .iter()
        .any(|c| matches!(c, AudioCommand::Play { cue, .. } if cue.starts_with("siren/"))));
    let mut dir = AudioDirector::new(lib(), 1);
    dir.register_car(&car("debug_sedan"));
    let out = frame_for(&mut dir, &s, "debug_sedan", DriverKind::Police, true, 0);
    assert!(
        out.iter()
            .any(|c| matches!(c, AudioCommand::Play { cue, .. } if cue == "siren/wail")),
        "Auto at 25 m/s wails"
    );
}

#[test]
fn traffic_engines_are_quieter_and_low_priority() {
    let s = VehicleState {
        rot: [0.0, 0.0, 0.0, 1.0],
        engine_rpm: 2500.0,
        ..Default::default()
    };
    let engine_gain_and_prio = |kind| {
        let mut dir = AudioDirector::new(lib(), 1);
        dir.register_car(&car("debug_sedan"));
        frame_for(&mut dir, &s, "debug_sedan", kind, false, 0)
            .into_iter()
            .filter_map(|c| match c {
                AudioCommand::Play {
                    cue,
                    params,
                    priority,
                    ..
                } if cue.starts_with("engine/") => Some((params.gain, priority)),
                _ => None,
            })
            .fold((0.0f32, Priority::Critical), |(g, p), (g2, p2)| {
                (g + g2, p.min(p2))
            })
    };
    let (player, pp) = engine_gain_and_prio(DriverKind::Player);
    let (traffic, tp) = engine_gain_and_prio(DriverKind::Traffic);
    let ratio = lib().config.engine.traffic_gain;
    assert!(
        (traffic / player - ratio).abs() < 1e-4,
        "{traffic} vs {player}"
    );
    assert_eq!(pp, Priority::Normal);
    assert_eq!(tp, Priority::Low);
}
