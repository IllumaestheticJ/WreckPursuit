//! Ground probing for wheels. The vehicle asks a [`WheelCaster`] for the ground under each
//! hub; `wp_sim` provides a Rapier-backed caster, tests use [`FlatGround`].

use rapier3d::parry::query::{ShapeCastOptions, ShapeCastStatus};
use rapier3d::parry::shape::Cylinder;
use rapier3d::prelude::{Pose, QueryPipeline, Ray};

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
pub struct RapierGround<'a> {
    /// The query pipeline.
    pub query: QueryPipeline<'a>,
}

impl WheelCaster for RapierGround<'_> {
    fn cast(&self, origin: V3, dir: V3, max_dist: f32) -> Option<GroundHit> {
        let ray = Ray::new(origin, dir);
        self.query
            .cast_ray_and_get_normal(&ray, max_dist, true)
            .map(|(_, hit)| GroundHit {
                distance: hit.time_of_impact,
                point: origin + dir * hit.time_of_impact,
                normal: hit.normal,
            })
    }

    fn cast_wheel(&self, p: &WheelProbe) -> Option<GroundHit> {
        let ray = centre_ray(self, p);
        let shape = Cylinder::new(p.half_width, p.radius);
        let options = ShapeCastOptions {
            max_time_of_impact: p.max_dist,
            target_distance: 0.0,
            stop_at_penetration: true,
            compute_impact_geometry_on_penetration: false,
        };
        let sweep =
            match self
                .query
                .cast_shape(&Pose::from_parts(p.center, p.rot), p.dir, &shape, options)
            {
                // Already touching at the start (a wall beside the tyre, or ground into the bump
                // stop): the sweep says nothing useful, keep the centre ray.
                Some((_, hit)) if hit.status == ShapeCastStatus::PenetratingOrWithinTargetDist => {
                    None
                }
                Some((_, hit)) => sweep_hit_on_tyre(p, hit.witness1, hit.normal1),
                None => None,
            };
        blend_envelope(ray, sweep)
    }
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
