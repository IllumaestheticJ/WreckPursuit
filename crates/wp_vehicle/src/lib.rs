//! # wp_vehicle
//!
//! Vehicle dynamics (owned by workstream 02). **Current state: kinematic stub.**
//!
//! [`Vehicle`] implements [`wp_contracts::VehicleSim`] with a kinematic bicycle model so the
//! testbed and the sim loop have something that drives. Prompt 02 replaces the internals with
//! the Rapier rigid body + raycast wheels + tire/drivetrain model; the public surface
//! (`Vehicle::new`, the trait impl, [`TireDb`]) stays.

#![warn(missing_docs)]

use std::collections::BTreeMap;

pub use wp_contracts::AssistProfile;
use wp_contracts::{
    FfbOutput, HandlingEffects, SimEvent, TireDef, VehicleDef, VehicleInput, VehicleSim,
    VehicleState, WheelState, WorldQuery,
};

/// Tire definitions keyed by tire id (as in [`wp_contracts::LoadedCar::tires`]).
pub type TireDb = BTreeMap<String, TireDef>;

/// One simulated vehicle.
#[derive(Debug, Clone)]
pub struct Vehicle {
    def: VehicleDef,
    effects: HandlingEffects,
    pos: [f32; 3],
    yaw: f32,
    speed: f32,
    steer_rad: f32,
    gear: i8,
    engine_rpm: f32,
}

impl Vehicle {
    /// Build a vehicle from its definition and tire database, placed at `pos` (world, m) with
    /// heading `yaw` (rad about +Z; 0 = facing world +Y).
    pub fn new(def: &VehicleDef, _tires: &TireDb, pos: [f32; 3], yaw: f32) -> Self {
        Self {
            def: def.clone(),
            effects: HandlingEffects::NEUTRAL,
            pos,
            yaw,
            speed: 0.0,
            steer_rad: 0.0,
            gear: 1,
            engine_rpm: def.engine.idle_rpm,
        }
    }

    /// The definition this vehicle was built from.
    pub fn def(&self) -> &VehicleDef {
        &self.def
    }

    /// Teleport to a pose and zero all motion.
    pub fn reset(&mut self, pos: [f32; 3], yaw: f32) {
        self.pos = pos;
        self.yaw = yaw;
        self.speed = 0.0;
        self.steer_rad = 0.0;
        self.gear = 1;
        self.engine_rpm = self.def.engine.idle_rpm;
    }

    /// Teleport to a pose already moving forward at `speed_mps` (along chassis +Y). Used by
    /// `wp_world` to promote kinematic traffic to a physical vehicle without a jolt.
    pub fn set_pose_and_speed(&mut self, pos: [f32; 3], yaw: f32, speed_mps: f32) {
        self.reset(pos, yaw);
        self.speed = speed_mps;
    }

    fn peak_torque_nm(&self) -> f32 {
        self.def
            .engine
            .torque_curve_nm
            .iter()
            .map(|p| p.1)
            .fold(0.0, f32::max)
    }
}

impl VehicleSim for Vehicle {
    fn step(
        &mut self,
        input: &VehicleInput,
        assists: &AssistProfile,
        _world: &dyn WorldQuery,
        dt: f32,
        _out: &mut Vec<SimEvent>,
    ) {
        // STUB: kinematic bicycle model. Replaced by prompt 02.
        let def = &self.def;
        let r = def.wheels[0].radius_m;
        let first = def.drivetrain.gear_ratios[0] * def.drivetrain.final_drive;
        let max_accel = (self.peak_torque_nm() * first / r / def.mass_kg).min(8.0);
        let power = if self.effects.engine_dead {
            0.0
        } else {
            self.effects.power_scale
        };

        let throttle = input.throttle.clamp(0.0, 1.0);
        let brake = input.brake.clamp(0.0, 1.0);
        // f32::signum(0.0) is 1.0; resistive forces need a true zero at rest.
        let dir = if self.speed > 0.0 {
            1.0
        } else if self.speed < 0.0 {
            -1.0
        } else {
            0.0
        };
        let drag = 0.5 * 1.2 * def.drag_cd_a_m2 * self.effects.drag_scale * self.speed * self.speed
            / def.mass_kg;
        let mut accel = -drag * dir - 0.15 * dir;
        if self.speed > 0.5 || brake == 0.0 {
            accel += throttle * max_accel * power;
            accel -= brake * 9.0 * dir;
        } else {
            // Stopped or slow: brake pedal reverses.
            accel -= brake * 3.0 * power;
        }
        accel -= input.handbrake.clamp(0.0, 1.0) * 6.0 * dir;
        let new_speed = (self.speed + accel * dt).clamp(-8.0, 90.0);
        // Do not let resistive forces flip the direction of travel.
        self.speed = if dir != 0.0 && new_speed * dir < 0.0 && throttle == 0.0 && brake == 0.0 {
            0.0
        } else {
            new_speed
        };

        let sensitivity = 1.0 / (1.0 + assists.steer_speed_sensitivity * self.speed.abs() / 15.0);
        let target = -(input.steer.clamp(-1.0, 1.0) + self.effects.steer_pull)
            * def.wheels[0].max_steer_rad
            * sensitivity;
        let max_rate = def.steering.rack_speed_rad_per_s * self.effects.steer_ratio_scale * dt;
        self.steer_rad += (target - self.steer_rad).clamp(-max_rate, max_rate);

        let (s, c) = wp_math::sin_cos(self.steer_rad);
        let yaw_rate = self.speed * (s / c) / def.wheelbase_m;
        self.yaw = wp_math::wrap_angle(self.yaw + yaw_rate * dt);
        let (sy, cy) = wp_math::sin_cos(self.yaw);
        self.pos[0] += -sy * self.speed * dt;
        self.pos[1] += cy * self.speed * dt;

        self.gear = if self.speed < -0.1 {
            -1
        } else {
            let wheel_rpm = self.speed.abs() / r * 60.0 / std::f32::consts::TAU;
            let mut g = 1;
            for (i, ratio) in def.drivetrain.gear_ratios.iter().enumerate() {
                g = i as i8 + 1;
                if wheel_rpm * ratio * def.drivetrain.final_drive < def.engine.redline_rpm * 0.85 {
                    break;
                }
            }
            g
        };
        let ratio = if self.gear < 0 {
            def.drivetrain.reverse_ratio
        } else {
            def.drivetrain.gear_ratios[(self.gear - 1) as usize]
        };
        let wheel_rpm = self.speed.abs() / r * 60.0 / std::f32::consts::TAU;
        self.engine_rpm = (wheel_rpm * ratio * def.drivetrain.final_drive)
            .clamp(def.engine.idle_rpm, def.engine.rev_limit_rpm);
    }

    fn state(&self) -> VehicleState {
        let (sy, cy) = wp_math::sin_cos(self.yaw);
        let (sh, ch) = wp_math::sin_cos(self.yaw * 0.5);
        let mut wheel_states = [WheelState::default(); 4];
        for (i, ws) in wheel_states.iter_mut().enumerate() {
            let w = &self.def.wheels[i];
            ws.omega = self.speed / w.radius_m;
            ws.steer_rad = if w.steerable { self.steer_rad } else { 0.0 };
            ws.compression_m = self.def.suspension[i].travel_m * 0.5;
            ws.fz_n = self.def.mass_kg * wp_contracts::GRAVITY_MPS2 * 0.25;
            ws.contact = !self.effects.wheel_lost[i];
            ws.detached = self.effects.wheel_lost[i];
        }
        VehicleState {
            pos: self.pos,
            rot: [0.0, 0.0, sh, ch],
            lin_vel: [-sy * self.speed, cy * self.speed, 0.0],
            ang_vel: [0.0, 0.0, 0.0],
            wheel_states,
            engine_rpm: self.engine_rpm,
            gear: self.gear,
            speed_mps: self.speed,
            health: 1.0,
            component_health: BTreeMap::new(),
            mode: Default::default(),
        }
    }

    fn apply_damage_effects(&mut self, eff: &HandlingEffects) {
        self.effects = *eff;
    }

    fn ffb(&self) -> FfbOutput {
        FfbOutput::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_contracts::{SignalState, SurfaceType, TimeOfDay};

    struct Flat;
    impl WorldQuery for Flat {
        fn surface_at(&self, _p: [f32; 3]) -> SurfaceType {
            SurfaceType::Asphalt
        }
        fn speed_limit_at(&self, _p: [f32; 3]) -> Option<f32> {
            None
        }
        fn signal_state_ahead(&self, _p: [f32; 3], _h: [f32; 3]) -> Option<SignalState> {
            None
        }
        fn time_of_day(&self) -> TimeOfDay {
            TimeOfDay::default()
        }
    }

    fn sedan() -> Vehicle {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/cars/debug_sedan.json"
        );
        let car = wp_contracts::load_car(path).expect("debug_sedan loads");
        Vehicle::new(&car.def, &car.tires, [0.0; 3], 0.0)
    }

    #[test]
    fn throttle_moves_forward_along_plus_y() {
        let mut v = sedan();
        let input = VehicleInput {
            throttle: 1.0,
            ..Default::default()
        };
        let mut ev = Vec::new();
        for _ in 0..240 {
            v.step(
                &input,
                &AssistProfile::keyboard_default(),
                &Flat,
                wp_contracts::PHYSICS_DT,
                &mut ev,
            );
        }
        let s = v.state();
        assert!(s.speed_mps > 2.0);
        assert!(s.pos[1] > 1.0);
        assert!(s.pos[0].abs() < 1e-3);
    }
}
