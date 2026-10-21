//! Terrain: a regular heightfield stored with the road network, plus the deterministic noise
//! helpers the procedural generators use to build one.
//!
//! The heightfield is the ground *between* roads. Road surfaces themselves are exact ribbons
//! ([`crate::network::RoadSurface`]); generators carve the heightfield a few centimetres below
//! every at-grade road so the two never fight (see `crate::vesper`). Bridge decks are left
//! uncarved, so the ground under them stays at its natural height.
//!
//! Everything here is plain `f32` arithmetic (no transcendental functions), so it is
//! bit-identical on native and wasm.

use serde::{Deserialize, Serialize};
use wp_math::Rng;

use crate::geom::V2;

/// A regular grid of ground heights. Vertex `(i, j)` sits at
/// `origin + (i, j) * cell_m` and its height is `z[j * nx + i]`. Between vertices the
/// height is bilinear; outside the grid it is clamped to the edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Heightfield {
    /// World position of vertex (0, 0), m.
    pub origin: [f32; 2],
    /// Vertex spacing, m.
    pub cell_m: f32,
    /// Vertices along X.
    pub nx: u32,
    /// Vertices along Y.
    pub ny: u32,
    /// Heights, row-major (`j * nx + i`), m.
    pub z: Vec<f32>,
}

impl Heightfield {
    /// A flat field at height `z0` covering `lo..hi` (at least 2×2 vertices).
    pub fn flat(lo: V2, hi: V2, cell_m: f32, z0: f32) -> Self {
        let nx = (((hi[0] - lo[0]) / cell_m).ceil() as u32 + 1).max(2);
        let ny = (((hi[1] - lo[1]) / cell_m).ceil() as u32 + 1).max(2);
        Self {
            origin: lo,
            cell_m,
            nx,
            ny,
            z: vec![z0; (nx * ny) as usize],
        }
    }

    /// World position of vertex `(i, j)`.
    pub fn vertex_pos(&self, i: u32, j: u32) -> V2 {
        [
            self.origin[0] + i as f32 * self.cell_m,
            self.origin[1] + j as f32 * self.cell_m,
        ]
    }

    /// Height of vertex `(i, j)`.
    pub fn at(&self, i: u32, j: u32) -> f32 {
        self.z[(j * self.nx + i) as usize]
    }

    /// Upper corner of the covered area.
    pub fn max_corner(&self) -> V2 {
        self.vertex_pos(self.nx - 1, self.ny - 1)
    }

    /// Ground height at `p` (bilinear, clamped to the grid).
    pub fn height_at(&self, p: V2) -> f32 {
        let fx = ((p[0] - self.origin[0]) / self.cell_m).clamp(0.0, (self.nx - 1) as f32);
        let fy = ((p[1] - self.origin[1]) / self.cell_m).clamp(0.0, (self.ny - 1) as f32);
        let i = (fx.floor() as u32).min(self.nx - 2);
        let j = (fy.floor() as u32).min(self.ny - 2);
        let (tx, ty) = (fx - i as f32, fy - j as f32);
        let a = self.at(i, j) + (self.at(i + 1, j) - self.at(i, j)) * tx;
        let b = self.at(i, j + 1) + (self.at(i + 1, j + 1) - self.at(i, j + 1)) * tx;
        a + (b - a) * ty
    }

    /// Ground height at `p` on the grid's triangles: each cell split along its
    /// `(i, j)`–`(i + 1, j + 1)` diagonal, which is how the greybox mesh draws the terrain and
    /// how `wp_sim`'s Rapier heightfield tiles are split. Clamped to the grid.
    pub fn height_on_triangles(&self, p: V2) -> f32 {
        let fx = ((p[0] - self.origin[0]) / self.cell_m).clamp(0.0, (self.nx - 1) as f32);
        let fy = ((p[1] - self.origin[1]) / self.cell_m).clamp(0.0, (self.ny - 1) as f32);
        let i = (fx.floor() as u32).min(self.nx - 2);
        let j = (fy.floor() as u32).min(self.ny - 2);
        let (u, v) = (fx - i as f32, fy - j as f32);
        let z0 = self.at(i, j);
        let z2 = self.at(i + 1, j + 1);
        if u >= v {
            z0 + u * (self.at(i + 1, j) - z0) + v * (z2 - self.at(i + 1, j))
        } else {
            z0 + v * (self.at(i, j + 1) - z0) + u * (z2 - self.at(i, j + 1))
        }
    }

    /// Coarse copy for drawing, with vertices `spacing_m` apart (rounded to whole cells): each
    /// coarse vertex is lowered to the minimum of the fine heights within one coarse step
    /// around it, minus 5 cm, so the coarse surface never rises above the fine one (and so
    /// never above a road). This is the terrain the greybox mesh draws.
    pub fn lowered_coarse(&self, spacing_m: f32) -> Heightfield {
        let k = ((spacing_m / self.cell_m).round() as u32).max(1);
        let cx = (self.nx - 1).div_ceil(k) + 1;
        let cy = (self.ny - 1).div_ceil(k) + 1;
        let mut z = vec![0.0f32; (cx * cy) as usize];
        for j in 0..cy {
            for i in 0..cx {
                let (fi, fj) = ((i * k).min(self.nx - 1), (j * k).min(self.ny - 1));
                let mut m = f32::INFINITY;
                for dj in -(k as i64)..=(k as i64) {
                    for di in -(k as i64)..=(k as i64) {
                        let a = (fi as i64 + di).clamp(0, self.nx as i64 - 1) as u32;
                        let b = (fj as i64 + dj).clamp(0, self.ny as i64 - 1) as u32;
                        m = m.min(self.at(a, b));
                    }
                }
                z[(j * cx + i) as usize] = m - 0.05;
            }
        }
        Heightfield {
            origin: self.origin,
            cell_m: self.cell_m * k as f32,
            nx: cx,
            ny: cy,
            z,
        }
    }

    /// Unit ground normal at `p` (central differences over one cell).
    pub fn normal_at(&self, p: V2) -> [f32; 3] {
        let h = self.cell_m;
        let dx = (self.height_at([p[0] + h, p[1]]) - self.height_at([p[0] - h, p[1]])) / (2.0 * h);
        let dy = (self.height_at([p[0], p[1] + h]) - self.height_at([p[0], p[1] - h])) / (2.0 * h);
        let l = (dx * dx + dy * dy + 1.0).sqrt();
        [-dx / l, -dy / l, 1.0 / l]
    }

    /// Lowest and highest height.
    pub fn range(&self) -> (f32, f32) {
        self.z
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), z| {
                (lo.min(*z), hi.max(*z))
            })
    }

    /// Whether `p` lies inside the covered area.
    pub fn contains(&self, p: V2) -> bool {
        let hi = self.max_corner();
        p[0] >= self.origin[0] && p[1] >= self.origin[1] && p[0] <= hi[0] && p[1] <= hi[1]
    }
}

/// Cubic smoothstep of `x` between `e0` and `e1` (0 below `e0`, 1 above `e1`; works for
/// `e0 > e1` too, as a falling edge).
pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Smooth radial bump: `1` at the centre, `0` at distance `radius` and beyond,
/// `(1 - r²/R²)²` in between (no transcendental functions).
pub fn bump(p: V2, centre: V2, radius: f32) -> f32 {
    let dx = p[0] - centre[0];
    let dy = p[1] - centre[1];
    let q = (dx * dx + dy * dy) / (radius * radius);
    if q >= 1.0 {
        0.0
    } else {
        (1.0 - q) * (1.0 - q)
    }
}

/// Seeded 2D value noise: random values on a square lattice of spacing `wavelength_m`,
/// smoothstep-interpolated. Output in `-1..1`.
#[derive(Debug, Clone)]
pub struct ValueNoise {
    origin: V2,
    wavelength: f32,
    n: u32,
    values: Vec<f32>,
}

impl ValueNoise {
    /// Noise covering `lo..hi` (clamped outside) with lattice spacing `wavelength_m`.
    pub fn new(rng: &mut Rng, lo: V2, hi: V2, wavelength_m: f32) -> Self {
        let span = (hi[0] - lo[0]).max(hi[1] - lo[1]);
        let n = ((span / wavelength_m).ceil() as u32 + 2).max(2);
        let values = (0..n * n).map(|_| rng.range_f32(-1.0, 1.0)).collect();
        Self {
            origin: lo,
            wavelength: wavelength_m,
            n,
            values,
        }
    }

    /// Noise value at `p`.
    pub fn at(&self, p: V2) -> f32 {
        let fx = ((p[0] - self.origin[0]) / self.wavelength).clamp(0.0, (self.n - 1) as f32);
        let fy = ((p[1] - self.origin[1]) / self.wavelength).clamp(0.0, (self.n - 1) as f32);
        let i = (fx.floor() as u32).min(self.n - 2);
        let j = (fy.floor() as u32).min(self.n - 2);
        let s = |t: f32| t * t * (3.0 - 2.0 * t);
        let (tx, ty) = (s(fx - i as f32), s(fy - j as f32));
        let v = |a: u32, b: u32| self.values[(b * self.n + a) as usize];
        let a = v(i, j) + (v(i + 1, j) - v(i, j)) * tx;
        let b = v(i, j + 1) + (v(i + 1, j + 1) - v(i, j + 1)) * tx;
        a + (b - a) * ty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bilinear_and_clamped() {
        let mut h = Heightfield::flat([0.0, 0.0], [10.0, 10.0], 10.0, 0.0);
        assert_eq!((h.nx, h.ny), (2, 2));
        h.z = vec![0.0, 10.0, 0.0, 10.0];
        assert!((h.height_at([5.0, 5.0]) - 5.0).abs() < 1e-5);
        assert!((h.height_at([-50.0, 3.0]) - 0.0).abs() < 1e-5);
        assert!((h.height_at([50.0, 3.0]) - 10.0).abs() < 1e-5);
        let n = h.normal_at([5.0, 5.0]);
        assert!(n[0] < -0.3 && n[2] > 0.5);
    }

    #[test]
    fn lowered_coarse_never_rises_above_the_fine_field() {
        let mut h = Heightfield::flat([0.0, 0.0], [40.0, 40.0], 5.0, 0.0);
        for j in 0..h.ny {
            for i in 0..h.nx {
                h.z[(j * h.nx + i) as usize] = (i * i) as f32 * 0.3 + j as f32;
            }
        }
        let c = h.lowered_coarse(10.0);
        assert_eq!((c.nx, c.ny, c.cell_m), (5, 5, 10.0));
        for j in 0..h.ny {
            for i in 0..h.nx {
                let p = h.vertex_pos(i, j);
                assert!(c.height_on_triangles(p) <= h.at(i, j) - 0.05 + 1e-4);
            }
        }
        // On the triangles, a vertex's own height is exact and a cell centre lies on the
        // (i, j)–(i + 1, j + 1) diagonal.
        assert_eq!(c.height_on_triangles([10.0, 20.0]), c.at(1, 2));
        let mid = (c.at(1, 1) + c.at(2, 2)) * 0.5;
        assert!((c.height_on_triangles([15.0, 15.0]) - mid).abs() < 1e-4);
    }

    #[test]
    fn noise_is_deterministic_and_bounded() {
        let a = ValueNoise::new(&mut Rng::new(3), [0.0, 0.0], [1000.0, 1000.0], 100.0);
        let b = ValueNoise::new(&mut Rng::new(3), [0.0, 0.0], [1000.0, 1000.0], 100.0);
        for k in 0..200 {
            let p = [k as f32 * 4.7, k as f32 * 3.1];
            assert_eq!(a.at(p), b.at(p));
            assert!(a.at(p).abs() <= 1.0);
        }
        assert_eq!(bump([0.0, 0.0], [0.0, 0.0], 10.0), 1.0);
        assert_eq!(bump([10.0, 0.0], [0.0, 0.0], 10.0), 0.0);
        assert!((smoothstep(0.0, 1.0, 0.5) - 0.5).abs() < 1e-6);
        assert!((smoothstep(1.0, 0.0, 0.25) - smoothstep(0.0, 1.0, 0.75)).abs() < 1e-6);
    }
}
