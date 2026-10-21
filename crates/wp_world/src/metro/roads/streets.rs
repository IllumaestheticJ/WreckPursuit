//! Street layouts per district (CITY.md §2–3) and the named arterials of `plan.arterials`,
//! as raw [`Line`]s. Clipping (water, sites, rail, highways, interchanges) and
//! planarisation happen afterwards, so layouts may overshoot freely: a grid line runs 30 m
//! past its district edge so it meets the neighbour's boundary street, and stubs beyond
//! the last junction are trimmed by the graph cleanup.
//!
//! Districts:
//! - **Lantern Row**: 100 m grid of signalised avenues (collectors with parking) with
//!   arterial columns, Lantern Way's bus lanes, mid-block alleys in most core blocks.
//! - **Midtown**: wide one-way pairs (3-lane one-way avenues alternating direction) on a
//!   140 m grid, two-way cross streets.
//! - **Uptown**: 100 × 130 m grid continuing Lantern Row's columns, Founders Parkway.
//! - **Old Harbor**: tight 90–110 m cobbled grid between Kettle Street and Harbor Boulevard.
//! - **Mercer**: dense 120 m grid, parking on the cross streets, Mercer Street.
//! - **Eastgate**: 110 m grid climbing the bluff (steep cross streets).
//! - **Larkspur, Southmoor, Northfield, South Shore, Westridge Woods**: warped organic
//!   grids with dropped edges and cul-de-sacs.
//! - **Westline Yards, Canal Flats, Westline Outskirts**: 260–300 m industrial blocks with
//!   service roads; **Gantry Row / terminal**: port roads.
//! - **Hollins Heights**: contour roads, Hollins Drive (≈ 4 km, 11 hairpins to the Knob
//!   Observatory Lot, which also ends Skyline Drive), Skyline
//!   Drive through Westridge Woods, Bluff Road along the cliff.

use wp_math::Rng;

use super::geo::{bent, catmull_rom, fillet, resample};
use super::lines::Line;
use super::style::{self, Style};
use crate::builder::RoadClass;
use crate::geom::{self, V2};

/// Line collector with helpers.
pub struct Lay {
    /// Lines so far.
    pub lines: Vec<Line>,
    rng: Rng,
}

/// The Hollins Drive switchback path: a meander up the Knob's south-east face, legs
/// across the slope joined by hairpins (two 90° fillets each, ≈ 29 m radius), 11 hairpins.
pub fn hollins_path() -> Vec<V2> {
    let start: V2 = [-1900.0, 1965.0];
    let knob: V2 = [-2300.0, 2522.0];
    let u = geom::normalize(geom::sub(knob, start)); // uphill
    let v = [u[1], -u[0]]; // across the slope
    let legs = 12;
    let pitch = (geom::dist(start, knob) - 60.0) / legs as f32;
    let at = |a: f32, b: f32| geom::add(start, geom::add(geom::scale(u, a), geom::scale(v, b)));
    let mut ctrl = vec![start, at(30.0, 0.0)];
    for k in 0..legs {
        let side = if k % 2 == 0 { 1.0 } else { -1.0 };
        let w = 180.0 - 5.0 * k as f32;
        let y = 30.0 + pitch * k as f32;
        ctrl.push(at(y, side * w));
        ctrl.push(at(y + pitch, side * w));
    }
    ctrl.push(knob);
    let mut rad = vec![0.0; ctrl.len()];
    for r in rad.iter_mut().take(ctrl.len() - 1).skip(1) {
        *r = pitch * 0.5 - 0.5;
    }
    resample(&fillet(&ctrl, &rad, false, 2.0), 5.0)
}

impl Lay {
    fn new(seed: u64) -> Self {
        Self {
            lines: Vec::new(),
            rng: Rng::new(seed ^ 0x57ee_7a11),
        }
    }

    /// Add a polyline; returns its index.
    pub fn add(&mut self, path: Vec<V2>, st: Style, name: &'static str) -> usize {
        let mut l = Line::new(path, st, name);
        l.origin = self.lines.len();
        self.lines.push(l);
        self.lines.len() - 1
    }

    /// Straight line.
    fn seg(&mut self, a: V2, b: V2, st: Style, name: &'static str) -> usize {
        self.add(vec![a, b], st, name)
    }

    /// Smooth curve through control points.
    fn curve(&mut self, ctrl: &[V2], st: Style, name: &'static str) -> usize {
        let p = resample(&catmull_rom(ctrl, 6.0), 8.0);
        self.add(p, st, name)
    }

    /// Full-length grid lines: verticals at `xs` over `y0..y1`, horizontals at `ys` over
    /// `x0..x1` (both overshooting by `over`).
    #[allow(clippy::too_many_arguments)]
    fn grid(
        &mut self,
        xs: &[f32],
        ys: &[f32],
        x0: f32,
        x1: f32,
        y0: f32,
        y1: f32,
        over: f32,
        st_v: &dyn Fn(usize, f32) -> Style,
        st_h: &dyn Fn(usize, f32) -> Style,
        name: &'static str,
    ) {
        for (i, x) in xs.iter().enumerate() {
            let st = st_v(i, *x);
            let (a, b) = ([*x, y0 - over], [*x, y1 + over]);
            // One-way verticals alternate direction.
            if st.two_way() || i % 2 == 0 {
                self.seg(a, b, st, name);
            } else {
                self.seg(b, a, st, name);
            }
        }
        for (j, y) in ys.iter().enumerate() {
            let st = st_h(j, *y);
            let (a, b) = ([x0 - over, *y], [x1 + over, *y]);
            if st.two_way() || j % 2 == 0 {
                self.seg(a, b, st, name);
            } else {
                self.seg(b, a, st, name);
            }
        }
    }

    /// Mid-block alleys: in each block of the grid (`xs` × `ys`) with probability `p`, an
    /// east-west alley half-way between the rows, overshooting the columns by 6 m.
    fn alleys(&mut self, xs: &[f32], ys: &[f32], p: f32, name: &'static str) {
        for j in 0..ys.len().saturating_sub(1) {
            for i in 0..xs.len().saturating_sub(1) {
                if self.rng.next_f32() >= p {
                    continue;
                }
                let y = 0.5 * (ys[j] + ys[j + 1]);
                self.seg([xs[i] - 6.0, y], [xs[i + 1] + 6.0, y], style::ALLEY, name);
            }
        }
    }

    /// Organic warped grid: jittered vertices, bent edges, dropped edges, cul-de-sacs.
    #[allow(clippy::too_many_arguments, clippy::needless_range_loop)]
    fn organic(
        &mut self,
        x0: f32,
        x1: f32,
        y0: f32,
        y1: f32,
        dx: f32,
        dy: f32,
        warp: f32,
        bulge: f32,
        drop: f32,
        culs: f32,
        st: &dyn Fn(bool, usize) -> Style,
        name: &'static str,
    ) {
        let nx = ((x1 - x0) / dx).round().max(1.0) as usize;
        let ny = ((y1 - y0) / dy).round().max(1.0) as usize;
        let sx = (x1 - x0) / nx as f32;
        let sy = (y1 - y0) / ny as f32;
        let mut pts = vec![vec![[0.0f32; 2]; nx + 1]; ny + 1];
        for (j, row) in pts.iter_mut().enumerate() {
            for (i, p) in row.iter_mut().enumerate() {
                // Boundary vertices stay on the district edge (meeting boundary streets);
                // they overshoot outwards so the neighbour's street is crossed.
                let edge_x = i == 0 || i == nx;
                let edge_y = j == 0 || j == ny;
                let jx = if edge_x {
                    0.0
                } else {
                    self.rng.range_f32(-warp, warp)
                };
                let jy = if edge_y {
                    0.0
                } else {
                    self.rng.range_f32(-warp, warp)
                };
                let ox = if i == 0 {
                    -25.0
                } else if i == nx {
                    25.0
                } else {
                    0.0
                };
                let oy = if j == 0 {
                    -25.0
                } else if j == ny {
                    25.0
                } else {
                    0.0
                };
                *p = [x0 + i as f32 * sx + jx + ox, y0 + j as f32 * sy + jy + oy];
            }
        }
        for j in 0..=ny {
            for i in 0..nx {
                // Edges along the district boundary are the neighbour's / boundary street's.
                if j == 0 || j == ny {
                    continue;
                }
                if self.rng.next_f32() < drop {
                    continue;
                }
                let b = self.rng.range_f32(-bulge, bulge);
                let (a, c) = (pts[j][i], pts[j][i + 1]);
                self.add(bent(a, c, b, 8.0), st(true, j), name);
            }
        }
        for i in 0..=nx {
            for j in 0..ny {
                if i == 0 || i == nx {
                    continue;
                }
                if self.rng.next_f32() < drop {
                    continue;
                }
                let b = self.rng.range_f32(-bulge, bulge);
                let (a, c) = (pts[j][i], pts[j + 1][i]);
                self.add(bent(a, c, b, 8.0), st(false, i), name);
            }
        }
        // Cul-de-sacs from interior vertices into the blocks.
        for j in 1..ny {
            for i in 1..nx {
                if self.rng.next_f32() >= culs {
                    continue;
                }
                let a = pts[j][i];
                let ang = self.rng.range_f32(0.0, core::f32::consts::TAU);
                // Diagonal into a block (between the grid directions).
                let q = (ang / core::f32::consts::FRAC_PI_2).floor();
                let ang = (q + 0.5) * core::f32::consts::FRAC_PI_2;
                let (s, c) = wp_math::sin_cos(ang);
                let len = (sx.min(sy) * 0.42).min(75.0);
                let b = [a[0] + c * len, a[1] + s * len];
                let bulge = self.rng.range_f32(-6.0, 6.0);
                let k = self.add(bent(a, b, bulge, 6.0), style::STREET, name);
                self.lines[k].keep_ends = true;
            }
        }
    }
}

/// Lay out every street line of the city.
pub fn layout(seed: u64) -> Vec<Line> {
    let mut l = Lay::new(seed);
    arterials(&mut l);
    lantern_row(&mut l);
    midtown(&mut l);
    uptown(&mut l);
    old_harbor(&mut l);
    mercer(&mut l);
    eastgate(&mut l);
    suburbs(&mut l);
    industrial(&mut l);
    port(&mut l);
    hills(&mut l);
    outskirts(&mut l);
    l.lines
}

fn arterials(l: &mut Lay) {
    let art = style::ARTERIAL;
    // Meridian Avenue: the spine, jogging east around the Northfield Mall.
    l.add(
        resample(
            &fillet(
                &[
                    [400.0, -3250.0],
                    [400.0, 2130.0],
                    [500.0, 2230.0],
                    [500.0, 2440.0],
                    [400.0, 2540.0],
                    [400.0, 2985.0],
                ],
                &[0.0, 60.0, 60.0, 60.0, 60.0, 0.0],
                false,
                4.0,
            ),
            10.0,
        ),
        art,
        "Meridian Avenue",
    );
    l.seg(
        [1500.0, 1830.0],
        [1500.0, -1690.0],
        art.at(35.0),
        "Kettle Street",
    );
    let k = l.curve(
        &[
            [2150.0, -330.0],
            [2150.0, 150.0],
            [2140.0, 600.0],
            [2090.0, 1300.0],
            [2050.0, 1905.0],
        ],
        art.at(30.0).with_parking(),
        "Harbor Boulevard",
    );
    l.lines[k].keep_ends = true;
    // Lantern Way: west piece between the rail yard and the roundhouse, main piece from
    // Westline Yards (east of the Westline Cloverleaf) to Harbor Boulevard.
    let lw = Style {
        bus: true,
        ..art.at(35.0)
    };
    l.seg(
        [-3060.0, 235.0],
        [-2290.0, 235.0],
        art.at(35.0),
        "Lantern Way",
    );
    l.seg([-1720.0, 150.0], [2175.0, 150.0], lw, "Lantern Way");
    l.seg([-3060.0, 1300.0], [2120.0, 1300.0], art, "Founders Parkway");
    let k = l.seg([-3060.0, -1012.0], [2780.0, -1012.0], art, "Canal Street");
    l.lines[k].keep_ends = true;
    l.seg(
        [-3185.0, -2400.0],
        [1180.0, -2400.0],
        art,
        "Terminal Avenue",
    );
    // Old Post Road: the colonial diagonal (2+2; 1+1 on the Iron Bridge).
    l.add(
        resample(
            &[
                [-2600.0, -2960.0],
                [-1006.0, -1003.0],
                [-480.0, -460.0],
                [0.0, 150.0],
            ],
            10.0,
        ),
        art.at(35.0),
        "Old Post Road",
    );
    // Westline Avenue: south of the rail yard (Low Bridge), jogging west around the yard
    // over the level crossing, and north of it from the yard gate.
    l.seg(
        [-2500.0, -2960.0],
        [-2500.0, -150.0],
        art.at(35.0),
        "Westline Avenue",
    );
    l.seg(
        [-3060.0, -180.0],
        [-2420.0, -180.0],
        art.at(35.0),
        "Westline Avenue",
    );
    l.seg(
        [-3040.0, -200.0],
        [-3040.0, 260.0],
        art.at(35.0),
        "Westline Avenue",
    );
    let k = l.seg(
        [-2500.0, 2130.0],
        [-2500.0, 162.0],
        art.at(35.0),
        "Westline Avenue",
    );
    l.lines[k].keep_ends = true;
    let k = l.seg(
        [-600.0, -3160.0],
        [-600.0, -545.0],
        art.at(30.0),
        "Dunmore Avenue",
    );
    l.lines[k].keep_ends = true;
    l.seg(
        [-900.0, -380.0],
        [-900.0, 1830.0],
        style::COLLECTOR.at(25.0).with_parking(),
        "Mercer Street",
    );
    // Shore Drive along the beach park.
    l.seg(
        [-2960.0, -3195.0],
        [300.0, -3195.0],
        style::SCENIC.at(35.0),
        "Shore Drive",
    );
    // Airport cargo road to Westline Avenue.
    l.curve(
        &[
            [-3500.0, -1095.0],
            [-3500.0, -900.0],
            [-3200.0, -700.0],
            [-2700.0, -655.0],
            [-2480.0, -655.0],
        ],
        art.at(35.0),
        "Airport Road",
    );
    // Riverside Drive over the Riverside Bridge.
    l.curve(
        &[
            [-4330.0, -150.0],
            [-3900.0, -175.0],
            [-3640.0, -190.0],
            [-3600.0, -260.0],
            [-3600.0, -700.0],
            [-3560.0, -860.0],
            [-3480.0, -905.0],
        ],
        style::COLLECTOR.at(30.0).no_parking(),
        "Riverside Drive",
    );
    // Airfield Road: from Meridian's north end to the airfield gate.
    let k = l.add(
        resample(
            &fillet(
                &[[395.0, 2985.0], [900.0, 2995.0], [900.0, 3042.0]],
                &[0.0, 25.0, 0.0],
                false,
                3.0,
            ),
            8.0,
        ),
        style::STREET.at(25.0),
        "Airfield Road",
    );
    l.lines[k].keep_ends = true;
}

fn lantern_row(l: &mut Lay) {
    // Columns every 100 m (Meridian at 400 is an arterial line already); rows every 100 m
    // from the river bank to the Crosstown (y = 800, rows near it are dropped).
    let xs: Vec<f32> = (0..15)
        .map(|i| -200.0 + 100.0 * i as f32)
        .filter(|x| (*x - 400.0).abs() > 1.0)
        .collect();
    let ys = [
        -350.0, -250.0, -150.0, -50.0, 50.0, 250.0, 350.0, 450.0, 550.0, 650.0,
    ];
    let av = style::AVENUE;
    l.grid(
        &xs,
        &ys,
        -300.0,
        1200.0,
        -400.0,
        720.0,
        30.0,
        &|_, x| {
            if (x - 800.0).abs() < 1.0 || (x - 0.0).abs() < 1.0 {
                style::ARTERIAL.at(35.0).lanes(2)
            } else {
                av
            }
        },
        &|_, y| {
            if (y - 450.0).abs() < 1.0 || (y + 250.0).abs() < 1.0 {
                style::ARTERIAL.at(35.0)
            } else {
                av
            }
        },
        "Lantern Row",
    );
    let mut core: Vec<f32> = xs.clone();
    core.push(400.0);
    core.sort_by(f32::total_cmp);
    let rows = [
        -250.0, -150.0, -50.0, 50.0, 150.0, 250.0, 350.0, 450.0, 550.0,
    ];
    l.alleys(&core[1..core.len() - 2], &rows, 0.55, "Lantern Row alley");
    // Boundary streets with Midtown and Old Harbor.
    l.seg(
        [-300.0, -380.0],
        [-300.0, 1830.0],
        style::COLLECTOR,
        "Ash Street",
    );
    l.seg(
        [1200.0, -380.0],
        [1200.0, 1830.0],
        style::COLLECTOR,
        "Kelp Street",
    );
}

fn midtown(l: &mut Lay) {
    // One-way pairs: east-west avenues alternate direction; north-south two-way streets.
    let ow = style::ARTERIAL.one_way(3).at(35.0);
    let ys = [-330.0, -195.0, -60.0, 290.0, 430.0, 570.0, 700.0];
    let xs = [
        -1430.0, -1300.0, -1160.0, -1030.0, -770.0, -640.0, -510.0, -400.0,
    ];
    l.grid(
        &xs,
        &ys,
        -1500.0,
        -300.0,
        -400.0,
        760.0,
        30.0,
        &|i, _| {
            if i % 3 == 1 {
                ow
            } else {
                style::COLLECTOR
            }
        },
        &|_, _| ow,
        "Midtown",
    );
    l.alleys(&xs, &[-330.0, -195.0, -60.0], 0.35, "Midtown alley");
    l.seg(
        [-1500.0, -380.0],
        [-1500.0, 2060.0],
        style::COLLECTOR,
        "Larch Street",
    );
}

fn uptown(l: &mut Lay) {
    let xs: Vec<f32> = (0..15)
        .map(|i| -200.0 + 100.0 * i as f32)
        .filter(|x| (*x - 400.0).abs() > 1.0)
        .collect();
    let ys = [880.0, 1010.0, 1140.0, 1440.0, 1570.0, 1700.0];
    l.grid(
        &xs,
        &ys,
        -300.0,
        1200.0,
        870.0,
        1800.0,
        30.0,
        &|_, x| {
            if (x - 800.0).abs() < 1.0 || x.abs() < 1.0 {
                style::ARTERIAL.at(35.0)
            } else {
                style::AVENUE
            }
        },
        &|_, _| style::AVENUE,
        "Uptown",
    );
    l.seg(
        [-1500.0, 1800.0],
        [1200.0, 1800.0],
        style::COLLECTOR,
        "Ridge Street",
    );
}

fn old_harbor(l: &mut Lay) {
    let cob = style::LOCAL.at(25.0);
    let xs = [1300.0, 1400.0, 1610.0, 1700.0, 1910.0, 2020.0];
    let ys = [
        -330.0, -230.0, -130.0, -30.0, 60.0, 250.0, 350.0, 450.0, 550.0, 650.0, 750.0, 850.0,
        950.0, 1030.0,
    ];
    l.grid(
        &xs,
        &ys,
        1200.0,
        2150.0,
        -400.0,
        1100.0,
        30.0,
        &|_, _| cob,
        &|j, _| if j % 4 == 1 { style::COLLECTOR } else { cob },
        "Old Harbor",
    );
    l.seg(
        [1200.0, 1100.0],
        [2120.0, 1100.0],
        style::COLLECTOR,
        "Gull Street",
    );
}

fn mercer(l: &mut Lay) {
    let xs = [
        -1400.0, -1280.0, -1160.0, -1030.0, -780.0, -660.0, -540.0, -420.0,
    ];
    let ys = [890.0, 1010.0, 1130.0, 1420.0, 1540.0, 1660.0];
    l.grid(
        &xs,
        &ys,
        -1500.0,
        -300.0,
        860.0,
        1800.0,
        30.0,
        &|_, _| style::LOCAL,
        &|j, _| {
            if j == 2 {
                style::COLLECTOR.with_parking()
            } else {
                style::LOCAL.with_parking()
            }
        },
        "Mercer",
    );
}

fn eastgate(l: &mut Lay) {
    let xs = [1310.0, 1420.0, 1600.0, 1700.0, 1910.0, 2010.0];
    let ys = [1200.0, 1400.0, 1500.0, 1610.0, 1720.0];
    l.grid(
        &xs,
        &ys,
        1200.0,
        2090.0,
        1100.0,
        1800.0,
        30.0,
        &|_, _| style::LOCAL.at(25.0),
        &|_, _| style::LOCAL.at(25.0),
        "Eastgate",
    );
    l.seg(
        [1200.0, 1800.0],
        [2060.0, 1800.0],
        style::COLLECTOR,
        "Bayview Terrace",
    );
}

fn suburbs(l: &mut Lay) {
    let sub = |row: bool, k: usize| {
        if (row && k == 2) || (!row && k == 3) {
            style::COLLECTOR.no_parking()
        } else {
            style::STREET
        }
    };
    // Larkspur (west of Mercer, north of the yards).
    l.organic(
        -3000.0, -1500.0, 800.0, 1800.0, 210.0, 200.0, 28.0, 18.0, 0.12, 0.3, &sub, "Larkspur",
    );
    l.seg(
        [-3060.0, 800.0],
        [-1470.0, 800.0],
        style::COLLECTOR,
        "Larkspur Avenue",
    );
    l.seg(
        [-3060.0, 1800.0],
        [-1470.0, 1800.0],
        style::COLLECTOR,
        "Hollins Road",
    );
    // Southmoor (south-west suburb).
    l.organic(
        -3000.0,
        -1500.0,
        -3000.0,
        -1700.0,
        220.0,
        210.0,
        30.0,
        20.0,
        0.12,
        0.35,
        &sub,
        "Southmoor",
    );
    l.seg(
        [-3060.0, -1700.0],
        [1420.0, -1700.0],
        style::COLLECTOR.no_parking(),
        "Moor Street",
    );
    l.seg(
        [-3060.0, -3000.0],
        [430.0, -3000.0],
        style::COLLECTOR.no_parking(),
        "Sands Road",
    );
    // Northfield (north of Uptown, big blocks).
    l.organic(
        -1000.0,
        1000.0,
        1800.0,
        3000.0,
        230.0,
        210.0,
        30.0,
        22.0,
        0.1,
        0.25,
        &sub,
        "Northfield",
    );
    l.seg(
        [-1030.0, 3000.0],
        [1030.0, 3000.0],
        style::STREET,
        "Field Road",
    );
    // South Shore strip between Sands Road and Shore Drive.
    l.organic(
        -2950.0,
        350.0,
        -3195.0,
        -3000.0,
        220.0,
        195.0,
        15.0,
        12.0,
        0.15,
        0.2,
        &sub,
        "South Shore",
    );
    // Westridge Woods (east of the forest).
    l.organic(
        -3700.0,
        -3000.0,
        820.0,
        1800.0,
        200.0,
        220.0,
        30.0,
        25.0,
        0.2,
        0.3,
        &sub,
        "Westridge Woods",
    );
    l.seg(
        [-3060.0, 800.0],
        [-3060.0, 1830.0],
        style::COLLECTOR.no_parking(),
        "Westridge Road",
    );
    // Ferris Park (south bank, residential grid).
    l.organic(
        -3000.0,
        -1500.0,
        -1700.0,
        -520.0,
        190.0,
        190.0,
        18.0,
        10.0,
        0.06,
        0.15,
        &sub,
        "Ferris Park",
    );
    l.seg(
        [-1500.0, -1720.0],
        [-1500.0, -560.0],
        style::COLLECTOR,
        "Ferris Avenue",
    );
    // Dunmore (port workers' neighbourhood).
    l.organic(
        -1500.0,
        400.0,
        -3000.0,
        -1700.0,
        180.0,
        180.0,
        12.0,
        8.0,
        0.05,
        0.1,
        &|_, _| style::LOCAL,
        "Dunmore",
    );
    l.seg(
        [-1500.0, -3030.0],
        [-1500.0, -1680.0],
        style::COLLECTOR,
        "Ferris Avenue",
    );
    // North Hollins strip.
    l.organic(
        -3000.0,
        -1000.0,
        3000.0,
        3380.0,
        260.0,
        190.0,
        20.0,
        15.0,
        0.2,
        0.25,
        &|_, _| style::STREET,
        "North Hollins",
    );
    l.seg(
        [-3030.0, 3000.0],
        [-970.0, 3000.0],
        style::STREET,
        "Knob View Road",
    );
}

fn industrial(l: &mut Lay) {
    let svc = style::SERVICE;
    // Westline Yards east of the Loop (around the freight line and Central Line).
    for y in [-300.0, -130.0, 430.0, 620.0] {
        l.seg([-2030.0, y], [-1470.0, y], svc, "Westline Yards");
    }
    for x in [-1870.0, -1580.0] {
        l.seg([x, -420.0], [x, 820.0], svc, "Westline Yards");
    }
    // North of the yard and around the roundhouse.
    l.seg([-2980.0, 620.0], [-2030.0, 620.0], svc, "Westline Yards");
    l.seg([-2760.0, 230.0], [-2760.0, 820.0], svc, "Westline Yards");
    let k = l.seg([-2300.0, 640.0], [-2300.0, 488.0], svc, "Roundhouse Road");
    l.lines[k].keep_ends = true;
    // Canal Flats: big blocks, mill land.
    let cf_x = [-1250.0, -1000.0, -760.0, -420.0, -150.0, 120.0];
    let cf_y = [-1415.0, -1200.0, -800.0, -650.0];
    l.grid(
        &cf_x,
        &cf_y,
        -1500.0,
        400.0,
        -1700.0,
        -540.0,
        30.0,
        &|_, _| svc,
        &|j, _| {
            if j == 1 {
                style::COLLECTOR.no_parking()
            } else {
                svc
            }
        },
        "Canal Flats",
    );
    // Westline Outskirts.
    let wo_x = [-4100.0, -3800.0, -3260.0];
    let wo_y = [120.0, 520.0, 700.0];
    l.grid(
        &wo_x,
        &wo_y,
        -4380.0,
        -3000.0,
        -600.0,
        790.0,
        30.0,
        &|_, _| style::COLLECTOR.no_parking(),
        &|_, _| svc,
        "Westline Outskirts",
    );
}

fn port(l: &mut Lay) {
    let p = style::PORT;
    // Gantry Row: port roads in big blocks between Meridian and the quay.
    for y in [-1500.0, -1180.0, -820.0, -640.0] {
        l.seg(
            [400.0, y],
            [2590.0, y],
            if y == -1180.0 { p } else { style::SERVICE },
            "Gantry Row",
        );
    }
    for x in [700.0, 1000.0, 1250.0, 1750.0, 2050.0, 2350.0, 2520.0] {
        l.seg(
            [x, -1720.0],
            [x, -540.0],
            if x == 2350.0 { p } else { style::SERVICE },
            "Gantry Row",
        );
    }
    // Container terminal approach (west of the fence).
    for x in [650.0, 950.0, 1400.0] {
        l.seg([x, -3030.0], [x, -1680.0], style::SERVICE, "Terminal Road");
    }
    for y in [-2700.0, -2950.0] {
        l.seg([380.0, y], [1530.0, y], style::SERVICE, "Terminal Road");
    }
}

fn hills(l: &mut Lay) {
    // Hollins Drive: the touge.
    let k = l.add(hollins_path(), style::HILL, "Hollins Drive");
    l.lines[k].protect = 45.0;
    l.lines[k].keep_ends = true;
    // Hollins Heights lower slopes: contour roads east and west of the drive.
    let hill = style::HILL;
    l.curve(
        &[
            [-1470.0, 1830.0],
            [-1500.0, 2000.0],
            [-1380.0, 2250.0],
            [-1150.0, 2420.0],
            [-1030.0, 2560.0],
        ],
        hill,
        "Hollins Heights",
    );
    l.curve(
        &[
            [-1030.0, 2150.0],
            [-1250.0, 2120.0],
            [-1500.0, 2200.0],
            [-1620.0, 2420.0],
            [-1700.0, 2750.0],
            [-1900.0, 2950.0],
        ],
        hill,
        "Hollins Heights",
    );
    l.curve(
        &[
            [-2950.0, 2050.0],
            [-2700.0, 2000.0],
            [-2450.0, 1950.0],
            [-2200.0, 1895.0],
            [-2000.0, 1925.0],
            [-1896.0, 1966.0],
        ],
        hill,
        "Hollins Heights",
    );
    l.curve(
        &[
            [-2960.0, 2400.0],
            [-2800.0, 2300.0],
            [-2650.0, 2150.0],
            [-2600.0, 1990.0],
        ],
        hill,
        "Hollins Heights",
    );
    l.curve(
        &[
            [-2960.0, 2800.0],
            [-2750.0, 2900.0],
            [-2500.0, 2980.0],
            [-2200.0, 2990.0],
            [-1900.0, 2950.0],
            [-1600.0, 2990.0],
            [-1020.0, 2990.0],
        ],
        hill,
        "Hollins Heights",
    );
    l.curve(
        &[
            [-2700.0, 2000.0],
            [-2780.0, 2300.0],
            [-2800.0, 2600.0],
            [-2700.0, 2890.0],
        ],
        hill,
        "Hollins Heights",
    );
    // Skyline Drive: from the Route 40 West interchange north through the Westridge Tunnel
    // and along the ridge up to the Knob.
    let k = l.curve(
        &[
            [-3400.0, 140.0],
            [-3400.0, 330.0],
            [-3420.0, 700.0],
            [-3500.0, 1050.0],
            [-3510.0, 1350.0],
            [-3450.0, 1650.0],
            [-3250.0, 2100.0],
            [-3000.0, 2420.0],
            [-2650.0, 2560.0],
            [-2390.0, 2528.0],
        ],
        style::SCENIC.at(35.0),
        "Skyline Drive",
    );
    l.lines[k].protect = 30.0;
    l.lines[k].keep_ends = true;
    // The Knob Observatory Lot spur between the two drives' ends.
    let k = l.seg(
        [-2390.0, 2528.0],
        [-2300.0, 2522.0],
        style::STREET,
        "Knob Observatory Lot",
    );
    l.lines[k].keep_ends = true;
    // Bluff Road along the Bayview cliff, down to Harbor Boulevard.
    let k = l.curve(
        &[
            [1000.0, 2955.0],
            [1350.0, 2880.0],
            [1700.0, 2710.0],
            [2000.0, 2530.0],
            [2220.0, 2320.0],
            [2230.0, 2100.0],
            [2050.0, 1905.0],
        ],
        style::SCENIC.at(30.0),
        "Bluff Road",
    );
    l.lines[k].protect = 30.0;
    l.lines[k].keep_ends = true;
    // Bayview Bluffs streets inland of Bluff Road.
    l.organic(
        1000.0,
        2000.0,
        1800.0,
        2600.0,
        200.0,
        200.0,
        25.0,
        18.0,
        0.15,
        0.3,
        &|_, _| style::STREET,
        "Bayview Bluffs",
    );
    l.seg(
        [1000.0, 1830.0],
        [1000.0, 2960.0],
        style::COLLECTOR.no_parking(),
        "Cliff Avenue",
    );
}

fn outskirts(l: &mut Lay) {
    // A few roads west of the airport landside and the airport police.
    let k = l.seg(
        [-3090.0, -2225.0],
        [-3350.0, -2225.0],
        style::SERVICE,
        "Long Stay Road",
    );
    l.lines[k].keep_ends = true;
    l.seg(
        [-3120.0, -2600.0],
        [-3120.0, -1000.0],
        style::COLLECTOR.no_parking(),
        "Airport Boulevard",
    );
    let _ = RoadClass::Local;
}
