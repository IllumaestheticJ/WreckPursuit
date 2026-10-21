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
