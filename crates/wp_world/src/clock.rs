//! Day/night clock and renderer helpers.

use serde::{Deserialize, Serialize};
use wp_contracts::{DayPhase, TimeOfDay};

/// Default clock ratio: 1 real minute = 10 game minutes (a game day lasts 2.4 real hours).
pub const DEFAULT_TIME_RATIO: f32 = 10.0;

/// In-game clock advanced by simulation time only (never wall-clock).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WorldClock {
    /// Current time of day.
    pub time: TimeOfDay,
    /// In-game seconds per simulated second (default [`DEFAULT_TIME_RATIO`]).
    pub ratio: f32,
}

impl Default for WorldClock {
    fn default() -> Self {
        Self {
            time: TimeOfDay::from_hours(12.0),
            ratio: DEFAULT_TIME_RATIO,
        }
    }
}

impl WorldClock {
    /// Clock at `time` running at `ratio`.
    pub fn new(time: TimeOfDay, ratio: f32) -> Self {
        Self { time, ratio }
    }

    /// Advance by `dt` simulated seconds.
    pub fn step(&mut self, dt: f32) {
        self.time = self.time.advanced(dt * self.ratio);
    }

    /// Coarse day phase.
    pub fn phase(&self) -> DayPhase {
        self.time.phase()
    }

    /// Darkness factor 0 (noon) ..= 1 (midnight).
    pub fn night_factor(&self) -> f32 {
        self.time.night_factor()
    }

    /// Unit vector pointing **towards** the sun in world space (+X east, +Y north, +Z up).
    /// See [`sun_direction`].
    pub fn sun_direction(&self) -> [f32; 3] {
        sun_direction(self.time)
    }
}

/// Unit vector towards the sun for a time of day: rises in the east at 06:00, culminates in
/// the south at 12:00 (≈ 60° elevation), sets in the west at 18:00 and is below the horizon at
/// night (negative Z — renderers should switch to moonlight when `z < 0`).
pub fn sun_direction(t: TimeOfDay) -> [f32; 3] {
    let theta = (t.hours() - 6.0) / 12.0 * std::f32::consts::PI;
    let (s, c) = wp_math::sin_cos(theta);
    // East at sunrise, tilted towards the south (-Y) through the day.
    let v = [c, -0.55 * s, 0.85 * s];
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / l, v[1] / l, v[2] / l]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_advances_with_ratio_and_wraps() {
        let mut c = WorldClock::new(TimeOfDay::from_hours(23.5), 10.0);
        for _ in 0..360 {
            c.step(1.0);
        }
        assert!((c.time.hours() - 0.5).abs() < 1e-3);
        assert_eq!(c.phase(), DayPhase::Night);
    }

    #[test]
    fn default_day_length_is_2_4_hours() {
        let mut c = WorldClock::default();
        let start = c.time;
        // 2.4 real hours at 60 Hz steps.
        for _ in 0..(144 * 60) {
            c.step(1.0);
        }
        assert!((c.time.hours() - start.hours()).abs() < 1e-2);
    }

    #[test]
    fn sun_path() {
        let noon = sun_direction(TimeOfDay::from_hours(12.0));
        assert!(noon[2] > 0.8 && noon[1] < 0.0);
        let dawn = sun_direction(TimeOfDay::from_hours(6.0));
        assert!(dawn[0] > 0.99 && dawn[2].abs() < 1e-3);
        let dusk = sun_direction(TimeOfDay::from_hours(18.0));
        assert!(dusk[0] < -0.99);
        assert!(sun_direction(TimeOfDay::from_hours(0.0))[2] < -0.8);
    }
}
