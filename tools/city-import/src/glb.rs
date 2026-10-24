//! Greybox mesh builder and minimal glTF 2.0 binary (`.glb`) writer.
//!
//! Emits one mesh with a primitive per material: road ribbons (asphalt / highway concrete /
//! parking / bus, banked like the road surface), junction boxes, raised sidewalks with kerb
//! faces along ribbon edges, ground per district and extruded buildings. Cities with terrain
//! (Port Vesper v2) also get the terrain mesh (decimated, never above a road), a sea plane,
//! retaining walls where roads sit above the ground, bridge decks with parapets and
//! piers, and tunnels / covered roads (walls, roof deck, emissive ceiling lights). Highways are paved from their road surfaces (lanes plus
//! shoulders) with ramp taper connectors through freeway merges and diverges, and a concrete
//! (Jersey-profile) median barrier between divided carriageways. World space is +Z up; glTF is +Y up, so positions are written as `(x, z, -y)` (a
//! rotation, so triangle winding is preserved). Units are metres. Primitives with more than
//! 65 535 vertices are split so every index buffer is 16-bit.

use serde_json::json;
use wp_world::geom::{self, V2, V3};
use wp_world::map::CityMap;
use wp_world::network::{
    BridgeKind, Building, BuildingKind, DecorKind, DistrictKind, LaneKind, RoadMaterial,
    RoadNetwork, StructureKind, SEA_LEVEL_M,
};
use wp_world::residential;
use wp_world::surface::{
    edge_barriers, inside_other_paving, is_elevated, lane_bank, median_barriers, node_patches,
    sidewalk_mesh, surface_edges, surface_ribbon, TriMesh,
};
use wp_world::terrain::Heightfield;

/// Height of the parking / bus lane tint above the road surface, m.
const LANE_TINT_DZ_M: f32 = 0.01;
/// Sea level drawn as water, m.
const SEA_Z: f32 = SEA_LEVEL_M;

/// Median barrier cross-section (half width, height above the road), m: a Jersey profile.
const BARRIER_PROFILE: [(f32, f32); 4] = [(0.30, 0.0), (0.28, 0.08), (0.18, 0.33), (0.08, 0.81)];

/// Triangle soup for one material.
#[derive(Debug, Clone, Default)]
pub struct Prim {
    /// Material name.
    pub material: String,
    /// Base colour RGBA.
    pub color: [f32; 4],
    /// Positions (world space, +Z up).
    pub positions: Vec<V3>,
    /// Normals (world space); only for smooth-shaded primitives (terrain). Without normals
    /// the testbed flat-shades the faces, which is all flat greybox geometry needs.
    pub normals: Vec<V3>,
    /// Triangle indices.
    pub indices: Vec<u32>,
    /// Store per-vertex normals.
    pub smooth: bool,
}

impl Prim {
    fn new(material: &str, color: [f32; 4]) -> Self {
        Self {
            material: material.into(),
            color,
            ..Default::default()
        }
    }

    fn smooth(material: &str, color: [f32; 4]) -> Self {
        Self {
            smooth: true,
            ..Self::new(material, color)
        }
    }

    fn vert(&mut self, p: V3, n: V3) -> u32 {
        self.positions.push(p);
        if self.smooth {
            self.normals.push(n);
        }
        (self.positions.len() - 1) as u32
    }

    /// Append an indexed mesh: the same triangles and winding, vertices at identical
    /// positions shared.
    fn mesh(&mut self, m: &TriMesh) {
        let mut seen: std::collections::BTreeMap<[u32; 3], u32> = std::collections::BTreeMap::new();
        let ids: Vec<u32> = m
            .positions
            .iter()
            .map(|p| {
                let key = p.map(f32::to_bits);
                *seen
                    .entry(key)
                    .or_insert_with(|| self.vert(*p, [0.0, 0.0, 1.0]))
            })
            .collect();
        for t in &m.indices {
            self.indices.extend_from_slice(&[
                ids[t[0] as usize],
                ids[t[1] as usize],
                ids[t[2] as usize],
            ]);
        }
    }

    /// Up-facing strip between two edge polylines, sharing vertices between quads.
    fn strip(&mut self, left: &[V3], right: &[V3]) {
        let base = self.positions.len() as u32;
        for (l, r) in left.iter().zip(right) {
            self.vert(*l, [0.0, 0.0, 1.0]);
            self.vert(*r, [0.0, 0.0, 1.0]);
        }
        for k in 0..left.len().saturating_sub(1) as u32 {
            let (l0, r0, l1, r1) = (
                base + 2 * k,
                base + 2 * k + 1,
                base + 2 * k + 2,
                base + 2 * k + 3,
            );
            self.indices.extend_from_slice(&[l0, r0, r1, l0, r1, l1]);
        }
    }

    /// Quad `a b c d` (counter-clockwise seen from the normal side).
    fn quad(&mut self, a: V3, b: V3, c: V3, d: V3, n: V3) {
        let i = self.vert(a, n);
        self.vert(b, n);
        self.vert(c, n);
        self.vert(d, n);
        self.indices
            .extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
    }

    /// Flat ribbon of `width` along `pts` at `pts.z + dz`, facing up.
    fn ribbon(&mut self, pts: &[V3], left: f32, right: f32, dz: f32) {
        let mut l = geom::offset_polyline(pts, -left);
        let mut r = geom::offset_polyline(pts, right);
        for v in l.iter_mut().chain(r.iter_mut()) {
            v[2] += dz;
        }
        self.strip(&l, &r);
    }

    /// [`Prim::ribbon`] on a banked road: cross slope `bank[k]` (dz per metre to the right of
    /// the polyline's direction) lifts the right edge and lowers the left one.
    fn ribbon_banked(&mut self, pts: &[V3], left: f32, right: f32, dz: f32, bank: &[f32]) {
        if bank.len() != pts.len() {
            self.ribbon(pts, left, right, dz);
            return;
        }
        let mut l = geom::offset_polyline(pts, -left);
        let mut r = geom::offset_polyline(pts, right);
        for k in 0..pts.len() {
            l[k][2] += dz - bank[k] * left;
            r[k][2] += dz + bank[k] * right;
        }
        self.strip(&l, &r);
    }

    /// Vertical strip hanging from `top` (a polyline) down to `bottom[k]`, facing the right of
    /// the polyline direction when `face_right`. Segments of zero height are left out.
    fn curtain(&mut self, top: &[V3], bottom: &[f32], face_right: bool) {
        let base = self.positions.len() as u32;
        for (t, b) in top.iter().zip(bottom) {
            self.vert(*t, [0.0, 0.0, 1.0]);
            self.vert([t[0], t[1], *b], [0.0, 0.0, 1.0]);
        }
        for k in 0..top.len().saturating_sub(1) {
            if top[k][2] - bottom[k] < 0.05 && top[k + 1][2] - bottom[k + 1] < 0.05 {
                continue;
            }
            let k = k as u32;
            let (t0, b0, t1, b1) = (
                base + 2 * k,
                base + 2 * k + 1,
                base + 2 * k + 2,
                base + 2 * k + 3,
            );
            // Seen from the facing side: top-left, bottom-left, bottom-right, top-right.
            if face_right {
                self.indices.extend_from_slice(&[t0, b0, b1, t0, b1, t1]);
            } else {
                self.indices.extend_from_slice(&[t1, b1, b0, t1, b0, t0]);
            }
        }
    }

    /// Closed prism (walls + roof) over `poly` from `z0` to `z1`, sharing its 2n corner
    /// vertices between faces (flat-shaded primitives only).
    fn prism(&mut self, poly: &[V2], z0: f32, z1: f32) {
        let n = poly.len() as u32;
        let ccw = signed_area(poly) > 0.0;
        let base = self.positions.len() as u32;
        for p in poly {
            self.vert([p[0], p[1], z0], [0.0, 0.0, 1.0]);
            self.vert([p[0], p[1], z1], [0.0, 0.0, 1.0]);
        }
        for i in 0..n {
            let (a, b) = if ccw {
                (i, (i + 1) % n)
            } else {
                ((i + 1) % n, i)
            };
            let (a0, a1, b0, b1) = (
                base + 2 * a,
                base + 2 * a + 1,
                base + 2 * b,
                base + 2 * b + 1,
            );
            self.indices.extend_from_slice(&[a0, b0, b1, a0, b1, a1]);
        }
        for t in triangulate(poly) {
            self.indices.extend_from_slice(&[
                base + 2 * t[0] + 1,
                base + 2 * t[1] + 1,
                base + 2 * t[2] + 1,
            ]);
        }
    }

    /// Axis-free box: footprint `corners` (4, counter-clockwise) from `z0` to `z1`.
    fn block(&mut self, corners: &[V2; 4], z0: f32, z1: f32) {
        self.walls(corners, z0, z1);
        self.polygon(corners, z1);
    }

    /// Closed solid over `poly` from `z0` to `z1` with a downward-facing underside (floating
    /// structures: canopies, tunnel ceilings, crane booms).
    fn solid(&mut self, poly: &[V2], z0: f32, z1: f32) {
        self.walls(poly, z0, z1);
        self.polygon(poly, z1);
        let base = self.positions.len() as u32;
        for p in poly {
            self.vert([p[0], p[1], z0], [0.0, 0.0, -1.0]);
        }
        for t in triangulate(poly) {
            self.indices
                .extend_from_slice(&[base + t[0], base + t[2], base + t[1]]);
        }
    }

    /// Upward-facing polygon at height `z` (ear clipping; handles concave footprints).
    fn polygon(&mut self, poly: &[V2], z: f32) {
        let tris = triangulate(poly);
        let base = self.positions.len() as u32;
        for p in poly {
            self.vert([p[0], p[1], z], [0.0, 0.0, 1.0]);
        }
        for t in tris {
            self.indices
                .extend_from_slice(&[base + t[0], base + t[1], base + t[2]]);
        }
    }

    /// Vertical walls of an extruded polygon from `z0` to `z1`.
    fn walls(&mut self, poly: &[V2], z0: f32, z1: f32) {
        let ccw = signed_area(poly) > 0.0;
        let n = poly.len();
        for i in 0..n {
            let (a, b) = if ccw {
                (poly[i], poly[(i + 1) % n])
            } else {
                (poly[(i + 1) % n], poly[i])
            };
            let d = geom::normalize(geom::sub(b, a));
            let out = geom::right_of(d);
            self.quad(
                [a[0], a[1], z0],
                [b[0], b[1], z0],
                [b[0], b[1], z1],
                [a[0], a[1], z1],
                [out[0], out[1], 0.0],
            );
        }
    }
}

fn signed_area(p: &[V2]) -> f32 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (u, v) = (p[i], p[(i + 1) % p.len()]);
        a += u[0] * v[1] - v[0] * u[1];
    }
    a * 0.5
}

/// Ear-clipping triangulation; returns CCW triangles as polygon vertex indices.
pub fn triangulate(poly: &[V2]) -> Vec<[u32; 3]> {
    let n = poly.len();
    if n < 3 {
        return Vec::new();
    }
    let mut idx: Vec<u32> = (0..n as u32).collect();
    if signed_area(poly) < 0.0 {
        idx.reverse();
    }
    let mut out = Vec::new();
    let mut guard = 0;
    while idx.len() > 3 && guard < n * n {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let (a, b, c) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let (pa, pb, pc) = (poly[a as usize], poly[b as usize], poly[c as usize]);
            if geom::cross(geom::sub(pb, pa), geom::sub(pc, pb)) <= 0.0 {
                continue; // reflex
            }
            let inside = idx.iter().any(|&o| {
                if o == a || o == b || o == c {
                    return false;
                }
                let p = poly[o as usize];
                geom::cross(geom::sub(pb, pa), geom::sub(p, pa)) > 0.0
                    && geom::cross(geom::sub(pc, pb), geom::sub(p, pb)) > 0.0
                    && geom::cross(geom::sub(pa, pc), geom::sub(p, pc)) > 0.0
            });
            if inside {
                continue;
            }
            out.push([a, b, c]);
            idx.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            break; // degenerate polygon: fall back to a fan for the rest
        }
    }
    for k in 1..idx.len().saturating_sub(1) {
        out.push([idx[0], idx[k], idx[k + 1]]);
    }
    out
}

fn district_color(k: DistrictKind) -> [f32; 4] {
    match k {
        DistrictKind::Core => [0.55, 0.52, 0.50, 1.0],
        DistrictKind::Commercial => [0.58, 0.56, 0.50, 1.0],
        DistrictKind::Residential => [0.36, 0.48, 0.32, 1.0],
        DistrictKind::Industrial => [0.47, 0.44, 0.38, 1.0],
        DistrictKind::Port => [0.42, 0.42, 0.44, 1.0],
        DistrictKind::Suburb => [0.40, 0.52, 0.34, 1.0],
    }
}

/// Plain concrete wall along `line` (road height at its foot; `BARRIER_PROFILE`'s base width
/// and height): two sides and the top, nearly straight stretches merged into one piece. Cheaper
/// than [`barrier_mesh`] for the tens of kilometres of highway edge.
fn wall_mesh(p: &mut Prim, line: &[V3]) {
    let (w, h) = (
        BARRIER_PROFILE[0].0,
        BARRIER_PROFILE[BARRIER_PROFILE.len() - 1].1,
    );
    // Keep a point where the line turns by more than ~1°, climbs off the straight, or every 60 m.
    let mut pts: Vec<V3> = vec![line[0]];
    for k in 1..line.len() {
        let last = *pts.last().unwrap();
        let next = line[(k + 1).min(line.len() - 1)];
        let a = geom::normalize(geom::sub(geom::xy(line[k]), geom::xy(last)));
        let b = geom::normalize(geom::sub(geom::xy(next), geom::xy(line[k])));
        let bend = geom::cross(a, b).abs() > 0.017;
        let far = geom::dist(geom::xy(line[k]), geom::xy(last)) > 60.0;
        let t = geom::dist(geom::xy(line[k]), geom::xy(last))
            / geom::dist(geom::xy(next), geom::xy(last)).max(1e-3);
        let climb = (line[k][2] - (last[2] + (next[2] - last[2]) * t)).abs() > 0.03;
        if bend || far || climb || k == line.len() - 1 {
            pts.push(line[k]);
        }
    }
    for q in pts.windows(2) {
        let d = geom::normalize(geom::sub(geom::xy(q[1]), geom::xy(q[0])));
        let r = geom::right_of(d);
        let at =
            |c: V3, side: f32, z: f32| [c[0] + r[0] * side * w, c[1] + r[1] * side * w, c[2] + z];
        let (a, b) = (q[0], q[1]);
        p.quad(
            at(a, 1.0, 0.0),
            at(b, 1.0, 0.0),
            at(b, 1.0, h),
            at(a, 1.0, h),
            [r[0], r[1], 0.0],
        );
        p.quad(
            at(b, -1.0, 0.0),
            at(a, -1.0, 0.0),
            at(a, -1.0, h),
            at(b, -1.0, h),
            [-r[0], -r[1], 0.0],
        );
        p.quad(
            at(a, -1.0, h),
            at(a, 1.0, h),
            at(b, 1.0, h),
            at(b, -1.0, h),
            [0.0, 0.0, 1.0],
        );
    }
}

/// Jersey-profile barrier along `line` (road height at the barrier's foot).
fn barrier_mesh(p: &mut Prim, line: &[V3]) {
    let n = line.len();
    if n < 2 {
        return;
    }
    // Cross-section points per line point: left side bottom → top, then right side.
    let sections: Vec<Vec<V3>> = (0..n)
        .map(|k| {
            let a = geom::xy(line[k.saturating_sub(1)]);
            let b = geom::xy(line[(k + 1).min(n - 1)]);
            let r = geom::right_of(geom::normalize(geom::sub(b, a)));
            let c = line[k];
            let at = |side: f32, w: f32, h: f32| {
                [c[0] + r[0] * side * w, c[1] + r[1] * side * w, c[2] + h]
            };
            BARRIER_PROFILE
                .iter()
                .map(|(w, h)| at(-1.0, *w, *h))
                .chain(BARRIER_PROFILE.iter().map(|(w, h)| at(1.0, *w, *h)))
                .collect()
        })
        .collect();
    let m = BARRIER_PROFILE.len();
    for k in 0..n - 1 {
        let (s0, s1) = (&sections[k], &sections[k + 1]);
        for j in 0..m - 1 {
            // Right side faces +r: along the line is to the viewer's right.
            p.quad(
                s0[m + j],
                s1[m + j],
                s1[m + j + 1],
                s0[m + j + 1],
                [0.0, 0.0, 1.0],
            );
            // Left side faces -r.
            p.quad(s1[j], s0[j], s0[j + 1], s1[j + 1], [0.0, 0.0, 1.0]);
        }
        p.quad(
            s0[m - 1],
            s0[2 * m - 1],
            s1[2 * m - 1],
            s1[m - 1],
            [0.0, 0.0, 1.0],
        );
    }
}

/// Points of the median barriers bucketed on a 10 m grid.
struct BarrierIndex {
    cells: std::collections::BTreeMap<(i32, i32), Vec<(V2, V2)>>,
}

impl BarrierIndex {
    fn key(p: V2) -> (i32, i32) {
        ((p[0] / 10.0).floor() as i32, (p[1] / 10.0).floor() as i32)
    }

    fn new(lines: &[Vec<V3>]) -> Self {
        let mut cells: std::collections::BTreeMap<(i32, i32), Vec<(V2, V2)>> = Default::default();
        for l in lines {
            for w in l.windows(2) {
                let (a, b) = (geom::xy(w[0]), geom::xy(w[1]));
                for p in [a, b] {
                    cells.entry(Self::key(p)).or_default().push((a, b));
                }
            }
        }
        Self { cells }
    }

    /// Within 0.6 m of a barrier line.
    fn near(&self, p: V2) -> bool {
        let (i, j) = Self::key(p);
        (i - 1..=i + 1).any(|x| {
            (j - 1..=j + 1).any(|y| {
                self.cells.get(&(x, y)).is_some_and(|segs| {
                    segs.iter()
                        .any(|(a, b)| geom::point_segment_distance(p, *a, *b) < 0.6)
                })
            })
        })
    }
}

/// Render terrain: `coarse` is [`wp_world::surface::drawn_terrain`] (never above a road).
/// Physics collides with the same surface off the roads (`wp_world::surface::collision_terrain`).
fn render_terrain(
    coarse: &Heightfield,
    net: &RoadNetwork,
    ground: &mut [Prim],
    rock: &mut Prim,
    seabed: &mut Prim,
) {
    let (cx, cy) = (coarse.nx, coarse.ny);
    let z = &coarse.z;
    let pos: Vec<V2> = (0..cy)
        .flat_map(|j| (0..cx).map(move |i| (i, j)))
        .map(|(i, j)| coarse.vertex_pos(i, j))
        .collect();
    let zone_kind = |p: V2| {
        net.zones
            .iter()
            .find(|zn| geom::point_in_polygon(p, &zn.polygon))
            .map_or(DistrictKind::Suburb, |zn| zn.kind)
    };
    // Smooth normals from central differences on the coarse grid.
    let at = |i: u32, j: u32| z[(j.min(cy - 1) * cx + i.min(cx - 1)) as usize];
    let step = coarse.cell_m;
    let normal = |i: u32, j: u32| -> V3 {
        let dx = (at(i + 1, j) - at(i.saturating_sub(1), j)) / (2.0 * step);
        let dy = (at(i, j + 1) - at(i, j.saturating_sub(1))) / (2.0 * step);
        let l = (dx * dx + dy * dy + 1.0).sqrt();
        [-dx / l, -dy / l, 1.0 / l]
    };
    // Per primitive, coarse vertex → local index (shared between the cells it touches).
    let n_prims = ground.len() + 2;
    let mut local: Vec<Vec<u32>> = vec![vec![u32::MAX; (cx * cy) as usize]; n_prims];
    for j in 0..cy - 1 {
        for i in 0..cx - 1 {
            let idx = |a: u32, b: u32| (b * cx + a) as usize;
            let v = [idx(i, j), idx(i + 1, j), idx(i + 1, j + 1), idx(i, j + 1)];
            let ij = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
            let p: Vec<V3> = v.iter().map(|q| [pos[*q][0], pos[*q][1], z[*q]]).collect();
            let centre = [(p[0][0] + p[2][0]) * 0.5, (p[0][1] + p[2][1]) * 0.5];
            let zmax = p.iter().map(|q| q[2]).fold(f32::NEG_INFINITY, f32::max);
            let zmin = p.iter().map(|q| q[2]).fold(f32::INFINITY, f32::min);
            let span = geom::dist(geom::xy(p[0]), geom::xy(p[2]));
            let which = if zmax < SEA_Z {
                ground.len() + 1
            } else if (zmax - zmin) / span > 0.45 {
                ground.len()
            } else {
                zone_kind(centre) as usize
            };
            let prim: &mut Prim = if which == ground.len() + 1 {
                &mut *seabed
            } else if which == ground.len() {
                &mut *rock
            } else {
                &mut ground[which]
            };
            let mut ids = [0u32; 4];
            for c in 0..4 {
                let slot = &mut local[which][v[c]];
                if *slot == u32::MAX {
                    *slot = prim.vert(p[c], normal(ij[c].0, ij[c].1));
                }
                ids[c] = *slot;
            }
            prim.indices
                .extend_from_slice(&[ids[0], ids[1], ids[2], ids[0], ids[2], ids[3]]);
        }
    }
}

/// Bridge piers: a block under every deck about every 30 m (`(footprint, bottom, top)`),
/// 1.6 m deep and 80 % of the deck wide, down to the ground. A pier never stands on a road
/// under its deck (an interchange crossroad over the highway) or beside one within 1.5 m:
/// such sites move on to the next centreline point.
pub fn deck_piers(net: &RoadNetwork) -> Vec<([V2; 4], f32, f32)> {
    let Some(t) = &net.terrain else {
        return Vec::new();
    };
    let ground_z = |q: V2| t.height_at(q);
    let mut out = Vec::new();
    for (si, s) in net.surfaces.iter().enumerate() {
        let n = s.centre.len();
        let deck: Vec<bool> = (0..n).map(|k| is_elevated(s, k, &ground_z, 1.5)).collect();
        let cum = geom::cumulative(&s.centre);
        let mut next_pier = 0.0;
        for k in 0..n.saturating_sub(1) {
            if !(deck[k] && deck[k + 1]) || cum[k] < next_pier {
                continue;
            }
            let c = geom::xy(s.centre[k]);
            let d = geom::normalize(geom::sub(geom::xy(s.centre[k + 1]), c));
            let r = geom::right_of(d);
            let half_w = 0.4 * (s.left_m + s.right_m);
            let corner =
                |a: f32, b: f32| geom::add(c, geom::add(geom::scale(d, a), geom::scale(r, b)));
            let fp = [
                corner(-0.8, -half_w),
                corner(-0.8, half_w),
                corner(0.8, half_w),
                corner(0.8, -half_w),
            ];
            let fp = if signed_area(&fp) < 0.0 {
                [fp[3], fp[2], fp[1], fp[0]]
            } else {
                fp
            };
            let mid = |a: V2, b: V2| geom::scale(geom::add(a, b), 0.5);
            let probes = [
                fp[0],
                fp[1],
                fp[2],
                fp[3],
                c,
                mid(fp[0], fp[1]),
                mid(fp[2], fp[3]),
                mid(c, mid(fp[0], fp[1])),
                mid(c, mid(fp[2], fp[3])),
            ];
            let deck_z = s.centre[k][2];
            if probes.iter().any(|q| on_paving_below(net, si, *q, deck_z)) {
                continue;
            }
            next_pier = cum[k] + 30.0;
            let base = fp
                .iter()
                .map(|q| ground_z(*q))
                .fold(f32::INFINITY, f32::min)
                - 0.5;
            out.push((fp, base, deck_z - 1.2));
        }
    }
    out
}

/// Depth of a bridge deck's slab below its road surface, m (the slab sides, and how far a
/// wall on a deck edge may hang).
const DECK_SLAB_M: f32 = 1.2;

/// Foot height of a wall whose top is at `top`: `foot` (down to the ground), except over the
/// paving of a road below (an overpass's sidewalk back or edge over the road it crosses),
/// where it ends at the deck slab's depth, clear of the traffic underneath (owner row 159:
/// sidewalk-back walls on the interchange crossroads hung down onto Loop 9).
fn wall_foot(net: &RoadNetwork, top: V3, foot: f32) -> f32 {
    if top[2] - foot > DECK_SLAB_M && on_paving_below(net, usize::MAX, geom::xy(top), top[2]) {
        top[2] - DECK_SLAB_M
    } else {
        foot
    }
}

/// Whether `q` lies on (or within 1.5 m of) the paving of a road other than surface `si`, or
/// a junction box, more than 1 m below height `z` (where a bridge pier must not stand).
fn on_paving_below(net: &RoadNetwork, si: usize, q: V2, z: f32) -> bool {
    const MARGIN_M: f32 = 1.5;
    let roads = net.surfaces.iter().enumerate().any(|(oi, o)| {
        if oi == si || o.centre.len() < 2 {
            return false;
        }
        let (a, b) = (
            geom::xy(o.centre[0]),
            geom::xy(o.centre[o.centre.len() - 1]),
        );
        let w = o.left_m.max(o.right_m) + MARGIN_M;
        if geom::dist(q, geom::scale(geom::add(a, b), 0.5)) > geom::dist(a, b) * 0.5 + w + 400.0 {
            return false;
        }
        let cum = geom::cumulative(&o.centre);
        let pr = geom::project(&o.centre, &cum, q);
        let edge = if pr.lateral < 0.0 {
            o.left_m
        } else {
            o.right_m
        };
        let len = cum[cum.len() - 1];
        pr.s >= -MARGIN_M
            && pr.s <= len + MARGIN_M
            && pr.lateral.abs() <= edge + MARGIN_M
            && geom::point_at(&o.centre, &cum, pr.s.clamp(0.0, len)).0[2] < z - 1.0
    });
    roads
        || net.intersections.iter().any(|ix| {
            let n = net.nodes[ix.node as usize].pos;
            ix.radius_m > 0.0
                && n[2] < z - 1.0
                && geom::dist(q, geom::xy(n)) <= ix.radius_m + MARGIN_M
        })
}

/// Build the greybox primitives for a city.
pub fn build_greybox(map: &CityMap) -> Vec<Prim> {
    let net = &map.net;
    let mut asphalt = Prim::new("asphalt", [0.20, 0.20, 0.21, 1.0]);
    let mut concrete = Prim::new("highway_concrete", [0.42, 0.42, 0.40, 1.0]);
    // Unpaved roads (Ashpeak County farm and forest roads): tan-grey gravel, brown dirt.
    let mut gravel = Prim::new("gravel_road", [0.56, 0.52, 0.45, 1.0]);
    let mut dirt = Prim::new("dirt_road", [0.43, 0.32, 0.21, 1.0]);
    let mut parking = Prim::new("parking", [0.26, 0.26, 0.27, 1.0]);
    let mut bus = Prim::new("bus_lane", [0.35, 0.16, 0.14, 1.0]);
    let mut sidewalk = Prim::new("sidewalk", [0.62, 0.61, 0.58, 1.0]);
    let mut corner = Prim::new("sidewalk_corner", [0.62, 0.61, 0.58, 1.0]);
    // Level lots behind the sidewalks and the skirts beyond them (same colour; separate
    // materials so tools can tell the flush lot from the slope down to the ground).
    let mut lot = Prim::new("lot", [0.50, 0.52, 0.44, 1.0]);
    let mut lot_skirt = Prim::new("lot_skirt", [0.50, 0.52, 0.44, 1.0]);
    let mut kerb = Prim::new("kerb", [0.70, 0.69, 0.66, 1.0]);
    let mut buildings = Prim::new("building", [0.75, 0.74, 0.72, 1.0]);
    // Ashpeak County building kinds: red barns under dark metal roofs, galvanised silos,
    // glassy landmarks, concrete construction frames with orange safety bands.
    let mut barn = Prim::new("barn", [0.56, 0.17, 0.13, 1.0]);
    let mut barn_roof = Prim::new("barn_roof", [0.30, 0.31, 0.32, 1.0]);
    let mut silo = Prim::new("silo", [0.78, 0.80, 0.82, 1.0]);
    let mut silo_cap = Prim::new("silo_cap", [0.66, 0.69, 0.72, 1.0]);
    let mut landmark = Prim::new("landmark", [0.52, 0.64, 0.73, 1.0]);
    let mut construction = Prim::new("construction", [0.60, 0.59, 0.56, 1.0]);
    let mut construction_accent = Prim::new("construction_accent", [0.95, 0.46, 0.08, 1.0]);
    // Residential lots (playtest row 95): grass verges, pitched roofs, garages (trees,
    // driveways, hedges and fences are instanced: `build_instanced`).
    let mut verge = Prim::new("verge", [0.42, 0.52, 0.34, 1.0]);
    let mut roof = Prim::new("roof", [0.46, 0.30, 0.28, 1.0]);
    let mut garage = Prim::new("garage", [0.68, 0.66, 0.62, 1.0]);
    let mut wall = Prim::new("retaining_wall", [0.52, 0.50, 0.46, 1.0]);
    let mut bridge = Prim::new("bridge", [0.58, 0.58, 0.56, 1.0]);
    let mut rock = Prim::smooth("rock", [0.45, 0.42, 0.37, 1.0]);
    let mut seabed = Prim::smooth("seabed", [0.55, 0.52, 0.40, 1.0]);
    let mut water = Prim::new("water", [0.16, 0.30, 0.45, 1.0]);
    let mut taper = Prim::new("ramp_taper", [0.42, 0.42, 0.40, 1.0]);
    let mut barrier = Prim::new("median_barrier", [0.66, 0.65, 0.62, 1.0]);
    let mut river = Prim::new("river", [0.18, 0.34, 0.44, 1.0]);
    let mut canopy = Prim::new("canopy", [0.33, 0.36, 0.38, 1.0]);
    let mut steel = Prim::new("steel", [0.50, 0.24, 0.18, 1.0]);
    let mut ground: Vec<Prim> = DistrictKind::ALL
        .iter()
        .map(|k| Prim::smooth(&format!("ground_{k:?}").to_lowercase(), district_color(*k)))
        .collect();

    let terrain = net.terrain.as_ref();
    match terrain {
        Some(t) => {
            let coarse = wp_world::surface::drawn_terrain(map).expect("terrain");
            render_terrain(&coarse, net, &mut ground, &mut rock, &mut seabed);
            let (lo, hi) = (t.origin, t.max_corner());
            let m = 400.0;
            // Only a city with seabed gets the sea (the grid's flat ground sits just under
            // its streets, below sea level, and stays dry).
            let has_sea = t.z.iter().any(|z| *z < -1.0);
            if has_sea {
                water.quad(
                    [lo[0] - m, lo[1] - m, SEA_Z],
                    [hi[0] + m, lo[1] - m, SEA_Z],
                    [hi[0] + m, hi[1] + m, SEA_Z],
                    [lo[0] - m, hi[1] + m, SEA_Z],
                    [0.0, 0.0, 1.0],
                );
            }
        }
        None => {
            for z in &net.zones {
                ground[z.kind as usize].polygon(&z.polygon, -0.05);
            }
        }
    }
    // Pavement: the road surface ribbons, the very triangles `wp_sim` collides with; parking
    // and bus lanes are tinted 1 cm above them.
    for s in &net.surfaces {
        let prim = match s.material {
            _ if s.highway => &mut concrete,
            RoadMaterial::Asphalt => &mut asphalt,
            RoadMaterial::Gravel => &mut gravel,
            RoadMaterial::Dirt => &mut dirt,
        };
        prim.mesh(&surface_ribbon(s));
    }
    for l in &net.lanes {
        let prim = match l.kind {
            LaneKind::Parking => &mut parking,
            LaneKind::Bus => &mut bus,
            _ => continue,
        };
        let bank = lane_bank(net, l);
        prim.ribbon_banked(
            &l.polyline,
            l.width_m * 0.5,
            l.width_m * 0.5,
            LANE_TINT_DZ_M,
            &bank,
        );
    }
    // Raised sidewalks, their lots and skirts and the kerb faces: the very triangles `wp_sim`
    // collides with.
    let walks = sidewalk_mesh(map);
    sidewalk.mesh(&walks.tops);
    verge.mesh(&walks.verges);
    corner.mesh(&walks.corners);
    lot.mesh(&walks.lots);
    lot_skirt.mesh(&walks.skirts);
    kerb.mesh(&walks.kerbs);
    // Pavement between the ribbon ends (junctions, bends, dead ends): the very triangles
    // `wp_sim` collides with.
    let node_material = node_materials(map);
    for p in node_patches(map) {
        // Freeway merge / diverge (radius-0 junction): the ramp tapers overlap the
        // carriageway's paving, so they get their own (offset) material.
        let freeway = net.nodes[p.node as usize]
            .intersection
            .is_some_and(|ix| net.intersections[ix as usize].radius_m <= 0.0);
        let prim = match node_material.get(p.node as usize) {
            _ if freeway => &mut taper,
            _ if p.highway => &mut concrete,
            Some(RoadMaterial::Gravel) => &mut gravel,
            Some(RoadMaterial::Dirt) => &mut dirt,
            _ => &mut asphalt,
        };
        prim.mesh(&p.mesh);
    }
    // Median barriers.
    let barriers = median_barriers(net);
    for line in &barriers {
        barrier_mesh(&mut barrier, line);
    }
    // Outer highway edges (not in the parapet index below: they stop where decks start).
    for line in &edge_barriers(map) {
        wall_mesh(&mut barrier, line);
    }
    let near_barrier = BarrierIndex::new(&barriers);
    // Hilly cities: retaining walls where a road edge stands above the (render) ground, and
    // bridge decks with parapets and piers.
    // How far wall feet go below the ground: on hills the drawn (coarse) ground can sit lower
    // than the physics terrain; a flat terrain (main's grid, which has one only for its
    // viaduct) is drawn exactly, so the feet stay just under it.
    let foot_m = if wp_world::surface::has_relief(net) {
        1.5
    } else {
        0.2
    };
    if let Some(t) = terrain {
        let ground_z = |q: V2| t.height_at(q);
        for (si, s) in net.surfaces.iter().enumerate() {
            let (l, r) = surface_edges(s);
            let n = s.centre.len();
            let deck: Vec<bool> = (0..n).map(|k| is_elevated(s, k, &ground_z, 1.5)).collect();
            // Retaining walls 0.3 m outside a highway's paved edge (sidewalk roads: under the
            // skirt feet and bare sidewalk edges, below).
            for (side, edge) in [(-1.0f32, &l), (1.0, &r)] {
                if !s.highway {
                    continue;
                }
                let line = geom::offset_polyline(edge, side * 0.3);
                let top: Vec<V3> = line
                    .iter()
                    .zip(edge.iter())
                    .map(|(p, e)| [p[0], p[1], e[2]])
                    .collect();
                let bottom: Vec<f32> = top
                    .iter()
                    .zip(&deck)
                    .map(|(p, d)| {
                        if *d {
                            p[2]
                        } else {
                            wall_foot(net, *p, (ground_z(geom::xy(*p)) - foot_m).min(p[2]))
                        }
                    })
                    .collect();
                wall.curtain(&top, &bottom, side > 0.0);
            }
            // Decks: 1.2 m deep slab sides, 0.9 m parapets (piers below).
            for k in 0..n - 1 {
                if !(deck[k] && deck[k + 1]) {
                    continue;
                }
                for (side, e) in [(-1.0f32, &l), (1.0, &r)] {
                    let seg = [e[k], e[k + 1]];
                    // The median side of a divided highway has the barrier instead; a ramp
                    // edge on the carriageway it leaves has nothing.
                    if near_barrier.near(geom::xy(seg[0])) && near_barrier.near(geom::xy(seg[1])) {
                        continue;
                    }
                    if inside_other_paving(net, si, seg[0]) && inside_other_paving(net, si, seg[1])
                    {
                        continue;
                    }
                    bridge.curtain(
                        &seg,
                        &[seg[0][2] - DECK_SLAB_M, seg[1][2] - DECK_SLAB_M],
                        side > 0.0,
                    );
                    let top = [
                        [seg[0][0], seg[0][1], seg[0][2] + 0.9],
                        [seg[1][0], seg[1][1], seg[1][2] + 0.9],
                    ];
                    bridge.curtain(&top, &[seg[0][2], seg[1][2]], side > 0.0);
                }
            }
        }
    }
    for (fp, z0, z1) in deck_piers(net) {
        bridge.block(&fp, z0, z1);
    }
    // Sidewalk roads: walls from the skirt feet (and the back edges of sidewalks without a
    // lot, e.g. on decks) down past the drawn ground, which is coarser and lower than the
    // physics terrain the skirts end on. On flat terrain (main's grid) a skirt foot stands on
    // the ground, so its curtain would be buried whole: only the bare backs get one there
    // (render budget: ~10 draw calls and 6k triangles a frame on the grid).
    if let Some(t) = terrain {
        let ground_z = |q: V2| t.height_at(q);
        let feet: &[(V3, V3)] = if wp_world::surface::has_relief(net) {
            &walks.skirt_feet
        } else {
            &[]
        };
        for line in chain_segments(feet.iter().chain(&walks.bare_backs)) {
            let bottom: Vec<f32> = line
                .iter()
                .map(|p| wall_foot(net, *p, (ground_z(geom::xy(*p)) - foot_m).min(p[2])))
                .collect();
            wall.curtain(&line, &bottom, true);
        }
        // The guard rail `wp_sim` puts just outside a lot-less sidewalk edge: 0.3 m wide,
        // 0.9 m tall (both faces and the top).
        for (a, b) in &walks.bare_backs {
            let d = geom::normalize(geom::sub(geom::xy(*b), geom::xy(*a)));
            let r = geom::right_of(d);
            let at = |p: &V3, off: f32, dz: f32| {
                let q = geom::add(geom::xy(*p), geom::scale(r, off));
                [q[0], q[1], p[2] + dz]
            };
            let (i0, i1) = (at(a, 0.0, 0.9), at(b, 0.0, 0.9));
            let (o0, o1) = (at(a, 0.3, 0.9), at(b, 0.3, 0.9));
            bridge.curtain(&[i1, i0], &[b[2], a[2]], true);
            bridge.curtain(&[o0, o1], &[a[2] - 0.2, b[2] - 0.2], true);
            bridge.quad(i0, o0, o1, i1, [0.0, 0.0, 1.0]);
        }
    }
    for b in &net.buildings {
        let (walls, r) = match b.kind {
            BuildingKind::Garage => (&mut garage, &mut roof),
            BuildingKind::Farm => (&mut barn, &mut barn_roof),
            BuildingKind::Silo => (&mut silo, &mut silo_cap),
            BuildingKind::Landmark => (&mut landmark, &mut roof),
            BuildingKind::Construction => {
                safety_bands(&mut construction_accent, b);
                (&mut construction, &mut roof)
            }
            _ => (&mut buildings, &mut roof),
        };
        house(walls, r, b);
    }
    // Tunnels and covered roads (backlog B04): concrete walls, the roof deck, ceiling lights.
    // Colours follow docs/art/STYLE_DECISION.md's palette (retaining wall / bridge greys,
    // sodium lamps); the low-poly dark restyle maps these material names.
    let mut tunnel_wall = Prim::new("tunnel_wall", [0.24, 0.25, 0.29, 1.0]);
    let mut tunnel_roof = Prim::new("tunnel_roof", [0.28, 0.29, 0.34, 1.0]);
    let mut tunnel_light = Prim::new("tunnel_light", [1.0, 0.76, 0.48, 1.0]);
    for span in &net.covered {
        render_cover(span, &mut tunnel_wall, &mut tunnel_roof, &mut tunnel_light);
    }
    // Rivers above sea level (at the sea the sea plane is the water).
    for w in &net.features.waterways {
        let edge = |side: f32| -> Vec<V3> {
            w.centre
                .iter()
                .enumerate()
                .map(|(k, p)| {
                    let n = w.centre.len();
                    let a = geom::xy(w.centre[k.saturating_sub(1)]);
                    let b = geom::xy(w.centre[(k + 1).min(n - 1)]);
                    let r = geom::right_of(geom::normalize(geom::sub(b, a)));
                    let q = geom::add(
                        geom::xy(*p),
                        geom::scale(r, side * (w.half_width_m[k] + 3.0)),
                    );
                    [q[0], q[1], p[2]]
                })
                .collect()
        };
        let (l, r) = (edge(-1.0), edge(1.0));
        for k in 0..w.centre.len() - 1 {
            if w.centre[k][2] > SEA_Z + 0.05 || w.centre[k + 1][2] > SEA_Z + 0.05 {
                river.strip(&l[k..k + 2], &r[k..k + 2]);
            }
        }
    }
    // Steel decks of truss bridges.
    for b in net
        .features
        .bridges
        .iter()
        .filter(|b| b.kind == BridgeKind::Truss)
    {
        steel.ribbon(&b.centre, b.half_width_m, b.half_width_m, 0.02);
    }
    for st in &net.features.structures {
        let prim = match st.kind {
            StructureKind::Canopy | StructureKind::Pillar => &mut canopy,
            StructureKind::Landmark => &mut landmark,
            StructureKind::Truss => &mut steel,
        };
        prim.solid(&st.footprint, st.z0_m, st.z1_m);
    }
    let mut out = vec![
        asphalt,
        concrete,
        gravel,
        dirt,
        parking,
        bus,
        sidewalk,
        verge,
        corner,
        lot,
        lot_skirt,
        kerb,
        buildings,
        roof,
        garage,
        barn,
        barn_roof,
        silo,
        silo_cap,
        landmark,
        construction,
        construction_accent,
        wall,
        bridge,
        rock,
        seabed,
        water,
        tunnel_wall,
        tunnel_roof,
        tunnel_light,
        taper,
        barrier,
        river,
        canopy,
        steel,
    ];
    out.extend(ground);
    out.retain(|p| !p.indices.is_empty());
    out
}

/// Paving per node for its junction patch: gravel or dirt when every road meeting there is
/// unpaved (gravel if they mix), else asphalt.
fn node_materials(map: &CityMap) -> Vec<RoadMaterial> {
    let net = &map.net;
    // None = no road seen yet.
    let mut per: Vec<Option<RoadMaterial>> = vec![None; net.nodes.len()];
    for (i, l) in net.lanes.iter().enumerate() {
        let m = map.lane_material(i as u32);
        for n in [l.from_node, l.to_node] {
            let Some(slot) = per.get_mut(n as usize) else {
                continue;
            };
            *slot = Some(match (*slot, m) {
                (None, m) => m,
                (Some(RoadMaterial::Asphalt), _) | (_, RoadMaterial::Asphalt) => {
                    RoadMaterial::Asphalt
                }
                (Some(a), b) if a == b => a,
                _ => RoadMaterial::Gravel,
            });
        }
    }
    per.into_iter().map(Option::unwrap_or_default).collect()
}

/// Orange safety-netting bands around a building under construction: 0.4 m tall, 0.15 m
/// proud of the walls, one per ~4 m storey and one at the top.
fn safety_bands(p: &mut Prim, b: &Building) {
    let f = &b.footprint;
    if f.len() < 3 {
        return;
    }
    let n = f.len() as f32;
    let c = [
        f.iter().map(|q| q[0]).sum::<f32>() / n,
        f.iter().map(|q| q[1]).sum::<f32>() / n,
    ];
    let outset: Vec<V2> = f
        .iter()
        .map(|q| {
            let d = geom::sub(*q, c);
            let len = geom::dist(*q, c).max(0.1);
            geom::add(*q, geom::scale(d, 0.15 / len))
        })
        .collect();
    let (z0, z1) = (b.base_z_m, b.roof_z());
    let mut z = z0 + 4.0;
    let mut bands = 0;
    while z < z1 - 1.0 && bands < 40 {
        p.walls(&outset, z - 0.2, z + 0.2);
        z += 4.0;
        bands += 1;
    }
    p.prism(&outset, z1 - 0.4, z1 + 0.05);
}

/// Push triangle `a b c` into `p`, wound so its normal points along `out` (flat-shaded).
fn tri_facing(p: &mut Prim, a: V3, b: V3, c: V3, out: V3) {
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let i = p.vert(a, out);
    p.vert(b, out);
    p.vert(c, out);
    if n[0] * out[0] + n[1] * out[1] + n[2] * out[2] >= 0.0 {
        p.indices.extend_from_slice(&[i, i + 1, i + 2]);
    } else {
        p.indices.extend_from_slice(&[i, i + 2, i + 1]);
    }
}

/// Vertex ids of a small shared-vertex mesh piece (one building's walls or roof).
struct Shared<'a> {
    p: &'a mut Prim,
    ids: Vec<(V3, u32)>,
}

impl Shared<'_> {
    fn id(&mut self, v: V3) -> u32 {
        if let Some((_, i)) = self.ids.iter().find(|(q, _)| *q == v) {
            return *i;
        }
        let i = self.p.vert(v, [0.0, 0.0, 1.0]);
        self.ids.push((v, i));
        i
    }

    /// Triangle wound so its normal points along `out`.
    fn tri(&mut self, a: V3, b: V3, c: V3, out: V3) {
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let (ia, ib, ic) = (self.id(a), self.id(b), self.id(c));
        if n[0] * out[0] + n[1] * out[1] + n[2] * out[2] >= 0.0 {
            self.p.indices.extend_from_slice(&[ia, ib, ic]);
        } else {
            self.p.indices.extend_from_slice(&[ia, ic, ib]);
        }
    }
}

/// A building: walls up to the eaves, then a flat top or a gable / hip roof over its
/// rectangle ([`Building::ridge`]; gable ends are wall). Vertices are shared within each
/// building's walls and roof (the testbed flat-shades the greybox).
fn house(walls: &mut Prim, roof: &mut Prim, b: &Building) {
    let Some(ridge) = b.ridge() else {
        walls.prism(&b.footprint, b.base_z_m, b.roof_z());
        return;
    };
    let z = b.roof_z();
    let f = &b.footprint;
    let n = f.len();
    let centre = [
        f.iter().map(|q| q[0]).sum::<f32>() / n as f32,
        f.iter().map(|q| q[1]).sum::<f32>() / n as f32,
    ];
    let near = |q: V2| {
        if geom::dist(q, geom::xy(ridge[0])) <= geom::dist(q, geom::xy(ridge[1])) {
            ridge[0]
        } else {
            ridge[1]
        }
    };
    let mut w = Shared {
        p: walls,
        ids: Vec::new(),
    };
    let mut r = Shared {
        p: roof,
        ids: Vec::new(),
    };
    for i in 0..n {
        let (qa, qb) = (f[i], f[(i + 1) % n]);
        let (a, bb) = ([qa[0], qa[1], z], [qb[0], qb[1], z]);
        let (a0, b0) = ([qa[0], qa[1], b.base_z_m], [qb[0], qb[1], b.base_z_m]);
        let mid = [(qa[0] + qb[0]) * 0.5, (qa[1] + qb[1]) * 0.5];
        let o = geom::normalize(geom::sub(mid, centre));
        let out = [o[0], o[1], 0.0];
        let up = [o[0], o[1], 1.0];
        w.tri(a0, b0, bb, out);
        w.tri(a0, bb, a, out);
        let (ra, rb) = (near(qa), near(qb));
        if ra == rb {
            // Triangle: vertical gable end (ridge end above the edge) or a hip slope.
            if geom::dist(geom::xy(ra), mid) < 0.05 {
                w.tri(a, bb, ra, out);
            } else {
                r.tri(a, bb, ra, up);
            }
        } else {
            r.tri(a, bb, rb, up);
            r.tri(a, rb, ra, up);
        }
    }
}

/// A unit mesh drawn many times (glTF `EXT_mesh_gpu_instancing`): trees.
#[derive(Debug, Clone, Default)]
pub struct Instanced {
    /// Mesh name.
    pub name: String,
    /// Parts (one primitive per material), world-style coordinates (+Z up) of the unit mesh.
    pub parts: Vec<Prim>,
    /// Per instance: base position (world), yaw about +Z, scale (x, y, z world axes).
    pub instances: Vec<(V3, f32, V3)>,
}

/// Unit tree (1 m crown diameter, 1 m tall): a hexagonal trunk to 0.45 and a low-poly crown
/// (an icosahedron stretched to 0.32..1.0).
fn unit_tree() -> Vec<Prim> {
    let mut trunk = Prim::new("tree_trunk", [0.36, 0.28, 0.22, 1.0]);
    let mut crown = Prim::new("tree_crown", [0.27, 0.42, 0.25, 1.0]);
    let r = 0.045;
    let ring: Vec<V2> = (0..6)
        .map(|k| {
            let (s, c) = wp_math::sin_cos(k as f32 * std::f32::consts::TAU / 6.0);
            [c * r, s * r]
        })
        .collect();
    trunk.walls(&ring, -0.05, 0.45);
    // Icosahedron.
    let t = (1.0 + 5f32.sqrt()) * 0.5;
    let raw: [[f32; 3]; 12] = [
        [-1.0, t, 0.0],
        [1.0, t, 0.0],
        [-1.0, -t, 0.0],
        [1.0, -t, 0.0],
        [0.0, -1.0, t],
        [0.0, 1.0, t],
        [0.0, -1.0, -t],
        [0.0, 1.0, -t],
        [t, 0.0, -1.0],
        [t, 0.0, 1.0],
        [-t, 0.0, -1.0],
        [-t, 0.0, 1.0],
    ];
    let faces: [[usize; 3]; 20] = [
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    let len = (1.0 + t * t).sqrt();
    let v: Vec<V3> = raw
        .iter()
        .map(|p| [p[0] / len * 0.5, p[1] / len * 0.5, 0.66 + p[2] / len * 0.34])
        .collect();
    for f in faces {
        let (a, b, c) = (v[f[0]], v[f[1]], v[f[2]]);
        let m = [
            (a[0] + b[0] + c[0]) / 3.0,
            (a[1] + b[1] + c[1]) / 3.0,
            (a[2] + b[2] + c[2]) / 3.0 - 0.66,
        ];
        tri_facing(&mut crown, a, b, c, m);
    }
    vec![trunk, crown]
}

/// Axis-aligned box from (x0, y0, z0) to (x1, y1, z1) into `p` (walls and top).
fn add_box(p: &mut Prim, x: [f32; 2], y: [f32; 2], z: [f32; 2]) {
    let sq = [[x[0], y[0]], [x[1], y[0]], [x[1], y[1]], [x[0], y[1]]];
    p.block(&sq, z[0], z[1]);
}

/// Closed solid: convex `profile` in the x-z plane extruded along +Y from `y0` to `y1`.
fn extrude_y(p: &mut Prim, profile: &[V2], y0: f32, y1: f32) {
    let n = profile.len();
    let c = [
        profile.iter().map(|q| q[0]).sum::<f32>() / n as f32,
        profile.iter().map(|q| q[1]).sum::<f32>() / n as f32,
    ];
    let v = |q: V2, y: f32| [q[0], y, q[1]];
    for i in 0..n {
        let (a, b) = (profile[i], profile[(i + 1) % n]);
        let out = [(a[0] + b[0]) * 0.5 - c[0], 0.0, (a[1] + b[1]) * 0.5 - c[1]];
        tri_facing(p, v(a, y0), v(b, y0), v(b, y1), out);
        tri_facing(p, v(a, y0), v(b, y1), v(a, y1), out);
        tri_facing(p, v(c, y0), v(a, y0), v(b, y0), [0.0, -1.0, 0.0]);
        tri_facing(p, v(c, y1), v(a, y1), v(b, y1), [0.0, 1.0, 0.0]);
    }
}

/// Cone of radius `r` (`sides`-gon) from a base at `z0` to the apex at `z1`, base closed.
fn cone(p: &mut Prim, r: f32, z0: f32, z1: f32, sides: usize) {
    let ring: Vec<V2> = (0..sides)
        .map(|k| {
            let (s, c) = wp_math::sin_cos(k as f32 * std::f32::consts::TAU / sides as f32);
            [c * r, s * r]
        })
        .collect();
    let apex = [0.0, 0.0, z1];
    for i in 0..sides {
        let (a, b) = (ring[i], ring[(i + 1) % sides]);
        let (a3, b3) = ([a[0], a[1], z0], [b[0], b[1], z0]);
        let m = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        tri_facing(p, a3, b3, apex, [m[0], m[1], r * 0.5]);
        tri_facing(p, [0.0, 0.0, z0], a3, b3, [0.0, 0.0, -1.0]);
    }
}

/// Unit pine (1 m crown diameter, 1 m tall): a hexagonal trunk and three stacked cones.
fn unit_pine() -> Vec<Prim> {
    let mut trunk = Prim::new("pine_trunk", [0.33, 0.24, 0.18, 1.0]);
    let mut crown = Prim::new("pine_crown", [0.15, 0.29, 0.20, 1.0]);
    let r = 0.05;
    let ring: Vec<V2> = (0..6)
        .map(|k| {
            let (s, c) = wp_math::sin_cos(k as f32 * std::f32::consts::TAU / 6.0);
            [c * r, s * r]
        })
        .collect();
    trunk.walls(&ring, -0.05, 0.3);
    cone(&mut crown, 0.5, 0.18, 0.66, 7);
    cone(&mut crown, 0.38, 0.42, 0.85, 7);
    cone(&mut crown, 0.25, 0.64, 1.0, 7);
    vec![trunk, crown]
}

/// Unit tower crane: x, y = the mast footprint (-0.5..0.5), z = full height (0..1). Lattice
/// mast to 0.9, slewing cab, a jib reaching 14 mast widths along +Y, a counter-jib along -Y
/// with its concrete counterweight, a hook line hanging from the jib.
fn unit_crane() -> Vec<Prim> {
    let mut steel = Prim::new("crane", [0.93, 0.71, 0.13, 1.0]);
    let mut weight = Prim::new("crane_counterweight", [0.55, 0.55, 0.53, 1.0]);
    let mut cable = Prim::new("crane_cable", [0.15, 0.15, 0.16, 1.0]);
    add_box(&mut steel, [-0.5, 0.5], [-0.5, 0.5], [0.0, 0.9]);
    add_box(&mut steel, [-0.6, 0.6], [-0.6, 0.9], [0.9, 0.935]);
    add_box(&mut steel, [-0.3, 0.3], [-5.0, 14.0], [0.935, 0.955]);
    // Apex and its tie line silhouette (a thin rod over the jib root).
    add_box(&mut steel, [-0.15, 0.15], [-0.15, 0.15], [0.955, 1.0]);
    add_box(&mut steel, [-0.05, 0.05], [0.15, 6.0], [0.975, 0.985]);
    add_box(&mut weight, [-0.45, 0.45], [-5.0, -3.2], [0.88, 0.935]);
    add_box(&mut cable, [-0.03, 0.03], [9.97, 10.03], [0.55, 0.935]);
    add_box(&mut weight, [-0.15, 0.15], [9.85, 10.15], [0.52, 0.55]);
    vec![steel, weight, cable]
}

/// Unit Jersey barrier: the median barrier's profile across x (-0.5..0.5), along y
/// (-0.5..0.5), 0..1 tall.
fn unit_jersey() -> Vec<Prim> {
    let mut p = Prim::new("jersey_barrier", [0.72, 0.71, 0.68, 1.0]);
    let (w, h) = (BARRIER_PROFILE[0].0 * 2.0, BARRIER_PROFILE[3].1);
    let right: Vec<V2> = BARRIER_PROFILE
        .iter()
        .map(|(x, z)| [x / w, z / h])
        .collect();
    let mut prof: Vec<V2> = right.clone();
    prof.extend(right.iter().rev().map(|q| [-q[0], q[1]]));
    extrude_y(&mut p, &prof, -0.5, 0.5);
    vec![p]
}

/// Unit construction-site hoarding panel: a green-grey panel with posts and a top rail.
fn unit_site_fence() -> Vec<Prim> {
    let mut panel = Prim::new("site_fence", [0.38, 0.45, 0.40, 1.0]);
    let mut posts = Prim::new("site_fence_post", [0.62, 0.63, 0.62, 1.0]);
    add_box(&mut panel, [-0.35, 0.35], [-0.5, 0.5], [0.05, 0.92]);
    add_box(&mut posts, [-0.5, 0.5], [-0.5, 0.5], [0.92, 1.0]);
    for y in [-0.47, 0.47] {
        add_box(&mut posts, [-0.5, 0.5], [y - 0.03, y + 0.03], [-0.1, 0.92]);
    }
    vec![panel, posts]
}

/// Unit round hay bale lying on its side: an octagonal roll along y, 1 × 1 × 1.
fn unit_hay_bale() -> Vec<Prim> {
    let mut p = Prim::new("hay_bale", [0.80, 0.68, 0.36, 1.0]);
    let prof: Vec<V2> = (0..8)
        .map(|k| {
            let (s, c) = wp_math::sin_cos((k as f32 + 0.5) * std::f32::consts::TAU / 8.0);
            [c * 0.5, 0.5 + s * 0.5]
        })
        .collect();
    extrude_y(&mut p, &prof, -0.5, 0.5);
    vec![p]
}

/// Unit roadside billboard: a panel over the top half on two posts, a walkway under it.
fn unit_billboard() -> Vec<Prim> {
    let mut frame = Prim::new("billboard", [0.30, 0.32, 0.36, 1.0]);
    let mut face = Prim::new("billboard_panel", [0.86, 0.82, 0.70, 1.0]);
    add_box(&mut frame, [-0.5, 0.5], [-0.5, 0.0], [0.48, 1.0]);
    add_box(&mut face, [-0.47, 0.47], [0.0, 0.25], [0.51, 0.97]);
    add_box(&mut frame, [-0.5, 0.5], [-0.5, 0.5], [0.45, 0.48]);
    for x in [-0.3, 0.3] {
        add_box(&mut frame, [x - 0.03, x + 0.03], [-0.2, 0.1], [0.0, 0.45]);
    }
    vec![frame, face]
}

/// Unit box: x, y in -0.5..0.5, z in 0..1 (walls and top), one material.
fn unit_box(material: &str, color: [f32; 4]) -> Prim {
    let mut p = Prim::new(material, color);
    let sq = [[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]];
    p.block(&sq, 0.0, 1.0);
    p
}

/// Instanced meshes of a city (residential lots, playtest row 95): every street and yard
/// tree as one unit tree scaled to its crown and height, and driveways, hedges and fences as
/// unit boxes. A box stands on its decor's base point (hedges and fences sunk 0.1 m); a
/// driveway runs from its mouth across the lot, its top 3 cm above the lot, its sides 0.4 m
/// deep (it never floats over the coarser drawn ground).
pub fn build_instanced(map: &CityMap) -> Vec<Instanced> {
    let decor = &map.net.decor;
    let mut out = Vec::new();
    let trees: Vec<(V3, f32, V3)> = decor
        .iter()
        .filter(|d| matches!(d.kind, DecorKind::StreetTree | DecorKind::YardTree))
        .map(|d| (d.pos, d.yaw, [d.size[0], d.size[1], d.size[2]]))
        .collect();
    if !trees.is_empty() {
        out.push(Instanced {
            name: "trees".into(),
            parts: unit_tree(),
            instances: trees,
        });
    }
    // Ashpeak County decor: shaped unit meshes, each scaled to its decor box (size x, y, z)
    // and standing on its base point.
    for (kind, name, parts) in [
        (DecorKind::PineTree, "pines", unit_pine()),
        (DecorKind::Crane, "cranes", unit_crane()),
        (DecorKind::JerseyBarrier, "jersey_barriers", unit_jersey()),
        (DecorKind::SiteFence, "site_fences", unit_site_fence()),
        (DecorKind::HayBale, "hay_bales", unit_hay_bale()),
        (DecorKind::Billboard, "billboards", unit_billboard()),
    ] {
        let instances: Vec<(V3, f32, V3)> = decor
            .iter()
            .filter(|d| d.kind == kind)
            .map(|d| (d.pos, d.yaw, [d.size[0], d.size[1], d.size[2]]))
            .collect();
        if !instances.is_empty() {
            out.push(Instanced {
                name: name.into(),
                parts,
                instances,
            });
        }
    }
    for (kind, name, color) in [
        (DecorKind::Driveway, "driveway", [0.58, 0.57, 0.54, 1.0]),
        (DecorKind::Hedge, "hedge", [0.24, 0.38, 0.22, 1.0]),
        (DecorKind::Fence, "fence", [0.60, 0.55, 0.48, 1.0]),
    ] {
        let boxes: Vec<(V3, f32, V3)> = decor
            .iter()
            .filter(|d| d.kind == kind)
            .map(|d| {
                if kind == DecorKind::Driveway {
                    let c = geom::add(
                        [d.pos[0], d.pos[1]],
                        geom::scale(residential::driveway_out(d), d.size[1] * 0.5),
                    );
                    let top = residential::driveway_top(d);
                    ([c[0], c[1], top - 0.4], d.yaw, [d.size[0], d.size[1], 0.4])
                } else {
                    (
                        [d.pos[0], d.pos[1], d.pos[2] - 0.1],
                        d.yaw,
                        [d.size[0], d.size[1], d.size[2] + 0.1],
                    )
                }
            })
            .collect();
        if !boxes.is_empty() {
            out.push(Instanced {
                name: name.into(),
                parts: vec![unit_box(name, color)],
                instances: boxes,
            });
        }
    }
    out
}

/// The whole greybox `.glb` of a city: [`build_greybox`] plus [`build_instanced`].
pub fn greybox_glb(map: &CityMap) -> Vec<u8> {
    write_glb_instanced(&build_greybox(map), &build_instanced(map), &map.net.id)
}

/// One covered span: both walls (inner and outer faces, from below the road to the ceiling),
/// the roof deck (ceiling underside, top, side edges, portal faces) and a ceiling light every
/// [`wp_world::covered::LIGHT_SPACING_M`] (a 1.6 × 0.5 m emissive panel 0.12 m below the
/// ceiling, two-sided).
fn render_cover(
    span: &wp_world::network::CoveredSpan,
    wall: &mut Prim,
    roof: &mut Prim,
    light: &mut Prim,
) {
    use wp_world::covered::{CoverShape, ROOF_M, WALL_M};
    let shape = CoverShape::new(span);
    let c = &span.centre;
    let h = span.clearance_m;
    let deck = shape.deck_half_width();
    for side in [-1.0f32, 1.0] {
        for off in [span.half_width_m, span.half_width_m + WALL_M] {
            let line = geom::offset_polyline(c, side * off);
            let top: Vec<V3> = line.iter().map(|p| [p[0], p[1], p[2] + h]).collect();
            let bottom: Vec<f32> = line.iter().map(|p| p[2] - 1.0).collect();
            // Inner faces look into the tube, outer faces away from it.
            let inner = off == span.half_width_m;
            wall.curtain(&top, &bottom, (side < 0.0) == inner);
        }
    }
    roof.ribbon(c, deck, deck, h);
    roof.ribbon(c, deck, deck, h + ROOF_M);
    for side in [-1.0f32, 1.0] {
        let line = geom::offset_polyline(c, side * deck);
        let top: Vec<V3> = line
            .iter()
            .map(|p| [p[0], p[1], p[2] + h + ROOF_M])
            .collect();
        let bottom: Vec<f32> = line.iter().map(|p| p[2] + h).collect();
        roof.curtain(&top, &bottom, side > 0.0);
    }
    // Portal faces: the deck's end edges.
    for k in [0usize, c.len() - 1] {
        let j = if k == 0 { 1 } else { k - 1 };
        let d = geom::normalize(geom::sub(geom::xy(c[k]), geom::xy(c[j])));
        let d = if k == 0 { geom::scale(d, -1.0) } else { d };
        let r = geom::right_of(d);
        let p = geom::xy(c[k]);
        let (z0, z1) = (c[k][2] + h, c[k][2] + h + ROOF_M);
        let a = geom::add(p, geom::scale(r, -deck));
        let b = geom::add(p, geom::scale(r, deck));
        roof.quad(
            [a[0], a[1], z0],
            [b[0], b[1], z0],
            [b[0], b[1], z1],
            [a[0], a[1], z1],
            [d[0], d[1], 0.0],
        );
    }
    for q in shape.light_points() {
        let loc = shape.locate(geom::xy(q));
        let (_, d) = geom::point_at(c, &shape.cum, loc.s);
        let r = geom::right_of(d);
        let z = q[2] - 0.12;
        let corner = |a: f32, b: f32| {
            let p = geom::add(geom::xy(q), geom::add(geom::scale(d, a), geom::scale(r, b)));
            [p[0], p[1], z]
        };
        light.quad(
            corner(-0.8, -0.25),
            corner(0.8, -0.25),
            corner(0.8, 0.25),
            corner(-0.8, 0.25),
            [0.0, 0.0, -1.0],
        );
    }
}

/// Consecutive segments `(a, b)` (the next starting exactly where one ends) joined into
/// polylines, in order.
fn chain_segments<'a>(segs: impl Iterator<Item = &'a (V3, V3)>) -> Vec<Vec<V3>> {
    let mut out: Vec<Vec<V3>> = Vec::new();
    for (a, b) in segs {
        match out.last_mut() {
            Some(line) if line.last() == Some(a) => line.push(*b),
            _ => out.push(vec![*a, *b]),
        }
    }
    out
}

/// Split a primitive into pieces of at most 65 535 vertices (16-bit indices), keeping
/// triangle order.
fn split_u16(p: &Prim) -> Vec<Prim> {
    const MAX: usize = 65_535;
    if p.positions.len() <= MAX {
        return vec![p.clone()];
    }
    let mut out = Vec::new();
    let fresh = || Prim {
        smooth: p.smooth,
        ..Prim::new(&p.material, p.color)
    };
    let mut cur = fresh();
    let mut remap: Vec<u32> = vec![u32::MAX; p.positions.len()];
    for tri in p.indices.chunks(3) {
        let new_verts = tri
            .iter()
            .filter(|i| remap[**i as usize] == u32::MAX)
            .count();
        if cur.positions.len() + new_verts > MAX {
            out.push(std::mem::replace(&mut cur, fresh()));
            remap.iter_mut().for_each(|r| *r = u32::MAX);
        }
        for &i in tri {
            if remap[i as usize] == u32::MAX {
                let n = p
                    .normals
                    .get(i as usize)
                    .copied()
                    .unwrap_or([0.0, 0.0, 1.0]);
                remap[i as usize] = cur.vert(p.positions[i as usize], n);
            }
            cur.indices.push(remap[i as usize]);
        }
    }
    if !cur.indices.is_empty() {
        out.push(cur);
    }
    out
}

/// Append one primitive's vertex / index data to `bin` and its views and accessors; returns
/// the glTF primitive (material `mi`).
fn push_prim(
    bin: &mut Vec<u8>,
    views: &mut Vec<serde_json::Value>,
    accessors: &mut Vec<serde_json::Value>,
    p: &Prim,
    mi: usize,
) -> serde_json::Value {
    let to_gl = |p: V3| [p[0], p[2], -p[1]];
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    let pos_off = bin.len();
    for v in &p.positions {
        let g = to_gl(*v);
        for k in 0..3 {
            lo[k] = lo[k].min(g[k]);
            hi[k] = hi[k].max(g[k]);
            bin.extend_from_slice(&g[k].to_le_bytes());
        }
    }
    let pos_len = bin.len() - pos_off;
    let nrm_off = bin.len();
    for n in &p.normals {
        for c in to_gl(*n) {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let nrm_len = bin.len() - nrm_off;
    let idx_off = bin.len();
    for i in &p.indices {
        bin.extend_from_slice(&(*i as u16).to_le_bytes());
    }
    let idx_len = bin.len() - idx_off;
    // Keep the next float buffer view 4-byte aligned.
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let has_n = !p.normals.is_empty();
    let pos_view = views.len();
    views.push(json!({"buffer": 0, "byteOffset": pos_off, "byteLength": pos_len, "target": 34962}));
    let nrm_view = views.len();
    if has_n {
        views.push(
            json!({"buffer": 0, "byteOffset": nrm_off, "byteLength": nrm_len, "target": 34962}),
        );
    }
    let idx_view = views.len();
    views.push(json!({"buffer": 0, "byteOffset": idx_off, "byteLength": idx_len, "target": 34963}));
    let pos_acc = accessors.len();
    accessors.push(json!({"bufferView": pos_view, "componentType": 5126, "count": p.positions.len(), "type": "VEC3", "min": lo, "max": hi}));
    let mut attributes = json!({ "POSITION": pos_acc });
    if has_n {
        attributes["NORMAL"] = json!(accessors.len());
        accessors.push(json!({"bufferView": nrm_view, "componentType": 5126, "count": p.normals.len(), "type": "VEC3"}));
    }
    let idx_acc = accessors.len();
    accessors.push(json!({"bufferView": idx_view, "componentType": 5123, "count": p.indices.len(), "type": "SCALAR"}));
    json!({
        "attributes": attributes,
        "indices": idx_acc,
        "material": mi,
        "mode": 4
    })
}

/// glTF material of a primitive.
fn material_json(p: &Prim) -> serde_json::Value {
    json!({
        "name": p.material,
        // Walls, kerbs and parapets are single quads: draw both faces.
        "doubleSided": true,
        // Lamps (`*_light`, e.g. tunnel ceiling lights) glow in their own colour.
        "emissiveFactor": if p.material.ends_with("_light") {
            [p.color[0], p.color[1], p.color[2]]
        } else {
            [0.0, 0.0, 0.0]
        },
        "pbrMetallicRoughness": {
            "baseColorFactor": p.color,
            "metallicFactor": 0.0,
            "roughnessFactor": 0.9
        }
    })
}

/// Encode primitives as a `.glb` (positions converted to glTF +Y-up).
pub fn write_glb(prims: &[Prim], name: &str) -> Vec<u8> {
    write_glb_instanced(prims, &[], name)
}

/// [`write_glb`] plus instanced meshes (`EXT_mesh_gpu_instancing`).
pub fn write_glb_instanced(prims: &[Prim], instanced: &[Instanced], name: &str) -> Vec<u8> {
    let parts: Vec<(usize, Prim)> = prims
        .iter()
        .enumerate()
        .flat_map(|(mi, p)| split_u16(p).into_iter().map(move |c| (mi, c)))
        .collect();
    let mut bin: Vec<u8> = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut materials = Vec::new();
    let mut primitives = Vec::new();
    for p in prims {
        materials.push(material_json(p));
    }
    for (mi, p) in &parts {
        primitives.push(push_prim(&mut bin, &mut views, &mut accessors, p, *mi));
    }
    let mut nodes = vec![json!({"name": name, "mesh": 0})];
    let mut meshes = vec![json!({"name": name, "primitives": primitives})];
    // Instanced meshes: one node each with EXT_mesh_gpu_instancing (translation, rotation
    // about the up axis, scale per instance; glTF axes).
    for inst in instanced {
        let mut prims_i = Vec::new();
        for part in &inst.parts {
            let mi = materials.len();
            materials.push(material_json(part));
            prims_i.push(push_prim(&mut bin, &mut views, &mut accessors, part, mi));
        }
        let mut push_vecs = |vals: Vec<f32>, ty: &str| {
            let off = bin.len();
            for v in &vals {
                bin.extend_from_slice(&v.to_le_bytes());
            }
            let view = views.len();
            views.push(json!({"buffer": 0, "byteOffset": off, "byteLength": bin.len() - off}));
            let acc = accessors.len();
            let n = if ty == "VEC4" { 4 } else { 3 };
            accessors.push(json!({"bufferView": view, "componentType": 5126, "count": vals.len() / n, "type": ty}));
            acc
        };
        let mut t = Vec::new();
        let mut r = Vec::new();
        let mut sc = Vec::new();
        for (p, yaw, scale) in &inst.instances {
            t.extend_from_slice(&[p[0], p[2], -p[1]]);
            let (sn, cs) = wp_math::sin_cos(*yaw * 0.5);
            r.extend_from_slice(&[0.0, sn, 0.0, cs]);
            sc.extend_from_slice(&[scale[0], scale[2], scale[1]]);
        }
        let (ta, ra, sa) = (
            push_vecs(t, "VEC3"),
            push_vecs(r, "VEC4"),
            push_vecs(sc, "VEC3"),
        );
        meshes.push(json!({"name": inst.name, "primitives": prims_i}));
        nodes.push(json!({
            "name": inst.name,
            "mesh": meshes.len() - 1,
            "extensions": {"EXT_mesh_gpu_instancing": {"attributes": {"TRANSLATION": ta, "ROTATION": ra, "SCALE": sa}}}
        }));
    }
    let node_ids: Vec<usize> = (0..nodes.len()).collect();
    let mut doc = json!({
        "asset": {"version": "2.0", "generator": "wreckpursuit city-import"},
        "scene": 0,
        "scenes": [{"name": name, "nodes": node_ids}],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "buffers": [{"byteLength": bin.len()}],
        "bufferViews": views,
        "accessors": accessors
    });
    if !instanced.is_empty() {
        doc["extensionsUsed"] = json!(["EXT_mesh_gpu_instancing"]);
    }
    let mut json_bytes = serde_json::to_vec(&doc).expect("gltf json");
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let total = 12 + 8 + json_bytes.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangulates_concave_l_shape() {
        let l = [
            [0.0, 0.0],
            [2.0, 0.0],
            [2.0, 1.0],
            [1.0, 1.0],
            [1.0, 2.0],
            [0.0, 2.0],
        ];
        let t = triangulate(&l);
        assert_eq!(t.len(), 4);
        let area: f32 = t
            .iter()
            .map(|t| {
                let (a, b, c) = (l[t[0] as usize], l[t[1] as usize], l[t[2] as usize]);
                0.5 * geom::cross(geom::sub(b, a), geom::sub(c, a))
            })
            .sum();
        assert!((area - 3.0).abs() < 1e-5);
    }

    #[test]
    fn no_bridge_pier_stands_on_a_road() {
        for net in [
            wp_world::vesper::generate(&wp_world::vesper::VesperConfig::default()),
            wp_world::procgen::generate(&wp_world::procgen::ProcGenConfig::default()),
        ] {
            let piers = deck_piers(&net);
            assert!(piers.len() > 20, "{} piers", piers.len());
            for (fp, _, top) in &piers {
                for q in fp {
                    // Nothing paved under a pier (beyond its own deck).
                    let paved = net.surfaces.iter().any(|o| {
                        let cum = geom::cumulative(&o.centre);
                        let pr = geom::project(&o.centre, &cum, *q);
                        let edge = if pr.lateral < 0.0 {
                            o.left_m
                        } else {
                            o.right_m
                        };
                        let len = cum[cum.len() - 1];
                        pr.s >= 0.0
                            && pr.s <= len
                            && pr.lateral.abs() <= edge
                            && geom::point_at(&o.centre, &cum, pr.s).0[2] < top - 1.0
                    });
                    assert!(!paved, "pier on a road at {q:?} in {}", net.id);
                }
            }
        }
    }

    #[test]
    fn glb_header_and_chunks() {
        let mut p = Prim::new("x", [1.0; 4]);
        p.quad(
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        );
        let g = write_glb(&[p], "t");
        assert_eq!(&g[0..4], b"glTF");
        assert_eq!(
            u32::from_le_bytes(g[8..12].try_into().unwrap()) as usize,
            g.len()
        );
        assert_eq!(&g[16..20], b"JSON");
        let jl = u32::from_le_bytes(g[12..16].try_into().unwrap()) as usize;
        let doc: serde_json::Value = serde_json::from_slice(&g[20..20 + jl]).unwrap();
        assert_eq!(doc["accessors"][0]["count"], 4);
        // +Z up became glTF +Y up: the quad lies in y = 0, z from 0 to -1.
        assert_eq!(doc["accessors"][0]["max"][1], 0.0);
        assert_eq!(doc["accessors"][0]["min"][2], -1.0);
        assert_eq!(&g[20 + jl + 4..20 + jl + 8], b"BIN\0");
    }

    /// The small procedural city with every odd road unpaved (gravel / dirt) plus one of
    /// each Ashpeak County building kind and decor kind.
    fn county_sample() -> CityMap {
        use wp_world::network::{Decor, RoofShape};
        use wp_world::procgen::{generate, ProcGenConfig};
        let mut net = generate(&ProcGenConfig::small());
        for s in &mut net.surfaces {
            if s.road % 2 == 1 && !s.highway {
                s.material = if s.road % 4 == 1 {
                    RoadMaterial::Gravel
                } else {
                    RoadMaterial::Dirt
                };
            }
        }
        let sq = |x: f32, y: f32, h: f32| vec![[x, y], [x + h, y], [x + h, y + h], [x, y + h]];
        for (i, kind) in [
            BuildingKind::Farm,
            BuildingKind::Silo,
            BuildingKind::Landmark,
            BuildingKind::Construction,
        ]
        .into_iter()
        .enumerate()
        {
            let mut b = Building::block(sq(-900.0 + 40.0 * i as f32, -900.0, 12.0), 20.0, 0.0);
            b.kind = kind;
            if kind == BuildingKind::Farm {
                b.footprint = vec![
                    [-900.0, -900.0],
                    [-888.0, -900.0],
                    [-888.0, -880.0],
                    [-900.0, -880.0],
                ];
                b.roof = RoofShape::Gable;
                b.roof_rise_m = 4.0;
            }
            net.buildings.push(b);
        }
        for (i, kind) in [
            DecorKind::PineTree,
            DecorKind::Crane,
            DecorKind::JerseyBarrier,
            DecorKind::SiteFence,
            DecorKind::HayBale,
            DecorKind::Billboard,
        ]
        .into_iter()
        .enumerate()
        {
            net.decor.push(Decor {
                kind,
                pos: [-900.0 + 20.0 * i as f32, -950.0, 0.0],
                yaw: 0.3,
                size: [2.0, 2.0, 6.0],
            });
        }
        CityMap::new(net)
    }

    #[test]
    fn unpaved_roads_and_county_buildings_get_their_own_materials() {
        let map = county_sample();
        let prims = build_greybox(&map);
        let tris = |name: &str| {
            prims
                .iter()
                .find(|p| p.material == name)
                .map_or(0, |p| p.indices.len() / 3)
        };
        for name in [
            "asphalt",
            "gravel_road",
            "dirt_road",
            "barn",
            "barn_roof",
            "silo",
            "landmark",
            "construction",
            "construction_accent",
        ] {
            assert!(tris(name) > 0, "{name} missing");
        }
        // A city without unpaved roads or county buildings has none of these primitives.
        let plain = build_greybox(&CityMap::new(wp_world::procgen::generate(
            &wp_world::procgen::ProcGenConfig::small(),
        )));
        for name in ["gravel_road", "dirt_road", "barn", "construction_accent"] {
            assert!(plain.iter().all(|p| p.material != name), "{name}");
        }
        // Junctions where only dirt roads meet are dirt, mixed with asphalt are asphalt.
        let mats = node_materials(&map);
        assert!(mats.contains(&RoadMaterial::Dirt) || mats.contains(&RoadMaterial::Gravel));
        assert!(mats.contains(&RoadMaterial::Asphalt));
    }

    #[test]
    fn county_decor_is_shaped_and_instanced() {
        let map = county_sample();
        let inst = build_instanced(&map);
        for (name, materials) in [
            ("pines", &["pine_trunk", "pine_crown"][..]),
            (
                "cranes",
                &["crane", "crane_counterweight", "crane_cable"][..],
            ),
            ("jersey_barriers", &["jersey_barrier"][..]),
            ("site_fences", &["site_fence", "site_fence_post"][..]),
            ("hay_bales", &["hay_bale"][..]),
            ("billboards", &["billboard", "billboard_panel"][..]),
        ] {
            let m = inst
                .iter()
                .find(|i| i.name == name)
                .unwrap_or_else(|| panic!("{name}"));
            assert_eq!(m.instances.len(), 1, "{name}");
            let got: Vec<&str> = m.parts.iter().map(|p| p.material.as_str()).collect();
            assert_eq!(got, materials, "{name}");
            for p in &m.parts {
                assert!(!p.indices.is_empty() && p.indices.len() % 3 == 0, "{name}");
                // Small unit meshes (instancing keeps the glb small).
                assert!(p.positions.len() < 400, "{name}: {}", p.positions.len());
            }
        }
        // Pines no longer ride on the broadleaf tree mesh.
        assert!(inst.iter().all(|i| i.name != "trees"));
        // The crane's jib reaches well past its mast footprint; everything stands on z >= -0.1.
        let crane = inst.iter().find(|i| i.name == "cranes").unwrap();
        let max_y = crane.parts[0]
            .positions
            .iter()
            .map(|p| p[1])
            .fold(f32::MIN, f32::max);
        assert!(max_y > 10.0, "{max_y}");
        for i in &inst {
            for p in &i.parts {
                assert!(p.positions.iter().all(|q| q[2] >= -0.11), "{}", i.name);
            }
        }
        // The whole glb still writes.
        let glb = greybox_glb(&map);
        assert_eq!(&glb[0..4], b"glTF");
    }
}
