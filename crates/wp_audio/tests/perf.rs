//! Director cost per frame. Opt-in (timing is machine dependent):
//! `cargo test -p wp_audio --release --test perf -- --ignored --nocapture`

use std::path::Path;
use std::time::Instant;

use wp_audio::*;
use wp_contracts::{load_car, EntityId, SimEvent, VehicleState};

#[test]
#[ignore]
fn director_frame_cost_24_cars() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let lib = AudioLibrary::embedded().unwrap();
    let mut dir = AudioDirector::new(lib, 1);
    let ids = [
        "debug_sedan",
        "hakone_gt",
        "sentinel_cruiser",
        "ironside_440",
    ];
    for id in ids {
        dir.register_car(&load_car(repo.join(format!("data/cars/{id}.json"))).unwrap());
    }
    let n = 24;
    let mut states: Vec<VehicleState> = (0..n)
        .map(|i| {
            let mut s = VehicleState {
                pos: [(i % 6) as f32 * 8.0, (i / 6) as f32 * 12.0, 0.5],
                rot: [0.0, 0.0, 0.0, 1.0],
                lin_vel: [0.0, 20.0, 0.0],
                engine_rpm: 3000.0,
                speed_mps: 20.0,
                ..Default::default()
            };
            for w in &mut s.wheel_states {
                w.contact = true;
                w.slip_angle = 0.2;
                w.fz_n = 4000.0;
                w.omega = 60.0;
            }
            s
        })
        .collect();
    let frames = 2000u64;
    let mut total = 0.0f64;
    let mut cmds = 0usize;
    for f in 0..frames {
        for (i, s) in states.iter_mut().enumerate() {
            s.engine_rpm = 1500.0 + ((f * 13 + i as u64 * 97) % 5500) as f32;
            s.wheel_states[0].compression_m = ((f + i as u64) % 7) as f32 * 0.01;
        }
        let inputs: Vec<VehicleAudioInput> = states
            .iter()
            .enumerate()
            .map(|(i, s)| VehicleAudioInput {
                entity: EntityId(i as u32 + 1),
                car_id: ids[i % ids.len()],
                state: s,
                throttle: if (f / 30 + i as u64).is_multiple_of(2) {
                    1.0
                } else {
                    0.0
                },
                throttle_applied: None,
                horn: false,
                siren: i % ids.len() == 2,
                road_rumble: 0.3,
                kind: DriverKind::Player,
            })
            .collect();
        let events: Vec<SimEvent> = (0..8u32)
            .map(|k| SimEvent::Contact {
                a: EntityId(k + 1),
                b: EntityId(k + 9),
                point: [0.0, 0.0, 0.5],
                normal: [1.0, 0.0, 0.0],
                impulse_ns: 2500.0,
                rel_speed: (f % 20) as f32,
            })
            .collect();
        let t = Instant::now();
        let out = dir.update(&AudioFrame {
            tick: f * 4,
            listener: Listener::default(),
            focus: Some(EntityId(1)),
            vehicles: &inputs,
            events: &events,
        });
        total += t.elapsed().as_secs_f64();
        cmds += out.len();
    }
    let ms = total * 1000.0 / frames as f64;
    println!("wp_audio director: {n} cars, 8 contacts/frame: {ms:.4} ms/frame ({:.4} ms/car), {} commands/frame", ms / n as f64, cmds as u64 / frames);
}
