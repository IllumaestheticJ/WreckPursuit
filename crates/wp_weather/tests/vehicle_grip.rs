//! End to end: a real car (`wp_vehicle::rig::Rig`, the same model `wp_sim` steps) on a
//! weathered world. Since W-1 `wp_vehicle` reads `WorldQuery::weather_at` per wheel; the
//! interim `WetAsphalt` remap in `surface_at` is still on by default and the vehicle skips its
//! own wet term on `WetAsphalt`, so wet grip is not counted twice either way.

use wp_contracts::{
    load_car, AssistProfile, SurfaceType, TimeOfDay, VehicleInput, WorldQuery, PHYSICS_DT,
};
use wp_vehicle::rig::{FlatWorld, Rig};
use wp_vehicle::VehicleExtras;
use wp_weather::{
    GridSpec, Regime, WeatherConfig, WeatherData, WeatherKind, WeatherSim, Weathered,
};

fn car() -> wp_contracts::LoadedCar {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    load_car(root.join("cars/debug_sedan.json")).unwrap()
}

/// Flat world under a locked weather regime with every cell holding `water_mm`.
fn weathered(kind: WeatherKind, water_mm: f32) -> Weathered<FlatWorld> {
    let cfg = WeatherConfig {
        seed: 1,
        locked: true,
        start: Regime::new(kind, 1.0),
        start_water_mm: water_mm,
        ..WeatherConfig::default()
    };
    let grid = GridSpec::covering([-50.0, -50.0], [50.0, 600.0], 8.0);
    let mut weather = WeatherSim::uniform(WeatherData::embedded(), cfg, grid, SurfaceType::Asphalt);
    weather.update(1.0, TimeOfDay::from_hours(12.0), &[]);
    Weathered::new(FlatWorld::default(), weather)
}

/// Brake from `v0` with ABS (AI assists); returns (stopping distance m, time s).
fn brake_from<W: WorldQuery>(world: W, v0: f32) -> (f32, f32) {
    let car = car();
    let mut rig = Rig::with_world(&car, VehicleExtras::default(), world, [0.0; 3], 0.0);
    // Settle on the springs, then launch at v0 along +Y.
    for _ in 0..240 {
        rig.step(&VehicleInput::default(), &AssistProfile::ai());
    }
    rig.teleport([0.0, 0.0, rig.state().pos[2]], 0.0, v0, 0.0);
    let start = rig.state().pos;
    let brake = VehicleInput {
        brake: 1.0,
        ..VehicleInput::default()
    };
    let mut ticks = 0;
    loop {
        rig.step(&brake, &AssistProfile::ai());
        ticks += 1;
        let v = rig.state().lin_vel;
        if (v[0] * v[0] + v[1] * v[1]).sqrt() < 0.5 || ticks > 240 * 20 {
            break;
        }
    }
    let p = rig.state().pos;
    let d = ((p[0] - start[0]).powi(2) + (p[1] - start[1]).powi(2)).sqrt();
    (d, ticks as f32 * PHYSICS_DT)
}

#[test]
fn soaked_road_lengthens_braking_and_dry_weather_changes_nothing() {
    let v0 = 27.8; // 100 km/h
    let (dry_plain, _) = brake_from(FlatWorld::default(), v0);
    let (dry_weather, _) = brake_from(weathered(WeatherKind::Clear, 0.0), v0);
    let (wet, wet_t) = brake_from(weathered(WeatherKind::HeavyRain, 1.5), v0);
    let (_, dry_t) = brake_from(FlatWorld::default(), v0);
    eprintln!("100-0: dry {dry_plain:.1} m in {dry_t:.2} s, wet {wet:.1} m in {wet_t:.2} s");
    // A dry road under the weather adapter is bit-for-bit the old dry world.
    assert_eq!(dry_plain, dry_weather);
    // debug_sedan's street tire keeps 0.70 of its grip on WetAsphalt. ABS and brake capacity do
    // not scale with grip, so the distance grows less than 1/0.7: measured 42.9 → 54.1 m (1.26×).
    let ratio = wet / dry_plain;
    assert!(
        (1.2..=1.7).contains(&ratio),
        "dry {dry_plain:.1} m, wet {wet:.1} m, ratio {ratio:.2}"
    );
    assert!((30.0..=60.0).contains(&dry_plain), "dry {dry_plain:.1} m");
    assert!(wet_t < 20.0);
}

#[test]
fn damp_road_below_threshold_now_costs_grip() {
    // Before W-1 a light film (wetness < interim_wet_threshold) did not reach the tires. Now
    // `wp_vehicle` reads `weather_at`, so a damp road brakes a little longer than a dry one.
    let d = WeatherData::embedded();
    let damp_mm = d.surfaces.film_mm * (d.surfaces.interim_wet_threshold * 0.5);
    let (dry, _) = brake_from(FlatWorld::default(), 20.0);
    let (damp, _) = brake_from(weathered(WeatherKind::LightRain, damp_mm), 20.0);
    assert!(
        damp > dry && damp < 1.3 * dry,
        "dry {dry:.2} m, damp {damp:.2} m"
    );
}

#[test]
fn interim_remap_off_gives_the_same_wet_stop() {
    // What the lead switches to once weather is wired into `wp_sim`: no remap, the vehicle's
    // own wet model alone. The street tire's wet ratio is 0.70 either way.
    let v0 = 27.8;
    let (remap, _) = brake_from(weathered(WeatherKind::HeavyRain, 1.5), v0);
    let mut w = weathered(WeatherKind::HeavyRain, 1.5);
    w.interim_surface = false;
    let (own, _) = brake_from(w, v0);
    assert!(
        (own / remap - 1.0).abs() < 0.05,
        "remap {remap:.1} m, weather_at only {own:.1} m"
    );
}

/// Speed `after_s` seconds into a full ABS stop from `v0`.
fn speed_after_braking<W: WorldQuery>(world: W, v0: f32, after_s: f32) -> f32 {
    let car = car();
    let mut rig = Rig::with_world(&car, VehicleExtras::default(), world, [0.0; 3], 0.0);
    for _ in 0..240 {
        rig.step(&VehicleInput::default(), &AssistProfile::ai());
    }
    rig.teleport([0.0, 0.0, rig.state().pos[2]], 0.0, v0, 0.0);
    let brake = VehicleInput {
        brake: 1.0,
        ..VehicleInput::default()
    };
    for _ in 0..(after_s / PHYSICS_DT).round() as u32 {
        rig.step(&brake, &AssistProfile::ai());
    }
    let v = rig.state().lin_vel;
    v[1] // facing +Y
}

#[test]
fn pending_wet_scenario_ranges_hold_on_the_real_vehicle() {
    // Mirrors apps/simcli/scenarios/weather/{dry_brake_100_0.ron, wet_brake_100_0.ron.pending}:
    // pedal at 14.0 s, check at 17.2 s (3.2 s later). The scenario starts braking from the
    // cruise controller's ~27.4 m/s.
    let v0 = 27.4;
    let dry = speed_after_braking(FlatWorld::default(), v0, 3.2);
    let wet = speed_after_braking(weathered(WeatherKind::HeavyRain, 1.5), v0, 3.2);
    eprintln!("3.2 s into the stop: dry {dry:.2} m/s, wet {wet:.2} m/s");
    assert!((-0.5..=1.0).contains(&dry), "dry {dry}");
    assert!((3.0..=12.0).contains(&wet), "wet {wet}");
}
