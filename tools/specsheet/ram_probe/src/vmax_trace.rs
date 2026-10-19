//! Top speed like `wp_vehicle::metrics::top_speed_kmh`, but stopping at the edge of the rig's
//! ground slab (workstream 09 tool).
//!
//! ```text
//! cargo run --release -p ram-probe --bin vmax-trace -- [--trace] data/cars/<id>.json ...
//! ```
//! The metric runs full throttle from 30 m/s until the speed gains < 0.04 m/s in 2 s or 150 s
//! pass. A slow car (the 207 kW, 2.5 t van) is still gaining at ~95 s, by which time it has
//! covered the 5 km half-length of the rig's ground slab (`rig::GROUND_HALF_EXTENTS`) and falls
//! off the world: the metric then reports the free-fall speed (295 km/h for the van). Same
//! rig, start, assists and stop rule here, plus a stop 200 m before the slab edge. Prints
//! `<id> <km/h> <converged|edge>`; `--trace` also prints speed, gear, rpm and height every
//! second.

use wp_contracts::VehicleInput;
use wp_vehicle::metrics::metric_assists;
use wp_vehicle::rig::{Rig, GROUND_HALF_EXTENTS};

/// Top speed (km/h) and whether the stop rule was met before the slab edge.
fn top_speed(path: &str, trace: bool) -> (String, f32, bool) {
    let car = wp_contracts::load_car(path).expect("car loads");
    let mut rig = Rig::new(&car);
    rig.teleport([0.0; 3], 0.0, 30.0, 0.0);
    let a = metric_assists();
    let input = VehicleInput {
        throttle: 1.0,
        ..Default::default()
    };
    let edge = GROUND_HALF_EXTENTS[1] - 200.0;
    let (mut best, mut last) = (0.0f32, 0.0f32);
    for tick in 0..150 * 240 {
        rig.step(&input, &a);
        let s = rig.state();
        best = best.max(s.speed_mps);
        if trace && tick % 240 == 0 {
            println!(
                "t={:>3} v={:6.2} m/s gear={:?} rpm={:6.0} y={:7.1} z={:6.3}",
                tick / 240,
                s.speed_mps,
                s.gear,
                s.engine_rpm,
                s.pos[1],
                s.pos[2]
            );
        }
        if s.pos[1] > edge {
            return (car.def.id.clone(), best * 3.6, false);
        }
        if tick % (2 * 240) == 0 {
            if tick > 0 && s.speed_mps - last < 0.04 {
                break;
            }
            last = s.speed_mps;
        }
    }
    (car.def.id.clone(), best * 3.6, true)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let trace = args.iter().any(|a| a == "--trace");
    for path in args.iter().filter(|a| !a.starts_with("--")) {
        let (id, kmh, converged) = top_speed(path, trace);
        println!(
            "{id} {kmh:.1} {}",
            if converged { "converged" } else { "edge" }
        );
    }
}
