//! Ground probing for wheels. The vehicle asks a [`WheelCaster`] for the ground under each
//! hub; `wp_sim` provides a Rapier-backed caster, tests use [`FlatGround`].

use rapier3d::parry::bounding_volume::BoundingVolume;
use rapier3d::parry::query::RayCast;
use rapier3d::parry::query::{ShapeCastOptions, ShapeCastStatus};
use rapier3d::parry::shape::{Cylinder, Shape, Triangle, TypedShape};
use rapier3d::prelude::{Pose, QueryFilter, QueryFilterFlags, QueryPipeline, Ray};

use crate::math::{Quat, V3};
use wp_contracts::GroundQuery;

/// Result of a ground probe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroundHit {
    /// Distance from the ray origin, m.
    pub distance: f32,
    /// Hit point, world space.
    pub point: V3,
    /// Unit surface normal, world space.
    pub normal: V3,
}

/// A wheel-shaped ground probe: the tyre as a cylinder swept from `center` along `dir`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WheelProbe {
    /// Wheel centre at the start of the sweep (top of the suspension travel), world space.
    pub center: V3,
    /// Unit sweep direction (chassis −Z).
    pub dir: V3,
    /// How far the wheel centre may travel, m.
    pub max_dist: f32,
    /// Tyre radius, m.
    pub radius: f32,
    /// Half the tyre width, m.
    pub half_width: f32,
    /// Orientation whose local +Y is the wheel's axle (Rapier cylinders are Y-aligned).
    pub rot: Quat,
}

/// Casts wheel probes into the world.
pub trait WheelCaster {
    /// Cast a ray from `origin` along unit `dir` up to `max_dist`.
    fn cast(&self, origin: V3, dir: V3, max_dist: f32) -> Option<GroundHit>;

    /// Sweep the tyre (see [`WheelProbe`]) and return the first touch, with `distance` = how
    /// far the **wheel centre** travelled (negative when the ground already reaches above the
    /// tyre's lowest point at the start, i.e. into the bump stop). The default is the centre
    /// ray of length `max_dist + radius` (a tyre that only touches straight below its hub);
    /// [`RapierGround`] sweeps a real cylinder, so a kerb edge ahead of the wheel lifts it
    /// over the tyre's approach distance instead of in one tick.
    fn cast_wheel(&self, probe: &WheelProbe) -> Option<GroundHit> {
        centre_ray(self, probe)
    }
}

fn centre_ray<C: WheelCaster + ?Sized>(caster: &C, p: &WheelProbe) -> Option<GroundHit> {
    caster
        .cast(p.center, p.dir, p.max_dist + p.radius)
        .map(|h| GroundHit {
            distance: h.distance - p.radius,
            ..h
        })
}

/// How much more the swept tyre is lifted than the centre ray (m) before the sweep takes
/// over; below it the centre ray (exact on smooth ground) is used, and between this and
/// twice it the two blend, so the choice never steps.
pub const ENVELOPE_BLEND_M: f32 = 0.0015;

/// Turn a sweep's witness point `w` (on the obstacle) into a wheel-centre travel. The sweep's
/// own time of impact is not used: against very large shapes (the 10 km ground slab) GJK is
/// only good to about a centimetre. Instead the centre travel is solved exactly for a tyre
/// circle passing through `w`. A witness that is not on the tyre's tread (beside it, or
/// further from the axle than the radius) is rejected.
pub fn sweep_hit_on_tyre(p: &WheelProbe, w: V3, normal: V3) -> Option<GroundHit> {
    let axle = p.rot * V3::Y;
    let rel = w - p.center;
    let along = rel.dot(p.dir);
    let side = rel.dot(axle);
    let a = (rel - p.dir * along - axle * side).length();
    if side.abs() > p.half_width + 0.02 || a >= p.radius {
        return None;
    }
    Some(GroundHit {
        distance: along - (p.radius * p.radius - a * a).sqrt(),
        point: w,
        normal,
    })
}

/// Envelope lift (sweep above the ray, m) beyond which the sweep is no longer trusted and
/// fades out over the next 5 cm: nothing a tyre rolls over lifts it this far before the hub
/// is above it, so it is a wall beside the tyre or another car's body overhead.
pub const ENVELOPE_MAX_LIFT_M: f32 = 0.2;

/// Combine the centre ray and the tyre sweep: the ray on smooth ground (exact and smooth),
/// the sweep where its envelope rises clearly above the ray (an edge ahead or behind the
/// contact), with a continuous blend between. Distances are wheel-centre travel.
pub fn blend_envelope(ray: Option<GroundHit>, sweep: Option<GroundHit>) -> Option<GroundHit> {
    match (ray, sweep) {
        (None, s) => s,
        (r, None) => r,
        (Some(r), Some(s)) => {
            let lift = r.distance - s.distance; // + = the sweep touches earlier (higher)
            let w = ((lift - ENVELOPE_BLEND_M) / ENVELOPE_BLEND_M).clamp(0.0, 1.0)
                * ((ENVELOPE_MAX_LIFT_M + 0.05 - lift) / 0.05).clamp(0.0, 1.0);
            if w <= 0.0 {
                Some(r)
            } else {
                Some(GroundHit {
                    distance: r.distance - lift.max(0.0) * w,
                    point: if w >= 0.5 { s.point } else { r.point },
                    normal: if w >= 0.5 { s.normal } else { r.normal },
                })
            }
        }
    }
}

/// Infinite horizontal plane at `height` (world z).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FlatGround {
    /// Plane height, m.
    pub height: f32,
}

impl WheelCaster for FlatGround {
    fn cast(&self, origin: V3, dir: V3, max_dist: f32) -> Option<GroundHit> {
        if dir.z >= -1e-6 {
            return None;
        }
        let t = (origin.z - self.height) / -dir.z;
        if !(0.0..=max_dist).contains(&t) {
            return None;
        }
        Some(GroundHit {
            distance: t,
            point: origin + dir * t,
            normal: V3::Z,
        })
    }
}

/// Rapier query-pipeline caster. Build the pipeline with a filter that excludes the vehicle's
/// own body (`QueryFilter::default().exclude_rigid_body(handle)`).
///
/// Probes see only the **static** world (fixed bodies and parentless colliders: roads, kerbs,
/// terrain, barriers, sleeping props); dynamic and kinematic bodies (other cars, debris, woken
/// props) are left to the chassis collider. A probe that starts inside another car's crushed
/// bodywork would otherwise read tens of centimetres of "ground" and launch the car
/// (playtest row 54: a torn-off wheel's hub probe in the Simulation T-bone gave 53 kN).
pub struct RapierGround<'a> {
    /// The query pipeline.
    pub query: QueryPipeline<'a>,
}

impl RapierGround<'_> {
    /// The pipeline restricted to the static world.
    fn static_only(&self) -> QueryPipeline<'_> {
        QueryPipeline {
            filter: QueryFilter {
                flags: self.query.filter.flags
                    | QueryFilterFlags::EXCLUDE_DYNAMIC
                    | QueryFilterFlags::EXCLUDE_KINEMATIC,
                ..self.query.filter
            },
            ..self.query
        }
    }
}

impl WheelCaster for RapierGround<'_> {
    fn cast(&self, origin: V3, dir: V3, max_dist: f32) -> Option<GroundHit> {
        let ray = Ray::new(origin, dir);
        self.static_only()
            .cast_ray_and_get_normal(&ray, max_dist, true)
            .map(|(_, hit)| GroundHit {
                distance: hit.time_of_impact,
                point: origin + dir * hit.time_of_impact,
                normal: hit.normal,
            })
    }

    fn cast_wheel(&self, p: &WheelProbe) -> Option<GroundHit> {
        let (ray, sweep) = self.probe(p);
        blend_envelope(ray, sweep)
    }
}

impl RapierGround<'_> {
    /// The tyre sweep of [`WheelCaster::cast_wheel`] alone (see [`RapierGround::probe`]).
    pub fn sweep_tyre(&self, p: &WheelProbe) -> Option<GroundHit> {
        self.probe(p).1
    }

    /// Both halves of [`WheelCaster::cast_wheel`] in one pass over the static colliders near
    /// the wheel: the centre ray (as [`WheelCaster::cast`] of length `max_dist + radius`,
    /// distance already minus the radius) and the tyre sweep (first touch of the swept tyre
    /// cylinder, as wheel-centre travel; `None` when nothing is touched within `max_dist` or
    /// the tyre already overlaps something at the start).
    ///
    /// The colliders whose bounding box meets the swept tyre's box are visited once. Triangle
    /// meshes and heightfields (roads, sidewalks, kerbs, terrain: everything a tyre rolls on
    /// in a city) are handled per triangle: the ray with parry's own triangle test (the same
    /// call parry's mesh and heightfield ray casts make, so the ray is unchanged) and the
    /// sweep in closed form ([`TyreFrame::sweep_triangle`]). Every other shape (the flat
    /// ground slab, barriers, buildings, rails) gets the same ray cast and GJK shape cast
    /// Rapier's query pipeline makes per collider, so flat-ground results are unchanged bit
    /// for bit. Rapier's cylinder-vs-triangle GJK cast was ~7 µs per wheel in Port Vesper
    /// (15× the centre ray) and the largest cost of the city sim step after round 12b; the
    /// closed form gives the same touch to within GJK's own error (`docs/integration/
    /// sim-perf.md`).
    pub fn probe(&self, p: &WheelProbe) -> (Option<GroundHit>, Option<GroundHit>) {
        let query = self.static_only();
        let shape = Cylinder::new(p.half_width, p.radius);
        let pose = Pose::from_parts(p.center, p.rot);
        let start = shape.compute_aabb(&pose);
        // Contains the ray too (from the centre, `max_dist + radius` along `dir`).
        let swept = start.merged(&start.translated(p.dir * p.max_dist));
        let tyre = TyreFrame::new(p);
        let ray = Ray::new(p.center, p.dir);
        let mut acc = ProbeAcc::new(p.max_dist + p.radius);
        for (_, co) in query.intersect_aabb_conservative(swept) {
            let co_pose = co.position();
            match co.shape().as_typed_shape() {
                TypedShape::TriMesh(mesh) => {
                    let local = LocalProbe::new(&tyre, &ray, co_pose);
                    let aabb = swept.transform_by(&co_pose.inverse());
                    for i in mesh.bvh().intersect_aabb(&aabb) {
                        acc.triangle(&local, &mesh.triangle(i));
                    }
                }
                TypedShape::HeightField(field) => {
                    let local = LocalProbe::new(&tyre, &ray, co_pose);
                    let aabb = swept.transform_by(&co_pose.inverse());
                    field
                        .map_elements_in_local_aabb(&aabb, &mut |_, tri| acc.triangle(&local, tri));
                }
                _ => {
                    // The same calls, with the same arguments, Rapier's query pipeline makes per
                    // collider (`cast_ray_and_get_normal`, `cast_shape`).
                    if let Some(h) =
                        co.shape()
                            .cast_ray_and_get_normal(co_pose, &ray, acc.ray_limit(), true)
                    {
                        acc.ray_hit(h.time_of_impact, h.normal);
                    }
                    if acc.void {
                        continue;
                    }
                    let options = ShapeCastOptions {
                        max_time_of_impact: p.max_dist,
                        target_distance: 0.0,
                        stop_at_penetration: true,
                        compute_impact_geometry_on_penetration: false,
                    };
                    let hit = query
                        .dispatcher
                        .cast_shapes(
                            &co_pose.inv_mul(&pose),
                            co_pose.rotation.inverse() * p.dir,
                            co.shape(),
                            &shape,
                            options,
                        )
                        .ok()
                        .flatten()
                        .map(|hit| hit.transform1_by(co_pose));
                    if let Some(hit) = hit {
                        if hit.status == ShapeCastStatus::PenetratingOrWithinTargetDist {
                            acc.void = true;
                        } else {
                            acc.sweep_hit(hit.time_of_impact, Best::Gjk(hit.witness1, hit.normal1));
                        }
                    }
                }
            }
        }
        let ray_hit = acc.ray.map(|(t, normal)| GroundHit {
            distance: t - p.radius,
            point: p.center + p.dir * t,
            normal,
        });
        let sweep = match acc.sweep {
            _ if acc.void => None,
            None => None,
            Some((_, Best::Exact(h))) => Some(h),
            Some((_, Best::Gjk(witness, normal))) => sweep_hit_on_tyre(p, witness, normal),
        };
        (ray_hit, sweep)
    }

    /// The tyre sweep as round 12b shipped it: one Rapier cylinder shape cast over the whole
    /// query pipeline. Kept as the reference [`RapierGround::sweep_tyre`] is tested and
    /// benchmarked against (`examples/probe_cost` in `wp_sim`).
    pub fn sweep_tyre_gjk(&self, p: &WheelProbe) -> Option<GroundHit> {
        let shape = Cylinder::new(p.half_width, p.radius);
        let options = ShapeCastOptions {
            max_time_of_impact: p.max_dist,
            target_distance: 0.0,
            stop_at_penetration: true,
            compute_impact_geometry_on_penetration: false,
        };
        let pose = Pose::from_parts(p.center, p.rot);
        match self.static_only().cast_shape(&pose, p.dir, &shape, options) {
            // Already touching at the start (a wall beside the tyre, or ground into the bump
            // stop): the sweep says nothing useful, keep the centre ray.
            Some((_, hit)) if hit.status == ShapeCastStatus::PenetratingOrWithinTargetDist => None,
            Some((_, hit)) => sweep_hit_on_tyre(p, hit.witness1, hit.normal1),
            None => None,
        }
    }
}

/// A sweep candidate (world space): an exact triangle touch, or a GJK witness point and
/// normal still to be converted with [`sweep_hit_on_tyre`].
enum Best {
    Exact(GroundHit),
    Gjk(V3, V3),
}

/// The probe's tyre and centre ray in one collider's local space, with that collider's pose.
struct LocalProbe<'a> {
    tyre: TyreFrame,
    ray: Ray,
    pose: &'a Pose,
}

impl<'a> LocalProbe<'a> {
    fn new(tyre: &TyreFrame, ray: &Ray, pose: &'a Pose) -> Self {
        Self {
            tyre: tyre.in_local_space(pose),
            ray: ray.inverse_transform_by(pose),
            pose,
        }
    }
}

/// What [`RapierGround::probe`] has found so far.
struct ProbeAcc {
    /// Ray length.
    ray_max: f32,
    /// Nearest ray hit: (time of impact, world normal).
    ray: Option<(f32, V3)>,
    /// Earliest sweep touch: (rank, hit). Exact touches rank by centre travel, GJK ones by
    /// their time of impact (as the composite shape cast ranked them).
    sweep: Option<(f32, Best)>,
    /// The tyre overlaps something at the start: the sweep has no result.
    void: bool,
}

impl ProbeAcc {
    fn new(ray_max: f32) -> Self {
        Self {
            ray_max,
            ray: None,
            sweep: None,
            void: false,
        }
    }

    /// How far a ray hit must be to beat the current one.
    fn ray_limit(&self) -> f32 {
        self.ray.map_or(self.ray_max, |(t, _)| t)
    }

    fn ray_hit(&mut self, t: f32, normal: V3) {
        if self.ray.is_none_or(|(best, _)| t < best) {
            self.ray = Some((t, normal));
        }
    }

    fn sweep_hit(&mut self, rank: f32, hit: Best) {
        if self.sweep.as_ref().is_none_or(|(best, _)| rank < *best) {
            self.sweep = Some((rank, hit));
        }
    }

    /// One triangle of a mesh or heightfield (collider-local): the ray with parry's own
    /// triangle test, then the closed-form tyre sweep.
    fn triangle(&mut self, local: &LocalProbe, tri: &Triangle) {
        if let Some(h) = tri.cast_local_ray_and_get_normal(&local.ray, self.ray_limit(), true) {
            self.ray_hit(h.time_of_impact, local.pose.rotation * h.normal);
        }
        if self.void {
            return;
        }
        match local.tyre.sweep_triangle([tri.a, tri.b, tri.c]) {
            TriSweep::Miss => {}
            TriSweep::Overlap => self.void = true,
            TriSweep::Hit(h) => self.sweep_hit(
                h.distance,
                Best::Exact(GroundHit {
                    distance: h.distance,
                    point: local.pose.transform_point(h.point),
                    normal: local.pose.transform_vector(h.normal),
                }),
            ),
        }
    }
}

/// What sweeping the tyre down onto one triangle found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TriSweep {
    /// The tyre does not touch the triangle within its travel (or the triangle lies wholly
    /// above the starting tyre).
    Miss,
    /// The starting tyre already overlaps the triangle.
    Overlap,
    /// First touch: `distance` = wheel-centre travel, `point` on the triangle, `normal` the
    /// obstacle's normal there (the face normal for a touch inside the face, the tyre's radial
    /// direction for a touch on an edge or corner, as GJK reports them).
    Hit(GroundHit),
}

/// The probe's tyre as an orthonormal frame: centre `c`, sweep direction `d`, axle `a`, and
/// `f = d × a` (in the wheel plane, across the sweep).
#[derive(Debug, Clone, Copy)]
pub struct TyreFrame {
    c: V3,
    d: V3,
    a: V3,
    f: V3,
    r: f32,
    hw: f32,
    max: f32,
}

/// Polygon vertex in tyre coordinates `[side, across, along]` and whether the edge leaving
/// it lies on the original triangle's boundary (`true`) or on a clip plane.
type ClipVertex = ([f32; 3], bool);

impl TyreFrame {
    /// The frame of `p` (world space).
    pub fn new(p: &WheelProbe) -> Self {
        let a = p.rot * V3::Y;
        Self {
            c: p.center,
            d: p.dir,
            a,
            f: p.dir.cross(a),
            r: p.radius,
            hw: p.half_width,
            max: p.max_dist,
        }
    }

    /// The same frame expressed in a collider's local space.
    fn in_local_space(&self, pose: &Pose) -> Self {
        Self {
            c: pose.inverse_transform_point(self.c),
            d: pose.inverse_transform_vector(self.d),
            a: pose.inverse_transform_vector(self.a),
            f: pose.inverse_transform_vector(self.f),
            ..*self
        }
    }

    /// Sweep the tyre (a cylinder of radius `r`, half-width `hw` about axle `a`, moving along
    /// `d` from `c` up to `max`) onto triangle `tri` (same space as the frame), in closed form.
    ///
    /// A point `P` with side `s = (P − c)·a`, across `u = (P − c)·f` and along
    /// `l = (P − c)·d` is inside the tyre at travel `t` when `|s| ≤ hw` and
    /// `u² + (l − t)² ≤ r²`; the swept tyre first reaches it at `t = l − √(r² − u²)` and
    /// leaves it at `l + √(r² − u²)`. The touch on a triangle is the minimum of the first over
    /// the triangle clipped to `|s| ≤ hw`, `|u| ≤ r`. That function is convex and depends on
    /// the point only through `l` and `u`, so its minimum over the convex polygon lies on the
    /// polygon's boundary: each edge is a 1-D convex minimisation with a closed-form
    /// stationary point. The tyre overlaps the triangle at the start when some point has
    /// first ≤ 0 ≤ last (the triangle is connected, so `min first ≤ 0 ≤ max last` suffices).
    pub fn sweep_triangle(&self, tri: [V3; 3]) -> TriSweep {
        let (r, hw) = (self.r, self.hw);
        let mut poly: [ClipVertex; 8] = [([0.0; 3], true); 8];
        let mut n = 0;
        for v in tri {
            let rel = v - self.c;
            poly[n] = ([rel.dot(self.a), rel.dot(self.f), rel.dot(self.d)], true);
            n += 1;
        }
        // Cheap rejections: beside the tread, ahead / behind the tyre, below its reach, or
        // wholly above the starting tyre.
        let all = |k: usize, test: &dyn Fn(f32) -> bool| poly[..3].iter().all(|v| test(v.0[k]));
        if all(0, &|x| x > hw)
            || all(0, &|x| x < -hw)
            || all(1, &|x| x > r)
            || all(1, &|x| x < -r)
            || all(2, &|x| x - r > self.max)
            || all(2, &|x| x + r < 0.0)
        {
            return TriSweep::Miss;
        }
        for (k, bound) in [(0, hw), (0, -hw), (1, r), (1, -r)] {
            n = clip(&mut poly, n, k, bound);
            if n == 0 {
                return TriSweep::Miss;
            }
        }
        // Minimum of the first-touch travel over the boundary.
        let r2 = r * r;
        let first = |q: [f32; 3]| q[2] - (r2 - q[1] * q[1]).max(0.0).sqrt();
        let mut best = (f32::MAX, [0.0f32; 3], false);
        for i in 0..n {
            let (q0, on_edge) = poly[i];
            let q1 = poly[(i + 1) % n].0;
            let q = along_edge(q0, q1, r, -1.0);
            let t = first(q);
            if t < best.0 {
                best = (t, q, on_edge);
            }
        }
        let (t, q, on_edge) = best;
        if t > self.max {
            return TriSweep::Miss;
        }
        if t <= 0.0 {
            // Overlapping at the start, or wholly behind (above) it.
            let last = |q: [f32; 3]| q[2] + (r2 - q[1] * q[1]).max(0.0).sqrt();
            let overlaps = (0..n).any(|i| {
                let q = along_edge(poly[i].0, poly[(i + 1) % n].0, r, 1.0);
                last(q) >= 0.0
            });
            return if overlaps {
                TriSweep::Overlap
            } else {
                TriSweep::Miss
            };
        }
        let point = self.c + self.a * q[0] + self.f * q[1] + self.d * q[2];
        // Tyre centre at the touch, and the radial direction from the touch towards it.
        let centre = self.c + self.d * t;
        let radial = {
            let v = centre - point;
            (v - self.a * v.dot(self.a)).normalize_or_zero()
        };
        let face = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize_or_zero();
        // A touch inside the face (on a clip edge: the tyre's tread or its rim edge meets
        // the face) has the face normal, turned towards the tyre (its centre is on the tyre's
        // side of the face plane); a touch on the triangle's own edge or corner the radial.
        let normal = if on_edge || face == V3::ZERO {
            radial
        } else if face.dot(centre - point) < 0.0 {
            -face
        } else {
            face
        };
        TriSweep::Hit(GroundHit {
            distance: t,
            point,
            normal,
        })
    }
}

/// Sutherland–Hodgman: clip the convex polygon `poly[..n]` to coordinate `k` ≤ `bound`
/// (`bound` > 0) or ≥ `bound` (`bound` < 0), in place; returns the new vertex count. A new
/// vertex on the clip plane starts a clip edge; the other edges keep their flag.
fn clip(poly: &mut [ClipVertex; 8], n: usize, k: usize, bound: f32) -> usize {
    let inside = |q: &[f32; 3]| {
        if bound > 0.0 {
            q[k] <= bound
        } else {
            q[k] >= bound
        }
    };
    let src = *poly;
    let mut m = 0;
    for i in 0..n {
        let (cur, flag) = src[i];
        let next = src[(i + 1) % n].0;
        let (ci, ni) = (inside(&cur), inside(&next));
        if ci {
            poly[m] = (cur, flag);
            m += 1;
        }
        if ci != ni && m < poly.len() {
            let w = (bound - cur[k]) / (next[k] - cur[k]);
            let mut x = [0.0f32; 3];
            for j in 0..3 {
                x[j] = cur[j] + (next[j] - cur[j]) * w;
            }
            x[k] = bound;
            // Leaving: the edge from the new vertex runs along the clip plane; entering: it
            // continues the original edge.
            poly[m] = (x, ni);
            if ci {
                poly[m].1 = false;
            } else {
                poly[m].1 = flag;
            }
            m += 1;
        }
    }
    m
}

/// The point of segment `q0 → q1` (tyre coordinates, `|across| ≤ r`) minimising
/// `along − √(r² − across²)` (`sign` = −1: the tyre's first touch) or maximising
/// `along + √(r² − across²)` (`sign` = +1: its last). Both are convex along the segment; the
/// stationary point solves `across / √(r² − across²) = −sign · Δalong / Δacross`.
fn along_edge(q0: [f32; 3], q1: [f32; 3], r: f32, sign: f32) -> [f32; 3] {
    let (du, dl) = (q1[1] - q0[1], q1[2] - q0[2]);
    let w = if du.abs() < 1e-7 {
        // `across` constant: `along` alone decides (lowest for the first touch).
        if (dl >= 0.0) == (sign < 0.0) {
            0.0
        } else {
            1.0
        }
    } else {
        let k = sign * dl / du;
        let u = k * r / (1.0 + k * k).sqrt();
        ((u - q0[1]) / du).clamp(0.0, 1.0)
    };
    [q0[0] + (q1[0] - q0[0]) * w, q0[1] + du * w, q0[2] + dl * w]
}

fn arr(v: V3) -> [f32; 3] {
    [v.x, v.y, v.z]
}

fn to_contract(h: GroundHit) -> wp_contracts::GroundHit {
    wp_contracts::GroundHit {
        dist_m: h.distance,
        point: arr(h.point),
        normal: arr(h.normal),
    }
}

impl GroundQuery for FlatGround {
    fn cast_ray(
        &self,
        origin: [f32; 3],
        dir: [f32; 3],
        max_m: f32,
    ) -> Option<wp_contracts::GroundHit> {
        self.cast(V3::from_array(origin), V3::from_array(dir), max_m)
            .map(to_contract)
    }
}

impl GroundQuery for RapierGround<'_> {
    fn cast_ray(
        &self,
        origin: [f32; 3],
        dir: [f32; 3],
        max_m: f32,
    ) -> Option<wp_contracts::GroundHit> {
        self.cast(V3::from_array(origin), V3::from_array(dir), max_m)
            .map(to_contract)
    }
}

/// Adapter: any contract [`GroundQuery`] used as this crate's [`WheelCaster`] (lead, v0.2.0).
pub struct ContractGround<'a>(pub &'a dyn GroundQuery);

impl WheelCaster for ContractGround<'_> {
    fn cast(&self, origin: V3, dir: V3, max_dist: f32) -> Option<GroundHit> {
        self.0
            .cast_ray(arr(origin), arr(dir), max_dist)
            .map(|h| GroundHit {
                distance: h.dist_m,
                point: V3::from_array(h.point),
                normal: V3::from_array(h.normal),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_blend_is_continuous() {
        let hit = |d: f32| GroundHit {
            distance: d,
            point: V3::ZERO,
            normal: V3::Z,
        };
        let r = Some(hit(0.10));
        // Within the blend threshold the ray wins exactly.
        assert_eq!(blend_envelope(r, Some(hit(0.0995))).unwrap().distance, 0.10);
        // Far above it the sweep wins exactly.
        assert!((blend_envelope(r, Some(hit(0.05))).unwrap().distance - 0.05).abs() < 1e-6);
        // No step anywhere in between.
        let mut prev = 0.10;
        for k in 0..400 {
            let s = 0.10 - k as f32 * 0.00001;
            let d = blend_envelope(r, Some(hit(s))).unwrap().distance;
            assert!((d - prev).abs() < 0.0001, "{s}: {d} vs {prev}");
            prev = d;
        }
        assert_eq!(blend_envelope(None, Some(hit(0.2))).unwrap().distance, 0.2);
        // A "kerb" far higher than anything a tyre climbs: not trusted.
        assert_eq!(blend_envelope(r, Some(hit(-0.3))).unwrap().distance, 0.10);
    }

    /// A tyre like `Vehicle::probe_wheel` builds: 0.32 m radius, 0.08 m half-width, 0.46 m
    /// of sweep, axle along `yaw`, tilted by `roll` / `pitch` (rad).
    fn probe_at(center: V3, yaw: f32, roll: f32, pitch: f32) -> WheelProbe {
        let rot = Quat::from_rotation_z(yaw)
            * Quat::from_rotation_y(roll)
            * Quat::from_rotation_x(pitch)
            * Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2);
        WheelProbe {
            center,
            dir: rot * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2) * -V3::Z,
            max_dist: 0.46,
            radius: 0.32,
            half_width: 0.08,
            rot,
        }
    }

    /// The closed-form tyre sweep against parry's GJK cylinder cast, triangle by triangle:
    /// random tyres (tilted up to ±0.3 rad) over random triangles around them (flat, sloped,
    /// kerb faces, slivers, triangles wider than the tyre). Same verdict (miss / overlap /
    /// hit) and the same touch to within GJK's tolerance; the few disagreements are touches
    /// within a millimetre of a boundary (start of the sweep, end of the reach, tread edge).
    #[test]
    fn triangle_sweep_matches_gjk() {
        let mut rng = wp_math::Rng::new(12);
        let (mut hits, mut overlaps, mut misses, mut borderline, mut gjk_off) = (0, 0, 0, 0, 0);
        let mut max_err = 0.0f32;
        for _ in 0..4000 {
            let p = probe_at(
                V3::new(0.0, 0.0, 0.8),
                rng.range_f32(-3.2, 3.2),
                rng.range_f32(-0.3, 0.3),
                rng.range_f32(-0.3, 0.3),
            );
            let mut v = || {
                V3::new(
                    rng.range_f32(-0.6, 0.6),
                    rng.range_f32(-0.6, 0.6),
                    rng.range_f32(-0.3, 0.7),
                )
            };
            let tri = [v(), v(), v()];
            let exact = TyreFrame::new(&p).sweep_triangle(tri);
            let shape = Cylinder::new(p.half_width, p.radius);
            let gjk = rapier3d::parry::query::cast_shapes(
                &Pose::IDENTITY,
                V3::ZERO,
                &Triangle::new(tri[0], tri[1], tri[2]),
                &Pose::from_parts(p.center, p.rot),
                p.dir,
                &shape,
                ShapeCastOptions {
                    max_time_of_impact: p.max_dist,
                    target_distance: 0.0,
                    stop_at_penetration: true,
                    compute_impact_geometry_on_penetration: false,
                },
            )
            .unwrap();
            match (exact, gjk) {
                (TriSweep::Hit(h), Some(g))
                    if g.status != ShapeCastStatus::PenetratingOrWithinTargetDist =>
                {
                    let err = (h.distance - g.time_of_impact).abs();
                    if err > 0.0005 {
                        // GJK's tolerance on thin / steep triangles: settle it by sampling.
                        let brute = brute_force_touch(&p, tri);
                        assert!(
                            (h.distance - brute).abs() < 0.0002 && h.distance <= brute + 1e-5,
                            "{tri:?}: exact {} vs brute force {brute} (GJK {})",
                            h.distance,
                            g.time_of_impact
                        );
                        gjk_off += 1;
                    } else {
                        max_err = max_err.max(err);
                    }
                    // The touch point is on the triangle and on the tyre at that travel.
                    let rel = h.point - (p.center + p.dir * h.distance);
                    let axle = p.rot * V3::Y;
                    let radial = (rel - axle * rel.dot(axle)).length();
                    assert!(
                        (radial - p.radius).abs() < 1e-4
                            && rel.dot(axle).abs() <= p.half_width + 1e-4
                    );
                    assert!(
                        h.normal.dot(-rel) >= 0.0,
                        "normal faces the tyre: {h:?} {tri:?}"
                    );
                    hits += 1;
                }
                (TriSweep::Overlap, Some(g))
                    if g.status == ShapeCastStatus::PenetratingOrWithinTargetDist =>
                {
                    overlaps += 1
                }
                (TriSweep::Miss, None) => misses += 1,
                (e, g) => {
                    // Only touches right at a boundary may disagree.
                    let t = match e {
                        TriSweep::Hit(h) => h.distance,
                        _ => g.map_or(0.0, |g| g.time_of_impact),
                    };
                    assert!(
                        t < 0.002 || t > p.max_dist - 0.002 || borderline < 40,
                        "{tri:?}: exact {e:?} vs GJK {g:?}"
                    );
                    borderline += 1;
                }
            }
        }
        assert!(
            hits > 400 && overlaps > 200 && misses > 400,
            "{hits} {overlaps} {misses}"
        );
        assert!(borderline <= 40, "{borderline} borderline cases");
        assert!(
            gjk_off <= hits / 50,
            "{gjk_off} of {hits} hits off GJK by > 0.5 mm"
        );
        assert!(max_err < 0.0005);
    }

    /// First touch of the swept tyre on `tri`, independently of the closed form: slice the
    /// tyre at 801 side offsets (both tread edges included), cut the triangle with each slice
    /// plane and sample the cut segment at 2001 points. An upper bound on the exact touch
    /// that converges to it.
    fn brute_force_touch(p: &WheelProbe, tri: [V3; 3]) -> f32 {
        let axle = p.rot * V3::Y;
        let across = p.dir.cross(axle);
        let side: Vec<f32> = tri.iter().map(|v| (*v - p.center).dot(axle)).collect();
        let mut best = f32::MAX;
        for k in 0..=800 {
            let s = -p.half_width + 2.0 * p.half_width * k as f32 / 800.0;
            // Where the triangle's edges cross the slice plane.
            let mut cut: Vec<V3> = Vec::new();
            for e in 0..3 {
                let (i, j) = (e, (e + 1) % 3);
                let (si, sj) = (side[i], side[j]);
                if (si - s) * (sj - s) <= 0.0 && si != sj {
                    let w = (s - si) / (sj - si);
                    cut.push(tri[i] + (tri[j] - tri[i]) * w);
                }
            }
            if cut.len() < 2 {
                continue;
            }
            for m in 0..=2000 {
                let q = cut[0] + (cut[1] - cut[0]) * (m as f32 / 2000.0);
                let rel = q - p.center;
                let (u, l) = (rel.dot(across), rel.dot(p.dir));
                if u.abs() < p.radius {
                    best = best.min(l - (p.radius * p.radius - u * u).sqrt());
                }
            }
        }
        best
    }

    /// Flat ground under a level tyre: the sweep touches exactly where the centre ray does
    /// (the envelope keeps the ray), and a tilted tyre touches with its low tread edge first.
    #[test]
    fn triangle_sweep_on_flat_ground() {
        let big = [
            V3::new(-50.0, -50.0, 0.0),
            V3::new(50.0, -50.0, 0.0),
            V3::new(0.0, 80.0, 0.0),
        ];
        let p = probe_at(V3::new(0.3, 0.2, 0.7), 0.4, 0.0, 0.0);
        let TriSweep::Hit(h) = TyreFrame::new(&p).sweep_triangle(big) else {
            panic!("no hit");
        };
        assert!((h.distance - (0.7 - 0.32)).abs() < 1e-5, "{h:?}");
        assert!((h.normal - V3::Z).length() < 1e-5);
        // 0.05 rad of roll: the low tread edge is half_width × sin(roll) lower.
        let p = probe_at(V3::new(0.3, 0.2, 0.7), 0.4, 0.05, 0.0);
        let TriSweep::Hit(h) = TyreFrame::new(&p).sweep_triangle(big) else {
            panic!("no hit");
        };
        let axle = p.rot * V3::Y;
        let c = p.center + p.dir * h.distance;
        assert!((h.point.z).abs() < 1e-5);
        assert!(
            ((h.point - c).dot(axle).abs() - p.half_width).abs() < 1e-4,
            "edge touch"
        );
        // A triangle wholly above the tyre is not hit; one cutting through it overlaps.
        let above = big.map(|v| v + V3::Z * 2.0);
        assert_eq!(TyreFrame::new(&p).sweep_triangle(above), TriSweep::Miss);
        let through = big.map(|v| v + V3::Z * 0.6);
        assert_eq!(
            TyreFrame::new(&p).sweep_triangle(through),
            TriSweep::Overlap
        );
    }

    #[test]
    fn flat_ground_hits() {
        let g = FlatGround { height: 0.0 };
        let h = g
            .cast(V3::new(1.0, 2.0, 0.5), V3::new(0.0, 0.0, -1.0), 1.0)
            .unwrap();
        assert!((h.distance - 0.5).abs() < 1e-6);
        assert!(g
            .cast(V3::new(0.0, 0.0, 2.0), V3::new(0.0, 0.0, -1.0), 1.0)
            .is_none());
    }
}
