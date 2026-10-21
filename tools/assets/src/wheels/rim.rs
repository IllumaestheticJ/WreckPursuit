//! Rims (`research_mechanical.md` §4.2): a lathed barrel with the outer flange, lip face and
//! dish wall, then the design (spokes, steel face, hubcap …), hub pad, lug nuts and centre
//! cap, all facing the wheel's outboard side.
//!
//! Hub-space `(r, u)` coordinates (`u` toward the outboard face, `x = outboard · u`). Planes
//! along the axle, outboard → inboard: lip face `rw`, spoke / steel face at the rim edge
//! `u_face = rw − face_depth`, hub pad `u_hub = u_face − concave`, then the brake behind
//! ([`RimLayout::u_back_min`]).
//!
//! Parts per wheel: `wheel_xx_rim` (rim material), `wheel_xx_rim_bright` (lug nuts, polished
//! lips, trim rings, chrome caps), `wheel_xx_rim_dark` (vent holes, slots, windows),
//! `wheel_xx_rim_cap` (plastic hubcap) — only the ones a design uses. Metal rims and hubcaps
//! are drawn in the `rim` slot (tinted per car by `WheelStyle::rim_rgb`).

use crate::builder::{Builder, PartKey};
use crate::mechanical::prim::{hub_lathe, hub_pt, sc_deg};
use crate::mechanical::spec::{TireKind, WheelSpec};
use crate::mesh::{MaterialSlot, PartKind};
use crate::style::{CapKind, RimKind};

use super::WheelGeom;

/// Planes and radii of a rim along the axle (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RimLayout {
    /// Rim half width (lip face plane), m.
    pub rw: f32,
    /// Flange top radius, m.
    pub r_f: f32,
    /// Lip inner edge radius, m.
    pub r_li: f32,
    /// Barrel inner radius, m.
    pub r_b: f32,
    /// Face plane at the rim edge (spoke fronts), m.
    pub u_face: f32,
    /// Hub pad front plane, m.
    pub u_hub: f32,
    /// Hub pad radius, m.
    pub hub_r: f32,
    /// Spoke thickness (axial), m.
    pub spoke_t: f32,
    /// Closed face (steel / slotted / truck): the brake is hidden behind it.
    pub closed: bool,
    /// Most inboard back face of the spokes / face in the brake's radius band, m.
    pub u_back_min: f32,
    /// Size scale (rim radius / 18-inch rim radius).
    pub kz: f32,
}

impl RimLayout {
    /// Layout of a rim of spec `w` on a tire of section width `width`.
    pub fn new(w: &WheelSpec, width: f32, radius: f32, tire: TireKind) -> RimLayout {
        let tall = (radius - w.rim_r) / width.max(0.05) >= 0.6 || tire == TireKind::AllTerrain;
        let rw = (0.5 * (width - if tall { 0.03 } else { 0.02 })).max(0.06);
        let kz = (w.rim_r / 0.2286).clamp(0.6, 1.25);
        let r_f = w.rim_r + 0.012;
        let r_li = r_f - w.lip.clamp(0.006, 0.045);
        let r_b = (w.rim_r - 0.008).min(r_li - 0.002);
        let face_depth = w.face_depth.clamp(0.004, 0.07).min(rw - 0.02);
        let u_face = rw - face_depth;
        let u_hub = u_face - w.concave.clamp(0.0, 0.045);
        let closed = matches!(
            w.rim,
            RimKind::SteelHubcap
                | RimKind::Steel
                | RimKind::DogDish
                | RimKind::SlotMag
                | RimKind::Truck
        );
        let spoke_t = match w.rim {
            RimKind::Mesh => 0.012,
            RimKind::Turbine => 0.01,
            RimKind::Monoblock => 0.02,
            _ => 0.022,
        } * kz.max(0.85);
        let u_back_min = if closed {
            u_face - 0.016
        } else {
            u_hub.min(u_face) - spoke_t - if w.rim == RimKind::Mesh { 0.006 } else { 0.002 }
        };
        RimLayout {
            rw,
            r_f,
            r_li,
            r_b,
            u_face,
            u_hub,
            hub_r: (0.075 * kz).clamp(0.058, 0.09),
            spoke_t,
            closed,
            u_back_min,
            kz,
        }
    }
}

/// Keys of one wheel's rim parts.
struct Keys {
    rim: PartKey,
    bright: PartKey,
    dark: PartKey,
    cap: PartKey,
}

/// One spoke as a chain of stations `(r, w, u_front)` from hub to rim, swept from clock
/// angle `deg0` (hub end) to `deg1` (rim end), `t` thick, blades twisted by `twist_deg`.
#[allow(clippy::too_many_arguments)]
fn spoke(
    b: &mut Builder,
    key: &PartKey,
    s: f32,
    deg0: f32,
    deg1: f32,
    st: &[[f32; 3]],
    t: f32,
    twist_deg: f32,
) {
    let m = st.len();
    if m < 2 {
        return;
    }
    let (tw_s, tw_c) = sc_deg(twist_deg);
    let ring = |i: usize| -> [[f32; 3]; 4] {
        let f = i as f32 / (m - 1) as f32;
        let d = deg0 + (deg1 - deg0) * f;
        let [r, w, u] = st[i];
        let (sn, cs) = sc_deg(d);
        // Radial, tangential (CCW in the wheel plane), axial (outboard) unit vectors.
        let er = [0.0, cs, sn];
        let et = [0.0, -sn, cs];
        let ea = [s, 0.0, 0.0];
        let c = |ot: f32, oa: f32| -> [f32; 3] {
            // Twist rotates the (tangent, axial) section about the radial axis.
            let tt = ot * tw_c - oa * tw_s;
            let aa = ot * tw_s + oa * tw_c;
            let mid = u - 0.5 * t;
            [
                er[0] * r + et[0] * tt + ea[0] * (mid + aa),
                er[1] * r + et[1] * tt + ea[1] * (mid + aa),
                er[2] * r + et[2] * tt + ea[2] * (mid + aa),
            ]
        };
        [
            c(-0.5 * w, 0.5 * t),
            c(0.5 * w, 0.5 * t),
            c(0.5 * w, -0.5 * t),
            c(-0.5 * w, -0.5 * t),
        ]
    };
    let rings: Vec<[[f32; 3]; 4]> = (0..m).map(ring).collect();
    use crate::mechanical::prim::{centroid, sub};
    for i in 0..m - 1 {
        let all: Vec<[f32; 3]> = rings[i]
            .iter()
            .chain(rings[i + 1].iter())
            .copied()
            .collect();
        let mid = centroid(&all);
        for k in 0..4 {
            let k1 = (k + 1) % 4;
            let q = [rings[i][k], rings[i][k1], rings[i + 1][k1], rings[i + 1][k]];
            b.poly_facing(key, &q, sub(centroid(&q), mid));
        }
    }
    let first_mid = centroid(&rings[1]);
    b.poly_facing(key, &rings[0], sub(centroid(&rings[0]), first_mid));
    let last_mid = centroid(&rings[m - 2]);
    b.poly_facing(key, &rings[m - 1], sub(centroid(&rings[m - 1]), last_mid));
}

/// Height `u` of a face profile `(r, u)` (sorted by decreasing r) at radius `r`.
fn face_u(prof: &[[f32; 2]], r: f32) -> f32 {
    for w in prof.windows(2) {
        let (a, c) = (w[0], w[1]);
        if r <= a[0] && r >= c[0] {
            let t = (a[0] - r) / (a[0] - c[0]).max(1e-6);
            return a[1] + (c[1] - a[1]) * t;
        }
    }
    prof.last().map_or(0.0, |q| q[1])
}

/// Flat polygon following a face: corner list of (deg, r), lifted `lift` off `prof`.
fn face_poly(
    b: &mut Builder,
    key: &PartKey,
    s: f32,
    prof: &[[f32; 2]],
    pts: &[[f32; 2]],
    lift: f32,
) {
    let q: Vec<[f32; 3]> = pts
        .iter()
        .map(|p| hub_pt(s, p[0], p[1], face_u(prof, p[1]) + lift))
        .collect();
    b.poly_facing(key, &q, [s, 0.0, 0.0]);
}

/// Annular window / slot on a face between radii `r0..r1`, centred at `deg`, half-angle
/// `half_deg` (rounded ends with `steps` arc points).
#[allow(clippy::too_many_arguments)]
fn face_window(
    b: &mut Builder,
    key: &PartKey,
    s: f32,
    prof: &[[f32; 2]],
    deg: f32,
    r0: f32,
    r1: f32,
    half_outer: f32,
    half_inner: f32,
) {
    let mut pts = Vec::new();
    for k in 0..3 {
        let f = k as f32 / 2.0;
        pts.push([deg - half_inner + 2.0 * half_inner * f, r0]);
    }
    for k in 0..3 {
        let f = k as f32 / 2.0;
        pts.push([deg + half_outer - 2.0 * half_outer * f, r1]);
    }
    face_poly(b, key, s, prof, &pts, 0.002);
}

/// Round dark hole (vent) on a face.
fn face_hole(
    b: &mut Builder,
    key: &PartKey,
    s: f32,
    prof: &[[f32; 2]],
    deg: f32,
    rc: f32,
    hr: f32,
) {
    let (sn, cs) = sc_deg(deg);
    let c = [rc * cs, rc * sn];
    let u = face_u(prof, rc) + 0.0025;
    let q: Vec<[f32; 3]> = (0..8)
        .map(|i| {
            let (a, bb) = crate::builder::turn(i, 8);
            [s * u, c[0] + hr * bb, c[1] + hr * a]
        })
        .collect();
    b.poly_facing(key, &q, [s, 0.0, 0.0]);
}

/// Lug nuts (hex) on the bolt circle, standing on the plane `u0`.
fn lugs(b: &mut Builder, key: &PartKey, g: &WheelGeom, u0: f32) {
    let w = &g.spec;
    if w.lugs == 0 {
        return;
    }
    let s = g.outboard;
    let r = 0.0095 * g.layout.kz.max(0.9);
    let sides = if g.hero { 6 } else { 4 };
    for k in 0..w.lugs {
        let deg = 90.0 + 360.0 * k as f32 / w.lugs as f32;
        let (sn, cs) = sc_deg(deg);
        let (y, z) = (w.pcd_r * cs, w.pcd_r * sn);
        // Open at the back (sits on the face), capped at the front.
        let (a, e) = ([s * (u0 - 0.004), y, z], [s * (u0 + 0.016), y, z]);
        b.cylinder(key, a, e, r, r * 0.92, sides, false);
        let cap: Vec<[f32; 3]> = (0..sides)
            .map(|i| {
                let (sn2, cs2) = crate::builder::turn(i, sides);
                [e[0], y + 0.92 * r * cs2, z + 0.92 * r * sn2]
            })
            .collect();
        b.poly_facing(key, &cap, [s, 0.0, 0.0]);
    }
}

/// Domed cap of radius `r` standing on the plane `u0`, `h` tall.
fn dome(b: &mut Builder, key: &PartKey, s: f32, u0: f32, r: f32, h: f32, n: usize) {
    let prof = [
        [r, u0 - 0.004],
        [r, u0 + 0.35 * h],
        [0.6 * r, u0 + 0.9 * h],
        [0.0, u0 + h],
    ];
    // Coarse domes (traffic) skip the shoulder point.
    if n <= 8 {
        hub_lathe(b, key, s, &[prof[0], prof[1], prof[3]], n);
    } else {
        hub_lathe(b, key, s, &prof, n);
    }
}

/// Centre cap of kind `cap` on the plane `u0`.
fn centre_cap(b: &mut Builder, k: &Keys, g: &WheelGeom, cap: CapKind, u0: f32, mat_key: &PartKey) {
    let s = g.outboard;
    let n = (g.lathe_n() / 2).max(8);
    let max_r = (g.spec.pcd_r - 0.014).max(0.022);
    match cap {
        CapKind::None | CapKind::Hubcap => {}
        CapKind::Small => dome(b, mat_key, s, u0, 0.03f32.min(max_r), 0.016, n),
        CapKind::Centre => dome(b, mat_key, s, u0, 0.045f32.min(max_r), 0.02, n),
        CapKind::DogDish => {
            let r = 0.12 * g.layout.kz;
            dome(b, &k.bright, s, u0, r, 0.035, g.lathe_n());
        }
        CapKind::CentreLock => {
            // Hex nut + drive pins (coloured: caliper tint).
            let nut = PartKey::wheel(
                &g.name("rim_lock"),
                g.index,
                PartKind::Rim,
                MaterialSlot::Caliper,
            );
            b.cylinder(
                &nut,
                [s * (u0 - 0.004), 0.0, 0.0],
                [s * (u0 + 0.035), 0.0, 0.0],
                0.037,
                0.033,
                6,
                true,
            );
            for i in 0..6 {
                let (sn, cs) = crate::builder::turn(i, 6);
                let (y, z) = (0.052 * cs, 0.052 * sn);
                b.cylinder(
                    &k.bright,
                    [s * (u0 - 0.002), y, z],
                    [s * (u0 + 0.012), y, z],
                    0.006,
                    0.006,
                    4,
                    true,
                );
            }
        }
    }
}

/// Emit the rim of wheel `g`.
pub fn rim(g: &WheelGeom, b: &mut Builder) {
    let w = g.spec;
    let l = g.layout;
    let s = g.outboard;
    let n = g.lathe_n();
    let mk =
        |suffix: &str, m: MaterialSlot| PartKey::wheel(&g.name(suffix), g.index, PartKind::Rim, m);
    // Painted / alloy rims and plastic hubcaps use the per-car tintable `rim` slot.
    let body_mat = if w.material == MaterialSlot::Metal {
        MaterialSlot::Rim
    } else {
        w.material
    };
    let rim_key = mk("rim", body_mat);
    let bright = if w.bright == w.material {
        rim_key.clone()
    } else {
        mk("rim_bright", w.bright)
    };
    let k = Keys {
        rim: rim_key,
        bright,
        dark: mk("rim_dark", MaterialSlot::Grille),
        cap: mk("rim_cap", MaterialSlot::Rim),
    };
    let rim_r = g.rim_radius;
    let polished_lip = matches!(w.rim, RimKind::Mesh | RimKind::SlotMag | RimKind::DogDish);

    // Barrel: hidden flange back, flange top, lip face, dish wall, barrel, inboard flange.
    let lip_face = [[l.r_f, l.rw], [l.r_li, l.rw]];
    let mut barrel: Vec<[f32; 2]> = if g.hero {
        vec![[l.r_f, l.rw - 0.008], [l.r_f, l.rw]]
    } else {
        vec![[l.r_f, l.rw]]
    };
    if polished_lip {
        hub_lathe(b, &k.bright, s, &lip_face, n);
        hub_lathe(b, &k.rim, s, &[[l.r_f, l.rw - 0.008], [l.r_f, l.rw]], n);
        barrel = vec![[l.r_li, l.rw]];
    } else {
        barrel.push([l.r_li, l.rw]);
    }
    barrel.push([l.r_b, l.u_face.min(l.rw - 0.004)]);
    barrel.push([l.r_b, -l.rw + 0.004]);
    hub_lathe(b, &k.rim, s, &barrel, n);

    let hub_pad = |b: &mut Builder, key: &PartKey| {
        hub_lathe(
            b,
            key,
            s,
            &[
                [l.hub_r, l.u_hub - 0.03],
                [l.hub_r, l.u_hub],
                [0.55 * l.hub_r, l.u_hub + 0.005],
                [0.0, l.u_hub + 0.005],
            ],
            (n / 2).max(8),
        );
    };
    let kz = l.kz;
    let r_rim_end = l.r_b + 0.003;
    let r_hub_end = 0.8 * l.hub_r;
    let mid_u = |f: f32| l.u_hub + (l.u_face - l.u_hub) * f;
    let spokes = w.spokes.max(1);
    let two_seg = g.hero && spokes <= 6;
    // Straight / tapered spokes, optionally bowed outboard in the middle.
    let simple = |b: &mut Builder, count: u8, w0: f32, w1: f32, bow: f32, twist: f32, t: f32| {
        for i in 0..count {
            let deg = 90.0 + 360.0 * i as f32 / count as f32;
            let st: Vec<[f32; 3]> = if two_seg {
                vec![
                    [r_hub_end, w0, l.u_hub],
                    [
                        0.5 * (r_hub_end + r_rim_end),
                        0.5 * (w0 + w1),
                        mid_u(0.5) + bow,
                    ],
                    [r_rim_end, w1, l.u_face],
                ]
            } else {
                vec![[r_hub_end, w0, l.u_hub], [r_rim_end, w1, l.u_face]]
            };
            spoke(b, &k.rim, s, deg, deg, &st, t, twist);
        }
    };
    let mut cap_plane = l.u_hub + 0.004;
    let mut lug_plane = l.u_hub;
    let mut show_hub = true;
    match w.rim {
        RimKind::FiveSpoke | RimKind::Star => {
            simple(b, spokes, 0.07 * kz, 0.045 * kz, 0.006, 0.0, l.spoke_t)
        }
        RimKind::SixSpoke => simple(b, spokes, 0.045 * kz, 0.045 * kz, 0.003, 0.0, l.spoke_t),
        RimKind::MultiSpoke => simple(b, spokes, 0.024 * kz, 0.022 * kz, 0.004, 0.0, l.spoke_t),
        RimKind::OemAlloy => simple(b, spokes, 0.04 * kz, 0.03 * kz, 0.0, 0.0, l.spoke_t),
        RimKind::Turbine => {
            simple(b, spokes, 0.03 * kz, 0.036 * kz, 0.0, 32.0, l.spoke_t);
            // Closed outer ring at the blade tips.
            hub_lathe(
                b,
                &k.rim,
                s,
                &[
                    [l.r_b + 0.002, l.u_face - 0.012],
                    [l.r_b - 0.03 * kz, l.u_face + 0.002],
                ],
                n,
            );
        }
        RimKind::Mesh => {
            // Two crossing layers of thin spokes.
            for (layer, (dd, du)) in [(18.0f32, 0.0f32), (-18.0, -0.005)].iter().enumerate() {
                for i in 0..spokes {
                    let deg = 90.0 + 360.0 * (i as f32 + 0.5 * layer as f32) / spokes as f32;
                    let st = [
                        [r_hub_end, 0.016 * kz, l.u_hub + du],
                        [r_rim_end, 0.013 * kz, l.u_face + du],
                    ];
                    spoke(b, &k.rim, s, deg, deg + dd, &st, l.spoke_t, 0.0);
                }
            }
        }
        RimKind::SplitSpoke | RimKind::Monoblock => {
            let mono = w.rim == RimKind::Monoblock;
            let (r_end, u_end) = if mono {
                (l.r_li + 0.004, l.rw - 0.005)
            } else {
                (r_rim_end, l.u_face)
            };
            let split_r = r_hub_end + 0.5 * (r_end - r_hub_end);
            let u_split = l.u_hub + 0.5 * (u_end - l.u_hub) + 0.004;
            let gap_deg = (if mono { 0.022 } else { 0.03 } * kz / r_end).to_degrees();
            let arm_w = if mono { 0.018 } else { 0.022 } * kz;
            for i in 0..spokes {
                let deg = 90.0 + 360.0 * i as f32 / spokes as f32;
                let stem = [
                    [r_hub_end, 0.052 * kz, l.u_hub],
                    [split_r + 0.01, 0.042 * kz, u_split],
                ];
                spoke(b, &k.rim, s, deg, deg, &stem, l.spoke_t, 0.0);
                for side in [-1.0f32, 1.0] {
                    let arm = [
                        [split_r - 0.005, arm_w, u_split],
                        [r_end, arm_w * 0.9, u_end],
                    ];
                    spoke(
                        b,
                        &k.rim,
                        s,
                        deg + side * 0.15 * gap_deg,
                        deg + side * gap_deg,
                        &arm,
                        l.spoke_t * 0.9,
                        0.0,
                    );
                }
            }
        }
        RimKind::Steel | RimKind::SteelHubcap | RimKind::DogDish => {
            let prof: Vec<[f32; 2]> = if g.hero {
                vec![
                    [l.r_b, l.u_face - 0.014],
                    [0.80 * rim_r, l.u_face],
                    [0.52 * rim_r, l.u_face + 0.004],
                    [l.hub_r + 0.025, l.u_face + 0.016],
                    [l.hub_r, l.u_face + 0.010],
                    [0.0, l.u_face + 0.010],
                ]
            } else {
                vec![
                    [l.r_b, l.u_face - 0.014],
                    [0.80 * rim_r, l.u_face],
                    [l.hub_r + 0.025, l.u_face + 0.014],
                    [0.0, l.u_face + 0.010],
                ]
            };
            let capped = w.rim == RimKind::SteelHubcap && w.missing_cap != Some(g.index);
            if capped {
                // Plastic full-face hubcap with dark vent slots; the steel face is hidden.
                let dome_p = [
                    [rim_r + 0.006, l.u_face - 0.004],
                    [rim_r - 0.006, l.u_face + 0.014],
                    [0.66 * rim_r, l.u_face + 0.026],
                    [0.26 * rim_r, l.u_face + 0.036],
                    [0.0, l.u_face + 0.038],
                ];
                hub_lathe(b, &k.cap, s, &dome_p, n);
                let slots = spokes.clamp(4, 10);
                for i in 0..slots {
                    let deg = 90.0 + 360.0 * i as f32 / slots as f32;
                    face_window(
                        b,
                        &k.dark,
                        s,
                        &dome_p,
                        deg,
                        0.40 * rim_r,
                        0.62 * rim_r,
                        9.0,
                        6.0,
                    );
                }
                show_hub = false;
                cap_plane = f32::NAN;
                lug_plane = f32::NAN;
            } else {
                hub_lathe(b, &k.rim, s, &prof, n);
                let holes = if w.rim == RimKind::SteelHubcap {
                    6
                } else {
                    spokes.clamp(4, 8)
                };
                for i in 0..holes {
                    let deg = 90.0 + 180.0 / holes as f32 + 360.0 * i as f32 / holes as f32;
                    face_hole(b, &k.dark, s, &prof, deg, 0.64 * rim_r, 0.026 * kz);
                }
                if w.rim == RimKind::DogDish {
                    // Chrome trim ring over the dish wall.
                    hub_lathe(
                        b,
                        &k.bright,
                        s,
                        &[[l.r_li, l.rw + 0.001], [l.r_b - 0.004, l.u_face + 0.002]],
                        n,
                    );
                }
                show_hub = false;
                lug_plane = face_u(&prof, w.pcd_r);
                cap_plane = l.u_face + 0.010;
            }
        }
        RimKind::SlotMag => {
            let prof = [
                [l.r_b, l.u_face - 0.010],
                [0.86 * rim_r, l.u_face],
                [0.40 * rim_r, l.u_face + 0.012],
                [l.hub_r, l.u_face + 0.016],
                [0.0, l.u_face + 0.016],
            ];
            hub_lathe(b, &k.rim, s, &prof, n);
            let half = 15.0 * 5.0 / spokes as f32;
            for i in 0..spokes {
                let deg = 90.0 + 180.0 / spokes as f32 + 360.0 * i as f32 / spokes as f32;
                face_window(
                    b,
                    &k.dark,
                    s,
                    &prof,
                    deg,
                    0.45 * rim_r,
                    0.85 * rim_r,
                    half,
                    half * 1.4,
                );
            }
            show_hub = false;
            lug_plane = face_u(&prof, w.pcd_r);
            cap_plane = l.u_face + 0.016;
        }
        RimKind::Truck => {
            let prof: Vec<[f32; 2]> = if g.hero {
                vec![
                    [l.r_b, l.u_face - 0.012],
                    [0.85 * rim_r, l.u_face],
                    [0.45 * rim_r, l.u_face + 0.022],
                    [l.hub_r + 0.03, l.u_face + 0.04],
                    [l.hub_r, l.u_face + 0.036],
                    [0.0, l.u_face + 0.036],
                ]
            } else {
                vec![
                    [l.r_b, l.u_face - 0.012],
                    [0.85 * rim_r, l.u_face],
                    [l.hub_r + 0.03, l.u_face + 0.04],
                    [0.0, l.u_face + 0.036],
                ]
            };
            hub_lathe(b, &k.rim, s, &prof, n);
            for i in 0..spokes {
                let deg = 90.0 + 180.0 / spokes as f32 + 360.0 * i as f32 / spokes as f32;
                let half = 0.30 * 360.0 / spokes as f32;
                face_window(
                    b,
                    &k.dark,
                    s,
                    &prof,
                    deg,
                    0.50 * rim_r,
                    0.80 * rim_r,
                    half,
                    half * 0.8,
                );
            }
            show_hub = false;
            lug_plane = face_u(&prof, w.pcd_r);
            cap_plane = l.u_face + 0.036;
        }
    }
    if show_hub {
        hub_pad(b, &k.rim);
    }
    if lug_plane.is_finite() && w.cap != CapKind::CentreLock && w.cap != CapKind::DogDish {
        lugs(b, &k.bright, g, lug_plane);
    }
    if cap_plane.is_finite() {
        let cap = if w.rim == RimKind::SteelHubcap {
            CapKind::Small
        } else {
            w.cap
        };
        let cap_key = if matches!(w.rim, RimKind::Steel | RimKind::SlotMag | RimKind::DogDish) {
            k.bright.clone()
        } else {
            k.rim.clone()
        };
        centre_cap(b, &k, g, cap, cap_plane, &cap_key);
    }
}
