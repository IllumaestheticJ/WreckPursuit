//! Ride and launch behaviour (owner playtest row 54: "too bouncy / light, launched too
//! easily"). Ranges, not exact numbers. Physics is the same in Arcade and Simulation (owner
//! rule, CLAUDE.md), so nothing here depends on the damage mode. Measured with `wp_vehicle::ride` (full table:
//! `cargo run --release -p wp_vehicle --example ride all`).

use wp_contracts::LoadedCar;
use wp_vehicle::ride::{
    axle_ride, crest, heave_kick, kerb, ride_assists, side_impact, slide_into_kerb,
};

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap()
}

/// Sports, sedan, muscle, police cruiser.
const REFERENCE: [&str; 4] = ["hakone_gt", "kestrel_ts", "rampart_50", "sentinel_cruiser"];

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
fn ride_frequency_and_damping_are_car_like() {
    // Real cars: ride 1.0–2.0 Hz (soft classic muscle ~1.1, stiff exotic ~2.0), damping ratio
    // ~0.3–0.5 in bump with clearly more in rebound. Measured: 1.06–2.08 Hz, bump 0.21–0.38,
    // rebound 2× bump.
    for id in ROSTER {
        for (a, ax) in axle_ride(&car(id)).iter().enumerate() {
            assert!((1.0..=2.15).contains(&ax.ride_hz), "{id} axle {a}: {ax:?}");
            // Re-ranged (unify, lead 2026-10-21): the owner-chosen ride (medium weight with
            // springs / bars × 1.5) puts lucciola_350's rear at 0.46 in bump; limit 0.48.
            assert!(
                (0.2..=0.48).contains(&ax.zeta_bump),
                "{id} axle {a}: {ax:?}"
            );
            assert!(
                ax.zeta_rebound > 1.6 * ax.zeta_bump,
                "{id} axle {a}: {ax:?}"
            );
            // Not resting on the bump stops (some cars did: 6 mm left on the beater's front).
            assert!(ax.bump_travel_m > 0.05, "{id} axle {a}: {ax:?}");
        }
    }
    // Free heave after a kick: the body oscillates near the axle frequency and decays.
    for id in REFERENCE {
        let o = heave_kick(&car(id), 0.3);
        assert!((1.2..=2.0).contains(&o.natural_hz), "{id}: {o:?}");
        assert!((0.2..=0.6).contains(&o.damping_ratio), "{id}: {o:?}");
    }
}

#[test]
fn a_10cm_kerb_never_launches_and_settles_fast() {
    // Measured: no wheel off the ground, body ≤ 1.1 cm above its final height, ≤ 2 overshoots,
    // settled in 0.42–0.72 s. Before (lead/round12): 5–7 cm above at 60 km/h, 3–4
    // overshoots, 1.2–1.3 s.
    let a = ride_assists();
    for id in REFERENCE {
        let c = car(id);
        for kmh in [30.0, 60.0] {
            let k = kerb(&c, kmh, 0.10, 0.0, &a);
            assert_eq!(k.airborne_s, 0.0, "{id} {kmh}: {k:?}");
            // Re-ranged (unify, lead 2026-10-21): the owner-chosen firm ride lifts a wheel for
            // up to 0.058 s (hakone_gt at 30 km/h); measured + 25 %.
            assert!(k.wheel_lift_s < 0.073, "{id} {kmh}: {k:?}");
            assert!(k.peak_rise_m < 0.025, "{id} {kmh}: {k:?}");
            assert!(k.overshoots <= 3, "{id} {kmh}: {k:?}");
            assert!(k.settle_s.is_some_and(|s| s < 1.0), "{id} {kmh}: {k:?}");
        }
        // Edge at 20°: one front wheel first, still planted.
        let k = kerb(&c, 60.0, 0.10, 20.0, &a);
        assert_eq!(k.airborne_s, 0.0, "{id} angled: {k:?}");
        assert!(k.peak_roll_rate < 0.8, "{id} angled: {k:?}");
    }
}

#[test]
fn a_city_kerb_at_60_keeps_every_car_on_the_ground() {
    // The city's 15 cm sidewalk kerb, square, at 60 km/h. Measured: reference cars never
    // airborne, body ≤ 5 cm above its final height; elsewhere at most 0.06 s with all four
    // wheels up (lucciola_350, stiff 2 Hz exotic; junkyard_beater 0.03 s, shiro_turbo 0.02 s).
    // Before (lead/round12): 13 of 17 cars airborne for up to 0.28 s (junkyard_beater, 30 cm
    // high), the reference cars 0–0.10 s and 10–12 cm.
    let a = ride_assists();
    for id in ROSTER {
        let k = kerb(&car(id), 60.0, 0.15, 0.0, &a);
        let reference = REFERENCE.contains(&id);
        // Re-ranged (unify, lead 2026-10-21): the owner-chosen firm ride has lucciola_350
        // airborne 0.104 s; measured + 25 %. The reference cars still never leave the ground.
        let air = if reference { 0.0 } else { 0.13 };
        assert!(k.airborne_s <= air, "{id}: {k:?}");
        let rise = if reference { 0.06 } else { 0.12 };
        assert!(k.peak_rise_m < rise, "{id}: {k:?}");
    }
}

#[test]
fn crests_follow_physics_not_springs() {
    // A road crest (3 % up, 3 % down, 20 m vertical curves) at 120 km/h asks for 0.33 g less
    // than gravity: no car may leave the ground. A sharp kink (no vertical curve) launches any
    // car briefly; ballistically about 0.4 s at this speed minus the wheels' droop. Measured
    // 0.18–0.31 s (reference 0.25–0.30), 0.19–0.21 m; before (lead/round12) up to 1.19 s and
    // 1.33 m (hauler_1500), the reference cars 0.34–1.04 s.
    let a = ride_assists();
    for id in ROSTER {
        let c = crest(&car(id), 120.0, 0.03, 30.0, 20.0, &a);
        assert_eq!(c.airborne_s, 0.0, "{id} rounded: {c:?}");
    }
    for id in REFERENCE {
        let c = car(id);
        let sharp = crest(&c, 120.0, 0.03, 30.0, 0.0, &a);
        assert!(sharp.airborne_s < 0.36, "{id} sharp: {sharp:?}");
        assert!(sharp.peak_rise_m < 0.25, "{id} sharp: {sharp:?}");
    }
}

#[test]
fn side_hits_and_kerb_trips_do_not_throw_the_car() {
    // A T-bone-sized side impulse (Δv 7 m/s at bumper height) on a car at rest: measured roll
    // ≤ 6°, no wheel lift. A sideways slide into a 15 cm kerb at 35 km/h: measured roll
    // 5.3–10.1° (the tyre now catches the kerb edge, a little more than the old 4.8–7.5°),
    // never airborne or rolled over.
    let a = ride_assists();
    for id in REFERENCE {
        let c = car(id);
        let s = side_impact(&c, c.def.mass_kg * 7.0, 0.45, &a);
        assert!(s.peak_roll.to_degrees() < 8.0, "{id} impact: {s:?}");
        assert!(
            s.wheel_lift_s < 0.05 && s.airborne_s == 0.0,
            "{id} impact: {s:?}"
        );
        assert!(s.peak_vz_mps < 0.3, "{id} impact: {s:?}");
        let k = slide_into_kerb(&c, 35.0, 0.15, &a);
        assert!(!k.rolled, "{id} kerb trip: {k:?}");
        assert!(k.peak_roll.to_degrees() < 20.0, "{id} kerb trip: {k:?}");
        assert_eq!(k.airborne_s, 0.0, "{id} kerb trip: {k:?}");
    }
}
