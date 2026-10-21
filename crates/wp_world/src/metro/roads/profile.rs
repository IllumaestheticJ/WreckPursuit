//! Vertical profiles: grade-capped, bounded, smoothed road heights (generalised from Port
//! Vesper v2's `profile`).
//!
//! A profile is a height per uniform sample along a road. Inputs are the desired height
//! (usually the smoothed ground), per-sample lower / upper bounds (junction approaches are
//! fixed with `lo == hi`, bridge decks and grade separations give lower bounds, tunnels and
//! underpasses upper bounds) and the grade cap. The solver:
//! 1. builds the grade-Lipschitz envelopes `L ≤ z ≤ U` of the bounds (two passes each way;
//!    twice around for closed roads),
//! 2. clamps the desired heights into them and grade-limits the result with averaged
//!    forward / backward clamps (which stay inside the envelopes),
//! 3. smooths with binomial passes (≈ Gaussian) until a full grade reversal bends no tighter
//!    than `kmax` (crests and sags at 0.3 g at 1.15 × the limit), keeping pinned samples
//!    (junction boxes) fixed and re-clamping into the envelopes after every pass.
//!
//! Binomial smoothing and clamping into Lipschitz envelopes both preserve the grade cap, so
//! the result never exceeds it (except where the bounds themselves are infeasible, which is
//! reported).

/// Design crest / sag limit: vertical acceleration ≤ 0.3 g at the limit (+15 %), 1/m.
pub fn max_vertical_curvature(speed: f32) -> f32 {
    let v = speed * 1.15;
    0.3 * 9.81 / (v * v)
}

/// Smoothing width (m) that rounds a full grade reversal to curvature `kmax`.
pub fn smoothing_sigma(grade: f32, kmax: f32, ds: f32) -> f32 {
    (2.0 * grade / (kmax * 2.5)).max(2.0 * ds)
}

/// Result of [`solve`].
#[derive(Debug, Clone)]
pub struct Profile {
    /// Height per sample, m.
    pub z: Vec<f32>,
    /// Samples where the bounds could not all be met (lower envelope above the upper).
    pub infeasible: usize,
}

fn idx(i: isize, n: usize) -> usize {
    i.rem_euclid(n as isize) as usize
}

/// Lipschitz envelopes of the bounds: `(L, U)` with `|L[i+1] - L[i]| ≤ g` etc.
pub fn envelopes(lo: &[f32], hi: &[f32], g: f32, cyclic: bool) -> (Vec<f32>, Vec<f32>) {
    let n = lo.len();
    let mut l = lo.to_vec();
    let mut u = hi.to_vec();
    let laps = if cyclic { 2 } else { 1 };
    for _ in 0..laps {
        for k in 1..n as isize + if cyclic { 1 } else { 0 } {
            let (i, p) = (idx(k, n), idx(k - 1, n));
            l[i] = l[i].max(l[p] - g);
            u[i] = u[i].min(u[p] + g);
        }
        for k in (0..n as isize - if cyclic { 0 } else { 1 }).rev() {
            let (i, q) = (idx(k, n), idx(k + 1, n));
            l[i] = l[i].max(l[q] - g);
            u[i] = u[i].min(u[q] + g);
        }
    }
    (l, u)
}

/// Solve a profile (see the module docs). `pin[i]` keeps sample `i` fixed during smoothing.
/// `cyclic` treats the samples as a closed loop (sample `n-1` is followed by sample 0).
#[allow(clippy::too_many_arguments)]
pub fn solve(
    desired: &[f32],
    ds: f32,
    lo: &[f32],
    hi: &[f32],
    pin: &[bool],
    grade: f32,
    kmax: f32,
    cyclic: bool,
) -> Profile {
    let n = desired.len();
    debug_assert!(lo.len() == n && hi.len() == n && pin.len() == n);
    if n == 0 {
        return Profile {
            z: Vec::new(),
            infeasible: 0,
        };
    }
    let g = grade * ds;
    let (mut l, mut u) = envelopes(lo, hi, g, cyclic);
    let mut infeasible = 0;
    for i in 0..n {
        if l[i] > u[i] + 1e-3 {
            infeasible += 1;
            let m = 0.5 * (l[i] + u[i]);
            l[i] = m;
            u[i] = m;
        }
    }
    let clampi = |v: f32, i: usize, l: &[f32], u: &[f32]| v.max(l[i]).min(u[i]);
    let z0: Vec<f32> = (0..n).map(|i| clampi(desired[i], i, &l, &u)).collect();
    // Grade limit: forward-only and backward-only clamps, averaged.
    let laps = if cyclic { 3 } else { 1 };
    let mut fwd = z0.clone();
    for _ in 0..laps {
        for k in 1..n as isize + if cyclic { 1 } else { 0 } {
            let (i, p) = (idx(k, n), idx(k - 1, n));
            fwd[i] = clampi(fwd[i].clamp(fwd[p] - g, fwd[p] + g), i, &l, &u);
        }
    }
    let mut bwd = z0.clone();
    for _ in 0..laps {
        for k in (0..n as isize - if cyclic { 0 } else { 1 }).rev() {
            let (i, q) = (idx(k, n), idx(k + 1, n));
            bwd[i] = clampi(bwd[i].clamp(bwd[q] - g, bwd[q] + g), i, &l, &u);
        }
    }
    let mut z: Vec<f32> = (0..n).map(|i| 0.5 * (fwd[i] + bwd[i])).collect();
    // Binomial smoothing ≈ Gaussian with sigma = ds * sqrt(passes / 2).
    let sigma = smoothing_sigma(grade, kmax, ds);
    let passes = (2.0 * (sigma / ds) * (sigma / ds)).ceil() as usize;
    let mut prev = z.clone();
    for _ in 0..passes {
        prev.copy_from_slice(&z);
        for i in 0..n {
            if pin[i] {
                continue;
            }
            let (a, b) = if cyclic {
                (prev[idx(i as isize - 1, n)], prev[idx(i as isize + 1, n)])
            } else if i == 0 || i + 1 == n {
                continue;
            } else {
                (prev[i - 1], prev[i + 1])
            };
            z[i] = clampi(0.25 * a + 0.5 * prev[i] + 0.25 * b, i, &l, &u);
        }
    }
    Profile { z, infeasible }
}

/// Linear interpolation of a sampled profile at arc length `s` (samples `ds` apart).
pub fn sample(z: &[f32], ds: f32, s: f32) -> f32 {
    let n = z.len();
    if n == 1 {
        return z[0];
    }
    let f = (s / ds).max(0.0);
    let i = (f.floor() as usize).min(n - 2);
    let t = (f - i as f32).min(1.0);
    z[i] + (z[i + 1] - z[i]) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_profile_respects_grade_bounds_and_curvature() {
        // A 400 m road whose ground has a 30 m wall in the middle; a deck lower bound of
        // 10 m at 140..160 m and an upper bound (tunnel) of -3 m at 320..336 m.
        let ds = 4.0;
        let n = 101;
        let desired: Vec<f32> = (0..n)
            .map(|i| if (40..60).contains(&i) { 30.0 } else { 0.0 })
            .collect();
        let mut lo = vec![f32::NEG_INFINITY; n];
        let mut hi = vec![f32::INFINITY; n];
        for i in 0..4 {
            lo[i] = 0.0;
            hi[i] = 0.0;
            lo[n - 1 - i] = 0.0;
            hi[n - 1 - i] = 0.0;
        }
        for v in lo.iter_mut().take(40).skip(35) {
            *v = 10.0;
        }
        for v in hi.iter_mut().take(84).skip(80) {
            *v = -3.0;
        }
        let pin: Vec<bool> = (0..n).map(|i| i < 3 || i + 3 >= n).collect();
        let p = solve(&desired, ds, &lo, &hi, &pin, 0.1, 0.02, false);
        assert_eq!(p.infeasible, 0);
        for w in p.z.windows(2) {
            assert!(((w[1] - w[0]) / ds).abs() <= 0.1 + 1e-4);
        }
        assert_eq!(p.z[0], 0.0);
        assert_eq!(p.z[n - 1], 0.0);
        for i in 35..40 {
            assert!(p.z[i] >= 10.0 - 1e-3, "deck {}", p.z[i]);
        }
        for i in 80..84 {
            assert!(p.z[i] <= -3.0 + 1e-3, "tunnel {}", p.z[i]);
        }
    }

    #[test]
    fn cyclic_profile_wraps() {
        let ds = 5.0;
        let n = 200;
        let desired: Vec<f32> = (0..n).map(|i| if i < 100 { 0.0 } else { 20.0 }).collect();
        let lo = vec![f32::NEG_INFINITY; n];
        let hi = vec![f32::INFINITY; n];
        let pin = vec![false; n];
        let p = solve(&desired, ds, &lo, &hi, &pin, 0.05, 0.004, true);
        for i in 0..n {
            let j = (i + 1) % n;
            assert!(((p.z[j] - p.z[i]) / ds).abs() <= 0.05 + 1e-4, "at {i}");
        }
    }
}
