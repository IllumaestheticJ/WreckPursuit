//! Building blocks shared by the street lots and the landmark complexes: a local frame,
//! the [`Part`]s a lot or complex is made of (building masses, roof crowns, boxes,
//! cylinders, prisms, ground slabs, fences), and the two-phase check-then-commit that puts
//! them into the [`Ctx`].

use super::super::ctx::{box_on, cylinder, obb, prism, Ctx, Occ};
use super::super::ground::{inside, seg_dist, signed_dist};
use crate::geom::{self, V2};
use crate::network::{Building, Feature, FeatureKind, FeatureShape, Material};

/// A local frame: `u` along `t`, `v` along `n`, origin `o`.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    /// Origin, m.
    pub o: V2,
    /// Unit direction of local `u`.
    pub t: V2,
    /// Unit direction of local `v` (perpendicular to `t`, either side).
    pub n: V2,
}

impl Frame {
    /// An axis-aligned frame at `o` (`u` = +X, `v` = +Y).
    pub fn axis(o: V2) -> Self {
        Self {
            o,
            t: [1.0, 0.0],
            n: [0.0, 1.0],
        }
    }

    /// World point of local `(u, v)`.
    pub fn at(&self, u: f32, v: f32) -> V2 {
        [
            self.o[0] + self.t[0] * u + self.n[0] * v,
            self.o[1] + self.t[1] * u + self.n[1] * v,
        ]
    }

    /// World polygon of a local polygon (made counter-clockwise).
    pub fn poly(&self, local: &[V2]) -> Vec<V2> {
        super::super::ctx::ccw(local.iter().map(|p| self.at(p[0], p[1])).collect())
    }

    /// Yaw of the frame's `u` axis (a box's local +Y runs along `u`).
    pub fn yaw(&self) -> f32 {
        super::super::ctx::yaw_of([0.0, 0.0], self.t)
    }
}

/// Local rectangle `u0..u1 × v0..v1`.
pub fn rect(u0: f32, u1: f32, v0: f32, v1: f32) -> Vec<V2> {
    vec![[u0, v0], [u1, v0], [u1, v1], [u0, v1]]
}

/// Local rectangle with its corners cut by `c` (an octagon; convex).
pub fn chamfer(u0: f32, u1: f32, v0: f32, v1: f32, c: f32) -> Vec<V2> {
    let c = c.min((u1 - u0) * 0.45).min((v1 - v0) * 0.45).max(0.0);
    vec![
        [u0 + c, v0],
        [u1 - c, v0],
        [u1, v0 + c],
        [u1, v1 - c],
        [u1 - c, v1],
        [u0 + c, v1],
        [u0, v1 - c],
        [u0, v0 + c],
    ]
}

/// Regular `n`-gon of radius `r` around local `c` (convex).
pub fn ngon(c: V2, r: f32, n: usize) -> Vec<V2> {
    (0..n)
        .map(|k| {
            let (s, co) = wp_math::sin_cos(k as f32 * core::f32::consts::TAU / n as f32);
            [c[0] + r * co, c[1] + r * s]
        })
        .collect()
}

/// A boundary line style.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FenceSpec {
    /// Feature kind (`Wall`, `Fence`, `Hedge`).
    pub kind: FeatureKind,
    /// Material.
    pub mat: Material,
    /// Thickness, m.
    pub thick: f32,
    /// Height above the ground, m.
    pub height: f32,
}

/// Masonry wall.
pub const fn wall(mat: Material, height: f32) -> FenceSpec {
    FenceSpec {
        kind: FeatureKind::Wall,
        mat,
        thick: 0.4,
        height,
    }
}

/// Chain-link / board fence.
pub const fn fence(mat: Material, height: f32) -> FenceSpec {
    FenceSpec {
        kind: FeatureKind::Fence,
        mat,
        thick: 0.2,
        height,
    }
}

/// Hedge.
pub const fn hedge(height: f32) -> FenceSpec {
    FenceSpec {
        kind: FeatureKind::Hedge,
        mat: Material::Foliage,
        thick: 1.0,
        height,
    }
}

/// One piece of a lot or complex, in local frame coordinates.
#[derive(Debug, Clone)]
pub enum Part {
    /// A building (convex footprint): walls from the lowest ground under it, roof `h`
    /// above the highest.
    Mass {
        /// Footprint (convex), local.
        poly: Vec<V2>,
        /// Height above the highest ground, m.
        h: f32,
    },
    /// A prism on the roof of mass `mass` (crowns, atria), `dz` above the roof.
    Crown {
        /// Index of the mass part it stands on.
        mass: usize,
        /// Footprint (convex), local.
        poly: Vec<V2>,
        /// Offset above the roof, m.
        dz: f32,
        /// Height, m.
        h: f32,
        /// Kind.
        kind: FeatureKind,
        /// Material.
        mat: Material,
        /// Solid.
        solid: bool,
    },
    /// A cylinder on the roof of mass `mass`, `dz` above the roof (domes, spires, tanks).
    RoofCyl {
        /// Index of the mass part it stands on.
        mass: usize,
        /// Centre, local.
        c: V2,
        /// Offset above the roof, m.
        dz: f32,
        /// Radius, m.
        r: f32,
        /// Height, m.
        h: f32,
        /// Kind.
        kind: FeatureKind,
        /// Material.
        mat: Material,
        /// Solid.
        solid: bool,
    },
    /// A prism `z0` above the lowest ground under it (`z0 = 0`: sunk 0.3 m), top `h` above
    /// the highest ground plus `z0`.
    Prism {
        /// Footprint (convex if solid), local.
        poly: Vec<V2>,
        /// Bottom offset, m.
        z0: f32,
        /// Height, m.
        h: f32,
        /// Kind.
        kind: FeatureKind,
        /// Material.
        mat: Material,
        /// Solid.
        solid: bool,
    },
    /// A box `z0` above the ground at its centre (`z0 = 0`: from the lowest ground under it).
    Box {
        /// Centre, local.
        c: V2,
        /// Extent along `u`, m.
        du: f32,
        /// Extent along `v`, m.
        dv: f32,
        /// Bottom offset, m.
        z0: f32,
        /// Height, m.
        h: f32,
        /// Kind.
        kind: FeatureKind,
        /// Material.
        mat: Material,
        /// Solid.
        solid: bool,
        /// Check its footprint for clearance (false: overhangs / parts under a checked part).
        check: bool,
    },
    /// An upright cylinder `z0` above the ground at its centre.
    Cyl {
        /// Centre, local.
        c: V2,
        /// Radius, m.
        r: f32,
        /// Bottom offset, m.
        z0: f32,
        /// Height, m.
        h: f32,
        /// Kind.
        kind: FeatureKind,
        /// Material.
        mat: Material,
        /// Solid.
        solid: bool,
        /// Check its footprint for clearance.
        check: bool,
    },
    /// A flat ground slab (driveway, parking, plaza, pitch); dropped on slopes > 0.8 m.
    Flat {
        /// Centre, local.
        c: V2,
        /// Extent along `u`, m.
        du: f32,
        /// Extent along `v`, m.
        dv: f32,
        /// Material.
        mat: Material,
    },
    /// A sloped slab (conveyor, chute) from `a` (`za` above the ground there) to `b`.
    Ramp {
        /// Start, local.
        a: V2,
        /// Start height above the ground, m.
        za: f32,
        /// End, local.
        b: V2,
        /// End height above the ground, m.
        zb: f32,
        /// Width, m.
        w: f32,
        /// Kind.
        kind: FeatureKind,
        /// Material.
        mat: Material,
        /// Solid.
        solid: bool,
    },
    /// A boundary line from `a` to `b` with openings (gates) of `gap_m` around `gaps`.
    Line {
        /// Start, local.
        a: V2,
        /// End, local.
        b: V2,
        /// Style.
        spec: FenceSpec,
        /// Gate centres, local.
        gaps: Vec<V2>,
        /// Gate half width, m.
        gap_m: f32,
    },
}

/// Shorthand for a solid checked box standing on the ground.
#[allow(clippy::too_many_arguments)]
pub fn bx(c: V2, du: f32, dv: f32, z0: f32, h: f32, kind: FeatureKind, mat: Material) -> Part {
    Part::Box {
        c,
        du,
        dv,
        z0,
        h,
        kind,
        mat,
        solid: true,
        check: true,
    }
}

/// Shorthand for a solid checked cylinder.
pub fn cyl(c: V2, r: f32, z0: f32, h: f32, kind: FeatureKind, mat: Material) -> Part {
    Part::Cyl {
        c,
        r,
        z0,
        h,
        kind,
        mat,
        solid: true,
        check: true,
    }
}

/// Shorthand for a building mass from a local rectangle.
pub fn mass(u0: f32, u1: f32, v0: f32, v1: f32, h: f32) -> Part {
    Part::Mass {
        poly: rect(u0, u1, v0, v1),
        h,
    }
}

/// What a part becomes in the world.
enum Out {
    Building(Building),
    Feature(Feature, bool),
    Line(V2, V2, FenceSpec, Vec<V2>, f32),
    Skip,
}

/// How a group of parts is checked.
#[derive(Clone, Copy)]
pub enum Mode<'s> {
    /// Generic lot: everything must be [`Ctx::free`] or nothing is placed.
    Lot,
    /// Inside the caller's own site polygon: each part placed when free of everything but
    /// the site (parts outside the polygon or blocked are skipped one by one).
    Site(&'s [V2]),
}

/// Place `parts` (local to `frame`). In [`Mode::Lot`] returns false (placing nothing) when
/// any checked part is blocked or the ground is unsuitable; in [`Mode::Site`] places what
/// fits and returns true when at least one mass or feature was placed.
pub fn commit(ctx: &mut Ctx, frame: &Frame, parts: &[Part], mode: Mode) -> bool {
    // Phase 1: world geometry + clearance polygons.
    let mut outs: Vec<(Out, Option<Vec<V2>>, Option<usize>)> = Vec::with_capacity(parts.len());
    let mut roof: Vec<f32> = vec![0.0; parts.len()];
    // Unchecked boxes / cylinders (stacked rings, lamp heads, lintels) follow the nearest
    // preceding checked part: placed only if it is.
    let mut last_checked: Option<usize> = None;
    for (k, p) in parts.iter().enumerate() {
        let (out, chk, dep) = match p {
            Part::Mass { poly, h } => {
                let fp = frame.poly(poly);
                let (lo, hi) = ctx.ground_range(&fp);
                let base = lo - 0.3;
                let b = Building {
                    footprint: fp.clone(),
                    height_m: h + (hi - base),
                    base_z_m: base,
                };
                roof[k] = b.roof_z();
                (Out::Building(b), Some(fp), None)
            }
            Part::Crown {
                mass,
                poly,
                dz,
                h,
                kind,
                mat,
                solid,
            } => {
                let z = roof[*mass] + dz;
                let f = prism(*kind, *mat, frame.poly(poly), z, z + h, *solid);
                (Out::Feature(f, false), None, Some(*mass))
            }
            Part::RoofCyl {
                mass,
                c,
                dz,
                r,
                h,
                kind,
                mat,
                solid,
            } => {
                let w = frame.at(c[0], c[1]);
                let z = roof[*mass] + dz;
                let f = cylinder(*kind, *mat, [w[0], w[1], z], *r, *h, *solid);
                (Out::Feature(f, false), None, Some(*mass))
            }
            Part::Prism {
                poly,
                z0,
                h,
                kind,
                mat,
                solid,
            } => {
                let fp = frame.poly(poly);
                let (lo, hi) = ctx.ground_range(&fp);
                let base = if *z0 <= 0.01 { lo - 0.3 } else { lo + z0 };
                let f = prism(*kind, *mat, fp.clone(), base, hi + z0 + h, *solid);
                (Out::Feature(f, true), Some(fp), None)
            }
            Part::Box {
                c,
                du,
                dv,
                z0,
                h,
                kind,
                mat,
                solid,
                check,
            } => {
                let w = frame.at(c[0], c[1]);
                let fp = obb(w, [dv * 0.5, du * 0.5], frame.yaw());
                let gz = ctx.ground_z(w);
                let (base, top) = if *z0 <= 0.01 {
                    (ctx.ground_range(&fp).0 - 0.3, gz + h)
                } else {
                    (gz + z0, gz + z0 + h)
                };
                let f = box_on(
                    *kind,
                    *mat,
                    w,
                    base,
                    [*dv, *du, top - base],
                    frame.yaw(),
                    *solid,
                );
                let dep = if *check { None } else { last_checked };
                (Out::Feature(f, *solid), check.then_some(fp), dep)
            }
            Part::Cyl {
                c,
                r,
                z0,
                h,
                kind,
                mat,
                solid,
                check,
            } => {
                let w = frame.at(c[0], c[1]);
                let gz = ctx.ground_z(w);
                let fp = ngon(w, *r, 8);
                let (base, top) = if *z0 <= 0.01 {
                    (ctx.ground_range(&fp).0 - 0.3, gz + h)
                } else {
                    (gz + z0, gz + z0 + h)
                };
                let f = cylinder(*kind, *mat, [w[0], w[1], base], *r, top - base, *solid);
                let dep = if *check { None } else { last_checked };
                (Out::Feature(f, *solid), check.then_some(fp), dep)
            }
            Part::Flat { c, du, dv, mat } => {
                let w = frame.at(c[0], c[1]);
                let fp = obb(w, [dv * 0.5, du * 0.5], frame.yaw());
                let (lo, hi) = ctx.ground_range(&fp);
                if hi - lo > 0.8 {
                    (Out::Skip, None, None)
                } else {
                    let f = box_on(
                        FeatureKind::Ground,
                        *mat,
                        w,
                        lo - 0.1,
                        [*dv, *du, hi - lo + 0.18],
                        frame.yaw(),
                        false,
                    );
                    (Out::Feature(f, true), Some(fp), None)
                }
            }
            Part::Ramp {
                a,
                za,
                b,
                zb,
                w,
                kind,
                mat,
                solid,
            } => {
                let (wa, wb) = (frame.at(a[0], a[1]), frame.at(b[0], b[1]));
                let mut f = super::super::ctx::ramp(
                    *kind,
                    *mat,
                    [wa[0], wa[1], ctx.ground_z(wa) + za],
                    [wb[0], wb[1], ctx.ground_z(wb) + zb],
                    *w,
                    1.0,
                );
                f.solid = *solid;
                (Out::Feature(f, false), None, None)
            }
            Part::Line {
                a,
                b,
                spec,
                gaps,
                gap_m,
            } => {
                let g = gaps.iter().map(|g| frame.at(g[0], g[1])).collect();
                (
                    Out::Line(frame.at(a[0], a[1]), frame.at(b[0], b[1]), *spec, g, *gap_m),
                    None,
                    None,
                )
            }
        };
        if chk.is_some() {
            last_checked = Some(k);
        }
        outs.push((out, chk, dep));
    }
    // Phase 2: checks against the state before this group.
    let ok: Vec<bool> = outs
        .iter()
        .map(|(_, chk, _)| match chk {
            None => true,
            Some(fp) => match mode {
                Mode::Lot => ctx.free(fp, 0.0),
                Mode::Site(site) => {
                    fp.iter().all(|q| signed_dist(site, *q) <= 0.5) && ctx.free_in_site(fp, 0.0)
                }
            },
        })
        .collect();
    let mut ok: Vec<bool> = outs
        .iter()
        .enumerate()
        .map(|(k, (_, _, dep))| ok[k] && dep.is_none_or(|m| ok[m]))
        .collect();
    if matches!(mode, Mode::Lot) && ok.iter().any(|o| !o) {
        return false;
    }
    // Lines in a site are checked piece by piece against fewer classes.
    let line_blocked: &[Occ] = match mode {
        Mode::Lot => &LOT_LINE_BLOCKED,
        Mode::Site(_) => &SITE_LINE_BLOCKED,
    };
    let mut any = false;
    for (k, (out, _, _)) in outs.into_iter().enumerate() {
        if !ok[k] {
            continue;
        }
        match out {
            Out::Building(b) => {
                ctx.add_building(b);
                any = true;
            }
            Out::Feature(f, mark) => {
                if mark {
                    ctx.place(f);
                } else {
                    ctx.add(f);
                }
                any = true;
            }
            Out::Line(a, b, spec, gaps, gap_m) => {
                emit_line(ctx, a, b, spec, &gaps, gap_m, line_blocked);
            }
            Out::Skip => ok[k] = false,
        }
    }
    any
}

/// True when every checked part of `parts` (masses, prisms, checked boxes / cylinders,
/// slabs) would be clear in `frame` under `mode` (nothing is placed).
pub fn all_free(ctx: &Ctx, frame: &Frame, parts: &[Part], mode: Mode) -> bool {
    parts.iter().all(|p| {
        let fp = match p {
            Part::Mass { poly, .. } | Part::Prism { poly, .. } => frame.poly(poly),
            Part::Box {
                c,
                du,
                dv,
                check: true,
                ..
            }
            | Part::Flat { c, du, dv, .. } => {
                obb(frame.at(c[0], c[1]), [dv * 0.5, du * 0.5], frame.yaw())
            }
            Part::Cyl {
                c, r, check: true, ..
            } => ngon(frame.at(c[0], c[1]), *r, 8),
            _ => return true,
        };
        match mode {
            Mode::Lot => ctx.free(&fp, 0.0),
            Mode::Site(site) => {
                fp.iter().all(|q| signed_dist(site, *q) <= 0.5) && ctx.free_in_site(&fp, 0.0)
            }
        }
    })
}

/// Classes a lot boundary piece must stay clear of.
pub const LOT_LINE_BLOCKED: [Occ; 6] = [
    Occ::Site,
    Occ::Water,
    Occ::Feature,
    Occ::Building,
    Occ::Verge,
    Occ::Road,
];
/// Classes a site perimeter / internal fence piece must stay clear of.
pub const SITE_LINE_BLOCKED: [Occ; 4] = [Occ::Water, Occ::Building, Occ::Verge, Occ::Road];

/// Longest single boundary piece, m.
const MAX_PIECE_M: f32 = 80.0;
/// Most ground rise along one boundary piece (taller pieces step down a slope), m.
const MAX_PIECE_RISE_M: f32 = 3.0;

/// Emit a boundary line `a → b` as ground-following boxes (≤ 80 m, ≤ 3 m of ground rise
/// each), leaving openings of half width `gap_m` around each point of `gaps` near the line;
/// pieces whose (end-trimmed) footprint is within 0.35 m of a `blocked` cell are skipped.
/// Returns the number of pieces placed.
pub fn emit_line(
    ctx: &mut Ctx,
    a: V2,
    b: V2,
    spec: FenceSpec,
    gaps: &[V2],
    gap_m: f32,
    blocked: &[Occ],
) -> usize {
    let len = geom::dist(a, b);
    if len < 1.0 {
        return 0;
    }
    let d = geom::scale(geom::sub(b, a), 1.0 / len);
    // Open intervals along the line.
    let mut spans: Vec<(f32, f32)> = vec![(0.0, len)];
    for g in gaps {
        if seg_dist(a, b, *g) > 2.0 {
            continue;
        }
        let s = geom::dot(geom::sub(*g, a), d);
        let (c0, c1) = (s - gap_m, s + gap_m);
        let mut next = Vec::new();
        for (s0, s1) in spans {
            if c1 <= s0 || c0 >= s1 {
                next.push((s0, s1));
                continue;
            }
            if c0 > s0 {
                next.push((s0, c0));
            }
            if c1 < s1 {
                next.push((c1, s1));
            }
        }
        spans = next;
    }
    let yaw = super::super::ctx::yaw_of(a, b);
    let hard: Vec<Occ> = blocked
        .iter()
        .copied()
        .filter(|o| matches!(o, Occ::Road | Occ::Verge | Occ::Water | Occ::Site))
        .collect();
    let mut placed = 0;
    for (s0, s1) in spans {
        let span = s1 - s0;
        if span < 0.8 {
            continue;
        }
        let p0 = geom::add(a, geom::scale(d, s0));
        let p1 = geom::add(a, geom::scale(d, s1));
        let rise = (ctx.ground_z(p0) - ctx.ground_z(p1)).abs();
        let n = (span / MAX_PIECE_M)
            .ceil()
            .max((rise / MAX_PIECE_RISE_M).ceil())
            .clamp(1.0, 64.0) as usize;
        for k in 0..n {
            let q0 = super::super::ctx::lerp2(p0, p1, k as f32 / n as f32);
            let q1 = super::super::ctx::lerp2(p0, p1, (k + 1) as f32 / n as f32);
            placed += piece(ctx, q0, q1, yaw, spec, blocked, &hard, 3);
        }
    }
    placed
}

/// One boundary piece `q0 → q1`: the whole piece must be clear of `hard` classes, its
/// end-trimmed middle clear of `blocked` (so it may touch buildings at its ends). A blocked
/// piece is split in two (up to `depth` times) so only the blocked part is left out.
#[allow(clippy::too_many_arguments)]
fn piece(
    ctx: &mut Ctx,
    q0: V2,
    q1: V2,
    yaw: f32,
    spec: FenceSpec,
    blocked: &[Occ],
    hard: &[Occ],
    depth: u32,
) -> usize {
    let m = super::super::ctx::lerp2(q0, q1, 0.5);
    let pl = geom::dist(q0, q1);
    let full = obb(m, [spec.thick * 0.5, pl * 0.5], yaw);
    let trimmed = obb(m, [spec.thick * 0.5, (pl * 0.5 - 0.8).max(0.1)], yaw);
    if !(ctx.free_of(&full, 0.35, hard) && ctx.free_of(&trimmed, 0.35, blocked)) {
        if depth == 0 || pl < 6.0 {
            return 0;
        }
        return piece(ctx, q0, m, yaw, spec, blocked, hard, depth - 1)
            + piece(ctx, m, q1, yaw, spec, blocked, hard, depth - 1);
    }
    let (z0, z1, zm) = (ctx.ground_z(q0), ctx.ground_z(q1), ctx.ground_z(m));
    let z = z0.min(z1).min(zm) - 0.3;
    let top = z0.max(z1).max(zm) + spec.height;
    ctx.place(box_on(
        spec.kind,
        spec.mat,
        m,
        z,
        [spec.thick, pl, top - z],
        yaw,
        true,
    ));
    1
}

/// True when a closed polygon is convex and counter-clockwise (collinear edges allowed).
pub fn convex_ccw(poly: &[V2]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut area = 0.0;
    for i in 0..n {
        let (a, b, c) = (poly[i], poly[(i + 1) % n], poly[(i + 2) % n]);
        let cr = geom::cross(geom::sub(b, a), geom::sub(c, b));
        let scale = geom::dist(a, b) * geom::dist(b, c);
        if cr < -1e-4 * scale.max(1e-6) {
            return false;
        }
        area += a[0] * b[1] - b[0] * a[1];
    }
    area > 0.0
}

/// True when `p` is inside `poly` or within `tol` of it.
pub fn near_inside(poly: &[V2], p: V2, tol: f32) -> bool {
    inside(poly, p) || signed_dist(poly, p) <= tol
}

/// Footprint of a feature shape (re-exported for tests).
pub fn shape_footprint(s: &FeatureShape) -> Vec<V2> {
    super::super::ctx::footprint(s)
}
