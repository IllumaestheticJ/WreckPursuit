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

/// RWD (muscle, sports), FWD (sedan, compact, the tired beater), AWD (on-demand sports, police
/// interceptor).
const CARS: [&str; 7] = [
    "rampart_50",
    "hakone_gt",
    "kestrel_ts",
    "vireo_xs",
    "lindwurm_4t",
    "sentinel_wolfhound",
    "junkyard_beater",
];

#[test]
fn handbrake_turns_rotate_and_plain_steering_stays_planted() {
    // 60 km/h, 50 % steer, handbrake 0.6 s, default assists on. Measured: 92–123° in 1.5 s
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
            // The engine holds at least idle through the pull and the release: the automatic
            // clutch opens (also mid-downshift) before scrubbed driven wheels drag it down.
            // (junkyard_beater dipped to 750 rpm, idle 850, before; 572 with H's data.)
            assert!(
                hb.min_rpm >= c.def.engine.idle_rpm - 1.0,
                "{id} {name}: engine below idle {hb:?}"
            );
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

#[test]
fn the_automatic_box_still_engages_first_on_the_handbrake_at_a_standstill() {
    // The AI's crawl (`wp_world::manoeuvre::Crawl`) holds the handbrake and touches the
    // throttle at a standstill to engage first after reversing: the declutch keeps the gear
    // only while moving, so the box still shifts out of reverse here.
    use wp_contracts::VehicleInput;
    use wp_vehicle::rig::Rig;
    let a = AssistProfile::ai();
    for id in ["debug_sedan", "sentinel_cruiser"] {
        let mut rig = Rig::new(&car(id));
        // Brake held at a standstill selects reverse.
        for _ in 0..240 {
            rig.step(
                &VehicleInput {
                    brake: 0.7,
                    ..Default::default()
                },
                &a,
            );
        }
        assert_eq!(rig.state().gear, -1, "{id}: reverse selected");
        for _ in 0..240 {
            rig.step(
                &VehicleInput {
                    throttle: 0.15,
                    handbrake: 1.0,
                    ..Default::default()
                },
                &a,
            );
        }
        assert_eq!(rig.state().gear, 1, "{id}: first engaged on the handbrake");
    }
}

/// Keyboard preset as the owner drives it (TCS / ESC off, 75 % steering speed sensitivity).
fn owner_keys() -> AssistProfile {
    AssistProfile {
        tcs: false,
        esc: false,
        steer_speed_sensitivity: 0.75,
        ..AssistProfile::keyboard_default()
    }
}

#[test]
fn a_handbrake_stab_on_the_throttle_gives_the_drive_straight_back() {
    // Owner row 109 / reference clip 5: with the throttle and the key held, a 0.25 s handbrake
    // stab adds rotation and the drive is back at once when it ends. Measured at 80 km/h:
    // driven-wheel force positive again 0.07–0.08 s after the release on RWD, at once on FWD
    // (the engine revs up while the rear axle is freed and goes straight back into the drive).
    // (Holding the engine at road-speed revs during the stab was tried: 0.18–0.20 s, rejected.)
    use wp_contracts::VehicleInput;
    use wp_vehicle::rig::Rig;
    for id in ["rampart_50", "shiro_turbo", "kestrel_ts"] {
        let c = car(id);
        let a = owner_keys();
        let mut rig = Rig::new(&c);
        rig.teleport([0.0; 3], 0.0, 80.0 / 3.6, 0.0);
        for _ in 0..240 {
            rig.step(
                &VehicleInput {
                    throttle: 0.3,
                    ..Default::default()
                },
                &a,
            );
        }
        for _ in 0..60 {
            rig.step(
                &VehicleInput {
                    steer: -1.0,
                    throttle: 1.0,
                    handbrake: 1.0,
                    ..Default::default()
                },
                &a,
            );
        }
        let rwd = !matches!(c.def.drivetrain.layout, wp_contracts::DriveLayout::Fwd);
        let mut back = None;
        for t in 0..60 {
            rig.step(
                &VehicleInput {
                    steer: -1.0,
                    throttle: 1.0,
                    ..Default::default()
                },
                &a,
            );
            let fx = rig.vehicle.telemetry().fx;
            let drive = if rwd { fx[2] + fx[3] } else { fx[0] + fx[1] };
            if back.is_none() && drive > 0.0 {
                back = Some(t as f32 / 240.0);
            }
        }
        let back = back.unwrap_or(f32::INFINITY);
        assert!(back <= 0.1, "{id}: drive back after {back} s");
    }
}

#[test]
fn a_hairpin_pivot_is_caught_with_throttle_and_opposite_lock() {
    // Reference clip 2: key into the turn + handbrake 1.0 s off the throttle, then throttle +
    // opposite lock. Measured at 25 km/h: 52–65° by the release (the reference pivots
    // 150–180°: see docs/integration/vehicle.md, row 109), the rotation stops 0.22–0.28 s after
    // the release, body slip after the release ≤ 27°: caught, no donut.
    use wp_contracts::VehicleInput;
    use wp_vehicle::rig::Rig;
    for id in ["shiro_turbo", "rampart_50", "kestrel_ts"] {
        let c = car(id);
        let a = owner_keys();
        let mut rig = Rig::new(&c);
        rig.teleport([0.0; 3], 0.0, 25.0 / 3.6, 0.0);
        for _ in 0..120 {
            let s = rig.state().speed_mps;
            rig.step(
                &VehicleInput {
                    throttle: if s < 25.0 / 3.6 { 0.3 } else { 0.0 },
                    ..Default::default()
                },
                &a,
            );
        }
        let mut stopped = None;
        for t in 0..480 {
            let (steer, hb, thr) = if t < 240 {
                (1.0, 1.0, 0.0)
            } else {
                (-1.0, 0.0, 1.0)
            };
            rig.step(
                &VehicleInput {
                    steer,
                    handbrake: hb,
                    throttle: thr,
                    ..Default::default()
                },
                &a,
            );
            // Steering right turns clockwise (yaw rate < 0); caught once it is back to ≥ 0.
            if t >= 240 && stopped.is_none() && rig.state().ang_vel[2] >= 0.0 {
                stopped = Some((t - 239) as f32 / 240.0);
            }
        }
        let s = stopped.unwrap_or(f32::INFINITY);
        assert!(s < 0.5, "{id}: rotation stopped after {s} s");
        assert!(rig.state().speed_mps > 2.0, "{id}: drives out");
    }
}
