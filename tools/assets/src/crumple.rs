//! Crumple measurement: the Rust reference of the browser's per-part skinning
//! (`apps/testbed-web/src/render/panels.ts` `PanelBody.update`), including free panels (rigid
//! frame fit) and the lid / fender fold, plus the numbers the playtest feedback asks for:
//! largest and mean vertex displacement, fold lines and how far a lid's leading edge trails
//! the crushed front structure.
//!
//! Everything is in chassis space (+X right, +Y forward, +Z up), metres.

use wp_contracts::{BodyRegion, LatticeDef};
use wp_damage::{PanelState, PanelStatus};

use crate::mesh::MeshPart;
use crate::skin;

/// Rigid frame of a free panel: `x' = r x + t` (row-major 3×3).
#[derive(Debug, Clone, Copy)]
struct Frame {
    r: [[f32; 3]; 3],
    t: [f32; 3],
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt().max(1e-9);
    [a[0] / l, a[1] / l, a[2] / l]
}
fn len(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}
fn mat_vec(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [dot(m[0], v), dot(m[1], v), dot(m[2], v)]
}
fn transpose(m: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut t = [[0.0; 3]; 3];
    for (i, row) in m.iter().enumerate() {
        for (j, x) in row.iter().enumerate() {
            t[j][i] = *x;
        }
    }
    t
}

/// Three nodes of `region` spanning the largest triangle at rest (mirrors `spanTriangle`).
fn span_triangle(lattice: &LatticeDef, rest: &[[f32; 3]], region: BodyRegion) -> [usize; 3] {
    let ids: Vec<usize> = lattice
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.region == region)
        .map(|(i, _)| i)
        .collect();
    let mut best = [
        *ids.first().unwrap_or(&0),
        *ids.get(1).unwrap_or(&0),
        *ids.get(2).unwrap_or(&0),
    ];
    let mut area = -1.0f32;
    for i in 0..ids.len() {
        for j in i + 1..ids.len() {
            for k in j + 1..ids.len() {
                let a = rest[ids[i]];
                let c = cross(sub(rest[ids[j]], a), sub(rest[ids[k]], a));
                let ar = dot(c, c);
                if ar > area {
                    area = ar;
                    best = [ids[i], ids[j], ids[k]];
                }
            }
        }
    }
    best
}

/// Orthonormal frame of a node triangle: rows are the axes; plus its origin.
fn tri_frame(p: &[[f32; 3]], [i, j, k]: [usize; 3]) -> ([[f32; 3]; 3], [f32; 3]) {
    let o = p[i];
    let e1 = norm(sub(p[j], o));
    let e3 = norm(cross(e1, sub(p[k], o)));
    let e2 = cross(e3, e1);
    ([e1, e2, e3], o)
}

/// Rigid transform taking the triangle's rest frame to its current frame (mirrors `fitFrame`).
fn fit_frame(rest: &[[f32; 3]], now: &[[f32; 3]], tri: [usize; 3]) -> Frame {
    let (a, ao) = tri_frame(rest, tri);
    let (b, bo) = tri_frame(now, tri);
    // r = B^T A (columns of B times rows of A) maps rest axes onto current axes.
    let bt = transpose(&b);
    let mut r = [[0.0; 3]; 3];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, x) in row.iter_mut().enumerate() {
            *x = (0..3).map(|k| bt[i][k] * a[k][j]).sum();
        }
    }
    let t = sub(bo, mat_vec(&r, ao));
    Frame { r, t }
}

/// Inputs for skinning one car's parts in a given pose.
pub struct Pose<'a> {
    /// The car's lattice (node regions, rest positions).
    pub lattice: &'a LatticeDef,
    /// Rest node positions, m.
    pub rest: &'a [[f32; 3]],
    /// Current node positions, m.
    pub nodes: &'a [[f32; 3]],
    /// Current panel states (empty: every panel attached).
    pub states: &'a [PanelStatus],
}

impl Pose<'_> {
    fn state_of(&self, region: BodyRegion) -> Option<PanelState> {
        self.states
            .iter()
            .find(|s| s.region == region)
            .map(|s| s.state)
    }

    /// Lead node `n` as if its panel were still attached (see [`skin::LeadPos`]): a hanging
    /// panel's node rotated back by its hang angle about its pivot (the sim captures the hanging
    /// pose at angle 0, so this is the crushed pose it was released in); `None` once detached.
    pub fn lead_pos(&self, n: u16) -> Option<[f32; 3]> {
        let at = self.nodes[n as usize];
        let region = self.lattice.nodes[n as usize].region;
        match self.states.iter().find(|s| s.region == region) {
            Some(s) if s.state == PanelState::Detached => None,
            Some(s) if s.state == PanelState::HangingByOne => {
                Some(add(s.pivot, rotate(sub(at, s.pivot), s.axis, -s.angle_rad)))
            }
            _ => Some(at),
        }
    }
}

/// Rotate `v` about unit axis `u` by `angle` (Rodrigues; mirrors `wp_damage::panels`).
fn rotate(v: [f32; 3], u: [f32; 3], angle: f32) -> [f32; 3] {
    let (s, c) = wp_math::sin_cos(angle);
    let cr = cross(u, v);
    let along = dot(u, v) * (1.0 - c);
    [
        v[0] * c + cr[0] * s + u[0] * along,
        v[1] * c + cr[1] * s + u[1] * along,
        v[2] * c + cr[2] * s + u[2] * along,
    ]
}

/// Deformed vertex positions of `part` exactly as the browser draws it (attached parts: plain
/// skinning + fold; unlatched / hanging panels: dent in the panel's fitted rigid frame).
/// `memory` is the part's crumple memory ([`skin::FoldMemory`]); pass the same one every frame
/// (or `None` for a single pose).
pub fn render_part(
    part: &MeshPart,
    pose: &Pose,
    memory: Option<&mut skin::FoldMemory>,
) -> Vec<[f32; 3]> {
    render_detail(part, pose, memory).0
}

/// [`render_part`] plus, per vertex, the fold's `[rise, leading-end pull-back]` (m).
pub fn render_detail(
    part: &MeshPart,
    pose: &Pose,
    memory: Option<&mut skin::FoldMemory>,
) -> (Vec<[f32; 3]>, Vec<[f32; 2]>) {
    let state = part.region.and_then(|r| pose.state_of(r));
    let free = matches!(
        state,
        Some(PanelState::Unlatched | PanelState::HangingByOne)
    );
    let frame = match (free, part.region) {
        (true, Some(region)) => Some(fit_frame(
            pose.rest,
            pose.nodes,
            span_triangle(pose.lattice, pose.rest, region),
        )),
        _ => None,
    };
    // Dent in the panel frame: the nodes taken back through the frame.
    let local: Vec<[f32; 3]> = match frame {
        Some(f) => {
            let rt = transpose(&f.r);
            pose.nodes
                .iter()
                .map(|&n| mat_vec(&rt, sub(n, f.t)))
                .collect()
        }
        None => pose.nodes.to_vec(),
    };
    let lead_pos = |n: u16| pose.lead_pos(n);
    let (moved, detail) =
        skin::deform_detail(part, pose.rest, &local, pose.nodes, free, &lead_pos, memory);
    let moved = match frame {
        Some(f) => moved
            .into_iter()
            .map(|p| add(mat_vec(&f.r, p), f.t))
            .collect(),
        None => moved,
    };
    (moved, detail)
}

/// How one part looks after a hit.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PartCrumple {
    /// Largest vertex displacement from rest, m.
    pub max_disp_m: f32,
    /// Mean vertex displacement from rest, m.
    pub mean_disp_m: f32,
    /// Largest fold rise (buckle height along the fold direction), m.
    pub max_fold_m: f32,
    /// Largest leading-end pull-back toward the fixed end (following the crushed front), m.
    pub max_pull_m: f32,
    /// Fold lines: ridges along the part's long axis where the surface bends ≥
    /// [`FOLD_LINE_DEG`] more (convex) than at rest, counted on the line with the most.
    pub fold_lines: u32,
    /// Leading edge vs. the crushed front: how far (m, toward the fixed end is negative) the
    /// part's leading-edge vertices moved, minus how far the front-structure nodes it follows
    /// (its fold `leads`, a hanging bumper's taken back to its attached pose) moved, along the
    /// part's long axis. Positive
    /// = the panel floats ahead of a crushed front. 0 without a fold binding or while the panel
    /// hangs off one mount.
    pub lead_gap_m: f32,
}

/// Bend (degrees, convex) that counts as a fold line in [`PartCrumple::fold_lines`].
pub const FOLD_LINE_DEG: f32 = 8.0;

/// Measure `part` drawn as `moved` with fold `detail` (both from [`render_detail`]).
pub fn measure(
    part: &MeshPart,
    moved: &[[f32; 3]],
    detail: &[[f32; 2]],
    pose: &Pose,
) -> PartCrumple {
    let n = part.vertex_count();
    let mut out = PartCrumple::default();
    if n == 0 {
        return out;
    }
    let mut sum = 0.0;
    for (v, m) in moved.iter().enumerate() {
        let d = len(sub(*m, part.vertex(v)));
        sum += d;
        out.max_disp_m = out.max_disp_m.max(d);
    }
    out.max_fold_m = detail.iter().map(|d| d[0]).fold(0.0, f32::max);
    out.max_pull_m = detail.iter().map(|d| d[1]).fold(0.0, f32::max);
    out.mean_disp_m = sum / n as f32;
    out.fold_lines = fold_lines(part, moved);
    out.lead_gap_m = lead_gap(part, moved, pose);
    out
}

/// Fold lines along +Y: for each column of vertices (same rest x, or same rest z for side
/// parts), the convex ridges where the drawn profile bends ≥ [`FOLD_LINE_DEG`] more than at
/// rest. Returns the most found on any column.
pub fn fold_lines(part: &MeshPart, moved: &[[f32; 3]]) -> u32 {
    let n = part.vertex_count();
    // Lids (hood, trunk) are wide and flat: their surface normal is ~z and columns run at a
    // fixed x. Side parts (fenders) are thin in x: normal ~x, columns at a fixed z. Columns are
    // 20 mm bins of the across coordinate.
    let extent = |k: usize| {
        let (lo, hi) = (0..n).fold((i32::MAX, i32::MIN), |a, v| {
            let x = part.positions_mm[v * 3 + k];
            (a.0.min(x), a.1.max(x))
        });
        hi - lo
    };
    let (across, up) = if extent(0) >= extent(2) {
        (0, 2)
    } else {
        (2, 0)
    };
    let mut columns: std::collections::BTreeMap<i32, Vec<usize>> = Default::default();
    for v in 0..n {
        columns
            .entry(part.positions_mm[v * 3 + across].div_euclid(20))
            .or_default()
            .push(v);
    }
    let mut best = 0;
    for (_, mut col) in columns {
        col.sort_by_key(|&v| (part.positions_mm[v * 3 + 1], v));
        col.dedup_by_key(|v| part.positions_mm[*v * 3 + 1]);
        if col.len() < 5 {
            continue;
        }
        // Profile angle (y, up) per segment, rest vs drawn.
        let ang = |p: [f32; 3], q: [f32; 3]| (q[up] - p[up]).atan2(q[1] - p[1]);
        let mut bends = Vec::new();
        for w in col.windows(3) {
            let r0 = ang(part.vertex(w[0]), part.vertex(w[1]));
            let r1 = ang(part.vertex(w[1]), part.vertex(w[2]));
            let m0 = ang(moved[w[0]], moved[w[1]]);
            let m1 = ang(moved[w[1]], moved[w[2]]);
            // Convex bend (ridge, rising then falling) beyond the rest bend. Sign by the side's
            // outward direction for side parts (x < 0 → outward is −x).
            let sign = if up == 0 && part.vertex(w[1])[0] < 0.0 {
                -1.0
            } else {
                1.0
            };
            bends.push(sign * ((m0 - m1) - (r0 - r1)).to_degrees());
        }
        let mut lines = 0;
        let mut i = 0;
        while i < bends.len() {
            if bends[i] >= FOLD_LINE_DEG {
                lines += 1;
                // One ridge may spread over neighbouring rows.
                while i < bends.len() && bends[i] >= FOLD_LINE_DEG * 0.5 {
                    i += 1;
                }
            }
            i += 1;
        }
        best = best.max(lines);
    }
    best
}

/// See [`PartCrumple::lead_gap_m`].
fn lead_gap(part: &MeshPart, moved: &[[f32; 3]], pose: &Pose) -> f32 {
    let Some(f) = &part.fold else {
        return 0.0;
    };
    // A panel hanging off one mount has no leading edge on the structure any more.
    if part.region.and_then(|r| pose.state_of(r)) == Some(PanelState::HangingByOne) {
        return 0.0;
    }
    // Long axis: the first chord's rest direction (fixed → leading).
    let [a, b] = f.chords[0];
    let ru = sub(pose.rest[b as usize], pose.rest[a as usize]);
    let u = norm(ru);
    let n = part.vertex_count();
    let lead_most = (0..n)
        .map(|v| dot(part.vertex(v), u))
        .fold(f32::MIN, f32::max);
    let edge: Vec<usize> = (0..n)
        .filter(|&v| dot(part.vertex(v), u) > lead_most - 0.03)
        .collect();
    let edge_d = edge
        .iter()
        .map(|&v| dot(sub(moved[v], part.vertex(v)), u))
        .sum::<f32>()
        / edge.len().max(1) as f32;
    let at: Vec<(usize, [f32; 3])> = f
        .leads
        .iter()
        .flatten()
        .filter_map(|&l| pose.lead_pos(l).map(|p| (l as usize, p)))
        .collect();
    if at.is_empty() {
        return 0.0;
    }
    let lead_d = at
        .iter()
        .map(|&(l, p)| dot(sub(p, pose.rest[l]), u))
        .sum::<f32>()
        / at.len() as f32;
    edge_d - lead_d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tent_is_one_fold_line() {
        let root = crate::repo_root();
        let mesh = crate::load_mesh(&crate::mesh_path(&root, "debug_sedan")).unwrap();
        let hood = mesh.parts.iter().find(|p| p.name == "hood").unwrap();
        let moved: Vec<[f32; 3]> = (0..hood.vertex_count())
            .map(|v| {
                let mut p = hood.vertex(v);
                p[2] += 0.2 * hood.fold.as_ref().unwrap().weights[v * 3] as f32 / 255.0;
                p
            })
            .collect();
        assert!(
            fold_lines(hood, &moved) >= 1,
            "{}",
            fold_lines(hood, &moved)
        );
    }
}
