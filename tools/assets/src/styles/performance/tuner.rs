//! Tuners / sport: `kestrel_ts` (2000s sport sedan, rally-bred), `arashi_t4` (mid-90s AWD
//! rally saloon), `talus_20t` (late-90s AWD rally saloon), `shiro_turbo` (early-2000s FR
//! coupe), `hakone_gt` (90s twin-turbo GT liftback), `sperber_32` (2000s German I6 sport
//! sedan), `debug_coupe` (sport coupe).
//!
//! Each car has its own body knobs and kit (car-shapes phase 2, 2026-10-27; references in
//! `docs/integration/car-shapes-tuner.md`). Genre cues: wings that belong to the stock car
//! (pedestal wings on the rally saloons, a hoop wing on the GT, a boot lip on the sedan),
//! shaped lamps (swept, angular, twin round, wide two-tier tails), real grille and intake
//! shapes, a dropped stance on aftermarket-looking wheels.
//!
//! Phase 3 (pass 3): every car also uses the shared section and plan knobs (`plan_width`,
//! `side_sculpt`, `side_bulge_m`, `rocker_in_m`, fender rises, long overhang trims) and the
//! projected lamp clusters (`Kit::proj_cluster`). Local helpers: [`face_poly`] (shaped grilles
//! and intakes), [`swept_top_lamp`] (a top-view swept lamp on `proj_cluster`), [`blister`],
//! [`shoulder_line`].

use wp_contracts::{BodyRegion, VehicleDef};

use crate::builder::{Builder, PartKey};
use crate::lamp::LampView;
use crate::mesh::{MaterialSlot as M, PartKind};
use crate::style::{
    ArchShape, BodyStyle, CaliperKind, CapKind, CarStyle, DetailCtx, EngineKind, EngineLayout,
    ExhaustKind, FuelDoor, HoodScoop, Induction, RimKind, SideSculpt, Silhouette, SuspensionKind,
    TrimFinish,
};

use super::kit::{
    dkey, headlight_key, indicator_front_key, indicator_rear_key, lkey, sided, taillight_key,
    top_band, Kit, Seg, Stand, Wing, SIDES,
};
use super::{kit_detail, rgb, wheels};

// ---------------------------------------------------------------------- local helpers

/// Convex outline `(x, z)` on the nose (`front`) or tail face, standing `proud` m out of it
/// (its back buried 2 cm in the body): lamp lenses, grilles and intakes with real shapes.
fn face_poly(k: &mut Kit, key: &PartKey, front: bool, outline: &[[f32; 2]], proud: f32) {
    let (y0, y1) = if front {
        (k.yn - 0.02, k.yn + proud)
    } else {
        (k.yt + 0.02, k.yt - proud)
    };
    k.prism_y(key, outline, y0, y1);
}

/// An outline given for the right side (|x|) placed on side `sx`.
fn on_side(sx: f32, outline: &[[f32; 2]]) -> Vec<[f32; 2]> {
    outline.iter().map(|q| [sx * q[0], q[1]]).collect()
}

/// Head lamp lying on the nose top that sweeps back into the fender (side `sx`), drawn in the
/// top view and projected onto the body (`Kit::proj_cluster`): the lens runs from |x| `xin`
/// to `margin` m inside the body edge at its front edge `y1`; its inboard edge goes back to
/// `y0`, its outboard edge `sweep` m further, so the rear edge is a diagonal (a swept, tapering
/// lamp). The outer edge follows the body's plan width (a bowed nose narrows it toward the
/// tip). The outboard `sig_w` is the amber turn signal; a flush dark bezel `border` m wide
/// rings it.
#[allow(clippy::too_many_arguments)]
fn swept_top_lamp(
    k: &mut Kit,
    sx: f32,
    y0: f32,
    y1: f32,
    xin: f32,
    margin: f32,
    sweep: f32,
    border: f32,
    sig_w: f32,
) {
    // Behind a raked nose face: the lamp lies on the nose top, not down the face.
    let back = k.p.face_depth(true);
    let (y0, y1) = (y0 - back, (y1 - back).min(k.yn - 0.01));
    let yr = y0 - sweep;
    let xo1 = k.p.half_width(y1) - margin;
    let xor = k.p.half_width(yr) - margin - 0.03;
    // Keep a real lens when a narrow nose pulls the outer edge in (knob sweeps).
    let xin = xin.min(xo1 - 0.15);
    let sig_w = sig_w.min(0.4 * (xo1 - xin));
    let outline = [[xin, y1], [xo1, y1], [xor.max(xin + 0.1), yr], [xin, y0]];
    k.proj_cluster(
        sx,
        true,
        LampView::Top,
        &outline,
        Some(Seg::Outboard(sig_w)),
        M::EmissiveAmber,
        border,
        0.0,
    );
}

/// Body-shell triangles (loft panels and body parts) of a builder, for [`surface_y`].
fn shell_tris(b: &Builder) -> Vec<[[f32; 3]; 3]> {
    let mut out = Vec::new();
    for key in b.keys() {
        if !matches!(key.kind, PartKind::Panel | PartKind::Body) {
            continue;
        }
        if let Some((pos, _, idx)) = b.geometry(&key.name) {
            for t in idx.chunks(3) {
                if t.len() == 3 {
                    out.push([pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]]);
                }
            }
        }
    }
    out
}

/// Where a ray along −y (`front`, from far ahead) or +y (from far behind) at (x, z) first meets
/// the shell: the largest (front) / smallest y of the triangles over that point, if any.
fn surface_y(tris: &[[[f32; 3]; 3]], x: f32, z: f32, front: bool) -> Option<f32> {
    let mut best: Option<f32> = None;
    for t in tris {
        let (a, b, c) = (t[0], t[1], t[2]);
        // Barycentric coordinates of (x, z) in the triangle projected on the x–z plane.
        let d = (b[0] - a[0]) * (c[2] - a[2]) - (c[0] - a[0]) * (b[2] - a[2]);
        if d.abs() < 1e-9 {
            continue;
        }
        let u = ((x - a[0]) * (c[2] - a[2]) - (c[0] - a[0]) * (z - a[2])) / d;
        let v = ((b[0] - a[0]) * (z - a[2]) - (x - a[0]) * (b[2] - a[2])) / d;
        if u < 0.0 || v < 0.0 || u + v > 1.0 {
            continue;
        }
        let y = a[1] + u * (b[1] - a[1]) + v * (c[1] - a[1]);
        best = Some(match best {
            None => y,
            Some(bb) if front => bb.max(y),
            Some(bb) => bb.min(y),
        });
    }
    best
}

/// Moves the kit parts built on the old vertical nose / tail face plane (detail and light
/// parts lying within 0.4 m of the end and reaching its plane) onto the real end face, which
/// the face rake, chin and plan bow knobs set back: rays on a 3 × 4 grid over the part's x and
/// z span find the shell, and every vertex moves by the bilinear blend of those offsets, so the
/// part keeps its distance from a curved face. Parts named `<skip>_l` / `<skip>_r` (projected
/// lamps, which already follow the surface) stay put.
fn follow_faces(k: &mut Kit, skip: &[&str]) {
    let (yn, yt) = (k.yn, k.yt);
    let tris = shell_tris(k.b);
    for key in k.b.keys() {
        if !matches!(key.kind, PartKind::Detail | PartKind::Light) || key.wheel.is_some() {
            continue;
        }
        if skip
            .iter()
            .any(|s| key.name == format!("{s}_l") || key.name == format!("{s}_r"))
        {
            continue;
        }
        let Some((pos, _, _)) = k.b.geometry(&key.name) else {
            continue;
        };
        if pos.is_empty() {
            continue;
        }
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for q in pos {
            for i in 0..3 {
                lo[i] = lo[i].min(q[i]);
                hi[i] = hi[i].max(q[i]);
            }
        }
        let front = lo[1] > yn - 0.40 && hi[1] > yn - 0.05;
        let rear = hi[1] < yt + 0.40 && lo[1] < yt + 0.05;
        if !(front || rear) {
            continue;
        }
        // The old face plane the kit placed the part on.
        let plane = if front { yn } else { yt };
        let (nx, nz) = (3usize, 4usize);
        let xs: Vec<f32> = (0..nx)
            .map(|i| lo[0] + (hi[0] - lo[0]) * i as f32 / (nx - 1) as f32)
            .collect();
        let zs: Vec<f32> = (0..nz)
            .map(|j| {
                (lo[2] + 0.005) + ((hi[2] - lo[2]) - 0.01).max(0.0) * j as f32 / (nz - 1) as f32
            })
            .collect();
        let mut grid = vec![[0.0f32; 4]; nx];
        let mut moved = false;
        for (i, &x) in xs.iter().enumerate() {
            // A ray over the top of the face (or under it) meets the roof / underbody far from
            // the end: those count as misses and take the nearest hit in the column.
            let col: Vec<Option<f32>> = zs
                .iter()
                .map(|&z| {
                    surface_y(&tris, x, z, front)
                        .map(|ys| ys - plane)
                        .filter(|d| d.abs() <= 0.25)
                })
                .collect();
            for (j, cell) in grid[i].iter_mut().enumerate() {
                let near = col
                    .iter()
                    .enumerate()
                    .filter_map(|(jj, c)| c.map(|d| (jj.abs_diff(j), d)))
                    .min_by_key(|e| e.0)
                    .map(|e| e.1)
                    .unwrap_or(0.0);
                *cell = near;
                moved |= near.abs() >= 0.002;
            }
        }
        if !moved {
            continue;
        }
        let blend = |v: f32, axis: &[f32]| -> (usize, f32) {
            let n = axis.len();
            if axis[n - 1] - axis[0] < 1e-6 {
                return (0, 0.0);
            }
            let t = ((v - axis[0]) / (axis[n - 1] - axis[0])).clamp(0.0, 1.0) * (n - 1) as f32;
            let i = (t.floor() as usize).min(n - 2);
            (i, t - i as f32)
        };
        let name = key.name.clone();
        k.b.transform(
            |kk| kk.name == name,
            |q| {
                let (i, u) = blend(q[0], &xs);
                let (j, v) = blend(q[2], &zs);
                let g = |a: usize, b: usize| grid[a.min(nx - 1)][b.min(nz - 1)];
                let d = (1.0 - u) * ((1.0 - v) * g(i, j) + v * g(i, j + 1))
                    + u * ((1.0 - v) * g(i + 1, j) + v * g(i + 1, j + 1));
                [q[0], q[1] + d, q[2]]
            },
        );
    }
}

/// Front (`true`) / rear axle station and wheel radius from the car def.
fn axle(def: &VehicleDef, front: bool) -> (f32, f32) {
    let mut best: Option<(f32, f32)> = None;
    for w in &def.wheels {
        let y = w.position_m[1];
        let better = match best {
            None => true,
            Some((by, _)) => (front && y > by) || (!front && y < by),
        };
        if better {
            best = Some((y, w.radius_m));
        }
    }
    best.unwrap_or(if front { (1.3, 0.31) } else { (-1.3, 0.31) })
}

/// Shoulder line: a thin band `drop` m under the belt height `belt` standing `dx` m proud along
/// the whole car (a crisp character line under the window line).
fn shoulder_line(belt: f32, drop: f32, dx: f32) -> SideSculpt {
    SideSculpt {
        y0_m: -3.0,
        y1_m: 3.0,
        z0_m: belt - drop - 0.03,
        z1_m: belt - drop,
        dx_m: dx,
        falloff_m: 0.035,
    }
}

/// Blister over a wheel arch at station `y` (radius `r`): the side from `z0` up to `z1`
/// standing `dx` m proud, easing out over `falloff`.
fn blister(y: f32, r: f32, z0: f32, z1: f32, dx: f32, falloff: f32) -> SideSculpt {
    SideSculpt {
        y0_m: y - r - 0.05,
        y1_m: y + r + 0.05,
        z0_m: z0,
        z1_m: z1,
        dx_m: dx,
        falloff_m: falloff,
    }
}

/// Red reflectors low on the rear bumper corners (|x| `xin..xout`, `z0..z1`).
fn rear_reflectors(k: &mut Kit, xin: f32, xout: f32, z0: f32, z1: f32) {
    for sx in SIDES {
        let kr = sided(
            &dkey("rear_reflector_l", BodyRegion::RearBumper, M::Taillight),
            sx,
        );
        k.rear_box(&kr, sx * xin, sx * xout, z0, z1, 0.008, 0.003);
    }
}

/// Blank badge (no lettering) on the nose or tail face at (x, z), `w` × `h`.
fn blank_badge(k: &mut Kit, region: BodyRegion, front: bool, z: f32, w: f32, h: f32) {
    let kb = dkey(
        if front { "badge_front" } else { "badge_rear" },
        region,
        M::Chrome,
    );
    let (a, c) = (0.5 * w, 0.5 * h);
    let o = [
        [-a, z - 0.6 * c],
        [-0.6 * a, z - c],
        [0.6 * a, z - c],
        [a, z - 0.6 * c],
        [a, z + 0.6 * c],
        [0.6 * a, z + c],
        [-0.6 * a, z + c],
        [-a, z + 0.6 * c],
    ];
    face_poly(k, &kb, front, &o, 0.022);
}

// ---------------------------------------------------------------------- kestrel_ts

/// Mid-2000s Japanese sport sedan with the rally-bred look (`kestrel_ts`): high belt, swept
/// lamps running back into the fenders, a slatted trapezoid upper slot, wide
/// trapezoid lower mouth with fog pods, top-mount intercooler scoop, box flares, tall
/// pedestal wing, round-element tails, roof vent, mud flaps, gold wheels.
pub fn kestrel(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        nose_h: Some(0.62),
        nose_rake_deg: 24.0,
        nose_plan_round: 0.30,
        tail_plan_round: 0.24,
        cowl_h: 0.04,
        hood_crown: 0.015,
        hood_bulge_h: 0.015,
        hood_bulge_w: 0.42,
        hood_scoop: HoodScoop::Scoop,
        hood_scoop_h: 0.065,
        windshield_rake_deg: Some(60.0),
        a_pillar_rake_deg: 9.0,
        rear_glass_rake_deg: Some(53.0),
        roof_arc: 0.15,
        tumblehome: Some(0.14),
        belt_dz: 0.03,
        crease_z: 0.60,
        crease_depth: 0.010,
        shoulder_round: 0.07,
        haunch: 0.020,
        front_haunch: 0.030,
        flare: 0.030,
        flare_shape: ArchShape::Box,
        ride_dz: -0.02,
        ducktail_h: 0.015,
        belt_kick: 0.015,
        roof_len_trim: 0.15,
        c_pillar_w_m: 0.14,
        windshield_wrap_m: 0.07,
        fuel_door: FuelDoor::Right,
        sill_tuck: 0.09,
        tail_round: 0.25,
        arch_gap: 0.045,
        ..BodyStyle::preset(Silhouette::Notchback)
    });
    // Pass 3: 2000s sedan section (convex side, tucked sill, crisp shoulder), gently
    // tapered ends, flared arches standing proud, shorter overhangs.
    let (fy, fr) = axle(def, true);
    let (ry, rr) = axle(def, false);
    let belt = 0.59 * def.body_size_m[2];
    let b = &mut s.body;
    b.plan_width = vec![
        [fy + fr, 1.0],
        [fy + fr + 0.75, 0.93],
        [ry - rr, 1.0],
        [ry - rr - 0.8, 0.95],
    ];
    b.side_bulge_m = 0.025;
    b.side_bulge_z = 0.55;
    b.rocker_in_m = 0.04;
    b.side_sculpt = vec![
        shoulder_line(belt, 0.08, 0.02),
        blister(fy, fr, 0.25, belt - 0.05, 0.025, 0.12),
        blister(ry, rr, 0.25, belt - 0.05, 0.03, 0.12),
    ];
    // Peak band only at the very edge, so the swept lamps lie wholly on the hood side.
    b.fender_peak_w = 0.95;
    b.rear_fender_rise_m = 0.02;
    b.overhang_trim = (-0.10, -0.12);
    b.nose_face_rake_deg = 22.0;
    b.nose_chin_m = 0.06;
    b.nose_plan_bow_m = 0.25;
    b.nose_tip_w = 0.75;
    b.tail_face_rake_deg = 10.0;
    b.roof_plan_taper = 0.08;
    s.details.push(kit_detail(kestrel_kit));
    wheels(
        &mut s,
        def,
        RimKind::MultiSpoke,
        12,
        0.70,
        M::Metal,
        CapKind::Centre,
        (0.03, 0.015),
        (0.0, 1.0),
    );
    let m = &mut s.mechanical;
    m.engine = EngineKind::Inline4;
    m.induction = Induction::Turbo;
    m.front_suspension = SuspensionKind::Strut;
    m.rear_suspension = SuspensionKind::Multilink;
    m.caliper_rgb = Some(rgb(0xC81E1E));
    m.exhaust = ExhaustKind::Single;
    m.exhaust_tip_r_m = 0.055;
    s.tints.push((M::Paint, rgb(0x1D3F8C)));
    // Gold rally wheels (`rim` slot: discs and hardware stay metal).
    s.wheel.rim_rgb = Some(rgb(0xB8963E));
    // Subtle blue underglow, lit with the headlights (research §1.3 tuner neon).
    s.underglow_rgb = Some(rgb(0x2050D0));
    s
}

fn kestrel_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;

    // --- Front: swept lamps on the nose top, running back into the fenders.
    for sx in SIDES {
        swept_top_lamp(
            &mut k,
            sx,
            yn - 0.22,
            yn - 0.03,
            0.60,
            0.04,
            0.16,
            0.018,
            0.07,
        );
    }
    // Trapezoid upper slot grille (wider at the top) with two dark slats and a blank badge.
    let (gz0, gz1) = (nz1 - 0.11, nz1 - 0.03);
    let g = dkey("grille", BodyRegion::Hood, M::Grille);
    face_poly(
        &mut k,
        &g,
        true,
        &[[-0.31, gz1], [0.31, gz1], [0.26, gz0], [-0.26, gz0]],
        0.006,
    );
    let gb = dkey("grille_slats", BodyRegion::Hood, M::Trim);
    for t in [0.35, 0.68] {
        let z = gz0 + t * (gz1 - gz0);
        k.front_box(&gb, -0.27, 0.27, z - 0.006, z + 0.006, 0.012, 0.0);
    }
    let zg = 0.5 * (gz0 + gz1);
    blank_badge(&mut k, BodyRegion::Hood, true, zg, 0.09, 0.04);
    // Wide trapezoid lower mouth with a mesh, fog pods in the corners.
    let (zm0, zm1) = (nz0 + 0.06, nz0 + 0.26);
    let lm = dkey("lower_mouth", fb, M::Grille);
    face_poly(
        &mut k,
        &lm,
        true,
        &[[-0.50, zm0], [0.50, zm0], [0.42, zm1], [-0.42, zm1]],
        0.006,
    );
    k.mesh_grille(
        "lower_grille",
        fb,
        0.40,
        zm0 + 0.02,
        zm1 - 0.02,
        3,
        6,
        M::Trim,
    );
    let zf = 0.5 * (zm0 + zm1);
    for sx in SIDES {
        let kp = sided(&dkey("fog_pod_l", fb, M::Grille), sx);
        let x0 = wn - 0.24;
        let x1 = wn - 0.07;
        let o = on_side(
            sx,
            &[
                [x0, zf - 0.06],
                [x1, zf - 0.07],
                [x1, zf + 0.05],
                [x0 + 0.03, zf + 0.07],
            ],
        );
        face_poly(&mut k, &kp, true, &o, 0.008);
    }
    k.fogs(wn - 0.155, zf, 0.05);
    k.splitter(M::Trim, 0.05, 0.10, 0.025, false);
    let kt = dkey("tow_hook_front", fb, M::Caliper);
    k.front_box(&kt, 0.30, 0.36, zm0 - 0.01, zm0 + 0.03, 0.06, 0.004);

    // --- Rear: tall pedestal wing, round-element tails, reflectors, mud flaps.
    k.wing(Wing {
        lead: 0.36,
        chord: 0.24,
        height: 0.19,
        half_span: 0.70,
        thick: 0.035,
        aoa_deg: 8.0,
        stand: Stand::Pedestal(0.50),
        endplates: Some((0.10, 0.24)),
        mat: M::Paint,
        stand_mat: M::Paint,
        gurney: 0.0,
    });
    for sx in SIDES {
        let kh = sided(
            &dkey("taillight_housing_l", BodyRegion::RearLeftQuarter, M::Trim),
            sx,
        );
        let (z0, z1) = (tz1 - 0.17, tz1 - 0.02);
        let (xa, xb) = (sx * 0.36, sx * (wt - 0.02));
        k.rear_box(&kh, xa, xb, z0, z1, 0.012, 0.006);
        // Amber inboard section of the lens, clear of the inner round element.
        let r = 0.5 * (z1 - z0) - 0.016;
        let x_round = 0.5 * (0.36 + wt - 0.02) - 0.09 - r;
        let w_sig = (x_round - 0.012 - 0.35).clamp(0.04, 0.09);
        k.rear_cluster(
            sx,
            0.35,
            wt - 0.02,
            z0 + 0.012,
            z1 - 0.012,
            0.018,
            0.006,
            Seg::Inboard(w_sig),
            M::EmissiveAmber,
        );
        let kr = sided(
            &lkey(
                "taillight_round_l",
                BodyRegion::RearLeftQuarter,
                M::Taillight,
            ),
            sx,
        );
        let xm = 0.5 * (xa + xb);
        let kring = sided(
            &dkey("taillight_ring_l", BodyRegion::RearLeftQuarter, M::Chrome),
            sx,
        );
        for dx in [-0.09, 0.09] {
            k.round_lamp(&kr, false, xm + dx, 0.5 * (z0 + z1), r, 0.032, Some(&kring));
        }
    }
    let kp = dkey("plate_recess", BodyRegion::Trunk, M::Trim);
    k.rear_box(&kp, -0.24, 0.24, tz1 - 0.17, tz1 - 0.05, 0.004, 0.0);
    blank_badge(&mut k, BodyRegion::Trunk, false, tz1 - 0.02, 0.10, 0.03);
    let kv = dkey("rear_valance", BodyRegion::RearBumper, M::Trim);
    k.rear_box(
        &kv,
        -wt + 0.16,
        wt - 0.16,
        tz0 + 0.02,
        tz0 + 0.09,
        0.012,
        0.004,
    );
    rear_reflectors(&mut k, wt - 0.20, wt - 0.06, tz0 + 0.11, tz0 + 0.14);
    k.mud_flaps(M::Trim);

    // --- Roof vent scoop (rally), skirts.
    let ry = p.roof.1 - 0.12;
    let rz = k.top_z(ry, 0.0);
    let kr = dkey("roof_scoop", BodyRegion::Roof, M::Paint);
    k.prism_x(
        &kr,
        &[
            [ry + 0.02, rz - 0.01],
            [ry - 0.20, rz - 0.01],
            [ry - 0.20, rz + 0.045],
            [ry - 0.05, rz + 0.045],
        ],
        -0.10,
        0.10,
    );
    let km = dkey("roof_scoop_mouth", BodyRegion::Roof, M::Grille);
    k.bx(
        &km,
        [-0.085, ry - 0.06, rz + 0.005],
        [0.085, ry - 0.045, rz + 0.038],
    );
    k.skirts(M::Paint, 0.025, 0.06);
    k.mirrors(0.10, 0.11, None);
    k.fender_repeaters();
    follow_faces(&mut k, &["headlight", "indicator_front", "headlamp_bezel"]);
}

// ---------------------------------------------------------------------- arashi_t4

/// Mid-90s Japanese AWD rally-homologation saloon (`arashi_t4`): a compact, upright, boxy
/// three-box body with a tall greenhouse and thin pillars, square rectangular head lamps
/// with amber corners that wrap round the front fender, a slim slot grille, a deep bumper
/// with a central intake and big round driving lamps, a bonnet scoop, a tall pedestal wing on
/// the boot, wide rectangular tails joined by a red reflector panel, mud flaps.
pub fn arashi(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        nose_h: Some(0.66),
        nose_rake_deg: 10.0,
        nose_plan_round: 0.18,
        tail_plan_round: 0.16,
        cowl_h: 0.025,
        hood_crown: 0.012,
        hood_bulge_h: 0.0,
        hood_bulge_w: 0.36,
        hood_scoop: HoodScoop::Scoop,
        hood_scoop_h: 0.055,
        windshield_rake_deg: Some(56.0),
        a_pillar_rake_deg: 6.0,
        rear_glass_rake_deg: Some(48.0),
        roof_dz: 0.01,
        roof_crown: 0.012,
        roof_len_trim: 0.15,
        tumblehome: Some(0.12),
        belt_dz: 0.01,
        crease_z: 0.50,
        crease_depth: 0.008,
        shoulder_round: 0.035,
        haunch: 0.020,
        front_haunch: 0.020,
        flare: 0.022,
        flare_shape: ArchShape::Box,
        ride_dz: -0.01,
        ducktail_h: 0.0,
        tail_round: 0.15,
        tail_drop: 0.04,
        a_pillar_w_m: 0.07,
        c_pillar_w_m: 0.13,
        windshield_wrap_m: 0.04,
        rear_glass_wrap_m: 0.03,
        end_round_m: 0.025,
        fuel_door: FuelDoor::Right,
        sill_tuck: 0.07,
        arch_gap: 0.045,
        ..BodyStyle::preset(Silhouette::Notchback)
    });
    // Pass 3: boxy 90s section (near-flat side, small tuck), square ends, box flares.
    let (fy, fr) = axle(def, true);
    let (ry, rr) = axle(def, false);
    let belt = 0.59 * def.body_size_m[2];
    let b = &mut s.body;
    b.plan_width = vec![
        [fy + fr, 1.0],
        [fy + fr + 0.7, 0.96],
        [ry - rr, 1.0],
        [ry - rr - 0.7, 0.97],
    ];
    b.side_bulge_m = 0.012;
    b.rocker_in_m = 0.03;
    b.side_sculpt = vec![
        shoulder_line(belt, 0.06, 0.012),
        blister(fy, fr, 0.22, 0.58, 0.022, 0.06),
        blister(ry, rr, 0.22, 0.58, 0.022, 0.06),
    ];
    b.front_fender_rise_m = 0.015;
    b.overhang_trim = (-0.08, -0.10);
    b.nose_chin_m = 0.03;
    // No end-face rake: front / rear-view projected lamps land inside a raked face
    // (generator request, see the integration doc); the chin is fine.
    b.roof_plan_taper = 0.04;
    s.details.push(kit_detail(arashi_kit));
    wheels(
        &mut s,
        def,
        RimKind::SixSpoke,
        6,
        0.68,
        M::Metal,
        CapKind::Centre,
        (0.025, 0.012),
        (0.0, 0.8),
    );
    let m = &mut s.mechanical;
    m.engine = EngineKind::Inline4;
    m.induction = Induction::Turbo;
    m.front_suspension = SuspensionKind::Strut;
    m.rear_suspension = SuspensionKind::Multilink;
    m.caliper_rgb = Some(rgb(0xC81E1E));
    m.exhaust = ExhaustKind::Single;
    m.exhaust_tip_r_m = 0.055;
    s.tints.push((M::Paint, rgb(0xEDEDEA)));
    s.wheel.rim_rgb = Some(rgb(0xC9CCD1));
    s
}

fn arashi_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;

    // --- Front: square lamps with amber corners wrapping onto the fenders, slot grille.
    let (lz0, lz1) = (nz1 - 0.14, nz1 - 0.025);
    for sx in SIDES {
        // Square lens projected on the nose; the yaw wraps its amber corner onto the fender.
        k.proj_cluster(
            sx,
            true,
            LampView::Front,
            &[
                [0.285, lz0],
                [wn - 0.03, lz0],
                [wn - 0.03, lz1],
                [0.285, lz1],
            ],
            Some(Seg::Outboard(0.10)),
            M::EmissiveAmber,
            0.025,
            22.0,
        );
        // A lamp-washer-style divider splitting the lens into main / high beam.
        let kd = sided(
            &dkey("headlamp_divider_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        let xd = 0.5 * (0.285 + wn - 0.03 - 0.10);
        k.front_box(
            &kd,
            sx * (xd - 0.006),
            sx * (xd + 0.006),
            lz0,
            lz1,
            0.022,
            0.0,
        );
    }
    k.grille(
        "grille",
        BodyRegion::Hood,
        0.25,
        nz1 - 0.11,
        nz1 - 0.05,
        2,
        M::Trim,
        None,
    );
    blank_badge(&mut k, BodyRegion::Hood, true, nz1 - 0.08, 0.09, 0.04);
    // Deep bumper: central intake, big round driving lamps, tow hook.
    let (zm0, zm1) = (nz0 + 0.07, nz0 + 0.22);
    let lm = dkey("lower_mouth", fb, M::Grille);
    face_poly(
        &mut k,
        &lm,
        true,
        &[[-0.34, zm0], [0.34, zm0], [0.30, zm1], [-0.30, zm1]],
        0.006,
    );
    let ls = dkey("lower_mouth_slats", fb, M::Trim);
    for dz in [0.05, 0.10] {
        k.front_box(
            &ls,
            -0.30,
            0.30,
            zm0 + dz - 0.007,
            zm0 + dz + 0.007,
            0.012,
            0.0,
        );
    }
    k.fogs(wn - 0.17, 0.5 * (zm0 + zm1) + 0.01, 0.07);
    k.splitter(M::Trim, 0.03, 0.12, 0.02, false);
    let kt = dkey("tow_hook_front", fb, M::Caliper);
    k.front_box(&kt, -0.36, -0.30, zm0 - 0.03, zm0 + 0.01, 0.06, 0.004);

    // --- Rear: tall pedestal wing, wide tails joined by a red reflector panel.
    k.wing(Wing {
        lead: 0.30,
        chord: 0.20,
        height: 0.15,
        half_span: 0.66,
        thick: 0.03,
        aoa_deg: 6.0,
        stand: Stand::Pedestal(0.48),
        endplates: Some((0.07, 0.20)),
        mat: M::Paint,
        stand_mat: M::Paint,
        gurney: 0.008,
    });
    let (z0, z1) = (tz1 - 0.15, tz1 - 0.03);
    for sx in SIDES {
        k.proj_cluster(
            sx,
            false,
            LampView::Rear,
            &[[0.235, z0], [wt - 0.02, z0], [wt - 0.02, z1], [0.235, z1]],
            Some(Seg::Inboard(0.08)),
            M::EmissiveAmber,
            0.012,
            30.0,
        );
        // Horizontal ribs across the lens (90s multi-section tail).
        let kr = sided(
            &dkey("taillight_rib_l", BodyRegion::RearLeftQuarter, M::Trim),
            sx,
        );
        let zr = 0.5 * (z0 + z1);
        k.rear_box(
            &kr,
            sx * 0.33,
            sx * (wt - 0.02),
            zr - 0.004,
            zr + 0.004,
            0.022,
            0.0,
        );
    }
    let kg = dkey("tail_garnish", BodyRegion::Trunk, M::Taillight);
    k.rear_box(&kg, -0.21, 0.21, z0 + 0.02, z1 - 0.01, 0.012, 0.004);
    blank_badge(
        &mut k,
        BodyRegion::Trunk,
        false,
        0.5 * (z0 + z1),
        0.10,
        0.03,
    );
    let kp = dkey("plate_recess", BodyRegion::Trunk, M::Trim);
    k.rear_box(&kp, -0.22, 0.22, z0 - 0.14, z0 - 0.025, 0.004, 0.0);
    let kv = dkey("rear_valance", BodyRegion::RearBumper, M::Trim);
    k.rear_box(
        &kv,
        -wt + 0.10,
        wt - 0.10,
        tz0 + 0.02,
        tz0 + 0.08,
        0.012,
        0.004,
    );
    rear_reflectors(&mut k, wt - 0.18, wt - 0.06, tz0 + 0.10, tz0 + 0.13);
    k.mud_flaps(M::Trim);

    k.skirts(M::Paint, 0.015, 0.05);
    k.mirrors(0.09, 0.10, None);
    k.fender_repeaters();
    follow_faces(
        &mut k,
        &[
            "headlight_",
            "indicator_front_",
            "headlamp_bezel_",
            "taillight_",
            "indicator_rear_",
            "taillamp_bezel_",
        ],
    );
}

// ---------------------------------------------------------------------- talus_20t

/// Late-90s Japanese AWD rally-homologation saloon (`talus_20t`): an upright, boxy late-90s
/// greenhouse with a tall roof, a near-vertical nose with big angular clear head lamps, a
/// bumper that is mostly intercooler mesh with fog lamps in its outer corners, a flat bonnet
/// with twin vents and a centre duct, round blistered arches, a two-plane boot wing on tall
/// stands, tall tails with clear reverse lamps inboard. Dark red paint, bronze six-spokes.
pub fn talus(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        nose_h: Some(0.72),
        nose_rake_deg: 8.0,
        nose_plan_round: 0.14,
        tail_plan_round: 0.12,
        cowl_h: 0.025,
        hood_crown: 0.008,
        hood_bulge_h: 0.0,
        hood_bulge_w: 0.50,
        hood_scoop: HoodScoop::Vents,
        hood_scoop_h: 0.11,
        windshield_rake_deg: Some(55.0),
        a_pillar_rake_deg: 5.0,
        rear_glass_rake_deg: Some(46.0),
        tumblehome: Some(0.11),
        roof_dz: 0.03,
        roof_crown: 0.012,
        roof_len_trim: 0.13,
        belt_dz: 0.03,
        crease_z: 0.42,
        crease_depth: 0.008,
        shoulder_round: 0.03,
        haunch: 0.055,
        front_haunch: 0.055,
        flare: 0.018,
        flare_shape: ArchShape::Round,
        ride_dz: -0.025,
        deck_drop: -0.02,
        ducktail_h: 0.0,
        tail_round: 0.10,
        tail_drop: 0.03,
        a_pillar_w_m: 0.075,
        c_pillar_w_m: 0.13,
        windshield_wrap_m: 0.05,
        fuel_door: FuelDoor::Left,
        sill_tuck: 0.08,
        arch_gap: 0.045,
        ..BodyStyle::preset(Silhouette::Notchback)
    });
    // Pass 3: round blistered arches standing well proud of a pinched waist, tucked sill,
    // fender tops over the arches, short overhangs.
    let (fy, fr) = axle(def, true);
    let (ry, rr) = axle(def, false);
    let belt = 0.59 * def.body_size_m[2];
    let b = &mut s.body;
    b.plan_width = vec![
        [fy + fr, 1.0],
        [fy + fr + 0.7, 0.95],
        [fy - fr - 0.1, 1.0],
        [0.5 * (fy + ry), 0.95],
        [ry + rr + 0.1, 1.0],
        [ry - rr - 0.7, 0.96],
    ];
    b.side_bulge_m = 0.02;
    b.rocker_in_m = 0.045;
    b.side_sculpt = vec![
        blister(fy, fr, 0.22, belt - 0.03, 0.04, 0.15),
        blister(ry, rr, 0.22, belt - 0.03, 0.045, 0.15),
        shoulder_line(belt, 0.07, 0.012),
    ];
    b.front_fender_rise_m = 0.03;
    b.rear_fender_rise_m = 0.025;
    b.overhang_trim = (-0.08, -0.12);
    // No end-face rake: front / rear-view projected lamps land inside a raked face
    // (generator request, see the integration doc); a chin cuts under the signal here.
    b.roof_plan_taper = 0.05;
    s.details.push(kit_detail(talus_kit));
    wheels(
        &mut s,
        def,
        RimKind::SixSpoke,
        6,
        0.68,
        M::Metal,
        CapKind::Centre,
        (0.025, 0.012),
        (0.0, 0.8),
    );
    let m = &mut s.mechanical;
    m.engine = EngineKind::Inline4;
    m.induction = Induction::Turbo;
    m.front_suspension = SuspensionKind::Strut;
    m.rear_suspension = SuspensionKind::Multilink;
    m.caliper_rgb = Some(rgb(0xD4A017));
    m.exhaust = ExhaustKind::Single;
    m.exhaust_tip_r_m = 0.05;
    s.tints.push((M::Paint, rgb(0x7A1418)));
    s.wheel.rim_rgb = Some(rgb(0x8C6A3A));
    s
}

fn talus_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;

    // --- Front: big angular clear lamps (inner lower corner cut), amber outer corners.
    let (lt, lb) = (nz1 - 0.02, nz1 - 0.15);
    for sx in SIDES {
        let xo = wn - 0.06;
        let xs = xo - 0.10;
        // One angular clear lens projected on the nose, amber outer corner wrapping round.
        k.proj_cluster(
            sx,
            true,
            LampView::Front,
            &[[0.275, lt], [xo, lt], [xo, lb], [0.335, lb]],
            Some(Seg::Outboard(0.11)),
            M::EmissiveAmber,
            0.012,
            18.0,
        );
        // Twin reflector bowls behind the clear lens (low / high beam).
        let kb = sided(
            &dkey("headlamp_bowl_l", BodyRegion::FrontLeftFender, M::Chrome),
            sx,
        );
        let zc = 0.5 * (lt + lb) + 0.01;
        for x in [0.37, xs - 0.08] {
            k.round_lamp(&kb, true, sx * x, zc, 0.04, 0.022, None);
        }
    }
    // Narrow grille between the lamps with a blank badge.
    k.grille(
        "grille",
        BodyRegion::Hood,
        0.23,
        lb + 0.02,
        lt - 0.03,
        1,
        M::Trim,
        None,
    );
    blank_badge(
        &mut k,
        BodyRegion::Hood,
        true,
        0.5 * (lb + lt) - 0.005,
        0.10,
        0.04,
    );
    // Bumper: one big intercooler mesh mouth with the fogs in its outer corners.
    let (zm0, zm1) = (nz0 + 0.06, nz0 + 0.30);
    let lm = dkey("lower_mouth", fb, M::Grille);
    face_poly(
        &mut k,
        &lm,
        true,
        &[
            [-0.40, zm0],
            [0.40, zm0],
            [0.46, zm0 + 0.08],
            [0.44, zm1],
            [-0.44, zm1],
            [-0.46, zm0 + 0.08],
        ],
        0.006,
    );
    k.mesh_grille(
        "lower_grille",
        fb,
        0.36,
        zm0 + 0.03,
        zm1 - 0.03,
        4,
        7,
        M::Metal,
    );
    for sx in SIDES {
        let kp = sided(&dkey("fog_pod_l", fb, M::Grille), sx);
        let o = on_side(
            sx,
            &[
                [wn - 0.26, zm0 + 0.02],
                [wn - 0.07, zm0 + 0.02],
                [wn - 0.07, zm0 + 0.14],
                [wn - 0.26, zm0 + 0.14],
            ],
        );
        face_poly(&mut k, &kp, true, &o, 0.008);
    }
    k.fogs(wn - 0.165, zm0 + 0.08, 0.05);
    k.splitter(M::Trim, 0.04, 0.12, 0.02, false);
    // Centre duct on the bonnet's leading edge.
    let (yb0, yb1) = (p.y_bf - 0.30, p.y_bf - 0.10);
    k.top_vent(
        "hood_duct",
        BodyRegion::Hood,
        yb0,
        yb1,
        -0.10,
        0.10,
        0,
        M::Trim,
    );

    // --- Rear: two-plane wing on tall stands, tall tails with clear reverse lamps.
    k.wing(Wing {
        lead: 0.30,
        chord: 0.24,
        height: 0.16,
        half_span: 0.74,
        thick: 0.035,
        aoa_deg: 5.0,
        stand: Stand::Pedestal(0.52),
        endplates: Some((0.20, 0.26)),
        mat: M::Paint,
        stand_mat: M::Paint,
        gurney: 0.0,
    });
    k.wing(Wing {
        lead: 0.22,
        chord: 0.13,
        height: 0.25,
        half_span: 0.74,
        thick: 0.022,
        aoa_deg: 14.0,
        stand: Stand::None,
        endplates: None,
        mat: M::Paint,
        stand_mat: M::Paint,
        gurney: 0.0,
    });
    let (z0, z1) = (tz1 - 0.17, tz1 - 0.025);
    for sx in SIDES {
        k.proj_cluster(
            sx,
            false,
            LampView::Rear,
            &[[0.42, z0], [wt - 0.02, z0], [wt - 0.02, z1], [0.42, z1]],
            Some(Seg::Lower(0.045)),
            M::EmissiveAmber,
            0.012,
            30.0,
        );
        let kr = sided(
            &lkey("reverse_lamp_l", BodyRegion::RearLeftQuarter, M::Headlight),
            sx,
        );
        k.rear_box(&kr, sx * 0.315, sx * 0.41, z0, z1, 0.018, 0.004);
    }
    k.rear_panel(
        "tail_panel",
        BodyRegion::Trunk,
        0.29,
        z0 + 0.01,
        z1 - 0.01,
        M::Trim,
    );
    blank_badge(
        &mut k,
        BodyRegion::Trunk,
        false,
        0.5 * (z0 + z1),
        0.12,
        0.035,
    );
    let kp = dkey("plate_recess", BodyRegion::Trunk, M::Trim);
    k.rear_box(&kp, -0.22, 0.22, tz0 + 0.12, tz0 + 0.24, 0.004, 0.0);
    let kv = dkey("rear_valance", BodyRegion::RearBumper, M::Trim);
    k.rear_box(&kv, -0.42, 0.42, tz0 + 0.02, tz0 + 0.07, 0.010, 0.004);
    rear_reflectors(&mut k, wt - 0.18, wt - 0.06, tz0 + 0.09, tz0 + 0.12);

    k.skirts(M::Trim, 0.015, 0.05);
    k.mirrors(0.08, 0.12, None);
    k.fender_repeaters();
    follow_faces(
        &mut k,
        &[
            "headlight_",
            "indicator_front_",
            "headlamp_bezel_",
            "taillight_",
            "indicator_rear_",
            "taillamp_bezel_",
        ],
    );
}

// ---------------------------------------------------------------------- shiro_turbo

/// Early-2000s Japanese FR coupe in drift trim (`shiro_turbo`): low, wedged notchback with a
/// long cabin and short boot, fast windshield and rear glass, sharp swept lamps running back
/// into the fenders, a wide lower mouth with a lip and canards, vented carbon bonnet,
/// bolt-on box flares, a GT wing on swan necks, chrome-housed clear tails with round red
/// inserts, a vinyl band, deep-dish mesh wheels.
pub fn shiro(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: -0.06,
        nose_h: Some(0.55),
        nose_rake_deg: 32.0,
        nose_plan_round: 0.32,
        tail_plan_round: 0.25,
        hood_crown: 0.015,
        windshield_rake_deg: Some(62.0),
        a_pillar_rake_deg: 10.0,
        rear_glass_rake_deg: Some(63.0),
        roof_arc: 0.35,
        roof_dz: -0.03,
        roof_len_trim: 0.10,
        tumblehome: Some(0.15),
        belt_dz: 0.05,
        belt_rake: 0.03,
        belt_kick: 0.02,
        crease_z: 0.56,
        crease_depth: 0.012,
        shoulder_round: 0.08,
        haunch: 0.045,
        front_haunch: 0.020,
        flare: 0.035,
        flare_shape: ArchShape::Box,
        ride_dz: -0.045,
        stance_pitch_deg: 0.2,
        ducktail_h: 0.02,
        a_pillar_w_m: 0.07,
        c_pillar_w_m: 0.16,
        b_pillar_w_m: 0.08,
        windshield_wrap_m: 0.08,
        rear_glass_wrap_m: 0.08,
        fuel_door: FuelDoor::Right,
        sill_tuck: 0.09,
        arch_gap: 0.045,
        ..BodyStyle::preset(Silhouette::Notchback)
    });
    // Pass 3: coupe plan (narrow nose, waist, tapered tail), rounded convex side, rear
    // haunch rising over the arch, long nose / short boot.
    let (fy, fr) = axle(def, true);
    let (ry, rr) = axle(def, false);
    let belt = 0.59 * def.body_size_m[2];
    let b = &mut s.body;
    b.plan_width = vec![
        [fy + fr, 1.0],
        [fy + fr + 0.75, 0.89],
        [0.5 * (fy + ry), 0.96],
        [ry, 1.0],
        [ry - rr - 0.75, 0.92],
    ];
    b.side_bulge_m = 0.03;
    b.side_bulge_z = 0.6;
    b.rocker_in_m = 0.045;
    b.side_sculpt = vec![
        shoulder_line(belt, 0.08, 0.015),
        blister(ry, rr, 0.3, belt, 0.03, 0.18),
    ];
    b.rear_fender_rise_m = 0.035;
    b.overhang_trim = (0.02, -0.16);
    b.nose_face_rake_deg = 30.0;
    b.nose_chin_m = 0.06;
    b.nose_plan_bow_m = 0.25;
    b.nose_tip_w = 0.7;
    b.tail_face_rake_deg = 15.0;
    b.roof_plan_taper = 0.12;
    b.lower_lean_in_m = 0.03;
    s.details.push(kit_detail(shiro_kit));
    wheels(
        &mut s,
        def,
        RimKind::Mesh,
        10,
        0.74,
        M::Metal,
        CapKind::Small,
        (0.055, 0.035),
        (1.0, 3.0),
    );
    let m = &mut s.mechanical;
    m.engine = EngineKind::Inline4;
    m.induction = Induction::Turbo;
    m.show_engine_detail = true;
    m.front_suspension = SuspensionKind::Coilover;
    m.rear_suspension = SuspensionKind::Coilover;
    m.front_brakes.caliper = CaliperKind::Fixed4;
    m.caliper_rgb = Some(rgb(0xD4A017));
    m.spring_rgb = Some(rgb(0xE0301E));
    m.exhaust = ExhaustKind::Single;
    m.exhaust_tip_r_m = 0.06;
    s.tints.push((M::Paint, rgb(0xEEEDE6)));
    // Vinyl band in the accent slot (gold, matching the calipers).
    s.accent_rgb = Some(rgb(0xD4A017));
    // Cyan underglow, lit with the headlights (research §1.3 tuner neon).
    s.underglow_rgb = Some(rgb(0x18C8E8));
    s
}

fn shiro_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (_, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;

    // --- Front: sharp swept lamps, slim slot, wide lower mouth, lip + fences + canards.
    for sx in SIDES {
        swept_top_lamp(
            &mut k,
            sx,
            yn - 0.16,
            yn - 0.03,
            0.42,
            0.05,
            0.30,
            0.02,
            0.07,
        );
    }
    k.grille(
        "grille",
        BodyRegion::Hood,
        0.22,
        nz1 - 0.06,
        nz1 - 0.025,
        0,
        M::Trim,
        None,
    );
    let (zm0, zm1) = (nz0 + 0.05, nz0 + 0.23);
    let q = ((wn - 0.08) / 0.56).min(1.0);
    let lm = dkey("lower_mouth", fb, M::Grille);
    face_poly(
        &mut k,
        &lm,
        true,
        &[
            [-0.48 * q, zm0],
            [0.48 * q, zm0],
            [0.56 * q, zm0 + 0.10],
            [0.50 * q, zm1],
            [-0.50 * q, zm1],
            [-0.56 * q, zm0 + 0.10],
        ],
        0.006,
    );
    k.mesh_grille(
        "lower_mouth_mesh",
        fb,
        0.44 * q,
        zm0 + 0.02,
        zm1 - 0.02,
        3,
        9,
        M::Trim,
    );
    k.splitter(M::Carbon, 0.09, 0.04, 0.024, true);
    k.canards(M::Carbon, zm0 + 0.05);
    let kt = dkey("tow_hook_front", fb, M::Caliper);
    k.front_box(&kt, 0.40, 0.46, zm0 - 0.02, zm0 + 0.03, 0.07, 0.004);

    // --- Carbon bonnet with two vents.
    let kc = dkey("carbon_hood", BodyRegion::Hood, M::Carbon);
    let hw = p.half_width(p.y_bf) - 0.10;
    k.patch(
        &kc,
        p.y_fw + 0.03,
        p.y_bf - 0.02,
        top_band(-hw, hw),
        0.004,
        0.0,
        (0, 6),
    );
    let span = p.y_bf - p.y_fw;
    for sx in SIDES {
        let name = if sx < 0.0 {
            "hood_vent_l"
        } else {
            "hood_vent_r"
        };
        k.top_vent(
            name,
            BodyRegion::Hood,
            p.y_fw + 0.45 * span,
            p.y_fw + 0.70 * span,
            (sx * 0.20).min(sx * 0.42),
            (sx * 0.20).max(sx * 0.42),
            4,
            M::Carbon,
        );
    }

    // --- Rear: GT wing on swan necks, clear tails with round red inserts, diffuser.
    k.wing(Wing {
        lead: 0.46,
        chord: 0.24,
        height: 0.20,
        half_span: 0.72,
        thick: 0.03,
        aoa_deg: 9.0,
        stand: Stand::SwanNeck(0.42),
        endplates: Some((0.14, 0.27)),
        mat: M::Carbon,
        stand_mat: M::Trim,
        gurney: 0.010,
    });
    for sx in SIDES {
        let (z0, z1) = (tz1 - 0.13, tz1 - 0.025);
        let (xa, xb) = (sx * 0.26, sx * (wt - 0.015));
        let kh = sided(
            &dkey(
                "taillight_housing_l",
                BodyRegion::RearLeftQuarter,
                M::Chrome,
            ),
            sx,
        );
        // Lens tapers down toward the plate (inboard end lower).
        let o = [[xa, z0 + 0.03], [xb, z0], [xb, z1], [xa, z1 - 0.01]];
        face_poly(&mut k, &kh, false, &o, 0.014);
        let kg = sided(
            &dkey("taillight_lens_l", BodyRegion::RearLeftQuarter, M::Glass),
            sx,
        );
        let e = sx * 0.012;
        let o = [
            [xa + e, z0 + 0.04],
            [xb - e, z0 + 0.012],
            [xb - e, z1 - 0.01],
            [xa + e, z1 - 0.02],
        ];
        face_poly(&mut k, &kg, false, &o, 0.022);
        let tk = taillight_key(sx);
        let r = 0.5 * (z1 - z0) - 0.022;
        let zc = 0.5 * (z0 + z1);
        k.round_lamp(&tk, false, sx * (wt - 0.09), zc, r, 0.034, None);
        // The inner round element of the cluster is the amber turn signal.
        let ka = indicator_rear_key(sx, M::EmissiveAmber);
        k.round_lamp(
            &ka,
            false,
            sx * (wt - 0.09 - 2.3 * r),
            zc + 0.004,
            r,
            0.034,
            None,
        );
    }
    blank_badge(&mut k, BodyRegion::Trunk, false, tz1 - 0.06, 0.10, 0.03);
    let kp = dkey("plate_recess", BodyRegion::Trunk, M::Trim);
    k.rear_box(&kp, -0.22, 0.22, tz1 - 0.24, tz1 - 0.13, 0.004, 0.0);
    k.diffuser(M::Carbon, 0.50, 0.12, 3, 0.04);

    // --- Body kit sides, vinyl band.
    k.skirts(M::Paint, 0.035, 0.07);
    let (yf, _, rf) = k.arch(true);
    let (yr, _, rr) = k.arch(false);
    let zv = 0.30 * p.roof.2;
    for sx in SIDES {
        let kv = dkey("vinyl_band_l", BodyRegion::LeftDoorF, M::Trim);
        k.side_patch(
            &kv,
            sx,
            yr + rr + 0.05,
            yf - rf - 0.08,
            zv + 0.10,
            zv + 0.17,
            0.004,
            0.0,
            (10, 1),
        );
        let ka = dkey("vinyl_slash_l", BodyRegion::LeftDoorF, M::Accent);
        k.side_patch(
            &ka,
            sx,
            yr + rr + 0.05,
            yf - rf - 0.40,
            zv + 0.18,
            zv + 0.21,
            0.004,
            0.0,
            (8, 1),
        );
    }
    k.mirrors(0.10, 0.10, Some(M::Carbon));
    k.fender_repeaters();
    follow_faces(&mut k, &["headlight", "indicator_front", "headlamp_bezel"]);
}

// ---------------------------------------------------------------------- hakone_gt

/// 90s Japanese twin-turbo FR grand tourer liftback (`hakone_gt`): long low nose, cabin set
/// well back under a bubble roof that narrows into a teardrop canopy, a big glass hatch
/// running down to a high tail, soft rounded flanks and fat rear haunches. Triple projector
/// lamps under long clear covers, a big rounded central mouth with a body-colour bar, fender
/// vents, a hoop wing over the hatch, wide two-tier tails in a body-wide smoked panel.
pub fn hakone(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: -0.14,
        nose_h: Some(0.56),
        nose_rake_deg: 40.0,
        nose_plan_round: 0.42,
        tail_plan_round: 0.32,
        hood_crown: 0.025,
        hood_bulge_h: 0.012,
        hood_bulge_w: 0.55,
        windshield_rake_deg: Some(61.0),
        a_pillar_rake_deg: 12.0,
        roof_arc: 0.55,
        c_pillar_rake_deg: 8.0,
        tumblehome: Some(0.24),
        belt_dz: 0.035,
        belt_kick: 0.02,
        crease_z: 0.50,
        crease_depth: 0.0,
        shoulder_round: 0.12,
        haunch: 0.065,
        front_haunch: 0.045,
        flare: 0.012,
        ride_dz: -0.02,
        rear_glass_bow: 0.06,
        tail_round: 0.28,
        ducktail_h: 0.02,
        a_pillar_w_m: 0.07,
        c_pillar_w_m: 0.14,
        b_pillar_w_m: 0.08,
        windshield_wrap_m: 0.10,
        rear_glass_wrap_m: 0.10,
        end_round_m: 0.04,
        fuel_door: FuelDoor::Left,
        roof_dz: -0.02,
        sill_tuck: 0.10,
        arch_gap: 0.045,
        ..BodyStyle::preset(Silhouette::Fastback)
    });
    // Pass 3: GT plan (pointed nose, wide hips, tapered tail), soft round section with a
    // big tumblehome, fat rear haunches rising above the hatch line, long nose.
    let (fy, fr) = axle(def, true);
    let (ry, rr) = axle(def, false);
    let belt = 0.59 * def.body_size_m[2];
    let b = &mut s.body;
    b.plan_width = vec![
        [fy + fr, 1.0],
        [fy + fr + 0.75, 0.86],
        [0.5 * (fy + ry), 0.95],
        [ry, 1.0],
        [ry - rr - 0.8, 0.90],
    ];
    b.side_bulge_m = 0.04;
    b.side_bulge_z = 0.55;
    b.rocker_in_m = 0.05;
    b.tumblehome_m = Some(0.26);
    b.side_sculpt = vec![
        blister(ry, rr, 0.25, belt + 0.02, 0.04, 0.22),
        blister(fy, fr, 0.25, belt, 0.02, 0.18),
    ];
    b.rear_fender_rise_m = 0.05;
    b.fender_peak_w = 0.78;
    b.overhang_trim = (0.05, -0.12);
    b.nose_face_rake_deg = 35.0;
    b.nose_chin_m = 0.05;
    b.nose_plan_bow_m = 0.30;
    b.nose_tip_w = 0.7;
    b.tail_face_rake_deg = 20.0;
    b.roof_plan_taper = 0.15;
    b.lower_lean_in_m = 0.04;
    s.details.push(kit_detail(hakone_kit));
    wheels(
        &mut s,
        def,
        RimKind::FiveSpoke,
        5,
        0.70,
        M::Metal,
        CapKind::Centre,
        (0.03, 0.015),
        (0.0, 1.0),
    );
    let m = &mut s.mechanical;
    m.engine = EngineKind::Inline6;
    m.induction = Induction::Turbo;
    m.front_suspension = SuspensionKind::DoubleWishbone;
    m.rear_suspension = SuspensionKind::DoubleWishbone;
    m.front_brakes.caliper = CaliperKind::Fixed4;
    m.rear_brakes.caliper = CaliperKind::Fixed4;
    m.caliper_rgb = Some(rgb(0xC81E1E));
    m.exhaust = ExhaustKind::Dual;
    m.exhaust_tip_r_m = 0.045;
    s.tints.push((M::Paint, rgb(0x3B2350)));
    s
}

fn hakone_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;

    // --- Front: long clear covers over triple projectors, sweeping back into the fenders.
    for sx in SIDES {
        swept_top_lamp(
            &mut k,
            sx,
            yn - 0.27,
            yn - 0.10,
            0.38,
            0.08,
            0.24,
            0.022,
            0.07,
        );
        let kp = sided(
            &lkey(
                "headlamp_projector_l",
                BodyRegion::FrontLeftFender,
                M::Headlight,
            ),
            sx,
        );
        let kr = sided(
            &dkey(
                "headlamp_projector_ring_l",
                BodyRegion::FrontLeftFender,
                M::Chrome,
            ),
            sx,
        );
        for i in 0..3 {
            let x = sx * (0.50 + 0.10 * i as f32).min(wn - 0.10);
            k.round_lamp(&kp, true, x, nz1 - 0.05, 0.028, 0.024, Some(&kr));
        }
    }
    // Big rounded central mouth (octagon) with a body-colour bar and a mesh behind.
    let (zm0, zm1) = (nz0 + 0.05, nz0 + 0.28);
    let (h, c) = ((wn - 0.14).min(0.42), 0.07);
    let lm = dkey("lower_mouth", fb, M::Grille);
    face_poly(
        &mut k,
        &lm,
        true,
        &[
            [-h + c, zm0],
            [h - c, zm0],
            [h, zm0 + c],
            [h, zm1 - c],
            [h - c, zm1],
            [-h + c, zm1],
            [-h, zm1 - c],
            [-h, zm0 + c],
        ],
        0.006,
    );
    k.mesh_grille(
        "lower_mouth_mesh",
        fb,
        h - 0.04,
        zm0 + 0.03,
        zm1 - 0.03,
        3,
        8,
        M::Trim,
    );
    let kb = dkey("lower_mouth_bar", fb, M::Paint);
    let zb = 0.5 * (zm0 + zm1) + 0.02;
    k.front_box(
        &kb,
        -h + 0.01,
        h - 0.01,
        zb - 0.018,
        zb + 0.018,
        0.016,
        0.008,
    );
    blank_badge(&mut k, fb, true, zb, 0.10, 0.03);
    for sx in SIDES {
        let ki = sided(&dkey("brake_duct_l", fb, M::Grille), sx);
        let o = on_side(
            sx,
            &[
                [wn - 0.25, zm0 + 0.02],
                [wn - 0.08, zm0 + 0.02],
                [wn - 0.08, zm0 + 0.08],
                [wn - 0.25, zm0 + 0.11],
            ],
        );
        face_poly(&mut k, &ki, true, &o, 0.006);
    }
    k.splitter(M::Trim, 0.03, 0.14, 0.02, false);

    // --- Fender vents behind the front wheels.
    let (yf, zf, rf) = k.arch(true);
    for sx in SIDES {
        let kv = dkey("fender_vent_l", BodyRegion::FrontLeftFender, M::Grille);
        let (y0, y1) = (yf - rf - 0.18, yf - rf - 0.04);
        let z0 = zf + 0.10;
        k.side_patch(&kv, sx, y0, y1, z0, z0 + 0.09, 0.002, 0.008, (2, 1));
        let ks = dkey("fender_vent_slats_l", BodyRegion::FrontLeftFender, M::Paint);
        for dz in [0.03, 0.06] {
            k.side_patch(
                &ks,
                sx,
                y0,
                y1,
                z0 + dz - 0.006,
                z0 + dz + 0.006,
                0.004,
                0.012,
                (2, 1),
            );
        }
    }

    // --- Rear: hoop wing, wide two-tier tails in a body-wide smoked panel, centre garnish.
    k.hoop_wing(0.42, 0.20, 0.12, M::Paint);
    let (pz0, pz1) = (tz1 - 0.20, tz1 - 0.025);
    k.rear_panel(
        "tail_panel",
        BodyRegion::Trunk,
        wt - 0.07,
        pz0,
        pz1,
        M::Grille,
    );
    let zc = 0.5 * (pz0 + pz1);
    for sx in SIDES {
        // One wide lens per side with rounded outer corners: red outboard, amber inboard.
        let (za, zb) = (pz0 + 0.025, pz1 - 0.02);
        let (xi, xo) = (wt - 0.44, wt - 0.05);
        let xs = xi + 0.11;
        let c = 0.04;
        let o = on_side(
            sx,
            &[
                [xs + 0.008, za],
                [xo - c, za],
                [xo, za + c],
                [xo, zb - c],
                [xo - c, zb],
                [xs + 0.008, zb],
            ],
        );
        face_poly(&mut k, &taillight_key(sx), false, &o, 0.024);
        let o = on_side(sx, &[[xi, za + 0.01], [xs, za], [xs, zb], [xi, zb - 0.01]]);
        face_poly(
            &mut k,
            &indicator_rear_key(sx, M::EmissiveAmber),
            false,
            &o,
            0.024,
        );
        // A dark horizontal bar across the red lens (two-tier tail).
        let kr = sided(
            &dkey("taillight_bar_l", BodyRegion::RearLeftQuarter, M::Trim),
            sx,
        );
        k.rear_box(
            &kr,
            sx * (xs + 0.02),
            sx * (xo - 0.02),
            zc - 0.005,
            zc + 0.005,
            0.030,
            0.0,
        );
    }
    let kg = dkey("tail_garnish", BodyRegion::Trunk, M::Chrome);
    k.rear_box(&kg, -0.22, 0.22, zc - 0.012, zc + 0.012, 0.016, 0.004);
    let kp = dkey("plate_recess", BodyRegion::RearBumper, M::Trim);
    k.rear_box(&kp, -0.22, 0.22, pz0 - 0.15, pz0 - 0.035, 0.004, 0.0);
    let kv = dkey("rear_valance", BodyRegion::RearBumper, M::Trim);
    k.rear_box(&kv, -0.45, 0.45, tz0 + 0.02, tz0 + 0.08, 0.012, 0.004);
    rear_reflectors(&mut k, wt - 0.22, wt - 0.08, tz0 + 0.10, tz0 + 0.13);

    k.mirrors(0.12, 0.11, None);
    k.fender_repeaters();
    follow_faces(&mut k, &["headlight", "indicator_front", "headlamp_bezel"]);
}

// ---------------------------------------------------------------------- sperber_32

/// 2000s German FR sport sedan with a high-revving six (`sperber_32`): long bonnet with a
/// power dome and the cabin set well back (long dash-to-axle), a crisp notchback with a
/// short boot and an integrated boot-lid lip, a sharp shoulder line, wide C-pillars. Split
/// full-width grille with a body-colour bar, twin round lamps under one cover, three-part
/// front intake, fender gills, horizontal tails wrapping onto the quarters, carbon roof,
/// diffuser, quad tips.
pub fn sperber(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: -0.16,
        nose_h: Some(0.66),
        nose_rake_deg: 14.0,
        nose_plan_round: 0.22,
        tail_plan_round: 0.22,
        cowl_h: 0.04,
        hood_crown: 0.012,
        power_bulge_h: 0.03,
        hood_bulge_w: 0.32,
        windshield_rake_deg: Some(59.0),
        a_pillar_rake_deg: 9.0,
        rear_glass_rake_deg: Some(54.0),
        roof_arc: 0.25,
        roof_dz: -0.015,
        roof_len_trim: 0.15,
        tumblehome: Some(0.16),
        belt_dz: 0.035,
        belt_kick: 0.025,
        crease_z: 0.64,
        crease_depth: 0.014,
        shoulder_round: 0.045,
        haunch: 0.035,
        front_haunch: 0.020,
        flare: 0.015,
        ride_dz: -0.02,
        ducktail_h: 0.022,
        c_pillar_w_m: 0.18,
        windshield_wrap_m: 0.06,
        door_handles: TrimFinish::Chrome,
        fuel_door: FuelDoor::Right,
        sill_tuck: 0.08,
        arch_gap: 0.045,
        ..BodyStyle::preset(Silhouette::Notchback)
    });
    // Pass 3: crisp German sedan section (sharp shoulder line, a lower character line, convex
    // side), short front overhang / long dash-to-axle, gently tapered ends.
    let (fy, fr) = axle(def, true);
    let (ry, rr) = axle(def, false);
    let belt = 0.59 * def.body_size_m[2];
    let b = &mut s.body;
    b.plan_width = vec![
        [fy + fr, 1.0],
        [fy + fr + 0.65, 0.94],
        [ry - rr, 1.0],
        [ry - rr - 0.8, 0.95],
    ];
    b.side_bulge_m = 0.02;
    b.side_bulge_z = 0.5;
    b.rocker_in_m = 0.035;
    b.side_sculpt = vec![
        shoulder_line(belt, 0.07, 0.022),
        SideSculpt {
            y0_m: ry + rr + 0.1,
            y1_m: fy - fr - 0.1,
            z0_m: 0.30,
            z1_m: 0.38,
            dx_m: -0.02,
            falloff_m: 0.06,
        },
        blister(ry, rr, 0.3, belt - 0.05, 0.02, 0.15),
    ];
    b.front_fender_rise_m = 0.02;
    b.overhang_trim = (-0.14, -0.06);
    b.nose_face_rake_deg = 12.0;
    b.nose_chin_m = 0.05;
    b.nose_plan_bow_m = 0.15;
    b.nose_tip_w = 0.8;
    b.tail_face_rake_deg = 8.0;
    b.roof_plan_taper = 0.06;
    s.details.push(kit_detail(sperber_kit));
    wheels(
        &mut s,
        def,
        RimKind::MultiSpoke,
        10,
        0.76,
        M::Metal,
        CapKind::Centre,
        (0.03, 0.015),
        (0.0, 0.5),
    );
    let m = &mut s.mechanical;
    m.engine = EngineKind::Inline6;
    m.induction = Induction::Stacks;
    m.front_suspension = SuspensionKind::Strut;
    m.rear_suspension = SuspensionKind::Multilink;
    m.caliper_rgb = Some(rgb(0x7A8088));
    m.exhaust = ExhaustKind::Quad;
    m.exhaust_tip_r_m = 0.042;
    s.tints.push((M::Paint, rgb(0x24407A)));
    s
}

fn sperber_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yt = k.yt;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (_, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;

    // --- Front: one full-width grille between the lamps (rounded corners, dark mesh) split
    // by a body-colour horizontal bar.
    let (gz0, gz1) = (nz1 - 0.13, nz1 - 0.025);
    let rounded = |x: f32, z0: f32, z1: f32, c: f32| {
        [
            [-x, z0 + c],
            [-x + c, z0],
            [x - c, z0],
            [x, z0 + c],
            [x, z1 - c],
            [x - c, z1],
            [-x + c, z1],
            [-x, z1 - c],
        ]
    };
    let gf = dkey("grille_frame", BodyRegion::Hood, M::Trim);
    face_poly(&mut k, &gf, true, &rounded(0.25, gz0, gz1, 0.03), 0.010);
    let g = dkey("grille", BodyRegion::Hood, M::Grille);
    face_poly(
        &mut k,
        &g,
        true,
        &rounded(0.235, gz0 + 0.012, gz1 - 0.012, 0.024),
        0.014,
    );
    k.mesh_grille(
        "grille_mesh",
        BodyRegion::Hood,
        0.22,
        gz0 + 0.02,
        gz1 - 0.02,
        2,
        8,
        M::Trim,
    );
    let gb = dkey("grille_bar", BodyRegion::Hood, M::Paint);
    let zg = 0.5 * (gz0 + gz1);
    k.front_box(&gb, -0.245, 0.245, zg - 0.012, zg + 0.012, 0.022, 0.006);
    for sx in SIDES {
        // Twin round lamps under one clear cover.
        let kh = sided(
            &dkey("headlamp_housing_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        let xo = wn - 0.03;
        let o = on_side(
            sx,
            &[
                [0.27, nz1 - 0.015],
                [xo, nz1 - 0.015],
                [xo, nz1 - 0.09],
                [xo - 0.05, nz1 - 0.13],
                [0.27, nz1 - 0.13],
            ],
        );
        face_poly(&mut k, &kh, true, &o, 0.010);
        let hk = headlight_key(sx);
        let r = 0.040;
        let zl = nz1 - 0.07;
        let xs = [0.38, (wn - 0.12).max(0.49)];
        for &x in &xs {
            k.round_lamp(&hk, true, sx * x, zl, r, 0.024, None);
        }
        // Amber turn-signal strip in the inboard end of the housing.
        let ka = indicator_front_key(sx);
        k.front_box(
            &ka,
            sx * 0.28,
            sx * (xs[0] - r - 0.016),
            nz1 - 0.12,
            nz1 - 0.025,
            0.014,
            0.004,
        );
        // Outer intakes of the three-part bumper.
        let kd = sided(&dkey("side_duct_l", fb, M::Grille), sx);
        let o = on_side(
            sx,
            &[
                [wn - 0.30, nz0 + 0.07],
                [wn - 0.07, nz0 + 0.07],
                [wn - 0.07, nz0 + 0.17],
                [wn - 0.26, nz0 + 0.21],
            ],
        );
        face_poly(&mut k, &kd, true, &o, 0.006);
    }
    let lm = dkey("lower_intake", fb, M::Grille);
    let (zm0, zm1) = (nz0 + 0.06, nz0 + 0.22);
    face_poly(
        &mut k,
        &lm,
        true,
        &[[-0.40, zm0], [0.40, zm0], [0.36, zm1], [-0.36, zm1]],
        0.006,
    );
    k.mesh_grille(
        "lower_intake_mesh",
        fb,
        0.34,
        zm0 + 0.02,
        zm1 - 0.02,
        2,
        6,
        M::Trim,
    );
    k.splitter(M::Trim, 0.035, 0.14, 0.02, false);

    // --- Fender gills behind the front wheels.
    let (yf, zf, rf) = k.arch(true);
    for sx in SIDES {
        let (y0, y1) = (yf - rf - 0.20, yf - rf - 0.05);
        let z0 = zf + 0.14;
        let kv = dkey("fender_gill_l", BodyRegion::FrontLeftFender, M::Grille);
        k.side_patch(&kv, sx, y0, y1, z0, z0 + 0.07, 0.002, 0.008, (2, 1));
        let ks = dkey("fender_gill_slats_l", BodyRegion::FrontLeftFender, M::Paint);
        for dz in [0.023, 0.047] {
            k.side_patch(
                &ks,
                sx,
                y0,
                y1,
                z0 + dz - 0.005,
                z0 + dz + 0.005,
                0.004,
                0.012,
                (2, 1),
            );
        }
    }

    // --- Rear: horizontal tails wrapping onto the quarters, diffuser, carbon roof.
    for sx in SIDES {
        let tk = taillight_key(sx);
        let kh = sided(
            &dkey("taillight_housing_l", BodyRegion::RearLeftQuarter, M::Trim),
            sx,
        );
        let (z0, z1) = (tz1 - 0.15, tz1 - 0.03);
        k.rear_box(&kh, sx * 0.34, sx * (wt + 0.005), z0, z1, 0.012, 0.006);
        // One horizontal lens over the housing (amber strip along its bottom), tapering down
        // toward the plate.
        let (lz0, lz1) = (z0 + 0.012, z1 - 0.012);
        let o = on_side(
            sx,
            &[
                [0.36, lz0 + 0.03],
                [wt - 0.01, lz0 + 0.025],
                [wt - 0.01, lz1],
                [0.36, lz1],
            ],
        );
        face_poly(&mut k, &tk, false, &o, 0.026);
        let ka = indicator_rear_key(sx, M::EmissiveAmber);
        let o = on_side(
            sx,
            &[
                [0.53, lz0],
                [wt - 0.03, lz0],
                [wt - 0.03, lz0 + 0.018],
                [0.53, lz0 + 0.022],
            ],
        );
        face_poly(&mut k, &ka, false, &o, 0.030);
        let kw = PartKey {
            kind: PartKind::Light,
            ..dkey(
                "taillight_wrap_l",
                BodyRegion::RearLeftQuarter,
                M::Taillight,
            )
        };
        k.side_patch(
            &kw,
            sx,
            yt + 0.01,
            yt + 0.14,
            z0 + 0.04,
            z1 - 0.014,
            0.002,
            0.012,
            (2, 1),
        );
        let kr = sided(
            &lkey("reverse_lamp_l", BodyRegion::RearLeftQuarter, M::Headlight),
            sx,
        );
        k.rear_box(
            &kr,
            sx * 0.40,
            sx * 0.52,
            z0 + 0.02,
            z1 - 0.065,
            0.018,
            0.004,
        );
    }
    blank_badge(&mut k, BodyRegion::Trunk, false, tz1 - 0.035, 0.06, 0.05);
    let kp = dkey("plate_recess", BodyRegion::Trunk, M::Trim);
    k.rear_box(&kp, -0.24, 0.24, tz1 - 0.30, tz1 - 0.18, 0.004, 0.0);
    k.diffuser(M::Trim, 0.55, 0.13, 4, 0.04);
    let (r0, r1, _, _) = p.roof;
    let hw = k.roof_edge(0.5 * (r0 + r1)) - 0.01;
    k.top_stripe(
        "carbon_roof",
        BodyRegion::Roof,
        r0 + 0.02,
        r1 - 0.02,
        -hw,
        hw,
        M::Carbon,
    );

    k.skirts(M::Paint, 0.02, 0.05);
    k.mirrors(0.10, 0.11, Some(M::Carbon));
    k.mirror_repeaters(0.10, 0.11, true);
    follow_faces(&mut k, &[]);
}

// ---------------------------------------------------------------------- debug_coupe

/// Sport coupe (the old worked example): fastback with splitter, slim LED lamps with DRL,
/// hood vents, ducktail, full-width rear light bar with thick ends, diffuser.
pub fn debug_coupe(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: -0.10,
        nose_h: Some(0.64),
        nose_rake_deg: 25.0,
        nose_plan_round: 0.30,
        tail_plan_round: 0.22,
        hood_crown: 0.020,
        hood_bulge_h: 0.015,
        hood_bulge_w: 0.40,
        windshield_rake_deg: Some(60.0),
        a_pillar_rake_deg: 10.0,
        roof_len_trim: 0.05,
        roof_dz: -0.02,
        tumblehome: Some(0.18),
        crease_z: 0.60,
        crease_depth: 0.012,
        haunch: 0.035,
        flare: 0.020,
        ride_dz: -0.01,
        ducktail_h: 0.035,
        belt_kick: 0.03,
        ..BodyStyle::preset(Silhouette::Fastback)
    });
    s.details.push(kit_detail(debug_coupe_kit));
    wheels(
        &mut s,
        def,
        RimKind::SplitSpoke,
        5,
        0.74,
        M::Engine,
        CapKind::Centre,
        (0.035, 0.015),
        (0.0, 0.5),
    );
    let m = &mut s.mechanical;
    m.engine = EngineKind::Inline6;
    m.induction = Induction::Turbo;
    m.layout = EngineLayout::Front;
    m.front_brakes.caliper = CaliperKind::Fixed4;
    m.caliper_rgb = Some(rgb(0xC81E1E));
    m.exhaust = ExhaustKind::Quad;
    m.exhaust_tip_r_m = 0.045;
    s.tints.push((M::Paint, rgb(0x3A3F45)));
    s
}

fn debug_coupe_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;

    for sx in SIDES {
        // LED lens with an amber inboard segment.
        k.top_lamp_split(
            sx,
            yn - 0.17,
            yn - 0.03,
            0.48,
            wn - 0.07,
            0.022,
            Seg::Inboard(0.08),
        );
        let drl = sided(
            &lkey("drl_l", BodyRegion::FrontLeftFender, M::Headlight),
            sx,
        );
        k.front_box(
            &drl,
            sx * 0.46,
            sx * (wn - 0.06),
            nz1 - 0.06,
            nz1 - 0.044,
            0.016,
            0.0,
        );
        let kh = sided(
            &dkey("headlamp_housing_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        k.front_box(
            &kh,
            sx * 0.44,
            sx * (wn - 0.04),
            nz1 - 0.08,
            nz1 - 0.02,
            0.008,
            0.0,
        );
    }
    k.grille(
        "grille",
        BodyRegion::Hood,
        0.30,
        nz1 - 0.08,
        nz1 - 0.03,
        1,
        M::Trim,
        None,
    );
    k.mesh_grille(
        "lower_mouth",
        fb,
        0.55,
        nz0 + 0.06,
        nz0 + 0.24,
        3,
        8,
        M::Trim,
    );
    k.splitter(M::Carbon, 0.07, 0.10, 0.02, false);
    let span = yn - p.y_fw;
    for sx in SIDES {
        let name = if sx < 0.0 {
            "hood_vent_l"
        } else {
            "hood_vent_r"
        };
        let (a, c) = ((sx * 0.22).min(sx * 0.38), (sx * 0.22).max(sx * 0.38));
        k.top_vent(
            name,
            BodyRegion::Hood,
            yn - 0.50 * span,
            yn - 0.28 * span,
            a,
            c,
            4,
            M::Trim,
        );
    }

    // Full-width light bar with thicker ends.
    let z = tz1 - 0.06;
    let kb = lkey("taillight_bar", BodyRegion::Trunk, M::Taillight);
    k.rear_box(&kb, -wt + 0.30, wt - 0.30, z - 0.016, z + 0.016, 0.022, 0.0);
    for sx in SIDES {
        // Thick bar end with an amber inboard section over the bar's end.
        k.rear_cluster(
            sx,
            wt - 0.32,
            wt - 0.02,
            z - 0.045,
            z + 0.035,
            0.026,
            0.008,
            Seg::Inboard(0.08),
            M::EmissiveAmber,
        );
        let kh = sided(
            &dkey("taillight_housing_l", BodyRegion::RearLeftQuarter, M::Trim),
            sx,
        );
        k.rear_box(
            &kh,
            sx * (wt - 0.34),
            sx * (wt - 0.0),
            z - 0.06,
            z + 0.05,
            0.012,
            0.006,
        );
    }
    k.diffuser(M::Carbon, 0.55, 0.14, 3, 0.05);
    k.skirts(M::Carbon, 0.03, 0.05);
    k.mirrors(0.10, 0.11, Some(M::Carbon));
    k.mirror_repeaters(0.10, 0.11, true);
    k.door_handles(M::Trim);
    k.fuel_filler(1.0, 0.10, M::Paint);
    let _ = tz0;
}
