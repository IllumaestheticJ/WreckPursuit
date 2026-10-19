//! Detached parts and wheels, and their conversion to Rapier debris bodies. The damage model
//! does not own a Rapier world; `wp_sim` drains [`crate::DamageModel::take_detached_parts`]
//! and inserts the bodies built here.

use rapier3d::prelude::*;
use wp_contracts::BodyRegion;

/// What came off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetachedKind {
    /// A detachable body part (bumper, hood, door, trunk) — `Detachable::group`.
    Part {
        /// `Detachable::group` id.
        group: u16,
        /// Region of the part.
        region: BodyRegion,
    },
    /// A wheel (0 FL, 1 FR, 2 RL, 3 RR).
    Wheel {
        /// Wheel index.
        index: u8,
    },
}

/// A part that left the car, described in the car's chassis space at the moment it left.
#[derive(Debug, Clone, PartialEq)]
pub struct DetachedPart {
    /// What came off.
    pub kind: DetachedKind,
    /// Mass of the debris body, kg.
    pub mass_kg: f32,
    /// Convex-hull source points relative to `centroid_local` (empty for wheels), m.
    pub hull_points_local: Vec<[f32; 3]>,
    /// Part centre (wheel: hub node) in chassis space, m.
    pub centroid_local: [f32; 3],
    /// Velocity relative to the chassis, chassis space, m/s.
    pub velocity_local: [f32; 3],
}

impl DetachedPart {
    /// Build a dynamic Rapier body + collider for this part given the chassis pose
    /// (`pos`, unit quaternion `rot` = `[x, y, z, w]`) and linear velocity in world space.
    /// Wheels become cylinders of `wheel_radius_m` × `wheel_width_m` with their axle along
    /// chassis X. Returns `None` if the part's hull is degenerate.
    pub fn to_rapier(
        &self,
        chassis_pos: [f32; 3],
        chassis_rot: [f32; 4],
        chassis_linvel: [f32; 3],
        wheel_radius_m: f32,
        wheel_width_m: f32,
    ) -> Option<(RigidBody, Collider)> {
        let rot = Rotation::from_xyzw(
            chassis_rot[0],
            chassis_rot[1],
            chassis_rot[2],
            chassis_rot[3],
        );
        let c = Vector::new(
            self.centroid_local[0],
            self.centroid_local[1],
            self.centroid_local[2],
        );
        let v = Vector::new(
            self.velocity_local[0],
            self.velocity_local[1],
            self.velocity_local[2],
        );
        let world_pos = Vector::new(chassis_pos[0], chassis_pos[1], chassis_pos[2]) + rot * c;
        let world_vel =
            Vector::new(chassis_linvel[0], chassis_linvel[1], chassis_linvel[2]) + rot * v;
        let mass = self.mass_kg.max(0.1);
        let (body_rot, collider) = match self.kind {
            DetachedKind::Part { .. } => {
                let pts: Vec<Vector> = self
                    .hull_points_local
                    .iter()
                    .map(|p| Vector::new(p[0], p[1], p[2]))
                    .collect();
                (rot, ColliderBuilder::convex_hull(&pts)?.mass(mass).build())
            }
            DetachedKind::Wheel { .. } => {
                // Parry cylinders run along local Y; turn the axle onto chassis X.
                // 90° about Z, written out exactly (no libm trig in sim code).
                let h = std::f32::consts::FRAC_1_SQRT_2;
                let axle = Rotation::from_xyzw(0.0, 0.0, h, h);
                (
                    rot * axle,
                    ColliderBuilder::cylinder(
                        0.5 * wheel_width_m.max(0.05),
                        wheel_radius_m.max(0.05),
                    )
                    .mass(mass)
                    .build(),
                )
            }
        };
        let body = RigidBodyBuilder::dynamic()
            .pose(Pose::from_parts(world_pos, body_rot))
            .linvel(world_vel)
            .build();
        Some((body, collider))
    }
}
