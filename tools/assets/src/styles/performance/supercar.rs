//! Wave-3 supercars: `halberd_40` (2020s carbon-tub TT V8), `ombra_v12` (2010s V12 flagship),
//! `ostwind_6t` (2020s rear-engine flat-six) and `kagero_hx` (2010s hybrid V6).
//!
//! Like the wave-2 exotics, the mid-engine lattices carry a front-engine cabin, so the
//! cab-forward look comes from `cabin_shift_m`. The rear-engine Ostwind keeps a classic
//! long-roof fastback (small shift). Mechanical parts come from `mechanical::spec` (table);
//! the styles here only add caliper / spring colours on top of it.
//!
//! Each car also has its own silhouette (owner 2026-10-26, "the exotic cars all basically look
//! like each other"): Halberd a rounded body under a teardrop canopy (`roof_arc`,
//! `c_pillar_rake_deg`) with a fastback tail; Ombra a very low, flat, sharp-cornered wedge with
//! a long shallow engine cover; Ostwind a tall one-arc fastback roof with fender tops standing
//! above a low front lid (`hood_valley`) and wide hips; Kagero a notchback with an upright rear
//! glass between flying buttresses and a rising belt.
//!
//! Each car has its own body kit and its own night identifier (tail-lamp pattern):
//! - Halberd: one hair-thin full-width LED line over an exposed black rear mesh.
//! - Ombra: a double chevron at each corner in an angular black housing.
//! - Ostwind: a full-width red bar through a black band, three stepped dashes per side.
//! - Kagero: a thin full-width bar whose ends fold down into a Z at each corner.

// Kit code takes explicit coordinates (x0, x1, z0, z1, …), as in `kit.rs`.
#![allow(clippy::too_many_arguments)]

use std::collections::BTreeSet;

use wp_contracts::{BodyRegion, VehicleDef};

use crate::builder::{Builder, PartKey};
use crate::mesh::{MaterialSlot as M, PartKind, INDICATOR_SEAM_M};
use crate::profile::RING_GLASS_TOP;
use crate::style::{
    ArchShape, BodyStyle, CapKind, CarStyle, DetailCtx, RimKind, SideSculpt, Silhouette,
};

use super::kit::{
    dkey, headlight_key, indicator_front_key, indicator_rear_key, lkey, sided, taillight_key,
    top_band, Kit, Stand, Wing, SIDES,
};
use super::{kit_detail, rgb, wheels};

/// A straight stroke from `a` to `c` lying on a surface with normal `n`: `width` is the
/// visible width across the stroke, `depth` how far it stands along the normal.
pub(super) fn stroke(
    k: &mut Kit,
    key: &PartKey,
    a: [f32; 3],
    c: [f32; 3],
    n: [f32; 3],
    width: f32,
    depth: f32,
) {
    k.plate(key, a, c, n, depth, width);
}

/// A stroke on the nose face from (x, z) `a` to `c`, standing `proud` out of the face.
pub(super) fn front_stroke(
    k: &mut Kit,
    key: &PartKey,
    a: [f32; 2],
    c: [f32; 2],
    width: f32,
    proud: f32,
) {
    let y = k.yn + 0.5 * (proud - 0.015);
    let depth = proud + 0.015;
    stroke(
        k,
        key,
        [a[0], y, a[1]],
        [c[0], y, c[1]],
        [0.0, 1.0, 0.0],
        width,
        depth,
    );
}

/// A stroke on the tail face from (x, z) `a` to `c`, standing `proud` out of the face.
pub(super) fn rear_stroke(
    k: &mut Kit,
    key: &PartKey,
    a: [f32; 2],
    c: [f32; 2],
    width: f32,
    proud: f32,
) {
    let y = k.yt - 0.5 * (proud - 0.015);
    let depth = proud + 0.015;
    stroke(
        k,
        key,
        [a[0], y, a[1]],
        [c[0], y, c[1]],
        [0.0, -1.0, 0.0],
        width,
        depth,
    );
}

/// A stroke on the top surface from (x, y) `a` to `c`, `lift` m above the surface.
pub(super) fn top_stroke(
    k: &mut Kit,
    key: &PartKey,
    a: [f32; 2],
    c: [f32; 2],
    width: f32,
    lift: f32,
) {
    let pa = [a[0], a[1], k.top_z(a[1], a[0]) + lift];
    let pc = [c[0], c[1], k.top_z(c[1], c[0]) + lift];
    let n = k.p.top_point(0.5 * (a[1] + c[1]), 0.5 * (a[0] + c[0])).1;
    stroke(k, key, pa, pc, n, width, 0.012);
}

// ------------------------------------------------------------------ shaped-panel helpers
//
// Local to the exotic / supercar styles (phase 2 of the car-shapes pass): flat outlines that
// follow the loft, so lamps, intakes and vents can take their real shapes instead of boxes.
// Outlines must be star-shaped round their vertex mean (every vertex visible from it); edges
// are subdivided so the panel drapes over curved surfaces.

/// Outline `pts` with every edge split into pieces of at most `step` m.
fn subdivide(pts: &[[f32; 2]], step: f32) -> Vec<[f32; 2]> {
    let n = pts.len();
    let mut out = Vec::new();
    for i in 0..n {
        let a = pts[i];
        let c = pts[(i + 1) % n];
        let len = ((c[0] - a[0]).powi(2) + (c[1] - a[1]).powi(2)).sqrt();
        let m = ((len / step).ceil() as usize).max(1);
        for j in 0..m {
            let t = j as f32 / m as f32;
            out.push([a[0] + (c[0] - a[0]) * t, a[1] + (c[1] - a[1]) * t]);
        }
    }
    out
}

/// Mean of an outline.
fn mean2(pts: &[[f32; 2]]) -> [f32; 2] {
    let n = pts.len().max(1) as f32;
    let s = pts
        .iter()
        .fold([0.0, 0.0], |s, q| [s[0] + q[0], s[1] + q[1]]);
    [s[0] / n, s[1] / n]
}

/// Places an outline point (2-D) at an offset from a surface (see [`drape`]).
type Place<'a> = &'a dyn Fn(&Kit, [f32; 2], f32) -> [f32; 3];

/// Draped thin shell: outline `o` (2-D, already subdivided) placed by `at(q, offset)` onto a
/// surface. The face is built from concentric rings (`rings`: scale toward the outline's
/// mean, outer first, starting at 1.0) and a centre fan, every
/// ring point placed on the surface, so the panel follows curved and recessed surfaces inside
/// its outline too; the back is one fan `thick` below, the rim joins the outer edges. Faces are
/// turned along `up` (face), against it (back) or away from the middle (rim).
fn drape(
    k: &mut Kit,
    key: &PartKey,
    o: &[[f32; 2]],
    off: f32,
    thick: f32,
    up: [f32; 3],
    rings: &[f32],
    at: Place,
) {
    let n = o.len();
    let c = mean2(o);
    let toward = |q: [f32; 2], t: f32| [c[0] + (q[0] - c[0]) * t, c[1] + (q[1] - c[1]) * t];
    let rings: Vec<Vec<[f32; 3]>> = rings
        .iter()
        .map(|&t| o.iter().map(|&q| at(k, toward(q, t), off)).collect())
        .collect();
    let ct = at(k, c, off);
    let bot: Vec<[f32; 3]> = o.iter().map(|&q| at(k, q, off - thick)).collect();
    let cb = at(k, c, off - thick);
    let down = [-up[0], -up[1], -up[2]];
    let mid = [
        0.5 * (ct[0] + cb[0]),
        0.5 * (ct[1] + cb[1]),
        0.5 * (ct[2] + cb[2]),
    ];
    for i in 0..n {
        let j = (i + 1) % n;
        for r in 0..rings.len() - 1 {
            let (a, b) = (&rings[r], &rings[r + 1]);
            k.b.poly_facing(key, &[a[i], a[j], b[j]], up);
            k.b.poly_facing(key, &[a[i], b[j], b[i]], up);
        }
        let last = &rings[rings.len() - 1];
        k.b.poly_facing(key, &[ct, last[i], last[j]], up);
        k.b.poly_facing(key, &[cb, bot[j], bot[i]], down);
        let top = &rings[0];
        let q = [top[i], top[j], bot[j], bot[i]];
        let e = [
            0.25 * (q[0][0] + q[1][0] + q[2][0] + q[3][0]),
            0.25 * (q[0][1] + q[1][1] + q[2][1] + q[3][1]),
            0.25 * (q[0][2] + q[1][2] + q[2][2] + q[3][2]),
        ];
        k.b.poly_facing(key, &q, [e[0] - mid[0], e[1] - mid[1], e[2] - mid[2]]);
    }
}

/// Panel lying on the top surface (hood, fender tops, deck): outline `pts` in chassis (x, y),
/// its face `lift` m above the paint and `thick` m deep.
pub(super) fn top_poly(k: &mut Kit, key: &PartKey, pts: &[[f32; 2]], lift: f32, thick: f32) {
    let o = subdivide(pts, 0.05);
    let at = |k: &Kit, q: [f32; 2], d: f32| [q[0], q[1], k.top_z(q[1], q[0]) + d];
    drape(k, key, &o, lift, thick, [0.0, 0.0, 1.0], &[1.0], &at);
}

/// Point `off` m out from the side surface of side `sx` at (y, z), and the outward normal.
fn side_at(k: &Kit, sx: f32, y: f32, z: f32, off: f32) -> ([f32; 3], [f32; 3]) {
    let (pt, n) = k.p.side_point(y, z);
    let (pt, n) = if sx > 0.0 {
        ([-pt[0], pt[1], pt[2]], [-n[0], n[1], n[2]])
    } else {
        (pt, n)
    };
    ([pt[0] + n[0] * off, pt[1], pt[2] + n[2] * off], n)
}

/// Extra lift of every [`side_poly`] panel over the loft's facets, m.
const SIDE_LIFT_M: f32 = 0.010;

/// Panel on the body side of side `sx` (key given for the left side; the right one is
/// derived): outline `pts` in (y, z), its face `off` m out of the paint, `thick` m deep.
pub(super) fn side_poly(
    k: &mut Kit,
    key_left: &PartKey,
    sx: f32,
    pts: &[[f32; 2]],
    off: f32,
    thick: f32,
) {
    let key = sided(key_left, sx);
    let o = subdivide(pts, 0.05);
    let n = side_at(k, sx, mean2(&o)[0], mean2(&o)[1], 0.0).1;
    let at = move |k: &Kit, q: [f32; 2], d: f32| side_at(k, sx, q[0], q[1], d).0;
    // The loft's flat facets sit up to ≈ 1 cm off the smooth surface on sculpted sides; lift
    // every side panel by that much (stacking order between panels unchanged).
    drape(
        k,
        &key,
        &o,
        off + SIDE_LIFT_M,
        thick + SIDE_LIFT_M,
        n,
        &[1.0, 0.5],
        &at,
    );
}

/// y of the nose (`front`) or tail face at (x, z): the end face where the plan-rounded
/// corners and the falling top leave room for the point.
pub(super) fn face_y(k: &Kit, front: bool, x: f32, z: f32) -> f32 {
    let p = k.p;
    let inside = |y: f32| {
        let ring = p.ring(y);
        let lo = ring.iter().map(|q| q.p[2]).fold(f32::MAX, f32::min);
        let hi = ring.iter().map(|q| q.p[2]).fold(f32::MIN, f32::max);
        let zs = z.clamp(lo + 0.005, (hi - 0.005).max(lo + 0.005));
        z >= lo - 0.002 && z <= hi + 0.002 && x.abs() <= k.side_x(y, zs) + 0.002
    };
    let (mut a, mut b) = if front {
        (k.yn, k.yn - 0.6)
    } else {
        (k.yt, k.yt + 0.6)
    };
    // `a` outside (the end plane), `b` inside: bisect.
    if inside(a) {
        return a;
    }
    for _ in 0..16 {
        let m = 0.5 * (a + b);
        if inside(m) {
            b = m;
        } else {
            a = m;
        }
    }
    b
}

/// Panel on the nose (`front`) or tail face: outline `pts` in (x, z), its face `proud` m out
/// of the face and `depth` m deep, following the plan-rounded corners.
pub(super) fn end_poly(
    k: &mut Kit,
    key: &PartKey,
    front: bool,
    pts: &[[f32; 2]],
    proud: f32,
    depth: f32,
) {
    let d = if front { 1.0 } else { -1.0 };
    let o = subdivide(pts, 0.06);
    // On the vertical end plane; [`shear_to_faces`] then moves it onto the real face (plan
    // corners, rake, chin), like every other kit part built on the face.
    let y0 = if front { k.yn } else { k.yt };
    let at = move |_: &Kit, q: [f32; 2], off: f32| [q[0], y0 + d * off, q[1]];
    drape(k, key, &o, proud, depth, [0.0, d, 0.0], &[1.0], &at);
}

/// Names of the parts that already have geometry (call right after [`Kit::new`]), so
/// [`shear_to_faces`] only moves what the kit itself builds.
pub(super) fn built_parts(k: &Kit) -> BTreeSet<String> {
    k.b.keys()
        .into_iter()
        .filter(|key| k.b.geometry(&key.name).is_some_and(|g| !g.0.is_empty()))
        .map(|key| key.name)
        .collect()
}

/// Move every detail / light part the kit built on the vertical nose or tail plane (vertices
/// within 3 cm of it) back onto the real end face: the plan-rounded corners, the face rake and
/// the chin undercut. Parts in `pre` (built before the kit) and names starting with any of
/// `skip` (lamps already cast onto the face) stay.
pub(super) fn shear_to_faces(k: &mut Kit, pre: &BTreeSet<String>, skip: &[&str]) {
    // Set-back tables over (x, z), 4 cm × 2 cm cells, bilinear.
    const NX: usize = 56;
    const NZ: usize = 60;
    let (x0, dx, z0, dz) = (-1.12, 0.04, 0.0, 0.02);
    let table = |front: bool| -> Vec<f32> {
        let end = if front { k.yn } else { k.yt };
        let mut t = Vec::with_capacity(NX * NZ);
        for iz in 0..NZ {
            for ix in 0..NX {
                let (x, z) = (x0 + dx * ix as f32, z0 + dz * iz as f32);
                t.push((end - face_y(k, front, x, z)).abs());
            }
        }
        t
    };
    let (tf, tr) = (table(true), table(false));
    let look = move |t: &[f32], x: f32, z: f32| -> f32 {
        let fx = ((x - x0) / dx).clamp(0.0, (NX - 1) as f32 - 1e-3);
        let fz = ((z - z0) / dz).clamp(0.0, (NZ - 1) as f32 - 1e-3);
        let (ix, iz) = (fx as usize, fz as usize);
        let (u, v) = (fx - ix as f32, fz - iz as f32);
        let g = |i: usize, j: usize| t[j * NX + i];
        let a = g(ix, iz) * (1.0 - u) + g(ix + 1, iz) * u;
        let c = g(ix, iz + 1) * (1.0 - u) + g(ix + 1, iz + 1) * u;
        a * (1.0 - v) + c * v
    };
    let (yn, yt) = (k.yn, k.yt);
    let skip: Vec<String> = skip.iter().map(|s| s.to_string()).collect();
    k.b.transform(
        |key| {
            matches!(key.kind, PartKind::Detail | PartKind::Light)
                && !pre.contains(&key.name)
                && !skip.iter().any(|s| key.name.starts_with(s.as_str()))
        },
        |q| {
            let wf = smoothstep01((q[1] - (yn - 0.03)) / 0.01);
            let wr = smoothstep01(((yt + 0.03) - q[1]) / 0.01);
            let y = q[1] - wf * look(&tf, q[0], q[2]) + wr * look(&tr, q[0], q[2]);
            [q[0], y, q[2]]
        },
    );
}

/// Smoothstep on 0..1 (clamped).
fn smoothstep01(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Mirror an outline given for the left side (x < 0) to side `sx` (x only; order kept, the
/// helpers turn every face outward themselves).
pub(super) fn flip_x(pts: &[[f32; 2]], sx: f32) -> Vec<[f32; 2]> {
    pts.iter().map(|q| [sx * q[0].abs(), q[1]]).collect()
}

/// Rear quarter window behind the door glass on side `sx` (for bodies whose loft paints that
/// greenhouse panel): a glass panel on a black seal, from `gap` m behind the door glass back
/// to where the roof rail meets the belt (at most `max_len` m), its top edge `drop` m under
/// the roof rail.
pub(super) fn quarter_window(k: &mut Kit, sx: f32, gap: f32, max_len: f32, drop: f32) {
    let p = k.p;
    let ya = p.front_window().0 - gap;
    let zb = |y: f32| p.belt(y) + 0.035;
    let zt = |y: f32| p.ring(y)[RING_GLASS_TOP].p[2] - drop;
    if !p.has_greenhouse(ya) || zt(ya) - zb(ya) < 0.10 {
        return;
    }
    let mut top = Vec::new();
    let mut y = ya;
    let mut tip = ya;
    while y > ya - max_len && p.has_greenhouse(y) && zt(y) - zb(y) > 0.05 {
        top.push([y, zt(y)]);
        tip = y;
        y -= 0.04;
    }
    if top.len() < 3 {
        return;
    }
    let mut glass = vec![[ya, zb(ya)]];
    glass.extend(top.iter().copied());
    glass.push([tip - 0.02, zb(tip) + 0.01]);
    let seal: Vec<[f32; 2]> = {
        let c = mean2(&glass);
        glass
            .iter()
            .map(|q| [c[0] + (q[0] - c[0]) * 1.04, c[1] + (q[1] - c[1]) * 1.10])
            .collect()
    };
    let ks = dkey("quarter_glass_seal_l", BodyRegion::RearLeftQuarter, M::Trim);
    side_poly(k, &ks, sx, &seal, 0.002, 0.008);
    let kg = dkey("quarter_glass_l", BodyRegion::RearLeftQuarter, M::Glass);
    side_poly(k, &kg, sx, &glass, 0.005, 0.006);
}

/// Stadium (pill) outline centred at (`cx`, `cz`): `hw` half length along the first axis,
/// `hh` half height (the end radius), `n` points per round end.
pub(super) fn stadium(cx: f32, cz: f32, hw: f32, hh: f32, n: usize) -> Vec<[f32; 2]> {
    let r = hh.min(hw);
    let mut out = Vec::new();
    for (end, sgn) in [(cx + hw - r, 1.0f32), (cx - hw + r, -1.0)] {
        for i in 0..=n {
            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / n as f32;
            let (sa, ca) = a.sin_cos();
            out.push([end + sgn * r * ca, cz + sgn * r * sa]);
        }
    }
    out
}

/// Phase-3 body shaping shared by the exotic / supercar styles, placed from the wheels.
#[derive(Debug, Clone, Copy)]
pub(super) struct Shape3 {
    /// Plan-width scale at the nose (reached 0.6 m ahead of the front arch).
    pub nose: f32,
    /// Plan-width scale at the tail (reached 0.6 m behind the rear arch).
    pub tail: f32,
    /// Plan-width scale at the doors (waist between the arches).
    pub waist: f32,
    /// Depth of the recessed scallop ahead of the rear wheel (negative = in), m; 0 = none.
    pub scallop: f32,
    /// Shoulder line standing out under the belt, m; 0 = none.
    pub shoulder: f32,
    /// `side_bulge_m`.
    pub bulge: f32,
    /// `rocker_in_m`.
    pub rocker: f32,
    /// `front_fender_rise_m`, `rear_fender_rise_m`.
    pub rise: (f32, f32),
    /// End faces: (nose rake °, nose chin m, tail rake °, tail chin m).
    pub face: (f32, f32, f32, f32),
    /// Plan bows: (nose bow m, nose tip fraction, tail bow m, tail tip fraction).
    pub bow: (f32, f32, f32, f32),
    /// `lower_lean_in_m`.
    pub lean: f32,
}

/// Apply [`Shape3`] to `s`: plan taper and waist, the side scallop, shoulder line, section
/// bulge, rocker tuck and fender rise.
pub(super) fn shape3(s: &mut CarStyle, def: &VehicleDef, o: Shape3) {
    let w = &def.wheels;
    let fy = w.iter().map(|q| q.position_m[1]).fold(f32::MIN, f32::max);
    let ry = w.iter().map(|q| q.position_m[1]).fold(f32::MAX, f32::min);
    let r = w.iter().map(|q| q.radius_m).fold(0.0, f32::max) + 0.07;
    let wz = w.iter().map(|q| q.position_m[2]).fold(0.0, f32::max);
    let b = &mut s.body;
    b.plan_width = vec![
        [ry - r - 0.6, o.tail],
        [ry - r + 0.05, 1.0],
        [ry + r + 0.10, 1.0],
        [0.5 * (fy + ry), o.waist],
        [fy - r - 0.10, 1.0],
        [fy + r - 0.05, 1.0],
        [fy + r + 0.6, o.nose],
    ];
    // Belt height from the body height: the lattice belt plus this group's belt_dz.
    let belt = 0.62 * def.body_size_m[2] + b.belt_dz.max(0.0);
    let mut sculpt = Vec::new();
    if o.scallop < 0.0 {
        sculpt.push(SideSculpt {
            y0_m: ry + r + 0.04,
            y1_m: ry + r + 0.75,
            z0_m: wz - 0.04,
            z1_m: belt - 0.10,
            dx_m: o.scallop,
            falloff_m: 0.12,
        });
    }
    if o.shoulder > 0.0 {
        sculpt.push(SideSculpt {
            y0_m: ry - r,
            y1_m: fy + r,
            z0_m: belt - 0.10,
            z1_m: belt - 0.07,
            dx_m: o.shoulder,
            falloff_m: 0.035,
        });
    }
    b.side_sculpt = sculpt;
    b.side_bulge_m = o.bulge;
    b.rocker_in_m = o.rocker;
    b.front_fender_rise_m = o.rise.0;
    b.rear_fender_rise_m = o.rise.1;
    b.nose_face_rake_deg = o.face.0;
    b.nose_chin_m = o.face.1;
    b.tail_face_rake_deg = o.face.2;
    b.tail_chin_m = o.face.3;
    b.nose_plan_bow_m = o.bow.0;
    b.nose_tip_w = o.bow.1;
    b.tail_plan_bow_m = o.bow.2;
    b.tail_tip_w = o.bow.3;
    b.lower_lean_in_m = o.lean;
}

/// Air outlet on the front fender behind the front wheel: dark slot with `slats` blades.
pub(super) fn fender_outlet(k: &mut Kit, slats: usize, slat_mat: M) {
    let p = k.p;
    let (yf, zf, rf) = k.arch(true);
    let (y0, y1) = (p.y_fd_hinge + 0.04, yf - rf - 0.02);
    if y1 - y0 < 0.08 {
        return;
    }
    let (z0, z1) = (zf + 0.02, (p.belt(y1) - 0.06).max(zf + 0.16));
    for sx in SIDES {
        let kg = dkey("fender_outlet_l", BodyRegion::FrontLeftFender, M::Grille);
        k.side_patch(&kg, sx, y0, y1, z0, z1, 0.002, 0.006, (2, 2));
        let ks = dkey(
            "fender_outlet_slats_l",
            BodyRegion::FrontLeftFender,
            slat_mat,
        );
        for i in 0..slats {
            let z = z0 + (z1 - z0) * (i as f32 + 0.5) / slats as f32;
            k.side_patch(&ks, sx, y0, y1, z - 0.008, z + 0.008, 0.008, 0.010, (2, 1));
        }
    }
}

/// Dark roof sheet (glass canopy / black floating roof) over the roof top, `inset` from the
/// roof edges, `off` above the paint, `mat`.
pub(super) fn roof_sheet(k: &mut Kit, name: &str, inset: f32, off: f32, mat: M) {
    let (r0, r1, _, _) = k.p.roof;
    // Narrowest roof width along the sheet, so a tapered (canopy) roof never overhangs it.
    let hw = [r0 + 0.02, 0.5 * (r0 + r1), r1 - 0.02]
        .iter()
        .map(|&y| k.roof_edge(y))
        .fold(f32::MAX, f32::min)
        - inset;
    let key = dkey(name, BodyRegion::Roof, mat);
    k.patch(
        &key,
        r0 + 0.02,
        r1 - 0.02,
        top_band(-hw, hw),
        off,
        0.0,
        (8, 6),
    );
}

// ===================================================================================== Halberd

/// 2020s British carbon-tub supercar: teardrop glass canopy, dihedral doors, hooked blade
/// lamps (this fictional maker's signature), door-to-haunch air channel, glass engine cover
/// between flying buttresses, flush active airbrake, hair-line tail over mesh.
pub fn halberd(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: 0.42,
        nose_h: Some(0.45),
        cowl_h: 0.12,
        belt_dz: 0.08,
        nose_rake_deg: 40.0,
        nose_plan_round: 0.45,
        tail_plan_round: 0.42,
        hood_crown: 0.022,
        hood_valley: 0.02,
        windshield_rake_deg: Some(64.0),
        a_pillar_rake_deg: 25.0,
        c_pillar_rake_deg: 25.0,
        roof_arc: 0.85,
        tumblehome: Some(0.30),
        crease_z: 0.44,
        crease_depth: 0.0,
        shoulder_round: 0.10,
        sill_tuck: 0.11,
        haunch: 0.08,
        front_haunch: 0.06,
        flare: 0.0,
        ride_dz: -0.03,
        stance_pitch_deg: 0.2,
        ducktail_h: 0.020,
        rear_glass_bow: 0.02,
        belt_kick: 0.0,
        belt_rake: 0.02,
        tail_round: 0.25,
        tail_drop: 0.06,
        roof_dz: 0.02,
        ..BodyStyle::preset(Silhouette::Fastback)
    });
    s.details.push(kit_detail(halberd_kit));
    shape3(
        &mut s,
        def,
        Shape3 {
            nose: 0.90,
            tail: 0.9,
            waist: 0.94,
            scallop: -0.07,
            shoulder: 0.0,
            bulge: 0.035,
            rocker: 0.05,
            rise: (0.03, 0.04),
            face: (30.0, 0.06, 12.0, 0.08),
            bow: (0.25, 0.60, 0.12, 0.85),
            lean: 0.04,
        },
    );
    wheels(
        &mut s,
        def,
        RimKind::Monoblock,
        10,
        0.78,
        M::Trim,
        CapKind::CentreLock,
        (0.035, 0.012),
        (0.0, 0.5),
    );
    let m = &mut s.mechanical;
    m.caliper_rgb = Some(rgb(0xFF6A13));
    m.spring_rgb = Some(rgb(0x1C1C1E));
    s.tints.push((M::Paint, rgb(0xFF6A13)));
    s
}

fn halberd_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let pre = built_parts(&k);
    let p = ctx.profile;
    let yn = k.yn;
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;

    // Lamp anchors: where the nose top starts behind the raked face, so the lamps lie on the
    // top surface instead of across the face edge.
    let yl = k.yn - p.face_depth(true);
    let wl = p.half_width(yl);
    // --- Blade lamps (this maker's signature): a slim black blade along each nose-top
    //     corner whose outer end hooks back inward, an LED stroke tracing the blade and its
    //     hook, a small lens in the hook; a separate vertical intake slot on the face below.
    for sx in SIDES {
        let wf = p.half_width(yl - 0.30);
        let kh = sided(
            &dkey("headlamp_housing_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        let blade = [
            [0.40, yl - 0.05],
            [wl - 0.03, yl - 0.01],
            [wf - 0.04, yl - 0.20],
            [wf - 0.16, yl - 0.22],
            [wl - 0.14, yl - 0.10],
            [0.42, yl - 0.10],
        ];
        top_poly(&mut k, &kh, &flip_x(&blade, sx), 0.004, 0.012);
        let hk = headlight_key(sx);
        let h0 = [sx * 0.57, yl - 0.06];
        let h1 = [sx * (wl - 0.05), yl - 0.028];
        let h2 = [sx * (wf - 0.06), yl - 0.19];
        let h3 = [sx * (wf - 0.15), yl - 0.20];
        for (a, c) in [(h0, h1), (h1, h2), (h2, h3)] {
            top_stroke(&mut k, &hk, a, c, 0.018, 0.016);
        }
        let lens = [
            [wl - 0.16, yl - 0.08],
            [wl - 0.08, yl - 0.06],
            [wf - 0.08, yl - 0.16],
            [wf - 0.15, yl - 0.17],
        ];
        top_poly(&mut k, &hk, &flip_x(&lens, sx), 0.020, 0.018);
        // Amber signal: the inner end of the blade.
        let ik = indicator_front_key(sx);
        let sig = [
            [0.44, yl - 0.065],
            [0.53, yl - 0.06],
            [0.55, yl - 0.095],
            [0.46, yl - 0.095],
        ];
        top_poly(&mut k, &ik, &flip_x(&sig, sx), 0.022, 0.02);
        let kv = sided(
            &dkey("corner_intake_l", BodyRegion::FrontLeftFender, M::Grille),
            sx,
        );
        let slot = [
            [0.46, nz1 - 0.03],
            [0.60, nz1 - 0.03],
            [0.64, nz1 - 0.15],
            [0.52, nz1 - 0.12],
        ];
        end_poly(&mut k, &kv, true, &flip_x(&slot, sx), 0.016, 0.03);
        let kb = sided(
            &dkey(
                "corner_intake_vane_l",
                BodyRegion::FrontLeftFender,
                M::Carbon,
            ),
            sx,
        );
        front_stroke(
            &mut k,
            &kb,
            [sx * 0.53, nz1 - 0.035],
            [sx * 0.58, nz1 - 0.135],
            0.012,
            0.022,
        );
    }
    // --- Wide low centre intake, carbon splitter with fences, hood duct.
    k.mesh_grille(
        "front_intake",
        fb,
        0.40,
        nz0 + 0.05,
        nz0 + 0.15,
        2,
        6,
        M::Trim,
    );
    k.splitter(M::Carbon, 0.09, 0.05, 0.022, true);
    // Hood duct: a dark trapezoid just behind the nose, wider at its leading edge.
    let kd = dkey("hood_duct", BodyRegion::Hood, M::Grille);
    let duct = [
        [-0.22, yn - 0.30],
        [0.22, yn - 0.30],
        [0.15, yn - 0.50],
        [-0.15, yn - 0.50],
    ];
    top_poly(&mut k, &kd, &duct, 0.004, 0.01);

    // --- Glass canopy: dark roof sheet, dihedral door cut lines in the roof, door seams.
    roof_sheet(&mut k, "canopy_roof_base", 0.03, 0.004, M::Trim);
    roof_sheet(&mut k, "canopy_roof", 0.05, 0.010, M::Glass);
    let (r0, r1, _, _) = p.roof;
    let ws = dkey("door_roof_seam_l", BodyRegion::Roof, M::Grille);
    for sx in SIDES {
        let xe = k.roof_edge(0.5 * (r0 + r1)) - 0.10;
        let ws = sided(&ws, sx);
        k.patch(
            &ws,
            r0 + 0.04,
            r1 - 0.02,
            top_band(sx * xe, sx * (xe + 0.012)),
            0.007,
            0.0,
            (3, 1),
        );
    }

    // --- Air channel scooped into the door, feeding a big intake on the rear haunch whose
    //     front edge steps back toward the sill (teardrop read), carbon lip under it.
    let (yr, zr, rr) = k.arch(false);
    let ya = yr + rr + 0.02;
    let zc = p.belt(p.y_fd_rear) - 0.21;
    for sx in SIDES {
        let kc = dkey("door_channel_l", BodyRegion::LeftDoorF, M::Grille);
        k.side_patch(
            &kc,
            sx,
            p.y_fd_rear + 0.01,
            p.y_fd_hinge - 0.28,
            zc,
            zc + 0.08,
            0.002,
            0.004,
            (5, 1),
        );
        let ki = dkey("haunch_intake_l", BodyRegion::RearLeftQuarter, M::Grille);
        let bands = [
            (zc - 0.02, zc + 0.13, p.y_fd_rear - 0.01),
            (zr + 0.13, zc - 0.02, p.y_fd_rear - 0.08),
            (zr + 0.03, zr + 0.13, p.y_fd_rear - 0.18),
        ];
        for (z0, z1, y1) in bands {
            k.side_patch(&ki, sx, ya, y1, z0, z1, 0.002, 0.008, (2, 1));
        }
        let kw = dkey(
            "haunch_intake_lip_l",
            BodyRegion::RearLeftQuarter,
            M::Carbon,
        );
        k.side_patch(
            &kw,
            sx,
            ya,
            p.y_fd_rear - 0.18,
            zr + 0.005,
            zr + 0.03,
            0.010,
            0.014,
            (2, 1),
        );
        // Slanted carbon blade along the stepped front edge of the intake.
        let ke = sided(
            &dkey(
                "haunch_intake_edge_l",
                BodyRegion::RearLeftQuarter,
                M::Carbon,
            ),
            sx,
        );
        let (ya0, za0) = (p.y_fd_rear - 0.02, zc + 0.12);
        let (ya1, za1) = (p.y_fd_rear - 0.19, zr + 0.04);
        let (pa, _) = p.side_point(ya0, za0);
        let (pc, _) = p.side_point(ya1, za1);
        k.plate(
            &ke,
            [sx * (-pa[0] + 0.012), ya0, za0],
            [sx * (-pc[0] + 0.012), ya1, za1],
            [sx, 0.0, 0.0],
            0.03,
            0.05,
        );
    }
    k.skirts(M::Carbon, 0.035, 0.05);
    fender_outlet(&mut k, 2, M::Carbon);
    // --- Glass engine cover between flying buttresses (the double-skin rear deck).
    let (d0, d1) = (yr - rr + 0.12, p.y_deck - 0.04);
    let kg = dkey("engine_cover_glass", BodyRegion::Trunk, M::Glass);
    k.patch(&kg, d0, d1, top_band(-0.28, 0.28), 0.004, 0.006, (4, 2));
    let r0 = p.roof.0;
    for sx in SIDES {
        let xs = k.roof_edge(r0 + 0.02) - 0.02;
        let ks = sided(
            &dkey("buttress_l", BodyRegion::RearLeftQuarter, M::Paint),
            sx,
        );
        k.sail(&ks, sx, r0 + 0.02, k.yt + 0.40, xs, 0.10, 0.03);
        // Dark channel under each buttress.
        let kc = sided(
            &dkey("buttress_channel_l", BodyRegion::RearLeftQuarter, M::Grille),
            sx,
        );
        let ch = [
            [xs - 0.10, r0 - 0.10],
            [xs - 0.04, r0 - 0.10],
            [xs - 0.04, k.yt + 0.45],
            [xs - 0.12, k.yt + 0.45],
        ];
        top_poly(&mut k, &kc, &flip_x(&ch, sx), 0.003, 0.008);
    }

    // --- Full-width active airbrake lying flush on the tail of the deck.
    k.lip_spoiler(M::Paint, k.yt + 0.32, 0.03, 0.30, 0.06);

    // --- Rear: hair-line LED tail over an exposed black mesh, high exhaust surrounds.
    let zt = tz1 - 0.035;
    let kb = lkey("taillight_bar", BodyRegion::Trunk, M::Taillight);
    k.rear_box(&kb, -0.30, 0.30, zt - 0.007, zt + 0.007, 0.022, 0.0);
    for sx in SIDES {
        let tk = taillight_key(sx);
        let x_sig = wt - 0.08;
        k.rear_box(
            &tk,
            sx * 0.30,
            sx * (x_sig - INDICATOR_SEAM_M),
            zt - 0.007,
            zt + 0.007,
            0.022,
            0.0,
        );
        // The hairline's outer end folds down round the corner of the mesh opening.
        rear_stroke(
            &mut k,
            &tk,
            [sx * (x_sig - 0.02), zt - 0.01],
            [sx * (x_sig - 0.08), zt - 0.15],
            0.014,
            0.022,
        );
        let ka = indicator_rear_key(sx, M::EmissiveAmber);
        k.rear_box(
            &ka,
            sx * x_sig,
            sx * (wt - 0.02),
            zt - 0.007,
            zt + 0.007,
            0.022,
            0.0,
        );
    }
    let (mz0, mz1) = (tz0 + 0.17, zt - 0.025);
    k.rear_panel(
        "rear_mesh_panel",
        BodyRegion::RearBumper,
        wt - 0.05,
        mz0,
        mz1,
        M::Grille,
    );
    let km = dkey("rear_mesh", BodyRegion::RearBumper, M::Trim);
    for i in 1..4 {
        let z = mz0 + (mz1 - mz0) * i as f32 / 4.0;
        k.rear_box(&km, -wt + 0.07, wt - 0.07, z - 0.004, z + 0.004, 0.014, 0.0);
    }
    for i in 0..10 {
        let x = -wt + 0.12 + (2.0 * wt - 0.24) * i as f32 / 9.0;
        k.rear_box(&km, x - 0.004, x + 0.004, mz0, mz1, 0.014, 0.0);
    }
    let ze = (p.z_bumper_top.1 - 0.12).max(p.z_floor + 0.26);
    let kx = dkey("exhaust_surround", BodyRegion::RearBumper, M::Carbon);
    for sx in SIDES {
        k.rear_box(
            &kx,
            sx * 0.005,
            sx * 0.25,
            ze - 0.065,
            ze + 0.065,
            0.02,
            0.01,
        );
    }
    k.diffuser(M::Carbon, 0.72, 0.17, 7, 0.10);

    k.mirrors(0.16, 0.10, Some(M::Carbon));

    // Turn signals as segments beside the lamps (playtest row 136).
    k.signals_beside_lamps(0.06, M::EmissiveAmber);
    shear_to_faces(
        &mut k,
        &pre,
        &["headlamp_", "headlight_", "indicator_front_"],
    );
}

// ======================================================================================= Ombra

/// 2010s Italian V12 flagship: very low and wide, sharp-cornered wedge; double-chevron lamp
/// graphics front and rear (this fictional maker's signature), trapezoid corner intakes split
/// by two bars, V hood creases with flush ducts, giant angular side intakes + shoulder air
/// ears, glass-louvred engine cover, centre exhaust in a wide pill surround.
pub fn ombra(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: 0.32,
        nose_h: Some(0.45),
        belt_dz: 0.07,
        nose_rake_deg: 58.0,
        nose_plan_round: 0.08,
        tail_plan_round: 0.08,
        hood_crown: 0.008,
        cowl_h: 0.16,
        windshield_rake_deg: Some(66.0),
        a_pillar_rake_deg: 20.0,
        c_pillar_rake_deg: 12.0,
        rear_glass_rake_deg: Some(70.0),
        roof_len_trim: 0.0,
        tumblehome: Some(0.30),
        crease_z: 0.42,
        crease_depth: 0.030,
        shoulder_round: 0.02,
        sill_tuck: 0.07,
        haunch: 0.08,
        front_haunch: 0.05,
        flare: 0.025,
        flare_shape: ArchShape::Box,
        ride_dz: -0.035,
        stance_pitch_deg: 0.3,
        ducktail_h: 0.0,
        deck_valley: 0.03,
        belt_kick: 0.04,
        belt_rake: 0.06,
        tail_round: 0.05,
        tail_drop: 0.0,
        roof_dz: -0.07,
        ..BodyStyle::preset(Silhouette::Wedge)
    });
    s.details.push(kit_detail(ombra_kit));
    shape3(
        &mut s,
        def,
        Shape3 {
            nose: 0.92,
            tail: 0.97,
            waist: 0.95,
            scallop: -0.07,
            shoulder: 0.02,
            bulge: 0.01,
            rocker: 0.04,
            rise: (0.0, 0.02),
            face: (35.0, 0.04, 5.0, 0.08),
            bow: (0.12, 0.90, 0.08, 0.92),
            lean: 0.02,
        },
    );
    wheels(
        &mut s,
        def,
        RimKind::SplitSpoke,
        5,
        0.79,
        M::Trim,
        CapKind::CentreLock,
        (0.03, 0.012),
        (0.0, 0.5),
    );
    let m = &mut s.mechanical;
    m.caliper_rgb = Some(rgb(0xE87A10));
    m.spring_rgb = Some(rgb(0xE87A10));
    s.tints.push((M::Paint, rgb(0x2B2D42)));
    s
}

fn ombra_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let pre = built_parts(&k);
    let p = ctx.profile;
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;
    let hood = BodyRegion::Hood;

    // Lamp anchors: where the nose top starts behind the raked face, so the lamps lie on the
    // top surface instead of across the face edge.
    let yl = k.yn - p.face_depth(true);
    let wl = p.half_width(yl);
    // --- Long angular lamps on the nose-top corners, running back along the fender edge:
    //     dark lens on a black housing, a double-chevron DRL inside, the amber signal at the
    //     tip.
    for sx in SIDES {
        let wf = p.half_width(yl - 0.40);
        let hous = [
            [0.40, yl - 0.11],
            [wl - 0.02, yl - 0.02],
            [wf - 0.03, yl - 0.42],
            [wf - 0.16, yl - 0.40],
        ];
        let lens = [
            [0.43, yl - 0.115],
            [wl - 0.045, yl - 0.04],
            [wf - 0.055, yl - 0.39],
            [wf - 0.15, yl - 0.375],
        ];
        let kh = sided(
            &dkey("headlamp_housing_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        top_poly(&mut k, &kh, &flip_x(&hous, sx), 0.004, 0.012);
        let kl = sided(
            &dkey("headlamp_lens_l", BodyRegion::FrontLeftFender, M::Glass),
            sx,
        );
        top_poly(&mut k, &kl, &flip_x(&lens, sx), 0.009, 0.012);
        let hk = headlight_key(sx);
        // Double chevron pointing outward (this maker's signature).
        for d in [0.0, 0.09] {
            let a = [sx * (0.48 + d), yl - 0.11 - 0.02 * d];
            let apex = [sx * (wl - 0.12 - d), yl - 0.20 - 0.3 * d];
            let c = [sx * (wf - 0.20 - d), yl - 0.35];
            top_stroke(&mut k, &hk, a, apex, 0.015, 0.012);
            top_stroke(&mut k, &hk, apex, c, 0.015, 0.012);
        }
        let proj = [
            [wf - 0.14, yl - 0.22],
            [wf - 0.07, yl - 0.21],
            [wf - 0.075, yl - 0.30],
            [wf - 0.13, yl - 0.30],
        ];
        top_poly(&mut k, &hk, &flip_x(&proj, sx), 0.012, 0.01);
        let ik = indicator_front_key(sx);
        let sig = [
            [wl - 0.12, yl - 0.05],
            [wl - 0.055, yl - 0.045],
            [wl - 0.07, yl - 0.09],
            [wl - 0.13, yl - 0.095],
        ];
        top_poly(&mut k, &ik, &flip_x(&sig, sx), 0.022, 0.02);
    }
    // --- Huge corner intakes (trapezoids leaning in at the top) split by two body-colour
    //     bars, narrow centre slot under the pointed nose, carbon splitter.
    let (zi0, zi1) = (nz0 + 0.03, nz1 - 0.04);
    for sx in SIDES {
        let (xi, xo) = (0.34, wn - 0.03);
        let kg = sided(&dkey("front_intake_l", fb, M::Grille), sx);
        let intake = [[xi, zi0], [xo, zi0], [xo, zi1 - 0.03], [xi + 0.14, zi1]];
        end_poly(&mut k, &kg, true, &flip_x(&intake, sx), 0.006, 0.03);
        // Two body-colour bars split the intake into three stacked slots.
        let kb = sided(&dkey("front_intake_blade_l", fb, M::Paint), sx);
        for f in [0.36, 0.68] {
            let z = zi0 + f * (zi1 - zi0);
            let xe = xi + 0.14 * (z - zi0) / (zi1 - zi0);
            let bar = [
                [xe - 0.01, z - 0.016],
                [xo + 0.005, z - 0.016],
                [xo + 0.005, z + 0.016],
                [xe - 0.01, z + 0.016],
            ];
            end_poly(&mut k, &kb, true, &flip_x(&bar, sx), 0.02, 0.03);
        }
        // Horizontal mesh bars behind the blade.
        let km = sided(&dkey("front_intake_mesh_l", fb, M::Trim), sx);
        for i in 1..4 {
            let z = zi0 + (zi1 - zi0) * i as f32 / 4.0;
            let xe = xi + 0.14 * (z - zi0) / (zi1 - zi0) + 0.02;
            k.front_box(
                &km,
                sx * xe,
                sx * (xo - 0.02),
                z - 0.005,
                z + 0.005,
                0.012,
                0.0,
            );
        }
    }
    k.mesh_grille(
        "centre_slot",
        fb,
        0.24,
        nz0 + 0.03,
        nz0 + 0.10,
        1,
        6,
        M::Trim,
    );
    k.splitter(M::Carbon, 0.10, 0.04, 0.022, true);

    // --- Hood: two creases converging into a V at the nose tip, a flush duct either side.
    let kc = dkey("hood_crease", hood, M::Grille);
    for sx in SIDES {
        let a = [sx * 0.03, yn - 0.06];
        let c = [sx * 0.30, p.y_fw + 0.06];
        top_stroke(&mut k, &kc, a, c, 0.008, 0.004);
    }
    for sx in SIDES {
        let name = if sx < 0.0 {
            "flush_duct_l"
        } else {
            "flush_duct_r"
        };
        let kd = dkey(name, hood, M::Trim);
        let ya = p.y_fw + 0.10;
        let duct = [
            [sx * 0.10, ya + 0.30],
            [sx * 0.17, ya + 0.30],
            [sx * 0.26, ya],
            [sx * 0.12, ya],
        ];
        top_poly(&mut k, &kd, &duct, 0.003, 0.008);
    }

    // --- Scissor-door seams, giant angular side intake, shoulder air "ears".
    let (yr, zr, rr) = k.arch(false);
    let (y0, y1) = (yr + rr + 0.03, p.y_fd_rear - 0.01);
    let (z0, z1) = (zr - 0.06, p.belt(y1) - 0.03);
    let (yf, zf, rf) = k.arch(true);
    for sx in SIDES {
        // Sharp groove rising from behind the front wheel along the door to the intake.
        let kc = dkey("side_line_l", BodyRegion::LeftDoorF, M::Grille);
        let (ya, za) = (yf - rf + 0.02, zf + 0.12);
        let (yb, zb) = (y1 + 0.10, z0 + 0.10);
        let line = [
            [ya, za],
            [ya - 0.04, za - 0.012],
            [yb, zb - 0.010],
            [yb + 0.02, zb + 0.004],
        ];
        side_poly(&mut k, &kc, sx, &line, 0.003, 0.008);
    }
    for sx in SIDES {
        let ki = dkey("side_intake_l", BodyRegion::RearLeftQuarter, M::Grille);
        k.side_patch(&ki, sx, y0, y1, z0, z1, 0.002, 0.008, (3, 3));
        // Angular front wall leaning forward, and a diagonal blade across the mouth.
        let kw = sided(
            &dkey("side_intake_wall_l", BodyRegion::RearLeftQuarter, M::Paint),
            sx,
        );
        let (pa, _) = p.side_point(y1, z0);
        let (pb, _) = p.side_point(y1 + 0.12, z1);
        let xa = sx * (-pa[0] + 0.03);
        let xb = sx * (-pb[0] + 0.03);
        k.plate(
            &kw,
            [xa, y1, z0],
            [xb, y1 + 0.12, z1],
            [sx, 0.0, 0.0],
            0.06,
            0.02,
        );
        let kb = sided(
            &dkey(
                "side_intake_blade_l",
                BodyRegion::RearLeftQuarter,
                M::Carbon,
            ),
            sx,
        );
        let (pc, _) = p.side_point(y0 + 0.05, z1 - 0.04);
        let (pd, _) = p.side_point(y1 - 0.02, z0 + 0.06);
        k.plate(
            &kb,
            [sx * (-pc[0] + 0.02), y0 + 0.05, z1 - 0.04],
            [sx * (-pd[0] + 0.02), y1 - 0.02, z0 + 0.06],
            [sx, 0.0, 0.0],
            0.04,
            0.022,
        );
        // Air ear on the shoulder behind the side glass: a wedge with a dark mouth.
        let ke = sided(
            &dkey("air_ear_l", BodyRegion::RearLeftQuarter, M::Paint),
            sx,
        );
        let (ye0, ye1) = (p.y_deck - 0.40, p.y_deck + 0.02);
        let xo = p.half_width(ye1) - 0.10;
        let xi = xo - 0.20;
        let zb = k.top_z(ye1, sx * (xo - 0.10)) - 0.02;
        let prof = [
            [sx * xi, zb],
            [sx * xo, zb],
            [sx * xo, zb + 0.09],
            [sx * (xi + 0.03), zb + 0.11],
        ];
        k.prism_y(&ke, &prof, ye0, ye1);
        let km = sided(
            &dkey("air_ear_mouth_l", BodyRegion::RearLeftQuarter, M::Grille),
            sx,
        );
        k.bx(
            &km,
            [sx * (xi + 0.03), ye1 - 0.01, zb + 0.03],
            [sx * (xo - 0.015), ye1 + 0.004, zb + 0.095],
        );
    }
    k.skirts(M::Carbon, 0.04, 0.06);
    fender_outlet(&mut k, 3, M::Paint);

    // --- Engine cover: glass panel over the V12 with dark louvres across it, side vents.
    let (le0, le1) = (yr - rr + 0.10, p.y_deck - 0.08);
    let kg = dkey("engine_cover_glass", BodyRegion::Trunk, M::Glass);
    k.patch(&kg, le0, le1, top_band(-0.32, 0.32), 0.004, 0.006, (4, 2));
    let kl = dkey("engine_louvres", BodyRegion::Trunk, M::Trim);
    for i in 0..6 {
        let y = le0 + (le1 - le0) * (i as f32 + 0.5) / 6.0;
        let z = k.top_z(y, 0.0).max(k.top_z(y, 0.3)) + 0.008;
        let prof = [
            [y + 0.02, z],
            [y - 0.02, z],
            [y - 0.03, z + 0.018],
            [y + 0.0, z + 0.024],
        ];
        k.prism_x(&kl, &prof, -0.30, 0.30);
    }
    for sx in SIDES {
        let name = if sx < 0.0 {
            "deck_vent_l"
        } else {
            "deck_vent_r"
        };
        let (a, c) = (sx * 0.40, sx * 0.62);
        k.top_vent(
            name,
            BodyRegion::Trunk,
            le0 + 0.10,
            le1 - 0.05,
            a.min(c),
            a.max(c),
            3,
            M::Trim,
        );
    }

    for sx in SIDES {
        let name = if sx < 0.0 {
            "haunch_louvre_l"
        } else {
            "haunch_louvre_r"
        };
        let xo = p.half_width(yr) - 0.10;
        let (a, c) = (sx * (xo - 0.20), sx * xo);
        k.top_vent(
            name,
            BodyRegion::Trunk,
            yr - 0.12,
            yr + 0.20,
            a.min(c),
            a.max(c),
            3,
            M::Trim,
        );
    }
    let kbl = lkey("brake_light_high", BodyRegion::Trunk, M::Taillight);
    let ybl = le0 - 0.06;
    let zbl = k.top_z(ybl, 0.0);
    k.bx(
        &kbl,
        [-0.14, ybl - 0.015, zbl - 0.002],
        [0.14, ybl + 0.015, zbl + 0.010],
    );

    // --- Deck-mounted carbon wing.
    k.wing(Wing {
        lead: 0.30,
        chord: 0.22,
        height: 0.07,
        half_span: 0.84,
        thick: 0.028,
        aoa_deg: 9.0,
        stand: Stand::Pedestal(0.40),
        endplates: Some((0.08, 0.22)),
        mat: M::Carbon,
        stand_mat: M::Carbon,
        gurney: 0.012,
    });

    // --- Rear: double-chevron tails in angular black housings, mesh fascia, a wide pill
    //     exhaust surround.
    let zt = tz1 - 0.07;
    for sx in SIDES {
        let kh = sided(
            &dkey("taillight_housing_l", BodyRegion::RearLeftQuarter, M::Trim),
            sx,
        );
        k.rear_box(
            &kh,
            sx * 0.38,
            sx * (wt - 0.01),
            zt - 0.08,
            zt + 0.065,
            0.010,
            0.004,
        );
        let tk = taillight_key(sx);
        // Double chevron pointing outward, as at the front, plus a bar linking it inboard.
        rear_stroke(&mut k, &tk, [sx * 0.40, zt], [sx * 0.56, zt], 0.020, 0.026);
        for d in [0.0, 0.10] {
            let apex = [sx * (wt - 0.05 - d), zt];
            let top = [sx * (wt - 0.15 - d), zt + 0.05];
            let bot = [sx * (wt - 0.15 - d), zt - 0.06];
            rear_stroke(&mut k, &tk, top, apex, 0.020, 0.026);
            rear_stroke(&mut k, &tk, apex, bot, 0.020, 0.026);
        }
    }
    let (fz0, fz1) = (tz0 + 0.12, zt - 0.10);
    k.rear_panel(
        "rear_fascia",
        BodyRegion::RearBumper,
        wt - 0.06,
        fz0,
        fz1,
        M::Grille,
    );
    let km = dkey("rear_fascia_mesh", BodyRegion::RearBumper, M::Trim);
    for i in 0..8 {
        let x = -wt + 0.20 + (2.0 * wt - 0.40) * i as f32 / 7.0;
        rear_stroke(
            &mut k,
            &km,
            [x - 0.04, fz0 + 0.01],
            [x + 0.04, fz1 - 0.01],
            0.008,
            0.014,
        );
    }
    let ze = (p.z_floor + 0.14).max(0.24);
    let kx = dkey("exhaust_surround", BodyRegion::RearBumper, M::Trim);
    let surround = stadium(0.0, ze, 0.27, 0.075, 5);
    end_poly(&mut k, &kx, false, &surround, 0.03, 0.05);
    k.diffuser(M::Carbon, 0.80, 0.15, 5, 0.12);

    k.mirrors(0.18, 0.09, Some(M::Trim));

    // Turn signals as segments beside the lamps (playtest row 136).
    k.signals_beside_lamps(0.06, M::EmissiveAmber);
    shear_to_faces(
        &mut k,
        &pre,
        &["headlamp_", "headlight_", "indicator_front_"],
    );
}

// ===================================================================================== Ostwind

/// 2020s German rear-engine flagship: oval triple-bar lamps on high front fenders, front trunk
/// lid, slatted low intakes, small haunch intakes, longitudinal engine grille in the deck lid,
/// raised lip spoiler, full-width light bar, twin rectangular tips in the valance.
pub fn ostwind(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: 0.10,
        nose_h: Some(0.56),
        belt_dz: 0.04,
        nose_rake_deg: 30.0,
        nose_plan_round: 0.32,
        tail_plan_round: 0.30,
        hood_crown: 0.020,
        hood_valley: 0.020,
        windshield_rake_deg: Some(60.0),
        a_pillar_rake_deg: 10.0,
        c_pillar_rake_deg: 6.0,
        roof_arc: 0.55,
        tumblehome: Some(0.22),
        rear_glass_bow: 0.05,
        crease_z: 0.50,
        crease_depth: 0.0,
        shoulder_round: 0.10,
        sill_tuck: 0.09,
        haunch: 0.080,
        front_haunch: 0.07,
        flare: 0.0,
        ride_dz: -0.005,
        stance_pitch_deg: 0.1,
        ducktail_h: 0.030,
        deck_drop: 0.0,
        roof_len_trim: -0.18,
        belt_kick: 0.0,
        belt_rake: 0.0,
        tail_round: 0.35,
        tail_drop: 0.10,
        ..BodyStyle::preset(Silhouette::Fastback)
    });
    s.details.push(kit_detail(ostwind_kit));
    shape3(
        &mut s,
        def,
        Shape3 {
            nose: 0.94,
            tail: 0.94,
            waist: 0.94,
            scallop: 0.0,
            shoulder: 0.0,
            bulge: 0.035,
            rocker: 0.04,
            rise: (0.06, 0.05),
            face: (25.0, 0.05, 15.0, 0.06),
            bow: (0.30, 0.55, 0.30, 0.65),
            lean: 0.03,
        },
    );
    wheels(
        &mut s,
        def,
        RimKind::MultiSpoke,
        10,
        0.80,
        M::Metal,
        CapKind::CentreLock,
        (0.025, 0.012),
        (0.0, 0.5),
    );
    let m = &mut s.mechanical;
    m.caliper_rgb = Some(rgb(0xF0C419));
    s.tints.push((M::Paint, rgb(0x8A9BA8)));
    s
}

/// Surface normal `n` turned 40° about x from +z toward +y, for lamps that face ahead.
fn tilt_fwd(n: [f32; 3]) -> [f32; 3] {
    let (s, c) = (0.643, 0.766);
    [n[0], c * n[1] + s * n[2], c * n[2] - s * n[1]]
}

fn ostwind_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let pre = built_parts(&k);
    let p = ctx.profile;
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;
    let hood = BodyRegion::Hood;

    // --- Oval lamps on the fender fronts: chrome bezel, dark bowl, three light bars.
    // Two overlapping discs along y give the oval; the bars sit in the front one.
    let yc = yn - 0.22;
    let xc = wn - 0.17;
    for sx in SIDES {
        let kb = sided(
            &dkey("headlamp_bezel_l", BodyRegion::FrontLeftFender, M::Chrome),
            sx,
        );
        let kd = sided(
            &dkey("headlamp_bowl_l", BodyRegion::FrontLeftFender, M::Grille),
            sx,
        );
        for dy in [0.0, -0.07] {
            let (q, n) = p.top_point(yc + dy, sx * xc);
            // Face the lamp forward like an upright oval on the raked fender front.
            let n = tilt_fwd(n);
            let along = |d: f32| [q[0] + n[0] * d, q[1] + n[1] * d, q[2] + n[2] * d];
            k.b.cylinder(&kb, along(-0.01), along(0.018), 0.125, 0.125, 20, true);
            k.b.cylinder(&kd, along(0.0), along(0.022), 0.108, 0.108, 20, true);
        }
        let (pt, n) = p.top_point(yc - 0.035, sx * xc);
        let n = tilt_fwd(n);
        let hk = headlight_key(sx);
        // Three horizontal light bars across the oval (this maker's signature; the lower
        // one is the turn signal), on the
        // bowl's plane so they stay proud of it where the fender curves into the valley.
        let u = {
            let dz = -n[0] / n[2].max(0.2);
            let l = (1.0 + dz * dz).sqrt();
            [1.0 / l, 0.0, dz / l]
        };
        let v = [
            n[1] * u[2] - n[2] * u[1],
            n[2] * u[0] - n[0] * u[2],
            n[0] * u[1] - n[1] * u[0],
        ];
        for (dv, half) in [(-0.045, 0.055), (0.0, 0.07), (0.045, 0.055)] {
            let c = [
                pt[0] + v[0] * dv + n[0] * 0.03,
                pt[1] + v[1] * dv + n[1] * 0.03,
                pt[2] + v[2] * dv + n[2] * 0.03,
            ];
            let a = [c[0] - u[0] * half, c[1] - u[1] * half, c[2] - u[2] * half];
            let e = [c[0] + u[0] * half, c[1] + u[1] * half, c[2] + u[2] * half];
            // The lower bar is the amber turn signal.
            if dv < 0.0 {
                k.plate(&indicator_front_key(sx), a, e, v, 0.02, 0.03);
            } else {
                k.plate(&hk, a, e, v, 0.02, 0.03);
            }
        }
    }
    // --- Front trunk lid shut lines.
    let kt = dkey("frunk_seam", hood, M::Grille);
    let xs = wn - 0.32;
    let (ya, yb) = (p.y_fw + 0.10, yn - 0.10);
    for sx in SIDES {
        let (a, c) = (sx * xs, sx * (xs + 0.012));
        k.patch(&kt, ya, yb, top_band(a, c), 0.004, 0.0, (6, 1));
    }
    for y in [ya, yb] {
        k.patch(
            &kt,
            y - 0.006,
            y + 0.006,
            top_band(-xs, xs),
            0.004,
            0.0,
            (1, 4),
        );
    }

    // --- Low wide intakes with horizontal active-flap slats; slim DRL strip over each side.
    let (zi0, zi1) = (nz0 + 0.04, nz0 + 0.17);
    k.grille("front_intake", fb, 0.34, zi0, zi1 - 0.02, 3, M::Trim, None);
    for sx in SIDES {
        let (xi, xo) = (0.42, wn - 0.08);
        let kg = sided(&dkey("side_intake_front_l", fb, M::Grille), sx);
        k.front_box(&kg, sx * xi, sx * xo, zi0, zi1, 0.006, 0.0);
        let ks = sided(&dkey("side_intake_front_slats_l", fb, M::Trim), sx);
        for i in 1..4 {
            let z = zi0 + (zi1 - zi0) * i as f32 / 4.0;
            k.front_box(
                &ks,
                sx * (xi + 0.02),
                sx * (xo - 0.02),
                z - 0.008,
                z + 0.008,
                0.014,
                0.0,
            );
        }
        let kd = sided(&lkey("drl_l", fb, M::Headlight), sx);
        k.front_box(
            &kd,
            sx * (xi + 0.02),
            sx * (xo - 0.02),
            zi1 + 0.02,
            zi1 + 0.034,
            0.012,
            0.0,
        );
    }
    k.splitter(M::Trim, 0.03, 0.10, 0.016, false);
    // Blank number plate in a dark frame above the centre intake.
    let (pz0, pz1) = (zi1 + 0.02, (zi1 + 0.13).min(nz1 - 0.02));
    let kpf = dkey("number_plate_front_frame", fb, M::Trim);
    k.front_box(&kpf, -0.25, 0.25, pz0, pz1, 0.008, 0.003);
    let kp = dkey("number_plate_front", fb, M::Metal);
    k.front_box(&kp, -0.235, 0.235, pz0 + 0.012, pz1 - 0.012, 0.012, 0.0);
    // Vertical air curtains at the bumper corners, amber side markers.
    for sx in SIDES {
        let kc = sided(&dkey("air_curtain_l", fb, M::Grille), sx);
        k.front_box(
            &kc,
            sx * (wn - 0.06),
            sx * (wn - 0.02),
            zi0,
            zi1 + 0.06,
            0.008,
            0.0,
        );
        let km = sided(&lkey("side_marker_l", fb, M::EmissiveAmber), sx);
        k.front_box(
            &km,
            // ≥ 4 cm each way so the mid LOD's 3.5 cm vertex clustering keeps it.
            sx * (wn - 0.07),
            sx * (wn - 0.025),
            zi1 + 0.075,
            zi1 + 0.12,
            0.010,
            0.0,
        );
    }

    // --- Small intakes on the rear haunches just behind the doors.
    let (yr, _, rr) = k.arch(false);
    for sx in SIDES {
        let ki = dkey("haunch_intake_l", BodyRegion::RearLeftQuarter, M::Grille);
        let (y0, y1) = (yr + rr - 0.04, p.y_fd_rear - 0.03);
        let z1 = p.belt(y1) - 0.06;
        k.side_patch(&ki, sx, y0, y1, z1 - 0.10, z1, 0.002, 0.006, (2, 1));
        let ks = dkey(
            "haunch_intake_slat_l",
            BodyRegion::RearLeftQuarter,
            M::Paint,
        );
        k.side_patch(
            &ks,
            sx,
            y0,
            y1,
            z1 - 0.058,
            z1 - 0.042,
            0.007,
            0.010,
            (2, 1),
        );
    }
    k.skirts(M::Paint, 0.015, 0.05);
    for sx in SIDES {
        quarter_window(&mut k, sx, 0.05, 0.75, 0.025);
    }
    // Chrome window line along the belt from the A-pillar to the quarter window.
    let kw = dkey("window_line_l", BodyRegion::LeftDoorF, M::Chrome);
    let (w0, w1) = (p.y_fd_rear - 0.30, p.y_fw - 0.10);
    let zw = p.belt(0.5 * (w0 + w1));
    for sx in SIDES {
        k.side_patch(
            &kw,
            sx,
            w0,
            w1,
            zw - 0.012,
            zw + 0.002,
            0.003,
            0.004,
            (8, 1),
        );
    }

    // --- Engine grille in the deck lid: dark base, longitudinal fins, centre brake light.
    let (g0, g1) = (yr - rr + 0.20, p.y_deck - 0.06);
    let gw = 0.40;
    let kgb = dkey("engine_grille", BodyRegion::Trunk, M::Grille);
    k.patch(&kgb, g0, g1, top_band(-gw, gw), 0.003, 0.005, (3, 2));
    let kf = dkey("engine_grille_fins", BodyRegion::Trunk, M::Trim);
    for i in 0..9 {
        let x = -gw + 0.04 + (2.0 * gw - 0.08) * i as f32 / 8.0;
        let za = k.top_z(g0 + 0.02, x) + 0.012;
        let zb = k.top_z(g1 - 0.02, x) + 0.012;
        k.plate(
            &kf,
            [x, g0 + 0.02, za],
            [x, g1 - 0.02, zb],
            [0.0, 0.0, 1.0],
            0.016,
            0.014,
        );
    }
    let kbl = lkey("brake_light_high", BodyRegion::Trunk, M::Taillight);
    let zbl = k.top_z(g1 + 0.03, 0.0) + 0.006;
    k.bx(
        &kbl,
        [-0.16, g1 + 0.015, zbl - 0.004],
        [0.16, g1 + 0.045, zbl + 0.008],
    );

    // --- Extending spoiler: body-colour blade raised on two short stands.
    k.wing(Wing {
        lead: 0.26,
        chord: 0.20,
        height: 0.05,
        half_span: 0.62,
        thick: 0.024,
        aoa_deg: 12.0,
        stand: Stand::Pedestal(0.34),
        endplates: None,
        mat: M::Paint,
        stand_mat: M::Trim,
        gurney: 0.008,
    });

    // --- Tail: black band with a full-width red bar, three stepped dashes per side.
    let zt = tz1 - 0.06;
    let kh = dkey("tail_band", BodyRegion::Trunk, M::Trim);
    k.rear_box(
        &kh,
        -wt + 0.02,
        wt - 0.02,
        zt - 0.045,
        zt + 0.03,
        0.010,
        0.004,
    );
    let kb = lkey("taillight_bar", BodyRegion::Trunk, M::Taillight);
    k.rear_box(
        &kb,
        -wt + 0.06,
        wt - 0.06,
        zt - 0.006,
        zt + 0.008,
        0.02,
        0.0,
    );
    for sx in SIDES {
        let tk = taillight_key(sx);
        // Three dashes stepping down toward the corner.
        for i in 0..3 {
            let x = wt - 0.10 - 0.105 * i as f32;
            let dz = -0.008 * (2 - i) as f32;
            k.rear_box(
                &tk,
                sx * (x - 0.045),
                sx * (x + 0.045),
                zt - 0.038 + dz,
                zt - 0.020 + dz,
                0.024,
                0.0,
            );
        }
    }
    // --- Valance with twin rectangular tips (round pipes inside chrome rectangles).
    let ze = (p.z_floor + 0.14).max(0.24);
    k.diffuser(M::Trim, 0.60, 0.12, 4, 0.03);
    let kc = dkey("exhaust_frame", BodyRegion::RearBumper, M::Chrome);
    let kd = dkey("exhaust_frame_inner", BodyRegion::RearBumper, M::Grille);
    for sx in SIDES {
        k.rear_box(&kc, sx * 0.20, sx * 0.40, ze - 0.05, ze + 0.05, 0.02, 0.006);
        k.rear_box(
            &kd,
            sx * 0.215,
            sx * 0.385,
            ze - 0.037,
            ze + 0.037,
            0.024,
            0.0,
        );
    }
    // Blank rear number plate between the light bar and the valance.
    let (pz0, pz1) = (ze + 0.09, (ze + 0.20).min(zt - 0.07));
    let kpf = dkey("number_plate_rear_frame", BodyRegion::Trunk, M::Trim);
    k.rear_box(&kpf, -0.25, 0.25, pz0, pz1, 0.008, 0.003);
    let kp = dkey("number_plate_rear", BodyRegion::Trunk, M::Metal);
    k.rear_box(&kp, -0.235, 0.235, pz0 + 0.012, pz1 - 0.012, 0.012, 0.0);
    // Red reflectors at the valance corners.
    let kr = dkey("rear_reflector_l", BodyRegion::RearBumper, M::Taillight);
    for sx in SIDES {
        let kr = sided(&kr, sx);
        k.rear_box(
            &kr,
            sx * (wt - 0.16),
            sx * (wt - 0.05),
            tz0 + 0.07,
            tz0 + 0.085,
            0.012,
            0.0,
        );
    }

    k.mirrors(0.10, 0.11, None);
    k.door_handles(M::Paint);
    k.fuel_filler(1.0, 0.10, M::Paint);

    // Turn signals as segments beside the lamps (playtest row 136).
    k.signals_beside_lamps(0.06, M::EmissiveAmber);
    shear_to_faces(&mut k, &pre, &[]);
}

// ====================================================================================== Kagero

/// 2010s Japanese hybrid supercar: slim lamps with twin parallel light bars (this fictional
/// maker's signature), narrow upper grille over a straight silver line, black floating roof +
/// A-pillars, flying buttresses with a C-pillar air channel, one big door-to-quarter scoop,
/// Z-shaped full-width tails over a black mesh, quad exhaust in a carbon tray, blue hybrid
/// accent strips.
pub fn kagero(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: 0.30,
        nose_h: Some(0.50),
        cowl_h: 0.08,
        belt_dz: 0.07,
        nose_rake_deg: 38.0,
        nose_plan_round: 0.30,
        tail_plan_round: 0.12,
        hood_crown: 0.025,
        hood_valley: 0.02,
        windshield_rake_deg: Some(62.0),
        a_pillar_rake_deg: 14.0,
        rear_glass_rake_deg: Some(30.0),
        roof_arc: 0.50,
        roof_len_trim: 0.05,
        tumblehome: Some(0.28),
        crease_z: 0.48,
        crease_depth: 0.028,
        haunch: 0.075,
        front_haunch: 0.05,
        flare: 0.010,
        ride_dz: -0.025,
        stance_pitch_deg: 0.2,
        ducktail_h: 0.030,
        belt_kick: 0.03,
        belt_rake: 0.05,
        tail_round: 0.22,
        tail_drop: 0.06,
        roof_dz: 0.03,
        sill_tuck: 0.09,
        ..BodyStyle::preset(Silhouette::Notchback)
    });
    s.details.push(kit_detail(kagero_kit));
    shape3(
        &mut s,
        def,
        Shape3 {
            nose: 0.92,
            tail: 0.94,
            waist: 0.95,
            scallop: -0.06,
            shoulder: 0.015,
            bulge: 0.02,
            rocker: 0.04,
            rise: (0.02, 0.03),
            face: (28.0, 0.05, 10.0, 0.07),
            bow: (0.20, 0.70, 0.10, 0.88),
            lean: 0.03,
        },
    );
    wheels(
        &mut s,
        def,
        RimKind::Turbine,
        15,
        0.76,
        M::Metal,
        CapKind::Centre,
        (0.025, 0.012),
        (0.0, 0.5),
    );
    let m = &mut s.mechanical;
    m.caliper_rgb = Some(rgb(0x1E6FD9));
    s.accent_rgb = Some(rgb(0x1E8CFF));
    s.tints.push((M::Paint, rgb(0xD9DDE2)));
    s
}

fn kagero_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let pre = built_parts(&k);
    let p = ctx.profile;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;

    // Lamp anchors: where the nose top starts behind the raked face, so the lamps lie on the
    // top surface instead of across the face edge.
    let yl = k.yn - p.face_depth(true);
    let wl = p.half_width(yl);
    // --- Slim angular lamps tapering back along each fender, with twin parallel light bars
    //     and a small projector in them, the amber signal at the inner end.
    for sx in SIDES {
        let wf = p.half_width(yl - 0.36);
        let kh = sided(
            &dkey("headlamp_housing_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        let hous = [
            [0.40, yl - 0.07],
            [wl - 0.025, yl - 0.02],
            [wf - 0.04, yl - 0.38],
            [wf - 0.12, yl - 0.36],
            [0.52, yl - 0.14],
        ];
        top_poly(&mut k, &kh, &flip_x(&hous, sx), 0.004, 0.012);
        let hk = headlight_key(sx);
        // Twin bars (this maker's signature): two parallel light bars running from the inner
        // front corner back along the lens to the outer rear, the upper one longer.
        for (d, back) in [(0.0, 0.0), (0.05, 0.06)] {
            let a = [sx * (0.58 + d), yl - 0.07 - 0.6 * d];
            let c = [sx * (wf - 0.09 - d), yl - 0.30 + back];
            top_stroke(&mut k, &hk, a, c, 0.016, 0.018);
        }
        let proj = [
            [wl - 0.20, yl - 0.12],
            [wl - 0.12, yl - 0.10],
            [wl - 0.13, yl - 0.17],
            [wl - 0.21, yl - 0.18],
        ];
        top_poly(&mut k, &hk, &flip_x(&proj, sx), 0.012, 0.010);
        let ik = indicator_front_key(sx);
        let sig = [
            [0.43, yl - 0.085],
            [0.54, yl - 0.085],
            [0.56, yl - 0.12],
            [0.50, yl - 0.13],
        ];
        top_poly(&mut k, &ik, &flip_x(&sig, sx), 0.022, 0.02);
    }
    // --- Front: narrow black upper grille between the lamps over a straight silver line;
    //     big low centre intake; tall corner intakes, each split by one
    //     slanted body-colour blade; blue hybrid accent; carbon splitter.
    let kg = dkey("upper_grille", fb, M::Grille);
    let ug = [
        [-0.42, nz1 - 0.10],
        [0.42, nz1 - 0.10],
        [0.36, nz1 - 0.03],
        [-0.36, nz1 - 0.03],
    ];
    end_poly(&mut k, &kg, true, &ug, 0.006, 0.03);
    let kbl = dkey("nose_blade", fb, M::Metal);
    let blade = [
        [-0.46, nz1 - 0.125],
        [0.46, nz1 - 0.125],
        [0.48, nz1 - 0.105],
        [-0.48, nz1 - 0.105],
    ];
    end_poly(&mut k, &kbl, true, &blade, 0.014, 0.03);
    let (zo0, zo1) = (nz0 + 0.04, nz1 - 0.15);
    for sx in SIDES {
        let (xi, xo) = (0.48, wn - 0.035);
        let kg = sided(&dkey("front_intake_l", fb, M::Grille), sx);
        let intake = [[xi, zo0], [xo, zo0], [xo, zo1 + 0.02], [xi + 0.06, zo1]];
        end_poly(&mut k, &kg, true, &flip_x(&intake, sx), 0.006, 0.03);
        let km = sided(&dkey("front_intake_bars_l", fb, M::Trim), sx);
        for i in 1..3 {
            let z = zo0 + (zo1 - zo0) * i as f32 / 3.0;
            k.front_box(
                &km,
                sx * (xi + 0.06),
                sx * (xo - 0.02),
                z - 0.005,
                z + 0.005,
                0.012,
                0.0,
            );
        }
        let kb = sided(&dkey("front_intake_blade_l", fb, M::Paint), sx);
        let xm = 0.5 * (xi + xo);
        let bl = [
            [xm - 0.02, zo0],
            [xm + 0.02, zo0],
            [xm + 0.07, zo1],
            [xm + 0.03, zo1],
        ];
        end_poly(&mut k, &kb, true, &flip_x(&bl, sx), 0.02, 0.03);
    }
    k.mesh_grille(
        "centre_intake",
        fb,
        0.40,
        nz0 + 0.03,
        nz0 + 0.13,
        1,
        8,
        M::Trim,
    );
    let ka = dkey("hybrid_accent_front", fb, M::Accent);
    k.front_box(&ka, -0.40, 0.40, nz0 + 0.135, nz0 + 0.148, 0.012, 0.0);
    k.splitter(M::Carbon, 0.07, 0.06, 0.02, false);

    // --- Black floating roof and A-pillars.
    roof_sheet(&mut k, "roof_black", 0.01, 0.010, M::Trim);
    let (r0, r1, _, _) = p.roof;
    let kp = dkey("a_pillar_black_l", BodyRegion::Roof, M::Trim);
    for sx in SIDES {
        let kp = sided(&kp, sx);
        let mut ya = p.y_fw;
        while ya > r1 && !p.has_greenhouse(ya) {
            ya -= 0.02;
        }
        let yb = r1 + 0.02;
        let xa = k.roof_edge(ya) - 0.02;
        let xb = k.roof_edge(yb) - 0.02;
        let a = [sx * xa, ya, k.top_z(ya, sx * xa) + 0.006];
        let c = [sx * xb, yb, k.top_z(yb, sx * xb) + 0.006];
        let n = p.top_point(0.5 * (ya + yb), sx * 0.5 * (xa + xb)).1;
        stroke(&mut k, &kp, a, c, n, 0.06, 0.012);
    }

    // --- Flying buttresses (body colour) over a dark air channel through the C-pillar.
    let yb = p.y_deck - 0.30;
    for sx in SIDES {
        let xa = k.roof_edge(r0) - 0.03;
        let xb = p.half_width(yb) - 0.16;
        let a = [sx * xa, r0 - 0.02, k.top_z(r0 - 0.02, sx * xa) + 0.005];
        let c = [sx * xb, yb, k.top_z(yb, sx * xb) + 0.01];
        let kc = sided(
            &dkey("c_pillar_channel_l", BodyRegion::RearLeftQuarter, M::Grille),
            sx,
        );
        let a2 = [a[0] - sx * 0.09, a[1] - 0.04, a[2] - 0.02];
        let c2 = [c[0] - sx * 0.09, c[1], c[2] - 0.01];
        stroke(&mut k, &kc, a2, c2, [sx * 0.5, 0.0, 0.85], 0.10, 0.02);
        let kf = sided(
            &dkey("buttress_l", BodyRegion::RearLeftQuarter, M::Paint),
            sx,
        );
        stroke(&mut k, &kf, a, c, [sx * 0.5, 0.0, 0.85], 0.13, 0.04);
    }

    // --- One big scoop: it starts as a narrow groove in the door under the belt and opens
    //     rearward and downward into the quarter intake ahead of the rear wheel; a folded
    //     body-colour lip over its top edge and a blue hybrid line along its lower edge.
    let (yr, zr, rr) = k.arch(false);
    let zb = p.belt(p.y_fd_rear);
    let ys = p.y_fd_hinge - 0.30;
    let yq = yr + rr + 0.04;
    for sx in SIDES {
        let kd = dkey("door_intake_l", BodyRegion::LeftDoorF, M::Grille);
        let door = [
            [ys, zb - 0.10],
            [p.y_fd_rear + 0.01, zb - 0.07],
            [p.y_fd_rear + 0.01, zr + 0.06],
            [ys + 0.02, zb - 0.13],
        ];
        side_poly(&mut k, &kd, sx, &door, 0.002, 0.01);
        let kq = dkey("quarter_intake_l", BodyRegion::RearLeftQuarter, M::Grille);
        let quarter = [
            [p.y_fd_rear - 0.01, zb - 0.07],
            [yq + 0.06, zb - 0.06],
            [yq, zr + 0.02],
            [p.y_fd_rear - 0.01, zr + 0.06],
        ];
        side_poly(&mut k, &kq, sx, &quarter, 0.002, 0.01);
        let kf = dkey("door_fold_l", BodyRegion::LeftDoorF, M::Paint);
        let lip = [
            [ys - 0.02, zb - 0.085],
            [p.y_fd_rear + 0.01, zb - 0.055],
            [p.y_fd_rear + 0.01, zb - 0.075],
            [ys, zb - 0.10],
        ];
        side_poly(&mut k, &kf, sx, &lip, 0.012, 0.02);
        let kl = dkey("quarter_fold_l", BodyRegion::RearLeftQuarter, M::Paint);
        let lip_q = [
            [p.y_fd_rear - 0.01, zb - 0.055],
            [yq + 0.06, zb - 0.045],
            [yq + 0.06, zb - 0.065],
            [p.y_fd_rear - 0.01, zb - 0.075],
        ];
        side_poly(&mut k, &kl, sx, &lip_q, 0.012, 0.02);
        let ka = dkey("door_accent_l", BodyRegion::LeftDoorF, M::Accent);
        let acc = [
            [ys + 0.10, zb - 0.14],
            [p.y_fd_rear + 0.01, zr + 0.075],
            [p.y_fd_rear + 0.01, zr + 0.06],
            [ys + 0.10, zb - 0.155],
        ];
        side_poly(&mut k, &ka, sx, &acc, 0.008, 0.012);
    }
    k.skirts(M::Trim, 0.03, 0.05);
    fender_outlet(&mut k, 2, M::Paint);
    // Blue accent line along the outer face of each skirt (skirt: 0.03 out of the side).
    let (yf, _, rf) = k.arch(true);
    let ys = 0.5 * (yr + rr + yf - rf);
    let zs = p.sill(ys);
    let xs = k.side_x(ys, zs + 0.06) + 0.03;
    for sx in SIDES {
        let ka = sided(
            &dkey("skirt_accent_l", BodyRegion::FrameLeft, M::Accent),
            sx,
        );
        k.bx(
            &ka,
            [sx * (xs - 0.004), yr + rr + 0.08, zs - 0.004],
            [sx * (xs + 0.004), yf - rf - 0.08, zs + 0.010],
        );
    }

    // --- Engine deck vents either side of the rear glass base.
    for sx in SIDES {
        let name = if sx < 0.0 {
            "deck_vent_l"
        } else {
            "deck_vent_r"
        };
        let (a, c) = (sx * 0.22, sx * 0.48);
        k.top_vent(
            name,
            BodyRegion::Trunk,
            yr - rr + 0.25,
            p.y_deck - 0.08,
            a.min(c),
            a.max(c),
            4,
            M::Trim,
        );
    }
    let kg = dkey("engine_cover_glass", BodyRegion::Trunk, M::Glass);
    k.patch(
        &kg,
        yr - rr + 0.25,
        p.y_deck - 0.08,
        top_band(-0.17, 0.17),
        0.004,
        0.006,
        (3, 1),
    );
    // --- Small carbon active spoiler at the deck edge.
    k.lip_spoiler(M::Carbon, k.yt + 0.22, 0.05, 0.07, 0.24);

    // --- Rear: black mesh fascia, thin full-width bar folding into a Z at each corner.
    let zt = tz1 - 0.04;
    let (fz0, fz1) = (tz0 + 0.10, zt - 0.03);
    k.rear_panel(
        "rear_fascia",
        BodyRegion::RearBumper,
        wt - 0.04,
        fz0,
        fz1,
        M::Grille,
    );
    let km = dkey("rear_fascia_mesh", BodyRegion::RearBumper, M::Trim);
    for i in 1..4 {
        let z = fz0 + (fz1 - fz0) * i as f32 / 4.0;
        k.rear_box(&km, -wt + 0.08, wt - 0.08, z - 0.005, z + 0.005, 0.016, 0.0);
    }
    let kb = lkey("taillight_bar", BodyRegion::Trunk, M::Taillight);
    k.rear_box(&kb, -0.42, 0.42, zt - 0.006, zt + 0.008, 0.022, 0.0);
    let ka = dkey("hybrid_accent_rear", BodyRegion::Trunk, M::Accent);
    k.rear_box(&ka, -0.30, 0.30, zt - 0.03, zt - 0.02, 0.018, 0.0);
    for sx in SIDES {
        let tk = taillight_key(sx);
        let (xa, xb) = (0.42, wt - 0.02);
        k.rear_box(&tk, sx * xa, sx * xb, zt - 0.006, zt + 0.008, 0.022, 0.0);
        rear_stroke(
            &mut k,
            &tk,
            [sx * (xb - 0.01), zt],
            [sx * (xb - 0.17), zt - 0.075],
            0.014,
            0.022,
        );
        k.rear_box(
            &tk,
            sx * (xb - 0.18),
            sx * xb,
            zt - 0.082,
            zt - 0.068,
            0.022,
            0.0,
        );
    }
    // --- Quad tips in a carbon tray, diffuser.
    let ze = (p.z_floor + 0.14).max(0.24);
    let kx = dkey("exhaust_tray", BodyRegion::RearBumper, M::Carbon);
    k.rear_box(&kx, -0.42, 0.42, ze - 0.065, ze + 0.065, 0.02, 0.012);
    k.diffuser(M::Carbon, 0.66, 0.12, 4, 0.06);

    k.mirrors(0.16, 0.10, Some(M::Trim));

    // Turn signals as segments beside the lamps (playtest row 136).
    k.signals_beside_lamps(0.06, M::EmissiveAmber);
    shear_to_faces(
        &mut k,
        &pre,
        &["headlamp_", "headlight_", "indicator_front_"],
    );
}
