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
        ("deck", [-1517.0, 91.0], Some(1000.0)),
        ("under the deck", [-1517.0, 91.0], Some(-1000.0)),
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
    assert!(c.tunnel_boxes > 60, "{c:?}");
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

/// A car driven at a tunnel wall stops at it: the Founders Cut walls are solid, and the car
/// inside rests on the road under the deck (not on the roof).
#[test]
fn tunnel_walls_hold_a_car_inside() {
    let c = net()
        .covered
        .iter()
        .find(|c| c.name == "Founders Cut")
        .unwrap();
    let shape = wp_world::covered::CoverShape::new(c);
    let (mid, dir) = wp_world::geom::point_at(&c.centre, &shape.cum, shape.length() * 0.5);
    let car = load_car(data("cars/debug_sedan.json")).unwrap();
    let mut s = sim(true);
    // Facing the right-hand wall (yaw of the right normal), 4 m from the centreline.
    let right = [dir[1], -dir[0]];
    let xy = [mid[0] + right[0] * 4.0, mid[1] + right[1] * 4.0];
    let yaw = wp_math::atan2(-right[0], right[1]);
    let id = s.spawn_vehicle(&car, [xy[0], xy[1], 0.0], yaw).unwrap();
    s.place_on_ground(id, xy, yaw, 0.0, Some(mid[2])).unwrap();
    s.set_input(
        id,
        VehicleInput {
            throttle: 1.0,
            ..Default::default()
        },
    )
    .unwrap();
    let mut max_lat = 0.0f32;
    for _ in 0..(4.0 * 240.0) as usize {
        s.step_fixed();
        let p = s.vehicle_states()[0].1.pos;
        max_lat = max_lat.max(shape.locate([p[0], p[1]]).lateral.abs());
        assert!(
            p[2] < mid[2] + 3.0,
            "car on the roof: z {} road {}",
            p[2],
            mid[2]
        );
    }
    assert!(
        max_lat < c.half_width_m,
        "through the wall: {max_lat} m (wall face at {} m)",
        c.half_width_m
    );
    assert!(
        max_lat > c.half_width_m - 3.0,
        "never reached the wall: {max_lat} m"
    );
}
/// What you see is what you hit (playtest: crashing into invisible map geometry): off the
/// roads, rays against the Rapier terrain land on the greybox's drawn terrain, which lies up to
/// ~10 m below the raw heightfield on slopes; under roads and sidewalks they land on
/// `CityMap::ground`. Sampled off the grid vertices, so cell triangulation must match too.
#[test]
fn solid_terrain_is_the_drawn_terrain() {
    use rapier3d::prelude::{QueryFilter, Ray, Vector};
    let map = wp_world::CityMap::new(net().clone());
    let mut phys = wp_sim::PhysicsWorld::new();
    phys.set_city_ground(&map);
    let raw = net().terrain.as_ref().unwrap();
    let drawn = wp_world::surface::drawn_terrain(&map).unwrap();
    let ground = map.ground.as_ref().unwrap();
    let only_terrain = |_, c: &rapier3d::prelude::Collider| c.shape().as_heightfield().is_some();
    let q = phys.query_pipeline(QueryFilter::default().predicate(&only_terrain));
    let (lo, hi) = (raw.origin, raw.max_corner());
    let (mut off_road, mut worst, mut raw_above) = (0, 0.0f32, 0.0f32);
    let mut y = lo[1] + 1.3;
    while y < hi[1] {
        let mut x = lo[0] + 2.9;
        while x < hi[0] {
            let ray = Ray::new(Vector::new(x, y, 300.0), Vector::new(0.0, 0.0, -1.0));
            let (_, toi) = q
                .cast_ray(&ray, 600.0, true)
                .expect("terrain under every point");
            let hit = 300.0 - toi;
            let want = ground.height_on_triangles([x, y]);
            assert!(
                (hit - want).abs() < 0.02,
                "({x}, {y}): solid {hit}, collision terrain {want}"
            );
            let seen = drawn.height_on_triangles([x, y]);
            if (want - seen).abs() < 1e-3 {
                off_road += 1;
                worst = worst.max((hit - seen).abs());
                raw_above = raw_above.max(raw.height_on_triangles([x, y]) - hit);
            }
            x += 7.3;
        }
        y += 7.3;
    }
    println!("off-road samples {off_road}, worst |solid − drawn| {worst:.4} m, raw terrain up to {raw_above:.1} m above");
    assert!(off_road > 50_000, "{off_road}");
    assert!(worst < 0.02, "{worst}");
}
