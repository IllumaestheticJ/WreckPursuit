//! Street *lines*: the raw polylines of every street before they become a planar graph,
//! with their grade-separated ranges (bridges, underpasses, tunnels, overpasses), forced
//! split points (ramp terminals) and the clipping that keeps them out of water, sites, rail
//! corridors, highway corridors and interchanges.

use super::geo::{self, cum2, point_on, sub_path};
use super::style::Style;
use crate::geom::{self, V2};

/// What a grade-separated range of a street does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SepKind {
    /// Bridge deck over water: the core stays at or above `bound` (deck height).
    Bridge,
    /// Overpass (over a rail line): the core stays at or above `bound`.
    Over,
    /// Underpass / covered trench / tunnel: the core stays `bound` metres below the ground.
    Under,
}

/// A range of a line without at-grade junctions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sep {
    /// Whole range (core + approach ramps), arc lengths on the line.
    pub s0: f32,
    /// End of the whole range.
    pub s1: f32,
    /// Core range start (where the bound applies).
    pub core0: f32,
    /// Core range end.
    pub core1: f32,
    /// Kind.
    pub kind: SepKind,
    /// Deck height (Bridge / Over) or cover depth below the ground (Under), m.
    pub bound: f32,
    /// Structure name (empty for automatic ones).
    pub name: &'static str,
    /// Covered (a tunnel roof is built over the core).
    pub covered: bool,
    /// Lanes each way on the structure (0 = the line's own).
    pub lanes: u8,
}

impl Sep {
    /// Shift by `-ds` (re-base onto a piece starting at `ds`).
    pub fn shifted(&self, ds: f32) -> Sep {
        Sep {
            s0: self.s0 - ds,
            s1: self.s1 - ds,
            core0: self.core0 - ds,
            core1: self.core1 - ds,
            ..*self
        }
    }

    /// Whether `s` lies in the whole range.
    pub fn contains(&self, s: f32) -> bool {
        s >= self.s0 - 0.01 && s <= self.s1 + 0.01
    }
}

/// A street polyline.
#[derive(Debug, Clone)]
pub struct Line {
    /// Polyline (segments ≤ 12 m).
    pub path: Vec<V2>,
    /// Cumulative arc length of `path`.
    pub cum: Vec<f32>,
    /// Style.
    pub st: Style,
    /// Name.
    pub name: &'static str,
    /// Grade-separated ranges.
    pub seps: Vec<Sep>,
    /// Forced junction points (arc lengths): ramp terminals.
    pub forced: Vec<f32>,
    /// Gate access road: (site index, gate index). The line ends at the gate.
    pub gate: Option<(usize, usize)>,
    /// Do not trim dangling ends (gate access, map-edge roads, cul-de-sacs).
    pub keep_ends: bool,
    /// Other lines are clipped within this distance (protected scenic / touge roads), m.
    pub protect: f32,
    /// Origin id (the layout line this piece came from).
    pub origin: usize,
    /// May cross rail lines at grade.
    pub rail_ok: bool,
    /// Crossroad of an interchange (not clipped by interchange footprints).
    pub crossroad: bool,
    /// Arc length of this piece's start on its origin line.
    pub base_s: f32,
}

impl Line {
    /// A line from a polyline (resampled to ≤ 12 m segments).
    pub fn new(path: Vec<V2>, st: Style, name: &'static str) -> Self {
        let path = densify(&path, 12.0);
        let cum = cum2(&path);
        let rail_ok = st.class.rank() >= 2;
        Self {
            path,
            cum,
            st,
            name,
            seps: Vec::new(),
            forced: Vec::new(),
            gate: None,
            keep_ends: false,
            protect: 0.0,
            origin: 0,
            rail_ok,
            crossroad: false,
            base_s: 0.0,
        }
    }

    /// Length, m.
    pub fn len(&self) -> f32 {
        self.cum[self.cum.len() - 1]
    }

    /// Whether the line is degenerate.
    pub fn is_empty(&self) -> bool {
        self.path.len() < 2
    }

    /// Point and tangent at `s`.
    pub fn at(&self, s: f32) -> (V2, V2) {
        point_on(&self.path, &self.cum, s)
    }

    /// The grade-separated range containing `s`, if any.
    pub fn sep_at(&self, s: f32) -> Option<&Sep> {
        self.seps.iter().find(|r| r.contains(s))
    }

    /// Piece `[s0, s1]` of this line with its seps / forced points re-based (seps that do not
    /// fit wholly inside are dropped).
    pub fn piece(&self, s0: f32, s1: f32) -> Line {
        let path = sub_path(&self.path, &self.cum, s0, s1);
        let cum = cum2(&path);
        let len = cum[cum.len() - 1];
        Line {
            seps: self
                .seps
                .iter()
                .filter(|r| r.s0 >= s0 - 0.5 && r.s1 <= s1 + 0.5)
                .map(|r| {
                    let mut q = r.shifted(s0);
                    q.s0 = q.s0.max(0.0);
                    q.s1 = q.s1.min(len);
                    q
                })
                .collect(),
            forced: self
                .forced
                .iter()
                .filter(|f| **f > s0 && **f < s1)
                .map(|f| f - s0)
                .collect(),
            path,
            cum,
            base_s: self.base_s + s0,
            ..self.clone()
        }
    }
}

/// Split segments longer than `max` metres.
pub fn densify(p: &[V2], max: f32) -> Vec<V2> {
    let mut out = vec![p[0]];
    for w in p.windows(2) {
        let l = geom::dist(w[0], w[1]);
        if l < 0.05 {
            continue;
        }
        let n = (l / max).ceil().max(1.0) as usize;
        for k in 1..=n {
            let t = k as f32 / n as f32;
            out.push([
                w[0][0] + (w[1][0] - w[0][0]) * t,
                w[0][1] + (w[1][1] - w[0][1]) * t,
            ]);
        }
    }
    out
}

/// Merge overlapping `(s0, s1)` ranges.
pub fn merge_ranges(mut r: Vec<(f32, f32)>) -> Vec<(f32, f32)> {
    r.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f32, f32)> = Vec::new();
    for (a, b) in r {
        if let Some(last) = out.last_mut() {
            if a <= last.1 {
                last.1 = last.1.max(b);
                continue;
            }
        }
        out.push((a, b));
    }
    out
}

/// Remove the ranges `bad` from a line; pieces shorter than `min_len` are dropped. Bad
/// ranges that would cut through a grade-separated core are trimmed to leave the core
/// whole (the structure decides).
pub fn cut(line: &Line, bad: &[(f32, f32)], min_len: f32) -> Vec<Line> {
    if bad.is_empty() {
        return vec![line.clone()];
    }
    let len = line.len();
    let bad = merge_ranges(
        bad.iter()
            .map(|(a, b)| (a.max(0.0), b.min(len)))
            .filter(|(a, b)| b > a)
            .collect(),
    );
    let mut keep = Vec::new();
    let mut at = 0.0;
    for (a, b) in &bad {
        if *a > at {
            keep.push((at, *a));
        }
        at = at.max(*b);
    }
    if at < len {
        keep.push((at, len));
    }
    keep.into_iter()
        .filter(|(a, b)| b - a >= min_len)
        .map(|(a, b)| {
            let mut p = line.piece(a, b);
            // A cut end is a dead end: never a gate end.
            if line.gate.is_some() && b < len - 0.5 {
                p.gate = None;
            }
            p
        })
        .collect()
}

/// Sample positions along a line every `step` metres (both ends included): `(s, point)`.
pub fn samples(line: &Line, step: f32) -> Vec<(f32, V2)> {
    let len = line.len();
    let n = ((len / step).ceil() as usize).max(1);
    (0..=n)
        .map(|k| {
            let s = len * k as f32 / n as f32;
            (s, line.at(s).0)
        })
        .collect()
}

/// Ranges of `line` where `bad(point)` holds (sampled every `step` m), grown by `grow` m.
pub fn bad_ranges(
    line: &Line,
    step: f32,
    grow: f32,
    bad: &dyn Fn(f32, V2) -> bool,
) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    let mut start: Option<f32> = None;
    let pts = samples(line, step);
    for (k, (s, p)) in pts.iter().enumerate() {
        let b = bad(*s, *p);
        match (b, start) {
            (true, None) => start = Some(*s),
            (false, Some(a)) => {
                out.push((a - grow, pts[k - 1].0 + grow));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(a) = start {
        out.push((a - grow, line.len() + grow));
    }
    out
}

/// Crossings of a line with a polyline: `(s on line, s on other, point, sin of angle)`.
pub fn crossings_with(line: &Line, other: &[V2]) -> Vec<(f32, f32, V2, f32)> {
    let oc = cum2(other);
    geo::path_crossings(&line.path, other)
        .into_iter()
        .map(|(sa, sb, p)| {
            let (_, da) = line.at(sa);
            let (_, db) = point_on(other, &oc, sb);
            (sa, sb, p, geom::cross(da, db).abs())
        })
        .collect()
}
