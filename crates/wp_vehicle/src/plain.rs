//! Drift Lab W: the **plain** keyboard steering profile (no intent modes).
//!
//! The reference game's keyboard steering, as measured from the owner's clips
//! (`docs/reference/wreckfest-keyboard.md`, "Keyboard input is raw" and "Steering-wheel
//! timing"): the key reaches the car as a raw −1 / 0 / +1 and the steering rack moves toward
//! `key × lock(speed)` at a **constant rate**, full lock in [`PlainSteer::t_full_s`] (0.28 s)
//! and back to centre in [`PlainSteer::t_return_s`] (0.33 s). Nothing else: no hold / angle-up /
//! transition / catch modes, no slide envelope, no throttle shaping, no slide-throttle
//! feathering, no confirmed-slide gate. The car's tyres, weight and torque do the rest.
//!
//! The rack runs in normalised units (`rack`, −1..=1, + = right like the driver's key); the
//! centre road-wheel angle is `−rack × lock_frac(speed) × max_steer` (+ angle = left). So the
//! time to full lock is the same at every speed, and at speed the angle is smaller
//! ([`PlainSteer::lock_frac`], the "steering speed sensitivity" curve, 75 % by default).
//!
//! The rack position is not stored: it is recovered every tick from the centre angle the
//! vehicle already keeps ([`rack_of`]), so snapshots, rewinds and resets need nothing new.
//!
//! Like [`crate::drift::set_tuning_override`] this is a process-wide **developer** override
//! for the testbed's Drift Lab W ([`set_override`]); it is not in the replay log or the
//! netcode, and the server never sets it. It is independent of the damage mode (Arcade and
//! Simulation run the same physics).

use std::sync::RwLock;

/// Plain keyboard steering constants (Drift Lab W tune panel).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PlainSteer {
    /// Time for the rack to go from centre to full lock with a key held, s (reference ≈ 0.28).
    pub t_full_s: f32,
    /// Time for the rack to return from full lock to centre after the key is released, s
    /// (reference ≈ 0.33). A reversal (one key released, the other pressed) runs at this rate
    /// back to centre, then at the [`PlainSteer::t_full_s`] rate out to the other side.
    pub t_return_s: f32,
    /// Steering speed sensitivity, 0..=1 (the reference's "Steering speed sensitivity"; the
    /// owner's setting is 0.75). 0 = full lock at every speed.
    pub speed_sensitivity: f32,
    /// Speed where the lock has dropped to `1 / (1 + speed_sensitivity)` (at 75 %: 57 %), km/h.
    pub sens_ref_kmh: f32,
    /// Exponent of the speed-sensitivity curve, 1..=4 in steps of 0.5 (2 = the lock falls with
    /// speed², like the steer angle a corner of constant lateral g needs).
    pub sens_exponent: f32,
    /// Self-align, 0..=1 (default 0): with the keys released the rack returns toward
    /// `self_align × the front axle's direction of travel` instead of the centre (a car whose
    /// wheels trail the slide). 0 = plain return to centre. An A/B switch: we have no reference
    /// data for released keys mid-drift.
    pub self_align: f32,
    /// Where the speed sensitivity acts (lead A/B, clip 8: the reference's cockpit wheel turns
    /// as far at 150 km/h as at a standstill).
    pub sens_applies: SensApplies,
    /// Velocity-relative steering (reference clip 8 "the steering wheel during a drift"): in a
    /// slide the wheels are pulled toward the front axle's direction of travel and the key
    /// adds `key × lock(speed)` on top, so the key steers relative to where the car is going.
    /// The wheel target is `align_gain × fade × front-axle travel − key × lock`, clamped to the
    /// car's full lock, where `fade` comes in with body slip from [`PlainSteer::align_from_deg`]
    /// to [`PlainSteer::align_full_deg`] (grip driving and standstill are the plain profile).
    /// `false` (default) = the plain profile only. Off by default: measured in step 7, it acts
    /// as a strong catch (swing-through) and breaks the spin model's held drift.
    pub velocity_relative: bool,
    /// Velocity-relative steering: how far the wheels follow the front axle's travel in a slide
    /// (1 = exactly along it).
    pub align_gain: f32,
    /// Body slip where the alignment starts to fade in, degrees.
    pub align_from_deg: f32,
    /// Body slip where the alignment is full, degrees.
    pub align_full_deg: f32,
    /// In a slide the wheels move at least this fast: full lock in this time, s (the
    /// reference's wheel swung from full lock to near centre in ≈ 0.4 s, clip 8).
    pub t_align_s: f32,
    /// Velocity-relative steering: the key's offset in a full slide, × lock(speed) (1 = the
    /// same offset as in grip; blended in by the slide fade). Less = a held key asks for less
    /// angle relative to the travel in a slide.
    pub slide_key_gain: f32,
}

/// Where [`PlainSteer::speed_sensitivity`] acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SensApplies {
    /// The road-wheel lock shrinks with speed ([`PlainSteer::lock_frac`]): what a held key
    /// asks for is smaller at speed.
    #[default]
    Rack,
    /// The road wheels keep the full lock at every speed; the front tyres saturate and limit
    /// the car by themselves (the curve is ignored).
    Tyre,
}

impl Default for PlainSteer {
    fn default() -> Self {
        Self {
            t_full_s: 0.28,
            t_return_s: 0.33,
            speed_sensitivity: 0.75,
            sens_ref_kmh: 113.0,
            sens_exponent: 2.0,
            self_align: 0.0,
            sens_applies: SensApplies::Rack,
            velocity_relative: false,
            align_gain: 1.0,
            align_from_deg: 2.0,
            align_full_deg: 6.0,
            t_align_s: 0.28,
            slide_key_gain: 1.0,
        }
    }
}

impl PlainSteer {
    /// Fraction of the car's full lock available at `speed` (m/s, either direction), 0..=1:
    /// `1 / (1 + s × (v / v_ref)^p)`. Defaults (75 %, 113 km/h, p = 2): 1.00 at 5 km/h, 0.83 at
    /// 60, 0.63 at 100, 0.47 at 140, 0.40 at 160 km/h.
    pub fn lock_frac(&self, speed: f32) -> f32 {
        if self.sens_applies == SensApplies::Tyre {
            return 1.0;
        }
        let s = self.speed_sensitivity.clamp(0.0, 1.0);
        if s <= 0.0 {
            return 1.0;
        }
        let v_ref = (self.sens_ref_kmh / 3.6).max(1.0);
        let x = pow_half_steps(speed.abs() / v_ref, self.sens_exponent);
        1.0 / (1.0 + s * x)
    }

    /// Rack slew away from centre, units of full rack per second.
    fn rate_out(&self) -> f32 {
        1.0 / self.t_full_s.max(0.01)
    }

    /// Rack slew toward centre, units of full rack per second.
    fn rate_back(&self) -> f32 {
        1.0 / self.t_return_s.max(0.01)
    }
}

/// `x^e` for `x >= 0` and `e` rounded to a half step in 1..=4, from multiplies and one `sqrt`
/// only (bit-identical on every target, unlike `powf`).
fn pow_half_steps(x: f32, e: f32) -> f32 {
    let halves = (e.clamp(1.0, 4.0) * 2.0).round() as u32;
    let mut r = 1.0f32;
    for _ in 0..halves / 2 {
        r *= x;
    }
    if halves % 2 == 1 {
        r *= x.sqrt();
    }
    r
}

static OVERRIDE: RwLock<Option<PlainSteer>> = RwLock::new(None);

/// Turn the plain steering profile on (`Some`) or off (`None`) for every vehicle in this
/// process. Testbed Drift Lab W and its measurements only; see the module docs.
pub fn set_override(p: Option<PlainSteer>) {
    if let Ok(mut o) = OVERRIDE.write() {
        *o = p;
    }
}

/// The plain steering profile in effect, or `None` (the normal steering model).
pub fn active() -> Option<PlainSteer> {
    OVERRIDE.read().ok().and_then(|o| *o)
}

/// Rack position (−1..=1, + = right) that gives centre angle `center` (rad, + = left) at
/// lock `lock` (rad, > 0) with damage pull `pull` (the key-equivalent offset of a bent rack).
pub fn rack_of(center: f32, lock: f32, pull: f32) -> f32 {
    if lock <= 1e-6 {
        return 0.0;
    }
    (-center / lock - pull).clamp(-1.0, 1.0)
}

/// Move the rack from `rack` toward `target` (both −1..=1) for `dt` seconds: at the return
/// rate while heading toward the centre, at the full-lock rate while heading away from it
/// (a reversal does both in one step if it crosses the centre).
pub fn slew(rack: f32, target: f32, p: &PlainSteer, dt: f32) -> f32 {
    let target = target.clamp(-1.0, 1.0);
    let mut r = rack;
    let mut t = dt;
    // At most two phases: back toward the centre, then out.
    for _ in 0..2 {
        if t <= 0.0 || r == target {
            break;
        }
        let toward_centre = r != 0.0 && (target - r).signum() != r.signum();
        if toward_centre {
            // Heading back: stop at the target if it is on this side, else at the centre.
            let stop = if target * r > 0.0 { target } else { 0.0 };
            let need = (stop - r).abs() / p.rate_back();
            if need <= t {
                r = stop;
                t -= need;
            } else {
                r += (stop - r).signum() * p.rate_back() * t;
                t = 0.0;
            }
        } else {
            let need = (target - r).abs() / p.rate_out();
            if need <= t {
                r = target;
                t = 0.0;
            } else {
                r += (target - r).signum() * p.rate_out() * t;
                t = 0.0;
            }
        }
    }
    r
}

/// Inputs of [`update_center_angle`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlainInput {
    /// Driver steer key, −1..=1 (+ = right).
    pub driver: f32,
    /// Forward speed, m/s.
    pub speed: f32,
    /// The car's full lock, rad (× the damage rack scale).
    pub full_lock: f32,
    /// Damage pull (key-equivalent offset, + = right).
    pub pull: f32,
    /// Front axle's direction of travel, rad (+ = left; for [`PlainSteer::self_align`] and
    /// velocity-relative steering).
    pub front_axle_angle: f32,
    /// Body slip at the CG, rad (velocity-relative steering's fade).
    pub body_slip: f32,
}

/// Velocity-relative steering's slide fade, 0..=1, for `body_slip` (rad) at `speed` (m/s):
/// smoothstep from [`PlainSteer::align_from_deg`] to [`PlainSteer::align_full_deg`]; 0 below
/// 3 m/s (standstill = plain lock).
pub fn align_fade(p: &PlainSteer, body_slip: f32, speed: f32) -> f32 {
    if !p.velocity_relative || speed.abs() < 3.0 {
        return 0.0;
    }
    let a = body_slip.abs().to_degrees();
    let (lo, hi) = (
        p.align_from_deg,
        p.align_full_deg.max(p.align_from_deg + 0.1),
    );
    let t = ((a - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Advance the centre road-wheel angle (rad, + = left) one tick under the plain profile.
pub fn update_center_angle(current: f32, inp: &PlainInput, p: &PlainSteer, dt: f32) -> f32 {
    if p.velocity_relative {
        return velocity_relative_angle(current, inp, p, dt);
    }
    let lock = inp.full_lock * p.lock_frac(inp.speed);
    let rack = rack_of(current, lock, inp.pull);
    let key = inp.driver.clamp(-1.0, 1.0);
    let target = if key.abs() <= crate::steering::STEER_DEADZONE
        && p.self_align > 0.0
        && inp.speed > 3.0
        && lock > 1e-6
    {
        // Released keys: return toward the travel instead of the centre.
        (-(inp.front_axle_angle * p.self_align.clamp(0.0, 1.0)) / lock).clamp(-1.0, 1.0)
    } else {
        key
    };
    let rack = slew(rack, target, p, dt);
    -(rack + inp.pull) * lock
}

/// Velocity-relative steering (see [`PlainSteer::velocity_relative`]): the angle moves toward
/// `align_gain × fade × travel − (key + pull) × lock(speed)` (clamped to full lock). It moves at
/// lock(speed) / `t_full` away from the centre and lock(speed) / `t_return` back, i.e. the plain
/// timing in grip; in a slide it moves at least full lock / `t_align`, blended by the fade.
/// With fade 0 (grip, standstill) this is the plain profile's motion in angle units.
fn velocity_relative_angle(current: f32, inp: &PlainInput, p: &PlainSteer, dt: f32) -> f32 {
    let lock = inp.full_lock * p.lock_frac(inp.speed);
    let key = inp.driver.clamp(-1.0, 1.0);
    let fade = align_fade(p, inp.body_slip, inp.speed);
    let full = inp.full_lock.max(1e-6);
    let key_gain = 1.0 + fade * (p.slide_key_gain.max(0.0) - 1.0);
    let target = (p.align_gain.max(0.0) * fade * inp.front_axle_angle
        - (key * key_gain + inp.pull) * lock)
        .clamp(-full, full);
    let out_rate = lock / p.t_full_s.max(0.01);
    let back_rate = lock / p.t_return_s.max(0.01);
    let slide_rate = full / p.t_align_s.max(0.01);
    let blend = |r: f32| r + fade * (slide_rate.max(r) - r);
    let (out_rate, back_rate) = (blend(out_rate), blend(back_rate));
    // Two phases at most: back toward the centre, then out (like [`slew`]).
    let mut a = current;
    let mut t = dt;
    for _ in 0..2 {
        if t <= 0.0 || a == target {
            break;
        }
        let toward_centre = a != 0.0 && (target - a).signum() != a.signum();
        if toward_centre {
            let stop = if target * a > 0.0 { target } else { 0.0 };
            let need = (stop - a).abs() / back_rate.max(1e-6);
            if need <= t {
                a = stop;
                t -= need;
            } else {
                a += (stop - a).signum() * back_rate * t;
                t = 0.0;
            }
        } else {
            let need = (target - a).abs() / out_rate.max(1e-6);
            if need <= t {
                a = target;
            } else {
                a += (target - a).signum() * out_rate * t;
            }
            t = 0.0;
        }
    }
    a
}

/// Readout values for the lab: `(rack, lock_frac)` for centre angle `center` at `speed`.
pub fn readout(center: f32, speed: f32, full_lock: f32, pull: f32, p: &PlainSteer) -> (f32, f32) {
    let f = p.lock_frac(speed);
    (rack_of(center, full_lock * f, pull), f)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 240.0;

    fn run(mut rack: f32, target: f32, p: &PlainSteer, ticks: usize) -> f32 {
        for _ in 0..ticks {
            rack = slew(rack, target, p, DT);
        }
        rack
    }

    #[test]
    fn full_lock_in_t_full_and_back_in_t_return() {
        let p = PlainSteer::default();
        // 0.28 s = 67.2 ticks: not there at 66, there at 68.
        assert!(run(0.0, 1.0, &p, 66) < 1.0);
        assert_eq!(run(0.0, 1.0, &p, 68), 1.0);
        // Constant rate: half lock at half the time.
        assert!((run(0.0, 1.0, &p, 34) - 0.506).abs() < 0.01);
        // Back to centre: 0.33 s = 79.2 ticks.
        assert!(run(1.0, 0.0, &p, 78) > 0.0);
        assert_eq!(run(1.0, 0.0, &p, 80), 0.0);
    }

    #[test]
    fn reversal_returns_then_goes_out() {
        let p = PlainSteer::default();
        // Full right → full left: 0.33 s back + 0.28 s out = 0.61 s = 146.4 ticks.
        assert!(run(1.0, -1.0, &p, 145) > -1.0);
        assert_eq!(run(1.0, -1.0, &p, 147), -1.0);
        // Partial return toward a same-side target runs at the return rate.
        let r = run(1.0, 0.5, &p, 24);
        assert!((r - (1.0 - 24.0 * DT / 0.33)).abs() < 1e-4);
    }

    #[test]
    fn speed_sensitivity_curve() {
        let p = PlainSteer::default();
        let f = |kmh: f32| p.lock_frac(kmh / 3.6);
        assert!(f(5.0) > 0.99);
        assert!((0.80..0.86).contains(&f(60.0)), "{}", f(60.0));
        assert!((0.35..0.45).contains(&f(160.0)), "{}", f(160.0));
        let off = PlainSteer {
            speed_sensitivity: 0.0,
            ..p
        };
        assert_eq!(off.lock_frac(50.0), 1.0);
        // More sensitivity = less lock.
        let full = PlainSteer {
            speed_sensitivity: 1.0,
            ..p
        };
        assert!(full.lock_frac(30.0) < p.lock_frac(30.0));
    }

    #[test]
    fn centre_angle_round_trips_through_the_rack() {
        let p = PlainSteer::default();
        let inp = PlainInput {
            driver: 1.0,
            speed: 20.0,
            full_lock: 0.6,
            pull: 0.0,
            front_axle_angle: 0.0,
            body_slip: 0.0,
        };
        let mut a = 0.0;
        for _ in 0..240 {
            a = update_center_angle(a, &inp, &p, DT);
        }
        // Right key = negative angle, at the speed's lock.
        let lock = 0.6 * p.lock_frac(20.0);
        assert!((a + lock).abs() < 1e-5, "{a} vs {lock}");
        // Released: back to exactly 0.
        let rel = PlainInput { driver: 0.0, ..inp };
        for _ in 0..240 {
            a = update_center_angle(a, &rel, &p, DT);
        }
        assert_eq!(a, 0.0);
    }

    #[test]
    fn tyre_mode_keeps_full_lock_at_speed() {
        let p = PlainSteer {
            sens_applies: SensApplies::Tyre,
            ..PlainSteer::default()
        };
        assert_eq!(p.lock_frac(150.0 / 3.6), 1.0);
        // Serialised lowercase for the testbed.
        let j = serde_json::to_string(&p).unwrap();
        assert!(j.contains(r#""sens_applies":"tyre""#), "{j}");
    }

    #[test]
    fn velocity_relative_follows_the_travel_in_a_slide_and_is_plain_in_grip() {
        let p = PlainSteer {
            velocity_relative: true,
            ..PlainSteer::default()
        };
        let run = |inp: &PlainInput| {
            let mut a = 0.0;
            for _ in 0..480 {
                a = update_center_angle(a, inp, &p, DT);
            }
            a
        };
        // Grip (1° of body slip): the plain target, key right = −lock.
        let grip = PlainInput {
            driver: 1.0,
            speed: 15.0,
            full_lock: 0.6,
            pull: 0.0,
            front_axle_angle: 0.3,
            body_slip: 1f32.to_radians(),
        };
        assert!((run(&grip) + 0.6 * p.lock_frac(15.0)).abs() < 1e-5);
        // Slide (20°), keys released: the wheels point along the travel.
        let slide = PlainInput {
            driver: 0.0,
            body_slip: 20f32.to_radians(),
            ..grip
        };
        assert!((run(&slide) - 0.3).abs() < 1e-5, "{}", run(&slide));
        // Slide, key into the travel side (left = −1) adds lock on top, clamped to full lock.
        let into = PlainInput {
            driver: -1.0,
            ..slide
        };
        assert!((run(&into) - 0.6).abs() < 1e-5);
        // Standstill: plain full lock.
        let still = PlainInput { speed: 0.0, ..into };
        assert!((run(&still) - 0.6).abs() < 1e-5);
    }

    #[test]
    fn self_align_returns_toward_the_travel() {
        let p = PlainSteer {
            velocity_relative: false,
            self_align: 1.0,
            ..PlainSteer::default()
        };
        let inp = PlainInput {
            driver: 0.0,
            speed: 15.0,
            full_lock: 0.6,
            pull: 0.0,
            front_axle_angle: 0.2,
            body_slip: 0.0,
        };
        let mut a = 0.0;
        for _ in 0..240 {
            a = update_center_angle(a, &inp, &p, DT);
        }
        assert!((a - 0.2).abs() < 1e-4, "{a}");
    }
}
