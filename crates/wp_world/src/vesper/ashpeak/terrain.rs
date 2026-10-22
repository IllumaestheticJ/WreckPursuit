//! Ashpeak County's natural (uncarved) ground. Inside Port Vesper's extent it is Port
//! Vesper's own ground; it blends into Ashpeak's over [`plan::BLEND_M`] outside it.
//!
//! - **Ashpeak Range**: a massif whose crest line (control points in [`CREST`]) runs west to
//!   east between [`plan::RANGE_SOUTH_Y`] and [`plan::RANGE_NORTH_Y`]: the Grizzly saddle
//!   (Route 12's pass), the Ashpeak summit, the Crowfoot saddle (the touge) and Talon Ridge
//!   (Route 7's tunnel). Peaks along the crest, spurs and gullies running down both faces
//!   (noise stretched along the fall line), a rocky texture on top.
//! - **Road benches**: the mountain roads ([`super::mountains::corridors`]) get a designed
//!   height profile (the ground along them, smoothed and grade-limited) and the ground within
//!   [`CONFORM_REACH_M`] of them is pulled to it, so switchbacks sit on benches cut into the
//!   slope instead of on bridges or in trenches.
//! - **Silver River gorge** along the range's northern foot, east of its head near Route 7,
//!   down to the sea.
//! - **Harlan Flats**: low rolling farmland west of Port Vesper, cut by the Sable River
//!   valley, which heads up into the range as a mountain valley between the Grizzly saddle
//!   and the summit.
//! - **Foothills** north-west of the range; the **Ashpeak City basin** rises gently north of
//!   it, the sea to the east.

use wp_math::Rng;

use super::plan::{self, LO};
use crate::geom::{self, V2};
use crate::terrain::{bump, smoothstep, ValueNoise};

/// Crest line of the range: `(x, crest y, crest height above the plain)`, interpolated by a
/// cubic through the points (flat at local extremes: the saddles and peaks).
const CREST: &[(f32, f32, f32)] = &[
    (-4800.0, 2560.0, 290.0),
    (-4100.0, 2600.0, 370.0),
    (-3550.0, 2650.0, 340.0),
    (-3000.0, 2700.0, 250.0), // Grizzly saddle
    (-2450.0, 2730.0, 360.0),
    (-1900.0, 2750.0, 395.0), // Ashpeak (the summit cone adds to it)
    (-1350.0, 2700.0, 290.0),
    (-800.0, 2650.0, 250.0),
    (-250.0, 2625.0, 215.0), // Crowfoot saddle
    (300.0, 2600.0, 290.0),
    (850.0, 2600.0, 330.0), // Talon Ridge
    (1300.0, 2630.0, 250.0),
    (1900.0, 2700.0, 120.0),
];

/// Southern foot of the range (where it starts to rise), m.
const FOOT_SOUTH_Y: f32 = plan::RANGE_SOUTH_Y - 150.0;
/// Northern foot of the range, m.
const FOOT_NORTH_Y: f32 = plan::RANGE_NORTH_Y + 150.0;

/// Ground within this distance of a mountain road's centreline is pulled to the road's
/// designed profile (fully within [`CONFORM_CORE_M`]), m.
pub(super) const CONFORM_REACH_M: f32 = 60.0;
/// Fully conformed half-width around a mountain road (lane, sidewalk, lot, verge), m.
const CONFORM_CORE_M: f32 = 16.0;
/// Grade the mountain roads' designed profiles keep to (their style caps are higher).
pub(super) const DESIGN_GRADE: f32 = 0.095;

/// Cubic Hermite through the `(x, value)` table column `col` (1 = crest y, 2 = height).
fn crest_interp(x: f32, col: usize) -> f32 {
    let v = |i: usize| match col {
        1 => CREST[i].1,
        _ => CREST[i].2,
    };
    let n = CREST.len();
    if x <= CREST[0].0 {
        return v(0);
    }
    if x >= CREST[n - 1].0 {
        return v(n - 1);
    }
    let i = (0..n - 1).find(|i| x <= CREST[i + 1].0).unwrap_or(n - 2);
    let tangent = |k: usize| -> f32 {
        if k == 0 || k == n - 1 {
            return 0.0;
        }
        let (a, b, c) = (v(k - 1), v(k), v(k + 1));
        // Flat at local extremes, so saddles and peaks stay where the table puts them.
        if (b - a) * (c - b) <= 0.0 {
            0.0
        } else {
            (c - a) / (CREST[k + 1].0 - CREST[k - 1].0)
        }
    };
    let (x0, x1) = (CREST[i].0, CREST[i + 1].0);
    let h = x1 - x0;
    let t = (x - x0) / h;
    let (t2, t3) = (t * t, t * t * t);
    (2.0 * t3 - 3.0 * t2 + 1.0) * v(i)
        + (t3 - 2.0 * t2 + t) * h * tangent(i)
        + (-2.0 * t3 + 3.0 * t2) * v(i + 1)
        + (t3 - t2) * h * tangent(i + 1)
}

/// Crest line (y) of the range along x, m.
fn crest_y(x: f32) -> f32 {
    crest_interp(x, 1)
}

/// Crest height above the plain along x, m.
fn crest_h(x: f32) -> f32 {
    crest_interp(x, 2)
}

/// Silver River centreline y along x (east of the gorge's head), m.
fn silver_y(x: f32) -> f32 {
    let (s, _) = wp_math::sin_cos(x / 420.0);
    3330.0 - 0.05 * (x - plan::SILVER_GORGE[0]) + 35.0 * s
}

/// Sable River centreline x along y, m.
fn sable_x(y: f32) -> f32 {
    let (s, _) = wp_math::sin_cos(y / 650.0);
    plan::SABLE_RIVER_X + 150.0 * s
}

/// Lower `z` into a river valley: bed `bed` at the centreline, banks rising `slope` per m
/// beyond a `half` m wide bed.
fn valley(z: f32, dist: f32, bed: f32, half: f32, slope: f32) -> f32 {
    z.min(bed + (dist - half).max(0.0) * slope)
}

/// Distance of `p` outside Port Vesper's extent (0 inside), m.
fn outside_vesper(p: V2) -> f32 {
    let dx = (plan::VESPER_LO[0] - p[0])
        .max(p[0] - plan::VESPER_HI[0])
        .max(0.0);
    let dy = (plan::VESPER_LO[1] - p[1])
        .max(p[1] - plan::VESPER_HI[1])
        .max(0.0);
    (dx * dx + dy * dy).sqrt()
}

/// Ashpeak County's own ground (no blending, no road benches).
struct Ground {
    n1: ValueNoise,
    n2: ValueNoise,
    n3: ValueNoise,
    /// Spurs and gullies (sampled with y compressed towards the crest: long along the fall
    /// line).
    gully: ValueNoise,
    /// Peaks along the crest.
    peaks: ValueNoise,
}

impl Ground {
    /// Plain (farmland, basin) height under the range, m.
    fn plain(&self, p: V2) -> f32 {
        let [x, y] = p;
        let base = 6.0
            + 20.0 * smoothstep(-900.0, 1600.0, y)
            + 10.0 * smoothstep(3500.0, 5600.0, y)
            + 7.0 * self.n1.at(p);
        // Foothills north-west of the range (west of Ashpeak City): long gentle hills.
        let nw = smoothstep(-2300.0, -2900.0, x)
            * smoothstep(3300.0, 3700.0, y)
            * smoothstep(4900.0, 4300.0, y);
        base + nw * (22.0 + 18.0 * self.n2.at([x, y * 0.6]))
    }

    /// The range alone (above the plain), m.
    fn range(&self, p: V2) -> f32 {
        let [x, y] = p;
        let cy = crest_y(x);
        let dy = y - cy;
        let w = if dy < 0.0 {
            cy - FOOT_SOUTH_Y
        } else {
            FOOT_NORTH_Y - cy
        };
        let t = (1.0 - dy.abs() / w).clamp(0.0, 1.0);
        if t <= 0.0 {
            return 0.0;
        }
        // Cross profile: flat at the foot, steepest on the mid-slopes, a rounded crest.
        let f = t * t * (3.0 - 2.0 * t);
        // Peaks along the crest, kept off the two named saddles and the summit.
        let calm = (1.0 - bump(p, plan::CROWFOOT_SADDLE, 700.0))
            * (1.0 - bump(p, plan::GRIZZLY_SADDLE, 600.0))
            * (1.0 - bump(p, plan::ASHPEAK_SUMMIT, 500.0));
        // The Halcyon ridge between the saddle and the summit stays an even crest (its road
        // runs along it).
        let ridge = smoothstep(-1650.0, -1400.0, x) * smoothstep(0.0, -250.0, x);
        let peaks = 55.0 * self.peaks.at([x, 0.0]).max(-0.4) * calm * (1.0 - 0.85 * ridge);
        let summit = 70.0 * bump(p, plan::ASHPEAK_SUMMIT, 520.0);
        let top = (crest_h(x) + peaks) * f + summit;
        // Spurs and gullies down both faces, strongest on the mid-slopes.
        let along = cy + dy * 0.3;
        let g = self.gully.at([x, along]);
        let ridged = 1.0 - 2.0 * g.abs();
        let mid = 4.0 * t * (1.0 - t);
        let carve = 28.0 * mid * ridged * (0.4 + 0.6 * calm);
        let rough = f * (16.0 * self.n2.at(p) + 7.0 * self.n3.at(p));
        top + carve + rough
    }

    /// Plain plus range, before the rivers.
    fn land(&self, p: V2) -> f32 {
        self.plain(p) + self.range(p)
    }

    fn z(&self, p: V2) -> f32 {
        let [x, y] = p;
        let mut z = self.land(p);
        // Crowfoot Hollow: a broad valley falling north from the Crowfoot saddle, which the
        // pass's north side winds down.
        if y > plan::CROWFOOT_SADDLE[1] && y < FOOT_NORTH_Y + 100.0 && (-1100.0..700.0).contains(&x)
        {
            let u = ((y - plan::CROWFOOT_SADDLE[1]) / (FOOT_NORTH_Y - plan::CROWFOOT_SADDLE[1]))
                .clamp(0.0, 1.0);
            let floor = self.plain(p) + 205.0 * (1.0 - u) * (1.0 - u * 0.35);
            let cx = -155.0 - 300.0 * smoothstep(3150.0, 3600.0, y);
            let open = smoothstep(plan::CROWFOOT_SADDLE[1], plan::CROWFOOT_SADDLE[1] + 150.0, y);
            let carved = valley(z, (x - cx).abs(), floor, 300.0, 0.32);
            z += (carved - z) * open;
        }
        // Silver River gorge along the range's northern foot, from its head near Route 7 down
        // to the sea.
        if x > 150.0 {
            let head = smoothstep(150.0, 450.0, x);
            let bed = -2.0 + 30.0 * smoothstep(1500.0, 200.0, x);
            let d = (y - silver_y(x)).abs();
            let carved = valley(z, d, bed, 18.0, 1.4);
            z += (carved - z) * head;
        }
        // Sable River: through Harlan Flats down to the sea, heading up into the range as a
        // mountain valley that fades out below the crest.
        if x < plan::FARMS_EAST_X + 400.0 && y < 3050.0 {
            let sx = sable_x(y);
            let low_bed = -1.5 + 0.0045 * (y + 1150.0).max(0.0);
            let bed = low_bed + 260.0 * smoothstep(1850.0, 3050.0, y);
            let up = smoothstep(1700.0, 2300.0, y);
            let slope = 0.18 + 0.45 * up;
            let half = 25.0 - 10.0 * up;
            let carved = valley(z, (x - sx).abs(), bed, half, slope);
            // The valley head closes below the crest.
            z = carved + (z - carved) * smoothstep(2700.0, 3050.0, y);
        }
        // Sea: south of the coast (as Port Vesper's) and east of x = 1620.
        let land = z.max(1.5);
        let sea = smoothstep(-1110.0, -1180.0, y).max(smoothstep(1600.0, 1660.0, x));
        land + (-3.0 - land) * sea
    }
}

/// Port Vesper's smooth base height (its own ground without the hills and noise), m: stands in
/// for Port Vesper's ground when designing road profiles near its north edge.
fn vesper_base(p: V2) -> f32 {
    2.5 + 22.0 * smoothstep(-650.0, 1400.0, p[1])
}

/// Blend factor of Ashpeak's own ground at `p` (0 inside Port Vesper).
fn own_weight(p: V2) -> f32 {
    smoothstep(0.0, plan::BLEND_M, outside_vesper(p))
}

/// Designed profiles of the mountain roads and a bucket grid over their samples.
struct Benches {
    /// `(position, designed height)` samples of every road.
    samples: Vec<(V2, f32)>,
    lo: V2,
    cell: f32,
    nx: usize,
    ny: usize,
    buckets: Vec<Vec<u32>>,
}

/// Sample spacing of the designed profiles, m.
const BENCH_DS: f32 = 4.0;

/// Moving average over `half` samples each side, ends kept.
fn smooth_keep_ends(v: &[f32], half: usize) -> Vec<f32> {
    let n = v.len();
    (0..n)
        .map(|i| {
            if i == 0 || i + 1 == n {
                return v[i];
            }
            let a = i.saturating_sub(half);
            let b = (i + half).min(n - 1);
            v[a..=b].iter().sum::<f32>() / (b - a + 1) as f32
        })
        .collect()
}

/// A grade-limited profile following `desired` (samples `ds` apart) between fixed end
/// heights `za`, `zb`; `grade[i]` caps the step from sample `i - 1` to `i`.
pub(super) fn design_profile(desired: &[f32], ds: f32, za: f32, zb: f32, grade: &[f32]) -> Vec<f32> {
    let n = desired.len();
    let g: Vec<f32> = grade.iter().map(|x| x * ds).collect();
    // Largest climb possible from each end to each sample.
    let mut from_a = vec![0.0f32; n];
    for i in 1..n {
        from_a[i] = from_a[i - 1] + g[i];
    }
    let mut from_b = vec![0.0f32; n];
    for i in (0..n - 1).rev() {
        from_b[i] = from_b[i + 1] + g[i + 1];
    }
    let mut z: Vec<f32> = (0..n)
        .map(|i| {
            let lo = (za - from_a[i]).max(zb - from_b[i]);
            let hi = (za + from_a[i]).min(zb + from_b[i]);
            if lo <= hi {
                desired[i].clamp(lo, hi)
            } else {
                0.5 * (lo + hi)
            }
        })
        .collect();
    z[0] = za;
    z[n - 1] = zb;
    let mut fwd = z.clone();
    for i in 1..n - 1 {
        fwd[i] = fwd[i].clamp(fwd[i - 1] - g[i], fwd[i - 1] + g[i]);
    }
    let mut bwd = z.clone();
    for i in (1..n - 1).rev() {
        bwd[i] = bwd[i].clamp(bwd[i + 1] - g[i + 1], bwd[i + 1] + g[i + 1]);
    }
    for i in 1..n - 1 {
        z[i] = 0.5 * (fwd[i] + bwd[i]);
    }
    // Binomial passes (ends pinned) round crests and sags (and keep a uniform grade limit).
    for _ in 0..24 {
        let prev = z.clone();
        for i in 1..n - 1 {
            z[i] = 0.25 * prev[i - 1] + 0.5 * prev[i] + 0.25 * prev[i + 1];
        }
    }
    z
}

/// Design grade along a path: `grade`, easing off to 45 % of it through tight
/// bends (hairpins are nearly level, as on real mountain roads).
fn grade_along(pts: &[V2], grade: f32) -> Vec<f32> {
    let n = pts.len();
    let mut out = vec![grade; n];
    for i in 1..n.saturating_sub(1) {
        let a = geom::sub(pts[i], pts[i - 1]);
        let b = geom::sub(pts[i + 1], pts[i]);
        let turn = wp_math::atan2(geom::cross(a, b), geom::dot(a, b)).abs();
        let k = turn / (0.5 * (geom::len(a) + geom::len(b))).max(1e-3);
        out[i] = grade * (1.0 - 0.55 * smoothstep(1.0 / 70.0, 1.0 / 28.0, k));
    }
    // Ease in and out over about 30 m.
    let half = 4;
    (0..n)
        .map(|i| {
            let a = i.saturating_sub(half);
            let b = (i + half).min(n - 1);
            out[a..=b].iter().copied().fold(f32::INFINITY, f32::min)
        })
        .collect::<Vec<f32>>()
}

/// Uniform resampling of a polyline at about `step` m (both ends kept).
fn resample(p: &[V2], step: f32) -> Vec<V2> {
    let mut cum = vec![0.0f32];
    for w in p.windows(2) {
        cum.push(cum[cum.len() - 1] + geom::dist(w[0], w[1]));
    }
    let total = cum[cum.len() - 1];
    let n = ((total / step).ceil() as usize).max(1);
    let mut out = Vec::with_capacity(n + 1);
    let mut i = 0;
    for k in 0..=n {
        let s = total * k as f32 / n as f32;
        while i + 2 < p.len() && cum[i + 1] < s {
            i += 1;
        }
        let l = (cum[i + 1] - cum[i]).max(1e-6);
        let t = ((s - cum[i]) / l).clamp(0.0, 1.0);
        out.push(geom::add(p[i], geom::scale(geom::sub(p[i + 1], p[i]), t)));
    }
    out
}

impl Benches {
    /// Design every road in order: an end on an earlier road takes that road's height there.
    fn new(ground: &Ground, roads: &[(Vec<V2>, f32)]) -> Self {
        let raw = |p: V2| {
            let w = own_weight(p);
            let v = vesper_base(p);
            v + (ground.z(p) - v) * w
        };
        let mut samples: Vec<(V2, f32)> = Vec::new();
        for (path, grade) in roads {
            let pts = resample(path, BENCH_DS);
            let n = pts.len();
            let ds = {
                let total: f32 = path.windows(2).map(|w| geom::dist(w[0], w[1])).sum();
                total / (n - 1) as f32
            };
            let rz: Vec<f32> = pts.iter().map(|p| raw(*p)).collect();
            let desired = smooth_keep_ends(&smooth_keep_ends(&rz, 6), 6);
            let end_z = |p: V2, fallback: f32| {
                samples
                    .iter()
                    .filter(|(q, _)| geom::dist(*q, p) < 6.0)
                    .min_by(|a, b| geom::dist(a.0, p).total_cmp(&geom::dist(b.0, p)))
                    .map_or(fallback, |s| s.1)
            };
            let za = end_z(pts[0], desired[0]);
            // A free end (a dead end, the lot) stays within reach of the start.
            let reach = 0.8 * grade * ds * (n - 1) as f32;
            let zb = end_z(pts[n - 1], desired[n - 1].clamp(za - reach, za + reach));
            let z = design_profile(&desired, ds, za, zb, &grade_along(&pts, *grade));
            samples.extend(pts.into_iter().zip(z));
        }
        let cell = CONFORM_REACH_M;
        let mut lo = [f32::INFINITY; 2];
        let mut hi = [f32::NEG_INFINITY; 2];
        for (p, _) in &samples {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k] - cell);
                hi[k] = hi[k].max(p[k] + cell);
            }
        }
        if samples.is_empty() {
            lo = [0.0, 0.0];
            hi = [1.0, 1.0];
        }
        let nx = ((hi[0] - lo[0]) / cell).ceil() as usize + 1;
        let ny = ((hi[1] - lo[1]) / cell).ceil() as usize + 1;
        let mut buckets = vec![Vec::new(); nx * ny];
        for (k, (p, _)) in samples.iter().enumerate() {
            let i = ((p[0] - lo[0]) / cell) as usize;
            let j = ((p[1] - lo[1]) / cell) as usize;
            buckets[j * nx + i].push(k as u32);
        }
        Self {
            samples,
            lo,
            cell,
            nx,
            ny,
            buckets,
        }
    }

    /// `z` pulled towards the designed road heights near `p`.
    fn apply(&self, p: V2, z: f32) -> f32 {
        let fi = (p[0] - self.lo[0]) / self.cell;
        let fj = (p[1] - self.lo[1]) / self.cell;
        if fi < 0.0 || fj < 0.0 || fi >= self.nx as f32 || fj >= self.ny as f32 {
            return z;
        }
        let (ci, cj) = (fi as usize, fj as usize);
        let r2 = CONFORM_REACH_M * CONFORM_REACH_M;
        let (mut wsum, mut zsum, mut dmin2) = (0.0f32, 0.0f32, f32::INFINITY);
        for j in cj.saturating_sub(1)..=(cj + 1).min(self.ny - 1) {
            for i in ci.saturating_sub(1)..=(ci + 1).min(self.nx - 1) {
                for &k in &self.buckets[j * self.nx + i] {
                    let (q, zq) = self.samples[k as usize];
                    let (dx, dy) = (p[0] - q[0], p[1] - q[1]);
                    let d2 = dx * dx + dy * dy;
                    if d2 >= r2 {
                        continue;
                    }
                    dmin2 = dmin2.min(d2);
                    // Inverse-distance weight, tapered to zero at the reach (continuous).
                    let taper = (1.0 - d2 / r2) * (1.0 - d2 / r2);
                    let w = taper / (d2 + 9.0);
                    wsum += w;
                    zsum += w * zq;
                }
            }
        }
        if wsum <= 0.0 {
            return z;
        }
        let cover = smoothstep(CONFORM_REACH_M, CONFORM_CORE_M, dmin2.sqrt());
        z + (zsum / wsum - z) * cover
    }
}

/// Ashpeak's own ground for `seed` (no road benches).
fn ground(seed: u64) -> Ground {
    let mut rng = Rng::new(seed ^ 0xa5be_a4c0_u64);
    Ground {
        n1: ValueNoise::new(&mut rng, LO, plan::HI, 520.0),
        n2: ValueNoise::new(&mut rng, LO, plan::HI, 260.0),
        n3: ValueNoise::new(&mut rng, LO, plan::HI, 95.0),
        gully: ValueNoise::new(&mut rng, LO, plan::HI, 190.0),
        peaks: ValueNoise::new(&mut rng, LO, plan::HI, 420.0),
    }
}

/// The combined map's natural ground: `f(p, vesper_z)`.
pub(super) fn natural(seed: u64) -> Box<dyn Fn(V2, f32) -> f32> {
    let ground = ground(seed);
    let benches = Benches::new(&ground, &super::mountains::corridors());
    Box::new(move |p, vesper_z| {
        let w = own_weight(p);
        if w <= 0.0 {
            return vesper_z;
        }
        let own = benches.apply(p, ground.z(p));
        vesper_z + (own - vesper_z) * w
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_saddles_and_summit() {
        let f = natural(1);
        let z = |p: V2| f(p, 0.0);
        let summit = z(plan::ASHPEAK_SUMMIT);
        let crowfoot = z(plan::CROWFOOT_SADDLE);
        assert!(summit > 450.0, "summit {summit}");
        assert!(
            crowfoot > 200.0 && crowfoot < summit - 150.0,
            "crowfoot {crowfoot}"
        );
        // The saddle is a saddle (in the ground before the road benches): lower along the
        // crest either way, higher across it.
        let raw = ground(1);
        let z = |p: V2| raw.z(p);
        let crowfoot = z(plan::CROWFOOT_SADDLE);
        let s = plan::CROWFOOT_SADDLE;
        let along = |dx: f32| z([s[0] + dx, crest_y(s[0] + dx)]);
        assert!(along(-700.0) > crowfoot + 30.0 && along(600.0) > crowfoot + 30.0);
        assert!(z([s[0], s[1] - 400.0]) < crowfoot - 60.0);
        assert!(z([s[0], s[1] + 400.0]) < crowfoot - 60.0);
        let z = |p: V2| f(p, 0.0);
        // Port Vesper's ground is untouched inside its extent.
        assert_eq!(f([0.0, 0.0], 12.5), 12.5);
        assert_eq!(f(plan::R40N_END, 31.0), 31.0);
        // Sea east of the metro.
        assert!(z([1690.0, 4500.0]) < 0.0);
        // The gorge is deep where Route 7 crosses it.
        let rim = z(crate::geom::add(plan::SILVER_GORGE, [0.0, -250.0]));
        let bed = z([plan::SILVER_GORGE[0], silver_y(plan::SILVER_GORGE[0])]);
        assert!(rim - bed > 40.0, "rim {rim} bed {bed}");
    }

    #[test]
    fn farmland_and_basin_stay_low_and_gentle() {
        let f = natural(1);
        let z = |p: V2| f(p, 20.0);
        // Harlan Flats: low rolling farmland, the Sable River below its banks.
        for x in [-4400.0, -3700.0, -3100.0, -2000.0] {
            for y in [-900.0, -300.0, 400.0, 1100.0, 1700.0] {
                let h = z([x, y]);
                assert!((0.0..60.0).contains(&h), "farm {h} at {x},{y}");
            }
        }
        for y in [-600.0, 0.0, 800.0, 1500.0] {
            let river = z([sable_x(y), y]);
            let bank = z([sable_x(y) + 300.0, y]);
            assert!(river < bank - 3.0, "Sable at {y}: {river} vs {bank}");
        }
        // Ashpeak City's basin: 10–60 m, gentle.
        for x in [-2000.0, -1300.0, -600.0, 0.0, 600.0, 1400.0] {
            for y in [3800.0, 4300.0, 4900.0, 5500.0] {
                let h = z([x, y]);
                assert!((5.0..60.0).contains(&h), "basin {h} at {x},{y}");
                let g = (z([x + 20.0, y]) - z([x - 20.0, y])).abs() / 40.0;
                assert!(g < 0.08, "basin slope {g} at {x},{y}");
            }
        }
    }

    #[test]
    fn sable_valley_heads_smoothly_into_the_range() {
        let f = natural(1);
        let z = |p: V2| f(p, 20.0);
        // Along the river the bed rises steadily, no cliff where the range starts.
        let mut prev = z([sable_x(1200.0), 1200.0]);
        let mut y = 1220.0;
        while y < 2800.0 {
            let h = z([sable_x(y), y]);
            assert!((h - prev).abs() < 20.0 * 0.45, "step {} at y {y}", h - prev);
            prev = h;
            y += 20.0;
        }
        // Still a valley up in the range.
        let (sx, y) = (sable_x(2300.0), 2300.0);
        let floor = z([sx, y]);
        assert!(floor > 40.0, "valley floor {floor}");
        assert!(z([sx - 350.0, y]) > floor + 25.0 && z([sx + 350.0, y]) > floor + 25.0);
    }

    #[test]
    fn road_benches_stay_close_to_the_natural_slope() {
        // The designed road profiles cut into and build out from the natural slope only
        // moderately (benches, not canyons or embankments).
        let raw = ground(1);
        let roads = super::super::mountains::corridors();
        let benches = Benches::new(&raw, &roads);
        let mut k = 0;
        let mut worst = Vec::new();
        let mut dbg_out = String::new();
        for (r, (path, _)) in roads.iter().enumerate() {
            let n = resample(path, BENCH_DS).len();
            let (mut cut, mut fill, mut sum) = (0.0f32, 0.0f32, 0.0f32);
            for (p, z) in &benches.samples[k..k + n] {
                let w = own_weight(*p);
                let natural = vesper_base(*p) + (raw.z(*p) - vesper_base(*p)) * w;
                cut = cut.max(natural - z);
                fill = fill.max(z - natural);
                sum += (natural - z).abs();
                dbg_out.push_str(&format!("{r} {} {} {natural} {z}\n", p[0], p[1]));
            }
            k += n;
            let mean = sum / n as f32;
            println!("road {r}: cut {cut:.1} fill {fill:.1} mean {mean:.1}");
            worst.push((r, cut, fill, mean));
        }
        std::fs::write("/tmp/bench_dbg.txt", dbg_out).unwrap();
        for (r, cut, fill, mean) in worst {
            assert!(mean < 15.0, "road {r} mean deviation {mean}");
            assert!(cut < 50.0 && fill < 35.0, "road {r}: cut {cut} fill {fill}");
        }
    }

    #[test]
    fn design_profile_keeps_its_grade_and_ends() {
        let desired: Vec<f32> = (0..200).map(|i| if i < 100 { 0.0 } else { 60.0 }).collect();
        let z = design_profile(&desired, 4.0, 0.0, 60.0, &[0.1; 200]);
        assert_eq!((z[0], z[199]), (0.0, 60.0));
        for w in z.windows(2) {
            assert!((w[1] - w[0]).abs() <= 0.4 + 1e-3);
        }
    }
}
