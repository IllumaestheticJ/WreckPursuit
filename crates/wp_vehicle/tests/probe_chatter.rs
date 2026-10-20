//! Tyre load must not chatter tick to tick in a steady corner (docs/integration/grip-balance.md):
//! the wheel probe cylinder rolls with the body, and on smooth ground its tilt alone used to
//! hand the contact to the cylinder sweep, whose rim witness point jittered (0.6–1 kN RMS
//! tick-to-tick load changes at 0.9 g, none when straight; fixed by
//! `wp_vehicle::ground::discount_tilt`).

use std::path::PathBuf;

use wp_contracts::{AssistProfile, LoadedCar, VehicleInput};
use wp_vehicle::rig::Rig;

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    wp_contracts::load_car(data.join("cars").join(format!("{id}.json"))).unwrap()
}

/// RMS of the per-tick change of each wheel's load (N) and the mean body roll proxy
/// (outside minus inside front load, N) over 2–4 s of a held steer input at 60 mph.
fn chatter(id: &str, steer: f32) -> ([f32; 4], f32) {
    let c = car(id);
    let mut rig = Rig::new(&c);
    let v = 26.8;
    rig.teleport([0.0; 3], 0.0, v, 0.0);
    let a = AssistProfile::keyboard_default();
    let mut prev: Option<[f32; 4]> = None;
    let (mut d2, mut n, mut transfer) = ([0.0f64; 4], 0usize, 0.0f64);
    for tick in 0..4 * 240 {
        let err = v - rig.state().speed_mps;
        rig.step(
            &VehicleInput {
                steer: if tick >= 240 { steer } else { 0.0 },
                throttle: (0.25 + 0.35 * err).clamp(0.0, 1.0),
                ..Default::default()
            },
            &a,
        );
        if tick < 2 * 240 {
            continue;
        }
        let fz = rig.vehicle.wheel_loads();
        if let Some(p) = prev {
            for i in 0..4 {
                d2[i] += ((fz[i] - p[i]) as f64).powi(2);
            }
            n += 1;
        }
        transfer += (fz[1] - fz[0]).abs() as f64;
        prev = Some(fz);
    }
    (
        std::array::from_fn(|i| (d2[i] / n as f64).sqrt() as f32),
        (transfer / n as f64) as f32,
    )
}

#[test]
fn tyre_load_is_smooth_in_a_steady_corner() {
    for id in ["rampart_50", "shiro_turbo", "kestrel_ts"] {
        let (straight, _) = chatter(id, 0.0);
        let (corner, transfer) = chatter(id, -0.6);
        // The corner really loads the outside wheels (so the body rolls).
        assert!(transfer > 2000.0, "{id}: load transfer {transfer} N");
        for i in 0..4 {
            assert!(
                straight[i] < 5.0,
                "{id} wheel {i}: straight chatter {} N",
                straight[i]
            );
            // Before the fix: 600–1000 N. A smooth corner changes the load by a few N a tick.
            assert!(
                corner[i] < 60.0,
                "{id} wheel {i}: corner chatter {} N",
                corner[i]
            );
        }
    }
}
