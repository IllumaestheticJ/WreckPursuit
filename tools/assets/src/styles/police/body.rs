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
//! References (generic descriptions only, CLAUDE.md rule 9):
//! - `sentinel_cruiser`: 2010s American full-size RWD police sedan with muscle-car styling:
//!   wide crosshair grille, narrow swept lamps, one full-width "racetrack" tail lamp.
//! - `sentinel_wolfhound`: 2010s American AWD police sedan on a large FWD-platform saloon: tall
//!   body, high belt, big trapezoid three-bar grille, swept lamps, wide tail lamps joined by a
//!   bright garnish.
//! - `sentinel_bastion`: 2010s American full-size body-on-frame SUV: tall upright nose, two-tier
//!   grille with a bright cross bar, stacked rectangular lamps, vertical tail lamps beside a
//!   near-vertical tailgate, running boards, hitch.
//! - `sentinel_warden`: late-80s American full-size body-on-frame sedan: flat slab sides,
//!   upright chrome egg-crate grille, quad rectangular sealed-beam lamps, amber wraparound
//!   corners, chrome bumpers with rubber strips, wide ribbed tail lamps.
//! - `sentinel_vigil`: 2010s American muscle coupe: slim upper grille slot in line with the
//!   lamps, big lower intake, power-dome hood with extractor vents, low "gun-slit" greenhouse,
//!   twin horizontal tail lamps in a dark band, quad tips.

// Kit helpers take explicit coordinates, like the performance kits.
#![allow(clippy::too_many_arguments)]

use wp_contracts::BodyRegion;

use crate::builder::{Builder, PartKey};
use crate::mesh::MaterialSlot as M;
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

// ---------------------------------------------------------------------------------------
// sentinel_cruiser
// ---------------------------------------------------------------------------------------

/// 2010s full-size RWD sedan: crosshair grille, swept lamps, racetrack tail lamp.
pub fn cruiser(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();

    // --- Front: wide grille with a crosshair, swept lamps over the corners, lower intake.
    let (gz0, gz1) = (nz1 - 0.19, nz1 - 0.035);
    k.mesh_grille("grille", FB, 0.47, gz0, gz1, 3, 12, M::Trim);
    let cross = dkey("grille_crosshair", FB, M::Metal);
    let zc = 0.5 * (gz0 + gz1);
    k.front_box(&cross, -0.47, 0.47, zc - 0.011, zc + 0.011, 0.022, 0.004);
    k.front_box(&cross, -0.012, 0.012, gz0, gz1, 0.022, 0.004);
    let surround = dkey("grille_surround", FB, M::Trim);
    k.front_box(&surround, -0.50, 0.50, gz1, gz1 + 0.022, 0.016, 0.005);
    k.front_box(&surround, -0.50, 0.50, gz0 - 0.022, gz0, 0.016, 0.005);
    for sx in SIDES {
        k.front_box(
            &surround,
            sx * 0.47,
            sx * 0.50,
            gz0 - 0.022,
            gz1 + 0.022,
            0.016,
            0.005,
        );
        // Narrow slit lamp across the upper corner of the face in a black housing that runs
        // back over the nose top; amber outboard segment, LED eyebrow under it.
        let hk = sided(
            &dkey("headlamp_housing_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        let (h0, h1) = (sx * 0.51, sx * (wn - 0.02));
        k.front_box(
            &hk,
            h0.min(h1),
            h0.max(h1),
            nz1 - 0.115,
            nz1 + 0.004,
            0.010,
            0.006,
        );
        k.patch(
            &hk,
            yn - 0.10,
            yn - 0.01,
            top_band(sx * 0.51, sx * (wn - 0.03)),
            0.002,
            0.006,
            (1, 2),
        );
        k.front_cluster(
            sx,
            0.53,
            wn - 0.04,
            nz1 - 0.085,
            nz1 - 0.012,
            0.020,
            0.006,
            Seg::Outboard(0.07),
        );
        let drl = sided(
            &lkey("drl_l", BodyRegion::FrontLeftFender, M::Headlight),
            sx,
        );
        k.front_box(
            &drl,
            sx * 0.55,
            sx * (wn - 0.12),
            nz1 - 0.105,
            nz1 - 0.093,
            0.018,
            0.0,
        );
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

    // --- Rear: full-width racetrack tail lamp on a dark band, reverse lamps, diffuser valance.
    let (lz0, lz1) = (tz1 - 0.15, tz1 - 0.035);
    k.rear_panel(
        "tail_panel",
        BodyRegion::Trunk,
        wt - 0.02,
        lz0 - 0.02,
        lz1 + 0.02,
        M::Grille,
    );
    for sx in SIDES {
        // One continuous lens per side meeting at the centre (a hair apart); the outer end is
        // the red-on-red turn signal.
        k.rear_cluster(
            sx,
            0.004,
            wt - 0.04,
            lz0 + 0.02,
            lz1 - 0.02,
            0.022,
            0.004,
            Seg::Outboard(0.16),
            M::Taillight,
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

/// 2010s AWD police sedan: big trapezoid three-bar grille, swept lamps, tail garnish.
pub fn wolfhound(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let yt = k.yt;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();

    // --- Front: trapezoid grille, three wide bars, narrowing downward.
    let (gz0, gz1) = (nz1 - 0.30, nz1 - 0.03);
    let g = dkey("grille", FB, M::Grille);
    let bars = dkey("grille_bars", FB, M::Chrome);
    let frame = dkey("grille_frame", FB, M::Trim);
    let rows = 5;
    for i in 0..rows {
        let t0 = i as f32 / rows as f32;
        let t1 = (i + 1) as f32 / rows as f32;
        let (z0, z1) = (gz0 + (gz1 - gz0) * t0, gz0 + (gz1 - gz0) * t1);
        let hw = 0.36 + 0.14 * t1;
        k.front_box(&g, -hw, hw, z0, z1, 0.006, 0.0);
        k.front_box(&frame, -hw - 0.025, -hw, z0, z1 + 0.001, 0.016, 0.004);
        k.front_box(&frame, hw, hw + 0.025, z0, z1 + 0.001, 0.016, 0.004);
    }
    k.front_box(&frame, -0.525, 0.525, gz1, gz1 + 0.022, 0.016, 0.004);
    k.front_box(&frame, -0.385, 0.385, gz0 - 0.022, gz0, 0.016, 0.004);
    for (i, t) in [0.25f32, 0.52, 0.78].iter().enumerate() {
        let z = gz0 + (gz1 - gz0) * t;
        let hw = 0.36 + 0.14 * t - 0.02;
        let h = if i == 2 { 0.016 } else { 0.013 };
        k.front_box(&bars, -hw, hw, z - h, z + h, 0.020, 0.005);
    }
    for sx in SIDES {
        // Swept lamp: a black housing over the face's upper corner and back over the nose
        // top, the lens with an amber outboard end, its inboard end kicked down beside the
        // grille.
        let hk = sided(
            &dkey("headlamp_housing_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        let (h0, h1) = (sx * 0.535, sx * (wn - 0.02));
        k.front_box(
            &hk,
            h0.min(h1),
            h0.max(h1),
            nz1 - 0.15,
            nz1 + 0.004,
            0.010,
            0.008,
        );
        k.patch(
            &hk,
            yn - 0.16,
            yn - 0.01,
            top_band(sx * 0.535, sx * (wn - 0.03)),
            0.002,
            0.006,
            (2, 2),
        );
        k.front_cluster(
            sx,
            0.55,
            wn - 0.04,
            nz1 - 0.10,
            nz1 - 0.015,
            0.020,
            0.008,
            Seg::Outboard(0.08),
        );
        let hl = headlight_key(sx);
        let (a, c) = (sx * 0.55, sx * 0.66);
        k.front_box(
            &hl,
            a.min(c),
            a.max(c),
            nz1 - 0.14,
            nz1 - 0.09,
            0.020,
            0.006,
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
        let hk = sided(
            &dkey("taillamp_housing_l", BodyRegion::Trunk, M::Grille),
            sx,
        );
        let (h0, h1) = (sx * 0.40, sx * (wt - 0.02));
        k.rear_box(
            &hk,
            h0.min(h1),
            h0.max(h1),
            lz0 - 0.01,
            lz1 + 0.01,
            0.010,
            0.006,
        );
        k.rear_cluster(
            sx,
            0.42,
            wt - 0.04,
            lz0 + 0.01,
            lz1 - 0.01,
            0.024,
            0.006,
            Seg::Lower(0.05),
            M::EmissiveAmber,
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
    let garnish = dkey("tail_garnish", BodyRegion::Trunk, M::Chrome);
    k.rear_box(&garnish, -0.40, 0.40, lz1 - 0.05, lz1 - 0.02, 0.012, 0.004);
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

/// Full-size body-on-frame SUV: tall two-tier grille, stacked lamps, vertical tail lamps,
/// running boards, hitch.
pub fn bastion(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yt = k.yt;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();

    // --- Front: two-tier grille split by a bright bar at lamp height, lamps beside it.
    let (lz0, lz1) = (nz1 - 0.17, nz1 - 0.04);
    let bar_z = 0.5 * (lz0 + lz1);
    k.mesh_grille("grille", FB, 0.50, nz1 - 0.42, nz1 - 0.03, 4, 6, M::Trim);
    let bar = dkey("grille_bar", FB, M::Chrome);
    k.front_box(
        &bar,
        -0.56,
        0.56,
        bar_z - 0.035,
        bar_z + 0.035,
        0.030,
        0.008,
    );
    let frame = dkey("grille_frame", FB, M::Chrome);
    k.front_box(&frame, -0.53, 0.53, nz1 - 0.035, nz1 - 0.012, 0.022, 0.006);
    k.front_box(&frame, -0.53, 0.53, nz1 - 0.445, nz1 - 0.42, 0.022, 0.006);
    for sx in SIDES {
        k.front_box(
            &frame,
            sx * 0.50,
            sx * 0.53,
            nz1 - 0.445,
            nz1 - 0.012,
            0.022,
            0.006,
        );
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

    // --- Front: slim upper slot between slim lamps; big lower intake with a body bar.
    let (sz0, sz1) = (nz1 - 0.11, nz1 - 0.025);
    k.mesh_grille("grille", FB, 0.42, sz0, sz1, 2, 10, M::Trim);
    for sx in SIDES {
        // Lamp lens in line with the slot, on the face (housing dark), amber inboard.
        let hk = sided(
            &dkey("headlamp_housing_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        let (h0, h1) = (sx * 0.44, sx * (wn - 0.03));
        k.front_box(
            &hk,
            h0.min(h1),
            h0.max(h1),
            sz0 - 0.01,
            sz1 + 0.012,
            0.010,
            0.006,
        );
        k.front_cluster(
            sx,
            0.46,
            wn - 0.05,
            sz0 + 0.005,
            sz1,
            0.020,
            0.006,
            Seg::Inboard(0.06),
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
    let (iz0, iz1) = (nz0 + 0.05, sz0 - 0.06);
    k.mesh_grille("lower_intake", FB, 0.50, iz0, iz1, 3, 12, M::Trim);
    let bar = dkey("intake_bar", FB, M::Paint);
    let zb = 0.5 * (iz0 + iz1);
    k.front_box(&bar, -0.50, 0.50, zb - 0.022, zb + 0.022, 0.024, 0.008);

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

    // --- Rear: dark band with twin horizontal lamps per side, reverse in the centre.
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
        let tk = taillight_key(sx);
        let ik = indicator_rear_key(sx, M::Taillight);
        // Inner lamp (indicator segment) and outer lamp, each a squared ring of red.
        let (i0, i1) = (0.31, 0.53);
        let (o0, o1) = (0.545, wt - 0.05);
        for (a, c, key) in [(i0, i1, &ik), (o0, o1, &tk)] {
            k.rear_box(key, sx * a, sx * c, lz0 + 0.01, lz1 - 0.01, 0.022, 0.006);
        }
        let core = sided(&dkey("taillight_core_l", BodyRegion::Trunk, M::Grille), sx);
        k.rear_box(
            &core,
            sx * (o0 + 0.05),
            sx * (o1 - 0.05),
            lz0 + 0.04,
            lz1 - 0.04,
            0.026,
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
