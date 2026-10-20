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
        ("deck", [-1524.0, 91.0], Some(1000.0)),
        ("under the deck", [-1524.0, 91.0], Some(-1000.0)),
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
