//! # car-assets (workstream R, prompt 21)
//!
//! Car asset pipeline: every car in `data/cars` gets
//! - a panel spec `data/panels/<car>.json` ([`wp_damage::PanelSpec`], `gen-panels`),
//! - a style sheet `data/panels/styles/<car>.toml` ([`style::CarStyle`], hand-written): the
//!   car's shape between its lattice anchors, lamps, bolt-on parts, wheels, palette and
//!   personality,
//! - a procedural low-poly mesh `data/panels/meshes/<car>.json` ([`mesh::CarMesh`],
//!   `gen-car-mesh`): one part per body region plus glass, lamps, bolt-on details, a small
//!   interior and wheels, skinned to the lattice by [`skin::bind_skin`] (`bind-skin`),
//! - the render list `data/panels/RENDER_LIST.md` ([`render_list`], `gen-car-mesh`),
//! - checked by [`validate::validate`] (`validate-car-asset`).
//!
//! Semi-trailers (`data/trailers/<id>.json`, same `VehicleDef` schema) get a style sheet and a
//! mesh the same way ([`trailer`], `bed = "trailer"`), but no panel spec: they have no doors,
//! hood or trunk lid that swing.
//!
//! See `README.md` for the file formats and commands.

#![warn(missing_docs)]

pub mod mesh;
pub mod parts;
pub mod profile;
pub mod render_list;
pub mod shell;
pub mod skin;
pub mod style;
pub mod trailer;
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

/// Every semi-trailer id in `data/trailers`, sorted.
pub fn trailer_ids(root: &Path) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(root.join("data/trailers"))
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

/// Every vehicle with a mesh: the cars (sorted), then the trailers (sorted).
pub fn vehicle_ids(root: &Path) -> Vec<String> {
    let mut ids = car_ids(root);
    ids.extend(trailer_ids(root));
    ids
}

/// True when `id` is a semi-trailer (`data/trailers/<id>.json`) rather than a car.
pub fn is_trailer(root: &Path, id: &str) -> bool {
    !root.join(format!("data/cars/{id}.json")).exists()
        && root.join(format!("data/trailers/{id}.json")).exists()
}

/// Path of a vehicle definition: `data/cars/<id>.json`, or `data/trailers/<id>.json` for a
/// trailer.
pub fn vehicle_path(root: &Path, id: &str) -> PathBuf {
    if is_trailer(root, id) {
        root.join(format!("data/trailers/{id}.json"))
    } else {
        root.join(format!("data/cars/{id}.json"))
    }
}

/// Load a car or trailer (definition + tires + lattice + materials).
pub fn load_car(root: &Path, id: &str) -> Result<LoadedCar, String> {
    wp_contracts::load_car(vehicle_path(root, id)).map_err(|e| e.to_string())
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

/// Path of a car's style sheet.
pub fn style_path(root: &Path, id: &str) -> PathBuf {
    root.join(format!("data/panels/styles/{id}.toml"))
}

/// Load a car's style sheet (every car has one).
pub fn load_style(root: &Path, id: &str) -> Result<style::CarStyle, String> {
    style::CarStyle::load(&style_path(root, id))
}

/// The generated, skinned mesh for a car (what `gen-car-mesh` writes).
pub fn build_mesh(car: &LoadedCar, style: &style::CarStyle) -> mesh::CarMesh {
    let mut m = mesh::generate(&car.def, &car.lattice, style);
    skin::bind_skin(&car.lattice, &mut m);
    m
}

/// Load a mesh file.
pub fn load_mesh(path: &Path) -> Result<mesh::CarMesh, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}
