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

/// How a ramp's profile ends.
#[derive(Debug, Clone, Copy)]
pub(super) enum ZEnd {
    /// On freeway `line`'s carriageway (a gore): the ramp lies on the carriageway's plane
    /// until it is clear of the paving.
    Plane { line: usize, z: f32 },
    /// At a node of height `z` with a flat junction box `flat` m long (0 at a split end).
    Node { z: f32, flat: f32 },
}

impl ZEnd {
    fn z(&self) -> f32 {
        match *self {
            ZEnd::Plane { z, .. } | ZEnd::Node { z, .. } => z,
        }
    }
}

/// Lateral distance from freeway `line`'s centreline beyond which a `lanes`-lane ramp's
/// paving is clear of the carriageway's (with a 1 m margin), m.
fn clear_of(plans: &[FreewayPlan], line: usize, lanes: u8) -> f32 {
    let f = &plans[line];
    f.offset()
        + 0.5 * f.st.fwd as f32 * HW_LANE_M
        + OUTER_SHOULDER_M
        + 0.5 * lanes as f32 * HW_LANE_M
        + 0.6
        + 1.0
}

/// Profile a ramp or link along `path` (travel order, ends on its nodes) in style `st`.
/// At a [`ZEnd::Plane`] end it lies exactly on the carriageway's plane (height and cross
/// slope) until its paving is clear of the carriageway's, so it never steps above or below
/// the carriageway's edge (nor runs under it); at a node end it is flat over the junction
/// box. In between it follows the ground (offset as at its ends), within the ramp grade cap,
/// and bridges every freeway it crosses and every earlier ramp in `others` (3D) at deck
/// height. Returns 3D points (about 6 m apart) and bank.
#[allow(clippy::too_many_arguments)]
pub(super) fn profile_path(
    lines: &[Ring],
    plans: &[FreewayPlan],
    st: &Style,
    path: &[V2],
    ends: (ZEnd, ZEnd),
    ground: &dyn Fn(V2) -> f32,
    others: &[Vec<V3>],
) -> (Vec<V3>, Vec<f32>) {
    let path = resample(path, 4.0);
    let cum = cum2(&path);
    let len = cum[cum.len() - 1];
    let n = path.len();
    let dsr = len / (n - 1) as f32;
    let lateral = |ring: &Ring, q: V2| {
        let s = ring.project(q);
        let (p, d, _, _) = ring.at(s);
        (s, geom::dot(geom::sub(q, p), geom::right_of(d)))
    };
    // Plane portions: from a gore end until the ramp is clear of the carriageway.
    let plane_run = |e: ZEnd, from_start: bool| -> Option<(usize, usize)> {
        let ZEnd::Plane { line, .. } = e else {
            return None;
        };
        let clear = clear_of(plans, line, st.fwd);
        let idx = |k: usize| if from_start { k } else { n - 1 - k };
        let mut k = 0;
        while k + 1 < n / 2 && lateral(&lines[line], path[idx(k)]).1.abs() < clear {
            k += 1;
        }
        Some((line, idx(k)))
    };
    let pa = plane_run(ends.0, true);
    let pb = plane_run(ends.1, false);
    let k_a = pa.map_or(0, |p| p.1);
    let k_b = pb.map_or(n - 1, |p| p.1);
    let raw: Vec<f32> = path.iter().map(|p| ground(*p)).collect();
    let gs = smooth_series(&raw, (12.0 / dsr) as usize);
    let plane_z = |li: usize, i: usize| lines[li].plane_z(path[i]);
    let z_a = pa.map_or(ends.0.z(), |(li, _)| plane_z(li, k_a));
    let z_b = pb.map_or(ends.1.z(), |(li, _)| plane_z(li, k_b));
    let (off_a, off_b) = (z_a - gs[k_a], z_b - gs[k_b]);
    let desired: Vec<f32> = (0..n)
        .map(|i| {
            let t = (cum[i] - cum[k_a]) / (cum[k_b] - cum[k_a]).max(1e-3);
            gs[i] + off_a + (off_b - off_a) * smoothstep(0.0, 1.0, t.clamp(0.0, 1.0))
        })
        .collect();
    // Pinned: every plane sample.
    let mut internal: Vec<(usize, usize, f32)> = Vec::new();
    if let Some((li, k)) = pa {
        internal.extend((0..=k).map(|i| (i, i, plane_z(li, i))));
    }
    if let Some((li, k)) = pb {
        internal.extend((k..n).map(|i| (i, i, plane_z(li, i))));
    }
    // Deck anchors over every freeway this ramp crosses (a run of samples within its
    // corridor in which the ramp passes from one side of the line to the other).
    let mut decks: Vec<(usize, usize, f32)> = Vec::new();
    for (li, ring) in lines.iter().enumerate() {
        let reach = plans[li].corridor();
        let total = ring.total();
        let mut run: Option<(usize, f32, bool, bool)> = None;
        for i in 0..=n {
            let inside = (i < n && i > k_a && i < k_b).then(|| {
                let (s, lat) = lateral(ring, path[i]);
                (s > 0.5 && s < total - 0.5 && lat.abs() < reach)
                    .then(|| (lat, ring.plane_z(path[i])))
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
                            decks.push((
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
        let ocum = cum2(&oxy);
        let o3 = geom::cumulative(o);
        for (sa, sb, _) in path_crossings(&path, &oxy) {
            let (_, da) = point_on(&path, &cum, sa);
            let (_, db) = point_on(&oxy, &ocum, sb);
            let sin = geom::cross(da, db).abs().max(0.2);
            let half = (10.0 / sin).min(40.0);
            let z_o = geom::point_at(o, &o3, sb).0[2];
            decks.push((
                ((sa - half) / dsr).floor().max(0.0) as usize,
                (((sa + half) / dsr).ceil() as usize).min(n - 1),
                z_o + LINK_CLEAR_M,
            ));
        }
    }
    internal.extend(merge_anchors(decks));
    let kmax = max_vertical_curvature(st.speed);
    let spread = 2.0 * smoothing_sigma(st.grade, kmax, dsr);
    let node_anchor = |e: ZEnd, at_start: bool| -> ((usize, usize, f32), usize) {
        match e {
            ZEnd::Node { z, flat } if flat > 0.0 => {
                let f = ((flat + spread) / dsr).ceil() as usize;
                let p = (flat / dsr).ceil() as usize;
                if at_start {
                    ((0, f.min(n - 1), z), p)
                } else {
                    (((n - 1).saturating_sub(f), n - 1, z), p)
                }
            }
            _ => {
                let i = if at_start { 0 } else { n - 1 };
                ((i, i, e.z()), 1)
            }
        }
    };
    let (aa, pin_a) = node_anchor(ends.0, true);
    let (ab, pin_b) = node_anchor(ends.1, false);
    let z = profile(
        &desired,
        dsr,
        &[aa, ab],
        &internal,
        (pin_a, pin_b),
        st.grade,
        kmax,
    );
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
    // Bank: the carriageway's on its plane (in travel terms), the curve's own further on,
    // none at a node.
    let kappa = curvature(&res);
    let v2 = st.speed * st.speed;
    let spacing = (len / (res.len() - 1).max(1) as f32).max(1.0);
    let raw: Vec<f32> = kappa
        .iter()
        .map(|k| (k * v2 / 9.81 * 0.55).clamp(-st.bank_max, st.bank_max))
        .collect();
    let curve = smooth_series(&raw, (30.0 / spacing) as usize);
    let total = rc[rc.len() - 1];
    let (s_a, s_b) = (cum[k_a], cum[k_b]);
    let travel_bank = |li: usize, p: V2, d: V2| {
        let ring = &lines[li];
        let s = ring.project(p);
        let (_, t, _, b) = ring.at(s);
        // The plane's cross slope right of travel along `d`.
        b * geom::dot(t, d).signum()
    };
    let bank: Vec<f32> = (0..res.len())
        .map(|i| {
            let (p, s) = (res[i], rc[i]);
            let d = if i + 1 < res.len() {
                geom::normalize(geom::sub(res[i + 1], p))
            } else {
                geom::normalize(geom::sub(p, res[i - 1]))
            };
            let wa = pa.map_or(0.0, |_| smoothstep(s_a + 40.0, s_a, s));
            let wb = pb.map_or(0.0, |_| smoothstep(s_b - 40.0, s_b, s));
            let fade = |e: ZEnd, dist: f32| match e {
                ZEnd::Node { flat, .. } => smoothstep(0.0, 60.0, dist - flat),
                ZEnd::Plane { .. } => 1.0,
            };
            let free = (1.0 - wa - wb).max(0.0);
            let ba = pa.map_or(0.0, |(li, _)| travel_bank(li, p, d));
            let bb = pb.map_or(0.0, |(li, _)| travel_bank(li, p, d));
            (wa * ba + wb * bb + free * curve[i] * fade(ends.0, s) * fade(ends.1, total - s))
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
    /// `(node, road)`: a forward carriageway's first piece leaving a fan start node.
    pub(super) fan_starts: Vec<(usize, usize)>,
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
    // Diamond ramps, then links (each over the freeways and the links before it).
    let ground = |p: V2| g.nat.z(p);
    let ramp_style = RAMP;
    let mut ramp_roads = Vec::new();
    let mut new_roads: Vec<(RoadSpec, RoadInfo)> = Vec::new();
    for (k, rp) in &ramps {
        let plane = ZEnd::Plane {
            line: *k,
            z: g.nodes[rp.gore].z,
        };
        let term = ZEnd::Node {
            z: g.nodes[rp.terminal].z,
            flat: flat[rp.terminal],
        };
        let (a, b, path, ends) = if rp.off {
            (rp.gore, rp.terminal, rp.path.clone(), (plane, term))
        } else {
            let mut p = rp.path.clone();
            p.reverse();
            (rp.terminal, rp.gore, p, (term, plane))
        };
        let mut path = path;
        let last = path.len() - 1;
        path[0] = g.nodes[a].pos;
        path[last] = g.nodes[b].pos;
        let (pts, bank) = profile_path(&lines, &plans, &ramp_style, &path, ends, &ground, &[]);
        let mut r = spec_from(a, b, &pts, bank, &ramp_style);
        r.shoulder_left_m = 0.6;
        r.shoulder_right_m = 1.5;
        new_roads.push((
            r,
            RoadInfo {
                name: plans[*k].ramp_name,
                speed: RAMP.speed,
                grade: RAMP.grade,
                deck: DeckRef::None,
            },
        ));
    }
    let mut built: Vec<Vec<V3>> = Vec::new();
    let mut link_specs = Vec::new();
    for (l, &(a, b)) in links.iter().zip(&link_nodes) {
        let (mut path, _, _) = link_path(&lines, &plans, l);
        let last = path.len() - 1;
        path[0] = g.nodes[a].pos;
        path[last] = g.nodes[b].pos;
        let zend = |e: LinkEnd, n: usize| match e {
            LinkEnd::Gore { line, .. } => ZEnd::Plane {
                line,
                z: g.nodes[n].z,
            },
            LinkEnd::LineEnd { .. } => ZEnd::Node {
                z: g.nodes[n].z,
                flat: 0.0,
            },
        };
        let st = l.style();
        let (pts, bank) = profile_path(
            &lines,
            &plans,
            &st,
            &path,
            (zend(l.from, a), zend(l.to, b)),
            &ground,
            &built,
        );
        let mut r = spec_from(a, b, &pts, bank, &st);
        r.shoulder_left_m = 0.6;
        r.shoulder_right_m = 1.5;
        built.push(pts);
        link_specs.push((
            r,
            RoadInfo {
                name: l.name,
                speed: st.speed,
                grade: st.grade,
                deck: DeckRef::None,
            },
        ));
    }
    let mut link_roads = Vec::new();
    for (i, (r, info)) in new_roads.into_iter().chain(link_specs).enumerate() {
        if i >= ramps.len() {
            link_roads.push(out.len());
        }
        ramp_roads.push(out.len());
        out.push(r);
        infos.push(info);
    }
    // Forward carriageways starting at a fan: their first pieces (fed after building).
    let fan_starts: Vec<(usize, usize)> = pieces
        .iter()
        .filter(|p| p.1 > 0.0 && plans[p.0].ends[0] == FreewayEnd::Fan)
        .filter(|p| out[p.2].a == g.drafts[plans[p.0].drafts[0]].a)
        .map(|p| (out[p.2].a, p.2))
        .collect();
    Divided {
        lines,
        pieces,
        ramps: ramp_roads,
        links: link_roads,
        fan_starts,
    }
}

/// At a fan start (a freeway beginning at an at-grade junction) the approach going straight
/// on may have fewer lanes than the carriageway: every carriageway lane no connector enters
/// gets one from the approach lane feeding its nearest fed neighbour (a lane added on the
/// left, as the right ones already take the turning traffic).
pub(super) fn feed_fan_starts(built: &mut Built, fan_starts: &[(usize, usize)]) {
    for &(_, road) in fan_starts {
        let outs = built.road_lanes[road].fwd.clone();
        let fed_by = |built: &Built, to: LaneId| -> Option<LaneId> {
            (0..built.lanes.len())
                .filter(|i| {
                    built.lanes[*i]
                        .connections
                        .iter()
                        .any(|c| c.to == to && c.turn == TurnKind::Straight)
                })
                .map(|i| i as LaneId)
                .min_by_key(|i| built.lanes[*i as usize].index_from_left)
        };
        for k in 0..outs.len() {
            let to = outs[k];
            let any_in = built
                .lanes
                .iter()
                .any(|l| l.connections.iter().any(|c| c.to == to));
            if any_in {
                continue;
            }
            // The nearest lane to the right that a straight approach feeds.
            let Some(src) = outs[k + 1..].iter().find_map(|o| fed_by(built, *o)) else {
                continue;
            };
            built.lanes[src as usize].connections.push(LaneConnection {
                to,
                turn: TurnKind::Straight,
            });
        }
    }
}
