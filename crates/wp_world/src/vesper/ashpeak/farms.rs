//! Harlan Flats: farmland west of Port Vesper (and the north-west farm valley beyond the
//! range).
//!
//! - **Harlan Road** carries Route 40 West on from Port Vesper's west edge
//!   ([`plan::R40W_END`]) over the Sable River, through the village of **Harlan Crossing**
//!   ([`plan::HARLAN_CROSSING`]) to Pell Road in the far west. Two asphalt links (also named
//!   Harlan Road) join it to the Route 12 interchange terminals ([`plan::INTERCHANGES`]).
//! - **Grange Road** (north–south through the village) and **Millbrook Road** (north–south
//!   east of the river; it crosses Route 12 on a bridge) are the other county roads.
//! - **Section-line gravel roads** ([`RoadMaterial::Gravel`]) in a loose grid, **dirt field
//!   tracks** ([`RoadMaterial::Dirt`]) looping along field edges, the **Sable Ford Track**
//!   across the river and the **Coyote Hollow Trail**, a twisty dirt rally trail along the
//!   western edge (the start of the Harlan Farm Run).
//! - **Farmsteads** along the gravel roads: farmhouse, red barn, silos, sheds, a fenced yard,
//!   a wind-break tree row and hay bales on the fields beyond.
//! - **North-west valley**: Cold Spring Road west from the Ashpeak West interchange with its
//!   own gravel grid and farmsteads.

use super::super::*;
use super::plan;

// ---- Road styles ----------------------------------------------------------------------------

/// Harlan Road out in the country: two-lane asphalt county road, 55 mph.
const HARLAN: Style = Style {
    class: RoadClass::Collector,
    fwd: 1,
    bwd: 1,
    speed: 24.586_72, // 55 mph
    parking: false,
    grade: 0.08,
    bank_max: 0.03,
    material: RoadMaterial::Asphalt,
};
/// Other county roads (Grange, Millbrook, Cold Spring, the interchange links): 50 mph.
const COUNTY: Style = Style {
    speed: 22.352, // 50 mph
    ..HARLAN
};
/// Harlan Road and Grange Road through the village: 35 mph, kerbside parking.
const VILLAGE_MAIN: Style = Style {
    speed: 15.6464, // 35 mph
    parking: true,
    bank_max: 0.0,
    ..HARLAN
};
/// Village side streets.
const VILLAGE: Style = Style {
    class: RoadClass::Local,
    fwd: 1,
    bwd: 1,
    speed: 11.176, // 25 mph
    parking: true,
    grade: 0.10,
    bank_max: 0.0,
    material: RoadMaterial::Asphalt,
};
/// Section-line gravel road, 40 mph.
const GRAVEL: Style = Style {
    class: RoadClass::Local,
    fwd: 1,
    bwd: 1,
    speed: 17.8816, // 40 mph
    parking: false,
    grade: 0.12,
    bank_max: 0.0,
    material: RoadMaterial::Gravel,
};
/// Dirt field track / rally trail (just above the verge-tree speed: no street trees).
const DIRT: Style = Style {
    speed: 14.0, // ≈ 31 mph
    grade: 0.15,
    material: RoadMaterial::Dirt,
    ..GRAVEL
};

// ---- Names ----------------------------------------------------------------------------------

/// Harlan Road (also the interchange links).
pub(crate) const HARLAN_ROAD: &str = "Harlan Road";
/// County road north–south through Harlan Crossing.
pub(crate) const GRANGE_ROAD: &str = "Grange Road";
/// County road north–south east of the Sable River (gravel south of Harlan Road).
pub(crate) const MILLBROOK_ROAD: &str = "Millbrook Road";
/// County road west from the Ashpeak West interchange.
pub(crate) const COLD_SPRING_ROAD: &str = "Cold Spring Road";
/// The dirt rally trail along the western edge.
pub(crate) const RALLY_TRAIL: &str = "Coyote Hollow Trail";
/// The dirt track fording the Sable River.
pub(crate) const FORD_TRACK: &str = "Sable Ford Track";

// ---- Areas ----------------------------------------------------------------------------------

/// Harlan Crossing village (generic street-front buildings allowed), m.
const VILLAGE_LO: V2 = [-3640.0, -180.0];
/// Harlan Crossing village, upper corner, m.
const VILLAGE_HI: V2 = [-2980.0, 480.0];
/// Village grid columns (Grange Road is the middle one), m.
const VX: [f32; 5] = [-3560.0, -3430.0, -3300.0, -3170.0, -3040.0];
/// Village grid rows (Harlan Road is the middle one), m.
const VY: [f32; 5] = [-110.0, 20.0, 150.0, 280.0, 410.0];
/// The rally trail's north end (on Pell Road), m.
const TRAIL_START: V2 = [-4300.0, 1700.0];

/// Southern farmland (Harlan Flats proper).
fn in_flats(p: V2) -> bool {
    p[0] < plan::FARMS_EAST_X && p[1] < plan::RANGE_SOUTH_Y && p[1] > plan::LO[1]
}

/// The north-west farm valley (west of Route 12).
fn nw_valley() -> Vec<V2> {
    vec![
        [plan::LO[0], 3500.0],
        [-2700.0, 3500.0],
        [-2420.0, 3950.0],
        [-2230.0, 4700.0],
        [-2180.0, plan::HI[1]],
        [plan::LO[0], plan::HI[1]],
    ]
}

fn in_nw_valley(p: V2) -> bool {
    geom::point_in_polygon(p, &nw_valley())
}

fn in_village(p: V2) -> bool {
    (VILLAGE_LO[0]..=VILLAGE_HI[0]).contains(&p[0])
        && (VILLAGE_LO[1]..=VILLAGE_HI[1]).contains(&p[1])
}

/// Whether `p` is this district's.
fn in_district(p: V2) -> bool {
    in_flats(p) || in_nw_valley(p)
}

/// Distance from `p` to the Route 12 corridor centreline, m.
fn route12_dist(p: V2) -> f32 {
    plan::ROUTE_12
        .windows(2)
        .map(|w| geom::point_segment_distance(p, w[0], w[1]))
        .fold(f32::INFINITY, f32::min)
}

// ---- Layout ---------------------------------------------------------------------------------

/// A way point: a junction (a node other roads may share) or a shape point.
#[derive(Debug, Clone, Copy)]
enum W {
    J(V2),
    S(V2),
}
use W::{J, S};

/// Lay a curved road through `pts` (Catmull-Rom sampled every `step` m), split into roads at
/// each junction point (the ends always are); each piece's style is `style` at its middle.
/// Returns the node per junction, in order.
fn way(
    g: &mut Gen,
    pts: &[W],
    step: f32,
    style: &dyn Fn(V2) -> Style,
    name: &'static str,
) -> Vec<usize> {
    let ctrl: Vec<V2> = pts
        .iter()
        .map(|w| match w {
            J(p) | S(p) => *p,
        })
        .collect();
    let curve = catmull_rom(&ctrl, step);
    // Sample index of every control point (the curve passes through them).
    let mut idx = vec![0usize];
    for w in ctrl.windows(2) {
        let m = ((geom::dist(w[0], w[1]) / step).ceil() as usize).max(1);
        idx.push(idx[idx.len() - 1] + m);
    }
    debug_assert_eq!(idx[idx.len() - 1] + 1, curve.len());
    let last = pts.len() - 1;
    let js: Vec<usize> = (0..pts.len())
        .filter(|k| *k == 0 || *k == last || matches!(pts[*k], J(_)))
        .collect();
    let nodes: Vec<usize> = js.iter().map(|k| g.node(ctrl[*k])).collect();
    for w in 0..js.len() - 1 {
        let (k0, k1) = (js[w], js[w + 1]);
        let mut path = curve[idx[k0]..=idx[k1]].to_vec();
        path[0] = ctrl[k0];
        let n = path.len();
        path[n - 1] = ctrl[k1];
        let mid = path[n / 2];
        g.road(nodes[w], nodes[w + 1], path, style(mid), name);
    }
    nodes
}

/// Constant style.
fn fixed(st: Style) -> impl Fn(V2) -> Style {
    move |_| st
}

/// County style, slowed to [`VILLAGE_MAIN`] inside the village.
fn county_or_village(st: Style) -> impl Fn(V2) -> Style {
    move |p| if in_village(p) { VILLAGE_MAIN } else { st }
}

/// Lay out this district's roads.
pub(super) fn layout(g: &mut Gen) {
    let ic = |name: &str| {
        plan::INTERCHANGES
            .iter()
            .find(|i| i.name == name)
            .copied()
            .expect("planned interchange")
    };
    let harlan_ic = ic("Harlan Road");
    let west_ic = ic("Ashpeak West");

    // ---- Harlan Road: Route 40 West's dead end → Harlan Crossing → Pell Road ------------
    let mut harlan = vec![
        J(plan::R40W_END),
        S([-1820.0, 82.0]),
        J([-2050.0, 105.0]), // Millbrook Road
        S([-2290.0, 150.0]),
        S([-2480.0, 190.0]), // over the Sable River
        J([-2780.0, 175.0]), // link to the interchange / Coulter Road
    ];
    harlan.extend(VX.iter().rev().map(|x| J([*x, VY[2]])));
    harlan.extend([
        J([-3700.0, 168.0]), // Harlan Feed & Tractor
        J([-3800.0, 182.0]), // Tolliver Road
        S([-4050.0, 215.0]),
        J([-4300.0, 230.0]), // Pell Road
    ]);
    way(g, &harlan, 8.0, &county_or_village(HARLAN), HARLAN_ROAD);
    // Links to the Route 12 interchange terminals.
    way(
        g,
        &[
            J([-2780.0, 175.0]),
            S([-2765.0, 450.0]),
            J([-2740.0, 700.0]), // Wexler Road
            S([-2700.0, 900.0]),
            J(harlan_ic.a),
        ],
        8.0,
        &fixed(COUNTY),
        HARLAN_ROAD,
    );
    way(
        g,
        &[J(harlan_ic.b), S([-2200.0, 1060.0]), J([-2072.0, 1062.0])],
        8.0,
        &fixed(COUNTY),
        HARLAN_ROAD,
    );

    // ---- County roads north–south -----------------------------------------------------
    let mut grange = vec![
        J([-3300.0, -850.0]), // Shoreline Road
        S([-3315.0, -600.0]),
        J([-3300.0, -350.0]), // Dutton Line
        S([-3290.0, -230.0]),
    ];
    grange.extend(VY.iter().map(|y| J([VX[2], *y])));
    grange.extend([
        S([-3285.0, 560.0]),
        J([-3290.0, 700.0]), // Wexler Road
        S([-3310.0, 980.0]),
        J([-3300.0, 1250.0]), // Hobb Road
        S([-3305.0, 1480.0]),
        J([-3320.0, 1700.0]), // Ridgefoot Road
    ]);
    way(g, &grange, 8.0, &county_or_village(COUNTY), GRANGE_ROAD);
    // Millbrook Road: gravel south of Harlan Road, asphalt north of it (over Route 12).
    way(
        g,
        &[
            J([-2050.0, -800.0]), // Shoreline Road
            S([-2060.0, -560.0]),
            J([-2050.0, -320.0]), // Sable Ford Track
            S([-2040.0, -100.0]),
            J([-2050.0, 105.0]),
        ],
        8.0,
        &fixed(GRAVEL),
        MILLBROOK_ROAD,
    );
    way(
        g,
        &[
            J([-2050.0, 105.0]),
            S([-2030.0, 350.0]),
            S([-2005.0, 585.0]), // bridge over Route 12
            S([-2035.0, 820.0]),
            J([-2072.0, 1062.0]), // link from the interchange
            J([-2090.0, 1170.0]), // field track
            J([-2112.0, 1380.0]), // field track
            J([-2128.0, 1450.0]), // Kestrel Road
            S([-2150.0, 1620.0]),
            J([-2180.0, 1770.0]), // Foothill Road
        ],
        8.0,
        &fixed(COUNTY),
        MILLBROOK_ROAD,
    );

    // ---- Section-line gravel roads -----------------------------------------------------
    let gravel = fixed(GRAVEL);
    let gravel_ways: [(&[W], &'static str); 12] = [
        (
            &[
                J([-4300.0, -800.0]),
                J([-4150.0, -808.0]), // field track
                J([-3950.0, -822.0]), // field track
                J([-3800.0, -830.0]),
                S([-3550.0, -845.0]),
                J([-3300.0, -850.0]),
                S([-3040.0, -830.0]),
                J([-2780.0, -820.0]),
                S([-2640.0, -812.0]), // over the Sable River
                S([-2420.0, -805.0]),
                J([-2050.0, -800.0]),
            ],
            "Shoreline Road",
        ),
        (
            &[
                J([-4300.0, -800.0]),
                S([-4320.0, -560.0]),
                J([-4300.0, -350.0]),
                S([-4285.0, -60.0]),
                J([-4300.0, 230.0]),
                S([-4280.0, 460.0]),
                J([-4300.0, 700.0]),
                S([-4315.0, 980.0]),
                J([-4300.0, 1250.0]),
                S([-4290.0, 1480.0]),
                J(TRAIL_START),
            ],
            "Pell Road",
        ),
        (
            &[
                J([-3800.0, -830.0]),
                S([-3790.0, -590.0]),
                J([-3800.0, -350.0]),
                S([-3815.0, -80.0]),
                J([-3800.0, 182.0]),
                S([-3790.0, 440.0]),
                J([-3800.0, 700.0]),
                S([-3812.0, 980.0]),
                J([-3800.0, 1250.0]),
            ],
            "Tolliver Road",
        ),
        (
            &[
                J([-4300.0, -350.0]),
                S([-4050.0, -340.0]),
                J([-3800.0, -350.0]),
                S([-3550.0, -362.0]),
                J([-3300.0, -350.0]),
                S([-3040.0, -335.0]),
                J([-2780.0, -330.0]),
            ],
            "Dutton Line",
        ),
        (
            &[
                J([-2780.0, -820.0]),
                J([-2780.0, -650.0]), // field track
                J([-2782.0, -450.0]), // field track
                J([-2780.0, -330.0]),
                S([-2775.0, -80.0]),
                J([-2780.0, 175.0]),
            ],
            "Coulter Road",
        ),
        (
            &[
                J([-4300.0, 700.0]),
                S([-4050.0, 690.0]),
                J([-3800.0, 700.0]),
                J([-3650.0, 706.0]), // field track
                J([-3450.0, 708.0]), // field track
                J([-3290.0, 700.0]),
                S([-3010.0, 690.0]),
                J([-2740.0, 700.0]),
            ],
            "Wexler Road",
        ),
        (
            &[
                J([-4300.0, 1250.0]),
                S([-4050.0, 1262.0]),
                J([-3800.0, 1250.0]),
                S([-3550.0, 1240.0]),
                J([-3300.0, 1250.0]),
                S([-3130.0, 1252.0]),
                J([-2980.0, 1240.0]),
            ],
            "Hobb Road",
        ),
        (
            &[
                J(TRAIL_START),
                S([-4050.0, 1690.0]),
                S([-3800.0, 1705.0]),
                S([-3550.0, 1712.0]),
                J([-3320.0, 1700.0]),
            ],
            "Ridgefoot Road",
        ),
        (
            &[
                J([-2128.0, 1450.0]),
                S([-2300.0, 1462.0]),
                S([-2470.0, 1452.0]),
                J([-2620.0, 1450.0]),
            ],
            "Kestrel Road",
        ),
        (
            &[
                J([-2180.0, 1770.0]),
                S([-2350.0, 1760.0]),
                S([-2540.0, 1772.0]),
                J([-2700.0, 1760.0]),
            ],
            "Foothill Road",
        ),
        // North-west valley.
        (
            &[
                J([-4300.0, 3850.0]),
                S([-4000.0, 3862.0]),
                J([-3700.0, 3850.0]),
                S([-3320.0, 3838.0]),
                J([-2950.0, 3850.0]),
            ],
            "Aspen Line",
        ),
        (
            &[
                J([-4300.0, 5300.0]),
                S([-4000.0, 5290.0]),
                J([-3700.0, 5300.0]),
                J([-3450.0, 5306.0]), // field track
                J([-3200.0, 5304.0]), // field track
                J([-2950.0, 5300.0]),
                S([-2700.0, 5312.0]),
                J([-2450.0, 5320.0]),
            ],
            "Harrow Line",
        ),
    ];
    for (pts, name) in gravel_ways {
        way(g, pts, 8.0, &gravel, name);
    }

    // ---- North-west valley: Cold Spring Road and its section lines ---------------------
    way(
        g,
        &[
            J(west_ic.a),
            S([-2560.0, 4480.0]),
            S([-2760.0, 4505.0]),
            J([-2950.0, 4520.0]), // Larch Road
            S([-3320.0, 4548.0]),
            J([-3700.0, 4560.0]), // Fenwick Road
            S([-4000.0, 4535.0]),
            J([-4300.0, 4500.0]), // Bramble Road
        ],
        8.0,
        &fixed(COUNTY),
        COLD_SPRING_ROAD,
    );
    for (x, name) in [
        (-2950.0, "Larch Road"),
        (-3700.0, "Fenwick Road"),
        (-4300.0, "Bramble Road"),
    ] {
        let mid = if x == -2950.0 {
            4520.0
        } else if x == -3700.0 {
            4560.0
        } else {
            4500.0
        };
        way(
            g,
            &[
                J([x, 3850.0]),
                S([x + 12.0, 4180.0]),
                J([x, mid]),
                S([x - 12.0, 4900.0]),
                J([x, 5300.0]),
            ],
            8.0,
            &gravel,
            name,
        );
    }

    // ---- Dirt field tracks: loops off the section lines along field edges ---------------
    let dirt = fixed(DIRT);
    let loops: [(&[W], &'static str); 5] = [
        (
            &[
                J([-4150.0, -808.0]),
                S([-4160.0, -620.0]),
                S([-4070.0, -540.0]),
                S([-3960.0, -610.0]),
                J([-3950.0, -822.0]),
            ],
            "Shoreline Field Track",
        ),
        (
            &[
                J([-2780.0, -650.0]),
                S([-2960.0, -660.0]),
                S([-3080.0, -560.0]),
                S([-2960.0, -455.0]),
                J([-2782.0, -450.0]),
            ],
            "Coulter Field Track",
        ),
        (
            &[
                J([-3650.0, 706.0]),
                S([-3660.0, 900.0]),
                S([-3550.0, 1010.0]),
                S([-3440.0, 900.0]),
                J([-3450.0, 708.0]),
            ],
            "Wexler Field Track",
        ),
        (
            &[
                J([-2090.0, 1170.0]),
                S([-2300.0, 1175.0]),
                S([-2420.0, 1280.0]),
                S([-2300.0, 1370.0]),
                J([-2112.0, 1380.0]),
            ],
            "Millbrook Field Track",
        ),
        (
            &[
                J([-3450.0, 5306.0]),
                S([-3460.0, 5120.0]),
                S([-3330.0, 5030.0]),
                S([-3200.0, 5120.0]),
                J([-3200.0, 5304.0]),
            ],
            "Harrow Field Track",
        ),
    ];
    for (pts, name) in loops {
        way(g, pts, 6.0, &dirt, name);
    }
    // The ford: a dirt track across the Sable River between Coulter and Millbrook Roads.
    way(
        g,
        &[
            J([-2780.0, -330.0]),
            S([-2620.0, -300.0]),
            S([-2500.0, -335.0]),
            S([-2330.0, -300.0]),
            S([-2200.0, -330.0]),
            J([-2050.0, -320.0]),
        ],
        6.0,
        &dirt,
        FORD_TRACK,
    );

    // ---- Coyote Hollow Trail: the dirt rally trail along the western edge ---------------
    // From Ridgefoot Road's west end down the strip west of Pell Road, swinging left and
    // right every ~110 m (rolling with the ground), back onto Pell Road at the Dutton Line.
    let mut trail = vec![J(TRAIL_START), S([-4420.0, 1690.0])];
    let mut y = 1590.0;
    let mut k = 0;
    while y > -200.0 {
        let side = if k % 2 == 0 { 1.0 } else { -1.0 };
        // Mostly quick esses, now and then a wider sweeper or a tight hairpin-like flick.
        let roll = g.rng.next_f32();
        let (amp, gap) = if roll < 0.2 {
            (g.rng.range_f32(30.0, 45.0), g.rng.range_f32(140.0, 180.0))
        } else if roll < 0.35 {
            (g.rng.range_f32(80.0, 92.0), g.rng.range_f32(75.0, 90.0))
        } else {
            (g.rng.range_f32(50.0, 80.0), g.rng.range_f32(95.0, 125.0))
        };
        trail.push(S([-4470.0 + side * amp, y]));
        y -= gap;
        k += 1;
    }
    trail.push(S([-4440.0, -320.0]));
    trail.push(J([-4300.0, -350.0]));
    way(g, &trail, 5.0, &dirt, RALLY_TRAIL);

    // ---- Harlan Crossing: the village grid around Harlan Road × Grange Road --------------
    let names_row = [
        "Elm Street",
        "Mill Street",
        "",
        "Church Street",
        "Depot Street",
    ];
    let names_col = [
        "First Avenue",
        "Second Avenue",
        "",
        "Fourth Avenue",
        "Fifth Avenue",
    ];
    for (j, y) in VY.iter().enumerate() {
        if j == 2 {
            continue;
        }
        for i in 0..VX.len() - 1 {
            let a = g.node([VX[i], *y]);
            let b = g.node([VX[i + 1], *y]);
            g.link(a, b, 0.0, VILLAGE, names_row[j]);
        }
    }
    for (i, x) in VX.iter().enumerate() {
        if i == 2 {
            continue;
        }
        for j in 0..VY.len() - 1 {
            let a = g.node([*x, VY[j]]);
            let b = g.node([*x, VY[j + 1]]);
            g.link(a, b, 0.0, VILLAGE, names_col[i]);
        }
    }
}

/// Traffic density of the farmland relative to the default Suburb density.
const FARM_DENSITY: f32 = 0.3;

/// This district's zones.
pub(super) fn zones() -> Vec<Zone> {
    let z = |kind: DistrictKind, scale: f32, polygon: Vec<V2>| Zone {
        kind,
        density_base: default_density_base(kind) * scale,
        polygon,
    };
    vec![
        // The village: a small commercial core on the crossroads, houses around it.
        z(
            DistrictKind::Commercial,
            0.5,
            rect(-3445.0, 85.0, -3155.0, 215.0),
        ),
        z(
            DistrictKind::Residential,
            0.6,
            rect(VILLAGE_LO[0], VILLAGE_LO[1], VILLAGE_HI[0], VILLAGE_HI[1]),
        ),
        z(
            DistrictKind::Suburb,
            FARM_DENSITY,
            rect(
                plan::LO[0],
                plan::LO[1],
                plan::FARMS_EAST_X,
                plan::RANGE_SOUTH_Y,
            ),
        ),
        z(DistrictKind::Suburb, FARM_DENSITY, nw_valley()),
    ]
}

/// Whether the generic street-front building pass may build at `p`: only in the village (the
/// farms build their own farmsteads).
pub(super) fn generic_buildings_at(p: V2) -> bool {
    in_village(p) || !in_district(p)
}

// ---- Buildings ------------------------------------------------------------------------------

/// Rectangle footprint (counter-clockwise) centred on `c`, `hl` along `f`, `hw` across.
fn rect_fp(c: V2, f: V2, hl: f32, hw: f32) -> Vec<V2> {
    let r = geom::right_of(f);
    let at = |u: f32, v: f32| geom::add(c, geom::add(geom::scale(f, u), geom::scale(r, v)));
    let mut fp = vec![at(-hl, -hw), at(hl, -hw), at(hl, hw), at(-hl, hw)];
    if signed_area(&fp) < 0.0 {
        fp.reverse();
    }
    fp
}

/// Round footprint (12 points, counter-clockwise).
fn round_fp(c: V2, r: f32) -> Vec<V2> {
    (0..12)
        .map(|k| {
            let (s, co) = wp_math::sin_cos(std::f32::consts::TAU * k as f32 / 12.0);
            [c[0] + r * co, c[1] + r * s]
        })
        .collect()
}

/// Whether a building / decor point is clear: off every road by `clear`, off the freeway
/// corridors, inside the map.
fn clear_at(site: &Site, p: V2, clear: f32) -> bool {
    site.clearance(p) > clear
        && route12_dist(p) > plan::FREEWAY_CLEAR_M + 15.0
        && p[0] > plan::LO[0] + 30.0
        && p[1] < plan::HI[1] - 30.0
}

/// A building on footprint `fp` standing on the ground (walls `wall` m above the highest
/// ground under it), or `None` where it is not clear of the roads or the ground is too steep.
fn place(
    site: &Site,
    fp: Vec<V2>,
    wall: f32,
    kind: BuildingKind,
    roof: RoofShape,
    rise: f32,
) -> Option<Building> {
    let mut probes = fp.clone();
    let n = fp.len() as f32;
    probes.push([
        fp.iter().map(|p| p[0]).sum::<f32>() / n,
        fp.iter().map(|p| p[1]).sum::<f32>() / n,
    ]);
    if !probes.iter().all(|p| clear_at(site, *p, 4.0)) {
        return None;
    }
    let zs: Vec<f32> = probes.iter().map(|p| site.ground(*p)).collect();
    let lo = zs.iter().copied().fold(f32::INFINITY, f32::min);
    let hi = zs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if hi - lo > 3.5 || lo < 1.0 {
        return None;
    }
    let base = lo - 0.3;
    Some(Building {
        footprint: fp,
        height_m: wall + (hi - base),
        base_z_m: base,
        kind,
        roof,
        roof_rise_m: rise,
    })
}

/// A decor box on the ground at `p`.
fn decor(site: &Site, kind: DecorKind, p: V2, yaw: f32, size: [f32; 3]) -> Decor {
    Decor {
        kind,
        pos: [p[0], p[1], site.ground(p)],
        yaw,
        size,
    }
}

/// Fence pieces (≈ 9 m) along `a`–`b`, skipping those not clear of the roads.
fn fence(site: &Site, a: V2, b: V2, out: &mut Vec<Decor>) {
    let len = geom::dist(a, b);
    let n = (len / 9.0).ceil().max(1.0) as usize;
    let d = geom::normalize(geom::sub(b, a));
    let piece = len / n as f32;
    for k in 0..n {
        let c = geom::add(a, geom::scale(d, piece * (k as f32 + 0.5)));
        let ends = [
            geom::add(c, geom::scale(d, -piece * 0.5)),
            geom::add(c, geom::scale(d, piece * 0.5)),
        ];
        if ends.iter().all(|e| clear_at(site, *e, 4.0)) {
            out.push(decor(
                site,
                DecorKind::Fence,
                c,
                geom::yaw_of(d),
                [0.12, piece - 0.2, 1.2],
            ));
        }
    }
}

/// One farmstead beside a road: `q` on the centreline, `t` along the road, `n` away from it
/// (towards the farm). Returns its buildings and decor, or `None` where the house and barn do
/// not fit.
fn farmstead(
    site: &mut Site,
    q: V2,
    t: V2,
    n: V2,
    others: &[V2],
) -> Option<(Vec<Building>, Vec<Decor>)> {
    let at = |u: f32, v: f32| geom::add(q, geom::add(geom::scale(t, u), geom::scale(n, v)));
    let house = place(
        site,
        rect_fp(at(-24.0, 34.0), t, 7.5, 5.5),
        5.8,
        BuildingKind::House,
        RoofShape::Gable,
        3.2,
    )?;
    let barn = place(
        site,
        rect_fp(at(16.0, 54.0), n, 15.0, 8.5),
        7.5,
        BuildingKind::Farm,
        RoofShape::Gable,
        5.5,
    )?;
    let mut b = vec![house, barn];
    // Silos beside the barn.
    let silos = 1 + site.rng.below(3);
    for k in 0..silos {
        let r = site.rng.range_f32(3.5, 4.5);
        let h = site.rng.range_f32(18.0, 25.0);
        if let Some(s) = place(
            site,
            round_fp(at(36.0 + 10.0 * k as f32, 40.0), r),
            h,
            BuildingKind::Silo,
            RoofShape::Flat,
            0.0,
        ) {
            b.push(s);
        }
    }
    // Sheds.
    if let Some(s) = place(
        site,
        rect_fp(at(-26.0, 72.0), t, 7.0, 4.0),
        3.6,
        BuildingKind::Farm,
        RoofShape::Gable,
        1.6,
    ) {
        b.push(s);
    }
    if site.rng.chance(0.6) {
        if let Some(s) = place(
            site,
            rect_fp(at(46.0, 76.0), t, 10.0, 6.0),
            4.5,
            BuildingKind::Farm,
            RoofShape::Gable,
            1.8,
        ) {
            b.push(s);
        }
    }
    let mut d = Vec::new();
    // Fenced yard, open at the drive.
    let (u0, u1, v0, v1) = (-46.0, 64.0, 20.0, 92.0);
    fence(site, at(u0, v0), at(-9.0, v0), &mut d);
    fence(site, at(5.0, v0), at(u1, v0), &mut d);
    fence(site, at(u1, v0), at(u1, v1), &mut d);
    fence(site, at(u1, v1), at(u0, v1), &mut d);
    fence(site, at(u0, v1), at(u0, v0), &mut d);
    // Wind-break tree row behind the yard.
    let mut u = u0;
    while u <= u1 {
        let p = at(u + site.rng.range_f32(-1.5, 1.5), 102.0);
        let crown = site.rng.range_f32(6.0, 8.5);
        let h = site.rng.range_f32(10.0, 14.0);
        let yaw = site.rng.range_f32(0.0, std::f32::consts::TAU);
        if clear_at(site, p, 5.0) {
            d.push(decor(site, DecorKind::YardTree, p, yaw, [crown, crown, h]));
        }
        u += 10.0;
    }
    // Hay bales on the field beyond.
    if site.rng.chance(0.8) {
        let yard = at(10.0, 56.0);
        for row in 0..3 {
            for i in 0..8 {
                let p = at(
                    -60.0 + 16.0 * i as f32 + site.rng.range_f32(-3.0, 3.0),
                    135.0 + 38.0 * row as f32 + site.rng.range_f32(-4.0, 4.0),
                );
                let yaw = site.rng.range_f32(0.0, std::f32::consts::TAU);
                let skip = site.rng.chance(0.25);
                let near_other = others
                    .iter()
                    .any(|o| geom::dist(*o, p) < 95.0 && geom::dist(*o, yard) > 1.0);
                if !skip && !near_other && clear_at(site, p, 6.0) {
                    d.push(decor(site, DecorKind::HayBale, p, yaw, [1.6, 1.3, 1.6]));
                }
            }
        }
    }
    Some((b, d))
}

/// Village landmarks: church, grain elevator, sheriff station, feed store; the Old Mill Barn
/// by the river.
fn village_landmarks(site: &mut Site) -> (Vec<Building>, Vec<Decor>) {
    let mut b = Vec::new();
    let e = [1.0, 0.0];
    let n = [0.0, 1.0];
    let flat = RoofShape::Flat;
    let gable = RoofShape::Gable;
    // Harlan Community Church: a gabled nave and its steeple, on Church Street's block.
    let specs: [(Vec<V2>, f32, BuildingKind, RoofShape, f32); 12] = [
        (
            rect_fp([-3365.0, 205.0], n, 14.0, 7.5),
            8.0,
            BuildingKind::Landmark,
            gable,
            6.0,
        ),
        (
            rect_fp([-3365.0, 226.0], n, 3.0, 3.0),
            30.0,
            BuildingKind::Landmark,
            flat,
            0.0,
        ),
        // Harlan Grain Elevator: a row of tall bins and the head house, east of the village.
        (
            round_fp([-2985.0, 245.0], 5.5),
            34.0,
            BuildingKind::Silo,
            flat,
            0.0,
        ),
        (
            round_fp([-2973.0, 245.0], 5.5),
            34.0,
            BuildingKind::Silo,
            flat,
            0.0,
        ),
        (
            round_fp([-2961.0, 245.0], 5.5),
            34.0,
            BuildingKind::Silo,
            flat,
            0.0,
        ),
        (
            round_fp([-2949.0, 245.0], 5.5),
            34.0,
            BuildingKind::Silo,
            flat,
            0.0,
        ),
        (
            rect_fp([-3000.0, 245.0], n, 5.0, 5.0),
            46.0,
            BuildingKind::Farm,
            flat,
            0.0,
        ),
        // Ashpeak Sheriff Harlan Substation.
        (
            rect_fp([-3495.0, 90.0], e, 14.0, 9.0),
            7.0,
            BuildingKind::Block,
            flat,
            0.0,
        ),
        // Harlan Feed & Tractor: big store shed and a machinery shed.
        (
            rect_fp([-3700.0, 110.0], e, 18.0, 11.0),
            7.5,
            BuildingKind::Farm,
            gable,
            3.0,
        ),
        (
            rect_fp([-3700.0, 70.0], e, 14.0, 8.0),
            6.0,
            BuildingKind::Farm,
            gable,
            2.0,
        ),
        // The Old Mill Barn by the Sable River ford.
        (
            rect_fp([-2560.0, -235.0], n, 16.0, 10.0),
            9.0,
            BuildingKind::Farm,
            gable,
            6.0,
        ),
        (
            round_fp([-2535.0, -218.0], 4.0),
            20.0,
            BuildingKind::Silo,
            flat,
            0.0,
        ),
    ];
    for (fp, wall, kind, roof, rise) in specs {
        if let Some(x) = place(site, fp, wall, kind, roof, rise) {
            b.push(x);
        }
    }
    (b, Vec::new())
}

/// Hand-placed buildings and decor: village landmarks and the farmsteads along the gravel
/// roads.
pub(super) fn extra_buildings(site: &mut Site) -> (Vec<Building>, Vec<Decor>) {
    let (mut b, mut d) = village_landmarks(site);
    // Farmstead sites: along every gravel road of the district, one per ~520 m.
    let mut cands: Vec<(V2, V2)> = Vec::new();
    for dr in site.drafts {
        if dr.st.material != RoadMaterial::Gravel {
            continue;
        }
        let cum = cum2(&dr.path);
        let len = cum[cum.len() - 1];
        if !in_district(point_on(&dr.path, &cum, len * 0.5).0) {
            continue;
        }
        let count = (len / 520.0).round() as usize;
        for k in 0..count {
            let (q, t) = point_on(&dr.path, &cum, len * (k as f32 + 0.5) / count as f32);
            cands.push((q, t));
        }
    }
    let mut farms: Vec<V2> = Vec::new();
    for (q, t) in cands {
        let first = if site.rng.chance(0.5) { 1.0 } else { -1.0 };
        for side in [first, -first] {
            let n = geom::scale(geom::right_of(t), side);
            let yard = geom::add(q, geom::scale(n, 56.0));
            if in_village(yard) || farms.iter().any(|f| geom::dist(*f, yard) < 260.0) {
                continue;
            }
            if let Some((fb, fd)) = farmstead(site, q, t, n, &farms) {
                farms.push(yard);
                b.extend(fb);
                d.extend(fd);
                break;
            }
        }
    }
    (b, d)
}

// ---- Points of interest ---------------------------------------------------------------------

/// Points of interest.
pub(super) fn pois(site: &mut Site) -> Vec<Poi> {
    let mut out = Vec::new();
    // The Harlan Farm Run: pole on the rally trail's first stretch, facing down the trail.
    if let Some(r) = site.road_named(RALLY_TRAIL, TRAIL_START, 20.0) {
        out.extend(site.lane_poi(PoiKind::RaceStartGrid, plan::POI_FARM_RUN, r, 30.0, true));
    }
    // Harlan Crossing: westbound on Harlan Road at the village's east edge.
    if let Some(r) = site.road_named(HARLAN_ROAD, [VX[4] - 10.0, VY[2]], 15.0) {
        out.extend(site.lane_poi(
            PoiKind::RaceStartGrid,
            plan::POI_HARLAN_CROSSING,
            r,
            10.0,
            true,
        ));
    }
    for (kind, name, p) in [
        (
            PoiKind::PoliceStation,
            "Ashpeak Sheriff Harlan Substation",
            [-3495.0, 120.0],
        ),
        (PoiKind::Garage, "Harlan Feed & Tractor", [-3700.0, 140.0]),
        (
            PoiKind::SafehouseCandidate,
            "Old Mill Barn",
            [-2560.0, -290.0],
        ),
    ] {
        out.extend(site.block_poi(kind, name, p));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_contracts::{SurfaceType, WorldQuery};

    fn city() -> &'static (RoadNetwork, VesperReport) {
        static CITY: std::sync::OnceLock<(RoadNetwork, VesperReport)> = std::sync::OnceLock::new();
        CITY.get_or_init(|| super::super::generate_with_report(&super::super::default_config()))
    }

    fn mine(s: &RoadSurface) -> bool {
        let c = &s.centre;
        in_district(geom::xy(c[c.len() / 2]))
    }

    fn length(s: &RoadSurface) -> f32 {
        s.centre
            .windows(2)
            .map(|w| geom::dist(geom::xy(w[0]), geom::xy(w[1])))
            .sum()
    }

    #[test]
    fn harlan_road_joins_route_40_the_village_and_the_interchange() {
        let (net, rep) = city();
        assert_eq!(rep.skipped_roads, 0);
        // Graph of Harlan Road's pieces (nodes joined by its lanes).
        let mut adj: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for l in &net.lanes {
            // Terminal b joins Harlan Road through Millbrook Road (the freeways' crossroad
            // between the terminals is also named Harlan Road).
            let name = rep.road_names[l.road as usize];
            if name == HARLAN_ROAD || name == MILLBROOK_ROAD {
                adj.entry(l.from_node).or_default().push(l.to_node);
            }
        }
        let node_at = |p: V2, reach: f32| {
            (0..net.nodes.len() as u32)
                .filter(|n| adj.contains_key(n))
                .find(|n| geom::dist(geom::xy(net.nodes[*n as usize].pos), p) < reach)
        };
        let start = node_at(plan::R40W_END, 2.0).expect("Harlan Road at Route 40 West");
        let mut seen = std::collections::BTreeSet::from([start]);
        let mut stack = vec![start];
        while let Some(n) = stack.pop() {
            for m in adj.get(&n).into_iter().flatten() {
                if seen.insert(*m) {
                    stack.push(*m);
                }
            }
        }
        let ic = plan::INTERCHANGES
            .iter()
            .find(|i| i.name == "Harlan Road")
            .unwrap();
        for (what, p, reach) in [
            ("terminal a", ic.a, 2.0),
            ("terminal b", ic.b, 2.0),
            ("Harlan Crossing", plan::HARLAN_CROSSING, 100.0),
            ("Pell Road", [-4300.0, 230.0], 2.0),
        ] {
            let n = node_at(p, reach).unwrap_or_else(|| panic!("no Harlan Road node at {what}"));
            assert!(seen.contains(&n), "{what} not reached along Harlan Road");
        }
    }

    #[test]
    fn unpaved_roads_gravel_and_dirt() {
        let (net, _) = city();
        let (mut gravel, mut dirt) = ((0, 0.0f32), (0, 0.0f32));
        for s in net.surfaces.iter().filter(|s| mine(s)) {
            match s.material {
                RoadMaterial::Gravel => {
                    gravel.0 += 1;
                    gravel.1 += length(s);
                }
                RoadMaterial::Dirt => {
                    dirt.0 += 1;
                    dirt.1 += length(s);
                }
                RoadMaterial::Asphalt => {}
            }
        }
        assert!(gravel.0 >= 40 && gravel.1 > 20_000.0, "gravel {gravel:?}");
        assert!(dirt.0 >= 7 && dirt.1 > 6_000.0, "dirt {dirt:?}");
        assert!(gravel.1 + dirt.1 > 15_000.0);
        // Wheels on an unpaved lane's centreline feel its material.
        let map = crate::CityMap::new(net.clone());
        for (m, want) in [
            (RoadMaterial::Gravel, SurfaceType::Gravel),
            (RoadMaterial::Dirt, SurfaceType::Dirt),
        ] {
            let s = net
                .surfaces
                .iter()
                .find(|s| s.material == m && mine(s) && length(s) > 200.0)
                .unwrap();
            let lane = net.lanes.iter().find(|l| l.road == s.road).unwrap();
            let p = lane.polyline[lane.polyline.len() / 2];
            assert_eq!(
                map.surface_at([p[0], p[1], p[2] + 0.05]),
                want,
                "{m:?} at {p:?}"
            );
        }
    }

    #[test]
    fn roads_keep_clear_of_route_12() {
        let (net, rep) = city();
        let terminals: Vec<V2> = plan::INTERCHANGES
            .iter()
            .filter(|i| i.freeway == "Route 12")
            .flat_map(|i| [i.a, i.b])
            .collect();
        for s in net.surfaces.iter().filter(|s| mine(s) && !s.highway) {
            let name = rep.road_names[s.road as usize];
            for p in &s.centre {
                let q = geom::xy(*p);
                let near_terminal = terminals.iter().any(|t| geom::dist(*t, q) < 200.0);
                assert!(
                    route12_dist(q) >= plan::FREEWAY_CLEAR_M
                        || near_terminal
                        || name == MILLBROOK_ROAD,
                    "{name} at {q:?} is {} m from Route 12",
                    route12_dist(q)
                );
            }
        }
    }

    #[test]
    fn farmsteads_have_barns_and_silos_off_the_roads() {
        let (net, _) = city();
        let farm = |b: &&Building| {
            let c = b.footprint[0];
            in_district(c) && !in_village(c)
        };
        let barns = net
            .buildings
            .iter()
            .filter(farm)
            .filter(|b| b.kind == BuildingKind::Farm && b.roof == RoofShape::Gable)
            .count();
        let silos = net
            .buildings
            .iter()
            .filter(farm)
            .filter(|b| b.kind == BuildingKind::Silo)
            .count();
        let houses = net
            .buildings
            .iter()
            .filter(farm)
            .filter(|b| b.kind == BuildingKind::House)
            .count();
        assert!((25..=45).contains(&houses), "{houses} farmhouses");
        assert!(
            barns >= houses && silos >= houses,
            "{barns} barns {silos} silos"
        );
        // No generic houses out on the farmland: one farmhouse per farmstead.
        let off_road = |c: V2| {
            net.surfaces.iter().all(|s| {
                let half = s.left_m.max(s.right_m) + 2.0;
                s.centre
                    .windows(2)
                    .all(|w| geom::point_segment_distance(c, geom::xy(w[0]), geom::xy(w[1])) > half)
            })
        };
        for b in net.buildings.iter().filter(farm) {
            for c in &b.footprint {
                assert!(off_road(*c), "{:?} corner {c:?} on a road", b.kind);
            }
        }
        let bales = net
            .decor
            .iter()
            .filter(|d| d.kind == DecorKind::HayBale && in_district(geom::xy(d.pos)))
            .count();
        assert!((300..=2500).contains(&bales), "{bales} hay bales");
        let all = net
            .decor
            .iter()
            .filter(|d| in_district(geom::xy(d.pos)))
            .count();
        assert!(all <= 6000, "{all} decor");
    }

    #[test]
    fn pois_exist() {
        let (net, rep) = city();
        let poi = |name: &str, kind: PoiKind| {
            net.pois
                .iter()
                .find(|p| p.name == name && p.kind == kind)
                .unwrap_or_else(|| panic!("no {name}"))
        };
        let run = poi(plan::POI_FARM_RUN, PoiKind::RaceStartGrid);
        let lane = &net.lanes[run.lane.unwrap() as usize];
        assert_eq!(rep.road_names[lane.road as usize], RALLY_TRAIL);
        let village = poi(plan::POI_HARLAN_CROSSING, PoiKind::RaceStartGrid);
        let lane = &net.lanes[village.lane.unwrap() as usize];
        assert_eq!(rep.road_names[lane.road as usize], HARLAN_ROAD);
        assert!(geom::dist(geom::xy(village.pos), plan::HARLAN_CROSSING) < 300.0);
        poi("Ashpeak Sheriff Harlan Substation", PoiKind::PoliceStation);
        poi("Harlan Feed & Tractor", PoiKind::Garage);
        poi("Old Mill Barn", PoiKind::SafehouseCandidate);
    }

    #[test]
    fn southern_farmland_reaches_port_vesper_without_route_12() {
        let (net, _) = city();
        let map = crate::CityMap::new(net.clone());
        let bad: Vec<u32> = super::super::tests::unreachable_lanes(&map, [0.0, -50.0])
            .into_iter()
            .filter(|l| in_flats(geom::xy(net.lanes[*l as usize].polyline[0])))
            .collect();
        assert!(
            bad.is_empty(),
            "{} farmland lanes not strongly connected, e.g. {:?}",
            bad.len(),
            bad.iter()
                .take(6)
                .map(|l| net.lanes[*l as usize].polyline[0])
                .collect::<Vec<_>>()
        );
    }
}
