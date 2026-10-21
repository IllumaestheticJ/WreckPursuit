//! Car stat bars for menus (acceleration, top speed, cornering), measured with the
//! [`crate::metrics`] rigs and mapped onto fixed 0–1 scales.
//!
//! The scales are absolute, not relative to the roster, so adding a car never moves another
//! car's bars. A bar is the measured value placed linearly between the scale's "empty" and
//! "full" ends and clamped to 0–1:
//!
//! | bar | measurement | empty | full |
//! |---|---|---|---|
//! | acceleration | [`metrics::zero_to_100`] (s, lower is better) | 16 s | 3.5 s |
//! | top speed | [`metrics::top_speed_kmh`] | 140 km/h | 310 km/h |
//! | cornering | [`metrics::skidpad`] max lateral g on a 50 m circle | 0.55 g | 1.05 g |
//!
//! Measuring one car runs three rigs (about a minute of simulated driving); callers precompute
//! the table (`simcli car-stats` writes `data/stats/cars.json`) rather than measure per frame.

use wp_contracts::LoadedCar;

use crate::metrics;

/// Skidpad radius for the cornering stat, m (same as `data/cars/VALIDATION.md`).
pub const CORNERING_SKIDPAD_RADIUS_M: f32 = 50.0;

/// 0–100 km/h time that gives an empty acceleration bar, s.
pub const ACCEL_EMPTY_S: f32 = 16.0;
/// 0–100 km/h time that gives a full acceleration bar, s.
pub const ACCEL_FULL_S: f32 = 3.5;
/// Top speed that gives an empty top-speed bar, km/h.
pub const TOP_SPEED_EMPTY_KMH: f32 = 140.0;
/// Top speed that gives a full top-speed bar, km/h.
pub const TOP_SPEED_FULL_KMH: f32 = 310.0;
/// Lateral grip that gives an empty cornering bar, g.
pub const CORNERING_EMPTY_G: f32 = 0.55;
/// Lateral grip that gives a full cornering bar, g.
pub const CORNERING_FULL_G: f32 = 1.05;

/// A car's measured menu stats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarStats {
    /// 0–100 km/h, s (`None` if the car does not reach 100 km/h within 40 s).
    pub zero_to_100_s: Option<f32>,
    /// Top speed, km/h.
    pub top_speed_kmh: f32,
    /// Maximum steady lateral acceleration on a 50 m skidpad, g.
    pub cornering_g: f32,
}

/// Stat bars, each 0 (empty) to 1 (full).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatBars {
    /// Acceleration bar (from 0–100 km/h time).
    pub acceleration: f32,
    /// Top-speed bar.
    pub top_speed: f32,
    /// Cornering bar (from skidpad lateral g).
    pub cornering: f32,
}

impl CarStats {
    /// Measure a car on flat dry asphalt with the metric assists (deterministic).
    pub fn measure(car: &LoadedCar) -> Self {
        Self {
            zero_to_100_s: metrics::zero_to_100(car),
            top_speed_kmh: metrics::top_speed_kmh(car),
            cornering_g: metrics::skidpad(car, CORNERING_SKIDPAD_RADIUS_M).max_lat_g,
        }
    }

    /// The stats placed on the fixed scales (see the module docs).
    pub fn bars(&self) -> StatBars {
        StatBars {
            acceleration: self
                .zero_to_100_s
                .map_or(0.0, |t| scale(t, ACCEL_EMPTY_S, ACCEL_FULL_S)),
            top_speed: scale(self.top_speed_kmh, TOP_SPEED_EMPTY_KMH, TOP_SPEED_FULL_KMH),
            cornering: scale(self.cornering_g, CORNERING_EMPTY_G, CORNERING_FULL_G),
        }
    }
}

/// `v` placed linearly from `empty` (0) to `full` (1), clamped; works for either direction.
fn scale(v: f32, empty: f32, full: f32) -> f32 {
    ((v - empty) / (full - empty)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_follow_the_fixed_scales() {
        let s = CarStats {
            zero_to_100_s: Some(9.75),
            top_speed_kmh: 225.0,
            cornering_g: 0.8,
        };
        let b = s.bars();
        assert!((b.acceleration - 0.5).abs() < 1e-6, "{b:?}");
        assert!((b.top_speed - 0.5).abs() < 1e-6, "{b:?}");
        assert!((b.cornering - 0.5).abs() < 1e-6, "{b:?}");
    }

    #[test]
    fn bars_clamp_and_handle_no_100() {
        let s = CarStats {
            zero_to_100_s: None,
            top_speed_kmh: 400.0,
            cornering_g: 0.2,
        };
        assert_eq!(
            s.bars(),
            StatBars {
                acceleration: 0.0,
                top_speed: 1.0,
                cornering: 0.0
            }
        );
        let quick = CarStats {
            zero_to_100_s: Some(2.0),
            ..s
        };
        assert_eq!(quick.bars().acceleration, 1.0);
    }
}
