//! `LocalConditions`: everything other systems need to know about the weather at one point.
//!
//! This is the payload of the requested `WorldQuery::weather_at(p) -> LocalConditions`
//! (request W-1). It is plain data so the lead can move it into `wp_contracts` unchanged.

use crate::kind::WeatherKind;

/// Visibility in clear air, m (`effects.toml: visibility.reference_sight_m` ships with this
/// value). Used by [`LocalConditions::DRY`]; longer than any police or traffic sight range, so
/// "no weather" and "clear weather" behave the same.
pub const CLEAR_VISIBILITY_M: f32 = 800.0;

/// Weather at one world-space point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalConditions {
    /// Current regime.
    pub kind: WeatherKind,
    /// Regime intensity, 0..1.
    pub intensity: f32,
    /// Rain rate, mm/h.
    pub rain_mm_per_h: f32,
    /// Water film on the road, mm (0 … cell capacity).
    pub water_mm: f32,
    /// Road wetness, 0 (dry) … 1 (fully wet film).
    pub wetness: f32,
    /// Standing water above the film, mm (aquaplaning, spray, puddles).
    pub standing_water_mm: f32,
    /// Air temperature, °C.
    pub air_temp_c: f32,
    /// Road surface temperature, °C (ice below ~0 when wet).
    pub road_temp_c: f32,
    /// Fog extinction coefficient, 1/m.
    pub fog_per_m: f32,
    /// AI sight-distance multiplier from fog and rain, 0..1 (night is not included).
    pub sight_multiplier: f32,
    /// How far anyone can see from this point, m: reference sight (800 m) × `sight_multiplier`.
    /// ~800 clear, ~480 in a 16 mm/h downpour, ~130 in thick fog. Night is not included. Police
    /// LOS caps its ranges with it (see `WeatherSim::visibility_m`).
    pub visibility_m: f32,
    /// Wind velocity in world XY, m/s (direction the air moves toward).
    pub wind_mps: [f32; 2],
}

impl LocalConditions {
    /// Dry, clear, 20 °C, no wind: the conditions every system assumed before weather existed.
    pub const DRY: LocalConditions = LocalConditions {
        kind: WeatherKind::Clear,
        intensity: 0.0,
        rain_mm_per_h: 0.0,
        water_mm: 0.0,
        wetness: 0.0,
        standing_water_mm: 0.0,
        air_temp_c: 20.0,
        road_temp_c: 20.0,
        fog_per_m: 0.0,
        sight_multiplier: 1.0,
        visibility_m: CLEAR_VISIBILITY_M,
        wind_mps: [0.0, 0.0],
    };
}

impl Default for LocalConditions {
    fn default() -> Self {
        Self::DRY
    }
}
