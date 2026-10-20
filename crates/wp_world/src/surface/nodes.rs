//! Paved areas at road graph nodes: junction surfaces, bend strips and dead-end turnarounds.
//!
//! Every road ribbon ([`super::surface_edges`]) stops short of its end nodes; this module paves
//! what lies between the ribbon ends so the city has no holes:
//! - **junction** (a node with an intersection): one polygon whose boundary *is* each arriving
//!   ribbon's end edge (the same two corner vertices, so no crack and no step), joined from one
//!   ribbon's corner to the next by a kerb curve: a circular fillet tangent to both kerb lines
//!   where they meet in front of the junction, a straight kerb where they are (nearly) in line,
//!   and an arc around the centre on the outside of a wide (> 170°) gap. Heights: the corners
//!   keep their ribbon heights, everything else lies on the least-squares plane through the
//!   corners (roads are graded slightly up to the junction centre, so a flat disc at the node
//!   height left 5–15 cm steps at the junction mouths);
//! - **bend** (two roads, no intersection): a strip from one ribbon's end edge to the other's,
//!   its sides two Bézier curves tangent to the road edges (full width: parking lanes and the
//!   insides of tight bends included);
//! - **dead end** (one road): the end edge closed by a half-round turnaround, long enough to hold
//!   the U-turn paths.
//!
//! [`NodePatch::rims`] are the kerb curves between ribbons, for the sidewalk corner fills.

use std::collections::BTreeMap;

use crate::geom::{self, V2, V3};
use crate::map::{CityMap, SegKind};
use crate::network::RoadNetwork;

use super::{surface_edges, TriMesh};

/// What kind of node a [`NodePatch`] paves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodePatchKind {
    /// A node with an intersection (three or more roads, or a signalised two-road node).
    Junction,
    /// Two roads meeting without an intersection.
    Bend,
    /// A single road ending.
    DeadEnd,
}

/// The pavement at one node.
#[derive(Debug, Clone, PartialEq)]
pub struct NodePatch {
    /// Node id.
    pub node: u32,
    /// Junction, bend or dead end.
    pub kind: NodePatchKind,
    /// Every road at the node is a highway (concrete, no sidewalks).
    pub highway: bool,
    /// Up-facing triangles.
    pub mesh: TriMesh,
    /// Some ribbon ends overlap (roads leaving at a sharp angle): they share one opening and
    /// the ribbons lie partly on top of the patch, at their own heights.
    pub overlapping_ends: bool,
    /// Kerb curves between consecutive ribbon ends, counter-clockwise around the node: each
    /// starts at one ribbon's corner and ends at the next ribbon's corner (both included). A
    /// dead end has one rim from one corner of its road around the turnaround to the other.
    pub rims: Vec<Vec<V3>>,
}

/// One ribbon end at a node.
#[derive(Debug, Clone, Copy)]
struct End {
    /// Corner on the clockwise side (right of `out` looking away from the node).
    cw: V3,
    /// Corner on the counter-clockwise side.
    ccw: V3,
    /// Unit direction along the road at the clockwise corner, away from the node.
    out: V2,
    /// The same at the counter-clockwise corner (differs from `out` only for merged ends).
    out_ccw: V2,
    /// Highway surface.
    highway: bool,
}

/// Ribbon ends per node (index = node id).
fn ends_per_node(net: &RoadNetwork) -> Vec<Vec<End>> {
    let mut per: Vec<Vec<End>> = vec![Vec::new(); net.nodes.len()];
    let mut road_nodes: BTreeMap<u32, [u32; 2]> = BTreeMap::new();
    for l in &net.lanes {
        road_nodes.entry(l.road).or_insert([l.from_node, l.to_node]);
    }
    for s in &net.surfaces {
        let n = s.centre.len();
        let Some(nodes) = road_nodes.get(&s.road) else {
            continue;
        };
        if n < 2 {
            continue;
        }
        let (left, right) = surface_edges(s);
        let first = geom::xy(s.centre[0]);
        let last = geom::xy(s.centre[n - 1]);
        let pos = |id: u32| geom::xy(net.nodes[id as usize].pos);
        // Which node each end belongs to (by distance; both when the road is a loop).
        let (n0, n1) = if geom::dist(first, pos(nodes[0])) + geom::dist(last, pos(nodes[1]))
            <= geom::dist(first, pos(nodes[1])) + geom::dist(last, pos(nodes[0]))
        {
            (nodes[0], nodes[1])
        } else {
            (nodes[1], nodes[0])
        };
        // Start: outward = along the surface; its left edge is the counter-clockwise side.
        per[n0 as usize].push(End {
            cw: right[0],
            ccw: left[0],
            out: geom::normalize(geom::sub(geom::xy(s.centre[1]), first)),
            out_ccw: geom::normalize(geom::sub(geom::xy(s.centre[1]), first)),
            highway: s.highway,
        });
        per[n1 as usize].push(End {
            cw: left[n - 1],
            ccw: right[n - 1],
            out: geom::normalize(geom::sub(geom::xy(s.centre[n - 2]), last)),
            out_ccw: geom::normalize(geom::sub(geom::xy(s.centre[n - 2]), last)),
            highway: s.highway,
        });
    }
    per
}

/// The paved patch of every node that has a ribbon ending at it, in node order.
pub fn node_patches(map: &CityMap) -> Vec<NodePatch> {
    let net = &map.net;
    let per = ends_per_node(net);
    let radii = super::junction_paved_radii(net);
    let mut out = Vec::new();
    for (node, ends) in per.iter().enumerate() {
        if ends.is_empty() {
            continue;
        }
        let nd = &net.nodes[node];
        let c = nd.pos;
        let highway = ends.iter().all(|e| e.highway);
        let patch = match (nd.intersection, ends.len()) {
            (None, 1) => Some(dead_end(map, node as u32, c, &ends[0])),
            // A bend is a two-road junction (same kerb curves, same turning-path check); the
            // plain strip between the two ends only if that outline is degenerate.
            (None, 2) => junction(c, ends, &turning_footprint(map, node as u32))
                .map(|(_, m, r)| (NodePatchKind::Bend, m, r))
                .or_else(|| bend(&ends[0], &ends[1])),
            _ => junction(c, ends, &turning_footprint(map, node as u32)),
        };
        let (kind, mesh, rims) = match patch {
            Some(p) => p,
            // Degenerate layout (overlapping ribbon ends): the old flat disc reaching every
            // corner, overlapping the ribbons.
            None => {
                let r = nd.intersection.map_or_else(
                    || {
                        ends.iter()
                            .flat_map(|e| [e.cw, e.ccw])
                            .map(|p| geom::dist(geom::xy(p), geom::xy(c)))
                            .fold(0.0f32, f32::max)
                            + 0.05
                    },
                    |ix| radii[ix as usize],
                );
                let poly: Vec<V3> = super::junction_polygon(geom::xy(c), r)
                    .iter()
                    .map(|q| [q[0], q[1], c[2]])
                    .collect();
                let mut m = TriMesh::default();
                m.fan(&poly);
                let kind = if nd.intersection.is_some() {
                    NodePatchKind::Junction
                } else if ends.len() == 1 {
                    NodePatchKind::DeadEnd
                } else {
                    NodePatchKind::Bend
                };
                (kind, m, Vec::new())
            }
        };
        let overlapping_ends = ends.len() > 1 && merge_spans(geom::xy(c), ends).len() < ends.len();
        out.push(NodePatch {
            node: node as u32,
            kind,
            highway,
            overlapping_ends,
            mesh,
            rims,
        });
    }
    out
}

type Patch = (NodePatchKind, TriMesh, Vec<Vec<V3>>);

/// Counter-clockwise angle of `p` around `c`, in [0, 2π).
fn angle_around(c: V2, p: V2) -> f32 {
    let d = geom::sub(p, c);
    wp_math::atan2(d[1], d[0]).rem_euclid(std::f32::consts::TAU)
}

/// Footprint sample points of the drivable turning paths through `node`: along each connector
/// every ~0.5 m, on its centreline and [`PATH_MARGIN_M`] inside each edge.
fn turning_footprint(map: &CityMap, node: u32) -> Vec<V2> {
    let mut out = Vec::new();
    for s in &map.segs {
        let SegKind::Connector { node: nd, .. } = s.kind else {
            continue;
        };
        if nd != node || !s.drivable {
            continue;
        }
        let half = (s.width * 0.5 - PATH_MARGIN_M).max(0.0);
        let mut d = 0.0;
        while d <= s.length {
            let (p, dir) = s.point_at(d);
            let r = geom::right_of(dir);
            for lat in [-half, 0.0, half] {
                out.push(geom::add(geom::xy(p), geom::scale(r, lat)));
            }
            d += 0.5;
        }
    }
    out
}

/// How far inside a turning path's edges the footprint samples lie, m (the pavement reaches
/// 5 cm past them).
pub const PATH_MARGIN_M: f32 = 0.1;

/// The ribbon ends at a node grouped into openings: the union of their angular spans
/// (clockwise → counter-clockwise corner around `c2`), each with its member ends; ends whose
/// spans overlap (roads leaving at a sharp angle, ribbons crossing near the node) share one.
fn merge_spans(c2: V2, ends: &[End]) -> Vec<(End, Vec<End>)> {
    let tau = std::f32::consts::TAU;
    let rel = |from: V3, x: V3| {
        (angle_around(c2, geom::xy(x)) - angle_around(c2, geom::xy(from))).rem_euclid(tau)
    };
    let mut by_cw: Vec<End> = ends.to_vec();
    by_cw.sort_by(|a, b| {
        angle_around(c2, geom::xy(a.cw)).total_cmp(&angle_around(c2, geom::xy(b.cw)))
    });
    let mut merged: Vec<(End, Vec<End>)> = Vec::new();
    for e in by_cw {
        if let Some((last, members)) = merged.last_mut() {
            if rel(last.cw, e.cw) <= rel(last.cw, last.ccw) {
                if rel(last.cw, e.ccw) > rel(last.cw, last.ccw) {
                    last.ccw = e.ccw;
                    last.out_ccw = e.out_ccw;
                }
                last.highway |= e.highway;
                members.push(e);
                continue;
            }
        }
        merged.push((e, vec![e]));
    }
    // The last span may wrap past 0 into the first.
    while merged.len() > 1 {
        let (first, last) = (merged[0].0, merged[merged.len() - 1].0);
        if rel(last.cw, first.cw) > rel(last.cw, last.ccw) {
            break;
        }
        let (_, last_members) = merged.pop().unwrap_or((last, Vec::new()));
        let head = &mut merged[0];
        head.0.cw = last.cw;
        head.0.out = last.out;
        if rel(last.cw, first.ccw) < rel(last.cw, last.ccw) {
            head.0.ccw = last.ccw;
            head.0.out_ccw = last.out_ccw;
        }
        head.0.highway |= last.highway;
        head.1.extend(last_members);
    }
    merged
}

/// Junction polygon: ribbon end edges joined by kerb curves; fan from the centre.
fn junction(c: V3, ends: &[End], paths: &[V2]) -> Option<Patch> {
    let c2 = geom::xy(c);
    let tau = std::f32::consts::TAU;
    // Ends whose spans overlap (roads meeting at a sharp angle, their ribbons crossing near
    // the junction) share one opening, from the first's clockwise corner to the last's
    // counter-clockwise one.
    let rel = |from: V3, x: V3| {
        (angle_around(c2, geom::xy(x)) - angle_around(c2, geom::xy(from))).rem_euclid(tau)
    };
    let mut merged = merge_spans(c2, ends);
    // Back to counter-clockwise order; a merged opening's outline follows the outermost of its
    // members' end edges (in 2° steps) so it reaches under every member ribbon.
    merged.sort_by(|a, b| {
        angle_around(c2, geom::xy(a.0.cw)).total_cmp(&angle_around(c2, geom::xy(b.0.cw)))
    });
    let envelopes: Vec<Vec<V2>> = merged
        .iter()
        .map(|(e, members)| {
            if members.len() < 2 {
                return Vec::new();
            }
            let edges: Vec<[V2; 2]> = members
                .iter()
                .map(|m| [geom::xy(m.cw), geom::xy(m.ccw)])
                .collect();
            let a0 = angle_around(c2, geom::xy(e.cw));
            let span = rel(e.cw, e.ccw);
            let steps = ((span / HUG_STEP).ceil() as usize).max(2);
            (1..steps)
                .filter_map(|i| {
                    let ang = a0 + span * i as f32 / steps as f32;
                    let (sn, cs) = wp_math::sin_cos(ang);
                    let r = edges
                        .iter()
                        .filter_map(|[u, v]| ray_hits(c2, [cs, sn], *u, *v))
                        .fold(0.0f32, f32::max);
                    (r > 0.0).then(|| [c2[0] + cs * r, c2[1] + sn * r])
                })
                .collect()
        })
        .collect();
    let merged: Vec<End> = merged.into_iter().map(|(e, _)| e).collect();
    let sorted: Vec<&End> = merged.iter().collect();
    // The corners must alternate cw, ccw, cw, ccw … going around.
    let first = angle_around(c2, geom::xy(sorted[0].cw));
    let mut prev = 0.0f32;
    for e in &sorted {
        for p in [e.cw, e.ccw] {
            let a = (angle_around(c2, geom::xy(p)) - first).rem_euclid(tau);
            if a + 1e-4 < prev {
                return None;
            }
            prev = a;
        }
    }
    if prev >= tau - 1e-4 {
        return None;
    }
    let n = sorted.len();
    let curves: Vec<Vec<V2>> = (0..n)
        .map(|k| {
            let (a, b) = (sorted[k], sorted[(k + 1) % n]);
            hug_paths(
                c2,
                geom::xy(a.ccw),
                geom::xy(b.cw),
                kerb_curve(c2, a, b),
                paths,
            )
        })
        .collect();
    // Heights: plane through every corner.
    let corners: Vec<V3> = sorted.iter().flat_map(|e| [e.cw, e.ccw]).collect();
    let plane = super::fit_plane(&corners, c2);
    let z_at = |p: V2| plane[0] + plane[1] * (p[0] - c2[0]) + plane[2] * (p[1] - c2[1]);
    let mut ring: Vec<V3> = Vec::new();
    let mut rims: Vec<Vec<V3>> = Vec::new();
    for (k, e) in sorted.iter().enumerate() {
        let next = sorted[(k + 1) % n];
        ring.push(e.cw);
        let (rc, rd) = (
            e.cw[2] - z_at(geom::xy(e.cw)),
            e.ccw[2] - z_at(geom::xy(e.ccw)),
        );
        let m = envelopes[k].len();
        ring.extend(envelopes[k].iter().enumerate().map(|(i, p)| {
            let f = (i + 1) as f32 / (m + 1) as f32;
            [p[0], p[1], z_at(*p) + rc + (rd - rc) * f]
        }));
        ring.push(e.ccw);
        // Kerb heights: the plane, plus the corners' own offsets from it blended along the
        // curve, so the rim meets both corners exactly.
        let (pa, pb) = (geom::xy(e.ccw), geom::xy(next.cw));
        let (ra, rb) = (e.ccw[2] - z_at(pa), next.cw[2] - z_at(pb));
        let mut along = vec![0.0f32];
        let mut prev = pa;
        for p in curves[k].iter().chain(std::iter::once(&pb)) {
            along.push(along[along.len() - 1] + geom::dist(prev, *p));
            prev = *p;
        }
        let total = along[along.len() - 1].max(1e-6);
        let mut rim = vec![e.ccw];
        for (i, p) in curves[k].iter().enumerate() {
            let f = along[i + 1] / total;
            let q = [p[0], p[1], z_at(*p) + ra + (rb - ra) * f];
            ring.push(q);
            rim.push(q);
        }
        rim.push(next.cw);
        rims.push(rim);
    }
    let centre = [c2[0], c2[1], z_at(c2)];
    let mut mesh = TriMesh::default();
    if !fan_around(&mut mesh, centre, &ring) && !polygon_mesh(&mut mesh, &ring) {
        return None;
    }
    Some((NodePatchKind::Junction, mesh, rims))
}

/// Append the closed counter-clockwise `ring` fanned from `centre`; false (nothing appended)
/// when the ring is not star-shaped around it (a triangle would face down or be degenerate).
fn fan_around(mesh: &mut TriMesh, centre: V3, ring: &[V3]) -> bool {
    let n = ring.len();
    let c2 = geom::xy(centre);
    for k in 0..n {
        let (a, b) = (geom::xy(ring[k]), geom::xy(ring[(k + 1) % n]));
        if geom::cross(geom::sub(a, c2), geom::sub(b, c2)) <= 1e-5 {
            return false;
        }
    }
    let base = mesh.positions.len() as u32;
    mesh.positions.push(centre);
    mesh.positions.extend_from_slice(ring);
    for k in 0..n as u32 {
        mesh.indices
            .push([base, base + 1 + k, base + 1 + (k + 1) % n as u32]);
    }
    true
}

/// Interior points of the kerb between `a`'s counter-clockwise corner `p` and `b`'s clockwise
/// corner `q`, going counter-clockwise around the junction centre `c`:
/// - kerb lines meeting in front of both corners: a circular fillet tangent to both, its
///   radius capped so it stays within the sidewalk band (a highway edge has no kerb: the
///   sharp corner where the edges meet);
/// - kerb lines in line or meeting behind the corners: a curve tangent to both road edges
///   (straight when in line, bulging out on the outside of a bend like the lane connectors);
/// - when that curve would not sweep steadily round the centre: an arc around the centre.
fn kerb_curve(c: V2, a: &End, b: &End) -> Vec<V2> {
    let (p, q) = (geom::xy(a.ccw), geom::xy(b.cw));
    // Kerb directions away from the junction at p and q.
    let a = &End {
        out: a.out_ccw,
        ..*a
    };
    let tau = std::f32::consts::TAU;
    let gap = (angle_around(c, q) - angle_around(c, p)).rem_euclid(tau);
    // Kerb lines p + s·a.out and q + u·b.out.
    let den = geom::cross(a.out, b.out);
    let cos_beta = geom::dot(a.out, b.out);
    if den.abs() > 0.05 {
        let s = geom::cross(geom::sub(q, p), b.out) / den;
        let u = geom::cross(geom::sub(q, p), a.out) / den;
        if s < -0.05 && u < -0.05 && cos_beta > -0.966 && gap < std::f32::consts::PI {
            let k = geom::add(p, geom::scale(a.out, s));
            if a.highway || b.highway {
                // No kerb on a highway edge: pave up to where the edges meet.
                return vec![k];
            }
            // Interior angle β at k; the fillet's deepest point lies t·(1 − sin β/2)/cos β/2
            // past k, the sidewalk's outer corner W / sin β/2: keep the fillet within 60 % of
            // that, and its radius t·tan β/2 within MAX_KERB_RADIUS_M.
            let beta = wp_math::atan2(den.abs(), cos_beta);
            let (sh, ch) = wp_math::sin_cos(beta * 0.5);
            let depth_cap = 0.6 * super::SIDEWALK_W_M / sh.max(1e-3) * ch / (1.0 - sh).max(1e-3);
            let radius_cap = MAX_KERB_RADIUS_M * ch / sh.max(1e-3);
            let t = (-s).min(-u).min(depth_cap).min(radius_cap);
            return fillet(p, q, k, a.out, b.out, t);
        }
    }
    if den.abs() < 0.02 && cos_beta < 0.0 && gap < std::f32::consts::PI {
        return Vec::new(); // in line: a straight kerb
    }
    // A curve tangent to both road edges, if it sweeps steadily round the centre.
    let chord = geom::dist(p, q);
    let k = chord * 0.4;
    let p3 = |v: V2| [v[0], v[1], 0.0];
    let pts: Vec<V2> = geom::bezier(
        p3(p),
        p3(geom::sub(p, geom::scale(a.out, k))),
        p3(geom::sub(q, geom::scale(b.out, k))),
        p3(q),
        BEND_STEPS,
    )
    .iter()
    .map(|v| geom::xy(*v))
    .collect();
    let a0 = angle_around(c, p);
    let mut last = 0.0f32;
    let steady = pts[1..].iter().all(|v| {
        let t = (angle_around(c, *v) - a0).rem_euclid(tau);
        let ok = t > last - 1e-4 && t <= gap + 1e-4;
        last = t;
        ok
    });
    if steady {
        return pts[1..pts.len() - 1].to_vec();
    }
    centre_arc(c, p, q, gap)
}

/// Interior points of an arc around `c` from `p` to `q` (counter-clockwise, `gap` radians),
/// its radius going from |p| to |q|, in steps of at most 7.5°.
fn centre_arc(c: V2, p: V2, q: V2, gap: f32) -> Vec<V2> {
    let (rp, rq) = (geom::dist(p, c), geom::dist(q, c));
    let a0 = angle_around(c, p);
    let steps = ((gap / (std::f32::consts::PI / 24.0)).ceil() as usize).max(2);
    (1..steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let r = rp + (rq - rp) * t;
            let (sn, cs) = wp_math::sin_cos(a0 + gap * t);
            [c[0] + cs * r, c[1] + sn * r]
        })
        .collect()
}

/// Distance along the ray `c + r·d` to segment `u`–`v`, if it hits it.
fn ray_hits(c: V2, d: V2, u: V2, v: V2) -> Option<f32> {
    let (u, v) = (geom::sub(u, c), geom::sub(v, c));
    let e = geom::sub(v, u);
    let den = geom::cross(d, e);
    if den.abs() < 1e-9 {
        return None;
    }
    let r = geom::cross(u, e) / den;
    let lam = geom::cross(u, d) / den;
    ((-1e-4..=1.0 + 1e-4).contains(&lam) && r > 0.0).then_some(r)
}

/// Largest kerb fillet radius, m.
pub const MAX_KERB_RADIUS_M: f32 = 8.0;

/// Angular step of a kerb pushed out by turning paths, rad (2°).
const HUG_STEP: f32 = std::f32::consts::PI / 90.0;

/// The kerb `curve` (interior points from `p` to `q`, counter-clockwise around `c`) pushed out
/// radially wherever a turning-path point in its sector lies beyond it, so every turning path
/// stays on the pavement ([`PATH_MARGIN_M`] inside its edges): in 2° steps, the kerb's
/// distance from the centre is the larger of its own and the farthest path point within one
/// step (+5 cm). Unchanged when no path crosses it.
fn hug_paths(c: V2, p: V2, q: V2, curve: Vec<V2>, paths: &[V2]) -> Vec<V2> {
    let tau = std::f32::consts::TAU;
    let a0 = angle_around(c, p);
    let gap = (angle_around(c, q) - a0).rem_euclid(tau);
    let mut full = vec![p];
    full.extend_from_slice(&curve);
    full.push(q);
    // Distance from c to the kerb along the ray at angle a0 + t.
    let kerb_r = |t: f32| -> f32 {
        let (sn, cs) = wp_math::sin_cos(a0 + t);
        let d = [cs, sn];
        let mut best = 0.0f32;
        for w in full.windows(2) {
            if let Some(r) = ray_hits(c, d, w[0], w[1]) {
                best = best.max(r);
            }
        }
        best
    };
    let inside: Vec<(f32, f32)> = paths
        .iter()
        .filter_map(|q| {
            let t = (angle_around(c, *q) - a0).rem_euclid(tau);
            (t > 1e-4 && t < gap - 1e-4).then(|| (t, geom::dist(*q, c)))
        })
        .collect();
    if inside.iter().all(|(t, r)| *r <= kerb_r(*t)) {
        return curve;
    }
    let steps = ((gap / HUG_STEP).ceil() as usize).max(2);
    let dt = gap / steps as f32;
    // Needed radius per step (farthest path point within 1.5 steps or 0.4 m of arc — path
    // samples are 0.5 m apart — + 5 cm), then the upper
    // envelope over ±1 step so the kerb does not zigzag between path samples.
    let need: Vec<f32> = (0..=steps)
        .map(|i| {
            let t = i as f32 * dt;
            inside
                .iter()
                .filter(|(tp, r)| (tp - t).abs() <= (dt * 1.5).max(0.4 / r.max(1.0)))
                .map(|(_, r)| r + 0.05)
                .fold(0.0f32, f32::max)
        })
        .collect();
    (1..steps)
        .map(|i| {
            let t = i as f32 * dt;
            let lo = i.saturating_sub(1);
            let hi = (i + 1).min(steps);
            let n = need[lo..=hi].iter().copied().fold(0.0f32, f32::max);
            let r = kerb_r(t).max(n);
            let (sn, cs) = wp_math::sin_cos(a0 + t);
            [c[0] + cs * r, c[1] + sn * r]
        })
        .collect()
}

/// A kerb from `p` along its kerb line towards the corner `k` (where it meets `q`'s), round
/// the corner on a circular arc tangent to both lines `t` from `k`, and on to `q`. `da` and
/// `db` point away from the junction along the two kerb lines.
fn fillet(p: V2, q: V2, k: V2, da: V2, db: V2, t: f32) -> Vec<V2> {
    let tau = std::f32::consts::TAU;
    let t1 = geom::add(k, geom::scale(da, t));
    let t2 = geom::add(k, geom::scale(db, t));
    // Interior angle β between the rays k→p and k→q; fillet radius t·tan(β/2).
    let beta = wp_math::atan2(geom::cross(da, db).abs(), geom::dot(da, db));
    let (sh, ch) = wp_math::sin_cos(beta * 0.5);
    let bis = geom::normalize(geom::add(da, db));
    let o = geom::add(k, geom::scale(bis, t / ch.max(1e-3)));
    let r = t * sh / ch.max(1e-3);
    let a0 = wp_math::atan2(t1[1] - o[1], t1[0] - o[0]);
    let a1 = wp_math::atan2(t2[1] - o[1], t2[0] - o[0]);
    // Go the short way from t1 to t2.
    let mut sweep = a1 - a0;
    while sweep > std::f32::consts::PI {
        sweep -= tau;
    }
    while sweep < -std::f32::consts::PI {
        sweep += tau;
    }
    let steps = ((sweep.abs() / (std::f32::consts::PI / 12.0)).ceil() as usize).max(2);
    let mut pts = Vec::with_capacity(steps + 3);
    if geom::dist(t1, p) > 0.05 {
        pts.push(t1);
    }
    for i in 1..steps {
        let ang = a0 + sweep * i as f32 / steps as f32;
        let (sn, cs) = wp_math::sin_cos(ang);
        pts.push([o[0] + cs * r, o[1] + sn * r]);
    }
    if geom::dist(t2, q) > 0.05 {
        pts.push(t2);
    }
    pts
}

/// Number of points along a bend side.
const BEND_STEPS: usize = 10;

/// Strip between two ribbon ends at a bend.
fn bend(e0: &End, e1: &End) -> Option<Patch> {
    // Travel e0 → node → e1: arrive along −e0.out, leave along e1.out. Travel-left at e0 is its
    // clockwise corner, at e1 its counter-clockwise one.
    let side = |p0: V3, p3: V3| -> Vec<V3> {
        let chord = geom::dist(geom::xy(p0), geom::xy(p3));
        let k = chord * 0.4;
        let p1 = [p0[0] - e0.out[0] * k, p0[1] - e0.out[1] * k, p0[2]];
        let p2 = [p3[0] - e1.out[0] * k, p3[1] - e1.out[1] * k, p3[2]];
        let mut pts = geom::bezier(p0, p1, p2, p3, BEND_STEPS);
        // Heights linear along the side.
        let cum = geom::cumulative(&pts);
        let total = cum[cum.len() - 1].max(1e-6);
        for (p, s) in pts.iter_mut().zip(&cum) {
            p[2] = p0[2] + (p3[2] - p0[2]) * s / total;
        }
        pts
    };
    let left = side(e0.cw, e1.ccw);
    let right = side(e0.ccw, e1.cw);
    let mut mesh = TriMesh::default();
    mesh.strip(&left, &right);
    if !faces_up(&mesh) {
        // A fold (very tight bend): triangulate the outline instead.
        let mut outline: Vec<V3> = right.clone();
        outline.extend(left.iter().rev());
        mesh = TriMesh::default();
        if !polygon_mesh(&mut mesh, &outline) {
            return None;
        }
    }
    // Rims counter-clockwise around the node: e0.ccw → e1.cw, e1.ccw → e0.cw.
    let rims = vec![right, left.into_iter().rev().collect()];
    Some((NodePatchKind::Bend, mesh, rims))
}

/// Every triangle of `m` faces up with a non-zero area.
fn faces_up(m: &TriMesh) -> bool {
    m.indices.iter().all(|t| {
        let [a, b, c] = t.map(|i| geom::xy(m.positions[i as usize]));
        geom::cross(geom::sub(b, a), geom::sub(c, a)) > 1e-6
    })
}

/// Append a simple polygon (either winding) triangulated by ear clipping, facing up; false
/// (nothing appended) when it cannot be triangulated.
pub(super) fn polygon_mesh(mesh: &mut TriMesh, poly: &[V3]) -> bool {
    let pts: Vec<V2> = poly.iter().map(|p| geom::xy(*p)).collect();
    let Some(tris) = ear_clip(&pts) else {
        return false;
    };
    let base = mesh.positions.len() as u32;
    mesh.positions.extend_from_slice(poly);
    for t in tris {
        mesh.indices
            .push([base + t[0] as u32, base + t[1] as u32, base + t[2] as u32]);
    }
    true
}

/// Ear-clipping triangulation of a simple polygon (either winding); triangles are
/// counter-clockwise. `None` when the polygon is degenerate or self-intersecting.
pub(super) fn ear_clip(pts: &[V2]) -> Option<Vec<[usize; 3]>> {
    let n = pts.len();
    if n < 3 {
        return None;
    }
    // Local coordinates: city positions are hundreds of metres out, where f32 shoelace sums
    // lose the centimetres that matter here.
    let o = pts[0];
    let local: Vec<V2> = pts.iter().map(|p| geom::sub(*p, o)).collect();
    let pts = &local[..];
    let area: f32 = (0..n)
        .map(|i| geom::cross(pts[i], pts[(i + 1) % n]))
        .sum::<f32>()
        * 0.5;
    if area.abs() < 1e-4 {
        return None;
    }
    let mut idx: Vec<usize> = if area > 0.0 {
        (0..n).collect()
    } else {
        (0..n).rev().collect()
    };
    let mut out = Vec::with_capacity(n - 2);
    let mut guard = 0;
    while idx.len() > 3 {
        guard += 1;
        if guard > 4 * n * n {
            return None;
        }
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let (ia, ib, ic) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let (a, b, c) = (pts[ia], pts[ib], pts[ic]);
            if geom::cross(geom::sub(b, a), geom::sub(c, b)) <= 1e-7 {
                continue; // reflex or straight
            }
            let inside = idx
                .iter()
                .any(|&j| j != ia && j != ib && j != ic && point_in_triangle(pts[j], a, b, c));
            if inside {
                continue;
            }
            out.push([ia, ib, ic]);
            idx.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            // Only straight vertices left (collinear runs): drop one.
            let straight = (0..idx.len()).find(|&i| {
                let m = idx.len();
                let (a, b, c) = (
                    pts[idx[(i + m - 1) % m]],
                    pts[idx[i]],
                    pts[idx[(i + 1) % m]],
                );
                geom::cross(geom::sub(b, a), geom::sub(c, b)).abs() <= 1e-7
            })?;
            idx.remove(straight);
        }
    }
    let (a, b, c) = (pts[idx[0]], pts[idx[1]], pts[idx[2]]);
    if geom::cross(geom::sub(b, a), geom::sub(c, b)) > 1e-7 {
        out.push([idx[0], idx[1], idx[2]]);
    }
    // Self-intersecting outlines clip into triangles whose areas no longer add up.
    let sum: f32 = out
        .iter()
        .map(|t| {
            geom::cross(
                geom::sub(pts[t[1]], pts[t[0]]),
                geom::sub(pts[t[2]], pts[t[0]]),
            ) * 0.5
        })
        .sum();
    if (sum - area.abs()).abs() > 1e-3 * area.abs().max(1.0) {
        return None;
    }
    Some(out)
}

/// `p` inside or on triangle `a b c` (counter-clockwise).
fn point_in_triangle(p: V2, a: V2, b: V2, c: V2) -> bool {
    geom::cross(geom::sub(b, a), geom::sub(p, a)) >= -1e-7
        && geom::cross(geom::sub(c, b), geom::sub(p, b)) >= -1e-7
        && geom::cross(geom::sub(a, c), geom::sub(p, c)) >= -1e-7
}

/// Points along a dead end's half-round.
const BULB_STEPS: usize = 12;

/// Dead end: the end edge closed by a straight run and a half-round ahead of it, holding every
/// U-turn path at the node.
fn dead_end(map: &CityMap, node: u32, c: V3, e: &End) -> Patch {
    let (a, b) = (geom::xy(e.cw), geom::xy(e.ccw));
    let fwd = geom::scale(e.out, -1.0);
    let m = geom::scale(geom::add(a, b), 0.5);
    let r = geom::dist(a, b) * 0.5;
    // Lateral axis from the cw corner to the ccw one.
    let lat_dir = geom::normalize(geom::sub(b, a));
    // Straight run so the U-turn paths (with their half widths) fit inside.
    let mut run = 0.0f32;
    for s in &map.segs {
        let SegKind::Connector { node: nd, .. } = s.kind else {
            continue;
        };
        if nd != node {
            continue;
        }
        let half = s.width * 0.5;
        for w in s.pts.windows(2) {
            let d = geom::normalize(geom::sub(geom::xy(w[1]), geom::xy(w[0])));
            let n = geom::right_of(d);
            for p in [w[0], w[1]] {
                for off in [-half, half] {
                    let q = geom::add(geom::xy(p), geom::scale(n, off));
                    let f = geom::dot(geom::sub(q, m), fwd);
                    let l = geom::dot(geom::sub(q, m), lat_dir).abs().min(r * 0.98);
                    run = run.max(f - (r * r - l * l).sqrt() + 0.3);
                }
            }
        }
    }
    let run = run.max(0.0);
    // Heights: across the end edge as the ribbon, constant forward.
    let z_at = |p: V2| {
        let t = (geom::dot(geom::sub(p, a), lat_dir) / (2.0 * r).max(1e-3)).clamp(0.0, 1.0);
        e.cw[2] + (e.ccw[2] - e.cw[2]) * t
    };
    let with_z = |p: V2| [p[0], p[1], z_at(p)];
    let _ = c;
    // Outline counter-clockwise: cw corner → (run) → half-round → (run back) → ccw corner.
    let mut rim: Vec<V3> = vec![e.ccw];
    let mc = geom::add(m, geom::scale(fwd, run));
    if run > 0.05 {
        rim.push(with_z(geom::add(b, geom::scale(fwd, run))));
    }
    // Half-round from the ccw side (angle of +lat) to the cw side through +fwd.
    for i in 1..BULB_STEPS {
        let t = i as f32 / BULB_STEPS as f32 * std::f32::consts::PI;
        let (sn, cs) = wp_math::sin_cos(t);
        let q = geom::add(
            mc,
            geom::add(geom::scale(lat_dir, cs * r), geom::scale(fwd, sn * r)),
        );
        rim.push(with_z(q));
    }
    if run > 0.05 {
        rim.push(with_z(geom::add(a, geom::scale(fwd, run))));
    }
    rim.push(e.cw);
    // The rim runs counter-clockwise around the turnaround (pavement on its left); closed by
    // the end edge (cw → ccw corner) it is the patch outline.
    let outline = rim.clone();
    let mut mesh = TriMesh::default();
    let mid = geom::add(m, geom::scale(fwd, (run + r) * 0.5));
    if !fan_around(&mut mesh, with_z(mid), &outline) {
        polygon_mesh(&mut mesh, &outline);
    }
    (NodePatchKind::DeadEnd, mesh, vec![rim])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ear_clip_handles_concave_and_rejects_bow_ties() {
        let l = [
            [0.0, 0.0],
            [2.0, 0.0],
            [2.0, 1.0],
            [1.0, 1.0],
            [1.0, 2.0],
            [0.0, 2.0],
        ];
        let t = ear_clip(&l).unwrap();
        assert_eq!(t.len(), 4);
        let cw: Vec<V2> = l.iter().rev().copied().collect();
        assert_eq!(ear_clip(&cw).unwrap().len(), 4);
        let pie = [
            [-294.0, -158.0],
            [-292.0, -158.0],
            [-292.0, -156.0],
            [-292.51764, -156.06815],
            [-293.0, -156.26794],
            [-293.4142, -156.58578],
            [-293.73206, -157.0],
            [-293.93185, -157.48236],
        ];
        assert!(ear_clip(&pie).is_some(), "pie");
        let skew = [
            [-1119.8691, -490.70355],
            [-1120.2799, -493.6753],
            [-1116.7711, -494.16028],
            [-1116.7754, -494.31412],
            [-1113.7766, -494.3967],
            [-1117.1897, -493.29166],
        ];
        assert!(ear_clip(&skew).is_some(), "skew");
        let bow = [[0.0, 0.0], [1.0, 1.0], [1.0, 0.0], [0.0, 1.0]];
        assert!(ear_clip(&bow).is_none());
    }
}
