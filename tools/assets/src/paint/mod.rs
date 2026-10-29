//! Code-painted detail textures (painted-car pipeline, PS2-era look): every painted car gets a
//! per-car RGBA texture `data/panels/textures/<car>.png`, drawn by code from its mesh, so the
//! silhouette stays in the geometry and the detail (lamp internals, grilles, intakes, intercooler,
//! badges, plates, handles, fuel door, vents, panel gaps, wheel spokes, baked shading) is texture.
//!
//! Pipeline ([`paint_mesh`]): [`layout::Layout`] gives every textured part a second UV set
//! (`MeshPart::duvs`); the full mesh is rasterised into texture space (part, position and normal
//! per texel); [`features`] colours each texel from the car's section spec and trim (its
//! `texture_layers`); panel gaps are drawn where neighbouring texels of one car-wide frame belong
//! to different panels; ambient occlusion is baked from the rest mesh ([`bake`]); faint noise;
//! islands are dilated into their padding. Deterministic: same mesh ⇒ same PNG bytes.
//!
//! Shader contract (renderer): `albedo = tex.rgb · mix(1, paint, tex.a)` — A is the paint mask
//! (1 on body paint, where RGB is the baked shading multiplier; 0 on trim, lamps, grilles,
//! badges, plates, wheels, where RGB is the colour); lamps glow `tex.rgb · level`.
//!
//! - [`canvas`]: RGBA canvas, PNG writer, pixel font, hash noise.
//! - [`layout`]: views, islands, packing, `duvs`.
//! - [`bake`]: BVH + ambient occlusion.
//! - [`features`]: the per-family texel painters.

pub mod bake;
pub mod canvas;
pub mod features;
pub mod layout;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

use wp_contracts::VehicleDef;

use crate::mesh::{DetailTexture, MeshPart};
use crate::profile::BodyProfile;
use canvas::{Canvas, Rgba};
use layout::{IslandKey, Layout, View, NEUTRAL_PX};

/// Strength of the baked ambient occlusion on body paint (0 = none, 1 = full).
pub const AO_STRENGTH: f32 = 0.85;
/// AO sample spacing, texels (AO is computed on this grid and reused by the texels between).
pub const AO_STRIDE: u32 = 2;
/// Amplitude of the faint per-texel noise (keeps the PNG small at this size).
pub const NOISE: f32 = 0.012;

/// One rasterised texel: the part it shows, its island, rest position and face normal.
#[derive(Debug, Clone, Copy)]
pub struct Texel {
    /// Index into the parts + 1 (0 = empty).
    pub part: u32,
    /// Index into the layout's islands (BTreeMap order).
    pub island: u32,
    /// Position (chassis space; hub space for wheel parts), m.
    pub pos: [f32; 3],
    /// Face normal (same space).
    pub n: [f32; 3],
}

/// The painted texture of a car: PNG bytes and the mesh reference to them.
#[derive(Debug, Clone, PartialEq)]
pub struct Painted {
    /// PNG file bytes.
    pub png: Vec<u8>,
    /// The mesh's `detail` field.
    pub detail: DetailTexture,
}

/// File of a car's detail texture under `data/panels/`.
pub fn texture_file(id: &str) -> String {
    format!("textures/{id}.png")
}

/// Paint the texture of a car from its full mesh parts (with `duvs` from `layout`). Cached per
/// process by a hash of the parts (the mid LOD and tests reuse the full mesh's texture).
pub fn paint_mesh(
    def: &VehicleDef,
    prof: &BodyProfile,
    layout: &Layout,
    parts: &[MeshPart],
) -> Arc<Painted> {
    static CACHE: OnceLock<Mutex<BTreeMap<String, Arc<Painted>>>> = OnceLock::new();
    let key = format!("{}:{}", def.id, parts_hash(parts));
    let cache = CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Some(p) = cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return p;
    }
    let canvas = paint(def, prof, layout, parts);
    let png = canvas::png_rgba8(canvas.size, canvas.size, &canvas.to_rgba8());
    let detail = DetailTexture {
        file: texture_file(&def.id),
        w: canvas.size,
        h: canvas.size,
        hash: canvas::fnv1a_hex(&png),
    };
    let out = Arc::new(Painted { png, detail });
    if let Ok(mut c) = cache.lock() {
        c.insert(key, out.clone());
    }
    out
}

/// FNV-1a over the parts' names, positions, indices and detail UVs (the painter's input).
fn parts_hash(parts: &[MeshPart]) -> String {
    let mut bytes = Vec::new();
    for p in parts {
        bytes.extend_from_slice(p.name.as_bytes());
        for v in &p.positions_mm {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        for v in &p.indices {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        for v in &p.duvs {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
    }
    canvas::fnv1a_hex(&bytes)
}

/// Rasterise the parts' faces into texture space (part, island, position, normal per texel).
pub fn rasterise(layout: &Layout, parts: &[MeshPart]) -> Vec<Texel> {
    let size = layout.size;
    let mut tex = vec![
        Texel {
            part: 0,
            island: 0,
            pos: [0.0; 3],
            n: [0.0; 3],
        };
        (size * size) as usize
    ];
    let island_index: BTreeMap<&IslandKey, u32> = layout
        .islands
        .keys()
        .enumerate()
        .map(|(i, k)| (k, i as u32))
        .collect();
    for (pi, p) in parts.iter().enumerate() {
        if p.duvs.len() != p.vertex_count() * 2 {
            continue;
        }
        for t in p.indices.chunks_exact(3) {
            let q = [0, 1, 2].map(|k| p.vertex(t[k] as usize));
            let uv = [0, 1, 2].map(|k| {
                let v = t[k] as usize;
                [
                    p.duvs[v * 2] as f32 / 65535.0 * size as f32,
                    p.duvs[v * 2 + 1] as f32 / 65535.0 * size as f32,
                ]
            });
            let e1 = [q[1][0] - q[0][0], q[1][1] - q[0][1], q[1][2] - q[0][2]];
            let e2 = [q[2][0] - q[0][0], q[2][1] - q[0][1], q[2][2] - q[0][2]];
            let mut n = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if l < 1e-12 {
                continue;
            }
            n = [n[0] / l, n[1] / l, n[2] / l];
            let cx = (q[0][0] + q[1][0] + q[2][0]) / 3.0;
            let Some(key) = layout::island_of(p, n, cx) else {
                continue;
            };
            let island = island_index.get(&key).copied().unwrap_or(0);
            let (x0, x1) = (
                uv.iter()
                    .map(|a| a[0])
                    .fold(f32::MAX, f32::min)
                    .floor()
                    .max(0.0) as u32,
                uv.iter()
                    .map(|a| a[0])
                    .fold(f32::MIN, f32::max)
                    .ceil()
                    .min(size as f32) as u32,
            );
            let (y0, y1) = (
                uv.iter()
                    .map(|a| a[1])
                    .fold(f32::MAX, f32::min)
                    .floor()
                    .max(0.0) as u32,
                uv.iter()
                    .map(|a| a[1])
                    .fold(f32::MIN, f32::max)
                    .ceil()
                    .min(size as f32) as u32,
            );
            let area = (uv[1][0] - uv[0][0]) * (uv[2][1] - uv[0][1])
                - (uv[2][0] - uv[0][0]) * (uv[1][1] - uv[0][1]);
            if area.abs() < 1e-9 {
                continue;
            }
            for y in y0..y1 {
                for x in x0..x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w1 = ((px - uv[0][0]) * (uv[2][1] - uv[0][1])
                        - (uv[2][0] - uv[0][0]) * (py - uv[0][1]))
                        / area;
                    let w2 = ((uv[1][0] - uv[0][0]) * (py - uv[0][1])
                        - (px - uv[0][0]) * (uv[1][1] - uv[0][1]))
                        / area;
                    let w0 = 1.0 - w1 - w2;
                    let eps = -1e-4;
                    if w0 < eps || w1 < eps || w2 < eps {
                        continue;
                    }
                    let pos = [0, 1, 2].map(|k| w0 * q[0][k] + w1 * q[1][k] + w2 * q[2][k]);
                    tex[(y * size + x) as usize] = Texel {
                        part: pi as u32 + 1,
                        island,
                        pos,
                        n,
                    };
                }
            }
        }
    }
    tex
}

/// Panel group of a part for gaps: its region for panels and painted sills, `None` for the
/// cabin body (pillars, roof rails: welded to the roof, no shut line). On a 2-door car the side
/// strip between the door and the quarter (lattice region `*DoorR`) is part of the quarter
/// panel: no shut line between them.
fn gap_group(p: &MeshPart, two_door: bool) -> Option<wp_contracts::BodyRegion> {
    use crate::mesh::PartKind;
    use wp_contracts::BodyRegion as R;
    let r = match (p.kind, p.region) {
        (PartKind::Panel, Some(r)) => r,
        (PartKind::Body, Some(r)) if r != R::Cabin => r,
        _ => return None,
    };
    Some(match r {
        R::LeftDoorR if two_door => R::RearLeftQuarter,
        R::RightDoorR if two_door => R::RearRightQuarter,
        r => r,
    })
}

/// True when the car has no rear doors (no rear door glass part).
pub fn is_two_door(parts: &[MeshPart]) -> bool {
    !parts.iter().any(|p| p.name.starts_with("door_rl_glass"))
}

/// Paint the texture (see the module docs).
pub fn paint(def: &VehicleDef, prof: &BodyProfile, layout: &Layout, parts: &[MeshPart]) -> Canvas {
    let size = layout.size;
    let tex = rasterise(layout, parts);
    let islands: Vec<&layout::Island> = layout.islands.values().collect();
    let mut c = Canvas::new(size, [1.0, 1.0, 1.0, 1.0]);
    let spec = prof.spec();
    let trim = spec.map(|s| s.trim(&s.active_trim)).unwrap_or_default();
    let env = features::Env {
        def,
        prof,
        trim: &trim,
        family: spec.map_or("", |s| s.family.as_str()),
        trim_key: spec.map_or("", |s| s.active_trim.as_str()),
    };
    // AO occluders: every part at rest, wheels at their hubs; glass and interior included.
    let mut tris = Vec::new();
    for p in parts {
        let off = p
            .wheel
            .and_then(|w| def.wheels.get(w as usize))
            .map_or([0.0; 3], |w| w.position_m);
        for t in p.indices.chunks_exact(3) {
            let q = [0, 1, 2].map(|k| {
                let v = p.vertex(t[k] as usize);
                [v[0] + off[0], v[1] + off[1], v[2] + off[2]]
            });
            tris.push(q);
        }
    }
    let bvh = bake::Bvh::new(tris);
    let ao_at = |t: &Texel, x: u32, y: u32| -> f32 {
        let seed = canvas::hash01(x, y, 7);
        bake::ao(&bvh, t.pos, t.n, seed)
    };
    // AO on a coarse grid: each texel takes the AO of its grid corner when that texel shows the
    // same part, else its own.
    let mut ao = vec![1.0f32; (size * size) as usize];
    for y in 0..size {
        for x in 0..size {
            let i = (y * size + x) as usize;
            let t = &tex[i];
            if t.part == 0 {
                continue;
            }
            let p = &parts[t.part as usize - 1];
            if p.wheel.is_some() || p.kind == crate::mesh::PartKind::Light {
                continue;
            }
            let (gx, gy) = (x - x % AO_STRIDE, y - y % AO_STRIDE);
            let gi = (gy * size + gx) as usize;
            ao[i] = if gi != i && tex[gi].part == t.part && gi < i {
                ao[gi]
            } else {
                ao_at(t, x, y)
            };
        }
    }
    for y in 0..size {
        for x in 0..size {
            let i = (y * size + x) as usize;
            let t = &tex[i];
            if t.part == 0 {
                continue;
            }
            let p = &parts[t.part as usize - 1];
            let isl = islands[t.island as usize];
            let ctx = features::Ctx {
                part: p,
                view: isl.view,
                frame: matches!(isl.key, IslandKey::Frame(_)),
                p: t.pos,
                n: t.n,
                ab: isl.view.ab(t.pos),
                m_per_px: 1.0 / isl.scale,
                px: [x, y],
            };
            let mut col = features::texel(&env, &ctx);
            // Baked AO darkens body paint (as a shading multiplier) and trim alike.
            let a = 1.0 - AO_STRENGTH * (1.0 - ao[i]);
            for ch in col.iter_mut().take(3) {
                *ch *= a;
            }
            // Faint noise (stable per texel).
            let nz = (canvas::hash01(x, y, 3) - 0.5) * 2.0 * NOISE;
            for ch in col.iter_mut().take(3) {
                *ch = (*ch + nz).max(0.0);
            }
            c.px[i] = col;
        }
    }
    gaps(&mut c, &tex, parts, &islands, size);
    dilate(&mut c, &tex, size, 3);
    for y in 0..NEUTRAL_PX {
        for x in 0..NEUTRAL_PX {
            let i = c.idx(x, y);
            c.px[i] = [1.0, 1.0, 1.0, 1.0];
        }
    }
    c
}

/// Panel gaps: inside a car-wide frame, a texel whose right / lower neighbour shows another
/// panel group is the dark shut line; the texel on the far side gets a soft shadow.
fn gaps(c: &mut Canvas, tex: &[Texel], parts: &[MeshPart], islands: &[&layout::Island], size: u32) {
    let line = gap_mask(tex, parts, islands, size);
    for (i, &l) in line.iter().enumerate() {
        match l {
            2 => c.px[i] = GAP,
            1 => {
                for ch in c.px[i].iter_mut().take(3) {
                    *ch *= 0.62;
                }
            }
            _ => {}
        }
    }
}

/// Colour of a panel gap texel.
pub const GAP: Rgba = [0.045, 0.045, 0.05, 0.0];

/// Panel gap mask per texel: 2 = shut line (this texel's right or lower neighbour in the same
/// car-wide frame shows another panel group), 1 = the shadow on the far side, 0 = none.
pub fn gap_mask(
    tex: &[Texel],
    parts: &[MeshPart],
    islands: &[&layout::Island],
    size: u32,
) -> Vec<u8> {
    let two_door = is_two_door(parts);
    let group = |i: usize| gap_group_of(tex, parts, islands, i, two_door);
    let mut line = vec![0u8; tex.len()];
    for y in 0..size - 1 {
        for x in 0..size - 1 {
            let i = (y * size + x) as usize;
            let Some((isl, g)) = group(i) else {
                continue;
            };
            for j in [i + 1, i + size as usize] {
                if let Some((isl2, g2)) = group(j) {
                    if isl2 == isl && g2 != g {
                        line[i] = 2;
                        if line[j] == 0 {
                            line[j] = 1;
                        }
                    }
                }
            }
        }
    }
    line
}

/// Group of a texel for gaps (`None` outside a car-wide frame or on the cabin body).
pub fn gap_group_of(
    tex: &[Texel],
    parts: &[MeshPart],
    islands: &[&layout::Island],
    i: usize,
    two_door: bool,
) -> Option<(u32, wp_contracts::BodyRegion)> {
    let t = &tex[i];
    if t.part == 0 || !matches!(islands[t.island as usize].key, IslandKey::Frame(_)) {
        return None;
    }
    gap_group(&parts[t.part as usize - 1], two_door).map(|g| (t.island, g))
}

/// Grow every island `passes` texels into its padding (copying the nearest painted texel), so
/// filtering at island edges never samples the white background.
fn dilate(c: &mut Canvas, tex: &[Texel], size: u32, passes: u32) {
    let mut filled: Vec<bool> = tex.iter().map(|t| t.part != 0).collect();
    for _ in 0..passes {
        let mut next = filled.clone();
        let mut out = c.px.clone();
        for y in 0..size {
            for x in 0..size {
                let i = (y * size + x) as usize;
                if filled[i] {
                    continue;
                }
                let mut acc = [0.0f32; 4];
                let mut n = 0.0;
                for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= size as i32 || ny >= size as i32 {
                        continue;
                    }
                    let j = (ny as u32 * size + nx as u32) as usize;
                    if filled[j] {
                        for (a, v) in acc.iter_mut().zip(c.px[j]) {
                            *a += v;
                        }
                        n += 1.0;
                    }
                }
                if n > 0.0 {
                    out[i] = acc.map(|v| v / n);
                    next[i] = true;
                }
            }
        }
        c.px = out;
        filled = next;
    }
}

/// Assign detail UVs to every textured part (no-op for others).
pub fn assign_duvs(layout: &Layout, parts: &mut [MeshPart]) {
    for p in parts.iter_mut() {
        layout.assign(p);
    }
}

/// Name of a [`View`] (diagnostics).
pub fn view_name(v: View) -> &'static str {
    match v {
        View::Left => "left",
        View::Right => "right",
        View::Top => "top",
        View::Front => "front",
        View::Rear => "rear",
        View::Bottom => "bottom",
        View::Wheel => "wheel",
    }
}
