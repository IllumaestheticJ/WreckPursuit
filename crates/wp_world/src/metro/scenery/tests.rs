//! Contract tests of the scenery module, run on the full v3 pipeline (with whatever the
//! other modules currently place).

use std::collections::BTreeMap;

use super::super::*;
use super::bounds::EDGE_INSET_M;
use super::{off_road, Index};
use crate::geom::{self, V2};
use crate::metro::ctx::{footprint, Occ, OCC_CELL_M};
use crate::metro::ground::{bounds, inside, signed_dist};
use crate::metro::plan::{SiteKind, MAP_HI, MAP_LO};
use crate::network::{Feature, FeatureKind as K, FeatureShape};

/// Run the pipeline up to (not including) scenery and hand the context to `f`.
fn with_ctx(f: impl FnOnce(&mut ctx::Ctx)) {
    let cfg = MetroConfig::default();
    let plan = plan::Plan::new();
    let mut rng = wp_math::Rng::new(cfg.seed);
    let nat = ground::Natural::new(&plan, &mut rng);
    let mut works = Vec::new();
    works.extend(airport::earthworks(&plan, &nat));
    works.extend(docks::earthworks(&plan, &nat));
    works.extend(rail::earthworks(&plan, &nat));
    works.extend(parking::earthworks(&plan, &nat));
    works.extend(construction::earthworks(&plan, &nat));
    works.extend(buildings::earthworks(&plan, &nat));
    works.extend(scenery::earthworks(&plan, &nat));
    for s in &plan.sites {
        if let Some(z) = s.pad_z {
            works.push(ground::Earthwork::Pad {
                polygon: s.polygon.clone(),
                z,
                blend_m: 60.0,
            });
        }
    }
    let mut gnd = ground::Ground::new(&nat, cfg.terrain_cell_m, &works);
    let roads = roads::build(&plan, &mut gnd, &cfg);
    let terrain = gnd.hf;
    let mut ctx = ctx::Ctx::new(&plan, &terrain, &roads, cfg.seed);
    airport::generate(&mut ctx);
    docks::generate(&mut ctx);
    rail::generate(&mut ctx);
    parking::generate(&mut ctx);
    construction::generate(&mut ctx);
    buildings::generate(&mut ctx);
    f(&mut ctx);
}

fn is_closure(f: &Feature) -> bool {
    matches!(f.kind, K::JerseyBarrier | K::RoadFurniture | K::Tunnel)
}

fn edge_dist(p: V2) -> f32 {
    (p[0] - MAP_LO[0])
        .min(MAP_HI[0] - p[0])
        .min(p[1] - MAP_LO[1])
        .min(MAP_HI[1] - p[1])
}

fn centre2(f: &Feature) -> V2 {
    let (lo, hi) = f.shape.bounds_2d();
    [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5]
}

fn convex(poly: &[V2]) -> bool {
    let n = poly.len();
    (0..n).all(|i| {
        let (a, b, c) = (poly[i], poly[(i + 1) % n], poly[(i + 2) % n]);
        geom::cross(geom::sub(b, a), geom::sub(c, b)) >= -1e-4
    })
}

/// Footprints of `fs` matching `pick`, bucketed for distance queries.
struct Near {
    polys: Vec<Vec<V2>>,
    idx: Index,
}

impl Near {
    fn new(fs: &[Feature], pick: impl Fn(&Feature) -> bool) -> Self {
        let mut polys = Vec::new();
        let mut idx = Index::new(16.0);
        for f in fs.iter().filter(|f| pick(f)) {
            let fp = footprint(&f.shape);
            let (lo, hi) = bounds(&fp);
            idx.insert(polys.len() as u32, lo, hi);
            polys.push(fp);
        }
        Self { polys, idx }
    }

    fn within(&self, p: V2, d: f32) -> bool {
        let mut ids = Vec::new();
        self.idx
            .query([p[0] - d, p[1] - d], [p[0] + d, p[1] + d], &mut ids);
        ids.iter()
            .any(|&k| signed_dist(&self.polys[k as usize], p) <= d)
    }
}

#[test]
fn scenery_contract() {
    with_ctx(|ctx| {
        let plan = ctx.plan;
        // --- Pre-state samples -------------------------------------------------------
        let foreign = |p: V2| {
            plan.sites
                .iter()
                .any(|s| s.kind != SiteKind::Park && inside(&s.polygon, p))
        };
        let beach = plan.site("south_shore_park").unwrap().polygon.clone();
        let mut water_samples = Vec::new();
        let c = OCC_CELL_M;
        let nx = ((MAP_HI[0] - MAP_LO[0]) / c) as i64;
        let ny = ((MAP_HI[1] - MAP_LO[1]) / c) as i64;
        let mut all_banks = Vec::new();
        for j in 1..ny - 1 {
            for i in 1..nx - 1 {
                let w = [
                    MAP_LO[0] + (i as f32 + 0.5) * c,
                    MAP_LO[1] + (j as f32 + 0.5) * c,
                ];
                if ctx.occ_at(w) != Occ::Water {
                    continue;
                }
                for d in [[c, 0.0], [-c, 0.0], [0.0, c], [0.0, -c]] {
                    let l = [w[0] + d[0], w[1] + d[1]];
                    if (i + j) % 7 == 0
                        && !ctx.is_water(l)
                        && matches!(ctx.occ_at(l), Occ::Free | Occ::Verge | Occ::Site)
                        && !foreign(l)
                        && !inside(&beach, l)
                        && edge_dist(l) >= 4.0
                    {
                        all_banks.push(w);
                    }
                    if ctx.is_water(l)
                        || !matches!(ctx.occ_at(l), Occ::Free | Occ::Verge | Occ::Site)
                        || foreign(l)
                        || inside(&beach, l)
                        || edge_dist(l) < 4.0
                        || ctx.road_distance(l, 25.0) >= 25.0
                    {
                        continue;
                    }
                    water_samples.push(w);
                    break;
                }
            }
        }
        let mut edge_samples = Vec::new();
        let ins = EDGE_INSET_M + 0.3;
        let corners = [
            [MAP_LO[0] + ins, MAP_LO[1] + ins],
            [MAP_HI[0] - ins, MAP_LO[1] + ins],
            [MAP_HI[0] - ins, MAP_HI[1] - ins],
            [MAP_LO[0] + ins, MAP_HI[1] - ins],
        ];
        for e in 0..4 {
            let (a, b) = (corners[e], corners[(e + 1) % 4]);
            let n = (geom::dist(a, b) / 20.0) as usize;
            for k in 1..n {
                let p = super::contour::lerp(a, b, k as f32 / n as f32);
                let d = geom::normalize(geom::sub(b, a));
                let inward = [-d[1], d[0]];
                let q = geom::add(p, geom::scale(inward, 4.0));
                if ctx.is_water(p) || ctx.is_water(q) {
                    continue;
                }
                edge_samples.push((p, ctx.occ_at(p) == Occ::Road));
            }
        }

        // --- Determinism (build is pure on the context) -------------------------------
        let a = super::build(ctx);
        let b = super::build(ctx);
        assert!(a == b, "same context, same features");

        // --- Generate ----------------------------------------------------------------
        let n0 = ctx.features.len();
        let t0 = std::time::Instant::now();
        scenery::generate(ctx);
        let ms = t0.elapsed().as_secs_f64() * 1e3;
        let mine: Vec<Feature> = ctx.features[n0..].to_vec();
        assert_eq!(mine, a, "generate places exactly what build computed");
        let mut by_kind: BTreeMap<K, (usize, usize)> = BTreeMap::new();
        for f in &mine {
            let e = by_kind.entry(f.kind).or_default();
            e.0 += 1;
            if f.solid {
                e.1 += 1;
            }
        }
        let solid = mine.iter().filter(|f| f.solid).count();
        eprintln!(
            "scenery: {} features ({} solid) in {ms:.0} ms (test profile)",
            mine.len(),
            solid
        );
        for (k, (n, s)) in &by_kind {
            eprintln!("  {k:?}: {n} ({s} solid)");
        }
        assert!(mine.len() <= 70_000, "feature budget");
        assert!(solid <= 35_000, "solid budget");

        // --- Nothing on road paving except the map-edge closures ---------------------
        let mut closures = 0;
        for f in &mine {
            if off_road(ctx, f) {
                continue;
            }
            assert!(
                is_closure(f),
                "{:?} on road paving at {:?}",
                f.kind,
                centre2(f)
            );
            assert!(
                edge_dist(centre2(f)) < 30.0,
                "closure {:?} far from the map edge at {:?}",
                f.kind,
                centre2(f)
            );
            closures += 1;
        }
        eprintln!("  closure parts on road paving: {closures}");

        // --- Solid prisms convex, shapes sane ----------------------------------------
        for f in &mine {
            match &f.shape {
                FeatureShape::Prism {
                    footprint,
                    base_z,
                    top_z,
                } => {
                    assert!(top_z > base_z);
                    if f.solid {
                        assert!(convex(footprint), "solid prism convex");
                    }
                }
                FeatureShape::Box { half, centre, .. } => {
                    assert!(half.iter().all(|h| *h > 0.0 && h.is_finite()));
                    assert!(centre.iter().all(|v| v.is_finite()));
                }
                FeatureShape::Cylinder {
                    radius,
                    height,
                    base,
                } => {
                    assert!(*radius > 0.0 && *height > 0.0);
                    assert!(base.iter().all(|v| v.is_finite()));
                }
            }
        }

        // --- Water edges barred near roads -------------------------------------------
        let barriers = Near::new(&mine, |f| {
            f.solid
                && matches!(
                    f.kind,
                    K::QuayWall
                        | K::GuardRail
                        | K::Rock
                        | K::Wall
                        | K::Hedge
                        | K::Fence
                        | K::SoundWall
                        | K::MapEdge
                )
        });
        let hit = water_samples
            .iter()
            .filter(|w| barriers.within(**w, 3.0))
            .count();
        eprintln!(
            "  water cells next to road-side land: {} sampled, {} barred",
            water_samples.len(),
            hit
        );
        let hit_all = all_banks
            .iter()
            .filter(|w| barriers.within(**w, 3.0))
            .count();
        eprintln!(
            "  all banks: {} sampled, {} barred",
            all_banks.len(),
            hit_all
        );
        assert!(water_samples.len() > 100, "sampled the banks");
        assert!(
            hit as f32 >= 0.9 * water_samples.len() as f32,
            "water edges barred: {hit} / {}",
            water_samples.len()
        );

        // --- Map edge closed on land --------------------------------------------------
        let solid_near = Near::new(&mine, |f| f.solid);
        let closure_near = Near::new(&mine, |f| f.solid && is_closure(f));
        let mut ok = 0;
        for (p, road) in &edge_samples {
            let closed = if *road {
                closure_near.within(*p, 30.0)
            } else {
                solid_near.within(*p, 0.5)
            };
            if closed {
                ok += 1;
            } else {
                eprintln!("  open map edge at {p:?} (road {road})");
            }
        }
        eprintln!(
            "  map edge: {ok} / {} land samples closed",
            edge_samples.len()
        );
        assert!(edge_samples.len() > 400);
        assert!(
            ok as f32 >= 0.98 * edge_samples.len() as f32,
            "map edge closed"
        );

        // --- Parks have content, streets have furniture ------------------------------
        for s in plan.sites_of(SiteKind::Park) {
            let n = mine
                .iter()
                .filter(|f| inside(&s.polygon, centre2(f)))
                .count();
            eprintln!("  park {}: {n} features", s.id);
            assert!(n >= 20, "park {} has content", s.id);
        }
        for k in [K::Lamp, K::TreeTrunk, K::TreeCanopy, K::Hedge, K::MapEdge] {
            assert!(by_kind.get(&k).is_some_and(|c| c.0 > 0), "{k:?} placed");
        }
    });
}

/// Release timing: `cargo test --release -p wp_world --lib scenery_timing -- --ignored --nocapture`.
#[test]
#[ignore]
fn scenery_timing() {
    with_ctx(|ctx| {
        let t0 = std::time::Instant::now();
        let fs = super::build(ctx);
        eprintln!(
            "scenery build: {:.0} ms, {} features",
            t0.elapsed().as_secs_f64() * 1e3,
            fs.len()
        );
        let mut t = std::time::Instant::now();
        let mut lap = |name: &str| {
            eprintln!("  {name}: {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
            t = std::time::Instant::now();
        };
        let env = super::Env::new(ctx);
        lap("env");
        let mut out = Vec::new();
        super::bounds::water_edges(&env, &mut out);
        lap("water");
        super::bounds::cliffs(&env, &mut out);
        lap("cliffs");
        super::bounds::highways(&env, &mut out);
        super::bounds::map_edge(&env, &mut out);
        lap("highways + map edge");
        super::street::streetscape(&env, &mut out);
        lap("street");
        super::nature::parks(&env, &mut out);
        lap("parks");
        let fm = super::nature::forest_mask(&env);
        lap("forest mask");
        super::nature::forest(&env, &fm, &mut out);
        lap("forest");
        super::nature::scattered(&env, &fm, &mut out);
        lap("scattered");
        out.retain(|f| off_road(ctx, f));
        lap("road filter");
    });
}

/// A divided highway (raised on an embankment in the middle) through Midtown and Mercer on
/// flat ground, and a street leaving the map: rails, sound walls, fences, closure.
#[test]
fn synthetic_highway_and_closure() {
    use crate::builder::{self, BuildOptions, NodeSpec, RoadClass, RoadSpec};
    let plan = plan::Plan::new();
    let terrain = crate::terrain::Heightfield::flat(MAP_LO, MAP_HI, 24.0, 5.0);
    let node = |x: f32, y: f32, z: f32| NodeSpec {
        pos: [x, y],
        z,
        signal: false,
        control: None,
    };
    let nodes = vec![
        node(-1193.0, -300.0, 5.0),
        node(-1193.0, 1700.0, 5.0),
        node(-1207.0, 1700.0, 5.0),
        node(-1207.0, -300.0, 5.0),
        node(500.0, 3000.0, 5.0),
        node(500.0, 3399.0, 5.0),
    ];
    let prof = |y: f32| 5.0 + 6.0 * (1.0 - ((y - 300.0).abs() / 300.0)).clamp(0.0, 1.0);
    let mut specs = Vec::new();
    for (a, b, med) in [(0usize, 1usize, false), (2, 3, true)] {
        let mut r = RoadSpec::new(a, b, RoadClass::Highway, 3, 0);
        let (ya, yb) = (nodes[a].pos[1], nodes[b].pos[1]);
        let x = nodes[a].pos[0];
        r.via = (1..100)
            .map(|k| [x, ya + (yb - ya) * k as f32 / 100.0])
            .collect();
        r.via_z = r.via.iter().map(|p| prof(p[1])).collect();
        r.median_barrier = med;
        specs.push(r);
    }
    specs.push(RoadSpec::new(4, 5, RoadClass::Local, 1, 1));
    let built = builder::build(&nodes, &specs, &BuildOptions::default());
    let roads = roads::RoadsOut {
        nodes,
        specs,
        built,
        names: vec![String::new(); 3],
        features: Vec::new(),
        notes: Vec::new(),
        report: Default::default(),
    };
    let ctx = ctx::Ctx::new(&plan, &terrain, &roads, 1);
    let fs = super::build(&ctx);
    let count = |k: K, pred: &dyn Fn(V2) -> bool| {
        fs.iter()
            .filter(|f| f.kind == k && pred(centre2(f)))
            .count()
    };
    let raised = |p: V2| (100.0..500.0).contains(&p[1]) && (p[0] + 1200.0).abs() < 25.0;
    let midtown = |p: V2| (-200.0..0.0).contains(&p[1]) && (p[0] + 1200.0).abs() < 25.0;
    let mercer = |p: V2| (1000.0..1600.0).contains(&p[1]) && (p[0] + 1200.0).abs() < 25.0;
    let median = |p: V2| (p[0] + 1200.0).abs() < 3.0 && (-200.0..1600.0).contains(&p[1]);
    // Barrier length (box lengths) in a region: pieces are joined into runs, so count metres.
    let metres = |k: K, pred: &dyn Fn(V2) -> bool| {
        fs.iter()
            .filter(|f| f.kind == k && pred(centre2(f)))
            .map(|f| match f.shape {
                crate::network::FeatureShape::Box { half, .. } => 2.0 * half[1],
                _ => 0.0,
            })
            .sum::<f32>()
    };
    let (rails, fence, sound) = (
        metres(K::GuardRail, &raised),
        metres(K::Fence, &midtown),
        metres(K::SoundWall, &mercer),
    );
    eprintln!("rails {rails:.0} m, fence {fence:.0} m, sound walls {sound:.0} m");
    // Two outer edges (the median edge has none) along each stretch.
    assert!(rails >= 650.0, "rails on the embankment: {rails} m");
    assert!(fence >= 340.0, "fence by the offices: {fence} m");
    assert!(sound >= 1000.0, "sound walls by the houses: {sound} m");
    for k in [K::GuardRail, K::Fence, K::SoundWall] {
        assert_eq!(count(k, &median), 0, "{k:?} in the median");
    }
    assert!(
        count(K::Lamp, &|p| (p[0] + 1200.0).abs() < 25.0) >= 20,
        "highway lamps"
    );
    let closure = |p: V2| geom::dist(p, [500.0, MAP_HI[1] - 14.0]) < 20.0;
    assert!(
        count(K::JerseyBarrier, &closure) >= 3,
        "road closed at the map edge"
    );
    for f in &fs {
        assert!(
            off_road(&ctx, f) || (is_closure(f) && edge_dist(centre2(f)) < 30.0),
            "{:?} on the road at {:?}",
            f.kind,
            centre2(f)
        );
    }
}

#[test]
fn closures_cut_roads_leaving_the_map() {
    let line = vec![[-4000.0, 340.0], [-4450.0, 340.0]];
    let (c, d, k) = super::bounds::leave_point(&line).expect("leaves the map");
    assert_eq!(k, 0);
    assert!((c[0] - (MAP_LO[0] + super::bounds::CLOSURE_INSET_M)).abs() < 0.1);
    assert!(d[0] < -0.99);
    assert!(super::bounds::leave_point(&[[0.0, 0.0], [10.0, 0.0]]).is_none());
    with_ctx(|ctx| {
        let env = super::Env::new(ctx);
        let mut out = Vec::new();
        super::bounds::closure(&env, c, 6.0, d, 7.0, 7.0, true, &mut out);
        let jersey: Vec<&Feature> = out.iter().filter(|f| f.kind == K::JerseyBarrier).collect();
        let span: f32 = jersey
            .iter()
            .map(|f| match f.shape {
                FeatureShape::Box { half, .. } => half[1] * 2.0,
                _ => 0.0,
            })
            .sum();
        assert!(span >= 14.0, "barriers span the road: {span}");
        assert!(out.iter().all(|f| edge_dist(centre2(f)) < 30.0));
        assert!(out
            .iter()
            .any(|f| f.kind == K::Tunnel && f.material == crate::network::Material::Black));
    });
}

/// Debug dump for eyeballing: `SCENERY_DUMP=<dir> cargo test -p wp_world --lib scenery_dump
/// -- --ignored` writes `features.csv` (kind, solid, footprint) and `occ.bin` (4 m grid of
/// occupancy class and ground height + 10 m, one byte each).
#[test]
#[ignore]
fn scenery_dump() {
    let Ok(dir) = std::env::var("SCENERY_DUMP") else {
        return;
    };
    with_ctx(|ctx| {
        let n0 = ctx.features.len();
        let mut occ = Vec::new();
        let (w, h) = (2000usize, 1750usize);
        for j in 0..h {
            for i in 0..w {
                let p = [
                    MAP_LO[0] + i as f32 * 4.0 + 2.0,
                    MAP_LO[1] + j as f32 * 4.0 + 2.0,
                ];
                occ.push(ctx.occ_at(p) as u8);
                occ.push((ctx.ground_z(p).clamp(-10.0, 150.0) + 10.0) as u8);
            }
        }
        std::fs::write(format!("{dir}/occ.bin"), occ).unwrap();
        scenery::generate(ctx);
        let mut s = String::new();
        let all = ctx.features[n0..]
            .iter()
            .map(|f| (true, f))
            .chain(ctx.roads.features.iter().map(|f| (false, f)));
        for (mine, f) in all {
            let pts: Vec<String> = footprint(&f.shape)
                .iter()
                .map(|p| format!("{:.1} {:.1}", p[0], p[1]))
                .collect();
            s.push_str(&format!(
                "{:?};{};{};{}\n",
                f.kind,
                mine,
                f.solid,
                pts.join(";")
            ));
        }
        std::fs::write(format!("{dir}/features.csv"), s).unwrap();
    });
}
