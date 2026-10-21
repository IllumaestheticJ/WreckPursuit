//! The lines themselves: track pieces, bridges and viaducts, level crossings, corridor
//! fences, catenary, signals and the passing loops with parked freight trains.

use std::collections::BTreeMap;

use wp_math::Rng;

use super::super::ctx::{box_on, cylinder, obb, yaw_of, Ctx, Occ};
use super::super::ground::{inside, signed_dist};
use super::super::plan::{self, RailKind, SiteKind};
use super::align::{RailAlignment, SpanKind, Support, BALLAST_HALF_M, TRACK_SPACING_M};
use super::cars;
use super::geo::{self, piece, strip, Paving};
use crate::geom::{self, V2, V3};
use crate::network::{Feature, FeatureKind, Material};

/// Longest track piece, m.
pub const MAX_PIECE_M: f32 = 20.0;
/// Fences stand this far outside the outer ballast edge, m.
pub const FENCE_OUTSET_M: f32 = 3.5;
/// Fence height above the ground, m.
pub const FENCE_H_M: f32 = 2.0;
/// Roads whose surface is within this height of the rail bed cross at grade, m.
pub const LEVEL_DZ_M: f32 = 4.0;
/// Lateral offset of the passing-loop track beyond the outer main track, m.
const LOOP_SHIFT_M: f32 = TRACK_SPACING_M;

/// Everything the rail module produces, committed to the [`Ctx`] at the end.
#[derive(Default)]
pub struct Output {
    /// Features in placement order.
    pub features: Vec<Feature>,
    /// Corridor polygons marked `Occ::Feature` on commit (keeps buildings off the tracks).
    pub occupy: Vec<Vec<V2>>,
    /// Laid track pieces (2D), bucketed by 20 m cells of their midpoint.
    laid: BTreeMap<(i32, i32), Vec<(V2, V2)>>,
}

impl Output {
    /// Push a feature.
    pub fn add(&mut self, f: Feature) {
        self.features.push(f);
    }

    /// Record a laid track piece.
    fn record(&mut self, a: V2, b: V2) {
        let m = geom::scale(geom::add(a, b), 0.5);
        self.laid.entry(cell(m)).or_default().push((a, b));
    }

    /// Distance from `p` to the nearest laid track piece (searched up to ~30 m), m.
    pub fn track_distance(&self, p: V2) -> f32 {
        let (ci, cj) = cell(p);
        let mut best = f32::MAX;
        for dj in -2..=2 {
            for di in -2..=2 {
                if let Some(v) = self.laid.get(&(ci + di, cj + dj)) {
                    for (a, b) in v {
                        best = best.min(super::super::ground::seg_dist(*a, *b, p));
                    }
                }
            }
        }
        best
    }

    /// Hand everything to the context.
    pub fn commit(self, ctx: &mut Ctx) {
        for poly in &self.occupy {
            ctx.occupy(poly, 0.0, Occ::Feature);
        }
        for f in self.features {
            ctx.add(f);
        }
    }
}

fn cell(p: V2) -> (i32, i32) {
    ((p[0] / 20.0).floor() as i32, (p[1] / 20.0).floor() as i32)
}

/// A road crossing the rail line.
#[derive(Debug, Clone)]
pub struct Crossing {
    /// Index of the line.
    pub line: usize,
    /// Arc length of the crossing point, m.
    pub s: f32,
    /// Arc-length range of the line under the paving, m.
    pub s_lo: f32,
    /// See `s_lo`.
    pub s_hi: f32,
    /// Crossing point, m.
    pub p: V2,
    /// Unit direction of the road.
    pub road_dir: V2,
    /// Half paved width of the road, m.
    pub half_w: f32,
    /// Road surface height at the crossing, m.
    pub road_z: f32,
    /// At grade (gates, planks, fence gaps); otherwise grade separated.
    pub level: bool,
}

/// Every road (surface quad or junction box) crossing every line, merged per line.
pub fn find_crossings(lines: &[RailAlignment], paving: &Paving) -> Vec<Crossing> {
    let mut out = Vec::new();
    for (li, a) in lines.iter().enumerate() {
        let mut raw: Vec<Crossing> = Vec::new();
        for i in 0..a.pts.len() - 1 {
            let (p0, p1) = (geom::xy(a.pts[i]), geom::xy(a.pts[i + 1]));
            let seg = geom::dist(p0, p1).max(1e-3);
            let tdir = geom::normalize(geom::sub(p1, p0));
            for qi in paving.near(&[p0, p1], 2.0) {
                let q = &paving.quads[qi as usize];
                match q.centre {
                    Some((c0, c1)) => {
                        let Some((t, u)) = geo::seg_intersect(p0, p1, geom::xy(c0), geom::xy(c1))
                        else {
                            continue;
                        };
                        let rdir = geom::normalize(geom::sub(geom::xy(c1), geom::xy(c0)));
                        let sin = geom::cross(tdir, rdir).abs().max(0.3);
                        let half = q.half_w / sin;
                        let s = a.cum[i] + t * seg;
                        let road_z = c0[2] + (c1[2] - c0[2]) * u;
                        let rail_z = a.pts[i][2] + (a.pts[i + 1][2] - a.pts[i][2]) * t;
                        raw.push(Crossing {
                            line: li,
                            s,
                            s_lo: s - half,
                            s_hi: s + half,
                            p: geo::along(p0, tdir, t * seg),
                            road_dir: rdir,
                            half_w: q.half_w,
                            road_z,
                            level: (road_z - rail_z).abs() <= LEVEL_DZ_M,
                        });
                    }
                    None => {
                        // Junction box: a disc of radius `half_w` around its centroid.
                        let c = centroid(&q.poly);
                        let r = q.half_w;
                        let t = (geom::dot(geom::sub(c, p0), tdir) / seg).clamp(0.0, 1.0);
                        let foot = geo::along(p0, tdir, t * seg);
                        let d = geom::dist(c, foot);
                        if d >= r || (t <= 0.0 && i > 0) || (t >= 1.0 && i + 2 < a.pts.len()) {
                            continue;
                        }
                        let half = (r * r - d * d).sqrt();
                        let s = a.cum[i] + t * seg;
                        let road_z = (q.z_lo + q.z_hi) * 0.5;
                        let rail_z = a.pts[i][2] + (a.pts[i + 1][2] - a.pts[i][2]) * t;
                        raw.push(Crossing {
                            line: li,
                            s,
                            s_lo: s - half,
                            s_hi: s + half,
                            p: foot,
                            road_dir: geom::right_of(tdir),
                            half_w: r,
                            road_z,
                            level: (road_z - rail_z).abs() <= LEVEL_DZ_M,
                        });
                    }
                }
            }
        }
        raw.sort_by(|x, y| x.s.total_cmp(&y.s));
        let mut merged: Vec<Crossing> = Vec::new();
        for c in raw {
            if let Some(m) = merged.last_mut() {
                if c.level == m.level && c.s_lo <= m.s_hi + 1.0 {
                    if c.half_w > m.half_w {
                        m.road_dir = c.road_dir;
                        m.half_w = c.half_w;
                        m.p = c.p;
                        m.s = c.s;
                        m.road_z = c.road_z;
                    }
                    m.s_lo = m.s_lo.min(c.s_lo);
                    m.s_hi = m.s_hi.max(c.s_hi);
                    continue;
                }
            }
            merged.push(c);
        }
        out.extend(merged);
    }
    out
}

fn centroid(poly: &[V2]) -> V2 {
    let n = poly.len() as f32;
    let s = poly.iter().fold([0.0, 0.0], |a, p| geom::add(a, *p));
    [s[0] / n, s[1] / n]
}

/// Points of a line offset `off` to the right (z = formation).
pub fn offset_line(a: &RailAlignment, off: f32) -> Vec<V3> {
    (0..a.pts.len())
        .map(|i| {
            let n = a.normal(i);
            [
                a.pts[i][0] + n[0] * off,
                a.pts[i][1] + n[1] * off,
                a.pts[i][2],
            ]
        })
        .collect()
}

/// Lay one track along `line` (z = formation) over the samples where `active` is true:
/// pieces of up to [`MAX_PIECE_M`] that follow curves (sagitta < 0.12 m), each a ballast
/// bed and two rails. Pieces lying on an already-laid track are skipped.
pub fn lay_track(out: &mut Output, line: &[V3], active: &[bool]) {
    let n = line.len();
    let per = (MAX_PIECE_M / super::align::SAMPLE_M).round() as usize;
    let mut i = 0;
    while i + 1 < n {
        if !active[i] || !active[i + 1] {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j + 1 < n && active[j + 1] && j + 1 - i <= per && sagitta(line, i, j + 1) < 0.12 {
            j += 1;
        }
        let (a, b) = (line[i], line[j]);
        let (a2, b2) = (geom::xy(a), geom::xy(b));
        let mid = geom::scale(geom::add(a2, b2), 0.5);
        if out.track_distance(mid) > 1.0 {
            track_piece(out, a, b);
            out.record(a2, b2);
        }
        i = j;
    }
}

/// Ballast bed + two rails from `a` to `b` (z = formation).
pub fn track_piece(out: &mut Output, a: V3, b: V3) {
    out.add(piece(
        FeatureKind::RailTrack,
        Material::Gravel,
        a,
        b,
        BALLAST_HALF_M * 2.0,
        -0.25,
        0.30,
        0.15,
        false,
    ));
    let d = geom::normalize(geom::sub(geom::xy(b), geom::xy(a)));
    let n = geom::right_of(d);
    for lat in [-0.7175f32, 0.7175] {
        let off = |p: V3| [p[0] + n[0] * lat, p[1] + n[1] * lat, p[2]];
        out.add(piece(
            FeatureKind::RailTrack,
            Material::Steel,
            off(a),
            off(b),
            0.08,
            0.30,
            cars::RAIL_TOP_M,
            0.15,
            false,
        ));
    }
}

fn sagitta(line: &[V3], i: usize, j: usize) -> f32 {
    let (a, b) = (geom::xy(line[i]), geom::xy(line[j]));
    (i + 1..j)
        .map(|k| super::super::ground::seg_dist(a, b, geom::xy(line[k])))
        .fold(0.0, f32::max)
}

/// A solid buffer stop at `p` facing along `dir` (z = formation), moved back along the
/// track (up to 20 m) when `p` is on road paving; none if it cannot be.
pub fn buffer_stop(out: &mut Output, paving: &Paving, p: V3, dir: V2) {
    // Step back along the track off any paving the line ends at (a quay road, a street).
    for back in 0..=20 {
        let c = geo::along([p[0], p[1]], dir, -(back as f32));
        let fp = obb(c, [1.5, 0.6], geom::yaw_of(dir));
        if paving.hits(&fp, 0.3, p[2] - 1.0, p[2] + 2.0) {
            continue;
        }
        out.add(box_on(
            FeatureKind::RailFurniture,
            Material::Red,
            c,
            p[2],
            [3.0, 1.2, 1.4],
            geom::yaw_of(dir),
            true,
        ));
        return;
    }
}

/// Zone and road test for corridor fences: built-up where a street is within 40 m or the
/// district is denser than the outskirts.
pub fn built_up(ctx: &Ctx, p: V2) -> bool {
    ctx.plan
        .zones
        .iter()
        .any(|z| z.zone.density_base >= 6.0 && inside(&z.zone.polygon, p))
        || ctx.road_distance(p, 40.0) < 40.0
}

/// Solid fence from `a` to `b` in pieces of at most 10 m. A piece is placed when `ok`
/// accepts it (`a`, `b`, and the parameters `ta`, `tb` interpolated along the run), it is
/// not on paving, not in water and not on another module's building / feature; a
/// rejected piece longer than 3 m is retried as four shorter ones (tight gaps at gates).
#[allow(clippy::too_many_arguments)]
pub fn fence_run(
    ctx: &Ctx,
    paving: &Paving,
    out: &mut Output,
    a: V2,
    b: V2,
    ta: f32,
    tb: f32,
    ok: &dyn Fn(V2, V2, f32, f32) -> bool,
) {
    let len = geom::dist(a, b);
    if len < 0.3 {
        return;
    }
    let n = (len / 10.0).ceil().max(1.0) as usize;
    for k in 0..n {
        let (u0, u1) = (k as f32 / n as f32, (k + 1) as f32 / n as f32);
        let p = super::super::ctx::lerp2(a, b, u0);
        let q = super::super::ctx::lerp2(a, b, u1);
        let (t0, t1) = (ta + (tb - ta) * u0, ta + (tb - ta) * u1);
        if fence_piece(ctx, paving, out, p, q, t0, t1, ok) {
            continue;
        }
        if geom::dist(p, q) > 3.0 {
            for m in 0..4 {
                let (v0, v1) = (m as f32 / 4.0, (m + 1) as f32 / 4.0);
                fence_piece(
                    ctx,
                    paving,
                    out,
                    super::super::ctx::lerp2(p, q, v0),
                    super::super::ctx::lerp2(p, q, v1),
                    t0 + (t1 - t0) * v0,
                    t0 + (t1 - t0) * v1,
                    ok,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fence_piece(
    ctx: &Ctx,
    paving: &Paving,
    out: &mut Output,
    a: V2,
    b: V2,
    ta: f32,
    tb: f32,
    ok: &dyn Fn(V2, V2, f32, f32) -> bool,
) -> bool {
    if !ok(a, b, ta, tb) {
        return false;
    }
    let m = super::super::ctx::lerp2(a, b, 0.5);
    let za = [ctx.ground_z(a), ctx.ground_z(b), ctx.ground_z(m)];
    let lo = za.iter().copied().fold(f32::MAX, f32::min);
    let hi = za.iter().copied().fold(f32::MIN, f32::max);
    if lo < plan::WATER_Z + 0.1 {
        return false;
    }
    let fp = strip(a, b, 0.1);
    if paving.hits(&fp, 0.6, lo - 1.0, hi + FENCE_H_M) {
        return false;
    }
    if !ctx.free_of(&fp, 0.0, &[Occ::Building, Occ::Feature]) {
        return false;
    }
    out.add(box_on(
        FeatureKind::Fence,
        Material::Steel,
        m,
        lo - 0.3,
        [0.12, geom::dist(a, b) + 0.12, hi + FENCE_H_M - lo + 0.3],
        yaw_of(a, b),
        true,
    ));
    true
}

/// A passing loop: a third track beside the main line between samples `i0..=i1`.
struct Loop {
    i0: usize,
    i1: usize,
    side: f32,
}

/// Build every line (tracks, decks, bridges, crossings, fences, furniture, loops).
pub fn build_lines(
    ctx: &Ctx,
    lines: &[RailAlignment],
    crossings: &[Crossing],
    paving: &Paving,
    rng: &mut Rng,
    out: &mut Output,
) {
    for (li, a) in lines.iter().enumerate() {
        let xs: Vec<&Crossing> = crossings.iter().filter(|c| c.line == li).collect();
        let n = a.pts.len();
        let in_level = |s: f32, pad: f32| {
            xs.iter()
                .any(|c| c.level && s >= c.s_lo - pad && s <= c.s_hi + pad)
        };
        let in_any = |s: f32, pad: f32| xs.iter().any(|c| s >= c.s_lo - pad && s <= c.s_hi + pad);

        // Where the line's own tracks run: up to a station throat, and not into a foreign
        // site where an earlier module already built something on the alignment.
        let mut last = n - 1;
        for i in 0..n {
            if a.support[i] == Support::StationDeck {
                last = i;
                break;
            }
            let p = geom::xy(a.pts[i]);
            let foreign = ctx.plan.sites.iter().any(|s| {
                !matches!(s.kind, SiteKind::RailYard | SiteKind::Station) && inside(&s.polygon, p)
            });
            if foreign
                && !ctx.free_of(
                    &strip(p, geo::along(p, [1.0, 0.0], 0.1), a.half_span_m()),
                    0.0,
                    &[Occ::Building, Occ::Feature],
                )
            {
                last = i.saturating_sub(4);
                break;
            }
        }
        let active: Vec<bool> = (0..n)
            .map(|i| i <= last && !in_level(a.cum[i], 0.5))
            .collect();
        for off in a.track_offsets() {
            let line = offset_line(a, off);
            lay_track(out, &line, &active);
            // Buffer stop where a line ends short of the map edge (not at a station).
            if last == n - 1 && a.support[last] != Support::StationDeck {
                let p = line[last];
                let at_edge = p[0] < plan::MAP_LO[0] + 10.0
                    || p[0] > plan::MAP_HI[0] - 10.0
                    || p[1] < plan::MAP_LO[1] + 10.0
                    || p[1] > plan::MAP_HI[1] - 10.0;
                let in_yard = ctx
                    .plan
                    .sites_of(SiteKind::RailYard)
                    .any(|s| signed_dist(&s.polygon, geom::xy(p)) < 5.0);
                if !at_edge && !in_yard {
                    let d =
                        geom::normalize(geom::sub(geom::xy(line[last]), geom::xy(line[last - 1])));
                    buffer_stop(out, paving, line[last], d);
                }
            } else if last < n - 1 && a.support[last] != Support::StationDeck {
                let d = geom::normalize(geom::sub(geom::xy(line[last]), geom::xy(line[last - 1])));
                buffer_stop(out, paving, line[last], d);
            }
            if a.support[0] != Support::StationDeck {
                let first_in_yard = ctx
                    .plan
                    .sites_of(SiteKind::RailYard)
                    .any(|s| signed_dist(&s.polygon, geom::xy(line[0])) < 5.0);
                let on_other = lines.iter().enumerate().any(|(lj, b)| {
                    lj != li && b.project(geom::xy(line[0])).dist < b.half_span_m() + 3.0
                });
                if !first_in_yard && !on_other {
                    let d = geom::normalize(geom::sub(geom::xy(line[0]), geom::xy(line[1])));
                    buffer_stop(out, paving, line[0], d);
                }
            }
        }

        // Crossing planks (flush with the road) and gates.
        for c in xs.iter().filter(|c| c.level) {
            let i_a = (0..n).rev().find(|&i| a.cum[i] < c.s_lo - 0.5).unwrap_or(0);
            let i_b = (0..n).find(|&i| a.cum[i] > c.s_hi + 0.5).unwrap_or(n - 1);
            if i_a > last {
                continue;
            }
            let (pa, pb) = (a.pts[i_a], a.pts[i_b.min(last)]);
            let w = a.half_span_m() * 2.0;
            out.add(piece(
                FeatureKind::RailTrack,
                Material::Wood,
                [pa[0], pa[1], c.road_z],
                [pb[0], pb[1], c.road_z],
                w,
                -0.12,
                0.03,
                0.0,
                false,
            ));
            crossing_gates(ctx, paving, out, a, c);
        }

        decks(ctx, paving, out, a);

        // Passing loops with parked freight trains.
        let loops = if a.kind == RailKind::Freight && a.tracks >= 2 {
            find_loops(ctx, paving, lines, li, &xs)
        } else {
            Vec::new()
        };
        let base_off = a.half_span_m() + FENCE_OUTSET_M;
        let mut extra = [vec![0.0f32; n], vec![0.0f32; n]];
        for lp in &loops {
            let outer = a.track_offsets().last().copied().unwrap_or(0.0);
            let line: Vec<V3> = (lp.i0..=lp.i1)
                .map(|i| {
                    let t = (a.cum[i] - a.cum[lp.i0]).min(a.cum[lp.i1] - a.cum[i]) / 45.0;
                    let t = t.clamp(0.0, 1.0);
                    let t = t * t * (3.0 - 2.0 * t);
                    let o = lp.side * (outer + LOOP_SHIFT_M * t);
                    let nn = a.normal(i);
                    [
                        a.pts[i][0] + nn[0] * o,
                        a.pts[i][1] + nn[1] * o,
                        a.pts[i][2],
                    ]
                })
                .collect();
            lay_track(out, &line, &vec![true; line.len()]);
            let cum = geom::cumulative(&line);
            let len = *cum.last().unwrap_or(&0.0);
            let consist = cars::freight_consist(rng, len - 130.0, true);
            let start = (len - cars::consist_length(&consist)) * 0.5;
            cars::park(&line, &cum, start, &consist, rng, &mut out.features);
            let side = if lp.side > 0.0 { 1 } else { 0 };
            for e in extra[side]
                .iter_mut()
                .take((lp.i1 + 3).min(n))
                .skip(lp.i0.saturating_sub(3))
            {
                *e = LOOP_SHIFT_M;
            }
        }

        // Corridor fences.
        for (side, sign) in [(0usize, -1.0f32), (1, 1.0)] {
            let pts: Vec<V2> = (0..n)
                .map(|i| {
                    let nn = a.normal(i);
                    geo::along(geom::xy(a.pts[i]), nn, sign * (base_off + extra[side][i]))
                })
                .collect();
            let ok = |p: V2, q: V2, s0: f32, s1: f32| -> bool {
                let (lo, hi) = (s0.min(s1), s0.max(s1));
                let (i0, i1) = (a.index_at(lo), a.index_at(hi));
                if (i0..=i1).any(|i| !a.support[i].on_ground()) {
                    return false;
                }
                if in_any(lo, 3.0) || in_any(hi, 3.0) {
                    return false;
                }
                let m = super::super::ctx::lerp2(p, q, 0.5);
                if !built_up(ctx, m) {
                    return false;
                }
                if ctx
                    .plan
                    .sites
                    .iter()
                    .any(|s| signed_dist(&s.polygon, m) < 2.0)
                {
                    return false;
                }
                !lines.iter().enumerate().any(|(lj, b)| {
                    lj != li && b.project(m).dist < b.half_span_m() + FENCE_OUTSET_M + 1.0
                })
            };
            for i in (0..n - 1).step_by(2) {
                let j = (i + 2).min(n - 1);
                fence_run(ctx, paving, out, pts[i], pts[j], a.cum[i], a.cum[j], &ok);
            }
        }

        if a.kind == RailKind::Passenger {
            catenary(ctx, paving, out, a, last, &in_any);
        }
        signals(ctx, paving, out, a, last, &in_any);

        // Keep the corridor clear of later modules' buildings.
        for i in (0..n.saturating_sub(2)).step_by(2) {
            if a.natural[i] < plan::WATER_Z || a.support[i] == Support::StationDeck {
                continue;
            }
            let half = base_off + extra[0][i].max(extra[1][i]) + 0.5;
            out.occupy
                .push(strip(geom::xy(a.pts[i]), geom::xy(a.pts[i + 2]), half));
        }
    }
}

/// Gates, crossbucks and lights at a level crossing: on each approach, a post on the
/// right-hand verge with its barrier arm raised, a warning sign and lights.
fn crossing_gates(ctx: &Ctx, paving: &Paving, out: &mut Output, a: &RailAlignment, c: &Crossing) {
    let (_, tdir) = a.at(c.s);
    let sin = geom::cross(tdir, c.road_dir).abs().max(0.3);
    let d_out = a.half_span_m() / sin + 2.5;
    for side in [-1.0f32, 1.0] {
        // Traffic approaching the track from this side heads along -side * road_dir.
        let travel = geom::scale(c.road_dir, -side);
        let right = geom::right_of(travel);
        let base = geo::along(c.p, c.road_dir, side * d_out);
        for push in [1.2f32, 2.2, 3.2, 4.5] {
            let post = geo::along(base, right, c.half_w + push);
            let fp = obb(post, [0.3, 0.3], 0.0);
            let z = ctx.ground_z(post);
            if paving.hits(&fp, 0.2, z - 1.0, z + 4.0) || ctx.is_water(post) {
                continue;
            }
            let yaw = geom::yaw_of(travel);
            // Post with the barrier arm raised (vertical, non-solid).
            out.add(box_on(
                FeatureKind::RailFurniture,
                Material::SafetyYellow,
                post,
                z,
                [0.4, 0.4, 1.3],
                yaw,
                true,
            ));
            out.add(box_on(
                FeatureKind::RailFurniture,
                Material::PaintWhite,
                geo::along(post, travel, 0.3),
                z + 1.0,
                [0.12, 0.12, c.half_w + 0.8],
                yaw,
                false,
            ));
            // Sign pole beside it (further from the track), crossbuck and lights.
            let pole = geo::along(post, travel, -1.2);
            out.add(cylinder(
                FeatureKind::RailFurniture,
                Material::Steel,
                [pole[0], pole[1], z],
                0.08,
                3.4,
                true,
            ));
            let face = geom::yaw_of(right);
            for pitch in [0.785f32, -0.785] {
                out.add(Feature {
                    kind: FeatureKind::RailFurniture,
                    material: Material::PaintWhite,
                    shape: crate::network::FeatureShape::Box {
                        centre: [pole[0], pole[1], z + 3.0],
                        half: [0.03, 0.6, 0.1],
                        yaw: face,
                        pitch,
                    },
                    solid: false,
                });
            }
            out.add(box_on(
                FeatureKind::RailFurniture,
                Material::Light,
                pole,
                z + 2.1,
                [0.25, 0.8, 0.3],
                face,
                false,
            ));
            break;
        }
    }
}

/// Decks, parapets and piers of the elevated parts: viaducts, the swing bridge, girder
/// overbridges (station decks are built by the station).
fn decks(ctx: &Ctx, paving: &Paving, out: &mut Output, a: &RailAlignment) {
    let n = a.pts.len();
    let deck_half = a.half_span_m() + 0.7;
    let elevated = |i: usize| matches!(a.support[i], Support::Viaduct | Support::Bridge);
    let span_of = |s: f32| {
        a.spans
            .iter()
            .find(|sp| s >= sp.s0 - 0.1 && s <= sp.s1 + 0.1)
    };
    // Deck and parapet pieces.
    let mut i = 0;
    while i + 1 < n {
        if !elevated(i) && !elevated(i + 1) {
            i += 1;
            continue;
        }
        let j = (i + 4).min(n - 1);
        let (pa, pb) = (a.pts[i], a.pts[j]);
        let span = span_of(a.cum[(i + j) / 2]);
        let (mat, thick) = match span.map(|s| s.kind) {
            Some(SpanKind::Swing) => (Material::Steel, 1.5),
            Some(SpanKind::Girder) => (Material::Steel, 1.2),
            None => (Material::Concrete, 1.2),
        };
        let mut d = piece(
            FeatureKind::RailBridge,
            mat,
            pa,
            pb,
            deck_half * 2.0,
            -thick,
            0.0,
            0.1,
            true,
        );
        d.material = mat;
        out.add(d);
        let dir = geom::normalize(geom::sub(geom::xy(pb), geom::xy(pa)));
        let nn = geom::right_of(dir);
        let girder = span.is_some_and(|s| s.kind == SpanKind::Girder);
        for side in [-1.0f32, 1.0] {
            let lat = side * (deck_half - 0.25);
            let off = |p: V3| [p[0] + nn[0] * lat, p[1] + nn[1] * lat, p[2]];
            let (z0, z1, w) = if girder {
                (-thick - 0.6, 1.8, 0.5)
            } else {
                (0.0, 1.1, 0.4)
            };
            out.add(piece(
                FeatureKind::RailBridge,
                if girder {
                    Material::Steel
                } else {
                    Material::Concrete
                },
                off(pa),
                off(pb),
                w,
                z0,
                z1,
                0.1,
                true,
            ));
        }
        i = j;
    }
    // Piers every 25 m under viaducts and approach spans, never on paving.
    for i in (0..n).step_by(5) {
        if !elevated(i) {
            continue;
        }
        let s = a.cum[i];
        let sp = span_of(s);
        if let Some(sp) = sp {
            if sp.kind == SpanKind::Swing {
                if let Some((w0, w1)) = sp.water {
                    let mid = (w0 + w1) * 0.5;
                    if (s - mid).abs() < 70.0 {
                        continue; // swing span: pivot + rest piers below
                    }
                }
            }
        }
        pier(ctx, paving, out, a, i, deck_half);
    }
    for sp in &a.spans {
        if sp.kind == SpanKind::Swing {
            swing_span(ctx, out, a, sp, deck_half);
        }
        // Abutments at both ends (solid, top at the formation: the bed runs onto them).
        for (s, dir) in [(sp.s0, 1.0f32), (sp.s1, -1.0)] {
            let (p, t) = a.at(s + dir * 4.0);
            let g = ctx.ground_z(geom::xy(p)).min(p[2] - 1.0);
            out.add(box_on(
                FeatureKind::RailBridge,
                Material::DarkConcrete,
                geom::xy(p),
                g - 0.5,
                [deck_half * 2.0 + 1.0, 8.0, p[2] - g + 0.5],
                geom::yaw_of(t),
                true,
            ));
        }
    }
}

/// A pier under sample `i`: one or two columns from the ground to the deck soffit.
fn pier(ctx: &Ctx, paving: &Paving, out: &mut Output, a: &RailAlignment, i: usize, deck_half: f32) {
    let p = a.pts[i];
    let nn = a.normal(i);
    let soffit = p[2] - 1.5;
    let lats: Vec<f32> = if a.tracks >= 2 {
        vec![-deck_half * 0.55, deck_half * 0.55]
    } else {
        vec![0.0]
    };
    for lat in lats {
        let c = geo::along(geom::xy(p), nn, lat);
        let g = ctx.ground_z(c);
        if soffit - g < 1.0 {
            continue;
        }
        let fp = obb(c, [1.2, 1.2], 0.0);
        if paving.hits_any(&fp, 1.0) {
            continue;
        }
        out.add(cylinder(
            FeatureKind::RailBridge,
            Material::Concrete,
            [c[0], c[1], g.min(plan::WATER_Z) - 0.5],
            0.9,
            soffit - g.min(plan::WATER_Z) + 0.5,
            true,
        ));
    }
}

/// Swing span over the river: camelback trusses, a pivot pier and two rest piers.
fn swing_span(
    ctx: &Ctx,
    out: &mut Output,
    a: &RailAlignment,
    sp: &super::align::BridgeSpan,
    deck_half: f32,
) {
    let Some((w0, w1)) = sp.water else { return };
    let mid = (w0 + w1) * 0.5;
    let half = 60.0f32;
    let panels = 12;
    let height = |s: f32| 7.0 + 3.5 * (1.0 - ((s - mid).abs() / half).min(1.0));
    let (pc, tc) = a.at(mid);
    let deck_z = pc[2];
    // Pivot pier (drum) and rest piers.
    let bed = ctx.ground_z(geom::xy(pc)).min(plan::WATER_Z) - 0.5;
    out.add(cylinder(
        FeatureKind::RailBridge,
        Material::DarkConcrete,
        [pc[0], pc[1], bed],
        7.0,
        deck_z - 1.5 - bed,
        true,
    ));
    for s in [mid - half, mid + half] {
        let (p, t) = a.at(s);
        let g = ctx.ground_z(geom::xy(p)).min(plan::WATER_Z) - 0.5;
        out.add(box_on(
            FeatureKind::RailBridge,
            Material::DarkConcrete,
            geom::xy(p),
            g,
            [deck_half * 2.0 + 2.0, 4.0, p[2] - 1.5 - g],
            geom::yaw_of(t),
            true,
        ));
    }
    // Trusses.
    let nn = geom::right_of(tc);
    for side in [-1.0f32, 1.0] {
        let lat = side * (deck_half - 0.3);
        let at = |s: f32, up: f32| {
            let (p, _) = a.at(s);
            let q = geo::along(geom::xy(p), nn, lat);
            [q[0], q[1], p[2] + up]
        };
        for k in 0..=panels {
            let s = mid - half + 2.0 * half * k as f32 / panels as f32;
            let h = height(s);
            // Vertical (solid).
            let b = at(s, 0.0);
            out.add(box_on(
                FeatureKind::RailBridge,
                Material::Steel,
                geom::xy(b),
                b[2],
                [0.45, 0.45, h],
                geom::yaw_of(tc),
                true,
            ));
            if k < panels {
                let s2 = s + 2.0 * half / panels as f32;
                let h2 = height(s2);
                // Top chord (solid) and a diagonal leaning towards the pivot (non-solid).
                out.add(piece(
                    FeatureKind::RailBridge,
                    Material::Steel,
                    at(s, h),
                    at(s2, h2),
                    0.6,
                    -0.3,
                    0.3,
                    0.2,
                    true,
                ));
                let (lo, hi) = if s < mid {
                    (at(s, 0.3), at(s2, h2))
                } else {
                    (at(s2, 0.3), at(s, h))
                };
                out.add(piece(
                    FeatureKind::RailBridge,
                    Material::Steel,
                    lo,
                    hi,
                    0.3,
                    -0.15,
                    0.15,
                    0.0,
                    false,
                ));
            }
        }
    }
    // Top bracing at every other panel, and the operator's cabin over the pivot.
    for k in (0..=panels).step_by(2) {
        let s = mid - half + 2.0 * half * k as f32 / panels as f32;
        let (p, t) = a.at(s);
        out.add(box_on(
            FeatureKind::RailBridge,
            Material::Steel,
            geom::xy(p),
            p[2] + height(s) - 0.3,
            [deck_half * 2.0 - 0.6, 0.4, 0.4],
            geom::yaw_of(t),
            false,
        ));
    }
    out.add(box_on(
        FeatureKind::RailBridge,
        Material::Rust,
        geom::xy(pc),
        deck_z + height(mid) + 0.3,
        [5.0, 6.0, 3.0],
        geom::yaw_of(tc),
        true,
    ));
}

/// Pick up to two stretches for passing loops: on graded ground, no crossings or sites
/// near, away from other lines, with room on one side.
fn find_loops(
    ctx: &Ctx,
    paving: &Paving,
    lines: &[RailAlignment],
    li: usize,
    xs: &[&Crossing],
) -> Vec<Loop> {
    let a = &lines[li];
    let n = a.pts.len();
    let good: Vec<bool> = (0..n)
        .map(|i| {
            let p = geom::xy(a.pts[i]);
            let s = a.cum[i];
            a.support[i] == Support::Grade
                && !xs.iter().any(|c| s > c.s_lo - 60.0 && s < c.s_hi + 60.0)
                && !ctx
                    .plan
                    .sites
                    .iter()
                    .any(|st| signed_dist(&st.polygon, p) < 40.0)
                && !lines
                    .iter()
                    .enumerate()
                    .any(|(lj, b)| lj != li && b.project(p).dist < 40.0)
                && !a
                    .spans
                    .iter()
                    .any(|sp| s > sp.s0 - 80.0 && s < sp.s1 + 80.0)
        })
        .collect();
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < n {
        if !good[i] {
            i += 1;
            continue;
        }
        let j0 = i;
        while i < n && good[i] {
            i += 1;
        }
        if a.cum[i - 1] - a.cum[j0] >= 450.0 {
            runs.push((j0, i - 1));
        }
    }
    runs.sort_by(|x, y| {
        let lx = a.cum[x.1] - a.cum[x.0];
        let ly = a.cum[y.1] - a.cum[y.0];
        ly.total_cmp(&lx).then(x.0.cmp(&y.0))
    });
    let mut out = Vec::new();
    for (r0, r1) in runs {
        if out.len() >= 2 {
            break;
        }
        let len = (a.cum[r1] - a.cum[r0] - 60.0).min(750.0);
        let mid = (a.cum[r0] + a.cum[r1]) * 0.5;
        let (i0, i1) = (a.index_at(mid - len * 0.5), a.index_at(mid + len * 0.5));
        let outer = a.track_offsets().last().copied().unwrap_or(0.0);
        for side in [1.0f32, -1.0] {
            let o = side * (outer + LOOP_SHIFT_M);
            let free = (i0..i1).step_by(2).all(|i| {
                let j = (i + 2).min(i1);
                let p = geo::along(geom::xy(a.pts[i]), a.normal(i), o);
                let q = geo::along(geom::xy(a.pts[j]), a.normal(j), o);
                let fp = strip(p, q, BALLAST_HALF_M + 2.0);
                ctx.free_of(&fp, 1.0, &[Occ::Building, Occ::Feature, Occ::Water])
                    && !paving.hits_any(&fp, 2.0)
            });
            if free {
                out.push(Loop { i0, i1, side });
                break;
            }
        }
    }
    out
}

/// Overhead line: masts both sides every 50 m with cantilever arms, contact wires over
/// every track (passenger lines).
fn catenary(
    ctx: &Ctx,
    paving: &Paving,
    out: &mut Output,
    a: &RailAlignment,
    last: usize,
    in_any: &dyn Fn(f32, f32) -> bool,
) {
    let end = a.cum[last];
    let mut s = 10.0;
    let lat = a.half_span_m() + 1.0;
    while s < end - 5.0 {
        let i = a.index_at(s);
        let p = a.pts[i];
        let nn = a.normal(i);
        let (_, t) = a.at(s);
        if !in_any(s, 4.0) {
            for side in [-1.0f32, 1.0] {
                let m = geo::along(geom::xy(p), nn, side * lat);
                let base = if a.support[i].on_ground() {
                    ctx.ground_z(m).min(p[2]) - 0.3
                } else {
                    p[2]
                };
                let fp = obb(m, [0.3, 0.3], 0.0);
                if paving.hits(&fp, 0.3, base - 1.0, p[2] + 8.0) {
                    continue;
                }
                out.add(cylinder(
                    FeatureKind::RailFurniture,
                    Material::Steel,
                    [m[0], m[1], base],
                    0.18,
                    p[2] + 7.4 - base,
                    true,
                ));
                // Cantilever arm towards the centreline.
                let c = geo::along(geom::xy(p), nn, side * lat * 0.5);
                out.add(box_on(
                    FeatureKind::RailFurniture,
                    Material::Steel,
                    c,
                    p[2] + 6.6,
                    [lat, 0.12, 0.12],
                    geom::yaw_of(t),
                    false,
                ));
            }
        }
        s += 50.0;
    }
    // Contact wires, 25 m pieces.
    for off in a.track_offsets() {
        let line = offset_line(a, off);
        let mut i = 0;
        while i < last {
            let j = (i + 5).min(last);
            out.add(piece(
                FeatureKind::RailFurniture,
                Material::Black,
                line[i],
                line[j],
                0.04,
                5.55,
                5.6,
                0.1,
                false,
            ));
            i = j;
        }
    }
}

/// Block signals every ~700 m on the right of the line.
fn signals(
    ctx: &Ctx,
    paving: &Paving,
    out: &mut Output,
    a: &RailAlignment,
    last: usize,
    in_any: &dyn Fn(f32, f32) -> bool,
) {
    let end = a.cum[last];
    let mut s = 150.0;
    while s < end - 50.0 {
        let i = a.index_at(s);
        if a.support[i].on_ground() && !in_any(s, 30.0) {
            let p = a.pts[i];
            let m = geo::along(geom::xy(p), a.normal(i), a.half_span_m() + 1.4);
            let z = ctx.ground_z(m).min(p[2]);
            let fp = obb(m, [0.3, 0.3], 0.0);
            if !paving.hits(&fp, 0.5, z - 1.0, z + 6.0) {
                out.add(cylinder(
                    FeatureKind::RailFurniture,
                    Material::Steel,
                    [m[0], m[1], z - 0.2],
                    0.12,
                    5.2,
                    true,
                ));
                let (_, t) = a.at(s);
                out.add(box_on(
                    FeatureKind::RailFurniture,
                    Material::Light,
                    m,
                    z + 4.0,
                    [0.4, 0.35, 0.9],
                    geom::yaw_of(t),
                    false,
                ));
            }
        }
        s += 700.0;
    }
}
