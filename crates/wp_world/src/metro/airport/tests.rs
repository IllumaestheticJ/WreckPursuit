//! Tests of the airport module on the real v3 pipeline (plan, ground, roads, context).

// Test-only module (declared under `#[cfg(test)]`); the attribute below also tells
// lint-determinism to stop scanning here (tests may time themselves).
#[cfg(test)]
use std::collections::BTreeMap;
use std::sync::OnceLock;

use wp_math::Rng;

use super::super::ctx::{footprint, Ctx, Occ};
use super::super::ground::{self, inside, signed_dist, Ground, Natural};
use super::super::plan::{Plan, SiteKind};
use super::super::roads::{self, RoadsOut};
use super::super::MetroConfig;
use super::footprint_samples;
use crate::geom::{self, V2};
use crate::network::{Feature, FeatureKind, FeatureShape};
use crate::terrain::Heightfield;

/// Plan, final terrain and roads, as `metro::generate_with_report` builds them.
fn base() -> &'static (Plan, Heightfield, RoadsOut) {
    static BASE: OnceLock<(Plan, Heightfield, RoadsOut)> = OnceLock::new();
    BASE.get_or_init(|| {
        let cfg = MetroConfig::default();
        let plan = Plan::new();
        let mut rng = Rng::new(cfg.seed);
        let nat = Natural::new(&plan, &mut rng);
        let mut works = Vec::new();
        works.extend(super::earthworks(&plan, &nat));
        works.extend(super::super::docks::earthworks(&plan, &nat));
        works.extend(super::super::rail::earthworks(&plan, &nat));
        works.extend(super::super::parking::earthworks(&plan, &nat));
        works.extend(super::super::construction::earthworks(&plan, &nat));
        works.extend(super::super::buildings::earthworks(&plan, &nat));
        works.extend(super::super::scenery::earthworks(&plan, &nat));
        for s in &plan.sites {
            if let Some(z) = s.pad_z {
                works.push(ground::Earthwork::Pad {
                    polygon: s.polygon.clone(),
                    z,
                    blend_m: 60.0,
                });
            }
        }
        let mut gnd = Ground::new(&nat, cfg.terrain_cell_m, &works);
        let roads = roads::build(&plan, &mut gnd, &cfg);
        (plan, gnd.hf, roads)
    })
}

/// A fresh context with only the airport module run on it.
fn run() -> Ctx<'static> {
    let (plan, terrain, roads) = base();
    let mut ctx = Ctx::new(plan, terrain, roads, MetroConfig::default().seed);
    super::generate(&mut ctx);
    ctx
}

fn own_sites(plan: &Plan) -> Vec<Vec<V2>> {
    plan.sites
        .iter()
        .filter(|s| matches!(s.kind, SiteKind::Airport | SiteKind::Airfield))
        .map(|s| s.polygon.clone())
        .collect()
}

#[test]
fn airport_content_is_where_it_belongs() {
    let ctx = run();
    let plan = ctx.plan;
    let feats = &ctx.features;

    // Counts by kind and budget.
    let mut counts: BTreeMap<FeatureKind, (usize, usize)> = BTreeMap::new();
    for f in feats {
        let e = counts.entry(f.kind).or_default();
        e.0 += 1;
        if f.solid {
            e.1 += 1;
        }
    }
    for (k, (n, s)) in &counts {
        eprintln!("{k:?}: {n} ({s} solid)");
    }
    let solid = feats.iter().filter(|f| f.solid).count();
    eprintln!(
        "airport: {} features, {} solid, {} buildings",
        feats.len(),
        solid,
        ctx.buildings.len()
    );
    assert!(feats.len() <= 6000, "feature budget: {}", feats.len());
    assert!(solid <= 3000, "solid budget: {solid}");

    // Runways: solid, flat, inside a site, coplanar per site, on the ground.
    let sites = own_sites(plan);
    let mut tops: BTreeMap<usize, (f32, f32)> = BTreeMap::new();
    let mut runway_count = 0;
    for f in feats.iter().filter(|f| f.kind == FeatureKind::Runway) {
        runway_count += 1;
        assert!(f.solid, "runway solid");
        let FeatureShape::Box {
            centre,
            half,
            pitch,
            ..
        } = f.shape
        else {
            panic!("runway is a box");
        };
        assert!(pitch.abs() < 0.01, "runway flat");
        let fp = footprint(&f.shape);
        let si = sites
            .iter()
            .position(|s| fp.iter().all(|p| inside(s, *p)))
            .expect("runway inside a site");
        let top = centre[2] + half[2];
        let e = tops.entry(si).or_insert((top, top));
        e.0 = e.0.min(top);
        e.1 = e.1.max(top);
        for p in &fp {
            let g = ctx.ground_z(*p);
            assert!((top - g).abs() < 0.3, "runway top {top} vs ground {g}");
        }
    }
    assert!(runway_count >= 3, "three runways' worth of slabs");
    for (lo, hi) in tops.values() {
        assert!(hi - lo < 0.01, "runway decks coplanar");
    }

    // Nothing solid on road paving; nothing solid but fences on verges.
    for f in feats.iter().filter(|f| f.solid) {
        for p in footprint_samples(f) {
            let o = ctx.occ_at(p);
            assert!(o != Occ::Road, "{:?} on road at {p:?}", f.kind);
            if f.kind != FeatureKind::Fence {
                assert!(o != Occ::Verge, "{:?} on verge at {p:?}", f.kind);
            }
        }
    }

    // Everything inside the airport / airfield sites, nothing in the parking sites.
    let others: Vec<&Vec<V2>> = plan
        .sites
        .iter()
        .filter(|s| !matches!(s.kind, SiteKind::Airport | SiteKind::Airfield))
        .map(|s| &s.polygon)
        .collect();
    for f in feats {
        for p in footprint(&f.shape) {
            let d = sites
                .iter()
                .map(|s| signed_dist(s, p))
                .fold(f32::MAX, f32::min);
            assert!(d < 0.5, "{:?} outside the sites at {p:?}", f.kind);
            assert!(
                others.iter().all(|o| signed_dist(o, p) > -0.5),
                "{:?} inside another site at {p:?}",
                f.kind
            );
        }
    }

    // Every aircraft lies inside its site.
    let mut aircraft = 0;
    for f in feats.iter().filter(|f| f.kind == FeatureKind::Aircraft) {
        aircraft += 1;
        let fp = footprint(&f.shape);
        assert!(
            sites.iter().any(|s| fp.iter().all(|p| inside(s, *p))),
            "aircraft part inside a site"
        );
    }
    assert!(aircraft > 100, "aircraft parts: {aircraft}");

    // Solid prisms are convex (CCW, no reflex corner).
    for f in feats.iter().filter(|f| f.solid) {
        if let FeatureShape::Prism { footprint, .. } = &f.shape {
            let n = footprint.len();
            for i in 0..n {
                let (a, b, c) = (footprint[i], footprint[(i + 1) % n], footprint[(i + 2) % n]);
                let cr = geom::cross(geom::sub(b, a), geom::sub(c, b));
                assert!(cr >= -1e-3, "convex prism");
            }
        }
    }

    // Each gate is open: a 6 m wide lane from the gate 80 m into the site meets no fence,
    // and the gate has a barrier arm.
    let fences: Vec<Vec<V2>> = feats
        .iter()
        .filter(|f| f.kind == FeatureKind::Fence)
        .map(|f| footprint(&f.shape))
        .collect();
    assert!(fences.len() > 300, "fence pieces: {}", fences.len());
    for s in plan
        .sites
        .iter()
        .filter(|s| s.id == "airport" || s.id == "airfield")
    {
        for g in &s.gates {
            let d = geom::dir_of_yaw(g.heading);
            let r = geom::right_of(d);
            let mut t = -5.0;
            while t <= 80.0 {
                for l in [-3.0, 0.0, 3.0] {
                    let p = geom::add(geom::add(g.pos, geom::scale(d, t)), geom::scale(r, l));
                    assert!(
                        !fences.iter().any(|fp| inside(fp, p)),
                        "fence blocks gate {:?} of {} at {p:?}",
                        g.pos,
                        s.id
                    );
                }
                t += 0.5;
            }
            let arm = feats.iter().any(|f| {
                f.kind == FeatureKind::Gate
                    && footprint(&f.shape)
                        .iter()
                        .any(|p| geom::dist(*p, g.pos) < 80.0)
            });
            assert!(arm, "barrier arm at gate {:?}", g.pos);
        }
    }

    // The fence closes the airside: walking the fence rectangle, every open stretch is a
    // gate, a road crossing or a closing building.
    // (Checked indirectly: total fence length covers most of the perimeter.)
    let len: f32 = feats
        .iter()
        .filter(|f| f.kind == FeatureKind::Fence)
        .map(|f| match f.shape {
            FeatureShape::Box { half, .. } => half[1] * 2.0,
            _ => 0.0,
        })
        .sum();
    let perimeter: f32 = plan
        .sites
        .iter()
        .filter(|s| s.id == "airport" || s.id == "airfield")
        .map(|s| {
            let n = s.polygon.len();
            (0..n)
                .map(|i| geom::dist(s.polygon[i], s.polygon[(i + 1) % n]))
                .sum::<f32>()
        })
        .sum();
    assert!(len > 0.85 * perimeter, "fence {len} m of {perimeter} m");

    // Hangar 3 (safehouse) stands at the plan's POI.
    let sh = plan
        .pois
        .iter()
        .find(|p| p.name.starts_with("SH13"))
        .expect("SH13 in plan")
        .pos;
    assert!(feats.iter().any(|f| f.kind == FeatureKind::Hangar
        && footprint(&f.shape)
            .iter()
            .any(|p| geom::dist(*p, sh) < 40.0)));

    // A terminal and a control tower exist.
    assert!(feats
        .iter()
        .any(|f| f.kind == FeatureKind::Terminal && f.solid));
    assert!(feats.iter().any(|f| f.kind == FeatureKind::ControlTower));
}

#[test]
fn airport_is_deterministic() {
    let a = run();
    let b = run();
    assert_eq!(a.features.len(), b.features.len());
    assert!(a.features == b.features, "same seed, same features");
    assert!(a.buildings == b.buildings, "same seed, same buildings");
    let _: &Vec<Feature> = &a.features;
}

#[test]
fn airport_generation_time() {
    let (plan, terrain, roads) = base();
    let mut ctx = Ctx::new(plan, terrain, roads, 1);
    let t = std::time::Instant::now();
    super::generate(&mut ctx);
    eprintln!(
        "airport::generate: {:.1} ms",
        t.elapsed().as_secs_f64() * 1e3
    );
    for id in ["airport", "airfield"] {
        let s = plan.site(id).expect("site");
        eprintln!("{id}: fence rect {:?}", super::fence_rect(&ctx, s));
    }
}
