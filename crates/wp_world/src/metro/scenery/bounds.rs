//! Boundaries: water edges, cliff tops, highway edges, the map edge and the road closures
//! where roads and rail leave the map.

use super::contour::{self, lerp, Grid};
use super::{Env, Occ};
use crate::geom::{self, V2};
use crate::metro::ctx::{box_on, cylinder, footprint, yaw_of};
use crate::metro::plan::{self, MAP_HI, MAP_LO};
use crate::network::{DistrictKind, Feature, FeatureKind as K, Material as M};
use crate::surface::surface_edges;

/// Inset of the map-edge wall line from the map boundary, m.
pub(super) const EDGE_INSET_M: f32 = 1.5;
/// Distance from the map boundary at which a road leaving the map is closed, m.
pub(super) const CLOSURE_INSET_M: f32 = 14.0;

/// Style of a straight barrier piece.
#[derive(Clone, Copy)]
pub(super) struct Style {
    pub kind: K,
    pub mat: M,
    pub thick: f32,
    pub height: f32,
}

/// A wall piece `a → b` standing on the ground: 0.3 m below the lowest ground under it,
/// `height` above the highest; overlaps its neighbours a little so joints stay closed.
pub(super) fn ground_piece(env: &Env, st: Style, a: V2, b: V2) -> Feature {
    let m = lerp(a, b, 0.5);
    let za = [env.gz(a), env.gz(b), env.gz(m)];
    let lo = za.iter().fold(f32::MAX, |x, y| x.min(*y)) - 0.3;
    let hi = za.iter().fold(f32::MIN, |x, y| x.max(*y)) + st.height;
    let overlap = st.thick.min(0.6);
    box_on(
        st.kind,
        st.mat,
        m,
        lo,
        [st.thick, geom::dist(a, b) + overlap, hi - lo],
        yaw_of(a, b),
        true,
    )
}

/// A barrier band of half width `half` around `p` (normal `n`) is clear of roads,
/// buildings, other features and other modules' sites (verge and water are fine).
pub(super) fn band_ok(env: &Env, p: V2, n: V2, half: f32) -> bool {
    let w = half + 0.2;
    let steps = ((2.0 * w) / 0.8).ceil().max(1.0) as usize;
    (0..=steps).all(|k| {
        let o = -w + 2.0 * w * k as f32 / steps as f32;
        let q = [p[0] + n[0] * o, p[1] + n[1] * o];
        !matches!(env.occ(q), Occ::Road | Occ::Building | Occ::Feature) && !env.foreign_site(q)
    })
}

/// Barriers along every bank: quay walls in town and along the walled canal, guard rails by
/// roads in the suburbs, rock revetment on natural banks. Traced from the terrain at the
/// water line (+0.1 m), on the land side; gaps where a road, bridge deck, building, pier or
/// another module's site meets the water; none on the South Shore beach (the beach park is
/// fenced from the road instead).
pub(super) fn water_edges(env: &Env, out: &mut Vec<Feature>) {
    let t = env.ctx.terrain;
    let g = Grid {
        nx: t.nx as usize,
        ny: t.ny as usize,
        origin: t.origin,
        cell: t.cell_m,
        v: &t.z,
    };
    let beach = env
        .plan()
        .site("south_shore_park")
        .map(|s| s.polygon.as_slice());
    for line in contour::contours(&g, plan::WATER_Z + 0.1) {
        for (a, b) in contour::pieces(&line, 0.5, 9.0) {
            let d = geom::normalize(geom::sub(b, a));
            let land = [-d[1], d[0]];
            let m = lerp(a, b, 0.5);
            if beach.is_some_and(|poly| crate::metro::ground::inside(poly, m)) {
                continue;
            }
            let st = water_style(env, m);
            let off = st.thick * 0.5 + 0.1;
            let (a2, b2) = (
                geom::add(a, geom::scale(land, off)),
                geom::add(b, geom::scale(land, off)),
            );
            for (p, q) in contour::runs(a2, b2, 0.8, 1.2, |s| band_ok(env, s, land, st.thick * 0.5))
            {
                if env.near_barrier(lerp(p, q, 0.5), 1.0) {
                    continue;
                }
                let mut st = st;
                if st.kind == K::Rock {
                    let h = env.h01((m[0] * 4.0) as i64, (m[1] * 4.0) as i64, 11);
                    st.height = 1.2 + 0.7 * h;
                }
                out.push(ground_piece(env, st, p, q));
            }
        }
    }
}

fn water_style(env: &Env, m: V2) -> Style {
    let canal = env.plan().channels.iter().any(|c| {
        c.walled && crate::metro::ground::polyline_dist(&c.line, m) < c.half_width_m + 14.0
    });
    if canal {
        return Style {
            kind: K::QuayWall,
            mat: M::Concrete,
            thick: 0.6,
            height: 1.1,
        };
    }
    match env.zone(m).map(|z| z.0) {
        Some(DistrictKind::Core | DistrictKind::Commercial | DistrictKind::Port) => Style {
            kind: K::QuayWall,
            mat: M::Concrete,
            thick: 0.5,
            height: 1.0,
        },
        Some(DistrictKind::Industrial) => Style {
            kind: K::QuayWall,
            mat: M::DarkConcrete,
            thick: 0.6,
            height: 1.0,
        },
        _ if env.ctx.road_distance(m, 30.0) < 30.0 => Style {
            kind: K::GuardRail,
            mat: M::Steel,
            thick: 0.3,
            height: 0.9,
        },
        _ => Style {
            kind: K::Rock,
            mat: M::DarkConcrete,
            thick: 2.2,
            height: 1.5,
        },
    }
}

/// Slope magnitude (rise / run) at every terrain vertex.
fn slope_field(env: &Env) -> Vec<f32> {
    let t = env.ctx.terrain;
    let (nx, ny) = (t.nx as usize, t.ny as usize);
    let mut s = vec![0.0; nx * ny];
    let c2 = 2.0 * t.cell_m;
    for j in 1..ny - 1 {
        for i in 1..nx - 1 {
            let gx = (t.z[j * nx + i + 1] - t.z[j * nx + i - 1]) / c2;
            let gy = (t.z[(j + 1) * nx + i] - t.z[(j - 1) * nx + i]) / c2;
            s[j * nx + i] = (gx * gx + gy * gy).sqrt();
        }
    }
    s
}

/// Walls / guard rails along the top of every real cliff (slope above 35° within 30 m of
/// the crest and a drop of more than 8 m): the Bayview Bluffs above the bay.
pub(super) fn cliffs(env: &Env, out: &mut Vec<Feature>) {
    let t = env.ctx.terrain;
    let slope = slope_field(env);
    let g = Grid {
        nx: t.nx as usize,
        ny: t.ny as usize,
        origin: t.origin,
        cell: t.cell_m,
        v: &slope,
    };
    // Crest: where the slope passes 20°; steep side on the left.
    for line in contour::contours(&g, 0.36) {
        for (a, b) in contour::pieces(&line, 0.8, 10.0) {
            let d = geom::normalize(geom::sub(b, a));
            let flat = geom::right_of(d);
            let steep = [-flat[0], -flat[1]];
            let m = lerp(a, b, 0.5);
            let top = env.gz(geom::add(m, geom::scale(flat, 3.0)));
            let bottom = env.gz(geom::add(m, geom::scale(steep, 30.0)));
            if top - bottom < 8.0 || env.ctx.is_water(geom::add(m, geom::scale(flat, 3.0))) {
                continue;
            }
            let max_slope = (1..=6)
                .map(|k| {
                    let q = geom::add(m, geom::scale(steep, 5.0 * k as f32));
                    let n = t.normal_at(q);
                    (1.0 - n[2] * n[2]).max(0.0).sqrt() / n[2].max(1e-3)
                })
                .fold(0.0, f32::max);
            if max_slope < 0.7 {
                continue;
            }
            let st = if env.ctx.road_distance(m, 30.0) < 30.0 {
                Style {
                    kind: K::GuardRail,
                    mat: M::Steel,
                    thick: 0.3,
                    height: 0.9,
                }
            } else {
                Style {
                    kind: K::Wall,
                    mat: M::DarkConcrete,
                    thick: 0.5,
                    height: 1.0,
                }
            };
            let off = 1.0;
            let (a2, b2) = (
                geom::add(a, geom::scale(flat, off)),
                geom::add(b, geom::scale(flat, off)),
            );
            for (p, q) in contour::runs(a2, b2, 0.8, 1.2, |s| band_ok(env, s, flat, st.thick * 0.5))
            {
                if env.near_barrier(lerp(p, q, 0.5), 1.0) {
                    continue;
                }
                out.push(ground_piece(env, st, p, q));
            }
        }
    }
}

/// Highway edges: on every outer paved edge of a highway surface (not the median, not an
/// edge facing another carriageway or ramp at the same height), a guard rail where the road
/// is more than 2 m above or below the ground beside it (embankment, deck, cutting), a 4.5 m
/// sound wall where it runs at grade next to housing, a 2 m fence elsewhere; tall lamps
/// every ~70 m and billboards every ~900 m on the outer verge.
pub(super) fn highways(env: &Env, out: &mut Vec<Feature>) {
    let surfs = &env.ctx.roads.built.surfaces;
    let mut lamps = Vec::new();
    let mut boards = Vec::new();
    for (si, s) in surfs.iter().enumerate() {
        if !s.highway || s.centre.len() < 2 {
            continue;
        }
        let (l, r) = surface_edges(s);
        for side in 0..2 {
            if side == 0 && s.median_barrier {
                continue;
            }
            let (e, sign) = if side == 0 { (&l, -1.0) } else { (&r, 1.0) };
            let mut walked = 0.0;
            let mut next_lamp = 35.0;
            let mut next_board = 450.0;
            for k in 0..e.len() - 1 {
                let (a3, b3) = (e[k], e[k + 1]);
                let len = geom::dist(geom::xy(a3), geom::xy(b3));
                if len < 0.05 {
                    continue;
                }
                let n_sub = (len / 10.0).ceil().max(1.0) as usize;
                for sub in 0..n_sub {
                    let t0 = sub as f32 / n_sub as f32;
                    let t1 = (sub + 1) as f32 / n_sub as f32;
                    let a = lerp(geom::xy(a3), geom::xy(b3), t0);
                    let b = lerp(geom::xy(a3), geom::xy(b3), t1);
                    let za = a3[2] + (b3[2] - a3[2]) * t0;
                    let zb = a3[2] + (b3[2] - a3[2]) * t1;
                    let d = geom::normalize(geom::sub(b, a));
                    let n = geom::scale(geom::right_of(d), sign);
                    let m = lerp(a, b, 0.5);
                    let ze = 0.5 * (za + zb);
                    let piece_len = geom::dist(a, b);
                    walked += piece_len;
                    let interior = [1.0, 3.0, 6.0, 10.0].iter().any(|o| {
                        env.on_paving(geom::add(m, geom::scale(n, *o)), ze, 1.5, Some(si))
                    });
                    if interior {
                        continue;
                    }
                    let dz = ze - env.gz(geom::add(m, geom::scale(n, 4.0)));
                    let elevated = dz.abs() > 2.0;
                    if elevated {
                        guard_rail(env, si, (a, za), (b, zb), n, out);
                    } else {
                        at_grade_edge(env, a, b, n, out);
                    }
                    if walked >= next_lamp {
                        next_lamp += 70.0;
                        lamps.push((m, n, if elevated { ze } else { f32::NAN }));
                    }
                    if walked >= next_board {
                        next_board += 900.0;
                        if !elevated {
                            boards.push((m, n));
                        }
                    }
                }
            }
        }
    }
    // Lamps on the verge of at-grade stretches only (raised ones have rails, not verge),
    // clear of every lane like the street lamps.
    for (m, n, ze) in lamps {
        if !ze.is_nan() {
            continue;
        }
        // On the shoulder side of the fence if the shoulder is wide enough, else behind it.
        for off in [1.2, 3.2] {
            let p = geom::add(m, geom::scale(n, off));
            if matches!(env.occ(p), Occ::Building)
                || env.foreign_site(p)
                || env.near_junction(p, super::JUNCTION_CLEAR_M)
            {
                continue;
            }
            let fs = lamp(p, env.gz(p), 12.0, [-n[0], -n[1]], 1.5 + off);
            if env.clear_of_lanes(&fs) {
                out.extend(fs);
                break;
            }
        }
    }
    for (m, n) in boards {
        let p = geom::add(m, geom::scale(n, 11.0));
        let fs = billboard(env, p, yaw_of([0.0, 0.0], n));
        let ok = fs.iter().all(|f| env.ctx.free(&footprint(&f.shape), 0.5));
        if ok {
            out.extend(fs);
        }
    }
}

/// Guard rail just outside a raised / sunk highway edge, at road height.
fn guard_rail(env: &Env, si: usize, a: (V2, f32), b: (V2, f32), n: V2, out: &mut Vec<Feature>) {
    let off = 0.35;
    let (p0, q0) = (
        geom::add(a.0, geom::scale(n, off)),
        geom::add(b.0, geom::scale(n, off)),
    );
    let z = |p: V2| {
        let t = (geom::dist(a.0, p) / geom::dist(a.0, b.0).max(1e-3)).clamp(0.0, 1.0);
        a.1 + (b.1 - a.1) * t
    };
    for (p, q) in contour::runs(p0, q0, 0.8, 1.2, |s| {
        let zs = z(s);
        !env.on_paving(s, zs, 1.5, Some(si))
            && !matches!(env.occ(s), Occ::Building)
            && (zs - env.gz(s) > 3.4 || !matches!(env.occ(s), Occ::Road))
    }) {
        if env.near_barrier(lerp(p, q, 0.5), 0.8) {
            continue;
        }
        let (zp, zq) = (z(p), z(q));
        let lo = zp.min(zq) - 0.25;
        let hi = zp.max(zq) + 0.85;
        out.push(box_on(
            K::GuardRail,
            M::Steel,
            lerp(p, q, 0.5),
            lo,
            [0.3, geom::dist(p, q) + 0.3, hi - lo],
            yaw_of(p, q),
            true,
        ));
    }
}

/// Sound wall (housing beside) or fence (elsewhere) on the verge of an at-grade edge.
fn at_grade_edge(env: &Env, a: V2, b: V2, n: V2, out: &mut Vec<Feature>) {
    let m = lerp(a, b, 0.5);
    let housing = matches!(
        env.zone(geom::add(m, geom::scale(n, 25.0))).map(|z| z.0),
        Some(DistrictKind::Residential | DistrictKind::Suburb)
    );
    let st = if housing {
        Style {
            kind: K::SoundWall,
            mat: M::Concrete,
            thick: 0.4,
            height: 4.5,
        }
    } else {
        Style {
            kind: K::Fence,
            mat: M::Steel,
            thick: 0.15,
            height: 2.0,
        }
    };
    let off = 2.2;
    let (p0, q0) = (
        geom::add(a, geom::scale(n, off)),
        geom::add(b, geom::scale(n, off)),
    );
    for (p, q) in contour::runs(p0, q0, 0.8, 1.2, |s| {
        band_ok(env, s, n, st.thick * 0.5) && !env.ctx.is_water(s)
    }) {
        if env.near_barrier(lerp(p, q, 0.5), 0.8) {
            continue;
        }
        out.push(ground_piece(env, st, p, q));
    }
}

/// A street lamp: solid pole `h` tall at `p` (ground `z`) and an emissive head on an arm
/// `arm_len` long towards `arm` (unit).
pub(super) fn lamp(p: V2, z: f32, h: f32, arm: V2, arm_len: f32) -> Vec<Feature> {
    let head = geom::add(p, geom::scale(arm, arm_len * 0.5));
    vec![
        cylinder(K::Lamp, M::Steel, [p[0], p[1], z], 0.12, h, true),
        box_on(
            K::Lamp,
            M::Light,
            head,
            z + h - 0.25,
            [0.35, arm_len + 0.3, 0.25],
            yaw_of(p, geom::add(p, arm)),
            false,
        ),
    ]
}

/// A billboard facing `yaw` (two solid legs, a non-solid board 6–9 m up).
pub(super) fn billboard(env: &Env, p: V2, yaw: f32) -> Vec<Feature> {
    let z = env.gz(p);
    let (s, c) = wp_math::sin_cos(yaw);
    let side = [c, s];
    let l1 = geom::add(p, geom::scale(side, -3.0));
    let l2 = geom::add(p, geom::scale(side, 3.0));
    vec![
        cylinder(K::Billboard, M::Steel, [l1[0], l1[1], z], 0.25, 9.0, true),
        cylinder(K::Billboard, M::Steel, [l2[0], l2[1], z], 0.25, 9.0, true),
        box_on(
            K::Billboard,
            M::White,
            p,
            z + 6.0,
            [9.0, 0.4, 3.2],
            yaw,
            false,
        ),
    ]
}

/// Map-edge walls on land along the whole map boundary (1.5 m inside it): a dense tree
/// line in the wooded districts, a 4 m concrete wall elsewhere. Gaps only over water and
/// where a road crosses (closed by [`closures`]).
pub(super) fn map_edge(env: &Env, out: &mut Vec<Feature>) {
    let i = EDGE_INSET_M;
    let (x0, y0, x1, y1) = (MAP_LO[0] + i, MAP_LO[1] + i, MAP_HI[0] - i, MAP_HI[1] - i);
    let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    for e in 0..4 {
        let (a, b) = (corners[e], corners[(e + 1) % 4]);
        let d = geom::normalize(geom::sub(b, a));
        let inward = [-d[1], d[0]];
        let len = geom::dist(a, b);
        let n = (len / 10.0).ceil() as usize;
        for k in 0..n {
            let p0 = lerp(a, b, k as f32 / n as f32);
            let q0 = lerp(a, b, (k + 1) as f32 / n as f32);
            let m = lerp(p0, q0, 0.5);
            let wooded = matches!(
                env.zone(geom::add(m, geom::scale(inward, 20.0))),
                Some((
                    _,
                    "Westridge Woods" | "North Hollins" | "Northfield Airfield"
                ))
            );
            let st = if wooded {
                Style {
                    kind: K::MapEdge,
                    mat: M::Foliage,
                    thick: 3.0,
                    height: 8.0,
                }
            } else {
                Style {
                    kind: K::MapEdge,
                    mat: M::DarkConcrete,
                    thick: 1.0,
                    height: 4.0,
                }
            };
            // Centre the band so its outer face sits on the inset line.
            let off = st.thick * 0.5;
            let (p1, q1) = (
                geom::add(p0, geom::scale(inward, off - 0.5)),
                geom::add(q0, geom::scale(inward, off - 0.5)),
            );
            for (p, q) in contour::runs(p1, q1, 0.8, 1.2, |s| {
                let w = st.thick * 0.5 + 0.2;
                [-w, 0.0, w].iter().all(|o| {
                    let r = geom::add(s, geom::scale(inward, *o));
                    !matches!(env.occ(r), Occ::Road) && !env.ctx.is_water(r)
                })
            }) {
                out.push(ground_piece(env, st, p, q));
            }
        }
    }
}

/// Where a crossing point of `line` with the closure rectangle lies, walking towards its
/// last point: point, direction (towards the edge), index of the segment.
pub(super) fn leave_point(line: &[V2]) -> Option<(V2, V2, usize)> {
    let c = CLOSURE_INSET_M;
    let ins = |p: V2| {
        p[0] > MAP_LO[0] + c && p[0] < MAP_HI[0] - c && p[1] > MAP_LO[1] + c && p[1] < MAP_HI[1] - c
    };
    let last = *line.last()?;
    if ins(last) || !ins(line[0]) {
        return None;
    }
    for k in 1..line.len() {
        if !ins(line[k]) {
            let (a, b) = (line[k - 1], line[k]);
            // Bisect for the crossing.
            let (mut lo, mut hi) = (0.0f32, 1.0f32);
            for _ in 0..24 {
                let t = 0.5 * (lo + hi);
                if ins(lerp(a, b, t)) {
                    lo = t;
                } else {
                    hi = t;
                }
            }
            let p = lerp(a, b, lo);
            return Some((p, geom::normalize(geom::sub(b, a)), k - 1));
        }
    }
    None
}

/// Distance from `p` along `d` to the map boundary.
fn to_boundary(p: V2, d: V2) -> f32 {
    let mut t = f32::MAX;
    for ax in 0..2 {
        if d[ax] > 1e-4 {
            t = t.min((MAP_HI[ax] - p[ax]) / d[ax]);
        } else if d[ax] < -1e-4 {
            t = t.min((MAP_LO[ax] - p[ax]) / d[ax]);
        }
    }
    t
}

/// Road closures where a road leaves the map (the only features this module puts on road
/// paving): a line of Jersey barriers across the road 14 m inside the map edge, a
/// "road closed" board, and a tunnel portal (side walls, roof, dark back face at the edge)
/// so the road reads as continuing off-map. Rail lines leaving the map get the portal
/// without the barriers (the map-edge wall closes them behind it).
pub(super) fn closures(env: &Env, out: &mut Vec<Feature>) {
    for s in &env.ctx.roads.built.surfaces {
        if s.centre.len() < 2 {
            continue;
        }
        let fwd: Vec<V2> = s.centre.iter().map(|p| geom::xy(*p)).collect();
        let back: Vec<V2> = fwd.iter().rev().copied().collect();
        for (dir_fwd, line) in [(true, &fwd), (false, &back)] {
            let Some((c, d, k)) = leave_point(line) else {
                continue;
            };
            let kk = if dir_fwd { k } else { s.centre.len() - 2 - k };
            let z = 0.5 * (s.centre[kk][2] + s.centre[kk + 1][2]);
            let (left, right) = if dir_fwd {
                (s.left_m, s.right_m)
            } else {
                (s.right_m, s.left_m)
            };
            closure(env, c, z, d, left, right, true, out);
        }
    }
    for rl in &env.plan().rail {
        let fwd = rl.line.clone();
        let back: Vec<V2> = fwd.iter().rev().copied().collect();
        for line in [&fwd, &back] {
            if let Some((c, d, _)) = leave_point(line) {
                let w = rl.tracks as f32 * 2.5 + 2.0;
                let z = env.gz(c);
                closure(env, c, z, d, w, w, false, out);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn closure(
    env: &Env,
    c: V2,
    z: f32,
    d: V2,
    left: f32,
    right: f32,
    barriers: bool,
    out: &mut Vec<Feature>,
) {
    let r = geom::right_of(d);
    let at =
        |along: f32, lat: f32| geom::add(c, geom::add(geom::scale(d, along), geom::scale(r, lat)));
    let yaw_across = yaw_of([0.0, 0.0], r);
    let yaw_along = yaw_of([0.0, 0.0], d);
    if barriers {
        let (l0, l1) = (-left - 0.8, right + 0.8);
        let n = ((l1 - l0) / 3.2).ceil().max(1.0) as usize;
        let seg = (l1 - l0) / n as f32;
        for k in 0..n {
            let lat = l0 + seg * (k as f32 + 0.5);
            out.push(box_on(
                K::JerseyBarrier,
                M::Concrete,
                at(0.0, lat),
                z - 0.05,
                [0.6, seg + 0.05, 0.9],
                yaw_across,
                true,
            ));
        }
        // "Road closed" board on two posts just behind the barriers.
        let b = at(1.2, 0.5 * (right - left));
        for o in [-1.1, 1.1] {
            let q = geom::add(b, geom::scale(r, o));
            out.push(cylinder(
                K::RoadFurniture,
                M::Steel,
                [q[0], q[1], z],
                0.05,
                2.2,
                true,
            ));
        }
        out.push(box_on(
            K::RoadFurniture,
            M::SafetyOrange,
            b,
            z + 1.1,
            [0.1, 2.8, 1.0],
            yaw_across,
            true,
        ));
    }
    // Portal from 2 m behind the barrier line to the map edge.
    let reach = to_boundary(c, d).clamp(4.0, 40.0);
    let (s0, s1) = (2.0, reach - 0.3);
    let len = s1 - s0;
    let mid = 0.5 * (s0 + s1);
    let zb = env.gz(at(mid, 0.0)).min(z) - 0.5;
    let top = z + 7.5;
    for lat in [-left - 1.6, right + 1.6] {
        out.push(box_on(
            K::Tunnel,
            M::DarkConcrete,
            at(mid, lat),
            zb,
            [1.2, len, top - zb],
            yaw_along,
            true,
        ));
    }
    let width = left + right + 4.4;
    let centre_lat = 0.5 * (right - left);
    out.push(box_on(
        K::Tunnel,
        M::Concrete,
        at(mid, centre_lat),
        z + 6.5,
        [width, len, 1.0],
        yaw_along,
        true,
    ));
    out.push(box_on(
        K::Tunnel,
        M::Black,
        at(s1 - 0.4, centre_lat),
        zb,
        [width, 0.6, top - zb],
        yaw_along,
        true,
    ));
}
