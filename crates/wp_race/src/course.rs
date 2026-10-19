//! Race geometry: checkpoint gates, the route corridor, start grids and gate-crossing tests.
//!
//! All geometry is 2D in the world XY plane (Z is carried for spawning only). Units: m.

use serde::{Deserialize, Serialize};

/// 2D vector helpers (world XY).
pub(crate) mod v2 {
    /// `a - b`.
    pub fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
        [a[0] - b[0], a[1] - b[1]]
    }
    /// Dot product.
    pub fn dot(a: [f32; 2], b: [f32; 2]) -> f32 {
        a[0] * b[0] + a[1] * b[1]
    }
    /// Length.
    pub fn len(a: [f32; 2]) -> f32 {
        dot(a, a).sqrt()
    }
    /// Unit vector (or +Y for a zero vector).
    pub fn norm(a: [f32; 2]) -> [f32; 2] {
        let l = len(a);
        if l > 1e-6 {
            [a[0] / l, a[1] / l]
        } else {
            [0.0, 1.0]
        }
    }
    /// Right-hand perpendicular of a direction (+Y forward → +X right).
    pub fn right(d: [f32; 2]) -> [f32; 2] {
        [d[1], -d[0]]
    }
    /// XY part of a 3D point.
    pub fn xy(p: [f32; 3]) -> [f32; 2] {
        [p[0], p[1]]
    }
}

/// A checkpoint gate: a line segment across the road that must be crossed in `dir`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Gate {
    /// Gate centre in world space, m.
    pub pos: [f32; 3],
    /// Unit travel direction through the gate (world XY).
    pub dir: [f32; 2],
    /// Gate width, m (full width across the road).
    pub width_m: f32,
}

/// A validated crossing of a gate between two samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossing {
    /// Fraction `0..=1` of the way from the earlier to the later sample where the line was
    /// crossed (used for sub-tick finish timing).
    pub frac: f32,
    /// Lateral offset of the crossing point from the gate centre, m (absolute).
    pub lateral_m: f32,
}

impl Gate {
    /// Test whether the straight path `p0 → p1` crosses this gate forwards. `tolerance_m`
    /// widens the gate on both sides (the judge scales it with speed so a coarse sample rate
    /// cannot make a car "miss" a gate it drove through).
    pub fn crossing(&self, p0: [f32; 3], p1: [f32; 3], tolerance_m: f32) -> Option<Crossing> {
        let c = v2::xy(self.pos);
        let s0 = v2::dot(v2::sub(v2::xy(p0), c), self.dir);
        let s1 = v2::dot(v2::sub(v2::xy(p1), c), self.dir);
        if !(s0 < 0.0 && s1 >= 0.0) {
            return None;
        }
        let frac = (-s0 / (s1 - s0)).clamp(0.0, 1.0);
        let q = [
            p0[0] + (p1[0] - p0[0]) * frac,
            p0[1] + (p1[1] - p0[1]) * frac,
        ];
        let lateral_m = v2::dot(v2::sub(q, c), v2::right(self.dir)).abs();
        if lateral_m <= self.width_m * 0.5 + tolerance_m {
            Some(Crossing { frac, lateral_m })
        } else {
            None
        }
    }
}

/// Where a point lies relative to the course centreline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection {
    /// Index of the centreline segment (`polyline[i] → polyline[i+1]`).
    pub segment: usize,
    /// Distance along the course, m.
    pub along_m: f32,
    /// Distance from the centreline, m.
    pub off_m: f32,
}

/// Start-grid layout.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum GridLayout {
    /// Two columns, staggered rows behind the start line (sprint, circuit, delivery, …).
    Staggered {
        /// Distance between rows, m.
        row_gap_m: f32,
        /// Distance between the two columns, m.
        column_gap_m: f32,
    },
    /// Side by side on the start line (drag, outrun).
    SideBySide {
        /// Distance between lanes, m.
        lane_gap_m: f32,
    },
    /// Single file, released one at a time (time attack).
    SingleFile {
        /// Distance between cars, m.
        gap_m: f32,
    },
}

/// One start-grid position.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GridSlot {
    /// Slot index, 0 = pole.
    pub index: u8,
    /// Spawn position (chassis reference point), m.
    pub pos: [f32; 3],
    /// Facing (unit, world XY).
    pub dir: [f32; 2],
}

/// A race course: ordered gates (the last is the finish; for a closed loop gate 0 is the
/// start/finish line) and the centreline used for the corridor rule and progress.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Course {
    /// Gates in order.
    pub gates: Vec<Gate>,
    /// Centreline, m. For a closed loop the last point equals the first.
    pub polyline: Vec<[f32; 3]>,
    /// Cumulative length at each polyline point, m.
    pub cum_m: Vec<f32>,
    /// Closed loop (circuit).
    pub closed_loop: bool,
    /// Half-width of the legal corridor around the centreline, m.
    pub corridor_half_width_m: f32,
}

impl Course {
    /// Build a course from a centreline and gates. A closed loop gets its polyline closed.
    pub fn new(
        mut polyline: Vec<[f32; 3]>,
        gates: Vec<Gate>,
        closed_loop: bool,
        corridor_half_width_m: f32,
    ) -> Course {
        if closed_loop && polyline.len() > 2 && polyline.first() != polyline.last() {
            polyline.push(polyline[0]);
        }
        let mut cum_m = Vec::with_capacity(polyline.len());
        let mut acc = 0.0f32;
        for (i, p) in polyline.iter().enumerate() {
            if i > 0 {
                acc += v2::len(v2::sub(v2::xy(*p), v2::xy(polyline[i - 1])));
            }
            cum_m.push(acc);
        }
        Course {
            gates,
            polyline,
            cum_m,
            closed_loop,
            corridor_half_width_m,
        }
    }

    /// Straight course from `start` along `dir`: gates every `gate_every_m` and at the finish.
    pub fn straight(
        start: [f32; 3],
        dir: [f32; 2],
        length_m: f32,
        gate_every_m: f32,
        width_m: f32,
    ) -> Course {
        let d = v2::norm(dir);
        let at = |s: f32| [start[0] + d[0] * s, start[1] + d[1] * s, start[2]];
        let mut gates = Vec::new();
        let step = gate_every_m.max(10.0);
        let mut s = step;
        while s < length_m - step * 0.5 {
            gates.push(Gate {
                pos: at(s),
                dir: d,
                width_m,
            });
            s += step;
        }
        gates.push(Gate {
            pos: at(length_m),
            dir: d,
            width_m,
        });
        Course::new(vec![start, at(length_m)], gates, false, width_m * 0.5 + 4.0)
    }

    /// Closed loop through `points` (in driving order), one gate per point; gate 0 at
    /// `points[0]` is the start/finish line.
    pub fn closed(points: &[[f32; 3]], width_m: f32) -> Course {
        let n = points.len();
        let gates = (0..n)
            .map(|i| {
                let prev = points[(i + n - 1) % n];
                let next = points[(i + 1) % n];
                Gate {
                    pos: points[i],
                    dir: v2::norm(v2::sub(v2::xy(next), v2::xy(prev))),
                    width_m,
                }
            })
            .collect();
        Course::new(points.to_vec(), gates, true, width_m * 0.5 + 4.0)
    }

    /// Centreline length, m (one lap for a closed loop).
    pub fn length_m(&self) -> f32 {
        self.cum_m.last().copied().unwrap_or(0.0)
    }

    /// The finish gate (gate 0 for a closed loop).
    pub fn finish_gate(&self) -> Option<&Gate> {
        if self.closed_loop {
            self.gates.first()
        } else {
            self.gates.last()
        }
    }

    /// Start line: the polyline start and its direction.
    pub fn start(&self) -> ([f32; 3], [f32; 2]) {
        let p0 = self.polyline.first().copied().unwrap_or([0.0; 3]);
        let p1 = self
            .polyline
            .get(1)
            .copied()
            .unwrap_or([p0[0], p0[1] + 1.0, p0[2]]);
        (p0, v2::norm(v2::sub(v2::xy(p1), v2::xy(p0))))
    }

    /// Point and unit direction at distance `along_m` on the centreline. Before the start the
    /// first segment is extended backwards; past the end a closed loop wraps and an open course
    /// extends the last segment.
    pub fn point_at(&self, along_m: f32) -> ([f32; 3], [f32; 2]) {
        let n = self.polyline.len();
        if n < 2 {
            return (
                self.polyline.first().copied().unwrap_or([0.0; 3]),
                [0.0, 1.0],
            );
        }
        let len = self.length_m();
        let s = if self.closed_loop && len > 0.0 && along_m >= 0.0 {
            along_m.rem_euclid(len)
        } else {
            along_m
        };
        // Segment containing s (first/last segment for out-of-range values).
        let mut i = 0;
        while i + 2 < n && self.cum_m[i + 1] <= s {
            i += 1;
        }
        let a = self.polyline[i];
        let b = self.polyline[i + 1];
        let seg_len = self.cum_m[i + 1] - self.cum_m[i];
        let d = v2::norm(v2::sub(v2::xy(b), v2::xy(a)));
        let u = s - self.cum_m[i];
        let t = if seg_len > 1e-6 { u / seg_len } else { 0.0 };
        (
            [
                a[0] + d[0] * u,
                a[1] + d[1] * u,
                a[2] + (b[2] - a[2]) * t.clamp(0.0, 1.0),
            ],
            d,
        )
    }

    fn project_segment(&self, i: usize, q: [f32; 2]) -> (f32, f32) {
        let a = v2::xy(self.polyline[i]);
        let b = v2::xy(self.polyline[i + 1]);
        let ab = v2::sub(b, a);
        let l2 = v2::dot(ab, ab);
        let t = if l2 > 1e-9 {
            (v2::dot(v2::sub(q, a), ab) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let p = [a[0] + ab[0] * t, a[1] + ab[1] * t];
        let off = v2::len(v2::sub(q, p));
        (self.cum_m[i] + t * (self.cum_m[i + 1] - self.cum_m[i]), off)
    }

    /// Project `p` onto the centreline. `hint` is the previous segment index for this racer:
    /// only `hint ± 8` segments are searched first; a full search runs if that window's best
    /// point is outside the corridor. Ties resolve to the lower segment index.
    pub fn project(&self, p: [f32; 3], hint: Option<usize>) -> Projection {
        let q = v2::xy(p);
        let nseg = self.polyline.len().saturating_sub(1);
        if nseg == 0 {
            let off = self
                .polyline
                .first()
                .map_or(0.0, |a| v2::len(v2::sub(q, v2::xy(*a))));
            return Projection {
                segment: 0,
                along_m: 0.0,
                off_m: off,
            };
        }
        let search = |lo: usize, hi: usize| {
            let mut best = Projection {
                segment: lo,
                along_m: 0.0,
                off_m: f32::INFINITY,
            };
            for i in lo..hi {
                let (along, off) = self.project_segment(i, q);
                if off < best.off_m {
                    best = Projection {
                        segment: i,
                        along_m: along,
                        off_m: off,
                    };
                }
            }
            best
        };
        if let Some(h) = hint {
            let lo = h.saturating_sub(8).min(nseg - 1);
            let hi = (h + 9).min(nseg);
            let local = search(lo, hi);
            if local.off_m <= self.corridor_half_width_m {
                return local;
            }
        }
        search(0, nseg)
    }

    /// Start-grid slots for `n` cars behind the start line (pole = slot 0).
    pub fn grid(&self, layout: GridLayout, n: usize) -> Vec<GridSlot> {
        let (p0, d) = self.start();
        let r = v2::right(d);
        let at = |back: f32, side: f32| {
            [
                p0[0] - d[0] * back + r[0] * side,
                p0[1] - d[1] * back + r[1] * side,
                p0[2],
            ]
        };
        (0..n)
            .map(|i| {
                let pos = match layout {
                    GridLayout::Staggered {
                        row_gap_m,
                        column_gap_m,
                    } => {
                        // Pole on the left column; the right column sits half a row back.
                        let row = (i / 2) as f32;
                        let right = i % 2 == 1;
                        let back =
                            3.0 + row * row_gap_m + if right { row_gap_m * 0.5 } else { 0.0 };
                        let side = if right {
                            column_gap_m * 0.5
                        } else {
                            -column_gap_m * 0.5
                        };
                        at(back, side)
                    }
                    GridLayout::SideBySide { lane_gap_m } => {
                        let side = (i as f32 - (n as f32 - 1.0) * 0.5) * lane_gap_m;
                        at(3.0, side)
                    }
                    GridLayout::SingleFile { gap_m } => at(3.0 + i as f32 * gap_m, 0.0),
                };
                GridSlot {
                    index: i as u8,
                    pos,
                    dir: d,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossing_forward_only_and_within_width() {
        let g = Gate {
            pos: [0.0, 10.0, 0.0],
            dir: [0.0, 1.0],
            width_m: 8.0,
        };
        let c = g.crossing([1.0, 9.0, 0.0], [1.0, 11.0, 0.0], 0.0).unwrap();
        assert!((c.frac - 0.5).abs() < 1e-6);
        assert!((c.lateral_m - 1.0).abs() < 1e-6);
        // Backwards does not count.
        assert!(g.crossing([1.0, 11.0, 0.0], [1.0, 9.0, 0.0], 0.0).is_none());
        // Outside the gate, rescued only by tolerance.
        assert!(g.crossing([5.0, 9.0, 0.0], [5.0, 11.0, 0.0], 0.0).is_none());
        assert!(g.crossing([5.0, 9.0, 0.0], [5.0, 11.0, 0.0], 1.5).is_some());
    }

    #[test]
    fn straight_course_projection_and_gates() {
        let c = Course::straight([0.0, 0.0, 0.0], [0.0, 1.0], 1000.0, 200.0, 10.0);
        assert_eq!(c.gates.len(), 5);
        assert!((c.length_m() - 1000.0).abs() < 1e-3);
        let p = c.project([3.0, 400.0, 0.0], None);
        assert!((p.along_m - 400.0).abs() < 1e-3);
        assert!((p.off_m - 3.0).abs() < 1e-3);
    }

    #[test]
    fn closed_course_loops() {
        let pts = [
            [0.0, 0.0, 0.0],
            [0.0, 100.0, 0.0],
            [100.0, 100.0, 0.0],
            [100.0, 0.0, 0.0],
        ];
        let c = Course::closed(&pts, 12.0);
        assert!(c.closed_loop);
        assert_eq!(c.polyline.len(), 5);
        assert!((c.length_m() - 400.0).abs() < 1e-3);
        assert_eq!(c.finish_gate(), c.gates.first());
    }

    #[test]
    fn grids_sit_behind_the_line() {
        let c = Course::straight([0.0, 0.0, 0.0], [0.0, 1.0], 402.0, 500.0, 10.0);
        for layout in [
            GridLayout::Staggered {
                row_gap_m: 8.0,
                column_gap_m: 5.0,
            },
            GridLayout::SideBySide { lane_gap_m: 5.0 },
            GridLayout::SingleFile { gap_m: 10.0 },
        ] {
            let g = c.grid(layout, 6);
            assert_eq!(g.len(), 6);
            for s in &g {
                assert!(s.pos[1] < 0.0, "{layout:?} slot {} at {:?}", s.index, s.pos);
            }
            for a in 0..g.len() {
                for b in a + 1..g.len() {
                    let d = v2::len(v2::sub(v2::xy(g[a].pos), v2::xy(g[b].pos)));
                    assert!(d > 2.0, "{layout:?} slots {a},{b} overlap");
                }
            }
        }
    }
}
