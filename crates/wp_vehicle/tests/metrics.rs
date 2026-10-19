//! Plausibility ranges for the debug cars (prompt 02 targets). Ranges, not exact numbers.

use wp_contracts::LoadedCar;
use wp_vehicle::metrics;

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap()
}

#[test]
fn fwd_sedan_metrics() {
    let c = car("debug_sedan");
    let r = metrics::report(&c);
    println!("{r:#?}");
    let t = r.zero_to_100_s.expect("reaches 100 km/h");
    assert!((8.0..=10.0).contains(&t), "0-100 {t}");
    assert!(
        (0.78..=0.92).contains(&r.skidpad_50m.max_lat_g),
        "skidpad {:?}",
        r.skidpad_50m
    );
    assert!(
        r.skidpad_50m.understeer_deg_per_g > 0.0,
        "understeer gradient must be positive"
    );
    assert!(
        (35.0..=48.0).contains(&r.braking_100_0_m),
        "braking {}",
        r.braking_100_0_m
    );
    assert!(
        r.power_on_curvature_ratio_40m < 0.95,
        "FWD should push wide on power"
    );
    assert!(
        r.lift_off_40m.curvature_ratio > 1.03,
        "FWD should tuck in on lift-off {:?}",
        r.lift_off_40m
    );
    assert!(
        r.power_oversteer_throttle_40m.is_none(),
        "FWD has no power oversteer"
    );
    assert!(
        (190.0..=260.0).contains(&r.top_speed_kmh),
        "top speed {}",
        r.top_speed_kmh
    );
    assert!((-5.0..=40.0).contains(&r.step_steer_overshoot_pct));
}

#[test]
fn rwd_coupe_metrics() {
    let c = car("debug_coupe");
    let r = metrics::report(&c);
    println!("{r:#?}");
    let t = r.zero_to_100_s.expect("reaches 100 km/h");
    assert!((5.0..=6.0).contains(&t), "0-100 {t}");
    assert!(
        (0.88..=1.02).contains(&r.skidpad_50m.max_lat_g),
        "skidpad {:?}",
        r.skidpad_50m
    );
    assert!(r.skidpad_50m.understeer_deg_per_g > 0.0);
    assert!(
        (35.0..=48.0).contains(&r.braking_100_0_m),
        "braking {}",
        r.braking_100_0_m
    );
    let onset = r
        .power_oversteer_throttle_40m
        .expect("RWD coupe must power-oversteer");
    assert!(onset < 0.7, "power oversteer onset at {onset}");
    assert!(
        r.power_on_curvature_ratio_40m > 1.0,
        "RWD should rotate on power"
    );
    assert!(
        (250.0..=320.0).contains(&r.top_speed_kmh),
        "top speed {}",
        r.top_speed_kmh
    );
}

#[test]
fn lift_off_oversteer_emerges_near_the_limit() {
    // Lifting at 0.75 g: yaw rate *rises* although the car slows (load moves forward).
    let l = metrics::lift_off_at(&car("debug_sedan"), 40.0, 0.75);
    assert!(l.yaw_rate_delta > 0.0, "{l:?}");
}
