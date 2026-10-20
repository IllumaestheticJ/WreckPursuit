//! Drift Lab B (owner rows 90 / 91): fast countersteer and the semi-intelligent keyboard drift
//! assist. Steering angle and throttle shaping only: nothing here adds grip or a yaw torque.
//!
//! - [`DriftTuning`]: the constants, with a process-wide runtime override for the testbed's
//!   "tune" panel ([`set_tuning_override`]). The override is a developer tool: it is not part of
//!   the replay log or the netcode, and the server never sets it.
//! - [`DriftState`]: per-vehicle state (the rear-axle slip rate the onset detector reads).
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
}

impl Default for DriftTuning {
    fn default() -> Self {
        Self {
            onset_slip_rad: 0.025,
            onset_full_rad: 0.06,
            onset_rate_rps: 0.3,
            catch_lock_s: 0.10,
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

/// Per-vehicle drift-assist state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DriftState {
    /// Rear-axle slip angle last tick, rad.
    pub prev_rear_slip: f32,
    /// Filtered rear-axle slip rate, rad/s (+ = toward +, i.e. the rear moving more to the
    /// left).
    pub rear_slip_rate: f32,
}

impl DriftState {
    /// Number of f32 values in [`DriftState::to_array`].
    pub const LEN: usize = 2;

    /// Advance the slip-rate filter with this tick's rear-axle slip (rad).
    pub fn update_slip(&mut self, rear_axle_slip: f32, dt: f32) {
        if dt > 0.0 {
            let raw = (rear_axle_slip - self.prev_rear_slip) / dt;
            self.rear_slip_rate += (raw - self.rear_slip_rate) * (dt / SLIP_RATE_TAU_S).min(1.0);
        }
        self.prev_rear_slip = rear_axle_slip;
    }

    /// The state as plain values (snapshots).
    pub fn to_array(&self) -> [f32; Self::LEN] {
        [self.prev_rear_slip, self.rear_slip_rate]
    }

    /// Inverse of [`DriftState::to_array`].
    pub fn from_array(a: [f32; Self::LEN]) -> Self {
        Self {
            prev_rear_slip: a[0],
            rear_slip_rate: a[1],
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
