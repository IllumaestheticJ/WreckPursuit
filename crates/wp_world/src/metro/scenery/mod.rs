//! Port Vesper v3 — scenery and boundaries: parks and forest, street trees, lamps, poles,
//! street furniture, water-edge railings, cliff guard rails, highway sound walls and fences,
//! map-edge walls — everything that keeps cars out of places not meant to be driven.
//!
//! **Owned by the v3 scenery workstream.** Runs last in the pipeline (`metro::generate`), so
//! everything else is already in the [`Ctx`]. Sections, in order:
//!
//! 1. [`bounds`]: water edges (quay walls / railings / rock revetment along every bank,
//!    traced from the terrain with marching squares), cliff-top walls (Bayview Bluffs), highway
//!    edges (sound walls by housing, fences elsewhere, guard rails on embankments, cuttings and
//!    decks, tall lamps, billboards), map-edge walls, and road-closed barriers with a tunnel
//!    portal wherever a road or rail line leaves the map;
//! 2. [`street`]: streetscape along every kerbed street (trees, lamps, utility poles,
//!    hydrants, benches, bins, bus shelters, billboards, junction sign posts), clear of
//!    junction boxes and site gates, thinned to a feature budget;
//! 3. [`nature`]: the plan's `Park` sites (lawns, paths, trees, benches, lamps, playgrounds,
//!    bandstand, monument, promenade, beach with boardwalk and lifeguard towers), forest on
//!    steep ground and in the forest districts (dense undergrowth along its edge so it cannot be
//!    driven into), and sparse trees on the remaining open land.
//!
//! Placement rules: nothing ever goes on `Occ::Road` cells (cell centres inside a feature's
//! footprint), except the map-edge road closures (Jersey barriers, sign, portal: kinds
//! `JerseyBarrier` / `RoadFurniture` / `Tunnel`, all within 30 m of the map edge) and overhead
//! parts at least 3 m above the ground (lamp heads, decks' guard rails over a road below).
//! Water barriers, highway fences / rails and street furniture may stand on `Occ::Verge`;
//! everything else only on free land or inside this module's own `Park` sites. Other
//! modules' sites (`Occ::Site`) are never entered.
//!
//! Randomness: one seed from `ctx.rng(SALT)`, hashed per grid cell / station, so a change in
//! one place never reshuffles the rest.

mod bounds;
mod contour;
mod nature;
mod street;

use super::ctx::{Ctx, Occ};
use super::ground::{inside, Earthwork, Natural};
use super::plan::{Plan, Site, SiteKind, MAP_HI, MAP_LO};
use crate::geom::{self, V2};
use crate::network::{DistrictKind, Feature, FeatureKind};

/// RNG salt of this module.
pub const SALT: u64 = 0x5C;

/// Ground levelling this module needs before roads are profiled (none).
pub fn earthworks(_plan: &Plan, _nat: &Natural) -> Vec<Earthwork> {
    Vec::new()
}

/// Place this module's content (see the module docs).
pub fn generate(ctx: &mut Ctx) {
    let feats = build(ctx);
    for f in feats {
        if f.solid {
            ctx.place(f);
        } else {
            ctx.add(f);
        }
    }
}

/// Compute every feature of this module against the current context (nothing placed yet).
fn build(ctx: &Ctx) -> Vec<Feature> {
    let env = Env::new(ctx);
    let mut out = Vec::new();
    bounds::water_edges(&env, &mut out);
    bounds::cliffs(&env, &mut out);
    bounds::highways(&env, &mut out);
    bounds::map_edge(&env, &mut out);
    street::streetscape(&env, &mut out);
    nature::parks(&env, &mut out);
    let fm = nature::forest_mask(&env);
    nature::forest(&env, &fm, &mut out);
    nature::scattered(&env, &fm, &mut out);
    // Final guard: nothing on road paving unless it is overhead.
    out.retain(|f| off_road(ctx, f));
    let mut out = join_runs(ctx, out);
    // The one exception: closures where roads leave the map.
    bounds::closures(&env, &mut out);
    out
}

/// Longest barrier box [`join_runs`] makes, m.
const JOIN_MAX_M: f32 = 30.0;
/// Largest sideways offset of a joined piece's ends from the joined box's axis, m.
const JOIN_TOL_M: f32 = 0.15;

/// Barrier kinds whose consecutive straight pieces are joined into longer boxes.
fn joinable(k: FeatureKind) -> bool {
    use FeatureKind as K;
    matches!(
        k,
        K::GuardRail | K::SoundWall | K::Fence | K::QuayWall | K::Hedge | K::Wall | K::MapEdge
    )
}

/// A flat (unpitched) barrier box as a segment: ends `a → b`, half thickness, z range.
#[derive(Clone, Copy)]
struct Seg {
    a: V2,
    b: V2,
    half_w: f32,
    lo: f32,
    hi: f32,
}

fn seg_of(f: &Feature) -> Option<Seg> {
    if !joinable(f.kind) {
        return None;
    }
    match f.shape {
        crate::network::FeatureShape::Box {
            centre,
            half,
            yaw,
            pitch: 0.0,
        } => {
            let (s, c) = wp_math::sin_cos(yaw);
            let d = [-s * half[1], c * half[1]];
            Some(Seg {
                a: [centre[0] - d[0], centre[1] - d[1]],
                b: [centre[0] + d[0], centre[1] + d[1]],
                half_w: half[0],
                lo: centre[2] - half[2],
                hi: centre[2] + half[2],
            })
        }
        _ => None,
    }
}

/// Joins runs of consecutive barrier pieces (same kind, material, thickness; end to start;
/// nearly collinear; similar heights) into boxes up to [`JOIN_MAX_M`] long, which keeps the
/// collider count down without opening gaps. A joined box that would touch road paving is
/// left as its pieces.
fn join_runs(ctx: &Ctx, feats: Vec<Feature>) -> Vec<Feature> {
    let mut out: Vec<Feature> = Vec::with_capacity(feats.len());
    let mut run: Vec<(Feature, Seg)> = Vec::new();
    let flush = |run: &mut Vec<(Feature, Seg)>, out: &mut Vec<Feature>| {
        if run.len() >= 2 {
            let (f0, s0) = &run[0];
            let b = run[run.len() - 1].1.b;
            let lo = run.iter().fold(f32::MAX, |m, (_, s)| m.min(s.lo));
            let hi = run.iter().fold(f32::MIN, |m, (_, s)| m.max(s.hi));
            let joined = super::ctx::box_on(
                f0.kind,
                f0.material,
                [(s0.a[0] + b[0]) * 0.5, (s0.a[1] + b[1]) * 0.5],
                lo,
                [2.0 * s0.half_w, geom::dist(s0.a, b), hi - lo],
                super::ctx::yaw_of(s0.a, b),
                f0.solid,
            );
            if off_road(ctx, &joined) {
                out.push(joined);
                run.clear();
                return;
            }
        }
        out.extend(run.drain(..).map(|(f, _)| f));
    };
    for f in feats {
        let Some(s) = seg_of(&f) else {
            flush(&mut run, &mut out);
            out.push(f);
            continue;
        };
        let fits = run.first().is_some_and(|(f0, s0)| {
            let last = run[run.len() - 1].1;
            let axis = geom::sub(s.b, s0.a);
            let len = geom::dist(s.b, s0.a);
            if f0.kind != f.kind
                || f0.material != f.material
                || f0.solid != f.solid
                || (s0.half_w - s.half_w).abs() > 1e-3
                || geom::dist(last.b, s.a) > 0.8
                || !(1e-3..=JOIN_MAX_M).contains(&len)
            {
                return false;
            }
            let u = geom::scale(axis, 1.0 / len);
            let off = |p: V2| {
                let q = geom::sub(p, s0.a);
                (q[0] * u[1] - q[1] * u[0]).abs()
            };
            let tall = run.iter().fold(s.hi - s.lo, |m, (_, x)| m.max(x.hi - x.lo));
            let lo = run.iter().fold(s.lo, |m, (_, x)| m.min(x.lo));
            let hi = run.iter().fold(s.hi, |m, (_, x)| m.max(x.hi));
            run.iter()
                .map(|(_, x)| x)
                .chain(std::iter::once(&s))
                .all(|x| off(x.a) <= JOIN_TOL_M && off(x.b) <= JOIN_TOL_M)
                && hi - lo <= tall + 0.5
        });
        if !fits {
            flush(&mut run, &mut out);
        }
        run.push((f, s));
    }
    flush(&mut run, &mut out);
    out
}

/// Clearance of solid street items (lamps, trees, poles, furniture, signs) from the edge
/// of every lane and junction connector, m.
pub(crate) const STREET_CLEAR_M: f32 = 1.8;
/// Clearance of solid street items from a junction box (beyond its radius), m.
pub(crate) const JUNCTION_CLEAR_M: f32 = 8.0;

/// Minimum height above the ground of a feature allowed over road paving, m.
pub(crate) const OVERHEAD_M: f32 = 3.0;

/// No road cell (2 m occupancy cell whose centre lies inside the footprint) under `f`,
/// unless `f`'s bottom is at least [`OVERHEAD_M`] above the ground there.
fn off_road(ctx: &Ctx, f: &Feature) -> bool {
    let fp = super::ctx::footprint(&f.shape);
    let bottom = bottom_z(f);
    let (lo, hi) = super::ground::bounds(&fp);
    let c = super::ctx::OCC_CELL_M;
    let i0 = ((lo[0] - MAP_LO[0]) / c - 0.5).ceil() as i64;
    let i1 = ((hi[0] - MAP_LO[0]) / c - 0.5).floor() as i64;
    let j0 = ((lo[1] - MAP_LO[1]) / c - 0.5).ceil() as i64;
    let j1 = ((hi[1] - MAP_LO[1]) / c - 0.5).floor() as i64;
    for j in j0..=j1 {
        for i in i0..=i1 {
            let p = [
                MAP_LO[0] + (i as f32 + 0.5) * c,
                MAP_LO[1] + (j as f32 + 0.5) * c,
            ];
            if inside(&fp, p) && ctx.occ_at(p) == Occ::Road && bottom < ctx.ground_z(p) + OVERHEAD_M
            {
                return false;
            }
        }
    }
    true
}

/// Lowest point of a feature, m.
fn bottom_z(f: &Feature) -> f32 {
    match &f.shape {
        crate::network::FeatureShape::Box {
            centre,
            half,
            pitch,
            ..
        } => {
            let (sp, cp) = wp_math::sin_cos(*pitch);
            centre[2] - half[2] * cp.abs() - half[1] * sp.abs()
        }
        crate::network::FeatureShape::Cylinder { base, .. } => base[2],
        crate::network::FeatureShape::Prism { base_z, .. } => *base_z,
    }
}

// ---------------------------------------------------------------------------------------
// Shared environment: lookups the sections need.
// ---------------------------------------------------------------------------------------

/// Uniform bucket grid over the map holding item ids by bounding box.
struct Index {
    cell: f32,
    nx: usize,
    ny: usize,
    b: Vec<Vec<u32>>,
}

impl Index {
    fn new(cell: f32) -> Self {
        let nx = ((MAP_HI[0] - MAP_LO[0]) / cell).ceil() as usize + 1;
        let ny = ((MAP_HI[1] - MAP_LO[1]) / cell).ceil() as usize + 1;
        Self {
            cell,
            nx,
            ny,
            b: vec![Vec::new(); nx * ny],
        }
    }

    fn range(&self, lo: V2, hi: V2) -> (usize, usize, usize, usize) {
        let f =
            |v: f32, o: f32, n: usize| (((v - o) / self.cell).floor().max(0.0) as usize).min(n - 1);
        (
            f(lo[0], MAP_LO[0], self.nx),
            f(lo[1], MAP_LO[1], self.ny),
            f(hi[0], MAP_LO[0], self.nx),
            f(hi[1], MAP_LO[1], self.ny),
        )
    }

    fn insert(&mut self, id: u32, lo: V2, hi: V2) {
        let (i0, j0, i1, j1) = self.range(lo, hi);
        for j in j0..=j1 {
            for i in i0..=i1 {
                self.b[j * self.nx + i].push(id);
            }
        }
    }

    /// Candidate ids whose boxes may touch `lo..hi` (sorted, unique).
    fn query(&self, lo: V2, hi: V2, out: &mut Vec<u32>) {
        out.clear();
        let (i0, j0, i1, j1) = self.range(lo, hi);
        for j in j0..=j1 {
            for i in i0..=i1 {
                out.extend_from_slice(&self.b[j * self.nx + i]);
            }
        }
        out.sort_unstable();
        out.dedup();
    }
}

/// Lookups shared by all sections.
struct Env<'c, 'a> {
    ctx: &'c Ctx<'a>,
    /// Module seed (hashed per cell / station).
    seed: u64,
    /// Other modules' sites: bounds and polygon.
    foreign: Vec<(V2, V2, &'a [V2])>,
    /// Footprints of earlier barrier-like features (roads' and other modules').
    bar_polys: Vec<Vec<V2>>,
    bar_idx: Index,
    /// Junction centres and radii.
    junctions: Vec<(V2, f32)>,
    junc_idx: Index,
    /// Site gates: position, clearance radius.
    gates: Vec<(V2, f32)>,
    /// Centreline segments of every surface: (surface, segment index).
    surf_segs: Vec<(u32, u32)>,
    surf_idx: Index,
    /// Every lane and junction connector as 2D segments: ends, half width, mean height.
    lane_segs: Vec<(V2, V2, f32, f32)>,
    lane_idx: Index,
}

impl<'c, 'a> Env<'c, 'a> {
    fn new(ctx: &'c Ctx<'a>) -> Self {
        let mut seed_rng = ctx.rng(SALT);
        let seed = seed_rng.next_u64();
        let foreign = ctx
            .plan
            .sites
            .iter()
            .filter(|s| s.kind != SiteKind::Park)
            .map(|s| {
                let (lo, hi) = super::ground::bounds(&s.polygon);
                (lo, hi, s.polygon.as_slice())
            })
            .collect();
        let mut bar_polys = Vec::new();
        let mut bar_idx = Index::new(32.0);
        for f in ctx.roads.features.iter().chain(ctx.features.iter()) {
            if is_barrier(f.kind) && f.solid {
                let fp = super::ctx::footprint(&f.shape);
                let (lo, hi) = super::ground::bounds(&fp);
                bar_idx.insert(bar_polys.len() as u32, lo, hi);
                bar_polys.push(fp);
            }
        }
        let b = &ctx.roads.built;
        let mut junctions = Vec::new();
        let mut junc_idx = Index::new(64.0);
        for it in &b.intersections {
            let p = geom::xy(b.nodes[it.node as usize].pos);
            let r = it.radius_m.max(6.0);
            junc_idx.insert(
                junctions.len() as u32,
                [p[0] - r, p[1] - r],
                [p[0] + r, p[1] + r],
            );
            junctions.push((p, r));
        }
        let gates = ctx
            .plan
            .sites
            .iter()
            .flat_map(|s| s.gates.iter().map(|g| (g.pos, g.width_m * 0.5 + 8.0)))
            .collect();
        let mut surf_segs = Vec::new();
        let mut surf_idx = Index::new(32.0);
        for (si, s) in b.surfaces.iter().enumerate() {
            let w = s.left_m.max(s.right_m) + 1.0;
            for k in 0..s.centre.len().saturating_sub(1) {
                let (p, q) = (geom::xy(s.centre[k]), geom::xy(s.centre[k + 1]));
                let lo = [p[0].min(q[0]) - w, p[1].min(q[1]) - w];
                let hi = [p[0].max(q[0]) + w, p[1].max(q[1]) + w];
                surf_idx.insert(surf_segs.len() as u32, lo, hi);
                surf_segs.push((si as u32, k as u32));
            }
        }
        // Lanes and the connector curves cars follow through junctions (as in `CityMap`).
        let mut lane_segs = Vec::new();
        let mut lane_idx = Index::new(32.0);
        let mut add_path = |pts: &[[f32; 3]], half: f32| {
            for w in pts.windows(2) {
                let (p, q) = (geom::xy(w[0]), geom::xy(w[1]));
                let lo = [p[0].min(q[0]) - half, p[1].min(q[1]) - half];
                let hi = [p[0].max(q[0]) + half, p[1].max(q[1]) + half];
                lane_idx.insert(lane_segs.len() as u32, lo, hi);
                lane_segs.push((p, q, half, 0.5 * (w[0][2] + w[1][2])));
            }
        };
        for l in &b.lanes {
            if l.polyline.len() < 2 {
                continue;
            }
            add_path(&l.polyline, l.width_m * 0.5);
            for c in &l.connections {
                let to = &b.lanes[c.to as usize];
                if to.polyline.len() < 2 {
                    continue;
                }
                let pts = crate::map::connector_curve(&l.polyline, &to.polyline, c.turn);
                add_path(&pts, 0.5 * l.width_m.max(to.width_m));
            }
        }
        Self {
            ctx,
            seed,
            foreign,
            bar_polys,
            bar_idx,
            junctions,
            junc_idx,
            gates,
            surf_segs,
            surf_idx,
            lane_segs,
            lane_idx,
        }
    }

    fn plan(&self) -> &'a Plan {
        self.ctx.plan
    }

    /// Deterministic value in `0..1` for a grid cell / station and a purpose tag.
    fn h01(&self, a: i64, b: i64, tag: u64) -> f32 {
        (self.hash(a, b, tag) >> 40) as f32 / (1u64 << 24) as f32
    }

    fn hash(&self, a: i64, b: i64, tag: u64) -> u64 {
        splitmix(
            self.seed
                ^ (a as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ (b as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
                ^ tag.wrapping_mul(0x1656_67B1_9E37_79F9),
        )
    }

    /// Inside another module's site.
    fn foreign_site(&self, p: V2) -> bool {
        self.foreign.iter().any(|(lo, hi, poly)| {
            p[0] >= lo[0] && p[0] <= hi[0] && p[1] >= lo[1] && p[1] <= hi[1] && inside(poly, p)
        })
    }

    /// District kind and name at `p`.
    fn zone(&self, p: V2) -> Option<(DistrictKind, &'static str)> {
        self.plan()
            .zones
            .iter()
            .find(|z| inside(&z.zone.polygon, p))
            .map(|z| (z.zone.kind, z.name))
    }

    /// An earlier barrier feature lies within `d` of `p`.
    fn near_barrier(&self, p: V2, d: f32) -> bool {
        let mut ids = Vec::new();
        self.bar_idx
            .query([p[0] - d, p[1] - d], [p[0] + d, p[1] + d], &mut ids);
        ids.iter()
            .any(|&k| super::ground::signed_dist(&self.bar_polys[k as usize], p) <= d)
    }

    /// Within `extra` m of a junction box.
    fn near_junction(&self, p: V2, extra: f32) -> bool {
        let mut ids = Vec::new();
        let d = extra + 1.0;
        self.junc_idx
            .query([p[0] - d, p[1] - d], [p[0] + d, p[1] + d], &mut ids);
        ids.iter().any(|&k| {
            let (c, r) = self.junctions[k as usize];
            geom::dist(c, p) < r + extra
        })
    }

    /// Within the clearance of a site gate.
    fn near_gate(&self, p: V2) -> bool {
        self.gates.iter().any(|(g, r)| geom::dist(*g, p) < *r)
    }

    /// `q` lies on the paving of a surface (other than `exclude`) whose height there is
    /// within `ztol` of `z`.
    fn on_paving(&self, q: V2, z: f32, ztol: f32, exclude: Option<usize>) -> bool {
        let mut ids = Vec::new();
        self.surf_idx.query(q, q, &mut ids);
        let surfs = &self.ctx.roads.built.surfaces;
        ids.iter().any(|&id| {
            let (si, k) = self.surf_segs[id as usize];
            if Some(si as usize) == exclude {
                return false;
            }
            let s = &surfs[si as usize];
            let (a, b) = (s.centre[k as usize], s.centre[k as usize + 1]);
            let ab = geom::sub(geom::xy(b), geom::xy(a));
            let l2 = geom::dot(ab, ab);
            if l2 < 1e-6 {
                return false;
            }
            let t = geom::dot(geom::sub(q, geom::xy(a)), ab) / l2;
            if !(-0.02..=1.02).contains(&t) {
                return false;
            }
            let dir = geom::scale(ab, 1.0 / l2.sqrt());
            let lat = geom::dot(geom::sub(q, geom::xy(a)), geom::right_of(dir));
            if lat < -s.left_m - 0.3 || lat > s.right_m + 0.3 {
                return false;
            }
            let bank = s.bank.get(k as usize).copied().unwrap_or(0.0);
            let zs = a[2] + (b[2] - a[2]) * t.clamp(0.0, 1.0) + bank * lat;
            (zs - z).abs() < ztol
        })
    }

    /// Smallest gap between any of `pts` and the edge of a lane or junction connector whose
    /// height is within 4 m of `z` (capped at `within`), m. Negative = inside a lane.
    fn lane_gap(&self, pts: &[V2], z: f32, within: f32) -> f32 {
        let (lo, hi) = super::ground::bounds(pts);
        let d = within + 3.0;
        let mut ids = Vec::new();
        self.lane_idx
            .query([lo[0] - d, lo[1] - d], [hi[0] + d, hi[1] + d], &mut ids);
        let mut gap = within;
        for id in ids {
            let (a, b, half, zl) = self.lane_segs[id as usize];
            if (zl - z).abs() > 4.0 {
                continue;
            }
            for p in pts {
                gap = gap.min(geom::point_segment_distance(*p, a, b) - half);
            }
        }
        gap
    }

    /// Every solid part of `fs` keeps [`STREET_CLEAR_M`] from every lane and connector.
    fn clear_of_lanes(&self, fs: &[Feature]) -> bool {
        fs.iter().filter(|f| f.solid).all(|f| {
            let fp = super::ctx::footprint(&f.shape);
            self.lane_gap(&fp, bottom_z(f), STREET_CLEAR_M + 1.0) >= STREET_CLEAR_M
        })
    }

    /// Adds the item `fs` (one tree, lamp, bench...) when it is [`Env::clear_of_lanes`].
    fn push_clear(&self, out: &mut Vec<Feature>, fs: Vec<Feature>) {
        if self.clear_of_lanes(&fs) {
            out.extend(fs);
        }
    }

    /// Occupancy at `p`.
    fn occ(&self, p: V2) -> Occ {
        self.ctx.occ_at(p)
    }

    /// Ground height.
    fn gz(&self, p: V2) -> f32 {
        self.ctx.ground_z(p)
    }
}

fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Kinds that already block cars (used to avoid doubling another module's wall).
fn is_barrier(k: FeatureKind) -> bool {
    use FeatureKind::*;
    matches!(
        k,
        Wall | Fence
            | Hedge
            | JerseyBarrier
            | GuardRail
            | SoundWall
            | MapEdge
            | QuayWall
            | Pier
            | ConstructionFence
            | Tunnel
            | BridgeStructure
            | Hangar
            | Terminal
            | Landmark
    )
}

/// This module's park sites.
fn park_sites(plan: &Plan) -> impl Iterator<Item = &Site> {
    plan.sites_of(SiteKind::Park)
}

/// Keep at most `cap` items, spread evenly over the list (every k-th).
fn thin<T>(v: Vec<T>, cap: usize) -> Vec<T> {
    if v.len() <= cap || cap == 0 {
        return if cap == 0 { Vec::new() } else { v };
    }
    let n = v.len();
    v.into_iter()
        .enumerate()
        .filter(|(i, _)| (i * cap) / n != ((i + 1) * cap) / n)
        .map(|(_, x)| x)
        .collect()
}

#[cfg(test)]
mod tests;
