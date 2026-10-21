//! # car-assets (workstream R, prompt 21)
//!
//! Car asset pipeline: every car in `data/cars` gets
//! - a panel spec `data/panels/<car>.json` ([`wp_damage::PanelSpec`], `gen-panels`),
//! - a procedural low-poly mesh `data/panels/meshes/<car>.json` ([`mesh::CarMesh`],
//!   `gen-car-mesh`): one part per body region plus glass, lights, mirrors, a minimal
//!   interior and wheels, skinned to the lattice by [`skin::bind_skin`] (`bind-skin`),
//! - checked by [`validate::validate`] (`validate-car-asset`).
//!
//! The body shape, details, wheels and mechanical parts of each car come from its
//! [`style::CarStyle`] ([`style::style_for`]): per-car style modules in [`styles`], the
//! loft in [`profile`] / [`mesh`], primitives in [`builder`], wheels in [`wheels`] and
//! engine / suspension / brakes in [`mechanical`].
//!
//! See `README.md` for the file formats and commands.

#![warn(missing_docs)]

pub mod builder;
pub mod mechanical;
pub mod mesh;
pub mod profile;
pub mod skin;
pub mod style;
pub mod styles;
pub mod validate;
pub mod wheels;

use std::path::{Path, PathBuf};

use wp_contracts::LoadedCar;
use wp_damage::{DeriveOptions, PanelSpec};

/// Repository root (from this crate's manifest dir).
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every car id in `data/cars`, sorted.
pub fn car_ids(root: &Path) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(root.join("data/cars"))
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
                .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids
}

/// Load a car (definition + tires + lattice + materials).
pub fn load_car(root: &Path, id: &str) -> Result<LoadedCar, String> {
    wp_contracts::load_car(root.join(format!("data/cars/{id}.json"))).map_err(|e| e.to_string())
}

/// Path of a car's panel spec.
pub fn panel_path(root: &Path, id: &str) -> PathBuf {
    root.join(format!("data/panels/{id}.json"))
}

/// Path of a car's mesh.
pub fn mesh_path(root: &Path, id: &str) -> PathBuf {
    root.join(format!("data/panels/meshes/{id}.json"))
}

/// The generated panel spec for a car (what `gen-panels` writes).
pub fn derive_panels(car: &LoadedCar) -> PanelSpec {
    PanelSpec::derive(
        &car.def.id,
        &car.lattice,
        DeriveOptions {
            body_size_m: car.def.body_size_m,
            fenders: true,
        },
    )
}

/// Panel spec as written to disk (pretty JSON + newline).
pub fn panels_json(spec: &PanelSpec) -> String {
    let mut s = serde_json::to_string_pretty(spec).expect("serialisable");
    s.push('\n');
    s
}

/// The generated, skinned mesh for a car (what `gen-car-mesh` writes).
pub fn build_mesh(car: &LoadedCar) -> mesh::CarMesh {
    let mut m = mesh::generate(&car.def, &car.lattice);
    skin::bind_skin(&car.lattice, &mut m);
    m
}

/// Load a mesh file.
pub fn load_mesh(path: &Path) -> Result<mesh::CarMesh, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}
