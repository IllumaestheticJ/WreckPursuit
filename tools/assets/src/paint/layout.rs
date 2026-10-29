//! Detail-texture layout: the second UV set (`MeshPart::duvs`) of a painted car.
//!
//! Every face is projected along one of six views ([`View`], picked from its normal with the
//! same thresholds as the livery atlas bands, so paint parts split their vertices in the same
//! places). The lofted body (`livery` parts) shares one car-wide frame per view, so neighbouring
//! panels sit side by side in the texture and the painter can draw gaps where they meet and
//! features that cross panels; every other textured part gets its own island per view; all
//! tires and rims share one wheel island (hub space, the rim face seen from outboard). Islands are
//! metric (the same px / m everywhere, lamps and wheels at [`DENSE`] times that) and shelf-packed
//! into the texture at the largest scale that fits. The first [`NEUTRAL_PX`]² texels are a
//! neutral block for parts without a detail UV (mods).

use std::collections::BTreeMap;

use crate::mesh::{MaterialSlot, MeshPart, PartKind};

/// Texture size (square), px.
pub const TEX_SIZE: u32 = 1024;
/// Padding round every island, px (keeps mip levels from bleeding across islands).
pub const PAD_PX: u32 = 3;
/// Side of the neutral block at (0, 0), px.
pub const NEUTRAL_PX: u32 = 8;
/// Density factor of lamps and wheels against the body.
pub const DENSE: f32 = 2.0;
/// Density factor of the underside and small trim islands.
pub const SPARSE: f32 = 0.5;

/// Projection of a face (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum View {
    /// Seen from the left (−X): (a, b) = (−y, −z).
    Left,
    /// Seen from the right: (y, −z).
    Right,
    /// Seen from above: (−y, x).
    Top,
    /// Seen from ahead: (−x, −z).
    Front,
    /// Seen from behind: (x, −z).
    Rear,
    /// Seen from below: (−y, −x).
    Bottom,
    /// Wheel island, hub space seen from outboard: (y, −z).
    Wheel,
}

impl View {
    /// (a, b) view coordinates of a point (a to the right, b down, as the viewer sees it), m.
    pub fn ab(self, q: [f32; 3]) -> [f32; 2] {
        match self {
            View::Left => [-q[1], -q[2]],
            View::Right => [q[1], -q[2]],
            View::Top => [-q[1], q[0]],
            View::Front => [-q[0], -q[2]],
            View::Rear => [q[0], -q[2]],
            View::Bottom => [-q[1], -q[0]],
            View::Wheel => [q[1], -q[2]],
        }
    }

    /// The view of a face with unit normal `n` and centroid x `cx` (the livery atlas rule plus a
    /// bottom view for faces facing down).
    pub fn of_face(n: [f32; 3], cx: f32) -> View {
        if n[2] > 0.55 {
            View::Top
        } else if n[1].abs() > n[0].abs() && n[1].abs() > 0.5 {
            if n[1] > 0.0 {
                View::Front
            } else {
                View::Rear
            }
        } else if n[2] < -0.75 {
            View::Bottom
        } else if n[0] < -0.05 || (n[0].abs() <= 0.05 && cx < 0.0) {
            View::Left
        } else {
            View::Right
        }
    }
}

/// What an island holds.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum IslandKey {
    /// The car-wide frame of the lofted body in a view.
    Frame(View),
    /// One part's faces in one view.
    Part(String, View),
    /// Every tire and rim (hub space).
    Wheel,
}

/// One packed island: its view-space box and where it sits in the texture.
#[derive(Debug, Clone, PartialEq)]
pub struct Island {
    /// What it holds.
    pub key: IslandKey,
    /// View used to project.
    pub view: View,
    /// View-space box (a0, b0, a1, b1), m.
    pub ab: [f32; 4],
    /// Texture origin (px, top-left of the box).
    pub px: [u32; 2],
    /// Texture size (px).
    pub size: [u32; 2],
    /// Scale, px per m.
    pub scale: f32,
}

impl Island {
    /// Texture coordinates (px, fractional) of a view-space point, clamped into the island.
    pub fn to_px(&self, ab: [f32; 2]) -> [f32; 2] {
        let u = ((ab[0] - self.ab[0]) * self.scale).clamp(0.0, self.size[0] as f32);
        let v = ((ab[1] - self.ab[1]) * self.scale).clamp(0.0, self.size[1] as f32);
        [self.px[0] as f32 + u, self.px[1] as f32 + v]
    }

    /// View-space point of a texel centre (px, py) of this island.
    pub fn to_ab(&self, px: u32, py: u32) -> [f32; 2] {
        [
            self.ab[0] + (px as f32 + 0.5 - self.px[0] as f32) / self.scale,
            self.ab[1] + (py as f32 + 0.5 - self.px[1] as f32) / self.scale,
        ]
    }
}

/// The detail layout of a car (computed once from the full mesh, reused by the mid LOD).
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// Texture size, px.
    pub size: u32,
    /// Islands by key.
    pub islands: BTreeMap<IslandKey, Island>,
}

/// Whether a part gets a detail UV: exterior parts (panels, lights, mirrors, details, tires,
/// rims, painted body) — not glass, interior, engine bay, suspension / exhaust, brakes, decal
/// carriers or black body trim (seals, underbody, pillar returns).
pub fn textured(p: &MeshPart) -> bool {
    match p.kind {
        PartKind::Panel
        | PartKind::Light
        | PartKind::Mirror
        | PartKind::Detail
        | PartKind::Tire
        | PartKind::Rim
        | PartKind::Lightbar => p.material != MaterialSlot::Glass,
        PartKind::Body => p.material.is_paint(),
        _ => false,
    }
}

/// The car-wide frame holds the lofted body: `livery` parts that are panels or painted body.
fn in_frame(p: &MeshPart) -> bool {
    p.material == MaterialSlot::Livery && matches!(p.kind, PartKind::Panel | PartKind::Body)
}

/// Density factor of an island key.
fn density(key: &IslandKey, parts: &BTreeMap<&str, &MeshPart>) -> f32 {
    match key {
        IslandKey::Wheel => DENSE,
        IslandKey::Frame(View::Bottom) => SPARSE,
        IslandKey::Frame(_) => 1.0,
        IslandKey::Part(name, view) => {
            let p = parts.get(name.as_str());
            if p.is_some_and(|p| p.kind == PartKind::Light) {
                DENSE
            } else if *view == View::Bottom {
                SPARSE
            } else {
                1.0
            }
        }
    }
}

/// Unit normal and centroid of a triangle (positions in mm).
fn face(p: &MeshPart, t: &[u32]) -> ([f32; 3], [f32; 3]) {
    let v = |i: u32| p.vertex(i as usize);
    let (a, b, c) = (v(t[0]), v(t[1]), v(t[2]));
    let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
    let cen = [
        (a[0] + b[0] + c[0]) / 3.0,
        (a[1] + b[1] + c[1]) / 3.0,
        (a[2] + b[2] + c[2]) / 3.0,
    ];
    ([n[0] / l, n[1] / l, n[2] / l], cen)
}

/// The island a face of part `p` falls in (`None` for untextured parts).
pub fn island_of(p: &MeshPart, n: [f32; 3], cx: f32) -> Option<IslandKey> {
    if !textured(p) {
        return None;
    }
    if p.wheel.is_some() {
        return Some(IslandKey::Wheel);
    }
    let view = View::of_face(n, cx);
    Some(if in_frame(p) {
        IslandKey::Frame(view)
    } else {
        IslandKey::Part(p.name.clone(), view)
    })
}

/// View of an island key.
pub fn view_of(key: &IslandKey) -> View {
    match key {
        IslandKey::Frame(v) | IslandKey::Part(_, v) => *v,
        IslandKey::Wheel => View::Wheel,
    }
}

impl Layout {
    /// Islands of the full mesh's textured parts, packed into a [`TEX_SIZE`]² texture.
    pub fn build(parts: &[MeshPart]) -> Layout {
        let mut boxes: BTreeMap<IslandKey, [f32; 4]> = BTreeMap::new();
        let by_name: BTreeMap<&str, &MeshPart> =
            parts.iter().map(|p| (p.name.as_str(), p)).collect();
        for p in parts {
            for t in p.indices.chunks_exact(3) {
                let (n, c) = face(p, t);
                let Some(key) = island_of(p, n, c[0]) else {
                    continue;
                };
                let view = view_of(&key);
                let bx = boxes
                    .entry(key)
                    .or_insert([f32::MAX, f32::MAX, f32::MIN, f32::MIN]);
                for &i in t {
                    let ab = view.ab(p.vertex(i as usize));
                    bx[0] = bx[0].min(ab[0]);
                    bx[1] = bx[1].min(ab[1]);
                    bx[2] = bx[2].max(ab[0]);
                    bx[3] = bx[3].max(ab[1]);
                }
            }
        }
        // The wheel island: square, centred on the hub, the largest tire's radius.
        if let Some(bx) = boxes.get_mut(&IslandKey::Wheel) {
            let r = bx.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            *bx = [-r, -r, r, r];
        }
        let keys: Vec<(IslandKey, [f32; 4], f32)> = boxes
            .into_iter()
            .map(|(k, b)| {
                let d = density(&k, &by_name);
                (k, b, d)
            })
            .collect();
        // Largest scale (px / m at density 1) whose shelf packing fits: bisection.
        let (mut lo, mut hi) = (20.0f32, 600.0f32);
        for _ in 0..24 {
            let mid = 0.5 * (lo + hi);
            if pack(&keys, mid, TEX_SIZE).is_some() {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let islands = pack(&keys, lo, TEX_SIZE).unwrap_or_default();
        Layout {
            size: TEX_SIZE,
            islands,
        }
    }

    /// Body texel size of the layout, m per px.
    pub fn body_m_per_px(&self) -> f32 {
        self.islands
            .get(&IslandKey::Frame(View::Left))
            .map_or(0.0, |i| 1.0 / i.scale)
    }

    /// Detail UVs (1/65535) of a part's vertices, splitting vertices whose faces fall in
    /// different islands (positions, livery UVs and skin data are duplicated). No-op (returns
    /// false) for untextured parts. Faces of a part whose island the layout lacks (a mid-LOD face
    /// turned into another view) use the part's first island; coordinates clamp into it.
    pub fn assign(&self, p: &mut MeshPart) -> bool {
        if !textured(p) {
            return false;
        }
        let fallback = self
            .islands
            .iter()
            .find(|(k, _)| matches!(k, IslandKey::Part(n, _) if *n == p.name))
            .map(|(_, i)| i.clone());
        let n_v = p.vertex_count();
        let mut map: BTreeMap<(u32, usize), u32> = BTreeMap::new();
        let mut keys: Vec<IslandKey> = Vec::new();
        let mut src: Vec<(u32, usize)> = Vec::new();
        let mut indices = Vec::with_capacity(p.indices.len());
        let tris: Vec<[u32; 3]> = p
            .indices
            .chunks_exact(3)
            .map(|t| [t[0], t[1], t[2]])
            .collect();
        for t in &tris {
            let (n, c) = face(p, t);
            let key = island_of(p, n, c[0]).expect("textured");
            let ki = keys.iter().position(|k| *k == key).unwrap_or_else(|| {
                keys.push(key.clone());
                keys.len() - 1
            });
            for &v in t {
                let id = *map.entry((v, ki)).or_insert_with(|| {
                    src.push((v, ki));
                    (src.len() - 1) as u32
                });
                indices.push(id);
            }
        }
        let islands: Vec<Option<Island>> = keys
            .iter()
            .map(|k| self.islands.get(k).cloned().or_else(|| fallback.clone()))
            .collect();
        let mut pos = Vec::with_capacity(src.len() * 3);
        let mut uvs = Vec::with_capacity(src.len() * 2);
        let mut duvs = Vec::with_capacity(src.len() * 2);
        let mut nodes = Vec::new();
        let mut weights = Vec::new();
        let skinned = p.skin_nodes.len() == n_v * 4;
        for &(v, ki) in &src {
            let v = v as usize;
            pos.extend_from_slice(&p.positions_mm[v * 3..v * 3 + 3]);
            uvs.extend_from_slice(&p.uvs[v * 2..v * 2 + 2]);
            if skinned {
                nodes.extend_from_slice(&p.skin_nodes[v * 4..v * 4 + 4]);
                weights.extend_from_slice(&p.skin_weights[v * 4..v * 4 + 4]);
            }
            let uv = match &islands[ki] {
                Some(isl) => {
                    let q = isl.to_px(isl.view.ab(p.vertex(v)));
                    [q[0] / self.size as f32, q[1] / self.size as f32]
                }
                None => {
                    let c = 0.5 * NEUTRAL_PX as f32 / self.size as f32;
                    [c, c]
                }
            };
            for t in uv {
                duvs.push((t.clamp(0.0, 1.0) * 65535.0).round() as u16);
            }
        }
        p.positions_mm = pos;
        p.uvs = uvs;
        p.indices = indices;
        if skinned {
            p.skin_nodes = nodes;
            p.skin_weights = weights;
        }
        p.duvs = duvs;
        true
    }
}

/// Shelf-pack `keys` (key, view box, density) at `scale` px / m into a `size`² texture: islands
/// sorted by height (then key), rows left to right below the neutral block. `None` if they do
/// not fit.
fn pack(
    keys: &[(IslandKey, [f32; 4], f32)],
    scale: f32,
    size: u32,
) -> Option<BTreeMap<IslandKey, Island>> {
    let mut items: Vec<(&IslandKey, [f32; 4], f32, [u32; 2])> = keys
        .iter()
        .map(|(k, b, d)| {
            let s = scale * d;
            let w = ((b[2] - b[0]).max(0.0) * s).ceil() as u32 + 1;
            let h = ((b[3] - b[1]).max(0.0) * s).ceil() as u32 + 1;
            (k, *b, s, [w, h])
        })
        .collect();
    items.sort_by(|a, b| b.3[1].cmp(&a.3[1]).then(a.0.cmp(b.0)));
    let mut out = BTreeMap::new();
    let (mut x, mut y, mut row_h) = (NEUTRAL_PX + PAD_PX, PAD_PX, 0u32);
    for (k, b, s, [w, h]) in items {
        if w + 2 * PAD_PX > size {
            return None;
        }
        if x + w + PAD_PX > size {
            x = PAD_PX;
            y += row_h + PAD_PX;
            row_h = 0;
        }
        if y + h + PAD_PX > size {
            return None;
        }
        out.insert(
            k.clone(),
            Island {
                key: k.clone(),
                view: view_of(k),
                ab: b,
                px: [x, y],
                size: [w - 1, h - 1],
                scale: s,
            },
        );
        x += w + PAD_PX;
        row_h = row_h.max(h);
    }
    Some(out)
}
