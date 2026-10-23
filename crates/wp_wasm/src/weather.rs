//! Weather for the browser testbed (lead, weather in the game).
//!
//! The weather is part of the scenario (simcli `weather:` field), so a session, its replay header
//! and `simcli replay` all agree: the testbed's weather selector restarts the session with
//! [`with_choice`] applied. Per frame the renderer reads [`info`] (sky, fog, rain, road state);
//! at about 1 Hz it reads the wetness grid ([`grid_meta`], [`grid_bytes`]) for wet-road gloss
//! and puddle sheen; [`spray`] gives per-vehicle spray behind fast cars on wet roads.

use simcli::{Scenario, WeatherSpec};
use wp_contracts::VehicleSim;
use wp_sim::Sim;
use wp_weather::{spray_intensity, Season, WeatherKind};

/// The selector's choices, in menu order: `[id, label]`.
pub const CHOICES: [(&str, &str); 7] = [
    ("none", "Off (dry, no weather)"),
    ("clear", "Clear"),
    ("overcast", "Overcast"),
    ("rain", "Rain"),
    ("heavy_rain", "Heavy rain"),
    ("fog", "Fog"),
    ("dynamic", "Dynamic"),
];

/// The scenario weather for a selector choice (`None` = weather off). Fixed choices hold their
/// regime (`locked`); rain choices start on an already wet road so grip changes at once;
/// `dynamic` starts overcast and lets the seeded schedule roll.
pub fn spec_for(choice: &str) -> Result<Option<WeatherSpec>, String> {
    let spec = |kind, intensity, water_mm, locked| WeatherSpec {
        kind,
        intensity,
        water_mm,
        locked,
        season: Season::Autumn,
        seed: None,
        puddles: Vec::new(),
    };
    Ok(match choice {
        "" | "none" => None,
        "clear" => Some(spec(WeatherKind::Clear, 0.5, 0.0, true)),
        "overcast" => Some(spec(WeatherKind::Overcast, 0.7, 0.0, true)),
        "rain" => Some(spec(WeatherKind::LightRain, 0.8, 0.6, true)),
        "heavy_rain" => Some(spec(WeatherKind::HeavyRain, 0.9, 1.5, true)),
        "fog" => Some(spec(WeatherKind::Fog, 0.9, 0.2, true)),
        "dynamic" => Some(spec(WeatherKind::Overcast, 0.6, 0.0, false)),
        other => return Err(format!("unknown weather {other:?}")),
    })
}

/// `sc` with the weather of a selector choice (replacing any weather it had).
pub fn with_choice(mut sc: Scenario, choice: &str) -> Result<Scenario, String> {
    sc.weather = spec_for(choice)?;
    Ok(sc)
}

/// The selector choice that matches a scenario's weather (`"none"` without weather, `"custom"`
/// for weather the selector does not offer, e.g. a scenario file's).
pub fn choice_of(sc: &Scenario) -> &'static str {
    let Some(w) = &sc.weather else {
        return "none";
    };
    for (id, _) in CHOICES {
        if let Ok(Some(c)) = spec_for(id) {
            if c.kind == w.kind && c.locked == w.locked && w.puddles.is_empty() {
                return id;
            }
        }
    }
    "custom"
}

/// The "rainy night" night run: the chase preset (23:00) in a steady downpour on a soaked
/// road. Same seed as the dry chase, so the two are comparable run for run.
pub fn rainy_night(mut sc: Scenario) -> Scenario {
    sc.name = format!("{}_rainy_night", sc.name);
    sc.weather = Some(WeatherSpec {
        kind: WeatherKind::HeavyRain,
        intensity: 0.7,
        water_mm: 1.2,
        locked: true,
        season: Season::Autumn,
        seed: None,
        puddles: Vec::new(),
    });
    sc
}

/// Floats in [`info`].
pub const INFO_STRIDE: usize = 18;

/// Per-frame weather state, [`INFO_STRIDE`] floats:
/// `[enabled (0/1), kind code (0 clear, 1 overcast, 2 light rain, 3 heavy rain, 4 fog),
/// intensity, rain_mm_per_h, fog_per_m, visibility_m, cloud_cover, sun_dimming, wind_x, wind_y,
/// mean_road_wetness, air_temp_c, road_temp_c, locked (0/1), and at `focus` (the player):
/// water_mm, wetness, standing_water_mm, and the focus car's aquaplaning (max over its wheels,
/// 0 full contact … 1 riding on water)]`. All zeros (and visibility 800) without weather.
pub fn info(sim: &Sim, focus: Option<&wp_sim::SimVehicle>) -> Vec<f32> {
    let Some(w) = sim.weather() else {
        let mut v = vec![0.0; INFO_STRIDE];
        v[5] = wp_weather::CLEAR_VISIBILITY_M;
        v[7] = 1.0;
        return v;
    };
    let r = w.render_params();
    let pos = focus.map(|v| v.vehicle.state().pos);
    let local = pos.map(|p| w.conditions_at(p)).unwrap_or_default();
    let planing = focus
        .map(|v| {
            v.vehicle
                .telemetry()
                .aquaplane
                .into_iter()
                .fold(0.0, f32::max)
        })
        .unwrap_or(0.0);
    vec![
        1.0,
        r.kind.code() as f32,
        r.intensity,
        r.rain_mm_per_h,
        r.fog_per_m,
        w.visibility_m(pos.unwrap_or([0.0; 3])),
        r.cloud_cover,
        r.sun_dimming,
        r.wind_mps[0],
        r.wind_mps[1],
        r.mean_road_wetness,
        r.air_temp_c,
        r.road_temp_c,
        if w.config().locked { 1.0 } else { 0.0 },
        local.water_mm,
        local.wetness,
        local.standing_water_mm,
        planing,
    ]
}

/// Wetness grid placement: `[origin_x, origin_y, cell_m, nx, ny]` (empty without weather).
pub fn grid_meta(sim: &Sim) -> Vec<f32> {
    let Some(w) = sim.weather() else {
        return Vec::new();
    };
    let s = w.state().wetness_by_cell.spec();
    vec![s.origin[0], s.origin[1], s.cell_m, s.nx as f32, s.ny as f32]
}

/// Standing water scale in [`grid_bytes`]: byte = mm × this (255 ≈ 6.4 mm).
pub const STANDING_Q: f32 = 40.0;

/// Two bytes per cell, row-major from the origin (x fastest): wetness × 255 and standing water
/// × [`STANDING_Q`] (clamped). Upload as an `nx × ny` RG8 texture. Empty without weather.
pub fn grid_bytes(sim: &Sim) -> Vec<u8> {
    let Some(w) = sim.weather() else {
        return Vec::new();
    };
    let table = &w.data().surfaces;
    let grid = &w.state().wetness_by_cell;
    let n = grid.spec().len();
    let mut out = Vec::with_capacity(n * 2);
    for water in &grid.water_mm()[..n] {
        let wet = wp_weather::WetnessGrid::wetness_of(*water, table);
        let standing = wp_weather::WetnessGrid::standing_of(*water, table);
        out.push((wet * 255.0).round() as u8);
        out.push((standing * STANDING_Q).round().clamp(0.0, 255.0) as u8);
    }
    out
}

/// Spray per sim vehicle (same order as `vehicle_states()`), 0..1: `spray_intensity` of the
/// water under the car at its speed. All zeros without weather.
pub fn spray(sim: &Sim) -> Vec<f32> {
    let Some(w) = sim.weather() else {
        return vec![0.0; sim.vehicles().len()];
    };
    sim.vehicles()
        .iter()
        .map(|v| {
            let s = v.vehicle.state();
            let speed = (s.lin_vel[0] * s.lin_vel[0] + s.lin_vel[1] * s.lin_vel[1]).sqrt();
            spray_intensity(w.conditions_at(s.pos).water_mm, speed)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_contracts::DamageMode;

    #[test]
    fn every_choice_round_trips_and_validates() {
        for (id, _) in CHOICES {
            let sc = with_choice(
                crate::scenario::free_drive("debug_sedan", DamageMode::Arcade, false),
                id,
            )
            .unwrap();
            sc.validate().unwrap();
            assert_eq!(choice_of(&sc), id);
            assert_eq!(crate::scenario::from_ron(&sc.to_ron()).unwrap(), sc);
        }
        assert!(spec_for("snow").is_err());
    }

    #[test]
    fn a_rainy_session_reports_weather_grid_and_spray() {
        let sc = with_choice(
            crate::scenario::free_drive("debug_sedan", DamageMode::Arcade, false),
            "heavy_rain",
        )
        .unwrap();
        let mut s = crate::session::Session::new(sc, false).unwrap();
        s.run_ticks(240);
        let player = s.vehicle(s.entities()[0]);
        let i = info(s.sim(), player);
        assert_eq!(i.len(), INFO_STRIDE);
        assert_eq!((i[0], i[1]), (1.0, 3.0), "{i:?}");
        assert!(i[3] > 1.0 && i[5] < 800.0 && i[15] > 0.9, "{i:?}");
        let meta = grid_meta(s.sim());
        assert_eq!(grid_bytes(s.sim()).len(), (meta[3] * meta[4]) as usize * 2);
        assert_eq!(spray(s.sim()).len(), s.sim().vehicles().len());
        // Without weather: neutral values, no grid.
        let dry = crate::session::Session::new(
            crate::scenario::free_drive("debug_sedan", DamageMode::Arcade, false),
            false,
        )
        .unwrap();
        assert_eq!(info(dry.sim(), None)[0], 0.0);
        assert!(grid_bytes(dry.sim()).is_empty());
    }

    #[test]
    fn rainy_night_is_a_soaked_night_run() {
        let sc = rainy_night(crate::scenario::night_run(
            "debug_coupe",
            DamageMode::Arcade,
        ));
        sc.validate().unwrap();
        let w = sc.weather.as_ref().unwrap();
        assert_eq!(w.kind, WeatherKind::HeavyRain);
        assert!(w.water_mm >= 0.5 && sc.time_of_day_h >= 21.0);
    }
}
