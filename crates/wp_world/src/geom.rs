//! Small deterministic 2D/3D geometry helpers (polylines, Bézier connectors, OBB overlap,
//! polygon tests). Only IEEE `+ - * / sqrt` are used so results are bit-identical on native and
//! WASM.

/// 2D point / vector in world XY, metres.
pub type V2 = [f32; 2];
/// 3D point in world space, metres.
pub type V3 = [f32; 3];

/// `a - b`.
pub fn sub(a: V2, b: V2) -> V2 {
    [a[0] - b[0], a[1] - b[1]]
}

/// `a + b`.
pub fn add(a: V2, b: V2) -> V2 {
    [a[0] + b[0], a[1] + b[1]]
}

/// `a * k`.
pub fn scale(a: V2, k: f32) -> V2 {
    [a[0] * k, a[1] * k]
}

/// Dot product.
pub fn dot(a: V2, b: V2) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

/// Z component of the 3D cross product (positive when `b` is counter-clockwise of `a`).
pub fn cross(a: V2, b: V2) -> f32 {
    a[0] * b[1] - a[1] * b[0]
}

/// Euclidean length.
pub fn len(a: V2) -> f32 {
    dot(a, a).sqrt()
}

/// Distance between two points.
pub fn dist(a: V2, b: V2) -> f32 {
    len(sub(a, b))
}

/// Unit vector (or `[0, 1]` for a zero vector).
pub fn normalize(a: V2) -> V2 {
    let l = len(a);
    if l > 1e-9 {
        [a[0] / l, a[1] / l]
    } else {
        [0.0, 1.0]
    }
}

/// Right-hand normal of a direction in a +Z-up world (facing +Y, right is +X).
pub fn right_of(d: V2) -> V2 {
    [d[1], -d[0]]
}

/// XY part of a 3D point.
pub fn xy(p: V3) -> V2 {
    [p[0], p[1]]
}

/// Yaw (rad about +Z, 0 = facing +Y, positive = counter-clockwise) of a unit direction.
pub fn yaw_of(d: V2) -> f32 {
    atan2(-d[0], d[1])
}

/// Unit direction for a yaw (inverse of [`yaw_of`]).
pub fn dir_of_yaw(yaw: f32) -> V2 {
    let (s, c) = wp_math::sin_cos(yaw);
    [-s, c]
}

/// Deterministic `atan(x)` (max abs error ≈ 1e-5 rad).
pub fn atan(x: f32) -> f32 {
    let ax = x.abs();
    let (t, flip) = if ax > 1.0 {
        (1.0 / ax, true)
    } else {
        (ax, false)
    };
    let t2 = t * t;
    let p = t
        * (0.999_977_3
            + t2 * (-0.332_623_5
                + t2 * (0.193_543_5
                    + t2 * (-0.116_432_9 + t2 * (0.052_653_3 - t2 * 0.011_721_2)))));
    let r = if flip {
        std::f32::consts::FRAC_PI_2 - p
    } else {
        p
    };
    if x < 0.0 {
        -r
    } else {
        r
    }
}

/// Deterministic `atan2(y, x)`.
pub fn atan2(y: f32, x: f32) -> f32 {
    use std::f32::consts::{FRAC_PI_2, PI};
    if x > 0.0 {
        atan(y / x)
    } else if x < 0.0 {
        if y >= 0.0 {
            atan(y / x) + PI
        } else {
            atan(y / x) - PI
        }
    } else if y > 0.0 {
        FRAC_PI_2
    } else if y < 0.0 {
        -FRAC_PI_2
    } else {
        0.0
    }
}

/// Pseudo-angle in `[0, 4)` that orders directions counter-clockwise from +X like `atan2`
/// would, without any trigonometry. Used for deterministic angular sorting.
pub fn pseudo_angle(d: V2) -> f32 {
    let s = d[0].abs() + d[1].abs();
    if s <= 0.0 {
        return 0.0;
    }
    let p = d[1] / s; // -1..1
    if d[0] >= 0.0 {
        if p >= 0.0 {
            p
        } else {
            4.0 + p
        }
    } else {
        2.0 - p
    }
}

/// Cumulative arc length of a polyline (first element 0).
pub fn cumulative(pts: &[V3]) -> Vec<f32> {
    let mut out = Vec::with_capacity(pts.len());
    let mut acc = 0.0;
    out.push(0.0);
    for w in pts.windows(2) {
        acc += dist(xy(w[0]), xy(w[1]));
        out.push(acc);
    }
    out
}

/// Point and unit tangent at arc length `s` (clamped to the polyline; extrapolates linearly
/// past either end so callers can ask slightly beyond).
pub fn point_at(pts: &[V3], cum: &[f32], s: f32) -> (V3, V2) {
    let n = pts.len();
    debug_assert!(n >= 2);
    // Index of the segment containing s.
    let i = match cum.binary_search_by(|c| c.total_cmp(&s)) {
        Ok(i) => i.min(n - 2),
        Err(i) => i.saturating_sub(1).min(n - 2),
    };
    let a = pts[i];
    let b = pts[i + 1];
    let seg_len = cum[i + 1] - cum[i];
    let d = normalize(sub(xy(b), xy(a)));
    let t = if seg_len > 1e-6 {
        (s - cum[i]) / seg_len
    } else {
        0.0
    };
    let p = [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ];
    (p, d)
}

/// Result of projecting a point onto a polyline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection {
    /// Arc length of the closest point (may be < 0 or > length when beyond the ends).
    pub s: f32,
    /// Signed lateral offset, positive to the right of the travel direction.
    pub lateral: f32,
    /// Unsigned distance to the polyline (clamped to the ends).
    pub dist: f32,
}

/// Project `p` onto a polyline. Points beyond the end segments project onto the extended end
/// segments for `s` (so a car past a stop line gets `s > length`), while `dist` is measured to
/// the clamped polyline.
pub fn project(pts: &[V3], cum: &[f32], p: V2) -> Projection {
    let n = pts.len();
    let mut best = Projection {
        s: 0.0,
        lateral: 0.0,
        dist: f32::INFINITY,
    };
    for i in 0..n - 1 {
        let a = xy(pts[i]);
        let b = xy(pts[i + 1]);
        let ab = sub(b, a);
        let l2 = dot(ab, ab);
        if l2 < 1e-12 {
            continue;
        }
        let l = l2.sqrt();
        let t_raw = dot(sub(p, a), ab) / l2;
        let t = t_raw.clamp(0.0, 1.0);
        let c = add(a, scale(ab, t));
        let d = dist(p, c);
        if d < best.dist {
            let ext_t = if (i == 0 && t_raw < 0.0) || (i == n - 2 && t_raw > 1.0) {
                t_raw
            } else {
                t
            };
            let dir = scale(ab, 1.0 / l);
            best = Projection {
                s: cum[i] + ext_t * l,
                lateral: dot(sub(p, a), right_of(dir)),
                dist: d,
            };
        }
    }
    best
}

/// Trim `start` metres off the beginning and `end` metres off the end of a polyline.
/// Returns at least two points (degenerates to the midpoint pair when over-trimmed).
pub fn trim_polyline(pts: &[V3], start: f32, end: f32) -> Vec<V3> {
    let cum = cumulative(pts);
    let total = *cum.last().unwrap_or(&0.0);
    let (s0, s1) = if start + end < total - 0.5 {
        (start, total - end)
    } else {
        let mid = total * 0.5;
        (mid - 0.25, mid + 0.25)
    };
    let mut out = vec![point_at(pts, &cum, s0).0];
    for (i, p) in pts.iter().enumerate() {
        if cum[i] > s0 + 1e-3 && cum[i] < s1 - 1e-3 {
            out.push(*p);
        }
    }
    out.push(point_at(pts, &cum, s1).0);
    out
}

/// Offset a polyline sideways by `off` metres (positive = right of travel), with mitred joins.
pub fn offset_polyline(pts: &[V3], off: f32) -> Vec<V3> {
    let n = pts.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let d_prev = if i > 0 {
            Some(normalize(sub(xy(pts[i]), xy(pts[i - 1]))))
        } else {
            None
        };
        let d_next = if i + 1 < n {
            Some(normalize(sub(xy(pts[i + 1]), xy(pts[i]))))
        } else {
            None
        };
        let nrm = match (d_prev, d_next) {
            (Some(a), Some(b)) => {
                let m = normalize(add(right_of(a), right_of(b)));
                // Mitre length 1/cos(half angle), limited for very sharp corners.
                let c = dot(m, right_of(a)).max(0.5);
                scale(m, 1.0 / c)
            }
            (Some(a), None) => right_of(a),
            (None, Some(b)) => right_of(b),
            (None, None) => [0.0, 0.0],
        };
        out.push([
            pts[i][0] + nrm[0] * off,
            pts[i][1] + nrm[1] * off,
            pts[i][2],
        ]);
    }
    out
}

/// Sample a cubic Bézier into `n + 1` points.
pub fn bezier(p0: V3, p1: V3, p2: V3, p3: V3, n: usize) -> Vec<V3> {
    let mut out = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let u = 1.0 - t;
        let b0 = u * u * u;
        let b1 = 3.0 * u * u * t;
        let b2 = 3.0 * u * t * t;
        let b3 = t * t * t;
        out.push([
            b0 * p0[0] + b1 * p1[0] + b2 * p2[0] + b3 * p3[0],
            b0 * p0[1] + b1 * p1[1] + b2 * p2[1] + b3 * p3[1],
            b0 * p0[2] + b1 * p1[2] + b2 * p2[2] + b3 * p3[2],
        ]);
    }
    out
}

/// Minimum distance between two 2D segments.
pub fn segment_distance(a0: V2, a1: V2, b0: V2, b1: V2) -> f32 {
    if segments_intersect(a0, a1, b0, b1) {
        return 0.0;
    }
    point_segment_distance(a0, b0, b1)
        .min(point_segment_distance(a1, b0, b1))
        .min(point_segment_distance(b0, a0, a1))
        .min(point_segment_distance(b1, a0, a1))
}

/// Distance from `p` to segment `a`–`b`.
pub fn point_segment_distance(p: V2, a: V2, b: V2) -> f32 {
    let ab = sub(b, a);
    let l2 = dot(ab, ab);
    if l2 < 1e-12 {
        return dist(p, a);
    }
    let t = (dot(sub(p, a), ab) / l2).clamp(0.0, 1.0);
    dist(p, add(a, scale(ab, t)))
}

/// Proper or touching intersection test for two 2D segments.
pub fn segments_intersect(a0: V2, a1: V2, b0: V2, b1: V2) -> bool {
    let d1 = cross(sub(a1, a0), sub(b0, a0));
    let d2 = cross(sub(a1, a0), sub(b1, a0));
    let d3 = cross(sub(b1, b0), sub(a0, b0));
    let d4 = cross(sub(b1, b0), sub(a1, b0));
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

/// Minimum distance between two polylines (2D).
pub fn polyline_distance(a: &[V3], b: &[V3]) -> f32 {
    let mut best = f32::INFINITY;
    for wa in a.windows(2) {
        for wb in b.windows(2) {
            best = best.min(segment_distance(xy(wa[0]), xy(wa[1]), xy(wb[0]), xy(wb[1])));
        }
    }
    best
}

/// Whether two polylines come within `d` of each other (2D); stops at the first close pair.
pub fn polylines_within(a: &[V3], b: &[V3], d: f32) -> bool {
    for wa in a.windows(2) {
        let (a0, a1) = (xy(wa[0]), xy(wa[1]));
        let (alo, ahi) = (
            [a0[0].min(a1[0]) - d, a0[1].min(a1[1]) - d],
            [a0[0].max(a1[0]) + d, a0[1].max(a1[1]) + d],
        );
        for wb in b.windows(2) {
            let (b0, b1) = (xy(wb[0]), xy(wb[1]));
            if b0[0].max(b1[0]) < alo[0]
                || b0[0].min(b1[0]) > ahi[0]
                || b0[1].max(b1[1]) < alo[1]
                || b0[1].min(b1[1]) > ahi[1]
            {
                continue;
            }
            if segment_distance(a0, a1, b0, b1) < d {
                return true;
            }
        }
    }
    false
}

/// Even-odd point-in-polygon test.
pub fn point_in_polygon(p: V2, poly: &[V2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
            if p[0] < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Oriented 2D box: centre, unit forward axis, half length (along forward) and half width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Obb {
    /// Centre, m.
    pub c: V2,
    /// Unit forward axis.
    pub f: V2,
    /// Half extent along `f`, m.
    pub hl: f32,
    /// Half extent across `f`, m.
    pub hw: f32,
}

impl Obb {
    fn project_radius(&self, axis: V2) -> f32 {
        let r = right_of(self.f);
        self.hl * dot(self.f, axis).abs() + self.hw * dot(r, axis).abs()
    }
}

/// Separating-axis overlap test for two oriented boxes.
pub fn obb_overlap(a: &Obb, b: &Obb) -> bool {
    let d = sub(b.c, a.c);
    for axis in [a.f, right_of(a.f), b.f, right_of(b.f)] {
        if dot(d, axis).abs() > a.project_radius(axis) + b.project_radius(axis) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atan_matches_std() {
        let mut x = -50.0f32;
        while x < 50.0 {
            assert!((atan(x) - x.atan()).abs() < 2e-5, "{x}");
            x += 0.037;
        }
        for (y, x) in [
            (1.0, 1.0),
            (-1.0, -1.0),
            (1.0, -2.0),
            (-3.0, 0.5),
            (0.0, -1.0),
        ] {
            assert!((atan2(y, x) - f32::atan2(y, x)).abs() < 3e-5);
        }
    }

    #[test]
    fn yaw_round_trip() {
        for yaw in [-3.0f32, -1.0, 0.0, 0.5, 2.0, 3.1] {
            assert!((yaw_of(dir_of_yaw(yaw)) - yaw).abs() < 1e-4);
        }
        assert!((yaw_of([1.0, 0.0]) + std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    }

    #[test]
    fn pseudo_angle_orders_like_atan2() {
        let dirs: [V2; 6] = [
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
            [-1.0, 0.2],
            [-0.5, -1.0],
            [0.5, -1.0],
        ];
        for w in dirs.windows(2) {
            assert!(pseudo_angle(w[0]) < pseudo_angle(w[1]));
        }
    }

    #[test]
    fn projection_and_point_at() {
        let pts = vec![[0.0, 0.0, 0.0], [0.0, 10.0, 0.0], [10.0, 10.0, 0.0]];
        let cum = cumulative(&pts);
        assert_eq!(cum[2], 20.0);
        let (p, d) = point_at(&pts, &cum, 15.0);
        assert!((p[0] - 5.0).abs() < 1e-5 && (p[1] - 10.0).abs() < 1e-5);
        assert_eq!(d, [1.0, 0.0]);
        let pr = project(&pts, &cum, [1.0, 5.0]);
        assert!((pr.s - 5.0).abs() < 1e-5 && (pr.lateral - 1.0).abs() < 1e-5);
        // Beyond the end: s extends.
        let pr = project(&pts, &cum, [12.0, 10.0]);
        assert!((pr.s - 22.0).abs() < 1e-4);
    }

    #[test]
    fn offset_and_trim() {
        let pts = vec![[0.0, 0.0, 0.0], [0.0, 100.0, 0.0]];
        let o = offset_polyline(&pts, 2.0);
        assert_eq!(o[0][0], 2.0);
        let t = trim_polyline(&pts, 10.0, 20.0);
        assert_eq!(t.len(), 2);
        assert!((t[0][1] - 10.0).abs() < 1e-5 && (t[1][1] - 80.0).abs() < 1e-5);
    }

    #[test]
    fn obb_overlap_basic() {
        let a = Obb {
            c: [0.0, 0.0],
            f: [0.0, 1.0],
            hl: 2.25,
            hw: 0.95,
        };
        let mut b = a;
        b.c = [0.0, 4.0];
        assert!(obb_overlap(&a, &b));
        b.c = [0.0, 4.6];
        assert!(!obb_overlap(&a, &b));
        b.c = [3.5, 0.0];
        assert!(!obb_overlap(&a, &b));
    }

    #[test]
    fn polygon_contains() {
        let sq = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        assert!(point_in_polygon([5.0, 5.0], &sq));
        assert!(!point_in_polygon([15.0, 5.0], &sq));
    }
}
