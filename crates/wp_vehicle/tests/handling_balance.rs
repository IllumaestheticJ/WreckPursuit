//! Handling balance per car class (owner playtest row 28, "all cars understeer too much").
//! Ranges, not exact numbers. Measured with `wp_vehicle::handling` (see
//! `cargo run --release -p wp_vehicle --example handling` for the full table).

use wp_contracts::{AssistProfile, LoadedCar};
use wp_vehicle::handling::{city_corner, presets, ramp_steer};

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap()
}

const V: f32 = 80.0 / 3.6;

fn wheel() -> AssistProfile {
    presets()[2].1
}

/// Chassis understeer gradient (wheel preset, 1:1 steering) at 80 km/h, road-wheel deg/g,
/// must sit in `range` for every car in `ids`.
fn gradient_in(ids: &[&str], range: std::ops::RangeInclusive<f32>) {
    for id in ids {
        let r = ramp_steer(&car(id), &wheel(), V);
        assert!(
            range.contains(&r.understeer_deg_per_g),
            "{id}: {:.2} deg/g not in {range:?} ({r:?})",
            r.understeer_deg_per_g
        );
    }
}

#[test]
fn front_drive_cars_understeer_mildly() {
    // Measured 0.46–0.78 deg/g.
    gradient_in(
        &["vireo_xs", "kestrel_ts", "junkyard_beater", "tansy_13"],
        0.3..=1.1,
    );
}

#[test]
fn rear_drive_cars_are_near_neutral() {
    // Measured 0.45–0.61 deg/g.
    gradient_in(
        &[
            "shiro_turbo",
            "hakone_gt",
            "rampart_50",
            "sperber_32",
            "lucciola_350",
            "sentinel_cruiser",
        ],
        0.2..=0.85,
    );
}

#[test]
fn awd_cars_are_stable_and_close_to_neutral() {
    // Measured 0.32–0.33 deg/g.
    gradient_in(&["lindwurm_4t", "sentinel_wolfhound"], 0.1..=0.6);
}

#[test]
fn heavy_cars_still_push() {
    // Measured 0.49–1.75 deg/g; the half-ton pickup pushes more than any sports car.
    gradient_in(
        &[
            "hauler_1500",
            "ironside_440",
            "drayman_cargo",
            "furrow_24",
            "sentinel_bastion",
        ],
        0.35..=2.2,
    );
    let truck = ramp_steer(&car("hauler_1500"), &wheel(), V).understeer_deg_per_g;
    let sport = ramp_steer(&car("shiro_turbo"), &wheel(), V).understeer_deg_per_g;
    assert!(truck > sport + 0.5, "truck {truck} sport {sport}");
}

#[test]
fn held_key_or_stick_reaches_the_grip_limit() {
    // Before the slip-targeted steering, a held key at 80 km/h made only 72–91% of the car's
    // grip (the fronts were driven ~3× past their peak slip angle). Now ≥ 90% (keyboard) and
    // ≥ 85% (gamepad, which gets a little more lock past the peak). The wheel stays raw.
    let [kb, gp, _] = presets();
    for id in [
        "vireo_xs",
        "kestrel_ts",
        "rampart_50",
        "lucciola_350",
        "lindwurm_4t",
        "hauler_1500",
        "ironside_440",
    ] {
        let c = car(id);
        let limit = ramp_steer(&c, &wheel(), V).max_lat_g;
        let k = ramp_steer(&c, &kb.1, V);
        let g = ramp_steer(&c, &gp.1, V);
        assert!(
            k.full_input_g >= 0.9 * limit,
            "{id} keyboard {:.2} g of {limit:.2}",
            k.full_input_g
        );
        assert!(
            g.full_input_g >= 0.85 * limit,
            "{id} gamepad {:.2} g of {limit:.2}",
            g.full_input_g
        );
        // ESC (keyboard preset) does not add understeer in smooth cornering.
        let w = ramp_steer(&c, &wheel(), V).understeer_deg_per_g;
        assert!(
            (k.understeer_deg_per_g - w).abs() < 0.15,
            "{id} keyboard {:.2} wheel {w:.2} deg/g",
            k.understeer_deg_per_g
        );
    }
}

#[test]
fn held_key_makes_a_city_corner() {
    // 90° left on a 15 m radius at 35 km/h with the key simply held: the car must not run
    // more than 1.5 m wide (all modern cars: 0; the 60s big-block on bias plies: ~1.2 m).
    let kb = presets()[0];
    for id in [
        "kestrel_ts",
        "tansy_13",
        "rampart_50",
        "hauler_1500",
        "ironside_440",
    ] {
        let r = city_corner(&car(id), &kb.1, kb.2);
        assert!(r.held_wide_m < 1.5, "{id}: {r:?}");
    }
}
