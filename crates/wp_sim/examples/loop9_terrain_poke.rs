//! Where the solid terrain heightfield, or any other static surface, stands above Loop 9's road mesh
//! (owner 2026-10-27: "right after one of the bumps ... clipping into the ground", nothing visible).
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
    let hf = |_: ColliderHandle, c: &Collider| c.shape().as_heightfield().is_some();
    let road = |_: ColliderHandle, c: &Collider| {
        c.parent().is_none() && c.shape().as_trimesh().is_some() && c.user_data == 0
    };
    let qh = pw.query_pipeline(QueryFilter::default().predicate(&hf));
    let qr = pw.query_pipeline(QueryFilter::default().predicate(&road));
    let down = |q: &QueryPipeline, x: f32, y: f32, z: f32| {
        q.cast_ray(
            &Ray::new(Vector::new(x, y, z + 3.0), Vector::new(0.0, 0.0, -1.0)),
            6.0,
            true,
        )
        .map(|(_, t)| z + 3.0 - t)
    };
    let mut worst: std::collections::BTreeMap<(usize, i32), (f32, [f32; 3], f32)> =
        Default::default();
    for (li, l) in net.lanes.iter().enumerate() {
        if target != "*" && names.get(l.road as usize).map(|s| s.as_str()) != Some(target.as_str())
        {
            continue;
        }
        let cum = geom::cumulative(&l.polyline);
        let len = cum[cum.len() - 1];
        let mut s = 0.0;
        while s < len {
            let (p, d) = geom::point_at(&l.polyline, &cum, s);
            let r = geom::right_of(d);
            for lat in [-1.5f32, -0.75, 0.0, 0.75, 1.5] {
                let (x, y) = (p[0] + r[0] * lat, p[1] + r[1] * lat);
                let rz = down(&qr, x, y, p[2]);
                let tz = down(&qh, x, y, p[2]);
                if let (Some(rz), Some(tz)) = (rz, tz) {
                    let poke = tz - rz;
                    if poke > 0.02 {
                        let key = (li, (s / 20.0) as i32);
                        let e = worst.entry(key).or_insert((0.0, p, s));
                        if poke > e.0 {
                            *e = (poke, [x, y, rz], s);
                        }
                    }
                }
            }
            s += 0.5;
        }
    }
    for ((li, _), (poke, p, s)) in &worst {
        println!(
            "lane {li} s={s:.1} terrain {poke:.2} m above road at ({:.1},{:.1},{:.2})",
            p[0], p[1], p[2]
        );
    }
    println!("{} spots", worst.len());
    // Road mesh profile: steps and kinks along each lane (what a car feels as a bump).
    let ground =
        |_: ColliderHandle, c: &Collider| c.parent().is_none() && c.user_data != 0x0BA2_21E2;
    let qg = pw.query_pipeline(QueryFilter::default().predicate(&ground));
    for (li, l) in net.lanes.iter().enumerate() {
        if target != "*" && names.get(l.road as usize).map(|s| s.as_str()) != Some(target.as_str())
        {
            continue;
        }
        let cum = geom::cumulative(&l.polyline);
        let len = cum[cum.len() - 1];
        for lat in [-1.0f32, 0.0, 1.0] {
            let mut zs = Vec::new();
            let mut s = 0.0;
            while s < len {
                let (p, d) = geom::point_at(&l.polyline, &cum, s);
                let r = geom::right_of(d);
                let z = down(&qg, p[0] + r[0] * lat, p[1] + r[1] * lat, p[2]);
                zs.push((s, p, z));
                s += 0.25;
            }
            let mut last_rep = -100.0f32;
            for w in zs.windows(3) {
                if let (Some(a), Some(b), Some(c)) = (w[0].2, w[1].2, w[2].2) {
                    let step = (b - a).abs().max((c - b).abs());
                    let kink = (c - 2.0 * b + a).abs();
                    if (step > 0.04 || kink > 0.02) && w[1].0 - last_rep > 5.0 {
                        last_rep = w[1].0;
                        let p = w[1].1;
                        println!("lane {li} lat {lat} s={:.1} at ({:.1},{:.1}) mesh z {a:.3} {b:.3} {c:.3} lane z {:.3}", w[1].0, p[0], p[1], p[2]);
                    }
                } else if w[1].2.is_none() && w[1].0 - last_rep > 5.0 {
                    last_rep = w[1].0;
                    println!(
                        "lane {li} lat {lat} s={:.1} NO GROUND at ({:.1},{:.1})",
                        w[1].0, w[1].1[0], w[1].1[1]
                    );
                }
            }
        }
    }
}
