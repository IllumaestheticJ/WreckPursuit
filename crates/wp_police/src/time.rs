//! Integer simulation time. Absolute times are whole microseconds in a `u64` (exact and
//! identical on every target, as the determinism lint requires); durations are `f32` seconds.

use serde::{Deserialize, Serialize};

/// Simulation time since the police system started, in microseconds.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub struct SimTime(pub u64);

impl SimTime {
    /// t = 0.
    pub const ZERO: SimTime = SimTime(0);

    /// `secs` seconds after t = 0 (negative clamps to 0).
    pub fn from_secs(secs: f32) -> Self {
        SimTime((secs.max(0.0) * 1.0e6).round() as u64)
    }

    /// This time plus `secs` seconds (negative durations clamp at t = 0).
    pub fn plus(self, secs: f32) -> Self {
        if secs >= 0.0 {
            SimTime(self.0 + (secs * 1.0e6).round() as u64)
        } else {
            SimTime(self.0.saturating_sub((-secs * 1.0e6).round() as u64))
        }
    }

    /// Seconds from `earlier` to `self` (0 if `earlier` is later).
    pub fn since(self, earlier: SimTime) -> f32 {
        // Split into whole seconds and remainder so long sessions keep sub-ms precision.
        let d = self.0.saturating_sub(earlier.0);
        (d / 1_000_000) as f32 + (d % 1_000_000) as f32 * 1.0e-6
    }

    /// Seconds since t = 0 (for display and statistics).
    pub fn secs(self) -> f32 {
        self.since(SimTime::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic() {
        let t = SimTime::from_secs(1.5);
        assert_eq!(t.plus(0.25).since(t), 0.25);
        assert_eq!(t.since(t.plus(1.0)), 0.0);
        assert_eq!(t.plus(-5.0), SimTime::ZERO);
        // A long session keeps millisecond resolution.
        let late = SimTime::from_secs(0.0).plus(3.0 * 86_400.0);
        assert!((late.plus(0.004).since(late) - 0.004).abs() < 1e-6);
    }
}
