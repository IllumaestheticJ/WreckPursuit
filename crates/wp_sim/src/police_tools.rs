//! Police tools in the physics world.
//!
//! `wp_police` plans roadblocks and spike strips; this module makes roadblocks solid (owner,
//! 2026-10-28: "roadblocks completely block the road"). Every barrier of an active roadblock
//! (`PoliceSystem::roadblocks`) becomes a fixed Rapier box: the barrier line runs from wall to
//! wall (or barrier to barrier) with overlapping boxes, so no car squeezes through it. Its one
//! gap holds a wooden sawhorse barricade, also fixed until a car reaches it at
//! [`BARRICADE_BREAK_MPS`] or faster: then it breaks into [`BARRICADE_PIECES`] loose timber
//! pieces (dynamic, together [`wp_police::tools::BARRICADE_MASS_KG`]) that the car smashes
//! through, paying with the damage those pieces do and the speed they take. Slower, the
//! barricade stops the car like a fence. The bodies are removed when the roadblock expires.
//! Spike strips need no body: `wp_police` detects wheels crossing the strip and its punctures
//! are merged into each vehicle's handling effects in [`Sim::step_fixed`].
//!
//! Order is deterministic: roadblocks in `wp_police`'s list order, barriers in plan order.

use rapier3d::math::Rotation;
use rapier3d::prelude::{ColliderBuilder, Pose, RigidBodyBuilder, RigidBodyHandle, Vector};
use wp_contracts::{EntityId, ToolId, VehicleSim};

use crate::Sim;

/// A car reaching a roadblock's barricade at this closing speed or faster smashes it (about
/// 18 mph); slower, it stops against it, m/s.
pub const BARRICADE_BREAK_MPS: f32 = 8.0;

/// Pieces a broken barricade falls into: the top board, the lower rail and the two A-frame
/// legs.
pub const BARRICADE_PIECES: usize = 4;

/// What a roadblock body is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPart {
    /// Solid barrier (fixed).
    Barrier,
    /// The intact wooden barricade in the gap (fixed until broken).
    Barricade,
    /// A loose piece of a broken barricade (dynamic).
    Piece,
}

/// One physical roadblock body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ToolBody {
    /// Roadblock it belongs to.
    pub tool: ToolId,
    /// Rapier body.
    pub body: RigidBodyHandle,
    /// Half extents `[across, along, up]`, m.
    pub half_extents_m: [f32; 3],
    /// What it is.
    pub part: ToolPart,
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
    /// What it is (barrier, intact barricade, broken barricade piece).
    pub part: ToolPart,
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

        let new: Vec<(ToolId, EntityId, wp_police::StaticCollider, ToolPart)> = self
            .police
            .roadblocks()
            .iter()
            .filter(|r| !self.tool_bodies.iter().any(|t| t.tool == r.id))
            .flat_map(|r| {
                r.barriers
                    .iter()
                    .map(move |b| (r.id, r.racer, *b, ToolPart::Barrier))
                    .chain(
                        r.barricade
                            .map(|bc| (r.id, r.racer, bc.collider, ToolPart::Barricade)),
                    )
            })
            .collect();
        for (tool, racer, mut b, part) in new {
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
            let body = self.insert_tool_box(&b, part);
            self.tool_bodies.push(ToolBody {
                tool,
                body,
                half_extents_m: b.half_extents_m,
                part,
            });
        }
    }

    /// A box body for `b`: fixed for barriers and the intact barricade, dynamic (with `b`'s
    /// mass) for a loose piece.
    fn insert_tool_box(
        &mut self,
        b: &wp_police::StaticCollider,
        part: ToolPart,
    ) -> RigidBodyHandle {
        // Barrier forward (x, y) → yaw about +Z with 0 = +Y.
        let yaw = wp_math::atan2(-b.forward[0], b.forward[1]);
        let (s, c) = wp_math::sin_cos(yaw * 0.5);
        let pose = Pose::from_parts(
            Vector::from_array(b.pos),
            Rotation::from_xyzw(0.0, 0.0, s, c),
        );
        let builder = if part == ToolPart::Piece {
            RigidBodyBuilder::dynamic()
        } else {
            RigidBodyBuilder::fixed()
        };
        let body = self.physics.bodies.insert(builder.pose(pose).build());
        let [hx, hy, hz] = b.half_extents_m;
        let col = ColliderBuilder::cuboid(hx, hy, hz)
            .mass(b.mass_kg.max(1.0))
            .friction(if part == ToolPart::Barrier { 0.7 } else { 0.5 })
            .restitution(0.05)
            .build();
        self.physics
            .colliders
            .insert_with_parent(col, body, &mut self.physics.bodies);
        body
    }

    /// Before the physics step: smash every intact barricade a vehicle is about to hit at
    /// [`BARRICADE_BREAK_MPS`] or faster (closing speed toward it). The fixed board is
    /// replaced by loose pieces where it stood; the car meets them this step, so the damage
    /// and the speed lost come from the contacts like any other hit. Deterministic: barricades
    /// in body order, vehicles in slot order.
    pub(crate) fn break_barricades(&mut self) {
        if !self
            .tool_bodies
            .iter()
            .any(|t| t.part == ToolPart::Barricade)
        {
            return;
        }
        let dt = wp_contracts::PHYSICS_DT;
        let mut broken: Vec<usize> = Vec::new();
        for (k, t) in self.tool_bodies.iter().enumerate() {
            if t.part != ToolPart::Barricade {
                continue;
            }
            let rb = &self.physics.bodies[t.body];
            let c = rb.translation().to_array();
            let r = *rb.rotation();
            // Box axes in world XY (local +X across the road, +Y along it).
            let ax = r * Vector::new(1.0, 0.0, 0.0);
            let ay = r * Vector::new(0.0, 1.0, 0.0);
            let [hx, hy, hz] = t.half_extents_m;
            let hit = self.vehicles.iter().any(|v| {
                if v.ghost_ticks > 0 {
                    return false;
                }
                let st = v.vehicle.state();
                let d = [st.pos[0] - c[0], st.pos[1] - c[1], st.pos[2] - c[2]];
                if d[2].abs() > hz + 2.0 {
                    return false;
                }
                let lx = d[0] * ax.x + d[1] * ax.y;
                let ly = d[0] * ay.x + d[1] * ay.y;
                let vy = st.lin_vel[0] * ay.x + st.lin_vel[1] * ay.y;
                // Closing on the board along the road (either way).
                let closing = if ly < 0.0 { vy } else { -vy };
                // Car half length ~2.4 m: about to touch within this step and the next two.
                let reach = hy + 2.5 + closing.max(0.0) * 3.0 * dt;
                closing >= BARRICADE_BREAK_MPS && lx.abs() <= hx + 1.0 && ly.abs() <= reach
            });
            if hit {
                broken.push(k);
            }
        }
        for k in broken.into_iter().rev() {
            let t = self.tool_bodies.remove(k);
            let rb = &self.physics.bodies[t.body];
            let c = rb.translation().to_array();
            let r = *rb.rotation();
            let ax = r * Vector::new(1.0, 0.0, 0.0);
            let fwd = r * Vector::new(0.0, 1.0, 0.0);
            self.physics.remove_body(t.body);
            let [hx, _, hz] = t.half_extents_m;
            let piece_mass = wp_police::tools::BARRICADE_MASS_KG / BARRICADE_PIECES as f32;
            // (across offset, height offset, half extents): top board, lower rail, two legs.
            let parts = [
                (0.0, hz * 0.6, [hx, 0.04, 0.15]),
                (0.0, -hz * 0.1, [hx * 0.95, 0.04, 0.08]),
                (-hx + 0.15, -hz * 0.3, [0.06, 0.3, hz * 0.65]),
                (hx - 0.15, -hz * 0.3, [0.06, 0.3, hz * 0.65]),
            ];
            for (x, z, he) in parts {
                let b = wp_police::StaticCollider {
                    pos: [c[0] + ax.x * x, c[1] + ax.y * x, c[2] + z],
                    half_extents_m: he,
                    forward: [fwd.x, fwd.y],
                    mass_kg: piece_mass,
                };
                let body = self.insert_tool_box(&b, ToolPart::Piece);
                self.tool_bodies.push(ToolBody {
                    tool: t.tool,
                    body,
                    half_extents_m: he,
                    part: ToolPart::Piece,
                });
            }
            self.barricades_broken += 1;
        }
    }

    /// Barricades smashed so far.
    pub fn barricades_broken(&self) -> u32 {
        self.barricades_broken
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
                    part: t.part,
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
