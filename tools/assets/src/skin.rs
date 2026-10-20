//! `bind-skin`: bind every vertex of every region part to its 4 nearest lattice nodes with
//! inverse-distance weights. Region-aware: a part only binds to nodes of its own region
//! (hood vertices never follow fender nodes), topped up from neighbour regions when its
//! region has fewer than four nodes (glass, 2-door side strips).
//!
//! Light lenses are the exception: they bind to the nearest nodes of the end of the car they
//! sit on (bumper, hood / trunk, the fender / quarter on their side, the frame), so they crush
//! and move with the bumper and hood. Hood and trunk lids, front fenders and rear quarters get a fold
//! binding ([`bind_fold`]) so a crushed panel buckles and follows the crushed front instead of
//! sliding back flat.

use wp_contracts::{BodyRegion, LatticeDef};

use crate::mesh::{
    fold_height, CarMesh, FoldBinding, MeshPart, PartKind, FENDER_CRUMPLE_GAIN, FENDER_FOLD_MAX_M,
    FOLD_MAX_M, FOLD_REAR_LIFT, FOLD_SPLIT, HOOD_CRUMPLE_GAIN, LID_POP_M, TRUNK_CRUMPLE_GAIN,
};

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
    part.fold = None;
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
    if part.kind == PartKind::Panel {
        part.fold = bind_fold(lattice, part);
    }
}

/// Where along the fold a rest point sits: 0 at the fixed row, 1 at the leading row.
fn fold_s(p: [f32; 3], fixed_y: f32, sign: f32, span: f32) -> f32 {
    ((p[1] - fixed_y) * sign / span).clamp(0.0, 1.0)
}

/// Piecewise-linear profile through `pts` (s ascending, from s = 0 to s = 1).
fn profile(s: f32, pts: &[(f32, f32)]) -> f32 {
    for w in pts.windows(2) {
        if s <= w[1].0 {
            let t = (s - w[0].0) / (w[1].0 - w[0].0).max(1e-4);
            return w[0].1 + (w[1].1 - w[0].1) * t;
        }
    }
    pts.last().map_or(0.0, |p| p.1)
}

/// Lid crease shape at fold position `s` (0 hinge … 1 latch) and lateral position `xn`
/// (−1 … 1): a main ridge a little behind mid-lid, a valley, a second smaller ridge toward the
/// latch; the crease lines run diagonally (skewed with `xn`) and the lid tents at its centre
/// line. Both ends stay put (the latch end pops separately).
pub fn crease_shape(s: f32, xn: f32) -> f32 {
    let p1 = 0.42 + 0.08 * xn;
    let v = 0.64 + 0.05 * xn;
    let p2 = 0.82 + 0.03 * xn;
    let f = profile(
        s,
        &[(0.0, 0.0), (p1, 1.0), (v, 0.25), (p2, 0.7), (1.0, 0.0)],
    );
    f * (1.0 - 0.55 * xn.abs())
}

/// Front-fender crease shape at `s` (0 rear row … 1 nose) and height position `zn` (−1 … 1):
/// one ridge ahead of the wheel, skewed with height, strongest at mid-height.
pub fn fender_crease_shape(s: f32, zn: f32) -> f32 {
    let p = 0.6 + 0.08 * zn;
    profile(s, &[(0.0, 0.0), (p, 1.0), (1.0, 0.0)]) * (1.0 - 0.4 * zn.abs())
}

/// Rear-edge lift shape: the hinge edge's middle bows up once the latch is gone.
pub fn rear_lift_shape(s: f32, xn: f32) -> f32 {
    (1.0 - s / 0.25).max(0.0) * (1.0 - xn.abs()).max(0.0)
}

/// Regions whose nodes a panel's leading end follows (front / rear structure), or `None` for
/// a panel without a fold binding.
pub fn lead_regions(region: BodyRegion) -> Option<&'static [BodyRegion]> {
    use BodyRegion::*;
    Some(match region {
        Hood => &[FrontBumper, FrameFront, FrontLeftFender, FrontRightFender],
        Trunk => &[RearBumper, FrameRear, RearLeftQuarter, RearRightQuarter],
        FrontLeftFender | FrontRightFender => &[FrontBumper, FrameFront],
        RearLeftQuarter | RearRightQuarter => &[RearBumper, FrameRear],
        _ => return None,
    })
}

/// Three nodes of `regions` (not pinned) nearest node `to`, ties by id.
fn nearest_leads(lattice: &LatticeDef, regions: &[BodyRegion], to: u16) -> Option<[u16; 3]> {
    let p = lattice.nodes[to as usize].pos_m;
    let mut c: Vec<(f32, u16)> = lattice
        .nodes
        .iter()
        .filter(|n| regions.contains(&n.region) && !n.pinned_to_chassis)
        .map(|n| (dist2(p, n.pos_m), n.id))
        .collect();
    c.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    (c.len() >= 3).then(|| [c[0].1, c[1].1, c[2].1])
}

/// Fold binding of a hood / trunk lid, front fender or rear quarter part (see [`FoldBinding`]); `None` for
/// other parts and for lids whose nodes all sit on the hinge line (hatch and pickup tailgates).
/// Lid chords run hinge row → latch row (from the lattice detachable's hinge nodes), spread
/// across x; fender chords run rear row → front row of each height row, spread across z.
pub fn bind_fold(lattice: &LatticeDef, part: &MeshPart) -> Option<FoldBinding> {
    let region = part.region?;
    let leads_from = lead_regions(region)?;
    let lid = matches!(region, BodyRegion::Hood | BodyRegion::Trunk);
    let nodes: Vec<&wp_contracts::LatticeNode> = lattice
        .nodes
        .iter()
        .filter(|n| n.region == region && !n.pinned_to_chassis)
        .collect();
    if nodes.len() < 4 {
        return None;
    }
    // Fixed row position along y and the direction toward the leading end.
    let (fixed_y, sign) = if lid {
        let det = lattice.detachables.iter().find(|d| d.region == region)?;
        let hinge_y = det
            .hinge_nodes
            .iter()
            .map(|&i| lattice.nodes[i as usize].pos_m[1])
            .sum::<f32>()
            / det.hinge_nodes.len().max(1) as f32;
        let mean_y = nodes.iter().map(|n| n.pos_m[1]).sum::<f32>() / nodes.len() as f32;
        (hinge_y, if mean_y >= hinge_y { 1.0 } else { -1.0 })
    } else {
        // Side panels: fixed at the door row, leading at the car's end (front fenders: the
        // nose, +Y; rear quarters: the tail, −Y).
        if matches!(
            region,
            BodyRegion::RearLeftQuarter | BodyRegion::RearRightQuarter
        ) {
            (
                nodes.iter().map(|n| n.pos_m[1]).fold(f32::MIN, f32::max),
                -1.0,
            )
        } else {
            (
                nodes.iter().map(|n| n.pos_m[1]).fold(f32::MAX, f32::min),
                1.0,
            )
        }
    };
    let d = |n: &wp_contracts::LatticeNode| (n.pos_m[1] - fixed_y) * sign;
    let d_min = nodes.iter().map(|n| d(n)).fold(f32::MAX, f32::min);
    let d_max = nodes.iter().map(|n| d(n)).fold(f32::MIN, f32::max);
    let span = d_max - d_min;
    if span < 0.15 {
        return None;
    }
    let across: usize = if lid { 0 } else { 2 };
    let near: Vec<&&wp_contracts::LatticeNode> =
        nodes.iter().filter(|n| d(n) < d_min + 0.03).collect();
    let far: Vec<&&wp_contracts::LatticeNode> =
        nodes.iter().filter(|n| d(n) > d_max - 0.03).collect();
    let mut chords: Vec<(f32, [u16; 2])> = Vec::new();
    for f in &far {
        let Some(n) = near.iter().min_by(|a, b| {
            (a.pos_m[across] - f.pos_m[across])
                .abs()
                .total_cmp(&(b.pos_m[across] - f.pos_m[across]).abs())
                .then(a.id.cmp(&b.id))
        }) else {
            continue;
        };
        chords.push((f.pos_m[across], [n.id, f.id]));
    }
    chords.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    chords.dedup_by(|a, b| a.1 == b.1);
    let leads: Option<Vec<[u16; 3]>> = chords
        .iter()
        .map(|c| nearest_leads(lattice, leads_from, c.1[1]))
        .collect();
    let leads = leads?;
    if chords.is_empty() {
        return None;
    }
    // Across coordinate normalised to −1 … 1 over the part (lids: |x| from the centre line;
    // fenders: height about the part's middle).
    let (lo, hi) = (0..part.vertex_count()).fold((f32::MAX, f32::MIN), |a, v| {
        let q = part.vertex(v)[across];
        (a.0.min(q), a.1.max(q))
    });
    let (mid, half) = if lid {
        (0.0, lo.abs().max(hi.abs()).max(0.1))
    } else {
        (0.5 * (lo + hi), (0.5 * (hi - lo)).max(0.05))
    };
    let y0 = fixed_y + sign * d_min;
    let q = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut weights = Vec::with_capacity(part.vertex_count() * 3);
    for v in 0..part.vertex_count() {
        let p = part.vertex(v);
        let s = fold_s(p, y0, sign, span);
        let an = ((p[across] - mid) / half).clamp(-1.0, 1.0);
        if lid {
            weights.push(q(crease_shape(s, an)));
            weights.push(q(rear_lift_shape(s, an)));
        } else {
            weights.push(q(fender_crease_shape(s, an)));
            weights.push(0);
        }
        // Leading-end follow: nothing at the fixed row, all of it from the leading row on.
        let lead = ((p[1] - y0) * sign / span).clamp(0.0, 1.0);
        weights.push(q(lead));
    }
    let (gain, max_m, pop_m, dir) = match region {
        BodyRegion::Hood => (HOOD_CRUMPLE_GAIN, FOLD_MAX_M, LID_POP_M, [0.0, 0.0, 1.0]),
        BodyRegion::Trunk => (TRUNK_CRUMPLE_GAIN, FOLD_MAX_M, LID_POP_M, [0.0, 0.0, 1.0]),
        // Front fenders and rear quarters buckle outward and up.
        _ => {
            let sx = if part_centre(part)[0] < 0.0 {
                -0.8
            } else {
                0.8
            };
            (FENDER_CRUMPLE_GAIN, FENDER_FOLD_MAX_M, 0.0, [sx, 0.0, 0.6])
        }
    };
    Some(FoldBinding {
        chords: chords.iter().map(|c| c.1).collect(),
        leads,
        across: across as u8,
        dir,
        gain,
        max_m,
        pop_m,
        weights,
    })
}

/// Per-chord fold values of one part in one pose: rest coordinate on the across axis, fold
/// height, extra pull-back of the leading end and leading-edge pop (m), and the chord's rest
/// direction (fixed → leading).
#[derive(Debug, Clone, Copy)]
pub struct ChordFold {
    /// Rest coordinate of the chord on the across axis, m.
    pub at: f32,
    /// Fold height, m.
    pub height: f32,
    /// How much further (m) the front structure moved toward the fixed end than the panel's
    /// own leading node.
    pub extra: f32,
    /// Leading-edge pop, m.
    pub pop: f32,
    /// Rest direction fixed → leading (unit).
    pub u: [f32; 3],
}

/// Crumple memory of one folding part: per chord, the largest effective shortening and
/// leading-end pull-back seen so far (m). Sheet metal does not spring back: a bumper that
/// crushed the hood's nose and then fell off leaves the hood crumpled. Cleared when the car
/// is back at rest (repaired). The browser keeps the same per part.
pub type FoldMemory = Vec<[f32; 2]>;

/// Largest leading-end pull-back, share of the chord's rest length.
pub const FOLD_PULL_MAX: f32 = 0.5;

/// Largest node offset from rest (m) that still counts as "at rest" for [`FoldMemory`].
pub const FOLD_REST_EPS_M: f32 = 1e-3;

/// Where a lead node is, chassis space, as if its panel were still attached (a hanging bumper's
/// nodes rotated back about its mount, keeping their crush), or `None` once it fell off.
pub type LeadPos<'a> = &'a dyn Fn(u16) -> Option<[f32; 3]>;

/// Fold values per chord. `local` are the nodes in the panel's own frame (= chassis nodes for
/// an attached panel), `chassis` the nodes in chassis space; `lead_pos` places the lead nodes;
/// `free` = the panel is unlatched or hanging. `memory` (if given) ratchets the shortening and
/// pull-back.
pub fn chord_folds(
    f: &FoldBinding,
    rest: &[[f32; 3]],
    local: &[[f32; 3]],
    chassis: &[[f32; 3]],
    free: bool,
    lead_pos: LeadPos,
    memory: Option<&mut FoldMemory>,
) -> Vec<ChordFold> {
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let off = |i: usize| {
        let d = sub(chassis[i], rest[i]);
        dot(d, d).sqrt()
    };
    let mut memory = memory;
    if let Some(m) = memory.as_deref_mut() {
        let at_rest = f
            .chords
            .iter()
            .flatten()
            .chain(f.leads.iter().flatten())
            .all(|&i| off(i as usize) <= FOLD_REST_EPS_M);
        if at_rest || m.len() != f.chords.len() {
            m.clear();
            m.resize(f.chords.len(), [0.0, 0.0]);
        }
    }
    f.chords
        .iter()
        .zip(&f.leads)
        .enumerate()
        .map(|(c, (&[a, b], leads))| {
            let (a, b) = (a as usize, b as usize);
            let ru = sub(rest[b], rest[a]);
            let la = dot(ru, ru).sqrt().max(1e-4);
            let u = [ru[0] / la, ru[1] / la, ru[2] / la];
            let now = sub(local[b], local[a]);
            let own = la - dot(now, now).sqrt();
            let da = sub(local[a], rest[a]);
            let push_latch = -dot(sub(sub(local[b], rest[b]), da), u);
            let dac = sub(chassis[a], rest[a]);
            let mut dl = [0.0f32; 3];
            let mut n = 0.0f32;
            for (l, at) in leads.iter().filter_map(|&l| lead_pos(l).map(|p| (l, p))) {
                let d = sub(at, rest[l as usize]);
                for k in 0..3 {
                    dl[k] += d[k];
                }
                n += 1.0;
            }
            let push_lead = if n > 0.0 {
                -dot(sub([dl[0] / n, dl[1] / n, dl[2] / n], dac), u)
            } else {
                push_latch
            };
            let mut extra = (push_lead - push_latch).max(0.0);
            let mut eff = f.gain * own.max(push_lead);
            if let Some(m) = memory.as_deref_mut() {
                eff = eff.max(m[c][0]);
                extra = extra.max(m[c][1]);
                m[c] = [eff, extra];
            }
            // The front can push the leading end back by at most half the panel.
            let extra = extra.min(FOLD_PULL_MAX * la);
            ChordFold {
                at: rest[b][f.across as usize],
                height: (FOLD_SPLIT * fold_height(la, eff)).min(f.max_m),
                extra,
                pop: if free { f.pop_m } else { 0.0 },
                u,
            }
        })
        .collect()
}

/// Chord values at across coordinate `x`, linear between chords (sorted by `at`).
pub fn fold_at(cs: &[ChordFold], x: f32) -> ChordFold {
    let Some(first) = cs.first() else {
        return ChordFold {
            at: x,
            height: 0.0,
            extra: 0.0,
            pop: 0.0,
            u: [0.0, 1.0, 0.0],
        };
    };
    let mix = |a: &ChordFold, b: &ChordFold, t: f32| ChordFold {
        at: x,
        height: a.height + (b.height - a.height) * t,
        extra: a.extra + (b.extra - a.extra) * t,
        pop: a.pop + (b.pop - a.pop) * t,
        u: a.u,
    };
    if x <= first.at {
        return *first;
    }
    for w in cs.windows(2) {
        if x <= w[1].at {
            let t = (x - w[0].at) / (w[1].at - w[0].at).max(1e-4);
            return mix(&w[0], &w[1], t);
        }
    }
    *cs.last().unwrap_or(first)
}

/// Reference CPU skinning of one part (what `apps/testbed-web/src/render/panels.ts` does):
/// each vertex moves by the weighted displacement of its 4 nodes (`local`: nodes in the
/// panel's own frame), then a part with a fold binding follows the crushed structure and
/// buckles (see [`FoldBinding`]; `chassis` nodes feed the lead push). Positions are in the
/// panel's own frame (chassis space for an attached panel). `memory`: see [`FoldMemory`].
pub fn deform_with(
    part: &MeshPart,
    rest: &[[f32; 3]],
    local: &[[f32; 3]],
    chassis: &[[f32; 3]],
    free: bool,
    lead_pos: LeadPos,
    memory: Option<&mut FoldMemory>,
) -> Vec<[f32; 3]> {
    deform_detail(part, rest, local, chassis, free, lead_pos, memory).0
}

/// [`deform_with`] plus, per vertex, the fold's `[rise along dir, leading-end pull-back]`, m.
pub fn deform_detail(
    part: &MeshPart,
    rest: &[[f32; 3]],
    local: &[[f32; 3]],
    chassis: &[[f32; 3]],
    free: bool,
    lead_pos: LeadPos,
    memory: Option<&mut FoldMemory>,
) -> (Vec<[f32; 3]>, Vec<[f32; 2]>) {
    let folds = part.fold.as_ref().map(|f| {
        (
            f,
            chord_folds(f, rest, local, chassis, free, lead_pos, memory),
        )
    });
    let mut out = Vec::with_capacity(part.vertex_count());
    let mut detail = vec![[0.0f32; 2]; part.vertex_count()];
    for (v, dv) in detail.iter_mut().enumerate() {
        let r = part.vertex(v);
        let mut p = r;
        if part.skin_nodes.len() == part.vertex_count() * 4 {
            for k in 0..4 {
                let i = part.skin_nodes[v * 4 + k] as usize;
                let w = part.skin_weights[v * 4 + k] as f32 / 255.0;
                for (a, pk) in p.iter_mut().enumerate() {
                    *pk += w * (local[i][a] - rest[i][a]);
                }
            }
        }
        if let Some((f, cs)) = &folds {
            let c = fold_at(cs, r[f.across as usize]);
            let crease = f.weights[v * 3] as f32 / 255.0;
            let lift = f.weights[v * 3 + 1] as f32 / 255.0;
            let lead = f.weights[v * 3 + 2] as f32 / 255.0;
            let rear = if free { FOLD_REAR_LIFT * lift } else { 0.0 };
            let rise = c.height * (crease + rear) + c.pop * lead * lead;
            for (k, pk) in p.iter_mut().enumerate() {
                *pk += f.dir[k] * rise - c.u[k] * c.extra * lead;
            }
            *dv = [rise, c.extra * lead];
        }
        out.push(p);
    }
    (out, detail)
}

/// [`deform_with`] for an attached panel (or a test pose) without crumple memory: `nodes`
/// are chassis space, every lead node counts; `unlatched` adds the fixed-end lift and the
/// leading-edge pop.
pub fn deform(
    part: &MeshPart,
    rest: &[[f32; 3]],
    nodes: &[[f32; 3]],
    unlatched: bool,
) -> Vec<[f32; 3]> {
    deform_with(
        part,
        rest,
        nodes,
        nodes,
        unlatched,
        &|n| Some(nodes[n as usize]),
        None,
    )
}

/// Bind every part of a mesh against `lattice`.
pub fn bind_skin(lattice: &LatticeDef, mesh: &mut CarMesh) {
    for part in &mut mesh.parts {
        bind_part(lattice, part);
    }
}
