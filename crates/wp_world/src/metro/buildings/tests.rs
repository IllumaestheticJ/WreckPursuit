use std::collections::BTreeMap;
use std::time::Instant;

use wp_math::Rng;

use super::super::ctx::{Ctx, Occ};
use super::super::ground::{self, inside, Ground, Natural};
use super::super::plan::{self, Plan, SiteKind};
use super::super::roads::{self, RoadsOut};
use super::super::{airport, construction, docks, parking, rail, scenery, MetroConfig};
use super::parts::{convex_ccw, shape_footprint};
use super::*;
use crate::builder::{self, BuildOptions, NodeSpec, RoadClass, RoadSpec};
use crate::network::{Building, DistrictKind, Feature, FeatureShape};
use crate::terrain::Heightfield;

/// Plan, final terrain and roads, as `metro::generate_with_report` builds them.
struct World {
    plan: Plan,
    terrain: Heightfield,
    roads: RoadsOut,
}

fn ground_for(plan: &Plan, cfg: &MetroConfig) -> Ground {
    let mut rng = Rng::new(cfg.seed);
    let nat = Natural::new(plan, &mut rng);
    let mut works = Vec::new();
    works.extend(airport::earthworks(plan, &nat));
    works.extend(docks::earthworks(plan, &nat));
    works.extend(rail::earthworks(plan, &nat));
    works.extend(parking::earthworks(plan, &nat));
    works.extend(construction::earthworks(plan, &nat));
    works.extend(earthworks(plan, &nat));
    works.extend(scenery::earthworks(plan, &nat));
    for s in &plan.sites {
        if let Some(z) = s.pad_z {
            works.push(ground::Earthwork::Pad {
                polygon: s.polygon.clone(),
                z,
                blend_m: 60.0,
            });
        }
    }
    Ground::new(&nat, cfg.terrain_cell_m, &works)
}

fn world(seed: u64) -> World {
    let cfg = MetroConfig {
        seed,
        ..Default::default()
    };
    let plan = Plan::new();
    let mut gnd = ground_for(&plan, &cfg);
    let roads = roads::build(&plan, &mut gnd, &cfg);
    World {
        plan,
        terrain: gnd.hf,
        roads,
    }
}

/// A dense synthetic street grid (120 m blocks of 1+1 locals, 2+2 every 4th line) over the
/// whole map: a stand-in for the real network when measuring counts and timing.
fn grid_world(seed: u64) -> World {
    let cfg = MetroConfig {
        seed,
        ..Default::default()
    };
    let plan = Plan::new();
    let gnd = ground_for(&plan, &cfg);
    let step = 120.0;
    let nx = ((plan::MAP_HI[0] - plan::MAP_LO[0]) / step) as usize;
    let ny = ((plan::MAP_HI[1] - plan::MAP_LO[1]) / step) as usize;
    let mut nodes = Vec::new();
    for j in 0..=ny {
        for i in 0..=nx {
            let p = [
                plan::MAP_LO[0] + 40.0 + i as f32 * step,
                plan::MAP_LO[1] + 40.0 + j as f32 * step,
            ];
            nodes.push(NodeSpec {
                pos: p,
                z: gnd.z(p).max(0.5),
                signal: false,
                control: None,
            });
        }
    }
    let id = |i: usize, j: usize| j * (nx + 1) + i;
    let blocked = |p: V2| {
        gnd.is_water(p)
            || plan
                .sites
                .iter()
                .any(|s| ground::signed_dist(&s.polygon, p) < 10.0)
    };
    let mut specs = Vec::new();
    let mut names = Vec::new();
    for j in 0..=ny {
        for i in 0..=nx {
            for (di, dj) in [(1, 0), (0, 1)] {
                let (i2, j2) = (i + di, j + dj);
                if i2 > nx || j2 > ny {
                    continue;
                }
                let (a, b) = (nodes[id(i, j)].pos, nodes[id(i2, j2)].pos);
                if [a, b, [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]]
                    .iter()
                    .any(|p| blocked(*p))
                {
                    continue;
                }
                let major = if di == 1 { j % 4 == 0 } else { i % 4 == 0 };
                let (class, lanes) = if major {
                    (RoadClass::Arterial, 2)
                } else {
                    (RoadClass::Local, 1)
                };
                specs.push(RoadSpec::new(id(i, j), id(i2, j2), class, lanes, lanes));
                names.push(String::new());
            }
        }
    }
    let built = builder::build(&nodes, &specs, &BuildOptions::default());
    World {
        plan,
        terrain: gnd.hf,
        roads: RoadsOut {
            nodes,
            specs,
            built,
            names,
            features: Vec::new(),
            notes: Vec::new(),
            report: Default::default(),
        },
    }
}

/// Context after the modules that run before buildings.
fn before(w: &World, seed: u64) -> Ctx<'_> {
    let mut ctx = Ctx::new(&w.plan, &w.terrain, &w.roads, seed);
    airport::generate(&mut ctx);
    docks::generate(&mut ctx);
    rail::generate(&mut ctx);
    parking::generate(&mut ctx);
    construction::generate(&mut ctx);
    ctx
}

/// CPU time of this thread, s (Linux; wall clock elsewhere is not comparable under load).
fn cpu_s() -> Option<f32> {
    let s = std::fs::read_to_string("/proc/thread-self/schedstat").ok()?;
    let ns: f64 = s.split_whitespace().next()?.parse().ok()?;
    Some((ns * 1e-9) as f32)
}

/// Run buildings; returns (context, features before, seconds: thread CPU time if
/// available, else wall clock).
fn after(w: &World, seed: u64) -> (Ctx<'_>, usize, f32) {
    let mut ctx = before(w, seed);
    let n0 = ctx.features.len();
    let t = Instant::now();
    let c = cpu_s();
    generate(&mut ctx);
    let secs = match (c, cpu_s()) {
        (Some(a), Some(b)) => b - a,
        _ => t.elapsed().as_secs_f32(),
    };
    (ctx, n0, secs)
}

fn centroid(p: &[V2]) -> V2 {
    let n = p.len() as f32;
    let s = p.iter().fold([0.0, 0.0], |a, q| [a[0] + q[0], a[1] + q[1]]);
    [s[0] / n, s[1] / n]
}

/// Grid cell centres (2 m) strictly inside `poly`.
fn cells_in(poly: &[V2]) -> Vec<V2> {
    let (lo, hi) = ground::bounds(poly);
    let c = super::super::ctx::OCC_CELL_M;
    let mut out = Vec::new();
    let i0 = ((lo[0] - plan::MAP_LO[0]) / c).floor() as i64;
    let i1 = ((hi[0] - plan::MAP_LO[0]) / c).ceil() as i64;
    let j0 = ((lo[1] - plan::MAP_LO[1]) / c).floor() as i64;
    let j1 = ((hi[1] - plan::MAP_LO[1]) / c).ceil() as i64;
    for j in j0..=j1 {
        for i in i0..=i1 {
            let p = [
                plan::MAP_LO[0] + (i as f32 + 0.5) * c,
                plan::MAP_LO[1] + (j as f32 + 0.5) * c,
            ];
            if inside(poly, p) {
                out.push(p);
            }
        }
    }
    out
}

fn report(ctx: &Ctx, n0: usize, secs: f32, label: &str) {
    let mut kinds: BTreeMap<_, (usize, usize)> = BTreeMap::new();
    for f in &ctx.features[n0..] {
        let e = kinds.entry(f.kind).or_default();
        e.0 += 1;
        if f.solid {
            e.1 += 1;
        }
    }
    let solid: usize = kinds.values().map(|v| v.1).sum();
    eprintln!(
        "[{label}] buildings {} | features {} ({} solid) | {:.3} s",
        ctx.buildings.len(),
        ctx.features.len() - n0,
        solid,
        secs
    );
    for (k, (n, s)) in &kinds {
        eprintln!("  {k:?}: {n} ({s} solid)");
    }
    let mut by: BTreeMap<&str, (usize, f32)> = BTreeMap::new();
    for b in &ctx.buildings {
        let c = centroid(&b.footprint);
        let z = lots::zone_at(ctx.plan, c).map_or("-", |z| z.0);
        let e = by.entry(z).or_default();
        e.0 += 1;
        e.1 += b.height_m;
    }
    for (z, (n, h)) in &by {
        eprintln!("  {z}: {n} buildings, mean height {:.1} m", h / *n as f32);
    }
}

#[test]
fn footprints_are_convex_ccw_and_clear() {
    let w = world(1);
    let pre = before(&w, 1);
    let (ctx, n0, secs) = after(&w, 1);
    report(&ctx, n0, secs, "stub roads");
    assert!(ctx.buildings.len() > 300, "{}", ctx.buildings.len());
    let landmark_sites: Vec<_> = w.plan.sites_of(SiteKind::Landmark).collect();
    // Only this module's buildings: earlier modules (airport, docks) add their own.
    for b in &ctx.buildings[pre.buildings.len()..] {
        assert!(convex_ccw(&b.footprint), "footprint not convex CCW: {b:?}");
        assert!(b.height_m > 2.0 && b.height_m < 260.0, "{b:?}");
        let c = centroid(&b.footprint);
        let own = landmark_sites
            .iter()
            .find(|s| inside(&s.polygon, c))
            .map(|s| &s.polygon);
        let mut cells = cells_in(&b.footprint);
        cells.push(c);
        for p in cells {
            let o = pre.occ_at(p);
            assert!(
                !pre.is_water(p),
                "building over water at {p:?}: {:?}",
                b.footprint
            );
            match own {
                None => assert_eq!(o, Occ::Free, "building on {o:?} at {p:?}"),
                Some(site) => {
                    assert!(
                        matches!(o, Occ::Free | Occ::Site),
                        "landmark on {o:?} at {p:?}"
                    );
                    assert!(ground::signed_dist(site, p) < 1.0, "outside its site");
                }
            }
        }
    }
    for f in &ctx.features[n0..] {
        if let FeatureShape::Prism { footprint, .. } = &f.shape {
            if f.solid {
                assert!(convex_ccw(footprint), "solid prism not convex: {f:?}");
            }
        }
        // Nothing solid on road paving.
        if f.solid {
            let fp = shape_footprint(&f.shape);
            for p in cells_in(&fp) {
                assert_ne!(pre.occ_at(p), Occ::Road, "{:?} on a road at {p:?}", f.kind);
            }
        }
    }
}

fn mean_heights(buildings: &[Building], plan: &Plan) -> BTreeMap<DistrictKind, (usize, f32)> {
    let mut out: BTreeMap<DistrictKind, (usize, f32)> = BTreeMap::new();
    let landmarks: Vec<_> = plan.sites_of(SiteKind::Landmark).collect();
    for b in buildings {
        let c = centroid(&b.footprint);
        if landmarks.iter().any(|s| inside(&s.polygon, c)) {
            continue;
        }
        if let Some(z) = plan.zones.iter().find(|z| inside(&z.zone.polygon, c)) {
            let e = out.entry(z.zone.kind).or_default();
            e.0 += 1;
            e.1 += b.height_m;
        }
    }
    out
}

#[test]
fn districts_have_plausible_heights_and_landmarks_exist() {
    let w = world(1);
    let (ctx, n0, _) = after(&w, 1);
    let m = mean_heights(&ctx.buildings, &w.plan);
    let mean = |k: DistrictKind| m.get(&k).map(|v| v.1 / v.0 as f32);
    eprintln!("{m:?}");
    let core = mean(DistrictKind::Core).expect("core buildings");
    let suburb = mean(DistrictKind::Suburb).expect("suburb buildings");
    for (k, (n, h)) in &m {
        let h = h / *n as f32;
        assert!(core >= h, "core {core} lower than {k:?} {h}");
        assert!(suburb <= h, "suburb {suburb} higher than {k:?} {h}");
    }
    assert!(core > 30.0, "core mean {core}");
    assert!(suburb < 9.0, "suburb mean {suburb}");
    // Every landmark site has content.
    for s in w.plan.sites_of(SiteKind::Landmark) {
        let n = ctx.features[n0..]
            .iter()
            .filter(|f| inside(&s.polygon, centroid(&shape_footprint(&f.shape))))
            .count()
            + ctx
                .buildings
                .iter()
                .filter(|b| inside(&s.polygon, centroid(&b.footprint)))
                .count();
        assert!(n >= 3, "{} has {n} pieces", s.id);
    }
    // Tall landmark towers downtown.
    let towers = ctx.buildings.iter().filter(|b| b.height_m > 150.0).count();
    assert!((1..=12).contains(&towers), "{towers} landmark towers");
}

/// The stadium stands are built in its site and the way in from the gate to the pitch is
/// open (no solid feature or building on it).
#[test]
fn stadium_gate_drives_onto_the_pitch() {
    let w = world(1);
    let (ctx, n0, _) = after(&w, 1);
    let site = w.plan.site("vesper_bowl").expect("vesper_bowl site");
    let stands = ctx.features[n0..]
        .iter()
        .filter(|f| f.solid && f.kind == crate::network::FeatureKind::Landmark)
        .filter(|f| inside(&site.polygon, centroid(&shape_footprint(&f.shape))))
        .count();
    assert!(stands >= 40, "{stands} stand pieces");
    let g = site.gates[0].pos;
    let c = centroid(&site.polygon);
    for k in 0..=20 {
        let t = k as f32 / 20.0 * 0.8;
        let p = [g[0] + (c[0] - g[0]) * t, g[1] + (c[1] - g[1]) * t];
        let gz = ctx.ground_z(p);
        for f in ctx.features[n0..].iter().filter(|f| f.solid) {
            // Overhead parts (the gate arch) leave room to drive under.
            let bottom = match &f.shape {
                FeatureShape::Box { centre, half, .. } => centre[2] - half[2],
                FeatureShape::Cylinder { base, .. } => base[2],
                FeatureShape::Prism { base_z, .. } => *base_z,
            };
            if bottom > gz + 4.5 {
                continue;
            }
            let fp = shape_footprint(&f.shape);
            assert!(
                !inside(&fp, p),
                "{:?} blocks the stadium gate at {p:?}",
                f.kind
            );
        }
        for b in &ctx.buildings {
            assert!(
                !inside(&b.footprint, p),
                "building blocks the stadium gate at {p:?}"
            );
        }
    }
}

#[test]
fn deterministic() {
    let w = world(1);
    let (a, n0, _) = after(&w, 1);
    let (b, _, _) = after(&w, 1);
    assert_eq!(a.buildings, b.buildings);
    let fa: &[Feature] = &a.features[n0..];
    let fb: &[Feature] = &b.features[n0..];
    assert_eq!(fa, fb);
    let (c, _, _) = after(&w, 2);
    assert_ne!(a.buildings, c.buildings, "seed changes the lots");
}

#[test]
fn budget_on_stub_roads() {
    let w = world(1);
    let (ctx, n0, _) = after(&w, 1);
    let feats = &ctx.features[n0..];
    assert!(ctx.buildings.len() <= 15_000);
    assert!(feats.len() <= 20_000, "{}", feats.len());
    assert!(feats.iter().filter(|f| f.solid).count() <= 15_000);
}

/// Counts and timing on a dense synthetic street grid (run with `--ignored --nocapture`,
/// ideally `--release`).
#[test]
#[ignore]
fn dense_grid_counts_and_timing() {
    let w = grid_world(1);
    let (ctx, n0, secs) = after(&w, 1);
    report(&ctx, n0, secs, "synthetic 120 m grid");
    eprintln!(
        "budget (row group, spread): {:?}",
        lots::budget(&before(&w, 1))
    );
    let pre = before(&w, 1);
    for b in &ctx.buildings {
        assert!(convex_ccw(&b.footprint));
        let c = centroid(&b.footprint);
        if w.plan
            .sites_of(SiteKind::Landmark)
            .any(|s| inside(&s.polygon, c))
        {
            continue;
        }
        for p in cells_in(&b.footprint) {
            assert_eq!(pre.occ_at(p), Occ::Free);
        }
    }
}

/// Dump buildings, this module's features, road edges and sites as JSON to
/// `$WP_BUILDINGS_DUMP` (previews; `--ignored`). `WP_BUILDINGS_GRID=1` uses the synthetic grid.
#[test]
#[ignore]
fn dump_for_preview() {
    let Ok(path) = std::env::var("WP_BUILDINGS_DUMP") else {
        return;
    };
    let w = if std::env::var("WP_BUILDINGS_GRID").is_ok() {
        grid_world(1)
    } else {
        world(1)
    };
    let (ctx, n0, _) = after(&w, 1);
    let b: Vec<_> = ctx
        .buildings
        .iter()
        .map(|b| serde_json::json!({"fp": b.footprint, "h": b.height_m}))
        .collect();
    let f: Vec<_> = ctx.features[n0..]
        .iter()
        .map(|f| {
            serde_json::json!({"fp": shape_footprint(&f.shape), "k": format!("{:?}", f.kind),
                "s": f.solid})
        })
        .collect();
    let r: Vec<_> = w
        .roads
        .built
        .surfaces
        .iter()
        .map(|s| {
            let (l, r) = crate::surface::surface_edges(s);
            let mut p: Vec<V2> = l.iter().map(|q| [q[0], q[1]]).collect();
            p.extend(r.iter().rev().map(|q| [q[0], q[1]]));
            p
        })
        .collect();
    let sites: Vec<_> = w.plan.sites.iter().map(|s| s.polygon.clone()).collect();
    let j = serde_json::json!({"b": b, "f": f, "r": r, "sites": sites});
    std::fs::write(path, j.to_string()).expect("write dump");
}
