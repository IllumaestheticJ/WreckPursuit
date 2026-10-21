//! Terrain carving (ported from Port Vesper v2's `carve`, with a trench mode).
//!
//! The heightfield is carved [`CARVE_BELOW_M`] under every at-grade road surface (full
//! strength [`SHOULDER_M`] beyond the paved edge, smoothstep blend over 15–22 m) and the
//! junction discs; it is never left above any road. Bridge decks (any stretch whose whole
//! cross-section is more than 3 m above the natural ground) are left natural, and the ground
//! under every deck is kept at least 0.5 m below it. Where a road runs more than 2.5 m below
//! the natural ground (cuts, underpasses, cut-and-cover tunnels) the carve is a narrow
//! trench: full strength only 1 m beyond the paving and one cell of blend, so tunnel covers
//! (features) can close it.

use super::super::ground::Ground;
use super::{RoadInfo, CARVE_BELOW_M};
use crate::builder::{Built, NodeSpec};
use crate::geom::{self, V2};
use crate::surface::is_elevated;
use crate::terrain::smoothstep;

/// Full-strength carve corridor beyond the paved edge, m.
pub const SHOULDER_M: f32 = 7.5;

/// Carve `ground.hf` in place (see the module docs).
pub fn carve(ground: &mut Ground, built: &Built, nodes: &[NodeSpec], _info: &[RoadInfo]) {
    let hf = &mut ground.hf;
    let nv = hf.z.len();
    let natural = hf.z.clone();
    let cell = hf.cell_m;
    let (nx, ny) = (hf.nx as i64, hf.ny as i64);
    let origin = hf.origin;
    let nat_at = |p: V2| -> f32 {
        let fx = ((p[0] - origin[0]) / cell).clamp(0.0, (nx - 1) as f32);
        let fy = ((p[1] - origin[1]) / cell).clamp(0.0, (ny - 1) as f32);
        let i = (fx.floor() as i64).min(nx - 2);
        let j = (fy.floor() as i64).min(ny - 2);
        let (tx, ty) = (fx - i as f32, fy - j as f32);
        let at = |i: i64, j: i64| natural[(j * nx + i) as usize];
        let a = at(i, j) + (at(i + 1, j) - at(i, j)) * tx;
        let b = at(i, j + 1) + (at(i + 1, j + 1) - at(i, j + 1)) * tx;
        a + (b - a) * ty
    };
    let mut core_min = vec![f32::INFINITY; nv];
    let mut acc_w = vec![0.0f32; nv];
    let mut acc_wt = vec![0.0f32; nv];
    let mut best_d = vec![f32::INFINITY; nv];
    let mut best_t = vec![0.0f32; nv];
    let mut best_sh = vec![SHOULDER_M; nv];
    let mut best_bl = vec![15.0f32; nv];
    let mut stamp = vec![u32::MAX; nv];
    let mut touched: Vec<usize> = Vec::new();

    let finish = |touched: &mut Vec<usize>,
                  best_d: &[f32],
                  best_t: &[f32],
                  best_sh: &[f32],
                  best_bl: &[f32],
                  core_min: &mut [f32],
                  acc_w: &mut [f32],
                  acc_wt: &mut [f32]| {
        for &v in touched.iter() {
            let d = best_d[v];
            let t = best_t[v];
            let sh = best_sh[v];
            if d <= sh {
                core_min[v] = core_min[v].min(t);
            } else {
                let w = smoothstep(sh + best_bl[v], sh, d);
                acc_w[v] += w;
                acc_wt[v] += w * t;
            }
        }
        touched.clear();
    };
    #[allow(clippy::too_many_arguments)]
    fn visit(
        id: u32,
        lo: V2,
        hi: V2,
        origin: V2,
        cell: f32,
        nx: i64,
        ny: i64,
        touched: &mut Vec<usize>,
        best_d: &mut [f32],
        best_t: &mut [f32],
        best_sh: &mut [f32],
        best_bl: &mut [f32],
        stamp: &mut [u32],
        f: &mut dyn FnMut(V2) -> (f32, f32, f32, f32),
    ) {
        let i0 = (((lo[0] - origin[0]) / cell).floor() as i64).clamp(0, nx - 1);
        let i1 = (((hi[0] - origin[0]) / cell).ceil() as i64).clamp(0, nx - 1);
        let j0 = (((lo[1] - origin[1]) / cell).floor() as i64).clamp(0, ny - 1);
        let j1 = (((hi[1] - origin[1]) / cell).ceil() as i64).clamp(0, ny - 1);
        for j in j0..=j1 {
            for i in i0..=i1 {
                let v = (j * nx + i) as usize;
                let p = [origin[0] + i as f32 * cell, origin[1] + j as f32 * cell];
                let (d, t, sh, bl) = f(p);
                // Compare by how far outside the full-strength corridor the vertex is.
                let better = d - sh < best_d[v] - best_sh[v];
                if stamp[v] != id {
                    stamp[v] = id;
                    best_d[v] = d;
                    best_t[v] = t;
                    best_sh[v] = sh;
                    best_bl[v] = bl;
                    touched.push(v);
                } else if better {
                    best_d[v] = d;
                    best_t[v] = t;
                    best_sh[v] = sh;
                    best_bl[v] = bl;
                }
            }
        }
    }

    let deck: Vec<Vec<bool>> = built
        .surfaces
        .iter()
        .map(|s| {
            (0..s.centre.len())
                .map(|k| is_elevated(s, k, &nat_at, 3.0))
                .collect()
        })
        .collect();
    let mut source = 0u32;
    for (ri, s) in built.surfaces.iter().enumerate() {
        let blend = if s.highway { 22.0 } else { 15.0 };
        for k in 0..s.centre.len() - 1 {
            let (a, b) = (s.centre[k], s.centre[k + 1]);
            if deck[ri][k] || deck[ri][k + 1] {
                continue;
            }
            let (pa, pb) = (geom::xy(a), geom::xy(b));
            let depth = nat_at(pa) - a[2];
            let trench = depth > 2.5 || nat_at(pb) - b[2] > 2.5;
            let (sh, bl) = if trench {
                (cell + 0.5, cell * 1.2)
            } else {
                (SHOULDER_M, blend)
            };
            let reach = s.left_m.max(s.right_m) + sh + bl;
            let lo = [pa[0].min(pb[0]) - reach, pa[1].min(pb[1]) - reach];
            let hi = [pa[0].max(pb[0]) + reach, pa[1].max(pb[1]) + reach];
            let ab = geom::sub(pb, pa);
            let l = geom::len(ab).max(1e-4);
            let dir = geom::scale(ab, 1.0 / l);
            let (bank_a, bank_b) = (s.bank[k], s.bank[k + 1]);
            // Steep roads: a little deeper, so the bilinear ground between vertices never
            // pokes through the paving edge.
            let slope = ((b[2] - a[2]) / l).abs() + bank_a.abs().max(bank_b.abs());
            let extra = (slope - 0.03).max(0.0) * cell * 0.6;
            let mut f = |p: V2| {
                let w = geom::sub(p, pa);
                let along = geom::dot(w, dir);
                let t = (along / l).clamp(0.0, 1.0);
                let lat = geom::dot(w, geom::right_of(dir));
                let edge = if lat > 0.0 { s.right_m } else { s.left_m };
                let d_out = (lat.abs() - edge).max(0.0);
                let d_end = (-along).max(along - l).max(0.0);
                let d = (d_out * d_out + d_end * d_end).sqrt();
                let zc = a[2] + (b[2] - a[2]) * t;
                let bank = bank_a + (bank_b - bank_a) * t;
                let tz = zc + bank * lat.clamp(-s.left_m, s.right_m) - CARVE_BELOW_M - extra;
                (d, tz, sh, bl)
            };
            visit(
                source,
                lo,
                hi,
                origin,
                cell,
                nx,
                ny,
                &mut touched,
                &mut best_d,
                &mut best_t,
                &mut best_sh,
                &mut best_bl,
                &mut stamp,
                &mut f,
            );
        }
        finish(
            &mut touched,
            &best_d,
            &best_t,
            &best_sh,
            &best_bl,
            &mut core_min,
            &mut acc_w,
            &mut acc_wt,
        );
        source += 1;
    }
    // Junction and bend areas: a disc per node reaching the trimmed road ends.
    let mut radius = vec![0.0f32; nodes.len()];
    for s in &built.surfaces {
        let Some(lane0) = built.lanes.iter().find(|l| l.road == s.road) else {
            continue;
        };
        let (f, t) = (lane0.from_node as usize, lane0.to_node as usize);
        let c0 = geom::xy(s.centre[0]);
        let (na, nb) = if geom::dist(c0, nodes[f].pos) <= geom::dist(c0, nodes[t].pos) {
            (f, t)
        } else {
            (t, f)
        };
        let w = s.left_m.max(s.right_m);
        let e = geom::xy(s.centre[s.centre.len() - 1]);
        let ka = 0;
        let kb = s.centre.len() - 1;
        let si = s.road as usize;
        let _ = si;
        if !is_elevated(s, ka, &nat_at, 3.0) {
            radius[na] = radius[na].max(geom::dist(c0, nodes[na].pos) + w);
        }
        if !is_elevated(s, kb, &nat_at, 3.0) {
            radius[nb] = radius[nb].max(geom::dist(e, nodes[nb].pos) + w);
        }
    }
    for ix in &built.intersections {
        let n = ix.node as usize;
        if ix.radius_m > 0.0 && nodes[n].z - nat_at(nodes[n].pos) < 3.0 {
            radius[n] = radius[n].max(ix.radius_m);
        }
    }
    for (n, node) in nodes.iter().enumerate() {
        if radius[n] <= 0.0 || node.z - nat_at(node.pos) > 3.0 {
            continue;
        }
        let blend = 15.0;
        let r = radius[n];
        let reach = r + SHOULDER_M + blend;
        let c = node.pos;
        let lo = [c[0] - reach, c[1] - reach];
        let hi = [c[0] + reach, c[1] + reach];
        let z = node.z - CARVE_BELOW_M;
        let mut f = |p: V2| ((geom::dist(p, c) - r).max(0.0), z, SHOULDER_M, blend);
        visit(
            source,
            lo,
            hi,
            origin,
            cell,
            nx,
            ny,
            &mut touched,
            &mut best_d,
            &mut best_t,
            &mut best_sh,
            &mut best_bl,
            &mut stamp,
            &mut f,
        );
        finish(
            &mut touched,
            &best_d,
            &best_t,
            &best_sh,
            &best_bl,
            &mut core_min,
            &mut acc_w,
            &mut acc_wt,
        );
        source += 1;
    }
    // Under bridge decks (and next to them) the ground stays at least 0.5 m below the deck.
    let mut under = vec![f32::INFINITY; nv];
    for (ri, s) in built.surfaces.iter().enumerate() {
        for k in 0..s.centre.len() - 1 {
            if !(deck[ri][k] || deck[ri][k + 1]) {
                continue;
            }
            let (a, b) = (s.centre[k], s.centre[k + 1]);
            let (pa, pb) = (geom::xy(a), geom::xy(b));
            let reach = s.left_m.max(s.right_m) + 1.0;
            let lo = [pa[0].min(pb[0]) - reach, pa[1].min(pb[1]) - reach];
            let hi = [pa[0].max(pb[0]) + reach, pa[1].max(pb[1]) + reach];
            let ab = geom::sub(pb, pa);
            let l = geom::len(ab).max(1e-4);
            let dir = geom::scale(ab, 1.0 / l);
            let i0 = (((lo[0] - origin[0]) / cell).floor() as i64).clamp(0, nx - 1);
            let i1 = (((hi[0] - origin[0]) / cell).ceil() as i64).clamp(0, nx - 1);
            let j0 = (((lo[1] - origin[1]) / cell).floor() as i64).clamp(0, ny - 1);
            let j1 = (((hi[1] - origin[1]) / cell).ceil() as i64).clamp(0, ny - 1);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let p = [origin[0] + i as f32 * cell, origin[1] + j as f32 * cell];
                    let w = geom::sub(p, pa);
                    let along = geom::dot(w, dir);
                    let lat = geom::dot(w, geom::right_of(dir));
                    if along < -cell
                        || along > l + cell
                        || lat < -s.left_m - cell
                        || lat > s.right_m + cell
                    {
                        continue;
                    }
                    let t = (along / l).clamp(0.0, 1.0);
                    let lat = lat.clamp(-s.left_m, s.right_m);
                    let z = a[2]
                        + (b[2] - a[2]) * t
                        + (s.bank[k] + (s.bank[k + 1] - s.bank[k]) * t) * lat;
                    let v = (j * nx + i) as usize;
                    under[v] = under[v].min(z - 0.5);
                }
            }
        }
    }
    for v in 0..nv {
        hf.z[v] = if core_min[v].is_finite() {
            core_min[v]
        } else if acc_w[v] <= 1.0 {
            natural[v] + acc_wt[v] - acc_w[v] * natural[v]
        } else {
            acc_wt[v] / acc_w[v]
        };
        hf.z[v] = hf.z[v].min(under[v]);
        hf.z[v] = (hf.z[v] * 1000.0).round() / 1000.0;
    }
}
