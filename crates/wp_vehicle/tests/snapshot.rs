//! Bit-exact rewind/replay via snapshots (client prediction, NET-N2).

use wp_contracts::{AssistProfile, LoadedCar, VehicleInput, VehicleSim};
use wp_vehicle::rig::Rig;
use wp_vehicle::{SnapshotError, VehicleSnapshot};

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap()
}

/// Busy input script: launch, shift, steer, brake (ABS), handbrake, reverse.
fn input_at(tick: usize) -> VehicleInput {
    let t = tick as f32 / 240.0;
    match t {
        t if t < 3.0 => VehicleInput {
            throttle: 1.0,
            steer: 0.2,
            ..Default::default()
        },
        t if t < 4.0 => VehicleInput {
            throttle: 0.5,
            steer: -0.8,
            ..Default::default()
        },
        t if t < 5.0 => VehicleInput {
            brake: 1.0,
            steer: 0.3,
            ..Default::default()
        },
        t if t < 5.5 => VehicleInput {
            handbrake: 1.0,
            throttle: 0.4,
            ..Default::default()
        },
        _ => VehicleInput {
            brake: 1.0,
            ..Default::default()
        },
    }
}

fn run(rig: &mut Rig, from: usize, to: usize) {
    let a = AssistProfile::keyboard_default();
    for tick in from..to {
        rig.step(&input_at(tick), &a);
    }
}

#[test]
fn rewind_and_replay_is_bit_exact() {
    for id in ["debug_sedan", "debug_coupe"] {
        let c = car(id);
        let mut rig = Rig::new(&c);
        run(&mut rig, 0, 700);
        let snap = rig.snapshot();
        run(&mut rig, 700, 1700);
        let truth = rig.state();
        let truth_hash = rig.state_hash();

        // Rewind the same rig.
        rig.restore(&snap).unwrap();
        rig.set_ticks(700);
        run(&mut rig, 700, 1700);
        assert_eq!(rig.state(), truth, "{id}: rewind diverged");
        assert_eq!(rig.state_hash(), truth_hash);

        // Restore into a fresh rig from the wire bytes (client receiving the server's snapshot).
        let bytes = snap.as_bytes().to_vec();
        let mut fresh = Rig::new(&c);
        fresh.restore(&VehicleSnapshot::from_bytes(bytes)).unwrap();
        fresh.set_ticks(700);
        run(&mut fresh, 700, 1700);
        assert_eq!(fresh.state(), truth, "{id}: fresh restore diverged");
    }
}

#[test]
fn snapshot_guards() {
    let sedan = Rig::new(&car("debug_sedan"));
    let mut coupe = Rig::new(&car("debug_coupe"));
    assert_eq!(
        coupe.restore(&sedan.snapshot()),
        Err(SnapshotError::WrongVehicle)
    );
    let mut bytes = sedan.snapshot().as_bytes().to_vec();
    bytes.pop();
    let mut s2 = Rig::new(&car("debug_sedan"));
    assert_eq!(
        s2.restore(&VehicleSnapshot::from_bytes(bytes)),
        Err(SnapshotError::Malformed)
    );
    assert_eq!(
        s2.restore(&VehicleSnapshot::from_bytes(vec![9])),
        Err(SnapshotError::Format(9))
    );
}

#[test]
fn set_state_is_a_close_approximation() {
    let c = car("debug_coupe");
    let mut rig = Rig::new(&c);
    run(&mut rig, 0, 800);
    let mid = rig.state();
    run(&mut rig, 800, 1040);
    let truth = rig.state();

    let mut other = Rig::new(&c);
    other.vehicle.set_state(&mid);
    let rb_restore = other.snapshot(); // push chassis into the body via restore
    other.restore(&rb_restore).unwrap();
    run(&mut other, 800, 1040);
    let s = other.vehicle.state();
    let d: f32 = (0..3)
        .map(|i| (s.pos[i] - truth.pos[i]).powi(2))
        .sum::<f32>()
        .sqrt();
    // The contract `VehicleState` has no pedal-filter, auto-box timer or ESC-reference state,
    // so the restored car re-ramps its throttle from 0. Since playtest row 28 the scripted
    // keyboard turn at 3.3 s runs at the grip limit (~1 g, instead of washing out at ~0.6 g),
    // where that throttle difference moves the car more: 0.27 m → 0.68 m after 1 s.
    assert!(d < 1.0, "set_state drift after 1 s: {d} m");
}
