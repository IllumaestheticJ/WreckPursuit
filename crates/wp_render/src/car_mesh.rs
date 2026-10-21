//! Reader for the per-panel car meshes in `data/panels/meshes/<car>.json` (formats 1–3, written
//! by `tools/assets`, see `data/panels/README.md`). Only the fields the renderer needs are read;
//! the writer and validator stay in `car-assets`. Formats 2 and 3 only add the optional sheet
//! buckling binding (hood / trunk / fenders / rear quarters), which this reader ignores: those
//! panels skin like any other (no crumple bend yet).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use wp_contracts::BodyRegion;

/// Mesh file format this reader writes (test meshes) and the oldest it reads.
pub const MESH_FORMAT: u32 = 1;

/// Newest mesh file format this reader understands (`car_assets::mesh::MESH_FORMAT`).
pub const MESH_FORMAT_MAX: u32 = 3;

/// What a mesh part is (mirrors `car_assets::mesh::PartKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    /// Body panel skinned to its region.
    Panel,
    /// Window glass.
    Glass,
    /// Head/tail light lens.
    Light,
    /// Door mirror.
    Mirror,
    /// Fixed body shell (sills, pillars, roof).
    Body,
    /// Seats, dash.
    Interior,
    /// Tire (wheel part, hub space).
    Tire,
    /// Rim (wheel part, hub space).
    Rim,
}

/// Material slot of a part (mirrors `car_assets::mesh::MaterialSlot`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialSlot {
    /// Body colour.
    Paint,
    /// Transparent glass.
    Glass,
    /// Black plastic trim.
    Trim,
    /// Headlight (emissive).
    Headlight,
    /// Taillight (emissive).
    Taillight,
    /// Tire rubber.
    Rubber,
    /// Bare metal (rims).
    Metal,
    /// Cabin interior.
    Interior,
}

/// One part of a car mesh.
#[derive(Debug, Clone, Deserialize)]
pub struct MeshPart {
    /// Unique part name (`door_fl`, `wheel_fl_tire`, …).
    pub name: String,
    /// Body region the part is skinned to; `None` for wheel parts.
    pub region: Option<BodyRegion>,
    /// Part kind.
    pub kind: PartKind,
    /// Material slot.
    pub material: MaterialSlot,
    /// Wheel index (FL, FR, RL, RR) for tire/rim parts, which are in hub space (axle on +X).
    #[serde(default)]
    pub wheel: Option<u8>,
    /// xyz per vertex, mm (chassis space; wheel parts: hub space).
    pub positions_mm: Vec<i32>,
    /// Triangles, counter-clockwise from outside.
    pub indices: Vec<u32>,
    /// 4 lattice node ids per vertex; empty for wheel parts.
    #[serde(default)]
    pub skin_nodes: Vec<u16>,
    /// 4 weights per vertex in 1/255; empty for wheel parts.
    #[serde(default)]
    pub skin_weights: Vec<u8>,
}

impl MeshPart {
    /// Vertex count.
    pub fn vertex_count(&self) -> usize {
        self.positions_mm.len() / 3
    }

    /// Vertex `v` in metres.
    pub fn vertex(&self, v: usize) -> [f32; 3] {
        let p = &self.positions_mm[v * 3..v * 3 + 3];
        [p[0] as f32 * 1e-3, p[1] as f32 * 1e-3, p[2] as f32 * 1e-3]
    }
}

/// A whole car mesh file.
#[derive(Debug, Clone, Deserialize)]
pub struct CarMesh {
    /// Car id (= file stem).
    pub id: String,
    /// Lattice the skin binding refers to.
    pub lattice_id: String,
    /// File format, [`MESH_FORMAT`]..=[`MESH_FORMAT_MAX`].
    pub format: u32,
    /// Parts in file order.
    pub parts: Vec<MeshPart>,
}

impl CarMesh {
    /// Parse a mesh file and check its format and index ranges.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let mesh: CarMesh = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if !(MESH_FORMAT..=MESH_FORMAT_MAX).contains(&mesh.format) {
            return Err(format!(
                "{}: mesh format {} (expected {MESH_FORMAT}..={MESH_FORMAT_MAX})",
                mesh.id, mesh.format
            ));
        }
        for p in &mesh.parts {
            let n = p.vertex_count() as u32;
            if p.indices.iter().any(|&i| i >= n) {
                return Err(format!("{}/{}: index out of range", mesh.id, p.name));
            }
            if p.wheel.is_none() && p.skin_nodes.len() != p.vertex_count() * 4 {
                return Err(format!("{}/{}: skin binding size", mesh.id, p.name));
            }
        }
        Ok(mesh)
    }
}

/// Loads car meshes on demand from a directory and caches them (misses too, so a car without
/// a mesh costs one failed read).
#[derive(Debug, Default)]
pub struct CarMeshLibrary {
    dir: Option<PathBuf>,
    cache: BTreeMap<String, Option<Arc<CarMesh>>>,
}

impl CarMeshLibrary {
    /// Library reading `<dir>/<car_id>.json`.
    pub fn from_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: Some(dir.into()),
            cache: BTreeMap::new(),
        }
    }

    /// `data/panels/meshes` of this checkout (for tools and tests run from the repo).
    pub fn repo_default() -> Self {
        Self::from_dir(Self::repo_dir())
    }

    /// Path of `data/panels/meshes` in this checkout.
    pub fn repo_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/panels/meshes")
    }

    /// Add a mesh by hand (embedded data, browser fetch).
    pub fn insert(&mut self, mesh: CarMesh) {
        self.cache.insert(mesh.id.clone(), Some(Arc::new(mesh)));
    }

    /// The mesh of `car_id`, loading it on first use; `None` if missing or invalid.
    pub fn get(&mut self, car_id: &str) -> Option<Arc<CarMesh>> {
        if let Some(hit) = self.cache.get(car_id) {
            return hit.clone();
        }
        let loaded = self.dir.as_ref().and_then(|dir| {
            let text = std::fs::read_to_string(dir.join(format!("{car_id}.json"))).ok()?;
            CarMesh::from_json(&text).ok().map(Arc::new)
        });
        self.cache.insert(car_id.to_string(), loaded.clone());
        loaded
    }
}
