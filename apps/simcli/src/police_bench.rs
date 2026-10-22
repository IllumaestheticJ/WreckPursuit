//! Police competency benchmark (`simcli police-bench`, lead/police-bench; see
//! `docs/integration/police-bench.md`).
//!
//! A scripted escapee ([`wp_opponent::Evader`]) starts wanted at L1, L2 or L3 somewhere central
//! in a city, already moving, and runs. Nothing else is staged: police find it the way they find
//! a player in the game (patrol sightings, reckless-driving calls from its speeding, responders,
//! backup), chase it, and the run ends at the first pursuit end (capture, escape) or after
//! [`BenchOptions::duration_s`]. Every run is a normal deterministic `wp_sim` run built by
//! [`crate::run::build_sim`] (traffic, parked cars, police cars, weather), so the numbers are the
//! game's numbers.
//!
//! The matrix: city (Port Vesper v2, 12×12 grid) × day / night × clear / rain × wanted L1–L3 ×
//! evader skill (casual / good / expert) × seeds. Per run it records [`RunMetrics`]; per cell
//! [`CellSummary`]. **Measurement only**: nothing here changes police behaviour.
//!
//! Runs are independent, so they run on worker threads; each one is single-threaded and
//! deterministic, and results are stored by index, so the report does not depend on the thread
//! count.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use wp_contracts::{DamageMode, EntityId, PoliceEvent, PursuitOutcome, SimEvent, WORLD_ENTITY};
use wp_opponent::{Evader, EvaderSkill, PoliceCar};
use wp_police::{Assignment, PursuitStatus, UnitKind, UnitRole};
use wp_sim::VehicleKind;
use wp_world::drive::forward_of;
use wp_world::leader::Obstacle;

use crate::run::{build_sim, RunOptions};
use crate::scenario::{AssistSpec, CitySpec, Driver, EntitySpec, Scenario, WeatherSpec};
use crate::DataSource;
use crate::RunError;

/// Car the evader drives (the testbed's default sports coupe; police drive
/// [`crate::run::POLICE_CAR`]).
pub const EVADER_CAR: &str = "debug_coupe";
/// Starting heat for wanted levels L1, L2, L3 (a few points into each band; pursuit heat
/// climbs 0.25/s while seen, so a long chase can escalate).
pub const LEVEL_HEAT: [f32; 3] = [15.0, 35.0, 55.0];
/// Evader start speed, m/s (already on the move, like the testbed's city chase).
pub const START_SPEED_MPS: f32 = 15.0;
/// Hour for day / night cells.
pub const DAY_HOUR: f32 = 12.0;
/// Hour for night cells.
pub const NIGHT_HOUR: f32 = 23.0;
/// Fraction of dry grip the evader plans for in rain (it knows the road is wet).
pub const RAIN_GRIP: f32 = 0.75;

/// "Close" distance for the response metric, m.
const NEAR_M: f32 = 100.0;
/// Radius for the units-around metric, m.
const AROUND_M: f32 = 300.0;
/// Contacts with the same unit closer than this are one contact, s.
const CONTACT_EPISODE_S: f32 = 1.0;
/// A police car hitting static geometry / a prop / traffic this fast is a crash, m/s.
const CRASH_REL_SPEED_MPS: f32 = 4.0;
/// A police car touching a civilian car this fast is a collision, m/s.
const CIVILIAN_REL_SPEED_MPS: f32 = 2.0;
/// Crash episodes per police car merge within this window, s.
const CRASH_EPISODE_S: f32 = 2.0;
/// A PIT attempt counts as a success if the evader spins or stops within this window, s.
const PIT_WINDOW_S: f32 = 3.0;

/// Which city.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchCity {
    /// Port Vesper v2 (the testbed's city-chase city).
    Vesper,
    /// The 12×12 procedural grid.
    Grid,
}

impl BenchCity {
    /// Both cities.
    pub const ALL: [BenchCity; 2] = [BenchCity::Vesper, BenchCity::Grid];

    /// Short name.
    pub fn name(self) -> &'static str {
        match self {
            BenchCity::Vesper => "vesper",
            BenchCity::Grid => "grid",
        }
    }

    /// The scenario city.
    pub fn spec(self) -> CitySpec {
        match self {
            BenchCity::Vesper => CitySpec::vesper(),
            BenchCity::Grid => {
                let pg = wp_world::procgen::ProcGenConfig::default();
                CitySpec::Procedural {
                    seed: pg.seed,
                    blocks_x: pg.nx as u32,
                    blocks_y: pg.ny as u32,
                    block_m: pg.spacing_m,
                }
            }
        }
    }
}

/// One cell of the matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct BenchCell {
    /// City.
    pub city: BenchCity,
    /// 23:00 instead of 12:00.
    pub night: bool,
    /// Heavy rain on a wet road instead of clear and dry.
    pub rain: bool,
    /// Starting wanted level, 1..=3.
    pub level: u8,
    /// Evader skill.
    pub skill: EvaderSkill,
}

impl BenchCell {
    /// Label, e.g. `vesper night rain L2 expert`.
    pub fn label(&self) -> String {
        format!(
            "{} {} {} L{} {}",
            self.city.name(),
            if self.night { "night" } else { "day" },
            if self.rain { "rain" } else { "clear" },
            self.level,
            self.skill.name()
        )
    }

    /// The full matrix (72 cells), city-major.
    pub fn matrix() -> Vec<BenchCell> {
        let mut v = Vec::new();
        for city in BenchCity::ALL {
            for night in [false, true] {
                for rain in [false, true] {
                    for level in 1..=3 {
                        for skill in EvaderSkill::ALL {
                            v.push(BenchCell {
                                city,
                                night,
                                rain,
                                level,
                                skill,
                            });
                        }
                    }
                }
            }
        }
        v
    }

    /// Heavy cells, left out of the default run (`--full` runs all 72). The default 12 cells
    /// are an environment sweep (the good evader at L2 in every city × phase × weather) plus
    /// skill and level sweeps at the reference spot, Port Vesper at night in clear weather
    /// (the testbed's city chase): casual / expert at L2, good at L1 / L3. With 8 seeds that is
    /// 96 runs, about 11 minutes on 4 cores.
    pub fn slow(&self) -> bool {
        let reference = self.city == BenchCity::Vesper && self.night && !self.rain;
        let env_sweep = self.level == 2 && self.skill == EvaderSkill::Good;
        let sweeps = reference && (self.level == 2 || self.skill == EvaderSkill::Good);
        !(env_sweep || sweeps)
    }

    /// The scenario for this cell and seed (the evader's pose is placed after the build, see
    /// [`run_one`]).
    pub fn scenario(&self, seed: u64, mode: DamageMode, duration_s: f32) -> Scenario {
        Scenario {
            name: format!("police_bench {} seed {seed}", self.label()),
            seed,
            city: self.city.spec(),
            time_of_day_h: if self.night { NIGHT_HOUR } else { DAY_HOUR },
            time_ratio: 1.0,
            mode,
            assists: AssistSpec::Ai,
            traffic_density: None,
            traffic_cars: true,
            parked_cars: true,
            police_cars: true,
            covered_los: true,
            car_mix: false,
            police_presence: None,
            slow: false,
            restart_after_capture: false,
            panels: false,
            systems: None,
            barriers: Vec::new(),
            parked: Vec::new(),
            police_orders: Vec::new(),
            weather: self.rain.then(|| WeatherSpec {
                kind: wp_weather::WeatherKind::HeavyRain,
                intensity: 0.8,
                water_mm: 1.0,
                locked: true,
                season: Default::default(),
                seed: None,
                puddles: Vec::new(),
            }),
            entities: vec![EntitySpec {
                id: "evader".into(),
                car: EVADER_CAR.into(),
                pos: [0.0; 3],
                yaw: 0.0,
                speed_mps: 0.0,
                police_heat: LEVEL_HEAT[(self.level.clamp(1, 3) - 1) as usize],
                assists: None,
                driver: Driver::Scripted,
                inputs: Vec::new(),
                tune: Default::default(),
                mods: Vec::new(),
                player_steering: None,
            }],
            duration_s,
            sample_hz: 60.0,
            hash_every: 240,
            expect: Vec::new(),
            expect_pending: Vec::new(),
        }
    }
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEnd {
    /// Stopped next to a unit.
    Captured,
    /// Wrecked while wanted.
    CapturedTotalLoss,
    /// Escape timer ran out.
    Escaped,
    /// Safehouse dwell.
    Safehouse,
    /// Supervisor called it off.
    CalledOff,
    /// A pursuit ran when time was up.
    TimeoutInPursuit,
    /// No pursuit ever started.
    NeverEngaged,
}

impl RunEnd {
    /// Captured either way.
    pub fn captured(self) -> bool {
        matches!(self, RunEnd::Captured | RunEnd::CapturedTotalLoss)
    }

    /// Got away (escape or safehouse).
    pub fn escaped(self) -> bool {
        matches!(self, RunEnd::Escaped | RunEnd::Safehouse)
    }
}

/// Everything measured in one run. Times are seconds from the start of the run unless noted.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunMetrics {
    /// Cell.
    pub cell: BenchCell,
    /// Seed.
    pub seed: u64,
    /// How it ended.
    pub end: RunEnd,
    /// Seconds simulated.
    pub duration_s: f32,
    /// First time a police car was within 100 m.
    pub first_near_s: Option<f32>,
    /// Pursuit start.
    pub pursuit_start_s: Option<f32>,
    /// Pursuit end (capture / escape / called off).
    pub pursuit_end_s: Option<f32>,
    /// First evader–police physical contact.
    pub first_contact_s: Option<f32>,
    /// Evader–police contact episodes (per unit, merged within 1 s).
    pub contacts: u32,
    /// Seconds in a pursuit (`Pursuit` or `Searching`).
    pub pursuit_s: f32,
    /// Seconds of that with police line of sight (`Pursuit` status).
    pub pursuit_seen_s: f32,
    /// PIT attempts: contacts by a unit in its `Pit` tactic (physical units), plus
    /// `PoliceStats::pit_attempts` (kinematic units).
    pub pit_attempts: u32,
    /// PIT attempts followed by the evader spinning (> 60° off its travel) or stopping within 3 s.
    pub pit_successes: u32,
    /// Mean in-service police cars within 300 m during the pursuit (sampled at 1 Hz).
    pub units_near_mean: f32,
    /// Keep-up: distance to the nearest unit chasing the evader (`Pursue` / `Intercept`),
    /// sampled at 1 Hz while police see it (`Pursuit` status), m.
    pub lead_gaps_m: Vec<f32>,
    /// Keep-up: the nearest chasing unit's speed over the evader's, sampled at 1 Hz while
    /// police see it and it drives faster than [`PACE_MIN_MPS`] (1.0 = matching it).
    pub pace: Vec<f32>,
    /// Search episodes (Pursuit → Searching) ended by re-acquiring the evader / by its escape.
    pub searches_reacquired: u32,
    /// See `searches_reacquired`.
    pub searches_escaped: u32,
    /// Police cars within [`SEARCH_ZONE_M`] of the last known position, 1 Hz while Searching.
    pub search_units: Vec<u32>,
    /// Most police cars within 300 m at once.
    pub units_near_max: u32,
    /// Police cars within 300 m at 15 / 30 / 60 / 120 s after the pursuit start (`None` once
    /// the run ended).
    pub units_near_at: [Option<u32>; 4],
    /// Police crashes into static geometry or props (≥ 4 m/s, merged per car within 2 s).
    pub police_static_crashes: u32,
    /// Police collisions with civilian cars (physical traffic ≥ 2 m/s, plus the built-in
    /// driver's traffic collisions).
    pub police_civilian_collisions: u32,
    /// Police–police collisions (≥ 4 m/s).
    pub police_police_crashes: u32,
    /// Police stuck events (`PoliceStats::stuck`).
    pub stuck_events: u32,
    /// Police cars that appeared in a racer's line of sight, or whose spot a racer saw within
    /// 3 s (row 131, pop-in).
    pub spawns_in_view: u32,
    /// Pursuit cars (responders, backup, tool cars) that appeared on the far carriageway of
    /// the pursued racer's divided road (row 140).
    pub spawns_far_carriageway: u32,
    /// Turn-rounds by units after the evader (the pro driver facing away from its line, a
    /// reverse swing or U-turn, or backing out), each from its start to driving on, s.
    pub turn_round_s: Vec<f32>,
    /// Police cars total-lost without / with racer contact.
    pub self_wrecks: u32,
    /// Police cars wrecked after racer contact.
    pub contact_wrecks: u32,
    /// Ground units that pursued at some point.
    pub units_engaged: u32,
    /// Reckless-driving calls.
    pub calls: u32,
    /// Highest wanted level reached.
    pub max_level: u8,
    /// Evader collisions with civilian cars (≥ 2 m/s).
    pub evader_civilian_collisions: u32,
    /// Evader final health 0..=1.
    pub evader_health: f32,
    /// Evader route plans / reversals.
    pub evader_plans: u32,
    /// Times the evader reversed out of a stop.
    pub evader_reversals: u32,
}

/// Options for [`run_bench`].
#[derive(Debug, Clone)]
pub struct BenchOptions {
    /// Cells to run.
    pub cells: Vec<BenchCell>,
    /// Seeds per cell.
    pub seeds: Vec<u64>,
    /// Run length cap, s (5 min by default).
    pub duration_s: f32,
    /// Damage mode (AI behaviour is identical in both; default Arcade).
    pub mode: DamageMode,
    /// Worker threads (0 = available parallelism).
    pub threads: usize,
    /// Print one line per finished run to stderr.
    pub progress: bool,
}

impl Default for BenchOptions {
    fn default() -> Self {
        Self {
            cells: BenchCell::matrix()
                .into_iter()
                .filter(|c| !c.slow())
                .collect(),
            seeds: (1..=8).collect(),
            duration_s: 300.0,
            mode: DamageMode::Arcade,
            threads: 0,
            progress: false,
        }
    }
}

/// Run every (cell, seed) and return the metrics in (cell, seed) order.
pub fn run_bench(
    data: &(dyn DataSource + Sync),
    opts: &BenchOptions,
) -> Result<Vec<RunMetrics>, RunError> {
    let jobs: Vec<(BenchCell, u64)> = opts
        .cells
        .iter()
        .flat_map(|c| opts.seeds.iter().map(move |s| (*c, *s)))
        .collect();
    let threads = if opts.threads == 0 {
        std::thread::available_parallelism().map_or(1, |n| n.get())
    } else {
        opts.threads
    }
    .min(jobs.len().max(1));
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Result<RunMetrics, RunError>>>> =
        Mutex::new((0..jobs.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(&(cell, seed)) = jobs.get(i) else {
                    break;
                };
                let r = run_one(data, cell, seed, opts.mode, opts.duration_s);
                if opts.progress {
                    match &r {
                        Ok(m) => eprintln!(
                            "[{}/{}] {} seed {seed}: {:?} at {:.0} s",
                            i + 1,
                            jobs.len(),
                            cell.label(),
                            m.end,
                            m.duration_s
                        ),
                        Err(e) => eprintln!(
                            "[{}/{}] {} seed {seed}: {e}",
                            i + 1,
                            jobs.len(),
                            cell.label()
                        ),
                    }
                }
                results.lock().expect("results lock")[i] = Some(r);
            });
        }
    });
    results
        .into_inner()
        .expect("results lock")
        .into_iter()
        .map(|r| r.expect("every job ran"))
        .collect()
}

/// Evader start: a lane in the core / commercial districts (any district if the city has none),
/// not a highway, not covered, at least 80 m long, picked by `seed`. Returns (xy, z, yaw).
fn start_pose(map: &wp_world::CityMap, seed: u64) -> Option<([f32; 2], f32, f32)> {
    use wp_contracts::DistrictId as DistrictKind;
    let lanes: Vec<usize> = (0..map.n_lanes())
        .filter(|&i| {
            let s = &map.segs[i];
            s.drivable && s.length >= 80.0 && s.speed_limit <= 20.0
        })
        .collect();
    let central: Vec<usize> = lanes
        .iter()
        .copied()
        .filter(|&i| {
            let (p, _) = map.segs[i].point_at(map.segs[i].length * 0.5);
            matches!(
                map.district_at([p[0], p[1]]),
                Some(DistrictKind::Core) | Some(DistrictKind::Commercial)
            ) && map.covered_at(p).is_none()
        })
        .collect();
    let pool = if central.is_empty() { &lanes } else { &central };
    if pool.is_empty() {
        return None;
    }
    let mut rng = wp_math::Rng::new(seed ^ 0x0BE7_C4A5);
    let seg = &map.segs[pool[rng.below(pool.len() as u32) as usize]];
    let (p, d) = seg.point_at(seg.length * 0.4);
    Some(([p[0], p[1]], p[2], wp_world::geom::yaw_of(d)))
}

/// Contact episode bookkeeping (one "contact" per key until it has been quiet for `window`).
#[derive(Default)]
struct Episodes(BTreeMap<(u32, u32), f32>);

impl Episodes {
    /// Record a contact at `t`; true when it starts a new episode.
    fn hit(&mut self, key: (u32, u32), t: f32, window: f32) -> bool {
        let new = self.0.get(&key).is_none_or(|last| t - last > window);
        self.0.insert(key, t);
        new
    }
}

/// One benchmark run.
pub fn run_one(
    data: &dyn DataSource,
    cell: BenchCell,
    seed: u64,
    mode: DamageMode,
    duration_s: f32,
) -> Result<RunMetrics, RunError> {
    let sc = cell.scenario(seed, mode, duration_s);
    let mut built = build_sim(&sc, data, &RunOptions::default())?;
    let me = built.ids[0];
    let sim = &mut built.sim;
    let (xy, z, yaw) = start_pose(sim.world().map(), seed).ok_or_else(|| {
        RunError::Scenario(sc.name.clone(), "no lane to start the evader on".into())
    })?;
    sim.place_on_ground(me, xy, yaw, START_SPEED_MPS, Some(z))?;
    let def = &built.cars[EVADER_CAR].def;
    let mut evader = Evader::new(cell.skill.params(), def, seed, START_SPEED_MPS);
    if cell.rain {
        evader.set_grip_scale(RAIN_GRIP);
    }
    let dt = wp_contracts::PHYSICS_DT;
    let ticks = (duration_s / dt).round() as u64;

    let mut m = RunMetrics {
        cell,
        seed,
        end: RunEnd::NeverEngaged,
        duration_s: 0.0,
        first_near_s: None,
        pursuit_start_s: None,
        pursuit_end_s: None,
        first_contact_s: None,
        contacts: 0,
        pursuit_s: 0.0,
        pursuit_seen_s: 0.0,
        pit_attempts: 0,
        pit_successes: 0,
        units_near_mean: 0.0,
        lead_gaps_m: Vec::new(),
        pace: Vec::new(),
        units_near_max: 0,
        units_near_at: [None; 4],
        police_static_crashes: 0,
        police_civilian_collisions: 0,
        police_police_crashes: 0,
        stuck_events: 0,
        spawns_in_view: 0,
        spawns_far_carriageway: 0,
        turn_round_s: Vec::new(),
        searches_reacquired: 0,
        searches_escaped: 0,
        search_units: Vec::new(),
        self_wrecks: 0,
        contact_wrecks: 0,
        units_engaged: 0,
        calls: 0,
        max_level: cell.level,
        evader_civilian_collisions: 0,
        evader_health: 1.0,
        evader_plans: 0,
        evader_reversals: 0,
    };
    // Pursuit status on the previous tick (search episodes).
    let mut prev_status: Option<PursuitStatus> = None;
    // Turn-round in progress per unit after the evader: when it started, s.
    let mut turn_started: std::collections::BTreeMap<wp_police::UnitId, f32> =
        std::collections::BTreeMap::new();
    let base = sim.police().stats().clone();
    let mut contact_eps = Episodes::default();
    let mut crash_eps = Episodes::default();
    let mut near_sum = 0.0f32;
    let mut near_samples = 0u32;
    let mut pit_seen = base.pit_attempts;
    let mut pit_open: Vec<f32> = Vec::new();
    let mut end: Option<RunEnd> = None;
    let mut police_ids: Vec<EntityId> = Vec::new();
    let mut traffic_ids: Vec<EntityId> = Vec::new();
    // Parked cars and cones (props): the evader steers round them like round traffic. Their
    // start positions pick the ones near the car; the physics pose is used (knocked props move).
    let props: Vec<([f32; 2], [f32; 3])> = sim
        .world()
        .static_props()
        .map(|p| ([p.pos[0], p.pos[1]], p.half_extents))
        .collect();
    // Debug: one line per second on stderr (`WP_BENCH_TRACE=1`).
    let trace = std::env::var_os("WP_BENCH_TRACE").is_some();
    let trace_every: u64 = std::env::var("WP_BENCH_TRACE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(240)
        .max(1);

    for tick in 0..ticks {
        let t = tick as f32 * dt;
        // Who is who this tick (police cars come and go).
        police_ids.clear();
        traffic_ids.clear();
        for v in sim.vehicles() {
            match v.kind {
                VehicleKind::Police => police_ids.push(v.id),
                VehicleKind::Traffic => traffic_ids.push(v.id),
                _ => {}
            }
        }
        let states = sim.vehicle_states();
        let Some(my) = states
            .iter()
            .find(|(id, _)| *id == me)
            .map(|(_, s)| s.clone())
        else {
            break;
        };
        let police: Vec<PoliceCar> = states
            .iter()
            .filter(|(id, _)| police_ids.contains(id))
            .map(|(id, s)| PoliceCar {
                id: id.0,
                pos: s.pos,
                vel: s.lin_vel,
            })
            .collect();
        let mut obstacles: Vec<Obstacle> = sim
            .world()
            .agents()
            .map(|a| Obstacle::from_agent(&a))
            .collect();
        for (id, s) in &states {
            if *id != me {
                obstacles.push(Obstacle::from_state((1 << 24) + id.0, s));
            }
        }
        for (i, (p, he)) in props.iter().enumerate() {
            if (p[0] - my.pos[0]).abs() > 60.0 || (p[1] - my.pos[1]).abs() > 60.0 {
                continue;
            }
            if let Some((pos, rot)) = sim.prop_pose(i) {
                obstacles.push(Obstacle {
                    id: (2 << 24) + i as u32,
                    pos,
                    vel: [0.0; 3],
                    forward: forward_of(rot),
                    half_width_m: he[0],
                    half_length_m: he[1],
                });
            }
        }
        let input = evader.step(sim.world().map(), &my, &police, &obstacles, dt);
        if trace && tick % trace_every == 0 {
            let near = police
                .iter()
                .map(|c| wp_world::geom::dist([c.pos[0], c.pos[1]], [my.pos[0], my.pos[1]]))
                .fold(f32::INFINITY, f32::min);
            let status = sim.police().racer(me).map(|r| (r.status, r.level.number()));
            // The nearest unit chasing the evader and what caps its speed.
            let chaser = sim
                .police()
                .units()
                .iter()
                .filter(|u| {
                    matches!(
                        u.assignment,
                        Assignment::Pursue { target } | Assignment::Intercept { target, .. }
                            if target == me
                    )
                })
                .min_by(|a, b| {
                    let d = |u: &&wp_police::unit::Unit| {
                        wp_world::geom::dist(u.body.pos, [my.pos[0], my.pos[1]])
                    };
                    d(a).total_cmp(&d(b))
                })
                .map(|u| {
                    format!(
                        "u{} {} {:?} v{:.1} cmd{:.1} lim{:.1} jl{:.1} wall{:.1} by{:?} pos({:.0},{:.0}) br{} sw{} rev{} pro{:?} trk{:?}",
                        u.id.0,
                        u.assignment.name(),
                        u.tactic,
                        u.body.speed,
                        u.cmd.speed_mps,
                        u.avoid.limit_mps,
                        u.avoid.junction_limit_mps,
                        u.avoid.wall.limit_mps,
                        u.avoid.blocked_by,
                        u.body.pos[0],
                        u.body.pos[1],
                        u.branch,
                        u.unstick.swinging(),
                        u.unstick.reversing(),
                        u.pro_dbg,
                        u.cmd
                            .track
                            .pts
                            .iter()
                            .take(4)
                            .map(|p| [p[0] as i32, p[1] as i32])
                            .collect::<Vec<_>>()
                    )
                });
            eprintln!("    chaser {chaser:?}");
            eprintln!(
                "t={t:5.0} pos=({:7.1},{:7.1}) v={:4.1} in=(s{:+.2} t{:.2} b{:.2}) mode={:?} known={} cop={near:5.0} {status:?} rev={} {:?}",
                my.pos[0],
                my.pos[1],
                my.speed_mps,
                input.steer,
                input.throttle,
                input.brake,
                evader.mode(),
                evader.known_units(),
                evader.stats().reversals,
                evader.debug()
            );
        }
        sim.set_input(me, input)?;
        sim.step_fixed();
        let events = sim.drain_events();
        let t1 = t + dt;

        // Police view of the evader.
        let pol = sim.police();
        let mp = [my.pos[0], my.pos[1]];
        let cars_near = |r: f32| -> u32 {
            pol.units()
                .iter()
                .filter(|u| {
                    u.kind == UnitKind::Car
                        && !u.wrecked
                        && u.assignment != Assignment::OutOfService
                        && wp_world::geom::dist(u.body.pos, mp) <= r
                })
                .count() as u32
        };
        if m.first_near_s.is_none() && cars_near(NEAR_M) > 0 {
            m.first_near_s = Some(t1);
        }
        if let Some(rec) = pol.racer(me) {
            // Search episodes (row 84): how each ends, and units around the last known spot.
            if prev_status == Some(PursuitStatus::Searching) && rec.status != prev_status.unwrap() {
                match rec.status {
                    PursuitStatus::Pursuit => m.searches_reacquired += 1,
                    _ => m.searches_escaped += 1,
                }
            }
            prev_status = Some(rec.status);
            if rec.status == PursuitStatus::Searching && tick % 240 == 0 {
                if let Some(lk) = rec.last_known {
                    let n = pol
                        .units()
                        .iter()
                        .filter(|u| {
                            u.kind == UnitKind::Car
                                && !u.wrecked
                                && wp_world::geom::dist(u.body.pos, lk) <= SEARCH_ZONE_M
                        })
                        .count() as u32;
                    m.search_units.push(n);
                }
            }
            m.max_level = m.max_level.max(rec.level.number());
            match rec.status {
                PursuitStatus::Pursuit => {
                    m.pursuit_s += dt;
                    m.pursuit_seen_s += dt;
                }
                PursuitStatus::Searching => m.pursuit_s += dt,
                _ => {}
            }
        }
        // Turn-rounds by units after the evader: start to end of each.
        for u in pol.units() {
            let after_me = matches!(
                u.assignment,
                Assignment::Pursue { target } | Assignment::Intercept { target, .. }
                    if target == me
            );
            // The pro driver's crawl round, a reverse swing or U-turn, or backing out.
            let turning = (u.turning_round || u.unstick.swinging() || u.unstick.reversing())
                && after_me
                && !u.wrecked;
            match (turning, turn_started.get(&u.id).copied()) {
                (true, None) => {
                    turn_started.insert(u.id, t1);
                }
                (false, Some(t0)) => {
                    turn_started.remove(&u.id);
                    if after_me {
                        m.turn_round_s.push(t1 - t0);
                    }
                }
                _ => {}
            }
        }
        // 1 Hz samples of units around, during the pursuit.
        if let Some(ps) = m.pursuit_start_s {
            if tick % 240 == 0 && m.pursuit_end_s.is_none() {
                let seen = pol
                    .racer(me)
                    .is_some_and(|r| r.status == PursuitStatus::Pursuit);
                let chaser = pol
                    .units()
                    .iter()
                    .filter(|u| {
                        u.kind == UnitKind::Car
                            && !u.wrecked
                            && matches!(
                                u.assignment,
                                Assignment::Pursue { target } | Assignment::Intercept { target, .. }
                                    if target == me
                            )
                    })
                    .map(|u| (wp_world::geom::dist(u.body.pos, mp), u.body.speed))
                    .min_by(|a, b| a.0.total_cmp(&b.0));
                if let (true, Some((d, v))) = (seen, chaser) {
                    m.lead_gaps_m.push(d);
                    if my.speed_mps > PACE_MIN_MPS {
                        m.pace.push(v / my.speed_mps);
                    }
                }
                let n = cars_near(AROUND_M);
                near_sum += n as f32;
                near_samples += 1;
                m.units_near_max = m.units_near_max.max(n);
            }
            for (k, after) in [15.0f32, 30.0, 60.0, 120.0].into_iter().enumerate() {
                if m.units_near_at[k].is_none() && t1 >= ps + after && t1 - dt < ps + after {
                    m.units_near_at[k] = Some(cars_near(AROUND_M));
                }
            }
        }
        // PIT attempts and whether they worked.
        let pits = pol.stats().pit_attempts;
        while pit_seen < pits {
            pit_seen += 1;
            m.pit_attempts += 1;
            pit_open.push(t1);
        }
        if !pit_open.is_empty() {
            let fwd = forward_of(my.rot);
            let v = [my.lin_vel[0], my.lin_vel[1]];
            let sp = (v[0] * v[0] + v[1] * v[1]).sqrt();
            let spun = sp > 3.0 && (fwd[0] * v[0] + fwd[1] * v[1]) / sp < 0.5;
            let stopped = sp < 2.0;
            if spun || stopped {
                m.pit_successes += pit_open.len() as u32;
                pit_open.clear();
            }
            pit_open.retain(|t0| t1 - t0 <= PIT_WINDOW_S);
        }

        for e in &events {
            match e {
                SimEvent::Contact {
                    a, b, rel_speed, ..
                } => {
                    let (a, b) = (*a, *b);
                    let is_pol = |x: EntityId| police_ids.contains(&x);
                    let is_civ = |x: EntityId| traffic_ids.contains(&x);
                    if (a == me && is_pol(b)) || (b == me && is_pol(a)) {
                        let unit = if a == me { b } else { a };
                        if contact_eps.hit((unit.0, me.0), t1, CONTACT_EPISODE_S) {
                            m.contacts += 1;
                            m.first_contact_s.get_or_insert(t1);
                            // A physical unit's PIT is a contact while its tactic is `Pit`
                            // (`PoliceStats::pit_attempts` counts only kinematic units).
                            let pit = sim.police().units().iter().any(|u| {
                                u.entity == Some(unit) && u.tactic == wp_police::Tactic::Pit
                            });
                            if pit {
                                m.pit_attempts += 1;
                                pit_open.push(t1);
                            }
                        }
                    } else if (a == me && is_civ(b)) || (b == me && is_civ(a)) {
                        let other = if a == me { b } else { a };
                        if *rel_speed >= CIVILIAN_REL_SPEED_MPS
                            && crash_eps.hit((me.0, other.0), t1, CRASH_EPISODE_S)
                        {
                            m.evader_civilian_collisions += 1;
                        }
                    } else if is_pol(a) || is_pol(b) {
                        let (cop, other) = if is_pol(a) { (a, b) } else { (b, a) };
                        if other == WORLD_ENTITY {
                            if *rel_speed >= CRASH_REL_SPEED_MPS
                                && crash_eps.hit((cop.0, other.0), t1, CRASH_EPISODE_S)
                            {
                                m.police_static_crashes += 1;
                            }
                        } else if is_civ(other) {
                            if *rel_speed >= CIVILIAN_REL_SPEED_MPS
                                && crash_eps.hit((cop.0, other.0), t1, CRASH_EPISODE_S)
                            {
                                m.police_civilian_collisions += 1;
                            }
                        } else if is_pol(other)
                            && cop.0 < other.0
                            && *rel_speed >= CRASH_REL_SPEED_MPS
                            && crash_eps.hit((cop.0, other.0), t1, CRASH_EPISODE_S)
                        {
                            m.police_police_crashes += 1;
                        }
                    }
                }
                SimEvent::Police(PoliceEvent::PursuitStarted { racer, .. }) if *racer == me => {
                    m.pursuit_start_s.get_or_insert(t1);
                }
                SimEvent::Police(PoliceEvent::PursuitEnded { racer, outcome, .. })
                    if *racer == me && end.is_none() =>
                {
                    m.pursuit_end_s = Some(t1);
                    end = Some(match outcome {
                        PursuitOutcome::Captured => RunEnd::Captured,
                        PursuitOutcome::CapturedTotalLoss => RunEnd::CapturedTotalLoss,
                        PursuitOutcome::Escaped => RunEnd::Escaped,
                        PursuitOutcome::Safehouse => RunEnd::Safehouse,
                        PursuitOutcome::CalledOff => RunEnd::CalledOff,
                    });
                }
                _ => {}
            }
        }
        m.duration_s = t1;
        if end.is_some() {
            break;
        }
    }

    let st = sim.police().stats();
    m.end = end.unwrap_or(if m.pursuit_start_s.is_some() {
        RunEnd::TimeoutInPursuit
    } else {
        RunEnd::NeverEngaged
    });
    m.units_near_mean = if near_samples > 0 {
        near_sum / near_samples as f32
    } else {
        0.0
    };
    m.police_civilian_collisions += st.traffic_collisions - base.traffic_collisions;
    m.stuck_events = (st.stuck.len() - base.stuck.len()) as u32;
    m.spawns_in_view = st.spawns[base.spawns.len()..]
        .iter()
        .filter(|s| s.in_sight || s.seen_after_s.is_some())
        .count() as u32;
    m.spawns_far_carriageway = st.spawns[base.spawns.len()..]
        .iter()
        .filter(|s| {
            s.far_carriageway
                && matches!(
                    s.role,
                    UnitRole::Responder | UnitRole::Reinforcement | UnitRole::Tool
                )
        })
        .count() as u32;
    m.self_wrecks = st.self_wrecks - base.self_wrecks;
    m.contact_wrecks = st.contact_wrecks - base.contact_wrecks;
    m.units_engaged = st.units_engaged - base.units_engaged;
    m.calls = (st.calls.len() - base.calls.len()) as u32;
    m.evader_health = sim
        .vehicles()
        .iter()
        .find(|v| v.id == me)
        .map_or(0.0, |v| v.damage.health());
    m.evader_plans = evader.stats().plans;
    m.evader_reversals = evader.stats().reversals;
    Ok(m)
}

/// Per-cell (or roll-up) aggregate.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CellSummary {
    /// Label (cell label or roll-up name).
    pub label: String,
    /// Runs.
    pub runs: u32,
    /// Runs with a pursuit.
    pub engaged: u32,
    /// Median time to the first unit within 100 m, s (runs where it happened).
    pub first_near_med_s: Option<f32>,
    /// Runs where no unit came within 100 m.
    pub never_near: u32,
    /// Median pursuit start, s.
    pub pursuit_start_med_s: Option<f32>,
    /// Median time to first contact, s (runs with contact).
    pub first_contact_med_s: Option<f32>,
    /// Runs with any contact.
    pub runs_with_contact: u32,
    /// Contact episodes per pursuit minute.
    pub contacts_per_min: f32,
    /// PIT attempts (sum).
    pub pit_attempts: u32,
    /// PIT successes (sum).
    pub pit_successes: u32,
    /// Fraction of pursuit time with police line of sight.
    pub los_frac: f32,
    /// Mean police cars within 300 m during pursuits.
    pub units_near_mean: f32,
    /// Median distance to the nearest chasing unit while in sight (all samples), m.
    pub lead_gap_med_m: Option<f32>,
    /// Median nearest-chaser speed over the evader's while it is fast (all samples).
    pub pace_med: Option<f32>,
    /// Capture rate (captured / runs).
    pub capture_rate: f32,
    /// Median capture time after the pursuit start, s.
    pub capture_med_s: Option<f32>,
    /// Escape rate (escaped / runs).
    pub escape_rate: f32,
    /// Runs that timed out still in a pursuit.
    pub timeouts: u32,
    /// Police crashes into static geometry / props per minute of run.
    pub static_crashes_per_min: f32,
    /// Police collisions with civilian cars per minute of run.
    pub civilian_per_min: f32,
    /// Police stuck events per minute of run.
    pub stuck_per_min: f32,
    /// Police cars appearing in a racer's line of sight per minute of run (row 131).
    pub pop_in_per_min: f32,
    /// Search episodes that ended with the evader re-acquired (rest: escaped).
    pub reacquire_frac: Option<f32>,
    /// Mean police cars within [`SEARCH_ZONE_M`] of the last known spot while searching.
    pub search_units_mean: Option<f32>,
    /// Turn-rounds per pursuit minute.
    pub turns_per_min: f32,
    /// Median turn-round time, s.
    pub turn_med_s: Option<f32>,
    /// Far-carriageway pursuit spawns per minute of run (row 140).
    pub far_cw_per_min: f32,
    /// Police self-wrecks (sum).
    pub self_wrecks: u32,
    /// Evader collisions with civilians per minute of run.
    pub evader_civilian_per_min: f32,
    /// Minutes simulated.
    pub minutes: f32,
}

fn median(mut v: Vec<f32>) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2]
    } else {
        0.5 * (v[n / 2 - 1] + v[n / 2])
    })
}

/// Aggregate runs.
pub fn summarize(label: &str, runs: &[&RunMetrics]) -> CellSummary {
    let n = runs.len().max(1) as f32;
    let minutes: f32 = runs.iter().map(|r| r.duration_s).sum::<f32>() / 60.0;
    let pursuit_min: f32 = runs.iter().map(|r| r.pursuit_s).sum::<f32>() / 60.0;
    let pursuit_s: f32 = runs.iter().map(|r| r.pursuit_s).sum();
    let seen_s: f32 = runs.iter().map(|r| r.pursuit_seen_s).sum();
    let engaged: Vec<&&RunMetrics> = runs
        .iter()
        .filter(|r| r.pursuit_start_s.is_some())
        .collect();
    let per_min = |x: u32| {
        if minutes > 0.0 {
            x as f32 / minutes
        } else {
            0.0
        }
    };
    let sum = |f: fn(&RunMetrics) -> u32| runs.iter().map(|r| f(r)).sum::<u32>();
    CellSummary {
        label: label.to_string(),
        runs: runs.len() as u32,
        engaged: engaged.len() as u32,
        first_near_med_s: median(runs.iter().filter_map(|r| r.first_near_s).collect()),
        never_near: runs.iter().filter(|r| r.first_near_s.is_none()).count() as u32,
        pursuit_start_med_s: median(runs.iter().filter_map(|r| r.pursuit_start_s).collect()),
        first_contact_med_s: median(runs.iter().filter_map(|r| r.first_contact_s).collect()),
        runs_with_contact: runs.iter().filter(|r| r.contacts > 0).count() as u32,
        contacts_per_min: if pursuit_min > 0.0 {
            sum(|r| r.contacts) as f32 / pursuit_min
        } else {
            0.0
        },
        pit_attempts: sum(|r| r.pit_attempts),
        pit_successes: sum(|r| r.pit_successes),
        los_frac: if pursuit_s > 0.0 {
            seen_s / pursuit_s
        } else {
            0.0
        },
        units_near_mean: if engaged.is_empty() {
            0.0
        } else {
            engaged.iter().map(|r| r.units_near_mean).sum::<f32>() / engaged.len() as f32
        },
        lead_gap_med_m: median(
            runs.iter()
                .flat_map(|r| r.lead_gaps_m.iter().copied())
                .collect(),
        ),
        pace_med: median(runs.iter().flat_map(|r| r.pace.iter().copied()).collect()),
        capture_rate: runs.iter().filter(|r| r.end.captured()).count() as f32 / n,
        capture_med_s: median(
            runs.iter()
                .filter(|r| r.end.captured())
                .filter_map(|r| Some(r.pursuit_end_s? - r.pursuit_start_s.unwrap_or(0.0)))
                .collect(),
        ),
        escape_rate: runs.iter().filter(|r| r.end.escaped()).count() as f32 / n,
        timeouts: runs
            .iter()
            .filter(|r| r.end == RunEnd::TimeoutInPursuit)
            .count() as u32,
        static_crashes_per_min: per_min(sum(|r| r.police_static_crashes)),
        civilian_per_min: per_min(sum(|r| r.police_civilian_collisions)),
        stuck_per_min: per_min(sum(|r| r.stuck_events)),
        pop_in_per_min: per_min(sum(|r| r.spawns_in_view)),
        reacquire_frac: {
            let re: u32 = runs.iter().map(|r| r.searches_reacquired).sum();
            let esc: u32 = runs.iter().map(|r| r.searches_escaped).sum();
            (re + esc > 0).then(|| re as f32 / (re + esc) as f32)
        },
        search_units_mean: {
            let xs: Vec<u32> = runs
                .iter()
                .flat_map(|r| r.search_units.iter().copied())
                .collect();
            (!xs.is_empty()).then(|| xs.iter().sum::<u32>() as f32 / xs.len() as f32)
        },
        turns_per_min: if pursuit_min > 0.0 {
            runs.iter().map(|r| r.turn_round_s.len()).sum::<usize>() as f32 / pursuit_min
        } else {
            0.0
        },
        turn_med_s: median(
            runs.iter()
                .flat_map(|r| r.turn_round_s.iter().copied())
                .collect(),
        ),
        far_cw_per_min: per_min(sum(|r| r.spawns_far_carriageway)),
        self_wrecks: sum(|r| r.self_wrecks),
        evader_civilian_per_min: per_min(sum(|r| r.evader_civilian_collisions)),
        minutes,
    }
}

/// Per-cell summaries in cell order, then roll-ups (overall, by skill, level, city, phase,
/// weather).
pub fn summaries(runs: &[RunMetrics]) -> (Vec<CellSummary>, Vec<CellSummary>) {
    let mut by_cell: BTreeMap<BenchCell, Vec<&RunMetrics>> = BTreeMap::new();
    let mut order: Vec<BenchCell> = Vec::new();
    for r in runs {
        if !by_cell.contains_key(&r.cell) {
            order.push(r.cell);
        }
        by_cell.entry(r.cell).or_default().push(r);
    }
    let cells = order
        .iter()
        .map(|c| summarize(&c.label(), &by_cell[c]))
        .collect();
    let all: Vec<&RunMetrics> = runs.iter().collect();
    let mut rollups = vec![summarize("ALL", &all)];
    let mut group = |name: String, f: &dyn Fn(&RunMetrics) -> bool| {
        let v: Vec<&RunMetrics> = runs.iter().filter(|r| f(r)).collect();
        if !v.is_empty() {
            rollups.push(summarize(&name, &v));
        }
    };
    for s in EvaderSkill::ALL {
        group(format!("skill {}", s.name()), &|r| r.cell.skill == s);
    }
    for l in 1..=3u8 {
        group(format!("start L{l}"), &|r| r.cell.level == l);
    }
    for c in BenchCity::ALL {
        group(format!("city {}", c.name()), &|r| r.cell.city == c);
    }
    group("day".into(), &|r| !r.cell.night);
    group("night".into(), &|r| r.cell.night);
    group("clear".into(), &|r| !r.cell.rain);
    group("rain".into(), &|r| r.cell.rain);
    (cells, rollups)
}

fn opt(v: Option<f32>) -> String {
    v.map_or("—".into(), |x| format!("{x:.0}"))
}

fn pct(x: f32) -> String {
    format!("{:.0}%", x * 100.0)
}

/// The search zone around the last known position for the bench's search columns (row 84), m.
pub const SEARCH_ZONE_M: f32 = 300.0;

/// Pace is sampled only while the evader is faster than this, m/s (~90 km/h).
pub const PACE_MIN_MPS: f32 = 25.0;

/// Column headers of [`table`].
const HEADER: [&str; 25] = [
    "cell",
    "runs",
    "engaged",
    "near100 s",
    "never near",
    "pursuit s",
    "contact s",
    "contacts/min",
    "PIT ok/att",
    "LOS",
    "units≤300m",
    "lead m",
    "pace",
    "capture",
    "capture s",
    "escape",
    "crash/min",
    "civ/min",
    "stuck/min",
    "pop-in/min",
    "re-acquire",
    "search units",
    "turns/min",
    "turn s",
    "far-cw/min",
];

fn row(s: &CellSummary) -> [String; 25] {
    [
        s.label.clone(),
        s.runs.to_string(),
        s.engaged.to_string(),
        opt(s.first_near_med_s),
        s.never_near.to_string(),
        opt(s.pursuit_start_med_s),
        opt(s.first_contact_med_s),
        format!("{:.2}", s.contacts_per_min),
        format!("{}/{}", s.pit_successes, s.pit_attempts),
        pct(s.los_frac),
        format!("{:.1}", s.units_near_mean),
        opt(s.lead_gap_med_m),
        s.pace_med.map_or("—".to_string(), |p| format!("{p:.2}")),
        pct(s.capture_rate),
        opt(s.capture_med_s),
        pct(s.escape_rate),
        format!("{:.2}", s.static_crashes_per_min),
        format!("{:.2}", s.civilian_per_min),
        format!("{:.2}", s.stuck_per_min),
        format!("{:.2}", s.pop_in_per_min),
        s.reacquire_frac.map_or("—".to_string(), pct),
        s.search_units_mean
            .map_or("—".to_string(), |x| format!("{x:.1}")),
        format!("{:.2}", s.turns_per_min),
        s.turn_med_s.map_or("—".to_string(), |x| format!("{x:.1}")),
        format!("{:.2}", s.far_cw_per_min),
    ]
}

/// Plain-text table (terminal).
pub fn table(rows: &[CellSummary]) -> String {
    let body: Vec<[String; 25]> = rows.iter().map(row).collect();
    let mut w = HEADER.map(|h| h.chars().count());
    for r in &body {
        for (k, c) in r.iter().enumerate() {
            w[k] = w[k].max(c.chars().count());
        }
    }
    let mut out = String::new();
    let line = |cells: Vec<&str>, out: &mut String| {
        for (k, c) in cells.iter().enumerate() {
            let pad = w[k] - c.chars().count();
            if k == 0 {
                let _ = write!(out, "{c}{}", " ".repeat(pad));
            } else {
                let _ = write!(out, "  {}{c}", " ".repeat(pad));
            }
        }
        out.push('\n');
    };
    line(HEADER.to_vec(), &mut out);
    for r in &body {
        line(r.iter().map(String::as_str).collect(), &mut out);
    }
    out
}

/// Markdown table.
pub fn markdown_table(rows: &[CellSummary]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "| {} |", HEADER.join(" | "));
    let _ = writeln!(
        out,
        "|{}",
        HEADER
            .iter()
            .enumerate()
            .map(|(k, _)| if k == 0 { "---|" } else { "---:|" })
            .collect::<String>()
    );
    for r in rows {
        let _ = writeln!(out, "| {} |", row(r).join(" | "));
    }
    out
}

/// The results document (`docs/integration/police-bench-results.md`).
pub fn markdown_report(label: &str, opts: &BenchOptions, runs: &[RunMetrics]) -> String {
    let (cells, rollups) = summaries(runs);
    let mut out = String::new();
    let _ = writeln!(out, "# Police benchmark results — {label}\n");
    let _ = writeln!(
        out,
        "Generated by `simcli police-bench` (see `docs/integration/police-bench.md` for what each \
         column means). {} cells × {} seeds = {} runs, up to {:.0} s each, {:?} damage, evader \
         car `{EVADER_CAR}`, police `{}`.\n",
        opts.cells.len(),
        opts.seeds.len(),
        runs.len(),
        opts.duration_s,
        opts.mode,
        crate::run::POLICE_CAR,
    );
    let _ = writeln!(
        out,
        "Times are medians in seconds from the run start, except `capture s` (after the \
         pursuit start). Rates per minute are per simulated minute; `contacts/min` per pursuit \
         minute. `—` = never happened.\n"
    );
    let _ = writeln!(out, "## Summary\n");
    out.push_str(&markdown_table(&rollups));
    let _ = writeln!(out, "\n## Per cell\n");
    out.push_str(&markdown_table(&cells));
    let mut ends: BTreeMap<RunEnd, u32> = BTreeMap::new();
    for r in runs {
        *ends.entry(r.end).or_default() += 1;
    }
    let _ = writeln!(out, "\n## Run ends\n");
    for (e, n) in ends {
        let _ = writeln!(out, "- {e:?}: {n}");
    }
    out
}
