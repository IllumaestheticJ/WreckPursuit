//! Every car has an up-to-date, valid mesh and panel spec; meshes deform with the lattice and
//! hinged parts line up with their hinge nodes.

use car_assets::mesh::{PartKind, TRI_BUDGET};
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

/// Lattice rest nodes, final nodes and final panel states after `car` drives into a
/// full-width rigid barrier at `kmh` (the real sim: Rapier contacts, lattice, panels).
fn frontal_hit(
    car: &str,
    kmh: f32,
    mode: &str,
) -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<wp_damage::PanelStatus>) {
    use wp_contracts::DamageSim;
    let v = kmh / 3.6;
    let ron = format!(
        "(name: \"hood_buckle\", seed: 7, mode: {mode}, assists: ai, panels: true,
          barriers: [(pos: (0.0, 8.0, 0.75), size: (6.0, 1.0, 1.5))],
          entities: [(id: \"car\", car: \"{car}\", speed_mps: {v}, driver: cruise(target_mps: {v}))],
          duration_s: 1.5, hash_every: 240)"
    );
    let sc = simcli::Scenario::from_ron("hood_buckle.ron", &ron).unwrap();
    let data = simcli::FsData {
        root: repo_root().join("data"),
    };
    let mut rest = Vec::new();
    let mut now = Vec::new();
    let mut states = Vec::new();
    let mut hook = |t: &simcli::TickView| -> Result<(), String> {
        let d = &t.sim.vehicles()[0].damage;
        if rest.is_empty() {
            rest = (0..d.lattice().nodes.len())
                .map(|i| d.rest_position(i))
                .collect();
        }
        now = d.node_positions().to_vec();
        states = d.panel_states();
        Ok(())
    };
    simcli::run_with_hook(&sc, &data, &simcli::RunOptions::default(), &mut hook).unwrap();
    (rest, now, states)
}

/// Largest vertex displacement and largest fold (rise above the plain skinned surface) of a
/// part after a hit.
fn hood_motion(
    part: &car_assets::mesh::MeshPart,
    rest: &[[f32; 3]],
    now: &[[f32; 3]],
    unlatched: bool,
) -> (f32, f32) {
    let moved = car_assets::skin::deform(part, rest, now, unlatched);
    let mut flat = part.clone();
    flat.fold_chords.clear();
    let skinned = car_assets::skin::deform(&flat, rest, now, unlatched);
    let mut disp = 0.0f32;
    let mut fold = 0.0f32;
    for v in 0..part.vertex_count() {
        let r = part.vertex(v);
        let m = moved[v];
        let d = ((m[0] - r[0]).powi(2) + (m[1] - r[1]).powi(2) + (m[2] - r[2]).powi(2)).sqrt();
        disp = disp.max(d);
        fold = fold.max(m[2] - skinned[v][2]);
    }
    (disp, fold)
}

#[test]
fn hood_buckles_in_a_56_kmh_frontal() {
    let root = repo_root();
    for (car, kmh, mode, min_fold) in [
        ("debug_sedan", 56.0, "Simulation", 0.10),
        ("sentinel_cruiser", 56.0, "Simulation", 0.08),
        ("debug_sedan", 40.0, "Arcade", 0.04),
    ] {
        let mesh = load_mesh(&mesh_path(&root, car)).unwrap();
        let hood = mesh
            .parts
            .iter()
            .find(|p| p.region == Some(BodyRegion::Hood) && p.kind == PartKind::Panel)
            .unwrap();
        assert!(
            !hood.fold_chords.is_empty(),
            "{car}: hood has no fold binding"
        );
        let (rest, now, states) = frontal_hit(car, kmh, mode);
        let unlatched = states
            .iter()
            .any(|p| p.region == BodyRegion::Hood && p.state != wp_damage::PanelState::Attached);
        let (disp, fold) = hood_motion(hood, &rest, &now, unlatched);
        eprintln!("{car} {kmh} km/h {mode}: hood moved {disp:.3} m, fold {fold:.3} m, unlatched {unlatched}");
        // Hood vertices move ≥ 10 cm and the hood buckles into a visible peak.
        assert!(
            disp >= 0.10,
            "{car} {kmh} km/h: hood vertices moved only {disp} m"
        );
        assert!(
            fold >= min_fold,
            "{car} {kmh} km/h: hood fold only {fold} m"
        );
        assert!(fold <= car_assets::mesh::FOLD_MAX_M * 1.5);
    }
}

#[test]
fn a_parked_car_has_a_flat_hood() {
    // No crush, no fold: rest nodes give the rest mesh.
    let root = repo_root();
    let car = load_car(&root, "debug_sedan").unwrap();
    let mesh = build_mesh(&car);
    let rest: Vec<[f32; 3]> = car.lattice.nodes.iter().map(|n| n.pos_m).collect();
    for part in mesh.parts.iter().filter(|p| !p.fold_chords.is_empty()) {
        let (disp, fold) = hood_motion(part, &rest, &rest, true);
        assert!(disp < 1e-6 && fold.abs() < 1e-6, "{}", part.name);
    }
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
        let lights: Vec<_> = mesh
            .parts
            .iter()
            .filter(|p| p.kind == PartKind::Light)
            .collect();
        assert_eq!(lights.len(), 4, "{id}: two head- and two taillights");
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
            // Rays from outside at the centre and near the four corners of the lens face.
            for (fx, fz) in [(0.5, 0.5), (0.1, 0.1), (0.9, 0.1), (0.1, 0.9), (0.9, 0.9)] {
                let x = lo[0] + fx * (hi[0] - lo[0]);
                let z = lo[2] + fz * (hi[2] - lo[2]);
                let hit = first_hit(&mesh, [x, y, z], dir);
                assert_eq!(hit.as_deref(), Some(l.name.as_str()), "{id}/{}", l.name);
            }
            // Proud of the body, facing out.
            assert!(
                l.material == car_assets::mesh::MaterialSlot::Headlight
                    || l.material == car_assets::mesh::MaterialSlot::Taillight
            );
            assert_eq!(
                l.material == car_assets::mesh::MaterialSlot::Headlight,
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
        for l in mesh.parts.iter().filter(|p| p.kind == PartKind::Light) {
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
