//! Highway-speed slide catches with keyboard / gamepad steering (owner playtest 2026-10-20:
//! "way too tail happy at speed and incredibly hard to catch a slide without snap
//! oversteering"). See `docs/integration/vehicle.md` → "High-speed catch on the round-12
//! steering".
//!
//! [`catch_snap_back`]: 150 km/h, yaw kick 1.4 rad/s, TCS / ESC off, the countersteer key (or
//! full stick) held from 0.25 s until the body slip is under 3°, then released at once or
//! 0.25 s late. Before the fix the gamepad spun in every case and a late keyboard release
//! snapped 80–180° the other way.

use wp_contracts::{AssistProfile, LoadedCar, TcsLevel};
use wp_vehicle::metrics::catch_snap_back;

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).expect("car loads")
}

fn no_tcs_esc(a: AssistProfile) -> AssistProfile {
    AssistProfile {
        tcs: TcsLevel::Off,
        esc: false,
        ..a
    }
}

#[test]
fn keyboard_and_gamepad_catch_a_150_kmh_slide_without_snap_back() {
    let presets = [
        ("keyboard", no_tcs_esc(AssistProfile::keyboard_default())),
        ("gamepad", no_tcs_esc(AssistProfile::gamepad_default())),
    ];
    // A FWD sport car and the RWD muscle car (the one that spun on a late keyboard release).
    for id in ["kestrel_ts", "rampart_50"] {
        let c = car(id);
        for (name, a) in presets {
            for late in [0.0, 0.25] {
                let r = catch_snap_back(&c, &a, 150.0 / 3.6, 1.4, late);
                let what = format!("{id} {name} late {late} s: {r:?}");
                // Caught: straight again at the end, the swing back no more than 15°, and the
                // slide itself held to a highway-sized one.
                assert!(r.final_deg.abs() < 3.0, "not caught: {what}");
                assert!(r.snap_deg <= 15.0, "snap-back: {what}");
                assert!(r.first_deg < 20.0, "slide: {what}");
            }
        }
    }
}
