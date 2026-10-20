//! Drift Lab B (owner rows 90 / 91): fast countersteer and the semi-intelligent keyboard drift
//! assist. Steering angle and throttle shaping only: nothing here adds grip or a yaw torque.
//!
//! - [`DriftTuning`]: the constants, with a process-wide runtime override for the testbed's
//!   "tune" panel ([`set_tuning_override`]). The override is a developer tool: it is not part of
//!   the replay log or the netcode, and the server never sets it.
//! - [`DriftState`]: per-vehicle state; [`DriftState::update`] reads the driver's intent each
//!   tick and returns a [`DriftCommand`] for the steering and the throttle.
//!
//! Intent modes (keyboard full strength, gamepad half, the wheel never — see
//! [`device_weight`]):
//! - steering released → **Straighten**: the wheels align with the front axle's travel, a
//!   little past it while the slide still grows ([`DriftTuning::straighten_gain`]);
//! - key into the turn → **Hold**: the front wheels go to the angle that keeps the captured
//!   slide angle ([`DriftTuning::hold_gain`] / `hold_damp` / `hold_base`), so a held key
//!   gives a steady drift; taps and the throttle move the held angle;
//! - key against the slide → **Catch**: fast countersteer plus extra lock;
//! - past the slide-angle envelope ([`base_envelope`], widening after a stable hold,
//!   narrowing with erratic input), or growing faster than it can cover → **Exceeded**: the
//!   assist (step 1's fast countersteer included) backs off and the car can spin.
//!
//! Throttle shaping maps a 0/1 key to a partial throttle that tends to hold the angle;
//! abrupt lifts are softened ([`DriftTuning::lift_soften`]), not cancelled. Both fade out
//! past the envelope.
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
    /// Intent modes on (step 3: hold, throttle shaping, envelope). Off = step 1 only.
    pub intent: bool,
    /// Hold: front slip angle (× the tyres' peak slip) given up per 0.1 rad (5.7°) the slide
    /// angle is past the held angle.
    pub hold_gain: f32,
    /// Hold: front slip angle given up per rad/s the slide angle grows, s.
    pub hold_damp: f32,
    /// Hold: front slip angle into the turn at the held angle, × the tyres' peak slip.
    pub hold_base: f32,
    /// Hold entered from grip (turning into a drift): the held angle starts at least here,
    /// degrees, so the drift can build. Re-entering Hold from a tap keeps the current angle.
    pub hold_entry_deg: f32,
    /// Throttle shaping strength, 0 (raw key) ..= 1.
    pub throttle_shape: f32,
    /// Shaped throttle at the held angle (a held key), 0..=1.
    pub throttle_hold: f32,
    /// Shaped throttle change per rad the slide angle is short of (+) / past (−) the held
    /// angle.
    pub throttle_gain: f32,
    /// Slide-angle envelope at the 60 % slider, degrees (+25° per +100 % of slider).
    pub envelope_deg: f32,
    /// Straighten: extra countersteer past the travel while the slide angle still grows,
    /// × the tyres' peak slip at 0.5 rad/s of growth.
    pub straighten_gain: f32,
    /// Catch: extra countersteer lock past the travel, × the tyres' peak slip.
    pub catch_extra: f32,
    /// Abrupt-lift softening, 0 (raw pedal release) ..= 1 (a full lift takes
    /// [`LIFT_SOFT_S`]).
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
            hold_gain: 0.3,
            hold_damp: 0.2,
            hold_base: 1.0,
            hold_entry_deg: 18.0,
            throttle_shape: 0.7,
            throttle_hold: 0.7,
            throttle_gain: 3.0,
            envelope_deg: 35.0,
            straighten_gain: 1.0,
            catch_extra: 0.5,
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
/// Hold's steering authority with the throttle released, fraction (a lift loosens the hold:
/// the snap is softened, not cancelled).
pub const HOLD_LIFT_AUTHORITY: f32 = 0.35;
/// Full-lift time at `lift_soften` 1, s.
pub const LIFT_SOFT_S: f32 = 0.5;
/// Held angle follows the slide angle up with this time constant while on the throttle, s.
pub const HOLD_FOLLOW_UP_S: f32 = 0.6;
/// … and down with this one, s.
pub const HOLD_FOLLOW_DOWN_S: f32 = 1.5;
/// Off the throttle the held angle shrinks at this rate, rad/s (a lift closes the slide).
pub const HOLD_LIFT_SHRINK_RPS: f32 = 0.25;

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
    /// [`HOLD_LIFT_AUTHORITY`] off it.
    pub hold_throttle: f32,
    /// Shortest time a full lift may take, s (0 = raw).
    pub lift_time_s: f32,
}

impl DriftState {
    /// Number of f32 values in [`DriftState::to_array`].
    pub const LEN: usize = 10;

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
        if sign != 0.0 && self.last_sign != 0.0 && sign != self.last_sign {
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
            self.mode = cmd.mode.code() as f32;
            return cmd;
        }
        cmd.weight = device_weight(inp.sensitivity);

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
        let ahead = a + self.slide_rate.max(0.0) * ENVELOPE_LOOKAHEAD_S;
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

        // Held angle: captured on entering Hold, follows the slide while on the throttle
        // (more throttle grows it), shrinks off the throttle (a lift closes it).
        if cmd.mode == DriftMode::Hold {
            let prev = DriftMode::from_code(self.mode);
            if prev == DriftMode::Grip || prev == DriftMode::Off {
                // Turning into a drift: let it build to the entry angle.
                self.hold_angle = a.max(t.hold_entry_deg.to_radians().min(cmd.envelope));
            } else if prev != DriftMode::Hold {
                // Back on the key after a tap: hold what there is now.
                self.hold_angle = a;
            } else if inp.throttle > 0.1 {
                let tau = if a > self.hold_angle {
                    HOLD_FOLLOW_UP_S
                } else {
                    HOLD_FOLLOW_DOWN_S
                };
                self.hold_angle += (a - self.hold_angle) * (dt / tau).min(1.0);
            } else {
                self.hold_angle = (self.hold_angle - HOLD_LIFT_SHRINK_RPS * dt).max(0.0);
            }
        } else if cmd.mode != DriftMode::Exceeded {
            self.hold_angle = a;
        }
        cmd.hold_angle = self.hold_angle;
        cmd.hold_throttle =
            HOLD_LIFT_AUTHORITY + (1.0 - HOLD_LIFT_AUTHORITY) * inp.throttle.clamp(0.0, 1.0);

        // Throttle shaping: the key becomes a partial throttle that tends to hold the angle
        // (more when the slide closes, less when it opens). Faded out past the envelope, so
        // full throttle at a big angle still spins a powerful car.
        let shaped = match cmd.mode {
            DriftMode::Hold => (t.throttle_hold + t.throttle_gain * (self.hold_angle - a)
                - 0.5 * self.slide_rate.max(0.0))
            .clamp(0.2, 1.0),
            DriftMode::Catch => 0.6,
            _ => 1.0,
        };
        let k = t.throttle_shape.clamp(0.0, 1.0) * cmd.authority * cmd.weight;
        cmd.throttle_scale = 1.0 - (1.0 - shaped) * k;
        // Abrupt lift: softened, not cancelled.
        if matches!(
            cmd.mode,
            DriftMode::Hold | DriftMode::Straighten | DriftMode::Catch
        ) {
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
    /// Sliding, steering into the turn: holding the slide angle.
    Hold,
    /// Sliding, steering against the slide: fast countersteer.
    Catch,
    /// Past the assist's envelope: it backs off and the car can spin.
    Exceeded,
}

impl DriftMode {
    /// Stable small number for packed telemetry (0 = Off … 5 = Exceeded).
    pub fn code(self) -> u8 {
        match self {
            DriftMode::Off => 0,
            DriftMode::Grip => 1,
            DriftMode::Straighten => 2,
            DriftMode::Hold => 3,
            DriftMode::Catch => 4,
            DriftMode::Exceeded => 5,
        }
    }

    /// Inverse of [`DriftMode::code`] (unknown → Off).
    pub fn from_code(c: f32) -> Self {
        match c as u8 {
            1 => DriftMode::Grip,
            2 => DriftMode::Straighten,
            3 => DriftMode::Hold,
            4 => DriftMode::Catch,
            5 => DriftMode::Exceeded,
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
}

/// Classify the steering intent for telemetry (and, from step 3, the assist): `driver` steer
/// (+ = right), countersteer `side` (+ = wheels left), `onset` 0..=1.
pub fn intent(driver: f32, side: f32, onset: f32) -> DriftMode {
    if onset <= 0.0 {
        DriftMode::Grip
    } else if driver.abs() <= crate::steering::STEER_DEADZONE {
        DriftMode::Straighten
    } else if -driver * side > 0.0 {
        DriftMode::Catch
    } else {
        DriftMode::Hold
    }
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
        let t = DriftTuning::default();
        let mut c = DriftCommand::default();
        for _ in 0..ticks {
            c = st.update(inp, &t, 1.0 / 240.0);
        }
        c
    }

    #[test]
    fn modes_follow_the_keys() {
        // Rear sliding right (countersteer side −): left key (−1) is into the turn. (Start
        // already at the angle: a jump from 0 reads as a slide growing past the envelope.)
        let mut st = DriftState {
            prev_slide: 15f32.to_radians(),
            ..DriftState::default()
        };
        assert_eq!(run(&mut st, &inputs(-1.0, 15.0), 10).mode, DriftMode::Hold);
        assert_eq!(
            run(&mut st, &inputs(0.0, 15.0), 10).mode,
            DriftMode::Straighten
        );
        assert_eq!(run(&mut st, &inputs(1.0, 15.0), 10).mode, DriftMode::Catch);
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
        let calm = run(&mut st, &inputs(-1.0, 20.0), 600);
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
    fn throttle_is_shaped_in_hold_only_inside_the_envelope() {
        let mut st = DriftState::default();
        let hold = run(&mut st, &inputs(-1.0, 20.0), 120);
        assert!(hold.throttle_scale < 0.95, "{hold:?}");
        let mut st = DriftState::default();
        let past = run(&mut st, &inputs(-1.0, 70.0), 120);
        assert_eq!(past.throttle_scale, 1.0);
        assert_eq!(past.authority, 0.0);
    }

    #[test]
    fn snapshot_array_round_trips() {
        let mut st = DriftState::default();
        run(&mut st, &inputs(-1.0, 20.0), 50);
        assert_eq!(DriftState::from_array(st.to_array()), st);
    }
}
