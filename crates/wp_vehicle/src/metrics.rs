//! Handling metrics for tuning and regression tests. Each metric runs a scripted manoeuvre
//! in a [`Rig`] with deterministic controllers:
//!
//! | metric | manoeuvre |
//! |---|---|
//! | [`zero_to_100`] | standing start, full throttle, auto gears/clutch, TCS on |
//! | [`braking_100_0`] | rolling at 100 km/h, full brake with ABS, distance to stop |
//! | [`skidpad`] | pure-pursuit on a circle, speed ramped until the car can't hold the line: max lateral g and understeer gradient (deg of road-wheel steer per g) |
//! | [`step_steer`] | 80 km/h, instant steer step sized for ~0.4 g: yaw-rate overshoot % |
//! | [`lift_off`] | steady 0.7 g on a 40 m circle, then throttle off with steering frozen: yaw-rate delta and path-curvature ratio after 1 s |
//! | [`power_on`] | steady 0.4 g on a 40 m circle, steering frozen, full throttle for 1.5 s: path-curvature ratio (< 1 = power understeer) |
//! | [`power_oversteer_onset`] | steady 0.4 g on a 40 m circle, throttle ramped open-loop to 100%: throttle at which the rear slides (rear slip angle exceeds front by 2°) |
//! | [`power_oversteer_exit`] | mid-corner on a circle, then full throttle while still steering in, then steering released with throttle held: peak body slip and whether the car straightens out (slide-control assists) |
//! | [`high_speed_catch`] | straight at speed, sudden yaw-rate kick (a nudge from traffic), driver countersteers with keys / stick after 0.15 s: peak body slip and whether the car is caught |
//! | [`split_mu_launch`] | standing start with the left wheels on ice, right on dry asphalt, heading held: distance, speed and ice-side wheelspin (differentials) |
//! | [`exit_on_circle`] | steady lateral g on a circle, then full throttle while following the line: inside/outside driven-wheel slip, exit speed, peak body slip (differentials) |
//! | [`top_speed_kmh`] | full throttle on a straight until acceleration stops |

use wp_contracts::{AssistProfile, LoadedCar, VehicleInput, GRAVITY_MPS2, PHYSICS_DT};

use crate::math::{atan, atan2, V3};
use crate::rig::Rig;

const TICKS_PER_S: usize = 240;

/// Assists used by the metric controllers: ABS + TCS, automatic gears/clutch, no input
/// filtering, 1:1 steering (so the controllers command road-wheel angle directly).
pub fn metric_assists() -> AssistProfile {
    AssistProfile {
        abs: true,
        tcs: true,
        esc: false,
        steer_speed_sensitivity: 0.0,
        countersteer_help: 0.0,
        throttle_filter: 0.0,
        brake_filter: 0.0,
        auto_gears: true,
        auto_clutch: true,
    }
}

fn no_tcs() -> AssistProfile {
    AssistProfile {
        tcs: false,
        ..metric_assists()
    }
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

/// Circle follower: pure pursuit toward a counter-clockwise circle of `radius` around
/// `center`, plus a PI correction on radial error (pure pursuit alone drifts wide as the car
/// understeers).
struct CircleFollower {
    center: V3,
    radius: f32,
    integral: f32,
}

impl CircleFollower {
    fn new(radius: f32) -> Self {
        Self {
            center: V3::new(-radius, 0.0, 0.0),
            radius,
            integral: 0.0,
        }
    }

    /// Steer input (-1..1) and radial error (m, + = outside the circle).
    fn steer(&mut self, rig: &Rig) -> (f32, f32) {
        let ch = rig.vehicle.chassis();
        let p = ch.pos;
        let d = p - self.center;
        let err = (d.x * d.x + d.y * d.y).sqrt() - self.radius;
        self.integral = (self.integral + err * PHYSICS_DT).clamp(-20.0, 20.0);
        let theta = atan2(d.y, d.x);
        let look = (0.8 * speed(rig)).max(6.0);
        let (s, c) = wp_math::sin_cos(theta + look / self.radius);
        let target = self.center + V3::new(c * self.radius, s * self.radius, 0.0);
        let to = target - p;
        let (x, y) = (to.dot(ch.rot * V3::X), to.dot(ch.rot * V3::Y));
        let curvature = -2.0 * x / (x * x + y * y);
        let delta =
            atan(rig.vehicle.def().wheelbase_m * curvature) + 0.02 * err + 0.01 * self.integral;
        let max = rig.vehicle.def().wheels[0].max_steer_rad;
        ((-delta / max).clamp(-1.0, 1.0), err)
    }
}

/// Seconds from 0 to 100 km/h (None if not reached within 40 s).
pub fn zero_to_100(car: &LoadedCar) -> Option<f32> {
    let mut rig = Rig::new(car);
    let a = metric_assists();
    let input = VehicleInput {
        throttle: 1.0,
        ..Default::default()
    };
    for _ in 0..40 * TICKS_PER_S {
        rig.step(&input, &a);
        if speed(&rig) >= 100.0 / 3.6 {
            return Some(rig.time());
        }
    }
    None
}

/// Distance in metres to stop from 100 km/h with full brakes and ABS.
pub fn braking_100_0(car: &LoadedCar) -> f32 {
    let mut rig = Rig::new(car);
    rig.teleport([0.0; 3], 0.0, 100.0 / 3.6, 0.0);
    let a = metric_assists();
    let input = VehicleInput {
        brake: 1.0,
        ..Default::default()
    };
    for _ in 0..20 * TICKS_PER_S {
        rig.step(&input, &a);
        if speed(&rig) < 0.05 {
            break;
        }
    }
    rig.state().pos[1]
}

/// Skidpad result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Skidpad {
    /// Highest lateral acceleration sustained for 1 s on the line (no gear change), g.
    pub max_lat_g: f32,
    /// Understeer gradient: road-wheel steer increase per g of lateral acceleration
    /// (0.1–0.5 g fit), deg/g. Positive = understeer.
    pub understeer_deg_per_g: f32,
}

/// Skidpad on a circle of `radius` m.
pub fn skidpad(car: &LoadedCar, radius: f32) -> Skidpad {
    let mut rig = Rig::new(car);
    let mut follow = CircleFollower::new(radius);
    let v0 = 6.0;
    rig.teleport([0.0; 3], 0.0, v0, v0 / radius);
    let a = metric_assists();
    let window = TICKS_PER_S;
    let (mut sum_a, mut sum_d, mut max_e, mut n) = (0.0f32, 0.0f32, 0.0f32, 0usize);
    let mut gear = rig.state().gear;
    let mut shifted = false;
    let mut best = 0.0f32;
    let mut points: Vec<(f32, f32)> = Vec::new();
    for tick in 0..150 * TICKS_PER_S {
        let target = v0 + 0.15 * tick as f32 * PHYSICS_DT;
        let (steer, err) = follow.steer(&rig);
        let (throttle, brake) = hold_speed(&rig, target);
        rig.step(
            &VehicleInput {
                steer,
                throttle,
                brake,
                ..Default::default()
            },
            &a,
        );
        if tick < 2 * TICKS_PER_S {
            continue;
        }
        shifted |= rig.state().gear != gear || rig.vehicle.telemetry().clutch < 1.0;
        gear = rig.state().gear;
        sum_a += speed(&rig) * yaw_rate(&rig);
        sum_d += rig.vehicle.telemetry().steer_rad;
        max_e = max_e.max(err.abs());
        n += 1;
        if n == window {
            let a_g = sum_a / n as f32 / GRAVITY_MPS2;
            if max_e < 1.0 && !shifted {
                best = best.max(a_g);
                points.push((a_g, (sum_d / n as f32).to_degrees()));
            }
            if err.abs() > 4.0 {
                break;
            }
            (sum_a, sum_d, max_e, n) = (0.0, 0.0, 0.0, 0);
            shifted = false;
        }
    }
    let fit: Vec<&(f32, f32)> = points
        .iter()
        .filter(|(a, _)| (0.1..=0.5).contains(a))
        .collect();
    let k = if fit.len() >= 2 {
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
    } else {
        0.0
    };
    Skidpad {
        max_lat_g: best,
        understeer_deg_per_g: k,
    }
}

/// Yaw-rate overshoot (%) after a step steer at 80 km/h sized for ~0.4 g steady state.
pub fn step_steer(car: &LoadedCar) -> f32 {
    let mut rig = Rig::new(car);
    let v = 80.0 / 3.6;
    rig.teleport([0.0; 3], 0.0, v, 0.0);
    let a = metric_assists();
    let def = rig.vehicle.def();
    let radius = v * v / (0.4 * GRAVITY_MPS2);
    let steer_step = -(def.wheelbase_m / radius) / def.wheels[0].max_steer_rad;
    let (mut peak, mut ss, mut n_ss) = (0.0f32, 0.0f32, 0usize);
    for tick in 0..5 * TICKS_PER_S {
        let t = tick as f32 * PHYSICS_DT;
        let steer = if t >= 1.0 { steer_step } else { 0.0 };
        let (throttle, brake) = hold_speed(&rig, v);
        rig.step(
            &VehicleInput {
                steer,
                throttle,
                brake,
                ..Default::default()
            },
            &a,
        );
        let r = yaw_rate(&rig).abs();
        if (1.0..3.5).contains(&t) {
            peak = peak.max(r);
        } else if t >= 4.0 {
            ss += r;
            n_ss += 1;
        }
    }
    let ss = ss / n_ss.max(1) as f32;
    (peak / ss - 1.0) * 100.0
}

/// Lift-off result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiftOff {
    /// Yaw rate 1 s after lifting minus yaw rate before, rad/s (same sign as the turn).
    pub yaw_rate_delta: f32,
    /// Path curvature (yaw rate / speed) 1 s after lifting divided by before. > 1 = the car
    /// tucks in / rotates.
    pub curvature_ratio: f32,
}

/// Lift-off test on a circle of `radius` at 0.7 g (near the limit, where lift-off
/// rotation should show).
pub fn lift_off(car: &LoadedCar, radius: f32) -> LiftOff {
    lift_off_at(car, radius, 0.7)
}

/// Lift-off test on a circle of `radius` at `lat_g` steady lateral acceleration.
pub fn lift_off_at(car: &LoadedCar, radius: f32, lat_g: f32) -> LiftOff {
    lift_off_with(
        car,
        crate::VehicleExtras::default(),
        radius,
        lat_g,
        &no_tcs(),
    )
}

/// [`lift_off_at`] with explicit [`crate::VehicleExtras`] (e.g. a differential override) and
/// assists (e.g. `auto_gears: false` holds the gear the car starts in, so engine braking is
/// not lost to an upshift).
pub fn lift_off_with(
    car: &LoadedCar,
    extras: crate::VehicleExtras,
    radius: f32,
    lat_g: f32,
    assists: &AssistProfile,
) -> LiftOff {
    let mut rig = Rig::with_extras(car, extras);
    let mut follow = CircleFollower::new(radius);
    let v = (lat_g * GRAVITY_MPS2 * radius).sqrt();
    rig.teleport([0.0; 3], 0.0, v, v / radius);
    let a = *assists;
    let mut steer_hist = Vec::new();
    for _ in 0..6 * TICKS_PER_S {
        let (steer, _) = follow.steer(&rig);
        let (throttle, brake) = hold_speed(&rig, v);
        steer_hist.push(steer);
        rig.step(
            &VehicleInput {
                steer,
                throttle,
                brake,
                ..Default::default()
            },
            &a,
        );
    }
    let tail = &steer_hist[steer_hist.len() - TICKS_PER_S / 2..];
    let frozen = tail.iter().sum::<f32>() / tail.len() as f32;
    let (r0, v0) = (yaw_rate(&rig), speed(&rig));
    for _ in 0..TICKS_PER_S {
        rig.step(
            &VehicleInput {
                steer: frozen,
                ..Default::default()
            },
            &a,
        );
    }
    let (r1, v1) = (yaw_rate(&rig), speed(&rig));
    LiftOff {
        yaw_rate_delta: (r1 - r0) * r0.signum(),
        curvature_ratio: (r1 / v1) / (r0 / v0),
    }
}

/// Path-curvature ratio after 1.5 s of full throttle with the steering frozen, starting from
/// steady 0.4 g on a circle of `radius` (TCS/ESC off). < 1 = the car pushes wide (power
/// understeer, FWD), > 1 = it tightens / rotates (power oversteer, RWD).
pub fn power_on(car: &LoadedCar, radius: f32) -> f32 {
    let mut rig = Rig::new(car);
    let mut follow = CircleFollower::new(radius);
    let v = (0.4 * GRAVITY_MPS2 * radius).sqrt();
    rig.teleport([0.0; 3], 0.0, v, v / radius);
    let a = no_tcs();
    let mut steer_hist = Vec::new();
    for _ in 0..5 * TICKS_PER_S {
        let (steer, _) = follow.steer(&rig);
        let (throttle, brake) = hold_speed(&rig, v);
        steer_hist.push(steer);
        rig.step(
            &VehicleInput {
                steer,
                throttle,
                brake,
                ..Default::default()
            },
            &a,
        );
    }
    let tail = &steer_hist[steer_hist.len() - TICKS_PER_S / 2..];
    let frozen = tail.iter().sum::<f32>() / tail.len() as f32;
    let k0 = yaw_rate(&rig) / speed(&rig);
    for _ in 0..3 * TICKS_PER_S / 2 {
        rig.step(
            &VehicleInput {
                steer: frozen,
                throttle: 1.0,
                ..Default::default()
            },
            &a,
        );
    }
    // Speed floored at 3 m/s: a car that spins out reads as a large ratio, not infinity.
    (yaw_rate(&rig) / speed(&rig).abs().max(3.0)) / k0
}

/// Throttle (0..1) at which power oversteer starts on a circle of `radius` (None if the rear
/// never breaks away). TCS/ESC off.
pub fn power_oversteer_onset(car: &LoadedCar, radius: f32) -> Option<f32> {
    let mut rig = Rig::new(car);
    let mut follow = CircleFollower::new(radius);
    let v = (0.4 * GRAVITY_MPS2 * radius).sqrt();
    rig.teleport([0.0; 3], 0.0, v, v / radius);
    let a = no_tcs();
    let mut throttle = 0.0;
    for _ in 0..4 * TICKS_PER_S {
        let (steer, _) = follow.steer(&rig);
        let (t, brake) = hold_speed(&rig, v);
        throttle = t;
        rig.step(
            &VehicleInput {
                steer,
                throttle,
                brake,
                ..Default::default()
            },
            &a,
        );
    }
    let start = throttle;
    let ramp_ticks = 4 * TICKS_PER_S;
    let mut held = 0;
    for tick in 0..ramp_ticks {
        let thr = start + (1.0 - start) * tick as f32 / ramp_ticks as f32;
        let (steer, _) = follow.steer(&rig);
        rig.step(
            &VehicleInput {
                steer,
                throttle: thr,
                ..Default::default()
            },
            &a,
        );
        let w = rig.state().wheel_states;
        let front = 0.5 * (w[0].slip_angle.abs() + w[1].slip_angle.abs());
        let rear = 0.5 * (w[2].slip_angle.abs() + w[3].slip_angle.abs());
        if rear - front > 2f32.to_radians() {
            held += 1;
            if held >= TICKS_PER_S / 10 {
                return Some(thr);
            }
        } else {
            held = 0;
        }
    }
    None
}

/// How the scripted driver in [`power_oversteer_exit`] steers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverStyle {
    /// Digital keys: the circle follower's steer is quantised to -1 / 0 / +1.
    Keyboard,
    /// Analog: the follower's steer is used as is (gamepad stick or wheel).
    Analog,
}

/// Result of [`power_oversteer_exit`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlideExit {
    /// Largest body slip angle (heading vs direction of travel) after the throttle goes
    /// down, degrees. ≥ 90 = the car spun.
    pub max_slip_deg: f32,
    /// Body slip at the end of the run, degrees.
    pub final_slip_deg: f32,
    /// Forward speed at the end of the run, m/s (negative = travelling backwards).
    pub final_speed_mps: f32,
    /// Gear when the throttle went down.
    pub gear_at_throttle: i8,
    /// Never past 90° of slip, and straight (< 15° slip, |yaw rate| < 0.5 rad/s) and moving
    /// forward faster than 8 m/s at the end.
    pub recovered: bool,
    /// Rig state hash at the end (determinism / bit-identity checks).
    pub hash: u64,
}

fn body_slip_deg(rig: &Rig) -> f32 {
    let s = rig.state();
    let ch = rig.vehicle.chassis();
    let v = V3::from(s.lin_vel);
    let fwd = ch.rot * V3::Y;
    let left = ch.rot * -V3::X;
    atan2(v.dot(left), v.dot(fwd)).to_degrees()
}

/// Power-oversteer exit: the car settles on a counter-clockwise circle of `radius` at
/// `lat_g`, then the driver floors the throttle while still steering into the corner for
/// `react_s` seconds, then lets go of the steering (keeps full throttle) for 3 s, expecting
/// to drive out of the corner. A naive driver: it never countersteers by itself, so this
/// measures what the car and the assists do with a power slide.
pub fn power_oversteer_exit(
    car: &LoadedCar,
    assists: &AssistProfile,
    style: DriverStyle,
    radius: f32,
    lat_g: f32,
    react_s: f32,
) -> SlideExit {
    let mut rig = Rig::new(car);
    let mut follow = CircleFollower::new(radius);
    let v = (lat_g * GRAVITY_MPS2 * radius).sqrt();
    rig.teleport([0.0; 3], 0.0, v, v / radius);
    let quantise = |s: f32| match style {
        DriverStyle::Analog => s,
        DriverStyle::Keyboard if s > 0.15 => 1.0,
        DriverStyle::Keyboard if s < -0.15 => -1.0,
        DriverStyle::Keyboard => 0.0,
    };
    for _ in 0..3 * TICKS_PER_S {
        let (steer, _) = follow.steer(&rig);
        let (throttle, brake) = hold_speed(&rig, v);
        rig.step(
            &VehicleInput {
                steer: quantise(steer),
                throttle,
                brake,
                ..Default::default()
            },
            assists,
        );
    }
    let gear_at_throttle = rig.vehicle.gear();
    let mut max_slip = 0.0f32;
    let react_ticks = (react_s * TICKS_PER_S as f32) as usize;
    for tick in 0..react_ticks + 3 * TICKS_PER_S {
        let steer = if tick < react_ticks {
            quantise(follow.steer(&rig).0)
        } else {
            0.0
        };
        rig.step(
            &VehicleInput {
                steer,
                throttle: 1.0,
                ..Default::default()
            },
            assists,
        );
        max_slip = max_slip.max(body_slip_deg(&rig).abs());
    }
    let final_slip = body_slip_deg(&rig);
    let final_speed = speed(&rig);
    SlideExit {
        max_slip_deg: max_slip,
        final_slip_deg: final_slip,
        final_speed_mps: final_speed,
        gear_at_throttle,
        recovered: max_slip < 90.0
            && final_slip.abs() < 15.0
            && yaw_rate(&rig).abs() < 0.5
            && final_speed > 8.0,
        hash: rig.state_hash(),
    }
}

/// Result of [`split_mu_launch`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitMuLaunch {
    /// Distance covered, m.
    pub distance_m: f32,
    /// Forward speed at the end, m/s.
    pub speed_mps: f32,
    /// Mean wheelspin of the ice-side driven wheel(s) over the run, m/s (surface speed minus
    /// ground speed).
    pub ice_wheel_slip_mps: f32,
    /// Rig state hash at the end (determinism checks).
    pub hash: u64,
}

/// Split-µ standing start: the left wheels (FL, RL) on ice (−4 °C, wet), the right wheels on
/// dry asphalt, `throttle` held for `seconds` while the driver steers to hold the heading.
pub fn split_mu_launch(
    car: &LoadedCar,
    assists: &AssistProfile,
    throttle: f32,
    seconds: f32,
) -> SplitMuLaunch {
    split_mu_launch_with(
        car,
        crate::VehicleExtras::default(),
        assists,
        throttle,
        seconds,
    )
}

/// [`split_mu_launch`] with explicit [`crate::VehicleExtras`] (e.g. a differential override).
pub fn split_mu_launch_with(
    car: &LoadedCar,
    extras: crate::VehicleExtras,
    assists: &AssistProfile,
    throttle: f32,
    seconds: f32,
) -> SplitMuLaunch {
    let mut rig = Rig::with_extras(car, extras);
    let ice = crate::SurfaceConditions {
        water_depth_mm: 0.6,
        temperature_c: -4.0,
        ..crate::SurfaceConditions::DRY
    };
    rig.vehicle.set_wheel_conditions(0, ice);
    rig.vehicle.set_wheel_conditions(2, ice);
    let def = rig.vehicle.def().clone();
    let driven: Vec<usize> = [0usize, 2]
        .into_iter()
        .filter(|i| match def.drivetrain.layout {
            wp_contracts::DriveLayout::Fwd => *i == 0,
            wp_contracts::DriveLayout::Rwd => *i == 2,
            _ => true,
        })
        .collect();
    let mut slip_sum = 0.0f32;
    let ticks = (seconds * TICKS_PER_S as f32) as usize;
    for _ in 0..ticks {
        // Heading hold (+ steer = right): the yaw moment of unequal traction is caught.
        let fwd = rig.vehicle.chassis().rot * V3::Y;
        let heading = atan2(-fwd.x, fwd.y);
        let steer = (3.0 * heading + 0.5 * yaw_rate(&rig)).clamp(-1.0, 1.0);
        rig.step(
            &VehicleInput {
                throttle,
                steer,
                ..Default::default()
            },
            assists,
        );
        let s = rig.state();
        for i in &driven {
            slip_sum += s.wheel_states[*i].omega * def.wheels[*i].radius_m - s.speed_mps;
        }
    }
    SplitMuLaunch {
        distance_m: rig.state().pos[1],
        speed_mps: speed(&rig),
        ice_wheel_slip_mps: slip_sum / (ticks * driven.len()).max(1) as f32,
        hash: rig.state_hash(),
    }
}

/// Result of [`exit_on_circle`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CornerExit {
    /// Mean slip ratio of the inside driven wheel(s) while on the throttle.
    pub inside_slip: f32,
    /// Mean slip ratio of the outside driven wheel(s) while on the throttle.
    pub outside_slip: f32,
    /// Forward speed at the end of the throttle phase, m/s.
    pub exit_speed_mps: f32,
    /// Largest body slip angle while on the throttle, degrees (power oversteer).
    pub peak_body_slip_deg: f32,
    /// Rig state hash at the end (determinism checks).
    pub hash: u64,
}

/// Corner-exit power: steady `lat_g` on a counter-clockwise circle of `radius` (left turn,
/// inside = left), then `throttle` for `seconds` while the circle follower keeps steering
/// for the line.
pub fn exit_on_circle(
    car: &LoadedCar,
    radius: f32,
    lat_g: f32,
    throttle: f32,
    seconds: f32,
    assists: &AssistProfile,
) -> CornerExit {
    exit_on_circle_with(
        car,
        crate::VehicleExtras::default(),
        radius,
        lat_g,
        throttle,
        seconds,
        assists,
    )
}

/// [`exit_on_circle`] with explicit [`crate::VehicleExtras`].
pub fn exit_on_circle_with(
    car: &LoadedCar,
    extras: crate::VehicleExtras,
    radius: f32,
    lat_g: f32,
    throttle: f32,
    seconds: f32,
    assists: &AssistProfile,
) -> CornerExit {
    let mut rig = Rig::with_extras(car, extras);
    let mut follow = CircleFollower::new(radius);
    let v = (lat_g * GRAVITY_MPS2 * radius).sqrt();
    rig.teleport([0.0; 3], 0.0, v, v / radius);
    for _ in 0..4 * TICKS_PER_S {
        let (steer, _) = follow.steer(&rig);
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
    }
    let (inside, outside): (Vec<usize>, Vec<usize>) = match car.def.drivetrain.layout {
        wp_contracts::DriveLayout::Fwd => (vec![0], vec![1]),
        wp_contracts::DriveLayout::Rwd => (vec![2], vec![3]),
        _ => (vec![0, 2], vec![1, 3]),
    };
    let (mut si, mut so, mut peak) = (0.0f32, 0.0f32, 0.0f32);
    let ticks = (seconds * TICKS_PER_S as f32) as usize;
    for _ in 0..ticks {
        let (steer, _) = follow.steer(&rig);
        rig.step(
            &VehicleInput {
                steer,
                throttle,
                ..Default::default()
            },
            assists,
        );
        let w = rig.state().wheel_states;
        si += inside.iter().map(|i| w[*i].slip_ratio).sum::<f32>() / inside.len() as f32;
        so += outside.iter().map(|i| w[*i].slip_ratio).sum::<f32>() / outside.len() as f32;
        peak = peak.max(body_slip_deg(&rig).abs());
    }
    let n = ticks.max(1) as f32;
    CornerExit {
        inside_slip: si / n,
        outside_slip: so / n,
        exit_speed_mps: speed(&rig),
        peak_body_slip_deg: peak,
        hash: rig.state_hash(),
    }
}

/// High-speed catch: driving straight at `speed_mps` with light throttle, the car gets a yaw
/// rate kick of `yaw_kick` rad/s (+ = nose to the left). After a 0.15 s reaction the driver
/// countersteers against the body slip: a full key past 2° while the slide grows or exceeds 6°
/// (keyboard), or a stick proportional to it (analog), throttle at 30%, for 4 s.
pub fn high_speed_catch(
    car: &LoadedCar,
    assists: &AssistProfile,
    style: DriverStyle,
    speed_mps: f32,
    yaw_kick: f32,
) -> SlideExit {
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
    let gear_at_throttle = rig.vehicle.gear();
    {
        let s = rig.state();
        let ch = *rig.vehicle.chassis();
        let pos = s.pos;
        let fwd = ch.rot * V3::Y;
        let yaw = atan2(-fwd.x, fwd.y);
        let v = speed(&rig);
        rig.teleport(pos, yaw, v, yaw_kick);
    }
    let react = TICKS_PER_S * 15 / 100;
    let mut max_slip = 0.0f32;
    let mut prev_slip = body_slip_deg(&rig);
    for tick in 0..4 * TICKS_PER_S {
        let slip = body_slip_deg(&rig);
        // Like a player tapping keys: hold the countersteer key while the slide grows or is
        // still big (> 6°), let go once it is coming back.
        let growing = slip.abs() > prev_slip.abs();
        prev_slip = slip;
        let key = slip.abs() > 2.0 && (growing || slip.abs() > 6.0);
        let steer = if tick < react {
            0.0
        } else {
            match style {
                // Slip < 0 = sliding toward the right of the heading: steer right (+).
                DriverStyle::Keyboard if key => -slip.signum(),
                DriverStyle::Keyboard => 0.0,
                DriverStyle::Analog => (-slip / 20.0).clamp(-1.0, 1.0),
            }
        };
        rig.step(
            &VehicleInput {
                steer,
                throttle: 0.3,
                ..Default::default()
            },
            assists,
        );
        max_slip = max_slip.max(body_slip_deg(&rig).abs());
    }
    let final_slip = body_slip_deg(&rig);
    let final_speed = speed(&rig);
    SlideExit {
        max_slip_deg: max_slip,
        final_slip_deg: final_slip,
        final_speed_mps: final_speed,
        gear_at_throttle,
        recovered: max_slip < 90.0
            && final_slip.abs() < 15.0
            && yaw_rate(&rig).abs() < 0.5
            && final_speed > 8.0,
        hash: rig.state_hash(),
    }
}

/// Top speed in km/h (full throttle until acceleration < 0.02 m/s² or 150 s).
pub fn top_speed_kmh(car: &LoadedCar) -> f32 {
    let mut rig = Rig::new(car);
    rig.teleport([0.0; 3], 0.0, 30.0, 0.0);
    let a = metric_assists();
    let input = VehicleInput {
        throttle: 1.0,
        ..Default::default()
    };
    let mut best = 0.0f32;
    let mut last = 0.0f32;
    for tick in 0..150 * TICKS_PER_S {
        rig.step(&input, &a);
        best = best.max(speed(&rig));
        if tick % (2 * TICKS_PER_S) == 0 {
            let v = speed(&rig);
            if tick > 0 && v - last < 0.04 {
                break;
            }
            last = v;
        }
    }
    best * 3.6
}

/// Every metric for one car.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricsReport {
    /// 0–100 km/h, s.
    pub zero_to_100_s: Option<f32>,
    /// 100–0 km/h braking distance, m.
    pub braking_100_0_m: f32,
    /// Skidpad on a 50 m radius.
    pub skidpad_50m: Skidpad,
    /// Step-steer yaw overshoot, %.
    pub step_steer_overshoot_pct: f32,
    /// Lift-off on a 40 m radius.
    pub lift_off_40m: LiftOff,
    /// Power-on curvature ratio on a 40 m radius (< 1 = power understeer).
    pub power_on_curvature_ratio_40m: f32,
    /// Power-oversteer onset throttle on a 40 m radius.
    pub power_oversteer_throttle_40m: Option<f32>,
    /// Top speed, km/h.
    pub top_speed_kmh: f32,
}

/// Run every metric.
pub fn report(car: &LoadedCar) -> MetricsReport {
    MetricsReport {
        zero_to_100_s: zero_to_100(car),
        braking_100_0_m: braking_100_0(car),
        skidpad_50m: skidpad(car, 50.0),
        step_steer_overshoot_pct: step_steer(car),
        lift_off_40m: lift_off(car, 40.0),
        power_on_curvature_ratio_40m: power_on(car, 40.0),
        power_oversteer_throttle_40m: power_oversteer_onset(car, 40.0),
        top_speed_kmh: top_speed_kmh(car),
    }
}
