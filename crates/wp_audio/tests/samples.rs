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
            assert!(
                pack.ends_with("_rec") || pack.contains("_rec_"),
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
