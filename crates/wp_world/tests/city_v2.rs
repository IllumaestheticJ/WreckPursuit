//! Port Vesper v2 (curves, elevation, terrain): traffic follows the 3D lanes, the soak gates
//! hold on the hilly city, and world queries respect terrain and grade separation.
//! Long runs are `#[ignore]`d: `cargo test -p wp_world --release --test city_v2 -- --ignored --nocapture`.

use std::sync::OnceLock;

use wp_contracts::{SurfaceType, TimeOfDay, WorldQuery, PHYSICS_DT};
use wp_world::network::RoadNetwork;
use wp_world::vesper::{generate, VesperConfig};
use wp_world::{TrafficConfig, World, WorldConfig};

fn net() -> &'static RoadNetwork {
    static NET: OnceLock<RoadNetwork> = OnceLock::new();
    NET.get_or_init(|| generate(&VesperConfig::default()))
}

fn world(seed: u64, aggressive: f32) -> World {
    World::new(
        net().clone(),
        WorldConfig {
            seed,
            start_time: TimeOfDay::from_hours(12.0),
            time_ratio: 1.0,
            traffic: TrafficConfig {
                max_agents: 400,
                density_scale: 4.0,
                detect_collisions: true,
                aggressive_fraction: aggressive,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}

/// Soak: (collisions, collisions with an aggressive driver, min agents, worst z error).
fn soak(seed: u64, minutes: f32, aggressive: f32) -> (u64, u64, usize, f32) {
    let mut w = world(seed, aggressive);
    w.populate();
    let mut ev = Vec::new();
    let mut min_agents = usize::MAX;
    let mut worst_z = 0.0f32;
    let ticks = (minutes * 60.0 / PHYSICS_DT) as usize;
    for i in 0..ticks {
        w.step(PHYSICS_DT, &mut ev);
        if i > 240 * 10 {
            min_agents = min_agents.min(w.agent_count());
        }
        if i % 240 == 0 {
            // Every car rides on the road surface under it (3D lanes, no floating or sinking).
            for a in w.agents() {
                let g = w.map().ground_height([a.pos[0], a.pos[1]], Some(a.pos[2]));
                worst_z = worst_z.max((g - a.pos[2]).abs());
            }
        }
    }
    let s = w.traffic_stats().clone();
    let moving = w.agents().filter(|a| a.speed_mps > 0.5).count();
    println!(
        "vesper seed {seed}, {minutes} min, aggressive {aggressive}: collisions {} (aggr {}), spawned {}, stuck {}, min agents {min_agents}, moving {moving}/{}, worst z {worst_z:.2} m",
        s.civilian_collisions,
        s.collisions_involving_aggressive,
        s.spawned,
        s.stuck_recycled,
        w.agent_count()
    );
    assert!(
        moving * 2 > w.agent_count(),
        "traffic is jammed: only {moving} moving"
    );
    (
        s.civilian_collisions,
        s.collisions_involving_aggressive,
        min_agents,
        worst_z,
    )
}

#[test]
fn traffic_400_agents_one_minute_on_the_hills_without_collisions() {
    let (c, _, n, z) = soak(1, 1.0, 0.0);
    assert_eq!(c, 0, "civilian-civilian collisions");
    assert!(n >= 380, "population collapsed to {n}");
    assert!(z < 0.3, "an agent is {z} m off the road surface");
}

#[test]
#[ignore = "10 minutes of sim per seed; run with --release"]
fn traffic_soak_ten_minutes_three_seeds() {
    for seed in [1, 2, 3] {
        let (c, _, n, _) = soak(seed, 10.0, 0.0);
        assert_eq!(c, 0, "seed {seed}: civilian-civilian collisions");
        assert!(n >= 380);
        let (c, ca, _, _) = soak(seed, 10.0, 0.05);
        assert!(
            c <= 4 && c == ca,
            "seed {seed}: {c} collisions with aggressive drivers ({ca})"
        );
    }
}

#[test]
fn terrain_blocks_line_of_sight_across_the_hill() {
    let w = world(1, 0.0);
    // From the Loop's west leg to downtown, straight through Hollins Heights.
    assert!(!w
        .map()
        .line_of_sight([-1500.0, 1050.0, 30.0], [-500.0, 1050.0, 30.0]));
    // A helicopter over the knob sees both sides.
    assert!(w
        .map()
        .line_of_sight([-1000.0, 1080.0, 260.0], [-500.0, 1050.0, 30.0]));
}

#[test]
fn queries_respect_grade_separation_and_terrain() {
    let w = world(1, 0.0);
    let map = w.map();
    // Route 40 West passes under the Loop near x = -1517, y = 91 (probe the inner
    // carriageway, 7 m east of the median).
    let p = [-1510.0, 92.0];
    let low = map.ground_height(p, Some(-100.0));
    let high = map.ground_height(p, Some(1000.0));
    assert!(high - low > 6.5, "deck {high} over road {low}");
    // The lower road is asphalt, the deck above it highway concrete.
    assert_eq!(map.surface_at([p[0], p[1], low]), SurfaceType::Asphalt);
    assert_eq!(map.surface_at([p[0], p[1], high]), SurfaceType::Concrete);
    // Off-road ground height comes from the terrain.
    let knob = map.ground_height([-1000.0, 1000.0], None);
    assert!(knob > 80.0, "Hollins Knob at {knob} m");
}
/// The drawn terrain (`surface::drawn_terrain`, the greybox and off-road physics) never rises
/// above a paved road, and off the roads it follows the heightfield instead of sinking to the
/// lowest ground nearby (playtest: dips drawn 1-2 m deep behind lots).
#[test]
fn drawn_terrain_stays_under_roads_and_follows_the_ground() {
    let map = wp_world::CityMap::new(net().clone());
    let raw = net().terrain.as_ref().unwrap();
    let drawn = wp_world::surface::drawn_terrain(&map).unwrap();
    let solid = map.ground.as_ref().unwrap();
    // Roads: every triangle corner, edge midpoint and centroid.
    let mesh = wp_world::surface::road_mesh(&map);
    let mut worst_road = f32::NEG_INFINITY;
    for tri in &mesh.indices {
        let p = tri.map(|i| mesh.positions[i as usize]);
        let mid = |a: [f32; 3], b: [f32; 3], w: f32| [0, 1, 2].map(|c| a[c] + (b[c] - a[c]) * w);
        let c = mid(mid(p[0], p[1], 0.5), p[2], 1.0 / 3.0);
        for q in [
            p[0],
            p[1],
            p[2],
            mid(p[0], p[1], 0.5),
            mid(p[1], p[2], 0.5),
            mid(p[2], p[0], 0.5),
            c,
        ] {
            worst_road = worst_road.max(drawn.height_on_triangles([q[0], q[1]]) - q[2]);
        }
    }
    // Off the roads (where solid = drawn): how far below the heightfield the drawn ground is.
    let mut gaps = Vec::new();
    for j in 0..raw.ny - 1 {
        for i in 0..raw.nx - 1 {
            let p = raw.vertex_pos(i, j);
            let p = [p[0] + 1.7, p[1] + 2.9];
            let d = drawn.height_on_triangles(p);
            if raw.height_at(p) > 0.5 && (solid.height_on_triangles(p) - d).abs() < 1e-3 {
                gaps.push(raw.height_at(p) - d);
            }
        }
    }
    gaps.sort_by(|a, b| a.total_cmp(b));
    let q = |f: f32| gaps[((gaps.len() - 1) as f32 * f) as usize];
    println!(
        "drawn terrain: worst {worst_road:.3} m above a road; off-road {} samples, heightfield − drawn p50 {:.2} p95 {:.2} p99 {:.2} min {:.2} max {:.2} m",
        gaps.len(), q(0.5), q(0.95), q(0.99), gaps[0], gaps[gaps.len() - 1]
    );
    assert!(
        worst_road <= 0.0,
        "drawn terrain {worst_road} m above a road"
    );
    assert!(gaps.len() > 100_000);
    // Measured: p50 0.05 (the 5 cm offset), p95 0.19, p99 0.51 m; the old "lowest ground
    // within 10 m" mesh had ~10 % of these samples more than 1 m low (up to 10.7 m).
    assert!(q(0.95) < 0.3, "p95 {}", q(0.95));
    assert!(q(0.99) < 0.75, "p99 {}", q(0.99));
}
