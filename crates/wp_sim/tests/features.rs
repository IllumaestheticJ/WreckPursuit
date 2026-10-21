//! Solid city features (road format v4) as static colliders: a car driven into a wall stops
//! and the hit reaches damage; a car parked on a parking deck 6 m up rests on it; a car driven
//! up a 1:7 ramp onto the deck climbs. Built on the small procedural grid with its terrain
//! replaced by a flat field that extends west of the city, where the features stand.
//!
//! The 50 k-box benchmark is `#[ignore]`d:
//! `cargo test --release -p wp_sim --test features -- --ignored --nocapture`.

use wp_contracts::{load_car, load_mode_table, EntityId, SimEvent, TimeOfDay, VehicleInput};
use wp_sim::{Sim, SimConfig};
use wp_world::metro::ctx::{box_on, ramp};
use wp_world::network::{Feature, FeatureKind, FeatureShape, Material, RoadNetwork};
use wp_world::procgen::{generate, ProcGenConfig};
use wp_world::terrain::Heightfield;
use wp_world::{TrafficConfig, World, WorldConfig};

fn data(path: &str) -> String {
    format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
}

/// West edge of the procedural city + margin: the test yard's x, m.
fn yard_x(net: &RoadNetwork) -> f32 {
    net.bounds_min[0] - 250.0
}

/// The small grid with a flat z = 0 field reaching 1.3 km west of it (and ±1.1 km north and
/// south), plus `features` (placed by the caller relative to [`yard_x`]).
fn network(features: impl FnOnce(f32) -> Vec<Feature>) -> RoadNetwork {
    let mut net = generate(&ProcGenConfig {
        nx: 4,
        ny: 4,
        ..ProcGenConfig::default()
    });
    let (lo, hi) = (net.bounds_min, net.bounds_max);
    net.terrain = Some(Heightfield::flat(
        [lo[0] - 1300.0, -1100.0],
        [hi[0] + 50.0, 1100.0],
        10.0,
        0.0,
    ));
    net.features = features(yard_x(&net));
    net
}

fn sim_with(net: RoadNetwork) -> Sim {
    let mode = load_mode_table(data("modes/arcade.toml")).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed: 4,
        mode,
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 10.0,
    });
    sim.set_world(World::new(
        net,
        WorldConfig {
            traffic: TrafficConfig {
                enabled: false,
                ..Default::default()
            },
            parked_cars: false,
            ..Default::default()
        },
    ));
    sim
}

fn drive(
    sim: &mut Sim,
    id: EntityId,
    input: VehicleInput,
    seconds: f32,
    mut each: impl FnMut(&mut Sim),
) {
    sim.set_input(id, input).unwrap();
    for _ in 0..(seconds * 240.0) as usize {
        sim.step_fixed();
        each(sim);
    }
}

fn hold() -> VehicleInput {
    VehicleInput {
        handbrake: 1.0,
        brake: 1.0,
        ..Default::default()
    }
}

/// Chassis height above flat ground at rest.
fn rest_height() -> f32 {
    let car = load_car(data("cars/debug_sedan.json")).unwrap();
    let mut s = sim_with(network(|_| Vec::new()));
    let x = yard_x(&s.world().network().clone());
    let id = s.spawn_vehicle(&car, [x, 0.0, 0.0], 0.0).unwrap();
    s.place_on_ground(id, [x, 0.0], 0.0, 0.0, None).unwrap();
    drive(&mut s, id, hold(), 2.0, |_| {});
    s.vehicle_states()[0].1.pos[2]
}

/// Deck: 12 × 30 m, top at 6 m, centred at (x, 100). Ramp: 1:7, 42 m long, from (x, 13, 0)
/// up to the deck's south edge (x, 85, 6).
const DECK_TOP: f32 = 6.0;
fn deck_and_ramp(x: f32) -> Vec<Feature> {
    vec![
        box_on(
            FeatureKind::ParkingDeck,
            Material::Concrete,
            [x, 100.0],
            DECK_TOP - 0.3,
            [12.0, 30.0, 0.3],
            0.0,
            true,
        ),
        ramp(
            FeatureKind::ParkingRamp,
            Material::Concrete,
            [x, 43.0, 0.0],
            [x, 85.0, DECK_TOP],
            6.0,
            0.3,
        ),
    ]
}

#[test]
fn features_are_counted_and_old_cities_get_none() {
    let s = sim_with(network(|x| {
        let mut f = deck_and_ramp(x);
        // A non-solid feature (no collider) and a cylinder + prism.
        f.push(box_on(
            FeatureKind::ParkingPaint,
            Material::PaintWhite,
            [x, 0.0],
            0.0,
            [0.1, 5.0, 0.02],
            0.0,
            false,
        ));
        f.push(Feature {
            kind: FeatureKind::Tank,
            material: Material::White,
            shape: FeatureShape::Cylinder {
                base: [x + 30.0, 0.0, 0.0],
                radius: 4.0,
                height: 10.0,
            },
            solid: true,
        });
        f.push(Feature {
            kind: FeatureKind::Hangar,
            material: Material::Steel,
            shape: FeatureShape::Prism {
                footprint: vec![
                    [x + 60.0, 0.0],
                    [x + 80.0, 0.0],
                    [x + 80.0, 20.0],
                    [x + 60.0, 20.0],
                ],
                base_z: 0.0,
                top_z: 9.0,
            },
            solid: true,
        });
        f
    }));
    let c = s.city_colliders();
    assert_eq!(c.features, 4, "{c:?}");
    assert!(
        c.feature_colliders >= 1 && c.feature_colliders <= 4,
        "{c:?}"
    );
    // The procedural grid without features keeps its old collider set.
    let plain = sim_with(generate(&ProcGenConfig {
        nx: 4,
        ny: 4,
        ..ProcGenConfig::default()
    }));
    assert_eq!(plain.city_colliders().features, 0);
    assert_eq!(plain.city_colliders().feature_colliders, 0);
}

#[test]
fn a_car_driven_into_a_wall_stops_and_takes_damage() {
    let car = load_car(data("cars/debug_sedan.json")).unwrap();
    // A 30 m long, 2 m tall concrete wall across the car's path, 25 m north of it.
    let wall_y = 25.0;
    let mut s = sim_with(network(|x| {
        vec![box_on(
            FeatureKind::Wall,
            Material::Concrete,
            [x, wall_y],
            -0.3,
            [0.6, 30.0, 2.3],
            core::f32::consts::FRAC_PI_2,
            true,
        )]
    }));
    // Yaw π/2 turns the box's local +Y (its 30 m length) to −X: it lies across the y axis.
    let x = yard_x(s.world().network());
    let id = s.spawn_vehicle(&car, [x, 0.0, 0.0], 0.0).unwrap();
    s.place_on_ground(id, [x, 0.0], 0.0, 20.0, None).unwrap();
    let health0 = s.vehicle_states()[0].1.health;
    let mut hit = false;
    let mut max_y = f32::MIN;
    drive(
        &mut s,
        id,
        VehicleInput {
            throttle: 0.5,
            ..Default::default()
        },
        3.0,
        |s| {
            let ev = s.drain_events();
            for e in &ev {
                if let SimEvent::Contact {
                    point, impulse_ns, ..
                } = e
                {
                    if std::env::var("WP_DEBUG").is_ok() {
                        println!("contact at {point:?} {impulse_ns}");
                    }
                }
            }
            hit |= ev.iter().any(|e| {
                matches!(e, SimEvent::Contact { b, impulse_ns, .. }
                    if *b == wp_contracts::WORLD_ENTITY && *impulse_ns > 500.0)
            });
            max_y = max_y.max(s.vehicle_states()[0].1.pos[1]);
        },
    );
    let st = s.vehicle_states()[0].1.clone();
    println!(
        "wall: max y {max_y:.2}, end y {:.2}, speed {:.2}, health {health0} -> {}",
        st.pos[1], st.speed_mps, st.health
    );
    assert!(hit, "no hard contact with the wall");
    // The car's nose (≈ 2.4 m ahead of its centre) never got through the wall face.
    assert!(
        max_y < wall_y - 0.3,
        "drove through the wall: max y {max_y}"
    );
    assert!(st.speed_mps.abs() < 6.0, "still moving at {}", st.speed_mps);
    assert!(st.health < health0, "no damage: {health0} -> {}", st.health);
}

#[test]
fn a_car_parked_on_a_deck_rests_on_it() {
    let rest = rest_height();
    let car = load_car(data("cars/debug_sedan.json")).unwrap();
    let mut s = sim_with(network(deck_and_ramp));
    let x = yard_x(s.world().network());
    let id = s
        .spawn_vehicle(&car, [x, 100.0, DECK_TOP + rest + 0.05], 0.0)
        .unwrap();
    drive(&mut s, id, hold(), 3.0, |_| {});
    let st = s.vehicle_states()[0].1.clone();
    println!(
        "deck: chassis z {:.3} (rest {:.3} above {DECK_TOP})",
        st.pos[2], rest
    );
    assert!(
        (st.pos[2] - (DECK_TOP + rest)).abs() < 0.15,
        "chassis at {} (deck top {DECK_TOP}, rest {rest})",
        st.pos[2]
    );
    assert!(st.speed_mps.abs() < 0.3, "moving at {}", st.speed_mps);
}

#[test]
fn a_car_driven_up_the_ramp_climbs_onto_the_deck() {
    let rest = rest_height();
    let car = load_car(data("cars/debug_sedan.json")).unwrap();
    let mut s = sim_with(network(deck_and_ramp));
    let x = yard_x(s.world().network());
    let id = s.spawn_vehicle(&car, [x, 20.0, 0.0], 0.0).unwrap();
    s.place_on_ground(id, [x, 20.0], 0.0, 8.0, None).unwrap();
    let z0 = s.vehicle_states()[0].1.pos[2];
    let mut best = (f32::MIN, 0.0f32);
    drive(
        &mut s,
        id,
        VehicleInput {
            throttle: 0.45,
            ..Default::default()
        },
        7.0,
        |s| {
            let p = s.vehicle_states()[0].1.pos;
            if p[2] > best.0 {
                best = (p[2], p[1]);
            }
        },
    );
    let st = s.vehicle_states()[0].1.clone();
    println!(
        "ramp: z0 {z0:.2}, max z {:.2} at y {:.1}, end {:?}, speed {:.1}",
        best.0, best.1, st.pos, st.speed_mps
    );
    assert!(best.0 > z0 + 3.0, "did not climb: {z0} -> {}", best.0);
    // It reached the deck (top at 6 m): chassis at deck + rest height somewhere on it.
    assert!(
        best.0 > DECK_TOP + rest - 0.3 && best.1 > 85.0,
        "never reached the deck: max z {} at y {}",
        best.0,
        best.1
    );
}

#[test]
#[ignore = "benchmark: cargo test --release -p wp_sim --test features -- --ignored --nocapture"]
fn bench_fifty_thousand_boxes() {
    use std::time::Instant;
    let car = load_car(data("cars/debug_sedan.json")).unwrap();
    // 50 k solid boxes (2 × 6 × 2.5 m "containers" / wall pieces) on a 224 × 224 grid every
    // 9 m over the flat yard, leaving a 30 m wide lane along x = yard_x clear.
    let net = network(|x0| {
        let mut out = Vec::new();
        for j in 0..224 {
            for i in 0..224 {
                let x = x0 - 1000.0 + i as f32 * 9.0;
                if (x - x0).abs() < 15.0 {
                    continue;
                }
                out.push(box_on(
                    FeatureKind::Container,
                    Material::Red,
                    [x, -1000.0 + j as f32 * 9.0],
                    0.0,
                    [2.0, 6.0, 2.5],
                    (i * 7 + j) as f32 * 0.1,
                    true,
                ));
            }
        }
        out
    });
    let n = net.features.len();
    let t0 = Instant::now();
    let mut s = sim_with(net);
    let build = t0.elapsed();
    println!(
        "{n} boxes: set_world (incl. city + colliders) {:.0} ms, {:?}",
        build.as_secs_f64() * 1e3,
        s.city_colliders()
    );
    let x = yard_x(s.world().network());
    let id = s.spawn_vehicle(&car, [x, -500.0, 0.0], 0.0).unwrap();
    s.place_on_ground(id, [x, -500.0], 0.0, 20.0, None).unwrap();
    s.set_input(
        id,
        VehicleInput {
            throttle: 0.5,
            ..Default::default()
        },
    )
    .unwrap();
    for _ in 0..240 {
        s.step_fixed();
    }
    let t1 = Instant::now();
    let steps = 2400;
    for _ in 0..steps {
        s.step_fixed();
    }
    let per = t1.elapsed().as_secs_f64() * 1e3 / steps as f64;
    println!("one car among them: {per:.3} ms per 240 Hz step");
    // Baseline: the same city without features.
    let mut b = sim_with(network(|_| Vec::new()));
    let id = b.spawn_vehicle(&car, [x, -500.0, 0.0], 0.0).unwrap();
    b.place_on_ground(id, [x, -500.0], 0.0, 20.0, None).unwrap();
    b.set_input(
        id,
        VehicleInput {
            throttle: 0.5,
            ..Default::default()
        },
    )
    .unwrap();
    for _ in 0..240 {
        b.step_fixed();
    }
    let t2 = Instant::now();
    for _ in 0..steps {
        b.step_fixed();
    }
    let base = t2.elapsed().as_secs_f64() * 1e3 / steps as f64;
    println!("baseline without features: {base:.3} ms per step");
}
