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

/// A smashed barricade costs the car the damage of hitting this much mass at its closing
/// speed (the timber frame with its sandbags, taken as one lump), kg.
pub const BARRICADE_HIT_MASS_KG: f32 = 260.0;

/// Unit forward (+Y) of a chassis with orientation `q` (`[x, y, z, w]`), world XY.
fn r_forward(q: [f32; 4]) -> [f32; 2] {
    let [x, y, z, w] = q;
    // Second column of the rotation matrix.
    [2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z)]
}

/// Pieces a broken barricade falls into: the two halves of the top board and of the lower
/// rail, and the two A-frame legs.
pub const BARRICADE_PIECES: usize = 6;

/// What [`Sim::probe_box`] may hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeFilter {
    /// Static world geometry (buildings, walls, barriers, rails, roads, sidewalks; never the
    /// terrain heightfield).
    pub world: bool,
    /// Solid roadblock barriers.
    pub barriers: bool,
    /// Roadblock barricades (intact, or the pieces of a smashed one).
    pub barricade: bool,
    /// Every other body: vehicles, props, debris.
    pub bodies: bool,
}

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

        let new: Vec<(
            ToolId,
            EntityId,
            Option<f32>,
            wp_police::StaticCollider,
            ToolPart,
        )> = self
            .police
            .roadblocks()
            .iter()
            .filter(|r| !self.tool_bodies.iter().any(|t| t.tool == r.id))
            .flat_map(|r| {
                r.barriers
                    .iter()
                    .map(move |b| (r.id, r.racer, r.z_m, *b, ToolPart::Barrier))
                    .chain(
                        r.barricade
                            .map(|bc| (r.id, r.racer, r.z_m, bc.collider, ToolPart::Barricade)),
                    )
            })
            .collect();
        for (tool, racer, road_z, mut b, part) in new {
            // wp_police puts a barrier's centre its half height above z = 0. On a terrain city
            // that is under the road (owner, 2026-10-26: an invisible barrier in the Talon
            // Tunnel; its barriers spawned 34 m under the tunnel floor and fell out of the
            // world): stand it on the road under it, the surface nearest its racer's height
            // (the tunnel floor, not the hillside; the road on a bridge, not under it).
            if self.has_terrain() {
                // The road height wp_police laid the block out at, else its racer's.
                let near = road_z.or_else(|| {
                    self.vehicles
                        .iter()
                        .find(|v| v.id == racer)
                        .map(|v| v.vehicle.state().pos[2])
                });
                // A box across a kerb, a lot edge or a camber stands on the lowest ground under
                // it and reaches its full height above the highest (no gap over its high end).
                let across = [b.forward[1], -b.forward[0]];
                let hx = b.half_extents_m[0];
                let map = self.world.map();
                let zs = [-1.0f32, 0.0, 1.0].map(|k| {
                    map.ground_height(
                        [b.pos[0] + across[0] * hx * k, b.pos[1] + across[1] * hx * k],
                        near,
                    )
                });
                let (lo, hi) = (zs[0].min(zs[1]).min(zs[2]), zs[0].max(zs[1]).max(zs[2]));
                let hz = b.half_extents_m[2];
                b.half_extents_m[2] = hz + 0.5 * (hi - lo);
                b.pos[2] = lo + b.half_extents_m[2];
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
        let mut broken: Vec<(usize, (usize, f32, bool))> = Vec::new();
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
            let hit = self.vehicles.iter().enumerate().find_map(|(slot, v)| {
                if v.ghost_ticks > 0 {
                    return None;
                }
                let st = v.vehicle.state();
                let d = [st.pos[0] - c[0], st.pos[1] - c[1], st.pos[2] - c[2]];
                if d[2].abs() > hz + 2.0 {
                    return None;
                }
                let lx = d[0] * ax.x + d[1] * ax.y;
                let ly = d[0] * ay.x + d[1] * ay.y;
                let vy = st.lin_vel[0] * ay.x + st.lin_vel[1] * ay.y;
                // Closing on the board along the road (either way).
                let closing = if ly < 0.0 { vy } else { -vy };
                // Car half length ~2.4 m: about to touch within this step and the next two.
                let reach = hy + 2.5 + closing.max(0.0) * 3.0 * dt;
                let smash =
                    closing >= BARRICADE_BREAK_MPS && lx.abs() <= hx + 1.0 && ly.abs() <= reach;
                // Front or rear first: the way the car is rolling.
                let fwd = r_forward(st.rot);
                let front = st.lin_vel[0] * fwd[0] + st.lin_vel[1] * fwd[1] >= 0.0;
                smash.then_some((slot, closing, front))
            });
            if let Some(h) = hit {
                broken.push((k, h));
            }
        }
        for (k, (slot, closing, front)) in broken.into_iter().rev() {
            // The damage cost: the frame and its sandbags hit as one lump of
            // BARRICADE_HIT_MASS_KG at the closing speed, on the leading bumper (the loose
            // pieces then take some speed off as the car ploughs through them).
            let sign = if front { 1.0 } else { -1.0 };
            self.barricade_hits.push((
                slot,
                wp_contracts::ContactImpulse {
                    point_local_m: [0.0, sign * 2.1, 0.35],
                    normal_local: [0.0, -sign, 0.0],
                    impulse_ns: BARRICADE_HIT_MASS_KG * closing,
                    tangent_impulse_local_ns: [0.0; 3],
                    rel_speed_mps: closing,
                    other: None,
                    other_mass_kg: BARRICADE_HIT_MASS_KG,
                },
            ));
            let t = self.tool_bodies.remove(k);
            let rb = &self.physics.bodies[t.body];
            let c = rb.translation().to_array();
            let r = *rb.rotation();
            let ax = r * Vector::new(1.0, 0.0, 0.0);
            let fwd = r * Vector::new(0.0, 1.0, 0.0);
            self.physics.remove_body(t.body);
            let [hx, _, hz] = t.half_extents_m;
            // The legs carry the sandbags that weigh the sawhorse down.
            let total = wp_police::tools::BARRICADE_MASS_KG;
            let (board_kg, leg_kg) = (total * 0.06, total * 0.38);
            // Every piece is shorter than the gap (the boards split in the middle), so the car
            // carries them through it instead of jamming them between the barrier ends.
            let gap = hx - wp_police::tools::BARRIER_OVERLAP_M;
            let board = 0.5 * gap - 0.08;
            // (across offset, height offset, half extents, mass): the top board's and the lower
            // rail's halves, and the two A-frame legs with their sandbags.
            let parts = [
                (-0.5 * gap, hz * 0.6, [board, 0.04, 0.15], board_kg),
                (0.5 * gap, hz * 0.6, [board, 0.04, 0.15], board_kg),
                (-0.5 * gap, -hz * 0.1, [board, 0.04, 0.08], board_kg),
                (0.5 * gap, -hz * 0.1, [board, 0.04, 0.08], board_kg),
                (-gap + 0.2, -hz * 0.3, [0.06, 0.3, hz * 0.65], leg_kg),
                (gap - 0.2, -hz * 0.3, [0.06, 0.3, hz * 0.65], leg_kg),
            ];
            for (x, z, he, mass_kg) in parts {
                let b = wp_police::StaticCollider {
                    pos: [c[0] + ax.x * x, c[1] + ax.y * x, c[2] + z],
                    half_extents_m: he,
                    forward: [fwd.x, fwd.y],
                    mass_kg,
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

    /// Whether a box with half extents `half` (`[across, along, up]`, local +Y along `dir`),
    /// centred at `centre`, swept `dist_m` along `dir` hits anything `filter` lets through.
    /// The terrain heightfield is never hit (a probe across the road tests what stands on
    /// it). Scene queries see the colliders as of the last physics step. For tests and tools
    /// (the roadblock seal probes).
    pub fn probe_box(
        &self,
        centre: [f32; 3],
        half: [f32; 3],
        dir: [f32; 2],
        dist_m: f32,
        filter: ProbeFilter,
    ) -> bool {
        use rapier3d::parry::query::ShapeCastOptions;
        use rapier3d::parry::shape::Cuboid;
        use rapier3d::prelude::{Collider, ColliderHandle, QueryFilter};
        let parts: Vec<(RigidBodyHandle, ToolPart)> =
            self.tool_bodies.iter().map(|t| (t.body, t.part)).collect();
        let pred = |_: ColliderHandle, c: &Collider| match c.parent() {
            None => filter.world && c.shape().as_heightfield().is_none(),
            Some(b) => match parts.iter().find(|(h, _)| *h == b).map(|(_, p)| *p) {
                Some(ToolPart::Barrier) => filter.barriers,
                Some(ToolPart::Barricade) | Some(ToolPart::Piece) => filter.barricade,
                None => filter.bodies,
            },
        };
        let qp = self
            .physics
            .query_pipeline(QueryFilter::default().predicate(&pred));
        let yaw = wp_math::atan2(-dir[0], dir[1]);
        let (s, c) = wp_math::sin_cos(yaw * 0.5);
        let pose = Pose::from_parts(
            Vector::from_array(centre),
            Rotation::from_xyzw(0.0, 0.0, s, c),
        );
        let shape = Cuboid::new(Vector::from_array(half));
        let vel = Vector::new(dir[0], dir[1], 0.0);
        qp.cast_shape(
            &pose,
            vel,
            &shape,
            ShapeCastOptions::with_max_time_of_impact(dist_m),
        )
        .is_some()
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

    /// Spike strips on the road, one view per segment (a wide road's strip can have several,
    /// all its unit's).
    pub fn spike_strips(&self) -> Vec<SpikeStripView> {
        self.police
            .spike_strips()
            .iter()
            .flat_map(|s| {
                s.segments()
                    .map(move |(a, b)| SpikeStripView { tool: s.id.0, a, b })
            })
            .collect()
    }

    /// Per-wheel tire deflation from spike strips for `id` (FL, FR, RL, RR; 0 = inflated).
    pub fn spike_puncture(&self, id: wp_contracts::EntityId) -> [f32; 4] {
        self.police.tire_puncture(id)
    }
}
