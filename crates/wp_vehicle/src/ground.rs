//! Ground probing for wheels. The vehicle asks a [`WheelCaster`] for the ground under each
//! hub; `wp_sim` provides a Rapier-backed caster, tests use [`FlatGround`].

use rapier3d::prelude::{QueryPipeline, Ray};

use crate::math::V3;
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
