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
    /// Self-align, 0..=1: with the keys released the rack returns toward
    /// `self_align × the front axle's direction of travel` instead of the centre (a car whose
    /// wheels trail the slide), within [`PlainSteer::self_align_cap`]. 0 = plain return to
    /// centre. A player car's countersteer help ([`wp_contracts::AssistProfile::countersteer_help`])
    /// sets it (the larger of the two): the reference game's hands-off wheel settles at a
    /// countersteer that follows the slide (`docs/reference/wf-clips`, row 142). A held key
    /// overrides it at once.
    pub self_align: f32,
    /// Self-align cap, × the car's full lock: the hands-off wheels follow the travel at most
    /// this far (the reference's hands-off countersteer stays within about a quarter to a
    /// third of full lock even in big slides; `docs/reference/wf-clips/OWNER_KEY_TECHNIQUE.md`).
    /// 1 = no cap.
    pub self_align_cap: f32,
    /// Self-align slide fade: body slip, degrees, where the alignment is fully in (from half of
    /// it), so released keys in grip return to the centre. 0 = always (no fade).
    pub self_align_slide_deg: f32,
    /// Where the speed sensitivity acts (lead A/B, clip 8: the reference's cockpit wheel turns
    /// as far at 150 km/h as at a standstill).
    pub sens_applies: SensApplies,
    /// Velocity-relative steering (reference clip 8 "the steering wheel during a drift"): in a
    /// slide, **while a steer key is held**, the wheels are pulled toward the front axle's
    /// direction of travel and the key adds `key × lock(speed)` on top, so the key steers
    /// relative to where the car is going. With no key the wheels return to the body's centre
    /// at `t_return` (no automatic countersteer: a powered slide slowly spins, as the reference
    /// does when the keys are let go mid-drift).
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
    /// Swing rack (tyre-slip, drift transitions): while a steer key is held **against** the
    /// car's rotation, or no key is held, and the yaw rate is at least [`PlainSteer::swing_yaw_dps`] (the car
    /// swinging through from one drift to the other; a held drift turns at ~30°/s, a swing at
    /// 100–140°/s), every rack rate is this many times faster, so the new side's countersteer
    /// arrives in time to catch the swing. 1 (default) = off.
    pub swing_rate: f32,
    /// Yaw rate where the swing rack acts, °/s.
    pub swing_yaw_dps: f32,
    /// Snap-back (lead, step 10): in a slide, a held countersteer key may point the front
    /// wheels at most this far past the front axle's direction of travel, degrees, so the lock
    /// unwinds as the slide closes (what a wheel's self-aligning torque does in the driver's
    /// hands) instead of staying at full counter-lock and yanking the nose the other way.
    /// Faded in with body slip between `counter_from_deg` and `counter_full_deg` (no effect in
    /// grip). 0 = off (no limit, the step 1–9 "before"); default 0.25° (step 10: the front
    /// tyres' side force flips about 1° past the travel, peak at ~4°).
    pub counter_margin_deg: f32,
    /// Body slip, degrees, where the countersteer limit starts fading in.
    pub counter_from_deg: f32,
    /// Body slip, degrees, from which the countersteer limit is fully on.
    pub counter_full_deg: f32,
    /// Swing-rack arming window, s (tyre-slip; reference: transitions start with a brief
    /// throttle lift): the swing rack acts only within this time of a throttle lift (input
    /// < 0.5) in a slide (|body slip| > 10°) — or of a steer-key reversal with [`PlainSteer::swing_arm_on_reversal`]. 0
    /// (default) = always armed (the yaw threshold alone).
    pub swing_arm_s: f32,
    /// Old-side cap (tyre-slip, drift transitions): in a slide past
    /// [`PlainSteer::old_side_from_deg`], a held key may point the wheels at most this far past
    /// the front axle's travel **into** the turn (the side away from the travel), degrees, so
    /// the new key of a flick does not turn the fronts into the old corner (where they would
    /// pull the car back into the old drift). 0 (default) = off.
    pub old_side_cap_deg: f32,
    /// Body slip where the old-side cap starts to fade in, degrees (drift entries switch keys
    /// below it).
    pub old_side_from_deg: f32,
    /// Body slip where the old-side cap is fully on, degrees.
    pub old_side_full_deg: f32,
    /// Also arm the swing rack on a steer-key reversal (one key released, the other pressed).
    pub swing_arm_on_reversal: bool,
    /// New-side slide (the key countersteering the current slide) where the swing rack starts
    /// to ease off, degrees: the catch, then a normal rack for the hold (no over-catch).
    pub swing_ease_from_deg: f32,
    /// New-side slide where the swing rack is fully off, degrees.
    pub swing_ease_full_deg: f32,
    /// Owner step 18 ("catch over-rotations a little easier"): the countersteer limit
    /// ([`PlainSteer::counter_margin_deg`]) is directional — it holds while the slide closes or
    /// stays (the snap-back case) and lets go while the car over-rotates (the slide growing
    /// faster than `over_from_dps`, from half that), so a held counter key may go to full lock
    /// and past the travel; the drift lock extension and over-rotation rack also need it.
    /// Off = the step-10 limit at every slide rate, no extension, no fast rack (the "before").
    pub counter_closing_only: bool,
    /// Drift lock extension (owner step 18): in a slide with the counter key held, the counter
    /// lock may reach the car's full lock × (1 + this), at every speed (no speed curve),
    /// faded in with body slip between `drift_lock_from_deg` and `drift_lock_full_deg` (past
    /// a normal drift's angle) and only while the car over-rotates (slide growth from
    /// `over_from_dps`, full at twice that). Grip driving, a held drift and the key into the
    /// turn are unchanged. 0 = off. Default 0.4 (full lock × 1.4).
    pub drift_lock_extra: f32,
    /// Body slip, degrees, where the drift lock extension starts.
    pub drift_lock_from_deg: f32,
    /// Body slip, degrees, from which the drift lock extension is full.
    pub drift_lock_full_deg: f32,
    /// Over-rotation rack (owner step 18, optional): while the counter key is held, the slide
    /// grows faster than `over_from_dps` and the body slip is past `drift_lock_from_deg`, the
    /// rack moves this many times faster. 1 = off.
    pub over_rate: f32,
    /// Slide growth, °/s, where the over-rotation rack acts and the drift lock extension starts
    /// (full at twice this).
    pub over_from_dps: f32,
    /// Transition rack (lead, step 12: lift-flick transitions never swung — on the flick the
    /// new key turned the fronts into the old corner, so their side force kept the old
    /// rotation). While a key points **into the old corner** (toward the new side of a flick,
    /// or into the turn) and the car still slides on the old side, the wheels move to the front
    /// axle's travel × `trans_gain` plus the key × `trans_key_gain` × lock (up to
    /// `trans_max_deg`), at `trans_t_s` per `trans_max_deg`, faded in with the old-side body slip
    /// between `trans_from_deg` and `trans_full_deg` — what the reference's cockpit wheel does
    /// (it follows the travel while a key is held). Countersteer (the hold, the catch), released
    /// keys and grip driving are unchanged. Off = the plain rack.
    pub trans_follow: bool,
    /// Transition rack: share of the front axle's travel the wheels follow (1 = along it).
    pub trans_gain: f32,
    /// Transition rack: the key's offset from the travel, × lock(speed).
    pub trans_key_gain: f32,
    /// Transition rack: old-side body slip where it starts, degrees.
    pub trans_from_deg: f32,
    /// Transition rack: old-side body slip from which it is full, degrees.
    pub trans_full_deg: f32,
    /// Transition rack: largest road-wheel angle, degrees.
    pub trans_max_deg: f32,
    /// Transition rack: time to move `trans_max_deg`, s.
    pub trans_t_s: f32,
    /// Transition rack: armed only within this time of a throttle lift in a slide or a key
    /// reversal (the vehicle's timers, [`PlainInput::since_lift_s`] /
    /// [`PlainInput::since_reversal_s`]), s, so the key into the turn that starts or angles up
    /// a drift keeps the plain rack. 0 = always armed.
    pub trans_arm_s: f32,
    /// New-side catch (lead, step 13: settle within 0.5 s of the crossing): in the same armed
    /// within `trans_catch_window_s` of the slide crossing from one side to the other (a
    /// flick), a **countersteer** key while the slide still grows (below `trans_catch_max_deg`) (faster than `trans_catch_growth_dps`) points the wheels at
    /// `trans_catch_gain` × the front axle's travel plus this, degrees (default 0.8 × travel +
    /// 0°: aiming past it over-catches and swings back), at the
    /// transition rack's speed — the fronts then pull against the swing's rotation at once instead
    /// of lagging the travel at the plain rack rate. Faded in with body slip by
    /// `counter_from_deg` → `counter_full_deg`. Negative = off.
    pub trans_catch_deg: f32,
    /// New-side catch: share of the travel the wheels aim at (before `trans_catch_deg`).
    pub trans_catch_gain: f32,
    /// New-side catch: acts while the slide grows faster than this, °/s.
    pub trans_catch_growth_dps: f32,
    /// New-side catch: only below this body slip, degrees; above it the plain rack (and the
    /// step-11 catch) take over. Default 45° (with the crossing timer the catch can run through
    /// the new side's overshoot: rwd_b settles in 0.27 s instead of 1.6 s at 30°).
    pub trans_catch_max_deg: f32,
    /// New-side catch: acts within this time of the slide crossing from one side to the other
    /// ([`PlainInput::since_cross_s`]), s.
    pub trans_catch_window_s: f32,
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
    /// The player's keyboard steering (owner, playtest rows 141 / 142: "this behavior of full
    /// lock the opposite way to enable transitioning needs to be removed entirely. It should
    /// behave more like Wreckfest"): the wheels go where the key says, `key × lock(speed)`, at
    /// the rack rate and return to centre when it is released, like the reference game's
    /// keyboard steering (`docs/reference/wf-clips`). Nothing moves the wheels away from the key
    /// position: [`PlainSteer::lab_w`] without the transition rack and new-side catch
    /// ([`PlainSteer::trans_follow`] off), the drift lock extension, the over-rotation rack and
    /// the countersteer limit. The only limit is the speed-sensitive lock
    /// ([`PlainSteer::lock_frac`], the reference's "steering speed sensitivity"). With no key
    /// held, the countersteer help aligns the wheels with the slide, capped
    /// ([`PlainSteer::self_align`]).
    fn default() -> Self {
        Self {
            trans_follow: false,
            drift_lock_extra: 0.0,
            over_rate: 1.0,
            counter_margin_deg: 0.0,
            // Hands-off countersteer (the countersteer help slider sets `self_align`): within
            // 30 % of full lock, only once the car slides (row 142, the reference clips).
            self_align_cap: 0.3,
            self_align_slide_deg: 8.0,
            ..Self::lab_w()
        }
    }
}

impl PlainSteer {
    /// Drift Lab W's full step-13b set (lab A/B only, `?tune=1`; never a player default since
    /// row 142): [`PlainSteer::default`] plus the transition rack, the new-side catch, the
    /// after-flick follow, the drift lock extension, the over-rotation rack and the
    /// countersteer limit.
    pub fn lab_w() -> Self {
        Self {
            t_full_s: 0.28,
            t_return_s: 0.33,
            speed_sensitivity: 0.75,
            sens_ref_kmh: 113.0,
            sens_exponent: 2.0,
            self_align: 0.0,
            self_align_cap: 1.0,
            self_align_slide_deg: 0.0,
            sens_applies: SensApplies::Rack,
            velocity_relative: false,
            align_gain: 1.0,
            align_from_deg: 2.0,
            align_full_deg: 6.0,
            t_align_s: 0.28,
            slide_key_gain: 1.0,
            swing_rate: 1.0,
            swing_yaw_dps: 60.0,
            counter_margin_deg: 0.25,
            counter_from_deg: 3.0,
            counter_full_deg: 8.0,
            swing_arm_s: 0.0,
            old_side_cap_deg: 0.0,
            old_side_from_deg: 20.0,
            old_side_full_deg: 25.0,
            swing_arm_on_reversal: false,
            swing_ease_from_deg: 90.0,
            swing_ease_full_deg: 90.0,
            counter_closing_only: true,
            drift_lock_extra: 0.4,
            drift_lock_from_deg: 40.0,
            drift_lock_full_deg: 50.0,
            over_rate: 2.0,
            over_from_dps: 15.0,
            trans_follow: true,
            trans_gain: 1.0,
            trans_key_gain: 0.0,
            trans_from_deg: 8.0,
            trans_full_deg: 15.0,
            trans_max_deg: 45.0,
            trans_t_s: 0.1,
            trans_arm_s: 1.5,
            trans_catch_deg: 0.0,
            // 0.9 until playtest row 137: it over-caught rampart_50 into a second swing
            // (tests/transition_once.rs). 0.8, not lower: 0.7 still caught only 19 / 27 of the
            // transition bench's flicks (0.8: 27 / 27, 0.9: 27 / 27).
            trans_catch_gain: 0.8,
            trans_catch_growth_dps: 7.5,
            trans_catch_max_deg: 45.0,
            trans_catch_window_s: 1.0,
        }
    }

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
    /// Yaw rate, rad/s (+ = left; the swing rack, [`PlainSteer::swing_rate`]).
    pub yaw_rate: f32,
    /// How fast the slide angle grows, rad/s (+ = opening / over-rotating, − = closing): the
    /// directional countersteer limit and the over-rotation rack (owner step 18).
    pub slide_growth: f32,
    /// Seconds since the throttle input was last below 0.5 in a slide (swing-rack arming).
    pub since_lift_s: f32,
    /// Seconds since the steer key last reversed (swing-rack arming).
    pub since_reversal_s: f32,
    /// Time since the body slip crossed from one side (≥ 10°) to the other, s (the vehicle's
    /// timer; the new-side catch, [`PlainSteer::trans_catch_gain`]).
    pub since_cross_s: f32,
}

/// `p` with the swing rack applied for `inp` (see [`PlainSteer::swing_rate`]): all rack
/// times ÷ `swing_rate` while the held key opposes a yaw rate past `swing_yaw_dps`.
fn with_swing(p: &PlainSteer, inp: &PlainInput) -> PlainSteer {
    let k = p.swing_rate;
    let key = inp.driver.clamp(-1.0, 1.0);
    // Key + = right steers toward −yaw: it opposes the rotation when key × yaw rate > 0.
    // Released keys (the gap of a key reversal) count too: the rack heads for the centre.
    let held = key.abs() > crate::steering::STEER_DEADZONE;
    let armed = p.swing_arm_s <= 0.0
        || inp.since_lift_s <= p.swing_arm_s
        || (p.swing_arm_on_reversal && inp.since_reversal_s <= p.swing_arm_s);
    let swinging = k > 1.0
        && armed
        && (!held || key * inp.yaw_rate > 0.0)
        && inp.yaw_rate.abs().to_degrees() >= p.swing_yaw_dps;
    if !swinging {
        return *p;
    }
    // Ease off on the new side: the key countersteers the slide when key × body slip < 0
    // (key + = right; body slip + = moving left of the heading).
    let new_side = if key * inp.body_slip < 0.0 {
        inp.body_slip.abs().to_degrees()
    } else {
        0.0
    };
    let (lo, hi) = (
        p.swing_ease_from_deg,
        p.swing_ease_full_deg.max(p.swing_ease_from_deg + 0.1),
    );
    let t = ((new_side - lo) / (hi - lo)).clamp(0.0, 1.0);
    let ease = 1.0 - t * t * (3.0 - 2.0 * t);
    let k = 1.0 + (k.min(10.0) - 1.0) * ease;
    PlainSteer {
        t_full_s: p.t_full_s / k,
        t_return_s: p.t_return_s / k,
        t_align_s: p.t_align_s / k,
        ..*p
    }
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
    let p = &with_swing(p, inp);
    if p.velocity_relative {
        return velocity_relative_angle(current, inp, p, dt);
    }
    if let Some(a) = transition_angle(current, inp, p, dt) {
        return a;
    }
    let p = &with_over_rate(p, inp);
    let lock = drift_lock(inp.full_lock * p.lock_frac(inp.speed), inp, p);
    // Past this tick's lock right after a flick (the transition rack / new-side catch just let
    // go): back inside it at the return rate, so the rack never jumps. Only then: a lock that
    // shrinks for other reasons (grip-balance's held-key cut — the vehicle rate-limits that —
    // or the step-11 extension ending) keeps the plain rack's behaviour; holding the
    // countersteer longer there spun held drifts (grip-balance repro).
    let after_flick = p.trans_follow
        && p.trans_arm_s > 0.0
        && inp.since_lift_s <= p.trans_arm_s + 0.5
        && inp.since_reversal_s <= p.trans_arm_s + 0.5;
    if after_flick && current.abs() > lock + 1e-6 {
        let rate = inp.full_lock.max(lock) / p.t_return_s.max(0.01);
        let a = current - current.signum() * rate * dt;
        return if a.abs() < lock {
            current.signum() * lock
        } else {
            a
        };
    }
    let rack = rack_of(current, lock, inp.pull);
    let key = inp.driver.clamp(-1.0, 1.0);
    let target = if key.abs() <= crate::steering::STEER_DEADZONE
        && p.self_align > 0.0
        && inp.speed > 3.0
        && lock > 1e-6
    {
        // Released keys: return toward the travel instead of the centre, faded in with the
        // slide and capped.
        let fade = if p.self_align_slide_deg > 0.0 {
            smooth(
                inp.body_slip.abs().to_degrees(),
                0.5 * p.self_align_slide_deg,
                p.self_align_slide_deg,
            )
        } else {
            1.0
        };
        let cap = p.self_align_cap.clamp(0.0, 1.0) * inp.full_lock;
        let aim = (inp.front_axle_angle * p.self_align.clamp(0.0, 1.0) * fade).clamp(-cap, cap);
        (-aim / lock).clamp(-1.0, 1.0)
    } else {
        key
    };
    let target = counter_limited(target, inp, p, lock);
    let target = old_side_limited(target, inp, p, lock);
    let rack = slew(rack, target, p, dt);
    -(rack + inp.pull) * lock
}

/// [`PlainSteer::counter_margin_deg`]: the rack target (−1..=1, + = right) limited so a held
/// countersteer key points the wheels at most the margin past the front axle's travel in a
/// slide. Unchanged when the limit is off, in grip, below 3 m/s or with the key released.
/// Smoothstep of `x` between `lo` and `hi`, 0..=1.
fn smooth(x: f32, lo: f32, hi: f32) -> f32 {
    let t = ((x - lo) / (hi - lo).max(1e-3)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Whether the held key countersteers the slide (key + = right; body slip + = moving left).
fn countering(inp: &PlainInput) -> bool {
    inp.driver.abs() > crate::steering::STEER_DEADZONE && inp.driver * inp.body_slip < 0.0
}

/// [`PlainSteer::drift_lock_extra`]: the lock for this tick, extended toward full lock ×
/// (1 + extra) in a slide with the counter key held; `lock` otherwise.
fn drift_lock(lock: f32, inp: &PlainInput, p: &PlainSteer) -> f32 {
    if p.drift_lock_extra <= 0.0
        || !p.counter_closing_only
        || inp.speed.abs() < 3.0
        || !countering(inp)
    {
        return lock;
    }
    let mut fade = smooth(
        inp.body_slip.abs().to_degrees(),
        p.drift_lock_from_deg,
        p.drift_lock_full_deg,
    );
    // Only while the car over-rotates (the slide opening): on the way back the extra lock
    // would over-catch.
    // Only while the car over-rotates (the slide opening faster than `over_from_dps`, full
    // at twice that): a held drift or the way back keeps the plain lock (it would over-catch
    // and make the snap-back worse).
    let from = p.over_from_dps.max(0.1);
    fade *= smooth(inp.slide_growth.to_degrees(), from, 2.0 * from);
    let ext = inp.full_lock * (1.0 + p.drift_lock_extra.min(1.0));
    lock + fade * (ext - lock).max(0.0)
}

/// [`PlainSteer::trans_follow`]: the centre angle for this tick while a key points into the
/// old corner in a slide, or `None` (the plain rack).
fn transition_angle(current: f32, inp: &PlainInput, p: &PlainSteer, dt: f32) -> Option<f32> {
    let key = inp.driver.clamp(-1.0, 1.0);
    if !p.trans_follow
        || inp.speed.abs() < 3.0
        || key.abs() <= crate::steering::STEER_DEADZONE
        || (p.trans_arm_s > 0.0
            && inp.since_lift_s > p.trans_arm_s
            && inp.since_reversal_s > p.trans_arm_s)
    {
        return None;
    }
    if key * inp.body_slip < 0.0 {
        return new_side_catch(current, inp, p, dt);
    }
    if key * inp.body_slip == 0.0 {
        return None;
    }
    let fade = smooth(
        inp.body_slip.abs().to_degrees(),
        p.trans_from_deg,
        p.trans_full_deg,
    );
    if fade <= 0.0 {
        return None;
    }
    let lock = inp.full_lock * p.lock_frac(inp.speed);
    let max = p.trans_max_deg.max(1.0).to_radians();
    let plain_w = -(key + inp.pull) * lock;
    let follow_w = (p.trans_gain * inp.front_axle_angle
        - (key + inp.pull) * p.trans_key_gain * lock)
        .clamp(-max, max);
    let target = plain_w + fade * (follow_w - plain_w);
    let plain_rate = lock / p.t_full_s.max(0.01);
    let fast = max / p.trans_t_s.max(0.01);
    let rate = plain_rate + fade * (fast - plain_rate).max(0.0);
    let step = rate * dt;
    Some(if (target - current).abs() <= step {
        target
    } else {
        current + (target - current).signum() * step
    })
}

/// [`PlainSteer::trans_catch_deg`]: a countersteer key in the armed window while the slide
/// grows: the wheels just past the travel, at the transition rack's speed.
fn new_side_catch(current: f32, inp: &PlainInput, p: &PlainSteer, dt: f32) -> Option<f32> {
    // Armed only just after the slide came through zero (the vehicle's crossing timer): the
    // lift + key-reversal window also fires inside a tapped drift and the snap-back exit.
    if p.trans_catch_deg < 0.0
        || inp.slide_growth.to_degrees() < p.trans_catch_growth_dps
        || inp.body_slip.abs().to_degrees() > p.trans_catch_max_deg
        || inp.since_cross_s > p.trans_catch_window_s
    {
        return None;
    }
    let fade = smooth(
        inp.body_slip.abs().to_degrees(),
        p.counter_from_deg,
        p.counter_full_deg,
    );
    if fade <= 0.0 {
        return None;
    }
    let lock = inp.full_lock * p.lock_frac(inp.speed);
    let max = p.trans_max_deg.max(1.0).to_radians();
    let travel = inp.front_axle_angle;
    let plain_w = -(inp.driver.clamp(-1.0, 1.0) + inp.pull) * lock;
    let catch_w = (p.trans_catch_gain * travel + travel.signum() * p.trans_catch_deg.to_radians())
        .clamp(-max, max);
    let target = plain_w + fade * (catch_w - plain_w);
    let rate = lock / p.t_full_s.max(0.01)
        + fade * (max / p.trans_t_s.max(0.01) - lock / p.t_full_s.max(0.01)).max(0.0);
    let step = rate * dt;
    Some(if (target - current).abs() <= step {
        target
    } else {
        current + (target - current).signum() * step
    })
}

/// [`PlainSteer::over_rate`]: faster rack times while the counter key is held and the slide
/// grows past `over_from_dps`.
fn with_over_rate(p: &PlainSteer, inp: &PlainInput) -> PlainSteer {
    if p.over_rate <= 1.0
        || !p.counter_closing_only
        || !countering(inp)
        || inp.slide_growth.to_degrees() < p.over_from_dps
        || inp.body_slip.abs().to_degrees() < p.drift_lock_from_deg
    {
        return *p;
    }
    let k = p.over_rate.min(10.0);
    PlainSteer {
        t_full_s: p.t_full_s / k,
        t_return_s: p.t_return_s / k,
        ..*p
    }
}

fn counter_limited(target: f32, inp: &PlainInput, p: &PlainSteer, lock: f32) -> f32 {
    if p.counter_margin_deg <= 0.0
        || inp.speed.abs() < 3.0
        || inp.driver.abs() <= crate::steering::STEER_DEADZONE
        || lock <= 1e-6
    {
        return target;
    }
    let a = inp.body_slip.abs().to_degrees();
    let (lo, hi) = (
        p.counter_from_deg,
        p.counter_full_deg.max(p.counter_from_deg + 0.1),
    );
    let t = ((a - lo) / (hi - lo)).clamp(0.0, 1.0);
    let mut fade = t * t * (3.0 - 2.0 * t);
    // Step 18: directional — off while the car over-rotates (the key may go to full lock).
    if p.counter_closing_only {
        let from = p.over_from_dps.max(0.1);
        fade *= 1.0 - smooth(inp.slide_growth.to_degrees(), 0.5 * from, from);
    }
    if fade <= 0.0 {
        return target;
    }
    let travel = inp.front_axle_angle;
    let wheel = -(target + inp.pull) * lock;
    // Countersteer = the wheels on the travel's side; only that side is limited.
    if wheel * travel <= 0.0 {
        return target;
    }
    // Faded: at fade 0 the limit sits a full lock further out (no effect).
    let limit = travel.abs() + p.counter_margin_deg.to_radians() + (1.0 - fade) * 2.0 * lock;
    if wheel.abs() <= limit {
        return target;
    }
    let w = travel.signum() * limit;
    (-w / lock - inp.pull).clamp(-1.0, 1.0)
}

/// [`PlainSteer::old_side_cap_deg`]: the rack target limited so a held key points the wheels at
/// most the cap past the front axle's travel on the side away from it (into the turn) in a
/// slide. Unchanged when off, in grip, below 3 m/s or with the key released.
fn old_side_limited(target: f32, inp: &PlainInput, p: &PlainSteer, lock: f32) -> f32 {
    if p.old_side_cap_deg <= 0.0
        || inp.speed.abs() < 3.0
        || inp.driver.abs() <= crate::steering::STEER_DEADZONE
        || lock <= 1e-6
    {
        return target;
    }
    let fade = smooth(
        inp.body_slip.abs().to_degrees(),
        p.old_side_from_deg,
        p.old_side_full_deg.max(p.old_side_from_deg + 0.1),
    );
    if fade <= 0.0 {
        return target;
    }
    let travel = inp.front_axle_angle;
    if travel.abs() < 1e-4 {
        return target;
    }
    let away = -travel.signum();
    let wheel = -(target + inp.pull) * lock;
    // How far the wheels point past the travel on the side away from it.
    let past = (wheel - travel) * away;
    // Faded: at fade 0 the cap sits two full locks further (no effect).
    let cap = p.old_side_cap_deg.to_radians() + (1.0 - fade) * 2.0 * lock;
    if past <= cap {
        return target;
    }
    let w = travel + away * cap;
    (-w / lock - inp.pull).clamp(-1.0, 1.0)
}

/// Velocity-relative steering (see [`PlainSteer::velocity_relative`]): the angle moves toward
/// `align_gain × fade × travel − (key + pull) × lock(speed)` (clamped to full lock). It moves at
/// lock(speed) / `t_full` away from the centre and lock(speed) / `t_return` back, i.e. the plain
/// timing in grip; in a slide it moves at least full lock / `t_align`, blended by the fade.
/// With fade 0 (grip, standstill) this is the plain profile's motion in angle units.
fn velocity_relative_angle(current: f32, inp: &PlainInput, p: &PlainSteer, dt: f32) -> f32 {
    let lock = inp.full_lock * p.lock_frac(inp.speed);
    let key = inp.driver.clamp(-1.0, 1.0);
    // Owner / clip 1 ("letting go of the steering mid-drift"): the pull toward the travel acts
    // only while a steer key is held; released keys return the wheels to the body's centre at
    // the plain return rate, with no automatic countersteer (a powered slide slowly spins).
    let key_held = key.abs() > crate::steering::STEER_DEADZONE;
    let fade = if key_held {
        align_fade(p, inp.body_slip, inp.speed)
    } else {
        0.0
    };
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
        let p = PlainSteer::lab_w();
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
        let p = PlainSteer::lab_w();
        // Full right → full left: 0.33 s back + 0.28 s out = 0.61 s = 146.4 ticks.
        assert!(run(1.0, -1.0, &p, 145) > -1.0);
        assert_eq!(run(1.0, -1.0, &p, 147), -1.0);
        // Partial return toward a same-side target runs at the return rate.
        let r = run(1.0, 0.5, &p, 24);
        assert!((r - (1.0 - 24.0 * DT / 0.33)).abs() < 1e-4);
    }

    #[test]
    fn speed_sensitivity_curve() {
        let p = PlainSteer::lab_w();
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
        let p = PlainSteer::lab_w();
        let inp = PlainInput {
            driver: 1.0,
            speed: 20.0,
            full_lock: 0.6,
            pull: 0.0,
            front_axle_angle: 0.0,
            body_slip: 0.0,
            yaw_rate: 0.0,
            slide_growth: 0.0,
            since_lift_s: 1.0e3,
            since_reversal_s: 1.0e3,
            since_cross_s: 1.0e3,
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
            ..PlainSteer::lab_w()
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
            ..PlainSteer::lab_w()
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
            yaw_rate: 0.0,
            slide_growth: 0.0,
            since_lift_s: 1.0e3,
            since_reversal_s: 1.0e3,
            since_cross_s: 1.0e3,
        };
        assert!((run(&grip) + 0.6 * p.lock_frac(15.0)).abs() < 1e-5);
        // Slide (20°), keys released: the wheels go back to the body's centre (no automatic
        // countersteer).
        let slide = PlainInput {
            driver: 0.0,
            body_slip: 20f32.to_radians(),
            ..grip
        };
        assert_eq!(run(&slide), 0.0);
        // Slide, a key held with its offset at 0: the wheels follow the travel.
        let along = PlainSteer {
            slide_key_gain: 0.0,
            ..p
        };
        let held = PlainInput {
            driver: 1.0,
            ..slide
        };
        let mut a = 0.0;
        for _ in 0..480 {
            a = update_center_angle(a, &held, &along, DT);
        }
        assert!((a - 0.3).abs() < 1e-5, "{a}");
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
    fn player_hands_off_follows_the_slide_up_to_the_cap_and_a_key_overrides_it() {
        // Row 142: the player default with countersteer help 0.3 (the vehicle sets
        // `self_align` from it). Full lock 0.6 rad, cap 30 % = 0.18 rad.
        let p = PlainSteer {
            self_align: 0.3,
            ..PlainSteer::default()
        };
        let slide = PlainInput {
            driver: 0.0,
            speed: 15.0,
            full_lock: 0.6,
            pull: 0.0,
            front_axle_angle: 0.4,
            body_slip: 0.4,
            yaw_rate: 0.0,
            slide_growth: 0.0,
            since_lift_s: 1.0e3,
            since_reversal_s: 1.0e3,
            since_cross_s: 1.0e3,
        };
        let run = |inp: &PlainInput, a0: f32, n: usize| {
            let mut a = a0;
            for _ in 0..n {
                a = update_center_angle(a, inp, &p, DT);
            }
            a
        };
        // In a slide: 0.3 × the travel (0.12 rad), within the cap.
        assert!((run(&slide, 0.0, 240) - 0.12).abs() < 1e-4);
        // A big slide: capped at 30 % of full lock.
        let big = PlainInput {
            front_axle_angle: 1.0,
            ..slide
        };
        assert!((run(&big, 0.0, 240) - 0.18).abs() < 1e-4);
        // Grip (no body slip): back to the centre.
        let grip = PlainInput {
            body_slip: 0.0,
            ..slide
        };
        assert!(run(&grip, 0.12, 240).abs() < 1e-4);
        // A key overrides it at once: the first tick already heads for the key's side.
        let key = PlainInput {
            driver: 1.0,
            ..slide
        };
        let a1 = run(&key, 0.12, 1);
        assert!(a1 < 0.12, "{a1}");
        assert!(run(&key, 0.12, 240) < -0.5, "the key's full lock");
    }

    #[test]
    fn self_align_returns_toward_the_travel() {
        let p = PlainSteer {
            velocity_relative: false,
            self_align: 1.0,
            ..PlainSteer::lab_w()
        };
        let inp = PlainInput {
            driver: 0.0,
            speed: 15.0,
            full_lock: 0.6,
            pull: 0.0,
            front_axle_angle: 0.2,
            body_slip: 0.0,
            yaw_rate: 0.0,
            slide_growth: 0.0,
            since_lift_s: 1.0e3,
            since_reversal_s: 1.0e3,
            since_cross_s: 1.0e3,
        };
        let mut a = 0.0;
        for _ in 0..240 {
            a = update_center_angle(a, &inp, &p, DT);
        }
        assert!((a - 0.2).abs() < 1e-4, "{a}");
    }

    /// Swing rack: off by default (bit-identical), faster only while swinging with the key
    /// against the rotation (or released), never for a held drift's slow rotation.
    #[test]
    fn swing_rack_only_speeds_up_a_swing() {
        // Rack at full right (angle −0.6), the left key pressed (driver −1 → angle +).
        let base = PlainInput {
            driver: -1.0,
            speed: 20.0,
            full_lock: 0.6,
            pull: 0.0,
            front_axle_angle: 0.0,
            body_slip: 0.0,
            yaw_rate: -120f32.to_radians(), // swinging right; the left key opposes it
            slide_growth: 0.0,
            since_lift_s: 1.0e3,
            since_reversal_s: 1.0e3,
            since_cross_s: 1.0e3,
        };
        let step = |p: &PlainSteer, inp: &PlainInput| {
            let mut a = -0.6;
            for _ in 0..24 {
                a = update_center_angle(a, inp, p, DT);
            }
            a
        };
        let off = PlainSteer::lab_w();
        let on = PlainSteer {
            swing_rate: 2.5,
            swing_yaw_dps: 80.0,
            ..off
        };
        let plain = PlainSteer {
            swing_rate: 1.0,
            ..on
        };
        assert_eq!(step(&off, &base), step(&plain, &base));
        // Swinging: further toward the new side in 0.1 s.
        assert!(step(&on, &base) > step(&off, &base) + 0.1);
        // A held drift's ~30°/s rotation, or a key with the rotation: unchanged.
        let slow = PlainInput {
            yaw_rate: -30f32.to_radians(),
            slide_growth: 0.0,
            since_lift_s: 1.0e3,
            since_reversal_s: 1.0e3,
            since_cross_s: 1.0e3,
            ..base
        };
        assert_eq!(step(&on, &slow), step(&off, &slow));
        let with = PlainInput {
            yaw_rate: 120f32.to_radians(),
            slide_growth: 0.0,
            since_lift_s: 1.0e3,
            since_reversal_s: 1.0e3,
            since_cross_s: 1.0e3,
            ..base
        };
        assert_eq!(step(&on, &with), step(&off, &with));
    }

    #[test]
    fn countersteer_limit_follows_the_travel_in_a_slide_only() {
        let p = PlainSteer {
            drift_lock_extra: 0.0,
            ..PlainSteer::lab_w()
        };
        let off = PlainSteer {
            counter_margin_deg: 0.0,
            ..p
        };
        // Left-hand slide: the front axle travels 20° right of the heading (−); the right key
        // (countersteer) would go to full lock (−0.6 rad × lock fraction).
        let slide = PlainInput {
            driver: 1.0,
            speed: 12.0,
            full_lock: 0.6,
            pull: 0.0,
            front_axle_angle: -20f32.to_radians(),
            body_slip: -30f32.to_radians(),
            yaw_rate: 0.0,
            slide_growth: -0.5, // closing (the snap-back case)
            since_lift_s: 1.0e3,
            since_reversal_s: 1.0e3,
            since_cross_s: 1.0e3,
        };
        let run = |inp: &PlainInput, p: &PlainSteer| {
            let mut a = 0.0;
            for _ in 0..480 {
                a = update_center_angle(a, inp, p, DT);
            }
            a
        };
        let limit = -(20.0 + p.counter_margin_deg).to_radians();
        assert!(
            (run(&slide, &p) - limit).abs() < 1e-4,
            "{}",
            run(&slide, &p)
        );
        let lock = 0.6 * p.lock_frac(12.0);
        assert!((run(&slide, &off) + lock).abs() < 1e-5);
        // In grip (body slip 1°), into the turn, or released: no limit.
        let grip = PlainInput {
            body_slip: -1f32.to_radians(),
            ..slide
        };
        assert!((run(&grip, &p) + lock).abs() < 1e-5);
        let into = PlainInput {
            driver: -1.0,
            ..slide
        };
        assert!((run(&into, &p) - lock).abs() < 1e-5);
        let rel = PlainInput {
            driver: 0.0,
            ..slide
        };
        assert_eq!(run(&rel, &p), 0.0);
    }

    #[test]
    fn over_rotation_gets_full_lock_and_the_drift_lock_extension() {
        let p = PlainSteer::lab_w();
        // Left-hand slide at 55° of body slip (past a normal drift), front axle travelling 20°
        // right, counter key.
        let slide = PlainInput {
            driver: 1.0,
            speed: 12.0,
            full_lock: 0.6,
            pull: 0.0,
            front_axle_angle: -20f32.to_radians(),
            body_slip: -55f32.to_radians(),
            yaw_rate: 0.0,
            slide_growth: 1.0, // over-rotating (57°/s)
            since_lift_s: 1.0e3,
            since_reversal_s: 1.0e3,
            since_cross_s: 1.0e3,
        };
        let lock_at = |p: &PlainSteer| 0.6 * p.lock_frac(12.0);
        let run = |inp: &PlainInput, p: &PlainSteer| {
            let mut a = 0.0;
            for _ in 0..480 {
                a = update_center_angle(a, inp, p, DT);
            }
            a
        };
        // Over-rotating: no countersteer limit; past 50° of slip the lock is full × 1.4.
        let ext = -0.6 * (1.0 + p.drift_lock_extra);
        assert!((run(&slide, &p) - ext).abs() < 1e-4, "{}", run(&slide, &p));
        // Closing: limited to the travel + margin again.
        let closing = PlainInput {
            slide_growth: -0.5,
            ..slide
        };
        let limit = -(20.0 + p.counter_margin_deg).to_radians();
        assert!((run(&closing, &p) - limit).abs() < 1e-4);
        // The "before" (limit at every rate, no extension): limited while over-rotating too.
        let before = PlainSteer {
            counter_closing_only: false,
            drift_lock_extra: 0.0,
            ..p
        };
        assert!((run(&slide, &before) - limit).abs() < 1e-4);
        // A normal drift angle (30°), grip (1°) and the key into the turn: no extension.
        let drift = PlainInput {
            body_slip: -30f32.to_radians(),
            ..slide
        };
        assert!((run(&drift, &p) + lock_at(&p)).abs() < 1e-5);
        let lock = lock_at(&p);
        let grip = PlainInput {
            body_slip: -1f32.to_radians(),
            ..slide
        };
        assert!((run(&grip, &p) + lock).abs() < 1e-5);
        let into = PlainInput {
            driver: -1.0,
            ..slide
        };
        assert!((run(&into, &p) - lock).abs() < 1e-5);
        // Over-rotation rack (default × 2): faster toward the counter lock in the first 0.1 s.
        let fast = p;
        let p = PlainSteer {
            over_rate: 1.0,
            ..p
        };
        let early = |p: &PlainSteer| {
            let mut a = 0.0;
            for _ in 0..24 {
                a = update_center_angle(a, &slide, p, DT);
            }
            a
        };
        assert!(early(&fast) < early(&p) - 0.05);
    }

    #[test]
    fn transition_rack_follows_the_travel_only_when_armed_and_into_the_old_corner() {
        let p = PlainSteer::lab_w();
        // Left drift at 35° (travel 30° right), the new side's key (left, −1) just after a lift.
        let flick = PlainInput {
            driver: -1.0,
            speed: 12.0,
            full_lock: 0.6,
            pull: 0.0,
            front_axle_angle: -30f32.to_radians(),
            body_slip: -35f32.to_radians(),
            yaw_rate: 0.0,
            slide_growth: 0.0,
            since_lift_s: 0.1,
            since_reversal_s: 1.0e3,
            since_cross_s: 1.0e3,
        };
        let run = |inp: &PlainInput, p: &PlainSteer| {
            let mut a = 0.0;
            for _ in 0..240 {
                a = update_center_angle(a, inp, p, DT);
            }
            a
        };
        // Along the travel (key offset 0 by default), not full left lock into the old corner.
        assert!((run(&flick, &p) - flick.front_axle_angle).abs() < 1e-4);
        let lock = 0.6 * p.lock_frac(12.0);
        // Not armed (no lift / reversal for 2 s): the plain rack (the key that angles up).
        let unarmed = PlainInput {
            since_lift_s: 2.0,
            ..flick
        };
        assert!((run(&unarmed, &p) - lock).abs() < 1e-5);
        // Off: the plain rack.
        let off = PlainSteer {
            trans_follow: false,
            ..p
        };
        assert!((run(&flick, &off) - lock).abs() < 1e-5);
        // Grip (slip 3°): the plain rack.
        let grip = PlainInput {
            body_slip: -3f32.to_radians(),
            ..flick
        };
        assert!((run(&grip, &p) - lock).abs() < 1e-5);
        // Leaving the transition past the plain lock just after a flick: back inside it
        // without a jump.
        let mut a = -0.7f32;
        let rel = PlainInput {
            driver: 0.0,
            since_reversal_s: 0.2,
            since_cross_s: 1.0e3,
            ..flick
        };
        let next = update_center_angle(a, &rel, &p, DT);
        assert!(next < -0.69 && next > a);
        // No flick (the lock shrank for another reason): the plain rack, as before step 12.
        let plain_next = update_center_angle(
            a,
            &PlainInput {
                driver: 0.0,
                ..unarmed
            },
            &p,
            DT,
        );
        assert!(plain_next > -lock - 1e-6);
        for _ in 0..240 {
            a = update_center_angle(a, &rel, &p, DT);
        }
        assert_eq!(a, 0.0);
    }

    #[test]
    fn new_side_catch_aims_at_the_travel_only_after_a_flick() {
        let p = PlainSteer::lab_w();
        // Just through zero onto the new side (tail left, slip +12°, growing), travel 15° left,
        // the new countersteer key (left, −1), within the window of a lift and a key reversal.
        let swing = PlainInput {
            driver: -1.0,
            speed: 18.0,
            full_lock: 0.6,
            pull: 0.0,
            front_axle_angle: 15f32.to_radians(),
            body_slip: 12f32.to_radians(),
            yaw_rate: -1.5,
            slide_growth: 1.0,
            since_lift_s: 0.5,
            since_reversal_s: 0.4,
            since_cross_s: 0.1,
        };
        let step = |inp: &PlainInput, p: &PlainSteer, n: usize| {
            let mut a = 0.0;
            for _ in 0..n {
                a = update_center_angle(a, inp, p, DT);
            }
            a
        };
        let aim = p.trans_catch_gain * swing.front_axle_angle;
        assert!((step(&swing, &p, 120) - aim).abs() < 1e-4);
        // Fast: most of the way within 0.05 s.
        assert!(step(&swing, &p, 12) > 0.8 * aim);
        let off = PlainSteer {
            trans_catch_deg: -1.0,
            ..p
        };
        assert!(step(&swing, &off, 12) < 0.5 * aim);
        // Not just after a crossing (a drift entry, a tapped drift, the snap-back exit — even
        // with a lift and a key reversal), a grown drift (50°), or the slide closing: the plain
        // rack.
        for inp in [
            PlainInput {
                since_cross_s: 5.0,
                ..swing
            },
            PlainInput {
                body_slip: 50f32.to_radians(),
                ..swing
            },
            PlainInput {
                slide_growth: -0.5,
                ..swing
            },
        ] {
            assert_eq!(step(&inp, &p, 12), step(&inp, &off, 12));
        }
    }
}
