//! CPU-side tests: mesh files, skinning and scene sync against a real free-drive session.

use std::collections::BTreeSet;

use glam::{Mat3, Quat, Vec3};
use wp_contracts::{DamageMode, EntityId, VehicleInput};
use wp_damage::{PanelKind, PanelState, PanelStatus};
use wp_render::{CarMesh, CarMeshLibrary, ChaseCamera, Scene, SkinnedBody, StaticBox};
use wp_wasm::session::Session;

fn session(car: &str) -> Session {
    Session::new(
        wp_wasm::scenario::free_drive(car, DamageMode::Arcade, false),
        false,
    )
    .expect("embedded data is valid")
}

fn lattice(car: &str) -> wp_contracts::LatticeDef {
    let s = session(car);
    s.sim().vehicles()[0].damage.lattice().clone()
}

#[test]
fn every_mesh_file_parses() {
    let dir = CarMeshLibrary::repo_dir();
    let mut count = 0;
    for entry in std::fs::read_dir(&dir).expect("data/panels/meshes") {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "json") {
            let text = std::fs::read_to_string(&path).unwrap();
            let mesh = CarMesh::from_json(&text).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(
                Some(mesh.id.as_str()),
                path.file_stem().and_then(|s| s.to_str())
            );
            count += 1;
        }
    }
    assert!(count >= 10, "only {count} meshes");
}

fn positions(body: &SkinnedBody) -> Vec<Vec3> {
    body.vertices().iter().map(|v| Vec3::from(v.pos)).collect()
}

fn mesh(car: &str) -> CarMesh {
    CarMeshLibrary::repo_default()
        .get(car)
        .map(|m| (*m).clone())
        .expect("mesh")
}

#[test]
fn rest_nodes_give_rest_mesh_and_translation_moves_it() {
    let lat = lattice("debug_sedan");
    let mut body = SkinnedBody::new(&mesh("debug_sedan"), &lat, [1.0, 0.0, 0.0]);
    let rest: Vec<[f32; 3]> = lat.nodes.iter().map(|n| n.pos_m).collect();
    let before = positions(&body);
    body.update(&rest, &[], &BTreeSet::new());
    let at_rest = positions(&body);
    for (a, b) in before.iter().zip(&at_rest) {
        assert!(a.distance(*b) < 1e-5);
    }
    // Moving every node by d moves every vertex by d (weights sum to 1).
    let d = Vec3::new(0.1, -0.2, 0.05);
    let moved: Vec<[f32; 3]> = rest
        .iter()
        .map(|p| (Vec3::from(*p) + d).to_array())
        .collect();
    body.update(&moved, &[], &BTreeSet::new());
    for (a, b) in at_rest.iter().zip(positions(&body)) {
        assert!((b - *a - d).length() < 2e-3, "{a} -> {b}");
    }
}

#[test]
fn free_panel_rotates_rigidly() {
    let lat = lattice("debug_sedan");
    let m = mesh("debug_sedan");
    let door = wp_contracts::BodyRegion::LeftDoorF;
    let mut body = SkinnedBody::new(&m, &lat, [1.0; 3]);
    let rest: Vec<[f32; 3]> = lat.nodes.iter().map(|n| n.pos_m).collect();
    body.update(&rest, &[], &BTreeSet::new());
    let at_rest = positions(&body);
    // Swing the door's nodes 0.6 rad about a vertical hinge through its front-most node.
    let ids: Vec<usize> = (0..lat.nodes.len())
        .filter(|&i| lat.nodes[i].region == door)
        .collect();
    let pivot = ids
        .iter()
        .map(|&i| Vec3::from(rest[i]))
        .max_by(|a, b| a.y.total_cmp(&b.y))
        .unwrap();
    let r = Mat3::from_quat(Quat::from_rotation_z(-0.6));
    let mut nodes = rest.clone();
    for &i in &ids {
        nodes[i] = (r * (Vec3::from(rest[i]) - pivot) + pivot).to_array();
    }
    let panel = PanelStatus {
        region: door,
        kind: PanelKind::Door,
        state: PanelState::Unlatched,
        angle_rad: 0.6,
        omega_rad_s: 0.0,
        pivot: pivot.to_array(),
        axis: [0.0, 0.0, 1.0],
        hinge_load_ns: 0.0,
    };
    body.update(&nodes, &[panel], &BTreeSet::new());
    let now = positions(&body);
    // Every door-panel vertex moved like the rigid rotation (vertices of other parts that
    // blend door nodes are not checked).
    let first = m.parts.iter().filter(|p| p.wheel.is_none());
    let mut v = 0;
    let mut checked = 0;
    for part in first {
        let n = part.vertex_count();
        if part.region == Some(door) && part.name == "door_fl" {
            for k in v..v + n {
                let expect = r * (at_rest[k] - pivot) + pivot;
                assert!(expect.distance(now[k]) < 5e-3, "vertex {k}");
                checked += 1;
            }
        }
        v += n;
    }
    assert!(checked > 0, "door_fl part not found");
}

#[test]
fn scene_follows_session() {
    let mut s = session("debug_sedan");
    let mut scene = Scene::new(CarMeshLibrary::repo_default());
    scene.set_static_boxes(&[StaticBox {
        pos: [0.0, 30.0, 0.75],
        size: [2.0, 1.0, 1.5],
        yaw: 0.0,
        color: [0.5; 3],
    }]);
    assert_eq!(scene.static_mesh().indices.len(), 36);
    let id = EntityId(1);
    for _ in 0..60 {
        let _ = s.set_input(
            id,
            VehicleInput {
                throttle: 0.8,
                ..Default::default()
            },
        );
        s.step(1.0 / 60.0);
        scene.sync(s.sim(), 1.0 / 60.0);
    }
    let car = scene.car(id).expect("player car");
    assert!(car.speed_mps > 1.0);
    assert!(car.wheels.iter().all(|w| w.visible));
    assert!(scene.wheel_meshes("debug_sedan").is_some());
    assert!(!car.body.opaque_indices().is_empty());
    assert!(!car.body.glass_indices().is_empty());
    // The rear-left wheel hub is behind and left of the chassis origin, near the ground.
    let hub = car.wheels[2].to_chassis.transform_point3(Vec3::ZERO);
    assert!(hub.x < 0.0 && hub.y < 0.0 && hub.z < 0.6, "{hub}");
    // The camera ends up behind the car and looks at it.
    let mut cam = ChaseCamera::default();
    let view = cam.update(car.pos, car.rot, car.speed_mps, 1.0 / 60.0);
    let forward = Quat::from_array(car.rot) * Vec3::Y;
    assert!((view.target - view.eye).dot(forward) > 0.0);
    let clip = view.view_proj(16.0 / 9.0) * Vec3::from(car.pos).extend(1.0);
    let ndc = clip.truncate() / clip.w;
    assert!(ndc.x.abs() < 0.2 && ndc.y.abs() < 0.5 && (0.0..1.0).contains(&ndc.z));
}

#[test]
fn missing_mesh_falls_back_to_a_box() {
    let lat = lattice("debug_sedan");
    let body = SkinnedBody::fallback_box(&lat, [0.5; 3]);
    assert_eq!(body.vertices().len(), 8);
    assert_eq!(body.opaque_indices().len(), 36);
}
