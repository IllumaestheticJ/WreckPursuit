//! Rapier world wrapper configured with the project conventions (+Z up, gravity on -Z,
//! fixed `PHYSICS_DT`).

use rapier3d::prelude::*;

/// All Rapier state for one simulation instance.
pub struct PhysicsWorld {
    /// Gravity vector (world -Z).
    pub gravity: Vector,
    /// Integration parameters (`dt = PHYSICS_DT`).
    pub params: IntegrationParameters,
    /// Rigid bodies.
    pub bodies: RigidBodySet,
    /// Colliders.
    pub colliders: ColliderSet,
    pipeline: PhysicsPipeline,
    islands: IslandManager,
    broad_phase: BroadPhaseBvh,
    narrow_phase: NarrowPhase,
    impulse_joints: ImpulseJointSet,
    multibody_joints: MultibodyJointSet,
    soft_bodies: SoftBodySet,
    ccd: CCDSolver,
}

impl PhysicsWorld {
    /// New world with a static ground slab whose top face is the plane z = 0.
    pub fn new() -> Self {
        let params = IntegrationParameters {
            dt: wp_contracts::PHYSICS_DT,
            ..Default::default()
        };
        let mut colliders = ColliderSet::new();
        // Shared with `wp_vehicle::rig::Rig` so the client prediction stepper matches exactly.
        colliders.insert(wp_vehicle::rig::ground_collider());
        let mut world = Self {
            gravity: Vector::new(0.0, 0.0, -wp_contracts::GRAVITY_MPS2),
            params,
            bodies: RigidBodySet::new(),
            colliders,
            pipeline: PhysicsPipeline::new(),
            islands: IslandManager::new(),
            broad_phase: BroadPhaseBvh::new(),
            narrow_phase: NarrowPhase::new(),
            impulse_joints: ImpulseJointSet::new(),
            multibody_joints: MultibodyJointSet::new(),
            soft_bodies: SoftBodySet::new(),
            ccd: CCDSolver::new(),
        };
        // Prime the broad phase so ground queries work on the very first tick.
        world.step();
        world
    }

    /// Scene query pipeline over the current colliders (as of the last step), with `filter`
    /// (e.g. excluding a vehicle's own body for its wheel probes).
    pub fn query_pipeline<'a>(&'a self, filter: QueryFilter<'a>) -> QueryPipeline<'a> {
        self.broad_phase.as_query_pipeline(
            self.narrow_phase.query_dispatcher(),
            &self.bodies,
            &self.colliders,
            filter,
        )
    }

    /// The narrow phase (contact pairs of the last step).
    pub fn narrow_phase(&self) -> &NarrowPhase {
        &self.narrow_phase
    }

    /// Advance one fixed step.
    pub fn step(&mut self) {
        self.pipeline.step(
            self.gravity,
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
}

impl Default for PhysicsWorld {
    fn default() -> Self {
        Self::new()
    }
}
