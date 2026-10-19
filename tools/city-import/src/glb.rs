//! Greybox mesh builder and minimal glTF 2.0 binary (`.glb`) writer.
//!
//! Emits one mesh with a primitive per material: road ribbons (asphalt / highway concrete /
//! parking), junction boxes, raised sidewalks along ribbon edges, ground per district and
//! extruded buildings. World space is +Z up; glTF is +Y up, so positions are written as
//! `(x, z, -y)` (a rotation, so triangle winding is preserved). Units are metres.

use serde_json::json;
use wp_world::geom::{self, V2, V3};
use wp_world::map::{CityMap, SegKind};
use wp_world::network::{DistrictKind, LaneKind};

/// Sidewalk width, m.
const SIDEWALK_M: f32 = 3.0;
/// Sidewalk (curb) height, m.
const CURB_M: f32 = 0.15;

/// Triangle soup for one material.
#[derive(Debug, Clone, Default)]
pub struct Prim {
    /// Material name.
    pub material: String,
    /// Base colour RGBA.
    pub color: [f32; 4],
    /// Positions (world space, +Z up).
    pub positions: Vec<V3>,
    /// Normals (world space).
    pub normals: Vec<V3>,
    /// Triangle indices.
    pub indices: Vec<u32>,
}

impl Prim {
    fn new(material: &str, color: [f32; 4]) -> Self {
        Self {
            material: material.into(),
            color,
            ..Default::default()
        }
    }

    fn vert(&mut self, p: V3, n: V3) -> u32 {
        self.positions.push(p);
        self.normals.push(n);
        (self.positions.len() - 1) as u32
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
        let l = geom::offset_polyline(pts, -left);
        let r = geom::offset_polyline(pts, right);
        for k in 0..pts.len() - 1 {
            let z0 = pts[k][2] + dz;
            let z1 = pts[k + 1][2] + dz;
            self.quad(
                [l[k][0], l[k][1], z0],
                [r[k][0], r[k][1], z0],
                [r[k + 1][0], r[k + 1][1], z1],
                [l[k + 1][0], l[k + 1][1], z1],
                [0.0, 0.0, 1.0],
            );
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

/// Build the greybox primitives for a city.
pub fn build_greybox(map: &CityMap) -> Vec<Prim> {
    let net = &map.net;
    let mut asphalt = Prim::new("asphalt", [0.20, 0.20, 0.21, 1.0]);
    let mut concrete = Prim::new("highway_concrete", [0.42, 0.42, 0.40, 1.0]);
    let mut parking = Prim::new("parking", [0.26, 0.26, 0.27, 1.0]);
    let mut bus = Prim::new("bus_lane", [0.35, 0.16, 0.14, 1.0]);
    let mut sidewalk = Prim::new("sidewalk", [0.62, 0.61, 0.58, 1.0]);
    let mut buildings = Prim::new("building", [0.75, 0.74, 0.72, 1.0]);
    let mut ground: Vec<Prim> = DistrictKind::ALL
        .iter()
        .map(|k| Prim::new(&format!("ground_{k:?}").to_lowercase(), district_color(*k)))
        .collect();

    for z in &net.zones {
        ground[z.kind as usize].polygon(&z.polygon, -0.05);
    }
    for l in &net.lanes {
        let half = l.width_m * 0.5;
        let prim = match l.kind {
            LaneKind::Highway => &mut concrete,
            LaneKind::Parking => &mut parking,
            LaneKind::Bus => &mut bus,
            _ => &mut asphalt,
        };
        prim.ribbon(&l.polyline, half, half, 0.0);
        if l.kind != LaneKind::Highway {
            if l.edge_right {
                sidewalk.ribbon(&l.polyline, -half, half + SIDEWALK_M, CURB_M);
            }
            if l.edge_left {
                sidewalk.ribbon(&l.polyline, half + SIDEWALK_M, -half, CURB_M);
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
    for b in &net.buildings {
        buildings.walls(&b.footprint, 0.0, b.height_m);
        buildings.polygon(&b.footprint, b.height_m);
    }
    let mut out = vec![asphalt, concrete, parking, bus, sidewalk, buildings];
    out.extend(ground);
    out.retain(|p| !p.indices.is_empty());
    out
}

/// Encode primitives as a `.glb` (positions converted to glTF +Y-up).
pub fn write_glb(prims: &[Prim], name: &str) -> Vec<u8> {
    let mut bin: Vec<u8> = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut materials = Vec::new();
    let mut primitives = Vec::new();
    let to_gl = |p: V3| [p[0], p[2], -p[1]];
    for (mi, p) in prims.iter().enumerate() {
        materials.push(json!({
            "name": p.material,
            "pbrMetallicRoughness": {
                "baseColorFactor": p.color,
                "metallicFactor": 0.0,
                "roughnessFactor": 0.9
            }
        }));
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
            bin.extend_from_slice(&i.to_le_bytes());
        }
        let idx_len = bin.len() - idx_off;
        let v0 = views.len();
        views.push(
            json!({"buffer": 0, "byteOffset": pos_off, "byteLength": pos_len, "target": 34962}),
        );
        views.push(
            json!({"buffer": 0, "byteOffset": nrm_off, "byteLength": nrm_len, "target": 34962}),
        );
        views.push(
            json!({"buffer": 0, "byteOffset": idx_off, "byteLength": idx_len, "target": 34963}),
        );
        let a0 = accessors.len();
        accessors.push(json!({"bufferView": v0, "componentType": 5126, "count": p.positions.len(), "type": "VEC3", "min": lo, "max": hi}));
        accessors.push(json!({"bufferView": v0 + 1, "componentType": 5126, "count": p.normals.len(), "type": "VEC3"}));
        accessors.push(json!({"bufferView": v0 + 2, "componentType": 5125, "count": p.indices.len(), "type": "SCALAR"}));
        primitives.push(json!({
            "attributes": {"POSITION": a0, "NORMAL": a0 + 1},
            "indices": a0 + 2,
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
