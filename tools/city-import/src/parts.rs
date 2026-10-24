//! Greybox parts (ashpeak-port; lead, 2026-10-24): Port Vesper ships once.
//!
//! The `vesper_v2` and `ashpeak` greyboxes are written as parts listed in a small manifest
//! (`<id>.greybox.parts.json`, `{"parts": [...]}`), which the browser loads together
//! (`apps/testbed-web/src/scene.ts`):
//!
//! - `vesper_v2` = `vesper_v2.shared` (everything `ashpeak` draws exactly the same way, by
//!   [`crate::dedup::Partition`]) + `vesper_v2.own` (its terrain-bound materials and the cells
//!   the county changes);
//! - `ashpeak` = `vesper_v2.shared` + `ashpeak.county<N>` (everything the county owns, cut
//!   into tiles of at most [`MAX_TILE_GLB_BYTES`] by halving along the longer side at the
//!   median item).
//!
//! The parts add up to the monolithic greyboxes triangle for triangle (`tests/greybox_parts.rs`);
//! the monolithic `.greybox.glb` files stay for the native tools, while the browser and the
//! preview read the parts (`.greybox.glb` per part, `.greybox.b64.txt` for the published build).

use std::path::Path;

use wp_world::geom::V2;
use wp_world::vesper::ashpeak::{self, plan};
use wp_world::vesper::{self, VesperConfig};
use wp_world::CityMap;

use crate::dedup::{drawn, partition, Partition};
use crate::glb::{build_greybox, build_instanced, write_glb_instanced, Instanced, Prim};

/// Largest county tile, bytes of `.glb` (about 12 MB as base64: under the preview's 14 MB
/// split threshold, so no tile is split again there).
pub const MAX_TILE_GLB_BYTES: usize = 9_000_000;

/// One part: its file stem (`<stem>.greybox.glb`) and bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    /// File stem, e.g. `vesper_v2.shared`.
    pub stem: String,
    /// The part's `.glb` bytes.
    pub glb: Vec<u8>,
}

/// Every part of both maps and the two manifests (city id, part stems in load order).
#[derive(Debug, Clone, PartialEq)]
pub struct GreyboxParts {
    /// `vesper_v2.shared`, `vesper_v2.own`, `ashpeak.county0`, ….
    pub parts: Vec<Part>,
    /// `(city id, part stems)`: `vesper_v2` and `ashpeak`.
    pub manifests: Vec<(String, Vec<String>)>,
}

/// Centroid (XY) of triangle `t` of `p`.
fn centroid(p: &Prim, t: &[u32]) -> V2 {
    let v = |i: u32| p.positions[i as usize];
    let (a, b, c) = (v(t[0]), v(t[1]), v(t[2]));
    [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0]
}

/// The triangles of `prims` that `keep(material, centroid)` accepts, with their vertices
/// (and normals) re-indexed; primitives left empty are dropped.
pub fn keep_prims(prims: &[Prim], keep: &dyn Fn(&str, V2) -> bool) -> Vec<Prim> {
    let mut out = Vec::new();
    for p in prims {
        let mut q = Prim {
            material: p.material.clone(),
            color: p.color,
            smooth: p.smooth,
            ..Default::default()
        };
        let mut remap = vec![u32::MAX; p.positions.len()];
        for t in p.indices.chunks(3) {
            if !keep(&p.material, centroid(p, t)) {
                continue;
            }
            for &i in t {
                let r = &mut remap[i as usize];
                if *r == u32::MAX {
                    *r = q.positions.len() as u32;
                    q.positions.push(p.positions[i as usize]);
                    if let Some(n) = p.normals.get(i as usize) {
                        q.normals.push(*n);
                    }
                }
                q.indices.push(*r);
            }
        }
        if !q.indices.is_empty() {
            out.push(q);
        }
    }
    out
}

/// The instances of `inst` that `keep("inst:<name>", position)` accepts; meshes left without
/// instances are dropped.
pub fn keep_instanced(inst: &[Instanced], keep: &dyn Fn(&str, V2) -> bool) -> Vec<Instanced> {
    inst.iter()
        .filter_map(|m| {
            let key = format!("inst:{}", m.name);
            let instances: Vec<_> = m
                .instances
                .iter()
                .filter(|(pos, _, _)| keep(&key, [pos[0], pos[1]]))
                .copied()
                .collect();
            (!instances.is_empty()).then(|| Instanced {
                name: m.name.clone(),
                parts: m.parts.clone(),
                instances,
            })
        })
        .collect()
}

/// A half-open box `[lo, hi)` on XY.
type Region = (V2, V2);

fn in_region(r: &Region, c: V2) -> bool {
    c[0] >= r.0[0] && c[0] < r.1[0] && c[1] >= r.0[1] && c[1] < r.1[1]
}

/// Cut `prims` + `inst` into tiles of at most [`MAX_TILE_GLB_BYTES`]: a region whose `.glb` is
/// larger is halved along its longer side at the median item coordinate (deterministic).
fn tiles(prims: &[Prim], inst: &[Instanced], region: Region, depth: u32, out: &mut Vec<Vec<u8>>) {
    let p = keep_prims(prims, &|_, c| in_region(&region, c));
    let i = keep_instanced(inst, &|_, c| in_region(&region, c));
    let glb = write_glb_instanced(&p, &i, "ashpeak county");
    if glb.len() <= MAX_TILE_GLB_BYTES || depth >= 8 {
        if !p.is_empty() || !i.is_empty() {
            out.push(glb);
        }
        return;
    }
    // Item centroids in the region, to pick the side and the median.
    let mut pts: Vec<V2> = Vec::new();
    for q in &p {
        for t in q.indices.chunks(3) {
            pts.push(centroid(q, t));
        }
    }
    for m in &i {
        pts.extend(m.instances.iter().map(|(pos, _, _)| [pos[0], pos[1]]));
    }
    let span = |k: usize| {
        let (lo, hi) = pts
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), c| {
                (lo.min(c[k]), hi.max(c[k]))
            });
        hi - lo
    };
    let axis = if span(0) >= span(1) { 0 } else { 1 };
    let mut vals: Vec<f32> = pts.iter().map(|c| c[axis]).collect();
    vals.sort_by(f32::total_cmp);
    let cut = vals[vals.len() / 2];
    let (mut a, mut b) = (region, region);
    a.1[axis] = cut;
    b.0[axis] = cut;
    tiles(&p, &i, a, depth + 1, out);
    tiles(&p, &i, b, depth + 1, out);
}

/// Build every part from the two generated maps (deterministic; nothing is stored).
pub fn build_parts() -> GreyboxParts {
    let v = CityMap::new(vesper::generate(&VesperConfig::default()));
    let a = CityMap::new(ashpeak::generate(&ashpeak::default_config()));
    build_parts_from(&v, &a)
}

/// [`build_parts`] from maps already generated (`vesper` = `vesper_v2`, `ashpeak`).
pub fn build_parts_from(vesper: &CityMap, ashpeak: &CityMap) -> GreyboxParts {
    let p: Partition = partition(
        &drawn(vesper),
        &drawn(ashpeak),
        plan::VESPER_LO,
        plan::VESPER_HI,
    );
    let (vp, vi) = (build_greybox(vesper), build_instanced(vesper));
    let shared = |m: &str, c: V2| !p.county_owns(m, c);
    let own = |m: &str, c: V2| p.county_owns(m, c);
    let mut parts = vec![
        Part {
            stem: format!("{}.shared", vesper::CITY_ID),
            glb: write_glb_instanced(
                &keep_prims(&vp, &shared),
                &keep_instanced(&vi, &shared),
                "vesper_v2 shared",
            ),
        },
        Part {
            stem: format!("{}.own", vesper::CITY_ID),
            glb: write_glb_instanced(
                &keep_prims(&vp, &own),
                &keep_instanced(&vi, &own),
                "vesper_v2 own",
            ),
        },
    ];
    let (ap, ai) = (build_greybox(ashpeak), build_instanced(ashpeak));
    let county = |m: &str, c: V2| p.county_owns(m, c);
    let (cp, ci) = (keep_prims(&ap, &county), keep_instanced(&ai, &county));
    let all: Region = ([f32::NEG_INFINITY; 2], [f32::INFINITY; 2]);
    let mut county_glbs = Vec::new();
    tiles(&cp, &ci, all, 0, &mut county_glbs);
    let mut ashpeak_parts = vec![parts[0].stem.clone()];
    for (k, glb) in county_glbs.into_iter().enumerate() {
        let stem = format!("{}.county{k}", ashpeak::CITY_ID);
        ashpeak_parts.push(stem.clone());
        parts.push(Part { stem, glb });
    }
    GreyboxParts {
        manifests: vec![
            (
                vesper::CITY_ID.to_string(),
                vec![parts[0].stem.clone(), parts[1].stem.clone()],
            ),
            (ashpeak::CITY_ID.to_string(), ashpeak_parts),
        ],
        parts,
    }
}

/// A manifest's JSON: `{"parts": ["vesper_v2.shared", …]}`.
pub fn manifest_json(stems: &[String]) -> String {
    let list: Vec<String> = stems.iter().map(|s| format!("\"{s}\"")).collect();
    format!("{{\"parts\": [{}]}}\n", list.join(", "))
}

/// Write every part (`<stem>.greybox.glb` + `.greybox.b64.txt`) and both manifests into `dir`,
/// remove county tiles left over from an earlier, larger split and the monolithic `.b64.txt`
/// of both maps (the browser reads the parts). Returns `(stem, glb bytes)` per part.
pub fn write_greybox_parts(dir: &Path) -> std::io::Result<Vec<(String, usize)>> {
    let gp = build_parts();
    for entry in std::fs::read_dir(dir)? {
        let name = entry?.file_name().to_string_lossy().to_string();
        if name.starts_with(&format!("{}.county", ashpeak::CITY_ID)) {
            std::fs::remove_file(dir.join(&name))?;
        }
    }
    for id in [vesper::CITY_ID, ashpeak::CITY_ID] {
        let _ = std::fs::remove_file(dir.join(format!("{id}.greybox.b64.txt")));
    }
    let mut out = Vec::new();
    for p in &gp.parts {
        std::fs::write(dir.join(format!("{}.greybox.glb", p.stem)), &p.glb)?;
        std::fs::write(
            dir.join(format!("{}.greybox.b64.txt", p.stem)),
            crate::base64(&p.glb),
        )?;
        out.push((p.stem.clone(), p.glb.len()));
    }
    for (id, stems) in &gp.manifests {
        std::fs::write(
            dir.join(format!("{id}.greybox.parts.json")),
            manifest_json(stems),
        )?;
    }
    Ok(out)
}
