//! Weather regimes and seasons.

use serde::{Deserialize, Serialize};

/// Weather regime (contract type since v0.3.0, W-1; `ALL`, `code`, `from_code`, `is_rain`).
pub use wp_contracts::WeatherKind;

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
