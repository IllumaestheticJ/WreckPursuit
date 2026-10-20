//! Slide-control assists for keyboard / gamepad drivers (owner playtest, 2026-10-19: "give the
//! higher-power RWD cars a little more steering assist / gyro so we can control the slides").
//!
//! Two pieces, both scaled by [`AssistProfile::countersteer_help`] and doing nothing at all
//! when it is 0 (wheel / AI / sim purist stay bit-identical to the plain model):
//! - self-aligning countersteer (released keys point the front wheels along the front axle's
//!   travel): see [`crate::steering::steer_target`]. It replaced the yaw "gyro" (owner rows
//!   57 / 72, 2026-10-20): no assist adds yaw torque any more, so handbrake turns rotate.
//! - [`slide_throttle`]: with TCS off, a keyboard 0/1 throttle ramps in more gently while the
//!   rear is already loose, and is feathered a little unless the driver steers into the turn
//!   (a key cannot feather by itself).
//!
//! How strongly a car gets helped comes from [`slide_class`]: rear-drive share × power to
//! weight, so a 200+ W/kg RWD coupe gets the full effect and a FWD hatch a light touch.

use wp_contracts::{AssistProfile, DriveLayout, VehicleDef};

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

/// How much slide help this car gets, `CLASS_FLOOR..=1`, with the car's own differentials
/// ([`crate::diff::DiffSetup::for_car`]); see [`slide_class_with_rear_lock`].
pub fn slide_class(def: &VehicleDef) -> f32 {
    slide_class_with_rear_lock(def, crate::diff::DiffSetup::for_car(def).rear.power_lock())
}

/// Rear-diff effect on the slide class: the power × layout term is scaled by
/// `DIFF_CLASS_OPEN + DIFF_CLASS_PER_LOCK × rear lock` (open 0.85 … locked 1.15). A locking
/// rear keeps both rear tires spinning together, so power slides start sooner and carry more
/// yaw; an open rear flares the inside wheel instead and limits them.
pub const DIFF_CLASS_OPEN: f32 = 0.85;
/// See [`DIFF_CLASS_OPEN`].
pub const DIFF_CLASS_PER_LOCK: f32 = 0.3;

/// How much slide help this car gets, `CLASS_FLOOR..=1`: rises with rear-drive share × power
/// to weight (from [`CLASS_PW_LOW`] to [`CLASS_PW_HIGH`] W/kg), scaled by the rear diff's
/// steady lock under power (`rear_lock` 0 open … 1 locked, see [`DIFF_CLASS_OPEN`]). AWD cars
/// count their rear share at 60% (the front axle pulls them straight).
/// [`SLIDE_CLASS_OVERRIDES`] wins if listed.
pub fn slide_class_with_rear_lock(def: &VehicleDef, rear_lock: f32) -> f32 {
    if let Some((_, c)) = SLIDE_CLASS_OVERRIDES.iter().find(|(id, _)| *id == def.id) {
        return c.clamp(0.0, 1.0);
    }
    let pw = peak_power_w(def) / def.mass_kg.max(1.0);
    let power = ((pw - CLASS_PW_LOW) / (CLASS_PW_HIGH - CLASS_PW_LOW)).clamp(0.0, 1.0);
    let rear = rear_drive_share(&def.drivetrain.layout);
    let layout = if rear >= 1.0 { 1.0 } else { 0.6 * rear };
    let diff = DIFF_CLASS_OPEN + DIFF_CLASS_PER_LOCK * rear_lock.clamp(0.0, 1.0);
    (CLASS_FLOOR + (1.0 - CLASS_FLOOR) * layout * power * diff).clamp(CLASS_FLOOR, 1.0)
}

/// 0..=1 "how much is the rear sliding" from the rear-axle slip angle (rad).
pub fn slide_amount(rear_axle_slip: f32) -> f32 {
    ((rear_axle_slip.abs() - SLIDE_START) / (SLIDE_FULL - SLIDE_START)).clamp(0.0, 1.0)
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
