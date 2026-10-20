//! Drift Lab B / C (owner rows 90 / 91 / 104 / 106): fast countersteer and the semi-intelligent
//! keyboard drift assist. Steering angle (and opt-in throttle shaping) only: nothing here adds
//! grip or a yaw torque.
//!
//! - [`DriftTuning`]: the constants, with a process-wide runtime override for the testbed's
//!   "tune" panel ([`set_tuning_override`]). The override is a developer tool: it is not part of
//!   the replay log or the netcode, and the server never sets it.
//! - [`DriftState`]: per-vehicle state; [`DriftState::update`] reads the driver's intent each
//!   tick and returns a [`DriftCommand`] for the steering and the throttle.
//!
//! Intent modes (Drift Lab C, row 106; keyboard full strength, gamepad half, the wheel never —
//! see [`device_weight`]):
//! - steering released → **Straighten**: the wheels align with the front axle's travel, a
//!   little past it while the slide still grows ([`DriftTuning::straighten_gain`]);
//! - countersteer key (against the slide) → **Hold**: the assist supplies the opposite lock
//!   that keeps the slide angle captured when the key went down ([`DriftTuning::hold_gain`] /
//!   `hold_damp` / `hold_base`) — never the key's full lock, so a held binary key neither
//!   snaps the car straight nor over-corrects. Holding it longer slowly shrinks the held
//!   angle ([`DriftTuning::hold_decay_dps`]); taps nudge it;
//! - key into the turn → **AngleUp**: the held angle grows ([`DriftTuning::angle_up_dps`])
//!   and the front tyres get more slip into the turn: more angle, a tighter line;
//! - the throttle grows the held angle, a lift shrinks it (both modes);
//! - past the slide-angle envelope ([`base_envelope`], widening after a stable hold,
//!   narrowing with erratic input), or growing faster than it can cover → **Exceeded**: the
//!   assist (step 1's fast countersteer included) backs off and the car can spin.
//!
//! Throttle shaping is **opt-in** ([`DriftTuning::throttle_shape`] defaults to 0: the driver
//! gets exactly the throttle they press, owner row 104). When turned on it is meant to be
//! invisible: the key passes through (full key = full
//! throttle, the revs climb), the rise is rate-limited in a slide
//! ([`DriftTuning::throttle_rise_s`]) and the throttle is trimmed only as the slide nears the
//! envelope ([`DriftTuning::trim_margin_deg`]); abrupt lifts are softened
//! ([`DriftTuning::lift_soften`]), not cancelled. All of it fades out past the envelope.
//!
//! How the steering uses it: [`crate::steering::slide_onset`] turns rear-axle slip and its
//! growth rate into a 0..=1 onset; from the onset the countersteer lock opens, released keys
//! align the wheels with the front axle's travel, and any rack movement toward the
//! countersteer side runs at the fast absolute rate ([`DriftTuning::catch_lock_s`]).

use std::sync::RwLock;

/// Drift-assist constants. [`DriftTuning::default`] is what ships; the testbed's tuning
/// panel can replace it at runtime ([`set_tuning_override`]).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DriftTuning {
    /// Rear-axle slip angle where the slide onset starts, rad.
    pub onset_slip_rad: f32,
    /// Rear-axle slip angle where the onset is full, rad.
    pub onset_full_rad: f32,
    /// Rear-axle slip growth rate that starts the onset on its own (a slide building fast,
    /// before the angle is large), rad/s; full at twice this.
    pub onset_rate_rps: f32,
    /// Time for the rack to travel the car's full lock toward the countersteer side at the
    /// default 60 % countersteer slider, s (scales with the slider: 100 % → 0.6 × this).
    pub catch_lock_s: f32,
    /// Intent modes on (hold, angle-up, envelope, opt-in throttle shaping). Off = step 1 only.
    pub intent: bool,
    /// Lift-off transitions (row 107): off the throttle with the slide swinging through the
    /// centre, the hold lets go ([`DriftMode::Transition`]). Off = Drift Lab C step 1.
    pub transition: bool,
    /// Hold: front slip angle (× the tyres' peak slip) given up per 0.1 rad (5.7°) the slide
    /// angle is past the held angle.
    pub hold_gain: f32,
    /// Hold: front slip angle given up per rad/s the slide angle grows, s.
    pub hold_damp: f32,
    /// Hold: front slip angle into the turn at the held angle, × the tyres' peak slip.
    pub hold_base: f32,
    /// AngleUp entered from grip (turning into a drift): the held angle starts at least here,
    /// degrees, so the drift can build. Entering from a tap keeps the current angle.
    pub hold_entry_deg: f32,
    /// Hold (countersteer key): the held angle shrinks this fast while the key stays down,
    /// degrees per second (a longer hold slowly closes the slide).
    pub hold_decay_dps: f32,
    /// AngleUp (key into the turn): the held angle grows this fast while the key is down,
    /// degrees per second (up to just inside the envelope).
    pub angle_up_dps: f32,
    /// AngleUp: extra front slip angle into the turn while the key is down, × the tyres' peak
    /// slip (a tighter line).
    pub angle_up_steer: f32,
    /// AngleUp: front slip angle given up per rad/s the slide angle grows, s (as
    /// [`DriftTuning::hold_damp`]; more, so a held key into the turn adds angle without
    /// overshooting the envelope).
    pub angle_up_damp: f32,
    /// Off the throttle the held angle shrinks this fast, degrees per second (a lift closes
    /// the slide).
    pub lift_shrink_dps: f32,
    /// Hold / AngleUp steering authority with the throttle released, 0..=1 (a lift loosens
    /// the hold toward the plain fallback; 1 = the lift changes only the held angle).
    pub lift_authority: f32,
    /// Throttle shaping strength, 0 (raw key, the default) ..= 1. Opt-in (owner rule, row
    /// 104: TCS off = the driver gets exactly the throttle they press). Also gates the
    /// lift softening.
    pub throttle_shape: f32,
    /// While sliding, the throttle may rise from 0 to 1 no faster than this, s (× shaping
    /// strength): a 0/1 key becomes a quick ramp, never a fixed partial throttle, so the
    /// revs still follow the key.
    pub throttle_rise_s: f32,
    /// Trim zone below the envelope, degrees: the throttle is trimmed only as the slide (or
    /// where it is heading in [`ENVELOPE_LOOKAHEAD_S`]) comes within this of the envelope.
    pub trim_margin_deg: f32,
    /// Deepest trim at the envelope edge, fraction of the throttle (× shaping strength).
    pub trim_depth: f32,
    /// Slide-angle envelope at the 60 % slider, degrees (+25° per +100 % of slider).
    pub envelope_deg: f32,
    /// Straighten: extra countersteer past the travel while the slide angle still grows,
    /// × the tyres' peak slip at 0.5 rad/s of growth.
    pub straighten_gain: f32,
    /// Abrupt-lift softening, 0 (raw pedal release) ..= 1 (a full lift takes
    /// [`LIFT_SOFT_S`]). Only with throttle shaping on ([`DriftTuning::throttle_shape`] > 0).
    pub lift_soften: f32,
}

impl Default for DriftTuning {
    fn default() -> Self {
        Self {
            onset_slip_rad: 0.025,
            onset_full_rad: 0.06,
            onset_rate_rps: 0.3,
            catch_lock_s: 0.10,
            intent: true,
            transition: true,
            hold_gain: 0.3,
            hold_damp: 0.2,
            hold_base: 1.0,
            hold_entry_deg: 18.0,
            hold_decay_dps: 3.0,
            angle_up_dps: 15.0,
            angle_up_steer: 0.5,
            angle_up_damp: 0.5,
            lift_shrink_dps: 6.0,
            lift_authority: 0.7,
            throttle_shape: 0.0,
            throttle_rise_s: 0.35,
            trim_margin_deg: 8.0,
            trim_depth: 0.6,
            envelope_deg: 35.0,
            straighten_gain: 1.0,
            lift_soften: 0.5,
        }
    }
}

static OVERRIDE: RwLock<Option<DriftTuning>> = RwLock::new(None);

/// Replace the drift-assist constants for every vehicle in this process (`None` = the
/// defaults). Testbed tuning panel only; see the module docs.
pub fn set_tuning_override(t: Option<DriftTuning>) {
    if let Ok(mut o) = OVERRIDE.write() {
        *o = t;
    }
}

/// The drift-assist constants in effect: the override if set, else the defaults.
pub fn tuning() -> DriftTuning {
    OVERRIDE.read().ok().and_then(|o| *o).unwrap_or_default()
}

/// Time constant of the rear-axle slip rate filter, s.
pub const SLIP_RATE_TAU_S: f32 = 0.02;
/// Time constant of the slide-angle rate filter, s.
pub const SLIDE_RATE_TAU_S: f32 = 0.05;
/// How far ahead the envelope looks at a growing slide, s: the assist backs off when the
/// slide angle this far ahead (at its current growth rate) is past the envelope — yaw it
/// cannot cover with steering alone.
pub const ENVELOPE_LOOKAHEAD_S: f32 = 0.25;
/// Width of the envelope's fade (full help → none), rad (8°).
pub const ENVELOPE_FADE_RAD: f32 = 0.14;
/// Once fully past the envelope the assist stays off until the slide is this far back
/// inside it, rad (5°).
pub const ENVELOPE_REARM_RAD: f32 = 0.087;
/// Envelope widening after a stable hold: up to this much, rad (5°), from 0.8 s to 1.8 s.
pub const ENVELOPE_BONUS_RAD: f32 = 0.087;
/// Envelope narrowing per unit of erratic input (0..=1), rad (15°).
pub const ENVELOPE_ERRATIC_RAD: f32 = 0.26;
/// Erratic-input score added per steering reversal (decays at 0.4 per second).
pub const ERRATIC_PER_REVERSAL: f32 = 0.4;
/// Full-lift time at `lift_soften` 1, s.
pub const LIFT_SOFT_S: f32 = 0.5;
/// Held angle follows the slide angle up with this time constant while on the throttle, s.
pub const HOLD_FOLLOW_UP_S: f32 = 0.6;
/// … and down with this one, s.
pub const HOLD_FOLLOW_DOWN_S: f32 = 1.5;

/// Slide angle that counts as a side for the lift-off transition, rad (2°).
pub const TRANSITION_SIDE_RAD: f32 = 0.035;
/// Closing rate of the slide angle that, off the throttle, counts as heading through zero
/// (with [`TRANSITION_LOOKAHEAD_S`]), rad/s.
pub const TRANSITION_RATE_RPS: f32 = 0.3;
/// After a transition the new drift's envelope ignores the look-ahead, and key swaps are
/// not erratic, for this long, s.
pub const TRANSITION_GRACE_S: f32 = 0.6;
/// Off the throttle, a slide that at its closing rate reaches zero within this is a lift-off
/// transition, s.
pub const TRANSITION_LOOKAHEAD_S: f32 = 0.4;

/// After the handbrake is released, the steering assists (self-align, the intent modes) stay
/// off this long, s (row 108; they were off for the whole
/// [`crate::steering::HANDBRAKE_GRACE_S`], 0.5 s, and released keys could not catch a
/// handbrake entry). ESC and the clutch keep the longer grace.
pub const HANDBRAKE_ASSIST_GRACE_S: f32 = 0.08;

/// Whether the steering assists treat the handbrake as held (row 108): while it is pulled
/// (`held`, or `hb_grace` at its full [`crate::steering::HANDBRAKE_GRACE_S`]) — handbrake turns
/// rotate freely. After the release it depends on the key (`driver` steer, + = right, against
/// the rear-axle slip `rear_axle_slip`, + = rear sliding left): the countersteer key brings
/// the assists back at once, released keys after [`HANDBRAKE_ASSIST_GRACE_S`] (the
/// self-aligning catch), and the key still into the turn keeps the whole grace (the handbrake
/// turn goes on rotating freely, rows 56 / 72).
pub fn handbrake_assist_hold(hb_grace: f32, held: bool, driver: f32, rear_axle_slip: f32) -> bool {
    let full = crate::steering::HANDBRAKE_GRACE_S;
    if held || hb_grace >= full {
        return true;
    }
    if hb_grace <= 0.0 {
        return false;
    }
    if driver.abs() <= crate::steering::STEER_DEADZONE {
        return full - hb_grace < HANDBRAKE_ASSIST_GRACE_S;
    }
    // Into the turn: the whole grace; countersteering: back at once.
    -driver * rear_axle_slip <= 0.0
}

/// Device weight of the intent modes from the speed sensitivity: keyboard (1.0) → 1,
/// gamepad (0.7) → 0.5, wheel (0) → 0.
pub fn device_weight(steer_speed_sensitivity: f32) -> f32 {
    ((steer_speed_sensitivity - 0.4) / 0.6).clamp(0.0, 1.0)
}

/// Slide-angle envelope for countersteer slider `help`, before the stable / erratic
/// adjustments, rad: `envelope_deg` at 60 %, ±25° per 100 % (30 % → 27.5°, 100 % → 45°).
pub fn base_envelope(t: &DriftTuning, help: f32) -> f32 {
    (t.envelope_deg + 25.0 * (help - 0.6))
        .max(10.0)
        .to_radians()
}

/// Per-vehicle drift-assist state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DriftState {
    /// Rear-axle slip angle last tick, rad.
    pub prev_rear_slip: f32,
    /// Filtered rear-axle slip rate, rad/s (+ = toward +, i.e. the rear moving more to the
    /// left).
    pub rear_slip_rate: f32,
    /// Slide angle (|body slip|) last tick, rad.
    pub prev_slide: f32,
    /// Filtered slide-angle rate, rad/s (+ = growing).
    pub slide_rate: f32,
    /// Slide angle Hold keeps, rad.
    pub hold_angle: f32,
    /// Time in a stable hold, s.
    pub stable_s: f32,
    /// Erratic-input score 0..=1 (steering reversals; decays).
    pub erratic: f32,
    /// Sign of the last non-zero steer input (−1, 0, +1).
    pub last_sign: f32,
    /// Past the envelope and not yet back inside by [`ENVELOPE_REARM_RAD`] (1 = yes).
    pub exceeded: f32,
    /// Mode code last tick ([`DriftMode::code`]).
    pub mode: f32,
    /// Side of the current drift: the sign of the body slip (+ = sliding toward the left of
    /// the heading), 0 = none.
    pub side: f32,
    /// 1 = in a lift-off transition ([`DriftMode::Transition`]); < 0 = the new drift's grace
    /// after one, seconds left (negated, counting up to 0); 0 = neither.
    pub transition: f32,
}

/// What the drift assist reads each tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriftInputs {
    /// Driver steer, -1 (left) ..= 1 (right).
    pub driver: f32,
    /// Throttle input 0..=1 (before filtering).
    pub throttle: f32,
    /// Forward speed, m/s.
    pub speed: f32,
    /// Body slip at the CG, rad (+ = moving toward the left of the heading).
    pub body_slip: f32,
    /// Countersteer side (+ = wheels left) and slide onset 0..=1
    /// ([`crate::steering::onset_of`]).
    pub side: f32,
    /// See [`DriftInputs::side`].
    pub onset: f32,
    /// Countersteer slider 0..=1.
    pub help: f32,
    /// Steering speed sensitivity (device).
    pub sensitivity: f32,
    /// Handbrake held or just released.
    pub handbrake_hold: bool,
}

/// What the drift assist asks of the steering and the throttle this tick.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DriftCommand {
    /// Mode.
    pub mode: DriftMode,
    /// Envelope gate 0..=1 on everything the assist does (step 1 included): 1 inside the
    /// envelope, fading to 0 past it. 1 with the intent modes off.
    pub authority: f32,
    /// Device weight of the intent modes ([`device_weight`]; 0 with them off).
    pub weight: f32,
    /// Slide angle, rad.
    pub slide: f32,
    /// Slide-angle rate, rad/s.
    pub slide_rate: f32,
    /// Slide angle Hold keeps, rad.
    pub hold_angle: f32,
    /// Envelope, rad (0 with the intent modes off).
    pub envelope: f32,
    /// Throttle multiplier, 0..=1.
    pub throttle_scale: f32,
    /// Hold's steering authority from the throttle: 1 on the throttle, down to
    /// [`DriftTuning::lift_authority`] off it.
    pub hold_throttle: f32,
    /// Shortest time a full lift may take, s (0 = raw).
    pub lift_time_s: f32,
    /// Shortest time the throttle may take to rise 0 → 1, s (0 = raw).
    pub rise_time_s: f32,
}

impl DriftState {
    /// Number of f32 values in [`DriftState::to_array`].
    pub const LEN: usize = 12;

    /// Advance the slip-rate filter with this tick's rear-axle slip (rad).
    pub fn update_slip(&mut self, rear_axle_slip: f32, dt: f32) {
        if dt > 0.0 {
            let raw = (rear_axle_slip - self.prev_rear_slip) / dt;
            self.rear_slip_rate += (raw - self.rear_slip_rate) * (dt / SLIP_RATE_TAU_S).min(1.0);
        }
        self.prev_rear_slip = rear_axle_slip;
    }

    /// Read the driver's intent and the slide, update the envelope, and return this tick's
    /// [`DriftCommand`] (call once per tick, after [`DriftState::update_slip`]).
    pub fn update(&mut self, inp: &DriftInputs, t: &DriftTuning, dt: f32) -> DriftCommand {
        let a = inp.body_slip.abs();
        if dt > 0.0 {
            let raw = (a - self.prev_slide) / dt;
            self.slide_rate += (raw - self.slide_rate) * (dt / SLIDE_RATE_TAU_S).min(1.0);
        }
        self.prev_slide = a;
        // Erratic input: steering reversals (left ↔ right) raise it, it decays in ~2.5 s.
        let dz = crate::steering::STEER_DEADZONE;
        let sign = if inp.driver > dz {
            1.0
        } else if inp.driver < -dz {
            -1.0
        } else {
            0.0
        };
        // The key swap of a lift-off transition (and of the new drift's first moments) is not
        // erratic.
        let swapping = self.transition != 0.0;
        if sign != 0.0 && self.last_sign != 0.0 && sign != self.last_sign && !swapping {
            self.erratic = (self.erratic + ERRATIC_PER_REVERSAL).min(1.0);
        }
        if sign != 0.0 {
            self.last_sign = sign;
        }
        self.erratic = (self.erratic - 0.4 * dt).max(0.0);

        let help = inp.help.clamp(0.0, 1.0);
        let mut cmd = DriftCommand {
            mode: DriftMode::Off,
            authority: 1.0,
            weight: 0.0,
            slide: a,
            slide_rate: self.slide_rate,
            hold_angle: self.hold_angle,
            envelope: 0.0,
            throttle_scale: 1.0,
            hold_throttle: 1.0,
            lift_time_s: 0.0,
            rise_time_s: 0.0,
        };
        if help <= 0.0 || inp.sensitivity <= 0.0 {
            self.reset_hold();
            return cmd;
        }
        let raw_mode = if inp.onset <= 0.0 && a < 0.07 {
            DriftMode::Grip
        } else {
            intent(inp.driver, inp.side, inp.onset.max(1e-3))
        };
        if !t.intent || inp.speed < 5.0 || inp.handbrake_hold {
            // Step 1 only (or slow / handbrake): no envelope, no hold, no shaping.
            cmd.mode = raw_mode;
            self.reset_hold();
            if t.intent && inp.handbrake_hold {
                // A handbrake entry is a deliberate rotation: the drift it starts gets the
                // fresh-drift grace (no envelope look-ahead) once the assists are back (row 108).
                self.transition = -TRANSITION_GRACE_S;
            }
            self.mode = cmd.mode.code() as f32;
            return cmd;
        }
        cmd.weight = device_weight(inp.sensitivity);

        // Lift-off transition (row 107): off the throttle with the slide crossing to the
        // other side, heading through zero within the look-ahead, or the countersteer key
        // pressed (not already held) — the key toward the next corner — the hold lets go at once
        // and the car rotates freely through the centre: released keys straighten (fast
        // rack), a held key steers plainly (no hold, no envelope) so the car can swing. It ends when
        // the new drift starts — slide on a side and the countersteer key or the throttle —
        // or when the car settles straight. The new drift gets a fresh held angle and its own
        // envelope (no stale "exceeded").
        let slide_side = if a > TRANSITION_SIDE_RAD {
            inp.body_slip.signum()
        } else {
            0.0
        };
        if self.transition > 0.0 {
            let settled = a < TRANSITION_SIDE_RAD && self.slide_rate.abs() < 0.3;
            // Back on the throttle (either side), or the countersteer key once the slide has
            // swung to the new side.
            let new_drift = slide_side != 0.0
                && (inp.throttle > 0.1 || (raw_mode == DriftMode::Hold && slide_side != self.side));
            if settled || raw_mode == DriftMode::Grip || new_drift {
                self.transition = if new_drift { -TRANSITION_GRACE_S } else { 0.0 };
                self.side = if new_drift { slide_side } else { 0.0 };
                self.exceeded = 0.0;
                self.stable_s = 0.0;
            }
        } else if t.transition
            && self.side != 0.0
            && inp.throttle < 0.1
            && matches!(
                DriftMode::from_code(self.mode),
                DriftMode::Hold | DriftMode::AngleUp | DriftMode::Straighten
            )
        {
            let crossed = slide_side != 0.0 && slide_side != self.side;
            let through = self.slide_rate < -TRANSITION_RATE_RPS
                && a + self.slide_rate * TRANSITION_LOOKAHEAD_S < TRANSITION_SIDE_RAD;
            // The countersteer key pressed off the throttle (it was not already holding): the
            // key toward the next corner of a flick, not a hold — a hold needs the throttle.
            let flick =
                raw_mode == DriftMode::Hold && DriftMode::from_code(self.mode) != DriftMode::Hold;
            if crossed || through || flick {
                self.transition = 1.0;
                self.exceeded = 0.0;
                self.stable_s = 0.0;
            }
        }
        if self.transition > 0.0 {
            self.hold_angle = a;
            cmd.mode = DriftMode::Transition;
            cmd.hold_angle = a;
            cmd.envelope = base_envelope(t, help);
            self.mode = cmd.mode.code() as f32;
            return cmd;
        }
        if raw_mode == DriftMode::Grip {
            self.side = 0.0;
        } else if slide_side != 0.0 {
            self.side = slide_side;
        }
        // The new drift's grace after a transition: no look-ahead on the envelope (the swing
        // builds the new slide fast; the hold's damping catches it).
        let grace = self.transition < 0.0;
        if grace {
            self.transition = (self.transition + dt).min(0.0);
        }

        // Envelope: wider after a stable hold, narrower with erratic input.
        let stable =
            DriftMode::from_code(self.mode) == DriftMode::Hold && self.slide_rate.abs() < 0.2;
        self.stable_s = if stable {
            self.stable_s + dt
        } else {
            (self.stable_s - 2.0 * dt).max(0.0)
        };
        let bonus = ENVELOPE_BONUS_RAD * ((self.stable_s - 0.8) / 1.0).clamp(0.0, 1.0);
        let env = (base_envelope(t, help) + bonus - ENVELOPE_ERRATIC_RAD * self.erratic).max(0.14);
        let ahead = if grace {
            a
        } else {
            a + self.slide_rate.max(0.0) * ENVELOPE_LOOKAHEAD_S
        };
        if ahead >= env + ENVELOPE_FADE_RAD {
            self.exceeded = 1.0;
        } else if a < env - ENVELOPE_REARM_RAD {
            self.exceeded = 0.0;
        }
        cmd.envelope = env;
        cmd.authority = if self.exceeded > 0.0 {
            0.0
        } else {
            1.0 - ((ahead - env) / ENVELOPE_FADE_RAD).clamp(0.0, 1.0)
        };
        cmd.mode = if raw_mode == DriftMode::Grip {
            DriftMode::Grip
        } else if cmd.authority <= 0.0 {
            DriftMode::Exceeded
        } else {
            raw_mode
        };

        // Held angle (row 106). Countersteer key (Hold): captured when the key goes down
        // (taps nudge it), shrinking slowly while it stays down. Key into the turn (AngleUp):
        // grows while the key is down. Both: on the throttle it follows the slide (more
        // throttle grows it), off the throttle it shrinks (a lift closes the slide). Never
        // past just inside the envelope.
        let prev = DriftMode::from_code(self.mode);
        let key = inp.driver.abs().min(1.0);
        let cap = (cmd.envelope - ENVELOPE_REARM_RAD).max(0.0);
        match cmd.mode {
            DriftMode::Hold | DriftMode::AngleUp if prev != cmd.mode => {
                self.hold_angle = if cmd.mode == DriftMode::AngleUp
                    && (prev == DriftMode::Grip || prev == DriftMode::Off)
                {
                    // Turning into a drift: let it build to the entry angle.
                    a.max(t.hold_entry_deg.to_radians())
                } else {
                    a
                }
                .min(cap);
            }
            DriftMode::Hold | DriftMode::AngleUp => {
                if inp.throttle > 0.1 {
                    let tau = if a > self.hold_angle {
                        HOLD_FOLLOW_UP_S
                    } else {
                        HOLD_FOLLOW_DOWN_S
                    };
                    self.hold_angle += (a - self.hold_angle) * (dt / tau).min(1.0);
                } else {
                    self.hold_angle -= t.lift_shrink_dps.to_radians() * dt;
                }
                self.hold_angle += if cmd.mode == DriftMode::Hold {
                    -t.hold_decay_dps.to_radians() * key * dt
                } else {
                    t.angle_up_dps.to_radians() * key * dt
                };
                self.hold_angle = self.hold_angle.clamp(0.0, cap);
            }
            DriftMode::Exceeded => {}
            _ => self.hold_angle = a,
        }
        cmd.hold_angle = self.hold_angle;
        let la = t.lift_authority.clamp(0.0, 1.0);
        cmd.hold_throttle = la + (1.0 - la) * inp.throttle.clamp(0.0, 1.0);

        // Throttle shaping (opt-in, default off — row 104), meant to be invisible: the key is passed through (a held key is
        // full throttle and the revs climb as the wheels spin up); in a slide the assist only
        // limits how fast the throttle rises, and trims it as the slide nears the envelope.
        // Both fade with the authority past the envelope, so flooring it at a big angle still
        // spins a powerful car.
        let sliding = matches!(
            cmd.mode,
            DriftMode::Hold | DriftMode::Straighten | DriftMode::AngleUp
        );
        let k = t.throttle_shape.clamp(0.0, 1.0) * cmd.authority * cmd.weight;
        if sliding && k > 0.0 {
            let margin = t.trim_margin_deg.max(0.1).to_radians();
            let near = ((ahead - (env - margin)) / margin).clamp(0.0, 1.0);
            cmd.throttle_scale = 1.0 - t.trim_depth.clamp(0.0, 1.0) * near * k;
            cmd.rise_time_s = t.throttle_rise_s.max(0.0) * k;
        }
        // Abrupt lift: softened, not cancelled — only with throttle shaping turned on.
        if t.throttle_shape > 0.0
            && matches!(
                cmd.mode,
                DriftMode::Hold | DriftMode::Straighten | DriftMode::AngleUp
            )
        {
            cmd.lift_time_s =
                LIFT_SOFT_S * t.lift_soften.clamp(0.0, 1.0) * cmd.authority * cmd.weight;
        }
        self.mode = cmd.mode.code() as f32;
        cmd
    }

    fn reset_hold(&mut self) {
        self.stable_s = 0.0;
        self.exceeded = 0.0;
        self.hold_angle = self.prev_slide;
        self.mode = 0.0;
        self.side = 0.0;
        self.transition = 0.0;
    }

    /// The state as plain values (snapshots).
    pub fn to_array(&self) -> [f32; Self::LEN] {
        [
            self.prev_rear_slip,
            self.rear_slip_rate,
            self.prev_slide,
            self.slide_rate,
            self.hold_angle,
            self.stable_s,
            self.erratic,
            self.last_sign,
            self.exceeded,
            self.mode,
            self.side,
            self.transition,
        ]
    }

    /// Inverse of [`DriftState::to_array`].
    pub fn from_array(a: [f32; Self::LEN]) -> Self {
        Self {
            prev_rear_slip: a[0],
            rear_slip_rate: a[1],
            prev_slide: a[2],
            slide_rate: a[3],
            hold_angle: a[4],
            stable_s: a[5],
            erratic: a[6],
            last_sign: a[7],
            exceeded: a[8],
            mode: a[9],
            side: a[10],
            transition: a[11],
        }
    }
}

/// What the drift assist is doing this tick (telemetry, the Drift Lab readout).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum DriftMode {
    /// No assist: countersteer slider at 0, or no speed sensitivity (wheel / AI).
    #[default]
    Off,
    /// Not sliding (below the onset), or too slow.
    Grip,
    /// Sliding, steering released: the wheels align with the front axle's travel.
    Straighten,
    /// Sliding, countersteer key (against the slide): the assist supplies the opposite lock
    /// that holds the slide angle (row 106).
    Hold,
    /// Sliding, key into the turn: more angle, a tighter line.
    AngleUp,
    /// Past the assist's envelope: it backs off and the car can spin.
    Exceeded,
    /// Lift-off transition (row 107): off the throttle with the slide swinging through the
    /// centre; no hold: released keys straighten, a held key steers plainly, until the new
    /// drift starts.
    Transition,
}

impl DriftMode {
    /// Stable small number for packed telemetry (0 = Off … 5 = Exceeded, 6 = Transition).
    pub fn code(self) -> u8 {
        match self {
            DriftMode::Off => 0,
            DriftMode::Grip => 1,
            DriftMode::Straighten => 2,
            DriftMode::Hold => 3,
            DriftMode::AngleUp => 4,
            DriftMode::Exceeded => 5,
            DriftMode::Transition => 6,
        }
    }

    /// Inverse of [`DriftMode::code`] (unknown → Off).
    pub fn from_code(c: f32) -> Self {
        match c as u8 {
            1 => DriftMode::Grip,
            2 => DriftMode::Straighten,
            3 => DriftMode::Hold,
            4 => DriftMode::AngleUp,
            5 => DriftMode::Exceeded,
            6 => DriftMode::Transition,
            _ => DriftMode::Off,
        }
    }
}

/// Drift-assist telemetry for one tick.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DriftTelemetry {
    /// Assist mode.
    pub mode: DriftMode,
    /// Slide angle (body slip at the CG, unsigned), rad.
    pub slide_angle: f32,
    /// Slide onset 0..=1 ([`crate::steering::slide_onset`]).
    pub onset: f32,
    /// Slide-angle envelope the assist helps up to, rad (0 = none).
    pub envelope: f32,
    /// Assist authority 0..=1 (1 inside the envelope, fading to 0 past it).
    pub authority: f32,
    /// Multiplier the assist applied to the throttle, 0..=1 (1 = untouched).
    pub throttle_scale: f32,
    /// Slide angle Hold / AngleUp keeps, rad.
    pub hold_angle: f32,
    /// Throttle key / pedal as pressed, 0..=1 (compare [`crate::Telemetry::throttle`], what
    /// reached the engine).
    pub throttle_key: f32,
}

/// Classify the steering intent: `driver` steer (+ = right), countersteer `side` (+ = wheels
/// left), `onset` 0..=1. Released → Straighten, countersteer key → Hold, key into the turn →
/// AngleUp (Drift Lab C, row 106).
pub fn intent(driver: f32, side: f32, onset: f32) -> DriftMode {
    if onset <= 0.0 {
        DriftMode::Grip
    } else if driver.abs() <= crate::steering::STEER_DEADZONE {
        DriftMode::Straighten
    } else if -driver * side > 0.0 {
        DriftMode::Hold
    } else {
        DriftMode::AngleUp
    }
}

/// Hold / AngleUp steering (Drift Lab C, row 106): `(target, weight)`. `target` is the centre
/// road-wheel angle (rad, + = left) that keeps the held slide angle: along the front axle's
/// travel plus a front slip angle into the turn that shrinks (to a countersteer past the
/// travel) as the slide opens past the held angle or grows fast; AngleUp adds
/// [`DriftTuning::angle_up_steer`] more into the turn. Off the throttle it loosens toward the
/// front axle's travel ([`DriftTuning::lift_authority`]). Below 3–7° of slide it leans toward
/// the entry fallback: the travel for Hold (a held counter key never asks for the key's full
/// lock), the key's own target for AngleUp (so the entry still turns in). `weight`
/// (authority × device weight) blends it with the key's own target `driver_target`; 0 = the
/// plain key.
pub fn hold_steer(
    inp: &crate::steering::SteerInput,
    driver_target: f32,
    max_steer: f32,
) -> (f32, f32) {
    let cmd = inp.drift;
    let t = &inp.tuning;
    let (side, onset) = crate::steering::slide_onset(inp);
    let peak = inp.front_peak_slip;
    let (into, damp) = if cmd.mode == DriftMode::AngleUp {
        (
            1.0 + t.angle_up_steer * inp.driver.abs().min(1.0),
            t.angle_up_damp,
        )
    } else {
        (1.0, t.hold_damp)
    };
    let alpha = (t.hold_base * peak * into
        - t.hold_gain * peak / 0.1 * (cmd.slide - cmd.hold_angle)
        - damp * cmd.slide_rate)
        .clamp(-2.0 * peak, 1.5 * peak);
    let full = max_steer * inp.ratio_scale;
    let hold = (inp.front_axle_angle - side * alpha).clamp(-full, full);
    // Off the throttle the hold loosens toward the front axle's travel (straighten).
    let travel = (inp.front_axle_angle * onset).clamp(-full, full);
    let assisted = travel + (hold - travel) * cmd.hold_throttle;
    let entry = if cmd.mode == DriftMode::Hold {
        travel
    } else {
        driver_target
    };
    let k = ((cmd.slide - 0.05) / 0.07).clamp(0.0, 1.0);
    let target = entry + (assisted - entry) * k;
    (target, cmd.authority * cmd.weight)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(driver: f32, slip_deg: f32) -> DriftInputs {
        DriftInputs {
            driver,
            throttle: 1.0,
            speed: 15.0,
            body_slip: -slip_deg.to_radians(),
            side: -1.0,
            onset: 1.0,
            help: 0.6,
            sensitivity: 1.0,
            handbrake_hold: false,
        }
    }

    fn run(st: &mut DriftState, inp: &DriftInputs, ticks: usize) -> DriftCommand {
        run_with(st, inp, ticks, &DriftTuning::default())
    }

    fn run_with(
        st: &mut DriftState,
        inp: &DriftInputs,
        ticks: usize,
        t: &DriftTuning,
    ) -> DriftCommand {
        let mut c = DriftCommand::default();
        for _ in 0..ticks {
            c = st.update(inp, t, 1.0 / 240.0);
        }
        c
    }

    #[test]
    fn modes_follow_the_keys() {
        // Rear sliding right (countersteer side −): the right key (+1) is the countersteer,
        // the left key (−1) is into the turn. (Start already at the angle: a jump from 0 reads
        // as a slide growing past the envelope.)
        let mut st = DriftState {
            prev_slide: 15f32.to_radians(),
            ..DriftState::default()
        };
        assert_eq!(run(&mut st, &inputs(1.0, 15.0), 10).mode, DriftMode::Hold);
        assert_eq!(
            run(&mut st, &inputs(0.0, 15.0), 10).mode,
            DriftMode::Straighten
        );
        assert_eq!(
            run(&mut st, &inputs(-1.0, 15.0), 10).mode,
            DriftMode::AngleUp
        );
        assert_eq!(
            run(&mut st, &inputs(1.0, 70.0), 10).mode,
            DriftMode::Exceeded
        );
        // The wheel (no speed sensitivity) and slider 0: off.
        let wheel = DriftInputs {
            sensitivity: 0.0,
            ..inputs(-1.0, 15.0)
        };
        assert_eq!(
            run(&mut DriftState::default(), &wheel, 10).mode,
            DriftMode::Off
        );
    }

    #[test]
    fn envelope_by_slider_and_behaviour() {
        let t = DriftTuning::default();
        assert!((base_envelope(&t, 0.6).to_degrees() - 35.0).abs() < 1e-3);
        assert!((base_envelope(&t, 1.0).to_degrees() - 45.0).abs() < 1e-3);
        // A stable hold widens it; sawing at the keys narrows it.
        let mut st = DriftState::default();
        let calm = run(&mut st, &inputs(1.0, 20.0), 600);
        assert!(calm.envelope > base_envelope(&t, 0.6) + 0.05, "{calm:?}");
        let mut st = DriftState::default();
        let mut c = DriftCommand::default();
        for i in 0..240 {
            let d = if (i / 30) % 2 == 0 { -1.0 } else { 1.0 };
            c = run(&mut st, &inputs(d, 20.0), 1);
        }
        assert!(c.envelope < base_envelope(&t, 0.6) - 0.1, "{c:?}");
    }

    #[test]
    fn throttle_is_untouched_by_default() {
        let mut st = DriftState {
            prev_slide: 33f32.to_radians(),
            ..DriftState::default()
        };
        let c = run(&mut st, &inputs(-1.0, 33.0), 120);
        assert_eq!(
            (c.throttle_scale, c.rise_time_s, c.lift_time_s),
            (1.0, 0.0, 0.0)
        );
    }

    #[test]
    fn throttle_is_rate_limited_and_trimmed_only_near_the_envelope() {
        // Opt-in: with shaping turned on.
        let on = DriftTuning {
            throttle_shape: 0.7,
            ..DriftTuning::default()
        };
        let settled = |deg: f32| DriftState {
            prev_slide: deg.to_radians(),
            ..DriftState::default()
        };
        // Well inside (20° of 35°): no trim, only a rise-rate limit (no plateau).
        let mut st = settled(20.0);
        let hold = run_with(&mut st, &inputs(-1.0, 20.0), 120, &on);
        assert_eq!(hold.throttle_scale, 1.0, "{hold:?}");
        assert!(hold.rise_time_s > 0.1, "{hold:?}");
        // Near the edge (33°): trimmed.
        let mut st = settled(33.0);
        let near = run_with(&mut st, &inputs(-1.0, 33.0), 120, &on);
        assert!(near.throttle_scale < 0.8, "{near:?}");
        // Past it: raw throttle, no help at all.
        let mut st = settled(70.0);
        let past = run_with(&mut st, &inputs(-1.0, 70.0), 120, &on);
        assert_eq!((past.throttle_scale, past.rise_time_s), (1.0, 0.0));
        assert_eq!(past.authority, 0.0);
    }

    #[test]
    fn counter_key_slowly_shrinks_the_held_angle_into_key_grows_it() {
        let start = DriftState {
            prev_slide: 20f32.to_radians(),
            mode: DriftMode::Straighten.code() as f32,
            ..DriftState::default()
        };
        // 1 s of the countersteer key at 20°: captured at 20°, −3°/s.
        let mut st = start;
        let c = run(&mut st, &inputs(1.0, 20.0), 240);
        let held = c.hold_angle.to_degrees();
        assert!((15.0..19.5).contains(&held), "{held}");
        // 0.5 s of the key into the turn: up by ~7°.
        let mut st = start;
        let c = run(&mut st, &inputs(-1.0, 20.0), 120);
        let held = c.hold_angle.to_degrees();
        assert!(held > 24.0 && held <= 30.0 + 1e-3, "{held}");
    }

    #[test]
    fn lift_with_the_flick_key_is_a_transition_and_the_new_drift_is_fresh() {
        // Left-hand drift (body slip −, countersteer key = right, +1) at 25°, keys released.
        let lifted = |driver: f32, slip_deg: f32| DriftInputs {
            throttle: 0.0,
            ..inputs(driver, slip_deg)
        };
        let mut st = DriftState {
            prev_slide: 25f32.to_radians(),
            ..DriftState::default()
        };
        run(&mut st, &inputs(0.0, 25.0), 30);
        assert_eq!(st.side, -1.0);
        // Off the throttle, the countersteer key goes down: the flick, not a hold.
        let c = run(&mut st, &lifted(1.0, 25.0), 1);
        assert_eq!(c.mode, DriftMode::Transition, "{c:?}");
        // The slide swings to the other side (body slip +); back on the throttle: a new drift
        // (rear now sliding left: countersteer side +), with no stale envelope state.
        let new = DriftInputs {
            side: 1.0,
            ..inputs(-1.0, -12.0)
        };
        st.prev_slide = 12f32.to_radians();
        let c = run(&mut st, &new, 1);
        assert_eq!(c.mode, DriftMode::Hold, "{c:?}");
        assert_eq!(st.side, 1.0);
        assert!(st.transition < 0.0 && c.authority > 0.99, "{st:?} {c:?}");
        // A lift while holding (no flick) keeps the hold.
        let mut st = DriftState {
            prev_slide: 25f32.to_radians(),
            ..DriftState::default()
        };
        run(&mut st, &inputs(1.0, 25.0), 30);
        assert_eq!(run(&mut st, &lifted(1.0, 25.0), 5).mode, DriftMode::Hold);
    }

    #[test]
    fn handbrake_assists_come_back_quickly() {
        let full = crate::steering::HANDBRAKE_GRACE_S;
        // Pulled: off (free rotation), whatever the keys.
        assert!(handbrake_assist_hold(full, true, 1.0, -0.3));
        // 40 ms after the release, keys released: still off; 100 ms: back.
        assert!(handbrake_assist_hold(full - 0.04, false, 0.0, -0.3));
        assert!(!handbrake_assist_hold(full - 0.1, false, 0.0, -0.3));
        // Already countersteering (rear sliding right, key right): back at once.
        assert!(!handbrake_assist_hold(full - 0.01, false, 1.0, -0.3));
        // Key into the turn after the release: off for the whole grace (the turn goes on).
        assert!(handbrake_assist_hold(full - 0.3, false, -1.0, -0.3));
        assert!(!handbrake_assist_hold(0.0, false, -1.0, -0.3));
    }

    #[test]
    fn snapshot_array_round_trips() {
        let mut st = DriftState::default();
        run(&mut st, &inputs(-1.0, 20.0), 50);
        assert_eq!(DriftState::from_array(st.to_array()), st);
    }
}
