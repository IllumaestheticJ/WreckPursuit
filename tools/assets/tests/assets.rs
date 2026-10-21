//! Every car has an up-to-date, valid mesh and panel spec; meshes deform with the lattice and
//! hinged parts line up with their hinge nodes.

use car_assets::mesh::{CarMesh, PartKind, TRI_BUDGET};
use car_assets::render_list::{render, Entry, RENDER_LIST_PATH};
use car_assets::{
    build_mesh, car_ids, is_trailer, load_car, load_mesh, load_style, mesh_path, panel_path,
    repo_root, validate::validate, vehicle_ids,
};
use wp_contracts::BodyRegion;
use wp_damage::PanelSpec;

/// Every car (and trailer) mesh is valid and up to date; cars also have a valid panel spec
/// (trailers have none); the render list covers them all.
#[test]
fn every_car_has_a_valid_up_to_date_mesh_and_panel_spec() {
    let root = repo_root();
    assert!(car_ids(&root).len() >= 16);
    let mut built = Vec::new();
    for id in vehicle_ids(&root) {
        let car = load_car(&root, &id).unwrap();
        let spec = if is_trailer(&root, &id) {
            None
        } else {
            let spec = PanelSpec::load(&panel_path(&root, &id)).unwrap();
            spec.validate(&car.lattice).unwrap();
            Some(spec)
        };
        let on_disk = load_mesh(&mesh_path(&root, &id)).unwrap();
        let errs = validate(&car, &on_disk, spec.as_ref());
        assert!(errs.is_empty(), "{id}: {errs:#?}");
        let style = load_style(&root, &id).unwrap();
        let fresh = build_mesh(&car, &style);
        assert!(
            fresh == on_disk,
            "{id}: data/panels/meshes is stale; run `cargo run -p car-assets --bin gen-car-mesh`"
        );
        let max = CarMesh::budget_max(car.def.body_size_m[1]);
        assert!((TRI_BUDGET.0..=max).contains(&fresh.tri_count));
        built.push((car, style, fresh));
    }
    let entries: Vec<Entry> = built
        .iter()
        .map(|(car, style, mesh)| Entry { car, style, mesh })
        .collect();
    let on_disk = std::fs::read_to_string(root.join(RENDER_LIST_PATH)).unwrap_or_default();
    assert!(
        render(&entries) == on_disk,
        "{RENDER_LIST_PATH} is stale; run `cargo run -p car-assets --bin gen-car-mesh`"
    );
}

#[test]
fn generation_is_deterministic() {
    let root = repo_root();
    let car = load_car(&root, "hauler_1500").unwrap();
    let style = load_style(&root, "hauler_1500").unwrap();
    assert_eq!(
        build_mesh(&car, &style).to_json(),
        build_mesh(&car, &style).to_json()
    );
}

#[test]
fn skinned_vertices_follow_their_region_nodes() {
    let root = repo_root();
    let car = load_car(&root, "debug_sedan").unwrap();
    let mesh = build_mesh(&car, &load_style(&root, "debug_sedan").unwrap());
    // Push every hood node down 5 cm: every hood vertex moves 5 cm, no fender vertex moves.
    let mut nodes: Vec<[f32; 3]> = car.lattice.nodes.iter().map(|n| n.pos_m).collect();
    for n in &car.lattice.nodes {
        if n.region == BodyRegion::Hood {
            nodes[n.id as usize][2] -= 0.05;
        }
    }
    for part in &mesh.parts {
        if part.region != Some(BodyRegion::Hood) && part.region != Some(BodyRegion::FrontLeftFender)
        {
            continue;
        }
        for v in 0..part.vertex_count() {
            let mut dz = 0.0;
            for k in 0..4 {
                let i = part.skin_nodes[v * 4 + k] as usize;
                let w = part.skin_weights[v * 4 + k] as f32 / 255.0;
                dz += w * (nodes[i][2] - car.lattice.nodes[i].pos_m[2]);
            }
            if part.region == Some(BodyRegion::Hood) {
                assert!(
                    (dz + 0.05).abs() < 1e-3,
                    "{}: hood vertex moved {dz}",
                    part.name
                );
            } else {
                assert!(
                    dz.abs() < 1e-6,
                    "{}: fender vertex followed the hood",
                    part.name
                );
            }
        }
    }
}

#[test]
fn hinged_parts_start_at_their_hinge_line() {
    let root = repo_root();
    for id in car_ids(&root) {
        let car = load_car(&root, &id).unwrap();
        let spec = PanelSpec::load(&panel_path(&root, &id)).unwrap();
        let mesh = load_mesh(&mesh_path(&root, &id)).unwrap();
        for p in spec.panels.iter().filter(|p| p.kind.is_hinged()) {
            let hinge_y = car.lattice.nodes[p.hinge_nodes[0] as usize].pos_m[1];
            let part = mesh
                .parts
                .iter()
                .find(|m| m.region == Some(p.region) && m.kind == PartKind::Panel)
                .unwrap();
            let ys: Vec<f32> = (0..part.vertex_count())
                .map(|v| part.vertex(v)[1])
                .collect();
            let (lo, hi) = ys
                .iter()
                .fold((f32::MAX, f32::MIN), |a, &y| (a.0.min(y), a.1.max(y)));
            // Doors and the trunk hinge at their front edge, the hood at its rear edge.
            let edge = if p.region == BodyRegion::Hood { lo } else { hi };
            assert!(
                (edge - hinge_y).abs() < 0.04,
                "{id}/{}: mesh edge y {edge} vs hinge y {hinge_y}",
                p.name
            );
        }
    }
}
