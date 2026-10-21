//! Highways: centrelines filleted from the plan (with the adjustments listed in the module
//! docs of [`super`]), one vertical profile and bank per highway (both carriageways share
//! the cross-section plane, as in Port Vesper v2's `Ring`), and the carriageway geometry the
//! interchanges and the builder use.
//!
//! Conventions: a highway's *path* runs along its plan control line. The `fwd` carriageway
//! (side `+1`) travels along the path on its right (lateral `+offset`), the `bwd` one
//! (side `-1`) against it on the left. Closed highways (Loop 9) wrap: arc lengths are taken
//! modulo the total.

use super::geo::{self, cum2, fillet, resample};
use super::profile;
use crate::geom::{self, V2, V3};

/// Highway lane width, m.
pub const HW_LANE_M: f32 = 3.7;
/// Paved median shoulder between the left lane and the barrier, m.
pub const MEDIAN_SHOULDER_M: f32 = 1.5;
/// Paved outer shoulder, m.
pub const OUTER_SHOULDER_M: f32 = 2.5;
/// Sample spacing of highway profiles, m.
pub const HW_DS: f32 = 5.0;

/// A designated structure span on a highway.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    /// Start arc length (may exceed the total on closed highways).
    pub s0: f32,
    /// End arc length.
    pub s1: f32,
    /// Viaduct: minimum height above the ground (or water); tunnel: cover depth, m.
    pub h: f32,
    /// Name.
    pub name: &'static str,
}

/// A highway.
#[derive(Debug, Clone)]
pub struct Hw {
    /// Name.
    pub name: &'static str,
    /// Centreline (closed: the last point is not repeated).
    pub path: Vec<V2>,
    /// Cumulative arc length (closed: includes the closing segment as the last entry).
    pub cum: Vec<f32>,
    /// Closed loop.
    pub closed: bool,
    /// Speed limit per lane-count span `(s_from, lanes, speed)`, sorted by `s_from`.
    pub lanes: Vec<(f32, u8, f32)>,
    /// Carriageway centreline offset from the highway centreline, m.
    pub offset: f32,
    /// Elevated spans (viaducts).
    pub viaducts: Vec<Span>,
    /// Tunnel spans (covered).
    pub tunnels: Vec<Span>,
    /// The start ends at a gate stub (pinned to the ground there).
    pub start_gate: Option<V2>,
    /// The end ends at a gate stub.
    pub end_gate: Option<V2>,
    /// Profile: height per sample (`HW_DS` apart; closed: wraps).
    pub z: Vec<f32>,
    /// Bank per sample (cross slope, dz per metre right of the path direction).
    pub bank: Vec<f32>,
}

impl Hw {
    /// A highway along a filleted control polyline.
    pub fn new(
        name: &'static str,
        ctrl: &[V2],
        radii: &[f32],
        closed: bool,
        lanes: u8,
        speed: f32,
    ) -> Self {
        let mut path = resample(&fillet(ctrl, radii, closed, 4.0), 8.0);
        if closed {
            path.pop(); // fillet repeats nothing, resample keeps both ends: drop the duplicate
            if geom::dist(path[0], path[path.len() - 1]) < 1.0 {
                path.pop();
            }
        }
        let mut c = path.clone();
        if closed {
            c.push(path[0]);
        }
        let cum = cum2(&c);
        Self {
            name,
            path,
            cum,
            closed,
            lanes: vec![(0.0, lanes, speed)],
            offset: MEDIAN_SHOULDER_M + lanes as f32 * HW_LANE_M * 0.5,
            viaducts: Vec::new(),
            tunnels: Vec::new(),
            start_gate: None,
            end_gate: None,
            z: Vec::new(),
            bank: Vec::new(),
        }
    }

    /// Total length, m.
    pub fn total(&self) -> f32 {
        self.cum[self.cum.len() - 1]
    }

    fn pts(&self) -> std::borrow::Cow<'_, [V2]> {
        if self.closed {
            let mut c = self.path.clone();
            c.push(self.path[0]);
            std::borrow::Cow::Owned(c)
        } else {
            std::borrow::Cow::Borrowed(&self.path)
        }
    }

    /// Wrap (closed) or clamp (open) an arc length.
    pub fn wrap(&self, s: f32) -> f32 {
        if self.closed {
            s.rem_euclid(self.total())
        } else {
            s.clamp(0.0, self.total())
        }
    }

    /// Point and unit tangent at `s`.
    pub fn at(&self, s: f32) -> (V2, V2) {
        let p = self.pts();
        geo::point_on(&p, &self.cum, self.wrap(s))
    }

    /// Arc length and signed lateral offset (+ right of the path) of the closest point.
    pub fn project(&self, q: V2) -> (f32, f32, f32) {
        let p = self.pts();
        geo::project_signed(&p, &self.cum, q)
    }

    /// Lanes per carriageway and speed at `s`.
    pub fn lanes_at(&self, s: f32) -> (u8, f32) {
        let s = self.wrap(s);
        let mut out = (self.lanes[0].1, self.lanes[0].2);
        for &(from, n, v) in &self.lanes {
            if s >= from {
                out = (n, v);
            }
        }
        out
    }

    /// Profile height at `s` (0 before profiling).
    pub fn z_at(&self, s: f32) -> f32 {
        if self.z.is_empty() {
            return 0.0;
        }
        let s = self.wrap(s);
        if self.closed {
            let n = self.z.len();
            let f = s / HW_DS;
            let i = (f.floor() as usize) % n;
            let t = f - f.floor();
            self.z[i] + (self.z[(i + 1) % n] - self.z[i]) * t
        } else {
            profile::sample(&self.z, self.sample_ds(), s)
        }
    }

    /// Bank at `s`.
    pub fn bank_at(&self, s: f32) -> f32 {
        if self.bank.is_empty() {
            return 0.0;
        }
        let s = self.wrap(s);
        if self.closed {
            let n = self.bank.len();
            let f = s / HW_DS;
            let i = (f.floor() as usize) % n;
            let t = f - f.floor();
            self.bank[i] + (self.bank[(i + 1) % n] - self.bank[i]) * t
        } else {
            profile::sample(&self.bank, self.sample_ds(), s)
        }
    }

    /// Sample spacing actually used (open highways fit an integer number of samples).
    pub fn sample_ds(&self) -> f32 {
        if self.closed {
            self.total() / self.sample_count() as f32
        } else {
            self.total() / (self.sample_count() - 1).max(1) as f32
        }
    }

    /// Number of profile samples.
    pub fn sample_count(&self) -> usize {
        if self.closed {
            ((self.total() / HW_DS).round() as usize).max(3)
        } else {
            ((self.total() / HW_DS).ceil() as usize).max(1) + 1
        }
    }

    /// Point on the cross-section plane at `s`, `lat` metres right of the path.
    pub fn offset_pt(&self, s: f32, lat: f32) -> V3 {
        let (p, d) = self.at(s);
        let q = geom::add(p, geom::scale(geom::right_of(d), lat));
        [q[0], q[1], self.z_at(s) + self.bank_at(s) * lat]
    }

    /// 2D point `lat` metres right of the path at `s`.
    pub fn offset2(&self, s: f32, lat: f32) -> V2 {
        let (p, d) = self.at(s);
        geom::add(p, geom::scale(geom::right_of(d), lat))
    }

    /// Height of the cross-section plane under `q` (nearest centreline point).
    pub fn plane_z(&self, q: V2) -> f32 {
        let (s, lat, _) = self.project(q);
        self.z_at(s) + self.bank_at(s) * lat
    }

    /// Travel direction of carriageway `side` at `s`.
    pub fn travel_dir(&self, side: f32, s: f32) -> V2 {
        geom::scale(self.at(s).1, side)
    }

    /// Lane centre offset of a ramp lane beside carriageway `side` at `s` (from the highway
    /// centreline, signed).
    pub fn ramp_lane_lat(&self, side: f32, s: f32) -> f32 {
        let (n, _) = self.lanes_at(s);
        side * (self.offset + (n as f32 * 0.5 + 0.5) * HW_LANE_M)
    }

    /// Half width of the whole highway (both carriageways and shoulders), m.
    pub fn half_width(&self) -> f32 {
        let n = self.lanes.iter().map(|l| l.1).max().unwrap_or(2);
        self.offset + n as f32 * HW_LANE_M * 0.5 + OUTER_SHOULDER_M
    }

    /// Whether `s` lies in a span list (handles wrap on closed highways).
    pub fn in_spans(&self, spans: &[Span], s: f32) -> Option<Span> {
        let t = self.total();
        spans.iter().copied().find(|sp| {
            if self.closed {
                let s = s.rem_euclid(t);
                (s >= sp.s0 && s <= sp.s1) || (s + t >= sp.s0 && s + t <= sp.s1)
            } else {
                s >= sp.s0 && s <= sp.s1
            }
        })
    }

    /// Arc length of the closest point to `q`.
    pub fn s_of(&self, q: V2) -> f32 {
        self.project(q).0
    }

    /// Arc distance from `a` to `b` along the path (closed: shortest signed).
    pub fn delta(&self, a: f32, b: f32) -> f32 {
        if self.closed {
            let t = self.total();
            let d = (b - a).rem_euclid(t);
            if d > t * 0.5 {
                d - t
            } else {
                d
            }
        } else {
            b - a
        }
    }
}

/// Samples of a carriageway between arc lengths (in travel order), `step` metres apart:
/// `(s, point on the plane, bank right of travel)`.
pub fn carriageway_samples(
    hw: &Hw,
    side: f32,
    s_from: f32,
    s_to: f32,
    lat: &dyn Fn(f32) -> f32,
    step: f32,
) -> Vec<(f32, V3, f32)> {
    let d = s_to - s_from;
    let n = ((d.abs() / step).ceil() as usize).max(1);
    (0..=n)
        .map(|k| {
            let s = s_from + d * k as f32 / n as f32;
            let l = lat(s);
            (s, hw.offset_pt(s, side * l), side * hw.bank_at(s))
        })
        .collect()
}
