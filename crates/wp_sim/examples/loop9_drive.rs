//! Drive every Loop 9 lane at speed in the full Ashpeak world and report anything a car hits
//! (owner 2026-10-26 "random object on Loop 9 completely stopped me").
use wp_contracts::{load_car, load_mode_table, SimEvent, TimeOfDay};
use wp_sim::{Sim, SimConfig};
use wp_world::drive::{DrivePath, FollowParams, PathFollower};
use wp_world::geom;
use wp_world::vesper::ashpeak;
use wp_world::{TrafficConfig, World, WorldConfig};

fn data(path: &str) -> String {
    format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
}

fn main() {
    let speed: f32 = std::env::var("SPEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(35.0);
    let target = std::env::var("ROAD").unwrap_or_else(|_| "Loop 9".into());
    let world = World::ashpeak(
        &ashpeak::default_config(),
        WorldConfig {
            traffic: TrafficConfig {
                enabled: false,
                ..Default::default()
            },
            parked_cars: true,
            ..Default::default()
        },
    );
    let names: Vec<String> = world.road_names().to_vec();
    let net = world.map().net.clone();
    let on = |l: usize| {
        names.get(net.lanes[l].road as usize).map(|s| s.as_str()) == Some(target.as_str())
    };
    // Chain lanes along the road, keeping the lane index.
    let mut used = vec![false; net.lanes.len()];
    let mut paths: Vec<(usize, Vec<[f32; 3]>)> = Vec::new();
    for start in 0..net.lanes.len() {
        if !on(start) || used[start] {
            continue;
        }
        // Walk back to a chain start (a lane no same-road lane leads into) or loop.
        let mut chain = vec![start];
        used[start] = true;
        let mut cur = start;
        loop {
            let idx = net.lanes[cur].index_from_left;
            let next = net.lanes[cur]
                .connections
                .iter()
                .map(|c| c.to as usize)
                .filter(|&n| on(n))
                .min_by_key(|&n| (net.lanes[n].index_from_left as i32 - idx as i32).abs());
            match next {
                Some(n) if !used[n] => {
                    used[n] = true;
                    chain.push(n);
                    cur = n;
                }
                _ => break,
            }
        }
        let mut pts: Vec<[f32; 3]> = Vec::new();
        for &l in &chain {
            for p in &net.lanes[l].polyline {
                if pts
                    .last()
                    .is_none_or(|q| geom::dist(geom::xy(*q), geom::xy(*p)) > 0.3)
                {
                    pts.push(*p);
                }
            }
        }
        let len = *geom::cumulative(&pts).last().unwrap();
        eprintln!("chain from lane {start}: {} lanes, {len:.0} m", chain.len());
        if len > 200.0 {
            paths.push((start, pts));
        }
    }
    let mode = load_mode_table(data("modes/arcade.toml")).unwrap();
    if std::env::var("CONES").is_ok() {
        // Straight through every crossover cone along the median line.
        paths.clear();
        let cones: Vec<_> = world
            .static_props()
            .filter(|p| p.kind == wp_world::props::PropKind::Cone)
            .copied()
            .collect();
        let xs = wp_world::crossover::crossovers(&net);
        for (k, c) in cones.iter().enumerate() {
            let x = xs
                .iter()
                .min_by(|a, b| {
                    geom::dist(geom::xy(**a), geom::xy(c.pos))
                        .total_cmp(&geom::dist(geom::xy(**b), geom::xy(c.pos)))
                })
                .unwrap();
            let along = geom::normalize(geom::sub(geom::xy(*x), geom::xy(c.pos)));
            let right = geom::right_of(along);
            let side: f32 = if k % 2 == 0 { 1.0 } else { -1.0 };
            let ang: f32 = std::env::var("ANG")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30.0);
            let (ca, sa) = (ang.to_radians().cos(), ang.to_radians().sin());
            let dir = [
                along[0] * ca + right[0] * sa * side,
                along[1] * ca + right[1] * sa * side,
            ];
            let at = |d: f32| {
                [
                    c.pos[0] + dir[0] * d,
                    c.pos[1] + dir[1] * d,
                    c.pos[2] - 0.35,
                ]
            };
            if k % 3 != 2 {
                continue;
            }
            let pts: Vec<[f32; 3]> = (-8..=5).rev().map(|i| at(i as f32 * 5.0)).collect();
            paths.push((k, pts));
        }
    }
    let car = load_car(data(
        &std::env::var("CAR").unwrap_or_else(|_| "cars/debug_sedan.json".into()),
    ))
    .unwrap();
    for (start, path) in &paths {
        let mut s = Sim::new(SimConfig {
            seed: 4,
            mode: mode.clone(),
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 10.0,
        });
        s.set_world(world.clone());
        let p = path[0];
        let d = geom::sub(geom::xy(path[1]), geom::xy(p));
        let yaw = geom::yaw_of(geom::normalize(d));
        let id = s.spawn_vehicle(&car, p, yaw).unwrap();
        s.place_on_ground(id, [p[0], p[1]], yaw, speed, Some(p[2]))
            .unwrap();
        let mut f = PathFollower::for_vehicle(&car.def, FollowParams::PURSUIT, speed);
        let dp = DrivePath::new(path, false);
        let len = *geom::cumulative(path).last().unwrap();
        let mut last_v = speed;
        let mut stuck = 0;
        for t in 0..((len / speed + 5.0) * 240.0) as usize {
            let st = s
                .vehicle_states()
                .into_iter()
                .find(|(e, _)| *e == id)
                .unwrap()
                .1;
            let out = f.follow(&st, &dp, speed, 0.0, 1.0 / 240.0);
            s.set_input(id, out.input).unwrap();
            s.step_fixed();
            let v = (st.lin_vel[0].powi(2) + st.lin_vel[1].powi(2)).sqrt();
            for e in s.drain_events() {
                if let SimEvent::Contact {
                    a,
                    b,
                    point,
                    normal,
                    impulse_ns,
                    rel_speed,
                } = e
                {
                    if a == id || b == id {
                        println!(
                            "chain {start} t={:.2}s v={v:.1} contact a={} b={} at ({:.1},{:.1},{:.1}) n=({:.2},{:.2},{:.2}) J={impulse_ns:.0} rel={rel_speed:.1}",
                            t as f32 / 240.0, a.0, b.0, point[0], point[1], point[2], normal[0], normal[1], normal[2]
                        );
                    }
                }
            }
            if last_v - v > 3.0 * (1.0 / 240.0) * 10.0 {
                println!("chain {start} t={:.2}s sudden decel {last_v:.1}->{v:.1} at ({:.1},{:.1},{:.1})", t as f32 / 240.0, st.pos[0], st.pos[1], st.pos[2]);
            }
            last_v = v;
            if v < 2.0 {
                stuck += 1;
                if stuck > 480 {
                    println!(
                        "chain {start} STOPPED at ({:.1},{:.1},{:.1})",
                        st.pos[0], st.pos[1], st.pos[2]
                    );
                    break;
                }
            }
        }
        let st = s
            .vehicle_states()
            .into_iter()
            .find(|(e, _)| *e == id)
            .unwrap()
            .1;
        let left = geom::dist(geom::xy(st.pos), geom::xy(*path.last().unwrap()));
        println!("chain {start} done, {left:.0} m from end");
    }
}
