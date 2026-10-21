//! Planarisation: street lines → a planar street graph.
//!
//! Every crossing of two lines outside their grade-separated ranges becomes a junction;
//! crossings closer than [`MERGE_M`] share one junction (forced terminal points win the
//! position). Lines are then split at their junctions into edges. A cleanup loop removes
//! self loops, merges junctions joined by very short edges, drops duplicate edges, short
//! dangling stubs and edges meeting at a knife angle, and finally keeps the largest
//! connected component. One-way edges that would trap traffic become two-way.

use std::collections::BTreeMap;

use super::geo::{self, cum2, SegGrid};
use super::lines::{Line, Sep};
use super::style::Style;
use crate::geom::{self, V2};

/// Crossings closer than this share one junction, m.
pub const MERGE_M: f32 = 30.0;
/// Edges shorter than this collapse into one junction, m.
pub const MIN_EDGE_M: f32 = 42.0;
/// Dangling stubs shorter than this are removed (unless the line keeps its ends), m.
pub const MIN_STUB_M: f32 = 70.0;
/// Edges leaving a junction closer than this angle: the weaker one is removed, rad.
pub const MIN_ANGLE: f32 = 0.42;

/// A street graph node.
#[derive(Debug, Clone)]
pub struct GNode {
    /// Position, m.
    pub pos: V2,
    /// Forced (ramp terminal): never moved by merging.
    pub forced: bool,
}

/// A street graph edge (one future builder road).
#[derive(Debug, Clone)]
pub struct GEdge {
    /// Start node.
    pub a: usize,
    /// End node.
    pub b: usize,
    /// Centreline a → b (ends on the node positions).
    pub path: Vec<V2>,
    /// Style.
    pub st: Style,
    /// Name.
    pub name: &'static str,
    /// Grade-separated range covering this whole edge, re-based to the edge.
    pub sep: Option<Sep>,
    /// Index of the line it came from.
    pub line: usize,
    /// Arc range on that line.
    pub on_line: (f32, f32),
    /// Keeps dangling ends.
    pub keep_ends: bool,
    /// Gate access: (site, gate) when the edge's end `b` is the gate.
    pub gate: Option<(usize, usize)>,
    /// Removed by cleanup.
    pub dead: bool,
}

impl GEdge {
    /// Length, m.
    pub fn len(&self) -> f32 {
        geo::length(&self.path)
    }

    /// Whether the edge has no geometry.
    pub fn is_empty(&self) -> bool {
        self.path.len() < 2
    }
}

/// The planar street graph.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    /// Nodes.
    pub nodes: Vec<GNode>,
    /// Edges (dead ones removed at the end).
    pub edges: Vec<GEdge>,
}

#[derive(Debug, Clone, Copy)]
struct Event {
    line: usize,
    s: f32,
    pos: V2,
}

/// Build the planar graph from `lines` (see the module docs).
pub fn planarize(lines: &[Line], lo: V2, hi: V2) -> Graph {
    let mut grid = SegGrid::new(lo, hi, 120.0);
    for (i, l) in lines.iter().enumerate() {
        grid.insert_line(i as u32, &l.path, 0.0);
    }
    // 1. Pairwise events (crossings) and per-line events (ends, forced points, sep bounds).
    let mut pairs: Vec<(Event, Event)> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        for k in 0..l.path.len() - 1 {
            let (a, b) = (l.path[k], l.path[k + 1]);
            let lo = [a[0].min(b[0]), a[1].min(b[1])];
            let hi = [a[0].max(b[0]), a[1].max(b[1])];
            for (j, m) in grid.query(lo, hi) {
                let j = j as usize;
                if j <= i {
                    continue;
                }
                let o = &lines[j];
                let m = m as usize;
                let Some((t, u)) = geo::seg_cross(a, b, o.path[m], o.path[m + 1]) else {
                    continue;
                };
                let si = l.cum[k] + t * (l.cum[k + 1] - l.cum[k]);
                let sj = o.cum[m] + u * (o.cum[m + 1] - o.cum[m]);
                if l.sep_at(si).is_some() || o.sep_at(sj).is_some() {
                    continue; // grade separation, no junction
                }
                let p = geom::add(a, geom::scale(geom::sub(b, a), t));
                pairs.push((
                    Event {
                        line: i,
                        s: si,
                        pos: p,
                    },
                    Event {
                        line: j,
                        s: sj,
                        pos: p,
                    },
                ));
            }
        }
    }
    // T-snaps: a line end within 30 m of another line's interior joins it.
    for (i, l) in lines.iter().enumerate() {
        for (s_end, e) in [(0.0, l.path[0]), (l.len(), l.path[l.path.len() - 1])] {
            if l.gate.is_some() && s_end > 0.0 {
                continue;
            }
            let near = grid.query([e[0] - 30.0, e[1] - 30.0], [e[0] + 30.0, e[1] + 30.0]);
            let mut best: Option<(f32, usize, f32, V2)> = None;
            for (j, m) in near {
                let j = j as usize;
                if j == i {
                    continue;
                }
                let o = &lines[j];
                let m = m as usize;
                let (a, b) = (o.path[m], o.path[m + 1]);
                let ab = geom::sub(b, a);
                let l2 = geom::dot(ab, ab).max(1e-9);
                let t = (geom::dot(geom::sub(e, a), ab) / l2).clamp(0.0, 1.0);
                let q = geom::add(a, geom::scale(ab, t));
                let d = geom::dist(e, q);
                if d < 30.0 && best.is_none_or(|bb| d < bb.0) {
                    best = Some((d, j, o.cum[m] + t * (o.cum[m + 1] - o.cum[m]), q));
                }
            }
            if let Some((_, j, sj, q)) = best {
                if lines[j].sep_at(sj).is_some() {
                    continue;
                }
                pairs.push((
                    Event {
                        line: i,
                        s: s_end,
                        pos: q,
                    },
                    Event {
                        line: j,
                        s: sj,
                        pos: q,
                    },
                ));
            }
        }
    }
    let mut singles: Vec<Event> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        for f in &l.forced {
            singles.push(Event {
                line: i,
                s: *f,
                pos: l.at(*f).0,
            });
        }
        for r in &l.seps {
            for s in [r.s0, r.s1] {
                if s > 0.5 && s < l.len() - 0.5 {
                    singles.push(Event {
                        line: i,
                        s,
                        pos: l.at(s).0,
                    });
                }
            }
        }
    }
    // 2. Clusters: forced events first (they own their position), then crossings.
    let mut clusters: Vec<(V2, bool, usize)> = Vec::new(); // pos, forced, members
    let mut cgrid: BTreeMap<(i32, i32), Vec<usize>> = BTreeMap::new();
    let key = |p: V2| {
        (
            (p[0] / MERGE_M).floor() as i32,
            (p[1] / MERGE_M).floor() as i32,
        )
    };
    let find = |clusters: &Vec<(V2, bool, usize)>,
                cgrid: &BTreeMap<(i32, i32), Vec<usize>>,
                p: V2|
     -> Option<usize> {
        let k = key(p);
        let mut best: Option<(f32, usize)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(v) = cgrid.get(&(k.0 + dx, k.1 + dy)) {
                    for &c in v {
                        let d = geom::dist(clusters[c].0, p);
                        if d < MERGE_M && best.is_none_or(|b| d < b.0) {
                            best = Some((d, c));
                        }
                    }
                }
            }
        }
        best.map(|b| b.1)
    };
    let mut attach: Vec<Vec<(f32, usize)>> = vec![Vec::new(); lines.len()];
    for e in &singles {
        let c = match find(&clusters, &cgrid, e.pos) {
            Some(c) if !clusters[c].1 => {
                clusters[c].0 = e.pos;
                clusters[c].1 = true;
                c
            }
            Some(c) => c,
            None => {
                clusters.push((e.pos, true, 0));
                cgrid
                    .entry(key(e.pos))
                    .or_default()
                    .push(clusters.len() - 1);
                clusters.len() - 1
            }
        };
        clusters[c].2 += 1;
        attach[e.line].push((e.s, c));
    }
    for (ea, eb) in &pairs {
        let c = match find(&clusters, &cgrid, ea.pos) {
            Some(c) => {
                if !clusters[c].1 {
                    // Running mean of the member positions.
                    let n = clusters[c].2 as f32;
                    let p = clusters[c].0;
                    clusters[c].0 = [
                        (p[0] * n + ea.pos[0]) / (n + 1.0),
                        (p[1] * n + ea.pos[1]) / (n + 1.0),
                    ];
                }
                c
            }
            None => {
                clusters.push((ea.pos, false, 0));
                cgrid
                    .entry(key(ea.pos))
                    .or_default()
                    .push(clusters.len() - 1);
                clusters.len() - 1
            }
        };
        clusters[c].2 += 1;
        attach[ea.line].push((ea.s, c));
        attach[eb.line].push((eb.s, c));
    }
    let mut g = Graph {
        nodes: clusters
            .iter()
            .map(|c| GNode {
                pos: c.0,
                forced: c.1,
            })
            .collect(),
        edges: Vec::new(),
    };
    // 3. Split lines into edges.
    for (i, l) in lines.iter().enumerate() {
        let mut at = std::mem::take(&mut attach[i]);
        at.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let len = l.len();
        if at.first().is_none_or(|f| f.0 > 0.5) {
            g.nodes.push(GNode {
                pos: l.path[0],
                forced: false,
            });
            at.insert(0, (0.0, g.nodes.len() - 1));
        }
        if at.last().is_none_or(|f| f.0 < len - 0.5) {
            g.nodes.push(GNode {
                pos: l.path[l.path.len() - 1],
                forced: l.gate.is_some(),
            });
            at.push((len, g.nodes.len() - 1));
        }
        // Collapse repeats of the same cluster in a row, keeping the attachment nearest to
        // the cluster's position.
        let mut col: Vec<(f32, usize)> = Vec::new();
        for x in at {
            if let Some(last) = col.last_mut() {
                if last.1 == x.1 {
                    let c = g.nodes[x.1].pos;
                    if geom::dist(l.at(x.0).0, c) < geom::dist(l.at(last.0).0, c) {
                        *last = x;
                    }
                    continue;
                }
            }
            col.push(x);
        }
        let at = col;
        for w in at.windows(2) {
            let ((s0, n0), (s1, n1)) = (w[0], w[1]);
            if s1 - s0 < 0.5 {
                continue;
            }
            let mid = 0.5 * (s0 + s1);
            let sep = l.sep_at(mid).map(|r| r.shifted(s0));
            let piece = geo::sub_path(&l.path, &l.cum, s0, s1);
            let gate = (l.gate.is_some() && s1 >= len - 0.5)
                .then_some(l.gate)
                .flatten();
            g.edges.push(GEdge {
                a: n0,
                b: n1,
                path: piece,
                st: l.st,
                name: l.name,
                sep,
                line: i,
                on_line: (s0, s1),
                keep_ends: l.keep_ends || l.gate.is_some(),
                gate,
                dead: false,
            });
        }
    }
    g.snap_paths();
    g.cleanup();
    g
}

impl Graph {
    /// Node degree (live edges).
    pub fn degrees(&self) -> Vec<usize> {
        let mut d = vec![0; self.nodes.len()];
        for e in self.edges.iter().filter(|e| !e.dead) {
            d[e.a] += 1;
            d[e.b] += 1;
        }
        d
    }

    /// Re-snap every edge's end points onto its node positions (dropping interior points that
    /// end up behind the node).
    fn snap_paths(&mut self) {
        let nodes = &self.nodes;
        for e in self.edges.iter_mut().filter(|e| !e.dead) {
            let (pa, pb) = (nodes[e.a].pos, nodes[e.b].pos);
            let mut p = std::mem::take(&mut e.path);
            let n = p.len();
            let mut inner: Vec<V2> = p[1..n - 1].to_vec();
            // Drop interior points within 10 m of either new end, and any that would make
            // the path double back from a moved end (a node merged along the line).
            inner.retain(|q| geom::dist(*q, pa) > 10.0 && geom::dist(*q, pb) > 10.0);
            let behind =
                |end: V2, a: V2, b: V2| geom::dot(geom::sub(a, end), geom::sub(b, a)) < 0.0;
            while inner.len() >= 2 && behind(pa, inner[0], inner[1]) {
                inner.remove(0);
            }
            while inner.len() >= 2 && {
                let m = inner.len();
                behind(pb, inner[m - 1], inner[m - 2])
            } {
                inner.pop();
            }
            p = vec![pa];
            p.extend(inner);
            p.push(pb);
            p.dedup_by(|x, y| geom::dist(*x, *y) < 0.05);
            if p.len() < 2 {
                p = vec![pa, pb];
            }
            e.path = p;
        }
    }

    /// Departure direction of edge `ei` from node `n`.
    pub fn dir_from(&self, ei: usize, n: usize) -> V2 {
        let e = &self.edges[ei];
        let p = if e.a == n {
            e.path.clone()
        } else {
            let mut q = e.path.clone();
            q.reverse();
            q
        };
        let c = cum2(&p);
        let total = c[c.len() - 1];
        let (q, _) = geo::point_on(&p, &c, total.min(12.0));
        geom::normalize(geom::sub(q, p[0]))
    }

    fn merge_nodes(&mut self, keep: usize, gone: usize) {
        for e in self.edges.iter_mut() {
            if e.a == gone {
                e.a = keep;
            }
            if e.b == gone {
                e.b = keep;
            }
        }
    }

    /// The cleanup loop (see the module docs).
    #[allow(clippy::needless_range_loop)]
    fn cleanup(&mut self) {
        for _round in 0..12 {
            let mut changed = false;
            // Self loops.
            for e in self.edges.iter_mut().filter(|e| !e.dead) {
                if e.a == e.b {
                    e.dead = true;
                    changed = true;
                }
            }
            // Short edges between distinct nodes: merge (never through a sep edge).
            let deg = self.degrees();
            let mut order: Vec<usize> = (0..self.edges.len())
                .filter(|i| !self.edges[*i].dead && self.edges[*i].sep.is_none())
                .collect();
            order.sort_by(|a, b| {
                self.edges[*a]
                    .len()
                    .total_cmp(&self.edges[*b].len())
                    .then(a.cmp(b))
            });
            let mut touched = vec![false; self.nodes.len()];
            for ei in order {
                let e = &self.edges[ei];
                if e.dead || e.len() >= MIN_EDGE_M || touched[e.a] || touched[e.b] {
                    continue;
                }
                if deg[e.a] <= 1 || deg[e.b] <= 1 {
                    continue; // a stub, handled below
                }
                let (a, b) = (e.a, e.b);
                let (fa, fb) = (self.nodes[a].forced, self.nodes[b].forced);
                if fa && fb {
                    continue;
                }
                let (keep, gone) = if fb || (!fa && deg[b] > deg[a]) {
                    (b, a)
                } else {
                    (a, b)
                };
                if !self.nodes[keep].forced {
                    let (pk, pg) = (self.nodes[keep].pos, self.nodes[gone].pos);
                    let wk = deg[keep] as f32;
                    let wg = deg[gone] as f32;
                    self.nodes[keep].pos = [
                        (pk[0] * wk + pg[0] * wg) / (wk + wg),
                        (pk[1] * wk + pg[1] * wg) / (wk + wg),
                    ];
                }
                self.edges[ei].dead = true;
                self.merge_nodes(keep, gone);
                touched[keep] = true;
                touched[gone] = true;
                changed = true;
            }
            if changed {
                self.snap_paths();
            }
            // Duplicate edges (same node pair): keep the strongest.
            let mut seen: BTreeMap<(usize, usize), usize> = BTreeMap::new();
            for ei in 0..self.edges.len() {
                if self.edges[ei].dead {
                    continue;
                }
                let e = &self.edges[ei];
                let k = (e.a.min(e.b), e.a.max(e.b));
                if let Some(&other) = seen.get(&k) {
                    let o = &self.edges[other];
                    let (r_new, r_old) = (e.st.rank(), o.st.rank());
                    // Two distinct paths between the same nodes are fine when they are far
                    // apart (a loop); only near-identical duplicates are removed.
                    let mid_e = geo::point_on(&e.path, &cum2(&e.path), e.len() * 0.5).0;
                    if geo::dist_to(&o.path, mid_e) > 40.0 {
                        continue;
                    }
                    if r_new > r_old {
                        self.edges[other].dead = true;
                        seen.insert(k, ei);
                    } else {
                        self.edges[ei].dead = true;
                    }
                    changed = true;
                } else {
                    seen.insert(k, ei);
                }
            }
            // Knife angles at a node.
            let mut inc: Vec<Vec<usize>> = vec![Vec::new(); self.nodes.len()];
            for (ei, e) in self.edges.iter().enumerate() {
                if !e.dead {
                    inc[e.a].push(ei);
                    inc[e.b].push(ei);
                }
            }
            for n in 0..self.nodes.len() {
                let list = inc[n].clone();
                for x in 0..list.len() {
                    for y in x + 1..list.len() {
                        let (ex, ey) = (list[x], list[y]);
                        if self.edges[ex].dead || self.edges[ey].dead || ex == ey {
                            continue;
                        }
                        let (dx, dy) = (self.dir_from(ex, n), self.dir_from(ey, n));
                        let ang = geom::atan2(geom::cross(dx, dy), geom::dot(dx, dy)).abs();
                        if ang < MIN_ANGLE {
                            let weak = {
                                let (a, b) = (&self.edges[ex], &self.edges[ey]);
                                if a.st.rank() != b.st.rank() {
                                    if a.st.rank() < b.st.rank() {
                                        ex
                                    } else {
                                        ey
                                    }
                                } else if a.len() < b.len() {
                                    ex
                                } else {
                                    ey
                                }
                            };
                            self.edges[weak].dead = true;
                            changed = true;
                        }
                    }
                }
            }
            // Dangling short stubs.
            loop {
                let deg = self.degrees();
                let mut removed = false;
                for e in self.edges.iter_mut().filter(|e| !e.dead) {
                    if e.keep_ends || e.sep.is_some() {
                        continue;
                    }
                    if (deg[e.a] == 1 || deg[e.b] == 1) && e.len() < MIN_STUB_M {
                        e.dead = true;
                        removed = true;
                    }
                }
                if !removed {
                    break;
                }
                changed = true;
            }
            if !changed {
                break;
            }
        }
    }

    /// Keep only the largest connected component; returns the number of edges removed.
    pub fn keep_largest_component(&mut self) -> usize {
        let n = self.nodes.len();
        let mut parent: Vec<usize> = (0..n).collect();
        fn root(p: &mut [usize], mut x: usize) -> usize {
            while p[x] != x {
                p[x] = p[p[x]];
                x = p[x];
            }
            x
        }
        for e in self.edges.iter().filter(|e| !e.dead) {
            let (ra, rb) = (root(&mut parent, e.a), root(&mut parent, e.b));
            if ra != rb {
                parent[ra.max(rb)] = ra.min(rb);
            }
        }
        let mut size: BTreeMap<usize, f32> = BTreeMap::new();
        for e in self.edges.iter().filter(|e| !e.dead) {
            *size.entry(root(&mut parent, e.a)).or_default() += e.len();
        }
        let Some((&best, _)) = size
            .iter()
            .max_by(|a, b| a.1.total_cmp(b.1).then(b.0.cmp(a.0)))
        else {
            return 0;
        };
        let mut removed = 0;
        for i in 0..self.edges.len() {
            if !self.edges[i].dead && root(&mut parent, self.edges[i].a) != best {
                self.edges[i].dead = true;
                removed += 1;
            }
        }
        removed
    }

    /// Make one-way edges two-way where they would trap traffic or end in a dead end:
    /// repeated until every node with an incoming lane has an outgoing one and the directed
    /// graph is strongly connected.
    pub fn fix_one_way(&mut self) -> usize {
        let mut fixed = 0;
        for _ in 0..50 {
            let n = self.nodes.len();
            let deg = self.degrees();
            // Directed adjacency.
            let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
            let mut inn: Vec<Vec<usize>> = vec![Vec::new(); n];
            for e in self.edges.iter().filter(|e| !e.dead) {
                out[e.a].push(e.b);
                inn[e.b].push(e.a);
                if e.st.two_way() {
                    out[e.b].push(e.a);
                    inn[e.a].push(e.b);
                }
            }
            let mut bad: Vec<usize> = Vec::new();
            for (ei, e) in self.edges.iter().enumerate() {
                if e.dead || e.st.two_way() {
                    continue;
                }
                if deg[e.a] == 1 || deg[e.b] == 1 || out[e.b].is_empty() || inn[e.a].is_empty() {
                    bad.push(ei);
                }
            }
            if bad.is_empty() {
                // Strong connectivity: nodes not reachable both ways from a hub.
                let hub = (0..n).find(|i| deg[*i] > 0);
                let Some(hub) = hub else { break };
                let reach = |adj: &Vec<Vec<usize>>| {
                    let mut seen = vec![false; n];
                    let mut st = vec![hub];
                    seen[hub] = true;
                    while let Some(x) = st.pop() {
                        for &y in &adj[x] {
                            if !seen[y] {
                                seen[y] = true;
                                st.push(y);
                            }
                        }
                    }
                    seen
                };
                let (f, b) = (reach(&out), reach(&inn));
                for (ei, e) in self.edges.iter().enumerate() {
                    if e.dead || e.st.two_way() {
                        continue;
                    }
                    let ok = |x: usize| f[x] && b[x];
                    if !ok(e.a) || !ok(e.b) {
                        bad.push(ei);
                    }
                }
            }
            if bad.is_empty() {
                break;
            }
            for ei in bad {
                let st = self.edges[ei].st;
                let lanes = st.fwd.div_ceil(2).max(1);
                self.edges[ei].st = Style {
                    fwd: lanes,
                    bwd: lanes,
                    ..st
                };
                fixed += 1;
            }
        }
        fixed
    }

    /// Drop dead edges and unused nodes (renumbering).
    pub fn compact(&mut self) {
        self.edges.retain(|e| !e.dead);
        let mut used = vec![false; self.nodes.len()];
        for e in &self.edges {
            used[e.a] = true;
            used[e.b] = true;
        }
        let mut map = vec![usize::MAX; self.nodes.len()];
        let mut nodes = Vec::new();
        for (i, n) in self.nodes.iter().enumerate() {
            if used[i] {
                map[i] = nodes.len();
                nodes.push(n.clone());
            }
        }
        for e in &mut self.edges {
            e.a = map[e.a];
            e.b = map[e.b];
        }
        self.nodes = nodes;
    }
}
