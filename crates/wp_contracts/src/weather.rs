//! Weather vocabulary (v0.3.0; workstream 14 requests W-1, W-2). `wp_weather` simulates it and
//! answers [`crate::WorldQuery::weather_at`]; worlds without weather return
//! [`LocalConditions::DRY`].

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Weather regime.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
)]
pub enum WeatherKind {
    /// Clear skies.
    #[default]
    Clear,
    /// Cloud cover, no rain.
    Overcast,
    /// Drizzle to steady rain (≈ 0.5–2.5 mm/h).
    LightRain,
    /// Downpour (≈ 4–16 mm/h).
    HeavyRain,
    /// Fog (visibility ≈ 650–130 m).
    Fog,
}

impl WeatherKind {
    /// Every regime, in declaration order.
    pub const ALL: [WeatherKind; 5] = [
        WeatherKind::Clear,
        WeatherKind::Overcast,
        WeatherKind::LightRain,
        WeatherKind::HeavyRain,
        WeatherKind::Fog,
    ];

    /// Stable small integer (hashing, snapshots, the WASM surface).
    pub fn code(self) -> u8 {
        self as u8
    }

    /// Inverse of [`WeatherKind::code`].
    pub fn from_code(code: u8) -> Option<Self> {
        Self::ALL.get(code as usize).copied()
    }

    /// Whether the regime produces rain.
    pub fn is_rain(self) -> bool {
        matches!(self, WeatherKind::LightRain | WeatherKind::HeavyRain)
    }
}

/// Weather at one world-space point (plain data).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LocalConditions {
    /// Current regime.
    pub kind: WeatherKind,
    /// Regime intensity, 0..1.
    pub intensity: f32,
    /// Rain rate, mm/h.
    pub rain_mm_per_h: f32,
    /// Water film on the road, mm.
    pub water_mm: f32,
    /// Road wetness, 0 (dry) … 1 (fully wet film).
    pub wetness: f32,
    /// Standing water above the film, mm (aquaplaning, spray, puddles).
    pub standing_water_mm: f32,
    /// Air temperature, °C.
    pub air_temp_c: f32,
    /// Road surface temperature, °C.
    pub road_temp_c: f32,
    /// Fog extinction coefficient, 1/m.
    pub fog_per_m: f32,
    /// AI sight-distance multiplier from fog and rain, 0..1 (night not included).
    pub sight_multiplier: f32,
    /// Wind velocity in world XY, m/s (direction the air moves toward).
    pub wind_mps: [f32; 2],
}

impl LocalConditions {
    /// Dry, clear, 20 °C, no wind: what every system assumed before weather existed.
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
        wind_mps: [0.0, 0.0],
    };
}

impl Default for LocalConditions {
    fn default() -> Self {
        Self::DRY
    }
}
