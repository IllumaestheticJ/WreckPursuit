//! Port Vesper v2 in the sim loop: terrain, road, building and deck-rail colliders replace the
//! flat slab; cars spawned anywhere settle on the surface under them (hill road, banked
//! highway, bridge deck, the road under it, a junction, open terrain), parked cars rest asleep
//! and the run is deterministic.

use std::sync::OnceLock;

use wp_contracts::{load_car, load_mode_table, EntityId, TimeOfDay, VehicleInput};
use wp_sim::{Sim, SimConfig};
use wp_world::network::RoadNetwork;
use wp_world::vesper::{generate, VesperConfig};
use wp_world::{TrafficConfig, World, WorldConfig};

fn data(path: &str) -> String {
    format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
}

fn net() -> &'static RoadNetwork {
    static NET: OnceLock<RoadNetwork> = OnceLock::new();
    NET.get_or_init(|| generate(&VesperConfig::default()))
}

fn sim(city: bool) -> Sim {
    let mode = load_mode_table(data("modes/arcade.toml")).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed: 4,
        mode,
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 10.0,
    });
    if city {
        sim.set_world(World::new(
            net().clone(),
            WorldConfig {
                traffic: TrafficConfig {
                    enabled: false,
                    ..Default::default()
                },
                parked_cars: false,
                ..Default::default()
            },
        ));
    }
    sim
}

fn hold(sim: &mut Sim, id: EntityId, seconds: f32) {
    sim.set_input(
        id,
        VehicleInput {
            handbrake: 1.0,
            ..Default::default()
        },
    )
    .unwrap();
    for _ in 0..(seconds * 240.0) as usize {
        sim.step_fixed();
    }
}

/// Chassis height above the ground when resting on flat ground.
fn rest_height() -> f32 {
    let car = load_car(data("cars/debug_sedan.json")).unwrap();
    let mut s = sim(false);
    let id = s.spawn_vehicle(&car, [0.0, 0.0, 0.0], 0.0).unwrap();
    hold(&mut s, id, 2.0);
    s.vehicle_states()[0].1.pos[2]
}

/// Points to test: (name, xy, near_z hint).
fn spots() -> Vec<(&'static str, [f32; 2], Option<f32>)> {
    let n = net();
    let hollins = &n
        .pois
        .iter()
        .find(|p| p.name == "Hollins Ascent")
        .unwrap()
        .pos;
    let loop_grid = &n
        .pois
        .iter()
        .find(|p| p.name == "Loop 9 sprint")
        .unwrap()
        .pos;
    let junction = n.nodes[n.intersections[0].node as usize].pos;
    vec![
        ("hill road", [hollins[0], hollins[1]], Some(hollins[2])),
        ("loop", [loop_grid[0], loop_grid[1]], Some(loop_grid[2])),
        ("deck", [-1510.0, 92.0], Some(1000.0)),
        ("under the deck", [-1510.0, 92.0], Some(-1000.0)),
        ("junction", [junction[0], junction[1]], None),
        ("hillside", [-1000.0, 900.0], None),
    ]
}

#[test]
fn city_ground_colliders_are_built() {
    let s = sim(true);
    let c = s.city_colliders();
    assert!(c.terrain_vertices > 100_000, "{c:?}");
    assert!(c.road_triangles > 10_000, "{c:?}");
    assert!(c.buildings > 2000, "{c:?}");
    assert!(c.rails > 10, "{c:?}");
    assert!(c.barriers > 900, "{c:?}");
    assert!(s.has_terrain());
    // A flat city keeps the slab.
    assert_eq!(
        sim(false).city_colliders(),
        wp_sim::CityColliders::default()
    );
}

#[test]
fn cars_settle_on_the_surface_everywhere() {
    let rest = rest_height();
    let car = load_car(data("cars/debug_sedan.json")).unwrap();
    let mut s = sim(true);
    let mut ids = Vec::new();
    for (k, (_, xy, near)) in spots().iter().enumerate() {
        let id = s
            .spawn_vehicle(&car, [xy[0], xy[1], 0.0], 0.3 * k as f32)
            .unwrap();
        s.place_on_ground(id, *xy, 0.3 * k as f32, 0.0, *near)
            .unwrap();
        ids.push(id);
    }
    for id in &ids {
        s.set_input(
            *id,
            VehicleInput {
                handbrake: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
    }
    for _ in 0..480 {
        s.step_fixed();
    }
    let states = s.vehicle_states();
    for ((name, xy, near), (_, st)) in spots().iter().zip(&states) {
        let ground = s.world().map().ground_height(*xy, *near);
        let above = st.pos[2] - ground;
        println!("{name}: ground {ground:.2}, chassis {:.2} ({above:.3} above, flat rest {rest:.3}), speed {:.2}", st.pos[2], st.speed_mps);
        assert!(
            (above - rest).abs() < 0.2,
            "{name}: chassis {above} m above ground (rest {rest})"
        );
        assert!(
            st.speed_mps.abs() < 0.5,
            "{name}: still moving at {}",
            st.speed_mps
        );
    }
}

#[test]
fn the_median_barrier_keeps_a_car_on_its_carriageway() {
    let car = load_car(data("cars/debug_sedan.json")).unwrap();
    let mut s = sim(true);
    let grid = net()
        .pois
        .iter()
        .find(|p| p.name == "Loop 9 sprint")
        .unwrap()
        .clone();
    // From the left lane, swerve 25° towards the barrier at 90 km/h.
    let f = wp_world::geom::dir_of_yaw(grid.heading_rad);
    let left = [-f[1], f[0]];
    let start = [
        grid.pos[0] + left[0] * 7.4 + f[0] * 30.0,
        grid.pos[1] + left[1] * 7.4 + f[1] * 30.0,
    ];
    let yaw = grid.heading_rad + 25f32.to_radians();
    let id = s
        .spawn_vehicle(&car, [start[0], start[1], 0.0], yaw)
        .unwrap();
    s.place_on_ground(id, start, yaw, 25.0, Some(grid.pos[2]))
        .unwrap();
    let lines = wp_world::surface::median_barriers(net());
    let side = |p: [f32; 3]| {
        let q = [p[0], p[1]];
        lines
            .iter()
            .map(|l| {
                let cum = wp_world::geom::cumulative(l);
                wp_world::geom::project(l, &cum, q)
            })
            .min_by(|a, b| a.dist.total_cmp(&b.dist))
            .unwrap()
    };
    let before = side(s.vehicle_states()[0].1.pos);
    assert!(before.lateral > 2.0 && before.dist < 10.0, "{before:?}");
    let mut hit = false;
    for _ in 0..(3.0 * 240.0) as usize {
        s.step_fixed();
        hit |= s
            .drain_events()
            .iter()
            .any(|e| matches!(e, wp_contracts::SimEvent::Contact { .. }));
        let pr = side(s.vehicle_states()[0].1.pos);
        assert!(pr.lateral > 0.0, "crossed the median: {pr:?}");
    }
    assert!(hit, "no contact with the barrier");
}

#[test]
fn hilly_city_run_is_deterministic() {
    let car = load_car(data("cars/debug_sedan.json")).unwrap();
    let run = || {
        let mut s = sim(true);
        let (_, xy, near) = spots()[0];
        let id = s.spawn_vehicle(&car, [xy[0], xy[1], 0.0], 0.0).unwrap();
        let yaw = net()
            .pois
            .iter()
            .find(|p| p.name == "Hollins Ascent")
            .unwrap()
            .heading_rad;
        s.place_on_ground(id, xy, yaw, 8.0, near).unwrap();
        s.set_input(
            id,
            VehicleInput {
                throttle: 0.4,
                ..Default::default()
            },
        )
        .unwrap();
        let z0 = s.vehicle_states()[0].1.pos[2];
        for _ in 0..480 {
            s.step_fixed();
        }
        let st = s.vehicle_states()[0].1.clone();
        // It drove off up the hill road.
        assert!(st.speed_mps > 5.0, "speed {}", st.speed_mps);
        assert!(st.pos[2] > z0, "z {} -> {}", z0, st.pos[2]);
        s.state_hash()
    };
    assert_eq!(run(), run());
}

#[test]
#[ignore = "3800 parked cars in unoptimised Rapier (~80 s): cargo test --release -p wp_sim --test city_v2 -- --ignored"]
fn parked_cars_rest_asleep_on_the_road_mesh() {
    let mode = load_mode_table(data("modes/arcade.toml")).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed: 4,
        mode,
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 10.0,
    });
    sim.set_world(World::new(
        net().clone(),
        WorldConfig {
            traffic: TrafficConfig {
                enabled: false,
                ..Default::default()
            },
            ..Default::default()
        },
    ));
    assert!(sim.prop_count() > 3000);
    for _ in 0..480 {
        sim.step_fixed();
    }
    // Thousands of parked cars on sloped, banked roads must not keep the solver busy.
    assert!(
        sim.awake_body_count() < 10,
        "{} bodies awake",
        sim.awake_body_count()
    );
    assert!(
        sim.resting_prop_count() + 5 >= sim.prop_count(),
        "{} of {} props left the resting group",
        sim.prop_count() - sim.resting_prop_count(),
        sim.prop_count()
    );
}

/// Every lane through the Hollins Tunnel (both carriageways, all three lanes) and over the
/// Iron Bridge's truss, driven at the limit: no contact with the lining, portals, aprons,
/// truss or terrain at the mouths, and the car stays on its lane.
#[test]
fn lanes_through_the_tunnel_and_the_truss_are_clear() {
    use wp_world::drive::{DrivePath, FollowParams, PathFollower};
    use wp_world::network::BridgeKind;
    let car = load_car(data("cars/debug_sedan.json")).unwrap();
    let n = net();
    let tunnel = &n.features.tunnels[0];
    let iron = n
        .features
        .bridges
        .iter()
        .find(|b| b.kind == BridgeKind::Truss)
        .unwrap();
    // (path, speed): the lane from 120 m before the structure to 60 m after it.
    let mut runs: Vec<(Vec<[f32; 3]>, f32)> = Vec::new();
    for l in n.lanes.iter().filter(|l| l.kind.drivable()) {
        let inside: Vec<usize> = (0..l.polyline.len())
            .filter(|k| {
                let p = l.polyline[*k];
                tunnel.contains([p[0], p[1], p[2] + 1.0])
                    || (l.road == iron.surface
                        && iron
                            .centre
                            .iter()
                            .any(|c| wp_world::geom::dist([c[0], c[1]], [p[0], p[1]]) < 8.0))
            })
            .collect();
        let (Some(&a), Some(&b)) = (inside.first(), inside.last()) else {
            continue;
        };
        let cum = wp_world::geom::cumulative(&l.polyline);
        let s0 = (cum[a] - 120.0).max(0.0);
        let s1 = (cum[b] + 60.0).min(cum[cum.len() - 1]);
        let path: Vec<[f32; 3]> = l
            .polyline
            .iter()
            .zip(&cum)
            .filter(|(_, s)| **s >= s0 && **s <= s1)
            .map(|(p, _)| *p)
            .collect();
        runs.push((path, l.speed_limit_mps));
    }
    assert!(runs.len() >= 8, "{} lanes", runs.len());
    let mut s = sim(true);
    let mut cars = Vec::new();
    for (path, speed) in &runs {
        let p = path[0];
        let d = wp_world::geom::sub([path[1][0], path[1][1]], [p[0], p[1]]);
        let yaw = wp_world::geom::yaw_of(wp_world::geom::normalize(d));
        let id = s.spawn_vehicle(&car, [p[0], p[1], p[2]], yaw).unwrap();
        s.place_on_ground(id, [p[0], p[1]], yaw, *speed, Some(p[2]))
            .unwrap();
        let follower = PathFollower::for_vehicle(&car.def, FollowParams::PURSUIT, *speed);
        cars.push((id, DrivePath::new(path, false), follower, *speed, 0.0f32));
    }
    let dt = 1.0 / 240.0;
    for _ in 0..(40.0 * 240.0) as usize {
        let states = s.vehicle_states();
        for (id, path, f, speed, worst) in cars.iter_mut() {
            let st = &states.iter().find(|(e, _)| e == id).unwrap().1;
            let out = f.follow(st, path, *speed, 0.0, dt);
            *worst = worst.max(out.cross_track_m.abs());
            s.set_input(*id, out.input).unwrap();
        }
        s.step_fixed();
        for e in s.drain_events() {
            if let wp_contracts::SimEvent::Contact { point, .. } = e {
                panic!("contact at {point:?}");
            }
        }
    }
    for (id, path, _, _, worst) in &cars {
        let st = &s
            .vehicle_states()
            .into_iter()
            .find(|(e, _)| e == id)
            .unwrap()
            .1;
        let end = path.points().last().copied().unwrap();
        let left = wp_world::geom::dist([st.pos[0], st.pos[1]], [end[0], end[1]]);
        assert!(left < 30.0, "car {id:?} stopped {left} m short");
        // Pure pursuit cuts the tunnel's long sweeper by ~1.7 m at the limit.
        assert!(*worst < 2.5, "car {id:?} {worst} m off its lane");
    }
}

/// Swerving off the right lane towards the verge at 90 km/h, the car hits the outer edge
/// barrier and stays on the carriageway.
#[test]
fn the_edge_barrier_keeps_a_car_off_the_verge() {
    let car = load_car(data("cars/debug_sedan.json")).unwrap();
    let mut s = sim(true);
    let grid = net()
        .pois
        .iter()
        .find(|p| p.name == "Loop 9 sprint")
        .unwrap()
        .clone();
    let f = wp_world::geom::dir_of_yaw(grid.heading_rad);
    let right = [f[1], -f[0]];
    let start = [grid.pos[0] + f[0] * 30.0, grid.pos[1] + f[1] * 30.0];
    let yaw = grid.heading_rad - 25f32.to_radians();
    let id = s
        .spawn_vehicle(&car, [start[0], start[1], 0.0], yaw)
        .unwrap();
    s.place_on_ground(id, start, yaw, 25.0, Some(grid.pos[2]))
        .unwrap();
    let lines = wp_world::surface::edge_barriers(net());
    let near = |p: [f32; 3]| {
        lines
            .iter()
            .map(|l| {
                let cum = wp_world::geom::cumulative(l);
                wp_world::geom::project(l, &cum, [p[0], p[1]])
            })
            .min_by(|a, b| a.dist.total_cmp(&b.dist))
            .unwrap()
    };
    let before = near(s.vehicle_states()[0].1.pos);
    assert!(before.dist < 8.0, "{before:?} (right {right:?})");
    let side = before.lateral.signum();
    let mut hit = false;
    for _ in 0..(3.0 * 240.0) as usize {
        s.step_fixed();
        hit |= s
            .drain_events()
            .iter()
            .any(|e| matches!(e, wp_contracts::SimEvent::Contact { .. }));
        let pr = near(s.vehicle_states()[0].1.pos);
        assert!(pr.lateral * side > 0.0, "went through the barrier: {pr:?}");
    }
    assert!(hit, "no contact with the barrier");
}
