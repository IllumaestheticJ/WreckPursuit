//! Handbrake behaviour (owner rows 56 and 72, 2026-10-20). Same in Arcade and Simulation: the
//! vehicle model never reads the damage mode.
//! - Row 72: "I'm unable to make quick direction changes when hitting the handbrake". The
//!   steering assists and ESC stand down while the handbrake is held and for 0.5 s after; the
//!   handbrake now locks the rears on every car (data: ≥ 1.5 × the static lock torque).
//! - Row 56: with an automatic clutch or box, the handbrake also declutches, so the rears lock,
//!   the engine neither drags them nor stalls, the gear is kept, and drive resumes on release.

use wp_contracts::{AssistProfile, LoadedCar};
use wp_vehicle::metrics::{handbrake_pull, handbrake_turn};

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap()
}

/// RWD (muscle, sports), FWD (sedan, compact), AWD (on-demand sports, police interceptor).
const CARS: [&str; 6] = [
    "rampart_50",
    "hakone_gt",
    "kestrel_ts",
    "vireo_xs",
    "lindwurm_4t",
    "sentinel_wolfhound",
];

#[test]
fn handbrake_turns_rotate_and_plain_steering_stays_planted() {
    // 60 km/h, 50 % steer, handbrake 0.6 s, default assists on. Measured: 94–120° in 1.5 s
    // (keyboard 36–46° before the fix), body slip 60–89°; the same steer without the
    // handbrake turns 42–51° with ≤ 8° slip.
    for id in CARS {
        let c = car(id);
        for (name, a) in [
            ("keyboard", AssistProfile::keyboard_default()),
            ("gamepad", AssistProfile::gamepad_default()),
        ] {
            let hb = handbrake_turn(&c, &a, 60.0 / 3.6, 0.5, 0.6, 1.5);
            let plain = handbrake_turn(&c, &a, 60.0 / 3.6, 0.5, 0.0, 1.5);
            assert!(hb.yaw_deg >= 70.0, "{id} {name}: {hb:?}");
            assert!(
                hb.rear_omega_end < 0.5,
                "{id} {name}: rears not locked {hb:?}"
            );
            assert!(
                plain.yaw_deg < 60.0 && plain.max_slip_deg < 10.0,
                "{id} {name} without handbrake: {plain:?}"
            );
        }
    }
}

#[test]
fn handbrake_declutches_and_drive_resumes() {
    // 60 km/h in 3rd, handbrake 1 s, release with half throttle. Automatic box, semi-auto
    // (manual shifting, automatic clutch) and a manual clutch pedal left up. Measured: rears
    // stop in 0.12–0.15 s, engine ≥ 1150 rpm throughout (idle 600–800), 3rd kept, speed
    // +0.3–1.7 m/s in the second after release.
    let kb = AssistProfile::keyboard_default();
    for id in CARS {
        let c = car(id);
        let idle = c.def.engine.idle_rpm;
        for (name, a) in [
            ("automatic", kb),
            (
                "semi-auto",
                AssistProfile {
                    auto_gears: false,
                    ..kb
                },
            ),
        ] {
            let r = handbrake_pull(&c, &a, 60.0 / 3.6, 3, 1.0);
            assert!(
                r.rear_lock_s.is_some_and(|t| t <= 0.3),
                "{id} {name}: {r:?}"
            );
            assert!(
                r.min_rpm_held >= idle && r.min_rpm_after >= idle,
                "{id} {name}: {r:?}"
            );
            assert_eq!(r.gear, (3, 3), "{id} {name}: gear kept");
            assert!(r.speed_gain_after > 0.0, "{id} {name}: drive resumes {r:?}");
        }
    }
}
