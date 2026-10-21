//! Waterline analysis of a site boundary, quay walls with kerbs and mooring bollards on the
//! seaward samples, and the perimeter fence (with gate / rail / road openings) on the land
//! samples, plus the closing fences and corner fills where the two meet.

use super::{lerp, off, put, Fr};
use crate::geom::{self, V2};
use crate::metro::ctx::{box_on, ccw, yaw_of, Ctx, Occ};
use crate::metro::plan::{Gate, Plan};
use crate::network::{Feature, FeatureKind, Material};

/// Boundary sample spacing, m.
pub const STEP: f32 = 6.0;
/// How far outward from a site edge water is looked for, m.
pub const REACH: f32 = 100.0;
/// Quay face beyond the waterline, m.
const FACE_OUT_M: f32 = 2.5;
/// Kerb height above the quay top, m.
pub const KERB_H: f32 = 0.8;

/// The seaward part of a boundary sample (distances along the outward normal).
#[derive(Debug, Clone, Copy)]
pub struct Sea {
    /// Inner edge of the quay wall (top of the pad's slope), m.
    pub d_in: f32,
    /// First water, m.
    pub d_w: f32,
    /// Quay face (outer edge), m.
    pub d_out: f32,
    /// Quay top height, m.
    pub z_top: f32,
}

/// One boundary sample.
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    /// Point on the boundary.
    pub p: V2,
    /// Distance along the edge.
    pub s: f32,
    /// Seaward data (water within [`REACH`] outward), if any.
    pub sea: Option<Sea>,
    /// The boundary point itself is in the water.
    pub wet: bool,
}

/// One site edge with its samples.
#[derive(Debug, Clone)]
pub struct Edge {
    /// Start vertex.
    pub a: V2,
    /// End vertex.
    pub b: V2,
    /// Unit direction a → b.
    pub t: V2,
    /// Outward unit normal.
    pub n: V2,
    /// Length.
    pub len: f32,
    /// Samples (centres of `STEP`-long intervals).
    pub samples: Vec<Sample>,
}

impl Edge {
    /// Frame at the edge start, `u` along the edge, `v` outward.
    pub fn frame(&self) -> Fr {
        Fr::new(self.a, self.t)
    }

    /// Spacing of the samples.
    pub fn step(&self) -> f32 {
        self.len / self.samples.len().max(1) as f32
    }
}

/// Sample a (CCW) site polygon. With `quay`, march outward from each sample to the water.
pub fn analyse(ctx: &Ctx, poly: &[V2], quay: bool) -> Vec<Edge> {
    let poly = ccw(poly.to_vec());
    let mut edges = Vec::new();
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let len = geom::dist(a, b);
        if len < 1.0 {
            continue;
        }
        let t = geom::normalize(geom::sub(b, a));
        let n = geom::right_of(t);
        let count = (len / STEP).ceil().max(1.0) as usize;
        let step = len / count as f32;
        let samples = (0..count)
            .map(|k| {
                let s = (k as f32 + 0.5) * step;
                let p = off(a, t, s);
                let wet = ctx.is_water(p);
                let sea = if quay { seaward(ctx, p, n) } else { None };
                Sample { p, s, sea, wet }
            })
            .collect();
        edges.push(Edge {
            a,
            b,
            t,
            n,
            len,
            samples,
        });
    }
    edges
}

fn seaward(ctx: &Ctx, p: V2, n: V2) -> Option<Sea> {
    let mut d_w = None;
    let mut d = 0.0;
    while d <= REACH {
        if ctx.is_water(off(p, n, d)) {
            d_w = Some(d);
            break;
        }
        d += 1.0;
    }
    let d_w = d_w?;
    let z_ref = ctx.ground_z(off(p, n, -2.0));
    let mut d_top = d_w;
    let mut d = 0.0;
    while d < d_w {
        if ctx.ground_z(off(p, n, d)) < z_ref - 0.4 {
            d_top = d;
            break;
        }
        d += 1.0;
    }
    // A short blend slope right outside the pad is covered from the boundary; a longer
    // strip of land is left as it is and the wall starts just before its slope.
    let (d_in, z_top) = if d_top <= 12.0 {
        (0.0, z_ref)
    } else {
        let d_in = d_top - 3.0;
        (d_in, ctx.ground_z(off(p, n, d_in)))
    };
    Some(Sea {
        d_in,
        d_w,
        d_out: d_w + FACE_OUT_M,
        z_top,
    })
}

/// Runs of consecutive seaward samples on an edge: (first, last) sample index.
pub fn sea_runs(e: &Edge) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut k = 0;
    while k < e.samples.len() {
        if e.samples[k].sea.is_some() {
            let k0 = k;
            while k + 1 < e.samples.len() && e.samples[k + 1].sea.is_some() {
                k += 1;
            }
            out.push((k0, k));
        }
        k += 1;
    }
    out
}

/// Reserve the land strip between the site boundary and a quay wall that starts further
/// out (pad extensions) as `Occ::Site`, so later modules keep out of it.
pub fn reserve_strips(ctx: &mut Ctx, edges: &[Edge]) {
    for e in edges {
        let fr = e.frame();
        let h = e.step() * 0.5;
        for s in &e.samples {
            if let Some(sea) = s.sea {
                if sea.d_in > 3.0 {
                    let poly = fr.rect(s.s - h, s.s + h, 0.0, sea.d_in);
                    ctx.occupy(&poly, 0.0, Occ::Site);
                }
            }
        }
    }
}

/// Quay walls, kerbs and bollards along all seaward samples, corner fills between two
/// seaward edges. Returns the waterline points (point, outward normal).
pub fn quay_walls(ctx: &mut Ctx, edges: &[Edge]) -> Vec<(V2, V2)> {
    let mut waterline = Vec::new();
    for e in edges {
        let fr = e.frame();
        let step = e.step();
        for (k0, k1) in sea_runs(e) {
            let mut k = k0;
            while k <= k1 {
                let first = e.samples[k].sea.expect("seaward");
                let mut last = k;
                while last < k1 && last - k < 3 {
                    let nx = e.samples[last + 1].sea.expect("seaward");
                    if (nx.d_in - first.d_in).abs() > 1.5 || (nx.d_out - first.d_out).abs() > 2.0 {
                        break;
                    }
                    last += 1;
                }
                let group = &e.samples[k..=last];
                let d_in = group
                    .iter()
                    .map(|s| s.sea.expect("seaward").d_in)
                    .fold(f32::MAX, f32::min);
                let d_out = group
                    .iter()
                    .map(|s| s.sea.expect("seaward").d_out)
                    .fold(f32::MIN, f32::max);
                let z_top = first.z_top;
                let z_bed = group
                    .iter()
                    .map(|s| ctx.ground_z(off(s.p, e.n, s.sea.expect("seaward").d_out)))
                    .fold(f32::MAX, f32::min)
                    .min(z_top - 4.0)
                    - 1.5;
                let a0 = group[0].s - step * 0.5;
                let a1 = group[group.len() - 1].s + step * 0.5;
                let am = (a0 + a1) * 0.5;
                let len = a1 - a0 + 0.1;
                let b0 = d_in - 0.5;
                put(
                    ctx,
                    fr.bx(
                        FeatureKind::QuayWall,
                        Material::Concrete,
                        am,
                        (b0 + d_out) * 0.5,
                        z_bed,
                        len,
                        d_out - b0,
                        z_top - z_bed,
                        true,
                    ),
                );
                put(
                    ctx,
                    fr.bx(
                        FeatureKind::QuayWall,
                        Material::SafetyYellow,
                        am,
                        d_out - 0.35,
                        z_top - 0.2,
                        len,
                        0.6,
                        KERB_H + 0.2,
                        true,
                    ),
                );
                put(
                    ctx,
                    super::super::ctx::cylinder(
                        FeatureKind::MooringBollard,
                        Material::Black,
                        {
                            let q = fr.at(am, d_out - 1.4);
                            [q[0], q[1], z_top]
                        },
                        0.35,
                        0.7,
                        true,
                    ),
                );
                for s in group {
                    waterline.push((off(s.p, e.n, s.sea.expect("seaward").d_w), e.n));
                }
                k = last + 1;
            }
        }
    }
    // Corner fills between two seaward edges (convex corners only).
    for i in 0..edges.len() {
        let (e0, e1) = (&edges[i], &edges[(i + 1) % edges.len()]);
        if geom::dist(e0.b, e1.a) > 0.5 || geom::cross(e0.t, e1.t) <= 0.0 {
            continue;
        }
        let (Some(s0), Some(s1)) = (
            e0.samples.last().and_then(|s| s.sea),
            e1.samples.first().and_then(|s| s.sea),
        ) else {
            continue;
        };
        // Frame at the corner, u along e0, v along e0's outward normal.
        let fr = Fr::new(e0.b, e0.t);
        let (ext_u, ext_v) = (s1.d_out, s0.d_out);
        let z_top = s0.z_top.min(s1.z_top);
        let z_bed = ctx.ground_z(fr.at(ext_u, ext_v)).min(z_top - 4.0) - 1.5;
        put(
            ctx,
            fr.bx(
                FeatureKind::QuayWall,
                Material::Concrete,
                ext_u * 0.5 - 0.25,
                ext_v * 0.5,
                z_bed,
                ext_u + 0.5,
                ext_v,
                z_top - z_bed,
                true,
            ),
        );
        put(
            ctx,
            fr.bx(
                FeatureKind::QuayWall,
                Material::SafetyYellow,
                ext_u * 0.5,
                ext_v - 0.35,
                z_top - 0.2,
                ext_u,
                0.6,
                KERB_H + 0.2,
                true,
            ),
        );
        put(
            ctx,
            fr.bx(
                FeatureKind::QuayWall,
                Material::SafetyYellow,
                ext_u - 0.35,
                ext_v * 0.5,
                z_top - 0.2,
                0.6,
                ext_v,
                KERB_H + 0.2,
                true,
            ),
        );
        waterline.push((fr.at(ext_u - FACE_OUT_M, ext_v - FACE_OUT_M), e0.n));
    }
    waterline
}

/// Perimeter fence specification.
#[derive(Debug, Clone, Copy)]
pub struct FenceSpec {
    /// Feature kind (`Fence` / `Wall`).
    pub kind: FeatureKind,
    /// Material.
    pub mat: Material,
    /// Thickness, m.
    pub thick: f32,
    /// Height above the ground, m.
    pub height: f32,
}

/// Fence along every land sample of the boundary (not seaward, not in the water), with
/// openings `gap_half(gate)` either side of every gate, 7 m either side of every rail line
/// crossing, and wherever a piece would touch road paving / verge. Closing fences run from
/// the boundary out to the quay face where a seaward run meets a land run.
pub fn perimeter(
    ctx: &mut Ctx,
    plan: &Plan,
    edges: &[Edge],
    gates: &[Gate],
    gap_half: &dyn Fn(&Gate) -> f32,
    spec: FenceSpec,
) {
    let mut lines: Vec<(V2, V2, f32)> = Vec::new();
    for e in edges {
        let step = e.step();
        // Land intervals.
        let mut iv: Vec<(f32, f32)> = Vec::new();
        for s in &e.samples {
            if s.sea.is_some() || s.wet {
                continue;
            }
            let (a, b) = (s.s - step * 0.5, s.s + step * 0.5);
            match iv.last_mut() {
                Some(last) if (last.1 - a).abs() < 1e-3 => last.1 = b,
                _ => iv.push((a, b)),
            }
        }
        // Openings.
        let mut cuts: Vec<(f32, f32)> = Vec::new();
        for g in gates {
            let rel = geom::sub(g.pos, e.a);
            let along = geom::dot(rel, e.t);
            let across = geom::dot(rel, e.n).abs();
            if across < 4.0 && along > -20.0 && along < e.len + 20.0 {
                let h = gap_half(g);
                cuts.push((along - h, along + h));
            }
        }
        for r in &plan.rail {
            for w in r.line.windows(2) {
                if let Some(along) = seg_cross(e.a, e.b, w[0], w[1]) {
                    cuts.push((along - 7.0, along + 7.0));
                }
            }
        }
        for (a, b) in subtract(&iv, &cuts) {
            if b - a > 0.5 {
                lines.push((off(e.a, e.t, a), off(e.a, e.t, b), f32::MIN));
            }
        }
        // Closing fences at mid-edge transitions.
        for k in 1..e.samples.len() {
            let (p, q) = (&e.samples[k - 1], &e.samples[k]);
            let (sea, land) = match (p.sea, q.sea) {
                (Some(s), None) => (s, q),
                (None, Some(s)) => (s, p),
                _ => continue,
            };
            if land.wet {
                continue;
            }
            let base = off(e.a, e.t, (p.s + q.s) * 0.5);
            lines.push((base, off(base, e.n, sea.d_out + 0.3), sea.z_top));
        }
    }
    // Closing fences at corners between a seaward and a land edge.
    for i in 0..edges.len() {
        let (e0, e1) = (&edges[i], &edges[(i + 1) % edges.len()]);
        if geom::dist(e0.b, e1.a) > 0.5 {
            continue;
        }
        let (Some(l0), Some(f1)) = (e0.samples.last(), e1.samples.first()) else {
            continue;
        };
        match (l0.sea, f1.sea) {
            (Some(s), None) if !f1.wet => {
                lines.push((e0.b, off(e0.b, e0.n, s.d_out + 0.3), s.z_top));
            }
            (None, Some(s)) if !l0.wet => {
                lines.push((e1.a, off(e1.a, e1.n, s.d_out + 0.3), s.z_top));
            }
            _ => {}
        }
    }
    for (a, b, z_floor) in lines {
        for f in fence_line(ctx, spec, a, b, z_floor) {
            put(ctx, f);
        }
    }
}

/// Fence pieces (≤ 12 m) from `a` to `b` following the ground; the top is at least
/// `z_floor + height`.
pub fn fence_line(ctx: &Ctx, spec: FenceSpec, a: V2, b: V2, z_floor: f32) -> Vec<Feature> {
    let len = geom::dist(a, b);
    let n = (len / 12.0).ceil().max(1.0) as usize;
    let mut out = Vec::new();
    for k in 0..n {
        let p = lerp(a, b, k as f32 / n as f32);
        let q = lerp(a, b, (k + 1) as f32 / n as f32);
        let m = lerp(p, q, 0.5);
        let zs = [ctx.ground_z(p), ctx.ground_z(q), ctx.ground_z(m)];
        let lo = zs.iter().copied().fold(f32::MAX, f32::min) - 0.3;
        let hi = zs.iter().copied().fold(f32::MIN, f32::max).max(z_floor) + spec.height;
        out.push(box_on(
            spec.kind,
            spec.mat,
            m,
            lo,
            [spec.thick, geom::dist(p, q) + spec.thick * 0.5, hi - lo],
            yaw_of(p, q),
            true,
        ));
    }
    out
}

/// Where segment `c`–`d` crosses segment `a`–`b`: distance along `a`–`b`.
fn seg_cross(a: V2, b: V2, c: V2, d: V2) -> Option<f32> {
    let r = geom::sub(b, a);
    let s = geom::sub(d, c);
    let den = geom::cross(r, s);
    if den.abs() < 1e-6 {
        return None;
    }
    let ac = geom::sub(c, a);
    let t = geom::cross(ac, s) / den;
    let u = geom::cross(ac, r) / den;
    if (-0.01..=1.01).contains(&t) && (-0.01..=1.01).contains(&u) {
        Some(t * geom::len(r))
    } else {
        None
    }
}

/// Interval list `iv` minus every interval in `cuts`.
fn subtract(iv: &[(f32, f32)], cuts: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut cur: Vec<(f32, f32)> = iv.to_vec();
    for &(c0, c1) in cuts {
        let mut next = Vec::new();
        for &(a, b) in &cur {
            if c1 <= a || c0 >= b {
                next.push((a, b));
                continue;
            }
            if c0 > a {
                next.push((a, c0));
            }
            if c1 < b {
                next.push((c1, b));
            }
        }
        cur = next;
    }
    cur
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_subtraction() {
        let r = subtract(&[(0.0, 10.0), (20.0, 30.0)], &[(5.0, 22.0)]);
        assert_eq!(r, vec![(0.0, 5.0), (22.0, 30.0)]);
    }

    #[test]
    fn segment_crossing() {
        let s = seg_cross([0.0, 0.0], [10.0, 0.0], [4.0, -1.0], [4.0, 1.0]);
        assert!((s.unwrap() - 4.0).abs() < 1e-4);
        assert!(seg_cross([0.0, 0.0], [10.0, 0.0], [4.0, 1.0], [4.0, 2.0]).is_none());
    }
}
