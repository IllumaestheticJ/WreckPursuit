//! Police vs civilian traffic contacts in the city (workstream 06, round 6; playtest row 11).
//!
//! 12×12 procedural city at noon with traffic and physical police cars (the
//! `soak/traffic_soak_observer` setup):
//! - `patrol`: an observer car parked off the road at the core anchors physical traffic; nobody
//!   is wanted, so every unit patrols. One run of `secs` seconds.
//! - `pursuit`: a wanted racer (heat 45) driven around the arterial ring at 20 m/s by
//!   `wp_world`'s path follower. A pursuit usually ends in a capture within a minute, so runs
//!   with different seeds and start points are added up until `secs` seconds of active pursuit.
//!
//! Incidents (once per pair per 2 s), split by whether the police car had its lights on:
//! - **wall hits**: contacts of a police car with the static world (walls, kerbs, buildings;
//!   lead P8 pro driving);
//! - **contacts**: Rapier contacts between a police car and a physical traffic car (traffic is
//!   physical only near a player);
//! - **overlaps**: a police car's footprint overlapping a kinematic traffic car's (they would have
//!   collided had the traffic car been physical). Overlaps with the police car stopped are traffic
//!   driving into it and are listed separately.
//!
//! ```sh
//! cargo run --release -p simcli --example police_traffic_contacts -- patrol 300
//! cargo run --release -p simcli --example police_traffic_contacts -- pursuit 300
//! ```

use std::collections::BTreeMap;

use simcli::run::build_sim;
use simcli::{load_scenario, FsData, RunOptions, Scenario};
use wp_contracts::{EntityId, SimEvent, VehicleSim, PHYSICS_DT};
use wp_sim::VehicleKind;
use wp_world::drive::{DrivePath, FollowParams, PathFollower};
use wp_world::traffic::Lod;

/// Incident counts; `[lights off, lights on]`.
#[derive(Default)]
struct Tally {
    contacts: [u32; 2],
    overlaps: [u32; 2],
    /// Overlaps with the police car stopped (traffic drove into it).
    overlaps_police_stopped: u32,
    /// Police car hits on the static world (walls, kerbs, buildings), `[lights off, on]`.
    wall_hits: [u32; 2],
    max_rel_mps: f32,
    sim_s: f32,
    pursued_s: f32,
}

/// Two car footprints (4.6 m × 1.9 m) overlap (separating-axis test).
fn boxes_overlap(pa: [f32; 2], fa: [f32; 2], pb: [f32; 2], fb: [f32; 2]) -> bool {
    let (hl, hw) = (2.3f32, 0.95f32);
    let d = [pb[0] - pa[0], pb[1] - pa[1]];
    let axes = [fa, [fa[1], -fa[0]], fb, [fb[1], -fb[0]]];
    let proj = |f: [f32; 2], ax: [f32; 2]| {
        let r = [f[1], -f[0]];
        hl * (f[0] * ax[0] + f[1] * ax[1]).abs() + hw * (r[0] * ax[0] + r[1] * ax[1]).abs()
    };
    axes.iter().all(|ax| {
        let dist = (d[0] * ax[0] + d[1] * ax[1]).abs();
        dist < proj(fa, *ax) + proj(fb, *ax)
    })
}

/// One run. `racer_start` = arc length on the arterial ring to start a wanted racer at (pursuit),
/// `None` = patrol. Stops after `secs`, or when the pursuit ends.
fn run_once(base: &Scenario, seed: u64, racer_start: Option<f32>, secs: f32, t: &mut Tally) {
    let mut sc = base.clone();
    sc.seed = seed;
    sc.police_cars = true;
    sc.duration_s = secs;
    if racer_start.is_some() {
        sc.entities[0].car = "debug_coupe".into();
        sc.entities[0].police_heat = 45.0;
        sc.entities[0].pos = [-385.0, -385.0, 0.0];
    } else {
        // Off the road inside a core block: on the road the world charges the parked observer
        // with collision violations and it gets pursued, which is not a patrol measurement.
        sc.entities[0].pos = [25.0, 25.0, 0.0];
    }
    let data = FsData::new("data");
    let mut built = build_sim(&sc, &data, &RunOptions::default()).expect("builds");
    let racer = built.ids[0];
    let sim = &mut built.sim;

    let mut follow = None;
    if let Some(s0) = racer_start {
        let ring = [
            [-385.0, -385.0],
            [385.0, -385.0],
            [385.0, 385.0],
            [-385.0, 385.0],
        ];
        let route = sim.world().plan_circuit(&ring).expect("arterial ring");
        let drive = DrivePath::from_route(&route);
        let (p, tan) = drive.point_at(s0);
        let yaw = wp_math::atan2(-tan[0], tan[1]);
        sim.place_vehicle(racer, p, yaw, 15.0)
            .expect("racer placed");
        let car = &built.cars[&sc.entities[0].car];
        let f = PathFollower::for_vehicle(&car.def, FollowParams::PURSUIT, 15.0);
        follow = Some((f, drive));
    }

    let ticks = (secs / PHYSICS_DT).round() as u64;
    let gap = (2.0 / PHYSICS_DT) as u64;
    let mut last_contact: BTreeMap<(u32, u32), u64> = BTreeMap::new();
    let mut last_overlap: BTreeMap<(u32, u32), u64> = BTreeMap::new();
    let mut was_pursued = false;
    let t_of = |tick: u64| tick as f32 * PHYSICS_DT;
    for tick in 0..ticks {
        if let Some((f, drive)) = follow.as_mut() {
            let st = sim
                .vehicle_states()
                .into_iter()
                .find(|(id, _)| *id == racer);
            if let Some((_, s)) = st {
                let out = f.follow(&s, drive, 20.0, 1.75, PHYSICS_DT);
                let _ = sim.set_input(racer, out.input);
            }
        }
        sim.step_fixed();
        t.sim_s += PHYSICS_DT;
        if let Ok(spec) = std::env::var("WP_TRACE_COP") {
            let mut it = spec.split(',');
            let (sd, ent): (u64, u32) = (
                it.next().unwrap().parse().unwrap(),
                it.next().unwrap().parse().unwrap(),
            );
            if sd == seed && tick % 24 == 0 {
                if let Some(v) = sim.vehicles().iter().find(|v| v.id.0 == ent) {
                    let st = v.vehicle.state();
                    let u = sim.police().units().iter().find(|u| u.entity == Some(v.id));
                    if let Some(u) = u {
                        eprintln!("TRACE t={:.1} pos=({:.1},{:.1}) v={:.1} in={:.2}/{:.2}/{:.2}/{:.1} plan={:.1} lim={:.1} cmd={:.1} {} lane={:.1} s={:.0}/{:.0}", t_of(tick), st.pos[0], st.pos[1], st.speed_mps, v.input.steer, v.input.throttle, v.input.brake, v.input.handbrake, u.planned_speed, u.avoid.limit_mps, u.cmd.speed_mps, u.assignment.name(), u.cmd.lane_offset_m, u.track_s, u.cmd.track.length());
                    }
                }
            }
        }
        // Police cars: entity → (lights on, position, forward, speed).
        let cops: BTreeMap<EntityId, (bool, [f32; 3], [f32; 2], f32)> = sim
            .vehicles()
            .iter()
            .filter(|v| v.kind == VehicleKind::Police)
            .map(|v| {
                let st = v.vehicle.state();
                let f = wp_world::drive::forward_of(st.rot);
                (v.id, (v.input.siren, st.pos, f, st.speed_mps))
            })
            .collect();
        let traffic: Vec<EntityId> = sim
            .vehicles()
            .iter()
            .filter(|v| v.kind == VehicleKind::Traffic)
            .map(|v| v.id)
            .collect();
        for e in sim.drain_events() {
            let SimEvent::Contact {
                a, b, rel_speed, ..
            } = e
            else {
                continue;
            };
            let wall = match (cops.get(&a), cops.get(&b)) {
                (Some(c), None) if b == wp_contracts::WORLD_ENTITY => Some((a, c)),
                (None, Some(c)) if a == wp_contracts::WORLD_ENTITY => Some((b, c)),
                _ => None,
            };
            if let Some((cop_id, c)) = wall {
                let key = (cop_id.0, u32::MAX);
                if last_contact.get(&key).is_none_or(|l| tick - l > gap) {
                    t.wall_hits[c.0 as usize] += 1;
                    println!(
                        "  seed {seed} t={:.1} s  wall hit police {} at ({:.0},{:.0}) v {:.1} lights {}",
                        t_of(tick),
                        cop_id.0,
                        c.1[0],
                        c.1[1],
                        c.3,
                        c.0
                    );
                }
                last_contact.insert(key, tick);
                continue;
            }
            let cop = match (cops.get(&a), cops.get(&b)) {
                (Some(c), None) if traffic.contains(&b) => c,
                (None, Some(c)) if traffic.contains(&a) => c,
                _ => continue,
            };
            t.max_rel_mps = t.max_rel_mps.max(rel_speed);
            let key = (a.0.min(b.0), a.0.max(b.0));
            if last_contact.get(&key).is_none_or(|l| tick - l > gap) {
                t.contacts[cop.0 as usize] += 1;
                println!(
                    "  seed {seed} t={:.1} s  contact police/traffic {key:?} rel {rel_speed:.1} m/s lights {}",
                    t_of(tick),
                    cop.0
                );
            }
            last_contact.insert(key, tick);
        }
        if tick % 4 == 0 {
            let agents: Vec<(u32, [f32; 3], f32)> = sim
                .world()
                .agents()
                .filter(|a| a.lod == Lod::Kinematic)
                .map(|a| (a.id, a.pos, a.yaw))
                .collect();
            for (id, (lights, p, f, v)) in &cops {
                for (agent, q, yaw) in &agents {
                    let (sn, cs) = wp_math::sin_cos(*yaw);
                    if !boxes_overlap([p[0], p[1]], *f, [q[0], q[1]], [-sn, cs]) {
                        continue;
                    }
                    let key = (id.0, *agent);
                    if last_overlap.get(&key).is_none_or(|l| tick - l > gap) {
                        t.overlaps[*lights as usize] += 1;
                        if v.abs() < 0.5 {
                            t.overlaps_police_stopped += 1;
                        }
                        println!(
                            "  seed {seed} t={:.1} s  overlap police {} at ({:.0},{:.0}) v {v:.1} / kinematic agent {agent} lights {lights}",
                            t_of(tick),
                            id.0,
                            p[0],
                            p[1]
                        );
                    }
                    last_overlap.insert(key, tick);
                }
            }
        }
        let pursued = sim.police().racer(racer).is_some_and(|r| r.pursued());
        if pursued {
            t.pursued_s += PHYSICS_DT;
        } else if was_pursued && racer_start.is_some() {
            println!("  seed {seed}: pursuit ended at {:.1} s", t_of(tick));
            return;
        }
        was_pursued |= pursued;
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "patrol".into());
    let secs: f32 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(300.0);
    let path = std::path::Path::new("apps/simcli/scenarios/soak/traffic_soak_observer.ron");
    let base = load_scenario(path).expect("scenario loads");
    let mut t = Tally::default();
    if mode == "pursuit" {
        for k in 0..20u64 {
            if t.pursued_s >= secs {
                break;
            }
            run_once(&base, 100 + k, Some(k as f32 * 450.0), 150.0, &mut t);
        }
    } else {
        run_once(&base, base.seed, None, secs, &mut t);
    }
    let base_s = if mode == "pursuit" {
        t.pursued_s
    } else {
        t.sim_s
    };
    let per5 = |n: u32| n as f32 * 300.0 / base_s.max(1.0);
    println!(
        "{mode}: simulated {:.0} s, racer pursued {:.0} s",
        t.sim_s, t.pursued_s
    );
    for (name, i) in [("lights off", 0), ("lights on ", 1)] {
        println!(
            "  {name}: {} contacts ({:.1} per 5 min) + {} kinematic overlaps ({:.1} per 5 min), {} wall hits ({:.1} per 5 min)",
            t.contacts[i],
            per5(t.contacts[i]),
            t.overlaps[i],
            per5(t.overlaps[i]),
            t.wall_hits[i],
            per5(t.wall_hits[i])
        );
    }
    println!(
        "  overlaps with the police car stopped (traffic drove into it): {}; max contact rel speed {:.1} m/s",
        t.overlaps_police_stopped, t.max_rel_mps
    );
    println!(
        "  (per 5 min of {})",
        if mode == "pursuit" {
            "pursuit"
        } else {
            "patrol"
        }
    );
}
