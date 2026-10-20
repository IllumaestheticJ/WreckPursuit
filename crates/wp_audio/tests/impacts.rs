//! Owner row 74: physically driven impact synthesis, crunch, glass, friction scrapes, horns and
//! the interior perspective, at the director level (the browser renders are checked by
//! `apps/testbed-web/e2e/audio_impacts.mjs`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wp_audio::types::slot;
use wp_audio::*;
use wp_contracts::{
    load_car, BodyRegion, DamageMode, EntityId, LoadedCar, SimEvent, VehicleState, WORLD_ENTITY,
};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn car(id: &str) -> LoadedCar {
    load_car(repo().join(format!("data/cars/{id}.json"))).expect("car loads")
}

fn director(cars: &[&LoadedCar]) -> AudioDirector {
    let mut d = AudioDirector::new(AudioLibrary::embedded().expect("audio data"), 5);
    for c in cars {
        d.register_car(c);
    }
    d
}

fn state(pos: [f32; 3], vel: [f32; 3]) -> VehicleState {
    VehicleState {
        pos,
        rot: [0.0, 0.0, 0.0, 1.0],
        lin_vel: vel,
        ang_vel: [0.0; 3],
        wheel_states: Default::default(),
        engine_rpm: 900.0,
        gear: 1,
        speed_mps: vel[1],
        health: 1.0,
        component_health: Default::default(),
        mode: DamageMode::Arcade,
    }
}

fn input<'a>(
    e: u32,
    car: &'a str,
    st: &'a VehicleState,
    horn: bool,
    kind: DriverKind,
) -> VehicleAudioInput<'a> {
    VehicleAudioInput {
        entity: EntityId(e),
        car_id: car,
        state: st,
        throttle: 0.0,
        throttle_applied: None,
        horn,
        siren: false,
        road_rumble: 0.0,
        kind,
    }
}

fn listener(pos: [f32; 3]) -> Listener {
    Listener {
        pos,
        ..Default::default()
    }
}

/// Plays of `cue` in a command list: (synth params, params).
fn plays<'a>(
    cmds: &'a [AudioCommand],
    cue: &str,
) -> Vec<(&'a BTreeMap<String, f32>, &'a VoiceParams)> {
    cmds.iter()
        .filter_map(|c| match c {
            AudioCommand::Play {
                cue: q,
                synth,
                params,
                ..
            } if q == cue => Some((synth, params)),
            _ => None,
        })
        .collect()
}

/// A frontal contact of car 1 (at the origin, nose +Y) with the world at `speed`.
fn frontal(speed: f32) -> SimEvent {
    SimEvent::Contact {
        a: EntityId(1),
        b: WORLD_ENTITY,
        point: [0.0, 2.4, 0.5],
        normal: [0.0, 1.0, 0.0],
        // First contact tick of a ~0.1 s crash pulse.
        impulse_ns: 70.0 * speed,
        rel_speed: speed,
    }
}

fn hit_once(speed: f32) -> Vec<AudioCommand> {
    let c = car("debug_sedan");
    let mut d = director(&[&c]);
    let st = state([0.0, 0.0, 0.0], [0.0, speed, 0.0]);
    let l = listener([6.0, 0.0, 1.5]);
    let v = [input(1, "debug_sedan", &st, false, DriverKind::AiRacer)];
    d.update(&AudioFrame {
        tick: 4,
        listener: l,
        focus: None,
        vehicles: &v,
        events: &[],
    });
    let ev = [frontal(speed)];
    d.update(&AudioFrame {
        tick: 8,
        listener: l,
        focus: None,
        vehicles: &v,
        events: &ev,
    })
}

#[test]
fn a_hit_plays_one_body_voice_in_the_same_frame_scaled_by_speed() {
    let soft = hit_once(4.0);
    let hard = hit_once(25.0);
    let (s, _) = plays(&soft, "impact/body")[0];
    let (h, hp) = plays(&hard, "impact/body")[0];
    assert_eq!(
        plays(&hard, "impact/body").len(),
        1,
        "one struck vehicle → one body voice"
    );
    assert!(h["strength"] > s["strength"]);
    assert!(h["transient"] > 0.8 && s["transient"] == 0.0);
    assert!(h["duck"] > 0.3 && h["thump"] > 0.8);
    assert_eq!(h["region"], 0.0, "front bumper");
    assert!(hp.gain > 0.0);
    // The recorded layers still play under it.
    assert!(!plays(&hard, "impact/plastic_crack").is_empty());
}

#[test]
fn car_to_car_rings_both_bodies() {
    let c = car("debug_sedan");
    let mut d = director(&[&c]);
    let a = state([0.0, 0.0, 0.0], [0.0, 15.0, 0.0]);
    let b = state([0.0, 4.6, 0.0], [0.0, 0.0, 0.0]);
    let v = [
        input(1, "debug_sedan", &a, false, DriverKind::AiRacer),
        input(2, "debug_sedan", &b, false, DriverKind::AiRacer),
    ];
    let l = listener([6.0, 2.0, 1.5]);
    d.update(&AudioFrame {
        tick: 4,
        listener: l,
        focus: None,
        vehicles: &v,
        events: &[],
    });
    let ev = [SimEvent::Contact {
        a: EntityId(1),
        b: EntityId(2),
        point: [0.0, 2.35, 0.5],
        normal: [0.0, 1.0, 0.0],
        impulse_ns: 9000.0,
        rel_speed: 15.0,
    }];
    let cmds = d.update(&AudioFrame {
        tick: 8,
        listener: l,
        focus: None,
        vehicles: &v,
        events: &ev,
    });
    let bodies = plays(&cmds, "impact/body");
    assert_eq!(bodies.len(), 2);
    let regions: Vec<f32> = bodies.iter().map(|b| b.0["region"]).collect();
    // Car 1 hits with its nose (bumper); car 2 is struck in the tail (bumper / rear rails).
    assert_eq!(regions[0], 0.0, "{regions:?}");
    assert!(bodies[0].0["seed"] != bodies[1].0["seed"]);
}

#[test]
fn dented_panels_ring_less_and_buzz() {
    let c = car("debug_sedan");
    let mut d = director(&[&c]);
    let st = state([0.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
    let v = [input(1, "debug_sedan", &st, false, DriverKind::AiRacer)];
    let l = listener([6.0, 0.0, 1.5]);
    let roof_hit = |tick: u64| SimEvent::Contact {
        a: EntityId(1),
        b: WORLD_ENTITY,
        point: [0.0, -0.2, 1.45],
        normal: [0.0, 0.0, 1.0],
        impulse_ns: 3000.0,
        rel_speed: 6.0 + tick as f32 * 0.0,
    };
    d.update(&AudioFrame {
        tick: 4,
        listener: l,
        focus: None,
        vehicles: &v,
        events: &[],
    });
    let fresh = d.update(&AudioFrame {
        tick: 8,
        listener: l,
        focus: None,
        vehicles: &v,
        events: &[roof_hit(8)],
    });
    let dmg = [SimEvent::RegionDamaged {
        vehicle: EntityId(1),
        region: BodyRegion::Roof,
        severity: 0.8,
    }];
    d.update(&AudioFrame {
        tick: 400,
        listener: l,
        focus: None,
        vehicles: &v,
        events: &dmg,
    });
    let dented = d.update(&AudioFrame {
        tick: 800,
        listener: l,
        focus: None,
        vehicles: &v,
        events: &[roof_hit(800)],
    });
    let f = plays(&fresh, "impact/body")[0].0;
    let g = plays(&dented, "impact/body")[0].0;
    assert_eq!(f["region"], g["region"]);
    assert!(
        g["ring_s"] < f["ring_s"] * 0.6,
        "{} vs {}",
        g["ring_s"],
        f["ring_s"]
    );
    assert!(g["buzz"] > 0.5 && f["buzz"] == 0.0);
}

#[test]
fn crunch_follows_measured_deformation() {
    let c = car("debug_sedan");
    let mut d = director(&[&c]);
    let st = state([0.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
    let v = [input(1, "debug_sedan", &st, false, DriverKind::AiRacer)];
    let l = listener([6.0, 0.0, 1.5]);
    let mut crunch_frames = Vec::new();
    // Plastic energy: 0 J, then 40 kJ absorbed over three frames, then nothing more.
    let energy = [0.0, 0.0, 15_000.0, 30_000.0, 40_000.0];
    for f in 0..60u64 {
        d.set_plastic_energy(EntityId(1), energy[(f as usize).min(4)]);
        let cmds = d.update(&AudioFrame {
            tick: 4 * (f + 1),
            listener: l,
            focus: None,
            vehicles: &v,
            events: &[],
        });
        let on = cmds.iter().any(|c| match c {
            AudioCommand::Play { voice, .. } | AudioCommand::Set { voice, .. } => {
                voice.slot == slot::CRUNCH
            }
            _ => false,
        });
        if on {
            crunch_frames.push(f);
        }
    }
    assert_eq!(
        crunch_frames.first(),
        Some(&2),
        "crunch starts the frame the lattice yields"
    );
    let len = crunch_frames.len();
    assert!(
        (4..40).contains(&len),
        "crunch lasts a little longer than the crush: {len} frames"
    );
    // Without any deformation there is no crunch.
    let mut quiet = director(&[&c]);
    for f in 0..20u64 {
        quiet.set_plastic_energy(EntityId(1), 123.0);
        let cmds = quiet.update(&AudioFrame {
            tick: 4 * (f + 1),
            listener: l,
            focus: None,
            vehicles: &v,
            events: &[],
        });
        assert!(plays(&cmds, "impact/crunch").is_empty());
    }
}

#[test]
fn unmeasured_cars_crunch_from_the_collision_energy() {
    let cmds = hit_once(20.0);
    assert_eq!(plays(&cmds, "impact/crunch").len(), 1);
    assert!(plays(&hit_once(1.0), "impact/crunch").is_empty());
}

#[test]
fn glass_event_plays_shatter_and_debris_tail() {
    let c = car("debug_sedan");
    let mut d = director(&[&c]);
    let st = state([0.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
    let v = [input(1, "debug_sedan", &st, false, DriverKind::AiRacer)];
    let l = listener([6.0, 0.0, 1.5]);
    d.update(&AudioFrame {
        tick: 4,
        listener: l,
        focus: None,
        vehicles: &v,
        events: &[],
    });
    let ev = [SimEvent::GlassShattered {
        vehicle: EntityId(1),
        region: BodyRegion::RearGlass,
    }];
    let cmds = d.update(&AudioFrame {
        tick: 8,
        listener: l,
        focus: None,
        vehicles: &v,
        events: &ev,
    });
    assert_eq!(plays(&cmds, "impact/glass_shatter").len(), 1);
    let g = plays(&cmds, "impact/body");
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].0["kind"], 1.0);
    assert!(g[0].0["tinkles"] > 10.0);
    // Once only.
    let again = d.update(&AudioFrame {
        tick: 12,
        listener: l,
        focus: None,
        vehicles: &v,
        events: &ev,
    });
    assert!(plays(&again, "impact/body").is_empty());
}

#[test]
fn scrape_carries_friction_parameters() {
    let c = car("debug_sedan");
    let mut d = director(&[&c]);
    let l = listener([6.0, 0.0, 1.5]);
    let mut rates = Vec::new();
    for (k, speed) in [5.0f32, 15.0].into_iter().enumerate() {
        let st = state([0.0, 0.0, 0.0], [0.0, speed, 0.0]);
        let v = [input(1, "debug_sedan", &st, false, DriverKind::AiRacer)];
        let base = 1000 * (k as u64 + 1);
        let ev = [SimEvent::Contact {
            a: EntityId(1),
            b: WORLD_ENTITY,
            point: [0.9, 0.0, 0.5],
            normal: [1.0, 0.0, 0.0],
            impulse_ns: 400.0,
            rel_speed: 0.5,
        }];
        let cmds = d.update(&AudioFrame {
            tick: base,
            listener: l,
            focus: None,
            vehicles: &v,
            events: &ev,
        });
        let p = cmds.iter().find_map(|c| match c {
            AudioCommand::Play {
                voice,
                synth,
                params,
                ..
            } if voice.slot & slot::SCRAPE != 0 && !slot::is_one_shot(voice.slot) => {
                Some((synth.clone(), *params))
            }
            AudioCommand::Set { voice, params }
                if voice.slot & slot::SCRAPE != 0 && !slot::is_one_shot(voice.slot) =>
            {
                Some((BTreeMap::new(), *params))
            }
            _ => None,
        });
        let (synth, params) = p.expect("scrape voice");
        if k == 0 {
            assert_eq!(synth["friction"], 1.0);
            assert_eq!(synth["sparks"], 1.0, "metal on the world throws sparks");
        }
        rates.push(params.freq_hz);
    }
    assert!(
        rates[1] > rates[0] * 1.5,
        "stick-slip rate follows sliding speed: {rates:?}"
    );
}

#[test]
fn horn_weakens_with_front_damage_and_police_get_the_air_horn() {
    let sedan = car("debug_sedan");
    let cruiser = car("sentinel_cruiser");
    let mut d = director(&[&sedan, &cruiser]);
    let st = state([0.0, 6.0, 0.0], [0.0, 0.0, 0.0]);
    let st2 = state([3.0, 6.0, 0.0], [0.0, 0.0, 0.0]);
    let l = listener([0.0, 0.0, 1.5]);
    let v = [
        input(1, "debug_sedan", &st, true, DriverKind::AiRacer),
        input(2, "sentinel_cruiser", &st2, true, DriverKind::Police),
    ];
    let cmds = d.update(&AudioFrame {
        tick: 4,
        listener: l,
        focus: None,
        vehicles: &v,
        events: &[],
    });
    assert_eq!(plays(&cmds, "horn/standard").len(), 1);
    assert_eq!(plays(&cmds, "horn/police").len(), 1);
    assert_eq!(plays(&cmds, "horn/standard")[0].1.load, 0.0);
    let dmg = [SimEvent::RegionDamaged {
        vehicle: EntityId(1),
        region: BodyRegion::FrontBumper,
        severity: 0.9,
    }];
    let cmds = d.update(&AudioFrame {
        tick: 8,
        listener: l,
        focus: None,
        vehicles: &v,
        events: &dmg,
    });
    let load = cmds.iter().find_map(|c| match c {
        AudioCommand::Set { voice, params } if voice.entity == 1 && voice.slot == slot::HORN => {
            Some(params.load)
        }
        _ => None,
    });
    // Damage arrives with this frame's events; the horn reads it from the next frame on.
    let cmds2 = d.update(&AudioFrame {
        tick: 12,
        listener: l,
        focus: None,
        vehicles: &v,
        events: &[],
    });
    let load2 = cmds2.iter().find_map(|c| match c {
        AudioCommand::Set { voice, params } if voice.entity == 1 && voice.slot == slot::HORN => {
            Some(params.load)
        }
        _ => None,
    });
    assert!(
        load.unwrap_or(0.0).max(load2.unwrap_or(0.0)) >= 0.9,
        "{load:?} {load2:?}"
    );
    // Still exactly one horn voice, stopped on release.
    let v_off = [
        input(1, "debug_sedan", &st, false, DriverKind::AiRacer),
        input(2, "sentinel_cruiser", &st2, false, DriverKind::Police),
    ];
    let off = d.update(&AudioFrame {
        tick: 16,
        listener: l,
        focus: None,
        vehicles: &v_off,
        events: &[],
    });
    let stops = off
        .iter()
        .filter(|c| matches!(c, AudioCommand::Stop { voice, .. } if voice.slot == slot::HORN))
        .count();
    assert_eq!(stops, 2);
}

#[test]
fn interior_camera_muffles_other_cars_and_the_own_horn() {
    let c = car("debug_sedan");
    let mut d = director(&[&c]);
    let own = state([0.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
    let other = state([4.0, 10.0, 0.0], [0.0, 0.0, 0.0]);
    let v = [
        input(1, "debug_sedan", &own, true, DriverKind::Player),
        input(2, "debug_sedan", &other, true, DriverKind::AiRacer),
    ];
    let lp_of = |cmds: &[AudioCommand], e: u32| {
        cmds.iter().find_map(|c| match c {
            AudioCommand::Play { voice, params, .. }
                if voice.entity == e && voice.slot == slot::HORN =>
            {
                Some(params.lowpass_hz)
            }
            _ => None,
        })
    };
    // Driver's seat.
    let inside = d.update(&AudioFrame {
        tick: 4,
        listener: listener([-0.35, -0.15, 1.05]),
        focus: Some(EntityId(1)),
        vehicles: &v,
        events: &[],
    });
    assert_eq!(d.interior(), Some(EntityId(1)));
    assert!(lp_of(&inside, 2).unwrap() <= 2200.0);
    assert!(lp_of(&inside, 1).unwrap() <= 1600.0);
    // Chase camera behind the car.
    let mut d2 = director(&[&c]);
    let outside = d2.update(&AudioFrame {
        tick: 4,
        listener: listener([0.0, -6.0, 2.0]),
        focus: Some(EntityId(1)),
        vehicles: &v,
        events: &[],
    });
    assert_eq!(d2.interior(), None);
    assert!(lp_of(&outside, 2).unwrap() > 5000.0);
}

#[test]
fn impact_stream_is_deterministic() {
    let a = hit_once(18.0);
    let b = hit_once(18.0);
    assert_eq!(a, b);
}

#[test]
fn a_horizontal_hit_on_the_hood_edge_strikes_the_bumper() {
    // Round 13: a frontal into a wall at the bumper / hood seam (its height moves with pitch)
    // resolved to the hood plate and rang it like a drum at 20 km/h.
    let c = car("debug_sedan");
    let at = |normal: [f32; 3]| {
        let mut d = director(&[&c]);
        let st = state([0.0, 0.0, 0.0], [0.0, 5.0, 0.0]);
        let v = [input(1, "debug_sedan", &st, false, DriverKind::AiRacer)];
        let l = listener([6.0, 0.0, 1.5]);
        d.update(&AudioFrame {
            tick: 4,
            listener: l,
            focus: None,
            vehicles: &v,
            events: &[],
        });
        let ev = [SimEvent::Contact {
            a: EntityId(1),
            b: WORLD_ENTITY,
            point: [0.0, 2.2, 0.85],
            normal,
            impulse_ns: 400.0,
            rel_speed: 5.0,
        }];
        let cmds = d.update(&AudioFrame {
            tick: 8,
            listener: l,
            focus: None,
            vehicles: &v,
            events: &ev,
        });
        plays(&cmds, "impact/body")[0].0["region"]
    };
    assert_eq!(at([0.0, 1.0, 0.0]), 0.0, "wall ahead → bumper");
    assert_eq!(at([0.0, 0.2, 1.0]), 1.0, "struck from above → hood plate");
}
