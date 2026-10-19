//! Standalone single-vehicle stepper: a Rapier world with a ground slab (top at z = 0) and one
//! vehicle, configured and stepped **exactly** like `wp_sim` (same slab, same priming, same
//! per-tick order), so a lone car on flat ground produces bit-identical states in both — this
//! is the client-side prediction stepper for `wp_net` (NET-N2), and backs [`crate::metrics`],
//! tests and the benchmark. Rewind with [`Rig::snapshot`] / [`Rig::restore`].
//!
//! Bit-exactness holds while the car touches nothing but the ground plane: other cars, static
//! boxes or city colliders on the server are not in the rig's world.

use rapier3d::prelude::*;
use wp_contracts::{
    AssistProfile, HandlingEffects, LoadedCar, SignalState, SimEvent, SurfaceType, TimeOfDay,
    VehicleInput, VehicleSim, VehicleState, WorldQuery, GRAVITY_MPS2, PHYSICS_DT,
};

use crate::body::VehicleBody;
use crate::ground::RapierGround;
use crate::snapshot::{SnapshotError, VehicleSnapshot};
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

/// Half extents of the ground slab (identical to `wp_sim::PhysicsWorld`), m.
pub const GROUND_HALF_EXTENTS: [f32; 3] = [5000.0, 5000.0, 0.5];

/// The static ground collider used by `wp_sim` and the rig: a slab whose top face is z = 0.
pub fn ground_collider() -> Collider {
    let [x, y, z] = GROUND_HALF_EXTENTS;
    ColliderBuilder::cuboid(x, y, z)
        .translation(Vector::new(0.0, 0.0, -z))
        .build()
}

/// One vehicle in its own Rapier world. `W` supplies surfaces, limits and time of day
/// (default: [`FlatWorld`]; pass the same world type the server uses for exact parity).
pub struct Rig<W: WorldQuery = FlatWorld> {
    /// The vehicle.
    pub vehicle: Vehicle,
    /// World queries.
    pub world: W,
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

impl Rig<FlatWorld> {
    /// Rig with the car at rest at the origin facing +Y.
    pub fn new(car: &LoadedCar) -> Self {
        Self::with_extras(car, VehicleExtras::default())
    }

    /// [`Rig::new`] with vehicle extras.
    pub fn with_extras(car: &LoadedCar, extras: VehicleExtras) -> Self {
        Self::with_world(car, extras, FlatWorld::default(), [0.0; 3], 0.0)
    }
}

impl<W: WorldQuery> Rig<W> {
    /// Rig with a custom world, spawning the car at `pos`/`yaw` (same sequence as
    /// `wp_sim::Sim::new` + `spawn_vehicle`: prime the empty world, then add the body).
    pub fn with_world(
        car: &LoadedCar,
        extras: VehicleExtras,
        world: W,
        pos: [f32; 3],
        yaw: f32,
    ) -> Self {
        let mut bodies = RigidBodySet::new();
        let mut colliders = ColliderSet::new();
        colliders.insert(ground_collider());
        let mut pipeline = PhysicsPipeline::new();
        let mut islands = IslandManager::new();
        let mut broad_phase = BroadPhaseBvh::new();
        let mut narrow_phase = NarrowPhase::new();
        let mut impulse_joints = ImpulseJointSet::new();
        let mut multibody_joints = MultibodyJointSet::new();
        let mut soft_bodies = SoftBodySet::new();
        let mut ccd = CCDSolver::new();
        let params = IntegrationParameters {
            dt: PHYSICS_DT,
            ..Default::default()
        };
        // Prime the broad phase (empty world) so the first wheel probes see the ground.
        pipeline.step(
            Vector::new(0.0, 0.0, -GRAVITY_MPS2),
            &params,
            &mut islands,
            &mut broad_phase,
            &mut narrow_phase,
            &mut bodies,
            &mut colliders,
            &mut impulse_joints,
            &mut multibody_joints,
            &mut soft_bodies,
            &mut ccd,
            &(),
            &(),
        );
        let vehicle = Vehicle::with_extras(&car.def, &car.tires, pos, yaw, extras);
        let handles = vehicle.build_body(&mut bodies, &mut colliders);
        Self {
            vehicle,
            world,
            events: Vec::new(),
            handles,
            bodies,
            colliders,
            pipeline,
            islands,
            broad_phase,
            narrow_phase,
            impulse_joints,
            multibody_joints,
            soft_bodies,
            ccd,
            params,
            ticks: 0,
        }
    }

    /// Full dynamic state of the car (see [`crate::snapshot`]). Rapier's own caches are not
    /// included; with the car touching only the ground they carry no state between ticks.
    pub fn snapshot(&self) -> VehicleSnapshot {
        self.vehicle.snapshot()
    }

    /// Rewind to a snapshot: restores the vehicle and puts the chassis body back.
    pub fn restore(&mut self, snap: &VehicleSnapshot) -> Result<(), SnapshotError> {
        self.vehicle.restore(snap)?;
        self.vehicle
            .write_chassis_to_body(&mut self.bodies[self.handles.body]);
        Ok(())
    }

    /// Set the tick counter (e.g. to the server tick after a restore).
    pub fn set_ticks(&mut self, ticks: u64) {
        self.ticks = ticks;
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
