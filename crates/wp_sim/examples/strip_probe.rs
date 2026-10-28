//! Road surface heights along a straight strip (AX AY -> BX BY, Z near), every 0.5 m, at offsets
//! across: what physics stands on, and which collider kind.
use rapier3d::prelude::*;
use wp_sim::PhysicsWorld;
use wp_world::vesper::ashpeak;

fn main() {
    let f = |k: &str| std::env::var(k).unwrap().parse::<f32>().unwrap();
    let (a, b, z) = ([f("AX"), f("AY")], [f("BX"), f("BY")], f("Z"));
    let net = ashpeak::generate(&ashpeak::default_config());
    let map = wp_world::CityMap::new(net);
    let mut pw = PhysicsWorld::new();
    pw.set_city_ground(&map);
    let world_only = |_: ColliderHandle, c: &Collider| c.parent().is_none();
    let qp = pw.query_pipeline(QueryFilter::default().predicate(&world_only));
    let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
    let d = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
    let r = [d[1], -d[0]];
    let mut s = 0.0;
    while s <= len {
        let mut row = format!("s={s:6.1}");
        for lat in [-5.0f32, -3.0, -1.0, 1.0, 3.0, 5.0] {
            let (x, y) = (a[0] + d[0] * s + r[0] * lat, a[1] + d[1] * s + r[1] * lat);
            let ray = Ray::new(Vector::new(x, y, z + 6.0), Vector::new(0.0, 0.0, -1.0));
            match qp.cast_ray(&ray, 30.0, true) {
                Some((h, t)) => {
                    let c = &pw.colliders[h];
                    let k = match c.shape().shape_type() {
                        ShapeType::TriMesh if c.user_data != 0 => 'B',
                        ShapeType::TriMesh => 'r',
                        ShapeType::HeightField => 't',
                        _ => 'o',
                    };
                    row += &format!("  {k}{:7.2}", z + 6.0 - t);
                }
                None => row += "      none",
            }
        }
        println!("{row}");
        s += 0.5;
    }
}
