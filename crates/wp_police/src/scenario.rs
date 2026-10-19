//! Headless police scenarios on `wp_world`'s procedural city (`crates/wp_police/scenarios/*.ron`).
//!
//! A scenario scripts one or more racers (roam a district at a multiple of the speed limit,
//! stop, run for a safehouse, total their car) and checks metric **ranges** after the run. With
//! `runs > 1` the scenario is repeated with seeds `seed, seed+1, …` and metrics are aggregated
//! (times averaged, counts summed, rates pooled). Speeding violations are generated here from
//! the racer's speed vs the segment limit, standing in for `wp_world`'s violation detector.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use wp_contracts::{
    DamageMode, EntityId, SignalState, SimEvent, SurfaceType, TimeOfDay, ViolationKind, WorldQuery,
    PHYSICS_DT,
};
use wp_math::{Rng, StateHasher};
use wp_world::procgen::ProcGenConfig;
use wp_world::CityMap;

use crate::config::PoliceConfig;
use crate::events::{PoliceEvent, UnitKind};
use crate::graph::{dist, lerp2, sub, District, NodeId, PoiKind, RoadClass, RoadGraph};
use crate::unit::{
    plan_unit_speed, step_ground, DriveCommand, DriveEnv, Track, Unit, UnitRole, LANE_OFFSET_M,
};
use crate::{PoliceSetup, PoliceSystem, RacerView};

fn d_core() -> District {
    District::Core
}
fn d_two() -> f32 {
    2.0
}
fn d_corner() -> f32 {
    0.85
}
fn d_true() -> bool {
    true
}
fn d_one() -> u32 {
    1
}

/// Scripted racer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RacerScript {
    /// District to start in and roam.
    #[serde(default = "d_core")]
    pub district: District,
    /// Target speed as a multiple of the local limit.
    #[serde(default = "d_two")]
    pub speed_factor: f32,
    /// Planning lateral acceleration, g.
    #[serde(default = "d_corner")]
    pub cornering_g: f32,
    /// Heat at t = 0.
    #[serde(default)]
    pub initial_heat: f32,
    /// Emit speeding violations when over the limit.
    #[serde(default = "d_true")]
    pub violations: bool,
    /// Stay in `district` when choosing destinations (otherwise roam the whole city).
    #[serde(default = "d_true")]
    pub stay_in_district: bool,
    /// Brake to a stop and stay stopped from this time, s.
    #[serde(default)]
    pub stop_at_s: Option<f32>,
    /// Drive to the nearest safehouse from this time and park inside, s.
    #[serde(default)]
    pub safehouse_at_s: Option<f32>,
    /// Emit `TotalLoss` for this racer at this time and stop, s.
    #[serde(default)]
    pub total_loss_at_s: Option<f32>,
}

/// Expected metric range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    /// Metric name (see [`PoliceRunSummary::metrics`]).
    pub metric: String,
    /// Inclusive minimum.
    pub min: f32,
    /// Inclusive maximum.
    pub max: f32,
}

/// A police scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoliceScenario {
    /// Name.
    pub name: String,
    /// Base seed (police RNG, racer starts, routes).
    pub seed: u64,
    /// `wp_world` procedural city to run on.
    #[serde(default)]
    pub city: ScenarioCity,
    /// Damage mode.
    #[serde(default)]
    pub mode: DamageMode,
    /// Start time of day, hours.
    pub time_of_day_h: f32,
    /// In-game seconds per sim second (0 = frozen clock).
    #[serde(default)]
    pub time_ratio: f32,
    /// Duration per run, s.
    pub duration_s: f32,
    /// Repetitions (seed, seed+1, …).
    #[serde(default = "d_one")]
    pub runs: u32,
    /// Stop a run as soon as the first pursuit ends.
    #[serde(default)]
    pub stop_after_first_pursuit: bool,
    /// Racers.
    pub racers: Vec<RacerScript>,
    /// Expected ranges.
    #[serde(default)]
    pub expect: Vec<Expect>,
}

/// Result of a scenario (all runs).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PoliceRunSummary {
    /// Scenario name.
    pub name: String,
    /// Combined state hash of every run (determinism).
    pub state_hash: u64,
    /// Aggregated metrics.
    pub metrics: BTreeMap<String, f32>,
    /// Failed expectations. Empty = pass.
    pub failures: Vec<String>,
}

impl PoliceRunSummary {
    /// Metric by name.
    pub fn metric(&self, name: &str) -> Option<f32> {
        self.metrics.get(name).copied()
    }
}

/// Scenario load error.
#[derive(Debug, thiserror::Error)]
pub enum ScenarioError {
    /// Unreadable.
    #[error("{0}: {1}")]
    Io(String, std::io::Error),
    /// Malformed.
    #[error("{0}: {1}")]
    Parse(String, String),
}

/// Parse a scenario file.
pub fn load_scenario(path: &Path) -> Result<PoliceScenario, ScenarioError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| ScenarioError::Io(path.display().to_string(), e))?;
    ron::from_str(&text)
        .map_err(|e| ScenarioError::Parse(path.display().to_string(), e.to_string()))
}

/// `wp_world` procedural city size and seed (`ProcGenConfig` with the remaining fields at
/// their defaults). The default is `ProcGenConfig::small()`; the safehouse scenario needs the
/// full 12×12 city (the small one has no residential district).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScenarioCity {
    /// Grid columns.
    pub nx: usize,
    /// Grid rows.
    pub ny: usize,
    /// Generator seed.
    pub seed: u64,
}

impl Default for ScenarioCity {
    fn default() -> Self {
        let small = ProcGenConfig::small();
        Self {
            nx: small.nx,
            ny: small.ny,
            seed: small.seed,
        }
    }
}

impl ScenarioCity {
    /// The generator config.
    pub fn procgen(&self) -> ProcGenConfig {
        ProcGenConfig {
            nx: self.nx,
            ny: self.ny,
            seed: self.seed,
            ..ProcGenConfig::default()
        }
    }
}

/// Minimal world for scenarios: the clock, plus line of sight from the `wp_world` city.
#[derive(Debug, Clone)]
pub struct ScenarioWorld {
    /// Time of day.
    pub time: TimeOfDay,
    /// Clock ratio.
    pub ratio: f32,
    /// City whose buildings block line of sight (`None`: nothing blocks).
    pub map: Option<Arc<CityMap>>,
}

impl ScenarioWorld {
    /// Clock-only world.
    pub fn clock(time: TimeOfDay, ratio: f32) -> Self {
        Self {
            time,
            ratio,
            map: None,
        }
    }
}

impl WorldQuery for ScenarioWorld {
    fn surface_at(&self, _p: [f32; 3]) -> SurfaceType {
        SurfaceType::Asphalt
    }
    fn speed_limit_at(&self, _p: [f32; 3]) -> Option<f32> {
        None
    }
    fn signal_state_ahead(&self, _p: [f32; 3], _h: [f32; 3]) -> Option<SignalState> {
        None
    }
    fn time_of_day(&self) -> TimeOfDay {
        self.time
    }
    fn line_of_sight(&self, a: [f32; 3], b: [f32; 3]) -> bool {
        self.map.as_ref().is_none_or(|m| m.line_of_sight(a, b))
    }
}

struct ScriptedRacer {
    entity: EntityId,
    script: RacerScript,
    body: Unit,
    total_lost: bool,
    to_safehouse: bool,
}

/// Per-run raw numbers.
#[derive(Debug, Clone, Default)]
struct RunNumbers {
    pursuit_start_s: Option<f32>,
    pursuit_end_s: Option<f32>,
    reinforcement_delays: Vec<f32>,
    escapes: u32,
    safehouse: u32,
    captures: u32,
    total_loss_captures: u32,
    pursuits: u32,
    units_engaged: u32,
    self_wrecks: u32,
    contact_wrecks: u32,
    pulled_over: u32,
    tool_sight: Vec<f32>,
    spike_hits: u32,
    helo_launches: u32,
    near_misses: u32,
    traffic_collisions: u32,
    run_wide: u32,
    pool: [u32; 3],
    max_level: u8,
    /// Radio lines while no racer is wanted / while some racer is in a pursuit.
    radio_calm: u32,
    radio_hot: u32,
    /// Seconds with no racer wanted / with some racer in a pursuit.
    calm_s: f32,
    hot_s: f32,
    hash: u64,
}

fn random_node(rng: &mut Rng, g: &RoadGraph, d: Option<District>) -> NodeId {
    let cands: Vec<u32> = (0..g.nodes.len() as u32)
        .filter(|&n| d.is_none_or(|d| g.nodes[n as usize].district == d))
        .collect();
    NodeId(cands[rng.below(cands.len() as u32) as usize])
}

fn racer_route(rng: &mut Rng, g: &RoadGraph, r: &ScriptedRacer) -> Track {
    let pos = r.body.body.pos;
    let rp = g.nearest_on_road(pos).expect("city has roads");
    let seg = *g.segment(rp.segment);
    let ab = sub(g.node(seg.b).pos, g.node(seg.a).pos);
    let start = if crate::graph::dot(ab, r.body.body.heading) >= 0.0 {
        seg.b
    } else {
        seg.a
    };
    // Don't drive to the node ahead only to come straight back: turn around now instead.
    let behind = g.other_end(rp.segment, start);
    let route_pts = |route: Vec<NodeId>| {
        let skip = route.len() >= 2 && route[1] == behind;
        let mut pts = vec![pos];
        pts.extend(route.iter().skip(skip as usize).map(|n| g.node(*n).pos));
        pts
    };
    let dest = if r.to_safehouse {
        let sh = g
            .pois_of(PoiKind::Safehouse)
            .min_by(|a, b| dist(a.pos, pos).total_cmp(&dist(b.pos, pos)))
            .expect("city has safehouses");
        let n = g.nearest_node(sh.pos).expect("nodes");
        let mut pts = route_pts(g.shortest_path(start, n).unwrap_or_default());
        pts.push(sh.pos);
        return Track::new(pts);
    } else {
        let d = r.script.stay_in_district.then_some(r.script.district);
        let mut dest = random_node(rng, g, d);
        if dest == start {
            dest = random_node(rng, g, d);
        }
        dest
    };
    Track::new(route_pts(g.shortest_path(start, dest).unwrap_or_default()))
}

fn run_once(
    sc: &PoliceScenario,
    seed: u64,
    cfg: &PoliceConfig,
    hook: &mut dyn FnMut(&PoliceSystem, &[SimEvent]),
) -> RunNumbers {
    let (graph, map) = crate::city::procedural(&sc.city.procgen());
    let map = Some(Arc::new(map));
    let setup = PoliceSetup {
        seed,
        mode: sc.mode,
        internal_driving: true,
        hazards: true,
    };
    let mut police = PoliceSystem::with_city(cfg.clone(), graph.clone(), setup);
    let mut world = ScenarioWorld {
        time: TimeOfDay::from_hours(sc.time_of_day_h),
        ratio: sc.time_ratio,
        map,
    };
    let mut rng = Rng::new(seed ^ 0x5CE7_A210);
    let mut racers: Vec<ScriptedRacer> = Vec::new();
    for (k, script) in sc.racers.iter().enumerate() {
        let cands: Vec<usize> = (0..graph.segments.len())
            .filter(|&s| {
                graph.segments[s].district == script.district
                    && graph.segments[s].class != RoadClass::Highway
            })
            .collect();
        let s = graph.segments[cands[rng.below(cands.len() as u32) as usize]];
        let a = graph.node(s.a).pos;
        let b = graph.node(s.b).pos;
        let entity = EntityId(1000 + k as u32);
        let mut body = Unit::new(
            crate::events::UnitId(0),
            UnitKind::Car,
            UnitRole::Player,
            lerp2(a, b, 0.5),
            sub(b, a),
            0.0,
        );
        body.body.speed = s.speed_limit_mps * script.speed_factor * 0.5;
        if script.initial_heat > 0.0 {
            police.set_heat(entity, script.initial_heat);
        }
        racers.push(ScriptedRacer {
            entity,
            script: script.clone(),
            body,
            total_lost: false,
            to_safehouse: false,
        });
    }
    let ticks = (sc.duration_s / PHYSICS_DT).round() as u64;
    let think_every = 24u64;
    let mut out = Vec::new();
    let mut nums = RunNumbers::default();
    let env_template = DriveEnv {
        limit_mps: 13.9,
        phase: world.time.phase(),
        mode: sc.mode,
        hazards: false,
    };
    for tick in 0..ticks {
        let t = tick as f32 * PHYSICS_DT;
        let mut events = Vec::new();
        for r in racers.iter_mut() {
            // Script triggers.
            if let Some(ts) = r.script.total_loss_at_s {
                if t >= ts && !r.total_lost {
                    r.total_lost = true;
                    events.push(SimEvent::TotalLoss { vehicle: r.entity });
                }
            }
            if let Some(ts) = r.script.safehouse_at_s {
                if t >= ts && !r.to_safehouse {
                    r.to_safehouse = true;
                    r.body.cmd.track = Track::default();
                }
            }
            let stopped = r.total_lost || r.script.stop_at_s.is_some_and(|s| t >= s);
            if tick % think_every == 0 {
                let limit = graph
                    .nearest_on_road(r.body.body.pos)
                    .map(|p| graph.segment(p.segment).speed_limit_mps)
                    .unwrap_or(13.9);
                r.body.limit_mps = limit;
                let arrived = r.body.track_s > r.body.cmd.track.length() - 10.0;
                if !r.body.cmd.track.is_drivable() || (arrived && !r.to_safehouse) {
                    let tr = racer_route(&mut rng, &graph, r);
                    r.body.set_command(DriveCommand {
                        track: tr,
                        speed_mps: 0.0,
                        cornering_g: r.script.cornering_g,
                        siren: false,
                        lane_offset_m: LANE_OFFSET_M,
                    });
                }
                r.body.cmd.speed_mps = if stopped {
                    0.0
                } else {
                    limit * r.script.speed_factor
                };
                r.body.planned_speed = plan_unit_speed(&r.body, 8.0);
            }
            let env = DriveEnv {
                limit_mps: r.body.limit_mps,
                ..env_template
            };
            step_ground(&mut r.body, &env, &cfg.pursuit, &mut rng, PHYSICS_DT);
            r.body.health = 1.0;
            if r.script.violations && r.body.body.speed > r.body.limit_mps + 1.0 {
                events.push(SimEvent::TrafficViolation {
                    vehicle: r.entity,
                    kind: ViolationKind::Speeding {
                        over_mps: r.body.body.speed - r.body.limit_mps,
                    },
                    pos: [r.body.body.pos[0], r.body.body.pos[1], 0.0],
                });
            }
        }
        let views: Vec<RacerView> = racers
            .iter()
            .map(|r| {
                let v = r.body.body.vel();
                RacerView {
                    entity: r.entity,
                    pos: [r.body.body.pos[0], r.body.body.pos[1], 0.0],
                    vel: [v[0], v[1], 0.0],
                }
            })
            .collect();
        world.time = world.time.advanced(PHYSICS_DT * world.ratio);
        out.clear();
        police.step(&world, &views, &events, PHYSICS_DT, &mut out);
        hook(&police, &out);
        let mut wanted = false;
        let mut hot = false;
        for r in &racers {
            let level = police.wanted_level(r.entity).number();
            nums.max_level = nums.max_level.max(level);
            wanted |= level > 0;
            hot |= police.racer(r.entity).is_some_and(|rec| rec.pursued());
        }
        let lines = out
            .iter()
            .filter(|e| {
                matches!(
                    PoliceEvent::from_sim_event(e),
                    Some(PoliceEvent::Radio { .. })
                )
            })
            .count() as u32;
        if hot {
            nums.hot_s += PHYSICS_DT;
            nums.radio_hot += lines;
        } else if !wanted {
            nums.calm_s += PHYSICS_DT;
            nums.radio_calm += lines;
        }
        if sc.stop_after_first_pursuit && police.stats().pursuits.iter().any(|p| p.end.is_some()) {
            break;
        }
    }
    let st = police.stats();
    nums.pursuit_start_s = st.pursuits.first().map(|p| p.start.secs());
    nums.pursuit_end_s = st.pursuits.iter().find_map(|p| p.end).map(|t| t.secs());
    nums.reinforcement_delays = st.reinforcement_delays_s.clone();
    for p in &st.pursuits {
        match p.outcome {
            Some(crate::PursuitOutcome::Escaped) => nums.escapes += 1,
            Some(crate::PursuitOutcome::Safehouse) => nums.safehouse += 1,
            Some(crate::PursuitOutcome::Captured) => nums.captures += 1,
            Some(crate::PursuitOutcome::CapturedTotalLoss) => nums.total_loss_captures += 1,
            _ => {}
        }
    }
    nums.pursuits = st
        .pursuits
        .iter()
        .filter(|p| p.end != Some(p.start))
        .count() as u32;
    nums.units_engaged = st.units_engaged;
    nums.self_wrecks = st.self_wrecks;
    nums.contact_wrecks = st.contact_wrecks;
    nums.pulled_over = st.pulled_over;
    nums.tool_sight = st.tool_sight_m.clone();
    nums.spike_hits = st.spike_hits;
    nums.helo_launches = st.helo_launches;
    nums.near_misses = st.near_misses;
    nums.traffic_collisions = st.traffic_collisions;
    nums.run_wide = st.run_wide_crashes;
    for (k, d) in District::ALL.iter().enumerate() {
        nums.pool[k] = police.pool_count(*d);
    }
    let mut h = StateHasher::new();
    h.write_u64(police.state_hash());
    for r in &racers {
        h.write_f32_q(r.body.body.pos[0], 1e3);
        h.write_f32_q(r.body.body.pos[1], 1e3);
    }
    nums.hash = h.finish();
    nums
}

fn mean(v: &[f32]) -> Option<f32> {
    (!v.is_empty()).then(|| v.iter().sum::<f32>() / v.len() as f32)
}

/// Run a scenario with the built-in tables.
pub fn run_scenario(sc: &PoliceScenario) -> PoliceRunSummary {
    run_scenario_with(sc, &PoliceConfig::builtin())
}

/// Run a scenario with explicit tables.
pub fn run_scenario_with(sc: &PoliceScenario, cfg: &PoliceConfig) -> PoliceRunSummary {
    run_scenario_with_hook(sc, cfg, &mut |_, _| {})
}

/// Run a scenario with explicit tables, calling `hook` after every police step with the
/// system and the events it emitted that tick (e.g. to print the radio log).
pub fn run_scenario_with_hook(
    sc: &PoliceScenario,
    cfg: &PoliceConfig,
    hook: &mut dyn FnMut(&PoliceSystem, &[SimEvent]),
) -> PoliceRunSummary {
    let runs: Vec<RunNumbers> = (0..sc.runs.max(1))
        .map(|k| run_once(sc, sc.seed + k as u64, cfg, hook))
        .collect();
    let never = sc.duration_s + 1.0;
    let mut m = BTreeMap::new();
    let starts: Vec<f32> = runs
        .iter()
        .map(|r| r.pursuit_start_s.unwrap_or(never))
        .collect();
    m.insert(
        "pursuit_start_s".to_string(),
        mean(&starts).unwrap_or(never),
    );
    m.insert(
        "pursuit_start_max_s".to_string(),
        starts.iter().copied().fold(0.0, f32::max),
    );
    m.insert(
        "pursuit_start_min_s".to_string(),
        starts.iter().copied().fold(f32::INFINITY, f32::min),
    );
    let ends: Vec<f32> = runs.iter().filter_map(|r| r.pursuit_end_s).collect();
    m.insert("pursuit_end_s".to_string(), mean(&ends).unwrap_or(never));
    let delays: Vec<f32> = runs
        .iter()
        .flat_map(|r| r.reinforcement_delays.iter().copied())
        .collect();
    m.insert(
        "reinforcement_delay_s".to_string(),
        mean(&delays).unwrap_or(-1.0),
    );
    m.insert("reinforcements".to_string(), delays.len() as f32);
    let sum = |f: &dyn Fn(&RunNumbers) -> u32| runs.iter().map(f).sum::<u32>() as f32;
    m.insert("escapes".to_string(), sum(&|r| r.escapes));
    m.insert("safehouse_escapes".to_string(), sum(&|r| r.safehouse));
    m.insert("captures".to_string(), sum(&|r| r.captures));
    m.insert(
        "total_loss_captures".to_string(),
        sum(&|r| r.total_loss_captures),
    );
    m.insert("pursuits".to_string(), sum(&|r| r.pursuits));
    let engaged = sum(&|r| r.units_engaged);
    let selfw = sum(&|r| r.self_wrecks);
    m.insert("units_engaged".to_string(), engaged);
    m.insert("self_wrecks".to_string(), selfw);
    m.insert("contact_wrecks".to_string(), sum(&|r| r.contact_wrecks));
    m.insert("pulled_over".to_string(), sum(&|r| r.pulled_over));
    m.insert(
        "self_wreck_rate".to_string(),
        if engaged > 0.0 { selfw / engaged } else { 0.0 },
    );
    let sights: Vec<f32> = runs
        .iter()
        .flat_map(|r| r.tool_sight.iter().copied())
        .collect();
    m.insert("tools_placed".to_string(), sights.len() as f32);
    m.insert(
        "min_tool_sight_m".to_string(),
        sights.iter().copied().fold(9999.0, f32::min),
    );
    m.insert("spike_hits".to_string(), sum(&|r| r.spike_hits));
    m.insert("helo_launches".to_string(), sum(&|r| r.helo_launches));
    m.insert("near_misses".to_string(), sum(&|r| r.near_misses));
    m.insert(
        "traffic_collisions".to_string(),
        sum(&|r| r.traffic_collisions),
    );
    m.insert("run_wide_crashes".to_string(), sum(&|r| r.run_wide));
    m.insert(
        "max_wanted_level".to_string(),
        runs.iter().map(|r| r.max_level).max().unwrap_or(0) as f32,
    );
    // Radio rates, pooled over runs (lines per minute of sim time).
    let per_min = |lines: u32, secs: f32| {
        if secs > 0.0 {
            lines as f32 * 60.0 / secs
        } else {
            0.0
        }
    };
    let calm_s: f32 = runs.iter().map(|r| r.calm_s).sum();
    let hot_s: f32 = runs.iter().map(|r| r.hot_s).sum();
    m.insert(
        "radio_per_min_calm".to_string(),
        per_min(sum(&|r| r.radio_calm) as u32, calm_s),
    );
    m.insert(
        "radio_per_min_pursuit".to_string(),
        per_min(sum(&|r| r.radio_hot) as u32, hot_s),
    );
    m.insert("calm_s".to_string(), calm_s);
    m.insert("pursuit_s".to_string(), hot_s);
    let last = runs.last().expect("at least one run");
    m.insert("patrols_core".to_string(), last.pool[0] as f32);
    m.insert("patrols_suburb".to_string(), last.pool[1] as f32);
    m.insert("patrols_industrial".to_string(), last.pool[2] as f32);
    let mut h = StateHasher::new();
    for r in &runs {
        h.write_u64(r.hash);
    }
    let mut summary = PoliceRunSummary {
        name: sc.name.clone(),
        state_hash: h.finish(),
        metrics: m,
        failures: Vec::new(),
    };
    for e in &sc.expect {
        match summary.metric(&e.metric) {
            Some(v) if v >= e.min && v <= e.max => {}
            Some(v) => summary
                .failures
                .push(format!("{} = {v} not in [{}, {}]", e.metric, e.min, e.max)),
            None => summary
                .failures
                .push(format!("unknown metric {}", e.metric)),
        }
    }
    summary
}
