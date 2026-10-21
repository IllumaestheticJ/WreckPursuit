//! Port Vesper v3 — parking: multi-storey parking structures (drivable decks and ramps),
//! surface lots with parked cars, the Starlite Drive-In.
//!
//! **Owned by the v3 parking workstream.** Contract with the pipeline (`metro::generate`):
//! - [`earthworks`] levels every garage / lot site the plan leaves natural to the natural
//!   ground height at its gate (so the street, the gate and the ground floor agree);
//! - [`generate`] runs after roads and fills the `ParkingGarage` and `SurfaceLot` sites.
//!
//! ## Garages
//! Ground floor (a solid podium prism levelled at the gate) plus 3–5 decks
//! ([`FLOOR_H`] floor to floor, [`SLAB_T`] slabs). All ramps are stacked in one walled ramp
//! lane ([`RAMP_W`] wide) on the right-hand side seen from the gate: ramp *k* climbs from
//! deck *k* to deck *k + 1* over the same footprint as every other ramp (eased ends at half
//! slope, main run at [`RAMP_SLOPE`]), so ramps are always [`FLOOR_H`] apart and headroom
//! is `FLOOR_H - SLAB_T` everywhere. The way up: gate → front cross aisle → ramp lane →
//! arrive at the back of the next deck → back cross aisle → any aisle back to the front →
//! next ramp. The space under ramp 0 is filled (thick ramp) so nothing gets wedged under
//! it; the top deck has a parapet across the open ramp well. Bays (2.6 × 5 m) sit in
//! double-loaded modules either side of 6.5 m aisles; pillars stand on the back lines of
//! the bay rows every three bays; two stair cores take the corner bays of the first row.
//!
//! ## Surface lots
//! A central spine aisle from the gate, a front cross aisle, side ring aisles and
//! double-loaded rows either side of the spine; kerb islands with lamps and trees at the
//! row ends; a non-solid asphalt slab a few cm above the (levelled) terrain; a hedge or
//! fence around the site with an opening at the gate.
//!
//! ## Starlite Drive-In (SH10)
//! Screen tower at the far end (clear space behind it for the safehouse), arcs of small
//! drivable mounds facing the screen, a projection booth, a ticket booth at the gate, a few
//! derelict cars and a wooden perimeter fence.

pub(super) mod kit;

use wp_math::Rng;

use self::kit::{Frame, Sink};
use super::ctx::{yaw_of, Ctx};
use super::ground::{Earthwork, Natural};
use super::plan::{Plan, Site, SiteKind};
use crate::network::{Feature, FeatureKind as K, Material as M};

/// RNG salt of this module.
pub const SALT: u64 = 0x9A;

/// Garage floor-to-floor height, m.
pub const FLOOR_H: f32 = 3.0;
/// Garage deck slab thickness, m.
pub const SLAB_T: f32 = 0.3;
/// Garage ramp width (between the ramp-lane walls), m.
pub const RAMP_W: f32 = 7.0;
/// Main slope of garage ramps (rise / run); the eased ends are half this.
pub const RAMP_SLOPE: f32 = 0.13;
/// Horizontal length of each eased ramp end, m.
const RAMP_EASE: f32 = 3.0;
/// Wall / parapet thickness, m.
const WALL_T: f32 = 0.3;
/// Parapet height above a deck, m.
const PARAPET_H: f32 = 1.1;
/// Depth of the front / back cross aisles in a garage, m.
const CROSS_AISLE: f32 = 7.0;
/// Drive aisle width, m.
const AISLE: f32 = 6.5;
/// Bay width, m.
const BAY_W: f32 = 2.6;
/// Bay depth, m.
const BAY_D: f32 = 5.0;
/// Parked car size: width, length, height, m.
const CAR: [f32; 3] = [1.8, 4.5, 1.45];
/// Garage apron between the gate and the building front, m.
const APRON: f32 = 4.0;
/// Garage entrance width, m.
const ENTRY_W: f32 = 8.0;
/// Largest garage footprint (across × deep), m.
const GARAGE_MAX: [f32; 2] = [90.0, 72.0];

/// Number of decks above the ground floor for a garage site.
fn decks_for(id: &str) -> usize {
    match id {
        // SH03 is "Lantern Garage Level 4": five decks above the ground floor.
        "lantern_garage" => 5,
        "midtown_mall_garage" | "airport_garage" => 4,
        _ => 3,
    }
}

/// Ground levelling this module needs before roads are profiled: every garage and lot
/// site without a plan pad is levelled to the natural ground at its gate.
pub fn earthworks(plan: &Plan, nat: &Natural) -> Vec<Earthwork> {
    plan.sites
        .iter()
        .filter(|s| matches!(s.kind, SiteKind::ParkingGarage | SiteKind::SurfaceLot))
        .filter_map(|s| kit::site_pad(s, nat))
        .collect()
}

/// Place this module's content.
pub fn generate(ctx: &mut Ctx) {
    let mut rng = ctx.rng(SALT);
    let sites: Vec<Site> = ctx
        .plan
        .sites
        .iter()
        .filter(|s| matches!(s.kind, SiteKind::ParkingGarage | SiteKind::SurfaceLot))
        .cloned()
        .collect();
    for s in &sites {
        // One stream per site, forked in plan order.
        let mut r = rng.fork();
        let mut sink = Sink::default();
        if s.gates.is_empty() {
            continue;
        }
        match (s.kind, s.id) {
            (SiteKind::ParkingGarage, _) => build_garage(ctx, s, &mut r, &mut sink),
            (_, "drive_in") => build_drive_in(ctx, s, &mut r, &mut sink),
            _ => build_lot(ctx, s, &mut r, &mut sink),
        }
        sink.flush(ctx);
    }
}

// ---------------------------------------------------------------------------------------
// Garages
// ---------------------------------------------------------------------------------------

/// Inputs of [`garage`] (building-local coordinates: x right, y away from the gate,
/// origin at the front-left corner).
#[derive(Debug, Clone)]
pub(crate) struct GarageSpec {
    /// Width (local x), m.
    pub w: f32,
    /// Depth (local y), m.
    pub d: f32,
    /// Decks above the ground floor.
    pub decks: usize,
    /// Centre of the ground-floor entrance (local x), m.
    pub gate_x: f32,
    /// Ground floor level, m.
    pub z0: f32,
    /// Bottom of the podium, m.
    pub base: f32,
}

/// Output of [`garage`].
#[derive(Debug, Default)]
pub(crate) struct GarageOut {
    /// Structure (podium, decks, ramps, walls, pillars, cores) — marks occupancy.
    pub structure: Vec<Feature>,
    /// Paint and parked cars.
    pub small: Vec<Feature>,
    /// Drive aisles, local `[x0, y0, x1, y1]` (cross aisles, row aisles, ramp lane).
    pub aisles: Vec<[f32; 4]>,
    /// Deck levels (ground floor first), m.
    pub levels: Vec<f32>,
}

/// One bay row: left edge (local x) and whether its back line is on the left.
#[derive(Debug, Clone, Copy)]
struct Row {
    x0: f32,
    back_left: bool,
}

/// A paint strip: one row, or two rows back to back.
#[derive(Debug, Clone, Copy)]
struct Strip {
    x0: f32,
    x1: f32,
    /// Pillar line (local x), if any.
    pillar_x: Option<f32>,
}

/// Build one parking structure in the frame `bf` (see [`GarageSpec`]).
pub(crate) fn garage(bf: &Frame, s: &GarageSpec, rng: &mut Rng) -> GarageOut {
    let mut out = GarageOut::default();
    let (w, d, n, h) = (s.w, s.d, s.decks, FLOOR_H);
    let zk = |k: usize| s.z0 + k as f32 * h;
    let top = zk(n);
    out.levels = (0..=n).map(zk).collect();
    let lane_x0 = w - WALL_T - RAMP_W;
    let ys = WALL_T + CROSS_AISLE;
    let ye = d - WALL_T - CROSS_AISLE;
    let a = ys + 3.0;
    let b = a + h / RAMP_SLOPE + RAMP_EASE;
    let st = &mut out.structure;

    // Ground floor podium and decks (each deck: main slab + ramp-lane slabs either side of
    // the ramp well).
    st.push(bf.prism_rect(
        K::ParkingDeck,
        M::Concrete,
        0.0,
        0.0,
        w,
        d,
        s.base,
        s.z0,
        true,
    ));
    for k in 1..=n {
        let z = zk(k);
        for (x0, y0, x1, y1) in [
            (0.0, 0.0, lane_x0, d),
            (lane_x0, 0.0, w, a),
            (lane_x0, b, w, d),
        ] {
            st.push(bf.span(
                K::ParkingDeck,
                M::Concrete,
                x0,
                y0,
                x1,
                y1,
                z - SLAB_T,
                z,
                true,
            ));
        }
    }

    // Ramps, all stacked in the ramp lane.
    let xc = lane_x0 + RAMP_W * 0.5;
    let ease_rise = RAMP_SLOPE * 0.5 * RAMP_EASE;
    for k in 0..n {
        let z = zk(k);
        let pts = [
            [a, z],
            [a + RAMP_EASE, z + ease_rise],
            [b - RAMP_EASE, z + h - ease_rise],
            [b, z + h],
        ];
        // Ramp 0 is a solid wedge down into the podium: nothing can get under it.
        let thick = if k == 0 { h + 0.6 } else { SLAB_T };
        for q in pts.windows(2) {
            st.push(bf.ramp(
                K::ParkingRamp,
                M::Concrete,
                [xc, q[0][0], q[0][1]],
                [xc, q[1][0], q[1][1]],
                RAMP_W,
                thick,
            ));
        }
    }

    // Walls: the ramp lane is walled full height on both sides along the ramps.
    let wall_top = top + PARAPET_H;
    for (x0, x1) in [(lane_x0 - WALL_T, lane_x0), (w - WALL_T, w)] {
        st.push(bf.span(
            K::Wall,
            M::Concrete,
            x0,
            a,
            x1,
            b,
            s.z0 - SLAB_T,
            wall_top,
            true,
        ));
    }
    let gx = s
        .gate_x
        .clamp(ENTRY_W * 0.5 + 1.0, lane_x0 - ENTRY_W * 0.5 - 1.0);
    for k in 0..=n {
        let z = zk(k);
        let (z0, z1) = (z - SLAB_T, z + PARAPET_H);
        let mut pieces = vec![
            (0.0, 0.0, WALL_T, d),
            (WALL_T, d - WALL_T, w, d),
            (w - WALL_T, 0.0, w, a),
            (w - WALL_T, b, w, d - WALL_T),
        ];
        if k == 0 {
            pieces.push((WALL_T, 0.0, gx - ENTRY_W * 0.5, WALL_T));
            pieces.push((gx + ENTRY_W * 0.5, 0.0, w - WALL_T, WALL_T));
        } else {
            pieces.push((WALL_T, 0.0, w - WALL_T, WALL_T));
        }
        for (x0, y0, x1, y1) in pieces {
            st.push(bf.span(K::Wall, M::Concrete, x0, y0, x1, y1, z0, z1, true));
        }
    }
    // Top deck: parapet across the open ramp well.
    st.push(bf.span(
        K::Wall,
        M::Concrete,
        lane_x0,
        a - WALL_T,
        w - WALL_T,
        a,
        top - SLAB_T,
        top + PARAPET_H,
        true,
    ));

    // Bay rows across the width left of the ramp lane.
    let ax0 = WALL_T;
    let ax1 = lane_x0 - WALL_T;
    let avail = ax1 - ax0;
    let module = 2.0 * BAY_D + AISLE;
    let nm = (avail / module).floor().max(0.0) as usize;
    let mut rem = avail - nm as f32 * module;
    let extra = rem >= BAY_D + AISLE;
    if extra {
        rem -= BAY_D + AISLE;
    }
    let na = nm + extra as usize;
    let aw = AISLE
        + if na > 0 {
            (rem / na as f32).min(1.0)
        } else {
            0.0
        };
    let mut rows = Vec::new();
    let mut x = ax0;
    for _ in 0..nm {
        rows.push(Row {
            x0: x,
            back_left: true,
        });
        x += BAY_D;
        out.aisles.push([x, ys, x + aw, ye]);
        x += aw;
        rows.push(Row {
            x0: x,
            back_left: false,
        });
        x += BAY_D;
    }
    if extra {
        rows.push(Row {
            x0: x,
            back_left: true,
        });
        x += BAY_D;
        out.aisles.push([x, ys, x + aw, ye]);
        x += aw;
    }
    if ax1 - x > 0.5 {
        out.aisles.push([x, ys, ax1, ye]);
    }
    out.aisles.push([ax0, WALL_T, ax1, ys]);
    out.aisles.push([ax0, ye, ax1, d - WALL_T]);
    out.aisles.push([lane_x0, 0.0, w - WALL_T, d]);

    let mut strips = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        let r = rows[i];
        let paired = !r.back_left
            && rows
                .get(i + 1)
                .is_some_and(|q| q.back_left && (q.x0 - (r.x0 + BAY_D)).abs() < 1e-3);
        if paired {
            strips.push(Strip {
                x0: r.x0,
                x1: r.x0 + 2.0 * BAY_D,
                pillar_x: Some(r.x0 + BAY_D),
            });
            i += 2;
        } else {
            let pillar_x = if r.back_left {
                Some(r.x0 + 0.25)
            } else if ax1 - (r.x0 + BAY_D) > 0.6 {
                Some(r.x0 + BAY_D - 0.25)
            } else {
                None
            };
            strips.push(Strip {
                x0: r.x0,
                x1: r.x0 + BAY_D,
                pillar_x,
            });
            i += 1;
        }
    }

    // 0.6 m of slack so the end pillars stay out of the cross aisles.
    let nb = ((ye - ys - 0.6) / BAY_W).floor().max(0.0) as usize;
    let yb0 = ys + ((ye - ys) - nb as f32 * BAY_W) * 0.5;
    let bay_y = |i: usize| yb0 + i as f32 * BAY_W;
    // Stair / lift cores take the first and last two bays of the first row.
    let cores = rows.first().is_some_and(|r| r.back_left) && nb >= 6;
    let core_rects = if cores {
        vec![
            [ax0, bay_y(0), ax0 + BAY_D, bay_y(2)],
            [ax0, bay_y(nb - 2), ax0 + BAY_D, bay_y(nb)],
        ]
    } else {
        Vec::new()
    };
    let core_bay = |row: usize, i: usize| cores && row == 0 && (i < 2 || i + 2 >= nb);
    for c in &core_rects {
        st.push(bf.prism_rect(
            K::Pillar,
            M::Concrete,
            c[0] + 0.1,
            c[1] + 0.1,
            c[2] - 0.1,
            c[3] - 0.1,
            s.z0 - SLAB_T,
            top + 3.0,
            true,
        ));
    }
    let in_core = |x: f32, y: f32| {
        core_rects
            .iter()
            .any(|c| x > c[0] - 0.3 && x < c[2] + 0.3 && y > c[1] - 0.3 && y < c[3] + 0.3)
    };

    // Pillars every three bays on the back lines, from the ground floor to the top deck.
    if n >= 1 {
        for sp in &strips {
            let Some(px) = sp.pillar_x else { continue };
            for i in (0..=nb).step_by(3) {
                let py = bay_y(i);
                if in_core(px, py) {
                    continue;
                }
                st.push(bf.span(
                    K::Pillar,
                    M::Concrete,
                    px - 0.25,
                    py - 0.25,
                    px + 0.25,
                    py + 0.25,
                    s.z0,
                    top - SLAB_T,
                    true,
                ));
            }
        }
    }

    // Paint and parked cars, level by level.
    for k in 0..=n {
        let z = zk(k);
        for (si, sp) in strips.iter().enumerate() {
            for i in 0..=nb {
                if cores && si == 0 && (i <= 2 || i + 2 >= nb) {
                    continue;
                }
                let y = bay_y(i);
                out.small.push(bf.span(
                    K::ParkingPaint,
                    M::PaintWhite,
                    sp.x0 + 0.1,
                    y - 0.06,
                    sp.x1 - 0.1,
                    y + 0.06,
                    z + 0.002,
                    z + 0.012,
                    false,
                ));
            }
        }
        let p = rng.range_f32(0.4, 0.6);
        for (ri, r) in rows.iter().enumerate() {
            for i in 0..nb {
                if core_bay(ri, i) || !rng.chance(p) {
                    continue;
                }
                let cy = bay_y(i) + BAY_W * 0.5 + rng.range_f32(-0.08, 0.08);
                let gap = 0.3 + rng.range_f32(-0.1, 0.1);
                let cx = if r.back_left {
                    r.x0 + gap + CAR[1] * 0.5
                } else {
                    r.x0 + BAY_D - gap - CAR[1] * 0.5
                };
                let jit = rng.range_f32(-0.01, 0.01);
                out.small.push(bf.bx(
                    K::ParkedCar,
                    M::CarPaint,
                    cx,
                    cy,
                    z,
                    [CAR[1], CAR[0], CAR[2]],
                    jit,
                    true,
                ));
            }
        }
    }
    out
}

/// Fill one garage site: the structure, the entrance driveway, gate arm and booth, and a
/// low wall round the site.
fn build_garage(ctx: &mut Ctx, site: &Site, rng: &mut Rng, sink: &mut Sink) {
    let g = site.gates[0];
    let gf = Frame::of_gate(&g);
    let (lo, hi) = gf.bounds_of(&site.polygon);
    let mut x_l = lo[0] + 1.5;
    let x_r = hi[0] - 1.5;
    let mut w = x_r - x_l;
    if w > GARAGE_MAX[0] {
        let c = 0.0f32.clamp(x_l + GARAGE_MAX[0] * 0.5, x_r - GARAGE_MAX[0] * 0.5);
        x_l = c - GARAGE_MAX[0] * 0.5;
        w = GARAGE_MAX[0];
    }
    let y0 = APRON;
    let d = (hi[1] - 1.5 - y0).min(GARAGE_MAX[1]);
    let bf = gf.shifted(x_l, y0);
    let foot = bf.rect(0.0, 0.0, w, d);
    let (glo, ghi) = ctx.ground_range(&foot);
    let zg = kit::gate_z(ctx, &g);
    let z0 = zg.max(ghi) + 0.03;
    let spec = GarageSpec {
        w,
        d,
        decks: decks_for(site.id),
        gate_x: -x_l,
        z0,
        base: glo.min(zg) - 1.0,
    };
    let out = garage(&bf, &spec, rng);
    sink.place_all(out.structure);
    sink.added.extend(out.small);

    // Driveway from the gate up to the ground floor (flush at both ends), arm and booth.
    let lane_x0 = w - WALL_T - RAMP_W;
    let ex = (spec
        .gate_x
        .clamp(ENTRY_W * 0.5 + 1.0, lane_x0 - ENTRY_W * 0.5 - 1.0))
        + x_l;
    sink.place(gf.ramp(
        K::ParkingRamp,
        M::Asphalt,
        [0.0, 0.0, zg],
        [ex, y0, z0],
        ENTRY_W,
        0.5,
    ));
    sink.add(gf.bx(
        K::Gate,
        M::PaintWhite,
        ex * 0.5,
        y0 * 0.5,
        zg + 0.95,
        [ENTRY_W - 1.0, 0.12, 0.12],
        0.0,
        false,
    ));
    sink.place(gf.bx(
        K::Gate,
        M::Glass,
        ex * 0.5 + ENTRY_W * 0.5 + 1.4,
        y0 * 0.5,
        ctx.ground_z(gf.p(ex * 0.5 + ENTRY_W * 0.5 + 1.4, y0 * 0.5)),
        [2.0, 2.0, 2.6],
        0.0,
        true,
    ));
    sink.place_all(kit::perimeter(
        ctx,
        K::Wall,
        M::Concrete,
        &site.polygon,
        0.3,
        1.2,
        10.0,
        &site.gates,
    ));
}

// ---------------------------------------------------------------------------------------
// Surface lots
// ---------------------------------------------------------------------------------------

/// Per-lot parameters.
struct LotStyle {
    /// Largest parked area (across × deep), m.
    max: [f32; 2],
    /// Occupancy range.
    occ: (f32, f32),
    /// Perimeter kind, material, thickness, height.
    edge: (K, M, f32, f32),
}

fn lot_style(id: &str) -> LotStyle {
    match id {
        "airport_long_stay" => LotStyle {
            max: [180.0, 170.0],
            occ: (0.4, 0.55),
            edge: (K::Fence, M::Steel, 0.2, 2.2),
        },
        "stadium_lot" => LotStyle {
            max: [180.0, 150.0],
            occ: (0.25, 0.4),
            edge: (K::Fence, M::Steel, 0.2, 2.0),
        },
        _ => LotStyle {
            max: [160.0, 150.0],
            occ: (0.3, 0.5),
            edge: (K::Hedge, M::Foliage, 1.0, 1.5),
        },
    }
}

/// Spine (entry aisle) width of a lot, m.
const SPINE: f32 = 8.0;
/// Kerb island length at row ends, m.
const ISLAND: f32 = 2.5;
/// Side ring aisle width, m.
const RING: f32 = 7.0;
/// Lot cross aisle depth, m.
const LOT_CROSS: f32 = 8.0;

/// Non-solid slab tiles (≤ 30 m) a few cm above the ground over the local rectangle.
#[allow(clippy::too_many_arguments)]
fn slab_tiles(
    ctx: &Ctx,
    fr: &Frame,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    kind: K,
    mat: M,
    out: &mut Vec<Feature>,
) {
    let nx = ((x1 - x0) / 30.0).ceil().max(1.0) as usize;
    let ny = ((y1 - y0) / 30.0).ceil().max(1.0) as usize;
    for j in 0..ny {
        for i in 0..nx {
            let ax = x0 + (x1 - x0) * i as f32 / nx as f32;
            let bx = x0 + (x1 - x0) * (i + 1) as f32 / nx as f32;
            let ay = y0 + (y1 - y0) * j as f32 / ny as f32;
            let by = y0 + (y1 - y0) * (j + 1) as f32 / ny as f32;
            let (lo, hi) = ctx.ground_range(&fr.rect(ax, ay, bx, by));
            out.push(fr.span(kind, mat, ax, ay, bx, by, lo - 0.05, hi + 0.03, false));
        }
    }
}

/// Fill a surface lot.
fn build_lot(ctx: &mut Ctx, site: &Site, rng: &mut Rng, sink: &mut Sink) {
    let style = lot_style(site.id);
    let g = site.gates[0];
    let gf = Frame::of_gate(&g);
    let (lo, hi) = gf.bounds_of(&site.polygon);
    let zx0 = (lo[0] + 3.0).max(-style.max[0] * 0.5);
    let zx1 = (hi[0] - 3.0).min(style.max[0] * 0.5);
    let zy0 = lo[1].max(0.0) + 2.0;
    let zy1 = (hi[1] - 3.0).min(zy0 + style.max[1]);
    let mut small = Vec::new();
    slab_tiles(
        ctx,
        &gf,
        zx0,
        zy0,
        zx1,
        zy1,
        K::Ground,
        M::Asphalt,
        &mut small,
    );
    let gz = |ctx: &Ctx, x: f32, y: f32| ctx.ground_z(gf.p(x, y));

    // Rows: modules of [row, aisle, row] stacked away from the gate.
    let module = 2.0 * BAY_D + AISLE + 0.5;
    let first = zy0 + LOT_CROSS;
    let nm = ((zy1 - RING - first) / module).floor().max(0.0) as usize;
    // (y0 of the row, back line low?)
    let mut rows: Vec<(f32, bool)> = Vec::new();
    for m in 0..nm {
        let y = first + m as f32 * module;
        rows.push((y, true));
        rows.push((y + module - BAY_D, false));
    }
    // Strips: [y0, y1] of single rows / back-to-back pairs.
    let mut strips: Vec<(f32, f32)> = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        let (y, low) = rows[i];
        if !low
            && rows
                .get(i + 1)
                .is_some_and(|q| q.1 && (q.0 - (y + BAY_D)).abs() < 1e-3)
        {
            strips.push((y, y + 2.0 * BAY_D));
            i += 2;
        } else {
            strips.push((y, y + BAY_D));
            i += 1;
        }
    }
    let occ = rng.range_f32(style.occ.0, style.occ.1);
    for side in [-1.0f32, 1.0] {
        // Row extent on this side, |x| from `inner` to `outer`.
        let inner = SPINE * 0.5 + ISLAND;
        let outer = if side > 0.0 { zx1 } else { -zx0 } - RING - ISLAND;
        if outer - inner < 4.0 * BAY_W {
            continue;
        }
        let nb = ((outer - inner) / BAY_W).floor() as usize;
        let bx = |i: usize| side * (inner + i as f32 * BAY_W);
        for &(y0, y1) in &strips {
            for i in 0..=nb {
                let x = bx(i);
                let z = gz(ctx, x, (y0 + y1) * 0.5);
                small.push(gf.span(
                    K::ParkingPaint,
                    M::PaintWhite,
                    x - 0.06,
                    y0 + 0.1,
                    x + 0.06,
                    y1 - 0.1,
                    z + 0.035,
                    z + 0.045,
                    false,
                ));
            }
            // Kerb islands at both row ends; a lamp at the spine end, a tree outside.
            let x_in = side * (inner - ISLAND * 0.5);
            let x_out = side * (inner + nb as f32 * BAY_W + ISLAND * 0.5);
            let ym = (y0 + y1) * 0.5;
            for (xi, lamp) in [(x_in, true), (x_out, false)] {
                let z = gz(ctx, xi, ym);
                sink.place(gf.span(
                    K::Ground,
                    M::Grass,
                    xi - ISLAND * 0.5 + 0.2,
                    y0 + 0.3,
                    xi + ISLAND * 0.5 - 0.2,
                    y1 - 0.3,
                    z - 0.1,
                    z + 0.15,
                    true,
                ));
                if lamp {
                    sink.place(super::ctx::cylinder(
                        K::Lamp,
                        M::Steel,
                        gf.p3(xi, ym, z + 0.15),
                        0.15,
                        9.0,
                        true,
                    ));
                } else if y1 - y0 > BAY_D + 1.0 {
                    sink.place(super::ctx::cylinder(
                        K::TreeTrunk,
                        M::Wood,
                        gf.p3(xi, ym, z + 0.15),
                        0.2,
                        3.0,
                        true,
                    ));
                    small.push(super::ctx::cylinder(
                        K::TreeCanopy,
                        M::Foliage,
                        gf.p3(xi, ym, z + 2.6),
                        2.0,
                        3.2,
                        false,
                    ));
                }
            }
        }
        // Parked cars.
        for &(y, low) in &rows {
            for i in 0..nb {
                if !rng.chance(occ) {
                    continue;
                }
                let cx = side * (inner + (i as f32 + 0.5) * BAY_W) + rng.range_f32(-0.1, 0.1);
                let gap = 0.3 + rng.range_f32(-0.1, 0.1);
                let cy = if low {
                    y + gap + CAR[1] * 0.5
                } else {
                    y + BAY_D - gap - CAR[1] * 0.5
                };
                let z = gz(ctx, cx, cy);
                small.push(gf.bx(
                    K::ParkedCar,
                    M::CarPaint,
                    cx,
                    cy,
                    z,
                    CAR,
                    rng.range_f32(-0.02, 0.02),
                    true,
                ));
            }
        }
    }
    sink.added.extend(small);
    let (k, m, t, h) = style.edge;
    sink.place_all(kit::perimeter(
        ctx,
        k,
        m,
        &site.polygon,
        t,
        h,
        10.0,
        &site.gates,
    ));
}

// ---------------------------------------------------------------------------------------
// Starlite Drive-In
// ---------------------------------------------------------------------------------------

/// Fill the drive-in site.
fn build_drive_in(ctx: &mut Ctx, site: &Site, rng: &mut Rng, sink: &mut Sink) {
    let g = site.gates[0];
    let gf = Frame::of_gate(&g);
    let (lo, hi) = gf.bounds_of(&site.polygon);
    let mut small = Vec::new();
    slab_tiles(
        ctx,
        &gf,
        lo[0] + 2.0,
        lo[1].max(0.0) + 2.0,
        hi[0] - 2.0,
        hi[1] - 2.0,
        K::Ground,
        M::Gravel,
        &mut small,
    );
    let gz = |ctx: &Ctx, x: f32, y: f32| ctx.ground_z(gf.p(x, y));
    let depth = hi[1];
    let half_w = hi[0].min(-lo[0]);

    // Screen tower near the far end (open ground behind it: SH10).
    let sy = depth - 18.0;
    let z = gz(ctx, 0.0, sy);
    sink.place(gf.span(
        K::Landmark,
        M::Brick,
        -14.0,
        sy - 2.0,
        14.0,
        sy + 2.0,
        z - 0.3,
        z + 3.5,
        true,
    ));
    sink.place(gf.span(
        K::Landmark,
        M::White,
        -12.5,
        sy - 0.4,
        12.5,
        sy + 0.4,
        z + 3.5,
        z + 15.0,
        true,
    ));
    for x in [-10.0, 0.0, 10.0] {
        sink.place(gf.span(
            K::Landmark,
            M::Steel,
            x - 0.4,
            sy + 0.4,
            x + 0.4,
            sy + 4.0,
            z - 0.3,
            z + 14.0,
            true,
        ));
    }

    // Arcs of mounds facing the screen.
    let c = [0.0, sy + 8.0];
    let mut r = 40.0;
    let booth_y = 0.35 * depth;
    while c[1] - r > booth_y + 10.0 {
        let tmax = 0.62f32;
        let dt = 5.5 / r;
        let ns = ((2.0 * tmax) / dt).floor() as usize;
        for j in 0..ns {
            let t = -tmax + (j as f32 + 0.5) * dt;
            let (st, ct) = wp_math::sin_cos(t);
            let p = [c[0] + r * st, c[1] - r * ct];
            if p[0].abs() > half_w - 12.0 {
                continue;
            }
            let v = [-st, ct];
            let at = |k: f32| [p[0] + v[0] * k, p[1] + v[1] * k];
            let (a, m, e) = (at(-2.0), at(1.0), at(4.0));
            let za = gz(ctx, a[0], a[1]);
            let zm = gz(ctx, m[0], m[1]) + 0.4;
            let ze = gz(ctx, e[0], e[1]);
            sink.place(gf.ramp(
                K::ParkingRamp,
                M::Asphalt,
                [a[0], a[1], za],
                [m[0], m[1], zm],
                4.2,
                0.6,
            ));
            sink.place(gf.ramp(
                K::ParkingRamp,
                M::Asphalt,
                [m[0], m[1], zm],
                [e[0], e[1], ze],
                4.2,
                0.6,
            ));
            if rng.chance(0.15) {
                let q = at(-2.3 - CAR[1] * 0.5);
                let yaw = yaw_of(q, m) + rng.range_f32(-0.1, 0.1);
                small.push(gf.bx(
                    K::ParkedCar,
                    M::Rust,
                    q[0],
                    q[1],
                    gz(ctx, q[0], q[1]),
                    CAR,
                    yaw,
                    true,
                ));
            }
        }
        r += 12.0;
    }

    // Projection booth / concession stand, ticket booth, lamps.
    let z = gz(ctx, 0.0, booth_y);
    sink.place(gf.span(
        K::Landmark,
        M::Brick,
        -7.0,
        booth_y - 4.0,
        7.0,
        booth_y + 4.0,
        z - 0.3,
        z + 4.0,
        true,
    ));
    let z = gz(ctx, 7.0, 9.0);
    sink.place(gf.bx(K::Gate, M::Wood, 7.0, 9.0, z, [2.2, 2.6, 2.6], 0.0, true));
    for y in [0.25 * depth, 0.55 * depth, 0.8 * depth] {
        for x in [-(half_w - 6.0), half_w - 6.0] {
            let z = gz(ctx, x, y);
            sink.place(super::ctx::cylinder(
                K::Lamp,
                M::Steel,
                gf.p3(x, y, z),
                0.15,
                8.0,
                true,
            ));
        }
    }
    sink.added.extend(small);
    sink.place_all(kit::perimeter(
        ctx,
        K::Fence,
        M::Wood,
        &site.polygon,
        0.2,
        2.2,
        10.0,
        &site.gates,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{self, Obb};
    use crate::network::FeatureShape;

    fn obb_of(f: &Feature) -> Obb {
        let FeatureShape::Box {
            centre,
            half,
            yaw,
            pitch,
        } = &f.shape
        else {
            panic!("box expected");
        };
        let (s, c) = wp_math::sin_cos(*yaw);
        let (_, cp) = wp_math::sin_cos(*pitch);
        Obb {
            c: [centre[0], centre[1]],
            f: [-s, c],
            hl: half[1] * cp,
            hw: half[0],
        }
    }

    fn rect_obb(r: [f32; 4]) -> Obb {
        Obb {
            c: [(r[0] + r[2]) * 0.5, (r[1] + r[3]) * 0.5],
            f: [0.0, 1.0],
            hl: (r[3] - r[1]) * 0.5,
            hw: (r[2] - r[0]) * 0.5,
        }
    }

    fn spec() -> GarageSpec {
        GarageSpec {
            w: 76.0,
            d: 54.5,
            decks: 5,
            gate_x: 38.0,
            z0: 10.0,
            base: 8.0,
        }
    }

    #[test]
    fn garage_cars_stay_in_bays() {
        for (w, d) in [(76.0, 54.5), (87.0, 64.5), (57.0, 72.0), (57.0, 59.5)] {
            let s = GarageSpec { w, d, ..spec() };
            let fr = Frame::new([0.0, 0.0], 0.0);
            let out = garage(&fr, &s, &mut Rng::new(3));
            let cars: Vec<&Feature> = out
                .small
                .iter()
                .filter(|f| f.kind == K::ParkedCar)
                .collect();
            assert!(cars.len() > 100, "{} cars", cars.len());
            let blockers: Vec<&Feature> = out
                .structure
                .iter()
                .filter(|f| {
                    matches!(f.kind, K::Pillar | K::Wall | K::ParkingRamp)
                        && matches!(f.shape, FeatureShape::Box { .. })
                })
                .collect();
            for c in &cars {
                let oc = obb_of(c);
                for a in &out.aisles {
                    assert!(!geom::obb_overlap(&oc, &rect_obb(*a)), "car in aisle {a:?}");
                }
                for b in &blockers {
                    if let FeatureShape::Box { centre, half, .. } = &b.shape {
                        let cz = match &c.shape {
                            FeatureShape::Box { centre, .. } => centre[2],
                            _ => unreachable!(),
                        };
                        if (centre[2] - cz).abs() > half[2] + 1.0 {
                            continue;
                        }
                    }
                    assert!(!geom::obb_overlap(&oc, &obb_of(b)), "car hits {:?}", b.kind);
                }
            }
            for (i, a) in cars.iter().enumerate() {
                for b in &cars[i + 1..] {
                    let (
                        FeatureShape::Box { centre: ca, .. },
                        FeatureShape::Box { centre: cb, .. },
                    ) = (&a.shape, &b.shape)
                    else {
                        unreachable!()
                    };
                    if (ca[2] - cb[2]).abs() < 1.0 {
                        assert!(!geom::obb_overlap(&obb_of(a), &obb_of(b)), "cars overlap");
                    }
                }
            }
            // Pillars stay out of the aisles.
            for p in out.structure.iter().filter(|f| f.kind == K::Pillar) {
                if let FeatureShape::Box { .. } = p.shape {
                    for a in &out.aisles {
                        let shrunk = [a[0] + 0.01, a[1] + 0.01, a[2] - 0.01, a[3] - 0.01];
                        assert!(!geom::obb_overlap(&obb_of(p), &rect_obb(shrunk)));
                    }
                }
            }
            // Ramps: slope ≤ 1:7, ≥ 6.5 m wide.
            for r in out.structure.iter().filter(|f| f.kind == K::ParkingRamp) {
                if let FeatureShape::Box { half, pitch, .. } = r.shape {
                    assert!(pitch.abs().tan() <= 1.0 / 7.0 + 1e-4);
                    assert!(half[0] * 2.0 >= 6.5);
                }
            }
        }
    }

    #[test]
    fn garage_is_deterministic() {
        let fr = Frame::new([10.0, 20.0], 0.7);
        let a = garage(&fr, &spec(), &mut Rng::new(9));
        let b = garage(&fr, &spec(), &mut Rng::new(9));
        assert_eq!(a.structure, b.structure);
        assert_eq!(a.small, b.small);
    }
}
