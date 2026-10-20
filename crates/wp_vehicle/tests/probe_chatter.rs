//! Tyre load must not chatter tick to tick in a steady corner (docs/integration/grip-balance.md):
//! the wheel probe cylinder rolls with the body, and on smooth ground its tilt alone used to
//! hand the contact to the cylinder sweep, whose rim witness point jittered (0.6–1 kN RMS
//! tick-to-tick load changes at 0.9 g, none when straight). The fix
//! (`wp_vehicle::ground::discount_tilt`) is behind the grip-balance lab's `probe_tilt_fix` until
//! the vehicle owner makes it the default; this binary only ever sets that one switch.

use std::path::PathBuf;

use wp_contracts::{AssistProfile, LoadedCar, VehicleInput};
use wp_vehicle::grip_lab::{self, GripBalance};
use wp_vehicle::rig::Rig;

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    wp_vehicle::drift_lab_w::load_any_car(&data, id).unwrap()
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
    // Without the fix the bug is there (documents it; the probe as shipped).
    grip_lab::set_override(None);
    let (corner, _) = chatter("rampart_50", -0.6);
    assert!(
        corner.iter().any(|c| *c > 300.0),
        "chatter without the fix: {corner:?}"
    );
    // With the fix.
    grip_lab::set_override(Some(GripBalance {
        probe_tilt_fix: true,
        ..GripBalance::default()
    }));
    for id in ["rampart_50", "parity_rwd_d", "parity_fwd_c"] {
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
