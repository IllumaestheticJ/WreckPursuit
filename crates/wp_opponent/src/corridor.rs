//! Race corridor: a route's centreline resampled every [`CORRIDOR_STEP_M`] with how far a
//! car may move to either side and still be on paved driving lanes.
//!
//! Coordinates: `s` = arc length along the centreline (m, wraps on circuits), `n` = lateral
//! offset (m, + = right of travel). The bounds come from the route lane's same-direction
//! neighbours (`Lane::left/right` chains — parking lanes are never part of a chain), plus the
//! opposing lanes of the same road when the event closes the road ([`CorridorOptions`]).
//! Inside junctions (connectors) the bounds blend from the arriving lane to the leaving lane.

use wp_world::geom::{self, V2, V3};
use wp_world::map::{CityMap, SegKind};
use wp_world::Route;

/// Resampling step, m.
pub const CORRIDOR_STEP_M: f32 = 2.0;

/// Whether racers may use the opposing lanes of two-way roads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpposingLanes {
    /// Never (law-abiding events).
    Never,
    /// Only to overtake, when no oncoming traffic is in the way (open-road street races;
    /// drivers with `risk` ≥ 0.5).
    ForOvertaking,
    /// Always part of the track (closed-road events).
    Always,
}

/// How the corridor is derived from the road.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CorridorOptions {
    /// Use of the opposing lanes of two-way roads.
    pub opposing: OpposingLanes,
    /// Extra width kept clear of every kerb, m.
    pub kerb_margin_m: f32,
}

impl Default for CorridorOptions {
    fn default() -> Self {
        Self {
            opposing: OpposingLanes::Never,
            kerb_margin_m: 0.3,
        }
    }
}

/// One corridor sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CorridorSample {
    /// Centreline point, m.
    pub pos: V3,
    /// Unit travel direction.
    pub tangent: V2,
    /// Drivable width to the left of the centreline (positive), m — the racing width (own
    /// direction, plus the opposing lanes with [`OpposingLanes::Always`]).
    pub left_m: f32,
    /// Width to the left usable for overtaking (includes the opposing lanes unless
    /// [`OpposingLanes::Never`]), m.
    pub left_pass_m: f32,
    /// Drivable width to the right of the centreline (positive), m.
    pub right_m: f32,
    /// Inside a junction box.
    pub junction: bool,
}

impl CorridorSample {
    /// Unit right normal.
    pub fn normal(&self) -> V2 {
        geom::right_of(self.tangent)
    }

    /// World point at lateral offset `n`.
    pub fn at(&self, n: f32) -> V3 {
        let r = self.normal();
        [self.pos[0] + r[0] * n, self.pos[1] + r[1] * n, self.pos[2]]
    }
}

/// Where a point sits in corridor coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CorridorPos {
    /// Arc length, m (`0..length` on circuits).
    pub s: f32,
    /// Lateral offset (+ right), m.
    pub n: f32,
    /// Nearest sample index.
    pub index: usize,
}

/// A sampled race corridor.
#[derive(Debug, Clone, PartialEq)]
pub struct Corridor {
    /// Samples, `CORRIDOR_STEP_M` apart (the last one of a circuit is one step before the
    /// first).
    pub samples: Vec<CorridorSample>,
    /// Closed loop.
    pub closed: bool,
    /// Total length, m.
    pub length_m: f32,
    /// Use of the opposing lanes this corridor was built with.
    pub opposing: OpposingLanes,
}

/// `(left racing, left for overtaking, right)` extents of a lane, kerb margin applied.
fn lane_extents(map: &CityMap, lane: u32, opts: &CorridorOptions) -> (f32, f32, f32) {
    let lanes = &map.net.lanes;
    let me = &lanes[lane as usize];
    let mut left = me.width_m * 0.5;
    let mut right = me.width_m * 0.5;
    let mut cur = me.left;
    let mut leftmost = lane;
    while let Some(c) = cur {
        left += lanes[c as usize].width_m;
        leftmost = c;
        cur = lanes[c as usize].left;
    }
    let mut cur = me.right;
    while let Some(c) = cur {
        right += lanes[c as usize].width_m;
        cur = lanes[c as usize].right;
    }
    let mut left_pass = left;
    if opts.opposing != OpposingLanes::Never && !lanes[leftmost as usize].edge_left {
        // Opposing driving lanes of the same road (all of them sit left of our leftmost lane
        // in right-hand traffic).
        let road = me.road;
        let dir = lane_dir(map, lane);
        let opp: f32 = lanes
            .iter()
            .enumerate()
            .filter(|(i, l)| {
                l.road == road
                    && l.kind.drivable()
                    && geom::dot(lane_dir(map, *i as u32), dir) < -0.5
            })
            .map(|(_, l)| l.width_m)
            .sum();
        left_pass += opp;
        if opts.opposing == OpposingLanes::Always {
            left += opp;
        }
    }
    let k = opts.kerb_margin_m;
    (
        (left - k).max(0.5),
        (left_pass - k).max(0.5),
        (right - k).max(0.5),
    )
}

fn lane_dir(map: &CityMap, lane: u32) -> V2 {
    let p = &map.net.lanes[lane as usize].polyline;
    geom::normalize(geom::sub(geom::xy(p[p.len() - 1]), geom::xy(p[0])))
}

/// One route segment piece: seg, start/end arc length on the seg.
struct Piece {
    seg: u32,
    a: f32,
    b: f32,
}

impl Corridor {
    /// Build the corridor of a `wp_world` route.
    pub fn from_route(map: &CityMap, route: &Route, opts: &CorridorOptions) -> Self {
        let n = route.segs.len();
        assert!(n > 0, "empty route");
        let first = map.seg(route.segs[0]);
        let last = map.seg(route.segs[n - 1]);
        let s_start = first
            .project(geom::xy(route.polyline[0]))
            .s
            .clamp(0.0, first.length);
        let s_end = last
            .project(geom::xy(*route.polyline.last().unwrap()))
            .s
            .clamp(0.0, last.length);
        let pieces: Vec<Piece> = route
            .segs
            .iter()
            .enumerate()
            .map(|(k, &seg)| Piece {
                seg,
                a: if k == 0 { s_start } else { 0.0 },
                b: if k == n - 1 {
                    s_end
                } else {
                    map.seg(seg).length
                },
            })
            .filter(|p| p.b - p.a > 1e-3)
            .collect();
        // Extents per lane piece (connectors get NaN and are blended afterwards).
        // (pos, tangent, left, left_pass, right, junction)
        let mut pts: Vec<(V3, V2, f32, f32, f32, bool)> = Vec::new();
        let mut carry = 0.0f32;
        for p in &pieces {
            let seg = map.seg(p.seg);
            let ext = match seg.kind {
                SegKind::Lane { lane } => Some(lane_extents(map, lane, opts)),
                SegKind::Connector { .. } => None,
            };
            let mut s = p.a + carry;
            while s < p.b {
                let (pos, t) = seg.point_at(s);
                let (l, lp, r) = ext.unwrap_or((f32::NAN, f32::NAN, f32::NAN));
                pts.push((pos, t, l, lp, r, ext.is_none()));
                s += CORRIDOR_STEP_M;
            }
            carry = s - p.b;
        }
        if !route.closed_loop {
            let lp = pieces.last().unwrap();
            let seg = map.seg(lp.seg);
            let (pos, t) = seg.point_at(lp.b);
            let ext = seg.lane().map(|l| lane_extents(map, l, opts)).unwrap_or((
                f32::NAN,
                f32::NAN,
                f32::NAN,
            ));
            pts.push((pos, t, ext.0, ext.1, ext.2, seg.is_connector()));
        }
        // Blend connector extents between the neighbouring lane samples.
        let m = pts.len();
        let closed = route.closed_loop;
        let mut i = 0;
        while i < m {
            if !pts[i].2.is_nan() {
                i += 1;
                continue;
            }
            let start = i;
            while i < m && pts[i].2.is_nan() {
                i += 1;
            }
            let end = i; // exclusive
            let before = if start > 0 {
                Some(start - 1)
            } else if closed {
                (0..m).rev().find(|&k| !pts[k].2.is_nan())
            } else {
                None
            };
            let after = if end < m {
                Some(end)
            } else if closed {
                (0..m).find(|&k| !pts[k].2.is_nan())
            } else {
                None
            };
            let fallback = (3.0, 3.0, 3.0);
            let eb = before
                .map(|k| (pts[k].2, pts[k].3, pts[k].4))
                .unwrap_or(fallback);
            let ea = after.map(|k| (pts[k].2, pts[k].3, pts[k].4)).unwrap_or(eb);
            let eb = if before.is_none() { ea } else { eb };
            let len = (end - start + 1) as f32;
            for (j, p) in pts[start..end].iter_mut().enumerate() {
                let t = (j + 1) as f32 / len;
                p.2 = eb.0 + (ea.0 - eb.0) * t;
                p.3 = eb.1 + (ea.1 - eb.1) * t;
                p.4 = eb.2 + (ea.2 - eb.2) * t;
            }
        }
        let samples: Vec<CorridorSample> = pts
            .into_iter()
            .map(|(pos, tangent, l, lp, r, j)| CorridorSample {
                pos,
                tangent,
                left_m: l,
                left_pass_m: lp,
                right_m: r,
                junction: j,
            })
            .collect();
        let length_m = if closed {
            samples.len() as f32 * CORRIDOR_STEP_M
        } else {
            (samples.len().saturating_sub(1)) as f32 * CORRIDOR_STEP_M
        };
        Self {
            samples,
            closed,
            length_m,
            opposing: opts.opposing,
        }
    }

    /// Number of samples.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// No samples.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Sample index `i` wrapped (circuits) or clamped (open routes).
    pub fn wrap(&self, i: isize) -> usize {
        let n = self.samples.len() as isize;
        if self.closed {
            i.rem_euclid(n) as usize
        } else {
            i.clamp(0, n - 1) as usize
        }
    }

    /// Sample at index `i` (wrapped/clamped).
    pub fn sample(&self, i: isize) -> &CorridorSample {
        &self.samples[self.wrap(i)]
    }

    /// Wrap an arc length into `0..length` (circuits) or clamp it (open routes).
    pub fn wrap_s(&self, s: f32) -> f32 {
        if self.closed {
            let l = self.length_m;
            let r = s - l * (s / l).floor();
            if r >= l {
                0.0
            } else {
                r
            }
        } else {
            s.clamp(0.0, self.length_m)
        }
    }

    /// Signed arc-length difference `b − a`, shortest way round on circuits.
    pub fn delta_s(&self, a: f32, b: f32) -> f32 {
        let d = b - a;
        if self.closed {
            let l = self.length_m;
            d - l * (d / l + 0.5).floor()
        } else {
            d
        }
    }

    /// Linear interpolation of a per-sample value at arc length `s`.
    pub fn interp(&self, values: &[f32], s: f32) -> f32 {
        let s = self.wrap_s(s);
        let x = s / CORRIDOR_STEP_M;
        let i = x.floor() as isize;
        let t = x - i as f32;
        let a = values[self.wrap(i)];
        let b = values[self.wrap(i + 1)];
        a + (b - a) * t
    }

    /// World point at corridor coordinates (`s`, `n`).
    pub fn point(&self, s: f32, n: f32) -> V3 {
        let s = self.wrap_s(s);
        let x = s / CORRIDOR_STEP_M;
        let i = x.floor() as isize;
        let t = x - i as f32;
        let a = self.sample(i).at(n);
        let b = self.sample(i + 1).at(n);
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ]
    }

    /// Left/right bounds (positive widths) at `s`, interpolated.
    pub fn bounds(&self, s: f32) -> (f32, f32) {
        let s = self.wrap_s(s);
        let x = s / CORRIDOR_STEP_M;
        let i = x.floor() as isize;
        let t = x - i as f32;
        let a = self.sample(i);
        let b = self.sample(i + 1);
        (
            a.left_m + (b.left_m - a.left_m) * t,
            a.right_m + (b.right_m - a.right_m) * t,
        )
    }

    /// Left/right widths usable for overtaking at `s` (left includes the opposing lanes
    /// unless [`OpposingLanes::Never`]).
    pub fn pass_bounds(&self, s: f32) -> (f32, f32) {
        let s = self.wrap_s(s);
        let x = s / CORRIDOR_STEP_M;
        let i = x.floor() as isize;
        let t = x - i as f32;
        let a = self.sample(i);
        let b = self.sample(i + 1);
        (
            a.left_pass_m + (b.left_pass_m - a.left_pass_m) * t,
            a.right_m + (b.right_m - a.right_m) * t,
        )
    }

    /// Narrowest bounds over `[s0, s0 + span]`.
    pub fn min_bounds(&self, s0: f32, span: f32) -> (f32, f32) {
        let i0 = (self.wrap_s(s0) / CORRIDOR_STEP_M).floor() as isize;
        let k = (span / CORRIDOR_STEP_M).ceil().max(1.0) as isize;
        let mut l = f32::INFINITY;
        let mut r = f32::INFINITY;
        for j in 0..=k {
            let c = self.sample(i0 + j);
            l = l.min(c.left_m);
            r = r.min(c.right_m);
        }
        (l, r)
    }

    /// Project a horizontal point. With `hint`, only samples within `window` of it are
    /// searched (cheap tracking); otherwise all samples.
    pub fn project(&self, p: V2, hint: Option<usize>, window: usize) -> CorridorPos {
        let n = self.samples.len() as isize;
        let (lo, hi) = match hint {
            Some(h) if (2 * window as isize + 1) < n => {
                (h as isize - window as isize, h as isize + window as isize)
            }
            _ => (0, n - 1),
        };
        let mut best = (f32::INFINITY, 0usize);
        let mut i = lo;
        while i <= hi {
            let k = self.wrap(i);
            let c = &self.samples[k];
            let d = geom::sub(p, geom::xy(c.pos));
            let d2 = geom::dot(d, d);
            if d2 < best.0 {
                best = (d2, k);
            }
            i += 1;
        }
        let k = best.1;
        let c = &self.samples[k];
        let d = geom::sub(p, geom::xy(c.pos));
        let along = geom::dot(d, c.tangent);
        let s = self.wrap_s(k as f32 * CORRIDOR_STEP_M + along);
        CorridorPos {
            s,
            n: geom::dot(d, c.normal()),
            index: k,
        }
    }

    /// Signed centreline curvature at `s` (1/m, + = turning right), from the tangents of the
    /// samples around it (±3 samples).
    pub fn centre_curvature(&self, s: f32) -> f32 {
        let i = (self.wrap_s(s) / CORRIDOR_STEP_M).floor() as isize;
        let a = self.sample(i - 3).tangent;
        let b = self.sample(i + 3).tangent;
        // cross(a, b) > 0 = counter-clockwise (left); + = right here.
        -geom::cross(a, b) / (6.0 * CORRIDOR_STEP_M)
    }

    /// Distance along the corridor from `s` to the next point where the corridor turns by more
    /// than `angle_rad` within `window_m`, searched up to `horizon_m` ahead (None if none).
    pub fn next_turn(
        &self,
        s: f32,
        horizon_m: f32,
        window_m: f32,
        angle_rad_cos: f32,
    ) -> Option<f32> {
        let i0 = (self.wrap_s(s) / CORRIDOR_STEP_M).floor() as isize;
        let w = (window_m / CORRIDOR_STEP_M).ceil() as isize;
        let k = (horizon_m / CORRIDOR_STEP_M).ceil() as isize;
        for j in 0..k {
            if !self.closed && i0 + j + w >= self.samples.len() as isize {
                return None;
            }
            let a = self.sample(i0 + j).tangent;
            let b = self.sample(i0 + j + w).tangent;
            if geom::dot(a, b) < angle_rad_cos {
                return Some(j as f32 * CORRIDOR_STEP_M);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight corridor along +Y, `len` m, 3 m each side.
    pub(crate) fn straight(len: f32, closed: bool) -> Corridor {
        let n = (len / CORRIDOR_STEP_M) as usize;
        let samples = (0..n)
            .map(|i| CorridorSample {
                pos: [0.0, i as f32 * CORRIDOR_STEP_M, 0.0],
                tangent: [0.0, 1.0],
                left_m: 3.0,
                left_pass_m: 3.0,
                right_m: 3.0,
                junction: false,
            })
            .collect::<Vec<_>>();
        Corridor {
            opposing: OpposingLanes::Never,
            length_m: if closed {
                n as f32 * CORRIDOR_STEP_M
            } else {
                (n - 1) as f32 * CORRIDOR_STEP_M
            },
            samples,
            closed,
        }
    }

    #[test]
    fn wrap_and_delta() {
        let c = straight(100.0, true);
        assert_eq!(c.wrap_s(105.0), 5.0);
        assert_eq!(c.wrap_s(-5.0), 95.0);
        assert_eq!(c.delta_s(95.0, 5.0), 10.0);
        assert_eq!(c.delta_s(5.0, 95.0), -10.0);
    }

    #[test]
    fn project_round_trips() {
        let c = straight(100.0, false);
        let p = c.point(31.0, 1.5);
        let q = c.project(geom::xy(p), None, 0);
        assert!(
            (q.s - 31.0).abs() < 1e-3 && (q.n - 1.5).abs() < 1e-3,
            "{q:?}"
        );
        let q = c.project(geom::xy(p), Some(10), 20);
        assert!((q.s - 31.0).abs() < 1e-3);
    }
}
