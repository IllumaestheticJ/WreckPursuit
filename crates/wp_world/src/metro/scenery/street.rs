//! Streetscape along every kerbed (non-highway) street: lamps, street trees, utility poles,
//! hydrants, benches, bins, bus shelters, billboards and sign posts before junctions.
//!
//! Stations are spaced along each paved edge with a sidewalk; a station is used when its
//! spot (on the verge, a fixed offset beyond the paved edge) is off every road's paving at
//! that height, near the ground (no furniture on bridges or embankments), away from junction
//! boxes and site gates, and not on a building, another feature, water or another module's
//! site. Each category is thinned evenly to its budget, so the real network (hundreds of km
//! of streets) stays within the city's feature budget.

use super::bounds::{billboard, lamp};
use super::nature::{tree, TreeKind};
use super::{thin, Env, Occ};
use crate::geom::{self, V2, V3};
use crate::metro::ctx::{box_on, cylinder, footprint, yaw_of};
use crate::network::{DistrictKind as D, Feature, FeatureKind as K, Material as M};
use crate::surface::surface_edges;

/// Per-category budgets (items, not features).
const CAP_LAMPS: usize = 3000;
const CAP_TREES: usize = 4000;
const CAP_POLES: usize = 1000;
const CAP_HYDRANTS: usize = 700;
const CAP_BENCHES: usize = 600;
const CAP_BINS: usize = 600;
const CAP_SHELTERS: usize = 300;
const CAP_BOARDS: usize = 150;
const CAP_SIGNS: usize = 800;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Cat {
    Shelter,
    Lamp,
    Sign,
    Tree,
    Pole,
    Hydrant,
    Bench,
    Bin,
    Board,
}

const CATS: [(Cat, usize); 9] = [
    (Cat::Shelter, CAP_SHELTERS),
    (Cat::Lamp, CAP_LAMPS),
    (Cat::Sign, CAP_SIGNS),
    (Cat::Tree, CAP_TREES),
    (Cat::Pole, CAP_POLES),
    (Cat::Hydrant, CAP_HYDRANTS),
    (Cat::Bench, CAP_BENCHES),
    (Cat::Bin, CAP_BINS),
    (Cat::Board, CAP_BOARDS),
];

/// A point at arc length `s` on a polyline: position and unit tangent.
fn at_s(e: &[V3], cum: &[f32], s: f32) -> (V3, V2) {
    let k = cum.partition_point(|x| *x <= s).clamp(1, e.len() - 1);
    let span = (cum[k] - cum[k - 1]).max(1e-6);
    let t = ((s - cum[k - 1]) / span).clamp(0.0, 1.0);
    let (a, b) = (e[k - 1], e[k]);
    let p = [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ];
    (p, geom::normalize(geom::sub(geom::xy(b), geom::xy(a))))
}

fn cumulative2(e: &[V3]) -> Vec<f32> {
    let mut c = vec![0.0];
    for w in e.windows(2) {
        let l = geom::dist(geom::xy(w[0]), geom::xy(w[1]));
        c.push(c.last().unwrap() + l);
    }
    c
}

/// Streetscape (see the module docs).
pub(super) fn streetscape(env: &Env, out: &mut Vec<Feature>) {
    let plan = env.plan();
    let arterial_names: Vec<&str> = plan.arterials.iter().map(|a| a.name).collect();
    let mut cands: Vec<(Cat, Vec<Feature>)> = Vec::new();
    for (si, s) in env.ctx.roads.built.surfaces.iter().enumerate() {
        if s.highway || s.centre.len() < 2 {
            continue;
        }
        let name = env
            .ctx
            .roads
            .names
            .get(s.road as usize)
            .map(|n| n.as_str())
            .unwrap_or("");
        let arterial = arterial_names.contains(&name) || s.left_m + s.right_m >= 12.0;
        let (l, r) = surface_edges(s);
        for side in 0..2 {
            if !s.sidewalk[side] {
                continue;
            }
            let (e, sign) = if side == 0 { (&l, -1.0) } else { (&r, 1.0) };
            let cum = cumulative2(e);
            let len = *cum.last().unwrap();
            if len < 8.0 {
                continue;
            }
            let mut used: Vec<f32> = Vec::new();
            let tag = (si * 2 + side) as i64;
            // Spot `off` m beyond the edge at arc length `st`, if usable.
            let spot = |st: f32, off: f32| -> Option<(V2, f32, V2, V2)> {
                if st < 2.0 || st > len - 2.0 {
                    return None;
                }
                let (pe, t) = at_s(e, &cum, st);
                let n = geom::scale(geom::right_of(t), sign);
                let p = geom::add(geom::xy(pe), geom::scale(n, off));
                let gz = env.gz(p);
                if (pe[2] - gz).abs() > 0.8
                    || matches!(
                        env.occ(p),
                        Occ::Building | Occ::Feature | Occ::Water | Occ::Site
                    )
                    || env.on_paving(p, pe[2], 2.5, None)
                    || env.near_junction(p, 6.0)
                    || env.near_gate(p)
                    || env.foreign_site(p)
                {
                    return None;
                }
                Some((p, gz, n, t))
            };
            let clear = |used: &[f32], st: f32, d: f32| used.iter().all(|u| (u - st).abs() >= d);
            let zone_at = |st: f32| {
                let (pe, _) = at_s(e, &cum, st.clamp(0.0, len));
                env.zone(geom::xy(pe)).map(|z| z.0)
            };
            // Bus shelters: arterials, every ~400 m per side.
            if arterial {
                let mut st = if side == 0 { 350.0 } else { 150.0 };
                while st < len - 20.0 {
                    if let Some((p, z, n, t)) = spot(st, 2.2) {
                        if clear(&used, st, 5.0) {
                            used.push(st);
                            cands.push((Cat::Shelter, shelter(p, z, n, t)));
                        }
                    }
                    st += 400.0;
                }
            }
            // Lamps: every 30 m, alternating sides.
            let mut st = if side == 0 { 5.0 } else { 20.0 };
            while st < len {
                if let Some((p, z, n, _)) = spot(st, 0.7) {
                    if clear(&used, st, 3.0) {
                        used.push(st);
                        cands.push((Cat::Lamp, lamp(p, z, 8.0, [-n[0], -n[1]], 2.0)));
                    }
                }
                st += 30.0;
            }
            // Street trees: every 12–13 m in town and residential streets.
            let mut st = 9.0;
            while st < len {
                let zk = zone_at(st);
                let step = if matches!(zk, Some(D::Residential)) {
                    13.0
                } else {
                    12.0
                };
                if matches!(zk, Some(D::Core | D::Commercial | D::Residential)) {
                    if let Some((p, z, _, _)) = spot(st, 1.8) {
                        if clear(&used, st, 3.0) {
                            used.push(st);
                            let h = env.h01(tag, (st * 4.0) as i64, 21);
                            cands.push((Cat::Tree, tree(p, z, TreeKind::Street, h)));
                        }
                    }
                }
                st += step;
            }
            // Utility poles: suburbs and industrial, right side only, every 40 m.
            if side == 1 {
                let mut st = 12.0;
                while st < len {
                    if matches!(zone_at(st), Some(D::Suburb | D::Industrial)) {
                        if let Some((p, z, n, _)) = spot(st, 2.4) {
                            if clear(&used, st, 3.0) {
                                used.push(st);
                                cands.push((Cat::Pole, pole(p, z, n)));
                            }
                        }
                    }
                    st += 40.0;
                }
            }
            // Hydrants every ~90 m (not on the industrial / port fringe).
            let mut st = if side == 0 { 40.0 } else { 85.0 };
            while st < len {
                if !matches!(zone_at(st), Some(D::Port) | None) {
                    if let Some((p, z, _, _)) = spot(st, 0.55) {
                        if clear(&used, st, 2.0) {
                            used.push(st);
                            cands.push((
                                Cat::Hydrant,
                                vec![cylinder(
                                    K::StreetFurniture,
                                    M::Red,
                                    [p[0], p[1], z],
                                    0.16,
                                    0.75,
                                    true,
                                )],
                            ));
                        }
                    }
                }
                st += 90.0;
            }
            // Benches and bins downtown and in commercial streets.
            let mut st = 35.0;
            while st < len {
                if matches!(zone_at(st), Some(D::Core | D::Commercial)) {
                    if let Some((p, z, _, t)) = spot(st, 2.3) {
                        if clear(&used, st, 2.5) {
                            used.push(st);
                            cands.push((
                                Cat::Bench,
                                vec![box_on(
                                    K::StreetFurniture,
                                    M::Wood,
                                    p,
                                    z,
                                    [0.5, 1.8, 0.5],
                                    yaw_of([0.0, 0.0], t),
                                    true,
                                )],
                            ));
                        }
                    }
                    if let Some((p, z, _, _)) = spot(st - 25.0, 0.65) {
                        if clear(&used, st - 25.0, 2.0) {
                            used.push(st - 25.0);
                            cands.push((
                                Cat::Bin,
                                vec![cylinder(
                                    K::StreetFurniture,
                                    M::DarkConcrete,
                                    [p[0], p[1], z],
                                    0.3,
                                    1.0,
                                    true,
                                )],
                            ));
                        }
                    }
                }
                st += 70.0;
            }
            // Billboards behind the sidewalk of arterials, right side.
            if arterial && side == 1 {
                let mut st = 250.0;
                while st < len - 20.0 {
                    let (pe, t) = at_s(e, &cum, st);
                    let n = geom::right_of(t);
                    let p = geom::add(geom::xy(pe), geom::scale(n, 8.0));
                    if !env.near_junction(p, 15.0) {
                        let fs = billboard(env, p, yaw_of([0.0, 0.0], n));
                        if fs.iter().all(|f| env.ctx.free(&footprint(&f.shape), 0.5)) {
                            cands.push((Cat::Board, fs));
                        }
                    }
                    st += 500.0;
                }
            }
            // Sign post before the junction at the end this side's traffic arrives at.
            let arrive_end = side == 1;
            let end_pt = if arrive_end {
                s.centre[s.centre.len() - 1]
            } else {
                s.centre[0]
            };
            if env.near_junction(geom::xy(end_pt), 4.0) {
                let st = if arrive_end { len - 4.0 } else { 4.0 };
                if let Some((p, z, _, t)) = spot(st, 0.6) {
                    if clear(&used, st, 1.5) {
                        used.push(st);
                        cands.push((Cat::Sign, sign_post(p, z, t)));
                    }
                }
            }
        }
    }
    let mut by_cat: Vec<Vec<Vec<Feature>>> = vec![Vec::new(); CATS.len()];
    for (c, fs) in cands {
        by_cat[c as usize].push(fs);
    }
    for ((_, cap), items) in CATS.iter().zip(by_cat) {
        for fs in thin(items, *cap) {
            out.extend(fs);
        }
    }
}

/// Bus shelter: solid glass box with a roof, open side towards the road.
fn shelter(p: V2, z: f32, n: V2, t: V2) -> Vec<Feature> {
    let yaw = yaw_of([0.0, 0.0], t);
    let back = geom::add(p, geom::scale(n, 0.6));
    vec![
        box_on(K::BusShelter, M::Glass, back, z, [0.3, 3.8, 2.4], yaw, true),
        box_on(
            K::BusShelter,
            M::Steel,
            geom::add(p, geom::scale(t, -1.8)),
            z,
            [1.4, 0.15, 2.4],
            yaw,
            true,
        ),
        box_on(
            K::BusShelter,
            M::Steel,
            geom::add(p, geom::scale(t, 1.8)),
            z,
            [1.4, 0.15, 2.4],
            yaw,
            true,
        ),
        box_on(
            K::BusShelter,
            M::DarkConcrete,
            p,
            z + 2.4,
            [1.7, 4.2, 0.15],
            yaw,
            false,
        ),
    ]
}

/// Wooden utility pole with a cross-arm.
fn pole(p: V2, z: f32, n: V2) -> Vec<Feature> {
    vec![
        cylinder(K::UtilityPole, M::Wood, [p[0], p[1], z], 0.15, 9.5, true),
        box_on(
            K::UtilityPole,
            M::Wood,
            p,
            z + 8.6,
            [0.12, 1.8, 0.12],
            yaw_of([0.0, 0.0], n),
            false,
        ),
    ]
}

/// Junction sign post with a plate facing approaching traffic.
fn sign_post(p: V2, z: f32, t: V2) -> Vec<Feature> {
    vec![
        cylinder(K::RoadFurniture, M::Steel, [p[0], p[1], z], 0.06, 2.9, true),
        box_on(
            K::RoadFurniture,
            M::PaintWhite,
            p,
            z + 2.1,
            [0.6, 0.05, 0.6],
            yaw_of([0.0, 0.0], t),
            false,
        ),
    ]
}
