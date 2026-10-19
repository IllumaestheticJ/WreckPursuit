//! Integration tests for the city, signals, traffic, density, violations, LOD and routes.
//!
//! Long runs are `#[ignore]`d; run them in release:
//! `cargo test -p wp_world --release -- --ignored --nocapture`.

use std::time::Instant;

use wp_contracts::{
    EntityId, SignalState, SimEvent, SurfaceType, TimeOfDay, ViolationKind, WorldQuery, PHYSICS_DT,
};
use wp_world::geom;
use wp_world::map::{CityMap, SegKind};
use wp_world::network::{DistrictKind, IntersectionControl, LaneKind, PoiKind, RoadNetwork};
use wp_world::procgen::{self, ProcGenConfig};
use wp_world::{ActiveRoute, Lod, LodChange, TrackedVehicle, TrafficConfig, World, WorldConfig};

fn data_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn world(traffic: TrafficConfig, hour: f32, ratio: f32) -> World {
    World::procedural(
        &ProcGenConfig::default(),
        WorldConfig {
            start_time: TimeOfDay::from_hours(hour),
            time_ratio: ratio,
            traffic,
            ..Default::default()
        },
    )
}

fn no_traffic() -> TrafficConfig {
    TrafficConfig {
        enabled: false,
        ..Default::default()
    }
}

fn run(w: &mut World, seconds: f32, ev: &mut Vec<SimEvent>) {
    for _ in 0..(seconds / PHYSICS_DT).round() as usize {
        w.step(PHYSICS_DT, ev);
    }
}

fn player(pos: [f32; 3], dir: [f32; 2], speed: f32) -> TrackedVehicle {
    TrackedVehicle {
        entity: EntityId(1),
        pos,
        vel: [dir[0] * speed, dir[1] * speed, 0.0],
        forward: dir,
        siren: false,
        player: true,
    }
}

// ---------------------------------------------------------------------------------------------
// Signals
// ---------------------------------------------------------------------------------------------

/// Rank of a movement under the current signal state (2 protected, 1 permissive left, 0 red).
fn rank(map: &CityMap, conn: u32) -> u8 {
    let SegKind::Connector { from, turn, .. } = map.seg(conn).kind else {
        return 0;
    };
    match map.lane_signal(from).map(|s| s.0) {
        Some(SignalState::Green) | Some(SignalState::Yellow) => {
            if turn == wp_world::network::TurnKind::Left {
                1
            } else {
                2
            }
        }
        _ => 0,
    }
}

#[test]
fn signals_never_show_conflicting_greens_and_honour_cycles() {
    let net = procgen::generate(&ProcGenConfig::default());
    let mut map = CityMap::new(net);
    let n_signals = map.net.signals.len();
    assert!(n_signals > 30);
    // Connectors at signalised junctions.
    let signal_conns: Vec<u32> = (map.n_lanes()..map.segs.len())
        .map(|i| i as u32)
        .filter(|&c| match map.seg(c).kind {
            SegKind::Connector { from, .. } => map.lane_signal(from).is_some(),
            _ => false,
        })
        .collect();
    let max_cycle = map
        .net
        .signals
        .iter()
        .map(|s| s.cycle_s())
        .fold(0.0f32, f32::max);
    let mut green_time = vec![vec![0.0f32; 8]; n_signals];
    let dt = 0.1f32;
    let steps = (2.0 * max_cycle / dt) as usize;
    for k in 0..steps {
        // 0.1 s = 24 physics ticks.
        map.signal_tick = k as u64 * 24;
        // 1. Conflicting movements are never both protected.
        for &c in &signal_conns {
            if rank(&map, c) < 2 {
                continue;
            }
            for &o in &map.seg(c).conflicts {
                let (SegKind::Connector { from: fc, .. }, SegKind::Connector { from: fo, .. }) =
                    (map.seg(c).kind, map.seg(o).kind)
                else {
                    continue;
                };
                // Merges within one approach (same signal group) are resolved first-come.
                if map.approach[fc as usize].map(|a| (a.intersection, a.group))
                    == map.approach[fo as usize].map(|a| (a.intersection, a.group))
                {
                    continue;
                }
                assert!(
                    rank(&map, o) < 2,
                    "conflicting protected movements {c} / {o} at tick {}",
                    map.signal_tick
                );
            }
        }
        // 2. Approaches with crossing headings are never green together.
        for (si, sc) in map.net.signals.iter().enumerate() {
            let ix = &map.net.intersections[sc.intersection as usize];
            let mut green_dirs: Vec<(u8, [f32; 2])> = Vec::new();
            for inc in &ix.incoming {
                let (st, _) = map.lane_signal(inc.lane).unwrap();
                if st == SignalState::Green || st == SignalState::Yellow {
                    let l = &map.net.lanes[inc.lane as usize];
                    let n = l.polyline.len();
                    let d = geom::normalize(geom::sub(
                        geom::xy(l.polyline[n - 1]),
                        geom::xy(l.polyline[n - 2]),
                    ));
                    green_dirs.push((inc.signal_group, d));
                }
            }
            for a in &green_dirs {
                for b in &green_dirs {
                    if a.0 != b.0 {
                        assert!(
                            geom::dot(a.1, b.1).abs() > 0.85,
                            "crossing approaches green together at signal {si}"
                        );
                    }
                }
            }
            if k as f32 * dt < sc.cycle_s() {
                for g in 0..sc.groups {
                    let st = wp_world::signals::group_state(sc, g, map.signal_tick).0;
                    if st == SignalState::Green {
                        green_time[si][g as usize] += dt;
                    }
                }
            }
        }
    }
    // 3. Cycle lengths honoured: each group's green per cycle equals its planned green
    //    (plus yellow+all-red of phases it spans), and state repeats every cycle.
    for (si, sc) in map.net.signals.iter().enumerate() {
        for g in 0..sc.groups {
            let mut planned = 0.0;
            let n = sc.phases.len();
            for (i, p) in sc.phases.iter().enumerate() {
                if p.green_groups.contains(&g) {
                    planned += p.green_s;
                    if n > 1 && sc.phases[(i + 1) % n].green_groups.contains(&g) {
                        planned += p.yellow_s + p.all_red_s;
                    }
                }
            }
            let measured = green_time[si][g as usize];
            assert!(
                (measured - planned).abs() <= 2.0 * dt + 1e-3,
                "signal {si} group {g}: green {measured} vs planned {planned}"
            );
            let cycle = wp_world::signals::ticks_from_secs(sc.cycle_s());
            for t in [792u64, 4296, 9840] {
                let a = wp_world::signals::group_state(sc, g, t).0;
                let b = wp_world::signals::group_state(sc, g, t + cycle).0;
                assert_eq!(a, b, "signal {si} not periodic");
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Format & data
// ---------------------------------------------------------------------------------------------

#[test]
fn road_format_round_trips() {
    let net = procgen::generate(&ProcGenConfig {
        nx: 6,
        ny: 6,
        ..Default::default()
    });
    let bin = net.to_bin();
    assert_eq!(&bin[0..4], b"WPRD");
    assert_eq!(RoadNetwork::from_bin(&bin).unwrap(), net);
    assert_eq!(RoadNetwork::from_json(&net.to_json()).unwrap(), net);
    let mut bad = bin.clone();
    bad[4] = 99;
    assert!(RoadNetwork::from_bin(&bad).is_err());
}

#[test]
fn shipped_procedural_city_is_current() {
    let dir = data_dir().join("city");
    let expected = procgen::generate(&ProcGenConfig::default());
    let bin = RoadNetwork::load(dir.join("procedural_12x12.road.bin"))
        .expect("data/city/procedural_12x12.road.bin (run `just city`)");
    assert!(
        bin == expected,
        "procedural_12x12.road.bin is stale: regenerate with `just city`"
    );
    let json = RoadNetwork::load(dir.join("procedural_12x12.road.json")).unwrap();
    assert!(json == expected, "procedural_12x12.road.json is stale");
    let glb = std::fs::read(dir.join("procedural_12x12.greybox.glb")).unwrap();
    assert_eq!(&glb[0..4], b"glTF");
}

// ---------------------------------------------------------------------------------------------
// WorldQuery
// ---------------------------------------------------------------------------------------------

#[test]
fn world_query_surfaces_limits_and_signals() {
    let w = world(no_traffic(), 12.0, 10.0);
    let net = w.network();
    // A local street lane (with parking on its right) and an arterial lane.
    let local = net
        .lanes
        .iter()
        .find(|l| l.kind == LaneKind::Through && (l.speed_limit_mps - 11.176).abs() < 0.01)
        .unwrap();
    let mid = local.polyline[0];
    let mid = [
        (local.polyline[0][0] + local.polyline[1][0]) * 0.5,
        (local.polyline[0][1] + local.polyline[1][1]) * 0.5,
        mid[2],
    ];
    assert_eq!(w.surface_at(mid), SurfaceType::Asphalt);
    let lim = w.speed_limit_at(mid).unwrap();
    assert!((lim - local.speed_limit_mps).abs() < 1e-3);
    // Highway lanes are concrete, limit 55 mph.
    let hw = net
        .lanes
        .iter()
        .find(|l| l.kind == LaneKind::Highway)
        .unwrap();
    let p = hw.polyline[0];
    let d = geom::normalize(geom::sub(geom::xy(hw.polyline[1]), geom::xy(p)));
    let p = [p[0] + d[0] * 10.0, p[1] + d[1] * 10.0, 0.0];
    assert_eq!(w.surface_at(p), SurfaceType::Concrete);
    assert!((w.speed_limit_at(p).unwrap() - 24.587).abs() < 0.01);
    // Curb just outside the right edge of an arterial lane without parking.
    let art = net
        .lanes
        .iter()
        .find(|l| l.kind == LaneKind::Through && l.edge_right && l.right.is_none())
        .unwrap();
    let a0 = geom::xy(art.polyline[0]);
    let a1 = geom::xy(art.polyline[1]);
    let dir = geom::normalize(geom::sub(a1, a0));
    let r = geom::right_of(dir);
    let m = geom::scale(geom::add(a0, a1), 0.5);
    let off = art.width_m * 0.5 + 0.2;
    assert_eq!(
        w.surface_at([m[0] + r[0] * off, m[1] + r[1] * off, 0.0]),
        SurfaceType::Curb
    );
    // Off-road: block interiors are grass (residential) or concrete (core/commercial).
    assert_eq!(w.surface_at([-440.0, 330.0, 0.0]), SurfaceType::Grass);
    assert_eq!(w.surface_at([110.0, 30.0, 0.0]), SurfaceType::Concrete);
    assert_eq!(w.surface_at([440.0, -330.0, 0.0]), SurfaceType::Dirt);
    assert_eq!(w.speed_limit_at([-440.0, 330.0, 0.0]), None);
    // Signal ahead agrees with the controller for a signalised approach.
    let map = w.map();
    let (li, lane) = net
        .lanes
        .iter()
        .enumerate()
        .find(|(i, _)| map.lane_signal(*i as u32).is_some())
        .unwrap();
    let n = lane.polyline.len();
    let end = lane.polyline[n - 1];
    let d = geom::normalize(geom::sub(geom::xy(end), geom::xy(lane.polyline[n - 2])));
    let p = [end[0] - d[0] * 5.0, end[1] - d[1] * 5.0, 0.0];
    assert_eq!(
        w.signal_state_ahead(p, [d[0], d[1], 0.0]),
        map.lane_signal(li as u32).map(|s| s.0)
    );
    assert_eq!(w.time_of_day().hours(), 12.0);
}

// ---------------------------------------------------------------------------------------------
// Traffic safety
// ---------------------------------------------------------------------------------------------

fn safety_run(minutes: f32, aggressive: f32) -> (u64, u64, u64, usize) {
    let mut w = world(
        TrafficConfig {
            max_agents: 400,
            density_scale: 4.0,
            detect_collisions: true,
            aggressive_fraction: aggressive,
            ..Default::default()
        },
        12.0,
        1.0,
    );
    w.populate();
    let mut ev = Vec::new();
    let mut min_agents = usize::MAX;
    let ticks = (minutes * 60.0 / PHYSICS_DT) as usize;
    for i in 0..ticks {
        w.step(PHYSICS_DT, &mut ev);
        if i > 240 * 10 {
            min_agents = min_agents.min(w.agent_count());
        }
    }
    let s = w.traffic_stats().clone();
    let moving = w.agents().filter(|a| a.speed_mps > 0.5).count();
    println!(
        "{minutes} min, aggressive {aggressive}: {s:?}, moving {moving}/{}",
        w.agent_count()
    );
    assert!(min_agents >= 380, "population collapsed to {min_agents}");
    assert!(
        moving * 2 > w.agent_count(),
        "traffic is jammed: only {moving} moving"
    );
    (
        s.civilian_collisions,
        s.collisions_involving_aggressive,
        s.spawned,
        w.agent_count(),
    )
}

#[test]
fn traffic_400_agents_one_minute_without_collisions() {
    let (c, _, _, n) = safety_run(1.0, 0.0);
    assert_eq!(c, 0, "civilian-civilian collisions");
    assert!(n >= 390);
}

#[test]
#[ignore = "10 minutes of sim; run with --release"]
fn traffic_400_agents_ten_minutes_without_collisions() {
    let (c, _, _, _) = safety_run(10.0, 0.0);
    assert_eq!(c, 0, "civilian-civilian collisions");
}

#[test]
#[ignore = "10 minutes of sim; run with --release"]
fn traffic_with_aggressive_drivers_ten_minutes_under_one_percent() {
    let (c, ca, spawned, _) = safety_run(10.0, 0.05);
    let rate = c as f64 / spawned as f64;
    println!("aggressive enabled: {c} collisions ({ca} involving aggressive) over {spawned} agents = {:.4} %", rate * 100.0);
    assert!(rate < 0.01);
}

// ---------------------------------------------------------------------------------------------
// Density
// ---------------------------------------------------------------------------------------------

fn by_district(w: &World) -> Vec<(DistrictKind, f32, u32)> {
    let (targets, counts) = w.zone_density();
    DistrictKind::ALL
        .iter()
        .map(|k| {
            let mut t = 0.0;
            let mut c = 0;
            for (z, zone) in w.network().zones.iter().enumerate() {
                if zone.kind == *k {
                    t += targets[z];
                    c += counts[z];
                }
            }
            (*k, t, c)
        })
        .collect()
}

#[test]
fn density_tracks_targets_after_warm_up() {
    let mut w = world(
        TrafficConfig {
            max_agents: 400,
            ..Default::default()
        },
        8.0,
        1.0,
    );
    w.populate();
    let mut ev = Vec::new();
    run(&mut w, 45.0, &mut ev);
    let mut checked = 0;
    for (k, t, c) in by_district(&w) {
        println!("{k:?}: target {t:.1} count {c}");
        if t >= 20.0 {
            checked += 1;
            let err = (c as f32 - t).abs() / t;
            assert!(err <= 0.15, "{k:?}: count {c} vs target {t:.1}");
        }
    }
    assert!(checked >= 3);
}

#[test]
fn density_noon_core_at_least_five_times_3am() {
    let cfg = TrafficConfig {
        max_agents: 5000,
        ..Default::default()
    };
    let core = |hour: f32| -> (f32, u32) {
        let mut w = world(cfg.clone(), hour, 1.0);
        w.populate();
        let r = by_district(&w);
        let c = r.iter().find(|r| r.0 == DistrictKind::Core).unwrap();
        (c.1, c.2)
    };
    let (t_noon, c_noon) = core(12.0);
    let (t_night, c_night) = core(3.0);
    println!(
        "core noon target {t_noon:.1} count {c_noon}, 03:00 target {t_night:.1} count {c_night}"
    );
    assert!(t_noon >= 5.0 * t_night);
    assert!(c_noon as f32 >= 5.0 * c_night.max(1) as f32);
}

#[test]
fn spawns_respect_sightline_and_active_area() {
    let mut w = world(TrafficConfig::default(), 12.0, 1.0);
    let me = player([0.0, -330.0, 0.0], [1.0, 0.0], 0.0);
    w.set_tracked_vehicles(&[me]);
    w.populate();
    assert!(w.agent_count() > 20);
    for a in w.agents() {
        let d = geom::dist(geom::xy(a.pos), [0.0, -330.0]);
        assert!(d >= 150.0, "agent spawned in sight at {d} m");
        assert!(d <= 400.0 + 60.0, "agent outside the active area at {d} m");
    }
    // Teleport far away: the old area empties (nobody sees it), the new one fills.
    let mut ev = Vec::new();
    let far = player([0.0, 650.0, 0.0], [1.0, 0.0], 0.0);
    w.set_tracked_vehicles(&[far]);
    run(&mut w, 10.0, &mut ev);
    let near_old = w
        .agents()
        .filter(|a| geom::dist(geom::xy(a.pos), [0.0, -330.0]) < 200.0)
        .count();
    assert_eq!(near_old, 0);
    assert!(w
        .agents()
        .any(|a| geom::dist(geom::xy(a.pos), [0.0, 650.0]) < 460.0));
}

// ---------------------------------------------------------------------------------------------
// Determinism & perf
// ---------------------------------------------------------------------------------------------

fn scripted_world(seed: u64) -> World {
    let mut w = World::procedural(
        &ProcGenConfig::default(),
        WorldConfig {
            seed,
            traffic: TrafficConfig {
                density_scale: 3.0,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let car = wp_contracts::load_car(data_dir().join("cars/debug_sedan.json")).unwrap();
    w.set_traffic_car(&car);
    w
}

fn scripted_run(w: &mut World, seconds: f32) -> Vec<u64> {
    let mut ev = Vec::new();
    let mut hashes = Vec::new();
    let ticks = (seconds / PHYSICS_DT) as usize;
    for i in 0..ticks {
        // A player cruising east along a row with the siren on half the time.
        let t = i as f32 * PHYSICS_DT;
        let mut p = player([-600.0 + 15.0 * t, -332.0, 0.0], [1.0, 0.0], 15.0);
        p.siren = (i / 1200) % 2 == 1;
        w.set_tracked_vehicles(&[p]);
        if i == 0 {
            w.populate();
        }
        w.step(PHYSICS_DT, &mut ev);
        if i % 480 == 0 {
            hashes.push(w.state_hash());
        }
    }
    hashes
}

#[test]
fn determinism_hash() {
    let mut a = scripted_world(7);
    let mut b = scripted_world(7);
    let ha = scripted_run(&mut a, 20.0);
    let hb = scripted_run(&mut b, 20.0);
    assert_eq!(ha, hb);
    assert!(a.traffic_stats().promotions > 0, "physical LOD exercised");
    let mut c = scripted_world(8);
    assert_ne!(scripted_run(&mut c, 20.0), ha);
}

#[test]
#[ignore = "perf: run with --release"]
fn perf_400_agents_under_budget() {
    let mut w = world(
        TrafficConfig {
            max_agents: 400,
            density_scale: 4.0,
            ..Default::default()
        },
        12.0,
        1.0,
    );
    w.populate();
    let mut ev = Vec::new();
    run(&mut w, 5.0, &mut ev);
    let frames = 60 * 30;
    let t0 = Instant::now();
    for _ in 0..frames * 4 {
        w.step(PHYSICS_DT, &mut ev);
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0 / frames as f64;
    println!("{} agents: {ms:.3} ms per 60 Hz frame", w.agent_count());
    assert!(w.agent_count() >= 390);
    assert!(ms < 2.0, "{ms} ms per frame over the 2 ms budget");
}

// ---------------------------------------------------------------------------------------------
// Reactions
// ---------------------------------------------------------------------------------------------

#[test]
fn civilians_stop_behind_a_stationary_player_and_yield_to_sirens() {
    let mut w = world(
        TrafficConfig {
            density_scale: 4.0,
            sight_radius_m: 0.0,
            ..Default::default()
        },
        12.0,
        1.0,
    );
    w.populate();
    let mut ev = Vec::new();
    run(&mut w, 2.0, &mut ev);
    // Park a (non-player) police car with siren in front of the fastest mid-lane agent.
    let a = w
        .agents()
        .filter(|a| a.speed_mps > 8.0)
        .max_by(|x, y| x.speed_mps.total_cmp(&y.speed_mps))
        .unwrap();
    let f = geom::dir_of_yaw(a.yaw);
    let obstacle = TrackedVehicle {
        entity: EntityId(9),
        pos: [a.pos[0] + f[0] * 45.0, a.pos[1] + f[1] * 45.0, a.pos[2]],
        vel: [0.0; 3],
        forward: f,
        siren: false,
        player: false,
    };
    w.set_tracked_vehicles(&[obstacle]);
    run(&mut w, 12.0, &mut ev);
    let after = w.agents().find(|x| x.id == a.id);
    if let Some(after) = after {
        let d = geom::dist(geom::xy(after.pos), geom::xy(obstacle.pos));
        assert!(d > 4.4, "civilian drove into the stopped car ({d} m)");
    }
    // Siren: nearby agents start yielding and slow down.
    let mut cop = obstacle;
    cop.siren = true;
    w.set_tracked_vehicles(&[cop]);
    run(&mut w, 4.0, &mut ev);
    let near: Vec<_> = w
        .agents()
        .filter(|x| geom::dist(geom::xy(x.pos), geom::xy(cop.pos)) < 40.0)
        .collect();
    assert!(!near.is_empty());
    assert!(near.iter().all(|x| x.yielding));
    // Fast highway cars may still be braking; on average everyone near has slowed right down.
    let mean = near.iter().map(|x| x.speed_mps).sum::<f32>() / near.len() as f32;
    assert!(mean < 5.0, "{near:?}");
}

#[test]
fn head_on_threat_triggers_emergency_braking() {
    let mut w = world(
        TrafficConfig {
            density_scale: 4.0,
            ..Default::default()
        },
        12.0,
        1.0,
    );
    w.populate();
    let mut ev = Vec::new();
    run(&mut w, 2.0, &mut ev);
    let a = w.agents().find(|a| a.speed_mps > 8.0).unwrap();
    let f = geom::dir_of_yaw(a.yaw);
    // A car coming straight at it at 25 m/s from 60 m.
    let start = [a.pos[0] + f[0] * 60.0, a.pos[1] + f[1] * 60.0, a.pos[2]];
    for k in 0..60 {
        let t = k as f32 / 60.0;
        let p = [
            start[0] - f[0] * 25.0 * t,
            start[1] - f[1] * 25.0 * t,
            start[2],
        ];
        let tv = TrackedVehicle {
            entity: EntityId(5),
            pos: p,
            vel: [-f[0] * 25.0, -f[1] * 25.0, 0.0],
            forward: [-f[0], -f[1]],
            siren: false,
            player: false,
        };
        w.set_tracked_vehicles(&[tv]);
        run(&mut w, 1.0 / 60.0, &mut ev);
    }
    assert!(w.traffic_stats().emergency_brakes > 0);
}

// ---------------------------------------------------------------------------------------------
// Violations
// ---------------------------------------------------------------------------------------------

fn violations(ev: &[SimEvent]) -> Vec<ViolationKind> {
    ev.iter()
        .filter_map(|e| match e {
            SimEvent::TrafficViolation { kind, .. } => Some(*kind),
            _ => None,
        })
        .collect()
}

/// Drive a tracked vehicle along `lane` (from arc length `s0`, `dir_sign` = -1 to drive it
/// backwards) at `speed` for `seconds`.
fn drive_lane(
    w: &mut World,
    lane: u32,
    s0: f32,
    speed: f32,
    seconds: f32,
    dir_sign: f32,
) -> Vec<SimEvent> {
    let mut ev = Vec::new();
    let ticks = (seconds / PHYSICS_DT) as usize;
    for i in 0..ticks {
        let s = s0 + dir_sign * speed * i as f32 * PHYSICS_DT;
        let seg = w.map().seg(lane);
        let (p, t) = seg.point_at(s);
        let f = geom::scale(t, dir_sign);
        w.set_tracked_vehicles(&[player(p, f, speed)]);
        w.step(PHYSICS_DT, &mut ev);
    }
    ev
}

#[test]
fn speeding_and_wrong_way_are_detected() {
    let mut w = world(no_traffic(), 12.0, 10.0);
    let lane = (0..w.network().lanes.len() as u32)
        .find(|&l| {
            let ln = &w.network().lanes[l as usize];
            ln.kind == LaneKind::Through && w.map().seg(l).length > 70.0
        })
        .unwrap();
    let limit = w.network().lanes[lane as usize].speed_limit_mps;
    let ev = drive_lane(&mut w, lane, 5.0, limit + 2.0, 2.0, 1.0);
    assert!(violations(&ev).is_empty(), "within margin is fine");
    let ev = drive_lane(&mut w, lane, 5.0, limit + 8.0, 2.0, 1.0);
    let v = violations(&ev);
    assert!(
        v.iter()
            .any(|k| matches!(k, ViolationKind::Speeding { over_mps } if *over_mps > 7.0)),
        "{v:?}"
    );
    let mut w = world(no_traffic(), 12.0, 10.0);
    let len = w.map().seg(lane).length;
    let ev = drive_lane(&mut w, lane, len - 5.0, 8.0, 3.0, -1.0);
    assert!(violations(&ev).contains(&ViolationKind::WrongWay));
}

#[test]
fn red_light_running_is_detected_and_green_is_not() {
    let mut w = world(no_traffic(), 12.0, 10.0);
    let map = w.map();
    let lane = (0..map.n_lanes() as u32)
        .find(|&l| map.lane_signal(l).is_some() && map.seg(l).length > 60.0)
        .unwrap();
    let len = w.map().seg(lane).length;
    let mut ev = Vec::new();
    // Wait for red, then cross the line.
    while w.map().lane_signal(lane).unwrap().0 != SignalState::Red {
        w.step(PHYSICS_DT, &mut ev);
    }
    // Make sure enough red remains to cross.
    while w.map().lane_signal(lane).unwrap().1 < 8.0 {
        w.step(PHYSICS_DT, &mut ev);
        while w.map().lane_signal(lane).unwrap().0 != SignalState::Red {
            w.step(PHYSICS_DT, &mut ev);
        }
    }
    let crossed = drive_lane(&mut w, lane, len - 40.0, 12.0, 4.5, 1.0);
    assert!(violations(&crossed).contains(&ViolationKind::RedLight));
    // On green: no violation.
    while w.map().lane_signal(lane).unwrap().0 != SignalState::Green
        || w.map().lane_signal(lane).unwrap().1 < 8.0
    {
        w.step(PHYSICS_DT, &mut ev);
    }
    let ev = drive_lane(&mut w, lane, len - 40.0, 12.0, 4.5, 1.0);
    assert!(!violations(&ev).contains(&ViolationKind::RedLight));
}

#[test]
fn hitting_a_civilian_is_a_collision_violation() {
    let mut w = world(
        TrafficConfig {
            density_scale: 3.0,
            sight_radius_m: 0.0,
            ..Default::default()
        },
        12.0,
        1.0,
    );
    w.populate();
    let a = w.agents().next().unwrap();
    let mut ev = Vec::new();
    w.set_tracked_vehicles(&[player(a.pos, geom::dir_of_yaw(a.yaw), 0.0)]);
    w.step(PHYSICS_DT, &mut ev);
    for _ in 0..3 {
        w.step(PHYSICS_DT, &mut ev);
    }
    assert!(violations(&ev).contains(&ViolationKind::Collision));
}

// ---------------------------------------------------------------------------------------------
// LOD
// ---------------------------------------------------------------------------------------------

#[test]
fn lod_promotion_and_demotion_are_seamless() {
    let mut w = scripted_world(3);
    let me = player([110.0, -332.0, 0.0], [1.0, 0.0], 0.0);
    w.set_tracked_vehicles(&[me]);
    w.populate();
    let mut ev = Vec::new();
    let mut promoted = 0;
    let mut max_jump = 0.0f32;
    let mut physical_on_road = true;
    for i in 0..(40.0 / PHYSICS_DT) as usize {
        let before: Vec<_> = w.agents().collect();
        // Leave after 25 s so physical agents demote.
        if i == (25.0 / PHYSICS_DT) as usize {
            w.set_tracked_vehicles(&[player([5000.0, 5000.0, 0.0], [1.0, 0.0], 0.0)]);
        }
        w.step(PHYSICS_DT, &mut ev);
        for ch in w.drain_lod_changes() {
            let id = match ch {
                LodChange::Promoted(id) => {
                    promoted += 1;
                    id
                }
                LodChange::Demoted(id) | LodChange::Despawned(id) => id,
            };
            if let (Some(b), Some(a)) = (
                before.iter().find(|x| x.id == id),
                w.agents().find(|x| x.id == id),
            ) {
                let jump = geom::dist(geom::xy(a.pos), geom::xy(b.pos)) - b.speed_mps * PHYSICS_DT;
                max_jump = max_jump.max(jump);
                assert!((a.speed_mps - b.speed_mps).abs() < 0.5);
            }
        }
        if i % 240 == 0 {
            for a in w.agents().filter(|a| a.lod == Lod::Physical) {
                let f = geom::dir_of_yaw(a.yaw);
                if w.map().locate(a.pos, Some(f), 1.5, true, true).is_none() {
                    physical_on_road = false;
                }
            }
        }
    }
    println!(
        "promotions {promoted}, max pose jump {max_jump:.3} m, stats {:?}",
        w.traffic_stats()
    );
    assert!(promoted > 0);
    assert!(w.traffic_stats().demotions > 0);
    assert!(max_jump < 0.6, "pose jump {max_jump} m at LOD change");
    assert!(physical_on_road, "physical traffic left the road");
    for a in w.agents().filter(|a| a.lod == Lod::Physical) {
        println!("still physical: {a:?}");
    }
    assert_eq!(w.agents().filter(|a| a.lod == Lod::Physical).count(), 0);
}

// ---------------------------------------------------------------------------------------------
// Routes & props
// ---------------------------------------------------------------------------------------------

#[test]
fn routes_between_pois_and_circuits() {
    let mut w = world(TrafficConfig::default(), 12.0, 1.0);
    let pois = &w.network().pois;
    let start = pois
        .iter()
        .position(|p| p.kind == PoiKind::RaceStartGrid)
        .unwrap();
    let finish = pois.iter().position(|p| p.kind == PoiKind::Garage).unwrap();
    let r = w.plan_route_between_pois(start, finish).expect("route");
    assert!(r.length_m > 100.0);
    assert!(!r.closed_loop);
    assert!(r.checkpoints.len() >= 2);
    for c in r.checkpoints.windows(2) {
        assert!(c[1].distance_m > c[0].distance_m);
    }
    // Lane-legal: every consecutive pair is connected.
    for s in r.segs.windows(2) {
        assert!(w.map().seg(s[0]).next.contains(&s[1]));
    }
    // Circuit around four core blocks.
    let c = w
        .plan_circuit(&[
            [-220.0, -220.0],
            [220.0, -220.0],
            [220.0, 220.0],
            [-220.0, 220.0],
        ])
        .expect("circuit");
    assert!(c.closed_loop);
    assert!(c.length_m > 1500.0);
    let first = c.polyline[0];
    let last = *c.polyline.last().unwrap();
    assert!(geom::dist(geom::xy(first), geom::xy(last)) < 1.0);
    for s in c.segs.windows(2) {
        assert!(w.map().seg(s[0]).next.contains(&s[1]));
    }
    // Closure: traffic suppressed on the route, cones at side streets.
    let closure = wp_world::routes::closure_for_route(w.map(), &c);
    assert!(c.lanes.iter().all(|l| closure.closed_lanes.contains(l)));
    assert!(closure.cones.len() >= 6);
    let parked = w.static_props().count();
    w.set_active_routes(vec![ActiveRoute {
        route: c.clone(),
        closed: true,
    }]);
    assert!(w.static_props().count() > parked);
    w.populate();
    let mut ev = Vec::new();
    run(&mut w, 2.0, &mut ev);
    for a in w.agents() {
        let f = geom::dir_of_yaw(a.yaw);
        if let Some(l) = w.map().locate(a.pos, Some(f), 0.0, true, false) {
            assert!(
                !closure.closed_lanes.contains(&l.seg),
                "agent {} on a closed lane",
                a.id
            );
        }
    }
}

#[test]
fn parked_cars_line_parking_lanes() {
    let w = world(no_traffic(), 12.0, 1.0);
    let parked: Vec<_> = w
        .static_props()
        .filter(|p| p.kind == wp_world::PropKind::ParkedCar)
        .collect();
    assert!(parked.len() > 500, "{}", parked.len());
    for p in parked.iter().take(50) {
        let loc = w.map().locate(p.pos, None, 0.3, false, false).unwrap();
        let lane = w.map().seg(loc.seg).lane().unwrap();
        assert_eq!(w.network().lanes[lane as usize].kind, LaneKind::Parking);
    }
    assert!(w
        .network()
        .intersections
        .iter()
        .any(|i| matches!(i.control, IntersectionControl::Signal { .. })));
}

// ---------------------------------------------------------------------------------------------
// Debris hazards
// ---------------------------------------------------------------------------------------------

/// Run a busy city for `seconds` with `hazards`; returns (agent–debris overlaps, lane changes,
/// moving fraction at the end).
fn debris_run(hazards: &[wp_world::Hazard], seconds: f32) -> (usize, u64, f32) {
    let mut w = world(
        TrafficConfig {
            density_scale: 4.0,
            sight_radius_m: 0.0,
            ..Default::default()
        },
        12.0,
        1.0,
    );
    w.set_hazards(hazards);
    w.populate();
    let mut ev = Vec::new();
    let mut overlaps = 0;
    for i in 0..(seconds / PHYSICS_DT) as usize {
        w.step(PHYSICS_DT, &mut ev);
        if i % 4 != 0 {
            continue;
        }
        for a in w.agents() {
            let car = geom::Obb {
                c: geom::xy(a.pos),
                f: geom::dir_of_yaw(a.yaw),
                hl: 2.25,
                hw: 0.95,
            };
            for h in hazards {
                let d = geom::Obb {
                    c: geom::xy(h.pos),
                    f: [0.0, 1.0],
                    hl: h.radius_m,
                    hw: h.radius_m,
                };
                if geom::obb_overlap(&car, &d) {
                    overlaps += 1;
                }
            }
        }
    }
    let moving = w.agents().filter(|a| a.speed_mps > 0.5).count() as f32 / w.agent_count() as f32;
    (overlaps, w.traffic_stats().lane_changes, moving)
}

/// A point on lane `lane` at the middle, offset `lat` to the right.
fn lane_point(net: &RoadNetwork, lane: usize, lat: f32) -> [f32; 3] {
    let l = &net.lanes[lane];
    let (a, b) = (
        geom::xy(l.polyline[0]),
        geom::xy(*l.polyline.last().unwrap()),
    );
    let d = geom::normalize(geom::sub(b, a));
    let r = geom::right_of(d);
    let m = geom::scale(geom::add(a, b), 0.5);
    [m[0] + r[0] * lat, m[1] + r[1] * lat, l.polyline[0][2]]
}

#[test]
fn debris_blocking_a_lane_is_passed_in_the_next_lane_without_contact() {
    let net = procgen::generate(&ProcGenConfig::default());
    // Arterial lanes (two per direction) on long straight roads: block the right lane.
    let lanes: Vec<usize> = net
        .lanes
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            l.kind == LaneKind::Through
                && l.left.is_some()
                && l.right.is_none()
                && geom::dist(
                    geom::xy(l.polyline[0]),
                    geom::xy(*l.polyline.last().unwrap()),
                ) > 70.0
        })
        .map(|(i, _)| i)
        .take(6)
        .collect();
    assert!(lanes.len() >= 4);
    let hazards: Vec<_> = lanes
        .iter()
        .map(|&l| wp_world::Hazard::new(lane_point(&net, l, 0.0), 1.2))
        .collect();
    let (overlaps, changes, moving) = debris_run(&hazards, 60.0);
    println!("blocked lanes: overlaps {overlaps}, lane changes {changes}, moving {moving:.2}");
    assert_eq!(overlaps, 0, "a civilian drove into debris");
    assert!(changes > 0);
    assert!(moving > 0.5, "traffic jammed behind debris");
}

#[test]
fn small_debris_is_swerved_around_and_junction_debris_is_avoided() {
    let net = procgen::generate(&ProcGenConfig::default());
    // Small debris near the right edge of single-lane local streets: swerve, keep moving.
    let mut hazards: Vec<_> = net
        .lanes
        .iter()
        .enumerate()
        .filter(|(_, l)| l.kind == LaneKind::Through && l.left.is_none() && l.right.is_none())
        .step_by(37)
        .take(8)
        .map(|(i, _)| wp_world::Hazard::new(lane_point(&net, i, 1.2), 0.3))
        .collect();
    // Debris in the middle of a few signalised junction boxes.
    for ix in net
        .intersections
        .iter()
        .filter(|i| matches!(i.control, IntersectionControl::Signal { .. }))
        .step_by(9)
        .take(4)
    {
        hazards.push(wp_world::Hazard::new(net.nodes[ix.node as usize].pos, 0.8));
    }
    let (overlaps, _, moving) = debris_run(&hazards, 60.0);
    println!("swerve + junction debris: overlaps {overlaps}, moving {moving:.2}");
    assert_eq!(overlaps, 0, "a civilian drove into debris");
    assert!(moving > 0.5);
}
