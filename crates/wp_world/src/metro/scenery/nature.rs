//! Nature: trees, the plan's parks (square parks, the promenade, the beach), forest on steep
//! ground and in the forest districts, and sparse trees on the remaining open land.

use super::bounds::{band_ok, ground_piece, lamp, Style};
use super::contour::{self, lerp, Grid};
use super::{park_sites, thin, Env, Occ};
use crate::geom::{self, V2};
use crate::metro::ctx::{box_on, cylinder, obb, prism, yaw_of};
use crate::metro::ground::{bounds, inside, seg_dist};
use crate::metro::plan::{Site, MAP_HI, MAP_LO, WATER_Z};
use crate::network::{DistrictKind as D, Feature, FeatureKind as K, FeatureShape, Material as M};
use crate::terrain::ValueNoise;

/// Forest trees (interior + edge) budget.
const CAP_FOREST_TREES: usize = 6500;
/// Undergrowth pieces along forest edges budget.
const CAP_UNDERGROWTH: usize = 8000;
/// Forest trees more than this many 12 m cells inside the forest edge are not solid.
const FOREST_SOLID_DEPTH: usize = 3;
/// Scattered trees on open land budget.
const CAP_SCATTERED: usize = 2500;
/// Forest mask cell, m.
const FOREST_CELL_M: f32 = 12.0;
/// Park mask cell, m.
const PARK_CELL_M: f32 = 4.0;

/// Tree shapes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TreeKind {
    /// Street tree: slim trunk, canopy high enough to overhang the kerb lane.
    Street,
    /// Broadleaf park / forest-edge tree.
    Broad,
    /// Conifer: short trunk, tall narrow canopy.
    Conifer,
    /// Deep-forest tree drawn as one solid foliage column (one feature instead of two).
    Mass,
}

/// A tree at `p` on ground `z`; `h` in `0..1` varies its size.
pub(super) fn tree(p: V2, z: f32, kind: TreeKind, h: f32) -> Vec<Feature> {
    let h2 = (h * 7.31 + 0.17).fract();
    let base = [p[0], p[1], z - 0.2];
    let trunk = |r: f32, th: f32| cylinder(K::TreeTrunk, M::Wood, base, r, th + 0.2, true);
    let canopy = |r: f32, z0: f32, ch: f32| {
        cylinder(K::TreeCanopy, M::Foliage, [p[0], p[1], z0], r, ch, false)
    };
    match kind {
        TreeKind::Street => {
            let th = 4.0 + 1.0 * h2;
            vec![
                trunk(0.3 + 0.1 * h, th),
                canopy(2.0 + 1.2 * h, z + th - 0.5, 3.0 + 2.0 * h2),
            ]
        }
        TreeKind::Broad => {
            let th = 3.0 + 2.0 * h2;
            vec![
                trunk(0.35 + 0.2 * h, th),
                canopy(2.5 + 2.0 * h, z + th - 0.8, 4.0 + 3.0 * h2),
            ]
        }
        TreeKind::Conifer => {
            let th = 1.5 + 1.0 * h2;
            vec![
                trunk(0.3 + 0.1 * h, th),
                canopy(1.4 + 0.8 * h, z + th, 7.0 + 6.0 * h2),
            ]
        }
        TreeKind::Mass => vec![cylinder(
            K::TreeTrunk,
            M::Foliage,
            base,
            1.6 + 1.0 * h,
            9.0 + 6.0 * h2,
            true,
        )],
    }
}

/// A regular-polygon footprint.
fn ngon(c: V2, r: f32, n: usize) -> Vec<V2> {
    (0..n)
        .map(|k| {
            let (s, co) = wp_math::sin_cos(k as f32 * core::f32::consts::TAU / n as f32);
            [c[0] + r * co, c[1] + r * s]
        })
        .collect()
}

// ---------------------------------------------------------------------------------------
// Parks
// ---------------------------------------------------------------------------------------

/// Usable-cell mask of a park (vertices on a 4 m grid around the site's bounds, one empty
/// ring outside so its outline closes).
struct ParkMask {
    lo: V2,
    nx: usize,
    ny: usize,
    v: Vec<f32>,
}

impl ParkMask {
    fn new(env: &Env, site: &Site) -> Self {
        let (a, b) = bounds(&site.polygon);
        let c = PARK_CELL_M;
        let lo = [a[0] - c, a[1] - c];
        let nx = ((b[0] - a[0]) / c).ceil() as usize + 3;
        let ny = ((b[1] - a[1]) / c).ceil() as usize + 3;
        let mut v = vec![0.0; nx * ny];
        for j in 0..ny {
            for i in 0..nx {
                let p = [lo[0] + i as f32 * c, lo[1] + j as f32 * c];
                let ok = inside(&site.polygon, p)
                    && [[0.0, 0.0], [1.2, 0.0], [-1.2, 0.0], [0.0, 1.2], [0.0, -1.2]]
                        .iter()
                        .all(|d| {
                            let q = [p[0] + d[0], p[1] + d[1]];
                            matches!(env.occ(q), Occ::Site | Occ::Free)
                                && !env.ctx.is_water(q)
                                && !env.foreign_site(q)
                        });
                if ok {
                    v[j * nx + i] = 1.0;
                }
            }
        }
        Self { lo, nx, ny, v }
    }

    /// Nearest mask vertex is usable.
    fn ok(&self, p: V2) -> bool {
        let i = ((p[0] - self.lo[0]) / PARK_CELL_M).round();
        let j = ((p[1] - self.lo[1]) / PARK_CELL_M).round();
        if i < 0.0 || j < 0.0 || i as usize >= self.nx || j as usize >= self.ny {
            return false;
        }
        self.v[j as usize * self.nx + i as usize] > 0.5
    }

    /// Usable within `r` m (centre and 8 points around).
    fn ok_r(&self, p: V2, r: f32) -> bool {
        self.ok(p) && ngon(p, r, 8).iter().all(|q| self.ok(*q))
    }

    fn grid(&self) -> Grid<'_> {
        Grid {
            nx: self.nx,
            ny: self.ny,
            origin: self.lo,
            cell: PARK_CELL_M,
            v: &self.v,
        }
    }
}

/// Park content for every `Park` site except the state forest (that one is forest).
pub(super) fn parks(env: &Env, out: &mut Vec<Feature>) {
    for site in park_sites(env.plan()) {
        if site.id == "westridge_forest" {
            continue;
        }
        let mask = ParkMask::new(env, site);
        let (a, b) = bounds(&site.polygon);
        let (w, h) = (b[0] - a[0], b[1] - a[1]);
        let beach = site.id == "south_shore_park";
        let promenade = !beach && w.max(h) > 2.5 * w.min(h);
        perimeter(env, &mask, beach, promenade, out);
        if beach {
            beach_park(env, site, &mask, out);
        } else if promenade {
            promenade_park(env, site, &mask, out);
        } else {
            square_park(env, site, &mask, out);
        }
    }
}

/// Hedge (fence on the beach, low wall on the promenade) round the usable part of a park,
/// with 1.6 m pedestrian gaps; never along water (the bank has its own barrier).
fn perimeter(env: &Env, mask: &ParkMask, beach: bool, promenade: bool, out: &mut Vec<Feature>) {
    let st = if beach {
        Style {
            kind: K::Fence,
            mat: M::Wood,
            thick: 0.2,
            height: 1.1,
        }
    } else if promenade {
        Style {
            kind: K::Wall,
            mat: M::Concrete,
            thick: 0.5,
            height: 0.7,
        }
    } else {
        Style {
            kind: K::Hedge,
            mat: M::Foliage,
            thick: 1.2,
            height: 1.3,
        }
    };
    let period = if promenade { 25.0 } else { 40.0 };
    for line in contour::contours(&mask.grid(), 0.5) {
        let mut s0 = 0.0;
        for (a, b) in contour::pieces(&line, 0.6, 10.0) {
            let len = geom::dist(a, b);
            let d = geom::normalize(geom::sub(b, a));
            let outward = geom::right_of(d);
            let m = lerp(a, b, 0.5);
            let wet = [2.0, 4.0, 7.0]
                .iter()
                .any(|o| env.ctx.is_water(geom::add(m, geom::scale(outward, *o))));
            let start = s0;
            s0 += len;
            if wet {
                continue;
            }
            let inward = [-outward[0], -outward[1]];
            let off = st.thick * 0.5 + 0.6;
            let (a2, b2) = (
                geom::add(a, geom::scale(inward, off)),
                geom::add(b, geom::scale(inward, off)),
            );
            // Pedestrian gaps every `period` m of outline.
            let ok = |p: V2| {
                let s = start + geom::dist(a2, p);
                s % period > 1.6 && band_ok(env, p, inward, st.thick * 0.5)
            };
            for (p, q) in contour::runs(a2, b2, 0.4, 1.0, ok) {
                out.push(ground_piece(env, st, p, q));
            }
        }
    }
}

/// Thin flat ground tiles (lawn, paving, sand) over fully usable `block`-sized squares;
/// sand tiles are pitched to follow the slope.
fn tiles(env: &Env, mask: &ParkMask, block: f32, mat: M, pitched: bool, out: &mut Vec<Feature>) {
    let (lo, n_i, n_j) = (
        mask.lo,
        (mask.nx as f32 * PARK_CELL_M / block) as usize + 1,
        (mask.ny as f32 * PARK_CELL_M / block) as usize + 1,
    );
    for j in 0..n_j {
        for i in 0..n_i {
            let c = [
                lo[0] + (i as f32 + 0.5) * block,
                lo[1] + (j as f32 + 0.5) * block,
            ];
            let hb = block * 0.5;
            let pts = [
                c,
                [c[0] - hb, c[1] - hb],
                [c[0] + hb, c[1] - hb],
                [c[0] + hb, c[1] + hb],
                [c[0] - hb, c[1] + hb],
            ];
            if !pts.iter().all(|p| mask.ok(*p)) {
                continue;
            }
            if pitched {
                let gx = (env.gz([c[0] + hb, c[1]]) - env.gz([c[0] - hb, c[1]])) / block;
                let gy = (env.gz([c[0], c[1] + hb]) - env.gz([c[0], c[1] - hb])) / block;
                let g = (gx * gx + gy * gy).sqrt();
                let dir = if g > 1e-4 {
                    [gx / g, gy / g]
                } else {
                    [0.0, 1.0]
                };
                let pitch = wp_math::atan2(g, 1.0);
                out.push(Feature {
                    kind: K::Ground,
                    material: mat,
                    shape: FeatureShape::Box {
                        centre: [c[0], c[1], env.gz(c) + 0.03],
                        half: [hb, hb, 0.04],
                        yaw: yaw_of([0.0, 0.0], dir),
                        pitch,
                    },
                    solid: false,
                });
            } else {
                let z = pts.iter().map(|p| env.gz(*p)).fold(f32::MIN, f32::max);
                out.push(box_on(
                    K::Ground,
                    mat,
                    c,
                    z,
                    [block, block, 0.06],
                    0.0,
                    false,
                ));
            }
        }
    }
}

/// Path pieces along a polyline (3 m wide gravel / paving), where the mask allows.
fn path(env: &Env, mask: &ParkMask, line: &[V2], mat: M, width: f32, out: &mut Vec<Feature>) {
    for w in line.windows(2) {
        let len = geom::dist(w[0], w[1]);
        let n = (len / 10.0).ceil().max(1.0) as usize;
        for k in 0..n {
            let a = lerp(w[0], w[1], k as f32 / n as f32);
            let b = lerp(w[0], w[1], (k + 1) as f32 / n as f32);
            let m = lerp(a, b, 0.5);
            if !(mask.ok(a) && mask.ok(b) && mask.ok(m)) {
                continue;
            }
            let z = [a, b, m]
                .iter()
                .map(|p| env.gz(*p))
                .fold(f32::MIN, f32::max);
            out.push(box_on(
                K::Ground,
                mat,
                m,
                z + 0.02,
                [width, geom::dist(a, b) + 0.5, 0.06],
                yaw_of(a, b),
                false,
            ));
        }
    }
}

/// Distance from `p` to the nearest of several polylines.
fn dist_to_lines(lines: &[Vec<V2>], p: V2) -> f32 {
    lines
        .iter()
        .flat_map(|l| l.windows(2).map(move |w| seg_dist(w[0], w[1], p)))
        .fold(f32::MAX, f32::min)
}

/// Things placed along a polyline every `step` m at lateral offset `off` (left positive).
fn along(line: &[V2], step: f32, off: f32) -> Vec<(V2, V2)> {
    let mut out = Vec::new();
    let mut next = step * 0.5;
    let mut s = 0.0;
    for w in line.windows(2) {
        let len = geom::dist(w[0], w[1]);
        if len < 1e-3 {
            continue;
        }
        let d = geom::normalize(geom::sub(w[1], w[0]));
        let left = [-d[1], d[0]];
        while next <= s + len {
            let p = lerp(w[0], w[1], (next - s) / len);
            out.push((geom::add(p, geom::scale(left, off)), d));
            next += step;
        }
        s += len;
    }
    out
}

/// Park trees on a jittered grid, avoiding paths and the given keep-out discs.
#[allow(clippy::too_many_arguments)]
fn park_trees(
    env: &Env,
    site: &Site,
    mask: &ParkMask,
    paths: &[Vec<V2>],
    keep: &[(V2, f32)],
    spacing: f32,
    prob: f32,
    out: &mut Vec<Feature>,
) {
    let (a, b) = bounds(&site.polygon);
    let tag = site.id.len() as u64 * 977 + site.id.as_bytes()[0] as u64;
    let nx = ((b[0] - a[0]) / spacing).ceil() as i64;
    let ny = ((b[1] - a[1]) / spacing).ceil() as i64;
    for j in 0..ny {
        for i in 0..nx {
            let r0 = env.h01(i, j, tag);
            let r1 = env.h01(i, j, tag + 1);
            let r2 = env.h01(i, j, tag + 2);
            let p = [
                a[0] + (i as f32 + 0.15 + 0.7 * r1) * spacing,
                a[1] + (j as f32 + 0.15 + 0.7 * r2) * spacing,
            ];
            if !mask.ok_r(p, 3.0)
                || dist_to_lines(paths, p) < 4.5
                || keep.iter().any(|(c, r)| geom::dist(*c, p) < *r)
            {
                continue;
            }
            let z = env.gz(p);
            if r0 < prob {
                let kind = if r0 < prob * 0.3 {
                    TreeKind::Conifer
                } else {
                    TreeKind::Broad
                };
                out.extend(tree(p, z, kind, r1));
            } else if r0 < prob + 0.12 {
                out.push(cylinder(
                    K::Bush,
                    M::Foliage,
                    [p[0], p[1], z - 0.1],
                    0.8 + 0.5 * r2,
                    1.3,
                    true,
                ));
            }
        }
    }
}

fn bench(p: V2, z: f32, d: V2) -> Feature {
    box_on(
        K::StreetFurniture,
        M::Wood,
        p,
        z,
        [0.5, 1.8, 0.5],
        yaw_of([0.0, 0.0], d),
        true,
    )
}

/// Square park: lawn, a ring path with cross and diagonal paths, a bandstand or a
/// monument in the middle, a playground, trees, bushes, benches and lamps.
fn square_park(env: &Env, site: &Site, mask: &ParkMask, out: &mut Vec<Feature>) {
    let (a, b) = bounds(&site.polygon);
    let c = lerp(a, b, 0.5);
    let (w, h) = (b[0] - a[0], b[1] - a[1]);
    tiles(env, mask, 12.0, M::Grass, false, out);
    let i = 18.0;
    let ring = vec![
        [a[0] + i, a[1] + i],
        [b[0] - i, a[1] + i],
        [b[0] - i, b[1] - i],
        [a[0] + i, b[1] - i],
        [a[0] + i, a[1] + i],
    ];
    let mut paths = vec![ring.clone()];
    for t in [
        [c[0], a[1] + 2.0],
        [c[0], b[1] - 2.0],
        [a[0] + 2.0, c[1]],
        [b[0] - 2.0, c[1]],
    ] {
        paths.push(vec![c, t]);
    }
    for corner in ring.iter().take(4) {
        paths.push(vec![c, *corner]);
    }
    for p in &paths {
        path(env, mask, p, M::Gravel, 3.0, out);
    }
    // Centrepiece.
    let zc = env.gz(c);
    let mut keep = vec![(c, 14.0)];
    if mask.ok_r(c, 8.0) {
        if w * h > 40_000.0 {
            // Bandstand: octagonal stage, four posts, roof.
            out.push(prism(
                K::Monument,
                M::Wood,
                ngon(c, 5.5, 8),
                zc - 0.3,
                zc + 0.7,
                true,
            ));
            for q in ngon(c, 4.8, 4) {
                out.push(cylinder(
                    K::Monument,
                    M::White,
                    [q[0], q[1], zc + 0.7],
                    0.15,
                    3.0,
                    true,
                ));
            }
            out.push(prism(
                K::Monument,
                M::DarkConcrete,
                ngon(c, 6.2, 8),
                zc + 3.7,
                zc + 4.2,
                false,
            ));
        } else {
            // Fountain with a statue on a plinth.
            out.push(cylinder(
                K::Monument,
                M::Concrete,
                [c[0], c[1], zc - 0.3],
                4.5,
                0.9,
                true,
            ));
            out.push(cylinder(
                K::Monument,
                M::Glass,
                [c[0], c[1], zc + 0.55],
                4.1,
                0.05,
                false,
            ));
            out.push(box_on(
                K::Monument,
                M::Concrete,
                c,
                zc + 0.6,
                [2.2, 2.2, 1.4],
                0.0,
                true,
            ));
            out.push(box_on(
                K::Monument,
                M::Steel,
                c,
                zc + 2.0,
                [0.8, 0.8, 2.4],
                0.0,
                true,
            ));
        }
    }
    // Playground in one quadrant.
    let pg = [c[0] - 0.3 * w, c[1] + 0.1 * h];
    if mask.ok_r(pg, 10.0) && dist_to_lines(&paths, pg) > 9.0 {
        keep.push((pg, 13.0));
        let z = env.gz(pg);
        out.push(box_on(
            K::Ground,
            M::Sand,
            pg,
            z + 0.02,
            [18.0, 14.0, 0.06],
            0.0,
            false,
        ));
        out.push(box_on(
            K::StreetFurniture,
            M::Wood,
            [pg[0] - 5.0, pg[1] + 2.5],
            z,
            [3.0, 3.0, 2.2],
            0.0,
            true,
        ));
        out.push(box_on(
            K::StreetFurniture,
            M::SafetyYellow,
            [pg[0] - 5.0, pg[1] - 1.0],
            z,
            [0.8, 3.0, 1.2],
            0.0,
            false,
        ));
        for dx in [-2.0f32, 2.0] {
            out.push(cylinder(
                K::StreetFurniture,
                M::Steel,
                [pg[0] + 3.5 + dx, pg[1] + 3.0, z],
                0.08,
                2.4,
                true,
            ));
        }
        out.push(box_on(
            K::StreetFurniture,
            M::Steel,
            [pg[0] + 3.5, pg[1] + 3.0],
            z + 2.3,
            [4.2, 0.12, 0.12],
            0.0,
            false,
        ));
        out.push(box_on(
            K::StreetFurniture,
            M::Red,
            [pg[0] + 3.5, pg[1] - 3.0],
            z,
            [0.3, 3.6, 0.6],
            0.0,
            true,
        ));
    }
    // Benches inside the ring path, lamps outside it.
    for (p, d) in along(&ring, 25.0, 2.6) {
        if mask.ok(p) && keep.iter().all(|(k, r)| geom::dist(*k, p) > *r) {
            out.push(bench(p, env.gz(p), d));
        }
    }
    for (p, d) in along(&ring, 35.0, -2.4) {
        if mask.ok(p) {
            out.extend(lamp(p, env.gz(p), 4.5, [d[1], -d[0]], 0.6));
        }
    }
    park_trees(env, site, mask, &paths, &keep, 13.0, 0.55, out);
}

/// Promenade (long thin park): paving, two tree rows, short lamps along the middle,
/// benches between the trees, a lantern column in the middle.
fn promenade_park(env: &Env, site: &Site, mask: &ParkMask, out: &mut Vec<Feature>) {
    let (a, b) = bounds(&site.polygon);
    let c = lerp(a, b, 0.5);
    let along_x = b[0] - a[0] >= b[1] - a[1];
    let (u, v) = if along_x {
        ([1.0, 0.0], [0.0, 1.0])
    } else {
        ([0.0, 1.0], [1.0, 0.0])
    };
    let (long, short) = if along_x {
        (b[0] - a[0], b[1] - a[1])
    } else {
        (b[1] - a[1], b[0] - a[0])
    };
    tiles(env, mask, 8.0, M::Concrete, false, out);
    let at = |s: f32, t: f32| geom::add(c, geom::add(geom::scale(u, s), geom::scale(v, t)));
    let mut s = -long * 0.5 + 6.0;
    let mut k = 0;
    while s < long * 0.5 - 6.0 {
        for row in [-0.25, 0.25] {
            let p = at(s, row * short);
            if mask.ok_r(p, 2.0) && geom::dist(p, c) > 6.0 {
                let h = env.h01(k, (row * 8.0) as i64, 31);
                out.extend(tree(p, env.gz(p), TreeKind::Broad, h * 0.6));
            }
            let q = at(s + 5.0, row * short);
            if k % 2 == 0 && mask.ok(q) && geom::dist(q, c) > 6.0 {
                out.push(bench(q, env.gz(q), u));
            }
        }
        if k % 3 == 1 {
            let p = at(s + 2.5, 0.0);
            if mask.ok(p) && geom::dist(p, c) > 5.0 {
                out.extend(lamp(p, env.gz(p), 4.2, v, 0.4));
            }
        }
        s += 10.0;
        k += 1;
    }
    if mask.ok_r(c, 2.0) {
        let z = env.gz(c);
        out.push(box_on(
            K::Monument,
            M::DarkConcrete,
            c,
            z,
            [2.4, 2.4, 1.0],
            0.0,
            true,
        ));
        out.push(cylinder(
            K::Monument,
            M::Steel,
            [c[0], c[1], z + 1.0],
            0.35,
            5.0,
            true,
        ));
        out.push(box_on(
            K::Monument,
            M::Light,
            c,
            z + 6.0,
            [0.9, 0.9, 0.9],
            0.0,
            false,
        ));
    }
}

/// Beach: sand over the usable ground above the water line, a boardwalk along the landward
/// side with trees, benches and lamps, lifeguard towers every ~300 m near the water.
fn beach_park(env: &Env, site: &Site, mask: &ParkMask, out: &mut Vec<Feature>) {
    let (a, b) = bounds(&site.polygon);
    let along_x = b[0] - a[0] >= b[1] - a[1];
    // Landward side: the long side whose middle is higher.
    let (lo_mid, hi_mid) = if along_x {
        (
            [0.5 * (a[0] + b[0]), a[1] + 4.0],
            [0.5 * (a[0] + b[0]), b[1] - 4.0],
        )
    } else {
        (
            [a[0] + 4.0, 0.5 * (a[1] + b[1])],
            [b[0] - 4.0, 0.5 * (a[1] + b[1])],
        )
    };
    let land_hi = env.gz(hi_mid) >= env.gz(lo_mid);
    tiles(env, mask, 16.0, M::Sand, true, out);
    let (u, inland) = match (along_x, land_hi) {
        (true, true) => ([1.0, 0.0], [0.0, 1.0]),
        (true, false) => ([1.0, 0.0], [0.0, -1.0]),
        (false, true) => ([0.0, 1.0], [1.0, 0.0]),
        (false, false) => ([0.0, 1.0], [-1.0, 0.0]),
    };
    let edge = if land_hi { hi_mid } else { lo_mid };
    let long = if along_x { b[0] - a[0] } else { b[1] - a[1] };
    let walk_c = geom::add(edge, geom::scale(inland, -6.0));
    let at = |s: f32, t: f32| {
        geom::add(
            walk_c,
            geom::add(geom::scale(u, s), geom::scale(inland, -t)),
        )
    };
    let mut s = -long * 0.5 + 8.0;
    let mut k: i64 = 0;
    while s < long * 0.5 - 8.0 {
        let p = at(s, 0.0);
        let q = at(s + 12.0, 0.0);
        if mask.ok(p) && mask.ok(q) {
            let z = env.gz(p).max(env.gz(q)).max(env.gz(lerp(p, q, 0.5)));
            out.push(box_on(
                K::Ground,
                M::Wood,
                lerp(p, q, 0.5),
                z + 0.15,
                [4.0, 12.2, 0.2],
                yaw_of(p, q),
                false,
            ));
        }
        if k % 3 == 0 {
            let t = at(s + 6.0, -4.5);
            if mask.ok_r(t, 2.0) {
                out.extend(tree(t, env.gz(t), TreeKind::Broad, env.h01(k, 0, 41)));
            }
        }
        if k % 4 == 1 {
            let bp = at(s + 6.0, -2.6);
            if mask.ok(bp) {
                out.push(bench(bp, env.gz(bp), u));
            }
        }
        if k % 4 == 3 {
            let lp = at(s + 6.0, 2.6);
            if mask.ok(lp) {
                out.extend(lamp(lp, env.gz(lp), 4.5, inland, 0.5));
            }
        }
        // Lifeguard tower every 300 m, where the sand is ~1.4 m above the water.
        if k % 25 == 12 {
            let mut t = 6.0;
            while t < 120.0 {
                let p = at(s, t);
                if !mask.ok(p) {
                    break;
                }
                if env.gz(p) < WATER_Z + 1.4 {
                    lifeguard(env, p, inland, out);
                    break;
                }
                t += 2.0;
            }
        }
        s += 12.0;
        k += 1;
    }
}

fn lifeguard(env: &Env, p: V2, facing: V2, out: &mut Vec<Feature>) {
    let z = env.gz(p);
    let yaw = yaw_of([0.0, 0.0], facing);
    for q in obb(p, [1.0, 1.0], yaw) {
        out.push(cylinder(
            K::StreetFurniture,
            M::Wood,
            [q[0], q[1], z - 0.3],
            0.1,
            2.6,
            true,
        ));
    }
    out.push(box_on(
        K::StreetFurniture,
        M::White,
        p,
        z + 2.3,
        [2.4, 2.4, 1.8],
        yaw,
        false,
    ));
    out.push(box_on(
        K::StreetFurniture,
        M::Red,
        p,
        z + 4.1,
        [2.8, 2.8, 0.2],
        yaw,
        false,
    ));
}

// ---------------------------------------------------------------------------------------
// Forest
// ---------------------------------------------------------------------------------------

/// Forest mask on a 12 m grid over the map (vertex `(i, j)` at the centre of cell `(i, j)`).
pub(super) struct ForestMask {
    nx: usize,
    ny: usize,
    v: Vec<f32>,
    clearing: ValueNoise,
}

impl ForestMask {
    fn centre(&self, i: usize, j: usize) -> V2 {
        [
            MAP_LO[0] + (i as f32 + 0.5) * FOREST_CELL_M,
            MAP_LO[1] + (j as f32 + 0.5) * FOREST_CELL_M,
        ]
    }

    /// The forest covers `p`.
    pub(super) fn at(&self, p: V2) -> bool {
        let i = ((p[0] - MAP_LO[0]) / FOREST_CELL_M).floor();
        let j = ((p[1] - MAP_LO[1]) / FOREST_CELL_M).floor();
        if i < 0.0 || j < 0.0 || i as usize >= self.nx || j as usize >= self.ny {
            return false;
        }
        self.v[j as usize * self.nx + i as usize] > 0.5
    }
}

/// Which ground is forest: the state forest, clusters in the forest districts (Westridge
/// Woods, North Hollins) and housing / suburb ground steeper than 8°, where the whole 12 m
/// cell and a 2 m ring round it are free (or inside the state forest).
pub(super) fn forest_mask(env: &Env) -> ForestMask {
    let nx = ((MAP_HI[0] - MAP_LO[0]) / FOREST_CELL_M).ceil() as usize;
    let ny = ((MAP_HI[1] - MAP_LO[1]) / FOREST_CELL_M).ceil() as usize;
    let mut rng = wp_math::Rng::new(env.seed ^ 0xF0_4E57);
    let cluster = ValueNoise::new(&mut rng, MAP_LO, MAP_HI, 260.0);
    let clearing = ValueNoise::new(&mut rng, MAP_LO, MAP_HI, 90.0);
    let state: Vec<&Site> = park_sites(env.plan())
        .filter(|s| s.id == "westridge_forest")
        .collect();
    let in_state = |p: V2| state.iter().any(|s| inside(&s.polygon, p));
    let mut v = vec![0.0; nx * ny];
    let mut m = ForestMask {
        nx,
        ny,
        v: Vec::new(),
        clearing,
    };
    for j in 0..ny {
        for i in 0..nx {
            let c = m.centre(i, j);
            let st = in_state(c);
            let want = st
                || match env.zone(c) {
                    Some((_, "Westridge Woods" | "North Hollins")) => cluster.at(c) > -0.25,
                    Some((D::Residential | D::Suburb, _)) => {
                        let n = env.ctx.terrain.normal_at(c);
                        n[2] < 0.990 // steeper than ~8°
                    }
                    _ => false,
                };
            if !want {
                continue;
            }
            let mut ok = true;
            'cells: for dj in -4..=4 {
                for di in -4..=4 {
                    let q = [c[0] + di as f32 * 2.0, c[1] + dj as f32 * 2.0];
                    let o = env.occ(q);
                    let fine = match o {
                        Occ::Free => true,
                        Occ::Site => st && in_state(q),
                        _ => false,
                    };
                    // (Free / Site cells are never water: water outranks both.)
                    if !fine {
                        ok = false;
                        break 'cells;
                    }
                }
            }
            if ok {
                v[j * nx + i] = 1.0;
            }
        }
    }
    m.v = v;
    m
}

/// Forest trees (full trees along the edge, single foliage columns inside, clearings) and
/// dense undergrowth along the forest edge so cars cannot drive into it.
pub(super) fn forest(env: &Env, fm: &ForestMask, out: &mut Vec<Feature>) {
    let n_cells = fm.v.iter().filter(|x| **x > 0.5).count();
    if n_cells == 0 {
        return;
    }
    let p_sub = (CAP_FOREST_TREES as f32 / (4.0 * n_cells as f32)).min(0.5);
    let mut trees: Vec<Vec<Feature>> = Vec::new();
    for j in 0..fm.ny {
        for i in 0..fm.nx {
            if fm.v[j * fm.nx + i] < 0.5 {
                continue;
            }
            let edge = [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)]
                .iter()
                .any(|(di, dj)| {
                    let (a, b) = (i as i64 + di, j as i64 + dj);
                    a < 0
                        || b < 0
                        || a >= fm.nx as i64
                        || b >= fm.ny as i64
                        || fm.v[b as usize * fm.nx + a as usize] < 0.5
                });
            // Far inside (every cell within FOREST_SOLID_DEPTH all round is forest): the
            // solid undergrowth along the edge keeps cars out, so these trees are visual only.
            let deep = !edge && {
                let r = FOREST_SOLID_DEPTH as i64;
                (-r..=r).all(|dj| {
                    (-r..=r).all(|di| {
                        let (a, b) = (i as i64 + di, j as i64 + dj);
                        a >= 0
                            && b >= 0
                            && a < fm.nx as i64
                            && b < fm.ny as i64
                            && fm.v[b as usize * fm.nx + a as usize] >= 0.5
                    })
                })
            };
            let c = fm.centre(i, j);
            let conifers = matches!(
                env.zone(c),
                Some((_, "Westridge Woods" | "North Hollins" | "Hollins Heights"))
            );
            for sub in 0..4u64 {
                let r0 = env.h01(i as i64, j as i64, 100 + sub);
                if r0 >= p_sub * if edge { 1.6 } else { 1.0 } {
                    continue;
                }
                let r1 = env.h01(i as i64, j as i64, 200 + sub);
                let r2 = env.h01(i as i64, j as i64, 300 + sub);
                let p = [
                    c[0] - 3.0 + 6.0 * (sub % 2) as f32 + (r1 - 0.5) * 4.0,
                    c[1] - 3.0 + 6.0 * (sub / 2) as f32 + (r2 - 0.5) * 4.0,
                ];
                if fm.clearing.at(p) > 0.55 {
                    continue;
                }
                let kind = if !edge {
                    TreeKind::Mass
                } else if (r1 < 0.7) == conifers {
                    TreeKind::Conifer
                } else {
                    TreeKind::Broad
                };
                let mut t = tree(p, env.gz(p), kind, r2);
                if deep {
                    for f in &mut t {
                        f.solid = false;
                    }
                }
                trees.push(t);
            }
        }
    }
    for t in thin(trees, CAP_FOREST_TREES) {
        out.extend(t);
    }
    // Undergrowth along the edge, nearest the roads first if over budget.
    let g = Grid {
        nx: fm.nx,
        ny: fm.ny,
        origin: fm.centre(0, 0),
        cell: FOREST_CELL_M,
        v: &fm.v,
    };
    let st = Style {
        kind: K::Hedge,
        mat: M::Foliage,
        thick: 1.6,
        height: 1.6,
    };
    let mut pieces: Vec<(f32, Feature)> = Vec::new();
    for line in contour::contours(&g, 0.5) {
        for (a, b) in contour::pieces(&line, 1.2, 18.0) {
            let d = geom::normalize(geom::sub(b, a));
            let inward = [-d[1], d[0]];
            let m = lerp(a, b, 0.5);
            let edge_d = (m[0] - MAP_LO[0])
                .min(MAP_HI[0] - m[0])
                .min(m[1] - MAP_LO[1])
                .min(MAP_HI[1] - m[1]);
            if edge_d < 25.0 {
                continue;
            }
            let (a2, b2) = (
                geom::add(a, geom::scale(inward, 1.0)),
                geom::add(b, geom::scale(inward, 1.0)),
            );
            for (p, q) in contour::runs(a2, b2, 0.8, 1.2, |s| {
                band_ok(env, s, inward, st.thick * 0.5) && !env.ctx.is_water(s)
            }) {
                let rd = env.ctx.road_distance(lerp(p, q, 0.5), 40.0);
                pieces.push((rd, ground_piece(env, st, p, q)));
            }
        }
    }
    if pieces.len() > CAP_UNDERGROWTH {
        let mut idx: Vec<usize> = (0..pieces.len()).collect();
        idx.sort_by(|x, y| pieces[*x].0.total_cmp(&pieces[*y].0).then(x.cmp(y)));
        let mut keep = vec![false; pieces.len()];
        for k in idx.into_iter().take(CAP_UNDERGROWTH) {
            keep[k] = true;
        }
        out.extend(
            pieces
                .into_iter()
                .zip(keep)
                .filter(|(_, k)| *k)
                .map(|((_, f), _)| f),
        );
    } else {
        out.extend(pieces.into_iter().map(|(_, f)| f));
    }
}

/// Sparse trees on open land outside the forest (jittered 30 m grid, more in the suburbs).
pub(super) fn scattered(env: &Env, fm: &ForestMask, out: &mut Vec<Feature>) {
    let s = 30.0;
    let nx = ((MAP_HI[0] - MAP_LO[0]) / s).ceil() as i64;
    let ny = ((MAP_HI[1] - MAP_LO[1]) / s).ceil() as i64;
    let mut trees = Vec::new();
    for j in 0..ny {
        for i in 0..nx {
            let r0 = env.h01(i, j, 500);
            let r1 = env.h01(i, j, 501);
            let r2 = env.h01(i, j, 502);
            let p = [
                MAP_LO[0] + (i as f32 + 0.2 + 0.6 * r1) * s,
                MAP_LO[1] + (j as f32 + 0.2 + 0.6 * r2) * s,
            ];
            let prob = match env.zone(p).map(|z| z.0) {
                Some(D::Suburb) => 0.5,
                Some(D::Residential) => 0.35,
                Some(D::Industrial) => 0.12,
                Some(D::Commercial) => 0.08,
                _ => 0.0,
            };
            if r0 >= prob || fm.at(p) {
                continue;
            }
            if !env.ctx.free(&ngon(p, 2.5, 8), 1.0) {
                continue;
            }
            let kind = if r1 < 0.3 {
                TreeKind::Conifer
            } else {
                TreeKind::Broad
            };
            trees.push(tree(p, env.gz(p), kind, r2));
        }
    }
    for t in thin(trees, CAP_SCATTERED) {
        out.extend(t);
    }
}
