//! Ambient-occlusion bake for the detail painter: a median-split BVH over the car's rest-pose
//! triangles (wheels at their hubs) and short cosine-weighted rays from texel positions.
//! Deterministic (fixed ray set, rotated per texel by an integer hash).

/// A triangle (chassis space, m).
pub type Tri = [[f32; 3]; 3];

#[derive(Debug, Clone)]
struct Node {
    lo: [f32; 3],
    hi: [f32; 3],
    /// Leaf: first triangle index and count; inner: left child index (right = left + 1), count 0.
    first: u32,
    count: u32,
}

/// Bounding-volume hierarchy over triangles.
#[derive(Debug, Clone)]
pub struct Bvh {
    tris: Vec<Tri>,
    nodes: Vec<Node>,
}

fn bounds(ts: &[Tri]) -> ([f32; 3], [f32; 3]) {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for t in ts {
        for v in t {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
    }
    (lo, hi)
}

fn centroid(t: &Tri) -> [f32; 3] {
    [
        (t[0][0] + t[1][0] + t[2][0]) / 3.0,
        (t[0][1] + t[1][1] + t[2][1]) / 3.0,
        (t[0][2] + t[1][2] + t[2][2]) / 3.0,
    ]
}

impl Bvh {
    /// Build over `tris` (leaves of ≤ 4 triangles, split at the median of the longest axis).
    pub fn new(mut tris: Vec<Tri>) -> Bvh {
        let mut nodes = Vec::new();
        let n = tris.len();
        let (lo, hi) = bounds(&tris);
        nodes.push(Node {
            lo,
            hi,
            first: 0,
            count: n as u32,
        });
        let mut stack = vec![0usize];
        while let Some(i) = stack.pop() {
            let (first, count) = (nodes[i].first as usize, nodes[i].count as usize);
            if count <= 4 {
                continue;
            }
            let (lo, hi) = (nodes[i].lo, nodes[i].hi);
            let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
            let axis = if ext[0] >= ext[1] && ext[0] >= ext[2] {
                0
            } else if ext[1] >= ext[2] {
                1
            } else {
                2
            };
            let slice = &mut tris[first..first + count];
            slice.sort_by(|a, b| centroid(a)[axis].total_cmp(&centroid(b)[axis]));
            let half = count / 2;
            let left = nodes.len();
            for (f, c) in [(first, half), (first + half, count - half)] {
                let (l, h) = bounds(&tris[f..f + c]);
                nodes.push(Node {
                    lo: l,
                    hi: h,
                    first: f as u32,
                    count: c as u32,
                });
            }
            nodes[i].first = left as u32;
            nodes[i].count = 0;
            stack.push(left);
            stack.push(left + 1);
        }
        Bvh { tris, nodes }
    }

    /// Distance to the nearest hit of the ray `o + t·d` with `t` in (`t0`, `t1`), if any.
    pub fn hit(&self, o: [f32; 3], d: [f32; 3], t0: f32, t1: f32) -> Option<f32> {
        if self.tris.is_empty() {
            return None;
        }
        let inv = [1.0 / d[0], 1.0 / d[1], 1.0 / d[2]];
        let mut best = t1;
        let mut found = false;
        let mut stack = vec![0usize];
        while let Some(i) = stack.pop() {
            let nd = &self.nodes[i];
            // Slab test.
            let mut tmin = t0;
            let mut tmax = best;
            for k in 0..3 {
                let a = (nd.lo[k] - o[k]) * inv[k];
                let b = (nd.hi[k] - o[k]) * inv[k];
                let (a, b) = if a < b { (a, b) } else { (b, a) };
                tmin = tmin.max(a);
                tmax = tmax.min(b);
            }
            if tmin > tmax {
                continue;
            }
            if nd.count == 0 {
                stack.push(nd.first as usize);
                stack.push(nd.first as usize + 1);
                continue;
            }
            for t in &self.tris[nd.first as usize..(nd.first + nd.count) as usize] {
                if let Some(h) = ray_tri(o, d, t) {
                    if h > t0 && h < best {
                        best = h;
                        found = true;
                    }
                }
            }
        }
        found.then_some(best)
    }
}

/// Möller–Trumbore ray / triangle distance (two-sided).
fn ray_tri(o: [f32; 3], d: [f32; 3], t: &Tri) -> Option<f32> {
    let e1 = sub(t[1], t[0]);
    let e2 = sub(t[2], t[0]);
    let p = cross(d, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-10 {
        return None;
    }
    let inv = 1.0 / det;
    let s = sub(o, t[0]);
    let u = dot(s, p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let v = dot(d, q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    Some(dot(e2, q) * inv)
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
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

/// Rays per AO sample.
pub const AO_RAYS: usize = 16;
/// Longest AO ray, m.
pub const AO_RANGE_M: f32 = 0.30;

/// Ambient occlusion 0 (fully occluded) … 1 (open) at `p` with unit normal `n`: [`AO_RAYS`]
/// cosine-weighted rays (Hammersley set rotated by `seed`) up to [`AO_RANGE_M`]; a hit at
/// distance t counts 1 − t / range. Rays start 4 mm off the surface.
pub fn ao(bvh: &Bvh, p: [f32; 3], n: [f32; 3], seed: f32) -> f32 {
    let (t, b) = crate::builder::basis(n);
    let o = [
        p[0] + n[0] * 0.004,
        p[1] + n[1] * 0.004,
        p[2] + n[2] * 0.004,
    ];
    let mut occ = 0.0;
    for i in 0..AO_RAYS {
        let u1 = (i as f32 + 0.5) / AO_RAYS as f32;
        let mut u2 = radical_inverse(i as u32) + seed;
        u2 -= u2.floor();
        let r = u1.sqrt();
        let (s, c) = wp_math::sin_cos(std::f32::consts::TAU * u2);
        let (x, y, z) = (r * c, r * s, (1.0 - u1).max(0.0).sqrt());
        let d = [
            t[0] * x + b[0] * y + n[0] * z,
            t[1] * x + b[1] * y + n[1] * z,
            t[2] * x + b[2] * y + n[2] * z,
        ];
        if let Some(h) = bvh.hit(o, d, 1e-4, AO_RANGE_M) {
            occ += 1.0 - h / AO_RANGE_M;
        }
    }
    1.0 - occ / AO_RAYS as f32
}

fn radical_inverse(mut i: u32) -> f32 {
    i = i.rotate_right(16);
    i = ((i & 0x5555_5555) << 1) | ((i & 0xAAAA_AAAA) >> 1);
    i = ((i & 0x3333_3333) << 2) | ((i & 0xCCCC_CCCC) >> 2);
    i = ((i & 0x0F0F_0F0F) << 4) | ((i & 0xF0F0_F0F0) >> 4);
    i = ((i & 0x00FF_00FF) << 8) | ((i & 0xFF00_FF00) >> 8);
    i as f32 / 4_294_967_296.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_plane_is_unoccluded_and_a_corner_is_dark() {
        let floor: Vec<Tri> = vec![
            [[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0]],
            [[-1.0, -1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0]],
        ];
        let bvh = Bvh::new(floor.clone());
        assert!(ao(&bvh, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 0.3) > 0.99);
        let mut walls = floor;
        walls.push([[0.05, -1.0, 0.0], [0.05, 1.0, 0.0], [0.05, 1.0, 1.0]]);
        walls.push([[0.05, -1.0, 0.0], [0.05, 1.0, 1.0], [0.05, -1.0, 1.0]]);
        let bvh = Bvh::new(walls);
        assert!(ao(&bvh, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 0.3) < 0.85);
    }
}
