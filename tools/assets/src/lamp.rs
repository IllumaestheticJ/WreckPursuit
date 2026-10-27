//! Lamps projected onto the body surface (car-shapes phase 3).
//!
//! A [`Lamp`] is a 2D outline in the coordinates of a view (the nose seen from ahead, the tail
//! from behind, a side, or the top). Every point of the outline is cast along the view
//! direction onto the body's panel / body triangles already built (the loft runs before style
//! details), so the lens sits proud of exactly the drawn surface and follows the curved nose,
//! fender or tail: a lens drawn in the front view near the corner wraps round the plan-rounded corner
//! onto the fender (more with [`Lamp::yaw_deg`]), one drawn in the top view lies on the hood
//! or fender top.
//!
//! - [`lens`]: the lens face [`Lamp::proud_m`] over the surface with a rim wall down to
//!   [`Lamp::depth_m`] under it (the back, inside the body, is left open). Use the head / tail lamp and turn-signal keys
//!   (`kit::headlight_key`, `mesh::indicator_front_key`, …) so the renderer's lamp slots and
//!   the turn-signal contract keep working.
//! - [`bezel`]: a trim ring of a given width round the outline, a little less proud.
//! - [`split_outline`] / [`clip_outline`]: cut an outline in two along a view axis with the
//!   [`INDICATOR_SEAM_M`] gap, for the amber turn-signal segment of a lamp cluster.
//!
//! Outlines must be star-shaped around their centroid (every convex shape, ovals, teardrops,
//! swept or slim strips; not a C or a boomerang: build those from two lenses). Points whose
//! ray misses the body (past the silhouette) are pulled toward the centroid until they hit.
//! Chassis metres; deterministic.

use crate::builder::{Builder, PartKey};
use crate::mesh::{PartKind, INDICATOR_SEAM_M};
use crate::profile::BodyProfile;

/// The view an outline is drawn in, and the direction its points are cast along.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LampView {
    /// The nose seen from ahead: outline `[x, z]` (chassis x, signed; z above ground), cast
    /// rearward (−Y).
    Front,
    /// The tail seen from behind: outline `[x, z]` (chassis x, signed), cast forward (+Y).
    Rear,
    /// A side: outline `[y, z]` (chassis y, z), cast inward from the left (`right = false`,
    /// x < 0) or the right side.
    Side {
        /// The right (+X) side.
        right: bool,
    },
    /// The top seen from above: outline `[x, y]` (chassis x signed, y), cast downward.
    Top,
}

/// A projected lamp lens (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub struct Lamp {
    /// View and cast direction.
    pub view: LampView,
    /// Outline in the view's coordinates (any winding, ≥ 3 points, star-shaped round its
    /// centroid).
    pub outline: Vec<[f32; 2]>,
    /// Front / rear views only: swings the cast direction toward the outline's own side by
    /// this many degrees (0…60), so a lens drawn near the corner wraps further round onto the
    /// fender / quarter (wraparound tail lamps). The outline's x is then measured across the
    /// swung direction (still in metres, still signed). Default 0.
    pub yaw_deg: f32,
    /// Lens face over the surface, m (0.002…0.03). Default 0.008.
    pub proud_m: f32,
    /// How far the lens rim reaches under the surface, m (0…0.01; keep it small: rim faces seen
    /// edge-on on a corner then dip into the body or under a bezel). Default 0.
    pub depth_m: f32,
    /// Grid step of the lens, m (0.01…0.08): perimeter spacing (8…32 points), rings every two
    /// steps (1…3 rings). Default 0.04. A lens costs ≈ 2 × points × rings + 2 × points
    /// triangles (≈ 100–250), a bezel 4 × points.
    pub step_m: f32,
}

impl Lamp {
    /// A lamp in `view` with the default proud / depth / step and no yaw.
    pub fn new(view: LampView, outline: Vec<[f32; 2]>) -> Lamp {
        Lamp {
            view,
            outline,
            yaw_deg: 0.0,
            proud_m: 0.008,
            depth_m: 0.0,
            step_m: 0.04,
        }
    }
}

/// Outline points of an ellipse centred at `c` with half axes `r` (view coordinates), `n`
/// points (≥ 6), counter-clockwise.
pub fn ellipse(c: [f32; 2], r: [f32; 2], n: usize) -> Vec<[f32; 2]> {
    let n = n.max(6);
    (0..n)
        .map(|i| {
            let (s, co) = crate::builder::turn(i, n);
            [c[0] + r[0] * co, c[1] + r[1] * s]
        })
        .collect()
}

/// The part of `outline` with coordinate `axis` (0 = first, 1 = second) below `at`
/// (`keep_below`) or above it (Sutherland–Hodgman clip against one line).
pub fn clip_outline(outline: &[[f32; 2]], axis: usize, at: f32, keep_below: bool) -> Vec<[f32; 2]> {
    let axis = axis.min(1);
    let inside = |q: [f32; 2]| {
        if keep_below {
            q[axis] <= at
        } else {
            q[axis] >= at
        }
    };
    let mut out = Vec::new();
    let n = outline.len();
    for i in 0..n {
        let a = outline[i];
        let b = outline[(i + 1) % n];
        let (ia, ib) = (inside(a), inside(b));
        if ia {
            out.push(a);
        }
        if ia != ib {
            let t = (at - a[axis]) / (b[axis] - a[axis]);
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    out
}

/// Split `outline` along `axis` at `at` into (below, above), each kept
/// [`INDICATOR_SEAM_M`] / 2 clear of the cut (the lens divider of a lamp cluster).
pub fn split_outline(outline: &[[f32; 2]], axis: usize, at: f32) -> (Vec<[f32; 2]>, Vec<[f32; 2]>) {
    let g = 0.5 * INDICATOR_SEAM_M;
    (
        clip_outline(outline, axis, at - g, true),
        clip_outline(outline, axis, at + g, false),
    )
}

/// The lens of `lamp` into part `key` (a face following the surface, with a rim wall).
pub fn lens(b: &mut Builder, p: &BodyProfile, key: &PartKey, lamp: &Lamp) {
    let Some(caster) = Caster::new(b, p, lamp) else {
        return;
    };
    let grid = caster.grid(lamp);
    let proud = lamp.proud_m.clamp(0.002, 0.03);
    let depth = lamp.depth_m.clamp(0.0, 0.01);
    shell(b, &caster, key, &grid, proud, -depth);
}

/// How far a [`bezel`] band stands off the surface, m (flush: it never shadows the lens rim).
pub const BEZEL_LIFT_M: f32 = 0.001;

/// A trim bezel `width_m` wide (0.003…0.05) round `lamp`'s outline into part `key`: a band
/// [`BEZEL_LIFT_M`] over the surface from the lens edge outward, with a skirt into the body.
/// Draw the lens with [`lens`] (same `lamp`) on top of it.
pub fn bezel(b: &mut Builder, p: &BodyProfile, key: &PartKey, lamp: &Lamp, width_m: f32) {
    let Some(caster) = Caster::new(b, p, lamp) else {
        return;
    };
    let w = width_m.clamp(0.003, 0.05);
    let depth = lamp.depth_m.clamp(0.0, 0.01);
    let ring_in = caster.boundary(lamp, 0.0);
    let ring_out = caster.boundary(lamp, w);
    let centre = caster.centre;
    let n = ring_in.len();
    for i in 0..n {
        let k = (i + 1) % n;
        let (a, c) = (ring_in[i], ring_in[k]);
        let (d, e) = (ring_out[i], ring_out[k]);
        let ai = add(a.0, scale(a.1, BEZEL_LIFT_M));
        let ci = add(c.0, scale(c.1, BEZEL_LIFT_M));
        let dout = add(d.0, scale(d.1, BEZEL_LIFT_M));
        let eout = add(e.0, scale(e.1, BEZEL_LIFT_M));
        let up = norm(add(a.1, d.1));
        b.poly_facing(key, &[ai, dout, eout, ci], up);
        // Skirt from the bezel's outer edge into the body.
        let dd = add(d.0, scale(d.1, -depth.max(0.004)));
        let ed = add(e.0, scale(e.1, -depth.max(0.004)));
        let mid = scale(add(add(dout, eout), add(dd, ed)), 0.25);
        b.poly_facing(key, &[dout, dd, ed, eout], sub(mid, centre));
    }
}

/// One projected grid point: (surface point, outward normal).
type Hit = ([f32; 3], [f32; 3]);

/// A body triangle seen in the view: vertex (u, v) and distance along the cast direction.
type ViewTri = [[f32; 3]; 3];

/// Casts view points onto the body: the loft's panel / body triangles already in the builder
/// (the mesh itself, so a lens is proud of exactly what is drawn).
struct Caster {
    tris: Vec<ViewTri>,
    /// Cast direction (unit).
    dir: [f32; 3],
    /// View axes: first and second outline coordinate.
    u: [f32; 3],
    v: [f32; 3],
    /// Start of a cast for outline point (0, 0): outside the body.
    origin: [f32; 3],
    /// 2D centroid of the outline.
    c2: [f32; 2],
    /// Surface point of the centroid.
    centre: [f32; 3],
}

impl Caster {
    fn new(b: &Builder, p: &BodyProfile, lamp: &Lamp) -> Option<Caster> {
        if lamp.outline.len() < 3 {
            return None;
        }
        let c2 = centroid(&lamp.outline);
        let yaw = lamp.yaw_deg.clamp(0.0, 60.0).to_radians();
        let (sy, cy) = wp_math::sin_cos(yaw);
        let side = if c2[0] < 0.0 { -1.0 } else { 1.0 };
        let (dir, u, v, origin) = match lamp.view {
            LampView::Front => {
                // From ahead (and, with yaw, from the outline's side), travelling in; the cast
                // plane passes through the nose face.
                let dir = [-side * sy, -cy, 0.0];
                let u = horizontal_across(dir, [1.0, 0.0, 0.0]);
                (
                    dir,
                    u,
                    [0.0, 0.0, 1.0],
                    sub([0.0, p.y_nose, 0.0], scale(dir, 1.5)),
                )
            }
            LampView::Rear => {
                let dir = [-side * sy, cy, 0.0];
                let u = horizontal_across(dir, [1.0, 0.0, 0.0]);
                (
                    dir,
                    u,
                    [0.0, 0.0, 1.0],
                    sub([0.0, p.y_tail, 0.0], scale(dir, 1.5)),
                )
            }
            LampView::Side { right } => {
                let sx = if right { 1.0 } else { -1.0 };
                (
                    [-sx, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    [0.0, 0.0, 1.0],
                    [sx * (p.half_w + 1.0), 0.0, 0.0],
                )
            }
            LampView::Top => (
                [0.0, 0.0, -1.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, p.z_max() + 1.0],
            ),
        };
        // Body triangles whose view footprint overlaps the outline (+ bezel and normal margin).
        let m = 0.08;
        let lo = [
            lamp.outline.iter().map(|q| q[0]).fold(f32::MAX, f32::min) - m,
            lamp.outline.iter().map(|q| q[1]).fold(f32::MAX, f32::min) - m,
        ];
        let hi = [
            lamp.outline.iter().map(|q| q[0]).fold(f32::MIN, f32::max) + m,
            lamp.outline.iter().map(|q| q[1]).fold(f32::MIN, f32::max) + m,
        ];
        let mut tris = Vec::new();
        for key in b.keys() {
            if key.wheel.is_some() || !matches!(key.kind, PartKind::Panel | PartKind::Body) {
                continue;
            }
            let Some((pos, _, idx)) = b.geometry(&key.name) else {
                continue;
            };
            for t in idx.chunks_exact(3) {
                let tri: ViewTri = [0, 1, 2].map(|i| {
                    let d = sub(pos[t[i] as usize], origin);
                    [dot(d, u), dot(d, v), dot(d, dir)]
                });
                let out = (0..2)
                    .any(|k| tri.iter().all(|w| w[k] < lo[k]) || tri.iter().all(|w| w[k] > hi[k]));
                if !out {
                    tris.push(tri);
                }
            }
        }
        if tris.is_empty() {
            return None;
        }
        let mut caster = Caster {
            tris,
            dir,
            u,
            v,
            origin,
            c2,
            centre: [0.0; 3],
        };
        caster.centre = caster.cast_or_pull(c2, c2).0;
        Some(caster)
    }

    /// Ray start for view point `q`.
    fn start(&self, q: [f32; 2]) -> [f32; 3] {
        add(self.origin, add(scale(self.u, q[0]), scale(self.v, q[1])))
    }

    /// First body surface point along the cast of view point `q` and the outward normal of
    /// the triangle hit (turned to the viewer), or None.
    fn cast_tri(&self, q: [f32; 2]) -> Option<Hit> {
        let mut best: Option<(f32, usize)> = None;
        for (i, t) in self.tris.iter().enumerate() {
            let (a, b, c) = (t[0], t[1], t[2]);
            let det = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
            if det.abs() < 1e-12 {
                continue;
            }
            let l1 = ((q[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (q[1] - a[1])) / det;
            let l2 = ((b[0] - a[0]) * (q[1] - a[1]) - (q[0] - a[0]) * (b[1] - a[1])) / det;
            let e = -1e-5;
            if l1 < e || l2 < e || l1 + l2 > 1.0 - e {
                continue;
            }
            let d = a[2] + (b[2] - a[2]) * l1 + (c[2] - a[2]) * l2;
            if d > 0.0 && best.is_none_or(|x| d < x.0) {
                best = Some((d, i));
            }
        }
        let (d, i) = best?;
        let t = self.tris[i];
        // Triangle normal from view coordinates back to chassis space, turned to the viewer.
        let w = |k: usize| {
            add(
                add(scale(self.u, t[k][0]), scale(self.v, t[k][1])),
                scale(self.dir, t[k][2]),
            )
        };
        let n = norm(cross(sub(w(1), w(0)), sub(w(2), w(0))));
        let n = if dot(n, self.dir) > 0.0 {
            scale(n, -1.0)
        } else {
            n
        };
        Some((add(self.start(q), scale(self.dir, d)), n))
    }

    /// Surface point and normal of view point `q`, pulled toward `toward` (a point known to
    /// hit) until the cast hits.
    fn cast_or_pull(&self, q: [f32; 2], toward: [f32; 2]) -> Hit {
        let mut f = 1.0f32;
        let mut hit = self.cast_tri(q);
        while hit.is_none() && f > 0.02 {
            f -= 0.08;
            hit = self.cast_tri([
                toward[0] + (q[0] - toward[0]) * f,
                toward[1] + (q[1] - toward[1]) * f,
            ]);
        }
        hit.unwrap_or_else(|| (self.start(q), scale(self.dir, -1.0)))
    }

    /// Polar grid of the lens: centre, then `nr` rings of the resampled outline.
    fn grid(&self, lamp: &Lamp) -> Vec<Vec<Hit>> {
        let step = lamp.step_m.clamp(0.01, 0.08);
        let ring2 = resample(&lamp.outline, step);
        let rmax = ring2
            .iter()
            .map(|q| ((q[0] - self.c2[0]).powi(2) + (q[1] - self.c2[1]).powi(2)).sqrt())
            .fold(0.0f32, f32::max);
        let nr = ((rmax / (2.0 * step)).ceil() as usize).clamp(1, 3);
        let centre = self.cast_or_pull(self.c2, self.c2);
        let mut rows = vec![vec![centre]];
        for k in 1..=nr {
            let f = k as f32 / nr as f32;
            rows.push(
                ring2
                    .iter()
                    .map(|q| {
                        let q = [
                            self.c2[0] + (q[0] - self.c2[0]) * f,
                            self.c2[1] + (q[1] - self.c2[1]) * f,
                        ];
                        self.cast_or_pull(q, self.c2)
                    })
                    .collect(),
            );
        }
        rows
    }

    /// The outline (resampled at the lens step) offset outward by `off` m in the view, cast.
    fn boundary(&self, lamp: &Lamp, off: f32) -> Vec<Hit> {
        resample(&lamp.outline, lamp.step_m.clamp(0.01, 0.08))
            .iter()
            .map(|q| {
                let d = [q[0] - self.c2[0], q[1] - self.c2[1]];
                let l = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-6);
                let q2 = [q[0] + d[0] / l * off, q[1] + d[1] / l * off];
                self.cast_or_pull(q2, self.c2)
            })
            .collect()
    }
}

/// Lens over a polar grid: the face at `hi` along the normals and a rim wall down to `lo`
/// (under the surface; the back is inside the body and left open). Faces whose middle would
/// sit less than half of `hi` clear of the body (a chord across a rounded corner) are pushed
/// toward the viewer until they clear it.
fn shell(b: &mut Builder, caster: &Caster, key: &PartKey, grid: &[Vec<Hit>], hi: f32, lo: f32) {
    let n = grid.get(1).map_or(0, |r| r.len());
    if n < 3 {
        return;
    }
    // Vertex (row, index) → slot; row 0 is the centre.
    let slot = |r: usize, i: usize| if r == 0 { 0 } else { 1 + (r - 1) * n + i % n };
    let mut top: Vec<[f32; 3]> = vec![add(grid[0][0].0, scale(grid[0][0].1, hi))];
    for row in &grid[1..] {
        for h in row {
            top.push(add(h.0, scale(h.1, hi)));
        }
    }
    let mut tris: Vec<[usize; 3]> = Vec::new();
    for i in 0..n {
        tris.push([slot(0, 0), slot(1, i), slot(1, i + 1)]);
    }
    for r in 1..grid.len() - 1 {
        for i in 0..n {
            let (a0, a1, e0, e1) = (
                slot(r, i),
                slot(r, i + 1),
                slot(r + 1, i),
                slot(r + 1, i + 1),
            );
            tris.push([a0, e0, e1]);
            tris.push([a0, e1, a1]);
        }
    }
    let depth = |q: [f32; 3]| dot(sub(q, caster.origin), caster.dir);
    for _ in 0..3 {
        let mut push = vec![0.0f32; top.len()];
        for t in &tris {
            let c = scale(add(add(top[t[0]], top[t[1]]), top[t[2]]), 1.0 / 3.0);
            let d = sub(c, caster.origin);
            let Some((s, _)) = caster.cast_tri([dot(d, caster.u), dot(d, caster.v)]) else {
                continue;
            };
            let need = 0.5 * hi - (depth(s) - depth(c));
            if need > 1e-4 {
                for &v in t {
                    push[v] = push[v].max(need);
                }
            }
        }
        if push.iter().all(|&x| x <= 0.0) {
            break;
        }
        for (q, &d) in top.iter_mut().zip(&push) {
            *q = sub(*q, scale(caster.dir, d));
        }
    }
    let hit = |v: usize| -> &Hit {
        if v == 0 {
            &grid[0][0]
        } else {
            &grid[1 + (v - 1) / n][(v - 1) % n]
        }
    };
    for t in &tris {
        let nrm = norm(add(add(hit(t[0]).1, hit(t[1]).1), hit(t[2]).1));
        b.poly_facing(key, &[top[t[0]], top[t[1]], top[t[2]]], nrm);
    }
    // Rim wall round the outer ring, from the (pushed) face down under the surface.
    let r = grid.len() - 1;
    let centre = grid[0][0].0;
    for i in 0..n {
        let (s0, s1) = (slot(r, i), slot(r, i + 1));
        let (h0, h1) = (hit(s0), hit(s1));
        let q = [
            add(h0.0, scale(h0.1, lo)),
            add(h1.0, scale(h1.1, lo)),
            top[s1],
            top[s0],
        ];
        let mid = scale(add(add(q[0], q[1]), add(q[2], q[3])), 0.25);
        b.poly_facing(key, &q, sub(mid, centre));
    }
}

/// Unit horizontal vector across `dir` (perpendicular to it and to Z), on the side of `hint`.
fn horizontal_across(dir: [f32; 3], hint: [f32; 3]) -> [f32; 3] {
    let a = norm([-dir[1], dir[0], 0.0]);
    if dot(a, hint) < 0.0 {
        scale(a, -1.0)
    } else {
        a
    }
}

/// Area centroid of a polygon (vertex mean if degenerate).
fn centroid(q: &[[f32; 2]]) -> [f32; 2] {
    let n = q.len();
    let (mut a, mut cx, mut cy) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..n {
        let (p0, p1) = (q[i], q[(i + 1) % n]);
        let c = p0[0] * p1[1] - p1[0] * p0[1];
        a += c;
        cx += (p0[0] + p1[0]) * c;
        cy += (p0[1] + p1[1]) * c;
    }
    if a.abs() < 1e-9 {
        let m = n.max(1) as f32;
        return [
            q.iter().map(|p| p[0]).sum::<f32>() / m,
            q.iter().map(|p| p[1]).sum::<f32>() / m,
        ];
    }
    [cx / (3.0 * a), cy / (3.0 * a)]
}

/// The closed outline resampled to points at most `step` apart (every input corner kept),
/// 8…32 points.
fn resample(q: &[[f32; 2]], step: f32) -> Vec<[f32; 2]> {
    let n = q.len();
    let per: f32 = (0..n)
        .map(|i| {
            let (a, c) = (q[i], q[(i + 1) % n]);
            ((c[0] - a[0]).powi(2) + (c[1] - a[1]).powi(2)).sqrt()
        })
        .sum();
    let target = (per / step).ceil().clamp(8.0, 32.0);
    let step = per / target;
    let mut out = Vec::new();
    for i in 0..n {
        let (a, c) = (q[i], q[(i + 1) % n]);
        let l = ((c[0] - a[0]).powi(2) + (c[1] - a[1]).powi(2)).sqrt();
        let k = ((l / step).round() as usize).max(1);
        for j in 0..k {
            let t = j as f32 / k as f32;
            out.push([a[0] + (c[0] - a[0]) * t, a[1] + (c[1] - a[1]) * t]);
        }
    }
    out
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt();
    if l < 1e-9 {
        [0.0, 0.0, 1.0]
    } else {
        scale(a, 1.0 / l)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_keeps_both_sides_apart_by_the_seam() {
        let sq = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let (lo, hi) = split_outline(&sq, 0, 0.3);
        let max_lo = lo.iter().map(|q| q[0]).fold(f32::MIN, f32::max);
        let min_hi = hi.iter().map(|q| q[0]).fold(f32::MAX, f32::min);
        assert!((max_lo - (0.3 - 0.5 * INDICATOR_SEAM_M)).abs() < 1e-6);
        assert!((min_hi - (0.3 + 0.5 * INDICATOR_SEAM_M)).abs() < 1e-6);
        assert_eq!(lo.len(), 4);
        assert_eq!(hi.len(), 4);
        // A cut outside the outline keeps it whole on one side.
        assert_eq!(clip_outline(&sq, 1, 2.0, true).len(), 4);
        assert!(clip_outline(&sq, 1, 2.0, false).is_empty());
    }

    #[test]
    fn resample_and_centroid() {
        let e = ellipse([0.5, -0.2], [0.2, 0.1], 12);
        let c = centroid(&e);
        assert!((c[0] - 0.5).abs() < 1e-4 && (c[1] + 0.2).abs() < 1e-4);
        let r = resample(&e, 0.01);
        assert!((8..=32).contains(&r.len()), "{}", r.len());
        let r = resample(&[[0.0, 0.0], [0.01, 0.0], [0.0, 0.01]], 0.04);
        assert!(r.len() >= 3 && r.len() <= 8, "{}", r.len());
    }

    #[test]
    fn a_lens_sits_proud_of_the_body_it_is_cast_on() {
        let root = crate::repo_root();
        let car = crate::load_car(&root, "kestrel_ts").unwrap();
        let st = crate::style::style_for(&car.def);
        let p = BodyProfile::styled(&car.def, &car.lattice, &st.body);
        let mut b = Builder::new();
        // A box "body" standing in for the loft: its front face at y_nose.
        let body = PartKey::body(
            "hood",
            wp_contracts::BodyRegion::Hood,
            PartKind::Panel,
            crate::mesh::MaterialSlot::Livery,
        );
        b.cuboid(&body, [-0.8, p.y_nose - 1.0, 0.2], [0.8, p.y_nose, 0.8]);
        let key = PartKey::body(
            "headlight_l",
            wp_contracts::BodyRegion::FrameFront,
            PartKind::Light,
            crate::mesh::MaterialSlot::Headlight,
        );
        let lamp = Lamp::new(LampView::Front, ellipse([-0.5, 0.6], [0.12, 0.05], 12));
        lens(&mut b, &p, &key, &lamp);
        let (pos, _, idx) = b.geometry("headlight_l").unwrap();
        assert!(idx.len() >= 3 * 12);
        let ymax = pos.iter().map(|q| q[1]).fold(f32::MIN, f32::max);
        let ymin = pos.iter().map(|q| q[1]).fold(f32::MAX, f32::min);
        assert!((ymax - (p.y_nose + 0.008)).abs() < 1e-4, "{ymax}");
        assert!(ymin >= p.y_nose - 1e-4, "{ymin}");
        let xmin = pos.iter().map(|q| q[0]).fold(f32::MAX, f32::min);
        assert!((xmin + 0.62).abs() < 0.01, "{xmin}");
    }
}
