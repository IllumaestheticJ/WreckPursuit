//! Time-of-day lighting and headlights against real city sessions.

use glam::Vec3;
use wp_contracts::{DamageMode, EntityId};
use wp_render::lighting::MAX_SPOT_LIGHTS;
use wp_render::{CarMeshLibrary, CityMesh, Scene};
use wp_wasm::session::Session;

fn city_session_at(id: &str, hour: f32) -> Session {
    let mut sc = wp_wasm::scenario::free_drive_in(
        "debug_sedan",
        DamageMode::Arcade,
        wp_wasm::scenario::city_spec(id),
    );
    sc.time_of_day_h = hour;
    Session::new(sc, false).expect("embedded data is valid")
}

fn synced(session: &mut Session) -> Scene {
    for _ in 0..30 {
        session.step(1.0 / 60.0);
    }
    let mut scene = Scene::new(CarMeshLibrary::repo_default());
    scene.sync(session.sim(), 1.0 / 60.0);
    scene
}

#[test]
fn noon_headlights_follow_driver_input() {
    let mut s = city_session_at("procedural_12x12", 12.0);
    let scene = synced(&mut s);
    assert!(scene.lighting().night < 0.05);
    // The player has not switched them on; police drive with them on all day.
    assert!(!scene.car(EntityId(1)).unwrap().headlights_on);
    let on = s
        .sim()
        .vehicles()
        .iter()
        .filter(|v| v.input.headlights)
        .count();
    assert_eq!(scene.lights().len(), 2 * on);
}

#[test]
fn night_turns_on_every_headlight() {
    let mut s = city_session_at("procedural_12x12", 23.0);
    let scene = synced(&mut s);
    assert!(scene.lighting().night > 0.9);
    let player = scene.car(EntityId(1)).unwrap();
    assert!(player.headlights_on);
    // Two per car plus two per abstract traffic box.
    let boxes = s
        .sim()
        .world()
        .agents()
        .filter(|a| a.lod != wp_world::Lod::Physical)
        .count();
    let cars = scene.cars().count();
    assert_eq!(scene.lights().len(), 2 * (cars + boxes));
    // The player's lamps are at its nose, pointing where it faces and slightly down.
    let forward = player.to_world.transform_vector3(Vec3::Y);
    let lamps: Vec<_> = scene
        .lights()
        .iter()
        .filter(|l| Vec3::from(l.pos).distance(Vec3::from(player.pos)) < 3.0)
        .collect();
    assert_eq!(lamps.len(), 2);
    for l in lamps {
        let d = Vec3::from(l.dir);
        assert!(d.dot(forward) > 0.99 && d.z < 0.0);
        assert!((Vec3::from(l.pos) - Vec3::from(player.pos)).dot(forward) > 1.0);
    }
}

#[test]
fn nearest_lights_are_capped_and_sorted() {
    let mut s = city_session_at("procedural_12x12", 0.0);
    let scene = synced(&mut s);
    let eye = Vec3::from(scene.car(EntityId(1)).unwrap().pos);
    let near = scene.lights_near(eye);
    assert_eq!(near.len(), scene.lights().len().min(MAX_SPOT_LIGHTS));
    let d: Vec<f32> = near
        .iter()
        .map(|l| Vec3::from(l.pos).distance(eye))
        .collect();
    assert!(d.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn building_walls_carry_face_normals() {
    let city = CityMesh::load_repo("procedural_12x12").unwrap();
    let walls: Vec<_> = city.layers[0]
        .vertices
        .iter()
        .filter(|v| v.shade == wp_render::Shade::Building as u32 as f32)
        .collect();
    assert!(!walls.is_empty());
    assert!(walls
        .iter()
        .all(|v| (Vec3::from(v.normal).length() - 1.0).abs() < 1e-3));
}
