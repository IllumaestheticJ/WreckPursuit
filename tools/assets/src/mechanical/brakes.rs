//! Brakes and pivot hardware of one wheel, hub space (`research_mechanical.md` §3, §2.1).
//!
//! - Spinning (`BrakeDisc`): disc annulus (`wheel_xx_disc`, metal / carbon), top hat
//!   (`wheel_xx_disc_hat`, dark), vent gap / drilled dots / slots (`wheel_xx_disc_dark`); or a
//!   drum with its hat.
//! - Pivot (`Caliper`, `Upright`: steer + follow compression, no spin): arc-swept caliper
//!   (`wheel_xx_caliper`, caliper tint) at its clock angle with a bracket for floating types;
//!   knuckle / carrier, strut lower body and spring seat, drum backing plate, live-axle half
//!   tube (`wheel_xx_upright`).
//!
//! The disc sits inboard of the spokes' back face with room for the caliper's outboard bulk;
//! closed-face wheels (steel, slotted, truck) hide it, so they get a cheap disc and caliper.

use crate::builder::{Builder, PartKey};
use crate::mesh::{MaterialSlot, PartKind};
use crate::wheels::WheelGeom;

use super::prim::{arc_sweep, hub_lathe, hub_poly, hub_pt};
use super::spec::{Caliper, DiscFace, Susp};

/// Caliper proportions: (arc span °, radial depth, axial width beyond the disc, inboard
/// share of the bulk).
fn caliper_dims(c: Caliper) -> (f32, f32, f32, f32) {
    match c {
        Caliper::Floating1 => (55.0, 0.062, 0.055, 0.72),
        Caliper::Floating2 => (66.0, 0.072, 0.068, 0.66),
        Caliper::Fixed2 => (52.0, 0.058, 0.05, 0.5),
        Caliper::Fixed4 => (70.0, 0.068, 0.058, 0.5),
        Caliper::Fixed6 => (90.0, 0.076, 0.064, 0.5),
        Caliper::Mono8 => (108.0, 0.082, 0.068, 0.5),
        Caliper::Drum => (0.0, 0.0, 0.0, 0.5),
    }
}

/// Where the brake ended up (for the pivot parts behind it).
pub struct BrakePlace {
    /// Most inboard `u` of the disc / caliper / drum.
    pub u_inboard: f32,
    /// Inboard face of the disc (or drum backing plate), `u`.
    pub u_disc_back: f32,
}

/// Emit the brake of wheel `g` into `b`; returns its inboard extent.
pub fn brake(g: &WheelGeom, b: &mut Builder) -> BrakePlace {
    let bs = g.brake;
    let l = g.layout;
    let s = g.outboard;
    let closed = l.closed;
    let n = if closed { 10 } else { g.lathe_n() };
    let rim_r = g.rim_radius;
    let fixed = matches!(
        bs.caliper,
        Caliper::Fixed2 | Caliper::Fixed4 | Caliper::Fixed6 | Caliper::Mono8
    );
    let mut disc_r = if bs.disc_r > 0.0 {
        bs.disc_r
    } else if bs.disc_r < 0.0 {
        -bs.disc_r * rim_r
    } else {
        0.8 * rim_r
    };
    let max_r = if bs.caliper == Caliper::Drum {
        l.r_b - 0.02
    } else if fixed {
        (rim_r - 0.045).min(l.r_b - 0.022)
    } else {
        (rim_r - 0.035).min(l.r_b - 0.02)
    };
    disc_r = disc_r.clamp(0.09, max_r.max(0.1));
    let t = bs.t.clamp(
        0.008,
        if bs.caliper == Caliper::Drum {
            0.09
        } else {
            0.045
        },
    );
    let (span, depth, extra, inb) = caliper_dims(bs.caliper);
    let cal_w = t + extra;
    let cal_out = cal_w * (1.0 - inb) - 0.5 * t;
    // Disc outboard face: clear of the spokes' back (with the caliper bulk) and the hub pad.
    let u_hat_top = l.u_hub - 0.03;
    let u_df = (l.u_back_min - cal_out.max(0.0) - 0.006)
        .min(u_hat_top - 0.006)
        .max(-l.rw + t + 0.01);
    let u_dc = u_df - 0.5 * t;
    let hat_r = (0.085 * l.kz).max(0.07).min(disc_r - 0.04).max(0.04);

    let key = |suffix: &str, kind: PartKind, m: MaterialSlot| {
        PartKey::wheel(&g.name(suffix), g.index, kind, m)
    };
    let disc_mat = if bs.face == DiscFace::Ceramic {
        MaterialSlot::Carbon
    } else {
        MaterialSlot::Metal
    };
    let hat_mat = if bs.face == DiscFace::Ceramic {
        MaterialSlot::Metal
    } else {
        MaterialSlot::Engine
    };
    let disc = key("disc", PartKind::BrakeDisc, disc_mat);
    let hat = key("disc_hat", PartKind::BrakeDisc, hat_mat);
    let dark = key("disc_dark", PartKind::BrakeDisc, MaterialSlot::Grille);
    let pivot = key("upright", PartKind::Upright, MaterialSlot::Engine);

    if bs.caliper == Caliper::Drum {
        // Drum (spins) + backing plate (pivot).
        let (u0, u1) = (u_df - t, u_df);
        let drum = key("disc", PartKind::BrakeDisc, MaterialSlot::Engine);
        hub_lathe(
            b,
            &drum,
            s,
            &[
                [disc_r - 0.01, u0],
                [disc_r, u0 + 0.01],
                [disc_r, u1 - 0.008],
                [disc_r - 0.012, u1],
                [hat_r, u1],
                [hat_r, u_hat_top.max(u1 + 0.004)],
                [0.0, u_hat_top.max(u1 + 0.004)],
            ],
            n,
        );
        if g.hero && !closed {
            // Cooling fins on the drum face.
            for i in 0..8 {
                let d = 360.0 * i as f32 / 8.0;
                arc_sweep(
                    b,
                    &drum,
                    s,
                    &[
                        [hat_r + 0.01, u1],
                        [disc_r - 0.015, u1],
                        [disc_r - 0.015, u1 + 0.008],
                        [hat_r + 0.01, u1 + 0.008],
                    ],
                    d - 2.0,
                    d + 2.0,
                    1,
                    true,
                );
            }
        }
        hub_lathe(
            b,
            &pivot,
            s,
            &[[disc_r + 0.016, u0 - 0.004], [disc_r - 0.02, u0 - 0.004]],
            n,
        );
        return BrakePlace {
            u_inboard: u0 - 0.006,
            u_disc_back: u0 - 0.004,
        };
    }

    // Disc annulus: inboard face, rim edge, outboard face.
    let r_in = hat_r - 0.004;
    let full = [
        [r_in, u_dc - 0.5 * t],
        [disc_r, u_dc - 0.5 * t],
        [disc_r, u_dc + 0.5 * t],
        [r_in, u_dc + 0.5 * t],
    ];
    hub_lathe(b, &disc, s, if closed { &full[1..] } else { &full[..] }, n);
    // Top hat up to the hub (hidden behind a closed face).
    if !closed {
        let hat_top = u_hat_top.max(u_df + 0.006);
        hub_lathe(
            b,
            &hat,
            s,
            &[[hat_r, u_df - 0.002], [hat_r, hat_top], [0.0, hat_top]],
            (n / 2).max(8),
        );
    }
    if !closed {
        if bs.vented {
            hub_lathe(
                b,
                &dark,
                s,
                &[
                    [disc_r + 0.0008, u_dc - 0.2 * t],
                    [disc_r + 0.0008, u_dc + 0.2 * t],
                ],
                n,
            );
        }
        let band = |k: f32| disc_r - 0.012 - k * (disc_r - hat_r - 0.03).max(0.02) / 2.0;
        match bs.face {
            DiscFace::Plain => {}
            DiscFace::Drilled | DiscFace::Ceramic if g.hero => {
                let per = 8;
                for ring in 0..3 {
                    let r = band(ring as f32);
                    for i in 0..per {
                        let d = (360.0 * i as f32 + 15.0 * ring as f32 * per as f32) / per as f32;
                        let (h_deg, h_r) = ((0.0035 / r).to_degrees(), 0.0035);
                        let pts = [
                            hub_pt(s, d - h_deg, r - h_r, u_df + 0.0015),
                            hub_pt(s, d + h_deg, r - h_r, u_df + 0.0015),
                            hub_pt(s, d + h_deg, r + h_r, u_df + 0.0015),
                            hub_pt(s, d - h_deg, r + h_r, u_df + 0.0015),
                        ];
                        hub_poly(b, &dark, s, &pts, true);
                    }
                }
            }
            DiscFace::Slotted if g.hero => {
                for i in 0..10 {
                    let d = 36.0 * i as f32;
                    let (r0, r1) = (band(2.0), disc_r - 0.008);
                    let w = (0.002 / r1).to_degrees();
                    let pts_a = [
                        hub_pt(s, d - w, r0, u_df + 0.0015),
                        hub_pt(s, d + 7.0 - w, 0.5 * (r0 + r1), u_df + 0.0015),
                        hub_pt(s, d + 7.0 + w, 0.5 * (r0 + r1), u_df + 0.0015),
                        hub_pt(s, d + w, r0, u_df + 0.0015),
                    ];
                    let pts_b = [
                        hub_pt(s, d + 7.0 - w, 0.5 * (r0 + r1), u_df + 0.0015),
                        hub_pt(s, d + 12.0 - w, r1, u_df + 0.0015),
                        hub_pt(s, d + 12.0 + w, r1, u_df + 0.0015),
                        hub_pt(s, d + 7.0 + w, 0.5 * (r0 + r1), u_df + 0.0015),
                    ];
                    hub_poly(b, &dark, s, &pts_a, true);
                    hub_poly(b, &dark, s, &pts_b, true);
                }
            }
            _ => {}
        }
    }

    // Caliper: arc-swept section straddling the disc edge (pivot part).
    let cal = key("caliper", PartKind::Caliper, MaterialSlot::Caliper);
    let r0 = disc_r - depth + 0.012;
    let r1 = (disc_r + 0.012).min(l.r_b - 0.005);
    let (u_out, u_inb) = (
        u_dc + 0.5 * t + cal_out.max(0.004),
        u_dc - 0.5 * t - (cal_w * inb - 0.5 * t).max(0.006),
    );
    let section: Vec<[f32; 2]> = if g.hero && !closed {
        let c = 0.010f32.min(0.25 * (r1 - r0)).min(0.25 * (u_out - u_inb));
        vec![
            [r0, u_inb + c],
            [r0 + c, u_inb],
            [r1 - c, u_inb],
            [r1, u_inb + c],
            [r1, u_out - c],
            [r1 - c, u_out],
            [r0 + c, u_out],
            [r0, u_out - c],
        ]
    } else {
        vec![[r0, u_inb], [r1, u_inb], [r1, u_out], [r0, u_out]]
    };
    let segs = if g.hero && !closed {
        ((span / 14.0).ceil() as usize).max(3)
    } else {
        2
    };
    let c = bs.clock_deg;
    arc_sweep(
        b,
        &cal,
        s,
        &section,
        c - 0.5 * span,
        c + 0.5 * span,
        segs,
        true,
    );
    if g.hero && !closed {
        if fixed {
            // Bridge "window" over the pad (dark) on the outer radial face.
            let (uw0, uw1) = (u_dc - 0.4 * t, u_dc + 0.4 * t);
            let h = 0.3 * span;
            let pts = [
                hub_pt(s, c - h, r1 + 0.0015, uw0),
                hub_pt(s, c + h, r1 + 0.0015, uw0),
                hub_pt(s, c + h, r1 + 0.0015, uw1),
                hub_pt(s, c - h, r1 + 0.0015, uw1),
            ];
            let mid = hub_pt(s, c, r1 - 0.01, u_dc);
            let out = super::prim::sub(super::prim::centroid(&pts), mid);
            b.poly_facing(&dark_pivot(g), &pts, out);
        } else {
            // Carrier bracket on the outboard side of the disc (a thin plate).
            arc_sweep(
                b,
                &pivot,
                s,
                &[
                    [r0 + 0.01, u_out],
                    [r1 - 0.004, u_out],
                    [r1 - 0.004, u_out + 0.006],
                    [r0 + 0.01, u_out + 0.006],
                ],
                c - 0.5 * span - 8.0,
                c + 0.5 * span + 8.0,
                2,
                true,
            );
        }
    }
    BrakePlace {
        u_inboard: u_inb.min(u_dc - 0.5 * t) - 0.004,
        u_disc_back: u_dc - 0.5 * t,
    }
}

fn dark_pivot(g: &WheelGeom) -> PartKey {
    PartKey::wheel(
        &g.name("caliper_dark"),
        g.index,
        PartKind::Caliper,
        MaterialSlot::Grille,
    )
}

/// Emit the knuckle / strut lower half / axle half tube of wheel `g` (pivot parts), behind
/// a brake whose inboard extent is `place`.
pub fn pivot_hardware(g: &WheelGeom, place: &BrakePlace, b: &mut Builder) {
    let s = g.outboard;
    let key = PartKey::wheel(
        &g.name("upright"),
        g.index,
        PartKind::Upright,
        MaterialSlot::Engine,
    );
    let k_in = g.knuckle_in.max(-place.u_inboard + 0.02);
    let x = |in_m: f32| -s * in_m;
    if g.susp.live_axle() {
        // Axle end bell + half tube toward the diff.
        let r = if g.radius > 0.36 { 0.044 } else { 0.038 };
        b.cylinder(
            &key,
            [x(k_in - 0.03), 0.0, 0.0],
            [x(k_in + 0.06), 0.0, 0.0],
            0.07,
            r,
            10,
            true,
        );
        b.cylinder(
            &key,
            [x(k_in + 0.06), 0.0, 0.0],
            [x(k_in + g.axle_in), 0.0, 0.0],
            r,
            r,
            8,
            false,
        );
        return;
    }
    let tall = matches!(
        g.susp,
        Susp::Sla | Susp::SlaCoil | Susp::SlaTorsion | Susp::Coilover
    );
    let (z0, z1): (f32, f32) = if tall { (-0.12, 0.23) } else { (-0.11, 0.10) };
    let carrier = matches!(g.susp, Susp::Multilink | Susp::TorsionBeam);
    let (hy, z1) = if carrier {
        (0.04, z1.max(0.14))
    } else {
        (0.035, z1)
    };
    let (xa, xb) = (x(k_in - 0.025), x(k_in + 0.025));
    b.beveled_box(
        &key,
        [xa.min(xb), -hy, z0],
        [xa.max(xb), hy, z1],
        if g.hero { 0.008 } else { 0.0 },
    );
    // Hub bearing boss toward the disc hat.
    b.cylinder(
        &key,
        [x(k_in - 0.02), 0.0, 0.0],
        [s * place.u_disc_back, 0.0, 0.0],
        0.05,
        0.045,
        8,
        false,
    );
    if g.steerable {
        // Steering arm toward the tie rod (behind the hub).
        let y_arm = -0.12;
        b.cuboid(
            &key,
            [x(k_in).min(x(k_in + 0.04)), y_arm, -0.035],
            [x(k_in).max(x(k_in + 0.04)), -0.03, -0.005],
        );
    }
    if g.susp == Susp::Strut {
        // Strut lower body (rises with the wheel) + spring seat.
        let p = |z: f32| {
            let f = (z - 0.08) / 0.50;
            [x(k_in + 0.08 * f), -0.02 - 0.02 * f, z]
        };
        b.cylinder(
            &key,
            p(0.06),
            p(0.30),
            0.027,
            0.027,
            if g.hero { 10 } else { 6 },
            true,
        );
        b.cylinder(&key, p(0.235), p(0.25), 0.078, 0.078, 8, true);
    }
}
