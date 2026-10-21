//! Spike strips heard (playtest row 132): the director plays a loud recorded blow-out per
//! punctured wheel (never culled), speaks a scanner line, flaps each deflated wheel at its
//! revolution rate, grinds a flat wheel's rim, and goes quiet again when the tyres are fine.

use std::path::Path;

use wp_audio::flat_tyre::{FLAP_SLOT, RIM_SLOT};
use wp_audio::*;
use wp_contracts::{load_car, EntityId, PoliceEvent, SimEvent, VehicleState};

fn director() -> AudioDirector {
    let mut d = AudioDirector::new(AudioLibrary::embedded().unwrap(), 3);
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    d.register_car(&load_car(repo.join("data/cars/debug_sedan.json")).unwrap());
    d.set_scanner_tier(ScannerTier::Elite);
    d
}

fn moving(omega: f32) -> VehicleState {
    let mut s = VehicleState {
        pos: [0.0, 12.0, 0.5],
        rot: [0.0, 0.0, 0.0, 1.0],
        lin_vel: [0.0, 20.0, 0.0],
        speed_mps: 20.0,
        engine_rpm: 3000.0,
        ..Default::default()
    };
    for w in &mut s.wheel_states {
        w.contact = true;
        w.omega = omega;
        w.fz_n = 3500.0;
    }
    s
}

fn input(s: &VehicleState) -> VehicleAudioInput<'_> {
    VehicleAudioInput {
        entity: EntityId(1),
        car_id: "debug_sedan",
        state: s,
        throttle: 0.4,
        throttle_applied: None,
        horn: false,
        siren: false,
        road_rumble: 0.0,
        kind: DriverKind::AiRacer,
    }
}

#[test]
fn spike_hit_pops_loudly_per_wheel_and_the_scanner_calls_it() {
    let mut d = director();
    let s = moving(60.0);
    let v = [input(&s)];
    let hit = [SimEvent::Police(PoliceEvent::SpikeHit {
        vehicle: EntityId(1),
        wheels: [true, true, false, false],
    })];
    let mut pops = Vec::new();
    let mut radio = Vec::new();
    for t in 0..240 * 5u64 {
        if t % 4 != 0 {
            continue;
        }
        let events: &[SimEvent] = if t == 40 { &hit } else { &[] };
        for c in d.update(&AudioFrame {
            tick: t,
            listener: Listener::default(),
            focus: None,
            vehicles: &v,
            events,
        }) {
            match c {
                AudioCommand::Play {
                    cue,
                    pack,
                    priority,
                    params,
                    ..
                } if cue == "tire/burst" => pops.push((pack, priority, params.gain)),
                AudioCommand::RadioSay { text, .. } => radio.push(text),
                _ => {}
            }
        }
    }
    assert_eq!(pops.len(), 2, "one pop per punctured wheel: {pops:?}");
    for (pack, prio, gain) in &pops {
        assert_eq!(pack, "world_rec", "the recorded blow-out");
        assert_eq!(*prio, Priority::Critical, "never culled");
        assert!(*gain > 0.5, "loud at 12 m: {gain}");
    }
    assert!(
        radio.iter().any(|t| t.to_lowercase().contains("spike")),
        "scanner line: {radio:?}"
    );
}

#[test]
fn deflated_wheels_flap_at_wheel_rate_and_flat_ones_grind() {
    let mut d = director();
    let mut tick = 0u64;
    let mut frame = |d: &mut AudioDirector, s: &VehicleState| {
        tick += 4;
        let v = [input(s)];
        d.update(&AudioFrame {
            tick,
            listener: Listener::default(),
            focus: None,
            vehicles: &v,
            events: &[],
        })
    };
    let params_of = |cmds: &[AudioCommand], slot: u32| {
        cmds.iter().find_map(|c| match c {
            AudioCommand::Play { voice, params, .. } | AudioCommand::Set { voice, params }
                if voice.entity == 1 && voice.slot == slot =>
            {
                Some(*params)
            }
            _ => None,
        })
    };
    // FL half flat, RR flat on its rim.
    d.set_tire_deflation(EntityId(1), [0.5, 0.0, 0.0, 1.0]);
    let fast = frame(&mut d, &moving(60.0));
    let flap = params_of(&fast, FLAP_SLOT).expect("FL flaps");
    let rev = 60.0 / std::f32::consts::TAU;
    assert!(
        (flap.freq_hz / flap.doppler - rev).abs() < 0.01,
        "one flap per revolution: {} vs {rev}",
        flap.freq_hz
    );
    assert!(params_of(&fast, RIM_SLOT).is_none(), "FL is not on its rim");
    assert!(params_of(&fast, FLAP_SLOT + 3).is_some());
    assert!(
        params_of(&fast, RIM_SLOT + 3).is_some(),
        "RR grinds on its rim"
    );
    assert!(params_of(&fast, FLAP_SLOT + 1).is_none(), "FR is fine");
    // Slower wheels flap slower.
    let slow = frame(&mut d, &moving(20.0));
    let s_flap = params_of(&slow, FLAP_SLOT).unwrap();
    assert!(s_flap.freq_hz < flap.freq_hz * 0.5);
    // With the vehicle's rim flag (grip-balance, 0.12.40): RR is flat but momentarily off the
    // road over a bump → no grind; back on the road → grinds again.
    d.set_rim_contact(EntityId(1), Some([false; 4]));
    let bump = frame(&mut d, &moving(60.0));
    assert!(
        params_of(&bump, RIM_SLOT + 3).is_none(),
        "no grind while the rim is off the road"
    );
    d.set_rim_contact(EntityId(1), Some([false, false, false, true]));
    let down = frame(&mut d, &moving(60.0));
    assert!(
        params_of(&down, RIM_SLOT + 3).is_some(),
        "grinds once the rim touches down"
    );
    d.set_rim_contact(EntityId(1), None);
    // Reinflated (repair): the loops stop.
    d.set_tire_deflation(EntityId(1), [0.0; 4]);
    let after = frame(&mut d, &moving(60.0));
    let stopped: Vec<u32> = after
        .iter()
        .filter_map(|c| match c {
            AudioCommand::Stop { voice, .. } if voice.entity == 1 => Some(voice.slot),
            _ => None,
        })
        .collect();
    for s in [FLAP_SLOT, FLAP_SLOT + 3, RIM_SLOT + 3] {
        assert!(stopped.contains(&s), "slot {s:#x} stops: {stopped:x?}");
    }
}
