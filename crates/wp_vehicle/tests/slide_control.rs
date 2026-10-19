//! Slide-control assists (owner playtest 2026-10-19: "more steering assist / gyro for the
//! higher-power RWD cars"). Scripted power-oversteer exits ([`power_oversteer_exit`]) on the
//! most powerful RWD roster car (rampart_50, 319 kW) and the best power-to-weight one
//! (lucciola_350, 207 W/kg). The scripted driver is naive: it floors the throttle mid-corner,
//! keeps steering into the corner for a reaction time, then lets go of the steering with the
//! throttle still down. It never countersteers by itself.

use wp_contracts::{AssistProfile, LoadedCar};
use wp_vehicle::metrics::{high_speed_catch, power_oversteer_exit, DriverStyle, SlideExit};

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap()
}

/// Keyboard assists with TCS and ESC off: the case the slide help exists for.
fn keyboard_no_tcs() -> AssistProfile {
    AssistProfile {
        tcs: false,
        esc: false,
        ..AssistProfile::keyboard_default()
    }
}

fn exit(id: &str, a: &AssistProfile, style: DriverStyle, lat_g: f32, react: f32) -> SlideExit {
    power_oversteer_exit(&car(id), a, style, 30.0, lat_g, react)
}

#[test]
fn keyboard_power_oversteer_exit_is_recoverable() {
    for a in [AssistProfile::keyboard_default(), keyboard_no_tcs()] {
        for id in ["rampart_50", "lucciola_350"] {
            for react in [0.3, 0.8] {
                let r = exit(id, &a, DriverStyle::Keyboard, 0.7, react);
                assert!(
                    r.recovered && r.max_slip_deg < 90.0,
                    "{id} tcs={} react {react}: {r:?}",
                    a.tcs
                );
                if id == "rampart_50" {
                    assert_eq!(r.gear_at_throttle, 2, "scripted in second gear");
                }
            }
        }
    }
}

#[test]
fn keyboard_drift_can_be_held_then_exited() {
    // 2 s of full throttle while steering into the corner: a real drift (> 10°) that does
    // not spin, then straightens once the steering is released.
    for id in ["rampart_50", "lucciola_350"] {
        let r = exit(id, &keyboard_no_tcs(), DriverStyle::Keyboard, 0.7, 2.0);
        assert!(
            r.max_slip_deg > 10.0 && r.max_slip_deg < 60.0 && r.recovered,
            "{id}: {r:?}"
        );
    }
}

#[test]
fn gamepad_power_slides_never_spin() {
    let gamepad_no_tcs = AssistProfile {
        tcs: false,
        ..AssistProfile::gamepad_default()
    };
    for a in [AssistProfile::gamepad_default(), gamepad_no_tcs] {
        for id in ["rampart_50", "lucciola_350"] {
            for react in [0.3, 0.8] {
                let r = exit(id, &a, DriverStyle::Analog, 0.7, react);
                assert!(r.max_slip_deg < 70.0, "{id} tcs={}: {r:?}", a.tcs);
            }
        }
    }
}

#[test]
fn wheel_profile_is_bit_identical_to_the_plain_model() {
    // Hashes recorded on main (612e036) before the slide assists existed. The wheel preset
    // has countersteer_help 0, so none of the slide code may change a single bit. (With no
    // help the naive driver spins: full sim, no safety net.)
    let cases = [
        ("rampart_50", 0.5, 0.3, 0xa0515ee69de2deca_u64),
        ("rampart_50", 0.7, 0.8, 0xdeddd3e5b1284e37),
        ("lucciola_350", 0.5, 0.3, 0x8cac80168e8bb943),
        ("lucciola_350", 0.7, 0.8, 0x20fdb5196575e4af),
    ];
    for (id, lat_g, react, hash) in cases {
        // The steering-lock data change (playtest item 6b) is not a code change: pin the lock
        // these hashes were recorded with.
        let mut c = car(id);
        let old_lock = if id == "rampart_50" { 0.576 } else { 0.5 };
        for w in c.def.wheels.iter_mut().filter(|w| w.steerable) {
            w.max_steer_rad = old_lock;
        }
        let r = power_oversteer_exit(
            &c,
            &AssistProfile::wheel_default(),
            DriverStyle::Analog,
            30.0,
            lat_g,
            react,
        );
        assert_eq!(r.hash, hash, "{id} {lat_g} {react}: {r:?}");
        assert!(r.max_slip_deg > 90.0, "still spins unassisted");
    }
}

#[test]
fn keyboard_catches_a_slide_above_100_kmh() {
    // A sudden yaw kick at 110 / 130 km/h (a nudge from traffic), the driver countersteers
    // with keys. Main spun 7 of 12 such cases with TCS/ESC off; now all are caught.
    for a in [AssistProfile::keyboard_default(), keyboard_no_tcs()] {
        for id in ["rampart_50", "shiro_turbo", "lucciola_350"] {
            for kmh in [110.0, 130.0] {
                let r = high_speed_catch(&car(id), &a, DriverStyle::Keyboard, kmh / 3.6, 1.2);
                assert!(
                    r.recovered && r.max_slip_deg < 30.0,
                    "{id} tcs={} {kmh} km/h: {r:?}",
                    a.tcs
                );
            }
        }
    }
}
