//! Docks tests: run the v3 pipeline up to (and including) this module, then check the
//! placed content against the terrain, the occupancy grid and the plan.

// Test-only module (declared under `#[cfg(test)]`); the attribute below also tells
// lint-determinism to stop scanning here (tests may time themselves).
#[cfg(test)]
use std::collections::BTreeMap;

use wp_math::Rng;

use super::super::ctx::{footprint, Ctx, Occ};
use super::super::ground::{inside, signed_dist};
use super::super::plan::{SiteKind, WATER_Z};
use super::super::{airport, ground, plan, roads, MetroConfig};
use super::*;
use crate::network::{FeatureKind, FeatureShape};

/// Run the pipeline up to the docks and hand the context + report to `f`.
fn with_docks<R>(seed: u64, f: impl FnOnce(&Ctx, &DocksReport) -> R) -> R {
    let cfg = MetroConfig {
        seed,
        ..MetroConfig::default()
    };
    let plan = plan::Plan::new();
    let mut rng = Rng::new(cfg.seed);
    let nat = ground::Natural::new(&plan, &mut rng);
    let mut works = Vec::new();
    works.extend(airport::earthworks(&plan, &nat));
    works.extend(earthworks(&plan, &nat));
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
    let mut ctx = Ctx::new(&plan, &terrain, &roads, cfg.seed);
    airport::generate(&mut ctx);
    let t0 = std::time::Instant::now();
    let rep = generate_with_report(&mut ctx);
    eprintln!("docks generate: {:.1} ms", t0.elapsed().as_secs_f64() * 1e3);
    f(&ctx, &rep)
}

fn docks_sites(ctx: &Ctx) -> Vec<Vec<V2>> {
    ctx.plan
        .sites
        .iter()
        .filter(|s| {
            matches!(
                s.kind,
                SiteKind::ContainerTerminal | SiteKind::Quay | SiteKind::Piers
            )
        })
        .map(|s| ccw(s.polygon.clone()))
        .collect()
}

fn centre(f: &Feature) -> V2 {
    match &f.shape {
        FeatureShape::Box { centre, .. } => [centre[0], centre[1]],
        FeatureShape::Cylinder { base, .. } => [base[0], base[1]],
        FeatureShape::Prism { footprint, .. } => {
            let n = footprint.len() as f32;
            footprint
                .iter()
                .fold([0.0, 0.0], |s, p| [s[0] + p[0] / n, s[1] + p[1] / n])
        }
    }
}

fn top_z(f: &Feature) -> f32 {
    match &f.shape {
        FeatureShape::Box { centre, half, .. } => centre[2] + half[2],
        FeatureShape::Cylinder { base, height, .. } => base[2] + height,
        FeatureShape::Prism { top_z, .. } => *top_z,
    }
}

fn is_convex(poly: &[V2]) -> bool {
    let n = poly.len();
    (0..n).all(|i| {
        let (a, b, c) = (poly[i], poly[(i + 1) % n], poly[(i + 2) % n]);
        geom::cross(geom::sub(b, a), geom::sub(c, b)) >= -1e-3
    })
}

#[test]
fn builds_within_budget() {
    with_docks(1, |ctx, rep| {
        let fs = &ctx.features[rep.features.clone()];
        let mut by_kind: BTreeMap<FeatureKind, (usize, usize)> = BTreeMap::new();
        for f in fs {
            let e = by_kind.entry(f.kind).or_default();
            e.0 += 1;
            if f.solid {
                e.1 += 1;
            }
        }
        for (k, (n, s)) in &by_kind {
            eprintln!("{k:?}: {n} ({s} solid)");
        }
        let solid = fs.iter().filter(|f| f.solid).count();
        eprintln!(
            "docks: {} features ({} solid), {} blocks, {} ships, {} cranes, {} piers, {} lanes",
            fs.len(),
            solid,
            rep.blocks,
            rep.ships,
            rep.cranes,
            rep.piers.len(),
            rep.lanes.len()
        );
        assert!(fs.len() <= 12_500, "feature budget: {}", fs.len());
        assert!(solid <= 8_000, "solid budget: {solid}");
        assert!(rep.blocks >= 100, "container blocks: {}", rep.blocks);
        assert!((2..=4).contains(&(rep.ships - 1)), "ships: {}", rep.ships);
        assert!((9..=14).contains(&rep.cranes), "cranes: {}", rep.cranes);
        assert_eq!(rep.piers.len(), 4, "one pier per gate");
    });
}

#[test]
fn terminal_waterline_has_a_barrier() {
    with_docks(1, |ctx, rep| {
        let fs = &ctx.features[rep.features.clone()];
        let site = ctx.plan.site("container_terminal").expect("terminal");
        let pad = site.pad_z.expect("pad");
        // Independent waterline walk: every 10 m of the boundary, march outward to water.
        let poly = ccw(site.polygon.clone());
        let mut total = 0;
        let mut ok = 0;
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let t = geom::normalize(geom::sub(b, a));
            let n = geom::right_of(t);
            let len = geom::dist(a, b);
            let mut s = 5.0;
            while s < len {
                let p = off(a, t, s);
                let mut d = 0.0;
                while d <= 100.0 && !ctx.is_water(off(p, n, d)) {
                    d += 1.0;
                }
                if d <= 100.0 {
                    total += 1;
                    let w = off(p, n, d);
                    let barrier = fs.iter().any(|f| {
                        f.kind == FeatureKind::QuayWall
                            && f.solid
                            && top_z(f) >= pad + 0.3
                            && signed_dist(&footprint(&f.shape), w) <= 5.0
                    });
                    if barrier {
                        ok += 1;
                    } else if total % 5 == 0 {
                        eprintln!("no kerb near waterline {w:?} (edge {i}, d {d})");
                    }
                }
                s += 10.0;
            }
        }
        eprintln!("terminal waterline: {ok}/{total} samples with a kerb");
        assert!(total > 100, "terminal has a waterline");
        assert!(ok as f32 >= 0.9 * total as f32, "{ok}/{total}");
    });
}

#[test]
fn containers_on_land_in_a_site_or_on_a_ship() {
    with_docks(1, |ctx, rep| {
        let fs = &ctx.features[rep.features.clone()];
        let sites = docks_sites(ctx);
        let hulls: Vec<Vec<V2>> = fs
            .iter()
            .filter(|f| f.kind == FeatureKind::Ship && f.solid)
            .map(|f| footprint(&f.shape))
            .collect();
        let mut on_ships = 0;
        for f in fs.iter().filter(|f| f.kind == FeatureKind::Container) {
            let c = centre(f);
            if hulls.iter().any(|h| inside(h, c)) {
                on_ships += 1;
                continue;
            }
            assert!(
                sites.iter().any(|s| inside(s, c)) && !ctx.is_water(c),
                "container at {c:?} is not on land in a site"
            );
        }
        assert!(on_ships > 50, "deck cargo on ships: {on_ships}");
    });
}

#[test]
fn crane_legs_on_land() {
    with_docks(1, |ctx, rep| {
        let fs = &ctx.features[rep.features.clone()];
        let legs: Vec<&Feature> = fs
            .iter()
            .filter(|f| f.kind == FeatureKind::GantryCrane && f.solid)
            .collect();
        assert!(legs.len() >= 36);
        for f in legs {
            for p in footprint(&f.shape) {
                assert!(ctx.ground_z(p) > WATER_Z, "crane leg in the water at {p:?}");
            }
        }
    });
}

#[test]
fn piers_are_solid_and_reachable_from_their_gates() {
    with_docks(1, |ctx, rep| {
        let fs = &ctx.features[rep.features.clone()];
        let site = ctx.plan.site("old_harbor_piers").expect("piers");
        for p in &rep.piers {
            assert!(
                site.gates.iter().any(|g| geom::dist(g.pos, p.gate) < 0.1),
                "pier starts at a plan gate"
            );
            let start = [p.start[0], p.start[1]];
            assert!(geom::dist(start, p.gate) <= 12.0, "deck starts at the gate");
            let gz = ctx.ground_z(start);
            assert!(
                (p.start[2] - gz).abs() <= 0.5,
                "deck {} vs ground {gz} at the gate",
                p.start[2]
            );
            // A solid Pier deck covers the gate end and the far end.
            let far = geom::scale(geom::add(p.footprint[1], p.footprint[2]), 0.5);
            for q in [off(start, geom::normalize(geom::sub(far, start)), 1.0), far] {
                let q = off(q, geom::normalize(geom::sub(start, far)), 1.0);
                assert!(
                    fs.iter().any(|f| f.kind == FeatureKind::Pier
                        && f.solid
                        && inside(&footprint(&f.shape), q)),
                    "solid deck at {q:?}"
                );
            }
        }
        let rails = fs
            .iter()
            .filter(|f| f.kind == FeatureKind::GuardRail)
            .count();
        assert!(rails > 60, "pier railings: {rails}");
    });
}

#[test]
fn fences_open_at_gates_and_close_elsewhere() {
    with_docks(1, |ctx, rep| {
        let fs = &ctx.features[rep.features.clone()];
        let fences: Vec<Vec<V2>> = fs
            .iter()
            .filter(|f| matches!(f.kind, FeatureKind::Fence | FeatureKind::Wall) && f.solid)
            .map(|f| footprint(&f.shape))
            .collect();
        assert!(
            fences.len() > 200,
            "perimeter fence pieces: {}",
            fences.len()
        );
        for s in ctx.plan.sites.iter().filter(|s| {
            matches!(
                s.kind,
                SiteKind::ContainerTerminal | SiteKind::Quay | SiteKind::Piers
            )
        }) {
            for g in &s.gates {
                for fp in &fences {
                    let d = signed_dist(fp, g.pos);
                    assert!(
                        d >= g.width_m * 0.5 - 0.5,
                        "{}: fence {d:.1} m from the gate at {:?}",
                        s.id,
                        g.pos
                    );
                }
            }
        }
    });
}

#[test]
fn nothing_on_roads_and_prisms_convex() {
    with_docks(1, |ctx, rep| {
        for f in &ctx.features[rep.features.clone()] {
            let fp = footprint(&f.shape);
            let blocked: &[Occ] = if matches!(f.kind, FeatureKind::Pier | FeatureKind::GuardRail) {
                &[Occ::Road]
            } else {
                &[Occ::Road, Occ::Verge]
            };
            assert!(
                ctx.free_of(&fp, 0.0, blocked),
                "{:?} on a road at {:?}",
                f.kind,
                centre(f)
            );
            if let FeatureShape::Prism { footprint, .. } = &f.shape {
                if f.solid {
                    assert!(is_convex(footprint), "{:?} prism not convex", f.kind);
                }
            }
        }
    });
}

#[test]
fn lanes_between_blocks_are_clear() {
    with_docks(1, |ctx, rep| {
        let fs = &ctx.features[rep.features.clone()];
        assert!(rep.lanes.len() > 100, "lanes: {}", rep.lanes.len());
        // Bucket solid footprints on a 50 m grid.
        let mut grid: BTreeMap<(i32, i32), Vec<usize>> = BTreeMap::new();
        let fps: Vec<Vec<V2>> = fs.iter().map(|f| footprint(&f.shape)).collect();
        for (i, f) in fs.iter().enumerate() {
            if !f.solid {
                continue;
            }
            let (lo, hi) = super::super::ground::bounds(&fps[i]);
            for gx in (lo[0] / 50.0).floor() as i32..=(hi[0] / 50.0).floor() as i32 {
                for gy in (lo[1] / 50.0).floor() as i32..=(hi[1] / 50.0).floor() as i32 {
                    grid.entry((gx, gy)).or_default().push(i);
                }
            }
        }
        for lane in &rep.lanes {
            let (lo, hi) = super::super::ground::bounds(lane);
            for gx in (lo[0] / 50.0).floor() as i32..=(hi[0] / 50.0).floor() as i32 {
                for gy in (lo[1] / 50.0).floor() as i32..=(hi[1] / 50.0).floor() as i32 {
                    for &i in grid.get(&(gx, gy)).map(|v| v.as_slice()).unwrap_or(&[]) {
                        assert!(
                            !convex_overlap(&fps[i], lane),
                            "{:?} at {:?} blocks a lane",
                            fs[i].kind,
                            centre(&fs[i])
                        );
                    }
                }
            }
        }
    });
}

#[test]
fn deterministic() {
    let a = with_docks(7, |ctx, rep| ctx.features[rep.features.clone()].to_vec());
    let b = with_docks(7, |ctx, rep| ctx.features[rep.features.clone()].to_vec());
    assert_eq!(a.len(), b.len());
    assert!(a == b, "same seed, same features");
}

/// Debug dump for a top-down picture: `DOCKS_DUMP=/path cargo test dump -- --ignored`.
#[test]
#[ignore]
fn dump() {
    let Ok(path) = std::env::var("DOCKS_DUMP") else {
        return;
    };
    with_docks(1, |ctx, rep| {
        use std::fmt::Write as _;
        let mut s = String::new();
        let mut y = -3400.0f32;
        while y <= 1000.0 {
            let mut row = String::from("W ");
            let mut x = 1400.0f32;
            while x <= 3600.0 {
                let c = match ctx.occ_at([x, y]) {
                    Occ::Road => 'R',
                    Occ::Verge => 'v',
                    _ if ctx.is_water([x, y]) => '~',
                    _ => '.',
                };
                row.push(c);
                x += 4.0;
            }
            writeln!(s, "{row}").unwrap();
            y += 4.0;
        }
        for f in &ctx.features[rep.features.clone()] {
            let fp = footprint(&f.shape);
            let pts: Vec<String> = fp
                .iter()
                .map(|p| format!("{:.1},{:.1}", p[0], p[1]))
                .collect();
            writeln!(
                s,
                "F {:?} {} {:.1} {:?} {}",
                f.kind,
                f.solid as u8,
                top_z(f),
                f.material,
                pts.join(" ")
            )
            .unwrap();
        }
        for b in &ctx.buildings {
            let pts: Vec<String> = b
                .footprint
                .iter()
                .map(|p| format!("{:.1},{:.1}", p[0], p[1]))
                .collect();
            writeln!(
                s,
                "F Building 1 {:.1} Concrete {}",
                b.roof_z(),
                pts.join(" ")
            )
            .unwrap();
        }
        std::fs::write(path, s).unwrap();
    });
}
