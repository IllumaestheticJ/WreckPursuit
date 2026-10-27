//! `cargo run -p car-assets --example phase3_demo -- <out_dir>`: scratch demo of the car-shapes
//! phase 3 knobs (plan-width curve, side sculpts and section shape, fender tops, overhangs,
//! projected lamps). Writes `<out_dir>/<car>__<variant>.json` meshes (full LOD, not skinned)
//! for a few cars; render them with
//! `MESH_DIR=<out_dir> node tools/assets/mesh_shots.mjs <shots_dir> <car>__<variant> …`.
//!
//! These are demonstrations only, not the cars' styles (the per-car sessions own those).

use car_assets::builder::{Builder, PartKey};
use car_assets::lamp::{ellipse, LampView};
use car_assets::mesh::{generate_styled, MaterialSlot};
use car_assets::profile::BodyProfile;
use car_assets::style::{style_for, CarStyle, Detail, DetailCtx, Shape, SideSculpt};
use car_assets::styles::performance::kit::{
    headlight_key, indicator_front_key, indicator_rear_key, taillight_key, Kit, Seg, SIDES,
};
use car_assets::{load_car, repo_root};
use wp_contracts::{BodyRegion, VehicleDef};

/// Front / rear wheel stations (y, radius) of a car.
fn axles(def: &VehicleDef) -> ((f32, f32), (f32, f32)) {
    let f = def
        .wheels
        .iter()
        .max_by(|a, b| a.position_m[1].total_cmp(&b.position_m[1]))
        .map_or((1.3, 0.33), |w| (w.position_m[1], w.radius_m));
    let r = def
        .wheels
        .iter()
        .min_by(|a, b| a.position_m[1].total_cmp(&b.position_m[1]))
        .map_or((-1.3, 0.33), |w| (w.position_m[1], w.radius_m));
    (f, r)
}

/// A kit over an existing build whose own head / tail lamps and signals are cleared (the
/// car's other kit parts stay).
fn relamp<'a>(p: &'a BodyProfile, b: &'a mut Builder, front: bool, rear: bool) -> Kit<'a> {
    for sx in SIDES {
        if front {
            b.set_geometry(&headlight_key(sx).name, Vec::new(), Vec::new(), Vec::new());
            b.remove_part(&indicator_front_key(sx).name);
            for n in ["headlamp_housing_l", "headlamp_housing_r"] {
                b.remove_part(n);
            }
        }
        if rear {
            b.set_geometry(&taillight_key(sx).name, Vec::new(), Vec::new(), Vec::new());
            for m in [MaterialSlot::EmissiveAmber, MaterialSlot::Taillight] {
                b.remove_part(&indicator_rear_key(sx, m).name);
            }
        }
    }
    Kit {
        p,
        b,
        yn: p.y_nose,
        yt: p.y_tail,
    }
}

/// Sedan: a headlamp drawn in the front view that runs to the corner and wraps onto the
/// fender (outboard amber segment, bezel), and a tail lamp wrapping round the rear corner.
fn wrap_lamps(ctx: &DetailCtx, _k: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let mut kit = relamp(p, b, true, true);
    let (wn, (_, zt)) = (kit.wn(), kit.nose_z());
    for sx in SIDES {
        let head = [
            [-(wn - 0.30), zt - 0.11],
            [-(wn + 0.02), zt - 0.07],
            [-(wn + 0.02), zt - 0.025],
            [-(wn - 0.36), zt - 0.04],
        ];
        kit.proj_cluster(
            sx,
            true,
            LampView::Front,
            &head,
            Some(Seg::Outboard(0.06)),
            MaterialSlot::EmissiveAmber,
            0.012,
            20.0,
        );
        let (wt, (_, tt)) = (kit.wt(), kit.tail_z());
        let tail = [
            [-(wt - 0.34), tt - 0.13],
            [-(wt + 0.03), tt - 0.13],
            [-(wt + 0.03), tt - 0.03],
            [-(wt - 0.40), tt - 0.05],
        ];
        kit.proj_cluster(
            sx,
            false,
            LampView::Rear,
            &tail,
            Some(Seg::Inboard(0.09)),
            MaterialSlot::EmissiveAmber,
            0.010,
            35.0,
        );
    }
}

/// Hatch: teardrop head lamps drawn in the top view, lying on the curved nose and swept back
/// along the fender top (leading amber strip), round tail lamps on the rounded tail.
fn swept_lamps(ctx: &DetailCtx, _k: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let mut kit = relamp(p, b, true, true);
    let (wn, yn) = (kit.wn(), kit.yn);
    for sx in SIDES {
        let tear = [
            [-(wn - 0.05), yn - 0.03],
            [-(wn - 0.30), yn - 0.07],
            [-(wn - 0.24), yn - 0.26],
            [-(wn - 0.10), yn - 0.42],
            [-(wn - 0.05), yn - 0.30],
        ];
        kit.proj_cluster(
            sx,
            true,
            LampView::Top,
            &tear,
            Some(Seg::Lower(0.04)),
            MaterialSlot::EmissiveAmber,
            0.010,
            0.0,
        );
        let (wt, (_, tt)) = (kit.wt(), kit.tail_z());
        let round = ellipse([-(wt - 0.16), tt - 0.10], [0.085, 0.065], 16);
        kit.proj_cluster(
            sx,
            false,
            LampView::Rear,
            &round,
            Some(Seg::Inboard(0.05)),
            MaterialSlot::Taillight,
            0.012,
            15.0,
        );
    }
}

/// Prototype: slim LED head strips on the curved nose, a full-width tail strip split in two.
fn led_strips(ctx: &DetailCtx, _k: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let mut kit = relamp(p, b, true, true);
    let (wn, (_, zt)) = (kit.wn(), kit.nose_z());
    let (wt, (_, tt)) = (kit.wt(), kit.tail_z());
    for sx in SIDES {
        let strip = [
            [-(wn - 0.40), zt - 0.035],
            [-(wn + 0.02), zt - 0.02],
            [-(wn + 0.02), zt + 0.005],
            [-(wn - 0.42), zt - 0.012],
        ];
        kit.proj_cluster(
            sx,
            true,
            LampView::Front,
            &strip,
            Some(Seg::Outboard(0.05)),
            MaterialSlot::EmissiveAmber,
            0.006,
            25.0,
        );
        let tail = [
            [-0.06, tt - 0.06],
            [-(wt + 0.02), tt - 0.06],
            [-(wt + 0.02), tt - 0.03],
            [-0.06, tt - 0.03],
        ];
        kit.proj_cluster(
            sx,
            false,
            LampView::Rear,
            &tail,
            Some(Seg::Outboard(0.10)),
            MaterialSlot::Taillight,
            0.0,
            30.0,
        );
    }
}

fn custom(f: car_assets::style::DetailFn) -> Detail {
    Detail::new(
        "demo_lamps",
        BodyRegion::Cabin,
        MaterialSlot::Trim,
        Shape::Custom(f),
    )
}

/// Every demo variant of a car: (name, style).
fn variants(
    id: &str,
    def: &VehicleDef,
    p: &BodyProfile,
    base: &CarStyle,
) -> Vec<(&'static str, CarStyle)> {
    let ((fy, fr), (ry, rr)) = axles(def);
    // Side heights from the styled profile: sill, belt at the doors.
    let (sill, belt) = (p.sill(0.0), p.belt(0.0));
    let hgt = belt - sill;
    let mut out = vec![("base", base.clone())];
    let mut v = |name: &'static str, f: &dyn Fn(&mut CarStyle)| {
        let mut s = base.clone();
        f(&mut s);
        out.push((name, s));
    };
    match id {
        "kestrel_ts" => {
            // Coke-bottle waist: full width over the arches, 5 % in at the doors.
            v("plan_coke", &|s| {
                s.body.plan_width = vec![
                    [fy - fr - 0.1, 1.0],
                    [0.5 * (fy + ry), 0.95],
                    [ry + rr + 0.1, 1.0],
                ];
            });
            // Strong shoulder line, a door cove and a convex section.
            v("sculpt", &|s| {
                s.body.side_sculpt = vec![
                    SideSculpt {
                        y0_m: ry - 0.2,
                        y1_m: fy + 0.2,
                        z0_m: belt - 0.10,
                        z1_m: belt - 0.07,
                        dx_m: 0.025,
                        falloff_m: 0.04,
                    },
                    SideSculpt {
                        y0_m: ry + rr + 0.3,
                        y1_m: fy - fr - 0.4,
                        z0_m: sill + 0.25 * hgt,
                        z1_m: sill + 0.5 * hgt,
                        dx_m: -0.045,
                        falloff_m: 0.12,
                    },
                ];
                s.body.side_bulge_m = 0.03;
                s.body.rocker_in_m = 0.04;
            });
            v("lamps", &|s| s.details.push(custom(wrap_lamps)));
            // Long nose and short tail (the new range: −0.35…+0.25 m).
            v("overhang", &|s| s.body.overhang_trim = (0.25, -0.30));
        }
        "vireo_xs" => {
            // Rounded-wedge nose and tapered tail.
            v("plan_taper", &|s| {
                s.body.plan_width = vec![
                    [ry - rr - 0.1, 1.0],
                    [ry - rr - 0.6, 0.92],
                    [fy + fr, 1.0],
                    [fy + fr + 0.6, 0.86],
                ];
            });
            v("section", &|s| {
                s.body.side_bulge_m = 0.04;
                s.body.side_bulge_z = 0.6;
                s.body.rocker_in_m = 0.05;
                s.body.tumblehome_m = Some(0.18);
            });
            v("lamps", &|s| s.details.push(custom(swept_lamps)));
            v("overhang", &|s| s.body.overhang_trim = (-0.30, 0.20));
        }
        "hauler_1500" => {
            // Fenders wider than the cab and bed sides.
            v("plan_fenders", &|s| {
                s.body.plan_width = vec![
                    [fy - fr - 0.15, 1.0],
                    [fy - fr - 0.45, 0.93],
                    [ry + rr + 0.45, 0.93],
                    [ry + rr + 0.15, 1.0],
                ];
            });
            // Short front overhang (pre-runner), longer bed overhang.
            v("overhang", &|s| s.body.overhang_trim = (-0.35, 0.15));
            v("upright_glass", &|s| {
                s.body.tumblehome_m = Some(0.05);
                s.body.glass_inset = 0.04;
            });
        }
        "nyx_proto" => {
            // Fender peaks above a low hood without sinking it (no valley), pods behind the
            // front wheels stepping in.
            v("fender_rise", &|s| {
                s.body.hood_valley = 0.0;
                s.body.front_fender_rise_m = 0.10;
                s.body.rear_fender_rise_m = 0.08;
                s.body.fender_peak_w = 0.75;
            });
            v("side_pods", &|s| {
                s.body.side_sculpt = vec![SideSculpt {
                    y0_m: ry + rr + 0.1,
                    y1_m: fy - fr - 0.15,
                    z0_m: 0.0,
                    z1_m: sill + 0.7 * hgt,
                    dx_m: -0.07,
                    falloff_m: 0.15,
                }];
            });
            v("lamps", &|s| s.details.push(custom(led_strips)));
        }
        "ironside_440" => {
            // Long-hood fender tops standing above the hood centre; pontoon rear haunch.
            v("fender_rise", &|s| {
                s.body.front_fender_rise_m = 0.05;
                s.body.fender_peak_w = 0.85;
                s.body.side_sculpt = vec![SideSculpt {
                    y0_m: ry - 0.3,
                    y1_m: ry + 0.3,
                    z0_m: sill + 0.45 * hgt,
                    z1_m: belt,
                    dx_m: 0.04,
                    falloff_m: 0.2,
                }];
            });
        }
        _ => {}
    }
    out
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/phase3_demo".into());
    std::fs::create_dir_all(&out).expect("out dir");
    let root = repo_root();
    for id in [
        "kestrel_ts",
        "vireo_xs",
        "hauler_1500",
        "nyx_proto",
        "ironside_440",
    ] {
        let car = load_car(&root, id).expect("car");
        let base = style_for(&car.def);
        let p = BodyProfile::styled(&car.def, &car.lattice, &base.body);
        for (name, st) in variants(id, &car.def, &p, &base) {
            let m = generate_styled(&car.def, &car.lattice, &st);
            let path = format!("{out}/{id}__{name}.json");
            std::fs::write(&path, m.to_json()).expect("write");
            println!("{path}: {} tris", m.tri_count);
        }
    }
}
