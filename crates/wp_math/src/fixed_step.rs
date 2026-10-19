//! Fixed-timestep accumulator.

/// Converts variable frame durations (supplied by the caller — the sim never reads a clock)
/// into a whole number of fixed ticks plus an interpolation fraction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FixedStep {
    /// Fixed tick length, s.
    pub dt: f32,
    /// Maximum ticks returned by one [`FixedStep::advance`] call (spiral-of-death guard).
    pub max_ticks_per_advance: u32,
    accumulator: f32,
}

impl FixedStep {
    /// New accumulator with tick length `dt` seconds and at most `max_ticks_per_advance` ticks
    /// per call.
    pub fn new(dt: f32, max_ticks_per_advance: u32) -> Self {
        Self {
            dt,
            max_ticks_per_advance,
            accumulator: 0.0,
        }
    }

    /// Add `frame_dt` seconds and return how many fixed ticks to run now. Excess time beyond
    /// `max_ticks_per_advance` ticks is dropped.
    pub fn advance(&mut self, frame_dt: f32) -> u32 {
        self.accumulator += frame_dt.max(0.0);
        let mut ticks = 0;
        while self.accumulator >= self.dt && ticks < self.max_ticks_per_advance {
            self.accumulator -= self.dt;
            ticks += 1;
        }
        if ticks == self.max_ticks_per_advance && self.accumulator >= self.dt {
            self.accumulator = 0.0;
        }
        ticks
    }

    /// Interpolation fraction `0..1` between the last two ticks, for rendering.
    pub fn alpha(&self) -> f32 {
        (self.accumulator / self.dt).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulates_whole_ticks() {
        let mut fs = FixedStep::new(0.25, 100);
        assert_eq!(fs.advance(0.6), 2);
        assert!((fs.alpha() - 0.4).abs() < 1e-5);
        assert_eq!(fs.advance(0.15), 1);
    }

    #[test]
    fn caps_ticks() {
        let mut fs = FixedStep::new(0.1, 3);
        assert_eq!(fs.advance(10.0), 3);
        assert_eq!(fs.alpha(), 0.0);
    }
}
