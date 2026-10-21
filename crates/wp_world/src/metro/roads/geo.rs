//! 2D polyline helpers for the road generator (ported from Port Vesper v2 and extended):
//! arc lengths, resampling, projection, sub-paths, fillets, Catmull-Rom curves, offsets,
//! segment crossings and a uniform segment grid for fast crossing queries.
//!
//! Everything is plain `f32` with `wp_math` trigonometry, so results are bit-identical on
//! native and wasm.

use crate::geom::{self, V2};

/// Cumulative arc length of a 2D polyline (first element 0).
pub fn cum2(p: &[V2]) -> Vec<f32> {
    let mut out = Vec::with_capacity(p.len());
    let mut acc = 0.0;
    out.push(0.0);
    for w in p.windows(2) {
        acc += geom::dist(w[0], w[1]);
        out.push(acc);
    }
    out
}

/// Total length of a 2D polyline, m.
pub fn length(p: &[V2]) -> f32 {
    p.windows(2).map(|w| geom::dist(w[0], w[1])).sum()
}

/// Point and unit tangent at arc length `s` (clamped).
pub fn point_on(p: &[V2], cum: &[f32], s: f32) -> (V2, V2) {
    let n = p.len();
    let s = s.clamp(0.0, cum[n - 1]);
    let i = match cum.binary_search_by(|c| c.total_cmp(&s)) {
        Ok(i) => i.min(n - 2),
        Err(i) => i.saturating_sub(1).min(n - 2),
    };
    let l = (cum[i + 1] - cum[i]).max(1e-6);
    let t = (s - cum[i]) / l;
    let d = geom::normalize(geom::sub(p[i + 1], p[i]));
    (
        geom::add(p[i], geom::scale(geom::sub(p[i + 1], p[i]), t)),
        d,
    )
}

/// Uniform resampling at about `step` metres, keeping both ends.
pub fn resample(p: &[V2], step: f32) -> Vec<V2> {
    let cum = cum2(p);
    let total = cum[cum.len() - 1];
    let n = ((total / step).ceil() as usize).max(1);
    (0..=n)
        .map(|k| point_on(p, &cum, total * k as f32 / n as f32).0)
        .collect()
}

/// Closest point on a polyline: (arc length, distance).
pub fn project2(p: &[V2], cum: &[f32], q: V2) -> (f32, f32) {
    let mut best = (0.0, f32::INFINITY);
    for i in 0..p.len() - 1 {
        let ab = geom::sub(p[i + 1], p[i]);
        let l2 = geom::dot(ab, ab).max(1e-12);
        let t = (geom::dot(geom::sub(q, p[i]), ab) / l2).clamp(0.0, 1.0);
        let c = geom::add(p[i], geom::scale(ab, t));
        let d = geom::dist(q, c);
        if d < best.1 {
            best = (cum[i] + t * (cum[i + 1] - cum[i]), d);
        }
    }
    best
}

/// Closest point with a signed lateral offset (+ = right of travel): (s, lateral, distance).
pub fn project_signed(p: &[V2], cum: &[f32], q: V2) -> (f32, f32, f32) {
    let mut best = (0.0, 0.0, f32::INFINITY);
    for i in 0..p.len() - 1 {
        let ab = geom::sub(p[i + 1], p[i]);
        let l2 = geom::dot(ab, ab).max(1e-12);
        let t = (geom::dot(geom::sub(q, p[i]), ab) / l2).clamp(0.0, 1.0);
        let c = geom::add(p[i], geom::scale(ab, t));
        let d = geom::dist(q, c);
        if d < best.2 {
            let dir = geom::normalize(ab);
            let lat = geom::dot(geom::sub(q, p[i]), geom::right_of(dir));
            best = (cum[i] + t * (cum[i + 1] - cum[i]), lat, d);
        }
    }
    best
}

/// Distance from `q` to a polyline.
pub fn dist_to(p: &[V2], q: V2) -> f32 {
    p.windows(2)
        .map(|w| geom::point_segment_distance(q, w[0], w[1]))
        .fold(f32::INFINITY, f32::min)
}

/// Sub-path between arc lengths `s0 < s1` (interior points closer than 0.5 m to the ends
/// are dropped).
pub fn sub_path(p: &[V2], cum: &[f32], s0: f32, s1: f32) -> Vec<V2> {
    let mut out = vec![point_on(p, cum, s0).0];
    for (i, q) in p.iter().enumerate() {
        if cum[i] > s0 + 0.5 && cum[i] < s1 - 0.5 {
            out.push(*q);
        }
    }
    out.push(point_on(p, cum, s1).0);
    out
}

/// Catmull-Rom curve through `ctrl`, sampled about every `step` metres.
pub fn catmull_rom(ctrl: &[V2], step: f32) -> Vec<V2> {
    let n = ctrl.len();
    if n < 3 {
        return resample(ctrl, step);
    }
    let get = |i: isize| -> V2 {
        if i < 0 {
            geom::sub(geom::scale(ctrl[0], 2.0), ctrl[1])
        } else if i as usize >= n {
            geom::sub(geom::scale(ctrl[n - 1], 2.0), ctrl[n - 2])
        } else {
            ctrl[i as usize]
        }
    };
    let mut out = vec![ctrl[0]];
    for i in 0..n - 1 {
        let (p0, p1, p2, p3) = (
            get(i as isize - 1),
            get(i as isize),
            get(i as isize + 1),
            get(i as isize + 2),
        );
        let m = ((geom::dist(p1, p2) / step).ceil() as usize).max(1);
        for k in 1..=m {
            let t = k as f32 / m as f32;
            let (t2, t3) = (t * t, t * t * t);
            let c = |a: f32, b: f32, c: f32, d: f32| {
                0.5 * (2.0 * b
                    + (-a + c) * t
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
                    + (-a + 3.0 * b - 3.0 * c + d) * t3)
            };
            out.push([c(p0[0], p1[0], p2[0], p3[0]), c(p0[1], p1[1], p2[1], p3[1])]);
        }
    }
    out
}

/// Round the corners of a polyline (closed when `closed`) with circular arcs of radius
/// `radius[i]` at vertex `i` (limited so arcs never overlap), sampled every `step` metres.
pub fn fillet(ctrl: &[V2], radius: &[f32], closed: bool, step: f32) -> Vec<V2> {
    let n = ctrl.len();
    let mut out: Vec<V2> = Vec::new();
    if !closed {
        out.push(ctrl[0]);
    }
    let range: Vec<usize> = if closed {
        (0..n).collect()
    } else {
        (1..n - 1).collect()
    };
    for i in range {
        let prev = ctrl[(i + n - 1) % n];
        let next = ctrl[(i + 1) % n];
        let c = ctrl[i];
        let (l1, l2) = (geom::dist(prev, c), geom::dist(c, next));
        let d1 = geom::normalize(geom::sub(c, prev));
        let d2 = geom::normalize(geom::sub(next, c));
        let theta = geom::atan2(geom::cross(d1, d2), geom::dot(d1, d2));
        let half = theta.abs() * 0.5;
        if half < 1e-3 || radius[i] <= 0.0 {
            out.push(c);
            continue;
        }
        let (s, co) = wp_math::sin_cos(half);
        let tan_half = s / co;
        // Arcs at both ends of an edge share it (the open ends are not filleted).
        let share1 = if closed || i > 1 { 0.49 } else { 0.98 };
        let share2 = if closed || i + 2 < n { 0.49 } else { 0.98 };
        let t = (radius[i] * tan_half).min(share1 * l1).min(share2 * l2);
        let r = t / tan_half;
        let a = geom::sub(c, geom::scale(d1, t));
        let left = [-d1[1], d1[0]];
        let sign = if theta > 0.0 { 1.0 } else { -1.0 };
        let centre = geom::add(a, geom::scale(left, r * sign));
        let arm = geom::sub(a, centre);
        let m = ((r * theta.abs() / step).ceil() as usize).max(2);
        for k in 0..=m {
            let (sn, cs) = wp_math::sin_cos(theta * k as f32 / m as f32);
            out.push(geom::add(
                centre,
                [arm[0] * cs - arm[1] * sn, arm[0] * sn + arm[1] * cs],
            ));
        }
    }
    if !closed {
        out.push(ctrl[n - 1]);
    }
    out
}

/// Gentle curve from `a` to `b` bulging `bulge` metres to the left at the middle.
pub fn bent(a: V2, b: V2, bulge: f32, step: f32) -> Vec<V2> {
    let d = geom::normalize(geom::sub(b, a));
    let mid = geom::scale(geom::add(a, b), 0.5);
    let ctrl = geom::add(mid, geom::scale([-d[1], d[0]], 2.0 * bulge));
    let n = ((geom::dist(a, b) / step).ceil() as usize).max(2);
    (0..=n)
        .map(|k| {
            let t = k as f32 / n as f32;
            let u = 1.0 - t;
            [
                u * u * a[0] + 2.0 * u * t * ctrl[0] + t * t * b[0],
                u * u * a[1] + 2.0 * u * t * ctrl[1] + t * t * b[1],
            ]
        })
        .collect()
}

/// Cubic Bézier in 2D sampled into `n + 1` points.
pub fn bezier2(p0: V2, p1: V2, p2: V2, p3: V2, n: usize) -> Vec<V2> {
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let (b0, b1, b2, b3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            [
                b0 * p0[0] + b1 * p1[0] + b2 * p2[0] + b3 * p3[0],
                b0 * p0[1] + b1 * p1[1] + b2 * p2[1] + b3 * p3[1],
            ]
        })
        .collect()
}

/// Smooth connection from pose `(a, da)` to pose `(b, db)` (unit directions of travel):
/// a cubic Bézier with handles `k * chord` long.
pub fn hermite(a: V2, da: V2, b: V2, db: V2, k: f32, step: f32) -> Vec<V2> {
    let chord = geom::dist(a, b);
    let h = k * chord;
    let n = ((chord * 1.3 / step).ceil() as usize).max(4);
    bezier2(
        a,
        geom::add(a, geom::scale(da, h)),
        geom::sub(b, geom::scale(db, h)),
        b,
        n,
    )
}

/// Intersection of segments `a0a1` and `b0b1` (parameters along each), if any.
pub fn seg_cross(a0: V2, a1: V2, b0: V2, b1: V2) -> Option<(f32, f32)> {
    let r = geom::sub(a1, a0);
    let s = geom::sub(b1, b0);
    let den = geom::cross(r, s);
    if den.abs() < 1e-9 {
        return None;
    }
    let w = geom::sub(b0, a0);
    let t = geom::cross(w, s) / den;
    let u = geom::cross(w, r) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((t, u))
}

/// Every crossing of two paths: (s on a, s on b, point). Shared end points (closer than
/// 1 m) are ignored.
pub fn path_crossings(a: &[V2], b: &[V2]) -> Vec<(f32, f32, V2)> {
    let (ca, cb) = (cum2(a), cum2(b));
    let (alo, ahi) = bbox(a);
    let (blo, bhi) = bbox(b);
    if alo[0] > bhi[0] || blo[0] > ahi[0] || alo[1] > bhi[1] || blo[1] > ahi[1] {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0..a.len() - 1 {
        let (lo, hi) = bbox(&a[i..i + 2]);
        for j in 0..b.len() - 1 {
            let (l2, h2) = bbox(&b[j..j + 2]);
            if lo[0] > h2[0] || l2[0] > hi[0] || lo[1] > h2[1] || l2[1] > hi[1] {
                continue;
            }
            if let Some((t, u)) = seg_cross(a[i], a[i + 1], b[j], b[j + 1]) {
                let p = geom::add(a[i], geom::scale(geom::sub(a[i + 1], a[i]), t));
                let near_end = [a[0], a[a.len() - 1]]
                    .iter()
                    .chain([b[0], b[b.len() - 1]].iter())
                    .any(|e| geom::dist(*e, p) < 1.0);
                if !near_end {
                    out.push((
                        ca[i] + t * (ca[i + 1] - ca[i]),
                        cb[j] + u * (cb[j + 1] - cb[j]),
                        p,
                    ));
                }
            }
        }
    }
    out
}

/// Axis-aligned bounds of a point list.
pub fn bbox(p: &[V2]) -> (V2, V2) {
    let mut lo = [f32::INFINITY; 2];
    let mut hi = [f32::NEG_INFINITY; 2];
    for q in p {
        lo = [lo[0].min(q[0]), lo[1].min(q[1])];
        hi = [hi[0].max(q[0]), hi[1].max(q[1])];
    }
    (lo, hi)
}

/// Signed curvature (1/m, + = left turn) at each point of a polyline.
pub fn curvature(p: &[V2]) -> Vec<f32> {
    let n = p.len();
    (0..n)
        .map(|i| {
            if i == 0 || i + 1 >= n {
                return 0.0;
            }
            let a = geom::sub(p[i], p[i - 1]);
            let b = geom::sub(p[i + 1], p[i]);
            let ang = geom::atan2(geom::cross(a, b), geom::dot(a, b));
            ang / (0.5 * (geom::len(a) + geom::len(b))).max(1e-3)
        })
        .collect()
}

/// Smallest radius of curvature along a polyline measured over chords of about `span`
/// metres (three points `span` apart), m.
pub fn min_radius(p: &[V2], span: f32) -> f32 {
    let cum = cum2(p);
    let total = cum[cum.len() - 1];
    let mut best = f32::INFINITY;
    let mut s = span;
    while s + span <= total {
        let (a, _) = point_on(p, &cum, s - span);
        let (b, _) = point_on(p, &cum, s);
        let (c, _) = point_on(p, &cum, s + span);
        // Circumradius of the triangle.
        let ab = geom::dist(a, b);
        let bc = geom::dist(b, c);
        let ca = geom::dist(c, a);
        let area2 = geom::cross(geom::sub(b, a), geom::sub(c, a)).abs();
        if area2 > 1e-6 {
            best = best.min(ab * bc * ca / (2.0 * area2));
        }
        s += span * 0.5;
    }
    best
}

/// Moving average over `half` points each side.
pub fn smooth_series(v: &[f32], half: usize) -> Vec<f32> {
    let n = v.len();
    if half == 0 || n == 0 {
        return v.to_vec();
    }
    // Prefix sums keep this linear for long series.
    let mut pre = Vec::with_capacity(n + 1);
    pre.push(0.0f32);
    for x in v {
        pre.push(pre[pre.len() - 1] + x);
    }
    (0..n)
        .map(|i| {
            let a = i.saturating_sub(half);
            let b = (i + half).min(n - 1);
            (pre[b + 1] - pre[a]) / (b - a + 1) as f32
        })
        .collect()
}

/// Offset a 2D polyline sideways by `off` metres (+ = right of travel), mitred.
pub fn offset2(p: &[V2], off: f32) -> Vec<V2> {
    let p3: Vec<[f32; 3]> = p.iter().map(|q| [q[0], q[1], 0.0]).collect();
    geom::offset_polyline(&p3, off)
        .iter()
        .map(|q| [q[0], q[1]])
        .collect()
}

/// Unit vector rotated left by 90°.
pub fn left_of(d: V2) -> V2 {
    [-d[1], d[0]]
}

/// Unit direction for a yaw angle in the map convention of `ctx::yaw_of` (0 = +Y, CCW).
pub fn dir_of(yaw: f32) -> V2 {
    let (s, c) = wp_math::sin_cos(yaw);
    [-s, c]
}

/// Uniform grid of polyline segments for crossing and proximity queries.
pub struct SegGrid {
    lo: V2,
    cell: f32,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<(u32, u32)>>,
}

impl SegGrid {
    /// Grid over `lo..hi` with `cell` metre cells.
    pub fn new(lo: V2, hi: V2, cell: f32) -> Self {
        let nx = ((hi[0] - lo[0]) / cell).ceil().max(1.0) as usize + 1;
        let ny = ((hi[1] - lo[1]) / cell).ceil().max(1.0) as usize + 1;
        Self {
            lo,
            cell,
            nx,
            ny,
            cells: vec![Vec::new(); nx * ny],
        }
    }

    fn range(&self, lo: V2, hi: V2) -> (usize, usize, usize, usize) {
        let ci = |v: f32, o: f32, n: usize| -> usize {
            (((v - o) / self.cell).floor().max(0.0) as usize).min(n - 1)
        };
        (
            ci(lo[0], self.lo[0], self.nx),
            ci(hi[0], self.lo[0], self.nx),
            ci(lo[1], self.lo[1], self.ny),
            ci(hi[1], self.lo[1], self.ny),
        )
    }

    /// Insert segment `seg` of polyline `line` (bounds grown by `pad`).
    pub fn insert(&mut self, line: u32, seg: u32, a: V2, b: V2, pad: f32) {
        let lo = [a[0].min(b[0]) - pad, a[1].min(b[1]) - pad];
        let hi = [a[0].max(b[0]) + pad, a[1].max(b[1]) + pad];
        let (i0, i1, j0, j1) = self.range(lo, hi);
        for j in j0..=j1 {
            for i in i0..=i1 {
                self.cells[j * self.nx + i].push((line, seg));
            }
        }
    }

    /// Insert every segment of a polyline.
    pub fn insert_line(&mut self, line: u32, p: &[V2], pad: f32) {
        for k in 0..p.len().saturating_sub(1) {
            self.insert(line, k as u32, p[k], p[k + 1], pad);
        }
    }

    /// Candidate `(line, segment)` pairs near the box `lo..hi` (sorted, deduplicated).
    pub fn query(&self, lo: V2, hi: V2) -> Vec<(u32, u32)> {
        let (i0, i1, j0, j1) = self.range(lo, hi);
        let mut out = Vec::new();
        for j in j0..=j1 {
            for i in i0..=i1 {
                out.extend_from_slice(&self.cells[j * self.nx + i]);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}
