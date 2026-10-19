//! `bind-skin`: bind every vertex of every region part to its 4 nearest lattice nodes with
//! inverse-distance weights. Region-aware: a part only binds to nodes of its own region
//! (hood vertices never follow fender nodes), topped up from neighbour regions when its
//! region has fewer than four nodes (glass, 2-door side strips).

use wp_contracts::{BodyRegion, LatticeDef};

use crate::mesh::{CarMesh, MeshPart};

/// Regions a part may borrow nodes from when its own region has fewer than four.
pub fn fallback_regions(r: BodyRegion) -> &'static [BodyRegion] {
    use BodyRegion::*;
    match r {
        Windshield | RearGlass => &[Roof, Cabin],
        Roof => &[Cabin],
        LeftDoorR => &[RearLeftQuarter, Cabin],
        RightDoorR => &[RearRightQuarter, Cabin],
        Trunk => &[RearLeftQuarter, RearRightQuarter, FrameRear],
        FrameLeft | FrameRight => &[Cabin],
        _ => &[Cabin],
    }
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

/// Candidate nodes for a part of `region`: its own nodes, plus the nearest-to-centroid nodes
/// of its fallback regions until there are at least four.
pub fn candidates(lattice: &LatticeDef, region: BodyRegion) -> Vec<u16> {
    let own: Vec<u16> = lattice
        .nodes
        .iter()
        .filter(|n| n.region == region)
        .map(|n| n.id)
        .collect();
    if own.len() >= 4 {
        return own;
    }
    let mut out = own;
    for &f in fallback_regions(region) {
        out.extend(lattice.nodes.iter().filter(|n| n.region == f).map(|n| n.id));
        if out.len() >= 4 {
            break;
        }
    }
    out
}

/// Bind one part (no-op for wheel parts).
pub fn bind_part(lattice: &LatticeDef, part: &mut MeshPart) {
    part.skin_nodes.clear();
    part.skin_weights.clear();
    let Some(region) = part.region else {
        return;
    };
    let cand = candidates(lattice, region);
    let own_count = lattice.nodes.iter().filter(|n| n.region == region).count();
    for v in 0..part.vertex_count() {
        let p = part.vertex(v);
        // Own-region nodes first when there are enough; borrowed ones only fill the gap.
        let mut best: Vec<(f32, u16)> = cand
            .iter()
            .map(|&i| (dist2(p, lattice.nodes[i as usize].pos_m), i))
            .collect();
        best.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        if own_count >= 4 {
            best.retain(|b| lattice.nodes[b.1 as usize].region == region);
        }
        best.truncate(4);
        while best.len() < 4 {
            best.push(best[0]);
        }
        let w: Vec<f32> = best.iter().map(|b| 1.0 / (b.0 + 1e-4)).collect();
        let sum: f32 = w.iter().sum();
        let mut q: Vec<u8> = w.iter().map(|x| (x / sum * 255.0).round() as u8).collect();
        // Make the quantised weights sum to exactly 255 (fix on the largest).
        let total: i32 = q.iter().map(|&x| x as i32).sum();
        let k = (0..4)
            .max_by(|&a, &b| q[a].cmp(&q[b]).then(b.cmp(&a)))
            .unwrap_or(0);
        q[k] = (q[k] as i32 + 255 - total).clamp(0, 255) as u8;
        for (d, node) in &best {
            let _ = d;
            part.skin_nodes.push(*node);
        }
        part.skin_weights.extend_from_slice(&q);
    }
}

/// Bind every part of a mesh against `lattice`.
pub fn bind_skin(lattice: &LatticeDef, mesh: &mut CarMesh) {
    for part in &mut mesh.parts {
        bind_part(lattice, part);
    }
}
