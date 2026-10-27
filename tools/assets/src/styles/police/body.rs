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
use crate::lamp::{self, Element, Lamp, LampView};
use crate::mesh::MaterialSlot as M;
use crate::style::DetailCtx;
use crate::styles::performance::kit::{
    dkey, headlight_key, indicator_front_key, lkey, sided, Kit, Seg, SIDES,
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
    let raked = k.p.style.nose_face_rake_deg.abs() > 0.5 || k.p.style.nose_chin_m > 0.0;
    if front && !raked {
        // Upright face: plain boxes are cheaper.
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
    } else if front {
        // Projected: follows a raked nose face.
        let (x, zr) = ((-hw - 0.02, hw + 0.02), (z - hh - 0.02, z + hh + 0.02));
        proj_rect(k, &kr, true, x, zr, 0.004);
        proj_rect(k, &kp, true, (-hw, hw), (z - hh, z + hh), 0.012);
    } else {
        // The tails are upright: plain boxes are cheaper.
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

/// Shaped lens / panel on the nose (`front`) or tail face: a convex (|x|, z) outline on side
/// `sx`, standing `proud` out of the face (2 cm buried behind it).
fn face_poly(k: &mut Kit, key: &PartKey, front: bool, sx: f32, pts: &[[f32; 2]], proud: f32) {
    if pts.len() < 3 {
        return;
    }
    let signed: Vec<[f32; 2]> = pts.iter().map(|q| [sx * q[0], q[1]]).collect();
    proj(k, key, front, &signed, proud);
}

/// Projected panel / lens: a signed-x `[x, z]` outline cast onto the nose (`front`) or tail
/// face ([`crate::lamp::lens`]), its face `proud` m (2…30 mm) over the surface, so it follows a
/// raked or curved face.
fn proj(k: &mut Kit, key: &PartKey, front: bool, pts: &[[f32; 2]], proud: f32) -> Lamp {
    let view = if front {
        LampView::Front
    } else {
        LampView::Rear
    };
    let mut lamp = Lamp::new(view, pts.to_vec());
    lamp.proud_m = proud.clamp(0.002, 0.03);
    // Flat panels and grilles need no fine grid (the lamps keep the default 4 cm).
    lamp.step_m = 0.08;
    lamp::lens(k.b, k.p, key, &lamp);
    lamp
}

/// [`proj`] of the rectangle `x0..x1 × z0..z1` (signed x).
fn proj_rect(k: &mut Kit, key: &PartKey, front: bool, x: (f32, f32), z: (f32, f32), proud: f32) {
    let (x0, x1) = (x.0.min(x.1), x.0.max(x.1));
    proj(
        k,
        key,
        front,
        &[[x0, z.0], [x1, z.0], [x1, z.1], [x0, z.1]],
        proud,
    );
}

/// Projected mesh grille on the nose: a dark insert `|x| < half`, `z0..z1`, with `rows - 1`
/// horizontal and `cols - 1` vertical bars as surface strips (`{name}_mesh`, `bar_mat`).
fn proj_mesh_grille(
    k: &mut Kit,
    name: &str,
    half: f32,
    z0: f32,
    z1: f32,
    rows: usize,
    cols: usize,
    bar_mat: M,
) {
    let g = dkey(name, FB, M::Grille);
    let pts = [[-half, z0], [half, z0], [half, z1], [-half, z1]];
    let l = proj(k, &g, true, &pts, 0.006);
    let kb = dkey(&format!("{name}_mesh"), FB, bar_mat);
    for i in 1..rows.max(1) {
        let z = z0 + (z1 - z0) * i as f32 / rows as f32;
        strip(k, &kb, &l, vec![[-half + 0.01, z], [half - 0.01, z]], 0.010);
    }
    for i in 1..cols.max(1) {
        let x = -half + 2.0 * half * i as f32 / cols as f32;
        strip(k, &kb, &l, vec![[x, z0 + 0.008], [x, z1 - 0.008]], 0.010);
    }
}

/// A bar following the surface over lens `l` (an [`Element::Strip`]).
fn strip(k: &mut Kit, key: &PartKey, l: &Lamp, pts: Vec<[f32; 2]>, w: f32) {
    lamp::element(
        k.b,
        k.p,
        key,
        l,
        [0.0, 0.0],
        &Element::Strip { pts, w_m: w },
    );
}

/// Round fog lamps at ±x, z, radius r, projected onto the nose (in a black ring when `ring`).
fn proj_fogs(k: &mut Kit, x: f32, z: f32, r: f32, ring: bool) {
    for sx in SIDES {
        let kf = sided(&lkey("fog_l", FB, M::Headlight), sx);
        let kr = sided(&dkey("fog_ring_l", FB, M::Trim), sx);
        let mut l = Lamp::new(LampView::Front, lamp::ellipse([sx * x, z], [r, r], 14));
        l.proud_m = 0.012;
        l.step_m = 0.08;
        if ring {
            lamp::bezel(k.b, k.p, &kr, &l, 0.014);
        }
        lamp::lens(k.b, k.p, &kf, &l);
    }
}

/// A shaped lamp cluster projected onto the nose (`front`) or tail face
/// ([`Kit::proj_cluster`], so the lens follows the surface): the convex outline `pts` (|x|, z)
/// on side `sx`, cut at |x| = `cut` into the lamp and its turn signal (outboard of the cut when
/// `sig_out`), with a black bezel `border` wide. `yaw_deg` wraps a corner lamp round onto the
/// fender / quarter. `proud` is kept for call-site symmetry (the projected lens is 8 mm proud).
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
    shaped_cluster_yaw(k, front, sx, pts, cut, sig_out, sig_mat, proud, border, 0.0);
}

/// [`shaped_cluster`] with a wrap angle.
fn shaped_cluster_yaw(
    k: &mut Kit,
    front: bool,
    sx: f32,
    pts: &[[f32; 2]],
    cut: f32,
    sig_out: bool,
    sig_mat: M,
    _proud: f32,
    border: f32,
    yaw_deg: f32,
) {
    use crate::lamp::LampView;
    let xmax = pts.iter().map(|q| q[0]).fold(f32::MIN, f32::max);
    let xmin = pts.iter().map(|q| q[0]).fold(f32::MAX, f32::min);
    let seg = if sig_out {
        Seg::Outboard(xmax - cut)
    } else {
        Seg::Inboard(cut - xmin)
    };
    // Outline for the LEFT side (x < 0); proj_cluster mirrors it for sx > 0.
    let left: Vec<[f32; 2]> = pts.iter().map(|q| [-q[0], q[1]]).collect();
    let view = if front {
        LampView::Front
    } else {
        LampView::Rear
    };
    k.proj_cluster(sx, front, view, &left, Some(seg), sig_mat, border, yaw_deg);
}

/// The fleet's fictional maker identity (no real brand's grille or badge): a hexagonal
/// "shield" grille, widest at `zm`, with a frame in `frame` a little larger behind it.
/// `half` = (top, widest, bottom) half widths; `rows` horizontal slats and `cols` vertical bars
/// in `bar_mat` (surface strips, or boxes on the upright face when `flat`: cheaper). Each car
/// uses its own proportions and fill.
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
    flat: bool,
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
    let kf = dkey(&format!("{name}_frame"), FB, frame);
    let kg = dkey(name, FB, M::Grille);
    let mut l = Lamp::new(LampView::Front, outline(0.0));
    l.proud_m = 0.012;
    l.step_m = 0.08;
    lamp::bezel(k.b, k.p, &kf, &l, 0.022);
    lamp::lens(k.b, k.p, &kg, &l);
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
        let w = hw(z) - 0.015;
        if flat {
            k.front_box(&kb, -w, w, z - 0.006, z + 0.006, 0.026, 0.0);
        } else {
            strip(k, &kb, &l, vec![[-w, z], [w, z]], 0.012);
        }
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
            if flat {
                k.front_box(
                    &kb,
                    x - 0.006,
                    x + 0.006,
                    za + 0.008,
                    zb - 0.008,
                    0.026,
                    0.0,
                );
            } else {
                strip(k, &kb, &l, vec![[x, za + 0.008], [x, zb - 0.008]], 0.012);
            }
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
    proj(k, &kb, front, &prof, 0.024);
}

// ---------------------------------------------------------------------------------------
// sentinel_cruiser
// ---------------------------------------------------------------------------------------

/// 2010s full-size RWD sedan: wide low shield grille, swept slit lamps, boomerang tail lamps.
pub fn cruiser(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
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
        1,
        0,
        M::Trim,
        M::Chrome,
        false,
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
    }
    proj_fogs(&mut k, wn - 0.18, nz0 + 0.14, 0.035, false);
    proj_mesh_grille(
        &mut k,
        "lower_intake",
        0.52,
        nz0 + 0.07,
        nz0 + 0.20,
        1,
        1,
        M::Trim,
    );

    // --- Rear: per side a boomerang lamp (a long bar along the deck edge with a drop at the
    // outer corner, the drop being the red-on-red signal) on a dark band, blank badge between
    // them, reverse lamps, diffuser valance.
    let (lz0, lz1) = (tz1 - 0.15, tz1 - 0.035);
    // Dark band 2 mm proud, so the projected lenses (8 mm) stand clear of it.
    let band = dkey("tail_panel", BodyRegion::Trunk, M::Grille);
    k.rear_box(
        &band,
        -wt + 0.02,
        wt - 0.02,
        lz1 - 0.06,
        lz1 + 0.02,
        0.002,
        0.0,
    );
    badge(&mut k, false, lz1 - 0.03, 0.06);
    for sx in SIDES {
        // Blade lamp: thin along the deck edge, growing taller toward the corner, wrapping
        // round onto the quarter; the outer end is the red-on-red signal.
        let blade_lamp = [
            [0.20, lz1 - 0.040],
            [wt - 0.05, lz0 + 0.015],
            [wt - 0.05, lz1 - 0.005],
            [0.22, lz1 - 0.005],
        ];
        shaped_cluster_yaw(
            &mut k,
            false,
            sx,
            &blade_lamp,
            wt - 0.15,
            true,
            M::Taillight,
            0.024,
            0.012,
            20.0,
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
    let yt = k.yt;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();

    // --- Front: the fleet's shield grille, tall here, filled with fine vertical bars, badge.
    let (gz0, gz1) = (nz1 - 0.32, nz1 - 0.06);
    shield_grille(
        &mut k,
        "grille",
        (0.40, 0.46, 0.30),
        gz0,
        gz0 + 0.17,
        gz1,
        0,
        5,
        M::Trim,
        M::Chrome,
        false,
    );
    badge(&mut k, true, gz1 - 0.035, 0.08);
    for sx in SIDES {
        // Swept lamp: a long pentagon rising from the grille corner to the fender, amber
        // outboard end, black housing that also runs back over the nose top.
        let t = nz1;
        let outline = [
            [0.50, t - 0.19],
            [wn - 0.06, t - 0.14],
            [wn - 0.06, t - 0.05],
            [0.55, t - 0.05],
            [0.50, t - 0.08],
        ];
        shaped_cluster(
            &mut k,
            true,
            sx,
            &outline,
            wn - 0.135,
            true,
            M::EmissiveAmber,
            0.022,
            0.016,
        );
        // Lower corner intake with a round fog lamp.
        let pod = sided(&dkey("fog_pod_l", FB, M::Grille), sx);
        proj_rect(
            &mut k,
            &pod,
            true,
            (sx * (wn - 0.30), sx * (wn - 0.06)),
            (nz0 + 0.08, nz0 + 0.22),
            0.006,
        );
    }
    proj_fogs(&mut k, wn - 0.18, nz0 + 0.15, 0.04, true);
    proj_mesh_grille(
        &mut k,
        "lower_intake",
        0.42,
        nz0 + 0.06,
        nz0 + 0.17,
        1,
        5,
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
        true,
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
        // Tall vertical lamp projected onto the corner and wrapped round onto the quarter.
        let tall = [
            [-(wt - 0.17), rz0],
            [-(wt - 0.025), rz0],
            [-(wt - 0.025), rz1],
            [-(wt - 0.17), rz1],
        ];
        k.proj_cluster(
            sx,
            false,
            crate::lamp::LampView::Rear,
            &tall,
            Some(Seg::Lower(0.09)),
            M::EmissiveAmber,
            0.012,
            25.0,
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
    k.mirrors(0.08, 0.14, Some(M::Grille));
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
        proj_rect(
            &mut k,
            &drl,
            true,
            (sx * 0.60, sx * (wn - 0.05)),
            (sz0 - 0.03, sz0 - 0.017),
            0.016,
        );
        // Brake-cooling inlets at the lower corners.
        let duct = sided(&dkey("brake_duct_l", FB, M::Grille), sx);
        proj_rect(
            &mut k,
            &duct,
            true,
            (sx * (wn - 0.20), sx * (wn - 0.06)),
            (nz0 + 0.06, nz0 + 0.20),
            0.008,
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
        2,
        5,
        M::Trim,
        M::Grille,
        false,
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
    // Dark band 2 mm proud, so the projected lenses (8 mm) stand clear of it.
    let band = dkey("tail_panel", BodyRegion::Trunk, M::Grille);
    k.rear_box(
        &band,
        -wt + 0.02,
        wt - 0.02,
        lz0 - 0.02,
        lz1 + 0.02,
        0.002,
        0.0,
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
