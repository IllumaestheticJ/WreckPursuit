//! Vesper International: plan sites `airport` (airside) and `airport_landside` (terminal
//! and curbside).
//!
//! Airside layout, relative to the airside fence rectangle `[x0, y0, x1, y1]` (normally the
//! site, x −4350..−3550, y −3500..−900):
//! - runway 18L/36R, 2400 × 45 m at `x1 − 400`, full-length parallel taxiway A 160 m east;
//! - runway 18R/36L, 1600 × 45 m 250 m west of it, with taxiway B between the two and
//!   cross-field connectors to taxiway A;
//! - along the east side (south → north): cargo apron with cargo aircraft, cargo sheds and
//!   hangars; the control tower (plan landmark); a remote apron (airliners, light aircraft,
//!   helicopters); the terminal apron with three concourse piers, jet bridges and ~12
//!   airliners at gates; the fuel farm by the north gate;
//! - maintenance hangars (one with an airliner inside) on a west apron at the north end.
//!
//! The terminal (400 × 60 m, two-storey concrete hall with a glass upper level) sits along
//! the airside / landside boundary with its landside face on the curbside: it is pushed west
//! until it clears the road paving and verges by 5 m, whatever the road network looks like.
//! The fence along the boundary stops at the terminal, which closes the line itself.

use wp_math::Rng;

use super::super::ctx::{box_on, cylinder, prism, wall_along, Ctx, Occ};
use super::super::ground::bounds;
use super::super::plan::rect;
use super::*;
use crate::network::{Building, FeatureKind as K, Material as M};

/// Terminal depth (east–west), m.
const TERMINAL_DEPTH_M: f32 = 60.0;
/// Terminal length (north–south), m.
const TERMINAL_LEN_M: f32 = 400.0;
/// Concourse pier length, m.
const PIER_LEN_M: f32 = 150.0;
/// Concourse pier width, m.
const PIER_W_M: f32 = 20.0;
/// Spacing of the piers, m.
const PIER_PITCH_M: f32 = 135.0;
/// Airliners parked at the 18 pier stands.
const GATE_AIRCRAFT: usize = 12;

/// Build Vesper International (no-op if the plan has no `airport` site).
pub(super) fn build(ctx: &mut Ctx, rng: &mut Rng) {
    let plan = ctx.plan;
    let Some(site) = plan.site("airport") else {
        return;
    };
    let fr = fence_rect(ctx, site);
    let [x0, y0, x1, y1] = fr;
    let fr_poly = rect_poly(fr);
    let z = deck_z(ctx, &fr_poly);

    // --- Terminal position (landside face on the curbside) ---
    let land = plan.site("airport_landside");
    let (ty0, ty1, mut xe) = match land {
        Some(l) => {
            let (lo, hi) = bounds(&l.polygon);
            (hi[1] - 90.0 - TERMINAL_LEN_M, hi[1] - 90.0, lo[0] + 40.0)
        }
        None => (y1 - 290.0 - TERMINAL_LEN_M, y1 - 290.0, x1 + 35.0),
    };
    for _ in 0..60 {
        let poly = rect(xe - TERMINAL_DEPTH_M, ty0, xe, ty1);
        if ctx.free_of(&poly, 5.0, &[Occ::Road, Occ::Verge, Occ::Water]) {
            break;
        }
        xe -= 2.0;
    }
    let xw = xe - TERMINAL_DEPTH_M;
    let terminal_poly = rect(xw, ty0, xe, ty1);

    // --- Runways, taxiways, aprons ---
    let x_m = x1 - 400.0;
    let (ya, yb) = (y0 + 100.0, y1 - 100.0);
    let x_a = x_m + 160.0;
    let x_w = x_m - 250.0;
    let (wa, wb) = (y0 + 200.0, y0 + 1800.0);
    let x_b = x_m - 125.0;
    let hw = 22.5;
    let tw = 23.0;
    let mut f = Vec::new();
    runway(
        ctx,
        Runway {
            a: [x_m, ya],
            b: [x_m, yb],
            width: 45.0,
        },
        z,
        &fr_poly,
        &mut f,
    );
    runway(
        ctx,
        Runway {
            a: [x_w, wa],
            b: [x_w, wb],
            width: 45.0,
        },
        z,
        &fr_poly,
        &mut f,
    );
    taxiway(
        [x_a, ya + 15.0],
        [x_a, yb - 15.0],
        tw,
        z,
        0.0,
        0.0,
        true,
        &mut f,
    );
    taxiway(
        [x_b, wa + 15.0],
        [x_b, wb - 15.0],
        tw,
        z,
        0.0,
        0.0,
        true,
        &mut f,
    );
    // Main runway ↔ taxiway A.
    for y in [
        ya + 15.0,
        ya + 500.0,
        ya + 1000.0,
        ya + 1500.0,
        yb - 450.0,
        yb - 15.0,
    ] {
        taxiway([x_m, y], [x_a, y], tw, z, hw, tw * 0.5, false, &mut f);
    }
    // Cross-field: west runway → taxiway B → main runway → taxiway A.
    for y in [wa + 15.0, wa + 800.0, wb - 15.0] {
        taxiway([x_w, y], [x_b, y], tw, z, hw, tw * 0.5, false, &mut f);
        taxiway([x_b, y], [x_m, y], tw, z, tw * 0.5, hw, false, &mut f);
        taxiway([x_m, y], [x_a, y], tw, z, hw, tw * 0.5, false, &mut f);
    }
    // West runway ↔ taxiway B.
    for y in [wa + 400.0, wa + 1200.0] {
        taxiway([x_w, y], [x_b, y], tw, z, hw, tw * 0.5, false, &mut f);
    }
    let apron_x0 = x_a + tw * 0.5 - 0.5;
    let main_apron = [apron_x0, ty0 - 130.0, xw + 1.0, ty1 + 90.0];
    let remote_apron = [apron_x0, ty0 - 630.0, x1 - 50.0, ty0 - 170.0];
    let cargo_apron = [apron_x0, y0 + 100.0, x_a + 90.0, ty0 - 830.0];
    let maint_apron = [x0 + 105.0, y1 - 620.0, x_m - hw + 0.5, y1 - 180.0];
    for r in [main_apron, remote_apron, cargo_apron, maint_apron] {
        if r[2] - r[0] > 10.0 && r[3] - r[1] > 10.0 {
            f.extend(deck_rect(K::Apron, M::Concrete, r, z));
            // Apron edge line.
            for (a, b) in [([r[0], r[1]], [r[2], r[1]]), ([r[0], r[3]], [r[2], r[3]])] {
                f.extend(paint(M::PaintYellow, a, b, 0.3, z));
            }
        }
    }
    put(ctx, f);

    // --- Terminal, piers, jet bridges, gate aircraft ---
    let tg = ctx.ground_range(&terminal_poly).0;
    let mut term = Vec::new();
    let sections = 4;
    for k in 0..sections {
        let s0 = ty0 + TERMINAL_LEN_M * k as f32 / sections as f32;
        let s1 = ty0 + TERMINAL_LEN_M * (k + 1) as f32 / sections as f32;
        term.push(prism(
            K::Terminal,
            M::Concrete,
            rect(xw, s0, xe, s1),
            tg - 0.3,
            tg + 12.0,
            true,
        ));
        term.push(prism(
            K::Terminal,
            M::Glass,
            rect(xw + 5.0, s0, xe - 5.0, s1),
            tg + 12.0,
            tg + 21.0,
            true,
        ));
        term.push(prism(
            K::Terminal,
            M::DarkConcrete,
            rect(xw + 3.0, s0 + 1.0, xe - 3.0, s1 - 1.0),
            tg + 21.0,
            tg + 22.0,
            true,
        ));
    }
    put(ctx, term);
    let mut stands = Vec::new();
    for i in 0..3 {
        let yp = ty0 + 65.0 + PIER_PITCH_M * i as f32;
        let tip = xw - PIER_LEN_M;
        put(
            ctx,
            vec![
                prism(
                    K::Terminal,
                    M::Glass,
                    rect(tip, yp - PIER_W_M * 0.5, xw + 1.0, yp + PIER_W_M * 0.5),
                    z - 0.3,
                    z + 9.0,
                    true,
                ),
                prism(
                    K::Terminal,
                    M::Glass,
                    ngon([tip, yp], 14.0, 8),
                    z - 0.3,
                    z + 10.0,
                    true,
                ),
            ],
        );
        for side in [-1.0f32, 1.0] {
            for k in 0..3 {
                stands.push(([xw - 30.0 - 45.0 * k as f32, yp], side));
            }
        }
    }
    let occupied = pick(rng, stands.len(), GATE_AIRCRAFT);
    for (n, &(p, side)) in stands.iter().enumerate() {
        let busy = occupied.contains(&n);
        let sc = if busy { rng.range_f32(0.9, 1.1) } else { 1.0 };
        let yaw = if side > 0.0 {
            core::f32::consts::PI
        } else {
            0.0
        };
        let c = [p[0], p[1] + side * (PIER_W_M * 0.5 + 6.0 + 20.0 * sc)];
        let af = Frame::new(c, yaw, z);
        // Jet bridge: from the pier side out along the aircraft's left side.
        put(
            ctx,
            vec![
                af.part(
                    K::AirportFurniture,
                    M::Steel,
                    -4.0,
                    20.0 * sc + 0.75,
                    3.6,
                    [3.0, 11.5, 2.6],
                    true,
                ),
                af.cyl(
                    K::AirportFurniture,
                    M::Steel,
                    -4.0,
                    20.0 * sc - 4.0,
                    0.0,
                    0.4,
                    3.6,
                    true,
                ),
            ],
        );
        if busy {
            let liv = livery(rng);
            put(ctx, airliner(af, sc, liv));
            let fuel = rng.chance(0.4);
            put(ctx, ground_service(af, sc, fuel));
        }
    }

    // --- Remote apron: airliners, light aircraft, helicopters ---
    let ra = remote_apron;
    for k in 0..3 {
        let c = [ra[0] + 72.0, ra[1] + 30.0 + 60.0 * k as f32];
        let sc = rng.range_f32(0.95, 1.1);
        let af = Frame::new(c, -core::f32::consts::FRAC_PI_2, z);
        let liv = livery(rng);
        put(ctx, airliner(af, sc, liv));
        put(ctx, ground_service(af, sc, false));
    }
    for k in 0..5 {
        let c = [ra[0] + 28.0 + 25.0 * k as f32, ra[1] + 260.0];
        let liv = livery(rng);
        put(ctx, light_aircraft(Frame::new(c, 0.0, z), liv));
    }
    for k in 0..2 {
        let c = [ra[0] + 40.0 + 40.0 * k as f32, ra[1] + 380.0];
        f = vec![box_on(
            K::AirfieldMarking,
            M::PaintYellow,
            c,
            z,
            [16.0, 16.0, PAINT_THICK_M],
            0.0,
            false,
        )];
        f.extend(helicopter(Frame::new(c, 0.0, z), M::Blue));
        put(ctx, f);
    }

    // --- Control tower (plan landmark) ---
    let tower = plan
        .landmarks
        .iter()
        .find(|l| l.owner == "airport")
        .map(|l| l.pos)
        .unwrap_or([x1 - 50.0, ty0 - 710.0]);
    let tz = ctx.ground_z(tower);
    let (tx, ty) = (tower[0], tower[1]);
    put(
        ctx,
        vec![
            prism(
                K::ControlTower,
                M::Concrete,
                rect(tx - 10.0, ty - 10.0, tx + 10.0, ty + 10.0),
                tz - 0.3,
                tz + 8.0,
                true,
            ),
            cylinder(K::ControlTower, M::Concrete, [tx, ty, tz], 4.5, 50.0, true),
            cylinder(
                K::ControlTower,
                M::Glass,
                [tx, ty, tz + 50.0],
                8.0,
                6.0,
                true,
            ),
            cylinder(
                K::ControlTower,
                M::DarkConcrete,
                [tx, ty, tz + 56.0],
                9.0,
                1.0,
                true,
            ),
            cylinder(
                K::ControlTower,
                M::Steel,
                [tx, ty, tz + 57.0],
                0.2,
                9.0,
                true,
            ),
        ],
    );

    // --- Cargo: aircraft on the cargo apron, sheds and hangars east of it ---
    let ca = cargo_apron;
    for yc in [ca[1] + 200.0, ca[1] + 550.0] {
        let af = Frame::new([ca[0] + 39.0, yc], 0.0, z);
        put(ctx, airliner(af, 1.1, M::SafetyOrange));
        put(ctx, ground_service(af, 1.1, true));
    }
    let (bx0, bx1) = (ca[2], x1 - 50.0);
    if bx1 - bx0 > 40.0 {
        let mut y = ca[1] + 10.0;
        for (is_hangar, len) in [
            (true, 90.0),
            (false, 120.0),
            (false, 120.0),
            (true, 90.0),
            (false, 120.0),
            (true, 90.0),
            (false, 120.0),
        ] {
            if y + len > ty0 - 760.0 {
                break;
            }
            let yc = y + len * 0.5;
            if is_hangar {
                let hf = Frame::new([(bx0 + bx1) * 0.5, yc], core::f32::consts::FRAC_PI_2, z);
                put(ctx, hangar(hf, len, bx1 - bx0, 20.0, true));
            } else {
                put(
                    ctx,
                    vec![prism(
                        K::Hangar,
                        M::Concrete,
                        rect(bx0 + 5.0, y, bx1 - 5.0, y + len),
                        z - 0.3,
                        z + 12.0,
                        true,
                    )],
                );
            }
            y += len + 20.0;
        }
    }

    // --- Maintenance base: three hangars (doors east), one airliner inside, one outside ---
    let ma = maint_apron;
    for j in 0..3 {
        let c = [x0 + 70.0, y1 - 560.0 + 160.0 * j as f32];
        let hf = Frame::new(c, -core::f32::consts::FRAC_PI_2, z);
        put(ctx, hangar(hf, 90.0, 70.0, 24.0, true));
        if j == 1 {
            let liv = livery(rng);
            put(ctx, airliner(hf, 1.0, liv));
        }
    }
    let liv = livery(rng);
    put(
        ctx,
        airliner(
            Frame::new(
                [(ma[0] + ma[2]) * 0.5, (ma[1] + ma[3]) * 0.5],
                core::f32::consts::FRAC_PI_2,
                z,
            ),
            1.0,
            liv,
        ),
    );

    // --- Fuel farm by the north gate ---
    let fy = y1 - 50.0;
    let mut tanks = Vec::new();
    for k in 0..3 {
        let c = [x1 - 150.0 + 35.0 * k as f32, fy];
        tanks.push(cylinder(
            K::Tank,
            M::White,
            [c[0], c[1], ctx.ground_z(c) - 0.2],
            12.0,
            14.0,
            true,
        ));
    }
    let bund = [
        [x1 - 170.0, fy - 18.0],
        [x1 - 60.0, fy - 18.0],
        [x1 - 60.0, fy + 18.0],
        [x1 - 170.0, fy + 18.0],
        [x1 - 170.0, fy - 18.0],
    ];
    tanks.extend(wall_along(
        ctx,
        K::Wall,
        M::Concrete,
        &bund,
        0.4,
        1.2,
        40.0,
        &[],
        0.0,
    ));
    put(ctx, tanks);

    // --- Navaids: glide-slope sheds, windsocks ---
    for y in [ya + 300.0, yb - 300.0] {
        let g = ctx.ground_z([x_m + 105.0, y]);
        let sf = Frame::new([x_m + 105.0, y], 0.0, g);
        put(
            ctx,
            vec![
                sf.part(
                    K::AirportFurniture,
                    M::White,
                    0.0,
                    0.0,
                    0.0,
                    [3.0, 4.0, 3.0],
                    true,
                ),
                sf.cyl(K::AirportFurniture, M::Red, 3.0, 0.0, 0.0, 0.25, 9.0, true),
            ],
        );
    }
    for p in [
        [x_m + 80.0, yb - 200.0],
        [x_m + 80.0, ya + 200.0],
        [x_w - 60.0, (wa + wb) * 0.5],
    ] {
        let g = ctx.ground_z(p);
        put(ctx, windsock(Frame::new(p, 0.6, g)));
    }

    // --- Perimeter fence and gates ---
    let gates = fence_gates(site, fr);
    let fence = fence_loop(ctx, fr, &gates, std::slice::from_ref(&terminal_poly));
    for piece in fence {
        ctx.place(piece);
    }
    for (p, h, w) in gates {
        gate_kit(ctx, p, h, w);
    }

    // --- Landside: curbside canopy, bollards, rental office and hotel ---
    if let Some(l) = land {
        landside(ctx, l, [xw, ty0, xe, ty1], tg);
    }
}

/// Curbside canopy (non-solid, high over the curb road), bollards along the terminal's
/// landside face, and two landside buildings where they are clear of roads and other sites.
fn landside(ctx: &mut Ctx, land: &super::super::plan::Site, term: Rect, tg: f32) {
    let [_, ty0, xe, ty1] = term;
    for k in 0..4 {
        let s0 = ty0 + TERMINAL_LEN_M * k as f32 / 4.0;
        ctx.add(box_on(
            K::Terminal,
            M::Steel,
            [xe + 8.0, s0 + TERMINAL_LEN_M / 8.0],
            tg + 9.0,
            [16.0, TERMINAL_LEN_M / 4.0, 0.4],
            0.0,
            false,
        ));
    }
    let mut y = ty0 + 2.0;
    while y <= ty1 - 2.0 {
        let p = [xe + 3.0, y];
        let b = cylinder(
            K::Bollard,
            M::Steel,
            [p[0], p[1], ctx.ground_z(p) - 0.2],
            0.15,
            1.2,
            true,
        );
        put(ctx, vec![b]);
        y += 4.0;
    }
    let (lo, hi) = bounds(&land.polygon);
    let own = ["airport", "airport_landside"];
    for (r, h) in [
        (
            rect(lo[0] + 110.0, hi[1] - 400.0, lo[0] + 170.0, hi[1] - 320.0),
            30.0,
        ),
        (
            rect(hi[0] - 180.0, lo[1] + 360.0, hi[0] - 140.0, lo[1] + 390.0),
            6.0,
        ),
    ] {
        if ctx.free_in_site(&r, 3.0) && outside_other_sites(ctx, &r, &own) {
            let base = ctx.ground_range(&r).0;
            ctx.add_building(Building {
                footprint: r,
                height_m: h,
                base_z_m: base,
            });
        }
    }
}
