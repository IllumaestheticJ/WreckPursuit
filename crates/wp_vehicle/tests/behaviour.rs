//! Behavioural tests: stability, determinism, emergent handling traits, assists, damage
//! feedback. All run in the single-car Rapier rig.

use wp_contracts::{AssistProfile, HandlingEffects, LoadedCar, SimEvent, VehicleInput};
use wp_vehicle::metrics::metric_assists;
use wp_vehicle::rig::Rig;

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap()
}

fn run(rig: &mut Rig, input: VehicleInput, assists: &AssistProfile, seconds: f32) {
    for _ in 0..(seconds * 240.0) as usize {
        rig.step(&input, assists);
    }
}

fn throttle(t: f32) -> VehicleInput {
    VehicleInput {
        throttle: t,
        ..Default::default()
    }
}

#[test]
fn parked_car_settles_without_jitter() {
    for id in ["debug_sedan", "debug_coupe"] {
        let mut rig = Rig::new(&car(id));
        run(
            &mut rig,
            VehicleInput::default(),
            &AssistProfile::keyboard_default(),
            10.0,
        );
        let s = rig.state();
        let v: f32 = s.lin_vel.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!(v < 0.01, "{id} creeping at {v}");
        assert!(
            s.pos[0].abs() < 0.01 && s.pos[1].abs() < 0.01,
            "{id} drifted {:?}",
            s.pos
        );
        assert!(s.wheel_states.iter().all(|w| w.contact));
        // Static loads add up to the weight.
        let fz: f32 = s.wheel_states.iter().map(|w| w.fz_n).sum();
        let weight = rig.vehicle.def().mass_kg * 9.81;
        assert!(
            (fz - weight).abs() / weight < 0.02,
            "{id} fz {fz} vs {weight}"
        );
    }
}

#[test]
fn identical_runs_give_identical_hashes() {
    let c = car("debug_coupe");
    let script = |rig: &mut Rig| {
        let a = AssistProfile::gamepad_default();
        run(rig, throttle(1.0), &a, 3.0);
        run(
            rig,
            VehicleInput {
                steer: 0.7,
                throttle: 0.8,
                ..Default::default()
            },
            &a,
            2.0,
        );
        run(
            rig,
            VehicleInput {
                brake: 1.0,
                steer: -0.3,
                ..Default::default()
            },
            &a,
            2.0,
        );
        rig.state_hash()
    };
    let (mut a, mut b) = (Rig::new(&c), Rig::new(&c));
    assert_eq!(script(&mut a), script(&mut b));
}

#[test]
fn fwd_torque_steer_rwd_none() {
    let heading_after = |id: &str| {
        let mut rig = Rig::new(&car(id));
        run(&mut rig, throttle(1.0), &metric_assists(), 3.0);
        let r = rig.state().rot;
        2.0 * r[2].atan2(r[3])
    };
    let fwd = heading_after("debug_sedan");
    let rwd = heading_after("debug_coupe");
    assert!(
        fwd.abs() > 1e-4,
        "FWD should tug under full throttle: {fwd}"
    );
    assert!(
        rwd.abs() < fwd.abs() * 0.1,
        "RWD should run straight: {rwd} vs {fwd}"
    );
}

#[test]
fn abs_prevents_lockup_and_shortens_stops() {
    let c = car("debug_sedan");
    let stop = |abs: bool| {
        let mut rig = Rig::new(&c);
        rig.teleport([0.0; 3], 0.0, 25.0, 0.0);
        let a = AssistProfile {
            abs,
            ..metric_assists()
        };
        let mut locked_ticks = 0;
        for _ in 0..240 * 6 {
            rig.step(
                &VehicleInput {
                    brake: 1.0,
                    ..Default::default()
                },
                &a,
            );
            let s = rig.state();
            if s.speed_mps > 3.0 && s.wheel_states[0].omega.abs() < 0.1 {
                locked_ticks += 1;
            }
            if s.speed_mps < 0.05 {
                break;
            }
        }
        (rig.state().pos[1], locked_ticks)
    };
    let (d_abs, lock_abs) = stop(true);
    let (d_raw, lock_raw) = stop(false);
    assert!(lock_raw > 100, "without ABS the fronts lock ({lock_raw})");
    assert!(lock_abs < 10, "ABS keeps wheels turning ({lock_abs})");
    assert!(d_abs < d_raw, "ABS {d_abs} vs locked {d_raw}");
}

#[test]
fn handbrake_locks_rears_and_declutches() {
    let mut rig = Rig::new(&car("debug_coupe"));
    rig.teleport([0.0; 3], 0.0, 20.0, 0.0);
    run(
        &mut rig,
        VehicleInput {
            throttle: 0.5,
            handbrake: 1.0,
            ..Default::default()
        },
        &metric_assists(),
        0.5,
    );
    let w = rig.state().wheel_states;
    assert!(
        w[2].omega.abs() < 0.5 && w[3].omega.abs() < 0.5,
        "rears locked {w:?}"
    );
    assert!(w[0].omega > 20.0, "fronts still rolling");
}

#[test]
fn all_assists_off_still_drives() {
    let off = AssistProfile {
        abs: false,
        tcs: false,
        esc: false,
        steer_speed_sensitivity: 0.0,
        countersteer_help: 0.0,
        throttle_filter: 0.0,
        brake_filter: 0.0,
        auto_gears: true,
        auto_clutch: true,
    };
    let mut rig = Rig::new(&car("debug_sedan"));
    run(&mut rig, throttle(0.6), &off, 6.0);
    assert!(rig.state().speed_mps > 15.0);
}

#[test]
fn manual_gearbox_and_clutch() {
    let manual = AssistProfile {
        auto_gears: false,
        auto_clutch: false,
        ..AssistProfile::wheel_default()
    };
    let mut rig = Rig::new(&car("debug_coupe"));
    // Launch in first with the clutch pedal released: anti-stall slip, car moves.
    run(&mut rig, throttle(0.6), &manual, 3.0);
    assert_eq!(rig.state().gear, 1);
    assert!(rig.state().speed_mps > 3.0);
    rig.step(
        &VehicleInput {
            gear_up: true,
            throttle: 0.6,
            ..Default::default()
        },
        &manual,
    );
    run(&mut rig, throttle(0.6), &manual, 1.0);
    assert_eq!(rig.state().gear, 2);
    assert!(rig.state().engine_rpm > 800.0, "engine running");
}

#[test]
fn rev_limiter_holds() {
    let manual = AssistProfile {
        auto_gears: false,
        ..metric_assists()
    };
    let mut rig = Rig::new(&car("debug_sedan"));
    run(&mut rig, throttle(1.0), &manual, 6.0);
    let limit = rig.vehicle.def().engine.rev_limit_rpm;
    assert!(
        rig.state().engine_rpm <= limit + 50.0,
        "rpm {}",
        rig.state().engine_rpm
    );
    assert!(rig.state().engine_rpm > limit - 400.0);
}

#[test]
fn damage_power_and_engine_dead() {
    let c = car("debug_sedan");
    let speed_with = |eff: HandlingEffects| {
        let mut rig = Rig::new(&c);
        rig.set_effects(&eff);
        run(&mut rig, throttle(1.0), &metric_assists(), 4.0);
        rig.state().speed_mps
    };
    let full = speed_with(HandlingEffects::NEUTRAL);
    let half = speed_with(HandlingEffects {
        power_scale: 0.5,
        ..HandlingEffects::NEUTRAL
    });
    let dead = speed_with(HandlingEffects {
        engine_dead: true,
        ..HandlingEffects::NEUTRAL
    });
    assert!(half < full * 0.9, "{half} vs {full}");
    assert!(dead < 0.2, "dead engine moved the car: {dead}");
}

#[test]
fn damage_steer_pull_and_toe_make_the_car_drift() {
    let c = car("debug_sedan");
    let lateral_after = |eff: HandlingEffects| {
        let mut rig = Rig::new(&c);
        rig.teleport([0.0; 3], 0.0, 20.0, 0.0);
        rig.set_effects(&eff);
        run(
            &mut rig,
            throttle(0.3),
            &AssistProfile::wheel_default(),
            3.0,
        );
        rig.state().pos[0]
    };
    let straight = lateral_after(HandlingEffects::NEUTRAL);
    let pulled = lateral_after(HandlingEffects {
        steer_pull: 0.05,
        ..HandlingEffects::NEUTRAL
    });
    // Bent left-front toe-out: car pulls left (−X).
    let mut toe = HandlingEffects::NEUTRAL;
    toe.toe_delta[0] = -0.03;
    let toed = lateral_after(toe);
    assert!(straight.abs() < 0.5, "neutral car drifted {straight}");
    assert!(pulled > straight + 2.0, "pull right: {pulled}");
    assert!(toed < straight - 0.5, "toe pull: {toed}");
}

#[test]
fn damage_lost_wheel_drops_corner_onto_hub() {
    let mut rig = Rig::new(&car("debug_sedan"));
    let mut eff = HandlingEffects::NEUTRAL;
    eff.wheel_lost[1] = true;
    rig.set_effects(&eff);
    run(
        &mut rig,
        VehicleInput::default(),
        &AssistProfile::keyboard_default(),
        3.0,
    );
    let s = rig.state();
    assert!(s.wheel_states[1].detached);
    // Front-right corner dropped: chassis rolled toward +X (right side down).
    let roll = 2.0 * s.rot[1].atan2(s.rot[3]);
    assert!(roll > 0.03, "roll {roll}");
    // Dragging hub: pulls toward the lost corner when driven.
    rig.teleport([0.0; 3], 0.0, 15.0, 0.0);
    run(
        &mut rig,
        throttle(0.3),
        &AssistProfile::wheel_default(),
        2.0,
    );
    assert!(
        rig.state().pos[0] > 0.5,
        "should pull right, x = {}",
        rig.state().pos[0]
    );
}

#[test]
fn damage_brakes_grip_and_puncture() {
    let c = car("debug_sedan");
    let stop = |eff: HandlingEffects| {
        let mut rig = Rig::new(&c);
        rig.teleport([0.0; 3], 0.0, 25.0, 0.0);
        rig.set_effects(&eff);
        run(
            &mut rig,
            VehicleInput {
                brake: 1.0,
                ..Default::default()
            },
            &metric_assists(),
            6.0,
        );
        rig.state().pos[1]
    };
    let base = stop(HandlingEffects::NEUTRAL);
    let weak = stop(HandlingEffects {
        brake_scale: [0.3; 4],
        ..HandlingEffects::NEUTRAL
    });
    let slick = stop(HandlingEffects {
        grip_scale: [0.5; 4],
        ..HandlingEffects::NEUTRAL
    });
    let flat = stop(HandlingEffects {
        tire_puncture: [1.0; 4],
        ..HandlingEffects::NEUTRAL
    });
    assert!(weak > base * 1.5, "{weak} vs {base}");
    assert!(slick > base * 1.4, "{slick} vs {base}");
    assert!(flat > base * 1.2, "{flat} vs {base}");
}

#[test]
fn damage_overheating_fuel_and_gearbox() {
    let c = car("debug_sedan");
    let mut rig = Rig::new(&c);
    let mut eff = HandlingEffects {
        overheating: 1.0,
        ..HandlingEffects::NEUTRAL
    };
    rig.set_effects(&eff);
    run(&mut rig, throttle(0.5), &metric_assists(), 10.0);
    let heat_events = rig
        .events
        .iter()
        .filter(|e| matches!(e, SimEvent::ComponentDamaged { .. }))
        .count();
    assert!(
        heat_events >= 4,
        "overheating should report engine damage ({heat_events})"
    );
    assert!(rig.vehicle.telemetry().engine_heat > 0.4);

    eff = HandlingEffects {
        fuel_leak_rate: 0.5,
        ..HandlingEffects::NEUTRAL
    };
    let mut rig = Rig::new(&c);
    rig.set_effects(&eff);
    run(&mut rig, throttle(1.0), &metric_assists(), 6.0);
    assert_eq!(rig.vehicle.telemetry().fuel, 0.0);
    let v_dry = rig.state().speed_mps;
    run(&mut rig, throttle(1.0), &metric_assists(), 2.0);
    assert!(
        rig.state().speed_mps < v_dry,
        "engine cut once the tank is empty"
    );

    let mut rig = Rig::new(&c);
    rig.set_effects(&HandlingEffects {
        gearbox_health: 0.2,
        ..HandlingEffects::NEUTRAL
    });
    run(&mut rig, throttle(1.0), &metric_assists(), 15.0);
    assert!(rig.state().gear <= 2, "gear {}", rig.state().gear);
}

#[test]
fn ffb_self_centres_in_a_turn() {
    let mut rig = Rig::new(&car("debug_sedan"));
    rig.teleport([0.0; 3], 0.0, 20.0, 0.0);
    run(
        &mut rig,
        VehicleInput {
            steer: -0.15,
            throttle: 0.3,
            ..Default::default()
        },
        &AssistProfile::wheel_default(),
        2.0,
    );
    use wp_contracts::VehicleSim;
    let f = rig.vehicle.ffb();
    // Steering left (negative input): the rack pushes back toward the right (+).
    assert!(f.rack_torque_nm > 0.5, "{f:?}");
    assert!(f.damper > 0.0);
}

#[test]
fn surface_changes_grip() {
    let c = car("debug_sedan");
    let stop = |surface| {
        let mut rig = Rig::new(&c);
        rig.world.surface = surface;
        rig.teleport([0.0; 3], 0.0, 20.0, 0.0);
        run(
            &mut rig,
            VehicleInput {
                brake: 1.0,
                ..Default::default()
            },
            &metric_assists(),
            8.0,
        );
        rig.state().pos[1]
    };
    assert!(
        stop(wp_contracts::SurfaceType::Grass) > 1.6 * stop(wp_contracts::SurfaceType::Asphalt)
    );
}
