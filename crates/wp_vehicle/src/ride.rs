//! Ride and "launch" measurements (owner playtest row 54: "the car is too bouncy/light and
//! gets launched too easily").
//!
//! Every test runs one car in a [`Rig`] on purpose-built ground:
//!
//! | test | what it measures |
//! |---|---|
//! | [`axle_ride`] | per-axle ride frequency and bump/rebound damping ratio from the car data (no simulation) |
//! | [`heave_kick`], [`pitch_kick`] | free body oscillation after a velocity kick at rest: damped and natural frequency, damping ratio |
//! | [`kerb`] | driving over a kerb (square step, chassis cannot touch it, like city sidewalks) at a speed and angle: peak vertical speed, roll/pitch rate, airtime, overshoots, settle time |
//! | [`slide_into_kerb`] | car sliding sideways into a kerb: peak roll rate/angle, wheel lift, rollover |
//! | [`crest`] | a crest (grade up, grade down, vertical curves of a given length): airtime and how far the body rises above its normal ride height |
//! | [`side_impact`] | a lateral impulse at bumper height on a car at rest (a T-bone): roll rate, vertical speed, wheel lift |
//!
//! Real-car targets (playtest row 54): ride 1.2–1.8 Hz, damping ratio about 0.3–0.5 in bump with
//! more in rebound, and no kerb strike at 60 km/h should launch a car off the ground.

use rapier3d::prelude::*;
use wp_contracts::{AssistProfile, LoadedCar, VehicleInput, GRAVITY_MPS2, PHYSICS_DT};

use crate::math::{v3, V3};
use crate::rig::{ground_collider, FlatWorld, Rig};
use crate::vehicle::VehicleExtras;

const TICKS_PER_S: usize = 240;
const TAU: f32 = std::f32::consts::TAU;

/// Assists used by the ride tests: wheel preset (no driver filtering) with automatic gears,
/// so the numbers are the chassis's own.
pub fn ride_assists() -> AssistProfile {
    AssistProfile {
        auto_gears: true,
        ..AssistProfile::wheel_default()
    }
}

/// Interaction groups of the kerb colliders: wheel probes see them, the chassis never touches
/// them (as `wp_sim` does for sidewalks while a car is on its wheels).
fn probe_only_groups() -> InteractionGroups {
    InteractionGroups::new(Group::GROUP_2, Group::NONE, InteractionTestMode::And)
}

fn yaw_rot(yaw: f32) -> Rotation {
    crate::math::yaw_quat(yaw)
}

/// A kerb: a raised slab `height_m` tall whose near edge is the line through the origin
/// rotated by `yaw` (0 = the edge runs along X, the kerb covers y ≥ 0), 400 m deep.
pub fn kerb_collider(height_m: f32, yaw: f32) -> Collider {
    let depth = 400.0;
    let rot = yaw_rot(yaw);
    let centre = rot * V3::new(0.0, depth * 0.5, height_m * 0.5);
    ColliderBuilder::cuboid(60.0, depth * 0.5, height_m * 0.5)
        .position(Pose::from_parts(centre, rot))
        .collision_groups(probe_only_groups())
        .build()
}

/// Road height of the crest profile at `y`: flat, then `grade` up for `half_len_m`, then
/// `grade` down for `half_len_m`, then flat again; every grade change is a vertical curve
/// `curve_m` long (slope changing linearly), centred on the kink.
pub fn crest_height(y: f32, grade: f32, half_len_m: f32, curve_m: f32) -> f32 {
    // Integrate the slope numerically with a fine step (deterministic, cheap enough).
    let slope = |s: f32| {
        let ramp = |a: f32, from: f32, to: f32| {
            if curve_m <= 1e-3 {
                if s < a {
                    from
                } else {
                    to
                }
            } else {
                let t = ((s - (a - curve_m * 0.5)) / curve_m).clamp(0.0, 1.0);
                from + (to - from) * t
            }
        };
        if s < half_len_m * 0.5 {
            ramp(0.0, 0.0, grade)
        } else if s < half_len_m * 1.5 {
            ramp(half_len_m, grade, -grade)
        } else {
            ramp(2.0 * half_len_m, -grade, 0.0)
        }
    };
    let step = 0.05;
    let mut z = 0.0;
    let mut s = -curve_m - 1.0;
    while s < y {
        let ds = step.min(y - s);
        z += slope(s + ds * 0.5) * ds;
        s += ds;
    }
    z
}

/// Crest ground as a triangle strip 30 m wide (sampled every 0.5 m), from y = −200 m to past
/// the crest; the flat slab stays under it.
pub fn crest_collider(grade: f32, half_len_m: f32, curve_m: f32) -> Collider {
    let mut verts = Vec::new();
    let mut idx = Vec::new();
    let y0 = -200.0;
    let y1 = 2.0 * half_len_m + 200.0;
    let n = ((y1 - y0) / 0.5) as u32;
    for k in 0..=n {
        let y = y0 + k as f32 * 0.5;
        let z = crest_height(y, grade, half_len_m, curve_m);
        verts.push(Vector::new(-15.0, y, z));
        verts.push(Vector::new(15.0, y, z));
        if k > 0 {
            let a = 2 * (k - 1);
            idx.push([a, a + 1, a + 2]);
            idx.push([a + 1, a + 3, a + 2]);
        }
    }
    ColliderBuilder::trimesh(verts, idx)
        .expect("crest mesh")
        .friction(0.6)
        .build()
}

fn hold_speed(rig: &Rig, target: f32) -> VehicleInput {
    let err = target - rig.state().speed_mps;
    let (throttle, brake) = if err > -1.0 {
        ((0.25 + 0.35 * err).clamp(0.0, 1.0), 0.0)
    } else {
        (0.0, (-0.2 * err).clamp(0.0, 1.0))
    };
    VehicleInput {
        throttle,
        brake,
        ..VehicleInput::default()
    }
}

/// Per-tick body sample.
#[derive(Debug, Clone, Copy)]
struct Sample {
    t: f32,
    z: f32,
    vz: f32,
    pitch_rate: f32,
    roll_rate: f32,
    roll: f32,
    up_z: f32,
    contact: [bool; 4],
}

fn sample(rig: &Rig) -> Sample {
    let ch = rig.vehicle.chassis();
    let fwd = ch.rot * V3::Y;
    let right = ch.rot * V3::X;
    let up = ch.rot * V3::Z;
    let st = rig.state();
    Sample {
        t: rig.time(),
        z: ch.com.z,
        vz: ch.lin_vel.z,
        pitch_rate: ch.ang_vel.dot(right),
        roll_rate: ch.ang_vel.dot(fwd),
        roll: crate::math::atan2(right.z, up.z),
        up_z: up.z,
        contact: std::array::from_fn(|i| st.wheel_states[i].contact),
    }
}

/// Longest stretch (s) where `f(sample)` holds.
fn longest(samples: &[Sample], f: impl Fn(&Sample) -> bool) -> f32 {
    let (mut best, mut run) = (0usize, 0usize);
    for s in samples {
        if f(s) {
            run += 1;
            best = best.max(run);
        } else {
            run = 0;
        }
    }
    best as f32 * PHYSICS_DT
}

/// Damped oscillation fit of a free response `e(t)` (deviation from equilibrium) after a
/// kick: the first two extrema give the half period and the decay.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Oscillation {
    /// Damped frequency, Hz (from the spacing of the first two extrema); 0 if no second
    /// extremum (over-damped or nearly).
    pub damped_hz: f32,
    /// Undamped natural frequency, Hz (`damped_hz / √(1 − ζ²)`).
    pub natural_hz: f32,
    /// Damping ratio from the ratio of the first two (opposite) extrema; 1.0 if there was no
    /// measurable second extremum.
    pub damping_ratio: f32,
    /// First extremum (the kick's peak), m or rad.
    pub first_peak: f32,
    /// Second (opposite) extremum, m or rad.
    pub second_peak: f32,
}

fn fit_oscillation(t: &[f32], e: &[f32]) -> Oscillation {
    // Extrema: sign changes of the discrete derivative.
    let mut ext: Vec<(f32, f32)> = Vec::new();
    for k in 1..e.len().saturating_sub(1) {
        let d0 = e[k] - e[k - 1];
        let d1 = e[k + 1] - e[k];
        if (d0 > 0.0 && d1 <= 0.0) || (d0 < 0.0 && d1 >= 0.0) {
            // Ignore tiny ripples.
            if ext.last().is_none_or(|(_, v)| (e[k] - v).abs() > 1e-5) {
                ext.push((t[k], e[k]));
            }
        }
    }
    let Some(&(t1, a1)) = ext.first() else {
        return Oscillation {
            damped_hz: 0.0,
            natural_hz: 0.0,
            damping_ratio: 1.0,
            first_peak: 0.0,
            second_peak: 0.0,
        };
    };
    match ext.get(1) {
        Some(&(t2, a2)) if a2 * a1 < 0.0 && a2.abs() > a1.abs() * 0.01 => {
            let half = t2 - t1;
            let fd = 0.5 / half;
            let d = (a1 / a2).abs().ln();
            let zeta = d / (std::f32::consts::PI.powi(2) + d * d).sqrt();
            Oscillation {
                damped_hz: fd,
                natural_hz: fd / (1.0 - zeta * zeta).max(1e-3).sqrt(),
                damping_ratio: zeta,
                first_peak: a1,
                second_peak: a2,
            }
        }
        _ => Oscillation {
            damped_hz: 0.0,
            natural_hz: 0.0,
            damping_ratio: 1.0,
            first_peak: a1,
            second_peak: 0.0,
        },
    }
}

/// Ride numbers of one axle straight from the car data (the model's sprung mass is the whole
/// car: wheels have no mass of their own).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxleRide {
    /// Static load per wheel, N.
    pub corner_load_n: f32,
    /// Ride (heave) frequency of the axle, Hz.
    pub ride_hz: f32,
    /// Damping ratio in bump.
    pub zeta_bump: f32,
    /// Damping ratio in rebound.
    pub zeta_rebound: f32,
    /// Travel left before the bump stop at static ride height, m.
    pub bump_travel_m: f32,
}

/// [`AxleRide`] for the front (`[0]`) and rear (`[1]`) axle.
pub fn axle_ride(car: &LoadedCar) -> [AxleRide; 2] {
    let def = &car.def;
    let v = crate::vehicle::Vehicle::new(def, &car.tires, [0.0; 3], 0.0);
    let cg_y = v.com_local().y;
    let yf = def.wheels[0].position_m[1];
    let yr = def.wheels[2].position_m[1];
    let w = def.mass_kg * GRAVITY_MPS2;
    let front_axle = w * (cg_y - yr) / (yf - yr);
    let loads = [front_axle * 0.5, (w - front_axle) * 0.5];
    std::array::from_fn(|a| {
        let s = &def.suspension[a * 2];
        let m = loads[a] / GRAVITY_MPS2;
        let crit = 2.0 * (s.spring_n_per_m * m).sqrt();
        let x_static = crate::suspension::static_compression(s, loads[a]);
        AxleRide {
            corner_load_n: loads[a],
            ride_hz: (s.spring_n_per_m / m).sqrt() / TAU,
            zeta_bump: s.damper_bump_ns_per_m / crit,
            zeta_rebound: s.damper_rebound_ns_per_m / crit,
            bump_travel_m: s.travel_m - x_static,
        }
    })
}

/// A rig on flat ground with the car settled at rest.
fn settled_rig(car: &LoadedCar) -> Rig {
    let mut rig = Rig::new(car);
    let idle = VehicleInput {
        brake: 1.0,
        ..VehicleInput::default()
    };
    let a = ride_assists();
    for _ in 0..3 * TICKS_PER_S {
        rig.step(&idle, &a);
    }
    rig
}

/// Heave: kick a car at rest upward by `dv` m/s at its centre of mass and fit the body's
/// vertical free response.
pub fn heave_kick(car: &LoadedCar, dv: f32) -> Oscillation {
    let mut rig = settled_rig(car);
    let a = ride_assists();
    let idle = VehicleInput {
        brake: 1.0,
        ..VehicleInput::default()
    };
    let z0 = rig.vehicle.chassis().com.z;
    rig.kick([0.0, 0.0, dv], [0.0; 3]);
    let (mut t, mut e) = (Vec::new(), Vec::new());
    for _ in 0..3 * TICKS_PER_S {
        rig.step(&idle, &a);
        t.push(rig.time());
        e.push(rig.vehicle.chassis().com.z - z0);
    }
    fit_oscillation(&t, &e)
}

/// Pitch: kick a car at rest nose-up by `dw` rad/s about its lateral axis and fit the pitch
/// angle's free response.
pub fn pitch_kick(car: &LoadedCar, dw: f32) -> Oscillation {
    let mut rig = settled_rig(car);
    let a = ride_assists();
    let idle = VehicleInput {
        brake: 1.0,
        ..VehicleInput::default()
    };
    let pitch = |rig: &Rig| {
        let fwd = rig.vehicle.chassis().rot * V3::Y;
        fwd.z.clamp(-1.0, 1.0)
    };
    let p0 = pitch(&rig);
    let right = rig.vehicle.chassis().rot * V3::X;
    rig.kick([0.0; 3], (right * dw).to_array());
    let (mut t, mut e) = (Vec::new(), Vec::new());
    for _ in 0..3 * TICKS_PER_S {
        rig.step(&idle, &a);
        t.push(rig.time());
        e.push(pitch(&rig) - p0);
    }
    fit_oscillation(&t, &e)
}

/// Result of a kerb run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KerbRun {
    /// Largest upward body (centre of mass) vertical speed, m/s.
    pub peak_vz_mps: f32,
    /// Largest body rise above the final ride height on top of the kerb, m.
    pub peak_rise_m: f32,
    /// Largest |pitch rate|, rad/s.
    pub peak_pitch_rate: f32,
    /// Largest |roll rate|, rad/s.
    pub peak_roll_rate: f32,
    /// Longest time with all four wheels off the ground, s.
    pub airborne_s: f32,
    /// Longest time any single wheel was off the ground, s.
    pub wheel_lift_s: f32,
    /// Body heave oscillations after the last wheel climbed (sign changes of the heave error
    /// with more than 3 mm between them).
    pub overshoots: u32,
    /// Time from the first wheel strike until heave stays within 5 mm and pitch/roll rates
    /// within 0.05 rad/s, s (`None`: never within the run).
    pub settle_s: Option<f32>,
}

/// Distance of world point `p` past the kerb edge (+ = on the kerb).
fn past_edge(p: V3, yaw: f32) -> f32 {
    let n = yaw_rot(yaw) * V3::Y;
    p.dot(n)
}

/// Drive straight at `speed_kmh` over a kerb `height_m` tall whose edge is turned `angle_deg`
/// from square (0 = both front wheels at once).
pub fn kerb(car: &LoadedCar, speed_kmh: f32, height_m: f32, angle_deg: f32) -> KerbRun {
    let v = speed_kmh / 3.6;
    let yaw = angle_deg.to_radians();
    let start = (v * 4.0).max(20.0);
    let mut rig = Rig::with_world(
        car,
        VehicleExtras::default(),
        FlatWorld::default(),
        [0.0, -start, 0.0],
        0.0,
    );
    rig.set_city_ground([ground_collider(), kerb_collider(height_m, yaw)]);
    rig.teleport([0.0, -start, 0.0], 0.0, v, 0.0);
    let a = ride_assists();
    let hubs: [V3; 4] = std::array::from_fn(|i| v3(car.def.wheels[i].position_m));
    let wheel_past = |rig: &Rig| {
        let ch = rig.vehicle.chassis();
        hubs.map(|h| past_edge(ch.to_world(h), yaw) > 0.0)
    };
    let mut samples = Vec::new();
    let mut t_first = None;
    let mut t_last = None;
    let limit = ((start / v + 6.0) * TICKS_PER_S as f32) as usize;
    for _ in 0..limit {
        let input = hold_speed(&rig, v);
        rig.step(&input, &a);
        let past = wheel_past(&rig);
        if t_first.is_none() && past.iter().any(|p| *p) {
            t_first = Some(samples.len());
        }
        if t_last.is_none() && past.iter().all(|p| *p) {
            t_last = Some(samples.len());
        }
        samples.push(sample(&rig));
        if let Some(l) = t_last {
            if samples.len() > l + 4 * TICKS_PER_S {
                break;
            }
        }
    }
    let first = t_first.unwrap_or(samples.len() - 1);
    let last = t_last.unwrap_or(samples.len() - 1);
    let tail = &samples[samples.len().saturating_sub(TICKS_PER_S)..];
    let z_ref = tail.iter().map(|s| s.z).sum::<f32>() / tail.len() as f32;
    let after = &samples[first..];
    let mut overshoots = 0;
    let mut sign = 0.0f32;
    let mut extreme = 0.0f32;
    for s in &samples[last..] {
        let e = s.z - z_ref;
        if e.abs() > extreme.abs() && e * sign >= 0.0 {
            extreme = e;
        }
        if e.abs() > 0.003 && e.signum() != sign {
            if sign != 0.0 {
                overshoots += 1;
            }
            sign = e.signum();
            extreme = e;
        }
    }
    let unsettled = |s: &Sample| {
        (s.z - z_ref).abs() > 0.005 || s.pitch_rate.abs() > 0.05 || s.roll_rate.abs() > 0.05
    };
    let settle_s = match after.iter().rposition(unsettled) {
        Some(k) if k + 1 < after.len() => Some(after[k + 1].t - after[0].t),
        Some(_) => None,
        None => Some(0.0),
    };
    KerbRun {
        peak_vz_mps: after.iter().map(|s| s.vz).fold(0.0, f32::max),
        peak_rise_m: after.iter().map(|s| s.z - z_ref).fold(0.0, f32::max),
        peak_pitch_rate: after.iter().map(|s| s.pitch_rate.abs()).fold(0.0, f32::max),
        peak_roll_rate: after.iter().map(|s| s.roll_rate.abs()).fold(0.0, f32::max),
        airborne_s: longest(after, |s| s.contact.iter().all(|c| !c)),
        wheel_lift_s: (0..4)
            .map(|i| longest(after, |s| !s.contact[i]))
            .fold(0.0, f32::max),
        overshoots,
        settle_s,
    }
}

/// Result of a sideways slide into a kerb, or of a side impact.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SideRun {
    /// Largest |roll rate|, rad/s.
    pub peak_roll_rate: f32,
    /// Largest |roll angle|, rad.
    pub peak_roll: f32,
    /// Largest upward body vertical speed, m/s.
    pub peak_vz_mps: f32,
    /// Longest time any single wheel was off the ground, s.
    pub wheel_lift_s: f32,
    /// Longest time with all four wheels off the ground, s.
    pub airborne_s: f32,
    /// The car ended up on its side or roof.
    pub rolled: bool,
}

fn side_run(samples: &[Sample]) -> SideRun {
    SideRun {
        peak_roll_rate: samples
            .iter()
            .map(|s| s.roll_rate.abs())
            .fold(0.0, f32::max),
        peak_roll: samples.iter().map(|s| s.roll.abs()).fold(0.0, f32::max),
        peak_vz_mps: samples.iter().map(|s| s.vz).fold(0.0, f32::max),
        wheel_lift_s: (0..4)
            .map(|i| longest(samples, |s| !s.contact[i]))
            .fold(0.0, f32::max),
        airborne_s: longest(samples, |s| s.contact.iter().all(|c| !c)),
        rolled: samples.last().is_some_and(|s| s.up_z < 0.5),
    }
}

/// A car at rest, turned so it slides sideways (its right side first) at `speed_kmh` into a
/// kerb `height_m` tall; the right wheels start 0.3 m from the kerb face.
pub fn slide_into_kerb(car: &LoadedCar, speed_kmh: f32, height_m: f32) -> SideRun {
    let mut rig = settled_rig(car);
    // Kerb edge along the car's length, 0.3 m right of the right wheels' outer face.
    let half = 0.5 * car.def.track_front_m.max(car.def.track_rear_m)
        + 0.5 * car.def.wheels[0].width_m
        + 0.3;
    let edge_x = rig.vehicle.chassis().pos.x + half;
    let mut kerb = kerb_collider(height_m, -std::f32::consts::FRAC_PI_2);
    kerb.set_translation(kerb.translation() + Vector::new(edge_x, 0.0, 0.0));
    let col = kerb;
    rig.set_city_ground([ground_collider(), col]);
    rig.kick([speed_kmh / 3.6, 0.0, 0.0], [0.0; 3]);
    let a = ride_assists();
    let input = VehicleInput::default();
    let mut samples = Vec::new();
    for _ in 0..4 * TICKS_PER_S {
        rig.step(&input, &a);
        samples.push(sample(&rig));
    }
    side_run(&samples)
}

/// A car at rest hit on its right side: `impulse_ns` (N·s, toward −X) spread over 0.1 s at
/// `height_m` above the ground, at the centre of mass's fore-aft position.
pub fn side_impact(car: &LoadedCar, impulse_ns: f32, height_m: f32) -> SideRun {
    let mut rig = settled_rig(car);
    let a = ride_assists();
    let input = VehicleInput::default();
    let n = TICKS_PER_S / 10;
    let mut samples = Vec::new();
    for k in 0..4 * TICKS_PER_S {
        if k < n {
            let ch = *rig.vehicle.chassis();
            let right = ch.rot * V3::X;
            let ground_z = ch.pos.z;
            let p = V3::new(ch.com.x, ch.com.y, ground_z + height_m)
                + right * (0.5 * car.def.body_size_m[0]);
            let j = -right * (impulse_ns / n as f32);
            rig.apply_impulse_at_point(p.to_array(), j.to_array());
        }
        rig.step(&input, &a);
        samples.push(sample(&rig));
    }
    side_run(&samples)
}

/// Result of a crest run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrestRun {
    /// Longest time with all four wheels off the ground, s.
    pub airborne_s: f32,
    /// Longest time any single wheel was off the ground, s.
    pub wheel_lift_s: f32,
    /// Largest rise of the body above its normal ride height over the road under it, m.
    pub peak_rise_m: f32,
    /// Largest downward body speed relative to the road slope at landing, m/s.
    pub landing_vz_mps: f32,
    /// Body heave oscillations after the crest (sign changes of the ride-height error with
    /// more than 5 mm between them).
    pub overshoots: u32,
}

/// Drive over a crest (`grade` up for `half_len_m`, then down; vertical curves `curve_m`) at
/// `speed_kmh`.
pub fn crest(
    car: &LoadedCar,
    speed_kmh: f32,
    grade: f32,
    half_len_m: f32,
    curve_m: f32,
) -> CrestRun {
    let v = speed_kmh / 3.6;
    let start = (v * 3.0).max(30.0);
    let mut rig = Rig::with_world(
        car,
        VehicleExtras::default(),
        FlatWorld::default(),
        [0.0, -start, 0.0],
        0.0,
    );
    rig.set_city_ground([
        ground_collider(),
        crest_collider(grade, half_len_m, curve_m),
    ]);
    rig.teleport([0.0, -start, 0.0], 0.0, v, 0.0);
    let a = ride_assists();
    let mut ride_h = None;
    let mut rise: Vec<f32> = Vec::new();
    let mut samples = Vec::new();
    let mut landing = 0.0f32;
    let end_y = 2.0 * half_len_m + 3.0 * v;
    for _ in 0..30 * TICKS_PER_S {
        let input = hold_speed(&rig, v);
        rig.step(&input, &a);
        let ch = rig.vehicle.chassis();
        let y = ch.com.y;
        let road = crest_height(y, grade, half_len_m, curve_m);
        let h = ch.com.z - road;
        if y < -curve_m - 2.0 {
            ride_h = Some(h);
            continue;
        }
        let Some(h0) = ride_h else { continue };
        let road_vz =
            (crest_height(y + 0.1, grade, half_len_m, curve_m) - road) / 0.1 * ch.lin_vel.y;
        landing = landing.max(road_vz - ch.lin_vel.z);
        rise.push(h - h0);
        samples.push(sample(&rig));
        if y > end_y {
            break;
        }
    }
    let mut overshoots = 0;
    let mut sign = 0.0f32;
    let peak_k = rise
        .iter()
        .enumerate()
        .fold(
            (0, f32::MIN),
            |b, (k, r)| if *r > b.1 { (k, *r) } else { b },
        )
        .0;
    for e in &rise[peak_k..] {
        if e.abs() > 0.005 && e.signum() != sign {
            if sign != 0.0 {
                overshoots += 1;
            }
            sign = e.signum();
        }
    }
    CrestRun {
        airborne_s: longest(&samples, |s| s.contact.iter().all(|c| !c)),
        wheel_lift_s: (0..4)
            .map(|i| longest(&samples, |s| !s.contact[i]))
            .fold(0.0, f32::max),
        peak_rise_m: rise.iter().copied().fold(0.0, f32::max),
        landing_vz_mps: landing,
        overshoots,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crest_profile_shape() {
        // Sharp: 3 % up for 20 m then down.
        assert!((crest_height(20.0, 0.03, 20.0, 0.0) - 0.6).abs() < 1e-3);
        assert!(crest_height(40.0, 0.03, 20.0, 0.0).abs() < 1e-3);
        assert!(crest_height(-5.0, 0.03, 20.0, 0.0).abs() < 1e-6);
        // Rounded: same end heights.
        assert!(crest_height(60.0, 0.03, 20.0, 10.0).abs() < 1e-3);
        assert!(crest_height(20.0, 0.03, 20.0, 10.0) < 0.6);
    }

    #[test]
    fn oscillation_fit_recovers_a_damped_sine() {
        let (fd, zeta) = (1.5f32, 0.3f32);
        let wn = TAU * fd / (1.0 - zeta * zeta).sqrt();
        let t: Vec<f32> = (0..720).map(|k| k as f32 / 240.0).collect();
        let e: Vec<f32> = t
            .iter()
            .map(|t| (-zeta * wn * t).exp() * (TAU * fd * t).sin())
            .collect();
        let o = fit_oscillation(&t, &e);
        assert!((o.damped_hz - fd).abs() < 0.05, "{o:?}");
        assert!((o.damping_ratio - zeta).abs() < 0.03, "{o:?}");
    }
}
