//! Ashpeak City: the metropolitan downtown north of the range, its landmarks and construction
//! sites.
//!
//! - **Downtown core** ([`plan::METRO_CORE_LO`]..[`plan::METRO_CORE_HI`]): a 100 m street grid
//!   ([`XS`] × [`YS`]) of 2 + 2 avenues, two 3 + 3 grand avenues with bus lanes (Meridian
//!   Avenue, Grand Concourse), a one-way avenue pair (Bastion northbound, Corwin southbound, two lanes each)
//!   and a one-way street pair (3rd Street eastbound, 4th Street westbound); every other
//!   street is a collector, so every core junction is signalised. **Meridian Plaza** (two by
//!   two blocks) holds the **Meridian Spire** (≈ 385 m) and billboards; City Hall faces it,
//!   **Ashpeak Central Station** stands on Southgate Boulevard. Hand-placed towers fill the
//!   blocks, tallest near the Spire.
//! - **Midtown**: the same grid continues east to Midtown Avenue (Midtown and Northgate
//!   interchanges on Route 7) with the **Ashpeak Coliseum** and its lot, and west to Granite
//!   Street; **Westbrook** (houses) beyond it reaches the Ashpeak West interchange.
//! - **Northgate Heights**: a warped residential grid with courts north of Union Street, and
//!   **Northline Parkway** from Route 12's end to Route 7's end and on to the waterfront.
//! - **Waterfront** east of Route 7: a grid down to **Seawall Boulevard** along the coast,
//!   docks at its south end with the Seawall Point lighthouse, Beacon Point Tower.
//! - Construction sites ([`SITES`]): partial towers, cranes, hoardings, jersey barriers along
//!   the kerb.

use super::super::*;
use super::plan;

// ---- Styles -------------------------------------------------------------------------------

/// Downtown 2 + 2 avenue (35 mph).
const AVENUE: Style = BLVD;
/// 3 + 3 grand avenue (35 mph); `profile_roads` gives 3 + 3 arterials a bus lane each way.
const GRAND: Style = Style {
    fwd: 3,
    bwd: 3,
    ..BLVD
};
/// One-way two-lane avenue with kerbside parking (35 mph). Two lanes, not three: the lane
/// graph has no lane changes, and a middle lane of three is only reachable straight on.
const ONE_WAY: Style = Style {
    fwd: 2,
    bwd: 0,
    parking: true,
    ..BLVD
};
/// Downtown cross street: 1 + 1 with parking, collector class (signalised), 25 mph.
const STREET: Style = Style {
    speed: 11.176,
    ..COLLECTOR
};
/// Northline Parkway (45 mph, 2 + 2).
const PARKWAY: Style = ROUTE;
/// Seawall Boulevard (40 mph, 2 + 2, banked curves).
const SEAWALL: Style = ARTERIAL;

// ---- Downtown / midtown grid -------------------------------------------------------------

/// Grid columns (avenues and streets running north–south), x in m: Westbrook, Granite
/// District, the core (100 m blocks), midtown east.
pub(super) const XS: [f32; 21] = [
    -1880.0, -1760.0, -1630.0, -1500.0, -1400.0, -1300.0, -1200.0, -1100.0, -1000.0, -900.0,
    -800.0, -700.0, -600.0, -500.0, -400.0, -300.0, -200.0, -100.0, 20.0, 140.0, 250.0,
];
/// Grid rows (streets running east–west), y in m.
pub(super) const YS: [f32; 10] = [
    4200.0, 4300.0, 4400.0, 4500.0, 4600.0, 4700.0, 4800.0, 4900.0, 5000.0, 5100.0,
];
/// Midtown Avenue gets an extra node at this y for the Midtown Interchange link.
const MIDTOWN_Y: f32 = 4450.0;

/// Meridian Plaza (centre-line rectangle `[x0, y0, x1, y1]`): its middle node is left out.
pub(super) const PLAZA: [f32; 4] = [-800.0, 4600.0, -600.0, 4800.0];
/// The Meridian Spire stands in the middle of the plaza.
pub(super) const SPIRE: V2 = [-700.0, 4700.0];
/// City Hall's block (west of the plaza).
const CITY_HALL: [f32; 4] = [-900.0, 4700.0, -800.0, 4800.0];
/// Ashpeak Central Station's superblock (the street between is left out).
const STATION: [f32; 4] = [-500.0, 4200.0, -300.0, 4300.0];
/// Ashpeak Coliseum's superblock (stadium north, parking lot south).
pub(super) const STADIUM: [f32; 4] = [20.0, 4600.0, 250.0, 4900.0];
/// Construction sites: block rectangle and the face whose kerb gets jersey barriers
/// (0 south, 1 east, 2 north, 3 west).
pub(super) const SITES: [([f32; 4], usize); 5] = [
    ([-1200.0, 4800.0, -1100.0, 4900.0], 0),
    ([-500.0, 4900.0, -400.0, 5000.0], 2),
    ([-300.0, 4400.0, -200.0, 4500.0], 3),
    ([20.0, 4300.0, 140.0, 4400.0], 0),
    ([-1500.0, 4900.0, -1400.0, 5000.0], 1),
];

/// Lane direction along increasing grid index.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Flow {
    Both,
    /// One-way towards increasing x / y.
    Up,
    /// One-way towards decreasing x / y.
    Down,
}

/// Style, name and flow of the grid column at `x`.
fn column(x: f32) -> (Style, &'static str, Flow) {
    match x as i32 {
        -1880 => (AVENUE, "Westbrook Avenue", Flow::Both),
        -1760 => (LOCAL, "Fallow Street", Flow::Both),
        -1630 => (LOCAL, "Copper Street", Flow::Both),
        -1500 => (AVENUE, "Granite Street", Flow::Both),
        -1400 => (STREET, "Quarry Street", Flow::Both),
        -1300 => (AVENUE, "Tamarack Avenue", Flow::Both),
        -1200 => (STREET, "Alder Street", Flow::Both),
        -1100 => (ONE_WAY, "Bastion Avenue", Flow::Up),
        -1000 => (ONE_WAY, "Corwin Avenue", Flow::Down),
        -900 => (STREET, "Delmar Street", Flow::Both),
        -800 => (STREET, "Ember Street", Flow::Both),
        -700 => (GRAND, "Meridian Avenue", Flow::Both),
        -600 => (AVENUE, "Crowfoot Avenue", Flow::Both),
        -500 => (STREET, "Foundry Street", Flow::Both),
        -400 => (STREET, "Gallery Street", Flow::Both),
        -300 => (STREET, "Halsey Street", Flow::Both),
        -200 => (STREET, "Ironwood Street", Flow::Both),
        -100 => (AVENUE, "Juniper Avenue", Flow::Both),
        20 => (STREET, "Kestrel Street", Flow::Both),
        140 => (STREET, "Lockhart Street", Flow::Both),
        _ => (AVENUE, "Midtown Avenue", Flow::Both),
    }
}

/// Style, name and flow of the grid row at `y`.
fn row(y: f32) -> (Style, &'static str, Flow) {
    match y as i32 {
        4200 => (AVENUE, "Southgate Boulevard", Flow::Both),
        4300 => (STREET, "2nd Street", Flow::Both),
        4400 => (ONE_WAY, "3rd Street", Flow::Up),
        4500 => (ONE_WAY, "4th Street", Flow::Down),
        4600 => (STREET, "5th Street", Flow::Both),
        4700 => (GRAND, "Grand Concourse", Flow::Both),
        4800 => (STREET, "7th Street", Flow::Both),
        4900 => (STREET, "8th Street", Flow::Both),
        5000 => (STREET, "9th Street", Flow::Both),
        _ => (AVENUE, "Union Street", Flow::Both),
    }
}

/// Westbrook (the residential west end of the grid): x below this, m.
const WESTBROOK_X: f32 = -1560.0;

/// Whether grid node `(x, y)` is left out (plaza, stadium).
fn removed(x: f32, y: f32) -> bool {
    let at = |p: V2| (p[0] - x).abs() < 1.0 && (p[1] - y).abs() < 1.0;
    at(SPIRE) || at([140.0, 4700.0]) || at([140.0, 4800.0])
}

// ---- Northgate Heights, Northline Parkway --------------------------------------------------

/// Northgate Heights columns (all are grid columns, so the links meet Union Street nodes).
const NORTH_XS: [f32; 11] = [
    -1880.0, -1630.0, -1400.0, -1200.0, -1000.0, -800.0, -600.0, -400.0, -200.0, 20.0, 250.0,
];
/// Northgate Heights rows, m.
const NORTH_YS: [f32; 2] = [5250.0, 5400.0];
/// Northline Parkway's y, m.
const PARKWAY_Y: f32 = 5500.0;

// ---- Waterfront ---------------------------------------------------------------------------

/// Waterfront grid columns east of Route 7, m.
const WXS: [f32; 4] = [900.0, 1060.0, 1220.0, 1370.0];
/// Waterfront grid rows, m (Pier Street at the Midtown Interchange, Northgate Road at the
/// Northgate Interchange).
const WYS: [f32; 8] = [
    3900.0, 4100.0, 4300.0, 4450.0, 4650.0, 4850.0, 5100.0, 5300.0,
];
/// Seawall Boulevard's south end (a lookout with the lighthouse).
const SEAWALL_POINT: V2 = [1470.0, 3770.0];

/// Seawall Boulevard's x at `y`.
fn seawall_x(y: f32) -> f32 {
    let (s, _) = wp_math::sin_cos(y / 320.0);
    1525.0 + 15.0 * s
}

fn waterfront_row(y: f32) -> (Style, &'static str) {
    match y as i32 {
        3900 => (PORT, "Dock Street"),
        4100 => (PORT, "Cannery Row"),
        4300 => (STREET, "Tidewater Street"),
        4450 => (AVENUE, "Pier Street"),
        4650 => (STREET, "Marina Street"),
        4850 => (STREET, "Beacon Street"),
        5100 => (AVENUE, "Northgate Road"),
        _ => (STREET, "Gull Street"),
    }
}

fn waterfront_column(x: f32) -> &'static str {
    match x as i32 {
        900 => "Saltmarsh Avenue",
        1060 => "Wharf Street",
        1220 => "Anchor Street",
        _ => "Ferry Street",
    }
}

/// Plan position of interchange `name`'s terminal assigned to this district.
fn terminal(name: &str, a_side: bool) -> V2 {
    let ic = plan::INTERCHANGES
        .iter()
        .find(|ic| ic.name == name)
        .expect("planned interchange");
    if a_side {
        debug_assert_eq!(ic.a_by, "metro");
        ic.a
    } else {
        debug_assert_eq!(ic.b_by, "metro");
        ic.b
    }
}

/// Node position where freeway `name` ends.
fn freeway_end(name: &str) -> V2 {
    plan::FREEWAY_ENDS
        .iter()
        .find(|e| e.0 == name)
        .expect("planned freeway end")
        .1
}

/// Lay out this district's roads.
#[allow(clippy::needless_range_loop)]
pub(super) fn layout(g: &mut Gen) {
    // ---- Downtown + midtown grid -----------------------------------------------------
    let (nx, ny) = (XS.len(), YS.len());
    let mut ids: Vec<Vec<Option<usize>>> = vec![vec![None; nx]; ny];
    for (j, y) in YS.iter().enumerate() {
        for (i, x) in XS.iter().enumerate() {
            if !removed(*x, *y) {
                ids[j][i] = Some(g.node([*x, *y]));
            }
        }
    }
    // Rows (east–west).
    for j in 0..ny {
        let (st, name, flow) = row(YS[j]);
        for i in 0..nx - 1 {
            let (Some(a), Some(b)) = (ids[j][i], ids[j][i + 1]) else {
                continue;
            };
            let st = if XS[i + 1] < WESTBROOK_X && st.class == RoadClass::Collector {
                LOCAL
            } else {
                st
            };
            let (a, b) = if flow == Flow::Down { (b, a) } else { (a, b) };
            g.road(a, b, Vec::new(), st, name);
        }
    }
    // Columns (north–south).
    let station_col = XS
        .iter()
        .position(|x| *x == -400.0)
        .expect("Gallery Street");
    let mut midtown_node = None;
    for i in 0..nx {
        let (st, name, flow) = column(XS[i]);
        for j in 0..ny - 1 {
            if i == station_col && j == 0 {
                continue; // Central Station's superblock
            }
            let (Some(a), Some(b)) = (ids[j][i], ids[j + 1][i]) else {
                continue;
            };
            let (a, b) = if flow == Flow::Down { (b, a) } else { (a, b) };
            if i == nx - 1 && YS[j] < MIDTOWN_Y && YS[j + 1] > MIDTOWN_Y {
                let m = g.node([XS[i], MIDTOWN_Y]);
                g.road(a, m, Vec::new(), st, name);
                g.road(m, b, Vec::new(), st, name);
                midtown_node = Some(m);
            } else {
                g.road(a, b, Vec::new(), st, name);
            }
        }
    }
    let grid = |g: &mut Gen, x: f32, y: f32| g.node([x, y]);

    // ---- Interchange and freeway-end connections --------------------------------------
    let crowfoot = g.node(terminal("Crowfoot Interchange", false));
    let n = grid(g, -600.0, YS[0]);
    g.road(crowfoot, n, Vec::new(), AVENUE, "Crowfoot Avenue");
    let granite = g.node(terminal("Granite Street", false));
    let n = grid(g, -1500.0, YS[0]);
    g.road(granite, n, Vec::new(), AVENUE, "Granite Street");
    let west = g.node(terminal("Ashpeak West", false));
    let n = grid(g, XS[0], 4500.0);
    g.road(west, n, Vec::new(), AVENUE, "Ashpeak West");
    let mid_a = g.node(terminal("Midtown Interchange", true));
    let m = midtown_node.expect("Midtown Avenue's interchange node");
    g.road(m, mid_a, Vec::new(), AVENUE, "Midtown Avenue");
    let north_a = g.node(terminal("Northgate Interchange", true));
    let n = grid(g, XS[nx - 1], YS[ny - 1]);
    g.road(n, north_a, Vec::new(), AVENUE, "Union Street");

    // ---- Coliseum parking lot ------------------------------------------------------------
    let lot_ctrl = [
        [140.0, 4640.0],
        [215.0, 4640.0],
        [215.0, 4700.0],
        [65.0, 4700.0],
        [65.0, 4640.0],
    ];
    let lot = fillet(&lot_ctrl, &[0.0, 12.0, 12.0, 12.0, 12.0], true, 4.0);
    let lcum = {
        let mut c = lot.clone();
        c.push(lot[0]);
        cum2(&c)
    };
    let mut closed = lot.clone();
    closed.push(lot[0]);
    let far = project2(&closed, &lcum, [140.0, 4700.0]).0;
    let (lot_nodes, _) = g.ring(&lot, &[0.0, far], SERVICE, "Coliseum Lot");
    let gate = grid(g, 140.0, 4600.0);
    g.road(gate, lot_nodes[0], Vec::new(), SERVICE, "Coliseum Lot");

    // ---- Northgate Heights ---------------------------------------------------------------
    let north = g.warped_grid(
        &NORTH_XS,
        &NORTH_YS,
        16.0,
        14.0,
        0.1,
        &|row, k| {
            if !row && (k == 4 || k == 8) {
                COLLECTOR
            } else {
                LOCAL
            }
        },
        "Northgate Heights",
    );
    for (i, x) in NORTH_XS.iter().enumerate() {
        let below = grid(g, *x, YS[ny - 1]);
        // Collectors: the Union Street junctions downtown stay signalised.
        let bulge = g.rng.range_f32(-6.0, 6.0);
        g.link(below, north[0][i], bulge, COLLECTOR, "Northgate Heights");
    }

    // ---- Northline Parkway ---------------------------------------------------------------
    let r12 = g.node(freeway_end("Route 12"));
    let r7 = g.node(freeway_end("Route 7"));
    let mut stops = vec![r12];
    for x in NORTH_XS {
        stops.push(g.node([x, PARKWAY_Y]));
    }
    stops.push(r7);
    for x in WXS {
        stops.push(g.node([x, PARKWAY_Y]));
    }
    for w in stops.windows(2) {
        g.road(w[0], w[1], Vec::new(), PARKWAY, "Northline Parkway");
    }
    for (i, x) in NORTH_XS.iter().enumerate() {
        let above = g.node([*x, PARKWAY_Y]);
        let st = if i == 4 || i == 8 { COLLECTOR } else { LOCAL };
        let bulge = g.rng.range_f32(-6.0, 6.0);
        g.link(north[1][i], above, bulge, st, "Northgate Heights");
    }

    // ---- Waterfront ------------------------------------------------------------------------
    let mut w: Vec<Vec<usize>> = Vec::new();
    for y in WYS {
        w.push(WXS.iter().map(|x| g.node([*x, y])).collect());
    }
    let sea: Vec<usize> = WYS.iter().map(|y| g.node([seawall_x(*y), *y])).collect();
    for (j, y) in WYS.iter().enumerate() {
        let (st, name) = waterfront_row(*y);
        for i in 0..WXS.len() - 1 {
            g.road(w[j][i], w[j][i + 1], Vec::new(), st, name);
        }
        g.road(w[j][WXS.len() - 1], sea[j], Vec::new(), st, name);
    }
    for (i, x) in WXS.iter().enumerate() {
        let name = waterfront_column(*x);
        let st = if i == 0 { AVENUE } else { STREET };
        for j in 0..WYS.len() - 1 {
            g.road(w[j][i], w[j + 1][i], Vec::new(), st, name);
        }
        let top = g.node([*x, PARKWAY_Y]);
        g.road(w[WYS.len() - 1][i], top, Vec::new(), st, name);
    }
    // Seawall Boulevard: from the point north along the coast, then round to the parkway.
    let point = g.node(SEAWALL_POINT);
    g.road(point, sea[0], Vec::new(), SEAWALL, "Seawall Boulevard");
    for k in 0..sea.len() - 1 {
        g.road(sea[k], sea[k + 1], Vec::new(), SEAWALL, "Seawall Boulevard");
    }
    let last = sea[sea.len() - 1];
    let end = g.node([WXS[WXS.len() - 1], PARKWAY_Y]);
    let curve = catmull_rom(
        &[
            g.pos(last),
            [seawall_x(5300.0) - 5.0, 5400.0],
            [1440.0, 5480.0],
            g.pos(end),
        ],
        6.0,
    );
    g.road(last, end, curve, SEAWALL, "Seawall Boulevard");
    let pier = g.node(terminal("Midtown Interchange", false));
    let wi = WYS.iter().position(|y| *y == 4450.0).expect("Pier Street");
    g.road(pier, w[wi][0], Vec::new(), AVENUE, "Pier Street");
    let ng = g.node(terminal("Northgate Interchange", false));
    let wi = WYS
        .iter()
        .position(|y| *y == 5100.0)
        .expect("Northgate Road");
    g.road(ng, w[wi][0], Vec::new(), AVENUE, "Northgate Road");

    // ---- Courts (last: every road above keeps its shape) ----------------------------------
    g.courts(&north, "Northgate Court");
}

/// Covered roads (tunnels) on this district's roads.
pub(super) fn covers() -> Vec<CoverSpec> {
    Vec::new()
}

/// Traffic density multiplier over the default for each district kind in Ashpeak City.
pub(super) const CORE_DENSITY_MUL: f32 = 1.8;

/// This district's zones.
pub(super) fn zones() -> Vec<Zone> {
    let z = |kind: DistrictKind, mul: f32, poly: Vec<V2>| Zone {
        kind,
        density_base: default_density_base(kind) * mul,
        polygon: poly,
    };
    let [cx0, cy0] = plan::METRO_CORE_LO;
    let [cx1, cy1] = plan::METRO_CORE_HI;
    vec![
        // Downtown: the core grid between Tamarack and Juniper avenues.
        z(
            DistrictKind::Core,
            CORE_DENSITY_MUL,
            rect(cx0 - 50.0, cy0, cx1 + 50.0, cy1),
        ),
        // Midtown east (Coliseum) and the Granite District.
        z(
            DistrictKind::Commercial,
            1.4,
            rect(cx1 + 50.0, cy0, 430.0, cy1),
        ),
        z(
            DistrictKind::Commercial,
            1.4,
            rect(WESTBROOK_X, cy0, cx0 - 50.0, cy1),
        ),
        // Waterfront: shops and offices along the middle, lofts north, docks south.
        z(
            DistrictKind::Commercial,
            1.2,
            rect(820.0, 4200.0, 1650.0, 5000.0),
        ),
        z(DistrictKind::Port, 1.0, rect(820.0, 3650.0, 1650.0, 4200.0)),
        z(
            DistrictKind::Residential,
            1.0,
            rect(820.0, 5000.0, 1650.0, 5600.0),
        ),
        // Westbrook and Northgate Heights.
        z(
            DistrictKind::Residential,
            1.0,
            rect(-1960.0, cy0, WESTBROOK_X, cy1),
        ),
        z(
            DistrictKind::Residential,
            1.0,
            rect(-1960.0, cy1, 450.0, 5600.0),
        ),
    ]
}

/// Whether the generic street-front building pass may build at `p`: not inside the
/// Coliseum's superblock (the stadium and its lot), nor along the links to the interchange
/// terminals (beside the ramps), nor near a freeway in Ashpeak City.
pub(super) fn generic_buildings_at(p: V2) -> bool {
    let [x0, y0, x1, y1] = STADIUM;
    let stadium = p[0] > x0 + 5.0 && p[0] < x1 - 5.0 && p[1] > y0 + 5.0 && p[1] < y1 - 5.0;
    let ramps = plan::INTERCHANGES
        .iter()
        .flat_map(|ic| [(ic.a, ic.a_by), (ic.b, ic.b_by)])
        .any(|(t, by)| by == "metro" && geom::dist(p, t) < TERMINAL_CLEAR_M);
    let freeway = in_metro(p, 0.0)
        && [plan::ROUTE_7, plan::ROUTE_12, plan::CROSSTOWN]
            .iter()
            .any(|line| {
                line.windows(2).any(|w| {
                    geom::point_segment_distance(p, w[0], w[1]) < plan::FREEWAY_CLEAR_M + 30.0
                })
            });
    !stadium && !ramps && !freeway
}

/// No street-front buildings this close to an interchange terminal, m.
const TERMINAL_CLEAR_M: f32 = 110.0;

// ---- Buildings ----------------------------------------------------------------------------

/// Clearance every hand-placed building keeps from paved edges (sidewalk plus a metre), m.
const CLEAR_M: f32 = SIDEWALK_W_M + 1.0;

/// Junction discs (centre, radius + margin) of the nodes in Ashpeak City.
fn junction_discs(site: &Site) -> Vec<(V2, f32)> {
    let r = node_radii(site.built, site.nodes);
    site.nodes
        .iter()
        .zip(&r)
        .filter(|(n, r)| **r > 0.0 && in_metro(n.pos, 50.0))
        .map(|(n, r)| (n.pos, r + 2.0))
        .collect()
}

fn in_metro(p: V2, pad: f32) -> bool {
    p[0] >= plan::METRO_LO[0] - pad
        && p[0] <= plan::METRO_HI[0] + pad
        && p[1] >= plan::METRO_LO[1] - pad
        && p[1] <= plan::METRO_HI[1] + pad
}

/// Counter-clockwise rectangle footprint `[x0, y0, x1, y1]`.
fn fp_rect(r: [f32; 4]) -> Vec<V2> {
    rect(r[0], r[1], r[2], r[3])
}

fn shrink(r: [f32; 4], d: f32) -> [f32; 4] {
    [r[0] + d, r[1] + d, r[2] - d, r[3] - d]
}

/// Rectangle of half-sizes `hx`, `hy` around `c`.
fn around(c: V2, hx: f32, hy: f32) -> [f32; 4] {
    [c[0] - hx, c[1] - hy, c[0] + hx, c[1] + hy]
}

fn centre(r: [f32; 4]) -> V2 {
    [(r[0] + r[2]) * 0.5, (r[1] + r[3]) * 0.5]
}

/// Regular `n`-gon footprint (counter-clockwise) with radii `rx`, `ry`.
fn oval(c: V2, rx: f32, ry: f32, n: usize) -> Vec<V2> {
    (0..n)
        .map(|k| {
            let (s, co) = wp_math::sin_cos(std::f32::consts::TAU * k as f32 / n as f32);
            [c[0] + rx * co, c[1] + ry * s]
        })
        .collect()
}

/// Whether a footprint keeps [`CLEAR_M`] from every paved edge and junction disc (vertices,
/// edge midpoints and centroid probed).
fn fits(site: &Site, discs: &[(V2, f32)], fp: &[V2], clear: f32) -> bool {
    let n = fp.len();
    let mut probes: Vec<V2> = fp.to_vec();
    for k in 0..n {
        probes.push(geom::scale(geom::add(fp[k], fp[(k + 1) % n]), 0.5));
    }
    let c = geom::scale(
        fp.iter().fold([0.0, 0.0], |a, p| geom::add(a, *p)),
        1.0 / n as f32,
    );
    probes.push(c);
    probes
        .iter()
        .all(|q| site.clearance(*q) >= clear && discs.iter().all(|(d, r)| geom::dist(*q, *d) >= *r))
}

/// Rectangle `r` shrunk by 2 m steps (at most `tries` times) until it [`fits`].
fn fit_rect(site: &Site, discs: &[(V2, f32)], mut r: [f32; 4], tries: usize) -> Option<[f32; 4]> {
    for _ in 0..=tries {
        if r[2] - r[0] < 8.0 || r[3] - r[1] < 8.0 {
            return None;
        }
        if fits(site, discs, &fp_rect(r), CLEAR_M) {
            return Some(r);
        }
        r = shrink(r, 2.0);
    }
    None
}

/// Lowest and highest ground under a footprint.
fn ground_range(site: &Site, fp: &[V2]) -> (f32, f32) {
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for p in fp {
        let z = site.ground(*p);
        lo = lo.min(z);
        hi = hi.max(z);
    }
    (lo, hi)
}

/// A building on the ground: walls from just under the lowest ground to `height` above the
/// highest.
fn on_ground(site: &Site, fp: Vec<V2>, height: f32, kind: BuildingKind) -> Building {
    let (lo, hi) = ground_range(site, &fp);
    let base = lo - 0.3;
    Building {
        kind,
        ..Building::block(fp, height + (hi - base), base)
    }
}

/// A building standing on another's roof (`base` = that roof's height).
fn stacked(fp: Vec<V2>, height: f32, base: f32, kind: BuildingKind) -> Building {
    Building {
        kind,
        ..Building::block(fp, height, base)
    }
}

/// The buildable interior of the block around `c`: marching out from `c` along ±x and ±y
/// until [`CLEAR_M`] from the paving (at most `reach` m).
fn interior(site: &Site, c: V2, reach: f32) -> [f32; 4] {
    let march = |d: V2| {
        let mut t = 0.0;
        while t < reach {
            let q = geom::add(c, geom::scale(d, t + 1.0));
            if site.clearance(q) < CLEAR_M {
                break;
            }
            t += 1.0;
        }
        t
    };
    [
        c[0] - march([-1.0, 0.0]),
        c[1] - march([0.0, -1.0]),
        c[0] + march([1.0, 0.0]),
        c[1] + march([0.0, 1.0]),
    ]
}

/// A tower in rectangle `r` (shrunk until it fits, at most three times), `h` m tall: tall
/// ones step back twice. Returns whether it was placed.
fn tower(
    site: &mut Site,
    discs: &[(V2, f32)],
    mut r: [f32; 4],
    h: f32,
    kind: BuildingKind,
    out: &mut Vec<Building>,
) -> bool {
    for _ in 0..4 {
        if r[2] - r[0] < 12.0 || r[3] - r[1] < 12.0 {
            return false;
        }
        let fp = fp_rect(r);
        if fits(site, discs, &fp, CLEAR_M) {
            if h > 110.0 {
                let w = (r[2] - r[0]).min(r[3] - r[1]);
                let first = on_ground(site, fp, h * 0.55, kind);
                let z1 = first.roof_z();
                out.push(first);
                let r2 = shrink(r, w * 0.1);
                out.push(stacked(fp_rect(r2), h * 0.3, z1, kind));
                let r3 = shrink(r, w * 0.2);
                out.push(stacked(fp_rect(r3), h * 0.15, z1 + h * 0.3, kind));
            } else {
                out.push(on_ground(site, fp, h, kind));
            }
            return true;
        }
        r = shrink(r, 2.5);
    }
    false
}

/// Fill one block's interior `r` with towers between `hmin` and `hmax` m: one, two or four.
fn tower_block(
    site: &mut Site,
    discs: &[(V2, f32)],
    r: [f32; 4],
    hmin: f32,
    hmax: f32,
    out: &mut Vec<Building>,
) {
    let (w, d) = (r[2] - r[0], r[3] - r[1]);
    if w < 24.0 || d < 24.0 {
        return;
    }
    let roll = site.rng.next_f32();
    let gap = 4.0;
    let [x0, y0, x1, y1] = r;
    let (mx, my) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
    let cells: Vec<[f32; 4]> = if roll < 0.2 {
        vec![r]
    } else if roll < 0.5 {
        if w >= d {
            vec![[x0, y0, mx - gap, y1], [mx + gap, y0, x1, y1]]
        } else {
            vec![[x0, y0, x1, my - gap], [x0, my + gap, x1, y1]]
        }
    } else {
        vec![
            [x0, y0, mx - gap, my - gap],
            [mx + gap, y0, x1, my - gap],
            [x0, my + gap, mx - gap, y1],
            [mx + gap, my + gap, x1, y1],
        ]
    };
    for c in cells {
        let keep = site.rng.chance(0.92);
        let inset = site.rng.range_f32(0.0, 2.0);
        let h = site.rng.range_f32(hmin, hmax);
        if keep {
            tower(site, discs, shrink(c, inset), h, BuildingKind::Block, out);
        }
    }
}

/// Decor standing on the ground at `p`.
fn decor(site: &Site, kind: DecorKind, p: V2, yaw: f32, size: [f32; 3]) -> Decor {
    Decor {
        kind,
        pos: [p[0], p[1], site.ground(p)],
        yaw,
        size,
    }
}

/// Fence panels (and a gate gap on the `gate` side) along rectangle `r`.
fn fence(site: &Site, r: [f32; 4], gate: usize, out: &mut Vec<Decor>) {
    let corners = [[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]];
    for k in 0..4 {
        let (a, b) = (corners[k], corners[(k + 1) % 4]);
        let len = geom::dist(a, b);
        let dir = geom::normalize(geom::sub(b, a));
        let n = (len / 3.5).floor().max(1.0) as usize;
        let step = len / n as f32;
        for i in 0..n {
            let t = (i as f32 + 0.5) * step;
            if k == gate && (t - len * 0.5).abs() < 5.0 {
                continue;
            }
            let p = geom::add(a, geom::scale(dir, t));
            out.push(decor(
                site,
                DecorKind::SiteFence,
                p,
                geom::yaw_of(dir),
                [0.15, step - 0.1, 2.4],
            ));
        }
    }
}

/// A construction site in block interior `r`: hoarding, a foundation slab (keeps the street-
/// front pass off the lot), a partial tower or two, cranes, and jersey barriers on the
/// sidewalk along the kerb of face `kerb` (0 south, 1 east, 2 north, 3 west).
fn construction(
    site: &mut Site,
    discs: &[(V2, f32)],
    r: [f32; 4],
    kerb: usize,
    b: &mut Vec<Building>,
    d: &mut Vec<Decor>,
) {
    let f = shrink(r, 0.5);
    fence(site, f, (kerb + 2) % 4, d);
    b.push(on_ground(
        site,
        fp_rect(shrink(f, 0.4)),
        0.6,
        BuildingKind::Construction,
    ));
    let c = centre(f);
    let (hx, hy) = ((f[2] - f[0]) * 0.28, (f[3] - f[1]) * 0.28);
    let h = site.rng.range_f32(24.0, 70.0);
    let t = around(c, hx, hy);
    if tower(site, discs, t, h, BuildingKind::Construction, b) {
        // Crane beside the tower, its mast well above the frame.
        let mast = [t[2] + 6.0, t[1] + (t[3] - t[1]) * 0.3];
        let crane_h = h + site.rng.range_f32(30.0, 45.0);
        let yaw = site.rng.range_f32(0.0, std::f32::consts::TAU);
        d.push(decor(
            site,
            DecorKind::Crane,
            mast,
            yaw,
            [2.5, 2.5, crane_h],
        ));
        if f[2] - f[0] > 60.0 {
            let mast2 = [t[0] - 6.0, t[3] - (t[3] - t[1]) * 0.25];
            let yaw = site.rng.range_f32(0.0, std::f32::consts::TAU);
            d.push(decor(
                site,
                DecorKind::Crane,
                mast2,
                yaw,
                [2.5, 2.5, crane_h * 0.8],
            ));
        }
    }
    // Jersey barriers: on the sidewalk just behind the kerb of face `kerb`.
    let (a, e, out_dir): (V2, V2, V2) = match kerb {
        0 => ([f[0], f[1]], [f[2], f[1]], [0.0, -1.0]),
        1 => ([f[2], f[1]], [f[2], f[3]], [1.0, 0.0]),
        2 => ([f[2], f[3]], [f[0], f[3]], [0.0, 1.0]),
        _ => ([f[0], f[3]], [f[0], f[1]], [-1.0, 0.0]),
    };
    let mid = geom::scale(geom::add(a, e), 0.5);
    // Distance from the hoarding to 0.9 m behind the kerb.
    let mut off = 0.0;
    while off < 12.0 && site.clearance(geom::add(mid, geom::scale(out_dir, off + 0.25))) > 0.9 {
        off += 0.25;
    }
    let dir = geom::normalize(geom::sub(e, a));
    let len = geom::dist(a, e);
    let n = (len / 3.2).floor() as usize;
    for i in 0..n {
        let s = (i as f32 + 0.5) * len / n as f32;
        let p = geom::add(geom::add(a, geom::scale(dir, s)), geom::scale(out_dir, off));
        let cl = site.clearance(p);
        if !(0.4..=2.5).contains(&cl) || discs.iter().any(|(q, rr)| geom::dist(p, *q) < *rr) {
            continue;
        }
        d.push(decor(
            site,
            DecorKind::JerseyBarrier,
            p,
            geom::yaw_of(dir),
            [0.6, 3.0, 0.81],
        ));
    }
}

/// Hand-placed buildings and decor.
pub(super) fn extra_buildings(site: &mut Site) -> (Vec<Building>, Vec<Decor>) {
    let discs = junction_discs(site);
    let mut b = Vec::new();
    let mut d = Vec::new();
    landmarks(site, &discs, &mut b, &mut d);
    for (r, kerb) in SITES {
        let inner = interior(site, centre(r), 80.0);
        construction(site, &discs, inner, kerb, &mut b, &mut d);
    }
    // Towers: every ordinary core block (tallest near the Spire), some midtown blocks.
    let special = |c: V2| {
        let inside = |r: [f32; 4]| c[0] > r[0] && c[0] < r[2] && c[1] > r[1] && c[1] < r[3];
        inside(PLAZA)
            || inside(CITY_HALL)
            || inside(STATION)
            || inside(STADIUM)
            || SITES.iter().any(|(r, _)| inside(*r))
    };
    let [cx0, _] = plan::METRO_CORE_LO;
    let [cx1, _] = plan::METRO_CORE_HI;
    for j in 0..YS.len() - 1 {
        for i in 0..XS.len() - 1 {
            let r = [XS[i], YS[j], XS[i + 1], YS[j + 1]];
            let c = centre(r);
            if special(c) || c[0] < WESTBROOK_X {
                continue;
            }
            let inner = interior(site, c, 70.0);
            if c[0] > cx0 && c[0] < cx1 {
                let near = 1.0 - smoothstep(0.0, 950.0, geom::dist(c, SPIRE));
                let hmax = 70.0 + 190.0 * near;
                tower_block(site, &discs, inner, 0.35 * hmax + 20.0, hmax, &mut b);
            } else if site.rng.chance(0.5) {
                let h = site.rng.range_f32(28.0, 85.0);
                let q = around(centre(inner), 16.0, 16.0);
                tower(site, &discs, q, h, BuildingKind::Block, &mut b);
            }
        }
    }
    // Waterfront offices and lofts.
    for j in 0..WYS.len() - 1 {
        for i in 0..WXS.len() {
            let x1 = if i + 1 < WXS.len() {
                WXS[i + 1]
            } else {
                seawall_x(WYS[j])
            };
            let c = [(WXS[i] + x1) * 0.5, (WYS[j] + WYS[j + 1]) * 0.5];
            if c[1] < 4200.0 || !site.rng.chance(0.45) {
                continue;
            }
            let inner = interior(site, c, 90.0);
            let h = site.rng.range_f32(22.0, 65.0);
            let q = around(centre(inner), 15.0, 18.0);
            tower(site, &discs, q, h, BuildingKind::Block, &mut b);
        }
    }
    (b, d)
}

/// The Meridian Spire and its plaza, City Hall, Central Station, the Coliseum, Beacon Point
/// Tower and the Seawall Point lighthouse; billboards round the plaza.
fn landmarks(site: &mut Site, discs: &[(V2, f32)], b: &mut Vec<Building>, d: &mut Vec<Decor>) {
    let lm = BuildingKind::Landmark;
    // Meridian Plaza: a paved plinth over the whole superblock, the Spire in the middle.
    let plaza = interior(site, SPIRE, 120.0);
    let plinth = shrink(plaza, 0.5);
    let base = on_ground(site, fp_rect(plinth), 0.4, lm);
    let top = base.roof_z();
    b.push(base);
    let mut z = top;
    for (half, h) in [(31.0, 140.0), (25.0, 110.0), (19.0, 90.0), (8.0, 45.0)] {
        b.push(stacked(fp_rect(around(SPIRE, half, half)), h, z, lm));
        z += h;
    }
    // Billboards round the square facing the streets: two per side.
    for k in 0..4 {
        let (a, e, out_dir): (V2, V2, V2) = match k {
            0 => ([plinth[0], plinth[1]], [plinth[2], plinth[1]], [0.0, -1.0]),
            1 => ([plinth[2], plinth[1]], [plinth[2], plinth[3]], [1.0, 0.0]),
            2 => ([plinth[2], plinth[3]], [plinth[0], plinth[3]], [0.0, 1.0]),
            _ => ([plinth[0], plinth[3]], [plinth[0], plinth[1]], [-1.0, 0.0]),
        };
        for t in [0.22, 0.78] {
            let p = geom::add(
                geom::add(a, geom::scale(geom::sub(e, a), t)),
                geom::scale(out_dir, -6.0),
            );
            let mut bb = decor(
                site,
                DecorKind::Billboard,
                p,
                geom::yaw_of(out_dir),
                [14.0, 0.8, 11.0],
            );
            bb.pos[2] = top;
            d.push(bb);
        }
    }

    // City Hall: a broad hall with a central clock tower.
    let ch = interior(site, centre(CITY_HALL), 70.0);
    let c = centre(ch);
    let hall = around(c, (ch[2] - ch[0]) * 0.42, (ch[3] - ch[1]) * 0.4);
    if let Some(hall) = fit_rect(site, discs, hall, 4) {
        let main = on_ground(site, fp_rect(hall), 26.0, lm);
        let z1 = main.roof_z();
        b.push(main);
        b.push(stacked(fp_rect(around(c, 10.0, 10.0)), 22.0, z1, lm));
        b.push(stacked(fp_rect(around(c, 5.0, 5.0)), 14.0, z1 + 22.0, lm));
    }

    // Ashpeak Central Station: a long hall on Southgate Boulevard, clock tower at its west end.
    let st = interior(site, centre(STATION), 110.0);
    if let Some(hall) = fit_rect(site, discs, shrink(st, 1.0), 6) {
        let main = on_ground(site, fp_rect(hall), 22.0, lm);
        let z1 = main.roof_z();
        b.push(main);
        let tc = [hall[0] + 10.0, (hall[1] + hall[3]) * 0.5];
        b.push(stacked(fp_rect(around(tc, 7.0, 7.0)), 30.0, z1, lm));
    }

    // Ashpeak Coliseum: an oval bowl north in its superblock (the lot is south).
    let [sx0, _, sx1, sy1] = STADIUM;
    let mut rx = (sx1 - sx0) * 0.5 - 14.0;
    let mut ry = 72.0;
    for _ in 0..6 {
        let c = [(sx0 + sx1) * 0.5, sy1 - 24.0 - ry];
        let fp = oval(c, rx, ry, 40);
        if fits(site, discs, &fp, CLEAR_M) {
            b.push(on_ground(site, fp, 35.0, lm));
            break;
        }
        rx *= 0.94;
        ry *= 0.94;
    }

    // Beacon Point Tower (observation tower) by Seawall Boulevard, the lighthouse at the point.
    for (at, parts) in [
        (
            [seawall_x(4650.0) + 40.0, 4650.0],
            &[(5.0, 150.0), (14.0, 16.0), (2.0, 30.0)][..],
        ),
        (
            [SEAWALL_POINT[0] + 40.0, SEAWALL_POINT[1] - 10.0],
            &[(4.5, 28.0), (2.5, 6.0)][..],
        ),
    ] {
        // Shift towards the boulevard if the coast is too close (stay on land, off the road).
        let mut p: V2 = at;
        let mut ok = false;
        for _ in 0..8 {
            let fp = oval(p, parts[0].0 + 2.0, parts[0].0 + 2.0, 12);
            if fits(site, discs, &fp, CLEAR_M) && ground_range(site, &fp).0 > 1.0 {
                ok = true;
                break;
            }
            p[0] -= 5.0;
        }
        if !ok {
            continue;
        }
        let first = on_ground(site, oval(p, parts[0].0, parts[0].0, 12), parts[0].1, lm);
        let mut z = first.roof_z();
        b.push(first);
        for (r, h) in &parts[1..] {
            b.push(stacked(oval(p, *r, *r, 12), *h, z, lm));
            z += h;
        }
    }
}

/// Points of interest.
pub(super) fn pois(site: &mut Site) -> Vec<Poi> {
    let mut out = Vec::new();
    // Race grids: eastbound along Southgate Boulevard from Tamarack Avenue, northbound on
    // Crowfoot Avenue past the Spire, northbound up Seawall Boulevard from the point.
    for (name, road, near, s) in [
        (
            plan::POI_DOWNTOWN,
            "Southgate Boulevard",
            [-1250.0, 4200.0],
            15.0,
        ),
        (plan::POI_SPIRE, "Crowfoot Avenue", [-600.0, 4650.0], 10.0),
        (
            plan::POI_WATERFRONT,
            "Seawall Boulevard",
            [1475.0, 3830.0],
            15.0,
        ),
    ] {
        if let Some(r) = site.road_named(road, near, 30.0) {
            out.extend(site.lane_poi(PoiKind::RaceStartGrid, name, r, s, true));
        }
    }
    for (kind, name, p) in [
        (
            PoiKind::PoliceStation,
            "Ashpeak PD Central",
            [-1250.0, 4450.0],
        ),
        (
            PoiKind::Helipad,
            "Ashpeak PD Central helipad",
            [-1240.0, 4460.0],
        ),
        (
            PoiKind::PoliceStation,
            "Ashpeak PD Northgate",
            [-500.0, 5330.0],
        ),
        (PoiKind::Garage, "Spire Auto Body", [-450.0, 4760.0]),
        (
            PoiKind::SafehouseCandidate,
            "Waterfront Lofts",
            [1300.0, 5200.0],
        ),
    ] {
        out.extend(site.block_poi(kind, name, p));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::tests::city;
    use super::*;

    fn in_core(p: V2) -> bool {
        let (lo, hi) = (plan::METRO_CORE_LO, plan::METRO_CORE_HI);
        p[0] >= lo[0] - 1.0 && p[0] <= hi[0] + 1.0 && p[1] >= lo[1] && p[1] <= hi[1] + 1.0
    }

    #[test]
    fn core_grid_is_signalised() {
        let (net, rep) = city();
        assert_eq!(rep.skipped_roads, 0, "skipped roads");
        let (mut sig, mut all) = (0, 0);
        for ix in &net.intersections {
            let p = geom::xy(net.nodes[ix.node as usize].pos);
            if !in_core(p) {
                continue;
            }
            all += 1;
            if matches!(ix.control, IntersectionControl::Signal { .. }) {
                sig += 1;
            }
        }
        // 13 × 10 grid minus the plaza's middle node; the station's T junctions included.
        assert!(sig >= 110, "{sig} signalised of {all} core junctions");
        assert_eq!(sig, all, "every core junction is signalised");
        // Bus lanes on the grand avenues, one-way avenues.
        let bus = net
            .lanes
            .iter()
            .filter(|l| l.kind == LaneKind::Bus && in_core(geom::xy(l.polyline[0])))
            .count();
        assert!(bus >= 20, "{bus} bus lanes downtown");
    }

    #[test]
    fn busy_core_zone() {
        let (net, _) = city();
        let core = net
            .zones
            .iter()
            .find(|z| {
                z.kind == DistrictKind::Core && geom::point_in_polygon([-700.0, 4700.0], &z.polygon)
            })
            .expect("Ashpeak City core zone");
        assert!(core.density_base >= 1.6 * default_density_base(DistrictKind::Core));
        // Tall, dense downtown: many towers over 100 m.
        let tall = net
            .buildings
            .iter()
            .filter(|b| {
                b.kind == BuildingKind::Block && b.height_m > 100.0 && in_core(b.footprint[0])
            })
            .count();
        let blocks = net
            .buildings
            .iter()
            .filter(|b| in_core(b.footprint[0]))
            .count();
        assert!(tall >= 25, "{tall} towers over 100 m");
        assert!(blocks >= 400, "{blocks} downtown buildings");
    }

    /// Distance from `p` to the nearest drivable lane centre line in Ashpeak City.
    fn lane_dist(net: &RoadNetwork, p: V2) -> f32 {
        let mut best = f32::INFINITY;
        for l in &net.lanes {
            if !l.kind.drivable() || !in_metro(geom::xy(l.polyline[0]), 300.0) {
                continue;
            }
            for w in l.polyline.windows(2) {
                best = best.min(geom::point_segment_distance(
                    p,
                    geom::xy(w[0]),
                    geom::xy(w[1]),
                ));
            }
        }
        best
    }

    #[test]
    fn landmarks_stand_off_the_roads() {
        let (net, _) = city();
        let lms: Vec<&Building> = net
            .buildings
            .iter()
            .filter(|b| b.kind == BuildingKind::Landmark && in_metro(b.footprint[0], 0.0))
            .collect();
        let top = lms.iter().map(|b| b.roof_z()).fold(0.0f32, f32::max);
        let ground = lms
            .iter()
            .filter(|b| b.footprint.iter().any(|p| geom::dist(*p, SPIRE) < 50.0))
            .map(|b| b.base_z_m)
            .fold(f32::INFINITY, f32::min);
        assert!(top - ground > 350.0, "Meridian Spire {:.0} m", top - ground);
        // Spire, plaza, City Hall, station, Coliseum, Beacon Point Tower, lighthouse.
        let near = |c: V2, r: f32| {
            lms.iter()
                .any(|b| b.footprint.iter().any(|p| geom::dist(*p, c) < r))
        };
        assert!(near(SPIRE, 120.0));
        assert!(near(centre(CITY_HALL), 60.0), "City Hall");
        assert!(near(centre(STATION), 110.0), "Central Station");
        assert!(near([135.0, 4800.0], 110.0), "Coliseum");
        assert!(
            near([seawall_x(4650.0) + 40.0, 4650.0], 60.0),
            "Beacon Point"
        );
        assert!(lms.len() >= 12, "{} landmark pieces", lms.len());
        for b in &lms {
            for p in &b.footprint {
                let dd = lane_dist(net, *p);
                assert!(dd > 4.0, "landmark corner {p:?} {dd:.1} m from a lane");
            }
        }
        let boards = net
            .decor
            .iter()
            .filter(|d| d.kind == DecorKind::Billboard && in_core([d.pos[0], d.pos[1]]))
            .count();
        assert!(boards >= 8, "{boards} billboards round the square");
    }

    #[test]
    fn construction_sites_with_cranes_and_barriers() {
        let (net, _) = city();
        let count = |k: DecorKind| {
            net.decor
                .iter()
                .filter(|d| d.kind == k && in_metro([d.pos[0], d.pos[1]], 0.0))
                .count()
        };
        let sites = net
            .buildings
            .iter()
            .filter(|b| b.kind == BuildingKind::Construction && b.height_m > 10.0)
            .filter(|b| in_metro(b.footprint[0], 0.0))
            .count();
        assert!(sites >= 4, "{sites} partial towers");
        assert!(count(DecorKind::Crane) >= 4);
        assert!(count(DecorKind::SiteFence) >= 80);
        assert!(count(DecorKind::JerseyBarrier) >= 10);
        for dc in &net.decor {
            if dc.kind == DecorKind::JerseyBarrier && in_metro([dc.pos[0], dc.pos[1]], 0.0) {
                let dd = lane_dist(net, [dc.pos[0], dc.pos[1]]);
                assert!(dd > 3.0, "barrier {:?} {dd:.1} m from a lane", dc.pos);
            }
        }
    }

    #[test]
    fn connection_terminals_have_metro_roads() {
        let (net, _) = city();
        let mut ends: Vec<V2> = plan::INTERCHANGES
            .iter()
            .flat_map(|ic| [(ic.a, ic.a_by), (ic.b, ic.b_by)])
            .filter(|(_, by)| *by == "metro")
            .map(|(p, _)| p)
            .collect();
        ends.extend(
            plan::FREEWAY_ENDS
                .iter()
                .filter(|e| e.2 == "metro")
                .map(|e| e.1),
        );
        assert_eq!(ends.len(), 9);
        for p in ends {
            let n = net
                .nodes
                .iter()
                .position(|n| geom::dist(geom::xy(n.pos), p) < 0.05)
                .unwrap_or_else(|| panic!("no node at {p:?}"));
            let lanes = net
                .lanes
                .iter()
                .filter(|l| {
                    l.kind.drivable() && (l.from_node as usize == n || l.to_node as usize == n)
                })
                .count();
            assert!(lanes >= 2, "terminal {p:?} has {lanes} lanes");
        }
    }

    #[test]
    fn metro_pois_present() {
        let (net, _) = city();
        for (kind, name) in [
            (PoiKind::RaceStartGrid, plan::POI_DOWNTOWN),
            (PoiKind::RaceStartGrid, plan::POI_SPIRE),
            (PoiKind::RaceStartGrid, plan::POI_WATERFRONT),
            (PoiKind::PoliceStation, "Ashpeak PD Central"),
            (PoiKind::Helipad, "Ashpeak PD Central helipad"),
            (PoiKind::PoliceStation, "Ashpeak PD Northgate"),
            (PoiKind::Garage, "Spire Auto Body"),
            (PoiKind::SafehouseCandidate, "Waterfront Lofts"),
        ] {
            let p = net
                .pois
                .iter()
                .find(|p| p.kind == kind && p.name == name)
                .unwrap_or_else(|| panic!("no {name}"));
            assert!(in_metro([p.pos[0], p.pos[1]], 0.0), "{name} at {:?}", p.pos);
            let lane = &net.lanes[p.lane.expect("on a lane") as usize];
            assert!(lane.kind.drivable(), "{name} on a {:?} lane", lane.kind);
        }
    }

    #[test]
    fn metro_streets_are_one_network() {
        // Core, midtown, Northgate Heights and the waterfront are strongly connected on their
        // own streets (the freeways add more ways in and out).
        let (net, _) = city();
        let map = crate::CityMap::new(net.clone());
        let bad: Vec<u32> = super::super::tests::unreachable_lanes(&map, [-650.0, 4400.0])
            .into_iter()
            .filter(|l| {
                let p = geom::xy(net.lanes[*l as usize].polyline[0]);
                in_metro(p, 0.0) && p[1] > 4100.0
            })
            .collect();
        assert!(
            bad.is_empty(),
            "{} Ashpeak City lanes not strongly connected, e.g. {:?}",
            bad.len(),
            bad.iter()
                .take(6)
                .map(|l| net.lanes[*l as usize].polyline[0])
                .collect::<Vec<_>>()
        );
    }
}
