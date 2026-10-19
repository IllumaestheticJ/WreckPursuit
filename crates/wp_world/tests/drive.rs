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
    let start = pois
        .iter()
        .position(|p| p.kind == PoiKind::RaceStartGrid)
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
        .plan_circuit(&[[-220.0, -220.0], [220.0, -220.0], [220.0, 220.0], [-220.0, 220.0]])
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
    assert!(ct_civ < 1.5 && ct_pur < 2.0);
}
