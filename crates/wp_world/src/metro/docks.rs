//! Port Vesper v3 — port and waterfront: container terminal (stacks, gantry cranes, yard
//! machines, gates), Gantry Row quay, Old Harbor piers, ferry terminal, ships and boats,
//! quay walls.
//!
//! **Owned by the v3 docks workstream.** Contract with the pipeline (`metro::generate`):
//! - [`earthworks`] runs before roads exist (ground levelling beyond the plan's site pads);
//! - [`generate`] runs after roads, in pipeline order, and adds features / buildings /
//!   POIs to the [`Ctx`]. Use `ctx.rng(SALT)` for randomness, stay inside your sites
//!   (`ctx.plan.sites_of(..)`) or check `ctx.free(..)` before placing, and `ctx.place` /
//!   `ctx.add_building` so later modules see what you placed.
//!
//! What it builds (all from the plan's site polygons, gates and rail lines, and the terrain):
//! - **Waterline analysis** ([`quay`]): every site edge is sampled every [`quay::STEP`] m and
//!   marched outward to the first water. Seaward samples get a solid concrete quay wall
//!   (top flush with the pad, face down to the sea bed, covering the pad's blend slope), a
//!   0.8 m kerb on the edge and mooring bollards; land samples get the perimeter fence,
//!   with openings only at the plan's gates, at rail crossings and where roads cross.
//! - **Container terminal / Gantry Row quay** ([`yard`]): gate complexes, admin building,
//!   maintenance shed, container blocks (one box per run of equal stacks) with ≥ 8 m lanes
//!   between them, reefer racks, an empty-container depot, an open autocross block, rubber-
//!   tyred yard cranes, straddle carriers, reach stackers, floodlight masts.
//! - **Berths** ([`kit`]): container ships / a bulk carrier moored along the longest quay
//!   runs with ship-to-shore gantry cranes over them.
//! - **Old Harbor** ([`harbor`]): one pier per plan gate (ferry terminal with a moored drive-in
//!   ferry, fish market, restaurant, Pier 9 boathouse), railings, piles, marinas.

mod harbor;
mod kit;
mod quay;
mod yard;

use std::ops::Range;

use super::ctx::{ccw, footprint, Ctx, Occ};
use super::ground::{inside, signed_dist, Earthwork, Natural};
use super::plan::{self, Plan, Site, SiteKind};
use crate::geom::{self, V2, V3};
use crate::network::{Feature, FeatureKind, FeatureShape, Material};

/// RNG salt of this module.
pub const SALT: u64 = 0xD0;

/// What [`generate_with_report`] built (for tests and diagnostics).
#[derive(Debug, Clone, Default)]
pub struct DocksReport {
    /// Indices of this module's features in `ctx.features`.
    pub features: Range<usize>,
    /// Waterline points found along padded sites (site id, waterline point, outward normal).
    pub waterline: Vec<(&'static str, V2, V2)>,
    /// Drivable lanes between container blocks (8 m wide core rectangles, CCW).
    pub lanes: Vec<Vec<V2>>,
    /// Piers: gate position, deck start point (top of deck), deck footprint.
    pub piers: Vec<PierInfo>,
    /// Number of container blocks placed.
    pub blocks: usize,
    /// Number of ships moored.
    pub ships: usize,
    /// Number of ship-to-shore gantry cranes.
    pub cranes: usize,
}

/// One built pier.
#[derive(Debug, Clone)]
pub struct PierInfo {
    /// The plan gate the pier starts at.
    pub gate: V2,
    /// Top of the deck at its landward end.
    pub start: V3,
    /// Footprint of the whole deck (CCW).
    pub footprint: Vec<V2>,
}

/// Ground levelling this module needs before roads are profiled: where a padded port site
/// stops short of the sea (a strip of natural shore between the pad and the water), the pad
/// is extended out to just before the natural waterline so the quay meets the sea.
pub fn earthworks(plan: &Plan, nat: &Natural) -> Vec<Earthwork> {
    let mut out = Vec::new();
    for s in &plan.sites {
        if !matches!(s.kind, SiteKind::ContainerTerminal | SiteKind::Quay) {
            continue;
        }
        let Some(z) = s.pad_z else { continue };
        let poly = ccw(s.polygon.clone());
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let t = geom::normalize(geom::sub(b, a));
            let n = geom::right_of(t);
            let mut min_dw = f32::MAX;
            let mut all = true;
            for k in 0..9 {
                let p = lerp(a, b, (k as f32 + 0.5) / 9.0);
                let mut d = 0.0;
                let mut found = None;
                while d <= quay::REACH {
                    if nat.z(off(p, n, d)) < plan::WATER_Z {
                        found = Some(d);
                        break;
                    }
                    d += 2.0;
                }
                match found {
                    Some(d) => min_dw = min_dw.min(d),
                    None => all = false,
                }
            }
            if all && min_dw >= 20.0 {
                let e = min_dw - 6.0;
                out.push(Earthwork::Pad {
                    polygon: ccw(vec![
                        off(a, n, -2.0),
                        off(b, n, -2.0),
                        off(b, n, e),
                        off(a, n, e),
                    ]),
                    z,
                    blend_m: 6.0,
                });
            }
        }
    }
    out
}

/// Place this module's content.
pub fn generate(ctx: &mut Ctx) {
    generate_with_report(ctx);
}

/// Place this module's content and report what was built.
pub fn generate_with_report(ctx: &mut Ctx) -> DocksReport {
    let start = ctx.features.len();
    let mut rng = ctx.rng(SALT);
    let mut rep = DocksReport::default();
    let sites: Vec<Site> = ctx
        .plan
        .sites
        .iter()
        .filter(|s| {
            matches!(
                s.kind,
                SiteKind::ContainerTerminal | SiteKind::Quay | SiteKind::Piers
            )
        })
        .cloned()
        .collect();
    for site in &sites {
        match site.kind {
            SiteKind::ContainerTerminal => yard::terminal(ctx, &mut rng, site, &mut rep),
            SiteKind::Quay => yard::gantry_quay(ctx, &mut rng, site, &mut rep),
            _ => harbor::piers(ctx, &mut rng, site, &mut rep),
        }
    }
    rep.features = start..ctx.features.len();
    rep
}

// --- Shared helpers -------------------------------------------------------------------

/// A local 2D frame: `u` forward (unit), `v` = right of `u`.
#[derive(Debug, Clone, Copy)]
struct Fr {
    o: V2,
    u: V2,
    v: V2,
}

impl Fr {
    /// Frame at `o` facing `u` (normalised here).
    fn new(o: V2, u: V2) -> Self {
        let u = geom::normalize(u);
        Self {
            o,
            u,
            v: geom::right_of(u),
        }
    }

    /// World point at `a` along `u` and `b` along `v`.
    fn at(&self, a: f32, b: f32) -> V2 {
        [
            self.o[0] + self.u[0] * a + self.v[0] * b,
            self.o[1] + self.u[1] * a + self.v[1] * b,
        ]
    }

    /// Yaw whose local +Y is `u`.
    fn yaw(&self) -> f32 {
        geom::yaw_of(self.u)
    }

    /// A box with its centre at (`a`, `b`) in the frame: `len` along `u`, `w` along `v`,
    /// from `z0` up `h`.
    #[allow(clippy::too_many_arguments)]
    fn bx(
        &self,
        kind: FeatureKind,
        mat: Material,
        a: f32,
        b: f32,
        z0: f32,
        len: f32,
        w: f32,
        h: f32,
        solid: bool,
    ) -> Feature {
        super::ctx::box_on(kind, mat, self.at(a, b), z0, [w, len, h], self.yaw(), solid)
    }

    /// Rectangle polygon (CCW) spanning `a0..a1` along `u` and `b0..b1` along `v`.
    fn rect(&self, a0: f32, a1: f32, b0: f32, b1: f32) -> Vec<V2> {
        ccw(vec![
            self.at(a0, b0),
            self.at(a1, b0),
            self.at(a1, b1),
            self.at(a0, b1),
        ])
    }

    /// A 3D point in the frame.
    fn p3(&self, a: f32, b: f32, z: f32) -> V3 {
        let p = self.at(a, b);
        [p[0], p[1], z]
    }
}

/// `p + n * d`.
fn off(p: V2, n: V2, d: f32) -> V2 {
    [p[0] + n[0] * d, p[1] + n[1] * d]
}

/// Linear interpolation.
fn lerp(a: V2, b: V2, t: f32) -> V2 {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// A straight beam / strut box between two 3D points (`thick` square section).
fn strut(kind: FeatureKind, mat: Material, a: V3, b: V3, thick: f32, solid: bool) -> Feature {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let horiz = (d[0] * d[0] + d[1] * d[1]).sqrt();
    let len = (horiz * horiz + d[2] * d[2]).sqrt();
    let yaw = if horiz > 1e-4 {
        geom::yaw_of([d[0] / horiz, d[1] / horiz])
    } else {
        0.0
    };
    Feature {
        kind,
        material: mat,
        shape: FeatureShape::Box {
            centre: [
                (a[0] + b[0]) * 0.5,
                (a[1] + b[1]) * 0.5,
                (a[2] + b[2]) * 0.5,
            ],
            half: [thick * 0.5, len * 0.5, thick * 0.5],
            yaw,
            pitch: wp_math::atan2(d[2], horiz),
        },
        solid,
    }
}

/// True when a feature's footprint touches no road paving or verge.
fn clear_of_roads(ctx: &Ctx, f: &Feature) -> bool {
    ctx.free_of(&footprint(&f.shape), 0.0, &[Occ::Road, Occ::Verge])
}

/// Place `f` unless it touches road paving / verge. Returns whether it was placed.
fn put(ctx: &mut Ctx, f: Feature) -> bool {
    if clear_of_roads(ctx, &f) {
        ctx.place(f);
        true
    } else {
        false
    }
}

/// Place all of `fs` (one structure) or none of them if any part touches a road / verge.
fn put_all(ctx: &mut Ctx, fs: Vec<Feature>) -> bool {
    if fs.iter().all(|f| clear_of_roads(ctx, f)) {
        for f in fs {
            ctx.place(f);
        }
        true
    } else {
        false
    }
}

/// Place all of `fs` (a drivable deck continuing a road, its railings) or none of them if
/// any part touches road paving (the verge where a road ends at a gate is allowed).
fn put_deck(ctx: &mut Ctx, fs: Vec<Feature>) -> bool {
    if fs
        .iter()
        .all(|f| ctx.free_of(&footprint(&f.shape), 0.0, &[Occ::Road]))
    {
        for f in fs {
            ctx.place(f);
        }
        true
    } else {
        false
    }
}

/// All vertices of `poly` at least `inset` inside the site polygon.
fn inside_site(site: &[V2], poly: &[V2], inset: f32) -> bool {
    poly.iter()
        .all(|p| inside(site, *p) && signed_dist(site, *p) <= -inset)
}

/// Separating-axis overlap test of two convex polygons.
fn convex_overlap(a: &[V2], b: &[V2]) -> bool {
    for poly in [a, b] {
        for i in 0..poly.len() {
            let e = geom::sub(poly[(i + 1) % poly.len()], poly[i]);
            let ax = [-e[1], e[0]];
            let proj = |q: &[V2]| {
                q.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
                    let d = geom::dot(*p, ax);
                    (lo.min(d), hi.max(d))
                })
            };
            let (alo, ahi) = proj(a);
            let (blo, bhi) = proj(b);
            if ahi < blo || bhi < alo {
                return false;
            }
        }
    }
    true
}

/// Minimum distance between a polyline and a polygon (0 if they touch or the line is
/// inside).
fn line_poly_dist(line: &[V2], poly: &[V2]) -> f32 {
    let mut best = f32::MAX;
    for w in line.windows(2) {
        if inside(poly, w[0]) || inside(poly, w[1]) {
            return 0.0;
        }
        for i in 0..poly.len() {
            let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
            best = best.min(geom::segment_distance(w[0], w[1], p, q));
        }
    }
    best
}

/// All water under `poly` (vertices, centre and edge midpoints).
fn all_water(ctx: &Ctx, poly: &[V2]) -> bool {
    let n = poly.len() as f32;
    let c = poly
        .iter()
        .fold([0.0, 0.0], |s, p| [s[0] + p[0] / n, s[1] + p[1] / n]);
    if !ctx.is_water(c) {
        return false;
    }
    (0..poly.len()).all(|i| {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        ctx.is_water(a) && ctx.is_water(lerp(a, b, 0.5))
    })
}

/// Pick from `items` with integer weights.
fn pick<T: Copy>(rng: &mut wp_math::Rng, items: &[(T, u32)]) -> T {
    let total: u32 = items.iter().map(|i| i.1).sum();
    let mut r = rng.below(total.max(1));
    for (t, w) in items {
        if r < *w {
            return *t;
        }
        r -= w;
    }
    items[items.len() - 1].0
}

#[cfg(test)]
mod tests;
