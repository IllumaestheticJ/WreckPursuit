//! Handbrake pivot aid (owner row 109, 2026-10-20): **an owner-approved exception to the
//! row-57 rule that no assist adds yaw torque**, for this case only. The reference game's
//! handbrake hairpin (clip 2: key + handbrake 1.0 s off the throttle at ~45–55 km/h turns the car
//! 150–180° within ~1.2 s) is about twice as fast as tyre physics allows (measured 67–78° by the
//! release, see `docs/integration/vehicle.md`), so a yaw torque helps the rotation:
//!
//! - only while the handbrake is pulled **and** a steer key is held; it turns the way the key
//!   points, scaled by key × handbrake, building up over the first [`BUILD_UP_S`] of the pull
//!   (a short tap stays mild);
//! - full up to [`FADE_START_MPS`] (55 km/h), fading to nothing at [`FADE_END_MPS`]
//!   (75 km/h), so a fast handbrake tap stays a mild kick;
//! - faded out as the yaw rate the key asks for nears [`PivotTuning::cap`] (it never adds
//!   to a spin that is already that fast);
//! - off the moment the handbrake is released: the catch (throttle + opposite lock) stays purely
//!   physical;
//! - keyboard and gamepad only (`steer_speed_sensitivity > 0`); the wheel preset never gets it.
//!
//! Strength = [`PivotTuning::accel`] (yaw acceleration, rad/s²; 0 = off). Deterministic, the
//! same in Arcade and Simulation. (Round 12 kept these two values in A's drift-lab
//! `DriftTuning`; Drift Lab W's lineage has no `DriftTuning`, so they live here.)

use wp_contracts::AssistProfile;

/// Default pivot aid strength, rad/s² (round 12's `HB_PIVOT_ACCEL`).
pub const HB_PIVOT_ACCEL: f32 = 7.0;
/// Default yaw-rate cap of the pivot aid, rad/s (round 12's `HB_PIVOT_CAP`).
pub const HB_PIVOT_CAP: f32 = 3.0;

/// Pivot aid strength and cap. [`Default`] = the shipped aid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PivotTuning {
    /// Yaw acceleration at full key + handbrake, rad/s². 0 = off.
    pub accel: f32,
    /// Yaw rate (rad/s) toward the key at which the aid has faded out.
    pub cap: f32,
}

impl Default for PivotTuning {
    fn default() -> Self {
        Self {
            accel: HB_PIVOT_ACCEL,
            cap: HB_PIVOT_CAP,
        }
    }
}

/// Forward speed (m/s) up to which the aid is at full strength (55 km/h: the clip-2 hairpin
/// entry is 45–55 km/h).
pub const FADE_START_MPS: f32 = 55.0 / 3.6;
/// Forward speed (m/s) at which it is gone (75 km/h). The lead's first spec was 60 → 100 km/h;
/// with it a 0.3 s handbrake tap at 80 km/h doubled its peak body slip (38–57° → 80–100°)
/// instead of staying a mild kick.
pub const FADE_END_MPS: f32 = 75.0 / 3.6;
/// Handbrake / steer input below which the aid is off.
const ON: f32 = 0.1;
/// The aid builds up over this long a handbrake pull, s: a short tap (0.3 s) gets little of it,
/// a 1 s hairpin pull nearly all.
pub const BUILD_UP_S: f32 = 0.4;

/// Yaw acceleration the aid adds this tick, rad/s² (+ = left, about the car's up axis).
/// `steer` = driver steer (−1 left … +1 right), `handbrake` 0..=1, `held_s` = how long the
/// handbrake has been pulled, `speed` = forward speed m/s, `yaw_rate` rad/s (+ = left).
pub fn pivot_yaw_accel(
    tuning: &PivotTuning,
    assists: &AssistProfile,
    handbrake: f32,
    held_s: f32,
    steer: f32,
    speed: f32,
    yaw_rate: f32,
) -> f32 {
    if tuning.accel <= 0.0
        || assists.steer_speed_sensitivity <= 0.0
        || handbrake <= ON
        || steer.abs() <= ON
    {
        return 0.0;
    }
    let speed_fade =
        ((FADE_END_MPS - speed.abs()) / (FADE_END_MPS - FADE_START_MPS)).clamp(0.0, 1.0);
    // Steering right (+) asks for a clockwise turn (negative yaw).
    let dir = -steer.signum();
    let rate_toward = yaw_rate * dir;
    let cap_fade = (1.0 - rate_toward / tuning.cap.max(0.1)).clamp(0.0, 1.0);
    let build = (held_s / BUILD_UP_S).clamp(0.0, 1.0);
    dir * tuning.accel * build * steer.abs().min(1.0) * handbrake.min(1.0) * speed_fade * cap_fade
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kb() -> AssistProfile {
        AssistProfile::keyboard_default()
    }

    #[test]
    fn only_with_handbrake_and_key_and_never_on_the_wheel() {
        let t = PivotTuning::default();
        assert!(
            pivot_yaw_accel(&t, &kb(), 1.0, 1.0, 1.0, 13.0, 0.0) < 0.0,
            "right key: clockwise"
        );
        assert!(pivot_yaw_accel(&t, &kb(), 1.0, 1.0, -1.0, 13.0, 0.0) > 0.0);
        assert_eq!(
            pivot_yaw_accel(&t, &kb(), 0.0, 1.0, 1.0, 13.0, 0.0),
            0.0,
            "no handbrake"
        );
        assert_eq!(
            pivot_yaw_accel(&t, &kb(), 1.0, 1.0, 0.0, 13.0, 0.0),
            0.0,
            "no key"
        );
        let wheel = AssistProfile::wheel_default();
        assert_eq!(pivot_yaw_accel(&t, &wheel, 1.0, 1.0, 1.0, 13.0, 0.0), 0.0);
        let off = PivotTuning { accel: 0.0, ..t };
        assert_eq!(pivot_yaw_accel(&off, &kb(), 1.0, 1.0, 1.0, 13.0, 0.0), 0.0);
    }

    #[test]
    fn builds_up_over_the_pull() {
        let t = PivotTuning::default();
        let at = |held: f32| pivot_yaw_accel(&t, &kb(), 1.0, held, -1.0, 10.0, 0.0);
        assert_eq!(at(0.0), 0.0);
        assert!((at(0.5 * BUILD_UP_S) - 0.5 * at(1.0)).abs() < 1e-4);
        assert_eq!(at(BUILD_UP_S), at(2.0));
    }

    #[test]
    fn fades_with_speed_and_at_the_yaw_cap() {
        let t = PivotTuning::default();
        let full = pivot_yaw_accel(&t, &kb(), 1.0, 1.0, -1.0, 10.0, 0.0);
        let mid = pivot_yaw_accel(&t, &kb(), 1.0, 1.0, -1.0, 65.0 / 3.6, 0.0);
        assert!((mid / full - 0.5).abs() < 1e-3, "{mid} / {full}");
        assert_eq!(
            pivot_yaw_accel(&t, &kb(), 1.0, 1.0, -1.0, 80.0 / 3.6, 0.0),
            0.0
        );
        // At the cap (turning left already that fast): nothing more.
        let cap = t.cap;
        assert_eq!(pivot_yaw_accel(&t, &kb(), 1.0, 1.0, -1.0, 10.0, cap), 0.0);
        // Spinning the other way: full help toward the key.
        assert_eq!(pivot_yaw_accel(&t, &kb(), 1.0, 1.0, -1.0, 10.0, -1.0), full);
    }
}
