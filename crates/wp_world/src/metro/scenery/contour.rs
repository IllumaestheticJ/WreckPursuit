//! Marching squares on a regular vertex grid: oriented iso-lines (the "inside", values above
//! the iso level, is always on the left of the travel direction), chained into polylines and
//! cut into straight pieces for walls, railings and hedges.

use crate::geom::{self, V2};

use super::super::ground::seg_dist;

/// A scalar field sampled on a regular vertex grid (`v[j * nx + i]` at
/// `origin + (i, j) * cell`).
pub struct Grid<'a> {
    /// Vertices along X.
    pub nx: usize,
    /// Vertices along Y.
    pub ny: usize,
    /// World position of vertex (0, 0), m.
    pub origin: V2,
    /// Vertex spacing, m.
    pub cell: f32,
    /// Values, row-major.
    pub v: &'a [f32],
}

impl Grid<'_> {
    fn val(&self, i: usize, j: usize) -> f32 {
        self.v[j * self.nx + i]
    }

    fn pos(&self, i: usize, j: usize) -> V2 {
        [
            self.origin[0] + i as f32 * self.cell,
            self.origin[1] + j as f32 * self.cell,
        ]
    }

    /// Crossing point on a grid edge (`id` = 2 × vertex + 0 for the +X edge, + 1 for +Y).
    fn edge_point(&self, id: u64, iso: f32) -> V2 {
        let v = (id / 2) as usize;
        let (i, j) = (v % self.nx, v / self.nx);
        let (i2, j2) = if id.is_multiple_of(2) {
            (i + 1, j)
        } else {
            (i, j + 1)
        };
        let (a, b) = (self.val(i, j), self.val(i2, j2));
        let t = if (b - a).abs() > 1e-9 {
            ((iso - a) / (b - a)).clamp(0.0, 1.0)
        } else {
            0.5
        };
        let (p, q) = (self.pos(i, j), self.pos(i2, j2));
        [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]
    }
}

/// Iso-lines of `g` at level `iso` as polylines; values `> iso` lie on the left of each
/// line. Lines are closed loops (first point repeated at the end) or end at the grid border.
pub fn contours(g: &Grid, iso: f32) -> Vec<Vec<V2>> {
    let nx = g.nx;
    let hid = |i: usize, j: usize| ((j * nx + i) * 2) as u64;
    let vid = |i: usize, j: usize| ((j * nx + i) * 2 + 1) as u64;
    // Oriented segments (from edge, to edge).
    let mut segs: Vec<(u64, u64)> = Vec::new();
    for j in 0..g.ny.saturating_sub(1) {
        for i in 0..nx.saturating_sub(1) {
            let c = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
            let vals = c.map(|(a, b)| g.val(a, b));
            let ins = vals.map(|v| v > iso);
            if ins.iter().all(|x| *x) || ins.iter().all(|x| !*x) {
                continue;
            }
            // Cell edges walked counter-clockwise: bottom, right, top, left.
            let eids = [hid(i, j), vid(i + 1, j), hid(i, j + 1), vid(i, j)];
            let mut xs: [(u64, bool); 4] = [(0, false); 4];
            let mut n = 0;
            for e in 0..4 {
                let (a, b) = (ins[e], ins[(e + 1) % 4]);
                if a != b {
                    // Exit = leaving the inside while walking counter-clockwise.
                    xs[n] = (eids[e], a);
                    n += 1;
                }
            }
            if n == 2 {
                let (ex, en) = if xs[0].1 {
                    (xs[0].0, xs[1].0)
                } else {
                    (xs[1].0, xs[0].0)
                };
                segs.push((ex, en));
            } else if n == 4 {
                // Saddle: decide by the cell centre.
                let centre_in = (vals[0] + vals[1] + vals[2] + vals[3]) * 0.25 > iso;
                for q in 0..4 {
                    if xs[q].1 {
                        let partner = if centre_in {
                            xs[(q + 1) % 4].0
                        } else {
                            xs[(q + 3) % 4].0
                        };
                        segs.push((xs[q].0, partner));
                    }
                }
            }
        }
    }
    // Chain: successor of a segment = the segment starting where it ends.
    let mut by_from: Vec<(u64, u32)> = segs
        .iter()
        .enumerate()
        .map(|(k, s)| (s.0, k as u32))
        .collect();
    by_from.sort_unstable();
    let mut by_to: Vec<(u64, u32)> = segs
        .iter()
        .enumerate()
        .map(|(k, s)| (s.1, k as u32))
        .collect();
    by_to.sort_unstable();
    let find = |v: &[(u64, u32)], id: u64| -> Option<usize> {
        let k = v.partition_point(|x| x.0 < id);
        (k < v.len() && v[k].0 == id).then(|| v[k].1 as usize)
    };
    let mut visited = vec![false; segs.len()];
    let mut out = Vec::new();
    for s0 in 0..segs.len() {
        if visited[s0] {
            continue;
        }
        // Walk back to the start of an open chain (or once round a loop).
        let mut start = s0;
        while let Some(p) = find(&by_to, segs[start].0) {
            if p == s0 || visited[p] {
                break;
            }
            start = p;
        }
        let mut line = vec![g.edge_point(segs[start].0, iso)];
        let mut cur = start;
        loop {
            visited[cur] = true;
            line.push(g.edge_point(segs[cur].1, iso));
            match find(&by_from, segs[cur].1) {
                Some(nx_) if !visited[nx_] => cur = nx_,
                _ => break,
            }
        }
        if line.len() >= 2 {
            out.push(line);
        }
    }
    out
}

/// Cut a polyline into straight pieces: each piece spans as many vertices as stay within
/// `tol` of its chord and at most `max_len` long (longer single segments are split).
pub fn pieces(line: &[V2], tol: f32, max_len: f32) -> Vec<(V2, V2)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < line.len() {
        let mut j = i + 1;
        while j + 1 < line.len() {
            let cand = j + 1;
            if geom::dist(line[i], line[cand]) > max_len {
                break;
            }
            if (i + 1..cand).any(|k| seg_dist(line[i], line[cand], line[k]) > tol) {
                break;
            }
            j = cand;
        }
        let (a, b) = (line[i], line[j]);
        let len = geom::dist(a, b);
        if len > 0.05 {
            let n = (len / max_len).ceil().max(1.0) as usize;
            for k in 0..n {
                let t0 = k as f32 / n as f32;
                let t1 = (k + 1) as f32 / n as f32;
                out.push((lerp(a, b, t0), lerp(a, b, t1)));
            }
        }
        i = j;
    }
    out
}

/// Sub-segments of `a`–`b` where `ok` holds at every sample (every ≤ `step` m); runs
/// shorter than `min_len` are dropped.
pub fn runs(a: V2, b: V2, step: f32, min_len: f32, ok: impl Fn(V2) -> bool) -> Vec<(V2, V2)> {
    let len = geom::dist(a, b);
    let n = (len / step).ceil().max(1.0) as usize;
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let push = |s: usize, e: usize, out: &mut Vec<(V2, V2)>| {
        let (p, q) = (
            lerp(a, b, s as f32 / n as f32),
            lerp(a, b, e as f32 / n as f32),
        );
        if geom::dist(p, q) >= min_len {
            out.push((p, q));
        }
    };
    for k in 0..=n {
        let good = ok(lerp(a, b, k as f32 / n as f32));
        match (good, start) {
            (true, None) => start = Some(k),
            (false, Some(s)) => {
                if k > s + 1 {
                    push(s, k - 1, &mut out);
                }
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        if n > s {
            push(s, n, &mut out);
        }
    }
    out
}

/// Linear interpolation.
pub fn lerp(a: V2, b: V2, t: f32) -> V2 {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disc_gives_one_counter_clockwise_loop() {
        let n = 21;
        let v: Vec<f32> = (0..n * n)
            .map(|k| {
                let (i, j) = ((k % n) as f32 - 10.0, (k / n) as f32 - 10.0);
                25.0 - (i * i + j * j)
            })
            .collect();
        let g = Grid {
            nx: n,
            ny: n,
            origin: [0.0, 0.0],
            cell: 1.0,
            v: &v,
        };
        let ls = contours(&g, 0.0);
        assert_eq!(ls.len(), 1);
        let l = &ls[0];
        assert!(geom::dist(l[0], *l.last().unwrap()) < 1e-4, "closed");
        let mut area = 0.0;
        for w in l.windows(2) {
            area += w[0][0] * w[1][1] - w[1][0] * w[0][1];
        }
        assert!(area > 0.0, "inside on the left = counter-clockwise");
        for p in l {
            let r = geom::dist(*p, [10.0, 10.0]);
            assert!((r - 5.0).abs() < 0.3, "{r}");
        }
        let ps = pieces(l, 0.2, 3.0);
        assert!(ps.iter().all(|(a, b)| geom::dist(*a, *b) <= 3.0 + 1e-3));
    }

    #[test]
    fn runs_skip_blocked_samples() {
        let r = runs([0.0, 0.0], [10.0, 0.0], 1.0, 1.0, |p| {
            !(4.0..=6.0).contains(&p[0])
        });
        assert_eq!(r.len(), 2);
        assert!(r[0].1[0] <= 3.01 && r[1].0[0] >= 6.99);
    }
}
