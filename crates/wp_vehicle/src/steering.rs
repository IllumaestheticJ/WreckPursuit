//! Steering: speed-sensitive lock and rate (keyboard/gamepad), countersteer help, Ackermann,
//! toe, damage pull, and FWD torque steer.

use wp_contracts::AssistProfile;

use crate::math::{atan, atan2, lerp, tan};

/// Steering inputs gathered once per tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SteerInput {
    /// Driver steer, -1 (left) ..= 1 (right).
    pub driver: f32,
    /// Forward speed, m/s.
    pub speed: f32,
    /// Body slip angle at the CG, rad (+ = moving toward the left of the heading).
    pub body_slip: f32,
    /// Mean rear slip angle, rad.
    pub rear_slip: f32,
    /// Damage pull, fraction of full lock (+ = right).
    pub pull: f32,
    /// Damage rack responsiveness multiplier.
    pub ratio_scale: f32,
}

/// Fraction of full lock available at `speed` for a given speed sensitivity: full below
/// 30 km/h, tapering linearly to 25% at 150 km/h when `sensitivity = 1`.
pub fn lock_scale(sensitivity: f32, speed: f32) -> f32 {
    let kmh = speed.abs() * 3.6;
    1.0 - sensitivity * 0.75 * ((kmh - 30.0) / 120.0).clamp(0.0, 1.0)
}

/// Advance the centre road-wheel angle (rad, + = left) toward the driver's target.
pub fn update_center_angle(
    current: f32,
    inp: &SteerInput,
    assists: &AssistProfile,
    max_steer: f32,
    rack_speed: f32,
    dt: f32,
) -> f32 {
    let lock = max_steer * inp.ratio_scale * lock_scale(assists.steer_speed_sensitivity, inp.speed);
    let mut target = -(inp.driver.clamp(-1.0, 1.0) + inp.pull) * lock;
    if assists.countersteer_help > 0.0 && inp.speed > 3.0 {
        let h = assists.countersteer_help * ((inp.rear_slip.abs() - 0.06) / 0.1).clamp(0.0, 1.0);
        target = lerp(target, inp.body_slip.clamp(-max_steer, max_steer), h);
    }
    target = target.clamp(-max_steer, max_steer);
    if assists.steer_speed_sensitivity <= 0.0 && assists.countersteer_help <= 0.0 {
        // Wheel profile: 1:1, unfiltered.
        return target;
    }
    let rate =
        rack_speed * inp.ratio_scale * lock_scale(assists.steer_speed_sensitivity, inp.speed);
    current + (target - current).clamp(-rate * dt, rate * dt)
}

/// Per-wheel steer angle with Ackermann blending. `center` + = left; `left` wheel?
/// `wheelbase`, `track` in m.
pub fn ackermann(center: f32, left: bool, ackermann_frac: f32, wheelbase: f32, track: f32) -> f32 {
    if center.abs() < 1e-5 || ackermann_frac <= 0.0 {
        return center;
    }
    let radius = wheelbase / tan(center.abs());
    let inner = (center > 0.0) == left;
    let ideal = if inner {
        atan(wheelbase / (radius - track * 0.5).max(0.1))
    } else {
        atan(wheelbase / (radius + track * 0.5))
    };
    let ideal = ideal * center.signum();
    lerp(center, ideal, ackermann_frac)
}

/// Torque-steer angle offset, rad (+ = left), from left/right driven torque on a steered
/// axle. Includes a 3% geometric imbalance (unequal half-shafts) so FWD cars tug gently
/// under full throttle even with an open diff.
pub fn torque_steer(t_left: f32, t_right: f32) -> f32 {
    let imbalance = (t_left - t_right) + 0.03 * 0.5 * (t_left + t_right);
    -2.0e-5 * imbalance
}

/// Body slip angle from chassis-frame velocity components.
pub fn body_slip(v_forward: f32, v_left: f32) -> f32 {
    if v_forward.abs() < 1.0 {
        0.0
    } else {
        atan2(v_left, v_forward.abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_sensitive_lock() {
        assert_eq!(lock_scale(1.0, 5.0), 1.0);
        assert!((lock_scale(1.0, 150.0 / 3.6) - 0.25).abs() < 1e-4);
        assert_eq!(lock_scale(0.0, 60.0), 1.0);
    }

    #[test]
    fn ackermann_inner_wheel_steers_more() {
        let l = ackermann(0.3, true, 1.0, 2.7, 1.55);
        let r = ackermann(0.3, false, 1.0, 2.7, 1.55);
        assert!(l > 0.3 && r < 0.3, "{l} {r}");
    }

    #[test]
    fn wheel_profile_is_unfiltered() {
        let inp = SteerInput {
            driver: 1.0,
            speed: 30.0,
            body_slip: 0.0,
            rear_slip: 0.0,
            pull: 0.0,
            ratio_scale: 1.0,
        };
        let a = update_center_angle(0.0, &inp, &AssistProfile::wheel_default(), 0.6, 1.0, 0.004);
        assert_eq!(a, -0.6);
        let k = update_center_angle(
            0.0,
            &inp,
            &AssistProfile::keyboard_default(),
            0.6,
            1.0,
            0.004,
        );
        assert!(k > -0.01);
    }
}
