//! Road structures that are not paving, as [`Feature`]s:
//! - **piers** under every elevated deck (one per carriageway, every ~32 m; never inside
//!   another road's clearance envelope),
//! - the **Iron Bridge** steel truss (posts, chords, diagonals, overhead bracing above the
//!   5.5 m envelope), **Kettle Street Drawbridge** towers and counterweights beside the
//!   deck, abutments of the river bridges,
//! - **tunnels** built cut-and-cover: side walls, a roof slab at road + 6.5 m, a solid
//!   grass plug from the roof up to the surrounding natural ground (below the water line
//!   under the river, below any road passing over it) wide enough to cover the trench's
//!   sloped faces, headwall portals; retaining walls (with a parapet) along open cuts,
//! - **sign gantries** over the highway carriageways and before every diverge,
//! - a **guard rail** on the sea side of Bluff Road.
//!
//! Wheels drive on the road mesh, so a pier or wall is only placed where it is clear of
//! every lane polyline (lane width / 2 + 0.6 m) between the lane and 5.5 m above it
//! ([`envelope_clear`]).

use super::super::ctx::{box_on, ramp};
use super::super::plan::{self, Plan};
use super::geo::{self, cum2, SegGrid};
use super::highways::Hw;
use super::{Crossing, RoadInfo, RoadKind};
use crate::builder::{Built, RoadSpec};
use crate::geom::{self, V2, V3};
use crate::network::{Feature, FeatureKind, FeatureShape, Material};
use crate::terrain::Heightfield;

/// Height of the road clearance envelope above a lane, m.
pub const ENVELOPE_H_M: f32 = 5.5;
/// Extra lateral margin of the envelope beyond half a lane, m.
pub const ENVELOPE_PAD_M: f32 = 0.6;

/// Lane segments for envelope queries.
pub struct LaneGrid<'a> {
    built: &'a Built,
    grid: SegGrid,
}

impl<'a> LaneGrid<'a> {
    /// Index every lane of `built`.
    pub fn new(built: &'a Built) -> Self {
        let mut grid = SegGrid::new(plan::MAP_LO, plan::MAP_HI, 40.0);
        for (i, l) in built.lanes.iter().enumerate() {
            let p: Vec<V2> = l.polyline.iter().map(|q| geom::xy(*q)).collect();
            grid.insert_line(i as u32, &p, l.width_m * 0.5 + ENVELOPE_PAD_M);
        }
        Self { built, grid }
    }

    /// Whether a solid volume (2D footprint polygon, z range) stays clear of every lane's
    /// envelope.
    pub fn clear(&self, fp: &[V2], z0: f32, z1: f32) -> bool {
        let (lo, hi) = geo::bbox(fp);
        for (li, k) in self.grid.query(lo, hi) {
            let l = &self.built.lanes[li as usize];
            let (a, b) = (l.polyline[k as usize], l.polyline[k as usize + 1]);
            let w = l.width_m * 0.5 + ENVELOPE_PAD_M;
            // Distance from the segment to the polygon (0 if they overlap).
            let d = seg_poly_dist(geom::xy(a), geom::xy(b), fp);
            if d >= w {
                continue;
            }
            let (za, zb) = (a[2].min(b[2]), a[2].max(b[2]));
            if z1 > za && z0 < zb + ENVELOPE_H_M {
                return false;
            }
        }
        true
    }
}

fn seg_poly_dist(a: V2, b: V2, poly: &[V2]) -> f32 {
    if crate::geom::point_in_polygon(a, poly) || crate::geom::point_in_polygon(b, poly) {
        return 0.0;
    }
    let n = poly.len();
    let mut best = f32::INFINITY;
    for i in 0..n {
        let (c, d) = (poly[i], poly[(i + 1) % n]);
        if geo::seg_cross(a, b, c, d).is_some() {
            return 0.0;
        }
        best = best
            .min(geom::point_segment_distance(c, a, b))
            .min(geom::point_segment_distance(d, a, b))
            .min(geom::point_segment_distance(a, c, d))
            .min(geom::point_segment_distance(b, c, d));
    }
    best
}

/// Footprint polygon of a feature shape.
fn fp(f: &Feature) -> Vec<V2> {
    super::super::ctx::footprint(&f.shape)
}

/// Vertical extent of a feature.
fn z_range(f: &Feature) -> (f32, f32) {
    match &f.shape {
        FeatureShape::Box {
            centre,
            half,
            pitch,
            ..
        } => {
            let (sp, cp) = wp_math::sin_cos(*pitch);
            let h = half[2] * cp.abs() + half[1] * sp.abs();
            (centre[2] - h, centre[2] + h)
        }
        FeatureShape::Cylinder { base, height, .. } => (base[2], base[2] + height),
        FeatureShape::Prism { base_z, top_z, .. } => (*base_z, *top_z),
    }
}

/// Whether feature `f` is clear of every lane envelope.
pub fn envelope_clear(lg: &LaneGrid, f: &Feature) -> bool {
    let (z0, z1) = z_range(f);
    lg.clear(&fp(f), z0, z1)
}

/// Collector that drops solid pieces violating an envelope.
struct Out<'a> {
    lg: &'a LaneGrid<'a>,
    v: Vec<Feature>,
    dropped: usize,
}

impl Out<'_> {
    fn push(&mut self, f: Feature) {
        if f.solid && !envelope_clear(self.lg, &f) {
            self.dropped += 1;
            return;
        }
        self.v.push(f);
    }
}

/// Every road structure feature.
#[allow(clippy::too_many_arguments)]
pub fn features(
    plan: &Plan,
    terrain: &Heightfield,
    natural: &Heightfield,
    built: &Built,
    specs: &[RoadSpec],
    info: &[RoadInfo],
    names: &[String],
    hws: &[Hw],
    crossings: &[Crossing],
    notes: &mut Vec<String>,
) -> Vec<Feature> {
    let lg = LaneGrid::new(built);
    let mut out = Out {
        lg: &lg,
        v: Vec::new(),
        dropped: 0,
    };
    let _ = (specs, hws, crossings, plan);
    let mut surf_grid = SegGrid::new(plan::MAP_LO, plan::MAP_HI, 40.0);
    for (i, s) in built.surfaces.iter().enumerate() {
        let p: Vec<V2> = s.centre.iter().map(|q| geom::xy(*q)).collect();
        surf_grid.insert_line(i as u32, &p, s.left_m.max(s.right_m));
    }
    // Lowest other paving over a footprint (plugs stay under it).
    let paving_min = |own: usize, poly: &[V2]| -> f32 {
        let (lo, hi) = geo::bbox(poly);
        let mut m = f32::INFINITY;
        for (si, k) in surf_grid.query(lo, hi) {
            if si as usize == own {
                continue;
            }
            let s = &built.surfaces[si as usize];
            let k = k as usize;
            let (a, b) = (geom::xy(s.centre[k]), geom::xy(s.centre[k + 1]));
            if seg_poly_dist(a, b, poly) < s.left_m.max(s.right_m) + 1.0 {
                m = m.min(s.centre[k][2].min(s.centre[k + 1][2]) - 0.6);
            }
        }
        m
    };
    let mut n_piers = 0;
    let mut n_tunnel = 0;
    let mut covered_m = 0.0f32;
    for (si, s) in built.surfaces.iter().enumerate() {
        let r = s.road as usize;
        let n = s.centre.len();
        if n < 2 {
            continue;
        }
        let pts: Vec<V2> = s.centre.iter().map(|q| geom::xy(*q)).collect();
        let cum = cum2(&pts);
        let len = cum[n - 1];
        let half = s.left_m.max(s.right_m);
        let width = s.left_m + s.right_m;
        let is_hw = s.highway;
        let covered = &info[r].covered;
        // Map spec-arc covered ranges onto this trimmed surface (the trim is a few metres).
        let sp = &specs[r];
        let mut spec_path = vec![geom::xy([0.0, 0.0, 0.0])];
        spec_path.clear();
        let _ = sp;
        // ---- Piers under elevated stretches, abutment walls ----
        let mut s_at = 16.0;
        while s_at < len - 8.0 {
            let (p, d) = geo::point_on(&pts, &cum, s_at);
            let k = cum.partition_point(|c| *c <= s_at).clamp(1, n - 1);
            let t = ((s_at - cum[k - 1]) / (cum[k] - cum[k - 1]).max(1e-3)).clamp(0.0, 1.0);
            let z = s.centre[k - 1][2] + (s.centre[k][2] - s.centre[k - 1][2]) * t;
            let off = 0.5 * (s.right_m - s.left_m);
            let pc = geom::add(p, geom::scale(geom::right_of(d), off));
            let g = terrain.height_at(pc).min(natural.height_at(pc));
            let deck_under = z - 0.9;
            if deck_under - g > 3.0 {
                let yaw = super::super::ctx::yaw_of(p, geom::add(p, d));
                let pw = (width * 0.55).clamp(1.6, 9.0);
                let f = box_on(
                    FeatureKind::BridgeStructure,
                    Material::Concrete,
                    pc,
                    g - 1.0,
                    [pw, 1.6, deck_under - g + 1.0],
                    yaw,
                    true,
                );
                let before = out.v.len();
                out.push(f);
                if out.v.len() > before {
                    n_piers += 1;
                    // Pier cap under the deck (non-solid visual).
                    out.push(box_on(
                        FeatureKind::BridgeStructure,
                        Material::Concrete,
                        pc,
                        deck_under - 0.8,
                        [width.max(pw), 1.8, 0.8],
                        yaw,
                        false,
                    ));
                }
            }
            s_at += if is_hw { 32.0 } else { 28.0 };
        }
        // ---- Cuts and tunnels ----
        let mut k = 0;
        while k + 1 < n {
            let (a, b) = (s.centre[k], s.centre[k + 1]);
            let (pa, pb) = (geom::xy(a), geom::xy(b));
            let seg = geom::dist(pa, pb);
            if seg < 0.5 {
                k += 1;
                continue;
            }
            let mid = geom::scale(geom::add(pa, pb), 0.5);
            let zr = 0.5 * (a[2] + b[2]);
            let nat = natural.height_at(mid);
            let depth = nat - zr;
            let d = geom::normalize(geom::sub(pb, pa));
            let yaw = super::super::ctx::yaw_of(pa, pb);
            let rt = geom::right_of(d);
            let s_mid = cum[k] + seg * 0.5;
            let in_cov = covered
                .iter()
                .any(|(c0, c1)| s_mid >= c0 - 2.0 && s_mid <= c1 + 2.0);
            let water_under = nat < plan::WATER_Z;
            if in_cov || depth > 2.5 {
                // Side walls (outside the paving).
                let roof = zr + 6.5;
                let wall_top = if in_cov { roof } else { nat.max(zr) + 1.0 };
                for side in [-1.0f32, 1.0] {
                    let edge = if side > 0.0 { s.right_m } else { s.left_m };
                    let c = geom::add(mid, geom::scale(rt, side * (edge + 0.9)));
                    out.push(box_on(
                        FeatureKind::Tunnel,
                        Material::DarkConcrete,
                        c,
                        zr.min(a[2].min(b[2])) - 0.5,
                        [0.6, seg + 0.4, wall_top - zr + 0.5 + (a[2] - b[2]).abs()],
                        yaw,
                        true,
                    ));
                }
                if in_cov {
                    covered_m += seg;
                    n_tunnel += 1;
                    // Roof slab, then the plug up to the natural ground.
                    let rw = width + 2.0 * 1.3;
                    let zr_hi = a[2].max(b[2]);
                    out.push(box_on(
                        FeatureKind::Tunnel,
                        Material::Concrete,
                        geom::add(mid, geom::scale(rt, 0.5 * (s.right_m - s.left_m))),
                        zr_hi + 6.0,
                        [rw, seg + 0.3, 0.6],
                        yaw,
                        true,
                    ));
                    let pw = width + 2.0 * 15.0;
                    let foot = super::super::ctx::obb(
                        geom::add(mid, geom::scale(rt, 0.5 * (s.right_m - s.left_m))),
                        [pw * 0.5, seg * 0.5 + 0.2],
                        yaw,
                    );
                    let mut top = nat;
                    if water_under {
                        top = top.min(plan::WATER_Z - 0.4);
                    }
                    top = top.min(paving_min(si, &foot));
                    let base = zr_hi + 6.6;
                    if top > base + 0.3 {
                        out.push(Feature {
                            kind: FeatureKind::Tunnel,
                            material: Material::Grass,
                            shape: FeatureShape::Prism {
                                footprint: super::super::ctx::ccw(foot),
                                base_z: base,
                                top_z: top,
                            },
                            solid: true,
                        });
                    }
                }
            }
            k += 1;
        }
        // Portals (headwalls) at the ends of covered stretches.
        for (c0, c1) in covered {
            for (at, dir) in [(*c0, 1.0f32), (*c1, -1.0)] {
                if at <= 1.0 || at >= len - 1.0 {
                    continue;
                }
                let (p, d) = geo::point_on(&pts, &cum, at);
                let k = cum.partition_point(|c| *c <= at).clamp(1, n - 1);
                let z = s.centre[k][2];
                let nat = natural.height_at(p);
                let top = nat.max(z + 8.0) + 0.6;
                let yaw = super::super::ctx::yaw_of(p, geom::add(p, d));
                let c = geom::sub(p, geom::scale(d, dir * 0.5));
                out.push(box_on(
                    FeatureKind::Tunnel,
                    Material::Concrete,
                    c,
                    z + 6.0,
                    [width + 2.0 * 3.0, 1.0, top - z - 6.0],
                    yaw,
                    true,
                ));
            }
        }
        let _ = (half, names);
    }
    // ---- Named bridges: Iron Bridge truss, Kettle Street Drawbridge towers ----
    for (si, s) in built.surfaces.iter().enumerate() {
        let r = s.road as usize;
        let name = names[r].as_str();
        if info[r].kind != RoadKind::Street {
            continue;
        }
        let pts: Vec<V2> = s.centre.iter().map(|q| geom::xy(*q)).collect();
        let n = pts.len();
        if n < 2 {
            continue;
        }
        let over_water = |p: V2| natural.height_at(p) < plan::WATER_Z;
        let wet: Vec<usize> = (0..n).filter(|k| over_water(pts[*k])).collect();
        if wet.is_empty() {
            continue;
        }
        let (k0, k1) = (wet[0], wet[wet.len() - 1]);
        match name {
            "Old Post Road" => {
                // Truss over the whole river span: posts every ~12 m on both sides, top chord
                // at deck + 7, diagonals, lateral bracing overhead.
                let cum = cum2(&pts);
                let (s0, s1) = (cum[k0.saturating_sub(1)], cum[(k1 + 1).min(n - 1)]);
                let mut prev: Option<(V3, V3)> = None;
                let mut sa = s0;
                while sa <= s1 + 0.1 {
                    let (p, d) = geo::point_on(&pts, &cum, sa);
                    let k = cum.partition_point(|c| *c <= sa).clamp(1, n - 1);
                    let z = s.centre[k][2];
                    let rt = geom::right_of(d);
                    let yaw = super::super::ctx::yaw_of(p, geom::add(p, d));
                    let mut tops = Vec::new();
                    for side in [-1.0f32, 1.0] {
                        let edge = if side > 0.0 { s.right_m } else { s.left_m };
                        let c = geom::add(p, geom::scale(rt, side * (edge + 0.5)));
                        out.push(box_on(
                            FeatureKind::BridgeStructure,
                            Material::Rust,
                            c,
                            z - 1.0,
                            [0.4, 0.4, 8.0],
                            yaw,
                            true,
                        ));
                        tops.push([c[0], c[1], z + 7.0]);
                    }
                    // Overhead lateral beam.
                    let mid = geom::scale(geom::add(geom::xy(tops[0]), geom::xy(tops[1])), 0.5);
                    out.push(box_on(
                        FeatureKind::BridgeStructure,
                        Material::Rust,
                        mid,
                        z + 6.6,
                        [s.left_m + s.right_m + 1.4, 0.3, 0.4],
                        yaw,
                        true,
                    ));
                    if let Some((pl, pr)) = prev {
                        for (a, b) in [(pl, tops[0]), (pr, tops[1])] {
                            // Top chord and a diagonal down to the deck.
                            out.push(ramp(
                                FeatureKind::BridgeStructure,
                                Material::Rust,
                                a,
                                b,
                                0.4,
                                0.4,
                            ));
                            let low = [a[0], a[1], z + 0.4];
                            out.push(ramp(
                                FeatureKind::BridgeStructure,
                                Material::Rust,
                                low,
                                b,
                                0.25,
                                0.25,
                            ));
                        }
                    }
                    prev = Some((tops[0], tops[1]));
                    sa += 12.0;
                }
            }
            "Kettle Street" => {
                // Bascule towers at both ends of the lifting span (mid river), counterweight
                // boxes beside them.
                let cum = cum2(&pts);
                let smid = 0.5 * (cum[k0] + cum[k1]);
                for off in [-22.0f32, 22.0] {
                    let (p, d) = geo::point_on(&pts, &cum, smid + off);
                    let k = cum.partition_point(|c| *c <= smid + off).clamp(1, n - 1);
                    let z = s.centre[k][2];
                    let rt = geom::right_of(d);
                    let yaw = super::super::ctx::yaw_of(p, geom::add(p, d));
                    for side in [-1.0f32, 1.0] {
                        let edge = if side > 0.0 { s.right_m } else { s.left_m };
                        let c = geom::add(p, geom::scale(rt, side * (edge + 2.5)));
                        out.push(box_on(
                            FeatureKind::BridgeStructure,
                            Material::Steel,
                            c,
                            plan::WATER_Z - 2.0,
                            [3.5, 4.0, z + 16.0 - plan::WATER_Z],
                            yaw,
                            true,
                        ));
                        let cw = geom::add(c, geom::scale(d, -off.signum() * 5.0));
                        out.push(box_on(
                            FeatureKind::BridgeStructure,
                            Material::DarkConcrete,
                            cw,
                            z + 7.0,
                            [3.0, 5.0, 4.0],
                            yaw,
                            true,
                        ));
                    }
                    // Overhead lifting gear beam (above the envelope).
                    out.push(box_on(
                        FeatureKind::BridgeStructure,
                        Material::Steel,
                        p,
                        z + 14.0,
                        [s.left_m + s.right_m + 9.0, 2.0, 1.5],
                        yaw,
                        true,
                    ));
                }
            }
            _ => {}
        }
        let _ = si;
    }
    // ---- Sign gantries over carriageways ----
    let mut n_gantry = 0;
    for s in &built.surfaces {
        let r = s.road as usize;
        if info[r].kind != RoadKind::Carriageway || !s.median_barrier {
            continue;
        }
        let pts: Vec<V2> = s.centre.iter().map(|q| geom::xy(*q)).collect();
        let cum = cum2(&pts);
        let n = pts.len();
        let len = cum[n - 1];
        if len < 160.0 {
            continue;
        }
        // Near the end of every piece (before the next gore) and every ~900 m.
        let mut at = vec![len - 120.0];
        let mut x = 600.0;
        while x < len - 400.0 {
            at.push(x);
            x += 900.0;
        }
        for sa in at {
            let (p, d) = geo::point_on(&pts, &cum, sa);
            let k = cum.partition_point(|c| *c <= sa).clamp(1, n - 1);
            let z = s.centre[k][2];
            let rt = geom::right_of(d);
            let yaw = super::super::ctx::yaw_of(p, geom::add(p, d));
            let ground = terrain.height_at(p);
            let elevated = z - ground > 3.0;
            // Span both carriageways (the barrier is this piece's left edge; the other
            // carriageway mirrors this one beyond it).
            let full = s.left_m + s.right_m;
            let mut legs = Vec::new();
            for lat in [s.right_m + 0.8, -s.left_m - full - 0.8] {
                let c = geom::add(p, geom::scale(rt, lat));
                let z0 = if elevated {
                    z - 0.2
                } else {
                    terrain.height_at(c) - 0.3
                };
                legs.push(box_on(
                    FeatureKind::RoadFurniture,
                    Material::Steel,
                    c,
                    z0,
                    [0.5, 0.5, z + 7.6 - z0],
                    yaw,
                    true,
                ));
            }
            let mid = geom::add(p, geom::scale(rt, 0.5 * (s.right_m - s.left_m - full)));
            let beam = box_on(
                FeatureKind::RoadFurniture,
                Material::Steel,
                mid,
                z + 6.4,
                [2.0 * full + 2.2, 0.6, 1.2],
                yaw,
                true,
            );
            let sign = box_on(
                FeatureKind::RoadFurniture,
                Material::Green,
                geom::add(
                    geom::add(p, geom::scale(rt, 0.5 * (s.right_m - s.left_m))),
                    geom::scale(d, -0.4),
                ),
                z + 6.2,
                [full * 0.8, 0.15, 2.2],
                yaw,
                false,
            );
            let all = [legs[0].clone(), legs[1].clone(), beam.clone()];
            if all.iter().all(|f| envelope_clear(&lg, f)) {
                for f in all {
                    out.push(f);
                }
                out.push(sign);
                n_gantry += 1;
            }
        }
    }
    // ---- Bluff Road sea-side guard rail ----
    let mut n_rail = 0;
    for s in &built.surfaces {
        let r = s.road as usize;
        if names[r] != "Bluff Road" {
            continue;
        }
        for k in 0..s.centre.len().saturating_sub(1) {
            let (a, b) = (geom::xy(s.centre[k]), geom::xy(s.centre[k + 1]));
            let seg = geom::dist(a, b);
            if seg < 0.5 {
                continue;
            }
            let d = geom::normalize(geom::sub(b, a));
            let rt = geom::right_of(d);
            let mid = geom::scale(geom::add(a, b), 0.5);
            // Sea side = the lower side.
            let gl = terrain.height_at(geom::sub(mid, geom::scale(rt, 25.0)));
            let gr = terrain.height_at(geom::add(mid, geom::scale(rt, 25.0)));
            let side = if gr < gl { 1.0 } else { -1.0 };
            if (gl - gr).abs() < 2.0 {
                continue;
            }
            let edge = if side > 0.0 { s.right_m } else { s.left_m };
            let c = geom::add(mid, geom::scale(rt, side * (edge + 0.6)));
            let z = 0.5 * (s.centre[k][2] + s.centre[k + 1][2]);
            out.push(box_on(
                FeatureKind::GuardRail,
                Material::Steel,
                c,
                z - 0.2,
                [0.3, seg + 0.2, 1.0],
                super::super::ctx::yaw_of(a, b),
                true,
            ));
            n_rail += 1;
        }
    }
    notes.push(format!(
        "roads: structures: {n_piers} piers, {n_tunnel} covered tunnel pieces ({:.2} km), {n_gantry} sign gantries, {n_rail} cliff guard-rail pieces, {} solid pieces dropped for clearance",
        covered_m / 1000.0,
        out.dropped
    ));
    out.v
}
