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
        let layers = EngineLayers::resolve(
            &lib.packs[&def.engine_pack],
            &def.engine,
            e.idle_rpm,
            e.redline_rpm,
        );
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
        // Idle plays a low layer near its own pitch (recorded packs root each loop at its
        // measured firing frequency, not at the car's idle), the top of the range the highest
        // on-layer.
        let idle_layer = &layers.layers[layers.dominant_on_layer(e.idle_rpm)];
        let rate = e.idle_rpm / idle_layer.root_rpm;
        assert!(
            (0.5..=2.0).contains(&rate),
            "{id}: idle plays {} at {rate:.2}x",
            idle_layer.cue
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
        tier: 0,
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

/// Police pacing (`data/police/radio.toml`) keeps the channel busy for a line's estimated length
/// with the scanner's own speaking rate, so the scanner never has two lines to play at once
/// (owner playtest row 79).
#[test]
fn radio_pacing_matches_the_scanner_speaking_rate() {
    let radio = wp_police::PoliceConfig::builtin().radio;
    let sc = &lib().config.scanner;
    assert_eq!(
        radio.words_per_s, sc.words_per_s,
        "radio.toml words_per_s vs audio_config scanner.words_per_s"
    );
    assert_eq!(
        radio.line_pad_s, sc.line_pad_s,
        "radio.toml line_pad_s vs audio_config scanner.line_pad_s"
    );
    let text = "Unit 12, I've got eyes on him again, northbound on Harbor Boulevard.";
    assert!((radio.line_s(text) - wp_audio::scanner::line_duration_s(sc, text)).abs() < 1e-5);
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
fn tier_one_police_lines_are_heard_at_once_and_tiers_set_the_priority() {
    // Row 144: the caller passes each line's police tier; tier 1 skips the scanner delay (the
    // owner: heard together with the map marker), the rest keep it.
    use wp_police::{PoliceEvent, PoliceEventExt};
    let (mut dir, s) = one_car_dir();
    dir.set_scanner_tier(ScannerTier::Basic); // 10 s
    let warn = "Dispatch, racer #1: roadblock 400 metres ahead, northbound on Lantern Row.";
    let call = "Dispatch: caller reports reckless driver, downtown.";
    dir.set_police_radio_tiers(vec![(None, warn.into(), 1), (None, call.into(), 4)]);
    let ev = [
        PoliceEvent::Radio {
            unit: None,
            text: call.into(),
        }
        .to_sim_event(),
        PoliceEvent::Radio {
            unit: None,
            text: warn.into(),
        }
        .to_sim_event(),
    ];
    let mut said = Vec::new();
    for c in step(&mut dir, &s, 0, &ev) {
        if let AudioCommand::RadioSay {
            text,
            priority,
            tier,
            ..
        } = c
        {
            said.push((0u64, text, priority, tier));
        }
    }
    for f in 1..=700u64 {
        for c in step(&mut dir, &s, f * 4, &[]) {
            if let AudioCommand::RadioSay {
                text,
                priority,
                tier,
                ..
            } = c
            {
                said.push((f * 4, text, priority, tier));
            }
        }
    }
    assert_eq!(said.len(), 2, "{said:?}");
    assert_eq!(said[0].0, 0, "tier 1 in the same frame: {said:?}");
    assert_eq!(said[0].1, warn);
    assert_eq!((said[0].2, said[0].3), (RadioPriority::Urgent, 1));
    assert_eq!(said[1].0, 10 * 240, "tier 4 after the Basic delay");
    assert_eq!((said[1].2, said[1].3), (RadioPriority::Routine, 4));
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

/// Grass: plain rolling plays only the `surface/grass` loop; a slide whose slip flickers
/// around the limit (2 frames sliding, 3 gripping) starts the soft grass scrub once and holds
/// it (no stop/restart churn), never the gravel scrub; it is released once after the slide.
#[test]
fn grass_drive_scrubs_softly_once_without_churn() {
    use wp_contracts::{SurfaceType, WheelState};
    let (mut dir, mut s) = one_car_dir();
    let wheel = |slip_angle| WheelState {
        slip_angle,
        fz_n: 4000.0,
        surface: SurfaceType::Grass,
        contact: true,
        omega: 45.0,
        ..Default::default()
    };
    let grass_scrub = VoiceId::new(1, slot::SCRUB_GRASS);
    let mut all = Vec::new();
    let mut tick = 0;
    let mut run = |s: &mut VehicleState, frames: u64, slip: &dyn Fn(u64) -> f32| {
        let mut cmds = Vec::new();
        for f in 0..frames {
            s.wheel_states = [wheel(slip(f)); 4];
            cmds.extend(step(&mut dir, s, tick, &[]));
            tick += 4;
        }
        cmds
    };
    let rolling = run(&mut s, 60, &|_| 0.03);
    assert_eq!(plays(&rolling, "surface/grass"), 1);
    assert_eq!(
        plays(&rolling, "tire/scrub_grass"),
        0,
        "rolling is not sliding"
    );
    assert_eq!(plays(&rolling, "tire/scrub_loose"), 0);
    let sliding = run(&mut s, 180, &|f| if f % 5 < 2 { 0.3 } else { 0.05 });
    assert_eq!(plays(&sliding, "tire/scrub_grass"), 1);
    assert_eq!(
        plays(&sliding, "tire/scrub_loose"),
        0,
        "no gravel crackle on grass"
    );
    let stops = |c: &[AudioCommand]| {
        c.iter()
            .filter(|c| matches!(c, AudioCommand::Stop { voice, .. } if *voice == grass_scrub))
            .count()
    };
    assert_eq!(stops(&sliding), 0, "held through the flicker");
    let grip = run(&mut s, 60, &|_| 0.03);
    assert_eq!(stops(&grip), 1);
    assert_eq!(plays(&grip, "tire/scrub_grass"), 0);
    all.extend(rolling);
    all.extend(sliding);
    all.extend(grip);
    // Only the rolling loop and the grass scrub ever play for the tires (no gravel roost).
    for c in &all {
        if let AudioCommand::Play { cue, .. } = c {
            assert!(
                !cue.starts_with("surface/") || cue == "surface/grass",
                "rolling loop swapped to {cue}"
            );
            assert!(cue != "tire/spin_loose", "gravel roost on grass");
        }
    }
}

/// The grass cues of every world pack stay soft: no stone crackle, darker than the gravel
/// scrub, and the rolling loop darker than asphalt.
#[test]
fn grass_cues_are_soft() {
    use wp_audio::data::{cues, CueSource, PackKind, SamplePack};
    let lib = lib();
    let param = |pack: &SamplePack, cue: &str, k: &str| match &pack.cues[cue].source {
        CueSource::Synth { params, .. } => params.get(k).copied(),
        CueSource::Samples { .. } => None,
    };
    let mut checked = 0;
    for p in lib.packs.values().filter(|p| p.kind == PackKind::World) {
        if !matches!(p.cues[cues::SCRUB_GRASS].source, CueSource::Synth { .. }) {
            continue; // recorded packs are judged by ear
        }
        let grass = cues::SURFACES[4];
        for cue in [cues::SCRUB_GRASS, grass] {
            assert!(
                param(p, cue, "crackle").unwrap_or(0.0) == 0.0,
                "{}: {cue} crackles",
                p.id
            );
        }
        let lp = |cue| param(p, cue, "lowpass_hz").unwrap_or(2000.0);
        assert!(lp(cues::SCRUB_GRASS) < 0.5 * lp(cues::SCRUB), "{}", p.id);
        assert!(lp(grass) < lp(cues::SURFACES[0]), "{}", p.id);
        assert!(
            p.cues[cues::SCRUB_GRASS].gain < p.cues[cues::SCRUB].gain,
            "{}",
            p.id
        );
        checked += 1;
    }
    assert!(checked > 0);
}

/// Asphalt drive through the director: a straight cruise (tiny real slip) plays the asphalt
/// rolling loop alone, getting louder and brighter (pitch → noise low-pass) with speed; ABS
/// braking whose slip cycles around the peak plays the squeal once and holds it (it used to
/// stop and restart it every 50-100 ms).
#[test]
fn asphalt_drive_rolls_quietly_and_squeals_once_under_abs() {
    use wp_contracts::{SurfaceType, WheelState};
    let (mut dir, mut s) = one_car_dir();
    let wheel = |v: f32, slip_ratio: f32, slip_angle: f32| WheelState {
        omega: v / 0.33,
        slip_ratio,
        slip_angle,
        fz_n: 4000.0,
        surface: SurfaceType::Asphalt,
        contact: true,
        ..Default::default()
    };
    let surf = VoiceId::new(1, slot::SURFACE);
    let squeal = VoiceId::new(1, slot::SQUEAL);
    let mut tick = 0;
    let mut last_surface: Option<VoiceParams> = None;
    let mut run = |s: &mut VehicleState, frames: u64, kmh: f32, sr: &dyn Fn(u64) -> f32| {
        let v = kmh / 3.6;
        let mut cmds = Vec::new();
        for f in 0..frames {
            s.lin_vel = [0.0, v, 0.0];
            s.speed_mps = v;
            s.wheel_states = [wheel(v, sr(f), 0.003); 4];
            let c = step(&mut dir, s, tick, &[]);
            for c in &c {
                match c {
                    AudioCommand::Play { voice, params, .. }
                    | AudioCommand::Set { voice, params }
                        if *voice == surf =>
                    {
                        last_surface = Some(*params);
                    }
                    _ => {}
                }
            }
            cmds.extend(c);
            tick += 4;
        }
        (cmds, last_surface)
    };
    let (c50, p50) = run(&mut s, 120, 50.0, &|_| 0.002);
    let (c100, p100) = run(&mut s, 120, 100.0, &|_| 0.002);
    for c in [&c50, &c100] {
        for cue in ["tire/squeal", "tire/spin", "tire/scrub_loose"] {
            assert_eq!(plays(c, cue), 0, "{cue} while cruising");
        }
    }
    assert_eq!(plays(&c50, "surface/asphalt"), 1);
    assert_eq!(plays(&c100, "surface/asphalt"), 0, "held, not restarted");
    let (p50, p100) = (p50.unwrap(), p100.unwrap());
    assert!(p100.gain > 1.3 * p50.gain, "{} {}", p50.gain, p100.gain);
    assert!(p100.pitch > p50.pitch, "brighter with speed");
    // ABS stop: slip ratio 3 frames past the peak, 3 frames released.
    let (abs, _) = run(&mut s, 90, 60.0, &|f| if f % 6 < 3 { -0.2 } else { -0.03 });
    assert_eq!(plays(&abs, "tire/squeal"), 1);
    let stops = abs
        .iter()
        .filter(|c| matches!(c, AudioCommand::Stop { voice, .. } if *voice == squeal))
        .count();
    assert_eq!(stops, 0, "squeal held through the ABS cycling");
}

/// Modelled share of a noise cue's power above `f_hz` at playback `pitch`: white noise through
/// the `noise` synth's 2nd-order high-pass and `lp_stages` 2nd-order low-passes (Q 0.707,
/// cut-off `lowpass_hz × pitch^track`), like `synth.ts`. The rumble and peak terms (low
/// frequencies) are left out, so this over-estimates the bright share.
fn bright_share(params: &BTreeMap<String, f32>, pitch: f32, f_hz: f32) -> f32 {
    let hp = params.get("highpass_hz").copied().unwrap_or(60.0);
    let track = params.get("track").copied().unwrap_or(0.0);
    let lp = params.get("lowpass_hz").copied().unwrap_or(2000.0) * pitch.powf(track);
    let stages = params.get("lp_stages").copied().unwrap_or(1.0).max(1.0) as i32;
    let (mut total, mut above) = (0.0f64, 0.0f64);
    let mut f = 20.0f32;
    while f < 20000.0 {
        let h = (f / hp).powi(4);
        let p = h / (1.0 + h) / (1.0 + (f / lp).powi(4)).powi(stages);
        total += p as f64;
        if f >= f_hz {
            above += p as f64;
        }
        f += 5.0;
    }
    (above / total) as f32
}

/// The paved rolling loops of every world pack are tyre roar, not hiss: asphalt is a
/// two-stage low-pass that follows speed with almost nothing above 4 kHz even at 140 km/h;
/// wet asphalt keeps some spray brightness but is no longer an 0.8-4.5 kHz hiss band (or
/// louder than dry); concrete is a little brighter than asphalt and still not hissy.
#[test]
fn asphalt_cues_roar_instead_of_hissing() {
    use wp_audio::data::{cues, CueSource, PackKind};
    let lib = lib();
    let mix = &lib.config.tires;
    // Director pitch for a synth rolling loop at `kmh` (no recorded root speed).
    let pitch = |kmh: f32| 0.6 + 0.8 * (kmh / 3.6 / mix.rumble_speed_ref_mps).min(1.0);
    let mut checked = 0;
    for p in lib.packs.values().filter(|p| p.kind == PackKind::World) {
        let synth = |cue: &str| match &p.cues[cue].source {
            CueSource::Synth { params, .. } => Some(params.clone()),
            CueSource::Samples { .. } => None,
        };
        let (Some(dry), Some(wet), Some(conc)) = (
            synth(cues::SURFACES[0]),
            synth(cues::SURFACES[1]),
            synth(cues::SURFACES[2]),
        ) else {
            continue; // recorded loops are judged by ear
        };
        let id = &p.id;
        for (name, q) in [("asphalt", &dry), ("concrete", &conc)] {
            assert!(
                q.get("track").is_some_and(|t| *t >= 1.0),
                "{id} {name}: track"
            );
            assert!(q.get("lp_stages").is_some_and(|n| *n >= 2.0), "{id} {name}");
            assert!(
                q.get("lowpass_hz").is_some_and(|f| *f <= 1000.0),
                "{id} {name}"
            );
            assert!(q.get("crackle").is_none(), "{id} {name}: crackle");
        }
        let s = |q: &BTreeMap<String, f32>, kmh: f32, f: f32| bright_share(q, pitch(kmh), f);
        // Asphalt: mostly below 1 kHz at cruise, nothing above 4 kHz even flat out.
        assert!(
            s(&dry, 50.0, 1000.0) < 0.10,
            "{id}: {}",
            s(&dry, 50.0, 1000.0)
        );
        assert!(
            s(&dry, 100.0, 1000.0) < 0.25,
            "{id}: {}",
            s(&dry, 100.0, 1000.0)
        );
        assert!(
            s(&dry, 140.0, 4000.0) < 0.005,
            "{id}: {}",
            s(&dry, 140.0, 4000.0)
        );
        // Brighter with speed.
        assert!(s(&dry, 100.0, 1000.0) > 1.5 * s(&dry, 50.0, 1000.0), "{id}");
        // Concrete: a little brighter than asphalt, still no hiss.
        assert!(s(&conc, 100.0, 1000.0) > s(&dry, 100.0, 1000.0), "{id}");
        assert!(s(&conc, 100.0, 1000.0) < 0.5, "{id}");
        assert!(s(&conc, 140.0, 4000.0) < 0.01, "{id}");
        // Wet: spray brightness, tamed.
        assert!(
            s(&wet, 100.0, 1000.0) > s(&dry, 100.0, 1000.0),
            "{id}: wet is distinct"
        );
        assert!(
            s(&wet, 100.0, 4000.0) < 0.02,
            "{id}: {}",
            s(&wet, 100.0, 4000.0)
        );
        assert!(
            wet.get("highpass_hz").copied().unwrap_or(60.0) <= 200.0,
            "{id}: wet keeps its roar"
        );
        assert!(
            p.cues[cues::SURFACES[1]].gain <= p.cues[cues::SURFACES[0]].gain,
            "{id}: wet no louder than dry"
        );
        checked += 1;
    }
    assert!(checked > 0);
}
