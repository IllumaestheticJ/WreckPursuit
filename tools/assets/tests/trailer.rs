//! The semi-trailer mesh (`box_trailer`, `data/trailers/`): valid, up to date, and shaped like
//! a dry van — wheels on the def's tandem, landing gear behind the kingpin, rear lamps, no head
//! lamps, box clear of the tires.

use car_assets::mesh::{CarMesh, MaterialSlot, MeshPart, PartKind};
use car_assets::trailer::TrailerBox;
use car_assets::{
    build_mesh, car_ids, is_trailer, load_car, load_mesh, load_style, mesh_path, repo_root,
    trailer_ids, validate::validate, vehicle_ids,
};
use wp_contracts::{BodyRegion, LoadedCar};

const ID: &str = "box_trailer";

fn trailer() -> (LoadedCar, CarMesh) {
    let root = repo_root();
    let car = load_car(&root, ID).unwrap();
    let mesh = build_mesh(&car, &load_style(&root, ID).unwrap());
    (car, mesh)
}

/// Axis-aligned bounds of a part (chassis metres).
fn bounds(p: &MeshPart) -> ([f32; 3], [f32; 3]) {
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

fn part<'a>(m: &'a CarMesh, name: &str) -> &'a MeshPart {
    m.parts
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("no part {name}"))
}

#[test]
fn trailers_are_found_and_kept_apart_from_cars() {
    let root = repo_root();
    assert!(trailer_ids(&root).contains(&ID.to_string()));
    assert!(!car_ids(&root).contains(&ID.to_string()));
    assert!(is_trailer(&root, ID));
    assert!(!is_trailer(&root, "longhaul_tractor"));
    assert_eq!(vehicle_ids(&root).last().map(String::as_str), Some(ID));
}

#[test]
fn trailer_mesh_is_valid_and_up_to_date() {
    let root = repo_root();
    let (car, fresh) = trailer();
    let errs = validate(&car, &fresh, None);
    assert!(errs.is_empty(), "{errs:#?}");
    let on_disk = load_mesh(&mesh_path(&root, ID)).unwrap();
    assert!(
        fresh == on_disk,
        "data/panels/meshes/{ID}.json is stale; run `cargo run -p car-assets --bin gen-car-mesh`"
    );
    assert!(fresh.tri_count <= CarMesh::budget_max(car.def.body_size_m[1]));
    assert_eq!(fresh.to_json(), trailer().1.to_json(), "deterministic");
}

#[test]
fn every_lattice_region_is_covered_and_no_cabin_parts() {
    let (car, mesh) = trailer();
    for n in &car.lattice.nodes {
        assert!(
            mesh.parts.iter().any(|p| p.region == Some(n.region)),
            "region {:?} has nodes but no part",
            n.region
        );
    }
    // The trailer lattice has no Cabin nodes (relabelled FrameRear): nothing may bind there.
    assert!(mesh
        .parts
        .iter()
        .all(|p| p.region != Some(BodyRegion::Cabin)));
    // The rear door leaves are the trunk (as on box trucks) and are painted panels.
    let doors = part(&mesh, "trunk");
    assert_eq!(doors.kind, PartKind::Panel);
    let (lo, hi) = bounds(doors);
    assert!(
        hi[1] - lo[1] < 0.01,
        "door leaves are flat on the rear face"
    );
    assert!(lo[1] < car.def.wheels[2].position_m[1]);
}

#[test]
fn wheels_are_dual_on_both_tandem_axles_at_the_def_positions() {
    let (car, mesh) = trailer();
    assert_eq!(car.def.wheels.len(), 4);
    for (i, w) in car.def.wheels.iter().enumerate() {
        let tire = mesh
            .parts
            .iter()
            .find(|p| p.wheel == Some(i as u8) && p.kind == PartKind::Tire)
            .unwrap();
        // On the physics hub (no visual extra axle): the renderer places it at position_m.
        assert!(tire.hub_offset_mm.is_none());
        let (lo, hi) = bounds(tire);
        assert!((hi[2] - w.radius_m).abs() < 0.01, "wheel {i} radius");
        assert!(
            (hi[0] - lo[0] - w.width_m).abs() < 0.03,
            "wheel {i} dual width"
        );
        // Dual: two tires side by side, so there are inner sidewalls near the hub-space
        // centre plane (a single tire's sidewalls are at ±width/2).
        let inner = (0..tire.vertex_count()).any(|v| tire.vertex(v)[0].abs() < 0.25 * w.width_m);
        assert!(inner, "wheel {i} should be a dual pair");
    }
    // The box floor clears every tire top.
    let wheel_top = car
        .def
        .wheels
        .iter()
        .map(|w| w.position_m[2] + w.radius_m)
        .fold(f32::MIN, f32::max);
    for p in mesh.parts.iter().filter(|p| p.kind == PartKind::Panel) {
        let (lo, _) = bounds(p);
        assert!(lo[2] > wheel_top + 0.1, "{} hangs into the wheels", p.name);
    }
    // Mud flaps behind the trailing axle.
    let trail = car.def.wheels[2].position_m[1];
    for side in ["l", "r"] {
        let (_, hi) = bounds(part(&mesh, &format!("mudflap_{side}")));
        assert!(hi[1] < trail - car.def.wheels[2].radius_m);
    }
}

#[test]
fn landing_gear_stands_behind_the_kingpin_ahead_of_the_tandem() {
    let root = repo_root();
    let (car, mesh) = trailer();
    let style = load_style(&root, ID).unwrap();
    let [kp_y, kp_z] = style.parts.trailer.kingpin;
    let (lo, hi) = bounds(part(&mesh, "kingpin"));
    assert!(lo[1] < kp_y && hi[1] > kp_y && (hi[2] - kp_z).abs() < 0.005);
    let lead = car.def.wheels[0].position_m[1];
    for side in ["l", "r"] {
        let leg = part(&mesh, &format!("landing_gear_{side}"));
        assert_eq!(leg.region, Some(BodyRegion::FrameFront));
        let (lo, hi) = bounds(leg);
        let y = 0.5 * (lo[1] + hi[1]);
        assert!(y < kp_y - 0.5 && y > lead + 2.0, "leg at y {y}");
        let (plo, _) = bounds(part(&mesh, &format!("landing_gear_pad_{side}")));
        assert!(plo[2] > 0.1 && plo[2] < 0.5, "pad raised off the ground");
        assert_eq!((lo[0] > 0.0), side == "r");
    }
    assert!(mesh.parts.iter().any(|p| p.name == "landing_gear_crank"));
}

#[test]
fn rear_lamps_and_markers_but_no_head_lamps() {
    let (_, mesh) = trailer();
    let bx = {
        let root = repo_root();
        let car = load_car(&root, ID).unwrap();
        TrailerBox::measure(&car.def, &car.lattice, &load_style(&root, ID).unwrap())
    };
    let lights: Vec<&MeshPart> = mesh
        .parts
        .iter()
        .filter(|p| p.kind == PartKind::Light)
        .collect();
    assert!(lights
        .iter()
        .all(|p| p.material != MaterialSlot::Headlight && p.material != MaterialSlot::Lens));
    let at_rear = |p: &MeshPart| bounds(p).1[1] < bx.y_tail;
    // Low red tail/brake lamps and amber indicators on the rear, both sides.
    for (mat, min) in [(MaterialSlot::Taillight, 4), (MaterialSlot::Indicator, 2)] {
        let low: Vec<_> = lights
            .iter()
            .filter(|p| p.material == mat && at_rear(p) && bounds(p).1[2] < bx.z_floor + 0.3)
            .collect();
        assert!(low.len() >= min, "{mat:?}: {}", low.len());
    }
    // Red clearance markers at the rear top corners.
    let top_red = lights
        .iter()
        .filter(|p| p.material == MaterialSlot::Taillight && at_rear(p))
        .filter(|p| bounds(p).0[2] > bx.z_roof - 0.2 && bounds(p).0[0].abs() > 0.9)
        .count();
    assert_eq!(top_red, 2);
    // Amber markers along the sides.
    let side_amber = lights
        .iter()
        .filter(|p| p.material == MaterialSlot::Indicator)
        .filter(|p| bounds(p).1[0].abs() >= bx.half_w || bounds(p).0[0].abs() >= bx.half_w)
        .count();
    assert!(side_amber >= 6, "{side_amber} side markers");
}
