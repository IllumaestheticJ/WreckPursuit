//! Shared context for the v3 modules that run after the road network exists (sites,
//! buildings, scenery): final terrain, a 2 m occupancy grid (roads, water, what earlier
//! modules placed), per-module RNGs and helpers that make [`Feature`]s.
//!
//! **Owned by the v3 lead.** Modules may *call* anything here; if a helper is missing, add
//! it in your own module (or ask in your report) rather than editing this file.

use wp_math::Rng;

use super::ground::{bounds, inside, seg_dist, signed_dist};
use super::plan::{self, Plan, PlannedPoi, MAP_HI, MAP_LO};
use super::roads::RoadsOut;
use crate::geom::{self, V2, V3};
use crate::network::{Building, Feature, FeatureKind, FeatureShape, Material};
use crate::terrain::Heightfield;

/// Occupancy grid cell size, m.
pub const OCC_CELL_M: f32 = 2.0;
/// Sidewalk / verge kept clear beside every paved road edge, m.
pub const VERGE_M: f32 = 3.0;

/// What occupies a grid cell (ordered: a higher class is never overwritten by a lower one).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Occ {
    /// Nothing.
    Free = 0,
    /// Reserved by a site (only its owning module builds here).
    Site = 1,
    /// Water.
    Water = 2,
    /// Placed scenery / structure.
    Feature = 3,
    /// Building.
    Building = 4,
    /// Sidewalk / verge beside a road.
    Verge = 5,
    /// Road paving.
    Road = 6,
}

/// The context.
pub struct Ctx<'a> {
    /// The plan.
    pub plan: &'a Plan,
    /// Final (carved) terrain.
    pub terrain: &'a Heightfield,
    /// The road network.
    pub roads: &'a RoadsOut,
    /// Generator seed.
    pub seed: u64,
    /// Features placed so far (in placement order).
    pub features: Vec<Feature>,
    /// Buildings placed so far.
    pub buildings: Vec<Building>,
    /// POIs modules add (snapped to the nearest lane with the plan's POIs).
    pub pois: Vec<PlannedPoi>,
    nx: usize,
    ny: usize,
    occ: Vec<u8>,
}

impl<'a> Ctx<'a> {
    /// Build the context: rasterises water, road paving, verges and reserved sites.
    pub fn new(plan: &'a Plan, terrain: &'a Heightfield, roads: &'a RoadsOut, seed: u64) -> Self {
        let nx = ((MAP_HI[0] - MAP_LO[0]) / OCC_CELL_M).ceil() as usize;
        let ny = ((MAP_HI[1] - MAP_LO[1]) / OCC_CELL_M).ceil() as usize;
        let mut c = Self {
            plan,
            terrain,
            roads,
            seed,
            features: Vec::new(),
            buildings: Vec::new(),
            pois: Vec::new(),
            nx,
            ny,
            occ: vec![0; nx * ny],
        };
        for s in &plan.sites {
            c.mark_poly(&s.polygon, 0.0, Occ::Site);
        }
        for j in 0..ny {
            for i in 0..nx {
                let p = c.cell_centre(i, j);
                if terrain.height_at(p) < plan::WATER_Z {
                    c.occ[j * nx + i] = Occ::Water as u8;
                }
            }
        }
        for s in &roads.built.surfaces {
            let (l, r) = crate::surface::surface_edges(s);
            for k in 0..l.len().saturating_sub(1) {
                let quad = [
                    geom::xy(l[k]),
                    geom::xy(l[k + 1]),
                    geom::xy(r[k + 1]),
                    geom::xy(r[k]),
                ];
                let quad = ccw(quad.to_vec());
                c.mark_poly(&quad, VERGE_M, Occ::Verge);
                c.mark_poly(&quad, 0.0, Occ::Road);
            }
        }
        // Junction boxes.
        for it in &roads.built.intersections {
            let n = &roads.built.nodes[it.node as usize];
            let r = it.radius_m.max(6.0);
            c.mark_disc(geom::xy(n.pos), r + VERGE_M, Occ::Verge);
            c.mark_disc(geom::xy(n.pos), r, Occ::Road);
        }
        c
    }

    /// A deterministic RNG for one module (`salt` = any constant unique to the module).
    pub fn rng(&self, salt: u64) -> Rng {
        Rng::new(self.seed ^ salt.wrapping_mul(0x9e37_79b9_7f4a_7c15))
    }

    /// Ground height at `p` (carved terrain), m.
    pub fn ground_z(&self, p: V2) -> f32 {
        self.terrain.height_at(p)
    }

    /// Lowest and highest ground under a polygon (sampled every 4 m and at its vertices), m.
    pub fn ground_range(&self, poly: &[V2]) -> (f32, f32) {
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        let mut take = |p: V2| {
            let z = self.ground_z(p);
            lo = lo.min(z);
            hi = hi.max(z);
        };
        for p in poly {
            take(*p);
        }
        let (a, b) = bounds(poly);
        let mut y = a[1];
        while y <= b[1] {
            let mut x = a[0];
            while x <= b[0] {
                if inside(poly, [x, y]) {
                    take([x, y]);
                }
                x += 4.0;
            }
            y += 4.0;
        }
        (lo, hi)
    }

    /// Water at `p`.
    pub fn is_water(&self, p: V2) -> bool {
        self.ground_z(p) < plan::WATER_Z
    }

    /// Occupancy at `p` (`Occ::Road` outside the map).
    pub fn occ_at(&self, p: V2) -> Occ {
        match self.cell_of(p) {
            Some(k) => occ_from(self.occ[k]),
            None => Occ::Road,
        }
    }

    /// Distance from `p` to the nearest road paving edge, searched up to `max_m` (returns
    /// `max_m` if none is closer), m.
    pub fn road_distance(&self, p: V2, max_m: f32) -> f32 {
        let r = (max_m / OCC_CELL_M).ceil() as i64;
        let Some(k) = self.cell_of(p) else {
            return 0.0;
        };
        let (ci, cj) = ((k % self.nx) as i64, (k / self.nx) as i64);
        let mut best = max_m;
        for dj in -r..=r {
            for di in -r..=r {
                let (i, j) = (ci + di, cj + dj);
                if i < 0 || j < 0 || i >= self.nx as i64 || j >= self.ny as i64 {
                    continue;
                }
                if self.occ[j as usize * self.nx + i as usize] == Occ::Road as u8 {
                    let d = geom::dist(p, self.cell_centre(i as usize, j as usize));
                    best = best.min(d);
                }
            }
        }
        best
    }

    /// True when no cell within `margin_m` of `poly` (inside or around it) is of a class in
    /// `blocked`, and the polygon is inside the map.
    pub fn free_of(&self, poly: &[V2], margin_m: f32, blocked: &[Occ]) -> bool {
        let (lo, hi) = bounds(poly);
        if lo[0] < MAP_LO[0] || lo[1] < MAP_LO[1] || hi[0] > MAP_HI[0] || hi[1] > MAP_HI[1] {
            return false;
        }
        let mut ok = true;
        self.for_cells(poly, margin_m, |k| {
            if blocked.contains(&occ_from(self.occ[k])) {
                ok = false;
            }
        });
        ok
    }

    /// Free for generic placement (buildings, scenery): no road, verge, water, building,
    /// feature or reserved site within `margin_m`.
    pub fn free(&self, poly: &[V2], margin_m: f32) -> bool {
        self.free_of(
            poly,
            margin_m,
            &[
                Occ::Site,
                Occ::Water,
                Occ::Feature,
                Occ::Building,
                Occ::Verge,
                Occ::Road,
            ],
        )
    }

    /// Free inside a site the caller owns: like [`Ctx::free`] but `Occ::Site` is allowed.
    pub fn free_in_site(&self, poly: &[V2], margin_m: f32) -> bool {
        self.free_of(
            poly,
            margin_m,
            &[
                Occ::Water,
                Occ::Feature,
                Occ::Building,
                Occ::Verge,
                Occ::Road,
            ],
        )
    }

    /// Mark `poly` (grown by `margin_m`) as `class` (never downgrades a cell).
    pub fn occupy(&mut self, poly: &[V2], margin_m: f32, class: Occ) {
        self.mark_poly(poly, margin_m, class);
    }

    /// Add a feature (does not touch the occupancy grid; see [`Ctx::place`]).
    pub fn add(&mut self, f: Feature) {
        self.features.push(f);
    }

    /// Add a feature and mark its footprint as `Occ::Feature`.
    pub fn place(&mut self, f: Feature) {
        let fp = footprint(&f.shape);
        self.mark_poly(&fp, 0.0, Occ::Feature);
        self.features.push(f);
    }

    /// Add a building and mark its footprint as `Occ::Building`.
    pub fn add_building(&mut self, b: Building) {
        self.mark_poly(&b.footprint, 0.0, Occ::Building);
        self.buildings.push(b);
    }

    fn cell_centre(&self, i: usize, j: usize) -> V2 {
        [
            MAP_LO[0] + (i as f32 + 0.5) * OCC_CELL_M,
            MAP_LO[1] + (j as f32 + 0.5) * OCC_CELL_M,
        ]
    }

    fn cell_of(&self, p: V2) -> Option<usize> {
        let i = ((p[0] - MAP_LO[0]) / OCC_CELL_M).floor();
        let j = ((p[1] - MAP_LO[1]) / OCC_CELL_M).floor();
        if i < 0.0 || j < 0.0 || i as usize >= self.nx || j as usize >= self.ny {
            return None;
        }
        Some(j as usize * self.nx + i as usize)
    }

    fn for_cells(&self, poly: &[V2], margin_m: f32, mut f: impl FnMut(usize)) {
        let (lo, hi) = bounds(poly);
        let m = margin_m + OCC_CELL_M * 0.5;
        let i0 = ((lo[0] - m - MAP_LO[0]) / OCC_CELL_M).floor().max(0.0) as usize;
        let j0 = ((lo[1] - m - MAP_LO[1]) / OCC_CELL_M).floor().max(0.0) as usize;
        let i1 = (((hi[0] + m - MAP_LO[0]) / OCC_CELL_M).ceil().max(0.0) as usize).min(self.nx - 1);
        let j1 = (((hi[1] + m - MAP_LO[1]) / OCC_CELL_M).ceil().max(0.0) as usize).min(self.ny - 1);
        for j in j0..=j1 {
            for i in i0..=i1 {
                let p = self.cell_centre(i, j);
                let hit = if margin_m <= 0.0 {
                    inside(poly, p)
                } else {
                    signed_dist(poly, p) <= margin_m
                };
                if hit {
                    f(j * self.nx + i);
                }
            }
        }
    }

    fn mark_poly(&mut self, poly: &[V2], margin_m: f32, class: Occ) {
        let mut cells = Vec::new();
        self.for_cells(poly, margin_m, |k| cells.push(k));
        for k in cells {
            if self.occ[k] < class as u8 {
                self.occ[k] = class as u8;
            }
        }
    }

    fn mark_disc(&mut self, c: V2, r: f32, class: Occ) {
        let poly: Vec<V2> = (0..12)
            .map(|k| {
                let (s, co) = wp_math::sin_cos(k as f32 * core::f32::consts::TAU / 12.0);
                [c[0] + r * co, c[1] + r * s]
            })
            .collect();
        self.mark_poly(&poly, 0.0, class);
    }
}

fn occ_from(v: u8) -> Occ {
    match v {
        0 => Occ::Free,
        1 => Occ::Site,
        2 => Occ::Water,
        3 => Occ::Feature,
        4 => Occ::Building,
        5 => Occ::Verge,
        _ => Occ::Road,
    }
}

/// Make a polygon counter-clockwise.
pub fn ccw(mut poly: Vec<V2>) -> Vec<V2> {
    let mut a = 0.0;
    for i in 0..poly.len() {
        let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
        a += p[0] * q[1] - q[0] * p[1];
    }
    if a < 0.0 {
        poly.reverse();
    }
    poly
}

/// 2D footprint polygon (CCW) of a shape.
pub fn footprint(shape: &FeatureShape) -> Vec<V2> {
    match shape {
        FeatureShape::Box {
            centre,
            half,
            yaw,
            pitch,
        } => {
            let (_, cp) = wp_math::sin_cos(*pitch);
            let hy = half[1] * cp.abs();
            obb([centre[0], centre[1]], [half[0], hy.max(0.05)], *yaw)
        }
        FeatureShape::Cylinder { base, radius, .. } => (0..8)
            .map(|k| {
                let (s, c) = wp_math::sin_cos(k as f32 * core::f32::consts::TAU / 8.0);
                [base[0] + radius * c, base[1] + radius * s]
            })
            .collect(),
        FeatureShape::Prism { footprint, .. } => footprint.clone(),
    }
}

/// Corners (CCW) of an oriented rectangle: centre, half extents (x right, y forward), yaw.
pub fn obb(c: V2, half: V2, yaw: f32) -> Vec<V2> {
    let (s, co) = wp_math::sin_cos(yaw);
    // Local +Y (forward) maps to (-sin, cos); local +X (right) to (cos, sin).
    let fx = [-s, co];
    let rx = [co, s];
    let pt = |a: f32, b: f32| [c[0] + rx[0] * a + fx[0] * b, c[1] + rx[1] * a + fx[1] * b];
    vec![
        pt(-half[0], -half[1]),
        pt(half[0], -half[1]),
        pt(half[0], half[1]),
        pt(-half[0], half[1]),
    ]
}

/// Yaw (0 = +Y, CCW) of the direction `a → b`.
pub fn yaw_of(a: V2, b: V2) -> f32 {
    wp_math::atan2(-(b[0] - a[0]), b[1] - a[1])
}

/// A box feature standing on `z0` (bottom face at `z0`): `size` = full `[w, l, h]` with `l`
/// along the yaw direction.
pub fn box_on(
    kind: FeatureKind,
    material: Material,
    c: V2,
    z0: f32,
    size: V3,
    yaw: f32,
    solid: bool,
) -> Feature {
    Feature {
        kind,
        material,
        shape: FeatureShape::Box {
            centre: [c[0], c[1], z0 + size[2] * 0.5],
            half: [size[0] * 0.5, size[1] * 0.5, size[2] * 0.5],
            yaw,
            pitch: 0.0,
        },
        solid,
    }
}

/// An upright cylinder feature.
pub fn cylinder(
    kind: FeatureKind,
    material: Material,
    base: V3,
    radius: f32,
    height: f32,
    solid: bool,
) -> Feature {
    Feature {
        kind,
        material,
        shape: FeatureShape::Cylinder {
            base,
            radius,
            height,
        },
        solid,
    }
}

/// A prism feature.
pub fn prism(
    kind: FeatureKind,
    material: Material,
    footprint: Vec<V2>,
    base_z: f32,
    top_z: f32,
    solid: bool,
) -> Feature {
    Feature {
        kind,
        material,
        shape: FeatureShape::Prism {
            footprint: ccw(footprint),
            base_z,
            top_z,
        },
        solid,
    }
}

/// A sloped slab (ramp) from `a` to `b` (3D points on the slab's top centreline), `width`
/// wide and `thick` thick.
pub fn ramp(
    kind: FeatureKind,
    material: Material,
    a: V3,
    b: V3,
    width: f32,
    thick: f32,
) -> Feature {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let horiz = (d[0] * d[0] + d[1] * d[1]).sqrt();
    let len = (horiz * horiz + d[2] * d[2]).sqrt();
    let pitch = wp_math::atan2(d[2], horiz);
    let yaw = yaw_of(geom::xy(a), geom::xy(b));
    // Centre of the slab: midpoint of the top centreline, moved down half the thickness
    // along the slab normal.
    let (sp, cp) = wp_math::sin_cos(pitch);
    let (sy, cy) = wp_math::sin_cos(yaw);
    let fwd_h = [-sy, cy];
    let n = [-fwd_h[0] * sp, -fwd_h[1] * sp, cp];
    let mid = [
        (a[0] + b[0]) * 0.5,
        (a[1] + b[1]) * 0.5,
        (a[2] + b[2]) * 0.5,
    ];
    Feature {
        kind,
        material,
        shape: FeatureShape::Box {
            centre: [
                mid[0] - n[0] * thick * 0.5,
                mid[1] - n[1] * thick * 0.5,
                mid[2] - n[2] * thick * 0.5,
            ],
            half: [width * 0.5, len * 0.5, thick * 0.5],
            yaw,
            pitch,
        },
        solid: true,
    }
}

/// Wall / fence pieces along a polyline, following the ground: one box per segment (split
/// into ≤ `max_piece_m` pieces), `thick` wide, `height` above the ground. Pieces whose
/// midpoint is closer than `gap_m` to any point in `gaps` are skipped (gates).
#[allow(clippy::too_many_arguments)]
pub fn wall_along(
    ctx: &Ctx,
    kind: FeatureKind,
    material: Material,
    line: &[V2],
    thick: f32,
    height: f32,
    max_piece_m: f32,
    gaps: &[V2],
    gap_m: f32,
) -> Vec<Feature> {
    let mut out = Vec::new();
    for w in line.windows(2) {
        let len = geom::dist(w[0], w[1]);
        if len < 0.05 {
            continue;
        }
        let n = (len / max_piece_m).ceil().max(1.0) as usize;
        for k in 0..n {
            let t0 = k as f32 / n as f32;
            let t1 = (k + 1) as f32 / n as f32;
            let a = lerp2(w[0], w[1], t0);
            let b = lerp2(w[0], w[1], t1);
            let m = lerp2(a, b, 0.5);
            if gaps.iter().any(|g| seg_dist(a, b, *g) < gap_m) {
                continue;
            }
            let z = ctx.ground_z(a).min(ctx.ground_z(b)).min(ctx.ground_z(m));
            let top = ctx.ground_z(a).max(ctx.ground_z(b)).max(ctx.ground_z(m)) + height;
            out.push(box_on(
                kind,
                material,
                m,
                z - 0.3,
                [thick, geom::dist(a, b) + thick, top - z + 0.3],
                yaw_of(a, b),
                true,
            ));
        }
    }
    out
}

/// Linear interpolation between two points.
pub fn lerp2(a: V2, b: V2, t: f32) -> V2 {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}
