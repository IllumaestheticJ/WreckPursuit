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
            // The engine holds idle through the pull and the release: the automatic clutch opens
            // (also mid-downshift, and when a spun car rolls backwards in a forward gear) before
            // the driven wheels drag it down. (junkyard_beater dipped to 750 rpm, idle 850,
            // before; 572 with H's data.) 3 % margin: a torque-converter box sags a few rpm at
            // idle while coupled (sentinel_wolfhound 645 rpm at a 650 rpm idle since the row-109
            // pivot aid makes this turn a ~130° spin).
            assert!(
                hb.min_rpm >= 0.97 * c.def.engine.idle_rpm,
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
        tcs: wp_contracts::TcsLevel::Off,
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
    // opposite lock at once. At 25 km/h the rotation stops 0.43–0.60 s after the release with
    // the row-109 pivot aid (0.22–0.28 s without it, when the car had turned only 52–65°): caught,
    // no donut. (Measured on A's keyboard steering. On Drift Lab C / W's keyboard lineage, which
    // round12 merged into lead/drift-lab-w, the catch is slower: 0.55–0.93 s with the aid,
    // 0.24–0.51 s without; still caught and driving out, so the limit there is 1.0 s.)
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
        assert!(s < 1.0, "{id}: rotation stopped after {s} s");
        assert!(rig.state().speed_mps > 2.0, "{id}: drives out");
    }
}

#[test]
fn the_clip_2_hairpin_pivots_with_the_aid_and_is_caught() {
    // Owner row 109 (owner-approved pivot aid, `wp_vehicle::pivot`): 50 km/h, right key +
    // handbrake 1.0 s off the throttle, then throttle + right 0.8 s, then left (opposite lock) +
    // throttle, as in the reference clip 2. Measured: 139–145° by 1.2 s after the handbrake goes
    // on (67–78° without the aid; the reference 150–180°), 155–200° in all, the rotation calm
    // (< 0.5 rad/s) within 0–0.9 s of the opposite lock, no spin. (On the Drift Lab C / W
    // keyboard lineage: 139–143° by 1.2 s, 159–190° at the opposite lock; the AWD kestrel_ts
    // takes ~1.0 s to calm, so the run is 0.8 s longer than A's.)
    use wp_contracts::VehicleInput;
    use wp_vehicle::rig::Rig;
    let heading = |rig: &Rig| {
        let f = rig.vehicle.chassis().rot * wp_vehicle::math::V3::Y;
        (-f.x).atan2(f.y)
    };
    for id in ["shiro_turbo", "rampart_50", "kestrel_ts"] {
        let c = car(id);
        let a = owner_keys();
        let mut rig = Rig::new(&c);
        rig.teleport([0.0; 3], 0.0, 50.0 / 3.6, 0.0);
        for _ in 0..240 {
            let s = rig.state().speed_mps;
            rig.step(
                &VehicleInput {
                    throttle: if s < 50.0 / 3.6 { 0.4 } else { 0.0 },
                    ..Default::default()
                },
                &a,
            );
        }
        let (mut prev, mut rot) = (heading(&rig), 0.0f32);
        let (mut at_1_2, mut at_catch, mut calm, mut caught) = (0.0f32, 0.0f32, 0usize, None);
        for t in 0..912usize {
            let k = if t < 240 {
                VehicleInput {
                    steer: 1.0,
                    handbrake: 1.0,
                    ..Default::default()
                }
            } else if t < 432 {
                VehicleInput {
                    steer: 1.0,
                    throttle: 1.0,
                    ..Default::default()
                }
            } else {
                VehicleInput {
                    steer: if caught.is_none() { -1.0 } else { 0.0 },
                    throttle: 1.0,
                    ..Default::default()
                }
            };
            rig.step(&k, &a);
            let h = heading(&rig);
            let mut d = h - prev;
            if d > std::f32::consts::PI {
                d -= std::f32::consts::TAU;
            } else if d < -std::f32::consts::PI {
                d += std::f32::consts::TAU;
            }
            rot += d;
            prev = h;
            if t == 287 {
                at_1_2 = rot.abs().to_degrees();
            }
            if t == 431 {
                at_catch = rot.abs().to_degrees();
            }
            if t >= 432 {
                calm = if rig.state().ang_vel[2].abs() < 0.5 {
                    calm + 1
                } else {
                    0
                };
                if caught.is_none() && calm >= 60 {
                    caught = Some((t - 431 - 59) as f32 / 240.0);
                }
            }
        }
        assert!(at_1_2 >= 130.0, "{id}: {at_1_2}° by 1.2 s");
        let c_s = caught.unwrap_or(f32::INFINITY);
        assert!(c_s < 1.2, "{id}: caught after {c_s} s");
        assert!(
            rot.abs().to_degrees() - at_catch < 90.0,
            "{id}: spun on ({at_catch} → {})",
            rot.abs().to_degrees()
        );
    }
}
