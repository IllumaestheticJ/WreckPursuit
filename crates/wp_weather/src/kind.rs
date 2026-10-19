//! Weather regimes and seasons.

use serde::{Deserialize, Serialize};

/// Weather regime. The schedule moves between these; `intensity` (0..1) scales each regime's
/// parameters between the `[at 0, at 1]` pairs in `data/weather/climate.toml`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
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
    /// Every regime, in declaration order (also the order used for Markov rows).
    pub const ALL: [WeatherKind; 5] = [
        WeatherKind::Clear,
        WeatherKind::Overcast,
        WeatherKind::LightRain,
        WeatherKind::HeavyRain,
        WeatherKind::Fog,
    ];

    /// Stable small integer (for hashing, snapshots and the WASM surface).
    pub fn code(self) -> u8 {
        self as u8
    }

    /// Inverse of [`WeatherKind::code`].
    pub fn from_code(code: u8) -> Option<Self> {
        Self::ALL.get(code as usize).copied()
    }

    /// Whether the regime produces rain (it may still be dry at intensity 0, see the tables).
    pub fn is_rain(self) -> bool {
        matches!(self, WeatherKind::LightRain | WeatherKind::HeavyRain)
    }
}

/// Season; selects the Markov rows and the temperature curve.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum Season {
    /// Spring.
    Spring,
    /// Summer.
    Summer,
    /// Autumn (the default: the wettest racing season).
    #[default]
    Autumn,
    /// Winter (freezing nights: black ice on wet roads).
    Winter,
}

impl Season {
    /// Every season.
    pub const ALL: [Season; 4] = [
        Season::Spring,
        Season::Summer,
        Season::Autumn,
        Season::Winter,
    ];
}
