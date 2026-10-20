//! Debug trace for `wp_vehicle::ride`: per-tick wheel compression/load over a kerb.
//! `cargo run --release -p wp_vehicle --example ride_trace -- <car> <kmh> <kerb_m>`
use wp_contracts::{VehicleInput, PHYSICS_DT};
use wp_vehicle::ride::{crest_collider, kerb_collider, ride_assists};
use wp_vehicle::rig::{ground_collider, FlatWorld, Rig};
use wp_vehicle::VehicleExtras;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let id = a.first().map(String::as_str).unwrap_or("rampart_50");
    let kmh: f32 = a.get(1).map_or(30.0, |s| s.parse().unwrap());
    let h: f32 = a.get(2).map_or(0.10, |s| s.parse().unwrap());
    let car = wp_contracts::load_car(format!(
        "{}/../../data/cars/{id}.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let v = kmh / 3.6;
    let start = (v * 4.0f32).max(20.0);
    let mut rig = Rig::with_world(
        &car,
        VehicleExtras::default(),
        FlatWorld::default(),
        [0.0, -start, 0.0],
        0.0,
    );
    let crest = std::env::var("CREST").is_ok();
    if crest {
        rig.set_city_ground([ground_collider(), crest_collider(0.03, 30.0, 0.0)]);
    } else {
        rig.set_city_ground([ground_collider(), kerb_collider(h, 0.0)]);
    }
    let (y0, y1) = if crest { (-3.0, 40.0) } else { (-3.5, 4.0) };
    rig.teleport([0.0, -start, 0.0], 0.0, v, 0.0);
    let asst = ride_assists();
    for k in 0..((start / v + 2.0) / PHYSICS_DT) as usize {
        let err = v - rig.state().speed_mps;
        let input = VehicleInput {
            throttle: (0.25 + 0.35 * err).clamp(0.0, 1.0),
            ..Default::default()
        };
        rig.step(&input, &asst);
        let ch = rig.vehicle.chassis();
        if ch.pos.y > y0 && ch.pos.y < y1 {
            let s = rig.state();
            let w = &s.wheel_states;
            println!(
                "{k} y {:.3} z {:.3} vz {:.2} | x {:.3} {:.3} {:.3} {:.3} | fz {:.0} {:.0} {:.0} {:.0} | c {}{}{}{}",
                ch.pos.y, ch.com.z, ch.lin_vel.z,
                w[0].compression_m, w[1].compression_m, w[2].compression_m, w[3].compression_m,
                w[0].fz_n, w[1].fz_n, w[2].fz_n, w[3].fz_n,
                w[0].contact as u8, w[1].contact as u8, w[2].contact as u8, w[3].contact as u8,
            );
        }
    }
}
