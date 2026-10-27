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
use crate::lamp::{clip_outline, lens, Lamp, LampView};
use crate::mesh::MaterialSlot as M;
use crate::mesh::INDICATOR_SEAM_M;
use crate::style::{
    ArchShape, BodyStyle, CapKind, CarStyle, DetailCtx, FuelDoor, RimKind, SideSculpt, Silhouette,
    TrimFinish,
};

use super::kit::{
    dkey, headlight_key, indicator_front_key, indicator_rear_key, lkey, sided, taillight_key,
    top_band, Kit, Seg, Stand, Wing, SIDES,
};
use super::{kit_detail, rgb, wheels};

// ---------------------------------------------------------------------- shared helpers

/// (front axle y, front wheel radius, rear axle y, rear wheel radius) from the car def, m.
fn axles(def: &VehicleDef) -> (f32, f32, f32, f32) {
    let mut f = (f32::MIN, 0.33);
    let mut r = (f32::MAX, 0.33);
    for w in &def.wheels {
        let y = w.position_m[1];
        if y > f.0 {
            f = (y, w.radius_m);
        }
        if y < r.0 {
            r = (y, w.radius_m);
        }
    }
    (f.0, f.1, r.0, r.1)
}

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

/// [`Kit::proj_cluster`] with a coarser lens grid (`step_m`, see [`crate::lamp::Lamp`]) for a
/// car at the edge of its triangle budget: the lamp lens and its turn-signal segment cut
/// across x (`Seg::Outboard` / `Seg::Inboard` only), no bezel. `outline` is the LEFT side's.
#[allow(clippy::too_many_arguments)]
fn lean_cluster(
    k: &mut Kit,
    sx: f32,
    front: bool,
    view: LampView,
    outline: &[[f32; 2]],
    seg: Seg,
    rear_mat: M,
    yaw_deg: f32,
    step_m: f32,
) {
    let right = sx > 0.0;
    let pts: Vec<[f32; 2]> = outline.iter().map(|q| [sx * q[0].abs(), q[1]]).collect();
    let mut lamp = Lamp::new(view, pts.clone());
    lamp.yaw_deg = yaw_deg;
    lamp.step_m = step_m;
    let (lamp_key, sig_key) = if front {
        (headlight_key(sx), indicator_front_key(sx))
    } else {
        (taillight_key(sx), indicator_rear_key(sx, rear_mat))
    };
    let lo = pts.iter().map(|q| q[0]).fold(f32::MAX, f32::min);
    let hi = pts.iter().map(|q| q[0]).fold(f32::MIN, f32::max);
    let outer = !matches!(seg, Seg::Inboard(_));
    let w = match seg {
        Seg::Outboard(w) | Seg::Inboard(w) | Seg::Lower(w) | Seg::Upper(w) => w,
    };
    // Signal on the high-x side of the cut when it is the right side's outer end or the left
    // side's inner end (as `proj_cluster`).
    let (at, sig_high) = if outer == right {
        (hi - w, true)
    } else {
        (lo + w, false)
    };
    let g = 0.5 * INDICATOR_SEAM_M;
    let (sig, rest) = if sig_high {
        (
            clip_outline(&pts, 0, at + g, false),
            clip_outline(&pts, 0, at - g, true),
        )
    } else {
        (
            clip_outline(&pts, 0, at - g, true),
            clip_outline(&pts, 0, at + g, false),
        )
    };
    for (key, o) in [(&lamp_key, rest), (&sig_key, sig)] {
        if o.len() >= 3 {
            let mut l = lamp.clone();
            l.outline = o;
            lens(k.b, k.p, key, &l);
        }
    }
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
    // Three rows up and 8 mm off: the bulged / sculpted door side bows out between rows.
    k.side_patch(&kp, sx, y0, y1, z0, z1, 0.008, 0.003, (2, 3));
}

// ---------------------------------------------------------------------- morrigan_55

/// Front-engine GT endurance racer built from a coupe: long louvred hood, vented box flares,
/// huge carbon splitter with dive planes, swan-neck wing on big end plates, deep diffuser.
/// Slate teal with lime accents and number panels, carbon aero (a fictional team's colours;
/// rule 9: no real team's livery).
pub fn morrigan(def: &VehicleDef) -> CarStyle {
    let (fy, fr, ry, rr) = axles(def);
    let mut s = CarStyle::with_body(BodyStyle {
        // Phase 3: narrow nose and tail, fender peaks above a low hood, round haunches standing
        // proud, a cove stepping in behind the front wheel (exit-vent area), curved section.
        plan_width: vec![
            [fy + fr + 0.05, 1.0],
            [fy + fr + 0.75, 0.90],
            [ry - rr - 0.05, 1.0],
            [ry - rr - 0.70, 0.93],
        ],
        front_fender_rise_m: 0.08,
        rear_fender_rise_m: 0.06,
        fender_peak_w: 0.65,
        // Raked nose face. Chins stay 0: in the smooth-shaded renderer a chin swept the face
        // back over the mouth grille and brake ducts (they vanished from low views).
        nose_face_rake_deg: 12.0,
        nose_chin_m: 0.0,
        tail_chin_m: 0.0,
        side_sculpt: vec![
            SideSculpt {
                y0_m: fy - fr - 0.85,
                y1_m: fy - fr - 0.05,
                z0_m: 0.10,
                z1_m: 0.42,
                dx_m: -0.045,
                falloff_m: 0.15,
            },
            SideSculpt {
                y0_m: ry - 0.35,
                y1_m: ry + 0.35,
                z0_m: 0.36,
                z1_m: 0.66,
                dx_m: 0.035,
                falloff_m: 0.20,
            },
        ],
        side_bulge_m: 0.025,
        rocker_in_m: 0.04,
        cabin_shift_m: -0.22,
        overhang_trim: (0.10, 0.06),
        roof_dz: -0.06,
        sill_tuck: 0.10,
        nose_h: Some(0.44),
        nose_rake_deg: 52.0,
        nose_plan_round: 0.30,
        tail_plan_round: 0.16,
        hood_crown: 0.010,
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
        shoulder_round: 0.11,
        haunch: 0.080,
        front_haunch: 0.080,
        flare: 0.070,
        flare_shape: ArchShape::Round,
        ride_dz: -0.05,
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
        6,
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
    s.tints.push((M::Paint, rgb(0x2F4F57)));
    s.accent_rgb = Some(rgb(0xB8E62A));
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

    // --- Nose: long swept lamps lying on the fender peaks (projected onto the surface, amber
    // signal as the outer tip), endurance lamp pod, big mesh mouth, ducts.
    for sx in SIDES {
        let lamp = [
            [-(wn - 0.05), yn - 0.10],
            [-(wn - 0.19), yn - 0.17],
            [-(wn - 0.17), yn - 0.50],
            [-(wn - 0.07), yn - 0.42],
        ];
        lean_cluster(
            &mut k,
            sx,
            true,
            LampView::Top,
            &lamp,
            Seg::Outboard(0.045),
            M::EmissiveAmber,
            0.0,
            0.07,
        );
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
        2,
        6,
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

    // --- Hood: extractors over the front wheel wells either side of the power bulge, carbon
    // centre stripe; side exit vents behind the front wheels.
    for sx in SIDES {
        sided_vent(
            &mut k,
            "hood_extractor_l",
            BodyRegion::Hood,
            sx,
            yf - 0.10,
            yf + 0.30,
            0.16,
            0.44,
            4,
            M::Carbon,
        );
    }
    let khs = dkey("hood_stripe", BodyRegion::Hood, M::Carbon);
    k.patch(
        &khs,
        p.y_fw + 0.04,
        p.y_bf - 0.02,
        top_band(-0.05, 0.05),
        0.008,
        0.0,
        (8, 1),
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
    }
    // Carbon sill pods between the wheels (the GT class's deep side skirts).
    k.skirts(M::Carbon, 0.08, 0.16);
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
    // Slim wrap-round tail blades (projected round the rear corners), amber inboard segment.
    for sx in SIDES {
        let tail = [
            [-(wt - 0.42), tz1 - 0.075],
            [-(wt + 0.02), tz1 - 0.085],
            [-(wt + 0.02), tz1 - 0.040],
            [-(wt - 0.44), tz1 - 0.045],
        ];
        lean_cluster(
            &mut k,
            sx,
            false,
            LampView::Rear,
            &tail,
            Seg::Inboard(0.09),
            M::EmissiveAmber,
            30.0,
            0.07,
        );
    }
    k.diffuser(M::Carbon, 0.80, 0.18, 5, 0.16);
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
    let (fy, fr, ry, rr) = axles(def);
    let mut s = CarStyle::with_body(BodyStyle {
        // Phase 3: wheel pods standing above a low nose and engine cover, narrow canopy,
        // sidepods stepping in behind the front wheels, nose and tail tapered in plan.
        front_fender_rise_m: 0.11,
        rear_fender_rise_m: 0.09,
        fender_peak_w: 0.68,
        nose_face_rake_deg: 15.0,
        nose_chin_m: 0.0,
        tail_chin_m: 0.0,
        tumblehome_m: Some(0.34),
        plan_width: vec![
            [fy + fr + 0.05, 1.0],
            [fy + fr + 0.80, 0.90],
            [ry - rr - 0.05, 1.0],
            [ry - rr - 0.60, 0.95],
        ],
        side_sculpt: vec![SideSculpt {
            y0_m: ry + rr + 0.10,
            y1_m: fy - fr - 0.15,
            z0_m: 0.0,
            z1_m: 0.42,
            dx_m: -0.07,
            falloff_m: 0.15,
        }],
        rocker_in_m: 0.03,
        cabin_shift_m: 0.30,
        nose_h: Some(0.40),
        nose_rake_deg: 30.0,
        nose_plan_round: 0.10,
        tail_plan_round: 0.04,
        hood_crown: 0.040,
        windshield_rake_deg: Some(64.0),
        a_pillar_rake_deg: 22.0,
        c_pillar_rake_deg: 18.0,
        rear_glass_rake_deg: Some(74.0),
        roof_arc: 0.70,
        roof_len_trim: 0.25,
        crease_z: 0.36,
        crease_depth: 0.030,
        haunch: 0.080,
        front_haunch: 0.080,
        flare: 0.050,
        sill_tuck: 0.10,
        shoulder_round: 0.10,
        roof_dz: -0.04,
        ride_dz: -0.05,
        stance_pitch_deg: 0.5,
        ducktail_h: 0.0,
        deck_valley: 0.08,
        overhang_trim: (0.07, 0.05),
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
        // Angular lamp unit lying on the fender pod (projected), a wide amber outer tip (a
        // narrow one sits in the hood's shadow from ahead).
        let lamp = [
            [-(wn - 0.03), yn - 0.12],
            [-(wn - 0.26), yn - 0.20],
            [-(wn - 0.24), yn - 0.40],
            [-(wn - 0.05), yn - 0.33],
        ];
        k.proj_cluster(
            sx,
            true,
            LampView::Top,
            &lamp,
            Some(Seg::Outboard(0.08)),
            M::EmissiveAmber,
            0.0,
            0.0,
        );
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

    // --- Louvred tops on the fender pods (the pods are `front_fender_rise_m`); rear arch
    // louvres.
    for sx in SIDES {
        let hw = p.half_width(yf);
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
            0.004,
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
            let z = k.top_z(y, sx * xi).max(k.top_z(y, sx * xo)) + 0.006;
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
    let (fy, fr, ry, _) = axles(def);
    let mut s = CarStyle::with_body(BodyStyle {
        // Phase 3: bolt-on arch extensions standing proud over both axles (the sedan's doors
        // read pinched between them), curved section, slightly tapered nose.
        plan_width: vec![[fy + fr + 0.05, 1.0], [fy + fr + 0.60, 0.95]],
        side_sculpt: vec![
            SideSculpt {
                y0_m: fy - 0.30,
                y1_m: fy + 0.30,
                z0_m: 0.30,
                z1_m: 0.74,
                dx_m: 0.035,
                falloff_m: 0.15,
            },
            SideSculpt {
                y0_m: ry - 0.30,
                y1_m: ry + 0.30,
                z0_m: 0.30,
                z1_m: 0.74,
                dx_m: 0.035,
                falloff_m: 0.15,
            },
        ],
        side_bulge_m: 0.02,
        rocker_in_m: 0.035,
        front_fender_rise_m: 0.02,
        nose_face_rake_deg: 12.0,
        nose_chin_m: 0.0,
        tail_chin_m: 0.0,
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
        haunch: 0.060,
        front_haunch: 0.060,
        flare: 0.080,
        flare_shape: ArchShape::Box,
        roof_dz: -0.04,
        sill_tuck: 0.09,
        shoulder_round: 0.09,
        overhang_trim: (0.06, 0.03),
        ride_dz: -0.05,
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
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;
    let (yr, _, rr) = k.arch(false);

    // --- Front: slim angular lamps drawn on the nose face and projected round the corner
    // onto the fender (amber outboard segment, black bezel), big grille mesh, lower intake,
    // deep splitter.
    for sx in SIDES {
        let head = [
            [-(wn - 0.34), nz1 - 0.115],
            [-(wn - 0.01), nz1 - 0.075],
            [-(wn - 0.01), nz1 - 0.030],
            [-(wn - 0.40), nz1 - 0.055],
        ];
        k.proj_cluster(
            sx,
            true,
            LampView::Front,
            &head,
            Some(Seg::Outboard(0.10)),
            M::EmissiveAmber,
            0.0,
            25.0,
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
    // Coarse grids (8 × 1 cells): the fine `top_stripe` grid costs ~270 triangles here.
    let khs = dkey("hood_stripe", BodyRegion::Hood, M::Accent);
    k.patch(
        &khs,
        p.y_fw + 0.04,
        p.y_bf - 0.02,
        top_band(-0.09, 0.09),
        0.008,
        0.0,
        (8, 1),
    );
    let kts = dkey("trunk_stripe", BodyRegion::Trunk, M::Accent);
    k.patch(
        &kts,
        k.yt + 0.08,
        p.y_deck - 0.04,
        top_band(-0.09, 0.09),
        0.008,
        0.0,
        (6, 1),
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
    // Slim tails wrapping round the rear corners (projected, amber inboard segment), joined
    // by a full-width red light bar on the lid.
    let zt = tz1 - 0.07;
    for sx in SIDES {
        let tail = [
            [-(wt - 0.40), zt - 0.035],
            [-(wt + 0.02), zt - 0.050],
            [-(wt + 0.02), zt + 0.020],
            [-(wt - 0.42), zt + 0.015],
        ];
        k.proj_cluster(
            sx,
            false,
            LampView::Rear,
            &tail,
            Some(Seg::Inboard(0.09)),
            M::EmissiveAmber,
            0.0,
            30.0,
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
