//! `PathFollower` end to end: the real `wp_vehicle` model (in its single-car Rapier rig, the
//! same per-tick sequence as `wp_sim`) driven along city routes.

use wp_contracts::{AssistProfile, LoadedCar, VehicleSim, PHYSICS_DT};
use wp_vehicle::rig::Rig;
use wp_vehicle::VehicleExtras;
use wp_world::drive::{DrivePath, FollowParams, PathFollower};
use wp_world::geom;
use wp_world::network::PoiKind;
use wp_world::procgen::ProcGenConfig;
use wp_world::{TrafficConfig, World, WorldConfig};

fn world() -> World {
    World::procedural(
        &ProcGenConfig::default(),
        WorldConfig {
            traffic: TrafficConfig {
                enabled: false,
                ..Default::default()
            },
            parked_cars: false,
            ..Default::default()
        },
    )
}

struct Run {
    max_cross_track: f32,
    off_road_ticks: usize,
    done: bool,
    max_speed: f32,
    seconds: f32,
}

fn sedan() -> LoadedCar {
    wp_contracts::load_car(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/cars/debug_sedan.json"),
    )
    .unwrap()
}

/// The car in a Rapier rig at the start of `path`, facing along it.
fn rig_at(w: &World, car: &LoadedCar, path: &DrivePath) -> Rig<wp_world::CityMap> {
    let (p0, t0) = path.point_at(0.0);
    Rig::with_world(
        car,
        VehicleExtras::default(),
        w.map().clone(),
        p0,
        geom::yaw_of(t0),
    )
}

/// Drive the debug sedan along `path` (AI assists, controller at 60 Hz, physics at 240 Hz).
fn drive(w: &World, path: &DrivePath, target: f32, params: FollowParams, max_s: f32) -> Run {
    let car = sedan();
    let mut rig = rig_at(w, &car, path);
    let mut f = PathFollower::for_vehicle(&car.def, params, 0.0);
    let assists = AssistProfile::ai();
    let mut input = Default::default();
    let mut run = Run {
        max_cross_track: 0.0,
        off_road_ticks: 0,
        done: false,
        max_speed: 0.0,
        seconds: 0.0,
    };
    let ticks = (max_s / PHYSICS_DT) as usize;
    for i in 0..ticks {
        if i % 4 == 0 {
            let st = rig.vehicle.state();
            let out = f.follow(&st, path, target, 0.0, 4.0 * PHYSICS_DT);
            input = out.input;
            // Skip the launch while the car settles onto the path.
            if i > 240 {
                run.max_cross_track = run.max_cross_track.max(out.cross_track_m.abs());
            }
            run.max_speed = run.max_speed.max(st.speed_mps);
            if w.map().locate(st.pos, None, 1.0, true, true).is_none()
                && w.map().intersection_at(geom::xy(st.pos)).is_none()
            {
                run.off_road_ticks += 1;
            }
            if out.done {
                run.done = true;
                run.seconds = i as f32 * PHYSICS_DT;
                break;
            }
        }
        rig.step(&input, &assists);
    }
    run
}

#[test]
fn follows_a_route_between_pois_and_stops_at_the_end() {
    let w = world();
    let pois = &w.network().pois;
    // At street level: the single-car rig drives on a flat slab, so it stays off the
    // highway viaduct (the "Highway sprint" grid).
    let start = pois
        .iter()
        .position(|p| p.kind == PoiKind::RaceStartGrid && p.name == "Arterial drag")
        .unwrap();
    let finish = pois.iter().position(|p| p.kind == PoiKind::Garage).unwrap();
    let route = w.plan_route_between_pois(start, finish).unwrap();
    let path = DrivePath::from_route(&route);
    let r = drive(&w, &path, 15.0, FollowParams::CIVILIAN, 240.0);
    println!(
        "route {:.0} m: done {} in {:.1} s, max cross-track {:.2} m, max speed {:.1}, off-road {}",
        path.length(),
        r.done,
        r.seconds,
        r.max_cross_track,
        r.max_speed,
        r.off_road_ticks
    );
    assert!(r.done, "did not reach the end");
    // Pure pursuit cuts junction corners a little; the car never leaves the road.
    assert!(r.max_cross_track < 2.0, "cross-track {}", r.max_cross_track);
    assert_eq!(r.off_road_ticks, 0);
    assert!(r.max_speed <= 15.5);
}

#[test]
fn pursuit_params_lap_a_circuit_faster_than_civilian() {
    let w = world();
    let route = w
        .plan_circuit(&[
            [-220.0, -220.0],
            [220.0, -220.0],
            [220.0, 220.0],
            [-220.0, 220.0],
        ])
        .unwrap();
    let path = DrivePath::from_route(&route);
    assert!(path.looped());
    let lap = |params| {
        // Time to cover one lap length (looped paths never finish).
        let car = sedan();
        let mut rig = rig_at(&w, &car, &path);
        let mut f = PathFollower::for_vehicle(&car.def, params, 0.0);
        let assists = AssistProfile::ai();
        let mut input = Default::default();
        let (mut travelled, mut last_s, mut max_ct) = (0.0f32, 0.0f32, 0.0f32);
        for i in 0..(240 * 300) {
            if i % 4 == 0 {
                let out = f.follow(&rig.vehicle.state(), &path, 30.0, 0.0, 4.0 * PHYSICS_DT);
                input = out.input;
                let mut ds = out.s - last_s;
                if ds < -path.length() * 0.5 {
                    ds += path.length();
                }
                travelled += ds.max(0.0);
                last_s = out.s;
                if i > 240 {
                    max_ct = max_ct.max(out.cross_track_m.abs());
                }
                if travelled >= path.length() {
                    return (i as f32 * PHYSICS_DT, max_ct);
                }
            }
            rig.step(&input, &assists);
        }
        (f32::INFINITY, max_ct)
    };
    let (t_civ, ct_civ) = lap(FollowParams::CIVILIAN);
    let (t_pur, ct_pur) = lap(FollowParams::PURSUIT);
    println!(
        "circuit {:.0} m: civilian {t_civ:.1} s (cross-track {ct_civ:.2} m), pursuit {t_pur:.1} s ({ct_pur:.2} m)",
        path.length()
    );
    assert!(t_civ.is_finite() && t_pur.is_finite());
    assert!(t_pur < t_civ * 0.9);
    // Pursuit peak 1.81 m on the old 10 km ground slab, 2.04 m on the half-space ground (same
    // lap time to 0.1 s; the slab's bogus contact normals nudged the car): still well on the
    // road, whose lanes are 3.5 m.
    assert!(ct_civ < 1.5 && ct_pur < 2.2);
}

// ---------------------------------------------------------------------------------------------
// Leader query: police / racers stop behind vehicles ahead (lead round 6).
// ---------------------------------------------------------------------------------------------

use wp_world::drive::FollowTraffic;
use wp_world::leader::{CarFollowing, LeaderParams, Obstacle, CAR_HALF_LENGTH_M};

/// A pursuit-tuned car on a straight 600 m path along +Y (flat ground) drives at `target`
/// m/s while `leader_at(t)` gives the other car's centre `y` and speed at time `t`. Returns
/// (smallest bumper gap seen, final gap, final speed).
fn approach(target: f32, leader_at: impl Fn(f32) -> (f32, f32), seconds: f32) -> (f32, f32, f32) {
    let car = sedan();
    let mut rig = Rig::new(&car);
    let path = DrivePath::new(&[[0.0, 0.0, 0.0], [0.0, 600.0, 0.0]], false);
    let mut f = PathFollower::for_vehicle(&car.def, FollowParams::PURSUIT, 0.0);
    let assists = AssistProfile::ai();
    let mut input = Default::default();
    let mut min_gap = f32::INFINITY;
    let (mut gap, mut speed) = (0.0, 0.0);
    let own = 1u32;
    for i in 0..((seconds / PHYSICS_DT) as usize) {
        let t = i as f32 * PHYSICS_DT;
        let (ly, lv) = leader_at(t);
        let st = rig.vehicle.state();
        gap = ly - st.pos[1] - 2.0 * CAR_HALF_LENGTH_M;
        min_gap = min_gap.min(gap);
        speed = st.speed_mps;
        if i % 4 == 0 {
            let other = Obstacle {
                id: 2,
                pos: [0.0, ly, 0.0],
                vel: [0.0, lv, 0.0],
                forward: [0.0, 1.0],
                half_width_m: 0.9,
                half_length_m: CAR_HALF_LENGTH_M,
            };
            // The car itself is in the list too, as wp_sim would pass every vehicle.
            let me = Obstacle::from_state(own, &st);
            let obstacles = [me, other];
            let traffic = FollowTraffic {
                obstacles: &obstacles,
                skip: &[own],
                corridor: LeaderParams::CAR,
                following: CarFollowing::PURSUIT,
            };
            let out = f.follow_behind(&st, &path, target, 0.0, 4.0 * PHYSICS_DT, Some(&traffic));
            input = out.input;
        }
        rig.step(&input, &assists);
    }
    (min_gap, gap, speed)
}

#[test]
fn pursuit_follower_stops_behind_a_stopped_car() {
    // Stopped car 250 m ahead; the unit is at 25 m/s well before reaching it.
    let (min_gap, gap, speed) = approach(25.0, |_| (250.0, 0.0), 40.0);
    println!("stopped leader: min gap {min_gap:.2} m, final gap {gap:.2} m, speed {speed:.2}");
    assert!(min_gap > 1.0, "came within {min_gap} m");
    assert!(gap < 6.0, "stopped too far back: {gap}");
    assert!(speed < 0.5);
}

#[test]
fn pursuit_follower_settles_behind_a_slower_car() {
    // A car at 8 m/s starting 80 m ahead; the unit wants 25 m/s.
    let (min_gap, gap, speed) = approach(25.0, |t| (80.0 + 8.0 * t, 8.0), 60.0);
    println!("moving leader: min gap {min_gap:.2} m, final gap {gap:.2} m, speed {speed:.2}");
    assert!(min_gap > 4.0, "came within {min_gap} m");
    assert!((speed - 8.0).abs() < 1.0, "speed {speed}");
    assert!(gap < 25.0, "hangs back {gap} m");
}

#[test]
fn follow_without_traffic_is_unchanged() {
    // `follow` == `follow_behind(.., None)`, and an empty obstacle list changes nothing.
    let car = sedan();
    let rig = Rig::new(&car);
    let path = DrivePath::new(&[[0.0, 0.0, 0.0], [0.0, 600.0, 0.0]], false);
    let st = rig.vehicle.state();
    let mut a = PathFollower::for_vehicle(&car.def, FollowParams::PURSUIT, 0.0);
    let mut b = a.clone();
    let mut c = a.clone();
    let oa = a.follow(&st, &path, 20.0, 0.0, 1.0 / 60.0);
    let ob = b.follow_behind(&st, &path, 20.0, 0.0, 1.0 / 60.0, None);
    let empty = FollowTraffic {
        obstacles: &[],
        skip: &[],
        corridor: LeaderParams::CAR,
        following: CarFollowing::PURSUIT,
    };
    let oc = c.follow_behind(&st, &path, 20.0, 0.0, 1.0 / 60.0, Some(&empty));
    assert_eq!(oa, ob);
    assert_eq!(oa, oc);
}

// ---------------------------------------------------------------------------------------------
// Manoeuvres (playtest row 32: turn around when blocked)
// ---------------------------------------------------------------------------------------------

use wp_world::manoeuvre::{
    CarShape, ManoeuvreStatus, ReverseDriver, RoadCorridor, TurnDriver, TURN_DONE_COS,
};

/// Corners' worst lateral excess outside `[lo, hi]` for a body at `pos` facing `f`.
fn body_excess(pos: [f32; 2], f: [f32; 2], lo: f32, hi: f32) -> f32 {
    let r = geom::right_of(f);
    let mut worst = 0.0f32;
    for (a, b) in [(2.25, 0.95), (2.25, -0.95), (-2.25, 0.95), (-2.25, -0.95)] {
        let x = pos[0] + f[0] * a + r[0] * b;
        worst = worst.max(lo - x).max(x - hi);
    }
    worst
}

/// The real car turns around on a 1+1 street (7 m kerb to kerb) behind a stopped car, inside
/// the kerbs and without touching the blocker.
#[test]
fn turn_driver_turns_a_real_car_around_on_a_two_lane_street() {
    let car = sedan();
    let mut rig = Rig::new(&car);
    let assists = AssistProfile::ai();
    let st0 = rig.vehicle.state();
    let (p0, f0) = (geom::xy(st0.pos), wp_world::drive::forward_of(st0.rot));
    assert!(geom::dot(f0, [0.0, 1.0]) > 0.99);
    let road = RoadCorridor {
        origin: p0,
        dir: [0.0, 1.0],
        lat_min: -5.25,
        lat_max: 1.75,
    };
    let blocker = geom::Obb {
        c: [p0[0], p0[1] + 2.25 + 7.0 + 2.25],
        f: [0.0, 1.0],
        hl: 2.25,
        hw: 0.95,
    };
    let shape = CarShape::from_def(&car.def);
    let mut drv = TurnDriver::new(shape, road, [0.0, -1.0], vec![blocker]);
    let mut input = Default::default();
    let mut status = ManoeuvreStatus::Running;
    let mut worst_edge = 0.0f32;
    let mut t_done = 0.0;
    for i in 0..((60.0 / PHYSICS_DT) as usize) {
        let st = rig.vehicle.state();
        let (p, f) = (geom::xy(st.pos), wp_world::drive::forward_of(st.rot));
        worst_edge = worst_edge.max(body_excess(
            [p[0] - p0[0], p[1]],
            f,
            road.lat_min,
            road.lat_max,
        ));
        let me = geom::Obb {
            c: p,
            f,
            hl: 2.25,
            hw: 0.95,
        };
        assert!(!geom::obb_overlap(&me, &blocker), "hit the blocker at {i}");
        if i % 4 == 0 {
            let (inp, s) = drv.step(&st, 4.0 * PHYSICS_DT);
            input = inp;
            status = s;
            if s != ManoeuvreStatus::Running {
                t_done = i as f32 * PHYSICS_DT;
                break;
            }
        }
        rig.step(&input, &assists);
    }
    let st = rig.vehicle.state();
    let f = wp_world::drive::forward_of(st.rot);
    println!(
        "turn: {status:?} after {t_done:.1} s, {} legs, worst kerb excess {worst_edge:.2} m, end {:?}",
        drv.legs_driven(),
        st.pos
    );
    assert_eq!(status, ManoeuvreStatus::Done);
    assert!(geom::dot(f, [0.0, -1.0]) >= TURN_DONE_COS);
    assert!(worst_edge < 0.3, "left the street by {worst_edge} m");
    assert!(t_done < 40.0);
}

/// The real car reverses 40 m down a straight lane and stops at the end.
#[test]
fn reverse_driver_backs_a_real_car_down_a_lane() {
    let car = sedan();
    let mut rig = Rig::new(&car);
    let assists = AssistProfile::ai();
    let st0 = rig.vehicle.state();
    let p0 = geom::xy(st0.pos);
    let shape = CarShape::from_def(&car.def);
    // Rear-axle path straight back, then a slight kink to the right.
    let rear0 = shape.rear(p0, [0.0, 1.0]);
    let pts = [
        [rear0[0], rear0[1], 0.0],
        [rear0[0], rear0[1] - 30.0, 0.0],
        [rear0[0] + 2.0, rear0[1] - 40.0, 0.0],
    ];
    let mut drv = ReverseDriver::new(shape, &pts, 2.0);
    let mut input = Default::default();
    let mut status = ManoeuvreStatus::Running;
    let mut max_off = 0.0f32;
    for i in 0..((60.0 / PHYSICS_DT) as usize) {
        let st = rig.vehicle.state();
        if i % 4 == 0 {
            let (inp, s) = drv.step(&st, 4.0 * PHYSICS_DT);
            input = inp;
            status = s;
            let (pos, f) = wp_world::drive::pose_2d(&st);
            let (_, _, d) = drv.path.project(shape.rear(pos, f), None);
            max_off = max_off.max(d);
            if s != ManoeuvreStatus::Running {
                break;
            }
        }
        rig.step(&input, &assists);
    }
    let st = rig.vehicle.state();
    println!(
        "reverse: {status:?}, end {:?}, max off-path {max_off:.2} m",
        st.pos
    );
    assert_eq!(status, ManoeuvreStatus::Done);
    assert!(max_off < 0.8, "{max_off}");
    let f = wp_world::drive::forward_of(st.rot);
    assert!(f[1] > 0.9, "still facing forward: {f:?}");
}
