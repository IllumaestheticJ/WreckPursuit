//! Director command streams for the master-mix headroom renders
//! (`apps/testbed-web/e2e/audio_mix.mjs`): realistic scenes played through the browser's whole
//! mixer (every bus, siren rack, scanner, master chain) to check gain staging.
//!
//! Scenes (player `rampart_50`, a V8, on the flat `wp_vehicle` rig with the modelled engine;
//! chase camera 6 m behind, 1.8 m up, moving with the car):
//! - `v8_wot`: idle, then wide-open throttle through the gears;
//! - `chase`: the player at WOT with three `sentinel_cruiser` units, sirens on: two following
//!   close behind, one oncoming pass-by in the other lane; a cornering squeal, a big hit from a
//!   following unit (contact, plastic energy, glass) and two pursuit radio lines;
//! - `cruise`: a short pull, then a light part-throttle cruise;
//! - `idle`: standing still at idle.
//!
//! Each scene carries `analyse: [t0, t1]`, the window the measurements use (after warm-up).
//!
//! `cargo run --release -p wp_audio --example mix_trace -- out.json [--player <car_id>]`

use serde_json::json;
use wp_audio::*;
use wp_contracts::{
    load_car, AssistProfile, BodyRegion, EntityId, LoadedCar, SimEvent, VehicleInput, VehicleState,
};
use wp_vehicle::metrics::metric_assists;
use wp_vehicle::rig::Rig;

/// Physics ticks per audio frame (240 Hz sim, 60 Hz frames).
const TICKS_PER_FRAME: u64 = 4;
const PLAYER: EntityId = EntityId(1);
/// Default player car (`--player <car_id>` picks another, e.g. the louder flat-plane V8).
const PLAYER_CAR: &str = "rampart_50";
const POLICE_CAR: &str = "sentinel_cruiser";

fn car(id: &str) -> LoadedCar {
    let p =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../data/cars/{id}.json"));
    load_car(p).expect("car loads")
}

/// Chassis +Y in world space from a quaternion `[x, y, z, w]`.
fn forward(q: [f32; 4]) -> [f32; 3] {
    let [x, y, z, w] = q;
    [
        2.0 * (x * y - w * z),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z + w * x),
    ]
}

/// Chase camera: 6 m behind, 1.8 m up, moving with the car.
fn chase_cam(st: &VehicleState) -> Listener {
    let f = forward(st.rot);
    Listener {
        pos: [
            st.pos[0] - 6.0 * f[0],
            st.pos[1] - 6.0 * f[1],
            st.pos[2] + 1.8,
        ],
        vel: st.lin_vel,
        forward: f,
        up: [0.0, 0.0, 1.0],
    }
}

/// What a scene does at time `t` (s): player pedal, police present, extra events.
struct Scene {
    name: &'static str,
    secs: f32,
    analyse: [f32; 2],
    pedal: fn(f32) -> f32,
    police: bool,
}

fn run(scene: &Scene, player: &str) -> serde_json::Value {
    let lib = AudioLibrary::embedded().expect("audio data");
    let player_car = car(player);
    let police_car = car(POLICE_CAR);
    let mut dir = AudioDirector::new(lib, 11);
    dir.register_car(&player_car);
    dir.register_car(&police_car);
    dir.set_engine_model(EngineModelMode::Physical);
    dir.set_scanner_tier(ScannerTier::Elite);
    let assists: AssistProfile = metric_assists();
    let mut rig = Rig::new(&player_car);
    // One rig drives the police engines (they chase at full throttle too).
    let mut police_rig = Rig::new(&police_car);
    let units = [EntityId(11), EntityId(12), EntityId(13)];
    let n = (scene.secs * 60.0).round() as u64;
    let mut frames = Vec::new();
    let mut plastic = 0.0f32;
    for f in 0..n {
        let t = f as f32 / 60.0;
        let pedal = (scene.pedal)(t);
        let input = VehicleInput {
            throttle: pedal,
            ..Default::default()
        };
        let police_in = VehicleInput {
            throttle: 1.0,
            ..Default::default()
        };
        for _ in 0..TICKS_PER_FRAME {
            rig.step(&input, &assists);
            if scene.police {
                police_rig.step(&police_in, &assists);
            }
        }
        let tick = (f + 1) * TICKS_PER_FRAME;
        let mut st = rig.state();
        let tel = rig.vehicle.telemetry();
        let fwd = forward(st.rot);
        let side = [fwd[1], -fwd[0], 0.0];
        let at = |along: f32, lat: f32, z: f32| {
            [
                st.pos[0] + along * fwd[0] + lat * side[0],
                st.pos[1] + along * fwd[1] + lat * side[1],
                z,
            ]
        };
        // Cornering squeal: the tyres past their slip peak for 2 s.
        if scene.police && (6.0..8.0).contains(&t) {
            for w in st.wheel_states.iter_mut() {
                w.slip_angle = 0.2;
            }
        }
        let listener = chase_cam(&st);
        let mut states = Vec::new();
        if scene.police {
            let ps = police_rig.state();
            let follow = |along: f32, lat: f32| VehicleState {
                pos: at(along, lat, ps.pos[2]),
                rot: st.rot,
                lin_vel: st.lin_vel,
                ..ps.clone()
            };
            // Unit 11: 10 m behind the player (4 m behind the camera), right lane.
            states.push(follow(-10.0, 2.5));
            // Unit 12: 18 m behind, left.
            states.push(follow(-18.0, -2.5));
            // Unit 13: oncoming at 25 m/s in the other lane, passing the camera at t = 9 s.
            let closing = st.speed_mps + 25.0;
            let mut on = follow(-6.0 + (9.0 - t) * closing, -6.0);
            on.rot = [0.0, 0.0, 1.0, 0.0];
            on.lin_vel = [-25.0 * fwd[0], -25.0 * fwd[1], 0.0];
            states.push(on);
        }
        let mut inputs = vec![VehicleAudioInput {
            entity: PLAYER,
            car_id: player,
            state: &st,
            throttle: pedal,
            throttle_applied: Some(tel.throttle),
            horn: false,
            siren: false,
            road_rumble: 0.0,
            kind: DriverKind::Player,
        }];
        for (e, s) in units.iter().zip(&states) {
            inputs.push(VehicleAudioInput {
                entity: *e,
                car_id: POLICE_CAR,
                state: s,
                throttle: 1.0,
                throttle_applied: Some(1.0),
                horn: false,
                siren: true,
                road_rumble: 0.0,
                kind: DriverKind::Police,
            });
        }
        let mut events = Vec::new();
        if scene.police {
            // Radio chatter.
            for (at_s, unit, text) in [
                (
                    4.5,
                    11,
                    "Unit eleven, in pursuit, northbound, speeds over one-forty.",
                ),
                (10.0, 12, "Twelve, closing in, attempting contact."),
            ] {
                if (t - at_s).abs() < 0.5 / 60.0 {
                    dir.push_radio_line(RadioLine {
                        unit,
                        text: text.into(),
                        priority: RadioPriority::Pursuit,
                        emitted_tick: tick,
                    });
                }
            }
            // The big hit: unit 11 rams the player's rear quarter at 14 m/s closing speed.
            if (t - 12.0).abs() < 0.5 / 60.0 {
                events.push(SimEvent::Contact {
                    a: units[0],
                    b: PLAYER,
                    point: at(-2.2, 0.8, st.pos[2] + 0.5),
                    normal: fwd,
                    impulse_ns: 24_000.0,
                    rel_speed: 14.0,
                });
                events.push(SimEvent::GlassShattered {
                    vehicle: PLAYER,
                    region: BodyRegion::RearBumper,
                });
                plastic += 45_000.0;
            }
            dir.set_plastic_energy(PLAYER, plastic);
        }
        let commands = dir.update(&AudioFrame {
            tick,
            listener,
            focus: Some(PLAYER),
            vehicles: &inputs,
            events: &events,
        });
        let siren = dir.siren_space(&listener, &OpenAir);
        frames.push(json!({
            "t": t,
            "listener": [listener.pos, listener.forward, listener.up],
            "commands": commands,
            "siren": siren,
            "kmh": st.speed_mps * 3.6,
            "rpm": st.engine_rpm,
        }));
    }
    json!({ "name": scene.name, "analyse": scene.analyse, "frames": frames })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let player = args
        .iter()
        .position(|a| a == "--player")
        .and_then(|i| args.get(i + 1))
        .map_or(PLAYER_CAR, |s| s.as_str());
    let out = args
        .first()
        .filter(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| "mix_trace.json".into());
    let scenes = [
        Scene {
            name: "v8_wot",
            secs: 15.0,
            analyse: [1.0, 15.0],
            pedal: |t| if t < 1.0 { 0.0 } else { 1.0 },
            police: false,
        },
        Scene {
            name: "chase",
            secs: 18.0,
            analyse: [4.0, 18.0],
            pedal: |t| if (14.0..15.0).contains(&t) { 0.0 } else { 1.0 },
            police: true,
        },
        Scene {
            name: "cruise",
            secs: 20.0,
            analyse: [10.0, 20.0],
            pedal: |t| if t < 5.0 { 1.0 } else { 0.22 },
            police: false,
        },
        Scene {
            name: "idle",
            secs: 8.0,
            analyse: [1.0, 8.0],
            pedal: |_| 0.0,
            police: false,
        },
    ];
    let lib = AudioLibrary::embedded().expect("audio data");
    let doc = json!({
        "library": { "config": lib.config, "packs": lib.packs },
        "scenes": scenes.iter().map(|s| run(s, player)).collect::<Vec<_>>(),
    });
    std::fs::write(&out, doc.to_string()).expect("write json");
    eprintln!("wrote {out}");
}
