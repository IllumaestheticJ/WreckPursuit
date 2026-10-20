//! Per-car sim cost with N driven cars, in Port Vesper and on flat ground (release):
//! `cargo run --release -p simcli --example vehicle_cost -- [city|flat|both] [N ...]`
//! (default: both, 1 8 24).
//!
//! Cars (sports, sedan, muscle, police cruiser, cycled) start at 15 m/s; a speed controller
//! holds 15 m/s, steering straight. In the city each car starts on its own long straight lane
//! (no traffic, no police: only the vehicles, their damage and Rapier run); on flat ground they
//! drive side by side 6 m apart. After 1 s of warm-up, 4 s are timed with every system and 4 s
//! with the vehicle system off (Rapier still steps the bodies), so "vehicle" below is the
//! vehicle model + wheel probes. Numbers quoted in `docs/integration/sim-perf.md`.

use std::time::Instant;

use simcli::data::FsData;
use simcli::run::{build_sim, RunOptions};
use wp_contracts::VehicleInput;
use wp_sim::Systems;

const CARS: [&str; 4] = ["hakone_gt", "kestrel_ts", "rampart_50", "sentinel_cruiser"];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let which = args.first().cloned().unwrap_or_else(|| "both".into());
    let mut counts: Vec<usize> = args.iter().skip(1).filter_map(|s| s.parse().ok()).collect();
    if counts.is_empty() {
        counts = vec![1, 8, 24];
    }
    let data = FsData::new(std::path::PathBuf::from("data"));
    let city_lanes = straight_lanes(&data, 24);
    for place in ["city", "flat"] {
        if which != "both" && which != place {
            continue;
        }
        for &n in &counts {
            run(&data, place, n, &city_lanes);
        }
    }
}

/// Start poses (x, y, z, yaw) on the longest straight lane segments of Port Vesper, at least
/// 60 m apart.
fn straight_lanes(data: &FsData, n: usize) -> Vec<[f32; 4]> {
    let city = simcli::scenario::CitySpec::vesper();
    let world = simcli::data::load_world(data, &city, Default::default())
        .expect("city")
        .expect("not flat");
    let mut segs: Vec<(f32, [f32; 4])> = Vec::new();
    for lane in &world.network().lanes {
        for w in lane.polyline.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len = (dx * dx + dy * dy).sqrt();
            if len > 80.0 && (b[2] - a[2]).abs() < 0.05 * len {
                let yaw = wp_math::atan2(-dx, dy);
                segs.push((
                    len,
                    [a[0] + dx / len * 5.0, a[1] + dy / len * 5.0, a[2], yaw],
                ));
            }
        }
    }
    segs.sort_by(|x, y| y.0.total_cmp(&x.0));
    let mut out: Vec<[f32; 4]> = Vec::new();
    for (_, p) in segs {
        if out
            .iter()
            .all(|q| (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) > 60.0 * 60.0)
        {
            out.push(p);
            if out.len() == n {
                break;
            }
        }
    }
    out
}

fn run(data: &FsData, place: &str, n: usize, lanes: &[[f32; 4]]) {
    let mut entities = String::new();
    for i in 0..n {
        let (x, y, z, yaw) = if place == "city" {
            let p = lanes[i % lanes.len()];
            (p[0], p[1], p[2], p[3])
        } else {
            ((i as f32 - n as f32 * 0.5) * 6.0, 0.0, 0.0, 0.0)
        };
        entities.push_str(&format!(
            "(id: \"c{i}\", car: \"{}\", pos: ({x}, {y}, {z}), yaw: {yaw}, speed_mps: 15.0),\n",
            CARS[i % CARS.len()]
        ));
    }
    let city = if place == "city" {
        "city: preset(name: \"vesper_v2\", seed: 1),\n    traffic_density: 0.0,\n    police_cars: false,"
    } else {
        ""
    };
    let ron = format!(
        "(name: \"vehicle_cost\", seed: 1, {city} assists: ai, entities: [{entities}], duration_s: 10.0)"
    );
    let sc = simcli::Scenario::from_ron("vehicle_cost", &ron).expect("scenario");
    let built = build_sim(&sc, data, &RunOptions::default()).expect("build");
    let (mut sim, ids) = (built.sim, built.ids);
    let drive = |sim: &mut wp_sim::Sim| {
        let speeds: Vec<f32> = sim
            .vehicle_states()
            .iter()
            .map(|(_, s)| s.speed_mps)
            .collect();
        for (id, v) in ids.iter().zip(speeds) {
            let e = 15.0 - v;
            let input = VehicleInput {
                throttle: (e * 0.3).clamp(0.0, 1.0),
                brake: (-e * 0.3).clamp(0.0, 1.0),
                ..Default::default()
            };
            let _ = sim.set_input(*id, input);
        }
    };
    for _ in 0..240 {
        drive(&mut sim);
        sim.step_fixed();
    }
    let mut time = |sys: Systems| {
        sim.set_systems(sys);
        let t = Instant::now();
        for _ in 0..960 {
            drive(&mut sim);
            sim.step_fixed();
        }
        t.elapsed().as_secs_f64() * 1e3 / 240.0
    };
    let all = time(Systems::ALL);
    let mut off = Systems::ALL;
    off.vehicle = false;
    let no_vehicle = time(off);
    println!(
        "{place:4} {n:2} cars: {all:6.3} ms/frame all, {no_vehicle:6.3} vehicles off → vehicle \
         {:.3} ms/frame = {:.4} ms/car/frame",
        all - no_vehicle,
        (all - no_vehicle) / n as f64
    );
}
