//! Sweep: solid things on Loop 9's lanes (owner 2026-10-26 "random object stopped me").
use rapier3d::prelude::*;
use wp_sim::PhysicsWorld;
use wp_world::geom;
use wp_world::vesper::ashpeak;

fn main() {
    let (net, report) = ashpeak::generate_with_report(&ashpeak::default_config());
    let names: Vec<String> = report.road_names.iter().map(|n| n.to_string()).collect();
    let target = std::env::var("ROAD").unwrap_or_else(|_| "Loop 9".into());
    let map = wp_world::CityMap::new(net.clone());
    let mut pw = PhysicsWorld::new();
    pw.set_city_ground(&map);
    let filt = |_: ColliderHandle, c: &Collider| c.parent().is_none();
    let qp = pw.query_pipeline(QueryFilter::default().predicate(&filt));
    let mut hits: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    let mut sane = 0;
    for (li, l) in net.lanes.iter().enumerate() {
        let nm = names.get(l.road as usize).cloned().unwrap_or_default();
        if target != "*" && nm != target {
            continue;
        }
        if l.kind == wp_world::network::LaneKind::Parking {
            continue;
        }
        let cum = geom::cumulative(&l.polyline);
        let len = cum[cum.len() - 1];
        let mut s = 0.0;
        while s < len {
            let (p, d) = geom::point_at(&l.polyline, &cum, s);
            let r = geom::right_of(d);
            let side = Ray::new(
                Vector::new(p[0], p[1], p[2] + 0.45),
                Vector::new(-r[0], -r[1], 0.0),
            );
            if qp.cast_ray(&side, 8.0, true).is_some() {
                sane += 1;
            }
            for lat in [-1.6f32, -0.9, 0.0, 0.9, 1.6] {
                for h in [0.25f32, 0.45, 1.0] {
                    let o = Vector::new(p[0] + r[0] * lat, p[1] + r[1] * lat, p[2] + h);
                    let ray = Ray::new(o, Vector::new(d[0], d[1], 0.0));
                    if let Some((ch, hit)) = qp.cast_ray_and_get_normal(&ray, 1.5, true) {
                        let toi = hit.time_of_impact;
                        if hit.normal.z.abs() > 0.6 {
                            continue;
                        }
                        let c = &pw.colliders[ch];
                        let st = format!("{:?}", c.shape().shape_type());
                        if st == "TriMesh" || st == "HeightField" {
                            // ground mesh hit at a slope: check how steep
                        }
                        let a = c.compute_aabb();
                        let key = format!(
                            "{nm} | {st} ud={:x} aabb=({:.0},{:.0},{:.1})-({:.0},{:.0},{:.1})",
                            c.user_data, a.mins.x, a.mins.y, a.mins.z, a.maxs.x, a.maxs.y, a.maxs.z
                        );
                        hits.entry(key).or_default().push(format!(
                            "lane {li} s={s:.0} lat={lat} h={h} at ({:.1},{:.1},{:.1}) toi={toi:.2} n=({:.2},{:.2},{:.2})",
                            p[0], p[1], p[2], hit.normal.x, hit.normal.y, hit.normal.z
                        ));
                    }
                }
            }
            s += 1.0;
        }
    }
    eprintln!(
        "sane {sane} lanes {} names {} target lanes {}",
        net.lanes.len(),
        names.len(),
        net.lanes
            .iter()
            .filter(|l| names.get(l.road as usize).map(|s| s.as_str()) == Some(target.as_str()))
            .count()
    );
    for (k, v) in &hits {
        println!("{k}: {} hits, e.g. {}", v.len(), v[0]);
    }
    // Props on the lanes (cones, parked cars).
    let w = wp_world::World::ashpeak(&ashpeak::default_config(), Default::default());
    for pr in w.static_props() {
        for (li, l) in net.lanes.iter().enumerate() {
            if names.get(l.road as usize).map(|s| s.as_str()) != Some(target.as_str()) {
                continue;
            }
            let cum = geom::cumulative(&l.polyline);
            let pj = geom::project(&l.polyline, &cum, [pr.pos[0], pr.pos[1]]);
            if pj.dist < 0.5 * l.width_m + 0.3 {
                println!(
                    "prop {:?} at {:?} on lane {li} dist {:.2}",
                    pr.kind, pr.pos, pj.dist
                );
            }
        }
    }
}
