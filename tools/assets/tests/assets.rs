//! Every car has an up-to-date, valid mesh and panel spec; meshes deform with the lattice and
//! hinged parts line up with their hinge nodes.

use car_assets::mesh::{PartKind, TRI_BUDGET};
use car_assets::profile::BodyProfile;
use car_assets::style::style_for;
use car_assets::{
    build_mesh, car_ids, load_car, load_mesh, mesh_path, panel_path, repo_root, validate::validate,
};
use wp_contracts::BodyRegion;
use wp_damage::PanelSpec;

#[test]
fn every_car_has_a_valid_up_to_date_mesh_and_panel_spec() {
    let root = repo_root();
    let ids = car_ids(&root);
    assert!(ids.len() >= 16);
    for id in ids {
        let car = load_car(&root, &id).unwrap();
        let spec = PanelSpec::load(&panel_path(&root, &id)).unwrap();
        spec.validate(&car.lattice).unwrap();
        let on_disk = load_mesh(&mesh_path(&root, &id)).unwrap();
        let errs = validate(&car, &on_disk, Some(&spec));
        assert!(errs.is_empty(), "{id}: {errs:#?}");
        let fresh = build_mesh(&car);
        assert!(
            fresh == on_disk,
            "{id}: data/panels/meshes is stale; run `cargo run -p car-assets --bin gen-car-mesh`"
        );
        assert!((TRI_BUDGET.0..=TRI_BUDGET.1).contains(&fresh.tri_count));
    }
}

#[test]
fn generation_is_deterministic() {
    let root = repo_root();
    let car = load_car(&root, "hauler_1500").unwrap();
    assert_eq!(build_mesh(&car).to_json(), build_mesh(&car).to_json());
}

#[test]
fn skinned_vertices_follow_their_region_nodes() {
    let root = repo_root();
    let car = load_car(&root, "debug_sedan").unwrap();
    let mesh = build_mesh(&car);
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

/// Hinged panel meshes start at their hinge line. Doors use the lattice hinge. The hood's
/// rear edge and the trunk lid's front edge follow the car's *visual* cabin placement
/// (`BodyStyle::cabin_shift_m` / `hood_length` move the windshield base and the rear-glass
/// base on purpose, see PLAN.md "Foundation API"), so they are checked against the styled
/// profile, and the visual edge may sit at most 0.6 m from the lattice hinge.
#[test]
fn hinged_parts_start_at_their_hinge_line() {
    let root = repo_root();
    for id in car_ids(&root) {
        let car = load_car(&root, &id).unwrap();
        let spec = PanelSpec::load(&panel_path(&root, &id)).unwrap();
        let mesh = load_mesh(&mesh_path(&root, &id)).unwrap();
        let style = style_for(&car.def);
        let prof = BodyProfile::styled(&car.def, &car.lattice, &style.body);
        for p in spec.panels.iter().filter(|p| p.kind.is_hinged()) {
            let lattice_hinge_y = car.lattice.nodes[p.hinge_nodes[0] as usize].pos_m[1];
            let hinge_y = match p.region {
                BodyRegion::Hood => lattice_hinge_y + (prof.y_fw - prof.lattice_y_fw),
                BodyRegion::Trunk => lattice_hinge_y + (prof.y_deck - prof.lattice_y_deck),
                _ => lattice_hinge_y,
            };
            assert!(
                (hinge_y - lattice_hinge_y).abs() <= 0.6,
                "{id}/{}: visual hinge {hinge_y} too far from the lattice hinge {lattice_hinge_y}",
                p.name
            );
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
