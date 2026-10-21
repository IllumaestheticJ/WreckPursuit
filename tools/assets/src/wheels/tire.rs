//! Tire: one lathe from the inboard bead over the tread to the outboard bead
//! (`research_mechanical.md` §4.3), plus all-terrain shoulder lugs.
//!
//! Profile points per side (outboard half; mirrored inboard): bead (tucked under the rim
//! flange), maximum section width (sidewall bulge), shoulder, tread edge; hero tires add an
//! upper-sidewall point. Circumferential grooves are V-notches in the tread
//! (3 profile points each). Cost `2 · N · (P − 1)`: hero N = 24 with 2 grooves ≈ 620, traffic
//! N = 16 ≈ 160.

use crate::builder::{Builder, PartKey};
use crate::mechanical::prim::{hexa, hub_lathe, hub_pt};
use crate::mechanical::spec::TireKind;
use crate::mesh::{MaterialSlot, PartKind};

use super::WheelGeom;

/// Sidewall / tread shape of a tire family: (bulge, shoulder radius ÷ W, tread width ÷ W,
/// crown, grooves, groove depth).
fn shape(kind: TireKind, aspect: f32) -> (f32, f32, f32, f32, usize, f32) {
    let tall = aspect >= 0.62;
    let low = aspect < 0.48;
    match kind {
        TireKind::VintageBias => (0.015, 0.24, 0.72, 0.008, 3, 0.005),
        TireKind::DragRadial => (0.018, 0.20, 0.85, 0.0, 0, 0.0),
        TireKind::SemiSlick => (0.004, 0.08, 0.90, 0.0, 2, 0.003),
        TireKind::Wet => (0.008, 0.13, 0.82, 0.003, 4, 0.007),
        TireKind::AllTerrain => (0.013, 0.16, 0.80, 0.004, 2, 0.008),
        _ if low => (0.004, 0.10, 0.86, 0.002, 2, 0.006),
        _ if tall => (0.012, 0.16, 0.80, 0.004, 2, 0.006),
        _ => (0.008, 0.13, 0.82, 0.003, 2, 0.006),
    }
}

/// Emit the tire of wheel `g` (part `wheel_xx_tire`).
pub fn tire(g: &WheelGeom, b: &mut Builder) {
    let key = PartKey::wheel(
        &g.name("tire"),
        g.index,
        PartKind::Tire,
        MaterialSlot::Rubber,
    );
    let s = g.outboard;
    let n = g.segments;
    let (r_out, w) = (g.radius, g.width);
    let rim_r = g.rim_radius;
    let h = (r_out - rim_r).max(0.03);
    let (bulge, sh_k, tread_k, crown, grooves, depth) = shape(g.tire, h / w.max(0.05));
    let sh = (sh_k * w).min(0.45 * h);
    let half = 0.5 * w;
    let bead_u = g.layout.rw - 0.006;
    // Outboard half from the bead up to the tread edge: (r, u).
    let mut side: Vec<[f32; 2]> = vec![
        [rim_r + 0.004, bead_u],
        [rim_r + 0.40 * h, half + bulge],
        [r_out - sh, half - 0.12 * sh],
    ];
    let tread_half = (0.5 * tread_k * w).min(half - 0.3 * sh);
    if g.hero {
        side.push([r_out - crown, tread_half]);
    }
    // Tread across (inboard edge → outboard edge), with V grooves (hero only).
    let mut tread: Vec<[f32; 2]> = Vec::new();
    let grooves = if g.hero { grooves.min(3) } else { 0 };
    if grooves > 0 {
        let gw = 0.012f32.min(0.08 * w);
        let xs: Vec<f32> = match grooves {
            1 => vec![0.0],
            2 => vec![-0.22 * w, 0.22 * w],
            _ => vec![-0.28 * w, 0.0, 0.28 * w],
        };
        for x in xs {
            let r_at = r_out - crown * (x / tread_half.max(0.01)).powi(2);
            tread.push([r_at, x - 0.5 * gw]);
            tread.push([r_at - depth, x]);
            tread.push([r_at, x + 0.5 * gw]);
        }
    } else {
        tread.push([r_out, 0.0]);
    }
    let mut prof: Vec<[f32; 2]> = side.iter().map(|q| [q[0], -q[1]]).collect();
    prof.extend(tread);
    prof.extend(side.iter().rev());
    hub_lathe(b, &key, s, &prof, n);

    if g.tire == TireKind::AllTerrain {
        // Two offset rows of shoulder lugs (blocks wrapping over each shoulder).
        let lugs = if g.hero { n / 2 } else { 9 };
        // Traffic: the outboard row only (the inboard one is rarely seen).
        let rows: &[f32] = if g.hero { &[-1.0, 1.0] } else { &[1.0] };
        for &side_sign in rows {
            for k in 0..lugs {
                let off = if side_sign > 0.0 { 0.5 } else { 0.0 };
                let deg = 360.0 * (k as f32 + off) / lugs as f32;
                let span = 0.55 * 180.0 / lugs as f32;
                let (u0, u1) = (side_sign * (tread_half - 0.05), side_sign * (half - 0.02));
                let (r0, r1) = (r_out - 0.008, r_out + 0.006);
                let c = |d: f32, r: f32, u: f32| hub_pt(s, d, r, u);
                hexa(
                    b,
                    &key,
                    [
                        c(deg - span, r0, u0),
                        c(deg + span, r0, u0),
                        c(deg + span, r0, u1),
                        c(deg - span, r0, u1),
                        c(deg - span, r1, u0),
                        c(deg + span, r1, u0),
                        c(deg + span, r1 - 0.016, u1),
                        c(deg - span, r1 - 0.016, u1),
                    ],
                );
            }
        }
    }
}
