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
    /// Guard-rail segments along bridge decks and bare sidewalk backs (one smooth mesh
    /// collider per chained run).
    pub rails: usize,
    /// Wall and roof boxes of covered roads (tunnels).
    pub tunnel_boxes: usize,
    /// Tree trunks, hedges and fences (residential lots, playtest row 95).
    pub decor: usize,
    /// Median barrier segments between divided-highway carriageways (one smooth mesh collider
    /// per barrier line).
    pub barriers: usize,
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

/// Collider `user_data` bit of a ghosted vehicle chassis (see [`GhostHooks`]).
pub const GHOST_USER_BIT: u128 = 1;

/// Collider `user_data` bit of a city prop (parked car, cone, bin): never solid for a ghost,
/// even while it rests as a fixed body.
pub const PROP_USER_BIT: u128 = 2;

/// Contact filter for ghosted vehicles (a car reset in place, playtest row 65). A ghost chassis
/// keeps its contacts with static world geometry (ground slab, roads, terrain, sidewalks,
/// buildings, rails, tunnel boxes, scenario barriers: colliders without a body or on a fixed
/// body) and passes through everything that moves: other cars, props and debris. Only pairs
/// with a ghost reach this hook (ghost chassis carry `ActiveHooks::FILTER_CONTACT_PAIRS`), so
/// every other pair keeps Rapier's default handling. Stateless: deterministic.
pub struct GhostHooks;

impl PhysicsHooks for GhostHooks {
    fn filter_contact_pair(&self, ctx: &PairFilterContext) -> Option<SolverFlags> {
        let c1 = &ctx.colliders[ctx.collider1];
        let c2 = &ctx.colliders[ctx.collider2];
        let g1 = c1.user_data & GHOST_USER_BIT != 0;
        let g2 = c2.user_data & GHOST_USER_BIT != 0;
        if !g1 && !g2 {
            return Some(SolverFlags::default());
        }
        if g1 && g2 {
            return None;
        }
        let (other, other_body) = if g1 {
            (c2, ctx.rigid_body2)
        } else {
            (c1, ctx.rigid_body1)
        };
        let static_world = other.user_data & PROP_USER_BIT == 0
            && other_body.is_none_or(|h| ctx.bodies.get(h).is_some_and(|b| b.is_fixed()));
        static_world.then(SolverFlags::default)
    }
}

/// Mark (or unmark) a chassis collider as a ghost for [`GhostHooks`].
pub fn set_ghost_flags(col: &mut Collider, ghost: bool) {
    let hooks = col.active_hooks();
    if ghost {
        col.user_data |= GHOST_USER_BIT;
        col.set_active_hooks(hooks | ActiveHooks::FILTER_CONTACT_PAIRS);
    } else {
        col.user_data &= !GHOST_USER_BIT;
        col.set_active_hooks(hooks.difference(ActiveHooks::FILTER_CONTACT_PAIRS));
    }
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

/// A full copy of the Rapier state. `PhysicsPipeline` only holds per-step scratch buffers and
/// counters, so the copy gets a fresh one (stepping it gives the same results).
impl Clone for PhysicsWorld {
    fn clone(&self) -> Self {
        Self {
            gravity: self.gravity,
            params: self.params,
            bodies: self.bodies.clone(),
            colliders: self.colliders.clone(),
            pipeline: PhysicsPipeline::new(),
            islands: self.islands.clone(),
            broad_phase: self.broad_phase.clone(),
            narrow_phase: self.narrow_phase.clone(),
            impulse_joints: self.impulse_joints.clone(),
            multibody_joints: self.multibody_joints.clone(),
            soft_bodies: self.soft_bodies.clone(),
            ccd: self.ccd.clone(),
            ground: self.ground,
        }
    }
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
        let mut added: Vec<ColliderHandle> = Vec::new();
        let Some((colliders, out)) = city_ground_colliders(map) else {
            // Flat city: the slab stays; only its sidewalks and kerbs are added.
            let (colliders, sidewalk_triangles) = city_sidewalk_colliders(map);
            for c in colliders {
                added.push(self.colliders.insert(c));
            }
            self.register(&added);
            return CityColliders {
                sidewalk_triangles,
                ..CityColliders::default()
            };
        };
        // Park the slab.
        let hz = wp_vehicle::rig::GROUND_HALF_EXTENTS[2];
        if let Some(c) = self.colliders.get_mut(self.ground) {
            c.set_translation(Vector::new(0.0, 0.0, SAFETY_FLOOR_Z - hz));
            c.set_collision_groups(terrain_groups());
        }
        added.push(self.ground);
        for c in colliders {
            added.push(self.colliders.insert(c));
        }
        self.register(&added);
        out
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

    /// Height of the drivable ground (road and sidewalk meshes, terrain: the triangle-mesh and
    /// heightfield colliders, never buildings, rails, decor or bodies) hit by a ray straight
    /// down from `(xy, from_z)`, at most `max_m` below it. Seats parked cars on what is drawn.
    pub fn drivable_ground_z(&self, xy: [f32; 2], from_z: f32, max_m: f32) -> Option<f32> {
        let ground = |_: ColliderHandle, c: &Collider| {
            c.parent().is_none()
                && (c.shape().as_trimesh().is_some() || c.shape().as_heightfield().is_some())
        };
        let qp = self.query_pipeline(QueryFilter::default().predicate(&ground));
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

    /// Handles of the awake (non-sleeping) dynamic and kinematic bodies.
    pub fn active_bodies(&self) -> impl Iterator<Item = RigidBodyHandle> + '_ {
        self.islands.active_bodies()
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
            &GhostHooks,
            &(),
        );
    }
}

impl Default for PhysicsWorld {
    fn default() -> Self {
        Self::new()
    }
}

/// The static colliders of a city with terrain (Port Vesper v2), in the order
/// [`PhysicsWorld::set_city_ground`] inserts them: terrain heightfield tiles, road triangle
/// meshes, buildings (not on flat ground), deck guard rails, median barriers, tunnel walls
/// and roofs. `None` for flat cities (they keep the slab). Shared
/// with the client prediction stepper (`wp_vehicle::rig::Rig::set_city_ground`, used by
/// `wp_net`) so a predicted car drives on exactly the server's ground.
pub fn city_ground_colliders(map: &wp_world::CityMap) -> Option<(Vec<Collider>, CityColliders)> {
    let net = &map.net;
    // The terrain as drawn (fine heights only along at-grade roads), not the raw
    // `net.terrain`, which stands up to ~10 m above the greybox ground on slopes.
    let t = map.ground.as_deref()?;
    let mut out = CityColliders::default();
    let mut added: Vec<Collider> = Vec::new();

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
            added.push(hf);
            i0 = i1;
        }
        j0 = j1;
    }
    out.terrain_vertices = (t.nx * t.ny) as usize;

    // Roads, then sidewalks and kerbs: triangle meshes per 256 m cell.
    let mesh = wp_world::surface::road_mesh(map);
    out.road_triangles = mesh.indices.len();
    added.extend(mesh_tile_colliders(&mesh, InteractionGroups::all()));
    let walks = wp_world::surface::sidewalk_mesh(map);
    let (walk_colliders, sidewalk_triangles) = sidewalk_mesh_colliders(&walks);
    out.sidewalk_triangles = sidewalk_triangles;
    added.extend(walk_colliders);

    // Buildings: extruded footprints as convex hulls (footprints are convex lots), pitched
    // roofs included (a gable or hip house is still convex). A city on flat ground (the
    // procedural grid, whose terrain only exists so its highway viaduct is a real bridge)
    // keeps its old behaviour: buildings do not block cars.
    let flat_ground = !wp_world::surface::has_relief(net);
    for b in net.buildings.iter().filter(|_| !flat_ground) {
        let mut pts: Vec<Vector> = Vec::with_capacity(b.footprint.len() * 2 + 2);
        for q in &b.footprint {
            pts.push(Vector::new(q[0], q[1], b.base_z_m));
            pts.push(Vector::new(q[0], q[1], b.roof_z()));
        }
        for r in b.ridge().into_iter().flatten() {
            pts.push(Vector::new(r[0], r[1], r[2]));
        }
        if let Some(cb) = ColliderBuilder::convex_hull(&pts) {
            added.push(cb.friction(0.6).build());
            out.buildings += 1;
        }
    }

    // Guard rails along decks: 0.3 m wide, 0.9 m tall on each paved edge, one smooth mesh per
    // chained run of rail segments (row 164: no seams for a scraping car to catch).
    for run in chain_segments(&wp_world::surface::deck_rails(net)) {
        added.extend(barrier_colliders(&run, 0.0, RAIL_HALF_W_M, RAIL_HEIGHT_M));
        out.rails += run.len() - 1;
    }
    // Median barriers between divided-highway carriageways: 0.5 m wide, 0.81 m tall concrete,
    // one smooth mesh per barrier line.
    for line in wp_world::surface::median_barriers(net) {
        added.extend(barrier_colliders(
            &line,
            0.0,
            BARRIER_HALF_W_M,
            BARRIER_HEIGHT_M,
        ));
        out.barriers += line.len() - 1;
    }
    // And just outside the back edge of every sidewalk without a lot behind it (decks and
    // their approaches: the ground falls away there).
    for run in chain_segments(&walks.bare_backs) {
        added.extend(barrier_colliders(&run, 0.15, RAIL_HALF_W_M, RAIL_HEIGHT_M));
        out.rails += run.len() - 1;
    }

    // Covered roads: a wall box along each side (inner face at the span's half width, from
    // below the banked road edge up to the ceiling) and the roof deck, per centreline segment.
    for span in &net.covered {
        let shape = wp_world::covered::CoverShape::new(span);
        let c = &span.centre;
        let deck_half = shape.deck_half_width();
        let wall = wp_world::covered::WALL_M;
        let roof = wp_world::covered::ROOF_M;
        for k in 0..c.len().saturating_sub(1) {
            let (a, b) = (c[k], c[k + 1]);
            let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
            if len < 0.05 {
                continue;
            }
            let yaw = wp_math::atan2(-d[0], d[1]);
            let (s, co) = wp_math::sin_cos(yaw * 0.5);
            let rot = rapier3d::math::Rotation::from_xyzw(0.0, 0.0, s, co);
            let right = [d[1] / len, -d[0] / len];
            let mid = [
                (a[0] + b[0]) * 0.5,
                (a[1] + b[1]) * 0.5,
                (a[2] + b[2]) * 0.5,
            ];
            let slope = d[2].abs() * 0.5;
            // Walls: 1 m below the centreline (banked edges) up to the ceiling.
            let (z0, z1) = (mid[2] - 1.0, mid[2] + span.clearance_m);
            for side in [-1.0f32, 1.0] {
                let off = side * (span.half_width_m + wall * 0.5);
                let centre = Vector::new(
                    mid[0] + right[0] * off,
                    mid[1] + right[1] * off,
                    (z0 + z1) * 0.5,
                );
                let col =
                    ColliderBuilder::cuboid(wall * 0.5, len * 0.5 + 0.05, (z1 - z0) * 0.5 + slope)
                        .position(Pose::from_parts(centre, rot))
                        .friction(0.5)
                        .build();
                added.push(col);
                out.tunnel_boxes += 1;
            }
            let roof_mid = Vector::new(mid[0], mid[1], mid[2] + span.clearance_m + roof * 0.5);
            let col = ColliderBuilder::cuboid(deck_half, len * 0.5 + 0.05, roof * 0.5 + slope)
                .position(Pose::from_parts(roof_mid, rot))
                .friction(0.6)
                .build();
            added.push(col);
            out.tunnel_boxes += 1;
        }
    }

    // Street and yard furniture: tree trunks, hedges and fences (driveways are flush).
    // Like buildings: none on flat ground (the grid kept the slab and its sidewalks only before
    // main's viaduct gave it a flat heightfield).
    for d in net.decor.iter().filter(|_| !flat_ground) {
        if let Some(col) = decor_collider(d) {
            added.push(col);
            out.decor += 1;
        }
    }

    Some((added, out))
}

/// Static collider of one piece of street / yard furniture: a tree trunk (cylinder up to the
/// crown, 2.5 m), a hedge or fence (box). `None` for flush decor (driveways).
pub fn decor_collider(d: &wp_world::network::Decor) -> Option<Collider> {
    use wp_world::network::DecorKind;
    let (s, c) = wp_math::sin_cos(d.yaw * 0.5);
    let rot = rapier3d::math::Rotation::from_xyzw(0.0, 0.0, s, c);
    match d.kind {
        DecorKind::StreetTree | DecorKind::YardTree => {
            let h = 2.5f32.min(d.size[2]);
            // Parry cylinders stand along local Y: rotate +90° about X so it stands on Z.
            let half = core::f32::consts::FRAC_1_SQRT_2;
            let up = rapier3d::math::Rotation::from_xyzw(half, 0.0, 0.0, half);
            Some(
                ColliderBuilder::cylinder(h * 0.5, wp_world::residential::TRUNK_R_M)
                    .position(Pose::from_parts(
                        Vector::new(d.pos[0], d.pos[1], d.pos[2] + h * 0.5 - 0.1),
                        up,
                    ))
                    .friction(0.6)
                    .build(),
            )
        }
        DecorKind::Hedge | DecorKind::Fence => Some(
            ColliderBuilder::cuboid(d.size[0] * 0.5, d.size[1] * 0.5, d.size[2] * 0.5 + 0.1)
                .position(Pose::from_parts(
                    Vector::new(d.pos[0], d.pos[1], d.pos[2] + d.size[2] * 0.5 - 0.1),
                    rot,
                ))
                .friction(0.6)
                .build(),
        ),
        DecorKind::Driveway => None,
    }
}

/// Guard rail half width, m (0.3 m wide).
pub const RAIL_HALF_W_M: f32 = 0.15;
/// Guard rail height above its foot line, m.
pub const RAIL_HEIGHT_M: f32 = 0.9;
/// Median barrier half width, m (0.5 m wide concrete).
pub const BARRIER_HALF_W_M: f32 = 0.25;
/// Median barrier height above the road edge, m.
pub const BARRIER_HEIGHT_M: f32 = 0.81;
/// How far a barrier or rail mesh reaches below its foot line, m (closes the gap to a banked or
/// sloped surface under it).
const BARRIER_FOOT_M: f32 = 0.3;
/// Friction of barriers and rails: smooth concrete and steel a car slides along (row 164).
const BARRIER_FRICTION: f32 = 0.3;

/// Length of the 2D vector `(x, y)` (plain `sqrt`, which is exactly rounded, not `hypot`).
fn len2(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

/// Chain `segs` (`a → b` pieces, in order) into polylines: a piece starting exactly where an
/// open run ends extends it. Deterministic (runs keyed by their end point's bits).
fn chain_segments(segs: &[([f32; 3], [f32; 3])]) -> Vec<Vec<[f32; 3]>> {
    let key = |p: [f32; 3]| (p[0].to_bits(), p[1].to_bits(), p[2].to_bits());
    let mut runs: Vec<Vec<[f32; 3]>> = Vec::new();
    let mut open: std::collections::BTreeMap<(u32, u32, u32), usize> =
        std::collections::BTreeMap::new();
    for &(a, b) in segs {
        match open.remove(&key(a)) {
            Some(i) => {
                runs[i].push(b);
                open.insert(key(b), i);
            }
            None => {
                runs.push(vec![a, b]);
                open.insert(key(b), runs.len() - 1);
            }
        }
    }
    runs
}

/// Static colliders of a barrier along the polyline `line` (its foot, shifted `right` m to its
/// right), `2 · half_w` m wide and `height` m tall (row 164: a car glancing into it slides
/// along). Two triangle meshes:
///
/// - the two side faces and the end caps, mitered at every joint and facing outward, with
///   [`TriMeshFlags::FIX_INTERNAL_EDGES`]: a car scraping along meets one continuous face, with
///   no box corners at segment joints (per-segment boxes spun a scraping car at 100+ °/s);
/// - the top, 2 cm inside the side faces and 1 cm below their top edge, so a scraping body
///   never touches its edges. Sharing the top edge in one mesh let the internal-edge fix tilt
///   the contact normal up there and roll a scraping car onto the barrier.
///
/// Deck guard rails, sidewalk bare-back rails and median barriers. Empty for a degenerate line.
fn barrier_colliders(line: &[[f32; 3]], right: f32, half_w: f32, height: f32) -> Vec<Collider> {
    // Drop repeated points, then a unit direction per segment.
    let mut pts: Vec<[f32; 3]> = Vec::with_capacity(line.len());
    for &p in line {
        if pts
            .last()
            .is_none_or(|q: &[f32; 3]| len2(p[0] - q[0], p[1] - q[1]) >= 0.05)
        {
            pts.push(p);
        }
    }
    if pts.len() < 2 {
        return Vec::new();
    }
    let dirs: Vec<[f32; 2]> = pts
        .windows(2)
        .map(|w| {
            let (dx, dy) = (w[1][0] - w[0][0], w[1][1] - w[0][1]);
            let l = len2(dx, dy);
            [dx / l, dy / l]
        })
        .collect();
    let right_of = |d: [f32; 2]| [d[1], -d[0]];
    let n = pts.len();
    // Per point: the centre of the section and its mitered right normal, scaled so offsets
    // along it stay perpendicular distances from both neighbouring segments.
    let sections: Vec<([f32; 2], [f32; 2], f32)> = pts
        .iter()
        .enumerate()
        .map(|(k, p)| {
            let (ri, ro) = (
                right_of(dirs[k.saturating_sub(1)]),
                right_of(dirs[k.min(n - 2)]),
            );
            let m = [ri[0] + ro[0], ri[1] + ro[1]];
            let ml = len2(m[0], m[1]).max(1e-6);
            let m = [m[0] / ml, m[1] / ml];
            let scale = 1.0 / (m[0] * ri[0] + m[1] * ri[1]).max(0.35);
            let c = [p[0] + m[0] * right * scale, p[1] + m[1] * right * scale];
            (c, m, scale)
        })
        .collect();
    // 4 vertices per point (`inset` m inside the faces, top `drop` m lower): left foot, left
    // top, right top, right foot.
    let ring = |inset: f32, drop: f32| -> Vec<Vector> {
        let mut v = Vec::with_capacity(n * 4);
        for (p, (c, m, scale)) in pts.iter().zip(&sections) {
            let w = (half_w - inset) * scale;
            let (z0, z1) = (p[2] - BARRIER_FOOT_M, p[2] + height - drop);
            let (l, r) = (
                [c[0] - m[0] * w, c[1] - m[1] * w],
                [c[0] + m[0] * w, c[1] + m[1] * w],
            );
            v.push(Vector::new(l[0], l[1], z0));
            v.push(Vector::new(l[0], l[1], z1));
            v.push(Vector::new(r[0], r[1], z1));
            v.push(Vector::new(r[0], r[1], z0));
        }
        v
    };
    // The quad between ring edge `j → j + 1` at point k and k + 1, wound outward
    // (counter-clockwise seen from outside): j = 0 left face, 1 top, 2 right face.
    let quads = |j: u32| -> Vec<[u32; 3]> {
        (0..n - 1)
            .flat_map(|k| {
                let (a, b) = ((k * 4) as u32, (k * 4 + 4) as u32);
                let (j0, j1) = (j, (j + 1) % 4);
                [[a + j0, b + j1, b + j0], [a + j0, a + j1, b + j1]]
            })
            .collect()
    };
    let last = ((n - 1) * 4) as u32;
    let mut sides = quads(0);
    sides.extend(quads(2));
    sides.extend([
        [0, 2, 1],
        [0, 3, 2],
        [last, last + 1, last + 2],
        [last, last + 2, last + 3],
    ]);
    let mesh = |v: Vec<Vector>, idx: Vec<[u32; 3]>| match ColliderBuilder::trimesh_with_flags(
        v,
        idx,
        TriMeshFlags::FIX_INTERNAL_EDGES,
    ) {
        Ok(b) => Some(b.friction(BARRIER_FRICTION).restitution(0.0).build()),
        Err(e) => {
            eprintln!("barrier trimesh failed: {e:?}");
            None
        }
    };
    [
        mesh(ring(0.0, 0.0), sides),
        mesh(ring(0.02, 0.01), quads(1)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Sidewalk top, lot, skirt and kerb face colliders of `map`
/// ([`wp_world::surface::sidewalk_mesh`], the triangles the greybox draws) in [`sidewalk_groups`], and their triangle count. Every city
/// with sidewalks has them, flat ones too.
pub fn city_sidewalk_colliders(map: &wp_world::CityMap) -> (Vec<Collider>, usize) {
    sidewalk_mesh_colliders(&wp_world::surface::sidewalk_mesh(map))
}

/// [`city_sidewalk_colliders`] from an already built sidewalk mesh.
fn sidewalk_mesh_colliders(walks: &wp_world::surface::SidewalkMesh) -> (Vec<Collider>, usize) {
    let mesh = walks.all();
    let n = mesh.indices.len();
    (mesh_tile_colliders(&mesh, sidewalk_groups()), n)
}

/// `mesh` as static triangle-mesh colliders, one per 256 m cell (by triangle centroid, cells
/// in a fixed order), so a body only pairs with the surface around it.
fn mesh_tile_colliders(
    mesh: &wp_world::surface::TriMesh,
    groups: InteractionGroups,
) -> Vec<Collider> {
    const TILE_M: f32 = 256.0;
    let mut out = Vec::new();
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
            Ok(b) => out.push(b.collision_groups(groups).build()),
            Err(e) => eprintln!("city ground trimesh failed: {e:?}"),
        }
    }
    out
}

/// The flat slab as [`PhysicsWorld::set_city_ground`] parks it under a city with terrain (a
/// safety floor at [`SAFETY_FLOOR_Z`] in the terrain group). For client prediction rigs.
pub fn parked_slab_collider() -> Collider {
    let hz = wp_vehicle::rig::GROUND_HALF_EXTENTS[2];
    let mut c = wp_vehicle::rig::ground_collider();
    c.set_translation(Vector::new(0.0, 0.0, SAFETY_FLOOR_Z - hz));
    c.set_collision_groups(terrain_groups());
    c
}
