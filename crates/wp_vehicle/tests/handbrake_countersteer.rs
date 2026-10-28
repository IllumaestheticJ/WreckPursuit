//! Handbrake + held key vs the countersteer help (owner, 2026-10-28: "on the lindwurm 4T,
//! hitting the handbrake still forces the car to want to countersteer despite opposing
//! keyboard press"). Same in Arcade and Simulation: the vehicle model never reads the mode.
//!
//! The countersteer help's contract (testbed tooltip, `#cstr`): "Never acts while you steer
//! into the turn or during a handbrake turn; never adds grip or yaw. Keys released = the front
//! wheels follow the car's travel; key against the slide = fast catch."
//!
//! Repro: at 70 km/h, full left + handbrake for 0.6 s, then keep holding left. From the moment
//! the rack reaches the key's side it must stay there (never cross to the countersteer side)
//! while the key is held and the car moves forward, and while the handbrake is pulled it must
//! not be pulled back from where the key put it. Checked for AWD (Lindwurm 4T), RWD
//! (Rampart 50) and FWD (Vireo XS), keyboard and gamepad, the slider at 0 / 50 / 100 %, a
//! straight-line and a flick entry (out of a right-hand turn), on the shipped player steering,
//! on the lab plain-steering sets the testbed can carry over from Drift Lab W (they pulled the
//! wheels from 27° left to 45° right here before the fix), and on the normal steering model
//! (AI, bare rigs; its drift-intent hold came back 0.5 s after the release and countersteered).
//!
//! The plain-steering override is process-wide: every test here takes [`LOCK`].

use std::path::PathBuf;
use std::sync::Mutex;

use wp_contracts::{AssistProfile, LoadedCar, VehicleInput, PHYSICS_DT};
use wp_vehicle::car_lab::CarTune;
use wp_vehicle::drift_lab::slide_state;
use wp_vehicle::drift_lab_w::load_any_car;
use wp_vehicle::plain::{self, PlainSteer};
use wp_vehicle::rig::Rig;

static LOCK: Mutex<()> = Mutex::new(());

const TPS: usize = 240;

fn car(id: &str) -> LoadedCar {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    load_any_car(&data, id).unwrap()
}

/// Steering set under test.
#[derive(Clone, Copy)]
enum Set {
    /// The shipped player steering (plain rack, no override).
    Shipped,
    /// A lab plain-steering override on the player's car.
    Lab(&'static str, fn() -> PlainSteer),
    /// The normal steering model (a vehicle not marked player-driven).
    Normal,
}

impl Set {
    fn name(self) -> &'static str {
        match self {
            Set::Shipped => "shipped",
            Set::Lab(name, _) => name,
            Set::Normal => "normal model",
        }
    }
}

fn assists(dev: &str, help: f32) -> AssistProfile {
    let base = if dev == "gamepad" {
        AssistProfile::gamepad_default()
    } else {
        AssistProfile::keyboard_default()
    };
    AssistProfile {
        countersteer_help: help,
        ..base
    }
}

/// Rolling straight at `kmh` for 1 s (after `pre_right_s` of full right, the flick entry),
/// then full left + handbrake for `hb_s`, then full left held to `end_s`. Calls `each(t, rig,
/// handbrake)` every tick from the left key on (t = 0 at the pull).
#[allow(clippy::too_many_arguments)]
fn run(
    c: &LoadedCar,
    set: Set,
    a: &AssistProfile,
    kmh: f32,
    pre_right_s: f32,
    hb_s: f32,
    end_s: f32,
    mut each: impl FnMut(f32, &Rig, bool),
) {
    match set {
        Set::Lab(_, p) => plain::set_override(Some(p())),
        _ => plain::set_override(None),
    }
    let mut rig = Rig::new(c);
    if !matches!(set, Set::Normal) {
        rig.set_player_input(true);
    }
    rig.teleport([0.0; 3], 0.0, kmh / 3.6, 0.0);
    let pre = ((1.0 + pre_right_s) * TPS as f32) as usize;
    for k in 0..pre {
        let right = k as f32 * PHYSICS_DT >= 1.0;
        rig.step(
            &VehicleInput {
                throttle: 0.3,
                steer: if right { 1.0 } else { 0.0 },
                ..Default::default()
            },
            a,
        );
    }
    for k in 0..(end_s * TPS as f32) as usize {
        let t = k as f32 * PHYSICS_DT;
        let hb = t < hb_s;
        rig.step(
            &VehicleInput {
                steer: -1.0,
                handbrake: if hb { 1.0 } else { 0.0 },
                ..Default::default()
            },
            a,
        );
        each(t, &rig, hb);
    }
    plain::set_override(None);
}

/// Worst case of one run: (lowest rack angle on the key's side once it got there, °; most
/// the rack was pulled back from its peak while the handbrake was pulled, °, straight entry
/// only: in the flick entry the key starts as a countersteer to the old slide, and the lock
/// opened for that shrinks as the slide closes).
fn held_left(c: &LoadedCar, set: Set, a: &AssistProfile, pre_right_s: f32) -> (f32, f32) {
    let mut reached = false;
    let mut min_deg = f32::MAX;
    let mut peak = f32::MIN;
    let mut pulled_back = 0.0f32;
    run(c, set, a, 70.0, pre_right_s, 0.6, 2.5, |_, rig, hb| {
        let s = slide_state(rig);
        // + = left = the key's side.
        let deg = s.steer.to_degrees();
        if deg > 0.5 {
            reached = true;
        }
        if reached && s.v_fwd > 3.0 {
            min_deg = min_deg.min(deg);
            peak = peak.max(deg);
            if hb && pre_right_s == 0.0 {
                pulled_back = pulled_back.max(peak - deg);
            }
        }
    });
    assert!(reached, "the rack never reached the key's side");
    (min_deg, pulled_back)
}

const SETS: [Set; 5] = [
    Set::Shipped,
    Set::Lab("Drift Lab W", PlainSteer::lab_w),
    Set::Lab("spin + VR preset", CarTune::spin_vr_plain),
    Set::Lab("swing-rack preset", CarTune::transition_plain),
    Set::Normal,
];

#[test]
fn a_held_key_keeps_the_wheels_through_a_handbrake_turn() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut fails = Vec::new();
    for id in ["lindwurm_4t", "rampart_50", "vireo_xs"] {
        let c = car(id);
        for set in SETS {
            for dev in ["keyboard", "gamepad"] {
                for help in [0.0, 0.5, 1.0] {
                    for pre_right_s in [0.0, 1.0] {
                        let a = assists(dev, help);
                        let (min_deg, pulled_back) = held_left(&c, set, &a, pre_right_s);
                        if min_deg <= 0.0 || pulled_back > 1.0 {
                            fails.push(format!(
                                "{id} {} {dev} help {help} flick {pre_right_s}: min {min_deg:.1}°, \
                                 pulled back {pulled_back:.1}° under the handbrake",
                                set.name()
                            ));
                        }
                    }
                }
            }
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

/// The help still acts once the key is released or pressed against the slide: after a
/// handbrake entry (left + handbrake 0.3 s, then left held to 0.45 s), released keys bring the
/// wheels toward the car's travel (the countersteer side, right) with the slider on and back
/// toward the centre with it at 0; the counter key (right) gets the wheels there at once.
#[test]
fn released_and_counter_keys_still_get_the_help() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    plain::set_override(None);
    let c = car("lindwurm_4t");
    // Wheel angle (°, + = left) and front-axle travel (°) `after_s` past the key change at 0.45 s.
    let after = |help: f32, key: f32, after_s: f32| -> (f32, f32) {
        let a = assists("keyboard", help);
        let mut rig = Rig::new(&c);
        rig.set_player_input(true);
        rig.teleport([0.0; 3], 0.0, 70.0 / 3.6, 0.0);
        for _ in 0..TPS {
            rig.step(
                &VehicleInput {
                    throttle: 0.3,
                    ..Default::default()
                },
                &a,
            );
        }
        let end = 0.45 + after_s;
        for k in 0..(end * TPS as f32) as usize {
            let t = k as f32 * PHYSICS_DT;
            rig.step(
                &VehicleInput {
                    steer: if t < 0.45 { -1.0 } else { key },
                    handbrake: if t < 0.3 { 1.0 } else { 0.0 },
                    ..Default::default()
                },
                &a,
            );
        }
        let s = slide_state(&rig);
        (s.steer.to_degrees(), s.front_axle_angle.to_degrees())
    };
    // Released keys, past the 0.5 s handbrake grace: the help aligns with the travel (right).
    let (on, travel) = after(1.0, 0.0, 0.55);
    let (off, _) = after(0.0, 0.0, 0.55);
    assert!(
        travel < -5.0,
        "the car should be sliding (travel {travel:.1}°)"
    );
    assert!(
        on < -3.0 && on < off - 3.0,
        "released keys with the help on should follow the travel: {on:.1}° (help 0: {off:.1}°, \
         travel {travel:.1}°)"
    );
    // Counter key (right, against the slide): the wheels are on the right within 0.25 s.
    let (counter, _) = after(0.5, 1.0, 0.25);
    assert!(counter < -3.0, "counter key: wheels at {counter:.1}°");
}
