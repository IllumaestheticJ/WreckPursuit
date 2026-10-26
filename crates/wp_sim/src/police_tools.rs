//! Police tools in the physics world.
//!
//! `wp_police` plans roadblocks and spike strips; this module makes roadblock barriers solid.
//! Every barrier of an active roadblock (`PoliceSystem::roadblocks`) becomes a dynamic Rapier
//! box with the barrier's size and mass, so cars hit it, shove it and take damage from it
//! (contacts carry its mass like debris). The boxes are removed when the roadblock expires.
//! Spike strips need no body: `wp_police` detects wheels crossing the strip and its punctures
//! are merged into each vehicle's handling effects in [`Sim::step_fixed`].
//!
//! Order is deterministic: roadblocks in `wp_police`'s list order, barriers in plan order.

use rapier3d::math::Rotation;
use rapier3d::prelude::{ColliderBuilder, Pose, RigidBodyBuilder, RigidBodyHandle, Vector};
use wp_contracts::{EntityId, ToolId, VehicleSim};

use crate::Sim;

/// One physical roadblock barrier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ToolBody {
    /// Roadblock it belongs to.
    pub tool: ToolId,
    /// Rapier body.
    pub body: RigidBodyHandle,
    /// Half extents `[across, along, up]`, m.
    pub half_extents_m: [f32; 3],
}

/// A roadblock barrier as the renderer sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarrierView {
    /// Roadblock id.
    pub tool: u32,
    /// Centre, world m.
    pub pos: [f32; 3],
    /// Orientation `[x, y, z, w]`.
    pub rot: [f32; 4],
    /// Half extents `[across, along, up]`, m (box local +X across the road, +Y along it).
    pub half_extents_m: [f32; 3],
}

/// A spike strip as the renderer sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpikeStripView {
    /// Tool id.
    pub tool: u32,
    /// Strip end points on the road, world XY m.
    pub a: [f32; 2],
    /// Second end point.
    pub b: [f32; 2],
}

impl Sim {
    /// After the police step: spawn barrier bodies for new roadblocks, remove those of expired
    /// ones.
    pub(crate) fn sync_police_tools(&mut self) {
        let live: Vec<ToolId> = self.police.roadblocks().iter().map(|r| r.id).collect();
        let (keep, gone): (Vec<ToolBody>, Vec<ToolBody>) = self
            .tool_bodies
            .drain(..)
            .partition(|t| live.contains(&t.tool));
        for t in gone {
            self.physics.remove_body(t.body);
        }
        self.tool_bodies = keep;

        let new: Vec<(ToolId, EntityId, wp_police::StaticCollider)> = self
            .police
            .roadblocks()
            .iter()
            .filter(|r| !self.tool_bodies.iter().any(|t| t.tool == r.id))
            .flat_map(|r| r.barriers.iter().map(move |b| (r.id, r.racer, *b)))
            .collect();
        for (tool, racer, mut b) in new {
            // wp_police puts a barrier's centre its half height above z = 0. On a terrain city
            // that is under the road (owner, 2026-10-26: an invisible barrier in the Talon
            // Tunnel; its barriers spawned 34 m under the tunnel floor and fell out of the
            // world): stand it on the road under it, the surface nearest its racer's height
            // (the tunnel floor, not the hillside; the road on a bridge, not under it).
            if self.has_terrain() {
                let near = self
                    .vehicles
                    .iter()
                    .find(|v| v.id == racer)
                    .map(|v| v.vehicle.state().pos[2]);
                let z = self.world.map().ground_height([b.pos[0], b.pos[1]], near);
                b.pos[2] += z;
            }
            // Barrier forward (x, y) → yaw about +Z with 0 = +Y.
            let yaw = wp_math::atan2(-b.forward[0], b.forward[1]);
            let (s, c) = wp_math::sin_cos(yaw * 0.5);
            let pose = Pose::from_parts(
                Vector::from_array(b.pos),
                Rotation::from_xyzw(0.0, 0.0, s, c),
            );
            let body = self
                .physics
                .bodies
                .insert(RigidBodyBuilder::dynamic().pose(pose).build());
            let [hx, hy, hz] = b.half_extents_m;
            let col = ColliderBuilder::cuboid(hx, hy, hz)
                .mass(b.mass_kg.max(1.0))
                .friction(0.7)
                .restitution(0.05)
                .build();
            self.physics
                .colliders
                .insert_with_parent(col, body, &mut self.physics.bodies);
            self.tool_bodies.push(ToolBody {
                tool,
                body,
                half_extents_m: b.half_extents_m,
            });
        }
    }

    /// Remove every barrier body (the police system was replaced).
    pub(crate) fn clear_police_tools(&mut self) {
        for t in std::mem::take(&mut self.tool_bodies) {
            self.physics.remove_body(t.body);
        }
    }

    /// Physical roadblock barriers with their current poses (they move when hit).
    pub fn roadblock_barriers(&self) -> Vec<BarrierView> {
        self.tool_bodies
            .iter()
            .map(|t| {
                let rb = &self.physics.bodies[t.body];
                let r = *rb.rotation();
                BarrierView {
                    tool: t.tool.0,
                    pos: rb.translation().to_array(),
                    rot: [r.x, r.y, r.z, r.w],
                    half_extents_m: t.half_extents_m,
                }
            })
            .collect()
    }

    /// Spike strips on the road.
    pub fn spike_strips(&self) -> Vec<SpikeStripView> {
        self.police
            .spike_strips()
            .iter()
            .map(|s| SpikeStripView {
                tool: s.id.0,
                a: s.a,
                b: s.b,
            })
            .collect()
    }

    /// Per-wheel tire deflation from spike strips for `id` (FL, FR, RL, RR; 0 = inflated).
    pub fn spike_puncture(&self, id: wp_contracts::EntityId) -> [f32; 4] {
        self.police.tire_puncture(id)
    }
}
