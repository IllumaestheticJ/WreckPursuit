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
    /// Direction of travel of the front axle relative to the heading, rad (+ = left): the
    /// centre road-wheel angle that leaves the front tyres with no slip angle. See
    /// [`front_axle_angle`].
    pub front_axle_angle: f32,
    /// Handbrake pulled, or released less than [`HANDBRAKE_GRACE_S`] ago: the self-aligning
    /// countersteer stays out of the way (handbrake turns rotate freely).
    pub handbrake_hold: bool,
    /// Highway catch latch ([`next_catch_key`]): the direction (−1 / +1) of the countersteer
    /// key held through a highway-speed catch, 0 when none. While that key stays held the
    /// catch stays on, through straight and into the swing back.
    pub catch_key: f32,
    /// Drift / countersteer tuning (dev override; defaults = the shipped constants).
    pub tuning: DriftTuning,
    /// Drift hold (owner row 91, [`crate::drift`]): the road-wheel centre angle to hold the
    /// slide with, when the driver holds the key into the turn with throttle on.
    pub hold_angle: Option<f32>,
}

/// Runtime tuning of the keyboard / gamepad slide assists (owner rows 90 / 91 / 98, the
/// testbed's drift lab). A dev override set per vehicle (`Vehicle::set_drift_tuning`); it is
/// not in the car data, not in replays or snapshots, and [`Default`] is exactly the shipped
/// behaviour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriftTuning {
    /// Rear-axle slip, rad, where released keys start turning the wheels toward the front
    /// axle's travel ([`ALIGN_SLIDE_START`]).
    pub align_onset_rad: f32,
    /// Rear-axle slip, rad, where that alignment is complete ([`ALIGN_SLIDE_FULL`]).
    pub align_full_rad: f32,
    /// Rack-speed boost per unit of countersteer help ([`ALIGN_RATE_BOOST`]): the aligning /
    /// countersteering rack moves at `max(lock rate, rack speed) × (1 + this × help)`.
    pub slew_boost: f32,
    /// Rear-axle slip, rad, where the save-the-car countersteer lock starts opening.
    pub counter_lock_onset_rad: f32,
    /// Rear-axle slip, rad, where it is fully open.
    pub counter_lock_full_rad: f32,
    /// (row 91) Gain of the "hold" mode that keeps the body slip angle steady.
    pub hold_gain: f32,
    /// Keyboard throttle smoothing with TCS off, 0..=1 (row 98, [`crate::throttle`]). Default
    /// 0 (owner row 104: TCS off means the driver gets the throttle they press); a lab-only
    /// option.
    pub throttle_shaping: f32,
    /// (row 91) Slide-angle envelope at countersteer help 100 %, degrees (scaled down with the
    /// slider).
    pub envelope_deg: f32,
    /// Drift "hold" mode (row 91, [`crate::drift`]) on. Default off (the owner rejected the
    /// key-into-the-turn hold, rows 106 / 111): a lab-only option.
    pub hold_enabled: bool,
}

impl Default for DriftTuning {
    fn default() -> Self {
        Self {
            align_onset_rad: ALIGN_SLIDE_START,
            align_full_rad: ALIGN_SLIDE_FULL,
            slew_boost: ALIGN_RATE_BOOST,
            counter_lock_onset_rad: SLIDE_START,
            counter_lock_full_rad: SLIDE_FULL,
            hold_gain: 1.0,
            throttle_shaping: 0.0,
            envelope_deg: 45.0,
            hold_enabled: false,
        }
    }
}

/// What the slide assists are doing this tick (telemetry; the drift lab readout).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AssistMode {
    /// Not acting (driver steering in grip, help 0, below 3 m/s, handbrake hold).
    #[default]
    Off,
    /// Released keys in a slide: the wheels align with the front axle's travel.
    Straighten,
    /// The driver steers toward the side the rear slides to: fast rack, open lock.
    Catch,
    /// (row 91) Holding a steady drift angle.
    Hold,
    /// (row 91) Past the slide envelope: the assist has backed off.
    Exceeded,
}

impl AssistMode {
    /// Stable numeric code for the wasm readout: 0 off, 1 straighten, 2 catch, 3 hold,
    /// 4 exceeded.
    pub fn code(self) -> u8 {
        match self {
            AssistMode::Off => 0,
            AssistMode::Straighten => 1,
            AssistMode::Catch => 2,
            AssistMode::Hold => 3,
            AssistMode::Exceeded => 4,
        }
    }
}

/// The slide-assist mode for steering inputs `inp` under `assists` (see [`AssistMode`]).
pub fn assist_mode(inp: &SteerInput, assists: &AssistProfile) -> AssistMode {
    let help = assists.countersteer_help.clamp(0.0, 1.0);
    if assists.steer_speed_sensitivity > 0.0 && countering(inp) {
        AssistMode::Catch
    } else if help > 0.0
        && inp.speed > 3.0
        && inp.driver.abs() <= STEER_DEADZONE
        && !inp.handbrake_hold
        && align_slide(inp) > 0.0
    {
        AssistMode::Straighten
    } else {
        AssistMode::Off
    }
}

/// The driver steers toward the side the rear slides to while it slides.
fn countering(inp: &SteerInput) -> bool {
    -inp.driver * inp.rear_axle_slip > 0.0 && align_slide(inp) > 0.0
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

/// Driver steer inputs within this of 0 count as "not steering": released keys and a stick
/// inside its deadzone (the input layer applies the stick deadzone and sends 0). Kept tiny so
/// a slowly growing analog input steers from the first degree.
pub const STEER_DEADZONE: f32 = 1.0e-3;
/// After the handbrake is released, the self-aligning countersteer stays off this long, s.
pub const HANDBRAKE_GRACE_S: f32 = 0.5;
/// Rack-speed multiplier of the self-aligning countersteer at slider 100 % (`1 + this × help`:
/// 25 % → 2×, 50 % → 3×, 100 % → 5×).
pub const ALIGN_RATE_BOOST: f32 = 4.0;

/// How far the self-aligning countersteer turns the wheels toward the front axle's direction
/// of travel for slider value `help` (0..=1): `1 − (1 − help)²` (0 → none, 25 % → 0.44,
/// 50 % → 0.75, 75 % → 0.94, 100 % → full alignment). Together with the rack speed
/// ([`ALIGN_RATE_BOOST`]) each step of the slider is about as noticeable as the last: the low
/// end mostly adds angle, the high end mostly speed.
pub fn align_strength(help: f32) -> f32 {
    let h = help.clamp(0.0, 1.0);
    1.0 - (1.0 - h) * (1.0 - h)
}

/// Rear-axle slip angle (rad) where the countersteer lock starts / is fully open (0.06 → 0.16
/// before owner row 90: the save-the-car lock now opens as the slide begins).
const SLIDE_START: f32 = 0.03;
const SLIDE_FULL: f32 = 0.10;

/// Speed-sensitive lock while countersteering a slide: when the driver steers toward the side
/// the rear slides to, the lock opens (blended by how much
/// the rear slides) to that angle plus the slip-targeted margin, so the slip-limited lock never
/// stops a countersteer. Grip cornering is untouched.
fn countersteer_lock(inp: &SteerInput, assists: &AssistProfile, max_steer: f32, lock: f32) -> f32 {
    // Countersteering = steering toward the side the rear axle slides to (a right-hand
    // countersteer for the rear stepping out right). In ordinary cornering the driver steers
    // the other way, so the lock is untouched there.
    if assists.steer_speed_sensitivity <= 0.0 || -inp.driver * inp.rear_axle_slip <= 0.0 {
        return lock;
    }
    let t = &inp.tuning;
    let slide = ((inp.rear_axle_slip.abs() - t.counter_lock_onset_rad)
        / (t.counter_lock_full_rad - t.counter_lock_onset_rad).max(1e-4))
    .clamp(0.0, 1.0);
    if slide <= 0.0 {
        return lock;
    }
    let margin = slip_target_lock(
        assists.steer_speed_sensitivity,
        1.0e3,
        inp.wheelbase,
        inp.front_mu,
        inp.front_peak_slip,
    );
    let needed = (inp.front_axle_angle.abs() + margin).min(max_steer * inp.ratio_scale);
    lock + (needed.max(lock) - lock) * slide
}

/// Where the steering is heading this tick: the centre road-wheel angle the rack moves toward
/// (rad, + = left) and the rack-speed multiplier. See [`update_center_angle`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SteerTarget {
    /// Target centre road-wheel angle, rad (+ = left), within ±`max_steer`.
    pub angle: f32,
    /// Rack-speed multiplier.
    pub rate_boost: f32,
}

/// The steering target for the current state (`current` = centre angle now, rad; unused,
/// kept for callers).
///
/// Countersteer help (`AssistProfile::countersteer_help`, the 0–100 % slider) is
/// **self-aligning steering**, Wreckfest-style (owner rows 57 / 72): while the driver is not
/// steering (|input| ≤ [`STEER_DEADZONE`]) and moving forward above 3 m/s, the wheels go to
/// [`align_strength`] × the front axle's direction of travel ([`SteerInput::front_axle_angle`],
/// within the car's lock) once the rear slides, to the centre while the tyres grip (blended by
/// rear-axle slip, see [`ALIGN_SLIDE_START`]), and the rack moves up to [`ALIGN_RATE_BOOST`]
/// faster. In a slide that is exactly the countersteer that cancels the front slip angle. It never acts while the driver steers (the
/// speed-sensitive lock and rate apply as usual), nor while the handbrake is held or within
/// [`HANDBRAKE_GRACE_S`] of releasing it, and it adds no yaw torque. With help 0 this is
/// exactly the plain model.
pub fn steer_target(
    _current: f32,
    inp: &SteerInput,
    assists: &AssistProfile,
    max_steer: f32,
) -> SteerTarget {
    let lock = countersteer_lock(
        inp,
        assists,
        max_steer,
        driver_lock(inp, assists, max_steer),
    );
    let driver_target = -(inp.driver.clamp(-1.0, 1.0) + inp.pull) * lock;
    let help = assists.countersteer_help.clamp(0.0, 1.0);
    if let Some(angle) = inp.hold_angle {
        // Drift hold (row 91): the hold logic picked the angle; move there at the fast rate.
        return SteerTarget {
            angle: angle.clamp(-max_steer, max_steer),
            rate_boost: 1.0 + inp.tuning.slew_boost * help,
        };
    }
    let aligning =
        help > 0.0 && inp.speed > 3.0 && inp.driver.abs() <= STEER_DEADZONE && !inp.handbrake_hold;
    if !aligning {
        let catch = highway_catch(inp, assists);
        return SteerTarget {
            // The catch angle: the front axle's direction of travel (front slip zero). Main
            // kept ~10 % of the driver's lock on top; here that trim was the swing back.
            angle: lerp(driver_target, inp.front_axle_angle, catch).clamp(-max_steer, max_steer),
            // At highway speed a catch gets the full rack boost whatever the preset (a
            // slammed stick over-countersteers just like a key).
            rate_boost: 1.0 + inp.tuning.slew_boost * lerp(help, 1.0, catch) * catch,
        };
    }
    let full = max_steer * inp.ratio_scale;
    // At highway speed the released keys align fully whatever the preset: the swing back
    // after a catch carries the car into a slide the other way unless the wheels follow the
    // front axle all the way (the job main's highway gyro damper did).
    let help = lerp(help, 1.0, highway_amount(inp.speed));
    let align = aligning_angle(inp).clamp(-full, full) * align_strength(help);
    SteerTarget {
        angle: (align - inp.pull * lock).clamp(-max_steer, max_steer),
        rate_boost: 1.0 + inp.tuning.slew_boost * help,
    }
}

/// Speeds (km/h) where the highway catch ([`highway_catch`]) starts / is full.
pub const HIGHWAY_KMH: (f32, f32) = (60.0, 110.0);

/// 0..=1 "how much is this highway speed" from forward speed (m/s): 0 up to
/// [`HIGHWAY_KMH`]`.0`, 1 from [`HIGHWAY_KMH`]`.1`. Above it a slide is a mistake to catch,
/// not a drift to hold.
pub fn highway_amount(speed: f32) -> f32 {
    let kmh = speed.abs() * 3.6;
    ((kmh - HIGHWAY_KMH.0) / (HIGHWAY_KMH.1 - HIGHWAY_KMH.0)).clamp(0.0, 1.0)
}

/// Highway catch (main's high-speed catch fix, ported onto the self-aligning steering), 0..=1:
/// how far a held key's target moves from the driver's lock toward the catch angle (the front
/// axle's direction of travel, front slip zero) at highway speed with countersteer help on.
///
/// The countersteer key opens the lock past the front axle's travel by the slip margin, and
/// as the slide ends that lock shrinks back to the grip-limit lock, which a key still held
/// then asks for the other way: the swing back through straight became a slide the other way
/// (owner playtest 2026-10-20, the snap-back). While the driver countersteers the catch stays
/// on until the rear is back in line; a key held into the new slide (the countersteer key
/// held too long) is caught the same way. Street speed (and the wheel profile, help 0) are
/// unchanged, so flicks and drifts stay playful.
pub fn highway_catch(inp: &SteerInput, assists: &AssistProfile) -> f32 {
    let help = assists.countersteer_help.clamp(0.0, 1.0);
    if help <= 0.0
        || assists.steer_speed_sensitivity <= 0.0
        || inp.handbrake_hold
        || inp.driver.abs() <= STEER_DEADZONE
    {
        return 0.0;
    }
    let highway = highway_amount(inp.speed);
    if highway <= 0.0 {
        return 0.0;
    }
    if inp.catch_key != 0.0 && inp.driver * inp.catch_key > 0.0 {
        // The countersteer key of a catch still held (late release): keep catching.
        return highway;
    }
    let slide = align_slide(inp);
    let countersteering = -inp.driver * inp.rear_axle_slip > 0.0;
    let weight = if countersteering {
        // Full from the onset of the slide all the way back to straight.
        slide.max((inp.rear_axle_slip.abs() / inp.tuning.align_onset_rad.max(1e-4)).min(1.0))
    } else {
        slide
    };
    highway * weight
}

/// The highway catch latch after this tick ([`SteerInput::catch_key`]): set to the key's
/// direction while the driver countersteers a slide at highway speed (catch more than half
/// on), kept while that key stays held, cleared when it is released or reversed, or below
/// highway speed.
pub fn next_catch_key(inp: &SteerInput, assists: &AssistProfile) -> f32 {
    if highway_amount(inp.speed) <= 0.0 || inp.driver.abs() <= STEER_DEADZONE {
        return 0.0;
    }
    if inp.catch_key != 0.0 && inp.driver * inp.catch_key > 0.0 {
        return inp.catch_key;
    }
    let countersteering = -inp.driver * inp.rear_axle_slip > 0.0;
    if countersteering && highway_catch(inp, assists) > 0.5 {
        inp.driver.signum()
    } else {
        0.0
    }
}

/// Rear-axle slip angle (rad) where the self-aligning countersteer starts turning the wheels
/// toward the front axle's travel / turns them all the way (below it they return to centre).
pub const ALIGN_SLIDE_START: f32 = 0.03;
/// See [`ALIGN_SLIDE_START`].
pub const ALIGN_SLIDE_FULL: f32 = 0.08;

/// Where released keys point the wheels (rad, + = left) before [`align_strength`]: the
/// centre while the tyres grip, the front axle's direction of travel in a slide, blended by
/// how much the rear axle slides ([`ALIGN_SLIDE_START`] → [`ALIGN_SLIDE_FULL`]).
///
/// In grip driving the front axle's travel includes the car's own yaw (it is the steer angle
/// of a car cornering with no front slip), so aligning to it would hold whatever turn the car
/// is in: a kerb or a bump that yaws the car would keep turning it (the sidewalk drives in
/// `wp_sim` were turned back onto the road that way).
fn aligning_angle(inp: &SteerInput) -> f32 {
    inp.front_axle_angle * align_slide(inp)
}

/// How much the rear slides, 0..=1, on the self-aligning countersteer's ramp (rear-axle slip
/// `align_onset_rad` → `align_full_rad`).
pub fn align_slide_amount(inp: &SteerInput) -> f32 {
    align_slide(inp)
}

/// How much the rear slides for the self-aligning countersteer, 0..=1 (rear-axle slip
/// [`ALIGN_SLIDE_START`] → [`ALIGN_SLIDE_FULL`]).
fn align_slide(inp: &SteerInput) -> f32 {
    let t = &inp.tuning;
    ((inp.rear_axle_slip.abs() - t.align_onset_rad)
        / (t.align_full_rad - t.align_onset_rad).max(1e-4))
    .clamp(0.0, 1.0)
}

/// Direction of travel of the front axle relative to the heading, rad (+ = left), from forward
/// / leftward CG speed, yaw rate (+ = left) and the CG-to-front-axle distance. 0 below 1 m/s
/// forward (and in reverse).
pub fn front_axle_angle(v_forward: f32, v_left: f32, yaw_rate: f32, cg_to_front: f32) -> f32 {
    if v_forward < 1.0 {
        0.0
    } else {
        atan2(v_left + yaw_rate * cg_to_front, v_forward)
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
        // The rack covers the lock in use (opened while countersteering) in the ramp time.
        let lock = countersteer_lock(
            inp,
            assists,
            max_steer,
            driver_lock(inp, assists, max_steer),
        );
        (lock / (STEER_RAMP_S * s)).min(rack_speed * inp.ratio_scale)
    } else {
        rack_speed * inp.ratio_scale
    };
    // Catching a slide (owner row 90): the self-aligning countersteer, and the driver steering
    // toward the side the rear slides to, never move slower than the car's own rack (the
    // slip-targeted lock shrinks with speed, and so would the rate), times the slider's boost.
    let (base, rate_boost) = if s > 0.0 && countering(inp) {
        let help = assists.countersteer_help.clamp(0.0, 1.0);
        (
            base.max(rack_speed * inp.ratio_scale),
            1.0 + inp.tuning.slew_boost * help,
        )
    } else if rate_boost > 1.0 {
        (base.max(rack_speed * inp.ratio_scale), rate_boost)
    } else {
        (base, rate_boost)
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
            front_axle_angle: 0.0,
            handbrake_hold: false,
            catch_key: 0.0,
            tuning: DriftTuning::default(),
            hold_angle: None,
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
        // Left-hand power oversteer: the front axle travels 0.25 rad right of the heading.
        SteerInput {
            body_slip: -0.15,
            rear_slip: -0.2,
            rear_axle_slip: -0.3,
            front_axle_angle: -0.25,
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

    fn help(h: f32) -> AssistProfile {
        AssistProfile {
            countersteer_help: h,
            ..AssistProfile::keyboard_default()
        }
    }

    #[test]
    fn released_keys_align_the_wheels_with_the_travel() {
        // 0 %: plain return to centre. 100 %: the full front-axle angle. 50 %: in between.
        assert_eq!(settle(&sliding(0.0), &help(0.0)), 0.0);
        let full = settle(&sliding(0.0), &help(1.0));
        assert!((full + 0.25).abs() < 1e-4, "{full}");
        let half = settle(&sliding(0.0), &help(0.5));
        assert!(half < -0.15 && half > full, "{half}");
        // Driving straight it is ~0.
        assert!(settle(&straight(0.0, 20.0), &help(1.0)).abs() < 1e-6);
    }

    #[test]
    fn released_keys_centre_the_wheels_while_the_tyres_grip() {
        // A car yawing with the rear gripping (a kerb kick, a released corner): the wheels go to
        // the centre, not along the front axle's travel (that would hold the turn).
        let grip = |rear: f32| SteerInput {
            rear_axle_slip: rear,
            ..sliding(0.0)
        };
        assert!(settle(&grip(0.01), &help(1.0)).abs() < 1e-6);
        let part = settle(&grip(-0.055), &help(1.0));
        assert!(part < -0.1 && part > -0.15, "{part}");
        assert!((settle(&grip(-0.1), &help(1.0)) + 0.25).abs() < 1e-4);
    }

    #[test]
    fn the_slider_speeds_up_the_catch() {
        // Ticks to reach 0.08 rad of countersteer (every setting from 25 % aims past it).
        let t = |h: f32| {
            let (mut c, mut n) = (0.0f32, 0);
            while c > -0.08 {
                c = update_center_angle(c, &sliding(0.0), &help(h), 0.6, 1.0, 0.004);
                n += 1;
            }
            n
        };
        assert!(
            t(1.0) < t(0.5) && t(0.5) < t(0.25),
            "{} {} {}",
            t(1.0),
            t(0.5),
            t(0.25)
        );
    }

    #[test]
    fn a_countersteer_key_moves_at_least_at_the_rack_speed_scaled_by_the_slider() {
        // Owner row 90: at 100 km/h the slip-targeted lock is small, so the keyboard rate tied
        // to it took ~130 ms to swing into a countersteer. Now: never slower than the rack,
        // times 1 + 4 × help.
        let inp = SteerInput {
            speed: 100.0 / 3.6,
            ..sliding(1.0)
        };
        let ticks = |h: f32| {
            let (mut c, mut n) = (0.05f32, 0);
            while c > -0.1 && n < 1000 {
                c = update_center_angle(c, &inp, &help(h), 0.6, 1.4, 1.0 / 240.0);
                n += 1;
            }
            n
        };
        let full_rack = (0.15 / 1.4 * 240.0) as usize;
        assert!(ticks(0.0) <= full_rack + 1, "{}", ticks(0.0));
        assert!(ticks(0.3) < ticks(0.0) && ticks(1.0) < ticks(0.3));
        // 60 %: under 50 ms (3.4 × the rack).
        assert!(ticks(0.6) <= 12, "{}", ticks(0.6));
    }

    #[test]
    fn never_fights_the_driver_or_the_handbrake() {
        let a = help(1.0);
        // Any steer input: exactly the plain target (as with help 0).
        for d in [0.3f32, -0.3, 1.0, -1.0] {
            assert_eq!(
                steer_target(0.0, &sliding(d), &a, 0.6),
                steer_target(0.0, &sliding(d), &help(0.0), 0.6)
            );
        }
        // Handbrake held (or just released): no alignment, plain return to centre.
        let hb = SteerInput {
            handbrake_hold: true,
            catch_key: 0.0,
            ..sliding(0.0)
        };
        assert_eq!(settle(&hb, &a), 0.0);
    }
}
