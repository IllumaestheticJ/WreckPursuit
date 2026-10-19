//! Breadcrumb trail of where a racer has driven, used to measure the Outrun gap *along the
//! road* instead of in a straight line.
//!
//! Outrun has no route: both cars drive wherever they like. The leader's own trail is the road
//! the chaser has to cover, so the gap is "how far along the leader's trail the leader is ahead
//! of the chaser's closest point on it". A chaser that left the trail (took another street)
//! falls back to the straight-line distance, which never over-counts.

use std::collections::VecDeque;

use crate::course::v2;

/// Spacing between breadcrumbs, m.
pub(crate) const TRAIL_SPACING_M: f32 = 2.0;

/// A chaser farther than this from the leader's trail is "off the trail", m.
pub(crate) const OFF_TRAIL_M: f32 = 25.0;

/// Breadcrumbs of one racer: `(point, distance driven at that point)`, oldest first. Points
/// more than `keep_m` behind the newest are dropped.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Trail {
    points: VecDeque<([f32; 2], f32)>,
    keep_m: f32,
}

impl Trail {
    /// An empty trail keeping at least `keep_m` of road behind the racer.
    pub(crate) fn new(keep_m: f32) -> Trail {
        Trail {
            points: VecDeque::new(),
            keep_m: keep_m.max(TRAIL_SPACING_M * 4.0),
        }
    }

    /// Distance driven up to the newest breadcrumb, m.
    pub(crate) fn head_m(&self) -> f32 {
        self.points.back().map_or(0.0, |p| p.1)
    }

    /// Record a position. Adds a breadcrumb once the racer moved `TRAIL_SPACING_M` from the
    /// newest one.
    pub(crate) fn push(&mut self, p: [f32; 2]) {
        let Some(&(last, cum)) = self.points.back() else {
            self.points.push_back((p, 0.0));
            return;
        };
        let d = v2::len(v2::sub(p, last));
        if d < TRAIL_SPACING_M {
            return;
        }
        let head = cum + d;
        self.points.push_back((p, head));
        while self
            .points
            .front()
            .is_some_and(|f| head - f.1 > self.keep_m)
            && self.points.len() > 2
        {
            self.points.pop_front();
        }
    }

    /// Gap from `chaser` to the trail owner standing at `owner`, along the trail, m.
    ///
    /// The chaser is projected onto the trail; the gap is the trail distance from that point
    /// to the newest breadcrumb plus the owner's distance past it. A chaser more than
    /// `OFF_TRAIL_M` from the trail, or with no trail yet, gets the straight-line distance.
    /// When the chaser is behind the oldest kept breadcrumb, the gap is at least the whole
    /// kept length plus the distance to that breadcrumb.
    pub(crate) fn gap_m(&self, owner: [f32; 2], chaser: [f32; 2]) -> f32 {
        let straight = v2::len(v2::sub(owner, chaser));
        if self.points.len() < 2 {
            return straight;
        }
        let head = self.head_m();
        let tail_extra = v2::len(v2::sub(owner, self.points[self.points.len() - 1].0));
        let mut best: Option<(f32, f32)> = None; // (distance to trail, along)
        for i in 0..self.points.len() - 1 {
            let (a, ca) = self.points[i];
            let (b, cb) = self.points[i + 1];
            let ab = v2::sub(b, a);
            let l2 = v2::dot(ab, ab);
            let t = if l2 > 1e-9 {
                (v2::dot(v2::sub(chaser, a), ab) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let q = [a[0] + ab[0] * t, a[1] + ab[1] * t];
            let d = v2::len(v2::sub(chaser, q));
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, ca + (cb - ca) * t));
            }
        }
        let Some((d, along)) = best else {
            return straight;
        };
        if d > OFF_TRAIL_M {
            return straight;
        }
        let first = self.points[0];
        let behind_tail = along <= first.1 + 1e-3;
        let gap = head - along + tail_extra + if behind_tail { d } else { 0.0 };
        gap.max(straight)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_road_matches_straight_line() {
        let mut t = Trail::new(500.0);
        for i in 0..=100 {
            t.push([0.0, i as f32 * 2.0]);
        }
        let g = t.gap_m([0.0, 200.0], [0.0, 50.0]);
        assert!((g - 150.0).abs() < 0.5, "{g}");
    }

    #[test]
    fn u_turn_counts_the_road_not_the_shortcut() {
        // Up 100 m, across 20 m, back down 100 m. Chaser at the start: 20 m away in a straight
        // line, 220 m along the road.
        let mut t = Trail::new(1000.0);
        for i in 0..=50 {
            t.push([0.0, i as f32 * 2.0]);
        }
        for i in 1..=10 {
            t.push([i as f32 * 2.0, 100.0]);
        }
        for i in 1..=50 {
            t.push([20.0, 100.0 - i as f32 * 2.0]);
        }
        let g = t.gap_m([20.0, 0.0], [0.0, 0.0]);
        assert!((g - 220.0).abs() < 1.0, "{g}");
    }

    #[test]
    fn off_trail_falls_back_to_straight_line() {
        let mut t = Trail::new(500.0);
        for i in 0..=50 {
            t.push([0.0, i as f32 * 2.0]);
        }
        let g = t.gap_m([0.0, 100.0], [80.0, 40.0]);
        let straight = (80.0f32 * 80.0 + 60.0 * 60.0).sqrt();
        assert!((g - straight).abs() < 1e-3);
    }

    #[test]
    fn trail_is_bounded() {
        let mut t = Trail::new(100.0);
        for i in 0..=1000 {
            t.push([0.0, i as f32 * 2.0]);
        }
        assert!(t.points.len() <= 100 / 2 + 3);
        // Chaser far behind the kept tail still counts the full distance.
        let g = t.gap_m([0.0, 2000.0], [0.0, 1880.0]);
        assert!((g - 120.0).abs() < 0.5, "{g}");
    }
}
