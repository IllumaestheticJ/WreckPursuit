//! Drivable surface geometry shared by physics (`wp_sim` colliders), the greybox exporter and
//! `wp_world`'s own physical traffic:
//! - [`road_mesh`]: one triangle mesh of every paved surface — road ribbons (banked), junction
//!   discs and the connector ribbons through bends;
//! - [`deck_rails`]: guard-rail lines along bridge decks;
//! - [`deck_slabs`]: bridge-deck pieces that hide what is under them (line of sight);
//! - [`MapGround`]: a wheel ground probe over roads and terrain without a physics engine.

use wp_vehicle::{GroundHit, WheelCaster};

use crate::geom::{self, V2, V3};
use crate::map::{CityMap, SegKind};
use crate::network::{RoadNetwork, RoadSurface};

/// Indexed triangle mesh, world space (+Z up), counter-clockwise seen from above.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TriMesh {
    /// Vertex positions, m.
    pub positions: Vec<V3>,
    /// Triangles.
    pub indices: Vec<[u32; 3]>,
}

impl TriMesh {
    fn quad(&mut self, a: V3, b: V3, c: V3, d: V3) {
        let i = self.positions.len() as u32;
        self.positions.extend_from_slice(&[a, b, c, d]);
        self.indices.push([i, i + 1, i + 2]);
        self.indices.push([i, i + 2, i + 3]);
    }

    /// Append a ribbon: `left[k]`–`right[k]` cross sections along a strip.
    pub fn strip(&mut self, left: &[V3], right: &[V3]) {
        for k in 0..left.len().saturating_sub(1) {
            // Travel along the strip, right edge first, so triangles face up.
            self.quad(right[k], right[k + 1], left[k + 1], left[k]);
        }
    }

    /// Append a flat convex polygon (fan) at its own vertex heights.
    pub fn fan(&mut self, poly: &[V3]) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(poly);
        for k in 1..poly.len().saturating_sub(1) as u32 {
            self.indices.push([base, base + k, base + k + 1]);
        }
    }
}

/// Left and right paved edges of a surface (mitred like the lanes; banked heights).
pub fn surface_edges(s: &RoadSurface) -> (Vec<V3>, Vec<V3>) {
    let mut left = geom::offset_polyline(&s.centre, -s.left_m);
    let mut right = geom::offset_polyline(&s.centre, s.right_m);
    for k in 0..s.centre.len() {
        left[k][2] = s.z_at(k, -s.left_m);
        right[k][2] = s.z_at(k, s.right_m);
    }
    (left, right)
}

/// Every paved surface of a city as one mesh: road ribbons, junction discs (16-gons) and the
/// connector ribbons through bend nodes (degree-2 nodes have no junction disc).
pub fn road_mesh(map: &CityMap) -> TriMesh {
    let net = &map.net;
    let mut m = TriMesh::default();
    for s in &net.surfaces {
        let (l, r) = surface_edges(s);
        m.strip(&l, &r);
    }
    for ix in &net.intersections {
        let c = net.nodes[ix.node as usize].pos;
        let poly: Vec<V3> = (0..16)
            .map(|k| {
                let (sn, cs) = wp_math::sin_cos(k as f32 / 16.0 * std::f32::consts::TAU);
                [c[0] + cs * ix.radius_m, c[1] + sn * ix.radius_m, c[2]]
            })
            .collect();
        m.fan(&poly);
    }
    for s in &map.segs {
        if let SegKind::Connector { node, .. } = s.kind {
            if net.nodes[node as usize].intersection.is_none() {
                let half = s.width * 0.5;
                let l = geom::offset_polyline(&s.pts, -half);
                let r = geom::offset_polyline(&s.pts, half);
                m.strip(&l, &r);
            }
        }
    }
    m
}

/// Whether the whole cross-section of `s` at point `k` is more than `gap` metres above
/// `ground` (a bridge deck rather than an embankment).
pub fn is_elevated(s: &RoadSurface, k: usize, ground: &dyn Fn(V2) -> f32, gap: f32) -> bool {
    let n = s.centre.len();
    let p = s.centre[k];
    let tan = if k + 1 < n {
        geom::normalize(geom::sub(geom::xy(s.centre[k + 1]), geom::xy(p)))
    } else {
        geom::normalize(geom::sub(geom::xy(p), geom::xy(s.centre[k - 1])))
    };
    let r = geom::right_of(tan);
    [-s.left_m, 0.0, s.right_m].iter().all(|lat| {
        let q = geom::add(geom::xy(p), geom::scale(r, *lat));
        s.z_at(k, *lat) - ground(q) > gap
    })
}

/// Guard-rail lines `(a, b)` (at road height) along both edges of every bridge-deck stretch:
/// surface segments whose whole cross-section is more than 1.5 m above the terrain. Empty
/// without terrain.
pub fn deck_rails(net: &RoadNetwork) -> Vec<(V3, V3)> {
    let Some(t) = &net.terrain else {
        return Vec::new();
    };
    let ground = |q: V2| t.height_at(q);
    let mut out = Vec::new();
    for s in &net.surfaces {
        let (l, r) = surface_edges(s);
        for k in 0..s.centre.len() - 1 {
            if is_elevated(s, k, &ground, 1.5) && is_elevated(s, k + 1, &ground, 1.5) {
                out.push((l[k], l[k + 1]));
                out.push((r[k], r[k + 1]));
            }
        }
    }
    out
}

/// Clearance under a deck piece before it counts as cover for line of sight, m (a car fits).
pub const DECK_COVER_CLEARANCE_M: f32 = 3.0;

/// One piece of bridge deck between two surface centreline points: the paved strip from
/// `left_m` left to `right_m` right of the centreline `a`→`b`, treated as a thin plane (bank
/// ignored). Used by [`CityMap::line_of_sight`] so overpasses hide the road below them from a
/// helicopter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeckSlab {
    /// Centreline start (road height), m.
    pub a: V3,
    /// Centreline end, m.
    pub b: V3,
    /// Paved width left of the centreline, m.
    pub left_m: f32,
    /// Paved width right of the centreline, m.
    pub right_m: f32,
}

impl DeckSlab {
    /// Whether the 3D segment `p`→`q` passes through the slab. The slab is the plane through
    /// the centreline that is level across the road; along the segment both the slab's height
    /// under it and its own height are linear in the segment parameter, so there is at most one
    /// crossing, found in closed form.
    pub fn blocks(&self, p: V3, q: V3) -> bool {
        let e = geom::sub(geom::xy(self.b), geom::xy(self.a));
        let e2 = geom::dot(e, e);
        if e2 < 1e-6 {
            return false;
        }
        let (pp, d) = (geom::xy(p), geom::sub(geom::xy(q), geom::xy(p)));
        // Along-slab parameter u(t) = u0 + du·t; slab height z0 + dz·u(t); segment p.z + sz·t.
        let u0 = geom::dot(geom::sub(pp, geom::xy(self.a)), e) / e2;
        let du = geom::dot(d, e) / e2;
        let dz = self.b[2] - self.a[2];
        let den = (q[2] - p[2]) - dz * du;
        if den.abs() < 1e-6 {
            return false; // parallel to the deck plane
        }
        let t = (self.a[2] + dz * u0 - p[2]) / den;
        if !(0.0..=1.0).contains(&t) {
            return false;
        }
        let u = u0 + du * t;
        if !(0.0..=1.0).contains(&u) {
            return false;
        }
        let at = geom::add(pp, geom::scale(d, t));
        let rel = geom::sub(at, geom::xy(self.a));
        // Signed offset, + right of the travel direction.
        let lat = geom::dot(rel, geom::right_of(e)) / e2.sqrt();
        lat >= -self.left_m && lat <= self.right_m
    }
}

/// Bridge-deck pieces with at least [`DECK_COVER_CLEARANCE_M`] under their whole
/// cross-section at both ends. Empty without terrain.
pub fn deck_slabs(net: &RoadNetwork) -> Vec<DeckSlab> {
    let Some(t) = &net.terrain else {
        return Vec::new();
    };
    let ground = |q: V2| t.height_at(q);
    let gap = DECK_COVER_CLEARANCE_M;
    let mut out = Vec::new();
    for s in &net.surfaces {
        for k in 0..s.centre.len().saturating_sub(1) {
            if is_elevated(s, k, &ground, gap) && is_elevated(s, k + 1, &ground, gap) {
                out.push(DeckSlab {
                    a: s.centre[k],
                    b: s.centre[k + 1],
                    left_m: s.left_m,
                    right_m: s.right_m,
                });
            }
        }
    }
    out
}

/// Wheel probes against the city's roads and terrain (no physics engine): the drivable
/// surface nearest the probe origin's height ([`CityMap::ground_height`]), with a
/// finite-difference normal. On a flat city this is exactly the z = 0 plane.
pub struct MapGround<'a> {
    /// The city.
    pub map: &'a CityMap,
}

impl WheelCaster for MapGround<'_> {
    fn cast(
        &self,
        origin: wp_vehicle::math::V3,
        dir: wp_vehicle::math::V3,
        max_dist: f32,
    ) -> Option<GroundHit> {
        if dir.z >= -1e-6 {
            return None;
        }
        let p = [origin.x, origin.y];
        let h = self.map.ground_height(p, Some(origin.z));
        let t = (origin.z - h) / -dir.z;
        if !(0.0..=max_dist).contains(&t) {
            return None;
        }
        let e = 0.5;
        let hx = self.map.ground_height([p[0] + e, p[1]], Some(h))
            - self.map.ground_height([p[0] - e, p[1]], Some(h));
        let hy = self.map.ground_height([p[0], p[1] + e], Some(h))
            - self.map.ground_height([p[0], p[1] - e], Some(h));
        let normal = if hx == 0.0 && hy == 0.0 {
            wp_vehicle::math::V3::Z
        } else {
            wp_vehicle::math::V3::new(-hx / (2.0 * e), -hy / (2.0 * e), 1.0).normalize()
        };
        Some(GroundHit {
            distance: t,
            point: origin + dir * t,
            normal,
        })
    }
}

impl wp_contracts::GroundQuery for MapGround<'_> {
    fn cast_ray(
        &self,
        origin: [f32; 3],
        dir: [f32; 3],
        max_m: f32,
    ) -> Option<wp_contracts::GroundHit> {
        self.cast(
            wp_vehicle::math::V3::from_array(origin),
            wp_vehicle::math::V3::from_array(dir),
            max_m,
        )
        .map(|h| wp_contracts::GroundHit {
            dist_m: h.distance,
            point: h.point.to_array(),
            normal: h.normal.to_array(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A level 10 m wide deck 8 m up, running +Y from (0, 0) to (0, 40), 4 m left / 6 m right.
    fn slab() -> DeckSlab {
        DeckSlab {
            a: [0.0, 0.0, 8.0],
            b: [0.0, 40.0, 8.0],
            left_m: 4.0,
            right_m: 6.0,
        }
    }

    #[test]
    fn deck_slab_blocks_only_rays_through_it() {
        let d = slab();
        let heli = [30.0, 20.0, 120.0];
        // Under the deck (right of the centreline is +X here), and on top of it.
        assert!(d.blocks(heli, [2.0, 20.0, 1.0]));
        assert!(!d.blocks(heli, [2.0, 20.0, 9.0]));
        // Beside the deck at ground level, past either edge or end.
        assert!(!d.blocks([10.0, 20.0, 120.0], [10.0, 20.0, 1.0]));
        assert!(!d.blocks([-6.0, 20.0, 120.0], [-6.0, 20.0, 1.0]));
        assert!(!d.blocks([0.0, 50.0, 120.0], [0.0, 50.0, 1.0]));
        // Left edge counts left_m, right edge right_m.
        assert!(d.blocks([-3.5, 20.0, 120.0], [-3.5, 20.0, 1.0]));
        assert!(d.blocks([5.5, 20.0, 120.0], [5.5, 20.0, 1.0]));
        // Horizontal rays under or over it never cross the plane; order does not matter.
        assert!(!d.blocks([-20.0, 20.0, 1.3], [20.0, 20.0, 1.3]));
        assert!(d.blocks([2.0, 20.0, 1.0], heli));
    }

    #[test]
    fn sloped_slab_follows_its_ramp() {
        // Rises from 2 m to 10 m: a ray ending 5 m up blocks near the high end only.
        let d = DeckSlab {
            a: [0.0, 0.0, 2.0],
            b: [0.0, 40.0, 10.0],
            left_m: 4.0,
            right_m: 4.0,
        };
        assert!(d.blocks([0.0, 36.0, 100.0], [0.0, 36.0, 5.0]));
        assert!(!d.blocks([0.0, 4.0, 100.0], [0.0, 4.0, 5.0]));
    }
}
