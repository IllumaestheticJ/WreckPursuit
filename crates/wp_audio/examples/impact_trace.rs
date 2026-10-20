//! Director command streams for the impact / horn renders (owner row 74), written as JSON for
//! `apps/testbed-web/e2e/audio_impacts.mjs`, which plays them through the browser's real voice
//! pool in an `OfflineAudioContext`, writes WAVs to `docs/media/audio/impacts/` and checks them.
//!
//! The crashes are real: each case runs a simcli scenario (Rapier contacts, `wp_damage`
//! lattice) and feeds the director every 4 ticks (60 Hz, like the browser) with the sim's
//! vehicles, the drained events and each car's measured plastic energy. Glass and horns are
//! short director-only scripts (a `GlassShattered` event; a held horn button).
//!
//! `cargo run -p wp_audio --example impact_trace -- out.json`

use std::path::{Path, PathBuf};

use serde_json::json;
use wp_audio::{
    AudioCommand, AudioDirector, AudioFrame, AudioLibrary, DriverKind, Listener, VehicleAudioInput,
};
use wp_contracts::{load_car, BodyRegion, EntityId, LoadedCar, SimEvent, VehicleState};

const TICKS_PER_FRAME: u64 = 4;
const TICK_S: f32 = 1.0 / 240.0;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn car(id: &str) -> LoadedCar {
    load_car(repo().join(format!("data/cars/{id}.json"))).expect("car loads")
}

/// Where the listener is: fixed in the world, or riding in the focus car's cabin.
#[derive(Clone, Copy)]
enum Ear {
    Fixed([f32; 3], [f32; 3]),
    Cabin,
}

fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q;
    let u = [x, y, z];
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let t = cross(u, v).map(|c| 2.0 * c);
    let c2 = cross(u, t);
    [
        v[0] + w * t[0] + c2[0],
        v[1] + w * t[1] + c2[1],
        v[2] + w * t[2] + c2[2],
    ]
}

fn listener(ear: Ear, focus: Option<&VehicleState>) -> Listener {
    match (ear, focus) {
        (Ear::Cabin, Some(s)) => {
            // Driver's head: left of centre, a little behind the middle, at eye height.
            let o = rotate(s.rot, [-0.35, -0.15, 1.05]);
            Listener {
                pos: [s.pos[0] + o[0], s.pos[1] + o[1], s.pos[2] + o[2]],
                vel: s.lin_vel,
                forward: rotate(s.rot, [0.0, 1.0, 0.0]),
                up: rotate(s.rot, [0.0, 0.0, 1.0]),
            }
        }
        (Ear::Fixed(pos, look), _) => {
            let d = [look[0] - pos[0], look[1] - pos[1], 0.0];
            let n = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-3);
            Listener {
                pos,
                vel: [0.0; 3],
                forward: [d[0] / n, d[1] / n, 0.0],
                up: [0.0, 0.0, 1.0],
            }
        }
        (Ear::Cabin, None) => Listener::default(),
    }
}

struct Case {
    name: String,
    what: String,
    frames: Vec<serde_json::Value>,
    contacts: Vec<serde_json::Value>,
    glass: Vec<f32>,
    extra: serde_json::Value,
}

fn frame_json(t: f32, l: &Listener, commands: &[AudioCommand]) -> serde_json::Value {
    json!({ "t": t, "listener": [l.pos, l.forward, l.up], "commands": commands })
}

/// Run a simcli scenario (RON text) and the director alongside it.
fn sim_case(name: &str, what: &str, ron: &str, ear: Ear, keep_s: f32) -> Case {
    let sc = simcli::Scenario::from_ron(name, ron).expect("scenario parses");
    let data = simcli::FsData::new(repo().join("data"));
    let mut d = AudioDirector::new(AudioLibrary::embedded().expect("audio data"), 0xA0D10);
    let mut pending: Vec<SimEvent> = Vec::new();
    let mut frames = Vec::new();
    let mut contacts = Vec::new();
    let mut glass = Vec::new();
    let mut first_contact: Option<f32> = None;
    let mut tick = 0u64;
    let mut hook = |tv: &simcli::TickView| -> Result<(), String> {
        tick += 1;
        let t = tick as f32 * TICK_S;
        for e in tv.events {
            match e {
                SimEvent::Contact {
                    a,
                    b,
                    rel_speed,
                    impulse_ns,
                    ..
                } => {
                    if *rel_speed >= 0.6 {
                        first_contact.get_or_insert(t);
                    }
                    if contacts.len() < 400 {
                        contacts.push(json!({ "t": t, "a": a.0, "b": b.0, "rel_speed": rel_speed, "impulse_ns": impulse_ns }));
                    }
                }
                SimEvent::GlassShattered { .. } => glass.push(t),
                _ => {}
            }
            pending.push(e.clone());
        }
        if !tick.is_multiple_of(TICKS_PER_FRAME) {
            return Ok(());
        }
        let sim = tv.sim;
        let states = sim.vehicle_states();
        let focus = sim.vehicles().first().map(|v| v.id);
        let mut inputs = Vec::new();
        for v in sim.vehicles() {
            if !d.has_car(&v.car_id) {
                d.register_car(&car(&v.car_id));
            }
            d.set_plastic_energy(v.id, v.damage.total_plastic_energy_j());
        }
        for (id, st) in &states {
            let Some(v) = sim.vehicles().iter().find(|v| v.id == *id) else {
                continue;
            };
            inputs.push(VehicleAudioInput {
                entity: *id,
                car_id: &v.car_id,
                state: st,
                throttle: v.input.throttle,
                throttle_applied: Some(v.vehicle.telemetry().throttle),
                horn: v.input.horn,
                siren: false,
                road_rumble: 0.0,
                kind: if Some(*id) == focus {
                    DriverKind::Player
                } else {
                    DriverKind::AiRacer
                },
            });
        }
        let fst = states.iter().find(|s| Some(s.0) == focus).map(|s| &s.1);
        let l = listener(ear, fst);
        let cmds = d.update(&AudioFrame {
            tick,
            listener: l,
            focus,
            vehicles: &inputs,
            events: &pending,
        });
        pending.clear();
        frames.push(frame_json(t, &l, &cmds));
        Ok(())
    };
    simcli::run_with_hook(&sc, &data, &simcli::RunOptions::default(), &mut hook)
        .expect("scenario runs");
    let fc = first_contact.unwrap_or(0.0);
    // Keep from the start (loops must be played before they are set) to `keep_s` after the hit.
    let frames: Vec<_> = frames
        .into_iter()
        .filter(|f| f["t"].as_f64().unwrap_or(0.0) as f32 <= fc + keep_s)
        .collect();
    Case {
        name: name.into(),
        what: what.into(),
        frames,
        contacts,
        glass,
        extra: json!({ "first_contact_s": first_contact }),
    }
}

fn frontal(kmh: f32, ear: Ear) -> Case {
    let v = kmh / 3.6;
    // Contact ~0.6 s in: bumper 2.35 m ahead of the chassis origin, barrier 1 m thick.
    let y = v * 0.6 + 2.35 + 0.5;
    let ron = format!(
        r#"(
    name: "audio_frontal_{kmh}",
    seed: 74,
    mode: Arcade,
    assists: ai,
    barriers: [(pos: (0.0, {y}, 0.75), size: (8.0, 1.0, 1.5))],
    entities: [
        (id: "car", car: "debug_sedan", speed_mps: {v}, driver: cruise(target_mps: {v})),
    ],
    duration_s: 3.4,
    hash_every: 240,
    expect: [],
)"#
    );
    let (tag, ear_txt) = match ear {
        Ear::Cabin => ("_interior", "driver's seat"),
        Ear::Fixed(..) => ("", "8 m to the side of the barrier"),
    };
    sim_case(
        &format!("frontal_{}kmh{tag}", kmh as u32),
        &format!("debug_sedan into a rigid barrier at {kmh} km/h (Arcade), heard from {ear_txt}"),
        &ron,
        ear,
        2.6,
    )
}

fn sideswipe() -> Case {
    // An overtaking car (22 m/s) leans into a slower one (12 m/s): ~10 m/s of sliding.
    let ron = r#"(
    name: "audio_sideswipe_render",
    seed: 1301,
    mode: Arcade,
    assists: ai,
    entities: [
        (id: "fast", car: "debug_sedan", pos: (0.0, -8.0, 0.0), speed_mps: 22.0, driver: cruise(target_mps: 22.0),
            inputs: [(t_s: 0.5, input: (steer: 0.22, throttle: 0.6)), (t_s: 0.85, input: (steer: 0.05, throttle: 0.6))]),
        (id: "slow", car: "debug_sedan", pos: (2.4, 0.0, 0.0), speed_mps: 12.0, driver: cruise(target_mps: 12.0)),
    ],
    duration_s: 3.6,
    hash_every: 240,
    expect: [],
)"#;
    sim_case(
        "sideswipe_scrape",
        "a sedan at 80 km/h overtakes one at 43 km/h and leans into its side, heard from the roadside",
        ron,
        Ear::Fixed([8.0, 14.0, 1.5], [1.2, 14.0, 0.5]),
        2.8,
    )
}

fn wall_scrape() -> Case {
    // A sedan at 70 km/h glances off a long wall and keeps steering into it: metal on concrete.
    let ron = r#"(
    name: "audio_wall_scrape_render",
    seed: 1302,
    mode: Arcade,
    assists: ai,
    barriers: [(pos: (3.4, 40.0, 0.75), size: (1.0, 80.0, 1.5))],
    entities: [
        (id: "car", car: "debug_sedan", pos: (0.0, 0.0, 0.0), yaw: -0.12, speed_mps: 19.5, driver: cruise(target_mps: 19.5),
            inputs: [(t_s: 0.3, input: (steer: 0.12, throttle: 0.7))]),
    ],
    duration_s: 3.6,
    hash_every: 240,
    expect: [],
)"#;
    sim_case(
        "wall_scrape",
        "a sedan at 70 km/h glances into a wall and grinds along it (sparks), heard from the roadside",
        ron,
        Ear::Fixed([-6.0, 30.0, 1.5], [2.9, 30.0, 0.5]),
        2.9,
    )
}

/// Director-only script on one car standing 6 m in front of the listener.
fn scripted(
    name: &str,
    what: &str,
    car_id: &str,
    kind: DriverKind,
    seconds: f32,
    horn: impl Fn(f32) -> bool,
    events_at: impl Fn(u64) -> Vec<SimEvent>,
) -> Case {
    let c = car(car_id);
    let mut d = AudioDirector::new(AudioLibrary::embedded().expect("audio data"), 0xA0D10);
    d.register_car(&c);
    let entity = EntityId(7);
    let l = listener(Ear::Fixed([0.0, 0.0, 1.5], [0.0, 6.0, 1.5]), None);
    let mut st = VehicleState {
        pos: [0.0, 6.0, 0.0],
        rot: [0.0, 0.0, 0.0, 1.0],
        lin_vel: [0.0; 3],
        ang_vel: [0.0; 3],
        wheel_states: Default::default(),
        engine_rpm: c.def.engine.idle_rpm,
        gear: 0,
        speed_mps: 0.0,
        health: 1.0,
        component_health: Default::default(),
        mode: wp_contracts::DamageMode::Arcade,
    };
    let mut frames = Vec::new();
    let mut glass = Vec::new();
    let n = (seconds * 60.0).round() as u64;
    for f in 0..n {
        let tick = (f + 1) * TICKS_PER_FRAME;
        let t = f as f32 / 60.0;
        let events = events_at(f);
        for e in &events {
            if matches!(e, SimEvent::GlassShattered { .. }) {
                glass.push(t);
            }
            if let SimEvent::RegionDamaged { severity, .. } = e {
                st.health = st.health.min(1.0 - 0.5 * severity);
            }
        }
        let inputs = [VehicleAudioInput {
            entity,
            car_id: &c.def.id,
            state: &st,
            throttle: 0.0,
            throttle_applied: Some(0.0),
            horn: horn(t),
            siren: false,
            road_rumble: 0.0,
            kind,
        }];
        let cmds = d.update(&AudioFrame {
            tick,
            listener: l,
            focus: (kind == DriverKind::Player).then_some(entity),
            vehicles: &inputs,
            events: &events,
        });
        frames.push(frame_json(t, &l, &cmds));
    }
    Case {
        name: name.into(),
        what: what.into(),
        frames,
        contacts: Vec::new(),
        glass,
        extra: json!({ "car": car_id }),
    }
}

const HOLD_S: f32 = 2.0;

fn horn_case(name: &str, what: &str, car_id: &str, kind: DriverKind, damaged: bool) -> Case {
    let mut c = scripted(
        name,
        what,
        car_id,
        kind,
        HOLD_S + 0.6,
        // Pressed at 0.1 s (frame 6), released 2.0 s later (frame 126).
        |t| (0.1..0.1 + HOLD_S - 1e-4).contains(&(t + 1e-4)),
        move |f| {
            if damaged && f == 0 {
                [
                    (BodyRegion::FrontBumper, 0.9),
                    (BodyRegion::Hood, 0.7),
                    (BodyRegion::FrameFront, 0.6),
                ]
                .into_iter()
                .map(|(region, severity)| SimEvent::RegionDamaged {
                    vehicle: EntityId(7),
                    region,
                    severity,
                })
                .collect()
            } else {
                Vec::new()
            }
        },
    );
    c.extra = json!({ "car": car_id, "hold_s": HOLD_S, "press_s": 0.1, "damaged": damaged });
    c
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "impact_trace.json".into());
    let side = Ear::Fixed([8.0, 0.0, 1.6], [0.0, 0.0, 0.6]);
    let mut cases = Vec::new();
    for kmh in [20.0, 50.0, 90.0] {
        // Ear 8 m to the side of the barrier face (moved per case with the barrier).
        let v = kmh / 3.6;
        let y = v * 0.6 + 2.35;
        let ear = match side {
            Ear::Fixed(p, _) => Ear::Fixed([p[0], y, p[2]], [0.0, y, 0.6]),
            e => e,
        };
        cases.push(frontal(kmh, ear));
    }
    cases.push(frontal(50.0, Ear::Cabin));
    cases.push(sideswipe());
    cases.push(wall_scrape());
    cases.push(scripted(
        "glass_break",
        "rear window (tempered) breaks at 0.2 s, windshield (laminated) at 1.6 s; car 6 m away",
        "debug_sedan",
        DriverKind::AiRacer,
        3.4,
        |_| false,
        |f| match f {
            12 => vec![SimEvent::GlassShattered {
                vehicle: EntityId(7),
                region: BodyRegion::RearGlass,
            }],
            96 => vec![SimEvent::GlassShattered {
                vehicle: EntityId(7),
                region: BodyRegion::Windshield,
            }],
            _ => Vec::new(),
        },
    ));
    cases.push(horn_case(
        "horn_disc_pair",
        "car horn (two electromagnetic disc horns, 420 + 500 Hz) held 2.0 s, 6 m away",
        "tansy_13",
        DriverKind::AiRacer,
        false,
    ));
    cases.push(horn_case(
        "horn_disc_pair_damaged",
        "the same horn with a smashed front end (bumper 0.9, hood 0.7): weaker, rattly, detuned",
        "tansy_13",
        DriverKind::AiRacer,
        true,
    ));
    cases.push(horn_case(
        "horn_air_truck",
        "truck air horn (three trumpets) held 2.0 s, 6 m away",
        "hauler_1500",
        DriverKind::AiRacer,
        false,
    ));
    cases.push(horn_case(
        "horn_air_police",
        "police air horn (two trumpets) held 2.0 s, 6 m away",
        "sentinel_cruiser",
        DriverKind::Police,
        false,
    ));
    let lib = AudioLibrary::embedded().expect("audio data");
    let cs: Vec<_> = cases
        .iter()
        .map(|c| {
            json!({
                "name": c.name,
                "what": c.what,
                "frames": c.frames,
                "contacts": c.contacts,
                "glass": c.glass,
                "extra": c.extra,
            })
        })
        .collect();
    let out = json!({ "library": { "config": lib.config, "packs": lib.packs }, "cases": cs });
    std::fs::write(&path, serde_json::to_string(&out).expect("json")).expect("write json");
    for c in &cases {
        println!(
            "{:<24} {:>4} frames, {:>3} contacts, glass {:?}, {}",
            c.name,
            c.frames.len(),
            c.contacts.len(),
            c.glass,
            c.extra
        );
    }
    println!("-> {path}");
}
