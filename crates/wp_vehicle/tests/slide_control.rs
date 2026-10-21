//! Slide-control assists. Since owner rows 57 / 72 (2026-10-20) the countersteer help is the
//! Wreckfest-style self-aligning steering (0–100 % slider = `countersteer_help`): released
//! keys point the front wheels along the front axle's travel; it never fights an input or a
//! handbrake turn and adds no yaw torque (the earlier slide "gyro" is gone). Scripted
//! power-oversteer exits ([`power_oversteer_exit`]) on the
//! most powerful RWD roster car (rampart_50, 319 kW) and the best power-to-weight one
//! (lucciola_350, 207 W/kg). The scripted driver is naive: it floors the throttle mid-corner,
//! keeps steering into the corner for a reaction time, then lets go of the steering with the
//! throttle still down. It never countersteers by itself.

use wp_contracts::{AssistProfile, LoadedCar};
use wp_vehicle::metrics::{
    catch_snap_back, high_speed_catch, keyboard_corner_wobble, power_oversteer_exit, DriverStyle,
    SlideExit,
};

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

/// Recovered power-oversteer exits over the grid lat 0.5 / 0.7 g × reaction 0.3 / 0.8 / 2 s.
fn recovered(id: &str, a: &AssistProfile, style: DriverStyle) -> usize {
    let mut n = 0;
    for lat_g in [0.5, 0.7] {
        for react in [0.3, 0.8, 2.0] {
            if exit(id, a, style, lat_g, react).recovered {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn release_the_keys_recovery_grows_with_the_slider() {
    // The slider at 0 / 50 / 100 %: keyboard with TCS/ESC off (the slide help's own case) and
    // the gamepad preset. Measured (recovered of 6 per car, rampart / lucciola / hakone /
    // sperber): keyboard 0/1/1/3 → 1/0/4/3 → 6/4/5/6, gamepad 2/2/2/3 → 3/3/4/5 → 6/6/6/6.
    let cars = ["rampart_50", "lucciola_350", "hakone_gt", "sperber_32"];
    let kb = |h: f32| AssistProfile {
        countersteer_help: h,
        ..keyboard_no_tcs()
    };
    let gp = |h: f32| AssistProfile {
        countersteer_help: h,
        ..AssistProfile::gamepad_default()
    };
    for (name, make, style) in [
        (
            "keyboard",
            &kb as &dyn Fn(f32) -> AssistProfile,
            DriverStyle::Keyboard,
        ),
        ("gamepad", &gp, DriverStyle::Analog),
    ] {
        let total =
            |h: f32| -> usize { cars.iter().map(|id| recovered(id, &make(h), style)).sum() };
        let (off, mid, full) = (total(0.0), total(0.5), total(1.0));
        assert!(
            off < mid && mid < full,
            "{name}: {off} / {mid} / {full} of 24"
        );
        assert!(full >= 20, "{name}: 100 % recovers {full} of 24");
        for id in cars {
            assert!(
                recovered(id, &make(1.0), style) >= recovered(id, &make(0.0), style),
                "{name} {id}"
            );
        }
    }
    // 0 % spins like no assist at all: the keyboard car with the slider at 0 loses most
    // exits, as the unassisted wheel preset does.
    let none: usize = cars
        .iter()
        .map(|id| recovered(id, &kb(0.0), DriverStyle::Keyboard))
        .sum();
    assert!(none <= 8, "slider 0: {none} of 24 recovered");
}

#[test]
fn self_aligning_steering_does_not_wobble_a_keyboard_corner() {
    // A tapped key on a 40 m circle at 0.6 g: the yaw-rate spread with the help on is no
    // larger than with it off (measured 0.18–0.25 rad/s on, 0.21–0.25 off), and the car
    // stays on the line.
    for id in [
        "rampart_50",
        "hakone_gt",
        "kestrel_ts",
        "lindwurm_4t",
        "sentinel_cruiser",
    ] {
        let c = car(id);
        let a = |h: f32| AssistProfile {
            countersteer_help: h,
            ..AssistProfile::keyboard_default()
        };
        let (off, _) = keyboard_corner_wobble(&c, &a(0.0), 40.0, 0.6);
        for h in [0.6, 1.0] {
            let (on, err) = keyboard_corner_wobble(&c, &a(h), 40.0, 0.6);
            assert!(on <= off + 0.01, "{id} {h}: {on} vs {off}");
            assert!(err < 1.0, "{id} {h}: {err} m off the line");
        }
    }
}

#[test]
fn wheel_profile_is_bit_identical_to_the_plain_model() {
    // Hashes first recorded on main (612e036) before the slide assists existed; re-recorded
    // [bless] on ws/vehicle (2026-10-19) when the differential model replaced the old LSD
    // (a drivetrain change, not a slide-code change), and again [bless] in round 12b (ride
    // rework: tyre compliance, dampers, car damper data; a suspension change, not a slide-code
    // change). The wheel preset has countersteer_help 0, so none of the slide code may change a
    // single bit. (With no help the naive driver spins: full sim, no safety net.)
    let cases = [
        ("rampart_50", 0.5, 0.3, 0x9f758e911a35d298_u64),
        ("rampart_50", 0.7, 0.8, 0x6090d6f8ac1e1867),
        ("lucciola_350", 0.5, 0.3, 0xb0b0eb7b8b321b3b),
        ("lucciola_350", 0.7, 0.8, 0x6ef3f84e1ecb1aa5),
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
    // with keys. Main spun 7 of 12 such cases with TCS/ESC off; now all are caught (the
    // speed-sensitive lock opens toward the countersteer a slide needs).
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

#[test]
fn keyboard_catch_at_speed_does_not_snap_back() {
    // Owner playtest 2026-10-20: "way too tail happy at speed and incredibly hard to catch a
    // slide without snap oversteering". Yaw kick at 110 / 150 km/h, countersteer key held
    // until nearly straight. Before the fix every case snapped back further than the slide it
    // caught (150 km/h, prompt release: lindwurm 10° → 14°, sedan 12° → 18°); now the swing
    // back is well under the slide, and even a key held 0.25 s too long no longer overshoots.
    // Driver: full key / full stick (see `metrics::catch_snap_back`).
    for id in ["lindwurm_4t", "rampart_50", "debug_sedan"] {
        for kmh in [110.0, 150.0] {
            let c = car(id);
            let prompt = catch_snap_back(&c, &keyboard_no_tcs(), kmh / 3.6, 1.4, 0.0);
            assert!(
                prompt.snap_deg < 0.7 * prompt.first_deg && prompt.final_deg.abs() < 2.0,
                "{id} {kmh} km/h, prompt release: {prompt:?}"
            );
            let late = catch_snap_back(&c, &keyboard_no_tcs(), kmh / 3.6, 1.4, 0.25);
            assert!(
                late.snap_deg <= late.first_deg + 0.5 && late.final_deg.abs() < 2.0,
                "{id} {kmh} km/h, late release: {late:?}"
            );
            // Gamepad, full stick (the help is weaker, so this snapped back hardest: 15° → 22°
            // on the lindwurm at 150 km/h).
            let gamepad = AssistProfile {
                tcs: false,
                ..AssistProfile::gamepad_default()
            };
            let stick = catch_snap_back(&c, &gamepad, kmh / 3.6, 1.4, 0.0);
            assert!(
                stick.snap_deg < 0.7 * stick.first_deg && stick.final_deg.abs() < 2.0,
                "{id} {kmh} km/h, gamepad: {stick:?}"
            );
        }
    }
}
