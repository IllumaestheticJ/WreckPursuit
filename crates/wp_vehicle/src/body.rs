//! Rapier glue: one dynamic rigid body per vehicle. The body origin is the chassis reference
//! point, its mass properties come from the `VehicleDef` (mass, CG, principal inertias) and its
//! collider is the body box (prompt 03 later swaps in the lattice hull).
//!
//! Per tick: `step_with_ground` → [`Vehicle::write_body`] → Rapier step → [`Vehicle::read_body`].

use rapier3d::prelude::*;

use crate::math::{v3, yaw_quat, V3};
use crate::vehicle::{Chassis, Vehicle};

/// Height of the bottom of the chassis collider above the chassis reference point, m.
pub const BODY_CLEARANCE_M: f32 = 0.15;

/// Restitution of the chassis collider, combined with the other collider's by `Min`.
pub const CHASSIS_RESTITUTION: f32 = 0.1;

/// Rapier handles owned by one vehicle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VehicleBody {
    /// The chassis rigid body.
    pub body: RigidBodyHandle,
    /// The chassis collider.
    pub collider: ColliderHandle,
}

impl Vehicle {
    /// The chassis body's mass properties: the car data's mass and inertia, the inertia scaled
    /// by the Drift Lab W ride override when it applies ([`Vehicle::body_inertia`]).
    fn body_mass_properties(&self) -> MassProperties {
        let i = self.body_inertia();
        // Contract order is [roll (about Y), pitch (about X), yaw (about Z)].
        MassProperties::new(
            self.com_local(),
            self.def().mass_kg,
            Vector::new(i[1], i[0], i[2]),
        )
    }

    /// Re-apply [`Vehicle::body_inertia`] to an existing chassis body (after
    /// a ride-lab inertia change).
    pub fn apply_body_inertia(&self, rb: &mut RigidBody) {
        rb.set_additional_mass_properties(self.body_mass_properties(), true);
    }

    /// Create the chassis rigid body + box collider at the vehicle's current chassis pose.
    pub fn build_body(
        &self,
        bodies: &mut RigidBodySet,
        colliders: &mut ColliderSet,
    ) -> VehicleBody {
        let def = self.def();
        let [w, l, h] = def.body_size_m;
        let clearance = BODY_CLEARANCE_M.min(h * 0.3);
        let mprops = self.body_mass_properties();
        let ch = self.chassis();
        let rb = RigidBodyBuilder::dynamic()
            .pose(Pose::from_parts(ch.pos, ch.rot))
            .linvel(ch.lin_vel)
            .angvel(ch.ang_vel)
            .additional_mass_properties(mprops)
            .can_sleep(false)
            .ccd_enabled(true)
            .build();
        let body = bodies.insert(rb);
        let half_h = (h - clearance) * 0.5;
        let col = ColliderBuilder::cuboid(w * 0.5, l * 0.5, half_h)
            .translation(Vector::new(0.0, 0.0, clearance + half_h))
            .density(0.0)
            .friction(0.4)
            // Bounce only against other cars (0.1 each, so 0.1): the Min rule makes the
            // static world (restitution 0) a dead stop, so a landing or a kerb face never
            // springs the body back up (playtest row 54).
            .restitution(CHASSIS_RESTITUTION)
            .restitution_combine_rule(CoefficientCombineRule::Min)
            .build();
        let collider = colliders.insert_with_parent(col, body, bodies);
        VehicleBody { body, collider }
    }

    /// Mirror the rigid body's pose and velocities into the vehicle.
    pub fn read_body(&mut self, rb: &RigidBody) {
        self.set_chassis(Chassis {
            pos: rb.translation(),
            rot: *rb.rotation(),
            lin_vel: rb.linvel(),
            ang_vel: rb.angvel(),
            com: rb.center_of_mass(),
        });
    }

    /// Replace the body's accumulated forces with this step's chassis force and torque.
    pub fn write_body(&self, rb: &mut RigidBody) {
        let (f, t) = self.forces();
        rb.reset_forces(true);
        rb.reset_torques(true);
        rb.add_force(f, true);
        rb.add_torque(t, true);
    }

    /// Push the vehicle's chassis state (pose, velocities) into the body and clear its
    /// accumulated forces — after [`Vehicle::restore`] / [`Vehicle::set_state`].
    pub fn write_chassis_to_body(&self, rb: &mut RigidBody) {
        let ch = self.chassis();
        rb.set_position(Pose::from_parts(ch.pos, ch.rot), true);
        rb.set_linvel(ch.lin_vel, true);
        rb.set_angvel(ch.ang_vel, true);
        rb.reset_forces(true);
        rb.reset_torques(true);
    }

    /// Teleport body and vehicle to `pos`/`yaw` moving forward at `speed` (m/s) with yaw rate
    /// `yaw_rate` (rad/s); wheels and engine are spun up to match (see `set_rolling`).
    pub fn place_body(
        &mut self,
        rb: &mut RigidBody,
        pos: [f32; 3],
        yaw: f32,
        speed: f32,
        yaw_rate: f32,
    ) {
        let rot = yaw_quat(yaw);
        rb.set_position(Pose::from_parts(v3(pos), rot), true);
        rb.set_linvel(rot * V3::Y * speed, true);
        rb.set_angvel(V3::Z * yaw_rate, true);
        rb.reset_forces(true);
        rb.reset_torques(true);
        if speed == 0.0 {
            self.reset(pos, yaw);
        } else {
            self.set_rolling(speed);
        }
        self.forget_motion();
        self.read_body(rb);
    }
}
