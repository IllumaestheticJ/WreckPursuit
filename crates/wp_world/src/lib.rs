//! # wp_world
//!
//! City, road network, signals, civilian traffic and the day/night clock (owned by
//! workstream 05). **Current state: stub** — an infinite flat asphalt plane with no speed
//! limits or signals, plus a working [`WorldClock`].

#![warn(missing_docs)]

use serde::{Deserialize, Serialize};
use wp_contracts::{SignalState, SimEvent, SurfaceType, TimeOfDay, WorldQuery};

/// In-game clock advanced by simulation time only.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WorldClock {
    /// Current time of day.
    pub time: TimeOfDay,
    /// In-game seconds per simulated second (default 10: 1 real minute = 10 game minutes).
    pub ratio: f32,
}

impl Default for WorldClock {
    fn default() -> Self {
        Self {
            time: TimeOfDay::from_hours(12.0),
            ratio: 10.0,
        }
    }
}

impl WorldClock {
    /// Advance by `dt` simulated seconds.
    pub fn step(&mut self, dt: f32) {
        self.time = self.time.advanced(dt * self.ratio);
    }
}

/// The world: road network, traffic and clock.
#[derive(Debug, Clone, Default)]
pub struct World {
    /// Day/night clock.
    pub clock: WorldClock,
}

impl World {
    /// An empty flat world (stub) starting at `time` with clock `ratio`.
    pub fn flat(time: TimeOfDay, ratio: f32) -> Self {
        Self {
            clock: WorldClock { time, ratio },
        }
    }

    /// Advance clock, signals and traffic by `dt` seconds. Pushes events
    /// (e.g. `TrafficViolation`) into `out`.
    pub fn step(&mut self, dt: f32, _out: &mut Vec<SimEvent>) {
        self.clock.step(dt);
    }
}

impl WorldQuery for World {
    fn surface_at(&self, _p: [f32; 3]) -> SurfaceType {
        SurfaceType::Asphalt
    }

    fn speed_limit_at(&self, _p: [f32; 3]) -> Option<f32> {
        None
    }

    fn signal_state_ahead(&self, _p: [f32; 3], _heading: [f32; 3]) -> Option<SignalState> {
        None
    }

    fn time_of_day(&self) -> TimeOfDay {
        self.clock.time
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_advances_with_ratio_and_wraps() {
        let mut w = World::flat(TimeOfDay::from_hours(23.5), 10.0);
        let mut ev = Vec::new();
        // 360 sim seconds * 10 = 1 game hour.
        for _ in 0..360 {
            w.step(1.0, &mut ev);
        }
        assert!((w.time_of_day().hours() - 0.5).abs() < 1e-3);
        assert_eq!(w.surface_at([0.0; 3]), SurfaceType::Asphalt);
    }
}
