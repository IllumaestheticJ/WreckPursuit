//! Keyboard / gamepad drift "hold" (owner row 91): steering *into* the turn with the throttle
//! on while the rear slides asks the assist to hold the slide angle. It works only through the
//! steering angle and a bounded throttle trim: it never adds grip and never applies a yaw
//! torque, and it lets go past a slide-angle envelope, so mistakes still cost.
//!
//! - **Intent:** keys released → straighten (the self-aligning countersteer, rows 57 / 90);
//!   key against the slide → catch (row 90); key into the turn + throttle in a slide → hold.
//! - **Hold steering:** the front wheels go to the front axle's travel (no front slip), plus a
//!   little bite toward the corner, plus `hold_gain × (β − β*)`: more countersteer when the
//!   body slip β grows past the held angle β*, less when it shrinks. β* follows β slowly
//!   ([`TARGET_FOLLOW_PER_S`]), so throttle still grows or shrinks the angle.
//! - **Hold throttle (keyboard / gamepad pedal filter only):** the pedal target is trimmed by
//!   up to [`THROTTLE_TRIM_MAX`] while the slide is bigger than the held angle, fading out over
//!   the last 40 % of the envelope.
//! - **Envelope:** `envelope_deg × (0.6 + 0.4 × help)` (45° tuning: 37.8° at 60 %, 45° at
//!   100 %), up to [`ENVELOPE_GROWTH`] wider after [`STABLE_S`] of a steady hold. Past it the
//!   assist backs off ([`AssistMode::Exceeded`]): the driver's own steering applies and the car
//!   can spin.
//!
//! Same in Arcade and Simulation (the vehicle model never reads the damage mode).

use wp_contracts::AssistProfile;

use crate::steering::{AssistMode, SteerInput, STEER_DEADZONE};

/// How fast the held angle β* follows the actual body slip, 1/s.
pub const TARGET_FOLLOW_PER_S: f32 = 8.0;
/// Largest throttle cut of the hold, fraction of the pedal.
pub const THROTTLE_TRIM_MAX: f32 = 0.4;
/// The throttle trim fades to 0 over the last this fraction of the envelope.
const TRIM_FADE: f32 = 0.4;
/// Body-slip excess over the held angle (rad) at which the throttle trim is full.
const TRIM_FULL_RAD: f32 = 0.12;
/// Seconds of a steady hold (|β − β*| < [`STEADY_RAD`]) before the envelope starts growing.
pub const STABLE_S: f32 = 1.0;
/// Extra envelope after a long steady hold, fraction.
pub const ENVELOPE_GROWTH: f32 = 0.15;
/// "Steady": the slip stays this close to the held angle, rad.
const STEADY_RAD: f32 = 0.06;
/// Rear-axle slip (rad) above which a key into the turn with throttle is a drift to hold
/// (ordinary limit cornering with a held key stays well below it).
pub const HOLD_MIN_SLIP_RAD: f32 = 0.17;
/// Minimum throttle for a hold.
const HOLD_THROTTLE: f32 = 0.1;
/// Largest hold correction on top of the front axle's travel, rad.
pub const HOLD_CORR_MAX: f32 = 0.06;
/// Bite toward the corner on top of the front axle's travel, × the front tyres' peak slip.
const BITE: f32 = 0.5;

/// Hold state carried between ticks (part of the vehicle snapshot).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DriftHold {
    /// Held body slip angle β*, rad.
    pub target_slip: f32,
    /// Seconds of the current steady hold (0 when not holding / not steady).
    pub steady_s: f32,
    /// Holding this tick.
    pub active: bool,
}

/// One tick of the hold logic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HoldOut {
    /// Hold or Exceeded when the driver steers into the slide with throttle; Off otherwise.
    pub mode: AssistMode,
    /// Road-wheel centre angle to steer to (rad, + = left) while holding.
    pub angle: Option<f32>,
    /// Throttle-target multiplier (1 = untouched).
    pub throttle_scale: f32,
    /// Current envelope, degrees.
    pub envelope_deg: f32,
}

/// Envelope for slider value `help` and the hold's steadiness, degrees.
pub fn envelope_deg(inp: &SteerInput, help: f32, steady_s: f32) -> f32 {
    let growth = ((steady_s - STABLE_S) / STABLE_S).clamp(0.0, 1.0) * ENVELOPE_GROWTH;
    inp.tuning.envelope_deg * (0.6 + 0.4 * help) * (1.0 + growth)
}

/// Advance the hold for this tick: `throttle` = the driver's throttle input (0..=1),
/// `handbrake_pulled` = the handbrake is pulled right now, `max_steer` = the car's lock (rad).
/// Unlike the self-aligning countersteer the hold may act within the handbrake grace after a
/// release: a handbrake flick into a powered drift is how drifts start, and a plain handbrake
/// turn (no throttle) never asks for a hold.
pub fn hold_update(
    state: &mut DriftHold,
    inp: &SteerInput,
    assists: &AssistProfile,
    throttle: f32,
    handbrake_pulled: bool,
    max_steer: f32,
    dt: f32,
) -> HoldOut {
    let help = assists.countersteer_help.clamp(0.0, 1.0);
    let into_turn = inp.driver * inp.rear_axle_slip > 0.0 && inp.driver.abs() > STEER_DEADZONE;
    let wants = assists.steer_speed_sensitivity > 0.0
        && help > 0.0
        && inp.speed > 3.0
        && !handbrake_pulled
        && into_turn
        && throttle > HOLD_THROTTLE
        && inp.rear_axle_slip.abs() >= HOLD_MIN_SLIP_RAD;
    let beta = inp.body_slip;
    if !wants {
        *state = DriftHold {
            target_slip: beta,
            ..DriftHold::default()
        };
        return HoldOut {
            mode: AssistMode::Off,
            angle: None,
            throttle_scale: 1.0,
            envelope_deg: envelope_deg(inp, help, 0.0),
        };
    }
    if !state.active {
        state.target_slip = beta;
        state.steady_s = 0.0;
    }
    let env = envelope_deg(inp, help, state.steady_s);
    if beta.abs().to_degrees() > env {
        // Past the envelope: let go (the driver's own steering applies; the car may spin).
        *state = DriftHold {
            target_slip: beta,
            ..DriftHold::default()
        };
        return HoldOut {
            mode: AssistMode::Exceeded,
            angle: None,
            throttle_scale: 1.0,
            envelope_deg: env,
        };
    }
    state.active = true;
    state.target_slip += (beta - state.target_slip) * (TARGET_FOLLOW_PER_S * dt).min(1.0);
    let err = beta - state.target_slip;
    state.steady_s = if err.abs() < STEADY_RAD {
        state.steady_s + dt
    } else {
        0.0
    };
    // Driver steering left (−) = a left-hand corner: the front bites to the left (+).
    let bite = -inp.driver.signum() * BITE * inp.front_peak_slip;
    let full = max_steer * inp.ratio_scale;
    // Bounded correction: a steady push (full throttle held) can out-grow it, past the
    // envelope, and spin.
    let corr = (inp.tuning.hold_gain * err).clamp(-HOLD_CORR_MAX, HOLD_CORR_MAX);
    let angle = (inp.front_axle_angle + bite + corr).clamp(-full, full);
    // More slide than held: trim the throttle (keyboard / gamepad pedal filter only).
    let excess = (beta.abs() - state.target_slip.abs()).max(0.0);
    // The trim fades out toward the envelope: full throttle held at a big angle still grows
    // the slide past it, and the car can spin (punishment for mistakes).
    let room = ((env - beta.abs().to_degrees()) / (TRIM_FADE * env)).clamp(0.0, 1.0);
    let trim = if assists.throttle_filter > 0.0 {
        THROTTLE_TRIM_MAX * (excess / TRIM_FULL_RAD).min(1.0) * help * room
    } else {
        0.0
    };
    HoldOut {
        mode: AssistMode::Hold,
        angle: Some(angle),
        throttle_scale: 1.0 - trim,
        envelope_deg: env,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steering::DriftTuning;

    fn drifting(driver: f32) -> SteerInput {
        // Left-hand drift: rear sliding right (rear-axle slip −), body slip −0.3 rad.
        SteerInput {
            driver,
            speed: 15.0,
            body_slip: -0.3,
            rear_slip: -0.3,
            rear_axle_slip: -0.35,
            slide_class: 1.0,
            pull: 0.0,
            ratio_scale: 1.0,
            wheelbase: 2.6,
            front_mu: 1.1,
            front_peak_slip: 0.11,
            front_axle_angle: -0.2,
            handbrake_hold: false,
            tuning: DriftTuning::default(),
            hold_angle: None,
        }
    }

    #[test]
    fn holds_only_into_the_turn_with_throttle() {
        let kb = AssistProfile::keyboard_default();
        let mut s = DriftHold::default();
        let into = hold_update(&mut s, &drifting(-1.0), &kb, 1.0, false, 0.6, 1.0 / 240.0);
        assert_eq!(into.mode, AssistMode::Hold);
        let mut s = DriftHold::default();
        assert_eq!(
            hold_update(&mut s, &drifting(1.0), &kb, 1.0, false, 0.6, 1.0 / 240.0).mode,
            AssistMode::Off,
            "countersteer key: that is a catch"
        );
        let mut s = DriftHold::default();
        assert_eq!(
            hold_update(&mut s, &drifting(-1.0), &kb, 0.0, false, 0.6, 1.0 / 240.0).mode,
            AssistMode::Off,
            "no throttle"
        );
        let wheel = AssistProfile::wheel_default();
        let mut s = DriftHold::default();
        assert_eq!(
            hold_update(
                &mut s,
                &drifting(-1.0),
                &wheel,
                1.0,
                false,
                0.6,
                1.0 / 240.0
            )
            .mode,
            AssistMode::Off,
            "the wheel preset is untouched"
        );
    }

    #[test]
    fn more_slide_more_countersteer_and_less_throttle() {
        let kb = AssistProfile::keyboard_default();
        let mut s = DriftHold::default();
        let base = hold_update(&mut s, &drifting(-1.0), &kb, 1.0, false, 0.6, 1.0 / 240.0);
        // The slide grows by 0.1 rad beyond the held angle.
        let bigger = SteerInput {
            body_slip: -0.4,
            ..drifting(-1.0)
        };
        let out = hold_update(&mut s, &bigger, &kb, 1.0, false, 0.6, 1.0 / 240.0);
        assert!(
            out.angle.unwrap() < base.angle.unwrap(),
            "{out:?} vs {base:?}"
        );
        assert!(out.throttle_scale < 1.0 && out.throttle_scale >= 1.0 - THROTTLE_TRIM_MAX);
    }

    #[test]
    fn lets_go_past_the_envelope() {
        let kb = AssistProfile::keyboard_default();
        let mut s = DriftHold::default();
        let spin = SteerInput {
            body_slip: -0.9,
            ..drifting(-1.0)
        };
        let out = hold_update(&mut s, &spin, &kb, 1.0, false, 0.6, 1.0 / 240.0);
        assert_eq!(out.mode, AssistMode::Exceeded);
        assert!(out.angle.is_none() && out.throttle_scale == 1.0);
        // Envelope: 45° tuning → 37.8° at the keyboard's 60 %, 45° at 100 %.
        assert!((envelope_deg(&drifting(-1.0), 0.6, 0.0) - 37.8).abs() < 1e-3);
        assert!((envelope_deg(&drifting(-1.0), 1.0, 0.0) - 45.0).abs() < 1e-3);
        assert!(envelope_deg(&drifting(-1.0), 0.6, 3.0) > 37.8 * 1.14);
    }
}
