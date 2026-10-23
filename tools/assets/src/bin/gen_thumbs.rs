//! `gen-thumbs [--check] [car ...]`: render every car's menu thumbnail and turntable strip
//! (`data/cars/thumbs/<car>.png`, `<car>.turn.png`, [`car_assets::thumb`]) from its panel mesh.
//! `--check` writes nothing and exits 1 if a file is missing or stale.

use car_assets::thumb::{is_current, thumb_path, thumbnail, turntable, turntable_path, Image};
use car_assets::{car_ids, load_car, load_mesh, mesh_path, repo_root};

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
        let mesh = load_mesh(&mesh_path(&root, &id)).unwrap_or_else(|e| panic!("{id}: {e}"));
        for (img, path) in [
            (thumbnail(&mesh, &car.def), thumb_path(&root, &id)),
            (turntable(&mesh, &car.def), turntable_path(&root, &id)),
        ] {
            if check {
                let ok = std::fs::read(&path)
                    .ok()
                    .and_then(|b| Image::from_png(&b).ok())
                    .is_some_and(|d| is_current(&d, &img));
                if !ok {
                    eprintln!("stale: {}", path.display());
                    stale += 1;
                }
                continue;
            }
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, img.to_png()).unwrap();
            println!("{id}: {}x{} -> {}", img.width, img.height, path.display());
        }
    }
    if stale > 0 {
        eprintln!(
            "{stale} stale thumbnail(s): run `cargo run --release -p car-assets --bin gen-thumbs`"
        );
        std::process::exit(1);
    }
}
