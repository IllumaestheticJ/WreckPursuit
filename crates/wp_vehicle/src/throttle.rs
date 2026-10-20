//! Keyboard throttle smoothing with TCS off (owner row 98): "driving without TCS on keyboard
//! is incredibly difficult". Two light, bounded parts, both only with TCS off (with TCS on the
//! TCS does this job) and never on the wheel preset:
//!
//! - **Rise ramp:** the pedal rises from 0 to 1 no faster than [`ramp_time_s`] — slower in
//!   1st / 2nd at low speed in a powerful RWD car (up to ~0.9 s at 100 %), quick in high gears.
//!   Lifting is never slowed.
//! - **Traction trim:** while the driven wheels' slip ratio runs past the tyre's peak, the
//!   pedal target is eased down by up to [`TRIM_MAX`] × smoothing. Not TCS: no brakes, no
//!   ignition cut. It stands down during a drift hold (row 91 has its own throttle shaping), so
//!   full throttle held in a slide can still spin a powerful car.
//!
//! The smoothing amount is `DriftTuning::throttle_shaping` (drift lab slider); a negative value
//! means the preset's default ([`default_smoothing`]: keyboard 60 %, gamepad 20 %, wheel 0).
//! Same in Arcade and Simulation.

use wp_contracts::AssistProfile;

/// Shortest rise time (0 → full pedal) at full smoothing, s (high gears, speed, slow cars).
pub const RAMP_MIN_S: f32 = 0.25;
/// Extra rise time at full smoothing for a powerful RWD car in 1st / 2nd at low speed, s.
pub const RAMP_EXTRA_S: f32 = 0.65;
/// Speed (m/s) above which the low-gear part of the ramp is gone.
const RAMP_FADE_MPS: f32 = 20.0;
/// Largest pedal-target cut of the traction trim, at full smoothing.
pub const TRIM_MAX: f32 = 0.35;

/// Smoothing for a preset when the tuning asks for its default: keyboard (steering speed
/// sensitivity ≥ 0.95) 0.6, gamepad (other sensitivities) 0.2, wheel (0) 0.
pub fn default_smoothing(assists: &AssistProfile) -> f32 {
    if assists.steer_speed_sensitivity >= 0.95 {
        0.6
    } else if assists.steer_speed_sensitivity > 0.0 {
        0.2
    } else {
        0.0
    }
}

/// Smoothing in effect, 0..=1: `tuning` (≥ 0) or the preset default, and 0 with TCS on.
pub fn effective_smoothing(assists: &AssistProfile, tuning: f32) -> f32 {
    if assists.tcs {
        return 0.0;
    }
    let s = if tuning < 0.0 {
        default_smoothing(assists)
    } else {
        tuning
    };
    s.clamp(0.0, 1.0)
}

/// Rise time 0 → full pedal, s, for smoothing `s`, the car's slide class (power × RWD,
/// `crate::slide::slide_class`, 0.25..=1), gear and forward speed.
pub fn ramp_time_s(s: f32, class: f32, gear: i8, speed: f32) -> f32 {
    let low_gear = match gear {
        i8::MIN..=2 => 1.0,
        3 => 0.5,
        _ => 0.2,
    };
    let slow = (1.0 - speed.abs() / RAMP_FADE_MPS).clamp(0.0, 1.0);
    let power = ((class - 0.25) / 0.75).clamp(0.0, 1.0);
    s * (RAMP_MIN_S + RAMP_EXTRA_S * low_gear * (0.4 + 0.6 * slow) * power)
}

/// Traction trim multiplier on the pedal target (1 = none): `excess` = the worst driven
/// wheel's slip ratio past its peak, as a fraction of the peak (0 = at or under the peak, 1 =
/// twice the peak); `holding` = a drift hold is active.
pub fn traction_trim(s: f32, excess: f32, holding: bool) -> f32 {
    if holding || s <= 0.0 {
        return 1.0;
    }
    1.0 - TRIM_MAX * s * excess.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_and_tcs() {
        let kb = AssistProfile::keyboard_default();
        let off = AssistProfile { tcs: false, ..kb };
        assert_eq!(effective_smoothing(&kb, -1.0), 0.0, "TCS on does the job");
        assert_eq!(effective_smoothing(&off, -1.0), 0.6);
        let gp = AssistProfile {
            tcs: false,
            ..AssistProfile::gamepad_default()
        };
        assert_eq!(effective_smoothing(&gp, -1.0), 0.2);
        let wheel = AssistProfile {
            tcs: false,
            ..AssistProfile::wheel_default()
        };
        assert_eq!(effective_smoothing(&wheel, -1.0), 0.0);
        assert_eq!(effective_smoothing(&off, 0.3), 0.3);
    }

    #[test]
    fn ramp_is_slow_low_and_powerful_quick_high() {
        let muscle_2nd = ramp_time_s(1.0, 1.0, 2, 5.0);
        let muscle_5th = ramp_time_s(1.0, 1.0, 5, 30.0);
        let fwd_2nd = ramp_time_s(1.0, 0.25, 2, 5.0);
        assert!((0.8..=0.95).contains(&muscle_2nd), "{muscle_2nd}");
        assert!(muscle_5th < 0.4 && fwd_2nd < 0.3, "{muscle_5th} {fwd_2nd}");
        assert_eq!(ramp_time_s(0.0, 1.0, 1, 0.0), 0.0);
    }

    #[test]
    fn trim_is_bounded_and_off_in_a_hold() {
        assert_eq!(traction_trim(1.0, 0.0, false), 1.0);
        assert!((traction_trim(1.0, 5.0, false) - (1.0 - TRIM_MAX)).abs() < 1e-6);
        assert_eq!(traction_trim(1.0, 1.0, true), 1.0);
    }
}
