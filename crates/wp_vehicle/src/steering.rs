//! Steering: speed-sensitive lock and rate (keyboard/gamepad), countersteer help, Ackermann,
//! toe, damage pull, and FWD torque steer.

use wp_contracts::AssistProfile;

use crate::math::{atan, atan2, lerp, tan};
use crate::slide::{catch_amount, highway_amount, slide_amount};

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
    /// Road-wheel angle (rad) past which the front tires make no more cornering force at
    /// this speed (see [`grip_lock`]). Assisted profiles cap their lock here.
    pub grip_lock: f32,
}

/// How far past the front tires' peak slip angle the grip lock reaches, × peak slip angle.
/// 1 = exactly the angle that gives peak cornering force on a neutral car; the lock still
/// opens past it to countersteer a slide.
pub const GRIP_LOCK_MARGIN: f32 = 1.0;

/// Road-wheel angle (rad) that uses all of the front grip at `speed` (m/s): the path angle of
/// the tightest circle the tires can hold (`atan(L·μ·g / v²)`) plus [`GRIP_LOCK_MARGIN`] × the
/// front tires' peak slip angle. More lock than this only pushes the front tires past their
/// peak; at speed a full key or stick then turns into a yaw kick (and, once a slide is caught
/// with the countersteer key still held, a hard turn the other way: the snap-back).
pub fn grip_lock(wheelbase: f32, mu: f32, peak_slip: f32, speed: f32) -> f32 {
    let v2 = (speed * speed).max(1.0);
    atan(wheelbase * mu * wp_contracts::GRAVITY_MPS2 / v2) + GRIP_LOCK_MARGIN * peak_slip
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
/// While sliding, the driver's available lock opens up to this × the rear-axle slip angle.
const SLIDE_LOCK_MARGIN: f32 = 1.3;
/// Rack speed multiplier at full help in a full slide (the rack "catches" quickly).
const SLIDE_RACK_BOOST: f32 = 2.5;

/// At highway speed the driver trim shrinks to this fraction of [`DRIVER_TRIM`]: a full
/// countersteer key then means "catch it" (front wheels at the rear-axle slip), not "add 35%
/// of the lock on top", which over-countersteers and swings the car back hard.
const HIGHWAY_TRIM_SCALE: f32 = 0.3;

/// Advance the centre road-wheel angle (rad, + = left) toward the driver's target.
///
/// With `countersteer_help > 0` and the rear sliding, the target blends from the driver's
/// angle toward the rear-axle slip angle (front wheels pointed where the rear is going) plus
/// a trim from the driver's input, the rack moves faster (and always unwinds fast), and the
/// speed-sensitive lock opens toward the countersteer needed, so a full countersteer key can
/// catch a slide at speed. If the driver steers hard against the countersteer (into the
/// slide) the help fades to 40%, so it does not fight a deliberate flick. At highway speed
/// ([`highway_amount`]) the flick exemption and most of the driver trim fade out: there a
/// key held into the new slide is the countersteer key held too long, not a flick. While the
/// driver countersteers, the help stays on until the rear is back in line. With help 0 this is
/// exactly the plain model.
pub fn update_center_angle(
    current: f32,
    inp: &SteerInput,
    assists: &AssistProfile,
    max_steer: f32,
    rack_speed: f32,
    dt: f32,
) -> f32 {
    let mut lock =
        max_steer * inp.ratio_scale * lock_scale(assists.steer_speed_sensitivity, inp.speed);
    if assists.steer_speed_sensitivity > 0.0 {
        // Assisted lock never reaches far past what the front tires can use.
        lock = lerp(
            lock,
            inp.grip_lock.min(lock),
            assists.steer_speed_sensitivity.min(1.0),
        );
    }
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
        // Countersteering (the driver's input points the same way as the catch): the help stays
        // on until the rear is back in line, so the front wheels follow the rear-axle slip
        // down to straight. Fading it with the slide hands the driver's full countersteer lock
        // back just as the car swings back, which snaps it into a slide the other way.
        let countersteering = catch * driver_target > 0.0;
        let weight = if countersteering {
            slide.max(catch_amount(inp.rear_axle_slip))
        } else {
            slide
        };
        let highway = highway_amount(inp.speed);
        // At highway speed a countersteer input gets the full help whatever the preset (a
        // slammed stick at 150 km/h over-countersteers just like a key).
        let help = if countersteering {
            lerp(help, 1.0, highway)
        } else {
            help
        };
        let h = help * weight * (1.0 - 0.6 * opposing * (1.0 - highway));
        let trim = DRIVER_TRIM * lerp(1.0, HIGHWAY_TRIM_SCALE, highway);
        target = lerp(driver_target, catch + trim * driver_target, h);
        // Unwinding toward centre (letting go after a catch) is always fast with help on, like
        // the self-aligning torque spinning the wheel back through the driver's hands; a slow
        // unwind leaves countersteer on as the car swings back (tank-slapper).
        let unwinding = target.abs() < current.abs() || target * current < 0.0;
        let catch_or_unwind = if unwinding { 1.0 } else { slide };
        rate_boost = 1.0 + SLIDE_RACK_BOOST * help * catch_or_unwind;
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
            grip_lock: 1.0,
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
            grip_lock: 1.0,
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
        // Grip: full right key at 140 km/h gets the speed-reduced lock (≈ 0.17 rad).
        let grip = settle(&fast(0.0), &kb);
        assert!(
            (grip + 0.6 * lock_scale(1.0, 140.0 / 3.6)).abs() < 1e-3,
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
