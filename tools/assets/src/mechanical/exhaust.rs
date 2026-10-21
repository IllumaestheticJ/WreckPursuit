//! Exhaust (`research_mechanical.md` §1.2 exhaust notes): pipe runs from the engine's
//! manifold / header / turbo exits down under the floor, mufflers, and the tips (round,
//! oval, trapezoid, turned down, quad, centre, high, side exits, bare pipes, side pipes).
//!
//! Parts (rigid, `Suspension` kind, `FrameRear`): `exhaust_pipe` (exhaust metal),
//! `exhaust_muffler` (dark), `exhaust_tip` (chrome, or plain exhaust metal on police / work
//! vehicles), `exhaust_tip_inner` (dark mouth).

use wp_contracts::{BodyRegion, VehicleClass, VehicleDef};

use crate::builder::{Builder, PartKey};
use crate::mesh::{MaterialSlot, PartKind};
use crate::profile::BodyProfile;

use super::engine::EngineOut;
use super::prim::{add, mul, tube, unit};
use super::spec::{CarSpec, Tips};

/// Tip mouth shape.
#[derive(Clone, Copy, PartialEq)]
enum Mouth {
    Round,
    Oval,
    Trapezoid,
}

struct Keys {
    pipe: PartKey,
    muffler: PartKey,
    tip: PartKey,
    inner: PartKey,
}

/// One tip of radius `r` ending at `at`, pointing along `dir` (outward), 0.14 long.
fn tip(b: &mut Builder, k: &Keys, at: [f32; 3], dir: [f32; 3], r: f32, mouth: Mouth, hero: bool) {
    let d = unit(dir);
    let back = add(at, mul(d, -0.14));
    let sides = if hero { 12 } else { 8 };
    let prof: Vec<[f32; 2]> = match mouth {
        Mouth::Round => (0..sides)
            .map(|i| {
                let (s, c) = crate::builder::turn(i, sides);
                [c * r, s * r]
            })
            .collect(),
        Mouth::Oval => (0..sides)
            .map(|i| {
                let (s, c) = crate::builder::turn(i, sides);
                [c * 1.45 * r, s * 0.8 * r]
            })
            .collect(),
        Mouth::Trapezoid => vec![
            [-1.3 * r, -0.7 * r],
            [1.3 * r, -0.7 * r],
            [1.5 * r, 0.55 * r],
            [1.35 * r, 0.75 * r],
            [-1.35 * r, 0.75 * r],
            [-1.5 * r, 0.55 * r],
        ],
    };
    let up = if d[2].abs() > 0.9 {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    b.extrude(&k.tip, &prof, &[back, at], up, false);
    // Dark mouth recessed a little.
    let m = add(at, mul(d, -0.012));
    let side = unit(super::prim::cross(d, up));
    let upv = super::prim::cross(side, d);
    let mouth_pts: Vec<[f32; 3]> = prof
        .iter()
        .map(|q| add(m, add(mul(side, 0.88 * q[0]), mul(upv, 0.88 * q[1]))))
        .collect();
    b.poly_facing(&k.inner, &mouth_pts, d);
}

fn pipe(b: &mut Builder, k: &Keys, path: &[[f32; 3]], r: f32, hero: bool) {
    tube(b, &k.pipe, path, r, if hero { 8 } else { 6 }, false);
}

/// Muffler box centred at `c` with half sizes `h`.
fn muffler(b: &mut Builder, k: &Keys, c: [f32; 3], h: [f32; 3], hero: bool) {
    if hero {
        b.beveled_box(
            &k.muffler,
            [c[0] - h[0], c[1] - h[1], c[2] - h[2]],
            [c[0] + h[0], c[1] + h[1], c[2] + h[2]],
            0.025,
        );
    } else {
        b.cuboid(
            &k.muffler,
            [c[0] - h[0], c[1] - h[1], c[2] - h[2]],
            [c[0] + h[0], c[1] + h[1], c[2] + h[2]],
        );
    }
}

/// Emit the exhaust system.
pub fn exhaust(def: &VehicleDef, p: &BodyProfile, s: &CarSpec, e: &EngineOut, b: &mut Builder) {
    let ex = s.exhaust;
    if e.exits.is_empty() {
        return;
    }
    let hero = s.hero;
    let police = matches!(
        def.class,
        VehicleClass::PoliceCruiser | VehicleClass::PoliceInterceptor | VehicleClass::PoliceSuv
    );
    let tip_mat = if police || matches!(ex.tips, Tips::Bare | Tips::BareSide) {
        MaterialSlot::Exhaust
    } else {
        MaterialSlot::Chrome
    };
    let key = |n: &str, m: MaterialSlot| {
        PartKey::body(n, BodyRegion::FrameRear, PartKind::Suspension, m).rigid()
    };
    let k = Keys {
        pipe: key("exhaust_pipe", MaterialSlot::Exhaust),
        muffler: key("exhaust_muffler", MaterialSlot::Engine),
        tip: key("exhaust_tip", tip_mat),
        inner: key("exhaust_tip_inner", MaterialSlot::Grille),
    };
    let y_fa = def.wheels[0].position_m[1];
    let y_ra = def.wheels[2].position_m[1];
    let rear_r = def.wheels[2].radius_m;
    let pr = ex.pipe_r.clamp(0.02, 0.045);
    let tr = ex.tip_r.clamp(0.02, 0.06);
    let truck = def.body_size_m[2] > 1.7;
    let z_pipe = (p.z_floor - pr - 0.01).max(pr + 0.09);
    let hw = p.half_w;
    // Mouths 2 cm proud of the tail so they show past bumpers, valances and diffusers.
    let y_tip = p.y_tail - 0.02;
    let z_tip = (p.z_floor + 0.05).max(0.15);
    let x_out = (hw - 0.36).clamp(0.25, 0.6);

    // Pipes from the exits down to the floor, merged into one or two runs.
    let dual = ex.dual && e.exits.len() >= 2;
    let run_x: Vec<f32> = if dual { vec![-0.3, 0.3] } else { vec![0.18] };
    let mut run_starts = Vec::new();
    for &rx in &run_x {
        let mut ys: Vec<[f32; 3]> = e
            .exits
            .iter()
            .copied()
            .filter(|q| !dual || q[0].signum() == rx.signum())
            .collect();
        if ys.is_empty() {
            // All exits on the other side (twin turbos on one bank): split after a Y.
            ys = e.exits.clone();
        }
        let y_join =
            ys.iter().map(|q| q[1]).fold(f32::MAX, f32::min) - if e.mid { 0.1 } else { 0.35 };
        let join = [rx, y_join, z_pipe];
        for q in &ys {
            let mid = [
                0.5 * (q[0] + rx),
                q[1] - 0.12,
                (q[2] - 0.08).max(z_pipe + 0.03),
            ];
            pipe(b, &k, &[*q, mid, join], pr, hero);
        }
        run_starts.push(join);
    }

    let side_exit = matches!(ex.tips, Tips::SideRight | Tips::BareSide);
    if e.mid {
        // Mufflers at the tail behind the gearbox.
        let yc = (y_ra - 0.5).max(p.y_tail + 0.28);
        let zc = (p.z_floor + 0.14).max(0.24);
        let corners = ex.tips == Tips::QuadHigh;
        let high = matches!(ex.tips, Tips::QuadHigh | Tips::HighCentre);
        let zm = if ex.tips == Tips::HighCentre {
            zc + 0.1
        } else {
            zc
        };
        if corners {
            for sx in [-1.0f32, 1.0] {
                let c = [sx * 0.32, yc, zc + 0.08];
                muffler(b, &k, c, [0.1, 0.17, 0.1], hero);
                let start = run_starts[if dual && sx > 0.0 { 1 } else { 0 }];
                pipe(
                    b,
                    &k,
                    &[
                        start,
                        [sx * 0.3, start[1] - 0.2, start[2]],
                        [c[0], c[1] + 0.17, c[2]],
                    ],
                    pr,
                    hero,
                );
            }
        } else {
            muffler(b, &k, [0.0, yc, zm], [0.4, 0.11, 0.11], hero);
            for start in &run_starts {
                pipe(
                    b,
                    &k,
                    &[*start, [start[0], yc + 0.25, zm], [start[0], yc + 0.11, zm]],
                    pr,
                    hero,
                );
            }
        }
        let zt = if high {
            (p.z_bumper_top.1 - 0.12).max(zc + 0.12)
        } else {
            zc
        };
        let tips: Vec<f32> = match ex.tips {
            Tips::QuadHigh | Tips::Quad | Tips::QuadTrap => vec![-0.32, -0.17, 0.17, 0.32],
            Tips::CentreQuad => vec![-0.17, -0.06, 0.06, 0.17],
            Tips::HighCentre => vec![-0.18, -0.06, 0.06, 0.18],
            Tips::Dual | Tips::DualDown | Tips::DualOval => vec![-0.3, 0.3],
            Tips::Single | Tips::SingleDown => vec![-0.3],
            _ => vec![-0.07, 0.07],
        };
        for x in tips {
            let at = [x, y_tip, zt];
            let from = if corners {
                [x.signum() * 0.32, yc - 0.17, zt]
            } else {
                [x, yc - 0.11, zm]
            };
            pipe(
                b,
                &k,
                &[from, [x, at[1] + 0.2, zt], [x, at[1] + 0.12, zt]],
                pr,
                hero,
            );
            tip(b, &k, at, [0.0, -1.0, 0.0], tr, Mouth::Round, hero);
        }
        return;
    }

    // Front engine: run(s) back to the muffler(s).
    let (m_len, m_half) = if truck {
        (0.55, [0.13, 0.275, 0.09])
    } else {
        (0.45, [0.11, 0.225, 0.06])
    };
    let mut ym0 = if side_exit { y_ra + 0.9 } else { y_ra - 0.25 };
    if !side_exit && ym0 - m_len < p.y_tail + 0.3 {
        ym0 = p.y_tail + 0.3 + m_len;
    }
    let ym1 = ym0 - m_len;
    let zm = z_pipe + 0.02;
    let mut muff_out = Vec::new();
    for start in &run_starts {
        let mx = if dual {
            start[0].signum() * x_out.min(0.45)
        } else if side_exit {
            0.35
        } else {
            start[0]
        };
        let c = [mx, 0.5 * (ym0 + ym1), zm];
        // Resonator on hero turbo / long runs.
        let path = [
            *start,
            [mx, (start[1] - 0.5).max(ym0 + 0.05), z_pipe],
            [mx, ym0, z_pipe],
        ];
        pipe(b, &k, &path, pr, hero);
        if hero && start[1] - ym0 > 1.2 {
            let ry = 0.5 * (start[1] + ym0);
            b.cylinder(
                &k.muffler,
                [mx, ry + 0.15, z_pipe],
                [mx, ry - 0.15, z_pipe],
                0.05,
                0.05,
                8,
                true,
            );
        }
        muffler(b, &k, c, m_half, hero);
        muff_out.push([mx, ym1, z_pipe]);
    }

    let down = |at: [f32; 3]| [at[0], at[1] + 0.06, at[2] + 0.05];
    let tail_to = |b: &mut Builder, from: [f32; 3], at: [f32; 3]| {
        pipe(
            b,
            &k,
            &[
                from,
                [at[0], from[1] - 0.15, from[2]],
                [at[0], at[1] + 0.18, at[2]],
                [at[0], at[1] + 0.12, at[2]],
            ],
            pr,
            hero,
        );
    };
    match ex.tips {
        Tips::Hidden => {
            for &m in &muff_out {
                pipe(b, &k, &[m, [m[0], p.y_tail + 0.3, m[2]]], pr, hero);
            }
        }
        Tips::Bare => {
            let m = muff_out[0];
            let end = [-x_out, p.y_tail + 0.1, z_pipe - 0.02];
            pipe(
                b,
                &k,
                &[
                    m,
                    [m[0], m[1] - 0.2, m[2]],
                    [end[0], end[1] + 0.3, z_pipe],
                    end,
                ],
                pr,
                hero,
            );
        }
        Tips::SideRight | Tips::BareSide => {
            let m = muff_out[0];
            let (y_out, bare) = if ex.tips == Tips::SideRight {
                (y_ra - rear_r - 0.16, false)
            } else {
                (y_ra + rear_r + 0.12, true)
            };
            let x_end = hw - 0.08;
            let end = [x_end, y_out, z_pipe];
            if bare {
                pipe(
                    b,
                    &k,
                    &[[m[0], ym0, m[2]], [m[0], ym0 + 0.05, m[2]]],
                    pr,
                    hero,
                );
                pipe(
                    b,
                    &k,
                    &[
                        m,
                        [m[0], m[1] - 0.1, z_pipe],
                        [x_end - 0.15, y_out, z_pipe],
                        [x_end, y_out - 0.02, z_pipe - 0.04],
                    ],
                    pr,
                    hero,
                );
            } else {
                pipe(
                    b,
                    &k,
                    &[
                        m,
                        [m[0], y_out + 0.2, z_pipe],
                        [x_end - 0.2, y_out, z_pipe],
                        [x_end - 0.14, y_out, z_pipe],
                    ],
                    pr,
                    hero,
                );
                tip(b, &k, end, [1.0, 0.0, -0.15], tr, Mouth::Round, hero);
            }
        }
        Tips::SidePipes => {
            for sx in [-1.0f32, 1.0] {
                let x = sx * (hw - 0.015);
                let z = (p.z_floor + 0.035).max(0.16);
                b.cylinder(
                    &k.tip,
                    [x, y_fa - 0.45, z],
                    [x, y_ra + 0.45, z],
                    tr,
                    tr,
                    if hero { 12 } else { 8 },
                    true,
                );
            }
        }
        _ => {
            // Rear tips.
            let mut tips: Vec<([f32; 3], usize)> = Vec::new();
            let left = -x_out;
            let gap = 2.6 * tr;
            match ex.tips {
                Tips::Single | Tips::SingleDown => tips.push(([left, y_tip, z_tip], 0)),
                Tips::Dual | Tips::DualDown | Tips::DualOval => {
                    tips.push(([left, y_tip, z_tip], 0));
                    tips.push(([x_out, y_tip, z_tip], 1));
                }
                Tips::DualLeft => {
                    tips.push(([left - 0.5 * gap, y_tip, z_tip], 0));
                    tips.push(([left + 0.5 * gap, y_tip, z_tip], 0));
                }
                Tips::Quad | Tips::QuadTrap => {
                    let zt = if ex.tips == Tips::QuadTrap {
                        z_tip + 0.04
                    } else {
                        z_tip
                    };
                    for sx in [-1.0f32, 1.0] {
                        let i = usize::from(sx > 0.0);
                        tips.push(([sx * x_out - 0.5 * gap, y_tip, zt], i));
                        tips.push(([sx * x_out + 0.5 * gap, y_tip, zt], i));
                    }
                }
                Tips::Centre | Tips::QuadHigh | Tips::CentreQuad | Tips::HighCentre => {
                    tips.push(([-0.07, y_tip, z_tip], 0));
                    tips.push(([0.07, y_tip, z_tip], 0));
                }
                _ => {}
            }
            let turned = matches!(ex.tips, Tips::DualDown | Tips::SingleDown);
            let mouth = match ex.tips {
                Tips::DualOval => Mouth::Oval,
                Tips::QuadTrap => Mouth::Trapezoid,
                _ => Mouth::Round,
            };
            for (at, i) in tips {
                let m = muff_out[i.min(muff_out.len() - 1)];
                if turned {
                    let at2 = [at[0], at[1] + 0.06, at[2] - 0.01];
                    let back = down(at2);
                    pipe(
                        b,
                        &k,
                        &[
                            m,
                            [at[0], m[1] - 0.15, m[2]],
                            [at[0], back[1] + 0.25, back[2]],
                            back,
                        ],
                        pr,
                        hero,
                    );
                    tip(b, &k, at2, [0.0, -0.8, -0.6], tr, Mouth::Round, hero);
                } else {
                    tail_to(b, m, at);
                    tip(b, &k, at, [0.0, -1.0, 0.0], tr, mouth, hero);
                }
            }
        }
    }
}
