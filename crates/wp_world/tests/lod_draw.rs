//! Traffic draw distance on Port Vesper v2 (lead helper, playtest row 25 "LOD pop-in"):
//! a player at highway speed on Loop 9 must never see a car appear or vanish close by. New
//! agents spawn, and old ones despawn, at least `sight_radius_m` from the player, so that radius
//! is the distance at which cars pop in. The browser draws every agent (instanced far LOD), so
//! this is the whole draw-distance rule.
//!
//! `cargo test -p wp_world --release --test lod_draw -- --nocapture` prints the measurements.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use wp_contracts::{EntityId, TimeOfDay, PHYSICS_DT};
use wp_world::network::RoadNetwork;
use wp_world::vesper::{generate, VesperConfig};
use wp_world::{TrackedVehicle, TrafficConfig, World, WorldConfig};

fn net() -> &'static RoadNetwork {
    static NET: OnceLock<RoadNetwork> = OnceLock::new();
    NET.get_or_init(|| generate(&VesperConfig::default()))
}

/// What a highway-speed drive saw.
#[derive(Debug, Default)]
struct DriveLog {
    /// Closest distance from the player at which an agent appeared, m.
    min_spawn_m: f32,
    /// Closest distance from the player at which an agent disappeared, m.
    min_despawn_m: f32,
    /// Mean number of agents within 300 m of the player.
    mean_within_300: f32,
    /// Mean number of agents within 150 m of the player.
    mean_within_150: f32,
    /// Mean agent count.
    mean_agents: f32,
    /// Agents that appeared while the player drove.
    spawns: usize,
    /// Player's mean speed, m/s.
    mean_speed: f32,
}

/// The player rides along with the fastest highway agent (a stand-in for a player on Loop 9:
/// same lanes, same speeds) for `seconds`, starting after a 20 s warm-up.
fn highway_drive_seed(cfg: TrafficConfig, seconds: f32, seed: u64) -> DriveLog {
    let mut w = World::new(
        net().clone(),
        WorldConfig {
            seed,
            start_time: TimeOfDay::from_hours(17.0),
            time_ratio: 1.0,
            traffic: cfg,
            ..Default::default()
        },
    );
    let mut ev = Vec::new();
    // Start on the race grid area (Loop 9) like the city chase.
    let mut player = [0.0f32, 0.0, 0.0];
    let mut vel = [0.0f32, 0.0, 0.0];
    let track = |w: &mut World, p: [f32; 3], v: [f32; 3]| {
        let s = (v[0] * v[0] + v[1] * v[1]).sqrt().max(1e-3);
        w.set_tracked_vehicles(&[TrackedVehicle {
            entity: EntityId(1),
            pos: p,
            vel: v,
            forward: [v[0] / s, v[1] / s],
            siren: false,
            player: true,
        }]);
    };
    track(&mut w, player, vel);
    w.populate();
    let mut log = DriveLog {
        min_spawn_m: f32::INFINITY,
        min_despawn_m: f32::INFINITY,
        ..Default::default()
    };
    let mut host: Option<u32> = None;
    let mut last: BTreeMap<u32, [f32; 3]> = BTreeMap::new();
    let warm = (20.0 / PHYSICS_DT) as usize;
    let ticks = warm + (seconds / PHYSICS_DT) as usize;
    let mut samples = 0usize;
    for i in 0..ticks {
        // Follow the host agent; pick the fastest agent when it is gone or too slow.
        let agents: Vec<_> = w.agents().collect();
        let h = host.and_then(|id| agents.iter().find(|a| a.id == id));
        let h = match h {
            Some(a) if a.speed_mps > 15.0 || i < warm => *a,
            _ => {
                let best = agents
                    .iter()
                    .max_by(|a, b| a.speed_mps.total_cmp(&b.speed_mps).then(b.id.cmp(&a.id)))
                    .copied()
                    .expect("no traffic");
                host = Some(best.id);
                best
            }
        };
        let (s, c) = (h.yaw.sin(), h.yaw.cos());
        vel = [-s * h.speed_mps, c * h.speed_mps, 0.0];
        // Ride 12 m behind the host so it does not count as "the player".
        player = [h.pos[0] + s * 12.0, h.pos[1] - c * 12.0, h.pos[2]];
        track(&mut w, player, vel);
        w.step(PHYSICS_DT, &mut ev);
        let d = |p: [f32; 3]| ((p[0] - player[0]).powi(2) + (p[1] - player[1]).powi(2)).sqrt();
        let now: BTreeMap<u32, [f32; 3]> = w.agents().map(|a| (a.id, a.pos)).collect();
        if i >= warm {
            for (id, p) in &now {
                if !last.contains_key(id) && Some(*id) != host {
                    log.min_spawn_m = log.min_spawn_m.min(d(*p));
                    log.spawns += 1;
                }
            }
            for (id, p) in &last {
                if !now.contains_key(id) && Some(*id) != host {
                    log.min_despawn_m = log.min_despawn_m.min(d(*p));
                }
            }
            if i % 24 == 0 {
                samples += 1;
                log.mean_within_300 += now.values().filter(|p| d(**p) < 300.0).count() as f32;
                log.mean_within_150 += now.values().filter(|p| d(**p) < 150.0).count() as f32;
                log.mean_agents += now.len() as f32;
                log.mean_speed += h.speed_mps;
            }
        }
        last = now;
    }
    let n = samples.max(1) as f32;
    log.mean_within_300 /= n;
    log.mean_within_150 /= n;
    log.mean_agents /= n;
    log.mean_speed /= n;
    log
}

#[test]
fn highway_player_never_sees_traffic_pop_in_closer_than_300_m() {
    let old = TrafficConfig {
        sight_radius_m: 150.0,
        active_radius_m: 400.0,
        ..TrafficConfig::default()
    };
    let (mut near_new, mut near_old) = (0.0, 0.0);
    for seed in [1u64, 7, 23, 42, 99] {
        let log = highway_drive_seed(TrafficConfig::default(), 60.0, seed);
        println!("seed {seed}, default config: {log:?}");
        assert!(log.mean_speed > 15.0, "player too slow: {log:?}");
        assert!(log.spawns > 0, "nothing spawned: {log:?}");
        assert!(
            log.min_spawn_m >= 300.0,
            "a car appeared {:.0} m from the player",
            log.min_spawn_m
        );
        assert!(
            log.min_despawn_m >= 300.0,
            "a car vanished {:.0} m from the player",
            log.min_despawn_m
        );
        near_new += log.mean_within_150;
        near_old += highway_drive_seed(old.clone(), 60.0, seed).mean_within_150;
    }
    // The streets around the player stay about as busy as with the old radii.
    println!("cars within 150 m (sum of 5 seeds): new {near_new:.1}, old {near_old:.1}");
    assert!(near_new >= 0.6 * near_old, "new {near_new} vs old {near_old}");
}

/// Radii comparison over five seeds (mean agents, agents near the player, spawn churn):
/// `cargo test -p wp_world --release --test lod_draw -- --ignored --nocapture`.
#[test]
#[ignore]
fn compare_radii() {
    let d = TrafficConfig::default();
    let candidates = [
        (150.0, 400.0),
        (d.sight_radius_m, d.active_radius_m),
        (320.0, 480.0),
        (340.0, 600.0),
    ];
    for (sr, ar) in candidates {
        let mut acc = [0.0f32; 4];
        for seed in [1u64, 7, 23, 42, 99] {
            let c = TrafficConfig {
                sight_radius_m: sr,
                active_radius_m: ar,
                ..TrafficConfig::default()
            };
            let l = highway_drive_seed(c, 60.0, seed);
            acc[0] += l.mean_agents / 5.0;
            acc[1] += l.mean_within_150 / 5.0;
            acc[2] += l.mean_within_300 / 5.0;
            acc[3] += l.spawns as f32 / 5.0;
        }
        println!(
            "sight {sr} m, active {ar} m: agents {:.0}, within 150 m {:.1}, within 300 m {:.1}, spawns/min {:.0}",
            acc[0], acc[1], acc[2], acc[3]
        );
    }
}
