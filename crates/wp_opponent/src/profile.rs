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
    /// Willingness to attack: following distance, overtake margin, defending, side-by-side,
    /// and how far ahead (and how well) the driver plans a route round traffic, police and
    /// players before slowing for them.
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
        DriverParams {
            usage: Usage {
                lateral: lerp(0.68, 0.93, p.skill),
                braking: lerp(0.68, 0.92, p.skill),
                accel: lerp(0.85, 1.0, p.skill),
            },
            lookahead_per_mps: lerp(0.45, 0.35, p.skill),
            throttle_rate_per_s: lerp(2.0, 8.0, p.skill),
            brake_error_s: lerp(0.12, 0.0, p.consistency) + lerp(0.03, 0.0, p.skill),
            line_noise_m: lerp(0.6, 0.05, p.consistency),
            pressure_sensitivity: lerp(1.5, 0.2, p.consistency),
            headway_s: lerp(0.9, 0.45, p.aggression),
            overtake_margin_m: lerp(1.6, 0.9, p.aggression),
            side_by_side_s: lerp(0.5, 3.0, p.aggression),
            defends: p.aggression >= 0.3,
            overtake_patience_s: lerp(2.5, 0.6, p.aggression),
            plan_ahead_m: lerp(150.0, 400.0, p.aggression),
            plan_horizon_s: lerp(6.0, 15.0, p.aggression),
            plan_step_m: lerp(0.9, 0.25, p.aggression),
            evade_rate_mps: lerp(1.3, 2.6, p.aggression),
            oncoming_buffer_s: lerp(2.5, 1.0, p.aggression),
            civilian_margin_m: lerp(1.8, 1.1, p.risk),
            civilian_headway_s: lerp(1.6, 0.9, p.risk),
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
    /// How far ahead the driver sees and plans a route round traffic, police and players
    /// instead of braking for them, m (aggression). The planner uses all of it.
    pub plan_ahead_m: f32,
    /// Longest prediction horizon, s (aggression): the planner predicts far enough to reach
    /// the edge of sight at its speed (`plan_ahead_m / v`, at least 3 s), up to this.
    pub plan_horizon_s: f32,
    /// Spacing of the lateral positions the planner tries, m (aggression: finer = finds
    /// narrower gaps).
    pub plan_step_m: f32,
    /// Lateral speed of an evasive move, m/s (aggression).
    pub evade_rate_mps: f32,
    /// Extra time an evasive line must stay clear of oncoming cars, s (aggression).
    pub oncoming_buffer_s: f32,
    /// Lateral clearance to civilians, m (risk).
    pub civilian_margin_m: f32,
    /// Time gap to civilians ahead, s (risk).
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
        // Aggression: plans further ahead, further into the future, more finely.
        assert!(hi.plan_ahead_m > lo.plan_ahead_m);
        assert!(hi.plan_horizon_s > lo.plan_horizon_s);
        assert!(hi.plan_step_m < lo.plan_step_m);
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
