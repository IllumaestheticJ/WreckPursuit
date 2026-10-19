//! Slide-control assists for keyboard / gamepad drivers (owner playtest, 2026-10-19: "give the
//! higher-power RWD cars a little more steering assist / gyro so we can control the slides").
//!
//! Three pieces, all scaled by [`AssistProfile::countersteer_help`] and doing nothing at all
//! when it is 0 (wheel / AI / sim purist stay bit-identical to the plain model):
//! - [`gyro_yaw_torque`]: a yaw damper that acts only while the rear is sliding. It damps the
//!   *rate of change* of the slide angle (yaw rate minus the rotation rate of the velocity
//!   vector), not the slide angle itself, so a snap into a spin is slowed down enough to
//!   catch. With the steering centred it aims for a slowly shrinking slide angle (the car
//!   straightens up on its own); steering into the turn holds the drift, steering against it
//!   straightens faster.
//! - countersteer targeting and rack speed-up: see [`crate::steering::update_center_angle`].
//! - [`slide_throttle`]: with TCS off, a keyboard 0/1 throttle ramps in more gently while the
//!   rear is already loose, and is feathered a little unless the driver steers into the turn
//!   (a key cannot feather by itself).
//!
//! How strongly a car gets helped comes from [`slide_class`]: rear-drive share × power to
//! weight, so a 200+ W/kg RWD coupe gets the full effect and a FWD hatch a light touch.

use wp_contracts::{AssistProfile, DriveLayout, VehicleDef};

use crate::math::atan2;

/// Power-to-weight (W/kg) where the car-class scaling starts to rise.
pub const CLASS_PW_LOW: f32 = 120.0;
/// Power-to-weight (W/kg) at which the car-class scaling is full.
pub const CLASS_PW_HIGH: f32 = 250.0;
/// Class floor: every car gets this much of the slide help (lift-off and handbrake slides
/// happen in FWD cars too).
pub const CLASS_FLOOR: f32 = 0.25;

/// Per-car override of [`slide_class`] (car id, class 0..=1), the tuning knob for cars whose
/// derived class feels wrong. The car JSON schema rejects unknown fields, so this lives here
/// until the contract gains an optional field (vehicle.md, contract request 7).
pub const SLIDE_CLASS_OVERRIDES: &[(&str, f32)] = &[];

/// Rear-slip angle (rad) where the slide assists start / are fully on.
const SLIDE_START: f32 = 0.06;
const SLIDE_FULL: f32 = 0.16;
/// Gyro gain at full help, full class and a full slide, 1/s: the yaw torque is
/// `gain × I_zz × (yaw rate − path rate)`.
const GYRO_GAIN: f32 = 9.0;
/// While the slide angle is shrinking (the car straightening up) the damper is weaker, so it
/// slows a snap-back without holding the car in the slide.
const GYRO_RECOVER_SCALE: f32 = 0.4;
/// Straighten-up rate with the steering centred, 1/s: the gyro aims for a slide angle that
/// decays as `exp(-K t)`.
const GYRO_STRAIGHTEN: f32 = 0.9;
/// Torque cap as a fraction of `m · g · wheelbase / 2` (about what the rear tires can make).
const GYRO_CAP: f32 = 0.35;

/// Peak engine power from the torque curve, W.
pub fn peak_power_w(def: &VehicleDef) -> f32 {
    def.engine
        .torque_curve_nm
        .iter()
        .map(|(rpm, nm)| rpm * nm * (std::f32::consts::PI / 30.0))
        .fold(0.0, f32::max)
}

/// Fraction of drive torque that normally goes to the rear axle.
fn rear_drive_share(layout: &DriveLayout) -> f32 {
    match *layout {
        DriveLayout::Fwd => 0.0,
        DriveLayout::Rwd => 1.0,
        DriveLayout::Awd { front_split } => 1.0 - front_split,
        // The coupling can send up to `max_rear_split` rearward; count half of that headroom.
        DriveLayout::AwdOnDemand {
            base_front_split,
            max_rear_split,
            ..
        } => 0.5 * ((1.0 - base_front_split) + max_rear_split),
    }
}

/// How much slide help this car gets, `CLASS_FLOOR..=1`: rises with rear-drive share × power
/// to weight (from [`CLASS_PW_LOW`] to [`CLASS_PW_HIGH`] W/kg). AWD cars count their rear share
/// at 60% (the front axle pulls them straight). [`SLIDE_CLASS_OVERRIDES`] wins if listed.
pub fn slide_class(def: &VehicleDef) -> f32 {
    if let Some((_, c)) = SLIDE_CLASS_OVERRIDES.iter().find(|(id, _)| *id == def.id) {
        return c.clamp(0.0, 1.0);
    }
    let pw = peak_power_w(def) / def.mass_kg.max(1.0);
    let power = ((pw - CLASS_PW_LOW) / (CLASS_PW_HIGH - CLASS_PW_LOW)).clamp(0.0, 1.0);
    let rear = rear_drive_share(&def.drivetrain.layout);
    let layout = if rear >= 1.0 { 1.0 } else { 0.6 * rear };
    CLASS_FLOOR + (1.0 - CLASS_FLOOR) * layout * power
}

/// 0..=1 "how much is the rear sliding" from the rear-axle slip angle (rad).
pub fn slide_amount(rear_axle_slip: f32) -> f32 {
    ((rear_axle_slip.abs() - SLIDE_START) / (SLIDE_FULL - SLIDE_START)).clamp(0.0, 1.0)
}

/// Inputs to [`gyro_yaw_torque`], all chassis-frame and measured this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GyroInput {
    /// Forward speed, m/s.
    pub v_fwd: f32,
    /// Leftward speed, m/s.
    pub v_left: f32,
    /// Yaw rate, rad/s (+ = turning left).
    pub yaw_rate: f32,
    /// Horizontal force on the chassis this tick, chassis frame `(left, forward)`, N.
    pub force_left: f32,
    /// See `force_left`.
    pub force_fwd: f32,
    /// Rear-axle slip angle, rad.
    pub rear_axle_slip: f32,
    /// Mass, kg.
    pub mass: f32,
    /// Yaw inertia, kg·m².
    pub yaw_inertia: f32,
    /// Wheelbase, m.
    pub wheelbase: f32,
    /// Driver steer input, -1 (left) ..= 1 (right).
    pub driver_steer: f32,
}

/// Yaw torque (N·m about chassis up, + = left) of the slide gyro, scaled by
/// `min(2·help, 1) × class`. 0 unless help > 0, the car
/// is moving forward faster than 5 m/s and the rear is sliding.
///
/// "Path rate" is how fast the velocity vector itself turns (lateral acceleration / speed);
/// yaw rate minus path rate is how fast the slide angle changes. The gyro pushes that toward
/// a target: slide angle shrinking at [`GYRO_STRAIGHTEN`] with the steering centred, held
/// (target 0, a pure damper) with full steer into the turn, shrinking twice as fast with full
/// countersteer. Over-rotation is damped at full gain, straightening faster than the target
/// at 40%.
pub fn gyro_yaw_torque(g: &GyroInput, help: f32, class: f32) -> f32 {
    if help <= 0.0 || g.v_fwd < 5.0 {
        return 0.0;
    }
    let slide = slide_amount(g.rear_axle_slip);
    if slide <= 0.0 {
        return 0.0;
    }
    let v2 = g.v_fwd * g.v_fwd + g.v_left * g.v_left;
    // Rotation rate of the velocity vector: (v × a)·up / |v|² (x = right, y = forward).
    let path_rate = (g.v_fwd * g.force_left - g.v_left * g.force_fwd) / (g.mass * v2);
    // Slide-angle rate is path − yaw; "excess" = yaw − path (> 0 = rotating left of the path).
    let excess = g.yaw_rate - path_rate;
    let beta = atan2(g.v_left, g.v_fwd);
    let (hold, counter) = slide_intent(g.driver_steer, g.rear_axle_slip);
    let k = GYRO_STRAIGHTEN * (1.0 - hold) * (1.0 + counter);
    // Wanted slide-angle rate −k·β ⇒ wanted excess k·β.
    let err = excess - k * beta;
    // err · β < 0 = rotating toward more slide than wanted.
    let over_rotating = err * beta <= 0.0;
    let scale = if over_rotating {
        1.0
    } else {
        GYRO_RECOVER_SCALE
    };
    // Full gyro from help 0.5 up (keyboard 0.6 → full, gamepad 0.3 → 60%).
    let gain = GYRO_GAIN * (2.0 * help).min(1.0) * class * slide * scale;
    let cap = GYRO_CAP * g.mass * wp_contracts::GRAVITY_MPS2 * g.wheelbase * 0.5;
    (-gain * g.yaw_inertia * err).clamp(-cap, cap)
}

/// Driver intent while sliding, from steer input (+ = right) and rear-axle slip (+ = rear
/// moving left): `(hold, counter)`, each 0..=1. `hold` = steering into the turn (asking for
/// slide angle), `counter` = steering against it.
pub fn slide_intent(driver_steer: f32, rear_axle_slip: f32) -> (f32, f32) {
    let side = driver_steer.clamp(-1.0, 1.0) * rear_axle_slip.signum();
    (side.clamp(0.0, 1.0), (-side).clamp(0.0, 1.0))
}

/// Slide-throttle assist output for one tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlideThrottle {
    /// Pedal filter strength to use (see [`crate::assists::filter_pedal`]).
    pub filter: f32,
    /// Multiplier on the throttle target, 0.65..=1.
    pub scale: f32,
}

/// Largest throttle-target cut of the feathering, at full filter/class/slide.
const FEATHER_MAX: f32 = 0.35;

/// Slide throttle (keyboard help, TCS off): while the rear slides, the pedal filter is up to
/// `1 + 1.5 × class` times stronger, and the target is feathered by up to [`FEATHER_MAX`] ×
/// `throttle_filter` × class unless the driver steers into the turn. Only with TCS off,
/// countersteer help on and a throttle filter (with TCS on, the TCS does this job; an
/// unfiltered pedal is never touched), so the wheel/AI/sim presets are unaffected.
pub fn slide_throttle(
    assists: &AssistProfile,
    class: f32,
    rear_axle_slip: f32,
    driver_steer: f32,
) -> SlideThrottle {
    let base = assists.throttle_filter;
    if assists.tcs || assists.countersteer_help <= 0.0 || base <= 0.0 {
        return SlideThrottle {
            filter: base,
            scale: 1.0,
        };
    }
    let help = (2.0 * assists.countersteer_help).min(1.0);
    let slide = slide_amount(rear_axle_slip);
    let (hold, _) = slide_intent(driver_steer, rear_axle_slip);
    SlideThrottle {
        filter: base * (1.0 + 1.5 * class * help * slide),
        scale: 1.0 - FEATHER_MAX * base.min(1.0) * class * help * slide * (1.0 - hold),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn car(id: &str) -> VehicleDef {
        let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
        wp_contracts::load_car(p).unwrap().def
    }

    #[test]
    fn class_favours_powerful_rwd() {
        let rwd = slide_class(&car("lucciola_350"));
        let mid = slide_class(&car("hakone_gt"));
        let fwd = slide_class(&car("kestrel_ts"));
        let slow_rwd = slide_class(&car("sentinel_bastion"));
        assert!(rwd > 0.7, "{rwd}");
        assert!(mid > fwd && mid < rwd, "{mid}");
        assert_eq!(fwd, CLASS_FLOOR);
        assert_eq!(slow_rwd, CLASS_FLOOR);
    }

    fn spin_in() -> GyroInput {
        // 20 m/s, sliding right of the heading (rear out to the right in a left turn), the
        // body rotating left faster than its path.
        GyroInput {
            v_fwd: 20.0,
            v_left: -4.0,
            yaw_rate: 1.2,
            force_left: 8000.0,
            force_fwd: 0.0,
            rear_axle_slip: -0.3,
            mass: 1500.0,
            yaw_inertia: 2500.0,
            wheelbase: 2.7,
            driver_steer: -1.0,
        }
    }

    #[test]
    fn gyro_resists_spin_and_is_off_without_help() {
        let t = gyro_yaw_torque(&spin_in(), 0.6, 1.0);
        assert!(t < 0.0, "{t}");
        assert_eq!(gyro_yaw_torque(&spin_in(), 0.0, 1.0), 0.0);
        let held = GyroInput {
            // Steady drift: yaw rate equals the path rate.
            yaw_rate: (20.0 * 8000.0 - (-4.0) * 0.0) / (1500.0 * 416.0),
            ..spin_in()
        };
        // Steady drift with the driver steering into the turn: held (pure damper, ~0).
        assert!(gyro_yaw_torque(&held, 0.6, 1.0).abs() < 1.0);
        // Same drift with the steering centred: nudged straight (yaw to the right).
        let neutral = GyroInput {
            driver_steer: 0.0,
            ..held
        };
        assert!(gyro_yaw_torque(&neutral, 0.6, 1.0) < -100.0);
        let straightening = GyroInput {
            yaw_rate: -0.5,
            ..spin_in()
        };
        let r = gyro_yaw_torque(&straightening, 0.6, 1.0);
        assert!(r > 0.0 && r < -t, "{r} {t}");
    }

    #[test]
    fn slide_throttle_only_without_tcs() {
        let kb = AssistProfile::keyboard_default();
        let plain = SlideThrottle {
            filter: kb.throttle_filter,
            scale: 1.0,
        };
        assert_eq!(slide_throttle(&kb, 1.0, -0.3, 0.0), plain);
        let off = AssistProfile { tcs: false, ..kb };
        let st = slide_throttle(&off, 1.0, -0.3, 0.0);
        assert!(st.filter > kb.throttle_filter && st.scale < 0.8, "{st:?}");
        // Steering into the turn (left, rear sliding right): no feathering.
        assert_eq!(slide_throttle(&off, 1.0, -0.3, -1.0).scale, 1.0);
        assert_eq!(slide_throttle(&off, 1.0, 0.0, 0.0), plain);
        let wheel = AssistProfile::wheel_default();
        let w = slide_throttle(&wheel, 1.0, -0.3, 0.0);
        assert_eq!((w.filter, w.scale), (0.0, 1.0));
    }
}
