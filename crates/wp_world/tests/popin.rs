//! Traffic pop-in in Free Roam (lead/car-popin, owner report "cars also render in free roam too
//! close"): on the combined world (Port Vesper + Ashpeak County), a player driving with highway or
//! city traffic must never see a traffic car appear or vanish. The rule
//! (`TrafficConfig::sight_radius_m` + `view_radius_m`):
//!
//! * no agent spawns within N = `sight_radius_m` (320 m) of the player, in any direction (mirrors,
//!   rear and orbit cameras), and
//! * none spawns within `view_radius_m` (1000 m, the distance at which the browser's thinnest
//!   daytime fog hides ~92 % of a car and it is ~3 px at 720p) where a chase camera has a clear
//!   line of sight to it ([`World::line_of_sight`]: terrain, buildings, decks, tunnels). The
//!   camera frustum is inside that test (line of sight ignores direction), so "not in the
//!   frustum at a reasonable distance" holds a fortiori.
//!
//! The cameras here are the testbed's near and far chase cameras placed independently of the
//! traffic code's own constants, at the player's position after the step (not the one the
//! traffic step used), so the check is stricter than the rule it tests.
//!
//! Parked cars are all placed when the world is built (whole map), never later
//! (`parked_cars_never_appear_during_play`).
//!
//! `cargo test -p wp_world --release --test popin -- --nocapture` prints the measurements.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use wp_contracts::{EntityId, TimeOfDay, WorldQuery, PHYSICS_DT};
use wp_world::network::RoadNetwork;
use wp_world::{ashpeak, TrackedVehicle, TrafficConfig, World, WorldConfig};

fn net() -> &'static RoadNetwork {
    static NET: OnceLock<RoadNetwork> = OnceLock::new();
    NET.get_or_init(|| ashpeak::generate_with_report(&ashpeak::default_config()).0)
}

/// Chase cameras `[back, up]` from the player's position, m: the testbed's near chase
/// (`cameras.ts` NEAR_CHASE 5.6 m back, 1.8 m up) and far chase (8.5 m + 1.5 m pull-back, 2.9 m up).
const CAMS: [[f32; 2]; 2] = [[5.6, 1.8], [10.0, 2.9]];
/// Points of a car a camera must not see appear: mid body and roof, m above the road.
const BODY_UP: [f32; 2] = [0.8, 1.4];

/// Who the player rides with.
#[derive(Clone, Copy, Debug)]
enum Ride {
    /// The fastest car (highways and arterials).
    Highway,
    /// A car at town speed.
    City,
}

/// What a drive saw.
#[derive(Debug)]
struct DriveLog {
    spawns: usize,
    /// Spawns a chase camera had a line of sight to, within `view_radius_m`.
    spawns_in_view: usize,
    /// Closest distance from the player at which a car appeared, m.
    min_spawn_m: f32,
    despawns: usize,
    /// Despawns a chase camera had a line of sight to, within `view_radius_m`.
    despawns_in_view: usize,
    /// Mean number of cars within 150 m of the player.
    mean_within_150: f32,
    /// Mean number of cars a chase camera can see within `view_radius_m`.
    mean_in_view: f32,
}

/// The player rides 12 m behind a host car for `seconds` after a 10 s warm-up; every traffic
/// step (4 physics ticks) checks which agents appeared and vanished.
fn drive(cfg: TrafficConfig, seed: u64, hour: f32, ride: Ride, seconds: f32) -> DriveLog {
    let view_m = TrafficConfig::default().view_radius_m;
    let mut w = World::new(
        net().clone(),
        WorldConfig {
            seed,
            start_time: TimeOfDay::from_hours(hour),
            time_ratio: 1.0,
            traffic: cfg,
            ..Default::default()
        },
    );
    let track = |w: &mut World, p: [f32; 3], v: [f32; 3], f: [f32; 2]| {
        w.set_tracked_vehicles(&[TrackedVehicle {
            entity: EntityId(1),
            pos: p,
            vel: v,
            forward: f,
            siren: false,
            player: true,
        }]);
    };
    track(&mut w, [0.0; 3], [0.0; 3], [0.0, 1.0]);
    w.populate();
    let mut log = DriveLog {
        spawns: 0,
        spawns_in_view: 0,
        min_spawn_m: f32::INFINITY,
        despawns: 0,
        despawns_in_view: 0,
        mean_within_150: 0.0,
        mean_in_view: 0.0,
    };
    let mut ev = Vec::new();
    let mut host: Option<u32> = None;
    let mut last: BTreeMap<u32, [f32; 3]> = BTreeMap::new();
    let warm = (10.0 / PHYSICS_DT) as usize;
    let ticks = warm + (seconds / PHYSICS_DT) as usize;
    let mut samples = 0usize;
    let (mut player, mut fwd) = ([0.0f32; 3], [0.0f32, 1.0]);
    for i in 0..ticks {
        if i % 4 == 0 {
            let agents: Vec<_> = w.agents().collect();
            let keep = |speed: f32| match ride {
                Ride::Highway => speed > 15.0,
                Ride::City => speed > 2.0,
            };
            let h = match host.and_then(|id| agents.iter().find(|a| a.id == id)) {
                Some(a) if keep(a.speed_mps) || i < warm => *a,
                _ => {
                    let pick = match ride {
                        Ride::Highway => agents.iter().max_by(|a, b| {
                            a.speed_mps.total_cmp(&b.speed_mps).then(b.id.cmp(&a.id))
                        }),
                        Ride::City => agents
                            .iter()
                            .filter(|a| a.speed_mps > 5.0 && a.speed_mps < 16.0)
                            .min_by_key(|a| a.id),
                    };
                    let b = *pick.or(agents.first()).expect("no traffic");
                    host = Some(b.id);
                    b
                }
            };
            let (s, c) = (h.yaw.sin(), h.yaw.cos());
            fwd = [-s, c];
            player = [h.pos[0] + s * 12.0, h.pos[1] - c * 12.0, h.pos[2]];
            let v = [-s * h.speed_mps, c * h.speed_mps, 0.0];
            track(&mut w, player, v, fwd);
        }
        w.step(PHYSICS_DT, &mut ev);
        if i % 4 != 3 {
            continue;
        }
        let now: BTreeMap<u32, [f32; 3]> = w.agents().map(|a| (a.id, a.pos)).collect();
        let d = |p: &[f32; 3]| ((p[0] - player[0]).powi(2) + (p[1] - player[1]).powi(2)).sqrt();
        let seen = |p: &[f32; 3]| {
            d(p) < view_m
                && CAMS.iter().any(|[back, up]| {
                    let cam = [
                        player[0] - fwd[0] * back,
                        player[1] - fwd[1] * back,
                        player[2] + up,
                    ];
                    BODY_UP
                        .iter()
                        .any(|z| w.line_of_sight(cam, [p[0], p[1], p[2] + z]))
                })
        };
        if i >= warm {
            for (id, p) in &now {
                if !last.contains_key(id) && Some(*id) != host {
                    log.spawns += 1;
                    log.min_spawn_m = log.min_spawn_m.min(d(p));
                    if seen(p) {
                        log.spawns_in_view += 1;
                    }
                }
            }
            for (id, p) in &last {
                if !now.contains_key(id) && Some(*id) != host {
                    log.despawns += 1;
                    if seen(p) {
                        log.despawns_in_view += 1;
                    }
                }
            }
            if i % 96 == 3 {
                samples += 1;
                log.mean_within_150 += now.values().filter(|p| d(p) < 150.0).count() as f32;
                log.mean_in_view += now.values().filter(|p| seen(p)).count() as f32;
            }
        }
        last = now;
    }
    let n = samples.max(1) as f32;
    log.mean_within_150 /= n;
    log.mean_in_view /= n;
    log
}

/// Drives: (ride, hour, seed).
const DRIVES: [(Ride, f32, u64); 6] = [
    (Ride::Highway, 13.0, 1),
    (Ride::Highway, 13.0, 7),
    (Ride::City, 13.0, 1),
    (Ride::City, 13.0, 7),
    (Ride::Highway, 22.0, 1),
    (Ride::Highway, 22.0, 7),
];

#[test]
fn free_roam_traffic_never_appears_where_the_player_can_see_it() {
    let cfg = TrafficConfig::default();
    let n_m = cfg.sight_radius_m;
    let (mut spawns, mut despawns, mut despawns_in_view) = (0, 0, 0);
    for (ride, hour, seed) in DRIVES {
        let log = drive(cfg.clone(), seed, hour, ride, 40.0);
        println!("{ride:?} {hour}:00 seed {seed}: {log:?}");
        assert!(log.spawns > 0, "nothing spawned: {log:?}");
        assert!(
            log.min_spawn_m >= n_m,
            "{ride:?} {hour}:00 seed {seed}: a car appeared {:.0} m from the player (N = {n_m} m)",
            log.min_spawn_m
        );
        assert_eq!(
            log.spawns_in_view, 0,
            "{ride:?} {hour}:00 seed {seed}: cars appeared in a chase camera's line of sight \
             within {} m: {log:?}",
            cfg.view_radius_m
        );
        spawns += log.spawns;
        despawns += log.despawns;
        despawns_in_view += log.despawns_in_view;
    }
    // Despawns obey the same veto; the only exception is a car that reaches the end of a road
    // with no way on (a dead end at the map edge), which is rare.
    println!("spawns {spawns}, despawns {despawns} (in view: {despawns_in_view})");
    assert!(
        despawns_in_view * 100 <= despawns,
        "{despawns_in_view} of {despawns} despawns were in view"
    );
}

#[test]
fn line_of_sight_veto_keeps_the_streets_busy() {
    // The old rule (radius only) as the reference: the veto must not empty the player's
    // surroundings (it moves spawns behind buildings and crests rather than stopping them).
    let old = TrafficConfig {
        view_radius_m: 0.0,
        ..TrafficConfig::default()
    };
    let (mut near_new, mut near_old, mut view_new, mut view_old) = (0.0, 0.0, 0.0, 0.0);
    for (ride, hour, seed) in DRIVES {
        let new = drive(TrafficConfig::default(), seed, hour, ride, 40.0);
        let was = drive(old.clone(), seed, hour, ride, 40.0);
        near_new += new.mean_within_150;
        near_old += was.mean_within_150;
        view_new += new.mean_in_view;
        view_old += was.mean_in_view;
    }
    println!(
        "cars within 150 m (sum of drives): new {near_new:.1}, old {near_old:.1}; \
         cars in view: new {view_new:.1}, old {view_old:.1}"
    );
    assert!(
        near_new >= 0.7 * near_old,
        "new {near_new} vs old {near_old}"
    );
    assert!(
        view_new >= 0.7 * view_old,
        "new {view_new} vs old {view_old}"
    );
}

#[test]
fn parked_cars_never_appear_during_play() {
    // Every parked car is placed when the world is built, over the whole map; driving
    // around never adds, removes or moves one (the browser draws all of them, render/props.ts).
    let mut w = World::new(net().clone(), WorldConfig::default());
    let before: Vec<_> = w.static_props().copied().collect();
    assert!(before.len() > 100, "{} props", before.len());
    let mut ev = Vec::new();
    for k in 0..(20.0 / PHYSICS_DT) as usize {
        let x = k as f32 * PHYSICS_DT * 30.0;
        w.set_tracked_vehicles(&[TrackedVehicle {
            entity: EntityId(1),
            pos: [x, 0.0, 0.0],
            vel: [30.0, 0.0, 0.0],
            forward: [1.0, 0.0],
            siren: false,
            player: true,
        }]);
        w.step(PHYSICS_DT, &mut ev);
    }
    let after: Vec<_> = w.static_props().copied().collect();
    assert_eq!(before, after);
}

/// Classify the browser's pop-ins (`apps/testbed-web/e2e/popin_shots.mjs` writes
/// `docs/media/popin/<label>_summary.json`): the script only knows a car was inside the camera's
/// view cone; this checks the line of sight from the recorded camera position to the car on
/// the same map (mid body or roof), so a car that appeared behind a building is told apart from
/// one that appeared in plain view. Writes `<label>_los.json` (per shot, one bool per event:
/// in plain view) for `e2e/popin_compose.mjs` and prints the counts.
/// `cargo test -p wp_world --release --test popin -- --ignored --nocapture`.
#[test]
#[ignore]
fn classify_browser_popins() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/media/popin");
    let w = World::new(net().clone(), WorldConfig::default());
    for label in ["before", "after"] {
        let Ok(text) = std::fs::read_to_string(format!("{dir}/{label}_summary.json")) else {
            continue;
        };
        let v: serde_json::Value = serde_json::from_str(&text).expect("summary json");
        let mut los = serde_json::Map::new();
        for shot in v["shots"].as_array().into_iter().flatten() {
            let id = shot["id"].as_str().unwrap_or("?");
            let classify = |key: &str| -> Vec<bool> {
                let f = |x: &serde_json::Value, i: usize| x[i].as_f64().unwrap_or(0.0) as f32;
                shot[key]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|e| {
                        let (p, c) = (&e["pos"], &e["cam"]);
                        let cam = [f(c, 0), f(c, 1), f(c, 2)];
                        BODY_UP
                            .iter()
                            .any(|z| w.line_of_sight(cam, [f(p, 0), f(p, 1), f(p, 2) + z]))
                    })
                    .collect()
            };
            let (a, d) = (classify("appeared"), classify("vanished"));
            let nearest = |key: &str, seen: &[bool]| {
                shot[key]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .zip(seen)
                    .filter(|(_, s)| **s)
                    .map(|(e, _)| e["dist"].as_f64().unwrap_or(0.0))
                    .fold(f64::INFINITY, f64::min)
            };
            println!(
                "{label} {id}: appeared in the view cone {}, in plain view {} (nearest {:.0} m); \
                 vanished {}, in plain view {} (nearest {:.0} m)",
                a.len(),
                a.iter().filter(|s| **s).count(),
                nearest("appeared", &a),
                d.len(),
                d.iter().filter(|s| **s).count(),
                nearest("vanished", &d),
            );
            los.insert(
                id.to_string(),
                serde_json::json!({ "appeared": a, "vanished": d }),
            );
        }
        let out = serde_json::to_string_pretty(&serde_json::Value::Object(los)).unwrap();
        std::fs::write(format!("{dir}/{label}_los.json"), out + "\n").expect("write los json");
    }
}
