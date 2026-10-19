//! Driver-facing handling measurements: how a car turns in for a keyboard, gamepad or wheel
//! driver (owner playtest row 28, "all cars understeer too much").
//!
//! [`crate::metrics`] measures the chassis with 1:1 steering. These run the same rig through a
//! real [`AssistProfile`] with a scripted driver, so speed-sensitive lock, rack filtering and
//! ESC show up in the numbers:
//!
//! | test | manoeuvre |
//! |---|---|
//! | [`ramp_steer`] | constant speed, steer input ramped slowly from 0 to full: understeer gradient (road-wheel deg/g), road-wheel angle and input at 0.6 g, lateral g at full input |
//! | [`constant_radius`] | path-following on a circle, speed ramped until the car leaves the line: max lateral g and road-wheel angle at 0.6 g |
//! | [`step_steer`] | constant speed, steer step (full key, or the input for 0.4 g with an analog driver): yaw-rate rise time to 90% |
//! | [`slalom`] | 18 m cone slalom at rising speed: fastest clean run |
//! | [`city_corner`] | 90° left, 15 m radius, at a city entry speed: how far the car runs wide, with a following driver and with the key simply held |
//!
//! Drivers: [`DriverStyle::Analog`] knows what input gives which road-wheel angle (a gamepad
//! or wheel player who has learnt the car) and steers to the path follower's angle;
//! [`DriverStyle::Keyboard`] taps the turn key (minimum 50 ms per press or release, sigma-delta on the angle error) to keep
//! the road-wheel angle near the follower's.

use wp_contracts::{AssistProfile, LoadedCar, VehicleInput, GRAVITY_MPS2, PHYSICS_DT};

use crate::math::{atan, atan2, V3};
use crate::metrics::DriverStyle;
use crate::rig::Rig;

const TICKS_PER_S: usize = 240;

/// The three shipped input presets with the scripted driver style that matches each. The
/// scripted drivers do not shift, so every preset gets automatic gears here (the wheel preset
/// is manual in game); nothing else is changed.
pub fn presets() -> [(&'static str, AssistProfile, DriverStyle); 3] {
    [
        (
            "keyboard",
            AssistProfile::keyboard_default(),
            DriverStyle::Keyboard,
        ),
        (
            "gamepad",
            AssistProfile::gamepad_default(),
            DriverStyle::Analog,
        ),
        (
            "wheel",
            AssistProfile {
                auto_gears: true,
                ..AssistProfile::wheel_default()
            },
            DriverStyle::Analog,
        ),
    ]
}

fn speed(rig: &Rig) -> f32 {
    rig.state().speed_mps
}

fn yaw_rate(rig: &Rig) -> f32 {
    rig.state().ang_vel[2]
}

fn hold_speed(rig: &Rig, target: f32) -> (f32, f32) {
    let err = target - speed(rig);
    if err > -1.0 {
        ((0.25 + 0.35 * err).clamp(0.0, 1.0), 0.0)
    } else {
        (0.0, (-0.2 * err).clamp(0.0, 1.0))
    }
}

/// Lateral acceleration meter: the change of the chassis velocity over one tick, projected on
/// the car's left, m/s² (unlike speed × yaw rate it stays honest when the car slides).
#[derive(Debug, Clone, Copy)]
struct LatMeter {
    prev: Option<V3>,
}

impl LatMeter {
    fn new() -> Self {
        Self { prev: None }
    }

    /// Call once after every rig step.
    fn sample(&mut self, rig: &Rig) -> f32 {
        let v = V3::from(rig.state().lin_vel);
        let a = self.prev.map_or(V3::ZERO, |p| (v - p) / PHYSICS_DT);
        self.prev = Some(v);
        a.dot(rig.vehicle.chassis().rot * -V3::X)
    }
}

/// A path as a polyline (x, y) with arc length, optionally closed.
#[derive(Debug, Clone)]
pub struct Path {
    pts: Vec<(f32, f32)>,
    s: Vec<f32>,
    closed: bool,
    idx: usize,
}

impl Path {
    /// Path through `pts` (spacing ≲ 0.5 m).
    pub fn new(pts: Vec<(f32, f32)>, closed: bool) -> Self {
        let mut s = vec![0.0];
        for w in pts.windows(2) {
            let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            let d = (dx * dx + dy * dy).sqrt();
            s.push(s.last().unwrap() + d);
        }
        Self {
            pts,
            s,
            closed,
            idx: 0,
        }
    }

    /// Counter-clockwise circle of `radius` through the origin, heading +Y there.
    pub fn circle(radius: f32) -> Self {
        let n = ((std::f32::consts::TAU * radius) / 0.25).ceil() as usize;
        let pts = (0..n)
            .map(|i| {
                let a = std::f32::consts::TAU * i as f32 / n as f32;
                let (s, c) = wp_math::sin_cos(a);
                (-radius + radius * c, radius * s)
            })
            .collect();
        Self::new(pts, true)
    }

    fn len(&self) -> f32 {
        *self.s.last().unwrap()
    }

    fn seg(&self, i: usize) -> ((f32, f32), (f32, f32)) {
        let n = self.pts.len();
        let a = self.pts[i % n];
        let b = self.pts[(i + 1) % n];
        (a, b)
    }

    /// Advance the running nearest-segment index and return `(arc length, signed offset)` of
    /// the point (offset + = left of the path).
    fn project(&mut self, p: (f32, f32)) -> (f32, f32) {
        let n = self.pts.len();
        let last = if self.closed { n } else { n - 1 };
        let mut best = (f32::MAX, 0.0f32, 0.0f32, self.idx);
        for k in 0..60 {
            let i = self.idx + k;
            if !self.closed && i >= last {
                break;
            }
            let i = i % last.max(1);
            let (a, b) = self.seg(i);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l2 = (dx * dx + dy * dy).max(1e-9);
            let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0);
            let (qx, qy) = (a.0 + t * dx, a.1 + t * dy);
            let (ex, ey) = (p.0 - qx, p.1 - qy);
            let d2 = ex * ex + ey * ey;
            if d2 < best.0 {
                let side = dx * (p.1 - a.1) - dy * (p.0 - a.0);
                let s0 = self.s[i];
                best = (d2, s0 + t * l2.sqrt(), d2.sqrt() * side.signum(), i);
            }
        }
        self.idx = best.3;
        (best.1, best.2)
    }

    /// Point at arc length `s` (wraps on a closed path, clamps on an open one).
    fn at(&self, s: f32) -> (f32, f32) {
        let total = self.len();
        let s = if self.closed {
            s.rem_euclid(total)
        } else {
            s.clamp(0.0, total)
        };
        let i = match self.s.binary_search_by(|v| v.partial_cmp(&s).unwrap()) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let i = i.min(self.pts.len() - 1);
        let j = if self.closed {
            (i + 1) % self.pts.len()
        } else {
            (i + 1).min(self.pts.len() - 1)
        };
        let seg = (self.s.get(i + 1).copied().unwrap_or(total) - self.s[i]).max(1e-6);
        let t = ((s - self.s[i]) / seg).clamp(0.0, 1.0);
        let (a, b) = (self.pts[i], self.pts[j]);
        (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1))
    }
}

/// Pure-pursuit path follower with a PI correction on the offset; outputs the road-wheel
/// angle it wants (rad, + = left).
#[derive(Debug, Clone)]
pub struct Follower {
    /// The path.
    pub path: Path,
    integral: f32,
    /// Arc length of the car's projection, m.
    pub s: f32,
    /// Offset from the path, m (+ = left of it).
    pub offset: f32,
}

impl Follower {
    /// Follower for `path`.
    pub fn new(path: Path) -> Self {
        Self {
            path,
            integral: 0.0,
            s: 0.0,
            offset: 0.0,
        }
    }

    /// Wanted centre road-wheel angle, rad (+ = left).
    pub fn angle(&mut self, rig: &Rig) -> f32 {
        let ch = rig.vehicle.chassis();
        let p = (ch.pos.x, ch.pos.y);
        let (s, off) = self.path.project(p);
        self.s = s;
        self.offset = off;
        self.integral = (self.integral + off * PHYSICS_DT).clamp(-20.0, 20.0);
        let look = (0.8 * speed(rig)).max(6.0);
        let t = self.path.at(s + look);
        let to = V3::new(t.0 - p.0, t.1 - p.1, 0.0);
        let (x, y) = (to.dot(ch.rot * V3::X), to.dot(ch.rot * V3::Y));
        let curvature = -2.0 * x / (x * x + y * y).max(1e-6);
        atan(rig.vehicle.def().wheelbase_m * curvature) - 0.02 * off - 0.01 * self.integral
    }
}

/// Scripted steering driver: turns the follower's wanted road-wheel angle into a steer input.
#[derive(Debug, Clone)]
pub struct SteerDriver {
    style: DriverStyle,
    key: f32,
    dwell: usize,
    acc: f32,
}

/// Minimum time a key stays pressed or released, ticks (50 ms).
const KEY_DWELL: usize = 12;
/// Below this wanted angle the keys stay released, rad of road-wheel angle.
const KEY_BAND: f32 = 0.004;

impl SteerDriver {
    /// Driver of the given style.
    pub fn new(style: DriverStyle) -> Self {
        Self {
            style,
            key: 0.0,
            dwell: 0,
            acc: 0.0,
        }
    }

    /// Steer input (-1..=1, + = right) for wanted centre angle `want` (rad, + = left).
    pub fn input(&mut self, rig: &Rig, assists: &AssistProfile, want: f32) -> f32 {
        let cur = rig.vehicle.telemetry().steer_rad;
        match self.style {
            DriverStyle::Analog => invert(rig, assists, want),
            DriverStyle::Keyboard => {
                // Sigma-delta tapping: the accumulated angle error decides press / release,
                // so the average angle follows the wanted one.
                self.acc = (self.acc + (want - cur) * PHYSICS_DT).clamp(-0.01, 0.01);
                if self.dwell > 0 {
                    self.dwell -= 1;
                    return self.key;
                }
                let dir = if want.abs() < KEY_BAND {
                    0.0
                } else {
                    want.signum()
                };
                let next = if dir != 0.0 && self.acc * dir > 0.0 {
                    -dir
                } else {
                    0.0
                };
                if next != self.key {
                    self.key = next;
                    self.dwell = KEY_DWELL;
                }
                self.key
            }
        }
    }
}

/// Input (-1..=1) whose steering target is closest to `want` (bisection; the target falls
/// as the input rises).
fn invert(rig: &Rig, assists: &AssistProfile, want: f32) -> f32 {
    let (mut lo, mut hi) = (-1.0f32, 1.0f32);
    if rig.vehicle.steer_target_for(lo, assists) <= want {
        return lo;
    }
    if rig.vehicle.steer_target_for(hi, assists) >= want {
        return hi;
    }
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if rig.vehicle.steer_target_for(mid, assists) > want {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

fn interp_at(points: &[(f32, f32)], x: f32) -> Option<f32> {
    points.windows(2).find_map(|w| {
        let (a, b) = (w[0], w[1]);
        if (a.0 <= x && x <= b.0) && b.0 > a.0 {
            Some(a.1 + (x - a.0) / (b.0 - a.0) * (b.1 - a.1))
        } else {
            None
        }
    })
}

fn slope(points: &[(f32, f32)], lo: f32, hi: f32) -> f32 {
    let fit: Vec<&(f32, f32)> = points
        .iter()
        .filter(|(a, _)| (lo..=hi).contains(a))
        .collect();
    if fit.len() < 2 {
        return 0.0;
    }
    let m = fit.len() as f32;
    let (sx, sy) = fit
        .iter()
        .fold((0.0, 0.0), |(sx, sy), (x, y)| (sx + x, sy + y));
    let (mx, my) = (sx / m, sy / m);
    let (num, den) = fit.iter().fold((0.0, 0.0), |(n, d), (x, y)| {
        (n + (x - mx) * (y - my), d + (x - mx) * (x - mx))
    });
    if den > 0.0 {
        num / den
    } else {
        0.0
    }
}

/// [`ramp_steer`] result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RampSteer {
    /// Understeer gradient, road-wheel deg per g (0.1–0.5 g fit, kinematic L·g/v² removed).
    pub understeer_deg_per_g: f32,
    /// Highest lateral acceleration in the ramp, g.
    pub max_lat_g: f32,
    /// Centre road-wheel angle at 0.6 g, deg (None if 0.6 g was not reached).
    pub steer_deg_at_06g: Option<f32>,
    /// Steer input at 0.6 g, 0..=1 (None if not reached).
    pub input_at_06g: Option<f32>,
    /// Steer input at 0.4 g, 0..=1 (None if not reached).
    pub input_at_04g: Option<f32>,
    /// Lateral acceleration 1–3 s after the input steps to full and is held, g (a held key
    /// or a stick pushed to the stop).
    pub full_input_g: f32,
    /// Limit balance at the highest lateral g: front slip / front peak slip over rear slip /
    /// rear peak slip (axle means). > 1 = the front tyres are further past their peak
    /// (understeer at the limit), < 1 = the rear (oversteer).
    pub limit_balance: f32,
}

/// Ramp steer at `speed_mps`: the road-wheel angle the driver asks for ramps up at 0.6°/s
/// (an analog driver through the preset's mapping, also for the keyboard preset) until
/// lateral acceleration stops rising; then, from straight again, the input steps to full.
pub fn ramp_steer(car: &LoadedCar, assists: &AssistProfile, speed_mps: f32) -> RampSteer {
    let mut rig = Rig::new(car);
    rig.teleport([0.0; 3], 0.0, speed_mps, 0.0);
    let mut meter = LatMeter::new();
    let rate = 0.6f32.to_radians();
    let window = TICKS_PER_S / 4;
    let (mut sum_a, mut sum_d, mut sum_u, mut n) = (0.0f32, 0.0f32, 0.0f32, 0usize);
    let mut pts_d: Vec<(f32, f32)> = Vec::new();
    let mut pts_u: Vec<(f32, f32)> = Vec::new();
    let mut best = 0.0f32;
    let mut falling = 0;
    let mut sum_bal = 0.0f32;
    let mut balance = 1.0f32;
    let (pf, pr) = (rig.vehicle.tire_peak_slip(0), rig.vehicle.tire_peak_slip(2));
    for tick in 0..30 * TICKS_PER_S {
        let t = tick as f32 * PHYSICS_DT - 1.0;
        let want = (t * rate).max(0.0);
        let u = -invert(&rig, assists, want);
        let (throttle, brake) = hold_speed(&rig, speed_mps);
        rig.step(
            &VehicleInput {
                steer: -u,
                throttle,
                brake,
                ..Default::default()
            },
            assists,
        );
        let a = meter.sample(&rig) / GRAVITY_MPS2;
        if t < 0.0 {
            continue;
        }
        sum_a += a;
        sum_d += rig.vehicle.telemetry().steer_rad;
        sum_u += u;
        let ws = rig.state().wheel_states;
        let af = 0.5 * (ws[0].slip_angle.abs() + ws[1].slip_angle.abs()) / pf;
        let ar = 0.5 * (ws[2].slip_angle.abs() + ws[3].slip_angle.abs()) / pr;
        sum_bal += af / ar.max(1e-4);
        n += 1;
        if n == window {
            let (a, d, u) = (sum_a / n as f32, sum_d / n as f32, sum_u / n as f32);
            if a > best {
                best = a;
                balance = sum_bal / n as f32;
                falling = 0;
                pts_d.push((a, d.to_degrees()));
                pts_u.push((a, u));
            } else {
                falling += 1;
            }
            (sum_a, sum_d, sum_u, sum_bal, n) = (0.0, 0.0, 0.0, 0.0, 0);
            if falling >= 8 || u >= 0.999 {
                break;
            }
        }
    }
    // Held full input from straight running.
    let mut rig = Rig::new(car);
    rig.teleport([0.0; 3], 0.0, speed_mps, 0.0);
    let mut meter = LatMeter::new();
    let mut full = (0.0f32, 0usize);
    for tick in 0..3 * TICKS_PER_S {
        let (throttle, brake) = hold_speed(&rig, speed_mps);
        rig.step(
            &VehicleInput {
                steer: -1.0,
                throttle,
                brake,
                ..Default::default()
            },
            assists,
        );
        let a = meter.sample(&rig) / GRAVITY_MPS2;
        if tick >= TICKS_PER_S {
            full = (full.0 + a, full.1 + 1);
        }
    }
    let kin = (car.def.wheelbase_m * GRAVITY_MPS2 / (speed_mps * speed_mps)).to_degrees();
    RampSteer {
        understeer_deg_per_g: slope(&pts_d, 0.1, 0.5) - kin,
        max_lat_g: best,
        steer_deg_at_06g: interp_at(&pts_d, 0.6),
        input_at_06g: interp_at(&pts_u, 0.6),
        input_at_04g: interp_at(&pts_u, 0.4),
        full_input_g: full.0 / full.1.max(1) as f32,
        limit_balance: balance,
    }
}

/// [`constant_radius`] result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConstantRadius {
    /// Highest lateral acceleration held for 1 s within 1 m of the line, g.
    pub max_lat_g: f32,
    /// Understeer gradient, road-wheel deg per g (0.1–0.5 g fit).
    pub understeer_deg_per_g: f32,
    /// Centre road-wheel angle at 0.6 g, deg.
    pub steer_deg_at_06g: Option<f32>,
}

/// Constant-radius test: follow a circle of `radius`, speed ramped up at 0.15 m/s² until the
/// car is 4 m off the line.
pub fn constant_radius(
    car: &LoadedCar,
    assists: &AssistProfile,
    style: DriverStyle,
    radius: f32,
) -> ConstantRadius {
    let mut rig = Rig::new(car);
    let mut follow = Follower::new(Path::circle(radius));
    let mut driver = SteerDriver::new(style);
    let v0 = 6.0;
    rig.teleport([0.0; 3], 0.0, v0, v0 / radius);
    let window = TICKS_PER_S;
    let (mut sum_a, mut sum_d, mut max_e, mut n) = (0.0f32, 0.0f32, 0.0f32, 0usize);
    let mut best = 0.0f32;
    let mut points: Vec<(f32, f32)> = Vec::new();
    let mut meter = LatMeter::new();
    for tick in 0..200 * TICKS_PER_S {
        let target = v0 + 0.15 * tick as f32 * PHYSICS_DT;
        let want = follow.angle(&rig);
        let steer = driver.input(&rig, assists, want);
        let (throttle, brake) = hold_speed(&rig, target);
        rig.step(
            &VehicleInput {
                steer,
                throttle,
                brake,
                ..Default::default()
            },
            assists,
        );
        let a = meter.sample(&rig);
        if tick < 2 * TICKS_PER_S {
            continue;
        }
        sum_a += a;
        sum_d += rig.vehicle.telemetry().steer_rad;
        max_e = max_e.max(follow.offset.abs());
        n += 1;
        if n == window {
            let a_g = sum_a / n as f32 / GRAVITY_MPS2;
            if max_e < 1.0 {
                best = best.max(a_g);
                points.push((a_g, (sum_d / n as f32).to_degrees()));
            }
            if follow.offset.abs() > 4.0 {
                break;
            }
            (sum_a, sum_d, max_e, n) = (0.0, 0.0, 0.0, 0);
        }
    }
    points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    ConstantRadius {
        max_lat_g: best,
        understeer_deg_per_g: slope(&points, 0.1, 0.5),
        steer_deg_at_06g: interp_at(&points, 0.6),
    }
}

/// [`step_steer`] result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepSteer {
    /// Time from the step to 90% of the steady yaw rate, s.
    pub rise_time_s: f32,
    /// Yaw-rate overshoot over steady state, %.
    pub overshoot_pct: f32,
    /// Steady lateral acceleration after the step, g.
    pub steady_lat_g: f32,
}

/// Step steer at `speed_mps`: input steps from 0 to `input` (left) at t = 1 s and is held.
pub fn step_steer(
    car: &LoadedCar,
    assists: &AssistProfile,
    speed_mps: f32,
    input: f32,
) -> StepSteer {
    let mut rig = Rig::new(car);
    rig.teleport([0.0; 3], 0.0, speed_mps, 0.0);
    let mut trace: Vec<f32> = Vec::new();
    let mut meter = LatMeter::new();
    let (mut ss, mut ss_a, mut n_ss) = (0.0f32, 0.0f32, 0usize);
    for tick in 0..5 * TICKS_PER_S {
        let t = tick as f32 * PHYSICS_DT;
        let steer = if t >= 1.0 { -input } else { 0.0 };
        let (throttle, brake) = hold_speed(&rig, speed_mps);
        rig.step(
            &VehicleInput {
                steer,
                throttle,
                brake,
                ..Default::default()
            },
            assists,
        );
        let a = meter.sample(&rig);
        if t >= 1.0 {
            trace.push(yaw_rate(&rig));
        }
        if t >= 3.5 {
            ss += yaw_rate(&rig);
            ss_a += a;
            n_ss += 1;
        }
    }
    let ss = ss / n_ss.max(1) as f32;
    let rise = trace
        .iter()
        .position(|r| r * ss.signum() >= 0.9 * ss.abs())
        .map(|i| (i + 1) as f32 * PHYSICS_DT)
        .unwrap_or(4.0);
    let peak = trace.iter().fold(0.0f32, |m, r| m.max(r * ss.signum()));
    StepSteer {
        rise_time_s: rise,
        overshoot_pct: (peak / ss.abs().max(1e-6) - 1.0) * 100.0,
        steady_lat_g: ss_a / n_ss.max(1) as f32 / GRAVITY_MPS2,
    }
}

/// Run one slalom at `speed_mps`: worst offset from the line (m), and whether the car got
/// through (offset < 1.2 m, body slip < 20°).
pub fn slalom_run(
    car: &LoadedCar,
    assists: &AssistProfile,
    style: DriverStyle,
    speed_mps: f32,
) -> (f32, bool) {
    // Cones every 18 m, line weaving ±1.5 m; 40 m lead-in, 8 cones, 30 m out.
    let spacing = 18.0;
    let amp = 1.5;
    let lead = 40.0;
    let cones = 8.0;
    let mut pts = Vec::new();
    let mut y = 0.0f32;
    while y < lead + cones * spacing + 30.0 {
        let x = if y < lead || y > lead + cones * spacing {
            0.0
        } else {
            amp * wp_math::sin_cos(std::f32::consts::PI * (y - lead) / spacing).0
        };
        pts.push((x, y));
        y += 0.25;
    }
    let mut rig = Rig::new(car);
    rig.teleport([0.0; 3], 0.0, speed_mps, 0.0);
    let mut follow = Follower::new(Path::new(pts, false));
    let mut driver = SteerDriver::new(style);
    let mut worst = 0.0f32;
    let mut slip_ok = true;
    let end = lead + cones * spacing + 10.0;
    for _ in 0..40 * TICKS_PER_S {
        let want = follow.angle(&rig);
        let steer = driver.input(&rig, assists, want);
        let (throttle, brake) = hold_speed(&rig, speed_mps);
        rig.step(
            &VehicleInput {
                steer,
                throttle,
                brake,
                ..Default::default()
            },
            assists,
        );
        if follow.s > lead - 5.0 {
            worst = worst.max(follow.offset.abs());
        }
        let s = rig.state();
        let v = V3::from(s.lin_vel);
        let ch = rig.vehicle.chassis();
        let slip = atan2(v.dot(ch.rot * -V3::X), v.dot(ch.rot * V3::Y)).abs();
        if v.length() > 3.0 && slip > 20f32.to_radians() {
            slip_ok = false;
        }
        if follow.s > end || !slip_ok || worst > 3.0 {
            break;
        }
    }
    (worst, slip_ok && worst < 1.2)
}

/// Fastest clean slalom run, km/h (5 km/h steps from 40 km/h; 0 if 40 km/h fails).
pub fn slalom(car: &LoadedCar, assists: &AssistProfile, style: DriverStyle) -> f32 {
    let mut best = 0.0;
    let mut kmh = 40.0;
    while kmh <= 130.0 {
        if slalom_run(car, assists, style, kmh / 3.6).1 {
            best = kmh;
        } else {
            break;
        }
        kmh += 5.0;
    }
    best
}

/// [`city_corner`] result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CityCorner {
    /// How far the following driver ran wide of the line, m.
    pub follow_wide_m: f32,
    /// How far the car ran wide with the turn key / stick held fully through the corner, m.
    pub held_wide_m: f32,
    /// Fraction of the corner with ESC braking (following driver).
    pub esc_frac: f32,
}

/// The city corner path: 60 m straight along +Y, 90° left on a 15 m radius, 40 m along −X.
pub const CITY_RADIUS_M: f32 = 15.0;
/// City corner entry speed, km/h (≈ 0.64 g on the 15 m radius).
pub const CITY_ENTRY_KMH: f32 = 35.0;

fn city_path() -> (Path, f32, f32) {
    let r = CITY_RADIUS_M;
    let mut pts = Vec::new();
    let mut y = 0.0f32;
    while y < 60.0 {
        pts.push((0.0, y));
        y += 0.25;
    }
    let s_in = 60.0;
    let n = ((std::f32::consts::FRAC_PI_2 * r) / 0.25).ceil() as usize;
    for i in 0..=n {
        let a = std::f32::consts::FRAC_PI_2 * i as f32 / n as f32;
        let (s, c) = wp_math::sin_cos(a);
        pts.push((-r + r * c, 60.0 + r * s));
    }
    let mut x = -r - 0.25;
    while x > -r - 40.0 {
        pts.push((x, 60.0 + r));
        x -= 0.25;
    }
    let s_out = s_in + std::f32::consts::FRAC_PI_2 * r;
    (Path::new(pts, false), s_in, s_out)
}

/// 90° city corner at [`CITY_ENTRY_KMH`]: once with the following driver, once with the turn
/// key (or stick) held fully from the corner entry to its end.
pub fn city_corner(car: &LoadedCar, assists: &AssistProfile, style: DriverStyle) -> CityCorner {
    let v = CITY_ENTRY_KMH / 3.6;
    let run = |held: bool| -> (f32, f32) {
        let (path, s_in, s_out) = city_path();
        let mut rig = Rig::new(car);
        rig.teleport([0.0; 3], 0.0, v, 0.0);
        let mut follow = Follower::new(path);
        let mut driver = SteerDriver::new(style);
        let mut wide = 0.0f32;
        let (mut esc, mut n) = (0usize, 0usize);
        for _ in 0..20 * TICKS_PER_S {
            let want = follow.angle(&rig);
            let s = follow.s;
            let steer = if held && s > s_in - 2.0 && s < s_out {
                -1.0
            } else {
                driver.input(&rig, assists, want)
            };
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
            if s > s_in && s < s_out + 15.0 {
                // Running wide = to the right of the line (outside of a left turn).
                wide = wide.max(-follow.offset);
                n += 1;
                esc += rig.vehicle.telemetry().esc_active as usize;
            }
            if s > s_out + 30.0 {
                break;
            }
        }
        (wide, esc as f32 / n.max(1) as f32)
    };
    let (follow_wide_m, esc_frac) = run(false);
    let (held_wide_m, _) = run(true);
    CityCorner {
        follow_wide_m,
        held_wide_m,
        esc_frac,
    }
}
