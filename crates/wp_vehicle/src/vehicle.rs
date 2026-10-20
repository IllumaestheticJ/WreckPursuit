//! The vehicle: per-tick orchestration of input → assists → gearbox → steering → ground probe
//! → suspension → brakes → (sub-stepped) drivetrain + wheel spin + tires → chassis force and
//! torque. The chassis itself is a Rapier rigid body (see [`crate::body`]): this module only
//! *computes* forces from the chassis state it was given; Rapier integrates them and resolves
//! collisions.

use wp_contracts::{
    AssistProfile, Component, DriveLayout, EntityId, FfbOutput, HandlingEffects, SimEvent,
    SurfaceType, SuspensionDef, TireDef, VehicleDef, VehicleInput, VehicleSim, VehicleState,
    WheelState, WorldQuery, GRAVITY_MPS2, VEHICLE_SUBSTEPS,
};

use crate::aero::aero_forces;
use crate::assists::{
    btc_rear_authority, esc_reference, filter_pedal, BtcAxle, Esc, Tcs, BTC_MAX_FRAC,
    TCS_BRAKE_ACTIVE_NM, TCS_BRAKE_HANDOFF,
};
use crate::brakes::{service_torques, Abs};
use crate::diff::{axle_transfer, centre_transfer, AxleDiff, CentreDiff, DiffInput, DiffSetup};
use crate::drivetrain::{auto_clutch_update, Converter, Gearbox, ShiftInputs};
use crate::engine::{Engine, EngineExtras, RPM_PER_RADPS};
use crate::ffb::{self, FfbWheel};
use crate::ground::{WheelCaster, WheelProbe};
use crate::launch::LaunchGuard;
use crate::math::{arr, sin_cos, v3, yaw_quat, Quat, V3};
use crate::slide::{slide_class_with_rear_lock, slide_throttle};
use crate::steering::{
    ackermann, body_slip, front_axle_angle, rear_axle_slip, steer_target, torque_steer,
    update_center_angle, SteerInput, HANDBRAKE_GRACE_S,
};
use crate::surface::{conditions_grip, ConditionsSource, SurfaceConditions, TireWetProfile};
use crate::suspension::{arb_force, static_compression, step_strut, TYRE_STATIC_DEFLECTION_M};
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
/// Handbrake input above which the handbrake counts as pulled (declutch, steering assists).
pub const HANDBRAKE_ON: f32 = 0.1;
/// Above this forward speed (m/s) a handbrake declutch keeps the selected gear (no automatic
/// shifts); below it the automatic box selects gears as usual.
pub const HB_KEEP_GEAR_MPS: f32 = 1.0;
/// The automatic clutch opens during a shift, off the throttle, when the driven wheels turn
/// the clutch slower than this fraction of idle (the same threshold as
/// `drivetrain::auto_clutch_update`'s "rolling").
const AUTO_CLUTCH_OPEN_FRAC: f32 = 1.2;
/// Manual clutch: the engine stalls below this fraction of its idle speed (row 59).
pub const STALL_FRAC: f32 = 0.5;
/// Manual clutch: pressing the pedal at least this far re-cranks a stalled engine.
pub const RECRANK_PEDAL: f32 = 0.9;
/// The tyre sweep starts this far above the top of the travel, m (see `probe_wheel`).
const PROBE_LIFT_M: f32 = 0.3;
/// Fraction of the tyre's half width used by the sweep (the shoulder is rounded).
const PROBE_WIDTH_FRAC: f32 = 0.8;
/// Normal used for the tyre's force frame: the ground normal, leaned back toward the strut
/// axis `up` when the contact is a steep edge (a kerb corner reports a normal pointing back
/// at the wheel; driving and braking forces along it would turn the kerb into a ramp).
fn tyre_normal(n: V3, up: V3) -> V3 {
    let w = ((n.dot(up) - 0.6) / 0.25).clamp(0.0, 1.0);
    let t = (up + (n - up) * w).normalize_or_zero();
    if t == V3::ZERO {
        up
    } else {
        t
    }
}
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
    /// Differential override (tests, tuning, mods). `None` = the car's own setup
    /// ([`DiffSetup::for_car`]: `data/cars/differentials.toml`, else the car file).
    pub diffs: Option<DiffSetup>,
}

impl Default for VehicleExtras {
    fn default() -> Self {
        Self {
            engine: EngineExtras::default(),
            handbrake_disconnects_rear: true,
            diffs: None,
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
    pub half_width: f32,
    /// Vertical tyre rate, N/m (from the static corner load).
    pub tyre_rate: f32,
    pub inertia: f32,
    pub steerable: bool,
    pub max_steer: f32,
    pub ackermann: f32,
    pub camber: f32,
    pub toe: f32,
    pub caster: f32,
    pub tire: TireDef,
    pub wet: TireWetProfile,
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
    /// Where the wheel centre sits relative to the ground contact, as a compression (m): the
    /// strut compression plus how much more than statically the tyre is squashed. This is
    /// what `VehicleState::compression_m` reports (a rigid-tyre renderer then never draws the
    /// wheel into the road); `x` itself is the strut's.
    pub x_wheel: f32,
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
    /// Per-wheel aquaplaning 0 (full contact) … 1 (riding on water), from
    /// [`Vehicle::set_surface_conditions`].
    pub aquaplane: [f32; 4],
    /// Torque each differential moved this tick (front, rear, centre), N·m: half the
    /// left-right (or front-rear) wheel-torque difference, + = toward the right (rear) side.
    pub diff_transfer_nm: [f32; 3],
    /// Brake-based traction control torque per wheel, N·m.
    pub tcs_brake_nm: [f32; 4],
    /// Upward speed the contact launch limiter took back this tick, m/s ([`crate::launch`]).
    pub contact_lift_cut_mps: f32,
    /// Engine stalled (manual clutch; press the clutch to re-crank).
    pub stalled: bool,
    /// Drift assist (Drift Lab B): mode, slide angle, envelope.
    pub drift: crate::drift::DriftTelemetry,
    /// A slide is confirmed ([`crate::steering::SlideGate`], row 102): the steering's slide
    /// help is acting. The Drift Lab readout's "sliding: yes / no".
    pub sliding: bool,
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
    pub(crate) diffs: DiffSetup,
    pub(crate) center_steer: f32,
    /// Slide-assist class from the car data (constant; see [`crate::slide::slide_class`]).
    pub(crate) slide_class: f32,
    pub(crate) throttle_f: f32,
    pub(crate) brake_f: f32,
    pub(crate) tcs: Tcs,
    pub(crate) esc: Esc,
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
    pub(crate) conditions: [SurfaceConditions; 4],
    pub(crate) conditions_source: ConditionsSource,
    pub(crate) launch: LaunchGuard,
    /// Handbrake hold for the steering assists: [`crate::steering::HANDBRAKE_GRACE_S`] while
    /// the handbrake is pulled, then counting down to 0 after release, s.
    pub(crate) hb_grace: f32,
    /// Engine stalled (manual clutch only, see [`STALL_FRAC`]).
    pub(crate) stalled: bool,
    /// Drift-assist state ([`crate::drift`]).
    pub(crate) drift: crate::drift::DriftState,
    /// This tick's drift-assist command (recomputed every step from `drift`).
    pub(crate) drift_cmd: crate::drift::DriftCommand,
    /// Confirmed-slide gate of the steering assists (row 102).
    pub(crate) slide_gate: crate::steering::SlideGate,
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
                half_width: 0.5 * w.width_m,
                tyre_rate: load.max(500.0) / TYRE_STATIC_DEFLECTION_M,
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
                wet: TireWetProfile::new(&tires[&w.tire_id], w.width_m),
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
        let diffs = extras.diffs.unwrap_or_else(|| DiffSetup::for_car(def));
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
            diffs,
            center_steer: 0.0,
            slide_class: slide_class_with_rear_lock(def, diffs.rear.power_lock()),
            throttle_f: 0.0,
            brake_f: 0.0,
            tcs: Tcs::default(),
            esc: Esc::default(),
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
            conditions: [SurfaceConditions::DRY; 4],
            conditions_source: ConditionsSource::World,
            launch: LaunchGuard::default(),
            hb_grace: 0.0,
            drift: crate::drift::DriftState::default(),
            drift_cmd: crate::drift::DriftCommand::default(),
            slide_gate: crate::steering::SlideGate::default(),
            stalled: false,
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
                x_wheel: c.x_static,
                r_eff: c.radius,
                grip: 1.0,
                ..Wheel::default()
            };
        }
    }

    /// Set the road conditions under each wheel (FL, FR, RL, RR) by hand and switch to
    /// [`ConditionsSource::Manual`]: the world's weather (`WorldQuery::weather_at`) is ignored
    /// until [`Vehicle::use_world_weather`]. They are an input, like the driver's: they stay until
    /// changed and are not part of [`Vehicle::snapshot`].
    pub fn set_surface_conditions(&mut self, conditions: [SurfaceConditions; 4]) {
        self.conditions = conditions;
        self.conditions_source = ConditionsSource::Manual;
    }

    /// Set the road conditions under one wheel (0 FL, 1 FR, 2 RL, 3 RR; others ignored) and
    /// switch to [`ConditionsSource::Manual`] (see [`Vehicle::set_surface_conditions`]).
    pub fn set_wheel_conditions(&mut self, wheel: usize, conditions: SurfaceConditions) {
        if let Some(c) = self.conditions.get_mut(wheel) {
            *c = conditions;
            self.conditions_source = ConditionsSource::Manual;
        }
    }

    /// Go back to reading the world's weather (`WorldQuery::weather_at` at each contact point,
    /// every step) — the default for a new vehicle.
    pub fn use_world_weather(&mut self) {
        self.conditions_source = ConditionsSource::World;
    }

    /// Where the road conditions come from (world weather by default).
    pub fn conditions_source(&self) -> ConditionsSource {
        self.conditions_source
    }

    /// The road conditions currently set per wheel.
    pub fn surface_conditions(&self) -> &[SurfaceConditions; 4] {
        &self.conditions
    }

    /// The differentials in use (see [`crate::diff`]).
    pub fn diffs(&self) -> &DiffSetup {
        &self.diffs
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
        self.esc = Esc::default();
        self.abs = Abs::default();
        self.heat = 0.0;
        self.heat_reported = 0.0;
        self.fuel = 1.0;
        self.force = V3::ZERO;
        self.torque = V3::ZERO;
        self.launch.forget();
        self.hb_grace = 0.0;
        self.drift = crate::drift::DriftState::default();
        self.drift_cmd = crate::drift::DriftCommand::default();
        self.slide_gate = crate::steering::SlideGate::default();
        self.stalled = false;
    }

    /// Forget the chassis's previous motion for the contact launch limiter
    /// ([`crate::launch`]): call after changing the body's velocity directly (outside
    /// [`Vehicle::place_body`] / [`Vehicle::reset`] / [`Vehicle::set_state`]), so the change is
    /// not mistaken for a contact.
    pub fn forget_motion(&mut self) {
        self.launch.forget();
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
        self.launch.forget();
        // A stalled engine reports 0 rpm; anything turning is running.
        self.stalled = s.engine_rpm < STALL_FRAC * self.def.engine.idle_rpm;
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
            w.x_wheel = ws.compression_m;
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
        // ESC reference (not in the contract state): assume it had caught up with the steering.
        let v_fwd = self.chassis.lin_vel.dot(self.chassis.rot * V3::Y);
        self.esc.reference = esc_reference(
            self.center_steer,
            v_fwd,
            self.def.wheelbase_m,
            self.front_mu(),
        );
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
        self.stalled = false;
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
                base_front_split, ..
            } if self.diffs.centre != CentreDiff::Default => {
                // A centre diff from the table replaces the clutch coupling: static split.
                let f = base_front_split.clamp(0.0, 1.0);
                [f * 0.5, f * 0.5, (1.0 - f) * 0.5, (1.0 - f) * 0.5]
            }
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
        if self.diffs.centre != CentreDiff::Default {
            self.awd_coupling = 0.0;
            return;
        }
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

    /// Steering-model inputs for driver steer `driver` in the current state.
    fn steer_input(&self, driver: f32, v_fwd: f32, v_left: f32, rear_axle: f32) -> SteerInput {
        let eff = self.effects;
        SteerInput {
            driver,
            speed: v_fwd,
            body_slip: body_slip(v_fwd, v_left),
            rear_slip: 0.5 * (self.wheels[2].alpha_lag + self.wheels[3].alpha_lag),
            rear_axle_slip: rear_axle,
            slide_class: self.slide_class,
            pull: eff.steer_pull,
            ratio_scale: eff.steer_ratio_scale.max(0.05),
            wheelbase: self.def.wheelbase_m,
            front_mu: self.front_mu(),
            front_peak_slip: self.corners[0].tire.peak_slip_angle_rad,
            front_axle_angle: front_axle_angle(
                v_fwd,
                v_left,
                self.chassis.ang_vel.dot(self.chassis.rot * V3::Z),
                self.def.wheels[0].position_m[1] - self.com_local.y,
            ),
            handbrake_hold: self.hb_grace > 0.0,
            rear_slip_rate: self.drift.rear_slip_rate,
            tuning: crate::drift::tuning(),
            drift: self.drift_cmd,
            slide_gate: self.slide_gate.amount,
            align_travel: self.slide_gate.travel,
        }
    }

    /// Front tyres' available friction: lateral peak μ × the mean front grip multiplier from
    /// the last tick (surface, weather, damage), floored at 0.1.
    fn front_mu(&self) -> f32 {
        let grip = 0.5 * (self.wheels[0].grip + self.wheels[1].grip);
        (self.corners[0].tire.lateral.d * grip).max(0.1)
    }

    fn max_steer(&self) -> f32 {
        self.corners[0].max_steer.max(self.corners[1].max_steer)
    }

    /// Peak slip angle of wheel `i`'s tire, rad.
    pub fn tire_peak_slip(&self, i: usize) -> f32 {
        self.corners[i].tire.peak_slip_angle_rad
    }

    /// Centre road-wheel angle (rad, + = left) the steering would head for if the driver
    /// held steer input `driver` (-1..=1) now, under `assists` (see
    /// [`crate::steering::steer_target`]). For tools and test drivers that need to know what
    /// an input does; it does not change the vehicle.
    pub fn steer_target_for(&self, driver: f32, assists: &AssistProfile) -> f32 {
        let ch = self.chassis;
        let up = ch.rot * V3::Z;
        let v_fwd = ch.lin_vel.dot(ch.rot * V3::Y);
        let v_left = -ch.lin_vel.dot(ch.rot * V3::X);
        let cg_to_rear = self.com_local.y - self.def.wheels[2].position_m[1];
        let rear_axle = rear_axle_slip(v_fwd, v_left, ch.ang_vel.dot(up), cg_to_rear);
        let inp = self.steer_input(driver, v_fwd, v_left, rear_axle);
        steer_target(self.center_steer, &inp, assists, self.max_steer()).angle
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

    /// How much of the tires' lateral grip is in use, 0..1 (`Σ|Fy| / Σ μ·Fz·grip` over wheels
    /// in contact, last tick). The automatic box holds its gear above ~0.75.
    fn lateral_usage(&self) -> f32 {
        let (mut used, mut cap) = (0.0f32, 0.0f32);
        for (w, c) in self.wheels.iter().zip(&self.corners) {
            if w.contact {
                used += w.fy.abs();
                cap += c.tire.lateral.d * w.fz.max(0.0) * w.grip;
            }
        }
        if cap > 1.0 {
            (used / cap).min(1.0)
        } else {
            0.0
        }
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
        // Contact launch limiter: upward speed Rapier's contacts added since last step.
        let launch_cut = self
            .launch
            .update(ch.lin_vel.z, self.force.z, self.def.mass_kg, dt);

        // --- Pedals (auto-reverse swaps them while in reverse) ---------------------------
        let raw_thr = input.throttle.clamp(0.0, 1.0);
        let raw_brk = input.brake.clamp(0.0, 1.0);
        let (thr_in, brk_in) = if assists.auto_gears && self.gearbox.gear < 0 {
            (raw_brk, raw_thr)
        } else {
            (raw_thr, raw_brk)
        };
        // Rear-axle slip angle (slide assists): CG-to-rear-axle distance from the car data.
        let cg_to_rear = self.com_local.y - self.def.wheels[2].position_m[1];
        let rear_axle = rear_axle_slip(v_fwd, v_left, ch.ang_vel.dot(up), cg_to_rear);
        self.drift.update_slip(rear_axle, dt);
        let tuning = crate::drift::tuning();
        let (cs_side, onset) =
            crate::steering::onset_of(rear_axle, self.drift.rear_slip_rate, &tuning);
        // Row 102: the onset only counts once the slide is confirmed.
        // The drift intent modes keep the raw onset (they act from 3–7° of body slip, and a
        // drift entry with the key held into the turn must not wait for the gate).
        self.slide_gate.update(
            rear_axle,
            front_axle_angle(
                v_fwd,
                v_left,
                ch.ang_vel.dot(up),
                self.def.wheels[0].position_m[1] - self.com_local.y,
            ),
            self.center_steer,
            v_fwd,
            &tuning,
            dt,
        );
        self.drift_cmd = self.drift.update(
            &crate::drift::DriftInputs {
                driver: input.steer,
                throttle: thr_in,
                speed: v_fwd,
                body_slip: body_slip(v_fwd, v_left),
                side: cs_side,
                onset,
                help: assists.countersteer_help,
                sensitivity: assists.steer_speed_sensitivity,
                handbrake_hold: self.hb_grace > 0.0 || input.handbrake > HANDBRAKE_ON,
            },
            &tuning,
            dt,
        );
        let st = slide_throttle(assists, self.slide_class, rear_axle, input.steer);
        // `scale` is exactly 1.0 unless the slide throttle is active (bit-identical otherwise).
        let thr_target = if st.scale < 1.0 {
            thr_in * st.scale
        } else {
            thr_in
        };
        // Drift assist throttle shaping (exactly 1.0 when inactive: bit-identical otherwise).
        let dc = self.drift_cmd;
        let thr_target = if dc.throttle_scale < 1.0 {
            thr_target * dc.throttle_scale
        } else {
            thr_target
        };
        let prev_thr = self.throttle_f;
        self.throttle_f = filter_pedal(self.throttle_f, thr_target, st.filter, dt);
        if dc.lift_time_s > 0.0 && self.throttle_f < prev_thr {
            // Abrupt lift in a slide: softened (a full lift takes `lift_time_s`), not cancelled.
            self.throttle_f = self.throttle_f.max(prev_thr - dt / dc.lift_time_s);
        }
        if dc.rise_time_s > 0.0 && self.throttle_f > prev_thr {
            // In a slide the throttle rises no faster than 0 → 1 in `rise_time_s`.
            self.throttle_f = self.throttle_f.min(prev_thr + dt / dc.rise_time_s);
        }
        self.brake_f = filter_pedal(self.brake_f, brk_in, assists.brake_filter, dt);
        let handbrake = input.handbrake.clamp(0.0, 1.0);
        // Handbrake hold for the assists (steering alignment, ESC): on while pulled and for
        // `HANDBRAKE_GRACE_S` after release, so a handbrake turn is not fought.
        let hb_was_held = self.hb_grace > 0.0;
        self.hb_grace = if handbrake > HANDBRAKE_ON {
            HANDBRAKE_GRACE_S
        } else {
            (self.hb_grace - dt).max(0.0)
        };
        let hb_hold = self.hb_grace > 0.0;

        // --- Gearbox ------------------------------------------------------------------------
        self.update_awd_coupling(thr_in, v_fwd, dt);
        let base_split = self.drive_split(0.0);
        let axle_omega = self.axle_omega(&base_split);
        let max_gear = self.max_gear();
        // Handbrake declutch (owner rows 56/72): with an automatic clutch or a converter box,
        // a pulled handbrake decouples the whole driveline (as if the clutch pedal were down),
        // so the rears lock, the engine neither drags them nor stalls, AWD fronts do not pull
        // the rears round, and the selected gear is kept. A manual clutch pedal is never
        // overridden (`handbrake_disconnects_rear` still frees the rear axle there).
        let hb_declutch =
            handbrake > HANDBRAKE_ON && (assists.auto_clutch || self.converter.is_some());
        // The gear is kept only while moving: at a standstill on the handbrake the automatic
        // box still selects first / reverse (the AI's crawl holds the handbrake and touches the
        // throttle to engage first, `wp_world::manoeuvre::Crawl`).
        let hb_keep_gear = hb_declutch && v_fwd.abs() > HB_KEEP_GEAR_MPS;
        if assists.auto_gears && !hb_keep_gear {
            let inputs = ShiftInputs {
                redline_rpm: self.def.engine.redline_rpm,
                axle_omega,
                throttle: raw_thr,
                brake: raw_brk,
                speed: v_fwd,
                max_gear,
                lateral_usage: self.lateral_usage(),
            };
            self.gearbox.auto_shift(&self.def.drivetrain, &inputs, dt);
        } else if !assists.auto_gears {
            let g = self.gearbox.gear;
            // A destroyed gearbox (max_gear 0) has no reverse either.
            let lowest = if max_gear >= 1 { -1 } else { 0 };
            if input.gear_up && g < max_gear {
                self.gearbox.shift(g + 1, self.def.drivetrain.shift_time_s);
            } else if input.gear_down && g > lowest {
                self.gearbox.shift(g - 1, self.def.drivetrain.shift_time_s);
            } else if g > max_gear || g < lowest {
                self.gearbox
                    .shift(max_gear, self.def.drivetrain.shift_time_s);
            }
        }
        self.gearbox.update(dt);

        // --- Clutch engagement ----------------------------------------------------------
        let driven_corner = (0..4).find(|i| base_split[*i] > 0.0).unwrap_or(0);
        let ratio_now = self.gearbox.total_ratio(&self.def.drivetrain);
        self.auto_clutch = assists.auto_clutch;
        // Clutch-input speed the automatic clutch judges "rolling" by, rpm.
        let rolling_rpm = if hb_was_held {
            // Just after a handbrake declutch the driven wheels may still be stopped (locked
            // rears): re-engage only once they have spun back up, so the engine is not dragged
            // below idle.
            (ratio_now * axle_omega).abs() * RPM_PER_RADPS
        } else {
            // The slower of ground speed and the driven wheels: wheelspin does not count, and
            // driven wheels slowed under the car (scrubbed in a big slide, braked) open the
            // clutch before they drag the engine below idle.
            let ground = (ratio_now * v_fwd / self.corners[driven_corner].radius).abs();
            ground.min((ratio_now * axle_omega).abs()) * RPM_PER_RADPS
        };
        self.clutch = if self.converter.is_some() {
            // Torque-converter automatic: the converter replaces the launch clutch; the clutch
            // value is the lock-up clutch's availability. Shifts happen under power (the
            // lock-up clutch opens for the shift, see `substep`).
            1.0
        } else if self.gearbox.shifting() {
            // Clutch shift: the clutch releases, the box goes through neutral (engine
            // rev-matched in `substep`) and re-engages already in sync.
            let phase = self.gearbox.shift_engagement();
            if assists.auto_clutch {
                // Anti-stall during a shift too: off the throttle with the driven wheels too
                // slow to keep the engine at idle (a coasting downshift while they are
                // scrubbed down in a slide), the clutch opens at once instead of following
                // the shift's release.
                let stalling = thr_in <= 0.02
                    && ratio_now != 0.0
                    && rolling_rpm < AUTO_CLUTCH_OPEN_FRAC * self.def.engine.idle_rpm;
                if stalling {
                    0.0
                } else {
                    phase
                }
            } else {
                phase.min(1.0 - input.clutch.clamp(0.0, 1.0))
            }
        } else if assists.auto_clutch {
            auto_clutch_update(
                self.clutch,
                rolling_rpm,
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

        if hb_declutch && self.converter.is_none() {
            // Re-engages through the automatic clutch's normal launch / rolling logic.
            self.clutch = 0.0;
        }

        // Manual clutch: stall and re-crank (owner row 59). With the clutch pedal the
        // player's, the anti-stall is off (see `substep`), so letting the clutch up without
        // enough revs drags the engine below `STALL_FRAC` × idle and it dies. Pressing the
        // pedal past `RECRANK_PEDAL` re-cranks it to idle; rolling with the clutch engaged
        // until the engine turns at idle bump-starts it. The automatic clutch never stalls.
        let idle_rpm = self.def.engine.idle_rpm;
        let manual_clutch = !assists.auto_clutch && self.converter.is_none();
        if self.stalled {
            let recrank = input.clutch >= RECRANK_PEDAL;
            if !manual_clutch || recrank || self.engine.rpm() >= idle_rpm {
                self.stalled = false;
                if self.engine.rpm() < idle_rpm {
                    self.engine.omega = idle_rpm / RPM_PER_RADPS;
                }
            }
        } else if manual_clutch
            && self.gearbox.gear != 0
            && self.clutch > 0.3
            && self.engine.rpm() < STALL_FRAC * idle_rpm
        {
            self.stalled = true;
        }
        let power_scale = if self.stalled { 0.0 } else { power_scale };

        // --- Assists ----------------------------------------------------------------------
        let peak_slip = (0..4)
            .find(|i| base_split[*i] > 0.0)
            .map(|i| self.corners[i].tire.peak_slip_ratio)
            .unwrap_or(0.1);
        // TCS throttle cut watches the driven wheels in contact. While brake-based TC is
        // braking the faster wheel of an axle (one wheel spinning alone on an open diff) and
        // has brake to spare, that spin is the brakes' job: the throttle reacts to the slower
        // wheel. As brake-TC nears its limit, the faster wheel is blended back in.
        let mut driven_slip = 0.0f32;
        for (l, r, max_brake) in [
            (0usize, 1usize, self.def.brakes.max_torque_front_nm),
            (2, 3, self.def.brakes.max_torque_rear_nm),
        ] {
            let k = |i: usize| {
                (base_split[i] > 0.0 && self.wheels[i].contact).then_some(self.wheels[i].kappa)
            };
            let s = match (k(l), k(r)) {
                (Some(a), Some(b)) => {
                    let fast = if a >= b { l } else { r };
                    let held = self.tcs.brake[fast];
                    // 0 = brakes holding the spin … 1 = brakes saturated / not acting.
                    let w = if assists.tcs && held > TCS_BRAKE_ACTIVE_NM {
                        let use_frac = held / (BTC_MAX_FRAC * max_brake).max(1.0);
                        ((use_frac - TCS_BRAKE_HANDOFF) / (1.0 - TCS_BRAKE_HANDOFF)).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                    let (lo, hi) = (a.min(b), a.max(b));
                    lo + (hi - lo) * w
                }
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => 0.0,
            };
            driven_slip = driven_slip.max(s);
        }
        let tcs_mult = self.tcs.update(assists.tcs, driven_slip, peak_slip, dt);
        let yaw_rate = ch.ang_vel.dot(up);
        let front_mu = self.front_mu();
        let mut esc_out = self.esc.update(
            assists.esc && !hb_hold,
            yaw_rate,
            self.center_steer,
            v_fwd,
            wheelbase,
            front_mu,
            dt,
        );
        if self.diffs.rear == AxleDiff::Locked && base_split[2] > 0.0 && base_split[3] > 0.0 {
            // A locked rear shares a single-wheel brake between both rears (no yaw moment):
            // ESC brakes the front wheel on the same side instead.
            for (rear, front) in [(2usize, 0usize), (3, 1)] {
                esc_out.brake[front] += esc_out.brake[rear];
                esc_out.brake[rear] = 0.0;
            }
        }
        // TCS acts on engine torque once the clutch is locked; during an automatic-clutch
        // launch (slipping) wheel torque equals clutch torque, so TCS trims the clutch instead.
        let launching = self.auto_clutch && !self.clutch_locked && self.converter.is_none();
        // Upshift on a clutch gearbox: ignition cut while the clutch is open, so the engine
        // drops toward the new gear instead of flaring.
        let shift_cut = if self.converter.is_none()
            && self.gearbox.shifting()
            && self.gearbox.target() > self.gearbox.gear
        {
            self.gearbox.shift_engagement()
        } else {
            1.0
        };
        let throttle = self.throttle_f
            * esc_out.throttle_scale
            * shift_cut
            * if launching { 1.0 } else { tcs_mult };
        let clutch_trim = if launching { tcs_mult } else { 1.0 };

        // --- Steering ---------------------------------------------------------------------
        let steer_in = self.steer_input(input.steer, v_fwd, v_left, rear_axle);
        let max_steer = self.max_steer();
        self.center_steer = update_center_angle(
            self.center_steer,
            &steer_in,
            assists,
            max_steer,
            self.def.steering.rack_speed_rad_per_s,
            dt,
        );
        let drift_tel = crate::drift::DriftTelemetry {
            mode: dc.mode,
            slide_angle: dc.slide,
            // The steering's onset: gated by the confirmed-slide gate (row 102).
            onset: onset * self.slide_gate.amount,
            envelope: dc.envelope,
            authority: if dc.mode == crate::drift::DriftMode::Off {
                0.0
            } else {
                dc.authority
            },
            throttle_scale: dc.throttle_scale,
        };
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
            self.probe_wheel(i, &ch, up, ground, world, dt);
            // Weather under the contact patch (W-1). Off the ground the last value stays; it
            // has no effect there (no grip, planing reported as 0).
            if self.conditions_source == ConditionsSource::World && self.wheels[i].contact {
                let local = world.weather_at(arr(self.wheels[i].point));
                self.conditions[i] = SurfaceConditions::from_local(&local);
            }
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
        let mut aquaplane = [0.0f32; 4];
        for (i, planing_out) in aquaplane.iter_mut().enumerate() {
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
            let (cond_grip, planing) =
                conditions_grip(&self.conditions[i], w.surface, w.vx, &c.wet);
            *planing_out = if w.contact { planing } else { 0.0 };
            w.grip = surface_grip(&c.tire, w.surface)
                * cond_grip
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
        // Brake-based traction control (part of TCS) on driven axles whose diff can slip.
        let dir = if self.gearbox.gear < 0 { -1.0 } else { 1.0 };
        let mut tc_axles = [BtcAxle {
            l: 0,
            r: 1,
            v_l: 0.0,
            v_r: 0.0,
            v_kin: 0.0,
            ff: [0.0; 2],
            max_nm: 0.0,
            authority: 0.0,
        }; 2];
        let mut n_tc = 0;
        for (l, r, d, track, max) in [
            (
                0usize,
                1usize,
                self.diffs.front,
                self.def.track_front_m,
                self.def.brakes.max_torque_front_nm,
            ),
            (
                2,
                3,
                self.diffs.rear,
                self.def.track_rear_m,
                self.def.brakes.max_torque_rear_nm,
            ),
        ] {
            if base_split[l] > 0.0 && base_split[r] > 0.0 && d != AxleDiff::Locked {
                let v = |i: usize| self.wheels[i].omega * self.wheels[i].r_eff * dir;
                tc_axles[n_tc] = BtcAxle {
                    l,
                    r,
                    v_l: v(l),
                    v_r: v(r),
                    v_kin: -yaw_rate * track * dir,
                    ff: [l, r].map(|i| {
                        let w = &self.wheels[i];
                        ((w.drive - w.fx * w.r_eff) * dir).max(0.0)
                    }),
                    max_nm: BTC_MAX_FRAC * max,
                    authority: if l >= 2 {
                        btc_rear_authority(v_fwd)
                    } else {
                        1.0
                    },
                };
                n_tc += 1;
            }
        }
        let tc_driving = self.throttle_f > 0.05 && self.brake_f < 0.05 && handbrake < 0.1;
        let tc_brake =
            self.tcs
                .update_brakes(assists.tcs, tc_driving, &tc_axles[..n_tc], v_fwd, dt);
        for (i, b) in brake.iter_mut().enumerate() {
            *b += esc_out.brake[i] + tc_brake[i];
            self.wheels[i].brake = *b;
        }

        // --- Sub-stepped drivetrain, wheel spin and tire forces ----------------------------
        let n_sub = VEHICLE_SUBSTEPS.max(1);
        let h = dt / n_sub as f32;
        let split = if hb_declutch {
            [0.0; 4]
        } else {
            self.drive_split(handbrake)
        };
        // Wheel-speed difference (left − right, rad/s) the cornering geometry asks for, per
        // axle: the inside wheel turns slower (active diffs lock only beyond it).
        let kin = [
            -yaw_rate * self.def.track_front_m / self.corners[0].radius.max(0.05),
            -yaw_rate * self.def.track_rear_m / self.corners[2].radius.max(0.05),
        ];
        let mut acc_fx = [0.0f32; 4];
        let mut acc_fy = [0.0f32; 4];
        let mut acc_drive = [0.0f32; 4];
        let mut acc_x = [0.0f32; 3];
        for _ in 0..n_sub {
            let (drive, x) = self.substep(throttle, clutch_trim, power_scale, &split, kin, h);
            for i in 0..4 {
                acc_fx[i] += self.wheels[i].fx;
                acc_fy[i] += self.wheels[i].fy;
                acc_drive[i] += drive[i];
            }
            for k in 0..3 {
                acc_x[k] += x[k];
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
        if launch_cut > 0.0 && dt > 0.0 {
            force -= V3::Z * (self.def.mass_kg * launch_cut / dt);
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
            aquaplane,
            diff_transfer_nm: acc_x.map(|x| x * inv),
            tcs_brake_nm: tc_brake,
            contact_lift_cut_mps: launch_cut,
            stalled: self.stalled,
            drift: drift_tel,
            sliding: self.slide_gate.sliding(),
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
        dt: f32,
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
            w.x_wheel = 0.0;
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
        // The tyre sweep starts PROBE_LIFT_M higher so a kerb edge ahead never overlaps the
        // starting cylinder (which would void the sweep mid-climb).
        let probe = WheelProbe {
            center: origin - down * PROBE_LIFT_M,
            dir: down,
            max_dist: travel + PROBE_LIFT_M,
            radius: c.radius,
            half_width: c.half_width * PROBE_WIDTH_FRAC,
            rot: ch.rot * yaw_quat(w.steer - std::f32::consts::FRAC_PI_2),
        };
        // Rigid-tyre compression plus the tyre's static deflection (data radius = loaded).
        let hit = ground.cast_wheel(&probe);
        let reach = hit.map(|h| travel - (h.distance - PROBE_LIFT_M) + TYRE_STATIC_DEFLECTION_M);
        let st = step_strut(&c.susp, c.tyre_rate, w.x, w.vcomp, reach, dt);
        w.x = st.x;
        w.vcomp = st.v;
        w.fz = st.fz;
        w.x_wheel = match reach {
            Some(r) if st.fz > 0.0 => r - TYRE_STATIC_DEFLECTION_M,
            _ => st.x,
        };
        match hit {
            Some(hit) if st.fz > 0.0 => {
                w.contact = true;
                w.point = hit.point;
                w.normal = tyre_normal(hit.normal, up);
                w.surface = world.surface_at(arr(hit.point));
            }
            _ => {
                w.contact = false;
                w.surface = SurfaceType::default();
            }
        }
    }

    /// One drivetrain/wheel sub-step of length `h`; `clutch_trim` scales clutch capacity
    /// (launch-phase TCS), `kin` is the per-axle cornering speed difference (front, rear).
    /// Returns the drive torque given to each wheel and the torque each diff moved (front,
    /// rear, centre).
    fn substep(
        &mut self,
        throttle: f32,
        clutch_trim: f32,
        power_scale: f32,
        split: &[f32; 4],
        kin: [f32; 2],
        h: f32,
    ) -> ([f32; 4], [f32; 3]) {
        let ratio = self.gearbox.total_ratio(&self.def.drivetrain);
        let s_sum: f32 = split.iter().sum();
        let axle = self.axle_omega(split);
        let omega_in = ratio * axle;
        let idle_w = self.def.engine.idle_rpm / RPM_PER_RADPS;
        // Anti-stall slip: only for the automatic clutch (a manual clutch can stall).
        let anti_stall = if self.auto_clutch {
            ((self.engine.omega - 0.6 * idle_w) / (0.4 * idle_w)).clamp(0.0, 1.0)
        } else {
            1.0
        };
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
            Some(c) => !self.gearbox.shifting() && self.engine.rpm() >= 0.9 * c.def.coupling_rpm,
            // While a clutch shift re-engages, the clutch may lock as soon as it is in sync.
            None => !self.auto_clutch || self.clutch >= 0.99 || self.gearbox.shifting(),
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
        let x_diff = self.solve_diffs(&mut drive, wheel_side, reflected, split, s_sum, kin, h);

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
            if !self.gearbox.shifting()
                && self.engine.rpm() >= c.def.coupling_rpm
                && sr >= 0.92
                && cap >= t_e.abs()
            {
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
        } else if let (true, Some(left)) = (
            self.auto_clutch && converter.is_none(),
            self.gearbox.neutral_time_left(),
        ) {
            // Neutral phase of an automated clutch shift: the ECU rev-matches the engine to
            // the new gear (throttle cut on upshifts, blip on downshifts) before re-engaging.
            let target = Gearbox::ratio_of(&self.def.drivetrain, self.gearbox.target())
                * self.def.drivetrain.final_drive
                * self.axle_omega(split);
            let limit = self.def.engine.rev_limit_rpm / RPM_PER_RADPS;
            let target = target.abs().clamp(idle_w, limit);
            let k = (h / left.max(h)).min(1.0);
            self.engine.omega += (target - self.engine.omega) * k;
        } else {
            self.engine.omega += h * t_e / i_e;
        }
        self.engine.omega = self.engine.omega.max(0.0);
        (drive, x_diff)
    }

    /// Centre then axle differentials for one sub-step (see [`crate::diff`]): moves torque
    /// within `drive` (per-wheel, already split by `split`) and returns the transfers
    /// (front, rear, centre). `wheel_side` is the total torque entering the driveline at the
    /// wheels, `reflected` the engine inertia seen at the wheels (shared by `split`).
    #[allow(clippy::too_many_arguments)]
    fn solve_diffs(
        &self,
        drive: &mut [f32; 4],
        wheel_side: f32,
        reflected: f32,
        split: &[f32; 4],
        s_sum: f32,
        kin: [f32; 2],
        h: f32,
    ) -> [f32; 3] {
        let mut xs = [0.0f32; 3];
        if s_sum <= 0.0 {
            return xs;
        }
        let inertia = |i: usize| self.corners[i].inertia + reflected * split[i] / s_sum;
        // Torque the wheel resists against drive in direction `sense` (tire + brake).
        let react = |i: usize, sense: f32| {
            let w = &self.wheels[i];
            (w.fx * w.r_eff * sense).max(0.0) + w.brake
        };
        let sense_of = |t: f32| if t < 0.0 { -1.0 } else { 1.0 };

        // Centre (front = "left", rear = "right"), speeds compared at the tire surface.
        let (fs, rs) = (split[0] + split[1], split[2] + split[3]);
        if fs > 0.0 && rs > 0.0 && self.diffs.centre != CentreDiff::Default {
            let axle = |a: usize, b: usize, sh: f32| {
                let w = (split[a] * self.wheels[a].omega + split[b] * self.wheels[b].omega) / sh;
                let r = (split[a] * self.wheels[a].r_eff + split[b] * self.wheels[b].r_eff) / sh;
                (w, r)
            };
            let (wf, rf) = axle(0, 1, fs);
            let (wr, rr) = axle(2, 3, rs);
            let rm = (0.5 * (rf + rr)).max(0.05);
            let (i_f, i_r) = (inertia(0) + inertia(1), inertia(2) + inertia(3));
            let sense = sense_of(wheel_side);
            let x = centre_transfer(
                &self.diffs.centre,
                &DiffInput {
                    t_in: wheel_side,
                    omega_l: wf * rf / rm,
                    omega_r: wr * rr / rm,
                    k_eq: 1.0 / (h * (1.0 / i_f.max(1e-3) + 1.0 / i_r.max(1e-3))),
                    react_l: react(0, sense) + react(1, sense),
                    react_r: react(2, sense) + react(3, sense),
                    dw_kin: 0.0,
                },
            );
            for i in 0..4 {
                if i < 2 {
                    drive[i] -= x * split[i] / fs;
                } else {
                    drive[i] += x * split[i] / rs;
                }
            }
            xs[2] = x;
        }

        // Axles: both sides must be connected (a lost wheel or a declutched axle has no diff).
        for (k, (l, r, d)) in [(0usize, 1usize, self.diffs.front), (2, 3, self.diffs.rear)]
            .into_iter()
            .enumerate()
        {
            if split[l] <= 0.0 || split[r] <= 0.0 || d == AxleDiff::Open {
                continue;
            }
            let t_in = drive[l] + drive[r];
            let sense = sense_of(t_in);
            let x = axle_transfer(
                &d,
                &DiffInput {
                    t_in,
                    omega_l: self.wheels[l].omega,
                    omega_r: self.wheels[r].omega,
                    k_eq: inertia(l) / h * 0.5,
                    react_l: react(l, sense),
                    react_r: react(r, sense),
                    dw_kin: kin[k],
                },
            );
            drive[l] -= x;
            drive[r] += x;
            xs[k] = x;
        }
        xs
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
                compression_m: w.x_wheel,
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
