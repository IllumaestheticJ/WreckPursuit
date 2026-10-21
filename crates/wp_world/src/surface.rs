//! Drivable surface geometry shared by physics (`wp_sim` colliders), the greybox exporter and
//! `wp_world`'s own physical traffic:
//! - [`road_mesh`]: one triangle mesh of every paved surface — road ribbons (banked), junction
//!   discs and the connector ribbons through bends;
//! - [`deck_rails`]: guard-rail lines along bridge decks;
//! - [`collision_terrain`]: the terrain physics collides with (the drawn terrain off the roads);
//! - [`MapGround`]: a wheel ground probe over roads and terrain without a physics engine.

use wp_vehicle::{GroundHit, WheelCaster};

use crate::geom::{self, V2, V3};
use crate::map::{CityMap, SegKind};
use crate::network::{RoadNetwork, RoadSurface};
use crate::terrain::Heightfield;

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

/// Vertex spacing of the terrain the greybox mesh draws ([`Heightfield::lowered_coarse`]), m.
pub const TERRAIN_RENDER_M: f32 = 10.0;

/// Sidewalk width beside a non-highway road edge (greybox and collision terrain), m.
pub const SIDEWALK_M: f32 = 3.0;

/// The terrain physics collides with, on the fine grid of `net.terrain` (`None` without
/// terrain): the surface the greybox draws ([`Heightfield::lowered_coarse`] at
/// [`TERRAIN_RENDER_M`]), except in the band around at-grade roads, junctions and bends
/// (paved width + sidewalk + half a cell), which keeps the fine heights so the ground under
/// sidewalks stays at road level. Drawn and solid ground agree everywhere else, so there is
/// no hillside the player can hit but not see; the fine grid samples the coarse triangles
/// exactly (same diagonal), so the two surfaces are identical off the roads.
pub fn collision_terrain(map: &CityMap) -> Option<Heightfield> {
    let net = &map.net;
    let t = net.terrain.as_ref()?;
    let drawn = t.lowered_coarse(TERRAIN_RENDER_M);
    let mut keep = vec![false; t.z.len()];
    let margin = t.cell_m * 0.5;
    let mut mark = |a: V2, b: V2, r: f32| {
        let lo = [a[0].min(b[0]) - r, a[1].min(b[1]) - r];
        let hi = [a[0].max(b[0]) + r, a[1].max(b[1]) + r];
        let cell = |x: f32, o: f32, n: u32| (((x - o) / t.cell_m).max(0.0) as u32).min(n - 1);
        let (i0, i1) = (
            cell(lo[0], t.origin[0], t.nx),
            cell(hi[0], t.origin[0], t.nx) + 1,
        );
        let (j0, j1) = (
            cell(lo[1], t.origin[1], t.ny),
            cell(hi[1], t.origin[1], t.ny) + 1,
        );
        for j in j0..=j1.min(t.ny - 1) {
            for i in i0..=i1.min(t.nx - 1) {
                if geom::point_segment_distance(t.vertex_pos(i, j), a, b) <= r {
                    keep[(j * t.nx + i) as usize] = true;
                }
            }
        }
    };
    let ground = |q: V2| t.height_at(q);
    for s in &net.surfaces {
        let side = if s.highway { 0.3 } else { SIDEWALK_M };
        let r = s.left_m.max(s.right_m) + side + margin;
        for k in 0..s.centre.len() - 1 {
            // Bridge decks: the ground under them is the drawn ground.
            if is_elevated(s, k, &ground, 1.5) && is_elevated(s, k + 1, &ground, 1.5) {
                continue;
            }
            mark(geom::xy(s.centre[k]), geom::xy(s.centre[k + 1]), r);
        }
    }
    for ix in &net.intersections {
        let c = net.nodes[ix.node as usize].pos;
        if c[2] - ground(geom::xy(c)) <= 1.5 {
            mark(geom::xy(c), geom::xy(c), ix.radius_m + SIDEWALK_M + margin);
        }
    }
    for s in &map.segs {
        if let SegKind::Connector { node, .. } = s.kind {
            if net.nodes[node as usize].intersection.is_none() {
                let r = s.width * 0.5 + SIDEWALK_M + margin;
                for w in s.pts.windows(2) {
                    if w[0][2] - ground(geom::xy(w[0])) <= 1.5 {
                        mark(geom::xy(w[0]), geom::xy(w[1]), r);
                    }
                }
            }
        }
    }
    let mut out = t.clone();
    for j in 0..t.ny {
        for i in 0..t.nx {
            let n = (j * t.nx + i) as usize;
            if !keep[n] {
                out.z[n] = drawn.height_on_triangles(t.vertex_pos(i, j));
            }
        }
    }
    Some(out)
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
