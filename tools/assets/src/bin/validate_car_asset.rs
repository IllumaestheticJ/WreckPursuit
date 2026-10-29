//! `validate-car-asset [car ...]`: validate every car's mesh and panel spec (or the named
//! cars'). Exits 1 on any problem.

use car_assets::{
    car_ids, load_car, load_mesh, mesh_path, panel_path, repo_root,
    validate::{validate, validate_detail_file},
};

fn main() {
    let root = repo_root();
    let mut ids: Vec<String> = std::env::args().skip(1).collect();
    if ids.is_empty() {
        ids = car_ids(&root);
    }
    let mut bad = 0;
    for id in ids {
        let car = match load_car(&root, &id) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{id}: {e}");
                bad += 1;
                continue;
            }
        };
        let spec = match wp_damage::PanelSpec::load(&panel_path(&root, &id)) {
            Ok(s) => match s.validate(&car.lattice) {
                Ok(()) => Some(s),
                Err(e) => {
                    eprintln!("{e}");
                    bad += 1;
                    None
                }
            },
            Err(e) => {
                eprintln!("{e}");
                bad += 1;
                None
            }
        };
        let mesh = match load_mesh(&mesh_path(&root, &id)) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("{e}");
                bad += 1;
                continue;
            }
        };
        let mut errs = validate(&car, &mesh, spec.as_ref());
        errs.extend(validate_detail_file(&mesh));
        if errs.is_empty() {
            println!(
                "ok  {id}: {} parts, {} tris",
                mesh.parts.len(),
                mesh.tri_count
            );
        } else {
            for e in &errs {
                eprintln!("ERR {e}");
            }
            bad += 1;
        }
    }
    if bad > 0 {
        std::process::exit(1);
    }
}
