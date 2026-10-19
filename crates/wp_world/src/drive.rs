//! Shared AI driving controller: pure-pursuit steering + PID speed control.
//!
//! Built for physical (near-LOD) civilian traffic and shared with every other AI driver
//! (`wp_opponent` racers, police). It turns "where should I be" (a target point ahead) and
//! "how should my speed change" (a commanded acceleration) into a [`VehicleInput`]'s steer,
//! throttle and brake. Higher-level behaviour (paths, speed planning, tactics) lives in the
//! callers; this module only tracks.
//!
//! Defaults ([`DriveGains::TRAFFIC`]) are exactly the gains civilian traffic always used.

use wp_contracts::VehicleState;

use crate::geom::{self, V2, V3};

/// Tuning for [`DriveController`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriveGains {
    /// Lookahead distance at standstill, m.
    pub lookahead_base_m: f32,
    /// Lookahead added per m/s of speed, s.
    pub lookahead_per_mps: f32,
    /// Minimum lookahead, m.
    pub lookahead_min_m: f32,
    /// Maximum lookahead, m.
    pub lookahead_max_m: f32,
    /// Commanded acceleration that alone maps to full pedal (feed-forward), m/s².
    pub accel_full_pedal: f32,
    /// Proportional pedal per m/s of speed error.
    pub kp: f32,
    /// Integral pedal per m of accumulated speed error.
    pub ki: f32,
    /// Integral clamp, m.
    pub integral_limit: f32,
    /// Reference speed is re-synced to the measured speed when they drift further apart, m/s.
    pub resync_mps: f32,
    /// Below this speed the brake is released (no creeping backwards), m/s.
    pub hold_speed_mps: f32,
}

impl DriveGains {
    /// The gains civilian traffic uses (machine D's original controller).
    pub const TRAFFIC: Self = Self {
        lookahead_base_m: 4.0,
        lookahead_per_mps: 0.6,
        lookahead_min_m: 5.0,
        lookahead_max_m: 25.0,
        accel_full_pedal: 6.0,
        kp: 0.5,
        ki: 0.1,
        integral_limit: 5.0,
        resync_mps: 3.0,
        hold_speed_mps: 0.6,
    };
}

impl Default for DriveGains {
    fn default() -> Self {
        Self::TRAFFIC
    }
}

/// Pure-pursuit + PID tracker for one vehicle. Deterministic (IEEE `+ - * /`, polynomial
/// `atan`); call [`DriveController::steer`] and [`DriveController::pedals`] at a fixed rate.
#[derive(Debug, Clone, PartialEq)]
pub struct DriveController {
    /// Gains.
    pub gains: DriveGains,
    /// Wheelbase, m.
    pub wheelbase_m: f32,
    /// Road-wheel angle at full steering input, rad.
    pub max_steer_rad: f32,
    v_ref: f32,
    integ: f32,
}

impl DriveController {
    /// New controller for a car with this wheelbase and full-lock road-wheel angle, starting at
    /// `speed_mps`.
    pub fn new(gains: DriveGains, wheelbase_m: f32, max_steer_rad: f32, speed_mps: f32) -> Self {
        Self {
            gains,
            wheelbase_m,
            max_steer_rad: max_steer_rad.max(0.1),
            v_ref: speed_mps,
            integ: 0.0,
        }
    }

    /// Lookahead distance at speed `v`, m.
    pub fn lookahead(&self, v: f32) -> f32 {
        let g = &self.gains;
        (g.lookahead_base_m + g.lookahead_per_mps * v.abs())
            .clamp(g.lookahead_min_m, g.lookahead_max_m)
    }

    /// Pure-pursuit steering input (-1..1, + = right) toward `target` for a car at `pos`
    /// facing unit `forward` (both horizontal).
    pub fn steer(&self, pos: V2, forward: V2, target: V3) -> f32 {
        let d = geom::sub(geom::xy(target), pos);
        let long = geom::dot(d, forward);
        let latd = geom::dot(d, geom::right_of(forward));
        let l2 = (long * long + latd * latd).max(1.0);
        let curvature = -2.0 * latd / l2;
        let delta = geom::atan(self.wheelbase_m * curvature);
        (-delta / self.max_steer_rad).clamp(-1.0, 1.0)
    }

    /// Throttle and brake (0..1 each) for commanded acceleration `acc` (m/s²) at measured
    /// forward speed `v`, advancing the PID by `dt`. `disabled` = hold the brakes.
    pub fn pedals(&mut self, v: f32, acc: f32, dt: f32, disabled: bool) -> (f32, f32) {
        let g = self.gains;
        self.v_ref = (self.v_ref + acc * dt).max(0.0);
        if (self.v_ref - v).abs() > g.resync_mps {
            self.v_ref = v.max(0.0);
            self.integ = 0.0;
        }
        let err = self.v_ref - v;
        self.integ = (self.integ + err * dt).clamp(-g.integral_limit, g.integral_limit);
        let u = acc / g.accel_full_pedal + g.kp * err + g.ki * self.integ;
        if disabled {
            (0.0, if v > g.hold_speed_mps { 1.0 } else { 0.0 })
        } else if u >= 0.0 {
            (u.min(1.0), 0.0)
        } else if v > g.hold_speed_mps {
            (0.0, (-u).min(1.0))
        } else {
            (0.0, 0.0)
        }
    }

    /// Reset the speed reference (after a teleport or a reverse manoeuvre).
    pub fn resync(&mut self, speed_mps: f32) {
        self.v_ref = speed_mps.max(0.0);
        self.integ = 0.0;
    }
}

/// Horizontal position and unit forward direction of a vehicle state.
pub fn pose_2d(state: &VehicleState) -> (V2, V2) {
    (geom::xy(state.pos), forward_of(state.rot))
}

/// Unit horizontal forward (+Y body axis) of a quaternion `[x, y, z, w]`.
pub fn forward_of(q: [f32; 4]) -> V2 {
    let [x, y, z, w] = q;
    geom::normalize([2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steers_toward_target_side() {
        let c = DriveController::new(DriveGains::TRAFFIC, 2.7, 0.6, 10.0);
        // Target ahead and to the right (+X) → positive (right) steer.
        let s = c.steer([0.0, 0.0], [0.0, 1.0], [3.0, 10.0, 0.0]);
        assert!(s > 0.0, "{s}");
        let s = c.steer([0.0, 0.0], [0.0, 1.0], [-3.0, 10.0, 0.0]);
        assert!(s < 0.0, "{s}");
        assert_eq!(c.steer([0.0, 0.0], [0.0, 1.0], [0.0, 10.0, 0.0]), 0.0);
    }

    #[test]
    fn pedals_follow_acceleration_sign() {
        let mut c = DriveController::new(DriveGains::TRAFFIC, 2.7, 0.6, 10.0);
        let (t, b) = c.pedals(10.0, 3.0, 1.0 / 60.0, false);
        assert!(t > 0.0 && b == 0.0);
        let mut c = DriveController::new(DriveGains::TRAFFIC, 2.7, 0.6, 10.0);
        let (t, b) = c.pedals(10.0, -8.0, 1.0 / 60.0, false);
        assert!(t == 0.0 && b > 0.9);
        let (t, b) = c.pedals(10.0, 0.0, 1.0 / 60.0, true);
        assert_eq!((t, b), (0.0, 1.0));
    }
}
