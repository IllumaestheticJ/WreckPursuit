//! Nissan Skyline R34 coupe family (1998–2002): 25GT (RB25DE), 25GT-T (RB25DET), GT-R and GT-R
//! V-Spec II (RB26DETT, wide body).
//!
//! The body comes from `data/panels/models/nissan_skyline_r34.json` (side view, plan view and
//! sections measured against the real car: 4600 × 1785 × 1360 mm GT-R, 4580 × 1725 × 1340 mm
//! 25GT, wheelbase 2665 mm). Geometry here is only what the silhouette needs: flat head-lamp
//! lenses on the raked nose, the bumper-corner turn signals, the quad round tail lamps (outer =
//! tail / brake, inner = turn signal), the trim's rear wing and the door mirrors. Grilles,
//! intakes, the intercooler, badges, plates, handles, the fuel door, fender vents, panel gaps
//! and wheel spokes are painted ([`crate::paint`], layers named by the trim).

use std::sync::Arc;

use wp_contracts::{BodyRegion, VehicleDef};

use crate::builder::{Builder, PartKey};
use crate::lamp::{ellipse, lens, Lamp, LampView};
use crate::mesh::{indicator_front_key, indicator_rear_key, MaterialSlot as M};
use crate::section::{SectionSpec, WingKind};
use crate::style::{
    ArchShape, BodyStyle, CaliperKind, CapKind, CarStyle, Detail, DetailCtx, EngineKind,
    ExhaustKind, FuelDoor, Induction, RimKind, Shape, Silhouette, SuspensionKind, TrimFinish,
};
use crate::styles::performance::kit::{dkey, taillight_key, Kit, SIDES};

/// Head-lamp outline seen from ahead, LEFT side as |x| (m, z m above ground; the GT-R spec's
/// heights): a wedge whose inner end drops toward the grille, the outer end wrapping round the
/// corner (cast with [`HEADLAMP_YAW_DEG`]).
pub const HEADLAMP: [[f32; 2]; 7] = [
    [0.372, 0.664],
    [0.600, 0.684],
    [0.792, 0.708],
    [0.826, 0.738],
    [0.816, 0.772],
    [0.600, 0.790],
    [0.362, 0.786],
];
/// Head-lamp cast yaw: the outer end wraps onto the fender, deg.
pub const HEADLAMP_YAW_DEG: f32 = 18.0;
/// Front turn signal in the bumper corner, under the lamp's outer half (|x|, z).
pub const FRONT_SIGNAL: [[f32; 2]; 4] = [
    [0.650, 0.612],
    [0.792, 0.622],
    [0.788, 0.660],
    [0.650, 0.652],
];
/// Tail lamps (centre |x|, centre z, radius): outer tail / brake lamp, inner turn signal.
pub const TAIL_OUTER: (f32, f32, f32) = (0.655, 0.815, 0.083);
/// Inner round lamp (turn signal + reverse ring).
pub const TAIL_INNER: (f32, f32, f32) = (0.462, 0.815, 0.076);

/// Style of an R34 trim (`spec` = the family spec fitted to the car, its `active_trim` set).
pub fn style(def: &VehicleDef, spec: Arc<SectionSpec>) -> CarStyle {
    let trim = spec.trim(&spec.active_trim);
    let wide = spec.active_trim.starts_with("gtr");
    let mut body = BodyStyle {
        silhouette: Silhouette::Notchback,
        hood_crown: 0.018,
        roof_crown: 0.022,
        power_bulge_h: if wide { 0.010 } else { 0.0 },
        hood_bulge_w: 0.40,
        nose_rake_deg: 30.0,
        crease_z: 0.42,
        crease_depth: 0.004,
        sill_tuck: 0.05,
        shoulder_round: 0.025,
        glass_inset: 0.10,
        flare: trim.flare_m.unwrap_or(if wide { 0.010 } else { 0.004 }),
        flare_shape: ArchShape::Round,
        arch_gap: trim.arch_gap_m.unwrap_or(if wide { 0.030 } else { 0.040 }),
        a_pillar_w_m: 0.07,
        c_pillar_w_m: 0.17,
        b_pillar_w_m: 0.07,
        b_pillar: TrimFinish::Black,
        roof_rail_drop_m: 0.045,
        windshield_wrap_m: 0.05,
        rear_glass_wrap_m: 0.06,
        cowl_len_m: 0.08,
        end_round_m: 0.02,
        dlo_kink_m: 0.035,
        dlo_kink_len_m: 0.18,
        arch_lip_m: 0.012,
        // Painted instead: handles, fuel door, panel gaps.
        door_handles: TrimFinish::None,
        fuel_door: FuelDoor::None,
        panel_seams: false,
        mirrors: TrimFinish::Body,
        sections: Some(spec.clone()),
        ..BodyStyle::preset(Silhouette::Notchback)
    };
    body.overhang_trim = (0.0, 0.0);
    let mut s = CarStyle::with_body(body);
    s.details.push(Detail::new(
        "body_kit",
        BodyRegion::Cabin,
        M::Trim,
        Shape::Custom(kit),
    ));
    let r = def.wheels.first().map(|w| w.radius_m).unwrap_or(0.32);
    let w = &mut s.wheel;
    let rim_in = trim.wheel.as_ref().map_or(17.0, |w| w.diameter_in);
    w.rim = RimKind::FiveSpoke;
    w.spokes = 5;
    w.rim_radius_m = Some((0.5 * rim_in * 0.0254).min(0.82 * r));
    w.rim_material = M::Metal;
    w.rim_rgb = trim.wheel.as_ref().map(|w| w.rgb);
    w.cap = CapKind::None;
    w.face_depth_m = 0.025;
    w.lip_width_m = 0.012;
    w.segments = 28;
    w.painted = true;
    let m = &mut s.mechanical;
    m.engine = EngineKind::Inline6;
    m.induction = if spec.active_trim == "25gt" {
        Induction::Natural
    } else {
        Induction::Turbo
    };
    m.show_engine_detail = true;
    m.front_suspension = SuspensionKind::Multilink;
    m.rear_suspension = SuspensionKind::Multilink;
    m.front_brakes.caliper = if spec.active_trim == "25gt" {
        CaliperKind::Floating
    } else {
        CaliperKind::Fixed4
    };
    m.exhaust = ExhaustKind::Single;
    m.exhaust_tip_r_m = if wide { 0.045 } else { 0.04 };
    if wide {
        // Gold Brembo calipers.
        m.caliper_rgb = Some([204, 163, 41]);
    }
    if let Some(c) = trim.paint_rgb {
        s.tints.push((M::Paint, c));
    }
    s
}

/// The family kit: lamps, signals, wing, mirrors (see the module docs).
fn kit(ctx: &DetailCtx, _key: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let trim = p.spec().map(|s| s.trim(&s.active_trim)).unwrap_or_default();
    let mut k = Kit::new(p, b);
    k.lens_step_m = 0.03;
    // Heights are measured on the GT-R; the trims' ride offsets move them.
    let dz = p.belt(p.y_nose) - 0.62;
    let up = |o: &[[f32; 2]]| -> Vec<[f32; 2]> { o.iter().map(|q| [q[0], q[1] + dz]).collect() };
    for sx in SIDES {
        // Head lamp: one flat lens on the raked upper nose (the two round elements are painted).
        k.proj_cluster(
            sx,
            true,
            LampView::Front,
            &up(&HEADLAMP),
            None,
            M::EmissiveAmber,
            0.0,
            HEADLAMP_YAW_DEG,
        );
        // Turn signal in the bumper corner under the lamp's outer half.
        let mut sig = Lamp::new(
            LampView::Front,
            up(&FRONT_SIGNAL)
                .iter()
                .map(|q| [sx * q[0], q[1]])
                .collect(),
        );
        sig.yaw_deg = 10.0;
        sig.proud_m = 0.006;
        sig.step_m = 0.03;
        lens(k.b, p, &indicator_front_key(sx > 0.0), &sig);
        // Quad round tails: outer tail / brake lamp, inner turn signal (red lens, amber light).
        let dzt = p.belt(p.y_tail) - 0.92;
        for ((cx, cz, r), key) in [
            (TAIL_OUTER, taillight_key(sx)),
            (TAIL_INNER, indicator_rear_key(sx > 0.0, M::EmissiveAmber)),
        ] {
            let mut l = Lamp::new(LampView::Rear, ellipse([sx * cx, cz + dzt], [r, r], 14));
            l.proud_m = 0.010;
            l.step_m = 0.03;
            lens(k.b, p, &key, &l);
        }
    }
    match trim.wing {
        WingKind::TwoElement => two_element_wing(&mut k),
        WingKind::Pedestal => pedestal_wing(&mut k),
        WingKind::Lip => lip(&mut k),
        WingKind::None => {}
    }
    k.mirrors(0.09, 0.105, None);
}

/// GT-R wing: a main plane on two raked stalks near the trunk's sides, small end plates and the
/// adjustable lower blade between the stalks (all on the trunk lid, so it swings with it).
fn two_element_wing(k: &mut Kit) {
    let p = k.p;
    let kw = dkey("wing", BodyRegion::Trunk, M::Paint);
    let ks = dkey("wing_stands", BodyRegion::Trunk, M::Paint);
    let kb = dkey("wing_lower_blade", BodyRegion::Trunk, M::Paint);
    let y_le = p.y_tail + 0.40;
    let c = 0.235;
    let deck = k.top_z(y_le - 0.5 * c, 0.0);
    let zb = deck + 0.125;
    let t = 0.030;
    let rise = 0.035;
    let hs = (p.half_width(y_le) - 0.08).min(0.73);
    let prof = [
        [y_le, zb + 0.30 * t],
        [y_le - 0.05 * c, zb + 0.85 * t],
        [y_le - 0.35 * c, zb + t + 0.35 * rise],
        [y_le - 0.80 * c, zb + 0.75 * t + 0.80 * rise],
        [y_le - c, zb + 0.2 * t + rise],
        [y_le - c, zb + rise - 0.002],
        [y_le - 0.5 * c, zb + 0.5 * rise - 0.004],
        [y_le - 0.08 * c, zb + 0.02 * t],
    ];
    k.prism_x(&kw, &prof, -hs, hs);
    for sx in SIDES {
        // End plate.
        let x = sx * (hs + 0.004);
        let ep = [
            [y_le + 0.02, zb - 0.035],
            [y_le - c - 0.01, zb + rise - 0.03],
            [y_le - c - 0.02, zb + rise + 0.055],
            [y_le - 0.15, zb + 0.075],
        ];
        k.prism_x(&ks, &ep, x - 0.005, x + 0.005);
        // Stalk: raked back, from the deck to the main plane's underside.
        let xs = sx * (hs - 0.09);
        let yb = y_le - 0.45 * c;
        let zd = k.top_z(yb, xs);
        let st = [
            [yb + 0.075, zd - 0.01],
            [yb - 0.045, zd - 0.01],
            [yb - 0.085, zb + 0.008],
            [yb + 0.015, zb + 0.008],
        ];
        k.prism_x(&ks, &st, xs - 0.011, xs + 0.011);
    }
    // Lower blade between the stalks, under the main plane's trailing half.
    let yl = y_le - 0.55 * c;
    let zl = deck + 0.075;
    let bh = hs - 0.10;
    let blade = [
        [yl, zl + 0.006],
        [yl - 0.035, zl + 0.012],
        [yl - 0.085, zl + 0.022],
        [yl - 0.085, zl + 0.014],
        [yl - 0.03, zl],
    ];
    k.prism_x(&kb, &blade, -bh, bh);
}

/// 25GT-T wing: one plane on two mid-height pedestals.
fn pedestal_wing(k: &mut Kit) {
    use crate::styles::performance::kit::{Stand, Wing};
    let hs = (k.p.half_width(k.p.y_tail + 0.3) - 0.14).min(0.66);
    k.wing(Wing {
        lead: 0.34,
        chord: 0.19,
        height: 0.085,
        half_span: hs,
        thick: 0.026,
        aoa_deg: 5.0,
        stand: Stand::Pedestal(hs - 0.12),
        endplates: None,
        mat: M::Paint,
        stand_mat: M::Paint,
        gurney: 0.0,
    });
}

/// Trunk-lid lip spoiler.
fn lip(k: &mut Kit) {
    let y = k.p.y_tail + 0.06;
    k.lip_spoiler(M::Paint, y, 0.025, 0.10, 0.12);
}
