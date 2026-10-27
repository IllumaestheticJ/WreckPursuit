//! Lamps projected onto the body surface (car-shapes phase 3).
//!
//! A [`Lamp`] is a 2D outline in the coordinates of a view (the nose seen from ahead, the tail
//! from behind, a side, or the top). Every point of the outline is cast along the view
//! direction onto the loft exactly as the mesh is built (same stations, the rounded end rings
//! included; see [`crate::mesh::loft_rings`]), so the lens follows the curved nose, fender or
//! tail: a lens drawn in the front view near the corner wraps round the plan-rounded corner
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
use crate::mesh::{loft_rings, INDICATOR_SEAM_M};
use crate::profile::{BodyProfile, RingPt};

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
    /// Lens back under the surface, m (0…0.03). Default 0.010.
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
            depth_m: 0.010,
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

/// The lens of `lamp` into part `key` (a closed shell following the surface).
pub fn lens(b: &mut Builder, p: &BodyProfile, key: &PartKey, lamp: &Lamp) {
    let Some(caster) = Caster::new(p, lamp) else {
        return;
    };
    let grid = caster.grid(lamp);
    let proud = lamp.proud_m.clamp(0.002, 0.03);
    let depth = lamp.depth_m.clamp(0.0, 0.03);
    shell(b, key, &grid, proud, -depth);
}

/// A trim bezel `width_m` wide (0.003…0.05) round `lamp`'s outline into part `key`: a ring
/// from the lens edge at half the lens's proud height down to the surface, with a skirt into
/// the body. Draw the lens with [`lens`] (same `lamp`) on top of it.
pub fn bezel(b: &mut Builder, p: &BodyProfile, key: &PartKey, lamp: &Lamp, width_m: f32) {
    let Some(caster) = Caster::new(p, lamp) else {
        return;
    };
    let w = width_m.clamp(0.003, 0.05);
    let proud = lamp.proud_m.clamp(0.002, 0.03);
    let depth = lamp.depth_m.clamp(0.0, 0.03);
    let ring_in = caster.boundary(lamp, 0.0);
    let ring_out = caster.boundary(lamp, w);
    let centre = caster.centre;
    let n = ring_in.len();
    for i in 0..n {
        let k = (i + 1) % n;
        let (a, c) = (ring_in[i], ring_in[k]);
        let (d, e) = (ring_out[i], ring_out[k]);
        let ai = add(a.0, scale(a.1, 0.5 * proud));
        let ci = add(c.0, scale(c.1, 0.5 * proud));
        let dout = add(d.0, scale(d.1, 0.002));
        let eout = add(e.0, scale(e.1, 0.002));
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

/// Casts view points onto the loft.
struct Caster {
    ys: Vec<f32>,
    rings: Vec<Vec<RingPt>>,
    /// Cast direction (unit).
    dir: [f32; 3],
    /// View axes: first and second outline coordinate.
    u: [f32; 3],
    v: [f32; 3],
    /// Start of a cast for outline point (0, 0): far outside the body.
    origin: [f32; 3],
    /// Longest cast, m.
    reach: f32,
    /// 2D centroid of the outline.
    c2: [f32; 2],
    /// Surface point of the centroid.
    centre: [f32; 3],
}

impl Caster {
    fn new(p: &BodyProfile, lamp: &Lamp) -> Option<Caster> {
        if lamp.outline.len() < 3 {
            return None;
        }
        let (ys, rings) = loft_rings(p);
        if ys.len() < 2 {
            return None;
        }
        let c2 = centroid(&lamp.outline);
        let reach = (p.y_nose - p.y_tail) + 2.0 * p.half_w + 2.0;
        let yaw = lamp.yaw_deg.clamp(0.0, 60.0).to_radians();
        let (sy, cy) = wp_math::sin_cos(yaw);
        let side = if c2[0] < 0.0 { -1.0 } else { 1.0 };
        let (dir, u, v, origin) = match lamp.view {
            LampView::Front => {
                // From ahead (and, with yaw, from the outline's side), travelling in.
                let dir = [-side * sy, -cy, 0.0];
                let u = horizontal_across(dir, [1.0, 0.0, 0.0]);
                (dir, u, [0.0, 0.0, 1.0], [0.0, p.y_nose + 1.0, 0.0])
            }
            LampView::Rear => {
                let dir = [-side * sy, cy, 0.0];
                let u = horizontal_across(dir, [1.0, 0.0, 0.0]);
                (dir, u, [0.0, 0.0, 1.0], [0.0, p.y_tail - 1.0, 0.0])
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
        // Yawed front / rear views: start far back along the swung ray so the cast plane
        // passes through the face.
        let origin = match lamp.view {
            LampView::Front | LampView::Rear => {
                let face = if lamp.view == LampView::Front {
                    p.y_nose
                } else {
                    p.y_tail
                };
                sub([0.0, face, 0.0], scale(dir, 1.5))
            }
            _ => origin,
        };
        let mut caster = Caster {
            ys,
            rings,
            dir,
            u,
            v,
            origin,
            reach: reach.max(3.5),
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

    /// Is `q` inside the loft? Rings are interpolated linearly between stations (the loft's
    /// faces), the left half mirrored.
    fn inside(&self, q: [f32; 3]) -> bool {
        let y = q[1];
        let n = self.ys.len();
        if y < self.ys[0] || y > self.ys[n - 1] {
            return false;
        }
        let s = match self.ys.iter().position(|&ys| ys >= y) {
            Some(0) => 0,
            Some(i) => i - 1,
            None => n - 2,
        };
        let (y0, y1) = (self.ys[s], self.ys[s + 1]);
        let t = if y1 - y0 > 1e-6 {
            ((y - y0) / (y1 - y0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (ra, rb) = (&self.rings[s], &self.rings[s + 1]);
        let pt = |j: usize| -> [f32; 2] {
            let (a, c) = (ra[j].p, rb[j].p);
            [-(a[0] + (c[0] - a[0]) * t), a[2] + (c[2] - a[2]) * t]
        };
        // Polygon in (|x|, z): centre line at the underside, the ring, centre line at the top.
        let ax = q[0].abs();
        let z = q[2];
        let m = ra.len();
        let mut poly: Vec<[f32; 2]> = Vec::with_capacity(m + 1);
        poly.push([0.0, pt(0)[1]]);
        for j in 0..m {
            poly.push(pt(j));
        }
        let mut inside = false;
        let k = poly.len();
        for i in 0..k {
            let a = poly[i];
            let c = poly[(i + 1) % k];
            if (a[1] > z) != (c[1] > z) {
                let x = a[0] + (z - a[1]) / (c[1] - a[1]) * (c[0] - a[0]);
                if ax < x {
                    inside = !inside;
                }
            }
        }
        inside
    }

    /// First surface point along the cast of view point `q`, or None.
    fn cast(&self, q: [f32; 2]) -> Option<[f32; 3]> {
        let o = self.start(q);
        let step = 0.01;
        let mut t = 0.0;
        let mut prev = 0.0;
        while t <= self.reach {
            if self.inside(add(o, scale(self.dir, t))) {
                let (mut lo, mut hi) = (prev, t);
                for _ in 0..14 {
                    let mid = 0.5 * (lo + hi);
                    if self.inside(add(o, scale(self.dir, mid))) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                return Some(add(o, scale(self.dir, hi)));
            }
            prev = t;
            t += step;
        }
        None
    }

    /// Surface point and normal of view point `q`, pulled toward `toward` (a point known to
    /// hit) until the cast hits.
    fn cast_or_pull(&self, q: [f32; 2], toward: [f32; 2]) -> Hit {
        let mut f = 1.0f32;
        let mut q2 = q;
        let mut hit = self.cast(q2);
        while hit.is_none() && f > 0.02 {
            f -= 0.08;
            q2 = [
                toward[0] + (q[0] - toward[0]) * f,
                toward[1] + (q[1] - toward[1]) * f,
            ];
            hit = self.cast(q2);
        }
        let h = hit.unwrap_or_else(|| self.start(q));
        (h, self.normal(q2, h))
    }

    /// Outward normal at the surface point `h` of view point `q`, from neighbouring casts.
    fn normal(&self, q: [f32; 2], h: [f32; 3]) -> [f32; 3] {
        let e = 0.006;
        let at = |du: f32, dv: f32| self.cast([q[0] + du, q[1] + dv]);
        let back = scale(self.dir, -1.0);
        let pu = at(e, 0.0).map(|x| sub(x, h));
        let mu = at(-e, 0.0).map(|x| sub(h, x));
        let pv = at(0.0, e).map(|x| sub(x, h));
        let mv = at(0.0, -e).map(|x| sub(h, x));
        let du = pu.or(mu);
        let dv = pv.or(mv);
        match (du, dv) {
            (Some(a), Some(c)) => {
                let n = norm(cross(a, c));
                // Grazing casts (a corner seen edge-on) give wild normals: keep them on the
                // viewer's side.
                if dot(n, back) < 0.0 {
                    scale(n, -1.0)
                } else {
                    n
                }
            }
            _ => back,
        }
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
/// (under the surface; the back is inside the body and left open).
fn shell(b: &mut Builder, key: &PartKey, grid: &[Vec<Hit>], hi: f32, lo: f32) {
    let off = |h: &Hit, d: f32| add(h.0, scale(h.1, d));
    let c = &grid[0][0];
    let n = grid.get(1).map_or(0, |r| r.len());
    if n < 3 {
        return;
    }
    // The lens back is inside the body and never seen: only the face and the rim wall.
    {
        let d = hi;
        let face = |b: &mut Builder, q: &[[f32; 3]], nrm: [f32; 3]| {
            b.poly_facing(key, q, nrm);
        };
        let r1 = &grid[1];
        for i in 0..n {
            let k = (i + 1) % n;
            let nrm = norm(add(add(c.1, r1[i].1), r1[k].1));
            face(b, &[off(c, d), off(&r1[i], d), off(&r1[k], d)], nrm);
        }
        for r in 1..grid.len() - 1 {
            let (a, e) = (&grid[r], &grid[r + 1]);
            for i in 0..n {
                let k = (i + 1) % n;
                let nrm = norm(add(add(a[i].1, a[k].1), add(e[i].1, e[k].1)));
                face(
                    b,
                    &[off(&a[i], d), off(&e[i], d), off(&e[k], d), off(&a[k], d)],
                    nrm,
                );
            }
        }
    }
    // Rim wall round the outer ring.
    let last = &grid[grid.len() - 1];
    for i in 0..n {
        let k = (i + 1) % n;
        let q = [
            off(&last[i], lo),
            off(&last[k], lo),
            off(&last[k], hi),
            off(&last[i], hi),
        ];
        let mid = scale(add(add(q[0], q[1]), add(q[2], q[3])), 0.25);
        b.poly_facing(key, &q, sub(mid, c.0));
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
