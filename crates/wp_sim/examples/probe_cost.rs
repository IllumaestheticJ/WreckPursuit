//! Cost of one wheel ground probe in Port Vesper, by probe kind (release):
//! `cargo run --release -p wp_sim --example probe_cost -- [points]`.
//!
//! Builds the city's static colliders exactly as `Sim` does (`PhysicsWorld::set_city_ground`),
//! samples wheel positions along every lane (centre and both lane edges, so kerbs and junction
//! steps are included), and times each probe kind over the same points: the centre ray (the
//! pre-round-12b probe), the ray + Rapier GJK cylinder sweep (round 12b) and
//! `RapierGround::cast_wheel` (ray + the closed-form sweep). Also reports how often the sweep
//! changes the result (the envelope lifts the wheel above the ray) and how far the closed-form
//! sweep is from the GJK one. Numbers quoted in `docs/integration/sim-perf.md`.

use std::hint::black_box;
use std::time::Instant;

use rapier3d::prelude::QueryFilter;
use wp_sim::PhysicsWorld;
use wp_vehicle::ground::blend_envelope;
use wp_vehicle::math::{Quat, V3};
use wp_vehicle::{RapierGround, WheelCaster, WheelProbe};

fn main() {
    let max_points: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20_000);
    let world = wp_world::World::vesper(
        &wp_world::vesper::VesperConfig {
            seed: 1,
            ..Default::default()
        },
        Default::default(),
    );
    let mut physics = PhysicsWorld::new();
    physics.set_city_ground(world.map());

    // A mid-size tyre (sports/sedan data): 0.32 m radius, 0.2 m wide (0.8 of it swept), the
    // sweep starting 0.3 m above the top of 0.16 m travel, as `Vehicle::probe_wheel` builds it.
    let (radius, half_width, travel, lift) = (0.32_f32, 0.1 * 0.8, 0.16_f32, 0.3_f32);
    let mut probes: Vec<WheelProbe> = Vec::new();
    'lanes: for lane in &world.network().lanes {
        for w in lane.polyline.windows(2) {
            let (a, b) = (V3::from_array(w[0]), V3::from_array(w[1]));
            let d = b - a;
            let len = (d.x * d.x + d.y * d.y).sqrt();
            if len < 0.5 {
                continue;
            }
            let fwd = V3::new(d.x / len, d.y / len, 0.0);
            let right = V3::new(fwd.y, -fwd.x, 0.0);
            let yaw = wp_math::atan2(-fwd.x, fwd.y);
            let steps = (len / 2.0).ceil() as usize;
            for k in 0..steps {
                let p = a + d * (k as f32 / steps as f32);
                for off in [0.0, 0.5 * lane.width_m - 0.3, -0.5 * lane.width_m + 0.3] {
                    let xy = p + right * off;
                    let Some(gz) = physics.static_ground_z([xy.x, xy.y], p.z + 3.0, 6.0) else {
                        continue;
                    };
                    // The wheel at rest (6 cm static compression): the top of its travel is
                    // `travel − 0.06` above the resting hub; the sweep starts `lift` above that.
                    let top = gz + radius + (travel - 0.06);
                    probes.push(WheelProbe {
                        center: V3::new(xy.x, xy.y, top + lift),
                        dir: V3::new(0.0, 0.0, -1.0),
                        max_dist: travel + lift,
                        radius,
                        half_width,
                        rot: Quat::from_rotation_z(yaw - std::f32::consts::FRAC_PI_2),
                    });
                    if probes.len() >= max_points {
                        break 'lanes;
                    }
                }
            }
        }
    }
    let n = probes.len();
    println!("{n} wheel probes on Port Vesper lanes (centre + both lane edges)");

    let ground = RapierGround {
        query: physics.query_pipeline(QueryFilter::default()),
    };
    let time = |name: &str, f: &dyn Fn(&WheelProbe) -> Option<f32>| -> Vec<Option<f32>> {
        let mut out = Vec::with_capacity(n);
        // Warm-up, then best of 3 (this container is shared).
        for p in &probes[..n.min(1000)] {
            black_box(f(p));
        }
        let mut best = f64::MAX;
        for _ in 0..3 {
            out.clear();
            let t = Instant::now();
            for p in &probes {
                out.push(black_box(f(p)));
            }
            best = best.min(t.elapsed().as_secs_f64());
        }
        println!(
            "  {name:34} {:6.2} µs/probe  ({:.3} ms per car-frame: 4 wheels × 4 ticks)",
            best * 1e6 / n as f64,
            best * 1e3 / n as f64 * 16.0
        );
        out
    };
    let ray = time("centre ray (pre-12b probe)", &|p| {
        ground
            .cast(p.center, p.dir, p.max_dist + p.radius)
            .map(|h| h.distance - p.radius)
    });
    let ray_hit = |p: &WheelProbe| {
        ground
            .cast(p.center, p.dir, p.max_dist + p.radius)
            .map(|h| wp_vehicle::GroundHit {
                distance: h.distance - p.radius,
                ..h
            })
    };
    let gjk = time("ray + GJK cylinder sweep (12b)", &|p| {
        blend_envelope(ray_hit(p), ground.sweep_tyre_gjk(p)).map(|h| h.distance)
    });
    let exact = time("cast_wheel (ray + exact sweep)", &|p| {
        ground.cast_wheel(p).map(|h| h.distance)
    });
    let sweep_gjk: Vec<Option<f32>> = probes
        .iter()
        .map(|p| ground.sweep_tyre_gjk(p).map(|h| h.distance))
        .collect();
    let sweep_exact: Vec<Option<f32>> = probes
        .iter()
        .map(|p| ground.sweep_tyre(p).map(|h| h.distance))
        .collect();
    let mut lifted = 0;
    let mut max_lift = 0.0f32;
    for (r, w) in ray.iter().zip(&exact) {
        if let (Some(r), Some(w)) = (r, w) {
            if r - w > wp_vehicle::ground::ENVELOPE_BLEND_M {
                lifted += 1;
                max_lift = max_lift.max(r - w);
            }
        }
    }
    println!(
        "  sweep lifts the wheel > 1.5 mm above the ray on {lifted} of {n} probes ({:.1} %), max {:.1} cm",
        100.0 * lifted as f64 / n as f64,
        max_lift * 100.0
    );
    let compare = |name: &str, a: &[Option<f32>], b: &[Option<f32>]| {
        let (mut only, mut max_d, mut sum_d, mut both, mut over_1mm) = (0, 0.0f32, 0.0f64, 0, 0);
        for (x, y) in a.iter().zip(b) {
            match (x, y) {
                (Some(x), Some(y)) => {
                    let d = (x - y).abs();
                    both += 1;
                    sum_d += d as f64;
                    max_d = max_d.max(d);
                    if d > 0.001 {
                        over_1mm += 1;
                    }
                }
                (None, None) => {}
                _ => only += 1,
            }
        }
        println!(
            "  {name}: {both} both hit, mean |Δ| {:.3} mm, max {:.2} mm, {over_1mm} over 1 mm; \
             {only} hit by only one",
            sum_d / both.max(1) as f64 * 1e3,
            max_d * 1e3
        );
    };
    let same_ray = probes
        .iter()
        .filter(|p| ground.probe(p).0 == ray_hit(p))
        .count();
    println!(
        "  centre ray from the shared pass identical to the plain ray cast on {same_ray} of {n}"
    );
    compare("sweep alone, exact vs GJK", &sweep_exact, &sweep_gjk);
    compare("wheel travel (ray + sweep), exact vs GJK", &exact, &gjk);
}
