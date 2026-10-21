//! The renderer-side picture of a [`Sim`]: one [`CarView`] per vehicle (skinned body, wheel
//! poses, chassis transform) plus static boxes. Pure CPU; the GPU layer only uploads what this
//! produces. Reading the sim never changes it.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use glam::{Mat4, Quat, Vec3};
use wp_contracts::{BodyRegion, EntityId};
use wp_damage::PanelStatus;
use wp_sim::Sim;

use crate::car_mesh::CarMeshLibrary;
use crate::skin::{material_look, wheel_meshes, SkinnedBody, StaticMesh};
use crate::{Shade, Vertex};

/// A static axis-aligned-then-yawed box (scenario barriers, test props).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StaticBox {
    /// Centre, world m.
    pub pos: [f32; 3],
    /// Full size `[x, y, z]`, m (before yaw).
    pub size: [f32; 3],
    /// Heading, rad about +Z.
    pub yaw: f32,
    /// Linear RGB colour.
    pub color: [f32; 3],
}

/// Pose of one wheel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WheelPose {
    /// Hub space → chassis space (steer and spin included).
    pub to_chassis: Mat4,
    /// False once the wheel has been torn off.
    pub visible: bool,
}

/// Everything needed to draw one vehicle.
#[derive(Debug, Clone)]
pub struct CarView {
    /// Sim entity.
    pub entity: EntityId,
    /// Car definition id (mesh and wheel lookup).
    pub car_id: String,
    /// Chassis space → world space.
    pub to_world: Mat4,
    /// Chassis position, world m.
    pub pos: [f32; 3],
    /// Chassis orientation `[x, y, z, w]`.
    pub rot: [f32; 4],
    /// Forward speed, m/s.
    pub speed_mps: f32,
    /// Deformed body.
    pub body: SkinnedBody,
    /// Increments whenever [`Self::body`] vertices changed (GPU re-upload).
    pub body_revision: u64,
    /// Wheel poses FL, FR, RL, RR.
    pub wheels: [WheelPose; 4],
    spin: [f32; 4],
    last_nodes: Vec<[f32; 3]>,
    last_panels: Vec<PanelStatus>,
    last_shattered: BTreeSet<BodyRegion>,
}

/// Renderer-side scene, kept in step with a [`Sim`] by [`Scene::sync`].
#[derive(Debug)]
pub struct Scene {
    library: CarMeshLibrary,
    cars: BTreeMap<EntityId, CarView>,
    wheel_meshes: BTreeMap<String, Arc<[StaticMesh; 4]>>,
    boxes: StaticMesh,
    boxes_revision: u64,
}

/// Body colours handed out by entity id (linear RGB).
const PAINTS: [[f32; 3]; 6] = [
    [0.55, 0.05, 0.03],
    [0.03, 0.18, 0.45],
    [0.60, 0.42, 0.04],
    [0.08, 0.30, 0.10],
    [0.50, 0.50, 0.52],
    [0.25, 0.04, 0.30],
];

/// Paint of an entity: police analogs are near-black, everyone else cycles [`PAINTS`].
pub fn paint_for(entity: EntityId, car_id: &str) -> [f32; 3] {
    if car_id.starts_with("sentinel_") {
        [0.02, 0.025, 0.04]
    } else {
        PAINTS[(entity.0 as usize).wrapping_sub(1) % PAINTS.len()]
    }
}

impl Scene {
    /// Empty scene reading car meshes from `library`.
    pub fn new(library: CarMeshLibrary) -> Self {
        Self {
            library,
            cars: BTreeMap::new(),
            wheel_meshes: BTreeMap::new(),
            boxes: StaticMesh::default(),
            boxes_revision: 0,
        }
    }

    /// Replace the static boxes.
    pub fn set_static_boxes(&mut self, boxes: &[StaticBox]) {
        let mut mesh = StaticMesh::default();
        for b in boxes {
            let to_world =
                Mat4::from_rotation_translation(Quat::from_rotation_z(b.yaw), Vec3::from(b.pos));
            push_box(&mut mesh, to_world, Vec3::from(b.size) * 0.5, b.color);
        }
        self.boxes = mesh;
        self.boxes_revision += 1;
    }

    /// Static boxes as one world-space mesh.
    pub fn static_mesh(&self) -> &StaticMesh {
        &self.boxes
    }

    /// Increments whenever [`Self::static_mesh`] changed.
    pub fn static_revision(&self) -> u64 {
        self.boxes_revision
    }

    /// Bring every car up to date with `sim`. `frame_dt` (s) only advances wheel spin angles.
    pub fn sync(&mut self, sim: &Sim, frame_dt: f32) {
        let states: BTreeMap<EntityId, _> = sim.vehicle_states().into_iter().collect();
        self.cars.retain(|id, _| states.contains_key(id));
        for v in sim.vehicles() {
            let Some(st) = states.get(&v.id) else {
                continue;
            };
            let lattice = v.damage.lattice();
            if !self.cars.contains_key(&v.id) {
                let paint = paint_for(v.id, &v.car_id);
                let body = match self.library.get(&v.car_id) {
                    Some(mesh) if mesh.lattice_id == lattice.id => {
                        self.wheel_meshes
                            .entry(v.car_id.clone())
                            .or_insert_with(|| Arc::new(wheel_meshes(&mesh)));
                        SkinnedBody::new(&mesh, lattice, paint)
                    }
                    _ => SkinnedBody::fallback_box(lattice, paint),
                };
                self.cars.insert(
                    v.id,
                    CarView {
                        entity: v.id,
                        car_id: v.car_id.clone(),
                        to_world: Mat4::IDENTITY,
                        pos: st.pos,
                        rot: st.rot,
                        speed_mps: 0.0,
                        body,
                        body_revision: 0,
                        wheels: [WheelPose {
                            to_chassis: Mat4::IDENTITY,
                            visible: false,
                        }; 4],
                        spin: [0.0; 4],
                        last_nodes: Vec::new(),
                        last_panels: Vec::new(),
                        last_shattered: BTreeSet::new(),
                    },
                );
            }
            let car = self.cars.get_mut(&v.id).expect("inserted above");
            car.pos = st.pos;
            car.rot = st.rot;
            car.speed_mps = st.speed_mps;
            car.to_world =
                Mat4::from_rotation_translation(Quat::from_array(st.rot), Vec3::from(st.pos));

            let Ok(nodes) = sim.node_positions(v.id) else {
                continue;
            };
            let panels = v.damage.panel_states();
            let shattered = v.damage.shattered_regions();
            if car.body_revision == 0
                || car.last_nodes != nodes
                || car.last_panels != panels
                || &car.last_shattered != shattered
            {
                car.body.update(nodes, &panels, shattered);
                car.body_revision += 1;
                car.last_nodes = nodes.to_vec();
                car.last_panels = panels;
                car.last_shattered = shattered.clone();
            }

            let def = v.vehicle.def();
            for i in 0..4 {
                let ws = &st.wheel_states[i];
                car.spin[i] = (car.spin[i] + ws.omega * frame_dt) % std::f32::consts::TAU;
                let [x, y, z] = def.wheels[i].position_m;
                // Same approximation as the browser view: static hub height plus compression
                // about mid-travel.
                let z = z + ws.compression_m - def.suspension[i].travel_m * 0.5;
                car.wheels[i] = WheelPose {
                    to_chassis: Mat4::from_translation(Vec3::new(x, y, z))
                        * Mat4::from_rotation_z(ws.steer_rad)
                        * Mat4::from_rotation_x(car.spin[i]),
                    visible: !ws.detached,
                };
            }
        }
    }

    /// Cars in ascending entity order.
    pub fn cars(&self) -> impl Iterator<Item = &CarView> {
        self.cars.values()
    }

    /// One car.
    pub fn car(&self, id: EntityId) -> Option<&CarView> {
        self.cars.get(&id)
    }

    /// Wheel meshes of a car id (hub space), if its mesh file has them.
    pub fn wheel_meshes(&self, car_id: &str) -> Option<&Arc<[StaticMesh; 4]>> {
        self.wheel_meshes.get(car_id)
    }
}

/// Append a box with half extents `half`, transformed by `to_world`.
fn push_box(mesh: &mut StaticMesh, to_world: Mat4, half: Vec3, color: [f32; 3]) {
    let look = material_look(crate::car_mesh::MaterialSlot::Paint, color);
    let base = mesh.vertices.len() as u32;
    for i in 0..8 {
        let c = Vec3::new(
            if i & 1 == 0 { -half.x } else { half.x },
            if i & 2 == 0 { -half.y } else { half.y },
            if i & 4 == 0 { -half.z } else { half.z },
        );
        mesh.vertices.push(Vertex {
            pos: to_world.transform_point3(c).to_array(),
            color: look.0,
            shade: Shade::Lit as u32 as f32,
        });
    }
    for q in [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ] {
        mesh.indices
            .extend([q[0], q[1], q[2], q[0], q[2], q[3]].map(|i| base + i));
    }
}
