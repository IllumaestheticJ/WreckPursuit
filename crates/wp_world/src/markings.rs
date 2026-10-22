//! Painted road markings (contract v0.3.0 `SurfaceType::PaintedLine`).
//!
//! Markings are not stored in the road format: they are derived from the lane layout, so every
//! city (procedural, imported, v2) gets them and the renderer can draw the same lines from the
//! same rules. All dimensions below are in metres.
//!
//! | Marking | Where | Width | Pattern |
//! |---|---|---|---|
//! | Centre line | left edge of the leftmost lane of a two-way road | 2 × [`CENTRE_LINE_W`], gap [`CENTRE_GAP_W`] | double solid |
//! | Lane line | between same-direction general lanes | [`LANE_LINE_W`] | [`DASH_M`] on, [`DASH_GAP_M`] off from the lane start |
//! | Separator | next to a bus lane or a parking strip | [`SEPARATOR_W`] | solid |
//! | Edge line | inside the curb edge of the paved ribbon | [`EDGE_LINE_W`] | solid |
//! | Stop line | last [`STOP_LINE_W`] of a lane that must stop (signal, all-way stop, minor approach) | lane width | solid |
//! | Crosswalk | [`CROSSWALK_FROM_M`]–[`CROSSWALK_TO_M`] past each lane end / before each lane start at signalised and all-way-stop junctions | lane width | zebra bars [`ZEBRA_BAR_M`] on / off across the lane |
//! | Dotted line | between a carriageway's right lane and a ramp: along the taper through a freeway merge / diverge and the auxiliary lane beside it | [`DOTTED_W`] | [`DOT_M`] on, [`DOT_GAP_M`] off |
//! | Gore chevrons | paved gore where a ramp and its carriageway split (or meet), up to where the paving ends | bars [`CHEVRON_BAR_M`] every [`CHEVRON_PERIOD_M`] | V pointing at the gore's narrow end |
//!
//! Alleys and parking strips carry no markings; highways get centre, lane and edge lines.
//! Through a freeway merge / diverge (a radius-0 junction) the main road's lane and median
//! edge lines continue along its connectors, the taper connector carries the outer edge line,
//! and the carriageway's outer edge becomes the dotted line; see [`FreewayPaint`].
//! A line straddling two lanes is split between them (each lane owns its half), so the
//! answer does not depend on which lane a query hits first.
//!
//! [`marking_mesh`] turns the same rules into a render mesh (white and yellow paint) lying on
//! the banked road surface, so what is drawn is exactly what [`marking_at`] reports. Centre
//! lines and the left edge line of a one-way highway (the median side) are yellow.

use crate::geom::{self, V2, V3};
use crate::map::{CityMap, SegKind, Segment};
use crate::network::{IntersectionControl, Lane, LaneId, LaneKind, RoadSurface};
use crate::surface::TriMesh;

/// Width of each of the two centre lines, m.
pub const CENTRE_LINE_W: f32 = 0.12;
/// Gap between the two centre lines, m.
pub const CENTRE_GAP_W: f32 = 0.12;
/// Dashed lane line width, m.
pub const LANE_LINE_W: f32 = 0.12;
/// Solid separator (bus lane, parking strip) width, m.
pub const SEPARATOR_W: f32 = 0.20;
/// Edge line width, m (painted inside the lane, against the curb).
pub const EDGE_LINE_W: f32 = 0.15;
/// Dash length, m.
pub const DASH_M: f32 = 3.0;
/// Gap between dashes, m.
pub const DASH_GAP_M: f32 = 9.0;
/// Stop line depth along the lane, m (ends at the lane's stop point).
pub const STOP_LINE_W: f32 = 0.5;
/// Crosswalk start past the stop point (towards the junction centre), m.
pub const CROSSWALK_FROM_M: f32 = 0.2;
/// Crosswalk end past the stop point, m (inside the builder's 2 m crosswalk setback).
pub const CROSSWALK_TO_M: f32 = 1.8;
/// Zebra bar width (and gap) across the lane, m.
pub const ZEBRA_BAR_M: f32 = 0.5;

/// Dotted line width (straddles the boundary between a carriageway and a ramp), m.
pub const DOTTED_W: f32 = 0.20;
/// Dot length, m.
pub const DOT_M: f32 = 1.0;
/// Gap between dots, m.
pub const DOT_GAP_M: f32 = 2.0;
/// Chevron spacing along the gore, m.
pub const CHEVRON_PERIOD_M: f32 = 4.0;
/// Chevron bar length along the gore, m (the bars run at 45°).
pub const CHEVRON_BAR_M: f32 = 0.6;
/// Longest painted gore, m.
const GORE_MAX_M: f32 = 80.0;
/// Gore width sample spacing, m.
const GORE_STEP_M: f32 = 0.5;
/// A ramp lane's left edge this close to a carriageway lane's right edge runs alongside it, m.
const ALONGSIDE_TOL_M: f32 = 0.25;
/// A gore's two shoulders are level when their heights differ by less than this, m.
const GORE_LEVEL_M: f32 = 0.015;

/// Height of the render mesh's paint above the road surface, m.
pub const MESH_LIFT_M: f32 = 0.02;

/// Wheels further than this above or below a lane are on another level (grade separation).
const LEVEL_TOL_M: f32 = 3.0;

/// Kind of painted marking under a point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MarkingKind {
    /// Double solid line between opposing directions.
    CentreLine,
    /// Dashed line between same-direction lanes.
    LaneLine,
    /// Solid line next to a bus lane or parking strip.
    Separator,
    /// Solid line along the curb.
    EdgeLine,
    /// Stop line across a lane at its stop point.
    StopLine,
    /// Zebra crossing bar.
    Crosswalk,
    /// Dotted line between a carriageway and a ramp lane alongside it.
    DottedLine,
    /// Chevron bar in a gore.
    GoreChevron,
}

/// Marking under world point `p`, if any (`p[2]` selects the level where roads cross).
pub fn marking_at(map: &CityMap, p: V3) -> Option<MarkingKind> {
    let net = &map.net;
    if net.lanes.is_empty() {
        return None;
    }
    let q = geom::xy(p);
    let mut found: Option<MarkingKind> = None;
    for &id in map.segs_near(q) {
        let seg = &map.segs[id as usize];
        let m = match seg.kind {
            SegKind::Lane { lane } => {
                let l = &net.lanes[lane as usize];
                if matches!(l.kind, LaneKind::Parking | LaneKind::Alley) {
                    continue;
                }
                let pr = seg.project(q);
                let half = seg.width * 0.5;
                // A dotted line straddling the right edge belongs to this lane as a whole.
                let overhang = pr.lateral > half
                    && pr.lateral <= half + DOTTED_W * 0.5
                    && map.paint.dotted_right(lane, pr.s);
                if pr.lateral.abs() > half && !overhang {
                    continue;
                }
                let len = seg.length;
                if (seg.point_at(pr.s.clamp(0.0, len)).0[2] - p[2]).abs() > LEVEL_TOL_M {
                    continue;
                }
                if overhang {
                    dot(pr.s).then_some(MarkingKind::DottedLine)
                } else {
                    lane_marking(map, lane, l, pr.s, pr.lateral, half, len)
                }
            }
            SegKind::Connector { .. } => connector_marking(map, seg, p),
        };
        // Several lanes can claim a point only at their shared seams; pick deterministically.
        if let Some(k) = m {
            found = Some(found.map_or(k, |f| f.min(k)));
        }
    }
    if found.is_none() {
        found = map
            .paint
            .gores
            .iter()
            .find(|g| g.chevron_at(map, p))
            .map(|_| MarkingKind::GoreChevron);
    }
    found
}

/// Whether `p` is on a painted marking (what `WorldQuery::surface_at` reports as
/// `SurfaceType::PaintedLine`).
pub fn painted_at(map: &CityMap, p: V3) -> bool {
    marking_at(map, p).is_some()
}

/// Marking at arc length `s` and signed lateral offset `lat` (positive right) of lane `l`.
fn lane_marking(
    map: &CityMap,
    id: LaneId,
    l: &Lane,
    s: f32,
    lat: f32,
    half: f32,
    len: f32,
) -> Option<MarkingKind> {
    // Crosswalks lie beyond the lane's ends, inside the junction setback.
    if s > len {
        let past = s - len;
        if (CROSSWALK_FROM_M..=CROSSWALK_TO_M).contains(&past)
            && zebra(lat + half)
            && end_has_crosswalk(map, id)
        {
            return Some(MarkingKind::Crosswalk);
        }
        return None;
    }
    if s < 0.0 {
        let before = -s;
        if (CROSSWALK_FROM_M..=CROSSWALK_TO_M).contains(&before)
            && zebra(lat + half)
            && start_has_crosswalk(map, l)
        {
            return Some(MarkingKind::Crosswalk);
        }
        return None;
    }
    if s >= len - STOP_LINE_W && must_stop(map, id) {
        return Some(MarkingKind::StopLine);
    }
    let net = &map.net;
    let from_right = half - lat;
    let from_left = half + lat;
    // Right boundary.
    if l.edge_right {
        if map.paint.dotted_right(id, s) {
            if from_right <= DOTTED_W * 0.5 && dot(s) {
                return Some(MarkingKind::DottedLine);
            }
        } else if from_right <= EDGE_LINE_W {
            return Some(MarkingKind::EdgeLine);
        }
    } else {
        match l.right {
            Some(r) if net.lanes[r as usize].kind == LaneKind::Bus || l.kind == LaneKind::Bus => {
                if from_right <= SEPARATOR_W * 0.5 {
                    return Some(MarkingKind::Separator);
                }
            }
            Some(_) => {
                if from_right <= LANE_LINE_W * 0.5 && dash(s) {
                    return Some(MarkingKind::LaneLine);
                }
            }
            // Parking strip (or nothing) beside the lane.
            None => {
                if from_right <= SEPARATOR_W * 0.5 {
                    return Some(MarkingKind::Separator);
                }
            }
        }
    }
    // Left boundary (left bare where a carriageway's dotted line covers it).
    if l.edge_left {
        if from_left <= EDGE_LINE_W && !map.paint.bare_left(id, s) {
            return Some(MarkingKind::EdgeLine);
        }
    } else {
        match l.left {
            Some(lf) if net.lanes[lf as usize].kind == LaneKind::Bus || l.kind == LaneKind::Bus => {
                if from_left <= SEPARATOR_W * 0.5 {
                    return Some(MarkingKind::Separator);
                }
            }
            Some(_) => {
                if from_left <= LANE_LINE_W * 0.5 && dash(s) {
                    return Some(MarkingKind::LaneLine);
                }
            }
            // Leftmost lane of a two-way road: the centre line is on its left edge.
            None => {
                let g = CENTRE_GAP_W * 0.5;
                if from_left >= g && from_left <= g + CENTRE_LINE_W {
                    return Some(MarkingKind::CentreLine);
                }
            }
        }
    }
    None
}

/// Render mesh of every painted marking, split by paint colour.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MarkingMesh {
    /// Lane, edge, separator and stop lines and crosswalks.
    pub white: TriMesh,
    /// Centre lines and the median-side edge line of one-way highways.
    pub yellow: TriMesh,
}

/// Distance from `q` to `s`'s centreline and the height of its banked surface under `q`
/// (nearest centreline point).
fn surface_z(s: &RoadSurface, cum: &[f32], q: V2) -> (f32, f32) {
    let pr = geom::project(&s.centre, cum, q);
    let n = s.centre.len();
    let t = pr.s.clamp(0.0, cum[n - 1]);
    let (c, _) = geom::point_at(&s.centre, cum, t);
    let i = cum.partition_point(|x| *x <= t).clamp(1, n - 1);
    let span = (cum[i] - cum[i - 1]).max(1e-6);
    let f = ((t - cum[i - 1]) / span).clamp(0.0, 1.0);
    let bank = s.bank[i - 1] + (s.bank[i] - s.bank[i - 1]) * f;
    (pr.dist, c[2] + bank * pr.lateral)
}

/// Paints bands along one lane or connector: arc range × lateral range (m right of its
/// centre).
struct LanePainter<'a> {
    pts: &'a [V3],
    cum: &'a [f32],
    /// Paving under the paint (the nearest one wins); none = follow the polyline's height.
    surfaces: Vec<(&'a RoadSurface, Vec<f32>)>,
}

impl<'a> LanePainter<'a> {
    /// Painter along segment `seg`, on the paving of `roads`.
    fn new(map: &'a CityMap, seg: &'a crate::map::Segment, roads: &[u32]) -> Self {
        let surfaces = roads
            .iter()
            .filter_map(|r| map.net.surfaces.get(*r as usize))
            .filter(|s| s.centre.len() >= 2)
            .map(|s| (s, geom::cumulative(&s.centre)))
            .collect();
        Self {
            pts: &seg.pts,
            cum: &seg.cum,
            surfaces,
        }
    }

    fn z(&self, q: V2, fallback: f32) -> f32 {
        // The highest paving that covers `q` (overlapping ribbons: the one on top), else the
        // nearest at all.
        let z = self
            .surfaces
            .iter()
            .map(|(s, cum)| {
                let (d, z) = surface_z(s, cum, q);
                let pr = geom::project(&s.centre, cum, q);
                let covers = pr.s >= 0.0
                    && pr.s <= cum[cum.len() - 1]
                    && (-s.left_m..=s.right_m).contains(&pr.lateral);
                (!covers, d, z)
            })
            .filter(|(_, _, z)| (z - fallback).abs() < LEVEL_TOL_M)
            .min_by(|a, b| {
                a.0.cmp(&b.0).then(if a.0 {
                    a.1.total_cmp(&b.1)
                } else {
                    b.2.total_cmp(&a.2)
                })
            })
            .map_or(fallback, |(_, _, z)| z);
        z + MESH_LIFT_M
    }

    /// Point `d` m right of the lane at arc length `s`; beyond either end the end tangent
    /// is extended.
    fn at(&self, s: f32, d: f32) -> V3 {
        let len = self.cum[self.cum.len() - 1];
        let (c, t) = geom::point_at(self.pts, self.cum, s.clamp(0.0, len));
        let extra = if s > len {
            s - len
        } else if s < 0.0 {
            s
        } else {
            0.0
        };
        let r = geom::right_of(t);
        let q = [
            c[0] + t[0] * extra + r[0] * d,
            c[1] + t[1] * extra + r[1] * d,
        ];
        [q[0], q[1], self.z(q, c[2])]
    }

    fn band(&self, m: &mut TriMesh, s0: f32, s1: f32, d0: f32, d1: f32) {
        if s1 - s0 < 0.01 {
            return;
        }
        let mut ss = vec![s0];
        ss.extend(
            self.cum
                .iter()
                .copied()
                .filter(|c| *c > s0 + 0.05 && *c < s1 - 0.05),
        );
        ss.push(s1);
        // Long straight runs still follow the surface's grade and bank.
        let mut fine = vec![ss[0]];
        for w in ss.windows(2) {
            let n = ((w[1] - w[0]) / 8.0).ceil().max(1.0) as usize;
            for k in 1..=n {
                fine.push(w[0] + (w[1] - w[0]) * k as f32 / n as f32);
            }
        }
        let left: Vec<V3> = fine.iter().map(|s| self.at(*s, d0)).collect();
        let right: Vec<V3> = fine.iter().map(|s| self.at(*s, d1)).collect();
        m.strip(&left, &right);
    }

    /// Dashes `on` m long every `period` m between arc lengths `s0` and `s1`, the pattern
    /// starting `phase` m before arc length 0.
    #[allow(clippy::too_many_arguments)]
    fn pattern(
        &self,
        m: &mut TriMesh,
        (s0, s1): (f32, f32),
        on: f32,
        period: f32,
        phase: f32,
        d0: f32,
        d1: f32,
    ) {
        let mut a = ((s0 + phase) / period).floor() * period - phase;
        while a < s1 {
            self.band(m, a.max(s0), (a + on).min(s1), d0, d1);
            a += period;
        }
    }

    fn dashed(&self, m: &mut TriMesh, s0: f32, s1: f32, phase: f32, d0: f32, d1: f32) {
        self.pattern(m, (s0, s1), DASH_M, DASH_M + DASH_GAP_M, phase, d0, d1);
    }

    fn dotted(&self, m: &mut TriMesh, s0: f32, s1: f32, d0: f32, d1: f32) {
        self.pattern(m, (s0, s1), DOT_M, DOT_M + DOT_GAP_M, 0.0, d0, d1);
    }

    /// Zebra bars across the lane between arc lengths `s0 < s1` (outside the lane's ends).
    fn zebra(&self, m: &mut TriMesh, s0: f32, s1: f32, half: f32) {
        let mut x = 0.0;
        while x < 2.0 * half - 1e-3 {
            if zebra(x + 1e-3) {
                let d0 = x - half;
                let d1 = (x + ZEBRA_BAR_M).min(2.0 * half) - half;
                let left = [self.at(s0, d0), self.at(s1, d0)];
                let right = [self.at(s0, d1), self.at(s1, d1)];
                m.strip(&left, &right);
            }
            x += ZEBRA_BAR_M;
        }
    }
}

/// Render mesh of the city's painted markings, following the rules in the module table.
pub fn marking_mesh(map: &CityMap) -> MarkingMesh {
    let mut out = MarkingMesh::default();
    for i in 0..map.net.lanes.len() {
        paint_lane(map, i, &mut out);
    }
    for seg in &map.segs[map.net.lanes.len()..] {
        paint_connector(map, seg, &mut out);
    }
    for g in &map.paint.gores {
        g.paint(map, &mut out.white);
    }
    out
}

/// Add the lines of a connector through a freeway merge / diverge to `out`.
fn paint_connector(map: &CityMap, seg: &Segment, out: &mut MarkingMesh) {
    let Some((left, right)) = freeway_connector_lines(map, seg) else {
        return;
    };
    let SegKind::Connector { node, .. } = seg.kind else {
        return;
    };
    // On the paving of every road at the node (the taper crosses the main road's on both
    // sides of it).
    let mut roads: Vec<u32> = map
        .net
        .lanes
        .iter()
        .filter(|l| l.from_node == node || l.to_node == node)
        .map(|l| l.road)
        .collect();
    roads.sort_unstable();
    roads.dedup();
    let p = LanePainter::new(map, seg, &roads);
    let (len, half) = (seg.length, seg.width * 0.5);
    match right {
        ConnLine::Edge => p.band(&mut out.white, 0.0, len, half - EDGE_LINE_W, half),
        ConnLine::Dotted => {
            let h = DOTTED_W * 0.5;
            p.dotted(&mut out.white, 0.0, len, half - h, half + h);
        }
        ConnLine::Dashed(phase) => {
            let h = LANE_LINE_W * 0.5;
            p.dashed(&mut out.white, 0.0, len, phase, half - h, half + h);
        }
        ConnLine::Bare => {}
    }
    if left == ConnLine::Edge {
        p.band(&mut out.yellow, 0.0, len, -half, -half + EDGE_LINE_W);
    }
}

/// A painted boundary of a connector through a freeway merge / diverge.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ConnLine {
    /// Nothing painted.
    Bare,
    /// Solid edge line inside the boundary (yellow on the left, white on the right).
    Edge,
    /// Lane line continuing the from-lane's dashes (pattern phase, m); straddles the
    /// boundary.
    Dashed(f32),
    /// Dotted line along the taper (straddles the boundary).
    Dotted,
}

/// Left and right boundary lines of `seg` if it is a connector through a freeway merge or
/// diverge (radius-0 junction). The taper connector (between roads with different lane
/// counts) carries the outer edge line; the main road's connectors continue its lines, with
/// the dotted line on their outer edge.
fn freeway_connector_lines(map: &CityMap, seg: &Segment) -> Option<(ConnLine, ConnLine)> {
    let SegKind::Connector { from, to, node, .. } = seg.kind else {
        return None;
    };
    let net = &map.net;
    let ix = net.nodes[node as usize].intersection?;
    if net.intersections[ix as usize].radius_m > 0.0 {
        return None;
    }
    let (f, t) = (&net.lanes[from as usize], &net.lanes[to as usize]);
    if f.kind != LaneKind::Highway || t.kind != LaneKind::Highway {
        return None;
    }
    let lanes = |l: &Lane| map.paint.road_lanes.get(l.road as usize).copied();
    if lanes(f) != lanes(t) {
        return Some((ConnLine::Bare, ConnLine::Edge));
    }
    // A line between two connectors belongs whole to the one on its left (their dash phases
    // differ on a curve).
    let left = if f.edge_left {
        ConnLine::Edge
    } else {
        ConnLine::Bare
    };
    let right = if f.edge_right {
        ConnLine::Dotted
    } else if f.right.is_some() {
        ConnLine::Dashed(map.segs[from as usize].length)
    } else {
        ConnLine::Bare
    };
    Some((left, right))
}

/// Marking under `p` on connector `seg` (only connectors through freeway merges and diverges
/// are painted).
fn connector_marking(map: &CityMap, seg: &Segment, p: V3) -> Option<MarkingKind> {
    let (left, right) = freeway_connector_lines(map, seg)?;
    let pr = seg.project(geom::xy(p));
    let (len, half) = (seg.length, seg.width * 0.5);
    if pr.s < 0.0 || pr.s > len {
        return None;
    }
    if (seg.point_at(pr.s).0[2] - p[2]).abs() > LEVEL_TOL_M {
        return None;
    }
    let (s, lat) = (pr.s, pr.lateral);
    // The half of a line beyond the right boundary.
    if lat > half {
        return match right {
            ConnLine::Dotted if lat <= half + DOTTED_W * 0.5 => {
                dot(s).then_some(MarkingKind::DottedLine)
            }
            ConnLine::Dashed(phase) if lat <= half + LANE_LINE_W * 0.5 => {
                dash(s + phase).then_some(MarkingKind::LaneLine)
            }
            _ => None,
        };
    }
    if lat.abs() > half {
        return None;
    }
    let (from_right, from_left) = (half - lat, half + lat);
    let hit = match right {
        ConnLine::Edge => from_right <= EDGE_LINE_W,
        ConnLine::Dotted => from_right <= DOTTED_W * 0.5 && dot(s),
        ConnLine::Dashed(phase) => from_right <= LANE_LINE_W * 0.5 && dash(s + phase),
        ConnLine::Bare => false,
    };
    if hit {
        return Some(if right == ConnLine::Edge {
            MarkingKind::EdgeLine
        } else if right == ConnLine::Dotted {
            MarkingKind::DottedLine
        } else {
            MarkingKind::LaneLine
        });
    }
    (left == ConnLine::Edge && from_left <= EDGE_LINE_W).then_some(MarkingKind::EdgeLine)
}

/// Freeway paint derived from the lane layout when a [`CityMap`] is built: where a ramp lane
/// runs alongside a carriageway lane (the auxiliary lane before a diverge or after a merge)
/// the two edge lines become one dotted line, and where they split a chevron gore is painted.
#[derive(Debug, Clone, Default)]
pub struct FreewayPaint {
    /// Per lane: sorted arc ranges where its right edge line is the dotted line instead.
    dotted_right: Vec<Vec<[f32; 2]>>,
    /// Per lane: sorted arc ranges where its left edge line is left out (under the dotted line
    /// of the carriageway lane alongside).
    bare_left: Vec<Vec<[f32; 2]>>,
    /// Lanes per road.
    road_lanes: Vec<u32>,
    /// Chevron gores.
    gores: Vec<Gore>,
}

impl FreewayPaint {
    fn dotted_right(&self, lane: LaneId, s: f32) -> bool {
        self.dotted_right
            .get(lane as usize)
            .is_some_and(|r| in_ranges(r, s))
    }

    fn bare_left(&self, lane: LaneId, s: f32) -> bool {
        self.bare_left
            .get(lane as usize)
            .is_some_and(|r| in_ranges(r, s))
    }
}

/// The paved gore between a carriageway lane and a ramp lane beside it, from where their
/// edge lines part to where the paving between them ends.
#[derive(Debug, Clone)]
struct Gore {
    /// Carriageway lane on the gore's left.
    main: LaneId,
    /// Ramp lane on its right.
    ramp: LaneId,
    /// Paved shoulder beyond `main`'s right edge and beyond `ramp`'s left edge, m.
    shoulders: [f32; 2],
    /// Arc length on `main` of the gore's narrow end.
    s0: f32,
    /// +1 when the gore widens with increasing arc length (diverge), -1 (merge).
    dir: f32,
    /// Gore width beyond `main`'s right edge every [`GORE_STEP_M`] from the narrow end, m.
    width: Vec<f32>,
    /// Painted reach beyond `main`'s right edge at the same samples, m: the whole width where
    /// the two shoulders are level, else only the carriageway's own shoulder (a ramp climbing
    /// or dropping away is on another level already).
    reach: Vec<f32>,
    /// Bounding box (xy).
    lo: V2,
    hi: V2,
}

/// Linear interpolation in samples taken every [`GORE_STEP_M`].
fn sample(v: &[f32], u: f32) -> f32 {
    let f = (u / GORE_STEP_M).clamp(0.0, (v.len() - 1) as f32);
    let i = (f.floor() as usize).min(v.len().saturating_sub(2));
    let t = f - i as f32;
    let v1 = v.get(i + 1).copied().unwrap_or(v[i]);
    v[i] + (v1 - v[i]) * t
}

/// Chevron bar at `u` m from a gore's narrow end and `x` m across a gore `w` m wide.
fn chevron(u: f32, x: f32, w: f32) -> bool {
    let c = u - (x - w * 0.5).abs();
    c >= 0.0 && c - (c / CHEVRON_PERIOD_M).floor() * CHEVRON_PERIOD_M < CHEVRON_BAR_M
}

impl Gore {
    /// Gore length, m.
    fn length(&self) -> f32 {
        (self.width.len() - 1) as f32 * GORE_STEP_M
    }

    /// Width at `u` m from the narrow end.
    fn width_at(&self, u: f32) -> f32 {
        sample(&self.width, u)
    }

    /// Painted reach at `u` m from the narrow end.
    fn reach_at(&self, u: f32) -> f32 {
        sample(&self.reach, u)
    }

    /// Whether `p` is on one of the gore's chevron bars.
    fn chevron_at(&self, map: &CityMap, p: V3) -> bool {
        let q = geom::xy(p);
        if q[0] < self.lo[0] || q[1] < self.lo[1] || q[0] > self.hi[0] || q[1] > self.hi[1] {
            return false;
        }
        let seg = &map.segs[self.main as usize];
        let pr = seg.project(q);
        let u = (pr.s - self.s0) * self.dir;
        if u < 0.0 || u > self.length() || pr.s < 0.0 || pr.s > seg.length {
            return false;
        }
        if (seg.point_at(pr.s).0[2] - p[2]).abs() > LEVEL_TOL_M {
            return false;
        }
        let w = self.width_at(u);
        let x = pr.lateral - seg.width * 0.5;
        x > 0.0 && x < self.reach_at(u) && chevron(u, x, w)
    }

    /// Add the chevron bars to `m`: each bar's two arms, row by row along the gore.
    fn paint(&self, map: &CityMap, m: &mut TriMesh) {
        let seg = &map.segs[self.main as usize];
        let half = seg.width * 0.5;
        let p = LanePainter::new(map, seg, &[]);
        let surf = |l: LaneId| {
            let s = &map.net.surfaces[map.net.lanes[l as usize].road as usize];
            (s, geom::cumulative(&s.centre))
        };
        let (sa, sb) = (surf(self.main), surf(self.ramp));
        let len = self.length();
        let first = m.indices.len();
        // On the shoulder under the point (they are level where both reach).
        let to_world = |u: f32, x: f32| {
            let mut q = p.at(self.s0 + self.dir * u, half + x);
            let (s, cum) = if x <= self.shoulders[0] { &sa } else { &sb };
            q[2] = surface_z(s, cum, geom::xy(q)).1 + MESH_LIFT_M;
            q
        };
        let mut c = 0.0;
        while c < len {
            // Rows from the bar's tip until its arms leave the gore.
            let mut rows: Vec<(f32, f32, f32)> = Vec::new();
            let mut u = c;
            loop {
                let u_row = u.min(len);
                let w = self.width_at(u_row);
                let mid = w * 0.5;
                let lo = mid + (u_row - c - CHEVRON_BAR_M).max(0.0);
                // The first row is the bar's tip (zero width); the arms end at the gore's sides.
                if lo >= w {
                    break;
                }
                let hi = (mid + (u_row - c)).min(w);
                rows.push((u_row, lo, hi));
                if u_row >= len {
                    break;
                }
                u += 0.25;
            }
            if rows.len() >= 2 {
                // Right arm (x from mid outwards) and its mirror image, the left arm.
                for mirror in [false, true] {
                    let mut inner: Vec<V3> = Vec::new();
                    let mut outer: Vec<V3> = Vec::new();
                    for (k, &(u, lo, hi)) in rows.iter().enumerate() {
                        let w = self.width_at(u);
                        let (a, b) = if mirror { (w - hi, w - lo) } else { (lo, hi) };
                        // Clipped to the painted reach: the arm ends where it leaves it.
                        let r = self.reach_at(u);
                        if k > 0 && a >= r {
                            break;
                        }
                        inner.push(to_world(u, a.min(r)));
                        outer.push(to_world(u, b.min(r)));
                    }
                    // Strips face up when their left edge is on the left of the rows' travel.
                    if self.dir > 0.0 {
                        m.strip(&inner, &outer);
                    } else {
                        m.strip(&outer, &inner);
                    }
                }
            }
            c += CHEVRON_PERIOD_M;
        }
        // A bar's tip row has zero width: drop the slivers it leaves.
        let pos = &m.positions;
        let mut tris = m.indices.split_off(first);
        tris.retain(|t| {
            let [a, b, c] = t.map(|i| geom::xy(pos[i as usize]));
            geom::cross(geom::sub(b, a), geom::sub(c, a)).abs() > 1e-6
        });
        m.indices.extend(tris);
    }
}

/// Derive the city's [`FreewayPaint`] (called once when the map is built).
pub(crate) fn freeway_paint(map: &CityMap) -> FreewayPaint {
    let net = &map.net;
    let n = net.lanes.len();
    let mut out = FreewayPaint {
        dotted_right: vec![Vec::new(); n],
        bare_left: vec![Vec::new(); n],
        road_lanes: Vec::new(),
        gores: Vec::new(),
    };
    for l in &net.lanes {
        let r = l.road as usize;
        if out.road_lanes.len() <= r {
            out.road_lanes.resize(r + 1, 0);
        }
        out.road_lanes[r] += 1;
    }
    for a in 0..n {
        let la = &net.lanes[a];
        if la.kind != LaneKind::Highway || !la.edge_right || la.polyline.len() < 2 {
            continue;
        }
        for run in alongside_runs(map, a as LaneId) {
            let (len_a, len_b) = (map.segs[a].length, map.segs[run.b as usize].length);
            out.dotted_right[a].push([(run.a0 - 0.5).max(0.0), (run.a1 + 0.5).min(len_a)]);
            out.bare_left[run.b as usize]
                .push([(run.b0 - 0.5).max(0.0), (run.b1 + 0.5).min(len_b)]);
            // Gores where both lanes go on past the run: they part there.
            if run.a1 < len_a - 1.0 && run.b1 < len_b - 1.0 {
                out.gores.extend(gore(map, a as LaneId, run.b, run.a1, 1.0));
            }
            if run.a0 > 1.0 && run.b0 > 1.0 {
                out.gores
                    .extend(gore(map, a as LaneId, run.b, run.a0, -1.0));
            }
        }
    }
    for r in out.dotted_right.iter_mut().chain(out.bare_left.iter_mut()) {
        r.sort_by(|x, y| x[0].total_cmp(&y[0]));
    }
    out
}

/// A stretch where lane `b`'s left edge runs along lane `a`'s right edge.
struct Run {
    b: LaneId,
    /// Arc range on `a`.
    a0: f32,
    a1: f32,
    /// Arc range on `b`.
    b0: f32,
    b1: f32,
}

/// Lane `b` (another road's one-way highway lane, same level and direction) whose left edge
/// lies within [`ALONGSIDE_TOL_M`] of `a`'s right edge at arc length `s`, and `b`'s arc length.
fn alongside_at(map: &CityMap, a: LaneId, s: f32) -> Option<(LaneId, f32)> {
    let net = &map.net;
    let seg = &map.segs[a as usize];
    let (c, t) = seg.point_at(s);
    let r = geom::right_of(t);
    let half = seg.width * 0.5;
    let e = [c[0] + r[0] * half, c[1] + r[1] * half];
    let road = net.lanes[a as usize].road;
    map.segs_near(e)
        .iter()
        .filter_map(|&id| {
            let sb = &map.segs[id as usize];
            let SegKind::Lane { lane: b } = sb.kind else {
                return None;
            };
            let lb = &net.lanes[b as usize];
            if b == a || lb.road == road || lb.kind != LaneKind::Highway || !lb.edge_left {
                return None;
            }
            let pr = sb.project(e);
            if pr.s < 0.0 || pr.s > sb.length {
                return None;
            }
            let (cb, tb) = sb.point_at(pr.s);
            let ok = geom::dot(t, tb) > 0.9
                && (cb[2] - c[2]).abs() < 1.0
                && (pr.lateral + sb.width * 0.5).abs() < ALONGSIDE_TOL_M;
            ok.then_some((b, pr.s))
        })
        .min_by_key(|(b, _)| *b)
}

/// Every stretch (at least 5 m) where another lane runs alongside lane `a`'s right edge.
fn alongside_runs(map: &CityMap, a: LaneId) -> Vec<Run> {
    let len = map.segs[a as usize].length;
    let steps = len.ceil().max(1.0) as usize;
    let mut runs: Vec<Run> = Vec::new();
    let mut open: Option<Run> = None;
    for k in 0..=steps {
        let s = len * k as f32 / steps as f32;
        let hit = alongside_at(map, a, s);
        match (&mut open, hit) {
            (Some(run), Some((b, sb))) if run.b == b => {
                run.a1 = s;
                run.b0 = run.b0.min(sb);
                run.b1 = run.b1.max(sb);
            }
            (_, hit) => {
                runs.extend(open.take());
                open = hit.map(|(b, sb)| Run {
                    b,
                    a0: s,
                    a1: s,
                    b0: sb,
                    b1: sb,
                });
            }
        }
    }
    runs.extend(open);
    runs.retain(|r| r.a1 - r.a0 >= 5.0);
    runs
}

/// The gore opening from arc length `s0` on carriageway lane `a` in direction `dir` between
/// it and ramp lane `b`, while the paving between them (the two lanes' shoulders) lasts.
fn gore(map: &CityMap, a: LaneId, b: LaneId, s0: f32, dir: f32) -> Option<Gore> {
    let net = &map.net;
    let (sa, sb) = (&map.segs[a as usize], &map.segs[b as usize]);
    let (half_a, half_b) = (sa.width * 0.5, sb.width * 0.5);
    let edge_a = |s: f32| {
        let (c, t) = sa.point_at(s);
        let r = geom::right_of(t);
        [c[0] + r[0] * half_a, c[1] + r[1] * half_a]
    };
    // Paved width beyond each lane's edge (shoulders) at the narrow end.
    let surf_a = net.surfaces.get(net.lanes[a as usize].road as usize)?;
    let surf_b = net.surfaces.get(net.lanes[b as usize].road as usize)?;
    let e0 = edge_a(s0);
    let lat = |s: &RoadSurface, q: V2| geom::project(&s.centre, &geom::cumulative(&s.centre), q);
    let shoulder_a = surf_a.right_m - lat(surf_a, e0).lateral;
    let shoulder_b = surf_b.left_m + lat(surf_b, e0).lateral;
    let paved = (shoulder_a + shoulder_b - 0.1).min(12.0);
    let (cum_a, cum_b) = (
        geom::cumulative(&surf_a.centre),
        geom::cumulative(&surf_b.centre),
    );
    let mut width: Vec<f32> = Vec::new();
    let mut reach: Vec<f32> = Vec::new();
    let mut lo = [f32::INFINITY; 2];
    let mut hi = [f32::NEG_INFINITY; 2];
    let mut u = 0.0;
    while u <= GORE_MAX_M {
        let s = s0 + dir * u;
        if !(0.0..=sa.length).contains(&s) {
            break;
        }
        let e = edge_a(s);
        let pr = sb.project(e);
        if pr.s < 0.0 || pr.s > sb.length {
            break;
        }
        let w = (-(pr.lateral + half_b)).max(0.0);
        if w > paved {
            break;
        }
        let (c, t) = sa.point_at(s);
        let r = geom::right_of(t);
        let far = [c[0] + r[0] * (half_a + w), c[1] + r[1] * (half_a + w)];
        let za = surface_z(surf_a, &cum_a, far).1;
        let zb = surface_z(surf_b, &cum_b, far).1;
        reach.push(if (za - zb).abs() < GORE_LEVEL_M {
            w
        } else {
            w.min(w - shoulder_b).max(0.0)
        });
        width.push(w);
        // Box around the gore's far side too.
        for q in [e, far] {
            lo = [lo[0].min(q[0]), lo[1].min(q[1])];
            hi = [hi[0].max(q[0]), hi[1].max(q[1])];
        }
        u += GORE_STEP_M;
    }
    (width.len() >= 8).then_some(Gore {
        main: a,
        ramp: b,
        shoulders: [shoulder_a, shoulder_b],
        s0,
        dir,
        width,
        reach,
        lo: [lo[0] - 0.5, lo[1] - 0.5],
        hi: [hi[0] + 0.5, hi[1] + 0.5],
    })
}

/// Add lane `i`'s markings to `out`.
fn paint_lane(map: &CityMap, i: usize, out: &mut MarkingMesh) {
    let net = &map.net;
    let l = &net.lanes[i];
    {
        if matches!(l.kind, LaneKind::Parking | LaneKind::Alley) || l.polyline.len() < 2 {
            return;
        }
        let id = i as LaneId;
        let seg = &map.segs[i];
        let len = seg.length;
        let half = seg.width * 0.5;
        let own = net
            .surfaces
            .get(l.road as usize)
            .is_some_and(|s| s.road == l.road);
        let roads = [l.road];
        let p = LanePainter::new(map, seg, if own { &roads[..] } else { &[] });
        let w = &mut out.white;
        if must_stop(map, id) {
            p.band(w, len - STOP_LINE_W, len, -half, half);
        }
        if end_has_crosswalk(map, id) {
            p.zebra(w, len + CROSSWALK_FROM_M, len + CROSSWALK_TO_M, half);
        }
        if start_has_crosswalk(map, l) {
            p.zebra(w, -CROSSWALK_TO_M, -CROSSWALK_FROM_M, half);
        }
        let solid_to = if must_stop(map, id) {
            len - STOP_LINE_W
        } else {
            len
        };
        // Right boundary: shared lines are drawn whole by the lane on their left.
        if l.edge_right {
            let dotted = map.paint.dotted_right.get(i).map_or(&[][..], |r| &r[..]);
            for [a, b] in outside(dotted, 0.0, solid_to) {
                p.band(w, a, b, half - EDGE_LINE_W, half);
            }
            let h = DOTTED_W * 0.5;
            for [a, b] in dotted {
                p.dotted(w, *a, b.min(solid_to), half - h, half + h);
            }
        } else {
            match l.right {
                Some(r)
                    if net.lanes[r as usize].kind == LaneKind::Bus || l.kind == LaneKind::Bus =>
                {
                    let h = SEPARATOR_W * 0.5;
                    p.band(w, 0.0, solid_to, half - h, half + h);
                }
                Some(_) => {
                    let h = LANE_LINE_W * 0.5;
                    p.dashed(w, 0.0, solid_to, 0.0, half - h, half + h);
                }
                None => p.band(w, 0.0, solid_to, half - SEPARATOR_W * 0.5, half),
            }
        }
        // Left boundary.
        if l.edge_left {
            let m = if l.kind == LaneKind::Highway {
                &mut out.yellow
            } else {
                &mut out.white
            };
            let bare = map.paint.bare_left.get(i).map_or(&[][..], |r| &r[..]);
            for [a, b] in outside(bare, 0.0, solid_to) {
                p.band(m, a, b, -half, -half + EDGE_LINE_W);
            }
        } else if l.left.is_none() {
            let g = CENTRE_GAP_W * 0.5;
            p.band(
                &mut out.yellow,
                0.0,
                solid_to,
                -half + g,
                -half + g + CENTRE_LINE_W,
            );
        }
    }
}

/// Dash pattern from the lane start.
fn dash(s: f32) -> bool {
    let period = DASH_M + DASH_GAP_M;
    s - (s / period).floor() * period < DASH_M
}

/// Dot pattern from the lane start.
fn dot(s: f32) -> bool {
    let period = DOT_M + DOT_GAP_M;
    s - (s / period).floor() * period < DOT_M
}

/// Whether `s` lies in one of the sorted, disjoint `ranges`.
fn in_ranges(ranges: &[[f32; 2]], s: f32) -> bool {
    ranges.iter().any(|[a, b]| (*a..=*b).contains(&s))
}

/// Parts of `[a, b]` outside the sorted, disjoint `ranges`.
fn outside(ranges: &[[f32; 2]], a: f32, b: f32) -> Vec<[f32; 2]> {
    let mut out = Vec::new();
    let mut from = a;
    for [r0, r1] in ranges {
        if *r0 > from {
            out.push([from, r0.min(b)]);
        }
        from = from.max(*r1);
    }
    if from < b {
        out.push([from, b]);
    }
    out.retain(|[x, y]| y > x);
    out
}

/// Zebra bar at `x` m from the lane's left edge.
fn zebra(x: f32) -> bool {
    ((x / ZEBRA_BAR_M).floor() as i32).rem_euclid(2) == 0
}

/// Lane `id` must stop at its end (signal, all-way stop, minor approach at a priority
/// junction other than a freeway on-ramp, which merges).
pub fn must_stop(map: &CityMap, id: LaneId) -> bool {
    match map.lane_control(id) {
        Some((IntersectionControl::Signal { .. }, _, _))
        | Some((IntersectionControl::AllWayStop, _, _)) => true,
        Some((IntersectionControl::Priority, ap, _)) => {
            !ap.major && map.net.intersections[ap.intersection as usize].radius_m > 0.0
        }
        _ => false,
    }
}

fn has_crosswalk(c: IntersectionControl) -> bool {
    matches!(
        c,
        IntersectionControl::Signal { .. } | IntersectionControl::AllWayStop
    )
}

/// The junction at lane `id`'s end has crosswalks.
fn end_has_crosswalk(map: &CityMap, id: LaneId) -> bool {
    map.lane_control(id).is_some_and(|(c, _, _)| {
        has_crosswalk(c) && map.net.lanes[id as usize].kind != LaneKind::Highway
    })
}

/// The junction at lane `l`'s start has crosswalks.
fn start_has_crosswalk(map: &CityMap, l: &Lane) -> bool {
    if l.kind == LaneKind::Highway || l.polyline.len() < 2 {
        return false;
    }
    let net = &map.net;
    // Step 1 m back from the lane start, into the junction box.
    let a = geom::xy(l.polyline[0]);
    let b = geom::xy(l.polyline[1]);
    let d = geom::normalize(geom::sub(b, a));
    let probe = geom::sub(a, d);
    map.intersection_at(probe).is_some_and(|ix| {
        let ix = &net.intersections[ix as usize];
        ix.node == l.from_node && has_crosswalk(ix.control)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procgen::{generate, ProcGenConfig};
    use wp_contracts::{SurfaceType, WorldQuery};

    fn small_map() -> CityMap {
        CityMap::new(generate(&ProcGenConfig::small()))
    }

    /// Point at arc length `s` and lateral `lat` of lane `id`.
    fn lane_point(map: &CityMap, id: LaneId, s: f32, lat: f32) -> V3 {
        let seg = &map.segs[id as usize];
        let (c, t) = seg.point_at(s);
        let r = geom::right_of(t);
        [c[0] + r[0] * lat, c[1] + r[1] * lat, c[2]]
    }

    /// Lane end point extended `past` m beyond the stop point.
    fn past_end(map: &CityMap, id: LaneId, past: f32, lat: f32) -> V3 {
        let seg = &map.segs[id as usize];
        let (c, t) = seg.point_at(seg.length);
        let r = geom::right_of(t);
        [
            c[0] + t[0] * past + r[0] * lat,
            c[1] + t[1] * past + r[1] * lat,
            c[2],
        ]
    }

    fn lanes_where(map: &CityMap, f: impl Fn(LaneId, &Lane) -> bool) -> Vec<LaneId> {
        map.net
            .lanes
            .iter()
            .enumerate()
            .filter(|(i, l)| f(*i as LaneId, l))
            .map(|(i, _)| i as LaneId)
            .collect()
    }

    #[test]
    fn lane_centres_are_not_painted() {
        let map = small_map();
        let mut checked = 0;
        for (i, l) in map.net.lanes.iter().enumerate() {
            let seg = &map.segs[i];
            if seg.length < 30.0 || l.kind == LaneKind::Parking {
                continue;
            }
            let p = lane_point(&map, i as LaneId, seg.length * 0.5, 0.0);
            assert_ne!(marking_at(&map, p), Some(MarkingKind::StopLine));
            if map.intersection_at(geom::xy(p)).is_none() {
                assert_eq!(marking_at(&map, p), None, "lane {i} centre painted");
                checked += 1;
            }
        }
        assert!(checked > 100, "{checked}");
    }

    #[test]
    fn centre_line_on_two_way_roads() {
        let map = small_map();
        let two_way = lanes_where(&map, |_, l| {
            l.left.is_none()
                && !l.edge_left
                && l.index_from_left == 0
                && l.kind == LaneKind::Through
        });
        assert!(!two_way.is_empty());
        for &id in &two_way {
            let len = map.segs[id as usize].length;
            if len < 20.0 {
                continue;
            }
            let half = map.segs[id as usize].width * 0.5;
            let on = lane_point(
                &map,
                id,
                len * 0.5,
                -half + CENTRE_GAP_W * 0.5 + CENTRE_LINE_W * 0.5,
            );
            assert_eq!(
                marking_at(&map, on),
                Some(MarkingKind::CentreLine),
                "lane {id}"
            );
            // The gap between the two yellow lines is bare asphalt.
            let gap = lane_point(&map, id, len * 0.5, -half + 0.01);
            assert_eq!(marking_at(&map, gap), None, "lane {id} gap");
        }
    }

    #[test]
    fn lane_lines_are_dashed() {
        let map = small_map();
        let multi = lanes_where(&map, |_, l| {
            l.kind == LaneKind::Through
                && l.right.is_some_and(|r| {
                    // General neighbour on the right.
                    let _ = r;
                    true
                })
        });
        let net = &map.net;
        let mut on = 0;
        let mut off = 0;
        for &id in &multi {
            let r = net.lanes[id as usize].right.unwrap();
            if net.lanes[r as usize].kind == LaneKind::Bus {
                continue;
            }
            let seg = &map.segs[id as usize];
            let half = seg.width * 0.5;
            let mut s = 1.0;
            while s < seg.length - 2.0 {
                let p = lane_point(&map, id, s, half - 0.02);
                // Projected arc length (differs from `s` near polyline corners).
                let sp = seg.project(geom::xy(p)).s;
                if map.intersection_at(geom::xy(p)).is_none() {
                    match marking_at(&map, p) {
                        Some(MarkingKind::LaneLine) => {
                            on += 1;
                        }
                        None => {
                            assert!(!dash(sp), "lane {id} s {sp}: dash missing");
                            off += 1;
                        }
                        _ => {}
                    }
                }
                s += 1.0;
            }
        }
        assert!(on > 0 && off > on, "on {on} off {off}");
    }

    #[test]
    fn stop_lines_at_signals_and_crosswalks_past_them() {
        let map = small_map();
        let signalised = lanes_where(&map, |id, l| {
            l.kind == LaneKind::Through
                && matches!(
                    map.lane_control(id),
                    Some((IntersectionControl::Signal { .. }, _, _))
                )
        });
        assert!(!signalised.is_empty());
        for &id in &signalised {
            let len = map.segs[id as usize].length;
            let stop = lane_point(&map, id, len - 0.2, 0.0);
            assert_eq!(
                marking_at(&map, stop),
                Some(MarkingKind::StopLine),
                "lane {id}"
            );
            let before = lane_point(&map, id, len - 2.0, 0.3);
            assert_ne!(marking_at(&map, before), Some(MarkingKind::StopLine));
            // A zebra bar is painted at the lane's left edge (bar 0) and not on the next 0.5 m.
            let half = map.segs[id as usize].width * 0.5;
            let bar = past_end(&map, id, 1.0, -half + 0.25);
            assert_eq!(
                marking_at(&map, bar),
                Some(MarkingKind::Crosswalk),
                "lane {id}"
            );
            let gap = past_end(&map, id, 1.0, -half + 0.75);
            assert_ne!(
                marking_at(&map, gap),
                Some(MarkingKind::Crosswalk),
                "lane {id}"
            );
            assert_eq!(map.surface_at(bar), SurfaceType::PaintedLine);
        }
        // Lanes that leave a signalised junction start behind a crosswalk too.
        let leaving = lanes_where(&map, |_, l| {
            l.kind == LaneKind::Through && start_has_crosswalk(&map, l)
        });
        assert!(!leaving.is_empty());
        for &id in &leaving {
            let seg = &map.segs[id as usize];
            let (c, t) = seg.point_at(0.0);
            let half = seg.width * 0.5;
            let r = geom::right_of(t);
            let p = [
                c[0] - t[0] * 1.0 + r[0] * (-half + 0.25),
                c[1] - t[1] * 1.0 + r[1] * (-half + 0.25),
                c[2],
            ];
            assert_eq!(
                marking_at(&map, p),
                Some(MarkingKind::Crosswalk),
                "lane {id}"
            );
        }
    }

    #[test]
    fn edge_lines_and_surface_type() {
        let map = small_map();
        let edged = lanes_where(&map, |_, l| l.edge_right && l.kind == LaneKind::Through);
        assert!(!edged.is_empty());
        let mut n = 0;
        for &id in &edged {
            let seg = &map.segs[id as usize];
            if seg.length < 20.0 {
                continue;
            }
            let half = seg.width * 0.5;
            let p = lane_point(&map, id, seg.length * 0.5, half - 0.05);
            if map.intersection_at(geom::xy(p)).is_some() {
                continue;
            }
            assert_eq!(
                marking_at(&map, p),
                Some(MarkingKind::EdgeLine),
                "lane {id}"
            );
            assert_eq!(map.surface_at(p), SurfaceType::PaintedLine);
            // A wheel 0.3 m inside the line is on plain road again.
            let inside = lane_point(&map, id, seg.length * 0.5, half - 0.3);
            assert_ne!(map.surface_at(inside), SurfaceType::PaintedLine);
            n += 1;
        }
        assert!(n > 10);
    }

    #[test]
    fn painted_share_is_plausible() {
        // Over a grid of points on lanes, a few percent should be paint, not most of the road.
        let map = small_map();
        let mut road = 0u32;
        let mut paint = 0u32;
        let (lo, hi) = (map.net.bounds_min, map.net.bounds_max);
        let mut x = lo[0];
        while x < hi[0] {
            let mut y = lo[1];
            while y < hi[1] {
                let p = [x, y, 0.0];
                match map.surface_at(p) {
                    SurfaceType::PaintedLine => {
                        paint += 1;
                        road += 1;
                    }
                    SurfaceType::Asphalt | SurfaceType::Concrete
                        if map.locate(p, None, 0.0, false, true).is_some() =>
                    {
                        road += 1;
                    }
                    _ => {}
                }
                y += 0.37;
            }
            x += 0.37;
        }
        let share = paint as f32 / road.max(1) as f32;
        assert!(road > 10_000, "{road}");
        assert!((0.02..0.15).contains(&share), "painted share {share}");
    }

    /// `cargo test -p wp_world --release --lib surface_timing -- --ignored --nocapture`
    #[test]
    #[ignore = "timing report, run in release"]
    fn surface_timing() {
        let map = CityMap::new(generate(&ProcGenConfig::default()));
        let (lo, hi) = (map.net.bounds_min, map.net.bounds_max);
        let mut rng = wp_math::Rng::new(9);
        let pts: Vec<V3> = (0..200_000)
            .map(|_| {
                [
                    lo[0] + rng.next_f32() * (hi[0] - lo[0]),
                    lo[1] + rng.next_f32() * (hi[1] - lo[1]),
                    0.0,
                ]
            })
            .collect();
        let t0 = std::time::Instant::now();
        let mut n = 0u32;
        for p in &pts {
            n += u32::from(map.surface_at(*p) == SurfaceType::PaintedLine);
        }
        let all = t0.elapsed().as_secs_f64() / pts.len() as f64;
        let t0 = std::time::Instant::now();
        for p in &pts {
            n += u32::from(painted_at(&map, *p));
        }
        let paint = t0.elapsed().as_secs_f64() / pts.len() as f64;
        println!(
            "surface_at {:.2} us/query, of which markings {:.2} us ({n})",
            all * 1e6,
            paint * 1e6
        );
    }

    /// Height of the paving under `q` nearest to `z` (any road with a lane or connector
    /// nearby).
    fn paving_height(map: &CityMap, q: V2, z: f32) -> f32 {
        let mut roads: Vec<u32> = Vec::new();
        for &id in map.segs_near(q) {
            match map.segs[id as usize].kind {
                SegKind::Lane { lane } => roads.push(map.net.lanes[lane as usize].road),
                SegKind::Connector { from, to, .. } => {
                    roads.push(map.net.lanes[from as usize].road);
                    roads.push(map.net.lanes[to as usize].road);
                }
            }
        }
        roads.sort_unstable();
        roads.dedup();
        roads
            .iter()
            .filter_map(|r| map.net.surfaces.get(*r as usize))
            .filter(|s| s.centre.len() >= 2)
            .map(|s| crate::surface::height_under(s, q))
            .filter(|(d, _)| *d < 15.0)
            .map(|(_, h)| h)
            .min_by(|a, b| (a - z).abs().total_cmp(&(b - z).abs()))
            .unwrap_or(f32::NAN)
    }

    /// Share of `m`'s triangles whose centroid `marking_at` reports painted (in that colour);
    /// also asserts every triangle lies just above its road.
    fn mesh_share(map: &CityMap, m: &TriMesh, yellow: bool) -> f32 {
        let mut hit = 0;
        let mut n = 0;
        for t in &m.indices {
            let p: Vec<V3> = t.iter().map(|i| m.positions[*i as usize]).collect();
            let c = [
                (p[0][0] + p[1][0] + p[2][0]) / 3.0,
                (p[0][1] + p[1][1] + p[2][1]) / 3.0,
                (p[0][2] + p[1][2] + p[2][2]) / 3.0 - MESH_LIFT_M,
            ];
            n += 1;
            match marking_at(map, c) {
                Some(MarkingKind::CentreLine | MarkingKind::EdgeLine) if yellow => hit += 1,
                Some(MarkingKind::CentreLine) => {}
                Some(_) if !yellow => hit += 1,
                _ => {}
            }
            // Every mark lies just above its road (streets at 0, the highway viaduct and
            // its ramps above them): each corner on paving (between corners the paint
            // can bridge a step where two ribbons overlap at different heights).
            for v in &p {
                let z = v[2] - MESH_LIFT_M;
                let g = paving_height(map, [v[0], v[1]], z);
                assert!((z - g).abs() < 0.01, "paint at {v:?} over road {g}");
            }
        }
        hit as f32 / n.max(1) as f32
    }

    #[test]
    fn mesh_paints_what_marking_at_reports() {
        let map = small_map();
        let mesh = marking_mesh(&map);
        assert!(mesh.white.indices.len() > 1000 && mesh.yellow.indices.len() > 100);
        for (m, yellow) in [(&mesh.white, false), (&mesh.yellow, true)] {
            let share = mesh_share(&map, m, yellow);
            assert!(
                share > 0.98,
                "yellow {yellow}: {share} of triangles over a marking"
            );
        }
    }

    #[test]
    fn freeway_tapers_are_dotted_and_gores_chevroned() {
        let grid = small_map();
        let vesper = CityMap::new(crate::vesper::generate(
            &crate::vesper::VesperConfig::default(),
        ));
        // Vesper: 22 interchange ramps + 2 per rest stop (row 153).
        for (name, map, ramps) in [("grid", &grid, 4), ("vesper", &vesper, 28)] {
            // One dotted stretch and one gore per ramp.
            let ramp_lanes = lanes_where(map, |_, l| {
                l.kind == LaneKind::Highway && map.paint.road_lanes[l.road as usize] == 1
            });
            assert_eq!(ramp_lanes.len(), ramps, "{name}");
            for &b in &ramp_lanes {
                assert_eq!(map.paint.bare_left[b as usize].len(), 1, "{name} ramp {b}");
            }
            assert_eq!(map.paint.gores.len(), ramps, "{name}");
            // The dotted line along the carriageway's right edge: dots and gaps.
            let (mut dots, mut gaps) = (0, 0);
            for (a, ranges) in map.paint.dotted_right.iter().enumerate() {
                let seg = &map.segs[a];
                let half = seg.width * 0.5;
                for [r0, r1] in ranges {
                    assert!(r1 - r0 > 50.0, "{name} lane {a}: dotted {r0}..{r1}");
                    let mut s = r0 + 1.0;
                    while s < r1 - 1.0 {
                        for lat in [half - 0.05, half + 0.05] {
                            let p = lane_point(map, a as LaneId, s, lat);
                            let sp = seg.project(geom::xy(p)).s;
                            let m = marking_at(map, p);
                            if dot(sp) {
                                assert_eq!(m, Some(MarkingKind::DottedLine), "{name} {a} s {sp}");
                                assert_eq!(map.surface_at(p), SurfaceType::PaintedLine);
                                dots += 1;
                            } else if m.is_none() {
                                gaps += 1;
                            }
                        }
                        s += 0.25;
                    }
                }
            }
            assert!(
                dots > 100 && gaps > 2 * dots - 100,
                "{name}: dots {dots} gaps {gaps}"
            );
            // Chevrons across each gore, with bare concrete between the bars.
            for g in &map.paint.gores {
                let seg = &map.segs[g.main as usize];
                let (mut bars, mut bare) = (0, 0);
                // From 1 m in: the dotted line ends over the narrow end.
                for k in 0..40 {
                    let u = 1.0 + (g.length() - 1.0) * k as f32 / 40.0;
                    let w = g.width_at(u);
                    for j in 1..8 {
                        let x = w * j as f32 / 8.0;
                        let p = lane_point(map, g.main, g.s0 + g.dir * u, seg.width * 0.5 + x);
                        let m = marking_at(map, p);
                        // Where the point projects back (the lane may curve).
                        let pr = seg.project(geom::xy(p));
                        let (u, x) = ((pr.s - g.s0) * g.dir, pr.lateral - seg.width * 0.5);
                        if chevron(u, x, g.width_at(u)) && x < g.reach_at(u) {
                            assert_eq!(m, Some(MarkingKind::GoreChevron), "{name} gore u {u}");
                            assert_eq!(map.surface_at(p), SurfaceType::PaintedLine);
                            bars += 1;
                        } else {
                            assert_eq!(m, None, "{name} gore u {u} x {x}");
                            bare += 1;
                        }
                    }
                }
                assert!(bars > 10 && bare > bars, "{name}: bars {bars} bare {bare}");
            }
            // Lines go on through the merges and diverges, and the paint matches the rules.
            let mut freeway = MarkingMesh::default();
            for seg in &map.segs[map.net.lanes.len()..] {
                paint_connector(map, seg, &mut freeway);
            }
            for g in &map.paint.gores {
                g.paint(map, &mut freeway.white);
            }
            assert!(freeway.white.indices.len() > 1000, "{name}");
            assert!(freeway.yellow.indices.len() > 50, "{name}");
            for (m, yellow) in [(&freeway.white, false), (&freeway.yellow, true)] {
                let share = mesh_share(map, m, yellow);
                assert!(share > 0.98, "{name} yellow {yellow}: share {share}");
            }
        }
    }

    #[test]
    fn deterministic_answer() {
        let a = small_map();
        let b = small_map();
        let mut h1 = 0u64;
        let mut h2 = 0u64;
        for k in 0..20_000u32 {
            let p = [(k % 200) as f32 * 1.13, (k / 200) as f32 * 1.07, 0.0];
            h1 = h1
                .wrapping_mul(31)
                .wrapping_add(marking_at(&a, p).map_or(0, |m| m as u64 + 1));
            h2 = h2
                .wrapping_mul(31)
                .wrapping_add(marking_at(&b, p).map_or(0, |m| m as u64 + 1));
        }
        assert_eq!(h1, h2);
    }
}
