//! Step 2 of the roadmap: asymmetric multiplayer on the real game. One authoritative
//! `ServerInstance<WpSim>` runs Port Vesper v2 (terrain, elevation, buildings) at night with
//! physical + kinematic traffic and AI police; 4 clients join over lossy links for 3 minutes:
//!
//! - **p0, p1 — racers** (bots lapping the Lantern Row arterial loop downtown; p1 jinks and
//!   crashes now and then and is chased by AI police);
//! - **p2 — player cop** (a bot steering a police car from what *its client* sees: it follows
//!   racer 0, calls in a roadblock and spikes, and stops behind it when it pulls over);
//! - **p3 — captain** (orders the player cop onto racer 0 and sets the priority target).
//!
//! Racer 0 becomes wanted while the player cop is right behind it, so the cop's `wp_police`
//! unit recognises it and starts the pursuit; after a while racer 0 gives up and stops, and
//! the cop captures it. Racer 1 drops its link for 3 s and reconnects with its token.
//!
//! Asserts:
//! - **no desync beyond the correction budget**: prediction RMS < 0.5 m (chase budget, rams
//!   are not predicted — NET-N11), no prediction resets, no decode errors; contact-only damage
//!   replicas never diverge, < 5 % of lattice hash checks mismatch;
//! - **everyone sees the same world**: every vehicle state a client holds (racers, the cop, AI
//!   police, physical traffic) equals the server's quantised state for that tick; vehicles
//!   within 60 m of a car for 1 s are known to its client; kinematic traffic a client holds
//!   equals the server's agents; the captain's map units sit exactly where the server had
//!   them; after the run settles every damage replica equals the server's lattice;
//! - **pursuit + capture**: the pursuit of racer 0 is started by the player cop's unit, and
//!   the capture (`PursuitEnded { outcome: Captured }` crediting the player cop) reaches all
//!   four clients;
//! - **budgets**: every client's worst second ≤ 40 kB/s; server net tick avg < 8 ms and
//!   p99 < 16.7 ms (release).
//!
//! `cargo test -p wp_net --release --test vesper_e2e -- --nocapture` prints the numbers
//! recorded in `crates/wp_net/README.md`. `WP_VESPER_E2E_SECS` shortens the run (local
//! iteration only; the assertions are tuned for 180 s).

mod common;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::OnceLock;

use common::Harness;
use wp_contracts::{
    CaptainCommand, DamageMode, EntityId, LoadedCar, ModeTable, PoliceEvent, PursuitOutcome, Role,
    SimEvent, TimeOfDay, TrafficAgentSnapshot, UnitId, UnitOrder, VehicleInput, VehicleState,
};
use wp_net::bot::BotDriver;
use wp_net::client::default_damage_factory;
use wp_net::netsim::{city_ground, server_world_config, CitySetup, NetSim, WpSim};
use wp_net::protocol::{EntityKind, PLAYER_UNIT_BASE};
use wp_net::quant::{QTrafficAgent, QVehicle};
use wp_net::server::ServerConfig;
use wp_net::sim_link::LinkConfig;
use wp_net::testkit::{load_data, repo_data_dir, AI_POLICE_CAR, TRAFFIC_CAR};
use wp_net::DOWNLINK_BUDGET_BPS;
use wp_sim::SimConfig;
use wp_world::network::RoadNetwork;

const SEED: u64 = 2027;
/// Racer 1 laps the downtown arterials: x = ±200 m, y = −250 / 150 m (Lantern Row).
const DOWNTOWN_LOOP: [[f32; 2]; 4] = [
    [-200.0, -250.0],
    [200.0, -250.0],
    [200.0, 150.0],
    [-200.0, 150.0],
];
/// Racer 0 and the cop lap the port block (lane centrelines, right-hand traffic): east along
/// Gantry Row (y = −780 m, a 470 m straight — the only stretch near the centre with the
/// 120 m approach sight police need for a roadblock or spike strip; downtown has a cross
/// street every 100 m), south on x = 920 m, west along the quay (y = −950 m), north on
/// x = 450 m. `plan_circuit` does not route this block, so the loop is written out.
const PORT_LOOP: [[f32; 3]; 4] = [
    [453.5, -781.8, 0.0],
    [916.5, -781.8, 0.0],
    [916.5, -946.5, 0.0],
    [453.5, -946.5, 0.0],
];
/// Route distance (m) of each seat's spawn on its loop: racer 0 (port), racer 1 (downtown),
/// cop (port, behind racer 0).
const SPAWN_S: [f32; 3] = [45.0, 300.0, 20.0];

fn network() -> &'static RoadNetwork {
    static NET: OnceLock<RoadNetwork> = OnceLock::new();
    NET.get_or_init(|| wp_world::vesper::generate(&wp_world::vesper::VesperConfig::default()))
}

/// Up-link latencies grow with the peer index so Hellos arrive (and player ids are given out)
/// in peer order: p0 = player 1, …
fn link(seed: u64, latency_ms: f64) -> LinkConfig {
    LinkConfig {
        latency_ms,
        jitter_ms: 8.0,
        loss: 0.01,
        retransmit_ms: 120.0,
        seed,
    }
}

/// Points every `step` m along a polyline with their headings (yaw, 0 = +Y), and the
/// cumulative distance of each.
fn resample(poly: &[[f32; 3]], step: f32) -> Vec<([f32; 2], f32, f32)> {
    let mut out = Vec::new();
    let mut s_total = 0.0;
    let mut next = 0.0;
    for w in poly.windows(2) {
        let d = [w[1][0] - w[0][0], w[1][1] - w[0][1]];
        let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
        if len < 1e-3 {
            continue;
        }
        while next <= s_total + len {
            let t = (next - s_total) / len;
            let p = [w[0][0] + d[0] * t, w[0][1] + d[1] * t];
            out.push((p, wp_math::atan2(-d[0], d[1]), next));
            next += step;
        }
        s_total += len;
    }
    out
}

/// `poly` with every interior corner replaced by a quarter-circle-ish fillet of radius `r`
/// (5 points), so a waypoint follower takes junction corners on the road.
fn rounded(poly: &[[f32; 3]], r: f32) -> Vec<[f32; 3]> {
    let n = poly.len();
    let mut out = vec![poly[0]];
    for i in 1..n - 1 {
        let (a, c, b) = (poly[i - 1], poly[i], poly[i + 1]);
        let unit = |p: [f32; 3], q: [f32; 3]| {
            let d = [q[0] - p[0], q[1] - p[1]];
            let l = (d[0] * d[0] + d[1] * d[1]).sqrt();
            [d[0] / l, d[1] / l]
        };
        let (u, v) = (unit(c, a), unit(c, b));
        let p0 = [c[0] + u[0] * r, c[1] + u[1] * r];
        let p1 = [c[0] + v[0] * r, c[1] + v[1] * r];
        for k in 0..5 {
            // Quadratic Bézier with the corner as control point.
            let t = k as f32 / 4.0;
            let w = [(1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t];
            out.push([
                w[0] * p0[0] + w[1] * c[0] + w[2] * p1[0],
                w[0] * p0[1] + w[1] * c[1] + w[2] * p1[1],
                0.0,
            ]);
        }
    }
    out.push(poly[n - 1]);
    out
}

/// The loop's waypoints (every 20 m), rotated to start just ahead of route distance `s`.
fn route_from(points: &[([f32; 2], f32, f32)], s: f32) -> Vec<[f32; 2]> {
    let k = points
        .iter()
        .position(|(_, _, d)| *d > s + 12.0)
        .unwrap_or(0);
    points[k..]
        .iter()
        .chain(&points[..k])
        .map(|(p, _, _)| *p)
        .collect()
}

fn pose_at(points: &[([f32; 2], f32, f32)], s: f32) -> ([f32; 3], f32) {
    let (p, yaw, _) = points
        .iter()
        .min_by(|a, b| (a.2 - s).abs().total_cmp(&(b.2 - s).abs()))
        .expect("route points");
    ([p[0], p[1], 0.0], *yaw)
}

type Data = (BTreeMap<String, LoadedCar>, BTreeMap<DamageMode, ModeTable>);

fn vesper_sim(data: &Data) -> WpSim {
    let (cars, modes) = data;
    let config = SimConfig {
        seed: SEED,
        mode: modes[&DamageMode::Arcade].clone(),
        time_of_day: TimeOfDay::from_hours(23.0),
        time_ratio: 1.0,
    };
    let world = wp_world::World::new(network().clone(), server_world_config(&config));
    let city = CitySetup {
        world,
        police_car: Some(cars[AI_POLICE_CAR].clone()),
        traffic_car: Some(cars[TRAFFIC_CAR].clone()),
    };
    WpSim::with_city(config, cars.clone(), city)
}

/// Quantised state of every vehicle, exactly as the server sends it.
fn quantised(sim: &WpSim) -> BTreeMap<u32, QVehicle> {
    sim.vehicles()
        .into_iter()
        .map(|(e, s)| (e.0, QVehicle::from_state(&s)))
        .collect()
}

/// Hash of a vehicle's lattice node positions (what replicas must reproduce).
fn node_hash(v: &wp_sim::SimVehicle) -> u64 {
    let mut h = wp_math::StateHasher::new();
    for p in wp_contracts::DamageSim::node_positions(&v.damage) {
        for x in p {
            h.write_bytes(&x.to_le_bytes());
        }
    }
    h.finish()
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

/// Speed-controlled route follower: `BotDriver` steering, own pedals toward `target_mps`.
fn drive(bot: &mut BotDriver, s: &VehicleState, target_mps: f32) -> VehicleInput {
    bot.cruise_mps = target_mps.max(0.0);
    let mut input = bot.input(s, 4);
    if bot.crashing() {
        return input;
    }
    let err = target_mps - s.speed_mps;
    if target_mps < 0.5 {
        input.throttle = 0.0;
        input.brake = 1.0;
        input.handbrake = if s.speed_mps < 1.0 { 1.0 } else { 0.0 };
    } else if err < -1.5 {
        input.throttle = 0.0;
        input.brake = (-err / 6.0).clamp(0.2, 1.0);
    }
    input
}

/// Gets a bot car out of a wall it drove into: a car pressing the throttle at walking pace
/// for `STUCK_S` backs out with opposite lock for `BACK_OUT_S` (wp_vehicle's auto gearbox
/// engages reverse when the brake is held at a standstill; the brake then drives backwards).
#[derive(Default)]
struct Unstick {
    stuck_steps: u64,
    back_out_steps: u64,
    steer: f32,
}

impl Unstick {
    const STUCK_S: u64 = 1;
    const BACK_OUT_S: u64 = 2;

    /// The input to send instead of `input` (60 Hz steps).
    fn filter(&mut self, s: &VehicleState, input: VehicleInput) -> VehicleInput {
        if self.back_out_steps > 0 {
            self.back_out_steps -= 1;
            return VehicleInput {
                steer: self.steer,
                brake: 1.0,
                headlights: true,
                ..Default::default()
            };
        }
        if input.throttle > 0.5 && s.speed_mps.abs() < 0.5 {
            self.stuck_steps += 1;
        } else {
            self.stuck_steps = 0;
        }
        if self.stuck_steps > Self::STUCK_S * 60 + 30 {
            self.stuck_steps = 0;
            self.back_out_steps = Self::BACK_OUT_S * 60 + 30;
            self.steer = -input.steer.signum();
        }
        input
    }
}

#[derive(Default)]
struct Measured {
    states_compared: u64,
    states_differing: u64,
    kinds_seen: BTreeSet<EntityKind>,
    near_checks: u64,
    near_missing: u64,
    traffic_checked: u64,
    traffic_differing: u64,
    captain_units_checked: u64,
    captain_units_differing: u64,
    max_vehicles: usize,
    max_police: usize,
    max_traffic_physical: usize,
}

#[test]
fn four_clients_three_minutes_in_port_vesper() {
    let secs: u64 = std::env::var("WP_VESPER_E2E_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(180);
    let data = load_data(&repo_data_dir()).expect("data");
    let t0 = std::time::Instant::now();
    let sim = vesper_sim(&data);
    let ground = city_ground(sim.sim().world()).expect("Port Vesper has terrain");
    let downtown =
        wp_world::routes::plan_circuit(sim.sim().world().map(), &DOWNTOWN_LOOP).expect("loop");
    // Round all four corners: [c3, c0, c1, c2, c3, c0] has every corner inside; drop the
    // two unrounded ends so the loop runs from the c0 fillet round to the c3 fillet.
    let c = &PORT_LOOP;
    let mut port_poly = rounded(&[c[3], c[0], c[1], c[2], c[3], c[0]], 16.0);
    port_poly.remove(0);
    port_poly.pop();
    let port_pts = resample(&port_poly, 12.0);
    let downtown_pts = resample(&downtown.polyline, 20.0);
    println!(
        "setup: Port Vesper v2 + ground colliders {} in {:.1} s; loops {:.0} m (port), {:.0} m \
         (downtown)",
        ground.len(),
        t0.elapsed().as_secs_f64(),
        port_pts.last().map_or(0.0, |p| p.2),
        downtown.length_m,
    );

    // Seats spawn by player id (1 = racer 0, 2 = racer 1, 3 = cop; the captain has no car).
    let routes = [&port_pts, &downtown_pts, &port_pts];
    let mut spawn_points = vec![pose_at(&port_pts, 0.0)];
    for (pts, s) in routes.iter().zip(SPAWN_S) {
        spawn_points.push(pose_at(pts, s));
    }
    let cfg = ServerConfig {
        racer_car: "debug_coupe".into(),
        police_car: AI_POLICE_CAR.into(),
        spawn_points,
        // 2 racers vs a chase unit + the captain (the default 40 % allows one cop in four).
        roles: wp_net::roles::RoleConfig {
            police_ratio: 0.5,
            ..Default::default()
        },
        ..ServerConfig::default()
    };
    let (cars, modes) = data;
    let mut h = Harness::with_sim(cfg, sim, cars, modes, default_damage_factory);
    h.city_ground = Some(ground);
    for (i, lat) in [20.0, 32.0, 45.0, 60.0].into_iter().enumerate() {
        let i = i as u64;
        h.add_peer(&format!("p{i}"), link(i, lat), link(100 + i, lat));
    }
    h.wait_joined();
    for (i, role) in [
        Role::Racer,
        Role::Racer,
        Role::PoliceChase,
        Role::PoliceCaptain,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(h.peers[i].client.player().map(|p| p.0), Some(i as u32 + 1));
        // The captain sits down first (otherwise the seat is offered to the chase unit).
        if role == Role::PoliceCaptain {
            h.request_role(i, role);
            h.run_secs(0.5);
        }
    }
    for i in 0..3 {
        let role = if i == 2 {
            Role::PoliceChase
        } else {
            Role::Racer
        };
        h.request_role(i, role);
    }
    h.run_secs(1.0);
    for (i, role) in [
        Role::Racer,
        Role::Racer,
        Role::PoliceChase,
        Role::PoliceCaptain,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(h.peers[i].client.role(), role, "p{i} role");
    }
    let racer0 = h.peers[0].client.own_vehicle().expect("racer 0 car");
    let racer1 = h.peers[1].client.own_vehicle().expect("racer 1 car");
    let cop = h.peers[2].client.own_vehicle().expect("cop car");
    let cop_unit = h
        .server
        .sim()
        .police_unit(cop)
        .expect("cop seat is a police unit");

    let mut bots = [
        BotDriver::new(11, route_from(routes[0], SPAWN_S[0])),
        BotDriver::new(12, route_from(routes[1], SPAWN_S[1])),
        BotDriver::new(13, route_from(routes[2], SPAWN_S[2])),
    ];
    bots[0].crash_chance_per_s = 0.0;
    bots[1].crash_chance_per_s = 0.08;
    bots[2].crash_chance_per_s = 0.0;

    let total_steps = secs * 60;
    // Racer 0 gives up two thirds into the run (or 50 s after its pursuit started).
    let give_up_step = total_steps * 2 / 3;
    let mut give_up_at: Option<u64> = None;
    let mut pursuit_started_step: Option<u64> = None;
    let mut cop_unstick = Unstick::default();
    let mut cop_was_close = false;
    // Cop tool requests: seq → 0 roadblock / 1 spikes, replies, success per tool.
    let mut cop_tool_seqs: BTreeMap<u32, usize> = BTreeMap::new();
    let mut cop_tool_replies: Vec<(usize, Result<(), wp_contracts::CaptainError>)> = Vec::new();
    let mut cop_tool_ok = [false; 2];
    let mut cop_tool_tries = 0;
    let mut captain_seqs: Vec<u32> = Vec::new();
    let mut captain_acted = false;
    let reconnect = (total_steps / 4, total_steps / 4 + 180);
    let mut reconnected = false;

    // Everything the clients were told, per client (the client keeps a bounded queue).
    let mut events: Vec<Vec<SimEvent>> = vec![Vec::new(); 4];
    // Step at which p0 saw racer 0's pursuit end.
    let mut capture_step = None;
    let mut history: VecDeque<(u64, BTreeMap<u32, QVehicle>)> = VecDeque::new();
    let mut positions: VecDeque<(u64, BTreeMap<u32, [f32; 3]>)> = VecDeque::new();
    let mut traffic_history: BTreeMap<u64, BTreeMap<u32, TrafficAgentSnapshot>> = BTreeMap::new();
    let mut m = Measured::default();
    let mut lattice_history: VecDeque<BTreeMap<u32, u64>> = VecDeque::new();

    for step in 0..total_steps {
        // --- Scripted players (inputs from what each client knows) ---------------------
        let racer0_remote = h.peers[2]
            .client
            .remotes()
            .get(&racer0.0)
            .and_then(|r| r.buffer.latest().map(|(_, q)| q.decode()));
        for (i, bot) in bots.iter_mut().enumerate() {
            let Some(s) = h.peers[i].client.predicted_state() else {
                continue;
            };
            let input = match i {
                0 => {
                    if give_up_at.is_some() {
                        drive(bot, &s, 0.0)
                    } else {
                        drive(bot, &s, 14.0)
                    }
                }
                1 => drive(bot, &s, 17.0),
                _ => match &racer0_remote {
                    // Close: drive at a point 9 m behind racer 0 (as the cop's client sees it)
                    // and match its speed; stop behind it when it has stopped.
                    Some(r) if dist2(s.pos, r.pos) < 70.0 * 70.0 => {
                        cop_was_close = true;
                        let f = wp_net::quant::rotate(r.rot, [0.0, 1.0, 0.0]);
                        let aim = [r.pos[0] - 9.0 * f[0], r.pos[1] - 9.0 * f[1]];
                        let gap = dist2(s.pos, [aim[0], aim[1], 0.0]).sqrt();
                        let v = r.speed_mps.max(0.0);
                        let target = if gap < 4.0 && v < 2.0 {
                            0.0
                        } else {
                            (v + 0.5 * gap).clamp(0.0, 24.0)
                        };
                        let mut chase = BotDriver::new(1, vec![aim]);
                        chase.crash_chance_per_s = 0.0;
                        drive(&mut chase, &s, target)
                    }
                    // Far: follow the loop (racer 0's loop too), from the nearest point on
                    // it when the cop just fell back (e.g. after backing out of a wall).
                    _ => {
                        if std::mem::take(&mut cop_was_close) {
                            let (_, _, s_near) = routes[2]
                                .iter()
                                .min_by(|a, b| {
                                    dist2([a.0[0], a.0[1], 0.0], s.pos)
                                        .total_cmp(&dist2([b.0[0], b.0[1], 0.0], s.pos))
                                })
                                .expect("route points");
                            *bot = BotDriver::new(13, route_from(routes[2], *s_near));
                            bot.crash_chance_per_s = 0.0;
                        }
                        drive(bot, &s, 18.0)
                    }
                },
            };
            // Since the differentials (round 12) the cop can run wide where racer 0 turns
            // off the loop and wedge itself against the kerb wall: back out and carry on.
            let input = if i == 2 {
                cop_unstick.filter(&s, input)
            } else {
                input
            };
            h.peers[i].manual_input = input;
        }
        if give_up_at.is_none()
            && (step >= give_up_step || pursuit_started_step.is_some_and(|t| step >= t + 50 * 60))
        {
            give_up_at = Some(step);
        }

        // --- Server-side setup: heat on the racers (stands in for violations) ----------
        if step == 3 * 60 {
            h.server
                .sim_mut()
                .sim_mut()
                .police_mut()
                .set_heat(racer0, 55.0);
        }
        if step == 10 * 60 {
            h.server
                .sim_mut()
                .sim_mut()
                .police_mut()
                .set_heat(racer1, 40.0);
        }

        // --- Reconnect: racer 1 drops its link and comes back with its token ----------
        if step == reconnect.0 {
            h.drop_link(1);
        }
        if step == reconnect.1 {
            let token = h.peers[1].client.token();
            h.connect(1, token);
        }

        h.step();

        for (i, p) in h.peers.iter_mut().enumerate() {
            events[i].extend(p.client.game_events.drain(..));
        }
        if capture_step.is_none()
            && events[0].iter().any(|e| {
                matches!(e, SimEvent::Police(PoliceEvent::PursuitEnded { racer, .. })
                    if *racer == racer0)
            })
        {
            capture_step = Some(step);
        }
        let tick = h.server.sim().tick();

        // --- Police tools and orders, once the pursuit is on ---------------------------
        if pursuit_started_step.is_none()
            && events[2].iter().any(|e| {
                matches!(e, SimEvent::Police(PoliceEvent::PursuitStarted { racer, .. })
                    if *racer == racer0)
            })
        {
            pursuit_started_step = Some(step);
        }
        if let Some(t) = pursuit_started_step {
            let now = h.now_ms;
            if !captain_acted && step >= t + 60 {
                captain_acted = true;
                let c = &mut h.peers[3].client;
                captain_seqs.push(c.captain_command(
                    CaptainCommand::AssignUnit {
                        unit: UnitId(PLAYER_UNIT_BASE + cop.0),
                        order: UnitOrder::Pursue { target: racer0 },
                    },
                    now,
                ));
                captain_seqs.push(c.captain_command(
                    CaptainCommand::SetPriorityTarget {
                        target: Some(racer0),
                    },
                    now,
                ));
            }
            // The cop calls in a roadblock and a spike strip ahead of racer 0, again every 3 s
            // while the police refuse the site (blind corner, no site on the path).
            if step >= t + 4 * 60 && (step - t).is_multiple_of(3 * 60) && cop_tool_tries < 80 {
                let c = &mut h.peers[2].client;
                for (k, ok) in cop_tool_ok.iter().enumerate() {
                    if *ok {
                        continue;
                    }
                    let cmd = if k == 0 {
                        CaptainCommand::RequestRoadblock {
                            target: racer0,
                            road: None,
                        }
                    } else {
                        CaptainCommand::RequestSpikes {
                            target: racer0,
                            road: None,
                        }
                    };
                    cop_tool_seqs.insert(c.captain_command(cmd, now), k);
                    cop_tool_tries += 1;
                }
            }
        }
        for (seq, r) in h.peers[2].client.captain_replies.drain(..) {
            if let Some(k) = cop_tool_seqs.get(&seq) {
                cop_tool_ok[*k] |= r.is_ok();
                cop_tool_replies.push((*k, r));
            }
        }
        if step == reconnect.1 + 120 {
            assert_eq!(
                h.peers[1].client.own_vehicle(),
                Some(racer1),
                "reconnect kept racer 1's car"
            );
            reconnected = true;
        }

        if std::env::var("WP_VESPER_DEBUG").is_ok() && step % 300 == 0 {
            let st: BTreeMap<EntityId, VehicleState> =
                h.server.sim().vehicles().into_iter().collect();
            let police = h.server.sim().sim().police();
            let u = police.units().iter().find(|u| u.id == cop_unit);
            println!(
                "dbg step {step}: r0 {:?} v {:.1} lvl {:?} | cop {:?} v {:.1} unit {:?} | p0 holds cop {:?} p2 holds r0 {}",
                st.get(&racer0).map(|s| s.pos),
                st.get(&racer0).map_or(0.0, |s| s.speed_mps),
                police.wanted_level(racer0),
                st.get(&cop).map(|s| s.pos),
                st.get(&cop).map_or(0.0, |s| s.speed_mps),
                u.map(|u| (u.body.pos, u.wrecked, format!("{:?}", u.assignment))),
                h.peers[0].client.remotes().get(&cop.0).map(|r| (r.kind, r.buffer.latest().map(|(t, _)| t), tick)),
                h.peers[2].client.remotes().contains_key(&racer0.0),
            );
        }

        if std::env::var("WP_VESPER_DEBUG").is_ok() && step % 120 == 0 {
            let mode = h.modes[&DamageMode::Arcade].clone();
            for v in h.server.sim().sim().vehicles() {
                let pristine = wp_contracts::DamageSim::node_positions(&v.damage)
                    .iter()
                    .zip(&v.damage.lattice().nodes)
                    .all(|(a, b)| *a == b.pos_m);
                if !pristine {
                    continue;
                }
                let car = &h.cars[&v.car_id];
                let mut fresh = wp_damage::DamageModel::new(&car.lattice, &car.materials);
                let mut sink = Vec::new();
                let n = h.server.sim().damage_steps(v.id).unwrap();
                for _ in 0..n {
                    wp_contracts::DamageSim::apply_contacts(
                        &mut fresh,
                        &[],
                        &mode,
                        wp_contracts::PHYSICS_DT,
                        &mut sink,
                    );
                }
                if fresh.state_hash() != v.damage.state_hash() {
                    println!(
                        "dbg STEPS step {step}: vehicle {:?} {:?} steps {n} disagree",
                        v.id, v.kind
                    );
                }
            }
        }

        // --- Server truth history ------------------------------------------------------
        let q = quantised(h.server.sim());
        m.max_vehicles = m.max_vehicles.max(q.len());
        {
            let sim = h.server.sim().sim();
            m.max_police = m.max_police.max(
                sim.vehicles()
                    .iter()
                    .filter(|v| v.police_unit.is_some())
                    .count(),
            );
            m.max_traffic_physical = m.max_traffic_physical.max(
                sim.vehicles()
                    .iter()
                    .filter(|v| v.traffic_agent.is_some())
                    .count(),
            );
        }
        history.push_back((tick, q));
        positions.push_back((
            tick,
            h.server
                .sim()
                .vehicles()
                .into_iter()
                .map(|(e, s)| (e.0, s.pos))
                .collect(),
        ));
        while positions.len() > 240 {
            positions.pop_front();
        }
        while history.len() > 240 {
            history.pop_front();
        }
        traffic_history.insert(
            tick,
            h.server
                .sim()
                .traffic_agents()
                .into_iter()
                .map(|a| (a.id, a))
                .collect(),
        );
        while traffic_history.len() > 120 {
            traffic_history.pop_first();
        }
        if step % 60 == 0 {
            let hashes: BTreeMap<u32, u64> = h
                .server
                .sim()
                .sim()
                .vehicles()
                .iter()
                .map(|v| (v.id.0, node_hash(v)))
                .collect();
            lattice_history.push_back(hashes);
            while lattice_history.len() > 4 {
                lattice_history.pop_front();
            }
        }

        // --- What the clients hold vs the server, twice a second -----------------------
        if step % 30 != 29 {
            continue;
        }
        let server_states: BTreeMap<u32, VehicleState> = h
            .server
            .sim()
            .vehicles()
            .into_iter()
            .map(|(e, s)| (e.0, s))
            .collect();
        for (i, p) in h.peers.iter().enumerate() {
            if !p.online {
                continue;
            }
            for (e, r) in p.client.remotes() {
                let Some((t, q)) = r.buffer.latest() else {
                    continue;
                };
                let Some((_, at)) = history.iter().find(|(ht, _)| *ht == t) else {
                    continue;
                };
                let Some(sq) = at.get(e) else { continue };
                m.kinds_seen.insert(r.kind);
                m.states_compared += 1;
                if sq != q {
                    m.states_differing += 1;
                    eprintln!("p{i} holds a different state for {e} ({:?}) at {t}", r.kind);
                }
            }
            // Kinematic traffic.
            if let Some(held) = traffic_history.get(&p.client.traffic_tick()) {
                for a in p.client.traffic() {
                    m.traffic_checked += 1;
                    let same = held.get(&a.id).is_some_and(|truth| {
                        QTrafficAgent::from_snapshot(truth) == QTrafficAgent::from_snapshot(a)
                    });
                    if !same {
                        m.traffic_differing += 1;
                    }
                }
            }
            // Vehicles well inside the near radius for a second are known.
            let Some(own) = p.client.own_vehicle().and_then(|v| server_states.get(&v.0)) else {
                continue;
            };
            let latest = &history.back().expect("history").1;
            let old = &history[history.len().saturating_sub(60)].1;
            for e in latest.keys().filter(|e| old.contains_key(e)) {
                if Some(EntityId(*e)) == p.client.own_vehicle() {
                    continue;
                }
                if dist2(server_states[e].pos, own.pos) < 60.0 * 60.0 {
                    m.near_checks += 1;
                    if !p.client.remotes().contains_key(e) {
                        m.near_missing += 1;
                    }
                }
            }
        }
        // The captain's map: every unit where the server had it at the map's tick.
        let map = &h.peers[3].client.captain;
        if let Some((_, at)) = positions.iter().find(|(t, _)| *t == map.tick) {
            for u in map.units.values() {
                let Some(p) = at.get(&u.entity) else { continue };
                m.captain_units_checked += 1;
                if (p[0] * 10.0).round() as i32 != u.x_dm || (p[1] * 10.0).round() as i32 != u.y_dm
                {
                    m.captain_units_differing += 1;
                }
            }
        }
    }

    // --- Settle: everyone stops for 3 s, then replicas must equal the server's lattices --
    for p in h.peers.iter_mut() {
        p.manual_input = VehicleInput {
            brake: 1.0,
            handbrake: 1.0,
            ..Default::default()
        };
    }
    h.run_secs(3.0);
    let server_hash: BTreeMap<u32, u64> = h
        .server
        .sim()
        .sim()
        .vehicles()
        .iter()
        .map(|v| (v.id.0, node_hash(v)))
        .collect();
    let steady = |e: &u32| {
        lattice_history
            .iter()
            .all(|hs| hs.get(e) == server_hash.get(e))
    };
    let (mut replicas_checked, mut replicas_differing, mut replicas_dented) = (0, 0, 0);
    let mut shared: BTreeMap<u32, Vec<Vec<[f32; 3]>>> = BTreeMap::new();
    for (i, p) in h.peers.iter().enumerate() {
        for e in p
            .client
            .remotes()
            .keys()
            .copied()
            .chain(p.client.own_vehicle().map(|v| v.0))
        {
            let (Some(nodes), Some(truth)) = (
                p.client.replica_nodes(e),
                h.server.sim().lattice_nodes(EntityId(e)),
            ) else {
                continue;
            };
            if !steady(&e) || p.client.pending_resyncs().contains(&e) {
                continue;
            }
            replicas_checked += 1;
            let rest = h.server.sim().lattice_rest(EntityId(e)).expect("rest");
            if truth.iter().zip(&rest).any(|(a, b)| a != b) {
                replicas_dented += 1;
            }
            if nodes != truth.as_slice() {
                replicas_differing += 1;
                eprintln!("p{i}: replica of {e} differs from the server");
            }
            shared.entry(e).or_default().push(nodes.to_vec());
        }
    }
    let mut state_sizes: Vec<(u32, usize, usize)> = h
        .server
        .sim()
        .vehicles()
        .into_iter()
        .filter_map(|(e, _)| {
            let st = h.server.sim().damage_state(e)?;
            let nodes = h.server.sim().lattice_nodes(e)?;
            let rest = h.server.sim().lattice_rest(e)?;
            let snap = wp_net::lattice::LatticeSnapshot::capture(&nodes, &rest);
            (!snap.is_pristine()).then_some((e.0, snap.displaced.len() * 12, st.len()))
        })
        .collect();
    state_sizes.sort_by_key(|x| std::cmp::Reverse(x.1 + x.2));
    println!("dented cars (entity, node bytes, damage state bytes): {state_sizes:?}");
    let seen_by_several = shared.values().filter(|v| v.len() >= 2).count();

    // --- Report -----------------------------------------------------------------------
    let stats = h.server.stats();
    println!("bytes by message tag: {:x?}", stats.bytes_by_tag);
    let (mut total_hash, mut total_mismatch) = (0, 0);
    for (i, p) in h.peers.iter().enumerate() {
        let s = p.client.stats();
        let down: Vec<u64> = p.down_per_second.iter().skip(2).copied().collect();
        let mean = down.iter().sum::<u64>() as f64 / down.len().max(1) as f64;
        let worst = down.iter().copied().max().unwrap_or(0);
        println!(
            "p{i} {:?}: rtt {:.0} ms, reconciles {}, mispredictions {}, rms {:.3} m, max {:.2} \
             m, resets {}; remotes {}, traffic {}; hash {}/{} mismatched (contact-only {}), \
             resyncs {}, max resync {} ticks, full restores {}, missing baselines {}; down mean {mean:.0} B/s worst {worst} B/s; events {}",
            p.client.role(),
            s.rtt_ms,
            s.reconciles,
            s.mispredictions,
            s.correction_rms_m(),
            s.max_correction_m,
            s.prediction_resets,
            p.client.remotes().len(),
            p.client.traffic().len(),
            s.hash_mismatches,
            s.hash_checks,
            s.hash_mismatches_contact_only,
            s.resyncs,
            s.max_resync_ticks,
            s.full_restores,
            s.missing_baselines,
            events[i].len(),
        );
        let mut top: Vec<(usize, u64)> = down.iter().copied().enumerate().collect();
        top.sort_by_key(|x| std::cmp::Reverse(x.1));
        top.truncate(3);
        println!("p{i} busiest seconds (index, bytes): {top:?}");
        total_hash += s.hash_checks;
        total_mismatch += s.hash_mismatches;
        assert_eq!(s.decode_errors, 0, "p{i} decode errors");
        assert_eq!(
            s.hash_mismatches_contact_only, 0,
            "p{i}: a replica built only from contacts diverged"
        );
        assert!(
            worst as usize <= DOWNLINK_BUDGET_BPS,
            "p{i} worst second {worst} B/s"
        );
        if p.client.own_vehicle().is_some() {
            assert!(
                s.correction_rms_m() < 0.5,
                "p{i} rms correction {}",
                s.correction_rms_m()
            );
            // Racer 1 was offline for 3 s: its predictor restarts once on the way back.
            let allowed = u64::from(i == 1);
            assert!(s.prediction_resets <= allowed, "p{i} prediction resets");
            // ~30 own states a second (racer 1 was offline for 3 s).
            assert!(s.reconciles > secs * 28, "p{i} reconciles {}", s.reconciles);
        }
    }
    let mut ms = h.server_ms[h.server_ms.len() - total_steps as usize..].to_vec();
    ms.sort_by(f64::total_cmp);
    let avg = ms.iter().sum::<f64>() / ms.len() as f64;
    let p99 = ms[ms.len() * 99 / 100];
    let max = ms[ms.len() - 1];
    println!(
        "server net tick (4 physics ticks + output, 4 clients, up to {} vehicles: {} police \
         cars, {} physical traffic): avg {avg:.3} ms, p99 {p99:.3} ms, max {max:.2} ms; \
         downlink max {:.0} B/s",
        m.max_vehicles, m.max_police, m.max_traffic_physical, stats.downlink_bps_max
    );
    println!(
        "agreement: {} vehicle states compared ({} differ), kinds {:?}; near vehicles known \
         {}/{}; kinematic traffic {} checked ({} differ); captain map {} units checked ({} \
         differ); replicas after settling {replicas_checked} checked ({replicas_differing} \
         differ, {replicas_dented} dented, {seen_by_several} vehicles held by 2+ clients); \
         lattice hashes {total_mismatch}/{total_hash} mismatched",
        m.states_compared,
        m.states_differing,
        m.kinds_seen,
        m.near_checks - m.near_missing,
        m.near_checks,
        m.traffic_checked,
        m.traffic_differing,
        m.captain_units_checked,
        m.captain_units_differing,
    );

    // --- Pursuit and capture --------------------------------------------------------
    let started: Vec<Option<UnitId>> = events[3]
        .iter()
        .filter_map(|e| match e {
            SimEvent::Police(PoliceEvent::PursuitStarted { racer, unit }) if *racer == racer0 => {
                Some(*unit)
            }
            _ => None,
        })
        .collect();
    println!(
        "pursuit of racer 0 started by {started:?} (player cop unit {cop_unit:?}) at step \
         {pursuit_started_step:?}; racer 0 gave up at step {give_up_at:?}; captain replies \
         {:?}",
        h.peers[3].client.captain_replies,
    );
    assert_eq!(
        started.first().copied().flatten(),
        Some(cop_unit),
        "the player cop started the pursuit"
    );
    for (i, ev) in events.iter().enumerate() {
        let capture = ev.iter().find_map(|e| match e {
            SimEvent::Police(PoliceEvent::PursuitEnded {
                racer,
                outcome,
                capturer,
                assists,
                ..
            }) if *racer == racer0 => Some((*outcome, *capturer, assists.clone())),
            _ => None,
        });
        println!("p{i} sees the end of racer 0's pursuit: {capture:?}");
        // Racer 1's link is down from reconnect.0 to reconnect.1: events sent then are not
        // replayed on reconnect, so p1 misses a capture that lands in that window (police
        // round 12c caught racer 0 at step ≈ 2746, inside it). The other three must see it.
        let offline = capture_step.is_some_and(|t| t + 5 >= reconnect.0 && t <= reconnect.1 + 5);
        if i == 1 && capture.is_none() && offline {
            println!("p1 was offline at the capture (step {capture_step:?})");
            continue;
        }
        let (outcome, capturer, assists) =
            capture.unwrap_or_else(|| panic!("p{i} never saw racer 0's pursuit end"));
        assert_eq!(outcome, PursuitOutcome::Captured, "p{i}");
        assert!(
            capturer == Some(cop_unit) || assists.contains(&cop_unit),
            "p{i}: the player cop took part in the capture ({capturer:?}, {assists:?})"
        );
    }
    let replies: BTreeMap<u32, _> = h.peers[3].client.captain_replies.iter().copied().collect();
    for seq in &captain_seqs {
        assert_eq!(replies.get(seq), Some(&Ok(())), "captain command {seq}");
    }
    println!("cop tool replies (0 = roadblock, 1 = spikes): {cop_tool_replies:?}");
    assert!(!cop_tool_replies.is_empty(), "cop tool requests answered");
    for (_, r) in &cop_tool_replies {
        assert!(
            !matches!(r, Err(wp_contracts::CaptainError::Unavailable)),
            "chase units may call in tools: {r:?}"
        );
    }
    assert!(
        cop_tool_ok.iter().any(|ok| *ok),
        "a roadblock or spike strip was placed: {cop_tool_replies:?}"
    );
    assert_eq!(
        h.peers[2].client.unit_order,
        Some(UnitOrder::Pursue { target: racer0 }),
        "the captain's order reached the player cop"
    );
    assert!(reconnected);

    // --- Agreement and budgets -------------------------------------------------------
    assert!(
        m.max_police >= 2,
        "AI police cars spawned: {}",
        m.max_police
    );
    assert!(m.max_traffic_physical >= 1, "physical traffic");
    for k in [
        EntityKind::Racer,
        EntityKind::PoliceChase,
        EntityKind::AiPolice,
    ] {
        assert!(m.kinds_seen.contains(&k), "no client held a {k:?}");
    }
    assert!(m.states_compared > 1000);
    assert_eq!(m.states_differing, 0, "clients disagree with the server");
    assert_eq!(m.near_missing, 0, "nearby vehicles not replicated");
    assert!(m.traffic_checked > 100, "kinematic traffic held");
    assert_eq!(m.traffic_differing, 0, "kinematic traffic differs");
    assert!(m.captain_units_checked > 100);
    assert_eq!(m.captain_units_differing, 0, "captain map differs");
    assert!(replicas_checked > 20 && replicas_dented >= 1 && seen_by_several >= 2);
    assert_eq!(
        replicas_differing, 0,
        "damage replicas differ after settling"
    );
    assert!(total_hash > 0);
    assert!(
        (total_mismatch as f64) < 0.05 * total_hash as f64,
        "{total_mismatch}/{total_hash} lattice hash mismatches"
    );
    assert!(stats.downlink_bps_max <= DOWNLINK_BUDGET_BPS as f64);
    if !cfg!(debug_assertions) {
        assert!(avg < 8.0, "server net tick avg {avg} ms");
        assert!(p99 < 1000.0 / 60.0, "server net tick p99 {p99} ms");
    }
}
