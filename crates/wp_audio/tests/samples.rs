//! Recorded sample assets: every file a pack references exists, stays inside
//! `data/audio/packs/`, is listed in `data/audio/LICENSES.md` under an allowed licence, and the
//! whole set fits the download budget. Also checks every recorded pack has a procedural fallback.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use wp_audio::data::CueSource;
use wp_audio::AudioLibrary;

/// Total size budget for all audio files under `data/audio/` (bytes).
const BUDGET_BYTES: u64 = 15 * 1024 * 1024;

/// Licences allowed for recorded files (first cell after the file in `LICENSES.md`).
const ALLOWED: [&str; 3] = ["CC0 1.0", "CC-BY 3.0", "CC-BY 4.0"];

fn audio_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/audio")
}

/// `packs/<pack>/<file>` with `..` resolved lexically; `None` if it escapes `data/audio/packs`.
fn resolve(pack: &str, file: &str) -> Option<String> {
    let mut parts: Vec<String> = vec!["packs".into(), pack.into()];
    for c in Path::new(file).components() {
        match c {
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            Component::ParentDir => {
                parts.pop();
                if parts.is_empty() {
                    return None;
                }
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    (parts.len() >= 3 && parts[0] == "packs").then(|| parts.join("/"))
}

/// Every `(pack, cue, relative path)` a pack references as a recorded sample.
fn referenced(lib: &AudioLibrary) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for p in lib.packs.values() {
        for (name, c) in &p.cues {
            if let CueSource::Samples { files } = &c.source {
                for f in files {
                    let rel = resolve(&p.id, f).unwrap_or_else(|| {
                        panic!("packs/{}: {name}: {f} escapes data/audio/packs", p.id)
                    });
                    out.push((p.id.clone(), name.clone(), rel));
                }
            }
        }
    }
    out
}

fn audio_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            audio_files(&p, out);
        } else if matches!(
            p.extension().and_then(|e| e.to_str()),
            Some("ogg" | "mp3" | "wav")
        ) {
            out.push(p);
        }
    }
}

#[test]
fn every_referenced_sample_exists_and_is_licensed() {
    let dir = audio_dir();
    let lib = AudioLibrary::load_dir(&dir).expect("load data/audio");
    let licenses = std::fs::read_to_string(dir.join("LICENSES.md")).expect("LICENSES.md");
    let refs = referenced(&lib);
    assert!(!refs.is_empty(), "no recorded samples referenced");
    for (pack, cue, rel) in &refs {
        let ext = Path::new(rel).extension().and_then(|e| e.to_str());
        assert!(
            matches!(ext, Some("ogg" | "mp3" | "wav")),
            "packs/{pack}: {cue}: {rel} must be .ogg, .mp3 or .wav (the published link serves no .flac)"
        );
        let path = dir.join(rel);
        assert!(
            path.is_file(),
            "packs/{pack}: {cue}: missing file {}",
            path.display()
        );
        let row = licenses
            .lines()
            .find(|l| l.starts_with(&format!("| {rel} |")))
            .unwrap_or_else(|| panic!("{rel} (packs/{pack} {cue}) is not listed in LICENSES.md"));
        let licence = row.split('|').nth(2).unwrap_or("").trim();
        assert!(
            licence.split(" + ").all(|l| ALLOWED.contains(&l)),
            "{rel}: licence {licence:?} not in {ALLOWED:?}"
        );
    }
}

#[test]
fn no_unreferenced_audio_and_total_size_within_budget() {
    let dir = audio_dir();
    let lib = AudioLibrary::load_dir(&dir).unwrap();
    let used: BTreeSet<String> = referenced(&lib).into_iter().map(|r| r.2).collect();
    let mut files = Vec::new();
    audio_files(&dir, &mut files);
    let mut total = 0;
    for f in &files {
        total += std::fs::metadata(f).unwrap().len();
        let rel = f
            .strip_prefix(&dir)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        assert!(
            used.contains(&rel),
            "{rel} is not referenced by any pack (remove it or use it)"
        );
    }
    assert!(
        total <= BUDGET_BYTES,
        "audio files total {:.2} MB > budget {:.2} MB",
        total as f64 / 1e6,
        BUDGET_BYTES as f64 / 1e6
    );
}

#[test]
fn recorded_packs_have_a_procedural_fallback_and_cars_use_them() {
    let lib = AudioLibrary::load_dir(audio_dir()).unwrap();
    assert!(lib.validate().is_empty(), "{:?}", lib.validate());
    for p in lib.packs.values() {
        let recorded = p
            .cues
            .values()
            .any(|c| matches!(c.source, CueSource::Samples { .. }));
        if !recorded {
            continue;
        }
        let fb = p
            .fallback
            .as_ref()
            .unwrap_or_else(|| panic!("packs/{}: recorded pack needs a `fallback`", p.id));
        let f = &lib.packs[fb];
        assert!(
            f.cues
                .values()
                .all(|c| matches!(c.source, CueSource::Synth { .. })),
            "packs/{}: fallback {fb} must be fully procedural",
            p.id
        );
    }
    for car in lib.cars.values() {
        for pack in [&car.engine_pack, &car.world_pack] {
            let recorded = pack.ends_with("_rec") || pack.contains("_rec_");
            // The one exception (owner 0.12.34, police "singing / laughing"): a police car whose
            // recorded engine twin would pitch a loop above MAX_RECORDED_PITCH uses the
            // procedural twin until a recording covers its rev range.
            let police_exception = pack == &car.engine_pack
                && car.siren.is_some()
                && recorded_twin_top_pitch(&lib, &car.id, pack)
                    .is_some_and(|p| p > MAX_RECORDED_PITCH);
            assert!(
                recorded || police_exception,
                "cars/{}: {pack} is not a recorded pack",
                car.id
            );
        }
        // Police sirens play recordings too (owner: "sirens seem unrealistic — use real
        // sounds"); the synth is only their fallback.
        if let Some(siren) = &car.siren {
            assert!(
                lib.packs[&siren.pack]
                    .cues
                    .values()
                    .any(|c| matches!(c.source, CueSource::Samples { .. })),
                "cars/{}: siren pack {} plays no recordings",
                car.id,
                siren.pack
            );
        }
    }
}

#[test]
fn gear_change_plays_the_recorded_shift_clunk_once() {
    use wp_audio::{AudioDirector, AudioFrame, DriverKind, Listener, VehicleAudioInput};
    use wp_contracts::{load_car, EntityId, VehicleState};
    let car = load_car(audio_dir().join("../cars/debug_sedan.json")).expect("car loads");
    let mut dir = AudioDirector::new(AudioLibrary::embedded().unwrap(), 3);
    dir.register_car(&car);
    let mut s = VehicleState {
        rot: [0.0, 0.0, 0.0, 1.0],
        engine_rpm: 3000.0,
        gear: 1,
        ..Default::default()
    };
    let mut shifts = Vec::new();
    for f in 0..6u64 {
        if f == 3 {
            s.gear = 2;
        }
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
        let cmds = dir.update(&AudioFrame {
            tick: f * 4,
            listener: Listener::default(),
            focus: Some(EntityId(1)),
            vehicles: &inputs,
            events: &[],
        });
        let n = cmds
            .iter()
            .filter(|c| matches!(c, wp_audio::AudioCommand::Play { cue, pack, .. } if cue == "drivetrain/shift" && pack == "world_rec"))
            .count();
        shifts.push(n);
    }
    assert_eq!(
        shifts,
        vec![0, 0, 0, 1, 0, 0],
        "one clunk on the frame the gear changes"
    );
}

/// Playtest "engine sounds are bad": recorded layers were rooted at the car's idle or at a
/// fraction of its redline, so the six and V8 loops played 15-47 semitones under the car's
/// firing frequency and two crossfading loops sounded at pitches up to ~2 octaves apart. Every
/// recorded layer is now anchored at its loop's measured firing frequency: through the real
/// director, each audible layer's pitch × its anchor frequency is the car's firing frequency, so
/// crossfading loops share one pitch.
#[test]
fn recorded_engine_layers_follow_the_cars_firing_frequency() {
    use wp_audio::data::RpmAnchor;
    use wp_audio::types::slot;
    use wp_audio::{
        AudioCommand, AudioDirector, AudioFrame, DriverKind, Listener, VehicleAudioInput,
    };
    use wp_contracts::{load_car, EntityId, VehicleState};
    let lib = AudioLibrary::embedded().unwrap();
    // Every recorded layer is anchored by its firing frequency (also enforced by validate()).
    for p in lib.packs.values() {
        for l in &p.engine_layers {
            let recorded = matches!(p.cues[&l.cue].source, CueSource::Samples { .. });
            assert!(
                !recorded || matches!(l.anchor, RpmAnchor::FiringHz(_)),
                "packs/{}: recorded layer {} anchored by {:?}",
                p.id,
                l.cue,
                l.anchor
            );
        }
    }
    let mut checked = 0;
    for (id, def) in &lib.cars {
        let pack = &lib.packs[&def.engine_pack];
        let car = load_car(audio_dir().join(format!("../cars/{id}.json"))).expect("car loads");
        let e = &car.def.engine;
        let mut dir = AudioDirector::new(lib.clone(), 5);
        dir.register_car(&car);
        let mut rpm = e.idle_rpm;
        let mut tick = 0u64;
        while rpm <= e.rev_limit_rpm {
            for throttle in [1.0f32, 0.0] {
                let s = VehicleState {
                    rot: [0.0, 0.0, 0.0, 1.0],
                    engine_rpm: rpm,
                    gear: 3,
                    ..Default::default()
                };
                let inputs = [VehicleAudioInput {
                    entity: EntityId(1),
                    car_id: id,
                    state: &s,
                    throttle,
                    throttle_applied: Some(throttle),
                    horn: false,
                    siren: false,
                    road_rumble: 0.0,
                    kind: DriverKind::Player,
                }];
                // A few frames so load settles; then read every layer's commanded pitch.
                let mut pitch = std::collections::BTreeMap::new();
                for _ in 0..20 {
                    tick += 4;
                    for c in dir.update(&AudioFrame {
                        tick,
                        listener: Listener::default(),
                        focus: Some(EntityId(1)),
                        vehicles: &inputs,
                        events: &[],
                    }) {
                        match c {
                            AudioCommand::Play { voice, params, .. }
                            | AudioCommand::Set { voice, params }
                                if (slot::ENGINE_LAYER..slot::ENGINE_LIMITER)
                                    .contains(&voice.slot) =>
                            {
                                pitch.insert(
                                    voice.slot - slot::ENGINE_LAYER,
                                    (params.pitch, params.gain),
                                );
                            }
                            AudioCommand::Stop { voice, .. } => {
                                pitch.remove(&(voice.slot.wrapping_sub(slot::ENGINE_LAYER)));
                            }
                            _ => {}
                        }
                    }
                }
                let want = def.engine.firing_hz(rpm);
                let lead = pitch.values().map(|v| v.1).fold(0.0f32, f32::max);
                for (i, (p, gain)) in &pitch {
                    let l = &pack.engine_layers[*i as usize];
                    let RpmAnchor::FiringHz(hz) = l.anchor else {
                        continue;
                    };
                    if !(0.26..=3.99).contains(p) {
                        continue; // clamped (never in a road car's range, see below)
                    }
                    let err = (p * hz / want).ln().abs() * 1200.0 / std::f32::consts::LN_2;
                    assert!(
                        err < 5.0,
                        "{id} at {rpm} rpm: {} plays {:.1} Hz, the car fires at {want:.1} Hz",
                        l.cue,
                        p * hz
                    );
                    // The leading loop is never stretched beyond what the rev range needs.
                    assert!(
                        *gain < lead || (0.5..=3.6).contains(p),
                        "{id} at {rpm} rpm: {} at {p:.2}x",
                        l.cue
                    );
                    checked += 1;
                }
            }
            rpm += 400.0;
        }
    }
    assert!(checked > 500, "only {checked} layer pitches checked");
}

/// `validate()` rejects a recorded engine layer rooted by the car's idle / redline / a fixed rpm.
#[test]
fn validate_rejects_recorded_layers_without_a_firing_frequency() {
    use wp_audio::data::RpmAnchor;
    let mut lib = AudioLibrary::embedded().unwrap();
    assert!(lib.validate().is_empty(), "{:?}", lib.validate());
    for bad in [
        RpmAnchor::Idle,
        RpmAnchor::RedlineFrac(0.5),
        RpmAnchor::Rpm(3000.0),
        RpmAnchor::FiringHz(0.0),
    ] {
        let pack = lib.packs.get_mut("engine_rec_v8").unwrap();
        let keep = pack.engine_layers[1].anchor;
        pack.engine_layers[1].anchor = bad;
        let errs = lib.validate();
        assert!(
            errs.iter()
                .any(|e| e.contains("engine_rec_v8") && e.contains("FiringHz")),
            "{bad:?}: {errs:?}"
        );
        lib.packs.get_mut("engine_rec_v8").unwrap().engine_layers[1].anchor = keep;
    }
}

/// Highest playback-rate ratio a recorded engine layer may reach within a car's rev range before
/// it sounds sped up ("singing", owner feedback on preview 0.12.34).
const MAX_RECORDED_PITCH: f32 = 1.6;

/// Idle and redline rpm of a car (`data/cars/<id>.json`).
fn car_rpm(car_id: &str) -> (f32, f32) {
    let path = audio_dir().join(format!("../cars/{car_id}.json"));
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let e = &v["engine"];
    (
        e["idle_rpm"].as_f64().unwrap() as f32,
        e["redline_rpm"].as_f64().unwrap() as f32,
    )
}

/// Playback-rate ratio of `pack`'s highest recorded layer at the car's redline.
fn top_pitch(lib: &AudioLibrary, car_id: &str, pack: &str) -> Option<f32> {
    let p = lib.packs.get(pack)?;
    let (idle, redline) = car_rpm(car_id);
    let layers =
        wp_audio::engine::EngineLayers::resolve(p, &lib.cars[car_id].engine, idle, redline);
    let top = layers
        .layers
        .iter()
        .map(|l| l.root_rpm)
        .fold(0.0f32, f32::max);
    (top > 0.0).then(|| redline / top)
}

/// For a car on a procedural engine pack `engine_proc_*`, the top layer pitch its recorded twin
/// (`engine_rec_*`) would reach at redline. `None` if there is no twin.
fn recorded_twin_top_pitch(lib: &AudioLibrary, car_id: &str, pack: &str) -> Option<f32> {
    let twin = format!("engine_rec_{}", pack.strip_prefix("engine_proc_")?);
    top_pitch(lib, car_id, &twin)
}

#[test]
fn police_engines_are_not_sped_up_recordings() {
    // Owner 0.12.34: "police car audio pitch is wrong, some of it sounds like it's singing".
    let lib = AudioLibrary::load_dir(audio_dir()).unwrap();
    for car in lib.cars.values().filter(|c| c.siren.is_some()) {
        let pack = &lib.packs[&car.engine_pack];
        let recorded = pack
            .cues
            .values()
            .any(|c| matches!(c.source, CueSource::Samples { .. }));
        if recorded {
            let ratio = top_pitch(&lib, &car.id, &car.engine_pack).unwrap();
            assert!(
                ratio <= MAX_RECORDED_PITCH,
                "cars/{}: {} pitches its top loop {ratio:.2}x at redline (max {MAX_RECORDED_PITCH})",
                car.id,
                car.engine_pack
            );
        }
    }
}
