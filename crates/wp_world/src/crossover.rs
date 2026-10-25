//! Median crossovers: openings in a divided highway's median barrier, marked with traffic
//! cones, where a car can cross to the other carriageway (owner request 2026-10-25: "add
//! cones in select sections of loop 9 so we can cross over the center divider").
//!
//! The generator picks the spots (Loop 9: `vesper::crossovers`) and records each as a
//! [`PoiKind::MedianCrossover`] point on the median line, so no road file layout changes.
//! Everything else is derived from those points:
//! - [`crate::surface::median_barriers`] leaves a [`GAP_M`] opening in the barrier line
//!   centred on each one (physics and the greybox both draw from it);
//! - [`cones`] stands a short row of cones in from each end of the opening (World props:
//!   light, knockable).
//!
//! Both carriageways' median-side paving already meets on the median line, so the opening is
//! paved: nothing about the lanes changes, and traffic keeps to its own carriageway.

use crate::geom::{self, V2, V3};
use crate::network::{PoiKind, RoadNetwork};
use crate::props::{cone, StaticProp};

/// Length of the opening in the median barrier, m.
pub const GAP_M: f32 = 30.0;
/// Cones stand on the median line this far in from each barrier end, m: the clear opening
/// between the innermost cones is `GAP_M - 2 * 4.0` = 22 m.
pub const CONE_INSET_M: [f32; 3] = [1.0, 2.5, 4.0];
/// A crossover cuts a barrier line only if its point lies this close to the line, m.
const ON_LINE_M: f32 = 1.5;

/// The median crossover points of a network (plan position on the median line).
pub fn crossovers(net: &RoadNetwork) -> Vec<V3> {
    net.pois
        .iter()
        .filter(|p| p.kind == PoiKind::MedianCrossover)
        .map(|p| p.pos)
        .collect()
}

/// `line` (a median barrier polyline) with a [`GAP_M`] opening cut around every crossover
/// in `points` that lies on it: the pieces left, in order (pieces shorter than 0.5 m are
/// dropped).
pub fn cut_line(line: &[V3], points: &[V3]) -> Vec<Vec<V3>> {
    if line.len() < 2 {
        return vec![line.to_vec()];
    }
    let cum = geom::cumulative(line);
    let len = cum[cum.len() - 1];
    let mut gaps: Vec<(f32, f32)> = points
        .iter()
        .filter_map(|p| {
            let pr = geom::project(line, &cum, geom::xy(*p));
            (pr.dist < ON_LINE_M && (pr.s >= 0.0 && pr.s <= len))
                .then_some((pr.s - 0.5 * GAP_M, pr.s + 0.5 * GAP_M))
        })
        .collect();
    if gaps.is_empty() {
        return vec![line.to_vec()];
    }
    gaps.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut from = 0.0f32;
    for (g0, g1) in gaps {
        if g0 > from + 0.5 {
            out.push(geom::trim_polyline(line, from, len - g0.min(len)));
        }
        from = from.max(g1);
    }
    if len > from + 0.5 {
        out.push(geom::trim_polyline(line, from, 0.0));
    }
    out
}

/// Cones at every crossover: [`CONE_INSET_M`] in from each end of the opening, on the median
/// line at road height, in crossover order.
pub fn cones(net: &RoadNetwork) -> Vec<StaticProp> {
    let points = crossovers(net);
    let mut out = Vec::new();
    if points.is_empty() {
        return out;
    }
    for line in crate::surface::median_barrier_lines(net) {
        let cum = geom::cumulative(&line);
        let len = cum[cum.len() - 1];
        for p in &points {
            let pr = geom::project(&line, &cum, geom::xy(*p));
            if pr.dist >= ON_LINE_M || pr.s < 0.0 || pr.s > len {
                continue;
            }
            for end in [-1.0f32, 1.0] {
                for inset in CONE_INSET_M {
                    let s = pr.s + end * (0.5 * GAP_M - inset);
                    if s < 0.0 || s > len {
                        continue;
                    }
                    let (q, dir) = geom::point_at(&line, &cum, s);
                    out.push(cone(q, geom::yaw_of(dir)));
                }
            }
        }
    }
    out
}

/// Whether plan point `q` lies within an opening (tests and diagnostics).
pub fn in_opening(net: &RoadNetwork, q: V2) -> bool {
    crossovers(net)
        .iter()
        .any(|p| geom::dist(geom::xy(*p), q) < 0.5 * GAP_M)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::PoiKind;
    use crate::surface::{edge_barriers, median_barrier_lines, median_barriers, surface_edges};
    use crate::vesper::{generate, VesperConfig};

    fn net() -> &'static RoadNetwork {
        static NET: std::sync::OnceLock<RoadNetwork> = std::sync::OnceLock::new();
        NET.get_or_init(|| generate(&VesperConfig::default()))
    }

    /// Distance from `q` to the nearest point of any line in `lines` (plan).
    fn nearest(lines: &[Vec<V3>], q: V2) -> f32 {
        lines
            .iter()
            .flat_map(|l| l.windows(2))
            .map(|w| geom::point_segment_distance(q, geom::xy(w[0]), geom::xy(w[1])))
            .fold(f32::INFINITY, f32::min)
    }

    #[test]
    fn loop9_has_crossovers_spread_round_the_loop() {
        let pts = crossovers(net());
        assert_eq!(pts.len(), 4, "{pts:?}");
        let lines = median_barrier_lines(net());
        for (i, p) in pts.iter().enumerate() {
            assert!(
                nearest(&lines, geom::xy(*p)) < 0.5,
                "crossover {i} off the median"
            );
            for q in &pts[i + 1..] {
                assert!(geom::dist(geom::xy(*p), geom::xy(*q)) > 400.0);
            }
        }
        let names: Vec<&str> = net()
            .pois
            .iter()
            .filter(|p| p.kind == PoiKind::MedianCrossover)
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names[0], "Loop 9 crossover 1");
        // Deterministic.
        assert_eq!(pts, crossovers(&generate(&VesperConfig::default())));
    }

    #[test]
    fn the_barrier_is_open_at_each_crossover_and_only_there() {
        let barriers = median_barriers(net());
        let lines = median_barrier_lines(net());
        let full: f32 = lines
            .iter()
            .map(|l| geom::cumulative(l).last().copied().unwrap_or(0.0))
            .sum();
        let cut: f32 = barriers
            .iter()
            .map(|l| geom::cumulative(l).last().copied().unwrap_or(0.0))
            .sum();
        let pts = crossovers(net());
        let opened = full - cut;
        let want = GAP_M * pts.len() as f32;
        assert!(
            (opened - want).abs() < 1.0,
            "opened {opened} m, want {want}"
        );
        let edges = edge_barriers(&crate::CityMap::new(net().clone()));
        for p in &pts {
            let q = geom::xy(*p);
            assert!(nearest(&barriers, q) > 0.5 * GAP_M - 1.0);
            // No outer-edge barrier fills the opening either.
            assert!(nearest(&edges, q) > 8.0, "edge barrier at {q:?}");
        }
    }

    #[test]
    fn both_carriageways_meet_level_in_each_opening() {
        let net = net();
        for p in crossovers(net) {
            let q = geom::xy(p);
            // Every highway surface's left (median-side) edge near the opening.
            let mut zs = Vec::new();
            for s in net.surfaces.iter().filter(|s| s.highway) {
                let (l, _) = surface_edges(s);
                let cum = geom::cumulative(&l);
                let pr = geom::project(&l, &cum, q);
                if pr.dist < 0.5 && pr.s > 0.0 && pr.s < cum[cum.len() - 1] {
                    zs.push(geom::point_at(&l, &cum, pr.s).0[2]);
                }
            }
            assert_eq!(zs.len(), 2, "carriageway edges at {q:?}: {zs:?}");
            assert!((zs[0] - zs[1]).abs() < 0.05, "step {zs:?} at {q:?}");
        }
    }

    #[test]
    fn cones_mark_the_ends_of_each_opening() {
        let pts = crossovers(net());
        let cones = cones(net());
        assert_eq!(cones.len(), pts.len() * 2 * CONE_INSET_M.len());
        for c in &cones {
            let d = pts
                .iter()
                .map(|p| geom::dist(geom::xy(*p), geom::xy(c.pos)))
                .fold(f32::INFINITY, f32::min);
            // Inside the opening, clear of its middle.
            assert!(
                d < 0.5 * GAP_M && d > 0.5 * GAP_M - 4.5,
                "cone {d} m from centre"
            );
        }
        // The world stands them up as props.
        let w = crate::World::vesper(&VesperConfig::default(), Default::default());
        let in_world = w
            .static_props()
            .filter(|p| p.kind == crate::props::PropKind::Cone)
            .count();
        assert_eq!(in_world, cones.len());
    }

    #[test]
    fn cities_without_crossovers_are_unchanged() {
        let grid = crate::procgen::generate(&Default::default());
        assert!(crossovers(&grid).is_empty());
        assert_eq!(median_barriers(&grid), median_barrier_lines(&grid));
    }
}
