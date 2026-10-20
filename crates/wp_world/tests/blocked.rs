//! Blocked-vehicle behaviour (owner playtest row 32): civilians go around or turn around when
//! a stationary vehicle blocks their lane, without touching it or each other.
//!
//! `cargo test -p wp_world --release --test blocked -- --nocapture` prints the numbers quoted in
//! `docs/integration/world.md`.

use std::collections::BTreeMap;

use wp_contracts::{EntityId, TimeOfDay, PHYSICS_DT};
use wp_world::builder::{self, BuildOptions, NodeSpec, RoadClass, RoadSpec};
use wp_world::geom::{self, Obb, V2, V3};
use wp_world::network::{DistrictKind, LaneKind, RoadNetwork, Zone};
use wp_world::procgen::ProcGenConfig;
use wp_world::vesper::VesperConfig;
use wp_world::{TrackedVehicle, TrafficConfig, TrafficStats, World, WorldConfig};

fn cfg(density: f32) -> WorldConfig {
    WorldConfig {
        start_time: TimeOfDay::from_hours(12.0),
        time_ratio: 1.0,
        traffic: TrafficConfig {
            density_scale: density,
            sight_radius_m: 0.0,
            detect_collisions: true,
            ..Default::default()
        },
        parked_cars: false,
        ..Default::default()
    }
}

/// A stopped car (tracked, not a player) parked on the centre of lane `lane` at arc length
/// `at`, facing along it (`across`: turned 90°, as if parked across the lane).
fn blocker_on(
    net: &RoadNetwork,
    lane: usize,
    at: f32,
    across: bool,
    player: bool,
) -> TrackedVehicle {
    let pts = &net.lanes[lane].polyline;
    let cum = geom::cumulative(pts);
    let (p, t) = geom::point_at(pts, &cum, at);
    TrackedVehicle {
        entity: EntityId(77),
        pos: p,
        vel: [0.0; 3],
        forward: if across { geom::right_of(t) } else { t },
        siren: false,
        player,
    }
}

fn body(c: V2, f: V2) -> Obb {
    Obb {
        c,
        f,
        hl: 2.25,
        hw: 0.95,
    }
}

/// What happened around a blocker.
#[derive(Debug)]
struct BlockRun {
    /// Agent–blocker body overlaps (samples at 60 Hz).
    contacts: usize,
    /// New civilian–civilian overlap pairs.
    collisions: u64,
    /// Agents that stood behind the blocker (within 30 m, in its lane band, stopped).
    queued: usize,
    /// Of those, agents that later got away (passed it, turned, reversed, re-routed).
    resolved: usize,
    /// Longest time an agent that got away had stood behind it, s.
    max_wait_s: f32,
    /// Agents still standing behind it at the end.
    still_waiting: usize,
    stats: TrafficStats,
}

/// Run `w` for `seconds` with `blocker` parked; `lane_pts` is the blocked lane.
fn blocked_run(w: &mut World, blocker: TrackedVehicle, lane_pts: &[V3], seconds: f32) -> BlockRun {
    let cum = geom::cumulative(lane_pts);
    let b_box = Obb {
        hl: 2.3,
        hw: 1.0,
        ..body(geom::xy(blocker.pos), geom::normalize(blocker.forward))
    };
    let b_s = geom::project(lane_pts, &cum, geom::xy(blocker.pos)).s;
    w.set_tracked_vehicles(&[blocker]);
    let mut ev = Vec::new();
    let mut contacts = 0;
    let mut since: BTreeMap<u32, f32> = BTreeMap::new();
    let mut queued_ids: BTreeMap<u32, ()> = BTreeMap::new();
    let mut resolved = 0;
    let mut max_wait = 0.0f32;
    let steps = (seconds / PHYSICS_DT) as usize;
    for k in 0..steps {
        w.set_tracked_vehicles(&[blocker]);
        w.step(PHYSICS_DT, &mut ev);
        if k % 4 != 0 {
            continue;
        }
        let t = k as f32 * PHYSICS_DT;
        let mut behind_now: BTreeMap<u32, ()> = BTreeMap::new();
        for a in w.agents() {
            let f = geom::dir_of_yaw(a.yaw);
            if geom::obb_overlap(&body(geom::xy(a.pos), f), &b_box) {
                contacts += 1;
            }
            let pr = geom::project(lane_pts, &cum, geom::xy(a.pos));
            let (_, lt) = geom::point_at(lane_pts, &cum, pr.s);
            let in_band = pr.dist < 1.2 && geom::dot(f, lt) > 0.8;
            if in_band && pr.s < b_s && pr.s > b_s - 30.0 && a.speed_mps < 0.5 {
                behind_now.insert(a.id, ());
                since.entry(a.id).or_insert(t);
                queued_ids.insert(a.id, ());
            }
        }
        // Agents that were queued and are no longer behind it (or gone) got away.
        let gone: Vec<u32> = since
            .keys()
            .copied()
            .filter(|id| !behind_now.contains_key(id))
            .collect();
        for id in gone {
            let t0 = since.remove(&id).unwrap();
            // Only count it if it left the queue for good (not just crept forward a car length).
            let still_behind = w.agents().any(|a| {
                if a.id != id {
                    return false;
                }
                let pr = geom::project(lane_pts, &cum, geom::xy(a.pos));
                pr.dist < 1.2 && pr.s < b_s && pr.s > b_s - 30.0
            });
            if still_behind {
                since.insert(id, t0);
                continue;
            }
            resolved += 1;
            max_wait = max_wait.max(t - t0);
        }
    }
    BlockRun {
        contacts,
        collisions: w.traffic_stats().civilian_collisions,
        queued: queued_ids.len(),
        resolved,
        max_wait_s: max_wait,
        still_waiting: since.len(),
        stats: w.traffic_stats().clone(),
    }
}

fn len_of(net: &RoadNetwork, lane: usize) -> f32 {
    *geom::cumulative(&net.lanes[lane].polyline).last().unwrap()
}

/// Lanes of two-lane (1+1) streets at least `min_len` long with an opposing lane.
fn two_lane_streets(net: &RoadNetwork, min_len: f32) -> Vec<usize> {
    net.lanes
        .iter()
        .enumerate()
        .filter(|(i, l)| {
            l.kind == LaneKind::Through
                && l.left.is_none()
                && l.right.is_none()
                && l.speed_limit_mps < 15.0
                && len_of(net, *i) >= min_len
                && net.lanes.iter().any(|o| {
                    o.road == l.road && o.from_node == l.to_node && o.to_node == l.from_node
                })
        })
        .map(|(i, _)| i)
        .collect()
}

fn report(name: &str, r: &BlockRun) {
    println!(
        "{name}: contacts {}, civilian collisions {}, queued {}, got away {}, max wait {:.1} s, \
         still waiting {}; passes {} (aborted {}), go-around lane changes {}, turnarounds {} \
         ({} done), reverses {}, reroutes {}",
        r.contacts,
        r.collisions,
        r.queued,
        r.resolved,
        r.max_wait_s,
        r.still_waiting,
        r.stats.oncoming_passes,
        r.stats.pass_aborts,
        r.stats.go_around_lane_changes,
        r.stats.turnarounds,
        r.stats.turnarounds_completed,
        r.stats.reverses,
        r.stats.reroutes
    );
}

/// The busiest straight two-lane street in the 12×12 grid: block one lane mid-block.
fn grid_two_lane(density: f32, across: bool, player: bool) -> (World, TrackedVehicle, Vec<V3>) {
    let mut w = World::procedural(&ProcGenConfig::default(), cfg(density));
    let net = w.network().clone();
    let lanes = two_lane_streets(&net, 90.0);
    // Near the core (busy) but not at it.
    let lane = *lanes
        .iter()
        .min_by(|&&a, &&b| {
            let d = |l: usize| {
                let p = net.lanes[l].polyline[0];
                (geom::len(geom::xy(p)) - 250.0).abs()
            };
            d(a).total_cmp(&d(b)).then(a.cmp(&b))
        })
        .unwrap();
    let at = len_of(&net, lane) * 0.6;
    let b = blocker_on(&net, lane, at, across, player);
    w.set_tracked_vehicles(&[b]);
    w.populate();
    (w, b, net.lanes[lane].polyline.clone())
}

#[test]
fn two_lane_street_followers_pass_in_the_oncoming_lane_without_contact() {
    let (mut w, b, lane) = grid_two_lane(4.0, false, false);
    let r = blocked_run(&mut w, b, &lane, 150.0);
    report("grid 1+1, stopped car in lane", &r);
    assert_eq!(r.contacts, 0, "a civilian touched the stopped car");
    assert_eq!(r.collisions, 0, "civilian-civilian collision");
    assert!(r.queued >= 2, "nobody was held up: {r:?}");
    assert!(r.stats.oncoming_passes >= 1, "{r:?}");
    assert!(
        r.resolved * 2 >= r.queued,
        "most queued cars got away: {r:?}"
    );
    assert!(r.max_wait_s < 75.0, "{r:?}");
    assert!(r.still_waiting <= 3, "{r:?}");
}

#[test]
fn car_parked_across_a_two_lane_street_is_passed_or_turned_away_from() {
    // Across the lane it reaches over the centre line: passing needs more room, turning works.
    let (mut w, b, lane) = grid_two_lane(4.0, true, false);
    let r = blocked_run(&mut w, b, &lane, 150.0);
    report("grid 1+1, car parked across the lane", &r);
    assert_eq!(r.contacts, 0);
    assert_eq!(r.collisions, 0);
    assert!(r.queued >= 2, "{r:?}");
    assert!(r.resolved * 2 >= r.queued, "{r:?}");
    assert!(r.still_waiting <= 3, "{r:?}");
}

#[test]
fn multi_lane_road_changes_lane_around_a_stopped_car() {
    let mut w = World::procedural(&ProcGenConfig::default(), cfg(4.0));
    let net = w.network().clone();
    // Arterial (2 per direction), right lane, long block.
    let lane = net
        .lanes
        .iter()
        .enumerate()
        .filter(|(i, l)| {
            l.kind == LaneKind::Through
                && l.left.is_some()
                && l.right.is_none()
                // A full block (89 m between the arterial junctions since the medians).
                && len_of(&net, *i) > 80.0
        })
        .map(|(i, _)| i)
        .min_by(|&a, &b| {
            let d = |l: usize| (geom::len(geom::xy(net.lanes[l].polyline[0])) - 250.0).abs();
            d(a).total_cmp(&d(b)).then(a.cmp(&b))
        })
        .unwrap();
    let b = blocker_on(&net, lane, len_of(&net, lane) * 0.6, false, false);
    w.set_tracked_vehicles(&[b]);
    w.populate();
    let r = blocked_run(&mut w, b, &net.lanes[lane].polyline, 120.0);
    report("grid 2+2, stopped car in the right lane", &r);
    assert_eq!(r.contacts, 0);
    assert_eq!(r.collisions, 0);
    assert!(r.stats.go_around_lane_changes >= 2, "{r:?}");
    assert_eq!(
        r.stats.oncoming_passes, 0,
        "no oncoming passes on a multi-lane road"
    );
    assert!(r.still_waiting <= 2, "{r:?}");
}

/// A small town: a 3×2 ladder of two-lane streets (150 m blocks) whose middle rung is a
/// single-lane one-way street.
fn ladder_with_one_way() -> RoadNetwork {
    let node = |x: f32, y: f32| NodeSpec {
        pos: [x, y],
        z: 0.0,
        signal: false,
        control: None,
    };
    let nodes = vec![
        node(0.0, 0.0),
        node(150.0, 0.0),
        node(300.0, 0.0),
        node(0.0, 150.0),
        node(150.0, 150.0),
        node(300.0, 150.0),
    ];
    let two = |a, b| RoadSpec::new(a, b, RoadClass::Local, 1, 1);
    let roads = vec![
        two(0, 1),
        two(1, 2),
        two(3, 4),
        two(4, 5),
        two(0, 3),
        two(2, 5),
        // One-way northbound.
        RoadSpec::new(1, 4, RoadClass::Local, 1, 0),
    ];
    let built = builder::build(&nodes, &roads, &BuildOptions::default());
    let zone = Zone {
        kind: DistrictKind::Residential,
        density_base: 40.0,
        polygon: vec![
            [-50.0, -50.0],
            [350.0, -50.0],
            [350.0, 200.0],
            [-50.0, 200.0],
        ],
    };
    built.into_network(
        "ladder_one_way",
        "test",
        None,
        [150.0, 75.0],
        vec![zone],
        Vec::new(),
        Vec::new(),
    )
}

#[test]
fn single_lane_one_way_followers_reverse_and_reroute() {
    let net = ladder_with_one_way();
    let one_way = net
        .lanes
        .iter()
        .position(|l| {
            l.kind != LaneKind::Parking
                && !net
                    .lanes
                    .iter()
                    .any(|o| o.road == l.road && o.from_node == l.to_node)
        })
        .expect("one-way lane");
    let b = blocker_on(&net, one_way, len_of(&net, one_way) * 0.7, false, false);
    let mut w = World::new(net.clone(), cfg(1.5));
    w.set_tracked_vehicles(&[b]);
    w.populate();
    let r = blocked_run(&mut w, b, &net.lanes[one_way].polyline, 240.0);
    report("ladder, single-lane one-way blocked", &r);
    assert_eq!(r.contacts, 0);
    assert_eq!(r.collisions, 0);
    assert!(r.queued >= 1, "{r:?}");
    assert_eq!(r.stats.oncoming_passes, 0);
    assert_eq!(r.stats.turnarounds, 0, "no room to turn on a one-way lane");
    assert!(r.stats.reverses >= 1, "{r:?}");
    assert!(r.stats.reroutes >= 1, "{r:?}");
    assert!(r.still_waiting <= 1, "{r:?}");
}

#[test]
fn port_vesper_two_lane_street_gets_around_a_stopped_car() {
    let mut w = World::vesper(&VesperConfig::default(), cfg(4.0));
    let net = w.network().clone();
    // A straight-ish 1+1 street (bend < 20° over the lane) near downtown.
    let lanes: Vec<usize> = two_lane_streets(&net, 100.0)
        .into_iter()
        .filter(|&l| {
            let p = &net.lanes[l].polyline;
            let d0 = geom::normalize(geom::sub(geom::xy(p[1]), geom::xy(p[0])));
            let n = p.len();
            let d1 = geom::normalize(geom::sub(geom::xy(p[n - 1]), geom::xy(p[n - 2])));
            geom::dot(d0, d1) > 0.94
        })
        .collect();
    assert!(!lanes.is_empty());
    let lane = *lanes
        .iter()
        .min_by(|&&a, &&b| {
            let d = |l: usize| geom::dist(geom::xy(net.lanes[l].polyline[0]), net.core);
            d(a).total_cmp(&d(b)).then(a.cmp(&b))
        })
        .unwrap();
    let b = blocker_on(&net, lane, len_of(&net, lane) * 0.6, false, false);
    w.set_tracked_vehicles(&[b]);
    w.populate();
    let r = blocked_run(&mut w, b, &net.lanes[lane].polyline, 150.0);
    report("Port Vesper 1+1, stopped car in lane", &r);
    assert_eq!(r.contacts, 0);
    assert_eq!(r.collisions, 0);
    assert!(r.queued >= 1, "{r:?}");
    assert!(r.resolved * 2 >= r.queued, "{r:?}");
    assert!(r.still_waiting <= 3, "{r:?}");
}

#[test]
fn map_helpers_for_other_drivers_give_the_street_width_and_a_turn() {
    use wp_world::manoeuvre::{overtake_sight_clear, plan_turn, road_corridor, CarShape};
    let w = World::procedural(&ProcGenConfig::default(), cfg(0.0));
    let net = w.network();
    let lane = two_lane_streets(net, 90.0)[0];
    let pts = &net.lanes[lane].polyline;
    let cum = geom::cumulative(pts);
    let (p, t) = geom::point_at(pts, &cum, len_of(net, lane) * 0.5);
    let road = road_corridor(w.map(), p, t).expect("on the road");
    // A 1+1 street: 7 m of driving lanes, the car's lane on the right.
    assert!((road.lat_max - 1.75).abs() < 0.1, "{road:?}");
    assert!((road.lat_min + 5.25).abs() < 0.1, "{road:?}");
    let shape = CarShape::CIVILIAN;
    let plan = plan_turn(
        &shape,
        &road,
        &[],
        shape.rear(geom::xy(p), t),
        t,
        [-t[0], -t[1]],
    );
    assert!(plan.is_some_and(|p| p.legs.len() >= 2));
    // Flat, straight grid street: visible.
    assert!(overtake_sight_clear(w.map(), p, t, 100.0));
    // Inside a building block, away from any lane: no corridor.
    let off = geom::add(geom::xy(p), geom::scale(geom::right_of(t), 30.0));
    assert!(road_corridor(w.map(), [off[0], off[1], 0.0], t).is_none());
}

#[test]
#[ignore = "perf: run with --release"]
fn perf_400_agents_with_blockers_under_budget() {
    let mut w = World::procedural(
        &ProcGenConfig::default(),
        WorldConfig {
            traffic: TrafficConfig {
                density_scale: 4.0,
                sight_radius_m: 0.0,
                ..Default::default()
            },
            ..cfg(4.0)
        },
    );
    let net = w.network().clone();
    let mut blockers: Vec<TrackedVehicle> = two_lane_streets(&net, 90.0)
        .into_iter()
        .step_by(7)
        .take(8)
        .enumerate()
        .map(|(k, l)| {
            let mut b = blocker_on(&net, l, len_of(&net, l) * 0.6, k % 2 == 1, false);
            b.entity = EntityId(100 + k as u32);
            b
        })
        .collect();
    let arterial = net
        .lanes
        .iter()
        .enumerate()
        .filter(|(i, l)| l.left.is_some() && l.right.is_none() && len_of(&net, *i) > 90.0)
        .map(|(i, _)| i)
        .step_by(11)
        .take(4);
    for (k, l) in arterial.enumerate() {
        let mut b = blocker_on(&net, l, len_of(&net, l) * 0.6, false, false);
        b.entity = EntityId(200 + k as u32);
        blockers.push(b);
    }
    w.set_tracked_vehicles(&blockers);
    w.populate();
    let mut ev = Vec::new();
    // Warm up 20 s (queues form, manoeuvres start), then time 60 s.
    for _ in 0..(20.0 / PHYSICS_DT) as usize {
        w.step(PHYSICS_DT, &mut ev);
    }
    let t0 = std::time::Instant::now();
    let frames = 60 * 60;
    for _ in 0..frames * 4 {
        w.step(PHYSICS_DT, &mut ev);
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0 / frames as f64;
    let s = w.traffic_stats();
    println!(
        "{} agents, {} blockers: {ms:.3} ms per 60 Hz frame; passes {}, go-around lane changes {}, \
         turnarounds {}, reverses {}, reroutes {}",
        w.agent_count(),
        blockers.len(),
        s.oncoming_passes,
        s.go_around_lane_changes,
        s.turnarounds,
        s.reverses,
        s.reroutes
    );
    assert!(w.agent_count() >= 390);
    assert!(ms < 2.0, "{ms} ms per frame over the 2 ms budget");
}
