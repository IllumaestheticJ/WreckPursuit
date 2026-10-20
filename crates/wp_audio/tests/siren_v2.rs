//! Siren v2 (playtest row 82) through the whole director: police-state cues pick the pattern,
//! pattern changes crossfade (old loop fades with the pack's `release_s`), whoop and air-horn
//! one-shots fire, the siren limit keeps the loudest and the focus, Doppler has the right sign
//! for an approaching and a receding car, and the stream stays deterministic.

use std::path::{Path, PathBuf};

use wp_audio::siren_unit::{AIRHORN_SLOT, WHOOP_SLOT};
use wp_audio::types::slot;
use wp_audio::*;
use wp_contracts::{load_car, EntityId, VehicleState};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn director() -> AudioDirector {
    let mut d = AudioDirector::new(AudioLibrary::embedded().unwrap(), 1);
    d.register_car(&load_car(repo().join("data/cars/sentinel_cruiser.json")).unwrap());
    d
}

fn police_state(pos: [f32; 3], vel: [f32; 3]) -> VehicleState {
    VehicleState {
        pos,
        rot: [0.0, 0.0, 0.0, 1.0],
        lin_vel: vel,
        speed_mps: (vel[0] * vel[0] + vel[1] * vel[1]).sqrt(),
        engine_rpm: 1500.0,
        ..Default::default()
    }
}

fn input<'a>(e: u32, s: &'a VehicleState, on: bool) -> VehicleAudioInput<'a> {
    VehicleAudioInput {
        entity: EntityId(e),
        car_id: "sentinel_cruiser",
        state: s,
        throttle: 0.3,
        throttle_applied: None,
        horn: false,
        siren: on,
        road_rumble: 0.0,
        kind: DriverKind::Police,
    }
}

fn frame<'a>(
    tick: u64,
    vehicles: &'a [VehicleAudioInput<'a>],
    focus: Option<u32>,
) -> AudioFrame<'a> {
    AudioFrame {
        tick,
        listener: Listener::default(),
        focus: focus.map(EntityId),
        vehicles,
        events: &[],
    }
}

fn is_loop(s: u32) -> bool {
    (slot::SIREN..slot::SIREN + 4).contains(&s)
}

/// Siren loop params of entity `e` in a frame's commands (Play or Set).
fn siren_params(cmds: &[AudioCommand], e: u32) -> Option<VoiceParams> {
    cmds.iter().find_map(|c| match c {
        AudioCommand::Play { voice, params, .. } | AudioCommand::Set { voice, params }
            if voice.entity == e && is_loop(voice.slot) =>
        {
            Some(*params)
        }
        _ => None,
    })
}

#[test]
fn doppler_is_up_approaching_and_down_receding() {
    for (vel_y, approaching) in [(-30.0, true), (30.0, false)] {
        let mut d = director();
        d.set_siren_cue(EntityId(5), Some(SirenCue::Respond));
        let s = police_state([0.0, 80.0, 0.5], [0.0, vel_y, 0.0]);
        let v = [input(5, &s, true)];
        let p = siren_params(&d.update(&frame(10, &v, None)), 5).expect("siren plays");
        let wail = d.library().config.sirens.cadence[&SirenPattern::Wail];
        let base = p.freq_hz / p.doppler;
        assert!(
            base >= wail.lo_hz * 0.98 && base <= wail.hi_hz * 1.02,
            "un-Dopplered frequency {base} in the wail range"
        );
        if approaching {
            assert!(p.doppler > 1.05 && p.freq_hz > base, "{p:?}");
            // Physical value c / (c - v) for a source closing at 30 m/s.
            assert!((p.doppler - 343.0 / 313.0).abs() < 0.01, "{}", p.doppler);
        } else {
            assert!(p.doppler < 0.95 && p.freq_hz < base, "{p:?}");
        }
    }
}

#[test]
fn cues_drive_patterns_with_crossfades_whoop_and_horn() {
    let mut d = director();
    let s = police_state([0.0, 30.0, 0.5], [0.0, 10.0, 0.0]);
    let v = [input(5, &s, true)];
    let script = |t: u64| match t {
        0..480 => SirenCue::Off,
        480..1200 => SirenCue::Respond,
        1200..2400 => SirenCue::Close,
        2400..3600 => SirenCue::Blocked,
        _ => SirenCue::Off,
    };
    let mut plays: Vec<(u64, String)> = Vec::new();
    let mut stops: Vec<(u64, u32, f32)> = Vec::new();
    let mut whoops = 0;
    let mut horns = 0;
    for t in (0..4200u64).step_by(4) {
        d.set_siren_cue(EntityId(5), Some(script(t)));
        for c in d.update(&frame(t, &v, None)) {
            match c {
                AudioCommand::Play { voice, cue, .. } if voice.entity == 5 => {
                    if is_loop(voice.slot) {
                        plays.push((t, cue));
                    } else if voice.slot == WHOOP_SLOT {
                        whoops += 1;
                    } else if voice.slot == AIRHORN_SLOT {
                        horns += 1;
                    }
                }
                AudioCommand::Stop { voice, fade_s } if is_loop(voice.slot) => {
                    stops.push((t, voice.slot, fade_s));
                }
                _ => {}
            }
        }
    }
    let cues: Vec<&str> = plays.iter().map(|(_, c)| c.as_str()).collect();
    assert_eq!(cues, ["siren/wail", "siren/yelp"], "{plays:?}");
    assert_eq!(
        plays[0].0, 480,
        "patrol is silent, responding wails at once"
    );
    assert_eq!(plays[1].0, 1200, "the wail has played longer than the hold");
    // Wail → yelp crossfades with the pack's release; yelp stops when the unit stands down.
    assert_eq!(stops.len(), 2, "{stops:?}");
    assert!(
        stops.iter().all(|(_, _, f)| (*f - 0.15).abs() < 1e-6),
        "{stops:?}"
    );
    assert_eq!(stops[1].0, 3600);
    assert_eq!(
        whoops, 1,
        "whoop when the chase starts (blocked is inside the cooldown)"
    );
    // 5 s blocked: bursts of 2 every 2.6 s → 4 blasts.
    assert!((3..=5).contains(&horns), "{horns} horn blasts");
}

#[test]
fn siren_limit_keeps_the_loudest_and_the_focus() {
    let mut d = director();
    let max = d.library().config.sirens.v2.max_sirens;
    let states: Vec<VehicleState> = (0..8)
        .map(|i| police_state([0.0, 20.0 + 25.0 * i as f32, 0.5], [0.0; 3]))
        .collect();
    let v: Vec<VehicleAudioInput> = states
        .iter()
        .enumerate()
        .map(|(i, s)| input(10 + i as u32, s, true))
        .collect();
    for e in 10..18 {
        d.set_siren_cue(EntityId(e), Some(SirenCue::Respond));
    }
    // Focus = the farthest car (17): it keeps its siren, the nearest others fill the rest.
    let cmds = d.update(&frame(1, &v, Some(17)));
    let mut ents: Vec<u32> = cmds
        .iter()
        .filter_map(|c| match c {
            AudioCommand::Play { voice, .. } if is_loop(voice.slot) => Some(voice.entity),
            _ => None,
        })
        .collect();
    ents.sort();
    assert_eq!(ents.len(), max);
    assert!(ents.contains(&17));
    assert_eq!(
        &ents[..max - 1],
        &(10..10 + max as u32 - 1).collect::<Vec<_>>()[..]
    );
    assert_eq!(d.sirens_playing(), ents);
    // The space pass reports one entry per playing siren.
    let space = d.siren_space(&Listener::default(), &OpenAir);
    assert_eq!(space.voices.len(), max);
    assert!(space
        .voices
        .iter()
        .all(|v| v.occlusion == 0.0 && v.gain == 1.0));
}

#[test]
fn several_units_beat_instead_of_phasing() {
    // Two units on the same cue at the same tick play slightly different frequencies.
    let mut d = director();
    let a = police_state([-5.0, 30.0, 0.5], [0.0; 3]);
    let b = police_state([5.0, 30.0, 0.5], [0.0; 3]);
    let v = [input(21, &a, true), input(22, &b, true)];
    d.set_siren_cue(EntityId(21), Some(SirenCue::Respond));
    d.set_siren_cue(EntityId(22), Some(SirenCue::Respond));
    let mut diffs = 0;
    for t in (0..2400).step_by(8) {
        let cmds = d.update(&frame(t, &v, None));
        let (pa, pb) = (
            siren_params(&cmds, 21).unwrap(),
            siren_params(&cmds, 22).unwrap(),
        );
        if (pa.freq_hz - pb.freq_hz).abs() > 1.0 {
            diffs += 1;
        }
    }
    assert!(
        diffs > 250,
        "units should not play in lockstep ({diffs}/300 frames differ)"
    );
}

#[test]
fn siren_stream_is_deterministic() {
    let run = || {
        let mut d = director();
        let s = police_state([3.0, 40.0, 0.5], [0.0, -12.0, 0.0]);
        let v = [input(5, &s, true)];
        let mut all = Vec::new();
        for t in (0..3000u64).step_by(4) {
            let cue = SirenCue::ALL[(t / 400) as usize % SirenCue::ALL.len()];
            d.set_siren_cue(EntityId(5), Some(cue));
            all.extend(d.update(&frame(t, &v, None)));
            let sp = d.siren_space(&Listener::default(), &OpenAir);
            all.push(AudioCommand::Duck {
                bus: Bus::Engine,
                gain: sp.engine_duck,
                fade_s: 0.0,
            });
        }
        serde_json::to_string(&all).unwrap()
    };
    assert_eq!(run(), run());
}
