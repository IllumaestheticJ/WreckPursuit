//! Every car has an up-to-date, valid mesh and panel spec; meshes deform with the lattice and
//! hinged parts line up with their hinge nodes.

#[path = "common/crash.rs"]
mod crash;

use car_assets::crumple::{measure, render_detail, PartCrumple, Pose};
use car_assets::mesh::{
    PartKind, FOLD_MAX_M, INDICATOR_PREFIX, LID_POP_M, MESH_FORMAT, MID_TRI_BUDGET, TRI_BUDGET,
};
use car_assets::profile::BodyProfile;
use car_assets::skin::{deform_detail, FoldMemory};
use car_assets::style::style_for;
use car_assets::{
    build_mesh, build_mid_mesh, car_ids, load_car, load_mesh, mesh_path, mid_mesh_path, panel_path,
    repo_root, validate::validate,
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

/// Every car has an up-to-date mid LOD (`data/panels/meshes/mid/<car>.json`) within
/// [`MID_TRI_BUDGET`] that paints exactly like its full mesh (same atlas, livery hint, tints,
/// format) and keeps the body panels, glass, lights and wheels.
#[test]
fn every_car_has_an_up_to_date_mid_lod() {
    let root = repo_root();
    for id in car_ids(&root) {
        let car = load_car(&root, &id).unwrap();
        let full = load_mesh(&mesh_path(&root, &id)).unwrap();
        let on_disk = load_mesh(&mid_mesh_path(&root, &id)).unwrap();
        let fresh = build_mid_mesh(&car);
        assert!(
            fresh == on_disk,
            "{id}: data/panels/meshes/mid is stale; run `cargo run -p car-assets --bin gen-car-mesh`"
        );
        let m = &fresh;
        assert_eq!(m.id, id);
        assert_eq!(m.format, MESH_FORMAT, "{id}");
        assert_eq!(m.lattice_id, full.lattice_id, "{id}");
        assert_eq!(m.atlas, full.atlas, "{id}: mid atlas differs");
        assert_eq!(m.livery, full.livery, "{id}: mid livery differs");
        assert_eq!(m.wheel_camber_deg, full.wheel_camber_deg, "{id}");
        for (slot, rgb) in &m.tints {
            assert_eq!(full.tints.get(slot), Some(rgb), "{id}: mid tint {slot:?}");
        }
        let tris: u32 = m.parts.iter().map(|p| p.tri_count() as u32).sum();
        assert_eq!(tris, m.tri_count, "{id}");
        assert!(
            (MID_TRI_BUDGET.0..=MID_TRI_BUDGET.1).contains(&m.tri_count),
            "{id}: mid LOD {} tris outside {MID_TRI_BUDGET:?}",
            m.tri_count
        );
        // Mid ≤ full / 2 + 550: the small traffic vans sit near the bound (satchel_20: 4693 of
        // 8362 after the turn-signal lenses, which every LOD keeps, were cut into the lamps).
        assert!(
            m.tri_count * 2 <= full.tri_count + 1100,
            "{id}: mid LOD barely smaller"
        );
        // Every exterior panel, glass and light of the full mesh is in the mid LOD, and every
        // mid part comes from the full mesh with the same kind and material.
        for p in &full.parts {
            let outside = matches!(
                p.kind,
                PartKind::Panel | PartKind::Glass | PartKind::Light | PartKind::Tire
            );
            if outside {
                assert!(
                    m.parts.iter().any(|q| q.name == p.name),
                    "{id}: mid LOD lost {}",
                    p.name
                );
            }
        }
        for q in &m.parts {
            let p = full.parts.iter().find(|p| p.name == q.name);
            assert!(
                p.is_some_and(|p| p.kind == q.kind
                    && p.material == q.material
                    && p.wheel == q.wheel),
                "{id}: mid part {} not in the full mesh",
                q.name
            );
            assert!(
                q.skin_nodes.is_empty() && q.fold.is_none(),
                "{id}: mid LOD is not skinned"
            );
            assert_eq!(q.uvs.len() * 3, q.positions_mm.len() * 2, "{id}/{}", q.name);
        }
    }
}

/// Format 4 = detailed-car-models' format 2 + round 12's format 3 (see `MESH_FORMAT`); every
/// mesh on disk is format 4 and the panels that buckle carry a fold binding.
#[test]
fn meshes_are_format_4_with_fold_bindings() {
    assert_eq!(MESH_FORMAT, 4);
    let root = repo_root();
    for id in car_ids(&root) {
        let mesh = load_mesh(&mesh_path(&root, &id)).unwrap();
        assert_eq!(mesh.format, 4, "{id}");
        assert!(
            mesh.atlas.is_some(),
            "{id}: format-4 meshes carry the livery atlas"
        );
        let hood = mesh.parts.iter().find(|p| p.name == "hood");
        assert!(
            hood.is_some_and(|h| h.fold.is_some()),
            "{id}: hood without a fold binding"
        );
        for name in ["fender_fl", "fender_fr"] {
            let part = mesh.parts.iter().find(|p| p.name == name);
            assert!(
                part.is_some_and(|p| p.fold.is_some()),
                "{id}/{name}: no fold binding"
            );
        }
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
    use car_assets::style::{BodyStyle, SideSculpt, Silhouette};
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
        for (arc, rake, valley) in [(5.0f32, 90.0f32, 1.0f32), (-1.0, -10.0, -1.0)] {
            for &sil in &silhouettes {
                variants.push(BodyStyle {
                    silhouette: sil,
                    roof_arc: arc,
                    c_pillar_rake_deg: rake,
                    a_pillar_rake_deg: rake,
                    hood_valley: valley,
                    deck_valley: valley,
                    ..base.body.clone()
                });
            }
        }
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
        // Greenhouse framing and trim knobs (car-shapes) at both extremes: full generation.
        for k in [1.0f32, -1.0] {
            let mut st = base.clone();
            let b = &mut st.body;
            b.a_pillar_w_m = k;
            b.c_pillar_w_m = k;
            b.b_pillar_w_m = k;
            b.a_sail_len_m = k;
            b.roof_rail_drop_m = k;
            b.glass_recess_m = k;
            b.glass_frame_m = k;
            b.windshield_wrap_m = 5.0 * k;
            b.rear_glass_wrap_m = 5.0 * k;
            b.cowl_len_m = k;
            b.wipers = if k > 0.0 { 9 } else { 0 };
            b.seam_w_m = k;
            b.arch_lip_m = k;
            b.end_round_m = k;
            let m = generate_styled(&car.def, &car.lattice, &st);
            assert!(m.tri_count > 0, "{id}");
        }
        // Phase 3 knobs far past both ends of their ranges: full generation.
        for k in [1.0f32, -1.0] {
            let mut st = base.clone();
            let b = &mut st.body;
            b.plan_width = vec![
                [-9.0, 9.0 * k],
                [0.0, -9.0 * k],
                [0.3, f32::NAN],
                [9.0, 0.0],
            ];
            b.side_sculpt = vec![SideSculpt {
                y0_m: 9.0 * k,
                y1_m: -9.0 * k,
                z0_m: -1.0,
                z1_m: 5.0,
                dx_m: 2.0 * k,
                falloff_m: 5.0 * k,
            }];
            b.side_bulge_m = k;
            b.side_bulge_z = 3.0 * k;
            b.rocker_in_m = k;
            b.tumblehome_m = Some(2.0 * k);
            b.front_fender_rise_m = k;
            b.rear_fender_rise_m = k;
            b.fender_peak_w = 3.0 * k;
            b.overhang_trim = (3.0 * k, 3.0 * k);
            b.nose_face_rake_deg = 90.0 * k;
            b.tail_face_rake_deg = -90.0 * k;
            b.nose_chin_m = k;
            b.tail_chin_m = k;
            b.nose_plan_bow_m = 2.0 * k;
            b.tail_plan_bow_m = 2.0 * k;
            b.nose_tip_w = 3.0 * k;
            b.tail_tip_w = -3.0 * k;
            b.roof_plan_taper = k;
            b.lower_lean_in_m = k;
            let m = generate_styled(&car.def, &car.lattice, &st);
            assert!(m.tri_count > 0, "{id}");
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

/// Hood / fender motion of `car` after a frontal hit, as the browser draws it.
fn crumple_after(
    car: &str,
    kmh: f32,
    target: &str,
    mode: &str,
    frame_ticks: u64,
) -> (PartCrumple, PartCrumple, PartCrumple) {
    let root = repo_root();
    let def = load_car(&root, car).unwrap();
    let mesh = load_mesh(&mesh_path(&root, car)).unwrap();
    let mut c = crash::frontal(
        car,
        &def.lattice,
        &mesh,
        kmh,
        target,
        mode,
        2.0,
        frame_ticks,
    );
    let pose = Pose {
        lattice: &def.lattice,
        rest: &c.rest,
        nodes: &c.now,
        states: &c.states,
    };
    let mut m = |name: &str| {
        let i = mesh.parts.iter().position(|p| p.name == name).unwrap();
        let (moved, detail) = render_detail(&mesh.parts[i], &pose, Some(&mut c.memory[i]));
        measure(&mesh.parts[i], &moved, &detail, &pose)
    };
    let out = (m("hood"), m("fender_fl"), m("fender_fr"));
    eprintln!(
        "{car} {kmh} km/h {target} {mode} @{frame_ticks} ticks: hood {:?}",
        out.0
    );
    out
}

/// Lead feedback round 12b ("the hood doesn't crumple enough"): in a 50 km/h frontal the hood
/// buckles into a visible fold with at least one fold line, its leading edge sits on the
/// crushed front (no stiff panel floating over a crushed nose), and the fenders buckle too.
/// Renderer frames every 8 ticks (30 fps), the harder case for the crumple memory.
#[test]
fn front_crumples_and_follows_the_crushed_front() {
    for (car, kmh, target, mode, min_fold) in [
        ("debug_sedan", 50.0, "car", "Simulation", 0.12),
        ("debug_sedan", 50.0, "barrier", "Simulation", 0.15),
        ("shiro_turbo", 50.0, "car", "Simulation", 0.12),
        ("sentinel_cruiser", 50.0, "barrier", "Simulation", 0.15),
        ("debug_sedan", 56.0, "barrier", "Extreme", 0.2),
    ] {
        let (hood, fl, fr) = crumple_after(car, kmh, target, mode, 8);
        let tag = format!("{car} {kmh} km/h {target} {mode}");
        assert!(hood.max_fold_m >= min_fold, "{tag}: hood fold {hood:?}");
        assert!(
            hood.max_fold_m <= FOLD_MAX_M + LID_POP_M + 0.01,
            "{tag}: {hood:?}"
        );
        assert!(
            (1..=6).contains(&hood.fold_lines),
            "{tag}: hood fold lines {hood:?}"
        );
        assert!(hood.lead_gap_m <= 0.03, "{tag}: hood floats {hood:?}");
        for f in [fl, fr] {
            assert!(f.max_fold_m >= 0.04, "{tag}: fender {f:?}");
            assert!(f.lead_gap_m <= 0.03, "{tag}: fender floats {f:?}");
        }
    }
}

/// Rear-ended at 50 km/h: both rear quarters buckle outward with a fold line, their tail edges
/// sit on the crushed rear structure, and the trunk lid (where there is one) folds.
#[test]
fn rear_quarters_crumple_and_follow_the_crushed_tail() {
    let root = repo_root();
    for car in ["debug_sedan", "sentinel_cruiser", "vireo_xs"] {
        let def = load_car(&root, car).unwrap();
        let mesh = load_mesh(&mesh_path(&root, car)).unwrap();
        let mut c = crash::rear_ended(car, &def.lattice, &mesh, 50.0, "Simulation", 8);
        let pose = Pose {
            lattice: &def.lattice,
            rest: &c.rest,
            nodes: &c.now,
            states: &c.states,
        };
        for (i, p) in mesh.parts.iter().enumerate() {
            let quarter = matches!(p.name.as_str(), "quarter_rl" | "quarter_rr");
            // Hatch tailgates have every node on the hinge line, so no trunk binding there.
            if !quarter && (p.name != "trunk" || p.fold.is_none()) {
                continue;
            }
            assert!(p.fold.is_some(), "{car}/{}: no crumple binding", p.name);
            let (moved, detail) = render_detail(p, &pose, Some(&mut c.memory[i]));
            let q = measure(p, &moved, &detail, &pose);
            eprintln!("{car}/{}: {q:?}", p.name);
            let min_fold = if quarter { 0.05 } else { 0.08 };
            assert!(q.max_fold_m >= min_fold, "{car}/{}: {q:?}", p.name);
            assert!(q.fold_lines >= 1, "{car}/{}: {q:?}", p.name);
            // Hatch quarters (vireo_xs) reach 23 cm past the lattice's last node row; their tail
            // edge is averaged against lead nodes at other heights, so allow 5 cm there.
            assert!(q.lead_gap_m <= 0.05, "{car}/{}: floats {q:?}", p.name);
        }
    }
}

/// The crumple must not depend on the browser's frame rate: a bumper that crushes and lets go
/// between two frames still counts (its hanging pose is rotated back to the crushed one).
#[test]
fn crumple_does_not_depend_on_frame_rate() {
    let (fast, _, _) = crumple_after("debug_sedan", 50.0, "car", "Simulation", 4);
    let (slow, _, _) = crumple_after("debug_sedan", 50.0, "car", "Simulation", 16);
    assert!(
        (fast.max_fold_m - slow.max_fold_m).abs() <= 0.03,
        "{fast:?} vs {slow:?}"
    );
}

/// Harder hits crumple more (up to the cap).
#[test]
fn harder_hits_crumple_more() {
    let (soft, _, _) = crumple_after("debug_sedan", 30.0, "car", "Simulation", 4);
    let (hard, _, _) = crumple_after("debug_sedan", 70.0, "car", "Simulation", 4);
    assert!(soft.max_fold_m > 0.02, "{soft:?}");
    assert!(
        hard.max_fold_m > soft.max_fold_m + 0.03,
        "{soft:?} vs {hard:?}"
    );
}

#[test]
fn a_parked_car_has_a_flat_front() {
    // No crush, no fold: rest nodes give the rest mesh (with or without crumple memory).
    let root = repo_root();
    let car = load_car(&root, "debug_sedan").unwrap();
    let mesh = build_mesh(&car);
    let rest: Vec<[f32; 3]> = car.lattice.nodes.iter().map(|n| n.pos_m).collect();
    for part in mesh.parts.iter().filter(|p| p.fold.is_some()) {
        let mut memory = FoldMemory::new();
        let (moved, detail) = deform_detail(
            part,
            &rest,
            &rest,
            &rest,
            false,
            &|n| Some(rest[n as usize]),
            Some(&mut memory),
        );
        for (v, m) in moved.iter().enumerate() {
            assert_eq!(*m, part.vertex(v), "{}", part.name);
        }
        assert!(detail.iter().all(|d| d[0] == 0.0 && d[1] == 0.0));
    }
}

/// Bumper nodes of `car` pushed back by `dy` (m).
fn crushed_bumper(car: &wp_contracts::LoadedCar, dy: f32) -> Vec<[f32; 3]> {
    car.lattice
        .nodes
        .iter()
        .map(|n| {
            let mut p = n.pos_m;
            if n.region == BodyRegion::FrontBumper {
                p[1] -= dy;
            }
            p
        })
        .collect()
}

#[test]
fn the_hood_nose_follows_a_crushed_bumper_and_the_memory_holds_until_repair() {
    let root = repo_root();
    let car = load_car(&root, "debug_sedan").unwrap();
    let mesh = build_mesh(&car);
    let hood = mesh.parts.iter().find(|p| p.name == "hood").unwrap();
    let rest: Vec<[f32; 3]> = car.lattice.nodes.iter().map(|n| n.pos_m).collect();
    let crushed = crushed_bumper(&car, 0.2);
    let mut memory = FoldMemory::new();
    let run = |nodes: &[[f32; 3]], memory: &mut FoldMemory| {
        let pose = Pose {
            lattice: &car.lattice,
            rest: &rest,
            nodes,
            states: &[],
        };
        let (moved, detail) = render_detail(hood, &pose, Some(memory));
        measure(hood, &moved, &detail, &pose)
    };
    let hit = run(&crushed, &mut memory);
    // The hood's own nodes did not move; it still folds and its nose comes back with the bumper.
    assert!(hit.max_fold_m > 0.1, "{hit:?}");
    assert!(hit.max_pull_m > 0.1, "{hit:?}");
    // Bumper springs halfway back (or is replaced by a less crushed pose): the hood stays bent.
    let half = crushed_bumper(&car, 0.1);
    let held = run(&half, &mut memory);
    assert!((held.max_fold_m - hit.max_fold_m).abs() < 1e-4, "{held:?}");
    // Repaired: every node at rest clears the memory.
    let fixed = run(&rest, &mut memory);
    assert_eq!(fixed.max_fold_m, 0.0);
    assert_eq!(fixed.max_disp_m, 0.0);
}

#[test]
fn a_hanging_bumper_still_counts_as_crushed() {
    // Crush the bumper 20 cm, then let it hang from its left end, swung 0.6 rad: the hood folds
    // exactly as with the bumper still attached.
    let root = repo_root();
    let car = load_car(&root, "debug_sedan").unwrap();
    let mesh = build_mesh(&car);
    let hood = mesh.parts.iter().find(|p| p.name == "hood").unwrap();
    let rest: Vec<[f32; 3]> = car.lattice.nodes.iter().map(|n| n.pos_m).collect();
    let crushed = crushed_bumper(&car, 0.2);
    let pivot = car
        .lattice
        .nodes
        .iter()
        .filter(|n| n.region == BodyRegion::FrontBumper)
        .min_by(|a, b| a.pos_m[0].total_cmp(&b.pos_m[0]))
        .map(|n| crushed[n.id as usize])
        .unwrap();
    let (axis, angle) = ([0.0, 1.0, 0.0], 0.6f32);
    let (s, c) = (angle.sin(), angle.cos());
    let hanging: Vec<[f32; 3]> = car
        .lattice
        .nodes
        .iter()
        .map(|n| {
            let p = crushed[n.id as usize];
            if n.region != BodyRegion::FrontBumper {
                return p;
            }
            // Rotate about +Y through the pivot (x, z plane).
            let (x, z) = (p[0] - pivot[0], p[2] - pivot[2]);
            [pivot[0] + x * c + z * s, p[1], pivot[2] - x * s + z * c]
        })
        .collect();
    let status = wp_damage::PanelStatus {
        region: BodyRegion::FrontBumper,
        kind: wp_damage::PanelKind::Bumper,
        state: wp_damage::PanelState::HangingByOne,
        angle_rad: angle,
        omega_rad_s: 0.0,
        pivot,
        axis,
        hinge_load_ns: 0.0,
    };
    let fold = |nodes: &[[f32; 3]], states: &[wp_damage::PanelStatus]| {
        let pose = Pose {
            lattice: &car.lattice,
            rest: &rest,
            nodes,
            states,
        };
        let (moved, detail) = render_detail(hood, &pose, None);
        measure(hood, &moved, &detail, &pose)
    };
    let attached = fold(&crushed, &[]);
    let hung = fold(&hanging, &[status]);
    assert!(attached.max_fold_m > 0.1);
    assert!(
        (attached.max_fold_m - hung.max_fold_m).abs() < 2e-3,
        "{attached:?} vs {hung:?}"
    );
    // Gone (detached): only the frame and fenders count, which did not move.
    let gone = wp_damage::PanelStatus {
        state: wp_damage::PanelState::Detached,
        ..status
    };
    assert!(fold(&hanging, &[gone]).max_fold_m < 1e-4);
}

#[test]
fn an_unlatched_hood_pops_at_the_latch() {
    let root = repo_root();
    let car = load_car(&root, "debug_sedan").unwrap();
    let mesh = build_mesh(&car);
    let hood = mesh.parts.iter().find(|p| p.name == "hood").unwrap();
    let rest: Vec<[f32; 3]> = car.lattice.nodes.iter().map(|n| n.pos_m).collect();
    let moved = car_assets::skin::deform(hood, &rest, &rest, true);
    let ys: Vec<f32> = (0..hood.vertex_count())
        .map(|v| hood.vertex(v)[1])
        .collect();
    let (lo, hi) = ys
        .iter()
        .fold((f32::MAX, f32::MIN), |a, &y| (a.0.min(y), a.1.max(y)));
    for (v, m) in moved.iter().enumerate() {
        let rise = m[2] - hood.vertex(v)[2];
        if ys[v] > hi - 0.01 {
            assert!((rise - LID_POP_M).abs() < 0.01, "latch edge rose {rise}");
        }
        if ys[v] < lo + 0.01 {
            assert!(rise.abs() < 1e-4, "hinge edge rose {rise}");
        }
    }
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// First mesh part a ray from `o` along `d` hits (body parts only), or None.
fn first_hit(mesh: &car_assets::mesh::CarMesh, o: [f32; 3], d: [f32; 3]) -> Option<String> {
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let mut best: Option<(f32, String)> = None;
    for p in mesh.parts.iter().filter(|p| p.wheel.is_none()) {
        for t in p.indices.chunks(3) {
            let (a, b, c) = (
                p.vertex(t[0] as usize),
                p.vertex(t[1] as usize),
                p.vertex(t[2] as usize),
            );
            let (e1, e2) = (sub(b, a), sub(c, a));
            let h = cross(d, e2);
            let det = dot(e1, h);
            if det.abs() < 1e-9 {
                continue;
            }
            let s = sub(o, a);
            let u = dot(s, h) / det;
            let q = cross(s, e1);
            let v = dot(d, q) / det;
            let dist = dot(e2, q) / det;
            if u < 0.0 || v < 0.0 || u + v > 1.0 || dist <= 0.0 {
                continue;
            }
            if best.as_ref().is_none_or(|b| dist < b.0) {
                best = Some((dist, p.name.clone()));
            }
        }
    }
    best.map(|b| b.1)
}

#[test]
fn lights_sit_outside_the_shell() {
    let root = repo_root();
    for id in car_ids(&root) {
        let mesh = load_mesh(&mesh_path(&root, &id)).unwrap();
        check_lights(&id, &mesh);
    }
}

/// Head / tail lamps and their turn-signal segments of `mesh` are outside the body shell,
/// seen from ahead / behind, in the right materials.
fn check_lights(id: &str, mesh: &car_assets::mesh::CarMesh) {
    {
        // Head / tail lamps and the turn-signal segments cut out of them (style kits add fog
        // lamps, DRLs, markers etc. in other regions).
        let lights: Vec<_> = mesh
            .parts
            .iter()
            .filter(|p| car_assets::skin::is_end_lamp(p))
            .collect();
        let is_signal = |p: &car_assets::mesh::MeshPart| p.name.starts_with(INDICATOR_PREFIX);
        assert_eq!(
            lights.iter().filter(|p| !is_signal(p)).count(),
            4,
            "{id}: two head- and two taillights"
        );
        assert_eq!(
            lights.iter().filter(|p| is_signal(p)).count(),
            4,
            "{id}: front and rear turn signals in the lamp clusters"
        );
        for l in lights {
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for v in 0..l.vertex_count() {
                let q = l.vertex(v);
                for k in 0..3 {
                    lo[k] = lo[k].min(q[k]);
                    hi[k] = hi[k].max(q[k]);
                }
            }
            let front = hi[1] > 0.0;
            let dir = if front {
                [0.0, -1.0, 0.0]
            } else {
                [0.0, 1.0, 0.0]
            };
            let y = if front { hi[1] + 3.0 } else { lo[1] - 3.0 };
            // Style kits shape the lenses (round / quad / strip lamps with bezels and rings), so
            // rays go through the centroids of the lens's outward-facing triangles: the body
            // shell (panel / body parts) covers at most a tenth of them (a lamp corner swept
            // round the bumper wrap, tansy_13), and at least half are seen directly.
            let (mut faces, mut seen) = (0, 0);
            let mut hidden = Vec::new();
            for t in l.indices.chunks(3) {
                let (a, b, c) = (
                    l.vertex(t[0] as usize),
                    l.vertex(t[1] as usize),
                    l.vertex(t[2] as usize),
                );
                let n = cross3(sub3(b, a), sub3(c, a));
                let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                if len < 1e-9 || n[1] / len * -dir[1] < 0.7 {
                    continue;
                }
                faces += 1;
                let cx = (a[0] + b[0] + c[0]) / 3.0;
                let cz = (a[2] + b[2] + c[2]) / 3.0;
                let hit = first_hit(mesh, [cx, y, cz], dir);
                if hit.as_deref() == Some(l.name.as_str()) {
                    seen += 1;
                }
                let shell = hit
                    .as_deref()
                    .and_then(|h| mesh.parts.iter().find(|p| p.name == h));
                if shell.is_some_and(|p| matches!(p.kind, PartKind::Panel | PartKind::Body)) {
                    hidden.push(hit);
                }
            }
            assert!(faces > 0, "{id}/{}: no outward lens face", l.name);
            assert!(
                hidden.len() * 10 <= faces,
                "{id}/{}: {} of {faces} lens faces under the body shell: {hidden:?}",
                l.name,
                hidden.len()
            );
            assert!(
                seen * 2 >= faces,
                "{id}/{}: only {seen} of {faces} lens faces visible",
                l.name
            );
            // Proud of the body, facing out.
            use car_assets::mesh::MaterialSlot as M;
            if is_signal(l) {
                assert!(
                    l.material == M::EmissiveAmber || (!front && l.material == M::Taillight),
                    "{id}/{}: amber (or red-on-red at the rear) turn signal",
                    l.name
                );
                continue;
            }
            assert!(l.material == M::Headlight || l.material == M::Taillight);
            assert_eq!(
                l.material == M::Headlight,
                front,
                "{id}/{}: headlights at the front, taillights at the back",
                l.name
            );
        }
    }
}

#[test]
fn lights_move_with_the_front_and_rear_structure() {
    let root = repo_root();
    for id in ["debug_sedan", "hauler_1500", "lucciola_350", "vireo_xs"] {
        let car = load_car(&root, id).unwrap();
        let mesh = load_mesh(&mesh_path(&root, id)).unwrap();
        let rest: Vec<[f32; 3]> = car.lattice.nodes.iter().map(|n| n.pos_m).collect();
        for l in mesh
            .parts
            .iter()
            .filter(|p| car_assets::skin::is_end_lamp(p))
        {
            let regions = car_assets::skin::light_regions(car_assets::skin::part_centre(l));
            // Bound only to its end of the car, and to the bumper (the first thing a hit crushes).
            assert!(l
                .skin_nodes
                .iter()
                .all(|&i| regions.contains(&car.lattice.nodes[i as usize].region)));
            assert!(
                l.skin_nodes
                    .iter()
                    .any(|&i| car.lattice.nodes[i as usize].region == regions[0]),
                "{id}/{}: not bound to the bumper",
                l.name
            );
            // Crush the whole end 20 cm inward: every lens vertex moves 20 cm with it.
            let push = if regions[0] == BodyRegion::FrontBumper {
                -0.2
            } else {
                0.2
            };
            let mut now = rest.clone();
            for n in &car.lattice.nodes {
                if regions.contains(&n.region) {
                    now[n.id as usize][1] += push;
                }
            }
            let moved = car_assets::skin::deform(l, &rest, &now, false);
            for (v, m) in moved.iter().enumerate() {
                let dy = m[1] - l.vertex(v)[1];
                assert!((dy - push).abs() < 2e-3, "{id}/{}: lens moved {dy}", l.name);
            }
            // The fender alone (other side of the car) does not drag the lens.
            let mut far = rest.clone();
            let other = match regions[2] {
                BodyRegion::FrontLeftFender => BodyRegion::FrontRightFender,
                BodyRegion::FrontRightFender => BodyRegion::FrontLeftFender,
                BodyRegion::RearLeftQuarter => BodyRegion::RearRightQuarter,
                _ => BodyRegion::RearLeftQuarter,
            };
            for n in &car.lattice.nodes {
                if n.region == other {
                    far[n.id as usize][2] += 0.2;
                }
            }
            let still = car_assets::skin::deform(l, &rest, &far, false);
            assert!(still
                .iter()
                .enumerate()
                .all(|(v, m)| (m[2] - l.vertex(v)[2]).abs() < 1e-6));
        }
    }
}

/// Full-mesh triangle counts the turn-signal lamps may add at most 2 % to (round 12, playtest
/// row 136: the signals were rebuilt into the lamp clusters). Rebased on 2026-10-27 to the
/// counts after the car-shapes generator pass (framed glass, pillars, common trim), so it keeps
/// guarding foundation growth; a deliberate per-car detail pass updates its row.
const TRIS_BEFORE_INTEGRATED_SIGNALS: &[(&str, u32)] = &[
    ("arashi_t4", 18292),
    ("brigand_400", 20986),
    ("bulwark_400", 20579),
    ("debug_coupe", 17927),
    ("debug_sedan", 9605),
    ("drayman_cargo", 10488),
    ("fulmine_v10", 19884),
    ("furrow_24", 9414),
    ("hakone_gt", 18873),
    ("hauler_1500", 10625),
    ("ironside_440", 19809),
    ("junkyard_beater", 8836),
    ("kestrel_ts", 18914),
    ("lindwurm_4t", 17474),
    ("lucciola_350", 18170),
    ("marten_20", 17056),
    ("rampart_50", 19954),
    ("satchel_20", 9221),
    ("sentinel_bastion", 21419),
    ("sentinel_cruiser", 21893),
    ("sentinel_vigil", 21343),
    ("sentinel_warden", 21251),
    ("sentinel_wolfhound", 21646),
    ("shiro_turbo", 18992),
    ("sperber_32", 19112),
    ("stoat_16v", 16894),
    ("talus_20t", 17376),
    ("tansy_13", 8214),
    ("tumbril_20", 9418),
    ("vireo_xs", 17065),
];

/// Bounding box (lo, hi) of a part, m.
fn part_box(p: &car_assets::mesh::MeshPart) -> ([f32; 3], [f32; 3]) {
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for v in 0..p.vertex_count() {
        let q = p.vertex(v);
        for k in 0..3 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
        }
    }
    (lo, hi)
}

/// Check the turn-signal contract on one mesh (full or mid LOD): exactly
/// `indicator_front_l/_r` + `indicator_rear_l/_r` (+ optional `indicator_side_l/_r` pair), kind
/// `light`, front amber in `FrameFront`, rear amber or red-on-red in `FrameRear`, repeaters amber
/// on a front fender or front door (mirror), `_l` at x < 0, each a segment of its lamp cluster
/// (within 5 cm of the same side's head / tail lamp).
fn check_signals(label: &str, mesh: &car_assets::mesh::CarMesh) {
    use car_assets::mesh::MaterialSlot as M;
    use BodyRegion as R;
    let signals: Vec<_> = mesh
        .parts
        .iter()
        .filter(|p| p.name.starts_with(INDICATOR_PREFIX))
        .collect();
    let names: Vec<&str> = signals.iter().map(|p| p.name.as_str()).collect();
    for want in [
        "indicator_front_l",
        "indicator_front_r",
        "indicator_rear_l",
        "indicator_rear_r",
    ] {
        assert!(names.contains(&want), "{label}: no {want} in {names:?}");
    }
    assert_eq!(
        names.contains(&"indicator_side_l"),
        names.contains(&"indicator_side_r"),
        "{label}: repeaters come in pairs"
    );
    let get = |n: &str| mesh.parts.iter().find(|p| p.name == n);
    for s in &signals {
        let n = s.name.as_str();
        let left = n.ends_with("_l");
        assert!(left || n.ends_with("_r"), "{label}/{n}: no side");
        assert_eq!(s.kind, PartKind::Light, "{label}/{n}");
        assert!(s.tri_count() > 0, "{label}/{n}: empty");
        let (lo, hi) = part_box(s);
        assert!(
            if left { hi[0] < 0.0 } else { lo[0] > 0.0 },
            "{label}/{n}: on the wrong side ({lo:?}..{hi:?})"
        );
        let side = if left { "l" } else { "r" };
        let lamp = match n
            .strip_prefix(INDICATOR_PREFIX)
            .and_then(|t| t.split('_').next())
        {
            Some("front") => {
                assert_eq!(s.region, Some(R::FrameFront), "{label}/{n}");
                assert_eq!(s.material, M::EmissiveAmber, "{label}/{n}");
                Some(format!("headlight_{side}"))
            }
            Some("rear") => {
                assert_eq!(s.region, Some(R::FrameRear), "{label}/{n}");
                assert!(
                    matches!(s.material, M::EmissiveAmber | M::Taillight),
                    "{label}/{n}: {:?}",
                    s.material
                );
                Some(format!("taillight_{side}"))
            }
            Some("side") => {
                assert_eq!(s.material, M::EmissiveAmber, "{label}/{n}");
                let ok: &[R] = if left {
                    &[R::FrontLeftFender, R::LeftDoorF]
                } else {
                    &[R::FrontRightFender, R::RightDoorF]
                };
                assert!(
                    s.region.is_some_and(|r| ok.contains(&r)),
                    "{label}/{n}: {:?}",
                    s.region
                );
                None
            }
            _ => panic!("{label}: unexpected part {n}"),
        };
        if let Some(lamp) = lamp {
            let l = get(&lamp).unwrap_or_else(|| panic!("{label}: no {lamp}"));
            let (llo, lhi) = part_box(l);
            let gap = (0..3)
                .map(|k| (llo[k] - hi[k]).max(lo[k] - lhi[k]).max(0.0))
                .fold(0.0f32, f32::max);
            assert!(
                gap <= 0.05,
                "{label}/{n}: {gap} m away from {lamp}, not part of its cluster"
            );
        }
    }
    // Left and right lenses match.
    for base in ["indicator_front", "indicator_rear", "indicator_side"] {
        if let (Some(l), Some(r)) = (get(&format!("{base}_l")), get(&format!("{base}_r"))) {
            assert_eq!(l.material, r.material, "{label}/{base}");
        }
    }
}

/// Playtest row 136: every car's turn signals are segments of its head / tail lamp clusters
/// (plus optional flush repeaters), in the full and the mid LOD, bound like the lamps, and
/// the rebuild costs at most 2 % of a car's triangles.
#[test]
fn every_car_has_turn_signals_integrated_into_its_lamps() {
    let root = repo_root();
    let ids = car_ids(&root);
    for id in &ids {
        let car = load_car(&root, id).unwrap();
        let full = load_mesh(&mesh_path(&root, id)).unwrap();
        let mid = load_mesh(&mid_mesh_path(&root, id)).unwrap();
        check_signals(&format!("{id} (full)"), &full);
        check_signals(&format!("{id} (mid)"), &mid);
        // Front / rear signals bind like their lamp: to the nodes of their end of the car.
        for s in full.parts.iter().filter(|p| {
            p.name.starts_with("indicator_front") || p.name.starts_with("indicator_rear")
        }) {
            assert!(car_assets::skin::is_end_lamp(s), "{id}/{}", s.name);
            let regions = car_assets::skin::light_regions(car_assets::skin::part_centre(s));
            assert!(
                !s.skin_nodes.is_empty()
                    && s.skin_nodes
                        .iter()
                        .all(|&i| regions.contains(&car.lattice.nodes[i as usize].region)),
                "{id}/{}: not bound to its end of the car",
                s.name
            );
        }
        if let Some(&(_, before)) = TRIS_BEFORE_INTEGRATED_SIGNALS.iter().find(|(c, _)| c == id) {
            assert!(
                full.tri_count * 100 <= before * 102,
                "{id}: {} tris, more than 2 % over {before}",
                full.tri_count
            );
        }
    }
    // The parity test cars (`tools/parity/build_parity_cars.py`) carry copies of their template
    // car's meshes; they must follow the same contract.
    let mut parity = 0;
    for e in std::fs::read_dir(root.join("data/panels/meshes")).unwrap() {
        let path = e.unwrap().path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if path.extension().and_then(|x| x.to_str()) != Some("json")
            || ids.iter().any(|i| i == stem)
        {
            continue;
        }
        check_signals(&format!("{stem} (full)"), &load_mesh(&path).unwrap());
        let mid = load_mesh(&mid_mesh_path(&root, stem)).unwrap();
        check_signals(&format!("{stem} (mid)"), &mid);
        parity += 1;
    }
    assert!(parity >= 4, "parity meshes checked: {parity}");
}

/// The phase 3 knobs at strong in-range settings (narrow plan, deep sculpts, bulged section,
/// tall fender tops, shortest and longest overhangs) still give valid meshes: every panel on
/// its lattice region, skinned to its own nodes, inside the body size.
#[test]
fn phase3_knobs_keep_panels_on_their_regions() {
    use car_assets::style::SideSculpt;
    let root = repo_root();
    for id in car_ids(&root) {
        let car = load_car(&root, &id).unwrap();
        let spec = PanelSpec::load(&panel_path(&root, &id)).unwrap();
        let base = style_for(&car.def);
        let p0 = BodyProfile::styled(&car.def, &car.lattice, &base.body);
        // (Both ends at +0.25 can push a car that is already long past validate's ±15 % length
        // sanity check: a style call, not a generator limit.)
        for (k, oh) in [(0, (-0.35f32, 0.25f32)), (1, (0.25, -0.35))] {
            let mut st = base.clone();
            let b = &mut st.body;
            b.plan_width = vec![
                [p0.y_tail, 0.80],
                [p0.y_tail + 0.8, 1.0],
                [0.0, 0.90],
                [p0.y_nose - 0.8, 1.05],
                [p0.y_nose, 0.80],
            ];
            b.side_sculpt = vec![
                SideSculpt {
                    y0_m: -0.8,
                    y1_m: 0.8,
                    z0_m: 0.0,
                    z1_m: 0.6,
                    dx_m: -0.08,
                    falloff_m: 0.2,
                },
                SideSculpt {
                    y0_m: p0.y_tail,
                    y1_m: p0.y_nose,
                    z0_m: p0.z_belt - 0.08,
                    z1_m: p0.z_belt - 0.05,
                    dx_m: 0.06,
                    falloff_m: 0.03,
                },
            ];
            b.side_bulge_m = 0.06;
            b.rocker_in_m = 0.08;
            b.tumblehome_m = Some(if k == 0 { 0.03 } else { 0.36 });
            b.front_fender_rise_m = 0.12;
            b.rear_fender_rise_m = 0.12;
            b.overhang_trim = oh;
            b.nose_face_rake_deg = if k == 0 { 50.0 } else { -15.0 };
            b.tail_face_rake_deg = if k == 0 { -15.0 } else { 50.0 };
            b.nose_chin_m = 0.20;
            b.tail_chin_m = 0.20;
            b.nose_plan_bow_m = 0.45;
            b.nose_tip_w = 0.2;
            b.tail_plan_bow_m = 0.45;
            b.tail_tip_w = 0.3;
            b.roof_plan_taper = 0.3;
            b.lower_lean_in_m = 0.08;
            let mut m = car_assets::mesh::generate_styled(&car.def, &car.lattice, &st);
            car_assets::skin::bind_skin(&car.lattice, &mut m);
            // Budgets are the style author's call (these settings add loft stations).
            let errs: Vec<String> = validate(&car, &m, Some(&spec))
                .into_iter()
                .filter(|e| !e.contains("budget"))
                .collect();
            assert!(errs.is_empty(), "{id} (overhang {oh:?}): {errs:#?}");
        }
    }
}

/// The plan-width curve never pulls the body inside a tire (outer face + 1 cm), even at the
/// narrowest scale.
#[test]
fn plan_width_keeps_the_body_outside_the_tires() {
    let root = repo_root();
    for id in car_ids(&root) {
        let car = load_car(&root, &id).unwrap();
        let mut body = style_for(&car.def).body;
        let w0 = BodyProfile::styled(&car.def, &car.lattice, &body);
        body.plan_width = vec![[0.0, 0.80]];
        let p = BodyProfile::styled(&car.def, &car.lattice, &body);
        for (w, &(wy, _, _)) in car.def.wheels.iter().zip(&p.arches) {
            let tire = w.position_m[0].abs() + 0.5 * w.width_m;
            let want = (tire + 0.01).min(w0.half_width(wy));
            let got = p.half_width(wy);
            assert!(
                got >= want - 1e-4,
                "{id}: half width {got} over a wheel, tire at {tire}"
            );
        }
        assert!(p.half_width(0.0) <= 0.81 * w0.half_width(0.0) + 0.05);
    }
}

/// Fender tops raised with `front_fender_rise_m` (instead of sinking the hood with
/// `hood_valley`) leave the car's kit head lamps visible, at the largest rise.
#[test]
fn fender_rise_keeps_kit_lamps_visible() {
    let root = repo_root();
    for id in ["nyx_proto", "morrigan_55", "ironside_440", "kestrel_ts"] {
        let car = load_car(&root, id).unwrap();
        let mut st = style_for(&car.def);
        st.body.hood_valley = 0.0;
        st.body.deck_valley = 0.0;
        st.body.front_fender_rise_m = 0.12;
        st.body.rear_fender_rise_m = 0.12;
        // The peak starts inboard of the kits' fender-top lamps (they sit on its plateau, not
        // across the band's bend).
        st.body.fender_peak_w = 0.65;
        let mut m = car_assets::mesh::generate_styled(&car.def, &car.lattice, &st);
        car_assets::skin::bind_skin(&car.lattice, &mut m);
        check_lights(id, &m);
    }
}

/// Lamps projected onto the surface (`lamp.rs` via `Kit::proj_cluster`): front-view lenses
/// wrapping round the corner, top-view lenses on the nose, wraparound tail lamps, with their
/// turn-signal segments and bezels, sit outside the shell and keep the turn-signal contract.
#[test]
fn projected_lamps_sit_outside_the_shell_and_keep_the_signal_contract() {
    use car_assets::builder::{Builder, PartKey};
    use car_assets::lamp::{ellipse, LampView};
    use car_assets::mesh::MaterialSlot as M;
    use car_assets::style::{Detail, DetailCtx, Shape};
    use car_assets::styles::performance::kit::{Kit, Seg, SIDES};
    fn front_view(ctx: &DetailCtx, _k: &PartKey, b: &mut Builder) {
        let mut kit = Kit::new(ctx.profile, b);
        let (wn, (_, zt)) = (kit.wn(), kit.nose_z());
        let (wt, (_, tt)) = (kit.wt(), kit.tail_z());
        for sx in SIDES {
            // Kept under the nose face's top edge (an outline across a sharp edge splits over
            // two surfaces).
            let head = [
                [-(wn - 0.30), zt - 0.13],
                [-(wn - 0.01), zt - 0.09],
                [-(wn - 0.01), zt - 0.045],
                [-(wn - 0.36), zt - 0.06],
            ];
            let o = Seg::Outboard(0.10);
            kit.proj_cluster(
                sx,
                true,
                LampView::Front,
                &head,
                Some(o),
                M::EmissiveAmber,
                0.012,
                20.0,
            );
            let tail = [
                [-(wt - 0.34), tt - 0.13],
                [-(wt + 0.03), tt - 0.13],
                [-(wt + 0.03), tt - 0.03],
                [-(wt - 0.40), tt - 0.05],
            ];
            let i = Seg::Inboard(0.09);
            kit.proj_cluster(
                sx,
                false,
                LampView::Rear,
                &tail,
                Some(i),
                M::EmissiveAmber,
                0.01,
                35.0,
            );
        }
    }
    fn top_view(ctx: &DetailCtx, _k: &PartKey, b: &mut Builder) {
        let mut kit = Kit::new(ctx.profile, b);
        let (wn, yn) = (kit.wn(), kit.yn);
        let (wt, (_, tt)) = (kit.wt(), kit.tail_z());
        for sx in SIDES {
            let tear = [
                [-(wn - 0.05), yn - 0.03],
                [-(wn - 0.30), yn - 0.07],
                [-(wn - 0.24), yn - 0.26],
                [-(wn - 0.10), yn - 0.42],
                [-(wn - 0.05), yn - 0.30],
            ];
            let l = Seg::Outboard(0.06);
            kit.proj_cluster(
                sx,
                true,
                LampView::Top,
                &tear,
                Some(l),
                M::EmissiveAmber,
                0.01,
                0.0,
            );
            let round = ellipse([-(wt - 0.16), tt - 0.10], [0.085, 0.065], 16);
            let i = Seg::Inboard(0.05);
            kit.proj_cluster(
                sx,
                false,
                LampView::Rear,
                &round,
                Some(i),
                M::Taillight,
                0.012,
                15.0,
            );
        }
    }
    // A concave (L-shaped) head lamp with a projector bowl, a chrome ring and an LED strip.
    fn concave(ctx: &DetailCtx, _k: &PartKey, b: &mut Builder) {
        use car_assets::lamp::{element, Element, Lamp};
        use car_assets::styles::performance::kit::{dkey, headlight_key, sided};
        let mut kit = Kit::new(ctx.profile, b);
        let (wn, (_, zt)) = (kit.wn(), kit.nose_z());
        let (wt, (_, tt)) = (kit.wt(), kit.tail_z());
        for sx in SIDES {
            let l = [
                [-(wn - 0.40), zt - 0.14],
                [-(wn - 0.02), zt - 0.14],
                [-(wn - 0.02), zt - 0.05],
                [-(wn - 0.14), zt - 0.05],
                [-(wn - 0.14), zt - 0.10],
                [-(wn - 0.40), zt - 0.10],
            ];
            let i = Seg::Inboard(0.06);
            kit.proj_cluster(
                sx,
                true,
                LampView::Front,
                &l,
                Some(i),
                M::EmissiveAmber,
                0.01,
                15.0,
            );
            let pts = l.iter().map(|q| [sx * q[0].abs(), q[1]]).collect();
            let lamp = Lamp::new(LampView::Front, pts);
            let hk = headlight_key(sx);
            let c = [sx * (wn - 0.08), zt - 0.10];
            let bowl = Element::Bowl {
                r_m: 0.03,
                h_m: 0.01,
            };
            element(kit.b, ctx.profile, &hk, &lamp, c, &bowl);
            let ring = sided(&dkey("lamp_ring_l", BodyRegion::FrameFront, M::Chrome), sx);
            let rg = Element::Ring {
                r_m: 0.036,
                w_m: 0.006,
            };
            element(kit.b, ctx.profile, &ring, &lamp, c, &rg);
            let strip = Element::Strip {
                pts: vec![[sx * (wn - 0.38), zt - 0.12], [sx * (wn - 0.17), zt - 0.12]],
                w_m: 0.012,
            };
            element(kit.b, ctx.profile, &hk, &lamp, c, &strip);
            let tail = [
                [-(wt - 0.30), tt - 0.12],
                [-(wt - 0.02), tt - 0.12],
                [-(wt - 0.02), tt - 0.03],
                [-(wt - 0.30), tt - 0.03],
            ];
            let o = Seg::Outboard(0.08);
            kit.proj_cluster(
                sx,
                false,
                LampView::Rear,
                &tail,
                Some(o),
                M::EmissiveAmber,
                0.0,
                0.0,
            );
        }
    }
    let root = repo_root();
    for id in [
        "kestrel_ts",
        "vireo_xs",
        "nyx_proto",
        "hauler_1500",
        "sentinel_cruiser",
    ] {
        let car = load_car(&root, id).unwrap();
        for f in [front_view as car_assets::style::DetailFn, top_view, concave] {
            let mut st = style_for(&car.def);
            st.details = vec![Detail::new(
                "lamps",
                BodyRegion::Cabin,
                M::Trim,
                Shape::Custom(f),
            )];
            let mut m = car_assets::mesh::generate_styled(&car.def, &car.lattice, &st);
            car_assets::skin::bind_skin(&car.lattice, &mut m);
            check_lights(id, &m);
            check_signals(id, &m);
            for s in m.parts.iter().filter(|p| p.name.starts_with("indicator_")) {
                assert!(car_assets::skin::is_end_lamp(s), "{id}/{}", s.name);
            }
            let bezels = m
                .parts
                .iter()
                .filter(|p| p.name.contains("lamp_bezel"))
                .count();
            assert!(
                bezels == 4 || bezels == 2,
                "{id}: head (and tail) bezels: {bezels}"
            );
        }
    }
}
