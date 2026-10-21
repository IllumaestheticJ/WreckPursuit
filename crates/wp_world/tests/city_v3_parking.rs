//! Port Vesper v3: parking structures, surface lots, the drive-in and the construction
//! sites, checked from the generated features (geometry, drivability, enclosure, budgets,
//! determinism).

use std::collections::BTreeMap;

use wp_world::geom::{self, Obb, V2, V3};
use wp_world::metro::ctx::{footprint, Ctx, Occ};
use wp_world::metro::plan::{Plan, Site, SiteKind};
use wp_world::metro::{self, ground};
use wp_world::network::{Feature, FeatureKind as K, FeatureShape};

/// Run the v3 pipeline up to (and including) construction; hand the context and the
/// parking / construction features to `f`.
fn with_city<R>(seed: u64, f: impl FnOnce(&Ctx, &[Feature], &[Feature]) -> R) -> R {
    let cfg = metro::MetroConfig {
        seed,
        ..Default::default()
    };
    let plan = Plan::new();
    let mut rng = wp_math::Rng::new(cfg.seed);
    let nat = ground::Natural::new(&plan, &mut rng);
    let mut works = Vec::new();
    works.extend(metro::airport::earthworks(&plan, &nat));
    works.extend(metro::docks::earthworks(&plan, &nat));
    works.extend(metro::rail::earthworks(&plan, &nat));
    works.extend(metro::parking::earthworks(&plan, &nat));
    works.extend(metro::construction::earthworks(&plan, &nat));
    works.extend(metro::buildings::earthworks(&plan, &nat));
    works.extend(metro::scenery::earthworks(&plan, &nat));
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
    let roads = metro::roads::build(&plan, &mut gnd, &cfg);
    let terrain = gnd.hf;
    let mut ctx = Ctx::new(&plan, &terrain, &roads, cfg.seed);
    metro::airport::generate(&mut ctx);
    metro::docks::generate(&mut ctx);
    metro::rail::generate(&mut ctx);
    let a = ctx.features.len();
    let t0 = std::time::Instant::now();
    metro::parking::generate(&mut ctx);
    let t1 = std::time::Instant::now();
    let b = ctx.features.len();
    metro::construction::generate(&mut ctx);
    let t2 = std::time::Instant::now();
    eprintln!(
        "timing: parking {:.1} ms, construction {:.1} ms",
        (t1 - t0).as_secs_f64() * 1e3,
        (t2 - t1).as_secs_f64() * 1e3
    );
    let parking = ctx.features[a..b].to_vec();
    let construction = ctx.features[b..].to_vec();
    f(&ctx, &parking, &construction)
}

/// Vertical extent of a feature on the vertical line through `p`, if it crosses it.
fn vspan(f: &Feature, p: V2) -> Option<(f32, f32)> {
    match &f.shape {
        FeatureShape::Box {
            centre,
            half,
            yaw,
            pitch,
        } => {
            let (sy, cy) = (yaw.sin(), yaw.cos());
            let (sp, cp) = (pitch.sin(), pitch.cos());
            let (dx, dy) = (p[0] - centre[0], p[1] - centre[1]);
            let ux = dx * cy + dy * sy;
            let uy = -dx * sy + dy * cy;
            if ux.abs() > half[0] {
                return None;
            }
            // Local y = uy cp + w sp, local z = -uy sp + w cp, w = z - centre z.
            let mut lo = f32::MIN;
            let mut hi = f32::MAX;
            let mut clip = |a: f32, b: f32, h: f32| -> bool {
                // |a + b w| <= h
                if b.abs() < 1e-6 {
                    return a.abs() <= h;
                }
                let (w0, w1) = ((-h - a) / b, (h - a) / b);
                lo = lo.max(w0.min(w1));
                hi = hi.min(w0.max(w1));
                true
            };
            if !clip(uy * cp, sp, half[1]) || !clip(-uy * sp, cp, half[2]) || lo > hi {
                return None;
            }
            Some((centre[2] + lo, centre[2] + hi))
        }
        FeatureShape::Cylinder {
            base,
            radius,
            height,
        } => (geom::dist(p, [base[0], base[1]]) <= *radius).then_some((base[2], base[2] + height)),
        FeatureShape::Prism {
            footprint,
            base_z,
            top_z,
        } => geom::point_in_polygon(p, footprint).then_some((*base_z, *top_z)),
    }
}

/// 2D oriented box around a feature footprint (cylinders / prisms: their bounds).
fn obb(f: &Feature) -> Obb {
    match &f.shape {
        FeatureShape::Box {
            centre,
            half,
            yaw,
            pitch,
        } => Obb {
            c: [centre[0], centre[1]],
            f: [-yaw.sin(), yaw.cos()],
            hl: half[1] * pitch.cos() + half[2] * pitch.sin().abs(),
            hw: half[0],
        },
        _ => {
            let (lo, hi) = ground::bounds(&footprint(&f.shape));
            Obb {
                c: [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5],
                f: [0.0, 1.0],
                hl: (hi[1] - lo[1]) * 0.5,
                hw: (hi[0] - lo[0]) * 0.5,
            }
        }
    }
}

/// Rough vertical range of a feature.
fn zrange(f: &Feature) -> (f32, f32) {
    match &f.shape {
        FeatureShape::Box {
            centre,
            half,
            pitch,
            ..
        } => {
            let e = half[2] * pitch.cos() + half[1] * pitch.sin().abs();
            (centre[2] - e, centre[2] + e)
        }
        FeatureShape::Cylinder { base, height, .. } => (base[2], base[2] + height),
        FeatureShape::Prism { base_z, top_z, .. } => (*base_z, *top_z),
    }
}

fn centroid(f: &Feature) -> V2 {
    let fp = footprint(&f.shape);
    let n = fp.len() as f32;
    fp.iter()
        .fold([0.0, 0.0], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n])
}

fn in_site(s: &Site, f: &Feature) -> bool {
    ground::signed_dist(&s.polygon, centroid(f)) < 2.0
}

fn drivable(f: &Feature) -> bool {
    f.solid
        && matches!(
            f.kind,
            K::ParkingDeck | K::ParkingRamp | K::Frame | K::Earthwork | K::Pier
        )
}

/// The two top-centre end points of a pitched box (low end first) and its slope.
fn ramp_ends(f: &Feature) -> Option<(V3, V3, f32, V2)> {
    let FeatureShape::Box {
        centre,
        half,
        yaw,
        pitch,
    } = &f.shape
    else {
        return None;
    };
    if pitch.abs() < 1e-4 {
        return None;
    }
    let (sp, cp) = (pitch.sin(), pitch.cos());
    let fwd = [-yaw.sin(), yaw.cos()];
    // Top centre = centre + n * half_z, n = (-fwd sp, cp).
    let top = [
        centre[0] - fwd[0] * sp * half[2],
        centre[1] - fwd[1] * sp * half[2],
        centre[2] + cp * half[2],
    ];
    let along = |s: f32| {
        [
            top[0] + fwd[0] * cp * half[1] * s,
            top[1] + fwd[1] * cp * half[1] * s,
            top[2] + sp * half[1] * s,
        ]
    };
    let (a, b) = (along(-1.0), along(1.0));
    let (lo, hi, dir) = if a[2] <= b[2] {
        (a, b, fwd)
    } else {
        (b, a, [-fwd[0], -fwd[1]])
    };
    Some((lo, hi, pitch.tan().abs(), dir))
}

/// Does any drivable feature other than `skip` have its top within 3 cm of `e` just
/// beyond `e` (stepping `out` metres horizontally along `dir`)?
fn meets_surface(all: &[&Feature], skip: usize, e: V3, dir: V2, out: f32) -> bool {
    let q = [e[0] + dir[0] * out, e[1] + dir[1] * out];
    all.iter().enumerate().any(|(i, f)| {
        i != skip && drivable(f) && vspan(f, q).is_some_and(|(_, top)| (top - e[2]).abs() <= 0.03)
    })
}

fn sites(ctx: &Ctx, kinds: &[SiteKind]) -> Vec<Site> {
    ctx.plan
        .sites
        .iter()
        .filter(|s| kinds.contains(&s.kind))
        .cloned()
        .collect()
}

#[test]
fn budgets_and_counts() {
    with_city(1, |_ctx, park, cons| {
        let mut by: BTreeMap<K, (usize, usize)> = BTreeMap::new();
        for f in park.iter().chain(cons) {
            let e = by.entry(f.kind).or_default();
            e.0 += 1;
            e.1 += f.solid as usize;
        }
        for (k, (n, s)) in &by {
            eprintln!("{k:?}: {n} ({s} solid)");
        }
        let solid = |v: &[Feature]| v.iter().filter(|f| f.solid).count();
        eprintln!(
            "parking {} ({} solid), construction {} ({} solid)",
            park.len(),
            solid(park),
            cons.len(),
            solid(cons)
        );
        assert!(park.len() <= 10_000 && solid(park) <= 7_000);
        assert!(cons.len() <= 4_000);
        assert!(park.iter().filter(|f| f.kind == K::ParkedCar).count() > 1500);
    });
}

#[test]
fn garages_drive_from_gate_to_roof() {
    with_city(1, |ctx, park, _| {
        for s in sites(ctx, &[SiteKind::ParkingGarage]) {
            let fs: Vec<&Feature> = park.iter().filter(|f| in_site(&s, f)).collect();
            let gate = s.gates[0].pos;
            // Ramps: ≤ 1:7, ≥ 6.5 m wide, both ends flush with another surface (or, for the
            // driveway's low end, with the street / ground at the gate).
            let mut edges: Vec<Vec<usize>> = vec![Vec::new(); fs.len()];
            let mut entry = None;
            for (i, f) in fs.iter().enumerate() {
                if f.kind != K::ParkingRamp || !f.solid {
                    continue;
                }
                let (lo, hi, slope, dir) = ramp_ends(f).expect("ramps are pitched boxes");
                assert!(slope <= 1.0 / 7.0 + 1e-3, "{}: ramp slope {slope}", s.id);
                if let FeatureShape::Box { half, .. } = f.shape {
                    assert!(half[0] * 2.0 >= 6.5, "{}: ramp too narrow", s.id);
                }
                for (e, d) in [(hi, dir), (lo, [-dir[0], -dir[1]])] {
                    let q = [e[0] + d[0] * 0.05, e[1] + d[1] * 0.05];
                    let hits: Vec<usize> = fs
                        .iter()
                        .enumerate()
                        .filter(|(j, g)| {
                            *j != i
                                && drivable(g)
                                && vspan(g, q).is_some_and(|(_, t)| (t - e[2]).abs() <= 0.03)
                        })
                        .map(|(j, _)| j)
                        .collect();
                    if hits.is_empty() {
                        assert!(
                            geom::dist([e[0], e[1]], gate) < 1.0
                                && (e[2] - ctx.ground_z(gate)).abs() < 0.2,
                            "{}: ramp end {e:?} meets nothing",
                            s.id
                        );
                        entry = Some(i);
                    }
                    for j in hits {
                        edges[i].push(j);
                        edges[j].push(i);
                    }
                }
            }
            // Decks on one level connect where they touch.
            for i in 0..fs.len() {
                for j in i + 1..fs.len() {
                    let (a, b) = (fs[i], fs[j]);
                    if a.kind != K::ParkingDeck || b.kind != K::ParkingDeck {
                        continue;
                    }
                    if (zrange(a).1 - zrange(b).1).abs() > 0.01 {
                        continue;
                    }
                    let (mut oa, mut ob) = (obb(a), obb(b));
                    oa.hl += 0.05;
                    ob.hl += 0.05;
                    oa.hw += 0.05;
                    ob.hw += 0.05;
                    if geom::obb_overlap(&oa, &ob) {
                        edges[i].push(j);
                        edges[j].push(i);
                    }
                }
            }
            // The terrain never pokes through the ground floor.
            for f in fs.iter().filter(|f| f.kind == K::ParkingDeck) {
                if let FeatureShape::Prism {
                    footprint, top_z, ..
                } = &f.shape
                {
                    let (lo, hi) = ground::bounds(footprint);
                    for i in 0..=10 {
                        for j in 0..=10 {
                            let p = [
                                lo[0] + (hi[0] - lo[0]) * i as f32 / 10.0,
                                lo[1] + (hi[1] - lo[1]) * j as f32 / 10.0,
                            ];
                            assert!(
                                ctx.ground_z(p) <= top_z + 0.01,
                                "{}: terrain above the ground floor at {p:?}",
                                s.id
                            );
                        }
                    }
                }
            }
            let entry = entry.unwrap_or_else(|| panic!("{}: no driveway at the gate", s.id));
            let mut seen = vec![false; fs.len()];
            let mut todo = vec![entry];
            seen[entry] = true;
            while let Some(i) = todo.pop() {
                for &j in &edges[i] {
                    if !seen[j] {
                        seen[j] = true;
                        todo.push(j);
                    }
                }
            }
            let mut levels: Vec<f32> = fs
                .iter()
                .filter(|f| f.kind == K::ParkingDeck)
                .map(|f| zrange(f).1)
                .collect();
            levels.sort_by(f32::total_cmp);
            levels.dedup_by(|a, b| (*a - *b).abs() < 0.05);
            let roof = *levels.last().unwrap();
            let reached = fs.iter().enumerate().any(|(i, f)| {
                seen[i] && f.kind == K::ParkingDeck && (zrange(f).1 - roof).abs() < 0.01
            });
            assert!(reached, "{}: roof not reachable from the gate", s.id);
            let decks = levels.len() - 1;
            eprintln!("{}: {} decks above the ground floor", s.id, decks);
            assert!((3..=6).contains(&decks), "{}: {decks} decks", s.id);
            if s.id == "lantern_garage" {
                assert!(decks >= 4, "Lantern Garage Level 4 needs ≥ 4 decks");
            }
            for w in levels.windows(2) {
                assert!(
                    (2.9..=3.2).contains(&(w[1] - w[0])),
                    "{}: floor height",
                    s.id
                );
            }
        }
    });
}

#[test]
fn garages_have_headroom_everywhere_drivable() {
    with_city(1, |ctx, park, _| {
        for s in sites(ctx, &[SiteKind::ParkingGarage]) {
            let fs: Vec<&Feature> = park
                .iter()
                .filter(|f| f.solid && f.kind != K::ParkedCar && in_site(&s, f))
                .collect();
            let (lo, hi) = ground::bounds(&s.polygon);
            let mut checked = 0;
            let mut y = lo[1] + 0.35;
            while y < hi[1] {
                let mut x = lo[0] + 0.35;
                while x < hi[0] {
                    let p = [x, y];
                    let mut spans: Vec<(f32, f32, bool)> = fs
                        .iter()
                        .filter_map(|f| vspan(f, p).map(|(a, b)| (a, b, drivable(f))))
                        .collect();
                    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
                    // Merge, remembering whether the merged top is a drivable top.
                    let mut merged: Vec<(f32, f32, bool)> = Vec::new();
                    for (a, b, d) in spans {
                        if let Some(m) = merged.last_mut() {
                            if a <= m.1 + 0.02 {
                                if b > m.1 + 0.005 {
                                    m.1 = b;
                                    m.2 = d;
                                } else if (b - m.1).abs() <= 0.005 {
                                    m.2 |= d;
                                }
                                continue;
                            }
                        }
                        merged.push((a, b, d));
                    }
                    let g = ctx.ground_z(p);
                    for w in merged.windows(2) {
                        let gap = w[1].0 - w[0].1;
                        // Voids buried under the podium / terrain are not drivable.
                        if w[0].2 && w[0].1 > g - 0.1 {
                            checked += 1;
                            assert!(
                                !(0.25..2.4).contains(&gap),
                                "{}: headroom {gap:.2} m at {p:?} z {:.2}",
                                s.id,
                                w[0].1
                            );
                        }
                    }
                    x += 1.3;
                }
                y += 1.3;
            }
            assert!(checked > 1000, "{}: {checked} covered points", s.id);
        }
    });
}

#[test]
fn parked_cars_clear_of_structure_and_each_other() {
    with_city(1, |ctx, park, _| {
        for s in sites(ctx, &[SiteKind::ParkingGarage, SiteKind::SurfaceLot]) {
            let cars: Vec<&Feature> = park
                .iter()
                .filter(|f| f.kind == K::ParkedCar && in_site(&s, f))
                .collect();
            assert!(!cars.is_empty(), "{}: no cars", s.id);
            let blockers: Vec<&Feature> = park
                .iter()
                .filter(|f| {
                    f.solid && f.kind != K::ParkedCar && f.kind != K::ParkingDeck && in_site(&s, f)
                })
                .collect();
            for c in &cars {
                let (oc, zc) = (obb(c), zrange(c));
                for b in &blockers {
                    let zb = zrange(b);
                    if zb.1 < zc.0 + 0.2 || zb.0 > zc.1 - 0.05 {
                        continue;
                    }
                    if b.kind == K::Ground {
                        // Kerb islands are low: only a car standing on one is a problem.
                        if zb.1 > zc.0 + 0.05 && geom::obb_overlap(&oc, &obb(b)) {
                            panic!("{}: car on a kerb island", s.id);
                        }
                        continue;
                    }
                    assert!(
                        !geom::obb_overlap(&oc, &obb(b)),
                        "{}: parked car hits {:?} at {:?}",
                        s.id,
                        b.kind,
                        centroid(c)
                    );
                }
            }
            for (i, a) in cars.iter().enumerate() {
                for b in &cars[i + 1..] {
                    if (zrange(a).0 - zrange(b).0).abs() < 1.0 {
                        assert!(
                            !geom::obb_overlap(&obb(a), &obb(b)),
                            "{}: cars overlap at {:?}",
                            s.id,
                            centroid(a)
                        );
                    }
                }
            }
            let occ = cars.len();
            eprintln!("{}: {occ} parked cars", s.id);
        }
    });
}

#[test]
fn sites_are_fenced_with_openings_at_gates() {
    with_city(1, |ctx, park, cons| {
        let all: Vec<&Feature> = park.iter().chain(cons).collect();
        let edge = |f: &Feature| {
            f.solid && matches!(f.kind, K::Wall | K::Fence | K::Hedge | K::ConstructionFence)
        };
        for s in sites(
            ctx,
            &[
                SiteKind::ParkingGarage,
                SiteKind::SurfaceLot,
                SiteKind::Construction,
            ],
        ) {
            // Ground-level boundary pieces (upper-deck parapets do not count).
            let fences: Vec<&&Feature> = all
                .iter()
                .filter(|f| {
                    edge(f) && in_site(&s, f) && zrange(f).0 < ctx.ground_z(centroid(f)) + 1.0
                })
                .collect();
            for g in &s.gates {
                for f in &fences {
                    let fp = footprint(&f.shape);
                    let d = ground::signed_dist(&fp, g.pos);
                    assert!(
                        d > g.width_m * 0.5 - 0.5,
                        "{}: gate blocked ({d:.1} m)",
                        s.id
                    );
                }
            }
            let n = s.polygon.len();
            let mut open = 0;
            for i in 0..n {
                let (a, b) = (s.polygon[i], s.polygon[(i + 1) % n]);
                let len = geom::dist(a, b);
                let steps = (len / 3.0).ceil() as usize;
                for k in 0..steps {
                    let p = wp_world::metro::ctx::lerp2(a, b, (k as f32 + 0.5) / steps as f32);
                    if s.gates
                        .iter()
                        .any(|g| geom::dist(p, g.pos) < g.width_m * 0.5 + 8.0)
                    {
                        continue;
                    }
                    if ctx.occ_at(p) == Occ::Road {
                        continue;
                    }
                    let covered = fences.iter().any(|f| {
                        let fp = footprint(&f.shape);
                        ground::signed_dist(&fp, p) < 0.6
                    });
                    if !covered {
                        open += 1;
                        eprintln!("{}: open boundary at {p:?}", s.id);
                    }
                }
            }
            assert_eq!(open, 0, "{}: boundary not closed", s.id);
        }
    });
}

#[test]
fn nothing_on_road_paving_and_prisms_convex() {
    with_city(1, |ctx, park, cons| {
        for f in park.iter().chain(cons) {
            let fp = footprint(&f.shape);
            for p in fp.iter().chain(std::iter::once(&centroid(f))) {
                assert_ne!(ctx.occ_at(*p), Occ::Road, "{:?} on road paving", f.kind);
            }
            if let FeatureShape::Prism { footprint, .. } = &f.shape {
                if f.solid {
                    let n = footprint.len();
                    for i in 0..n {
                        let (a, b, c) =
                            (footprint[i], footprint[(i + 1) % n], footprint[(i + 2) % n]);
                        let cr = geom::cross(geom::sub(b, a), geom::sub(c, b));
                        assert!(cr >= -1e-3, "{:?} prism not convex / CCW", f.kind);
                    }
                }
            }
        }
    });
}

#[test]
fn ramps_meet_the_ground_and_stay_gentle() {
    with_city(1, |ctx, park, cons| {
        let all: Vec<&Feature> = park.iter().chain(cons).collect();
        let mut high_free: Vec<V3> = Vec::new();
        for (i, f) in all.iter().enumerate() {
            if !drivable(f) {
                continue;
            }
            let Some((lo, hi, slope, dir)) = ramp_ends(f) else {
                continue;
            };
            let back = [-dir[0], -dir[1]];
            let lo_free = !meets_surface(&all, i, lo, back, 0.05);
            let hi_free = !meets_surface(&all, i, hi, dir, 0.05);
            for (e, free) in [(lo, lo_free), (hi, hi_free)] {
                if !free {
                    continue;
                }
                let d = e[2] - ctx.ground_z([e[0], e[1]]);
                assert!(
                    d.abs() <= 0.03 || d >= 1.0,
                    "{:?} ramp end {e:?} has a {d:.2} m lip",
                    f.kind
                );
                if d >= 1.0 {
                    high_free.push(e);
                }
            }
            let kicker =
                f.kind == K::Earthwork && hi_free && hi[2] - ctx.ground_z([hi[0], hi[1]]) >= 1.0;
            assert!(
                slope <= 1.0 / 7.0 + 1e-3 || kicker,
                "{:?} slope {slope:.3} at {lo:?}",
                f.kind
            );
        }
        // The Unfinished Exchange is a real jump: two broken deck ends ~9-12 m up, 15-30 m
        // apart.
        let s = Plan::new().site("unfinished_exchange").unwrap().clone();
        let ends: Vec<&V3> = high_free
            .iter()
            .filter(|e| ground::signed_dist(&s.polygon, [e[0], e[1]]) < 0.0)
            .filter(|e| e[2] - ctx.ground_z([e[0], e[1]]) > 9.0)
            .collect();
        let jump = ends.iter().any(|a| {
            ends.iter().any(|b| {
                let d = geom::dist([a[0], a[1]], [b[0], b[1]]);
                (15.0..30.0).contains(&d)
            })
        });
        assert!(jump, "no jump at the Unfinished Exchange: {ends:?}");
    });
}

#[test]
fn deterministic() {
    let a = with_city(7, |_, p, c| (p.to_vec(), c.to_vec()));
    let b = with_city(7, |_, p, c| (p.to_vec(), c.to_vec()));
    assert!(a == b, "same seed, same features");
    let c = with_city(8, |_, p, _| p.to_vec());
    assert!(a.0 != c, "seed changes the parked cars");
}

#[test]
fn canal_flats_pit_is_dug_and_dry() {
    with_city(1, |ctx, _, _| {
        let s = ctx.plan.site("canal_earthworks").unwrap();
        let gate_z = ctx.ground_z(s.gates[0].pos);
        // Pit centre: 60 m right / 125 m in from the gate (gate heading south).
        let g = s.gates[0].pos;
        let pit = [g[0] - 60.0, g[1] - 125.0];
        let z = ctx.ground_z(pit);
        assert!(z < gate_z - 1.5, "pit {z:.2} vs gate {gate_z:.2}");
        assert!(!ctx.is_water(pit));
    });
}
