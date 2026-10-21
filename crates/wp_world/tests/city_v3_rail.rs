//! Port Vesper v3 rail module: alignments (grade, radii, corridors never over water),
//! track continuity, bridges over the river, no solids on road paving, fence gaps at level
//! crossings, site content, convex prisms, budgets and determinism.
//!
//! `cargo test -p wp_world --test city_v3_rail -- --nocapture` prints the feature counts.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use wp_math::Rng;
use wp_world::geom::{self, V2};
use wp_world::metro::ctx::{footprint, Ctx};
use wp_world::metro::ground::{self, Earthwork, Ground, Natural};
use wp_world::metro::plan::{self, Plan, SiteKind};
use wp_world::metro::{
    airport, buildings, construction, docks, parking, rail, roads, scenery, MetroConfig,
};
use wp_world::network::{Feature, FeatureKind, FeatureShape, RoadSurface};

struct Run {
    plan: Plan,
    nat: Natural,
    lines: Vec<rail::RailAlignment>,
    surfaces: Vec<RoadSurface>,
    features: Vec<Feature>,
    again: Vec<Feature>,
    secs: f32,
}

fn run() -> &'static Run {
    static RUN: OnceLock<Run> = OnceLock::new();
    RUN.get_or_init(|| {
        let cfg = MetroConfig::default();
        let plan = Plan::new();
        let mut rng = Rng::new(cfg.seed);
        let nat = Natural::new(&plan, &mut rng);
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
                works.push(Earthwork::Pad {
                    polygon: s.polygon.clone(),
                    z,
                    blend_m: 60.0,
                });
            }
        }
        let mut gnd = Ground::new(&nat, cfg.terrain_cell_m, &works);
        let roads = roads::build(&plan, &mut gnd, &cfg);
        let terrain = gnd.hf;
        let gen = || {
            let mut ctx = Ctx::new(&plan, &terrain, &roads, cfg.seed);
            airport::generate(&mut ctx);
            docks::generate(&mut ctx);
            let before = ctx.features.len();
            let t = std::time::Instant::now();
            rail::generate(&mut ctx);
            let secs = t.elapsed().as_secs_f32();
            (ctx.features[before..].to_vec(), secs)
        };
        let (features, secs) = gen();
        let (again, _) = gen();
        let lines = rail::rail_lines(&plan, &nat);
        Run {
            surfaces: roads.built.surfaces.clone(),
            plan,
            nat,
            lines,
            features,
            again,
            secs,
        }
    })
}

/// Footprint, bottom and top of a feature.
fn extent(f: &Feature) -> (Vec<V2>, f32, f32) {
    let fp = footprint(&f.shape);
    match &f.shape {
        FeatureShape::Box {
            centre,
            half,
            pitch,
            ..
        } => {
            let (s, c) = wp_math::sin_cos(*pitch);
            let dz = half[1] * s.abs() + half[2] * c.abs();
            (fp, centre[2] - dz, centre[2] + dz)
        }
        FeatureShape::Cylinder { base, height, .. } => (fp, base[2], base[2] + height),
        FeatureShape::Prism { base_z, top_z, .. } => (fp, *base_z, *top_z),
    }
}

fn convex_overlap(a: &[V2], b: &[V2], shrink: f32) -> bool {
    for poly in [a, b] {
        let n = poly.len();
        for i in 0..n {
            let e = geom::sub(poly[(i + 1) % n], poly[i]);
            let ax = geom::normalize([-e[1], e[0]]);
            let pr = |p: &[V2]| {
                p.iter().fold((f32::MAX, f32::MIN), |(lo, hi), q| {
                    (lo.min(geom::dot(*q, ax)), hi.max(geom::dot(*q, ax)))
                })
            };
            let (a0, a1) = pr(a);
            let (b0, b1) = pr(b);
            if a1 - shrink < b0 || b1 - shrink < a0 {
                return false;
            }
        }
    }
    true
}

fn is_convex(poly: &[V2]) -> bool {
    let n = poly.len();
    let mut sign = 0.0f32;
    for i in 0..n {
        let (a, b, c) = (poly[i], poly[(i + 1) % n], poly[(i + 2) % n]);
        let cr = geom::cross(geom::sub(b, a), geom::sub(c, b));
        if cr.abs() < 1e-4 {
            continue;
        }
        if sign == 0.0 {
            sign = cr.signum();
        } else if cr.signum() != sign {
            return false;
        }
    }
    true
}

/// Paved quads with their height range.
fn paving(r: &Run) -> Vec<(Vec<V2>, f32, f32)> {
    let mut out = Vec::new();
    for s in &r.surfaces {
        let (l, rr) = wp_world::surface::surface_edges(s);
        for k in 0..l.len().saturating_sub(1) {
            let pts = [l[k], l[k + 1], rr[k + 1], rr[k]];
            let lo = pts.iter().map(|p| p[2]).fold(f32::MAX, f32::min);
            let hi = pts.iter().map(|p| p[2]).fold(f32::MIN, f32::max);
            out.push((pts.iter().map(|p| geom::xy(*p)).collect(), lo, hi));
        }
    }
    out
}

#[test]
fn counts_budget_and_timing() {
    let r = run();
    let mut by_kind: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for f in &r.features {
        let e = by_kind.entry(format!("{:?}", f.kind)).or_default();
        e.0 += 1;
        if f.solid {
            e.1 += 1;
        }
    }
    for (k, (n, s)) in &by_kind {
        println!("{k:>14}: {n:>6} ({s} solid)");
    }
    let solid = r.features.iter().filter(|f| f.solid).count();
    println!(
        "rail: {} features, {solid} solid, generate {:.3} s",
        r.features.len(),
        r.secs
    );
    for l in &r.lines {
        println!(
            "{}: {:.0} m, grade {:.4}, min radius {:.0} m, spans {:?}",
            l.name,
            l.length(),
            l.max_grade(),
            l.min_radius_m(),
            l.spans
                .iter()
                .map(|s| (s.name.clone(), s.s0 as i32, s.s1 as i32))
                .collect::<Vec<_>>()
        );
    }
    assert!(r.features.len() <= 15_000, "{} features", r.features.len());
    assert!(solid <= 6_000, "{solid} solid");
    for kind in [
        FeatureKind::RailTrack,
        FeatureKind::RailBridge,
        FeatureKind::RailCar,
        FeatureKind::RailFurniture,
        FeatureKind::Platform,
        FeatureKind::Fence,
    ] {
        assert!(r.features.iter().any(|f| f.kind == kind), "no {kind:?}");
    }
}

#[test]
fn profiles_are_gentle_and_curves_wide() {
    let r = run();
    for l in &r.lines {
        assert!(
            l.max_grade() <= rail::align::MAX_GRADE,
            "{}: grade {}",
            l.name,
            l.max_grade()
        );
        let need = if l.name == "Westline Freight Line" {
            300.0
        } else {
            150.0
        };
        assert!(
            l.radius_m >= need,
            "{}: design radius {}",
            l.name,
            l.radius_m
        );
        assert!(
            l.min_radius_m() >= l.radius_m * 0.95,
            "{}: radius {} < {}",
            l.name,
            l.min_radius_m(),
            l.radius_m
        );
        // On a padded site (and its boundary) the bed sits at the pad level.
        for (i, p) in l.pts.iter().enumerate() {
            for s in r.plan.sites.iter().filter(|s| s.pad_z.is_some()) {
                if ground::inside(&s.polygon, geom::xy(*p)) && l.support[i].on_ground() {
                    assert!(
                        (p[2] - s.pad_z.unwrap()).abs() < 0.01,
                        "{} off the {} pad",
                        l.name,
                        s.id
                    );
                }
            }
        }
        // The formation never dips into the water.
        assert!(
            l.pts.iter().all(|p| p[2] > plan::WATER_Z + 1.0),
            "{}",
            l.name
        );
    }
}

#[test]
fn corridors_never_over_water() {
    let r = run();
    let works = rail::earthworks(&r.plan, &r.nat);
    assert!(!works.is_empty());
    for w in &works {
        let line = match w {
            Earthwork::Corridor { line, .. } => line,
            Earthwork::Pad { polygon, .. } => {
                // Only under the elevated station, which is dry land.
                let st = &r.plan.site("vesper_central").unwrap().polygon;
                assert_eq!(polygon, st);
                assert!(polygon.iter().all(|p| r.nat.z(*p) > plan::WATER_Z + 1.0));
                continue;
            }
        };
        for p in line {
            assert!(
                r.nat.z(geom::xy(*p)) > plan::WATER_Z + 0.5,
                "corridor over water at {p:?}"
            );
        }
    }
    // Applying only the rail corridors keeps every water vertex water.
    let g0 = Ground::new(&r.nat, 6.0, &[]);
    let g1 = Ground::new(&r.nat, 6.0, &works);
    let mut wet = 0;
    for (a, b) in g0.hf.z.iter().zip(&g1.hf.z) {
        if *a < plan::WATER_Z {
            wet += 1;
            assert!(*b < plan::WATER_Z, "rail earthworks filled water");
        }
    }
    assert!(wet > 1000);
}

#[test]
fn bridges_span_the_river() {
    let r = run();
    let decks: Vec<(Vec<V2>, f32, f32)> = r
        .features
        .iter()
        .filter(|f| f.kind == FeatureKind::RailBridge && f.solid)
        .map(extent)
        .collect();
    let mut wet = 0;
    for l in &r.lines {
        for (i, p) in l.pts.iter().enumerate() {
            if l.natural[i] >= plan::WATER_Z {
                continue;
            }
            wet += 1;
            assert_eq!(
                l.support[i],
                rail::Support::Bridge,
                "{} over water off a bridge",
                l.name
            );
            let q = geom::xy(*p);
            assert!(
                decks
                    .iter()
                    .any(|(fp, _, top)| ground::inside(fp, q) && *top >= p[2] - 0.05),
                "{}: no deck at {q:?}",
                l.name
            );
        }
    }
    assert!(wet > 10, "the freight line crosses the river");
    let swing = r.lines[0]
        .spans
        .iter()
        .find(|s| s.name == "Westline Swing Bridge")
        .expect("swing bridge");
    assert!(swing.water.is_some());
    assert!(r
        .lines
        .iter()
        .any(|l| l.spans.iter().any(|s| s.name == "Central Rail Bridge")));
}

#[test]
fn track_is_continuous() {
    let r = run();
    // Ballast beds and crossing planks, bucketed by 20 m cells.
    let mut cells: BTreeMap<(i32, i32), Vec<Vec<V2>>> = BTreeMap::new();
    for f in r
        .features
        .iter()
        .filter(|f| f.kind == FeatureKind::RailTrack)
    {
        if let FeatureShape::Box { half, .. } = &f.shape {
            if half[0] < 0.5 {
                continue; // a rail
            }
        }
        let fp = footprint(&f.shape);
        let (lo, hi) = ground::bounds(&fp);
        for j in (lo[1] / 20.0).floor() as i32..=(hi[1] / 20.0).floor() as i32 {
            for i in (lo[0] / 20.0).floor() as i32..=(hi[0] / 20.0).floor() as i32 {
                cells.entry((i, j)).or_default().push(fp.clone());
            }
        }
    }
    let covered = |p: V2| {
        let k = ((p[0] / 20.0).floor() as i32, (p[1] / 20.0).floor() as i32);
        cells
            .get(&k)
            .is_some_and(|v| v.iter().any(|fp| ground::inside(fp, p)))
    };
    for l in &r.lines {
        let end = (0..l.pts.len())
            .find(|&i| l.support[i] == rail::Support::StationDeck)
            .map(|i| l.cum[i])
            .unwrap_or(l.length());
        let normal = |s: f32| geom::right_of(l.at(s).1);
        for off in l.track_offsets() {
            let mut gap = 0.0f32;
            let mut worst = 0.0f32;
            let mut s = 0.5;
            while s < end - 0.5 {
                let p = geom::xy(l.at(s).0);
                let q = [p[0] + normal(s)[0] * off, p[1] + normal(s)[1] * off];
                if covered(q) {
                    gap = 0.0;
                } else {
                    gap += 0.25;
                    worst = worst.max(gap);
                }
                s += 0.25;
            }
            assert!(worst <= 0.5, "{} track {off}: gap {worst} m", l.name);
        }
    }
}

#[test]
fn no_solid_on_road_paving() {
    let r = run();
    let pav = paving(r);
    let mut bad = Vec::new();
    for f in r.features.iter().filter(|f| f.solid) {
        let (fp, z0, z1) = extent(f);
        let (lo, hi) = ground::bounds(&fp);
        for (q, qlo, qhi) in &pav {
            let (a, b) = ground::bounds(q);
            if a[0] > hi[0] || b[0] < lo[0] || a[1] > hi[1] || b[1] < lo[1] {
                continue;
            }
            if z1 < qlo - 0.3 || z0 > qhi + 4.0 {
                continue;
            }
            if convex_overlap(&fp, q, 0.05) {
                bad.push((f.kind, fp[0]));
                break;
            }
        }
    }
    assert!(
        bad.is_empty(),
        "{} solids on paving, e.g. {:?}",
        bad.len(),
        &bad[..bad.len().min(5)]
    );
}

#[test]
fn level_crossings_have_gates_and_fence_gaps() {
    let r = run();
    let pav = paving(r);
    let mut crossings = 0;
    for l in &r.lines {
        for w in l.pts.windows(2) {
            for s in &r.surfaces {
                for c in s.centre.windows(2) {
                    let (a0, a1) = (geom::xy(w[0]), geom::xy(w[1]));
                    let (b0, b1) = (geom::xy(c[0]), geom::xy(c[1]));
                    if !geom::segments_intersect(a0, a1, b0, b1) {
                        continue;
                    }
                    if (c[0][2] - w[0][2]).abs() > 4.0 {
                        continue;
                    }
                    crossings += 1;
                    // Gate posts (solid furniture) within 25 m, none on the paving.
                    let gates = r
                        .features
                        .iter()
                        .filter(|f| f.kind == FeatureKind::RailFurniture && f.solid)
                        .filter(|f| geom::dist(footprint(&f.shape)[0], a0) < 25.0)
                        .count();
                    assert!(gates >= 2, "{}: crossing at {a0:?} without gates", l.name);
                }
            }
        }
    }
    assert!(crossings >= 3, "only {crossings} level crossings");
    // Fences never touch paving (the gaps are where the roads cross).
    for f in r.features.iter().filter(|f| f.kind == FeatureKind::Fence) {
        let (fp, _, _) = extent(f);
        for (q, _, _) in &pav {
            assert!(
                !convex_overlap(&fp, q, 0.05),
                "fence on paving at {:?}",
                fp[0]
            );
        }
    }
}

#[test]
fn sites_have_their_content() {
    let r = run();
    let count_in = |site: &str, kind: FeatureKind| {
        let poly = &r.plan.site(site).unwrap().polygon;
        r.features
            .iter()
            .filter(|f| f.kind == kind && ground::inside(poly, footprint(&f.shape)[0]))
            .count()
    };
    let cars = count_in("westline_yard", FeatureKind::RailCar);
    assert!(cars > 300, "yard cars {cars}");
    assert!(count_in("roundhouse", FeatureKind::Landmark) > 20);
    assert!(count_in("roundhouse", FeatureKind::RailCar) > 0);
    assert!(count_in("vesper_central", FeatureKind::Platform) >= 6);
    assert!(count_in("vesper_central", FeatureKind::RailCar) > 20);
    // Every rail site is fenced.
    for s in r
        .plan
        .sites
        .iter()
        .filter(|s| matches!(s.kind, SiteKind::RailYard | SiteKind::Station))
    {
        let fences = r
            .features
            .iter()
            .filter(|f| f.kind == FeatureKind::Fence)
            .filter(|f| ground::signed_dist(&s.polygon, footprint(&f.shape)[0]).abs() < 30.0)
            .count();
        assert!(fences > 20, "{}: {fences} fence pieces", s.id);
    }
    // The roundhouse can be driven into from its gate: nothing solid at car height on the
    // way from the gate to the turntable.
    let rh = r.plan.site("roundhouse").unwrap();
    let gate = rh.gates[0].pos;
    let c = [-2300.0f32, 380.0];
    let solids: Vec<(Vec<V2>, f32, f32)> =
        r.features.iter().filter(|f| f.solid).map(extent).collect();
    for k in 0..=40 {
        let p = wp_world::metro::ctx::lerp2(gate, c, k as f32 / 40.0 * 0.85);
        let car = wp_world::metro::ctx::obb(p, [1.0, 2.2], 0.0);
        let z = rh.pad_z.unwrap();
        assert!(
            !solids.iter().any(|(fp, z0, z1)| *z0 < z + 1.8
                && *z1 > z + 0.3
                && convex_overlap(fp, &car, 0.0)),
            "roundhouse blocked at {p:?}"
        );
    }
}

#[test]
fn solid_prisms_are_convex() {
    let r = run();
    for f in r.features.iter().filter(|f| f.solid) {
        if let FeatureShape::Prism { footprint, .. } = &f.shape {
            assert!(is_convex(footprint), "concave prism {footprint:?}");
        }
    }
}

#[test]
fn deterministic() {
    let r = run();
    assert_eq!(r.features, r.again);
    let a = rail::rail_lines(&r.plan, &r.nat);
    let b = rail::rail_lines(&r.plan, &r.nat);
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(x.pts, y.pts);
    }
}

/// One line per feature (kind, material, solid, centre, z range), for inspection.
#[test]
#[ignore]
fn debug_dump() {
    let r = run();
    for l in &r.lines {
        for i in (0..l.pts.len()).step_by(10) {
            println!(
                "L {} {:.0} {:.0} {:.0} z {:.1} nat {:.1} {:?}",
                l.name, l.cum[i], l.pts[i][0], l.pts[i][1], l.pts[i][2], l.natural[i], l.support[i]
            );
        }
    }
    for f in &r.features {
        let fp = footprint(&f.shape);
        let c = fp.iter().fold([0.0, 0.0], |a, p| geom::add(a, *p));
        let c = [c[0] / fp.len() as f32, c[1] / fp.len() as f32];
        let (_, z0, z1) = extent(f);
        println!(
            "F {:?} {:?} {} {:.0} {:.0} {:.1} {:.1}",
            f.kind, f.material, f.solid, c[0], c[1], z0, z1
        );
    }
}
