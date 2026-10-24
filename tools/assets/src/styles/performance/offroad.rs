//! Wave-3 off-road: `mesquite_pr` (2010s full-size pre-runner pickup) and `erg_hamada`
//! (2020s desert rally-raid prototype).
//!
//! Desert-racing vocabulary: tall stance on fat all-terrains, wide fibreglass flares that clear
//! long-travel suspension, tube bumpers / skid plates, light bars, roof scoops, spare wheels.
//! Both are hero cars (the performance module claims them), so they get the hero budget even
//! though their class is `Truck`. Mechanical parts come from `mechanical::spec` (table); the
//! styles here only add spring / caliper colours on top of it.
//!
//! The mechanical tyre lugs already cost most of the hero budget, so the kits are built from
//! cheap pieces: boxes, plates, 6-sided uncapped tubes and low-segment lathes. Stripes are
//! plain accent-colour patches (no text, numbers or marks).

use wp_contracts::{BodyRegion, VehicleDef};

use crate::builder::{Builder, PartKey};
use crate::mesh::MaterialSlot as M;
use crate::style::{
    ArchShape, BodyStyle, CapKind, CarStyle, DetailCtx, HoodScoop, RimKind, Silhouette,
};

use super::kit::{
    dkey, headlight_key, indicator_front_key, indicator_rear_key, lkey, sided, taillight_key,
    top_band, Kit, SIDES,
};
use super::{kit_detail, rgb, wheels};

/// Radius of the kit's tube work (bumpers, rack, frames), m.
const TUBE_R: f32 = 0.028;

/// Straight tube from `a` to `c` (5 sides — the pickup's mid LOD has to stay under its 6.5k
/// budget; open ends: the ends always meet another tube or a panel, so caps would be hidden
/// triangles).
fn tube(k: &mut Kit, key: &PartKey, a: [f32; 3], c: [f32; 3], r: f32) {
    k.b.cylinder(key, a, c, r, r, 5, false);
}

/// Flat colour stripe on the top surface (y0..y1 × x0..x1) with an explicit grid `steps`
/// (along y, across): `Kit::top_stripe` picks ~6 cm cells, which is costly on a long roof.
fn stripe(k: &mut Kit, key: &PartKey, y0: f32, y1: f32, x0: f32, x1: f32, steps: (usize, usize)) {
    k.patch(key, y0, y1, top_band(x0, x1), 0.008, 0.0, steps);
}

/// Spare wheel centred at `c` with its axle along `axis` (unit): a bevelled tyre lathe
/// (`r` outer radius, `hw` half width) plus a rim face and hub on the `+axis` side.
fn spare_wheel(
    k: &mut Kit,
    tire: &PartKey,
    rim: &PartKey,
    c: [f32; 3],
    axis: [f32; 3],
    r: f32,
    hw: f32,
) {
    let r_in = 0.62 * r;
    // Square-shouldered profile, 10 segments (10 / 6 on the rim and hub): the spares sit
    // inside racks and bays, and the pickup's mid LOD has to stay under its 6.5k budget.
    let prof = [[r_in, -hw], [r, -hw + 0.03], [r, hw - 0.03], [r_in, hw]];
    k.b.lathe(tire, c, axis, &prof, 10);
    let at = |d: f32| [c[0] + axis[0] * d, c[1] + axis[1] * d, c[2] + axis[2] * d];
    // Rim face fills the tyre's hole; the hub stands proud of it.
    k.b.cylinder(
        rim,
        at(-hw + 0.03),
        at(hw - 0.025),
        r_in + 0.005,
        r_in + 0.005,
        10,
        true,
    );
    k.b.cylinder(
        rim,
        at(hw - 0.03),
        at(hw + 0.02),
        0.16 * r,
        0.12 * r,
        6,
        true,
    );
}

/// Bolt-on box flare around one arch (side `sx`): a top band above the arch and two raked legs
/// down to the arch's front and rear edges, `out` m proud of the body side at `x_side`.
fn box_flare(k: &mut Kit, key_l: &PartKey, sx: f32, arch: (f32, f32, f32), x_side: f32, out: f32) {
    let (y, z, r) = arch;
    let key = sided(key_l, sx);
    let x = sx * (x_side + 0.5 * out - 0.02);
    let zt = z + r + 0.08;
    let ya = y + 0.55 * r;
    let yb = y - 0.55 * r;
    let across = [1.0, 0.0, 0.0];
    let band = 0.13;
    // Top band, then the front leg and the rear leg (raked down and outward along y).
    k.plate(
        &key,
        [x, ya + 0.04, zt],
        [x, yb - 0.04, zt],
        across,
        out,
        band,
    );
    k.plate(
        &key,
        [x, ya, zt],
        [x, y + r + 0.05, z - 0.02],
        across,
        out,
        band,
    );
    k.plate(
        &key,
        [x, yb, zt],
        [x, y - r - 0.05, z - 0.02],
        across,
        out,
        band,
    );
}

/// Pre-runner pickup: crew cab, short bed, bolt-on fibreglass fenders, tube bumpers. Sand paint
/// with black accents and signal-orange hardware (tow hooks, reservoir caps, pinstripe).
pub fn mesquite(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        nose_h: Some(1.05),
        hood_crown: 0.020,
        hood_scoop: HoodScoop::Scoop,
        hood_scoop_h: 0.07,
        windshield_rake_deg: Some(46.0),
        tumblehome: Some(0.10),
        haunch: 0.020,
        front_haunch: 0.020,
        flare: 0.080,
        flare_shape: ArchShape::Box,
        ride_dz: 0.04,
        stance_pitch_deg: -0.3,
        ..BodyStyle::preset(Silhouette::Pickup)
    });
    s.details.push(kit_detail(mesquite_kit));
    wheels(
        &mut s,
        def,
        RimKind::Truck,
        6,
        0.40,
        M::Metal,
        CapKind::Centre,
        (0.04, 0.015),
        (0.0, 0.0),
    );
    let m = &mut s.mechanical;
    m.caliper_rgb = Some(rgb(0x1E1E20));
    m.spring_rgb = Some(rgb(0xF26A1B));
    s.tints.push((M::Paint, rgb(0xC9B79C)));
    s.accent_rgb = Some(rgb(0xF26A1B));
    s
}

/// Mesquite kit: square lamps + big mesh grille, tube bumper with skid plate, LED bar and
/// orange tow hooks, vented fenders and hood, roof bar + A-pillar pods, rock sliders, bolt-on
/// flares, chase rack with an upright spare, bypass reservoirs in the bed, exo rear bumper,
/// tall three-cell tails + round bumper pods, rear mud flaps, whip antenna.
fn mesquite_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let yt = k.yt;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;
    let ff = BodyRegion::FrameFront;
    let fr = BodyRegion::FrameRear;

    // --- Nose: square head lamps in black housings, LED strip under each, big mesh grille.
    let (lz0, lz1) = (nz1 - 0.21, nz1 - 0.05);
    for sx in SIDES {
        let kh = sided(
            &dkey("headlamp_housing_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        k.front_box(
            &kh,
            sx * 0.54,
            sx * (wn - 0.03),
            lz0 - 0.02,
            lz1 + 0.02,
            0.010,
            0.0,
        );
        // Square lamp with its amber turn signal as the inboard segment of the same lens.
        let hk = headlight_key(sx);
        k.front_box(
            &hk,
            sx * 0.64,
            sx * (wn - 0.06),
            lz0 + 0.03,
            lz1,
            0.020,
            0.0,
        );
        let ik = indicator_front_key(sx);
        k.front_box(&ik, sx * 0.57, sx * 0.63, lz0 + 0.03, lz1, 0.020, 0.0);
        let kd = sided(
            &lkey("drl_l", BodyRegion::FrontLeftFender, M::Headlight),
            sx,
        );
        k.front_box(
            &kd,
            sx * 0.57,
            sx * (wn - 0.06),
            lz0 - 0.005,
            lz0 + 0.012,
            0.022,
            0.0,
        );
    }
    let (gz0, gz1) = (nz1 - 0.33, nz1 - 0.03);
    k.mesh_grille("grille", fb, 0.50, gz0, gz1, 3, 6, M::Trim);
    let kgf = dkey("grille_frame", fb, M::Trim);
    k.front_box(&kgf, -0.53, 0.53, gz1, gz1 + 0.03, 0.018, 0.0);
    k.front_box(&kgf, -0.53, 0.53, gz0 - 0.03, gz0, 0.018, 0.0);
    // Black bumper fascia below the grille (the stock bumper is gone; tubes sit in front).
    let kbf = dkey("bumper_fascia", fb, M::Trim);
    k.front_box(
        &kbf,
        -(wn - 0.03),
        wn - 0.03,
        nz0 + 0.02,
        gz0 - 0.04,
        0.008,
        0.0,
    );

    // --- Tube front bumper (frame hardware): two rails with bent-back ends, four uprights,
    // integrated LED bar between the rails, skid plate and orange tow hooks.
    let kt = dkey("front_tube_bumper", ff, M::Trim);
    let yb = yn + 0.11;
    let (zl, zu) = (nz0 + 0.08, gz0 - 0.08);
    let hb = wn - 0.08;
    for z in [zl, zu] {
        tube(&mut k, &kt, [-hb, yb, z], [hb, yb, z], TUBE_R);
        for sx in SIDES {
            tube(
                &mut k,
                &kt,
                [sx * hb, yb, z],
                [sx * (hb + 0.06), yn - 0.14, z],
                TUBE_R,
            );
        }
    }
    for x in [-0.62, -0.26, 0.26, 0.62] {
        tube(&mut k, &kt, [x, yb, zl - 0.03], [x, yb, zu + 0.03], TUBE_R);
    }
    let zm = 0.5 * (zl + zu);
    let kbh = dkey("bumper_lightbar_housing", ff, M::Trim);
    k.bx(
        &kbh,
        [-0.58, yb - 0.05, zm - 0.05],
        [0.58, yb + 0.03, zm + 0.05],
    );
    // Detail kind, not `Light`: a `Light` part on a frame region counts as one of the car's
    // four head / tail lenses (`skin::is_lens`).
    let kbl = dkey("bumper_lightbar", ff, M::Headlight);
    k.bx(
        &kbl,
        [-0.55, yb + 0.03, zm - 0.032],
        [0.55, yb + 0.042, zm + 0.032],
    );
    let kdv = dkey("bumper_lightbar_dividers", ff, M::Trim);
    for i in 1..6 {
        let x = -0.55 + 1.10 * i as f32 / 6.0;
        k.bx(
            &kdv,
            [x - 0.005, yb + 0.03, zm - 0.034],
            [x + 0.005, yb + 0.046, zm + 0.034],
        );
    }
    let ksk = dkey("front_skid_plate", ff, M::Metal);
    k.plate(
        &ksk,
        [0.0, yb + 0.02, zl - 0.02],
        [0.0, yn - 0.70, nz0 - 0.08],
        [1.0, 0.0, 0.0],
        1.10,
        0.012,
    );
    for sx in SIDES {
        let kh = sided(&dkey("tow_hook_front_l", ff, M::Accent), sx);
        k.b.torus(
            &kh,
            [sx * 0.44, yb + 0.05, zl - 0.02],
            [1.0, 0.0, 0.0],
            0.045,
            0.013,
            6,
            3,
        );
    }

    // --- Hood: twin vents either side of the scoop, black stripes beside them.
    let hood = BodyRegion::Hood;
    let span = yn - p.y_fw;
    for sx in SIDES {
        let name = if sx < 0.0 {
            "hood_vent_l"
        } else {
            "hood_vent_r"
        };
        let (a, c) = ((sx * 0.36).min(sx * 0.62), (sx * 0.36).max(sx * 0.62));
        k.top_vent(
            name,
            hood,
            p.y_fw + 0.22 * span,
            p.y_fw + 0.48 * span,
            a,
            c,
            4,
            M::Trim,
        );
        let ks = sided(&dkey("hood_stripe_l", hood, M::Trim), sx);
        stripe(
            &mut k,
            &ks,
            p.y_fw + 0.04,
            yn - 0.10,
            sx * 0.25,
            sx * 0.31,
            (14, 2),
        );
    }

    // --- Vented bolt-on fenders: black box flares on all four arches, louvred fender tops.
    let af = k.arch(true);
    let ar = k.arch(false);
    let xf = p.half_width(af.0) + p.style.flare.min(0.08);
    let xr = p.half_width(ar.0) + p.style.flare.min(0.08);
    for sx in SIDES {
        let kf = dkey("fender_flare_front_l", BodyRegion::FrontLeftFender, M::Trim);
        box_flare(&mut k, &kf, sx, af, xf, 0.09);
        let kr = dkey("fender_flare_rear_l", BodyRegion::RearLeftQuarter, M::Trim);
        box_flare(&mut k, &kr, sx, ar, xr, 0.10);
        let name = if sx < 0.0 {
            "fender_vent_l"
        } else {
            "fender_vent_r"
        };
        let xo = p.half_width(af.0) - 0.10;
        let (a, c) = (
            (sx * (xo - 0.20)).min(sx * xo),
            (sx * (xo - 0.20)).max(sx * xo),
        );
        k.top_vent(
            name,
            BodyRegion::FrontLeftFender,
            af.0 - 0.10,
            af.0 + 0.28,
            a,
            c,
            4,
            M::Trim,
        );
    }

    // --- Lights up top: LED bar over the windshield header, square pods on the A-pillar feet.
    let (roof_rear, roof_front, roof_z) = (p.roof.0, p.roof.1, p.roof.2);
    let yr0 = roof_front - 0.14;
    let zr = k.top_z(yr0, 0.0).max(roof_z);
    let half_bar = p.roof_half_w(yr0).min(0.90);
    let krb = dkey("roof_lightbar_housing", BodyRegion::Roof, M::Trim);
    for sx in SIDES {
        let x = sx * (half_bar - 0.10);
        k.bx(
            &krb,
            [x - 0.04, yr0 - 0.08, zr - 0.01],
            [x + 0.04, yr0 + 0.04, zr + 0.07],
        );
    }
    k.bbx(
        &krb,
        [-half_bar, yr0 - 0.02, zr + 0.06],
        [half_bar, yr0 + 0.10, zr + 0.16],
        0.0,
    );
    let krl = lkey("roof_lightbar", BodyRegion::Roof, M::Headlight);
    k.bx(
        &krl,
        [-half_bar + 0.03, yr0 + 0.10, zr + 0.075],
        [half_bar - 0.03, yr0 + 0.112, zr + 0.145],
    );
    for sx in SIDES {
        let y = p.y_fw - 0.02;
        let z = p.belt(y) + 0.03;
        let x = sx * (p.half_width(y) - 0.13);
        let kp = sided(
            &dkey("pillar_pod_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        k.bbx(
            &kp,
            [x - 0.07, y - 0.08, z],
            [x + 0.07, y + 0.04, z + 0.12],
            0.0,
        );
        let kl = sided(
            &lkey(
                "pillar_pod_lens_l",
                BodyRegion::FrontLeftFender,
                M::Headlight,
            ),
            sx,
        );
        k.bx(
            &kl,
            [x - 0.055, y + 0.04, z + 0.015],
            [x + 0.055, y + 0.052, z + 0.105],
        );
    }

    // --- Rock sliders between the arches on three legs each.
    let (y0, y1) = (ar.0 + ar.2 + 0.10, af.0 - af.2 - 0.10);
    for sx in SIDES {
        let ks = sided(&dkey("rock_slider_l", BodyRegion::FrameLeft, M::Trim), sx);
        let ym = 0.5 * (y0 + y1);
        let zs = p.sill(ym) + 0.03;
        let xs = sx * (k.side_x(ym, zs + 0.10) + 0.07);
        tube(&mut k, &ks, [xs, y0, zs], [xs, y1, zs], 0.038);
        for y in [y0 + 0.15, ym, y1 - 0.15] {
            let xi = sx * (k.side_x(y, zs + 0.10) - 0.10);
            k.bx(&ks, [xi, y - 0.03, zs - 0.03], [xs, y + 0.03, zs + 0.02]);
        }
    }

    // --- Bed: chase rack hoop behind the cab, diagonal stays to the rear corners, an upright
    // spare on a mount plate, bypass-shock towers and reservoirs over the rear arches.
    let (bed0, bed1, floor) = p.bed.unwrap_or((yt + 0.10, p.y_deck, p.z_belt - 0.45));
    let rail = p.belt(0.5 * (bed0 + bed1));
    let kr = dkey("chase_rack", fr, M::Trim);
    let yh = bed1 - 0.12;
    let xh = (p.half_width(yh) - 0.17).min(0.92);
    let zh = roof_z + 0.02;
    for sx in SIDES {
        let x = sx * xh;
        tube(&mut k, &kr, [x, yh, rail - 0.02], [x, yh, zh], 0.032);
        tube(
            &mut k,
            &kr,
            [x, yh, zh],
            [x, bed0 + 0.55, rail + 0.01],
            0.030,
        );
        tube(
            &mut k,
            &kr,
            [x, yh, rail + 0.30],
            [x, bed0 + 0.55, rail + 0.01],
            TUBE_R,
        );
        // Gusset from the hoop top down to the cab roof edge.
        tube(
            &mut k,
            &kr,
            [x, yh, zh],
            [sx * (xh - 0.10), roof_rear + 0.08, roof_z],
            TUBE_R,
        );
    }
    tube(&mut k, &kr, [-xh, yh, zh], [xh, yh, zh], 0.032);
    tube(
        &mut k,
        &kr,
        [-xh, yh, rail + 0.30],
        [xh, yh, rail + 0.30],
        TUBE_R,
    );
    tube(&mut k, &kr, [-xh, yh, rail + 0.30], [xh, yh, zh], TUBE_R);
    let r_sp = ctx.def.wheels.first().map(|w| w.radius_m).unwrap_or(0.44);
    let ys = yh - 0.30;
    let kmp = dkey("spare_mount", fr, M::Trim);
    k.bx(
        &kmp,
        [-0.10, yh - 0.06, floor],
        [0.10, yh + 0.02, floor + r_sp + 0.08],
    );
    let ktr = dkey("spare_tire", fr, M::Rubber);
    let krm = dkey("spare_rim", fr, M::Metal);
    spare_wheel(
        &mut k,
        &ktr,
        &krm,
        [0.0, ys, floor + r_sp + 0.01],
        [0.0, -1.0, 0.0],
        r_sp,
        0.15,
    );
    let kst = dkey("spare_strap", fr, M::Accent);
    k.bx(
        &kst,
        [-0.03, ys - 0.17, floor],
        [0.03, ys + 0.17, floor + 2.0 * r_sp + 0.03],
    );
    for sx in SIDES {
        let kb = sided(&dkey("bypass_shock_l", fr, M::Metal), sx);
        let kc = sided(&dkey("bypass_cap_l", fr, M::Accent), sx);
        let x = sx * (p.half_width(ar.0) - 0.36);
        let z0 = ar.1 + ar.2 + 0.06;
        for (i, dy) in [0.16f32, -0.02, -0.20].into_iter().enumerate() {
            let r = if i == 1 { 0.045 } else { 0.032 };
            let zt = rail + 0.18 - 0.04 * i as f32;
            k.b.cylinder(&kb, [x, ar.0 + dy, z0], [x, ar.0 + dy, zt], r, r, 6, false);
            k.b.cylinder(
                &kc,
                [x, ar.0 + dy, zt],
                [x, ar.0 + dy, zt + 0.03],
                r + 0.006,
                r + 0.006,
                5,
                true,
            );
        }
        let ktw = sided(&dkey("shock_tower_l", fr, M::Trim), sx);
        k.bx(
            &ktw,
            [x - sx * 0.07, ar.0 - 0.30, rail + 0.06],
            [x + sx * 0.07, ar.0 + 0.26, rail + 0.10],
        );
    }

    // --- Full-length side stripes under the belt: a black band over an orange pinstripe, one
    // segment per panel so each follows its own panel in a crash.
    let zb = p.z_belt;
    for sx in SIDES {
        for (tag, region, a, c) in [
            (
                "fender",
                BodyRegion::FrontLeftFender,
                p.y_fd_hinge + 0.02,
                yn - 0.22,
            ),
            (
                "door",
                BodyRegion::LeftDoorF,
                p.y_fd_rear + 0.02,
                p.y_fd_hinge - 0.02,
            ),
            (
                "quarter",
                BodyRegion::RearLeftQuarter,
                yt + 0.12,
                p.y_fd_rear - 0.02,
            ),
        ] {
            let kb = dkey(&format!("side_band_{tag}_l"), region, M::Trim);
            k.side_patch(&kb, sx, a, c, zb - 0.20, zb - 0.12, 0.004, 0.0, (4, 1));
            let kp = dkey(&format!("pinstripe_{tag}_l"), region, M::Accent);
            k.side_patch(&kp, sx, a, c, zb - 0.235, zb - 0.215, 0.004, 0.0, (4, 1));
        }
    }

    // --- Rear: exo tube bumper with skid plate, tall three-cell tails on the bed corners, round
    // red pods on the exo, tailgate handle, rear mud flaps, whip antenna on the rack.
    let ke = dkey("rear_exo_bumper", fr, M::Trim);
    let ye = yt - 0.12;
    let (el, eu) = (tz0 + 0.08, tz0 + 0.36);
    let he = wt - 0.06;
    for z in [el, eu] {
        tube(&mut k, &ke, [-he, ye, z], [he, ye, z], TUBE_R + 0.004);
    }
    for sx in SIDES {
        tube(
            &mut k,
            &ke,
            [sx * he, ye, el - 0.03],
            [sx * he, ye, eu + 0.03],
            TUBE_R,
        );
        tube(
            &mut k,
            &ke,
            [sx * he, ye, eu],
            [sx * (wt - 0.36), yt + 0.20, tz1 - 0.02],
            TUBE_R,
        );
        tube(
            &mut k,
            &ke,
            [sx * 0.30, ye, el],
            [sx * 0.30, ye, eu],
            TUBE_R,
        );
        tube(
            &mut k,
            &ke,
            [sx * 0.30, ye, el],
            [sx * 0.30, yt + 0.25, el],
            TUBE_R,
        );
    }
    let krs = dkey("rear_skid_plate", fr, M::Metal);
    k.plate(
        &krs,
        [0.0, ye, el - 0.03],
        [0.0, yt + 0.55, tz0 - 0.06],
        [1.0, 0.0, 0.0],
        1.0,
        0.012,
    );
    for sx in SIDES {
        let kh = sided(&dkey("tow_hook_rear_l", fr, M::Accent), sx);
        k.b.torus(
            &kh,
            [sx * 0.15, ye - 0.03, el],
            [1.0, 0.0, 0.0],
            0.04,
            0.012,
            6,
            3,
        );
        // Tall tail: black housing, three red cells (the top one amber-ringed).
        let tk = taillight_key(sx);
        let kth = sided(
            &dkey("taillight_housing_l", BodyRegion::RearLeftQuarter, M::Trim),
            sx,
        );
        let (x0, x1) = (sx * (wt - 0.14), sx * (wt - 0.02));
        let (t0, t1) = (tz1 - 0.46, tz1 - 0.04);
        k.rear_box(&kth, x0, x1, t0, t1, 0.012, 0.0);
        let h = (t1 - t0 - 0.04) / 3.0;
        for i in 0..3 {
            let z = t0 + 0.02 + h * i as f32;
            let key = if i == 2 {
                indicator_rear_key(sx, M::EmissiveAmber)
            } else {
                tk.clone()
            };
            k.rear_box(
                &key,
                x0 + sx * 0.015,
                x1 - sx * 0.015,
                z + 0.008,
                z + h - 0.008,
                0.026,
                0.0,
            );
        }
        // Round pod on the exo's upper rail.
        let kp = sided(&dkey("exo_lamp_pod_l", fr, M::Trim), sx);
        let x = sx * (he - 0.20);
        k.b.cylinder(
            &kp,
            [x, ye + 0.03, eu + 0.07],
            [x, ye - 0.04, eu + 0.07],
            0.06,
            0.06,
            10,
            true,
        );
        // Detail kind: only the corner tails are the car's tail-lens parts.
        let kpl = sided(&dkey("exo_lamp_l", fr, M::Taillight), sx);
        k.b.cylinder(
            &kpl,
            [x, ye - 0.04, eu + 0.07],
            [x, ye - 0.05, eu + 0.07],
            0.048,
            0.048,
            10,
            true,
        );
        // Rear mud flap behind the rear wheel.
        let kmf = sided(
            &dkey("mud_flap_rear_l", BodyRegion::RearLeftQuarter, M::Rubber),
            sx,
        );
        let yf = ar.0 - ar.2 - 0.04;
        let xo = xr - 0.02;
        k.bx(
            &kmf,
            [sx * (xo - 0.30), yf - 0.006, ar.1 - 0.26],
            [sx * xo, yf + 0.006, ar.1 + 0.10],
        );
    }
    let kg = dkey("tailgate_handle", BodyRegion::Trunk, M::Trim);
    k.rear_box(&kg, -0.14, 0.14, tz1 - 0.12, tz1 - 0.07, 0.016, 0.0);
    // Whip on the left rear bed corner (kept under the validator's height limit).
    let kw = dkey("whip_antenna", fr, M::Trim);
    let (yw, xw) = (yt + 0.18, -(p.half_width(yt + 0.18) - 0.10));
    let zw = p.belt(yw);
    k.b.cylinder(
        &kw,
        [xw, yw, zw - 0.02],
        [xw, yw - 0.10, zw + 0.98],
        0.006,
        0.003,
        5,
        false,
    );
    let kwt = dkey("whip_antenna_tip", fr, M::Accent);
    let tip0 = [xw, yw - 0.10, zw + 0.98];
    k.b.cylinder(
        &kwt,
        tip0,
        [xw, yw - 0.105, zw + 1.03],
        0.012,
        0.012,
        6,
        true,
    );

    k.mirrors(0.16, 0.13, Some(M::Trim));
    k.door_handles(M::Trim);
    k.wipers();

    // Turn signals as segments beside the lamps (playtest row 136).
    k.signals_beside_lamps(0.06, M::EmissiveAmber);
}

/// Rally-raid prototype: tube-frame buggy body, short nose, roof scoop, spare wheels in the tail.
/// Signal orange-red with navy accents.
pub fn erg(def: &VehicleDef) -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: 0.15,
        nose_h: Some(0.95),
        nose_rake_deg: 35.0,
        hood_crown: 0.040,
        windshield_rake_deg: Some(60.0),
        tumblehome: Some(0.18),
        haunch: 0.030,
        front_haunch: 0.030,
        flare: 0.080,
        flare_shape: ArchShape::Box,
        ride_dz: 0.03,
        stance_pitch_deg: -0.2,
        ..BodyStyle::preset(Silhouette::Suv)
    });
    s.details.push(kit_detail(erg_kit));
    wheels(
        &mut s,
        def,
        RimKind::Steel,
        6,
        0.38,
        M::Trim,
        CapKind::Centre,
        (0.045, 0.015),
        (0.0, 0.0),
    );
    let m = &mut s.mechanical;
    m.caliper_rgb = Some(rgb(0xF2C200));
    m.spring_rgb = Some(rgb(0xF2C200));
    s.tints.push((M::Paint, rgb(0xE4572E)));
    s.accent_rgb = Some(rgb(0x1B2A4A));
    s
}

/// Erg kit: black lamp panel with four round spots and slim outer head lamps, louvred
/// radiator outlets on the short hood, roof snorkel, navy box-flare lips, front / rear skid
/// plates, side cooling grilles, sand ladders, window nets, two upright spares in an open
/// tube-framed tail bay, roof wing, small LED tail strips, mud flaps, roof antennae.
fn erg_kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let mut k = Kit::new(ctx.profile, b);
    let p = ctx.profile;
    let yn = k.yn;
    let yt = k.yt;
    let wn = k.wn();
    let wt = k.wt();
    let (nz0, nz1) = k.nose_z();
    let (tz0, tz1) = k.tail_z();
    let fb = BodyRegion::FrontBumper;
    let ff = BodyRegion::FrameFront;
    let fr = BodyRegion::FrameRear;
    let hood = BodyRegion::Hood;

    // --- Lamp cluster: black panel, four round spotlights in a row, slim lamps at the corners.
    let (pz0, pz1) = (nz1 - 0.24, nz1 - 0.03);
    let kpn = dkey("lamp_panel", fb, M::Trim);
    k.front_box(&kpn, -(wn - 0.05), wn - 0.05, pz0, pz1, 0.010, 0.0);
    let zc = 0.5 * (pz0 + pz1) - 0.01;
    for sx in SIDES {
        let hk = headlight_key(sx);
        let kb = sided(&dkey("spot_bezel_l", fb, M::Chrome), sx);
        for x in [0.17, 0.45] {
            k.round_lamp(&hk, true, sx * x, zc, 0.072, 0.040, Some(&kb));
        }
        let ks = sided(
            &lkey("slim_lamp_l", BodyRegion::FrontLeftFender, M::Headlight),
            sx,
        );
        k.front_box(
            &ks,
            sx * 0.60,
            sx * (wn - 0.08),
            pz1 - 0.07,
            pz1 - 0.025,
            0.024,
            0.0,
        );
        let ka = sided(
            &dkey(
                "front_marker_l",
                BodyRegion::FrontLeftFender,
                M::EmissiveAmber,
            ),
            sx,
        );
        k.front_box(
            &ka,
            sx * 0.60,
            sx * (wn - 0.08),
            pz0 + 0.03,
            pz0 + 0.05,
            0.022,
            0.0,
        );
    }
    // Lower radiator intake, skid plate with navy tow straps.
    k.mesh_grille(
        "front_intake",
        fb,
        0.62,
        nz0 + 0.10,
        pz0 - 0.05,
        2,
        7,
        M::Trim,
    );
    let ksk = dkey("front_skid_plate", ff, M::Metal);
    k.plate(
        &ksk,
        [0.0, yn + 0.06, nz0 + 0.06],
        [0.0, yn - 0.75, nz0 - 0.10],
        [1.0, 0.0, 0.0],
        1.30,
        0.014,
    );
    for sx in SIDES {
        let kts = sided(&dkey("tow_strap_front_l", ff, M::Accent), sx);
        let x = sx * 0.50;
        k.bx(
            &kts,
            [x - 0.025, yn - 0.02, nz0 - 0.02],
            [x + 0.025, yn + 0.09, nz0 + 0.10],
        );
    }

    // --- Hood: big radiator-outlet louvres either side of a navy centre stripe, hood pins.
    let span = yn - p.y_fw;
    let (h0, h1) = (p.y_fw + 0.14 * span, p.y_fw + 0.62 * span);
    k.louvres(hood, h0, h1, 6, 0.14, 0.62, M::Trim);
    let khs = dkey("hood_stripe", hood, M::Accent);
    stripe(&mut k, &khs, p.y_fw + 0.02, yn - 0.06, -0.10, 0.10, (10, 1));
    k.hood_pins(yn - 0.18, 0.55);

    // --- Roof: snorkel scoop over the windshield header, navy roof panel, antennae + puck.
    let (roof_rear, roof_front) = (p.roof.0, p.roof.1);
    let zr = k.top_z(roof_front - 0.25, 0.0);
    let ksc = dkey("roof_scoop", BodyRegion::Roof, M::Paint);
    let prof = [
        [roof_front - 0.55, zr - 0.01],
        [roof_front + 0.10, zr - 0.01],
        [roof_front + 0.16, zr + 0.17],
        [roof_front - 0.25, zr + 0.15],
    ];
    k.prism_x(&ksc, &prof, -0.20, 0.20);
    let ksm = dkey("roof_scoop_mouth", BodyRegion::Roof, M::Grille);
    let pm = [
        [roof_front + 0.10, zr + 0.03],
        [roof_front + 0.135, zr + 0.03],
        [roof_front + 0.165, zr + 0.145],
        [roof_front + 0.13, zr + 0.145],
    ];
    k.prism_x(&ksm, &pm, -0.17, 0.17);
    let rh = p.roof_half_w(0.5 * (roof_rear + roof_front)) - 0.12;
    let krp = dkey("roof_panel", BodyRegion::Roof, M::Accent);
    stripe(
        &mut k,
        &krp,
        roof_rear + 0.20,
        roof_front - 0.58,
        -rh,
        rh,
        (3, 6),
    );
    let ka = dkey("roof_antennae", BodyRegion::Roof, M::Trim);
    // Short whips: the validator caps the mesh height at 1.2 × the body height.
    for (x, len) in [(-0.35, 0.30), (0.35, 0.22)] {
        let y = roof_rear + 0.35;
        let z = k.top_z(y, x);
        k.b.cylinder(
            &ka,
            [x, y, z - 0.005],
            [x, y, z + 0.03],
            0.016,
            0.012,
            6,
            true,
        );
        k.b.cylinder(
            &ka,
            [x, y, z + 0.03],
            [x, y - 0.05, z + len],
            0.004,
            0.0025,
            5,
            false,
        );
    }
    let kpk = dkey("roof_puck", BodyRegion::Roof, M::Trim);
    let zpk = k.top_z(roof_rear + 0.70, 0.0);
    k.b.cylinder(
        &kpk,
        [0.0, roof_rear + 0.70, zpk - 0.005],
        [0.0, roof_rear + 0.70, zpk + 0.03],
        0.07,
        0.06,
        10,
        true,
    );

    // --- Flanks: navy flare lips, side cooling grilles behind the rear arches, sand ladders,
    // navy lower stripe on the doors, window nets.
    let af = k.arch(true);
    let ar = k.arch(false);
    let xf = p.half_width(af.0) + p.style.flare.min(0.08);
    let xr = p.half_width(ar.0) + p.style.flare.min(0.08);
    for sx in SIDES {
        let kf = dkey("flare_lip_front_l", BodyRegion::FrontLeftFender, M::Accent);
        box_flare(&mut k, &kf, sx, af, xf, 0.08);
        let kr = dkey("flare_lip_rear_l", BodyRegion::RearLeftQuarter, M::Accent);
        box_flare(&mut k, &kr, sx, ar, xr, 0.08);
        // Rear cooling outlet behind the rear arch: dark mesh with three slats.
        let kg = dkey(
            "rear_cooling_grille_l",
            BodyRegion::RearLeftQuarter,
            M::Grille,
        );
        let (g0, g1) = (yt + 0.08, ar.0 - ar.2 - 0.03);
        let (gz0, gz1) = (ar.1 + 0.05, p.belt(g1) - 0.06);
        k.side_patch(&kg, sx, g0, g1, gz0, gz1, 0.002, 0.006, (2, 2));
        let kgs = dkey("rear_cooling_slats_l", BodyRegion::RearLeftQuarter, M::Trim);
        for i in 0..3 {
            let z = gz0 + (gz1 - gz0) * (i as f32 + 0.5) / 3.0;
            k.side_patch(&kgs, sx, g0, g1, z - 0.012, z + 0.012, 0.008, 0.012, (2, 1));
        }
        // Sand ladder: plate with raised rungs, on the rear door.
        let kl = sided(&dkey("sand_ladder_l", BodyRegion::FrameLeft, M::Metal), sx);
        let (l0, l1) = (ar.0 + ar.2 + 0.12, ar.0 + ar.2 + 1.12);
        let zl = p.sill(0.5 * (l0 + l1)) + 0.30;
        let n = 7;
        for i in 0..=n {
            let y = l0 + (l1 - l0) * i as f32 / n as f32;
            let h = if i == 0 || i == n { 0.18 } else { 0.14 };
            k.side_box(&kl, sx, y, zl, 0.035, h, 0.030, 0.012);
        }
        let ym = 0.5 * (l0 + l1);
        k.side_box(&kl, sx, ym, zl + 0.075, l1 - l0, 0.025, 0.020, 0.012);
        k.side_box(&kl, sx, ym, zl - 0.075, l1 - l0, 0.025, 0.020, 0.012);
        // Navy stripe low on both doors.
        let zs = p.sill(0.0) + 0.05;
        for (name, region, a, c) in [
            (
                "door_stripe_front_l",
                BodyRegion::LeftDoorF,
                p.y_fd_rear + 0.02,
                af.0 - af.2 - 0.04,
            ),
            (
                "door_stripe_rear_l",
                BodyRegion::LeftDoorR,
                ar.0 + ar.2 + 0.04,
                p.y_fd_rear - 0.02,
            ),
        ] {
            let ks = dkey(name, region, M::Accent);
            k.side_patch(&ks, sx, a, c, zs, zs + 0.09, 0.004, 0.0, (2, 1));
        }
        // Window net on the front door glass: a coarse grid of dark straps.
        let kn = sided(&dkey("window_net_l", BodyRegion::LeftDoorF, M::Trim), sx);
        let (wy0, wy1) = (p.y_fd_rear + 0.10, p.y_fd_hinge - 0.12);
        let zb = p.belt(0.5 * (wy0 + wy1)) + 0.06;
        let zt = zb + 0.26;
        for i in 0..4 {
            let y = wy0 + (wy1 - wy0) * i as f32 / 3.0;
            k.side_box(&kn, sx, y, 0.5 * (zb + zt), 0.012, zt - zb, 0.010, 0.0);
        }
        for i in 0..3 {
            let z = zb + (zt - zb) * i as f32 / 2.0;
            k.side_box(&kn, sx, 0.5 * (wy0 + wy1), z, wy1 - wy0, 0.012, 0.010, 0.0);
        }
    }
    k.mud_flaps(M::Rubber);

    // --- Tail: open bay (dark panel) framed by tubes, two upright spares, rear skid plate.
    let kbay = dkey("tail_bay", BodyRegion::Trunk, M::Grille);
    k.rear_box(
        &kbay,
        -(wt - 0.08),
        wt - 0.08,
        tz0 + 0.06,
        tz1 - 0.03,
        0.006,
        0.0,
    );
    let r_sp = ctx.def.wheels.first().map(|w| w.radius_m).unwrap_or(0.47);
    let zs = tz0 + 0.04 + r_sp;
    let ktr = dkey("spare_tires", fr, M::Rubber);
    let krm = dkey("spare_rims", fr, M::Trim);
    let xs = (wt - 0.10 - r_sp).max(r_sp + 0.01);
    for sx in SIDES {
        spare_wheel(
            &mut k,
            &ktr,
            &krm,
            [sx * xs, yt - 0.02, zs],
            [0.0, -1.0, 0.0],
            r_sp,
            0.15,
        );
    }
    let kf = dkey("tail_frame", fr, M::Trim);
    let yf = yt - 0.19;
    let (fz0, fz1) = (tz0 + 0.02, zs + r_sp + 0.04);
    let hx = wt - 0.03;
    for sx in SIDES {
        tube(&mut k, &kf, [sx * hx, yf, fz0], [sx * hx, yf, fz1], TUBE_R);
        tube(
            &mut k,
            &kf,
            [sx * hx, yf, fz1],
            [sx * hx, yt + 0.10, fz1 + 0.02],
            TUBE_R,
        );
        tube(
            &mut k,
            &kf,
            [sx * hx, yf, fz0],
            [sx * hx, yt + 0.10, fz0],
            TUBE_R,
        );
    }
    tube(&mut k, &kf, [-hx, yf, fz1], [hx, yf, fz1], TUBE_R);
    tube(&mut k, &kf, [-hx, yf, fz0], [hx, yf, fz0], TUBE_R);
    tube(&mut k, &kf, [0.0, yf, fz0], [0.0, yf, fz1], TUBE_R);
    let krs = dkey("rear_skid_plate", fr, M::Metal);
    k.plate(
        &krs,
        [0.0, yf, fz0 - 0.02],
        [0.0, yt + 0.60, tz0 - 0.08],
        [1.0, 0.0, 0.0],
        1.20,
        0.014,
    );

    // --- Small LED tails: the spares cover the whole tail face, so the lamps sit high on the
    // rear pillars (two slim vertical strips per side in a black housing), plus a centre stop
    // strip under the wing.
    let (zg0, zg1) = (p.roof.2, p.z_deck);
    let glass_y = |z: f32| {
        let t = ((zg0 - z) / (zg0 - zg1).max(1e-3)).clamp(0.0, 1.0);
        roof_rear + (p.y_deck - roof_rear) * t
    };
    let (lz0, lz1) = (zg1 + 0.22, zg1 + 0.50);
    let yl = glass_y(0.5 * (lz0 + lz1));
    let xo = k.side_x(yl, 0.5 * (lz0 + lz1)) - 0.015;
    for sx in SIDES {
        let tk = taillight_key(sx);
        let kth = sided(
            &dkey("taillight_housing_l", BodyRegion::RearLeftQuarter, M::Trim),
            sx,
        );
        k.bx(
            &kth,
            [sx * (xo - 0.12), yl + 0.02, lz0],
            [sx * xo, yl - 0.025, lz1],
        );
        // Outer strip red, inner strip the amber turn signal.
        let ik = indicator_rear_key(sx, M::EmissiveAmber);
        for (a, c, key) in [(0.015, 0.045, &tk), (0.065, 0.095, &ik)] {
            k.bx(
                key,
                [sx * (xo - c), yl - 0.02, lz0 + 0.02],
                [sx * (xo - a), yl - 0.04, lz1 - 0.02],
            );
        }
    }
    k.roof_spoiler(M::Accent, 0.26, 0.05, 0.05, 0.10);
    let kst = lkey("stop_lamp", BodyRegion::Trunk, M::Taillight);
    let ysl = roof_rear - 0.02;
    let zsl = k.top_z(roof_rear + 0.06, 0.0);
    k.bx(
        &kst,
        [-0.22, ysl - 0.012, zsl - 0.05],
        [0.22, ysl + 0.004, zsl - 0.03],
    );

    k.mirrors(0.14, 0.12, Some(M::Accent));
    k.wipers();

    // Turn signals as segments beside the lamps (playtest row 136).
    k.signals_beside_lamps(0.06, M::EmissiveAmber);
}
