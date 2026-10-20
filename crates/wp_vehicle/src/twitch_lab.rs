//! Keyboard twitch measurements (owner row 102, "the car still feels horridly twitchy"):
//! a closed-loop keyboard driver on a [`Rig`] through grip manoeuvres, scored on how busy the
//! rack is. Used by `examples/drift_twitch.rs` and the tables in
//! `docs/integration/drift-twitch.md`.
//!
//! The file only uses the crate's public rig / chassis / telemetry API, so the same file runs
//! on older branches (copied in as an example) for the before/after comparison.
//!
//! | case | manoeuvre |
//! |---|---|
//! | [`Case::LaneChange`] | straight, then a 3.5 m lane change to the left in 1.6 s, then straight |
//! | [`Case::Slalom`] | cones 18 m apart, ±1.5 m, 60 km/h |
//! | [`Case::Sweeper`] | straight into an 80 m left-hand arc at 70 km/h, 70 % grip |
//! | [`Case::Bump`] | straight at 100 km/h, a kerb hit (yaw, roll and lateral kick) after 1 s |
//!
//! The driver ([`KeyDriver`]): pure pursuit on the path (lookahead 0.9 s, min 8 m) with an
//! 80 ms reaction delay gives a wanted steer −1..1; a sigma-delta turns it into key presses in
//! 50 ms slots (a human tapping keys); the keys go through the testbed's keyboard ramp
//! ([`KeyRamp`], 60 Hz frames) and then to the sim. The throttle / brake hold the speed.
//!
//! Metrics (all over the scored window, see [`Twitch`]):
//! - *reversals/s*: the road-wheel angle changing direction by more than
//!   [`REVERSAL_DEG`] from its last extreme;
//! - *peak rack rate*: fastest road-wheel angle rate, °/s;
//! - *fast slew while not sliding*: share of ticks with the rack faster than [`FAST_DPS`]
//!   while the rear axle is not sliding (|rear-axle slip| < [`NOT_SLIDING_RAD`]);
//! - *yaw overshoot*: largest |yaw rate − path yaw rate| (speed × path curvature), °/s;
//! - *settling*: time from the end of the manoeuvre (lane change end, arc entry, kerb hit)
//!   until the yaw rate stays within 3 °/s of the path's, s.

use wp_contracts::{AssistProfile, HandlingEffects, LoadedCar, VehicleInput, PHYSICS_DT};

use crate::math::{atan2, V3};
use crate::rig::Rig;

const TICKS_PER_S: usize = 240;
/// Steering direction change that counts as a reversal, degrees of road-wheel angle.
pub const REVERSAL_DEG: f32 = 0.5;
/// Rack rate that counts as a fast slew (a snap), °/s. `origin/main`'s keyboard rack never
/// passes ~190 °/s in these cases; Drift Lab B's fast countersteer runs at 350–400 °/s.
pub const FAST_DPS: f32 = 150.0;
/// Below this rear-axle slip the car counts as not sliding, rad (2.9°).
pub const NOT_SLIDING_RAD: f32 = 0.05;
/// Yaw-rate band for settling, rad/s (3 °/s).
pub const SETTLE_RPS: f32 = 0.052;

/// The testbed's keyboard steer ramp (`apps/testbed-web/src/input.ts`): the input moves toward
/// the key target at `up` per second (away from centre) or `down` (back toward it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyRamp {
    /// Rate away from centre, 1/s.
    pub up: f32,
    /// Rate back toward centre, 1/s.
    pub down: f32,
    /// A reversal (key against the current input) starts from centre at once.
    pub reverse_from_centre: bool,
}

impl KeyRamp {
    /// `origin/main` and Drift Lab B: 4/s out, 6/s back, reversals ramp through centre.
    pub const MAIN: KeyRamp = KeyRamp {
        up: 4.0,
        down: 6.0,
        reverse_from_centre: false,
    };
    /// Machine A's row 90 (`lead/round12`): reversal from centre, 12/s release.
    pub const ROW90: KeyRamp = KeyRamp {
        up: 4.0,
        down: 12.0,
        reverse_from_centre: true,
    };

    /// One frame of the ramp (the `ramp` function of input.ts).
    pub fn step(&self, cur: f32, target: f32, dt: f32) -> f32 {
        let mut cur = cur;
        if self.reverse_from_centre
            && target != 0.0
            && cur != 0.0
            && target.signum() != cur.signum()
        {
            cur = 0.0;
        }
        let rate = if target.abs() > cur.abs() || target.signum() != cur.signum() {
            self.up
        } else {
            self.down
        };
        let d = target - cur;
        let step = rate * dt;
        if d.abs() <= step {
            target
        } else {
            cur + d.signum() * step
        }
    }
}

/// A grip manoeuvre.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Case {
    /// 3.5 m lane change to the left in 1.6 s at `kmh`.
    LaneChange {
        /// Speed, km/h.
        kmh: f32,
    },
    /// Cones 18 m apart (±1.5 m) at 60 km/h.
    Slalom,
    /// 80 m left-hand arc at 70 km/h on 70 % grip.
    Sweeper,
    /// Kerb hit at 100 km/h: after 1 s the car gets +0.2 rad/s yaw, 0.5 m/s sideways and
    /// 0.6 rad/s roll at once, then the driver holds the lane.
    Bump,
}

impl Case {
    /// The five standard cases.
    pub const ALL: [Case; 5] = [
        Case::LaneChange { kmh: 80.0 },
        Case::LaneChange { kmh: 130.0 },
        Case::Slalom,
        Case::Sweeper,
        Case::Bump,
    ];

    /// Short name for tables.
    pub fn name(&self) -> String {
        match self {
            Case::LaneChange { kmh } => format!("lane change {kmh:.0}"),
            Case::Slalom => "slalom 60".into(),
            Case::Sweeper => "sweeper 70 % grip".into(),
            Case::Bump => "kerb hit 100".into(),
        }
    }

    fn speed(&self) -> f32 {
        match self {
            Case::LaneChange { kmh } => kmh / 3.6,
            Case::Slalom => 60.0 / 3.6,
            Case::Sweeper => 70.0 / 3.6,
            Case::Bump => 100.0 / 3.6,
        }
    }

    /// Path point at distance `s` along it, (x right, y forward), m.
    fn point(&self, s: f32) -> (f32, f32) {
        let start = 30.0;
        match self {
            Case::LaneChange { .. } => {
                let len = self.speed() * 1.6;
                let t = ((s - start) / len).clamp(0.0, 1.0);
                let x = -3.5 * 0.5 * (1.0 - (std::f32::consts::PI * t).cos());
                (x, s)
            }
            Case::Slalom => {
                // Amplitude eased in over the first gate so the path has no kink.
                let u = ((s - start) / 18.0).clamp(0.0, 1.0);
                let ease = u * u * (3.0 - 2.0 * u);
                let x = 1.5 * ease * (std::f32::consts::PI * (s - start) / 18.0).sin();
                (x, s)
            }
            Case::Sweeper => {
                if s < start {
                    (0.0, s)
                } else {
                    let r = 80.0;
                    let a = (s - start) / r;
                    // Left-hand: centre at (−r, start).
                    (-r + r * a.cos(), start + r * a.sin())
                }
            }
            Case::Bump => (0.0, s),
        }
    }

    /// Path yaw rate at `s` for speed `v`, rad/s (+ = left).
    fn path_yaw_rate(&self, s: f32, v: f32) -> f32 {
        let h = 0.5;
        let (x0, y0) = self.point(s - h);
        let (x1, y1) = self.point(s);
        let (x2, y2) = self.point(s + h);
        let a0 = atan2(y1 - y0, x1 - x0);
        let a1 = atan2(y2 - y1, x2 - x1);
        let mut da = a1 - a0;
        if da > std::f32::consts::PI {
            da -= 2.0 * std::f32::consts::PI;
        } else if da < -std::f32::consts::PI {
            da += 2.0 * std::f32::consts::PI;
        }
        da / h * v
    }

    /// Distance along the path where the manoeuvre "ends" (settling starts), m; `None` for
    /// the slalom (it never ends).
    fn settle_from_s(&self) -> Option<f32> {
        match self {
            Case::LaneChange { .. } => Some(30.0 + self.speed() * 1.6),
            Case::Sweeper => Some(30.0),
            Case::Slalom | Case::Bump => None,
        }
    }

    fn duration_s(&self) -> f32 {
        match self {
            Case::LaneChange { .. } => 5.0,
            Case::Slalom => 8.0,
            Case::Sweeper => 7.0,
            Case::Bump => 4.0,
        }
    }
}

/// Closed-loop keyboard driver: pure pursuit → reaction delay → sigma-delta key presses in
/// 50 ms slots → keyboard ramp at 60 Hz.
#[derive(Debug, Clone)]
pub struct KeyDriver {
    ramp: KeyRamp,
    delay: Vec<f32>,
    head: usize,
    sigma: f32,
    key: f32,
    input: f32,
}

/// Reaction delay of [`KeyDriver`], ticks (80 ms).
pub const REACTION_TICKS: usize = 19;
/// Key slot of [`KeyDriver`], ticks (50 ms).
pub const KEY_SLOT_TICKS: usize = 12;

impl KeyDriver {
    /// A driver using the keyboard ramp `ramp`.
    pub fn new(ramp: KeyRamp) -> Self {
        Self {
            ramp,
            delay: vec![0.0; REACTION_TICKS],
            head: 0,
            sigma: 0.0,
            key: 0.0,
            input: 0.0,
        }
    }

    /// Advance one tick with this tick's wanted steer (−1..1, + = right); returns the steer
    /// input sent to the sim.
    pub fn step(&mut self, tick: usize, wanted: f32) -> f32 {
        let delayed = self.delay[self.head];
        self.delay[self.head] = wanted;
        self.head = (self.head + 1) % self.delay.len();
        if tick.is_multiple_of(KEY_SLOT_TICKS) {
            // Small errors are ignored (a human does not correct 3 % of lock).
            let w = if delayed.abs() < 0.05 {
                0.0
            } else {
                delayed.clamp(-1.0, 1.0)
            };
            self.sigma = (self.sigma + w - self.key).clamp(-2.0, 2.0);
            self.key = if self.sigma > 0.5 {
                1.0
            } else if self.sigma < -0.5 {
                -1.0
            } else {
                0.0
            };
        }
        if tick.is_multiple_of(4) {
            self.input = self.ramp.step(self.input, self.key, 1.0 / 60.0);
        }
        self.input
    }
}

/// Twitch metrics of one run (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Twitch {
    /// Steering reversals per second.
    pub reversals_ps: f32,
    /// Fastest road-wheel angle rate, °/s.
    pub peak_rate_dps: f32,
    /// Share of ticks with the rack faster than [`FAST_DPS`] while not sliding, %.
    pub fast_not_sliding_pct: f32,
    /// Largest |yaw rate − path yaw rate|, °/s.
    pub yaw_overshoot_dps: f32,
    /// Settling time, s (`None`: no settle point for this case, or never settled).
    pub settle_s: Option<f32>,
    /// RMS lateral path error, m.
    pub rms_err_m: f32,
    /// Largest |rear-axle slip|, degrees.
    pub max_rear_slip_deg: f32,
}

fn rear_axle_slip(rig: &Rig) -> (f32, f32, f32) {
    let ch = rig.vehicle.chassis();
    let up = ch.rot * V3::Z;
    let v_fwd = ch.lin_vel.dot(ch.rot * V3::Y);
    let v_left = -ch.lin_vel.dot(ch.rot * V3::X);
    let r = ch.ang_vel.dot(up);
    let def = rig.vehicle.def();
    let cg_to_rear = rig.vehicle.com_local().y - def.wheels[2].position_m[1];
    let slip = if v_fwd.abs() < 1.0 {
        0.0
    } else {
        atan2(v_left - r * cg_to_rear, v_fwd.abs())
    };
    (slip, r, v_fwd)
}

fn hold_speed(rig: &Rig, target: f32) -> (f32, f32) {
    let err = target - rig.state().speed_mps;
    if err > -1.0 {
        ((0.3 + 0.35 * err).clamp(0.0, 1.0), 0.0)
    } else {
        (0.0, (-0.2 * err).clamp(0.0, 1.0))
    }
}

/// Run `case` with `assists` and keyboard ramp `ramp`; `observe` sees the rig after every
/// scored tick (the Drift Lab uses it to log the steering's own state).
pub fn twitch_run_with(
    car: &LoadedCar,
    assists: &AssistProfile,
    ramp: KeyRamp,
    case: Case,
    mut observe: impl FnMut(&Rig),
) -> Twitch {
    let mut rig = Rig::new(car);
    let v = case.speed();
    if case == Case::Sweeper {
        rig.set_effects(&HandlingEffects {
            grip_scale: [0.7; 4],
            ..HandlingEffects::NEUTRAL
        });
    }
    rig.teleport([0.0; 3], 0.0, v, 0.0);
    // Settle the speed on the straight (no steering).
    for _ in 0..TICKS_PER_S {
        let (throttle, brake) = hold_speed(&rig, v);
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
        let st = rig.state();
        rig.teleport([0.0, 0.0, st.pos[2]], 0.0, st.speed_mps, 0.0);
    }
    let wheelbase = rig.vehicle.def().wheelbase_m;
    let grip = if case == Case::Sweeper { 0.7 } else { 1.0 };
    let mut drv = KeyDriver::new(ramp);
    let mut s_car = 0.0f32;
    let mut prev_steer = rig.vehicle.telemetry().steer_rad;
    let (mut extreme, mut dir) = (prev_steer, 0.0f32);
    let mut reversals = 0usize;
    let (mut peak_rate, mut fast_ticks, mut overshoot) = (0.0f32, 0usize, 0.0f32);
    let (mut sum_err2, mut max_rear) = (0.0f32, 0.0f32);
    let mut last_unsettled: Option<usize> = None;
    let mut settle_start: Option<usize> = None;
    let ticks = (case.duration_s() * TICKS_PER_S as f32) as usize;
    for tick in 0..ticks {
        if case == Case::Bump && tick == TICKS_PER_S {
            let mut st = rig.state();
            let ch = rig.vehicle.chassis();
            let right = ch.rot * V3::X;
            let fwd = ch.rot * V3::Y;
            let up = ch.rot * V3::Z;
            let w = up * 0.2 + fwd * 0.6;
            let dv = right * 0.5;
            for i in 0..3 {
                st.ang_vel[i] += [w.x, w.y, w.z][i];
                st.lin_vel[i] += [dv.x, dv.y, dv.z][i];
            }
            rig.set_state(&st);
            settle_start = Some(tick);
        }
        let st = rig.state();
        let p = (st.pos[0], st.pos[1]);
        // Nearest path point (local search forward from last tick's).
        let mut best = (f32::MAX, s_car);
        let mut s = s_car - 1.0;
        while s <= s_car + 3.0 {
            let (x, y) = case.point(s);
            let d = (x - p.0) * (x - p.0) + (y - p.1) * (y - p.1);
            if d < best.0 {
                best = (d, s);
            }
            s += 0.05;
        }
        s_car = best.1;
        sum_err2 += best.0;
        if settle_start.is_none() {
            if let Some(s0) = case.settle_from_s() {
                if s_car >= s0 {
                    settle_start = Some(tick);
                }
            }
        }
        let ch = rig.vehicle.chassis();
        let fwd = ch.rot * V3::Y;
        let right = ch.rot * V3::X;
        let speed = st.speed_mps.max(1.0);
        let look = (0.9 * speed).max(8.0);
        let (qx, qy) = case.point(s_car + look);
        let d = V3::new(qx - p.0, qy - p.1, 0.0);
        let x_right = d.dot(right);
        let y_fwd = d.dot(fwd).max(1.0);
        let l2 = x_right * x_right + y_fwd * y_fwd;
        let kappa_right = 2.0 * x_right / l2;
        let kappa_full = 0.9 * grip * wp_contracts::GRAVITY_MPS2 / (speed * speed);
        let kappa_full = kappa_full.min(0.6 / wheelbase);
        let wanted = kappa_right / kappa_full;
        let steer = drv.step(tick, wanted);
        let (throttle, brake) = hold_speed(&rig, v);
        rig.step(
            &VehicleInput {
                steer,
                throttle,
                brake,
                ..Default::default()
            },
            assists,
        );
        observe(&rig);
        let a = rig.vehicle.telemetry().steer_rad;
        let rate = (a - prev_steer) / PHYSICS_DT;
        prev_steer = a;
        peak_rate = peak_rate.max(rate.abs());
        let (rear, r, v_now) = rear_axle_slip(&rig);
        max_rear = max_rear.max(rear.abs());
        if rate.abs() > FAST_DPS.to_radians() && rear.abs() < NOT_SLIDING_RAD {
            fast_ticks += 1;
        }
        // Reversals with hysteresis.
        let delta = a - extreme;
        if dir == 0.0 {
            if delta.abs() > REVERSAL_DEG.to_radians() {
                dir = delta.signum();
                extreme = a;
            }
        } else if delta * dir > 0.0 {
            extreme = a;
        } else if delta.abs() > REVERSAL_DEG.to_radians() {
            reversals += 1;
            dir = -dir;
            extreme = a;
        }
        let r_path = case.path_yaw_rate(s_car, v_now);
        let err = (r - r_path).abs();
        overshoot = overshoot.max(err);
        if let Some(t0) = settle_start {
            if tick >= t0 && err > SETTLE_RPS {
                last_unsettled = Some(tick);
            }
        }
    }
    let secs = ticks as f32 / TICKS_PER_S as f32;
    let settle_s = settle_start.and_then(|t0| match last_unsettled {
        None => Some(0.0),
        Some(t) if t + TICKS_PER_S / 4 >= ticks => None,
        Some(t) => Some((t + 1 - t0) as f32 * PHYSICS_DT),
    });
    Twitch {
        reversals_ps: reversals as f32 / secs,
        peak_rate_dps: peak_rate.to_degrees(),
        fast_not_sliding_pct: 100.0 * fast_ticks as f32 / ticks as f32,
        yaw_overshoot_dps: overshoot.to_degrees(),
        settle_s,
        rms_err_m: (sum_err2 / ticks as f32).sqrt(),
        max_rear_slip_deg: max_rear.to_degrees(),
    }
}

/// [`twitch_run_with`] without an observer.
pub fn twitch_run(car: &LoadedCar, assists: &AssistProfile, ramp: KeyRamp, case: Case) -> Twitch {
    twitch_run_with(car, assists, ramp, case, |_| {})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_ramp_matches_input_ts() {
        let m = KeyRamp::MAIN;
        // Out at 4/s, back at 6/s, reversal through centre at the out rate.
        assert!((m.step(0.0, 1.0, 0.1) - 0.4).abs() < 1e-6);
        assert!((m.step(0.5, 0.0, 0.05) - 0.2).abs() < 1e-6);
        assert!((m.step(0.5, -1.0, 0.1) - 0.1).abs() < 1e-6);
        let a = KeyRamp::ROW90;
        assert!((a.step(0.5, -1.0, 0.1) + 0.4).abs() < 1e-6);
    }
}
