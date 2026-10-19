//! The vehicle: per-tick orchestration of input → assists → gearbox → steering → ground probe
//! → suspension → brakes → (sub-stepped) drivetrain + wheel spin + tires → chassis force and
//! torque. The chassis itself is a Rapier rigid body (see [`crate::body`]): this module only
//! *computes* forces from the chassis state it was given; Rapier integrates them and resolves
//! collisions.

use wp_contracts::{
    AssistProfile, Component, DiffDef, DriveLayout, EntityId, FfbOutput, HandlingEffects, SimEvent,
    SurfaceType, SuspensionDef, TireDef, VehicleDef, VehicleInput, VehicleSim, VehicleState,
    WheelState, WorldQuery, GRAVITY_MPS2, VEHICLE_SUBSTEPS,
};

use crate::aero::aero_forces;
use crate::assists::{esc, filter_pedal, Tcs};
use crate::brakes::{service_torques, Abs};
use crate::drivetrain::{auto_clutch_update, diff_transfer, Converter, Gearbox};
use crate::engine::{Engine, EngineExtras, RPM_PER_RADPS};
use crate::ffb::{self, FfbWheel};
use crate::ground::WheelCaster;
use crate::math::{arr, sin_cos, v3, yaw_quat, Quat, V3};
use crate::steering::{ackermann, body_slip, torque_steer, update_center_angle, SteerInput};
use crate::suspension::{arb_force, static_compression, strut_force};
use crate::tire::{
    combined_forces, load_sensitive_mu, low_speed_lateral, pneumatic_trail, relax, slip_angle,
    slip_ratio, surface_grip, SLIP_RATIO_V_MIN,
};
use crate::TireDb;

/// Radius of the bare hub/knuckle that touches the ground when a wheel is lost, m.
pub const HUB_RADIUS_M: f32 = 0.12;
const HUB_STIFFNESS: f32 = 200_000.0;
const HUB_DAMPING: f32 = 6_000.0;
const HUB_FRICTION: f32 = 0.8;
/// Lateral camber thrust per radian of camber, as a fraction of load.
const CAMBER_THRUST: f32 = 0.15;
/// Mechanical efficiency of gearbox + final drive for driving torque (engine braking passes
/// through unchanged).
pub const DRIVETRAIN_EFFICIENCY: f32 = 0.85;
/// Seconds at `overheating = 1` until the engine is cooked.
const OVERHEAT_TIME_S: f32 = 20.0;

/// Vehicle options that are not part of the shared contract (mods / tuning only).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VehicleExtras {
    /// Turbo options.
    pub engine: EngineExtras,
    /// Pulling the handbrake declutches the rear axle (rally/drift-style hydraulic handbrake).
    pub handbrake_disconnects_rear: bool,
}

impl Default for VehicleExtras {
    fn default() -> Self {
        Self {
            engine: EngineExtras::default(),
            handbrake_disconnects_rear: true,
        }
    }
}

/// Chassis rigid-body state (world space), mirrored from Rapier each tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chassis {
    /// Chassis reference point (body origin), m.
    pub pos: V3,
    /// Orientation.
    pub rot: Quat,
    /// Velocity of the centre of mass, m/s.
    pub lin_vel: V3,
    /// Angular velocity, rad/s.
    pub ang_vel: V3,
    /// Centre of mass, m.
    pub com: V3,
}

impl Chassis {
    /// Velocity of a world-space point rigidly attached to the chassis.
    pub fn velocity_at(&self, p: V3) -> V3 {
        self.lin_vel + self.ang_vel.cross(p - self.com)
    }

    /// Transform a chassis-space point to world space.
    pub fn to_world(&self, local: V3) -> V3 {
        self.pos + self.rot * local
    }
}

/// Static per-corner data derived from the definition.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Corner {
    pub hub: V3,
    pub x_static: f32,
    pub susp: SuspensionDef,
    pub radius: f32,
    pub inertia: f32,
    pub steerable: bool,
    pub max_steer: f32,
    pub ackermann: f32,
    pub camber: f32,
    pub toe: f32,
    pub caster: f32,
    pub tire: TireDef,
    pub left: bool,
}

/// Per-wheel runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Wheel {
    pub omega: f32,
    pub kappa: f32,
    pub alpha: f32,
    pub alpha_lag: f32,
    pub x: f32,
    pub vcomp: f32,
    pub contact: bool,
    pub fz: f32,
    pub surface: SurfaceType,
    pub fx: f32,
    pub fy: f32,
    pub mz: f32,
    pub steer: f32,
    pub point: V3,
    pub normal: V3,
    pub fwd: V3,
    pub lat: V3,
    pub vx: f32,
    pub vy: f32,
    pub r_eff: f32,
    pub grip: f32,
    pub drive: f32,
    pub brake: f32,
    pub hub_force: V3,
}

/// Extra per-tick values for telemetry, the HUD and tuning.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Telemetry {
    /// Throttle reaching the engine after filters/TCS/ESC, 0..=1.
    pub throttle: f32,
    /// Brake pedal after filtering, 0..=1.
    pub brake: f32,
    /// Clutch engagement, 0..=1.
    pub clutch: f32,
    /// Clutch locked (engine speed tied to wheels).
    pub clutch_locked: bool,
    /// Centre road-wheel angle, rad (+ = left).
    pub steer_rad: f32,
    /// TCS throttle cut 0..=1.
    pub tcs_cut: f32,
    /// ABS releasing any wheel.
    pub abs_active: bool,
    /// ESC braking any wheel.
    pub esc_active: bool,
    /// Per-wheel longitudinal tire force, N.
    pub fx: [f32; 4],
    /// Per-wheel lateral tire force, N.
    pub fy: [f32; 4],
    /// Accumulated engine heat damage 0..=1.
    pub engine_heat: f32,
    /// Fuel remaining 0..=1.
    pub fuel: f32,
}

/// One simulated vehicle. See the crate docs for the integration contract with Rapier.
#[derive(Debug, Clone)]
pub struct Vehicle {
    pub(crate) def: VehicleDef,
    pub(crate) extras: VehicleExtras,
    pub(crate) corners: [Corner; 4],
    pub(crate) wheels: [Wheel; 4],
    pub(crate) engine: Engine,
    pub(crate) gearbox: Gearbox,
    pub(crate) clutch_locked: bool,
    pub(crate) clutch: f32,
    pub(crate) auto_clutch: bool,
    pub(crate) launch_torque_ratio: f32,
    pub(crate) converter: Option<Converter>,
    pub(crate) awd_coupling: f32,
    pub(crate) center_steer: f32,
    pub(crate) throttle_f: f32,
    pub(crate) brake_f: f32,
    pub(crate) tcs: Tcs,
    pub(crate) abs: Abs,
    pub(crate) effects: HandlingEffects,
    pub(crate) heat: f32,
    pub(crate) heat_reported: f32,
    pub(crate) fuel: f32,
    pub(crate) chassis: Chassis,
    pub(crate) com_local: V3,
    pub(crate) force: V3,
    pub(crate) torque: V3,
    pub(crate) ffb: FfbOutput,
    pub(crate) telemetry: Telemetry,
    pub(crate) entity: EntityId,
}

impl Vehicle {
    /// Build a vehicle at world position `pos` with heading `yaw` (rad about +Z, 0 = +Y).
    /// `pos` is the chassis reference point: middle of the wheelbase, on the ground.
    ///
    /// # Panics
    /// If a wheel's `tire_id` is missing from `tires` (use a validated
    /// [`wp_contracts::LoadedCar`]).
    pub fn new(def: &VehicleDef, tires: &TireDb, pos: [f32; 3], yaw: f32) -> Self {
        Self::with_extras(def, tires, pos, yaw, VehicleExtras::default())
    }

    /// [`Vehicle::new`] with non-contract options (turbo, handbrake declutch).
    pub fn with_extras(
        def: &VehicleDef,
        tires: &TireDb,
        pos: [f32; 3],
        yaw: f32,
        extras: VehicleExtras,
    ) -> Self {
        let corners: [Corner; 4] = std::array::from_fn(|i| {
            let w = &def.wheels[i];
            let front = i < 2;
            let axle_frac = if front {
                def.front_weight_frac
            } else {
                1.0 - def.front_weight_frac
            };
            let load = def.mass_kg * GRAVITY_MPS2 * axle_frac * 0.5;
            Corner {
                hub: v3(w.position_m),
                x_static: static_compression(&def.suspension[i], load),
                susp: def.suspension[i].clone(),
                radius: w.radius_m,
                inertia: w
                    .inertia_kgm2
                    .unwrap_or(0.7 * w.mass_kg * w.radius_m * w.radius_m),
                steerable: w.steerable,
                max_steer: w.max_steer_rad,
                ackermann: w.ackermann_frac,
                camber: w.camber_rad,
                toe: w.toe_rad,
                caster: w.caster_rad,
                tire: tires
                    .get(&w.tire_id)
                    .unwrap_or_else(|| panic!("tire {:?} not in TireDb", w.tire_id))
                    .clone(),
                left: i % 2 == 0,
            }
        });
        let y_f = def.wheels[0].position_m[1];
        let y_r = def.wheels[2].position_m[1];
        let com_local = V3::new(
            0.0,
            y_r + def.front_weight_frac * (y_f - y_r),
            def.cg_height_m,
        );
        let rot = yaw_quat(yaw);
        let pos = v3(pos);
        let mut v = Self {
            def: def.clone(),
            extras,
            wheels: [Wheel::default(); 4],
            engine: Engine::new(&def.engine, extras.engine),
            gearbox: Gearbox::new(),
            clutch_locked: false,
            clutch: 0.0,
            auto_clutch: true,
            launch_torque_ratio: (1.2
                * def
                    .engine
                    .torque_curve_nm
                    .iter()
                    .map(|p| p.1)
                    .fold(0.0, f32::max)
                / def.drivetrain.clutch_max_nm)
                .min(1.0),
            converter: None,
            awd_coupling: 0.0,
            center_steer: 0.0,
            throttle_f: 0.0,
            brake_f: 0.0,
            tcs: Tcs::default(),
            abs: Abs::default(),
            effects: HandlingEffects::NEUTRAL,
            heat: 0.0,
            heat_reported: 0.0,
            fuel: 1.0,
            chassis: Chassis {
                pos,
                rot,
                lin_vel: V3::ZERO,
                ang_vel: V3::ZERO,
                com: pos + rot * com_local,
            },
            com_local,
            force: V3::ZERO,
            torque: V3::ZERO,
            ffb: FfbOutput::default(),
            telemetry: Telemetry::default(),
            entity: EntityId(0),
            corners,
        };
        v.converter = def
            .drivetrain
            .torque_converter
            .map(|tc| Converter::new(tc, v.engine.curve_torque(tc.stall_rpm)));
        v.reset_wheels();
        v
    }

    fn reset_wheels(&mut self) {
        for (w, c) in self.wheels.iter_mut().zip(&self.corners) {
            *w = Wheel {
                x: c.x_static,
                r_eff: c.radius,
                grip: 1.0,
                ..Wheel::default()
            };
        }
    }

    /// The definition this vehicle was built from.
    pub fn def(&self) -> &VehicleDef {
        &self.def
    }

    /// Entity id used in emitted events.
    pub fn set_entity(&mut self, id: EntityId) {
        self.entity = id;
    }

    /// Chassis state as last read from the rigid body.
    pub fn chassis(&self) -> &Chassis {
        &self.chassis
    }

    /// Overwrite the chassis state (normally done by [`Vehicle::read_body`]).
    pub fn set_chassis(&mut self, chassis: Chassis) {
        self.chassis = chassis;
    }

    /// Centre of mass in chassis space, m.
    pub fn com_local(&self) -> V3 {
        self.com_local
    }

    /// Net force (world, at the centre of mass, N) and torque (world, about the centre of
    /// mass, N·m) computed by the last step, excluding gravity.
    pub fn forces(&self) -> (V3, V3) {
        (self.force, self.torque)
    }

    /// Telemetry from the last step.
    pub fn telemetry(&self) -> Telemetry {
        self.telemetry
    }

    /// Current gear (-1 reverse, 0 neutral, 1.. forward).
    pub fn gear(&self) -> i8 {
        self.gearbox.gear
    }

    /// Reset drivetrain, wheels, assists and damage-derived state; place the chassis at
    /// `pos`/`yaw` at rest. Call [`Vehicle::place_body`] too when a body is attached.
    pub fn reset(&mut self, pos: [f32; 3], yaw: f32) {
        let rot = yaw_quat(yaw);
        let pos = v3(pos);
        self.chassis = Chassis {
            pos,
            rot,
            lin_vel: V3::ZERO,
            ang_vel: V3::ZERO,
            com: pos + rot * self.com_local,
        };
        self.reset_wheels();
        self.engine = Engine::new(&self.def.engine, self.extras.engine);
        self.gearbox = Gearbox::new();
        self.clutch_locked = false;
        self.clutch = 0.0;
        self.center_steer = 0.0;
        self.throttle_f = 0.0;
        self.brake_f = 0.0;
        self.tcs = Tcs::default();
        self.abs = Abs::default();
        self.heat = 0.0;
        self.heat_reported = 0.0;
        self.fuel = 1.0;
        self.force = V3::ZERO;
        self.torque = V3::ZERO;
    }

    /// Overwrite the dynamic state from a [`VehicleState`] (`VehicleSim::state` output):
    /// chassis pose and velocities, gear, engine rpm, and per wheel spin, steer, slip,
    /// compression and load; the clutch is treated as locked when engine and wheel speeds
    /// agree. Approximate: input filters, ABS/TCS state, shift timers and turbo boost are not
    /// in `VehicleState` and keep their values — for bit-exact rewind use
    /// [`Vehicle::snapshot`] / [`Vehicle::restore`]. With a Rapier body, call
    /// [`Vehicle::write_chassis_to_body`] afterwards. (Added by the integration lead for
    /// workstream F; extended by 02.)
    pub fn set_state(&mut self, s: &wp_contracts::VehicleState) {
        let rot = Quat::from_xyzw(s.rot[0], s.rot[1], s.rot[2], s.rot[3]);
        let pos = v3(s.pos);
        self.chassis = Chassis {
            pos,
            rot,
            lin_vel: v3(s.lin_vel),
            ang_vel: v3(s.ang_vel),
            com: pos + rot * self.com_local,
        };
        for (w, ws) in self.wheels.iter_mut().zip(&s.wheel_states) {
            w.omega = ws.omega;
            w.steer = ws.steer_rad;
            w.alpha_lag = ws.slip_angle;
            w.kappa = ws.slip_ratio;
            w.x = ws.compression_m;
            w.fz = ws.fz_n;
            w.contact = ws.contact;
            w.surface = ws.surface;
        }
        // Centre angle: mean of the steered wheels with their toe removed.
        let mut sum = 0.0;
        let mut n = 0;
        for i in 0..4 {
            let c = &self.corners[i];
            if c.steerable {
                let toe = c.toe + self.effects.toe_delta[i];
                sum += s.wheel_states[i].steer_rad - if c.left { -toe } else { toe };
                n += 1;
            }
        }
        if n > 0 {
            self.center_steer = sum / n as f32;
        }
        self.gearbox.set(s.gear);
        self.engine.omega = s.engine_rpm / RPM_PER_RADPS;
        let split = self.drive_split(0.0);
        let ratio = self.gearbox.total_ratio(&self.def.drivetrain);
        let w_in = ratio * self.axle_omega(&split);
        self.clutch_locked = ratio != 0.0 && (self.engine.omega - w_in).abs() < 1.0;
        if self.clutch_locked {
            self.clutch = 1.0;
        }
    }

    /// Teleport to `pos`/`yaw` already moving forward (chassis +Y) at `speed_mps`, wheels and
    /// engine matched. Used by `wp_world` to promote kinematic traffic to a physical vehicle
    /// without a jolt. Forward speeds only (a negative speed spins the wheels backwards in
    /// first gear). (Added by the integration lead for workstream D; reviewed by 02. When the
    /// vehicle has a Rapier body, use [`Vehicle::place_body`] instead.)
    pub fn set_pose_and_speed(&mut self, pos: [f32; 3], yaw: f32, speed_mps: f32) {
        self.reset(pos, yaw);
        let mut ch = *self.chassis();
        ch.lin_vel = ch.rot * V3::Y * speed_mps;
        self.set_chassis(ch);
        if speed_mps != 0.0 {
            self.set_rolling(speed_mps);
        }
    }

    /// Spin wheels and engine to match a forward `speed` (m/s) in a sensible gear, clutch
    /// locked. Used for rolling starts in tests and when teleporting a moving car.
    pub fn set_rolling(&mut self, speed: f32) {
        for (w, c) in self.wheels.iter_mut().zip(&self.corners) {
            w.omega = speed / c.radius;
        }
        let dt = &self.def.drivetrain;
        let wheel_omega = speed / self.corners[0].radius;
        let mut gear = 1;
        for g in 1..=dt.gear_ratios.len() as i8 {
            gear = g;
            let rpm = wheel_omega * Gearbox::ratio_of(dt, g) * dt.final_drive * RPM_PER_RADPS;
            if rpm < self.def.engine.redline_rpm * 0.75 {
                break;
            }
        }
        self.gearbox.set(gear);
        let rpm = wheel_omega * Gearbox::ratio_of(dt, gear) * dt.final_drive * RPM_PER_RADPS;
        self.engine.omega = rpm.max(self.def.engine.idle_rpm) / RPM_PER_RADPS;
        self.clutch = 1.0;
        self.clutch_locked = rpm >= self.def.engine.idle_rpm;
    }

    fn base_split(&self) -> [f32; 4] {
        match self.def.drivetrain.layout {
            DriveLayout::Fwd => [0.5, 0.5, 0.0, 0.0],
            DriveLayout::Rwd => [0.0, 0.0, 0.5, 0.5],
            DriveLayout::Awd { front_split } => [
                front_split * 0.5,
                front_split * 0.5,
                (1.0 - front_split) * 0.5,
                (1.0 - front_split) * 0.5,
            ],
            DriveLayout::AwdOnDemand {
                base_front_split,
                max_rear_split,
                ..
            } => {
                // Coupling state (0 open … 1 closed) is updated once per tick in
                // `update_awd_coupling`; the rear share moves from the base to the maximum.
                let base_rear = 1.0 - base_front_split;
                let rear =
                    (base_rear + self.awd_coupling * (max_rear_split - base_rear)).clamp(0.0, 1.0);
                let f = 1.0 - rear;
                [f * 0.5, f * 0.5, rear * 0.5, rear * 0.5]
            }
        }
    }

    /// On-demand AWD coupling (contract `DriveLayout::AwdOnDemand`): closes when the front axle
    /// out-runs the rear by more than `engage_slip` (fully at 2×), is pre-charged below 5 m/s
    /// (100% × max(throttle, 0.5)), closes fast (10/s) and opens slowly (2/s) like a
    /// hydraulic clutch pack, so it does not chatter as the slip it cures disappears.
    fn update_awd_coupling(&mut self, throttle: f32, speed: f32, dt: f32) {
        let DriveLayout::AwdOnDemand { engage_slip, .. } = self.def.drivetrain.layout else {
            self.awd_coupling = 0.0;
            return;
        };
        let lin = |i: usize| (self.wheels[i].omega * self.corners[i].radius).abs();
        let front = 0.5 * (lin(0) + lin(1));
        let rear = 0.5 * (lin(2) + lin(3));
        let slip = (front - rear) / rear.max(1.0);
        let engage = engage_slip.max(1e-3);
        let from_slip = ((slip - engage) / engage).clamp(0.0, 1.0);
        // Pre-charged while slow (like real on-demand couplings at a standstill), so a
        // torque-converter launch does not dump its stall torque on the front axle alone.
        let preload = if speed.abs() < 5.0 {
            1.0 * throttle.max(0.5)
        } else {
            0.0
        };
        let target = from_slip.max(preload);
        let rate = if target > self.awd_coupling {
            10.0
        } else {
            2.0
        };
        self.awd_coupling += (target - self.awd_coupling).clamp(-rate * dt, rate * dt);
    }

    fn drive_split(&self, handbrake: f32) -> [f32; 4] {
        let mut s = self.base_split();
        for (i, v) in s.iter_mut().enumerate() {
            if self.effects.wheel_lost[i] {
                *v = 0.0;
            }
        }
        if self.extras.handbrake_disconnects_rear && handbrake > 0.1 {
            s[2] = 0.0;
            s[3] = 0.0;
        }
        s
    }

    fn axle_omega(&self, split: &[f32; 4]) -> f32 {
        let sum: f32 = split.iter().sum();
        if sum <= 0.0 {
            return 0.0;
        }
        split
            .iter()
            .zip(&self.wheels)
            .map(|(s, w)| s * w.omega)
            .sum::<f32>()
            / sum
    }

    fn max_gear(&self) -> i8 {
        let n = self.def.drivetrain.gear_ratios.len() as i8;
        let gh = self.effects.gearbox_health;
        if gh <= 0.0 {
            0
        } else if gh < 0.25 {
            n.min(2)
        } else if gh < 0.5 {
            (n - 1).max(1)
        } else {
            n
        }
    }

    fn power_scale(&self) -> f32 {
        if self.effects.engine_dead || self.fuel <= 0.0 {
            0.0
        } else {
            (self.effects.power_scale * (1.0 - self.heat)).max(0.0)
        }
    }

    fn update_damage_state(&mut self, dt: f32, out: &mut Vec<SimEvent>) {
        if self.effects.overheating > 0.0 && self.heat < 1.0 {
            self.heat = (self.heat + self.effects.overheating * dt / OVERHEAT_TIME_S).min(1.0);
            if self.heat >= self.heat_reported + 0.1 {
                self.heat_reported = self.heat;
                out.push(SimEvent::ComponentDamaged {
                    vehicle: self.entity,
                    component: Component::Engine,
                    health: 1.0 - self.heat,
                });
            }
        }
        if self.effects.fuel_leak_rate > 0.0 {
            self.fuel = (self.fuel - self.effects.fuel_leak_rate * dt).max(0.0);
        }
    }

    /// Full step: like [`VehicleSim::step`] but probing the ground through `ground`
    /// (the Rapier caster in `wp_sim`). Computes [`Vehicle::forces`]; does not move the body.
    pub fn step_with_ground(
        &mut self,
        input: &VehicleInput,
        assists: &AssistProfile,
        world: &dyn WorldQuery,
        ground: &dyn WheelCaster,
        dt: f32,
        out: &mut Vec<SimEvent>,
    ) {
        let ch = self.chassis;
        let up = ch.rot * V3::Z;
        let fwd = ch.rot * V3::Y;
        let right = ch.rot * V3::X;
        let v_fwd = ch.lin_vel.dot(fwd);
        let v_left = -ch.lin_vel.dot(right);
        let eff = self.effects;
        let wheelbase = self.def.wheelbase_m;

        self.update_damage_state(dt, out);
        let power_scale = self.power_scale();

        // --- Pedals (auto-reverse swaps them while in reverse) ---------------------------
        let raw_thr = input.throttle.clamp(0.0, 1.0);
        let raw_brk = input.brake.clamp(0.0, 1.0);
        let (thr_in, brk_in) = if assists.auto_gears && self.gearbox.gear < 0 {
            (raw_brk, raw_thr)
        } else {
            (raw_thr, raw_brk)
        };
        self.throttle_f = filter_pedal(self.throttle_f, thr_in, assists.throttle_filter, dt);
        self.brake_f = filter_pedal(self.brake_f, brk_in, assists.brake_filter, dt);
        let handbrake = input.handbrake.clamp(0.0, 1.0);

        // --- Gearbox ------------------------------------------------------------------------
        self.update_awd_coupling(thr_in, v_fwd, dt);
        let base_split = self.drive_split(0.0);
        let axle_omega = self.axle_omega(&base_split);
        let max_gear = self.max_gear();
        if assists.auto_gears {
            self.gearbox.auto_shift(
                &self.def.drivetrain,
                self.def.engine.redline_rpm,
                axle_omega,
                raw_thr,
                raw_brk,
                v_fwd,
                max_gear,
                dt,
            );
        } else {
            let g = self.gearbox.gear;
            if input.gear_up && g < max_gear {
                self.gearbox.shift(g + 1, self.def.drivetrain.shift_time_s);
            } else if input.gear_down && g > -1 {
                self.gearbox.shift(g - 1, self.def.drivetrain.shift_time_s);
            } else if g > max_gear {
                self.gearbox
                    .shift(max_gear, self.def.drivetrain.shift_time_s);
            }
        }
        self.gearbox.update(dt);

        // --- Clutch engagement ----------------------------------------------------------
        let driven_corner = (0..4).find(|i| base_split[*i] > 0.0).unwrap_or(0);
        let ratio_now = self.gearbox.total_ratio(&self.def.drivetrain);
        self.auto_clutch = assists.auto_clutch;
        self.clutch = if self.gearbox.shifting() {
            0.0
        } else if self.converter.is_some() {
            // Torque-converter automatic: the converter replaces the launch clutch; the clutch
            // value is the lock-up clutch's availability.
            1.0
        } else if assists.auto_clutch {
            auto_clutch_update(
                self.clutch,
                // Judge "rolling" from ground speed so wheelspin does not count.
                (ratio_now * v_fwd / self.corners[driven_corner].radius).abs() * RPM_PER_RADPS,
                self.engine.rpm(),
                self.def.engine.idle_rpm,
                self.def.engine.redline_rpm,
                thr_in,
                self.launch_torque_ratio,
                dt,
            )
        } else {
            1.0 - input.clutch.clamp(0.0, 1.0)
        };

        // --- Assists ----------------------------------------------------------------------
        let driven: Vec<usize> = (0..4).filter(|i| base_split[*i] > 0.0).collect();
        // TCS watches the worst driven wheel (an open diff lets one wheel spin alone).
        let driven_slip = driven
            .iter()
            .filter(|i| self.wheels[**i].contact)
            .map(|i| self.wheels[*i].kappa)
            .fold(0.0f32, f32::max);
        let peak_slip = driven
            .first()
            .map(|i| self.corners[*i].tire.peak_slip_ratio)
            .unwrap_or(0.1);
        let tcs_mult = self.tcs.update(assists.tcs, driven_slip, peak_slip, dt);
        let yaw_rate = ch.ang_vel.dot(up);
        let esc_out = esc(assists.esc, yaw_rate, self.center_steer, v_fwd, wheelbase);
        // TCS acts on engine torque once the clutch is locked; during an automatic-clutch
        // launch (slipping) wheel torque equals clutch torque, so TCS trims the clutch instead.
        let launching = self.auto_clutch && !self.clutch_locked && self.converter.is_none();
        let throttle =
            self.throttle_f * esc_out.throttle_scale * if launching { 1.0 } else { tcs_mult };
        let clutch_trim = if launching { tcs_mult } else { 1.0 };

        // --- Steering ---------------------------------------------------------------------
        let steer_in = SteerInput {
            driver: input.steer,
            speed: v_fwd,
            body_slip: body_slip(v_fwd, v_left),
            rear_slip: 0.5 * (self.wheels[2].alpha_lag + self.wheels[3].alpha_lag),
            pull: eff.steer_pull,
            ratio_scale: eff.steer_ratio_scale.max(0.05),
        };
        let max_steer = self.corners[0].max_steer.max(self.corners[1].max_steer);
        self.center_steer = update_center_angle(
            self.center_steer,
            &steer_in,
            assists,
            max_steer,
            self.def.steering.rack_speed_rad_per_s,
            dt,
        );
        let ts = if base_split[0] > 0.0 && self.corners[0].steerable {
            torque_steer(self.wheels[0].drive, self.wheels[1].drive)
        } else {
            0.0
        };
        for i in 0..4 {
            let c = &self.corners[i];
            let track = if i < 2 {
                self.def.track_front_m
            } else {
                self.def.track_rear_m
            };
            let mut s = if c.steerable {
                ackermann(
                    self.center_steer + ts,
                    c.left,
                    c.ackermann,
                    wheelbase,
                    track,
                )
            } else {
                0.0
            };
            let toe = c.toe + eff.toe_delta[i];
            s += if c.left { -toe } else { toe };
            self.wheels[i].steer = s;
        }

        // --- Ground probe + suspension ------------------------------------------------------
        for i in 0..4 {
            self.probe_wheel(i, &ch, up, ground, world);
        }
        for (l, r) in [(0usize, 1usize), (2, 3)] {
            let f = arb_force(
                self.corners[l].susp.arb_n_per_m,
                self.wheels[l].x,
                self.wheels[r].x,
            );
            if self.wheels[l].contact && !eff.wheel_lost[l] {
                self.wheels[l].fz = (self.wheels[l].fz + f).max(0.0);
            }
            if self.wheels[r].contact && !eff.wheel_lost[r] {
                self.wheels[r].fz = (self.wheels[r].fz - f).max(0.0);
            }
        }

        // --- Contact frames ---------------------------------------------------------------
        for i in 0..4 {
            let c = &self.corners[i];
            let w = &mut self.wheels[i];
            let (s, co) = sin_cos(w.steer);
            let heading = ch.rot * V3::new(-s, co, 0.0);
            let n = if w.contact { w.normal } else { up };
            w.fwd = (heading - n * n.dot(heading)).normalize_or_zero();
            w.lat = n.cross(w.fwd);
            let vc = ch.velocity_at(if w.contact {
                w.point
            } else {
                ch.to_world(c.hub)
            });
            w.vx = vc.dot(w.fwd);
            w.vy = vc.dot(w.lat);
            let p = eff.tire_puncture[i].clamp(0.0, 1.0);
            w.r_eff = c.radius * (1.0 - 0.3 * p);
            let camber = c.camber + eff.camber_delta[i];
            let camber_grip = (1.0 - (camber.abs() - 0.03).max(0.0)).clamp(0.6, 1.0);
            w.grip = surface_grip(&c.tire, w.surface)
                * eff.grip_scale[i].max(0.0)
                * (1.0 - 0.5 * p)
                * camber_grip;
        }

        // --- Brakes -----------------------------------------------------------------------
        let mut brake = service_torques(&self.def.brakes, self.brake_f);
        for (b, s) in brake.iter_mut().zip(eff.brake_scale) {
            *b *= s.max(0.0);
        }
        if assists.abs {
            let kappas = std::array::from_fn(|i| self.wheels[i].kappa);
            let vxs = std::array::from_fn(|i| self.wheels[i].vx);
            let peaks = std::array::from_fn(|i| self.corners[i].tire.peak_slip_ratio);
            self.abs.apply(&mut brake, kappas, vxs, peaks, dt);
        }
        brake[2] += handbrake * self.def.brakes.handbrake_rear_nm;
        brake[3] += handbrake * self.def.brakes.handbrake_rear_nm;
        for (i, b) in brake.iter_mut().enumerate() {
            *b += esc_out.brake[i];
            self.wheels[i].brake = *b;
        }

        // --- Sub-stepped drivetrain, wheel spin and tire forces ----------------------------
        let n_sub = VEHICLE_SUBSTEPS.max(1);
        let h = dt / n_sub as f32;
        let split = self.drive_split(handbrake);
        let mut acc_fx = [0.0f32; 4];
        let mut acc_fy = [0.0f32; 4];
        let mut acc_drive = [0.0f32; 4];
        for _ in 0..n_sub {
            let drive = self.substep(throttle, clutch_trim, power_scale, &split, h);
            for i in 0..4 {
                acc_fx[i] += self.wheels[i].fx;
                acc_fy[i] += self.wheels[i].fy;
                acc_drive[i] += drive[i];
            }
        }
        let inv = 1.0 / n_sub as f32;

        // --- Chassis force & torque ------------------------------------------------------
        let mut force = V3::ZERO;
        let mut torque = V3::ZERO;
        for i in 0..4 {
            let c = &self.corners[i];
            let w = &mut self.wheels[i];
            w.fx = acc_fx[i] * inv;
            w.fy = acc_fy[i] * inv;
            w.drive = acc_drive[i] * inv;
            if !w.contact {
                w.mz = 0.0;
                continue;
            }
            let f = if eff.wheel_lost[i] {
                w.hub_force
            } else {
                let camber = c.camber + eff.camber_delta[i];
                let side = if c.left { 1.0 } else { -1.0 };
                let thrust = side * CAMBER_THRUST * camber * w.fz;
                w.mz = -pneumatic_trail(&c.tire, w.alpha_lag) * w.fy;
                up * w.fz + w.fwd * w.fx + w.lat * (w.fy + thrust)
            };
            force += f;
            torque += (w.point - ch.com).cross(f);
        }
        let aero = aero_forces(
            ch.lin_vel,
            fwd,
            self.def.drag_cd_a_m2,
            eff.drag_scale,
            self.def.lift_front - self.def.aero_downforce_coeffs[0],
            self.def.lift_rear - self.def.aero_downforce_coeffs[1],
        );
        force += aero.drag;
        for (y, lift) in [
            (self.def.wheels[0].position_m[1], aero.lift_front),
            (self.def.wheels[2].position_m[1], aero.lift_rear),
        ] {
            let p = ch.to_world(V3::new(0.0, y, self.def.cg_height_m));
            let f = up * lift;
            force += f;
            torque += (p - ch.com).cross(f);
        }
        self.force = force;
        self.torque = torque;

        // --- FFB & telemetry -------------------------------------------------------------
        let front = std::array::from_fn(|i| FfbWheel {
            fy: self.wheels[i].fy,
            mz: self.wheels[i].mz,
            caster_trail: self.corners[i].radius * self.corners[i].caster,
        });
        self.ffb = ffb::compute(
            front,
            self.def.steering.ratio,
            self.def.steering.ffb_scale,
            self.wheels.iter().map(|w| w.vcomp.abs()).sum(),
            std::array::from_fn(|i| self.wheels[i].surface),
            v_fwd,
        );
        self.telemetry = Telemetry {
            throttle,
            brake: self.brake_f,
            clutch: self.clutch,
            clutch_locked: self.clutch_locked,
            steer_rad: self.center_steer,
            tcs_cut: self.tcs.cut,
            abs_active: self.abs.active(),
            esc_active: esc_out.brake.iter().any(|b| *b > 0.0),
            fx: std::array::from_fn(|i| self.wheels[i].fx),
            fy: std::array::from_fn(|i| self.wheels[i].fy),
            engine_heat: self.heat,
            fuel: self.fuel,
        };
    }

    /// Probe the ground under wheel `i` and compute its strut (or hub) force.
    fn probe_wheel(
        &mut self,
        i: usize,
        ch: &Chassis,
        up: V3,
        ground: &dyn WheelCaster,
        world: &dyn WorldQuery,
    ) {
        let c = &self.corners[i];
        let lost = self.effects.wheel_lost[i];
        let w = &mut self.wheels[i];
        let down = -up;
        w.hub_force = V3::ZERO;
        if lost {
            let origin = ch.to_world(c.hub + V3::Z * 0.3);
            let max = 0.3 + HUB_RADIUS_M;
            w.omega = 0.0;
            w.x = 0.0;
            match ground.cast(origin, down, max) {
                Some(hit) => {
                    let vel = ch.velocity_at(hit.point);
                    w.vcomp = -vel.dot(up);
                    let fz =
                        (HUB_STIFFNESS * (max - hit.distance) + HUB_DAMPING * w.vcomp).max(0.0);
                    let v_t = vel - hit.normal * vel.dot(hit.normal);
                    let friction = -v_t * (HUB_FRICTION * fz / v_t.length().max(0.5));
                    w.contact = true;
                    w.point = hit.point;
                    w.normal = hit.normal;
                    w.fz = fz;
                    w.surface = world.surface_at(arr(hit.point));
                    w.hub_force = hit.normal * fz + friction;
                }
                None => {
                    w.contact = false;
                    w.fz = 0.0;
                }
            }
            return;
        }
        let travel = c.susp.travel_m;
        let top = c.hub + V3::Z * (travel - c.x_static);
        let origin = ch.to_world(top);
        let max = travel + c.radius;
        match ground.cast(origin, down, max) {
            Some(hit) => {
                let x = max - hit.distance;
                let hub_world = origin + down * (hit.distance - c.radius);
                w.vcomp = -ch.velocity_at(hub_world).dot(up);
                w.x = x;
                w.contact = true;
                w.point = hit.point;
                w.normal = hit.normal;
                w.surface = world.surface_at(arr(hit.point));
                w.fz = strut_force(&c.susp, x, w.vcomp);
            }
            None => {
                w.contact = false;
                w.x = 0.0;
                w.vcomp = 0.0;
                w.fz = 0.0;
                w.surface = SurfaceType::default();
            }
        }
    }

    /// One drivetrain/wheel sub-step of length `h`; `clutch_trim` scales clutch capacity
    /// (launch-phase TCS). Returns the drive torque given to each
    /// wheel.
    fn substep(
        &mut self,
        throttle: f32,
        clutch_trim: f32,
        power_scale: f32,
        split: &[f32; 4],
        h: f32,
    ) -> [f32; 4] {
        let ratio = self.gearbox.total_ratio(&self.def.drivetrain);
        let s_sum: f32 = split.iter().sum();
        let axle = self.axle_omega(split);
        let omega_in = ratio * axle;
        let idle_w = self.def.engine.idle_rpm / RPM_PER_RADPS;
        let anti_stall = ((self.engine.omega - 0.6 * idle_w) / (0.4 * idle_w)).clamp(0.0, 1.0);
        let converter = self.converter;
        let cap = if converter.is_some() {
            self.def.drivetrain.clutch_max_nm * self.clutch
        } else {
            self.def.drivetrain.clutch_max_nm * self.clutch * anti_stall * clutch_trim
        };
        let connected = ratio != 0.0 && s_sum > 0.0 && cap > 1.0;
        // The automatic clutch only locks when fully engaged (otherwise it is launching); the
        // converter's lock-up clutch only above (90% of) its coupling rpm.
        let may_lock = match converter {
            Some(c) => self.engine.rpm() >= 0.9 * c.def.coupling_rpm,
            None => !self.auto_clutch || self.clutch >= 0.99,
        };
        if !connected || !may_lock {
            self.clutch_locked = false;
        }
        let i_e = self.def.engine.inertia_kgm2;
        let t_e = self.engine.torque(throttle, power_scale, h);

        // Converter in fluid mode: torques plus their stiffness wrt engine and turbine speed,
        // treated semi-implicitly (engine side below, wheel side via `conv_damp`).
        let mut conv_engine_k = 0.0;
        let mut conv_damp = 0.0;
        let (wheel_side, reflected, t_clutch) = if connected && self.clutch_locked {
            (t_e * ratio, i_e * ratio * ratio, t_e)
        } else if let (true, Some(c)) = (connected, converter) {
            let w_e = self.engine.omega;
            let (t_p, t_t) = c.torques(w_e, omega_in);
            let eps = 0.5;
            conv_engine_k = ((c.torques(w_e + eps, omega_in).0 - c.torques(w_e - eps, omega_in).0)
                / (2.0 * eps))
                .max(0.0);
            // d(turbine torque)/d(turbine speed) is ≤ 0: a damper on the wheels, × ratio².
            conv_damp = (-(c.torques(w_e, omega_in + eps).1 - c.torques(w_e, omega_in - eps).1)
                / (2.0 * eps))
                .max(0.0)
                * ratio
                * ratio;
            (t_t * ratio, 0.0, t_p)
        } else if connected {
            let d = self.engine.omega - omega_in;
            let tc = if d.abs() < 1e-4 {
                t_e.clamp(-cap, cap)
            } else {
                cap * d.signum()
            };
            (tc * ratio, 0.0, tc)
        } else {
            (0.0, 0.0, 0.0)
        };

        // Losses only reduce torque flowing engine → wheels.
        let wheel_side = if wheel_side * ratio > 0.0 {
            wheel_side
                * self
                    .def
                    .drivetrain
                    .efficiency
                    .unwrap_or(DRIVETRAIN_EFFICIENCY)
        } else {
            wheel_side
        };
        let mut drive = [0.0f32; 4];
        if s_sum > 0.0 {
            for i in 0..4 {
                drive[i] = wheel_side * split[i] / s_sum;
            }
        }
        let diffs = [
            (0usize, 1usize, self.def.drivetrain.diff_front),
            (2, 3, self.def.drivetrain.diff_rear),
        ];
        for (l, r, diff) in diffs {
            if split[l] + split[r] <= 0.0 || matches!(diff, DiffDef::Open) {
                continue;
            }
            let k_eq = (self.corners[l].inertia + reflected * split[l] / s_sum) / h * 0.5;
            let x = diff_transfer(
                &diff,
                drive[l] + drive[r],
                self.wheels[l].omega,
                self.wheels[r].omega,
                k_eq,
            );
            drive[l] -= x;
            drive[r] += x;
        }

        for i in 0..4 {
            let c = &self.corners[i];
            let w = &mut self.wheels[i];
            if self.effects.wheel_lost[i] {
                w.fx = 0.0;
                w.fy = 0.0;
                continue;
            }
            let share = if s_sum > 0.0 { split[i] / s_sum } else { 0.0 };
            let inertia = c.inertia + reflected * share + h * conv_damp * share;
            if !w.contact {
                let mut om = w.omega + h * drive[i] / inertia;
                let db = h * w.brake / inertia;
                om = if om.abs() <= db {
                    0.0
                } else {
                    om - om.signum() * db
                };
                w.omega = om;
                w.fx = 0.0;
                w.fy = 0.0;
                w.kappa = 0.0;
                w.alpha = 0.0;
                w.alpha_lag = 0.0;
                continue;
            }
            w.alpha = slip_angle(w.vx, w.vy);
            w.alpha_lag = relax(w.alpha_lag, w.alpha, w.vx, c.tire.relaxation_length_m, h);
            let (vx, r, fz, grip, alpha) = (w.vx, w.r_eff, w.fz, w.grip, w.alpha_lag);
            let fx_at =
                |om: f32| combined_forces(&c.tire, slip_ratio(om, r, vx), alpha, fz, grip).0;
            let fx0 = fx_at(w.omega);
            let eps = 0.002 * vx.abs().max(SLIP_RATIO_V_MIN) / r;
            let k = ((fx_at(w.omega + eps) - fx_at(w.omega - eps)) / (2.0 * eps)).max(0.0);
            let i_eff = inertia + h * k * r;
            let mut om = w.omega + h * (drive[i] - fx0 * r) / i_eff;
            let db = h * w.brake / i_eff;
            om = if om.abs() <= db {
                0.0
            } else {
                om - om.signum() * db
            };
            w.omega = om;
            w.kappa = slip_ratio(om, r, vx);
            let (fx, fy) = combined_forces(&c.tire, w.kappa, alpha, fz, grip);
            let mu_y = load_sensitive_mu(
                c.tire.lateral.d,
                c.tire.load_sensitivity,
                fz.max(1.0),
                c.tire.fz0_n,
            );
            w.fy = low_speed_lateral(fy, vx, w.vy, fz, mu_y * grip);
            w.fx = fx - c.tire.rolling_resistance * fz * (vx / 0.5).clamp(-1.0, 1.0);
        }

        // Engine side.
        if connected && self.clutch_locked {
            let new_axle = self.axle_omega(split);
            let w_e = ratio * new_axle;
            let tc = t_e - i_e * (w_e - self.engine.omega) / h;
            self.engine.omega = w_e;
            if tc.abs() > cap * 1.05 || w_e < 0.6 * idle_w {
                self.clutch_locked = false;
            }
        } else if let (true, Some(c)) = (connected, converter) {
            self.engine.omega += h * (t_e - t_clutch) / (i_e + h * conv_engine_k);
            let new_in = ratio * self.axle_omega(split);
            let sr = new_in / self.engine.omega.max(1e-3);
            if self.engine.rpm() >= c.def.coupling_rpm && sr >= 0.92 && cap >= t_e.abs() {
                self.clutch_locked = true;
                self.engine.omega = new_in;
            }
        } else if connected {
            let d0 = self.engine.omega - omega_in;
            self.engine.omega += h * (t_e - t_clutch) / i_e;
            let new_in = ratio * self.axle_omega(split);
            let d1 = self.engine.omega - new_in;
            if may_lock && d0 * d1 <= 0.0 && cap >= t_e.abs() && new_in >= 0.6 * idle_w {
                self.clutch_locked = true;
                self.engine.omega = new_in;
            }
        } else {
            self.engine.omega += h * t_e / i_e;
        }
        self.engine.omega = self.engine.omega.max(0.0);
        drive
    }
}

impl VehicleSim for Vehicle {
    /// Computes forces against a flat ground plane at z = 0. The rigid body is integrated by
    /// Rapier — inside `wp_sim` use [`Vehicle::step_with_ground`] with a Rapier caster,
    /// [`Vehicle::write_body`] and [`Vehicle::read_body`] (see the crate docs). Without a body
    /// the vehicle does not move.
    fn step(
        &mut self,
        input: &VehicleInput,
        assists: &AssistProfile,
        world: &dyn WorldQuery,
        ground: &dyn wp_contracts::GroundQuery,
        dt: f32,
        out: &mut Vec<SimEvent>,
    ) {
        let caster = crate::ground::ContractGround(ground);
        self.step_with_ground(input, assists, world, &caster, dt, out);
    }

    fn state(&self) -> VehicleState {
        let ch = &self.chassis;
        let fwd = ch.rot * V3::Y;
        let wheel_states = std::array::from_fn(|i| {
            let w = &self.wheels[i];
            WheelState {
                omega: w.omega,
                steer_rad: w.steer,
                compression_m: w.x,
                slip_angle: w.alpha_lag,
                slip_ratio: w.kappa,
                fz_n: w.fz,
                surface: w.surface,
                contact: w.contact,
                detached: self.effects.wheel_lost[i],
                fx_n: w.fx,
                fy_n: w.fy,
            }
        });
        VehicleState {
            pos: arr(ch.pos),
            rot: [ch.rot.x, ch.rot.y, ch.rot.z, ch.rot.w],
            lin_vel: arr(ch.lin_vel),
            ang_vel: arr(ch.ang_vel),
            wheel_states,
            engine_rpm: self.engine.rpm(),
            gear: self.gearbox.gear,
            speed_mps: ch.lin_vel.dot(fwd),
            health: 1.0,
            component_health: Default::default(),
            mode: Default::default(),
        }
    }

    fn apply_damage_effects(&mut self, eff: &HandlingEffects) {
        self.effects = *eff;
    }

    fn ffb(&self) -> FfbOutput {
        self.ffb
    }
}
