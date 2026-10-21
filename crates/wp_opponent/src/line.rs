//! Racing line and speed profile.
//!
//! **Line.** Each corridor sample `i` gets a lateral offset `n_i` inside its bounds (minus a
//! half car width and a margin). The line points are `p_i = c_i + n_i · r_i`. We minimise the
//! summed squared discrete curvature `Σ |p_{i−1} − 2p_i + p_{i+1}|²` by projected
//! Gauss–Seidel: the optimum for one point with its neighbours fixed is
//! `p_i* = (4(p_{i−1} + p_{i+1}) − (p_{i−2} + p_{i+2})) / 6`; we move `n_i` toward it along
//! `r_i` (over-relaxed) and clamp to the bounds. Plain Gauss–Seidel on this fourth-order
//! problem converges very slowly over long wavelengths, so it runs coarse to fine (every 16th
//! sample, then 8th, … 1st; in-between samples interpolated after each level), K1999-style.
//! The coarsest level starts with a few shortest-path sweeps (`p_i* = (p_{i−1} + p_{i+1})/2`).
//! Deterministic: fixed sweep order, f32 only.
//!
//! **Speed profile.** From the line curvature `κ`: `v ≤ √(a_lat / |κ|)`, then a forward pass
//! (acceleration limited by power, falling off toward top speed, inside the friction ellipse)
//! and a backward pass (braking inside the friction ellipse). Circuits wrap the passes.

use serde::{Deserialize, Serialize};
use wp_contracts::GRAVITY_MPS2;
use wp_world::geom::{self, V2};

use crate::capability::CarCapability;
use crate::corridor::{Corridor, CORRIDOR_STEP_M};

/// Line optimiser settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LineParams {
    /// Clearance kept from the corridor bounds beyond the half car width, m.
    pub edge_margin_m: f32,
    /// Shortest-path warm-up sweeps (coarsest level only).
    pub smooth_sweeps: u32,
    /// Minimum-curvature sweeps per resolution level (strides 16, 8, 4, 2, 1).
    pub curvature_sweeps: u32,
    /// Over-relaxation factor for the curvature sweeps (1 = plain Gauss–Seidel).
    pub relaxation: f32,
}

impl Default for LineParams {
    fn default() -> Self {
        Self {
            edge_margin_m: 0.4,
            smooth_sweeps: 100,
            curvature_sweeps: 150,
            relaxation: 1.2,
        }
    }
}

/// An optimised line through a corridor.
#[derive(Debug, Clone, PartialEq)]
pub struct RacingLine {
    /// Lateral offset per corridor sample, m (+ right).
    pub offset: Vec<f32>,
    /// Signed curvature of the line per sample, 1/m (+ = turning right).
    pub curvature: Vec<f32>,
    /// Line length from sample `i` to `i + 1`, m.
    pub step_len: Vec<f32>,
    /// Allowed offset range per sample (after half width + margin): `(min, max)`.
    pub limits: Vec<(f32, f32)>,
}

fn pt(c: &Corridor, i: isize, n: &[f32]) -> V2 {
    let k = c.wrap(i);
    geom::xy(c.samples[k].at(n[k]))
}

impl RacingLine {
    /// Optimise a line for a car of `half_width_m` through `corridor`.
    pub fn optimise(corridor: &Corridor, half_width_m: f32, params: &LineParams) -> Self {
        let m = corridor.len();
        let limits: Vec<(f32, f32)> = corridor
            .samples
            .iter()
            .map(|c| {
                let lo = -c.left_m + half_width_m + params.edge_margin_m;
                let hi = c.right_m - half_width_m - params.edge_margin_m;
                if lo <= hi {
                    (lo, hi)
                } else {
                    let mid = 0.5 * (lo + hi);
                    (mid, mid)
                }
            })
            .collect();
        let mut n: Vec<f32> = limits
            .iter()
            .map(|(lo, hi)| (0.0f32).clamp(*lo, *hi))
            .collect();
        // Coarse to fine: at stride `st` only every `st`-th sample moves (neighbours `st` and
        // `2·st` away), then the samples in between are interpolated. Long wavelengths
        // converge on the coarse levels, detail on the fine ones.
        let mut strides = Vec::new();
        let mut st = 16usize;
        while st >= 1 {
            if m / st >= 8 {
                strides.push(st);
            }
            st /= 2;
        }
        let closed = corridor.closed;
        for (level, &st) in strides.iter().enumerate() {
            let lattice: Vec<usize> = if closed {
                (0..m).step_by(st).collect()
            } else {
                let mut l: Vec<usize> = (0..m).step_by(st).collect();
                if *l.last().unwrap() != m - 1 {
                    l.push(m - 1);
                }
                l
            };
            let q = lattice.len() as isize;
            let at = |j: isize, n: &[f32]| -> V2 {
                let k = if closed {
                    lattice[j.rem_euclid(q) as usize]
                } else {
                    lattice[j.clamp(0, q - 1) as usize]
                };
                geom::xy(corridor.samples[k].at(n[k]))
            };
            // Open routes keep their two end lattice points fixed.
            let (j0, j1) = if closed { (0, q - 1) } else { (2, q - 3) };
            let smooth = if level == 0 { params.smooth_sweeps } else { 0 };
            for _ in 0..smooth {
                for j in j0..=j1 {
                    let k = lattice[j as usize];
                    let target = geom::scale(geom::add(at(j - 1, &n), at(j + 1, &n)), 0.5);
                    let c = &corridor.samples[k];
                    let d = geom::dot(geom::sub(target, geom::xy(c.pos)), c.normal());
                    n[k] = d.clamp(limits[k].0, limits[k].1);
                }
            }
            for _ in 0..params.curvature_sweeps {
                for j in j0..=j1 {
                    let k = lattice[j as usize];
                    let a = geom::add(at(j - 1, &n), at(j + 1, &n));
                    let b = geom::add(at(j - 2, &n), at(j + 2, &n));
                    let target = geom::scale(geom::sub(geom::scale(a, 4.0), b), 1.0 / 6.0);
                    let c = &corridor.samples[k];
                    let d = geom::dot(geom::sub(target, geom::xy(c.pos)), c.normal());
                    let next = n[k] + params.relaxation * (d - n[k]);
                    n[k] = next.clamp(limits[k].0, limits[k].1);
                }
            }
            if st > 1 {
                for j in 0..q as usize {
                    let a = lattice[j];
                    let (b, span) = if j + 1 < q as usize {
                        (lattice[j + 1], lattice[j + 1] - a)
                    } else if closed {
                        (lattice[0], m - a)
                    } else {
                        continue;
                    };
                    for t in 1..span {
                        let k = (a + t) % m;
                        let f = t as f32 / span as f32;
                        n[k] = (n[a] + (n[b] - n[a]) * f).clamp(limits[k].0, limits[k].1);
                    }
                }
            }
        }
        let mut line = Self {
            offset: n,
            curvature: vec![0.0; m],
            step_len: vec![CORRIDOR_STEP_M; m],
            limits,
        };
        line.update_geometry(corridor);
        line
    }

    /// A line that simply follows offset `n` (no optimisation).
    pub fn constant(corridor: &Corridor, n: f32) -> Self {
        let m = corridor.len();
        let mut line = Self {
            offset: vec![n; m],
            curvature: vec![0.0; m],
            step_len: vec![CORRIDOR_STEP_M; m],
            limits: vec![(n, n); m],
        };
        line.update_geometry(corridor);
        line
    }

    fn update_geometry(&mut self, c: &Corridor) {
        let m = c.len();
        let n = &self.offset;
        let mut raw = vec![0.0f32; m];
        for (i, out) in raw.iter_mut().enumerate() {
            let ii = i as isize;
            if !c.closed && (i == 0 || i + 1 == m) {
                continue;
            }
            let a = pt(c, ii - 1, n);
            let b = pt(c, ii, n);
            let d = pt(c, ii + 1, n);
            let ab = geom::sub(b, a);
            let bd = geom::sub(d, b);
            let ad = geom::sub(d, a);
            let den = geom::len(ab) * geom::len(bd) * geom::len(ad);
            // cross(ab, bd) > 0 = turning left (counter-clockwise); we use + = right.
            *out = if den > 1e-6 {
                -2.0 * geom::cross(ab, bd) / den
            } else {
                0.0
            };
        }
        // Light 3-tap smoothing so a single kinked sample does not dictate a speed.
        for i in 0..m {
            let ii = i as isize;
            self.curvature[i] =
                0.25 * raw[c.wrap(ii - 1)] + 0.5 * raw[i] + 0.25 * raw[c.wrap(ii + 1)];
            self.step_len[i] = geom::len(geom::sub(pt(c, ii + 1, n), pt(c, ii, n))).max(0.1);
        }
        if !c.closed && m > 0 {
            self.step_len[m - 1] = CORRIDOR_STEP_M;
        }
    }

    /// Total line length, m.
    pub fn length_m(&self, closed: bool) -> f32 {
        let n = self.step_len.len();
        let k = if closed { n } else { n.saturating_sub(1) };
        self.step_len[..k].iter().sum()
    }

    /// Largest |curvature| over the line, 1/m.
    pub fn max_curvature(&self) -> f32 {
        self.curvature.iter().fold(0.0f32, |m, k| m.max(k.abs()))
    }
}

/// Fractions of the car's capability a driver dares to use.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    /// Fraction of measured lateral grip.
    pub lateral: f32,
    /// Fraction of measured braking.
    pub braking: f32,
    /// Fraction of measured acceleration.
    pub accel: f32,
}

/// Speed below which a car's acceleration is taken as traction-limited (constant), m/s.
const ACCEL_FLAT_MPS: f32 = 22.0;

/// Full-throttle acceleration at speed `u` on the flat, m/s²: `a0` (the measured mean
/// 0–100 km/h acceleration, scaled by the driver's usage) up to [`ACCEL_FLAT_MPS`], then
/// power-limited (`∝ 1/u`) minus aerodynamic drag sized so the car tops out at its measured
/// top speed. Never below 5 % of `a0` below top speed.
pub fn drive_accel(cap: &CarCapability, a0: f32, u: f32) -> f32 {
    let top = cap.top_speed_mps.max(ACCEL_FLAT_MPS + 1.0);
    let k = a0 * ACCEL_FLAT_MPS / (top * top * top);
    let power = a0 * (ACCEL_FLAT_MPS / u.max(ACCEL_FLAT_MPS));
    let a = power - k * u * u;
    if u < top {
        a.max(0.05 * a0)
    } else {
        a
    }
}

/// Target speed per corridor sample.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeedProfile {
    /// Target speed at each sample, m/s.
    pub v: Vec<f32>,
    /// Lateral acceleration limit used, m/s².
    pub a_lat: f32,
    /// Braking deceleration used, m/s².
    pub a_brake: f32,
}

impl SpeedProfile {
    /// Build the profile for `line` on `corridor` for a car with `cap`, used at `usage`.
    /// `v_cap` caps the whole profile (cool-down laps, a limping car), m/s.
    pub fn build(
        corridor: &Corridor,
        line: &RacingLine,
        cap: &CarCapability,
        usage: &Usage,
        v_cap: f32,
    ) -> Self {
        let m = corridor.len();
        let a_lat = cap.max_lat_g * GRAVITY_MPS2 * usage.lateral;
        let a_brk = cap.brake_g * GRAVITY_MPS2 * usage.braking;
        let a_acc0 = cap.accel_mps2 * usage.accel;
        let v_top = (cap.top_speed_mps * 0.98).min(v_cap);
        let mut v: Vec<f32> = line
            .curvature
            .iter()
            .map(|k| {
                let k = k.abs();
                if k < 1e-5 {
                    v_top
                } else {
                    (a_lat / k).sqrt().min(v_top)
                }
            })
            .collect();
        let v_corner = v.clone();
        // Remaining longitudinal grip inside the friction ellipse at speed `u` on sample `i`.
        let ellipse = |i: usize, u: f32| -> f32 {
            let lat = u * u * line.curvature[i].abs() / a_lat.max(0.1);
            (1.0 - lat * lat).max(0.0).sqrt().max(0.25)
        };
        // Road grade per sample (rise over run to the next sample): gravity takes
        // `g · grade` from the drive uphill and adds it to the brakes (downhill the reverse).
        let grade: Vec<f32> = (0..m)
            .map(|i| {
                let nx = if i + 1 < m {
                    i + 1
                } else if corridor.closed {
                    0
                } else {
                    return 0.0;
                };
                let dz = corridor.samples[nx].pos[2] - corridor.samples[i].pos[2];
                (dz / CORRIDOR_STEP_M).clamp(-0.3, 0.3)
            })
            .collect();
        let passes = if corridor.closed { 2 } else { 1 };
        for _ in 0..passes {
            // Forward: acceleration.
            for j in 0..m {
                let i = j;
                let nx = if i + 1 < m {
                    i + 1
                } else if corridor.closed {
                    0
                } else {
                    break;
                };
                let u = v[i];
                let a = drive_accel(cap, a_acc0, u) * ellipse(i, u) - GRAVITY_MPS2 * grade[i];
                let lim = (u * u + 2.0 * a * line.step_len[i]).max(0.0).sqrt();
                if v[nx] > lim {
                    v[nx] = lim;
                }
            }
        }
        for _ in 0..passes {
            // Backward: braking.
            for j in (0..m).rev() {
                let i = j;
                let pv = if i > 0 {
                    i - 1
                } else if corridor.closed {
                    m - 1
                } else {
                    break;
                };
                let u = v[i];
                let a = (a_brk * ellipse(pv, v[pv].min(v_corner[pv])) + GRAVITY_MPS2 * grade[pv])
                    .max(0.25 * a_brk);
                let lim = (u * u + 2.0 * a * line.step_len[pv]).sqrt();
                if v[pv] > lim {
                    v[pv] = lim;
                }
            }
        }
        Self {
            v,
            a_lat,
            a_brake: a_brk,
        }
    }

    /// Predicted lap (or route) time on this profile, s.
    pub fn lap_time_s(&self, line: &RacingLine, closed: bool) -> f32 {
        let m = self.v.len();
        let k = if closed { m } else { m.saturating_sub(1) };
        let mut t = 0.0;
        for i in 0..k {
            let j = (i + 1) % m;
            let vm = 0.5 * (self.v[i] + self.v[j]);
            t += line.step_len[i] / vm.max(0.5);
        }
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corridor::CorridorSample;

    /// A 90° right-hand corner: 60 m north, a 15 m radius quarter circle, 60 m east. Lanes
    /// 4 m either side of the centreline.
    fn corner() -> Corridor {
        let mut samples = Vec::new();
        let mut s = 0.0;
        let r = 15.0;
        let total = 60.0 + std::f32::consts::FRAC_PI_2 * r + 60.0;
        while s <= total {
            let (pos, t) = if s < 60.0 {
                ([0.0, s, 0.0], [0.0, 1.0])
            } else if s < 60.0 + std::f32::consts::FRAC_PI_2 * r {
                let a = (s - 60.0) / r;
                let (sn, cs) = wp_math::sin_cos(a);
                ([r - r * cs, 60.0 + r * sn, 0.0], [sn, cs])
            } else {
                let d = s - 60.0 - std::f32::consts::FRAC_PI_2 * r;
                ([r + d, 60.0 + r, 0.0], [1.0, 0.0])
            };
            samples.push(CorridorSample {
                pos,
                tangent: t,
                left_m: 4.0,
                left_pass_m: 4.0,
                right_m: 4.0,
                junction: false,
            });
            s += CORRIDOR_STEP_M;
        }
        let n = samples.len();
        Corridor {
            opposing: crate::corridor::OpposingLanes::Never,
            samples,
            closed: false,
            length_m: (n - 1) as f32 * CORRIDOR_STEP_M,
            junction_lanes: Vec::new(),
        }
    }

    #[test]
    fn line_uses_the_corridor_out_in_out() {
        let c = corner();
        let line = RacingLine::optimise(&c, 0.9, &LineParams::default());
        // Centreline curvature is 1/15; the line must be straighter.
        let k = line.max_curvature();
        assert!(k < 1.0 / 15.0 * 0.85, "max curvature {k}");
        // Apex on the inside (right) mid-corner, outside (left) on entry.
        let apex = (60.0 + std::f32::consts::FRAC_PI_4 * 15.0) / CORRIDOR_STEP_M;
        assert!(
            line.offset[apex as usize] > 1.5,
            "apex {}",
            line.offset[apex as usize]
        );
        assert!(line.offset[24] < -1.0, "entry {}", line.offset[24]);
        // Inside the bounds everywhere.
        for (o, (lo, hi)) in line.offset.iter().zip(&line.limits) {
            assert!(*o >= *lo - 1e-4 && *o <= *hi + 1e-4);
        }
    }

    #[test]
    fn profile_brakes_before_the_corner() {
        let c = corner();
        let line = RacingLine::optimise(&c, 0.9, &LineParams::default());
        let cap = CarCapability {
            max_lat_g: 0.9,
            brake_g: 1.0,
            accel_mps2: 4.0,
            top_speed_mps: 55.0,
            wheelbase_m: 2.7,
            max_steer_rad: 0.6,
            length_m: 4.5,
            width_m: 1.8,
        };
        let usage = Usage {
            lateral: 0.9,
            braking: 0.9,
            accel: 1.0,
        };
        let p = SpeedProfile::build(&c, &line, &cap, &usage, 100.0);
        let apex = ((60.0 + std::f32::consts::FRAC_PI_4 * 15.0) / CORRIDOR_STEP_M) as usize;
        let v_apex = p.v[apex];
        assert!(v_apex > 9.0 && v_apex < 20.0, "apex speed {v_apex}");
        // Monotonic slowdown into the corner, speed rising after it.
        assert!(p.v[20] > v_apex + 3.0);
        assert!(p.v[c.len() - 2] > v_apex + 3.0);
        // Braking never exceeds the limit between samples.
        for i in 0..c.len() - 1 {
            let dv2 = p.v[i] * p.v[i] - p.v[i + 1] * p.v[i + 1];
            assert!(dv2 <= 2.0 * p.a_brake * line.step_len[i] + 1e-2);
        }
        // A worse driver (less grip used) is slower through the apex.
        let slow = SpeedProfile::build(
            &c,
            &line,
            &cap,
            &Usage {
                lateral: 0.7,
                ..usage
            },
            100.0,
        );
        assert!(slow.v[apex] < v_apex);
        assert!(slow.lap_time_s(&line, false) > p.lap_time_s(&line, false));
    }
}
