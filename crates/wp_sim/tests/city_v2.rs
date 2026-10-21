//! Port Vesper v2 in the sim loop: terrain, road, building and deck-rail colliders replace the
//! flat slab; cars spawned anywhere settle on the surface under them (hill road, banked
//! highway, bridge deck, the road under it, a junction, open terrain), parked cars rest asleep,
//! median barriers and yard fences stop cars, and the run is deterministic.

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
    assert!(c.fences > 3000, "{c:?}");
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

/// A car driven off the street at a backyard fence is stopped by it.
#[test]
fn a_yard_fence_stops_a_car() {
    use wp_world::geom;
    let car = load_car(data("cars/debug_sedan.json")).unwrap();
    let n = net();
    let t = n.terrain.as_ref().unwrap();
    let blocked = |a: [f32; 2], b: [f32; 2], skip: usize| {
        n.buildings.iter().any(|bd| {
            let fp = &bd.footprint;
            (0..fp.len()).any(|i| geom::segments_intersect(a, b, fp[i], fp[(i + 1) % fp.len()]))
                || geom::point_in_polygon(b, fp)
        }) || n.fences.iter().enumerate().any(|(k, f)| {
            k != skip
                && f.line
                    .windows(2)
                    .any(|w| geom::segment_distance(a, b, geom::xy(w[0]), geom::xy(w[1])) < 1.5)
        })
    };
    // A wooden fence with 15 m of open, level lawn in front of its middle.
    let (k, mid, normal) = n
        .fences
        .iter()
        .enumerate()
        .filter(|(_, f)| f.kind == wp_world::FenceKind::Wood)
        .find_map(|(k, f)| {
            let (a, b) = (geom::xy(f.line[0]), geom::xy(*f.line.last().unwrap()));
            if geom::dist(a, b) < 12.0 {
                return None;
            }
            let m = geom::scale(geom::add(a, b), 0.5);
            let r = geom::right_of(geom::normalize(geom::sub(b, a)));
            [1.0f32, -1.0].into_iter().find_map(|side| {
                let nrm = geom::scale(r, side);
                let start = geom::add(m, geom::scale(nrm, 15.0));
                let level = (t.height_at(start) - t.height_at(m)).abs() < 0.4;
                let lane = near_lane(start);
                (level && !lane && t.height_at(start) > 1.0 && !blocked(start, m, k))
                    .then_some((k, m, nrm))
            })
        })
        .expect("an open fence");
    let start = geom::add(mid, geom::scale(normal, 15.0));
    let yaw = geom::yaw_of(geom::scale(normal, -1.0));
    let mut s = sim(true);
    let id = s
        .spawn_vehicle(&car, [start[0], start[1], 0.0], yaw)
        .unwrap();
    s.place_on_ground(id, start, yaw, 11.0, None).unwrap();
    let ahead = |p: [f32; 3]| geom::dot(geom::sub([p[0], p[1]], mid), normal);
    let mut hit = false;
    for _ in 0..(3.0 * 240.0) as usize {
        s.set_input(
            id,
            VehicleInput {
                throttle: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
        s.step_fixed();
        hit |= s
            .drain_events()
            .iter()
            .any(|e| matches!(e, wp_contracts::SimEvent::Contact { .. }));
        let p = s.vehicle_states()[0].1.pos;
        assert!(ahead(p) > 0.0, "drove through fence {k} at {mid:?}: {p:?}");
    }
    assert!(hit, "no contact with fence {k}");
}

/// No lane within 6 m of `p` (keeps the test car off the street).
fn near_lane(p: [f32; 2]) -> bool {
    net().lanes.iter().any(|l| {
        l.polyline.windows(2).any(|w| {
            wp_world::geom::point_segment_distance(
                p,
                wp_world::geom::xy(w[0]),
                wp_world::geom::xy(w[1]),
            ) < 6.0
        })
    })
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
