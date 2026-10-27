//! What static colliders and props are near a point (PX, PY, PZ env).
use rapier3d::prelude::*;
use wp_sim::PhysicsWorld;
use wp_world::vesper::ashpeak;

fn main() {
    let f = |k: &str| std::env::var(k).unwrap().parse::<f32>().unwrap();
    let p = [f("PX"), f("PY"), f("PZ")];
    let (net, report) = ashpeak::generate_with_report(&ashpeak::default_config());
    let map = wp_world::CityMap::new(net.clone());
    let mut pw = PhysicsWorld::new();
    pw.set_city_ground(&map);
    let qp = pw.query_pipeline(QueryFilter::default());
    let ball = Ball::new(1.5);
    for (h, c) in qp.intersect_shape(Pose::from_translation(Vector::from_array(p)), &ball) {
        let a = c.compute_aabb();
        println!(
            "{h:?} {:?} ud={:x} aabb=({:.1},{:.1},{:.1})-({:.1},{:.1},{:.1})",
            c.shape().shape_type(),
            c.user_data,
            a.mins.x,
            a.mins.y,
            a.mins.z,
            a.maxs.x,
            a.maxs.y,
            a.maxs.z
        );
    }
    let hf = |_: ColliderHandle, c: &Collider| c.shape().as_heightfield().is_some();
    let tm = |_: ColliderHandle, c: &Collider| {
        c.parent().is_none() && c.shape().as_trimesh().is_some() && c.user_data == 0
    };
    let qh = pw.query_pipeline(QueryFilter::default().predicate(&hf));
    let qt = pw.query_pipeline(QueryFilter::default().predicate(&tm));
    let down = |q: &QueryPipeline, x: f32, y: f32| {
        q.cast_ray(
            &Ray::new(Vector::new(x, y, p[2] + 20.0), Vector::new(0.0, 0.0, -1.0)),
            60.0,
            true,
        )
        .map(|(_, t)| p[2] + 20.0 - t)
    };
    println!("grid (2 m): road-mesh z / terrain z; * = terrain above road by >0.15, T = terrain only above p.z-0.5");
    for j in (-10..=10).rev() {
        let mut row = String::new();
        for i in -15..=15 {
            let (x, y) = (p[0] + i as f32 * 2.0, p[1] + j as f32 * 2.0);
            let r = down(&qt, x, y);
            let t = down(&qh, x, y);
            row.push(match (r, t) {
                (Some(r), Some(t)) if t > r + 0.15 => '*',
                (Some(_), _) => '.',
                (None, Some(t)) if t > p[2] - 0.5 => 'T',
                (None, Some(_)) => '_',
                _ => ' ',
            });
        }
        println!("{row}");
    }
    let w = wp_world::World::ashpeak(&ashpeak::default_config(), Default::default());
    for pr in w.static_props() {
        let d = ((pr.pos[0] - p[0]).powi(2) + (pr.pos[1] - p[1]).powi(2)).sqrt();
        if d < 30.0 {
            println!("prop {:?} at {:?} d={d:.1}", pr.kind, pr.pos);
        }
    }
    for (i, l) in net.lanes.iter().enumerate() {
        let cum = wp_world::geom::cumulative(&l.polyline);
        let pj = wp_world::geom::project(&l.polyline, &cum, [p[0], p[1]]);
        if pj.dist < 8.0 {
            println!(
                "lane {i} {:?} road {} {:?} dist {:.1} z {:.1}",
                l.kind,
                l.road,
                report.road_names.get(l.road as usize),
                pj.dist,
                wp_world::geom::point_at(&l.polyline, &cum, pj.s).0[2]
            );
        }
    }
}

