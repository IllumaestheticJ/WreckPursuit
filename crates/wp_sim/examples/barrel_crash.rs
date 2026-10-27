//! Drive a car head-on into Loop 9's blunt barrier ends, with and without the water barrels in
//! front of them (`wp_world::barrier_ends`), and compare the hits: peak deceleration, speed
//! time from the first contact to < 2 m/s, health left, roll.
//!
//! `cargo run --release -p wp_sim --example barrel_crash` (CAR=cars/x.json, SPEED=m/s, N=ends).
use wp_contracts::{load_car, load_mode_table, SimEvent, TimeOfDay, VehicleInput};
use wp_sim::{Sim, SimConfig};
use wp_world::geom;
use wp_world::props::PropKind;
use wp_world::vesper::ashpeak;
use wp_world::{TrafficConfig, World, WorldConfig};

fn data(path: &str) -> String {
    format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
}

fn main() {
    let speed: f32 = std::env::var("SPEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(40.0);
    let count: usize = std::env::var("N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(12);
    let car_path = std::env::var("CAR").unwrap_or_else(|_| "cars/debug_sedan.json".into());
    let car = load_car(data(&car_path)).unwrap();
    let mode = load_mode_table(data("modes/arcade.toml")).unwrap();
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
    let mut bare = world.clone();
    bare.retain_props(|p| p.kind != PropKind::WaterBarrel);
    let names: Vec<String> = world.road_names().to_vec();
    let net = world.map().net.clone();
    let ends: Vec<_> = wp_world::barrier_ends::exposed_ends(world.map())
        .into_iter()
        .filter(|e| {
            names
                .get(net.lanes[e.lane].road as usize)
                .map(|s| s.as_str())
                == Some("Loop 9")
        })
        .collect();
    let step = (ends.len() / count.max(1)).max(1);
    println!(
        "car {car_path} at {speed} m/s; {} Loop 9 ends, every {step}th",
        ends.len()
    );
    println!("end                      kind   | barrels: peak_g stop_s health tilt | bare: peak_g stop_s health tilt");
    let only: Option<f32> = std::env::var("ONLY_X").ok().and_then(|v| v.parse().ok());
    for e in ends
        .iter()
        .step_by(step)
        .filter(|e| only.is_none_or(|x| (e.pos[0] - x).abs() < 1.0))
    {
        let pts = &net.lanes[e.lane].polyline;
        let cum = geom::cumulative(pts);
        let pr = geom::project(pts, &cum, geom::xy(e.pos));
        // Along the lane, lined up on the end.
        let (_, dir) = geom::point_at(pts, &cum, pr.s);
        let back = speed * 1.0 + 8.0;
        let start = [e.pos[0] - dir[0] * back, e.pos[1] - dir[1] * back];
        let yaw = geom::yaw_of(dir);
        let mut row = format!("({:7.1},{:7.1}) {:6} |", e.pos[0], e.pos[1], e.kind.name());
        for w in [&world, &bare] {
            let mut sim = Sim::new(SimConfig {
                seed: 4,
                mode: mode.clone(),
                time_of_day: TimeOfDay::from_hours(12.0),
                time_ratio: 10.0,
            });
            sim.set_world(w.clone());
            let id = sim
                .spawn_vehicle(&car, [start[0], start[1], e.pos[2]], yaw)
                .unwrap();
            sim.place_on_ground(id, start, yaw, speed, Some(e.pos[2]))
                .unwrap();
            sim.set_input(id, VehicleInput::default()).unwrap();
            let mut hist: Vec<f32> = Vec::new();
            let (mut peak, mut tilt) = (0.0f32, 0.0f32);
            let mut hit_at: Option<usize> = None;
            let mut stop_s = f32::NAN;
            let mut health = 1.0f32;
            for k in 0..(4.0 * 240.0) as usize {
                sim.step_fixed();
                let hit = sim
                    .drain_events()
                    .iter()
                    .any(|ev| matches!(ev, SimEvent::Contact { a, b, .. } if *a == id || *b == id));
                let st = &sim.vehicle_states()[0].1;
                let v = (st.lin_vel[0].powi(2) + st.lin_vel[1].powi(2)).sqrt();
                hist.push(v);
                if std::env::var("TRACE").is_ok()
                    && hit_at.is_some_and(|h| k < h + 200)
                    && k % 4 == 0
                {
                    let d = geom::dist(geom::xy(e.pos), [st.pos[0], st.pos[1]]);
                    eprintln!("t{k} v {v:5.1} to end {d:5.2} z {:.2}", st.pos[2]);
                }
                if hit && hit_at.is_none() {
                    hit_at = Some(k);
                }
                if hist.len() > 12 {
                    // Mean deceleration over 0.05 s, in g.
                    let dv = hist[hist.len() - 13] - v;
                    peak = peak.max(dv / 0.05 / 9.81);
                }
                let [x, y, _, _] = st.rot;
                tilt = tilt.max(
                    (1.0 - 2.0 * (x * x + y * y))
                        .clamp(-1.0, 1.0)
                        .acos()
                        .to_degrees(),
                );
                if let Some(h) = hit_at {
                    if stop_s.is_nan() && v < 2.0 {
                        stop_s = (k - h) as f32 / 240.0;
                    }
                }
                health = st.health;
            }
            row += &format!("  {peak:6.1} {stop_s:6.2} {health:6.2} {tilt:5.0} |");
        }
        println!("{row}");
    }
}
