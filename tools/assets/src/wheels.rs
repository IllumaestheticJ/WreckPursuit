//! Wheels: tires with a rounded shoulder and styled rims (see [`RimStyle`]), dual rear tires
//! and visual extra axles (tandems). Hub space: axle along +X, origin at the hub; the outer
//! face is −X on the left wheels (FL, RL) and +X on the right (FR, RR).

use wp_contracts::VehicleDef;

use crate::mesh::{to_mm, MaterialSlot, MeshPart, PartKind};
use crate::style::{CarStyle, RimStyle};

const WHEEL_NAMES: [&str; 4] = ["fl", "fr", "rl", "rr"];
/// Segments around the axle.
const SEG: usize = 18;

fn at(x: f32, r: f32, ang: f32) -> [f32; 3] {
    let (s, c) = wp_math::sin_cos(ang);
    [x, r * s, r * c]
}

fn seg_ang(i: usize, n: usize) -> f32 {
    std::f32::consts::TAU * i as f32 / n as f32
}

/// Triangle soup for one wheel part.
#[derive(Default)]
struct Soup {
    pos: Vec<[f32; 3]>,
    idx: Vec<u32>,
}

impl Soup {
    fn poly(&mut self, pts: &[[f32; 3]]) {
        let base = self.pos.len() as u32;
        self.pos.extend_from_slice(pts);
        for i in 1..pts.len() as u32 - 1 {
            self.idx.extend_from_slice(&[base, base + i, base + i + 1]);
        }
    }

    /// Flat annulus at `x` between radii.
    fn annulus(&mut self, x: f32, r0: f32, r1: f32) {
        for i in 0..SEG {
            let (a, b) = (seg_ang(i, SEG), seg_ang(i + 1, SEG));
            self.poly(&[at(x, r0, a), at(x, r1, a), at(x, r1, b), at(x, r0, b)]);
        }
    }

    /// Cone band from (x0, r0) to (x1, r1).
    fn band(&mut self, x0: f32, r0: f32, x1: f32, r1: f32) {
        for i in 0..SEG {
            let (a, b) = (seg_ang(i, SEG), seg_ang(i + 1, SEG));
            self.poly(&[at(x0, r0, a), at(x1, r1, a), at(x1, r1, b), at(x0, r0, b)]);
        }
    }

    /// Disc at `x`.
    fn disc(&mut self, x: f32, r: f32, n: usize) {
        let pts: Vec<[f32; 3]> = (0..n).map(|i| at(x, r, seg_ang(i, n))).collect();
        self.poly(&pts);
    }

    /// Spoke from radius r0 (at x0) to r1 (at x1), angular half widths a0/a1, centre angle
    /// `c`, twisted by `twist` rad at the outer end.
    #[allow(clippy::too_many_arguments)]
    fn spoke(&mut self, x0: f32, x1: f32, r0: f32, r1: f32, a0: f32, a1: f32, c: f32, twist: f32) {
        self.poly(&[
            at(x0, r0, c - a0),
            at(x1, r1, c + twist - a1),
            at(x1, r1, c + twist + a1),
            at(x0, r0, c + a0),
        ]);
    }

    fn part(
        self,
        name: String,
        kind: PartKind,
        material: MaterialSlot,
        wheel: u8,
        off: Option<[i32; 3]>,
    ) -> MeshPart {
        let n = self.pos.len();
        MeshPart {
            name,
            region: None,
            kind,
            material,
            wheel: Some(wheel),
            hub_offset_mm: off,
            positions_mm: to_mm(&self.pos),
            uvs: vec![32768; n * 2],
            indices: self.idx,
            skin_nodes: Vec::new(),
            skin_weights: Vec::new(),
        }
    }
}

/// Tire tread + sidewalls with a rounded shoulder, centred at hub-space `xc`.
fn tire(s: &mut Soup, xc: f32, hw: f32, r: f32, ri: f32) {
    let sw = r - ri;
    let prof = [
        (xc - hw, ri),
        (xc - hw, r - 0.3 * sw),
        (xc - 0.82 * hw, r),
        (xc + 0.82 * hw, r),
        (xc + hw, r - 0.3 * sw),
        (xc + hw, ri),
    ];
    for i in 0..SEG {
        let (a, b) = (seg_ang(i, SEG), seg_ang(i + 1, SEG));
        for k in 0..prof.len() - 1 {
            let (p0, p1) = (prof[k], prof[k + 1]);
            s.poly(&[
                at(p0.0, p0.1, a),
                at(p1.0, p1.1, a),
                at(p1.0, p1.1, b),
                at(p0.0, p0.1, b),
            ]);
        }
    }
}

/// Rim face at hub-space `x` (outer side sign `so`), rim radius `ri`.
fn rim_face(
    rim: &mut Soup,
    back: &mut Soup,
    style: RimStyle,
    x: f32,
    so: f32,
    ri: f32,
    front: bool,
) {
    let out = |d: f32| x + so * d;
    // Dark backing (brake / barrel) shows between the spokes.
    back.disc(out(-0.05), ri * 0.98, SEG);
    match style {
        RimStyle::Star5 | RimStyle::Spoke10 | RimStyle::Six | RimStyle::Mesh => {
            let (n, a0, a1, pair) = match style {
                RimStyle::Star5 => (5, 0.30, 0.17, 0.0),
                RimStyle::Spoke10 => (10, 0.13, 0.07, 0.0),
                RimStyle::Six => (6, 0.07, 0.05, 0.13),
                _ => (16, 0.06, 0.035, 0.0),
            };
            rim.annulus(out(0.0), ri * 0.86, ri);
            rim.disc(out(0.012), ri * 0.24, 10);
            for i in 0..n {
                let c = seg_ang(i, n);
                if pair > 0.0 {
                    for sgn in [-1.0, 1.0] {
                        rim.spoke(
                            out(0.008),
                            out(-0.006),
                            ri * 0.22,
                            ri * 0.87,
                            a0,
                            a1,
                            c + sgn * pair,
                            sgn * 0.04,
                        );
                    }
                } else {
                    rim.spoke(
                        out(0.008),
                        out(-0.006),
                        ri * 0.22,
                        ri * 0.87,
                        a0,
                        a1,
                        c,
                        0.0,
                    );
                }
            }
            if style == RimStyle::Mesh {
                rim.annulus(out(0.002), ri * 0.5, ri * 0.56);
            }
        }
        RimStyle::Turbine => {
            rim.annulus(out(0.0), ri * 0.88, ri);
            rim.disc(out(0.015), ri * 0.26, 12);
            for i in 0..12 {
                rim.spoke(
                    out(0.01),
                    out(-0.004),
                    ri * 0.24,
                    ri * 0.89,
                    0.12,
                    0.13,
                    seg_ang(i, 12),
                    0.35,
                );
            }
        }
        RimStyle::Dish => {
            rim.annulus(out(0.0), ri * 0.62, ri);
            rim.band(out(0.0), ri * 0.62, out(-0.045), ri * 0.58);
            rim.disc(out(-0.02), ri * 0.2, 10);
            for i in 0..8 {
                rim.spoke(
                    out(-0.025),
                    out(-0.045),
                    ri * 0.18,
                    ri * 0.6,
                    0.16,
                    0.12,
                    seg_ang(i, 8),
                    0.0,
                );
            }
        }
        RimStyle::Steel => {
            rim.annulus(out(-0.01), ri * 0.3, ri * 0.97);
            rim.band(out(-0.01), ri * 0.3, out(0.005), ri * 0.24);
            back.disc(out(0.01), ri * 0.16, 8);
            // Vent holes: dark slots on the disc.
            for i in 0..6 {
                let c = seg_ang(i, 6) + 0.5;
                back.spoke(
                    out(-0.007),
                    out(-0.007),
                    ri * 0.52,
                    ri * 0.72,
                    0.12,
                    0.1,
                    c,
                    0.0,
                );
            }
        }
        RimStyle::Hubcap => {
            rim.annulus(out(0.0), ri * 0.3, ri * 0.96);
            rim.band(out(0.0), ri * 0.3, out(0.04), ri * 0.14);
            rim.disc(out(0.04), ri * 0.14, 10);
            for i in 0..10 {
                back.spoke(
                    out(0.003),
                    out(0.003),
                    ri * 0.6,
                    ri * 0.85,
                    0.07,
                    0.06,
                    seg_ang(i, 10),
                    0.0,
                );
            }
        }
        RimStyle::Truck => {
            rim.annulus(out(-0.02), ri * 0.32, ri * 0.97);
            for i in 0..8 {
                let c = seg_ang(i, 8) + 0.2;
                back.spoke(
                    out(-0.017),
                    out(-0.017),
                    ri * 0.5,
                    ri * 0.75,
                    0.13,
                    0.11,
                    c,
                    0.0,
                );
            }
            if front {
                rim.cone(out(-0.02), ri * 0.32, out(0.06), ri * 0.2);
                rim.disc(out(0.06), ri * 0.2, 10);
            } else {
                rim.band(out(-0.02), ri * 0.32, out(-0.06), ri * 0.28);
                rim.disc(out(-0.06), ri * 0.28, 10);
            }
        }
    }
}

impl Soup {
    fn cone(&mut self, x0: f32, r0: f32, x1: f32, r1: f32) {
        self.band(x0, r0, x1, r1);
    }
}

/// Every wheel part of a car: per wheel a tire, a rim and a dark backing, plus duplicates for
/// dual rear tires and visual extra axles.
pub fn wheel_parts(def: &VehicleDef, style: &CarStyle) -> Vec<MeshPart> {
    let ws = &style.wheels;
    let finish: MaterialSlot = ws.finish.into();
    let mut out = Vec::new();
    for (i, w) in def.wheels.iter().enumerate() {
        let index = i as u8;
        let front = i < 2;
        let so = if i % 2 == 0 { -1.0 } else { 1.0 };
        let r = w.radius_m;
        let ri = r * ws.rim_frac;
        let hw = 0.5 * w.width_m;
        let offsets: Vec<f32> = if front || ws.extra_axles.is_empty() {
            vec![0.0]
        } else {
            ws.extra_axles.clone()
        };
        for (ai, &dy) in offsets.iter().enumerate() {
            let stem = if ai == 0 {
                format!("wheel_{}", WHEEL_NAMES[i])
            } else {
                format!("wheel_{}_a{ai}", WHEEL_NAMES[i])
            };
            let off = if dy == 0.0 {
                None
            } else {
                Some([0, (dy * 1000.0).round() as i32, 0])
            };
            let mut t = Soup::default();
            let mut rim = Soup::default();
            let mut back = Soup::default();
            let dual = !front && ws.dual_rear;
            if dual {
                // Two tires side by side; the rim face shows on the outer one.
                let tw = 0.47 * hw;
                tire(&mut t, -0.5 * hw, tw, r, ri);
                tire(&mut t, 0.5 * hw, tw, r, ri);
                rim_face(
                    &mut rim,
                    &mut back,
                    ws.rim,
                    so * (0.5 * hw + 0.3 * tw),
                    so,
                    ri,
                    false,
                );
                back.disc(-so * (0.5 * hw + 0.3 * tw), ri * 0.98, SEG);
            } else {
                tire(&mut t, 0.0, hw, r, ri);
                rim_face(&mut rim, &mut back, ws.rim, so * hw * 0.62, so, ri, front);
                back.disc(-so * hw * 0.62, ri * 0.98, SEG);
            }
            if ws.white_letters {
                let mut wl = Soup::default();
                wl.annulus(so * (hw + 0.002), ri + 0.35 * (r - ri), ri + 0.6 * (r - ri));
                out.push(wl.part(
                    format!("{stem}_letters"),
                    PartKind::Tire,
                    MaterialSlot::Plate,
                    index,
                    off,
                ));
            }
            out.push(t.part(
                format!("{stem}_tire"),
                PartKind::Tire,
                MaterialSlot::Rubber,
                index,
                off,
            ));
            out.push(rim.part(format!("{stem}_rim"), PartKind::Rim, finish, index, off));
            out.push(back.part(
                format!("{stem}_hub"),
                PartKind::Rim,
                MaterialSlot::Trim,
                index,
                off,
            ));
        }
    }
    out
}
