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
    ground: ColliderHandle,
}

/// What [`PhysicsWorld::set_city_ground`] built.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CityColliders {
    /// Terrain heightfield vertices.
    pub terrain_vertices: usize,
    /// Road surface triangles.
    pub road_triangles: usize,
    /// Static building colliders.
    pub buildings: usize,
    /// Guard-rail boxes along bridge decks.
    pub rails: usize,
    /// Median barrier boxes between divided-highway carriageways.
    pub barriers: usize,
    /// Solid [`wp_world::network::Feature`]s turned into collider shapes (boxes, cylinders,
    /// convex prisms; format v4 cities such as Port Vesper v3).
    pub features: usize,
    /// Rapier colliders holding those shapes (one compound per [`FEATURE_TILE_M`] tile).
    pub feature_colliders: usize,
}

/// The flat ground slab is parked this far below the city when terrain replaces it (a safety
/// floor for anything that falls through the world).
pub const SAFETY_FLOOR_Z: f32 = -60.0;

/// Collision group of the terrain heightfield tiles and the safety floor (cities with
/// terrain). Resting props leave it out of their filter (see `Sim::set_world`).
pub const TERRAIN_GROUP: Group = Group::GROUP_2;

/// Interaction groups of terrain colliders.
pub fn terrain_groups() -> InteractionGroups {
    InteractionGroups::new(TERRAIN_GROUP, Group::ALL, InteractionTestMode::And)
}

/// Interaction groups of a resting prop: everything but the terrain (it sits on the road mesh).
pub fn resting_prop_groups() -> InteractionGroups {
    InteractionGroups::new(
        Group::ALL,
        Group::ALL.difference(TERRAIN_GROUP),
        InteractionTestMode::And,
    )
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
        let ground = colliders.insert(wp_vehicle::rig::ground_collider());
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
            ground,
        };
        // Prime the broad phase so ground queries work on the very first tick.
        world.step();
        world
    }

    /// Replace the flat ground slab by the city's own ground (cities with terrain, e.g.
    /// Port Vesper v2): a terrain heightfield, one triangle mesh of every paved surface
    /// (banked ribbons, junction discs, bend connectors), a static collider per building
    /// (not on flat ground: the grid city), guard rails along bridge decks and median
    /// barriers, plus a static collider for every solid feature (format v4, see
    /// [`feature_shape`]; grouped per [`FEATURE_TILE_M`] tile). The slab moves down to
    /// [`SAFETY_FLOOR_Z`]. Colliders are
    /// inserted in a fixed order and registered with the broad phase immediately, so ground
    /// probes see them on the next tick without a physics step. Call once, before the first
    /// tick (scenario setup).
    pub fn set_city_ground(&mut self, map: &wp_world::CityMap) -> CityColliders {
        let net = &map.net;
        let Some(t) = &net.terrain else {
            return CityColliders::default();
        };
        let mut out = CityColliders::default();
        let mut added: Vec<ColliderHandle> = Vec::new();
        // Park the slab.
        let hz = wp_vehicle::rig::GROUND_HALF_EXTENTS[2];
        if let Some(c) = self.colliders.get_mut(self.ground) {
            c.set_translation(Vector::new(0.0, 0.0, SAFETY_FLOOR_Z - hz));
            c.set_collision_groups(terrain_groups());
        }
        added.push(self.ground);

        // Terrain, in tiles of TILE_CELLS × TILE_CELLS cells (neighbours share their edge
        // vertices) so a body only pairs with the ground around it. Parry heightfields are Y-up
        // (rows along local z, columns along local x); rotating +90° about X maps local y →
        // world z and local z → world −y, so tile row r is the world row j1 − r.
        const TILE_CELLS: u32 = 64;
        let half = core::f32::consts::FRAC_1_SQRT_2;
        let mut j0 = 0;
        while j0 + 1 < t.ny {
            let j1 = (j0 + TILE_CELLS).min(t.ny - 1);
            let mut i0 = 0;
            while i0 + 1 < t.nx {
                let i1 = (i0 + TILE_CELLS).min(t.nx - 1);
                let (nc, nr) = ((i1 - i0 + 1) as usize, (j1 - j0 + 1) as usize);
                let mut heights = rapier3d::parry::utils::Array2::zeros(nr, nc);
                for r in 0..nr {
                    for c in 0..nc {
                        heights[(r, c)] = t.at(i0 + c as u32, j1 - r as u32);
                    }
                }
                let (sx, sy) = ((nc - 1) as f32 * t.cell_m, (nr - 1) as f32 * t.cell_m);
                let lo = t.vertex_pos(i0, j0);
                let pose = Pose::from_parts(
                    Vector::new(lo[0] + sx * 0.5, lo[1] + sy * 0.5, 0.0),
                    rapier3d::math::Rotation::from_xyzw(half, 0.0, 0.0, half),
                );
                let hf = ColliderBuilder::heightfield(heights, Vector::new(sx, 1.0, sy))
                    .position(pose)
                    .collision_groups(terrain_groups())
                    .build();
                added.push(self.colliders.insert(hf));
                i0 = i1;
            }
            j0 = j1;
        }
        out.terrain_vertices = (t.nx * t.ny) as usize;

        // Roads: one triangle mesh per 256 m cell (by triangle centroid).
        const ROAD_TILE_M: f32 = 256.0;
        let mesh = wp_world::surface::road_mesh(map);
        out.road_triangles = mesh.indices.len();
        let mut tiles: std::collections::BTreeMap<(i32, i32), Vec<[u32; 3]>> =
            std::collections::BTreeMap::new();
        for tri in &mesh.indices {
            let c = tri.iter().fold([0.0f32; 2], |acc, i| {
                let p = mesh.positions[*i as usize];
                [acc[0] + p[0] / 3.0, acc[1] + p[1] / 3.0]
            });
            let key = (
                (c[0] / ROAD_TILE_M).floor() as i32,
                (c[1] / ROAD_TILE_M).floor() as i32,
            );
            tiles.entry(key).or_default().push(*tri);
        }
        for tris in tiles.values() {
            let mut remap: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
            let mut verts: Vec<Vector> = Vec::new();
            let mut idx: Vec<[u32; 3]> = Vec::with_capacity(tris.len());
            for tri in tris {
                let mut out_tri = [0u32; 3];
                for (k, i) in tri.iter().enumerate() {
                    out_tri[k] = *remap.entry(*i).or_insert_with(|| {
                        verts.push(Vector::from_array(mesh.positions[*i as usize]));
                        (verts.len() - 1) as u32
                    });
                }
                idx.push(out_tri);
            }
            match ColliderBuilder::trimesh(verts, idx) {
                Ok(b) => added.push(self.colliders.insert(b.build())),
                Err(e) => eprintln!("road tile trimesh failed: {e:?}"),
            }
        }

        // Buildings: extruded footprints as convex hulls (footprints are convex lots). A city
        // on flat ground (the procedural grid, whose terrain only exists so its highway
        // viaduct is a real bridge) keeps its old behaviour: buildings do not block cars.
        let flat_ground = t.z.iter().all(|z| *z == t.z[0]);
        for b in net.buildings.iter().filter(|_| !flat_ground) {
            let mut pts: Vec<Vector> = Vec::with_capacity(b.footprint.len() * 2);
            for q in &b.footprint {
                pts.push(Vector::new(q[0], q[1], b.base_z_m));
                pts.push(Vector::new(q[0], q[1], b.roof_z()));
            }
            if let Some(cb) = ColliderBuilder::convex_hull(&pts) {
                added.push(self.colliders.insert(cb.friction(0.6).build()));
                out.buildings += 1;
            }
        }

        // Guard rails along decks: 0.3 m wide, 0.9 m tall boxes on each paved edge. Median
        // barriers: 0.5 m wide, 0.81 m tall concrete boxes on the barrier line.
        // (from, to, half width, half height, median barrier)
        type Rail = ([f32; 3], [f32; 3], f32, f32, bool);
        let rails: Vec<Rail> = wp_world::surface::deck_rails(net)
            .into_iter()
            .map(|(a, b)| (a, b, 0.15, 0.45, false))
            .chain(
                wp_world::surface::median_barriers(net)
                    .iter()
                    .flat_map(|l| l.windows(2).map(|w| (w[0], w[1], 0.25, 0.405, true)))
                    .collect::<Vec<_>>(),
            )
            .collect();
        for (a, b, half_w, half_h, barrier) in rails {
            let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
            if len < 0.05 {
                continue;
            }
            let yaw = wp_math::atan2(-d[0], d[1]);
            let (s, c) = wp_math::sin_cos(yaw * 0.5);
            let mid = Vector::new(
                (a[0] + b[0]) * 0.5,
                (a[1] + b[1]) * 0.5,
                (a[2] + b[2]) * 0.5 + half_h,
            );
            let col = ColliderBuilder::cuboid(half_w, len * 0.5 + 0.05, half_h + d[2].abs() * 0.5)
                .position(Pose::from_parts(
                    mid,
                    rapier3d::math::Rotation::from_xyzw(0.0, 0.0, s, c),
                ))
                .friction(0.4)
                .build();
            added.push(self.colliders.insert(col));
            if barrier {
                out.barriers += 1;
            } else {
                out.rails += 1;
            }
        }

        // Solid features (format v4): one compound collider per FEATURE_TILE_M tile, in tile
        // order, shapes in feature order inside it.
        let (n_shapes, handles) = self.insert_features(&net.features);
        out.features = n_shapes;
        out.feature_colliders = handles.len();
        added.extend(handles);

        for h in added {
            let aabb = self.colliders[h].compute_aabb();
            self.broad_phase.set_aabb(&self.params, h, aabb);
        }
        out
    }

    /// Insert the static colliders of every solid feature (no broad-phase registration):
    /// shapes grouped by the [`FEATURE_TILE_M`] tile of their centre into one compound
    /// collider per tile (a `BTreeMap` keyed by tile, so insertion order is fixed). Returns the
    /// number of shapes and the collider handles in insertion order.
    fn insert_features(
        &mut self,
        features: &[wp_world::network::Feature],
    ) -> (usize, Vec<ColliderHandle>) {
        // (tile, drivable) → shapes.
        type Parts = Vec<(Pose, SharedShape)>;
        let mut tiles: std::collections::BTreeMap<(i32, i32, bool), Parts> = Default::default();
        let mut n = 0;
        for f in features.iter().filter(|f| f.solid) {
            let Some((pose, shape)) = feature_shape(&f.shape) else {
                continue;
            };
            let key = (
                (pose.translation.x / FEATURE_TILE_M).floor() as i32,
                (pose.translation.y / FEATURE_TILE_M).floor() as i32,
                drivable(f.kind),
            );
            tiles.entry(key).or_default().push((pose, shape));
            n += 1;
        }
        let mut handles = Vec::with_capacity(tiles.len());
        for ((_, _, drive), parts) in tiles {
            let friction = if drive {
                FEATURE_DRIVABLE_FRICTION
            } else {
                FEATURE_FRICTION
            };
            let col = if parts.len() == 1 {
                let (pose, shape) = parts.into_iter().next().expect("one part");
                ColliderBuilder::new(shape).position(pose)
            } else {
                ColliderBuilder::compound(parts)
            };
            handles.push(self.colliders.insert(col.friction(friction).build()));
        }
        (n, handles)
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

    /// Remove a rigid body and its colliders (despawn). Deterministic: Rapier frees the
    /// handle; later insertions may reuse the slot with a new generation.
    pub fn remove_body(&mut self, body: RigidBodyHandle) {
        self.bodies.remove(
            body,
            &mut self.islands,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            &mut self.soft_bodies,
            true,
        );
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

/// Solid features are grouped into one compound collider per tile of this size (by shape
/// centre), m: tens of thousands of separate static colliders would bloat the broad phase.
pub const FEATURE_TILE_M: f32 = 256.0;
/// Friction of drivable feature colliders (decks, ramps, slabs, piers, platforms, earthworks).
pub const FEATURE_DRIVABLE_FRICTION: f32 = 0.8;
/// Friction of other solid features (walls, fences, poles, containers...).
pub const FEATURE_FRICTION: f32 = 0.5;

/// Feature kinds whose top is meant to be driven on (higher friction, see
/// [`FEATURE_DRIVABLE_FRICTION`]).
pub fn drivable(kind: wp_world::network::FeatureKind) -> bool {
    use wp_world::network::FeatureKind as K;
    matches!(
        kind,
        K::ParkingDeck
            | K::ParkingRamp
            | K::Runway
            | K::Taxiway
            | K::Apron
            | K::Pier
            | K::Platform
            | K::Earthwork
            | K::Tunnel
            | K::RailBridge
            | K::Ground
    )
}

/// Rotation of a feature box: pitch about local +X (positive = the +Y end up), then yaw about
/// world +Z (`wp_world::network::FeatureShape::Box`): `q = q_yaw · q_pitch`.
pub fn box_rotation(yaw: f32, pitch: f32) -> rapier3d::math::Rotation {
    let (sy, cy) = wp_math::sin_cos(yaw * 0.5);
    let (sp, cp) = wp_math::sin_cos(pitch * 0.5);
    rapier3d::math::Rotation::from_xyzw(cy * sp, sy * sp, cp * sy, cy * cp)
}

/// Collider pose and shape of one feature shape (world space): a cuboid for a box, a cylinder
/// rotated from Rapier's +Y axis onto +Z, the convex hull of footprint × `[base_z, top_z]` for
/// a prism. `None` for degenerate shapes (zero size, fewer than 3 footprint points).
pub fn feature_shape(shape: &wp_world::network::FeatureShape) -> Option<(Pose, SharedShape)> {
    use rapier3d::math::Rotation;
    use wp_world::network::FeatureShape as S;
    const MIN: f32 = 1e-3;
    match shape {
        S::Box {
            centre,
            half,
            yaw,
            pitch,
        } => {
            if half.iter().any(|h| !h.is_finite() || *h <= MIN) {
                return None;
            }
            Some((
                Pose::from_parts(Vector::from_array(*centre), box_rotation(*yaw, *pitch)),
                SharedShape::cuboid(half[0], half[1], half[2]),
            ))
        }
        S::Cylinder {
            base,
            radius,
            height,
        } => {
            if !(radius.is_finite() && height.is_finite()) || *radius <= MIN || *height <= MIN {
                return None;
            }
            let h = core::f32::consts::FRAC_1_SQRT_2;
            Some((
                Pose::from_parts(
                    Vector::new(base[0], base[1], base[2] + height * 0.5),
                    // +90° about X: local +Y (Rapier's cylinder axis) → world +Z.
                    Rotation::from_xyzw(h, 0.0, 0.0, h),
                ),
                SharedShape::cylinder(height * 0.5, *radius),
            ))
        }
        S::Prism {
            footprint,
            base_z,
            top_z,
        } => {
            let h = *top_z - *base_z;
            if footprint.len() < 3 || !h.is_finite() || h <= MIN {
                return None;
            }
            // Hull in local coordinates around the footprint's mean point (keeps the compound
            // BVH and f32 precision sane far from the origin).
            let k = footprint.len() as f32;
            let c = footprint
                .iter()
                .fold([0.0f32; 2], |a, p| [a[0] + p[0] / k, a[1] + p[1] / k]);
            let cz = (base_z + top_z) * 0.5;
            let hz = (top_z - base_z) * 0.5;
            let mut pts = Vec::with_capacity(footprint.len() * 2);
            for p in footprint {
                pts.push(Vector::new(p[0] - c[0], p[1] - c[1], -hz));
                pts.push(Vector::new(p[0] - c[0], p[1] - c[1], hz));
            }
            let shape = SharedShape::convex_hull(&pts)?;
            Some((Pose::from_translation(Vector::new(c[0], c[1], cz)), shape))
        }
    }
}

impl Default for PhysicsWorld {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_world::metro::ctx::{box_on, ramp};
    use wp_world::network::{FeatureKind, FeatureShape, Material};

    fn close(a: Vector, b: [f32; 3], tol: f32) -> bool {
        (a - Vector::from_array(b)).length() < tol
    }

    /// Height of the shape's top surface under `xy` (ray cast straight down from 100 m).
    fn top_at(pose: &Pose, shape: &SharedShape, xy: [f32; 2]) -> Option<f32> {
        let ray = rapier3d::parry::query::Ray::new(Vector::new(xy[0], xy[1], 100.0), -Vector::Z);
        shape
            .cast_ray(pose, &ray, 1000.0, true)
            .map(|toi| 100.0 - toi)
    }

    #[test]
    fn ramp_top_centreline_passes_through_its_end_points() {
        // Several directions and slopes, far from the origin.
        let cases = [
            ([1200.0, -800.0, 3.0], [1200.0, -758.0, 9.0]),
            ([-300.0, 40.0, 0.0], [-342.0, 40.0, 6.0]),
            ([10.0, 10.0, 12.0], [40.0, -20.0, 6.0]),
            ([0.0, 0.0, 0.0], [-20.0, -25.0, 4.0]),
        ];
        for (a, b) in cases {
            let f = ramp(FeatureKind::ParkingRamp, Material::Concrete, a, b, 6.0, 0.4);
            let FeatureShape::Box { half, .. } = f.shape else {
                panic!("ramp is a box");
            };
            let (pose, shape) = feature_shape(&f.shape).unwrap();
            // The top face's centreline ends: local (0, ∓hy, +hz).
            let start = pose.transform_point(Vector::new(0.0, -half[1], half[2]));
            let end = pose.transform_point(Vector::new(0.0, half[1], half[2]));
            assert!(close(start, a, 1e-3), "start {start:?} vs {a:?}");
            assert!(close(end, b, 1e-3), "end {end:?} vs {b:?}");
            // Ray casts onto the collider's top along the centreline follow the line a → b.
            for t in [0.1f32, 0.5, 0.9] {
                let p = [
                    a[0] + (b[0] - a[0]) * t,
                    a[1] + (b[1] - a[1]) * t,
                    a[2] + (b[2] - a[2]) * t,
                ];
                let z = top_at(&pose, &shape, [p[0], p[1]]).expect("ray hits the ramp");
                assert!((z - p[2]).abs() < 1e-2, "t {t}: top {z} vs {}", p[2]);
            }
        }
    }

    #[test]
    fn yaw_turns_local_x_to_cos_sin() {
        for yaw in [0.0f32, 0.4, 1.7, -2.5] {
            let q = box_rotation(yaw, 0.0);
            let x = q * Vector::X;
            let (s, c) = wp_math::sin_cos(yaw);
            assert!(close(x, [c, s, 0.0], 1e-5), "{yaw}: {x:?}");
            let y = q * Vector::Y;
            assert!(close(y, [-s, c, 0.0], 1e-5), "{yaw}: {y:?}");
        }
        // Positive pitch lifts the +Y end.
        let y = box_rotation(0.0, 0.3) * Vector::Y;
        assert!(y.z > 0.29 && y.y > 0.95, "{y:?}");
    }

    #[test]
    fn cylinders_stand_upright_and_prisms_span_base_to_top() {
        let (pose, shape) = feature_shape(&FeatureShape::Cylinder {
            base: [50.0, -20.0, 2.0],
            radius: 1.5,
            height: 8.0,
        })
        .unwrap();
        assert!((top_at(&pose, &shape, [50.0, -20.0]).unwrap() - 10.0).abs() < 1e-3);
        assert!((top_at(&pose, &shape, [51.4, -20.0]).unwrap() - 10.0).abs() < 1e-3);
        assert!(top_at(&pose, &shape, [51.6, -20.0]).is_none());
        let (pose, shape) = feature_shape(&FeatureShape::Prism {
            footprint: vec![[100.0, 100.0], [120.0, 100.0], [110.0, 115.0]],
            base_z: 1.0,
            top_z: 7.0,
        })
        .unwrap();
        assert!((top_at(&pose, &shape, [110.0, 105.0]).unwrap() - 7.0).abs() < 1e-3);
        assert!(top_at(&pose, &shape, [101.0, 114.0]).is_none());
        // A flat box's top is centre + half height.
        let f = box_on(
            FeatureKind::ParkingDeck,
            Material::Concrete,
            [5.0, 5.0],
            6.0,
            [10.0, 20.0, 0.3],
            0.7,
            true,
        );
        let (pose, shape) = feature_shape(&f.shape).unwrap();
        assert!((top_at(&pose, &shape, [5.0, 5.0]).unwrap() - 6.3).abs() < 1e-4);
        // Degenerate shapes make no collider.
        assert!(feature_shape(&FeatureShape::Prism {
            footprint: vec![[0.0, 0.0], [1.0, 0.0]],
            base_z: 0.0,
            top_z: 1.0
        })
        .is_none());
    }
}
