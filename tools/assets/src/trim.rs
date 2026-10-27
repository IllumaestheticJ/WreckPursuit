//! Body trim every car gets from the generator (style knobs in [`crate::style::BodyStyle`],
//! "Body details common to every car"): door mirrors on the sail at the A-pillar foot, door
//! handles, panel seams (door cut lines, hood / trunk shut lines, bumper splits), the fuel
//! filler door, wheel-arch lips and liners, the drip-rail line along the roof edge and the
//! windshield wipers. Every piece is bound to the region of the panel it sits on, so it dents,
//! swings and detaches with that panel.
//!
//! Style kits that draw their own version of a piece remove the generic one first
//! ([`remove_generic`] with [`GENERIC_MIRRORS`], [`GENERIC_HANDLES`], …).
//!
//! Units: metres, chassis space (+X right, +Y forward, +Z up). Left-side pieces are built at
//! x < 0 and mirrored.

use wp_contracts::BodyRegion;

use crate::builder::{Builder, PartKey};
use crate::mesh::{MaterialSlot, PartKind};
use crate::profile::{BodyProfile, RING_GLASS_TOP, RING_PILLAR_IN, RING_SHOULDER};
use crate::style::{FuelDoor, TrimFinish};

/// Generic door-mirror parts (housing, black base and arm, mirror glass).
pub const GENERIC_MIRRORS: [&str; 6] = [
    "mirror_l",
    "mirror_r",
    "mirror_base_l",
    "mirror_base_r",
    "mirror_glass_l",
    "mirror_glass_r",
];
/// Generic door-handle parts.
pub const GENERIC_HANDLES: [&str; 8] = [
    "door_handle_f_l",
    "door_handle_f_r",
    "door_handle_r_l",
    "door_handle_r_r",
    "door_handle_recess_f_l",
    "door_handle_recess_f_r",
    "door_handle_recess_r_l",
    "door_handle_recess_r_r",
];
/// Generic fuel-door parts.
pub const GENERIC_FUEL_DOOR: [&str; 4] = [
    "fuel_door_l",
    "fuel_door_r",
    "fuel_door_gap_l",
    "fuel_door_gap_r",
];
/// Generic drip-rail lines.
pub const GENERIC_DRIP_RAIL: [&str; 2] = ["roof_rail_line_l", "roof_rail_line_r"];
/// Generic wipers.
pub const GENERIC_WIPERS: [&str; 1] = ["wipers"];

/// How far seam lines and drip rails stand off the surface, m (clear of z-fighting at 30 m).
pub const SEAM_PROUD_M: f32 = 0.0018;

/// Remove the generic parts `names` (a style kit replacing them with its own).
pub fn remove_generic(b: &mut Builder, names: &[&str]) {
    for n in names {
        b.remove_part(n);
    }
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if l < 1e-9 {
        [0.0, 0.0, 1.0]
    } else {
        scale(a, 1.0 / l)
    }
}

fn key(name: &str, region: BodyRegion, kind: PartKind, mat: MaterialSlot) -> PartKey {
    PartKey::body(name, region, kind, mat)
}

/// Emit the left part `k` (name ending `_l`) and its mirror image.
fn mirrored(b: &mut Builder, k: &PartKey) {
    b.mirror_part(&k.name, &k.mirrored());
}

/// Every generic trim piece of the car (`mid`: the mid LOD keeps only the mirrors and arch
/// liners; its glass is flush, so it has no wipers).
pub fn body_trim(p: &BodyProfile, cowl_edge: Option<f32>, mid: bool, b: &mut Builder) {
    let s = &p.style;
    mirrors(p, s.mirrors, b);
    if s.arch_lip_m > 0.001 && !mid {
        arch_lips(p, s.arch_lip_m.clamp(0.0, 0.04), b);
    }
    if s.arch_liners {
        arch_liners(p, if mid { 5 } else { 9 }, b);
    }
    if mid {
        return;
    }
    wipers(p, cowl_edge, s.wipers.min(2), b);
    door_handles(p, s.door_handles, b);
    fuel_door(p, s.fuel_door, b);
    drip_rail(p, s.drip_rail, b);
    if s.panel_seams {
        panel_seams(p, s.seam_w_m.clamp(0.003, 0.012), b);
    }
}

// ------------------------------------------------------------------ helpers

/// A convex y–z profile (CCW or CW) swept straight along x from `x0` to `x1`, closed.
fn prism_x(b: &mut Builder, k: &PartKey, prof: &[[f32; 2]], x0: f32, x1: f32) {
    let n = prof.len();
    if n < 3 {
        return;
    }
    let cy = prof.iter().map(|q| q[0]).sum::<f32>() / n as f32;
    let cz = prof.iter().map(|q| q[1]).sum::<f32>() / n as f32;
    let c = [0.5 * (x0 + x1), cy, cz];
    let at = |x: f32, q: [f32; 2]| [x, q[0], q[1]];
    for i in 0..n {
        let (a, e) = (prof[i], prof[(i + 1) % n]);
        let quad = [at(x0, a), at(x0, e), at(x1, e), at(x1, a)];
        let mid = scale(add(add(quad[0], quad[1]), add(quad[2], quad[3])), 0.25);
        b.poly_facing(k, &quad, sub(mid, c));
    }
    let cap0: Vec<[f32; 3]> = prof.iter().map(|&q| at(x0, q)).collect();
    let cap1: Vec<[f32; 3]> = prof.iter().map(|&q| at(x1, q)).collect();
    let dir = if x1 > x0 { 1.0 } else { -1.0 };
    b.poly_facing(k, &cap0, [-dir, 0.0, 0.0]);
    b.poly_facing(k, &cap1, [dir, 0.0, 0.0]);
}

/// A thin plate on a surface: centre `c`, outward normal `n`, long axis `t` (unit, ⟂ `n`),
/// half sizes `(along t, along n × t)` and thickness `th` standing out from `c`.
fn surface_plate(
    b: &mut Builder,
    k: &PartKey,
    c: [f32; 3],
    n: [f32; 3],
    t: [f32; 3],
    half: (f32, f32),
    th: f32,
) {
    let n = norm(n);
    let w = norm(cross(n, t));
    let t = cross(w, n);
    b.oriented_box(
        k,
        add(c, scale(n, 0.5 * th)),
        [t, w, n],
        [half.0, half.1, 0.5 * th],
    );
}

/// A seam / trim line: a thin strip of width `w` through the surface points `pts` (with
/// outward normals), [`SEAM_PROUD_M`] off the surface.
fn line_strip(b: &mut Builder, k: &PartKey, pts: &[([f32; 3], [f32; 3])], w: f32) {
    if pts.len() < 2 {
        return;
    }
    let n = pts.len();
    let mut a = Vec::with_capacity(n);
    let mut c = Vec::with_capacity(n);
    for i in 0..n {
        let (q, nrm) = pts[i];
        let t = if i == 0 {
            sub(pts[1].0, q)
        } else if i == n - 1 {
            sub(q, pts[i - 1].0)
        } else {
            sub(pts[i + 1].0, pts[i - 1].0)
        };
        let side = norm(cross(norm(t), nrm));
        let q = add(q, scale(norm(nrm), SEAM_PROUD_M));
        a.push(add(q, scale(side, 0.5 * w)));
        c.push(add(q, scale(side, -0.5 * w)));
    }
    for i in 0..n - 1 {
        let quad = [a[i], a[i + 1], c[i + 1], c[i]];
        let nrm = add(pts[i].1, pts[i + 1].1);
        b.poly_facing(k, &quad, nrm);
    }
}

/// Outward normal (x–z) of the left ring of `y` between points `j0` and `j1`.
fn ring_seg_normal(p: &BodyProfile, y: f32, j0: usize, j1: usize) -> [f32; 3] {
    let r = p.ring(y);
    let (a, e) = (r[j0].p, r[j1].p);
    let (dx, dz) = (e[0] - a[0], e[2] - a[2]);
    let l = (dx * dx + dz * dz).sqrt().max(1e-6);
    [-dz / l, 0.0, dx / l]
}

/// Left side-surface point at (y, z) with its outward normal.
fn side(p: &BodyProfile, y: f32, z: f32) -> ([f32; 3], [f32; 3]) {
    p.side_point(y, z)
}

/// Samples `a..=b` in about `step` steps (at least 2 points).
fn samples(a: f32, b: f32, step: f32) -> Vec<f32> {
    let n = (((b - a).abs() / step).ceil() as usize).max(1);
    (0..=n).map(|i| a + (b - a) * i as f32 / n as f32).collect()
}

/// The front door region of the left side.
const DOOR_F: BodyRegion = BodyRegion::LeftDoorF;

// ------------------------------------------------------------------ mirrors

/// Door mirrors at the A-pillar foot (on the door skin; on the sail with the old layout): a
/// black base on the belt, a black arm, a housing in `finish` (`mirror_l`, kind `Mirror`) and the mirror glass on its rear face.
/// Region: the front doors.
pub fn mirrors(p: &BodyProfile, finish: TrimFinish, b: &mut Builder) {
    let Some(mat) = finish.material() else {
        return;
    };
    let (s0, s1) = p.sail();
    let yc = 0.5 * (s0 + s1);
    let zb = p.belt(yc);
    let w = -side(p, yc, zb - 0.02).0[0];
    let base = key(
        "mirror_base_l",
        DOOR_F,
        PartKind::Detail,
        MaterialSlot::Trim,
    );
    // Base: a black block on the belt ledge / sail (default layout: a low flag on the door
    // skin, under the door glass), and the arm out to the housing.
    let yb0 = yc - 0.06;
    let yb1 = (yc + 0.05).min(p.y_fw);
    let top = if p.style.a_sail_layout == crate::style::SailLayout::ToCorner {
        zb + 0.012
    } else {
        zb + 0.06
    };
    b.cuboid(
        &base,
        [-(w + 0.02), yb0, zb - 0.035],
        [-(w - 0.07), yb1, top],
    );
    b.cuboid(
        &base,
        [-(w + 0.075), yc - 0.045, zb + 0.025],
        [-(w + 0.015), yc + 0.005, zb + 0.055],
    );
    // Housing: blunt glass face at the back, rounded front.
    let housing = key("mirror_l", DOOR_F, PartKind::Mirror, mat);
    let (z0, z1) = (zb + 0.015, zb + 0.125);
    let (ya, len) = (yc - 0.085, 0.115);
    let h = z1 - z0;
    let prof = [
        [ya, z0],
        [ya + 0.6 * len, z0 - 0.006],
        [ya + len, z0 + 0.3 * h],
        [ya + len, z1 - 0.3 * h],
        [ya + 0.6 * len, z1 + 0.006],
        [ya, z1],
    ];
    prism_x(b, &housing, &prof, -(w + 0.06), -(w + 0.225));
    let glass = key(
        "mirror_glass_l",
        DOOR_F,
        PartKind::Detail,
        MaterialSlot::Chrome,
    );
    b.cuboid(
        &glass,
        [-(w + 0.215), ya - 0.004, z0 + 0.012],
        [-(w + 0.07), ya + 0.001, z1 - 0.012],
    );
    for k in [&base, &housing, &glass] {
        mirrored(b, k);
    }
}

// ------------------------------------------------------------------ door handles

/// Door handles: per door a black recess with a grab bar in `finish`, near the door's rear
/// edge just under the belt. Region: the door.
pub fn door_handles(p: &BodyProfile, finish: TrimFinish, b: &mut Builder) {
    let Some(mat) = finish.material() else {
        return;
    };
    let mut doors: Vec<(BodyRegion, &str, f32)> = Vec::new();
    match p.rear_door {
        Some((hinge, rear)) => {
            doors.push((DOOR_F, "f", hinge + 0.15));
            if p.style.silhouette != crate::style::Silhouette::Van {
                doors.push((BodyRegion::LeftDoorR, "r", rear + 0.15));
            }
        }
        None => doors.push((DOOR_F, "f", p.y_fd_rear + 0.17)),
    }
    for (region, tag, y) in doors {
        let z = p.belt(y) - 0.075;
        if z < p.z_door_bottom + 0.1 {
            continue;
        }
        let (c, n) = side(p, y, z);
        let t = [0.0, 1.0, 0.0];
        let recess = key(
            &format!("door_handle_recess_{tag}_l"),
            region,
            PartKind::Detail,
            MaterialSlot::Trim,
        );
        surface_plate(
            b,
            &recess,
            add(c, scale(n, -0.002)),
            n,
            t,
            (0.075, 0.024),
            0.0035,
        );
        let bar = key(
            &format!("door_handle_{tag}_l"),
            region,
            PartKind::Detail,
            mat,
        );
        surface_plate(b, &bar, c, n, t, (0.062, 0.011), 0.011);
        mirrored(b, &recess);
        mirrored(b, &bar);
    }
}

// ------------------------------------------------------------------ fuel door

/// Fuel filler door on a rear quarter: a body-colour octagonal flap on a dark gap ring,
/// above the rear arch. Region: the rear quarter of that side.
pub fn fuel_door(p: &BodyProfile, side_knob: FuelDoor, b: &mut Builder) {
    let right = match side_knob {
        FuelDoor::None => return,
        FuelDoor::Left => false,
        FuelDoor::Right => true,
    };
    let Some(&(yr, zr, rr)) = p
        .arches
        .iter()
        .min_by(|a, c| a.0.total_cmp(&c.0))
        .filter(|a| a.0 < 0.0)
    else {
        return;
    };
    let y = (yr + rr + 0.18)
        .min(p.y_quarter.0 - 0.10)
        .max(p.y_tail + 0.4);
    let zt = p.belt(y) - 0.10;
    let z = zt.max(zr + rr + 0.07);
    if z > p.belt(y) - 0.07 {
        return;
    }
    let (c, n) = side(p, y, z);
    let sx = if right { -1.0 } else { 1.0 };
    let c = [c[0] * sx, c[1], c[2]];
    let n = [n[0] * sx, n[1], n[2]];
    let (region, suffix) = if right {
        (BodyRegion::RearRightQuarter, "r")
    } else {
        (BodyRegion::RearLeftQuarter, "l")
    };
    let gap = key(
        &format!("fuel_door_gap_{suffix}"),
        region,
        PartKind::Detail,
        MaterialSlot::Trim,
    );
    let flap = key(
        &format!("fuel_door_{suffix}"),
        region,
        PartKind::Detail,
        MaterialSlot::Paint,
    );
    let r = 0.062;
    b.cylinder(
        &gap,
        add(c, scale(n, -0.003)),
        add(c, scale(n, 0.0012)),
        r + 0.005,
        r + 0.005,
        8,
        true,
    );
    b.cylinder(
        &flap,
        add(c, scale(n, -0.001)),
        add(c, scale(n, 0.0028)),
        r,
        r,
        8,
        true,
    );
}

// ------------------------------------------------------------------ arches

/// Points along the left wheel-arch opening of `arch` (rear → front over the top), on the
/// side surface, as the loft lifts the side onto it; each with |x| of the side there.
fn arch_path(p: &BodyProfile, arch: (f32, f32, f32), n: usize) -> Vec<[f32; 3]> {
    let (wy, _, r) = arch;
    let s = &p.style;
    let mut out = Vec::new();
    for i in 0..=n {
        let y = wy - 0.985 * r + 1.97 * r * i as f32 / n as f32;
        let Some(z) = p.arch_floor(y) else {
            continue;
        };
        let belt = p.belt(y);
        let ch = s
            .shoulder_round
            .clamp(0.01, 0.12)
            .min(0.3 * (belt - p.sill(y)));
        let z = z.min(belt - ch - 0.03);
        if z < p.sill(y) + 0.06 {
            continue;
        }
        let x = p.half_width(y) + p.haunch(y) + s.flare.clamp(0.0, 0.08) * p.arch_t(y);
        let x = x + p.side_dx(y, z);
        out.push([-x, y, z]);
    }
    out
}

fn arch_region(wy: f32) -> (BodyRegion, &'static str) {
    if wy >= 0.0 {
        (BodyRegion::FrontLeftFender, "f")
    } else {
        (BodyRegion::RearLeftQuarter, "r")
    }
}

/// Body-colour lip round each wheel opening, `w` wide (radially), standing a little proud.
pub fn arch_lips(p: &BodyProfile, w: f32, b: &mut Builder) {
    let mut done: Vec<f32> = Vec::new();
    for &a in &p.arches {
        if done.iter().any(|&y| (y - a.0).abs() < 0.05) {
            continue;
        }
        done.push(a.0);
        let path = arch_path(p, a, 9);
        if path.len() < 3 {
            continue;
        }
        let (region, tag) = arch_region(a.0);
        let mat = match p.style.arch_lip_finish {
            crate::style::TrimFinish::Black => MaterialSlot::Trim,
            crate::style::TrimFinish::Chrome => MaterialSlot::Chrome,
            _ => MaterialSlot::Paint,
        };
        let k = key(&format!("arch_lip_{tag}_l"), region, PartKind::Detail, mat);
        // Wedge profile (radial out, outward), CCW: the edge at the opening stands out most.
        let prof = [[-0.003, -0.006], [w, -0.006], [-0.003, 0.010]];
        b.extrude(&k, &prof, &path, [-1.0, 0.0, 0.0], true);
        mirrored(b, &k);
    }
}

/// Black liners inside the wheel arches: a band from the opening edge in to the wheel's inner
/// face, so the wheel well reads closed. `n` segments per arch.
pub fn arch_liners(p: &BodyProfile, n: usize, b: &mut Builder) {
    let mut done: Vec<f32> = Vec::new();
    for &a in &p.arches {
        if done.iter().any(|&y| (y - a.0).abs() < 0.05) {
            continue;
        }
        done.push(a.0);
        let (wy, wz, _) = a;
        let path = arch_path(p, a, n);
        if path.len() < 2 {
            continue;
        }
        let (region, tag) = arch_region(wy);
        let k = key(
            &format!("arch_liner_{tag}_l"),
            region,
            PartKind::Body,
            MaterialSlot::Trim,
        );
        let inner = (p.half_w - 0.36).max(0.35);
        // Slightly above the opening edge and set in from the side.
        let lift = |q: [f32; 3], x: f32| -> [f32; 3] {
            let d = norm([0.0, q[1] - wy, q[2] - wz]);
            [x, q[1] + d[1] * 0.012, q[2] + d[2] * 0.012]
        };
        let outer: Vec<[f32; 3]> = path.iter().map(|&q| lift(q, q[0] + 0.012)).collect();
        let inn: Vec<[f32; 3]> = path.iter().map(|&q| lift(q, -inner)).collect();
        for i in 0..outer.len() - 1 {
            let quad = [outer[i], outer[i + 1], inn[i + 1], inn[i]];
            let mid = scale(add(add(quad[0], quad[1]), add(quad[2], quad[3])), 0.25);
            // Faces toward the wheel centre.
            b.poly_facing(&k, &quad, [0.0, wy - mid[1], wz - mid[2]]);
        }
        mirrored(b, &k);
    }
}

// ------------------------------------------------------------------ roof / glass

/// Drip-rail line along the roof-rail corner (ring point 11), from the A-pillar foot over the
/// roof to the C-pillar foot. Region: the roof.
pub fn drip_rail(p: &BodyProfile, finish: TrimFinish, b: &mut Builder) {
    let Some(mat) = finish.material() else {
        return;
    };
    let k = key("roof_rail_line_l", BodyRegion::Roof, PartKind::Detail, mat);
    let (_, s1) = p.sail();
    let y_end = p.y_deck.max(p.y_tail + 0.3);
    let mut pts = Vec::new();
    for y in samples(s1 - 0.01, y_end + 0.02, 0.14).into_iter().rev() {
        if p.greenhouse_weight(y) < 0.6 {
            continue;
        }
        let r = p.ring(y);
        let j = RING_GLASS_TOP + 1;
        let q = r[j].p;
        let n = ring_seg_normal(p, y, j - 1, j + 1);
        pts.push((q, n));
    }
    // Front to rear.
    pts.reverse();
    line_strip(b, &k, &pts, 0.009);
    mirrored(b, &k);
}

/// Wipers parked on the windshield just above the cowl (or the windshield base): `count`
/// blades (0…2) with their arms. Region: the windshield.
pub fn wipers(p: &BodyProfile, cowl_edge: Option<f32>, count: u8, b: &mut Builder) {
    if count == 0 || !p.has_greenhouse(p.y_fw - 0.12) {
        return;
    }
    let k = key(
        "wipers",
        BodyRegion::Windshield,
        PartKind::Detail,
        MaterialSlot::Trim,
    );
    let y0 = cowl_edge.unwrap_or(p.y_fw - 0.05);
    let recess = p.style.glass_recess_m.clamp(0.0, 0.03);
    // Glass half width (inside the pillar and frame) at the wiper line.
    let ring = p.ring(y0 - 0.06);
    let hw = (-ring[RING_PILLAR_IN].p[0] - p.style.glass_frame_m.clamp(0.0, 0.04) - 0.03).max(0.2);
    let on_glass = |y: f32, x: f32| -> [f32; 3] {
        let (q, _) = p.top_point(y, x);
        [x, y, q[2] - recess + 0.003]
    };
    let blades: Vec<(f32, f32)> = if count == 1 {
        vec![(-0.85 * hw, 0.55 * hw)]
    } else {
        vec![(-0.92 * hw, -0.06), (0.02, 0.86 * hw)]
    };
    for (xa, xb) in blades {
        // Outer end a little higher up the glass than the inner end (pivot side).
        let (ya, yb) = (y0 - 0.075, y0 - 0.03);
        let a = on_glass(ya, xa);
        let c = on_glass(yb, xb);
        let t = norm(sub(c, a));
        let (_, n) = p.top_point(0.5 * (ya + yb), 0.5 * (xa + xb));
        let half =
            0.5 * ((c[0] - a[0]).powi(2) + (c[1] - a[1]).powi(2) + (c[2] - a[2]).powi(2)).sqrt();
        surface_plate(b, &k, scale(add(a, c), 0.5), n, t, (half, 0.007), 0.012);
        // Arm from the pivot (inner end, at the cowl) to the blade middle.
        let pivot = on_glass(y0 + 0.005, xa + 0.7 * (xb - xa));
        let mid = scale(add(a, c), 0.5);
        let ta = norm(sub(mid, pivot));
        let ha = 0.5 * ((mid[0] - pivot[0]).powi(2) + (mid[1] - pivot[1]).powi(2)).sqrt();
        surface_plate(
            b,
            &k,
            add(scale(add(pivot, mid), 0.5), scale(n, 0.006)),
            n,
            ta,
            (ha, 0.005),
            0.008,
        );
    }
}

// ------------------------------------------------------------------ seams

/// A vertical cut line on the left side at `y` from the door bottom (or sill) up to the belt.
fn cut_line(p: &BodyProfile, y: f32, z0: f32) -> Vec<([f32; 3], [f32; 3])> {
    let r = p.ring(y);
    let mut pts = Vec::new();
    let (q0, n0) = side(p, y, z0);
    pts.push((q0, n0));
    for (j, pt) in r.iter().enumerate().take(RING_SHOULDER + 1).skip(1) {
        let q = pt.p;
        if q[2] <= z0 + 0.02 {
            continue;
        }
        let n = ring_seg_normal(p, y, j - 1, (j + 1).min(RING_SHOULDER + 1));
        pts.push((q, n));
    }
    pts
}

/// A line along the shoulder (ring point 7) from `ya` to `yb`.
fn shoulder_line(p: &BodyProfile, ya: f32, yb: f32) -> Vec<([f32; 3], [f32; 3])> {
    samples(ya, yb, 0.2)
        .into_iter()
        .map(|y| {
            let r = p.ring(y);
            let n = ring_seg_normal(p, y, RING_SHOULDER - 1, RING_SHOULDER + 1);
            (r[RING_SHOULDER].p, n)
        })
        .collect()
}

/// A line across the top at station `y`, from the left shoulder to the centre line (mirrored
/// for the right half).
fn across_line(p: &BodyProfile, y: f32) -> Vec<([f32; 3], [f32; 3])> {
    let r = p.ring(y);
    (RING_SHOULDER..r.len())
        .map(|j| {
            let n = if j + 1 < r.len() {
                ring_seg_normal(p, y, j - 1, j + 1)
            } else {
                [0.0, 0.0, 1.0]
            };
            (r[j].p, n)
        })
        .collect()
}

/// A horizontal line on the left side at height `z` from `ya` to `yb`.
fn side_line(p: &BodyProfile, ya: f32, yb: f32, z: f32) -> Vec<([f32; 3], [f32; 3])> {
    samples(ya, yb, 0.25)
        .into_iter()
        .map(|y| side(p, y, z))
        .collect()
}

/// Panel seams as thin dark lines `w` wide: door cut lines (front, rear, bottom), hood and
/// trunk-lid shut lines, the front / rear bumper splits. Each on the panel it outlines.
pub fn panel_seams(p: &BodyProfile, w: f32, b: &mut Builder) {
    let mut n = 0usize;
    let mut emit =
        |b: &mut Builder, region: BodyRegion, pts: Vec<([f32; 3], [f32; 3])>, mirror: bool| {
            if pts.len() < 2 {
                return;
            }
            let k = key(
                &format!("seam_{n}_l"),
                region,
                PartKind::Detail,
                MaterialSlot::Trim,
            );
            n += 1;
            line_strip(b, &k, &pts, w);
            if mirror {
                mirrored(b, &k);
            }
        };
    let zd = p.z_door_bottom;
    // Doors: front edge, rear edge, bottom.
    let (f_front, f_rear, r_rear) = match p.rear_door {
        Some((rh, re)) => (p.y_fd_hinge, rh, Some(re)),
        None => (p.y_fd_hinge, p.y_fd_rear, None),
    };
    emit(b, DOOR_F, cut_line(p, f_front - 0.004, zd), true);
    emit(b, DOOR_F, cut_line(p, f_rear + 0.004, zd), true);
    emit(
        b,
        DOOR_F,
        side_line(p, f_rear + 0.004, f_front - 0.004, zd + 0.004),
        true,
    );
    if let Some(re) = r_rear {
        let van = p.style.silhouette == crate::style::Silhouette::Van;
        emit(b, BodyRegion::LeftDoorR, cut_line(p, re + 0.004, zd), true);
        if !van {
            emit(
                b,
                BodyRegion::LeftDoorR,
                side_line(p, re + 0.004, f_rear - 0.004, zd + 0.004),
                true,
            );
        }
    }
    // Hood: side shut lines along the shoulder, front line across at the hood's front edge.
    let hood_front = if p.y_nose - p.y_bf > 0.08 {
        p.y_bf
    } else {
        p.y_nose - 0.06
    };
    emit(
        b,
        BodyRegion::Hood,
        shoulder_line(p, p.y_fw + 0.01, hood_front),
        true,
    );
    emit(b, BodyRegion::Hood, across_line(p, hood_front), true);
    // Trunk lid: front line across at the deck start, side lines along the shoulder.
    if p.trunk_lid && p.bed.is_none() {
        let y_end = p.y_tail + 0.5 * p.style.tail_round.clamp(0.05, 0.6);
        emit(b, BodyRegion::Trunk, across_line(p, p.y_deck - 0.01), true);
        emit(
            b,
            BodyRegion::Trunk,
            shoulder_line(p, p.y_deck - 0.01, y_end),
            true,
        );
    }
    // Bumper splits: vertical at the bumper / fender boundary, then forward along the top of
    // the bumper cover.
    let zf = p.z_bumper_top.0 + 0.04;
    if p.y_nose - p.y_bf > 0.1 && zf < p.belt(p.y_bf) - 0.05 {
        let mut v = cut_line(p, p.y_bf + 0.004, p.sill(p.y_bf) + 0.03);
        v.retain(|q| q.0[2] <= zf + 1e-3);
        v.push(side(p, p.y_bf + 0.004, zf));
        emit(b, BodyRegion::FrontBumper, v, true);
        emit(
            b,
            BodyRegion::FrontBumper,
            side_line(p, p.y_bf + 0.004, p.y_nose - 0.10, zf),
            true,
        );
    }
    let zr = p.z_bumper_top.1 + 0.04;
    if p.y_quarter.1 - p.y_tail > 0.1 && zr < p.belt(p.y_quarter.1) - 0.05 {
        let y = p.y_quarter.1 - 0.004;
        let mut v = cut_line(p, y, p.sill(y) + 0.03);
        v.retain(|q| q.0[2] <= zr + 1e-3);
        v.push(side(p, y, zr));
        emit(b, BodyRegion::RearBumper, v, true);
        emit(
            b,
            BodyRegion::RearBumper,
            side_line(p, p.y_tail + 0.10, y, zr),
            true,
        );
    }
}
