//! Outer convex hull of the deformed lattice for the chassis collider (parry's quickhull,
//! which uses only IEEE arithmetic and no randomness, so it is deterministic).

use rapier3d::parry::transformation::try_convex_hull;
use rapier3d::prelude::{MassProperties, Vector};

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

/// Mass properties of a solid box of `mass` kg with `half_extents` (m) centred on `center`
/// (collider frame), axes aligned with that frame. Give these to a dynamic convex-hull collider
/// instead of letting Rapier derive them from the hull: a hull's inertia tensor is not
/// diagonal, and Rapier finds its principal axes with `glamx`'s `SymmetricEigen3`, which calls
/// `f32::acos` and `f32::cos`. Those come from the platform's libm and differ in the last bit
/// between native and wasm32, so a dented parked car or a torn-off panel moved differently
/// in the browser (`pursuit_jam_grid` native vs wasm). A diagonal inertia skips that path.
pub fn box_mass_properties(center: [f32; 3], half_extents: [f32; 3], mass: f32) -> MassProperties {
    let [hx, hy, hz] = half_extents.map(|h| h * h);
    let k = mass / 3.0;
    MassProperties::new(
        Vector::new(center[0], center[1], center[2]),
        mass,
        Vector::new(k * (hy + hz), k * (hx + hz), k * (hx + hy)),
    )
}

/// [`box_mass_properties`] of the axis-aligned bounding box of `points` (collider frame).
pub fn bounding_box_mass_properties(points: &[V3], mass: f32) -> MassProperties {
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    for p in points {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    if points.is_empty() {
        (lo, hi) = ([0.0; 3], [0.0; 3]);
    }
    let center = [0, 1, 2].map(|k| 0.5 * (lo[k] + hi[k]));
    let half = [0, 1, 2].map(|k| 0.5 * (hi[k] - lo[k]));
    box_mass_properties(center, half, mass)
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

    /// The box mass properties are diagonal in the collider frame (no principal-axis rotation,
    /// so no eigen-decomposition and no libm trig), and match a solid box's inertia.
    #[test]
    fn box_mass_properties_are_axis_aligned() {
        let mp = box_mass_properties([0.1, -0.2, 0.3], [1.0, 2.0, 0.5], 1200.0);
        assert_eq!(mp.mass(), 1200.0);
        assert_eq!(mp.local_com, Vector::new(0.1, -0.2, 0.3));
        assert_eq!(
            mp.principal_inertia_local_frame,
            rapier3d::prelude::Rotation::IDENTITY
        );
        let i = mp.principal_inertia();
        assert!((i.x - 400.0 * (4.0 + 0.25)).abs() < 1e-2, "{i:?}");
        assert!((i.y - 400.0 * (1.0 + 0.25)).abs() < 1e-2, "{i:?}");
        assert!((i.z - 400.0 * (1.0 + 4.0)).abs() < 1e-2, "{i:?}");
    }

    #[test]
    fn bounding_box_mass_properties_use_the_points_extent() {
        let pts = [
            [-1.0, 0.0, 0.0],
            [1.0, 4.0, 0.0],
            [0.0, 2.0, 1.0],
            [0.5, 1.0, 0.2],
        ];
        let mp = bounding_box_mass_properties(&pts, 10.0);
        assert_eq!(mp.local_com, Vector::new(0.0, 2.0, 0.5));
        assert_eq!(
            mp.principal_inertia_local_frame,
            rapier3d::prelude::Rotation::IDENTITY
        );
        let want = box_mass_properties([0.0, 2.0, 0.5], [1.0, 2.0, 0.5], 10.0);
        assert_eq!(mp.principal_inertia(), want.principal_inertia());
    }
}
