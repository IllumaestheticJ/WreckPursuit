//! Owner 2026-10-27 "the loop9 k rails launch cars too easily". A car driven into a concrete
//! median barrier stays on the ground: across a spread of speeds and angles it neither rolls
//! past 45° nor leaves the road. Before the fix the wheel probes read a K-rail's top edge as
//! ground: a tyre swept onto it got 0.8 m of sudden suspension travel and the car was thrown
//! up and over the barrier (≈ 1 hit in 7 on Loop 9 flipped or launched the car, even at 5°).

use std::sync::OnceLock;

use wp_contracts::{load_car, load_mode_table, TimeOfDay, VehicleInput};
use wp_sim::{Sim, SimConfig};
use wp_world::geom::{self, V3};
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

fn world() -> &'static World {
    static W: OnceLock<World> = OnceLock::new();
    W.get_or_init(|| {
        World::new(
            net().clone(),
            WorldConfig {
                traffic: TrafficConfig {
                    enabled: false,
                    ..Default::default()
                },
                parked_cars: false,
                ..Default::default()
            },
        )
    })
}

fn wrap(a: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    (a + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI
}

/// The first straight 120 m stretch (< 2° of bend) of every median barrier line, up to `n`.
fn sites(n: usize) -> Vec<(Vec<V3>, f32)> {
    let mut out = Vec::new();
    for line in wp_world::surface::median_barriers(net()) {
        let cum = geom::cumulative(&line);
        let total = *cum.last().unwrap();
        let mut s = 40.0;
        while s + 120.0 < total {
            let (_, d0) = geom::point_at(&line, &cum, s);
            let (_, d1) = geom::point_at(&line, &cum, s + 120.0);
            if wrap(geom::yaw_of(d1) - geom::yaw_of(d0)).abs() < 2f32.to_radians() {
                out.push((line.clone(), s));
                break;
            }
            s += 20.0;
        }
        if out.len() == n {
            break;
        }
    }
    out
}

/// Worst tilt off vertical (°) and time with the centre of mass more than 1.2 m above the
/// barrier foot (s) of a car driven at `speed` m/s and `angle_deg` into the barrier from its
/// `side` (+1 right, -1 left), holding a little throttle.
fn hit(line: &[V3], s0: f32, side: f32, speed: f32, angle_deg: f32) -> (f32, f32) {
    let car = load_car(data("cars/debug_sedan.json")).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed: 4,
        mode: load_mode_table(data("modes/arcade.toml")).unwrap(),
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 10.0,
    });
    sim.set_world(world().clone());
    let cum = geom::cumulative(line);
    let (p, d) = geom::point_at(line, &cum, s0);
    let r = geom::right_of(d);
    let a = angle_deg.to_radians();
    // Reach the barrier ≈ 0.4 s in.
    let off = 1.45 + speed * 0.4 * a.sin();
    let back = speed * 0.4 * a.cos();
    let start = [
        p[0] + r[0] * off * side - d[0] * back,
        p[1] + r[1] * off * side - d[1] * back,
    ];
    let yaw = geom::yaw_of(d) + side * a;
    let id = sim
        .spawn_vehicle(&car, [start[0], start[1], p[2]], yaw)
        .unwrap();
    sim.place_on_ground(id, start, yaw, speed, Some(p[2]))
        .unwrap();
    sim.set_input(
        id,
        VehicleInput {
            throttle: 0.3,
            ..Default::default()
        },
    )
    .unwrap();
    let (mut tilt, mut air) = (0.0f32, 0usize);
    for _ in 0..(2.0 * 240.0) as usize {
        sim.step_fixed();
        sim.drain_events();
        let st = &sim.vehicle_states()[0].1;
        let pr = geom::project(line, &cum, [st.pos[0], st.pos[1]]);
        let (foot, _) = geom::point_at(line, &cum, pr.s);
        let [x, y, _, _] = st.rot;
        let up_z = 1.0 - 2.0 * (x * x + y * y);
        tilt = tilt.max(up_z.clamp(-1.0, 1.0).acos().to_degrees());
        if st.pos[2] - foot[2] > 1.2 {
            air += 1;
        }
    }
    (tilt, air as f32 / 240.0)
}

#[test]
fn cars_driven_into_a_median_barrier_stay_on_the_ground() {
    let mut runs = Vec::new();
    for (line, s0) in sites(3) {
        for side in [1.0, -1.0] {
            for (speed, angle) in [(20.0, 30.0), (35.0, 5.0), (35.0, 15.0), (50.0, 15.0)] {
                let (tilt, air) = hit(&line, s0, side, speed, angle);
                eprintln!(
                    "s {s0:.0} side {side:+} {speed} m/s {angle}°: tilt {tilt:.0}° air {air:.2} s"
                );
                runs.push((s0, side, speed, angle, tilt, air));
            }
        }
    }
    assert!(
        runs.len() >= 16,
        "too few barrier stretches: {}",
        runs.len()
    );
    let bad: Vec<_> = runs.iter().filter(|r| r.4 > 45.0 || r.5 > 0.1).collect();
    assert!(bad.is_empty(), "launched or rolled: {bad:?}");
}
