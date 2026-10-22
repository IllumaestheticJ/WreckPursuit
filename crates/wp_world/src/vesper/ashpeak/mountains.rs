//! Ashpeak Range: Crowfoot Pass Road (the touge), the Halcyon ridge and summit roads, the
//! Halcyon Grade down the north face, overlooks, the Larchwood forest road, pine forests,
//! cabins, the Halcyon Observatory and the ASHPEAK hillside letters.
//!
//! Every road here is drawn with a turtle ([`Cmd`]: straights and circular arcs, legs run to a
//! given x or y), so hairpin radii and straights are exact; a road's last few hundred metres
//! are warped onto its end point. The same paths shape the ground ([`corridors`] feeds the
//! terrain's road benches), so the hairpin stacks sit on benches cut into the slope.
//!
//! - **Crowfoot Pass Road**: from Route 40 North's dead end ([`plan::R40N_END`]) a short
//!   foothill run with a fast kink and an ess, then nine hairpins up the south face to the
//!   Crowfoot saddle; down the north face as long flowing traverses with esses and three
//!   hairpins to the Crowfoot Interchange.
//! - **Halcyon Ridge Road** west from the saddle along the crest (two overlooks, the forest
//!   road), then **Observatory Road** switchbacks up the summit's east flank to the
//!   **Halcyon Observatory Loop** around the observatory.
//! - **Halcyon Grade**: from the ridge down the north face to the Granite Street
//!   interchange, closing the loop pass → ridge → Granite Street → Crosstown → Crowfoot.

use super::super::*;
use super::plan;

/// The touge.
pub(super) const PASS: &str = "Crowfoot Pass Road";
/// Saddle → summit flank, along the crest.
pub(super) const RIDGE: &str = "Halcyon Ridge Road";
/// Switchbacks up to the observatory.
pub(super) const SUMMIT_ROAD: &str = "Observatory Road";
/// Lot loop around the observatory.
pub(super) const LOT: &str = "Halcyon Observatory Loop";
/// Ridge → Granite Street interchange down the north face.
pub(super) const GRADE: &str = "Halcyon Grade";
/// Gravel forest road down the south face to Larchwood Camp.
pub(super) const FOREST: &str = "Larchwood Forest Road";
/// Overlook lot south of the ridge (towards Port Vesper).
pub(super) const OVERLOOK_S: &str = "Crowfoot Overlook";
/// Overlook lot north of the ridge (towards Ashpeak City).
pub(super) const OVERLOOK_N: &str = "Silverline Overlook";

/// Mountain road: two narrow lanes, no parking, steep, banked in the bends.
const TOUGE: Style = Style {
    class: RoadClass::Local,
    fwd: 1,
    bwd: 1,
    speed: 13.4112, // 30 mph
    parking: false,
    grade: 0.12,
    bank_max: 0.03,
    material: RoadMaterial::Asphalt,
};
/// Lots and overlook spurs.
const LOT_ROAD: Style = Style {
    speed: 6.7056, // 15 mph
    grade: 0.10,
    bank_max: 0.0,
    ..TOUGE
};
/// Graded gravel forest road.
const GRAVEL: Style = Style {
    speed: 11.176, // 25 mph
    material: RoadMaterial::Gravel,
    ..TOUGE
};

/// Lot loop radius around the observatory, m.
const LOT_RADIUS_M: f32 = 32.0;

/// Turtle command: `S(len)` straight, `A(radius, degrees)` arc (+ = left), `X(x)` / `Y(y)`
/// straight on until x / y is reached.
#[derive(Debug, Clone, Copy)]
enum Cmd {
    S(f32),
    A(f32, f32),
    X(f32),
    Y(f32),
}
use Cmd::{A, S, X, Y};

/// Walk `cmds` from `start` heading `heading_deg` (0 = +x, 90 = +y); returns the path
/// (about every 3 m) and the final heading in degrees.
fn turtle(start: V2, heading_deg: f32, cmds: &[Cmd]) -> (Vec<V2>, f32) {
    const STEP: f32 = 3.0;
    let mut p = start;
    let mut h = heading_deg.to_radians();
    let mut out = vec![p];
    for c in cmds {
        let (s, co) = wp_math::sin_cos(h);
        let (len, turn) = match *c {
            S(l) => (l, 0.0),
            A(r, deg) => (r * deg.to_radians().abs(), deg.to_radians()),
            X(x) => ((x - p[0]) / co, 0.0),
            Y(y) => ((y - p[1]) / s, 0.0),
        };
        debug_assert!(len > 0.0, "{c:?} from {p:?} heading {}", h.to_degrees());
        let len = len.max(0.0);
        let n = ((len / STEP).ceil() as usize).max(if turn != 0.0 { 2 } else { 1 });
        let (dl, dh) = (len / n as f32, turn / n as f32);
        for _ in 0..n {
            let (s, co) = wp_math::sin_cos(h + 0.5 * dh);
            p = [p[0] + co * dl, p[1] + s * dl];
            h += dh;
            out.push(p);
        }
    }
    (out, h.to_degrees())
}

/// Shift the last `tail` m of `path` (smoothly, more towards the end) so it ends at `target`.
fn warp(path: &mut [V2], target: V2, tail: f32) {
    let cum = cum2(path);
    let total = cum[cum.len() - 1];
    let e = geom::sub(target, path[path.len() - 1]);
    for (q, s) in path.iter_mut().zip(&cum) {
        let t = smoothstep(total - tail, total, *s);
        *q = geom::add(*q, geom::scale(e, t));
    }
}

/// Interchange terminal `a` of the named interchange.
fn terminal(name: &str) -> V2 {
    plan::INTERCHANGES
        .iter()
        .find(|i| i.name == name)
        .map(|i| i.a)
        .expect("planned interchange")
}

/// Position on `path` where it first reaches `x` going west, as an arc length.
fn arc_at_x(path: &[V2], x: f32) -> f32 {
    let cum = cum2(path);
    let i = path
        .iter()
        .position(|q| q[0] <= x)
        .unwrap_or(path.len() - 1);
    cum[i]
}

/// Every road path of the range (plan coordinates).
pub(super) struct Plans {
    /// Crowfoot Pass Road, R40N_END → saddle.
    pub south: Vec<V2>,
    /// Crowfoot Pass Road, saddle → Crowfoot Interchange.
    pub north: Vec<V2>,
    /// Halcyon Ridge Road, saddle → summit flank.
    pub ridge: Vec<V2>,
    /// Ridge junction arc lengths: south overlook, forest road, north overlook, Halcyon Grade.
    pub ridge_cuts: [f32; 4],
    /// Observatory Road, ridge end → lot gate.
    pub summit: Vec<V2>,
    /// Lot loop (open; closes back to its first point, the gate).
    pub lot: Vec<V2>,
    /// Halcyon Grade, ridge → Granite Street interchange.
    pub grade: Vec<V2>,
    /// Larchwood Forest Road, ridge → camp.
    pub forest: Vec<V2>,
    /// South overlook spur.
    pub overlook_s: Vec<V2>,
    /// North overlook spur.
    pub overlook_n: Vec<V2>,
}

/// The range's road paths (deterministic, no randomness).
pub(super) fn plans() -> Plans {
    let saddle = plan::CROWFOOT_SADDLE;
    // South face: a foothill run (fast kink, ess), then nine hairpins (radii 16–24 m) on legs
    // of 250–330 m, some with esses.
    let (mut south, _) = turtle(
        plan::R40N_END,
        90.0,
        &[
            S(30.0),
            A(300.0, 10.0),
            S(10.0),
            A(110.0, -30.0),
            A(110.0, 25.0),
            Y(1880.0),
            A(50.0, 81.0),
            X(-260.0),
            A(20.0, -168.0), // 1
            S(60.0),
            A(90.0, 14.0),
            A(90.0, -14.0),
            X(10.0),
            A(22.0, 168.0), // 2
            X(-300.0),
            A(18.0, -168.0), // 3
            S(70.0),
            A(120.0, -10.0),
            S(50.0),
            A(120.0, 10.0),
            X(30.0),
            A(24.0, 168.0), // 4
            S(70.0),
            A(70.0, 20.0),
            A(70.0, -20.0),
            X(-290.0),
            A(20.0, -168.0), // 5
            X(0.0),
            A(16.0, 168.0), // 6
            S(60.0),
            A(100.0, -16.0),
            A(100.0, 16.0),
            X(-310.0),
            A(21.0, -168.0), // 7
            X(-40.0),
            A(19.0, 168.0), // 8
            X(-330.0),
            A(23.0, -154.0), // 9
            S(10.0),
            A(60.0, 70.0),
            Y(saddle[1]),
        ],
    );
    warp(&mut south, saddle, 400.0);
    // North face: long traverses with esses, three hairpins, then sweeping bends down to the
    // Crowfoot Interchange.
    let (mut north, _) = turtle(
        saddle,
        92.0,
        &[
            S(40.0),
            A(60.0, -82.0),
            S(60.0),
            A(150.0, 10.0),
            A(150.0, -10.0),
            X(160.0),
            A(25.0, 160.0),
            S(80.0),
            A(120.0, -15.0),
            S(40.0),
            A(120.0, 15.0),
            X(-470.0),
            A(22.0, -164.0),
            S(120.0),
            A(200.0, 8.0),
            A(200.0, -8.0),
            X(60.0),
            A(28.0, 160.0),
            S(60.0),
            A(110.0, -20.0),
            A(110.0, 20.0),
            X(-380.0),
            A(90.0, -80.0),
            A(150.0, 40.0),
            S(60.0),
            A(130.0, -40.0),
            S(20.0),
            A(160.0, 25.0),
            S(20.0),
            A(200.0, -25.0),
            Y(terminal("Crowfoot Interchange")[1]),
        ],
    );
    warp(&mut north, terminal("Crowfoot Interchange"), 500.0);
    // Along the crest west to the summit's east flank.
    let (ridge, ridge_heading) = turtle(
        saddle,
        180.0,
        &[
            S(100.0),
            A(400.0, -6.0),
            S(200.0),
            A(300.0, 10.0),
            S(150.0),
            A(300.0, -14.0),
            X(-1300.0),
        ],
    );
    let ridge_cuts = [
        arc_at_x(&ridge, -450.0),
        arc_at_x(&ridge, -650.0),
        arc_at_x(&ridge, -880.0),
        arc_at_x(&ridge, -1100.0),
    ];
    let rcum = cum2(&ridge);
    let at = |s: f32| point_on(&ridge, &rcum, s).0;
    let gate = geom::add(plan::ASHPEAK_SUMMIT, [LOT_RADIUS_M, 0.0]);
    let (mut summit, _) = turtle(
        ridge[ridge.len() - 1],
        ridge_heading,
        &[
            A(40.0, -80.0),
            Y(2865.0),
            A(20.0, 172.0),
            Y(2610.0),
            A(20.0, -172.0),
            Y(2865.0),
            A(22.0, 172.0),
            Y(2610.0),
            A(18.0, -172.0),
            Y(2865.0),
            A(20.0, 172.0),
            Y(2610.0),
            A(21.0, -172.0),
            Y(2865.0),
            A(19.0, 172.0),
            Y(2610.0),
            A(20.0, -172.0),
            Y(2865.0),
            A(18.0, 172.0),
            Y(gate[1] + 30.0),
            A(30.0, -82.0),
            X(gate[0]),
        ],
    );
    warp(&mut summit, gate, 150.0);
    let lot: Vec<V2> = (0..48)
        .map(|k| {
            let (s, c) = wp_math::sin_cos(std::f32::consts::TAU * k as f32 / 48.0);
            geom::add(plan::ASHPEAK_SUMMIT, geom::scale([c, s], LOT_RADIUS_M))
        })
        .collect();
    let granite = terminal("Granite Street");
    let (mut grade, _) = turtle(
        at(ridge_cuts[3]),
        90.0,
        &[
            S(40.0),
            A(80.0, 80.0),
            X(-1250.0),
            A(25.0, -165.0),
            X(-720.0),
            A(25.0, 165.0),
            X(-1250.0),
            A(25.0, -165.0),
            X(-720.0),
            A(25.0, 165.0),
            X(-1200.0),
            A(120.0, -60.0),
            S(100.0),
            A(150.0, 40.0),
            S(80.0),
            A(150.0, -60.0),
            Y(granite[1]),
        ],
    );
    warp(&mut grade, granite, 500.0);
    let (forest, _) = turtle(
        at(ridge_cuts[1]),
        270.0,
        &[
            S(30.0),
            A(60.0, -85.0),
            X(-1000.0),
            A(22.0, 168.0),
            X(-600.0),
            A(22.0, -168.0),
            X(-1000.0),
            A(22.0, 168.0),
            X(-760.0),
            A(80.0, -40.0),
            S(40.0),
        ],
    );
    let (overlook_s, _) = turtle(at(ridge_cuts[0]), 270.0, &[S(45.0)]);
    let (overlook_n, _) = turtle(at(ridge_cuts[2]), 90.0, &[S(45.0)]);
    Plans {
        south,
        north,
        ridge,
        ridge_cuts,
        summit,
        lot,
        grade,
        forest,
        overlook_s,
        overlook_n,
    }
}

/// Every mountain road's centreline and design grade, in design order (an end on an earlier
/// road takes that road's height there): the terrain's road benches.
pub(super) fn corridors() -> Vec<(Vec<V2>, f32)> {
    let p = plans();
    let mut lot = p.lot.clone();
    lot.push(p.lot[0]);
    let g = super::terrain::DESIGN_GRADE;
    vec![
        (p.south, g),
        (p.north, g),
        (p.ridge, g),
        // The last climb to the summit is steeper (still under the 12 % cap).
        (p.summit, 0.108),
        (lot, g),
        (p.grade, g),
        (p.forest, g),
        (p.overlook_s, g),
        (p.overlook_n, g),
    ]
}

/// Lay out this district's roads.
pub(super) fn layout(g: &mut Gen) {
    let p = plans();
    let start = g.node(plan::R40N_END);
    let saddle = g.node(plan::CROWFOOT_SADDLE);
    let crowfoot = g.node(terminal("Crowfoot Interchange"));
    let granite = g.node(terminal("Granite Street"));
    g.road(start, saddle, p.south.clone(), TOUGE, PASS);
    g.road(saddle, crowfoot, p.north.clone(), TOUGE, PASS);
    let (cuts, _, ridge_end) = g.chain(&p.ridge, &p.ridge_cuts, TOUGE, RIDGE);
    let gate = g.node(p.lot[0]);
    g.road(ridge_end, gate, p.summit.clone(), TOUGE, SUMMIT_ROAD);
    let half = LOT_RADIUS_M * std::f32::consts::PI;
    g.ring(&p.lot, &[0.0, half], LOT_ROAD, LOT);
    g.road(cuts[3], granite, p.grade.clone(), TOUGE, GRADE);
    let camp = g.node(p.forest[p.forest.len() - 1]);
    g.road(cuts[1], camp, p.forest.clone(), GRAVEL, FOREST);
    for (k, path, name) in [
        (0, &p.overlook_s, OVERLOOK_S),
        (2, &p.overlook_n, OVERLOOK_N),
    ] {
        let end = g.node(path[path.len() - 1]);
        g.road(cuts[k], end, path.clone(), LOT_ROAD, name);
    }
}

/// Covered roads (tunnels) on this district's roads.
pub(super) fn covers() -> Vec<CoverSpec> {
    Vec::new()
}

/// The range's area (the zone, and where no generic street-front buildings go), m.
fn range_area() -> Vec<V2> {
    rect(-2600.0, plan::VESPER_HI[1], 1620.0, plan::METRO_LO[1])
}

/// This district's zones: quiet mountain roads (a third of the suburbs' traffic).
pub(super) fn zones() -> Vec<Zone> {
    vec![Zone {
        kind: DistrictKind::Suburb,
        density_base: default_density_base(DistrictKind::Suburb) / 3.0,
        polygon: range_area(),
    }]
}

/// Whether the generic street-front building pass may build at `p` (not in the range, nor
/// along the pass's first stretch inside Port Vesper: no houses along the touge).
pub(super) fn generic_buildings_at(p: V2) -> bool {
    let pass_foot = (p[0] - plan::R40N_END[0]).abs() < 80.0 && p[1] > plan::R40N_END[1] - 20.0;
    !(pass_foot || geom::point_in_polygon(p, &range_area()))
}

/// Rectangle footprint (counter-clockwise) centred on `c`, `w` across and `l` along `dir`.
fn rect_fp(c: V2, dir: V2, w: f32, l: f32) -> Vec<V2> {
    let d = geom::normalize(dir);
    let r = geom::right_of(d);
    let (hl, hw) = (0.5 * l, 0.5 * w);
    let corner = |a: f32, b: f32| geom::add(c, geom::add(geom::scale(d, a), geom::scale(r, b)));
    let mut fp = vec![
        corner(-hl, -hw),
        corner(-hl, hw),
        corner(hl, hw),
        corner(hl, -hw),
    ];
    if signed_area(&fp) < 0.0 {
        fp.reverse();
    }
    fp
}

/// Lowest ground under a footprint (its corners and centre), m.
fn base_of(site: &Site, fp: &[V2]) -> f32 {
    let n = fp.len() as f32;
    let c = fp
        .iter()
        .fold([0.0, 0.0], |a, q| [a[0] + q[0] / n, a[1] + q[1] / n]);
    fp.iter()
        .chain(std::iter::once(&c))
        .map(|q| site.ground(*q))
        .fold(f32::INFINITY, f32::min)
}

/// Highest ground under a footprint's corners, m.
fn top_of(site: &Site, fp: &[V2]) -> f32 {
    fp.iter()
        .map(|q| site.ground(*q))
        .fold(f32::NEG_INFINITY, f32::max)
}

/// The ASHPEAK letters, 5×7 each (row 0 at the top).
const LETTERS: [[&str; 7]; 7] = [
    [
        ".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#",
    ],
    [
        ".####", "#....", "#....", ".###.", "....#", "....#", "####.",
    ],
    [
        "#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#",
    ],
    [
        "####.", "#...#", "#...#", "####.", "#....", "#....", "#....",
    ],
    [
        "#####", "#....", "#....", "####.", "#....", "#....", "#####",
    ],
    [
        ".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#",
    ],
    [
        "#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#",
    ],
];
/// Centre of the ASHPEAK letters on the summit's north face (read from Ashpeak City, looking
/// south up the slope), m.
const SIGN_CENTRE: V2 = [-1680.0, 3180.0];
/// Letter pixel size across (x) and up the slope (y), m.
const SIGN_PX: V2 = [5.0, 7.0];

/// The hillside letters: one thin landmark slab per horizontal run of letter pixels, lying on
/// the slope (tops uphill, to the south; the first letter at the east end, as read from the
/// north).
fn sign(site: &Site) -> Vec<Building> {
    let cols = LETTERS.len() * 6 - 1;
    let mut out = Vec::new();
    for (li, rows) in LETTERS.iter().enumerate() {
        for (row, line) in rows.iter().enumerate() {
            let bytes = line.as_bytes();
            let mut c = 0;
            while c < 5 {
                if bytes[c] != b'#' {
                    c += 1;
                    continue;
                }
                let c0 = c;
                while c < 5 && bytes[c] == b'#' {
                    c += 1;
                }
                // Column counted from the reader's left (east).
                let (g0, g1) = ((li * 6 + c0) as f32, (li * 6 + c) as f32);
                let x_east = SIGN_CENTRE[0] + 0.5 * cols as f32 * SIGN_PX[0];
                let (xa, xb) = (x_east - g1 * SIGN_PX[0], x_east - g0 * SIGN_PX[0]);
                let y_top = SIGN_CENTRE[1] - 3.5 * SIGN_PX[1];
                let (ya, yb) = (
                    y_top + row as f32 * SIGN_PX[1],
                    y_top + (row + 1) as f32 * SIGN_PX[1],
                );
                let fp = vec![[xa, ya], [xb, ya], [xb, yb], [xa, yb]];
                let base = base_of(site, &fp);
                let rise = top_of(site, &fp) - base;
                out.push(Building {
                    footprint: fp,
                    height_m: (rise + 1.2).max(3.2),
                    base_z_m: base,
                    kind: BuildingKind::Landmark,
                    roof: RoofShape::Flat,
                    roof_rise_m: 0.0,
                });
            }
        }
    }
    out
}

/// A footprint is clear of every road by `gap` m.
fn clear(site: &Site, fp: &[V2], gap: f32) -> bool {
    fp.iter().all(|q| site.clearance(*q) > gap)
}

/// Hand-placed buildings and decor: the observatory, cabins, the hillside letters, overlook
/// barriers and the pine forests.
pub(super) fn extra_buildings(site: &mut Site) -> (Vec<Building>, Vec<Decor>) {
    let p = plans();
    let mut b = Vec::new();
    let mut d = Vec::new();
    // Halcyon Observatory: an octagonal drum inside the lot loop, an office wing beside it.
    let c = plan::ASHPEAK_SUMMIT;
    let drum: Vec<V2> = (0..8)
        .map(|k| {
            let (s, co) = wp_math::sin_cos(std::f32::consts::TAU * (k as f32 + 0.5) / 8.0);
            geom::add(c, [11.0 * co, 11.0 * s])
        })
        .collect();
    let wing = rect_fp(geom::add(c, [-4.0, -15.0]), [1.0, 0.0], 7.0, 16.0);
    for (fp, h) in [(drum, 15.0), (wing, 7.0)] {
        let base = base_of(site, &fp);
        b.push(Building {
            footprint: fp,
            height_m: h,
            base_z_m: base,
            kind: BuildingKind::Landmark,
            roof: RoofShape::Flat,
            roof_rise_m: 0.0,
        });
    }
    // Cabins: around Larchwood Camp (the forest road's end) and below the ridge.
    let camp = p.forest[p.forest.len() - 1];
    let ridge_mid = point_on(&p.ridge, &cum2(&p.ridge), p.ridge_cuts[2] + 60.0).0;
    let mut spots: Vec<(V2, V2)> = Vec::new();
    for k in 0..7 {
        let (s, co) = wp_math::sin_cos(0.9 * k as f32);
        spots.push((
            geom::add(
                camp,
                [(22.0 + 4.0 * k as f32) * co, (22.0 + 4.0 * k as f32) * s],
            ),
            [co, s],
        ));
    }
    spots.push((geom::add(ridge_mid, [0.0, -26.0]), [1.0, 0.1]));
    spots.push((geom::add(ridge_mid, [40.0, -30.0]), [1.0, -0.2]));
    let mut cabins = 0;
    for (pos, dir) in spots {
        let fp = rect_fp(pos, dir, 7.0, 9.5);
        if !clear(site, &fp, 4.0) {
            continue;
        }
        let base = base_of(site, &fp);
        let rise = top_of(site, &fp) - base;
        if rise > 3.0 {
            continue; // too steep for a cabin
        }
        b.push(Building {
            footprint: fp,
            height_m: 3.4 + rise,
            base_z_m: base,
            kind: BuildingKind::House,
            roof: RoofShape::Gable,
            roof_rise_m: 2.6,
        });
        cabins += 1;
        if cabins >= 6 {
            break;
        }
    }
    // The hillside letters.
    b.extend(sign(site));
    // Jersey barriers along the overlook ends.
    for spur in [&p.overlook_s, &p.overlook_n] {
        let n = spur.len();
        let end = spur[n - 1];
        let dir = geom::normalize(geom::sub(end, spur[n - 2]));
        for k in -3i32..=3 {
            let a = 0.28 * k as f32;
            let (s, co) = wp_math::sin_cos(a);
            let out = [dir[0] * co - dir[1] * s, dir[0] * s + dir[1] * co];
            let q = geom::add(end, geom::scale(out, 15.0));
            if site.clearance(q) < 0.6 {
                continue;
            }
            let along = geom::right_of(out);
            d.push(Decor {
                kind: DecorKind::JerseyBarrier,
                pos: [q[0], q[1], site.ground(q)],
                yaw: geom::yaw_of(along),
                size: [0.6, 3.6, 0.85],
            });
        }
    }
    // Pine forests on the slopes: clumped (noise), thinning out towards the treeline, never
    // on a road, a building or the river beds.
    let taken: Vec<(V2, f32)> = b
        .iter()
        .map(|bl| {
            let n = bl.footprint.len() as f32;
            let c = bl
                .footprint
                .iter()
                .fold([0.0, 0.0], |a, q| [a[0] + q[0] / n, a[1] + q[1] / n]);
            let r = bl
                .footprint
                .iter()
                .map(|q| geom::dist(*q, c))
                .fold(0.0f32, f32::max);
            (c, r + 6.0)
        })
        .collect();
    let mut trees = 0usize;
    let spacing = 21.0;
    let (x0, x1) = (-4500.0f32, 1560.0f32);
    let (y0, y1) = (plan::VESPER_HI[1] + 120.0, plan::METRO_LO[1] - 40.0);
    let nx = ((x1 - x0) / spacing) as usize;
    let ny = ((y1 - y0) / spacing) as usize;
    'grid: for j in 0..ny {
        for i in 0..nx {
            let jx = site.rng.range_f32(-0.45, 0.45);
            let jy = site.rng.range_f32(-0.45, 0.45);
            let roll = site.rng.next_f32();
            let size = site.rng.next_f32();
            let q = [
                x0 + (i as f32 + 0.5 + jx) * spacing,
                y0 + (j as f32 + 0.5 + jy) * spacing,
            ];
            let z = site.ground(q);
            // Forest only on the mountain: above the foothills, below the treeline; west of
            // the Sable valley only high up (the farms keep the low slopes).
            let floor = if q[0] < -2600.0 { 150.0 } else { 45.0 };
            if z < floor || z > 520.0 {
                continue;
            }
            let (s, co) = wp_math::sin_cos(q[0] / 310.0 + 0.7 * wp_math::sin_cos(q[1] / 240.0).0);
            let clump = 0.5 + 0.3 * s * co + 0.2 * wp_math::sin_cos(q[1] / 170.0).1;
            let thin = smoothstep(520.0, 380.0, z);
            if roll > 0.55 * clump * thin {
                continue;
            }
            if site.clearance(q) <= 6.0 || taken.iter().any(|(c, r)| geom::dist(*c, q) < *r) {
                continue;
            }
            let h = 11.0 + 9.0 * size;
            d.push(Decor {
                kind: DecorKind::PineTree,
                pos: [q[0], q[1], z],
                yaw: 0.0,
                size: [0.32 * h, 0.32 * h, h],
            });
            trees += 1;
            if trees >= 5800 {
                break 'grid;
            }
        }
    }
    (b, d)
}

/// Points of interest: the pass's three race grids and the observatory.
pub(super) fn pois(site: &mut Site) -> Vec<Poi> {
    let p = plans();
    let mut out = Vec::new();
    let probe = |path: &[V2], s: f32| point_on(path, &cum2(path), s).0;
    if let Some(r) = site.road_named(PASS, probe(&p.south, 60.0), 10.0) {
        out.extend(site.lane_poi(
            PoiKind::RaceStartGrid,
            plan::POI_CROWFOOT_ASCENT,
            r,
            15.0,
            true,
        ));
    }
    if let Some(r) = site.road_named(PASS, probe(&p.north, 60.0), 10.0) {
        out.extend(site.lane_poi(
            PoiKind::RaceStartGrid,
            plan::POI_CROWFOOT_SUMMIT,
            r,
            25.0,
            true,
        ));
        out.extend(site.lane_poi(
            PoiKind::RaceStartGrid,
            plan::POI_CROWFOOT_NORTH,
            r,
            30.0,
            false,
        ));
    }
    if let Some(r) = site.road_named(SUMMIT_ROAD, probe(&p.summit, 60.0), 10.0) {
        // Downhill from the lot gate.
        out.extend(site.lane_poi(
            PoiKind::RaceStartGrid,
            plan::POI_OBSERVATORY,
            r,
            25.0,
            false,
        ));
    }
    if let Some(r) = site.road_named(GRADE, probe(&p.grade, 60.0), 10.0) {
        out.extend(site.lane_poi(PoiKind::RaceStartGrid, GRADE, r, 20.0, true));
    }
    let lot_probe = geom::add(plan::ASHPEAK_SUMMIT, [0.0, LOT_RADIUS_M]);
    out.extend(site.block_poi(
        PoiKind::SafehouseCandidate,
        plan::POI_OBSERVATORY,
        lot_probe,
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::super::{default_config, generate_with_report};
    use super::*;

    fn city() -> &'static (RoadNetwork, VesperReport) {
        static CITY: std::sync::OnceLock<(RoadNetwork, VesperReport)> = std::sync::OnceLock::new();
        CITY.get_or_init(|| generate_with_report(&default_config()))
    }

    const MINE: [&str; 8] = [
        PASS,
        RIDGE,
        SUMMIT_ROAD,
        LOT,
        GRADE,
        FOREST,
        OVERLOOK_S,
        OVERLOOK_N,
    ];

    /// Surfaces of the roads named `name`.
    fn surfaces<'a>(net: &'a RoadNetwork, rep: &VesperReport, name: &str) -> Vec<&'a RoadSurface> {
        net.surfaces
            .iter()
            .filter(|s| rep.road_names[s.road as usize] == name)
            .collect()
    }

    /// Hairpins along a path: the heading turns by more than 150° within less than 80 m
    /// (each counted once).
    fn hairpins(p: &[V2]) -> usize {
        let cum = cum2(p);
        let mut head: Vec<f32> = Vec::new();
        for w in p.windows(2) {
            let d = geom::sub(w[1], w[0]);
            let a = wp_math::atan2(d[1], d[0]);
            let a = match head.last() {
                None => a,
                Some(&prev) => {
                    let mut x = a - prev;
                    while x > std::f32::consts::PI {
                        x -= std::f32::consts::TAU;
                    }
                    while x < -std::f32::consts::PI {
                        x += std::f32::consts::TAU;
                    }
                    prev + x
                }
            };
            head.push(a);
        }
        let (mut count, mut i) = (0, 0);
        while i < head.len() {
            let hit = (i..head.len())
                .take_while(|j| cum[*j] - cum[i] < 80.0)
                .find(|j| (head[*j] - head[i]).abs() > 150f32.to_radians());
            match hit {
                Some(j) => {
                    count += 1;
                    i = (j..head.len())
                        .find(|k| cum[*k] - cum[j] > 60.0)
                        .unwrap_or(head.len());
                }
                None => i += 1,
            }
        }
        count
    }

    #[test]
    fn crowfoot_pass_runs_end_to_end_over_the_saddle() {
        let (net, rep) = city();
        assert_eq!(rep.skipped_roads, 0);
        let pass = surfaces(net, rep, PASS);
        assert_eq!(pass.len(), 2, "south and north halves");
        let near = |p: V3, q: V2, r: f32| geom::dist(geom::xy(p), q) < r;
        let (south, north) = (pass[0], pass[1]);
        let crowfoot = terminal("Crowfoot Interchange");
        assert!(near(south.centre[0], plan::R40N_END, 15.0));
        assert!(near(
            *south.centre.last().unwrap(),
            plan::CROWFOOT_SADDLE,
            25.0
        ));
        assert!(near(north.centre[0], plan::CROWFOOT_SADDLE, 25.0));
        assert!(near(*north.centre.last().unwrap(), crowfoot, 15.0));
        // Nine hairpins on the south face, three on the north face.
        let xy = |s: &RoadSurface| -> Vec<V2> { s.centre.iter().map(|p| geom::xy(*p)).collect() };
        let (hs, hn) = (hairpins(&xy(south)), hairpins(&xy(north)));
        assert!(hs >= 8, "south face hairpins {hs}");
        assert!(hn >= 2, "north face hairpins {hn}");
        // The saddle is high above Port Vesper's north edge; a real descent beyond it.
        let rise = south.centre.last().unwrap()[2] - south.centre[0][2];
        assert!(rise >= 200.0, "pass rises {rise} m");
        let fall = north.centre[0][2] - north.centre.last().unwrap()[2];
        assert!(fall >= 150.0, "north face falls {fall} m");
    }

    #[test]
    fn mountain_roads_keep_their_grades_and_banks() {
        let (net, rep) = city();
        for s in &net.surfaces {
            let r = s.road as usize;
            if !MINE.contains(&rep.road_names[r]) {
                continue;
            }
            let cap = rep.road_grades[r];
            assert!(cap <= 0.12 + 1e-6);
            let cum = geom::cumulative(&s.centre);
            for k in 0..s.centre.len() - 1 {
                let ds = (cum[k + 1] - cum[k]).max(0.5);
                let g = (s.centre[k + 1][2] - s.centre[k][2]) / ds;
                assert!(
                    g.abs() <= cap + 0.005,
                    "{} grade {g:.3} at {:?}",
                    rep.road_names[r],
                    s.centre[k]
                );
                assert!(s.bank[k].abs() <= 0.0601);
            }
        }
    }

    #[test]
    fn mountain_roads_sit_on_the_ground() {
        // Benches, not bridges: terrain never above the road, and the road almost nowhere
        // more than 3 m above it (a deck).
        let (net, rep) = city();
        let t = net.terrain.as_ref().unwrap();
        let (mut decks, mut total) = (0usize, 0usize);
        for s in &net.surfaces {
            if !MINE.contains(&rep.road_names[s.road as usize]) {
                continue;
            }
            for k in 0..s.centre.len() {
                let p = s.centre[k];
                total += 1;
                if is_elevated(s, k, &|q| t.height_at(q), 3.0) {
                    decks += 1;
                    continue;
                }
                for lat in [-s.left_m, 0.0, s.right_m] {
                    let road = s.z_at(k, lat);
                    let tan = if k + 1 < s.centre.len() {
                        geom::normalize(geom::sub(geom::xy(s.centre[k + 1]), geom::xy(p)))
                    } else {
                        geom::normalize(geom::sub(geom::xy(p), geom::xy(s.centre[k - 1])))
                    };
                    let q = geom::add(geom::xy(p), geom::scale(geom::right_of(tan), lat));
                    let g = t.height_at(q);
                    assert!(
                        g <= road + 0.03,
                        "terrain {g} above road {road} at {q:?} on {}",
                        rep.road_names[s.road as usize]
                    );
                }
            }
        }
        assert!(total > 2000);
        assert!(decks * 100 < total, "{decks} of {total} points on decks");
    }

    #[test]
    fn observatory_reachable_and_pois_placed() {
        let (net, rep) = city();
        let map = crate::CityMap::new(net.clone());
        let bad = super::super::tests::unreachable_lanes(&map, [0.0, -50.0]);
        for s in MINE {
            let lanes: Vec<u32> = (0..net.lanes.len() as u32)
                .filter(|l| rep.road_names[net.lanes[*l as usize].road as usize] == s)
                .collect();
            assert!(!lanes.is_empty(), "{s} has lanes");
            assert!(
                lanes.iter().all(|l| !bad.contains(l)),
                "{s} not strongly connected"
            );
        }
        let poi = |name: &str, kind: PoiKind| {
            net.pois
                .iter()
                .find(|p| p.name == name && p.kind == kind)
                .unwrap_or_else(|| panic!("no {name}"))
                .clone()
        };
        let ascent = poi(plan::POI_CROWFOOT_ASCENT, PoiKind::RaceStartGrid);
        let summit = poi(plan::POI_CROWFOOT_SUMMIT, PoiKind::RaceStartGrid);
        let foot = poi(plan::POI_CROWFOOT_NORTH, PoiKind::RaceStartGrid);
        let obs = poi(plan::POI_OBSERVATORY, PoiKind::RaceStartGrid);
        poi(plan::POI_OBSERVATORY, PoiKind::SafehouseCandidate);
        assert!(geom::dist(geom::xy(ascent.pos), plan::R40N_END) < 40.0);
        assert!(geom::dist(geom::xy(summit.pos), plan::CROWFOOT_SADDLE) < 60.0);
        let crowfoot = terminal("Crowfoot Interchange");
        assert!(geom::dist(geom::xy(foot.pos), crowfoot) < 60.0);
        assert!(geom::dist(geom::xy(obs.pos), plan::ASHPEAK_SUMMIT) < 90.0);
        // Ascent uphill (north), summit grid downhill (north), north foot uphill (south).
        let dir = |p: &Poi| geom::dir_of_yaw(p.heading_rad);
        assert!(dir(&ascent)[1] > 0.3, "ascent grid faces north");
        assert!(dir(&summit)[1] > 0.3, "summit grid faces north");
        assert!(dir(&foot)[1] < -0.3, "north foot grid faces south");
        assert!(obs.pos[2] > summit.pos[2] + 150.0);
    }

    #[test]
    fn landmarks_forest_and_zone() {
        let (net, _) = city();
        let landmark_within = |c: V2, r: f32| {
            net.buildings
                .iter()
                .filter(|b| {
                    b.kind == BuildingKind::Landmark
                        && b.footprint.iter().all(|q| geom::dist(*q, c) < r)
                })
                .count()
        };
        assert_eq!(
            landmark_within(plan::ASHPEAK_SUMMIT, 25.0),
            2,
            "observatory"
        );
        let letters = landmark_within(SIGN_CENTRE, 130.0);
        assert!(letters >= 40, "{letters} letter slabs");
        let cabins = net
            .buildings
            .iter()
            .filter(|b| b.kind == BuildingKind::House && b.roof == RoofShape::Gable)
            .filter(|b| geom::point_in_polygon(b.footprint[0], &range_area()))
            // Farmsteads on the foothill farms reach just into the range's area.
            .filter(|b| b.footprint[0][1] >= plan::RANGE_SOUTH_Y)
            .count();
        assert!((3..=6).contains(&cabins), "{cabins} cabins");
        let pines = net
            .decor
            .iter()
            .filter(|d| d.kind == DecorKind::PineTree)
            .count();
        assert!((2000..=6000).contains(&pines), "{pines} pines");
        assert!(net.decor.iter().any(|d| d.kind == DecorKind::JerseyBarrier));
        let zone = net
            .zones
            .iter()
            .find(|z| geom::point_in_polygon(plan::CROWFOOT_SADDLE, &z.polygon))
            .unwrap();
        assert_eq!(zone.kind, DistrictKind::Suburb);
        assert!(zone.density_base < default_density_base(DistrictKind::Suburb) * 0.5);
    }
}
