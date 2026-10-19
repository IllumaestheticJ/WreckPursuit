//! Records a real `wp_police` pursuit for the UI kit, in simcli's output formats plus two extras.
//!
//! A scripted racer drives E's procedural police city (kinematic, along shortest paths, speeding
//! so heat builds); `PoliceSystem` runs with its own internal driving, exactly as in
//! `wp_police::scenario`. Writes into the directory given as the first argument
//! (default `../../fixtures/runs/police_pursuit`):
//!
//! - `events.jsonl` — every `SimEvent` fed to and emitted by the police (`{"tick","t_s","event"}`)
//! - `metrics.csv`  — the racer at 10 Hz (simcli columns; rpm/gear derived from speed)
//! - `summary.json` — entity mapping (simcli shape)
//! - `captain.jsonl` — `{"tick","view": CaptainView}` at 0.5 Hz (wp_police's own captain view)
//! - `map.json`     — the police road graph as a UI map (roads, POIs, occluders)
//!
//! Run via `npm run gen:police` in `apps/ui-kit`.

use std::fmt::Write as _;
use std::path::PathBuf;

use serde_json::json;
use wp_contracts::{
    EntityId, SignalState, SimEvent, SurfaceType, TimeOfDay, ViolationKind, WorldQuery, PHYSICS_DT,
};
use wp_math::Rng;
use wp_police::graph::{NodeId, RoadGraph};
use wp_police::{procedural_city, CityParams, District, PoliceConfig, PoliceSystem, RacerView};

const SEED: u64 = 4242;
const START_HOUR: f32 = 23.2;
const MAX_S: f32 = 240.0;
/// Keep recording this long after the first pursuit ends, s.
const TAIL_S: f32 = 12.0;
const RACER: EntityId = EntityId(1);
const SPEED_FACTOR: f32 = 2.3;
const CORNER_MPS: f32 = 13.0;
const INITIAL_HEAT: f32 = 15.0;

/// Clock-only world (same as `wp_police::scenario::ScenarioWorld`).
struct ClockWorld {
    time: TimeOfDay,
}

impl WorldQuery for ClockWorld {
    fn surface_at(&self, _p: [f32; 3]) -> SurfaceType {
        SurfaceType::Asphalt
    }
    fn speed_limit_at(&self, _p: [f32; 3]) -> Option<f32> {
        None
    }
    fn signal_state_ahead(&self, _p: [f32; 3], _h: [f32; 3]) -> Option<SignalState> {
        None
    }
    fn time_of_day(&self) -> TimeOfDay {
        self.time
    }
    fn line_of_sight(&self, _a: [f32; 3], _b: [f32; 3]) -> bool {
        true
    }
}

/// Kinematic racer following a node path (destinations in Core, like E's `day_core_speeder`).
struct Racer {
    pos: [f32; 2],
    speed: f32,
    path: Vec<NodeId>,
    next: usize,
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

fn random_node(rng: &mut Rng, g: &RoadGraph, d: Option<District>) -> NodeId {
    let cands: Vec<u32> = (0..g.nodes.len() as u32)
        .filter(|&n| d.is_none_or(|d| g.nodes[n as usize].district == d))
        .collect();
    NodeId(cands[rng.below(cands.len() as u32) as usize])
}

fn limit_at(g: &RoadGraph, p: [f32; 2]) -> f32 {
    g.nearest_on_road(p)
        .map(|r| g.segment(r.segment).speed_limit_mps)
        .unwrap_or(13.9)
}

fn gear_rpm(v: f32) -> (i32, f32) {
    let ratios = [3.6f32, 2.2, 1.6, 1.25, 1.0, 0.82];
    for (g, r) in ratios.iter().enumerate() {
        let rpm = v / 0.31 * r * 3.9 * 9.549;
        if rpm < 6800.0 || g == ratios.len() - 1 {
            return (g as i32 + 1, rpm.clamp(900.0, 7200.0));
        }
    }
    (1, 900.0)
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/runs/police_pursuit")
        });
    std::fs::create_dir_all(&out).expect("create output dir");

    let graph = procedural_city(&CityParams::default());
    let setup = wp_police::PoliceSetup {
        seed: SEED,
        internal_driving: true,
        hazards: true,
        ..Default::default()
    };
    let mut police = PoliceSystem::with_city(PoliceConfig::builtin(), graph.clone(), setup);
    let mut world = ClockWorld {
        time: TimeOfDay::from_hours(START_HOUR),
    };
    let mut rng = Rng::new(SEED ^ 0x0051_0E5A);

    let start = random_node(&mut rng, &graph, Some(District::Core));
    let mut racer = Racer {
        pos: graph.node(start).pos,
        speed: 8.0,
        path: vec![start],
        next: 1,
    };
    police.set_heat(RACER, INITIAL_HEAT);

    let mut events_jsonl = String::new();
    let mut captain_jsonl = String::new();
    let mut csv = String::from(
        "t_s,tick,entity,x,y,z,speed_mps,lat_g,long_g,yaw_rate_rps,engine_rpm,gear,health,crush_depth_m\n",
    );
    let mut out_events = Vec::new();
    let mut ended_at: Option<f32> = None;
    let max_ticks = (MAX_S / PHYSICS_DT).round() as u64;
    let mut last_violation_tick = 0u64;
    let mut prev_speed = racer.speed;
    let mut tick = 0u64;

    while tick < max_ticks {
        tick += 1;
        let t = tick as f32 * PHYSICS_DT;

        // New route when the current one is used up.
        if racer.next >= racer.path.len() {
            let from = *racer.path.last().expect("path");
            let mut to = random_node(&mut rng, &graph, Some(District::Core));
            if to == from {
                to = random_node(&mut rng, &graph, Some(District::Core));
            }
            racer.path = graph.shortest_path(from, to).unwrap_or_else(|| vec![from]);
            racer.next = 1;
            if racer.path.len() < 2 {
                continue;
            }
        }
        let target = graph.node(racer.path[racer.next]).pos;
        let d = dist(racer.pos, target);
        let limit = limit_at(&graph, racer.pos);
        let brake_d = (racer.speed * racer.speed - CORNER_MPS * CORNER_MPS).max(0.0) / (2.0 * 8.0);
        // Flat tyres from spike strips slow the racer down (puncture 0..1 per wheel).
        let punct = police.tire_puncture(RACER).iter().sum::<f32>() / 4.0;
        let cruise = limit * SPEED_FACTOR * (1.0 - 0.75 * punct);
        let want = if d < brake_d + 6.0 {
            CORNER_MPS.min(cruise)
        } else {
            cruise
        };
        let accel = if want > racer.speed { 6.0 } else { -8.0 };
        racer.speed = (racer.speed + accel * PHYSICS_DT).max(0.0);
        if (accel > 0.0 && racer.speed > want) || (accel < 0.0 && racer.speed < want) {
            racer.speed = want;
        }
        let step = racer.speed * PHYSICS_DT;
        let dir = [
            (target[0] - racer.pos[0]) / d.max(1e-3),
            (target[1] - racer.pos[1]) / d.max(1e-3),
        ];
        if step >= d {
            racer.pos = target;
            racer.next += 1;
        } else {
            racer.pos = [racer.pos[0] + dir[0] * step, racer.pos[1] + dir[1] * step];
        }

        // Speeding violations (thresholded to 2 Hz so the log stays readable).
        let mut events = Vec::new();
        if racer.speed > limit + 1.0 && tick - last_violation_tick >= 120 {
            last_violation_tick = tick;
            events.push(SimEvent::TrafficViolation {
                vehicle: RACER,
                kind: ViolationKind::Speeding {
                    over_mps: racer.speed - limit,
                },
                pos: [racer.pos[0], racer.pos[1], 0.0],
            });
        }
        let views = [RacerView {
            entity: RACER,
            pos: [racer.pos[0], racer.pos[1], 0.0],
            vel: [dir[0] * racer.speed, dir[1] * racer.speed, 0.0],
        }];
        world.time = world.time.advanced(PHYSICS_DT);
        out_events.clear();
        police.step(&world, &views, &events, PHYSICS_DT, &mut out_events);

        for e in events.iter().chain(out_events.iter()) {
            let line = json!({ "tick": tick, "t_s": t, "event": e });
            writeln!(events_jsonl, "{line}").expect("write");
        }
        if tick.is_multiple_of(24) {
            let (gear, rpm) = gear_rpm(racer.speed);
            let long_g = (racer.speed - prev_speed) / (24.0 * PHYSICS_DT) / 9.81;
            prev_speed = racer.speed;
            writeln!(
                csv,
                "{t:.4},{tick},racer,{:.4},{:.4},0.0000,{:.4},0.0000,{long_g:.4},0.0000,{rpm:.1},{gear},1.0000,0.0000",
                racer.pos[0], racer.pos[1], racer.speed
            )
            .expect("write");
        }
        if tick.is_multiple_of(480) {
            let line = json!({ "tick": tick, "view": police.captain_view() });
            writeln!(captain_jsonl, "{line}").expect("write");
        }
        if ended_at.is_none() && police.stats().pursuits.iter().any(|p| p.end_s.is_some()) {
            ended_at = Some(t);
        }
        if ended_at.is_some_and(|e| t > e + TAIL_S) {
            break;
        }
    }

    let summary = json!({
        "name": "police_pursuit",
        "recorded": "wp_police PoliceSystem (internal driving) + scripted kinematic racer; tools/record-police",
        "contract_version": wp_contracts::CONTRACT_VERSION,
        "seed": SEED,
        "ticks": tick,
        "entities": [{ "id": "racer", "entity": RACER.0, "car": "shiro_turbo" }],
        "pursuits": police.stats().pursuits.len(),
    });

    let roads: Vec<_> = graph
        .segments
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let a = graph.node(s.a).pos;
            let b = graph.node(s.b).pos;
            json!({ "id": i, "points": [a, b], "speed_limit_mps": s.speed_limit_mps, "district": s.district })
        })
        .collect();
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for n in &graph.nodes {
        for k in 0..2 {
            lo[k] = lo[k].min(n.pos[k]);
            hi[k] = hi[k].max(n.pos[k]);
        }
    }
    let map = json!({
        "id": "police_procedural_10x10",
        "source": "wp_police::procedural_city(CityParams::default()), recorded by tools/record-police",
        "bounds_min": lo,
        "bounds_max": hi,
        "roads": roads,
        "zones": graph.occluders.iter().enumerate().map(|(i, o)| json!({
            "id": i, "kind": "Building",
            "polygon": [[o.min[0], o.min[1]], [o.max[0], o.min[1]], [o.max[0], o.max[1]], [o.min[0], o.max[1]]],
        })).collect::<Vec<_>>(),
        "pois": graph.pois.iter().enumerate().map(|(i, p)| json!({
            "kind": format!("{:?}", p.kind), "name": format!("{:?} {}", p.kind, i + 1), "pos": p.pos,
        })).collect::<Vec<_>>(),
    });

    std::fs::write(out.join("events.jsonl"), events_jsonl).expect("write events");
    std::fs::write(out.join("metrics.csv"), csv).expect("write metrics");
    std::fs::write(out.join("captain.jsonl"), captain_jsonl).expect("write captain");
    std::fs::write(
        out.join("summary.json"),
        serde_json::to_string_pretty(&summary).expect("json") + "\n",
    )
    .expect("write summary");
    std::fs::write(out.join("map.json"), map.to_string()).expect("write map");
    println!(
        "police_pursuit: {tick} ticks ({:.1} s), {} pursuits, ended at {:?}",
        tick as f32 * PHYSICS_DT,
        police.stats().pursuits.len(),
        ended_at
    );
}
