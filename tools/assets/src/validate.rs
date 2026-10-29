//! `validate-car-asset`: checks a car's mesh and panel spec against its car and lattice.

use std::collections::BTreeSet;

use wp_contracts::{BodyRegion, LoadedCar};
use wp_damage::PanelSpec;

use crate::mesh::{CarMesh, PartKind, MESH_FORMAT, MESH_FORMAT_PLAIN};
use crate::skin::part_candidates;

/// Exterior regions that must have a mesh part whenever the lattice has nodes in them.
pub const EXTERIOR_REGIONS: [BodyRegion; 17] = [
    BodyRegion::FrontBumper,
    BodyRegion::Hood,
    BodyRegion::FrontLeftFender,
    BodyRegion::FrontRightFender,
    BodyRegion::LeftDoorF,
    BodyRegion::LeftDoorR,
    BodyRegion::RightDoorF,
    BodyRegion::RightDoorR,
    BodyRegion::Roof,
    BodyRegion::RearLeftQuarter,
    BodyRegion::RearRightQuarter,
    BodyRegion::Trunk,
    BodyRegion::RearBumper,
    BodyRegion::Windshield,
    BodyRegion::RearGlass,
    BodyRegion::FrameLeft,
    BodyRegion::FrameRight,
];

/// Largest detail texture side, px (1024 is the target; 2048 the cap).
pub const DETAIL_MAX_PX: u32 = 2048;

/// Problems with a mesh's detail texture and UVs (painted cars; for every other car: no
/// `detail`, no `duvs`). The texture must be a square power of two up to [`DETAIL_MAX_PX`]
/// (its file: [`validate_detail_file`]); every textured part
/// ([`crate::paint::layout::textured`]) has one detail UV per vertex, no other part has any.
pub fn validate_detail(car: &LoadedCar, mesh: &CarMesh) -> Vec<String> {
    let mut errs = Vec::new();
    let painted = crate::mesh::is_painted(&crate::style::style_for(&car.def));
    let Some(d) = &mesh.detail else {
        if painted {
            errs.push(format!("{}: painted car without a detail texture", mesh.id));
        }
        if let Some(p) = mesh.parts.iter().find(|p| !p.duvs.is_empty()) {
            errs.push(format!(
                "{}/{}: duvs without a detail texture",
                mesh.id, p.name
            ));
        }
        return errs;
    };
    if !painted {
        errs.push(format!(
            "{}: detail texture on a car that is not painted",
            mesh.id
        ));
    }
    if d.w != d.h || !d.w.is_power_of_two() || d.w > DETAIL_MAX_PX {
        errs.push(format!(
            "{}: detail {}×{} (square power of two ≤ {DETAIL_MAX_PX})",
            mesh.id, d.w, d.h
        ));
    }
    for p in &mesh.parts {
        let want = crate::paint::layout::textured(p);
        if want && p.duvs.len() != p.vertex_count() * 2 {
            errs.push(format!(
                "{}/{}: {} duvs for {} vertices",
                mesh.id,
                p.name,
                p.duvs.len(),
                p.vertex_count()
            ));
        }
        if !want && !p.duvs.is_empty() {
            errs.push(format!("{}/{}: untextured part with duvs", mesh.id, p.name));
        }
    }
    errs
}

/// Problems with the detail texture file a mesh on disk points at: it must exist and match the
/// mesh's `hash`, `w` and `h` (meshes generated with other styles, like the knob sweeps in the
/// tests, are checked by [`validate_detail`] only).
pub fn validate_detail_file(mesh: &CarMesh) -> Vec<String> {
    let mut errs = Vec::new();
    let Some(d) = &mesh.detail else {
        return errs;
    };
    let path = crate::detail_path(&crate::repo_root(), &d.file);
    match std::fs::read(&path) {
        Ok(bytes) => {
            if crate::paint::canvas::fnv1a_hex(&bytes) != d.hash {
                errs.push(format!("{}: {} does not match its hash", mesh.id, d.file));
            }
            if bytes.len() > 24 {
                let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
                let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
                if (w, h) != (d.w, d.h) {
                    errs.push(format!("{}: {} is {w}×{h}", mesh.id, d.file));
                }
            }
        }
        Err(e) => errs.push(format!("{}: {}: {e}", mesh.id, path.display())),
    }
    errs
}

/// Every problem found (empty = valid).
pub fn validate(car: &LoadedCar, mesh: &CarMesh, panels: Option<&PanelSpec>) -> Vec<String> {
    let mut errs = Vec::new();
    let lattice = &car.lattice;
    if mesh.id != car.def.id {
        errs.push(format!("mesh id {:?} != car id {:?}", mesh.id, car.def.id));
    }
    if mesh.lattice_id != lattice.id {
        errs.push(format!(
            "mesh lattice {:?} != {:?}",
            mesh.lattice_id, lattice.id
        ));
    }
    // Format 5 exactly when the mesh carries a detail texture, else format 4.
    let want = if mesh.detail.is_some() {
        MESH_FORMAT
    } else {
        MESH_FORMAT_PLAIN
    };
    if mesh.format != want {
        errs.push(format!("format {} (expected {want})", mesh.format));
    }
    errs.extend(validate_detail(car, mesh));
    let tris: u32 = mesh.parts.iter().map(|p| p.tri_count() as u32).sum();
    if tris != mesh.tri_count {
        errs.push(format!(
            "tri_count {} but parts have {tris}",
            mesh.tri_count
        ));
    }
    let tier = crate::style::style_for(&car.def).tier;
    let budget = tier.tri_budget();
    if tris < budget.0 || tris > budget.1 {
        errs.push(format!(
            "{}: {tris} tris outside the {tier:?} budget {budget:?}",
            mesh.id
        ));
    }
    let mut names = BTreeSet::new();
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in &mesh.parts {
        let tag = format!("{}/{}", mesh.id, p.name);
        if !names.insert(p.name.as_str()) {
            errs.push(format!("{tag}: duplicate part name"));
        }
        let lower = p.name.to_ascii_lowercase();
        if lower != p.name
            || !p
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            errs.push(format!("{tag}: names are lower_snake_case"));
        }
        let n = p.vertex_count();
        if p.positions_mm.len() % 3 != 0 || p.uvs.len() != n * 2 || p.indices.len() % 3 != 0 {
            errs.push(format!("{tag}: attribute lengths"));
            continue;
        }
        if p.indices.iter().any(|&i| i as usize >= n) {
            errs.push(format!("{tag}: index out of range"));
        }
        match (p.kind, p.region, p.wheel) {
            (k, None, Some(w)) if k.is_wheel_space() && w < 4 => {
                if !p.skin_nodes.is_empty() || p.rigid {
                    errs.push(format!("{tag}: wheel parts are not skinned"));
                }
            }
            (k, _, _) if k.is_wheel_space() => {
                errs.push(format!("{tag}: wheel parts need wheel 0..4 and no region"))
            }
            (_, Some(region), None) => {
                if p.skin_nodes.len() != n * 4 || p.skin_weights.len() != n * 4 {
                    errs.push(format!("{tag}: not skinned (run bind-skin)"));
                    continue;
                }
                let allowed: BTreeSet<u16> = part_candidates(lattice, p).into_iter().collect();
                if p.skin_nodes.iter().any(|i| !allowed.contains(i)) {
                    errs.push(format!(
                        "{tag}: bound to a node outside {region:?} and its fallbacks"
                    ));
                }
                if p.skin_weights
                    .chunks(4)
                    .any(|w| w.iter().map(|&x| x as u32).sum::<u32>() != 255)
                {
                    errs.push(format!("{tag}: weights must sum to 255"));
                }
                if p.rigid
                    && (p.skin_nodes.iter().any(|&i| i != p.skin_nodes[0])
                        || p.skin_weights.chunks(4).any(|w| w != [255, 0, 0, 0]))
                {
                    errs.push(format!("{tag}: rigid parts bind one node, weight 255"));
                }
                if let Some(f) = &p.fold {
                    if f.weights.len() != n * 3 {
                        errs.push(format!("{tag}: fold weights need 3 per vertex"));
                    }
                    if f.chords.is_empty() || f.leads.len() != f.chords.len() {
                        errs.push(format!("{tag}: fold needs chords and one lead triple each"));
                    }
                    if f.chords
                        .iter()
                        .flatten()
                        .any(|&i| lattice.nodes.get(i as usize).map(|nd| nd.region) != Some(region))
                    {
                        errs.push(format!("{tag}: fold chord node outside {region:?}"));
                    }
                    if f.leads.iter().flatten().any(|&i| {
                        lattice
                            .nodes
                            .get(i as usize)
                            .is_none_or(|nd| nd.region == region)
                    }) {
                        errs.push(format!(
                            "{tag}: fold lead node missing or inside {region:?}"
                        ));
                    }
                    let dl = f.dir.iter().map(|x| x * x).sum::<f32>().sqrt();
                    if f.across != 0 && f.across != 2 || (dl - 1.0).abs() > 0.01 {
                        errs.push(format!("{tag}: fold axis / direction"));
                    }
                    if !(1.0..=3.0).contains(&f.gain) || !(0.0..=0.4).contains(&f.max_m) {
                        errs.push(format!("{tag}: fold gain / max out of range"));
                    }
                }
                for v in 0..n {
                    let q = p.vertex(v);
                    for k in 0..3 {
                        lo[k] = lo[k].min(q[k]);
                        hi[k] = hi[k].max(q[k]);
                    }
                }
            }
            _ => errs.push(format!(
                "{tag}: body parts need a region and no wheel index"
            )),
        }
        if p.kind == PartKind::Glass && p.material != crate::mesh::MaterialSlot::Glass {
            errs.push(format!("{tag}: glass part without glass material"));
        }
    }
    for w in 0..4u8 {
        for kind in [PartKind::Tire, PartKind::Rim] {
            if !mesh
                .parts
                .iter()
                .any(|p| p.wheel == Some(w) && p.kind == kind)
            {
                errs.push(format!("{}: wheel {w} has no {kind:?}", mesh.id));
            }
        }
    }
    for r in EXTERIOR_REGIONS {
        let has_nodes = lattice.nodes.iter().any(|n| n.region == r);
        let has_part = mesh.parts.iter().any(|p| p.region == Some(r));
        if has_nodes && !has_part {
            errs.push(format!(
                "{}: region {r:?} has lattice nodes but no mesh part",
                mesh.id
            ));
        }
    }
    if let Some(spec) = panels {
        for p in &spec.panels {
            if !mesh
                .parts
                .iter()
                .any(|m| m.region == Some(p.region) && m.kind == PartKind::Panel)
            {
                errs.push(format!(
                    "{}: panel {} has no painted mesh part",
                    mesh.id, p.name
                ));
            }
        }
    }
    // Pivot / scale: chassis origin, metres, body within its declared size.
    let [w, l, h] = car.def.body_size_m;
    let size = [hi[0] - lo[0], hi[1] - lo[1], hi[2]];
    let mid_y = 0.5 * (hi[1] + lo[1]);
    if (size[0] - w).abs() > 0.35 * w
        || (size[1] - l).abs() > 0.15 * l
        || (size[2] - h).abs() > 0.2 * h
    {
        errs.push(format!(
            "{}: mesh size {size:?} vs body_size_m {:?} (pivot/scale)",
            mesh.id, car.def.body_size_m
        ));
    }
    if mid_y.abs() > 0.25 * l || lo[2] < -0.05 {
        errs.push(format!(
            "{}: mesh not centred on the chassis origin",
            mesh.id
        ));
    }
    errs
}
