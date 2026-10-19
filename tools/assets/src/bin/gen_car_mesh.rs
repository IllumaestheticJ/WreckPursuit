//! `gen-car-mesh [--check] [car ...]`: generate + skin the procedural mesh of every car (or
//! the named ones) into `data/panels/meshes/<car>.json`. `--check` writes nothing and exits 1
//! if a file on disk differs from what would be generated (stale).

use car_assets::{build_mesh, car_ids, load_car, mesh_path, repo_root};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = args.iter().any(|a| a == "--check");
    let root = repo_root();
    let mut ids: Vec<String> = args.into_iter().filter(|a| !a.starts_with("--")).collect();
    if ids.is_empty() {
        ids = car_ids(&root);
    }
    let mut stale = 0;
    for id in ids {
        let car = load_car(&root, &id).unwrap_or_else(|e| panic!("{id}: {e}"));
        let mesh = build_mesh(&car);
        let text = mesh.to_json();
        let path = mesh_path(&root, &id);
        if check {
            if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
                eprintln!("stale: {}", path.display());
                stale += 1;
            }
            continue;
        }
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        println!(
            "{id}: {} parts, {} tris -> {}",
            mesh.parts.len(),
            mesh.tri_count,
            path.display()
        );
    }
    if stale > 0 {
        eprintln!("{stale} stale mesh file(s): run `cargo run -p car-assets --bin gen-car-mesh`");
        std::process::exit(1);
    }
}
