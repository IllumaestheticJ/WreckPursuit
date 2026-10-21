//! Traffic stays calm when the player drives near it (owner: "don't have AI cars freak out and
//! slam the brakes when the player car gets near"): a player passing in the next lane, coming
//! the other way in the opposite lane, or cutting in ahead while pulling away gets no emergency
//! stop and no hard braking. A car actually coming at it in its lane still does.
//!
//! Each case runs the same world twice, with and without the player, and compares the chosen
//! agent's speed: only the difference is the player's doing.

use wp_contracts::{EntityId, SimEvent, TimeOfDay, PHYSICS_DT};
use wp_world::geom::{self, V2, V3};
use wp_world::procgen::ProcGenConfig;
use wp_world::{AgentId, TrackedVehicle, TrafficConfig, World, WorldConfig};

fn world() -> World {
    World::procedural(
        &ProcGenConfig::default(),
        WorldConfig {
            start_time: TimeOfDay::from_hours(12.0),
            time_ratio: 1.0,
            traffic: TrafficConfig {
                density_scale: 4.0,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}

/// The player's path relative to the agent's pose at the start: offset (along, right) in m and
/// velocity (along, right) in m/s, in the agent's frame.
#[derive(Clone, Copy)]
struct Pass {
    start: [f32; 2],
    vel: [f32; 2],
}

struct Reaction {
    /// Largest speed the agent lost compared with the run without the player, m/s.
    speed_loss: f32,
    /// Ticks the agent showed emergency hazards with the player and not without.
    emergencies: u64,
}

fn agent(w: &World, id: AgentId) -> (V3, f32, f32) {
    let a = w.agents().find(|a| a.id == id).unwrap();
    (a.pos, a.yaw, a.speed_mps)
}

fn react(pass: Pass, seconds: f32) -> Reaction {
    let mut ev: Vec<SimEvent> = Vec::new();
    let mut base = world();
    base.populate();
    for _ in 0..(2.0 / PHYSICS_DT) as usize {
        base.step(PHYSICS_DT, &mut ev);
    }
    // A moving car on a straight local street, far from other agents' influence as possible:
    // the first one doing 8–14 m/s.
    let id = base
        .agents()
        .find(|a| (8.0..14.0).contains(&a.speed_mps))
        .map(|a| a.id)
        .expect("a moving car");
    let mut with = base.clone();
    let (p0, yaw0, _) = agent(&base, id);
    let f: V2 = geom::dir_of_yaw(yaw0);
    let r = geom::right_of(f);
    let to_world = |v: [f32; 2]| [f[0] * v[0] + r[0] * v[1], f[1] * v[0] + r[1] * v[1]];
    let s0 = to_world(pass.start);
    let vel = to_world(pass.vel);
    let fwd = geom::normalize(vel);
    let mut loss = 0.0f32;
    let mut emergencies = 0u64;
    let steps = (seconds / PHYSICS_DT) as usize;
    for k in 0..steps {
        let t = k as f32 * PHYSICS_DT;
        let tv = TrackedVehicle {
            entity: EntityId(1),
            pos: [
                p0[0] + s0[0] + vel[0] * t,
                p0[1] + s0[1] + vel[1] * t,
                p0[2],
            ],
            vel: [vel[0], vel[1], 0.0],
            forward: fwd,
            siren: false,
            player: true,
        };
        with.set_tracked_vehicles(&[tv]);
        with.step(PHYSICS_DT, &mut ev);
        base.step(PHYSICS_DT, &mut ev);
        let (Some(a), Some(b)) = (
            with.agents().find(|a| a.id == id),
            base.agents().find(|a| a.id == id),
        ) else {
            break;
        };
        loss = loss.max(b.speed_mps - a.speed_mps);
        if a.hazard_lights && !b.hazard_lights {
            emergencies += 1;
        }
    }
    Reaction {
        speed_loss: loss,
        emergencies,
    }
}

#[test]
fn player_overtaking_in_the_next_lane_is_ignored() {
    // From 15 m behind, one lane to the left (drifting 0.6 m toward the line), 10 m/s faster.
    let r = react(
        Pass {
            start: [-15.0, -2.7],
            vel: [21.0, 0.0],
        },
        3.0,
    );
    assert_eq!(r.emergencies, 0);
    assert!(r.speed_loss < 1.0, "lost {:.1} m/s", r.speed_loss);
}

#[test]
fn player_oncoming_in_the_opposite_lane_is_ignored() {
    // Coming the other way one lane to the left (0.6 m toward the line), 20 m/s, from 60 m.
    let r = react(
        Pass {
            start: [60.0, -2.7],
            vel: [-20.0, 0.0],
        },
        3.5,
    );
    assert_eq!(r.emergencies, 0);
    assert!(r.speed_loss < 1.0, "lost {:.1} m/s", r.speed_loss);
}

#[test]
fn player_cutting_in_ahead_and_pulling_away_gets_no_hard_braking() {
    // Appears 9 m ahead in the same lane, 4 m/s faster than the agent.
    let r = react(
        Pass {
            start: [9.0, 0.0],
            vel: [16.0, 0.0],
        },
        2.0,
    );
    assert_eq!(r.emergencies, 0);
    // A firm lift is fine (the gap is short); slamming the brakes is not (2 s at the old -9 m/s²
    // cap lost ~8 m/s).
    assert!(r.speed_loss < 3.0, "lost {:.1} m/s", r.speed_loss);
}

#[test]
fn player_coming_at_it_in_its_lane_still_triggers_an_emergency_stop() {
    let r = react(
        Pass {
            start: [60.0, 0.0],
            vel: [-25.0, 0.0],
        },
        1.5,
    );
    assert!(r.emergencies > 0);
}
