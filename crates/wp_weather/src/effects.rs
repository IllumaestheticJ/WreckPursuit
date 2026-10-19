//! Weather → visibility, render, traffic, police and economy outputs.
//!
//! `wp_weather` only computes these numbers. The owning crates apply them; the exact call
//! sites are in `docs/integration/weather.md`.

use crate::data::{lerp2, EffectsTable, ModifierRow, VisibilityParams};
use crate::kind::WeatherKind;

/// Multipliers for `wp_police`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PoliceWeatherModifiers {
    /// Multiply patrol speed caps by this (≤ 1 in rain/fog).
    pub patrol_speed_cap_mult: f32,
    /// Multiply pursuit speed caps by this (≤ 1 in rain/fog).
    pub pursuit_speed_cap_mult: f32,
    /// Multiply the crash/incident rate (dispatch call generation) by this (≥ 1 in rain/fog).
    pub crash_incident_rate_mult: f32,
    /// Multiply LOS / roadblock sight distances by this (0..1).
    pub sight_multiplier: f32,
}

/// Multipliers for `wp_world` traffic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrafficWeatherModifiers {
    /// Multiply agents' desired speed by this.
    pub desired_speed_mult: f32,
    /// Multiply IDM time headway by this.
    pub time_headway_mult: f32,
    /// Multiply civilian density targets by this.
    pub density_mult: f32,
    /// Multiply hazard-reaction sight distances by this (0..1).
    pub hazard_sight_multiplier: f32,
    /// Multiply the traffic incident (civilian crash) rate by this.
    pub incident_rate_mult: f32,
}

/// Multipliers for `wp_economy` (table only; economy applies them at payout time).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EconomyWeatherModifiers {
    /// Multiply race purses by this (> 1 in rain: "+X% in the wet").
    pub payout_mult: f32,
    /// Multiply race reputation by this.
    pub rep_mult: f32,
}

/// Everything the renderer needs for sky, fog, rain and wet-road shading (one per frame).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderParams {
    /// Current regime.
    pub kind: WeatherKind,
    /// Regime intensity 0..1.
    pub intensity: f32,
    /// Rain rate for the particle system, mm/h.
    pub rain_mm_per_h: f32,
    /// Exponential fog density (extinction), 1/m. Three.js `FogExp2.density` ≈ this value.
    pub fog_per_m: f32,
    /// Meteorological visibility, m.
    pub visibility_m: f32,
    /// Cloud cover 0..1 (sky, sun intensity).
    pub cloud_cover: f32,
    /// Sun/ambient light multiplier from clouds, 0..1.
    pub sun_dimming: f32,
    /// Wind velocity in world XY, m/s (rain slant, smoke, debris).
    pub wind_mps: [f32; 2],
    /// Mean road wetness 0..1 (global gloss when no per-cell texture is used).
    pub mean_road_wetness: f32,
    /// Air temperature, °C (breath, steam).
    pub air_temp_c: f32,
    /// Road temperature, °C (frost sheen below ~0).
    pub road_temp_c: f32,
}

/// All modifiers blended for the current weather (fields as in [`ModifierRow`], one value each).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeatherModifiers {
    /// See [`PoliceWeatherModifiers::patrol_speed_cap_mult`].
    pub patrol_speed_cap: f32,
    /// See [`PoliceWeatherModifiers::pursuit_speed_cap_mult`].
    pub pursuit_speed_cap: f32,
    /// See [`PoliceWeatherModifiers::crash_incident_rate_mult`].
    pub crash_incident_rate: f32,
    /// See [`EconomyWeatherModifiers::payout_mult`].
    pub payout: f32,
    /// See [`EconomyWeatherModifiers::rep_mult`].
    pub rep: f32,
    /// See [`TrafficWeatherModifiers::desired_speed_mult`].
    pub traffic_speed: f32,
    /// See [`TrafficWeatherModifiers::time_headway_mult`].
    pub traffic_headway: f32,
    /// See [`TrafficWeatherModifiers::density_mult`].
    pub traffic_density: f32,
    /// AI sight multiplier, 0..1.
    pub sight_multiplier: f32,
}

impl WeatherModifiers {
    /// No effect (clear, dry).
    pub const NEUTRAL: WeatherModifiers = WeatherModifiers {
        patrol_speed_cap: 1.0,
        pursuit_speed_cap: 1.0,
        crash_incident_rate: 1.0,
        payout: 1.0,
        rep: 1.0,
        traffic_speed: 1.0,
        traffic_headway: 1.0,
        traffic_density: 1.0,
        sight_multiplier: 1.0,
    };

    /// Police projection.
    pub fn police(&self) -> PoliceWeatherModifiers {
        PoliceWeatherModifiers {
            patrol_speed_cap_mult: self.patrol_speed_cap,
            pursuit_speed_cap_mult: self.pursuit_speed_cap,
            crash_incident_rate_mult: self.crash_incident_rate,
            sight_multiplier: self.sight_multiplier,
        }
    }

    /// Traffic projection.
    pub fn traffic(&self) -> TrafficWeatherModifiers {
        TrafficWeatherModifiers {
            desired_speed_mult: self.traffic_speed,
            time_headway_mult: self.traffic_headway,
            density_mult: self.traffic_density,
            hazard_sight_multiplier: self.sight_multiplier,
            incident_rate_mult: self.crash_incident_rate,
        }
    }

    /// Economy projection.
    pub fn economy(&self) -> EconomyWeatherModifiers {
        EconomyWeatherModifiers {
            payout_mult: self.payout,
            rep_mult: self.rep,
        }
    }
}

/// Whichever of `a`, `b` deviates more from 1.0 (the more severe effect wins).
fn more_extreme(a: f32, b: f32) -> f32 {
    if (b - 1.0).abs() > (a - 1.0).abs() {
        b
    } else {
        a
    }
}

fn eval_row(r: &ModifierRow, t: f32) -> [f32; 8] {
    [
        lerp2(r.patrol_speed_cap, t),
        lerp2(r.pursuit_speed_cap, t),
        lerp2(r.crash_incident_rate, t),
        lerp2(r.payout, t),
        lerp2(r.rep, t),
        lerp2(r.traffic_speed, t),
        lerp2(r.traffic_headway, t),
        lerp2(r.traffic_density, t),
    ]
}

/// Blend the modifier rows: previous regime → current regime by `blend`, then the wet-road row
/// by `mean_wetness` (more severe wins), plus the sight multiplier.
pub fn blend_modifiers(
    t: &EffectsTable,
    prev: (WeatherKind, f32),
    cur: (WeatherKind, f32),
    blend: f32,
    mean_wetness: f32,
    sight_multiplier: f32,
) -> WeatherModifiers {
    let a = eval_row(&t.kinds[&prev.0], prev.1);
    let b = eval_row(&t.kinds[&cur.0], cur.1);
    let w = eval_row(&t.wet_road, mean_wetness);
    let blend = blend.clamp(0.0, 1.0);
    let mut v = [0.0f32; 8];
    for i in 0..8 {
        v[i] = more_extreme(a[i] + (b[i] - a[i]) * blend, w[i]);
    }
    WeatherModifiers {
        patrol_speed_cap: v[0],
        pursuit_speed_cap: v[1],
        crash_incident_rate: v[2],
        payout: v[3],
        rep: v[4],
        traffic_speed: v[5],
        traffic_headway: v[6],
        traffic_density: v[7],
        sight_multiplier,
    }
}

/// Meteorological visibility for a fog extinction coefficient, m (Koschmieder, 2 % contrast).
pub fn visibility_m(v: &VisibilityParams, fog_per_m: f32) -> f32 {
    3.912 / fog_per_m.max(v.clear_air_per_m)
}

/// AI sight-distance multiplier from fog and rain, `min_sight_mult ..= 1`.
pub fn sight_multiplier(v: &VisibilityParams, fog_per_m: f32, rain_mm_per_h: f32) -> f32 {
    let fog = (visibility_m(v, fog_per_m) / v.reference_sight_m).min(1.0);
    let rain =
        1.0 - (v.rain_sight_loss_per_mm_h * rain_mm_per_h.max(0.0)).min(v.max_rain_sight_loss);
    (fog * rain).clamp(v.min_sight_mult, 1.0)
}

/// Spray / wheel-wash intensity for the renderer (and audio), 0..1, from road water and speed.
pub fn spray_intensity(water_mm: f32, speed_mps: f32) -> f32 {
    // A damp film throws a light mist from ~10 m/s; standing water throws sheets.
    let water = (water_mm / 2.0).clamp(0.0, 1.0);
    let speed = ((speed_mps.abs() - 3.0) / 25.0).clamp(0.0, 1.0);
    water * speed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::WeatherData;

    #[test]
    fn rain_slows_police_raises_incidents_and_pays_more() {
        let e = WeatherData::embedded().effects;
        let clear = blend_modifiers(
            &e,
            (WeatherKind::Clear, 1.0),
            (WeatherKind::Clear, 1.0),
            1.0,
            0.0,
            1.0,
        );
        assert_eq!(clear, WeatherModifiers::NEUTRAL);
        let heavy = blend_modifiers(
            &e,
            (WeatherKind::Clear, 1.0),
            (WeatherKind::HeavyRain, 1.0),
            1.0,
            1.0,
            0.6,
        );
        assert!(heavy.patrol_speed_cap < 0.9 && heavy.pursuit_speed_cap < 0.9);
        assert!(heavy.crash_incident_rate > 1.5);
        assert!(heavy.payout > 1.2);
        let light = blend_modifiers(
            &e,
            (WeatherKind::Clear, 1.0),
            (WeatherKind::LightRain, 0.5),
            1.0,
            1.0,
            1.0,
        );
        assert!(light.payout > 1.0 && light.payout < heavy.payout);
        // Half-way through the blend is half-way between.
        let half = blend_modifiers(
            &e,
            (WeatherKind::Clear, 1.0),
            (WeatherKind::HeavyRain, 1.0),
            0.5,
            0.0,
            1.0,
        );
        assert!((half.payout - (1.0 + heavy.payout) / 2.0).abs() < 1e-4);
    }

    #[test]
    fn wet_road_after_rain_still_counts() {
        let e = WeatherData::embedded().effects;
        let m = blend_modifiers(
            &e,
            (WeatherKind::Clear, 1.0),
            (WeatherKind::Clear, 1.0),
            1.0,
            1.0,
            1.0,
        );
        assert!(m.crash_incident_rate > 1.2 && m.payout > 1.0);
    }

    #[test]
    fn fog_and_rain_cut_sight() {
        let v = WeatherData::embedded().effects.visibility;
        assert_eq!(sight_multiplier(&v, 0.0, 0.0), 1.0);
        let thick = sight_multiplier(&v, 0.03, 0.0);
        let thin = sight_multiplier(&v, 0.006, 0.0);
        assert!(thick < thin && thin < 1.0);
        assert!((thick - 3.912 / 0.03 / 800.0).abs() < 1e-3);
        let rain = sight_multiplier(&v, 0.0, 16.0);
        assert!((rain - 0.6).abs() < 1e-4);
        assert!(sight_multiplier(&v, 10.0, 100.0) >= v.min_sight_mult);
    }

    #[test]
    fn spray_needs_water_and_speed() {
        assert_eq!(spray_intensity(0.0, 30.0), 0.0);
        assert_eq!(spray_intensity(3.0, 2.0), 0.0);
        assert!(spray_intensity(3.0, 30.0) > spray_intensity(0.5, 30.0));
    }
}
