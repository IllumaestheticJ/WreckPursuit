//! `gen-panels [--check] [car ...]`: write the first-pass panel spec of every car (or the
//! named ones) to `data/panels/<car>.json` from its lattice (`wp_damage::PanelSpec::derive`).
//! `--check` writes nothing and exits 1 if a file differs from the derived spec. Hand-tuned
//! files are fine: just do not run without `--check` over them.

use car_assets::{car_ids, derive_panels, load_car, panel_path, panels_json, repo_root};

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
        let spec = derive_panels(&car);
        spec.validate(&car.lattice)
            .unwrap_or_else(|e| panic!("{e}"));
        let text = panels_json(&spec);
        let path = panel_path(&root, &id);
        if check {
            if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
                eprintln!("differs: {}", path.display());
                stale += 1;
            }
            continue;
        }
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        let names: Vec<&str> = spec.panels.iter().map(|p| p.name.as_str()).collect();
        println!("{id}: {}", names.join(", "));
    }
    if stale > 0 {
        std::process::exit(1);
    }
}
