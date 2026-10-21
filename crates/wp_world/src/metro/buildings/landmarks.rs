//! Landmark complexes in the plan's `Landmark` sites: Vesper Bowl, Founders Plaza & City
//! Hall, Calder Power Station, the Old Vesper Steel Mill, Hollins Knob Observatory, Northfield
//! Mall, Midtown Galleria and Calder Point Lighthouse.
//!
//! Each complex is laid out in an axis-aligned frame at an anchor inside its site. The
//! anchor is searched (nearest first) so the main masses clear whatever roads cross the
//! site; every other part is then placed where it is clear and skipped where it is not, so
//! the layouts survive a changing road network.

use wp_math::Rng;

use super::super::ctx::Ctx;
use super::super::ground::bounds;
use super::super::plan::{Site, SiteKind};
use super::parts::{all_free, bx, chamfer, commit, cyl, fence, mass, rect, Frame, Mode, Part};
use crate::geom::V2;
use crate::network::{FeatureKind as K, Material as M};

/// Build every landmark site the plan reserves for this module.
pub fn generate(ctx: &mut Ctx, rng: &mut Rng) {
    let plan = ctx.plan;
    for site in plan.sites_of(SiteKind::Landmark) {
        let (lo, hi) = bounds(&site.polygon);
        let centre = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5];
        match site.id {
            "vesper_bowl" => bowl(ctx, site, centre),
            "founders_plaza" => founders(ctx, site, centre),
            "calder_station" => calder(ctx, site, centre),
            "steel_mill" => steel_mill(ctx, site, centre, rng),
            "observatory" => observatory(ctx, site, centre),
            "northfield_mall" => mall(ctx, site, centre, false),
            "midtown_mall" => mall(ctx, site, centre, true),
            "lighthouse" => lighthouse(ctx, site, centre),
            _ => generic(ctx, site, centre),
        }
    }
}

/// Nearest anchor (searched on a 5 m grid within `reach`) where `main` is clear.
fn anchor(ctx: &Ctx, site: &Site, base: V2, main: &[Part], reach: V2) -> Option<V2> {
    let mut offs: Vec<(f32, V2)> = Vec::new();
    let (nx, ny) = ((reach[0] / 5.0) as i32, (reach[1] / 5.0) as i32);
    for j in -ny..=ny {
        for i in -nx..=nx {
            let o = [i as f32 * 5.0, j as f32 * 5.0];
            offs.push((o[0] * o[0] + o[1] * o[1], o));
        }
    }
    offs.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then(a.1[1].total_cmp(&b.1[1]))
            .then(a.1[0].total_cmp(&b.1[0]))
    });
    offs.into_iter()
        .map(|(_, o)| [base[0] + o[0], base[1] + o[1]])
        .find(|c| all_free(ctx, &Frame::axis(*c), main, Mode::Site(&site.polygon)))
}

/// Fence along the site polygon, open at the site's gates.
fn perimeter(site: &Site, frame: &Frame, spec: super::parts::FenceSpec) -> Vec<Part> {
    let local = |p: V2| [p[0] - frame.o[0], p[1] - frame.o[1]];
    let gaps: Vec<V2> = site.gates.iter().map(|g| local(g.pos)).collect();
    let gap_m = site
        .gates
        .iter()
        .map(|g| g.width_m * 0.5 + 1.0)
        .fold(7.0, f32::max);
    let n = site.polygon.len();
    (0..n)
        .map(|k| Part::Line {
            a: local(site.polygon[k]),
            b: local(site.polygon[(k + 1) % n]),
            spec,
            gaps: gaps.clone(),
            gap_m,
        })
        .collect()
}

/// Stack of cylinders (`(radius, height)` from the bottom) at `c`; only the base is checked.
fn stack(parts: &mut Vec<Part>, c: V2, rings: &[(f32, f32)], kind: K, mat: M, z0: f32) {
    let mut z = z0;
    for (k, (r, h)) in rings.iter().enumerate() {
        parts.push(Part::Cyl {
            c,
            r: *r,
            z0: z,
            h: *h,
            kind,
            mat,
            solid: true,
            check: k == 0,
        });
        z += h;
    }
}

/// Hemisphere of radius `r` as `n` stacked rings (radius, height).
fn dome(r: f32, n: usize) -> Vec<(f32, f32)> {
    let h = r / n as f32;
    (0..n)
        .map(|k| {
            let z = (k as f32 + 0.5) / n as f32;
            ((r * (1.0 - z * z).sqrt()).max(0.3), h)
        })
        .collect()
}

fn ellipse(a: f32, b: f32, t: f32) -> V2 {
    let (s, c) = wp_math::sin_cos(t);
    [a * c, b * s]
}

fn bowl(ctx: &mut Ctx, site: &Site, centre: V2) {
    // Inner (pitch side), middle (tier break) and outer ellipses.
    let (a0, b0, a1, b1, a2, b2) = (66.0, 48.0, 88.0, 70.0, 108.0, 90.0);
    let gate = site.gates.first().map(|g| g.pos);
    let main = vec![Part::Flat {
        c: [0.0, 0.0],
        du: 96.0,
        dv: 60.0,
        mat: M::Grass,
    }];
    let c = anchor(ctx, site, centre, &main, [20.0, 20.0]).unwrap_or(centre);
    let frame = Frame::axis(c);
    let tg = gate.map_or(-core::f32::consts::FRAC_PI_2, |g| {
        wp_math::atan2(g[1] - c[1], g[0] - c[0])
    });
    let n = 32;
    let dt = core::f32::consts::TAU / n as f32;
    let mut parts = main;
    for k in 0..n {
        let t0 = tg + (k as f32 - 0.5) * dt;
        let t1 = t0 + dt;
        if k == 0 {
            continue; // the gate: drive in through the stands onto the pitch
        }
        let ring = |a: f32, b: f32, a2: f32, b2: f32| {
            vec![
                ellipse(a, b, t0),
                ellipse(a, b, t1),
                ellipse(a2, b2, t1),
                ellipse(a2, b2, t0),
            ]
        };
        parts.push(Part::Prism {
            poly: ring(a0, b0, a1, b1),
            z0: 0.0,
            h: 9.0,
            kind: K::Landmark,
            mat: M::Concrete,
            solid: true,
        });
        parts.push(Part::Prism {
            poly: ring(a1, b1, a2, b2),
            z0: 0.0,
            h: 22.0,
            kind: K::Landmark,
            mat: M::Concrete,
            solid: true,
        });
        // Cantilever roof over the side opposite the gate.
        let mid = t0 + dt * 0.5;
        if wp_math::sin_cos(mid - tg).1 < -0.5 {
            parts.push(Part::Prism {
                poly: ring(a1 - 8.0, b1 - 8.0, a2, b2),
                z0: 24.0,
                h: 1.2,
                kind: K::Landmark,
                mat: M::Steel,
                solid: false,
            });
        }
    }
    // Arch over the gate opening.
    let g = ellipse(a2 + 1.5, b2 + 1.5, tg);
    let (sg, cg) = wp_math::sin_cos(tg);
    let (du, dv) = if sg.abs() > cg.abs() {
        (22.0, 3.0)
    } else {
        (3.0, 22.0)
    };
    parts.push(Part::Box {
        c: g,
        du,
        dv,
        z0: 7.0,
        h: 3.0,
        kind: K::Landmark,
        mat: M::Concrete,
        solid: true,
        check: false,
    });
    // Floodlight towers.
    for k in 0..4 {
        let t = core::f32::consts::FRAC_PI_4 + k as f32 * core::f32::consts::FRAC_PI_2;
        let p = ellipse(a2 + 8.0, b2 + 8.0, t);
        parts.push(cyl(p, 1.2, 0.0, 48.0, K::Landmark, M::Steel));
        parts.push(Part::Box {
            c: p,
            du: 8.0,
            dv: 2.0,
            z0: 48.0,
            h: 4.0,
            kind: K::Landmark,
            mat: M::Light,
            solid: false,
            check: false,
        });
    }
    // Concourse paving around the bowl (not over the pitch).
    let (lo, hi) = bounds(&site.polygon);
    let tile = 40.0;
    let mut y = lo[1] + tile * 0.5;
    while y < hi[1] {
        let mut x = lo[0] + tile * 0.5;
        while x < hi[0] {
            let l = [x - c[0], y - c[1]];
            let corners = [
                [l[0] - 20.0, l[1] - 20.0],
                [l[0] + 20.0, l[1] - 20.0],
                [l[0] + 20.0, l[1] + 20.0],
                [l[0] - 20.0, l[1] + 20.0],
            ];
            let in_e = |p: &V2, a: f32, b: f32| (p[0] / a).powi(2) + (p[1] / b).powi(2) < 1.0;
            if !corners.iter().any(|p| in_e(p, a0, b0)) {
                parts.push(Part::Flat {
                    c: l,
                    du: tile,
                    dv: tile,
                    mat: M::Concrete,
                });
            }
            x += tile;
        }
        y += tile;
    }
    parts.extend(perimeter(site, &frame, fence(M::Steel, 2.4)));
    commit(ctx, &frame, &parts, Mode::Site(&site.polygon));
}

fn founders(ctx: &mut Ctx, site: &Site, centre: V2) {
    // City hall on the north side of the plaza, facing south over the paving.
    let main = vec![
        mass(-40.0, 40.0, 8.0, 44.0, 20.0),
        mass(-70.0, -40.0, 12.0, 40.0, 15.0),
        mass(40.0, 70.0, 12.0, 40.0, 15.0),
    ];
    let c = anchor(ctx, site, centre, &main, [80.0, 10.0]).unwrap_or(centre);
    let frame = Frame::axis(c);
    let mut parts = main;
    for k in 0..8 {
        let u = -21.0 + 6.0 * k as f32;
        parts.push(cyl([u, 5.0], 0.9, 0.0, 14.0, K::Landmark, M::White));
    }
    let lintel = |c: V2, du: f32, dv: f32, z0: f32, h: f32| Part::Box {
        c,
        du,
        dv,
        z0,
        h,
        kind: K::Landmark,
        mat: M::White,
        solid: true,
        check: false,
    };
    parts.push(lintel([0.0, 5.5], 48.0, 5.0, 14.0, 3.0));
    parts.push(lintel([0.0, 5.5], 30.0, 4.0, 17.0, 2.5));
    parts.push(bx(
        [0.0, 1.5],
        50.0,
        3.0,
        0.0,
        0.6,
        K::Landmark,
        M::Concrete,
    ));
    // Drum, dome, lantern on the main block.
    let mut dz = 0.0;
    let mut rings = vec![(11.0, 6.0)];
    rings.extend(dome(10.5, 5));
    rings.push((1.6, 4.0));
    for (r, h) in rings {
        parts.push(Part::RoofCyl {
            mass: 0,
            c: [0.0, 26.0],
            dz,
            r,
            h,
            kind: K::Landmark,
            mat: M::White,
            solid: false,
        });
        dz += h;
    }
    // Plaza paving (tiles; skipped where a road or the trench is).
    for j in 0..3 {
        for i in 0..13 {
            parts.push(Part::Flat {
                c: [-120.0 + 20.0 * i as f32, -48.0 + 20.0 * j as f32],
                du: 20.0,
                dv: 20.0,
                mat: M::Concrete,
            });
        }
    }
    // Fountain.
    parts.push(cyl([0.0, -30.0], 9.0, 0.0, 0.8, K::Monument, M::Concrete));
    parts.push(cyl([0.0, -30.0], 3.0, 0.0, 1.8, K::Monument, M::Concrete));
    parts.push(Part::Cyl {
        c: [0.0, -30.0],
        r: 0.4,
        z0: 1.8,
        h: 5.0,
        kind: K::Monument,
        mat: M::White,
        solid: false,
        check: false,
    });
    // Flagpoles.
    for (u, m) in [(-12.0, M::Red), (0.0, M::Blue), (12.0, M::White)] {
        parts.push(cyl([u, -8.0], 0.15, 0.0, 18.0, K::Landmark, M::Steel));
        parts.push(Part::Box {
            c: [u + 0.9, -8.0],
            du: 1.8,
            dv: 0.06,
            z0: 16.5,
            h: 1.2,
            kind: K::Landmark,
            mat: m,
            solid: false,
            check: false,
        });
    }
    commit(ctx, &frame, &parts, Mode::Site(&site.polygon));
}

fn calder(ctx: &mut Ctx, site: &Site, centre: V2) {
    let main = vec![
        mass(-60.0, 50.0, -10.0, 32.0, 28.0),
        mass(50.0, 100.0, -16.0, 36.0, 46.0),
    ];
    let c = anchor(ctx, site, centre, &main, [20.0, 20.0]).unwrap_or(centre);
    let frame = Frame::axis(c);
    let mut parts = main;
    // Two hyperbolic cooling towers: r(z) = rt * sqrt(1 + ((z - zt) / k)^2).
    let (hgt, rt, rb) = (105.0f32, 22.0f32, 34.0f32);
    let zt = 0.78 * hgt;
    let k = zt / ((rb / rt).powi(2) - 1.0).sqrt();
    let slices = 10;
    let hs = hgt / slices as f32;
    let rings: Vec<(f32, f32)> = (0..slices)
        .map(|i| {
            let z = (i as f32 + 0.5) * hs;
            (rt * (1.0 + ((z - zt) / k).powi(2)).sqrt(), hs)
        })
        .collect();
    for p in [[-130.0, -55.0], [-130.0, 60.0]] {
        stack(&mut parts, p, &rings, K::Landmark, M::Concrete, 0.0);
    }
    // Chimney with a red band.
    parts.push(cyl(
        [120.0, -50.0],
        6.5,
        0.0,
        150.0,
        K::Landmark,
        M::Concrete,
    ));
    parts.push(Part::Cyl {
        c: [120.0, -50.0],
        r: 6.8,
        z0: 140.0,
        h: 2.0,
        kind: K::Landmark,
        mat: M::Red,
        solid: false,
        check: false,
    });
    // Coal and ash piles (stepped mounds).
    stack(
        &mut parts,
        [20.0, -95.0],
        &[(24.0, 3.0), (16.0, 3.0), (8.0, 3.0)],
        K::Earthwork,
        M::Black,
        0.0,
    );
    stack(
        &mut parts,
        [80.0, -102.0],
        &[(18.0, 2.5), (11.0, 2.5), (5.0, 2.5)],
        K::Earthwork,
        M::Black,
        0.0,
    );
    stack(
        &mut parts,
        [-60.0, -110.0],
        &[(14.0, 2.0), (9.0, 2.0), (4.0, 2.0)],
        K::Earthwork,
        M::Gravel,
        0.0,
    );
    // Coal conveyor to the boiler house on two trestles.
    parts.push(Part::Ramp {
        a: [45.0, -75.0],
        za: 4.0,
        b: [72.0, -18.0],
        zb: 32.0,
        w: 3.0,
        kind: K::Landmark,
        mat: M::Rust,
        solid: false,
    });
    for t in [0.33f32, 0.66] {
        let p = [45.0 + 27.0 * t, -75.0 + 57.0 * t];
        parts.push(cyl(p, 0.6, 0.0, 4.0 + 28.0 * t - 1.0, K::Pillar, M::Steel));
    }
    // Transformer yard (fenced inside the station fence).
    for (u, v) in [
        (135.0, 82.0),
        (150.0, 82.0),
        (165.0, 82.0),
        (135.0, 98.0),
        (150.0, 98.0),
        (165.0, 98.0),
    ] {
        parts.push(bx([u, v], 5.0, 4.0, 0.0, 4.0, K::Landmark, M::Steel));
    }
    for (u, v) in [(128.0, 75.0), (172.0, 75.0), (128.0, 105.0), (172.0, 105.0)] {
        parts.push(cyl([u, v], 0.3, 0.0, 12.0, K::UtilityPole, M::Steel));
    }
    let yard = rect(124.0, 176.0, 70.0, 110.0);
    for k in 0..4 {
        parts.push(Part::Line {
            a: yard[k],
            b: yard[(k + 1) % 4],
            spec: fence(M::Steel, 2.4),
            gaps: vec![[150.0, 70.0]],
            gap_m: 4.0,
        });
    }
    parts.extend(perimeter(site, &frame, fence(M::Steel, 2.8)));
    commit(ctx, &frame, &parts, Mode::Site(&site.polygon));
}

/// An open rusty shed: roof panels (one may be missing), columns, walls with gaps.
#[allow(clippy::too_many_arguments)]
fn ruined_shed(parts: &mut Vec<Part>, rng: &mut Rng, u0: f32, u1: f32, v0: f32, v1: f32, h: f32) {
    let rust = |c: V2, du: f32, dv: f32, z0: f32, hh: f32, check: bool| Part::Box {
        c,
        du,
        dv,
        z0,
        h: hh,
        kind: K::Landmark,
        mat: M::Rust,
        solid: true,
        check,
    };
    let panels = 3;
    let pw = (u1 - u0) / panels as f32;
    let missing = rng.below(panels + 1);
    for k in 0..panels {
        if k == missing {
            continue;
        }
        let u = u0 + pw * (k as f32 + 0.5);
        parts.push(rust([u, (v0 + v1) * 0.5], pw, v1 - v0, h - 0.6, 0.6, true));
    }
    // Columns every ~12 m along both long sides.
    let n = ((u1 - u0) / 12.0).ceil().max(1.0) as usize;
    for k in 0..=n {
        let u = u0 + (u1 - u0) * k as f32 / n as f32;
        for v in [v0, v1] {
            parts.push(cyl([u, v], 0.4, 0.0, h - 0.6, K::Pillar, M::Rust));
        }
    }
    // Wall panels between the columns, about half of them gone.
    for k in 0..n {
        let u = u0 + (u1 - u0) * (k as f32 + 0.5) / n as f32;
        let len = (u1 - u0) / n as f32 - 1.0;
        for v in [v0, v1] {
            if rng.chance(0.5) {
                parts.push(rust([u, v], len, 0.3, 0.0, h - 1.0, true));
            }
        }
    }
    for u in [u0, u1] {
        if rng.chance(0.5) {
            let len = (v1 - v0) * rng.range_f32(0.3, 0.8);
            parts.push(rust(
                [u, v0 + len * 0.5 + 0.5],
                0.3,
                len,
                0.0,
                h - 1.0,
                true,
            ));
        }
    }
}

fn steel_mill(ctx: &mut Ctx, site: &Site, centre: V2, rng: &mut Rng) {
    let main = vec![mass(-80.0, -40.0, -25.0, 3.0, 12.0)];
    let c = anchor(ctx, site, centre, &main, [20.0, 10.0]).unwrap_or(centre);
    let frame = Frame::axis(c);
    let mut parts = main;
    // Blast furnace, downcomer, dust catcher, hot-blast stoves.
    stack(
        &mut parts,
        [-60.0, 15.0],
        &[(10.0, 14.0), (8.0, 22.0), (5.5, 8.0), (3.0, 6.0)],
        K::Landmark,
        M::Rust,
        0.0,
    );
    parts.push(cyl([-47.0, 24.0], 1.2, 0.0, 48.0, K::Landmark, M::Rust));
    stack(
        &mut parts,
        [-30.0, 32.0],
        &[(4.0, 18.0), (2.0, 4.0)],
        K::Landmark,
        M::Rust,
        0.0,
    );
    for v in [17.0, 31.0, 45.0] {
        stack(
            &mut parts,
            [-100.0, v],
            &[(5.5, 34.0), (3.5, 2.0)],
            K::Landmark,
            M::Rust,
            0.0,
        );
    }
    ruined_shed(&mut parts, rng, 0.0, 70.0, 10.0, 46.0, 14.0);
    ruined_shed(&mut parts, rng, 15.0, 70.0, -70.0, -40.0, 11.0);
    ruined_shed(&mut parts, rng, 85.0, 140.0, -40.0, 50.0, 16.0);
    // Conveyor gantry from the ore shed to the furnace top, on trestles.
    let (a, b) = ([90.0, 20.0], [-52.0, 18.0]);
    parts.push(Part::Ramp {
        a,
        za: 17.0,
        b,
        zb: 44.0,
        w: 3.0,
        kind: K::Landmark,
        mat: M::Rust,
        solid: false,
    });
    for t in [0.25f32, 0.5, 0.75] {
        let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        parts.push(cyl(p, 0.6, 0.0, 16.0 + 27.0 * t, K::Pillar, M::Rust));
    }
    // Slag heap.
    stack(
        &mut parts,
        [-125.0, -60.0],
        &[(14.0, 3.0), (9.0, 3.0), (4.0, 3.0)],
        K::Earthwork,
        M::Gravel,
        0.0,
    );
    parts.extend(perimeter(site, &frame, fence(M::Steel, 2.4)));
    commit(ctx, &frame, &parts, Mode::Site(&site.polygon));
}

fn observatory(ctx: &mut Ctx, site: &Site, centre: V2) {
    // North half of the site; the south half stays free for the lot (SH06).
    let main = vec![
        cyl([0.0, 22.0], 11.0, 0.0, 9.0, K::Landmark, M::Concrete),
        mass(-44.0, -14.0, 16.0, 28.0, 6.0),
    ];
    let c = anchor(ctx, site, centre, &main, [15.0, 10.0]).unwrap_or(centre);
    let frame = Frame::axis(c);
    let mut parts = main;
    stack(
        &mut parts,
        [0.0, 22.0],
        &dome(11.0, 5),
        K::Landmark,
        M::White,
        9.0,
    );
    for (dz, r, h) in [(0.0, 3.0, 2.5), (2.5, 2.0, 1.2)] {
        parts.push(Part::RoofCyl {
            mass: 1,
            c: [-36.0, 22.0],
            dz,
            r,
            h,
            kind: K::Landmark,
            mat: M::White,
            solid: false,
        });
    }
    commit(ctx, &frame, &parts, Mode::Site(&site.polygon));
}

fn mall(ctx: &mut Ctx, site: &Site, centre: V2, galleria: bool) {
    let (lo, hi) = bounds(&site.polygon);
    let half = [(hi[0] - lo[0]) * 0.5, (hi[1] - lo[1]) * 0.5];
    for s in [1.0f32, 0.85, 0.7, 0.55] {
        let main = if galleria {
            vec![
                mass(-75.0 * s, 75.0 * s, -32.0, 32.0, 22.0),
                bx([0.0, -35.0], 16.0, 6.0, 0.0, 12.0, K::Landmark, M::Glass),
            ]
        } else {
            vec![
                mass(-95.0 * s, 95.0 * s, -30.0, 45.0, 14.0),
                bx([0.0, -33.0], 12.0, 6.0, 0.0, 10.0, K::Landmark, M::Glass),
            ]
        };
        let reach = [(half[0] - 100.0 * s).max(0.0), 5.0];
        let Some(c) = anchor(ctx, site, centre, &main, reach) else {
            continue;
        };
        let frame = Frame::axis(c);
        let mut parts = main;
        let glass = |poly: Vec<V2>, dz: f32, h: f32| Part::Crown {
            mass: 0,
            poly,
            dz,
            h,
            kind: K::Landmark,
            mat: M::Glass,
            solid: false,
        };
        if galleria {
            parts.push(glass(rect(-60.0 * s, 60.0 * s, -8.0, 8.0), 0.0, 6.0));
            parts.push(glass(rect(-50.0 * s, 50.0 * s, -5.0, 5.0), 6.0, 4.0));
            parts.push(Part::Mass {
                poly: chamfer(75.0 * s, 75.0 * s + 22.0, -24.0, 0.0, 3.0),
                h: 42.0,
            });
            parts.push(bx([0.0, 35.0], 16.0, 6.0, 0.0, 12.0, K::Landmark, M::Glass));
            parts.push(bx(
                [-45.0 * s, -35.0],
                10.0,
                6.0,
                0.0,
                9.0,
                K::Landmark,
                M::Glass,
            ));
        } else {
            parts.push(glass(rect(-20.0 * s, 20.0 * s, -10.0, 25.0), 0.0, 8.0));
            for u in [-50.0 * s, 50.0 * s] {
                parts.push(bx([u, -33.0], 12.0, 6.0, 0.0, 10.0, K::Landmark, M::Glass));
            }
            // Anchor stores at both ends.
            parts.push(mass(-95.0 * s - 50.0, -95.0 * s, -40.0, 50.0, 18.0));
            parts.push(mass(95.0 * s, 95.0 * s + 50.0, -40.0, 50.0, 18.0));
            for u in [-60.0 * s, 40.0 * s] {
                parts.push(Part::Crown {
                    mass: 0,
                    poly: rect(u, u + 10.0, 25.0, 33.0),
                    dz: 0.0,
                    h: 2.5,
                    kind: K::Landmark,
                    mat: M::Steel,
                    solid: false,
                });
            }
        }
        commit(ctx, &frame, &parts, Mode::Site(&site.polygon));
        return;
    }
}

fn lighthouse(ctx: &mut Ctx, site: &Site, centre: V2) {
    let main = vec![cyl([6.0, 6.0], 4.2, 0.0, 6.0, K::Landmark, M::White)];
    let c = anchor(ctx, site, centre, &main, [15.0, 15.0]).unwrap_or(centre);
    let frame = Frame::axis(c);
    let mut parts = Vec::new();
    let bands = [4.2, 3.9, 3.6, 3.3, 3.0];
    let mut z = 0.0;
    for (k, r) in bands.iter().enumerate() {
        parts.push(Part::Cyl {
            c: [6.0, 6.0],
            r: *r,
            z0: z,
            h: 6.0,
            kind: K::Landmark,
            mat: if k % 2 == 0 { M::White } else { M::Red },
            solid: true,
            check: k == 0,
        });
        z += 6.0;
    }
    for (r, h, mat) in [
        (3.6, 0.5, M::Steel),
        (2.2, 3.5, M::Light),
        (2.6, 1.2, M::Red),
    ] {
        parts.push(Part::Cyl {
            c: [6.0, 6.0],
            r,
            z0: z,
            h,
            kind: K::Landmark,
            mat,
            solid: false,
            check: false,
        });
        z += h;
    }
    parts.push(mass(-20.0, -6.0, -16.0, -6.0, 6.0));
    parts.push(Part::Crown {
        mass: parts.len() - 1,
        poly: rect(-19.0, -7.0, -15.0, -7.0),
        dz: 0.0,
        h: 1.5,
        kind: K::Landmark,
        mat: M::DarkConcrete,
        solid: false,
    });
    commit(ctx, &frame, &parts, Mode::Site(&site.polygon));
}

/// A landmark site this module does not know by id: one civic block in the middle.
fn generic(ctx: &mut Ctx, site: &Site, centre: V2) {
    let (lo, hi) = bounds(&site.polygon);
    let (w, d) = ((hi[0] - lo[0]) * 0.5, (hi[1] - lo[1]) * 0.5);
    let main = vec![mass(-w * 0.5, w * 0.5, -d * 0.5, d * 0.5, 18.0)];
    if let Some(c) = anchor(ctx, site, centre, &main, [w * 0.4, d * 0.4]) {
        commit(ctx, &Frame::axis(c), &main, Mode::Site(&site.polygon));
    }
}
