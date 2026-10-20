//! Brake dive (owner playtest row 92, v0.12.12: "headlights clip into the ground under
//! braking"). Every roster car, full brake from 100 and 60 km/h on flat asphalt, wheel preset
//! (a step on the pedal, manual gears), keyboard preset and the testbed's owner default
//! (keyboard, manual gears): the nose dives like a road car (≤ ~2.5° peak, asserted ≤ 2.55°;
//! real cars 1–2° at 1 g) and the body ahead of the front axle never reaches the road. Physics is the same in Arcade and Simulation (CLAUDE.md); the
//! two-mode check through `wp_sim` is `crates/wp_sim/tests/brake_dive.rs`.
//!
//! Measured (`cargo run --release -p wp_vehicle --example brake_dive`): peak 1.2–2.52°, front
//! body clearance ≥ 5.4 cm, collider corners ≥ 3.7 cm, bump stops never squashed past the end
//! of the travel. Before (lead/round12 @ 848465d3): peak 1.7–4.1°, front body down to 1.0 cm
//! *into* the road (junkyard_beater), five cars' collider corners on the road.

use wp_contracts::{AssistProfile, LoadedCar};
use wp_vehicle::brake_dive::{brake_dive, clearance_on_front_bump_stops, owner_default_assists};

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap()
}

const ROSTER: [&str; 17] = [
    "vireo_xs",
    "kestrel_ts",
    "shiro_turbo",
    "hakone_gt",
    "rampart_50",
    "ironside_440",
    "sperber_32",
    "lindwurm_4t",
    "lucciola_350",
    "hauler_1500",
    "sentinel_cruiser",
    "sentinel_wolfhound",
    "sentinel_bastion",
    "junkyard_beater",
    "tansy_13",
    "drayman_cargo",
    "furrow_24",
];

#[test]
fn hard_stops_dive_like_a_road_car_and_never_touch_the_road() {
    let presets = [
        ("wheel", AssistProfile::wheel_default()),
        ("keyboard", AssistProfile::keyboard_default()),
        ("owner default (manual gears)", owner_default_assists()),
    ];
    for id in ROSTER {
        let c = car(id);
        for (name, a) in &presets {
            for kmh in [100.0, 60.0] {
                let r = brake_dive(&c, kmh, false, a);
                let what = format!("{id} {name} {kmh} km/h: {r:?}");
                assert!(r.decel_g > 0.6, "brakes work: {what}");
                assert!(r.peak_pitch_deg <= 2.55, "peak pitch: {what}");
                assert!(r.steady_pitch_deg <= 2.4, "steady pitch: {what}");
                assert!(r.peak_pitch_deg > 0.8, "still dives visibly: {what}");
                assert!(r.min_clearance_m > 0.04, "front body clearance: {what}");
                assert!(r.min_collider_m > 0.03, "chassis collider: {what}");
                assert!(
                    r.bump_stop_m <= 0.0,
                    "never past the end of the travel: {what}"
                );
            }
        }
    }
}

#[test]
fn holding_the_brake_into_reverse_does_not_bury_the_nose() {
    // Keyboard drivers hold the brake key through the stop: the automatic box engages
    // reverse and the same key drives backwards, which also loads the front.
    for id in ["kestrel_ts", "junkyard_beater", "sentinel_bastion"] {
        let r = brake_dive(&car(id), 100.0, true, &AssistProfile::keyboard_default());
        assert!(r.peak_pitch_deg <= 2.55, "{id}: {r:?}");
        assert!(r.min_clearance_m > 0.04, "{id}: {r:?}");
    }
}

#[test]
fn the_bump_stops_hold_the_nose_off_the_road() {
    // Worst case geometry: both front struts at the end of their travel (on the progressive
    // bump stops), rear at rest: the body ahead of the front axle is still above the road.
    // Measured 1.2 cm (lucciola_350, low exotic) … 11 cm (hauler_1500); lindwurm_4t was 1.0
    // cm before its front travel was shortened by 1.5 cm.
    for id in ROSTER {
        let c = clearance_on_front_bump_stops(&car(id));
        assert!(c > 0.01, "{id}: {c:.3} m");
    }
}
