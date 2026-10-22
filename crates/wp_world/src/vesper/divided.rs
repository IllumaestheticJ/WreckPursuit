//! Open divided freeways for a [`Region`] (Ashpeak County's Route 7, Route 12 and the
//! Crosstown Expressway): Loop 9's division generalised to open lines.
//!
//! A region lays each freeway out as Highway-class drafts along its centreline (profiled like
//! Loop 9, with grade separations, tunnels and decks) and registers a [`FreewayPlan`]. After
//! profiling, [`divide_freeways`] replaces those drafts by two one-way carriageways with a
//! median barrier, split at the gores of
//! - **diamond interchanges** ([`Diamond`]): four ramps to the crossroad's terminals, laid
//!   exactly like Loop 9's (`ramp_path`, `profile_ramp`), and
//! - **links** ([`LinkPlan`]): freeway-to-freeway ramps between gores or split freeway ends,
//!   shaped by plan poses and profiled over whatever they cross (flyovers).
//!
//! A freeway end either fans both carriageways into one node (an at-grade junction there,
//! [`FreewayEnd::Fan`]) or splits them onto a node each for links ([`FreewayEnd::Split`]).

use super::*;

/// Length over which the carriageways fan in to a [`FreewayEnd::Fan`] end node, m.
pub(super) const FAN_M: f32 = 140.0;
/// Length of a link beside a carriageway from its gore (taper + parallel lane), m.
pub(super) const LINK_PARALLEL_M: f32 = 70.0;
/// Taper length of a link leaving or joining a split carriageway end, m.
pub(super) const END_TAPER_M: f32 = 30.0;
/// Height of a link deck over a road it bridges (clearance plus a margin for bank), m.
const LINK_CLEAR_M: f32 = CROSSING_CLEARANCE_M + 0.4;

/// How a freeway's carriageways end at one end of its line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FreewayEnd {
    /// Both carriageways fan in to the line's end node: an at-grade junction there.
    Fan,
    /// Each carriageway ends on its own node beside the line's end, where links attach.
    Split,
}

/// A diamond interchange on a freeway: the crossroad (a draft with `over`) bridges it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Diamond {
    /// A point on the centreline where the crossroad crosses it, m.
    pub(super) centre: V2,
    /// Terminal right of forward travel (the forward carriageway's ramps).
    pub(super) right: usize,
    /// Terminal left of forward travel.
    pub(super) left: usize,
    /// Gore distances from the centre towards lower / higher arc length, m.
    pub(super) span: [f32; 2],
}

/// An open divided freeway laid out by a region.
#[derive(Debug, Clone)]
pub(super) struct FreewayPlan {
    /// Road name of its carriageways.
    pub(super) name: &'static str,
    /// Road name of its diamond ramps.
    pub(super) ramp_name: &'static str,
    /// Its centreline drafts in order, each a → b forward (Highway class, two-way).
    pub(super) drafts: Vec<usize>,
    /// One carriageway (one-way: `fwd` lanes, `bwd` 0).
    pub(super) st: Style,
    /// At the start and the end of the line.
    pub(super) ends: [FreewayEnd; 2],
    /// Diamond interchanges.
    pub(super) diamonds: Vec<Diamond>,
}

impl FreewayPlan {
    /// Carriageway centreline offset from the line (the median barrier), m.
    pub(super) fn offset(&self) -> f32 {
        MEDIAN_SHOULDER_M + 0.5 * self.st.fwd as f32 * HW_LANE_M
    }

    /// Centre of a `lanes`-lane ramp running beside a carriageway, from its centreline, m.
    pub(super) fn lane_off(&self, lanes: u8) -> f32 {
        0.5 * (self.st.fwd + lanes) as f32 * HW_LANE_M
    }

    /// Lateral reach of the carriageways and ramp lanes beside them from the line, m.
    fn corridor(&self) -> f32 {
        self.offset() + self.lane_off(2) + HW_LANE_M + 3.0
    }
}

/// Where a link starts or ends.
#[derive(Debug, Clone, Copy)]
pub(super) enum LinkEnd {
    /// A gore on freeway `line`'s carriageway on `side` (+1 right of forward travel) nearest
    /// `at`: the link diverges there (start) or merges there (end).
    Gore { line: usize, side: f32, at: V2 },
    /// The node where freeway `line`'s carriageway on `side` ends (a [`FreewayEnd::Split`]
    /// end, `end` = the line's end rather than start). The link runs on along the line,
    /// `shift` m right of the carriageway centreline over the taper.
    LineEnd {
        line: usize,
        side: f32,
        end: bool,
        shift: f32,
    },
}

/// A freeway-to-freeway link (a system ramp).
#[derive(Debug, Clone)]
pub(super) struct LinkPlan {
    pub(super) from: LinkEnd,
    pub(super) to: LinkEnd,
    /// Plan poses (point, unit heading) it passes through between its ends.
    pub(super) poses: Vec<(V2, V2)>,
    pub(super) lanes: u8,
    /// Speed limit, m/s.
    pub(super) speed: f32,
    pub(super) name: &'static str,
}

impl LinkPlan {
    /// The link's style (one-way ramp).
    pub(super) fn style(&self) -> Style {
        Style {
            fwd: self.lanes,
            speed: self.speed,
            ..RAMP
        }
    }
}

/// Each freeway's centreline as an open line from the profiled drafts.
pub(super) fn freeway_lines(g: &Gen, specs: &[RoadSpec]) -> Vec<Ring> {
    g.freeways
        .iter()
        .map(|f| Ring::open(g, specs, &f.drafts))
        .collect()
}

/// Each freeway's centreline from its drafts' plan paths (flat; layout-time geometry).
pub(super) fn plan_lines(g: &Gen) -> Vec<Ring> {
    g.freeways
        .iter()
        .map(|f| {
            let mut pts: Vec<V2> = Vec::new();
            for &d in &f.drafts {
                let p = &g.drafts[d].path;
                let skip = usize::from(!pts.is_empty());
                pts.extend_from_slice(&p[skip..]);
            }
            Ring::from_xy(&pts)
        })
        .collect()
}

fn end_s(ring: &Ring, end: bool) -> f32 {
    if end {
        ring.total()
    } else {
        0.0
    }
}

/// Every diamond ramp of every freeway (gore nodes not created yet): (freeway, ramp).
pub(super) fn diamond_ramps(
    plans: &[FreewayPlan],
    lines: &[Ring],
    nodes: &[NodeSpec],
) -> Vec<(usize, RampPath)> {
    let mut out = Vec::new();
    for (k, f) in plans.iter().enumerate() {
        let ring = &lines[k];
        let o = f.offset();
        for d in &f.diamonds {
            let sc = ring.project(d.centre);
            for (side, term) in [(1.0f32, d.right), (-1.0, d.left)] {
                for off in [true, false] {
                    // Upstream gore for the off-ramp, downstream for the on-ramp.
                    let up = if off { -side } else { side };
                    let span = if up < 0.0 { d.span[0] } else { d.span[1] };
                    let gs = sc + up * span;
                    out.push((
                        k,
                        RampPath {
                            gore: usize::MAX,
                            terminal: term,
                            gore_p: ring.offset(gs, side * o),
                            gore_s: gs,
                            off,
                            path: ramp_path_at(
                                ring,
                                gs,
                                side,
                                off,
                                nodes[term].pos,
                                o,
                                f.lane_off(1),
                            ),
                            dir: side,
                        },
                    ));
                }
            }
        }
    }
    out
}

/// Height ranges for the freeways' diamond terminals (like [`terminal_bounds`]), added to
/// `bounds`.
pub(super) fn freeway_bounds(
    g: &Gen,
    specs: &[RoadSpec],
    opt: &BuildOptions,
    bounds: &mut BTreeMap<usize, (f32, f32)>,
) {
    if g.freeways.is_empty() {
        return;
    }
    let lines = freeway_lines(g, specs);
    let flat = flat_lengths(g, opt);
    let spread = 2.0 * smoothing_sigma(RAMP.grade, max_vertical_curvature(RAMP.speed), 4.0);
    let mut mine: BTreeMap<usize, (f32, f32)> = BTreeMap::new();
    for (k, rp) in diamond_ramps(&g.freeways, &lines, &g.nodes) {
        let c = cum2(&rp.path);
        let room = c[c.len() - 1] - RAMP_PARALLEL_M - flat[rp.terminal] - 2.0 * spread;
        let allow = RAMP.grade * room.max(20.0) * 0.8;
        let z = lines[k].plane_z(point_on(&rp.path, &c, RAMP_PARALLEL_M).0);
        let e = mine
            .entry(rp.terminal)
            .or_insert((f32::NEG_INFINITY, f32::INFINITY));
        e.0 = e.0.max(z - allow);
        e.1 = e.1.min(z + allow);
    }
    for (k, mut v) in mine {
        if v.0 > v.1 {
            let m = 0.5 * (v.0 + v.1);
            v = (m, m);
        }
        bounds.insert(k, v);
    }
}

/// Cubic Bézier from pose `(p0, d0)` to `(p1, d1)`, without its first point.
fn bezier(p0: V2, d0: V2, p1: V2, d1: V2, step: f32) -> Vec<V2> {
    let chord = geom::dist(p0, p1);
    let k = 0.4 * chord;
    let c1 = geom::add(p0, geom::scale(d0, k));
    let c2 = geom::sub(p1, geom::scale(d1, k));
    let n = ((chord / step).ceil() as usize).max(4);
    (1..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let w = 1.0 - t;
            let f = |a: f32, b: f32, c: f32, d: f32| {
                w * w * w * a + 3.0 * w * w * t * b + 3.0 * w * t * t * c + t * t * t * d
            };
            [f(p0[0], c1[0], c2[0], p1[0]), f(p0[1], c1[1], c2[1], p1[1])]
        })
        .collect()
}

/// The node position of a link end.
pub(super) fn link_end_pos(lines: &[Ring], plans: &[FreewayPlan], e: LinkEnd) -> V2 {
    match e {
        LinkEnd::Gore { line, side, at } => {
            let ring = &lines[line];
            geom::xy(ring.offset(ring.project(at), side * plans[line].offset()))
        }
        LinkEnd::LineEnd {
            line, side, end, ..
        } => {
            let ring = &lines[line];
            geom::xy(ring.offset(end_s(ring, end), side * plans[line].offset()))
        }
    }
}

/// Points of the end portion of a link at `e` (travel order: away from the node at the
/// start, towards it at the end) and the pose where the free part meets it.
fn end_portion(
    lines: &[Ring],
    plans: &[FreewayPlan],
    e: LinkEnd,
    lanes: u8,
    start: bool,
) -> (Vec<V2>, (V2, V2)) {
    let mut pts = Vec::new();
    match e {
        LinkEnd::Gore { line, side, at } => {
            let ring = &lines[line];
            let f = &plans[line];
            let (o, lo) = (f.offset(), f.lane_off(lanes));
            let s0 = ring.project(at);
            // Distance from the gore along travel: downstream at a diverge, upstream at a merge.
            let away = if start { side } else { -side };
            let n = (LINK_PARALLEL_M / 5.0).ceil() as usize;
            for i in 0..=n {
                let u = LINK_PARALLEL_M * i as f32 / n as f32;
                let x = lo * smoothstep(0.0, crate::builder::FREEWAY_SETBACK_M, u);
                pts.push(geom::xy(ring.offset(s0 + away * u, side * (o + x))));
            }
            let head = geom::scale(ring.at(s0 + away * LINK_PARALLEL_M).1, side);
            if !start {
                pts.reverse();
            }
            let p = if start { pts[pts.len() - 1] } else { pts[0] };
            (pts, (p, head))
        }
        LinkEnd::LineEnd {
            line,
            side,
            end,
            shift,
        } => {
            let ring = &lines[line];
            let s_e = end_s(ring, end);
            let node = geom::xy(ring.offset(s_e, side * plans[line].offset()));
            let head = geom::scale(ring.at(s_e).1, side);
            let r = geom::right_of(head);
            let n = (END_TAPER_M / 5.0).ceil() as usize;
            for i in 0..=n {
                let u = END_TAPER_M * i as f32 / n as f32;
                let x = shift * smoothstep(0.0, END_TAPER_M, u);
                let along = if start { u } else { -u };
                pts.push(geom::add(
                    geom::add(node, geom::scale(head, along)),
                    geom::scale(r, x),
                ));
            }
            if !start {
                pts.reverse();
            }
            let p = if start { pts[pts.len() - 1] } else { pts[0] };
            (pts, (p, head))
        }
    }
}

/// Plan path of a link (travel order, ends on its nodes) and the lengths of its portions
/// beside a carriageway at the start and the end (0 at a split end).
pub(super) fn link_path(lines: &[Ring], plans: &[FreewayPlan], l: &LinkPlan) -> (Vec<V2>, f32, f32) {
    let (mut path, p0) = end_portion(lines, plans, l.from, l.lanes, true);
    let (tail, p1) = end_portion(lines, plans, l.to, l.lanes, false);
    let mut poses = vec![p0];
    poses.extend(l.poses.iter().copied());
    poses.push(p1);
    for w in poses.windows(2) {
        let (a, b) = (w[0], w[1]);
        let mut seg = bezier(a.0, a.1, b.0, b.1, 3.0);
        seg.pop();
        path.extend(seg);
    }
    path.extend(tail);
    path.dedup_by(|x, y| geom::dist(*x, *y) < 0.05);
    let side_len = |e: LinkEnd| match e {
        LinkEnd::Gore { .. } => LINK_PARALLEL_M,
        LinkEnd::LineEnd { .. } => 0.0,
    };
    (path, side_len(l.from), side_len(l.to))
}

/// Merge overlapping internal anchors (keeping the higher).
fn merge_anchors(mut v: Vec<(usize, usize, f32)>) -> Vec<(usize, usize, f32)> {
    v.sort_by_key(|a| a.0);
    let mut out: Vec<(usize, usize, f32)> = Vec::new();
    for a in v {
        if let Some(last) = out.last_mut() {
            if a.0 <= last.1 + 1 {
                last.1 = last.1.max(a.1);
                last.2 = last.2.max(a.2);
                continue;
            }
        }
        out.push(a);
    }
    out
}

/// Profile a link (travel order): on the carriageway's plane beside it at a gore, at the node
/// height at a split end, following the ground in between and bridging every freeway it
/// crosses and every earlier link (`others`, 3D) at deck height. Returns 3D points and bank.
#[allow(clippy::too_many_arguments)]
pub(super) fn profile_link(
    lines: &[Ring],
    plans: &[FreewayPlan],
    l: &LinkPlan,
    path: &[V2],
    beside: (f32, f32),
    z_ends: (f32, f32),
    ground: &dyn Fn(V2) -> f32,
    others: &[Vec<V3>],
) -> (Vec<V3>, Vec<f32>) {
    let st = l.style();
    let path = resample(path, 4.0);
    let cum = cum2(&path);
    let len = cum[cum.len() - 1];
    let n = path.len();
    let dsr = len / (n - 1) as f32;
    let plane_of = |e: LinkEnd| match e {
        LinkEnd::Gore { line, side, .. } => Some((line, side)),
        LinkEnd::LineEnd { .. } => None,
    };
    let (pa, pb) = (plane_of(l.from), plane_of(l.to));
    let k_a = if pa.is_some() {
        cum.partition_point(|c| *c < beside.0).min(n - 1)
    } else {
        0
    };
    let k_b = if pb.is_some() {
        cum.partition_point(|c| *c < len - beside.1).min(n - 1)
    } else {
        n - 1
    };
    let raw: Vec<f32> = path.iter().map(|p| ground(*p)).collect();
    let gs = smooth_series(&raw, (12.0 / dsr) as usize);
    let z_at_a = match pa {
        Some((li, _)) => lines[li].plane_z(path[k_a]),
        None => z_ends.0,
    };
    let z_at_b = match pb {
        Some((li, _)) => lines[li].plane_z(path[k_b]),
        None => z_ends.1,
    };
    let (off_a, off_b) = (z_at_a - gs[k_a], z_at_b - gs[k_b]);
    let desired: Vec<f32> = (0..n)
        .map(|i| {
            if i <= k_a {
                if let Some((li, _)) = pa {
                    return lines[li].plane_z(path[i]);
                }
            }
            if i >= k_b {
                if let Some((li, _)) = pb {
                    return lines[li].plane_z(path[i]);
                }
            }
            let t = (cum[i] - cum[k_a]) / (cum[k_b] - cum[k_a]).max(1e-3);
            gs[i] + off_a + (off_b - off_a) * smoothstep(0.0, 1.0, t)
        })
        .collect();
    // Deck anchors over every freeway this link crosses (a run of samples within its
    // corridor in which the link passes from one side of the line to the other).
    let mut internal: Vec<(usize, usize, f32)> = Vec::new();
    for (li, ring) in lines.iter().enumerate() {
        let reach = plans[li].corridor();
        let total = ring.total();
        let mut run: Option<(usize, f32, bool, bool)> = None; // start, max z, side+, side-
        for i in 0..=n {
            let inside = (i < n && i > k_a && i < k_b).then(|| {
                let s = ring.project(path[i]);
                let (p, d, _, _) = ring.at(s);
                let lat = geom::dot(geom::sub(path[i], p), geom::right_of(d));
                (s > 0.5 && s < total - 0.5 && lat.abs() < reach).then(|| (lat, ring.plane_z(path[i])))
            });
            match inside.flatten() {
                Some((lat, z)) => {
                    let r = run.get_or_insert((i, f32::NEG_INFINITY, false, false));
                    r.1 = r.1.max(z);
                    r.2 |= lat > 0.0;
                    r.3 |= lat < 0.0;
                }
                None => {
                    if let Some((i0, z, a, b)) = run.take() {
                        if a && b {
                            internal.push((
                                i0.saturating_sub(2),
                                (i + 1).min(n - 1),
                                z + LINK_CLEAR_M,
                            ));
                        }
                    }
                }
            }
        }
    }
    for o in others {
        let oxy: Vec<V2> = o.iter().map(|p| geom::xy(*p)).collect();
        let ocum = geom::cumulative(o);
        let ocum2 = cum2(&oxy);
        for (sa, sb, _) in path_crossings(&path, &oxy) {
            let _ = ocum;
            let (_, da) = point_on(&path, &cum, sa);
            let (_, db) = point_on(&oxy, &ocum2, sb);
            let sin = geom::cross(da, db).abs().max(0.2);
            let half = (10.0 / sin).min(40.0);
            let z_o = geom::point_at(o, &geom::cumulative(o), sb).0[2];
            internal.push((
                ((sa - half) / dsr).floor().max(0.0) as usize,
                (((sa + half) / dsr).ceil() as usize).min(n - 1),
                z_o + LINK_CLEAR_M,
            ));
        }
    }
    let internal = merge_anchors(internal);
    let kmax = max_vertical_curvature(st.speed);
    let anchors = [(0, 0, z_ends.0), (n - 1, n - 1, z_ends.1)];
    let mut z = profile(&desired, dsr, &anchors, &internal, (1, 1), st.grade, kmax);
    // Beside the carriageway the link shares its plane exactly.
    if pa.is_some() {
        let same = k_a.saturating_sub(4) + 1;
        z[..same].copy_from_slice(&desired[..same]);
    }
    if pb.is_some() {
        let from = (k_b + 4).min(n - 1);
        z[from..].copy_from_slice(&desired[from..]);
    }
    let res = resample(&path, 6.0);
    let rc = cum2(&res);
    let pts: Vec<V3> = res
        .iter()
        .zip(&rc)
        .map(|(p, s)| {
            let f = s / dsr;
            let i = (f.floor() as usize).min(n - 2);
            let t = f - i as f32;
            [p[0], p[1], z[i] + (z[i + 1] - z[i]) * t]
        })
        .collect();
    // Bank: the carriageway's beside it (in travel terms), the curve's own further on, none
    // at a split end.
    let kappa = curvature(&res);
    let v2 = st.speed * st.speed;
    let spacing = (len / (res.len() - 1).max(1) as f32).max(1.0);
    let raw: Vec<f32> = kappa
        .iter()
        .map(|k| (k * v2 / 9.81 * 0.55).clamp(-st.bank_max, st.bank_max))
        .collect();
    let curve = smooth_series(&raw, (30.0 / spacing) as usize);
    let total = rc[rc.len() - 1];
    let bank: Vec<f32> = res
        .iter()
        .zip(&rc)
        .zip(&curve)
        .map(|((p, s), c)| {
            let plane = |e: Option<(usize, f32)>| {
                e.map_or(0.0, |(li, side)| {
                    let ring = &lines[li];
                    side * ring.at(ring.project(*p)).3
                })
            };
            let wa = if pa.is_some() {
                smoothstep(beside.0 + 40.0, beside.0, *s)
            } else {
                0.0
            };
            let wb = if pb.is_some() {
                smoothstep(total - beside.1 - 40.0, total - beside.1, *s)
            } else {
                0.0
            };
            let fade_a = if pa.is_none() {
                smoothstep(0.0, 60.0, *s)
            } else {
                1.0
            };
            let fade_b = if pb.is_none() {
                smoothstep(0.0, 60.0, total - s)
            } else {
                1.0
            };
            let free = (1.0 - wa - wb).max(0.0);
            (wa * plane(pa) + wb * plane(pb) + free * c * fade_a * fade_b)
                .clamp(-st.bank_max, st.bank_max)
        })
        .collect();
    (pts, bank)
}

/// The freeways split into carriageways, their ramps and links.
pub(super) struct Divided {
    /// Each freeway's profiled centreline.
    pub(super) lines: Vec<Ring>,
    /// `(freeway, side, road)` of every carriageway piece (side +1 = forward).
    pub(super) pieces: Vec<(usize, f32, usize)>,
    /// Diamond ramps and links.
    pub(super) ramps: Vec<usize>,
    /// Links only (subset of `ramps`).
    pub(super) links: Vec<usize>,
}

impl Divided {
    /// Carriageway pieces of the freeway owning draft `draft` (empty if none does).
    pub(super) fn pieces_of(&self, g: &Gen, draft: usize) -> Vec<usize> {
        match g.freeways.iter().position(|f| f.drafts.contains(&draft)) {
            Some(k) => self
                .pieces
                .iter()
                .filter(|p| p.0 == k)
                .map(|p| p.2)
                .collect(),
            None => Vec::new(),
        }
    }
}

/// Split every freeway into carriageways plus ramps and links; appends the builder roads to
/// `out` / `infos`. `lines` are [`freeway_lines`] of the profiled drafts.
pub(super) fn divide_freeways(
    g: &mut Gen,
    lines: Vec<Ring>,
    out: &mut Vec<RoadSpec>,
    infos: &mut Vec<RoadInfo>,
    opt: &BuildOptions,
) -> Divided {
    let plans = g.freeways.clone();
    let links = g.links.clone();
    let flat = flat_lengths(g, opt);
    let mut cuts: Vec<[Vec<(f32, usize)>; 2]> = vec![[Vec::new(), Vec::new()]; plans.len()];
    let side_ix = |side: f32| usize::from(side < 0.0);
    let gore = |g: &mut Gen, line: usize, side: f32, s: f32, cuts: &mut Vec<[Vec<(f32, usize)>; 2]>| {
        let p = lines[line].offset(s, side * plans[line].offset());
        let node = g.node(geom::xy(p));
        g.nodes[node].z = p[2];
        g.nodes[node].control = Some(ControlHint::Freeway);
        cuts[line][side_ix(side)].push((s, node));
        node
    };
    // Diamond gores.
    let mut ramps = diamond_ramps(&plans, &lines, &g.nodes);
    for (k, rp) in &mut ramps {
        rp.gore = gore(g, *k, rp.dir, rp.gore_s, &mut cuts);
    }
    // Line ends.
    for (k, f) in plans.iter().enumerate() {
        let ring = &lines[k];
        for (e, end) in [(f.ends[0], false), (f.ends[1], true)] {
            let s = end_s(ring, end);
            match e {
                FreewayEnd::Fan => {
                    let d = if end {
                        g.drafts[f.drafts[f.drafts.len() - 1]].b
                    } else {
                        g.drafts[f.drafts[0]].a
                    };
                    cuts[k][0].push((s, d));
                    cuts[k][1].push((s, d));
                }
                FreewayEnd::Split => {
                    for side in [1.0, -1.0] {
                        gore(g, k, side, s, &mut cuts);
                    }
                }
            }
        }
    }
    // Link end nodes.
    let mut link_nodes: Vec<(usize, usize)> = Vec::new();
    for l in &links {
        let mut ends = [0usize; 2];
        for (i, e) in [l.from, l.to].into_iter().enumerate() {
            ends[i] = match e {
                LinkEnd::Gore { line, side, at } => {
                    let s = lines[line].project(at);
                    gore(g, line, side, s, &mut cuts)
                }
                LinkEnd::LineEnd { .. } => g.node(link_end_pos(&lines, &plans, e)),
            };
        }
        link_nodes.push((ends[0], ends[1]));
    }
    // Carriageway pieces.
    let mut pieces = Vec::new();
    for (k, f) in plans.iter().enumerate() {
        let ring = &lines[k];
        let total = ring.total();
        let o = f.offset();
        let fan = |s: f32| {
            let a = if f.ends[0] == FreewayEnd::Fan {
                smoothstep(0.0, FAN_M, s)
            } else {
                1.0
            };
            let b = if f.ends[1] == FreewayEnd::Fan {
                smoothstep(0.0, FAN_M, total - s)
            } else {
                1.0
            };
            a * b
        };
        let info = RoadInfo {
            name: f.name,
            speed: f.st.speed,
            grade: f.st.grade,
            deck: DeckRef::Line(k),
        };
        for side in [1.0f32, -1.0] {
            let mut c = cuts[k][side_ix(side)].clone();
            c.sort_by(|a, b| a.0.total_cmp(&b.0));
            c.dedup_by(|a, b| a.1 == b.1);
            for w in c.windows(2) {
                let ((s_a, n_a), (s_b, n_b)) = (w[0], w[1]);
                let samples = if side > 0.0 {
                    arc_samples(s_a, s_b, 10.0)
                } else {
                    arc_samples(s_b, s_a, 10.0)
                };
                let pts: Vec<V3> = samples
                    .iter()
                    .map(|s| ring.offset(*s, side * o * fan(*s)))
                    .collect();
                let bank: Vec<f32> = samples.iter().map(|s| side * ring.at(*s).3).collect();
                let (a, b) = if side > 0.0 { (n_a, n_b) } else { (n_b, n_a) };
                let mut r = spec_from(a, b, &pts, bank, &f.st);
                r.shoulder_left_m = MEDIAN_SHOULDER_M;
                r.shoulder_right_m = OUTER_SHOULDER_M;
                r.median_barrier = side > 0.0;
                pieces.push((k, side, out.len()));
                out.push(r);
                infos.push(info);
            }
        }
    }
    // Diamond ramps.
    let mut ramp_roads = Vec::new();
    for (k, rp) in &ramps {
        let (mut pts, mut bank) = profile_ramp(
            &lines[*k],
            rp,
            g.nodes[rp.gore].z,
            g.nodes[rp.terminal].z,
            flat[rp.terminal],
        );
        let (a, b) = if rp.off {
            (rp.gore, rp.terminal)
        } else {
            pts.reverse();
            bank.reverse();
            for b in &mut bank {
                *b = -*b;
            }
            (rp.terminal, rp.gore)
        };
        let mut r = spec_from(a, b, &pts, bank, &RAMP);
        r.shoulder_left_m = 0.6;
        r.shoulder_right_m = 1.5;
        ramp_roads.push(out.len());
        out.push(r);
        infos.push(RoadInfo {
            name: plans[*k].ramp_name,
            speed: RAMP.speed,
            grade: RAMP.grade,
            deck: DeckRef::None,
        });
    }
    // Links, each over the freeways and the links before it.
    let mut built: Vec<Vec<V3>> = Vec::new();
    let mut link_roads = Vec::new();
    for (l, &(a, b)) in links.iter().zip(&link_nodes) {
        let (path, ba, bb) = link_path(&lines, &plans, l);
        let nat = &g.nat;
        let (pts, bank) = profile_link(
            &lines,
            &plans,
            l,
            &path,
            (ba, bb),
            (g.nodes[a].z, g.nodes[b].z),
            &|p| nat.z(p),
            &built,
        );
        let st = l.style();
        let mut r = spec_from(a, b, &pts, bank, &st);
        r.shoulder_left_m = 0.6;
        r.shoulder_right_m = 1.5;
        built.push(pts);
        ramp_roads.push(out.len());
        link_roads.push(out.len());
        out.push(r);
        infos.push(RoadInfo {
            name: l.name,
            speed: st.speed,
            grade: st.grade,
            deck: DeckRef::None,
        });
    }
    Divided {
        lines,
        pieces,
        ramps: ramp_roads,
        links: link_roads,
    }
}
