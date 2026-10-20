//! Greybox mesh builder and minimal glTF 2.0 binary (`.glb`) writer.
//!
//! Emits one mesh with a primitive per material: road ribbons (asphalt / highway concrete /
//! parking / bus, banked like the road surface), junction boxes, raised sidewalks with kerb
//! faces along ribbon edges, ground per district and extruded buildings. Cities with terrain
//! (Port Vesper v2) also get the terrain mesh (decimated, never above a road), a sea plane,
//! retaining walls where roads sit above the ground, bridge decks with parapets and
//! piers, and tunnels / covered roads (walls, roof deck, emissive ceiling lights). World space is +Z up; glTF is +Y up, so positions are written as `(x, z, -y)` (a
//! rotation, so triangle winding is preserved). Units are metres. Primitives with more than
//! 65 535 vertices are split so every index buffer is 16-bit.

use serde_json::json;
use wp_world::geom::{self, V2, V3};
use wp_world::map::{CityMap, SegKind};
use wp_world::network::{DistrictKind, Lane, LaneKind, RoadNetwork};
use wp_world::surface::{is_elevated, surface_edges};
use wp_world::terrain::Heightfield;

/// Sidewalk width, m.
const SIDEWALK_M: f32 = 3.0;
/// Sidewalk (curb) height, m.
const CURB_M: f32 = 0.15;
/// Render terrain vertex spacing (cities with terrain), m.
const TERRAIN_RENDER_M: f32 = 10.0;
/// Sea level drawn as water, m.
const SEA_Z: f32 = 0.25;

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

/// Cross slope of `lane` per polyline point, in its own direction (from its road surface).
fn lane_bank(net: &RoadNetwork, lane: &Lane) -> Vec<f32> {
    let Some(s) = net.surfaces.get(lane.road as usize) else {
        return Vec::new();
    };
    let n = s.centre.len();
    if s.road != lane.road || n != lane.polyline.len() {
        return Vec::new();
    }
    let p0 = geom::xy(lane.polyline[0]);
    let fwd = geom::dist(p0, geom::xy(s.centre[0])) <= geom::dist(p0, geom::xy(s.centre[n - 1]));
    if fwd {
        s.bank.clone()
    } else {
        s.bank.iter().rev().map(|b| -b).collect()
    }
}

/// Render terrain: every `k`-th vertex, each lowered to the minimum of the fine heights around
/// it, so the coarse surface never rises above the fine one (and so never above a road).
fn render_terrain(
    t: &Heightfield,
    net: &RoadNetwork,
    ground: &mut [Prim],
    rock: &mut Prim,
    seabed: &mut Prim,
) {
    let k = ((TERRAIN_RENDER_M / t.cell_m).round() as u32).max(1);
    let cx = (t.nx - 1).div_ceil(k) + 1;
    let cy = (t.ny - 1).div_ceil(k) + 1;
    let fine = |i: u32, j: u32| (i.min(t.nx - 1), j.min(t.ny - 1));
    let mut z = vec![0.0f32; (cx * cy) as usize];
    let mut pos = vec![[0.0f32; 2]; (cx * cy) as usize];
    for j in 0..cy {
        for i in 0..cx {
            let (fi, fj) = fine(i * k, j * k);
            let mut m = f32::INFINITY;
            for dj in -(k as i64)..=(k as i64) {
                for di in -(k as i64)..=(k as i64) {
                    let a = (fi as i64 + di).clamp(0, t.nx as i64 - 1) as u32;
                    let b = (fj as i64 + dj).clamp(0, t.ny as i64 - 1) as u32;
                    m = m.min(t.at(a, b));
                }
            }
            z[(j * cx + i) as usize] = m - 0.05;
            pos[(j * cx + i) as usize] = t.vertex_pos(fi, fj);
        }
    }
    let zone_kind = |p: V2| {
        net.zones
            .iter()
            .find(|zn| geom::point_in_polygon(p, &zn.polygon))
            .map_or(DistrictKind::Suburb, |zn| zn.kind)
    };
    // Smooth normals from central differences on the coarse grid.
    let at = |i: u32, j: u32| z[(j.min(cy - 1) * cx + i.min(cx - 1)) as usize];
    let step = (k as f32) * t.cell_m;
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

/// Build the greybox primitives for a city.
pub fn build_greybox(map: &CityMap) -> Vec<Prim> {
    let net = &map.net;
    let mut asphalt = Prim::new("asphalt", [0.20, 0.20, 0.21, 1.0]);
    let mut concrete = Prim::new("highway_concrete", [0.42, 0.42, 0.40, 1.0]);
    let mut parking = Prim::new("parking", [0.26, 0.26, 0.27, 1.0]);
    let mut bus = Prim::new("bus_lane", [0.35, 0.16, 0.14, 1.0]);
    let mut sidewalk = Prim::new("sidewalk", [0.62, 0.61, 0.58, 1.0]);
    let mut kerb = Prim::new("kerb", [0.70, 0.69, 0.66, 1.0]);
    let mut buildings = Prim::new("building", [0.75, 0.74, 0.72, 1.0]);
    let mut wall = Prim::new("retaining_wall", [0.52, 0.50, 0.46, 1.0]);
    let mut bridge = Prim::new("bridge", [0.58, 0.58, 0.56, 1.0]);
    let mut rock = Prim::smooth("rock", [0.45, 0.42, 0.37, 1.0]);
    let mut seabed = Prim::smooth("seabed", [0.55, 0.52, 0.40, 1.0]);
    let mut water = Prim::new("water", [0.16, 0.30, 0.45, 1.0]);
    let mut ground: Vec<Prim> = DistrictKind::ALL
        .iter()
        .map(|k| Prim::smooth(&format!("ground_{k:?}").to_lowercase(), district_color(*k)))
        .collect();

    let terrain = net.terrain.as_ref();
    match terrain {
        Some(t) => {
            render_terrain(t, net, &mut ground, &mut rock, &mut seabed);
            let (lo, hi) = (t.origin, t.max_corner());
            let m = 400.0;
            water.quad(
                [lo[0] - m, lo[1] - m, SEA_Z],
                [hi[0] + m, lo[1] - m, SEA_Z],
                [hi[0] + m, hi[1] + m, SEA_Z],
                [lo[0] - m, hi[1] + m, SEA_Z],
                [0.0, 0.0, 1.0],
            );
        }
        None => {
            for z in &net.zones {
                ground[z.kind as usize].polygon(&z.polygon, -0.05);
            }
        }
    }
    for l in &net.lanes {
        let half = l.width_m * 0.5;
        let bank = lane_bank(net, l);
        let prim = match l.kind {
            LaneKind::Highway => &mut concrete,
            LaneKind::Parking => &mut parking,
            LaneKind::Bus => &mut bus,
            _ => &mut asphalt,
        };
        prim.ribbon_banked(&l.polyline, half, half, 0.0, &bank);
        if l.kind != LaneKind::Highway {
            let edge = |side: f32| -> Vec<V3> {
                let mut e = geom::offset_polyline(&l.polyline, side * half);
                for (k, p) in e.iter_mut().enumerate() {
                    p[2] += bank.get(k).copied().unwrap_or(0.0) * side * half;
                }
                e
            };
            if l.edge_right {
                sidewalk.ribbon_banked(&l.polyline, -half, half + SIDEWALK_M, CURB_M, &bank);
                let top: Vec<V3> = edge(1.0)
                    .iter()
                    .map(|p| [p[0], p[1], p[2] + CURB_M])
                    .collect();
                let bottom: Vec<f32> = edge(1.0).iter().map(|p| p[2]).collect();
                kerb.curtain(&top, &bottom, false);
            }
            if l.edge_left {
                sidewalk.ribbon_banked(&l.polyline, half + SIDEWALK_M, -half, CURB_M, &bank);
                let top: Vec<V3> = edge(-1.0)
                    .iter()
                    .map(|p| [p[0], p[1], p[2] + CURB_M])
                    .collect();
                let bottom: Vec<f32> = edge(-1.0).iter().map(|p| p[2]).collect();
                kerb.curtain(&top, &bottom, true);
            }
        }
    }
    // Junction boxes (16-gons) and bend connectors.
    for ix in &net.intersections {
        let c = net.nodes[ix.node as usize].pos;
        let poly: Vec<V2> = (0..16)
            .map(|k| {
                let (s, co) = wp_math::sin_cos(k as f32 / 16.0 * std::f32::consts::TAU);
                [c[0] + co * ix.radius_m, c[1] + s * ix.radius_m]
            })
            .collect();
        asphalt.polygon(&poly, c[2] - 0.01);
    }
    for s in &map.segs {
        if let SegKind::Connector { node, .. } = s.kind {
            if net.nodes[node as usize].intersection.is_none() {
                asphalt.ribbon(&s.pts, s.width * 0.5, s.width * 0.5, -0.005);
            }
        }
    }
    // Hilly cities: retaining walls where a road edge stands above the (render) ground, and
    // bridge decks with parapets and piers.
    if let Some(t) = terrain {
        let ground_z = |q: V2| t.height_at(q);
        for s in &net.surfaces {
            let (l, r) = surface_edges(s);
            let n = s.centre.len();
            let deck: Vec<bool> = (0..n).map(|k| is_elevated(s, k, &ground_z, 1.5)).collect();
            // Retaining walls on the outer kerb line (sidewalk roads) or the paved edge.
            let out = if s.highway { 0.3 } else { SIDEWALK_M };
            for (side, edge) in [(-1.0f32, &l), (1.0, &r)] {
                let line = geom::offset_polyline(edge, side * out);
                let top: Vec<V3> = line
                    .iter()
                    .zip(edge.iter())
                    .map(|(p, e)| [p[0], p[1], e[2] + if s.highway { 0.0 } else { CURB_M }])
                    .collect();
                let bottom: Vec<f32> = top
                    .iter()
                    .zip(&deck)
                    .map(|(p, d)| {
                        if *d {
                            p[2]
                        } else {
                            (ground_z(geom::xy(*p)) - 1.5).min(p[2])
                        }
                    })
                    .collect();
                wall.curtain(&top, &bottom, side > 0.0);
            }
            // Decks: 1.2 m deep slab sides, 0.9 m parapets, piers every ~30 m.
            let cum = geom::cumulative(&s.centre);
            let mut next_pier = 0.0;
            for k in 0..n - 1 {
                if !(deck[k] && deck[k + 1]) {
                    continue;
                }
                for (side, e) in [(-1.0f32, &l), (1.0, &r)] {
                    let seg = [e[k], e[k + 1]];
                    bridge.curtain(&seg, &[seg[0][2] - 1.2, seg[1][2] - 1.2], side > 0.0);
                    let top = [
                        [seg[0][0], seg[0][1], seg[0][2] + 0.9],
                        [seg[1][0], seg[1][1], seg[1][2] + 0.9],
                    ];
                    bridge.curtain(&top, &[seg[0][2], seg[1][2]], side > 0.0);
                }
                if cum[k] >= next_pier {
                    next_pier = cum[k] + 30.0;
                    let c = geom::xy(s.centre[k]);
                    let (d, _) = (geom::normalize(geom::sub(geom::xy(s.centre[k + 1]), c)), 0);
                    let r = geom::right_of(d);
                    let half_w = 0.4 * (s.left_m + s.right_m);
                    let corner = |a: f32, b: f32| {
                        geom::add(c, geom::add(geom::scale(d, a), geom::scale(r, b)))
                    };
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
                    let base = fp
                        .iter()
                        .map(|q| ground_z(*q))
                        .fold(f32::INFINITY, f32::min)
                        - 0.5;
                    bridge.block(&fp, base, s.centre[k][2] - 1.2);
                }
            }
        }
    }
    for b in &net.buildings {
        buildings.prism(&b.footprint, b.base_z_m, b.roof_z());
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
    let mut out = vec![
        asphalt,
        concrete,
        parking,
        bus,
        sidewalk,
        kerb,
        buildings,
        wall,
        bridge,
        rock,
        seabed,
        water,
        tunnel_wall,
        tunnel_roof,
        tunnel_light,
    ];
    out.extend(ground);
    out.retain(|p| !p.indices.is_empty());
    out
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

/// Encode primitives as a `.glb` (positions converted to glTF +Y-up).
pub fn write_glb(prims: &[Prim], name: &str) -> Vec<u8> {
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
    let to_gl = |p: V3| [p[0], p[2], -p[1]];
    for p in prims {
        materials.push(json!({
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
        }));
    }
    for (mi, p) in &parts {
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
        views.push(
            json!({"buffer": 0, "byteOffset": pos_off, "byteLength": pos_len, "target": 34962}),
        );
        let nrm_view = views.len();
        if has_n {
            views.push(
                json!({"buffer": 0, "byteOffset": nrm_off, "byteLength": nrm_len, "target": 34962}),
            );
        }
        let idx_view = views.len();
        views.push(
            json!({"buffer": 0, "byteOffset": idx_off, "byteLength": idx_len, "target": 34963}),
        );
        let pos_acc = accessors.len();
        accessors.push(json!({"bufferView": pos_view, "componentType": 5126, "count": p.positions.len(), "type": "VEC3", "min": lo, "max": hi}));
        let mut attributes = json!({ "POSITION": pos_acc });
        if has_n {
            attributes["NORMAL"] = json!(accessors.len());
            accessors.push(json!({"bufferView": nrm_view, "componentType": 5126, "count": p.normals.len(), "type": "VEC3"}));
        }
        let idx_acc = accessors.len();
        accessors.push(json!({"bufferView": idx_view, "componentType": 5123, "count": p.indices.len(), "type": "SCALAR"}));
        primitives.push(json!({
            "attributes": attributes,
            "indices": idx_acc,
            "material": mi,
            "mode": 4
        }));
    }
    let doc = json!({
        "asset": {"version": "2.0", "generator": "wreckpursuit city-import"},
        "scene": 0,
        "scenes": [{"name": name, "nodes": [0]}],
        "nodes": [{"name": name, "mesh": 0}],
        "meshes": [{"name": name, "primitives": primitives}],
        "materials": materials,
        "buffers": [{"byteLength": bin.len()}],
        "bufferViews": views,
        "accessors": accessors
    });
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
}
