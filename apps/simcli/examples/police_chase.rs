//! Pursuit pressure in the full sim (workstream 06, owner playtest rows 61 / 63 / 64 / 66).
//!
//! A scripted racer flees through Port Vesper v2 (or the 12×12 grid) at `speed` m/s along
//! random routes, with traffic, parked cars and physical police cars. Each run places the racer
//! 30 m ahead of a patrolling police car, sets its heat to the middle of wanted level `L` and
//! starts the pursuit with [`wp_police::PoliceSystem::begin_pursuit`]. The heat is held at that
//! level for the run (normally a long chase escalates), so each level is measured on its own.
//! The racer never stops on purpose and is never re-placed: a spun or pinned racer that can't
//! drive on is captured by the normal rule (stopped 4 s with a unit within 15 m).
//!
//! Per level it reports:
//! - **converging (row 64)**: police cars in service within 300 m of the racer at 0, 10, …, 120 s;
//! - **aggression (row 63)**: time to the first police contact, contact episodes per pursuit
//!   minute, PIT attempts (a contact while the unit's tactic is `Pit`) and successes (the racer
//!   turns > 60° or drops below 4 m/s within 3 s), captures within the run;
//! - **push-through (row 61)**: shoves of blocking civilians / parked cars;
//! - **buildings (row 66)**: police contacts with building footprints and tunnel walls (a tall
//!   contact within 1.5 m of one; "fast" = at 3 m/s or more) and with parked props per pursuit minute
//!   (`PoliceStats::static_hits`).
//!
//! ```sh
//! cargo run --release -p simcli --example police_chase -- [vesper|grid] [hour] [runs] [secs] [speed] [levels] [-v]
//! # e.g. police_chase vesper 12 6 180 24 123
//! ```

use std::collections::BTreeMap;

use simcli::run::build_sim;
use simcli::{FsData, RunOptions, Scenario};
use wp_contracts::{EntityId, SimEvent, PHYSICS_DT};
use wp_police::stuck::StuckCause;
use wp_police::{Assignment, PoliceEvent, PursuitOutcome, Tactic, UnitKind};
use wp_world::drive::{DrivePath, FollowParams, PathFollower};

/// Sampling instants for the "units within 300 m" table, s after the pursuit start.
const SAMPLES: usize = 13;
const SAMPLE_EVERY_S: f32 = 10.0;
/// Verbose unit listing every this many seconds (`WP_UNITS`).
const LIST_EVERY_S: f32 = 2.0;
const NEAR_M: f32 = 300.0;
/// A wall contact at this speed or more is driving into it (below: scuffing while turning or
/// backing out), m/s.
const FAST_HIT_MPS: f32 = 3.0;

fn scenario(city: &str, hour: f32, seed: u64, secs: f32) -> Scenario {
    let city = match city {
        "grid" => "procedural(seed: 1, blocks_x: 12, blocks_y: 12, block_m: 110.0)",
        _ => "preset(name: \"vesper_v2\", seed: 1)",
    };
    let text = format!(
        r#"(
    name: "police_chase_probe",
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

/// Heat in the middle of each wanted level L1..L5.
fn level_heat(level: u32) -> f32 {
    [0.0, 20.0, 40.0, 60.0, 80.0, 95.0][level.min(5) as usize]
}

#[derive(Default, Clone)]
struct RunReport {
    /// Units within 300 m at each sample (None: the run had ended).
    near: Vec<Option<u32>>,
    pursuit_s: f32,
    first_contact_s: Option<f32>,
    contacts: u32,
    pit_attempts: u32,
    pit_success: u32,
    rams: u32,
    captured_s: Option<f32>,
    escaped_s: Option<f32>,
    pushes: u32,
    wall_hits: u32,
    wall_hits_fast: u32,
    prop_hits: u32,
    wall_hits_all: u32,
    unit_wrecks: u32,
    max_units: u32,
    racer_replaced: u32,
}

fn yaw_of(rot: [f32; 4]) -> f32 {
    let f = wp_world::drive::forward_of(rot);
    wp_math::atan2(f[1], f[0])
}

fn angle_diff(a: f32, b: f32) -> f32 {
    let mut d = a - b;
    while d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    }
    while d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    d.abs()
}

#[allow(clippy::too_many_arguments)]
fn run_once(
    city: &str,
    hour: f32,
    seed: u64,
    secs: f32,
    speed: f32,
    level: u32,
    verbose: bool,
) -> Option<RunReport> {
    let sc = scenario(city, hour, seed, secs + 5.0);
    let data = FsData::new("data");
    let mut built = build_sim(&sc, &data, &RunOptions::default()).expect("builds");
    let player = built.ids[0];
    let car = built.cars[&sc.entities[0].car].def.clone();
    let sim = &mut built.sim;
    if std::env::var("WP_NO_FEELERS").is_ok() {
        // Row 66 A/B: static feelers off.
        sim.police_mut().set_feelers(false);
    }
    let nodes: Vec<[f32; 2]> = sim
        .world()
        .road_graph()
        .nodes
        .iter()
        .map(|n| [n.pos[0], n.pos[1]])
        .collect();
    let mut rng = wp_math::Rng::new(seed ^ 0x0C4A_5E11);
    // Park the racer out of the way while the patrols spawn and get bodies.
    sim.place_vehicle(player, [nodes[0][0], nodes[0][1], 0.0], 0.0, 0.0)
        .expect("player placed");
    for _ in 0..(1.5 / PHYSICS_DT) as u32 {
        sim.step_fixed();
    }
    sim.drain_events();
    let pick_far = |rng: &mut wp_math::Rng, from: [f32; 2]| loop {
        let n = nodes[rng.below(nodes.len() as u32) as usize];
        if (n[0] - from[0]).hypot(n[1] - from[1]) > 500.0 {
            return n;
        }
    };
    let plan = |sim: &mut wp_sim::Sim,
                rng: &mut wp_math::Rng,
                from: [f32; 2],
                heading: Option<[f32; 2]>| {
        for _ in 0..20 {
            let to = pick_far(rng, from);
            if let Some(r) =
                sim.world()
                    .plan_route([from[0], from[1], 0.0], heading, [to[0], to[1], 0.0])
            {
                let d = DrivePath::from_route(&r);
                if d.length() > 200.0 {
                    return Some(d);
                }
            }
        }
        None
    };
    // A patrolling police car, the racer 30 m ahead of it.
    let patrols: Vec<([f32; 2], [f32; 2])> = sim
        .police()
        .units()
        .iter()
        .filter(|u| {
            u.kind == UnitKind::Car
                && u.entity.is_some()
                && matches!(u.assignment, Assignment::Patrol { .. })
        })
        .map(|u| (u.body.pos, u.body.heading))
        .collect();
    if patrols.is_empty() {
        println!("  no patrol to start from");
        return None;
    }
    let (up, uh) = patrols[rng.below(patrols.len() as u32) as usize];
    let start = [up[0] + uh[0] * 30.0, up[1] + uh[1] * 30.0];
    let mut drive = plan(sim, &mut rng, start, Some(uh))?;
    let (p, tan) = drive.point_at(0.0);
    sim.place_vehicle(player, p, wp_math::atan2(-tan[0], tan[1]), speed * 0.4)
        .expect("player placed");
    sim.step_fixed();
    sim.police_mut().set_heat(player, level_heat(level));
    let Some(first_unit) = sim.police_mut().begin_pursuit(player) else {
        println!("  pursuit did not start");
        return None;
    };
    if verbose {
        println!(
            "  pursuit by unit {} at ({:.0}, {:.0})",
            first_unit.0, p[0], p[1]
        );
    }
    let mut follower = PathFollower::for_vehicle(&car, FollowParams::PURSUIT, speed);
    let mut rep = RunReport {
        near: vec![None; SAMPLES],
        ..Default::default()
    };
    let hits_before = sim.police().stats().static_hits.len();
    let pushes_before = push_count(sim.police());
    // Contact episodes per unit: last contact time.
    let mut last_contact: BTreeMap<u32, f32> = BTreeMap::new();
    // Open PIT attempts: (time, racer yaw before).
    let mut pits: Vec<(f32, f32)> = Vec::new();
    let mut prev_yaw = 0.0f32;
    let ticks = (secs / PHYSICS_DT).round() as u64;
    let mut ended = false;
    let (mut stuck_s, mut reverse_until, mut reverse_steer, mut last_steer) =
        (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let mut tries: Vec<f32> = Vec::new();
    for tick in 0..ticks {
        let t = tick as f32 * PHYSICS_DT;
        let st = sim
            .vehicle_states()
            .into_iter()
            .find(|(id, _)| *id == player)
            .map(|(_, s)| s)
            .expect("player state");
        let pp = [st.pos[0], st.pos[1]];
        let yaw = yaw_of(st.rot);
        let (end, _) = drive.point_at(drive.length());
        if (end[0] - pp[0]).hypot(end[1] - pp[1]) < 90.0 {
            let f = wp_world::drive::forward_of(st.rot);
            if let Some(d) = plan(sim, &mut rng, pp, Some([f[0], f[1]])) {
                drive = d;
            }
        }
        // Hold the wanted level.
        if tick % 240 == 0 {
            let h = level_heat(level);
            let police = sim.police_mut();
            if police
                .racer(player)
                .is_some_and(|r| (r.heat - h).abs() > 1.0)
            {
                police.set_heat(player, h);
            }
        }
        if verbose && tick % 240 == 0 && t < 15.0 {
            println!(
                "  {t:6.1} s  racer at ({:.1}, {:.1}, {:.1}) {:.1} m/s, path {:.0} m",
                st.pos[0],
                st.pos[1],
                st.pos[2],
                st.speed_mps,
                drive.length()
            );
        }
        // A fleeing driver blocked by traffic or a wall backs out and goes again; after three
        // tries with no police car close by it is put back on its route a little further on
        // (the scripted driver can't plan round a jam the way a player would).
        if reverse_until > t {
            let input = wp_contracts::VehicleInput {
                brake: 0.7,
                steer: reverse_steer,
                ..Default::default()
            };
            let _ = sim.set_input(player, input);
        } else {
            stuck_s = if st.speed_mps.abs() < 1.0 {
                stuck_s + PHYSICS_DT
            } else {
                0.0
            };
            if stuck_s > 1.5 {
                stuck_s = 0.0;
                tries.retain(|x| t - x < 15.0);
                tries.push(t);
                let cop_near = sim.police().units().iter().any(|u| {
                    u.kind == UnitKind::Car
                        && !u.wrecked
                        && (u.body.pos[0] - pp[0]).hypot(u.body.pos[1] - pp[1]) < 20.0
                });
                if tries.len() >= 3 && !cop_near {
                    let (s_here, _, _) = drive.project(pp, None);
                    let (p, tan) = drive.point_at(s_here + 15.0);
                    let _ = sim.place_vehicle(player, p, wp_math::atan2(-tan[0], tan[1]), 5.0);
                    follower = PathFollower::for_vehicle(&car, FollowParams::PURSUIT, speed);
                    rep.racer_replaced += 1;
                    tries.clear();
                    if verbose {
                        println!("  {t:6.1} s  racer re-placed further along its route");
                    }
                    continue;
                }
                reverse_until = t + 1.5;
                reverse_steer = -last_steer.signum();
            }
            let out = follower.follow(&st, &drive, speed, lane_off(), PHYSICS_DT);
            last_steer = out.input.steer;
            let _ = sim.set_input(player, out.input);
        }
        sim.step_fixed();
        // PIT outcomes.
        pits.retain(|(t0, yaw0)| {
            if angle_diff(yaw, *yaw0) > 1.05 || st.speed_mps < 4.0 {
                rep.pit_success += 1;
                if verbose {
                    println!("  {t:6.1} s  PIT success ({:.1} s after contact)", t - t0);
                }
                false
            } else {
                t - t0 < 3.0
            }
        });
        let events = sim.drain_events();
        let unit_of = |e: EntityId| {
            sim.police()
                .units()
                .iter()
                .find(|u| u.entity == Some(e))
                .map(|u| (u.id.0, u.tactic))
        };
        for e in events {
            match e {
                SimEvent::Contact {
                    a, b, impulse_ns, ..
                } if a == player || b == player => {
                    let other = if a == player { b } else { a };
                    let Some((uid, tactic)) = unit_of(other) else {
                        if verbose && t < 30.0 && impulse_ns > 200.0 {
                            println!("  {t:6.1} s  racer touched {other:?} ({impulse_ns:.0} N·s)");
                        }
                        continue;
                    };
                    let fresh = last_contact.get(&uid).is_none_or(|l| t - l > 0.5);
                    last_contact.insert(uid, t);
                    if !fresh {
                        continue;
                    }
                    rep.contacts += 1;
                    if rep.first_contact_s.is_none() {
                        rep.first_contact_s = Some(t);
                    }
                    match tactic {
                        Tactic::Pit => {
                            rep.pit_attempts += 1;
                            pits.push((t, prev_yaw));
                        }
                        Tactic::Ram(_) => rep.rams += 1,
                        _ => {}
                    }
                    if verbose {
                        println!(
                            "  {t:6.1} s  contact unit {uid} ({tactic:?}) {impulse_ns:.0} N·s, racer {:.1} m/s",
                            st.speed_mps
                        );
                    }
                }
                SimEvent::Police(PoliceEvent::PursuitEnded { racer, outcome, .. })
                    if racer == player =>
                {
                    match outcome {
                        PursuitOutcome::Captured | PursuitOutcome::CapturedTotalLoss => {
                            rep.captured_s = Some(t);
                        }
                        _ => rep.escaped_s = Some(t),
                    }
                    if verbose {
                        println!("  {t:6.1} s  pursuit ended: {outcome:?}");
                    }
                    ended = true;
                }
                SimEvent::Contact {
                    a,
                    b,
                    point,
                    normal,
                    rel_speed,
                    ..
                } if verbose
                    && std::env::var("WP_WALLS").is_ok()
                    && (b == wp_contracts::WORLD_ENTITY || a == wp_contracts::WORLD_ENTITY)
                    && rel_speed >= 3.0
                    && normal[2].abs() < 0.6 =>
                {
                    let car = if a == wp_contracts::WORLD_ENTITY {
                        b
                    } else {
                        a
                    };
                    if let Some(u) = sim.police().units().iter().find(|u| u.entity == Some(car)) {
                        println!(
                            "  {t:6.1} s  WALL unit {} {} {:?} at ({:.0},{:.0},{:.1}) {:.1} m/s rel {:.1}, cmd {:.1} planned {:.1} wall {:.1} shift {:.1} routed {} reversing {} swinging {}",
                            u.id.0, u.assignment.name(), u.tactic, point[0], point[1], point[2] - u.body.alt_m,
                            u.body.speed, rel_speed, u.cmd.speed_mps, u.planned_speed, u.avoid.wall.limit_mps, u.avoid.wall.shift_m,
                            u.chase_route.is_some(), u.unstick.reversing(), u.unstick.swinging()
                        );
                    }
                }
                SimEvent::Police(PoliceEvent::UnitDisabled { .. }) => rep.unit_wrecks += 1,
                _ => {}
            }
        }
        prev_yaw = yaw;
        let police = sim.police();
        let in_service = police
            .units()
            .iter()
            .filter(|u| {
                u.kind == UnitKind::Car && !u.wrecked && u.assignment != Assignment::OutOfService
            })
            .count() as u32;
        rep.max_units = rep.max_units.max(in_service);
        if tick % (LIST_EVERY_S / PHYSICS_DT).round() as u64 == 0
            && verbose
            && std::env::var("WP_UNITS").is_ok()
        {
            for u in police
                .units()
                .iter()
                .filter(|u| u.assignment.target() == Some(player))
            {
                println!(
                            "      {t:5.1} unit {} {} {:?} {:.0} m, {:.1} m/s (cmd {:.1}, planned {:.1}, avoid {:.1} {:?}, wall {:.1}), risk {:.1}, track {:.0}/{:.0} routed {}, pos ({:.0},{:.0}) hd ({:.2},{:.2})",
                            u.id.0,
                            u.assignment.name(),
                            u.tactic,
                            (u.body.pos[0] - pp[0]).hypot(u.body.pos[1] - pp[1]),
                            u.body.speed,
                            u.cmd.speed_mps,
                            u.planned_speed,
                            u.avoid.limit_mps,
                            u.avoid.blocked_by,
                            u.avoid.wall.limit_mps,
                            u.risk,
                            u.track_s,
                            u.cmd.track.length(),
                            u.chase_route.is_some(),
                            u.body.pos[0], u.body.pos[1], u.body.heading[0], u.body.heading[1]
                        );
                if std::env::var("WP_WHY").is_ok()
                    && matches!(
                        u.avoid.blocked_by,
                        Some(wp_police::avoid::BlockedBy::Traffic)
                    )
                {
                    let q = u.body.pos;
                    for a in sim.world().agents() {
                        let d = (a.pos[0] - q[0]).hypot(a.pos[1] - q[1]);
                        if d < 15.0 {
                            println!(
                                "          agent at {d:.1} m lod {:?} speed {:.1}",
                                a.lod, a.speed_mps
                            );
                        }
                    }
                    for pr in sim.world().static_props() {
                        let d = (pr.pos[0] - q[0]).hypot(pr.pos[1] - q[1]);
                        if d < 10.0 {
                            println!(
                                "          prop at {d:.1} m ({:.1}, {:.1}) half {:?}",
                                pr.pos[0], pr.pos[1], pr.half_extents
                            );
                        }
                    }
                }
            }
        }
        if tick % (SAMPLE_EVERY_S / PHYSICS_DT).round() as u64 == 0 {
            let k = (t / SAMPLE_EVERY_S).round() as usize;
            if k < SAMPLES {
                let near = police
                    .units()
                    .iter()
                    .filter(|u| {
                        u.kind == UnitKind::Car
                            && !u.wrecked
                            && u.assignment != Assignment::OutOfService
                            && (u.body.pos[0] - pp[0]).hypot(u.body.pos[1] - pp[1]) <= NEAR_M
                    })
                    .count() as u32;
                rep.near[k] = Some(near);
                if verbose {
                    let on = police
                        .units()
                        .iter()
                        .filter(|u| u.assignment.target() == Some(player))
                        .count();
                    println!(
                        "  {t:6.1} s  racer ({:.0}, {:.0}) {:.1} m/s L{}; {near} units within {NEAR_M} m, {on} on the racer, {in_service} in service",
                        pp[0],
                        pp[1],
                        st.speed_mps,
                        police.wanted_level(player).number()
                    );
                }
            }
        }
        rep.pursuit_s = t;
        if ended {
            break;
        }
    }
    let walls = wall_segments(sim.world());
    let police = sim.police();
    for h in &police.stats().static_hits[hits_before..] {
        // Tall contacts on a building footprint or tunnel wall (kerbs and junction steps in
        // front of a building are not building hits).
        let on_building = h.kind == StuckCause::Wall
            && walls
                .iter()
                .any(|w| seg_dist(w[0], w[1], h.pos) <= BUILDING_HIT_M);
        if on_building {
            rep.wall_hits_all += 1;
        }
        if !h.pursuing {
            continue;
        }
        match h.kind {
            StuckCause::Wall if on_building => {
                rep.wall_hits += 1;
                rep.wall_hits_fast += (h.speed_mps >= FAST_HIT_MPS) as u32;
            }
            StuckCause::ParkedProp => rep.prop_hits += 1,
            _ => {}
        }
        if verbose && (on_building || h.kind == StuckCause::ParkedProp) {
            println!(
                "  {:6.1} s  unit {} hit {:?}{} at ({:.0}, {:.0}) at {:.1} m/s{}",
                h.at.secs(),
                h.unit.0,
                h.kind,
                if on_building { " (building)" } else { "" },
                h.pos[0],
                h.pos[1],
                h.speed_mps,
                if h.turning { " turning" } else { "" }
            );
        }
    }
    rep.pushes = push_count(police) - pushes_before;
    Some(rep)
}

/// A static contact this close to a building footprint or tunnel wall is a building hit, m.
const BUILDING_HIT_M: f32 = 1.5;

/// Building footprint edges and tunnel walls of the city (row 66's "buildings").
fn wall_segments(world: &wp_world::World) -> Vec<[[f32; 2]; 2]> {
    let net = world.network();
    let mut v = Vec::new();
    for b in &net.buildings {
        let o = &b.footprint;
        for k in 0..o.len() {
            v.push([o[k], o[(k + 1) % o.len()]]);
        }
    }
    for c in &net.covered {
        for w in c.centre.windows(2) {
            let (a, b) = ([w[0][0], w[0][1]], [w[1][0], w[1][1]]);
            let d = [b[0] - a[0], b[1] - a[1]];
            let l = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-6);
            let n = [-d[1] / l * c.half_width_m, d[0] / l * c.half_width_m];
            for sgn in [1.0, -1.0] {
                v.push([
                    [a[0] + sgn * n[0], a[1] + sgn * n[1]],
                    [b[0] + sgn * n[0], b[1] + sgn * n[1]],
                ]);
            }
        }
    }
    v
}

/// Distance from `p` to the segment `a`-`b`, m.
fn seg_dist(a: [f32; 2], b: [f32; 2], p: [f32; 2]) -> f32 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l2 = d[0] * d[0] + d[1] * d[1];
    let t = if l2 > 0.0 {
        (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((a[0] + t * d[0] - p[0]).powi(2) + (a[1] + t * d[1] - p[1]).powi(2)).sqrt()
}

/// Push-through shoves so far (row 61).
fn push_count(p: &wp_police::PoliceSystem) -> u32 {
    p.stats().pushes
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let city = args.get(1).map_or("vesper", String::as_str).to_string();
    let hour: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(12.0);
    let runs: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(6);
    let secs: f32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(180.0);
    let speed: f32 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(24.0);
    let levels: Vec<u32> = args
        .get(6)
        .map_or("123", String::as_str)
        .chars()
        .filter_map(|c| c.to_digit(10))
        .collect();
    let verbose = args.iter().any(|a| a == "-v");
    let only: Option<u64> = std::env::var("WP_SEED").ok().and_then(|v| v.parse().ok());
    println!("{city} at {hour:.0}:00, {runs} runs × {secs} s per level, racer at {speed} m/s");
    let mut table = Vec::new();
    for &level in &levels {
        println!("== L{level}");
        let mut reps = Vec::new();
        for seed in (1..=runs).filter(|s| only.is_none_or(|o| o == *s)) {
            println!("run seed {seed}");
            if let Some(r) = run_once(&city, hour, seed, secs, speed, level, verbose) {
                println!(
                    "  first contact {:?}, contacts {}, PIT {}/{}, rams {}, captured {:?}, escaped {:?}, walls {} (fast {}, all units {}), props {}, pushes {}, wrecks {}, racer re-placed {}, near {:?}",
                    r.first_contact_s.map(|v| (v * 10.0).round() / 10.0),
                    r.contacts,
                    r.pit_success,
                    r.pit_attempts,
                    r.rams,
                    r.captured_s.map(|v| v.round()),
                    r.escaped_s.map(|v| v.round()),
                    r.wall_hits,
                    r.wall_hits_fast,
                    r.wall_hits_all,
                    r.prop_hits,
                    r.pushes,
                    r.unit_wrecks,
                    r.racer_replaced,
                    r.near.iter().map(|n| n.map_or(-1, |v| v as i32)).collect::<Vec<_>>()
                );
                reps.push(r);
            }
        }
        table.push((level, reps));
    }
    println!();
    println!(
        "Units within {NEAR_M} m of the racer (mean over runs still going), t = 0, 10, … 120 s:"
    );
    for (level, reps) in &table {
        let row: Vec<String> = (0..SAMPLES)
            .map(|k| {
                let v: Vec<u32> = reps.iter().filter_map(|r| r.near[k]).collect();
                if v.is_empty() {
                    "-".to_string()
                } else {
                    format!("{:.1}", v.iter().sum::<u32>() as f32 / v.len() as f32)
                }
            })
            .collect();
        let max = reps.iter().map(|r| r.max_units).max().unwrap_or(0);
        println!("  L{level}: {} (most in service {max})", row.join(" | "));
    }
    println!();
    println!("| Level | Runs | First contact, mean s (runs with one) | Contacts / pursuit-min | PIT success / attempts | Rams | Captured in {secs:.0} s | Escaped | Wall hits / pursuit-min (pursuing units) | Prop hits / pursuit-min | Pushes | Units disabled |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|");
    for (level, reps) in &table {
        let n = reps.len().max(1) as f32;
        let mins: f32 = reps.iter().map(|r| r.pursuit_s).sum::<f32>() / 60.0;
        let fc: Vec<f32> = reps.iter().filter_map(|r| r.first_contact_s).collect();
        let fc_mean = fc.iter().sum::<f32>() / fc.len().max(1) as f32;
        let sum = |f: &dyn Fn(&RunReport) -> u32| reps.iter().map(f).sum::<u32>();
        let caps: Vec<f32> = reps.iter().filter_map(|r| r.captured_s).collect();
        println!(
            "| L{level} | {} | {} ({} of {}) | {:.2} | {} / {} | {} | {} of {} (mean {:.0} s) | {} | {:.2} | {:.2} | {} | {} |",
            reps.len(),
            if fc.is_empty() { "-".to_string() } else { format!("{fc_mean:.1}") },
            fc.len(),
            reps.len(),
            sum(&|r| r.contacts) as f32 / mins.max(1e-3),
            sum(&|r| r.pit_success),
            sum(&|r| r.pit_attempts),
            sum(&|r| r.rams),
            caps.len(),
            reps.len(),
            caps.iter().sum::<f32>() / caps.len().max(1) as f32,
            reps.iter().filter(|r| r.escaped_s.is_some()).count(),
            sum(&|r| r.wall_hits) as f32 / mins.max(1e-3),
            sum(&|r| r.prop_hits) as f32 / mins.max(1e-3),
            sum(&|r| r.pushes),
            sum(&|r| r.unit_wrecks),
        );
        let _ = n;
    }
}

/// Lateral offset the racer drives at from its route polyline, m (`WP_LANE_OFF`, default 0:
/// route polylines run along lane centres).
fn lane_off() -> f32 {
    std::env::var("WP_LANE_OFF")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0)
}
