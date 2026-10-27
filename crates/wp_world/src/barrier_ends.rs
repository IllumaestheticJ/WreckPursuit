//! Crash cushions at blunt barrier ends (owner 2026-10-27, Loop 9: "sections of railing that
//! abruptly start that will cause massive crashes - either fix or put water barrels in front
//! of all areas where barriers can cause massive impacts").
//!
//! A concrete barrier or steel rail that starts beside a fast lane shows its blunt end to the
//! traffic coming at it: at a gore, where a median opens at a crossover, where an edge
//! barrier begins after a gap. [`exposed_ends`] finds every such end (an end of a median,
//! edge, deck or sidewalk-back rail line that faces oncoming highway traffic and does not
//! join another barrier), and [`water_barrels`] stands a graded cushion of water-filled
//! barrels in front of each one, as real highways do: World props, fixed until a car comes near, then
//! light enough to be shoved and burst aside, so a car that runs into one is slowed over a
//! few metres instead of stopped dead by the concrete.
//!
//! Derived from the map alone (no generator or road file change): every city with highways
//! gets them, Ashpeak's Loop 9 included.

use crate::geom::{self, V2, V3};
use crate::network::LaneKind;
use crate::props::{PropKind, StaticProp};
use crate::CityMap;

/// What kind of barrier line an end belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BarrierKind {
    /// Concrete median barrier between carriageways.
    Median,
    /// Concrete barrier along a highway's outer edge.
    Edge,
    /// Steel guard rail along a bridge deck.
    Deck,
    /// Rail behind a sidewalk where the ground falls away.
    WalkBack,
}

impl BarrierKind {
    /// Short name (probes and logs).
    pub fn name(self) -> &'static str {
        match self {
            BarrierKind::Median => "median",
            BarrierKind::Edge => "edge",
            BarrierKind::Deck => "deck",
            BarrierKind::WalkBack => "walk",
        }
    }
}

/// A barrier end that faces oncoming traffic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarrierEnd {
    /// Barrier kind.
    pub kind: BarrierKind,
    /// The end point (foot of the barrier's centre line), m.
    pub pos: V3,
    /// Unit direction out of the barrier through its end (the way the end faces), plan.
    pub out: V2,
    /// The lane whose traffic runs at it (the closest one).
    pub lane: usize,
    /// Plan distance from that lane's centre line to the end, m.
    pub lateral_m: f32,
}

/// A lane counts as running at an end when its centre line passes within this distance of
/// it, m (the lane itself, or the next one over: a car drifting a lane hits it).
pub const LANE_REACH_M: f32 = 5.5;
/// ... and runs at it within this angle of head-on, rad (≈ 30°).
const HEAD_ON_COS: f32 = 0.866;
/// ... on a lane at the end's height (not a deck above or a road below), m.
const SAME_LEVEL_M: f32 = 2.5;
/// Lanes slower than this never get cushions, m/s (≈ 50 km/h: city streets).
const FAST_LANE_MPS: f32 = 14.0;
/// An end within this distance of another barrier line continues into it (a joined run, a
/// deck rail taking over from a K-rail): not exposed, m.
const JOINED_M: f32 = 1.5;
/// Ends closer together than this share one cushion, m (both ends of a short gap facing the
/// same way, the two rails of one deck end).
const MERGE_M: f32 = 3.0;

/// Every barrier line of `map` with its kind: median and edge barriers, deck rails and
/// sidewalk bare-back rails (deck and walk rails as chained runs).
pub fn barrier_lines(map: &CityMap) -> Vec<(BarrierKind, Vec<V3>)> {
    let mut out: Vec<(BarrierKind, Vec<V3>)> = Vec::new();
    for l in crate::surface::median_barriers(&map.net) {
        out.push((BarrierKind::Median, l));
    }
    for l in crate::surface::edge_barriers(map) {
        out.push((BarrierKind::Edge, l));
    }
    for run in chain(&crate::surface::deck_rails(&map.net)) {
        out.push((BarrierKind::Deck, run));
    }
    for run in chain(&crate::surface::sidewalk_mesh(map).bare_backs) {
        out.push((BarrierKind::WalkBack, run));
    }
    out.retain(|(_, l)| l.len() >= 2);
    out
}

/// Chain `a → b` segments into polylines where one starts exactly where the last ended.
fn chain(segs: &[(V3, V3)]) -> Vec<Vec<V3>> {
    let mut runs: Vec<Vec<V3>> = Vec::new();
    for &(a, b) in segs {
        match runs.last_mut() {
            Some(run) if geom::dist(geom::xy(*run.last().unwrap()), geom::xy(a)) < 0.01 => {
                run.push(b)
            }
            _ => runs.push(vec![a, b]),
        }
    }
    runs
}

/// Plan distance from `p` to the polyline `line`.
fn dist_to_line(line: &[V3], p: V2) -> f32 {
    line.windows(2)
        .map(|w| {
            let (a, b) = (geom::xy(w[0]), geom::xy(w[1]));
            let ab = geom::sub(b, a);
            let l2 = ab[0] * ab[0] + ab[1] * ab[1];
            let t = if l2 > 1e-9 {
                (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            geom::dist(p, [a[0] + ab[0] * t, a[1] + ab[1] * t])
        })
        .fold(f32::INFINITY, f32::min)
}

/// Every barrier end of `map` that faces oncoming traffic on a fast highway lane (see the
/// module docs), in barrier-line order, ends of one cushion merged.
pub fn exposed_ends(map: &CityMap) -> Vec<BarrierEnd> {
    let net = &map.net;
    let lines = barrier_lines(map);
    let lanes: Vec<usize> = (0..net.lanes.len())
        .filter(|&i| {
            let l = &net.lanes[i];
            l.kind == LaneKind::Highway && l.speed_limit_mps >= FAST_LANE_MPS
        })
        .collect();
    let mut out: Vec<BarrierEnd> = Vec::new();
    for (li, (kind, line)) in lines.iter().enumerate() {
        let n = line.len();
        for (p, q) in [(line[0], line[1]), (line[n - 1], line[n - 2])] {
            let out_dir = geom::normalize(geom::sub(geom::xy(p), geom::xy(q)));
            let pe = geom::xy(p);
            // Joined to another line (or this one, closing a loop).
            let joined = lines.iter().enumerate().any(|(lj, (_, other))| {
                if lj == li {
                    let (a, b) = (geom::xy(line[0]), geom::xy(line[n - 1]));
                    return n > 2 && geom::dist(a, b) < JOINED_M;
                }
                dist_to_line(other, pe) < JOINED_M
            });
            if joined {
                continue;
            }
            // The closest lane running at it.
            let mut best: Option<(usize, f32)> = None;
            for &i in &lanes {
                let pts = &net.lanes[i].polyline;
                let cum = geom::cumulative(pts);
                let pr = geom::project(pts, &cum, pe);
                let len = cum[cum.len() - 1];
                if pr.dist > LANE_REACH_M || pr.s <= 0.0 || pr.s >= len {
                    continue;
                }
                let (lp, dir) = geom::point_at(pts, &cum, pr.s);
                if (lp[2] - p[2]).abs() > SAME_LEVEL_M {
                    continue;
                }
                // Travel straight into the end: against its outward direction.
                if -(dir[0] * out_dir[0] + dir[1] * out_dir[1]) < HEAD_ON_COS {
                    continue;
                }
                if best.is_none_or(|b| pr.dist < b.1) {
                    best = Some((i, pr.dist));
                }
            }
            let Some((lane, lateral_m)) = best else {
                continue;
            };
            if out
                .iter()
                .any(|e| geom::dist(geom::xy(e.pos), pe) < MERGE_M)
            {
                continue;
            }
            out.push(BarrierEnd {
                kind: *kind,
                pos: p,
                out: out_dir,
                lane,
                lateral_m,
            });
        }
    }
    out
}

/// Water barrel radius, m (a 0.9 m drum).
pub const BARREL_RADIUS_M: f32 = 0.45;
/// Water barrel height, m.
pub const BARREL_HEIGHT_M: f32 = 1.0;
/// Rows of the cushion from the barrier end outward: barrels per row and each barrel's mass,
/// kg. Graded like a real sand/water barrel array: the light front barrel takes the first
/// knock, the heavy ones by the concrete soak up the rest, so a car that runs the whole way in
/// has shed most of its speed (≈ 4.7 t in all: a 1.5 t car at 40 m/s that picks every barrel
/// up is down to ≈ 10 m/s) before it reaches the barrier. Never wider than two barrels, so a
/// cushion at a median end stays clear of the lane beside it.
pub const CUSHION_ROWS: [(usize, f32); 5] =
    [(2, 850.0), (2, 650.0), (2, 450.0), (2, 300.0), (1, 150.0)];
/// Spacing between barrel centres, m.
const BARREL_PITCH_M: f32 = 0.95;

/// Depth of a barrel cushion from the barrier end to the far side of its last row, m.
pub fn cushion_depth_m() -> f32 {
    0.1 + (CUSHION_ROWS.len() - 1) as f32 * BARREL_PITCH_M + 2.0 * BARREL_RADIUS_M
}

/// `props` (light props such as crossover cones) with every one standing in line with an
/// [`exposed_ends`] end, less than 8 m out from it, moved [`cushion_depth_m`] + 0.5 m further
/// out along the barrier line: the row keeps its spacing and order and starts beyond the
/// barrels.
pub fn make_room(map: &CityMap, mut props: Vec<StaticProp>) -> Vec<StaticProp> {
    let ends = exposed_ends(map);
    let shift = cushion_depth_m() + 0.5;
    for p in &mut props {
        for e in &ends {
            let d = geom::sub(geom::xy(p.pos), geom::xy(e.pos));
            let along = d[0] * e.out[0] + d[1] * e.out[1];
            let side = geom::right_of(e.out);
            let across = d[0] * side[0] + d[1] * side[1];
            if (0.0..8.0).contains(&along) && across.abs() < 1.5 {
                p.pos[0] += e.out[0] * shift;
                p.pos[1] += e.out[1] * shift;
                break;
            }
        }
    }
    props
}

/// A water barrel prop standing on `foot` (ground under its centre).
pub fn water_barrel(foot: V3, mass_kg: f32) -> StaticProp {
    StaticProp {
        kind: PropKind::WaterBarrel,
        pos: [foot[0], foot[1], foot[2] + BARREL_HEIGHT_M * 0.5],
        yaw: 0.0,
        pitch: 0.0,
        half_extents: [BARREL_RADIUS_M, BARREL_RADIUS_M, BARREL_HEIGHT_M * 0.5],
        mass_kg,
    }
}

/// A cushion of water barrels ([`CUSHION_ROWS`]) in front of every [`exposed_ends`] end, centred
/// on the barrier's line (or moved up to [`MAX_SHIFT_M`] off it, away from the lane running at
/// the end, where it would stand in that lane; in single file, as heavy, where even that is
/// too tight), in end order.
pub fn water_barrels(map: &CityMap) -> Vec<StaticProp> {
    let mut out = Vec::new();
    let widest = CUSHION_ROWS.iter().map(|r| r.0).max().unwrap_or(1);
    let half_span = (widest as f32 - 1.0) * 0.5 * BARREL_PITCH_M + BARREL_RADIUS_M;
    for e in exposed_ends(map) {
        let side = geom::right_of(e.out);
        // The lane's side of the end (+1: along `side`), and how far its paint is from the end.
        let lane = &map.net.lanes[e.lane];
        let cum = geom::cumulative(&lane.polyline);
        let pr = geom::project(&lane.polyline, &cum, geom::xy(e.pos));
        let (lp, _) = geom::point_at(&lane.polyline, &cum, pr.s);
        let to_lane = geom::sub(geom::xy(lp), geom::xy(e.pos));
        let lane_side = if to_lane[0] * side[0] + to_lane[1] * side[1] >= 0.0 {
            1.0
        } else {
            -1.0
        };
        // Room between the barrier line and the lane's paint, the tightest along the cushion
        // (a lane curving towards a gore closes in on it).
        let depth = cushion_depth_m();
        let room = (0..=4)
            .map(|k| {
                let a = depth * k as f32 / 4.0;
                let q = [e.pos[0] + e.out[0] * a, e.pos[1] + e.out[1] * a];
                geom::project(&lane.polyline, &cum, q).dist
            })
            .fold(f32::INFINITY, f32::min)
            - 0.5 * lane.width_m
            - LANE_CLEAR_M;
        // Too tight for two abreast: one file, each barrel as heavy as the row it stands for.
        let single = room + MAX_SHIFT_M < half_span;
        let span = if single { BARREL_RADIUS_M } else { half_span };
        let shift = -lane_side * (span - room).clamp(0.0, MAX_SHIFT_M);
        for (row, &(count, mass_kg)) in CUSHION_ROWS.iter().enumerate() {
            let ahead = BARREL_RADIUS_M + 0.1 + row as f32 * BARREL_PITCH_M;
            let (count, mass_kg) = if single {
                (1, mass_kg * count as f32)
            } else {
                (count, mass_kg)
            };
            for k in 0..count {
                let across = (k as f32 - (count as f32 - 1.0) * 0.5) * BARREL_PITCH_M + shift;
                out.push(water_barrel(
                    [
                        e.pos[0] + e.out[0] * ahead + side[0] * across,
                        e.pos[1] + e.out[1] * ahead + side[1] * across,
                        e.pos[2],
                    ],
                    mass_kg,
                ));
            }
        }
    }
    out
}

/// Gap kept between a cushion and the paint of the lane beside it, m.
const LANE_CLEAR_M: f32 = 0.1;
/// Furthest a cushion moves off its barrier's line to keep out of a lane, m (the barrier end,
/// 0.5 m wide, still lies behind its wide rows).
pub const MAX_SHIFT_M: f32 = 0.45;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vesper::{generate, VesperConfig};

    fn map() -> CityMap {
        CityMap::new(generate(&VesperConfig::default()))
    }

    #[test]
    fn every_crossover_opening_has_a_cushion_at_both_barrier_ends() {
        let map = map();
        let ends = exposed_ends(&map);
        let points = crate::crossover::crossovers(&map.net);
        assert!(!points.is_empty());
        for (i, x) in points.iter().enumerate() {
            // Each barrier end at the opening faces the traffic running at it.
            let at = ends
                .iter()
                .filter(|e| e.kind == BarrierKind::Median)
                .filter(|e| {
                    (geom::dist(geom::xy(e.pos), geom::xy(*x)) - 0.5 * crate::crossover::GAP_M)
                        .abs()
                        < 0.5
                })
                .count();
            assert_eq!(at, 2, "crossover {i}: {at} cushioned ends");
        }
        // And more: the edge barriers that start after gaps along the highways.
        assert!(ends.len() > 2 * points.len(), "{} ends", ends.len());
    }

    #[test]
    fn barrels_stay_off_the_lanes_and_clear_of_each_other() {
        let map = map();
        let barrels = water_barrels(&map);
        let per_end: usize = CUSHION_ROWS.iter().map(|r| r.0).sum();
        let n_ends = exposed_ends(&map).len();
        assert!(barrels.len() >= n_ends * CUSHION_ROWS.len() && barrels.len() <= n_ends * per_end);
        for (i, b) in barrels.iter().enumerate() {
            let q = geom::xy(b.pos);
            for lane in map
                .net
                .lanes
                .iter()
                .filter(|l| l.kind == LaneKind::Highway && l.speed_limit_mps >= FAST_LANE_MPS)
            {
                let cum = geom::cumulative(&lane.polyline);
                let pr = geom::project(&lane.polyline, &cum, q);
                let (lp, _) = geom::point_at(&lane.polyline, &cum, pr.s);
                if (lp[2] - b.pos[2]).abs() > SAME_LEVEL_M + BARREL_HEIGHT_M {
                    continue;
                }
                assert!(
                    pr.dist >= 0.5 * lane.width_m + BARREL_RADIUS_M - 0.05,
                    "barrel {i} at {q:?} stands in a lane ({:.2} m from its centre)",
                    pr.dist
                );
            }
            for (j, o) in barrels.iter().enumerate().skip(i + 1) {
                let d = geom::dist(q, geom::xy(o.pos));
                assert!(d > 1.9 * BARREL_RADIUS_M, "barrels {i} and {j} {d} m apart");
            }
        }
    }

    #[test]
    fn crossover_cones_stand_beyond_the_barrels() {
        let w = crate::World::vesper(&VesperConfig::default(), Default::default());
        let props: Vec<&StaticProp> = w.static_props().collect();
        let cones: Vec<_> = props.iter().filter(|p| p.kind == PropKind::Cone).collect();
        let barrels: Vec<_> = props
            .iter()
            .filter(|p| p.kind == PropKind::WaterBarrel)
            .collect();
        assert!(!cones.is_empty() && !barrels.is_empty());
        for c in &cones {
            for b in &barrels {
                let d = geom::dist(geom::xy(c.pos), geom::xy(b.pos));
                assert!(
                    d > BARREL_RADIUS_M + 0.3,
                    "cone {:?} in a cushion ({d} m)",
                    c.pos
                );
            }
        }
    }
}
