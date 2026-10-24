//! Driver personality ([`DriverProfile`], five 0..1 knobs) and the concrete numbers derived
//! from it ([`DriverParams`]). The mapping is linear and documented field by field so designers
//! can predict what a knob does; the README has the same table.
//!
//! There is **no rubber-banding**: nothing here depends on race position or the gap to the
//! player. A driver is fast because of `skill` (how much of the car's measured grip and
//! braking they use, how precise they are) and the car itself.

use serde::{Deserialize, Serialize};

use crate::line::Usage;

/// Driver personality. Every field is 0..=1.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DriverProfile {
    /// Pace and precision: share of the car's grip/braking used, lookahead quality.
    pub skill: f32,
    /// Willingness to attack: following distance, overtake margin, defending, side-by-side.
    pub aggression: f32,
    /// Repeatability: braking-point and line noise, mistakes under pressure.
    pub consistency: f32,
    /// Appetite for risk: traffic gaps accepted, staying in a race with police around.
    pub risk: f32,
    /// Situational awareness: how far ahead/behind other cars and civilians are noticed.
    pub awareness: f32,
}

impl Default for DriverProfile {
    /// A solid mid-pack driver.
    fn default() -> Self {
        Self {
            skill: 0.6,
            aggression: 0.5,
            consistency: 0.7,
            risk: 0.4,
            awareness: 0.7,
        }
    }
}

impl DriverProfile {
    /// Profile with every knob clamped to 0..=1.
    pub fn clamped(self) -> Self {
        let c = |x: f32| x.clamp(0.0, 1.0);
        Self {
            skill: c(self.skill),
            aggression: c(self.aggression),
            consistency: c(self.consistency),
            risk: c(self.risk),
            awareness: c(self.awareness),
        }
    }

    /// Derived driving parameters.
    pub fn params(&self) -> DriverParams {
        let p = self.clamped();
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let traffic_risk = p.risk.max(0.7 * (1.0 - p.skill));
        DriverParams {
            usage: Usage {
                lateral: lerp(0.60, 0.95, p.skill),
                braking: lerp(0.55, 0.95, p.skill),
                accel: lerp(0.80, 1.0, p.skill),
            },
            lookahead_per_mps: lerp(0.45, 0.35, p.skill),
            throttle_rate_per_s: lerp(2.0, 8.0, p.skill),
            early_brake_s: lerp(0.35, 0.0, p.skill),
            brake_error_s: lerp(0.12, 0.0, p.consistency) + lerp(0.03, 0.0, p.skill),
            line_noise_m: lerp(0.6, 0.05, p.consistency),
            pressure_sensitivity: lerp(1.5, 0.2, p.consistency),
            headway_s: lerp(0.9, 0.45, p.aggression),
            overtake_margin_m: lerp(1.6, 0.9, p.aggression),
            side_by_side_s: lerp(0.5, 3.0, p.aggression),
            defends: p.aggression >= 0.3,
            overtake_patience_s: lerp(2.5, 0.6, p.aggression),
            traffic_risk,
            civilian_margin_m: lerp(0.6, 0.35, traffic_risk),
            civilian_headway_s: lerp(1.0, 0.5, traffic_risk),
            flee_wanted_level: 1 + (3.0 * p.risk + 0.5) as u8,
            perception_ahead_m: lerp(60.0, 130.0, p.awareness),
            mirror_range_m: lerp(12.0, 45.0, p.awareness),
        }
    }
}

/// Concrete numbers a driver works with (see the README table).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DriverParams {
    /// Fractions of the car's measured grip, braking and acceleration used (skill).
    pub usage: Usage,
    /// Pure-pursuit lookahead per m/s (skill: better drivers look at a nearer, sharper point).
    pub lookahead_per_mps: f32,
    /// Max throttle increase per second (skill: smoother, faster pedal work).
    pub throttle_rate_per_s: f32,
    /// How much earlier than the profile's braking point the driver brakes, s of travel
    /// (skill: a beginner lifts early, an expert brakes at the point).
    pub early_brake_s: f32,
    /// Std-dev of the braking point, expressed in seconds of travel (consistency, skill).
    pub brake_error_s: f32,
    /// Std-dev of the lateral line error, m (consistency).
    pub line_noise_m: f32,
    /// How much pressure (a car close behind / alongside) multiplies the errors (consistency).
    pub pressure_sensitivity: f32,
    /// Time gap kept to the car ahead when not passing, s (aggression).
    pub headway_s: f32,
    /// Lateral clearance required beside a car being passed, m (aggression).
    pub overtake_margin_m: f32,
    /// How long the driver stays side by side into a braking zone before backing out, s
    /// (aggression).
    pub side_by_side_s: f32,
    /// Makes a single defensive move when attacked (aggression ≥ 0.3).
    pub defends: bool,
    /// Time stuck behind a slower car before an overtake is considered, s (aggression).
    pub overtake_patience_s: f32,
    /// Risk taken in traffic: the opposing lanes to pass, the gaps accepted beside civilians
    /// (`risk`, raised for weak drivers: `max(risk, 0.7 · (1 − skill))` — a beginner drives
    /// wild, a skilled driver as carefully as the personality says). Police-related choices
    /// (running, red lights) keep the personality's own `risk`.
    pub traffic_risk: f32,
    /// Lateral clearance to civilians, m (traffic risk).
    pub civilian_margin_m: f32,
    /// Time gap to civilians ahead, s (traffic risk).
    pub civilian_headway_s: f32,
    /// Wanted level at which the driver abandons the race and runs (risk).
    pub flee_wanted_level: u8,
    /// Range ahead in which cars and civilians are noticed, m (awareness).
    pub perception_ahead_m: f32,
    /// Range behind in which cars are noticed (mirrors), m (awareness).
    pub mirror_range_m: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_raises_usage_and_aggression_shortens_headway() {
        let lo = DriverProfile {
            skill: 0.1,
            aggression: 0.1,
            ..Default::default()
        }
        .params();
        let hi = DriverProfile {
            skill: 0.9,
            aggression: 0.9,
            ..Default::default()
        }
        .params();
        assert!(hi.usage.lateral > lo.usage.lateral);
        assert!(hi.usage.braking > lo.usage.braking);
        assert!(hi.headway_s < lo.headway_s);
        assert!(hi.overtake_margin_m < lo.overtake_margin_m);
        assert!(!lo.defends && hi.defends);
    }

    #[test]
    fn risk_sets_flee_threshold() {
        let careful = DriverProfile {
            risk: 0.0,
            ..Default::default()
        }
        .params();
        let reckless = DriverProfile {
            risk: 1.0,
            ..Default::default()
        }
        .params();
        assert_eq!(careful.flee_wanted_level, 1);
        assert_eq!(reckless.flee_wanted_level, 4);
    }
}
