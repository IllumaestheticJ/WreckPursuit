//! `bind-skin`: bind every vertex of every region part to its 4 nearest lattice nodes with
//! inverse-distance weights. Region-aware: a part only binds to nodes of its own region
//! (hood vertices never follow fender nodes), topped up from neighbour regions when its
//! region has fewer than four nodes (glass, 2-door side strips).
//!
//! Light lenses are the exception: they bind to the nearest nodes of the end of the car they
//! sit on (bumper, hood / trunk, the fender / quarter on their side, the frame), so they crush
//! and move with the bumper and hood. Hood and trunk lids also get a fold binding
//! ([`bind_fold`]) so a crushed lid buckles instead of sliding back flat.

use wp_contracts::{BodyRegion, LatticeDef};

use crate::mesh::{fold_height, CarMesh, MeshPart, PartKind, FOLD_REAR_LIFT};

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

/// Regions a light lens at rest centre `c` binds to: everything at its end of the car on its
/// side (front: bumper, hood, fender, frame; rear: bumper, trunk, quarter, frame).
pub fn light_regions(c: [f32; 3]) -> [BodyRegion; 4] {
    use BodyRegion::*;
    match (c[1] > 0.0, c[0] > 0.0) {
        (true, false) => [FrontBumper, Hood, FrontLeftFender, FrameFront],
        (true, true) => [FrontBumper, Hood, FrontRightFender, FrameFront],
        (false, false) => [RearBumper, Trunk, RearLeftQuarter, FrameRear],
        (false, true) => [RearBumper, Trunk, RearRightQuarter, FrameRear],
    }
}

/// Rest centre (vertex average) of a part, m.
pub fn part_centre(part: &MeshPart) -> [f32; 3] {
    let n = part.vertex_count().max(1) as f32;
    let mut c = [0.0; 3];
    for v in 0..part.vertex_count() {
        let p = part.vertex(v);
        for k in 0..3 {
            c[k] += p[k] / n;
        }
    }
    c
}

/// Nodes a part may bind to: [`candidates`] of its region, or for a light lens every node of
/// its [`light_regions`] (not pinned to the chassis, so the lens follows the crush).
pub fn part_candidates(lattice: &LatticeDef, part: &MeshPart) -> Vec<u16> {
    let Some(region) = part.region else {
        return Vec::new();
    };
    if part.kind == PartKind::Light {
        let regions = light_regions(part_centre(part));
        return lattice
            .nodes
            .iter()
            .filter(|n| regions.contains(&n.region) && !n.pinned_to_chassis)
            .map(|n| n.id)
            .collect();
    }
    candidates(lattice, region)
}

/// Bind one part (no-op for wheel parts).
pub fn bind_part(lattice: &LatticeDef, part: &mut MeshPart) {
    part.skin_nodes.clear();
    part.skin_weights.clear();
    part.fold_chords.clear();
    part.fold_weights.clear();
    let Some(region) = part.region else {
        return;
    };
    let cand = part_candidates(lattice, part);
    // Lights pick the nearest of all their candidates; other parts prefer their own region.
    let own_count = if part.kind == PartKind::Light {
        0
    } else {
        lattice.nodes.iter().filter(|n| n.region == region).count()
    };
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
    if part.kind == PartKind::Panel && matches!(region, BodyRegion::Hood | BodyRegion::Trunk) {
        bind_fold(lattice, part);
    }
}

/// Where along the fold a rest point sits: 0 at the hinge row, 1 at the latch row.
fn fold_s(p: [f32; 3], hinge_y: f32, sign: f32, span: f32) -> f32 {
    ((p[1] - hinge_y) * sign / span).clamp(0.0, 1.0)
}

/// Crease shape at fold position `s` (0 hinge … 1 latch) and lateral position `xn` (−1 … 1):
/// a peak a little ahead of mid-panel, skewed across the lid so the crease runs diagonally,
/// and a smaller second crease toward the latch edge. Both ends stay put.
pub fn crease_shape(s: f32, xn: f32) -> f32 {
    let peak = 0.5 + 0.08 * xn;
    let knee = peak + 0.55 * (1.0 - peak);
    let f = if s <= peak {
        s / peak
    } else if s <= knee {
        1.0 - 0.7 * (s - peak) / (knee - peak)
    } else {
        0.3 * (1.0 - s) / (1.0 - knee)
    };
    // Softer toward the fenders (the lid's edges are stiffened).
    f * (1.0 - 0.35 * xn * xn)
}

/// Rear-edge lift shape: the hinge edge's middle bows up once the latch is gone.
pub fn rear_lift_shape(s: f32, xn: f32) -> f32 {
    (1.0 - s / 0.25).max(0.0) * (1.0 - xn.abs()).max(0.0)
}

/// Fold binding of a hood / trunk lid part: chords from the hinge row of its lattice region
/// to the latch row (sorted by rest x) and per-vertex crease / rear-lift weights. Lids whose
/// nodes all sit on the hinge line (hatch and pickup tailgates) get none.
pub fn bind_fold(lattice: &LatticeDef, part: &mut MeshPart) {
    part.fold_chords.clear();
    part.fold_weights.clear();
    let Some(region) = part.region else {
        return;
    };
    let Some(det) = lattice.detachables.iter().find(|d| d.region == region) else {
        return;
    };
    let nodes: Vec<&wp_contracts::LatticeNode> = lattice
        .nodes
        .iter()
        .filter(|n| n.region == region && !n.pinned_to_chassis)
        .collect();
    if nodes.len() < 4 {
        return;
    }
    let hinge_y = det
        .hinge_nodes
        .iter()
        .map(|&i| lattice.nodes[i as usize].pos_m[1])
        .sum::<f32>()
        / det.hinge_nodes.len().max(1) as f32;
    let mean_y = nodes.iter().map(|n| n.pos_m[1]).sum::<f32>() / nodes.len() as f32;
    let sign = if mean_y >= hinge_y { 1.0 } else { -1.0 };
    let d = |n: &wp_contracts::LatticeNode| (n.pos_m[1] - hinge_y) * sign;
    let d_min = nodes.iter().map(|n| d(n)).fold(f32::MAX, f32::min);
    let d_max = nodes.iter().map(|n| d(n)).fold(f32::MIN, f32::max);
    let span = d_max - d_min;
    if span < 0.15 {
        return;
    }
    let near: Vec<&&wp_contracts::LatticeNode> =
        nodes.iter().filter(|n| d(n) < d_min + 0.03).collect();
    let far: Vec<&&wp_contracts::LatticeNode> =
        nodes.iter().filter(|n| d(n) > d_max - 0.03).collect();
    let mut chords: Vec<(f32, [u16; 2])> = Vec::new();
    for f in &far {
        let Some(n) = near.iter().min_by(|a, b| {
            (a.pos_m[0] - f.pos_m[0])
                .abs()
                .total_cmp(&(b.pos_m[0] - f.pos_m[0]).abs())
                .then(a.id.cmp(&b.id))
        }) else {
            continue;
        };
        chords.push((f.pos_m[0], [n.id, f.id]));
    }
    chords.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    chords.dedup_by(|a, b| a.1 == b.1);
    if chords.is_empty() {
        return;
    }
    part.fold_chords = chords.iter().map(|c| c.1).collect();
    let hx = (0..part.vertex_count())
        .map(|v| part.vertex(v)[0].abs())
        .fold(0.0f32, f32::max)
        .max(0.1);
    // The fold runs from the hinge row to the latch row of the nodes.
    let y0 = hinge_y + sign * d_min;
    for v in 0..part.vertex_count() {
        let p = part.vertex(v);
        let s = fold_s(p, y0, sign, span);
        let xn = (p[0] / hx).clamp(-1.0, 1.0);
        let q = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        part.fold_weights.push(q(crease_shape(s, xn)));
        part.fold_weights.push(q(rear_lift_shape(s, xn)));
    }
}

/// Fold height (m) at rest x `x` from the part's chords, linear across x between chords.
pub fn fold_height_at(part: &MeshPart, rest: &[[f32; 3]], nodes: &[[f32; 3]], x: f32) -> f32 {
    let len = |a: [f32; 3], b: [f32; 3]| {
        let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    };
    let hs: Vec<(f32, f32)> = part
        .fold_chords
        .iter()
        .map(|&[a, b]| {
            let (a, b) = (a as usize, b as usize);
            (
                rest[b][0],
                fold_height(len(rest[a], rest[b]), len(nodes[a], nodes[b])),
            )
        })
        .collect();
    let Some(first) = hs.first() else {
        return 0.0;
    };
    if x <= first.0 {
        return first.1;
    }
    for w in hs.windows(2) {
        if x <= w[1].0 {
            let t = (x - w[0].0) / (w[1].0 - w[0].0).max(1e-4);
            return w[0].1 + (w[1].1 - w[0].1) * t;
        }
    }
    hs.last().map_or(0.0, |h| h.1)
}

/// Reference CPU skinning (what `apps/testbed-web/src/render/panels.ts` does for a part that
/// is not swinging): each vertex moves by the weighted displacement of its 4 nodes, then a
/// hood / trunk lid rises by its fold (`unlatched` adds the rear-edge lift). `rest` and
/// `nodes` are the lattice's rest and current node positions, m.
pub fn deform(
    part: &MeshPart,
    rest: &[[f32; 3]],
    nodes: &[[f32; 3]],
    unlatched: bool,
) -> Vec<[f32; 3]> {
    let mut out = Vec::with_capacity(part.vertex_count());
    for v in 0..part.vertex_count() {
        let mut p = part.vertex(v);
        if part.skin_nodes.len() == part.vertex_count() * 4 {
            let mut d = [0.0; 3];
            for k in 0..4 {
                let i = part.skin_nodes[v * 4 + k] as usize;
                let w = part.skin_weights[v * 4 + k] as f32 / 255.0;
                for (a, dk) in d.iter_mut().enumerate() {
                    *dk += w * (nodes[i][a] - rest[i][a]);
                }
            }
            for k in 0..3 {
                p[k] += d[k];
            }
        }
        if !part.fold_chords.is_empty() {
            let x = part.vertex(v)[0];
            let h = fold_height_at(part, rest, nodes, x);
            let crease = part.fold_weights[v * 2] as f32 / 255.0;
            let lift = part.fold_weights[v * 2 + 1] as f32 / 255.0;
            let rear = if unlatched {
                FOLD_REAR_LIFT * lift
            } else {
                0.0
            };
            p[2] += h * (crease + rear);
        }
        out.push(p);
    }
    out
}

/// Bind every part of a mesh against `lattice`.
pub fn bind_skin(lattice: &LatticeDef, mesh: &mut CarMesh) {
    for part in &mut mesh.parts {
        bind_part(lattice, part);
    }
}
