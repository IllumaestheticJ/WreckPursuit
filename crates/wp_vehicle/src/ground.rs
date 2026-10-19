//! Ground probing for wheels. The vehicle asks a [`WheelCaster`] for the ground under each
//! hub; `wp_sim` provides a Rapier-backed caster, tests use [`FlatGround`].

use rapier3d::prelude::{QueryPipeline, Ray};

use crate::math::V3;

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

/// Casts wheel probes into the world.
pub trait WheelCaster {
    /// Cast a ray from `origin` along unit `dir` up to `max_dist`.
    fn cast(&self, origin: V3, dir: V3, max_dist: f32) -> Option<GroundHit>;
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
