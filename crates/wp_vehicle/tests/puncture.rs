//! Punctured tyres (playtest row 132: "when spikes hit, the car should visibly drop lower").
//! A tyre at deflation `p` is `FLAT_DROP_FRAC × radius × p` smaller for the rolling radius and
//! the ground probe alike, so its corner of the body sits that much lower; a fully flat wheel
//! runs on its rim (less grip, more rolling resistance) and the car stays drivable but pulls
//! toward the flat side. The same in both damage modes (physics).

use wp_contracts::{AssistProfile, HandlingEffects, LoadedCar, VehicleInput};
use wp_vehicle::rig::Rig;
use wp_vehicle::vehicle::FLAT_DROP_FRAC;

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap()
}

/// World z of the body point above wheel `i` (its `position_m`), m.
fn corner_z(rig: &Rig, c: &LoadedCar, i: usize) -> f32 {
    let s = rig.state();
    let [x, y, z, w] = s.rot;
    let p = c.def.wheels[i].position_m;
    // z row of the rotation matrix.
    let rz = [
        2.0 * (x * z - w * y),
        2.0 * (y * z + w * x),
        1.0 - 2.0 * (x * x + y * y),
    ];
    s.pos[2] + rz[0] * p[0] + rz[1] * p[1] + rz[2] * p[2]
}

/// Heading (rad, + = left) of the body.
fn heading(rig: &Rig) -> f32 {
    let [x, y, z, w] = rig.state().rot;
    let fx = 2.0 * (x * y - w * z);
    let fy = 1.0 - 2.0 * (x * x + z * z);
    (-fx).atan2(fy)
}

fn settle(rig: &mut Rig, a: &AssistProfile, s: f32) {
    for _ in 0..(s * 240.0) as usize {
        rig.step(
            &VehicleInput {
                brake: 1.0,
                ..Default::default()
            },
            a,
        );
    }
}

/// Static corner heights `[FL, FR, RL, RR]` with `puncture`.
fn ride(c: &LoadedCar, puncture: [f32; 4]) -> [f32; 4] {
    let a = AssistProfile::default();
    let mut rig = Rig::new(c);
    rig.set_effects(&HandlingEffects {
        tire_puncture: puncture,
        ..HandlingEffects::NEUTRAL
    });
    settle(&mut rig, &a, 3.0);
    std::array::from_fn(|i| corner_z(&rig, c, i))
}

#[test]
fn a_flat_tyre_drops_its_corner_by_the_lost_radius() {
    for id in ["debug_sedan", "hakone_gt", "rampart_50", "vireo_xs"] {
        let c = car(id);
        let base = ride(&c, [0.0; 4]);
        for i in [0, 3] {
            let mut p = [0.0; 4];
            p[i] = 1.0;
            let flat = ride(&c, p);
            let want = FLAT_DROP_FRAC * c.def.wheels[i].radius_m;
            let drop = base[i] - flat[i];
            println!(
                "{id} wheel {i}: corner drop {:.1} mm (tyre radius loss {:.1} mm)",
                drop * 1000.0,
                want * 1000.0
            );
            // The body pivots on the other three wheels, so the corner drops a little less
            // than the radius loss (the diagonal corner rises slightly).
            assert!(
                (0.6 * want..=1.1 * want).contains(&drop),
                "{id} wheel {i}: drop {drop} vs radius loss {want}"
            );
        }
        // Half deflated: about half the drop.
        let mut p = [0.0; 4];
        p[0] = 0.5;
        let half = base[0] - ride(&c, p)[0];
        let full = base[0] - ride(&c, [1.0, 0.0, 0.0, 0.0])[0];
        assert!(
            (0.35 * full..=0.65 * full).contains(&half),
            "{id}: half {half} vs full {full}"
        );
    }
}

#[test]
fn a_flat_runs_on_its_rim_and_the_car_pulls_toward_it_but_stays_drivable() {
    let c = car("debug_sedan");
    let a = AssistProfile::keyboard_default();
    let drive = |puncture: [f32; 4]| {
        let mut rig = Rig::new(&c);
        rig.teleport([0.0; 3], 0.0, 20.0, 0.0);
        rig.set_effects(&HandlingEffects {
            tire_puncture: puncture,
            ..HandlingEffects::NEUTRAL
        });
        let h0 = heading(&rig);
        for _ in 0..4 * 240 {
            rig.step(
                &VehicleInput {
                    throttle: 0.4,
                    ..Default::default()
                },
                &a,
            );
        }
        (
            heading(&rig) - h0,
            rig.state().speed_mps,
            rig.vehicle.rim_contact(),
        )
    };
    let (h_ok, v_ok, rim_ok) = drive([0.0; 4]);
    let (h_fl, v_fl, rim_fl) = drive([1.0, 0.0, 0.0, 0.0]);
    let (h_fr, _, _) = drive([0.0, 1.0, 0.0, 0.0]);
    println!(
        "heading change 4 s: none {:.2}°, FL flat {:.2}°, FR flat {:.2}°; speed {v_ok:.1} / {v_fl:.1} m/s",
        h_ok.to_degrees(),
        h_fl.to_degrees(),
        h_fr.to_degrees()
    );
    assert_eq!(rim_ok, [false; 4]);
    assert_eq!(rim_fl, [true, false, false, false], "FL on its rim");
    // Pulls toward the flat side (+ = left), clearly more than the healthy car drifts.
    assert!(
        h_fl - h_ok > 1.0f32.to_radians(),
        "FL flat pulls left: {h_fl} vs {h_ok}"
    );
    assert!(
        h_fr - h_ok < -1.0f32.to_radians(),
        "FR flat pulls right: {h_fr} vs {h_ok}"
    );
    // Still drivable: keeps most of its speed, no spin.
    assert!(v_fl > 0.7 * v_ok, "speed {v_fl} vs {v_ok}");
    assert!(h_fl.abs() < 60.0f32.to_radians(), "no spin: {h_fl}");
}
