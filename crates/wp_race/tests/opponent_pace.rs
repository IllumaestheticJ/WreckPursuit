//! AI race pace, measured (playtest row 99: "the AI in the AI races are quite dumb and slow").
//!
//! Every picker race (plus the procedural city sprint) raced in the full `wp_sim` loop with
//! one driver skill in every seat, everyone else's personality at its default. For each car:
//! the judged time, the car's **ideal** time (its own speed profile on its own racing line with
//! the car's *measured* limits, used to 100 %, standing start) and where the difference went.
//!
//! Loss attribution, per physics tick, `dt · (1 − v / v_ideal(s))` booked to:
//! - `usage`: the driver's own target is below the ideal profile (skill: how much grip and
//!   braking the driver dares to use) — the intended difference between skill levels;
//! - `track`: below the driver's own target (throttle lag, traction, PID, braking early);
//! - `racer` / `civ` / `police`: a car ahead set the target;
//! - `backout`, `unstable`, `mode` (rejoin / park caps, off-line paths), `signal`;
//! - `rejoin` / `reverse`: recovering from an off or being stuck.
//!
//! `cargo test --release -p wp_race --test opponent_pace -- --ignored --nocapture`
//! Knobs: `WP_PACE_RACES=loop9_circuit,hollins_sprint`, `WP_PACE_SKILLS=0.25,0.95`,
//! `WP_PACE_POLICE=1` (keep the scenario's police; default off), `WP_PACE_DENSITY=0.0`,
//! `WP_PACE_TOD=23.0` (time of day, h),
//! `WP_PACE_SOLO=1` (one car only: the pole car), `WP_PACE_GEOM=1` (geometry-only limits,
//! the pre-row-99 behaviour), `WP_PACE_TRACE=<grid idx>` (2 Hz trace of that car).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use simcli::run::build_sim;
use simcli::{load_scenario, FsData, RunOptions};
use wp_contracts::{DamageSim, EntityId, LoadedCar, SimEvent, VehicleSim, PHYSICS_DT};
use wp_opponent::{
    CarCapability, DriverProfile, SimPolice, SpeedLimit, SpeedProfile, Tactic, Usage, VehicleObs,
};
use wp_race::bridge::{plan_city_race, CityRace};
use wp_race::catalog;
use wp_race::entrants::{AiEntrant, AiEntrants};
use wp_race::{EventId, Legality, RaceData, RaceJudge, RaceKind, RacerEntry, RacerSample};
use wp_sim::{Sim, VehicleKind};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn obs(sim: &Sim) -> Vec<VehicleObs> {
    sim.vehicles()
        .iter()
        .map(|v| VehicleObs {
            entity: v.id,
            state: v.vehicle.state(),
            effects: v.damage.handling_effects(),
            size_m: [
                v.vehicle.def().body_size_m[0],
                v.vehicle.def().body_size_m[1],
            ],
        })
        .collect()
}

/// Every race the pace sweep covers: picker ids plus `city_sprint`.
pub const RACES: [&str; 5] = [
    "loop9_circuit",
    "hollins_sprint",
    "lantern_checkpoint",
    "grid_highway_sprint",
    "city_sprint",
];

/// Pace options.
#[derive(Debug, Clone)]
pub struct PaceOpts {
    /// Keep the scenario's police.
    pub police: bool,
    /// Traffic density override.
    pub density: Option<f32>,
    /// Only the pole car races.
    pub solo: bool,
    /// Geometry-only capability (old behaviour).
    pub geometry: bool,
    /// Grid index to trace.
    pub trace: Option<usize>,
    /// Scenario seed override.
    pub seed: Option<u64>,
    /// Time of day override, h (`WP_PACE_TOD`).
    pub tod: Option<f32>,
}

impl PaceOpts {
    fn from_env() -> Self {
        let var = |k: &str| std::env::var(k).ok();
        Self {
            police: var("WP_PACE_POLICE").is_some(),
            density: var("WP_PACE_DENSITY").and_then(|v| v.parse().ok()),
            solo: var("WP_PACE_SOLO").is_some(),
            geometry: var("WP_PACE_GEOM").is_some(),
            trace: var("WP_PACE_TRACE").and_then(|v| v.parse().ok()),
            seed: var("WP_PACE_SEED").and_then(|v| v.parse().ok()),
            tod: var("WP_PACE_TOD").and_then(|v| v.parse().ok()),
        }
    }
}

/// Loss buckets (s).
const BUCKETS: [&str; 11] = [
    "usage", "track", "racer", "civ", "veh", "backout", "unstable", "mode", "signal", "rejoin",
    "reverse",
];

/// One car's result.
#[derive(Debug, Clone)]
pub struct CarPace {
    /// Scenario entity id.
    pub entity: String,
    /// Car id.
    pub car: String,
    /// Judged time (None = DNF / not finished).
    pub time_s: Option<f32>,
    /// Ideal time (measured limits used 100 %, standing start).
    pub ideal_s: f32,
    /// The driver's own profile time (standing start).
    pub own_s: f32,
    /// Loss per bucket, s.
    pub loss: [f32; BUCKETS.len()],
    /// Times the car entered Rejoin / Reverse.
    pub rejoins: u32,
    /// Reverse manoeuvres.
    pub reverses: u32,
    /// Static-geometry contacts (walls, kerbs, props), ≥ 400 N·s.
    pub wall_hits: u32,
    /// Mean |n − n_cmd| at speed, m.
    pub track_err_m: f32,
    /// Share of racing time out on the opposing lanes (0..1).
    pub opposing_frac: f32,
    /// Spread of the lateral position while racing (standard deviation of n), m.
    pub lateral_sd_m: f32,
    /// Overtakes completed per km of route.
    pub passes_per_km: f32,
    /// Nose-to-nose stand-offs (standing facing a standing car in my path): how many, and the
    /// time spent in them, s.
    pub head_on: (u32, f32),
    /// Of those, the ones out on the opposing lanes (a racer met on the wrong side of the road
    /// by a car coming the other way or out of a side road).
    pub head_on_opposing: u32,
    /// The judge's contact counts: (civilians / traffic, all cars).
    pub contacts: (u32, u32),
    /// Completed passes made between two cars (lane-splitting).
    pub splits: u32,
    /// Mean civilians within 300 m of the car (sampled at 1 Hz): (my direction, opposing).
    pub civ_near: (f32, f32),
    /// Lane-splits given up (tucked back in).
    pub split_aborts: u32,
    /// Judge status.
    pub status: String,
}

#[allow(clippy::too_many_arguments)]
fn standing_time(
    profile_v: &[f32],
    step_len: &[f32],
    cap: &CarCapability,
    accel: f32,
    s_from: usize,
    s_to: usize,
    m: usize,
    closed: bool,
) -> f32 {
    // Forward pass from a standstill at `s_from`, capped by the profile.
    let mut v = 0.0f32;
    let mut t = 0.0;
    let mut i = s_from;
    let mut steps = 0;
    let total = if closed {
        (s_to + m - s_from) % m
    } else {
        s_to.saturating_sub(s_from)
    };
    while steps < total {
        let j = if closed {
            (i + 1) % m
        } else {
            (i + 1).min(m - 1)
        };
        let a = wp_opponent::drive_accel(cap, accel, v);
        let v_next = (v * v + 2.0 * a * step_len[i]).sqrt().min(profile_v[j]);
        t += step_len[i] / (0.5 * (v + v_next)).max(0.5);
        v = v_next;
        i = j;
        steps += 1;
    }
    t
}

/// `WP_PACE_ASSISTS=esc_off|wheel|ai_nofilter`: assists for the AI cars.
fn assists_override() -> Option<wp_contracts::AssistProfile> {
    let base = wp_contracts::AssistProfile::ai();
    match std::env::var("WP_PACE_ASSISTS").ok()?.as_str() {
        "esc_off" => Some(wp_contracts::AssistProfile { esc: false, ..base }),
        "tcs_off" => Some(wp_contracts::AssistProfile {
            tcs: wp_contracts::TcsLevel::Off,
            ..base
        }),
        "both_off" => Some(wp_contracts::AssistProfile {
            esc: false,
            tcs: wp_contracts::TcsLevel::Off,
            ..base
        }),
        _ => None,
    }
}

/// The seat's picker personality (scenario entity `i`: 0 = the player's seat, which gets
/// the default driver, as on autopilot).
fn personality(id: &str, i: usize) -> DriverProfile {
    catalog::race(id)
        .filter(|_| i > 0)
        .and_then(|d| d.ai.get(i - 1).map(|a| a.profile))
        .unwrap_or_default()
}

fn plan(id: &str, world: &wp_world::World) -> (CityRace, RaceKind, bool, u64) {
    if id == "city_sprint" {
        let race = plan_city_race(
            world,
            [60.55, -437.0, 0.0],
            [0.0, 1.0],
            [60.55, -230.0, 0.0],
            3.0,
        )
        .unwrap();
        return (race, RaceKind::Sprint, false, 15);
    }
    let def = catalog::race(id).unwrap();
    let race = catalog::plan_course(world, &def).unwrap();
    (
        race,
        def.kind,
        def.legality == Legality::Sanctioned,
        def.seed,
    )
}

/// Race `id` with every seat at `skill`.
pub fn race_pace(
    id: &str,
    skill: f32,
    opts: &PaceOpts,
    caps: &mut BTreeMap<String, CarCapability>,
) -> Vec<CarPace> {
    let path = if id == "city_sprint" {
        repo().join("apps/simcli/scenarios/race/city_sprint_police.ron")
    } else {
        repo().join(format!("apps/simcli/scenarios/race/pick_{id}.ron"))
    };
    race_pace_in(load_scenario(&path).unwrap(), id, skill, opts, caps)
}

/// [`race_pace`] on a given scenario (a picker race's, or a pace scenario's from
/// `apps/simcli/scenarios/opponent/`).
pub fn race_pace_in(
    mut sc: simcli::Scenario,
    id: &str,
    skill: f32,
    opts: &PaceOpts,
    caps: &mut BTreeMap<String, CarCapability>,
) -> Vec<CarPace> {
    if !opts.police {
        sc.police_presence = Some(0.0);
        sc.police_cars = false;
    }
    if let Some(d) = opts.density {
        sc.traffic_density = Some(d);
    }
    if let Some(s) = opts.seed {
        sc.seed = s;
    }
    if let Some(t) = opts.tod {
        sc.time_of_day_h = t;
    }
    // Picker scenarios list the player first, the AI field (pole first) after; the judge
    // enters the AI field first.
    let picker = id != "city_sprint";
    let n = sc.entities.len();
    let order: Vec<usize> = if picker {
        (1..n).chain(std::iter::once(0)).collect()
    } else {
        (0..n).collect()
    };
    let data_src = FsData::new(repo().join("data"));
    let built = build_sim(&sc, &data_src, &RunOptions::default()).unwrap();
    let mut sim = built.sim;
    // The AI drives every seat, the player's car included: no player steering aids anywhere.
    for &id in &built.ids {
        sim.set_player_steering(id, Some(false)).unwrap();
    }
    let ids = built.ids;
    let (race, kind, closed, seed) = plan(id, sim.world());
    let data = RaceData::embedded();
    let mut judge = RaceJudge::new(EventId(1), data.rules.for_kind(kind), race.course.clone());
    let mut entrants = Vec::new();
    let mut measured = Vec::new();
    let mut car_ids = Vec::new();
    let mut entity_ids = Vec::new();
    for &i in &order {
        let spec = &sc.entities[i];
        let car: &LoadedCar = &built.cars[&spec.car];
        judge
            .enter(RacerEntry {
                racer: ids[i],
                redline_rpm: car.def.engine.redline_rpm,
            })
            .unwrap();
        // The player seat stays a `VehicleKind::Player` car (AI-driven) as in the browser, so
        // traffic spawns round it (with every seat an AI racer it spreads over the whole city:
        // Loop 9 at night had none near the field). Lead, 2026-10-22.
        if !(picker && i == 0) {
            sim.set_vehicle_kind(ids[i], VehicleKind::AiRacer).unwrap();
        }
        if let Some(a) = assists_override() {
            sim.set_assists(ids[i], a).unwrap();
        }
        let m = *caps
            .entry(spec.car.clone())
            .or_insert_with(|| CarCapability::for_car(car));
        measured.push(m);
        car_ids.push(spec.car.clone());
        entity_ids.push(spec.id.clone());
        entrants.push(AiEntrant {
            racer: ids[i],
            profile: DriverProfile {
                skill,
                ..personality(id, i)
            },
            capability: if opts.geometry {
                CarCapability::geometry_only(car)
            } else {
                m
            },
        });
    }
    // Solo: only the pole car is driven; the rest stay parked on the grid behind it.
    let all_racers: Vec<EntityId> = entrants.iter().map(|e| e.racer).collect();
    if opts.solo {
        entrants.truncate(1);
    }
    let entrants_profiles: Vec<DriverProfile> = entrants.iter().map(|e| e.profile).collect();
    let mut ai = AiEntrants::new(sim.world(), &race, kind, closed, &entrants, seed)
        .expect("a drivable route");
    let green = judge.start(sim.tick()).unwrap();
    let racers: Vec<EntityId> = entrants.iter().map(|e| e.racer).collect();
    let k_n = racers.len();

    // Ideal profiles (100 % usage on the measured limits) on each car's own line.
    let field = ai.field();
    let corridor = field.corridor().clone();
    let m = corridor.len();
    let ideal: Vec<SpeedProfile> = (0..k_n)
        .map(|k| {
            SpeedProfile::build(
                &corridor,
                field.line_of(k),
                &measured[k],
                &Usage {
                    lateral: 1.0,
                    braking: 1.0,
                    accel: 1.0,
                },
                f32::INFINITY,
            )
        })
        .collect();

    let mut loss = vec![[0.0f32; BUCKETS.len()]; k_n];
    let mut rejoins = vec![0u32; k_n];
    let mut reverses = vec![0u32; k_n];
    let mut walls = vec![0u32; k_n];
    let mut track_err = vec![(0.0f32, 0u32); k_n];
    // Racing ticks, ticks on the opposing lanes, Σn, Σn² (lateral line usage).
    let mut lateral = vec![(0u32, 0u32, 0.0f64, 0.0f64); k_n];
    // (stand-offs, seconds in them, in one now).
    let mut head_on = vec![(0u32, 0.0f32, false); k_n];
    let mut head_on_opp = vec![0u32; k_n];
    // Civilians within 300 m of each racer once a second, (same direction, opposing, samples).
    let mut civ_near = vec![(0u32, 0u32, 0u32); k_n];
    let mut last_tactic = vec![Tactic::Follow; k_n];
    // Last non-profile cause; the climb back to the profile afterwards is booked to it.
    let mut recovering: Vec<Option<usize>> = vec![None; k_n];
    let mut start_s: Vec<Option<f32>> = vec![None; k_n];
    let mut events: Vec<SimEvent> = Vec::new();
    let max_s = (race.route.length_m / 8.0 + 60.0) as u64;
    while sim.tick() < green + 240 * max_s {
        let tick = sim.tick();
        let inputs = ai.tick(
            &judge,
            tick,
            sim.world(),
            &obs(&sim),
            &events,
            &SimPolice(sim.police()),
        );
        for (e, inp) in inputs {
            sim.set_input(e, inp).unwrap();
        }
        for e in &all_racers[k_n..] {
            sim.set_input(
                *e,
                wp_contracts::VehicleInput {
                    handbrake: 1.0,
                    ..Default::default()
                },
            )
            .unwrap();
        }
        sim.step_fixed();
        events = sim.drain_events();
        let running_now: Vec<bool> = racers
            .iter()
            .map(|e| {
                judge
                    .results()
                    .iter()
                    .any(|r| r.racer == *e && r.status == wp_race::RacerStatus::Running)
            })
            .collect();
        for ev in &events {
            if let SimEvent::Contact {
                a,
                b,
                impulse_ns,
                rel_speed,
                ..
            } = ev
            {
                if *impulse_ns >= 400.0 && *b == wp_contracts::WORLD_ENTITY {
                    if let Some(k) = racers
                        .iter()
                        .position(|r| r == a)
                        .filter(|k| running_now[*k])
                    {
                        walls[k] += 1;
                        if opts.trace == Some(k) {
                            let d = ai.field().debug_of(k);
                            eprintln!(
                                "    WALL t={:.2} s={:.1} n={:.2} ncmd={:.2} imp={:.0} rel={:.1} tgt={:.1} lim={:?}",
                                (sim.tick().saturating_sub(green)) as f32 * PHYSICS_DT,
                                d.s, d.n, d.n_cmd, impulse_ns, rel_speed, d.v_target, d.limit
                            );
                        }
                    }
                }
            }
        }
        let samples: Vec<RacerSample> = sim
            .vehicles()
            .iter()
            .filter(|v| all_racers.contains(&v.id))
            .map(|v| RacerSample::from_state(v.id, &v.vehicle.state(), v.input.throttle))
            .collect();
        judge.observe(sim.tick(), &samples, &events);
        ai.report_to(&mut judge, sim.tick());
        if tick < green {
            continue;
        }
        // `WP_PACE_DUMP=x,y,t`: every vehicle within 35 m of (x, y) at race time t (s).
        if let Ok(v) = std::env::var("WP_PACE_DUMP") {
            let f: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
            if f.len() == 3 && tick == green + (f[2] * 240.0) as u64 {
                for a in sim.world().agents() {
                    let (dx, dy) = (a.pos[0] - f[0], a.pos[1] - f[1]);
                    if dx * dx + dy * dy < 35.0 * 35.0 {
                        let dir = wp_world::geom::dir_of_yaw(a.yaw);
                        eprintln!(
                            "DUMP agent {} pos ({:.1},{:.1}) dir ({:.2},{:.2}) v {:.1} yield {} haz {} turn {} lod {:?}",
                            a.id, a.pos[0], a.pos[1], dir[0], dir[1], a.speed_mps, a.yielding, a.hazard_lights, a.turn_signal, a.lod
                        );
                    }
                }
                for v in sim.vehicles() {
                    let st = v.vehicle.state();
                    let (dx, dy) = (st.pos[0] - f[0], st.pos[1] - f[1]);
                    if dx * dx + dy * dy < 35.0 * 35.0 {
                        let fw = wp_world::drive::forward_of(st.rot);
                        eprintln!(
                            "DUMP vehicle {:?} kind {:?} pos ({:.1},{:.1}) dir ({:.2},{:.2}) v {:.1}",
                            v.id, v.kind, st.pos[0], st.pos[1], fw[0], fw[1], st.speed_mps
                        );
                    }
                }
            }
        }
        let results = judge.results();
        let field = ai.field();
        for k in 0..k_n {
            let running = results
                .iter()
                .find(|r| r.racer == racers[k])
                .is_some_and(|r| r.status == wp_race::RacerStatus::Running);
            if !running {
                continue;
            }
            let d = field.debug_of(k);
            if start_s[k].is_none() {
                start_s[k] = Some(d.s);
            }
            let v = sim
                .vehicles()
                .iter()
                .find(|x| x.id == racers[k])
                .unwrap()
                .vehicle
                .state()
                .speed_mps;
            let vi = corridor.interp(&ideal[k].v, d.s).max(1.0);
            let lost = PHYSICS_DT * (1.0 - v / vi).max(0.0);
            let own = corridor.interp(&field.race_profile_of(k).v, d.s);
            let tac = field.tactic_of(k);
            if tac != last_tactic[k] {
                if tac == Tactic::Rejoin {
                    rejoins[k] += 1;
                }
                if tac == Tactic::Reverse {
                    reverses[k] += 1;
                }
                last_tactic[k] = tac;
            }
            let b = match tac {
                Tactic::Rejoin => 9,
                Tactic::Reverse => 10,
                _ => match d.limit {
                    SpeedLimit::Obstacle(w) if w >= 1_000_000 => 3,
                    SpeedLimit::Obstacle(w) => {
                        if racers.iter().any(|r| r.0 == w) {
                            2
                        } else {
                            4
                        }
                    }
                    SpeedLimit::BackOut(_) => 5,
                    SpeedLimit::Unstable => 6,
                    SpeedLimit::Mode | SpeedLimit::RejoinWait => 7,
                    SpeedLimit::Signal => 8,
                    SpeedLimit::Profile => {
                        if own < vi - 0.3 && v < own + 0.5 {
                            0
                        } else {
                            1
                        }
                    }
                },
            };
            let b = if b > 1 {
                recovering[k] = Some(b);
                b
            } else if v > own - 1.0 {
                recovering[k] = None;
                b
            } else {
                recovering[k].unwrap_or(b)
            };
            // Split "usage" and "track" when the driver is below its own target.
            if b <= 1 {
                let l_usage = PHYSICS_DT * (1.0 - own.min(vi) / vi).max(0.0);
                loss[k][0] += l_usage.min(lost);
                loss[k][1] += (lost - l_usage).max(0.0);
            } else {
                loss[k][b] += lost;
            }
            if v > 8.0 {
                track_err[k].0 += (d.n - d.n_cmd).abs();
                track_err[k].1 += 1;
            }
            {
                let h = &mut head_on[k];
                if d.head_on {
                    if !h.2 && d.opposing {
                        head_on_opp[k] += 1;
                    }
                    h.0 += u32::from(!h.2);
                    h.1 += PHYSICS_DT;
                }
                h.2 = d.head_on;
            }
            if tick.is_multiple_of(240) {
                if let Some(me) = sim.vehicles().iter().find(|x| x.id == racers[k]) {
                    let st = me.vehicle.state();
                    let fw = wp_world::drive::forward_of(st.rot);
                    let c = &mut civ_near[k];
                    c.2 += 1;
                    for a in sim.world().agents() {
                        let (dx, dy) = (a.pos[0] - st.pos[0], a.pos[1] - st.pos[1]);
                        if dx * dx + dy * dy > 300.0 * 300.0 || (a.pos[2] - st.pos[2]).abs() > 5.0 {
                            continue;
                        }
                        let dir = wp_world::geom::dir_of_yaw(a.yaw);
                        let along = dir[0] * fw[0] + dir[1] * fw[1];
                        c.0 += u32::from(along > 0.5);
                        c.1 += u32::from(along < -0.5);
                    }
                }
            }
            if v > 2.0 {
                let l = &mut lateral[k];
                l.0 += 1;
                l.1 += u32::from(d.opposing);
                l.2 += f64::from(d.n);
                l.3 += f64::from(d.n) * f64::from(d.n);
            }
            if opts.trace == Some(k) && tick.is_multiple_of(trace_every()) {
                if let SpeedLimit::Obstacle(w) = d.limit {
                    if w >= 1_000_000 {
                        let me = sim
                            .vehicles()
                            .iter()
                            .find(|x| x.id == racers[k])
                            .unwrap()
                            .vehicle
                            .state();
                        if let Some(a) = sim.world().agents().find(|a| a.id == w - 1_000_000) {
                            let (sn, cs) = (a.yaw.sin(), a.yaw.cos());
                            eprintln!(
                                "    civ {} at ({:.1},{:.1}) me ({:.1},{:.1}) v {:.1} dir ({:.2},{:.2}) yield {} haz {} turn {} lod {:?}",
                                a.id, a.pos[0], a.pos[1], me.pos[0], me.pos[1], a.speed_mps, cs, sn, a.yielding, a.hazard_lights, a.turn_signal, a.lod
                            );
                        }
                    }
                }
                eprintln!(
                    "  t={:6.1} s={:7.1} v={:5.1} tgt={:5.1} own={:5.1} ideal={:5.1} lim={:?} tac={:?} n={:5.2} ncmd={:5.2} opp={} ho={} pass={:?}",
                    (tick - green) as f32 * PHYSICS_DT,
                    d.s,
                    v,
                    d.v_target,
                    own,
                    vi,
                    d.limit,
                    tac,
                    d.n,
                    d.n_cmd,
                    u8::from(d.opposing),
                    u8::from(d.head_on),
                    d.pass_checks
                );
            }
        }
        let done = |e: &EntityId| {
            results
                .iter()
                .find(|r| r.racer == *e)
                .is_some_and(|r| r.status != wp_race::RacerStatus::Running)
        };
        if judge.is_finished() || racers.iter().all(done) {
            break;
        }
    }
    let results = judge.results();
    let laps = match kind {
        RaceKind::Circuit { laps } => laps.max(1) as usize,
        _ => 1,
    };
    (0..k_n)
        .map(|k| {
            let r = results.iter().find(|r| r.racer == racers[k]).unwrap();
            let line = ai.field().line_of(k);
            let s0 = start_s[k].unwrap_or(0.0);
            let i0 = corridor.wrap((s0 / wp_opponent::corridor::CORRIDOR_STEP_M) as isize);
            let (i1, closed_c) = if corridor.closed {
                (i0, true)
            } else {
                let fin = race.route.length_m;
                (
                    ((fin / wp_opponent::corridor::CORRIDOR_STEP_M) as usize).min(m - 1),
                    false,
                )
            };
            let one = |p: &SpeedProfile, acc: f32| {
                let mut t =
                    standing_time(&p.v, &line.step_len, &measured[k], acc, i0, i1, m, closed_c);
                if closed_c {
                    // Full laps: lap 1 from a standstill, then flying laps.
                    let flying = p.lap_time_s(line, true);
                    if t < 1.0 {
                        t = flying;
                    }
                    t += flying * (laps - 1) as f32;
                }
                t
            };
            let params = entrants_profiles[k].params();
            let ideal_s = one(&ideal[k], measured[k].accel_mps2);
            let own_s = one(
                ai.field().race_profile_of(k),
                measured[k].accel_mps2 * params.usage.accel,
            );
            CarPace {
                entity: entity_ids[k].clone(),
                car: car_ids[k].clone(),
                time_s: r.elapsed_s(),
                ideal_s,
                own_s,
                loss: loss[k],
                rejoins: rejoins[k],
                reverses: reverses[k],
                wall_hits: walls[k],
                track_err_m: track_err[k].0 / track_err[k].1.max(1) as f32,
                opposing_frac: lateral[k].1 as f32 / lateral[k].0.max(1) as f32,
                lateral_sd_m: {
                    let (n, _, sum, sq) = lateral[k];
                    let n = f64::from(n.max(1));
                    ((sq / n - (sum / n) * (sum / n)).max(0.0)).sqrt() as f32
                },
                // Completed overtakes of anything on the road (racers and civilians alike:
                // weaving through traffic counts), per km raced.
                passes_per_km: ai.field().passes_by(k) as f32
                    / (race.route.length_m * laps as f32 / 1000.0).max(0.1),
                head_on: (head_on[k].0, head_on[k].1),
                head_on_opposing: head_on_opp[k],
                contacts: (r.civilian_hits, r.contacts),
                splits: ai.field().splits_by(k) as u32,
                civ_near: {
                    let c = civ_near[k];
                    let n = c.2.max(1) as f32;
                    (c.0 as f32 / n, c.1 as f32 / n)
                },
                split_aborts: ai.field().split_aborts_by(k) as u32,
                status: format!("{:?}", r.status),
            }
        })
        .collect()
}

fn list<T: std::str::FromStr + Clone>(var: &str, default: &[T]) -> Vec<T> {
    std::env::var(var)
        .ok()
        .map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect())
        .unwrap_or_else(|| default.to_vec())
}

#[test]
#[ignore = "measurement sweep (minutes in release); see the module docs"]
fn pace_sweep() {
    let opts = PaceOpts::from_env();
    let races: Vec<String> = list(
        "WP_PACE_RACES",
        &RACES.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    );
    let skills: Vec<f32> = list("WP_PACE_SKILLS", &[0.25, 0.5, 0.75, 0.95]);
    let mut caps = BTreeMap::new();
    eprintln!("opts: {opts:?}");
    for id in &races {
        for &skill in &skills {
            let t0 = std::time::Instant::now();
            let cars = race_pace(id, skill, &opts, &mut caps);
            eprintln!(
                "== {id} skill {skill:.2} ({:.0} s wall)",
                t0.elapsed().as_secs_f32()
            );
            for c in &cars {
                let ratio = c.time_s.map(|t| t / c.ideal_s);
                let lost: Vec<String> = BUCKETS
                    .iter()
                    .zip(c.loss)
                    .filter(|(_, l)| *l > 0.05)
                    .map(|(b, l)| format!("{b} {l:.1}"))
                    .collect();
                eprintln!(
                    "  {:<14} time {:>7} ideal {:6.1} own {:6.1} ratio {:>5} | rej {} rev {} wall {} trk {:.2} | opp {:.0}% latsd {:.2} pass/km {:.1} nose {} {:.1}s opp-nose {} civhit {} hit {} split {} splitab {} civnear {:.2} {:.2} | {} | {}",
                    c.car,
                    c.time_s.map_or("-".into(), |t| format!("{t:.1}")),
                    c.ideal_s,
                    c.own_s,
                    ratio.map_or("-".into(), |r| format!("{r:.3}")),
                    c.rejoins,
                    c.reverses,
                    c.wall_hits,
                    c.track_err_m,
                    100.0 * c.opposing_frac,
                    c.lateral_sd_m,
                    c.passes_per_km,
                    c.head_on.0,
                    c.head_on.1,
                    c.head_on_opposing,
                    c.contacts.0,
                    c.contacts.1,
                    c.splits,
                    c.split_aborts,
                    c.civ_near.0,
                    c.civ_near.1,
                    c.status,
                    lost.join(", ")
                );
            }
        }
    }
    for (car, c) in &caps {
        eprintln!(
            "cap {car}: lat {:.2} g brake {:.2} g accel {:.2} top {:.1}",
            c.max_lat_g, c.brake_g, c.accel_mps2, c.top_speed_mps
        );
    }
}

/// Trace period, ticks (`WP_PACE_TRACE_HZ`, default 2 Hz).
fn trace_every() -> u64 {
    let hz: u64 = std::env::var("WP_PACE_TRACE_HZ")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);
    (240 / hz.clamp(1, 240)).max(1)
}

/// `opponent_pace_<race id>_s<skill %>` → (race id, skill).
fn parse_pace_name(name: &str) -> Option<(String, f32)> {
    let rest = name.strip_prefix("opponent_pace_")?;
    let (race, skill) = rest.rsplit_once("_s")?;
    let pct: f32 = skill.parse().ok()?;
    Some((race.to_string(), pct / 100.0))
}

/// The value of a pace metric (`expect_pending: [metric(name: ..)]` of a pace scenario).
/// Per entity: `race_time_s` (judged time; +inf when it did not finish), `race_ratio` (time
/// over the car's ideal time), `race_finished` (0/1), `wall_hits` (≥ 400 N·s static
/// contacts while racing), `rejoins`. Field-wide (no entity): `best_ratio`, `finishers`.
fn pace_metric(cars: &[CarPace], name: &str, entity: Option<&str>) -> Option<f32> {
    let car = |e: Option<&str>| cars.iter().find(|c| Some(c.entity.as_str()) == e);
    let time = |c: &CarPace| c.time_s.unwrap_or(f32::INFINITY);
    Some(match name {
        "best_ratio" => cars
            .iter()
            .map(|c| time(c) / c.ideal_s)
            .fold(f32::INFINITY, f32::min),
        "finishers" => cars.iter().filter(|c| c.time_s.is_some()).count() as f32,
        "race_time_s" => time(car(entity)?),
        "race_ratio" => {
            let c = car(entity)?;
            time(c) / c.ideal_s
        }
        "race_finished" => f32::from(u8::from(car(entity)?.time_s.is_some())),
        "wall_hits" => car(entity)?.wall_hits as f32,
        "rejoins" => car(entity)?.rejoins as f32,
        _ => return None,
    })
}

/// Pace lock-in (playtest row 99): every `apps/simcli/scenarios/opponent/pace_*.ron` is a
/// picker race (traffic as in the picker, police off) raced here by `wp_opponent` with every
/// seat at the skill in its name; its `expect_pending` ranges (simcli itself cannot drive AI
/// racers, so they are pending there) are checked against the result. Release only: whole
/// courses in the full sim (~3 min).
#[test]
#[cfg_attr(debug_assertions, ignore = "races whole picker courses (release only)")]
fn pace_scenarios_hold_their_ranges() {
    let dir = repo().join("apps/simcli/scenarios/opponent");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("pace_") && n.ends_with(".ron"))
        })
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no pace scenarios in {dir:?}");
    if let Ok(only) = std::env::var("WP_PACE_ONLY") {
        files.retain(|f| f.to_string_lossy().contains(&only));
    }
    let opts = PaceOpts {
        // The scenario file decides police and traffic.
        police: true,
        density: None,
        solo: false,
        geometry: false,
        // `WP_PACE_TRACE=<grid idx>` traces one car here too (with `WP_PACE_ONLY`).
        trace: std::env::var("WP_PACE_TRACE")
            .ok()
            .and_then(|v| v.parse().ok()),
        seed: None,
        tod: None,
    };
    let mut caps = BTreeMap::new();
    let mut fails = Vec::new();
    for f in &files {
        let sc = load_scenario(f).unwrap();
        let (race, skill) =
            parse_pace_name(&sc.name).unwrap_or_else(|| panic!("{f:?}: bad pace name"));
        let expects = sc.expect_pending.clone();
        let cars = race_pace_in(sc.clone(), &race, skill, &opts, &mut caps);
        for c in &cars {
            eprintln!(
                "{}: {} ({}) time {:?} ideal {:.1} ratio {:.3} walls {} rejoins {}",
                sc.name,
                c.entity,
                c.car,
                c.time_s,
                c.ideal_s,
                c.time_s.map_or(f32::INFINITY, |t| t / c.ideal_s),
                c.wall_hits,
                c.rejoins
            );
        }
        // A scenario whose `finishers` range allows a car short of the flag (or that has no
        // `finishers` metric: Loop 9, checked over 8 seeds by `loop9_traffic_field_finishes`)
        // checks the times of the cars that finish only.
        let dnf_ok = !expects.iter().any(|x| {
            matches!(x, simcli::scenario::Expect::Metric { name, min, .. }
                if name == "finishers" && *min >= cars.len() as f32)
        });
        for x in &expects {
            let simcli::scenario::Expect::Metric {
                name,
                entity,
                min,
                max,
            } = x
            else {
                continue;
            };
            let unfinished = |e: &str| cars.iter().any(|c| c.entity == e && c.time_s.is_none());
            if name == "race_time_s" && dnf_ok && entity.as_deref().is_some_and(unfinished) {
                continue;
            }
            match pace_metric(&cars, name, entity.as_deref()) {
                Some(v) if v >= *min && v <= *max => {}
                Some(v) => fails.push(format!(
                    "{}: {name}({entity:?}) = {v:.3}, want {min}..={max}",
                    sc.name
                )),
                None => fails.push(format!("{}: unknown pace metric {name:?}", sc.name)),
            }
        }
    }
    assert!(fails.is_empty(), "{fails:#?}");
}

/// Loop 9 at 23:00 with traffic density 2.0: at least 44 of 48 cars finish per skill over seeds
/// 1-8. A single seed's finishers swing by ±3 cars on millimetre grid differences (the pack
/// packs up behind racers held by traffic), so the pace RONs check times only and this is
/// the finishers gate. Release only, ~20 min: `just pace-loop9` (lead main-track gate,
/// docs/LEAD_AUTOMATION.md).
#[test]
#[ignore = "release-only 8-seed Loop 9 traffic check: `just pace-loop9`"]
fn loop9_traffic_field_finishes() {
    let mut caps = BTreeMap::new();
    let mut fails = Vec::new();
    for skill in [0.5f32, 0.95] {
        let mut fin = 0;
        let mut all = 0;
        for seed in 1..=8u64 {
            let opts = PaceOpts {
                police: false,
                density: None,
                solo: false,
                geometry: false,
                trace: None,
                seed: Some(seed),
                tod: None,
            };
            let cars = race_pace("loop9_circuit", skill, &opts, &mut caps);
            all += cars.len();
            fin += cars.iter().filter(|c| c.time_s.is_some()).count();
        }
        eprintln!("loop9 skill {skill}: {fin}/{all} finished over seeds 1-8");
        if fin * 48 < 44 * all {
            fails.push(format!(
                "skill {skill}: {fin}/{all} finished, want >= 44/48"
            ));
        }
    }
    assert!(fails.is_empty(), "{fails:#?}");
}
