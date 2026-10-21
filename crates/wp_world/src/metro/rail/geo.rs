//! Geometry helpers of the rail module: a spatial index of the road paving (with heights,
//! so grade-separated roads are told apart), convex overlap tests and box builders.

use super::super::ctx::{obb, yaw_of, Ctx};
use super::super::plan::{MAP_HI, MAP_LO};
use crate::geom::{self, V2, V3};
use crate::network::{Feature, FeatureKind, FeatureShape, Material};

/// Bucket size of the paving index, m.
const BUCKET_M: f32 = 50.0;
/// A solid must stay this far above paving it overlaps in plan view (clearance), m.
pub const CLEARANCE_M: f32 = 4.5;

/// One paved quad of a road surface (or a junction box polygon).
#[derive(Debug, Clone)]
pub struct PaveQuad {
    /// Outline (convex, CCW or CW), m.
    pub poly: Vec<V2>,
    /// Lowest surface height, m.
    pub z_lo: f32,
    /// Highest surface height, m.
    pub z_hi: f32,
    /// Centreline segment (start, end) for crossing detection (`None` for junction boxes).
    pub centre: Option<(V3, V3)>,
    /// Half paved width, m.
    pub half_w: f32,
    /// Part of a highway carriageway.
    pub highway: bool,
}

/// Spatial index of the paving of `ctx.roads.built`.
pub struct Paving {
    /// All quads.
    pub quads: Vec<PaveQuad>,
    nx: usize,
    ny: usize,
    buckets: Vec<Vec<u32>>,
}

impl Paving {
    /// Index every surface quad and junction box.
    pub fn new(ctx: &Ctx) -> Self {
        let mut quads = Vec::new();
        for s in &ctx.roads.built.surfaces {
            let (l, r) = crate::surface::surface_edges(s);
            let half_w = s.left_m.max(s.right_m);
            for k in 0..l.len().saturating_sub(1) {
                let pts = [l[k], l[k + 1], r[k + 1], r[k]];
                let z_lo = pts.iter().map(|p| p[2]).fold(f32::MAX, f32::min);
                let z_hi = pts.iter().map(|p| p[2]).fold(f32::MIN, f32::max);
                quads.push(PaveQuad {
                    poly: pts.iter().map(|p| geom::xy(*p)).collect(),
                    z_lo,
                    z_hi,
                    centre: Some((s.centre[k], s.centre[k + 1])),
                    half_w,
                    highway: s.highway,
                });
            }
        }
        for it in &ctx.roads.built.intersections {
            let n = &ctx.roads.built.nodes[it.node as usize];
            let r = it.radius_m.max(6.0);
            let poly: Vec<V2> = (0..12)
                .map(|k| {
                    let (s, c) = wp_math::sin_cos(k as f32 * core::f32::consts::TAU / 12.0);
                    [n.pos[0] + r * c, n.pos[1] + r * s]
                })
                .collect();
            quads.push(PaveQuad {
                poly,
                z_lo: n.pos[2] - 0.5,
                z_hi: n.pos[2] + 0.5,
                centre: None,
                half_w: r,
                highway: false,
            });
        }
        let nx = ((MAP_HI[0] - MAP_LO[0]) / BUCKET_M).ceil() as usize + 1;
        let ny = ((MAP_HI[1] - MAP_LO[1]) / BUCKET_M).ceil() as usize + 1;
        let mut p = Self {
            quads,
            nx,
            ny,
            buckets: vec![Vec::new(); nx * ny],
        };
        for (qi, q) in p.quads.iter().enumerate() {
            let (i0, j0, i1, j1) = p.range(&q.poly, 0.0);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    p.buckets[j * nx + i].push(qi as u32);
                }
            }
        }
        p
    }

    fn range(&self, poly: &[V2], margin: f32) -> (usize, usize, usize, usize) {
        let (lo, hi) = super::super::ground::bounds(poly);
        let cl = |v: f32, n: usize| (v.max(0.0) as usize).min(n - 1);
        (
            cl((lo[0] - margin - MAP_LO[0]) / BUCKET_M, self.nx),
            cl((lo[1] - margin - MAP_LO[1]) / BUCKET_M, self.ny),
            cl((hi[0] + margin - MAP_LO[0]) / BUCKET_M, self.nx),
            cl((hi[1] + margin - MAP_LO[1]) / BUCKET_M, self.ny),
        )
    }

    /// Indices of quads whose bucket overlaps `poly` grown by `margin` (deduplicated,
    /// ascending).
    pub fn near(&self, poly: &[V2], margin: f32) -> Vec<u32> {
        let (i0, j0, i1, j1) = self.range(poly, margin);
        let mut out: Vec<u32> = Vec::new();
        for j in j0..=j1 {
            for i in i0..=i1 {
                out.extend_from_slice(&self.buckets[j * self.nx + i]);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// True when convex `poly` grown by `margin` overlaps paving whose height band
    /// `[z_lo - 0.5, z_hi + CLEARANCE_M]` intersects `[z0, z1]`.
    pub fn hits(&self, poly: &[V2], margin: f32, z0: f32, z1: f32) -> bool {
        self.near(poly, margin).into_iter().any(|qi| {
            let q = &self.quads[qi as usize];
            q.z_lo - 0.5 <= z1
                && q.z_hi + CLEARANCE_M >= z0
                && convex_overlap(poly, &q.poly, margin)
        })
    }

    /// True when convex `poly` grown by `margin` overlaps highway paving in the height band
    /// `[z0, z1]` (see [`Paving::hits`]).
    pub fn hits_highway(&self, poly: &[V2], margin: f32, z0: f32, z1: f32) -> bool {
        self.near(poly, margin).into_iter().any(|qi| {
            let q = &self.quads[qi as usize];
            q.highway
                && q.z_lo - 0.5 <= z1
                && q.z_hi + CLEARANCE_M >= z0
                && convex_overlap(poly, &q.poly, margin)
        })
    }

    /// True when `poly` grown by `margin` overlaps any paving at any height.
    pub fn hits_any(&self, poly: &[V2], margin: f32) -> bool {
        self.hits(poly, margin, f32::MIN / 2.0, f32::MAX / 2.0)
    }
}

/// Separating-axis overlap test of two convex polygons, `a` grown by `margin`.
pub fn convex_overlap(a: &[V2], b: &[V2], margin: f32) -> bool {
    for poly in [a, b] {
        let n = poly.len();
        for i in 0..n {
            let e = geom::sub(poly[(i + 1) % n], poly[i]);
            let ax = geom::normalize([-e[1], e[0]]);
            let proj = |p: &[V2]| {
                p.iter().fold((f32::MAX, f32::MIN), |(lo, hi), q| {
                    let d = geom::dot(*q, ax);
                    (lo.min(d), hi.max(d))
                })
            };
            let (a0, a1) = proj(a);
            let (b0, b1) = proj(b);
            if a1 + margin < b0 || b1 + margin < a0 {
                return false;
            }
        }
    }
    true
}

/// Footprint (4 corners) of a strip from `a` to `b`, `half_w` to either side.
pub fn strip(a: V2, b: V2, half_w: f32) -> Vec<V2> {
    let mid = geom::scale(geom::add(a, b), 0.5);
    obb(mid, [half_w, geom::dist(a, b) * 0.5], yaw_of(a, b))
}

/// A box following a sloped piece from `a` to `b` (3D points on a reference line),
/// offset `lat` to the right, spanning `z0..z1` relative to the reference line, `width`
/// wide, extended by `extend` at both ends.
#[allow(clippy::too_many_arguments)]
pub fn piece(
    kind: FeatureKind,
    material: Material,
    a: V3,
    b: V3,
    width: f32,
    z0: f32,
    z1: f32,
    extend: f32,
    solid: bool,
) -> Feature {
    let horiz = geom::dist(geom::xy(a), geom::xy(b)).max(1e-3);
    let dz = b[2] - a[2];
    let len = (horiz * horiz + dz * dz).sqrt();
    let pitch = wp_math::atan2(dz, horiz);
    Feature {
        kind,
        material,
        shape: FeatureShape::Box {
            centre: [
                (a[0] + b[0]) * 0.5,
                (a[1] + b[1]) * 0.5,
                (a[2] + b[2]) * 0.5 + (z0 + z1) * 0.5,
            ],
            half: [width * 0.5, len * 0.5 + extend, (z1 - z0) * 0.5],
            yaw: yaw_of(geom::xy(a), geom::xy(b)),
            pitch,
        },
        solid,
    }
}

/// Point `p` moved `d` along unit direction `dir`.
pub fn along(p: V2, dir: V2, d: f32) -> V2 {
    [p[0] + dir[0] * d, p[1] + dir[1] * d]
}

/// 2D segment intersection: parameters `(t, u)` along `a0→a1` and `b0→b1`, if they cross.
pub fn seg_intersect(a0: V2, a1: V2, b0: V2, b1: V2) -> Option<(f32, f32)> {
    let r = geom::sub(a1, a0);
    let s = geom::sub(b1, b0);
    let den = geom::cross(r, s);
    if den.abs() < 1e-9 {
        return None;
    }
    let q = geom::sub(b0, a0);
    let t = geom::cross(q, s) / den;
    let u = geom::cross(q, r) / den;
    if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
        Some((t, u))
    } else {
        None
    }
}

/// Quadrilateral annulus sector (convex for spans < 180°): radii `r0..r1`, angles
/// `a0..a1` (rad, CCW from +X) around `c`.
pub fn sector(c: V2, r0: f32, r1: f32, a0: f32, a1: f32) -> Vec<V2> {
    let (s0, c0) = wp_math::sin_cos(a0);
    let (s1, c1) = wp_math::sin_cos(a1);
    vec![
        [c[0] + r0 * c0, c[1] + r0 * s0],
        [c[0] + r1 * c0, c[1] + r1 * s0],
        [c[0] + r1 * c1, c[1] + r1 * s1],
        [c[0] + r0 * c1, c[1] + r0 * s1],
    ]
}
