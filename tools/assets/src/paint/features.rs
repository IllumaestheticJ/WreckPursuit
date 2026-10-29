//! Texel painters: what colour a texel of the detail texture gets, from the part it shows, its
//! view, its rest position and the car's trim ([`Env`]). Features are drawn in car metres in the
//! view the texel belongs to (front view `(x, z)`, sides `(y, z)`, top `(x, y)`), clipped to the
//! texel's own part, so a grille never bleeds onto a lamp. A texel's alpha is the paint mask:
//! 1 = body paint (RGB = shading multiplier), 0 = the colour itself.
//!
//! Families: [`skyline_r34`] (layers `gtr_front`, `gt_front`, `gtr_badges`, `gtt_badges`,
//! `gt_badges`, `fender_vent`, `gtr_rear`, `gt_rear`, `vspec_diffuser`, `naca_duct`). Every
//! other family falls back to [`generic`] (material colours, shaded paint).

use wp_contracts::{BodyRegion, VehicleDef};

use super::canvas::{hash01, text_hit, Rgba};
use super::layout::View;
use crate::mesh::{MaterialSlot, MeshPart, PartKind};
use crate::profile::BodyProfile;
use crate::section::Trim;

/// What every texel of one car shares.
pub struct Env<'a> {
    /// The car.
    pub def: &'a VehicleDef,
    /// Its styled profile (spec curves).
    pub prof: &'a BodyProfile,
    /// Its trim.
    pub trim: &'a Trim,
    /// Family id (`""` without a spec).
    pub family: &'a str,
    /// Trim key.
    pub trim_key: &'a str,
}

impl Env<'_> {
    /// True if the trim lists texture layer `name`.
    pub fn layer(&self, name: &str) -> bool {
        self.trim.texture_layers.iter().any(|l| l == name)
    }
}

/// One texel to colour.
pub struct Ctx<'a> {
    /// The part it shows.
    pub part: &'a MeshPart,
    /// Its island's view.
    pub view: View,
    /// True in a car-wide body frame.
    pub frame: bool,
    /// Rest position (chassis space; hub space for wheels), m.
    pub p: [f32; 3],
    /// Face normal.
    pub n: [f32; 3],
    /// View coordinates of `p` ([`View::ab`]).
    pub ab: [f32; 2],
    /// Texel size here, m.
    pub m_per_px: f32,
    /// Texel coordinates.
    pub px: [u32; 2],
}

/// Body paint with shading multiplier `s`.
pub fn paint(s: f32) -> Rgba {
    [s, s, s, 1.0]
}

/// An opaque colour (sRGB 0..1).
pub fn rgb(r: f32, g: f32, b: f32) -> Rgba {
    [r, g, b, 0.0]
}

/// sRGB bytes as an opaque colour, times `k`.
pub fn rgb8(c: [u8; 3], k: f32) -> Rgba {
    [
        c[0] as f32 / 255.0 * k,
        c[1] as f32 / 255.0 * k,
        c[2] as f32 / 255.0 * k,
        0.0,
    ]
}

/// Scale a colour's RGB.
pub fn shade(c: Rgba, k: f32) -> Rgba {
    [c[0] * k, c[1] * k, c[2] * k, c[3]]
}

/// Point-in-polygon (even-odd), any winding.
pub fn in_poly(q: [f32; 2], poly: &[[f32; 2]]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > q[1]) != (b[1] > q[1]) {
            let x = a[0] + (q[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if q[0] < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Distance from `q` to the polygon's boundary, m (0 on it).
pub fn poly_edge_dist(q: [f32; 2], poly: &[[f32; 2]]) -> f32 {
    let n = poly.len();
    let mut d = f32::MAX;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        d = d.min(seg_dist(q, a, b));
    }
    d
}

/// Distance from `q` to the segment a–b.
pub fn seg_dist(q: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-12 {
        0.0
    } else {
        (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
    };
    let (px, py) = (a[0] + dx * t - q[0], a[1] + dy * t - q[1]);
    (px * px + py * py).sqrt()
}

/// Whether text `s` centred at `c` (same 2D space as `q`), `h` m tall (5×7 glyphs; the width
/// follows), covers `q`. `flip_x` mirrors it (a view whose first axis runs right to left).
pub fn text_at(s: &str, q: [f32; 2], c: [f32; 2], h: f32, flip_x: bool) -> bool {
    let n = s.chars().count();
    let w = h * (n * 6 - 1) as f32 / 7.0;
    let mut u = (q[0] - (c[0] - 0.5 * w)) / w;
    if flip_x {
        u = 1.0 - u;
    }
    let v = (c[1] + 0.5 * h - q[1]) / h;
    text_hit(s, u, v)
}

/// The colour of one texel (dispatch by family).
pub fn texel(env: &Env, c: &Ctx) -> Rgba {
    match env.family {
        "nissan_skyline_r34" => skyline_r34::texel(env, c),
        _ => generic(env, c),
    }
}

/// Default colour of a material (sRGB, opaque), for textured parts the family does not paint.
pub fn material(m: MaterialSlot) -> Rgba {
    use MaterialSlot as M;
    match m {
        M::Paint | M::Livery | M::Accent => paint(1.0),
        M::Trim => rgb(0.075, 0.075, 0.08),
        M::Grille => rgb(0.035, 0.035, 0.04),
        M::Chrome => rgb(0.78, 0.79, 0.80),
        M::Metal | M::Rim => rgb(0.62, 0.63, 0.65),
        M::Rubber => rgb(0.085, 0.085, 0.09),
        M::Carbon => rgb(0.12, 0.12, 0.13),
        M::Headlight => rgb(0.82, 0.84, 0.88),
        M::Taillight => rgb(0.62, 0.05, 0.06),
        M::EmissiveAmber => rgb(0.95, 0.55, 0.10),
        M::Exhaust => rgb(0.45, 0.42, 0.40),
        M::Glass => rgb(0.12, 0.15, 0.18),
        _ => rgb(0.3, 0.3, 0.32),
    }
}

/// Generic texel: material colour; body paint shaded by height (darker toward the sill).
pub fn generic(env: &Env, c: &Ctx) -> Rgba {
    let base = material(c.part.material);
    if base[3] < 0.5 {
        return base;
    }
    paint(body_shade(env.prof, c))
}

/// Baked body shading (multiplier): sides darken toward the sill and brighten in a band under
/// the shoulder; tops are bright, end faces a little darker. PS2-era "painted light".
pub fn body_shade(prof: &BodyProfile, c: &Ctx) -> f32 {
    let (y, z) = (c.p[1], c.p[2]);
    match c.view {
        View::Left | View::Right => {
            let sill = prof.sill(y);
            let belt = prof.belt(y).max(sill + 0.1);
            let t = ((z - sill) / (belt - sill)).clamp(0.0, 1.2);
            // Up-facing part of the side catches more light.
            let up = c.n[2].clamp(-1.0, 1.0);
            let band = (-((t - 0.86) / 0.07).powi(2)).exp();
            let base = 0.80 + 0.22 * t.min(1.0) + 0.10 * band + 0.10 * up;
            if z > belt + 0.005 {
                // Greenhouse side (pillars).
                0.95 + 0.08 * up
            } else {
                base
            }
        }
        View::Top => {
            let w = prof.half_width(y).max(0.3);
            let u = (c.p[0].abs() / w).clamp(0.0, 1.0);
            1.06 - 0.10 * u * u
        }
        View::Front | View::Rear => 0.90 + 0.10 * c.n[2].max(0.0),
        View::Bottom => 0.45,
        View::Wheel => 1.0,
    }
}

/// The R34 family's painter.
pub mod skyline_r34 {
    use super::*;
    use crate::styles::models::skyline_r34::{FRONT_SIGNAL, HEADLAMP, TAIL_INNER, TAIL_OUTER};

    /// Black plastic / mesh.
    const BLACK: Rgba = [0.045, 0.045, 0.05, 0.0];
    /// Chrome.
    const CHROME: Rgba = [0.80, 0.81, 0.83, 0.0];

    /// Height offset of the trim against the GT-R measurements, at the nose / tail.
    fn dz(env: &Env, front: bool) -> f32 {
        let p = env.prof;
        if front {
            p.belt(p.y_nose) - 0.62
        } else {
            p.belt(p.y_tail) - 0.90
        }
    }

    /// The texel's colour.
    pub fn texel(env: &Env, c: &Ctx) -> Rgba {
        let name = c.part.name.as_str();
        if c.part.wheel.is_some() {
            return wheel(env, c);
        }
        match c.part.kind {
            PartKind::Light => return lamp(env, c),
            PartKind::Mirror => return paint(body_shade(env.prof, c) * 1.02),
            _ => {}
        }
        if name.starts_with("mirror_glass") {
            let g = 0.55 + 0.35 * (c.p[2] * 9.0).fract();
            return rgb(g * 0.9, g * 0.95, g);
        }
        if name.starts_with("wing") {
            let s = if name == "wing_lower_blade" {
                0.80
            } else {
                1.0
            };
            let top = if c.view == View::Top { 1.06 } else { 0.86 };
            return paint(s * top);
        }
        let base = material(c.part.material);
        if base[3] < 0.5 {
            return base;
        }
        let s = body_shade(env.prof, c);
        let col = match c.view {
            View::Front => front(env, c),
            View::Rear => rear(env, c),
            View::Left | View::Right => side(env, c),
            View::Top => top(env, c),
            _ => None,
        };
        match col {
            Some(k) if k[3] > 0.5 => paint(s * k[0]),
            Some(k) => k,
            None => paint(s),
        }
    }

    // ---------------------------------------------------------------- front

    fn front(env: &Env, c: &Ctx) -> Option<Rgba> {
        let x = c.p[0].abs();
        let z = c.p[2] - dz(env, true);
        let q = [x, z];
        let gtr = env.layer("gtr_front");
        let px = c.m_per_px;
        // Lower lip: the chin's bottom few cm are black.
        let sill = env.prof.sill(c.p[1]) - dz(env, true);
        if z < sill + 0.040 {
            return Some(shade(BLACK, 1.5));
        }
        // Upper grille between the head lamps (honeycomb), the badge in its middle.
        let lamp_in = HEADLAMP[0][0].min(HEADLAMP[6][0]) - 0.012;
        let (gz0, gz1) = (HEADLAMP[0][1] + 0.004, HEADLAMP[6][1] - 0.004);
        if x < lamp_in && z > gz0 && z < gz1 {
            let edge = (lamp_in - x).min(z - gz0).min(gz1 - z);
            if edge < 1.5 * px {
                return Some(shade(BLACK, 0.6));
            }
            let badge_h = if gtr { 0.040 } else { 0.036 };
            let bc = [0.0, 0.5 * (gz0 + gz1)];
            if gtr {
                let bw = badge_h * 23.0 / 7.0 + 0.016;
                if (x).abs() < 0.5 * bw && (z - bc[1]).abs() < 0.5 * badge_h + 0.006 {
                    let rim = 0.5 * bw - x < px || (0.5 * badge_h + 0.006) - (z - bc[1]).abs() < px;
                    if rim {
                        return Some(CHROME);
                    }
                    if text_at("GT-R", [-c.p[0], z], bc, badge_h, false) {
                        // "GT" chrome, "-R" red (as seen from ahead).
                        return Some(if -c.p[0] > -0.004 {
                            rgb(0.82, 0.08, 0.08)
                        } else {
                            CHROME
                        });
                    }
                    return Some(rgb(0.10, 0.10, 0.11));
                }
            } else if (x).abs() < 0.035 && (z - bc[1]).abs() < 0.028 {
                // Round brand badge (25GT): chrome ring with a bar.
                let r = (x * x + (z - bc[1]).powi(2)).sqrt();
                return Some(if r > 0.030 || (z - bc[1]).abs() < 0.006 {
                    CHROME
                } else {
                    rgb(0.10, 0.12, 0.16)
                });
            }
            // Honeycomb: offset rows of dark cells with lighter walls.
            let cell = 0.012f32;
            let row = (z / cell).floor();
            let off = if row as i32 % 2 == 0 { 0.0 } else { 0.5 * cell };
            let fx = ((x + off) / cell).fract();
            let fz = (z / cell).fract();
            let wall = fx < 0.25 || fz < 0.25;
            return Some(if wall {
                rgb(0.14, 0.14, 0.15)
            } else {
                shade(BLACK, 0.7)
            });
        }
        if gtr {
            // Central intake (intercooler behind), trapezoid with chamfered corners.
            let intake = [
                [0.0, 0.245],
                [0.43, 0.245],
                [0.47, 0.29],
                [0.44, 0.535],
                [0.40, 0.558],
                [0.0, 0.558],
            ];
            if in_poly(q, &intake) {
                let e = poly_edge_dist(q, &intake);
                // Plate on a bar across the middle.
                if let Some(pc) = plate(q, [0.0, 0.405], c) {
                    return Some(pc);
                }
                if e < 0.010 {
                    return Some(shade(BLACK, 0.8));
                }
                if e < 0.022 {
                    return Some(shade(BLACK, 1.6));
                }
                // Intercooler core: fins (rows) and end tanks.
                let fin = ((z / (1.2 * px)).floor() as i32) % 2 == 0;
                let tank = x > 0.40 || !(0.27..=0.53).contains(&z);
                let shadow = 0.75 + 0.25 * ((z - 0.25) / 0.30).clamp(0.0, 1.0);
                return Some(if tank {
                    rgb(0.16, 0.16, 0.17)
                } else if fin {
                    shade(rgb(0.42, 0.43, 0.45), shadow)
                } else {
                    shade(rgb(0.24, 0.25, 0.26), shadow)
                });
            }
            // Outer brake / oil-cooler ducts under the signals.
            let duct = [
                [0.585, 0.265],
                [0.79, 0.29],
                [0.805, 0.42],
                [0.79, 0.565],
                [0.625, 0.565],
                [0.555, 0.40],
            ];
            if in_poly(q, &duct) {
                let e = poly_edge_dist(q, &duct);
                if e < 0.012 {
                    return Some(shade(BLACK, 0.7));
                }
                let mesh = ((x / 0.012).fract() < 0.3) || ((z / 0.012).fract() < 0.3);
                return Some(if mesh {
                    rgb(0.13, 0.13, 0.14)
                } else {
                    shade(BLACK, 0.6)
                });
            }
        } else {
            // 25GT / GT-T: a narrower upper grille slot and one wide lower intake with slats.
            let intake = [
                [0.0, 0.25],
                [0.52, 0.25],
                [0.56, 0.30],
                [0.52, 0.44],
                [0.0, 0.44],
            ];
            if in_poly(q, &intake) {
                if let Some(pc) = plate(q, [0.0, 0.345], c) {
                    return Some(pc);
                }
                let e = poly_edge_dist(q, &intake);
                if e < 0.01 {
                    return Some(shade(BLACK, 0.7));
                }
                let slat = ((z / 0.03).fract()) < 0.3;
                return Some(if slat {
                    rgb(0.16, 0.16, 0.17)
                } else {
                    shade(BLACK, 0.8)
                });
            }
            // Round fog lamps in the corners.
            let (fx, fz, fr) = (0.66, 0.36, 0.045);
            let r = ((x - fx).powi(2) + (z - fz).powi(2)).sqrt();
            if r < fr {
                return Some(if r > fr - 0.008 {
                    shade(BLACK, 1.2)
                } else {
                    rgb(0.78, 0.80, 0.82)
                });
            }
        }
        // A dark recess under the signal and round the head lamp's lower edge.
        let s = FRONT_SIGNAL;
        if x > s[0][0] - 0.01 && x < s[1][0] + 0.01 && z > s[0][1] - 0.01 && z < s[2][1] + 0.01 {
            return Some(shade(BLACK, 1.0));
        }
        None
    }

    /// A Japanese plate (330 × 165 mm) centred at `pc` in a front / rear view (q = (|x|, z)).
    fn plate(q: [f32; 2], pc: [f32; 2], c: &Ctx) -> Option<Rgba> {
        let (hw, hh) = (0.165, 0.0825);
        let x = if c.view == View::Rear {
            c.p[0]
        } else {
            -c.p[0]
        };
        let (dx, dz) = (x - pc[0], q[1] - pc[1]);
        if dx.abs() > hw || dz.abs() > hh {
            return None;
        }
        let px = c.m_per_px;
        if hw - dx.abs() < px || hh - dz.abs() < px {
            return Some(rgb(0.10, 0.30, 0.16));
        }
        let green = rgb(0.06, 0.32, 0.16);
        if text_at("34", [dx, dz], [0.0, 0.048], 0.030, false) {
            return Some(green);
        }
        if text_at("34-34", [dx, dz], [0.03, -0.018], 0.075, false) {
            return Some(green);
        }
        if text_at("R", [dx, dz], [-0.115, -0.018], 0.040, false) {
            return Some(green);
        }
        Some(rgb(0.93, 0.93, 0.90))
    }

    // ---------------------------------------------------------------- rear

    fn rear(env: &Env, c: &Ctx) -> Option<Rgba> {
        let x = c.p[0];
        let ax = x.abs();
        let z = c.p[2] - dz(env, false);
        let px = c.m_per_px;
        let gtr = env.layer("gtr_rear");
        // Dark wells round the round lamps.
        for (cx, cz, r) in [TAIL_OUTER, TAIL_INNER] {
            let d = ((ax - cx).powi(2) + (z - cz).powi(2)).sqrt();
            if d < r + 0.006 {
                return Some(shade(BLACK, 1.6));
            }
        }
        // Brand badge between the inner lamps, under the trunk lid's edge.
        let (bz, br) = (0.845, 0.034);
        let d = (x * x + ((z - bz) * 1.25).powi(2)).sqrt();
        if d < br {
            return Some(if d > br - 1.2 * px || (z - bz).abs() < 0.006 {
                CHROME
            } else {
                rgb(0.12, 0.14, 0.18)
            });
        }
        // Trim badge on the lid's lower right (seen from behind).
        let badge = if env.layer("gtr_badges") {
            Some(("GT-R", 0.040))
        } else if env.layer("gtt_badges") {
            Some(("GT-T", 0.034))
        } else if env.layer("gt_badges") {
            Some(("25GT", 0.032))
        } else {
            None
        };
        if let Some((s, h)) = badge {
            let at = [0.29, 0.80];
            if text_at(s, [x, z], at, h, false) {
                return Some(if s == "GT-R" && x > at[0] + 0.01 {
                    rgb(0.80, 0.07, 0.07)
                } else {
                    CHROME
                });
            }
        }
        if text_at("SKYLINE", [x, z], [0.0, 0.715], 0.026, false) {
            return Some(CHROME);
        }
        // Plate recess and plate on the bumper.
        let pc = [0.0, 0.505];
        if ax < 0.20 && (z - pc[1]).abs() < 0.105 {
            if let Some(p) = plate([ax, z], pc, c) {
                return Some(p);
            }
            // Recess walls and plate lamps.
            if z > pc[1] + 0.09 && ax < 0.06 {
                return Some(rgb(0.85, 0.85, 0.8));
            }
            return Some(shade(BLACK, 2.2));
        }
        // Reflectors in the bumper corners.
        if ax > 0.66 && ax < 0.79 && (z - 0.52).abs() < 0.016 {
            return Some(rgb(0.55, 0.04, 0.05));
        }
        // Lower valance / diffuser (black), the exhaust's cut-out on the left.
        let ex = [-0.56, 0.30];
        let de = (((x - ex[0]) / 0.085).powi(2) + ((z - ex[1]) / 0.06).powi(2)).sqrt();
        if de < 1.0 {
            return Some(shade(BLACK, 0.5));
        }
        if z < 0.37 {
            if env.layer("vspec_diffuser") && ax < 0.50 {
                // Carbon weave with vertical fences.
                if ((ax / 0.10).fract()) < 0.08 {
                    return Some(rgb(0.20, 0.20, 0.22));
                }
                let w = ((c.px[0] / 2 + c.px[1] / 2) % 2) as f32;
                return Some(rgb(0.07 + 0.04 * w, 0.07 + 0.04 * w, 0.08 + 0.04 * w));
            }
            if gtr && ax < 0.46 && z < 0.355 {
                // Diffuser: vertical fences in the black valance.
                if ((ax / 0.11).fract()) < 0.07 {
                    return Some(shade(BLACK, 3.0));
                }
                return Some(shade(BLACK, 0.8));
            }
            return Some(shade(BLACK, 1.6));
        }
        if z < 0.375 {
            return Some(shade(BLACK, 0.7));
        }
        // Trunk lock under the badge.
        if ax < 0.012 && (z - 0.79).abs() < 0.008 {
            return Some(shade(BLACK, 2.0));
        }
        None
    }

    // ---------------------------------------------------------------- sides

    fn side(env: &Env, c: &Ctx) -> Option<Rgba> {
        let p = env.prof;
        let (y, z) = (c.p[1], c.p[2]);
        let right = c.view == View::Right;
        let px = c.m_per_px;
        let region = c.part.region;
        let door = matches!(region, Some(BodyRegion::LeftDoorF | BodyRegion::RightDoorF));
        // Door handle: a pull handle near the door's rear top edge.
        if door {
            let hy = p.y_fd_rear + 0.17;
            let hz = p.belt(hy) - 0.075;
            let (dy, dz) = (y - hy, z - hz);
            if dy.abs() < 0.085 && dz.abs() < 0.022 {
                let rr = (dy / 0.085).powi(2) + (dz / 0.022).powi(2);
                if rr < 1.0 {
                    if dz > 0.004 && dy.abs() < 0.07 {
                        // The handle itself (body colour, lit from above).
                        return Some(paint(1.18));
                    }
                    return Some(shade(BLACK, 1.2));
                }
            }
            // Key cylinder.
            let kd = ((y - (hy + 0.115)).powi(2) + (z - hz).powi(2)).sqrt();
            if kd < 0.009 {
                return Some(if kd < 0.006 {
                    CHROME
                } else {
                    shade(BLACK, 1.5)
                });
            }
        }
        // Belt moulding on the door / quarter tops.
        if z > p.belt(y) - 0.006 && !matches!(region, Some(BodyRegion::Cabin)) {
            return Some(shade(BLACK, 1.4));
        }
        // GT-R emblem on the front fender, behind the arch, low.
        if env.layer("gtr_badges")
            && matches!(
                region,
                Some(BodyRegion::FrontLeftFender | BodyRegion::FrontRightFender)
            )
        {
            if let Some(&(wy, wz, r)) = p.arches.iter().find(|a| a.0 > 0.0) {
                let at = [wy - r - 0.09, wz + 0.08];
                let flip = !right;
                let q = [if flip { -y } else { y }, z];
                let c0 = [if flip { -at[0] } else { at[0] }, at[1]];
                if text_at("GT-R", q, c0, 0.036, false) {
                    let rr = if flip {
                        -y > -at[0] + 0.012
                    } else {
                        y > at[0] + 0.012
                    };
                    return Some(if rr { rgb(0.80, 0.07, 0.07) } else { CHROME });
                }
            }
        }
        // Brake-cooling slot in the bumper side ahead of the front wheel (GT-R).
        if env.layer("gtr_front") && region.is_some_and(|r| r == BodyRegion::FrontBumper) {
            if let Some(&(wy, wz, r)) = p.arches.iter().find(|a| a.0 > 0.0) {
                let (y0, y1) = (wy + r + 0.05, wy + r + 0.13);
                if y > y0 && y < y1 && z > wz - 0.06 && z < wz + 0.16 {
                    let e = (y - y0).min(y1 - y).min(z - (wz - 0.06)).min(wz + 0.16 - z);
                    let mesh = ((z / 0.012).fract() < 0.3) || ((y / 0.012).fract() < 0.3);
                    return Some(if e < 1.2 * px {
                        shade(BLACK, 0.6)
                    } else if mesh {
                        rgb(0.13, 0.13, 0.14)
                    } else {
                        shade(BLACK, 0.7)
                    });
                }
            }
        }
        // Front fender outlet behind the wheel (aftermarket layer).
        if env.layer("fender_vent")
            && matches!(
                region,
                Some(BodyRegion::FrontLeftFender | BodyRegion::FrontRightFender)
            )
        {
            if let Some(&(wy, wz, r)) = p.arches.iter().find(|a| a.0 > 0.0) {
                let y1 = wy - r - 0.035;
                let (y0, z0, z1) = (y1 - 0.16, wz + 0.20, wz + 0.29);
                // Parallelogram leaning back.
                let lean = (z - z0) * 0.35;
                if y < y1 - lean && y > y0 - lean && z > z0 && z < z1 {
                    let fz = (z - z0) / (z1 - z0);
                    let slat = (fz * 3.0).fract() < 0.34;
                    let edge = (y1 - lean - y).min(y - (y0 - lean)).min(z - z0).min(z1 - z);
                    return Some(if edge < 1.2 * px {
                        shade(BLACK, 0.7)
                    } else if slat {
                        paint(0.9)
                    } else {
                        shade(BLACK, 1.0)
                    });
                }
            }
        }
        // Fuel door on the right rear quarter, over the arch.
        if right && matches!(region, Some(BodyRegion::RearRightQuarter)) {
            let (fy, fz, hw, hh) = (-1.36, p.belt(-1.36) - 0.13, 0.085, 0.062);
            let (dy, dz) = ((y - fy).abs(), (z - fz).abs());
            let r = 0.02;
            let qx = (dy - (hw - r)).max(0.0);
            let qz = (dz - (hh - r)).max(0.0);
            let dd = (qx * qx + qz * qz).sqrt() - r;
            if dd.abs() < 0.6 * px {
                return Some(shade(BLACK, 1.0));
            }
            if dd < 0.0 && dd > -1.6 * px {
                return Some(paint(0.80));
            }
        }
        // Side skirt: darker lower edge between the arches.
        let sill = p.sill(y);
        if z < sill + 0.018 && y.abs() < 1.0 {
            return Some(shade(BLACK, 1.8));
        }
        if (z - (sill + 0.085)).abs() < 0.5 * px && y.abs() < 0.95 {
            return Some(paint(0.70));
        }
        None
    }

    // ---------------------------------------------------------------- tops

    fn top(env: &Env, c: &Ctx) -> Option<Rgba> {
        let p = env.prof;
        let (x, y) = (c.p[0], c.p[1]);
        let region = c.part.region;
        // Belt moulding on the door / quarter tops (the ledge under the side glass).
        if matches!(
            region,
            Some(
                BodyRegion::LeftDoorF
                    | BodyRegion::RightDoorF
                    | BodyRegion::LeftDoorR
                    | BodyRegion::RightDoorR
            )
        ) && c.p[2] > p.belt(y) - 0.004
        {
            return Some(shade(BLACK, 1.4));
        }
        if env.layer("naca_duct") && region == Some(BodyRegion::Hood) {
            // NACA duct left of centre, opening toward the windshield.
            let (y0, y1) = (p.y_bf - 0.62, p.y_bf - 0.30);
            if y > y0 && y < y1 {
                let t = (y1 - y) / (y1 - y0);
                let hw = 0.018 + 0.06 * t * t;
                let xc = -0.22;
                if (x - xc).abs() < hw {
                    let k = 0.25 + 0.5 * (1.0 - t);
                    return Some(rgb(0.05 * k + 0.02, 0.05 * k + 0.02, 0.055 * k + 0.02));
                }
            }
        }
        let _ = hash01;
        None
    }

    // ---------------------------------------------------------------- lamps

    fn lamp(env: &Env, c: &Ctx) -> Rgba {
        let name = c.part.name.as_str();
        let px = c.m_per_px;
        if name.starts_with("headlight") {
            let x = c.p[0].abs();
            let z = c.p[2] - dz(env, true);
            if c.view != View::Front {
                return rgb(0.30, 0.31, 0.33);
            }
            // Lower chrome strip.
            let bottom = HEADLAMP[0][1] + (HEADLAMP[2][1] - HEADLAMP[0][1]) * ((x - 0.372) / 0.42);
            if z < bottom + 0.010 {
                return rgb(0.58, 0.59, 0.61);
            }
            // Inner projector (low beam): dark lens in a chrome ring.
            let (pxc, pzc, pr) = (0.47, 0.733, 0.040);
            let d = ((x - pxc).powi(2) + (z - pzc).powi(2)).sqrt();
            if d < pr {
                if d > pr - 2.0 * px {
                    return CHROME;
                }
                let hi = ((x - pxc + 0.012).powi(2) + (z - pzc - 0.012).powi(2)).sqrt() < 0.008;
                return if hi {
                    rgb(0.80, 0.84, 0.92)
                } else {
                    rgb(0.10, 0.12, 0.17)
                };
            }
            // Outer reflector (high beam): chrome bowl with a dark bulb.
            let (rxc, rzc, rr) = (0.665, 0.740, 0.044);
            let d = ((x - rxc).powi(2) + (z - rzc).powi(2)).sqrt();
            if d < rr {
                if d < 0.008 {
                    return rgb(0.22, 0.22, 0.24);
                }
                let k = 0.98 - 0.45 * (d / rr);
                return rgb(k, k, k * 1.02);
            }
            // Housing: dark chrome, lighter toward the top, a few lens flutes.
            let t = ((z - bottom) / 0.10).clamp(0.0, 1.0);
            let flute = ((x / 0.02).fract() < 0.12) as i32 as f32;
            let k = 0.28 + 0.22 * t + 0.06 * flute;
            return rgb(k, k * 1.01, k * 1.04);
        }
        if name.starts_with("indicator_front") {
            let rib = ((c.p[0].abs() / 0.008).fract() < 0.4) as i32 as f32;
            return rgb(0.95 - 0.12 * rib, 0.56 - 0.08 * rib, 0.08);
        }
        if name.starts_with("taillight") || name.starts_with("indicator_rear") {
            let inner = name.starts_with("indicator_rear");
            let (cx, cz, r) = if inner { TAIL_INNER } else { TAIL_OUTER };
            let z = c.p[2] - dz(env, false);
            let d = ((c.p[0].abs() - cx).powi(2) + (z - cz).powi(2)).sqrt() / r;
            if c.view != View::Rear {
                return rgb(0.45, 0.05, 0.06);
            }
            if d > 0.90 {
                return CHROME;
            }
            if inner && d < 0.40 {
                // Clear reverse-lamp centre with a bulb.
                return if d < 0.10 {
                    rgb(0.55, 0.55, 0.55)
                } else {
                    rgb(0.86, 0.86, 0.88)
                };
            }
            if !inner && (d - 0.56).abs() < 0.07 {
                // The bright inner ring of the outer lamps.
                return rgb(0.98, 0.30, 0.30);
            }
            // Red lens, darker at the edge, highlight upper left.
            let hl =
                ((c.p[0].abs() - cx + 0.3 * r).powi(2) + (z - cz - 0.35 * r).powi(2)).sqrt() / r;
            let k = 0.85 - 0.35 * d + if hl < 0.18 { 0.35 } else { 0.0 };
            return rgb(
                0.80 * k + 0.05,
                0.06 * k + 0.02 + if hl < 0.18 { 0.3 } else { 0.0 },
                0.07 * k + 0.02 + if hl < 0.18 { 0.3 } else { 0.0 },
            );
        }
        material(c.part.material)
    }

    // ---------------------------------------------------------------- wheels

    fn wheel(env: &Env, c: &Ctx) -> Rgba {
        let (y, z) = (c.p[1], c.p[2]);
        let r = (y * y + z * z).sqrt();
        let px = c.m_per_px;
        let w = env.def.wheels.first();
        let tire_r = w.map_or(0.32, |w| w.radius_m);
        let rim_in = env.trim.wheel.as_ref().map_or(17.0, |w| w.diameter_in);
        let rim_r = (0.5 * rim_in * 0.0254).min(0.82 * tire_r);
        let rim_rgb = env.trim.wheel.as_ref().map_or([180, 182, 186], |w| w.rgb);
        let pattern = env
            .trim
            .wheel
            .as_ref()
            .map_or("five", |w| w.pattern.as_str());
        if c.part.kind == PartKind::Tire || r > rim_r + 0.012 {
            // Sidewall: rubber, a little lighter in the middle, a fine ring at the rim.
            let t = ((r - rim_r) / (tire_r - rim_r).max(0.02)).clamp(0.0, 1.0);
            let k = 0.075 + 0.05 * (1.0 - (2.0 * t - 1.0).powi(2));
            if (r - (rim_r + 0.018)).abs() < 0.6 * px {
                return rgb(0.16, 0.16, 0.17);
            }
            return rgb(k, k, k * 1.05);
        }
        let metal = |k: f32| rgb8(rim_rgb, k);
        let lip_in = rim_r - 0.014;
        if r > lip_in {
            // Polished lip.
            let k = 1.10 - 0.25 * ((r - lip_in) / 0.026);
            return metal(k);
        }
        if r > lip_in - 0.010 {
            return metal(0.35);
        }
        // Hub: lugs and centre cap.
        let hub = 0.068;
        let ang = z.atan2(y);
        if r < hub {
            if r < 0.026 {
                return if r < 0.010 {
                    rgb(0.75, 0.08, 0.08)
                } else {
                    rgb(0.12, 0.12, 0.13)
                };
            }
            for k in 0..5 {
                let a = std::f32::consts::TAU * k as f32 / 5.0 + 0.3;
                let (s, co) = wp_math::sin_cos(a);
                let d = ((y - 0.0571 * co).powi(2) + (z - 0.0571 * s).powi(2)).sqrt();
                if d < 0.009 {
                    return if d < 0.005 {
                        rgb(0.25, 0.25, 0.27)
                    } else {
                        metal(0.55)
                    };
                }
            }
            return metal(0.95);
        }
        // Spokes.
        let (n, pair, w_hub, w_rim): (usize, f32, f32, f32) = match pattern {
            "five_double" => (5, 0.017, 0.030, 0.020),
            "six" => (6, 0.0, 0.034, 0.026),
            "multi" => (10, 0.0, 0.018, 0.014),
            _ => (5, 0.0, 0.040, 0.030),
        };
        let t = ((r - hub) / (lip_in - hub)).clamp(0.0, 1.0);
        let half = 0.5 * (w_hub + (w_rim - w_hub) * t);
        let mut best = f32::MAX;
        let mut side_sign = 0.0;
        for k in 0..n {
            let a = std::f32::consts::TAU * k as f32 / n as f32 + std::f32::consts::FRAC_PI_2;
            let (s, co) = wp_math::sin_cos(a);
            // Distance from the spoke's centre line (radial through angle a), signed.
            let along = y * co + z * s;
            if along < 0.0 {
                continue;
            }
            let across = -y * s + z * co;
            // Double spokes: two bars, ± the pair offset growing toward the rim.
            let offs: &[f32] = if pair > 0.0 { &[-1.0, 1.0] } else { &[0.0] };
            for &o in offs {
                // Twin spokes: one bar at the hub splitting into a V toward the rim.
                let off = o * pair * (t * 1.15 - 0.1).clamp(0.0, 1.0);
                let d = across - off;
                if d.abs() < best.abs() {
                    best = d;
                    side_sign = d.signum();
                }
            }
        }
        let _ = ang;
        if best.abs() < half {
            // Bevelled spoke: lit on one side.
            let e = 1.0 - best.abs() / half;
            let k = if e < 0.25 {
                0.70 + 0.35 * (side_sign * 0.5 + 0.5)
            } else {
                1.0 - 0.10 * t
            };
            return metal(k);
        }
        // Between the spokes: the dark well, the brake disc and the caliper behind.
        let disc_r = 0.82 * rim_r;
        if r < disc_r && r > 0.40 * rim_r {
            let caliper = y < -0.04 && (z.abs() < 0.075) && r > 0.60 * rim_r;
            if caliper {
                return if env.trim_key.starts_with("gtr") {
                    rgb(0.80, 0.64, 0.16)
                } else {
                    rgb(0.30, 0.30, 0.32)
                };
            }
            let k = 0.13 + 0.03 * ((r / 0.012).fract() < 0.5) as i32 as f32;
            return rgb(k, k, k * 1.02);
        }
        rgb(0.03, 0.03, 0.035)
    }
}
