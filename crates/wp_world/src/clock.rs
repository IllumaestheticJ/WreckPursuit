//! Day/night clock and renderer helpers.

use serde::{Deserialize, Serialize};
use wp_contracts::{DayPhase, TimeOfDay};

/// Default clock ratio: 1 real minute = 10 game minutes (a game day lasts 2.4 real hours).
pub const DEFAULT_TIME_RATIO: f32 = 10.0;

/// Nanoseconds in one in-game day.
const DAY_NS: u64 = 86_400 * 1_000_000_000;

/// In-game clock advanced by simulation time only (never wall-clock).
///
/// The clock counts in whole nanoseconds of game time and derives [`WorldClock::time`] from
/// that count each step: adding a tick's few hundredths of a second to an `f32` seconds
/// count near 86 400 rounds the same way every tick, which ran the day up to 6 % fast or slow
/// depending on the hour.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WorldClock {
    /// Current time of day. May be set directly (a time skip); the next step continues from it.
    pub time: TimeOfDay,
    /// In-game seconds per simulated second (default [`DEFAULT_TIME_RATIO`]).
    pub ratio: f32,
    /// Time of day in nanoseconds, the exact count behind `time`.
    #[serde(skip)]
    day_ns: u64,
}

impl Default for WorldClock {
    fn default() -> Self {
        Self::new(TimeOfDay::from_hours(12.0), DEFAULT_TIME_RATIO)
    }
}

/// `t` in whole nanoseconds (millisecond precision: what an `f32` seconds count holds).
fn to_ns(t: TimeOfDay) -> u64 {
    ((t.seconds_since_midnight.max(0.0) * 1000.0).round() as u64 * 1_000_000) % DAY_NS
}

/// The time of day of a nanosecond count (whole milliseconds).
fn from_ns(ns: u64) -> TimeOfDay {
    TimeOfDay {
        seconds_since_midnight: ((ns % DAY_NS) / 1_000_000) as f32 / 1000.0,
    }
}

impl WorldClock {
    /// Clock at `time` running at `ratio`.
    pub fn new(time: TimeOfDay, ratio: f32) -> Self {
        let day_ns = to_ns(time);
        Self {
            time: from_ns(day_ns),
            ratio,
            day_ns,
        }
    }

    /// Advance by `dt` simulated seconds.
    pub fn step(&mut self, dt: f32) {
        if from_ns(self.day_ns) != self.time {
            // `time` was set from outside (or the clock was deserialized): continue from it.
            self.day_ns = to_ns(self.time);
        }
        let add = (dt.max(0.0) * self.ratio.max(0.0) * 1.0e9).round() as u64;
        self.day_ns = (self.day_ns + add % DAY_NS) % DAY_NS;
        self.time = from_ns(self.day_ns);
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

    /// Every hour of the day runs at the same rate (bughunt F5: the f32 seconds count ran
    /// 3 % fast at noon and 6 % slow in the evening at 240 Hz steps).
    #[test]
    fn the_clock_rate_does_not_depend_on_the_hour() {
        let dt = 1.0 / 240.0;
        for start in [0.5, 6.0, 12.0, 18.8, 23.0] {
            let mut c = WorldClock::new(TimeOfDay::from_hours(start), 10.0);
            let t0 = c.time.seconds_since_midnight;
            // Ten real minutes = 6000 game seconds.
            for _ in 0..240 * 600 {
                c.step(dt);
            }
            let advanced = (c.time.seconds_since_midnight - t0).rem_euclid(86_400.0);
            assert!(
                (advanced - 6000.0).abs() < 1.0,
                "from {start} h: {advanced} game s"
            );
        }
    }

    #[test]
    fn a_time_set_from_outside_is_continued() {
        let mut c = WorldClock::default();
        c.step(1.0);
        c.time = TimeOfDay::from_hours(20.0);
        c.step(1.0);
        assert!((c.time.hours() - (20.0 + 10.0 / 3600.0)).abs() < 1e-4);
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
