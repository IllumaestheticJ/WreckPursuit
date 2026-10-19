//! In-game clock types. The clock is advanced by simulation ticks, never by wall-clock time.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Seconds in one in-game day.
pub const SECONDS_PER_DAY: f32 = 86_400.0;

/// In-game time of day.
#[derive(
    Debug, Clone, Copy, PartialEq, PartialOrd, Default, Serialize, Deserialize, JsonSchema,
)]
pub struct TimeOfDay {
    /// Seconds since midnight, `0.0 ..< 86400.0`.
    pub seconds_since_midnight: f32,
}

/// Coarse phase of the day; police and traffic tables are keyed by this.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum DayPhase {
    /// 05:00–07:00.
    Dawn,
    /// 07:00–18:00.
    Day,
    /// 18:00–20:00.
    Dusk,
    /// 20:00–05:00.
    Night,
}

impl TimeOfDay {
    /// Construct from hours (wrapped into one day), e.g. `TimeOfDay::from_hours(13.5)` = 13:30.
    pub fn from_hours(hours: f32) -> Self {
        Self {
            seconds_since_midnight: (hours * 3600.0).rem_euclid(SECONDS_PER_DAY),
        }
    }

    /// Hours since midnight, `0.0 ..< 24.0`.
    pub fn hours(&self) -> f32 {
        self.seconds_since_midnight / 3600.0
    }

    /// Return this time advanced by `dt_s` in-game seconds, wrapping at midnight.
    pub fn advanced(&self, dt_s: f32) -> Self {
        Self {
            seconds_since_midnight: (self.seconds_since_midnight + dt_s)
                .rem_euclid(SECONDS_PER_DAY),
        }
    }

    /// Coarse day phase.
    pub fn phase(&self) -> DayPhase {
        let h = self.hours();
        if (5.0..7.0).contains(&h) {
            DayPhase::Dawn
        } else if (7.0..18.0).contains(&h) {
            DayPhase::Day
        } else if (18.0..20.0).contains(&h) {
            DayPhase::Dusk
        } else {
            DayPhase::Night
        }
    }

    /// Smooth darkness factor: 0 at noon, 1 at midnight, smoothstep in between.
    /// Polynomial only (no transcendental functions) so it is bit-identical on every target.
    pub fn night_factor(&self) -> f32 {
        let half_day = SECONDS_PER_DAY * 0.5;
        let d = ((self.seconds_since_midnight - half_day).abs() / half_day).clamp(0.0, 1.0);
        d * d * (3.0 - 2.0 * d)
    }
}
