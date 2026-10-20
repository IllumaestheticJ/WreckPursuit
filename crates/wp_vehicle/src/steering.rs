//! Steering: slip-targeted lock and rack rate (keyboard/gamepad), countersteer help,
//! Ackermann, toe, damage pull, and FWD torque steer.
//!
//! Keyboard and gamepad (`steer_speed_sensitivity > 0`): full input asks for the road-wheel
//! angle that puts the front tyres just past their peak slip angle with the car cornering at
//! its grip limit ([`slip_target_lock`]), never more than the car's lock. A held key is then
//! "turn as hard as the front tyres can", at any speed. (Before playtest row 28 the lock was a
//! fixed fraction of full lock, 25–100% by speed: at 80 km/h a held key asked for ~3× the slip
//! angle the tyres can use, the fronts washed out and the car made only 75–90% of its grip.)
//! The wheel profile stays 1:1 and unfiltered.

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
    /// Wheelbase, m.
    pub wheelbase: f32,
    /// Front tyres' available friction: lateral peak μ × the front wheels' current grip
    /// multiplier (surface, weather, damage).
    pub front_mu: f32,
    /// Front tyres' peak slip angle, rad.
    pub front_peak_slip: f32,
}

/// Fraction of the target slip angle a keyboard (sensitivity 1) asks for at full input: a
/// little past the tyres' peak, so a held key reaches the limit and a car that pushes still
/// pushes a little.
pub const SLIP_MARGIN: f32 = 1.1;
/// Extra slip margin for lower sensitivities: margin = `SLIP_MARGIN + SLIP_MARGIN_SOFT ×
/// (1 − s) / s` (gamepad 0.7 → 1.27; → full lock as s → 0).
pub const SLIP_MARGIN_SOFT: f32 = 0.4;
/// Time for the rack to travel the full slip-targeted lock at sensitivity 1, s (scaled by the
/// sensitivity, so the gamepad's is 0.7 × this). Never faster than the car's own rack speed.
pub const STEER_RAMP_S: f32 = 0.16;

/// Slip-targeted lock, rad: the steer angle a neutral car needs at its grip limit at `speed`
/// (`wheelbase · μ · g / v²`) plus the front tyres' peak slip angle × the margin for
/// `sensitivity` (> 0). Large at low speed (it is capped by the car's lock), shrinking with
/// speed to about the peak slip angle.
pub fn slip_target_lock(
    sensitivity: f32,
    speed: f32,
    wheelbase: f32,
    front_mu: f32,
    front_peak_slip: f32,
) -> f32 {
    let s = sensitivity.clamp(1e-3, 1.0);
    let v = speed.abs().max(1.0);
    let kinematic = wheelbase * front_mu * wp_contracts::GRAVITY_MPS2 / (v * v);
    let margin = SLIP_MARGIN + SLIP_MARGIN_SOFT * (1.0 - s) / s;
    kinematic + front_peak_slip * margin
}

/// Steering lock available to the driver, rad: the car's lock (× damage rack scale), and with
/// speed sensitivity on no more than [`slip_target_lock`].
pub fn driver_lock(inp: &SteerInput, assists: &AssistProfile, max_steer: f32) -> f32 {
    let full = max_steer * inp.ratio_scale;
    if assists.steer_speed_sensitivity <= 0.0 {
        return full;
    }
    let target = slip_target_lock(
        assists.steer_speed_sensitivity,
        inp.speed,
        inp.wheelbase,
        inp.front_mu,
        inp.front_peak_slip,
    );
    full.min(target * inp.ratio_scale)
}

/// While sliding, the driver's own input trims the countersteer target by this fraction of
/// their steer, so throttle + small corrections hold or change the drift angle.
const DRIVER_TRIM: f32 = 0.35;
/// While sliding, the driver's available lock opens up to this × the rear-axle slip angle.
const SLIDE_LOCK_MARGIN: f32 = 1.3;
/// Rack speed multiplier at full help in a full slide (the rack "catches" quickly).
const SLIDE_RACK_BOOST: f32 = 2.5;

/// Where the steering is heading this tick: the centre road-wheel angle the rack moves toward
/// (rad, + = left) and the rack-speed multiplier (> 1 while the countersteer help catches a
/// slide). See [`update_center_angle`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SteerTarget {
    /// Target centre road-wheel angle, rad (+ = left), within ±`max_steer`.
    pub angle: f32,
    /// Rack-speed multiplier.
    pub rate_boost: f32,
}

/// The steering target for the current state (`current` = centre angle now, rad).
///
/// With `countersteer_help > 0` and the rear sliding, the target blends from the driver's
/// angle toward the rear-axle slip angle (front wheels pointed where the rear is going) plus
/// a trim from the driver's input, the rack moves faster (and always unwinds fast), and the
/// speed-sensitive lock opens toward the countersteer needed, so a full countersteer key can
/// catch a slide at speed. If the driver steers hard against the countersteer (into the
/// slide) the help fades to 40%, so it does not fight a deliberate flick. With help 0 this is
/// exactly the plain model.
pub fn steer_target(
    current: f32,
    inp: &SteerInput,
    assists: &AssistProfile,
    max_steer: f32,
) -> SteerTarget {
    let mut lock = driver_lock(inp, assists, max_steer);
    let assisted = assists.countersteer_help > 0.0 && inp.speed > 3.0;
    let slide = if assisted {
        slide_amount(inp.rear_axle_slip)
    } else {
        0.0
    };
    if slide > 0.0 {
        // Save-the-car lock: the speed-sensitive lock opens toward the countersteer the slide
        // needs (rear-axle slip + margin), scaled by how much the rear slides.
        let needed =
            (inp.rear_axle_slip.abs() * SLIDE_LOCK_MARGIN).min(max_steer * inp.ratio_scale);
        lock += (needed.max(lock) - lock) * slide;
    }
    let driver_target = -(inp.driver.clamp(-1.0, 1.0) + inp.pull) * lock;
    let mut target = driver_target;
    let mut rate_boost = 1.0;
    if assisted {
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
        // Unwinding toward centre (letting go after a catch) is always fast with help on, like
        // the self-aligning torque spinning the wheel back through the driver's hands; a slow
        // unwind leaves countersteer on as the car swings back (tank-slapper).
        let unwinding = target.abs() < current.abs() || target * current < 0.0;
        let catch_or_unwind = if unwinding { 1.0 } else { slide };
        rate_boost = 1.0 + SLIDE_RACK_BOOST * help * catch_or_unwind;
    }
    SteerTarget {
        angle: target.clamp(-max_steer, max_steer),
        rate_boost,
    }
}

/// Advance the centre road-wheel angle (rad, + = left) toward [`steer_target`]. The wheel
/// profile (no speed sensitivity, no countersteer help) is 1:1 and unfiltered. Otherwise the
/// rack covers the driver's lock in [`STEER_RAMP_S`] × sensitivity, but never faster than
/// `rack_speed` (so low-speed steering keeps the car's own rack speed), times the slide
/// boost.
pub fn update_center_angle(
    current: f32,
    inp: &SteerInput,
    assists: &AssistProfile,
    max_steer: f32,
    rack_speed: f32,
    dt: f32,
) -> f32 {
    let SteerTarget {
        angle: target,
        rate_boost,
    } = steer_target(current, inp, assists, max_steer);
    if assists.steer_speed_sensitivity <= 0.0 && assists.countersteer_help <= 0.0 {
        // Wheel profile: 1:1, unfiltered.
        return target;
    }
    let s = assists.steer_speed_sensitivity;
    let base = if s > 0.0 {
        let lock = driver_lock(inp, assists, max_steer);
        (lock / (STEER_RAMP_S * s)).min(rack_speed * inp.ratio_scale)
    } else {
        rack_speed * inp.ratio_scale
    };
    let rate = base * rate_boost;
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

    /// Grip-cornering input for a sport-tyred 2.6 m car.
    fn straight(driver: f32, speed: f32) -> SteerInput {
        SteerInput {
            driver,
            speed,
            body_slip: 0.0,
            rear_slip: 0.0,
            rear_axle_slip: 0.0,
            slide_class: 1.0,
            pull: 0.0,
            ratio_scale: 1.0,
            wheelbase: 2.6,
            front_mu: 1.1,
            front_peak_slip: 0.11,
        }
    }

    #[test]
    fn slip_targeted_lock() {
        let kb = AssistProfile::keyboard_default();
        let gp = AssistProfile::gamepad_default();
        // Parking speed: the car's full lock.
        assert_eq!(driver_lock(&straight(1.0, 5.0), &kb, 0.6), 0.6);
        // 150 km/h: about the peak slip angle (1.1 × 0.11) plus a little kinematic angle.
        let fast = driver_lock(&straight(1.0, 150.0 / 3.6), &kb, 0.6);
        assert!((0.121..0.145).contains(&fast), "{fast}");
        // 80 km/h: the limit angle L·μ·g/v² + 1.1 × peak slip.
        let v = 80.0 / 3.6;
        let want = 2.6 * 1.1 * 9.81 / (v * v) + 1.1 * 0.11;
        assert!((driver_lock(&straight(1.0, v), &kb, 0.6) - want).abs() < 1e-4);
        // The gamepad gets a little more past the peak; the wheel the whole lock.
        assert!(driver_lock(&straight(1.0, v), &gp, 0.6) > want + 0.015);
        let wheel = AssistProfile::wheel_default();
        assert_eq!(driver_lock(&straight(1.0, v), &wheel, 0.6), 0.6);
        // Less grip (wet, gravel) = less lock at speed.
        let wet = SteerInput {
            front_mu: 0.6,
            ..straight(1.0, v)
        };
        assert!(driver_lock(&wet, &kb, 0.6) < want - 0.02);
    }

    #[test]
    fn keyboard_rack_reaches_the_lock_quickly_at_speed() {
        let kb = AssistProfile::keyboard_default();
        let inp = straight(-1.0, 100.0 / 3.6);
        let lock = driver_lock(&inp, &kb, 0.6);
        let mut c = 0.0;
        let mut ticks = 0;
        while c < 0.9 * lock {
            c = update_center_angle(c, &inp, &kb, 0.6, 1.4, 1.0 / 240.0);
            ticks += 1;
        }
        let t = ticks as f32 / 240.0;
        assert!((0.1..=0.2).contains(&t), "{t}");
        // Low speed: the car's own rack speed (1.4 rad/s) is the limit.
        let slow = straight(-1.0, 5.0);
        let c = update_center_angle(0.0, &slow, &kb, 0.6, 1.4, 0.1);
        assert!((c - 0.14).abs() < 1e-5, "{c}");
    }

    #[test]
    fn ackermann_inner_wheel_steers_more() {
        let l = ackermann(0.3, true, 1.0, 2.7, 1.55);
        let r = ackermann(0.3, false, 1.0, 2.7, 1.55);
        assert!(l > 0.3 && r < 0.3, "{l} {r}");
    }

    #[test]
    fn wheel_profile_is_unfiltered() {
        let inp = straight(1.0, 30.0);
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
            body_slip: -0.15,
            rear_slip: -0.2,
            rear_axle_slip: -0.3,
            ..straight(driver, 15.0)
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
    fn lock_opens_while_sliding_at_speed() {
        let kb = AssistProfile::keyboard_default();
        let fast = |rear_axle_slip| SteerInput {
            speed: 140.0 / 3.6,
            rear_axle_slip,
            ..sliding(1.0)
        };
        // Grip: full right key at 140 km/h gets the slip-targeted lock (≈ 0.14 rad).
        let grip = settle(&fast(0.0), &kb);
        assert!(
            (grip + driver_lock(&fast(0.0), &kb, 0.6)).abs() < 1e-3,
            "{grip}"
        );
        // Sliding 0.35 rad: full countersteer key reaches well past it.
        let slide = settle(&fast(-0.35), &kb);
        assert!(slide < -0.35, "{slide}");
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
