//! Owner playtest row 24 ("I sideswiped a cop and a chase didn't start"), end to end with real
//! Rapier cars: a player car sideswiping a patrolling police car (an AI unit embodied as a
//! `sentinel_cruiser`) produces `SimEvent::Contact`, `wp_police` maps the cop's entity to its
//! unit, counts it as assaulting an officer, and starts a pursuit within 1 s at wanted level
//! L2 or more, day or night, on the grid city and on Port Vesper v2. A 5 km/h parking bump
//! starts nothing.
//!
//! The patrol car is paused where it is (as if waiting at a junction) when the player is put
//! next to it, so the hit speed is the player's speed.

use std::sync::OnceLock;

use wp_contracts::{load_car, load_mode_table, EntityId, SimEvent, TimeOfDay, WantedLevel};
use wp_police::PoliceEvent;
use wp_sim::{Sim, SimConfig, VehicleKind};
use wp_world::network::RoadNetwork;
use wp_world::vesper::{generate, VesperConfig};
use wp_world::{TrafficConfig, World, WorldConfig};

fn data(path: &str) -> String {
    format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
}

#[derive(Debug, Clone, Copy)]
enum City {
    Grid,
    Vesper,
}

fn vesper() -> &'static RoadNetwork {
    static NET: OnceLock<RoadNetwork> = OnceLock::new();
    NET.get_or_init(|| generate(&VesperConfig::default()))
}

/// City at `hour` with police cars and no traffic or parked cars (nothing else to hit),
/// run until a police car is driving its patrol.
fn city_with_patrol(city: City, hour: f32) -> (Sim, EntityId) {
    let mode = load_mode_table(data("modes/sim.toml")).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed: 24,
        mode,
        time_of_day: TimeOfDay::from_hours(hour),
        time_ratio: 1.0,
    });
    sim.set_police_car(load_car(data("cars/sentinel_cruiser.json")).unwrap());
    let cfg = WorldConfig {
        traffic: TrafficConfig {
            enabled: false,
            ..Default::default()
        },
        parked_cars: false,
        ..Default::default()
    };
    sim.set_world(match city {
        City::Grid => World::procedural(&wp_world::procgen::ProcGenConfig::small(), cfg),
        City::Vesper => World::new(vesper().clone(), cfg),
    });
    for _ in 0..240 * 30 {
        sim.step_fixed();
        let states = sim.vehicle_states();
        let patrol = sim.vehicles().iter().zip(&states).find(|(v, (_, s))| {
            let speed = (s.lin_vel[0].powi(2) + s.lin_vel[1].powi(2)).sqrt();
            v.kind == VehicleKind::Police && v.police_unit.is_some() && speed > 4.0
        });
        if let Some((cop, _)) = patrol {
            let id = cop.id;
            return (sim, id);
        }
    }
    panic!("{city:?} at {hour}:00: no police car on patrol within 30 s");
}

/// Heading (yaw about +Z, 0 = +Y) and position of a vehicle.
fn pose(sim: &Sim, id: EntityId) -> ([f32; 3], f32) {
    let s = sim
        .vehicle_states()
        .into_iter()
        .find(|v| v.0 == id)
        .unwrap()
        .1;
    let [x, y, z, w] = s.rot;
    // Forward = rot · +Y.
    let fx = 2.0 * (x * y - w * z);
    let fy = 1.0 - 2.0 * (x * x + z * z);
    (s.pos, (-fx).atan2(fy))
}

/// Point `fwd` m ahead and `left` m to the left of a pose.
fn offset(p: [f32; 3], yaw: f32, fwd: f32, left: f32) -> [f32; 2] {
    let (s, c) = yaw.sin_cos();
    let f = [-s, c];
    let l = [-c, -s];
    [
        p[0] + f[0] * fwd + l[0] * left,
        p[1] + f[1] * fwd + l[1] * left,
    ]
}

/// What happened within `secs` of the player being placed.
struct Outcome {
    /// First player–cop contact, s after placement.
    contact_at_s: Option<f32>,
    pursuit_at_s: Option<f32>,
    level: WantedLevel,
    radio: bool,
}

/// How the player meets the cop.
#[derive(Debug, Clone, Copy)]
struct Approach {
    /// Player centre this far left of the cop's centre line, m.
    left: f32,
    /// Player aimed this far into the cop, degrees.
    angle_deg: f32,
    /// How far the player's nose is past the cop's tail when the bodies first touch, m
    /// (0 = its front corner meets the cop's rear corner).
    overlap: f32,
}

/// Aimed 6° in from the next lane: the front corner meets the cop's rear quarter.
const QUARTER: Approach = Approach {
    left: 2.3,
    angle_deg: 6.0,
    overlap: 0.0,
};
/// Glancing flank swipe: 3° in, the front corner rubs along the cop's side.
const GLANCING: Approach = Approach {
    left: 2.3,
    angle_deg: 3.0,
    overlap: 2.0,
};
/// Straight into the tail.
const TAIL: Approach = Approach {
    left: 0.0,
    angle_deg: 0.0,
    overlap: 0.0,
};

/// Pause the patrol car, put the player behind it on `approach` at `kmh`, and run `secs`.
fn hit(city: City, hour: f32, kmh: f32, approach: Approach, secs: f32) -> Outcome {
    let Approach {
        left,
        angle_deg,
        overlap,
    } = approach;
    let (mut sim, cop) = city_with_patrol(city, hour);
    let (cp, cyaw) = pose(&sim, cop);
    sim.place_on_ground(cop, [cp[0], cp[1]], cyaw, 0.0, Some(cp[2]))
        .unwrap();
    let car = load_car(data("cars/debug_sedan.json")).unwrap();
    // Centre-to-centre distance behind the cop (both cars ≈ 4.9 m long, 1.9 m wide).
    let back = if angle_deg > 0.0 {
        4.9 - overlap + (left - 1.9).max(0.0) / angle_deg.to_radians().tan()
    } else {
        4.9 + 0.2
    };
    let xy = offset(cp, cyaw, -back, left);
    let player = sim
        .spawn_vehicle(&car, [xy[0], xy[1], cp[2] + 0.5], cyaw)
        .unwrap();
    let yaw = cyaw - angle_deg.to_radians();
    sim.place_on_ground(player, xy, yaw, kmh / 3.6, Some(cp[2]))
        .unwrap();
    sim.drain_events();
    let mut out = Outcome {
        contact_at_s: None,
        pursuit_at_s: None,
        level: WantedLevel::None,
        radio: false,
    };
    let ticks = (secs * 240.0) as u32;
    for t in 0..ticks {
        sim.step_fixed();
        for e in sim.drain_events() {
            match &e {
                SimEvent::Contact { a, b, .. }
                    if (*a == player && *b == cop) || (*a == cop && *b == player) =>
                {
                    out.contact_at_s.get_or_insert((t + 1) as f32 / 240.0);
                }
                SimEvent::Police(PoliceEvent::PursuitStarted { racer, .. }) if *racer == player => {
                    out.pursuit_at_s.get_or_insert((t + 1) as f32 / 240.0);
                }
                SimEvent::Police(PoliceEvent::Radio { text, .. })
                    if text.contains("assault on an officer") =>
                {
                    out.radio = true;
                }
                _ => {}
            }
        }
    }
    out.level = sim.police().wanted_level(player);
    out
}

fn assert_assault(city: City, hour: f32, kmh: f32, approach: Approach) {
    let o = hit(city, hour, kmh, approach, 2.0);
    let label = format!("{city:?} {hour}:00, {approach:?} at {kmh} km/h");
    let hit_at = o
        .contact_at_s
        .unwrap_or_else(|| panic!("{label}: the cars never touched"));
    let at = o
        .pursuit_at_s
        .unwrap_or_else(|| panic!("{label}: no pursuit"));
    assert!(
        at >= hit_at && at - hit_at <= 1.0,
        "{label}: contact at {hit_at} s, pursuit at {at} s"
    );
    assert!(o.level >= WantedLevel::L2, "{label}: {:?}", o.level);
    assert!(o.radio, "{label}: no assault radio line");
}

#[test]
fn sideswiping_a_patrol_car_starts_a_pursuit_on_the_grid() {
    for hour in [12.0, 23.0] {
        for kmh in [30.0, 50.0] {
            for approach in [GLANCING, QUARTER] {
                assert_assault(City::Grid, hour, kmh, approach);
            }
        }
    }
}

#[test]
fn sideswiping_a_patrol_car_starts_a_pursuit_in_port_vesper() {
    for (hour, kmh) in [(12.0, 30.0), (23.0, 50.0)] {
        for approach in [GLANCING, QUARTER] {
            assert_assault(City::Vesper, hour, kmh, approach);
        }
    }
}

#[test]
fn a_parking_bump_starts_nothing() {
    for city in [City::Grid, City::Vesper] {
        for hour in [12.0, 23.0] {
            // Straight into the tail at 5 km/h; watch for 3 s.
            let o = hit(city, hour, 5.0, TAIL, 3.0);
            let label = format!("{city:?} {hour}:00");
            assert!(o.contact_at_s.is_some(), "{label}: the bump never touched");
            assert_eq!(o.pursuit_at_s, None, "{label}");
            assert_eq!(o.level, WantedLevel::None, "{label}");
            assert!(!o.radio, "{label}");
        }
    }
}
