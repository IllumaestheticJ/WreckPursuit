//! Brake dive measurement (owner playtest row 92: "headlights clip into the ground under
//! braking").
//!
//! [`brake_dive`] runs one car in a [`Rig`] on flat asphalt: settle at rest, cruise at a
//! speed, then full brake (with the given assists; the wheel preset is a step on the pedal,
//! the keyboard preset ramps it as the game does) to a stop and 1.5 s
//! beyond, and reports how far the nose goes down: peak pitch, front strut compression versus
//! travel, bump-stop use, and the lowest point of the body ahead of the front axle (lattice
//! nodes, which the panel meshes are skinned to and share their bottom with to within 5 mm;
//! and the chassis collider's front corners) versus the road.
//!
//! [`clearance_on_front_bump_stops`] is the geometric worst case: the front struts on their
//! bump stops.
//!
//! Real-car targets: about 1–2° of pitch under 1 g braking, and the bumper never touches.

use wp_contracts::{AssistProfile, LoadedCar, VehicleInput};

use crate::body::BODY_CLEARANCE_M;
use crate::math::{v3, V3};
use crate::rig::Rig;

const TICKS_PER_S: usize = 240;

/// Result of one brake run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrakeDive {
    /// Largest nose-down pitch relative to the car at rest, degrees.
    pub peak_pitch_deg: f32,
    /// Nose-down pitch relative to rest averaged over the steady part of the stop (from 80 %
    /// to 20 % of the start speed), degrees.
    pub steady_pitch_deg: f32,
    /// Mean deceleration from the start speed to 5 km/h, g.
    pub decel_g: f32,
    /// Largest front strut compression as a share of its travel (> 1: on the bump stop).
    pub front_compression_frac: f32,
    /// Front strut travel left before the bump stop at rest, m.
    pub front_bump_travel_m: f32,
    /// Deepest front bump-stop squash, m (0: the bump stop never engaged).
    pub bump_stop_m: f32,
    /// Lowest point of the body ahead of the front axle (lattice nodes) above the road at
    /// rest, m.
    pub rest_clearance_m: f32,
    /// Same point's lowest height above the road during the run, m (≤ 0: clips the road).
    pub min_clearance_m: f32,
    /// Lowest height of the chassis collider's front bottom corners above the road, m.
    pub min_collider_m: f32,
    /// Peak nose-down pitch rate, deg/s.
    pub peak_pitch_rate_dps: f32,
}

/// Nose-up pitch of the chassis, rad (`asin` of the forward axis's Z).
fn pitch(rig: &Rig) -> f32 {
    let fwd = rig.vehicle.chassis().rot * V3::Y;
    crate::math::atan2(fwd.z, (fwd.x * fwd.x + fwd.y * fwd.y).sqrt())
}

/// Body points ahead of the front axle in chassis space: the lattice nodes there.
pub fn front_body_points(car: &LoadedCar) -> Vec<V3> {
    let axle = car.def.wheels[0].position_m[1];
    car.lattice
        .nodes
        .iter()
        .map(|n| v3(n.pos_m))
        .filter(|p| p.y > axle)
        .collect()
}

/// The chassis collider's two front bottom corners in chassis space (see
/// [`crate::body::Vehicle::build_body`]).
pub fn collider_front_corners(car: &LoadedCar) -> [V3; 2] {
    let [w, l, h] = car.def.body_size_m;
    let z = BODY_CLEARANCE_M.min(h * 0.3);
    [V3::new(-w * 0.5, l * 0.5, z), V3::new(w * 0.5, l * 0.5, z)]
}

fn lowest(rig: &Rig, pts: &[V3]) -> f32 {
    let ch = rig.vehicle.chassis();
    pts.iter()
        .map(|p| ch.to_world(*p).z)
        .fold(f32::INFINITY, f32::min)
}

/// Brake pedal that holds a stopped car without engaging auto-reverse (which needs > 0.5).
const HOLD_PEDAL: f32 = 0.45;

/// Cruise at `speed_kmh` on flat asphalt, then full brake to a stop and 1.5 s beyond.
///
/// `into_reverse`: keep the pedal fully down after the stop, as a keyboard driver holding the
/// brake key does; the automatic box then engages reverse after 0.25 s and the same pedal
/// drives the car backwards (that launch also loads the front). Otherwise the pedal is eased
/// to a holding pressure at the stop.
pub fn brake_dive(
    car: &LoadedCar,
    speed_kmh: f32,
    into_reverse: bool,
    assists: &AssistProfile,
) -> BrakeDive {
    // The preset as given: the wheel preset and the testbed's owner default use manual gears,
    // so engine braking in the cruising gear adds to the stop.
    let a = *assists;
    let body = front_body_points(car);
    let corners = collider_front_corners(car);
    let mut rig = Rig::new(car);
    let hold = VehicleInput {
        brake: HOLD_PEDAL,
        ..VehicleInput::default()
    };
    let full = VehicleInput {
        brake: 1.0,
        ..VehicleInput::default()
    };
    for _ in 0..3 * TICKS_PER_S {
        rig.step(&hold, &a);
    }
    let rest_pitch = pitch(&rig);
    let rest_clearance_m = lowest(&rig, &body);
    let x_static = rig.vehicle.static_compression_m();
    let travel = car.def.suspension[0].travel_m;

    let v0 = speed_kmh / 3.6;
    rig.teleport([0.0, 0.0, 0.0], 0.0, v0, 0.0);
    for _ in 0..3 * TICKS_PER_S {
        let err = v0 - rig.state().speed_mps;
        let input = VehicleInput {
            throttle: (0.3 + 0.4 * err).clamp(0.0, 1.0),
            ..VehicleInput::default()
        };
        rig.step(&input, &a);
    }
    let v_start = rig.state().speed_mps;
    let t_start = rig.time();
    let mut t_stop = None;
    let mut out = BrakeDive {
        peak_pitch_deg: 0.0,
        steady_pitch_deg: 0.0,
        decel_g: 0.0,
        front_compression_frac: 0.0,
        front_bump_travel_m: travel - x_static[0].max(x_static[1]),
        bump_stop_m: 0.0,
        rest_clearance_m,
        min_clearance_m: f32::INFINITY,
        min_collider_m: f32::INFINITY,
        peak_pitch_rate_dps: 0.0,
    };
    let (mut steady_sum, mut steady_n) = (0.0f32, 0u32);
    let mut after_stop = 0usize;
    let mut stopped = false;
    for _ in 0..30 * TICKS_PER_S {
        let input = if stopped && !into_reverse {
            &hold
        } else {
            &full
        };
        rig.step(input, &a);
        let ch = *rig.vehicle.chassis();
        let speed = rig.state().speed_mps;
        stopped |= speed.abs() < 0.3;
        let dive = (rest_pitch - pitch(&rig)).to_degrees();
        out.peak_pitch_deg = out.peak_pitch_deg.max(dive);
        let right = ch.rot * V3::X;
        out.peak_pitch_rate_dps = out
            .peak_pitch_rate_dps
            .max(-ch.ang_vel.dot(right).to_degrees());
        if speed < 0.8 * v_start && speed > 0.2 * v_start {
            steady_sum += dive;
            steady_n += 1;
        }
        let x = rig.vehicle.strut_compression_m();
        for xi in &x[..2] {
            out.front_compression_frac = out.front_compression_frac.max(xi / travel);
            out.bump_stop_m = out.bump_stop_m.max(xi - travel);
        }
        out.min_clearance_m = out.min_clearance_m.min(lowest(&rig, &body));
        out.min_collider_m = out.min_collider_m.min(lowest(&rig, &corners));
        if t_stop.is_none() && speed < 5.0 / 3.6 {
            t_stop = Some(rig.time());
        }
        if stopped {
            after_stop += 1;
            if after_stop > 3 * TICKS_PER_S / 2 {
                break;
            }
        }
    }
    out.steady_pitch_deg = if steady_n > 0 {
        steady_sum / steady_n as f32
    } else {
        out.peak_pitch_deg
    };
    let t = t_stop.unwrap_or(rig.time()) - t_start;
    out.decel_g = (v_start - 5.0 / 3.6) / t.max(1e-3) / wp_contracts::GRAVITY_MPS2;
    out
}

/// Lowest body point ahead of the front axle (lattice nodes) above the road with both front
/// struts squashed to the end of their travel (on the bump stops) and the rear at its rest
/// height, m: the bump stops must stop the nose before the body reaches the road.
pub fn clearance_on_front_bump_stops(car: &LoadedCar) -> f32 {
    let v = crate::vehicle::Vehicle::new(&car.def, &car.tires, [0.0; 3], 0.0);
    let x_static = v.static_compression_m();
    let travel = car.def.suspension[0].travel_m;
    let drop = travel - x_static[0].min(x_static[1]);
    let y_f = car.def.wheels[0].position_m[1];
    let y_r = car.def.wheels[2].position_m[1];
    front_body_points(car)
        .iter()
        .map(|p| p.z - drop * (p.y - y_r) / (y_f - y_r))
        .fold(f32::INFINITY, f32::min)
}

/// The testbed's owner default (playtest row 29): keyboard preset with TCS, ESC and automatic
/// gears off — the setup row 92 was reported with.
pub fn owner_default_assists() -> AssistProfile {
    AssistProfile {
        tcs: false,
        esc: false,
        auto_gears: false,
        ..AssistProfile::keyboard_default()
    }
}
