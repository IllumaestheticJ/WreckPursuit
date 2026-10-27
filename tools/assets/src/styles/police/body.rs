//! Police body kits (car-shapes phase 2): each car's own grille, lamp shapes, bumpers, vents
//! and trim, so the five units read as five different real-world kinds of car before the
//! police equipment ([`super::kit`]) goes on.
//!
//! Everything is built with the shared performance body-kit helpers
//! ([`crate::styles::performance::kit::Kit`], read-only use) and placed from the styled
//! [`crate::profile::BodyProfile`]. [`Kit::new`] drops the foundation's default lamps and
//! mirrors; each kit re-emits head / tail lamps with their turn signals cut out of the same
//! lens, and its own mirrors.
//!
//! References are body types only (CLAUDE.md rule 9, brand cues included): the fleet wears
//! one invented maker identity, a hexagonal shield grille ([`shield_grille`]) and a blank
//! shield badge ([`badge`]), in a different proportion and fill on each car.
//! - `sentinel_cruiser`: 2010s full-size RWD police sedan: long hood, chopped greenhouse, wide
//!   low shield grille, swept slit lamps, boomerang tail lamps.
//! - `sentinel_wolfhound`: 2010s AWD police sedan on a large FWD-platform saloon: tall body,
//!   high belt, tall shield grille with fine vertical bars, swept pentagon lamps, corner lamps.
//! - `sentinel_bastion`: 2010s full-size body-on-frame SUV: upright nose, big egg-crate shield,
//!   stacked lamps, vertical tail lamps, running boards, hitch.
//! - `sentinel_warden`: late-80s full-size body-on-frame sedan: slab sides, period upright
//!   chrome grille with quad rectangular sealed beams, amber wraparound corners, chrome bumpers
//!   with rubber strips, wide ribbed tail lamps.
//! - `sentinel_vigil`: 2010s muscle coupe: power-dome hood with extractor vents, low greenhouse,
//!   slim lamps over one big shield-shaped mouth, swept tail lamps in a dark band.

// Kit helpers take explicit coordinates, like the performance kits.
#![allow(clippy::too_many_arguments)]

use wp_contracts::BodyRegion;

use crate::builder::{Builder, PartKey};
use crate::mesh::{MaterialSlot as M, INDICATOR_SEAM_M};
use crate::style::DetailCtx;
use crate::styles::performance::kit::{
    dkey, headlight_key, indicator_front_key, indicator_rear_key, lkey, sided, taillight_key,
    top_band, Kit, Seg, SIDES,
};

const FB: BodyRegion = BodyRegion::FrontBumper;
const RB: BodyRegion = BodyRegion::RearBumper;

/// Name with the side token for `sx` (`base_l` / `base_r`).
fn side_name(base: &str, sx: f32) -> String {
    format!("{base}_{}", if sx < 0.0 { "l" } else { "r" })
}

/// Bumper blade across the nose / tail face that also wraps round the corners onto the sides.
fn blade(k: &mut Kit, key: &PartKey, front: bool, z0: f32, z1: f32, proud: f32, wrap: f32) {
    let p = k.p;
    let (yf, w) = if front {
        (k.yn, k.wn())
    } else {
        (k.yt, k.wt())
    };
    if front {
        k.front_box(key, -w - 0.012, w + 0.012, z0, z1, proud, 0.012);
    } else {
        k.rear_box(key, -w - 0.012, w + 0.012, z0, z1, proud, 0.012);
    }
    if wrap > 0.0 {
        let d = if front { -1.0 } else { 1.0 };
        let yw = yf + d * wrap;
        for sx in SIDES {
            let ws = p.half_width(yf + d * 0.5 * wrap) + 0.012;
            k.bbx(
                key,
                [sx * (ws - 0.04), yw, z0],
                [sx * (ws + 0.012), yf - d * 0.01, z1],
                0.01,
            );
        }
    }
}

/// Licence plate (blank plate on a dark recess) on the nose or tail face, centred at `z`.
fn plate(k: &mut Kit, front: bool, z: f32) {
    let region = if front { FB } else { RB };
    let name = if front { "plate_front" } else { "plate_rear" };
    let kp = dkey(name, region, M::Metal);
    let kr = dkey(&format!("{name}_recess"), region, M::Grille);
    let (hw, hh) = (0.26, 0.075);
    if front {
        k.front_box(
            &kr,
            -hw - 0.02,
            hw + 0.02,
            z - hh - 0.02,
            z + hh + 0.02,
            0.006,
            0.0,
        );
        k.front_box(&kp, -hw, hw, z - hh, z + hh, 0.012, 0.004);
    } else {
        k.rear_box(
            &kr,
            -hw - 0.02,
            hw + 0.02,
            z - hh - 0.02,
            z + hh + 0.02,
            0.006,
            0.0,
        );
        k.rear_box(&kp, -hw, hw, z - hh, z + hh, 0.012, 0.004);
    }
}

/// Black lower-door / rocker cladding strip between the arches, `h` tall above the door bottom.
fn rocker_strip(k: &mut Kit, mat: M, h: f32, out: f32) {
    let p = k.p;
    let (yf, _, rf) = k.arch(true);
    let (yr, _, rr) = k.arch(false);
    let (y0, y1) = (yr + rr + 0.05, yf - rf - 0.05);
    let ym = 0.5 * (y0 + y1);
    // Just above the sill tuck (the loft's lowest side point).
    let z = p.sill(ym) + 0.03;
    for sx in SIDES {
        let kl = dkey("rocker_trim_l", BodyRegion::FrameLeft, mat);
        k.side_patch(&kl, sx, y0, y1, z, z + h, out, 0.006, (10, 1));
    }
}

/// Clip a convex (|x|, z) outline to `x0 <= |x| <= x1` (both vertical cuts); stays convex.
fn clip_x(pts: &[[f32; 2]], x0: f32, x1: f32) -> Vec<[f32; 2]> {
    let mut out: Vec<[f32; 2]> = pts.to_vec();
    for (edge, keep_above) in [(x0, true), (x1, false)] {
        let inside = |q: [f32; 2]| {
            if keep_above {
                q[0] >= edge
            } else {
                q[0] <= edge
            }
        };
        let src = std::mem::take(&mut out);
        let n = src.len();
        for i in 0..n {
            let a = src[i];
            let c = src[(i + 1) % n];
            if inside(a) {
                out.push(a);
            }
            if inside(a) != inside(c) {
                let t = (edge - a[0]) / (c[0] - a[0]);
                out.push([edge, a[1] + t * (c[1] - a[1])]);
            }
        }
    }
    out
}

/// Shaped lens / panel on the nose (`front`) or tail face: a convex (|x|, z) outline on side
/// `sx`, standing `proud` out of the face (2 cm buried behind it).
fn face_poly(k: &mut Kit, key: &PartKey, front: bool, sx: f32, pts: &[[f32; 2]], proud: f32) {
    if pts.len() < 3 {
        return;
    }
    let prof: Vec<[f32; 2]> = pts.iter().map(|q| [sx * q[0], q[1]]).collect();
    if front {
        let y = k.yn;
        k.prism_y(key, &prof, y - 0.02, y + proud);
    } else {
        let y = k.yt;
        k.prism_y(key, &prof, y + 0.02, y - proud);
    }
}

/// A shaped lamp cluster: the convex outline `pts` (|x|, z) on side `sx`, cut at |x| = `cut`
/// into the lamp (`lamp`) and the turn signal (`sig`), outboard of the cut when `sig_out`, with
/// the seam between them; a dark housing `border` larger behind it.
fn shaped_cluster(
    k: &mut Kit,
    front: bool,
    sx: f32,
    pts: &[[f32; 2]],
    cut: f32,
    sig_out: bool,
    sig_mat: M,
    proud: f32,
    border: f32,
) {
    let (lamp, sig) = if front {
        (headlight_key(sx), indicator_front_key(sx))
    } else {
        (taillight_key(sx), indicator_rear_key(sx, sig_mat))
    };
    let g = INDICATOR_SEAM_M;
    let (lp, sp) = if sig_out {
        (clip_x(pts, 0.0, cut - g), clip_x(pts, cut, 9.0))
    } else {
        (clip_x(pts, cut + g, 9.0), clip_x(pts, 0.0, cut))
    };
    face_poly(k, &lamp, front, sx, &lp, proud);
    face_poly(k, &sig, front, sx, &sp, proud);
    if border > 0.0 {
        let (cx, cz) = {
            let n = pts.len() as f32;
            (
                pts.iter().map(|q| q[0]).sum::<f32>() / n,
                pts.iter().map(|q| q[1]).sum::<f32>() / n,
            )
        };
        let grow: Vec<[f32; 2]> = pts
            .iter()
            .map(|q| {
                let (dx, dz) = (q[0] - cx, q[1] - cz);
                let l = (dx * dx + dz * dz).sqrt().max(1e-4);
                [q[0] + border * dx / l, q[1] + border * dz / l]
            })
            .collect();
        let region = if front {
            BodyRegion::FrontLeftFender
        } else {
            BodyRegion::Trunk
        };
        let name = if front {
            "headlamp_housing_l"
        } else {
            "taillamp_housing_l"
        };
        let hk = sided(&dkey(name, region, M::Trim), sx);
        face_poly(k, &hk, front, sx, &grow, proud - 0.012);
    }
}

/// The fleet's fictional maker identity (no real brand's grille or badge): a hexagonal
/// "shield" grille, widest at `zm`, with a frame in `frame` a little larger behind it.
/// `half` = (top, widest, bottom) half widths; `rows` horizontal slats and `cols` vertical bars
/// in `bar_mat`. Each car uses its own proportions and fill.
fn shield_grille(
    k: &mut Kit,
    name: &str,
    half: (f32, f32, f32),
    z0: f32,
    zm: f32,
    z1: f32,
    rows: usize,
    cols: usize,
    bar_mat: M,
    frame: M,
) {
    let (ht, hm, hb) = half;
    let outline = |grow: f32| {
        vec![
            [-ht - grow, z1 + grow],
            [ht + grow, z1 + grow],
            [hm + grow, zm],
            [hb + grow, z0 - grow],
            [-hb - grow, z0 - grow],
            [-hm - grow, zm],
        ]
    };
    let y = k.yn;
    let kf = dkey(&format!("{name}_frame"), FB, frame);
    k.prism_y(&kf, &outline(0.022), y - 0.02, y + 0.014);
    let kg = dkey(name, FB, M::Grille);
    k.prism_y(&kg, &outline(0.0), y - 0.02, y + 0.018);
    // Half width of the opening at height z.
    let hw = |z: f32| {
        if z >= zm {
            hm + (ht - hm) * (z - zm) / (z1 - zm).max(1e-3)
        } else {
            hb + (hm - hb) * (z - z0) / (zm - z0).max(1e-3)
        }
    };
    let kb = dkey(&format!("{name}_bars"), FB, bar_mat);
    for i in 1..=rows {
        let z = z0 + (z1 - z0) * i as f32 / (rows + 1) as f32;
        let w = hw(z) - 0.01;
        k.front_box(&kb, -w, w, z - 0.006, z + 0.006, 0.026, 0.0);
    }
    for i in 1..=cols {
        let x = -hm + 2.0 * hm * i as f32 / (cols + 1) as f32;
        // Vertical bar over the height where the opening is wider than |x|.
        let (mut za, mut zb) = (z0, z1);
        if x.abs() > hb {
            za = z0 + (zm - z0) * ((x.abs() - hb) / (hm - hb).max(1e-3)).clamp(0.0, 1.0);
        }
        if x.abs() > ht {
            zb = zm + (z1 - zm) * (1.0 - ((x.abs() - ht) / (hm - ht).max(1e-3)).clamp(0.0, 1.0));
        }
        if zb - za > 0.03 {
            k.front_box(
                &kb,
                x - 0.006,
                x + 0.006,
                za + 0.005,
                zb - 0.005,
                0.026,
                0.0,
            );
        }
    }
}

/// Blank maker's badge (a small shield, no lettering) at (0, z) on the nose or tail face.
fn badge(k: &mut Kit, front: bool, z: f32, size: f32) {
    let region = if front { FB } else { BodyRegion::Trunk };
    let name = if front { "badge_front" } else { "badge_rear" };
    let kb = dkey(name, region, M::Chrome);
    let (w, h) = (0.5 * size, 0.6 * size);
    let prof = [[-w, z + h], [w, z + h], [w, z], [0.0, z - h], [-w, z]];
    if front {
        let y = k.yn;
        k.prism_y(&kb, &prof, y - 0.01, y + 0.034);
    } else {
        let y = k.yt;
        k.prism_y(&kb, &prof, y + 0.01, y - 0.03);
    }
}

// ---------------------------------------------------------------------------------------
// sentinel_cruiser
// ---------------------------------------------------------------------------------------

/// 2010s full-size RWD sedan: wide low shield grille, swept slit lamps, boomerang tail lamps.
pub fn cruiser(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();

    // --- Front: the fleet's wide, low shield grille with four slats and a blank badge, swept
    // lamps over the corners, lower intake.
    let (gz0, gz1) = (nz1 - 0.19, nz1 - 0.035);
    shield_grille(
        &mut k,
        "grille",
        (0.44, 0.48, 0.36),
        gz0,
        gz0 + 0.10,
        gz1,
        4,
        0,
        M::Trim,
        M::Chrome,
    );
    badge(&mut k, true, gz1 - 0.02, 0.07);
    for sx in SIDES {
        // Swept slit lamp: pointed inboard end toward the grille, taller outboard end with
        // the amber signal, in a black housing that also runs back over the nose top.
        let t = nz1;
        let outline = [
            [0.50, t - 0.06],
            [0.60, t - 0.088],
            [wn - 0.03, t - 0.088],
            [wn - 0.03, t - 0.012],
            [0.57, t - 0.028],
        ];
        shaped_cluster(
            &mut k,
            true,
            sx,
            &outline,
            wn - 0.11,
            true,
            M::EmissiveAmber,
            0.022,
            0.016,
        );
        let hk = sided(
            &dkey("headlamp_housing_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        k.patch(
            &hk,
            yn - 0.10,
            yn - 0.01,
            top_band(sx * 0.53, sx * (wn - 0.03)),
            0.002,
            0.006,
            (1, 2),
        );
        // LED eyebrow under the lamp.
        let drl = sided(
            &lkey("drl_l", BodyRegion::FrontLeftFender, M::Headlight),
            sx,
        );
        let brow = [
            [0.60, t - 0.112],
            [wn - 0.05, t - 0.112],
            [wn - 0.05, t - 0.100],
            [0.62, t - 0.100],
        ];
        face_poly(&mut k, &drl, true, sx, &brow, 0.020);
        // Fog lamp pod in the lower bumper corner.
        let pod = sided(&dkey("fog_pod_l", FB, M::Grille), sx);
        k.front_box(
            &pod,
            sx * (wn - 0.28),
            sx * (wn - 0.08),
            nz0 + 0.09,
            nz0 + 0.19,
            0.006,
            0.01,
        );
    }
    k.fogs(wn - 0.18, nz0 + 0.14, 0.035);
    k.mesh_grille(
        "lower_intake",
        FB,
        0.52,
        nz0 + 0.07,
        nz0 + 0.20,
        2,
        10,
        M::Trim,
    );
    let chin = dkey("chin_lip", FB, M::Grille);
    k.front_box(
        &chin,
        -wn + 0.10,
        wn - 0.10,
        nz0 - 0.005,
        nz0 + 0.03,
        0.03,
        0.006,
    );

    // --- Rear: per side a boomerang lamp (a long bar along the deck edge with a drop at the
    // outer corner, the drop being the red-on-red signal) on a dark band, blank badge between
    // them, reverse lamps, diffuser valance.
    let (lz0, lz1) = (tz1 - 0.15, tz1 - 0.035);
    k.rear_panel(
        "tail_panel",
        BodyRegion::Trunk,
        wt - 0.02,
        lz1 - 0.06,
        lz1 + 0.02,
        M::Grille,
    );
    badge(&mut k, false, lz1 - 0.03, 0.06);
    for sx in SIDES {
        let bar = [
            [0.20, lz1 - 0.045],
            [wt - 0.13, lz1 - 0.045],
            [wt - 0.13, lz1 - 0.005],
            [0.24, lz1 - 0.005],
        ];
        face_poly(&mut k, &taillight_key(sx), false, sx, &bar, 0.024);
        let drop = [
            [wt - 0.125, lz0 + 0.01],
            [wt - 0.04, lz0 + 0.03],
            [wt - 0.04, lz1 - 0.005],
            [wt - 0.125, lz1 - 0.005],
        ];
        face_poly(
            &mut k,
            &indicator_rear_key(sx, M::Taillight),
            false,
            sx,
            &drop,
            0.024,
        );
        let hk = sided(&dkey("taillamp_housing_l", BodyRegion::Trunk, M::Trim), sx);
        let (h0, h1) = (sx * (wt - 0.14), sx * (wt - 0.025));
        k.rear_box(
            &hk,
            h0.min(h1),
            h0.max(h1),
            lz0 - 0.005,
            lz1 + 0.01,
            0.012,
            0.004,
        );
        let rev = sided(&lkey("reverse_lamp_l", BodyRegion::Trunk, M::Headlight), sx);
        let (r0, r1) = (sx * 0.30, sx * 0.40);
        k.rear_box(
            &rev,
            r0.min(r1),
            r0.max(r1),
            lz0 - 0.11,
            lz0 - 0.05,
            0.012,
            0.004,
        );
    }
    plate(&mut k, false, 0.5 * (tz0 + lz0) + 0.03);
    let val = dkey("rear_valance", RB, M::Grille);
    k.rear_box(
        &val,
        -wt + 0.08,
        wt - 0.08,
        tz0 - 0.01,
        tz0 + 0.10,
        0.012,
        0.006,
    );
    k.lip_spoiler(M::Paint, k.yt + 0.16, 0.025, 0.10, 0.12);

    // --- Sides: mirrors, handles, filler, black rocker.
    rocker_strip(&mut k, M::Grille, 0.06, 0.004);
    k.mirrors(0.10, 0.13, None);
    k.door_handles(M::Paint);
    k.fuel_filler(-1.0, 0.12, M::Paint);
    let _ = p;
}

// ---------------------------------------------------------------------------------------
// sentinel_wolfhound
// ---------------------------------------------------------------------------------------

/// 2010s AWD police sedan: tall shield grille with vertical bars, swept lamps, corner tail lamps.
pub fn wolfhound(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let yt = k.yt;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();

    // --- Front: the fleet's shield grille, tall here, filled with fine vertical bars, badge.
    let (gz0, gz1) = (nz1 - 0.30, nz1 - 0.03);
    shield_grille(
        &mut k,
        "grille",
        (0.40, 0.46, 0.30),
        gz0,
        gz0 + 0.17,
        gz1,
        0,
        11,
        M::Trim,
        M::Chrome,
    );
    badge(&mut k, true, gz1 - 0.035, 0.08);
    for sx in SIDES {
        // Swept lamp: a long pentagon rising from the grille corner to the fender, amber
        // outboard end, black housing that also runs back over the nose top.
        let t = nz1;
        let outline = [
            [0.535, t - 0.15],
            [wn - 0.03, t - 0.10],
            [wn - 0.03, t - 0.015],
            [0.58, t - 0.015],
            [0.535, t - 0.04],
        ];
        shaped_cluster(
            &mut k,
            true,
            sx,
            &outline,
            wn - 0.11,
            true,
            M::EmissiveAmber,
            0.022,
            0.016,
        );
        let hk = sided(
            &dkey("headlamp_housing_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        k.patch(
            &hk,
            yn - 0.14,
            yn - 0.01,
            top_band(sx * 0.56, sx * (wn - 0.03)),
            0.002,
            0.006,
            (2, 2),
        );
        // Lower corner intake with a round fog lamp.
        let pod = sided(&dkey("fog_pod_l", FB, M::Grille), sx);
        k.front_box(
            &pod,
            sx * (wn - 0.30),
            sx * (wn - 0.06),
            nz0 + 0.08,
            nz0 + 0.22,
            0.006,
            0.02,
        );
    }
    k.fogs(wn - 0.18, nz0 + 0.15, 0.04);
    k.mesh_grille(
        "lower_intake",
        FB,
        0.42,
        nz0 + 0.06,
        nz0 + 0.17,
        1,
        8,
        M::Trim,
    );
    plate(&mut k, true, nz0 + 0.27);

    // --- Rear: wide lamps on the face corners joined by a bright garnish, reverse lamps.
    let (lz0, lz1) = (tz1 - 0.20, tz1 - 0.03);
    for sx in SIDES {
        // Lamp wrapping the corner: wider at the top, amber outboard end.
        let outline = [
            [0.42, lz0 + 0.02],
            [wt - 0.07, lz0 + 0.05],
            [wt - 0.03, lz1 - 0.01],
            [0.40, lz1 - 0.01],
        ];
        shaped_cluster(
            &mut k,
            false,
            sx,
            &outline,
            wt - 0.15,
            true,
            M::EmissiveAmber,
            0.024,
            0.016,
        );
        let rev = sided(&lkey("reverse_lamp_l", BodyRegion::Trunk, M::Headlight), sx);
        let (r0, r1) = (sx * 0.43, sx * 0.55);
        k.rear_box(
            &rev,
            r0.min(r1),
            r0.max(r1),
            lz1 - 0.06,
            lz1 - 0.015,
            0.028,
            0.004,
        );
    }
    badge(&mut k, false, lz1 - 0.05, 0.07);
    plate(&mut k, false, lz0 - 0.12);
    let val = dkey("rear_valance", RB, M::Grille);
    k.rear_box(
        &val,
        -wt + 0.10,
        wt - 0.10,
        tz0 - 0.01,
        tz0 + 0.08,
        0.012,
        0.006,
    );
    for sx in SIDES {
        let refl = sided(&lkey("rear_reflector_l", RB, M::Taillight), sx);
        let (a, c) = (sx * (wt - 0.22), sx * (wt - 0.08));
        k.rear_box(
            &refl,
            a.min(c),
            a.max(c),
            tz0 + 0.12,
            tz0 + 0.15,
            0.010,
            0.002,
        );
    }
    let _ = (p, yt);

    // --- Sides.
    rocker_strip(&mut k, M::Grille, 0.07, 0.004);
    k.mirrors(0.10, 0.14, None);
    k.door_handles(M::Paint);
    k.fuel_filler(-1.0, 0.12, M::Paint);
}

// ---------------------------------------------------------------------------------------
// sentinel_bastion
// ---------------------------------------------------------------------------------------

/// Full-size body-on-frame SUV: big egg-crate shield grille, stacked lamps, vertical tail lamps,
/// running boards, hitch.
pub fn bastion(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yt = k.yt;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();

    // --- Front: the fleet's shield grille at its biggest, a coarse egg-crate fill, the blank
    // badge; lamps beside it.
    let (lz0, lz1) = (nz1 - 0.17, nz1 - 0.04);
    shield_grille(
        &mut k,
        "grille",
        (0.47, 0.50, 0.40),
        nz1 - 0.42,
        nz1 - 0.16,
        nz1 - 0.03,
        4,
        7,
        M::Trim,
        M::Chrome,
    );
    badge(&mut k, true, nz1 - 0.10, 0.10);
    for sx in SIDES {
        let hk = sided(
            &dkey("headlamp_housing_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        let (h0, h1) = (sx * 0.54, sx * (wn - 0.02));
        k.front_box(
            &hk,
            h0.min(h1),
            h0.max(h1),
            lz0 - 0.02,
            lz1 + 0.035,
            0.010,
            0.008,
        );
        // Stacked lamp: two projector lenses over the amber turn strip.
        k.front_cluster(
            sx,
            0.56,
            wn - 0.04,
            lz0,
            lz1 - 0.005,
            0.022,
            0.006,
            Seg::Lower(0.035),
        );
        let proj = sided(
            &dkey(
                "headlamp_projector_l",
                BodyRegion::FrontLeftFender,
                M::Chrome,
            ),
            sx,
        );
        let zp = lz0 + 0.035 + 0.5 * (lz1 - 0.005 - lz0 - 0.035);
        for t in [0.3f32, 0.7] {
            let x = sx * (0.56 + (wn - 0.04 - 0.56) * t);
            k.round_lamp(&proj, true, x, zp, 0.035, 0.028, None);
        }
        let drl = sided(
            &lkey("drl_l", BodyRegion::FrontLeftFender, M::Headlight),
            sx,
        );
        k.front_box(
            &drl,
            sx * 0.56,
            sx * (wn - 0.04),
            lz1 + 0.012,
            lz1 + 0.024,
            0.016,
            0.0,
        );
    }
    // Lower bumper: dark valance with fog lamps and a skid plate.
    let val = dkey("front_valance", FB, M::Grille);
    k.front_box(
        &val,
        -wn + 0.06,
        wn - 0.06,
        nz0 + 0.02,
        nz0 + 0.24,
        0.008,
        0.01,
    );
    k.fogs(wn - 0.20, nz0 + 0.15, 0.045);
    let skid = dkey("skid_plate", FB, M::Metal);
    k.front_box(&skid, -0.40, 0.40, nz0 - 0.02, nz0 + 0.05, 0.03, 0.01);
    plate(&mut k, true, nz0 + 0.33);

    // --- Rear: vertical lamps on the corners, black bumper step, plate, hitch.
    let (rz0, rz1) = (tz1 - 0.42, tz1 - 0.02);
    for sx in SIDES {
        k.rear_cluster(
            sx,
            wt - 0.17,
            wt - 0.025,
            rz0,
            rz1,
            0.026,
            0.008,
            Seg::Lower(0.09),
            M::EmissiveAmber,
        );
        let rev = sided(&lkey("reverse_lamp_l", BodyRegion::Trunk, M::Headlight), sx);
        let (r0, r1) = (sx * (wt - 0.165), sx * (wt - 0.03));
        k.rear_box(
            &rev,
            r0.min(r1),
            r0.max(r1),
            rz1 - 0.17,
            rz1 - 0.12,
            0.029,
            0.004,
        );
    }
    let step = dkey("bumper_step", RB, M::Grille);
    k.rear_box(
        &step,
        -wt + 0.04,
        wt - 0.04,
        tz0 + 0.20,
        tz0 + 0.32,
        0.04,
        0.01,
    );
    let handle = dkey("tailgate_handle", BodyRegion::Trunk, M::Chrome);
    k.rear_box(&handle, -0.28, 0.28, rz1 - 0.12, rz1 - 0.07, 0.016, 0.006);
    plate(&mut k, false, rz0 + 0.12);
    let hitch = dkey("hitch_receiver", RB, M::Grille);
    k.bbx(
        &hitch,
        [-0.04, yt + 0.25, tz0 - 0.02],
        [0.04, yt - 0.08, tz0 + 0.06],
        0.006,
    );
    let _ = p;

    // --- Sides: running boards, rocker cladding, big mirrors.
    k.skirts(M::Grille, 0.09, 0.05);
    rocker_strip(&mut k, M::Grille, 0.09, 0.004);
    k.mirrors(0.08, 0.19, Some(M::Grille));
    k.door_handles(M::Paint);
    k.fuel_filler(-1.0, 0.14, M::Paint);
}

// ---------------------------------------------------------------------------------------
// sentinel_warden
// ---------------------------------------------------------------------------------------

/// Late-80s boxy sedan: chrome egg-crate grille, quad rectangular lamps, amber wraparound
/// corners, chrome bumpers with rubber strips, wide ribbed tail lamps.
pub fn warden(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let yt = k.yt;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();

    // --- Front: bumper blade (the kit's rubber guards sit on it), grille, quad lamps.
    let (bz0, bz1) = (nz0 + 0.18, nz0 + 0.33);
    let chrome = dkey("bumper_blade_front", FB, M::Chrome);
    blade(&mut k, &chrome, true, bz0, bz1, 0.07, 0.30);
    let rub = dkey("bumper_rubber_front", FB, M::Rubber);
    k.front_box(&rub, -wn, wn, bz0 + 0.05, bz0 + 0.09, 0.075, 0.004);
    let val = dkey("front_valance", FB, M::Grille);
    k.front_box(&val, -wn + 0.08, wn - 0.08, nz0 + 0.02, bz0, 0.01, 0.0);

    let (gz0, gz1) = (bz1 + 0.025, nz1 - 0.02);
    let gw = 0.30;
    k.mesh_grille("grille", FB, gw, gz0, gz1, 4, 9, M::Chrome);
    let frame = dkey("grille_frame", FB, M::Chrome);
    k.front_box(
        &frame,
        -gw - 0.03,
        gw + 0.03,
        gz1,
        gz1 + 0.025,
        0.022,
        0.006,
    );
    k.front_box(&frame, -gw - 0.03, gw + 0.03, gz0 - 0.02, gz0, 0.022, 0.006);
    let hdr = dkey("header_panel", FB, M::Chrome);
    for sx in SIDES {
        k.front_box(&frame, sx * gw, sx * (gw + 0.03), gz0, gz1, 0.022, 0.006);
        // Quad rectangular sealed beams in one chrome bezel: inner + outer lamp.
        let bez = sided(
            &dkey("headlamp_bezel_l", BodyRegion::FrontLeftFender, M::Chrome),
            sx,
        );
        let (x_in, x_out) = (gw + 0.06, wn - 0.17);
        let (lz0, lz1) = (gz0 + 0.03, gz1 - 0.01);
        k.front_box(
            &bez,
            sx * (x_in - 0.02),
            sx * (x_out + 0.02),
            lz0 - 0.02,
            lz1 + 0.02,
            0.014,
            0.004,
        );
        let mid = 0.5 * (x_in + x_out);
        let hk = headlight_key(sx);
        for (a, c) in [(x_in, mid - 0.012), (mid + 0.012, x_out)] {
            k.front_box(&hk, sx * a, sx * c, lz0, lz1, 0.024, 0.006);
        }
        // Amber wraparound corner lamp: face part + a lens round the fender corner.
        let ik = indicator_front_key(sx);
        k.front_box(
            &ik,
            sx * (x_out + 0.03),
            sx * (wn - 0.01),
            lz0 - 0.01,
            lz1 + 0.01,
            0.02,
            0.006,
        );
        let wf = p.half_width(yn - 0.10);
        k.bbx(
            &ik,
            [sx * (wf - 0.03), yn - 0.18, lz0 - 0.01],
            [sx * (wf + 0.012), yn - 0.005, lz1 + 0.01],
            0.006,
        );
    }
    k.front_box(
        &hdr,
        -wn + 0.02,
        wn - 0.02,
        nz1 - 0.012,
        nz1 + 0.004,
        0.012,
        0.0,
    );
    plate(&mut k, true, nz0 + 0.12);

    // --- Rear: chrome blade, wide ribbed tail lamps, centre plate panel between them.
    let (bz0, bz1) = (tz0 + 0.14, tz0 + 0.28);
    let rchrome = dkey("bumper_blade_rear", RB, M::Chrome);
    blade(&mut k, &rchrome, false, bz0, bz1, 0.07, 0.30);
    let rrub = dkey("bumper_rubber_rear", RB, M::Rubber);
    k.rear_box(&rrub, -wt, wt, bz0 + 0.05, bz0 + 0.09, 0.075, 0.004);
    let (lz0, lz1) = (bz1 + 0.04, tz1 - 0.03);
    k.rear_panel(
        "tail_panel",
        BodyRegion::Trunk,
        wt - 0.01,
        lz0 - 0.015,
        lz1 + 0.015,
        M::Grille,
    );
    let ribs = dkey("taillight_ribs", BodyRegion::Trunk, M::Chrome);
    for sx in SIDES {
        k.rear_cluster(
            sx,
            0.30,
            wt - 0.03,
            lz0,
            lz1,
            0.020,
            0.004,
            Seg::Outboard(0.13),
            M::Taillight,
        );
        let rev = sided(&lkey("reverse_lamp_l", BodyRegion::Trunk, M::Headlight), sx);
        let (r0, r1) = (sx * 0.30, sx * 0.42);
        k.rear_box(
            &rev,
            r0.min(r1),
            r0.max(r1),
            lz0 + 0.005,
            lz0 + 0.045,
            0.024,
            0.0,
        );
        // Horizontal chrome ribs across each lens.
        for t in [0.33f32, 0.66] {
            let z = lz0 + (lz1 - lz0) * t;
            let (a, c) = (sx * 0.30, sx * (wt - 0.03));
            k.rear_box(&ribs, a.min(c), a.max(c), z - 0.005, z + 0.005, 0.026, 0.0);
        }
    }
    let pk = dkey("plate_panel", BodyRegion::Trunk, M::Chrome);
    k.rear_box(&pk, -0.28, 0.28, lz0 - 0.01, lz1 + 0.01, 0.012, 0.004);
    plate(&mut k, false, 0.5 * (lz0 + lz1));
    let _ = yt;

    // --- Sides: chrome drip / window trim come from the knobs; chrome handles, mirrors,
    // side marker lamps at the corners.
    k.mirrors(0.10, 0.11, Some(M::Chrome));
    k.door_handles(M::Chrome);
    k.fuel_filler(-1.0, 0.11, M::Paint);
    for sx in SIDES {
        let fr = sided(
            &lkey(
                "side_marker_rear_l",
                BodyRegion::RearLeftQuarter,
                M::Taillight,
            ),
            sx,
        );
        let y = k.yt + 0.14;
        k.side_box(&fr, sx, y, tz1 - 0.08, 0.10, 0.04, 0.008, 0.0);
    }
}

// ---------------------------------------------------------------------------------------
// sentinel_vigil
// ---------------------------------------------------------------------------------------

/// 2010s muscle coupe: slim upper slot + big lower intake, power-dome hood with extractor
/// vents, twin horizontal tail lamps in a dark band, quad tips.
pub fn vigil(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (_, tz1) = k.tail_z();

    // --- Front: slim lamps high on the corners over one big shield-shaped mouth (the fleet's
    // grille, inverted proportions: narrow top, wide lower half) with a mesh fill.
    let (sz0, sz1) = (nz1 - 0.11, nz1 - 0.025);
    for sx in SIDES {
        // Lamp lens in line with the slot, on the face (housing dark), amber inboard.
        // Slim, angry lamp: the top edge drops toward the slot, the outer end is taller;
        // amber inboard segment, dark housing.
        let outline = [
            [0.46, sz0 + 0.004],
            [wn - 0.04, sz0 + 0.018],
            [wn - 0.04, sz1 + 0.006],
            [0.50, sz1 - 0.008],
            [0.46, sz1 - 0.025],
        ];
        shaped_cluster(
            &mut k,
            true,
            sx,
            &outline,
            0.53,
            false,
            M::EmissiveAmber,
            0.022,
            0.012,
        );
        // Hockey-stick LED blade below the lamp.
        let drl = sided(
            &lkey("drl_l", BodyRegion::FrontLeftFender, M::Headlight),
            sx,
        );
        k.front_box(
            &drl,
            sx * 0.60,
            sx * (wn - 0.05),
            sz0 - 0.03,
            sz0 - 0.017,
            0.016,
            0.0,
        );
        // Brake-cooling inlets at the lower corners.
        let duct = sided(&dkey("brake_duct_l", FB, M::Grille), sx);
        k.front_box(
            &duct,
            sx * (wn - 0.20),
            sx * (wn - 0.06),
            nz0 + 0.06,
            nz0 + 0.20,
            0.008,
            0.01,
        );
    }
    let (mz0, mz1) = (nz0 + 0.05, nz1 - 0.03);
    shield_grille(
        &mut k,
        "grille",
        (0.34, 0.50, 0.44),
        mz0,
        mz0 + 0.45 * (mz1 - mz0),
        mz1,
        5,
        13,
        M::Trim,
        M::Grille,
    );
    badge(&mut k, true, mz1 - 0.03, 0.06);

    // Hood: two extractor vents either side of the power dome.
    let span = yn - p.y_fw;
    for sx in SIDES {
        let (x0, x1) = (sx * 0.26, sx * 0.44);
        k.top_vent(
            &side_name("hood_vent", sx),
            BodyRegion::Hood,
            yn - 0.40 * span,
            yn - 0.22 * span,
            x0.min(x1),
            x0.max(x1),
            4,
            M::Trim,
        );
    }

    // --- Rear: dark band, one swept lamp per side, reverse lamps and badge in the centre.
    let (lz0, lz1) = (tz1 - 0.15, tz1 - 0.03);
    k.rear_panel(
        "tail_panel",
        BodyRegion::Trunk,
        wt - 0.02,
        lz0 - 0.02,
        lz1 + 0.02,
        M::Grille,
    );
    for sx in SIDES {
        // One lamp per side whose outer end sweeps up into the corner; the inboard end is the
        // red-on-red signal.
        let outline = [
            [0.24, lz0 + 0.035],
            [wt - 0.06, lz0 + 0.01],
            [wt - 0.04, lz1 - 0.005],
            [0.28, lz1 - 0.015],
        ];
        shaped_cluster(
            &mut k,
            false,
            sx,
            &outline,
            0.36,
            false,
            M::Taillight,
            0.022,
            0.0,
        );
        let rev = sided(&lkey("reverse_lamp_l", BodyRegion::Trunk, M::Headlight), sx);
        let (r0, r1) = (sx * 0.05, sx * 0.20);
        k.rear_box(
            &rev,
            r0.min(r1),
            r0.max(r1),
            lz0 + 0.03,
            lz1 - 0.03,
            0.018,
            0.0,
        );
    }
    badge(&mut k, false, 0.5 * (lz0 + lz1), 0.05);
    plate(&mut k, false, lz0 - 0.11);
    k.diffuser(M::Grille, 0.62, 0.12, 0, 0.0);

    // --- Sides: side skirts, mirrors, handles, filler; rear-quarter faux vent ahead of the arch.
    k.skirts(M::Grille, 0.025, 0.05);
    k.mirrors(0.10, 0.12, None);
    k.door_handles(M::Paint);
    k.fuel_filler(-1.0, 0.11, M::Paint);
    let (yr, zr, rr) = k.arch(false);
    for sx in SIDES {
        let kv = dkey("quarter_vent_l", BodyRegion::RearLeftQuarter, M::Grille);
        let y1 = yr + rr + 0.12;
        k.side_patch(
            &kv,
            sx,
            y1,
            y1 + 0.16,
            zr + 0.10,
            zr + 0.20,
            0.003,
            0.006,
            (2, 1),
        );
    }
}
