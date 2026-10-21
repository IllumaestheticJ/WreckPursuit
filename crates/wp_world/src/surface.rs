//! Drivable surface geometry shared by physics (`wp_sim` colliders), the greybox exporter and
//! `wp_world`'s own physical traffic:
//! - [`road_mesh`]: one triangle mesh of every paved surface — road ribbons (banked), junction
//!   discs and the connector ribbons through bends;
//! - [`deck_rails`]: guard-rail lines along bridge decks;
//! - [`median_barriers`]: concrete barrier lines between divided-highway carriageways;
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

/// Distance from `q` to `s`'s centreline (clamped to its ends) and the height of the banked
/// surface under `q` (nearest centreline point).
pub fn height_under(s: &RoadSurface, q: V2) -> (f32, f32) {
    let cum = geom::cumulative(&s.centre);
    let n = s.centre.len();
    let pr = geom::project(&s.centre, &cum, q);
    let t = pr.s.clamp(0.0, cum[n - 1]);
    let (c, _) = geom::point_at(&s.centre, &cum, t);
    let i = cum.partition_point(|x| *x <= t).clamp(1, n - 1);
    let span = (cum[i] - cum[i - 1]).max(1e-6);
    let f = ((t - cum[i - 1]) / span).clamp(0.0, 1.0);
    let bank = match s.bank.len() {
        0 => 0.0,
        _ => s.bank[i - 1] + (s.bank[i] - s.bank[i - 1]) * f,
    };
    (pr.dist, c[2] + bank * pr.lateral)
}

/// Every paved surface of a city as one mesh: road ribbons, junction discs (16-gons) and the
/// connector ribbons through bend nodes (degree-2 nodes have no junction disc) and through
/// freeway merges and diverges (radius-0 junctions: the main road's paving runs through,
/// only the ramp's taper connectors are added).
pub fn road_mesh(map: &CityMap) -> TriMesh {
    let net = &map.net;
    let mut m = TriMesh::default();
    for s in &net.surfaces {
        let (l, r) = surface_edges(s);
        m.strip(&l, &r);
    }
    let mut lanes_per_road = vec![0u32; net.surfaces.len()];
    for l in &net.lanes {
        if let Some(n) = lanes_per_road.get_mut(l.road as usize) {
            *n += 1;
        }
    }
    // Disc radius: the junction box, grown to cover the corners of every lane end at it (and
    // the 16-gon's chords), so no wheel finds a gap between a lane and the box.
    let mut disc_r: Vec<f32> = net.intersections.iter().map(|ix| ix.radius_m).collect();
    for l in &net.lanes {
        let n = l.polyline.len();
        if n < 2 {
            continue;
        }
        for (node, p) in [(l.to_node, l.polyline[n - 1]), (l.from_node, l.polyline[0])] {
            let Some(ix) = net.nodes[node as usize].intersection else {
                continue;
            };
            if net.intersections[ix as usize].radius_m <= 0.0 {
                continue;
            }
            let c = geom::xy(net.nodes[node as usize].pos);
            let along = geom::dist(geom::xy(p), c);
            let corner = (along * along + 0.25 * l.width_m * l.width_m).sqrt();
            let r = &mut disc_r[ix as usize];
            // cos(π / 16): the 16-gon's chords sit this much inside its corners.
            *r = r.max(corner / 0.980_785_3 + 0.1);
        }
    }
    for (ix, r) in net.intersections.iter().zip(&disc_r) {
        if ix.radius_m <= 0.0 {
            continue;
        }
        let c = net.nodes[ix.node as usize].pos;
        let poly: Vec<V3> = (0..16)
            .map(|k| {
                let (sn, cs) = wp_math::sin_cos(k as f32 / 16.0 * std::f32::consts::TAU);
                [c[0] + cs * r, c[1] + sn * r, c[2]]
            })
            .collect();
        m.fan(&poly);
    }
    for s in &map.segs {
        if let SegKind::Connector { node, from, to, .. } = s.kind {
            let draw = match net.nodes[node as usize].intersection {
                None => true,
                Some(ix) if net.intersections[ix as usize].radius_m <= 0.0 => {
                    let n = |l: u32| lanes_per_road.get(net.lanes[l as usize].road as usize);
                    n(from) != n(to)
                }
                Some(_) => false,
            };
            if draw {
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
    for (si, s) in net.surfaces.iter().enumerate() {
        let (l, r) = surface_edges(s);
        for k in 0..s.centre.len() - 1 {
            if is_elevated(s, k, &ground, 1.5) && is_elevated(s, k + 1, &ground, 1.5) {
                for e in [&l, &r] {
                    if !inside_other_paving(net, si, e[k])
                        || !inside_other_paving(net, si, e[k + 1])
                    {
                        out.push((e[k], e[k + 1]));
                    }
                }
            }
        }
    }
    out
}

/// Whether `p` lies on the paving of a surface other than `si`, at its level (a ramp edge
/// inside the carriageway it leaves, for example: no rail there).
pub fn inside_other_paving(net: &RoadNetwork, si: usize, p: V3) -> bool {
    let q = geom::xy(p);
    net.surfaces.iter().enumerate().any(|(oi, o)| {
        if oi == si || o.centre.len() < 2 {
            return false;
        }
        let w = o.left_m.max(o.right_m);
        // Cheap reject on the end points' bounding box.
        let (a, b) = (
            geom::xy(o.centre[0]),
            geom::xy(o.centre[o.centre.len() - 1]),
        );
        let reach = geom::dist(a, b) * 0.5 + w + 400.0;
        if geom::dist(q, geom::scale(geom::add(a, b), 0.5)) > reach {
            return false;
        }
        let cum = geom::cumulative(&o.centre);
        let pr = geom::project(&o.centre, &cum, q);
        let inside = if pr.lateral < 0.0 {
            -pr.lateral < o.left_m - 0.3
        } else {
            pr.lateral < o.right_m - 0.3
        };
        pr.s > 0.0 && pr.s < cum[cum.len() - 1] && inside && {
            let (_, z) = height_under(o, q);
            (z - p[2]).abs() < 1.0
        }
    })
}

/// Median barrier polylines (at road height, along the left paved edge of every surface
/// flagged [`RoadSurface::median_barrier`]).
pub fn median_barriers(net: &RoadNetwork) -> Vec<Vec<V3>> {
    net.surfaces
        .iter()
        .filter(|s| s.median_barrier && s.centre.len() >= 2)
        .map(|s| surface_edges(s).0)
        .collect()
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
