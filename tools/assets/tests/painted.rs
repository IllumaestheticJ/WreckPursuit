//! Painted-car pipeline: section specs, detail UVs and the code-painted texture.

use car_assets::mesh::{painted_detail, stations, MaterialSlot, PartKind};
use car_assets::paint::layout::{IslandKey, Layout};
use car_assets::paint::{self, canvas};
use car_assets::profile::BodyProfile;
use car_assets::section::{Curve, SectionSpec};
use car_assets::style::{style_for, Tier};
use car_assets::styles::models::{spec_for, MODELS};
use car_assets::{build_mid_mesh, load_car, repo_root};

/// The model ids that exist in `data/cars` (the family may list trims not built yet).
fn painted_ids() -> Vec<&'static str> {
    let root = repo_root();
    MODELS
        .iter()
        .map(|m| m.0)
        .filter(|id| root.join(format!("data/cars/{id}.json")).exists())
        .collect()
}

#[test]
fn model_cars_are_painted_and_their_specs_are_valid() {
    let root = repo_root();
    assert!(!painted_ids().is_empty());
    for id in painted_ids() {
        let car = load_car(&root, id).unwrap();
        let style = style_for(&car.def);
        assert_eq!(style.tier, Tier::Painted, "{id}");
        let spec = spec_for(&car.def).unwrap();
        assert!(spec.validate().is_empty(), "{id}: {:?}", spec.validate());
        assert!(spec.trims.contains_key(&spec.active_trim), "{id}");
    }
}

/// The texture is the same bytes every time it is painted (no cache involved).
#[test]
fn painting_is_deterministic() {
    let root = repo_root();
    let car = load_car(&root, "nissan_skyline_r34_gtr").unwrap();
    let style = style_for(&car.def);
    let (layout, parts, cached) = painted_detail(&car.def, &car.lattice, &style).unwrap();
    let prof = BodyProfile::styled(&car.def, &car.lattice, &style.body);
    let a = paint::paint(&car.def, &prof, &layout, &parts);
    let b = paint::paint(&car.def, &prof, &layout, &parts);
    assert!(a == b);
    let png = canvas::png_rgba8(a.size, a.size, &a.to_rgba8());
    assert_eq!(png, cached.png);
    let (w, h, px) = canvas::decode_png(&png).unwrap();
    assert_eq!((w, h), (layout.size, layout.size));
    assert_eq!(px, a.to_rgba8());
}

/// Islands never overlap (each island owns its texels; the four wheels share one island).
#[test]
fn islands_do_not_overlap() {
    let root = repo_root();
    for id in painted_ids() {
        let car = load_car(&root, id).unwrap();
        let style = style_for(&car.def);
        let (layout, _, _) = painted_detail(&car.def, &car.lattice, &style).unwrap();
        let isl: Vec<_> = layout.islands.values().collect();
        assert!(isl.len() > 10, "{id}");
        for (i, a) in isl.iter().enumerate() {
            assert!(a.px[0] + a.size[0] <= layout.size && a.px[1] + a.size[1] <= layout.size);
            for b in &isl[i + 1..] {
                let apart = a.px[0] + a.size[0] < b.px[0]
                    || b.px[0] + b.size[0] < a.px[0]
                    || a.px[1] + a.size[1] < b.px[1]
                    || b.px[1] + b.size[1] < a.px[1];
                assert!(apart, "{id}: {:?} overlaps {:?}", a.key, b.key);
            }
        }
        // Body texel size: about 5–8 mm.
        let m = layout.body_m_per_px();
        assert!((0.004..0.009).contains(&m), "{id}: {m} m/px");
    }
}

/// The mid LOD uses the full mesh's layout: every mid part's detail UVs fall inside an island of
/// the same part (or the car-wide frames / wheel island), and both LODs name the same texture.
#[test]
fn detail_uvs_match_between_lods() {
    let root = repo_root();
    for id in painted_ids() {
        let car = load_car(&root, id).unwrap();
        let style = style_for(&car.def);
        let (layout, full, _) = painted_detail(&car.def, &car.lattice, &style).unwrap();
        let mid = build_mid_mesh(&car);
        assert!(mid.detail.is_some(), "{id}");
        let size = layout.size as f32;
        for p in &mid.parts {
            let f = full.iter().find(|q| q.name == p.name).unwrap();
            assert_eq!(p.duvs.is_empty(), f.duvs.is_empty(), "{id}/{}", p.name);
            let own: Vec<_> = layout
                .islands
                .values()
                .filter(|i| match &i.key {
                    IslandKey::Part(n, _) => *n == p.name,
                    IslandKey::Wheel => p.wheel.is_some(),
                    IslandKey::Frame(_) => p.material == MaterialSlot::Livery,
                })
                .collect();
            for uv in p.duvs.chunks_exact(2) {
                let (u, v) = (uv[0] as f32 / 65535.0 * size, uv[1] as f32 / 65535.0 * size);
                let inside = own.iter().any(|i| {
                    u >= i.px[0] as f32 - 0.5
                        && v >= i.px[1] as f32 - 0.5
                        && u <= (i.px[0] + i.size[0]) as f32 + 0.5
                        && v <= (i.px[1] + i.size[1]) as f32 + 0.5
                });
                assert!(inside, "{id}/{}: duv {u},{v} outside its islands", p.name);
            }
        }
    }
}

/// Panel gaps lie on panel boundaries: every shut-line texel has a neighbour in the same frame
/// showing another panel.
#[test]
fn painted_gaps_lie_on_part_boundaries() {
    let root = repo_root();
    let car = load_car(&root, "nissan_skyline_r34_gtr").unwrap();
    let style = style_for(&car.def);
    let (layout, parts, _) = painted_detail(&car.def, &car.lattice, &style).unwrap();
    let tex = paint::rasterise(&layout, &parts);
    let islands: Vec<_> = layout.islands.values().collect();
    let size = layout.size;
    let mask = paint::gap_mask(&tex, &parts, &islands, size);
    let two = paint::is_two_door(&parts);
    assert!(two, "the R34 is a coupe");
    let lines = mask.iter().filter(|&&m| m == 2).count();
    assert!(lines > 500, "only {lines} gap texels");
    for (i, &m) in mask.iter().enumerate() {
        if m != 2 {
            continue;
        }
        let g = paint::gap_group_of(&tex, &parts, &islands, i, two).unwrap();
        let n = [i + 1, i + size as usize];
        assert!(
            n.iter()
                .any(|&j| paint::gap_group_of(&tex, &parts, &islands, j, two)
                    .is_some_and(|h| h.0 == g.0 && h.1 != g.1)),
            "gap texel {i} is not on a panel boundary"
        );
    }
}

/// Features stay inside their part: the paint mask (alpha) is set only on body-paint parts,
/// lamps / wheels / trim get opaque colours, and every lamp lens shows painted detail (not the
/// blank background).
#[test]
fn painted_features_stay_inside_their_part() {
    let root = repo_root();
    let car = load_car(&root, "nissan_skyline_r34_gtr").unwrap();
    let style = style_for(&car.def);
    let (layout, parts, _) = painted_detail(&car.def, &car.lattice, &style).unwrap();
    let prof = BodyProfile::styled(&car.def, &car.lattice, &style.body);
    let tex = paint::rasterise(&layout, &parts);
    let c = paint::paint(&car.def, &prof, &layout, &parts);
    let mut lamp_texels = 0;
    for (i, t) in tex.iter().enumerate() {
        if t.part == 0 {
            continue;
        }
        let p = &parts[t.part as usize - 1];
        let a = c.px[i][3];
        if !p.material.is_paint() {
            assert!(
                a < 0.01,
                "{}: paint mask on a {:?} part",
                p.name,
                p.material
            );
        }
        if p.kind == PartKind::Light {
            lamp_texels += 1;
            let rgb = &c.px[i][..3];
            assert!(
                rgb.iter().any(|&v| v < 0.95),
                "{}: blank lamp texel",
                p.name
            );
        }
    }
    assert!(lamp_texels > 2000, "{lamp_texels} lamp texels");
}

/// Section specs at their extremes (inverted lines, tiny / huge widths, spikes, empty overlays)
/// never panic the profile or the generator.
#[test]
fn section_spec_extremes_never_panic() {
    let root = repo_root();
    let car = load_car(&root, "nissan_skyline_r34_gtr").unwrap();
    let base = spec_for(&car.def).unwrap();
    let scale =
        |c: &Curve, k: f32, d: f32| Curve(c.0.iter().map(|q| [q[0], q[1] * k + d]).collect());
    let mut variants: Vec<SectionSpec> = Vec::new();
    for (k, d) in [(0.0, 0.0), (0.3, 0.0), (2.0, 0.4), (1.0, -0.5), (1.0, 1.0)] {
        let mut s = base.clone();
        s.lines.belt = scale(&s.lines.belt, k, d);
        s.lines.greenhouse = scale(&s.lines.greenhouse, k, d);
        variants.push(s);
        let mut s = base.clone();
        s.plan.half_width = scale(&s.plan.half_width, k, d);
        s.plan.roof_half_width = scale(&s.plan.roof_half_width, k, -d);
        s.plan.haunch = scale(&s.plan.haunch, k * 5.0, d);
        variants.push(s);
        let mut s = base.clone();
        s.lines.sill = scale(&s.lines.sill, k, d);
        s.keyframes.iter_mut().for_each(|f| {
            f.crease_z_m = Some(d * 3.0);
            f.shoulder_round_m = Some(k);
            f.glass_inset_m = Some(-k);
        });
        variants.push(s);
    }
    let mut s = base.clone();
    s.anchors.y_windshield_base = s.anchors.y_nose + 1.0;
    s.anchors.y_deck = s.anchors.y_tail - 1.0;
    s.anchors.quarter_glass_y = Some([1.0, -1.0]);
    variants.push(s);
    let mut s = base.clone();
    s.lines.greenhouse = Curve(vec![[0.0, 3.0], [0.01, 0.0]]);
    variants.push(s);
    for (i, v) in variants.into_iter().enumerate() {
        let mut st = style_for(&car.def);
        st.body.sections = Some(std::sync::Arc::new(v));
        let p = BodyProfile::styled(&car.def, &car.lattice, &st.body);
        let ys = stations(&p, 0.15, 0.06);
        assert!(ys.len() > 10, "variant {i}");
        let m = car_assets::mesh::generate_lod(
            &car.def,
            &car.lattice,
            &st,
            car_assets::mesh::Lod::Full,
        );
        assert!(m.tri_count > 1000, "variant {i}");
    }
}

/// A section spec moves the visual body but keeps the lattice's zone stations (hood front,
/// door hinge and rear edge, quarters) as loft stations, so every panel starts on its zone.
/// (Hinge lines themselves: `hinged_parts_start_at_their_hinge_line` in `assets.rs`.)
#[test]
fn section_spec_keeps_zones_and_hinges() {
    let root = repo_root();
    for id in painted_ids() {
        let car = load_car(&root, id).unwrap();
        let style = style_for(&car.def);
        let p = BodyProfile::styled(&car.def, &car.lattice, &style.body);
        assert!(p.spec().is_some(), "{id}");
        let ys = stations(&p, 0.15, 0.06);
        for (name, k) in [
            ("y_bf", p.y_bf),
            ("y_fd_hinge", p.y_fd_hinge),
            ("y_fd_rear", p.y_fd_rear),
            ("y_quarter.0", p.y_quarter.0),
            ("y_quarter.1", p.y_quarter.1),
            ("y_fw", p.y_fw),
            ("y_deck", p.y_deck),
        ] {
            // (A key station closer than 0.4 × the fine step to another key, e.g. an arch edge,
            // gives way to it, as on every car.)
            assert!(
                ys.iter().any(|y| (y - k).abs() < 0.4 * 0.06 + 1e-3),
                "{id}: {name} {k} is not a loft station"
            );
        }
        // Steps: never longer than the flat-span cap (plus a key station's keep-out).
        assert!(
            ys.windows(2)
                .all(|w| w[1] - w[0] <= 0.15 + 0.4 * 0.06 + 1e-3),
            "{id}"
        );
        // The door hinge comes from the lattice.
        let hinge = car
            .lattice
            .detachables
            .iter()
            .find(|d| d.region == wp_contracts::BodyRegion::LeftDoorF)
            .map(|d| car.lattice.nodes[d.hinge_nodes[0] as usize].pos_m[1])
            .unwrap();
        assert!((p.y_fd_hinge - hinge).abs() < 1e-4, "{id}");
    }
}

/// The layout gives every textured part detail UVs on the full mesh.
#[test]
fn every_textured_part_has_detail_uvs() {
    let root = repo_root();
    for id in painted_ids() {
        let car = load_car(&root, id).unwrap();
        let style = style_for(&car.def);
        let (_, parts, _) = painted_detail(&car.def, &car.lattice, &style).unwrap();
        for p in &parts {
            let want = car_assets::paint::layout::textured(p);
            assert_eq!(
                p.duvs.len(),
                if want { p.vertex_count() * 2 } else { 0 },
                "{id}/{}",
                p.name
            );
        }
        let _ = Layout::build(&parts);
    }
}
