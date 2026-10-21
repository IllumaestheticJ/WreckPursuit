//! The lofted outer shell: cross-section rings of a styled [`BodyProfile`] and surface
//! queries on them (where is the nose / tail / side / top at a given point), so lamps,
//! grilles, bumpers and every other bolt-on part sit *on* the body instead of inside it.

use wp_contracts::BodyRegion;

use crate::mesh::MaterialSlot;
use crate::profile::BodyProfile;
use crate::style::{BedKind, CarStyle};

/// Points per half cross-section (bottom → centre line on top).
pub const RING_N: usize = 11;

/// A styled body: profile plus derived surface queries.
pub struct Shell {
    /// Profile (anchors + shape knobs).
    pub p: BodyProfile,
    /// Rear glass is tinted.
    pub dark_rear: bool,
}

impl Shell {
    /// Shell of a profile.
    pub fn new(p: BodyProfile, style: &CarStyle) -> Shell {
        Shell {
            p,
            dark_rear: style.parts.dark_rear_glass,
        }
    }

    /// Glass material for a side window of `region`.
    pub fn side_glass(&self, region: BodyRegion) -> MaterialSlot {
        let rear = matches!(
            region,
            BodyRegion::Cabin | BodyRegion::LeftDoorR | BodyRegion::RightDoorR
        );
        if self.dark_rear && rear {
            MaterialSlot::GlassDark
        } else {
            MaterialSlot::Glass
        }
    }

    /// Left half cross-section at `y` (x ≤ 0), bottom → top; the last point is on the centre
    /// line. Faces between points: 0 rocker chamfer, 1–4 lower side (crease at point 3),
    /// 5 shoulder ledge, 6 greenhouse side (or hood/deck edge), 7 roof edge, 8–9 top.
    pub fn ring(&self, y: f32) -> [[f32; 3]; RING_N] {
        let p = &self.p;
        let s = &p.s;
        if (p.in_bed(y) && s.bed == BedKind::Rails) || p.in_bed_gap(y) {
            let w = if s.bed == BedKind::Rails {
                p.half_width(y)
            } else {
                0.42 * p.half_w
            };
            let top = if s.bed == BedKind::Rails {
                s.bed_height
            } else {
                p.z_door_bottom + 0.2
            };
            let bot = (top - 0.32).max(p.sill(y));
            let zs = [
                bot,
                bot + 0.03,
                bot + 0.1,
                bot + 0.16,
                top - 0.08,
                top - 0.02,
            ];
            let mut r = [[0.0; 3]; RING_N];
            r[0] = [-(w - 0.03), y, zs[0]];
            for (i, &z) in zs.iter().enumerate().skip(1) {
                r[i] = [-w, y, z];
            }
            r[6] = [-(w - 0.03), y, top];
            r[7] = [-0.7 * w, y, top];
            r[8] = [-0.45 * w, y, top];
            r[9] = [-0.2 * w, y, top];
            r[10] = [0.0, y, top];
            return r;
        }
        let w = p.half_width(y);
        let f = p.flare(y);
        let ws = w + f;
        let sill = p.sill(y);
        let belt = p.belt(y);
        let rk = s.rocker;
        let shd = s.shoulder;
        let zc = sill + s.crease_at * (belt - sill);
        let mut r = [[0.0f32; 3]; RING_N];
        r[0] = [-(ws - rk), y, sill];
        r[1] = [-ws, y, sill + rk];
        r[2] = [-ws, y, sill + 0.5 * (zc - sill)];
        r[3] = [-(ws + s.crease), y, zc];
        r[4] = [-(w + 0.5 * f), y, belt - shd];
        r[5] = [-(w - 0.5 * shd), y, belt];
        if p.in_bed(y) {
            match s.bed {
                BedKind::Open => {
                    let floor = (belt - 0.46).max(sill + 0.15);
                    let iw = w - 0.1;
                    r[6] = [-iw, y, belt];
                    r[7] = [-iw, y, floor];
                    r[8] = [-0.6 * iw, y, floor];
                    r[9] = [-0.3 * iw, y, floor];
                    r[10] = [0.0, y, floor];
                }
                BedKind::Box => {
                    let top = p.top(y);
                    r[4] = [-ws, y, belt - shd];
                    r[5] = [-ws, y, belt];
                    r[6] = [-ws, y, belt + 0.02];
                    r[7] = [-ws, y, top - 0.04];
                    r[8] = [-(ws - 0.04), y, top];
                    r[9] = [-0.5 * ws, y, top + 0.005];
                    r[10] = [0.0, y, top + 0.01];
                }
                BedKind::Tipper => {
                    let top = p.top(y);
                    let floor = belt + 0.08;
                    r[4] = [-ws, y, belt - shd];
                    r[5] = [-ws, y, belt];
                    r[6] = [-(ws + 0.02), y, belt + 0.5 * (top - belt)];
                    r[7] = [-(ws + 0.02), y, top];
                    r[8] = [-(ws - 0.06), y, top];
                    r[9] = [-(ws - 0.08), y, floor];
                    r[10] = [0.0, y, floor];
                }
                BedKind::Rails | BedKind::Closed => {}
            }
        } else if p.has_greenhouse(y) {
            let top = p.top(y);
            let rw = p.roof_half_w();
            let gb = w - s.tumblehome;
            let z7 = (top - 0.06).max(belt + 0.02);
            r[6] = [-gb, y, belt + 0.01];
            r[7] = [-rw.min(gb - 0.005), y, z7];
            r[8] = [-(rw - 0.05).min(gb - 0.05), y, top.max(z7 + 0.005)];
            r[9] = [-0.45 * rw, y, top + 0.7 * s.roof_crown];
            r[10] = [0.0, y, top + s.roof_crown];
        } else {
            let top = p.top(y);
            let bulge = p.hood_bulge(y);
            let inner = w - shd;
            r[6] = [-(inner - 0.02), y, belt + 0.012];
            r[7] = [-0.75 * inner, y, lerp_top(belt, top, 0.4) + 0.3 * bulge];
            r[8] = [-0.5 * inner, y, lerp_top(belt, top, 0.7) + 0.75 * bulge];
            r[9] = [-0.25 * inner, y, top + 0.6 * s.roof_crown + 0.95 * bulge];
            r[10] = [0.0, y, top + s.roof_crown + bulge];
        }
        let raised =
            p.in_bed(y) && matches!(s.bed, BedKind::Box | BedKind::Tipper) && s.bed_floor > 0.0;
        if raised {
            // Body on the chassis: rails below the floor, the box steps out at the floor.
            let bf = s.bed_floor;
            let fw = 0.42 * p.half_w;
            r[0] = [-(fw - 0.03), y, bf - 0.32];
            r[1] = [-fw, y, bf - 0.29];
            r[2] = [-fw, y, bf - 0.02];
            r[3] = [-ws, y, bf];
            return r;
        }
        // Wheel arches: side points below the arch move up onto it.
        if let Some(arch) = p.arch_floor(y) {
            let arch = arch.min(belt - shd - 0.03);
            for pt in r.iter_mut().take(4) {
                if pt[2] < arch {
                    pt[2] = arch;
                    pt[0] = -ws;
                }
            }
        }
        r
    }

    /// Full (both sides) cross-section polygon at `y` in (x, z). An open bed counts as
    /// solid (so lamps and plates project onto the tailgate, not into the bed).
    fn section(&self, y: f32) -> Vec<[f32; 2]> {
        let mut r = self.ring(y);
        if self.p.in_bed(y) && self.p.s.bed == BedKind::Open {
            let z = r[5][2];
            for (i, k) in (6..RING_N).enumerate() {
                r[k] = [r[5][0] * (1.0 - 0.25 * (i + 1) as f32).max(0.0), y, z];
            }
        }
        let mut poly: Vec<[f32; 2]> = r.iter().map(|q| [q[0], q[2]]).collect();
        for q in r.iter().rev().skip(1) {
            poly.push([-q[0], q[2]]);
        }
        poly
    }

    /// Point (x, z) inside the cross-section at `y`.
    pub fn inside(&self, y: f32, x: f32, z: f32) -> bool {
        let poly = self.section(y);
        let mut c = false;
        let n = poly.len();
        for i in 0..n {
            let a = poly[i];
            let b = poly[(i + 1) % n];
            if (a[1] > z) != (b[1] > z) {
                let xi = a[0] + (z - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
                if x < xi {
                    c = !c;
                }
            }
        }
        c
    }

    /// Nose surface y at (x, z): the most forward station whose section contains the point.
    pub fn front_y(&self, x: f32, z: f32) -> Option<f32> {
        let p = &self.p;
        let mut y = p.y_nose;
        let stop = p.y_nose - 1.6;
        while y > stop {
            if self.inside(y, x, z) {
                return Some(y);
            }
            y -= 0.004;
        }
        None
    }

    /// Tail surface y at (x, z).
    pub fn rear_y(&self, x: f32, z: f32) -> Option<f32> {
        let p = &self.p;
        let mut y = p.y_tail;
        let stop = p.y_tail + 1.6;
        while y < stop {
            if self.inside(y, x, z) {
                return Some(y);
            }
            y += 0.004;
        }
        None
    }

    /// Outer side |x| at (y, z) (largest crossing of the section at height z).
    pub fn side_x(&self, y: f32, z: f32) -> Option<f32> {
        let r = self.ring(y);
        let mut best: Option<f32> = None;
        for i in 0..RING_N - 1 {
            let (a, b) = (r[i], r[i + 1]);
            if (a[2] - z) * (b[2] - z) <= 0.0 && (a[2] - b[2]).abs() > 1e-6 {
                let t = (z - a[2]) / (b[2] - a[2]);
                let x = -(a[0] + t * (b[0] - a[0]));
                best = Some(best.map_or(x, |m: f32| m.max(x)));
            } else if (a[2] - z).abs() < 1e-6 {
                best = Some(best.map_or(-a[0], |m: f32| m.max(-a[0])));
            }
        }
        best
    }

    /// Top surface z at (|x|, y) (highest crossing of the section at that x).
    pub fn top_z(&self, x: f32, y: f32) -> f32 {
        let r = self.ring(y);
        let ax = -x.abs();
        let mut best = f32::MIN;
        for i in 3..RING_N - 1 {
            let (a, b) = (r[i], r[i + 1]);
            let (lo, hi) = (a[0].min(b[0]), a[0].max(b[0]));
            if ax >= lo - 1e-6 && ax <= hi + 1e-6 {
                let t = if (b[0] - a[0]).abs() < 1e-6 {
                    1.0
                } else {
                    (ax - a[0]) / (b[0] - a[0])
                };
                best = best.max(a[2] + t * (b[2] - a[2]));
            }
        }
        if best == f32::MIN {
            r[RING_N - 1][2]
        } else {
            best
        }
    }

    /// Plan outline at height `z` around the nose (`front`) or tail: a path from the left
    /// side `wrap` m back from the end, across the end face, to the right side. Each point
    /// comes with its outward plan normal.
    pub fn end_path(&self, z: f32, wrap: f32, front: bool) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
        let p = &self.p;
        let (y_end, dir) = if front {
            (p.y_nose, 1.0)
        } else {
            (p.y_tail, -1.0)
        };
        let reach = (if front { p.s.nose_len } else { p.s.tail_len }).max(0.1) + wrap;
        let n = 10;
        let mut left = Vec::new();
        for i in 0..=n {
            let y = y_end - dir * reach * (1.0 - i as f32 / n as f32);
            if let Some(x) = self.side_x(y, z) {
                left.push([-x, y, z]);
            }
        }
        let mut path: Vec<[f32; 3]> = left.clone();
        // Across the end face.
        if let Some(&last) = left.last() {
            let span = -last[0];
            let m = 6;
            for i in 1..m {
                let x = -span + 2.0 * span * i as f32 / m as f32;
                path.push([x, y_end, z]);
            }
        }
        for q in left.iter().rev() {
            path.push([-q[0], q[1], q[2]]);
        }
        if !front {
            path.reverse();
        }
        let normals = (0..path.len())
            .map(|i| {
                let a = path[i.saturating_sub(1)];
                let b = path[(i + 1).min(path.len() - 1)];
                let t = [b[0] - a[0], b[1] - a[1], 0.0];
                // Path runs left → right across the nose (right → left across the tail):
                // outward is the tangent turned counter-clockwise seen from above.
                let n = [-t[1], t[0], 0.0];
                crate::mesh::norm(n)
            })
            .collect();
        (path, normals)
    }
}

fn lerp_top(belt: f32, top: f32, t: f32) -> f32 {
    belt + (top - belt) * t
}
