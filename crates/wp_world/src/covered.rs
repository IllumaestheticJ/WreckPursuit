//! Tunnels and covered roads (backlog B04): which points are under cover, and how cover
//! blocks line of sight.
//!
//! A [`CoveredSpan`] is an enclosure over one stretch of road. The rules, used by
//! [`crate::CityMap::line_of_sight`] (and therefore by every police sightline, the helicopter
//! and traffic cameras):
//! - a sightline from outside a span into it (or back out) is blocked: portals are dark, the
//!   roof and walls are solid;
//! - two points inside the same span see each other if the straight line between them stays
//!   inside (a bend's wall blocks);
//! - a sightline between two outside points is blocked where it passes through the shell
//!   (walls + roof slab), and clear above the roof.
//!
//! The greybox (`tools/city-import`), the physics colliders (`wp_sim`) and the generator
//! (`crate::vesper`) share the shell dimensions here.

use crate::geom::{self, V2, V3};
use crate::network::CoveredSpan;

pub use crate::network::CoverKind;

/// Wall thickness, m.
pub const WALL_M: f32 = 0.6;
/// Roof slab thickness, m.
pub const ROOF_M: f32 = 0.8;
/// How far the roof deck reaches past the outer wall face on each side (over the carved
/// shoulder of the cut, out over the backfilled ground), m: [`BACKFILL_START_M`] plus one
/// terrain cell diagonal (8 m cells), so the ground is level with the deck top all along its
/// edge instead of leaving a slot or a drop beside it.
pub const DECK_OVERHANG_M: f32 = 24.0;
/// How far past the outer wall face the ground is backfilled up to the deck, m: more than one
/// terrain cell diagonal (8 m cells: 11.3 m), so no backfilled terrain triangle reaches into
/// the tube. Between the wall and here the ground stays carved, hidden under the deck.
pub const BACKFILL_START_M: f32 = 12.0;
/// Slope of the backfill embankment beyond the deck edge (and past the portals, beside the
/// approaches) down to the natural ground, rise per metre: 1 in 3, drivable. A tunnel whose
/// natural ground is lower than its roof (the Hollins Tunnel's north side) is buried instead
/// of standing out as a box with a sheer drop off its deck edge.
pub const BACKFILL_SLOPE: f32 = 1.0 / 3.0;
/// How far below the road surface a point still counts as inside, m (eye heights are
/// approximate on slopes; nothing else is down there).
pub const BELOW_ROAD_M: f32 = 3.0;
/// Sampling step for sightlines through a span, m.
const SAMPLE_M: f32 = 2.0;
/// Spacing of the ceiling light fittings, m.
pub const LIGHT_SPACING_M: f32 = 12.0;

/// Where a point is relative to a span.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpanPoint {
    /// Arc length along the span centreline, m (outside `0..=length` = beyond a portal).
    pub s: f32,
    /// Lateral offset, m (+ = right of the centreline direction).
    pub lateral: f32,
    /// Road surface height at the projection, m.
    pub road_z: f32,
}

/// Runtime helper for one span: cached arc lengths and bounds.
#[derive(Debug, Clone, PartialEq)]
pub struct CoverShape {
    /// The span.
    pub span: CoveredSpan,
    /// Cumulative arc length of `span.centre`.
    pub cum: Vec<f32>,
    /// Plan bounds of the shell (walls included), `[min, max]`.
    pub bounds: [V2; 2],
    /// Height bounds of the shell, `[min, max]`.
    pub z_bounds: [f32; 2],
}

impl CoverShape {
    /// Build the helper for `span`.
    pub fn new(span: &CoveredSpan) -> Self {
        let cum = geom::cumulative(&span.centre);
        let r = span.half_width_m + WALL_M;
        let mut lo = [f32::INFINITY; 2];
        let mut hi = [f32::NEG_INFINITY; 2];
        let mut zl = f32::INFINITY;
        let mut zh = f32::NEG_INFINITY;
        for p in &span.centre {
            lo = [lo[0].min(p[0] - r), lo[1].min(p[1] - r)];
            hi = [hi[0].max(p[0] + r), hi[1].max(p[1] + r)];
            zl = zl.min(p[2] - BELOW_ROAD_M);
            zh = zh.max(p[2] + span.clearance_m + ROOF_M);
        }
        Self {
            span: span.clone(),
            cum,
            bounds: [lo, hi],
            z_bounds: [zl, zh],
        }
    }

    /// Length portal to portal, m.
    pub fn length(&self) -> f32 {
        *self.cum.last().unwrap_or(&0.0)
    }

    /// Project a plan point onto the span.
    pub fn locate(&self, p: V2) -> SpanPoint {
        let pr = geom::project(&self.span.centre, &self.cum, p);
        let (c, _) = geom::point_at(&self.span.centre, &self.cum, pr.s);
        SpanPoint {
            s: pr.s,
            lateral: pr.lateral,
            road_z: c[2],
        }
    }

    /// Whether `p` is inside the enclosure (between the walls and portals, under the ceiling).
    pub fn contains(&self, p: V3) -> bool {
        self.within(p, 0.0, 0.0)
    }

    /// Whether `p` is inside the shell including walls and roof slab (solid or hollow).
    fn in_shell(&self, p: V3) -> bool {
        self.within(p, WALL_M, ROOF_M)
    }

    fn within(&self, p: V3, wall: f32, roof: f32) -> bool {
        let r = self.span.half_width_m + wall;
        if p[0] < self.bounds[0][0] - 0.01
            || p[1] < self.bounds[0][1] - 0.01
            || p[0] > self.bounds[1][0] + 0.01
            || p[1] > self.bounds[1][1] + 0.01
            || p[2] < self.z_bounds[0]
            || p[2] > self.z_bounds[1]
        {
            return false;
        }
        let q = self.locate(geom::xy(p));
        q.s >= 0.0
            && q.s <= self.length()
            && q.lateral.abs() <= r
            && p[2] >= q.road_z - BELOW_ROAD_M
            && p[2] <= q.road_z + self.span.clearance_m + roof
    }

    /// Whether the segment's plan bounding box touches the shell's.
    fn near(&self, a: V3, b: V3) -> bool {
        let (lo, hi) = (self.bounds[0], self.bounds[1]);
        !(a[0].max(b[0]) < lo[0]
            || a[0].min(b[0]) > hi[0]
            || a[1].max(b[1]) < lo[1]
            || a[1].min(b[1]) > hi[1]
            || a[2].max(b[2]) < self.z_bounds[0]
            || a[2].min(b[2]) > self.z_bounds[1])
    }

    /// Whether this span blocks the sightline `a`→`b` (rules in the module docs).
    pub fn blocks(&self, a: V3, b: V3) -> bool {
        if !self.near(a, b) {
            return false;
        }
        let (ia, ib) = (self.contains(a), self.contains(b));
        if ia != ib {
            return true;
        }
        let len = geom::dist(geom::xy(a), geom::xy(b)).max((b[2] - a[2]).abs());
        let n = (len / SAMPLE_M).ceil().max(1.0) as u32;
        for k in 1..n {
            let f = k as f32 / n as f32;
            let p = [
                a[0] + (b[0] - a[0]) * f,
                a[1] + (b[1] - a[1]) * f,
                a[2] + (b[2] - a[2]) * f,
            ];
            if ia {
                // Both inside: the line must stay inside (around a bend it hits a wall).
                if !self.contains(p) {
                    return true;
                }
            } else if self.in_shell(p) {
                // Both outside: through the walls or the roof slab.
                return true;
            }
        }
        false
    }

    /// Half width of the roof deck (centreline to deck edge), m.
    pub fn deck_half_width(&self) -> f32 {
        self.span.half_width_m + WALL_M + DECK_OVERHANG_M
    }

    /// Whether plan point `p` lies over the span's deck or within `margin` m past its side
    /// edges (between the portals): the ground there is the tunnel's carve and backfill, drawn
    /// and collided at the terrain's own resolution ([`crate::surface::collision_terrain`]).
    pub fn over_deck(&self, p: V2, margin: f32) -> bool {
        let r = self.deck_half_width() + margin;
        if p[0] < self.bounds[0][0] - r
            || p[1] < self.bounds[0][1] - r
            || p[0] > self.bounds[1][0] + r
            || p[1] > self.bounds[1][1] + r
        {
            return false;
        }
        let q = self.locate(p);
        q.s >= 0.0 && q.s <= self.length() && q.lateral.abs() <= r
    }

    /// Wall-face lines at road level `[left, right]` along the span (inner faces).
    pub fn wall_lines(&self) -> [Vec<V3>; 2] {
        let w = self.span.half_width_m;
        [
            geom::offset_polyline(&self.span.centre, -w),
            geom::offset_polyline(&self.span.centre, w),
        ]
    }

    /// Ceiling light fittings: points on the ceiling centreline every
    /// [`LIGHT_SPACING_M`] (plan position, ceiling height).
    pub fn light_points(&self) -> Vec<V3> {
        let len = self.length();
        let mut out = Vec::new();
        let mut s = LIGHT_SPACING_M * 0.5;
        while s < len {
            let (c, _) = geom::point_at(&self.span.centre, &self.cum, s);
            out.push([c[0], c[1], c[2] + self.span.clearance_m]);
            s += LIGHT_SPACING_M;
        }
        out
    }
}

/// All spans of a network, ready for queries.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CoverIndex {
    /// One shape per [`crate::RoadNetwork::covered`] entry, same order.
    pub shapes: Vec<CoverShape>,
}

impl CoverIndex {
    /// Build from a network's spans.
    pub fn new(spans: &[CoveredSpan]) -> Self {
        Self {
            shapes: spans.iter().map(CoverShape::new).collect(),
        }
    }

    /// Index of the span containing `p`, if any.
    pub fn covered_at(&self, p: V3) -> Option<usize> {
        self.shapes.iter().position(|s| s.contains(p))
    }

    /// Whether any span blocks the sightline `a`→`b`.
    pub fn blocks(&self, a: V3, b: V3) -> bool {
        self.shapes.iter().any(|s| s.blocks(a, b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight 200 m tunnel along +Y at road height 10 m, 8 m half width, 6 m clearance.
    fn straight() -> CoverShape {
        CoverShape::new(&CoveredSpan {
            name: "test".into(),
            kind: CoverKind::Tunnel,
            road: 0,
            centre: vec![[0.0, 0.0, 10.0], [0.0, 200.0, 10.0]],
            half_width_m: 8.0,
            clearance_m: 6.0,
        })
    }

    #[test]
    fn inside_outside() {
        let t = straight();
        assert!(t.contains([0.0, 100.0, 11.3]));
        assert!(t.contains([7.5, 5.0, 10.5]));
        assert!(!t.contains([9.0, 100.0, 11.3]), "beyond the wall");
        assert!(!t.contains([0.0, -2.0, 11.3]), "before the portal");
        assert!(!t.contains([0.0, 100.0, 17.0]), "above the ceiling");
        assert!(!t.contains([0.0, 100.0, 5.0]), "far below the road");
    }

    #[test]
    fn sightlines() {
        let t = straight();
        let inside_a = [0.0, 50.0, 11.3];
        let inside_b = [3.0, 180.0, 11.3];
        let portal_out = [0.0, -40.0, 11.3];
        let heli = [0.0, 100.0, 120.0];
        // Inside ↔ inside: clear; outside ↔ inside and back: blocked.
        assert!(!t.blocks(inside_a, inside_b));
        assert!(t.blocks(portal_out, inside_a));
        assert!(t.blocks(inside_a, portal_out));
        assert!(t.blocks(heli, inside_a));
        // Outside ↔ outside: through the shell is blocked, over the roof is clear, beside it
        // is clear.
        assert!(t.blocks([-30.0, 100.0, 11.3], [30.0, 100.0, 11.3]));
        assert!(!t.blocks([-30.0, 100.0, 30.0], [30.0, 100.0, 30.0]));
        assert!(!t.blocks([-30.0, -10.0, 11.3], [30.0, -10.0, 11.3]));
        assert!(!t.blocks(heli, [40.0, 100.0, 11.3]));
    }

    #[test]
    fn bend_wall_blocks_inside() {
        // An L-shaped tunnel: 100 m north then 100 m east.
        let t = CoverShape::new(&CoveredSpan {
            name: "bend".into(),
            kind: CoverKind::Tunnel,
            road: 0,
            centre: vec![[0.0, 0.0, 0.0], [0.0, 100.0, 0.0], [100.0, 100.0, 0.0]],
            half_width_m: 6.0,
            clearance_m: 6.0,
        });
        let a = [0.0, 10.0, 1.3];
        let b = [90.0, 100.0, 1.3];
        assert!(t.contains(a) && t.contains(b));
        assert!(t.blocks(a, b), "the corner wall is in the way");
        assert!(!t.blocks(a, [0.0, 95.0, 1.3]));
    }

    #[test]
    fn index_finds_span() {
        let idx = CoverIndex::new(&[straight().span]);
        assert_eq!(idx.covered_at([0.0, 100.0, 11.0]), Some(0));
        assert_eq!(idx.covered_at([50.0, 100.0, 11.0]), None);
        assert_eq!(straight().light_points().len(), 17);
    }
}
