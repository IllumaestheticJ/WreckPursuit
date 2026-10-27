//! Drive cars into Loop 9's concrete barriers (median and edge K-rails) at a spread of speeds
//! and angles and report how far each one is thrown up (owner 2026-10-27 "the loop9 k rails
//! launch cars too easily").
//!
//! `cargo run --release -p wp_sim --example krail_launch` (CAR=cars/x.json, SITES=n).
use wp_contracts::{load_car, load_mode_table, TimeOfDay, VehicleInput};
use wp_sim::{Sim, SimConfig};
use wp_world::geom;
use wp_world::vesper::ashpeak;
use wp_world::{TrafficConfig, World, WorldConfig};

fn data(path: &str) -> String {
    format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
}

fn wrap(a: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    (a + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI
}

fn main() {
    let world = World::ashpeak(
        &ashpeak::default_config(),
        WorldConfig {
            traffic: TrafficConfig {
                enabled: false,
                ..Default::default()
            },
            parked_cars: false,
            ..Default::default()
        },
    );
    let names: Vec<String> = world.road_names().to_vec();
    let net = world.map().net.clone();
    let loop9: Vec<[f32; 2]> = net
        .lanes
        .iter()
        .filter(|l| names.get(l.road as usize).map(|s| s.as_str()) == Some("Loop 9"))
        .flat_map(|l| l.polyline.iter().map(|p| [p[0], p[1]]))
        .collect();
    let near_loop9 = |q: [f32; 2]| loop9.iter().any(|p| geom::dist(*p, q) < 12.0);
    let mut lines: Vec<(&str, Vec<[f32; 3]>)> = Vec::new();
    for l in wp_world::surface::median_barriers(&net) {
        lines.push(("median", l));
    }
    for l in wp_world::surface::edge_barriers(world.map()) {
        lines.push(("edge", l));
    }
    // Straight stretches next to Loop 9: 120 m with < 2° of bend, one per line.
    let per_kind: usize = std::env::var("SITES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    let mut sites = Vec::new();
    let mut counts = std::collections::BTreeMap::new();
    for (k, (kind, line)) in lines.iter().enumerate() {
        let cum = geom::cumulative(line);
        let total = *cum.last().unwrap();
        let mut s = 40.0;
        while s + 120.0 < total {
            let (p, d0) = geom::point_at(line, &cum, s);
            let (_, d1) = geom::point_at(line, &cum, s + 120.0);
            let (_, dm) = geom::point_at(line, &cum, s + 60.0);
            let bend = wrap(geom::yaw_of(d1) - geom::yaw_of(d0)).abs()
                + wrap(geom::yaw_of(dm) - geom::yaw_of(d0)).abs();
            if bend < 2f32.to_radians() && near_loop9([p[0], p[1]]) {
                let c = counts.entry(*kind).or_insert(0usize);
                if *c < per_kind {
                    *c += 1;
                    sites.push((k, s));
                }
                break;
            }
            s += 20.0;
        }
    }
    let car_path = std::env::var("CAR").unwrap_or_else(|_| "cars/debug_sedan.json".into());
    let car = load_car(data(&car_path)).unwrap();
    let mode = load_mode_table(data("modes/arcade.toml")).unwrap();
    println!("car {car_path}, {} sites", sites.len());
    println!(
        "kind   site                 side  v   ang | rise_m vz_max tilt_deg rate_dps air_s end_v"
    );
    let mut worst: f32 = 0.0;
    let (mut bad, mut runs) = (0usize, 0usize);
    for &(k, s0) in &sites {
        let (kind, line) = &lines[k];
        let cum = geom::cumulative(line);
        for side in [1.0f32, -1.0] {
            for speed in [20.0f32, 35.0, 50.0] {
                for ang in [5.0f32, 15.0, 30.0, 50.0] {
                    let mut sim = Sim::new(SimConfig {
                        seed: 4,
                        mode: mode.clone(),
                        time_of_day: TimeOfDay::from_hours(12.0),
                        time_ratio: 10.0,
                    });
                    sim.set_world(world.clone());
                    let (p, d) = geom::point_at(line, &cum, s0);
                    let r = geom::right_of(d);
                    // Start so the car reaches the barrier ~0.4 s in.
                    let off = 0.25 + 1.2 + speed * 0.4 * ang.to_radians().sin();
                    let back = speed * 0.4 * ang.to_radians().cos();
                    let start = [
                        p[0] + r[0] * off * side - d[0] * back,
                        p[1] + r[1] * off * side - d[1] * back,
                    ];
                    let yaw = geom::yaw_of(d) + side * ang.to_radians();
                    let Ok(id) = sim.spawn_vehicle(&car, [start[0], start[1], p[2]], yaw) else {
                        continue;
                    };
                    if sim
                        .place_on_ground(id, start, yaw, speed, Some(p[2]))
                        .is_err()
                    {
                        continue;
                    }
                    sim.set_input(
                        id,
                        VehicleInput {
                            throttle: 0.3,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                    let (mut rise, mut vz, mut roll, mut pitch, mut air) =
                        (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0usize);
                    let mut end_v = 0.0;
                    for _ in 0..(2.5 * 240.0) as usize {
                        sim.step_fixed();
                        sim.drain_events();
                        let st = &sim.vehicle_states()[0].1;
                        let pr = geom::project(line, &cum, [st.pos[0], st.pos[1]]);
                        let (gp, _) = geom::point_at(line, &cum, pr.s);
                        // Centre of mass above the barrier foot, less its ride height.
                        rise = rise.max(st.pos[2] - gp[2] - 0.5);
                        vz = vz.max(st.lin_vel[2]);
                        // Tilt of the body's up axis off vertical.
                        let [x, y, _, _] = st.rot;
                        let upz = 1.0 - 2.0 * (x * x + y * y);
                        let tilt = upz.clamp(-1.0, 1.0).acos().to_degrees();
                        roll = roll.max(tilt);
                        pitch =
                            pitch.max(st.ang_vel[0].abs().max(st.ang_vel[1].abs()).to_degrees());
                        if st.pos[2] - gp[2] > 1.2 {
                            air += 1;
                        }
                        end_v = st.speed_mps;
                    }
                    worst = worst.max(rise);
                    if roll > 45.0 || air > 24 {
                        bad += 1;
                    }
                    runs += 1;
                    println!(
                        "{kind:6} {:4}@{:5.0} ({:6.0},{:5.0}) {:+.0}  {speed:3.0} {ang:3.0} | {rise:5.2} {vz:5.2} {roll:6.1} {pitch:7.1} {:5.2} {end_v:5.1}",
                        k,
                        s0,
                        p[0],
                        p[1],
                        side,
                        air as f32 / 240.0
                    );
                }
            }
        }
    }
    println!("worst rise {worst:.2} m; {bad}/{runs} hits rolled > 45° or airborne > 0.1 s");
}
