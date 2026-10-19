//! Standalone single-vehicle test rig: a Rapier world with a ground slab (top at z = 0) and
//! one vehicle, stepped exactly like `wp_sim` does. Used by [`crate::metrics`], tests and the
//! benchmark.

use rapier3d::prelude::*;
use wp_contracts::{
    AssistProfile, HandlingEffects, LoadedCar, SignalState, SimEvent, SurfaceType, TimeOfDay,
    VehicleInput, VehicleSim, VehicleState, WorldQuery, GRAVITY_MPS2, PHYSICS_DT,
};

use crate::body::VehicleBody;
use crate::ground::RapierGround;
use crate::vehicle::{Vehicle, VehicleExtras};

/// Flat world with one surface type, no limits or signals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlatWorld {
    /// Surface everywhere.
    pub surface: SurfaceType,
    /// Fixed time of day.
    pub time: TimeOfDay,
}

impl Default for FlatWorld {
    fn default() -> Self {
        Self {
            surface: SurfaceType::Asphalt,
            time: TimeOfDay::from_hours(12.0),
        }
    }
}

impl WorldQuery for FlatWorld {
    fn surface_at(&self, _p: [f32; 3]) -> SurfaceType {
        self.surface
    }
    fn speed_limit_at(&self, _p: [f32; 3]) -> Option<f32> {
        None
    }
    fn signal_state_ahead(&self, _p: [f32; 3], _h: [f32; 3]) -> Option<SignalState> {
        None
    }
    fn time_of_day(&self) -> TimeOfDay {
        self.time
    }
}

/// One vehicle in its own Rapier world.
pub struct Rig {
    /// The vehicle.
    pub vehicle: Vehicle,
    /// World queries.
    pub world: FlatWorld,
    /// Events emitted so far.
    pub events: Vec<SimEvent>,
    handles: VehicleBody,
    bodies: RigidBodySet,
    colliders: ColliderSet,
    pipeline: PhysicsPipeline,
    islands: IslandManager,
    broad_phase: BroadPhaseBvh,
    narrow_phase: NarrowPhase,
    impulse_joints: ImpulseJointSet,
    multibody_joints: MultibodyJointSet,
    soft_bodies: SoftBodySet,
    ccd: CCDSolver,
    params: IntegrationParameters,
    ticks: u64,
}

impl Rig {
    /// Rig with the car at rest at the origin facing +Y.
    pub fn new(car: &LoadedCar) -> Self {
        Self::with_extras(car, VehicleExtras::default())
    }

    /// [`Rig::new`] with vehicle extras.
    pub fn with_extras(car: &LoadedCar, extras: VehicleExtras) -> Self {
        let vehicle = Vehicle::with_extras(&car.def, &car.tires, [0.0; 3], 0.0, extras);
        let mut bodies = RigidBodySet::new();
        let mut colliders = ColliderSet::new();
        colliders.insert(
            ColliderBuilder::cuboid(20_000.0, 20_000.0, 0.5)
                .translation(Vector::new(0.0, 0.0, -0.5))
                .friction(0.8)
                .build(),
        );
        let handles = vehicle.build_body(&mut bodies, &mut colliders);
        let mut rig = Self {
            vehicle,
            world: FlatWorld::default(),
            events: Vec::new(),
            handles,
            bodies,
            colliders,
            pipeline: PhysicsPipeline::new(),
            islands: IslandManager::new(),
            broad_phase: BroadPhaseBvh::new(),
            narrow_phase: NarrowPhase::new(),
            impulse_joints: ImpulseJointSet::new(),
            multibody_joints: MultibodyJointSet::new(),
            soft_bodies: SoftBodySet::new(),
            ccd: CCDSolver::new(),
            params: IntegrationParameters {
                dt: PHYSICS_DT,
                ..Default::default()
            },
            ticks: 0,
        };
        // Prime the broad phase so the first wheel probes see the ground, then restore.
        rig.physics_step();
        rig.teleport([0.0; 3], 0.0, 0.0, 0.0);
        rig.ticks = 0;
        rig
    }

    fn physics_step(&mut self) {
        self.pipeline.step(
            Vector::new(0.0, 0.0, -GRAVITY_MPS2),
            &self.params,
            &mut self.islands,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            &mut self.soft_bodies,
            &mut self.ccd,
            &(),
            &(),
        );
    }

    /// Advance one physics tick ([`PHYSICS_DT`]).
    pub fn step(&mut self, input: &VehicleInput, assists: &AssistProfile) {
        {
            let filter = QueryFilter::default().exclude_rigid_body(self.handles.body);
            let ground = RapierGround {
                query: self.broad_phase.as_query_pipeline(
                    self.narrow_phase.query_dispatcher(),
                    &self.bodies,
                    &self.colliders,
                    filter,
                ),
            };
            self.vehicle.step_with_ground(
                input,
                assists,
                &self.world,
                &ground,
                PHYSICS_DT,
                &mut self.events,
            );
        }
        self.vehicle.write_body(&mut self.bodies[self.handles.body]);
        self.physics_step();
        self.vehicle.read_body(&self.bodies[self.handles.body]);
        self.ticks += 1;
    }

    /// Teleport (see [`Vehicle::place_body`]).
    pub fn teleport(&mut self, pos: [f32; 3], yaw: f32, speed: f32, yaw_rate: f32) {
        let rb = &mut self.bodies[self.handles.body];
        self.vehicle.place_body(rb, pos, yaw, speed, yaw_rate);
    }

    /// Rewind vehicle *and* rigid body to a full-precision state (see [`Vehicle::set_state`]),
    /// clearing accumulated forces. Used by `wp_net` client reconciliation. (Added by
    /// workstream F, additive.)
    pub fn set_state(&mut self, s: &VehicleState) {
        self.vehicle.set_state(s);
        let ch = *self.vehicle.chassis();
        let rb = &mut self.bodies[self.handles.body];
        rb.set_position(Pose::from_parts(ch.pos, ch.rot), true);
        rb.set_linvel(ch.lin_vel, true);
        rb.set_angvel(ch.ang_vel, true);
        rb.reset_forces(true);
        rb.reset_torques(true);
        self.vehicle.read_body(rb);
    }

    /// Apply damage effects to the vehicle.
    pub fn set_effects(&mut self, eff: &HandlingEffects) {
        self.vehicle.apply_damage_effects(eff);
    }

    /// Vehicle state.
    pub fn state(&self) -> VehicleState {
        self.vehicle.state()
    }

    /// Simulated time, s.
    pub fn time(&self) -> f32 {
        self.ticks as f32 * PHYSICS_DT
    }

    /// Ticks run since construction.
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Stable hash of the vehicle state (same quantisation as `wp_sim`).
    pub fn state_hash(&self) -> u64 {
        let s = self.state();
        let mut h = wp_math::StateHasher::new();
        h.write_u64(self.ticks);
        for v in s.pos.iter().chain(s.lin_vel.iter()).chain(s.ang_vel.iter()) {
            h.write_f32_q(*v, 1e4);
        }
        for v in s.rot {
            h.write_f32_q(v, 1e5);
        }
        for w in &s.wheel_states {
            h.write_f32_q(w.omega, 1e3);
        }
        h.write_f32_q(s.engine_rpm, 1.0);
        h.write_i64(s.gear as i64);
        h.finish()
    }
}
