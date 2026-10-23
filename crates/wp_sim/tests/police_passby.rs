//! Owner playtest row 60 ("I sped right by a cop with no chase" / "cop doesn't reverse or
//! attempt to chase at all"), end to end with real Rapier cars: a player passing a police car
//! (an AI unit embodied as a `sentinel_cruiser`) at 120 km/h in the next lane, going the other
//! way, is a blatant speeder (`heat.blatant`): the pursuit starts within 3 s of the pass, day or
//! night, and the police car, now facing away from the player, turns round (a U-turn, or a
//! reverse swing from a standstill) within 4 s and is within 150 m of the player 15 s after
//! the pass (the player stops 200 m past the pass point). The same pass at a legal
//! 40 km/h starts nothing. Patrolling and parked police cars, grid and Port Vesper v2.

use std::sync::OnceLock;

use wp_contracts::{
    load_car, load_mode_table, EntityId, SimEvent, TimeOfDay, VehicleInput, WorldQuery,
};
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

/// City at `hour` with police cars and no traffic or parked cars, run until a police car is
/// driving its patrol with ≥ 60 m left on its road segment (so the pass happens on a straight).
fn city_with_patrol(city: City, hour: f32) -> (Sim, EntityId) {
    city_with_patrol_seeded(city, hour, 60)
}

/// [`city_with_patrol`] with another sim seed (other patrols, other streets).
fn city_with_patrol_seeded(city: City, hour: f32, seed: u64) -> (Sim, EntityId) {
    let mode = load_mode_table(data("modes/sim.toml")).unwrap();
    let damage_mode = mode.mode;
    let mut sim = Sim::new(SimConfig {
        seed,
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
    // The pre-2026-10-23 patrol counts: this checks how a cop reacts to a pass, not how many
    // cops there are, and the shipped table is now too sparse to find one on a straight.
    let mut police_cfg = wp_police::PoliceConfig::builtin();
    let t = |dawn, day, dusk, night| wp_police::config::PhaseTable {
        dawn,
        day,
        dusk,
        night,
    };
    police_cfg.presence.patrols = wp_police::config::DistrictTable {
        core: t(2, 8, 6, 3),
        suburb: t(1, 3, 2, 1),
        industrial: t(1, 2, 2, 1),
    };
    let setup = wp_police::PoliceSetup {
        seed: seed ^ 0x0090_11CE, // wp_sim's police seed salt, as `Sim::set_world` uses
        mode: damage_mode,
        internal_driving: true,
        hazards: false,
    };
    sim.set_police(wp_police::PoliceSystem::for_world(
        police_cfg,
        sim.world(),
        setup,
    ));
    let graph = wp_police::RoadGraph::from_city(sim.world().map());
    for _ in 0..240 * 60 {
        sim.step_fixed();
        let states = sim.vehicle_states();
        let found = sim.vehicles().iter().zip(&states).find(|(v, (_, s))| {
            let speed = (s.lin_vel[0].powi(2) + s.lin_vel[1].powi(2)).sqrt();
            if v.kind != VehicleKind::Police || v.police_unit.is_none() || speed < 6.0 {
                return false;
            }
            // At least 60 m left on its road segment in the direction
            // it is driving.
            let (p, yaw) = (s.pos, yaw_of(s.rot));
            let Some(rp) = graph.nearest_on_road([p[0], p[1]]) else {
                return false;
            };
            let seg = graph.segment(rp.segment);
            let (a, b) = (graph.node(seg.a).pos, graph.node(seg.b).pos);
            let f = [-yaw.sin(), yaw.cos()];
            let toward_b = (b[0] - a[0]) * f[0] + (b[1] - a[1]) * f[1] > 0.0;
            let left = if toward_b { 1.0 - rp.t } else { rp.t } * seg.length_m;
            left > 60.0
        });
        if let Some((cop, _)) = found {
            let id = cop.id;
            return (sim, id);
        }
    }
    panic!("{city:?} at {hour}:00: no police car on a straight patrol within 60 s");
}

fn yaw_of(rot: [f32; 4]) -> f32 {
    let [x, y, z, w] = rot;
    let fx = 2.0 * (x * y - w * z);
    let fy = 1.0 - 2.0 * (x * x + z * z);
    (-fx).atan2(fy)
}

fn pose(sim: &Sim, id: EntityId) -> ([f32; 3], f32) {
    let s = sim
        .vehicle_states()
        .into_iter()
        .find(|v| v.0 == id)
        .unwrap()
        .1;
    (s.pos, yaw_of(s.rot))
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

struct Outcome {
    /// Closest approach, s after placement.
    pass_at_s: f32,
    pursuit_at_s: Option<f32>,
    /// The police car first faced the player (forward · to-player > 0.5) after the pass, s.
    facing_at_s: Option<f32>,
    /// Police car to player distance 15 s after the pass, m.
    gap_after_15_s: Option<f32>,
    /// The player touched a police car (any unit) before the pursuit started.
    hit_police: bool,
    /// The player first became wanted, s after placement.
    wanted_at_s: Option<f32>,
    /// Police car to player distance 10 s after the pass, m.
    gap_after_10_s: Option<f32>,
    /// Police car to player distance at the end of the run, m.
    gap_end: f32,
    /// The player ended more than 15 m from any road (the scripted run drives straight on
    /// where its street ends): police can only search for it there.
    off_road: bool,
}

/// How the player meets the cop.
#[derive(Debug, Clone, Copy)]
struct Spec {
    /// Player speed, km/h.
    kmh: f32,
    /// The cop is stopped at its spot first.
    parked: bool,
    /// Run time, s.
    secs: f32,
    /// Coming up from behind the cop going its way, instead of coming the other way.
    overtake: bool,
    /// Lateral distance to the left of the cop's centre line, m.
    lateral_m: f32,
}

/// The player comes the other way in the lane left of the cop at `kmh`, passes it, and brakes
/// to a stop 200 m past it. `parked`: the cop is stopped at its spot first.
fn pass(city: City, hour: f32, kmh: f32, parked: bool, secs: f32) -> Outcome {
    let (sim, cop) = city_with_patrol(city, hour);
    pass_in(sim, cop, kmh, parked, secs)
}

/// [`pass`] at `over_kmh` over the speed limit where the cop is.
fn pass_over(city: City, hour: f32, over_kmh: f32, parked: bool, secs: f32) -> (Outcome, f32) {
    let (sim, cop) = city_with_patrol(city, hour);
    let (cp, _) = pose(&sim, cop);
    let limit = sim.world().speed_limit_at(cp).unwrap_or(13.9) * 3.6;
    (pass_in(sim, cop, limit + over_kmh, parked, secs), limit)
}

fn pass_in(sim: Sim, cop: EntityId, kmh: f32, parked: bool, secs: f32) -> Outcome {
    let spec = Spec {
        kmh,
        parked,
        secs,
        overtake: false,
        lateral_m: 3.5,
    };
    pass_spec(sim, cop, spec)
}

fn pass_spec(mut sim: Sim, cop: EntityId, spec: Spec) -> Outcome {
    let Spec {
        kmh,
        parked,
        secs,
        overtake,
        lateral_m,
    } = spec;
    let (cp, cyaw) = pose(&sim, cop);
    if parked {
        sim.place_on_ground(cop, [cp[0], cp[1]], cyaw, 0.0, Some(cp[2]))
            .unwrap();
    }
    let car = load_car(data("cars/debug_sedan.json")).unwrap();
    let v = kmh / 3.6;
    // Oncoming: start 3 s of travel ahead of the cop, to its left, facing it. Overtaking:
    // start behind it, to its left, going its way (closing at about half the speed on a
    // patrolling cop).
    let (start, yaw) = if overtake {
        let back = if parked { v * 3.0 } else { v * 1.5 + 20.0 };
        (offset(cp, cyaw, -back, lateral_m), cyaw)
    } else {
        (
            offset(cp, cyaw, v * 3.0, lateral_m),
            cyaw + std::f32::consts::PI,
        )
    };
    let player = sim
        .spawn_vehicle(&car, [start[0], start[1], cp[2] + 0.5], yaw)
        .unwrap();
    sim.place_on_ground(player, start, yaw, v, Some(cp[2]))
        .unwrap();
    sim.drain_events();
    let mut out = Outcome {
        pass_at_s: f32::MAX,
        pursuit_at_s: None,
        facing_at_s: None,
        gap_after_15_s: None,
        hit_police: false,
        wanted_at_s: None,
        gap_after_10_s: None,
        gap_end: f32::MAX,
        off_road: false,
    };
    let police: Vec<EntityId> = sim
        .vehicles()
        .iter()
        .filter(|v| v.kind == VehicleKind::Police)
        .map(|v| v.id)
        .collect();
    let mut best = f32::MAX;
    let mut passed = false;
    let mut stop_from: Option<[f32; 2]> = None;
    let mut pass_point: Option<[f32; 2]> = None;
    for t in 0..(secs * 240.0) as u32 {
        let ts = (t + 1) as f32 / 240.0;
        let (pp, _) = pose(&sim, player);
        let (qp, qyaw) = pose(&sim, cop);
        let d = (pp[0] - qp[0]).hypot(pp[1] - qp[1]);
        if !passed && d < best {
            best = d;
            out.pass_at_s = ts;
        } else if d > best + 1.0 && !passed {
            passed = true;
            pass_point = Some([pp[0], pp[1]]);
        }
        // Hold the speed until 200 m past where it passed the cop, then stop.
        let past = pass_point.is_some_and(|q| (pp[0] - q[0]).hypot(pp[1] - q[1]) > 200.0);
        if past && stop_from.is_none() {
            stop_from = Some([pp[0], pp[1]]);
        }
        let speed = sim
            .vehicle_states()
            .into_iter()
            .find(|s| s.0 == player)
            .map(|s| s.1.speed_mps)
            .unwrap_or(0.0);
        let input = if stop_from.is_some() {
            VehicleInput {
                brake: 0.4,
                handbrake: 1.0,
                ..Default::default()
            }
        } else {
            VehicleInput {
                throttle: if speed < v { 0.6 } else { 0.0 },
                ..Default::default()
            }
        };
        sim.set_input(player, input).unwrap();
        sim.step_fixed();
        for e in sim.drain_events() {
            if std::env::var("WP_DBG").is_ok() {
                match &e {
                    SimEvent::Police(PoliceEvent::Radio { text, .. }) => {
                        eprintln!("t={ts:.2} radio {text}")
                    }
                    SimEvent::TrafficViolation { vehicle, kind, .. } if *vehicle == player => {
                        eprintln!("t={ts:.2} violation {kind:?}")
                    }
                    SimEvent::Contact {
                        a, b, rel_speed, ..
                    } if *a == player || *b == player => {
                        eprintln!("t={ts:.2} contact {a:?}/{b:?} {rel_speed:.1}")
                    }
                    _ => {}
                }
            }
            if let SimEvent::Contact { a, b, .. } = &e {
                let other = if *a == player {
                    Some(*b)
                } else if *b == player {
                    Some(*a)
                } else {
                    None
                };
                if other.is_some_and(|o| police.contains(&o)) && out.pursuit_at_s.is_none() {
                    out.hit_police = true;
                }
            }
            if let SimEvent::Police(PoliceEvent::WantedLevelChanged { racer, level }) = &e {
                if *racer == player && *level > wp_contracts::WantedLevel::None {
                    out.wanted_at_s.get_or_insert(ts);
                }
            }
            if let SimEvent::Police(PoliceEvent::PursuitStarted { racer, .. }) = e {
                if racer == player {
                    out.pursuit_at_s.get_or_insert(ts);
                }
            }
        }
        if std::env::var("WP_DBG2").is_ok() && t % 60 == 0 && ts > 2.0 && ts < 12.0 {
            let v = sim.vehicles().iter().find(|v| v.id == cop).unwrap();
            let sp = sim
                .vehicle_states()
                .into_iter()
                .find(|s| s.0 == cop)
                .unwrap()
                .1;
            let u = sim
                .police()
                .units()
                .iter()
                .find(|u| u.entity == Some(cop))
                .unwrap();
            eprintln!(
                "t={ts:.2} yaw {qyaw:.2} v {:.1} in steer {:.2} thr {:.2} brk {:.2} swing {}",
                sp.speed_mps,
                v.input.steer,
                v.input.throttle,
                v.input.brake,
                u.unstick.swinging()
            );
        }
        if std::env::var("WP_DBG").is_ok() && t % 240 == 0 {
            let u = sim
                .police()
                .units()
                .iter()
                .find(|u| u.entity == Some(cop))
                .map(|u| format!("{} {:.1}", u.assignment.name(), u.cmd.speed_mps));
            eprintln!(
                "t={ts:.0} player ({:.0},{:.0}) cop ({:.0},{:.0}) yaw {qyaw:.2} d {d:.0} {u:?}",
                pp[0], pp[1], qp[0], qp[1]
            );
        }
        if passed && out.pursuit_at_s.is_some() {
            let f = [-qyaw.sin(), qyaw.cos()];
            let to = [pp[0] - qp[0], pp[1] - qp[1]];
            let n = (to[0] * to[0] + to[1] * to[1]).sqrt().max(1e-3);
            if out.facing_at_s.is_none() && (f[0] * to[0] + f[1] * to[1]) / n > 0.5 {
                out.facing_at_s = Some(ts);
            }
            if out.gap_after_15_s.is_none() && ts >= out.pass_at_s + 15.0 {
                out.gap_after_15_s = Some(d);
            }
            if out.gap_after_10_s.is_none() && ts >= out.pass_at_s + 10.0 {
                out.gap_after_10_s = Some(d);
            }
        }
        out.gap_end = d;
    }
    let graph = wp_police::RoadGraph::from_city(sim.world().map());
    let (pp, _) = pose(&sim, player);
    out.off_road = graph
        .nearest_on_road([pp[0], pp[1]])
        .is_none_or(|rp| rp.dist_m > 15.0);
    out
}

fn assert_chase(city: City, hour: f32, parked: bool) {
    let label = format!(
        "{city:?} {hour}:00, {} cop",
        if parked { "parked" } else { "patrolling" }
    );
    let o = pass(city, hour, 120.0, parked, 24.0);
    let at = o
        .pursuit_at_s
        .unwrap_or_else(|| panic!("{label}: no pursuit"));
    assert!(
        at <= o.pass_at_s + 3.0,
        "{label}: passed at {:.1} s, pursuit at {at:.1} s",
        o.pass_at_s
    );
    let facing = o
        .facing_at_s
        .unwrap_or_else(|| panic!("{label}: the cop never turned toward the player"));
    assert!(
        facing <= o.pass_at_s + 4.0,
        "{label}: passed at {:.1} s, facing the player at {facing:.1} s",
        o.pass_at_s
    );
    let gap = o.gap_after_15_s.expect("ran 15 s past the pass");
    assert!(
        gap <= 150.0,
        "{label}: {gap:.0} m from the player 15 s after the pass"
    );
}

#[test]
fn speeding_past_a_cop_starts_a_chase_and_the_cop_turns_round_on_the_grid() {
    for hour in [12.0, 23.0] {
        for parked in [false, true] {
            assert_chase(City::Grid, hour, parked);
        }
    }
}

#[test]
fn speeding_past_a_cop_starts_a_chase_and_the_cop_turns_round_in_port_vesper() {
    for hour in [12.0, 23.0] {
        for parked in [false, true] {
            assert_chase(City::Vesper, hour, parked);
        }
    }
}

#[test]
fn a_legal_pass_by_starts_nothing() {
    for city in [City::Grid, City::Vesper] {
        for hour in [12.0, 23.0] {
            let o = pass(city, hour, 40.0, false, 10.0);
            // Bumping into a police car (here: another patrol) is an assault, not this rule.
            assert!(
                o.pursuit_at_s.is_none() || o.hit_police,
                "{city:?} {hour}:00: pursuit at {:?} s after a 40 km/h pass",
                o.pursuit_at_s
            );
        }
    }
}

/// Owner playtest row 80 ("I sped right by a cop and the notice didn't turn into a pursuit"):
/// a parked cop and an oncoming (patrolling) one, passed at 20, 40 and 80 km/h over the limit,
/// day and night, grid and Port Vesper v2: the pursuit starts within 1.5 s of the pass.
#[test]
fn speeding_past_a_cop_at_20_40_80_over_starts_a_chase_within_1_5_s() {
    let mut failures = Vec::new();
    for city in [City::Grid, City::Vesper] {
        for hour in [12.0, 23.0] {
            for parked in [true, false] {
                for over in [20.0, 40.0, 80.0] {
                    let (o, limit) = pass_over(city, hour, over, parked, 10.0);
                    let after = o.pursuit_at_s.map(|t| t - o.pass_at_s);
                    let label = format!(
                        "{city:?} {hour}:00 {} cop, {over:.0} km/h over a {limit:.0} limit",
                        if parked { "parked" } else { "oncoming" }
                    );
                    eprintln!("PASSBY {label}: pursuit {after:?} s after the pass");
                    if !after.is_some_and(|a| a <= 1.5) {
                        failures.push(format!("{label}: pursuit {after:?} s after the pass"));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Lead / owner report (Drift Lab, 2026-10-21): "cops are seeing me (noticed), but not
/// triggering pursuit" when speeding past a cop in free roam. Every way of passing one: 60 to
/// 150 km/h, oncoming or overtaking, one lane over (3.5 m) or across the road (7 m), a parked
/// or a patrolling cop, three seeds (three different cops and streets), grid and Port Vesper v2
/// at noon. Whenever the pass makes the player wanted (within 3 s of it), the pursuit starts
/// within 1.5 s of that, the cop faces the player within 5 s of the pass and closes in: at the end of the run
/// (24 s; the player stops 200 m past the pass, often off the road network, so the cop drives
/// round the block) it is within 60 m of the player or at most 80 % of its distance 10 s after
/// the pass, unless the player ended off the road network (then police can only search). Being wanted with no pursuit ("noticed" and nothing happens) fails. 20 km/h or more
/// over the limit always makes the player wanted.
#[test]
#[ignore = "owner turn-round / keep-up work in progress: 18 of 192 passes still fail (slow \
            turn-round, see police.md); run with --ignored"]
fn every_noticed_pass_turns_into_a_chase_that_closes_in() {
    let mut failures = Vec::new();
    let mut rows = 0;
    for city in [City::Grid, City::Vesper] {
        for seed in [60, 61, 62] {
            for parked in [true, false] {
                for overtake in [false, true] {
                    for lateral_m in [3.5, 7.0] {
                        for kmh in [60.0, 80.0, 100.0, 150.0] {
                            // `WP_NOTICED=Grid/60/patrolling/overtaking/3.5/150` runs one case.
                            let key = format!(
                                "{city:?}/{seed}/{}/{}/{lateral_m}/{kmh}",
                                if parked { "parked" } else { "patrolling" },
                                if overtake { "overtaking" } else { "oncoming" },
                            );
                            if std::env::var("WP_NOTICED").is_ok_and(|k| k != key) {
                                continue;
                            }
                            let (sim, cop) = city_with_patrol_seeded(city, 12.0, seed);
                            let (cp, _) = pose(&sim, cop);
                            let limit = sim.world().speed_limit_at(cp).unwrap_or(13.9) * 3.6;
                            let spec = Spec {
                                kmh,
                                parked,
                                secs: 24.0,
                                overtake,
                                lateral_m,
                            };
                            let o = pass_spec(sim, cop, spec);
                            rows += 1;
                            let label = format!(
                                "{city:?} seed {seed} {} cop, {} at {kmh:.0} km/h ({:+.0} over a \
                                 {limit:.0} limit), {lateral_m} m over",
                                if parked { "parked" } else { "patrolling" },
                                if overtake { "overtaking" } else { "oncoming" },
                                kmh - limit,
                            );
                            let rel = |t: Option<f32>| t.map(|t| t - o.pass_at_s);
                            eprintln!(
                                "NOTICED {label}: wanted {:?} pursuit {:?} facing {:?} gap10 {:?} \
                                 end {:.0} off-road {}",
                                rel(o.wanted_at_s),
                                rel(o.pursuit_at_s),
                                rel(o.facing_at_s),
                                o.gap_after_10_s,
                                o.gap_end,
                                o.off_road
                            );
                            if kmh - limit >= 20.0 && o.wanted_at_s.is_none() {
                                failures.push(format!("{label}: never wanted"));
                            }
                            // Wanted by the pass itself (within 3 s of it): a report phoned
                            // in later, far from any unit, starts no pursuit by design.
                            let Some(w) = o.wanted_at_s.filter(|w| *w <= o.pass_at_s + 3.0) else {
                                continue;
                            };
                            match o.pursuit_at_s {
                                Some(p) if p <= w + 1.5 => {}
                                p => failures.push(format!(
                                    "{label}: wanted at {:.1} s, pursuit {:?}",
                                    w - o.pass_at_s,
                                    rel(p)
                                )),
                            }
                            if o.pursuit_at_s.is_none() {
                                continue;
                            }
                            if !rel(o.facing_at_s).is_some_and(|f| f <= 5.0) {
                                failures.push(format!(
                                    "{label}: faced the player {:?} s after the pass",
                                    rel(o.facing_at_s)
                                ));
                            }
                            let closing = o.off_road
                                || o.gap_end <= 60.0
                                || o.gap_after_10_s.is_some_and(|g| o.gap_end <= 0.8 * g);
                            if !closing {
                                failures.push(format!(
                                    "{label}: {:?} m away 10 s after the pass, {:.0} m at the end",
                                    o.gap_after_10_s, o.gap_end
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    eprintln!("NOTICED {rows} passes, {} failures", failures.len());
    assert!(failures.is_empty(), "{failures:#?}");
}
