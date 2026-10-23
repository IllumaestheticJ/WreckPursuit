//! Owner playtest row 159: overpasses must not block the road underneath. At every place one
//! road passes over another (an interchange crossroad over Loop 9, the grid's viaduct over its
//! streets, Route 40 under the loop), the lower road's lanes keep clear headroom and their
//! full width, both in the drawn greybox (`build_greybox`) and in the physics colliders
//! (`wp_sim::PhysicsWorld::set_city_ground`).

use city_import::glb::build_greybox;
use rapier3d::prelude::{Collider, QueryFilter, Ray, Vector};
use wp_world::geom::{self, V2, V3};
use wp_world::CityMap;

/// Clear height over every lane of a road that passes under another, m.
const HEADROOM_M: f32 = 4.5;
/// Samples start this far above the lower road (and inside its paved edges), m.
const LIFT_M: f32 = 0.3;
const INSET_M: f32 = 0.8;

/// Where segment a→b meets c→d (parameters along each).
fn crossing(a: V2, b: V2, c: V2, d: V2) -> Option<(f32, f32)> {
    let (r, s) = (geom::sub(b, a), geom::sub(d, c));
    let den = r[0] * s[1] - r[1] * s[0];
    if den.abs() < 1e-6 {
        return None;
    }
    let q = geom::sub(c, a);
    let t = (q[0] * s[1] - q[1] * s[0]) / den;
    let u = (q[0] * r[1] - q[1] * r[0]) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((t, u))
}

/// Distance along ray `o + t d` to triangle `v`, if it hits.
fn ray_triangle(o: V3, d: V3, v: &[V3; 3]) -> Option<f32> {
    let sub = |a: V3, b: V3| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: V3, b: V3| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dot = |a: V3, b: V3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let (e1, e2) = (sub(v[1], v[0]), sub(v[2], v[0]));
    let p = cross(d, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-9 {
        return None;
    }
    let s = sub(o, v[0]);
    let u = dot(s, p) / det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let w = dot(d, q) / det;
    if w < 0.0 || u + w > 1.0 {
        return None;
    }
    let t = dot(e2, q) / det;
    (t > 0.0).then_some(t)
}

fn check(name: &str, map: &CityMap, min_crossings: usize) {
    let net = &map.net;
    let mut tris: Vec<([V3; 3], String)> = Vec::new();
    for p in build_greybox(map) {
        if p.material == "water" {
            continue;
        }
        for t in p.indices.chunks(3) {
            let v = [
                p.positions[t[0] as usize],
                p.positions[t[1] as usize],
                p.positions[t[2] as usize],
            ];
            tris.push((v, p.material.clone()));
        }
    }
    let mut phys = wp_sim::PhysicsWorld::new();
    phys.set_city_ground(map);
    let world_only = |_, c: &Collider| c.parent().is_none();
    let q = phys.query_pipeline(QueryFilter::default().predicate(&world_only));
    let mut crossings = 0;
    let mut failures = Vec::new();
    for (ui, up) in net.surfaces.iter().enumerate() {
        for (li, lo) in net.surfaces.iter().enumerate() {
            if ui == li {
                continue;
            }
            let lcum = geom::cumulative(&lo.centre);
            for a in 0..up.centre.len().saturating_sub(1) {
                for b in 0..lo.centre.len().saturating_sub(1) {
                    let (ua, ub) = (up.centre[a], up.centre[a + 1]);
                    let (la, lb) = (lo.centre[b], lo.centre[b + 1]);
                    let Some((t, u)) =
                        crossing(geom::xy(ua), geom::xy(ub), geom::xy(la), geom::xy(lb))
                    else {
                        continue;
                    };
                    let z_up = ua[2] + (ub[2] - ua[2]) * t;
                    let z_lo = la[2] + (lb[2] - la[2]) * u;
                    if z_up - z_lo < 3.0 {
                        continue; // a junction or a near miss, not a grade separation
                    }
                    crossings += 1;
                    let x = geom::add(
                        geom::xy(la),
                        geom::scale(geom::sub(geom::xy(lb), geom::xy(la)), u),
                    );
                    let dir = geom::normalize(geom::sub(geom::xy(lb), geom::xy(la)));
                    let right = geom::right_of(dir);
                    // Under the upper road and 8 m either side of it.
                    let reach = up.left_m.max(up.right_m) + 8.0;
                    let r = reach + lo.left_m.max(lo.right_m) + 6.0;
                    let near: Vec<&([V3; 3], String)> = tris
                        .iter()
                        .filter(|(v, _)| v.iter().any(|p| geom::dist(geom::xy(*p), x) < r))
                        .collect();
                    let (mut drawn_head, mut drawn_what) = (f32::INFINITY, String::new());
                    let (mut phys_head, mut walls) = (f32::INFINITY, Vec::new());
                    let mut along = -reach;
                    while along <= reach {
                        let mut lat = -lo.left_m + INSET_M;
                        while lat <= lo.right_m - INSET_M {
                            let p = geom::add(
                                x,
                                geom::add(geom::scale(dir, along), geom::scale(right, lat)),
                            );
                            let pr = geom::project(&lo.centre, &lcum, p);
                            let z = geom::point_at(&lo.centre, &lcum, pr.s).0[2] + LIFT_M;
                            let o = [p[0], p[1], z];
                            for (v, m) in &near {
                                if let Some(h) = ray_triangle(o, [0.0, 0.0, 1.0], v) {
                                    if h < drawn_head {
                                        (drawn_head, drawn_what) = (h, m.clone());
                                    }
                                }
                            }
                            let up_ray =
                                Ray::new(Vector::new(o[0], o[1], o[2]), Vector::new(0.0, 0.0, 1.0));
                            if let Some((_, h)) = q.cast_ray(&up_ray, 2.0 * HEADROOM_M, true) {
                                phys_head = phys_head.min(h);
                            }
                            // Nothing standing in the lane over the next metre, low or high.
                            for dz in [1.0f32, HEADROOM_M - 1.0] {
                                let w = [o[0], o[1], o[2] - LIFT_M + dz];
                                let d = [dir[0], dir[1], 0.0];
                                if let Some((_, m)) = near
                                    .iter()
                                    .find(|(v, _)| ray_triangle(w, d, v).is_some_and(|h| h < 1.0))
                                {
                                    walls.push(format!("drawn {m} at ({:.0}, {:.0})", p[0], p[1]));
                                }
                                let wall_ray = Ray::new(
                                    Vector::new(w[0], w[1], w[2]),
                                    Vector::new(d[0], d[1], 0.0),
                                );
                                if q.cast_ray(&wall_ray, 1.0, true).is_some() {
                                    walls.push(format!("physics at ({:.0}, {:.0})", p[0], p[1]));
                                }
                            }
                            lat += 1.0;
                        }
                        along += 1.0;
                    }
                    let head = HEADROOM_M - LIFT_M;
                    if drawn_head < head || phys_head < head || !walls.is_empty() {
                        failures.push(format!(
                            "road {} under road {} at ({:.0}, {:.0}): drawn headroom {:.2} m \
                             ({drawn_what}), physics {:.2} m, obstacles {:?}",
                            lo.road,
                            up.road,
                            x[0],
                            x[1],
                            drawn_head + LIFT_M,
                            phys_head + LIFT_M,
                            &walls[..walls.len().min(3)]
                        ));
                    }
                }
            }
        }
    }
    println!(
        "{name}: {crossings} grade separations checked, {} blocked",
        failures.len()
    );
    assert!(
        crossings >= min_crossings,
        "{name}: only {crossings} crossings found"
    );
    assert!(failures.is_empty(), "{name}:\n{}", failures.join("\n"));
}

#[test]
fn vesper_v2_underpasses_are_clear() {
    let net = wp_world::vesper::generate(&wp_world::vesper::VesperConfig::default());
    check("vesper_v2", &CityMap::new(net), 14);
}

#[test]
fn grid_underpasses_are_clear() {
    let net = wp_world::procgen::generate(&wp_world::procgen::ProcGenConfig::default());
    check("grid", &CityMap::new(net), 16);
}
