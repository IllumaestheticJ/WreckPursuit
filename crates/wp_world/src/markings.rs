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
//!
//! Alleys and parking strips carry no markings; highways get centre, lane and edge lines.
//! A line straddling two lanes is split between them (each lane owns its half), so the
//! answer does not depend on which lane a query hits first.

use crate::geom::{self, V3};
use crate::map::{CityMap, SegKind};
use crate::network::{IntersectionControl, Lane, LaneId, LaneKind};

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
        let SegKind::Lane { lane } = seg.kind else {
            continue;
        };
        let l = &net.lanes[lane as usize];
        if matches!(l.kind, LaneKind::Parking | LaneKind::Alley) {
            continue;
        }
        let pr = seg.project(q);
        let half = seg.width * 0.5;
        if pr.lateral.abs() > half {
            continue;
        }
        let len = seg.length;
        if (seg.point_at(pr.s.clamp(0.0, len)).0[2] - p[2]).abs() > LEVEL_TOL_M {
            continue;
        }
        let m = lane_marking(map, lane, l, pr.s, pr.lateral, half, len);
        // Several lanes can claim a point only at their shared seams; pick deterministically.
        if let Some(k) = m {
            found = Some(found.map_or(k, |f| f.min(k)));
        }
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
        if from_right <= EDGE_LINE_W {
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
    // Left boundary.
    if l.edge_left {
        if from_left <= EDGE_LINE_W {
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

/// Dash pattern from the lane start.
fn dash(s: f32) -> bool {
    let period = DASH_M + DASH_GAP_M;
    s - (s / period).floor() * period < DASH_M
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
            !ap.major && map.net.lanes[id as usize].kind != LaneKind::Highway
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
