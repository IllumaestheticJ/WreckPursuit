//! Port Vesper v3 road network: streets of every district, the named arterials, bridges,
//! tunnels and the highway network (Loop 9, Route 40 West / North, the Crosstown
//! Expressway, the Airport Expressway, the Port Connector) with its interchanges, profiled
//! against the [`Ground`] and carved into it.
//!
//! **Owned by the v3 roads workstream.** Contract with the pipeline (`metro::generate`):
//! [`build`] receives the plan and the ground *after* earthworks, returns [`RoadsOut`] and
//! carves `ground.hf` in place (12 cm under every at-grade surface, never above a road,
//! natural under bridge decks, ≥ 0.5 m under decks, cut-and-cover trenches for tunnels).
//!
//! # Pipeline
//! 1. **Highways** ([`highways`]): filleted centrelines from the plan, with designated
//!    viaduct (Crosstown Viaduct, Harbor Skyway, Westline Viaduct) and tunnel (Calloway
//!    Tunnel) spans.
//! 2. **Interchanges** ([`interchanges`]): ramp centrelines for the Westline Cloverleaf,
//!    the Skyway Stack, three trumpets, seven diamonds, the Old Post Road parclo and the
//!    Founders Parkway half diamond; street terminals become forced junctions.
//! 3. **Streets** ([`streets`]): district layouts + arterials as raw lines, with planned
//!    structures (bridges, underpasses, the Westridge Tunnel) as grade-separated ranges, then
//!    clipped out of water (short crossings become small bridges), sites (except one access
//!    road per gate), rail corridors (at-grade crossings for collectors and up, Meridian over
//!    the freight line), highway corridors (local streets do not cross highways outside
//!    viaducts), interchange corridors and the protected scenic roads.
//! 4. **Planar graph** ([`graph`]): crossings become junctions, cleanup, largest component,
//!    one-way fix-up.
//! 5. **Profiles** ([`profile`]): streets (node heights relaxed to the grade caps, junction
//!    approaches flat), then highways (over every street they cross, under where they are in
//!    a tunnel, ≥ 8.2 m surface clearance), then ramps level by level. A crossing whose
//!    clearance cannot be met clips the street there and the street steps run again (up
//!    to four rounds).
//! 6. **Builder** input: street edges, carriageway pieces between gores (median barrier on
//!    the forward carriageway), ramps (freeway merges / diverges, ramp terminals), gate stubs.
//! 7. **Carving** ([`carve`]) and **structures** ([`structures`]): piers, trusses,
//!    drawbridge towers, tunnel walls / roofs / plugs / portals, sign gantries, cliff rails.

mod carve;
pub mod check;
pub mod geo;
pub mod graph;
pub mod highways;
pub mod interchanges;
pub mod lines;
pub mod profile;
pub mod streets;
pub mod structures;
pub mod style;

use std::collections::BTreeMap;

use super::ground::{signed_dist, Ground};
use super::plan::{self, InterchangeKind, Plan, StructureKind};
use super::MetroConfig;
use crate::builder::{self, BuildOptions, Built, ControlHint, NodeSpec, RoadClass, RoadSpec};
use crate::geom::{self, V2, V3};
use crate::network::Feature;
use crate::terrain::smoothstep;
use geo::{cum2, SegGrid};
use graph::Graph;
use highways::{Hw, Span};
use interchanges::{IcOut, RampEnd};
use lines::{Line, Sep, SepKind};
use style::Style;

/// Surface-to-surface clearance targeted at every grade separation, m (≥ 7.5 m required;
/// the extra covers the deck).
pub const CLEARANCE_M: f32 = 8.2;
/// Terrain sits this far below an at-grade road surface, m.
pub const CARVE_BELOW_M: f32 = 0.12;
/// Distance of a trumpet / stack spur's junction end from the through highway, m.
pub const SPUR_Y0_M: f32 = 400.0;

/// What a builder road is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RoadKind {
    /// Street (graph edge).
    Street,
    /// Highway carriageway piece.
    Carriageway,
    /// Interchange ramp.
    Ramp,
    /// Two-way stub from a highway end to a gate.
    Stub,
}

/// Per builder road (same order as `RoadsOut::specs`).
#[derive(Debug, Clone)]
pub struct RoadInfo {
    /// Kind.
    pub kind: RoadKind,
    /// Grade cap.
    pub grade: f32,
    /// Highway index (carriageways and stubs) or interchange index (ramps).
    pub owner: Option<usize>,
    /// Loop ramp.
    pub is_loop: bool,
    /// Covered (tunnel) arc ranges along the spec's centreline, m.
    pub covered: Vec<(f32, f32)>,
}

/// A grade separation between two builder roads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossing {
    /// Upper road (spec index).
    pub upper: usize,
    /// Lower road.
    pub lower: usize,
    /// Plan position, m.
    pub pos: V2,
    /// Surface height difference achieved, m.
    pub clearance: f32,
}

/// Where each plan gate is reached.
#[derive(Debug, Clone)]
pub struct GateReach {
    /// Site id.
    pub site: &'static str,
    /// Gate index in the site.
    pub gate: usize,
    /// Builder node at the gate (None = not reached; see `reason`).
    pub node: Option<usize>,
    /// Why not reached.
    pub reason: &'static str,
}

/// Road generator diagnostics.
#[derive(Debug, Clone, Default)]
pub struct RoadsReport {
    /// What each road is (`specs` order).
    pub info: Vec<RoadInfo>,
    /// Grade separations (every plan-view crossing of two roads without a junction).
    pub crossings: Vec<Crossing>,
    /// Gate reachability.
    pub gates: Vec<GateReach>,
    /// Highway names (index = `RoadInfo::owner` of carriageways).
    pub highway_names: Vec<String>,
    /// Interchanges: (name, kind, ramp spec indices).
    pub interchanges: Vec<(String, InterchangeKind, Vec<usize>)>,
}

/// Output of the road generator.
pub struct RoadsOut {
    /// Node specs (builder input; index = builder node).
    pub nodes: Vec<NodeSpec>,
    /// Road specs (builder input; index = road id of `built`).
    pub specs: Vec<RoadSpec>,
    /// The built lanes, junctions, signals and surfaces.
    pub built: Built,
    /// Name of each road (`specs` order; empty for unnamed streets).
    pub names: Vec<String>,
    /// Road structures that are not paving: bridge trusses / towers / piers, tunnel walls,
    /// roofs and portals, guard rails on retaining walls, sign gantries.
    pub features: Vec<Feature>,
    /// Diagnostics (one line each), printed by `gen-city` and the stats example.
    pub notes: Vec<String>,
    /// Road generator diagnostics (what each road is, grade separations, gates,
    /// interchanges). `Default` (empty) for hand-built test networks.
    pub report: RoadsReport,
}

// ------------------------------------------------------------------------------------------
// Highways from the plan
// ------------------------------------------------------------------------------------------

/// Index of each highway in the generator's list.
pub mod hwi {
    /// Loop 9.
    pub const LOOP: usize = 0;
    /// Route 40 West + Crosstown Expressway (one road through the Westline Cloverleaf).
    pub const CROSSTOWN: usize = 1;
    /// Route 40 North.
    pub const R40N: usize = 2;
    /// Airport Expressway.
    pub const AIRPORT: usize = 3;
    /// Port Connector.
    pub const PORT: usize = 4;
}

/// Interchange positions used (plan intent, moved where the geometry needs it).
const AIRPORT_JCT_Y: f32 = -1350.0;
const PORT_JCT_X: f32 = 1250.0;
const NORTH_JCT_X: f32 = -200.0;

fn build_highways(plan: &Plan) -> Vec<Hw> {
    let mph = |m: f32| m * 0.447_04;
    let lp = plan.highway("Loop 9").expect("Loop 9 in the plan");
    let n = lp.ctrl.len();
    let mut loop9 = Hw::new(
        "Loop 9",
        &lp.ctrl,
        &vec![400.0; n],
        true,
        lp.lanes,
        lp.speed_mps,
    );
    // Route 40 West + the Crosstown Expressway: straight through the cloverleaf, an
    // S-curve up to y = 800, the viaduct along the district line, an S-curve down to
    // y = 500 and the spur end before the Skyway Stack.
    let ct_end_x = 1800.0 - SPUR_Y0_M;
    let ctrl = vec![
        [-4395.0, 335.0],
        [-3400.0, 328.0],
        [-2600.0, 302.0],
        [-2000.0, 300.0],
        [-1250.0, 300.0],
        [-750.0, 800.0],
        [880.0, 800.0],
        [1180.0, 500.0],
        [ct_end_x, 500.0],
    ];
    let radii = [0.0, 3000.0, 3000.0, 0.0, 450.0, 450.0, 500.0, 500.0, 0.0];
    let mut ct = Hw::new("Crosstown Expressway", &ctrl, &radii, false, 3, mph(55.0));
    let s_clover = ct.s_of([-2000.0, 300.0]);
    ct.lanes = vec![(0.0, 2, mph(65.0)), (s_clover, 3, mph(55.0))];
    ct.offset = highways::MEDIAN_SHOULDER_M + 3.0 * highways::HW_LANE_M * 0.5;
    let s0 = ct.s_of([-1700.0, 300.0]);
    let s1 = ct.total();
    ct.viaducts.push(Span {
        s0,
        s1,
        h: 9.0,
        name: "Crosstown Viaduct",
    });
    let r40n = Hw::new(
        "Route 40 North",
        &[
            [NORTH_JCT_X, 1900.0 + SPUR_Y0_M],
            [-250.0, 2700.0],
            [-300.0, 3395.0],
        ],
        &[0.0, 2000.0, 0.0],
        false,
        2,
        mph(65.0),
    );
    let mut air = Hw::new(
        "Airport Expressway",
        &[
            [-2000.0 - SPUR_Y0_M, AIRPORT_JCT_Y],
            [-2650.0, AIRPORT_JCT_Y],
            [-2950.0, -1640.0],
            [-3122.0, -1650.0],
        ],
        &[0.0, 500.0, 400.0, 0.0],
        false,
        2,
        mph(55.0),
    );
    air.end_gate = Some([-3150.0, -1650.0]);
    let port_y0 = 300.0;
    let mut port = Hw::new(
        "Port Connector",
        &[
            [PORT_JCT_X, -2100.0 - port_y0],
            [PORT_JCT_X, -2500.0],
            [1472.0, -2500.0],
        ],
        &[0.0, 110.0, 0.0],
        false,
        2,
        mph(45.0),
    );
    port.end_gate = Some([1500.0, -2500.0]);
    // Loop 9 spans.
    let s_of = |hw: &Hw, p: V2| hw.s_of(p);
    let span = |hw: &Hw, a: V2, b: V2, h: f32, name: &'static str| {
        let (sa, sb) = (s_of(hw, a), s_of(hw, b));
        let (s0, mut s1) = (sa.min(sb), sa.max(sb));
        if hw.closed && s1 - s0 > hw.total() * 0.5 {
            // Wraps: express as s1 .. s0 + total.
            let t = hw.total();
            let (a2, b2) = (s1, s0 + t);
            return Span {
                s0: a2,
                s1: b2,
                h,
                name,
            };
        }
        s1 = s1.max(s0);
        Span { s0, s1, h, name }
    };
    loop9.viaducts.push(span(
        &loop9,
        [-2000.0, -930.0],
        [-2000.0, -20.0],
        12.0,
        "Westline Viaduct",
    ));
    loop9.viaducts.push(span(
        &loop9,
        [1800.0, -40.0],
        [1800.0, 1500.0],
        9.0,
        "Harbor Skyway",
    ));
    loop9.tunnels.push(span(
        &loop9,
        [1800.0, -1330.0],
        [1800.0, -250.0],
        9.0,
        "Calloway Tunnel",
    ));
    let _ = (StructureKind::Tunnel, plan::WATER_Z);
    vec![loop9, ct, r40n, air, port]
}

// ------------------------------------------------------------------------------------------
// Interchanges
// ------------------------------------------------------------------------------------------

fn art_path<'a>(raw: &'a [Line], name: &str, near: V2) -> Option<&'a Line> {
    raw.iter()
        .filter(|l| l.name == name)
        .min_by(|a, b| geo::dist_to(&a.path, near).total_cmp(&geo::dist_to(&b.path, near)))
}

/// Design every interchange; returns the designs and, per plan interchange, its index.
fn design_interchanges(
    plan: &Plan,
    hws: &[Hw],
    raw: &[Line],
) -> (IcOut, Vec<(String, InterchangeKind)>) {
    use hwi::*;
    let mut out = IcOut::default();
    let mut list = Vec::new();
    for (ic, x) in plan.interchanges.iter().enumerate() {
        list.push((x.name.to_string(), x.kind));
        let before = out.ramps.len();
        match x.name {
            "Westline Cloverleaf" => interchanges::cloverleaf(
                &mut out,
                ic,
                hws,
                LOOP,
                CROSSTOWN,
                [-2000.0, 300.0],
                x.name,
            ),
            "Skyway Stack" => interchanges::trumpet(
                &mut out, ic, hws, LOOP, CROSSTOWN, false, x.name, true, 130.0,
            ),
            "North Trumpet" => {
                interchanges::trumpet(&mut out, ic, hws, LOOP, R40N, true, x.name, false, 330.0)
            }
            "Airport Junction" => {
                interchanges::trumpet(&mut out, ic, hws, LOOP, AIRPORT, true, x.name, false, 330.0)
            }
            "Port Trumpet" => {
                interchanges::trumpet(&mut out, ic, hws, LOOP, PORT, true, x.name, false, 230.0)
            }
            "Old Post Road" => {
                if let Some(l) = art_path(raw, "Old Post Road", x.pos) {
                    interchanges::parclo(
                        &mut out,
                        ic,
                        hws,
                        LOOP,
                        x.name,
                        "Old Post Road",
                        &l.path,
                        x.pos,
                    );
                }
            }
            _ => {
                // Diamonds and the half diamond: crossroad by plan name.
                let (h, line_name, near): (usize, &'static str, V2) = match x.name {
                    "Meridian Avenue (south)" => (LOOP, "Meridian Avenue", x.pos),
                    "Harbor Boulevard" => (LOOP, "Founders Parkway", [1800.0, 1300.0]),
                    "Dunmore Avenue" => (LOOP, "Dunmore Avenue", x.pos),
                    "Mercer Street" => (CROSSTOWN, "Mercer Street", x.pos),
                    "Meridian Avenue (Crosstown)" => (CROSSTOWN, "Meridian Avenue", x.pos),
                    "Westline Avenue (airport)" => (AIRPORT, "Westline Avenue", x.pos),
                    "Route 40 West (outskirts)" => (CROSSTOWN, "Skyline Drive", x.pos),
                    "Founders Parkway (north)" => (LOOP, "Founders Parkway", x.pos),
                    _ => {
                        out.notes
                            .push(format!("interchange {} not built (unknown)", x.name));
                        continue;
                    }
                };
                let Some(l) = art_path(raw, line_name, near) else {
                    out.notes
                        .push(format!("interchange {}: no crossroad {line_name}", x.name));
                    continue;
                };
                let path = l.path.clone();
                let half = x.kind == InterchangeKind::HalfDiamond;
                // The Westline Avenue interchange sits right next to the Airport Junction's
                // spur end: only its airport-side ramps are built (a half diamond).
                let which = if half || x.name == "Westline Avenue (airport)" {
                    [false, true, true, false]
                } else {
                    [true; 4]
                };
                let urban = matches!(x.name, "Mercer Street" | "Meridian Avenue (Crosstown)");
                let reach = if urban { 105.0 } else { 140.0 };
                let span =
                    if urban || x.name == "Harbor Boulevard" || x.name == "Meridian Avenue (south)"
                    {
                        230.0
                    } else {
                        250.0
                    };
                let control = if half {
                    Some(ControlHint::RampTerminal)
                } else {
                    None
                };
                interchanges::diamond(
                    &mut out, ic, hws, h, x.name, line_name, &path, near, reach, span, which,
                    control,
                );
            }
        }
        let n = out.ramps.len() - before;
        out.notes
            .push(format!("interchange {} ({:?}): {n} ramps", x.name, x.kind));
    }
    (out, list)
}

// ------------------------------------------------------------------------------------------
// Street lines: structures, clipping, gates
// ------------------------------------------------------------------------------------------

fn approach_len(dz: f32, st: &Style) -> f32 {
    dz.abs() / (0.7 * st.grade) + 45.0
}

/// Attach planned street structures (bridges, underpasses, the Westridge Tunnel) and the
/// Meridian Avenue overpass over the freight line as grade-separated ranges.
fn attach_structures(
    plan: &Plan,
    gnd: &Ground,
    rails: &[Rail],
    raw: &mut [Line],
    notes: &mut Vec<String>,
) {
    for s in &plan.structures {
        let (kind, bound, covered, lanes) = match s.name {
            "Westline Low Bridge" => (SepKind::Bridge, 5.0, false, 0),
            "Iron Bridge" => (SepKind::Bridge, 6.0, false, 1),
            "Meridian Bridge" => (SepKind::Bridge, 13.0, false, 0),
            "Kettle Street Drawbridge" => (SepKind::Bridge, 5.5, false, 0),
            "Riverside Bridge" => (SepKind::Bridge, 6.0, false, 0),
            "Founders Cut" => (SepKind::Under, 8.6, true, 0),
            "Lantern Underpass" => (SepKind::Under, 5.0, false, 0),
            "Westridge Tunnel" => (SepKind::Under, 9.0, true, 0),
            _ => continue,
        };
        let mid = geom::scale(geom::add(s.from, s.to), 0.5);
        let Some(li) = raw
            .iter()
            .enumerate()
            .filter(|(_, l)| l.name == s.carries)
            .min_by(|a, b| geo::dist_to(&a.1.path, mid).total_cmp(&geo::dist_to(&b.1.path, mid)))
            .map(|(i, _)| i)
        else {
            notes.push(format!("structure {}: no road {}", s.name, s.carries));
            continue;
        };
        let l = &raw[li];
        let (sa, _) = geo::project2(&l.path, &l.cum, s.from);
        let (sb, _) = geo::project2(&l.path, &l.cum, s.to);
        let (mut c0, mut c1) = (sa.min(sb), sa.max(sb));
        if kind == SepKind::Bridge {
            // Make sure the core spans all the water.
            let mut s = (c0 - 200.0).max(0.0);
            let (mut w0, mut w1) = (f32::MAX, f32::MIN);
            while s < (c1 + 200.0).min(l.len()) {
                if gnd.z(l.at(s).0) < plan::WATER_Z + 0.8 {
                    w0 = w0.min(s);
                    w1 = w1.max(s);
                }
                s += 4.0;
            }
            if w0 <= w1 {
                c0 = c0.min(w0 - 12.0);
                c1 = c1.max(w1 + 12.0);
            }
        }
        let dz = match kind {
            SepKind::Under => bound,
            _ => {
                let g0 = gnd.z(l.at(c0).0).max(plan::WATER_Z);
                let g1 = gnd.z(l.at(c1).0).max(plan::WATER_Z);
                bound - g0.min(g1)
            }
        };
        let app = approach_len(dz, &l.st);
        let len = l.len();
        let sep = Sep {
            s0: (c0 - app).max(0.0),
            s1: (c1 + app).min(len),
            core0: c0.max(0.0),
            core1: c1.min(len),
            kind,
            bound,
            name: s.name,
            covered,
            lanes,
        };
        raw[li].seps.push(sep);
    }
    // Meridian Avenue rises a little over the Lantern Underpass (Lantern Way only dips
    // 5 m: the water table).
    let lantern: Vec<(Vec<V2>, Sep)> = raw
        .iter()
        .filter(|l| l.name == "Lantern Way")
        .flat_map(|l| l.seps.iter().map(move |r| (l.path.clone(), *r)))
        .filter(|(_, r)| r.name == "Lantern Underpass")
        .collect();
    for l in raw.iter_mut().filter(|l| l.name == "Meridian Avenue") {
        for (lp, _) in &lantern {
            for (s, _, p, _) in lines::crossings_with(l, lp) {
                let g = gnd.z(p);
                let rise = CLEARANCE_M - 5.0 + 0.4;
                let app = approach_len(rise, &l.st);
                let len = l.len();
                l.seps.push(Sep {
                    s0: (s - 15.0 - app).max(0.0),
                    s1: (s + 15.0 + app).min(len),
                    core0: s - 15.0,
                    core1: s + 15.0,
                    kind: SepKind::Over,
                    bound: g + rise,
                    name: "Lantern Underpass bridge",
                    covered: false,
                    lanes: 0,
                });
            }
        }
    }
    // Meridian Avenue over the freight line (grade-separated; CITY.md).
    for l in raw.iter_mut().filter(|l| l.name == "Meridian Avenue") {
        for r in rails.iter().filter(|r| r.name == "Westline Freight Line") {
            for (s, sr, p, _) in lines::crossings_with(l, &r.xy) {
                let g = r.at(sr).0.max(gnd.z(p));
                let app = approach_len(CLEARANCE_M, &l.st);
                let len = l.len();
                l.seps.push(Sep {
                    s0: (s - 15.0 - app).max(0.0),
                    s1: (s + 15.0 + app).min(len),
                    core0: s - 15.0,
                    core1: s + 15.0,
                    kind: SepKind::Over,
                    bound: g + CLEARANCE_M,
                    name: "Meridian rail overpass",
                    covered: false,
                    lanes: 0,
                });
            }
        }
    }
}

/// A rail line as the rail module builds it (filleted, profiled).
#[derive(Debug, Clone)]
pub struct Rail {
    /// Plan name.
    pub name: &'static str,
    /// Centreline.
    pub xy: Vec<V2>,
    /// Arc lengths.
    pub cum: Vec<f32>,
    /// Formation height per point, m.
    pub z: Vec<f32>,
    /// Point carried by a structure (viaduct, bridge, station deck).
    pub elevated: Vec<bool>,
}

impl Rail {
    /// Formation height and elevation flag at arc `s`.
    pub fn at(&self, s: f32) -> (f32, bool) {
        let n = self.xy.len();
        let s = s.clamp(0.0, self.cum[n - 1]);
        let i = match self.cum.binary_search_by(|c| c.total_cmp(&s)) {
            Ok(i) => i.min(n - 2),
            Err(i) => i.saturating_sub(1).min(n - 2),
        };
        let t = ((s - self.cum[i]) / (self.cum[i + 1] - self.cum[i]).max(1e-6)).clamp(0.0, 1.0);
        (
            self.z[i] + (self.z[i + 1] - self.z[i]) * t,
            self.elevated[i] || self.elevated[i + 1],
        )
    }
}

/// The rail module's alignments (same natural ground as the pipeline: the seed's first
/// draws).
fn rails(plan: &Plan, cfg: &MetroConfig) -> Vec<Rail> {
    use super::rail::Support;
    let mut rng = wp_math::Rng::new(cfg.seed);
    let nat = super::ground::Natural::new(plan, &mut rng);
    super::rail::rail_lines(plan, &nat)
        .into_iter()
        .map(|a| {
            let xy: Vec<V2> = a.pts.iter().map(|p| geom::xy(*p)).collect();
            Rail {
                name: a.name,
                cum: cum2(&xy),
                xy,
                z: a.pts.iter().map(|p| p[2]).collect(),
                elevated: a
                    .support
                    .iter()
                    .map(|s| matches!(s, Support::Viaduct | Support::Bridge | Support::StationDeck))
                    .collect(),
            }
        })
        .collect()
}

/// Level crossings: planar street edges crossing a rail line on the ground are held at the
/// formation height across the tracks (both bounds).
fn rail_pins(g: &Graph, rails: &[Rail]) -> BTreeMap<usize, Vec<(f32, f32, f32, bool)>> {
    let mut out: BTreeMap<usize, Vec<(f32, f32, f32, bool)>> = BTreeMap::new();
    for (ei, e) in g.edges.iter().enumerate() {
        if e.sep.is_some() {
            continue;
        }
        for r in rails {
            for (s, sr, _) in geo::path_crossings(&e.path, &r.xy) {
                let (rz, elevated) = r.at(sr);
                if elevated {
                    continue;
                }
                let w = 10.0;
                out.entry(ei)
                    .or_default()
                    .push((s - w, s + w, rz - 0.05, true));
                out.entry(ei)
                    .or_default()
                    .push((s - w, s + w, rz + 0.05, false));
            }
        }
    }
    out
}

/// Context for clipping.
struct ClipCtx<'a> {
    plan: &'a Plan,
    gnd: &'a Ground,
    hws: &'a [Hw],
    ics: &'a IcOut,
    rails: &'a [Rail],
    rail_grid: SegGrid,
    hw_grid: SegGrid,
    hw_paths: Vec<Vec<V2>>,
    hw_cums: Vec<Vec<f32>>,
    corr_grid: SegGrid,
    protect: Vec<(Vec<V2>, f32)>,
}

impl ClipCtx<'_> {
    fn rail_near(&self, p: V2, r: f32) -> f32 {
        let mut best = f32::INFINITY;
        for (li, k) in self
            .rail_grid
            .query([p[0] - r, p[1] - r], [p[0] + r, p[1] + r])
        {
            let line = &self.rails[li as usize].xy;
            let k = k as usize;
            best = best.min(geom::point_segment_distance(p, line[k], line[k + 1]));
        }
        best
    }
}

fn hw_pts(hw: &Hw) -> Vec<V2> {
    let mut p = hw.path.clone();
    if hw.closed {
        p.push(hw.path[0]);
    }
    p
}

/// Clip one line; returns the surviving pieces.
fn clip_line(cx: &ClipCtx, l: &Line, extra: &[(f32, f32)]) -> Vec<Line> {
    let half = l.st.half_width();
    let mut bad: Vec<(f32, f32)> = extra.to_vec();
    let lo = plan::MAP_LO;
    let hi = plan::MAP_HI;
    let in_sep = |s: f32| l.sep_at(s).is_some();
    let is_local = l.st.class.rank() <= RoadClass::Local.rank();
    // Map edge.
    bad.extend(lines::bad_ranges(l, 5.0, 2.0, &|_, p| {
        p[0] < lo[0] + 25.0 || p[1] < lo[1] + 25.0 || p[0] > hi[0] - 25.0 || p[1] > hi[1] - 25.0
    }));
    // Water: short runs become bridges (handled before clipping); here only what is left.
    bad.extend(lines::bad_ranges(l, 4.0, 18.0, &|s, p| {
        !in_sep(s) && cx.gnd.z(p) < plan::WATER_Z + 0.8
    }));
    // Sites (except the access road's own site).
    for (si, site) in cx.plan.sites.iter().enumerate() {
        if l.gate.is_some_and(|g| g.0 == si) {
            continue;
        }
        let (blo, bhi) = geo::bbox(&site.polygon);
        let (llo, lhi) = geo::bbox(&l.path);
        let m = half + 3.0;
        if llo[0] > bhi[0] + m || lhi[0] < blo[0] - m || llo[1] > bhi[1] + m || lhi[1] < blo[1] - m
        {
            continue;
        }
        bad.extend(lines::bad_ranges(l, 4.0, 6.0, &|_, p| {
            signed_dist(&site.polygon, p) < half + 1.5
        }));
    }
    // Rail corridors.
    let mut cross_pts: Vec<V2> = Vec::new();
    for r in cx.rails {
        for (s, sr, p, sin) in lines::crossings_with(l, &r.xy) {
            cross_pts.push(p);
            let (rz, elevated) = r.at(sr);
            let g = cx.gnd.z(p);
            // Under a rail viaduct: any street; at grade: collectors and up, rail on the
            // ground; otherwise only planned structures.
            let under = elevated && rz - g > 7.0;
            let level = !elevated && (rz - g).abs() < 1.5 && l.rail_ok && sin > 0.34;
            let ok = under || level || l.sep_at(s).is_some();
            if !ok {
                bad.push((s - half - 20.0, s + half + 20.0));
            }
        }
    }
    bad.extend(lines::bad_ranges(l, 5.0, 8.0, &|s, p| {
        !in_sep(s)
            && cx.rail_near(p, 30.0) < half + 9.0
            && cross_pts.iter().all(|q| geom::dist(*q, p) > 55.0)
    }));
    // Highways: no parallel running; local streets do not cross outside viaducts / tunnels.
    let mut hw_cross_pts: Vec<V2> = Vec::new();
    for k in 0..l.path.len() - 1 {
        let (a, b) = (l.path[k], l.path[k + 1]);
        let qlo = [a[0].min(b[0]), a[1].min(b[1])];
        let qhi = [a[0].max(b[0]), a[1].max(b[1])];
        for (h, m) in cx.hw_grid.query(qlo, qhi) {
            let hw = &cx.hws[h as usize];
            let hp = &cx.hw_paths[h as usize];
            let m = m as usize;
            let Some((t, u)) = geo::seg_cross(a, b, hp[m], hp[m + 1]) else {
                continue;
            };
            let s = l.cum[k] + t * (l.cum[k + 1] - l.cum[k]);
            let sh = cx.hw_cums[h as usize][m]
                + u * (cx.hw_cums[h as usize][m + 1] - cx.hw_cums[h as usize][m]);
            let p = geom::add(a, geom::scale(geom::sub(b, a), t));
            hw_cross_pts.push(p);
            let sin = geom::cross(
                geom::normalize(geom::sub(b, a)),
                geom::normalize(geom::sub(hp[m + 1], hp[m])),
            )
            .abs();
            let in_span =
                hw.in_spans(&hw.viaducts, sh).is_some() || hw.in_spans(&hw.tunnels, sh).is_some();
            if (is_local && !in_span) || sin < 0.4 {
                let w = (hw.half_width() + half) / sin.max(0.3) + 22.0;
                bad.push((s - w, s + w));
            }
        }
    }
    bad.extend(lines::bad_ranges(l, 5.0, 10.0, &|s, p| {
        let r = 40.0 + half;
        let mut hit = false;
        for (h, m) in cx.hw_grid.query([p[0] - r, p[1] - r], [p[0] + r, p[1] + r]) {
            let hw = &cx.hws[h as usize];
            let hp = &cx.hw_paths[h as usize];
            let m = m as usize;
            let d = geom::point_segment_distance(p, hp[m], hp[m + 1]);
            let hwh = hw.half_width();
            if d > hwh + half + 16.0 || hw_cross_pts.iter().any(|q| geom::dist(*q, p) < hwh + 30.0)
            {
                continue;
            }
            let (_, dl) = l.at(s);
            let dh = geom::normalize(geom::sub(hp[m + 1], hp[m]));
            if geom::dot(dl, dh).abs() > 0.6 || d < hwh + 2.0 {
                hit = true;
                break;
            }
        }
        hit
    }));
    // Interchange corridors (local streets; crossroads keep their terminals).
    if !l.crossroad && l.st.class.rank() <= RoadClass::Collector.rank() {
        let local = is_local || l.st.class == RoadClass::Collector;
        if local {
            bad.extend(lines::bad_ranges(l, 4.0, 6.0, &|_, p| {
                let near = cx
                    .corr_grid
                    .query([p[0] - 1.0, p[1] - 1.0], [p[0] + 1.0, p[1] + 1.0]);
                near.iter().any(|&(ci, k)| {
                    let (path, w) = &cx.ics.corridors[ci as usize];
                    let k = k as usize;
                    geom::point_segment_distance(p, path[k], path[k + 1]) < w + half
                })
            }));
        }
    }
    // Protected scenic roads.
    if l.protect <= 0.0 {
        let (llo, lhi) = geo::bbox(&l.path);
        for (pp, w) in &cx.protect {
            let (plo, phi) = geo::bbox(pp);
            let m = w + half + 2.0;
            if llo[0] > phi[0] + m
                || lhi[0] < plo[0] - m
                || llo[1] > phi[1] + m
                || lhi[1] < plo[1] - m
            {
                continue;
            }
            let (a, b) = (pp[0], pp[pp.len() - 1]);
            bad.extend(lines::bad_ranges(l, 4.0, 8.0, &|_, p| {
                geom::dist(p, a) > 70.0 && geom::dist(p, b) > 70.0 && geo::dist_to(pp, p) < w + half
            }));
        }
    }
    // Keep forced terminals and gate ends.
    let keep: Vec<(f32, f32)> = l.forced.iter().map(|f| (f - 45.0, f + 45.0)).collect();
    let bad: Vec<(f32, f32)> = lines::merge_ranges(bad)
        .into_iter()
        .flat_map(|(a, b)| {
            // Remove the protected windows from a bad range.
            let mut parts = vec![(a, b)];
            for (k0, k1) in &keep {
                parts = parts
                    .into_iter()
                    .flat_map(|(x, y)| {
                        if *k1 <= x || *k0 >= y {
                            vec![(x, y)]
                        } else {
                            let mut v = Vec::new();
                            if *k0 > x {
                                v.push((x, *k0));
                            }
                            if *k1 < y {
                                v.push((*k1, y));
                            }
                            v
                        }
                    })
                    .collect();
            }
            parts
        })
        .collect();
    lines::cut(l, &bad, if l.keep_ends { 20.0 } else { 45.0 })
}

/// Line ends that stop under (or on) another line's grade-separated range are trimmed back
/// clear of it: `origin → bad ranges`.
fn ends_under_structures(raw: &[Line]) -> BTreeMap<usize, Vec<(f32, f32)>> {
    let mut out: BTreeMap<usize, Vec<(f32, f32)>> = BTreeMap::new();
    for (i, l) in raw.iter().enumerate() {
        if l.gate.is_some() {
            continue;
        }
        for (at_start, e) in [(true, l.path[0]), (false, l.path[l.path.len() - 1])] {
            for (j, o) in raw.iter().enumerate() {
                if i == j || o.seps.is_empty() {
                    continue;
                }
                let (so, d) = geo::project2(&o.path, &o.cum, e);
                let reach = o.st.half_width() + l.st.half_width() + 20.0;
                if d < reach && o.sep_at(so).is_some() {
                    let len = l.len();
                    let cut = reach + 25.0;
                    out.entry(i).or_default().push(if at_start {
                        (0.0, cut)
                    } else {
                        (len - cut, len)
                    });
                }
            }
        }
    }
    out
}

/// Small water crossings (canal) become bridges; returns how many.
fn auto_bridges(gnd: &Ground, raw: &mut [Line]) -> usize {
    let mut n = 0;
    for l in raw.iter_mut() {
        let runs = lines::bad_ranges(l, 3.0, 0.0, &|_, p| gnd.z(p) < plan::WATER_Z + 0.8);
        for (a, b) in runs {
            if b - a > 70.0
                || a < 15.0
                || b > l.len() - 15.0
                || l.sep_at(a).is_some()
                || l.sep_at(b).is_some()
            {
                continue;
            }
            let (c0, c1) = ((a - 10.0).max(0.0), (b + 10.0).min(l.len()));
            let g = gnd
                .z(l.at(c0).0)
                .max(gnd.z(l.at(c1).0))
                .max(plan::WATER_Z + 1.0);
            let deck = g + 1.2;
            let app = approach_len(deck - g + 1.0, &l.st);
            let len = l.len();
            l.seps.push(Sep {
                s0: (c0 - app).max(0.0),
                s1: (c1 + app).min(len),
                core0: c0,
                core1: c1,
                kind: SepKind::Bridge,
                bound: deck,
                name: "",
                covered: false,
                lanes: 0,
            });
            n += 1;
        }
    }
    n
}

/// Gate access lines (one per reachable gate) from the nearest clipped street.
fn gate_lines(
    plan: &Plan,
    gnd: &Ground,
    hws: &[Hw],
    pieces: &[Line],
    reach: &mut Vec<GateReach>,
) -> Vec<Line> {
    let mut out = Vec::new();
    let mut grid = SegGrid::new(plan::MAP_LO, plan::MAP_HI, 100.0);
    for (i, l) in pieces.iter().enumerate() {
        grid.insert_line(i as u32, &l.path, 0.0);
    }
    for (si, site) in plan.sites.iter().enumerate() {
        for (gi, g) in site.gates.iter().enumerate() {
            let inside_other = plan
                .sites
                .iter()
                .enumerate()
                .any(|(o, s)| o != si && signed_dist(&s.polygon, g.pos) < 3.0);
            if inside_other {
                reach.push(GateReach {
                    site: site.id,
                    gate: gi,
                    node: None,
                    reason: "gate lies on another site (reached through it)",
                });
                continue;
            }
            let dir = geo::dir_of(g.heading);
            let g_in = geom::add(g.pos, geom::scale(dir, 0.5));
            let o = geom::sub(g.pos, geom::scale(dir, 28.0));
            // Candidate street points, nearest first.
            let mut cands: Vec<(f32, V2)> = Vec::new();
            for r in [150.0f32, 400.0, 900.0] {
                for (li, k) in grid.query([o[0] - r, o[1] - r], [o[0] + r, o[1] + r]) {
                    let l = &pieces[li as usize];
                    if l.gate.is_some() || l.sep_at(l.cum[k as usize]).is_some() {
                        continue;
                    }
                    let (a, b) = (l.path[k as usize], l.path[k as usize + 1]);
                    let ab = geom::sub(b, a);
                    let t = (geom::dot(geom::sub(o, a), ab) / geom::dot(ab, ab).max(1e-9))
                        .clamp(0.0, 1.0);
                    let q = geom::add(a, geom::scale(ab, t));
                    cands.push((geom::dist(o, q), q));
                }
                if !cands.is_empty() {
                    break;
                }
            }
            cands.sort_by(|a, b| a.0.total_cmp(&b.0));
            let valid = |q: V2| -> bool {
                let path = [q, o, g.pos];
                for w in path.windows(2) {
                    let len = geom::dist(w[0], w[1]);
                    let n = (len / 4.0).ceil().max(1.0) as usize;
                    for k in 0..=n {
                        let p = geo::point_on(w, &cum2(w), len * k as f32 / n as f32).0;
                        if gnd.z(p) < plan::WATER_Z + 0.8 {
                            return false;
                        }
                        for (oi, s) in plan.sites.iter().enumerate() {
                            if oi != si && signed_dist(&s.polygon, p) < 4.0 {
                                return false;
                            }
                        }
                        if oi_inside(site, p, g.pos) {
                            return false;
                        }
                        for hw in hws {
                            let (_, _, d) = hw.project(p);
                            if d < hw.half_width() + 6.0 {
                                return false;
                            }
                        }
                    }
                }
                true
            };
            let pick = cands.iter().take(40).find(|c| valid(c.1));
            let Some(&(_, q)) = pick else {
                reach.push(GateReach {
                    site: site.id,
                    gate: gi,
                    node: None,
                    reason:
                        "no street can reach the gate without crossing a site, water or a highway",
                });
                continue;
            };
            let mut path = vec![q];
            if geom::dist(q, o) > 1.0 {
                path.push(o);
            }
            path.push(g.pos);
            path.push(g_in);
            let mut l = Line::new(path, style::STREET.at(20.0), site.name);
            l.gate = Some((si, gi));
            l.keep_ends = true;
            l.rail_ok = true;
            out.push(l);
            reach.push(GateReach {
                site: site.id,
                gate: gi,
                node: None,
                reason: "",
            });
        }
    }
    out
}

/// Whether `p` lies inside the gate's own site, away from the gate.
fn oi_inside(site: &plan::Site, p: V2, gate: V2) -> bool {
    signed_dist(&site.polygon, p) < -1.0 && geom::dist(p, gate) > 6.0
}

// ------------------------------------------------------------------------------------------
// Profiled objects and crossing constraints
// ------------------------------------------------------------------------------------------

/// A profiled road for crossing queries: dense centreline with heights.
#[derive(Debug, Clone)]
struct Prof {
    path: Vec<V2>,
    cum: Vec<f32>,
    z: Vec<f32>,
    half: f32,
}

impl Prof {
    fn z_at(&self, s: f32) -> f32 {
        let n = self.path.len();
        let s = s.clamp(0.0, self.cum[n - 1]);
        let i = match self.cum.binary_search_by(|c| c.total_cmp(&s)) {
            Ok(i) => i.min(n - 2),
            Err(i) => i.saturating_sub(1).min(n - 2),
        };
        let l = (self.cum[i + 1] - self.cum[i]).max(1e-6);
        self.z[i] + (self.z[i + 1] - self.z[i]) * ((s - self.cum[i]) / l).clamp(0.0, 1.0)
    }

    fn max_z(&self, s0: f32, s1: f32) -> f32 {
        let mut m = f32::NEG_INFINITY;
        let mut s = s0;
        while s <= s1 {
            m = m.max(self.z_at(s));
            s += 2.0;
        }
        m.max(self.z_at(s1))
    }

    fn min_z(&self, s0: f32, s1: f32) -> f32 {
        let mut m = f32::INFINITY;
        let mut s = s0;
        while s <= s1 {
            m = m.min(self.z_at(s));
            s += 2.0;
        }
        m.min(self.z_at(s1))
    }
}

/// Index of profiled objects for crossing queries.
struct ProfIndex {
    profs: Vec<Prof>,
    grid: SegGrid,
}

impl ProfIndex {
    fn new() -> Self {
        Self {
            profs: Vec::new(),
            grid: SegGrid::new(plan::MAP_LO, plan::MAP_HI, 100.0),
        }
    }

    fn add(&mut self, p: Prof) -> usize {
        let id = self.profs.len();
        self.grid.insert_line(id as u32, &p.path, 0.0);
        self.profs.push(p);
        id
    }

    /// Crossings of `path` with indexed objects (excluding `skip`): (own s, object, its s,
    /// point, sin of the angle).
    fn crossings(
        &self,
        path: &[V2],
        skip: &dyn Fn(usize) -> bool,
    ) -> Vec<(f32, usize, f32, V2, f32)> {
        let c = cum2(path);
        let mut out = Vec::new();
        for k in 0..path.len() - 1 {
            let (a, b) = (path[k], path[k + 1]);
            let lo = [a[0].min(b[0]), a[1].min(b[1])];
            let hi = [a[0].max(b[0]), a[1].max(b[1])];
            for (oi, m) in self.grid.query(lo, hi) {
                let oi = oi as usize;
                if skip(oi) {
                    continue;
                }
                let o = &self.profs[oi];
                let m = m as usize;
                if let Some((t, u)) = geo::seg_cross(a, b, o.path[m], o.path[m + 1]) {
                    let s = c[k] + t * (c[k + 1] - c[k]);
                    let so = o.cum[m] + u * (o.cum[m + 1] - o.cum[m]);
                    let p = geom::add(a, geom::scale(geom::sub(b, a), t));
                    let d1 = geom::normalize(geom::sub(b, a));
                    let d2 = geom::normalize(geom::sub(o.path[m + 1], o.path[m]));
                    out.push((s, oi, so, p, geom::cross(d1, d2).abs().max(0.2)));
                }
            }
        }
        out
    }
}

/// Bounds over a sample range from a crossing.
fn add_bound(
    lo: &mut [f32],
    hi: &mut [f32],
    ds: f32,
    s0: f32,
    s1: f32,
    v: f32,
    upper: bool,
    cyclic_total: Option<f32>,
) {
    let n = lo.len();
    let i0 = (s0 / ds).floor() as isize;
    let i1 = (s1 / ds).ceil() as isize;
    for i in i0..=i1 {
        let k = match cyclic_total {
            Some(_) => i.rem_euclid(n as isize) as usize,
            None => {
                if i < 0 || i >= n as isize {
                    continue;
                }
                i as usize
            }
        };
        if upper {
            lo[k] = lo[k].max(v);
        } else {
            hi[k] = hi[k].min(v);
        }
    }
}

// ------------------------------------------------------------------------------------------
// Street profiles
// ------------------------------------------------------------------------------------------

const STREET_DS: f32 = 4.0;

struct StreetProfiles {
    node_z: Vec<f32>,
    /// Per edge: heights at uniform samples (`ds` per edge) and the sample spacing.
    edge_z: Vec<(Vec<f32>, f32)>,
    flat: Vec<f32>,
    infeasible: usize,
}

fn smoothed_ground(gnd: &Ground, p: V2, r: f32) -> f32 {
    let mut acc = gnd.z(p);
    for (dx, dy) in [(r, 0.0), (-r, 0.0), (0.0, r), (0.0, -r)] {
        acc += gnd.z([p[0] + dx, p[1] + dy]);
    }
    acc / 5.0
}

/// Interior bounds of an edge from its own grade-separated range: (lo, hi) per sample.
fn edge_own_bounds(e: &graph::GEdge, gnd: &Ground, pts: &[V2], ds: f32) -> (Vec<f32>, Vec<f32>) {
    let n = pts.len();
    let mut lo = vec![f32::NEG_INFINITY; n];
    let mut hi = vec![f32::INFINITY; n];
    for (i, p) in pts.iter().enumerate() {
        let s = i as f32 * ds;
        let under = e.sep.is_some_and(|r| {
            r.kind == SepKind::Under && r.covered && s >= r.core0 - 40.0 && s <= r.core1 + 40.0
        });
        if !under {
            lo[i] = plan::WATER_Z + 1.05;
        }
        if let Some(r) = e.sep {
            if s >= r.core0 && s <= r.core1 {
                match r.kind {
                    SepKind::Bridge | SepKind::Over => lo[i] = lo[i].max(r.bound),
                    SepKind::Under => {
                        let floor = if r.covered {
                            f32::NEG_INFINITY
                        } else {
                            plan::WATER_Z + 1.1
                        };
                        hi[i] = hi[i].min((gnd.z(*p) - r.bound).max(floor));
                    }
                }
            }
        }
    }
    (lo, hi)
}

fn edge_samples(path: &[V2]) -> (Vec<V2>, f32) {
    let c = cum2(path);
    let len = c[c.len() - 1];
    let n = ((len / STREET_DS).ceil() as usize).max(1) + 1;
    let ds = len / (n - 1) as f32;
    (
        (0..n)
            .map(|i| geo::point_on(path, &c, i as f32 * ds).0)
            .collect(),
        ds,
    )
}

fn profile_streets(
    g: &Graph,
    gnd: &Ground,
    extra_deg: &[usize],
    extra_bounds: &BTreeMap<usize, Vec<(f32, f32, f32, bool)>>,
) -> StreetProfiles {
    let deg = g.degrees();
    let nn = g.nodes.len();
    let mut hw = vec![0.0f32; nn];
    for e in &g.edges {
        for n in [e.a, e.b] {
            hw[n] = hw[n].max(e.st.half_width());
        }
    }
    let flat: Vec<f32> = (0..nn)
        .map(|n| {
            let d = deg[n] + extra_deg[n];
            if d >= 3 {
                hw[n] * 1.6 + 4.0
            } else {
                4.0
            }
        })
        .collect();
    let mut z: Vec<f32> = g
        .nodes
        .iter()
        .map(|n| smoothed_ground(gnd, n.pos, 8.0).max(plan::WATER_Z + 1.0))
        .collect();
    // Precompute per-edge interior envelopes.
    struct Env {
        a: usize,
        b: usize,
        len: f32,
        fa: f32,
        fb: f32,
        la: f32,
        ua: f32,
        lb: f32,
        ub: f32,
        g: f32,
    }
    let mut envs = Vec::new();
    for (ei, e) in g.edges.iter().enumerate() {
        let (pts, ds) = edge_samples(&e.path);
        // Node heights follow the edges' own structures only: crossing-induced bounds that
        // cannot be met fail the clearance check and clip the crossing street instead.
        let (mut lo, hi) = edge_own_bounds(e, gnd, &pts, ds);
        let _ = ei;
        // The global min height is not an interior constraint for node relaxation.
        for v in lo.iter_mut() {
            if *v <= plan::WATER_Z + 1.0 + 1e-3 {
                *v = f32::NEG_INFINITY;
            }
        }
        let gr = e.st.grade * 0.85;
        let (l, u) = profile::envelopes(&lo, &hi, gr * ds, false);
        let len = (pts.len() - 1) as f32 * ds;
        let spread = 2.0
            * profile::smoothing_sigma(e.st.grade, profile::max_vertical_curvature(e.st.speed), ds);
        let fa = flat[e.a] + spread;
        let fb = flat[e.b] + spread;
        let ia = ((fa / ds) as usize).min(pts.len() - 1);
        let ib = (pts.len() - 1).saturating_sub((fb / ds) as usize);
        envs.push(Env {
            a: e.a,
            b: e.b,
            len,
            fa,
            fb,
            la: l[ia],
            ua: u[ia],
            lb: l[ib],
            ub: u[ib],
            g: gr,
        });
    }
    for _ in 0..300 {
        let mut moved = false;
        for e in &envs {
            let room = (e.len - e.fa - e.fb).max(e.len * 0.3);
            let allow = e.g * room;
            let (za, zb) = (z[e.a], z[e.b]);
            let excess = (za - zb).abs() - allow;
            if excess > 0.01 {
                let step = 0.5 * excess * (za - zb).signum();
                z[e.a] -= step * 0.5;
                z[e.b] += step * 0.5;
                moved = true;
            }
            for (n, l, u) in [(e.a, e.la, e.ua), (e.b, e.lb, e.ub)] {
                if l.is_finite() && z[n] < l - 0.01 {
                    z[n] = l;
                    moved = true;
                }
                if u.is_finite() && z[n] > u + 0.01 {
                    z[n] = u;
                    moved = true;
                }
            }
        }
        if !moved {
            break;
        }
    }
    let mut edge_z = Vec::with_capacity(g.edges.len());
    let mut infeasible = 0;
    for (ei, e) in g.edges.iter().enumerate() {
        let (pts, ds) = edge_samples(&e.path);
        let n = pts.len();
        let raw: Vec<f32> = pts.iter().map(|p| gnd.z(*p)).collect();
        let mut desired = geo::smooth_series(&raw, (12.0 / ds) as usize);
        // Under a tunnel / underpass: a straight line between the core ends' ground.
        if let Some(r) = e.sep {
            if r.kind == SepKind::Under {
                let i0 = (r.core0 / ds).floor().max(0.0) as usize;
                let i1 = ((r.core1 / ds).ceil() as usize).min(n - 1);
                let (z0, z1) = (desired[i0] - r.bound, desired[i1] - r.bound);
                for (i, d) in desired.iter_mut().enumerate().take(i1 + 1).skip(i0) {
                    let t = (i - i0) as f32 / (i1 - i0).max(1) as f32;
                    *d = (z0 + (z1 - z0) * t).min(*d);
                }
            }
        }
        let (mut lo, mut hi) = edge_own_bounds(e, gnd, &pts, ds);
        if let Some(b) = extra_bounds.get(&ei) {
            for &(s0, s1, v, upper) in b {
                add_bound(&mut lo, &mut hi, ds, s0, s1, v, upper, None);
            }
        }
        let kmax = profile::max_vertical_curvature(e.st.speed);
        let spread = 2.0 * profile::smoothing_sigma(e.st.grade, kmax, ds);
        let (za, zb) = (z[e.a], z[e.b]);
        let fa = (((flat[e.a] + spread) / ds).ceil() as usize).min(n - 1);
        let fb = (((flat[e.b] + spread) / ds).ceil() as usize).min(n - 1);
        let pa = ((flat[e.a] / ds).ceil() as usize).min(n - 1);
        let pb = ((flat[e.b] / ds).ceil() as usize).min(n - 1);
        let short = fa + fb + 3 >= n && e.sep.is_none();
        let zs = if short {
            (0..n)
                .map(|i| za + (zb - za) * i as f32 / (n - 1) as f32)
                .collect()
        } else {
            // Junction approaches flat (box + smoothing run-out); if that leaves too little
            // room, only the boxes are fixed; failing that, a straight line.
            let pin: Vec<bool> = (0..n).map(|i| i <= pa || i + pb >= n - 1).collect();
            let mut best: Option<profile::Profile> = None;
            for (ra, rb) in [(fa, fb), (pa, pb)] {
                let (mut l2, mut h2) = (lo.clone(), hi.clone());
                for i in 0..=ra {
                    l2[i] = za;
                    h2[i] = za;
                }
                for i in (n - 1 - rb.min(n - 1))..n {
                    l2[i] = zb;
                    h2[i] = zb;
                }
                let p = profile::solve(&desired, ds, &l2, &h2, &pin, e.st.grade, kmax, false);
                let ok = p.infeasible == 0;
                best = Some(p);
                if ok {
                    break;
                }
            }
            let p = best.expect("profile");
            infeasible += usize::from(p.infeasible > 0);
            p.z
        };
        edge_z.push((zs, ds));
    }
    StreetProfiles {
        node_z: z,
        edge_z,
        flat,
        infeasible,
    }
}

// ------------------------------------------------------------------------------------------
// Highway profiles
// ------------------------------------------------------------------------------------------

/// Bank from curvature: superelevation for 55 % of the lateral acceleration at the limit.
fn bank_from(path: &[V2], speed: f32, max: f32, window_m: f32) -> Vec<f32> {
    let k = geo::curvature(path);
    let v2 = speed * speed;
    let raw: Vec<f32> = k
        .iter()
        .map(|c| (c * v2 / 9.81 * 0.55).clamp(-max, max))
        .collect();
    let c = cum2(path);
    let spacing = (c[c.len() - 1] / (path.len() - 1).max(1) as f32).max(1.0);
    geo::smooth_series(&raw, (window_m / spacing) as usize)
}

/// Profile a highway against the streets (and earlier highways) it crosses. Returns the
/// crossings it made `(own s, prof id, other s, point, upper?)`.
fn profile_highway(
    hw: &mut Hw,
    gnd: &Ground,
    idx: &ProfIndex,
    street_ids: &dyn Fn(usize) -> bool,
    rail: &[Rail],
    notes: &mut Vec<String>,
) -> Vec<(f32, usize, f32, V2, bool)> {
    let n = hw.sample_count();
    let ds = hw.sample_ds();
    let cyclic = hw.closed;
    let pts: Vec<V2> = (0..n).map(|i| hw.at(i as f32 * ds).0).collect();
    let raw: Vec<f32> = pts.iter().map(|p| gnd.z(*p)).collect();
    let mut desired = smooth_cyclic(&raw, (40.0 / ds) as usize, cyclic);
    let mut lo = vec![f32::NEG_INFINITY; n];
    let mut hi = vec![f32::INFINITY; n];
    for i in 0..n {
        let s = i as f32 * ds;
        let tunnel = hw.in_spans(&hw.tunnels, s);
        if let Some(t) = tunnel {
            let v = raw[i].min(desired[i]) - t.h;
            hi[i] = hi[i].min(v);
            desired[i] = v;
        } else {
            lo[i] = plan::WATER_Z + 1.35;
        }
        if let Some(v) = hw.in_spans(&hw.viaducts, s) {
            lo[i] = lo[i].max(raw[i].max(plan::WATER_Z) + v.h);
        }
    }
    // Water outside designated spans: a deck over it.
    for i in 0..n {
        if raw[i] < plan::WATER_Z + 0.5 && hi[i].is_infinite() {
            lo[i] = lo[i].max(plan::WATER_Z + 10.0);
        }
    }
    // Gate ends at the ground.
    if hw.end_gate.is_some() {
        for i in n.saturating_sub(8)..n {
            lo[i] = raw[i];
            hi[i] = raw[i];
        }
    }
    let total = hw.total();
    let ctot = cyclic.then_some(total);
    let path = {
        let mut p = pts.clone();
        if cyclic {
            p.push(pts[0]);
        }
        p
    };
    let kmax = profile::max_vertical_curvature(hw.lanes_at(0.0).1);
    let grade = style::HIGHWAY.grade;
    let pin = vec![false; n];
    // Preliminary profile (structures only) decides over / under per crossing.
    let prelim = profile::solve(&desired, ds, &lo, &hi, &pin, grade, kmax, cyclic).z;
    let half = hw.half_width();
    let mut made = Vec::new();
    for (s, oi, so, p, sin) in idx.crossings(&path, &|_| false) {
        let o = &idx.profs[oi];
        let w_own = (o.half + 6.0) / sin;
        let w_other = (half + 6.0) / sin;
        let own_z = profile::sample(&prelim, ds, s.min(total));
        let oz_max = o.max_z((so - w_other).max(0.0), so + w_other);
        let oz_min = o.min_z((so - w_other).max(0.0), so + w_other);
        let under = own_z < oz_min - 3.0;
        if under {
            add_bound(
                &mut lo,
                &mut hi,
                ds,
                s - w_own,
                s + w_own,
                oz_min - CLEARANCE_M,
                false,
                ctot,
            );
        } else {
            add_bound(
                &mut lo,
                &mut hi,
                ds,
                s - w_own - 10.0,
                s + w_own + 10.0,
                oz_max + CLEARANCE_M,
                true,
                ctot,
            );
        }
        made.push((s, oi, so, p, !under));
        let _ = street_ids(oi);
    }
    // Rail lines: over, or under inside a tunnel.
    for r in rail {
        for (s, sr, _p) in geo::path_crossings(&path, &r.xy) {
            let g = r.at(sr).0;
            let in_tunnel = hw.in_spans(&hw.tunnels, s).is_some();
            let w = half + 12.0;
            if in_tunnel {
                add_bound(
                    &mut lo,
                    &mut hi,
                    ds,
                    s - w,
                    s + w,
                    g - CLEARANCE_M,
                    false,
                    ctot,
                );
            } else {
                add_bound(
                    &mut lo,
                    &mut hi,
                    ds,
                    s - w,
                    s + w,
                    g + CLEARANCE_M,
                    true,
                    ctot,
                );
            }
        }
    }
    let p = profile::solve(&desired, ds, &lo, &hi, &pin, grade, kmax, cyclic);
    if p.infeasible > 0 {
        notes.push(format!(
            "{}: {} profile samples could not meet every bound",
            hw.name, p.infeasible
        ));
    }
    hw.z = p.z;
    // Bank from curvature (cyclic smoothing by wrapping the series).
    let speed = hw.lanes.iter().map(|l| l.2).fold(0.0f32, f32::max);
    let mut bp = pts.clone();
    if cyclic {
        let m = 12.min(n);
        let mut ext: Vec<V2> = pts[n - m..].to_vec();
        ext.extend_from_slice(&pts);
        ext.extend_from_slice(&pts[..m]);
        let b = bank_from(&ext, speed, style::HIGHWAY.bank_max, 60.0);
        hw.bank = b[m..m + n].to_vec();
    } else {
        bp.dedup_by(|a, b| geom::dist(*a, *b) < 0.01);
        let mut b = bank_from(&bp, speed, style::HIGHWAY.bank_max, 60.0);
        b.resize(n, 0.0);
        // Fade the bank out at gate ends.
        if hw.end_gate.is_some() {
            for (i, v) in b.iter_mut().enumerate() {
                *v *= smoothstep(0.0, 80.0, (n - 1 - i) as f32 * ds - 40.0);
            }
        }
        hw.bank = b;
    }
    made
}

fn smooth_cyclic(v: &[f32], half: usize, cyclic: bool) -> Vec<f32> {
    if !cyclic {
        return geo::smooth_series(v, half);
    }
    let n = v.len();
    let m = half.min(n);
    let mut ext: Vec<f32> = v[n - m..].to_vec();
    ext.extend_from_slice(v);
    ext.extend_from_slice(&v[..m]);
    geo::smooth_series(&ext, half)[m..m + n].to_vec()
}

// ------------------------------------------------------------------------------------------
// Assembly
// ------------------------------------------------------------------------------------------

/// Builder node registry with de-duplication of gores.
struct Nodes {
    specs: Vec<NodeSpec>,
}

impl Nodes {
    fn add(&mut self, pos: V2, z: f32, control: Option<ControlHint>) -> usize {
        self.specs.push(NodeSpec {
            pos,
            z,
            signal: false,
            control,
        });
        self.specs.len() - 1
    }
}

/// An attachment point on a carriageway.
#[derive(Debug, Clone, Copy)]
struct Attach {
    s: f32,
    node: usize,
}

fn spec_from_pts(a: usize, b: usize, pts: &[V3], bank: Vec<f32>, st: &Style) -> RoadSpec {
    let mut r = RoadSpec::new(a, b, st.class, st.fwd, st.bwd);
    r.speed_mps = st.speed;
    r.parking = st.parking;
    r.bus_lane = st.bus;
    r.via = pts[1..pts.len() - 1].iter().map(|p| geom::xy(*p)).collect();
    r.via_z = pts[1..pts.len() - 1].iter().map(|p| p[2]).collect();
    r.bank = bank;
    r
}

/// Build the road network (see the module docs for the contract).
pub fn build(plan: &Plan, ground: &mut Ground, cfg: &MetroConfig) -> RoadsOut {
    let mut notes: Vec<String> = Vec::new();
    let mut hws = build_highways(plan);
    let mut raw = streets::layout(cfg.seed);
    let (ics, ic_list) = design_interchanges(plan, &hws, &raw);
    notes.extend(ics.notes.iter().cloned());
    let rail_lines = rails(plan, cfg);
    attach_structures(plan, ground, &rail_lines, &mut raw, &mut notes);
    let nb = auto_bridges(ground, &mut raw);
    notes.push(format!(
        "roads: {nb} small bridges (canal / creek crossings)"
    ));
    // Terminals as forced points on their crossroads.
    let mut term_line: Vec<Option<usize>> = vec![None; ics.terminals.len()];
    for (ti, t) in ics.terminals.iter().enumerate() {
        let best = raw
            .iter()
            .enumerate()
            .filter(|(_, l)| l.name == t.line)
            .map(|(i, l)| (i, geo::project2(&l.path, &l.cum, t.pos)))
            .min_by(|a, b| a.1 .1.total_cmp(&b.1 .1));
        if let Some((li, (s, d))) = best {
            if d < 2.0 {
                raw[li].forced.push(s);
                raw[li].crossroad = true;
                term_line[ti] = Some(li);
                continue;
            }
        }
        notes.push(format!(
            "terminal on {} at {:?} not on its crossroad",
            t.line, t.pos
        ));
    }
    // Clip context.
    let mut rail_grid = SegGrid::new(plan::MAP_LO, plan::MAP_HI, 100.0);
    for (i, r) in rail_lines.iter().enumerate() {
        rail_grid.insert_line(i as u32, &r.xy, 0.0);
    }
    let mut hw_grid = SegGrid::new(plan::MAP_LO, plan::MAP_HI, 100.0);
    for (i, h) in hws.iter().enumerate() {
        hw_grid.insert_line(i as u32, &hw_pts(h), 0.0);
    }
    let mut corr_grid = SegGrid::new(plan::MAP_LO, plan::MAP_HI, 50.0);
    for (i, (p, w)) in ics.corridors.iter().enumerate() {
        corr_grid.insert_line(i as u32, p, *w + 8.0);
    }
    let protect: Vec<(Vec<V2>, f32)> = raw
        .iter()
        .filter(|l| l.protect > 0.0)
        .map(|l| (l.path.clone(), l.protect))
        .collect();
    let hws_clip = hws.clone();
    let cx = ClipCtx {
        plan,
        gnd: ground,
        hws: &hws_clip,
        ics: &ics,
        rails: &rail_lines,
        rail_grid,
        hw_grid,
        hw_paths: hws_clip.iter().map(hw_pts).collect(),
        hw_cums: hws_clip.iter().map(|h| cum2(&hw_pts(h))).collect(),
        corr_grid,
        protect,
    };
    let rail = rail_lines.clone();

    // Iterate: clip → graph → profiles → failed crossings clip more.
    let mut extra: BTreeMap<usize, Vec<(f32, f32)>> = ends_under_structures(&raw);
    let mut cache: Vec<Option<(usize, Vec<Line>)>> = vec![None; raw.len()];
    let mut round = 0;
    let (g, sp, gate_reach, lines_used, hw_cross, ramp_profiles, idx, hw_ids) = loop {
        round += 1;
        let mut pieces: Vec<Line> = Vec::new();
        for (oi, l) in raw.iter().enumerate() {
            let ex = extra.get(&oi).cloned().unwrap_or_default();
            if cache[oi].as_ref().is_none_or(|c| c.0 != ex.len()) {
                let mut v = clip_line(&cx, l, &ex);
                for p in v.iter_mut() {
                    p.origin = oi;
                }
                cache[oi] = Some((ex.len(), v));
            }
            pieces.extend(cache[oi].as_ref().expect("cached").1.iter().cloned());
        }
        let mut gate_reach = Vec::new();
        let gl = gate_lines(plan, ground, &hws, &pieces, &mut gate_reach);
        let first_gate = pieces.len();
        for (k, mut l) in gl.into_iter().enumerate() {
            l.origin = raw.len() + k;
            pieces.push(l);
        }
        let _ = first_gate;
        let mut g = graph::planarize(&pieces, plan::MAP_LO, plan::MAP_HI);
        let lost = g.keep_largest_component();
        let fixed = g.fix_one_way();
        g.compact();
        // Ramp terminal degree at street nodes.
        let mut extra_deg = vec![0usize; g.nodes.len()];
        let term_node: Vec<Option<usize>> = ics
            .terminals
            .iter()
            .map(|t| {
                g.nodes
                    .iter()
                    .position(|n| n.forced && geom::dist(n.pos, t.pos) < 1.0)
            })
            .collect();
        for r in &ics.ramps {
            for e in [r.from, r.to] {
                if let RampEnd::Terminal(ti) = e {
                    if let Some(n) = term_node[ti] {
                        extra_deg[n] += 1;
                    }
                }
            }
        }
        // Planar streets first, then grade-separated street edges against them.
        let pins = rail_pins(&g, &rail_lines);
        let sp0 = profile_streets(&g, ground, &extra_deg, &pins);
        let mut idx = ProfIndex::new();
        let mut street_prof = vec![usize::MAX; g.edges.len()];
        for (ei, e) in g.edges.iter().enumerate() {
            if e.sep.is_none() {
                let (z, ds) = &sp0.edge_z[ei];
                let (pts, _) = edge_samples(&e.path);
                let _ = ds;
                street_prof[ei] = idx.add(Prof {
                    cum: cum2(&pts),
                    path: pts,
                    z: z.clone(),
                    half: e.st.half_width(),
                });
            }
        }
        let mut sep_bounds: BTreeMap<usize, Vec<(f32, f32, f32, bool)>> = pins.clone();
        let mut street_fail: Vec<(usize, f32)> = Vec::new();
        let prof_edge0: BTreeMap<usize, usize> = street_prof
            .iter()
            .enumerate()
            .filter(|(_, p)| **p != usize::MAX)
            .map(|(e, p)| (*p, e))
            .collect();
        // The same road continuing from a structure edge's end node is a joint, not a
        // crossing (other streets meeting there are handled as crossings: they get clipped).
        let joint = |e: &graph::GEdge, oe: usize, s: f32| {
            let o = &g.edges[oe];
            let shared = [e.a, e.b].into_iter().any(|n| n == o.a || n == o.b);
            let same = pieces[o.line].origin == pieces[e.line].origin;
            shared && same && (s < 3.0 || s > e.len() - 3.0)
        };
        for (ei, e) in g.edges.iter().enumerate() {
            let Some(r) = e.sep else { continue };
            let (pts, _) = edge_samples(&e.path);
            for (s, oi, so, _p, sin) in idx.crossings(&pts, &|_| false) {
                if joint(e, prof_edge0[&oi], s) {
                    continue;
                }
                let o = &idx.profs[oi];
                let w_own = (o.half + 5.0) / sin;
                let w_o = (e.st.half_width() + 5.0) / sin;
                match r.kind {
                    SepKind::Under => {
                        let v = o.min_z((so - w_o).max(0.0), so + w_o) - CLEARANCE_M;
                        sep_bounds
                            .entry(ei)
                            .or_default()
                            .push((s - w_own, s + w_own, v, false));
                    }
                    _ => {
                        let v = o.max_z((so - w_o).max(0.0), so + w_o) + CLEARANCE_M;
                        sep_bounds.entry(ei).or_default().push((
                            s - w_own - 8.0,
                            s + w_own + 8.0,
                            v,
                            true,
                        ));
                    }
                }
            }
        }
        let sp = profile_streets(&g, ground, &extra_deg, &sep_bounds);
        // Verify street-street separations; failures clip the lower (planar) street.
        let mut idx = ProfIndex::new();
        let mut street_prof = vec![usize::MAX; g.edges.len()];
        for (ei, e) in g.edges.iter().enumerate() {
            let (z, _) = &sp.edge_z[ei];
            let (pts, _) = edge_samples(&e.path);
            street_prof[ei] = idx.add(Prof {
                cum: cum2(&pts),
                path: pts,
                z: z.clone(),
                half: e.st.half_width(),
            });
        }
        let prof_edge: BTreeMap<usize, usize> = street_prof
            .iter()
            .enumerate()
            .map(|(e, p)| (*p, e))
            .collect();
        for (ei, e) in g.edges.iter().enumerate() {
            if e.sep.is_none() {
                continue;
            }
            let own = street_prof[ei];
            let o = &idx.profs[own];
            for (s, oi, so, _, _) in idx.crossings(&o.path.clone(), &|x| x == own) {
                let oe = prof_edge[&oi];
                if g.edges[oe].sep.is_some() {
                    continue;
                }
                if joint(e, oe, s) {
                    continue;
                }
                let dz = (idx.profs[own].z_at(s) - idx.profs[oi].z_at(so)).abs();
                if dz < 7.5 {
                    street_fail.push((oe, so));
                }
            }
        }
        // Highways in order: Crosstown, spurs, then Loop 9 (over the Crosstown).
        let mut hw_cross: Vec<(usize, Vec<(f32, usize, f32, V2, bool)>)> = Vec::new();
        let mut hw_ids = vec![usize::MAX; hws.len()];
        let n_street_profs = idx.profs.len();
        for h in [
            hwi::CROSSTOWN,
            hwi::R40N,
            hwi::AIRPORT,
            hwi::PORT,
            hwi::LOOP,
        ] {
            let made = profile_highway(
                &mut hws[h],
                ground,
                &idx,
                &|oi| oi < n_street_profs,
                &rail,
                &mut notes,
            );
            let hw = &hws[h];
            let n = hw.sample_count();
            let ds = hw.sample_ds();
            let mut pts: Vec<V2> = (0..n).map(|i| hw.at(i as f32 * ds).0).collect();
            let mut z = hw.z.clone();
            if hw.closed {
                pts.push(pts[0]);
                z.push(z[0]);
            }
            hw_ids[h] = idx.add(Prof {
                cum: cum2(&pts),
                path: pts,
                z,
                half: hw.half_width(),
            });
            hw_cross.push((h, made));
        }
        // Check highway crossings; failures clip the street.
        for (h, made) in &hw_cross {
            let hw = &hws[*h];
            for &(s, oi, so, _p, upper) in made {
                if oi >= n_street_profs {
                    continue;
                }
                let own = hw.z_at(s);
                let other = idx.profs[oi].z_at(so);
                let dz = if upper { own - other } else { other - own };
                if dz < 7.5 {
                    street_fail.push((prof_edge[&oi], so));
                }
            }
        }
        // Ramps (level order).
        let ramp_profiles = profile_ramps(
            &ics,
            &hws,
            &g,
            &sp,
            &term_node,
            ground,
            &mut idx,
            &prof_edge,
            n_street_profs,
            &mut street_fail,
        );
        if street_fail.is_empty() || round >= 4 {
            if !street_fail.is_empty() {
                notes.push(format!(
                    "roads: {} grade separations still short after {round} rounds",
                    street_fail.len()
                ));
            }
            notes.push(format!("roads: {round} profile rounds, {lost} edges in small components dropped, {fixed} one-way edges made two-way"));
            break (
                g,
                sp,
                gate_reach,
                pieces,
                hw_cross,
                ramp_profiles,
                idx,
                hw_ids,
            );
        }
        // Clip the failing streets around the crossing and go again.
        for (ei, so) in street_fail {
            let e = &g.edges[ei];
            let line = &pieces[e.line];
            if line.gate.is_some() {
                continue;
            }
            let s_line = line.base_s + e.on_line.0 + so;
            extra
                .entry(line.origin)
                .or_default()
                .push((s_line - 35.0, s_line + 35.0));
        }
    };
    let _ = (&lines_used, &hw_cross, &hw_ids);
    assemble(
        plan,
        ground,
        cfg,
        &hws,
        &ics,
        ic_list,
        &g,
        &sp,
        gate_reach,
        ramp_profiles,
        idx,
        notes,
    )
}

/// Profiled ramp: 3D points (travel order) and bank.
struct RampProf {
    pts: Vec<V3>,
    bank: Vec<f32>,
}

/// Height of a ramp end.
fn end_z(e: RampEnd, hws: &[Hw], sp: &StreetProfiles, term_node: &[Option<usize>], pos: V2) -> f32 {
    match e {
        RampEnd::Gore { hw, .. } => hws[hw].plane_z(pos),
        RampEnd::Spur { hw, at_start, .. } => {
            let h = &hws[hw];
            let s = if at_start { 0.0 } else { h.total() };
            let (p, d) = h.at(s);
            h.z_at(s) + h.bank_at(s) * geom::dot(geom::sub(pos, p), geom::right_of(d))
        }
        RampEnd::Terminal(ti) => term_node[ti].map_or(0.0, |n| sp.node_z[n]),
    }
}

#[allow(clippy::too_many_arguments)]
fn profile_ramps(
    ics: &IcOut,
    hws: &[Hw],
    g: &Graph,
    sp: &StreetProfiles,
    term_node: &[Option<usize>],
    gnd: &Ground,
    idx: &mut ProfIndex,
    prof_edge: &BTreeMap<usize, usize>,
    n_street: usize,
    fail: &mut Vec<(usize, f32)>,
) -> Vec<RampProf> {
    let mut out: Vec<Option<RampProf>> = (0..ics.ramps.len()).map(|_| None).collect();
    let mut order: Vec<usize> = (0..ics.ramps.len()).collect();
    order.sort_by_key(|i| (ics.ramps[*i].level, *i));
    let mut ramp_ids: Vec<usize> = vec![usize::MAX; ics.ramps.len()];
    for ri in order {
        let r = &ics.ramps[ri];
        let path = geo::resample(&r.path, 4.0);
        let c = cum2(&path);
        let len = c[c.len() - 1];
        let n = path.len();
        let ds = len / (n - 1) as f32;
        let z0 = end_z(r.from, hws, sp, term_node, path[0]);
        let z1 = end_z(r.to, hws, sp, term_node, path[n - 1]);
        let mut lo = vec![plan::WATER_Z + 1.0; n];
        let mut hi = vec![f32::INFINITY; n];
        let mut desired: Vec<f32> = (0..n)
            .map(|i| z0 + (z1 - z0) * smoothstep(0.0, 1.0, c[i] / len))
            .collect();
        let plane = |e: RampEnd, p: V2| -> Option<f32> {
            match e {
                RampEnd::Gore { hw, .. } => Some(hws[hw].plane_z(p)),
                RampEnd::Spur { hw, .. } => Some(hws[hw].plane_z(p)),
                RampEnd::Terminal(_) => None,
            }
        };
        let mut pin = vec![false; n];
        for i in 0..n {
            if c[i] <= r.par_from + 0.5 {
                if let Some(z) = plane(r.from, path[i]) {
                    lo[i] = z;
                    hi[i] = z;
                    desired[i] = z;
                    pin[i] = true;
                }
            }
            if len - c[i] <= r.par_to + 0.5 {
                if let Some(z) = plane(r.to, path[i]) {
                    lo[i] = z;
                    hi[i] = z;
                    desired[i] = z;
                    pin[i] = true;
                }
            }
        }
        // End anchors.
        let flat_end = |e: RampEnd| -> f32 {
            match e {
                RampEnd::Terminal(ti) => term_node[ti].map_or(4.0, |nn| sp.flat[nn]),
                _ => 0.0,
            }
        };
        let (fa, fb) = (flat_end(r.from), flat_end(r.to));
        for i in 0..n {
            if c[i] <= fa {
                lo[i] = z0;
                hi[i] = z0;
                pin[i] = true;
            }
            if len - c[i] <= fb {
                lo[i] = z1;
                hi[i] = z1;
                pin[i] = true;
            }
        }
        lo[0] = z0;
        hi[0] = z0;
        lo[n - 1] = z1;
        hi[n - 1] = z1;
        pin[0] = true;
        pin[n - 1] = true;
        // Crossings with streets, highways and earlier ramps: the ramp goes over.
        let mut crosses: Vec<(f32, usize, f32)> = Vec::new();
        for (s, oi, so, _p, sin) in idx.crossings(&path, &|_| false) {
            // Skip the ramp's own highway attachments (crossings within the parallel parts).
            if s < r.par_from + 5.0 || s > len - r.par_to - 5.0 {
                continue;
            }
            let o = &idx.profs[oi];
            let w_own = (o.half + 5.0) / sin;
            let w_o = (r.st.half_width() + 6.0) / sin;
            let v = o.max_z((so - w_o).max(0.0), so + w_o) + CLEARANCE_M;
            add_bound(
                &mut lo,
                &mut hi,
                ds,
                s - w_own - 8.0,
                s + w_own + 8.0,
                v,
                true,
                None,
            );
            crosses.push((s, oi, so));
        }
        let kmax = profile::max_vertical_curvature(r.st.speed);
        let p = profile::solve(&desired, ds, &lo, &hi, &pin, r.st.grade, kmax, false);
        let z = p.z;
        for &(s, oi, so) in &crosses {
            let dz = profile::sample(&z, ds, s) - idx.profs[oi].z_at(so);
            if dz < 7.5 && oi < n_street {
                if let Some(e) = prof_edge.get(&oi) {
                    fail.push((*e, so));
                }
            }
        }
        // Bank.
        let curve = bank_from(&path, r.st.speed, r.st.bank_max, 30.0);
        let bank: Vec<f32> = (0..n)
            .map(|i| {
                let from_hw = |e: RampEnd, at_start: bool| -> Option<f32> {
                    let (h, side) = match e {
                        RampEnd::Gore { hw, side, .. } | RampEnd::Spur { hw, side, .. } => {
                            (hw, side)
                        }
                        RampEnd::Terminal(_) => return None,
                    };
                    let hw = &hws[h];
                    let (s, _, _) = hw.project(path[i]);
                    let _ = at_start;
                    Some(side * hw.bank_at(s))
                };
                let w0 = smoothstep(r.par_from + 40.0, r.par_from, c[i]);
                let w1 = smoothstep(r.par_to + 40.0, r.par_to, len - c[i]);
                let mut b = curve[i];
                // Fade to 0 at terminals.
                b *= smoothstep(0.0, 50.0, c[i] - fa) * smoothstep(0.0, 50.0, len - c[i] - fb);
                if let Some(hb) = from_hw(r.from, true) {
                    b = b * (1.0 - w0) + hb * w0;
                }
                if let Some(hb) = from_hw(r.to, false) {
                    b = b * (1.0 - w1) + hb * w1;
                }
                b.clamp(-r.st.bank_max, r.st.bank_max)
            })
            .collect();
        let pts3: Vec<V3> = path.iter().zip(&z).map(|(p, z)| [p[0], p[1], *z]).collect();
        ramp_ids[ri] = idx.add(Prof {
            path: path.clone(),
            cum: c.clone(),
            z: z.clone(),
            half: r.st.half_width(),
        });
        out[ri] = Some(RampProf { pts: pts3, bank });
        let _ = (g, gnd);
    }
    out.into_iter()
        .map(|o| o.expect("every ramp profiled"))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn assemble(
    plan: &Plan,
    ground: &mut Ground,
    _cfg: &MetroConfig,
    hws: &[Hw],
    ics: &IcOut,
    ic_list: Vec<(String, InterchangeKind)>,
    g: &Graph,
    sp: &StreetProfiles,
    mut gate_reach: Vec<GateReach>,
    ramps: Vec<RampProf>,
    _idx: ProfIndex,
    mut notes: Vec<String>,
) -> RoadsOut {
    let mut nodes = Nodes { specs: Vec::new() };
    let mut specs: Vec<RoadSpec> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut info: Vec<RoadInfo> = Vec::new();
    // Street nodes.
    let term_control: BTreeMap<usize, Option<ControlHint>> = BTreeMap::new();
    let mut term_node: Vec<Option<usize>> = vec![None; ics.terminals.len()];
    for (ti, t) in ics.terminals.iter().enumerate() {
        term_node[ti] = g
            .nodes
            .iter()
            .position(|n| n.forced && geom::dist(n.pos, t.pos) < 1.0);
    }
    let _ = term_control;
    for (ti, t) in ics.terminals.iter().enumerate() {
        if term_node[ti].is_none() {
            notes.push(format!(
                "terminal on {} at ({:.0}, {:.0}) lost in the street graph",
                t.line, t.pos[0], t.pos[1]
            ));
        }
    }
    for (i, n) in g.nodes.iter().enumerate() {
        let ctl = ics
            .terminals
            .iter()
            .enumerate()
            .find(|(ti, _)| term_node[*ti] == Some(i))
            .and_then(|(_, t)| t.control);
        nodes.add(n.pos, sp.node_z[i], ctl);
    }
    // Street edges.
    for (ei, e) in g.edges.iter().enumerate() {
        let (z, ds) = &sp.edge_z[ei];
        let mut st = e.st;
        if let Some(r) = e.sep {
            if r.lanes > 0 {
                st = Style {
                    fwd: r.lanes,
                    bwd: if st.bwd > 0 { r.lanes } else { 0 },
                    bus: false,
                    parking: false,
                    ..st
                };
            } else if r.kind != SepKind::Under {
                st.parking = false;
            }
        }
        let res = geo::resample(&e.path, 8.0);
        let rc = cum2(&res);
        let pts: Vec<V3> = res
            .iter()
            .zip(&rc)
            .map(|(p, s)| [p[0], p[1], profile::sample(z, *ds, *s)])
            .collect();
        let bank = if st.bank_max > 0.0 {
            let b = bank_from(&res, st.speed, st.bank_max, 30.0);
            let len = rc[rc.len() - 1];
            b.iter()
                .zip(&rc)
                .map(|(v, s)| {
                    v * smoothstep(0.0, 50.0, (s - sp.flat[e.a]).min(len - s - sp.flat[e.b]))
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut r = spec_from_pts(e.a, e.b, &pts, bank, &st);
        r.via_z = pts[1..pts.len() - 1].iter().map(|p| p[2]).collect();
        specs.push(r);
        names.push(e.name.to_string());
        let mut covered = Vec::new();
        if let Some(sep) = e.sep {
            if sep.covered {
                covered.push((sep.core0, sep.core1));
            }
        }
        info.push(RoadInfo {
            kind: RoadKind::Street,
            grade: st.grade,
            owner: None,
            is_loop: false,
            covered,
        });
    }
    // Gate nodes.
    for gr in gate_reach.iter_mut() {
        if gr.reason.is_empty() {
            let site = plan.site(gr.site).expect("site");
            let gp = site.gates[gr.gate].pos;
            // The access road ends 3 m inside the gate: its end node.
            let best = g
                .nodes
                .iter()
                .enumerate()
                .map(|(i, n)| (i, geom::dist(n.pos, gp)))
                .min_by(|a, b| a.1.total_cmp(&b.1));
            match best {
                Some((i, d)) if d < 8.0 => gr.node = Some(i),
                _ => gr.reason = "access road removed by the graph cleanup",
            }
        }
    }
    // Highway carriageways: attachments per (highway, side).
    let mut attach: BTreeMap<(usize, i8), Vec<Attach>> = BTreeMap::new();
    let key = |h: usize, side: f32| (h, if side > 0.0 { 1i8 } else { -1i8 });
    let mut ramp_nodes: Vec<(usize, usize)> = Vec::new();
    let mut spur_nodes: BTreeMap<(usize, i8, bool), usize> = BTreeMap::new();
    let mut end_node = |nodes: &mut Nodes,
                        e: RampEnd,
                        pt: V3,
                        attach: &mut BTreeMap<(usize, i8), Vec<Attach>>|
     -> usize {
        match e {
            RampEnd::Gore { hw, side, s } => {
                let n = nodes.add(geom::xy(pt), pt[2], Some(ControlHint::Freeway));
                let s = hws[hw].wrap(s);
                attach
                    .entry(key(hw, side))
                    .or_default()
                    .push(Attach { s, node: n });
                n
            }
            RampEnd::Spur { hw, side, at_start } => {
                let k = (hw, if side > 0.0 { 1i8 } else { -1i8 }, at_start);
                if let Some(n) = spur_nodes.get(&k) {
                    return *n;
                }
                let h = &hws[hw];
                let s = if at_start { 0.0 } else { h.total() };
                let p = h.offset_pt(s, side * h.offset);
                let n = nodes.add(geom::xy(p), p[2], Some(ControlHint::Freeway));
                spur_nodes.insert(k, n);
                let s_att = if at_start { 0.0 } else { h.total() };
                attach
                    .entry(key(hw, side))
                    .or_default()
                    .push(Attach { s: s_att, node: n });
                n
            }
            RampEnd::Terminal(ti) => term_node[ti].unwrap_or(usize::MAX),
        }
    };
    for (ri, r) in ics.ramps.iter().enumerate() {
        let rp = &ramps[ri];
        let a = end_node(&mut nodes, r.from, rp.pts[0], &mut attach);
        let b = end_node(&mut nodes, r.to, rp.pts[rp.pts.len() - 1], &mut attach);
        ramp_nodes.push((a, b));
    }
    // Map-edge / gate ends of open highways.
    let mut stub_specs: Vec<(RoadSpec, String, usize)> = Vec::new();
    for (h, hw) in hws.iter().enumerate() {
        if hw.closed {
            continue;
        }
        for (at_start, s) in [(true, 0.0), (false, hw.total())] {
            for side in [1.0f32, -1.0] {
                let k = (h, if side > 0.0 { 1i8 } else { -1i8 }, at_start);
                if spur_nodes.contains_key(&k) {
                    continue;
                }
                let gate = if at_start { hw.start_gate } else { hw.end_gate };
                if gate.is_some() {
                    continue; // handled below
                }
                let p = hw.offset_pt(s, side * hw.offset);
                let n = nodes.add(geom::xy(p), p[2], None);
                attach
                    .entry(key(h, side))
                    .or_default()
                    .push(Attach { s, node: n });
            }
        }
        if let Some(gp) = hw.end_gate {
            // Fan both carriageways into a junction 60 m before the path end, then a
            // two-way stub to the gate.
            let s_end = hw.total();
            let pe = hw.offset_pt(s_end, 0.0);
            let nj = nodes.add(geom::xy(pe), pe[2], Some(ControlHint::Priority));
            for side in [1.0f32, -1.0] {
                attach
                    .entry(key(h, side))
                    .or_default()
                    .push(Attach { s: s_end, node: nj });
            }
            let ng = nodes.add(gp, ground.z(gp).max(pe[2]), None);
            let (n_l, _) = hw.lanes_at(s_end);
            let st = Style {
                class: RoadClass::Arterial,
                fwd: n_l,
                bwd: n_l,
                speed: 13.4,
                parking: false,
                bus: false,
                grade: 0.08,
                bank_max: 0.0,
            };
            let mut r = RoadSpec::new(nj, ng, st.class, st.fwd, st.bwd);
            r.speed_mps = st.speed;
            stub_specs.push((r, format!("{} terminal", hw.name), h));
            if let Some(gr) = gate_reach.iter_mut().find(|gr| {
                plan.site(gr.site)
                    .is_some_and(|s| geom::dist(s.gates[gr.gate].pos, gp) < 1.0)
            }) {
                gr.node = Some(ng);
                gr.reason = "";
            }
        }
    }
    // Carriageway pieces.
    let mut hw_spec_ranges: Vec<Vec<(usize, f32, f32, f32)>> = vec![Vec::new(); hws.len()];
    for ((h, sk), mut list) in attach {
        let hw = &hws[h];
        let side = sk as f32;
        let total = hw.total();
        // Travel order.
        list.sort_by(|a, b| {
            let (x, y) = if side > 0.0 { (a.s, b.s) } else { (-a.s, -b.s) };
            x.total_cmp(&y).then(a.node.cmp(&b.node))
        });
        let m = list.len();
        let pieces: Vec<(Attach, Attach)> = if hw.closed {
            (0..m).map(|k| (list[k], list[(k + 1) % m])).collect()
        } else {
            (0..m.saturating_sub(1))
                .map(|k| (list[k], list[k + 1]))
                .collect()
        };
        for (a, b) in pieces {
            let (s0, mut s1) = (a.s, b.s);
            if hw.closed {
                if side > 0.0 && s1 <= s0 {
                    s1 += total;
                }
                if side < 0.0 && s1 >= s0 {
                    s1 -= total;
                }
            }
            let len = (s1 - s0).abs();
            if len < 1.0 || (hw.closed && len > total - 1.0) {
                notes.push(format!("{}: zero-length carriageway piece", hw.name));
                continue;
            }
            let gate_end = hw.end_gate.is_some()
                && (b.node == a.node
                    || nodes.specs[b.node].control == Some(ControlHint::Priority)
                    || nodes.specs[a.node].control == Some(ControlHint::Priority));
            let lat = |s: f32| -> f32 {
                if !gate_end {
                    return hw.offset;
                }
                // Fan into the end junction over the last 25 m: the junction setback then
                // ends both carriageways' lanes side by side.
                let d = (total - s).max(0.0);
                hw.offset * (d / 25.0).min(1.0)
            };
            let step = if gate_end { 5.0 } else { 10.0 };
            let samples = highways::carriageway_samples(hw, side, s0, s1, &lat, step);
            let mut pts: Vec<V3> = samples.iter().map(|x| x.1).collect();
            let bank: Vec<f32> = samples.iter().map(|x| x.2).collect();
            pts[0] = [
                nodes.specs[a.node].pos[0],
                nodes.specs[a.node].pos[1],
                pts[0][2],
            ];
            let last = pts.len() - 1;
            pts[last] = [
                nodes.specs[b.node].pos[0],
                nodes.specs[b.node].pos[1],
                pts[last][2],
            ];
            let mid = 0.5 * (s0 + s1);
            let (n_l, v) = hw.lanes_at(mid);
            let st = Style {
                fwd: n_l,
                bwd: 0,
                speed: v,
                ..style::HIGHWAY
            };
            let mut r = spec_from_pts(a.node, b.node, &pts, bank, &st);
            r.shoulder_left_m = (hw.offset - n_l as f32 * highways::HW_LANE_M * 0.5).max(0.3);
            r.shoulder_right_m = highways::OUTER_SHOULDER_M;
            r.median_barrier = side > 0.0 && !gate_end;
            hw_spec_ranges[h].push((specs.len(), s0, s1, side));
            let mut covered = Vec::new();
            for t in &hw.tunnels {
                // Covered where the carriageway is ≥ 7.3 m below the natural ground (found
                // in carve; recorded here as the planned span).
                let _ = t;
            }
            for (k, w) in samples.windows(2).enumerate() {
                let _ = (k, w);
            }
            let cum = {
                let p2: Vec<V2> = pts.iter().map(|p| geom::xy(*p)).collect();
                cum2(&p2)
            };
            let mut in_cov: Option<f32> = None;
            for (k, smp) in samples.iter().enumerate() {
                let p = smp.1;
                let near_tunnel = hw.tunnels.iter().any(|t| {
                    let ext = Span {
                        s0: t.s0 - 200.0,
                        s1: t.s1 + 200.0,
                        ..*t
                    };
                    hw.in_spans(&[ext], smp.0).is_some()
                });
                let cov = near_tunnel
                    && (ground.z(geom::xy(p)) - p[2] > 2.0 || p[2] < plan::WATER_Z + 1.3);
                match (cov, in_cov) {
                    (true, None) => in_cov = Some(cum[k]),
                    (false, Some(a0)) => {
                        covered.push((a0, cum[k]));
                        in_cov = None;
                    }
                    _ => {}
                }
            }
            if let Some(a0) = in_cov {
                covered.push((a0, cum[cum.len() - 1]));
            }
            specs.push(r);
            names.push(hw.name.to_string());
            info.push(RoadInfo {
                kind: RoadKind::Carriageway,
                grade: 0.06,
                owner: Some(h),
                is_loop: false,
                covered,
            });
        }
    }
    // Ramps.
    let mut ic_ramps: Vec<Vec<usize>> = vec![Vec::new(); ic_list.len()];
    for (ri, r) in ics.ramps.iter().enumerate() {
        let (a, b) = ramp_nodes[ri];
        if a == usize::MAX || b == usize::MAX {
            notes.push(format!("{}: ramp end not found", r.name));
            continue;
        }
        let rp = &ramps[ri];
        let mut pts = rp.pts.clone();
        pts[0] = [nodes.specs[a].pos[0], nodes.specs[a].pos[1], pts[0][2]];
        let last = pts.len() - 1;
        pts[last] = [nodes.specs[b].pos[0], nodes.specs[b].pos[1], pts[last][2]];
        let mut spec = spec_from_pts(a, b, &pts, rp.bank.clone(), &r.st);
        spec.shoulder_left_m = 0.6;
        spec.shoulder_right_m = 1.5;
        ic_ramps[r.ic].push(specs.len());
        specs.push(spec);
        names.push(r.name.clone());
        info.push(RoadInfo {
            kind: RoadKind::Ramp,
            grade: r.st.grade,
            owner: Some(r.ic),
            is_loop: r.is_loop,
            covered: Vec::new(),
        });
    }
    for (r, name, h) in stub_specs {
        specs.push(r);
        names.push(name);
        info.push(RoadInfo {
            kind: RoadKind::Stub,
            grade: 0.08,
            owner: Some(h),
            is_loop: false,
            covered: Vec::new(),
        });
    }
    let opt = BuildOptions::default();
    let built = builder::build(&nodes.specs, &specs, &opt);
    // Grade separations: every plan-view crossing of two surfaces without a shared node.
    let crossings = find_crossings(&built, &specs);
    // Carve and structures.
    let natural = ground.hf.clone();
    carve::carve(ground, &built, &nodes.specs, &info);
    let features = structures::features(
        plan, &ground.hf, &natural, &built, &specs, &info, &names, hws, &crossings, &mut notes,
    );
    let hw_km: f32 = hws.iter().map(|h| h.total()).sum::<f32>() / 1000.0;
    notes.push(format!(
        "roads: {} street edges, {} carriageway pieces, {} ramps, {:.1} km of highway centreline, {} grade separations",
        g.edges.len(),
        info.iter().filter(|i| i.kind == RoadKind::Carriageway).count(),
        info.iter().filter(|i| i.kind == RoadKind::Ramp).count(),
        hw_km,
        crossings.len()
    ));
    let unreached: Vec<String> = gate_reach
        .iter()
        .filter(|g| g.node.is_none())
        .map(|g| format!("{}#{} ({})", g.site, g.gate, g.reason))
        .collect();
    if !unreached.is_empty() {
        notes.push(format!(
            "roads: gates not reached: {}",
            unreached.join(", ")
        ));
    }
    if sp.infeasible > 0 {
        notes.push(format!(
            "roads: {} street profiles with infeasible bounds",
            sp.infeasible
        ));
    }
    let interchanges = ic_list
        .into_iter()
        .zip(ic_ramps)
        .map(|((n, k), r)| (n, k, r))
        .collect();
    RoadsOut {
        nodes: nodes.specs,
        specs,
        built,
        names,
        features,
        notes,
        report: RoadsReport {
            info,
            crossings,
            gates: gate_reach,
            highway_names: hws.iter().map(|h| h.name.to_string()).collect(),
            interchanges,
        },
    }
}

/// Every plan-view crossing of two road surfaces that do not share a node (grade
/// separations), with the achieved surface height difference.
fn find_crossings(built: &Built, specs: &[RoadSpec]) -> Vec<Crossing> {
    let mut grid = SegGrid::new(plan::MAP_LO, plan::MAP_HI, 80.0);
    let paths: Vec<Vec<V2>> = built
        .surfaces
        .iter()
        .map(|s| s.centre.iter().map(|p| geom::xy(*p)).collect())
        .collect();
    for (i, p) in paths.iter().enumerate() {
        grid.insert_line(i as u32, p, 0.0);
    }
    let mut out = Vec::new();
    for (i, s) in built.surfaces.iter().enumerate() {
        let p = &paths[i];
        for k in 0..p.len().saturating_sub(1) {
            let (a, b) = (p[k], p[k + 1]);
            let lo = [a[0].min(b[0]), a[1].min(b[1])];
            let hi = [a[0].max(b[0]), a[1].max(b[1])];
            for (j, m) in grid.query(lo, hi) {
                let j = j as usize;
                if j <= i {
                    continue;
                }
                let (ri, rj) = (
                    &specs[s.road as usize],
                    &specs[built.surfaces[j].road as usize],
                );
                if ri.a == rj.a || ri.a == rj.b || ri.b == rj.a || ri.b == rj.b {
                    continue;
                }
                let q = &paths[j];
                let m = m as usize;
                if let Some((t, u)) = geo::seg_cross(a, b, q[m], q[m + 1]) {
                    let zi = s.centre[k][2] + (s.centre[k + 1][2] - s.centre[k][2]) * t;
                    let o = &built.surfaces[j];
                    let zj = o.centre[m][2] + (o.centre[m + 1][2] - o.centre[m][2]) * u;
                    let pos = geom::add(a, geom::scale(geom::sub(b, a), t));
                    let (upper, lower) = if zi >= zj { (i, j) } else { (j, i) };
                    out.push(Crossing {
                        upper: built.surfaces[upper].road as usize,
                        lower: built.surfaces[lower].road as usize,
                        pos,
                        clearance: (zi - zj).abs(),
                    });
                }
            }
        }
    }
    out
}

/// The ground exactly as `metro::generate_with_report` prepares it for [`build`] (natural
/// ground, every module's earthworks, site pads), for tests and diagnostics that need the
/// road generator's own output ([`RoadsOut`]) rather than the final network.
pub fn prepared_ground(plan: &Plan, cfg: &MetroConfig) -> Ground {
    use super::{airport, buildings, construction, docks, ground, parking, rail, scenery};
    let mut rng = wp_math::Rng::new(cfg.seed);
    let nat = ground::Natural::new(plan, &mut rng);
    let mut works = Vec::new();
    works.extend(airport::earthworks(plan, &nat));
    works.extend(docks::earthworks(plan, &nat));
    works.extend(rail::earthworks(plan, &nat));
    works.extend(parking::earthworks(plan, &nat));
    works.extend(construction::earthworks(plan, &nat));
    works.extend(buildings::earthworks(plan, &nat));
    works.extend(scenery::earthworks(plan, &nat));
    for s in &plan.sites {
        if let Some(z) = s.pad_z {
            works.push(ground::Earthwork::Pad {
                polygon: s.polygon.clone(),
                z,
                blend_m: 60.0,
            });
        }
    }
    Ground::new(&nat, cfg.terrain_cell_m, &works)
}

/// Run the road generator on its own: (plan, carved ground, output).
pub fn build_standalone(cfg: &MetroConfig) -> (Plan, Ground, RoadsOut) {
    let plan = Plan::new();
    let mut g = prepared_ground(&plan, cfg);
    let out = build(&plan, &mut g, cfg);
    (plan, g, out)
}
