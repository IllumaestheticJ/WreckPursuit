//! The city greybox: roads, junctions, sidewalks and kerbs, district ground or terrain, sea,
//! retaining walls, bridges and buildings, read from the same `data/city/<id>.greybox.glb` the
//! browser testbed draws (written by `tools/city-import`, `gen-city`).
//!
//! The reader handles what `city-import` writes: glTF 2.0 binary, one buffer, triangle
//! primitives with `POSITION`, optional `NORMAL` (smooth-shaded terrain) and 8/16/32-bit
//! indices, one `baseColorFactor` per material. Node transforms are ignored (the exporter
//! writes none). glTF is +Y up; world is +Z up: world `(x, y, z)` = glTF `(x, -z, y)`.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::skin::StaticMesh;
use crate::{Shade, Vertex};

/// Number of decal layers (see [`decal_layer`]).
pub const DECAL_LAYERS: usize = 3;

/// Which depth-bias layer a greybox material draws in. Layers that share a plane draw over the
/// layer below instead of flickering: 2 = bus lanes and parking painted on asphalt, 1 = asphalt,
/// highway concrete and sidewalks over zone ground, 0 = everything else. Same table as the
/// browser (`apps/testbed-web/src/scene.ts`).
pub fn decal_layer(material: &str) -> usize {
    match material {
        "bus_lane" | "parking" => 2,
        "sidewalk" | "asphalt" | "highway_concrete" => 1,
        _ => 0,
    }
}

/// A city greybox as world-space meshes, one per decal layer.
#[derive(Debug, Clone, Default)]
pub struct CityMesh {
    /// City id (`procedural_12x12`, `vesper_v2`, ...).
    pub id: String,
    /// Meshes by [`decal_layer`]; index 0 is the base layer.
    pub layers: [StaticMesh; DECAL_LAYERS],
    /// World-space bounds `(min, max)`, m.
    pub bounds: ([f32; 3], [f32; 3]),
}

impl CityMesh {
    /// Path of `data/city/<id>.greybox.glb` in this checkout.
    pub fn repo_path(id: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/city")
            .join(format!("{id}.greybox.glb"))
    }

    /// Load `data/city/<id>.greybox.glb` from this checkout.
    pub fn load_repo(id: &str) -> Result<Self, String> {
        let path = Self::repo_path(id);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_glb(id, &bytes)
    }

    /// Parse a greybox `.glb` (see the module docs for what is supported).
    pub fn from_glb(id: &str, bytes: &[u8]) -> Result<Self, String> {
        let (doc, bin) = split_glb(bytes)?;
        let mut out = CityMesh {
            id: id.into(),
            bounds: ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]),
            ..Default::default()
        };
        let materials = doc["materials"].as_array().cloned().unwrap_or_default();
        for mesh in doc["meshes"].as_array().into_iter().flatten() {
            for prim in mesh["primitives"].as_array().into_iter().flatten() {
                if prim["mode"].as_u64().unwrap_or(4) != 4 {
                    return Err("only triangle primitives are supported".into());
                }
                let mat = prim["material"]
                    .as_u64()
                    .and_then(|m| materials.get(m as usize));
                let name = mat.and_then(|m| m["name"].as_str()).unwrap_or("");
                let mut color = [1.0f32; 4];
                if let Some(c) =
                    mat.and_then(|m| m["pbrMetallicRoughness"]["baseColorFactor"].as_array())
                {
                    for (k, v) in c.iter().take(4).enumerate() {
                        color[k] = v.as_f64().unwrap_or(1.0) as f32;
                    }
                }
                let attrs = &prim["attributes"];
                let pos_acc = attrs["POSITION"]
                    .as_u64()
                    .ok_or("primitive without POSITION")?;
                let positions = read_vec3(&doc, bin, pos_acc as usize)?;
                let normals = match attrs["NORMAL"].as_u64() {
                    Some(a) => Some(read_vec3(&doc, bin, a as usize)?),
                    None => None,
                };
                let indices = match prim["indices"].as_u64() {
                    Some(a) => read_indices(&doc, bin, a as usize)?,
                    None => (0..positions.len() as u32).collect(),
                };
                if indices.iter().any(|&i| i as usize >= positions.len()) {
                    return Err(format!("{name}: index out of range"));
                }

                let shade = if name == "building" {
                    Shade::Building
                } else {
                    Shade::Lit
                };
                let mut vertices: Vec<Vertex> = positions
                    .iter()
                    .enumerate()
                    .map(|(k, g)| Vertex {
                        pos: to_world(*g),
                        color,
                        shade: shade as u32 as f32,
                        normal: normals.as_ref().map_or([0.0; 3], |n| to_world(n[k])),
                    })
                    .collect();
                let mut indices = indices;
                if shade == Shade::Building {
                    (vertices, indices) = with_face_normals(&vertices, &indices);
                }
                for v in &vertices {
                    for (a, c) in v.pos.iter().enumerate() {
                        out.bounds.0[a] = out.bounds.0[a].min(*c);
                        out.bounds.1[a] = out.bounds.1[a].max(*c);
                    }
                }
                let layer = &mut out.layers[decal_layer(name)];
                let base = layer.vertices.len() as u32;
                layer.vertices.extend(vertices);
                layer.indices.extend(indices.iter().map(|i| base + i));
            }
        }
        if out.layers.iter().all(|l| l.indices.is_empty()) {
            return Err("greybox has no triangles".into());
        }
        Ok(out)
    }

    /// Total triangle count.
    pub fn triangle_count(&self) -> usize {
        self.layers.iter().map(|l| l.indices.len() / 3).sum()
    }
}

/// Unshare vertices so every triangle carries its exact face normal. Building walls need it:
/// the window pattern is laid out along the wall's direction, and a normal derived per pixel
/// from screen-space derivatives is too noisy for that at city-scale coordinates.
fn with_face_normals(vertices: &[Vertex], indices: &[u32]) -> (Vec<Vertex>, Vec<u32>) {
    let mut out = Vec::with_capacity(indices.len());
    for tri in indices.chunks_exact(3) {
        let p = [0, 1, 2].map(|k| glam::Vec3::from(vertices[tri[k] as usize].pos));
        let n = (p[1] - p[0])
            .cross(p[2] - p[0])
            .normalize_or_zero()
            .to_array();
        out.extend(tri.iter().map(|&i| Vertex {
            normal: n,
            ..vertices[i as usize]
        }));
    }
    let indices = (0..out.len() as u32).collect();
    (out, indices)
}

/// glTF (+Y up) → world (+Z up).
fn to_world(g: [f32; 3]) -> [f32; 3] {
    [g[0], -g[2], g[1]]
}

/// Split a `.glb` into its JSON document and binary chunk.
fn split_glb(bytes: &[u8]) -> Result<(Value, &[u8]), String> {
    let u32_at = |at: usize| -> Result<u32, String> {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| "truncated .glb".to_string())
    };
    if bytes.get(0..4) != Some(b"glTF") || u32_at(4)? != 2 {
        return Err("not a glTF 2.0 binary".into());
    }
    let json_len = u32_at(12)? as usize;
    if bytes.get(16..20) != Some(b"JSON") {
        return Err("first .glb chunk is not JSON".into());
    }
    let json = bytes.get(20..20 + json_len).ok_or("truncated .glb JSON")?;
    let doc: Value = serde_json::from_slice(json).map_err(|e| format!("glTF JSON: {e}"))?;
    let at = 20 + json_len;
    let bin = if at < bytes.len() {
        let bin_len = u32_at(at)? as usize;
        if bytes.get(at + 4..at + 8) != Some(b"BIN\0") {
            return Err("second .glb chunk is not BIN".into());
        }
        bytes
            .get(at + 8..at + 8 + bin_len)
            .ok_or("truncated .glb BIN")?
    } else {
        &[]
    };
    Ok((doc, bin))
}

/// Accessor `a`: (bytes of its buffer view from the accessor's start, element count, component
/// type, stride in bytes or 0 for tightly packed).
fn accessor<'a>(
    doc: &Value,
    bin: &'a [u8],
    a: usize,
) -> Result<(&'a [u8], usize, u64, usize), String> {
    let acc = &doc["accessors"][a];
    let count = acc["count"].as_u64().ok_or("accessor without count")? as usize;
    let ctype = acc["componentType"]
        .as_u64()
        .ok_or("accessor without type")?;
    let view = &doc["bufferViews"][acc["bufferView"].as_u64().ok_or("sparse accessor")? as usize];
    if view["buffer"].as_u64().unwrap_or(0) != 0 {
        return Err("only one buffer is supported".into());
    }
    let start = view["byteOffset"].as_u64().unwrap_or(0) as usize
        + acc["byteOffset"].as_u64().unwrap_or(0) as usize;
    let end = view["byteOffset"].as_u64().unwrap_or(0) as usize
        + view["byteLength"]
            .as_u64()
            .ok_or("buffer view without length")? as usize;
    let data = bin.get(start..end).ok_or("buffer view outside BIN")?;
    let stride = view["byteStride"].as_u64().unwrap_or(0) as usize;
    Ok((data, count, ctype, stride))
}

fn read_vec3(doc: &Value, bin: &[u8], a: usize) -> Result<Vec<[f32; 3]>, String> {
    let (data, count, ctype, stride) = accessor(doc, bin, a)?;
    if ctype != 5126 || doc["accessors"][a]["type"] != "VEC3" {
        return Err("expected a float VEC3 accessor".into());
    }
    let stride = if stride == 0 { 12 } else { stride };
    (0..count)
        .map(|i| {
            let e = data
                .get(i * stride..i * stride + 12)
                .ok_or("accessor outside its buffer view")?;
            let f = |k: usize| f32::from_le_bytes([e[k], e[k + 1], e[k + 2], e[k + 3]]);
            Ok([f(0), f(4), f(8)])
        })
        .collect()
}

fn read_indices(doc: &Value, bin: &[u8], a: usize) -> Result<Vec<u32>, String> {
    let (data, count, ctype, _) = accessor(doc, bin, a)?;
    let size = match ctype {
        5121 => 1,
        5123 => 2,
        5125 => 4,
        _ => return Err("index accessor is not u8/u16/u32".into()),
    };
    let data = data
        .get(..count * size)
        .ok_or("index accessor outside its buffer view")?;
    Ok(data
        .chunks_exact(size)
        .map(|b| match size {
            1 => b[0] as u32,
            2 => u16::from_le_bytes([b[0], b[1]]) as u32,
            _ => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decal_layers_match_the_browser() {
        assert_eq!(decal_layer("parking"), 2);
        assert_eq!(decal_layer("asphalt"), 1);
        assert_eq!(decal_layer("building"), 0);
    }

    #[test]
    fn rejects_garbage() {
        assert!(CityMesh::from_glb("x", b"not a glb").is_err());
        assert!(CityMesh::from_glb("x", b"glTF\x02\0\0\0").is_err());
    }
}
