//! Westline yard (sidings, ladders, stored trains, yard tower), the roundhouse (stalls,
//! turntable, old locomotives, the lead track from the yard) and the perimeter fences of
//! every rail site.

use wp_math::Rng;

use super::super::ctx::{box_on, cylinder, obb, prism, Ctx};
use super::super::ground::{inside, signed_dist};
use super::super::plan::{Site, SiteKind};
use super::align::{fillet, resample, RailAlignment, BALLAST_HALF_M, SAMPLE_M, TRACK_SPACING_M};
use super::cars::{self, CarKind};
use super::geo::{self, piece, sector, strip, Paving};
use super::track::{buffer_stop, fence_run, lay_track, Output};
use crate::geom::{self, V2, V3};
use crate::network::{FeatureKind, Material};

/// Sidings on each side of the through tracks.
const SIDINGS_PER_SIDE: usize = 8;
/// Distance between siding centres, m.
const SIDING_SPACING_M: f32 = 6.0;
/// Offset of the first siding from the yard centreline, m.
const FIRST_SIDING_M: f32 = 12.0;
/// Ladder slope (lateral / along).
const LADDER_SLOPE: f32 = 0.3;
/// Yard throat margin inside the site boundary, m.
const THROAT_M: f32 = 15.0;
/// Turntable pit radius, m.
const TURNTABLE_R_M: f32 = 14.5;
/// Roundhouse stall ring radii, m.
const STALL_R: (f32, f32) = (40.0, 70.0);

/// Point at arc length `s` on `a`, offset `o` to the right (z = formation).
fn at_off(a: &RailAlignment, s: f32, o: f32) -> V3 {
    let (p, t) = a.at(s);
    let n = geom::right_of(t);
    [p[0] + n[0] * o, p[1] + n[1] * o, p[2]]
}

/// The yard: the RailYard site a line runs through, and that line.
fn yard_site<'a>(
    ctx: &'a Ctx,
    lines: &'a [RailAlignment],
) -> Option<(&'a Site, &'a RailAlignment)> {
    ctx.plan.sites_of(SiteKind::RailYard).find_map(|s| {
        lines
            .iter()
            .map(|a| {
                let k = a
                    .pts
                    .iter()
                    .filter(|p| inside(&s.polygon, geom::xy(**p)))
                    .count();
                (k, a)
            })
            .filter(|(k, _)| *k > 20)
            .max_by_key(|(k, _)| *k)
            .map(|(_, a)| (s, a))
    })
}

/// The roundhouse: a RailYard site no line runs through.
fn roundhouse_site<'a>(ctx: &'a Ctx, lines: &[RailAlignment]) -> Option<&'a Site> {
    ctx.plan.sites_of(SiteKind::RailYard).find(|s| {
        !lines
            .iter()
            .any(|a| a.pts.iter().any(|p| inside(&s.polygon, geom::xy(*p))))
    })
}

/// Turntable centre and the unit direction from it towards the roundhouse gate.
fn turntable(site: &Site) -> (V2, V2) {
    let c = centroid(&site.polygon);
    let u = site
        .gates
        .first()
        .map(|g| geom::normalize(geom::sub(g.pos, c)))
        .unwrap_or([0.0, 1.0]);
    (geo::along(c, u, -20.0), u)
}

fn centroid(poly: &[V2]) -> V2 {
    let n = poly.len() as f32;
    let s = poly.iter().fold([0.0, 0.0], |a, p| geom::add(a, *p));
    [s[0] / n, s[1] / n]
}

/// A siding / ladder / lead between arc lengths along a sampled path; the samples that are
/// not on paving are active.
fn sampled(points: Vec<V3>, paving: &Paving) -> (Vec<V3>, Vec<bool>) {
    let n = points.len();
    let mut active = vec![true; n];
    for i in 0..n {
        let p = geom::xy(points[i]);
        let q = geom::xy(points[(i + 1).min(n - 1)]);
        let fp = strip(p, geo::along(q, [0.0, 0.0], 0.0), BALLAST_HALF_M);
        let z = points[i][2];
        if paving.hits(&fp, 2.0, z - 1.5, z + 3.0) {
            active[i] = false;
            if i + 1 < n {
                active[i + 1] = false;
            }
        }
    }
    (points, active)
}

/// Sample a straight track from `a` to `b` every [`SAMPLE_M`].
fn straight(a: V3, b: V3) -> Vec<V3> {
    let n = ((geom::dist(geom::xy(a), geom::xy(b)) / SAMPLE_M).ceil() as usize).max(1);
    (0..=n)
        .map(|k| {
            let t = k as f32 / n as f32;
            [
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            ]
        })
        .collect()
}

/// Runs of consecutive active samples (index ranges, inclusive).
fn runs(active: &[bool]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < active.len() {
        if !active[i] {
            i += 1;
            continue;
        }
        let j0 = i;
        while i < active.len() && active[i] {
            i += 1;
        }
        if i - 1 > j0 {
            out.push((j0, i - 1));
        }
    }
    out
}

/// Westline yard: sidings both sides of the through line with ladders at both throats,
/// stored freight trains, a yard tower and the lead to the roundhouse.
pub fn build_yard(
    ctx: &Ctx,
    lines: &[RailAlignment],
    paving: &Paving,
    rng: &mut Rng,
    out: &mut Output,
) {
    let Some((site, main)) = yard_site(ctx, lines) else {
        return;
    };
    let idx: Vec<usize> = (0..main.pts.len())
        .filter(|&i| inside(&site.polygon, geom::xy(main.pts[i])))
        .collect();
    let (s_in, s_out) = (main.cum[idx[0]], main.cum[*idx.last().unwrap_or(&idx[0])]);
    let (sw, se) = (s_in + THROAT_M, s_out - THROAT_M);
    let s_mid = (sw + se) * 0.5;
    let z = site.pad_z.unwrap_or(main.pts[idx[0]][2]);
    let lift = |p: V3| [p[0], p[1], z];
    let inner = (main.tracks.max(1) as f32 - 1.0) * TRACK_SPACING_M * 0.5;

    // Siding offsets that fit (10 m inside the site at mid-yard).
    let mut offsets: Vec<(f32, Option<f32>)> = Vec::new();
    for sign in [1.0f32, -1.0] {
        for k in 0..SIDINGS_PER_SIDE {
            let o = sign * (FIRST_SIDING_M + SIDING_SPACING_M * k as f32);
            let p = at_off(main, s_mid, o);
            if signed_dist(&site.polygon, geom::xy(p)) < -10.0 {
                offsets.push((o, None));
            }
        }
    }
    // Lines ending at the yard boundary connect to the nearest siding, which runs to them.
    for b in lines.iter().filter(|b| b.name != main.name) {
        for e in [b.pts[0], *b.pts.last().unwrap_or(&b.pts[0])] {
            if signed_dist(&site.polygon, geom::xy(e)).abs() > 8.0 {
                continue;
            }
            let pr = main.project(geom::xy(e));
            if let Some(best) = offsets
                .iter_mut()
                .filter(|(o, _)| o.signum() == pr.lateral.signum())
                .min_by(|x, y| {
                    (x.0 - pr.lateral)
                        .abs()
                        .total_cmp(&(y.0 - pr.lateral).abs())
                })
            {
                *best = (pr.lateral, Some(pr.s));
            }
        }
    }
    let o_max = |sign: f32| {
        offsets
            .iter()
            .filter(|(o, _)| o.signum() == sign)
            .map(|(o, _)| o.abs())
            .fold(0.0, f32::max)
    };

    // Ladders.
    for sign in [1.0f32, -1.0] {
        let om = o_max(sign);
        if om <= 0.0 {
            continue;
        }
        let run = (om - inner) / LADDER_SLOPE;
        for (s0, s1) in [(sw, sw + run), (se, se - run)] {
            let pts = straight(
                lift(at_off(main, s0, sign * inner)),
                lift(at_off(main, s1, sign * om)),
            );
            let (pts, active) = sampled(pts, paving);
            lay_track(out, &pts, &active);
        }
    }

    // Sidings, with trains on all but the connecting ones.
    for (o, to) in &offsets {
        let shift = (o.abs() - inner) / LADDER_SLOPE;
        let (mut s0, mut s1) = (sw + shift, se - shift);
        if let Some(t) = to {
            if *t > s_mid {
                s1 = *t;
            } else {
                s0 = *t;
            }
        }
        let n = (((s1 - s0) / SAMPLE_M).ceil() as usize).max(1);
        let pts: Vec<V3> = (0..=n)
            .map(|k| lift(at_off(main, s0 + (s1 - s0) * k as f32 / n as f32, *o)))
            .collect();
        let (pts, active) = sampled(pts, paving);
        lay_track(out, &pts, &active);
        let cum = geom::cumulative(&pts);
        for (r0, r1) in runs(&active) {
            // Buffer stops where a siding stops at a road.
            if r0 > 0 {
                let d = geom::normalize(geom::sub(geom::xy(pts[r0]), geom::xy(pts[r0 + 1])));
                buffer_stop(out, paving, pts[r0], d);
            }
            if r1 + 1 < pts.len() {
                let d = geom::normalize(geom::sub(geom::xy(pts[r1]), geom::xy(pts[r1 - 1])));
                buffer_stop(out, paving, pts[r1], d);
            }
            if to.is_some() {
                continue;
            }
            let (a, b) = (cum[r0] + 6.0, cum[r1] - 6.0);
            let mut s = a + rng.range_f32(0.0, 25.0);
            while s < b - 30.0 {
                if rng.chance(0.3) {
                    s += rng.range_f32(15.0, 70.0);
                    continue;
                }
                let len = rng.range_f32(80.0, 280.0).min(b - s);
                let loco = rng.chance(0.35);
                let consist = cars::freight_consist(rng, len, loco);
                if consist.is_empty() {
                    break;
                }
                s = cars::park(&pts, &cum, s, &consist, rng, &mut out.features);
                s += rng.range_f32(6.0, 30.0);
            }
        }
    }

    // Yard tower beside the outermost sidings, where there is room.
    'tower: for sign in [1.0f32, -1.0] {
        for ds in [0.0f32, -60.0, 60.0, -120.0, 120.0] {
            let c = geom::xy(at_off(main, s_mid + ds, sign * (o_max(sign) + 16.0)));
            let fp = obb(c, [5.0, 5.0], 0.0);
            if signed_dist(&site.polygon, c) > -8.0
                || paving.hits_any(&fp, 3.0)
                || out.track_distance(c) < 8.0
            {
                continue;
            }
            let g = ctx.ground_z(c);
            out.add(box_on(
                FeatureKind::Landmark,
                Material::Brick,
                c,
                g - 0.3,
                [5.0, 5.0, 12.3],
                0.0,
                true,
            ));
            out.add(box_on(
                FeatureKind::Landmark,
                Material::Glass,
                c,
                g + 12.0,
                [8.0, 8.0, 3.2],
                0.0,
                true,
            ));
            out.add(box_on(
                FeatureKind::Landmark,
                Material::DarkConcrete,
                c,
                g + 15.2,
                [9.0, 9.0, 0.5],
                0.0,
                true,
            ));
            break 'tower;
        }
    }

    // Lead from the outermost siding on the roundhouse side to the turntable.
    if let Some(rh) = roundhouse_site(ctx, lines) {
        let (t, u) = turntable(rh);
        let entry = geo::along(t, u, -TURNTABLE_R_M - 0.5);
        let pr = main.project(entry);
        let sign = pr.lateral.signum();
        let corner = geom::xy(at_off(main, pr.s, sign * o_max(sign)));
        let back = if pr.s > s_mid { -1.0 } else { 1.0 };
        let start = geom::xy(at_off(main, pr.s + back * 150.0, sign * o_max(sign)));
        let path = resample(
            &fillet(&[start, corner, entry], &[0.0, 150.0, 0.0], 2.0),
            SAMPLE_M,
        );
        let pts: Vec<V3> = path
            .iter()
            .map(|p| [p[0], p[1], ctx.ground_z(*p)])
            .collect();
        let (pts, active) = sampled(pts, paving);
        lay_track(out, &pts, &active);
        // Planks where the lead crosses a street (never a highway: the lead stops at
        // buffer stops either side of one); fences on both sides between the sites.
        let hw: Vec<bool> = (0..pts.len())
            .map(|i| {
                let (a, b) = (pts[i], pts[(i + 1).min(pts.len() - 1)]);
                let fp = strip(geom::xy(a), geom::xy(b), BALLAST_HALF_M);
                paving.hits_highway(&fp, 2.0, a[2] - 1.5, a[2] + 3.0)
            })
            .collect();
        let hw_near = |i: usize| (i.saturating_sub(2)..(i + 3).min(pts.len())).any(|j| hw[j]);
        for (r0, r1) in runs(&active) {
            if r0 > 0 && hw_near(r0 - 1) {
                let d = geom::normalize(geom::sub(geom::xy(pts[r0]), geom::xy(pts[r0 + 1])));
                buffer_stop(out, paving, pts[r0], d);
            }
            if r1 + 1 < pts.len() && hw_near(r1 + 1) {
                let d = geom::normalize(geom::sub(geom::xy(pts[r1]), geom::xy(pts[r1 - 1])));
                buffer_stop(out, paving, pts[r1], d);
            }
        }
        for i in 0..pts.len() - 1 {
            if (!active[i] || !active[i + 1]) && !hw_near(i) && !hw_near(i + 1) {
                let (a, b) = (pts[i], pts[i + 1]);
                let z = ctx.ground_z(geom::xy(a)).max(ctx.ground_z(geom::xy(b)));
                out.add(piece(
                    FeatureKind::RailTrack,
                    Material::Wood,
                    [a[0], a[1], z],
                    [b[0], b[1], z],
                    BALLAST_HALF_M * 2.0,
                    -0.12,
                    0.03,
                    0.1,
                    false,
                ));
            }
        }
        let ok = |p: V2, q: V2, _: f32, _: f32| {
            let m = super::super::ctx::lerp2(p, q, 0.5);
            !ctx.plan
                .sites
                .iter()
                .any(|s| signed_dist(&s.polygon, m) < 2.0)
        };
        for side in [-1.0f32, 1.0] {
            let off: Vec<V2> = (0..path.len())
                .map(|i| {
                    let a = path[i.saturating_sub(1)];
                    let b = path[(i + 1).min(path.len() - 1)];
                    let n = geom::right_of(geom::normalize(geom::sub(b, a)));
                    geo::along(path[i], n, side * (BALLAST_HALF_M + 3.0))
                })
                .collect();
            for w in off.windows(2) {
                fence_run(ctx, paving, out, w[0], w[1], 0.0, 0.0, &ok);
            }
        }
        for i in (0..path.len().saturating_sub(2)).step_by(2) {
            if !ctx.plan.sites.iter().any(|s| inside(&s.polygon, path[i])) {
                out.occupy
                    .push(strip(path[i], path[i + 2], BALLAST_HALF_M + 3.5));
            }
        }
    }
}

/// The roundhouse: semicircular stall building (convex prism pieces) around a turntable,
/// open towards the gate through a passage, with old locomotives in some stalls.
pub fn build_roundhouse(
    ctx: &Ctx,
    lines: &[RailAlignment],
    _paving: &Paving,
    rng: &mut Rng,
    out: &mut Output,
) {
    let Some(site) = roundhouse_site(ctx, lines) else {
        return;
    };
    let (t, u) = turntable(site);
    let base = site.pad_z.unwrap_or_else(|| ctx.ground_z(t));
    let phi = wp_math::atan2(u[1], u[0]);
    let deg = core::f32::consts::PI / 180.0;
    let (r0, r1) = STALL_R;
    let stall_w = 65.0 * deg / 6.0;
    let wall_half = 0.25 / ((r0 + r1) * 0.5);
    let mut boundaries: Vec<f32> = Vec::new();
    for side in [-1.0f32, 1.0] {
        for k in 0..6 {
            let a0 = phi + side * (10.0 * deg + stall_w * k as f32);
            let a1 = a0 + side * stall_w;
            let (lo, hi) = if side > 0.0 { (a0, a1) } else { (a1, a0) };
            boundaries.push(a0);
            if k == 5 {
                boundaries.push(a1);
            }
            out.add(prism(
                FeatureKind::Landmark,
                Material::DarkConcrete,
                sector(t, r0, r1, lo, hi),
                base + 7.0,
                base + 7.6,
                true,
            ));
            out.add(prism(
                FeatureKind::Landmark,
                Material::Brick,
                sector(t, r1 - 0.6, r1, lo, hi),
                base - 0.3,
                base + 7.0,
                true,
            ));
            // Stall track and, in some stalls, an old locomotive facing the turntable.
            let mid = (lo + hi) * 0.5;
            let (s, c) = wp_math::sin_cos(mid);
            let dir = [c, s];
            let pts: Vec<V3> = (0..=10)
                .map(|k| {
                    let p = geo::along(
                        t,
                        dir,
                        TURNTABLE_R_M + 1.0 + (r1 - 4.0 - TURNTABLE_R_M - 1.0) * k as f32 / 10.0,
                    );
                    [p[0], p[1], base]
                })
                .collect();
            lay_track(out, &pts, &[true; 11]);
            if rng.chance(0.5) {
                let outer = geo::along(t, dir, r1 - 3.0);
                let inner = geo::along(t, dir, r1 - 3.0 - CarKind::Steam.length());
                cars::car(
                    CarKind::Steam,
                    [outer[0], outer[1], base],
                    [inner[0], inner[1], base],
                    rng,
                    &mut out.features,
                );
            }
        }
    }
    for a in boundaries {
        out.add(prism(
            FeatureKind::Landmark,
            Material::Brick,
            sector(t, r0, r1 - 0.6, a - wall_half, a + wall_half),
            base - 0.3,
            base + 7.0,
            true,
        ));
    }
    // Turntable: pit kerb ring, bridge with rails, pivot.
    let segs = 24;
    for k in 0..segs {
        let a0 = k as f32 * core::f32::consts::TAU / segs as f32;
        let a1 = a0 + core::f32::consts::TAU / segs as f32;
        out.add(prism(
            FeatureKind::RailFurniture,
            Material::Concrete,
            sector(t, TURNTABLE_R_M, TURNTABLE_R_M + 0.8, a0, a1),
            base - 0.3,
            base + 0.3,
            true,
        ));
    }
    let a = geo::along(t, u, -TURNTABLE_R_M + 0.3);
    let b = geo::along(t, u, TURNTABLE_R_M - 0.3);
    out.add(piece(
        FeatureKind::RailFurniture,
        Material::Steel,
        [a[0], a[1], base],
        [b[0], b[1], base],
        4.2,
        0.0,
        0.5,
        0.0,
        true,
    ));
    super::track::track_piece(
        out,
        [a[0], a[1], base + 0.5 - 0.30],
        [b[0], b[1], base + 0.5 - 0.30],
    );
    out.add(cylinder(
        FeatureKind::RailFurniture,
        Material::Rust,
        [t[0], t[1], base],
        1.2,
        0.45,
        false,
    ));
}

/// Perimeter fences of every rail site (yards, roundhouse, stations): gaps at the plan
/// gates, where tracks leave the site at grade and where roads cross the boundary; where a
/// road runs along the boundary the fence steps inside it.
pub fn site_fences(ctx: &Ctx, lines: &[RailAlignment], paving: &Paving, out: &mut Output) {
    let sites: Vec<&Site> = ctx
        .plan
        .sites
        .iter()
        .filter(|s| matches!(s.kind, SiteKind::RailYard | SiteKind::Station))
        .collect();
    for site in sites {
        let c = centroid(&site.polygon);
        let n = site.polygon.len();
        for i in 0..n {
            let (p, q) = (site.polygon[i], site.polygon[(i + 1) % n]);
            let len = geom::dist(p, q);
            let pieces = (len / 10.0).ceil().max(1.0) as usize;
            let dir = geom::normalize(geom::sub(q, p));
            let inward = {
                let l = [-dir[1], dir[0]];
                if geom::dot(l, geom::sub(c, p)) > 0.0 {
                    l
                } else {
                    [-l[0], -l[1]]
                }
            };
            for k in 0..pieces {
                let a0 = super::super::ctx::lerp2(p, q, k as f32 / pieces as f32);
                let b0 = super::super::ctx::lerp2(p, q, (k + 1) as f32 / pieces as f32);
                let m0 = super::super::ctx::lerp2(a0, b0, 0.5);
                if site
                    .gates
                    .iter()
                    .any(|g| geom::dist(g.pos, m0) < g.width_m * 0.5 + 6.0)
                {
                    continue;
                }
                // Step inside a road running along the boundary; where a road crosses
                // the boundary every inset is paved and the fence leaves a gap there.
                let mut inset = 1.5;
                for d in [1.5f32, 6.0, 12.0, 20.0, 28.0] {
                    let fp = strip(geo::along(a0, inward, d), geo::along(b0, inward, d), 0.1);
                    let z = ctx.ground_z(m0);
                    if !paving.hits(&fp, 0.6, z - 1.0, z + 3.0) {
                        inset = d;
                        break;
                    }
                }
                perimeter_piece(
                    ctx,
                    lines,
                    paving,
                    out,
                    geo::along(a0, inward, inset),
                    geo::along(b0, inward, inset),
                );
            }
        }
    }
}

/// One perimeter piece, leaving gaps where tracks leave the site at grade.
fn perimeter_piece(
    ctx: &Ctx,
    lines: &[RailAlignment],
    paving: &Paving,
    out: &mut Output,
    a: V2,
    b: V2,
) {
    let accept = |x: V2, y: V2| {
        let m = super::super::ctx::lerp2(x, y, 0.5);
        if out.track_distance(m) < BALLAST_HALF_M + 2.5 {
            return false;
        }
        !lines.iter().any(|l| {
            let pr = l.project(m);
            pr.dist < l.half_span_m() + 2.5
                && l.support[l.index_at(pr.s.clamp(0.0, l.length()))].on_ground()
        })
    };
    let subs: Vec<(V2, V2)> = (0..4)
        .map(|m| {
            (
                super::super::ctx::lerp2(a, b, m as f32 / 4.0),
                super::super::ctx::lerp2(a, b, (m + 1) as f32 / 4.0),
            )
        })
        .filter(|(x, y)| accept(*x, *y))
        .collect();
    let yes = |_: V2, _: V2, _: f32, _: f32| true;
    if subs.len() == 4 {
        fence_run(ctx, paving, out, a, b, 0.0, 0.0, &yes);
    } else {
        for (x, y) in subs {
            fence_run(ctx, paving, out, x, y, 0.0, 0.0, &yes);
        }
    }
}
