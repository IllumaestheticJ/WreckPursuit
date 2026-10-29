//! Landmark places (lead, 2026-10-29: "the dealership icons look nothing like dealerships").
//! In Port Vesper v2 and Ashpeak County every drive-to place (repair shops, police stations,
//! safehouses, car lots, the player's garages) has its building:
//!
//! - within [`MAX_RING_DISTANCE_M`] of its ring, every place laid out;
//! - no place piece on a lane, a sidewalk, a junction or inside any place's ring;
//! - every piece has its collider (the real `wp_sim` city colliders: a ray down onto each main
//!   building hits its roof);
//! - the greybox draws them in their own materials.

use city_import::glb::build_greybox;
use wp_world::geom::{self, V2};
use wp_world::network::{PoiKind, StructureKind};
use wp_world::places::{self, PlaceStyle, MAX_RING_DISTANCE_M, RING_RADIUS_M};
use wp_world::surface::SIDEWALK_W_M;
use wp_world::CityMap;

/// Distance from `p` to a polygon's outline (0 inside, convex polygons).
fn dist_to(p: V2, poly: &[V2]) -> f32 {
    let n = poly.len();
    let mut inside = true;
    let mut best = f32::INFINITY;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if geom::cross(geom::sub(b, a), geom::sub(p, a)) < 0.0 {
            inside = false;
        }
        best = best.min(geom::point_segment_distance(p, a, b));
    }
    if inside {
        0.0
    } else {
        best
    }
}

/// Points along a polygon's outline every 0.5 m, and its centroid.
fn outline(poly: &[V2]) -> Vec<V2> {
    let mut out = Vec::new();
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let n = (geom::dist(a, b) / 0.5).ceil().max(1.0) as usize;
        for k in 0..n {
            let t = k as f32 / n as f32;
            out.push(geom::add(a, geom::scale(geom::sub(b, a), t)));
        }
    }
    let c = poly.iter().fold([0.0, 0.0], |s, q| {
        geom::add(s, geom::scale(*q, 1.0 / poly.len() as f32))
    });
    out.push(c);
    out
}

fn check(name: &str, map: &CityMap) {
    let net = &map.net;
    let layouts = places::plan_with_pieces(map);
    let specs = places::place_specs(map);
    println!("{name}: {} places, {} laid out", specs.len(), layouts.len());
    for (l, b, s) in &layouts {
        let d = l
            .main
            .iter()
            .map(|_| dist_to(geom::xy(l.ring), &l.main))
            .fold(f32::INFINITY, f32::min);
        println!(
            "  {:32} {:?}: main building {:.1} m from the ring, {} buildings, {} pieces",
            l.name,
            l.style,
            d,
            b.len(),
            s.len()
        );
    }
    // Every place is laid out, and the layouts are what the network holds (the renderer
    // recomputes them from the finished map).
    let missing: Vec<&str> = specs
        .iter()
        .filter(|s| !layouts.iter().any(|(l, _, _)| l.id == s.id))
        .map(|s| s.name.as_str())
        .collect();
    assert!(missing.is_empty(), "{name}: no plot for {missing:?}");
    for (l, b, s) in &layouts {
        // (Ashpeak keeps Port Vesper's own pieces for the plots the two maps share, standing
        // on Port Vesper's ground: compare footprints.)
        for x in b {
            assert!(
                net.buildings
                    .iter()
                    .any(|y| y.footprint == x.footprint && y.kind == x.kind),
                "{name}: {} building not in the network",
                l.name
            );
        }
        for x in s {
            assert!(
                net.features
                    .structures
                    .iter()
                    .any(|y| y.footprint == x.footprint && y.kind == x.kind),
                "{name}: {} piece not in the network",
                l.name
            );
        }
        assert!(!b.is_empty(), "{name}: {} has no building", l.name);
        let d = dist_to(geom::xy(l.ring), &l.main);
        assert!(
            d <= MAX_RING_DISTANCE_M,
            "{name}: {} stands {d:.1} m from its ring",
            l.name
        );
    }
    let n_place_structures = net
        .features
        .structures
        .iter()
        .filter(|s| matches!(s.kind, StructureKind::Place(_)))
        .count();
    assert_eq!(
        n_place_structures,
        layouts.iter().map(|(_, _, s)| s.len()).sum::<usize>(),
        "{name}: stray place pieces"
    );
    // POI kinds that are places all get one.
    for p in &net.pois {
        if matches!(
            p.kind,
            PoiKind::Garage | PoiKind::PoliceStation | PoiKind::SafehouseCandidate
        ) {
            assert!(
                layouts.iter().any(|(l, _, _)| l.id == p.name),
                "{name}: POI {} has no building",
                p.name
            );
        }
    }

    // Nothing on a lane, sidewalk, junction or ring.
    let rings: Vec<V2> = layouts.iter().map(|(l, _, _)| geom::xy(l.ring)).collect();
    let cums: Vec<Vec<f32>> = net
        .surfaces
        .iter()
        .map(|s| geom::cumulative(&s.centre))
        .collect();
    let mut fails = Vec::new();
    for (l, b, s) in &layouts {
        let polys = b
            .iter()
            .map(|x| x.footprint.clone())
            .chain(s.iter().map(|x| x.footprint.clone()));
        for poly in polys {
            for q in outline(&poly) {
                for r in &rings {
                    if geom::dist(q, *r) < RING_RADIUS_M {
                        fails.push(format!("{}: piece inside a ring at {q:?}", l.name));
                    }
                }
                for (si, srf) in net.surfaces.iter().enumerate() {
                    if srf.centre.len() < 2 {
                        continue;
                    }
                    let pr = geom::project(&srf.centre, &cums[si], q);
                    if pr.dist > 60.0 {
                        continue;
                    }
                    let right = pr.lateral > 0.0;
                    let edge = if right { srf.right_m } else { srf.left_m };
                    let walk = if srf.sidewalk[usize::from(right)] {
                        SIDEWALK_W_M
                    } else {
                        0.0
                    };
                    if pr.dist < edge + walk {
                        fails.push(format!(
                            "{}: piece on road {} paving / sidewalk at {q:?}",
                            l.name, srf.road
                        ));
                    }
                }
                for ix in &net.intersections {
                    let c = geom::xy(net.nodes[ix.node as usize].pos);
                    if geom::dist(q, c) < ix.radius_m.max(0.0) {
                        fails.push(format!("{}: piece in a junction at {q:?}", l.name));
                    }
                }
            }
        }
    }
    fails.dedup();
    for f in fails.iter().take(20) {
        println!("    {f}");
    }
    assert!(fails.is_empty(), "{name}: {} overlaps", fails.len());

    // Colliders: the main building's roof stops a ray from above; so does every piece.
    let mut phys = wp_sim::PhysicsWorld::new();
    let counts = phys.set_city_ground(map);
    assert_eq!(counts.buildings, net.buildings.len(), "{name}: buildings");
    assert_eq!(
        counts.structures,
        net.features.structures.len(),
        "{name}: structures"
    );
    for (l, b, s) in &layouts {
        // The network's own pieces (see above).
        let b = b.iter().map(|x| {
            net.buildings
                .iter()
                .find(|y| y.footprint == x.footprint && y.kind == x.kind)
                .unwrap()
        });
        let s = s.iter().map(|x| {
            net.features
                .structures
                .iter()
                .find(|y| y.footprint == x.footprint && y.kind == x.kind)
                .unwrap()
        });
        for x in b {
            let c = outline(&x.footprint).pop().unwrap();
            let top = x.roof_z();
            let hit = phys.static_ground_z(c, x.ridge_z() + 5.0, 80.0);
            assert!(
                hit.is_some_and(|z| (z - top).abs() < 0.05 || z > top),
                "{name}: {} building has no collider ({hit:?}, roof {top})",
                l.name
            );
        }
        for x in s {
            let c = outline(&x.footprint).pop().unwrap();
            let hit = phys.static_ground_z(c, x.z1_m + 2.0, 50.0);
            assert!(
                hit.is_some_and(|z| z >= x.z1_m - 0.05),
                "{name}: {} piece {:?} has no collider ({hit:?}, top {})",
                l.name,
                x.kind,
                x.z1_m
            );
        }
    }

    // Every other named landmark already stands as geometry (structures, or a landmark
    // building at its spot; bridges and tunnels are roads), so none needs a building.
    for lm in &net.features.landmarks {
        use wp_world::network::{BuildingKind, LandmarkKind};
        let road = matches!(lm.kind, LandmarkKind::Bridge | LandmarkKind::Tunnel);
        let building = net.buildings.iter().any(|b| {
            b.kind == BuildingKind::Landmark && dist_to(geom::xy(lm.pos), &b.footprint) < 60.0
        });
        println!(
            "  landmark {:28} {:?}: {} structures, landmark building {building}",
            lm.name,
            lm.kind,
            lm.structures.len()
        );
        assert!(
            road || building || !lm.structures.is_empty(),
            "{name}: landmark {} has no geometry",
            lm.name
        );
    }

    // Drawn in their own materials.
    let mats: Vec<String> = build_greybox(map).into_iter().map(|p| p.material).collect();
    let styles: Vec<PlaceStyle> = layouts.iter().map(|(l, _, _)| l.style).collect();
    for (style, m) in [
        (PlaceStyle::Dealership, "place_showroom"),
        (PlaceStyle::Dealership, "place_glass_light"),
        (PlaceStyle::RepairShop, "place_workshop"),
        (PlaceStyle::RepairShop, "place_bay_light"),
        (PlaceStyle::PoliceStation, "place_civic"),
        (PlaceStyle::PoliceStation, "place_blue_light"),
        (PlaceStyle::Safehouse, "place_door"),
    ] {
        if styles.contains(&style) {
            assert!(mats.iter().any(|x| x == m), "{name}: greybox has no {m}");
        }
    }
}

#[test]
fn vesper_v2_places_have_their_buildings() {
    let net = wp_world::vesper::generate(&wp_world::vesper::VesperConfig::default());
    check("vesper_v2", &CityMap::new(net));
}

#[test]
fn ashpeak_places_have_their_buildings() {
    let net = wp_world::ashpeak::generate(&wp_world::ashpeak::default_config());
    let map = CityMap::new(net);
    check("ashpeak", &map);
    // The combined world has every car lot and home garage.
    let laid: Vec<String> = places::plan(&map).into_iter().map(|l| l.id).collect();
    for s in &places::DRIVE_TO_SPOTS {
        assert!(
            laid.iter().any(|id| id == s.id),
            "ashpeak: {} not laid out",
            s.id
        );
    }
}
