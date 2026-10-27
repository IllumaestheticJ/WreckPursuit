//! Wave-3 race cars: `morrigan_55` (front-engine GT endurance racer), `nyx_proto`
//! (closed-cockpit sports prototype) and `merlo_20t` (FWD touring-car racer).
//!
//! Silhouettes (2026-10-26): Morrigan sits its cabin well back (`cabin_shift_m` −0.22) for a
//! long-hood front-engine fastback; Nyx is a narrow teardrop canopy (`c_pillar_rake_deg`)
//! between fender pods (`hood_valley` / `deck_valley`); Merlo an upright four-door sedan
//! greenhouse (small tumblehome, gentle rakes).
//!
//! Race cars share a vocabulary: low ride height, big splitters and wings, flat floors with
//! diffusers, centre-lock wheels, tow straps, roof scoops, door number panels, a windscreen
//! sun strip and a central rain light. Mechanical parts (engine, brakes, wheels, exhaust)
//! come from `mechanical::spec` (table); the styles here only add caliper / spring colours.
//!
//! Liveries are plain coloured patches (paint tint + `accent_rgb`): stripes and blank number
//! panels only, never text, numbers or marks. Each car has its own tail / rain-light pattern:
//! - `morrigan_55`: twin stacked bars + an outer vertical stroke per side, square rain light.
//! - `nyx_proto`: inverted-L lamps (top bar + outer blade) per side, tall narrow rain light.
//! - `merlo_20t`: two-piece sedan lamps (quarter block + lid block) per side, wide rain light.

use wp_contracts::{BodyRegion, VehicleDef};

use crate::builder::{Builder, PartKey};
use crate::mesh::MaterialSlot as M;
use crate::style::{
    ArchShape, BodyStyle, CapKind, CarStyle, DetailCtx, FuelDoor, RimKind, Silhouette, TrimFinish,
};

use super::kit::{dkey, lkey, sided, taillight_key, top_band, Kit, Seg, Stand, Wing, SIDES};
use super::{kit_detail, rgb, wheels};

// ---------------------------------------------------------------------- shared helpers

/// `Kit::top_vent` on side `sx`, from the LEFT-side name / region (`…_l`): the right copy
/// gets the mirrored name and region. `xin..xout` are unsigned |x| bounds.
#[allow(clippy::too_many_arguments)]
fn sided_vent(
    k: &mut Kit,
    name_l: &str,
    region_l: BodyRegion,
    sx: f32,
    y0: f32,
    y1: f32,
    xin: f32,
    xout: f32,
    slats: usize,
    mat: M,
) {
    let key = sided(&dkey(name_l, region_l, M::Grille), sx);
    let region = key.region.unwrap_or(region_l);
    let (a, c) = (sx * xin, sx * xout);
    k.top_vent(&key.name, region, y0, y1, a.min(c), a.max(c), slats, mat);
}

/// Windscreen sun strip: a band `depth` m long (in plan) across the top of the windscreen.
fn sun_strip(k: &mut Kit, depth: f32, mat: M) {
    let p = k.p;
    let y0 = p.roof.1 + 0.01;
    let y1 = y0 + depth;
    let hw = k.roof_edge(0.5 * (y0 + y1)) + 0.04;
    let ks = dkey("sun_strip", BodyRegion::Windshield, mat);
    // Fine across: a coarse grid cuts under the crowned, wrapped glass and flickers.
    k.patch(&ks, y0, y1, top_band(-hw, hw), 0.008, 0.0, (2, 10));
}

/// Tow straps: one flat loop on the nose face at (x, z) and one on the tail face at
/// (x_rear, z_rear), in the accent colour (bumper regions: they fall with the bumpers).
fn tow_straps(k: &mut Kit, x: f32, z: f32, x_rear: f32, z_rear: f32) {
    let kf = dkey("tow_strap_front", BodyRegion::FrontBumper, M::Accent);
    k.front_box(&kf, x - 0.028, x + 0.028, z - 0.05, z + 0.03, 0.05, 0.004);
    k.front_box(&kf, x - 0.028, x + 0.028, z - 0.05, z - 0.03, 0.09, 0.0);
    let kr = dkey("tow_strap_rear", BodyRegion::RearBumper, M::Accent);
    k.rear_box(
        &kr,
        x_rear - 0.028,
        x_rear + 0.028,
        z_rear - 0.05,
        z_rear + 0.03,
        0.05,
        0.004,
    );
    k.rear_box(
        &kr,
        x_rear - 0.028,
        x_rear + 0.028,
        z_rear - 0.05,
        z_rear - 0.03,
        0.09,
        0.0,
    );
}

/// Central rain light on the tail face: lens `w` × `h` from `z` up, on a dark housing, proud
/// enough to clear the diffuser fins.
fn rain_light(k: &mut Kit, z: f32, w: f32, h: f32, proud: f32) {
    let kh = dkey("rain_light_housing", BodyRegion::RearBumper, M::Trim);
    k.rear_box(
        &kh,
        -0.5 * w - 0.015,
        0.5 * w + 0.015,
        z - 0.015,
        z + h + 0.015,
        proud - 0.01,
        0.004,
    );
    let kl = lkey("rain_light", BodyRegion::RearBumper, M::Taillight);
    k.rear_box(&kl, -0.5 * w, 0.5 * w, z, z + h, proud, 0.0);
}

/// Blank door number panel (side `sx`) in the accent colour on the front door.
fn number_panel(k: &mut Kit, sx: f32, y0: f32, y1: f32, z0: f32, z1: f32) {
    let kp = dkey("number_panel_l", BodyRegion::LeftDoorF, M::Accent);
    k.side_patch(&kp, sx, y0, y1, z0, z1, 0.003, 0.003, (2, 2));
}

// ---------------------------------------------------------------------- morrigan_55

/// Front-engine GT endurance racer built from a coupe: long louvred hood, vented box flares,
/// huge carbon splitter with dive planes, swan-neck wing on big end plates, deep diffuser.
/// Yellow with black (carbon) accents and white number panels.
pub fn morrigan(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: -0.22,
        nose_h: Some(0.44),
        nose_rake_deg: 52.0,
        nose_plan_round: 0.30,
        tail_plan_round: 0.16,
        hood_crown: 0.010,
        hood_valley: 0.055,
        power_bulge_h: 0.035,
        hood_bulge_w: 0.36,
        windshield_rake_deg: Some(64.0),
        a_pillar_rake_deg: 12.0,
        c_pillar_rake_deg: 14.0,
        roof_arc: 0.75,
        roof_crown: 0.03,
        tumblehome: Some(0.27),
        glass_inset: 0.13,
        crease_z: 0.44,
        crease_depth: 0.020,
        shoulder_round: 0.08,
        haunch: 0.080,
        front_haunch: 0.075,
        flare: 0.050,
        flare_shape: ArchShape::Round,
        ride_dz: -0.03,
        stance_pitch_deg: 0.4,
        ducktail_h: 0.03,
        belt_kick: 0.05,
        belt_rake: 0.02,
        tail_round: 0.14,
        tail_drop: 0.03,
        a_pillar_w_m: 0.06,
        c_pillar_w_m: 0.16,
        b_pillar: TrimFinish::Body,
        a_sail_len_m: 0.20,
        windshield_wrap_m: 0.10,
        wipers: 1,
        door_handles: TrimFinish::None,
        fuel_door: FuelDoor::None,
        ..BodyStyle::preset(Silhouette::Fastback)
    });
    s.details.push(kit_detail(morrigan_kit));
    wheels(
        &mut s,
        def,
        RimKind::Monoblock,
        10,
        0.74,
        M::Trim,
        CapKind::CentreLock,
        (0.03, 0.010),
        (-1.5, -1.0),
    );
    // Engine, brakes and side pipes come from the spec table; only colours here.
    let m = &mut s.mechanical;
    m.show_engine_detail = true;
    m.caliper_rgb = Some(rgb(0xD21E1E));
    m.spring_rgb = Some(rgb(0x1C1C1C));
    s.tints.push((M::Paint, rgb(0xFFD100)));
    s.accent_rgb = Some(rgb(0xF4F4F0));
    s
}

fn morrigan_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;
    let (yf, _, _) = k.arch(true);
    let (yr, _, rr) = k.arch(false);

    // --- Nose: slim swept lamps on the corners, endurance lamp pod, big mesh mouth, ducts.
    // Long, slim lamps swept back along the fender peaks (signals: `signals_beside_lamps`).
    for sx in SIDES {
        k.top_lamp(sx, yn - 0.46, yn - 0.12, wn - 0.25, wn - 0.10, 0.022);
    }
    let kp = dkey("endurance_pod", fb, M::Trim);
    let (pz0, pz1) = (nz1 - 0.12, nz1 - 0.025);
    k.front_box(&kp, -0.24, 0.24, pz0, pz1, 0.015, 0.008);
    let kl = lkey("endurance_lamps", fb, M::Headlight);
    for x in [-0.12, 0.12] {
        k.round_lamp(&kl, true, x, 0.5 * (pz0 + pz1), 0.035, 0.03, None);
    }
    k.mesh_grille(
        "front_mouth",
        fb,
        0.50,
        nz0 + 0.06,
        pz0 - 0.03,
        3,
        10,
        M::Carbon,
    );
    for sx in SIDES {
        let kd = sided(&dkey("brake_duct_l", fb, M::Grille), sx);
        k.front_box(
            &kd,
            sx * 0.58,
            sx * (wn - 0.06),
            nz0 + 0.06,
            nz0 + 0.17,
            0.006,
            0.0,
        );
    }
    // Huge carbon splitter with fences, two tiers of dive planes on the bumper corners.
    k.splitter(M::Carbon, 0.16, 0.0, 0.026, true);
    k.canards(M::Carbon, nz0 + 0.14);
    k.canards(M::Carbon, nz0 + 0.25);

    // --- Hood: louvred extractors over the front wheel wells either side of the power bulge,
    // pins, carbon centre stripe; side exit vents behind the front wheels.
    k.louvres(
        BodyRegion::Hood,
        yf - 0.10,
        yf + 0.34,
        5,
        0.14,
        0.44,
        M::Carbon,
    );
    k.hood_pins(p.y_bf - 0.08, 0.55);
    k.top_stripe(
        "hood_stripe",
        BodyRegion::Hood,
        p.y_fw + 0.04,
        p.y_bf - 0.02,
        -0.05,
        0.05,
        M::Carbon,
    );
    for sx in SIDES {
        // Fender exit vent behind the front wheel: dark opening with three carbon blades.
        let (yf, zf, rf) = k.arch(true);
        let (v0, v1) = (yf - rf - 0.30, yf - rf - 0.06);
        let (vz0, vz1) = (zf + 0.02, p.belt(v1) - 0.06);
        let kv = dkey("fender_exit_vent_l", BodyRegion::FrontLeftFender, M::Grille);
        k.side_patch(&kv, sx, v0, v1, vz0, vz1, 0.002, 0.006, (2, 2));
        let kb = dkey(
            "fender_exit_blades_l",
            BodyRegion::FrontLeftFender,
            M::Carbon,
        );
        for i in 0..3 {
            let z = vz0 + (vz1 - vz0) * (i as f32 + 0.5) / 3.0;
            k.side_patch(
                &kb,
                sx,
                v0 - 0.01,
                v1,
                z - 0.01,
                z + 0.01,
                0.009,
                0.010,
                (2, 1),
            );
        }
    }

    // --- Roof: carbon air scoop at the roof front, windscreen sun strip.
    let ry = p.roof.1 - 0.04;
    let rz = k.top_z(ry, 0.0);
    let ks = dkey("roof_scoop", BodyRegion::Roof, M::Carbon);
    k.prism_x(
        &ks,
        &[
            [ry + 0.02, rz - 0.01],
            [ry - 0.26, rz - 0.01],
            [ry - 0.26, rz + 0.05],
            [ry - 0.04, rz + 0.06],
        ],
        -0.11,
        0.11,
    );
    let km = dkey("roof_scoop_mouth", BodyRegion::Roof, M::Grille);
    k.bx(
        &km,
        [-0.095, ry - 0.05, rz + 0.004],
        [0.095, ry - 0.035, rz + 0.048],
    );
    sun_strip(&mut k, 0.17, M::Trim);

    // --- Sides: number panels, sill pinstripe, side-pipe heat shields, quick-fill nozzles.
    let yd0 = p.y_fd_rear + 0.14;
    let yd1 = p.y_fd_hinge - 0.16;
    for sx in SIDES {
        number_panel(
            &mut k,
            sx,
            yd0,
            yd1,
            p.z_door_bottom + 0.07,
            p.z_belt - 0.05,
        );
        let kst = dkey("door_stripe_l", BodyRegion::LeftDoorF, M::Trim);
        k.side_patch(
            &kst,
            sx,
            p.y_fd_rear + 0.02,
            p.y_fd_hinge - 0.02,
            p.z_door_bottom + 0.015,
            p.z_door_bottom + 0.04,
            0.003,
            0.003,
            (2, 1),
        );
        // Exit shroud of the side pipes just ahead of the rear wheel.
        let ky = sided(
            &dkey("side_exit_shroud_l", BodyRegion::FrameLeft, M::Carbon),
            sx,
        );
        let ye = yr + rr + 0.16;
        let ze = p.sill(ye) + 0.10;
        k.side_box(&ky, sx, ye, ze, 0.30, 0.06, 0.03, 0.0);
        // Quick-fill fuel couplings in the rear quarter windows.
        let kn = sided(&dkey("fuel_coupling_l", BodyRegion::Cabin, M::Chrome), sx);
        let yq = 0.5 * (p.y_fd_rear + p.y_deck) + 0.05;
        let zq = p.z_belt + 0.12;
        let xq = k.side_x(yq, zq);
        k.b.cylinder(
            &kn,
            [sx * (xq - 0.02), yq, zq],
            [sx * (xq + 0.035), yq, zq],
            0.035,
            0.03,
            10,
            true,
        );
    }
    k.mirrors(0.16, 0.10, Some(M::Carbon));

    // --- Rear: swan-neck wing on big end plates, black fascia, twin-bar tails, diffuser.
    k.wing(Wing {
        lead: 0.34,
        chord: 0.30,
        height: 0.24,
        half_span: 0.90,
        thick: 0.035,
        aoa_deg: 9.0,
        stand: Stand::SwanNeck(0.34),
        endplates: Some((0.24, 0.42)),
        mat: M::Carbon,
        stand_mat: M::Carbon,
        gurney: 0.012,
    });
    let (fz0, fz1) = (tz0 + 0.18, tz1 - 0.16);
    k.rear_panel(
        "rear_fascia",
        BodyRegion::RearBumper,
        wt - 0.08,
        fz0,
        fz1,
        M::Grille,
    );
    let zt = tz1 - 0.07;
    for sx in SIDES {
        let tk = taillight_key(sx);
        let kh = sided(
            &dkey("taillight_housing_l", BodyRegion::RearLeftQuarter, M::Trim),
            sx,
        );
        k.rear_box(
            &kh,
            sx * 0.42,
            sx * (wt - 0.01),
            zt - 0.065,
            zt + 0.045,
            0.010,
            0.004,
        );
        // Two stacked bars and a vertical outer stroke.
        for dz in [0.0, -0.045] {
            k.rear_box(
                &tk,
                sx * 0.45,
                sx * (wt - 0.05),
                zt + dz,
                zt + dz + 0.018,
                0.022,
                0.0,
            );
        }
        k.rear_box(
            &tk,
            sx * (wt - 0.04),
            sx * (wt - 0.02),
            zt - 0.055,
            zt + 0.035,
            0.022,
            0.0,
        );
    }
    k.diffuser(M::Carbon, 0.80, 0.18, 7, 0.16);
    rain_light(&mut k, tz0 + 0.20, 0.08, 0.08, 0.05);
    tow_straps(&mut k, -0.34, nz0 + 0.10, 0.40, tz0 + 0.24);

    // Turn signals as segments beside the lamps (playtest row 136).
    k.signals_beside_lamps(0.06, M::EmissiveAmber);
}

// ---------------------------------------------------------------------- nyx_proto

/// Closed-cockpit endurance sports prototype: low and long, raised louvred fender domes,
/// raised centre nose, sidepods with radiator intakes, roof scoop + shark fin, full-width
/// twin-element wing, huge diffuser. Dark navy with a bright orange accent.
pub fn nyx(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: 0.30,
        nose_h: Some(0.40),
        nose_rake_deg: 30.0,
        nose_plan_round: 0.10,
        tail_plan_round: 0.04,
        hood_crown: 0.040,
        hood_valley: 0.060,
        windshield_rake_deg: Some(64.0),
        a_pillar_rake_deg: 22.0,
        c_pillar_rake_deg: 18.0,
        rear_glass_rake_deg: Some(74.0),
        roof_arc: 0.70,
        roof_len_trim: 0.25,
        tumblehome: Some(0.36),
        crease_z: 0.36,
        crease_depth: 0.030,
        haunch: 0.080,
        front_haunch: 0.080,
        flare: 0.030,
        ride_dz: -0.03,
        stance_pitch_deg: 0.5,
        ducktail_h: 0.0,
        deck_valley: 0.08,
        overhang_trim: (0.03, 0.08),
        a_pillar_w_m: 0.05,
        b_pillar: TrimFinish::Body,
        rear_glass_wrap_m: 0.0,
        wipers: 1,
        door_handles: TrimFinish::None,
        fuel_door: FuelDoor::None,
        belt_kick: 0.04,
        belt_rake: 0.02,
        tail_round: 0.02,
        tail_drop: 0.0,
        ..BodyStyle::preset(Silhouette::Wedge)
    });
    s.details.push(kit_detail(nyx_kit));
    wheels(
        &mut s,
        def,
        RimKind::Monoblock,
        9,
        0.74,
        M::Trim,
        CapKind::CentreLock,
        (0.03, 0.010),
        (-1.8, -1.2),
    );
    let m = &mut s.mechanical;
    m.show_engine_detail = true;
    m.caliper_rgb = Some(rgb(0xFF5A1F));
    m.spring_rgb = Some(rgb(0xFF5A1F));
    s.tints.push((M::Paint, rgb(0x1B1F3B)));
    s.accent_rgb = Some(rgb(0xFF5A1F));
    s
}

fn nyx_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, _nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;
    let (yf, _, rf) = k.arch(true);
    let (yr, _, rr) = k.arch(false);

    // --- Nose: raised centre nose (bumper + hood pieces), slim stacked lamps, intakes.
    let kn = dkey("centre_nose", fb, M::Paint);
    k.patch(
        &kn,
        p.y_bf,
        yn - 0.01,
        top_band(-0.22, 0.22),
        0.0,
        0.05,
        (3, 2),
    );
    let knh = dkey("centre_nose_hood", BodyRegion::Hood, M::Paint);
    k.patch(
        &knh,
        p.y_fw + 0.10,
        p.y_bf,
        top_band(-0.22, 0.22),
        0.0,
        0.05,
        (3, 2),
    );
    let kna = dkey("nose_stripe", fb, M::Accent);
    k.patch(
        &kna,
        yn - 0.30,
        yn - 0.01,
        top_band(-0.06, 0.06),
        0.052,
        0.0,
        (2, 0),
    );
    for sx in SIDES {
        k.top_lamp(sx, yn - 0.42, yn - 0.33, wn - 0.30, wn - 0.05, 0.02);
        k.top_lamp(sx, yn - 0.27, yn - 0.18, wn - 0.27, wn - 0.06, 0.02);
    }
    k.mesh_grille(
        "nose_intake",
        fb,
        0.24,
        nz0 + 0.05,
        nz0 + 0.17,
        2,
        6,
        M::Trim,
    );
    for sx in SIDES {
        let kd = sided(&dkey("nose_duct_l", fb, M::Grille), sx);
        k.front_box(
            &kd,
            sx * 0.34,
            sx * (wn - 0.08),
            nz0 + 0.05,
            nz0 + 0.14,
            0.006,
            0.0,
        );
    }
    k.splitter(M::Carbon, 0.18, 0.0, 0.024, true);
    k.canards(M::Carbon, nz0 + 0.16);

    // --- Raised fender domes over the front wheels with louvred tops; rear arch louvres.
    for sx in SIDES {
        let hw = p.half_width(yf);
        let kd = sided(
            &dkey("fender_dome_l", BodyRegion::FrontLeftFender, M::Paint),
            sx,
        );
        let (xa, xb) = (sx * 0.52, sx * (hw - 0.03));
        k.patch(
            &kd,
            yf - rf - 0.02,
            yf + rf + 0.06,
            top_band(xa, xb),
            0.0,
            0.06,
            (4, 3),
        );
        let kb = sided(
            &dkey("fender_dome_vent_l", BodyRegion::FrontLeftFender, M::Grille),
            sx,
        );
        let (xi, xo) = (0.60, hw - 0.12);
        let (va, vb) = (sx * xi, sx * xo);
        k.patch(
            &kb,
            yf - 0.14,
            yf + 0.20,
            top_band(va, vb),
            0.062,
            0.004,
            (2, 1),
        );
        let kl = sided(
            &dkey(
                "fender_dome_louvres_l",
                BodyRegion::FrontLeftFender,
                M::Carbon,
            ),
            sx,
        );
        for i in 0..5 {
            let y = yf - 0.14 + 0.34 * (i as f32 + 0.5) / 5.0;
            let z = k.top_z(y, sx * xi).max(k.top_z(y, sx * xo)) + 0.064;
            k.bx(&kl, [va, y - 0.012, z], [vb, y + 0.012, z + 0.016]);
        }
        let hr = p.half_width(yr);
        sided_vent(
            &mut k,
            "rear_arch_vent_l",
            BodyRegion::RearLeftQuarter,
            sx,
            yr - 0.12,
            yr + 0.22,
            hr - 0.36,
            hr - 0.10,
            4,
            M::Carbon,
        );
    }

    // --- Sidepods with big radiator intakes at their front; accent number panels.
    let (y0, y1) = (yr + rr + 0.06, yf - rf - 0.10);
    let zs = p.sill(0.5 * (y0 + y1));
    for sx in SIDES {
        let kpod = dkey("sidepod_l", BodyRegion::FrameLeft, M::Paint);
        k.side_patch(&kpod, sx, y0, y1, zs + 0.03, zs + 0.24, 0.0, 0.07, (4, 2));
        let xs = k.side_x(y1, zs + 0.14);
        let ki = sided(
            &dkey("sidepod_intake_l", BodyRegion::FrameLeft, M::Grille),
            sx,
        );
        k.bx(
            &ki,
            [sx * (xs - 0.01), y1 - 0.03, zs + 0.06],
            [sx * (xs + 0.068), y1 + 0.004, zs + 0.22],
        );
        let kv = sided(
            &dkey("sidepod_vane_l", BodyRegion::FrameLeft, M::Carbon),
            sx,
        );
        for dz in [0.10, 0.16] {
            k.bx(
                &kv,
                [sx * (xs - 0.005), y1 - 0.02, zs + dz],
                [sx * (xs + 0.072), y1 + 0.012, zs + dz + 0.012],
            );
        }
        number_panel(
            &mut k,
            sx,
            p.y_fd_rear + 0.12,
            p.y_fd_hinge - 0.12,
            zs + 0.28,
            p.z_belt - 0.03,
        );
    }

    // --- Engine cover: closed prototypes have no rear window; the rear glass is painted over
    // (glass region kept, so it still moves with the lattice) with louvres over the radiators.
    let (gc0, gc1) = (p.y_deck + 0.01, p.roof.0 - 0.02);
    if gc1 > gc0 + 0.2 {
        let hw = k.roof_edge(0.5 * (gc0 + gc1)) + 0.16;
        let kc = dkey("engine_cover_skin", BodyRegion::RearGlass, M::Paint);
        k.patch(&kc, gc0, gc1, top_band(-hw, hw), 0.0015, 0.0, (6, 6));
        for sx in SIDES {
            sided_vent(
                &mut k,
                "engine_cover_louvre_l",
                BodyRegion::RearGlass,
                sx,
                gc0 + 0.10,
                gc0 + 0.50,
                0.10,
                0.34,
                4,
                M::Carbon,
            );
        }
    }

    // --- Canopy: sun strip, roof engine-air scoop, shark fin down the engine cover.
    sun_strip(&mut k, 0.14, M::Accent);
    let ry = p.roof.0 + 0.10;
    let rz = k.top_z(ry, 0.0);
    let ys = ry - 0.42;
    let zg = k.top_z(ys, 0.0);
    let ks = dkey("roof_scoop", BodyRegion::Roof, M::Paint);
    k.prism_x(
        &ks,
        &[
            [ry, rz - 0.02],
            [ry, rz + 0.11],
            [ys, rz + 0.08],
            [ys, zg - 0.02],
        ],
        -0.10,
        0.10,
    );
    let km = dkey("roof_scoop_mouth", BodyRegion::Roof, M::Grille);
    k.bx(
        &km,
        [-0.085, ry - 0.02, rz + 0.01],
        [0.085, ry + 0.005, rz + 0.095],
    );
    // Fin: along the rear glass (roof piece), then along the deck to the wing (trunk piece).
    // Body colour, with an accent cap along the top edge.
    let kf0 = dkey("shark_fin_front", BodyRegion::Roof, M::Paint);
    let yd = p.y_deck;
    let zd = k.top_z(yd, 0.0);
    let zfd = zd + 0.20;
    k.prism_x(
        &kf0,
        &[
            [ys + 0.02, zg - 0.02],
            [ys + 0.02, rz + 0.06],
            [yd, zfd],
            [yd, zd - 0.01],
        ],
        -0.006,
        0.006,
    );
    let wing_lead = 0.30;
    let yw = k.yt + wing_lead - 0.10;
    let zw = k.top_z(yw, 0.0);
    let kf1 = dkey("shark_fin", BodyRegion::Trunk, M::Paint);
    let zfw = zw + 0.27;
    k.prism_x(
        &kf1,
        &[[yd, zd - 0.01], [yd, zfd], [yw, zfw], [yw, zw - 0.01]],
        -0.006,
        0.006,
    );
    let kfa = dkey("shark_fin_edge", BodyRegion::Trunk, M::Accent);
    for (a, c) in [([ys + 0.02, rz + 0.06], [yd, zfd]), ([yd, zfd], [yw, zfw])] {
        k.plate(
            &kfa,
            [0.0, a[0], a[1]],
            [0.0, c[0], c[1]],
            [1.0, 0.0, 0.0],
            0.016,
            0.03,
        );
    }
    k.mirrors(0.06, 0.09, Some(M::Accent));

    // --- Rear: full-width twin-element wing, open mesh tail, inverted-L lamps, diffuser.
    let main = Wing {
        lead: wing_lead,
        chord: 0.30,
        height: 0.30,
        half_span: 0.93,
        thick: 0.03,
        aoa_deg: 7.0,
        stand: Stand::Pedestal(0.30),
        endplates: Some((0.36, 0.56)),
        mat: M::Carbon,
        stand_mat: M::Carbon,
        gurney: 0.0,
    };
    k.wing(main);
    // Upper flap behind the main element's trailing edge, steeper.
    let y_le = k.yt + main.lead;
    let ym = y_le - 0.5 * main.chord;
    let zb = k.top_z(ym, 0.0) + main.height;
    let (sa, ca) = wp_math::sin_cos(26f32.to_radians());
    let fc = 0.15;
    let fy = y_le - 0.88 * main.chord - 0.5 * fc * ca;
    let fz = zb + 0.075 + 0.5 * fc * sa;
    let hs = main.half_span.min(p.half_w - 0.02);
    let kfl = dkey("wing_flap", BodyRegion::Trunk, M::Carbon);
    k.plate(&kfl, [-hs, fy, fz], [hs, fy, fz], [0.0, -ca, sa], fc, 0.014);
    let (mz0, mz1) = (tz0 + 0.26, tz1 - 0.06);
    k.rear_panel(
        "tail_mesh",
        BodyRegion::RearBumper,
        wt - 0.06,
        mz0,
        mz1,
        M::Grille,
    );
    let kmb = dkey("tail_mesh_bars", BodyRegion::RearBumper, M::Trim);
    for i in 1..4 {
        let z = mz0 + (mz1 - mz0) * i as f32 / 4.0;
        k.rear_box(
            &kmb,
            -wt + 0.10,
            wt - 0.10,
            z - 0.005,
            z + 0.005,
            0.016,
            0.0,
        );
    }
    for sx in SIDES {
        let tk = taillight_key(sx);
        // Top bar running inboard, then a tall blade down the outer edge.
        k.rear_box(
            &tk,
            sx * 0.40,
            sx * (wt - 0.02),
            mz1 - 0.006,
            mz1 + 0.016,
            0.026,
            0.0,
        );
        k.rear_box(
            &tk,
            sx * (wt - 0.045),
            sx * (wt - 0.02),
            mz0 + 0.02,
            mz1 + 0.016,
            0.026,
            0.0,
        );
    }
    k.diffuser(M::Carbon, 0.74, 0.24, 8, 0.24);
    rain_light(&mut k, tz0 + 0.06, 0.035, 0.16, 0.27);
    tow_straps(&mut k, 0.30, nz0 + 0.08, -0.48, mz0 + 0.03);

    // Turn signals as segments beside the lamps (playtest row 136).
    k.signals_beside_lamps(0.06, M::EmissiveAmber);
}

// ---------------------------------------------------------------------- merlo_20t

/// FWD touring-car racer built from a 4-door sedan: bolt-on box arch extensions, deep
/// splitter, big open grille mesh + lower intake, bonnet vents, skirts, single-element wing
/// on two pillars, diffuser, roof vent scoop. Cyan with white accents.
pub fn merlo(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        nose_h: Some(0.60),
        nose_rake_deg: 30.0,
        nose_plan_round: 0.28,
        tail_plan_round: 0.18,
        hood_crown: 0.020,
        power_bulge_h: 0.02,
        windshield_rake_deg: Some(61.0),
        a_pillar_rake_deg: 9.0,
        c_pillar_rake_deg: 7.0,
        rear_glass_rake_deg: Some(64.0),
        roof_arc: 0.55,
        roof_len_trim: -0.05,
        tumblehome: Some(0.19),
        crease_z: 0.58,
        crease_depth: 0.018,
        haunch: 0.040,
        front_haunch: 0.040,
        flare: 0.075,
        flare_shape: ArchShape::Box,
        ride_dz: -0.04,
        stance_pitch_deg: 0.3,
        ducktail_h: 0.02,
        belt_kick: 0.03,
        belt_rake: 0.03,
        tail_round: 0.10,
        tail_drop: 0.02,
        a_pillar_w_m: 0.07,
        c_pillar_w_m: 0.14,
        windshield_wrap_m: 0.08,
        wipers: 1,
        ..BodyStyle::preset(Silhouette::Notchback)
    });
    s.details.push(kit_detail(merlo_kit));
    wheels(
        &mut s,
        def,
        RimKind::Monoblock,
        6,
        0.76,
        M::Trim,
        CapKind::Centre,
        (0.025, 0.010),
        (-2.0, -1.0),
    );
    let m = &mut s.mechanical;
    m.show_engine_detail = true;
    m.caliper_rgb = Some(rgb(0xE8E8E8));
    m.spring_rgb = Some(rgb(0xF2F2F2));
    s.tints.push((M::Paint, rgb(0x00A6D6)));
    s.accent_rgb = Some(rgb(0xF6F6F6));
    s
}

fn merlo_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;
    let (yr, _, rr) = k.arch(false);

    // --- Front: slim angular lamps wrapping from the nose face onto the fender top (with the
    // signal as the outboard end of the face lens and an LED brow), big trapezoid grille mesh,
    // lower intake, deep splitter.
    for sx in SIDES {
        k.top_lamp(sx, yn - 0.17, yn - 0.04, wn - 0.34, wn - 0.07, 0.018);
        let kh = sided(
            &dkey(
                "headlamp_face_housing_l",
                BodyRegion::FrontLeftFender,
                M::Trim,
            ),
            sx,
        );
        k.front_box(
            &kh,
            sx * 0.48,
            sx * (wn - 0.03),
            nz1 - 0.10,
            nz1 - 0.01,
            0.012,
            0.006,
        );
        k.front_cluster(
            sx,
            0.52,
            wn - 0.06,
            nz1 - 0.08,
            nz1 - 0.035,
            0.02,
            0.0,
            Seg::Outboard(0.07),
        );
        let kd = sided(
            &lkey("drl_l", BodyRegion::FrontLeftFender, M::Headlight),
            sx,
        );
        k.front_box(
            &kd,
            sx * 0.50,
            sx * (wn - 0.05),
            nz1 - 0.095,
            nz1 - 0.085,
            0.026,
            0.0,
        );
    }
    let (gz0, gz1) = (nz0 + 0.22, nz1 - 0.03);
    k.mesh_grille("grille", fb, 0.40, gz0, gz1, 3, 9, M::Trim);
    k.grille(
        "lower_intake",
        fb,
        0.62,
        nz0 + 0.06,
        nz0 + 0.17,
        1,
        M::Trim,
        Some(M::Carbon),
    );
    k.splitter(M::Carbon, 0.14, 0.03, 0.03, true);
    k.canards(M::Carbon, nz0 + 0.20);
    for sx in SIDES {
        let kd = sided(&dkey("brake_duct_l", fb, M::Grille), sx);
        k.front_box(
            &kd,
            sx * 0.68,
            sx * (wn - 0.07),
            nz0 + 0.06,
            nz0 + 0.17,
            0.006,
            0.0,
        );
    }
    k.hood_pins(p.y_bf - 0.06, 0.60);
    // White centre stripe over bonnet and boot lid.
    k.top_stripe(
        "hood_stripe",
        BodyRegion::Hood,
        p.y_fw + 0.04,
        p.y_bf - 0.02,
        -0.09,
        0.09,
        M::Accent,
    );
    k.top_stripe(
        "trunk_stripe",
        BodyRegion::Trunk,
        k.yt + 0.08,
        p.y_deck - 0.04,
        -0.09,
        0.09,
        M::Accent,
    );
    k.fuel_filler(-1.0, 0.11, M::Trim);

    // --- Bonnet vents, roof vent scoop, sun strip.
    for sx in SIDES {
        let name = if sx < 0.0 {
            "hood_vent_l"
        } else {
            "hood_vent_r"
        };
        let (a, c) = (sx * 0.16, sx * 0.48);
        k.top_vent(
            name,
            BodyRegion::Hood,
            p.y_bf - 0.50,
            p.y_bf - 0.16,
            a.min(c),
            a.max(c),
            4,
            M::Trim,
        );
    }
    let ry = p.roof.1 - 0.10;
    let rz = k.top_z(ry, 0.0);
    let ks = dkey("roof_scoop", BodyRegion::Roof, M::Paint);
    k.prism_x(
        &ks,
        &[
            [ry + 0.02, rz - 0.01],
            [ry - 0.22, rz - 0.01],
            [ry - 0.22, rz + 0.04],
            [ry - 0.05, rz + 0.045],
        ],
        -0.12,
        0.12,
    );
    let km = dkey("roof_scoop_mouth", BodyRegion::Roof, M::Grille);
    k.bx(
        &km,
        [-0.105, ry - 0.06, rz + 0.004],
        [0.105, ry - 0.045, rz + 0.038],
    );
    sun_strip(&mut k, 0.16, M::Accent);

    // --- Sides: skirts, white number panels and rear-door band, arch-extension bolts.
    k.skirts(M::Carbon, 0.05, 0.08);
    for sx in SIDES {
        number_panel(
            &mut k,
            sx,
            p.y_fd_rear + 0.12,
            p.y_fd_hinge - 0.10,
            p.z_door_bottom + 0.12,
            p.z_belt - 0.06,
        );
        if let Some((hinge, rear)) = p.rear_door {
            let kb = dkey("door_band_l", BodyRegion::LeftDoorR, M::Accent);
            k.side_patch(
                &kb,
                sx,
                rear + 0.04,
                hinge - 0.04,
                p.z_door_bottom + 0.05,
                p.z_door_bottom + 0.10,
                0.003,
                0.003,
                (2, 1),
            );
        }
        // Heat shield above the side exhaust exit (right side only, spec `SideRight`).
        if sx > 0.0 {
            let ksh = dkey(
                "exhaust_heat_shield_r",
                BodyRegion::RearRightQuarter,
                M::Metal,
            );
            let ye = yr - rr - 0.16;
            k.side_box(&ksh, sx, ye, p.sill(ye) + 0.12, 0.26, 0.05, 0.012, 0.0);
        }
    }
    k.mirrors(0.10, 0.11, Some(M::Accent));
    k.door_handles(M::Trim);

    // --- Rear: single-element wing on two pillars, two-piece lamps, diffuser, rain light.
    k.wing(Wing {
        lead: 0.30,
        chord: 0.26,
        height: 0.24,
        half_span: 0.84,
        thick: 0.035,
        aoa_deg: 8.0,
        stand: Stand::Pedestal(0.46),
        endplates: Some((0.16, 0.32)),
        mat: M::Carbon,
        stand_mat: M::Carbon,
        gurney: 0.010,
    });
    // Slim two-piece tails (quarter + lid), joined by a full-width red light bar on the lid.
    let zt = tz1 - 0.07;
    for sx in SIDES {
        let tk = taillight_key(sx);
        let kh = sided(
            &dkey("taillight_housing_l", BodyRegion::RearLeftQuarter, M::Trim),
            sx,
        );
        k.rear_box(
            &kh,
            sx * 0.40,
            sx * (wt - 0.01),
            zt - 0.050,
            zt + 0.035,
            0.010,
            0.006,
        );
        // Outer lens with an inner hook (taller at the corner), signal beside it (kit call).
        k.rear_box(
            &tk,
            sx * 0.56,
            sx * (wt - 0.03),
            zt - 0.010,
            zt + 0.022,
            0.022,
            0.004,
        );
        k.rear_box(
            &tk,
            sx * (wt - 0.10),
            sx * (wt - 0.03),
            zt - 0.040,
            zt - 0.010,
            0.022,
            0.0,
        );
        let ki = sided(
            &lkey("taillight_inner_l", BodyRegion::Trunk, M::Taillight),
            sx,
        );
        k.rear_box(
            &ki,
            sx * 0.42,
            sx * 0.54,
            zt - 0.010,
            zt + 0.022,
            0.020,
            0.004,
        );
    }
    let kbar = dkey("tail_light_bar", BodyRegion::Trunk, M::Taillight);
    k.rear_box(&kbar, -0.40, 0.40, zt + 0.002, zt + 0.012, 0.018, 0.0);
    let kv = dkey("rear_valance", BodyRegion::RearBumper, M::Trim);
    k.rear_box(
        &kv,
        -wt + 0.10,
        wt - 0.10,
        tz0 + 0.02,
        tz0 + 0.20,
        0.010,
        0.004,
    );
    k.diffuser(M::Carbon, 0.62, 0.16, 5, 0.12);
    rain_light(&mut k, tz0 + 0.21, 0.16, 0.035, 0.05);
    tow_straps(&mut k, 0.38, nz0 + 0.12, -0.42, tz0 + 0.26);

    // Turn signals as segments beside the lamps (playtest row 136).
    k.signals_beside_lamps(0.06, M::EmissiveAmber);
}
