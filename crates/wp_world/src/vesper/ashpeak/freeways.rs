//! Ashpeak County's freeways: **Route 7**, **Route 12** and the **Crosstown Expressway**,
//! divided highways (two one-way carriageways, median barrier, shoulders, banked sweepers)
//! split by `divide_freeways` after profiling.
//!
//! - **Route 7** (3 lanes each way, 65 mph) leaves Port Vesper at the Uptown Connector's outer
//!   terminal (the carriageways fan in to that junction), climbs to the **Talon Tunnel**
//!   under Talon Ridge, comes out high over the Silver River on the **Silver Gorge Bridge**
//!   and runs up the east side of Ashpeak City past the Midtown and Northgate interchanges.
//! - **Route 12** (2 lanes each way, 60 mph, grade cap 7 %) leaves Lantern Way's outer
//!   terminal, crosses the Sable River valley past the Harlan Road interchange and climbs the
//!   **Grizzly Grade** in long sweepers to the **Grizzly Tunnel** under the Grizzly
//!   saddle, then sweeps down to the basin and up the west side of Ashpeak City past Ashpeak
//!   West.
//! - The **Crosstown Expressway** (3 lanes each way, 55 mph) runs south of downtown past the
//!   Granite Street and Crowfoot interchanges, ending in a trumpet interchange with Route 12
//!   (west) and with Route 7 (east): its carriageways split into four links each, two of them
//!   on a bridge over the through freeway and one of those a loop.
//!
//! The tunnel heights are found from the ground (the highest level the road reaches at its
//! grade cap with enough rock over the portals), so the portals sit where the mountain
//! really is: the Talon Tunnel runs from Talon Ridge's southern foot to the gorge's rim.

use super::super::*;
use super::plan;

/// Route 7: two-way style used while profiling the centreline (65 mph).
const FREEWAY: Style = Style {
    class: RoadClass::Highway,
    fwd: 3,
    bwd: 3,
    speed: 29.0576,
    parking: false,
    grade: 0.06,
    bank_max: 0.06,
    material: RoadMaterial::Asphalt,
};
/// Route 12: two lanes each way, mountain grade (60 mph).
const PASS: Style = Style {
    fwd: 2,
    bwd: 2,
    speed: 26.8224,
    grade: 0.07,
    ..FREEWAY
};
/// Crosstown Expressway (55 mph).
const EXPRESSWAY: Style = Style {
    speed: 24.586_72,
    ..FREEWAY
};
/// Loop links (30 mph advisory).
const LOOP_SPEED: f32 = 13.4112;
/// Other links (45 mph).
const LINK_SPEED: f32 = 20.1168;
/// Rock over a tunnel portal (ground above the road), m.
const TUNNEL_COVER_M: f32 = 14.0;
/// Steepest climb through a gorge tunnel toward its bridge.
const TUNNEL_CLIMB: f32 = 0.02;
/// Diamond gore distance from the crossroad, m.
const SPAN_M: f32 = 270.0;
/// Distance of a trumpet's stem end (the Crosstown's end) from the through freeway, m.
const STEM_M: f32 = 250.0;

/// Route 7's name.
pub(crate) const ROUTE_7: &str = "Route 7";
/// Route 12's name.
pub(crate) const ROUTE_12: &str = "Route 12";
/// The Crosstown Expressway's name.
pub(crate) const CROSSTOWN: &str = "Crosstown Expressway";
/// The Talon Tunnel (Route 7).
pub(crate) const TALON_TUNNEL: &str = "Talon Tunnel";
/// The Grizzly Tunnel (Route 12).
pub(crate) const GRIZZLY_TUNNEL: &str = "Grizzly Tunnel";
/// Where the trumpets join the Crosstown to Route 12 (west) and Route 7 (east).
const WEST_JUNCTION: V2 = [-2300.0, 3950.0];
const EAST_JUNCTION: V2 = [700.0, 3950.0];

/// Smooth flowing centreline through control points (passes through every one).
fn smooth(ctrl: &[V2]) -> Vec<V2> {
    resample(&catmull_rom(ctrl, 4.0), 6.0)
}

/// Route 7's control points after its start (the plan corridor plus the interchange
/// crossings).
fn route_7_ctrl(start: V2) -> Vec<V2> {
    vec![
        start,
        [620.0, 1950.0],
        [850.0, 2350.0],
        [860.0, 2850.0],
        [800.0, 3300.0],
        EAST_JUNCTION,
        [600.0, 4450.0],
        [550.0, 4700.0],
        [525.0, 5100.0],
        plan::FREEWAY_ENDS[0].1,
    ]
}

/// Route 12's control points: the plan corridor with sweepers up and down the Grizzly Grade
/// (within ~130 m of the plan line).
fn route_12_ctrl(start: V2) -> Vec<V2> {
    vec![
        start,
        [-2150.0, 650.0],
        [-2485.0, 1040.0],
        [-2750.0, 1350.0],
        [-2720.0, 1700.0],
        [-2950.0, 2050.0],
        [-2850.0, 2350.0],
        [-3000.0, 2700.0],
        [-3010.0, 3020.0],
        [-2750.0, 3400.0],
        WEST_JUNCTION,
        [-2155.0, 4500.0],
        [-2100.0, 4700.0],
        plan::FREEWAY_ENDS[1].1,
    ]
}

/// A tunnel (and a bridge straight after it) planned on a freeway draft.
#[derive(Debug, Clone, Copy)]
struct TunnelPlan {
    /// Portals (arc lengths on the draft path).
    s0: f32,
    s1: f32,
    /// Road height at the first portal, m.
    z0: f32,
    /// Road height at the second portal (and over the bridge), m; the road climbs evenly
    /// from `z0` through the tunnel.
    z: f32,
    /// Bridge deck end beyond the second portal (gorge), arc length.
    deck_end: Option<f32>,
}

/// Find the highest road level through the mountain between arc lengths `window` that the
/// road reaches at `grade` from the ground on either side, with [`TUNNEL_COVER_M`] of rock
/// over both portals. With `gorge`, the tunnel ends at the gorge's rim and the road bridges
/// it at the same level (the gorge floor does not count as ground to reach).
fn plan_tunnel(
    g: &Gen,
    path: &[V2],
    window: (f32, f32),
    grade: f32,
    gorge: Option<V2>,
) -> Option<TunnelPlan> {
    let cum = cum2(path);
    let total = cum[cum.len() - 1];
    let n = (total / 10.0).ceil() as usize + 1;
    let ds = total / (n - 1) as f32;
    let raw: Vec<f32> = (0..n)
        .map(|i| g.nat.z(point_on(path, &cum, i as f32 * ds).0))
        .collect();
    let gr = smooth_series(&raw, 2);
    let s_of = |i: usize| i as f32 * ds;
    let (w0, w1) = (
        (window.0 / ds) as usize,
        ((window.1 / ds) as usize).min(n - 1),
    );
    let i_g = gorge.map(|p| {
        let sg = project2(path, &cum, p).0;
        let (a, b) = (
            ((sg - 200.0) / ds).max(0.0) as usize,
            (((sg + 200.0) / ds) as usize).min(n - 1),
        );
        (a..=b)
            .min_by(|x, y| gr[*x].total_cmp(&gr[*y]))
            .unwrap_or(a)
    });
    let top = gr[w0..=w1]
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    let mut z = top - TUNNEL_COVER_M;
    while z > 0.0 {
        let last = i_g.map_or(w1, |ig| ig.min(w1));
        let p0 = (w0..=last).find(|i| gr[*i] >= z + TUNNEL_COVER_M);
        let p1 = (w0..=last).rev().find(|i| gr[*i] >= z + TUNNEL_COVER_M);
        if let (Some(p0), Some(p1)) = (p0, p1) {
            let end = match i_g {
                Some(ig) => (ig..n).find(|i| gr[*i] >= z - 3.0),
                None => Some(p1),
            };
            if let Some(end) = end {
                let fwd = (0..=p0)
                    .map(|j| gr[j] + grade * (s_of(p0) - s_of(j)))
                    .fold(f32::INFINITY, f32::min);
                let bwd = (end..n)
                    .map(|j| gr[j] + grade * (s_of(j) - s_of(end)))
                    .fold(f32::INFINITY, f32::min);
                if z <= fwd && z <= bwd {
                    let flat = TunnelPlan {
                        s0: s_of(p0),
                        s1: s_of(p1),
                        z0: z,
                        z,
                        deck_end: i_g.map(|_| s_of(end)),
                    };
                    return Some(match i_g {
                        Some(ig) => climb_to_gorge(&gr, ds, grade, flat, ig),
                        None => flat,
                    });
                }
            }
        }
        z -= 1.0;
    }
    None
}

/// Raise the bridge end of a gorge tunnel `flat`: the road climbs gently through the
/// tunnel (at most [`TUNNEL_CLIMB`]) so the deck stands higher over the gorge, as long as
/// the far side still reaches the ground at `grade` and the rock keeps its cover. `gr` is
/// the smoothed ground on samples `ds` apart, `ig` the gorge bed's sample.
fn climb_to_gorge(gr: &[f32], ds: f32, grade: f32, flat: TunnelPlan, ig: usize) -> TunnelPlan {
    let n = gr.len();
    let s_of = |i: usize| i as f32 * ds;
    let (p0, p1) = (
        (flat.s0 / ds).round() as usize,
        (flat.s1 / ds).round() as usize,
    );
    let mut z1 = flat.z0 + TUNNEL_CLIMB * (flat.s1 - flat.s0);
    while z1 > flat.z {
        let at = |i: usize| flat.z0 + (z1 - flat.z0) * (s_of(i) - flat.s0) / (flat.s1 - flat.s0);
        let covered = (p0..=p1).all(|i| gr[i] >= at(i) + TUNNEL_COVER_M);
        if let Some(end) = (ig..n).find(|i| gr[*i] >= z1 - 3.0) {
            let bwd = (end..n)
                .map(|j| gr[j] + grade * (s_of(j) - s_of(end)))
                .fold(f32::INFINITY, f32::min);
            if covered && z1 <= bwd {
                return TunnelPlan {
                    z: z1,
                    deck_end: Some(s_of(end)),
                    ..flat
                };
            }
        }
        z1 -= 0.5;
    }
    flat
}

/// Put a tunnel plan on draft `d`: height anchors climbing portal to portal, level over the
/// bridge, the covered stretch and the bridge deck.
fn apply_tunnel(g: &mut Gen, d: usize, t: &TunnelPlan, name: &'static str) {
    let path = g.drafts[d].path.clone();
    let cum = cum2(&path);
    let end = t.deck_end.unwrap_or(t.s1);
    // The climb through the tunnel as short anchors, then the level end (and deck).
    let steps = ((t.s1 - t.s0) / 20.0).ceil().max(1.0) as usize;
    for k in 0..steps {
        let s = t.s0 + (t.s1 - t.s0) * k as f32 / steps as f32;
        let z = t.z0 + (t.z - t.z0) * k as f32 / steps as f32;
        g.drafts[d].anchors.push((s, s, z));
    }
    g.drafts[d].anchors.push((t.s1, end, t.z));
    if let Some(e) = t.deck_end {
        g.drafts[d].decks.push((t.s1 - 10.0, e + 10.0));
    }
    g.drafts[d].covers.push(CoverPlan {
        p0: point_on(&path, &cum, t.s0).0,
        p1: point_on(&path, &cum, t.s1).0,
        kind: CoverKind::Tunnel,
        name,
    });
}

/// Arc length of the point of `path` nearest `p`.
fn arc_at(path: &[V2], p: V2) -> f32 {
    project2(path, &cum2(path), p).0
}

/// A plan frame at a trumpet: `a` along the through freeway in the far carriageway's travel
/// direction (curving with it), `c` along the stem axis `v` (from the stem's side across).
struct Frame<'a> {
    ring: &'a Ring,
    s0: f32,
    /// +1 when the far carriageway travels forward.
    fs: f32,
    v: V2,
}

impl Frame<'_> {
    fn p(&self, a: f32, c: f32) -> V2 {
        geom::add(
            self.ring.at(self.s0 + self.fs * a).0,
            geom::scale(self.v, c),
        )
    }
    fn w(&self, a: f32) -> V2 {
        geom::scale(self.ring.at(self.s0 + self.fs * a).1, self.fs)
    }
}

/// The four links of a trumpet joining stem freeway `stem` (ending `stem_end` at the
/// through freeway `through`) to the through freeway's two carriageways. `far` is the side
/// (+1 forward) of the through carriageway away from the stem. With `loop_in` the stem's
/// traffic onto the far carriageway takes the loop (it lies upstream of the stem along the
/// far carriageway), otherwise the far carriageway's traffic onto the stem does (downstream).
#[allow(clippy::too_many_arguments)]
fn trumpet(
    f: &Frame,
    plans: &[FreewayPlan],
    through: usize,
    far: f32,
    stem: usize,
    stem_end: bool,
    loop_in: bool,
    names: (&'static str, &'static str),
) -> Vec<LinkPlan> {
    // The stem carriageway arriving at the junction ends at its end node; the leaving one
    // starts there.
    let travel_fwd_at_end = |side: f32| (side > 0.0) == stem_end;
    let side_in = if travel_fwd_at_end(1.0) { 1.0 } else { -1.0 };
    let side_out = -side_in;
    let near = -far;
    let fly_lanes = 2;
    let c_b = plans[through].offset() + plans[through].lane_off(fly_lanes);
    let (r, l1) = (65.0f32, 50.0f32);
    let v = f.v;
    let nv = geom::scale(v, -1.0);
    let end = |side: f32, shift: f32| LinkEnd::LineEnd {
        line: stem,
        side,
        end: stem_end,
        shift,
    };
    let gore = |side: f32, a: f32| LinkEnd::Gore {
        line: through,
        side,
        at: f.p(a, 0.0),
    };
    let w = |a: f32| f.w(a);
    let nw = |a: f32| geom::scale(f.w(a), -1.0);
    let mut out = Vec::new();
    // Right turns on the stem's side.
    out.push(LinkPlan {
        from: end(side_in, 10.0),
        to: gore(near, if loop_in { -230.0 } else { -200.0 }),
        poses: Vec::new(),
        lanes: 1,
        speed: LINK_SPEED,
        name: names.0,
    });
    out.push(LinkPlan {
        from: gore(near, 190.0),
        to: end(side_out, 10.0),
        poses: Vec::new(),
        lanes: 1,
        speed: LINK_SPEED,
        name: names.1,
    });
    let a0 = -6.25;
    if loop_in {
        // Stem → far: over the through freeway, then a loop down onto the far carriageway.
        let c1 = c_b + r;
        out.push(LinkPlan {
            from: end(side_in, 1.0),
            to: gore(far, -25.0),
            poses: vec![
                (f.p(a0, c1), v),
                (f.p(a0 - r, c1 + r), nw(a0 - r)),
                (f.p(a0 - r - l1, c1 + r), nw(a0 - r - l1)),
                (f.p(a0 - 2.0 * r - l1, c1), nv),
                (f.p(a0 - r - l1, c_b), w(a0 - r - l1)),
            ],
            lanes: fly_lanes,
            speed: LOOP_SPEED,
            name: names.0,
        });
        // Far → stem: around the loop, then over the through freeway.
        let c_d = c_b + 2.0 * r + 40.0;
        out.push(LinkPlan {
            from: gore(far, -480.0),
            to: end(side_out, 1.0),
            poses: vec![
                (
                    f.p(-330.0, 0.5 * c_d),
                    geom::normalize(geom::add(w(-330.0), v)),
                ),
                (f.p(-230.0, c_d), w(-230.0)),
                (f.p(-110.0, c_d), w(-110.0)),
                (f.p(-a0, c_d - 110.0), nv),
            ],
            lanes: fly_lanes,
            speed: LINK_SPEED,
            name: names.1,
        });
    } else {
        // Stem → far: over the through freeway, around the loop, down onto the far
        // carriageway downstream.
        let top = c_b + 2.0 * r;
        out.push(LinkPlan {
            from: end(side_in, 1.0),
            to: gore(far, 540.0),
            poses: vec![
                (f.p(a0, top + 35.0), v),
                (f.p(60.0, top + 100.0), w(60.0)),
                (f.p(200.0, top + 100.0), w(200.0)),
            ],
            lanes: fly_lanes,
            speed: LINK_SPEED,
            name: names.0,
        });
        // Far → stem: a loop right after the bridge, then back over it.
        let a1 = 120.0;
        out.push(LinkPlan {
            from: gore(far, 30.0),
            to: end(side_out, 1.0),
            poses: vec![
                (f.p(a1, c_b), w(a1)),
                (f.p(a1 + r, c_b + r), v),
                (f.p(a1, c_b + 2.0 * r), nw(a1)),
                (f.p(-a0 + r, c_b + 2.0 * r), nw(-a0 + r)),
                (f.p(-a0, c_b + r), nv),
            ],
            lanes: fly_lanes,
            speed: LOOP_SPEED,
            name: names.1,
        });
    }
    out
}

/// Lay out the freeways (first of Ashpeak County's districts).
pub(super) fn layout(g: &mut Gen) {
    let t7 = super::outer_terminal(g, plan::UPTOWN_SITE);
    let t12 = super::outer_terminal(g, plan::LANTERN_SITE);
    let e7 = g.node(plan::FREEWAY_ENDS[0].1);
    let e12 = g.node(plan::FREEWAY_ENDS[1].1);
    let xw = g.node(geom::add(WEST_JUNCTION, [STEM_M, 0.0]));
    let xe = g.node(geom::sub(EAST_JUNCTION, [STEM_M, 0.0]));
    let p7 = smooth(&route_7_ctrl(g.pos(t7)));
    let p12 = smooth(&route_12_ctrl(g.pos(t12)));
    let pxt = smooth(&[g.pos(xw), [-1500.0, 3950.0], [-600.0, 3950.0], g.pos(xe)]);
    let d7 = g
        .road(t7, e7, p7, FREEWAY, ROUTE_7)
        .expect("highways are never dropped");
    let d12 = g
        .road(t12, e12, p12, PASS, ROUTE_12)
        .expect("highways are never dropped");
    let dxt = g
        .road(xw, xe, pxt, EXPRESSWAY, CROSSTOWN)
        .expect("highways are never dropped");

    // Talon Tunnel straight onto the Silver Gorge Bridge; the Grizzly Tunnel.
    let path7 = g.drafts[d7].path.clone();
    let w7 = (
        arc_at(&path7, [600.0, 1850.0]),
        arc_at(&path7, [800.0, 3350.0]),
    );
    if let Some(t) = plan_tunnel(g, &path7, w7, 0.06, Some(plan::SILVER_GORGE)) {
        apply_tunnel(g, d7, &t, TALON_TUNNEL);
    }
    let path12 = g.drafts[d12].path.clone();
    let w12 = (
        arc_at(&path12, [-2720.0, 1700.0]),
        arc_at(&path12, [-2750.0, 3400.0]),
    );
    if let Some(t) = plan_tunnel(g, &path12, w12, 0.06, None) {
        apply_tunnel(g, d12, &t, GRIZZLY_TUNNEL);
    }

    // Freeways (Route 7, Route 12, Crosstown: indices 0, 1, 2).
    let one_way = |st: Style| Style { bwd: 0, ..st };
    let mut plans = vec![
        FreewayPlan {
            name: ROUTE_7,
            ramp_name: "Route 7 ramp",
            drafts: vec![d7],
            st: one_way(FREEWAY),
            ends: [FreewayEnd::Fan, FreewayEnd::Fan],
            diamonds: Vec::new(),
        },
        FreewayPlan {
            name: ROUTE_12,
            ramp_name: "Route 12 ramp",
            drafts: vec![d12],
            st: one_way(PASS),
            ends: [FreewayEnd::Fan, FreewayEnd::Fan],
            diamonds: Vec::new(),
        },
        FreewayPlan {
            name: CROSSTOWN,
            ramp_name: "Crosstown Expressway ramp",
            drafts: vec![dxt],
            st: one_way(EXPRESSWAY),
            ends: [FreewayEnd::Split, FreewayEnd::Split],
            diamonds: Vec::new(),
        },
    ];
    // Diamond interchanges: the crossroad bridges the freeway between its exact terminals.
    for ip in plan::INTERCHANGES {
        let k = plans
            .iter()
            .position(|f| f.name == ip.freeway)
            .expect("interchange on a planned freeway");
        let (a, b) = (g.node(ip.a), g.node(ip.b));
        let path = bent(ip.a, ip.b, 0.0, 8.0);
        let line = g.drafts[plans[k].drafts[0]].path.clone();
        let Some(&(_, _, centre)) = path_crossings(&path, &line).first() else {
            continue;
        };
        if let Some(r) = g.road(a, b, path, CROSSROAD, ip.name) {
            g.drafts[r].over = true;
        }
        g.ramp_ends.extend([a, a, b, b]);
        let lc = cum2(&line);
        let (_, t) = point_on(&line, &lc, project2(&line, &lc, centre).0);
        let a_right = geom::dot(geom::sub(ip.a, centre), geom::right_of(t)) > 0.0;
        let (right, left) = if a_right { (a, b) } else { (b, a) };
        // Midtown's southern gores keep clear of the Crosstown trumpet's links.
        let span = if ip.name == "Midtown Interchange" {
            [200.0, SPAN_M]
        } else {
            [SPAN_M, SPAN_M]
        };
        plans[k].diamonds.push(Diamond {
            centre,
            right,
            left,
            span,
        });
    }
    g.freeways = plans;

    // Trumpets at both ends of the Crosstown.
    let lines = plan_lines(g);
    let east = Frame {
        ring: &lines[0],
        s0: lines[0].project(EAST_JUNCTION),
        fs: 1.0,
        v: [1.0, 0.0],
    };
    let west = Frame {
        ring: &lines[1],
        s0: lines[1].project(WEST_JUNCTION),
        fs: -1.0,
        v: [-1.0, 0.0],
    };
    let mut links = trumpet(
        &east,
        &g.freeways,
        0,
        1.0,
        2,
        true,
        true,
        ("Crosstown / Route 7 ramp", "Route 7 / Crosstown ramp"),
    );
    links.extend(trumpet(
        &west,
        &g.freeways,
        1,
        -1.0,
        2,
        false,
        false,
        ("Crosstown / Route 12 ramp", "Route 12 / Crosstown ramp"),
    ));
    // Every ramp's plan path is reserved: district roads may not cross one at grade.
    let mut reserved: Vec<Vec<V2>> = links
        .iter()
        .map(|l| link_path(&lines, &g.freeways, l).0)
        .collect();
    reserved.extend(
        diamond_ramps(&g.freeways, &lines, &g.nodes)
            .into_iter()
            .map(|(_, rp)| rp.path),
    );
    g.links = links;
    g.reserved.extend(reserved);
}

/// Covered roads: none as specs (the tunnels are planned on the drafts in [`layout`], where
/// their portals are found from the ground).
pub(super) fn covers() -> Vec<CoverSpec> {
    Vec::new()
}

/// Points of interest: the freeway race grids (forward kerb lane of the carriageway, facing
/// the travel direction).
pub(super) fn pois(site: &mut Site) -> Vec<Poi> {
    let mut out = Vec::new();
    for (name, freeway, side, near, s_off) in [
        (plan::POI_ROUTE7_SOUTH, ROUTE_7, 1.0, [430.0, 1700.0], 0.0),
        (plan::POI_ROUTE7_NORTH, ROUTE_7, -1.0, [520.0, 5300.0], 0.0),
        (
            plan::POI_GRIZZLY_GRADE,
            ROUTE_12,
            1.0,
            [-2740.0, 1500.0],
            0.0,
        ),
        (
            plan::POI_CROSSTOWN_WEST,
            CROSSTOWN,
            1.0,
            [-2010.0, 3945.0],
            0.0,
        ),
    ] {
        let Some((road, _)) = site.carriageway(freeway, side, near) else {
            continue;
        };
        let lane = site.built.road_lanes[road].fwd.last().copied();
        let Some(lane) = lane else { continue };
        let pl = &site.built.lanes[lane as usize].polyline;
        let cum = geom::cumulative(pl);
        let s = geom::project(pl, &cum, near).s + s_off;
        if let Some(p) = site.lane_poi(PoiKind::RaceStartGrid, name, road, s.max(1.0), true) {
            out.push(p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn city() -> &'static (RoadNetwork, VesperReport) {
        static CITY: std::sync::OnceLock<(RoadNetwork, VesperReport)> = std::sync::OnceLock::new();
        CITY.get_or_init(|| super::super::generate_with_report(&super::super::default_config()))
    }

    /// Builder roads named `name`.
    fn named(rep: &VesperReport, name: &str) -> Vec<usize> {
        (0..rep.road_names.len())
            .filter(|r| rep.road_names[*r] == name)
            .collect()
    }

    fn lanes_of(net: &RoadNetwork, road: usize) -> Vec<usize> {
        (0..net.lanes.len())
            .filter(|i| net.lanes[*i].road as usize == road)
            .collect()
    }

    #[test]
    fn freeways_are_divided_with_a_median_barrier() {
        let (net, rep) = city();
        assert_eq!(rep.skipped_roads, 0);
        for (name, lanes) in [(ROUTE_7, 3), (ROUTE_12, 2), (CROSSTOWN, 3)] {
            let roads = named(rep, name);
            assert!(roads.len() >= 4, "{name}: {} pieces", roads.len());
            let mut fwd_len = 0.0;
            for &r in &roads {
                assert!(rep.freeway_roads.contains(&r));
                let ls = lanes_of(net, r);
                assert_eq!(ls.len(), lanes, "{name} road {r}");
                assert!(ls.iter().all(|l| net.lanes[*l].kind == LaneKind::Highway));
                let s = &net.surfaces[r];
                if s.median_barrier {
                    fwd_len += *geom::cumulative(&s.centre).last().unwrap();
                }
            }
            // One carriageway (the forward one) carries the barrier end to end.
            assert!(fwd_len > 2500.0, "{name}: barrier only {fwd_len} m");
        }
    }

    #[test]
    fn diamonds_bridge_the_freeway_between_their_terminals() {
        let (net, rep) = city();
        for ip in plan::INTERCHANGES {
            let near = |p: V3, q: V2| geom::dist(geom::xy(p), q) < 0.05;
            let cross = named(rep, ip.name)
                .into_iter()
                .find(|r| {
                    net.lanes.iter().any(|l| {
                        l.road as usize == *r
                            && near(net.nodes[l.from_node as usize].pos, ip.a)
                            && near(net.nodes[l.to_node as usize].pos, ip.b)
                    })
                })
                .unwrap_or_else(|| panic!("no crossroad {} from a to b", ip.name));
            assert!(
                rep.crossings.iter().filter(|c| c.upper == cross).count() >= 2,
                "{} does not bridge both carriageways",
                ip.name
            );
            // Four ramps: an off- and an on-ramp at each terminal.
            for t in [ip.a, ip.b] {
                let (mut ins, mut outs) = (0, 0);
                for &r in &rep.freeway_ramps {
                    for l in lanes_of(net, r) {
                        let l = &net.lanes[l];
                        ins += usize::from(near(net.nodes[l.to_node as usize].pos, t));
                        outs += usize::from(near(net.nodes[l.from_node as usize].pos, t));
                    }
                }
                assert_eq!((ins, outs), (1, 1), "{} terminal {t:?}", ip.name);
            }
        }
    }

    #[test]
    fn tunnels_and_the_silver_gorge_bridge() {
        let (net, rep) = city();
        for (name, freeway) in [(TALON_TUNNEL, ROUTE_7), (GRIZZLY_TUNNEL, ROUTE_12)] {
            let c = net
                .covered
                .iter()
                .find(|c| c.name == name)
                .unwrap_or_else(|| panic!("no {name}"));
            assert_eq!(c.kind, CoverKind::Tunnel);
            assert_eq!(rep.road_names[c.road as usize], freeway);
            let len = *geom::cumulative(&c.centre).last().unwrap();
            assert!(len > 300.0, "{name} only {len} m");
            // It spans both carriageways.
            assert!(
                c.half_width_m > 12.0,
                "{name} half width {}",
                c.half_width_m
            );
        }
        // Talon Tunnel runs under Talon Ridge.
        let talon = net.covered.iter().find(|c| c.name == TALON_TUNNEL).unwrap();
        let pts: Vec<V2> = talon.centre.iter().map(|p| geom::xy(*p)).collect();
        assert!(project2(&pts, &cum2(&pts), plan::TALON_RIDGE).1 < 60.0);
        // The bridge deck stands high over the gorge bed, which is not carved up to it.
        let t = net.terrain.as_ref().unwrap();
        let mut best = 0.0f32;
        for r in named(rep, ROUTE_7) {
            for p in &net.surfaces[r].centre {
                if geom::dist(geom::xy(*p), plan::SILVER_GORGE) < 200.0 {
                    best = best.max(p[2] - t.height_at(geom::xy(*p)));
                }
            }
        }
        assert!(
            best >= 20.0,
            "Silver Gorge Bridge only {best} m over the bed"
        );
    }

    #[test]
    fn freeway_grades_banks_and_crests_within_design() {
        let (net, rep) = city();
        let mut checked = 0;
        for &r in rep.freeway_roads.iter().chain(&rep.freeway_ramps) {
            let s = &net.surfaces[r];
            let cum = geom::cumulative(&s.centre);
            let v = rep.road_speeds[r];
            let cap = rep.road_grades[r];
            assert!(cap <= 0.08 + 1e-6);
            for k in 0..s.centre.len() - 1 {
                let ds = (cum[k + 1] - cum[k]).max(0.5);
                let gr = (s.centre[k + 1][2] - s.centre[k][2]) / ds;
                assert!(
                    gr.abs() <= cap + 0.005,
                    "{} grade {gr:.3} > {cap} at {:?}",
                    rep.road_names[r],
                    s.centre[k]
                );
                assert!(s.bank[k].abs() <= 0.0601);
                if k > 0 {
                    let ds0 = (cum[k] - cum[k - 1]).max(0.5);
                    let g0 = (s.centre[k][2] - s.centre[k - 1][2]) / ds0;
                    let kv = (gr - g0) / (0.5 * (ds + ds0));
                    assert!(
                        -kv * v * v < 0.35 * 9.81,
                        "{} crest {:.2} g at {:?}",
                        rep.road_names[r],
                        -kv * v * v / 9.81,
                        s.centre[k]
                    );
                }
                checked += 1;
            }
        }
        assert!(checked > 1000);
        // Route 7 and the Crosstown keep to 6 %, Route 12 to 7 %.
        for (name, cap) in [(ROUTE_7, 0.06), (CROSSTOWN, 0.06), (ROUTE_12, 0.07)] {
            for r in named(rep, name) {
                assert_eq!(rep.road_grades[r], cap, "{name}");
            }
        }
    }

    /// Ramps stay beside the carriageways' paved edges (the procgen wedge bug): wherever a
    /// ramp's paving (centre or either edge) lies over a carriageway's paving in plan, it is
    /// on the carriageway's surface (within 0.3 m) or bridges it with clearance, never lower
    /// than it (cars would wedge underneath) and never a step above it. Only the node where
    /// the ramp meets the carriageway is skipped.
    #[test]
    fn ramps_never_pass_under_a_carriageway() {
        let (net, rep) = city();
        let mut on_plane = 0;
        for &r in &rep.freeway_ramps {
            let s = &net.surfaces[r];
            let ends: Vec<V2> = net
                .lanes
                .iter()
                .filter(|l| l.road as usize == r)
                .flat_map(|l| [l.from_node, l.to_node])
                .map(|n| geom::xy(net.nodes[n as usize].pos))
                .collect();
            for k in 0..s.centre.len() {
                let tan = if k + 1 < s.centre.len() {
                    geom::normalize(geom::sub(geom::xy(s.centre[k + 1]), geom::xy(s.centre[k])))
                } else {
                    geom::normalize(geom::sub(geom::xy(s.centre[k]), geom::xy(s.centre[k - 1])))
                };
                for lat in [-s.left_m, 0.0, s.right_m] {
                    let q = geom::add(geom::xy(s.centre[k]), geom::scale(geom::right_of(tan), lat));
                    if ends.iter().any(|e| geom::dist(*e, q) < 10.0) {
                        continue;
                    }
                    let zr = s.z_at(k, lat);
                    for &c in &rep.freeway_roads {
                        let cs = &net.surfaces[c];
                        let cum = geom::cumulative(&cs.centre);
                        let pr = geom::project(&cs.centre, &cum, q);
                        if pr.s <= 0.0 || pr.s >= cum[cum.len() - 1] {
                            continue;
                        }
                        let edge = if pr.lateral > 0.0 {
                            cs.right_m
                        } else {
                            cs.left_m
                        };
                        if pr.lateral.abs() > edge - 0.05 {
                            continue;
                        }
                        let j = cum.partition_point(|x| *x < pr.s).min(cs.centre.len() - 1);
                        let zc =
                            geom::point_at(&cs.centre, &cum, pr.s).0[2] + cs.bank[j] * pr.lateral;
                        let dz = zr - zc;
                        assert!(
                            dz.abs() < 0.3 || dz > 6.5,
                            "{} at {q:?} is {dz:.2} m from {}'s paving",
                            rep.road_names[r],
                            rep.road_names[c]
                        );
                        on_plane += usize::from(dz.abs() < 0.3);
                    }
                }
            }
        }
        // Ramp edges do lie on the carriageways beside their gores.
        assert!(on_plane > 200, "{on_plane}");
    }

    /// Where any two freeway roads (carriageways, ramps, links) cross in plan, one bridges
    /// the other with clearance.
    #[test]
    fn freeway_crossings_are_grade_separated() {
        let (net, rep) = city();
        let roads: Vec<usize> = rep
            .freeway_roads
            .iter()
            .chain(&rep.freeway_ramps)
            .copied()
            .collect();
        let xy = |r: usize| -> Vec<V2> {
            net.surfaces[r]
                .centre
                .iter()
                .map(|p| geom::xy(*p))
                .collect()
        };
        let z_at = |r: usize, p: V2| {
            let c = &net.surfaces[r].centre;
            let cum = geom::cumulative(c);
            let pr = geom::project(c, &cum, p);
            geom::point_at(c, &cum, pr.s).0[2]
        };
        let mut found = 0;
        for (i, &a) in roads.iter().enumerate() {
            for &b in &roads[i + 1..] {
                for (_, _, p) in path_crossings(&xy(a), &xy(b)) {
                    let gap = (z_at(a, p) - z_at(b, p)).abs();
                    assert!(
                        gap > 6.5,
                        "{} and {} cross {gap:.1} m apart at {p:?}",
                        rep.road_names[a],
                        rep.road_names[b]
                    );
                    found += 1;
                }
            }
        }
        // At least the four links over the through freeways at each trumpet.
        assert!(found >= 8, "{found}");
    }

    #[test]
    fn trumpets_join_the_crosstown_both_ways() {
        let (net, rep) = city();
        assert_eq!(rep.freeway_links.len(), 8);
        let mut pairs: Vec<(&str, &str)> = Vec::new();
        for &r in &rep.freeway_links {
            let ls = lanes_of(net, r);
            let from: Vec<&str> = net
                .lanes
                .iter()
                .filter(|m| m.connections.iter().any(|c| ls.contains(&(c.to as usize))))
                .map(|m| rep.road_names[m.road as usize])
                .filter(|n| *n != rep.road_names[r])
                .collect();
            let to: Vec<&str> = ls
                .iter()
                .flat_map(|l| net.lanes[*l].connections.iter())
                .map(|c| rep.road_names[net.lanes[c.to as usize].road as usize])
                .filter(|n| *n != rep.road_names[r])
                .collect();
            assert!(!from.is_empty() && !to.is_empty(), "link {r}");
            pairs.push((from[0], to[0]));
        }
        for (a, b) in [
            (CROSSTOWN, ROUTE_7),
            (ROUTE_7, CROSSTOWN),
            (CROSSTOWN, ROUTE_12),
            (ROUTE_12, CROSSTOWN),
        ] {
            let n = pairs.iter().filter(|p| **p == (a, b)).count();
            assert_eq!(n, 2, "{a} -> {b}: {pairs:?}");
        }
    }

    #[test]
    fn freeways_are_strongly_connected_to_port_vesper() {
        let (net, rep) = city();
        let map = crate::CityMap::new(net.clone());
        let bad = super::super::tests::unreachable_lanes(&map, [0.0, -50.0]);
        let ours: Vec<u32> = bad
            .into_iter()
            .filter(|l| {
                let r = net.lanes[*l as usize].road as usize;
                rep.freeway_roads.contains(&r) || rep.freeway_ramps.contains(&r)
            })
            .collect();
        assert!(
            ours.is_empty(),
            "{} freeway lanes not strongly connected, e.g. {:?}",
            ours.len(),
            ours.iter()
                .take(6)
                .map(|l| net.lanes[*l as usize].polyline[0])
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn freeway_race_grids_face_along_their_lanes() {
        let (net, rep) = city();
        for (name, freeway) in [
            (plan::POI_ROUTE7_SOUTH, ROUTE_7),
            (plan::POI_ROUTE7_NORTH, ROUTE_7),
            (plan::POI_GRIZZLY_GRADE, ROUTE_12),
            (plan::POI_CROSSTOWN_WEST, CROSSTOWN),
        ] {
            let p = net
                .pois
                .iter()
                .find(|p| p.name == name)
                .unwrap_or_else(|| panic!("no {name}"));
            assert_eq!(p.kind, PoiKind::RaceStartGrid);
            let l = &net.lanes[p.lane.unwrap() as usize];
            assert_eq!(rep.road_names[l.road as usize], freeway, "{name}");
            // Kerb (rightmost) lane.
            assert!(l.right.is_none(), "{name} not on the kerb lane");
            let cum = geom::cumulative(&l.polyline);
            let pr = geom::project(&l.polyline, &cum, geom::xy(p.pos));
            let (_, d) = geom::point_at(&l.polyline, &cum, pr.s);
            let diff = (p.heading_rad - geom::yaw_of(d)).rem_euclid(std::f32::consts::TAU);
            assert!(
                diff.min(std::f32::consts::TAU - diff) < 0.2,
                "{name} heading"
            );
            // Room ahead to race.
            assert!(cum[cum.len() - 1] - pr.s > 150.0, "{name}");
        }
    }
}
