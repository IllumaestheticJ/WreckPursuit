//! Body-fixed suspension and driveline (`research_mechanical.md` §2): arms, links, springs,
//! damper tops / rods, anti-roll bars, leaf packs, diffs, half shafts, driveshaft and frame
//! rails, chassis space, rigid (`Suspension` kind).
//!
//! The knuckle / strut lower body / live-axle half tubes are pivot parts drawn in hub space
//! by [`super::brakes::pivot_hardware`]; the arms here end ~4 cm short of them (inside the
//! arch shadow) so wheel travel never opens a visible gap at a joint. Both sides use
//! [`crate::wheels::knuckle_in`] for the knuckle position.
//!
//! Parts: `susp_front` / `susp_rear` (dark: arms, damper bodies, mounts, leaves, diff),
//! `susp_front_metal` / `susp_rear_metal` (links, rods, shafts, axle), `susp_front_spring` /
//! `susp_rear_spring` (coil springs: metal, caliper tint for coilovers / painted springs, dark
//! when rusty), `frame_rail_l` / `frame_rail_r`, `driveline`.

use wp_contracts::{BodyRegion, VehicleDef};

use crate::builder::{Builder, PartKey};
use crate::mesh::{MaterialSlot, PartKind};
use crate::profile::BodyProfile;
use crate::wheels::knuckle_in;

use super::engine::EngineOut;
use super::prim::{bar, helix, rod, tube};
use super::spec::{CarSpec, Mount, Susp};

/// Part keys of one axle.
struct Axle {
    dark: PartKey,
    metal: PartKey,
    spring: PartKey,
}

impl Axle {
    fn new(front: bool, s: &CarSpec) -> Axle {
        let (name, region) = if front {
            ("susp_front", BodyRegion::FrameFront)
        } else {
            ("susp_rear", BodyRegion::FrameRear)
        };
        let spring_mat = if s.rusty {
            MaterialSlot::Engine
        } else if s.tinted_springs {
            MaterialSlot::Caliper
        } else {
            MaterialSlot::Metal
        };
        let k =
            |n: &str, m: MaterialSlot| PartKey::body(n, region, PartKind::Suspension, m).rigid();
        Axle {
            dark: k(name, MaterialSlot::Engine),
            metal: k(&format!("{name}_metal"), MaterialSlot::Metal),
            spring: k(&format!("{name}_spring"), spring_mat),
        }
    }
}

/// One corner: hub, outboard sign, knuckle inboard distance, travel.
#[derive(Clone, Copy)]
struct Corner {
    h: [f32; 3],
    s: f32,
    k_in: f32,
    travel: f32,
}

impl Corner {
    /// Chassis point `in_m` inboard of the hub plane, `dy` forward, `dz` up from the hub.
    fn p(&self, in_m: f32, dy: f32, dz: f32) -> [f32; 3] {
        [self.h[0] - self.s * in_m, self.h[1] + dy, self.h[2] + dz]
    }
}

/// Drawing context.
struct Ctx<'a> {
    b: &'a mut Builder,
    hero: bool,
}

impl Ctx<'_> {
    fn arm(&mut self, k: &PartKey, a: [f32; 3], e: [f32; 3]) {
        bar(self.b, k, a, e, 0.05, 0.022, [0.0, 0.0, 1.0]);
    }
    fn link(&mut self, k: &PartKey, a: [f32; 3], e: [f32; 3], r: f32) {
        rod(self.b, k, a, e, r, if self.hero { 6 } else { 4 });
    }
    /// Coil spring: helix (hero) or a plain cylinder (traffic).
    #[allow(clippy::too_many_arguments)]
    fn spring(
        &mut self,
        k: &PartKey,
        a: [f32; 3],
        e: [f32; 3],
        mean_r: f32,
        wire: f32,
        coils: f32,
    ) {
        if self.hero {
            helix(self.b, k, a, e, mean_r, wire, coils, 6, 3);
        } else {
            self.b
                .cylinder(k, a, e, mean_r + wire, mean_r + wire, 6, false);
        }
    }
    /// Damper: body (dark) from `a` to `m`, chrome-ish rod (metal) from `m` to `e`.
    fn damper(&mut self, ax: &Axle, a: [f32; 3], m: [f32; 3], e: [f32; 3], r: f32) {
        let sides = if self.hero { 8 } else { 6 };
        self.b.cylinder(&ax.dark, a, m, r, r, sides, true);
        rod(
            self.b,
            &ax.metal,
            m,
            e,
            0.4 * r,
            if self.hero { 6 } else { 4 },
        );
    }
}

/// Point at height `z` on a strut line (matches `brakes::pivot_hardware`).
fn strut_pt(c: &Corner, z: f32) -> [f32; 3] {
    let f = (z - 0.08) / 0.50;
    c.p(c.k_in + 0.08 * f, -0.02 - 0.02 * f, z)
}

fn tie_rod(x: &mut Ctx, ax: &Axle, c: &Corner) {
    x.link(
        &ax.metal,
        c.p(c.k_in + 0.045, -0.12, -0.02),
        c.p(c.k_in + 0.38, -0.11, -0.01),
        0.009,
    );
}

fn strut(x: &mut Ctx, ax: &Axle, c: &Corner, steer: bool, tinted: bool) {
    let o = c.p(c.k_in + 0.04, 0.0, -0.10);
    for dy in [-0.15f32, 0.15] {
        x.arm(&ax.dark, o, c.p(c.k_in + 0.36, dy, -0.11));
    }
    // Rod from inside the (pivot) lower body up to the top mount, spring, top mount.
    let low = (0.30 - 0.9 * c.travel).max(0.12);
    let sides = if x.hero { 6 } else { 4 };
    rod(
        x.b,
        &ax.metal,
        strut_pt(c, low),
        strut_pt(c, 0.56),
        0.011,
        sides,
    );
    x.b.cylinder(
        &ax.dark,
        strut_pt(c, 0.54),
        strut_pt(c, 0.585),
        0.065,
        0.06,
        if x.hero { 10 } else { 6 },
        true,
    );
    let (mean, wire, coils) = if tinted {
        (0.045, 0.006, 7.0)
    } else {
        (0.07, 0.0065, 5.0)
    };
    x.spring(
        &ax.spring,
        strut_pt(c, 0.25),
        strut_pt(c, 0.53),
        mean,
        wire,
        coils,
    );
    if steer {
        tie_rod(x, ax, c);
    }
}

fn wishbones(x: &mut Ctx, ax: &Axle, c: &Corner, thin: bool) {
    let lo = c.p(c.k_in + 0.04, 0.0, -0.11);
    let up = c.p(c.k_in + 0.035, 0.0, 0.21);
    for dy in [-0.17f32, 0.17] {
        let a = c.p(c.k_in + 0.42, dy, -0.12);
        let u = c.p(c.k_in + 0.27, 0.7 * dy, 0.23);
        if thin {
            x.link(&ax.dark, lo, a, 0.012);
            x.link(&ax.dark, up, u, 0.011);
        } else {
            x.arm(&ax.dark, lo, a);
            x.arm(&ax.dark, up, u);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn sla(x: &mut Ctx, ax: &Axle, c: &Corner, kind: Susp, steer: bool, tinted: bool) {
    wishbones(x, ax, c, kind == Susp::Coilover);
    match kind {
        Susp::SlaCoil => {
            x.spring(
                &ax.spring,
                c.p(c.k_in + 0.24, 0.0, -0.08),
                c.p(c.k_in + 0.24, 0.0, 0.21),
                0.062,
                0.008,
                4.5,
            );
            rod(
                x.b,
                &ax.metal,
                c.p(c.k_in + 0.24, 0.0, -0.06),
                c.p(c.k_in + 0.24, 0.0, 0.3),
                0.012,
                4,
            );
        }
        Susp::SlaTorsion => {
            let bush = c.p(c.k_in + 0.42, -0.17, -0.12);
            x.link(&ax.metal, bush, [bush[0], bush[1] - 0.95, bush[2]], 0.0125);
            x.damper(
                ax,
                c.p(c.k_in + 0.17, 0.05, -0.08),
                c.p(c.k_in + 0.19, 0.05, 0.12),
                c.p(c.k_in + 0.21, 0.05, 0.27),
                0.02,
            );
        }
        Susp::Coilover => {
            let (a, e) = (
                c.p(c.k_in + 0.18, 0.0, -0.085),
                c.p(c.k_in + 0.31, 0.0, 0.29),
            );
            let m = super::prim::mix(a, e, 0.55);
            x.damper(ax, a, m, e, 0.023);
            let (sa, se) = (super::prim::mix(a, e, 0.3), super::prim::mix(a, e, 0.92));
            x.spring(&ax.spring, sa, se, 0.042, 0.006, 7.0);
        }
        _ => {
            // Coil-over-shock.
            let (a, e) = (
                c.p(c.k_in + 0.2, 0.03, -0.075),
                c.p(c.k_in + 0.3, 0.03, 0.38),
            );
            let m = super::prim::mix(a, e, 0.5);
            x.damper(ax, a, m, e, 0.024);
            let (mean, wire, coils) = if tinted {
                (0.045, 0.006, 7.0)
            } else {
                (0.056, 0.0075, 5.0)
            };
            x.spring(
                &ax.spring,
                super::prim::mix(a, e, 0.22),
                super::prim::mix(a, e, 0.88),
                mean,
                wire,
                coils,
            );
        }
    }
    if steer {
        tie_rod(x, ax, c);
    }
}

fn multilink(x: &mut Ctx, ax: &Axle, c: &Corner) {
    let k = c.k_in;
    x.link(
        &ax.metal,
        c.p(k + 0.03, 0.02, 0.17),
        c.p(k + 0.33, 0.0, 0.19),
        0.012,
    );
    x.arm(
        &ax.dark,
        c.p(k + 0.03, 0.04, -0.09),
        c.p(k + 0.43, 0.06, -0.1),
    );
    x.link(
        &ax.metal,
        c.p(k + 0.02, 0.06, -0.04),
        c.p(k + 0.1, 0.46, -0.03),
        0.012,
    );
    x.link(
        &ax.metal,
        c.p(k + 0.035, -0.12, -0.02),
        c.p(k + 0.38, -0.12, -0.03),
        0.011,
    );
    x.spring(
        &ax.spring,
        c.p(k + 0.25, 0.05, -0.07),
        c.p(k + 0.25, 0.05, 0.13),
        0.055,
        0.0075,
        4.0,
    );
    x.damper(
        ax,
        c.p(k + 0.06, -0.1, -0.08),
        c.p(k + 0.07, -0.11, 0.16),
        c.p(k + 0.08, -0.12, 0.36),
        0.02,
    );
}

fn torsion_beam_corner(x: &mut Ctx, ax: &Axle, c: &Corner) {
    let k = c.k_in;
    x.arm(&ax.dark, c.p(k + 0.12, 0.45, 0.02), c.p(k + 0.01, 0.0, 0.0));
    x.spring(
        &ax.spring,
        c.p(k + 0.1, 0.12, -0.02),
        c.p(k + 0.1, 0.12, 0.18),
        0.055,
        0.0075,
        4.5,
    );
    x.damper(
        ax,
        c.p(k + 0.05, -0.08, -0.05),
        c.p(k + 0.06, -0.05, 0.14),
        c.p(k + 0.07, -0.02, 0.32),
        0.02,
    );
}

fn leaf_corner(x: &mut Ctx, ax: &Axle, c: &Corner, leaves: u8, over: bool, axle_r: f32) {
    let in_l = (c.k_in + 0.12).max(0.24);
    let main_len = 1.45f32;
    let n = if x.hero {
        leaves.clamp(1, 6)
    } else {
        leaves.clamp(1, 3)
    };
    let segs = if x.hero { 4 } else { 3 };
    let t = 0.007;
    let z_top = if over {
        c.h[2] + axle_r + t * n as f32 + 0.004
    } else {
        c.h[2] - axle_r - 0.004
    };
    let xl = c.h[0] - c.s * in_l;
    let prof = [
        [-0.032, -0.5 * t],
        [0.032, -0.5 * t],
        [0.032, 0.5 * t],
        [-0.032, 0.5 * t],
    ];
    let mut eyes = ([0.0; 3], [0.0; 3]);
    for j in 0..n {
        let len = (main_len - 0.22 * j as f32).max(0.4);
        let zc = z_top - 0.5 * t - (t + 0.0005) * j as f32;
        let path: Vec<[f32; 3]> = (0..=segs)
            .map(|i| {
                let f = -1.0 + 2.0 * i as f32 / segs as f32;
                [xl, c.h[1] + 0.5 * len * f, zc + 0.07 * f * f]
            })
            .collect();
        if j == 0 {
            eyes = (path[segs], path[0]);
        }
        x.b.extrude(&ax.dark, &prof, &path, [0.0, 0.0, 1.0], true);
    }
    // Front hanger and rear shackle up to the frame.
    let (front, rear) = eyes;
    x.b.cuboid(
        &ax.dark,
        [xl - 0.02, front[1] - 0.03, front[2]],
        [xl + 0.02, front[1] + 0.03, front[2] + 0.08],
    );
    bar(
        x.b,
        &ax.dark,
        rear,
        [rear[0], rear[1] - 0.03, rear[2] + 0.09],
        0.05,
        0.02,
        [1.0, 0.0, 0.0],
    );
    if x.hero {
        for dy in [-0.045f32, 0.045] {
            let top = if over {
                z_top + 0.01
            } else {
                c.h[2] + axle_r + 0.01
            };
            let bot = if over {
                c.h[2] - axle_r - 0.01
            } else {
                z_top - t * n as f32 - 0.01
            };
            rod(
                x.b,
                &ax.metal,
                [xl, c.h[1] + dy, bot],
                [xl, c.h[1] + dy, top],
                0.006,
                4,
            );
        }
    }
    // Staggered shocks: left ahead of the axle, right behind.
    let dy = if c.s < 0.0 { 1.0 } else { -1.0 };
    x.damper(
        ax,
        c.p(in_l - 0.07, dy * 0.13, -0.02),
        c.p(in_l - 0.05, dy * 0.2, 0.14),
        c.p(in_l - 0.03, dy * 0.27, 0.32),
        0.022,
    );
}

fn coil_axle_corner(x: &mut Ctx, ax: &Axle, c: &Corner) {
    let i = (c.k_in + 0.2).max(0.28);
    x.arm(&ax.dark, c.p(i, 0.0, -0.1), c.p(i + 0.02, 0.5, -0.06));
    x.spring(
        &ax.spring,
        c.p(i, -0.02, 0.045),
        c.p(i, -0.02, 0.33),
        0.065,
        0.009,
        4.5,
    );
    x.damper(
        ax,
        c.p(i - 0.08, -0.13, -0.03),
        c.p(i - 0.07, -0.14, 0.14),
        c.p(i - 0.06, -0.15, 0.3),
        0.022,
    );
}

/// Diff housing (pumpkin) on the centre line, nose forward, at `y`, `z`.
fn diff(x: &mut Ctx, k: &PartKey, y: f32, z: f32, big: bool) {
    let r = if big { 0.15 } else { 0.13 };
    let n = if x.hero { 10 } else { 8 };
    x.b.lathe(
        k,
        [0.0, y, z],
        [0.0, 1.0, 0.0],
        &[
            [0.0, -0.09],
            [0.85 * r, -0.07],
            [r, 0.01],
            [0.7 * r, 0.09],
            [0.35 * r, 0.12],
            [0.0, 0.13],
        ],
        n,
    );
}

/// Emit the suspension, driveline and frame of the car.
pub fn suspension(def: &VehicleDef, p: &BodyProfile, s: &CarSpec, e: &EngineOut, b: &mut Builder) {
    let mut x = Ctx { b, hero: s.hero };
    let corners: Vec<Corner> = def
        .wheels
        .iter()
        .enumerate()
        .map(|(i, w)| Corner {
            h: w.position_m,
            s: if w.position_m[0] < 0.0 { -1.0 } else { 1.0 },
            k_in: knuckle_in(w.width_m),
            travel: def.suspension.get(i).map_or(0.18, |d| d.travel_m),
        })
        .collect();
    let y_fa = def.wheels[0].position_m[1];
    let y_ra = def.wheels[2].position_m[1];
    let truck = def.body_size_m[2] > 1.7;
    let rear_driven = !matches!(s.mount, Mount::Transverse) || s.awd;
    let front_driven = s.mount == Mount::Transverse || s.awd;
    for front in [true, false] {
        let ax = Axle::new(front, s);
        let kind = if front { s.front_susp } else { s.rear_susp };
        let cs: Vec<Corner> = corners
            .iter()
            .copied()
            .filter(|c| (c.h[1] >= 0.0) == front)
            .collect();
        if cs.len() < 2 {
            continue;
        }
        let axle_r = if def.wheels[2].radius_m > 0.36 {
            0.044
        } else {
            0.038
        };
        for c in &cs {
            match kind {
                Susp::Strut => strut(&mut x, &ax, c, front, s.tinted_springs),
                Susp::Sla | Susp::SlaCoil | Susp::SlaTorsion | Susp::Coilover => {
                    sla(&mut x, &ax, c, kind, front, s.tinted_springs)
                }
                Susp::Multilink => multilink(&mut x, &ax, c),
                Susp::TorsionBeam => torsion_beam_corner(&mut x, &ax, c),
                Susp::LeafAxle { leaves, over } => {
                    leaf_corner(&mut x, &ax, c, leaves, over, axle_r)
                }
                Susp::CoilAxle => coil_axle_corner(&mut x, &ax, c),
            }
        }
        let (l, r) = (cs[0], cs[1]);
        let y = 0.5 * (l.h[1] + r.h[1]);
        let z = 0.5 * (l.h[2] + r.h[2]);
        match kind {
            Susp::TorsionBeam => {
                bar(
                    x.b,
                    &ax.dark,
                    l.p(l.k_in + 0.08, 0.22, -0.02),
                    r.p(r.k_in + 0.08, 0.22, -0.02),
                    0.09,
                    0.06,
                    [0.0, 0.0, 1.0],
                );
            }
            Susp::LeafAxle { .. } | Susp::CoilAxle => {
                diff(&mut x, &ax.dark, y, z, truck);
                if kind == Susp::CoilAxle {
                    x.link(
                        &ax.metal,
                        l.p(0.25, -0.17, 0.1),
                        r.p(0.18, -0.17, 0.0),
                        0.015,
                    );
                }
            }
            _ => {
                let driven = if front { front_driven } else { rear_driven };
                if driven && !(front && s.mount == Mount::Transverse) {
                    x.b.beveled_box(
                        &ax.dark,
                        [-0.13, y - 0.1, z - 0.09],
                        [0.13, y + 0.1, z + 0.09],
                        if x.hero { 0.02 } else { 0.0 },
                    );
                }
                if driven {
                    for c in [l, r] {
                        let inner = [c.s * 0.12, y, z];
                        x.link(&ax.metal, inner, c.p(c.k_in + 0.05, 0.0, 0.0), 0.02);
                    }
                }
                if !front && kind == Susp::Multilink {
                    // Subframe cross tube.
                    bar(
                        x.b,
                        &ax.dark,
                        l.p(l.k_in + 0.38, 0.22, -0.06),
                        r.p(r.k_in + 0.38, 0.22, -0.06),
                        0.07,
                        0.05,
                        [0.0, 0.0, 1.0],
                    );
                }
            }
        }
        if x.hero && front && !kind.live_axle() {
            // Anti-roll bar.
            let zb = z - 0.08;
            let path = [
                l.p(l.k_in + 0.1, 0.02, -0.08),
                [0.5 * l.h[0], y + 0.2, zb],
                [0.5 * r.h[0], y + 0.2, zb],
                r.p(r.k_in + 0.1, 0.02, -0.08),
            ];
            tube(x.b, &ax.metal, &path, 0.011, 6, false);
        }
    }
    // Driveline: driveshaft from the gearbox to the rear diff / transaxle.
    let line = PartKey::body(
        "driveline",
        BodyRegion::FrameRear,
        PartKind::Suspension,
        MaterialSlot::Metal,
    )
    .rigid();
    let hub_z = def.wheels[2].position_m[2];
    if let (Some(g), true) = (e.gearbox_end, rear_driven && !e.mid) {
        let end = [0.0, y_ra + 0.2, hub_z];
        if end[1] < g[1] - 0.2 {
            rod(x.b, &line, g, end, 0.035, if x.hero { 8 } else { 6 });
        }
    }
    if s.awd && e.mid {
        let fz = def.wheels[0].position_m[2];
        let dk = PartKey::body(
            "driveline_front",
            BodyRegion::FrameFront,
            PartKind::Suspension,
            MaterialSlot::Engine,
        )
        .rigid();
        x.b.cuboid(
            &dk,
            [-0.1, y_fa - 0.1, fz - 0.1],
            [0.1, y_fa + 0.1, fz + 0.1],
        );
        rod(
            x.b,
            &line,
            [-0.12, y_ra + 1.05, e.crank_z - 0.1],
            [-0.04, y_fa - 0.1, fz],
            0.03,
            6,
        );
    }
    // Frame rails (body-on-frame).
    if s.frame_rails {
        let half_track = 0.5 * (corners[0].h[0].abs() + corners[2].h[0].abs());
        let xr = (half_track - 0.3).max(0.35);
        let h = if truck { 0.15 } else { 0.1 };
        let top = (p.z_floor + 0.03).max(if truck { 0.45 } else { 0.27 });
        let (y0, y1) = (y_ra - 0.8, y_fa + 0.6);
        let rail = PartKey::body(
            "frame_rail_l",
            BodyRegion::FrameLeft,
            PartKind::Suspension,
            MaterialSlot::Engine,
        )
        .rigid();
        x.b.cuboid(&rail, [-xr - 0.035, y0, top - h], [-xr + 0.035, y1, top]);
        let rail_r = PartKey::body(
            "frame_rail_r",
            BodyRegion::FrameRight,
            PartKind::Suspension,
            MaterialSlot::Engine,
        )
        .rigid();
        x.b.mirror_part("frame_rail_l", &rail_r);
        if x.hero || truck {
            let cross = PartKey::body(
                "frame_cross",
                BodyRegion::FrameFront,
                PartKind::Suspension,
                MaterialSlot::Engine,
            )
            .rigid();
            x.b.cuboid(
                &cross,
                [-xr, y_fa - 0.35, top - h],
                [xr, y_fa - 0.25, top - 0.02],
            );
        }
    }
}
