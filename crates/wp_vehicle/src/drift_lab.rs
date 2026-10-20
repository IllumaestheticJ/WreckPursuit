//! Drift Lab B measurements (owner rows 90 / 91): scripted slide drivers on a [`Rig`], used
//! by `examples/drift_lab_b.rs`, `tests/drift_lab_b.rs` and the numbers in
//! `docs/integration/drift-lab-b.md`.
//!
//! | measurement | manoeuvre |
//! |---|---|
//! | [`kick_catch`] | straight at speed, a yaw-rate kick; the driver releases the keys, or holds the countersteer key after a reaction time: countersteer slew, onset-to-aligned time, caught or spun |
//! | [`drift_run`] | a power-oversteer drift on a circle held by a scripted driver ([`DriftDriver`]): drift time inside the window, spun or not |
//!
//! Definitions (fixed here, independent of the assist's own tuning):
//! - *slide onset*: the first tick the rear axle's kinematic slip exceeds
//!   [`ONSET_MEASURE_RAD`];
//! - *aligned*: the centre road-wheel angle within 3° of the front axle's direction of travel
//!   (the countersteer that leaves the front tyres no slip angle);
//! - *caught*: never past 90° of body slip, and at the end < 15° slip, |yaw rate| < 0.5 rad/s,
//!   moving forward faster than 8 m/s (as [`crate::metrics::SlideExit::recovered`]).

use wp_contracts::{AssistProfile, LoadedCar, VehicleInput, GRAVITY_MPS2, PHYSICS_DT};
use wp_math::Rng;

use crate::math::{atan, atan2, V3};
use crate::rig::Rig;
use crate::steering::{front_axle_angle, rear_axle_slip};

const TICKS_PER_S: usize = 240;

/// Rear-axle slip that counts as the start of a slide for the measurements, rad.
pub const ONSET_MEASURE_RAD: f32 = 0.02;

/// Kinematic quantities of the rig's car this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlideState {
    /// Forward speed, m/s.
    pub v_fwd: f32,
    /// Body slip at the CG, rad (+ = moving toward the left of the heading).
    pub body_slip: f32,
    /// Rear-axle slip angle, rad (see [`rear_axle_slip`]).
    pub rear_axle_slip: f32,
    /// Front axle's direction of travel, rad (see [`front_axle_angle`]).
    pub front_axle_angle: f32,
    /// Yaw rate, rad/s (+ = left).
    pub yaw_rate: f32,
    /// Centre road-wheel angle, rad (+ = left).
    pub steer: f32,
}

/// Read [`SlideState`] from a rig.
pub fn slide_state(rig: &Rig) -> SlideState {
    let ch = rig.vehicle.chassis();
    let up = ch.rot * V3::Z;
    let v_fwd = ch.lin_vel.dot(ch.rot * V3::Y);
    let v_left = -ch.lin_vel.dot(ch.rot * V3::X);
    let r = ch.ang_vel.dot(up);
    let def = rig.vehicle.def();
    let com = rig.vehicle.com_local();
    let body_slip = if v_fwd.abs() < 1.0 {
        0.0
    } else {
        atan2(v_left, v_fwd.abs())
    };
    SlideState {
        v_fwd,
        body_slip,
        rear_axle_slip: rear_axle_slip(v_fwd, v_left, r, com.y - def.wheels[2].position_m[1]),
        front_axle_angle: front_axle_angle(v_fwd, v_left, r, def.wheels[0].position_m[1] - com.y),
        yaw_rate: r,
        steer: rig.vehicle.telemetry().steer_rad,
    }
}

fn hold_speed(rig: &Rig, target: f32) -> (f32, f32) {
    let err = target - rig.state().speed_mps;
    if err > -1.0 {
        ((0.25 + 0.35 * err).clamp(0.0, 1.0), 0.0)
    } else {
        (0.0, (-0.2 * err).clamp(0.0, 1.0))
    }
}

/// What the driver does after the kick in [`kick_catch`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CatchDriver {
    /// Hands off: steering released, throttle 30 %.
    Released,
    /// Holds the countersteer key (full input toward the slide) from `react_s` after the
    /// onset while the slide grows or is past 6°, else releases it; throttle 30 %.
    Counter {
        /// Reaction time after the slide onset, s.
        react_s: f32,
    },
}

/// Result of [`kick_catch`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KickCatch {
    /// Fastest centre road-wheel slew toward the countersteer side in the first 0.4 s after
    /// the onset, °/s.
    pub cs_slew_dps: f32,
    /// Time from the slide onset until the wheels stay aligned with the front axle's travel
    /// (within 3°), s; `None` if they are still off after 2 s (or no slide started).
    pub onset_to_aligned_s: Option<f32>,
    /// Time from the slide onset until the wheels stay within 3° of where the steering is
    /// heading ([`crate::Vehicle::steer_target_for`]: the align target with released keys),
    /// s — the rack's response time.
    pub onset_to_target_s: Option<f32>,
    /// Largest misalignment of the wheels with the front axle's travel in the 2 s after the
    /// onset, degrees (only meaningful when caught: a spinning car's travel swings round).
    pub peak_align_err_deg: f32,
    /// Largest body slip, degrees.
    pub max_slip_deg: f32,
    /// Caught (see the module docs).
    pub caught: bool,
    /// Rig state hash at the end.
    pub hash: u64,
}

/// Straight at `speed_mps` with light throttle, then a yaw-rate kick of `yaw_kick` rad/s
/// (+ = nose to the left, a nudge from traffic); `driver` reacts for 4 s.
pub fn kick_catch(
    car: &LoadedCar,
    assists: &AssistProfile,
    driver: CatchDriver,
    speed_mps: f32,
    yaw_kick: f32,
) -> KickCatch {
    let mut rig = Rig::new(car);
    rig.teleport([0.0; 3], 0.0, speed_mps, 0.0);
    for _ in 0..TICKS_PER_S {
        let (throttle, brake) = hold_speed(&rig, speed_mps);
        rig.step(
            &VehicleInput {
                throttle,
                brake,
                ..Default::default()
            },
            assists,
        );
    }
    {
        let s = rig.state();
        let fwd = rig.vehicle.chassis().rot * V3::Y;
        let v = s.speed_mps;
        rig.teleport(s.pos, atan2(-fwd.x, fwd.y), v, yaw_kick);
    }
    let mut onset: Option<usize> = None;
    let mut last_off: Option<usize> = None;
    let mut last_off_target: Option<usize> = None;
    let mut peak_err = 0.0f32;
    let mut slew = 0.0f32;
    let mut max_slip = 0.0f32;
    let mut prev = slide_state(&rig);
    for tick in 0..4 * TICKS_PER_S {
        let s = slide_state(&rig);
        if onset.is_none() && s.rear_axle_slip.abs() > ONSET_MEASURE_RAD {
            onset = Some(tick);
        }
        let growing = s.body_slip.abs() > prev.body_slip.abs();
        let steer = match (driver, onset) {
            (CatchDriver::Counter { react_s }, Some(t0))
                if tick >= t0 + (react_s * TICKS_PER_S as f32) as usize =>
            {
                let deg = s.body_slip.to_degrees().abs();
                if deg > 2.0 && (growing || deg > 6.0) {
                    // Body slip < 0 = sliding toward the right: steer right (+).
                    -s.body_slip.signum()
                } else {
                    0.0
                }
            }
            _ => 0.0,
        };
        prev = s;
        rig.step(
            &VehicleInput {
                steer,
                throttle: 0.3,
                ..Default::default()
            },
            assists,
        );
        let n = slide_state(&rig);
        max_slip = max_slip.max(n.body_slip.to_degrees().abs());
        if let Some(t0) = onset {
            if tick < t0 + TICKS_PER_S * 4 / 10 {
                // Countersteer side = the side the rear axle slides to (sign of its slip).
                let side = s.rear_axle_slip.signum();
                slew = slew.max((n.steer - s.steer) * side / PHYSICS_DT);
            }
            let err = (n.steer - n.front_axle_angle).abs();
            let target = rig.vehicle.steer_target_for(steer, assists);
            if tick < t0 + 2 * TICKS_PER_S && (n.steer - target).abs() > 3f32.to_radians() {
                last_off_target = Some(tick + 1 - t0);
            }
            if tick < t0 + 2 * TICKS_PER_S {
                peak_err = peak_err.max(err);
                if err > 3f32.to_radians() {
                    last_off = Some(tick + 1 - t0);
                }
            }
        }
    }
    let end = slide_state(&rig);
    KickCatch {
        cs_slew_dps: slew.to_degrees(),
        onset_to_aligned_s: match (onset, last_off) {
            (None, _) => None,
            (Some(_), None) => Some(0.0),
            (Some(_), Some(t)) if t >= 2 * TICKS_PER_S => None,
            (Some(_), Some(t)) => Some(t as f32 * PHYSICS_DT),
        },
        peak_align_err_deg: peak_err.to_degrees(),
        onset_to_target_s: onset.map(|_| last_off_target.map_or(0.0, |t| t as f32 * PHYSICS_DT)),
        max_slip_deg: max_slip,
        caught: max_slip < 90.0
            && end.body_slip.to_degrees().abs() < 15.0
            && end.yaw_rate.abs() < 0.5
            && end.v_fwd > 8.0,
        hash: rig.state_hash(),
    }
}

/// Scripted drift drivers for [`drift_run`] (keyboard: steer and throttle are 0 / 1 keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftDriver {
    /// Holds the key into the turn and the throttle, tapping off the key when the slide angle
    /// gets past 30° and the throttle when it is past 35° (hold + modulate).
    Good,
    /// Holds the key into the turn and full throttle the whole time.
    Naive,
    /// Enters like [`DriftDriver::Naive`], then from 0.8 s on random 0.15–0.5 s segments
    /// (seeded [`Rng`]): key into the turn + throttle 40 %, released + throttle 20 %, against
    /// the slide + throttle 20 %, into the turn + lift 20 % — a driver who gets sideways and
    /// then saws at the keys.
    Sloppy,
    /// Like [`DriftDriver::Good`] for 2 s, then lifts off the throttle at once and keeps the
    /// key into the turn.
    AbruptLift,
    /// The car is flicked into a big angle (a [`FLICK_RPS`] yaw kick into the turn), then the
    /// key into the turn and full throttle: floors it at a big angle.
    Flood,
    /// The same flick as [`DriftDriver::Flood`], then the keys released and the throttle
    /// lifted: the contrast case.
    FlickLift,
}

/// Yaw kick of [`DriftDriver::Flood`] / [`DriftDriver::FlickLift`], rad/s.
pub const FLICK_RPS: f32 = 2.0;

/// Result of [`drift_run`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriftRun {
    /// Time spent drifting with 10–50° of body slip, s (of the 6 s run).
    pub drift_s: f32,
    /// Longest unbroken stretch with 10–50° of body slip, s.
    pub longest_s: f32,
    /// Largest body slip, degrees.
    pub max_slip_deg: f32,
    /// Largest body slip toward the other side (the tail swinging past straight: a snap),
    /// degrees.
    pub flip_deg: f32,
    /// Spun: past 90° of body slip, or travelling backwards.
    pub spun: bool,
    /// Rig state hash at the end.
    pub hash: u64,
}

/// A counter-clockwise (left-hand) corner of `radius` taken at `lat_g`, then the driver kicks
/// the throttle with the key into the turn and drifts for 6 s under `driver` (`seed` drives
/// [`DriftDriver::Sloppy`]). The circle follower settles the car first with analog steering.
pub fn drift_run(
    car: &LoadedCar,
    assists: &AssistProfile,
    driver: DriftDriver,
    radius: f32,
    lat_g: f32,
    seed: u64,
) -> DriftRun {
    drift_run_with(car, assists, driver, radius, lat_g, seed, |_, _, _| {})
}

/// [`drift_run`] calling `on_tick(rig, steer, throttle)` after every tick of the drift phase
/// (traces).
pub fn drift_run_with(
    car: &LoadedCar,
    assists: &AssistProfile,
    driver: DriftDriver,
    radius: f32,
    lat_g: f32,
    seed: u64,
    mut on_tick: impl FnMut(&Rig, f32, f32),
) -> DriftRun {
    let mut rig = Rig::new(car);
    let v = (lat_g * GRAVITY_MPS2 * radius).sqrt();
    rig.teleport([0.0; 3], 0.0, v, v / radius);
    let center = V3::new(-radius, 0.0, 0.0);
    // Settle on the circle with a 1:1 pure-pursuit follower.
    let settle = AssistProfile {
        steer_speed_sensitivity: 0.0,
        countersteer_help: 0.0,
        throttle_filter: 0.0,
        ..*assists
    };
    for _ in 0..2 * TICKS_PER_S {
        let ch = rig.vehicle.chassis();
        let d = ch.pos - center;
        let theta = atan2(d.y, d.x);
        let look = (0.8 * v).max(6.0);
        let (s, c) = wp_math::sin_cos(theta + look / radius);
        let to = center + V3::new(c * radius, s * radius, 0.0) - ch.pos;
        let (x, y) = (to.dot(ch.rot * V3::X), to.dot(ch.rot * V3::Y));
        let delta = atan(rig.vehicle.def().wheelbase_m * -2.0 * x / (x * x + y * y));
        let max = rig.vehicle.def().wheels[0].max_steer_rad;
        let (throttle, brake) = hold_speed(&rig, v);
        rig.step(
            &VehicleInput {
                steer: (-delta / max).clamp(-1.0, 1.0),
                throttle,
                brake,
                ..Default::default()
            },
            &settle,
        );
    }
    if matches!(driver, DriftDriver::Flood | DriftDriver::FlickLift) {
        let s = rig.state();
        let fwd = rig.vehicle.chassis().rot * V3::Y;
        rig.teleport(
            s.pos,
            atan2(-fwd.x, fwd.y),
            s.speed_mps,
            s.ang_vel[2] + FLICK_RPS,
        );
    }
    let mut rng = Rng::new(seed);
    let (mut sloppy_steer, mut sloppy_thr, mut sloppy_left) = (-1.0f32, 1.0f32, 0usize);
    let (mut drift, mut run, mut longest) = (0usize, 0usize, 0usize);
    let mut max_slip = 0.0f32;
    let mut flip = 0.0f32;
    let mut spun = false;
    let mut steer_key = -1.0f32;
    let mut thr_key = 1.0f32;
    for tick in 0..6 * TICKS_PER_S {
        let s = slide_state(&rig);
        // Left-hand corner: the slide angle is −body slip (velocity right of the heading).
        let angle = (-s.body_slip).to_degrees();
        // Into the turn = left = −1.
        let (steer, throttle) = match driver {
            DriftDriver::Naive | DriftDriver::Flood => (-1.0, 1.0),
            DriftDriver::FlickLift => (0.0, 0.0),
            DriftDriver::Good | DriftDriver::AbruptLift => {
                // Hysteresis taps, like a player modulating two keys.
                if angle > 30.0 {
                    steer_key = 0.0;
                } else if angle < 22.0 {
                    steer_key = -1.0;
                }
                if angle > 35.0 {
                    thr_key = 0.0;
                } else if angle < 25.0 {
                    thr_key = 1.0;
                }
                if driver == DriftDriver::AbruptLift && tick >= 2 * TICKS_PER_S {
                    (-1.0, 0.0)
                } else {
                    (steer_key, thr_key)
                }
            }
            DriftDriver::Sloppy if tick < TICKS_PER_S * 8 / 10 => (-1.0, 1.0),
            DriftDriver::Sloppy => {
                if sloppy_left == 0 {
                    sloppy_left = TICKS_PER_S * 15 / 100
                        + (rng.next_f32() * 0.35 * TICKS_PER_S as f32) as usize;
                    (sloppy_steer, sloppy_thr) = match rng.next_f32() {
                        r if r < 0.4 => (-1.0, 1.0),
                        r if r < 0.6 => (0.0, 1.0),
                        r if r < 0.8 => (1.0, 1.0),
                        _ => (-1.0, 0.0),
                    };
                }
                sloppy_left -= 1;
                (sloppy_steer, sloppy_thr)
            }
        };
        rig.step(
            &VehicleInput {
                steer,
                throttle,
                ..Default::default()
            },
            assists,
        );
        on_tick(&rig, steer, throttle);
        let n = slide_state(&rig);
        let a = n.body_slip.to_degrees().abs();
        max_slip = max_slip.max(a);
        // Left-hander: drifting is body slip < 0; > 0 is the tail past straight.
        flip = flip.max(n.body_slip.to_degrees());
        if a > 90.0 || n.v_fwd < 0.0 {
            spun = true;
        }
        if !spun && (10.0..=50.0).contains(&a) {
            drift += 1;
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    DriftRun {
        drift_s: drift as f32 * PHYSICS_DT,
        longest_s: longest as f32 * PHYSICS_DT,
        max_slip_deg: max_slip,
        flip_deg: flip,
        spun,
        hash: rig.state_hash(),
    }
}
