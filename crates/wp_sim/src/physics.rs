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
    /// Sidewalk top and kerb face triangles (every city with sidewalks, flat ones too).
    pub sidewalk_triangles: usize,
    /// Static building colliders.
    pub buildings: usize,
    /// Guard-rail boxes along bridge decks.
    pub rails: usize,
}

/// The flat ground slab is parked this far below the city when terrain replaces it (a safety
/// floor for anything that falls through the world).
pub const SAFETY_FLOOR_Z: f32 = -60.0;

/// Collision group of the terrain heightfield tiles and the safety floor (cities with
/// terrain). Resting props leave it out of their filter (see `Sim::set_world`).
pub const TERRAIN_GROUP: Group = Group::GROUP_2;

/// Collision group of the sidewalk and kerb colliders (every city with sidewalks). The
/// chassis of a car upright on its wheels leaves it out of its filter (`Sim` switches it per
/// tick): the wheels climb kerbs and carry the car over sidewalks.
pub const SIDEWALK_GROUP: Group = Group::GROUP_3;

/// Interaction groups of sidewalk colliders.
pub fn sidewalk_groups() -> InteractionGroups {
    InteractionGroups::new(SIDEWALK_GROUP, Group::ALL, InteractionTestMode::And)
}

/// Interaction groups of a vehicle chassis upright on its wheels: everything but sidewalks.
pub fn chassis_on_wheels_groups() -> InteractionGroups {
    InteractionGroups::new(
        Group::ALL,
        Group::ALL.difference(SIDEWALK_GROUP),
        InteractionTestMode::And,
    )
}

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

    /// Build the city's ground colliders. Every city gets its raised sidewalks and kerb faces
    /// ([`wp_world::surface::sidewalk_mesh`], the same triangles the greybox draws), so a car
    /// bumps up the kerb and drives on the sidewalk at its visual height. A flat city keeps
    /// the slab at z = 0 under them. A city with terrain (Port Vesper v2) replaces the slab:
    /// a terrain heightfield, one triangle mesh of every paved surface (banked ribbons,
    /// junction discs, bend connectors) and sidewalk, a static collider per building and
    /// guard rails along bridge decks; the slab moves down to [`SAFETY_FLOOR_Z`]. Colliders are
    /// inserted in a fixed order and registered with the broad phase immediately, so ground
    /// probes see them on the next tick without a physics step. Call once, before the first
    /// tick (scenario setup).
    pub fn set_city_ground(&mut self, map: &wp_world::CityMap) -> CityColliders {
        let net = &map.net;
        let mut out = CityColliders::default();
        let mut added: Vec<ColliderHandle> = Vec::new();
        let walks = wp_world::surface::sidewalk_mesh(map);
        out.sidewalk_triangles = walks.tops.indices.len() + walks.kerbs.indices.len();
        let mut walk_mesh = walks.tops;
        walk_mesh.append(&walks.kerbs);
        let Some(t) = &net.terrain else {
            self.insert_mesh_tiles(&walk_mesh, sidewalk_groups(), &mut added);
            self.register(&added);
            return out;
        };
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

        // Roads, then sidewalks: triangle meshes per 256 m cell.
        let mesh = wp_world::surface::road_mesh(map);
        out.road_triangles = mesh.indices.len();
        self.insert_mesh_tiles(&mesh, InteractionGroups::all(), &mut added);
        self.insert_mesh_tiles(&walk_mesh, sidewalk_groups(), &mut added);

        // Buildings: extruded footprints as convex hulls (footprints are convex lots).
        for b in &net.buildings {
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

        // Guard rails along decks: 0.3 m wide, 0.9 m tall boxes on each paved edge.
        for (a, b) in wp_world::surface::deck_rails(net) {
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
                (a[2] + b[2]) * 0.5 + 0.45,
            );
            let col = ColliderBuilder::cuboid(0.15, len * 0.5 + 0.05, 0.45 + d[2].abs() * 0.5)
                .position(Pose::from_parts(
                    mid,
                    rapier3d::math::Rotation::from_xyzw(0.0, 0.0, s, c),
                ))
                .friction(0.4)
                .build();
            added.push(self.colliders.insert(col));
            out.rails += 1;
        }

        self.register(&added);
        out
    }

    /// Insert `mesh` as static triangle meshes, one per 256 m cell (by triangle centroid,
    /// cells in a fixed order), so a body only pairs with the surface around it.
    fn insert_mesh_tiles(
        &mut self,
        mesh: &wp_world::surface::TriMesh,
        groups: InteractionGroups,
        added: &mut Vec<ColliderHandle>,
    ) {
        const TILE_M: f32 = 256.0;
        let mut tiles: std::collections::BTreeMap<(i32, i32), Vec<[u32; 3]>> =
            std::collections::BTreeMap::new();
        for tri in &mesh.indices {
            let c = tri.iter().fold([0.0f32; 2], |acc, i| {
                let p = mesh.positions[*i as usize];
                [acc[0] + p[0] / 3.0, acc[1] + p[1] / 3.0]
            });
            let key = (
                (c[0] / TILE_M).floor() as i32,
                (c[1] / TILE_M).floor() as i32,
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
                Ok(b) => added.push(self.colliders.insert(b.collision_groups(groups).build())),
                Err(e) => eprintln!("city ground trimesh failed: {e:?}"),
            }
        }
    }

    /// Register freshly inserted static colliders with the broad phase (queries see them
    /// before the next step).
    fn register(&mut self, added: &[ColliderHandle]) {
        for h in added {
            let aabb = self.colliders[*h].compute_aabb();
            self.broad_phase.set_aabb(&self.params, *h, aabb);
        }
    }

    /// Height of the first static surface (ground, road, sidewalk, deck, building roof) hit
    /// by a ray cast straight down from `(xy, from_z)`, at most `max_m` below it. Bodies
    /// (vehicles, props, debris) are ignored. Diagnostics and tests (the visual vs
    /// physics surface sweep).
    pub fn static_ground_z(&self, xy: [f32; 2], from_z: f32, max_m: f32) -> Option<f32> {
        // World geometry has no parent body (props and vehicles do).
        let world_only = |_: ColliderHandle, c: &Collider| c.parent().is_none();
        let qp = self.query_pipeline(QueryFilter::default().predicate(&world_only));
        let ray = Ray::new(
            Vector::new(xy[0], xy[1], from_z),
            Vector::new(0.0, 0.0, -1.0),
        );
        qp.cast_ray(&ray, max_m, true).map(|(_, toi)| from_z - toi)
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

impl Default for PhysicsWorld {
    fn default() -> Self {
        Self::new()
    }
}
