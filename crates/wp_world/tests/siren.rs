//! Civilians yield to siren units (owner playtest rows 61–62): a unit with its siren on comes up
//! behind cars on a two-lane street; they pull over and it gets past all of them.
//!
//! The unit here is a stand-in driven by the test: a police-car-sized box (4.9 m × 1.9 m) that
//! hugs the centre line (where a real unit passes yielding traffic) at up to 16 m/s and brakes
//! hard for anything in its swept path, so it only gets past cars that actually make room.
//!
//! `cargo test -p wp_world --release --test siren -- --nocapture` prints the numbers quoted in
//! `docs/integration/world.md` (`SIREN_DEBUG=1` adds a 2 Hz timeline of the target cars).

use std::collections::BTreeMap;

use wp_contracts::{EntityId, SignalState, TimeOfDay, PHYSICS_DT};
use wp_world::builder::{self, BuildOptions, NodeSpec, RoadClass, RoadSpec};
use wp_world::geom::{self, Obb, V2, V3};
use wp_world::network::{DistrictKind, LaneKind, RoadNetwork, Zone};
use wp_world::{TrackedVehicle, TrafficConfig, TrafficStats, World, WorldConfig};

const UNIT_MAX_MPS: f32 = 16.0;

fn cfg(density: f32, seed: u64) -> WorldConfig {
    WorldConfig {
        seed,
        start_time: TimeOfDay::from_hours(12.0),
        time_ratio: 1.0,
        // Default sightline rule: nothing spawns within 320 m of the unit (a player).
        traffic: TrafficConfig {
            density_scale: density,
            detect_collisions: true,
            ..Default::default()
        },
        parked_cars: false,
    }
}

/// A 1 km × 150 m loop of two-lane (1+1) streets; the 1 km sides have no junction.
/// `lanes_each_way` > 1 makes the west side a multi-lane arterial.
fn long_loop(lanes_each_way: u8) -> RoadNetwork {
    let node = |x: f32, y: f32| NodeSpec {
        pos: [x, y],
        z: 0.0,
        signal: false,
        control: None,
    };
    let nodes = vec![
        node(0.0, 0.0),
        node(0.0, 1000.0),
        node(150.0, 1000.0),
        node(150.0, 0.0),
    ];
    let two = |a, b| RoadSpec::new(a, b, RoadClass::Local, 1, 1);
    let mut roads = vec![two(1, 2), two(2, 3), two(3, 0)];
    if lanes_each_way > 1 {
        roads.push(RoadSpec::new(
            0,
            1,
            RoadClass::Arterial,
            lanes_each_way,
            lanes_each_way,
        ));
    } else {
        roads.push(two(0, 1));
    }
    let built = builder::build(&nodes, &roads, &BuildOptions::default());
    let zone = Zone {
        kind: DistrictKind::Residential,
        density_base: 60.0,
        polygon: vec![
            [-210.0, -60.0],
            [210.0, -60.0],
            [210.0, 1060.0],
            [-210.0, 1060.0],
        ],
    };
    built.into_network(
        "siren_loop",
        "test",
        None,
        [75.0, 500.0],
        vec![zone],
        Vec::new(),
        Vec::new(),
    )
}

/// A 3 × 3 grid of two-lane (1+1) streets, 200 m blocks, with a signalised crossroads in the
/// middle.
fn signal_grid() -> RoadNetwork {
    let mut nodes = Vec::new();
    for j in 0..3 {
        for i in 0..3 {
            nodes.push(NodeSpec {
                pos: [i as f32 * 200.0, j as f32 * 200.0],
                z: 0.0,
                signal: i == 1 && j == 1,
                control: None,
            });
        }
    }
    let two = |a, b| RoadSpec::new(a, b, RoadClass::Local, 1, 1);
    let mut roads = Vec::new();
    for j in 0..3 {
        for i in 0..3 {
            let n = j * 3 + i;
            if i < 2 {
                roads.push(two(n, n + 1));
            }
            if j < 2 {
                roads.push(two(n, n + 3));
            }
        }
    }
    let built = builder::build(&nodes, &roads, &BuildOptions::default());
    let zone = Zone {
        kind: DistrictKind::Residential,
        density_base: 60.0,
        polygon: vec![
            [-60.0, -60.0],
            [460.0, -60.0],
            [460.0, 460.0],
            [-60.0, 460.0],
        ],
    };
    built.into_network(
        "siren_grid",
        "test",
        None,
        [200.0, 200.0],
        vec![zone],
        Vec::new(),
        Vec::new(),
    )
}

/// The northbound lanes of the west side starting at y = 0 (x ≈ 0, heading +Y, at least
/// 500 m long), right lane first.
fn northbound_lanes(net: &RoadNetwork) -> Vec<usize> {
    let mut v: Vec<(f32, usize)> = net
        .lanes
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            let p = &l.polyline;
            l.kind != LaneKind::Parking
                && p[0][0].abs() < 12.0
                && p[0][1] < 60.0
                && p[p.len() - 1][0].abs() < 12.0
                && p[p.len() - 1][1] > p[0][1] + 500.0
        })
        .map(|(i, l)| (-l.polyline[0][0], i))
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v.into_iter().map(|(_, i)| i).collect()
}

fn body(c: V2, f: V2, hl: f32, hw: f32) -> Obb {
    Obb { c, f, hl, hw }
}

/// What happened around the unit.
#[derive(Debug)]
struct SirenRun {
    /// Cars ahead of the unit at the start (in its lane, nearest first).
    targets: usize,
    /// Of those, cars the unit got past.
    passed: usize,
    /// Time to get past all of them, s (`None` if it did not).
    all_passed_s: Option<f32>,
    /// Distance from the unit to the furthest target at the start, m.
    span_m: f32,
    /// Unit–civilian body overlaps (samples at 240 Hz).
    contacts: usize,
    /// Slowest the unit went after its first 3 s until it had passed everyone, m/s.
    min_unit_mps: f32,
    /// Of the cars passed, those driving on (> 2 m/s, back in lane, not yielding) at the end.
    moving_again: usize,
    /// Most civilians flagged as yielding at once.
    max_yielding: usize,
    stats: TrafficStats,
}

/// Run a stand-in siren unit up lane `lane` from arc length `s0`, at the `n` cars ahead of it
/// in that lane, for `seconds`. `siren` switches its lights. The unit hugs the line on the left
/// of the lane, or rides the lane centre with `centre`.
fn siren_run(w: &mut World, lane: usize, s0: f32, n: usize, siren: bool, seconds: f32) -> SirenRun {
    siren_run_at(w, lane, s0, n, siren, seconds, false)
}

fn siren_run_at(
    w: &mut World,
    lane: usize,
    s0: f32,
    n: usize,
    siren: bool,
    seconds: f32,
    centre: bool,
) -> SirenRun {
    let debug = std::env::var("SIREN_DEBUG").is_ok();
    let net = w.network().clone();
    let pts: Vec<V3> = net.lanes[lane].polyline.clone();
    let cum = geom::cumulative(&pts);
    let half_lane = net.lanes[lane].width_m * 0.5;
    let on_lane = |p: V3, yaw: f32| -> Option<f32> {
        let pr = geom::project(&pts, &cum, geom::xy(p));
        let (_, t) = geom::point_at(&pts, &cum, pr.s);
        (pr.dist < 1.4 && geom::dot(geom::dir_of_yaw(yaw), t) > 0.8).then_some(pr.s)
    };
    let mut cars: Vec<(f32, u32)> = w
        .agents()
        .filter_map(|a| on_lane(a.pos, a.yaw).map(|s| (s, a.id)))
        .filter(|(s, _)| *s > s0)
        .collect();
    cars.sort_by(|a, b| a.0.total_cmp(&b.0));
    cars.truncate(n);
    assert_eq!(cars.len(), n, "not enough cars on the lane");
    let span_m = cars[n - 1].0 - s0;
    let mut s_u = s0;
    let mut v_u = 12.0f32;
    // Hugging the line on its left (its left side 0.2 m over it), or the lane centre.
    let offset = if centre { 0.0 } else { -(half_lane - 0.2) };
    let mut passed: BTreeMap<u32, f32> = BTreeMap::new();
    let mut contacts = 0;
    let mut max_yielding = 0;
    let mut min_unit_mps = f32::INFINITY;
    let mut all_passed_s = None;
    let mut ev = Vec::new();
    let steps = (seconds / PHYSICS_DT) as usize;
    for k in 0..steps {
        let t = k as f32 * PHYSICS_DT;
        let (p, f) = geom::point_at(&pts, &cum, s_u);
        let r = geom::right_of(f);
        let c = geom::add(geom::xy(p), geom::scale(r, offset));
        let unit = TrackedVehicle {
            entity: EntityId(500),
            pos: [c[0], c[1], p[2]],
            vel: [f[0] * v_u, f[1] * v_u, 0.0],
            forward: f,
            siren,
            player: true,
        };
        w.set_tracked_vehicles(&[unit]);
        w.step(PHYSICS_DT, &mut ev);
        // The unit: brake for anything in its swept path, else speed up to the max.
        let look = 2.0 + v_u * v_u / 12.0 + v_u * 0.3;
        let swept = body(
            geom::add(c, geom::scale(f, look * 0.5)),
            f,
            2.45 + look * 0.5,
            1.0,
        );
        let unit_box = body(c, f, 2.45, 0.95);
        let mut blocked = false;
        let mut yielding = 0;
        for a in w.agents() {
            if (a.pos[2] - p[2]).abs() > 3.0 {
                continue;
            }
            let ab = body(geom::xy(a.pos), geom::dir_of_yaw(a.yaw), 2.25, 0.95);
            if geom::obb_overlap(&ab, &unit_box) {
                contacts += 1;
                if debug && contacts < 5 {
                    let rel = geom::sub(geom::xy(a.pos), c);
                    println!(
                        "  contact t={t:.2} agent {} long {:.2} lat {:.2} v {:.1} yielding {}",
                        a.id,
                        geom::dot(rel, f),
                        geom::dot(rel, r),
                        a.speed_mps,
                        a.yielding
                    );
                }
            }
            if geom::obb_overlap(&ab, &swept) {
                blocked = true;
            }
            if a.yielding {
                yielding += 1;
            }
        }
        max_yielding = max_yielding.max(yielding);
        if debug && k % 120 == 0 {
            let mut line = format!("t={t:.1} unit s {s_u:.0} v {v_u:.1} blocked {blocked} |");
            for &(_, id) in &cars {
                if let Some(a) = w.agents().find(|a| a.id == id) {
                    let pr = geom::project(&pts, &cum, geom::xy(a.pos));
                    line += &format!(
                        " {}:{:.0}/{:.2}/{:.1}{}",
                        id,
                        pr.s - s_u,
                        pr.lateral,
                        a.speed_mps,
                        if a.yielding { "Y" } else { "" }
                    );
                }
            }
            println!("{line}");
        }
        // It stops at the end of its lane (what it does in the junction is not tested here).
        let end = cum[cum.len() - 1] - 3.0;
        let acc = if blocked { -6.0 } else { 3.0 };
        let v_end = (2.0 * 6.0 * (end - s_u).max(0.0)).sqrt();
        v_u = (v_u + acc * PHYSICS_DT).clamp(0.0, UNIT_MAX_MPS.min(v_end));
        s_u = (s_u + v_u * PHYSICS_DT).min(end);
        if t > 3.0 && all_passed_s.is_none() && s_u < end - 40.0 {
            min_unit_mps = min_unit_mps.min(v_u);
        }
        // Passed: the unit's rear bumper is ahead of the car's front bumper.
        for &(_, id) in &cars {
            if passed.contains_key(&id) {
                continue;
            }
            let s = w
                .agents()
                .find(|a| a.id == id)
                .map(|a| geom::project(&pts, &cum, geom::xy(a.pos)).s);
            if s.is_none_or(|s| s_u - 2.45 > s + 2.25) {
                passed.insert(id, t);
            }
        }
        if all_passed_s.is_none() && passed.len() == cars.len() {
            all_passed_s = Some(t);
        }
    }
    let moving_again = passed
        .keys()
        .filter(|id| {
            w.agents().any(|a| {
                a.id == **id
                    && a.speed_mps > 2.0
                    && !a.yielding
                    && geom::project(&pts, &cum, geom::xy(a.pos)).dist < 0.3
            }) || !w.agents().any(|a| a.id == **id)
        })
        .count();
    SirenRun {
        targets: cars.len(),
        passed: passed.len(),
        all_passed_s,
        span_m,
        contacts,
        min_unit_mps,
        moving_again,
        max_yielding,
        stats: w.traffic_stats().clone(),
    }
}

fn report(name: &str, r: &SirenRun) {
    println!(
        "{name}: {} of {} cars ({:.0} m ahead) passed after {:?} s, unit min {:.1} m/s, \
         contacts {}, moving again {}, max yielding {}, yields {}, siren lane changes {}, \
         civilian collisions {}",
        r.passed,
        r.targets,
        r.span_m,
        r.all_passed_s,
        r.min_unit_mps,
        r.contacts,
        r.moving_again,
        r.max_yielding,
        r.stats.siren_yields,
        r.stats.siren_lane_changes,
        r.stats.civilian_collisions
    );
}

fn step_for(w: &mut World, seconds: f32) {
    let mut ev = Vec::new();
    for _ in 0..(seconds / PHYSICS_DT) as usize {
        w.step(PHYSICS_DT, &mut ev);
    }
}

fn warmed_loop(lanes_each_way: u8, seed: u64) -> World {
    let mut w = World::new(long_loop(lanes_each_way), cfg(2.0, seed));
    w.populate();
    step_for(&mut w, 20.0);
    w
}

/// First spot at or after `s` (`step` > 0) or at or before it (`step` < 0) on `lane` with no
/// car within 12 m (any lane).
fn free_spot(w: &World, lane: usize, mut s: f32, step: f32) -> f32 {
    let pts = &w.network().lanes[lane].polyline;
    let cum = geom::cumulative(pts);
    loop {
        let (p, _) = geom::point_at(pts, &cum, s);
        if !w
            .agents()
            .any(|a| geom::dist(geom::xy(a.pos), geom::xy(p)) < 12.0)
        {
            return s;
        }
        s += step;
    }
}

#[test]
fn siren_unit_gets_past_moving_traffic_without_slowing() {
    for seed in [1u64, 2, 3] {
        let mut w = warmed_loop(1, seed);
        let lane = northbound_lanes(w.network())[0];
        let s0 = free_spot(&w, lane, 200.0, 2.0);
        let r = siren_run(&mut w, lane, s0, 6, true, 32.0);
        report(&format!("1+1 moving, siren on, seed {seed}"), &r);
        assert_eq!(r.contacts, 0, "the unit touched a civilian: {r:?}");
        assert_eq!(r.stats.civilian_collisions, 0, "{r:?}");
        assert_eq!(r.passed, 6, "{r:?}");
        // Hardly held up: the cars are out of its way before it gets there (a slow reactor
        // right ahead at the start may cost it a few m/s).
        assert!(r.min_unit_mps >= UNIT_MAX_MPS - 4.0, "{r:?}");
        // Cars drive on at ~9 m/s until they notice it (≤ 120 m back), then stop.
        let t = r.all_passed_s.unwrap();
        assert!(t <= 6.0 + r.span_m / 9.0, "took {t} s: {r:?}");
        assert!(r.stats.siren_yields >= 6, "{r:?}");
        // Everyone merges back and drives on after the unit has gone.
        assert!(r.moving_again >= 5, "{r:?}");
    }
}

#[test]
fn siren_unit_gets_past_six_queued_cars_within_ten_seconds() {
    // Wait for six cars queued at the red on a 1+1 approach to the crossroads, with room for the
    // unit 30 m behind the sixth.
    for seed in [1u64, 2, 3] {
        // 2.0 at noon on the round-13 curve (Residential 0.75) = the old 3.0 × 0.5: same cars.
        let mut w = World::new(signal_grid(), cfg(2.0, seed));
        w.populate();
        let net = w.network().clone();
        let lanes: Vec<usize> = (0..net.lanes.len())
            .filter(|&l| {
                let ln = &net.lanes[l];
                ln.kind == LaneKind::Through
                    && ln.left.is_none()
                    && ln.right.is_none()
                    && w.map().lane_signal(l as u32).is_some()
                    && *geom::cumulative(&ln.polyline).last().unwrap() > 120.0
            })
            .collect();
        assert!(!lanes.is_empty());
        let mut found = None;
        'wait: for _ in 0..(300.0 / (4.0 * PHYSICS_DT)) as usize {
            step_for(&mut w, 4.0 * PHYSICS_DT);
            for &lane in &lanes {
                if w.map().lane_signal(lane as u32).map(|x| x.0) != Some(SignalState::Red) {
                    continue;
                }
                let pts = &net.lanes[lane].polyline;
                let cum = geom::cumulative(pts);
                let len = *cum.last().unwrap();
                let mut queued: Vec<f32> = w
                    .agents()
                    .filter(|a| a.speed_mps < 0.3)
                    .filter_map(|a| {
                        let pr = geom::project(pts, &cum, geom::xy(a.pos));
                        (pr.dist < 1.0 && pr.s > len - 60.0).then_some(pr.s)
                    })
                    .collect();
                queued.sort_by(|a, b| b.total_cmp(a));
                if queued.len() >= 6 && queued[5] - 30.0 > 5.0 {
                    let s0 = queued[5] - 30.0;
                    if free_spot(&w, lane, s0, 2.0) == s0 {
                        found = Some((lane, s0));
                        break 'wait;
                    }
                }
            }
        }
        let (lane, s0) = found.expect("no queue of six formed at a red");
        let r = siren_run(&mut w, lane, s0, 6, true, 20.0);
        report(
            &format!("grid 1+1 queue at a red, siren on, seed {seed}"),
            &r,
        );
        assert_eq!(r.contacts, 0, "{r:?}");
        assert_eq!(r.stats.civilian_collisions, 0, "{r:?}");
        assert_eq!(r.passed, 6, "{r:?}");
        let t = r.all_passed_s.unwrap();
        assert!(t <= 10.0, "took {t} s: {r:?}");
    }
}

#[test]
fn without_the_siren_nobody_pulls_over() {
    for seed in [1u64, 2, 3] {
        let mut w = warmed_loop(1, seed);
        let lane = northbound_lanes(w.network())[0];
        let s0 = free_spot(&w, lane, 200.0, 2.0);
        let r = siren_run(&mut w, lane, s0, 6, false, 20.0);
        report(&format!("1+1 moving, siren off, seed {seed}"), &r);
        assert_eq!(r.stats.siren_yields, 0, "{r:?}");
        assert_eq!(r.max_yielding, 0, "{r:?}");
        assert_eq!(r.contacts, 0, "{r:?}");
        assert!(
            r.passed <= 1,
            "the unit got past traffic that did not yield: {r:?}"
        );
    }
}

#[test]
fn multi_lane_road_makes_room_between_the_lanes() {
    for seed in [1u64, 2, 3] {
        let mut w = warmed_loop(2, seed);
        let lanes = northbound_lanes(w.network());
        assert_eq!(lanes.len(), 2);
        // The unit comes up the right lane hugging the line to the left lane: right-lane cars
        // pull to the kerb, left-lane cars to the left.
        let s0 = free_spot(&w, lanes[0], 200.0, 2.0);
        let r = siren_run(&mut w, lanes[0], s0, 4, true, 32.0);
        report(&format!("2+2 moving, siren on, seed {seed}"), &r);
        assert_eq!(r.contacts, 0, "{r:?}");
        assert_eq!(r.stats.civilian_collisions, 0, "{r:?}");
        assert_eq!(r.passed, 4, "{r:?}");
        // A slow reactor right ahead at the start may cost it a little (seed 1 since the
        // arterial median moved the lanes, row 94: a car 13 m ahead, 13.6 m/s once).
        assert!(r.min_unit_mps >= UNIT_MAX_MPS - 2.5, "{r:?}");
    }
}

#[test]
fn multi_lane_road_clears_the_lane_the_unit_is_in() {
    let mut changes = 0;
    for seed in [1u64, 2, 3] {
        let mut w = warmed_loop(2, seed);
        let lanes = northbound_lanes(w.network());
        // The unit comes up the middle of the left (fast) lane: cars in it move over to the
        // right lane where there is room, the others pull aside within the lane.
        let s0 = free_spot(&w, lanes[1], 200.0, 2.0);
        let r = siren_run_at(&mut w, lanes[1], s0, 4, true, 32.0, true);
        report(
            &format!("2+2 moving, unit in the left lane, seed {seed}"),
            &r,
        );
        assert_eq!(r.contacts, 0, "{r:?}");
        assert_eq!(r.stats.civilian_collisions, 0, "{r:?}");
        changes += r.stats.siren_lane_changes;
    }
    // Only where the other lane has a safe gap (cars alongside block it).
    assert!(changes >= 2, "nobody moved out of the unit's lane");
}

/// A unit `back_m` behind car `id` in its lane (centre line side), at `speed` m/s along it.
fn unit_behind(w: &World, lane: usize, id: u32, back_m: f32, speed: f32) -> TrackedVehicle {
    let pts = &w.network().lanes[lane].polyline;
    let cum = geom::cumulative(pts);
    let a = w.agents().find(|a| a.id == id).unwrap();
    let s = geom::project(pts, &cum, geom::xy(a.pos)).s - back_m;
    let (p, f) = geom::point_at(pts, &cum, s);
    TrackedVehicle {
        entity: EntityId(500),
        pos: p,
        vel: [f[0] * speed, f[1] * speed, 0.0],
        forward: f,
        siren: true,
        player: true,
    }
}

/// Lateral offset of agent `id` from `lane`'s centre, and its speed.
fn lat_and_speed(w: &World, lane: usize, id: u32) -> (f32, f32) {
    let pts = &w.network().lanes[lane].polyline;
    let cum = geom::cumulative(pts);
    let a = w.agents().find(|a| a.id == id).unwrap();
    (
        geom::project(pts, &cum, geom::xy(a.pos)).lateral,
        a.speed_mps,
    )
}

#[test]
fn unit_stuck_right_behind_a_car_gets_it_to_pull_over_then_it_gives_up_on_a_parked_one() {
    let mut w = warmed_loop(1, 4);
    let lane = northbound_lanes(w.network())[0];
    let pts = w.network().lanes[lane].polyline.clone();
    let cum = geom::cumulative(&pts);
    let id = w
        .agents()
        .filter(|a| {
            let pr = geom::project(&pts, &cum, geom::xy(a.pos));
            pr.dist < 1.0 && pr.s > 300.0 && pr.s < 700.0
        })
        .map(|a| a.id)
        .next()
        .expect("a car on the lane");
    // Crawling 12 m behind it in its lane (wp_police pushes through stuck traffic like this).
    let mut ev = Vec::new();
    for _ in 0..(6.0 / PHYSICS_DT) as usize {
        let u = unit_behind(&w, lane, id, 12.0, 1.0);
        w.set_tracked_vehicles(&[u]);
        w.step(PHYSICS_DT, &mut ev);
    }
    let (lat, v) = lat_and_speed(&w, lane, id);
    assert!(lat > 0.5, "did not pull over: lat {lat}, v {v}");
    assert!(v < 0.5, "did not stop: v {v}");
    assert!(w.agents().find(|a| a.id == id).unwrap().yielding);
    // Now the unit parks where it is: after `STALL_S` the car gives up on it, merges back and
    // drives off, and does not pull over for it again.
    let parked = unit_behind(&w, lane, id, 12.0, 0.0);
    let mut yielded_again = false;
    let mut left_kerb = false;
    for _ in 0..(30.0 / PHYSICS_DT) as usize {
        w.set_tracked_vehicles(&[parked]);
        w.step(PHYSICS_DT, &mut ev);
        let Some(a) = w.agents().find(|a| a.id == id) else {
            break;
        };
        let lat = geom::project(&pts, &cum, geom::xy(a.pos)).lateral;
        if lat < 0.2 {
            left_kerb = true;
        } else if left_kerb && lat > 0.5 {
            yielded_again = true;
        }
    }
    assert!(left_kerb, "never merged back from a parked unit");
    assert!(!yielded_again, "pulled over again for a parked unit");
    let (_, v) = lat_and_speed(&w, lane, id);
    assert!(v > 2.0, "not driving on: {v}");
}

/// Pulling over for a siren indicates towards the kerb (right) while pulling over and waiting,
/// away from it (left) while merging back, and nothing once back in the lane.
#[test]
fn pulling_over_and_merging_back_are_indicated() {
    let mut w = warmed_loop(1, 4);
    let lane = northbound_lanes(w.network())[0];
    let pts = w.network().lanes[lane].polyline.clone();
    let cum = geom::cumulative(&pts);
    let id = w
        .agents()
        .filter(|a| {
            let pr = geom::project(&pts, &cum, geom::xy(a.pos));
            pr.dist < 1.0 && pr.s > 300.0 && pr.s < 700.0
        })
        .map(|a| a.id)
        .next()
        .expect("a car on the lane");
    let mut ev = Vec::new();
    let mut towards_kerb = 0;
    for _ in 0..(6.0 / PHYSICS_DT) as usize {
        let u = unit_behind(&w, lane, id, 12.0, 1.0);
        w.set_tracked_vehicles(&[u]);
        w.step(PHYSICS_DT, &mut ev);
        let a = w.agents().find(|a| a.id == id).unwrap();
        assert!(
            a.turn_signal >= 0,
            "indicated away from the kerb while pulling over"
        );
        if a.yielding && a.turn_signal == 1 {
            towards_kerb += 1;
        }
    }
    assert!(towards_kerb > 0, "never indicated towards the kerb");
    let (lat, _) = lat_and_speed(&w, lane, id);
    assert!(lat > 0.5, "did not pull over: lat {lat}");
    // The unit parks: the car merges back (indicating left) and drives on (indicator off).
    let parked = unit_behind(&w, lane, id, 12.0, 0.0);
    let mut merging_left = 0;
    let mut off_in_lane = 0;
    for _ in 0..(30.0 / PHYSICS_DT) as usize {
        w.set_tracked_vehicles(&[parked]);
        w.step(PHYSICS_DT, &mut ev);
        let Some(a) = w.agents().find(|a| a.id == id) else {
            break;
        };
        let pr = geom::project(&pts, &cum, geom::xy(a.pos));
        if a.turn_signal == -1 && pr.lateral > 0.2 {
            merging_left += 1;
        }
        if !a.yielding && pr.lateral.abs() < 0.1 && a.speed_mps > 2.0 && a.turn_signal == 0 {
            off_in_lane += 1;
        }
    }
    assert!(merging_left > 0, "merged back without indicating");
    assert!(off_in_lane > 0, "indicator left on after merging back");
}
