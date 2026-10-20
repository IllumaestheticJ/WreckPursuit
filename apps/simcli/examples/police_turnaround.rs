//! Police turn-arounds with physical cars (workstream 06): a patrolling police car sees a wanted
//! racer behind it and turns around (U-turn or three-point turn, `wp_police::turn`).
//!
//! 12×12 procedural city at dusk (recognition takes 2 s, so the turn starts before the pursuit)
//! with traffic and physical police cars (the `soak/traffic_soak_observer` setup). Per trial: a
//! police car on patrol mid-way along a two-way street is picked, and a wanted racer is placed
//! 35 m behind it in the other lane, driving away at 9 m/s. Reported per trial and in total:
//! whether the unit turned before the pursuit started, how long the turn took, its legs, the
//! farthest the car got from the street's centreline, contacts of that car (vehicles / world:
//! parked cars, kerbs, buildings) while turning, and whether it ends up facing after the racer.
//!
//! ```sh
//! cargo run --release -p simcli --example police_turnaround -- [trials=10] [first seed=300]
//! TRACE=1 cargo run --release -p simcli --example police_turnaround -- 1 305   # per-tick log
//! ```

use simcli::run::build_sim;
use simcli::{load_scenario, FsData, RunOptions, Scenario};
use wp_contracts::{SimEvent, PHYSICS_DT, WORLD_ENTITY};
use wp_police::graph::{dot, normalize, perp_left, scale, sub, RoadClass};
use wp_police::{Assignment, UnitKind};
use wp_world::drive::{DrivePath, FollowParams, PathFollower};

/// What one trial measured.
#[derive(Default, Debug)]
struct Trial {
    picked: bool,
    turned_before_pursuit: bool,
    turn_s: Option<f32>,
    legs: u32,
    max_offset_m: f32,
    vehicle_contacts: u32,
    world_contacts: u32,
    faces_racer: bool,
}

fn add(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

fn trial(base: &Scenario, seed: u64) -> Trial {
    let mut out = Trial::default();
    let mut sc = base.clone();
    sc.seed = seed;
    sc.police_cars = true;
    sc.time_of_day_h = 19.0;
    sc.entities[0].car = "debug_coupe".into();
    sc.entities[0].police_heat = 0.0;
    // Out of every unit's sight until the trial starts (recognition time builds up while a
    // unit sees a racer, wanted or not).
    sc.entities[0].pos = [-3000.0, -3000.0, 0.0];
    let data = FsData::new("data");
    let mut built = build_sim(&sc, &data, &RunOptions::default()).expect("builds");
    let racer = built.ids[0];
    let def = built.cars[&sc.entities[0].car].def.clone();
    let sim = &mut built.sim;
    for _ in 0..(4.0 / PHYSICS_DT) as usize {
        sim.step_fixed();
    }

    // A patrolling police car mid-way along a two-way street, with room behind it.
    let graph = sim.police().graph().clone();
    let pick =
        sim.police().units().iter().find_map(|u| {
            if u.kind != UnitKind::Car
                || u.entity.is_none()
                || !matches!(u.assignment, Assignment::Patrol { .. })
                || u.body.speed < 3.0
            {
                return None;
            }
            let rp = graph.nearest_on_road(u.body.pos)?;
            let seg = *graph.segment(rp.segment);
            if seg.oneway || seg.class != RoadClass::Local || rp.dist_m > 3.0 {
                return None;
            }
            let (a, b) = (graph.node(seg.a).pos, graph.node(seg.b).pos);
            let ab = normalize(sub(b, a));
            let dir = if dot(ab, u.body.heading) >= 0.0 {
                ab
            } else {
                scale(ab, -1.0)
            };
            let behind = if dir == ab { a } else { b };
            let ahead = if dir == ab { b } else { a };
            let room_behind = dot(sub(rp.point, behind), dir);
            let room_ahead = dot(sub(ahead, rp.point), dir);
            (dot(dir, u.body.heading) > 0.95 && room_behind > 50.0 && room_ahead > 20.0)
                .then_some((u.id, u.entity.unwrap(), rp.point, dir, rp.segment))
        });
    let Some((unit, entity, centre, dir, segment)) = pick else {
        return out;
    };
    out.picked = true;
    let half_width = graph.half_width(segment);

    // The racer: 35 m behind, other lane, driving away.
    let start = add(add(centre, scale(dir, -35.0)), scale(perp_left(dir), 1.75));
    let yaw = wp_math::atan2(dir[0], -dir[1]);
    sim.place_vehicle(racer, [start[0], start[1], 0.0], yaw, 9.0)
        .expect("racer placed");
    let far = add(centre, scale(dir, -400.0));
    let path = DrivePath::new(&[[centre[0], centre[1], 0.0], [far[0], far[1], 0.0]], false);
    let mut follower = PathFollower::for_vehicle(&def, FollowParams::PURSUIT, 9.0);
    sim.police_mut().set_heat(racer, 30.0);
    let preempt_before = sim.police().stats().preempt_turns;

    let trace = std::env::var("TRACE").is_ok();
    let mut turn_start: Option<f32> = None;
    let mut last_legs = 0;
    let mut t = 0.0f32;
    while t < 30.0 {
        if let Some((_, s)) = sim
            .vehicle_states()
            .into_iter()
            .find(|(id, _)| *id == racer)
        {
            let o = follower.follow(&s, &path, 9.0, 1.75, PHYSICS_DT);
            let _ = sim.set_input(racer, o.input);
        }
        sim.step_fixed();
        t += PHYSICS_DT;
        let turning = sim
            .police()
            .units()
            .iter()
            .find(|u| u.id == unit)
            .and_then(|u| u.turn);
        for e in sim.drain_events() {
            let SimEvent::Contact {
                a,
                b,
                rel_speed,
                point,
                ..
            } = e
            else {
                continue;
            };
            if turning.is_none() || rel_speed < 0.5 || (a != entity && b != entity) {
                continue;
            }
            if a == WORLD_ENTITY || b == WORLD_ENTITY {
                out.world_contacts += 1;
                if trace {
                    let u = sim.police().units().iter().find(|u| u.id == unit).unwrap();
                    println!(
                        "    contact world at {point:?} rel {rel_speed:.2} unit pos {:?} h {:?}",
                        u.body.pos, u.body.heading
                    );
                }
            } else if a != racer && b != racer {
                out.vehicle_contacts += 1;
            }
        }
        let Some(u) = sim.police().units().iter().find(|u| u.id == unit) else {
            break; // retired
        };
        if trace && ((t / PHYSICS_DT).round() as u32).is_multiple_of(24) {
            let input = sim
                .vehicles()
                .iter()
                .find(|v| v.id == entity)
                .map(|v| v.input);
            println!(
                "    t {t:.1} {} h {:?} v {:.2} fwd {:.2} turn {:?} in {:?}",
                u.assignment.name(),
                u.body.heading,
                u.body.speed,
                u.body.fwd_speed,
                u.turn.map(|s| (s.phase, s.legs, s.goal)),
                input.map(|i| (i.steer, i.throttle, i.brake, i.handbrake))
            );
        }
        match (turning, turn_start) {
            (Some(st), None) => {
                turn_start = Some(t);
                out.turned_before_pursuit = !sim.police().racer(racer).is_some_and(|r| r.pursued());
                last_legs = st.legs;
            }
            (Some(st), Some(_)) => {
                last_legs = st.legs;
                let off = dot(sub(u.body.pos, centre), perp_left(dir)).abs();
                out.max_offset_m = out.max_offset_m.max(off);
            }
            (None, Some(t0)) if out.turn_s.is_none() => {
                out.turn_s = Some(t - t0);
                out.legs = last_legs;
                let racer_pos = sim
                    .vehicle_states()
                    .into_iter()
                    .find(|(id, _)| *id == racer)
                    .map(|(_, s)| [s.pos[0], s.pos[1]])
                    .unwrap_or(centre);
                let to = normalize(sub(racer_pos, u.body.pos));
                out.faces_racer = dot(u.body.heading, to) > 0.7;
                break;
            }
            _ => {}
        }
    }
    let preempted = sim.police().stats().preempt_turns > preempt_before;
    out.turned_before_pursuit &= preempted;
    println!(
        "  seed {seed}: unit {} half width {half_width:.2} m: {:?}",
        unit.0, out
    );
    out
}

fn main() {
    let n: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    let path = std::path::Path::new("apps/simcli/scenarios/soak/traffic_soak_observer.ron");
    let base = load_scenario(path).expect("scenario loads");
    let first: u64 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    let trials: Vec<Trial> = (0..n).map(|k| trial(&base, first + k)).collect();
    let picked: Vec<&Trial> = trials.iter().filter(|t| t.picked).collect();
    let done: Vec<f32> = picked.iter().filter_map(|t| t.turn_s).collect();
    let mean = done.iter().sum::<f32>() / done.len().max(1) as f32;
    let max = done.iter().copied().fold(0.0, f32::max);
    println!(
        "police_turnaround: {} trials, {} with a unit on a two-way street",
        n,
        picked.len()
    );
    println!(
        "  turned before the pursuit started: {}; turns finished: {} (mean {mean:.1} s, max {max:.1} s); facing after the racer: {}",
        picked.iter().filter(|t| t.turned_before_pursuit).count(),
        done.len(),
        picked.iter().filter(|t| t.faces_racer).count()
    );
    println!(
        "  legs: {:?}; max offset from the centreline {:.2} m; contacts while turning: {} vehicles, {} world (parked cars, kerbs, buildings)",
        picked.iter().map(|t| t.legs).collect::<Vec<_>>(),
        picked.iter().map(|t| t.max_offset_m).fold(0.0, f32::max),
        picked.iter().map(|t| t.vehicle_contacts).sum::<u32>(),
        picked.iter().map(|t| t.world_contacts).sum::<u32>()
    );
}
