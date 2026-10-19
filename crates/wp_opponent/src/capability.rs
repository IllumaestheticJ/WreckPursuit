//! What a car can do, measured once with `wp_vehicle::metrics` and reused for every speed
//! profile built for it. Difficulty never touches these numbers: a driver only ever uses a
//! *fraction* of them (see [`crate::DriverParams`]).

use serde::{Deserialize, Serialize};
use wp_contracts::{HandlingEffects, LoadedCar, GRAVITY_MPS2};
use wp_vehicle::metrics;

/// Skidpad radius used for the lateral-grip measurement, m (city corners are 10–40 m).
pub const SKIDPAD_RADIUS_M: f32 = 30.0;

/// A car's measured limits.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CarCapability {
    /// Maximum steady lateral acceleration, g (`metrics::skidpad`).
    pub max_lat_g: f32,
    /// Mean deceleration of a 100–0 km/h ABS stop, g (`metrics::braking_100_0`).
    pub brake_g: f32,
    /// Mean acceleration 0–100 km/h, m/s² (`metrics::zero_to_100`).
    pub accel_mps2: f32,
    /// Top speed, m/s (`metrics::top_speed_kmh`).
    pub top_speed_mps: f32,
    /// Wheelbase, m.
    pub wheelbase_m: f32,
    /// Full-lock road-wheel angle, rad.
    pub max_steer_rad: f32,
    /// Body length, m.
    pub length_m: f32,
    /// Body width, m.
    pub width_m: f32,
}

impl CarCapability {
    /// Measure a car by running the `wp_vehicle::metrics` rigs (deterministic; a few hundred
    /// milliseconds in release). Cache the result per car id.
    pub fn measure(car: &LoadedCar) -> Self {
        let sk = metrics::skidpad(car, SKIDPAD_RADIUS_M);
        let d = metrics::braking_100_0(car).max(5.0);
        let v100 = 100.0 / 3.6;
        let brake_g = v100 * v100 / (2.0 * d) / GRAVITY_MPS2;
        let accel = metrics::zero_to_100(car).map(|t| v100 / t).unwrap_or(1.0);
        let top = metrics::top_speed_kmh(car) / 3.6;
        Self {
            max_lat_g: sk.max_lat_g.max(0.3),
            brake_g: brake_g.max(0.3),
            accel_mps2: accel.max(0.5),
            top_speed_mps: top.max(10.0),
            ..Self::geometry_only(car)
        }
    }

    /// Only the geometric fields from the car file, with typical street-car limits (0.85 g
    /// lateral, 0.95 g braking, 4 m/s², 55 m/s). For unit tests and quick tools.
    pub fn geometry_only(car: &LoadedCar) -> Self {
        let d = &car.def;
        Self {
            max_lat_g: 0.85,
            brake_g: 0.95,
            accel_mps2: 4.0,
            top_speed_mps: 55.0,
            wheelbase_m: d.wheelbase_m,
            max_steer_rad: d.wheels[0].max_steer_rad.max(0.1),
            length_m: d.body_size_m[1],
            width_m: d.body_size_m[0],
        }
    }

    /// The same car with damage applied: grip, brakes and power scaled by the worst affected
    /// wheel / the engine (what the driver *feels*, used to rebuild the speed profile).
    pub fn damaged(&self, e: &HandlingEffects) -> Self {
        let min4 = |a: [f32; 4]| a.iter().fold(1.0f32, |m, x| m.min(*x));
        let puncture = e.tire_puncture.iter().fold(0.0f32, |m, x| m.max(*x));
        let grip = (min4(e.grip_scale) * (1.0 - 0.5 * puncture)).clamp(0.2, 1.0);
        let brake = (e.brake_scale.iter().sum::<f32>() * 0.25).clamp(0.2, 1.0);
        let power = (e.power_scale * (1.0 - 0.4 * e.overheating)).clamp(0.05, 1.0);
        Self {
            max_lat_g: self.max_lat_g * grip,
            brake_g: self.brake_g * brake.min(grip.max(0.5)),
            accel_mps2: self.accel_mps2 * power,
            top_speed_mps: self.top_speed_mps * (0.6 + 0.4 * power) / e.drag_scale.max(1.0),
            ..*self
        }
    }
}
