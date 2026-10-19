//! Outer convex hull of the deformed lattice for the chassis collider (parry's quickhull,
//! which uses only IEEE arithmetic and no randomness, so it is deterministic).

use rapier3d::parry::transformation::try_convex_hull;
use rapier3d::prelude::Vector;

use crate::vec3::V3;

/// Vertices of the convex hull of `points`. Falls back to returning the input unchanged for
/// degenerate sets (< 4 points or coplanar), which a convex-hull collider still accepts.
pub fn convex_hull_points(points: &[V3]) -> Vec<V3> {
    if points.len() < 4 {
        return points.to_vec();
    }
    let pts: Vec<Vector> = points
        .iter()
        .map(|p| Vector::new(p[0], p[1], p[2]))
        .collect();
    match try_convex_hull(&pts) {
        Ok((verts, _)) if verts.len() >= 4 => verts.iter().map(|v| [v.x, v.y, v.z]).collect(),
        _ => points.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_hull_drops_interior_point() {
        let mut pts = Vec::new();
        for x in [0.0, 1.0] {
            for y in [0.0, 1.0] {
                for z in [0.0, 1.0] {
                    pts.push([x, y, z]);
                }
            }
        }
        pts.push([0.5, 0.5, 0.5]);
        let hull = convex_hull_points(&pts);
        assert_eq!(hull.len(), 8);
        assert!(!hull.contains(&[0.5, 0.5, 0.5]));
    }
}
