//! Every car has an up-to-date, valid mesh and panel spec; meshes deform with the lattice and
//! hinged parts line up with their hinge nodes.

#[path = "common/crash.rs"]
mod crash;

use car_assets::crumple::{measure, render_detail, PartCrumple, Pose};
use car_assets::mesh::{PartKind, FOLD_MAX_M, LID_POP_M, TRI_BUDGET};
use car_assets::skin::{deform_detail, FoldMemory};
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
        ("debug_sedan", 50.0, "car", "Arcade", 0.12),
        ("debug_sedan", 50.0, "barrier", "Arcade", 0.15),
        ("shiro_turbo", 50.0, "car", "Arcade", 0.12),
        ("sentinel_cruiser", 50.0, "barrier", "Arcade", 0.15),
        ("debug_sedan", 56.0, "barrier", "Simulation", 0.2),
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

/// The crumple must not depend on the browser's frame rate: a bumper that crushes and lets go
/// between two frames still counts (its hanging pose is rotated back to the crushed one).
#[test]
fn crumple_does_not_depend_on_frame_rate() {
    let (fast, _, _) = crumple_after("debug_sedan", 50.0, "car", "Arcade", 4);
    let (slow, _, _) = crumple_after("debug_sedan", 50.0, "car", "Arcade", 16);
    assert!(
        (fast.max_fold_m - slow.max_fold_m).abs() <= 0.03,
        "{fast:?} vs {slow:?}"
    );
}

/// Harder hits crumple more (up to the cap).
#[test]
fn harder_hits_crumple_more() {
    let (soft, _, _) = crumple_after("debug_sedan", 30.0, "car", "Arcade", 4);
    let (hard, _, _) = crumple_after("debug_sedan", 70.0, "car", "Arcade", 4);
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
