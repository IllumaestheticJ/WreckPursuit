//! Steering: speed-sensitive lock and rate (keyboard/gamepad), countersteer help, Ackermann,
//! toe, damage pull, and FWD torque steer.

use wp_contracts::AssistProfile;

use crate::math::{atan, atan2, lerp, tan};
use crate::slide::slide_amount;

/// Steering inputs gathered once per tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SteerInput {
    /// Driver steer, -1 (left) ..= 1 (right).
    pub driver: f32,
    /// Forward speed, m/s.
    pub speed: f32,
    /// Body slip angle at the CG, rad (+ = moving toward the left of the heading).
    pub body_slip: f32,
    /// Mean rear tire slip angle, rad (lagged tire value; kept for telemetry/tools).
    pub rear_slip: f32,
    /// Kinematic rear-axle slip angle, rad (+ = the rear axle moves toward the left of the
    /// heading). See [`rear_axle_slip`].
    pub rear_axle_slip: f32,
    /// Car slide class, 0..=1 (see [`crate::slide::slide_class`]).
    pub slide_class: f32,
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

/// While sliding, the driver's own input trims the countersteer target by this fraction of
/// their steer, so throttle + small corrections hold or change the drift angle.
const DRIVER_TRIM: f32 = 0.35;
/// Rack speed multiplier at full help in a full slide (the rack "catches" quickly).
const SLIDE_RACK_BOOST: f32 = 2.5;

/// Advance the centre road-wheel angle (rad, + = left) toward the driver's target.
///
/// With `countersteer_help > 0` and the rear sliding, the target blends from the driver's
/// angle toward the rear-axle slip angle (front wheels pointed where the rear is going) plus
/// a trim from the driver's input, and the rack moves faster. If the driver steers hard
/// against the countersteer (into the slide) the help fades to 40%, so it does not fight a
/// deliberate flick. With help 0 this is exactly the plain model.
pub fn update_center_angle(
    current: f32,
    inp: &SteerInput,
    assists: &AssistProfile,
    max_steer: f32,
    rack_speed: f32,
    dt: f32,
) -> f32 {
    let lock = max_steer * inp.ratio_scale * lock_scale(assists.steer_speed_sensitivity, inp.speed);
    let driver_target = -(inp.driver.clamp(-1.0, 1.0) + inp.pull) * lock;
    let mut target = driver_target;
    let mut rate_boost = 1.0;
    if assists.countersteer_help > 0.0 && inp.speed > 3.0 {
        let slide = slide_amount(inp.rear_axle_slip);
        // Higher-class (powerful RWD) cars get more: help moves toward 1 by `help × class`
        // (keyboard 0.6 → 0.84 on a full-class car, gamepad 0.3 → 0.51).
        let base = assists.countersteer_help.min(1.0);
        let help = base + (1.0 - base) * base * inp.slide_class;
        let catch = inp.rear_axle_slip.clamp(-max_steer, max_steer);
        let opposing = if catch * driver_target < 0.0 {
            ((inp.driver.abs() - 0.5) / 0.5).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let h = help * slide * (1.0 - 0.6 * opposing);
        target = lerp(driver_target, catch + DRIVER_TRIM * driver_target, h);
        rate_boost = 1.0 + SLIDE_RACK_BOOST * help * slide;
    }
    target = target.clamp(-max_steer, max_steer);
    if assists.steer_speed_sensitivity <= 0.0 && assists.countersteer_help <= 0.0 {
        // Wheel profile: 1:1, unfiltered.
        return target;
    }
    let rate = rack_speed
        * inp.ratio_scale
        * lock_scale(assists.steer_speed_sensitivity, inp.speed)
        * rate_boost;
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

/// Kinematic rear-axle slip angle (rad, + = rear axle moving toward the left of the heading)
/// from forward / leftward CG speed, yaw rate (+ = left) and the CG-to-rear-axle distance.
/// 0 below 1 m/s, like [`body_slip`].
pub fn rear_axle_slip(v_forward: f32, v_left: f32, yaw_rate: f32, cg_to_rear: f32) -> f32 {
    if v_forward.abs() < 1.0 {
        0.0
    } else {
        atan2(v_left - yaw_rate * cg_to_rear, v_forward.abs())
    }
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
            rear_axle_slip: 0.0,
            slide_class: 1.0,
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

    fn sliding(driver: f32) -> SteerInput {
        // Left-hand power oversteer: rear axle moving right of the heading.
        SteerInput {
            driver,
            speed: 15.0,
            body_slip: -0.15,
            rear_slip: -0.2,
            rear_axle_slip: -0.3,
            slide_class: 1.0,
            pull: 0.0,
            ratio_scale: 1.0,
        }
    }

    fn settle(inp: &SteerInput, a: &AssistProfile) -> f32 {
        let mut c = 0.0;
        for _ in 0..2000 {
            c = update_center_angle(c, inp, a, 0.6, 1.0, 0.004);
        }
        c
    }

    #[test]
    fn countersteer_targets_rear_axle_and_driver_trims() {
        let kb = AssistProfile::keyboard_default();
        let neutral = settle(&sliding(0.0), &kb);
        assert!(neutral < -0.15, "countersteers right: {neutral}");
        // Steering further right (more countersteer) adds angle; a light input into the
        // slide takes some away.
        assert!(settle(&sliding(0.4), &kb) < neutral);
        assert!(settle(&sliding(-0.4), &kb) > neutral);
        // Full lock into the slide: the help fades, the driver gets most of their angle.
        let flick = settle(&sliding(-1.0), &kb);
        assert!(flick > 0.0, "{flick}");
    }

    #[test]
    fn rack_catches_faster_when_sliding() {
        let kb = AssistProfile::keyboard_default();
        let grip = SteerInput {
            rear_axle_slip: 0.0,
            ..sliding(1.0)
        };
        let a = update_center_angle(0.0, &grip, &kb, 0.6, 1.0, 0.004).abs();
        let b = update_center_angle(0.0, &sliding(0.0), &kb, 0.6, 1.0, 0.004).abs();
        assert!(b > 2.0 * a, "{a} {b}");
    }
}
