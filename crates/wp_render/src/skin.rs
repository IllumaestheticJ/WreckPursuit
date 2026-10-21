//! CPU skinning of a car mesh to its damage lattice (the Rust port of
//! `apps/testbed-web/src/render/panels.ts`).
//!
//! Every body vertex is its rest position plus the weighted displacement of up to four lattice
//! nodes, so every dent shows on the panels. Free panels (unlatched doors, hanging bumpers) are
//! rigid bodies in the sim whose nodes are already rotated about the hinge; plain blending would
//! smear them, so for those regions we fit the panel's rigid frame from three of its nodes, take
//! the dent in that frame and put the vertices back through it. Detached regions and shattered
//! glass are hidden (their triangles collapse to a point).

use std::collections::{BTreeMap, BTreeSet};

use glam::{Mat3, Vec3};
use wp_contracts::{BodyRegion, LatticeDef};
use wp_damage::{PanelState, PanelStatus};

use crate::car_mesh::{CarMesh, MaterialSlot, MeshPart, PartKind, MESH_FORMAT};
use crate::{Shade, Vertex};

/// Colour and shading of a material slot. `paint` is the body colour (linear RGB).
pub fn material_look(slot: MaterialSlot, paint: [f32; 3]) -> ([f32; 4], Shade) {
    match slot {
        MaterialSlot::Paint => ([paint[0], paint[1], paint[2], 1.0], Shade::Lit),
        MaterialSlot::Glass => ([0.10, 0.16, 0.20, 0.45], Shade::Lit),
        MaterialSlot::Trim => ([0.03, 0.03, 0.035, 1.0], Shade::Lit),
        MaterialSlot::Headlight => ([1.0, 0.95, 0.80, 1.0], Shade::Emissive),
        MaterialSlot::Taillight => ([0.55, 0.02, 0.02, 1.0], Shade::Emissive),
        MaterialSlot::Rubber => ([0.02, 0.02, 0.02, 1.0], Shade::Lit),
        MaterialSlot::Metal => ([0.45, 0.47, 0.50, 1.0], Shade::Lit),
        MaterialSlot::Interior => ([0.05, 0.045, 0.04, 1.0], Shade::Lit),
    }
}

fn vertex(pos: Vec3, look: ([f32; 4], Shade)) -> Vertex {
    Vertex {
        pos: pos.to_array(),
        color: look.0,
        shade: look.1 as u32 as f32,
        normal: [0.0; 3],
    }
}

/// A static triangle mesh (wheels, barriers, ground).
#[derive(Debug, Clone, Default)]
pub struct StaticMesh {
    /// Vertices.
    pub vertices: Vec<Vertex>,
    /// Opaque triangles.
    pub indices: Vec<u32>,
}

/// One body part being skinned.
#[derive(Debug, Clone)]
struct BodyPart {
    region: Option<BodyRegion>,
    glass: bool,
    first_vertex: usize,
    rest: Vec<Vec3>,
    nodes: Vec<[u16; 4]>,
    weights: Vec<[f32; 4]>,
}

/// A car body whose vertices follow the damage lattice.
#[derive(Debug, Clone)]
pub struct SkinnedBody {
    parts: Vec<BodyPart>,
    rest_nodes: Vec<Vec3>,
    /// Region → three nodes spanning its largest rest triangle (rigid frame fit).
    span: BTreeMap<BodyRegion, [usize; 3]>,
    vertices: Vec<Vertex>,
    opaque_indices: Vec<u32>,
    glass_indices: Vec<u32>,
}

impl SkinnedBody {
    /// Build from a car mesh (wheel parts are skipped, see [`wheel_meshes`]) and its lattice.
    pub fn new(mesh: &CarMesh, lattice: &LatticeDef, paint: [f32; 3]) -> Self {
        let rest_nodes: Vec<Vec3> = lattice.nodes.iter().map(|n| Vec3::from(n.pos_m)).collect();
        let mut body = SkinnedBody {
            parts: Vec::new(),
            rest_nodes,
            span: BTreeMap::new(),
            vertices: Vec::new(),
            opaque_indices: Vec::new(),
            glass_indices: Vec::new(),
        };
        let node_count = lattice.nodes.len();
        for def in mesh.parts.iter().filter(|p| p.wheel.is_none()) {
            // A binding that points past the lattice (stale mesh) would panic later; skip it.
            if def.skin_nodes.iter().any(|&n| n as usize >= node_count) {
                continue;
            }
            let look = material_look(def.material, paint);
            let first_vertex = body.vertices.len();
            let n = def.vertex_count();
            let rest: Vec<Vec3> = (0..n).map(|v| Vec3::from(def.vertex(v))).collect();
            body.vertices.extend(rest.iter().map(|&p| vertex(p, look)));
            let glass = def.kind == PartKind::Glass;
            let out = if glass {
                &mut body.glass_indices
            } else {
                &mut body.opaque_indices
            };
            out.extend(def.indices.iter().map(|&i| i + first_vertex as u32));
            let nodes = def
                .skin_nodes
                .chunks_exact(4)
                .map(|c| [c[0], c[1], c[2], c[3]])
                .collect();
            let weights = def
                .skin_weights
                .chunks_exact(4)
                .map(|c| [c[0], c[1], c[2], c[3]].map(|w| w as f32 / 255.0))
                .collect();
            if let Some(region) = def.region {
                body.span
                    .entry(region)
                    .or_insert_with(|| span_triangle(lattice, region));
            }
            body.parts.push(BodyPart {
                region: def.region,
                glass,
                first_vertex,
                rest,
                nodes,
                weights,
            });
        }
        body
    }

    /// A plain box over the lattice bounds, each corner bound to its nearest node. Used for cars
    /// without a mesh file so they still show crush.
    pub fn fallback_box(lattice: &LatticeDef, paint: [f32; 3]) -> Self {
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for n in &lattice.nodes {
            lo = lo.min(Vec3::from(n.pos_m));
            hi = hi.max(Vec3::from(n.pos_m));
        }
        let mut positions_mm = Vec::new();
        let mut skin_nodes = Vec::new();
        for i in 0..8 {
            let c = Vec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            );
            positions_mm.extend(c.to_array().map(|v| (v * 1000.0).round() as i32));
            let nearest = lattice
                .nodes
                .iter()
                .enumerate()
                .min_by(|a, b| {
                    let da = Vec3::from(a.1.pos_m).distance_squared(c);
                    let db = Vec3::from(b.1.pos_m).distance_squared(c);
                    da.total_cmp(&db)
                })
                .map_or(0, |(i, _)| i as u16);
            skin_nodes.extend([nearest, nearest, nearest, nearest]);
        }
        let skin_weights = [255u8, 0, 0, 0].repeat(8);
        // Corners: bit 0 = +X, bit 1 = +Y, bit 2 = +Z; quads counter-clockwise from outside.
        let quads = [
            [0, 2, 3, 1], // bottom
            [4, 5, 7, 6], // top
            [0, 1, 5, 4], // rear
            [2, 6, 7, 3], // front
            [0, 4, 6, 2], // left
            [1, 3, 7, 5], // right
        ];
        let indices = quads
            .iter()
            .flat_map(|q| [q[0], q[1], q[2], q[0], q[2], q[3]])
            .collect();
        let mesh = CarMesh {
            id: lattice.id.clone(),
            lattice_id: lattice.id.clone(),
            format: MESH_FORMAT,
            parts: vec![MeshPart {
                name: "fallback_box".into(),
                region: None,
                kind: PartKind::Body,
                material: MaterialSlot::Paint,
                wheel: None,
                positions_mm,
                indices,
                skin_nodes,
                skin_weights,
            }],
        };
        Self::new(&mesh, lattice, paint)
    }

    /// Re-skin from the current lattice nodes (chassis space, same order as the lattice), the
    /// panel states and the regions whose glass has shattered.
    pub fn update(
        &mut self,
        nodes: &[[f32; 3]],
        panels: &[PanelStatus],
        shattered: &BTreeSet<BodyRegion>,
    ) {
        if nodes.len() != self.rest_nodes.len() {
            return;
        }
        let mut frames: BTreeMap<BodyRegion, (Mat3, Vec3)> = BTreeMap::new();
        let mut detached: BTreeSet<BodyRegion> = BTreeSet::new();
        for p in panels {
            match p.state {
                PanelState::Unlatched | PanelState::HangingByOne => {
                    if let Some(&tri) = self.span.get(&p.region) {
                        frames.insert(p.region, fit_frame(&self.rest_nodes, nodes, tri));
                    }
                }
                PanelState::Detached => {
                    detached.insert(p.region);
                }
                PanelState::Attached => {}
            }
        }
        for part in &self.parts {
            let hidden = part
                .region
                .is_some_and(|r| detached.contains(&r) || (part.glass && shattered.contains(&r)));
            let frame = part.region.and_then(|r| frames.get(&r));
            for (v, rest) in part.rest.iter().enumerate() {
                let out = &mut self.vertices[part.first_vertex + v].pos;
                if hidden {
                    *out = part.rest[0].to_array();
                    continue;
                }
                let mut dent = Vec3::ZERO;
                for k in 0..4 {
                    let w = part.weights[v][k];
                    if w == 0.0 {
                        continue;
                    }
                    let i = part.nodes[v][k] as usize;
                    let now = Vec3::from(nodes[i]);
                    let local = match frame {
                        // Node in the panel's own frame.
                        Some((r, t)) => r.transpose() * (now - *t),
                        None => now,
                    };
                    dent += w * (local - self.rest_nodes[i]);
                }
                let p = match frame {
                    Some((r, t)) => *r * (*rest + dent) + *t,
                    None => *rest + dent,
                };
                *out = p.to_array();
            }
        }
    }

    /// Vertices in chassis space.
    pub fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }

    /// Opaque triangles (indices into [`Self::vertices`]).
    pub fn opaque_indices(&self) -> &[u32] {
        &self.opaque_indices
    }

    /// Glass triangles, drawn after the opaque pass with blending.
    pub fn glass_indices(&self) -> &[u32] {
        &self.glass_indices
    }
}

/// The four wheels' meshes in hub space (axle on +X), indexed FL, FR, RL, RR. Wheels without
/// parts in the file are empty.
pub fn wheel_meshes(mesh: &CarMesh) -> [StaticMesh; 4] {
    let mut out: [StaticMesh; 4] = Default::default();
    for def in &mesh.parts {
        let Some(w) = def.wheel.filter(|&w| w < 4) else {
            continue;
        };
        let m = &mut out[w as usize];
        let look = material_look(def.material, [0.0; 3]);
        let base = m.vertices.len() as u32;
        m.vertices
            .extend((0..def.vertex_count()).map(|v| vertex(Vec3::from(def.vertex(v)), look)));
        m.indices.extend(def.indices.iter().map(|&i| i + base));
    }
    out
}

/// Three nodes of `region` spanning the largest triangle in the rest pose.
fn span_triangle(lattice: &LatticeDef, region: BodyRegion) -> [usize; 3] {
    let ids: Vec<usize> = (0..lattice.nodes.len())
        .filter(|&i| lattice.nodes[i].region == region)
        .collect();
    let p = |i: usize| Vec3::from(lattice.nodes[i].pos_m);
    let mut best = [0, 1, 2].map(|k| ids.get(k).copied().unwrap_or(0));
    let mut area = -1.0;
    for (a, &i) in ids.iter().enumerate() {
        for (b, &j) in ids.iter().enumerate().skip(a + 1) {
            for &k in ids.iter().skip(b + 1) {
                let ar = (p(j) - p(i)).cross(p(k) - p(i)).length_squared();
                if ar > area {
                    area = ar;
                    best = [i, j, k];
                }
            }
        }
    }
    best
}

/// Orthonormal frame (columns) and origin of a node triangle.
fn tri_frame(nodes: impl Fn(usize) -> Vec3, [i, j, k]: [usize; 3]) -> (Mat3, Vec3) {
    let o = nodes(i);
    let e1 = (nodes(j) - o).normalize_or_zero();
    let e3 = e1.cross(nodes(k) - o).normalize_or_zero();
    let e2 = e3.cross(e1);
    (Mat3::from_cols(e1, e2, e3), o)
}

/// Rigid transform `x' = R x + t` taking a triangle's rest frame to its current frame.
fn fit_frame(rest: &[Vec3], cur: &[[f32; 3]], tri: [usize; 3]) -> (Mat3, Vec3) {
    let (ma, oa) = tri_frame(|i| rest[i], tri);
    let (mb, ob) = tri_frame(|i| Vec3::from(cur[i]), tri);
    let r = mb * ma.transpose();
    (r, ob - r * oa)
}
