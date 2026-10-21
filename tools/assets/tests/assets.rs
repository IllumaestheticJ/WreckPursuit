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

/// Style knobs at the ends of (and past) their documented ranges never panic the profile or
/// the generator, on every car's lattice.
#[test]
fn style_knob_extremes_never_panic() {
    use car_assets::mesh::generate_styled;
    use car_assets::style::{BodyStyle, Silhouette};
    let root = repo_root();
    let silhouettes = [
        Silhouette::Notchback,
        Silhouette::Fastback,
        Silhouette::Hatch,
        Silhouette::Wedge,
        Silhouette::Suv,
    ];
    for id in car_ids(&root) {
        let car = load_car(&root, &id).unwrap();
        let base = style_for(&car.def);
        let mut variants: Vec<BodyStyle> = Vec::new();
        for &sil in &silhouettes {
            for &shift in &[-0.8f32, -0.6, 0.0, 0.6, 0.8] {
                for &(ws, rg, hood) in &[
                    (Some(30.0f32), Some(15.0f32), Some(0.4f32)),
                    (Some(75.0), Some(85.0), Some(3.5)),
                    (Some(72.0), Some(80.0), None),
                    (None, Some(90.0), Some(1.0)),
                ] {
                    variants.push(BodyStyle {
                        silhouette: sil,
                        cabin_shift_m: shift,
                        windshield_rake_deg: ws,
                        rear_glass_rake_deg: rg,
                        hood_length: hood,
                        roof_len_trim: if shift > 0.0 { 0.15 } else { -0.25 },
                        roof_dz: if shift > 0.0 { -0.12 } else { 0.10 },
                        ..base.body.clone()
                    });
                }
            }
        }
        variants.push(BodyStyle {
            nose_h: Some(0.0),
            belt_dz: 0.3,
            ride_dz: -0.3,
            crease_z: 2.0,
            sill_tuck: 1.0,
            shoulder_round: 1.0,
            tumblehome: Some(1.0),
            glass_inset: 1.0,
            overhang_trim: (-1.0, -1.0),
            tail_round: 2.0,
            ..base.body.clone()
        });
        for (i, body) in variants.into_iter().enumerate() {
            let p = BodyProfile::styled(&car.def, &car.lattice, &body);
            let (r0, r1, _, _) = p.roof;
            assert!(
                r0 < r1 && r1 < p.y_fw,
                "{id} variant {i}: roof {r0}..{r1}, fw {}",
                p.y_fw
            );
            for y in [p.y_tail, p.y_deck, r0, r1, p.y_fw, p.y_bf, p.y_nose] {
                let _ = p.ring(y);
            }
        }
        // Full generation at the two cabin-shift ends (the slow part: once per car).
        for shift in [-0.6f32, 0.6] {
            let mut st = base.clone();
            st.body.cabin_shift_m = shift;
            st.body.rear_glass_rake_deg = Some(85.0);
            st.body.hood_length = Some(0.5);
            let m = generate_styled(&car.def, &car.lattice, &st);
            assert!(m.tri_count > 0, "{id}");
        }
    }
}
