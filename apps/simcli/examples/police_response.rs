//! Dispatch-driven police response in the full sim (workstream 06, lead feedback round 12b:
//! "the police spawn too far — spawn them in when there's public safety calls for reckless
//! driving").
//!
//! A player car drives Port Vesper v2 or the 12×12 grid at `speed` m/s (≈ 1.6× a 50 km/h
//! limit by default) with traffic, parked cars and physical police cars, from route to random
//! route. For each run it reports:
//! - the first reckless-driving call (`PoliceStats::calls`) and the time from it to the first
//!   police car within 100 m of the player;
//! - every police ground unit that appeared after the first second: distance to the player and
//!   whether the player (driver's eye, or a chase camera 6 m behind and 2.5 m up) had line of
//!   sight to it and it was in the player's view cone (±60°);
//! - every despawn: same check at the unit's last position.
//!
//! ```sh
//! cargo run --release -p simcli --example police_response -- [vesper|grid] [hour] [runs] [secs] [speed] [-v]
//! ```

use std::collections::BTreeMap;

use simcli::run::build_sim;
use simcli::{FsData, RunOptions, Scenario};
use wp_contracts::{SimEvent, WorldQuery, PHYSICS_DT};
use wp_police::{PoliceEvent, UnitKind};
use wp_world::drive::{DrivePath, FollowParams, PathFollower};

/// "On scene": a police car this close to the player, m.
const ON_SCENE_M: f32 = 100.0;
/// View-cone half angle for the pop-in audit, rad (≈ 60°).
const VIEW_HALF_ANGLE_RAD: f32 = 1.05;
/// Eye height for the player's line of sight, m.
const EYE_M: f32 = 1.3;

fn scenario(city: &str, hour: f32, seed: u64, secs: f32) -> Scenario {
    let city = match city {
        "grid" => "procedural(seed: 1, blocks_x: 12, blocks_y: 12, block_m: 110.0)",
        _ => "preset(name: \"vesper_v2\", seed: 1)",
    };
    let text = format!(
        r#"(
    name: "police_response_probe",
    seed: {seed},
    city: {city},
    time_of_day_h: {hour},
    time_ratio: 1.0,
    mode: Arcade,
    assists: keyboard,
    entities: [(id: "player", car: "debug_coupe", pos: (0.0, 0.0, 0.0))],
    duration_s: {secs},
)"#
    );
    ron::from_str(&text).expect("probe scenario parses")
}

#[derive(Default)]
struct RunReport {
    call_s: Option<f32>,
    on_scene_s: Option<f32>,
    spawns: u32,
    spawns_in_view: u32,
    spawns_in_sight: u32,
    spawn_min_m: f32,
    despawns: u32,
    despawns_in_sight: u32,
}

fn run_once(city: &str, hour: f32, seed: u64, secs: f32, speed: f32, verbose: bool) -> RunReport {
    let sc = scenario(city, hour, seed, secs);
    let data = FsData::new("data");
    let mut built = build_sim(&sc, &data, &RunOptions::default()).expect("builds");
    let player = built.ids[0];
    let car = built.cars[&sc.entities[0].car].def.clone();
    let sim = &mut built.sim;
    let nodes: Vec<[f32; 2]> = sim
        .world()
        .road_graph()
        .nodes
        .iter()
        .map(|n| [n.pos[0], n.pos[1]])
        .collect();
    let mut rng = wp_math::Rng::new(seed ^ 0xC0FF_EE11);
    // Start on a road near a random node, then chain routes to random far nodes.
    let start = nodes[rng.below(nodes.len() as u32) as usize];
    let pick_far = |rng: &mut wp_math::Rng, from: [f32; 2]| loop {
        let n = nodes[rng.below(nodes.len() as u32) as usize];
        if (n[0] - from[0]).hypot(n[1] - from[1]) > 400.0 {
            return n;
        }
    };
    let plan = |sim: &mut wp_sim::Sim, rng: &mut wp_math::Rng, from: [f32; 2]| {
        for _ in 0..20 {
            let to = pick_far(rng, from);
            if let Some(r) =
                sim.world()
                    .plan_route([from[0], from[1], 0.0], None, [to[0], to[1], 0.0])
            {
                return Some(DrivePath::from_route(&r));
            }
        }
        None
    };
    let mut drive = plan(sim, &mut rng, start).expect("a route");
    let (p, tan) = drive.point_at(0.0);
    sim.place_vehicle(player, p, wp_math::atan2(-tan[0], tan[1]), speed * 0.5)
        .expect("player placed");
    let mut follower = PathFollower::for_vehicle(&car, FollowParams::PURSUIT, speed);
    let mut stuck_s = 0.0f32;

    let mut rep = RunReport {
        spawn_min_m: f32::MAX,
        ..RunReport::default()
    };
    let cos_cone = wp_math::sin_cos(VIEW_HALF_ANGLE_RAD).1;
    let mut last_pos: BTreeMap<u32, [f32; 2]> = BTreeMap::new();
    let ticks = (secs / PHYSICS_DT).round() as u64;
    for tick in 0..ticks {
        let t = tick as f32 * PHYSICS_DT;
        let st = sim
            .vehicle_states()
            .into_iter()
            .find(|(id, _)| *id == player)
            .map(|(_, s)| s)
            .expect("player state");
        let pp = [st.pos[0], st.pos[1]];
        let (end, _) = drive.point_at(drive.length());
        if (end[0] - pp[0]).hypot(end[1] - pp[1]) < 90.0 {
            if let Some(d) = plan(sim, &mut rng, pp) {
                drive = d;
            }
        }
        // The probe's driver sometimes clips a kerb or wall: after 3 s stuck, put the car
        // back on a lane on a new route (a jump of a few metres).
        stuck_s = if st.speed_mps.abs() < 1.0 {
            stuck_s + PHYSICS_DT
        } else {
            0.0
        };
        if stuck_s > 3.0 {
            stuck_s = 0.0;
            if let Some(d) = plan(sim, &mut rng, pp) {
                let (p, tan) = d.point_at(0.0);
                let _ = sim.place_vehicle(player, p, wp_math::atan2(-tan[0], tan[1]), 5.0);
                follower = PathFollower::for_vehicle(&car, FollowParams::PURSUIT, speed);
                drive = d;
                if verbose {
                    println!("  {t:6.1} s  player stuck: re-placed on a new route");
                }
            }
            continue;
        }
        let out = follower.follow(&st, &drive, speed, 1.75, PHYSICS_DT);
        let _ = sim.set_input(player, out.input);
        sim.step_fixed();
        let fwd = wp_world::drive::forward_of(st.rot);
        // The driver's eye and a chase camera 6 m behind, 2.5 m up, looking at a car body.
        let eye = [pp[0], pp[1], st.pos[2] + EYE_M];
        let cam = [pp[0] - fwd[0] * 6.0, pp[1] - fwd[1] * 6.0, st.pos[2] + 2.5];
        let events = sim.drain_events();
        let sees = |q: [f32; 2]| {
            let body = [q[0], q[1], st.pos[2] + 1.0];
            sim.world().line_of_sight(eye, body) || sim.world().line_of_sight(cam, body)
        };
        for e in events {
            if verbose {
                if let SimEvent::Contact {
                    a, b, rel_speed, ..
                } = &e
                {
                    if (*a == player || *b == player) && *rel_speed > 1.0 {
                        println!("  {t:6.1} s  player contact {a:?}/{b:?} at {rel_speed:.1} m/s");
                    }
                }
            }
            let SimEvent::Police(ev) = e else { continue };
            match ev {
                PoliceEvent::UnitSpawned {
                    unit,
                    kind: UnitKind::Car,
                    pos,
                    ..
                } if t > 1.0 => {
                    let d = (pos[0] - pp[0]).hypot(pos[1] - pp[1]);
                    let los = sees([pos[0], pos[1]]);
                    let cone = d > 0.0
                        && ((pos[0] - pp[0]) * fwd[0] + (pos[1] - pp[1]) * fwd[1]) / d >= cos_cone;
                    rep.spawns += 1;
                    rep.spawn_min_m = rep.spawn_min_m.min(d);
                    rep.spawns_in_sight += los as u32;
                    rep.spawns_in_view += (d < 120.0 || (los && cone)) as u32;
                    println!(
                        "  {t:6.1} s  spawn unit {} at {d:.0} m, sight {los}, cone {cone}",
                        unit.0
                    );
                }
                PoliceEvent::UnitDespawned { unit } => {
                    if let Some(q) = last_pos.get(&unit.0) {
                        let d = (q[0] - pp[0]).hypot(q[1] - pp[1]);
                        let los = sees(*q);
                        rep.despawns += 1;
                        rep.despawns_in_sight += (d < 120.0 || los) as u32;
                    }
                }
                _ => {}
            }
        }
        let police = sim.police();
        last_pos = police
            .units()
            .iter()
            .filter(|u| u.kind == UnitKind::Car)
            .map(|u| (u.id.0, u.body.pos))
            .collect();
        if rep.call_s.is_none() {
            if let Some(c) = police.stats().calls.first() {
                rep.call_s = Some(c.at.secs());
                println!("  {t:6.1} s  first call (spawned responder: {})", c.spawned);
            }
        }
        if verbose && tick % (2.0 / PHYSICS_DT) as u64 == 0 {
            let near = police
                .units()
                .iter()
                .filter(|u| u.kind == UnitKind::Car)
                .map(|u| ((u.body.pos[0] - pp[0]).hypot(u.body.pos[1] - pp[1]), u))
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let level = police.wanted_level(player).number();
            if let Some((d, u)) = near {
                println!(
                    "  {t:6.1} s  player ({:.0}, {:.0}) {:.1} m/s L{level}; nearest unit {} {:?} {} {d:.0} m at ({:.0}, {:.0}) {:.1} m/s (cmd {:.1}); incidents {}",
                    pp[0],
                    pp[1],
                    st.speed_mps,
                    u.id.0,
                    u.role,
                    u.assignment.name(),
                    u.body.pos[0],
                    u.body.pos[1],
                    u.body.speed,
                    u.cmd.speed_mps,
                    police.incidents().len()
                );
            }
        }
        if rep.call_s.is_some() && rep.on_scene_s.is_none() {
            let near = police.units().iter().any(|u| {
                u.kind == UnitKind::Car
                    && !u.wrecked
                    && (u.body.pos[0] - pp[0]).hypot(u.body.pos[1] - pp[1]) <= ON_SCENE_M
            });
            if near {
                rep.on_scene_s = Some(t);
                println!("  {t:6.1} s  police car within {ON_SCENE_M} m");
            }
        }
    }
    rep
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let city = args.get(1).map_or("vesper", String::as_str).to_string();
    let hour: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(23.0);
    let runs: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(4);
    let secs: f32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(60.0);
    let speed: f32 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(22.0);
    let verbose = args.iter().any(|a| a == "-v");
    println!("{city} at {hour:.0}:00, {runs} runs × {secs} s, player at {speed} m/s");
    let mut responses = Vec::new();
    let mut total = RunReport {
        spawn_min_m: f32::MAX,
        ..RunReport::default()
    };
    let mut censored = 0;
    for seed in 1..=runs {
        println!("run seed {seed}");
        let r = run_once(&city, hour, seed, secs, speed, verbose);
        match (r.call_s, r.on_scene_s) {
            (Some(c), Some(o)) => responses.push(o - c),
            // A call in the last 30 s that nobody reached yet says nothing.
            (Some(c), None) if c > secs - 30.0 => {
                censored += 1;
                println!("  call at {c:.1} s, too late in the run to count");
            }
            (Some(_), None) => responses.push(f32::INFINITY),
            _ => println!("  no call (no unseen violation)"),
        }
        total.spawns += r.spawns;
        total.spawns_in_view += r.spawns_in_view;
        total.spawns_in_sight += r.spawns_in_sight;
        total.spawn_min_m = total.spawn_min_m.min(r.spawn_min_m);
        total.despawns += r.despawns;
        total.despawns_in_sight += r.despawns_in_sight;
    }
    let finite: Vec<f32> = responses
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .collect();
    let mean = finite.iter().sum::<f32>() / finite.len().max(1) as f32;
    let max = responses.iter().copied().fold(0.0, f32::max);
    println!(
        "call -> police car within {ON_SCENE_M} m: {} runs, mean {mean:.1} s, max {max:.1} s ({} never; {censored} calls too late to count)",
        responses.len(),
        responses.len() - finite.len()
    );
    println!(
        "spawns after 1 s: {} (in view cone or < 120 m: {}, in line of sight at all: {}), closest {}",
        total.spawns,
        total.spawns_in_view,
        total.spawns_in_sight,
        if total.spawns > 0 {
            format!("{:.0} m", total.spawn_min_m)
        } else {
            "-".to_string()
        }
    );
    println!(
        "despawns: {} (in sight or < 120 m: {})",
        total.despawns, total.despawns_in_sight
    );
}
