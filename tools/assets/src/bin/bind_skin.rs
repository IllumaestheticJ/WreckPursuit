//! `bind-skin <mesh.json> [<lattice.json>]`: (re)compute the skin binding of an existing mesh
//! file in place: 4 nearest lattice nodes per vertex, inverse-distance weights, region-aware.
//! The lattice defaults to `data/lattices/<mesh lattice_id>.json`. Use it after a lattice is
//! regenerated, or on a hand-made mesh in the same format.

use std::path::PathBuf;

use car_assets::{load_mesh, repo_root, skin::bind_skin};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(mesh_file) = args.first() else {
        eprintln!("usage: bind-skin <mesh.json> [<lattice.json>]");
        std::process::exit(2);
    };
    let mesh_path = PathBuf::from(mesh_file);
    let mut mesh = load_mesh(&mesh_path).unwrap_or_else(|e| panic!("{e}"));
    let lattice_path = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join(format!("data/lattices/{}.json", mesh.lattice_id)));
    let lattice = wp_contracts::load_lattice_def(&lattice_path).unwrap_or_else(|e| panic!("{e}"));
    bind_skin(&lattice, &mut mesh);
    std::fs::write(&mesh_path, mesh.to_json()).unwrap();
    let verts: usize = mesh.parts.iter().map(|p| p.vertex_count()).sum();
    println!(
        "{}: bound {verts} vertices to {}",
        mesh_path.display(),
        lattice.id
    );
}
