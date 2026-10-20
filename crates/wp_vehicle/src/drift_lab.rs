//! Drift Lab B / C measurements (owner rows 90 / 91 / 106): scripted slide drivers on a
//! [`Rig`], used by `examples/drift_lab_b.rs`, `tests/drift_lab_b.rs` and the numbers in
//! `docs/integration/drift-lab-b.md` / `drift-lab-c.md`.
//!
//! | measurement | manoeuvre |
//! |---|---|
//! | [`kick_catch`] | straight at speed, a yaw-rate kick; the driver releases the keys, or holds the countersteer key after a reaction time: countersteer slew, onset-to-aligned time, caught or spun |
//! | [`drift_run`] | a power-oversteer drift on a circle held by a scripted driver ([`DriftDriver`]): drift time inside the window, spun or not |
//! | [`handbrake_entry`] | straight at speed, key into the turn + a handbrake tap, then the countersteer key + throttle (row 108): release-to-aligned time, caught or spun |
//! | [`tap_response`] | straight at speed, one steering-key tap (row 98 / clip 2): peak wheel angle, peak yaw rate, heading change |
//! | [`hairpin`] | clip 2's handbrake hairpin: feint, key into the turn + handbrake 1 s, throttle, countersteer key + throttle: rotation, catch |
//! | [`transition_run`] | a left drift, a lift-off swing into a right drift (S-bend, row 107): transition time, overshoot, wobbles, spun or not |
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

/// With manual gears ([`AssistProfile::auto_gears`] off, the owner's reference) the scripted
/// drift drivers shift down to this gear for the drift, as a player would.
pub const DRIFT_GEAR: i8 = 2;

/// Gear-down key for a manual-gear drift driver: down until [`DRIFT_GEAR`].
fn drift_gear_down(rig: &Rig, assists: &AssistProfile) -> bool {
    !assists.auto_gears && rig.vehicle.gear() > DRIFT_GEAR
}

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
    // With manual gears the automatic box picks the cruising gear; the driver keeps it.
    let auto = AssistProfile {
        auto_gears: true,
        ..*assists
    };
    for _ in 0..TICKS_PER_S {
        let (throttle, brake) = hold_speed(&rig, speed_mps);
        rig.step(
            &VehicleInput {
                throttle,
                brake,
                ..Default::default()
            },
            &auto,
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
///
/// Drift Lab C (row 106): holding a slide is the countersteer key. [`DriftDriver::Good`],
/// [`DriftDriver::Naive`], [`DriftDriver::AbruptLift`] and [`DriftDriver::IntoTurn`] all
/// enter the same way — key into the turn + throttle until the slide passes
/// [`ENTRY_DEG`] — then do their own thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftDriver {
    /// After the entry, holds the countersteer key and modulates the throttle key (off past
    /// 32° of slide, back on below 25°); if the slide closes below 10° it taps the key into
    /// the turn (AngleUp) until it is past 15° again (hold + modulate).
    Good,
    /// After the entry, holds the countersteer key and full throttle the whole time.
    Naive,
    /// Enters like [`DriftDriver::Naive`], then from 0.8 s on random 0.15–0.5 s segments
    /// (seeded [`Rng`]): key into the turn + throttle 40 %, released + throttle 20 %, against
    /// the slide + throttle 20 %, into the turn + lift 20 % — a driver who gets sideways and
    /// then saws at the keys.
    Sloppy,
    /// Like [`DriftDriver::Good`] for 2 s, then lifts off the throttle at once and keeps the
    /// countersteer key.
    AbruptLift,
    /// The car is flicked into a big angle (a [`FLICK_RPS`] yaw kick into the turn), then the
    /// key into the turn and full throttle: floors it at a big angle.
    Flood,
    /// The same flick as [`DriftDriver::Flood`], then the keys released and the throttle
    /// lifted: the contrast case.
    FlickLift,
    /// After the entry, keeps the key into the turn (AngleUp: more angle, a tighter line)
    /// with [`DriftDriver::Good`]'s throttle modulation.
    IntoTurn,
}

/// Slide angle where the scripted drivers switch from the entry (key into the turn +
/// throttle) to their own inputs, degrees.
pub const ENTRY_DEG: f32 = 12.0;

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
    /// Mean slide angle while drifting (10–50°), degrees.
    pub mean_deg: f32,
    /// Steadiness: standard deviation of the slide angle while drifting (10–50°), degrees
    /// (small = a steady drift).
    pub sd_deg: f32,
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

/// A rig settled on a counter-clockwise circle of `radius` at `lat_g` (2 s of a 1:1
/// pure-pursuit follower with analog steering).
fn settle_on_circle(car: &LoadedCar, assists: &AssistProfile, radius: f32, lat_g: f32) -> Rig {
    let mut rig = Rig::new(car);
    let v = (lat_g * GRAVITY_MPS2 * radius).sqrt();
    rig.teleport([0.0; 3], 0.0, v, v / radius);
    let center = V3::new(-radius, 0.0, 0.0);
    // Settle on the circle with a 1:1 pure-pursuit follower.
    // The automatic box picks the gear while settling; with manual gears the driver then
    // holds it through the drift (as a player would).
    let settle = AssistProfile {
        steer_speed_sensitivity: 0.0,
        countersteer_help: 0.0,
        throttle_filter: 0.0,
        auto_gears: true,
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
    rig
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
    let mut rig = settle_on_circle(car, assists, radius, lat_g);
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
    let (mut sum, mut sum2) = (0.0f32, 0.0f32);
    let mut thr_key = 1.0f32;
    let mut entered = false;
    let mut regain = false;
    for tick in 0..6 * TICKS_PER_S {
        let s = slide_state(&rig);
        // Left-hand corner: the slide angle is −body slip (velocity right of the heading).
        let angle = (-s.body_slip).to_degrees();
        entered |= angle > ENTRY_DEG;
        // Throttle key with hysteresis, like a player modulating it.
        if angle > 32.0 {
            thr_key = 0.0;
        } else if angle < 25.0 {
            thr_key = 1.0;
        }
        if angle < 10.0 {
            regain = true;
        } else if angle > 15.0 {
            regain = false;
        }
        // The good driver's steering: the countersteer key, or into the turn to regain angle.
        let good = if regain { -1.0 } else { 1.0 };
        // Into the turn = left = −1; the countersteer key = right = +1.
        let (steer, throttle) = match driver {
            DriftDriver::Flood => (-1.0, 1.0),
            DriftDriver::FlickLift => (0.0, 0.0),
            DriftDriver::Good
            | DriftDriver::Naive
            | DriftDriver::AbruptLift
            | DriftDriver::IntoTurn
                if !entered =>
            {
                (-1.0, 1.0)
            }
            DriftDriver::Naive => (1.0, 1.0),
            DriftDriver::Good => (good, thr_key),
            DriftDriver::AbruptLift if tick >= 2 * TICKS_PER_S => (1.0, 0.0),
            DriftDriver::AbruptLift => (good, thr_key),
            DriftDriver::IntoTurn => (-1.0, thr_key),
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
                gear_down: drift_gear_down(&rig, assists),
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
            sum += a;
            sum2 += a * a;
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    let (mean, sd) = if drift > 0 {
        let m = sum / drift as f32;
        (m, (sum2 / drift as f32 - m * m).max(0.0).sqrt())
    } else {
        (0.0, 0.0)
    };
    DriftRun {
        mean_deg: mean,
        sd_deg: sd,
        drift_s: drift as f32 * PHYSICS_DT,
        longest_s: longest as f32 * PHYSICS_DT,
        max_slip_deg: max_slip,
        flip_deg: flip,
        spun,
        hash: rig.state_hash(),
    }
}

/// Result of [`transition_run`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransitionRun {
    /// Time from the lift until the slide is past 10° on the new side, s (`None`: it never
    /// got there).
    pub transition_s: Option<f32>,
    /// Largest slide angle on the new side in the 1.5 s after the swap, degrees (overshoot).
    pub peak_new_deg: f32,
    /// Largest yaw rate toward the new side during the swap (lift + 1.5 s), rad/s.
    pub peak_yaw_rps: f32,
    /// Slide-direction reversals after the swap (past 5° each way): 0 = a clean swap, more
    /// = a tank-slapper.
    pub wobbles: u32,
    /// Time drifting (10–50°) on the new side after the swap, s.
    pub new_drift_s: f32,
    /// Spun: past 90° of body slip, or travelling backwards.
    pub spun: bool,
    /// Rig state hash at the end.
    pub hash: u64,
}

/// Lift-off transition (owner row 107) on an S-bend: a left-hand drift on a 40 m-style circle
/// (`radius`, `lat_g`) held 2.5 s by the good driver (entry key into the turn, then the
/// countersteer key + modulated throttle), then the flick: a lift with the keys released for
/// 0.15 s, then still off the throttle the key toward the new (right-hand) turn until the slide
/// has swung past 2° the other way (at most 1 s), then that key + throttle until 12°, then the
/// new countersteer key + modulated throttle, for 4 s.
pub fn transition_run(
    car: &LoadedCar,
    assists: &AssistProfile,
    radius: f32,
    lat_g: f32,
) -> TransitionRun {
    transition_run_with(car, assists, radius, lat_g, |_, _, _| {})
}

/// [`transition_run`] calling `on_tick(rig, steer, throttle)` after every tick (traces).
pub fn transition_run_with(
    car: &LoadedCar,
    assists: &AssistProfile,
    radius: f32,
    lat_g: f32,
    mut on_tick: impl FnMut(&Rig, f32, f32),
) -> TransitionRun {
    let mut rig = settle_on_circle(car, assists, radius, lat_g);
    let lift_tick = TICKS_PER_S * 5 / 2;
    // Phases: 0 left drift, 1 lift + released, 2 into the new turn, 3 new countersteer.
    let mut phase = 0;
    let mut phase_ticks = 0usize;
    let (mut entered, mut regain, mut thr_key) = (false, false, 1.0f32);
    let mut swap_tick: Option<usize> = None;
    let mut peak_new = 0.0f32;
    let mut peak_yaw = 0.0f32;
    let mut wobbles = 0u32;
    let mut last_side = 0.0f32;
    let mut new_drift = 0usize;
    let mut spun = false;
    for tick in 0..(lift_tick + 4 * TICKS_PER_S) {
        let s = slide_state(&rig);
        // Left drift: slide angle = −body slip; the new (right-hand) drift: +body slip.
        let left = (-s.body_slip).to_degrees();
        let right = s.body_slip.to_degrees();
        let angle = if phase == 0 { left } else { right };
        if angle > 32.0 {
            thr_key = 0.0;
        } else if angle < 25.0 {
            thr_key = 1.0;
        }
        if angle < 10.0 {
            regain = true;
        } else if angle > 15.0 {
            regain = false;
        }
        phase_ticks += 1;
        if phase == 0 && tick >= lift_tick {
            phase = 1;
            phase_ticks = 0;
        } else if phase == 1 && (right > 2.0 || phase_ticks >= TICKS_PER_S) {
            phase = 2;
            phase_ticks = 0;
        } else if phase == 2 && right > ENTRY_DEG {
            phase = 3;
            phase_ticks = 0;
            regain = false;
        }
        // Keys: left = −1, right = +1. Left drift: into = −1, counter = +1; right drift:
        // into = +1, counter = −1.
        let (steer, throttle) = match phase {
            0 => {
                entered |= left > ENTRY_DEG;
                if !entered {
                    (-1.0, 1.0)
                } else if regain {
                    (-1.0, thr_key)
                } else {
                    (1.0, thr_key)
                }
            }
            1 if phase_ticks < TICKS_PER_S * 15 / 100 => (0.0, 0.0),
            1 => (1.0, 0.0),
            2 => (1.0, 1.0),
            _ => (if regain { 1.0 } else { -1.0 }, thr_key),
        };
        rig.step(
            &VehicleInput {
                steer,
                throttle,
                gear_down: drift_gear_down(&rig, assists),
                ..Default::default()
            },
            assists,
        );
        on_tick(&rig, steer, throttle);
        let n = slide_state(&rig);
        let deg = n.body_slip.to_degrees();
        if n.body_slip.abs().to_degrees() > 90.0 || n.v_fwd < 0.0 {
            spun = true;
        }
        if tick >= lift_tick {
            if swap_tick.is_none() && deg > 10.0 {
                swap_tick = Some(tick);
            }
            let within = swap_tick.is_none_or(|t0| tick < t0 + TICKS_PER_S * 3 / 2);
            if within {
                // Yaw toward the new (right-hand) side is negative.
                peak_yaw = peak_yaw.max(-n.yaw_rate);
            }
            if let Some(t0) = swap_tick {
                if tick < t0 + TICKS_PER_S * 3 / 2 {
                    peak_new = peak_new.max(deg);
                }
                let side = if deg > 5.0 {
                    1.0
                } else if deg < -5.0 {
                    -1.0
                } else {
                    0.0
                };
                if side != 0.0 {
                    if last_side != 0.0 && side != last_side {
                        wobbles += 1;
                    }
                    last_side = side;
                }
                if !spun && (10.0..=50.0).contains(&deg) {
                    new_drift += 1;
                }
            }
        }
    }
    TransitionRun {
        transition_s: swap_tick.map(|t| (t - lift_tick) as f32 * PHYSICS_DT),
        peak_new_deg: peak_new,
        peak_yaw_rps: peak_yaw,
        wobbles,
        new_drift_s: new_drift as f32 * PHYSICS_DT,
        spun,
        hash: rig.state_hash(),
    }
}

/// Result of [`handbrake_entry`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HandbrakeEntry {
    /// Time from the handbrake release until the wheels first reach the front axle's travel
    /// (within 3°), s (`None`: not within 1.5 s).
    pub release_to_aligned_s: Option<f32>,
    /// Fastest centre road-wheel slew toward the countersteer side in the 0.4 s after the
    /// release, °/s.
    pub cs_slew_dps: f32,
    /// Largest body slip, degrees.
    pub max_slip_deg: f32,
    /// Time drifting (10–50° of body slip) after the release, s.
    pub drift_s: f32,
    /// Caught: never past 90° of body slip and never backwards (the car is driving on, in a
    /// drift or straight).
    pub caught: bool,
    /// Rig state hash at the end.
    pub hash: u64,
}

/// Handbrake entry (owner row 108): straight at `speed_mps`, then the key into a left turn +
/// the handbrake for `tap_s` (throttle off), then the handbrake released and — `counter` —
/// the countersteer key (released once the slide is under 5°), or — not `counter` — the
/// steering keys released (the self-aligning catch); throttle key off past 32° of slide, on
/// below 25°; for 3 s.
pub fn handbrake_entry(
    car: &LoadedCar,
    assists: &AssistProfile,
    speed_mps: f32,
    tap_s: f32,
    counter: bool,
) -> HandbrakeEntry {
    handbrake_entry_with(car, assists, speed_mps, tap_s, counter, |_, _, _| {})
}

/// [`handbrake_entry`] calling `on_tick(rig, steer, throttle)` after every tick (traces).
pub fn handbrake_entry_with(
    car: &LoadedCar,
    assists: &AssistProfile,
    speed_mps: f32,
    tap_s: f32,
    counter: bool,
    mut on_tick: impl FnMut(&Rig, f32, f32),
) -> HandbrakeEntry {
    let mut rig = Rig::new(car);
    rig.teleport([0.0; 3], 0.0, speed_mps, 0.0);
    let auto = AssistProfile {
        auto_gears: true,
        ..*assists
    };
    for _ in 0..TICKS_PER_S {
        let (throttle, brake) = hold_speed(&rig, speed_mps);
        rig.step(
            &VehicleInput {
                throttle,
                brake,
                ..Default::default()
            },
            &auto,
        );
    }
    let tap = (tap_s * TICKS_PER_S as f32) as usize;
    let mut thr_key = 1.0f32;
    let (mut aligned, mut slew, mut max_slip) = (None, 0.0f32, 0.0f32);
    let mut drift = 0usize;
    let mut spun = false;
    let mut prev = slide_state(&rig);
    for tick in 0..tap + 3 * TICKS_PER_S {
        let s = slide_state(&rig);
        // Left turn: slide angle = −body slip; the countersteer key is right (+1).
        let angle = (-s.body_slip).to_degrees();
        if angle > 32.0 {
            thr_key = 0.0;
        } else if angle < 25.0 {
            thr_key = 1.0;
        }
        let input = if tick < tap {
            VehicleInput {
                steer: -1.0,
                handbrake: 1.0,
                ..Default::default()
            }
        } else {
            VehicleInput {
                steer: if counter && angle > 5.0 { 1.0 } else { 0.0 },
                throttle: thr_key,
                ..Default::default()
            }
        };
        rig.step(&input, assists);
        on_tick(&rig, input.steer, input.throttle);
        let n = slide_state(&rig);
        let deg = n.body_slip.to_degrees().abs();
        max_slip = max_slip.max(deg);
        if deg > 90.0 || n.v_fwd < 0.0 {
            spun = true;
        }
        if tick >= tap {
            let since = tick - tap;
            if since < TICKS_PER_S * 4 / 10 {
                let side = prev.rear_axle_slip.signum();
                slew = slew.max((n.steer - prev.steer) * side / PHYSICS_DT);
            }
            if aligned.is_none()
                && since < TICKS_PER_S * 3 / 2
                && (n.steer - n.front_axle_angle).abs() < 3f32.to_radians()
            {
                aligned = Some((since + 1) as f32 * PHYSICS_DT);
            }
            if !spun && (10.0..=50.0).contains(&deg) {
                drift += 1;
            }
        }
        prev = n;
    }
    HandbrakeEntry {
        release_to_aligned_s: aligned,
        cs_slew_dps: slew.to_degrees(),
        max_slip_deg: max_slip,
        drift_s: drift as f32 * PHYSICS_DT,
        caught: !spun,
        hash: rig.state_hash(),
    }
}

/// Result of [`tap_response`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TapResponse {
    /// Largest centre road-wheel angle, degrees.
    pub peak_wheel_deg: f32,
    /// Largest yaw rate, degrees per second.
    pub peak_yaw_dps: f32,
    /// Heading change 1.5 s after the tap started, degrees.
    pub heading_deg: f32,
    /// Largest body slip, degrees.
    pub max_slip_deg: f32,
}

/// One steering-key tap (owner clip 2: racing steering is mostly 0.1–0.25 s taps): straight
/// at `speed_mps` holding speed, then the left key for `tap_s` (throttle held as before),
/// measured for 1.5 s.
pub fn tap_response(
    car: &LoadedCar,
    assists: &AssistProfile,
    speed_mps: f32,
    tap_s: f32,
) -> TapResponse {
    let mut rig = Rig::new(car);
    rig.teleport([0.0; 3], 0.0, speed_mps, 0.0);
    let auto = AssistProfile {
        auto_gears: true,
        ..*assists
    };
    for _ in 0..TICKS_PER_S {
        let (throttle, brake) = hold_speed(&rig, speed_mps);
        rig.step(
            &VehicleInput {
                throttle,
                brake,
                ..Default::default()
            },
            &auto,
        );
    }
    let heading = |rig: &Rig| {
        let f = rig.vehicle.chassis().rot * V3::Y;
        atan2(-f.x, f.y)
    };
    let h0 = heading(&rig);
    let (throttle, _) = hold_speed(&rig, speed_mps);
    let tap = (tap_s * TICKS_PER_S as f32) as usize;
    let (mut wheel, mut yaw, mut slip) = (0.0f32, 0.0f32, 0.0f32);
    for tick in 0..TICKS_PER_S * 3 / 2 {
        rig.step(
            &VehicleInput {
                steer: if tick < tap { -1.0 } else { 0.0 },
                throttle,
                ..Default::default()
            },
            assists,
        );
        let n = slide_state(&rig);
        wheel = wheel.max(n.steer.abs());
        yaw = yaw.max(n.yaw_rate.abs());
        slip = slip.max(n.body_slip.abs());
    }
    TapResponse {
        peak_wheel_deg: wheel.to_degrees(),
        peak_yaw_dps: yaw.to_degrees(),
        heading_deg: (heading(&rig) - h0).to_degrees(),
        max_slip_deg: slip.to_degrees(),
    }
}

/// Result of [`hairpin`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hairpin {
    /// Heading change while the handbrake is held, degrees.
    pub pivot_deg: f32,
    /// Heading change from the feint to the end, degrees.
    pub total_deg: f32,
    /// Time from the countersteer key until the car is caught (body slip < 15° and |yaw
    /// rate| < 0.5 rad/s), s (`None`: not within the run).
    pub catch_s: Option<f32>,
    /// Speed at the end, km/h.
    pub exit_kmh: f32,
    /// Spun: travelling backwards after the catch phase started, or past 120° of slip then.
    pub spun: bool,
}

/// Owner clip 2's handbrake hairpin, scripted: approach at `speed_mps` (holding speed),
/// a 0.3 s feint (the key away from the turn, throttle on), then the key into a right-hand
/// turn + handbrake for 1.0 s with the throttle off, then the throttle on still holding the
/// key into the turn for 0.8 s, then the countersteer key + throttle until caught (at most
/// 2 s) and the keys released + throttle for 1 s.
pub fn hairpin(car: &LoadedCar, assists: &AssistProfile, speed_mps: f32) -> Hairpin {
    let mut rig = Rig::new(car);
    rig.teleport([0.0; 3], 0.0, speed_mps, 0.0);
    let auto = AssistProfile {
        auto_gears: true,
        ..*assists
    };
    for _ in 0..TICKS_PER_S {
        let (throttle, brake) = hold_speed(&rig, speed_mps);
        rig.step(
            &VehicleInput {
                throttle,
                brake,
                ..Default::default()
            },
            &auto,
        );
    }
    let heading = |rig: &Rig| {
        let f = rig.vehicle.chassis().rot * V3::Y;
        atan2(-f.x, f.y)
    };
    let unwrap = |d: f32| {
        if d > std::f32::consts::PI {
            d - std::f32::consts::TAU
        } else if d < -std::f32::consts::PI {
            d + std::f32::consts::TAU
        } else {
            d
        }
    };
    let (feint, pivot, power) = (TICKS_PER_S * 3 / 10, TICKS_PER_S, TICKS_PER_S * 8 / 10);
    let mut prev = heading(&rig);
    let (mut total, mut pivot_deg) = (0.0f32, 0.0f32);
    let mut catch_start: Option<usize> = None;
    let mut caught: Option<usize> = None;
    let mut spun = false;
    let end = feint + pivot + power + 3 * TICKS_PER_S;
    for tick in 0..end {
        let s = slide_state(&rig);
        // Right-hand turn: key into the turn = right (+1); the countersteer key = left (−1).
        let input = if tick < feint {
            VehicleInput {
                steer: -1.0,
                throttle: 0.5,
                ..Default::default()
            }
        } else if tick < feint + pivot {
            VehicleInput {
                steer: 1.0,
                handbrake: 1.0,
                ..Default::default()
            }
        } else if tick < feint + pivot + power {
            VehicleInput {
                steer: 1.0,
                throttle: 1.0,
                ..Default::default()
            }
        } else if caught.is_none() && tick < feint + pivot + power + 2 * TICKS_PER_S {
            catch_start.get_or_insert(tick);
            VehicleInput {
                steer: -1.0,
                throttle: 1.0,
                ..Default::default()
            }
        } else {
            VehicleInput {
                throttle: 1.0,
                ..Default::default()
            }
        };
        rig.step(&input, assists);
        let h = heading(&rig);
        let d = unwrap(h - prev);
        prev = h;
        total += d;
        if (feint..feint + pivot).contains(&tick) {
            pivot_deg += d;
        }
        let n = slide_state(&rig);
        if let Some(t0) = catch_start {
            if n.v_fwd < -1.0 || n.body_slip.abs().to_degrees() > 120.0 {
                spun = true;
            }
            if caught.is_none()
                && tick > t0
                && n.body_slip.abs().to_degrees() < 15.0
                && n.yaw_rate.abs() < 0.5
                && n.v_fwd > 2.0
            {
                caught = Some(tick);
            }
        }
        let _ = s;
    }
    Hairpin {
        // Right-hand turn: heading decreases.
        pivot_deg: -pivot_deg.to_degrees(),
        total_deg: -total.to_degrees(),
        catch_s: match (catch_start, caught) {
            (Some(a), Some(b)) => Some((b - a) as f32 * PHYSICS_DT),
            _ => None,
        },
        exit_kmh: rig.state().speed_mps * 3.6,
        spun,
    }
}
