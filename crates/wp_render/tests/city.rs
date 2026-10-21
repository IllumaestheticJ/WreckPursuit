//! City greybox loading, traffic boxes and the reversed-Z camera.

use std::time::Instant;

use glam::Vec3;
use wp_contracts::DamageMode;
use wp_render::city::{decal_layer, DECAL_LAYERS};
use wp_render::{CarMeshLibrary, ChaseCamera, CityMesh, Scene};
use wp_wasm::session::Session;

fn city_session(id: &str) -> Session {
    Session::new(
        wp_wasm::scenario::free_drive_in(
            "debug_sedan",
            DamageMode::Arcade,
            wp_wasm::scenario::city_spec(id),
        ),
        false,
    )
    .expect("embedded data is valid")
}

/// The greybox covers the road network it was generated from, in world (+Z up) coordinates.
fn check_city(id: &str) -> CityMesh {
    let started = Instant::now();
    let city = CityMesh::load_repo(id).unwrap_or_else(|e| panic!("{e}"));
    println!(
        "{id}: {} triangles, loaded in {:.0} ms",
        city.triangle_count(),
        started.elapsed().as_secs_f64() * 1e3
    );
    assert_eq!(city.id, id);
    // Base layer (buildings, ground) and the road layer are both present.
    assert!(!city.layers[0].indices.is_empty());
    assert!(!city.layers[1].indices.is_empty());
    for layer in &city.layers {
        assert_eq!(layer.indices.len() % 3, 0);
        assert!(layer
            .vertices
            .iter()
            .all(|v| v.pos.iter().all(|c| c.is_finite())));
    }
    // Buildings stand up along +Z.
    assert!(city.bounds.1[2] > 10.0, "{:?}", city.bounds);

    let session = city_session(id);
    let net = session.sim().world().network();
    assert_eq!(net.id, id);
    let (lo, hi) = city.bounds;
    for n in &net.nodes {
        let p = n.pos;
        assert!(
            (0..3).all(|k| p[k] >= lo[k] - 1.0 && p[k] <= hi[k] + 1.0),
            "road node {p:?} outside greybox {:?}",
            city.bounds
        );
    }
    city
}

#[test]
fn procedural_city_greybox_loads() {
    let city = check_city("procedural_12x12");
    // Flat city: nothing far below the ground, and the road layer sits at z = 0.
    assert!(city.bounds.0[2] > -1.0, "{:?}", city.bounds);
    assert!(city.layers[1].vertices.iter().all(|v| v.pos[2].abs() < 0.5));
}

#[test]
fn vesper_greybox_loads_with_smooth_terrain() {
    let city = check_city("vesper_v2");
    // Terrain is smooth-shaded: it carries unit normals, mostly pointing up. (Building walls
    // carry face normals too; leave them out.)
    let normals: Vec<Vec3> = city.layers[0]
        .vertices
        .iter()
        .filter(|v| v.shade != wp_render::Shade::Building as u32 as f32)
        .map(|v| Vec3::from(v.normal))
        .filter(|n| *n != Vec3::ZERO)
        .collect();
    assert!(normals.len() > 1000, "{} smooth vertices", normals.len());
    assert!(normals.iter().all(|n| (n.length() - 1.0).abs() < 0.01));
    assert!(normals.iter().filter(|n| n.z > 0.5).count() > normals.len() * 9 / 10);
    // Hills: the city spans tens of metres of height.
    assert!(
        city.bounds.1[2] - city.bounds.0[2] > 50.0,
        "{:?}",
        city.bounds
    );
}

#[test]
fn decal_layer_indices_are_in_range() {
    for m in [
        "asphalt", "parking", "bus_lane", "sidewalk", "building", "water", "",
    ] {
        assert!(decal_layer(m) < DECAL_LAYERS);
    }
}

#[test]
fn traffic_far_from_the_player_draws_as_boxes() {
    let mut session = city_session("procedural_12x12");
    for _ in 0..30 {
        session.step(1.0 / 60.0);
    }
    let mut scene = Scene::new(CarMeshLibrary::repo_default());
    scene.sync(session.sim(), 1.0 / 60.0);
    let boxed = session
        .sim()
        .world()
        .agents()
        .filter(|a| a.lod != wp_world::Lod::Physical)
        .count();
    assert!(boxed > 0, "no abstract traffic in the city");
    let mesh = scene.traffic_mesh();
    assert_eq!(mesh.vertices.len(), boxed * 8);
    assert_eq!(mesh.indices.len(), boxed * 36);
    // Boxes sit on the road: bottom at the agent's height, 1.4 m tall.
    let a = session
        .sim()
        .world()
        .agents()
        .find(|a| a.lod != wp_world::Lod::Physical)
        .unwrap();
    let z: Vec<f32> = mesh.vertices[..8].iter().map(|v| v.pos[2]).collect();
    let (lo, hi) = z
        .iter()
        .fold((f32::MAX, f32::MIN), |(l, h), z| (l.min(*z), h.max(*z)));
    assert!((lo - a.pos[2]).abs() < 1e-3 && (hi - lo - 1.4).abs() < 1e-3);
}

#[test]
fn camera_depth_is_reversed() {
    let mut cam = ChaseCamera::default();
    let view = cam.update([0.0; 3], [0.0, 0.0, 0.0, 1.0], 0.0, 1.0 / 60.0);
    let vp = view.view_proj(16.0 / 9.0);
    let depth = |p: Vec3| {
        let c = vp * p.extend(1.0);
        c.z / c.w
    };
    let near = depth(Vec3::new(0.0, 2.0, 1.0));
    let far = depth(Vec3::new(0.0, 500.0, 1.0));
    assert!(near > far && far > 0.0 && near < 1.0, "{near} {far}");
}
